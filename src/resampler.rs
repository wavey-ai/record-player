//! Allocation-free interpolation for variable-speed groove playback.
//!
//! Catmull-Rom is responsive at cueing speeds, but aliases badly when one output
//! frame crosses several source frames. Above the transition speed we therefore
//! use a compact Blackman-windowed sinc whose cutoff follows the actual
//! source-frame step. The blend avoids a tonal jump around normal playback.

const SINC_RADIUS: isize = 12;
const BANDLIMIT_BLEND_START: f64 = 1.05;
const BANDLIMIT_BLEND_END: f64 = 1.5;

fn clamped_sample(channel: &[f32], index: isize) -> f64 {
    channel[index.clamp(0, channel.len().saturating_sub(1) as isize) as usize] as f64
}

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
    // 0.47 retains a small transition band below Nyquist at the rendered rate.
    let cutoff = (0.47 / source_step.max(1.0)).min(0.47);
    let center = position.floor() as isize;
    let mut weighted = 0.0;
    let mut weight_sum = 0.0;

    for tap in (center - SINC_RADIUS + 1)..=(center + SINC_RADIUS) {
        let distance = position - tap as f64;
        let normalized_distance = (distance / SINC_RADIUS as f64).abs();
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
        let mut position = 64.25;
        let mut sum = 0.0;
        let mut count = 0;
        while position < channel.len() as f64 - 64.0 {
            let sample = adaptive_sample(channel, position, source_step).unwrap();
            sum += sample * sample;
            count += 1;
            position += source_step;
        }
        (sum / count as f64).sqrt()
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
    fn high_speed_filter_rejects_content_above_rendered_nyquist() {
        let above_nyquist = sine(8_192, 0.40);
        assert!(rendered_rms(&above_nyquist, 4.0) < 0.03);
    }

    #[test]
    fn high_speed_filter_preserves_passband_content() {
        let passband = sine(8_192, 0.03);
        let rms = rendered_rms(&passband, 4.0);
        assert!((rms - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.03);
    }

    #[test]
    fn interpolation_is_direction_symmetric() {
        let channel = sine(2_048, 0.07);
        for position in [128.125, 511.5, 1_024.875] {
            let forward = adaptive_sample(&channel, position, 3.2).unwrap();
            let reverse = adaptive_sample(&channel, position, -3.2).unwrap();
            assert!((forward - reverse).abs() < 1e-12);
        }
    }
}
