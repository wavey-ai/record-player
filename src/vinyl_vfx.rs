//! Geometry-driven vinyl VFX shared by live playback and offline REMIX.
//!
//! The processors in this module use record coordinates rather than wall
//! clock time. A scene therefore keeps its phase when the platter slows,
//! reverses, or moves under a hand.
//!
//! The polar memory stores one revolution of audio indexed by angle at
//! full fidelity: 2^17 bins per turn oversamples a 33 1/3 RPM revolution
//! at 48 kHz (86,400 frames) by ~1.5x, writes fill every bin the stylus
//! crosses, and reads interpolate between bins. Each bin is tagged with
//! the unwrapped turn time it was written, so a read only returns audio
//! the stylus actually cut on the pass being asked for.

use std::f64::consts::TAU;

pub const VINYL_VFX_NONE: u32 = 0;
pub const VINYL_VFX_ADJACENT_GHOST: u32 = 1;
pub const VINYL_VFX_THREE_NEEDLES: u32 = 2;
pub const VINYL_VFX_CUT_CONSTELLATION: u32 = 3;
pub const VINYL_VFX_INNER_FIRE: u32 = 4;
pub const VINYL_VFX_SPLIT_WALLS: u32 = 5;
pub const VINYL_VFX_WORN_HALO: u32 = 6;
pub const VINYL_VFX_MAX_SCENE: u32 = VINYL_VFX_WORN_HALO;

const POLAR_BINS: usize = 1 << 17;
const WEAR_BINS: usize = 2_048;
/// A bin tag may differ from the requested time by a couple of bins of
/// rounding before the content is someone else's revolution.
const POLAR_TAG_TOLERANCE: f64 = 2.5 / POLAR_BINS as f64;
const CONSTELLATION_SECTORS: u32 = 12;
const CONSTELLATION_DEFAULT_PATTERN: u32 = 0b1011_0100_1101;

#[derive(Clone, Copy, Debug)]
pub struct VinylVfxContext {
    pub sample_rate: f64,
    pub rpm: f64,
    pub start_turns: f64,
    pub end_turns: f64,
    pub start_position: f64,
    pub end_position: f64,
    pub total_frames: usize,
    pub pressing_seed: u32,
}

impl Default for VinylVfxContext {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            rpm: 33.333_333,
            start_turns: 0.0,
            end_turns: 0.0,
            start_position: 0.0,
            end_position: 0.0,
            total_frames: 0,
            pressing_seed: 0,
        }
    }
}

/// Preallocated state for one graph instance.
///
/// Live players keep one instance beside the acoustic DSP. Offline buses use
/// the same type, so a scene's block order and state transitions are shared.
pub struct VinylVfxProcessor {
    scene: u32,
    amount: f64,
    polar_samples: Vec<[f32; 2]>,
    polar_written: Vec<f64>,
    last_write: Option<(f64, [f32; 2])>,
    wear: Vec<f32>,
    lowpass: [f64; 2],
    gate_gain: f64,
}

impl Default for VinylVfxProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl VinylVfxProcessor {
    pub fn new() -> Self {
        Self {
            scene: VINYL_VFX_NONE,
            amount: 1.0,
            polar_samples: vec![[0.0; 2]; POLAR_BINS],
            polar_written: vec![f64::NEG_INFINITY; POLAR_BINS],
            last_write: None,
            wear: vec![0.0; WEAR_BINS],
            lowpass: [0.0; 2],
            gate_gain: 1.0,
        }
    }

    pub fn set_scene(&mut self, scene: u32, amount: f64) {
        let scene = scene.min(VINYL_VFX_MAX_SCENE);
        let amount = finite_or(amount, 1.0).clamp(0.0, 1.0);
        if self.scene != scene {
            self.scene = scene;
            self.reset_transient_state();
        }
        self.amount = amount;
    }

    pub fn scene(&self) -> u32 {
        self.scene
    }

    pub fn amount(&self) -> f64 {
        self.amount
    }

    pub fn reset_transient_state(&mut self) {
        self.polar_samples.fill([0.0; 2]);
        self.polar_written.fill(f64::NEG_INFINITY);
        self.last_write = None;
        self.lowpass = [0.0; 2];
        self.gate_gain = 1.0;
    }

    pub fn process_interleaved(
        &mut self,
        samples: &mut [f32],
        channel_count: usize,
        context: VinylVfxContext,
    ) {
        if self.scene == VINYL_VFX_NONE
            || self.amount <= f64::EPSILON
            || !(1..=2).contains(&channel_count)
        {
            return;
        }
        let frame_count = samples.len() / channel_count;
        if frame_count == 0 {
            return;
        }
        for frame_index in 0..frame_count {
            let offset = frame_index * channel_count;
            let mut frame = [samples[offset], samples[offset]];
            if channel_count == 2 {
                frame[1] = samples[offset + 1];
            }
            self.process_frame(&mut frame, channel_count, frame_index, frame_count, context);
            samples[offset] = frame[0];
            if channel_count == 2 {
                samples[offset + 1] = frame[1];
            }
        }
    }

    pub fn process_planar(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        context: VinylVfxContext,
    ) {
        if self.scene == VINYL_VFX_NONE || self.amount <= f64::EPSILON {
            return;
        }
        let frame_count = left.len().min(right.len());
        for frame_index in 0..frame_count {
            let mut frame = [left[frame_index], right[frame_index]];
            self.process_frame(&mut frame, 2, frame_index, frame_count, context);
            left[frame_index] = frame[0];
            right[frame_index] = frame[1];
        }
    }

    fn process_frame(
        &mut self,
        frame: &mut [f32; 2],
        channel_count: usize,
        frame_index: usize,
        frame_count: usize,
        context: VinylVfxContext,
    ) {
        let progress = if frame_count > 1 {
            frame_index as f64 / (frame_count - 1) as f64
        } else {
            0.0
        };
        let turns = interpolate(context.start_turns, context.end_turns, progress);
        let position = interpolate(context.start_position, context.end_position, progress);
        let total_frames = context.total_frames.max(1) as f64;
        let radius = (position / total_frames).clamp(0.0, 1.0);
        let signed_motion = context.end_turns - context.start_turns;
        let direction = if signed_motion < 0.0 { -1.0 } else { 1.0 };
        let dry = *frame;

        match self.scene {
            VINYL_VFX_ADJACENT_GHOST => {
                let previous_turn = self.read_polar(turns - direction, channel_count);
                let transfer = self.amount * 0.72;
                for channel in 0..channel_count {
                    self.lowpass[channel] +=
                        (f64::from(previous_turn[channel]) - self.lowpass[channel]) * 0.22;
                    frame[channel] =
                        soft_limit(f64::from(dry[channel]) + self.lowpass[channel] * transfer);
                }
            }
            VINYL_VFX_THREE_NEEDLES => {
                let second = self.read_polar(turns - direction / 3.0, channel_count);
                let third = self.read_polar(turns - direction * 2.0 / 3.0, channel_count);
                let level = self.amount * 0.52;
                for channel in 0..channel_count {
                    let head_mix =
                        f64::from(second[channel]) * 0.58 + f64::from(third[channel]) * 0.42;
                    frame[channel] = soft_limit(f64::from(dry[channel]) + head_mix * level);
                }
            }
            VINYL_VFX_CUT_CONSTELLATION => {
                let phase = turns.rem_euclid(1.0);
                let sector = ((phase * f64::from(CONSTELLATION_SECTORS)).floor() as u32)
                    .min(CONSTELLATION_SECTORS - 1);
                let pattern = constellation_pattern(context.pressing_seed);
                let open = pattern & (1 << sector) != 0;
                let target = if open { 1.0 } else { 1.0 - self.amount * 0.96 };
                let sample_rate = finite_or(context.sample_rate, 48_000.0).max(1.0);
                let alpha = 1.0 - (-1.0 / (sample_rate * 0.0018)).exp();
                self.gate_gain += (target - self.gate_gain) * alpha;
                for sample in frame.iter_mut().take(channel_count) {
                    *sample = (f64::from(*sample) * self.gate_gain) as f32;
                }
            }
            VINYL_VFX_INNER_FIRE => {
                let heat = self.amount * (0.35 + 0.65 * radius.powf(1.35));
                let sample_rate = finite_or(context.sample_rate, 48_000.0).max(1.0);
                let cutoff = 18_000.0 - heat * 11_000.0;
                let alpha = 1.0 - (-TAU * cutoff / sample_rate).exp();
                for channel in 0..channel_count {
                    let driven = (f64::from(dry[channel]) * (1.0 + heat * 1.8)).tanh();
                    self.lowpass[channel] += (driven - self.lowpass[channel]) * alpha;
                    frame[channel] = soft_limit(self.lowpass[channel] * (1.0 - heat * 0.22));
                }
                if channel_count == 2 {
                    narrow_stereo(frame, heat * 0.6);
                }
            }
            VINYL_VFX_SPLIT_WALLS => {
                let left_head = self.read_polar(turns - direction * 0.25, channel_count);
                let right_head = self.read_polar(turns - direction * 0.625, channel_count);
                let wall = self.amount * 0.68;
                if channel_count == 2 {
                    let lateral = (f64::from(dry[0]) + f64::from(dry[1])) * 0.5;
                    let vertical = (f64::from(dry[0]) - f64::from(dry[1])) * 0.5;
                    let delayed_lateral = (f64::from(left_head[0]) + f64::from(left_head[1])) * 0.5;
                    let delayed_vertical =
                        (f64::from(right_head[0]) - f64::from(right_head[1])) * 0.5;
                    frame[0] = soft_limit(lateral + vertical + delayed_lateral * wall);
                    frame[1] = soft_limit(lateral - vertical - delayed_vertical * wall);
                } else {
                    frame[0] = soft_limit(
                        f64::from(dry[0])
                            + (f64::from(left_head[0]) - f64::from(right_head[0])) * wall,
                    );
                }
            }
            VINYL_VFX_WORN_HALO => {
                let wear_index = wear_bin(turns);
                let sample_rate = finite_or(context.sample_rate, 48_000.0).max(1.0);
                let increment = self.amount / (sample_rate * 20.0);
                self.wear[wear_index] =
                    (f64::from(self.wear[wear_index]) + increment).clamp(0.0, 1.0) as f32;
                let worn = f64::from(self.wear[wear_index]).sqrt() * self.amount;
                let cutoff = 19_000.0 - worn * 11_000.0;
                let alpha = 1.0 - (-TAU * cutoff / sample_rate).exp();
                let crackle = deterministic_crackle(
                    wear_index as u64,
                    turns.floor() as i64,
                    context.pressing_seed,
                    worn,
                );
                for channel in 0..channel_count {
                    self.lowpass[channel] +=
                        (f64::from(dry[channel]) - self.lowpass[channel]) * alpha;
                    frame[channel] = soft_limit(self.lowpass[channel] + crackle);
                }
            }
            _ => {}
        }

        self.write_polar(turns, dry);
    }

    /// Interpolated read at an unwrapped turn coordinate. Bins whose write
    /// tag disagrees with the requested time contribute silence rather than
    /// another revolution's audio.
    fn read_polar(&self, turns: f64, channel_count: usize) -> [f32; 2] {
        let turns = finite_or(turns, 0.0);
        let bin_position = turns * POLAR_BINS as f64;
        let base = bin_position.floor();
        let fraction = bin_position - base;
        let mut value = [0.0_f64; 2];
        for (step, weight) in [(0.0, 1.0 - fraction), (1.0, fraction)] {
            let unwrapped = base + step;
            let index = wrap_bin(unwrapped);
            let expected = unwrapped / POLAR_BINS as f64;
            if (self.polar_written[index] - expected).abs() <= POLAR_TAG_TOLERANCE {
                value[0] += f64::from(self.polar_samples[index][0]) * weight;
                value[1] += f64::from(self.polar_samples[index][1]) * weight;
            }
        }
        let mut frame = [value[0] as f32, value[1] as f32];
        if channel_count == 1 {
            frame[1] = frame[0];
        }
        frame
    }

    /// Records the stylus pass, filling every bin crossed since the last
    /// write with linearly interpolated audio so the memory has no holes at
    /// any platter speed, in either direction.
    fn write_polar(&mut self, turns: f64, frame: [f32; 2]) {
        let turns = finite_or(turns, 0.0);
        let bin_position = turns * POLAR_BINS as f64;
        if let Some((previous_turns, previous_frame)) = self.last_write {
            let previous_position = previous_turns * POLAR_BINS as f64;
            let span = bin_position - previous_position;
            // A jump wider than a quarter turn is a seek, not a pass.
            if span != 0.0 && span.abs() <= POLAR_BINS as f64 * 0.25 {
                let low = previous_position.min(bin_position);
                let high = previous_position.max(bin_position);
                let mut bin = low.ceil();
                while bin <= high {
                    let t = ((bin - previous_position) / span).clamp(0.0, 1.0);
                    let index = wrap_bin(bin);
                    self.polar_samples[index] = [
                        lerp(previous_frame[0], frame[0], t),
                        lerp(previous_frame[1], frame[1], t),
                    ];
                    self.polar_written[index] = bin / POLAR_BINS as f64;
                    bin += 1.0;
                }
                self.last_write = Some((turns, frame));
                return;
            }
        }
        let index = wrap_bin(bin_position.floor());
        self.polar_samples[index] = frame;
        self.polar_written[index] = bin_position.floor() / POLAR_BINS as f64;
        self.last_write = Some((turns, frame));
    }
}

fn wrap_bin(unwrapped: f64) -> usize {
    (unwrapped.rem_euclid(POLAR_BINS as f64) as usize).min(POLAR_BINS - 1)
}

fn wear_bin(turns: f64) -> usize {
    let phase = finite_or(turns, 0.0).rem_euclid(1.0);
    ((phase * WEAR_BINS as f64).floor() as usize).min(WEAR_BINS - 1)
}

fn lerp(start: f32, end: f32, t: f64) -> f32 {
    (f64::from(start) + (f64::from(end) - f64::from(start)) * t) as f32
}

/// The sector map is a property of the pressing: seed zero keeps the house
/// cut, any other pressing carves its own — always with at least three open
/// and three closed sectors so the gate stays a rhythm, not a mute.
fn constellation_pattern(seed: u32) -> u32 {
    if seed == 0 {
        return CONSTELLATION_DEFAULT_PATTERN;
    }
    let mut hash = u64::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for _ in 0..8 {
        hash ^= hash >> 30;
        hash = hash.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        hash ^= hash >> 27;
        let pattern = (hash as u32) & 0xFFF;
        let open = pattern.count_ones();
        if (3..=9).contains(&open) {
            return pattern;
        }
        hash = hash.wrapping_add(0x9E37_79B9);
    }
    CONSTELLATION_DEFAULT_PATTERN
}

fn interpolate(start: f64, end: f64, progress: f64) -> f64 {
    finite_or(start, 0.0) + (finite_or(end, start) - finite_or(start, 0.0)) * progress
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

fn soft_limit(value: f64) -> f32 {
    value.clamp(-1.0, 1.0) as f32
}

fn narrow_stereo(frame: &mut [f32; 2], amount: f64) {
    let amount = amount.clamp(0.0, 1.0);
    let mid = (f64::from(frame[0]) + f64::from(frame[1])) * 0.5;
    let side = (f64::from(frame[0]) - f64::from(frame[1])) * 0.5 * (1.0 - amount);
    frame[0] = soft_limit(mid + side);
    frame[1] = soft_limit(mid - side);
}

fn deterministic_crackle(index: u64, turn: i64, seed: u32, worn: f64) -> f64 {
    if worn <= f64::EPSILON {
        return 0.0;
    }
    let mut hash = index
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(turn as u64)
        .wrapping_add(u64::from(seed));
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    hash ^= hash >> 27;
    let chance = hash & 0x7ff;
    if chance >= (worn * 22.0) as u64 {
        return 0.0;
    }
    let bipolar = ((hash >> 16) & 0xffff) as f64 / 32_767.5 - 1.0;
    bipolar * worn * 0.09
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(turns: f64, frames: usize) -> VinylVfxContext {
        VinylVfxContext {
            start_turns: turns,
            end_turns: turns + frames as f64 / 4_800.0,
            end_position: frames as f64,
            total_frames: frames * 4,
            ..VinylVfxContext::default()
        }
    }

    #[test]
    fn bypass_is_bit_exact() {
        let mut processor = VinylVfxProcessor::new();
        let mut samples = vec![0.25_f32, -0.5, 0.75, -0.125];
        let expected = samples.clone();
        processor.process_interleaved(&mut samples, 2, context(0.0, 2));
        assert_eq!(samples, expected);
    }

    #[test]
    fn constellation_is_painted_on_turns_not_clock_time() {
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_CUT_CONSTELLATION, 1.0);
        let mut first = vec![0.5_f32; 9_600];
        processor.process_interleaved(&mut first, 2, context(0.0, 4_800));
        assert!(first.iter().any(|sample| sample.abs() < 0.2));
        assert!(first.iter().any(|sample| sample.abs() > 0.45));
    }

    #[test]
    fn constellation_repeats_the_same_cut_every_revolution() {
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_CUT_CONSTELLATION, 1.0);
        let mut first = vec![0.5_f32; 9_600];
        processor.process_interleaved(&mut first, 2, context(0.0, 4_800));
        let mut second = vec![0.5_f32; 9_600];
        processor.process_interleaved(&mut second, 2, context(1.0, 4_800));
        let mut third = vec![0.5_f32; 9_600];
        processor.process_interleaved(&mut third, 2, context(2.0, 4_800));
        // Once the gate slew has settled, revolution three must retrace
        // revolution two sample for sample.
        let drift = second
            .iter()
            .zip(third.iter())
            .skip(1_000)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(drift < 0.01, "cut drifted between revolutions: {drift}");
    }

    #[test]
    fn constellation_cut_belongs_to_the_pressing() {
        let mut house = VinylVfxProcessor::new();
        house.set_scene(VINYL_VFX_CUT_CONSTELLATION, 1.0);
        let mut pressed = VinylVfxProcessor::new();
        pressed.set_scene(VINYL_VFX_CUT_CONSTELLATION, 1.0);
        let mut house_output = vec![0.5_f32; 9_600];
        house.process_interleaved(&mut house_output, 2, context(0.0, 4_800));
        let mut pressed_output = vec![0.5_f32; 9_600];
        let seeded = VinylVfxContext {
            pressing_seed: 0xB17_5EED,
            ..context(0.0, 4_800)
        };
        pressed.process_interleaved(&mut pressed_output, 2, seeded);
        assert!(house_output
            .iter()
            .zip(pressed_output.iter())
            .any(|(a, b)| (a - b).abs() > 0.1));
    }

    #[test]
    fn adjacent_ghost_reads_the_previous_revolution() {
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_ADJACENT_GHOST, 1.0);
        let mut first = vec![0.4_f32; 9_600];
        processor.process_interleaved(&mut first, 2, context(0.0, 4_800));
        let mut second = vec![0.0_f32; 9_600];
        processor.process_interleaved(&mut second, 2, context(1.0, 4_800));
        assert!(second.iter().any(|sample| sample.abs() > 0.05));
    }

    #[test]
    fn ghost_echo_is_smooth_not_stair_stepped() {
        // A sine cut on revolution one must come back on revolution two as
        // a smooth (filtered) wave. The old 4,096-bin memory returned a
        // zero-order-hold staircase whose sample-to-sample jumps rival the
        // signal itself.
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_ADJACENT_GHOST, 1.0);
        let frames = 4_800_usize;
        let mut first: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let value = (i as f64 / frames as f64 * TAU * 96.0).sin() as f32 * 0.6;
                [value, value]
            })
            .collect();
        processor.process_interleaved(&mut first, 2, context(0.0, frames));
        let mut second = vec![0.0_f32; frames * 2];
        processor.process_interleaved(&mut second, 2, context(1.0, frames));
        let ghost: Vec<f32> = second.chunks_exact(2).map(|f| f[0]).collect();
        let peak = ghost.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
        assert!(peak > 0.05, "no ghost came back: {peak}");
        let max_jump = ghost
            .windows(2)
            .skip(200)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);
        // 96 cycles per 4,800-frame turn moves at most ~7.5% of the peak
        // per sample; allow filter transients double that.
        assert!(
            max_jump < peak * 0.15,
            "ghost is stair-stepped: jump {max_jump} against peak {peak}"
        );
    }

    #[test]
    fn split_walls_keeps_stereo_channels_distinct() {
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_SPLIT_WALLS, 1.0);
        let mut warmup = Vec::with_capacity(9_600);
        for _ in 0..4_800 {
            warmup.extend_from_slice(&[0.7, -0.2]);
        }
        processor.process_interleaved(&mut warmup, 2, context(0.0, 4_800));
        let mut output = warmup.clone();
        processor.process_interleaved(&mut output, 2, context(1.0, 4_800));
        assert!(output.chunks_exact(2).any(|frame| frame[0] != frame[1]));
    }

    #[test]
    fn inner_fire_is_audible_but_not_blown_out() {
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_INNER_FIRE, 1.0);
        let frames = 4_800_usize;
        let mut samples: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let value = (i as f64 / frames as f64 * TAU * 48.0).sin() as f32 * 0.5;
                [value, value]
            })
            .collect();
        let dry = samples.clone();
        processor.process_interleaved(&mut samples, 2, context(0.0, frames));
        let wet_peak = samples.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
        let dry_peak = dry.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
        assert!(
            samples
                .iter()
                .zip(dry.iter())
                .any(|(a, b)| (a - b).abs() > 0.02),
            "fire did nothing"
        );
        assert!(
            wet_peak < dry_peak * 1.6,
            "fire blew out: {wet_peak} against dry {dry_peak}"
        );
    }
}
