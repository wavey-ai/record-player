//! Allocation-free interpolation for variable-speed groove playback.
//!
//! Catmull-Rom is responsive at cueing speeds, but aliases badly when one output
//! frame crosses several source frames. Above the transition speed we therefore
//! use a compact Blackman-windowed sinc whose cutoff follows the actual
//! source-frame step. The blend avoids a tonal jump around normal playback.

const MIN_SINC_RADIUS: isize = 12;
const SINC_RADIUS_OUTPUT_FRAMES: f64 = 8.0;
const MAX_SINC_RADIUS: isize = 256;
const RENDERED_CUTOFF: f64 = 0.44;
const BANDLIMIT_BLEND_START: f64 = 1.05;
const BANDLIMIT_BLEND_END: f64 = 1.5;

fn clamped_sample(channel: &[f32], index: isize) -> f64 {
    channel[index.clamp(0, channel.len().saturating_sub(1) as isize) as usize] as f64
}

#[cfg(test)]
fn sinc(value: f64) -> f64 {
    if value.abs() < 1e-9 {
        1.0
    } else {
        let phase = std::f64::consts::PI * value;
        phase.sin() / phase
    }
}

fn cubic(channel: &[f32], position: f64) -> f64 {
    let index = position.floor() as isize;
    let t = position - index as f64;
    let p0 = clamped_sample(channel, index - 1);
    let p1 = clamped_sample(channel, index);
    let p2 = clamped_sample(channel, index + 1);
    let p3 = clamped_sample(channel, index + 2);
    let a = p2 - p0;
    let b = 2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3;
    let c = 3.0 * (p1 - p2) + p3 - p0;
    p1 + 0.5 * t * (a + t * (b + t * c))
}

fn bandlimited(channel: &[f32], position: f64, source_step: f64) -> f64 {
    // 0.44 reserves a transition band below rendered Nyquist so frequencies
    // that would fold back into the audible band are attenuated in time.
    let speed = source_step.abs().max(1.0);
    let cutoff = (RENDERED_CUTOFF / speed).min(RENDERED_CUTOFF);
    // Keep a constant number of sinc lobes in the rendered/output domain. A
    // fixed source-domain radius collapses to only 1.5 lobes at 8x and causes
    // both audible passband droop and near-Nyquist alias leakage.
    let radius =
        (SINC_RADIUS_OUTPUT_FRAMES * speed).clamp(MIN_SINC_RADIUS as f64, MAX_SINC_RADIUS as f64);
    let tap_radius = radius.ceil() as isize;
    let center = position.floor() as isize;
    let first_tap = center - tap_radius + 1;
    let mut distance = position - first_tap as f64;
    let kernel_step = std::f64::consts::TAU * cutoff;
    let (kernel_step_sin, kernel_step_cos) = kernel_step.sin_cos();
    let (mut kernel_sin, mut kernel_cos) = (kernel_step * distance).sin_cos();
    let window_step = std::f64::consts::PI / radius;
    let (window_step_sin, window_step_cos) = window_step.sin_cos();
    let (mut window_sin, mut window_cos) = (window_step * distance).sin_cos();
    let mut weighted = 0.0;
    let mut weight_sum = 0.0;

    for tap in first_tap..=(center + tap_radius) {
        let normalized_distance = (distance / radius).abs();
        if normalized_distance < 1.0 {
            let double_window_cos = 2.0 * window_cos * window_cos - 1.0;
            let window = 0.42 + 0.5 * window_cos + 0.08 * double_window_cos;
            let lowpass = if distance.abs() < 1e-9 {
                2.0 * cutoff
            } else {
                kernel_sin / (std::f64::consts::PI * distance)
            };
            let weight = lowpass * window;
            weighted += clamped_sample(channel, tap) * weight;
            weight_sum += weight;
        }

        let next_kernel_sin = kernel_sin * kernel_step_cos - kernel_cos * kernel_step_sin;
        let next_kernel_cos = kernel_cos * kernel_step_cos + kernel_sin * kernel_step_sin;
        kernel_sin = next_kernel_sin;
        kernel_cos = next_kernel_cos;
        let next_window_sin = window_sin * window_step_cos - window_cos * window_step_sin;
        let next_window_cos = window_cos * window_step_cos + window_sin * window_step_sin;
        window_sin = next_window_sin;
        window_cos = next_window_cos;
        distance -= 1.0;
    }

    if weight_sum.abs() > 1e-12 {
        weighted / weight_sum
    } else {
        cubic(channel, position)
    }
}

pub(crate) fn adaptive_sample(channel: &[f32], position: f64, source_step: f64) -> Option<f64> {
    if channel.len() < 2 || !position.is_finite() || !source_step.is_finite() {
        return None;
    }
    if position < 0.0 || position >= channel.len().saturating_sub(1) as f64 {
        return None;
    }

    let speed = source_step.abs();
    let cubic_sample = cubic(channel, position);
    if speed <= BANDLIMIT_BLEND_START {
        return Some(cubic_sample);
    }
    let sinc_sample = bandlimited(channel, position, speed);
    if speed >= BANDLIMIT_BLEND_END {
        return Some(sinc_sample);
    }
    let blend = (speed - BANDLIMIT_BLEND_START) / (BANDLIMIT_BLEND_END - BANDLIMIT_BLEND_START);
    Some(cubic_sample + (sinc_sample - cubic_sample) * blend)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frames: usize, cycles_per_frame: f64) -> Vec<f32> {
        (0..frames)
            .map(|frame| (std::f64::consts::TAU * cycles_per_frame * frame as f64).sin() as f32)
            .collect()
    }

    fn rendered_rms(channel: &[f32], source_step: f64) -> f64 {
        let mut position = if source_step >= 0.0 {
            64.25
        } else {
            channel.len() as f64 - 64.25
        };
        let mut sum = 0.0;
        let mut count = 0;
        while position >= 64.0 && position < channel.len() as f64 - 64.0 {
            let sample = adaptive_sample(channel, position, source_step).unwrap();
            sum += sample * sample;
            count += 1;
            position += source_step;
        }
        (sum / count as f64).sqrt()
    }

    fn bandlimited_reference(channel: &[f32], position: f64, source_step: f64) -> f64 {
        let speed = source_step.abs().max(1.0);
        let cutoff = (RENDERED_CUTOFF / speed).min(RENDERED_CUTOFF);
        let radius = (SINC_RADIUS_OUTPUT_FRAMES * speed)
            .clamp(MIN_SINC_RADIUS as f64, MAX_SINC_RADIUS as f64);
        let tap_radius = radius.ceil() as isize;
        let center = position.floor() as isize;
        let mut weighted = 0.0;
        let mut weight_sum = 0.0;
        for tap in (center - tap_radius + 1)..=(center + tap_radius) {
            let distance = position - tap as f64;
            let normalized_distance = (distance / radius).abs();
            if normalized_distance >= 1.0 {
                continue;
            }
            let window = 0.42
                + 0.5 * (std::f64::consts::PI * normalized_distance).cos()
                + 0.08 * (std::f64::consts::TAU * normalized_distance).cos();
            let weight = 2.0 * cutoff * sinc(2.0 * cutoff * distance) * window;
            weighted += clamped_sample(channel, tap) * weight;
            weight_sum += weight;
        }
        weighted / weight_sum
    }

    #[test]
    fn normal_speed_retains_the_low_latency_cubic_path() {
        let channel = sine(512, 0.08);
        let position = 127.375;
        assert!(
            (adaptive_sample(&channel, position, 1.0).unwrap() - cubic(&channel, position)).abs()
                < 1e-12
        );
    }

    #[test]
    fn recurrence_kernel_matches_the_direct_sinc_reference() {
        let channel = (0..4_096)
            .map(|frame| {
                let first = (std::f64::consts::TAU * 0.031 * frame as f64).sin();
                let second = (std::f64::consts::TAU * 0.173 * frame as f64).sin();
                (first * 0.73 + second * 0.27) as f32
            })
            .collect::<Vec<_>>();
        for source_step in [1.5, 2.0, 4.0, 8.0, 10.0] {
            for position in [512.125, 1_024.5, 2_047.875] {
                let optimized = bandlimited(&channel, position, source_step);
                let reference = bandlimited_reference(&channel, position, source_step);
                assert!(
                    (optimized - reference).abs() < 1e-11,
                    "{source_step}x at {position}: {optimized} != {reference}"
                );
            }
        }
    }

    #[test]
    fn adaptive_filter_changes_continuously_across_blend_and_support_boundaries() {
        let channel = (0..4_096)
            .map(|frame| {
                let first = (std::f64::consts::TAU * 0.043 * frame as f64).sin();
                let second = (std::f64::consts::TAU * 0.211 * frame as f64).sin();
                (first * 0.61 + second * 0.39) as f32
            })
            .collect::<Vec<_>>();
        let epsilon = 1e-7;
        let boundaries = [BANDLIMIT_BLEND_START, BANDLIMIT_BLEND_END]
            .into_iter()
            .chain((13..=80).map(|radius| radius as f64 / SINC_RADIUS_OUTPUT_FRAMES));
        for source_step in boundaries {
            let before = adaptive_sample(&channel, 2_047.375, source_step - epsilon).unwrap();
            let after = adaptive_sample(&channel, 2_047.375, source_step + epsilon).unwrap();
            assert!(
                (after - before).abs() < 1e-6,
                "filter jumped from {before} to {after} around {source_step}x"
            );
        }
    }

    #[test]
    fn high_speed_filter_rejects_a_stopband_sweep_in_both_directions() {
        for source_step in [2.0_f64, 4.0, 8.0, -2.0, -4.0, -8.0] {
            for cycles_per_output_frame in [0.58, 0.72, 0.88] {
                let cycles_per_source_frame = cycles_per_output_frame / source_step.abs();
                let above_rendered_nyquist = sine(16_384, cycles_per_source_frame);
                let rms = rendered_rms(&above_rendered_nyquist, source_step);
                assert!(
                    rms < 0.003,
                    "{source_step}x at {cycles_per_output_frame} cycles/output-frame \
                     leaked {rms} RMS"
                );
            }
        }
    }

    #[test]
    fn high_speed_filter_preserves_a_passband_sweep_in_both_directions() {
        for source_step in [2.0_f64, 4.0, 8.0, -2.0, -4.0, -8.0] {
            for cycles_per_output_frame in [0.04, 0.12, 0.24, 0.32] {
                let cycles_per_source_frame = cycles_per_output_frame / source_step.abs();
                let passband = sine(16_384, cycles_per_source_frame);
                let rms = rendered_rms(&passband, source_step);
                assert!(
                    (rms - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.012,
                    "{source_step}x at {cycles_per_output_frame} cycles/output-frame \
                     produced {rms} RMS"
                );
            }
        }
    }

    #[test]
    fn interpolation_is_direction_symmetric_at_every_high_speed_tier() {
        let channel = sine(2_048, 0.07);
        for source_step in [2.0, 4.0, 8.0] {
            for position in [128.125, 511.5, 1_024.875] {
                let forward = adaptive_sample(&channel, position, source_step).unwrap();
                let reverse = adaptive_sample(&channel, position, -source_step).unwrap();
                assert!((forward - reverse).abs() < 1e-12);
            }
        }
    }
}
