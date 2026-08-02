use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::trace_admission::{
    certify_groove_trace_representation, GrooveTraceAdmissionBinding,
    GrooveTraceAdmissionCertificate, GrooveTraceAdmissionError, GrooveTraceAdmissionLevel,
    GrooveTraceEdgeCoverage, GrooveTraceRepresentationKind,
    ValidatedGrooveTraceAdmissionCertificate, CONTIGUOUS_TRACE_REPRESENTATION_FORMAT_VERSION,
};
use super::{RiaaConfig, RiaaRecordFilter, StylusGeometry};
use crate::resampler::adaptive_sample;

const MIN_RADIUS_M: f64 = 0.03;
const MAX_RADIUS_M: f64 = 0.20;
const MIN_SAMPLE_RATE_HZ: f64 = 8_000.0;
const MAX_SAMPLE_RATE_HZ: f64 = 768_000.0;
const MIN_GROOVE_PITCH_M_PER_REVOLUTION: f64 = 20.0e-6;
const MAX_GROOVE_PITCH_M_PER_REVOLUTION: f64 = 2.0e-3;
const MIN_CUTTER_DIMENSION_M: f64 = 1.0e-6;
const MAX_CUTTER_DIMENSION_M: f64 = 500.0e-6;
const SQRT_2: f64 = std::f64::consts::SQRT_2;

pub const GROOVE_ASSET_FORMAT_VERSION: u32 = CONTIGUOUS_TRACE_REPRESENTATION_FORMAT_VERSION;
pub const GROOVE_CONTENT_IDENTITY_VERSION: u32 = 4;
pub const GROOVE_SPATIAL_PYRAMID_FORMAT_VERSION: u32 = 1;
pub const GROOVE_SPATIAL_PYRAMID_LEVELS: usize = 4;
const GROOVE_SPATIAL_DECIMATION_FILTER_RADIUS: u32 = 32;
/// The largest pyramid kernel reaches this many source frames from its center.
pub const GROOVE_SPATIAL_FILTER_RADIUS_FRAMES: u32 =
    GROOVE_SPATIAL_DECIMATION_FILTER_RADIUS * ((1 << GROOVE_SPATIAL_PYRAMID_LEVELS) - 1);

/// Symmetric coefficients for offsets zero through 32.
///
/// The filter keeps 0.20 cycles per input frame within 1.2 percent. It rejects
/// the 0.25-to-0.50 cycles-per-frame stopband by at least 59 dB.
pub(crate) const GROOVE_SPATIAL_DECIMATION_COEFFICIENTS: [f64; 33] = [
    4.428_171_744_467_482e-1,
    3.127_211_016_310_901e-1,
    5.593_188_671_501_963e-2,
    -8.911_536_994_600_117e-2,
    -5.094_508_469_759_113e-2,
    3.717_767_613_478_482e-2,
    4.338_770_485_648_104e-2,
    -1.209_318_524_546_747_4e-2,
    -3.424_283_826_693_659_5e-2,
    -1.886_258_513_871_952_8e-3,
    2.462_575_149_425_354_6e-2,
    9.186_702_198_493_069e-3,
    -1.559_217_227_701_167_7e-2,
    -1.192_218_547_395_385_2e-2,
    7.973_644_851_276_999e-3,
    1.159_170_475_303_402_4e-2,
    -2.269_548_053_323_093_5e-3,
    -9.455_322_466_135_344e-3,
    -1.385_366_253_159_790_5e-3,
    6.569_491_002_902_621e-3,
    3.181_320_507_974_482_4e-3,
    -3.737_826_459_833_403e-3,
    -3.536_999_309_777_814_4e-3,
    1.471_580_967_475_256_3e-3,
    2.976_252_960_867_272e-3,
    8.434_343_501_728_16e-6,
    -2.006_378_421_135_840_7e-3,
    -7.225_311_134_582_802e-4,
    1.025_135_003_785_896_9e-3,
    8.508_076_189_363_661e-4,
    -2.653_896_391_728_101_6e-4,
    -6.370_553_635_578_468e-4,
    -2.742_707_628_628_425e-4,
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveSpatialLevel {
    first_source_frame: u64,
    source_frame_step: u32,
    lateral_displacement_m: Box<[f32]>,
    vertical_displacement_m: Box<[f32]>,
}

impl GrooveSpatialLevel {
    pub fn first_source_frame(&self) -> u64 {
        self.first_source_frame
    }

    pub fn source_frame_step(&self) -> u32 {
        self.source_frame_step
    }

    pub fn lateral_displacement_m(&self) -> &[f32] {
        &self.lateral_displacement_m
    }

    pub fn vertical_displacement_m(&self) -> &[f32] {
        &self.vertical_displacement_m
    }

    fn view(&self) -> GrooveSpatialLevelView<'_> {
        GrooveSpatialLevelView {
            first_source_frame: self.first_source_frame,
            source_frame_step: self.source_frame_step,
            lateral_displacement_m: &self.lateral_displacement_m,
            vertical_displacement_m: &self.vertical_displacement_m,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveSpatialPyramid {
    format_version: u32,
    levels: Box<[GrooveSpatialLevel]>,
}

impl GrooveSpatialPyramid {
    pub fn build(
        lateral_displacement_m: &[f32],
        vertical_displacement_m: &[f32],
    ) -> Result<Self, GrooveError> {
        Self::build_window(0, lateral_displacement_m, vertical_displacement_m)
    }

    pub(crate) fn build_window(
        first_source_frame: u64,
        lateral_displacement_m: &[f32],
        vertical_displacement_m: &[f32],
    ) -> Result<Self, GrooveError> {
        if lateral_displacement_m.len() != vertical_displacement_m.len() {
            return Err(GrooveError::ChannelLengthMismatch);
        }
        if lateral_displacement_m.len() < 4 {
            return Err(GrooveError::InsufficientFrames);
        }
        if lateral_displacement_m
            .iter()
            .chain(vertical_displacement_m)
            .any(|sample| !sample.is_finite())
        {
            return Err(GrooveError::NonfiniteDisplacement);
        }

        let mut levels: Vec<GrooveSpatialLevel> = Vec::with_capacity(GROOVE_SPATIAL_PYRAMID_LEVELS);
        for _ in 0..GROOVE_SPATIAL_PYRAMID_LEVELS {
            let (current_first, current_step, current_lateral, current_vertical) =
                if let Some(level) = levels.last() {
                    (
                        level.first_source_frame,
                        level.source_frame_step,
                        level.lateral_displacement_m.as_ref(),
                        level.vertical_displacement_m.as_ref(),
                    )
                } else {
                    (
                        first_source_frame,
                        1,
                        lateral_displacement_m,
                        vertical_displacement_m,
                    )
                };
            let next_step = current_step * 2;
            let next_first = align_up(current_first, u64::from(next_step))
                .ok_or(GrooveError::InvalidSpatialPyramid)?;
            if current_lateral.is_empty() {
                levels.push(GrooveSpatialLevel {
                    first_source_frame: next_first,
                    source_frame_step: next_step,
                    lateral_displacement_m: Box::new([]),
                    vertical_displacement_m: Box::new([]),
                });
                continue;
            }
            let current_span = u64::try_from(current_lateral.len().saturating_sub(1))
                .map_err(|_| GrooveError::InvalidSpatialPyramid)?
                .checked_mul(u64::from(current_step))
                .ok_or(GrooveError::InvalidSpatialPyramid)?;
            let current_last = current_first
                .checked_add(current_span)
                .ok_or(GrooveError::InvalidSpatialPyramid)?;
            let next_count = if next_first > current_last {
                0
            } else {
                usize::try_from((current_last - next_first) / u64::from(next_step) + 1)
                    .map_err(|_| GrooveError::InvalidSpatialPyramid)?
            };
            let center_offset = if next_count == 0 {
                0
            } else {
                usize::try_from((next_first - current_first) / u64::from(current_step))
                    .map_err(|_| GrooveError::InvalidSpatialPyramid)?
            };
            let next_lateral = filter_and_decimate(current_lateral, center_offset, next_count);
            let next_vertical = filter_and_decimate(current_vertical, center_offset, next_count);
            levels.push(GrooveSpatialLevel {
                first_source_frame: next_first,
                source_frame_step: next_step,
                lateral_displacement_m: next_lateral.into_boxed_slice(),
                vertical_displacement_m: next_vertical.into_boxed_slice(),
            });
        }
        Ok(Self {
            format_version: GROOVE_SPATIAL_PYRAMID_FORMAT_VERSION,
            levels: levels.into_boxed_slice(),
        })
    }

    pub fn format_version(&self) -> u32 {
        self.format_version
    }

    pub fn levels(&self) -> &[GrooveSpatialLevel] {
        &self.levels
    }

    pub(crate) fn validate_against_window(
        &self,
        first_source_frame: u64,
        lateral_displacement_m: &[f32],
        vertical_displacement_m: &[f32],
    ) -> Result<(), GrooveError> {
        let expected = Self::build_window(
            first_source_frame,
            lateral_displacement_m,
            vertical_displacement_m,
        )?;
        if self != &expected {
            return Err(GrooveError::SpatialPyramidMismatch);
        }
        Ok(())
    }

    pub(crate) fn select<'a>(
        &'a self,
        base_first_source_frame: u64,
        base_lateral_displacement_m: &'a [f32],
        base_vertical_displacement_m: &'a [f32],
        source_frame_advance: f64,
    ) -> Result<GrooveSpatialLevelSelection<'a>, GrooveError> {
        if !source_frame_advance.is_finite() {
            return Err(GrooveError::InvalidSourceFrameAdvance);
        }
        let base = GrooveSpatialLevelView {
            first_source_frame: base_first_source_frame,
            source_frame_step: 1,
            lateral_displacement_m: base_lateral_displacement_m,
            vertical_displacement_m: base_vertical_displacement_m,
        };
        let available_levels = self
            .levels
            .iter()
            .take_while(|level| level.lateral_displacement_m.len() >= 4)
            .count();
        if available_levels == 0 || source_frame_advance.abs() <= 1.0 {
            return Ok(GrooveSpatialLevelSelection {
                lower: base,
                upper: base,
                upper_level_blend: 0.0,
            });
        }
        let octave = source_frame_advance
            .abs()
            .log2()
            .clamp(0.0, available_levels as f64);
        let lower_index = octave.floor() as usize;
        let upper_index = octave.ceil() as usize;
        let view_for_index = |index: usize| {
            if index == 0 {
                base
            } else {
                self.levels[index - 1].view()
            }
        };
        Ok(GrooveSpatialLevelSelection {
            lower: view_for_index(lower_index),
            upper: view_for_index(upper_index),
            upper_level_blend: octave - lower_index as f64,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GrooveSpatialLevelView<'a> {
    first_source_frame: u64,
    source_frame_step: u32,
    lateral_displacement_m: &'a [f32],
    vertical_displacement_m: &'a [f32],
}

impl<'a> GrooveSpatialLevelView<'a> {
    pub fn first_source_frame(self) -> u64 {
        self.first_source_frame
    }

    pub fn source_frame_step(self) -> u32 {
        self.source_frame_step
    }

    pub fn lateral_displacement_m(self) -> &'a [f32] {
        self.lateral_displacement_m
    }

    pub fn vertical_displacement_m(self) -> &'a [f32] {
        self.vertical_displacement_m
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GrooveSpatialLevelSelection<'a> {
    lower: GrooveSpatialLevelView<'a>,
    upper: GrooveSpatialLevelView<'a>,
    upper_level_blend: f64,
}

impl<'a> GrooveSpatialLevelSelection<'a> {
    pub fn lower(self) -> GrooveSpatialLevelView<'a> {
        self.lower
    }

    pub fn upper(self) -> GrooveSpatialLevelView<'a> {
        self.upper
    }

    pub fn upper_level_blend(self) -> f64 {
        self.upper_level_blend
    }
}

pub(crate) fn align_up(value: u64, alignment: u64) -> Option<u64> {
    let remainder = value % alignment;
    if remainder == 0 {
        Some(value)
    } else {
        value.checked_add(alignment - remainder)
    }
}

fn filter_and_decimate(input: &[f32], center_offset: usize, output_count: usize) -> Vec<f32> {
    let mut output = Vec::with_capacity(output_count);
    for output_index in 0..output_count {
        let center = center_offset + output_index * 2;
        let mut filtered = GROOVE_SPATIAL_DECIMATION_COEFFICIENTS[0] * f64::from(input[center]);
        for (offset, coefficient) in GROOVE_SPATIAL_DECIMATION_COEFFICIENTS
            .iter()
            .copied()
            .enumerate()
            .skip(1)
        {
            let left = f64::from(input[center.saturating_sub(offset)]);
            let right = f64::from(input[center.saturating_add(offset).min(input.len() - 1)]);
            filtered += coefficient * (left + right);
        }
        output.push(filtered as f32);
    }
    output
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveLayout {
    pub outer_program_radius_m: f64,
    pub inner_program_radius_m: f64,
    pub nominal_rpm: f64,
    pub groove_sample_rate_hz: f64,
}

impl GrooveLayout {
    pub fn lp_33_seed() -> Self {
        Self {
            outer_program_radius_m: 0.146_05,
            inner_program_radius_m: 0.060_325,
            nominal_rpm: 33.333_333_333_333_336,
            groove_sample_rate_hz: 192_000.0,
        }
    }

    pub fn validate(self) -> Result<Self, GrooveError> {
        for (field, radius) in [
            ("outerProgramRadiusM", self.outer_program_radius_m),
            ("innerProgramRadiusM", self.inner_program_radius_m),
        ] {
            if !radius.is_finite() || !(MIN_RADIUS_M..=MAX_RADIUS_M).contains(&radius) {
                return Err(GrooveError::InvalidRadius { field });
            }
        }
        if self.inner_program_radius_m >= self.outer_program_radius_m {
            return Err(GrooveError::InvalidRadiusOrder);
        }
        if !self.nominal_rpm.is_finite() || self.nominal_rpm <= 0.0 {
            return Err(GrooveError::InvalidNominalRpm);
        }
        if !self.groove_sample_rate_hz.is_finite()
            || !(MIN_SAMPLE_RATE_HZ..=MAX_SAMPLE_RATE_HZ).contains(&self.groove_sample_rate_hz)
        {
            return Err(GrooveError::InvalidSampleRate);
        }
        Ok(self)
    }

    pub fn nominal_angular_velocity_rad_s(self) -> f64 {
        self.nominal_rpm * std::f64::consts::TAU / 60.0
    }

    pub fn unclamped_radius_at_frame(self, frame: f64, groove_pitch_m_per_revolution: f64) -> f64 {
        let nonnegative_frame = if frame.is_finite() {
            frame.max(0.0)
        } else {
            0.0
        };
        let revolutions =
            nonnegative_frame * self.nominal_rpm / (60.0 * self.groove_sample_rate_hz);
        self.outer_program_radius_m - revolutions * groove_pitch_m_per_revolution
    }

    pub fn radius_at_frame(self, frame: f64, groove_pitch_m_per_revolution: f64) -> f64 {
        self.unclamped_radius_at_frame(frame, groove_pitch_m_per_revolution)
            .clamp(self.inner_program_radius_m, self.outer_program_radius_m)
    }

    pub fn meters_per_frame_at(self, frame: f64, groove_pitch_m_per_revolution: f64) -> f64 {
        self.nominal_angular_velocity_rad_s()
            * self.radius_at_frame(frame, groove_pitch_m_per_revolution)
            / self.groove_sample_rate_hz
    }
}

impl Default for GrooveLayout {
    fn default() -> Self {
        Self::lp_33_seed()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveClearanceFramePair {
    pub outer_frame: u64,
    /// This is the lower interpolation frame when one revolution is fractional.
    pub inner_frame: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveCutReport {
    pub peak_left_velocity_m_s: f64,
    pub peak_right_velocity_m_s: f64,
    pub rms_left_velocity_m_s: f64,
    pub rms_right_velocity_m_s: f64,
    pub peak_lateral_displacement_m: f64,
    pub peak_vertical_displacement_m: f64,
    pub final_lateral_drift_m: f64,
    pub final_vertical_drift_m: f64,
    pub groove_pitch_m_per_revolution: f64,
    pub final_program_radius_m: f64,
    pub programme_exceeds_available_radius: bool,
    /// This is the smallest edge-to-edge land between adjacent turns.
    pub minimum_adjacent_turn_clearance_m: Option<f64>,
    pub first_failing_clearance_frame_pair: Option<GrooveClearanceFramePair>,
    pub adjacent_turn_clearance_failed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordCutConfig {
    pub full_scale_sine_velocity_rms_m_s: f64,
    pub cutter_highpass_hz: f64,
    pub cutter_bandwidth_hz: f64,
    /// Constant radial feed for this seed cut.
    pub groove_pitch_m_per_revolution: f64,
    /// Estimated total groove width at the record surface.
    pub groove_top_width_m: f64,
    /// Estimated minimum uncut land between adjacent groove edges.
    pub minimum_land_width_m: f64,
}

impl RecordCutConfig {
    pub fn seed() -> Self {
        Self {
            full_scale_sine_velocity_rms_m_s: 0.05,
            cutter_highpass_hz: 20.0,
            cutter_bandwidth_hz: 50_000.0,
            groove_pitch_m_per_revolution: 125.0e-6,
            groove_top_width_m: 50.0e-6,
            minimum_land_width_m: 20.0e-6,
        }
    }

    pub fn validate(self, sample_rate_hz: f64) -> Result<Self, GrooveError> {
        if !self.full_scale_sine_velocity_rms_m_s.is_finite()
            || self.full_scale_sine_velocity_rms_m_s <= 0.0
        {
            return Err(GrooveError::InvalidCutSetting {
                field: "fullScaleSineVelocityRmsMS",
            });
        }
        if !self.cutter_highpass_hz.is_finite()
            || self.cutter_highpass_hz <= 0.0
            || self.cutter_highpass_hz >= sample_rate_hz * 0.5
        {
            return Err(GrooveError::InvalidCutSetting {
                field: "cutterHighpassHz",
            });
        }
        RiaaConfig::new(sample_rate_hz, self.cutter_bandwidth_hz).map_err(|_| {
            GrooveError::InvalidCutSetting {
                field: "cutterBandwidthHz",
            }
        })?;
        validate_groove_pitch(self.groove_pitch_m_per_revolution)?;
        for (field, value) in [
            ("grooveTopWidthM", self.groove_top_width_m),
            ("minimumLandWidthM", self.minimum_land_width_m),
        ] {
            if !value.is_finite()
                || !(MIN_CUTTER_DIMENSION_M..=MAX_CUTTER_DIMENSION_M).contains(&value)
            {
                return Err(GrooveError::InvalidCutSetting { field });
            }
        }
        Ok(self)
    }
}

impl Default for RecordCutConfig {
    fn default() -> Self {
        Self::seed()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GrooveSourceKind {
    Pcm,
    StereoWallVelocity,
    SpatialDisplacement,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveSourceProvenance {
    kind: GrooveSourceKind,
    source_sample_rate_hz: Option<f64>,
    source_channel_count: Option<u8>,
    source_frame_count: u64,
}

impl GrooveSourceProvenance {
    pub fn kind(self) -> GrooveSourceKind {
        self.kind
    }

    pub fn source_sample_rate_hz(self) -> Option<f64> {
        self.source_sample_rate_hz
    }

    pub fn source_channel_count(self) -> Option<u8> {
        self.source_channel_count
    }

    pub fn source_frame_count(self) -> u64 {
        self.source_frame_count
    }

    fn pcm(sample_rate_hz: f64, channel_count: usize, frame_count: usize) -> Self {
        Self::pcm_frames(sample_rate_hz, channel_count as u8, frame_count as u64)
    }

    pub(crate) fn pcm_frames(sample_rate_hz: f64, channel_count: u8, frame_count: u64) -> Self {
        Self {
            kind: GrooveSourceKind::Pcm,
            source_sample_rate_hz: Some(sample_rate_hz),
            source_channel_count: Some(channel_count),
            source_frame_count: frame_count,
        }
    }

    fn wall_velocity(frame_count: usize) -> Self {
        Self {
            kind: GrooveSourceKind::StereoWallVelocity,
            source_sample_rate_hz: None,
            source_channel_count: None,
            source_frame_count: frame_count as u64,
        }
    }

    fn displacement(frame_count: usize) -> Self {
        Self {
            kind: GrooveSourceKind::SpatialDisplacement,
            source_sample_rate_hz: None,
            source_channel_count: None,
            source_frame_count: frame_count as u64,
        }
    }

    fn validate(
        self,
        output_frame_count: usize,
        output_sample_rate_hz: f64,
    ) -> Result<(), GrooveError> {
        if self.source_frame_count < 4 {
            return Err(GrooveError::InvalidProvenance);
        }
        match self.kind {
            GrooveSourceKind::Pcm => {
                let Some(sample_rate_hz) = self.source_sample_rate_hz else {
                    return Err(GrooveError::InvalidProvenance);
                };
                if !sample_rate_hz.is_finite() || sample_rate_hz <= 0.0 {
                    return Err(GrooveError::InvalidProvenance);
                }
                if !matches!(self.source_channel_count, Some(1 | 2)) {
                    return Err(GrooveError::InvalidProvenance);
                }
                let source_step = sample_rate_hz / output_sample_rate_hz;
                let expected_output_frames =
                    (((self.source_frame_count - 1) as f64 / source_step).floor() as usize).max(1);
                if expected_output_frames != output_frame_count {
                    return Err(GrooveError::InvalidProvenance);
                }
            }
            GrooveSourceKind::StereoWallVelocity | GrooveSourceKind::SpatialDisplacement => {
                if self.source_sample_rate_hz.is_some()
                    || self.source_channel_count.is_some()
                    || self.source_frame_count != output_frame_count as u64
                {
                    return Err(GrooveError::InvalidProvenance);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveContentIdentity {
    identity_version: u32,
    sha256: [u8; 32],
}

impl GrooveContentIdentity {
    /// Creates a versioned identity from a SHA-256 digest supplied by a trusted source.
    pub fn from_sha256(sha256: [u8; 32]) -> Self {
        Self {
            identity_version: GROOVE_CONTENT_IDENTITY_VERSION,
            sha256,
        }
    }

    pub fn identity_version(self) -> u32 {
        self.identity_version
    }

    pub fn sha256(self) -> [u8; 32] {
        self.sha256
    }

    pub(crate) fn validate_current(self) -> Result<(), GrooveError> {
        if self.identity_version != GROOVE_CONTENT_IDENTITY_VERSION {
            return Err(GrooveError::ContentIdentityMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveCutProvenance {
    source: GrooveSourceProvenance,
    cut: RecordCutConfig,
    content_identity: GrooveContentIdentity,
}

impl GrooveCutProvenance {
    pub fn source(self) -> GrooveSourceProvenance {
        self.source
    }

    pub fn cut(self) -> RecordCutConfig {
        self.cut
    }

    pub fn content_identity(self) -> GrooveContentIdentity {
        self.content_identity
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveAsset {
    format_version: u32,
    layout: GrooveLayout,
    lateral_displacement_m: Vec<f32>,
    vertical_displacement_m: Vec<f32>,
    spatial_pyramid: GrooveSpatialPyramid,
    trace_admission_certificate: Option<GrooveTraceAdmissionCertificate>,
    #[serde(skip)]
    validated_trace_admission: Option<ValidatedGrooveTraceAdmissionCertificate>,
    report: GrooveCutReport,
    provenance: GrooveCutProvenance,
}

impl GrooveAsset {
    /// Bakes decoded programme PCM into a spatial groove outside rendering.
    pub fn cut_from_pcm(
        channels: &[&[f32]],
        source_sample_rate_hz: f64,
        layout: GrooveLayout,
        cut: RecordCutConfig,
    ) -> Result<Self, GrooveError> {
        let layout = layout.validate()?;
        let cut = cut.validate(layout.groove_sample_rate_hz)?;
        if !source_sample_rate_hz.is_finite() || source_sample_rate_hz <= 0.0 {
            return Err(GrooveError::InvalidSourceSampleRate);
        }
        if !(1..=2).contains(&channels.len()) {
            return Err(GrooveError::InvalidSourceChannelCount);
        }
        let source_frames = channels[0].len();
        if source_frames < 4 {
            return Err(GrooveError::InsufficientFrames);
        }
        if channels
            .iter()
            .any(|channel| channel.len() != source_frames)
        {
            return Err(GrooveError::ChannelLengthMismatch);
        }
        if channels
            .iter()
            .flat_map(|channel| channel.iter())
            .any(|sample| !sample.is_finite())
        {
            return Err(GrooveError::NonfiniteProgramme);
        }

        let mut left = resample_offline(
            channels[0],
            source_sample_rate_hz,
            layout.groove_sample_rate_hz,
        );
        let mut right = if channels.len() == 2 {
            resample_offline(
                channels[1],
                source_sample_rate_hz,
                layout.groove_sample_rate_hz,
            )
        } else {
            left.clone()
        };
        let frame_count = left.len().min(right.len());
        left.truncate(frame_count);
        right.truncate(frame_count);
        if frame_count < 4 {
            return Err(GrooveError::InsufficientFrames);
        }

        let riaa_config = RiaaConfig::new(layout.groove_sample_rate_hz, cut.cutter_bandwidth_hz)
            .map_err(|_| GrooveError::InvalidCutSetting {
                field: "cutterBandwidthHz",
            })?;
        let mut left_riaa = RiaaRecordFilter::new(riaa_config);
        let mut right_riaa = RiaaRecordFilter::new(riaa_config);
        let mut left_highpass =
            CutterHighpass::new(cut.cutter_highpass_hz, layout.groove_sample_rate_hz);
        let mut right_highpass = left_highpass;
        let velocity_scale = cut.full_scale_sine_velocity_rms_m_s * std::f64::consts::SQRT_2;
        for frame in 0..frame_count {
            let left_programme = left_highpass.process(left[frame] as f64);
            let right_programme = right_highpass.process(right[frame] as f64);
            left[frame] = (left_riaa.process_sample_f64(left_programme) * velocity_scale) as f32;
            right[frame] = (right_riaa.process_sample_f64(right_programme) * velocity_scale) as f32;
        }
        Self::integrate_stereo_wall_velocity_m_s(
            &left,
            &right,
            layout,
            cut,
            GrooveSourceProvenance::pcm(source_sample_rate_hz, channels.len(), source_frames),
        )
    }

    /// Integrates RIAA-encoded wall velocities into spatial groove displacement.
    pub fn from_stereo_wall_velocity_m_s(
        left_velocity_m_s: &[f32],
        right_velocity_m_s: &[f32],
        layout: GrooveLayout,
    ) -> Result<Self, GrooveError> {
        Self::from_stereo_wall_velocity_m_s_with_pitch(
            left_velocity_m_s,
            right_velocity_m_s,
            layout,
            RecordCutConfig::default().groove_pitch_m_per_revolution,
        )
    }

    pub fn from_stereo_wall_velocity_m_s_with_pitch(
        left_velocity_m_s: &[f32],
        right_velocity_m_s: &[f32],
        layout: GrooveLayout,
        groove_pitch_m_per_revolution: f64,
    ) -> Result<Self, GrooveError> {
        let cut = RecordCutConfig {
            groove_pitch_m_per_revolution,
            ..RecordCutConfig::default()
        };
        Self::from_stereo_wall_velocity_m_s_with_cut(
            left_velocity_m_s,
            right_velocity_m_s,
            layout,
            cut,
        )
    }

    pub fn from_stereo_wall_velocity_m_s_with_cut(
        left_velocity_m_s: &[f32],
        right_velocity_m_s: &[f32],
        layout: GrooveLayout,
        cut: RecordCutConfig,
    ) -> Result<Self, GrooveError> {
        let source = GrooveSourceProvenance::wall_velocity(left_velocity_m_s.len());
        Self::integrate_stereo_wall_velocity_m_s(
            left_velocity_m_s,
            right_velocity_m_s,
            layout,
            cut,
            source,
        )
    }

    fn integrate_stereo_wall_velocity_m_s(
        left_velocity_m_s: &[f32],
        right_velocity_m_s: &[f32],
        layout: GrooveLayout,
        cut: RecordCutConfig,
        source: GrooveSourceProvenance,
    ) -> Result<Self, GrooveError> {
        let layout = layout.validate()?;
        let cut = cut.validate(layout.groove_sample_rate_hz)?;
        if left_velocity_m_s.len() != right_velocity_m_s.len() {
            return Err(GrooveError::ChannelLengthMismatch);
        }
        if left_velocity_m_s.len() < 4 {
            return Err(GrooveError::InsufficientFrames);
        }
        if left_velocity_m_s
            .iter()
            .chain(right_velocity_m_s)
            .any(|sample| !sample.is_finite())
        {
            return Err(GrooveError::NonfiniteVelocity);
        }

        let frame_count = left_velocity_m_s.len();
        let dt = 1.0 / layout.groove_sample_rate_hz;
        let mut lateral_displacement_m = Vec::with_capacity(frame_count);
        let mut vertical_displacement_m = Vec::with_capacity(frame_count);
        let mut lateral_position = 0.0_f64;
        let mut vertical_position = 0.0_f64;
        let (mut previous_lateral_velocity, mut previous_vertical_velocity) =
            encode_45_45(left_velocity_m_s[0] as f64, right_velocity_m_s[0] as f64);
        lateral_displacement_m.push(0.0);
        vertical_displacement_m.push(0.0);
        let mut peak_left_velocity_m_s = left_velocity_m_s[0].abs() as f64;
        let mut peak_right_velocity_m_s = right_velocity_m_s[0].abs() as f64;
        let mut sum_left_velocity_squared = (left_velocity_m_s[0] as f64).powi(2);
        let mut sum_right_velocity_squared = (right_velocity_m_s[0] as f64).powi(2);
        let mut peak_lateral_displacement_m = 0.0_f64;
        let mut peak_vertical_displacement_m = 0.0_f64;

        for frame in 1..frame_count {
            let left_velocity = left_velocity_m_s[frame] as f64;
            let right_velocity = right_velocity_m_s[frame] as f64;
            let (lateral_velocity, vertical_velocity) = encode_45_45(left_velocity, right_velocity);
            lateral_position += 0.5 * (previous_lateral_velocity + lateral_velocity) * dt;
            vertical_position += 0.5 * (previous_vertical_velocity + vertical_velocity) * dt;
            lateral_displacement_m.push(lateral_position as f32);
            vertical_displacement_m.push(vertical_position as f32);
            peak_left_velocity_m_s = peak_left_velocity_m_s.max(left_velocity.abs());
            peak_right_velocity_m_s = peak_right_velocity_m_s.max(right_velocity.abs());
            sum_left_velocity_squared += left_velocity * left_velocity;
            sum_right_velocity_squared += right_velocity * right_velocity;
            peak_lateral_displacement_m = peak_lateral_displacement_m.max(lateral_position.abs());
            peak_vertical_displacement_m =
                peak_vertical_displacement_m.max(vertical_position.abs());
            previous_lateral_velocity = lateral_velocity;
            previous_vertical_velocity = vertical_velocity;
        }

        let final_program_radius_m = layout
            .unclamped_radius_at_frame((frame_count - 1) as f64, cut.groove_pitch_m_per_revolution);
        let clearance = adjacent_turn_clearance(&lateral_displacement_m, layout, cut);
        let report = GrooveCutReport {
            peak_left_velocity_m_s,
            peak_right_velocity_m_s,
            rms_left_velocity_m_s: (sum_left_velocity_squared / frame_count as f64).sqrt(),
            rms_right_velocity_m_s: (sum_right_velocity_squared / frame_count as f64).sqrt(),
            peak_lateral_displacement_m,
            peak_vertical_displacement_m,
            final_lateral_drift_m: lateral_position,
            final_vertical_drift_m: vertical_position,
            groove_pitch_m_per_revolution: cut.groove_pitch_m_per_revolution,
            final_program_radius_m,
            programme_exceeds_available_radius: final_program_radius_m
                < layout.inner_program_radius_m,
            minimum_adjacent_turn_clearance_m: clearance.minimum_clearance_m,
            first_failing_clearance_frame_pair: clearance.first_failing_frame_pair,
            adjacent_turn_clearance_failed: clearance.first_failing_frame_pair.is_some(),
        };
        Self::from_validated_parts(
            layout,
            cut,
            source,
            lateral_displacement_m,
            vertical_displacement_m,
            report,
        )
    }

    pub fn from_displacement_m(
        lateral_displacement_m: Vec<f32>,
        vertical_displacement_m: Vec<f32>,
        layout: GrooveLayout,
        groove_pitch_m_per_revolution: f64,
        report: GrooveCutReport,
    ) -> Result<Self, GrooveError> {
        let cut = RecordCutConfig {
            groove_pitch_m_per_revolution,
            ..RecordCutConfig::default()
        };
        Self::from_displacement_m_with_cut(
            lateral_displacement_m,
            vertical_displacement_m,
            layout,
            cut,
            report,
        )
    }

    pub fn from_displacement_m_with_cut(
        lateral_displacement_m: Vec<f32>,
        vertical_displacement_m: Vec<f32>,
        layout: GrooveLayout,
        cut: RecordCutConfig,
        report: GrooveCutReport,
    ) -> Result<Self, GrooveError> {
        let layout = layout.validate()?;
        let cut = cut.validate(layout.groove_sample_rate_hz)?;
        if lateral_displacement_m.len() != vertical_displacement_m.len() {
            return Err(GrooveError::ChannelLengthMismatch);
        }
        if lateral_displacement_m.len() < 4 {
            return Err(GrooveError::InsufficientFrames);
        }
        if lateral_displacement_m
            .iter()
            .chain(&vertical_displacement_m)
            .any(|sample| !sample.is_finite())
        {
            return Err(GrooveError::NonfiniteDisplacement);
        }
        let source = GrooveSourceProvenance::displacement(lateral_displacement_m.len());
        Self::from_validated_parts(
            layout,
            cut,
            source,
            lateral_displacement_m,
            vertical_displacement_m,
            report,
        )
    }

    fn from_validated_parts(
        layout: GrooveLayout,
        cut: RecordCutConfig,
        source: GrooveSourceProvenance,
        lateral_displacement_m: Vec<f32>,
        vertical_displacement_m: Vec<f32>,
        report: GrooveCutReport,
    ) -> Result<Self, GrooveError> {
        let spatial_pyramid =
            GrooveSpatialPyramid::build(&lateral_displacement_m, &vertical_displacement_m)?;
        let mut asset = Self {
            format_version: GROOVE_ASSET_FORMAT_VERSION,
            layout,
            lateral_displacement_m,
            vertical_displacement_m,
            spatial_pyramid,
            trace_admission_certificate: None,
            validated_trace_admission: None,
            report,
            provenance: GrooveCutProvenance {
                source,
                cut,
                content_identity: GrooveContentIdentity {
                    identity_version: GROOVE_CONTENT_IDENTITY_VERSION,
                    sha256: [0; 32],
                },
            },
        };
        asset.provenance.content_identity = asset.calculate_content_identity();
        asset.trace_admission_certificate = Some(asset.calculate_trace_admission_certificate()?);
        asset.validate_with_pyramid_check(false)?;
        asset.validated_trace_admission = Some(asset.validated_trace_admission()?);
        Ok(asset)
    }

    fn validate(&self) -> Result<(), GrooveError> {
        self.validate_with_pyramid_check(true)
    }

    fn validate_with_pyramid_check(
        &self,
        rebuild_spatial_pyramid: bool,
    ) -> Result<(), GrooveError> {
        if self.format_version != GROOVE_ASSET_FORMAT_VERSION {
            return Err(GrooveError::UnsupportedAssetFormatVersion {
                version: self.format_version,
            });
        }
        let layout = self.layout.validate()?;
        let cut = self.provenance.cut.validate(layout.groove_sample_rate_hz)?;
        if self.lateral_displacement_m.len() != self.vertical_displacement_m.len() {
            return Err(GrooveError::ChannelLengthMismatch);
        }
        if self.lateral_displacement_m.len() < 4 {
            return Err(GrooveError::InsufficientFrames);
        }
        if self
            .lateral_displacement_m
            .iter()
            .chain(&self.vertical_displacement_m)
            .any(|sample| !sample.is_finite())
        {
            return Err(GrooveError::NonfiniteDisplacement);
        }
        self.provenance.source.validate(
            self.lateral_displacement_m.len(),
            layout.groove_sample_rate_hz,
        )?;
        if rebuild_spatial_pyramid {
            self.spatial_pyramid.validate_against_window(
                0,
                &self.lateral_displacement_m,
                &self.vertical_displacement_m,
            )?;
        }
        let certificate = self
            .trace_admission_certificate
            .ok_or(GrooveError::MissingTraceAdmissionCertificate)?;
        certificate.validate_recomputed(self.calculate_trace_admission_certificate()?)?;
        validate_cut_report(self.report, &self.lateral_displacement_m, layout, cut)?;
        if self.provenance.content_identity.identity_version != GROOVE_CONTENT_IDENTITY_VERSION
            || self.provenance.content_identity != self.calculate_content_identity()
        {
            return Err(GrooveError::ContentIdentityMismatch);
        }
        Ok(())
    }

    fn calculate_content_identity(&self) -> GrooveContentIdentity {
        compute_content_identity(
            self.format_version,
            self.layout,
            self.provenance.source,
            self.provenance.cut,
            self.report,
            &self.lateral_displacement_m,
            &self.vertical_displacement_m,
        )
    }

    fn calculate_trace_admission_certificate(
        &self,
    ) -> Result<GrooveTraceAdmissionCertificate, GrooveTraceAdmissionError> {
        let levels = self.spatial_pyramid.levels();
        if levels.len() != GROOVE_SPATIAL_PYRAMID_LEVELS {
            return Err(GrooveTraceAdmissionError::InvalidRepresentation);
        }
        let spatial_levels = std::array::from_fn(|index| GrooveTraceAdmissionLevel {
            first_source_frame: levels[index].first_source_frame,
            source_frame_step: levels[index].source_frame_step,
            lateral_displacement_m: &levels[index].lateral_displacement_m,
            vertical_displacement_m: &levels[index].vertical_displacement_m,
        });
        let final_frame = self.lateral_displacement_m.len().saturating_sub(1) as f64;
        certify_groove_trace_representation(
            GrooveTraceAdmissionBinding {
                representation_kind: GrooveTraceRepresentationKind::Contiguous,
                representation_format_version: GROOVE_ASSET_FORMAT_VERSION,
                source_content_identity: self.provenance.content_identity,
                generation: 0,
                core_start_frame: 0,
                core_end_frame_exclusive: self.lateral_displacement_m.len() as u64,
                stored_start_frame: 0,
                stored_end_frame_exclusive: self.lateral_displacement_m.len() as u64,
                record_end_frame_exclusive: self.lateral_displacement_m.len() as u64,
                minimum_meters_per_source_frame: self.layout.meters_per_frame_at(
                    final_frame,
                    self.provenance.cut.groove_pitch_m_per_revolution,
                ),
                maximum_geometry: StylusGeometry::default(),
                edge_coverage: GrooveTraceEdgeCoverage::contiguous(),
            },
            GrooveTraceAdmissionLevel {
                first_source_frame: 0,
                source_frame_step: 1,
                lateral_displacement_m: &self.lateral_displacement_m,
                vertical_displacement_m: &self.vertical_displacement_m,
            },
            &spatial_levels,
        )
    }

    pub fn format_version(&self) -> u32 {
        self.format_version
    }

    pub fn provenance(&self) -> GrooveCutProvenance {
        self.provenance
    }

    pub fn layout(&self) -> GrooveLayout {
        self.layout
    }

    pub fn frame_count(&self) -> usize {
        self.lateral_displacement_m.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lateral_displacement_m.is_empty()
    }

    pub fn lateral_displacement_m(&self) -> &[f32] {
        &self.lateral_displacement_m
    }

    pub fn vertical_displacement_m(&self) -> &[f32] {
        &self.vertical_displacement_m
    }

    pub fn spatial_pyramid(&self) -> &GrooveSpatialPyramid {
        &self.spatial_pyramid
    }

    pub fn trace_admission_certificate(&self) -> GrooveTraceAdmissionCertificate {
        self.trace_admission_certificate
            .expect("a validated groove asset has a trace-admission certificate")
    }

    pub fn trace_admission_identity(&self) -> GrooveContentIdentity {
        let mut hash = GrooveContentHasher::new(b"record-player-trace-admitted-source-v1\0");
        hash.identity(self.provenance.content_identity);
        hash.identity(self.trace_admission_certificate().certificate_identity());
        hash.finish()
    }

    pub(crate) fn validated_trace_admission(
        &self,
    ) -> Result<ValidatedGrooveTraceAdmissionCertificate, GrooveTraceAdmissionError> {
        if let Some(validated) = self.validated_trace_admission {
            return Ok(validated);
        }
        self.trace_admission_certificate()
            .validate_recomputed(self.calculate_trace_admission_certificate()?)
    }

    /// Selects two immutable levels for one render-step source advance.
    pub fn spatial_level_selection(
        &self,
        source_frame_advance: f64,
    ) -> Result<GrooveSpatialLevelSelection<'_>, GrooveError> {
        self.spatial_pyramid.select(
            0,
            &self.lateral_displacement_m,
            &self.vertical_displacement_m,
            source_frame_advance,
        )
    }

    pub fn report(&self) -> GrooveCutReport {
        self.report
    }

    pub fn radius_at_frame(&self, frame: f64) -> f64 {
        self.layout
            .radius_at_frame(frame, self.provenance.cut.groove_pitch_m_per_revolution)
    }

    pub fn meters_per_frame_at(&self, frame: f64) -> f64 {
        self.layout
            .meters_per_frame_at(frame, self.provenance.cut.groove_pitch_m_per_revolution)
    }

    pub fn groove_pitch_m_per_revolution(&self) -> f64 {
        self.provenance.cut.groove_pitch_m_per_revolution
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrooveAssetWire {
    format_version: u32,
    layout: GrooveLayout,
    lateral_displacement_m: Vec<f32>,
    vertical_displacement_m: Vec<f32>,
    spatial_pyramid: GrooveSpatialPyramid,
    trace_admission_certificate: GrooveTraceAdmissionCertificate,
    report: GrooveCutReport,
    provenance: GrooveCutProvenance,
}

impl<'de> Deserialize<'de> for GrooveAsset {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = GrooveAssetWire::deserialize(deserializer)?;
        let mut asset = Self {
            format_version: wire.format_version,
            layout: wire.layout,
            lateral_displacement_m: wire.lateral_displacement_m,
            vertical_displacement_m: wire.vertical_displacement_m,
            spatial_pyramid: wire.spatial_pyramid,
            trace_admission_certificate: Some(wire.trace_admission_certificate),
            validated_trace_admission: None,
            report: wire.report,
            provenance: wire.provenance,
        };
        asset.validate().map_err(serde::de::Error::custom)?;
        asset.validated_trace_admission = Some(
            asset
                .validated_trace_admission()
                .map_err(serde::de::Error::custom)?,
        );
        Ok(asset)
    }
}

pub fn encode_45_45(left: f64, right: f64) -> (f64, f64) {
    ((left + right) / SQRT_2, (left - right) / SQRT_2)
}

pub fn decode_45_45(lateral: f64, vertical: f64) -> (f64, f64) {
    ((lateral + vertical) / SQRT_2, (lateral - vertical) / SQRT_2)
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum GrooveError {
    #[error("groove asset format version {version} is not supported")]
    UnsupportedAssetFormatVersion { version: u32 },
    #[error("{field} is outside the supported record radius")]
    InvalidRadius { field: &'static str },
    #[error("inner program radius must be less than outer program radius")]
    InvalidRadiusOrder,
    #[error("nominal RPM must be finite and positive")]
    InvalidNominalRpm,
    #[error("groove sample rate is outside the supported range")]
    InvalidSampleRate,
    #[error("groove channels must have equal lengths")]
    ChannelLengthMismatch,
    #[error("groove requires at least four frames")]
    InsufficientFrames,
    #[error("wall velocity contains a nonfinite sample")]
    NonfiniteVelocity,
    #[error("groove displacement contains a nonfinite sample")]
    NonfiniteDisplacement,
    #[error("source sample rate must be finite and positive")]
    InvalidSourceSampleRate,
    #[error("source must have one or two channels")]
    InvalidSourceChannelCount,
    #[error("programme PCM contains a nonfinite sample")]
    NonfiniteProgramme,
    #[error("record cut setting {field} is invalid")]
    InvalidCutSetting { field: &'static str },
    #[error("groove cut report is inconsistent or contains a nonfinite value")]
    InvalidCutReport,
    #[error("groove cut provenance is inconsistent")]
    InvalidProvenance,
    #[error("groove content identity does not match the asset")]
    ContentIdentityMismatch,
    #[error("groove asset has no trace-admission certificate")]
    MissingTraceAdmissionCertificate,
    #[error("spatial groove pyramid is invalid")]
    InvalidSpatialPyramid,
    #[error("spatial groove pyramid does not match the source groove")]
    SpatialPyramidMismatch,
    #[error("source-frame advance must be finite")]
    InvalidSourceFrameAdvance,
    #[error(transparent)]
    TraceAdmission(#[from] GrooveTraceAdmissionError),
}

fn validate_groove_pitch(value: f64) -> Result<(), GrooveError> {
    if value.is_finite()
        && (MIN_GROOVE_PITCH_M_PER_REVOLUTION..=MAX_GROOVE_PITCH_M_PER_REVOLUTION).contains(&value)
    {
        Ok(())
    } else {
        Err(GrooveError::InvalidCutSetting {
            field: "groovePitchMPerRevolution",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AdjacentTurnClearance {
    minimum_clearance_m: Option<f64>,
    first_failing_frame_pair: Option<GrooveClearanceFramePair>,
}

fn adjacent_turn_clearance(
    lateral_displacement_m: &[f32],
    layout: GrooveLayout,
    cut: RecordCutConfig,
) -> AdjacentTurnClearance {
    let frames_per_revolution = layout.groove_sample_rate_hz * 60.0 / layout.nominal_rpm;
    let last_frame = lateral_displacement_m.len().saturating_sub(1) as f64;
    if !frames_per_revolution.is_finite()
        || frames_per_revolution <= 0.0
        || frames_per_revolution > last_frame
    {
        return AdjacentTurnClearance {
            minimum_clearance_m: None,
            first_failing_frame_pair: None,
        };
    }

    let final_outer_frame = (last_frame - frames_per_revolution).floor() as usize;
    let mut minimum_clearance_m = f64::INFINITY;
    let mut first_failing_frame_pair = None;
    for outer_frame in 0..=final_outer_frame {
        let inner_frame_position = outer_frame as f64 + frames_per_revolution;
        let outer_centerline_m = layout
            .unclamped_radius_at_frame(outer_frame as f64, cut.groove_pitch_m_per_revolution)
            + f64::from(lateral_displacement_m[outer_frame]);
        let inner_centerline_m = layout
            .unclamped_radius_at_frame(inner_frame_position, cut.groove_pitch_m_per_revolution)
            + sample_linear(lateral_displacement_m, inner_frame_position);
        let clearance_m = outer_centerline_m - inner_centerline_m - cut.groove_top_width_m;
        minimum_clearance_m = minimum_clearance_m.min(clearance_m);
        if first_failing_frame_pair.is_none() && clearance_m < cut.minimum_land_width_m {
            first_failing_frame_pair = Some(GrooveClearanceFramePair {
                outer_frame: outer_frame as u64,
                inner_frame: inner_frame_position.floor() as u64,
            });
        }
    }
    AdjacentTurnClearance {
        minimum_clearance_m: Some(minimum_clearance_m),
        first_failing_frame_pair,
    }
}

fn sample_linear(samples: &[f32], position: f64) -> f64 {
    let lower = position.floor() as usize;
    let upper = (lower + 1).min(samples.len() - 1);
    let fraction = position - lower as f64;
    f64::from(samples[lower]) + (f64::from(samples[upper]) - f64::from(samples[lower])) * fraction
}

fn validate_cut_report(
    report: GrooveCutReport,
    lateral_displacement_m: &[f32],
    layout: GrooveLayout,
    cut: RecordCutConfig,
) -> Result<(), GrooveError> {
    let scalar_values_are_finite = [
        report.peak_left_velocity_m_s,
        report.peak_right_velocity_m_s,
        report.rms_left_velocity_m_s,
        report.rms_right_velocity_m_s,
        report.peak_lateral_displacement_m,
        report.peak_vertical_displacement_m,
        report.final_lateral_drift_m,
        report.final_vertical_drift_m,
        report.groove_pitch_m_per_revolution,
        report.final_program_radius_m,
    ]
    .iter()
    .all(|value| value.is_finite())
        && report
            .minimum_adjacent_turn_clearance_m
            .is_none_or(f64::is_finite);
    if !scalar_values_are_finite
        || report.peak_left_velocity_m_s < 0.0
        || report.peak_right_velocity_m_s < 0.0
        || report.rms_left_velocity_m_s < 0.0
        || report.rms_right_velocity_m_s < 0.0
        || report.rms_left_velocity_m_s > report.peak_left_velocity_m_s * (1.0 + 1.0e-9)
        || report.rms_right_velocity_m_s > report.peak_right_velocity_m_s * (1.0 + 1.0e-9)
        || report.peak_lateral_displacement_m < 0.0
        || report.peak_vertical_displacement_m < 0.0
        || report.groove_pitch_m_per_revolution != cut.groove_pitch_m_per_revolution
    {
        return Err(GrooveError::InvalidCutReport);
    }

    let expected_final_radius = layout.unclamped_radius_at_frame(
        lateral_displacement_m.len().saturating_sub(1) as f64,
        cut.groove_pitch_m_per_revolution,
    );
    if (report.final_program_radius_m - expected_final_radius).abs() > 1.0e-12
        || report.programme_exceeds_available_radius
            != (expected_final_radius < layout.inner_program_radius_m)
    {
        return Err(GrooveError::InvalidCutReport);
    }

    let expected_clearance = adjacent_turn_clearance(lateral_displacement_m, layout, cut);
    let clearance_matches = match (
        report.minimum_adjacent_turn_clearance_m,
        expected_clearance.minimum_clearance_m,
    ) {
        (Some(actual), Some(expected)) => (actual - expected).abs() <= 1.0e-15,
        (None, None) => true,
        _ => false,
    };
    if !clearance_matches
        || report.first_failing_clearance_frame_pair != expected_clearance.first_failing_frame_pair
        || report.adjacent_turn_clearance_failed
            != expected_clearance.first_failing_frame_pair.is_some()
    {
        return Err(GrooveError::InvalidCutReport);
    }
    Ok(())
}

fn compute_content_identity(
    format_version: u32,
    layout: GrooveLayout,
    source: GrooveSourceProvenance,
    cut: RecordCutConfig,
    report: GrooveCutReport,
    lateral_displacement_m: &[f32],
    vertical_displacement_m: &[f32],
) -> GrooveContentIdentity {
    let frame_count = lateral_displacement_m.len() as u64;
    let mut lateral_hash = begin_groove_lateral_content_hash(frame_count);
    for sample in lateral_displacement_m {
        lateral_hash.f32(*sample);
    }
    let mut vertical_hash = begin_groove_vertical_content_hash(frame_count);
    for sample in vertical_displacement_m {
        vertical_hash.f32(*sample);
    }
    finalize_groove_content_identity(
        format_version,
        layout,
        source,
        cut,
        report,
        lateral_hash.finish(),
        vertical_hash.finish(),
    )
}

pub(crate) fn begin_groove_lateral_content_hash(frame_count: u64) -> GrooveContentHasher {
    let mut hash = GrooveContentHasher::new(b"record-player-groove-lateral-v3\0");
    hash.u64(frame_count);
    hash
}

pub(crate) fn begin_groove_vertical_content_hash(frame_count: u64) -> GrooveContentHasher {
    let mut hash = GrooveContentHasher::new(b"record-player-groove-vertical-v3\0");
    hash.u64(frame_count);
    hash
}

pub(crate) fn finalize_groove_content_identity(
    format_version: u32,
    layout: GrooveLayout,
    source: GrooveSourceProvenance,
    cut: RecordCutConfig,
    report: GrooveCutReport,
    lateral_content_identity: GrooveContentIdentity,
    vertical_content_identity: GrooveContentIdentity,
) -> GrooveContentIdentity {
    let mut hash = GrooveContentHasher::new(b"record-player-groove-asset-v3\0");
    hash.u32(format_version);
    for value in [
        layout.outer_program_radius_m,
        layout.inner_program_radius_m,
        layout.nominal_rpm,
        layout.groove_sample_rate_hz,
    ] {
        hash.f64(value);
    }
    hash.u8(match source.kind {
        GrooveSourceKind::Pcm => 0,
        GrooveSourceKind::StereoWallVelocity => 1,
        GrooveSourceKind::SpatialDisplacement => 2,
    });
    match source.source_sample_rate_hz {
        Some(value) => {
            hash.u8(1);
            hash.f64(value);
        }
        None => hash.u8(0),
    }
    match source.source_channel_count {
        Some(value) => {
            hash.u8(1);
            hash.u8(value);
        }
        None => hash.u8(0),
    }
    hash.u64(source.source_frame_count);
    for value in [
        cut.full_scale_sine_velocity_rms_m_s,
        cut.cutter_highpass_hz,
        cut.cutter_bandwidth_hz,
        cut.groove_pitch_m_per_revolution,
        cut.groove_top_width_m,
        cut.minimum_land_width_m,
        report.peak_left_velocity_m_s,
        report.peak_right_velocity_m_s,
        report.rms_left_velocity_m_s,
        report.rms_right_velocity_m_s,
        report.peak_lateral_displacement_m,
        report.peak_vertical_displacement_m,
        report.final_lateral_drift_m,
        report.final_vertical_drift_m,
        report.groove_pitch_m_per_revolution,
        report.final_program_radius_m,
    ] {
        hash.f64(value);
    }
    hash.bool(report.programme_exceeds_available_radius);
    hash.bool(report.adjacent_turn_clearance_failed);
    match report.minimum_adjacent_turn_clearance_m {
        Some(value) => {
            hash.u8(1);
            hash.f64(value);
        }
        None => hash.u8(0),
    }
    match report.first_failing_clearance_frame_pair {
        Some(pair) => {
            hash.u8(1);
            hash.u64(pair.outer_frame);
            hash.u64(pair.inner_frame);
        }
        None => hash.u8(0),
    }
    hash.identity(lateral_content_identity);
    hash.identity(vertical_content_identity);
    hash.finish()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Sha256 {
    state: [u32; 8],
    #[serde(with = "sha256_buffer_serde")]
    buffer: [u8; 64],
    buffer_len: usize,
    message_bit_len: u64,
}

mod sha256_buffer_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(buffer: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(buffer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        bytes
            .try_into()
            .map_err(|value: Vec<u8>| serde::de::Error::invalid_length(value.len(), &"64 bytes"))
    }
}

/// Writes canonical, domain-separated values into a groove SHA-256 identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct GrooveContentHasher(Sha256);

impl GrooveContentHasher {
    pub(crate) fn new(domain: &[u8]) -> Self {
        let mut hash = Sha256::new();
        hash.update(domain);
        Self(hash)
    }

    pub(crate) fn bytes(&mut self, value: &[u8]) {
        self.0.update(value);
    }

    pub(crate) fn bool(&mut self, value: bool) {
        self.bytes(&[u8::from(value)]);
    }

    pub(crate) fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    pub(crate) fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    pub(crate) fn f32(&mut self, value: f32) {
        self.u32(value.to_bits());
    }

    pub(crate) fn f64(&mut self, value: f64) {
        self.u64(value.to_bits());
    }

    pub(crate) fn identity(&mut self, value: GrooveContentIdentity) {
        self.u32(value.identity_version);
        self.bytes(&value.sha256);
    }

    pub(crate) fn finish(self) -> GrooveContentIdentity {
        GrooveContentIdentity::from_sha256(self.finish_sha256())
    }

    pub(crate) fn finish_sha256(self) -> [u8; 32] {
        self.0.finalize()
    }

    pub(crate) fn is_valid(&self) -> bool {
        self.0.buffer_len < self.0.buffer.len()
            && self.0.message_bit_len.is_multiple_of(8)
            && (self.0.message_bit_len / 8) % self.0.buffer.len() as u64 == self.0.buffer_len as u64
    }

    pub(crate) fn byte_len(&self) -> u64 {
        self.0.message_bit_len / 8
    }
}

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09_e667,
                0xbb67_ae85,
                0x3c6e_f372,
                0xa54f_f53a,
                0x510e_527f,
                0x9b05_688c,
                0x1f83_d9ab,
                0x5be0_cd19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            message_bit_len: 0,
        }
    }

    fn update(&mut self, mut bytes: &[u8]) {
        self.message_bit_len = self
            .message_bit_len
            .wrapping_add((bytes.len() as u64).wrapping_mul(8));
        while !bytes.is_empty() {
            let available = 64 - self.buffer_len;
            let copied = available.min(bytes.len());
            self.buffer[self.buffer_len..self.buffer_len + copied]
                .copy_from_slice(&bytes[..copied]);
            self.buffer_len += copied;
            bytes = &bytes[copied..];
            if self.buffer_len == 64 {
                let block = self.buffer;
                self.compress(block);
                self.buffer_len = 0;
            }
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;
        if self.buffer_len > 56 {
            self.buffer[self.buffer_len..].fill(0);
            let block = self.buffer;
            self.compress(block);
            self.buffer = [0; 64];
        } else {
            self.buffer[self.buffer_len..56].fill(0);
        }
        self.buffer[56..64].copy_from_slice(&self.message_bit_len.to_be_bytes());
        let block = self.buffer;
        self.compress(block);

        let mut digest = [0; 32];
        for (index, value) in self.state.iter().enumerate() {
            digest[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
        }
        digest
    }

    fn compress(&mut self, block: [u8; 64]) {
        const K: [u32; 64] = [
            0x428a_2f98,
            0x7137_4491,
            0xb5c0_fbcf,
            0xe9b5_dba5,
            0x3956_c25b,
            0x59f1_11f1,
            0x923f_82a4,
            0xab1c_5ed5,
            0xd807_aa98,
            0x1283_5b01,
            0x2431_85be,
            0x550c_7dc3,
            0x72be_5d74,
            0x80de_b1fe,
            0x9bdc_06a7,
            0xc19b_f174,
            0xe49b_69c1,
            0xefbe_4786,
            0x0fc1_9dc6,
            0x240c_a1cc,
            0x2de9_2c6f,
            0x4a74_84aa,
            0x5cb0_a9dc,
            0x76f9_88da,
            0x983e_5152,
            0xa831_c66d,
            0xb003_27c8,
            0xbf59_7fc7,
            0xc6e0_0bf3,
            0xd5a7_9147,
            0x06ca_6351,
            0x1429_2967,
            0x27b7_0a85,
            0x2e1b_2138,
            0x4d2c_6dfc,
            0x5338_0d13,
            0x650a_7354,
            0x766a_0abb,
            0x81c2_c92e,
            0x9272_2c85,
            0xa2bf_e8a1,
            0xa81a_664b,
            0xc24b_8b70,
            0xc76c_51a3,
            0xd192_e819,
            0xd699_0624,
            0xf40e_3585,
            0x106a_a070,
            0x19a4_c116,
            0x1e37_6c08,
            0x2748_774c,
            0x34b0_bcb5,
            0x391c_0cb3,
            0x4ed8_aa4a,
            0x5b9c_ca4f,
            0x682e_6ff3,
            0x748f_82ee,
            0x78a5_636f,
            0x84c8_7814,
            0x8cc7_0208,
            0x90be_fffa,
            0xa450_6ceb,
            0xbef9_a3f7,
            0xc671_78f2,
        ];

        let mut schedule = [0_u32; 64];
        for (index, word) in schedule.iter_mut().take(16).enumerate() {
            let offset = index * 4;
            *word = u32::from_be_bytes([
                block[offset],
                block[offset + 1],
                block[offset + 2],
                block[offset + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = schedule[index - 15].rotate_right(7)
                ^ schedule[index - 15].rotate_right(18)
                ^ (schedule[index - 15] >> 3);
            let s1 = schedule[index - 2].rotate_right(17)
                ^ schedule[index - 2].rotate_right(19)
                ^ (schedule[index - 2] >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(s0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temporary1 = h
                .wrapping_add(sum1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(schedule[index]);
            let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temporary2 = sum0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temporary1);
            d = c;
            c = b;
            b = a;
            a = temporary1.wrapping_add(temporary2);
        }
        for (state, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *state = state.wrapping_add(value);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(super) struct CutterHighpass {
    coefficient: f64,
    state: f64,
}

impl CutterHighpass {
    pub(super) fn new(cutoff_hz: f64, sample_rate_hz: f64) -> Self {
        let g = (std::f64::consts::PI * cutoff_hz / sample_rate_hz).tan();
        Self {
            coefficient: g / (1.0 + g),
            state: 0.0,
        }
    }

    pub(super) fn process(&mut self, input: f64) -> f64 {
        let integrator_input = (input - self.state) * self.coefficient;
        let lowpass = integrator_input + self.state;
        self.state = lowpass + integrator_input;
        input - lowpass
    }

    pub(super) fn is_valid(self) -> bool {
        self.coefficient.is_finite()
            && self.coefficient > 0.0
            && self.coefficient < 1.0
            && self.state.is_finite()
    }

    pub(super) fn configuration_matches(self, cutoff_hz: f64, sample_rate_hz: f64) -> bool {
        self.coefficient == Self::new(cutoff_hz, sample_rate_hz).coefficient
    }
}

fn resample_offline(input: &[f32], input_rate_hz: f64, output_rate_hz: f64) -> Vec<f32> {
    let source_step = input_rate_hz / output_rate_hz;
    let frame_count = (((input.len() - 1) as f64 / source_step).floor() as usize).max(1);
    let mut output = Vec::with_capacity(frame_count);
    for frame in 0..frame_count {
        let position = frame as f64 * source_step;
        output.push(adaptive_sample(input, position, source_step).unwrap_or(0.0) as f32);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_round_trip_preserves_channels_and_energy() {
        for (left, right) in [(1.0, 0.0), (0.25, -0.8), (-0.4, -0.7)] {
            let (lateral, vertical) = encode_45_45(left, right);
            let decoded = decode_45_45(lateral, vertical);
            assert!((decoded.0 - left).abs() < 1.0e-12);
            assert!((decoded.1 - right).abs() < 1.0e-12);
            assert!(
                (left * left + right * right - lateral * lateral - vertical * vertical).abs()
                    < 1.0e-12
            );
        }
    }

    #[test]
    fn mono_is_lateral_and_antiphase_is_vertical() {
        let mono = encode_45_45(0.5, 0.5);
        assert!((mono.0 - 1.0 / SQRT_2).abs() < 1.0e-12);
        assert!(mono.1.abs() < 1.0e-12);
        let antiphase = encode_45_45(0.5, -0.5);
        assert!(antiphase.0.abs() < 1.0e-12);
        assert!((antiphase.1 - 1.0 / SQRT_2).abs() < 1.0e-12);
    }

    #[test]
    fn velocity_integration_preserves_linear_motion() {
        let layout = GrooveLayout::default();
        let velocity = vec![0.05_f32; 193];
        let silence = vec![0.0_f32; velocity.len()];
        let groove =
            GrooveAsset::from_stereo_wall_velocity_m_s(&velocity, &silence, layout).unwrap();
        let elapsed = (velocity.len() - 1) as f64 / layout.groove_sample_rate_hz;
        let expected_lateral = 0.05 * elapsed / SQRT_2;
        let final_lateral = *groove.lateral_displacement_m().last().unwrap() as f64;
        let final_vertical = *groove.vertical_displacement_m().last().unwrap() as f64;
        assert!((final_lateral - expected_lateral).abs() < 1.0e-9);
        assert!((final_vertical - expected_lateral).abs() < 1.0e-9);
    }

    #[test]
    fn radius_and_spatial_step_decrease_toward_the_inner_groove() {
        let layout = GrooveLayout::default();
        let pitch = 125.0e-6;
        assert_eq!(
            layout.radius_at_frame(0.0, pitch),
            layout.outer_program_radius_m
        );
        let frames_to_inner = (layout.outer_program_radius_m - layout.inner_program_radius_m)
            / pitch
            * 60.0
            * layout.groove_sample_rate_hz
            / layout.nominal_rpm;
        assert!(
            (layout.radius_at_frame(frames_to_inner, pitch) - layout.inner_program_radius_m).abs()
                < f64::EPSILON
        );
        assert!(
            layout.meters_per_frame_at(0.0, pitch)
                > layout.meters_per_frame_at(frames_to_inner, pitch)
        );
        assert!(layout.radius_at_frame(100.0, pitch) > layout.radius_at_frame(99_000.0, pitch));
    }

    #[test]
    fn adjacent_turn_clearance_distinguishes_vertical_and_lateral_modulation() {
        let layout = GrooveLayout::default();
        let frames_per_revolution =
            (layout.groove_sample_rate_hz * 60.0 / layout.nominal_rpm).round() as usize;
        let frame_count = frames_per_revolution + 1_024;
        let positive = vec![40.0e-6_f32; frame_count];
        let negative = vec![-40.0e-6_f32; frame_count];

        let vertical =
            GrooveAsset::from_stereo_wall_velocity_m_s(&positive, &negative, layout).unwrap();
        let vertical_report = vertical.report();
        assert!(vertical_report.peak_vertical_displacement_m > 80.0e-6);
        assert!(vertical_report.peak_lateral_displacement_m < 1.0e-12);
        assert!(!vertical_report.adjacent_turn_clearance_failed);
        assert!(vertical_report.first_failing_clearance_frame_pair.is_none());
        let expected_land = RecordCutConfig::default().groove_pitch_m_per_revolution
            - RecordCutConfig::default().groove_top_width_m;
        assert!(
            (vertical_report.minimum_adjacent_turn_clearance_m.unwrap() - expected_land).abs()
                < 1.0e-12
        );

        let lateral =
            GrooveAsset::from_stereo_wall_velocity_m_s(&positive, &positive, layout).unwrap();
        let lateral_report = lateral.report();
        assert!(lateral_report.adjacent_turn_clearance_failed);
        assert!(
            lateral_report.minimum_adjacent_turn_clearance_m.unwrap()
                < RecordCutConfig::default().minimum_land_width_m
        );
        assert_eq!(
            lateral_report.first_failing_clearance_frame_pair,
            Some(GrooveClearanceFramePair {
                outer_frame: 0,
                inner_frame: frames_per_revolution as u64,
            })
        );
    }

    #[test]
    fn one_kilohertz_full_scale_sine_cuts_at_the_declared_rms_velocity() {
        let sample_rate = 48_000.0;
        let frequency = 1_000.0;
        let frames = sample_rate as usize;
        let programme: Vec<f32> = (0..frames)
            .map(|frame| {
                (std::f64::consts::TAU * frequency * frame as f64 / sample_rate).sin() as f32
            })
            .collect();
        let groove = GrooveAsset::cut_from_pcm(
            &[&programme],
            sample_rate,
            GrooveLayout::default(),
            RecordCutConfig::default(),
        )
        .unwrap();
        assert!((groove.report().rms_left_velocity_m_s - 0.05).abs() < 8.0e-4);
        assert!((groove.report().rms_right_velocity_m_s - 0.05).abs() < 8.0e-4);
        assert_eq!(groove.frame_count(), 191_996);
    }

    #[test]
    fn cutter_highpass_prevents_unbounded_dc_displacement() {
        let programme = vec![0.5_f32; 48_000];
        let groove = GrooveAsset::cut_from_pcm(
            &[&programme],
            48_000.0,
            GrooveLayout::default(),
            RecordCutConfig::default(),
        )
        .unwrap();
        assert!(groove.report().final_lateral_drift_m.abs() < 0.001);
        assert!(groove.report().final_vertical_drift_m.abs() < 1.0e-12);
    }

    #[test]
    fn pcm_cut_preserves_exact_source_and_cut_provenance() {
        let sample_rate_hz = 48_000.0;
        let programme: Vec<f32> = (0..4_800)
            .map(|frame| {
                (std::f64::consts::TAU * 997.0 * frame as f64 / sample_rate_hz).sin() as f32
            })
            .collect();
        let cut = RecordCutConfig {
            groove_top_width_m: 52.0e-6,
            minimum_land_width_m: 18.0e-6,
            ..RecordCutConfig::default()
        };
        let first =
            GrooveAsset::cut_from_pcm(&[&programme], sample_rate_hz, GrooveLayout::default(), cut)
                .unwrap();
        let second =
            GrooveAsset::cut_from_pcm(&[&programme], sample_rate_hz, GrooveLayout::default(), cut)
                .unwrap();
        let provenance = first.provenance();
        assert_eq!(first.format_version(), GROOVE_ASSET_FORMAT_VERSION);
        assert_eq!(provenance.source().kind(), GrooveSourceKind::Pcm);
        assert_eq!(
            provenance.source().source_sample_rate_hz(),
            Some(sample_rate_hz)
        );
        assert_eq!(provenance.source().source_channel_count(), Some(1));
        assert_eq!(
            provenance.source().source_frame_count(),
            programme.len() as u64
        );
        assert_eq!(provenance.cut(), cut);
        assert_eq!(
            provenance.content_identity(),
            second.provenance().content_identity()
        );

        let mut changed_programme = programme;
        changed_programme[1_000] += 0.125;
        let changed = GrooveAsset::cut_from_pcm(
            &[&changed_programme],
            sample_rate_hz,
            GrooveLayout::default(),
            cut,
        )
        .unwrap();
        assert_ne!(
            provenance.content_identity(),
            changed.provenance().content_identity()
        );
    }

    #[test]
    fn deserialization_revalidates_version_provenance_cut_and_identity() {
        let velocity = vec![0.01_f32; 512];
        let asset = GrooveAsset::from_stereo_wall_velocity_m_s(
            &velocity,
            &velocity,
            GrooveLayout::default(),
        )
        .unwrap();
        let value = serde_json::to_value(&asset).unwrap();
        let round_trip: GrooveAsset = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(round_trip, asset);

        let mut wrong_version = value.clone();
        wrong_version["formatVersion"] = serde_json::json!(99);
        assert!(serde_json::from_value::<GrooveAsset>(wrong_version).is_err());

        let mut invalid_cut = value.clone();
        invalid_cut["provenance"]["cut"]["grooveTopWidthM"] = serde_json::json!(0.0);
        assert!(serde_json::from_value::<GrooveAsset>(invalid_cut).is_err());

        let mut invalid_source = value.clone();
        invalid_source["provenance"]["source"]["sourceFrameCount"] = serde_json::json!(0);
        assert!(serde_json::from_value::<GrooveAsset>(invalid_source).is_err());

        let mut wrong_identity = value;
        wrong_identity["provenance"]["contentIdentity"]["sha256"][0] = serde_json::json!(255);
        assert!(serde_json::from_value::<GrooveAsset>(wrong_identity).is_err());
    }

    #[test]
    fn deserialization_requires_the_current_pyramid_and_trace_certificate() {
        let velocity = vec![0.01_f32; 512];
        let asset = GrooveAsset::from_stereo_wall_velocity_m_s(
            &velocity,
            &velocity,
            GrooveLayout::default(),
        )
        .unwrap();
        let mut value = serde_json::to_value(&asset).unwrap();
        value["formatVersion"] = serde_json::json!(1);
        assert!(serde_json::from_value::<GrooveAsset>(value).is_err());

        let mut missing_pyramid = serde_json::to_value(&asset).unwrap();
        missing_pyramid
            .as_object_mut()
            .unwrap()
            .remove("spatialPyramid");
        assert!(serde_json::from_value::<GrooveAsset>(missing_pyramid).is_err());

        let mut missing_certificate = serde_json::to_value(&asset).unwrap();
        missing_certificate
            .as_object_mut()
            .unwrap()
            .remove("traceAdmissionCertificate");
        assert!(serde_json::from_value::<GrooveAsset>(missing_certificate).is_err());
    }

    fn sample_level_at(level: GrooveSpatialLevelView<'_>, absolute_frame: f64) -> f64 {
        let position = (absolute_frame - level.first_source_frame() as f64)
            / f64::from(level.source_frame_step());
        let lower = position.floor().max(0.0) as usize;
        let upper = (lower + 1).min(level.lateral_displacement_m().len() - 1);
        let fraction = position - lower as f64;
        let lower_value = f64::from(level.lateral_displacement_m()[lower]);
        let upper_value = f64::from(level.lateral_displacement_m()[upper]);
        lower_value + (upper_value - lower_value) * fraction
    }

    fn sample_selection_at(selection: GrooveSpatialLevelSelection<'_>, absolute_frame: f64) -> f64 {
        let lower = sample_level_at(selection.lower(), absolute_frame);
        let upper = sample_level_at(selection.upper(), absolute_frame);
        lower + (upper - lower) * selection.upper_level_blend()
    }

    #[test]
    fn spatial_pyramid_rejects_stopband_sweeps_from_two_to_ten_times_speed() {
        let frame_count = 16_385;
        for frequency in [0.251_f64, 0.27, 0.31, 0.37, 0.43, 0.49] {
            let signal: Vec<f32> = (0..frame_count)
                .map(|frame| (std::f64::consts::TAU * frequency * frame as f64).sin() as f32)
                .collect();
            let zero = vec![0.0_f32; frame_count];
            let pyramid = GrooveSpatialPyramid::build(&signal, &zero).unwrap();
            for speed in [2.0_f64, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0] {
                let forward = pyramid.select(0, &signal, &zero, speed).unwrap();
                let reverse = pyramid.select(0, &signal, &zero, -speed).unwrap();
                let mut filtered_energy = 0.0;
                let mut source_energy = 0.0;
                for output_frame in 0..512 {
                    let position = 2_048.0 + output_frame as f64 * speed;
                    let forward_sample = sample_selection_at(forward, position);
                    let reverse_sample = sample_selection_at(reverse, position);
                    assert_eq!(forward_sample.to_bits(), reverse_sample.to_bits());
                    filtered_energy += forward_sample.powi(2);
                    source_energy += (std::f64::consts::TAU * frequency * position).sin().powi(2);
                }
                assert!(
                    filtered_energy < source_energy * 1.0e-4,
                    "{frequency} at {speed}x: {filtered_energy} >= {source_energy}"
                );
            }
        }
    }

    #[test]
    fn spatial_level_selection_is_exact_for_forward_and_reverse_advance() {
        let signal: Vec<f32> = (0..4_097)
            .map(|frame| {
                let phase = std::f64::consts::TAU * frame as f64 / 37.25;
                (phase.sin() + 0.2 * (2.7 * phase).cos()) as f32
            })
            .collect();
        let zero = vec![0.0_f32; signal.len()];
        let pyramid = GrooveSpatialPyramid::build(&signal, &zero).unwrap();
        for speed in [2.0_f64, 3.0, 4.0, 6.0, 8.0, 10.0] {
            let forward = pyramid.select(0, &signal, &zero, speed).unwrap();
            let reverse = pyramid.select(0, &signal, &zero, -speed).unwrap();
            assert_eq!(
                forward.upper_level_blend().to_bits(),
                reverse.upper_level_blend().to_bits()
            );
            assert_eq!(
                forward.lower().source_frame_step(),
                reverse.lower().source_frame_step()
            );
            assert_eq!(
                forward.upper().source_frame_step(),
                reverse.upper().source_frame_step()
            );
            for frame in 512..1_024 {
                assert_eq!(
                    sample_selection_at(forward, frame as f64).to_bits(),
                    sample_selection_at(reverse, frame as f64).to_bits()
                );
            }
        }
    }

    #[test]
    fn spatial_level_blends_are_continuous_at_octave_boundaries() {
        let signal: Vec<f32> = (0..4_097)
            .map(|frame| (std::f64::consts::TAU * frame as f64 / 23.75).sin() as f32)
            .collect();
        let zero = vec![0.0_f32; signal.len()];
        let pyramid = GrooveSpatialPyramid::build(&signal, &zero).unwrap();
        for boundary in [2.0_f64, 4.0, 8.0] {
            let below = pyramid
                .select(0, &signal, &zero, boundary * (1.0 - 1.0e-9))
                .unwrap();
            let exact = pyramid.select(0, &signal, &zero, boundary).unwrap();
            let above = pyramid
                .select(0, &signal, &zero, boundary * (1.0 + 1.0e-9))
                .unwrap();
            for frame in 1_024..1_088 {
                let exact_sample = sample_selection_at(exact, frame as f64);
                assert!((sample_selection_at(below, frame as f64) - exact_sample).abs() < 1.0e-8);
                assert!((sample_selection_at(above, frame as f64) - exact_sample).abs() < 1.0e-8);
            }
        }
    }

    #[test]
    fn deserialization_rejects_changed_spatial_pyramid_samples() {
        let velocity = vec![0.01_f32; 512];
        let mut asset = GrooveAsset::from_stereo_wall_velocity_m_s(
            &velocity,
            &velocity,
            GrooveLayout::default(),
        )
        .unwrap();
        let source_identity = asset.calculate_content_identity();
        asset.spatial_pyramid.levels[0].lateral_displacement_m[10] = 0.125;
        assert_eq!(asset.calculate_content_identity(), source_identity);
        assert_eq!(asset.validate(), Err(GrooveError::SpatialPyramidMismatch));
        let value = serde_json::to_value(asset).unwrap();
        assert!(serde_json::from_value::<GrooveAsset>(value).is_err());
    }

    #[test]
    fn content_identity_uses_standard_sha256() {
        let mut hash = Sha256::new();
        hash.update(b"abc");
        assert_eq!(
            hash.finalize(),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
    }
}
