//! Stateful physical phono-stage behavior, including playback RIAA equalization.
//!
//! Input noise and input overload occur before the equalizer. The stage then
//! applies playback RIAA, gain, overload recovery, symmetric output rails, and
//! output slew. All processing state has a fixed size.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{RiaaConfig, RiaaError, RiaaFilterState, RiaaPlaybackFilter};

pub const PHYSICAL_PHONO_STAGE_SNAPSHOT_VERSION: u32 = 1;

/// These engineering bounds prevent numeric overflow in an uncalibrated stage.
pub const MINIMUM_PHONO_GAIN_DB: f64 = -120.0;
pub const MAXIMUM_PHONO_GAIN_DB: f64 = 120.0;
pub const MAXIMUM_PHONO_HEADROOM_V_PEAK: f64 = 1_000.0;
pub const MAXIMUM_PHONO_TIME_SECONDS: f64 = 60.0;
pub const MAXIMUM_PHONO_OVERLOAD_GAIN_REDUCTION_DB: f64 = 120.0;
pub const MAXIMUM_PHONO_SLEW_RATE_V_PER_S: f64 = 1.0e12;
pub const MAXIMUM_PHONO_INPUT_NOISE_V_RMS: f64 = 1.0;
pub const MAXIMUM_PHONO_SAMPLE_RATE_HZ: f64 = 10_000_000.0;
pub const MAXIMUM_ABS_PHONO_INPUT_V: f64 = 1_000_000.0;
pub const MAXIMUM_ABS_PHONO_FILTER_STATE_V: f64 = 1.0e12;

const MINIMUM_POSITIVE_SETTING: f64 = 1.0e-12;
const NOISE_CHANNEL_OFFSET: u64 = 0x9e37_79b9_7f4a_7c15;
const NOISE_MULTIPLIER: u64 = 6_364_136_223_846_793_005;
const NOISE_INCREMENT: u64 = 1_442_695_040_888_963_407;
const UNIFORM_53_SCALE: f64 = 1.0 / ((1_u64 << 53) as f64);

/// Defines one two-channel phono-stage model.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhonoStageConfig {
    /// Sets the small-signal voltage gain.
    pub gain_db: f64,
    /// Sets the symmetric input-overload threshold.
    pub input_headroom_v_peak: f64,
    /// Sets the positive and negative output rails.
    pub output_headroom_v_peak: f64,
    /// Sets the overload-envelope attack time.
    pub input_overload_attack_seconds: f64,
    /// Sets the gain-recovery time after overload.
    pub overload_recovery_seconds: f64,
    /// Sets the maximum overload gain reduction.
    pub overload_gain_reduction_db: f64,
    /// Sets the maximum output change per second.
    pub output_slew_rate_v_per_s: f64,
    /// Sets the discrete input-referred white-noise RMS voltage.
    pub input_referred_noise_v_rms: f64,
    /// Selects the deterministic noise sequence.
    pub noise_seed: u64,
}

impl PhonoStageConfig {
    pub fn validate(self) -> Result<Self, PhonoStageConfigError> {
        for (field, value) in [
            ("gainDb", self.gain_db),
            ("inputHeadroomVPeak", self.input_headroom_v_peak),
            ("outputHeadroomVPeak", self.output_headroom_v_peak),
            (
                "inputOverloadAttackSeconds",
                self.input_overload_attack_seconds,
            ),
            ("overloadRecoverySeconds", self.overload_recovery_seconds),
            ("overloadGainReductionDb", self.overload_gain_reduction_db),
            ("outputSlewRateVPerS", self.output_slew_rate_v_per_s),
            ("inputReferredNoiseVRms", self.input_referred_noise_v_rms),
        ] {
            if !value.is_finite() {
                return Err(PhonoStageConfigError::InvalidField { field });
            }
        }
        if !(MINIMUM_PHONO_GAIN_DB..=MAXIMUM_PHONO_GAIN_DB).contains(&self.gain_db) {
            return Err(PhonoStageConfigError::InvalidField { field: "gainDb" });
        }
        if !(MINIMUM_POSITIVE_SETTING..=MAXIMUM_PHONO_HEADROOM_V_PEAK)
            .contains(&self.input_headroom_v_peak)
        {
            return Err(PhonoStageConfigError::InvalidField {
                field: "inputHeadroomVPeak",
            });
        }
        if !(MINIMUM_POSITIVE_SETTING..=MAXIMUM_PHONO_HEADROOM_V_PEAK)
            .contains(&self.output_headroom_v_peak)
        {
            return Err(PhonoStageConfigError::InvalidField {
                field: "outputHeadroomVPeak",
            });
        }
        for (field, value) in [
            (
                "inputOverloadAttackSeconds",
                self.input_overload_attack_seconds,
            ),
            ("overloadRecoverySeconds", self.overload_recovery_seconds),
        ] {
            if !(MINIMUM_POSITIVE_SETTING..=MAXIMUM_PHONO_TIME_SECONDS).contains(&value) {
                return Err(PhonoStageConfigError::InvalidField { field });
            }
        }
        if self.overload_recovery_seconds < self.input_overload_attack_seconds {
            return Err(PhonoStageConfigError::InvalidField {
                field: "overloadRecoverySeconds",
            });
        }
        if !(MINIMUM_POSITIVE_SETTING..=MAXIMUM_PHONO_OVERLOAD_GAIN_REDUCTION_DB)
            .contains(&self.overload_gain_reduction_db)
        {
            return Err(PhonoStageConfigError::InvalidField {
                field: "overloadGainReductionDb",
            });
        }
        if !(MINIMUM_POSITIVE_SETTING..=MAXIMUM_PHONO_SLEW_RATE_V_PER_S)
            .contains(&self.output_slew_rate_v_per_s)
        {
            return Err(PhonoStageConfigError::InvalidField {
                field: "outputSlewRateVPerS",
            });
        }
        if !(0.0..=MAXIMUM_PHONO_INPUT_NOISE_V_RMS).contains(&self.input_referred_noise_v_rms)
            || self.input_referred_noise_v_rms > self.input_headroom_v_peak
        {
            return Err(PhonoStageConfigError::InvalidField {
                field: "inputReferredNoiseVRms",
            });
        }
        Ok(self)
    }
}

/// Reports the last processed stereo frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalPhonoStageTelemetry {
    pub input_v: [f64; 2],
    pub noise_v: [f64; 2],
    pub equalized_input_v: [f64; 2],
    pub input_overload: [bool; 2],
    pub output_overload: [bool; 2],
    pub output_rail_limited: [bool; 2],
    pub output_slew_limited: [bool; 2],
    pub overload_state: [f64; 2],
    pub effective_gain_linear: [f64; 2],
    pub requested_output_v: [f64; 2],
    pub output_v: [f64; 2],
    pub completed_frames: u64,
}

impl PhysicalPhonoStageTelemetry {
    fn initial(gain_linear: f64) -> Self {
        Self {
            input_v: [0.0; 2],
            noise_v: [0.0; 2],
            equalized_input_v: [0.0; 2],
            input_overload: [false; 2],
            output_overload: [false; 2],
            output_rail_limited: [false; 2],
            output_slew_limited: [false; 2],
            overload_state: [0.0; 2],
            effective_gain_linear: [gain_linear; 2],
            requested_output_v: [0.0; 2],
            output_v: [0.0; 2],
            completed_frames: 0,
        }
    }
}

/// Stores all mutable state that affects later phono output.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalPhonoStageSnapshot {
    pub version: u32,
    pub config: PhonoStageConfig,
    pub sample_rate_hz: f64,
    pub cutter_bandwidth_hz: f64,
    pub playback_riaa: [RiaaFilterState; 2],
    pub overload_state: [f64; 2],
    pub output_v: [f64; 2],
    pub noise_state: [u64; 2],
    pub completed_frames: u64,
    pub last_telemetry: PhysicalPhonoStageTelemetry,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicalPhonoStageProcess {
    pub frames: usize,
    pub last_telemetry: PhysicalPhonoStageTelemetry,
}

/// Processes a stereo phono signal with fixed-size state.
#[derive(Debug, Clone, Copy)]
pub struct PhysicalPhonoStage {
    config: PhonoStageConfig,
    riaa_config: RiaaConfig,
    playback_riaa: [RiaaPlaybackFilter; 2],
    gain_linear: f64,
    attack_coefficient: f64,
    recovery_coefficient: f64,
    maximum_slew_per_frame_v: f64,
    overload_state: [f64; 2],
    output_v: [f64; 2],
    noise_state: [u64; 2],
    completed_frames: u64,
    last_telemetry: PhysicalPhonoStageTelemetry,
}

impl PhysicalPhonoStage {
    pub fn new(
        config: PhonoStageConfig,
        sample_rate_hz: f64,
        cutter_bandwidth_hz: f64,
    ) -> Result<Self, PhysicalPhonoStageError> {
        let config = config.validate()?;
        validate_sample_rate(sample_rate_hz)?;
        let riaa_config = RiaaConfig::new(sample_rate_hz, cutter_bandwidth_hz)?;
        let gain_linear = 10.0_f64.powf(config.gain_db / 20.0);
        let attack_coefficient =
            time_coefficient(config.input_overload_attack_seconds, sample_rate_hz);
        let recovery_coefficient =
            time_coefficient(config.overload_recovery_seconds, sample_rate_hz);
        let maximum_slew_per_frame_v = config.output_slew_rate_v_per_s / sample_rate_hz;
        if !gain_linear.is_finite()
            || !attack_coefficient.is_finite()
            || !recovery_coefficient.is_finite()
            || !maximum_slew_per_frame_v.is_finite()
        {
            return Err(PhysicalPhonoStageError::InvalidDerivedConfiguration);
        }
        let noise_state = initial_noise_state(config.noise_seed);
        Ok(Self {
            config,
            riaa_config,
            playback_riaa: [RiaaPlaybackFilter::new(riaa_config); 2],
            gain_linear,
            attack_coefficient,
            recovery_coefficient,
            maximum_slew_per_frame_v,
            overload_state: [0.0; 2],
            output_v: [0.0; 2],
            noise_state,
            completed_frames: 0,
            last_telemetry: PhysicalPhonoStageTelemetry::initial(gain_linear),
        })
    }

    pub fn from_snapshot(
        snapshot: &PhysicalPhonoStageSnapshot,
    ) -> Result<Self, PhysicalPhonoStageError> {
        let mut stage = Self::new(
            snapshot.config,
            snapshot.sample_rate_hz,
            snapshot.cutter_bandwidth_hz,
        )?;
        stage.restore(snapshot)?;
        Ok(stage)
    }

    pub const fn config(&self) -> PhonoStageConfig {
        self.config
    }

    pub const fn sample_rate_hz(&self) -> f64 {
        self.riaa_config.sample_rate_hz()
    }

    pub const fn cutter_bandwidth_hz(&self) -> f64 {
        self.riaa_config.cutter_bandwidth_hz()
    }

    pub const fn telemetry(&self) -> PhysicalPhonoStageTelemetry {
        self.last_telemetry
    }

    pub const fn completed_frames(&self) -> u64 {
        self.completed_frames
    }

    pub fn reset(&mut self) {
        for filter in &mut self.playback_riaa {
            filter.reset();
        }
        self.overload_state = [0.0; 2];
        self.output_v = [0.0; 2];
        self.noise_state = initial_noise_state(self.config.noise_seed);
        self.completed_frames = 0;
        self.last_telemetry = PhysicalPhonoStageTelemetry::initial(self.gain_linear);
    }

    pub fn process_frame(
        &mut self,
        input_v: [f64; 2],
    ) -> Result<PhysicalPhonoStageTelemetry, PhysicalPhonoStageError> {
        validate_input(input_v)?;
        self.completed_frames
            .checked_add(1)
            .ok_or(PhysicalPhonoStageError::FrameCounterOverflow)?;
        let mut next = *self;
        let telemetry = next.advance_validated(input_v);
        *self = next;
        Ok(telemetry)
    }

    /// Processes equal-length stereo frame slices without allocation.
    pub fn process_frames(
        &mut self,
        input_v: &[[f64; 2]],
        output_v: &mut [[f64; 2]],
    ) -> Result<PhysicalPhonoStageProcess, PhysicalPhonoStageError> {
        if output_v.len() < input_v.len() {
            return Err(PhysicalPhonoStageError::OutputTooSmall {
                required_frames: input_v.len(),
                available_frames: output_v.len(),
            });
        }
        for input in input_v.iter().copied() {
            validate_input(input)?;
        }
        let additional = u64::try_from(input_v.len())
            .map_err(|_| PhysicalPhonoStageError::FrameCounterOverflow)?;
        self.completed_frames
            .checked_add(additional)
            .ok_or(PhysicalPhonoStageError::FrameCounterOverflow)?;

        for (input, output) in input_v.iter().copied().zip(output_v.iter_mut()) {
            *output = self.advance_validated(input).output_v;
        }
        Ok(PhysicalPhonoStageProcess {
            frames: input_v.len(),
            last_telemetry: self.last_telemetry,
        })
    }

    pub fn snapshot(&self) -> PhysicalPhonoStageSnapshot {
        PhysicalPhonoStageSnapshot {
            version: PHYSICAL_PHONO_STAGE_SNAPSHOT_VERSION,
            config: self.config,
            sample_rate_hz: self.riaa_config.sample_rate_hz(),
            cutter_bandwidth_hz: self.riaa_config.cutter_bandwidth_hz(),
            playback_riaa: self.playback_riaa.map(RiaaPlaybackFilter::state),
            overload_state: self.overload_state,
            output_v: self.output_v,
            noise_state: self.noise_state,
            completed_frames: self.completed_frames,
            last_telemetry: self.last_telemetry,
        }
    }

    pub fn restore(
        &mut self,
        snapshot: &PhysicalPhonoStageSnapshot,
    ) -> Result<(), PhysicalPhonoStageError> {
        if snapshot.version != PHYSICAL_PHONO_STAGE_SNAPSHOT_VERSION {
            return Err(PhysicalPhonoStageError::UnsupportedSnapshotVersion {
                version: snapshot.version,
            });
        }
        if snapshot.config != self.config
            || snapshot.sample_rate_hz.to_bits() != self.riaa_config.sample_rate_hz().to_bits()
            || snapshot.cutter_bandwidth_hz.to_bits()
                != self.riaa_config.cutter_bandwidth_hz().to_bits()
        {
            return Err(PhysicalPhonoStageError::SnapshotConfigurationMismatch);
        }
        validate_snapshot(snapshot)?;
        let mut playback_riaa = self.playback_riaa;
        for (filter, state) in playback_riaa.iter_mut().zip(snapshot.playback_riaa) {
            filter.restore_state(state)?;
        }
        self.playback_riaa = playback_riaa;
        self.overload_state = snapshot.overload_state;
        self.output_v = snapshot.output_v;
        self.noise_state = snapshot.noise_state;
        self.completed_frames = snapshot.completed_frames;
        self.last_telemetry = snapshot.last_telemetry;
        Ok(())
    }

    fn advance_validated(&mut self, input_v: [f64; 2]) -> PhysicalPhonoStageTelemetry {
        let mut noise_v = [0.0; 2];
        let mut equalized_input_v = [0.0; 2];
        let mut input_overload = [false; 2];
        let mut output_overload = [false; 2];
        let mut output_rail_limited = [false; 2];
        let mut output_slew_limited = [false; 2];
        let mut effective_gain_linear = [0.0; 2];
        let mut requested_output_v = [0.0; 2];

        for channel in 0..2 {
            noise_v[channel] = self.next_noise(channel);
            let noisy_input = input_v[channel] + noise_v[channel];
            let input_ratio = noisy_input.abs() / self.config.input_headroom_v_peak;
            let limited_input = noisy_input.clamp(
                -self.config.input_headroom_v_peak,
                self.config.input_headroom_v_peak,
            );
            equalized_input_v[channel] =
                self.playback_riaa[channel].process_sample_f64(limited_input);
            let unrecovered_output = equalized_input_v[channel] * self.gain_linear;
            let output_ratio = unrecovered_output.abs() / self.config.output_headroom_v_peak;
            let overload_drive = ((input_ratio - 1.0).max(output_ratio - 1.0)).clamp(0.0, 1.0);
            let coefficient = if overload_drive > self.overload_state[channel] {
                self.attack_coefficient
            } else {
                self.recovery_coefficient
            };
            self.overload_state[channel] +=
                coefficient * (overload_drive - self.overload_state[channel]);
            self.overload_state[channel] = self.overload_state[channel].clamp(0.0, 1.0);

            let recovery_gain = 10.0_f64.powf(
                -self.config.overload_gain_reduction_db * self.overload_state[channel] / 20.0,
            );
            effective_gain_linear[channel] = self.gain_linear * recovery_gain;
            requested_output_v[channel] =
                equalized_input_v[channel] * effective_gain_linear[channel];
            let rail_target = requested_output_v[channel].clamp(
                -self.config.output_headroom_v_peak,
                self.config.output_headroom_v_peak,
            );
            let delta = rail_target - self.output_v[channel];
            let limited_delta = delta.clamp(
                -self.maximum_slew_per_frame_v,
                self.maximum_slew_per_frame_v,
            );
            self.output_v[channel] += limited_delta;

            input_overload[channel] = input_ratio > 1.0;
            output_overload[channel] = output_ratio > 1.0;
            output_rail_limited[channel] = rail_target != requested_output_v[channel];
            output_slew_limited[channel] = limited_delta != delta;
        }
        self.completed_frames += 1;
        self.last_telemetry = PhysicalPhonoStageTelemetry {
            input_v,
            noise_v,
            equalized_input_v,
            input_overload,
            output_overload,
            output_rail_limited,
            output_slew_limited,
            overload_state: self.overload_state,
            effective_gain_linear,
            requested_output_v,
            output_v: self.output_v,
            completed_frames: self.completed_frames,
        };
        debug_assert!(telemetry_is_valid(
            self.last_telemetry,
            self.config,
            self.gain_linear
        ));
        self.last_telemetry
    }

    fn next_noise(&mut self, channel: usize) -> f64 {
        if self.config.input_referred_noise_v_rms == 0.0 {
            return 0.0;
        }
        let mut sum = 0.0;
        for _ in 0..4 {
            self.noise_state[channel] = self.noise_state[channel]
                .wrapping_mul(NOISE_MULTIPLIER)
                .wrapping_add(NOISE_INCREMENT);
            let fraction = ((self.noise_state[channel] >> 11) as f64) * UNIFORM_53_SCALE;
            sum += fraction;
        }
        (sum - 2.0) * 3.0_f64.sqrt() * self.config.input_referred_noise_v_rms
    }
}

fn initial_noise_state(seed: u64) -> [u64; 2] {
    [
        mix_seed(seed),
        mix_seed(seed.wrapping_add(NOISE_CHANNEL_OFFSET)),
    ]
}

fn mix_seed(mut value: u64) -> u64 {
    value = value.wrapping_add(NOISE_CHANNEL_OFFSET);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn time_coefficient(time_seconds: f64, sample_rate_hz: f64) -> f64 {
    1.0 - (-1.0 / (time_seconds * sample_rate_hz)).exp()
}

fn validate_sample_rate(sample_rate_hz: f64) -> Result<(), PhysicalPhonoStageError> {
    if !sample_rate_hz.is_finite()
        || !(1.0..=MAXIMUM_PHONO_SAMPLE_RATE_HZ).contains(&sample_rate_hz)
    {
        Err(PhysicalPhonoStageError::InvalidSampleRate { sample_rate_hz })
    } else {
        Ok(())
    }
}

fn validate_input(input_v: [f64; 2]) -> Result<(), PhysicalPhonoStageError> {
    if input_v
        .into_iter()
        .any(|sample| !sample.is_finite() || sample.abs() > MAXIMUM_ABS_PHONO_INPUT_V)
    {
        Err(PhysicalPhonoStageError::InvalidInput)
    } else {
        Ok(())
    }
}

fn validate_snapshot(snapshot: &PhysicalPhonoStageSnapshot) -> Result<(), PhysicalPhonoStageError> {
    if snapshot.version != PHYSICAL_PHONO_STAGE_SNAPSHOT_VERSION {
        return Err(PhysicalPhonoStageError::UnsupportedSnapshotVersion {
            version: snapshot.version,
        });
    }
    snapshot.config.validate()?;
    validate_sample_rate(snapshot.sample_rate_hz)?;
    let riaa_config = RiaaConfig::new(snapshot.sample_rate_hz, snapshot.cutter_bandwidth_hz)?;
    for state in snapshot.playback_riaa {
        if [state.first_section_delay, state.second_section_delay]
            .into_iter()
            .any(|value| value.is_finite() && value.abs() > MAXIMUM_ABS_PHONO_FILTER_STATE_V)
        {
            return Err(PhysicalPhonoStageError::InvalidSnapshot);
        }
        let mut filter = RiaaPlaybackFilter::new(riaa_config);
        filter.restore_state(state)?;
    }
    let gain_linear = 10.0_f64.powf(snapshot.config.gain_db / 20.0);
    if snapshot
        .overload_state
        .into_iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        || snapshot
            .output_v
            .into_iter()
            .any(|value| !value.is_finite() || value.abs() > snapshot.config.output_headroom_v_peak)
        || snapshot.last_telemetry.completed_frames != snapshot.completed_frames
        || snapshot.last_telemetry.overload_state != snapshot.overload_state
        || snapshot.last_telemetry.output_v != snapshot.output_v
        || !telemetry_is_valid(snapshot.last_telemetry, snapshot.config, gain_linear)
    {
        return Err(PhysicalPhonoStageError::InvalidSnapshot);
    }
    Ok(())
}

fn telemetry_is_valid(
    telemetry: PhysicalPhonoStageTelemetry,
    config: PhonoStageConfig,
    gain_linear: f64,
) -> bool {
    let scalar_fields_are_valid = telemetry
        .input_v
        .into_iter()
        .all(|value| value.is_finite() && value.abs() <= MAXIMUM_ABS_PHONO_INPUT_V)
        && telemetry.noise_v.into_iter().all(|value| {
            value.is_finite()
                && value.abs() <= 12.0_f64.sqrt() * config.input_referred_noise_v_rms + f64::EPSILON
        })
        && telemetry
            .equalized_input_v
            .into_iter()
            .all(|value| value.is_finite() && value.abs() <= MAXIMUM_ABS_PHONO_FILTER_STATE_V)
        && telemetry
            .overload_state
            .into_iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
        && telemetry
            .effective_gain_linear
            .into_iter()
            .all(|value| value.is_finite() && value > 0.0 && value <= gain_linear)
        && telemetry.requested_output_v.into_iter().all(f64::is_finite)
        && telemetry
            .output_v
            .into_iter()
            .all(|value| value.is_finite() && value.abs() <= config.output_headroom_v_peak);
    scalar_fields_are_valid
        && (0..2).all(|channel| {
            let noisy_input = telemetry.input_v[channel] + telemetry.noise_v[channel];
            let expected_input_overload = noisy_input.abs() > config.input_headroom_v_peak;
            let expected_output_overload = (telemetry.equalized_input_v[channel] * gain_linear)
                .abs()
                > config.output_headroom_v_peak;
            let expected_gain = gain_linear
                * 10.0_f64.powf(
                    -config.overload_gain_reduction_db * telemetry.overload_state[channel] / 20.0,
                );
            let expected_requested = telemetry.equalized_input_v[channel] * expected_gain;
            let expected_rail_limited = expected_requested.abs() > config.output_headroom_v_peak;
            telemetry.input_overload[channel] == expected_input_overload
                && telemetry.output_overload[channel] == expected_output_overload
                && telemetry.effective_gain_linear[channel].to_bits() == expected_gain.to_bits()
                && telemetry.requested_output_v[channel].to_bits() == expected_requested.to_bits()
                && telemetry.output_rail_limited[channel] == expected_rail_limited
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum PhonoStageConfigError {
    #[error("phono stage field {field} is invalid")]
    InvalidField { field: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum PhysicalPhonoStageError {
    #[error(transparent)]
    Config(#[from] PhonoStageConfigError),
    #[error(transparent)]
    Riaa(#[from] RiaaError),
    #[error("phono sample rate {sample_rate_hz} Hz is invalid")]
    InvalidSampleRate { sample_rate_hz: f64 },
    #[error("phono input is nonfinite or exceeds the numeric safety limit")]
    InvalidInput,
    #[error("derived phono configuration is invalid")]
    InvalidDerivedConfiguration,
    #[error("phono output has {available_frames} frames but needs {required_frames}")]
    OutputTooSmall {
        required_frames: usize,
        available_frames: usize,
    },
    #[error("the phono frame counter exceeded its supported range")]
    FrameCounterOverflow,
    #[error("phono snapshot version {version} is unsupported")]
    UnsupportedSnapshotVersion { version: u32 },
    #[error("phono snapshot configuration does not match the stage")]
    SnapshotConfigurationMismatch,
    #[error("phono snapshot state is invalid")]
    InvalidSnapshot,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE_HZ: f64 = 192_000.0;
    const CUTTER_BANDWIDTH_HZ: f64 = 50_000.0;

    fn new_stage(config: PhonoStageConfig) -> PhysicalPhonoStage {
        PhysicalPhonoStage::new(config, SAMPLE_RATE_HZ, CUTTER_BANDWIDTH_HZ).unwrap()
    }

    fn test_config() -> PhonoStageConfig {
        PhonoStageConfig {
            gain_db: 40.0,
            input_headroom_v_peak: 0.1,
            output_headroom_v_peak: 10.0,
            input_overload_attack_seconds: 50.0e-6,
            overload_recovery_seconds: 20.0e-3,
            overload_gain_reduction_db: 24.0,
            output_slew_rate_v_per_s: 500_000.0,
            input_referred_noise_v_rms: 0.0,
            noise_seed: 0x56_49_4e_59_4c,
        }
    }

    fn input_signal(frames: usize) -> Vec<[f64; 2]> {
        (0..frames)
            .map(|frame| {
                let phase = std::f64::consts::TAU * 997.0 * frame as f64 / SAMPLE_RATE_HZ;
                [0.0001 * phase.sin(), 0.00007 * phase.cos()]
            })
            .collect()
    }

    #[test]
    fn small_signal_preserves_the_existing_riaa_chain_gain() {
        let mut config = test_config();
        config.output_slew_rate_v_per_s = MAXIMUM_PHONO_SLEW_RATE_V_PER_S;
        let mut stage = new_stage(config);
        let riaa_config = RiaaConfig::new(SAMPLE_RATE_HZ, CUTTER_BANDWIDTH_HZ).unwrap();
        let mut expected_riaa = [RiaaPlaybackFilter::new(riaa_config); 2];
        let gain = 10.0_f64.powf(config.gain_db / 20.0);
        for input in input_signal(16_384) {
            let expected = [
                expected_riaa[0].process_sample_f64(input[0]) * gain,
                expected_riaa[1].process_sample_f64(input[1]) * gain,
            ];
            let actual = stage.process_frame(input).unwrap();
            for (actual, expected) in actual.output_v.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 1.0e-15);
            }
            assert_eq!(actual.overload_state, [0.0; 2]);
            assert_eq!(actual.noise_v, [0.0; 2]);
        }
    }

    #[test]
    fn processing_is_bit_identical_across_partitions_with_noise() {
        let mut config = test_config();
        config.input_referred_noise_v_rms = 500.0e-9;
        let input = input_signal(4_097);
        let mut whole = new_stage(config);
        let mut whole_output = vec![[0.0; 2]; input.len()];
        whole.process_frames(&input, &mut whole_output).unwrap();

        let mut split = new_stage(config);
        let mut split_output = Vec::with_capacity(input.len());
        let mut offset = 0;
        for block in [1, 17, 251, 4, 1_024, 63].into_iter().cycle() {
            if offset == input.len() {
                break;
            }
            let end = (offset + block).min(input.len());
            let mut output = vec![[0.0; 2]; end - offset];
            split
                .process_frames(&input[offset..end], &mut output)
                .unwrap();
            split_output.extend(output);
            offset = end;
        }
        assert_eq!(split_output, whole_output);
        assert_eq!(split.snapshot(), whole.snapshot());
    }

    #[test]
    fn overload_clips_to_rails_and_recovers_causally() {
        let mut config = test_config();
        config.input_headroom_v_peak = 0.01;
        config.output_headroom_v_peak = 0.5;
        config.input_overload_attack_seconds = 10.0e-6;
        config.overload_recovery_seconds = 1.0e-3;
        config.output_slew_rate_v_per_s = MAXIMUM_PHONO_SLEW_RATE_V_PER_S;
        let mut stage = new_stage(config);

        let first = stage.process_frame([0.1, -0.1]).unwrap();
        assert_eq!(first.input_overload, [true; 2]);
        assert!(first.overload_state.into_iter().all(|state| state > 0.0));
        let mut output_overloaded = first.output_overload;
        for _ in 0..128 {
            let telemetry = stage.process_frame([0.1, -0.1]).unwrap();
            for (seen, overloaded) in output_overloaded.iter_mut().zip(telemetry.output_overload) {
                *seen |= overloaded;
            }
            assert!(telemetry
                .output_v
                .into_iter()
                .all(|output| output.abs() <= config.output_headroom_v_peak));
        }
        assert_eq!(output_overloaded, [true; 2]);
        let overloaded_state = stage.telemetry().overload_state;
        let overloaded_gain = stage.telemetry().effective_gain_linear;
        assert!(overloaded_state.into_iter().all(|state| state > 0.99));
        assert!(overloaded_gain
            .into_iter()
            .all(|gain| gain < stage.gain_linear));

        for _ in 0..20_000 {
            stage.process_frame([0.0; 2]).unwrap();
        }
        assert!(stage
            .telemetry()
            .overload_state
            .into_iter()
            .all(|state| state < 5.0e-5));
        assert!(stage
            .telemetry()
            .effective_gain_linear
            .into_iter()
            .all(|gain| (gain - stage.gain_linear).abs() < 0.02));
    }

    #[test]
    fn output_slew_is_bounded_per_frame() {
        let mut config = test_config();
        config.gain_db = 40.0;
        config.input_headroom_v_peak = 10.0;
        config.output_headroom_v_peak = 10.0;
        config.overload_gain_reduction_db = MINIMUM_POSITIVE_SETTING;
        config.output_slew_rate_v_per_s = SAMPLE_RATE_HZ;
        let mut stage = new_stage(config);
        let mut previous = [0.0; 2];
        let mut saw_limit = [false; 2];
        for _ in 0..32 {
            let telemetry = stage.process_frame([5.0, -5.0]).unwrap();
            for channel in 0..2 {
                assert!((telemetry.output_v[channel] - previous[channel]).abs() <= 1.0);
                saw_limit[channel] |= telemetry.output_slew_limited[channel];
            }
            previous = telemetry.output_v;
        }
        assert_eq!(saw_limit, [true; 2]);
    }

    #[test]
    fn snapshot_restore_preserves_noise_and_recovery_continuation() {
        let mut config = test_config();
        config.input_referred_noise_v_rms = 500.0e-9;
        let input = input_signal(2_000);
        let mut original = new_stage(config);
        let mut prefix = vec![[0.0; 2]; 777];
        original.process_frames(&input[..777], &mut prefix).unwrap();
        original.process_frame([0.2, -0.2]).unwrap();
        let snapshot = original.snapshot();
        let json = serde_json::to_string(&snapshot).unwrap();
        let decoded: PhysicalPhonoStageSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, snapshot);

        let mut expected = vec![[0.0; 2]; input.len() - 777];
        original
            .process_frames(&input[777..], &mut expected)
            .unwrap();
        let mut restored = PhysicalPhonoStage::from_snapshot(&decoded).unwrap();
        let mut actual = vec![[0.0; 2]; expected.len()];
        restored.process_frames(&input[777..], &mut actual).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(restored.snapshot(), original.snapshot());
    }

    #[test]
    fn invalid_configurations_are_rejected() {
        let valid = test_config();
        let invalid = [
            PhonoStageConfig {
                gain_db: f64::NAN,
                ..valid
            },
            PhonoStageConfig {
                input_headroom_v_peak: 0.0,
                ..valid
            },
            PhonoStageConfig {
                output_headroom_v_peak: MAXIMUM_PHONO_HEADROOM_V_PEAK + 1.0,
                ..valid
            },
            PhonoStageConfig {
                input_overload_attack_seconds: 0.0,
                ..valid
            },
            PhonoStageConfig {
                overload_recovery_seconds: valid.input_overload_attack_seconds * 0.5,
                ..valid
            },
            PhonoStageConfig {
                overload_gain_reduction_db: 0.0,
                ..valid
            },
            PhonoStageConfig {
                output_slew_rate_v_per_s: f64::INFINITY,
                ..valid
            },
            PhonoStageConfig {
                input_referred_noise_v_rms: valid.input_headroom_v_peak * 2.0,
                ..valid
            },
        ];
        for config in invalid {
            assert!(matches!(
                PhysicalPhonoStage::new(config, SAMPLE_RATE_HZ, CUTTER_BANDWIDTH_HZ),
                Err(PhysicalPhonoStageError::Config(_))
            ));
        }
        assert!(matches!(
            PhysicalPhonoStage::new(valid, f64::INFINITY, CUTTER_BANDWIDTH_HZ),
            Err(PhysicalPhonoStageError::InvalidSampleRate { .. })
        ));
        assert!(matches!(
            PhysicalPhonoStage::new(valid, SAMPLE_RATE_HZ, SAMPLE_RATE_HZ),
            Err(PhysicalPhonoStageError::Riaa(
                RiaaError::InvalidCutterBandwidth { .. }
            ))
        ));
    }

    #[test]
    fn invalid_input_and_snapshot_do_not_mutate_state() {
        let mut stage = new_stage(test_config());
        stage.process_frame([0.001, -0.002]).unwrap();
        let before = stage.snapshot();
        assert_eq!(
            stage.process_frame([f64::NAN, 0.0]),
            Err(PhysicalPhonoStageError::InvalidInput)
        );
        assert_eq!(stage.snapshot(), before);

        let mut short_output = [];
        assert_eq!(
            stage.process_frames(&[[0.0; 2]], &mut short_output),
            Err(PhysicalPhonoStageError::OutputTooSmall {
                required_frames: 1,
                available_frames: 0,
            })
        );
        assert_eq!(stage.snapshot(), before);

        let mut invalid = before;
        invalid.overload_state[0] = 2.0;
        assert_eq!(
            stage.restore(&invalid),
            Err(PhysicalPhonoStageError::InvalidSnapshot)
        );
        assert_eq!(stage.snapshot(), before);

        let mut invalid_riaa = before;
        invalid_riaa.playback_riaa[0].first_section_delay = f64::NAN;
        assert_eq!(
            stage.restore(&invalid_riaa),
            Err(PhysicalPhonoStageError::Riaa(RiaaError::InvalidState))
        );
        assert_eq!(stage.snapshot(), before);

        let mut wrong_config = before;
        wrong_config.config.gain_db += 1.0;
        assert_eq!(
            stage.restore(&wrong_config),
            Err(PhysicalPhonoStageError::SnapshotConfigurationMismatch)
        );
        assert_eq!(stage.snapshot(), before);
    }
}
