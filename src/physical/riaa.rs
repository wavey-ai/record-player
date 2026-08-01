//! Causal RIAA record and playback equalization.
//!
//! RIAA equalization uses three time constants: 3180 us, 318 us, and 75 us.
//! Each transfer has unity gain at 1 kHz.
//! The record transfer also has a cutter pole.
//! This pole limits gain above the audio band.
//! It gives the record transfer equal numerator and denominator orders.
//! Each output sample uses only the current and earlier input samples.

use std::error::Error;
use std::f64::consts::TAU;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The low-frequency RIAA time constant, in seconds.
pub const RIAA_BASS_TIME_CONSTANT_SECONDS: f64 = 3_180.0e-6;

/// The middle RIAA time constant, in seconds.
pub const RIAA_MID_TIME_CONSTANT_SECONDS: f64 = 318.0e-6;

/// The high-frequency RIAA time constant, in seconds.
pub const RIAA_TREBLE_TIME_CONSTANT_SECONDS: f64 = 75.0e-6;

/// The frequency that defines unity gain for each transfer.
pub const RIAA_REFERENCE_FREQUENCY_HZ: f64 = 1_000.0;

/// The internal sample rate for the physical groove model.
pub const RIAA_INTERNAL_SAMPLE_RATE_HZ: f64 = 192_000.0;

/// An initial cutter bandwidth for the physical groove model.
///
/// Cutter bandwidth is the extra high-frequency pole in the record transfer.
/// This value is a model setting, not an RIAA time constant.
pub const RIAA_INITIAL_CUTTER_BANDWIDTH_HZ: f64 = 50_000.0;

/// Validated settings for the record and playback filters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiaaConfig {
    sample_rate_hz: f64,
    cutter_bandwidth_hz: f64,
}

impl RiaaConfig {
    /// Creates settings with an explicit sample rate and cutter bandwidth.
    pub fn new(sample_rate_hz: f64, cutter_bandwidth_hz: f64) -> Result<Self, RiaaError> {
        if !sample_rate_hz.is_finite() || sample_rate_hz <= RIAA_REFERENCE_FREQUENCY_HZ * 2.0 {
            return Err(RiaaError::InvalidSampleRate { sample_rate_hz });
        }

        let nyquist_hz = sample_rate_hz * 0.5;
        if !cutter_bandwidth_hz.is_finite()
            || cutter_bandwidth_hz <= RIAA_REFERENCE_FREQUENCY_HZ
            || cutter_bandwidth_hz >= nyquist_hz
        {
            return Err(RiaaError::InvalidCutterBandwidth {
                cutter_bandwidth_hz,
                nyquist_hz,
            });
        }

        Ok(Self {
            sample_rate_hz,
            cutter_bandwidth_hz,
        })
    }

    /// Creates settings for the fixed 192 kHz physical processing rate.
    pub fn internal_192khz(cutter_bandwidth_hz: f64) -> Result<Self, RiaaError> {
        Self::new(RIAA_INTERNAL_SAMPLE_RATE_HZ, cutter_bandwidth_hz)
    }

    /// Returns the sample rate in hertz.
    pub const fn sample_rate_hz(self) -> f64 {
        self.sample_rate_hz
    }

    /// Returns the cutter bandwidth in hertz.
    pub const fn cutter_bandwidth_hz(self) -> f64 {
        self.cutter_bandwidth_hz
    }

    /// Returns the Nyquist frequency in hertz.
    pub fn nyquist_hz(self) -> f64 {
        self.sample_rate_hz * 0.5
    }
}

/// One complex frequency-response value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RiaaFrequencyResponse {
    /// The real component.
    pub real: f64,
    /// The imaginary component.
    pub imaginary: f64,
}

impl RiaaFrequencyResponse {
    /// Returns the linear amplitude.
    pub fn magnitude(self) -> f64 {
        self.real.hypot(self.imaginary)
    }

    /// Returns the amplitude in decibels.
    pub fn amplitude_db(self) -> f64 {
        20.0 * self.magnitude().log10()
    }

    /// Returns the phase in radians.
    pub fn phase_radians(self) -> f64 {
        self.imaginary.atan2(self.real)
    }

    /// Multiplies two transfer responses.
    pub fn product(self, other: Self) -> Self {
        Self {
            real: self.real * other.real - self.imaginary * other.imaginary,
            imaginary: self.real * other.imaginary + self.imaginary * other.real,
        }
    }

    fn quotient(self, denominator: Self) -> Self {
        let denominator_power =
            denominator.real * denominator.real + denominator.imaginary * denominator.imaginary;
        Self {
            real: (self.real * denominator.real + self.imaginary * denominator.imaginary)
                / denominator_power,
            imaginary: (self.imaginary * denominator.real - self.real * denominator.imaginary)
                / denominator_power,
        }
    }

    fn scale(self, gain: f64) -> Self {
        Self {
            real: self.real * gain,
            imaginary: self.imaginary * gain,
        }
    }
}

/// The complete delay state for one mono RIAA filter.
///
/// Copy this value to include the filter in an engine snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiaaFilterState {
    /// The transposed delay for the first filter section.
    pub first_section_delay: f64,
    /// The transposed delay for the second filter section.
    pub second_section_delay: f64,
}

impl RiaaFilterState {
    fn validate(self) -> Result<Self, RiaaError> {
        if !self.first_section_delay.is_finite() || !self.second_section_delay.is_finite() {
            return Err(RiaaError::InvalidState);
        }
        Ok(self)
    }
}

/// A causal mono RIAA playback filter.
///
/// Use one filter for each audio channel.
#[derive(Clone, Copy, Debug)]
pub struct RiaaPlaybackFilter {
    config: RiaaConfig,
    cascade: FirstOrderCascade,
}

impl RiaaPlaybackFilter {
    /// Creates a playback filter with cleared delay state.
    pub fn new(config: RiaaConfig) -> Self {
        let first = FirstOrderCoefficients::from_analog(
            1.0,
            RIAA_MID_TIME_CONSTANT_SECONDS,
            1.0,
            RIAA_BASS_TIME_CONSTANT_SECONDS,
            config.sample_rate_hz,
        );
        let second = FirstOrderCoefficients::from_analog(
            1.0,
            0.0,
            1.0,
            RIAA_TREBLE_TIME_CONSTANT_SECONDS,
            config.sample_rate_hz,
        );
        Self {
            config,
            cascade: FirstOrderCascade::normalized_at_reference(first, second, config),
        }
    }

    /// Returns the validated filter settings.
    pub const fn config(self) -> RiaaConfig {
        self.config
    }

    /// Processes one sample with double-precision state.
    pub fn process_sample_f64(&mut self, input: f64) -> f64 {
        self.cascade.process(input)
    }

    /// Processes one sample and returns a single-precision result.
    pub fn process_sample(&mut self, input: f32) -> f32 {
        self.process_sample_f64(f64::from(input)) as f32
    }

    /// Processes a single-precision block in place without allocation.
    pub fn process_in_place(&mut self, samples: &mut [f32]) {
        for sample in samples {
            *sample = self.process_sample(*sample);
        }
    }

    /// Processes a double-precision block in place without allocation.
    pub fn process_in_place_f64(&mut self, samples: &mut [f64]) {
        for sample in samples {
            *sample = self.process_sample_f64(*sample);
        }
    }

    /// Returns the complete delay state.
    pub const fn state(self) -> RiaaFilterState {
        self.cascade.state
    }

    /// Restores a validated delay state.
    pub fn restore_state(&mut self, state: RiaaFilterState) -> Result<(), RiaaError> {
        self.cascade.state = state.validate()?;
        Ok(())
    }

    /// Clears all delay state.
    pub fn reset(&mut self) {
        self.cascade.state = RiaaFilterState::default();
    }

    /// Returns the digital response at one frequency.
    pub fn response(&self, frequency_hz: f64) -> Result<RiaaFrequencyResponse, RiaaError> {
        validate_digital_frequency(self.config, frequency_hz)?;
        Ok(self.cascade.response(frequency_hz, self.config))
    }
}

/// A causal mono RIAA record filter with a cutter bandwidth pole.
///
/// Use one filter for each cutter channel.
#[derive(Clone, Copy, Debug)]
pub struct RiaaRecordFilter {
    config: RiaaConfig,
    cascade: FirstOrderCascade,
}

impl RiaaRecordFilter {
    /// Creates a record filter with cleared delay state.
    pub fn new(config: RiaaConfig) -> Self {
        let cutter_time_constant_seconds = 1.0 / (TAU * config.cutter_bandwidth_hz);
        let first = FirstOrderCoefficients::from_analog(
            1.0,
            RIAA_BASS_TIME_CONSTANT_SECONDS,
            1.0,
            RIAA_MID_TIME_CONSTANT_SECONDS,
            config.sample_rate_hz,
        );
        let second = FirstOrderCoefficients::from_analog(
            1.0,
            RIAA_TREBLE_TIME_CONSTANT_SECONDS,
            1.0,
            cutter_time_constant_seconds,
            config.sample_rate_hz,
        );
        Self {
            config,
            cascade: FirstOrderCascade::normalized_at_reference(first, second, config),
        }
    }

    /// Returns the validated filter settings.
    pub const fn config(self) -> RiaaConfig {
        self.config
    }

    /// Processes one sample with double-precision state.
    pub fn process_sample_f64(&mut self, input: f64) -> f64 {
        self.cascade.process(input)
    }

    /// Processes one sample and returns a single-precision result.
    pub fn process_sample(&mut self, input: f32) -> f32 {
        self.process_sample_f64(f64::from(input)) as f32
    }

    /// Processes a single-precision block in place without allocation.
    pub fn process_in_place(&mut self, samples: &mut [f32]) {
        for sample in samples {
            *sample = self.process_sample(*sample);
        }
    }

    /// Processes a double-precision block in place without allocation.
    pub fn process_in_place_f64(&mut self, samples: &mut [f64]) {
        for sample in samples {
            *sample = self.process_sample_f64(*sample);
        }
    }

    /// Returns the complete delay state.
    pub const fn state(self) -> RiaaFilterState {
        self.cascade.state
    }

    /// Restores a validated delay state.
    pub fn restore_state(&mut self, state: RiaaFilterState) -> Result<(), RiaaError> {
        self.cascade.state = state.validate()?;
        Ok(())
    }

    /// Clears all delay state.
    pub fn reset(&mut self) {
        self.cascade.state = RiaaFilterState::default();
    }

    /// Returns the digital response at one frequency.
    pub fn response(&self, frequency_hz: f64) -> Result<RiaaFrequencyResponse, RiaaError> {
        validate_digital_frequency(self.config, frequency_hz)?;
        Ok(self.cascade.response(frequency_hz, self.config))
    }
}

/// Returns the ideal analog playback amplitude in decibels.
///
/// The result has unity gain at 1 kHz.
pub fn ideal_playback_db(frequency_hz: f64) -> Result<f64, RiaaError> {
    validate_positive_frequency(frequency_hz)?;
    let reference_gain = analog_playback_magnitude(RIAA_REFERENCE_FREQUENCY_HZ);
    Ok(20.0 * (analog_playback_magnitude(frequency_hz) / reference_gain).log10())
}

/// Returns the ideal analog record amplitude in decibels.
///
/// The transfer includes the specified cutter bandwidth pole.
/// The result has unity gain at 1 kHz.
pub fn ideal_record_db(frequency_hz: f64, cutter_bandwidth_hz: f64) -> Result<f64, RiaaError> {
    validate_positive_frequency(frequency_hz)?;
    if !cutter_bandwidth_hz.is_finite() || cutter_bandwidth_hz <= RIAA_REFERENCE_FREQUENCY_HZ {
        return Err(RiaaError::InvalidCutterBandwidth {
            cutter_bandwidth_hz,
            nyquist_hz: f64::INFINITY,
        });
    }

    let magnitude = analog_record_magnitude(frequency_hz, cutter_bandwidth_hz);
    let reference_magnitude =
        analog_record_magnitude(RIAA_REFERENCE_FREQUENCY_HZ, cutter_bandwidth_hz);
    Ok(20.0 * (magnitude / reference_magnitude).log10())
}

/// Returns the normalized digital response of the cutter bandwidth pole.
///
/// A record and playback cascade has this response within numeric precision.
pub fn normalized_cutter_response(
    config: RiaaConfig,
    frequency_hz: f64,
) -> Result<RiaaFrequencyResponse, RiaaError> {
    validate_digital_frequency(config, frequency_hz)?;
    let cutter_time_constant_seconds = 1.0 / (TAU * config.cutter_bandwidth_hz);
    let coefficients = FirstOrderCoefficients::from_analog(
        1.0,
        0.0,
        1.0,
        cutter_time_constant_seconds,
        config.sample_rate_hz,
    );
    let reference = coefficients
        .response(RIAA_REFERENCE_FREQUENCY_HZ, config.sample_rate_hz)
        .magnitude();
    Ok(coefficients
        .response(frequency_hz, config.sample_rate_hz)
        .scale(reference.recip()))
}

/// An invalid RIAA filter setting or state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RiaaError {
    /// The sample rate cannot represent the 1 kHz reference.
    InvalidSampleRate { sample_rate_hz: f64 },
    /// The cutter pole is not between 1 kHz and Nyquist.
    InvalidCutterBandwidth {
        cutter_bandwidth_hz: f64,
        nyquist_hz: f64,
    },
    /// The response frequency is outside the supported range.
    InvalidFrequency { frequency_hz: f64, maximum_hz: f64 },
    /// A delay state contains a nonfinite number.
    InvalidState,
}

impl fmt::Display for RiaaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSampleRate { sample_rate_hz } => {
                write!(formatter, "invalid RIAA sample rate: {sample_rate_hz}")
            }
            Self::InvalidCutterBandwidth {
                cutter_bandwidth_hz,
                nyquist_hz,
            } => write!(
                formatter,
                "invalid cutter bandwidth {cutter_bandwidth_hz}; Nyquist is {nyquist_hz}"
            ),
            Self::InvalidFrequency {
                frequency_hz,
                maximum_hz,
            } => write!(
                formatter,
                "invalid response frequency {frequency_hz}; maximum is {maximum_hz}"
            ),
            Self::InvalidState => formatter.write_str("RIAA delay state is not finite"),
        }
    }
}

impl Error for RiaaError {}

#[derive(Clone, Copy, Debug)]
struct FirstOrderCoefficients {
    b0: f64,
    b1: f64,
    a1: f64,
}

impl FirstOrderCoefficients {
    fn from_analog(
        numerator_constant: f64,
        numerator_s: f64,
        denominator_constant: f64,
        denominator_s: f64,
        sample_rate_hz: f64,
    ) -> Self {
        let bilinear_scale = 2.0 * sample_rate_hz;
        let denominator = denominator_constant + denominator_s * bilinear_scale;
        let coefficients = Self {
            b0: (numerator_constant + numerator_s * bilinear_scale) / denominator,
            b1: (numerator_constant - numerator_s * bilinear_scale) / denominator,
            a1: (denominator_constant - denominator_s * bilinear_scale) / denominator,
        };
        debug_assert!(coefficients.a1.abs() < 1.0);
        coefficients
    }

    fn process(self, input: f64, delay: &mut f64) -> f64 {
        let output = self.b0.mul_add(input, *delay);
        let next_delay = self.b1.mul_add(input, -self.a1 * output);
        *delay = flush_subnormal(next_delay);
        output
    }

    fn response(self, frequency_hz: f64, sample_rate_hz: f64) -> RiaaFrequencyResponse {
        let angle = TAU * frequency_hz / sample_rate_hz;
        let cosine = angle.cos();
        let negative_sine = -angle.sin();
        let numerator = RiaaFrequencyResponse {
            real: self.b0 + self.b1 * cosine,
            imaginary: self.b1 * negative_sine,
        };
        let denominator = RiaaFrequencyResponse {
            real: 1.0 + self.a1 * cosine,
            imaginary: self.a1 * negative_sine,
        };
        numerator.quotient(denominator)
    }
}

#[derive(Clone, Copy, Debug)]
struct FirstOrderCascade {
    first: FirstOrderCoefficients,
    second: FirstOrderCoefficients,
    gain: f64,
    state: RiaaFilterState,
}

impl FirstOrderCascade {
    fn normalized_at_reference(
        first: FirstOrderCoefficients,
        second: FirstOrderCoefficients,
        config: RiaaConfig,
    ) -> Self {
        let reference_response = first
            .response(RIAA_REFERENCE_FREQUENCY_HZ, config.sample_rate_hz)
            .product(second.response(RIAA_REFERENCE_FREQUENCY_HZ, config.sample_rate_hz));
        Self {
            first,
            second,
            gain: reference_response.magnitude().recip(),
            state: RiaaFilterState::default(),
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        let first_output = self
            .first
            .process(input, &mut self.state.first_section_delay);
        self.second
            .process(first_output, &mut self.state.second_section_delay)
            * self.gain
    }

    fn response(self, frequency_hz: f64, config: RiaaConfig) -> RiaaFrequencyResponse {
        self.first
            .response(frequency_hz, config.sample_rate_hz)
            .product(self.second.response(frequency_hz, config.sample_rate_hz))
            .scale(self.gain)
    }
}

fn analog_playback_magnitude(frequency_hz: f64) -> f64 {
    let angular_frequency = TAU * frequency_hz;
    (1.0 + (angular_frequency * RIAA_MID_TIME_CONSTANT_SECONDS).powi(2)).sqrt()
        / ((1.0 + (angular_frequency * RIAA_BASS_TIME_CONSTANT_SECONDS).powi(2)).sqrt()
            * (1.0 + (angular_frequency * RIAA_TREBLE_TIME_CONSTANT_SECONDS).powi(2)).sqrt())
}

fn analog_record_magnitude(frequency_hz: f64, cutter_bandwidth_hz: f64) -> f64 {
    let cutter_magnitude = 1.0 / (1.0 + (frequency_hz / cutter_bandwidth_hz).powi(2)).sqrt();
    cutter_magnitude / analog_playback_magnitude(frequency_hz)
}

fn validate_positive_frequency(frequency_hz: f64) -> Result<(), RiaaError> {
    if !frequency_hz.is_finite() || frequency_hz <= 0.0 {
        return Err(RiaaError::InvalidFrequency {
            frequency_hz,
            maximum_hz: f64::INFINITY,
        });
    }
    Ok(())
}

fn validate_digital_frequency(config: RiaaConfig, frequency_hz: f64) -> Result<(), RiaaError> {
    if !frequency_hz.is_finite() || frequency_hz < 0.0 || frequency_hz > config.nyquist_hz() {
        return Err(RiaaError::InvalidFrequency {
            frequency_hz,
            maximum_hz: config.nyquist_hz(),
        });
    }
    Ok(())
}

fn flush_subnormal(value: f64) -> f64 {
    if value.abs() < 1.0e-300 {
        0.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCE_SPOTS: &[(f64, f64)] = &[
        (20.0, 19.274_148),
        (50.0, 16.945_666),
        (100.0, 13.088_460),
        (500.0, 2.647_603),
        (1_000.0, 0.0),
        (2_000.0, -2.588_541),
        (5_000.0, -8.209_628),
        (10_000.0, -13.734_342),
        (20_000.0, -19.620_332),
    ];

    const CUTTER_LIMITED_RECORD_SPOTS: &[(f64, f64)] = &[
        (20.0, -19.272_412),
        (50.0, -16.943_934),
        (100.0, -13.086_741),
        (500.0, -2.646_300),
        (1_000.0, 0.0),
        (2_000.0, 2.583_335),
        (5_000.0, 8.168_151),
        (10_000.0, 13.565_746),
        (20_000.0, 18.977_489),
    ];

    fn config() -> RiaaConfig {
        RiaaConfig::internal_192khz(RIAA_INITIAL_CUTTER_BANDWIDTH_HZ).unwrap()
    }

    fn assert_close(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "actual {actual}, expected {expected}, tolerance {tolerance}"
        );
    }

    #[test]
    fn time_constants_have_the_standard_break_frequencies() {
        assert_close(
            1.0 / (TAU * RIAA_BASS_TIME_CONSTANT_SECONDS),
            50.048_724,
            1.0e-6,
        );
        assert_close(
            1.0 / (TAU * RIAA_MID_TIME_CONSTANT_SECONDS),
            500.487_242,
            1.0e-6,
        );
        assert_close(
            1.0 / (TAU * RIAA_TREBLE_TIME_CONSTANT_SECONDS),
            2_122.065_908,
            1.0e-6,
        );
    }

    #[test]
    fn ideal_playback_matches_reference_spots() {
        for &(frequency_hz, expected_db) in REFERENCE_SPOTS {
            assert_close(
                ideal_playback_db(frequency_hz).unwrap(),
                expected_db,
                1.0e-6,
            );
        }
    }

    #[test]
    fn digital_playback_tracks_the_analog_curve_at_192khz() {
        let filter = RiaaPlaybackFilter::new(config());
        for &(frequency_hz, expected_db) in REFERENCE_SPOTS {
            let tolerance_db = if frequency_hz <= 10_000.0 { 0.08 } else { 0.32 };
            assert_close(
                filter.response(frequency_hz).unwrap().amplitude_db(),
                expected_db,
                tolerance_db,
            );
        }
    }

    #[test]
    fn cutter_limited_record_matches_spot_values() {
        let filter = RiaaRecordFilter::new(config());
        for &(frequency_hz, expected_db) in CUTTER_LIMITED_RECORD_SPOTS {
            assert_close(
                ideal_record_db(frequency_hz, RIAA_INITIAL_CUTTER_BANDWIDTH_HZ).unwrap(),
                expected_db,
                1.0e-6,
            );
            let tolerance_db = if frequency_hz <= 10_000.0 { 0.1 } else { 0.5 };
            assert_close(
                filter.response(frequency_hz).unwrap().amplitude_db(),
                expected_db,
                tolerance_db,
            );
        }
    }

    #[test]
    fn record_and_playback_are_normalized_at_1khz() {
        let playback = RiaaPlaybackFilter::new(config());
        let record = RiaaRecordFilter::new(config());
        assert_close(
            playback
                .response(RIAA_REFERENCE_FREQUENCY_HZ)
                .unwrap()
                .amplitude_db(),
            0.0,
            1.0e-12,
        );
        assert_close(
            record
                .response(RIAA_REFERENCE_FREQUENCY_HZ)
                .unwrap()
                .amplitude_db(),
            0.0,
            1.0e-12,
        );
    }

    #[test]
    fn record_and_playback_leave_only_the_cutter_pole() {
        let config = config();
        let playback = RiaaPlaybackFilter::new(config);
        let record = RiaaRecordFilter::new(config);
        for frequency_hz in [0.0, 20.0, 1_000.0, 10_000.0, 20_000.0, 50_000.0] {
            let actual = record
                .response(frequency_hz)
                .unwrap()
                .product(playback.response(frequency_hz).unwrap());
            let expected = normalized_cutter_response(config, frequency_hz).unwrap();
            assert_close(actual.real, expected.real, 2.0e-12);
            assert_close(actual.imaginary, expected.imaginary, 2.0e-12);
        }
    }

    #[test]
    fn ideal_record_and_playback_have_the_expected_reciprocity() {
        let cutter_hz = RIAA_INITIAL_CUTTER_BANDWIDTH_HZ;
        let reference_cutter =
            1.0 / (1.0 + (RIAA_REFERENCE_FREQUENCY_HZ / cutter_hz).powi(2)).sqrt();
        for &(frequency_hz, _) in REFERENCE_SPOTS {
            let product_db = ideal_record_db(frequency_hz, cutter_hz).unwrap()
                + ideal_playback_db(frequency_hz).unwrap();
            let cutter = 1.0 / (1.0 + (frequency_hz / cutter_hz).powi(2)).sqrt();
            let expected_db = 20.0 * (cutter / reference_cutter).log10();
            assert_close(product_db, expected_db, 1.0e-12);
        }
    }

    #[test]
    fn cutter_pole_keeps_the_record_transfer_bounded_at_nyquist() {
        let config = config();
        let filter = RiaaRecordFilter::new(config);
        let response = filter.response(config.nyquist_hz()).unwrap();
        assert!(response.real.is_finite());
        assert!(response.imaginary.is_finite());
        assert!(response.magnitude() < 300.0);
    }

    #[test]
    fn copied_state_restarts_at_the_same_sample() {
        let config = config();
        let mut first = RiaaPlaybackFilter::new(config);
        for index in 0..257 {
            let input = ((index as f64) * 0.037).sin();
            first.process_sample_f64(input);
        }

        let state = first.state();
        let mut restored = RiaaPlaybackFilter::new(config);
        restored.restore_state(state).unwrap();
        for index in 257..1_024 {
            let input = ((index as f64) * 0.037).sin();
            assert_eq!(
                first.process_sample_f64(input),
                restored.process_sample_f64(input)
            );
        }
    }

    #[test]
    fn in_place_processing_matches_sample_processing() {
        let config = config();
        let mut block_filter = RiaaRecordFilter::new(config);
        let mut sample_filter = RiaaRecordFilter::new(config);
        let mut block = [0.0_f32; 128];
        for (index, sample) in block.iter_mut().enumerate() {
            *sample = ((index as f32) * 0.11).sin();
        }
        let input = block;
        block_filter.process_in_place(&mut block);
        for (actual, source) in block.into_iter().zip(input) {
            assert_eq!(actual, sample_filter.process_sample(source));
        }
    }

    #[test]
    fn reset_clears_all_delay_state() {
        let mut filter = RiaaPlaybackFilter::new(config());
        filter.process_sample_f64(1.0);
        assert_ne!(filter.state(), RiaaFilterState::default());
        filter.reset();
        assert_eq!(filter.state(), RiaaFilterState::default());
    }

    #[test]
    fn invalid_state_does_not_replace_the_current_state() {
        let mut filter = RiaaRecordFilter::new(config());
        filter.process_sample_f64(1.0);
        let before = filter.state();
        let result = filter.restore_state(RiaaFilterState {
            first_section_delay: f64::NAN,
            second_section_delay: 0.0,
        });
        assert_eq!(result, Err(RiaaError::InvalidState));
        assert_eq!(filter.state(), before);
    }

    #[test]
    fn settings_reject_unrepresentable_values() {
        assert!(matches!(
            RiaaConfig::new(2_000.0, 1_500.0),
            Err(RiaaError::InvalidSampleRate { .. })
        ));
        assert!(matches!(
            RiaaConfig::new(192_000.0, 1_000.0),
            Err(RiaaError::InvalidCutterBandwidth { .. })
        ));
        assert!(matches!(
            RiaaConfig::new(192_000.0, 96_000.0),
            Err(RiaaError::InvalidCutterBandwidth { .. })
        ));
        assert!(matches!(
            RiaaConfig::new(f64::NAN, 50_000.0),
            Err(RiaaError::InvalidSampleRate { .. })
        ));
    }

    #[test]
    fn response_rejects_frequencies_above_nyquist() {
        let filter = RiaaPlaybackFilter::new(config());
        assert!(matches!(
            filter.response(96_001.0),
            Err(RiaaError::InvalidFrequency { .. })
        ));
        assert!(matches!(
            ideal_playback_db(0.0),
            Err(RiaaError::InvalidFrequency { .. })
        ));
    }
}
