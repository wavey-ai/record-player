//! Converts fixed-rate physical output to a supported host sample rate.
//!
//! The physical player supplies consecutive stereo frames at 192 kHz.
//! This module uses an exact rational clock for each supported output rate.
//! A polyphase Kaiser-windowed sinc filter removes frequencies that can alias.
//! The filter has a fixed linear-phase delay and does not allocate while processing.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PHYSICAL_OUTPUT_INPUT_RATE_HZ: u32 = 192_000;
pub const SUPPORTED_PHYSICAL_OUTPUT_RATES_HZ: [u32; 6] =
    [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
pub const STEREO_OUTPUT_RESAMPLER_SNAPSHOT_VERSION: u32 = 1;

const MAX_FILTER_TAPS: usize = 641;
const AUDIO_PASSBAND_HZ: f64 = 20_000.0;
const KAISER_BETA_100_DB: f64 = 10.061_26;

/// The largest numeric sample magnitude accepted by the output filter.
pub const MAX_ABS_PHYSICAL_OUTPUT_SAMPLE: f32 = 1.0e30;

/// Returns the physical input count at one exact host-frame boundary.
pub fn physical_input_frames_for_output_frames(
    output_sample_rate_hz: u32,
    output_frames: u64,
) -> Result<u64, StereoOutputResamplerError> {
    let spec = RateSpec::for_output_rate(output_sample_rate_hz).ok_or(
        StereoOutputResamplerError::UnsupportedOutputRate {
            output_sample_rate_hz,
        },
    )?;
    if output_frames == 0 {
        return Ok(0);
    }
    let final_output_index = output_frames - 1;
    let source_numerator =
        u128::from(final_output_index) * u128::from(spec.input_ratio_denominator);
    let final_source_frame = source_numerator / u128::from(spec.output_ratio_numerator);
    let required_total = final_source_frame + 1;
    u64::try_from(required_total).map_err(|_| StereoOutputResamplerError::FrameCounterOverflow)
}

#[derive(Debug, Clone, Copy)]
struct RateSpec {
    output_rate_hz: u32,
    output_ratio_numerator: u32,
    input_ratio_denominator: u32,
    filter_taps: usize,
}

impl RateSpec {
    fn for_output_rate(output_rate_hz: u32) -> Option<Self> {
        let (output_ratio_numerator, input_ratio_denominator, filter_taps) = match output_rate_hz {
            44_100 => (147, 640, 641),
            48_000 => (1, 4, 321),
            88_200 => (147, 320, 65),
            96_000 => (1, 2, 57),
            176_400 => (147, 160, 25),
            192_000 => (1, 1, 1),
            _ => return None,
        };
        Some(Self {
            output_rate_hz,
            output_ratio_numerator,
            input_ratio_denominator,
            filter_taps,
        })
    }

    fn radius(self) -> usize {
        (self.filter_taps - 1) / 2
    }

    fn phase_count(self) -> usize {
        self.output_ratio_numerator as usize
    }

    fn is_identity(self) -> bool {
        self.output_ratio_numerator == self.input_ratio_denominator
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StereoOutputProcess {
    pub input_frames: usize,
    pub output_frames: usize,
}

/// Stores all streaming state that changes future output.
///
/// A snapshot has a fixed size and does not allocate when copied.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StereoOutputResamplerSnapshot {
    pub version: u32,
    pub output_sample_rate_hz: u32,
    pub input_frames_consumed: u64,
    pub output_frames_produced: u64,
    pub next_source_frame: u64,
    pub next_phase: u32,
    #[serde(with = "snapshot_history_serde")]
    history: [[f32; 2]; MAX_FILTER_TAPS],
}

mod snapshot_history_serde {
    use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

    use super::MAX_FILTER_TAPS;

    pub fn serialize<S>(
        history: &[[f32; 2]; MAX_FILTER_TAPS],
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        history.as_slice().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[[f32; 2]; MAX_FILTER_TAPS], D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<[f32; 2]>::deserialize(deserializer)?;
        if values.len() != MAX_FILTER_TAPS {
            return Err(D::Error::invalid_length(values.len(), &"641 stereo frames"));
        }
        let mut history = [[0.0; 2]; MAX_FILTER_TAPS];
        history.copy_from_slice(&values);
        Ok(history)
    }
}

/// Converts consecutive 192 kHz stereo frames to one supported output rate.
///
/// Construction creates the coefficient bank. Processing uses fixed storage.
pub struct StereoOutputResampler {
    spec: RateSpec,
    coefficients: Box<[f64]>,
    history: [[f32; 2]; MAX_FILTER_TAPS],
    input_frames_consumed: u64,
    output_frames_produced: u64,
    next_source_frame: u64,
    next_phase: u32,
}

impl StereoOutputResampler {
    pub fn new(output_sample_rate_hz: u32) -> Result<Self, StereoOutputResamplerError> {
        let spec = RateSpec::for_output_rate(output_sample_rate_hz).ok_or(
            StereoOutputResamplerError::UnsupportedOutputRate {
                output_sample_rate_hz,
            },
        )?;
        Ok(Self {
            spec,
            coefficients: design_coefficients(spec).into_boxed_slice(),
            history: [[0.0; 2]; MAX_FILTER_TAPS],
            input_frames_consumed: 0,
            output_frames_produced: 0,
            next_source_frame: 0,
            next_phase: 0,
        })
    }

    pub fn from_snapshot(
        snapshot: &StereoOutputResamplerSnapshot,
    ) -> Result<Self, StereoOutputResamplerError> {
        let mut resampler = Self::new(snapshot.output_sample_rate_hz)?;
        resampler.restore(snapshot)?;
        Ok(resampler)
    }

    pub const fn input_sample_rate_hz(&self) -> u32 {
        PHYSICAL_OUTPUT_INPUT_RATE_HZ
    }

    pub fn output_sample_rate_hz(&self) -> u32 {
        self.spec.output_rate_hz
    }

    pub fn rational_ratio(&self) -> (u32, u32) {
        (
            self.spec.output_ratio_numerator,
            self.spec.input_ratio_denominator,
        )
    }

    pub fn filter_taps(&self) -> usize {
        self.spec.filter_taps
    }

    pub fn latency_input_frames(&self) -> usize {
        self.spec.radius()
    }

    pub fn latency_seconds(&self) -> f64 {
        self.latency_input_frames() as f64 / f64::from(PHYSICAL_OUTPUT_INPUT_RATE_HZ)
    }

    pub fn input_frames_consumed(&self) -> u64 {
        self.input_frames_consumed
    }

    pub fn output_frames_produced(&self) -> u64 {
        self.output_frames_produced
    }

    pub fn reset(&mut self) {
        self.history = [[0.0; 2]; MAX_FILTER_TAPS];
        self.input_frames_consumed = 0;
        self.output_frames_produced = 0;
        self.next_source_frame = 0;
        self.next_phase = 0;
    }

    pub fn expected_output_frames(
        &self,
        additional_input_frames: usize,
    ) -> Result<usize, StereoOutputResamplerError> {
        let additional = u64::try_from(additional_input_frames)
            .map_err(|_| StereoOutputResamplerError::FrameCounterOverflow)?;
        let total_input = self
            .input_frames_consumed
            .checked_add(additional)
            .ok_or(StereoOutputResamplerError::FrameCounterOverflow)?;
        let state = clock_state_for_input_count(self.spec, total_input)?;
        let produced = state
            .output_frames
            .checked_sub(self.output_frames_produced)
            .ok_or(StereoOutputResamplerError::InvalidClockState)?;
        usize::try_from(produced).map_err(|_| StereoOutputResamplerError::FrameCounterOverflow)
    }

    pub fn input_frames_required(
        &self,
        additional_output_frames: usize,
    ) -> Result<usize, StereoOutputResamplerError> {
        if additional_output_frames == 0 {
            return Ok(0);
        }
        let additional = u64::try_from(additional_output_frames)
            .map_err(|_| StereoOutputResamplerError::FrameCounterOverflow)?;
        let required_output_frames = self
            .output_frames_produced
            .checked_add(additional)
            .ok_or(StereoOutputResamplerError::FrameCounterOverflow)?;
        let required_total = physical_input_frames_for_output_frames(
            self.spec.output_rate_hz,
            required_output_frames,
        )?;
        let additional = required_total
            .checked_sub(self.input_frames_consumed)
            .ok_or(StereoOutputResamplerError::InvalidClockState)?;
        usize::try_from(additional).map_err(|_| StereoOutputResamplerError::FrameCounterOverflow)
    }

    pub fn push_frame(
        &mut self,
        input: [f32; 2],
    ) -> Result<Option<[f32; 2]>, StereoOutputResamplerError> {
        validate_input_frame(input)?;
        let next_input_count = self
            .input_frames_consumed
            .checked_add(1)
            .ok_or(StereoOutputResamplerError::FrameCounterOverflow)?;
        let next_clock = clock_state_for_input_count(self.spec, next_input_count)?;
        let output_count = next_clock
            .output_frames
            .checked_sub(self.output_frames_produced)
            .ok_or(StereoOutputResamplerError::InvalidClockState)?;
        if output_count > 1 || self.next_source_frame < self.input_frames_consumed {
            return Err(StereoOutputResamplerError::InvalidClockState);
        }

        let input_index = self.input_frames_consumed;
        if output_count == 1 && self.next_source_frame != input_index {
            return Err(StereoOutputResamplerError::InvalidClockState);
        }
        let slot = (input_index % self.spec.filter_taps as u64) as usize;
        self.history[slot] = input;
        self.input_frames_consumed = next_input_count;
        let output = if output_count == 1 {
            Some(self.filtered_frame(self.next_source_frame, self.next_phase))
        } else {
            None
        };

        self.output_frames_produced = next_clock.output_frames;
        self.next_source_frame = next_clock.next_source_frame;
        self.next_phase = next_clock.next_phase;
        Ok(output)
    }

    pub fn process_frames(
        &mut self,
        input: &[[f32; 2]],
        output: &mut [[f32; 2]],
    ) -> Result<StereoOutputProcess, StereoOutputResamplerError> {
        let required = self.prepare_block(input)?;
        if output.len() < required {
            return Err(StereoOutputResamplerError::OutputTooSmall {
                required_frames: required,
                available_frames: output.len(),
            });
        }
        let mut produced = 0;
        for frame in input.iter().copied() {
            if let Some(frame) = self.push_frame(frame)? {
                output[produced] = frame;
                produced += 1;
            }
        }
        debug_assert_eq!(produced, required);
        Ok(StereoOutputProcess {
            input_frames: input.len(),
            output_frames: produced,
        })
    }

    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<StereoOutputProcess, StereoOutputResamplerError> {
        if !input.len().is_multiple_of(2) {
            return Err(StereoOutputResamplerError::InputMustBeStereo);
        }
        if !output.len().is_multiple_of(2) {
            return Err(StereoOutputResamplerError::OutputMustBeStereo);
        }
        let input_frames = input.len() / 2;
        let required = self.expected_output_frames(input_frames)?;
        let available = output.len() / 2;
        if available < required {
            return Err(StereoOutputResamplerError::OutputTooSmall {
                required_frames: required,
                available_frames: available,
            });
        }
        for frame in input.chunks_exact(2) {
            validate_input_frame([frame[0], frame[1]])?;
        }
        let mut produced = 0;
        for frame in input.chunks_exact(2) {
            if let Some(frame) = self.push_frame([frame[0], frame[1]])? {
                output[produced * 2] = frame[0];
                output[produced * 2 + 1] = frame[1];
                produced += 1;
            }
        }
        debug_assert_eq!(produced, required);
        Ok(StereoOutputProcess {
            input_frames,
            output_frames: produced,
        })
    }

    pub fn snapshot(&self) -> StereoOutputResamplerSnapshot {
        StereoOutputResamplerSnapshot {
            version: STEREO_OUTPUT_RESAMPLER_SNAPSHOT_VERSION,
            output_sample_rate_hz: self.spec.output_rate_hz,
            input_frames_consumed: self.input_frames_consumed,
            output_frames_produced: self.output_frames_produced,
            next_source_frame: self.next_source_frame,
            next_phase: self.next_phase,
            history: self.history,
        }
    }

    pub fn restore(
        &mut self,
        snapshot: &StereoOutputResamplerSnapshot,
    ) -> Result<(), StereoOutputResamplerError> {
        validate_snapshot(snapshot, self.spec)?;
        self.history = snapshot.history;
        self.input_frames_consumed = snapshot.input_frames_consumed;
        self.output_frames_produced = snapshot.output_frames_produced;
        self.next_source_frame = snapshot.next_source_frame;
        self.next_phase = snapshot.next_phase;
        Ok(())
    }

    fn prepare_block(&self, input: &[[f32; 2]]) -> Result<usize, StereoOutputResamplerError> {
        for frame in input.iter().copied() {
            validate_input_frame(frame)?;
        }
        self.expected_output_frames(input.len())
    }

    fn filtered_frame(&self, source_frame: u64, phase: u32) -> [f32; 2] {
        let coefficient_offset = phase as usize * self.spec.filter_taps;
        let coefficients =
            &self.coefficients[coefficient_offset..coefficient_offset + self.spec.filter_taps];
        let mut output = [0.0_f64; 2];
        for (delay, coefficient) in coefficients.iter().copied().enumerate() {
            if delay as u64 > source_frame {
                continue;
            }
            let sample_index = source_frame - delay as u64;
            let slot = (sample_index % self.spec.filter_taps as u64) as usize;
            output[0] += f64::from(self.history[slot][0]) * coefficient;
            output[1] += f64::from(self.history[slot][1]) * coefficient;
        }
        [output[0] as f32, output[1] as f32]
    }
}

#[derive(Debug, Clone, Copy)]
struct ClockState {
    output_frames: u64,
    next_source_frame: u64,
    next_phase: u32,
}

fn clock_state_for_input_count(
    spec: RateSpec,
    input_frames: u64,
) -> Result<ClockState, StereoOutputResamplerError> {
    let numerator = u128::from(input_frames) * u128::from(spec.output_ratio_numerator);
    let denominator = u128::from(spec.input_ratio_denominator);
    let output_frames = if numerator == 0 {
        0
    } else {
        numerator.div_ceil(denominator)
    };
    if output_frames > u128::from(u64::MAX) {
        return Err(StereoOutputResamplerError::FrameCounterOverflow);
    }
    let next_numerator = output_frames * u128::from(spec.input_ratio_denominator);
    let ratio_numerator = u128::from(spec.output_ratio_numerator);
    let next_source_frame = next_numerator / ratio_numerator;
    if next_source_frame > u128::from(u64::MAX) {
        return Err(StereoOutputResamplerError::FrameCounterOverflow);
    }
    Ok(ClockState {
        output_frames: output_frames as u64,
        next_source_frame: next_source_frame as u64,
        next_phase: (next_numerator % ratio_numerator) as u32,
    })
}

fn design_coefficients(spec: RateSpec) -> Vec<f64> {
    if spec.is_identity() {
        return vec![1.0];
    }
    debug_assert!(spec.filter_taps <= MAX_FILTER_TAPS);
    debug_assert!(spec.filter_taps % 2 == 1);
    let radius = spec.radius() as f64;
    let stopband_hz = f64::from(spec.output_rate_hz) * 0.5;
    let cutoff_hz = (AUDIO_PASSBAND_HZ + stopband_hz) * 0.5;
    let normalized_cutoff = cutoff_hz / f64::from(PHYSICAL_OUTPUT_INPUT_RATE_HZ);
    let window_denominator = bessel_i0(KAISER_BETA_100_DB);
    let mut coefficients = Vec::with_capacity(spec.phase_count() * spec.filter_taps);
    for phase in 0..spec.phase_count() {
        let fraction = phase as f64 / f64::from(spec.output_ratio_numerator);
        let phase_start = coefficients.len();
        let mut coefficient_sum = 0.0;
        for delay in 0..spec.filter_taps {
            let centered_delay = delay as f64 - radius;
            let window_position = centered_delay / radius;
            let window = bessel_i0(
                KAISER_BETA_100_DB * (1.0 - window_position * window_position).max(0.0).sqrt(),
            ) / window_denominator;
            let distance = centered_delay + fraction;
            let coefficient = 2.0
                * normalized_cutoff
                * normalized_sinc(2.0 * normalized_cutoff * distance)
                * window;
            coefficients.push(coefficient);
            coefficient_sum += coefficient;
        }
        for coefficient in &mut coefficients[phase_start..] {
            *coefficient /= coefficient_sum;
        }
    }
    coefficients
}

fn normalized_sinc(value: f64) -> f64 {
    if value.abs() < 1.0e-14 {
        1.0
    } else {
        let phase = std::f64::consts::PI * value;
        phase.sin() / phase
    }
}

fn bessel_i0(value: f64) -> f64 {
    let argument = value * value * 0.25;
    let mut sum = 1.0;
    let mut term = 1.0;
    for order in 1..=64 {
        let order = f64::from(order);
        term *= argument / (order * order);
        sum += term;
        if term <= sum * f64::EPSILON {
            break;
        }
    }
    sum
}

fn validate_input_frame(input: [f32; 2]) -> Result<(), StereoOutputResamplerError> {
    if input
        .into_iter()
        .any(|sample| !sample.is_finite() || sample.abs() > MAX_ABS_PHYSICAL_OUTPUT_SAMPLE)
    {
        Err(StereoOutputResamplerError::InvalidInput)
    } else {
        Ok(())
    }
}

fn validate_snapshot(
    snapshot: &StereoOutputResamplerSnapshot,
    expected_spec: RateSpec,
) -> Result<(), StereoOutputResamplerError> {
    if snapshot.version != STEREO_OUTPUT_RESAMPLER_SNAPSHOT_VERSION {
        return Err(StereoOutputResamplerError::UnsupportedSnapshotVersion {
            version: snapshot.version,
        });
    }
    if snapshot.output_sample_rate_hz != expected_spec.output_rate_hz {
        return Err(StereoOutputResamplerError::SnapshotConfigurationMismatch);
    }
    if snapshot
        .history
        .iter()
        .flatten()
        .any(|sample| !sample.is_finite() || sample.abs() > MAX_ABS_PHYSICAL_OUTPUT_SAMPLE)
    {
        return Err(StereoOutputResamplerError::InvalidSnapshot);
    }
    let expected = clock_state_for_input_count(expected_spec, snapshot.input_frames_consumed)?;
    if snapshot.output_frames_produced != expected.output_frames
        || snapshot.next_source_frame != expected.next_source_frame
        || snapshot.next_phase != expected.next_phase
        || snapshot.next_phase >= expected_spec.output_ratio_numerator
    {
        return Err(StereoOutputResamplerError::InvalidSnapshot);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum StereoOutputResamplerError {
    #[error("output sample rate {output_sample_rate_hz} Hz is unsupported")]
    UnsupportedOutputRate { output_sample_rate_hz: u32 },
    #[error("input must contain complete interleaved stereo frames")]
    InputMustBeStereo,
    #[error("output must contain complete interleaved stereo frames")]
    OutputMustBeStereo,
    #[error("input contains a nonfinite or numerically unsafe value")]
    InvalidInput,
    #[error("output has {available_frames} frames but needs {required_frames}")]
    OutputTooSmall {
        required_frames: usize,
        available_frames: usize,
    },
    #[error("the rational frame counter exceeded its supported range")]
    FrameCounterOverflow,
    #[error("the rational frame clock is inconsistent")]
    InvalidClockState,
    #[error("snapshot version {version} is unsupported")]
    UnsupportedSnapshotVersion { version: u32 },
    #[error("snapshot output rate does not match the converter")]
    SnapshotConfigurationMismatch,
    #[error("snapshot state is invalid")]
    InvalidSnapshot,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_host_boundary_conversion_matches_the_stream_clock() {
        for output_sample_rate_hz in SUPPORTED_PHYSICAL_OUTPUT_RATES_HZ {
            for output_frames in [0u64, 1, 2, 137, 44_100] {
                let resampler = StereoOutputResampler::new(output_sample_rate_hz).unwrap();
                let expected = resampler
                    .input_frames_required(output_frames as usize)
                    .unwrap() as u64;
                assert_eq!(
                    physical_input_frames_for_output_frames(output_sample_rate_hz, output_frames)
                        .unwrap(),
                    expected
                );
            }
        }
    }

    fn input_signal(frames: usize) -> Vec<[f32; 2]> {
        (0..frames)
            .map(|frame| {
                let first = (std::f64::consts::TAU * 997.0 * frame as f64
                    / f64::from(PHYSICAL_OUTPUT_INPUT_RATE_HZ))
                .sin();
                let second = (std::f64::consts::TAU * 17_117.0 * frame as f64
                    / f64::from(PHYSICAL_OUTPUT_INPUT_RATE_HZ))
                .cos();
                [
                    (0.7 * first + 0.2 * second) as f32,
                    (0.3 * first - 0.6 * second) as f32,
                ]
            })
            .collect()
    }

    fn process_all(resampler: &mut StereoOutputResampler, input: &[[f32; 2]]) -> Vec<[f32; 2]> {
        let count = resampler.expected_output_frames(input.len()).unwrap();
        let mut output = vec![[0.0; 2]; count];
        let report = resampler.process_frames(input, &mut output).unwrap();
        assert_eq!(report.output_frames, count);
        output
    }

    fn sine_output(output_rate_hz: u32, frequency_hz: f64) -> Vec<[f32; 2]> {
        let input_frames = PHYSICAL_OUTPUT_INPUT_RATE_HZ as usize / 5;
        let input = (0..input_frames)
            .map(|frame| {
                let phase = std::f64::consts::TAU * frequency_hz * frame as f64
                    / f64::from(PHYSICAL_OUTPUT_INPUT_RATE_HZ);
                [phase.sin() as f32, phase.cos() as f32]
            })
            .collect::<Vec<_>>();
        process_all(
            &mut StereoOutputResampler::new(output_rate_hz).unwrap(),
            &input,
        )
    }

    fn channel_rms(frames: &[[f32; 2]], start: usize, channel: usize) -> f64 {
        let samples = &frames[start.min(frames.len())..];
        let power = samples
            .iter()
            .map(|frame| f64::from(frame[channel]).powi(2))
            .sum::<f64>()
            / samples.len() as f64;
        power.sqrt()
    }

    #[test]
    fn only_declared_output_rates_are_supported() {
        for rate in SUPPORTED_PHYSICAL_OUTPUT_RATES_HZ {
            let resampler = StereoOutputResampler::new(rate).unwrap();
            assert_eq!(resampler.output_sample_rate_hz(), rate);
        }
        assert!(matches!(
            StereoOutputResampler::new(32_000),
            Err(StereoOutputResamplerError::UnsupportedOutputRate {
                output_sample_rate_hz: 32_000
            })
        ));
    }

    #[test]
    fn rational_clock_has_exact_frame_accounting() {
        for rate in SUPPORTED_PHYSICAL_OUTPUT_RATES_HZ {
            let mut resampler = StereoOutputResampler::new(rate).unwrap();
            assert_eq!(
                resampler
                    .expected_output_frames(PHYSICAL_OUTPUT_INPUT_RATE_HZ as usize)
                    .unwrap(),
                rate as usize
            );
            let required_input = resampler.input_frames_required(rate as usize).unwrap();
            assert_eq!(
                resampler.expected_output_frames(required_input).unwrap(),
                rate as usize
            );
            assert_eq!(
                resampler
                    .expected_output_frames(required_input - 1)
                    .unwrap(),
                rate as usize - 1
            );

            let input = vec![[0.0; 2]; PHYSICAL_OUTPUT_INPUT_RATE_HZ as usize / 10];
            let mut produced = 0;
            for partition in input.chunks(137) {
                let count = resampler.expected_output_frames(partition.len()).unwrap();
                let mut output = vec![[0.0; 2]; count];
                produced += resampler
                    .process_frames(partition, &mut output)
                    .unwrap()
                    .output_frames;
            }
            assert_eq!(produced, rate as usize / 10);
        }
    }

    #[test]
    fn processing_is_bit_identical_across_partitions() {
        let input = input_signal(12_345);
        for rate in SUPPORTED_PHYSICAL_OUTPUT_RATES_HZ {
            let mut whole = StereoOutputResampler::new(rate).unwrap();
            let whole_output = process_all(&mut whole, &input);

            let mut split = StereoOutputResampler::new(rate).unwrap();
            let mut split_output = Vec::with_capacity(whole_output.len());
            let mut offset = 0;
            for partition_size in [1, 17, 251, 4, 1_024, 63].into_iter().cycle() {
                if offset == input.len() {
                    break;
                }
                let end = (offset + partition_size).min(input.len());
                split_output.extend(process_all(&mut split, &input[offset..end]));
                offset = end;
            }
            assert_eq!(split_output, whole_output, "{rate} Hz");
            assert_eq!(split.snapshot(), whole.snapshot(), "{rate} Hz");
        }
    }

    #[test]
    fn worst_rate_preserves_the_audio_passband() {
        for frequency_hz in [1_000.0, 18_000.0, 20_000.0] {
            let output = sine_output(44_100, frequency_hz);
            let rms = channel_rms(&output, 800, 0);
            assert!(
                (rms - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.006,
                "{frequency_hz} Hz produced {rms} RMS"
            );
        }
    }

    #[test]
    fn stopband_rejects_aliases_at_each_downsample_rate() {
        for (rate, frequency_hz) in [
            (44_100, 30_000.0),
            (48_000, 32_000.0),
            (88_200, 65_000.0),
            (96_000, 68_000.0),
            (176_400, 93_000.0),
        ] {
            let output = sine_output(rate, frequency_hz);
            let rms = channel_rms(&output, 800, 0);
            assert!(
                rms < 2.0e-4,
                "{rate} Hz leaked {rms} RMS at {frequency_hz} Hz"
            );
        }
    }

    #[test]
    fn impulse_response_is_finite_and_bounded() {
        for rate in SUPPORTED_PHYSICAL_OUTPUT_RATES_HZ {
            let mut input = vec![[0.0; 2]; 2_048];
            input[0] = [1.0, -1.0];
            let mut resampler = StereoOutputResampler::new(rate).unwrap();
            let output = process_all(&mut resampler, &input);
            assert!(output.iter().flatten().all(|sample| sample.is_finite()));
            assert!(output.iter().flatten().any(|sample| sample.abs() > 1.0e-6));
            assert!(output.iter().flatten().all(|sample| sample.abs() <= 1.01));
            assert!(output
                .iter()
                .rev()
                .take(100)
                .flatten()
                .all(|sample| *sample == 0.0));
        }
    }

    #[test]
    fn snapshot_restore_continues_with_identical_output() {
        let input = input_signal(10_000);
        for rate in SUPPORTED_PHYSICAL_OUTPUT_RATES_HZ {
            let mut original = StereoOutputResampler::new(rate).unwrap();
            process_all(&mut original, &input[..3_217]);
            let snapshot = original.snapshot();
            let mut restored = StereoOutputResampler::from_snapshot(&snapshot).unwrap();
            let expected = process_all(&mut original, &input[3_217..]);
            let actual = process_all(&mut restored, &input[3_217..]);
            assert_eq!(actual, expected, "{rate} Hz");
            assert_eq!(restored.snapshot(), original.snapshot(), "{rate} Hz");
        }
    }

    #[test]
    fn snapshot_serialization_preserves_the_fixed_history() {
        let input = input_signal(1_000);
        let mut resampler = StereoOutputResampler::new(44_100).unwrap();
        process_all(&mut resampler, &input);
        let snapshot = resampler.snapshot();
        let json = serde_json::to_string(&snapshot).unwrap();
        let decoded: StereoOutputResamplerSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, snapshot);
        assert_eq!(
            StereoOutputResampler::from_snapshot(&decoded)
                .unwrap()
                .snapshot(),
            snapshot
        );
    }

    #[test]
    fn invalid_restore_and_short_output_preserve_state() {
        let input = input_signal(1_000);
        let mut resampler = StereoOutputResampler::new(44_100).unwrap();
        process_all(&mut resampler, &input[..333]);
        let before = resampler.snapshot();

        let mut invalid = before.clone();
        invalid.next_phase += 1;
        assert_eq!(
            resampler.restore(&invalid),
            Err(StereoOutputResamplerError::InvalidSnapshot)
        );
        assert_eq!(resampler.snapshot(), before);

        let required = resampler
            .expected_output_frames(input[333..].len())
            .unwrap();
        let mut short = vec![[0.0; 2]; required - 1];
        assert!(matches!(
            resampler.process_frames(&input[333..], &mut short),
            Err(StereoOutputResamplerError::OutputTooSmall { .. })
        ));
        assert_eq!(resampler.snapshot(), before);

        let mut wrong_rate = StereoOutputResampler::new(48_000).unwrap();
        assert_eq!(
            wrong_rate.restore(&before),
            Err(StereoOutputResamplerError::SnapshotConfigurationMismatch)
        );
    }

    #[test]
    fn input_magnitude_limit_is_inclusive_and_preserves_state_on_error() {
        let mut resampler = StereoOutputResampler::new(PHYSICAL_OUTPUT_INPUT_RATE_HZ).unwrap();
        assert_eq!(
            resampler
                .push_frame([
                    MAX_ABS_PHYSICAL_OUTPUT_SAMPLE,
                    -MAX_ABS_PHYSICAL_OUTPUT_SAMPLE,
                ])
                .unwrap(),
            Some([
                MAX_ABS_PHYSICAL_OUTPUT_SAMPLE,
                -MAX_ABS_PHYSICAL_OUTPUT_SAMPLE,
            ])
        );
        let before = resampler.snapshot();
        assert_eq!(
            resampler.push_frame([MAX_ABS_PHYSICAL_OUTPUT_SAMPLE * 2.0, 0.0]),
            Err(StereoOutputResamplerError::InvalidInput)
        );
        assert_eq!(resampler.snapshot(), before);
    }

    #[test]
    fn interleaved_api_validates_stereo_shape_and_matches_frame_api() {
        let input = input_signal(1_234);
        let flat_input = input.iter().flatten().copied().collect::<Vec<_>>();
        let mut frame_resampler = StereoOutputResampler::new(48_000).unwrap();
        let expected = process_all(&mut frame_resampler, &input);
        let mut flat_output = vec![0.0; expected.len() * 2];
        let mut flat_resampler = StereoOutputResampler::new(48_000).unwrap();
        let report = flat_resampler
            .process_interleaved(&flat_input, &mut flat_output)
            .unwrap();
        assert_eq!(report.output_frames, expected.len());
        assert_eq!(
            flat_output,
            expected.iter().flatten().copied().collect::<Vec<_>>()
        );
        assert_eq!(
            flat_resampler.process_interleaved(&[0.0], &mut []),
            Err(StereoOutputResamplerError::InputMustBeStereo)
        );
    }
}
