use serde::{Deserialize, Serialize};
use std::mem::size_of;
use std::sync::Arc;
use thiserror::Error;

use super::groove::{
    align_up, GrooveContentHasher, GrooveContentIdentity, GrooveSpatialLevelSelection,
    GrooveSpatialPyramid, GROOVE_SPATIAL_FILTER_RADIUS_FRAMES,
    GROOVE_SPATIAL_PYRAMID_FORMAT_VERSION, GROOVE_SPATIAL_PYRAMID_LEVELS,
};
#[cfg(test)]
use super::stylus::trace_spherical_45_45_wall_multiresolution_contacts;
use super::stylus::{
    trace_spherical_45_45_wall_multiresolution_contacts_certified_concave,
    trace_spherical_45_45_wall_multiresolution_contacts_certified_piecewise, StylusGeometry,
    StylusTraceContactSet, StylusTraceError,
};
use super::trace_admission::{
    certify_groove_trace_representation, GrooveTraceAdmissionBinding,
    GrooveTraceAdmissionCertificate, GrooveTraceAdmissionClass, GrooveTraceAdmissionError,
    GrooveTraceAdmissionLevel, GrooveTraceAdmissionPolicy, GrooveTraceEdgeCoverage,
    GrooveTraceRepresentationKind, ValidatedGrooveTraceAdmissionCertificate,
    PAGED_TRACE_REPRESENTATION_FORMAT_VERSION,
};
use super::{GrooveAsset, GrooveCutReport, GrooveError, GrooveLayout, RecordCutConfig};

pub const PHYSICAL_GROOVE_SAMPLE_RATE_HZ: u32 = 192_000;
pub const PAGED_GROOVE_FORMAT_VERSION: u32 = PAGED_TRACE_REPRESENTATION_FORMAT_VERSION;

const MIN_TOTAL_FRAME_COUNT: u64 = 4;
pub const MAX_PAGED_GROOVE_TRACING_HALO_FRAMES: u32 = 4_096;
const REPORT_RADIUS_TOLERANCE_M: f64 = 1.0e-12;
const MAX_SPATIAL_LEVEL_STEP_FRAMES: u32 = 1 << GROOVE_SPATIAL_PYRAMID_LEVELS;
pub const PAGED_GROOVE_SPATIAL_STORAGE_MARGIN_FRAMES: u32 = GROOVE_SPATIAL_FILTER_RADIUS_FRAMES;
pub const MAX_PAGED_GROOVE_RENDER_SPEED: f64 = 20.0;
pub const MAX_PAGED_GROOVE_PREFETCH_CANDIDATES: usize = 1_024;

const MAX_CACHE_PAGE_COUNT: u32 = 4_096;
const MAX_CACHE_RESIDENT_BYTES: u64 = 8 * 1_024 * 1_024 * 1_024;

/// Identifies one immutable groove-data generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GrooveGenerationId(u64);

impl GrooveGenerationId {
    pub fn new(value: u64) -> Result<Self, PagedGrooveError> {
        if value == 0 {
            return Err(PagedGrooveError::InvalidGeneration);
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u64 {
        self.0
    }

    pub(crate) fn validate(self) -> Result<(), PagedGrooveError> {
        if self.0 == 0 {
            Err(PagedGrooveError::InvalidGeneration)
        } else {
            Ok(())
        }
    }
}

/// Defines an absolute half-open range in the 192 kHz groove coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveFrameRange {
    start_frame: u64,
    end_frame_exclusive: u64,
}

impl GrooveFrameRange {
    pub fn new(start_frame: u64, end_frame_exclusive: u64) -> Result<Self, PagedGrooveError> {
        if start_frame >= end_frame_exclusive {
            return Err(PagedGrooveError::InvalidFrameRange {
                start_frame,
                end_frame_exclusive,
            });
        }
        Ok(Self {
            start_frame,
            end_frame_exclusive,
        })
    }

    pub fn start_frame(self) -> u64 {
        self.start_frame
    }

    pub fn end_frame_exclusive(self) -> u64 {
        self.end_frame_exclusive
    }

    pub fn frame_count(self) -> u64 {
        self.end_frame_exclusive - self.start_frame
    }

    pub fn contains(self, frame: u64) -> bool {
        self.start_frame <= frame && frame < self.end_frame_exclusive
    }

    fn contains_range(self, other: Self) -> bool {
        self.start_frame <= other.start_frame
            && other.end_frame_exclusive <= self.end_frame_exclusive
    }

    fn validate(self) -> Result<(), PagedGrooveError> {
        if self.start_frame >= self.end_frame_exclusive {
            Err(PagedGrooveError::InvalidFrameRange {
                start_frame: self.start_frame,
                end_frame_exclusive: self.end_frame_exclusive,
            })
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GrooveCoordinateEncoding {
    LateralVertical45_45,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GrooveSampleEncoding {
    Float32Meters,
}

/// Defines the immutable binary interpretation of all pages in one asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalGrooveFormat {
    format_version: u32,
    sample_rate_hz: u32,
    coordinate_encoding: GrooveCoordinateEncoding,
    sample_encoding: GrooveSampleEncoding,
}

impl PhysicalGrooveFormat {
    pub fn physical_current() -> Self {
        Self {
            format_version: PAGED_GROOVE_FORMAT_VERSION,
            sample_rate_hz: PHYSICAL_GROOVE_SAMPLE_RATE_HZ,
            coordinate_encoding: GrooveCoordinateEncoding::LateralVertical45_45,
            sample_encoding: GrooveSampleEncoding::Float32Meters,
        }
    }

    pub fn format_version(self) -> u32 {
        self.format_version
    }

    pub fn sample_rate_hz(self) -> u32 {
        self.sample_rate_hz
    }

    pub fn coordinate_encoding(self) -> GrooveCoordinateEncoding {
        self.coordinate_encoding
    }

    pub fn sample_encoding(self) -> GrooveSampleEncoding {
        self.sample_encoding
    }

    fn validate(self) -> Result<(), PagedGrooveError> {
        if self != Self::physical_current() {
            return Err(PagedGrooveError::UnsupportedFormat);
        }
        Ok(())
    }
}

impl Default for PhysicalGrooveFormat {
    fn default() -> Self {
        Self::physical_current()
    }
}

/// Stores the physical cut configuration and the complete cut report.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalGrooveCutMetadata {
    layout: GrooveLayout,
    cut: RecordCutConfig,
    report: GrooveCutReport,
}

impl PhysicalGrooveCutMetadata {
    pub fn new(layout: GrooveLayout, cut: RecordCutConfig, report: GrooveCutReport) -> Self {
        Self {
            layout,
            cut,
            report,
        }
    }

    pub fn layout(self) -> GrooveLayout {
        self.layout
    }

    pub fn cut(self) -> RecordCutConfig {
        self.cut
    }

    pub fn report(self) -> GrooveCutReport {
        self.report
    }

    fn validate(self, total_frame_count: u64) -> Result<(), PagedGrooveError> {
        let layout = self.layout.validate()?;
        let cut = self
            .cut
            .validate(f64::from(PHYSICAL_GROOVE_SAMPLE_RATE_HZ))?;
        if layout.groove_sample_rate_hz != f64::from(PHYSICAL_GROOVE_SAMPLE_RATE_HZ) {
            return Err(PagedGrooveError::UnsupportedSampleRate);
        }
        if !cut_report_is_valid(self.report) {
            return Err(PagedGrooveError::InvalidCutReport);
        }
        if self.report.groove_pitch_m_per_revolution != cut.groove_pitch_m_per_revolution {
            return Err(PagedGrooveError::CutPitchMismatch);
        }
        let final_radius = layout.unclamped_radius_at_frame(
            total_frame_count.saturating_sub(1) as f64,
            cut.groove_pitch_m_per_revolution,
        );
        if (self.report.final_program_radius_m - final_radius).abs() > REPORT_RADIUS_TOLERANCE_M {
            return Err(PagedGrooveError::CutLengthMismatch);
        }
        let exceeds_radius = final_radius < layout.inner_program_radius_m;
        if self.report.programme_exceeds_available_radius != exceeds_radius {
            return Err(PagedGrooveError::InvalidCutReport);
        }
        let frames_per_revolution = layout.groove_sample_rate_hz * 60.0 / layout.nominal_rpm;
        let has_adjacent_turn = total_frame_count.saturating_sub(1) as f64 >= frames_per_revolution;
        if self.report.minimum_adjacent_turn_clearance_m.is_some() != has_adjacent_turn
            || self.report.adjacent_turn_clearance_failed
                != self.report.first_failing_clearance_frame_pair.is_some()
            || self.report.adjacent_turn_clearance_failed
                != self
                    .report
                    .minimum_adjacent_turn_clearance_m
                    .is_some_and(|clearance| clearance < cut.minimum_land_width_m)
        {
            return Err(PagedGrooveError::InvalidCutReport);
        }
        if let Some(pair) = self.report.first_failing_clearance_frame_pair {
            if pair.outer_frame >= total_frame_count
                || pair.inner_frame >= total_frame_count
                || pair.outer_frame >= pair.inner_frame
                || self.report.minimum_adjacent_turn_clearance_m.is_none()
            {
                return Err(PagedGrooveError::InvalidCutReport);
            }
        }
        Ok(())
    }
}

/// Describes one complete record independently from the pages currently present.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalGrooveMetadata {
    generation: GrooveGenerationId,
    format: PhysicalGrooveFormat,
    source_content_identity: GrooveContentIdentity,
    content_identity: GrooveContentIdentity,
    cut: PhysicalGrooveCutMetadata,
    total_frame_count: u64,
    tracing_halo_frames: u32,
    trace_admission_policy: GrooveTraceAdmissionPolicy,
}

impl PhysicalGrooveMetadata {
    pub fn new(
        generation: GrooveGenerationId,
        source_content_identity: GrooveContentIdentity,
        cut: PhysicalGrooveCutMetadata,
        total_frame_count: u64,
        tracing_halo_frames: u32,
    ) -> Result<Self, PagedGrooveError> {
        let trace_admission_policy = GrooveTraceAdmissionPolicy::standard(source_content_identity)?;
        let mut metadata = Self {
            generation,
            format: PhysicalGrooveFormat::physical_current(),
            source_content_identity,
            content_identity: GrooveContentIdentity::from_sha256([0; 32]),
            cut,
            total_frame_count,
            tracing_halo_frames,
            trace_admission_policy,
        };
        metadata.content_identity = metadata.calculate_content_identity();
        metadata.validate()?;
        Ok(metadata)
    }

    /// Creates metadata from one validated contiguous groove asset.
    pub fn from_groove_asset(
        generation: GrooveGenerationId,
        asset: &GrooveAsset,
        tracing_halo_frames: u32,
    ) -> Result<Self, PagedGrooveError> {
        Self::new(
            generation,
            asset.provenance().content_identity(),
            PhysicalGrooveCutMetadata::new(
                asset.layout(),
                asset.provenance().cut(),
                asset.report(),
            ),
            u64::try_from(asset.frame_count())
                .map_err(|_| PagedGrooveError::InvalidTotalFrameCount)?,
            tracing_halo_frames,
        )
    }

    pub fn generation(self) -> GrooveGenerationId {
        self.generation
    }

    pub fn format(self) -> PhysicalGrooveFormat {
        self.format
    }

    pub fn source_content_identity(self) -> GrooveContentIdentity {
        self.source_content_identity
    }

    /// This identity excludes the transient cache generation.
    pub fn content_identity(self) -> GrooveContentIdentity {
        self.content_identity
    }

    pub fn cut(self) -> PhysicalGrooveCutMetadata {
        self.cut
    }

    pub fn total_frame_count(self) -> u64 {
        self.total_frame_count
    }

    pub fn total_duration_seconds(self) -> f64 {
        self.total_frame_count as f64 / f64::from(PHYSICAL_GROOVE_SAMPLE_RATE_HZ)
    }

    pub fn tracing_halo_frames(self) -> u32 {
        self.tracing_halo_frames
    }

    pub fn trace_admission_policy(self) -> GrooveTraceAdmissionPolicy {
        self.trace_admission_policy
    }

    /// Returns the worst-case halo for one stylus over this record and pyramid.
    pub fn minimum_tracing_halo_frames(
        self,
        geometry: StylusGeometry,
    ) -> Result<u32, PagedGrooveError> {
        let final_frame = self.total_frame_count.saturating_sub(1) as f64;
        let meters_per_source_frame = self
            .cut
            .layout
            .meters_per_frame_at(final_frame, self.cut.cut.groove_pitch_m_per_revolution);
        Ok(geometry
            .multiresolution_support(meters_per_source_frame, MAX_SPATIAL_LEVEL_STEP_FRAMES)?
            .symmetric_halo_source_frames())
    }

    pub fn validate_tracing_geometry(
        self,
        geometry: StylusGeometry,
    ) -> Result<(), PagedGrooveError> {
        self.trace_admission_policy
            .validate(self.source_content_identity)?;
        if geometry.validate()?.tracing_radius_m
            > self
                .trace_admission_policy
                .maximum_geometry()
                .tracing_radius_m
        {
            return Err(PagedGrooveError::TraceAdmission(
                GrooveTraceAdmissionError::GeometryOutsideCertificate,
            ));
        }
        let required_frames = self.minimum_tracing_halo_frames(geometry)?;
        if required_frames > self.tracing_halo_frames {
            return Err(PagedGrooveError::InsufficientDeclaredTracingHalo {
                required_frames,
                declared_frames: self.tracing_halo_frames,
            });
        }
        Ok(())
    }

    /// Includes filter support and cubic interpolation support.
    pub fn required_storage_halo_frames(self) -> u32 {
        self.tracing_halo_frames + PAGED_GROOVE_SPATIAL_STORAGE_MARGIN_FRAMES
    }

    pub fn total_range(self) -> GrooveFrameRange {
        GrooveFrameRange {
            start_frame: 0,
            end_frame_exclusive: self.total_frame_count,
        }
    }

    pub fn radius_at_absolute_frame(self, frame: f64) -> Result<f64, PagedGrooveError> {
        if !frame.is_finite() || frame < 0.0 || frame >= self.total_frame_count as f64 {
            return Err(PagedGrooveError::FramePositionOutsideRecord);
        }
        Ok(self
            .cut
            .layout
            .radius_at_frame(frame, self.cut.cut.groove_pitch_m_per_revolution))
    }

    pub fn final_program_radius_m(self) -> f64 {
        self.cut.report.final_program_radius_m
    }

    pub(crate) fn validate(self) -> Result<(), PagedGrooveError> {
        self.generation.validate()?;
        self.format.validate()?;
        self.source_content_identity.validate_current()?;
        self.content_identity.validate_current()?;
        if self.total_frame_count < MIN_TOTAL_FRAME_COUNT {
            return Err(PagedGrooveError::InvalidTotalFrameCount);
        }
        if self.tracing_halo_frames == 0
            || self.tracing_halo_frames > MAX_PAGED_GROOVE_TRACING_HALO_FRAMES
        {
            return Err(PagedGrooveError::InvalidTracingHalo);
        }
        self.cut.validate(self.total_frame_count)?;
        self.trace_admission_policy
            .validate(self.source_content_identity)?;
        if self.content_identity != self.calculate_content_identity() {
            return Err(PagedGrooveError::MetadataContentIdentityMismatch);
        }
        Ok(())
    }

    fn calculate_content_identity(self) -> GrooveContentIdentity {
        let mut hash = GrooveContentHasher::new(b"record-player-paged-groove-metadata\0");
        hash.u32(self.format.format_version);
        hash.u32(self.format.sample_rate_hz);
        hash.u8(match self.format.coordinate_encoding {
            GrooveCoordinateEncoding::LateralVertical45_45 => 0,
        });
        hash.u8(match self.format.sample_encoding {
            GrooveSampleEncoding::Float32Meters => 0,
        });
        hash.identity(self.source_content_identity);
        hash.identity(self.trace_admission_policy.policy_identity());
        for value in [
            self.cut.layout.outer_program_radius_m,
            self.cut.layout.inner_program_radius_m,
            self.cut.layout.nominal_rpm,
            self.cut.layout.groove_sample_rate_hz,
            self.cut.cut.full_scale_sine_velocity_rms_m_s,
            self.cut.cut.cutter_highpass_hz,
            self.cut.cut.cutter_bandwidth_hz,
            self.cut.cut.groove_pitch_m_per_revolution,
            self.cut.cut.groove_top_width_m,
            self.cut.cut.minimum_land_width_m,
            self.cut.report.peak_left_velocity_m_s,
            self.cut.report.peak_right_velocity_m_s,
            self.cut.report.rms_left_velocity_m_s,
            self.cut.report.rms_right_velocity_m_s,
            self.cut.report.peak_lateral_displacement_m,
            self.cut.report.peak_vertical_displacement_m,
            self.cut.report.final_lateral_drift_m,
            self.cut.report.final_vertical_drift_m,
            self.cut.report.groove_pitch_m_per_revolution,
            self.cut.report.final_program_radius_m,
        ] {
            hash.f64(value);
        }
        hash.bool(self.cut.report.programme_exceeds_available_radius);
        hash.bool(self.cut.report.adjacent_turn_clearance_failed);
        match self.cut.report.minimum_adjacent_turn_clearance_m {
            Some(value) => {
                hash.u8(1);
                hash.f64(value);
            }
            None => hash.u8(0),
        }
        match self.cut.report.first_failing_clearance_frame_pair {
            Some(pair) => {
                hash.u8(1);
                hash.u64(pair.outer_frame);
                hash.u64(pair.inner_frame);
            }
            None => hash.u8(0),
        }
        hash.u64(self.total_frame_count);
        hash.u32(self.tracing_halo_frames);
        hash.finish()
    }
}

/// Stores one immutable core page and its tracing overlap.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalGroovePage {
    generation: GrooveGenerationId,
    asset_content_identity: GrooveContentIdentity,
    content_identity: GrooveContentIdentity,
    core_range: GrooveFrameRange,
    stored_range: GrooveFrameRange,
    lateral_displacement_m: Box<[f32]>,
    vertical_displacement_m: Box<[f32]>,
    spatial_pyramid: Option<GrooveSpatialPyramid>,
    trace_admission_certificate: Option<GrooveTraceAdmissionCertificate>,
    #[serde(skip)]
    spatial_pyramid_validated: bool,
    #[serde(skip)]
    validated_trace_admission: Option<ValidatedGrooveTraceAdmissionCertificate>,
}

impl<'de> Deserialize<'de> for PhysicalGroovePage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct WirePage {
            generation: GrooveGenerationId,
            asset_content_identity: GrooveContentIdentity,
            content_identity: GrooveContentIdentity,
            core_range: GrooveFrameRange,
            stored_range: GrooveFrameRange,
            lateral_displacement_m: Box<[f32]>,
            vertical_displacement_m: Box<[f32]>,
            spatial_pyramid: GrooveSpatialPyramid,
            trace_admission_certificate: GrooveTraceAdmissionCertificate,
        }

        let wire = WirePage::deserialize(deserializer)?;
        let mut page = Self {
            generation: wire.generation,
            asset_content_identity: wire.asset_content_identity,
            content_identity: wire.content_identity,
            core_range: wire.core_range,
            stored_range: wire.stored_range,
            lateral_displacement_m: wire.lateral_displacement_m,
            vertical_displacement_m: wire.vertical_displacement_m,
            spatial_pyramid: Some(wire.spatial_pyramid),
            trace_admission_certificate: Some(wire.trace_admission_certificate),
            spatial_pyramid_validated: false,
            validated_trace_admission: None,
        };
        page.core_range
            .validate()
            .map_err(serde::de::Error::custom)?;
        page.stored_range
            .validate()
            .map_err(serde::de::Error::custom)?;
        if !page.stored_range.contains_range(page.core_range) {
            return Err(serde::de::Error::custom(
                PagedGrooveError::StoredRangeDoesNotContainCore,
            ));
        }
        if page.lateral_displacement_m.len() != page.vertical_displacement_m.len() {
            return Err(serde::de::Error::custom(
                PagedGrooveError::ChannelLengthMismatch,
            ));
        }
        if page.stored_range.frame_count() != page.lateral_displacement_m.len() as u64 {
            return Err(serde::de::Error::custom(
                PagedGrooveError::StoredLengthMismatch,
            ));
        }
        if page
            .lateral_displacement_m
            .iter()
            .chain(page.vertical_displacement_m.iter())
            .any(|sample| !sample.is_finite())
        {
            return Err(serde::de::Error::custom(
                PagedGrooveError::NonfiniteDisplacement,
            ));
        }
        page.asset_content_identity
            .validate_current()
            .map_err(serde::de::Error::custom)?;
        page.content_identity
            .validate_current()
            .map_err(serde::de::Error::custom)?;
        wire.trace_admission_certificate
            .validate_static()
            .map_err(serde::de::Error::custom)?;
        validate_spatial_pyramid(&mut page).map_err(serde::de::Error::custom)?;
        if page.content_identity != page.calculate_content_identity() {
            return Err(serde::de::Error::custom(
                PagedGrooveError::PageContentIdentityMismatch { page_index: 0 },
            ));
        }
        Ok(page)
    }
}

impl PhysicalGroovePage {
    pub fn new(
        metadata: PhysicalGrooveMetadata,
        core_range: GrooveFrameRange,
        stored_range: GrooveFrameRange,
        lateral_displacement_m: Vec<f32>,
        vertical_displacement_m: Vec<f32>,
    ) -> Result<Self, PagedGrooveError> {
        metadata.validate()?;
        core_range.validate()?;
        stored_range.validate()?;
        if !stored_range.contains_range(core_range) {
            return Err(PagedGrooveError::StoredRangeDoesNotContainCore);
        }
        if lateral_displacement_m.len() != vertical_displacement_m.len() {
            return Err(PagedGrooveError::ChannelLengthMismatch);
        }
        if stored_range.frame_count() != lateral_displacement_m.len() as u64 {
            return Err(PagedGrooveError::StoredLengthMismatch);
        }
        if lateral_displacement_m
            .iter()
            .chain(&vertical_displacement_m)
            .any(|sample| !sample.is_finite())
        {
            return Err(PagedGrooveError::NonfiniteDisplacement);
        }
        let spatial_pyramid = GrooveSpatialPyramid::build_window(
            stored_range.start_frame,
            &lateral_displacement_m,
            &vertical_displacement_m,
        )?;
        let mut page = Self {
            generation: metadata.generation,
            asset_content_identity: metadata.content_identity,
            content_identity: GrooveContentIdentity::from_sha256([0; 32]),
            core_range,
            stored_range,
            lateral_displacement_m: lateral_displacement_m.into_boxed_slice(),
            vertical_displacement_m: vertical_displacement_m.into_boxed_slice(),
            spatial_pyramid: Some(spatial_pyramid),
            trace_admission_certificate: None,
            spatial_pyramid_validated: true,
            validated_trace_admission: None,
        };
        let certificate = page.calculate_trace_admission_certificate(metadata)?;
        page.validated_trace_admission = Some(certificate.validate_recomputed(certificate)?);
        page.trace_admission_certificate = Some(certificate);
        page.content_identity = page.calculate_content_identity();
        Ok(page)
    }

    pub fn generation(&self) -> GrooveGenerationId {
        self.generation
    }

    pub fn asset_content_identity(&self) -> GrooveContentIdentity {
        self.asset_content_identity
    }

    pub fn content_identity(&self) -> GrooveContentIdentity {
        self.content_identity
    }

    pub fn core_range(&self) -> GrooveFrameRange {
        self.core_range
    }

    pub fn stored_range(&self) -> GrooveFrameRange {
        self.stored_range
    }

    pub fn left_halo_range(&self) -> Option<GrooveFrameRange> {
        (self.stored_range.start_frame < self.core_range.start_frame).then_some(GrooveFrameRange {
            start_frame: self.stored_range.start_frame,
            end_frame_exclusive: self.core_range.start_frame,
        })
    }

    pub fn right_halo_range(&self) -> Option<GrooveFrameRange> {
        (self.core_range.end_frame_exclusive < self.stored_range.end_frame_exclusive).then_some(
            GrooveFrameRange {
                start_frame: self.core_range.end_frame_exclusive,
                end_frame_exclusive: self.stored_range.end_frame_exclusive,
            },
        )
    }

    pub fn lateral_displacement_m(&self) -> &[f32] {
        &self.lateral_displacement_m
    }

    pub fn vertical_displacement_m(&self) -> &[f32] {
        &self.vertical_displacement_m
    }

    pub fn spatial_pyramid(&self) -> Option<&GrooveSpatialPyramid> {
        self.spatial_pyramid.as_ref()
    }

    pub fn trace_admission_certificate(&self) -> Option<GrooveTraceAdmissionCertificate> {
        self.trace_admission_certificate
    }

    /// Returns the page data size that counts against a cache limit.
    pub fn resident_size_bytes(&self) -> u64 {
        self.checked_resident_size_bytes().unwrap_or(u64::MAX)
    }

    fn checked_resident_size_bytes(&self) -> Option<u64> {
        let mut bytes = u64::try_from(size_of::<Self>()).ok()?;
        let base_samples = self
            .lateral_displacement_m
            .len()
            .checked_add(self.vertical_displacement_m.len())?;
        bytes =
            bytes.checked_add(u64::try_from(base_samples.checked_mul(size_of::<f32>())?).ok()?)?;
        if let Some(pyramid) = &self.spatial_pyramid {
            bytes = bytes.checked_add(
                u64::try_from(
                    pyramid
                        .levels()
                        .len()
                        .checked_mul(size_of::<super::groove::GrooveSpatialLevel>())?,
                )
                .ok()?,
            )?;
            for level in pyramid.levels() {
                let level_samples = level
                    .lateral_displacement_m()
                    .len()
                    .checked_add(level.vertical_displacement_m().len())?;
                bytes = bytes.checked_add(
                    u64::try_from(level_samples.checked_mul(size_of::<f32>())?).ok()?,
                )?;
            }
        }
        Some(bytes)
    }

    fn sample_at_stored_frame(&self, frame: u64) -> Option<GrooveSample> {
        if !self.stored_range.contains(frame) {
            return None;
        }
        let index = usize::try_from(frame - self.stored_range.start_frame).ok()?;
        Some(GrooveSample {
            lateral_displacement_m: self.lateral_displacement_m[index],
            vertical_displacement_m: self.vertical_displacement_m[index],
        })
    }

    fn calculate_content_identity(&self) -> GrooveContentIdentity {
        let mut hash = GrooveContentHasher::new(b"record-player-paged-groove-page-v4\0");
        hash.u64(self.generation.get());
        hash.identity(self.asset_content_identity);
        match self.trace_admission_certificate {
            Some(certificate) => {
                hash.u8(1);
                hash.identity(certificate.certificate_identity());
            }
            None => hash.u8(0),
        }
        hash.u64(self.core_range.start_frame);
        hash.u64(self.core_range.end_frame_exclusive);
        hash.u64(self.stored_range.start_frame);
        hash.u64(self.stored_range.end_frame_exclusive);
        hash.u64(self.lateral_displacement_m.len() as u64);
        for sample in &self.lateral_displacement_m {
            hash.f32(*sample);
        }
        hash.u64(self.vertical_displacement_m.len() as u64);
        for sample in &self.vertical_displacement_m {
            hash.f32(*sample);
        }
        match &self.spatial_pyramid {
            Some(pyramid) => {
                hash.u8(1);
                hash.u32(pyramid.format_version());
                hash.u64(pyramid.levels().len() as u64);
                for level in pyramid.levels() {
                    hash.u64(level.first_source_frame());
                    hash.u32(level.source_frame_step());
                    hash.u64(level.lateral_displacement_m().len() as u64);
                    for sample in level.lateral_displacement_m() {
                        hash.f32(*sample);
                    }
                    hash.u64(level.vertical_displacement_m().len() as u64);
                    for sample in level.vertical_displacement_m() {
                        hash.f32(*sample);
                    }
                }
            }
            None => hash.u8(0),
        }
        hash.finish()
    }

    fn calculate_trace_admission_certificate(
        &self,
        metadata: PhysicalGrooveMetadata,
    ) -> Result<GrooveTraceAdmissionCertificate, PagedGrooveError> {
        let pyramid = self
            .spatial_pyramid
            .as_ref()
            .ok_or(PagedGrooveError::MissingSpatialPyramid)?;
        if pyramid.levels().len() != GROOVE_SPATIAL_PYRAMID_LEVELS {
            return Err(PagedGrooveError::TraceAdmission(
                GrooveTraceAdmissionError::InvalidRepresentation,
            ));
        }
        let levels = pyramid.levels();
        let spatial_levels = std::array::from_fn(|index| GrooveTraceAdmissionLevel {
            first_source_frame: levels[index].first_source_frame(),
            source_frame_step: levels[index].source_frame_step(),
            lateral_displacement_m: levels[index].lateral_displacement_m(),
            vertical_displacement_m: levels[index].vertical_displacement_m(),
        });
        let final_frame = metadata.total_frame_count.saturating_sub(1) as f64;
        Ok(certify_groove_trace_representation(
            GrooveTraceAdmissionBinding {
                representation_kind: GrooveTraceRepresentationKind::Paged,
                representation_format_version: PAGED_GROOVE_FORMAT_VERSION,
                source_content_identity: metadata.source_content_identity,
                generation: self.generation.get(),
                core_start_frame: self.core_range.start_frame,
                core_end_frame_exclusive: self.core_range.end_frame_exclusive,
                stored_start_frame: self.stored_range.start_frame,
                stored_end_frame_exclusive: self.stored_range.end_frame_exclusive,
                record_end_frame_exclusive: metadata.total_frame_count,
                minimum_meters_per_source_frame: metadata.cut.layout.meters_per_frame_at(
                    final_frame,
                    metadata.cut.cut.groove_pitch_m_per_revolution,
                ),
                maximum_geometry: metadata.trace_admission_policy.maximum_geometry(),
                edge_coverage: GrooveTraceEdgeCoverage::paged(
                    self.stored_range.start_frame == 0,
                    self.stored_range.end_frame_exclusive == metadata.total_frame_count,
                ),
            },
            GrooveTraceAdmissionLevel {
                first_source_frame: self.stored_range.start_frame,
                source_frame_step: 1,
                lateral_displacement_m: &self.lateral_displacement_m,
                vertical_displacement_m: &self.vertical_displacement_m,
            },
            &spatial_levels,
        )?)
    }

    pub(crate) fn validate_trace_admission(
        &mut self,
        metadata: PhysicalGrooveMetadata,
    ) -> Result<(), PagedGrooveError> {
        let certificate = self
            .trace_admission_certificate
            .ok_or(PagedGrooveError::MissingTraceAdmissionCertificate)?;
        self.validated_trace_admission = Some(
            certificate
                .validate_recomputed(self.calculate_trace_admission_certificate(metadata)?)?,
        );
        Ok(())
    }

    pub(crate) fn validated_trace_admission(
        &self,
    ) -> Result<ValidatedGrooveTraceAdmissionCertificate, PagedGrooveError> {
        self.validated_trace_admission
            .ok_or(PagedGrooveError::MissingTraceAdmissionCertificate)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrooveSample {
    pub lateral_displacement_m: f32,
    pub vertical_displacement_m: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrooveTravelDirection {
    Forward,
    Reverse,
}

/// Borrows one page and keeps its absolute coordinate mapping.
#[derive(Debug, Clone, Copy)]
pub struct GrooveTraceView<'a> {
    page: &'a PhysicalGroovePage,
    absolute_frame_position: f64,
    local_frame_position: f64,
    direction: GrooveTravelDirection,
}

/// Borrows the two spatial levels selected for one render step.
#[derive(Debug, Clone, Copy)]
pub struct AntiAliasedGrooveTraceView<'a> {
    page: &'a PhysicalGroovePage,
    selection: GrooveSpatialLevelSelection<'a>,
    absolute_frame_position: f64,
    direction: GrooveTravelDirection,
    available_tracing_halo_frames: u32,
}

impl<'a> AntiAliasedGrooveTraceView<'a> {
    pub fn absolute_frame_position(self) -> f64 {
        self.absolute_frame_position
    }

    pub fn direction(self) -> GrooveTravelDirection {
        self.direction
    }

    pub fn core_range(self) -> GrooveFrameRange {
        self.page.core_range
    }

    pub fn stored_range(self) -> GrooveFrameRange {
        self.page.stored_range
    }

    pub fn level_selection(self) -> GrooveSpatialLevelSelection<'a> {
        self.selection
    }

    pub fn trace_wall_contacts(
        self,
        wall_index: usize,
        meters_per_source_frame: f64,
        geometry: StylusGeometry,
    ) -> Result<StylusTraceContactSet, PagedGrooveError> {
        let lower = self.selection.lower();
        let upper = self.selection.upper();
        let maximum_level_step = lower.source_frame_step().max(upper.source_frame_step());
        let support =
            geometry.multiresolution_support(meters_per_source_frame, maximum_level_step)?;
        let required_frames = support.symmetric_halo_source_frames();
        if required_frames > self.available_tracing_halo_frames {
            return Err(PagedGrooveError::Stylus(
                super::StylusTraceError::InsufficientPageHalo {
                    required_frames,
                    available_frames: self.available_tracing_halo_frames,
                },
            ));
        }
        let admission = self.page.validated_trace_admission()?;
        admission.validate_for_active_tracing(geometry)?;
        match admission.certificate().admission_class() {
            GrooveTraceAdmissionClass::StrictConcavity => Ok(
                trace_spherical_45_45_wall_multiresolution_contacts_certified_concave(
                    lower.lateral_displacement_m(),
                    lower.vertical_displacement_m(),
                    lower.first_source_frame(),
                    lower.source_frame_step(),
                    upper.lateral_displacement_m(),
                    upper.vertical_displacement_m(),
                    upper.first_source_frame(),
                    upper.source_frame_step(),
                    self.selection.upper_level_blend(),
                    wall_index,
                    self.absolute_frame_position,
                    meters_per_source_frame,
                    geometry,
                    admission.certified_concave_trace_bounds(geometry)?,
                )?,
            ),
            GrooveTraceAdmissionClass::FixedCapPiecewise => Ok(
                trace_spherical_45_45_wall_multiresolution_contacts_certified_piecewise(
                    lower.lateral_displacement_m(),
                    lower.vertical_displacement_m(),
                    lower.first_source_frame(),
                    lower.source_frame_step(),
                    upper.lateral_displacement_m(),
                    upper.vertical_displacement_m(),
                    upper.first_source_frame(),
                    upper.source_frame_step(),
                    self.selection.upper_level_blend(),
                    wall_index,
                    self.absolute_frame_position,
                    meters_per_source_frame,
                    geometry,
                    admission.fixed_cap_piecewise_trace_bounds(geometry)?,
                )?,
            ),
            rejected => Err(rejected
                .rejection_error()
                .expect("non-trace admission classes have a typed rejection")
                .into()),
        }
    }
}

impl<'a> GrooveTraceView<'a> {
    pub fn absolute_frame_position(self) -> f64 {
        self.absolute_frame_position
    }

    pub fn local_frame_position(self) -> f64 {
        self.local_frame_position
    }

    pub fn direction(self) -> GrooveTravelDirection {
        self.direction
    }

    pub fn core_range(self) -> GrooveFrameRange {
        self.page.core_range
    }

    pub fn stored_range(self) -> GrooveFrameRange {
        self.page.stored_range
    }

    pub fn left_halo_range(self) -> Option<GrooveFrameRange> {
        self.page.left_halo_range()
    }

    pub fn right_halo_range(self) -> Option<GrooveFrameRange> {
        self.page.right_halo_range()
    }

    pub fn lateral_displacement_m(self) -> &'a [f32] {
        &self.page.lateral_displacement_m
    }

    pub fn vertical_displacement_m(self) -> &'a [f32] {
        &self.page.vertical_displacement_m
    }

    pub fn sample_at_absolute_frame(self, frame: u64) -> Option<GrooveSample> {
        self.page.sample_at_stored_frame(frame)
    }

    pub fn sample_in_travel_direction(self, distance_frames: u64) -> Option<GrooveSample> {
        let anchor = self.absolute_frame_position.floor() as u64;
        let frame = match self.direction {
            GrooveTravelDirection::Forward => anchor.checked_add(distance_frames)?,
            GrooveTravelDirection::Reverse => anchor.checked_sub(distance_frames)?,
        };
        self.sample_at_absolute_frame(frame)
    }
}

/// Owns validated immutable pages for a contiguous available record range.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedGrooveAsset {
    metadata: PhysicalGrooveMetadata,
    available_range: GrooveFrameRange,
    pages: Box<[PhysicalGroovePage]>,
}

impl<'de> Deserialize<'de> for PagedGrooveAsset {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct WireAsset {
            metadata: PhysicalGrooveMetadata,
            available_range: GrooveFrameRange,
            pages: Vec<PhysicalGroovePage>,
        }

        let wire = WireAsset::deserialize(deserializer)?;
        Self::new(wire.metadata, wire.available_range, wire.pages).map_err(serde::de::Error::custom)
    }
}

impl PagedGrooveAsset {
    /// Validates and freezes pages outside the render path.
    pub fn new(
        metadata: PhysicalGrooveMetadata,
        available_range: GrooveFrameRange,
        mut pages: Vec<PhysicalGroovePage>,
    ) -> Result<Self, PagedGrooveError> {
        metadata.validate()?;
        available_range.validate()?;
        if !metadata.total_range().contains_range(available_range) {
            return Err(PagedGrooveError::AvailableRangeOutsideRecord);
        }
        if pages.is_empty() {
            return Err(PagedGrooveError::NoPages);
        }

        let mut expected_core_start = available_range.start_frame;
        for (page_index, page) in pages.iter_mut().enumerate() {
            validate_page(metadata, available_range, page, page_index)?;
            if page.core_range.start_frame > expected_core_start {
                return Err(PagedGrooveError::CoreGap {
                    expected_frame: expected_core_start,
                    actual_frame: page.core_range.start_frame,
                });
            }
            if page.core_range.start_frame < expected_core_start {
                return Err(PagedGrooveError::CoreOverlap {
                    expected_frame: expected_core_start,
                    actual_frame: page.core_range.start_frame,
                });
            }
            expected_core_start = page.core_range.end_frame_exclusive;
        }
        if expected_core_start != available_range.end_frame_exclusive {
            return Err(PagedGrooveError::CoreGap {
                expected_frame: expected_core_start,
                actual_frame: available_range.end_frame_exclusive,
            });
        }

        for page in &mut pages {
            validate_spatial_pyramid(page)?;
        }
        for page_index in 1..pages.len() {
            validate_overlap(
                &pages[page_index - 1],
                &pages[page_index],
                page_index - 1,
                page_index,
            )?;
        }

        Ok(Self {
            metadata,
            available_range,
            pages: pages.into_boxed_slice(),
        })
    }

    pub fn metadata(&self) -> PhysicalGrooveMetadata {
        self.metadata
    }

    pub fn content_identity(&self) -> GrooveContentIdentity {
        self.metadata.content_identity
    }

    pub fn available_range(&self) -> GrooveFrameRange {
        self.available_range
    }

    pub fn pages(&self) -> &[PhysicalGroovePage] {
        &self.pages
    }

    pub fn is_frame_available(&self, frame: u64) -> bool {
        self.available_range.contains(frame)
    }

    /// Looks up one core sample without allocation or synchronization.
    pub fn sample_at(&self, frame: u64) -> Result<GrooveSample, PagedGrooveError> {
        let page = self.page_for_frame(frame)?;
        page.sample_at_stored_frame(frame)
            .ok_or(PagedGrooveError::InternalPageMap)
    }

    /// Returns one page and its tracing halos without allocation or synchronization.
    pub fn trace_view(
        &self,
        absolute_frame_position: f64,
        direction: GrooveTravelDirection,
    ) -> Result<GrooveTraceView<'_>, PagedGrooveError> {
        if !absolute_frame_position.is_finite() || absolute_frame_position < 0.0 {
            return Err(PagedGrooveError::InvalidFramePosition);
        }
        let anchor_frame = absolute_frame_position.floor();
        if anchor_frame > u64::MAX as f64 {
            return Err(PagedGrooveError::FrameUnavailable {
                frame: u64::MAX,
                available_range: self.available_range,
            });
        }
        let page = self.page_for_frame(anchor_frame as u64)?;
        Ok(GrooveTraceView {
            page,
            absolute_frame_position,
            local_frame_position: absolute_frame_position - page.stored_range.start_frame as f64,
            direction,
        })
    }

    /// Selects immutable spatial levels without render-time allocation.
    pub fn antialiased_trace_view(
        &self,
        absolute_frame_position: f64,
        direction: GrooveTravelDirection,
        source_frame_advance: f64,
    ) -> Result<AntiAliasedGrooveTraceView<'_>, PagedGrooveError> {
        if !absolute_frame_position.is_finite() || absolute_frame_position < 0.0 {
            return Err(PagedGrooveError::InvalidFramePosition);
        }
        let anchor_frame = absolute_frame_position.floor();
        if anchor_frame > u64::MAX as f64 {
            return Err(PagedGrooveError::FrameUnavailable {
                frame: u64::MAX,
                available_range: self.available_range,
            });
        }
        let page = self.page_for_frame(anchor_frame as u64)?;
        let selection = page
            .spatial_pyramid()
            .ok_or(PagedGrooveError::InternalPageMap)?
            .select(
                page.stored_range.start_frame,
                &page.lateral_displacement_m,
                &page.vertical_displacement_m,
                source_frame_advance,
            )?;
        Ok(AntiAliasedGrooveTraceView {
            page,
            selection,
            absolute_frame_position,
            direction,
            available_tracing_halo_frames: self.metadata.tracing_halo_frames,
        })
    }

    fn page_for_frame(&self, frame: u64) -> Result<&PhysicalGroovePage, PagedGrooveError> {
        if !self.available_range.contains(frame) {
            return Err(PagedGrooveError::FrameUnavailable {
                frame,
                available_range: self.available_range,
            });
        }
        let mut lower = 0;
        let mut upper = self.pages.len();
        while lower < upper {
            let middle = lower + (upper - lower) / 2;
            let page = &self.pages[middle];
            if frame < page.core_range.start_frame {
                upper = middle;
            } else if frame >= page.core_range.end_frame_exclusive {
                lower = middle + 1;
            } else {
                return Ok(page);
            }
        }
        Err(PagedGrooveError::InternalPageMap)
    }
}

/// Sets hard ownership limits for one immutable page cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedGrooveCacheLimits {
    maximum_pages: u32,
    maximum_resident_bytes: u64,
}

impl PagedGrooveCacheLimits {
    pub fn new(maximum_pages: u32, maximum_resident_bytes: u64) -> Result<Self, PagedGrooveError> {
        let limits = Self {
            maximum_pages,
            maximum_resident_bytes,
        };
        limits.validate()?;
        Ok(limits)
    }

    pub fn maximum_pages(self) -> u32 {
        self.maximum_pages
    }

    pub fn maximum_resident_bytes(self) -> u64 {
        self.maximum_resident_bytes
    }

    fn validate(self) -> Result<(), PagedGrooveError> {
        if self.maximum_pages == 0 || self.maximum_pages > MAX_CACHE_PAGE_COUNT {
            return Err(PagedGrooveError::InvalidCacheLimits {
                field: "maximumPages",
            });
        }
        if self.maximum_resident_bytes == 0
            || self.maximum_resident_bytes > MAX_CACHE_RESIDENT_BYTES
        {
            return Err(PagedGrooveError::InvalidCacheLimits {
                field: "maximumResidentBytes",
            });
        }
        Ok(())
    }
}

impl Default for PagedGrooveCacheLimits {
    fn default() -> Self {
        Self {
            maximum_pages: 16,
            maximum_resident_bytes: 64 * 1_024 * 1_024,
        }
    }
}

/// Builds a bounded cache outside the render thread.
#[derive(Debug)]
pub struct PagedGrooveCacheProducer {
    metadata: PhysicalGrooveMetadata,
    limits: PagedGrooveCacheLimits,
    pages: Vec<Arc<PhysicalGroovePage>>,
    resident_page_bytes: u64,
}

impl PagedGrooveCacheProducer {
    pub fn new(
        metadata: PhysicalGrooveMetadata,
        limits: PagedGrooveCacheLimits,
    ) -> Result<Self, PagedGrooveError> {
        metadata.validate()?;
        limits.validate()?;
        Ok(Self {
            metadata,
            limits,
            pages: Vec::with_capacity(limits.maximum_pages as usize),
            resident_page_bytes: 0,
        })
    }

    /// Starts an off-thread update from an immutable published cache.
    pub fn from_cache(cache: &PagedGrooveCache) -> Self {
        let mut pages = Vec::with_capacity(cache.limits.maximum_pages as usize);
        pages.extend(cache.pages.iter().cloned());
        Self {
            metadata: cache.metadata,
            limits: cache.limits,
            pages,
            resident_page_bytes: cache.resident_page_bytes,
        }
    }

    pub fn metadata(&self) -> PhysicalGrooveMetadata {
        self.metadata
    }

    pub fn limits(&self) -> PagedGrooveCacheLimits {
        self.limits
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn resident_page_bytes(&self) -> u64 {
        self.resident_page_bytes
    }

    /// Validates and inserts one page without changing published caches.
    pub fn insert_page(&mut self, mut page: PhysicalGroovePage) -> Result<(), PagedGrooveError> {
        if self.pages.len() >= self.limits.maximum_pages as usize {
            return Err(PagedGrooveError::CachePageLimitExceeded {
                maximum_pages: self.limits.maximum_pages,
            });
        }
        page.core_range.validate()?;
        let insertion_index = self.pages.partition_point(|existing| {
            existing.core_range.start_frame < page.core_range.start_frame
        });
        validate_page(
            self.metadata,
            self.metadata.total_range(),
            &mut page,
            insertion_index,
        )?;
        validate_spatial_pyramid(&mut page)?;
        let page_bytes = page
            .checked_resident_size_bytes()
            .ok_or(PagedGrooveError::CacheSizeOverflow)?;
        let next_resident_bytes = self
            .resident_page_bytes
            .checked_add(page_bytes)
            .ok_or(PagedGrooveError::CacheSizeOverflow)?;
        if next_resident_bytes > self.limits.maximum_resident_bytes {
            return Err(PagedGrooveError::CacheResidentLimitExceeded {
                maximum_resident_bytes: self.limits.maximum_resident_bytes,
                requested_resident_bytes: next_resident_bytes,
            });
        }

        if let Some(left) = insertion_index
            .checked_sub(1)
            .and_then(|index| self.pages.get(index))
        {
            if left.core_range.end_frame_exclusive > page.core_range.start_frame {
                return Err(PagedGrooveError::CacheCoreOverlap {
                    existing_core: left.core_range,
                    inserted_core: page.core_range,
                });
            }
        }
        if let Some(right) = self.pages.get(insertion_index) {
            if page.core_range.end_frame_exclusive > right.core_range.start_frame {
                return Err(PagedGrooveError::CacheCoreOverlap {
                    existing_core: right.core_range,
                    inserted_core: page.core_range,
                });
            }
        }
        for (existing_index, existing) in self.pages.iter().enumerate() {
            if ranges_overlap(existing.stored_range, page.stored_range) {
                validate_overlap(existing, &page, existing_index, insertion_index)?;
            }
        }

        self.pages.insert(insertion_index, Arc::new(page));
        self.resident_page_bytes = next_resident_bytes;
        Ok(())
    }

    /// Removes one core page. This method runs outside the render thread.
    pub fn remove_page_containing(&mut self, frame: u64) -> Option<Arc<PhysicalGroovePage>> {
        let index = page_index_for_frame(&self.pages, frame)?;
        let page = self.pages.remove(index);
        self.resident_page_bytes = self
            .resident_page_bytes
            .saturating_sub(page.resident_size_bytes());
        Some(page)
    }

    /// Publishes an immutable cache without materializing the complete record.
    pub fn publish(self) -> PagedGrooveCache {
        let mut manifest =
            PagedTraceRepresentationManifestHasher::new(self.metadata, self.pages.len() as u64);
        let mut maximum_certified_absolute_wall_slope = 0.0_f64;
        for page in &self.pages {
            let certificate = page
                .trace_admission_certificate()
                .expect("a cached page has a trace-admission certificate");
            maximum_certified_absolute_wall_slope = maximum_certified_absolute_wall_slope
                .max(certificate.maximum_absolute_wall_slope());
            manifest.append(
                page.core_range,
                page.stored_range,
                page.content_identity,
                certificate.certificate_identity(),
            );
        }
        let trace_admitted_representation_identity = manifest.finish();
        PagedGrooveCache {
            metadata: self.metadata,
            limits: self.limits,
            pages: self.pages.into_boxed_slice(),
            resident_page_bytes: self.resident_page_bytes,
            trace_admitted_representation_identity,
            maximum_certified_absolute_wall_slope,
        }
    }
}

pub(crate) struct PagedTraceRepresentationManifestHasher {
    hash: GrooveContentHasher,
}

impl PagedTraceRepresentationManifestHasher {
    pub(crate) fn new(metadata: PhysicalGrooveMetadata, page_count: u64) -> Self {
        let mut hash =
            GrooveContentHasher::new(b"record-player-paged-trace-representation-manifest-v1\0");
        hash.u32(PAGED_GROOVE_FORMAT_VERSION);
        hash.u32(GROOVE_SPATIAL_PYRAMID_FORMAT_VERSION);
        hash.identity(metadata.content_identity());
        hash.u64(metadata.generation().get());
        hash.u64(page_count);
        Self { hash }
    }

    pub(crate) fn append(
        &mut self,
        core_range: GrooveFrameRange,
        stored_range: GrooveFrameRange,
        page_content_identity: GrooveContentIdentity,
        certificate_identity: GrooveContentIdentity,
    ) {
        self.hash.u64(core_range.start_frame());
        self.hash.u64(core_range.end_frame_exclusive());
        self.hash.u64(stored_range.start_frame());
        self.hash.u64(stored_range.end_frame_exclusive());
        self.hash.identity(page_content_identity);
        self.hash.identity(certificate_identity);
    }

    pub(crate) fn finish(self) -> GrooveContentIdentity {
        self.hash.finish()
    }
}

/// Owns a sparse immutable set of validated groove pages.
#[derive(Debug)]
pub struct PagedGrooveCache {
    metadata: PhysicalGrooveMetadata,
    limits: PagedGrooveCacheLimits,
    pages: Box<[Arc<PhysicalGroovePage>]>,
    resident_page_bytes: u64,
    trace_admitted_representation_identity: GrooveContentIdentity,
    maximum_certified_absolute_wall_slope: f64,
}

impl PagedGrooveCache {
    pub fn metadata(&self) -> PhysicalGrooveMetadata {
        self.metadata
    }

    pub fn generation(&self) -> GrooveGenerationId {
        self.metadata.generation
    }

    pub fn content_identity(&self) -> GrooveContentIdentity {
        self.metadata.content_identity
    }

    pub fn trace_admitted_representation_identity(&self) -> GrooveContentIdentity {
        self.trace_admitted_representation_identity
    }

    pub(crate) fn maximum_certified_absolute_wall_slope(&self) -> f64 {
        self.maximum_certified_absolute_wall_slope
    }

    pub fn limits(&self) -> PagedGrooveCacheLimits {
        self.limits
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn resident_page_bytes(&self) -> u64 {
        self.resident_page_bytes
    }

    pub fn pages(&self) -> impl ExactSizeIterator<Item = &PhysicalGroovePage> {
        self.pages.iter().map(Arc::as_ref)
    }

    pub(crate) fn is_immutable_extension_of(&self, previous: &Self) -> bool {
        let mut next_page_index = 0;
        for previous_page in &previous.pages {
            while next_page_index < self.pages.len()
                && self.pages[next_page_index].core_range.start_frame
                    < previous_page.core_range.start_frame
            {
                next_page_index += 1;
            }
            let Some(next_page) = self.pages.get(next_page_index) else {
                return false;
            };
            if next_page.core_range != previous_page.core_range
                || next_page.stored_range != previous_page.stored_range
                || next_page.content_identity != previous_page.content_identity
                || next_page
                    .trace_admission_certificate()
                    .map(|certificate| certificate.certificate_identity())
                    != previous_page
                        .trace_admission_certificate()
                        .map(|certificate| certificate.certificate_identity())
            {
                return false;
            }
            next_page_index += 1;
        }
        true
    }

    /// Resolves one render position without allocation or synchronization.
    pub fn resolve(
        &self,
        request: PagedGrooveRenderRequest,
    ) -> Result<PagedGrooveRenderResolution<'_>, PagedGrooveError> {
        request.validate()?;
        if request.generation != self.metadata.generation {
            return Ok(PagedGrooveRenderResolution::Miss(
                PagedGrooveRenderMiss::StaleGeneration {
                    requested_generation: request.generation,
                    cached_generation: self.metadata.generation,
                },
            ));
        }
        let frame = request.anchor_frame(self.metadata.total_frame_count)?;
        let Some(page_index) = page_index_for_frame(&self.pages, frame) else {
            return Ok(PagedGrooveRenderResolution::Miss(
                PagedGrooveRenderMiss::PageUnavailable {
                    generation: request.generation,
                    frame,
                },
            ));
        };
        let page = self.pages[page_index].as_ref();
        let selection = page
            .spatial_pyramid()
            .ok_or(PagedGrooveError::InternalPageMap)?
            .select(
                page.stored_range.start_frame,
                &page.lateral_displacement_m,
                &page.vertical_displacement_m,
                request.source_frame_advance,
            )?;
        Ok(PagedGrooveRenderResolution::Ready(
            AntiAliasedGrooveTraceView {
                page,
                selection,
                absolute_frame_position: request.absolute_frame_position,
                direction: request.direction(),
                available_tracing_halo_frames: self.metadata.tracing_halo_frames,
            },
        ))
    }

    /// Traces both groove walls without allocation or synchronization.
    pub fn trace(
        &self,
        request: PagedGrooveRenderRequest,
        geometry: StylusGeometry,
    ) -> Result<PagedGrooveTraceResolution, PagedGrooveError> {
        let view = match self.resolve(request)? {
            PagedGrooveRenderResolution::Ready(view) => view,
            PagedGrooveRenderResolution::Miss(miss) => {
                return Ok(PagedGrooveTraceResolution::Miss(miss));
            }
        };
        let cut = self.metadata.cut;
        let groove_radius_m = cut.layout.radius_at_frame(
            request.absolute_frame_position,
            cut.cut.groove_pitch_m_per_revolution,
        );
        let meters_per_source_frame = cut.layout.meters_per_frame_at(
            request.absolute_frame_position,
            cut.cut.groove_pitch_m_per_revolution,
        );
        let selection = view.level_selection();
        let wall_contacts = [
            view.trace_wall_contacts(0, meters_per_source_frame, geometry)?,
            view.trace_wall_contacts(1, meters_per_source_frame, geometry)?,
        ];
        Ok(PagedGrooveTraceResolution::Ready(PagedGrooveTraceFrame {
            generation: request.generation,
            absolute_frame_position: request.absolute_frame_position,
            source_frame_advance: request.source_frame_advance,
            direction: request.direction(),
            page_core_range: view.core_range(),
            groove_radius_m,
            meters_per_source_frame,
            spatial_filter_lower_step_frames: selection.lower().source_frame_step(),
            spatial_filter_upper_step_frames: selection.upper().source_frame_step(),
            spatial_filter_upper_blend: selection.upper_level_blend(),
            wall_contacts,
        }))
    }

    /// Returns a fixed bidirectional request for an off-thread page producer.
    pub fn bidirectional_prefetch_request(
        &self,
        absolute_frame_position: f64,
        render_frame_horizon: u32,
    ) -> Result<PagedGroovePrefetchRequest, PagedGrooveError> {
        let request =
            PagedGrooveRenderRequest::new(self.metadata.generation, absolute_frame_position, 0.0)?;
        let anchor = request.anchor_frame(self.metadata.total_frame_count)?;
        let span = u64::from(render_frame_horizon)
            .checked_mul(MAX_PAGED_GROOVE_RENDER_SPEED as u64)
            .ok_or(PagedGrooveError::CacheSizeOverflow)?;
        let start_frame = anchor.saturating_sub(span);
        let end_frame_exclusive = anchor
            .saturating_add(span)
            .saturating_add(2)
            .min(self.metadata.total_frame_count);
        Ok(PagedGroovePrefetchRequest {
            generation: self.metadata.generation,
            core_range: GrooveFrameRange::new(start_frame, end_frame_exclusive)?,
        })
    }

    /// Plans bounded windows for the current position and explicit recapture candidates.
    pub fn bidirectional_prefetch_plan(
        &self,
        absolute_frame_position: f64,
        render_frame_horizon: u32,
        recapture_candidate_frame_positions: &[f64],
    ) -> Result<PagedGroovePrefetchPlan, PagedGrooveError> {
        if recapture_candidate_frame_positions.len() > MAX_PAGED_GROOVE_PREFETCH_CANDIDATES {
            return Err(PagedGrooveError::TooManyPrefetchCandidates {
                maximum_candidates: MAX_PAGED_GROOVE_PREFETCH_CANDIDATES,
            });
        }
        let mut ranges = Vec::with_capacity(recapture_candidate_frame_positions.len() + 1);
        ranges.push(
            self.bidirectional_prefetch_request(absolute_frame_position, render_frame_horizon)?
                .core_range,
        );
        for &candidate in recapture_candidate_frame_positions {
            ranges.push(
                self.bidirectional_prefetch_request(candidate, render_frame_horizon)?
                    .core_range,
            );
        }
        ranges.sort_unstable_by_key(|range| (range.start_frame, range.end_frame_exclusive));
        let mut merged: Vec<GrooveFrameRange> = Vec::with_capacity(ranges.len());
        for range in ranges {
            if let Some(previous) = merged.last_mut() {
                if range.start_frame <= previous.end_frame_exclusive {
                    previous.end_frame_exclusive =
                        previous.end_frame_exclusive.max(range.end_frame_exclusive);
                    continue;
                }
            }
            merged.push(range);
        }
        Ok(PagedGroovePrefetchPlan {
            generation: self.metadata.generation,
            core_ranges: merged.into_boxed_slice(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedGrooveRenderRequest {
    generation: GrooveGenerationId,
    absolute_frame_position: f64,
    source_frame_advance: f64,
}

impl PagedGrooveRenderRequest {
    pub fn new(
        generation: GrooveGenerationId,
        absolute_frame_position: f64,
        source_frame_advance: f64,
    ) -> Result<Self, PagedGrooveError> {
        let request = Self {
            generation,
            absolute_frame_position,
            source_frame_advance,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn generation(self) -> GrooveGenerationId {
        self.generation
    }

    pub fn absolute_frame_position(self) -> f64 {
        self.absolute_frame_position
    }

    pub fn source_frame_advance(self) -> f64 {
        self.source_frame_advance
    }

    pub fn direction(self) -> GrooveTravelDirection {
        if self.source_frame_advance < 0.0 {
            GrooveTravelDirection::Reverse
        } else {
            GrooveTravelDirection::Forward
        }
    }

    fn validate(self) -> Result<(), PagedGrooveError> {
        self.generation.validate()?;
        if !self.absolute_frame_position.is_finite() || self.absolute_frame_position < 0.0 {
            return Err(PagedGrooveError::InvalidFramePosition);
        }
        if !self.source_frame_advance.is_finite()
            || self.source_frame_advance.abs() > MAX_PAGED_GROOVE_RENDER_SPEED
        {
            return Err(PagedGrooveError::UnsupportedRenderSpeed);
        }
        Ok(())
    }

    fn anchor_frame(self, total_frame_count: u64) -> Result<u64, PagedGrooveError> {
        if self.absolute_frame_position >= total_frame_count as f64 {
            return Err(PagedGrooveError::FramePositionOutsideRecord);
        }
        let frame = self.absolute_frame_position.floor() as u64;
        if frame >= total_frame_count {
            return Err(PagedGrooveError::FramePositionOutsideRecord);
        }
        Ok(frame)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PagedGrooveRenderResolution<'a> {
    Ready(AntiAliasedGrooveTraceView<'a>),
    Miss(PagedGrooveRenderMiss),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PagedGrooveRenderMiss {
    #[error(
        "render request uses groove generation {requested_generation:?}, but the cache contains {cached_generation:?}"
    )]
    StaleGeneration {
        requested_generation: GrooveGenerationId,
        cached_generation: GrooveGenerationId,
    },
    #[error("groove page for frame {frame} in generation {generation:?} is unavailable")]
    PageUnavailable {
        generation: GrooveGenerationId,
        frame: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PagedGrooveTraceResolution {
    Ready(PagedGrooveTraceFrame),
    Miss(PagedGrooveRenderMiss),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PagedGrooveTraceFrame {
    pub generation: GrooveGenerationId,
    pub absolute_frame_position: f64,
    pub source_frame_advance: f64,
    pub direction: GrooveTravelDirection,
    pub page_core_range: GrooveFrameRange,
    pub groove_radius_m: f64,
    pub meters_per_source_frame: f64,
    pub spatial_filter_lower_step_frames: u32,
    pub spatial_filter_upper_step_frames: u32,
    pub spatial_filter_upper_blend: f64,
    pub wall_contacts: [StylusTraceContactSet; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagedGroovePrefetchRequest {
    generation: GrooveGenerationId,
    core_range: GrooveFrameRange,
}

impl PagedGroovePrefetchRequest {
    pub fn generation(self) -> GrooveGenerationId {
        self.generation
    }

    pub fn core_range(self) -> GrooveFrameRange {
        self.core_range
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagedGroovePrefetchPlan {
    generation: GrooveGenerationId,
    core_ranges: Box<[GrooveFrameRange]>,
}

impl PagedGroovePrefetchPlan {
    pub(crate) fn new(
        generation: GrooveGenerationId,
        core_ranges: Box<[GrooveFrameRange]>,
    ) -> Self {
        Self {
            generation,
            core_ranges,
        }
    }

    pub fn generation(&self) -> GrooveGenerationId {
        self.generation
    }

    pub fn core_ranges(&self) -> &[GrooveFrameRange] {
        &self.core_ranges
    }
}

fn page_index_for_frame(pages: &[Arc<PhysicalGroovePage>], frame: u64) -> Option<usize> {
    let mut lower = 0;
    let mut upper = pages.len();
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        let page = &pages[middle];
        if frame < page.core_range.start_frame {
            upper = middle;
        } else if frame >= page.core_range.end_frame_exclusive {
            lower = middle + 1;
        } else {
            return Some(middle);
        }
    }
    None
}

fn ranges_overlap(left: GrooveFrameRange, right: GrooveFrameRange) -> bool {
    left.start_frame < right.end_frame_exclusive && right.start_frame < left.end_frame_exclusive
}

fn validate_spatial_pyramid(page: &mut PhysicalGroovePage) -> Result<(), PagedGrooveError> {
    if page.spatial_pyramid_validated {
        return Ok(());
    }
    let spatial_pyramid = page
        .spatial_pyramid
        .as_ref()
        .ok_or(PagedGrooveError::MissingSpatialPyramid)?;
    spatial_pyramid.validate_against_window(
        page.stored_range.start_frame,
        &page.lateral_displacement_m,
        &page.vertical_displacement_m,
    )?;
    page.spatial_pyramid_validated = true;
    Ok(())
}

fn validate_page(
    metadata: PhysicalGrooveMetadata,
    available_range: GrooveFrameRange,
    page: &mut PhysicalGroovePage,
    page_index: usize,
) -> Result<(), PagedGrooveError> {
    page.core_range.validate()?;
    page.stored_range.validate()?;
    if page.generation != metadata.generation {
        return Err(PagedGrooveError::GenerationMismatch { page_index });
    }
    if page.asset_content_identity != metadata.content_identity {
        return Err(PagedGrooveError::AssetContentIdentityMismatch { page_index });
    }
    if !available_range.contains_range(page.core_range) {
        return Err(PagedGrooveError::CoreOutsideAvailableRange { page_index });
    }
    if !metadata.total_range().contains_range(page.stored_range) {
        return Err(PagedGrooveError::StoredRangeOutsideRecord { page_index });
    }
    if !page.stored_range.contains_range(page.core_range) {
        return Err(PagedGrooveError::StoredRangeDoesNotContainCore);
    }
    if page.lateral_displacement_m.len() != page.vertical_displacement_m.len() {
        return Err(PagedGrooveError::ChannelLengthMismatch);
    }
    if page.stored_range.frame_count() != page.lateral_displacement_m.len() as u64 {
        return Err(PagedGrooveError::StoredLengthMismatch);
    }
    if page
        .lateral_displacement_m
        .iter()
        .chain(page.vertical_displacement_m.iter())
        .any(|sample| !sample.is_finite())
    {
        return Err(PagedGrooveError::NonfiniteDisplacement);
    }
    page.content_identity.validate_current()?;
    if page.content_identity != page.calculate_content_identity() {
        return Err(PagedGrooveError::PageContentIdentityMismatch { page_index });
    }
    page.validate_trace_admission(metadata)?;

    let halo = u64::from(metadata.required_storage_halo_frames());
    let required_start = page.core_range.start_frame.saturating_sub(halo);
    let required_end = page
        .core_range
        .end_frame_exclusive
        .saturating_add(halo)
        .min(metadata.total_frame_count);
    if page.stored_range.start_frame > required_start {
        return Err(PagedGrooveError::InsufficientLeftHalo { page_index });
    }
    if page.stored_range.start_frame < required_start {
        return Err(PagedGrooveError::ExcessLeftHalo { page_index });
    }
    if page.stored_range.end_frame_exclusive < required_end {
        return Err(PagedGrooveError::InsufficientRightHalo { page_index });
    }
    if page.stored_range.end_frame_exclusive > required_end {
        return Err(PagedGrooveError::ExcessRightHalo { page_index });
    }
    Ok(())
}

fn validate_overlap(
    left: &PhysicalGroovePage,
    right: &PhysicalGroovePage,
    left_page_index: usize,
    right_page_index: usize,
) -> Result<(), PagedGrooveError> {
    let start = left
        .stored_range
        .start_frame
        .max(right.stored_range.start_frame);
    let end = left
        .stored_range
        .end_frame_exclusive
        .min(right.stored_range.end_frame_exclusive);
    if start >= end {
        return Err(PagedGrooveError::MissingSeamOverlap {
            left_page_index,
            right_page_index,
        });
    }
    for frame in start..end {
        let left_sample = left
            .sample_at_stored_frame(frame)
            .ok_or(PagedGrooveError::InternalPageMap)?;
        let right_sample = right
            .sample_at_stored_frame(frame)
            .ok_or(PagedGrooveError::InternalPageMap)?;
        if left_sample.lateral_displacement_m.to_bits()
            != right_sample.lateral_displacement_m.to_bits()
            || left_sample.vertical_displacement_m.to_bits()
                != right_sample.vertical_displacement_m.to_bits()
        {
            return Err(PagedGrooveError::SeamSampleMismatch {
                left_page_index,
                right_page_index,
                frame,
            });
        }
    }
    let spatial_start = start.saturating_add(u64::from(GROOVE_SPATIAL_FILTER_RADIUS_FRAMES));
    let spatial_end = end.saturating_sub(u64::from(GROOVE_SPATIAL_FILTER_RADIUS_FRAMES));
    if spatial_start < spatial_end {
        let left_levels = left
            .spatial_pyramid
            .as_ref()
            .ok_or(PagedGrooveError::MissingSpatialPyramid)?
            .levels();
        let right_levels = right
            .spatial_pyramid
            .as_ref()
            .ok_or(PagedGrooveError::MissingSpatialPyramid)?
            .levels();
        if left_levels.len() != right_levels.len() {
            return Err(PagedGrooveError::InternalPageMap);
        }
        for (level_index, (left_level, right_level)) in
            left_levels.iter().zip(right_levels).enumerate()
        {
            let step = u64::from(left_level.source_frame_step());
            if step == 0 || right_level.source_frame_step() != left_level.source_frame_step() {
                return Err(PagedGrooveError::InternalPageMap);
            }
            let mut frame =
                align_up(spatial_start, step).ok_or(PagedGrooveError::InternalPageMap)?;
            while frame < spatial_end {
                let left_index = frame
                    .checked_sub(left_level.first_source_frame())
                    .filter(|offset| offset % step == 0)
                    .and_then(|offset| usize::try_from(offset / step).ok())
                    .filter(|index| *index < left_level.lateral_displacement_m().len())
                    .ok_or(PagedGrooveError::InternalPageMap)?;
                let right_index = frame
                    .checked_sub(right_level.first_source_frame())
                    .filter(|offset| offset % step == 0)
                    .and_then(|offset| usize::try_from(offset / step).ok())
                    .filter(|index| *index < right_level.lateral_displacement_m().len())
                    .ok_or(PagedGrooveError::InternalPageMap)?;
                if left_level.lateral_displacement_m()[left_index].to_bits()
                    != right_level.lateral_displacement_m()[right_index].to_bits()
                    || left_level.vertical_displacement_m()[left_index].to_bits()
                        != right_level.vertical_displacement_m()[right_index].to_bits()
                {
                    return Err(PagedGrooveError::SpatialSeamSampleMismatch {
                        left_page_index,
                        right_page_index,
                        level_index: level_index as u8,
                        frame,
                    });
                }
                frame = frame
                    .checked_add(step)
                    .ok_or(PagedGrooveError::InternalPageMap)?;
            }
        }
    }
    Ok(())
}

fn cut_report_is_valid(report: GrooveCutReport) -> bool {
    let finite = [
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
    finite
        && report.peak_left_velocity_m_s >= 0.0
        && report.peak_right_velocity_m_s >= 0.0
        && report.rms_left_velocity_m_s >= 0.0
        && report.rms_right_velocity_m_s >= 0.0
        && report.rms_left_velocity_m_s <= report.peak_left_velocity_m_s * (1.0 + 1.0e-9)
        && report.rms_right_velocity_m_s <= report.peak_right_velocity_m_s * (1.0 + 1.0e-9)
        && report.peak_lateral_displacement_m >= 0.0
        && report.peak_vertical_displacement_m >= 0.0
}

#[derive(Debug, Error)]
pub enum PagedGrooveError {
    #[error("groove generation must be nonzero")]
    InvalidGeneration,
    #[error("groove format is not supported")]
    UnsupportedFormat,
    #[error("groove pages require an exact 192 kHz sample rate")]
    UnsupportedSampleRate,
    #[error("total groove length must contain at least four frames")]
    InvalidTotalFrameCount,
    #[error("tracing halo size is outside the supported range")]
    InvalidTracingHalo,
    #[error(
        "tracing halo has {declared_frames} frames but the selected stylus requires {required_frames}"
    )]
    InsufficientDeclaredTracingHalo {
        required_frames: u32,
        declared_frames: u32,
    },
    #[error("paged groove metadata content identity does not match its canonical fields")]
    MetadataContentIdentityMismatch,
    #[error("frame range {start_frame}..{end_frame_exclusive} is empty or reversed")]
    InvalidFrameRange {
        start_frame: u64,
        end_frame_exclusive: u64,
    },
    #[error("available frame range is outside the complete record")]
    AvailableRangeOutsideRecord,
    #[error("groove asset requires at least one page")]
    NoPages,
    #[error("page {page_index} has a different groove generation")]
    GenerationMismatch { page_index: usize },
    #[error("page {page_index} belongs to different groove content")]
    AssetContentIdentityMismatch { page_index: usize },
    #[error("page {page_index} content identity does not match its payload")]
    PageContentIdentityMismatch { page_index: usize },
    #[error("page {page_index} core is outside the available range")]
    CoreOutsideAvailableRange { page_index: usize },
    #[error("page {page_index} storage is outside the complete record")]
    StoredRangeOutsideRecord { page_index: usize },
    #[error("stored frame range does not contain the page core")]
    StoredRangeDoesNotContainCore,
    #[error("groove page channels must have equal lengths")]
    ChannelLengthMismatch,
    #[error("groove page storage length does not match its frame range")]
    StoredLengthMismatch,
    #[error("groove page contains a nonfinite displacement")]
    NonfiniteDisplacement,
    #[error("groove page has no spatial pyramid")]
    MissingSpatialPyramid,
    #[error("groove page has no trace-admission certificate")]
    MissingTraceAdmissionCertificate,
    #[error("page {page_index} does not contain the required earlier tracing halo")]
    InsufficientLeftHalo { page_index: usize },
    #[error("page {page_index} contains data before its declared tracing halo")]
    ExcessLeftHalo { page_index: usize },
    #[error("page {page_index} does not contain the required later tracing halo")]
    InsufficientRightHalo { page_index: usize },
    #[error("page {page_index} contains data after its declared tracing halo")]
    ExcessRightHalo { page_index: usize },
    #[error("groove page map has a gap: expected {expected_frame}, found {actual_frame}")]
    CoreGap {
        expected_frame: u64,
        actual_frame: u64,
    },
    #[error("groove page map overlaps: expected {expected_frame}, found {actual_frame}")]
    CoreOverlap {
        expected_frame: u64,
        actual_frame: u64,
    },
    #[error("pages {left_page_index} and {right_page_index} do not share a tracing seam")]
    MissingSeamOverlap {
        left_page_index: usize,
        right_page_index: usize,
    },
    #[error("pages {left_page_index} and {right_page_index} disagree at frame {frame}")]
    SeamSampleMismatch {
        left_page_index: usize,
        right_page_index: usize,
        frame: u64,
    },
    #[error(
        "pages {left_page_index} and {right_page_index} disagree in spatial level {level_index} at frame {frame}"
    )]
    SpatialSeamSampleMismatch {
        left_page_index: usize,
        right_page_index: usize,
        level_index: u8,
        frame: u64,
    },
    #[error("frame {frame} is outside available range {available_range:?}")]
    FrameUnavailable {
        frame: u64,
        available_range: GrooveFrameRange,
    },
    #[error("absolute groove frame position must be finite and nonnegative")]
    InvalidFramePosition,
    #[error("absolute groove frame position is outside the complete record")]
    FramePositionOutsideRecord,
    #[error("groove page map is internally inconsistent")]
    InternalPageMap,
    #[error("groove cut report is invalid")]
    InvalidCutReport,
    #[error("groove cut and report use different pitches")]
    CutPitchMismatch,
    #[error("groove cut report does not match the total record length")]
    CutLengthMismatch,
    #[error("paged groove cache limit {field} is invalid")]
    InvalidCacheLimits { field: &'static str },
    #[error("paged groove cache cannot contain more than {maximum_pages} pages")]
    CachePageLimitExceeded { maximum_pages: u32 },
    #[error(
        "paged groove cache needs {requested_resident_bytes} bytes but permits {maximum_resident_bytes} bytes"
    )]
    CacheResidentLimitExceeded {
        maximum_resident_bytes: u64,
        requested_resident_bytes: u64,
    },
    #[error("paged groove cache size calculation overflowed")]
    CacheSizeOverflow,
    #[error("prefetch plan cannot contain more than {maximum_candidates} recapture candidates")]
    TooManyPrefetchCandidates { maximum_candidates: usize },
    #[error("cached core {inserted_core:?} overlaps existing core {existing_core:?}")]
    CacheCoreOverlap {
        existing_core: GrooveFrameRange,
        inserted_core: GrooveFrameRange,
    },
    #[error("render speed is outside the supported range of plus or minus 20 times")]
    UnsupportedRenderSpeed,
    #[error(transparent)]
    Groove(#[from] GrooveError),
    #[error(transparent)]
    Stylus(#[from] StylusTraceError),
    #[error(transparent)]
    TraceAdmission(#[from] GrooveTraceAdmissionError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physical::{trace_spherical_uniform, GrooveAsset};

    const TOTAL_FRAMES: u64 = 4_096;
    const SEAM_FRAME: u64 = 2_048;
    const HALO_FRAMES: u32 = 64;

    fn storage_halo_frames() -> u64 {
        u64::from(HALO_FRAMES + PAGED_GROOVE_SPATIAL_STORAGE_MARGIN_FRAMES)
    }

    fn generation(value: u64) -> GrooveGenerationId {
        GrooveGenerationId::new(value).unwrap()
    }

    fn source_content_identity() -> GrooveContentIdentity {
        GrooveContentIdentity::from_sha256([0xa5; 32])
    }

    fn master_sample(frame: u64) -> GrooveSample {
        GrooveSample {
            lateral_displacement_m: frame as f32 * 1.0e-7,
            vertical_displacement_m: -(frame as f32) * 2.0e-7,
        }
    }

    fn cut_metadata(total_frame_count: u64) -> PhysicalGrooveCutMetadata {
        let layout = GrooveLayout::lp_33_seed();
        let cut = RecordCutConfig::seed();
        let final_program_radius_m = layout.unclamped_radius_at_frame(
            total_frame_count.saturating_sub(1) as f64,
            cut.groove_pitch_m_per_revolution,
        );
        PhysicalGrooveCutMetadata::new(
            layout,
            cut,
            GrooveCutReport {
                peak_left_velocity_m_s: 0.05,
                peak_right_velocity_m_s: 0.05,
                rms_left_velocity_m_s: 0.025,
                rms_right_velocity_m_s: 0.025,
                peak_lateral_displacement_m: 10.0e-6,
                peak_vertical_displacement_m: 5.0e-6,
                final_lateral_drift_m: 0.0,
                final_vertical_drift_m: 0.0,
                groove_pitch_m_per_revolution: cut.groove_pitch_m_per_revolution,
                final_program_radius_m,
                programme_exceeds_available_radius: false,
                minimum_adjacent_turn_clearance_m: None,
                first_failing_clearance_frame_pair: None,
                adjacent_turn_clearance_failed: false,
            },
        )
    }

    fn metadata(generation: GrooveGenerationId) -> PhysicalGrooveMetadata {
        PhysicalGrooveMetadata::new(
            generation,
            source_content_identity(),
            cut_metadata(TOTAL_FRAMES),
            TOTAL_FRAMES,
            HALO_FRAMES,
        )
        .unwrap()
    }

    fn page(
        generation: GrooveGenerationId,
        core_start: u64,
        core_end: u64,
        stored_start: u64,
        stored_end: u64,
    ) -> PhysicalGroovePage {
        page_for_metadata(
            metadata(generation),
            core_start,
            core_end,
            stored_start,
            stored_end,
        )
    }

    fn page_for_metadata(
        metadata: PhysicalGrooveMetadata,
        core_start: u64,
        core_end: u64,
        stored_start: u64,
        stored_end: u64,
    ) -> PhysicalGroovePage {
        let samples: Vec<GrooveSample> = (stored_start..stored_end).map(master_sample).collect();
        PhysicalGroovePage::new(
            metadata,
            GrooveFrameRange::new(core_start, core_end).unwrap(),
            GrooveFrameRange::new(stored_start, stored_end).unwrap(),
            samples
                .iter()
                .map(|sample| sample.lateral_displacement_m)
                .collect(),
            samples
                .iter()
                .map(|sample| sample.vertical_displacement_m)
                .collect(),
        )
        .unwrap()
    }

    fn complete_asset() -> PagedGrooveAsset {
        let generation = generation(7);
        let storage_halo = storage_halo_frames();
        PagedGrooveAsset::new(
            metadata(generation),
            GrooveFrameRange::new(0, TOTAL_FRAMES).unwrap(),
            vec![
                page(generation, 0, SEAM_FRAME, 0, SEAM_FRAME + storage_halo),
                page(
                    generation,
                    SEAM_FRAME,
                    TOTAL_FRAMES,
                    SEAM_FRAME - storage_halo,
                    TOTAL_FRAMES,
                ),
            ],
        )
        .unwrap()
    }

    fn complete_cache() -> PagedGrooveCache {
        let generation = generation(7);
        let storage_halo = storage_halo_frames();
        let mut producer =
            PagedGrooveCacheProducer::new(metadata(generation), PagedGrooveCacheLimits::default())
                .unwrap();
        producer
            .insert_page(page(
                generation,
                0,
                SEAM_FRAME,
                0,
                SEAM_FRAME + storage_halo,
            ))
            .unwrap();
        producer
            .insert_page(page(
                generation,
                SEAM_FRAME,
                TOTAL_FRAMES,
                SEAM_FRAME - storage_halo,
                TOTAL_FRAMES,
            ))
            .unwrap();
        producer.publish()
    }

    fn cache_trace(
        cache: &PagedGrooveCache,
        position: f64,
        source_frame_advance: f64,
    ) -> PagedGrooveTraceFrame {
        let request =
            PagedGrooveRenderRequest::new(cache.generation(), position, source_frame_advance)
                .unwrap();
        match cache
            .trace(
                request,
                StylusGeometry {
                    tracing_radius_m: 10.0e-6,
                },
            )
            .unwrap()
        {
            PagedGrooveTraceResolution::Ready(trace) => trace,
            PagedGrooveTraceResolution::Miss(miss) => {
                panic!("unexpected cache miss: {miss:?}")
            }
        }
    }

    fn trace_without_certified_position_intervals(
        mut trace: StylusTraceContactSet,
    ) -> StylusTraceContactSet {
        for contact in &mut trace.contacts[..usize::from(trace.contact_count)] {
            contact.certified_position_interval = None;
        }
        trace
    }

    fn assert_same_trace_and_material_cell(
        actual: StylusTraceContactSet,
        expected: StylusTraceContactSet,
        generation: GrooveGenerationId,
        wall: usize,
    ) {
        assert_eq!(
            trace_without_certified_position_intervals(actual),
            trace_without_certified_position_intervals(expected)
        );
        let resolve = |contacts| {
            super::super::tangential_identity::resolve_tangential_contact_identity(
                source_content_identity(),
                generation.get(),
                TOTAL_FRAMES,
                wall,
                contacts,
            )
            .unwrap()
        };
        assert_eq!(resolve(actual), resolve(expected));
    }

    fn assert_same_seam_trace_and_material_cell(
        actual: StylusTraceContactSet,
        expected: StylusTraceContactSet,
        generation: GrooveGenerationId,
        wall: usize,
    ) {
        assert_eq!(
            actual.center_displacement_m.to_bits(),
            expected.center_displacement_m.to_bits()
        );
        assert_eq!(actual.contact_count, expected.contact_count);
        let count = usize::from(actual.contact_count);
        for (actual, expected) in actual.contacts[..count]
            .iter()
            .zip(&expected.contacts[..count])
        {
            assert_eq!(
                actual.groove_displacement_m.to_bits(),
                expected.groove_displacement_m.to_bits()
            );
            assert_eq!(
                actual.groove_slope.to_bits(),
                expected.groove_slope.to_bits()
            );
            assert!(
                (actual.contact_offset_m - expected.contact_offset_m).abs()
                    <= super::super::stylus::SPHERICAL_TRACE_CONTACT_POSITION_ERROR_BOUND_M
            );
            assert!(
                (actual.tangent_residual - expected.tangent_residual).abs()
                    <= super::super::stylus::SPHERICAL_TRACE_TANGENT_RESIDUAL_ERROR_BOUND
            );
        }
        let resolve = |contacts| {
            super::super::tangential_identity::resolve_tangential_contact_identity(
                source_content_identity(),
                generation.get(),
                TOTAL_FRAMES,
                wall,
                contacts,
            )
            .unwrap()
        };
        assert_eq!(resolve(actual), resolve(expected));
    }

    #[test]
    fn forward_and_reverse_lookup_are_identical_across_the_seam() {
        let asset = complete_asset();
        let forward: Vec<GrooveSample> = (SEAM_FRAME - 3..SEAM_FRAME + 3)
            .map(|frame| asset.sample_at(frame).unwrap())
            .collect();
        let reverse: Vec<GrooveSample> = (SEAM_FRAME - 3..SEAM_FRAME + 3)
            .rev()
            .map(|frame| asset.sample_at(frame).unwrap())
            .collect();
        assert_eq!(forward, reverse.into_iter().rev().collect::<Vec<_>>());

        let forward_view = asset
            .trace_view(SEAM_FRAME as f64 - 0.25, GrooveTravelDirection::Forward)
            .unwrap();
        let reverse_view = asset
            .trace_view(SEAM_FRAME as f64 + 0.25, GrooveTravelDirection::Reverse)
            .unwrap();
        for frame in SEAM_FRAME - 4..SEAM_FRAME + 4 {
            assert_eq!(
                forward_view.sample_at_absolute_frame(frame),
                reverse_view.sample_at_absolute_frame(frame)
            );
        }
        assert_eq!(
            forward_view.sample_in_travel_direction(3),
            Some(master_sample(SEAM_FRAME + 2))
        );
        assert_eq!(
            reverse_view.sample_in_travel_direction(3),
            Some(master_sample(SEAM_FRAME - 3))
        );
    }

    #[test]
    fn trace_results_match_one_contiguous_source_at_both_seam_sides() {
        let asset = complete_asset();
        let master: Vec<f32> = (0..TOTAL_FRAMES)
            .map(|frame| master_sample(frame).lateral_displacement_m)
            .collect();
        for (position, direction) in [
            (SEAM_FRAME as f64 - 0.25, GrooveTravelDirection::Forward),
            (SEAM_FRAME as f64 + 0.25, GrooveTravelDirection::Reverse),
        ] {
            let view = asset.trace_view(position, direction).unwrap();
            let page_trace = trace_spherical_uniform(
                view.lateral_displacement_m(),
                view.local_frame_position(),
                5.0e-6,
                crate::physical::StylusGeometry {
                    tracing_radius_m: 10.0e-6,
                },
            )
            .unwrap();
            let contiguous_trace = trace_spherical_uniform(
                &master,
                position,
                5.0e-6,
                crate::physical::StylusGeometry {
                    tracing_radius_m: 10.0e-6,
                },
            )
            .unwrap();
            assert_eq!(
                page_trace.center_displacement_m.to_bits(),
                contiguous_trace.center_displacement_m.to_bits()
            );
            assert_eq!(
                page_trace.groove_slope.to_bits(),
                contiguous_trace.groove_slope.to_bits()
            );
        }
    }

    fn trace_selection(
        selection: GrooveSpatialLevelSelection<'_>,
        position: f64,
    ) -> StylusTraceContactSet {
        let lower = selection.lower();
        let upper = selection.upper();
        trace_spherical_45_45_wall_multiresolution_contacts(
            lower.lateral_displacement_m(),
            lower.vertical_displacement_m(),
            lower.first_source_frame(),
            lower.source_frame_step(),
            upper.lateral_displacement_m(),
            upper.vertical_displacement_m(),
            upper.first_source_frame(),
            upper.source_frame_step(),
            selection.upper_level_blend(),
            0,
            position,
            5.0e-6,
            StylusGeometry {
                tracing_radius_m: 10.0e-6,
            },
        )
        .unwrap()
    }

    #[test]
    fn antialiased_forward_and_reverse_traces_match_the_contiguous_pyramid_at_seams() {
        let asset = complete_asset();
        let master_lateral: Vec<f32> = (0..TOTAL_FRAMES)
            .map(|frame| master_sample(frame).lateral_displacement_m)
            .collect();
        let master_vertical: Vec<f32> = (0..TOTAL_FRAMES)
            .map(|frame| master_sample(frame).vertical_displacement_m)
            .collect();
        let contiguous = GrooveSpatialPyramid::build(&master_lateral, &master_vertical).unwrap();
        for (position, direction, advance) in [
            (
                SEAM_FRAME as f64 - 0.25,
                GrooveTravelDirection::Forward,
                20.0,
            ),
            (
                SEAM_FRAME as f64 - 0.25,
                GrooveTravelDirection::Reverse,
                -20.0,
            ),
            (
                SEAM_FRAME as f64 + 0.25,
                GrooveTravelDirection::Forward,
                20.0,
            ),
            (
                SEAM_FRAME as f64 + 0.25,
                GrooveTravelDirection::Reverse,
                -20.0,
            ),
        ] {
            let page_view = asset
                .antialiased_trace_view(position, direction, advance)
                .unwrap();
            let page_trace = page_view
                .trace_wall_contacts(
                    0,
                    5.0e-6,
                    StylusGeometry {
                        tracing_radius_m: 10.0e-6,
                    },
                )
                .unwrap();
            let contiguous_trace = trace_selection(
                contiguous
                    .select(0, &master_lateral, &master_vertical, advance)
                    .unwrap(),
                position,
            );
            assert_same_seam_trace_and_material_cell(
                page_trace,
                contiguous_trace,
                asset.metadata().generation(),
                0,
            );
        }
    }

    #[test]
    fn construction_rejects_missing_core_ranges() {
        let generation = generation(9);
        let storage_halo = storage_halo_frames();
        let error = PagedGrooveAsset::new(
            metadata(generation),
            GrooveFrameRange::new(0, TOTAL_FRAMES).unwrap(),
            vec![
                page(
                    generation,
                    0,
                    SEAM_FRAME - 1,
                    0,
                    SEAM_FRAME - 1 + storage_halo,
                ),
                page(
                    generation,
                    SEAM_FRAME,
                    TOTAL_FRAMES,
                    SEAM_FRAME - storage_halo,
                    TOTAL_FRAMES,
                ),
            ],
        )
        .unwrap_err();
        assert!(matches!(
            error,
            PagedGrooveError::CoreGap {
                expected_frame: 2_047,
                actual_frame: 2_048
            }
        ));
    }

    #[test]
    fn lookup_reports_frames_outside_the_explicit_available_range() {
        let generation = generation(11);
        let available_start = 1_024;
        let available_end = 3_072;
        let storage_halo = storage_halo_frames();
        let asset = PagedGrooveAsset::new(
            metadata(generation),
            GrooveFrameRange::new(available_start, available_end).unwrap(),
            vec![page(
                generation,
                available_start,
                available_end,
                available_start - storage_halo,
                available_end + storage_halo,
            )],
        )
        .unwrap();
        assert_eq!(
            asset.available_range(),
            GrooveFrameRange::new(available_start, available_end).unwrap()
        );
        assert!(matches!(
            asset.sample_at(available_start - 1),
            Err(PagedGrooveError::FrameUnavailable { frame: 1_023, .. })
        ));
        assert!(matches!(
            asset.sample_at(available_end),
            Err(PagedGrooveError::FrameUnavailable { frame: 3_072, .. })
        ));
    }

    #[test]
    fn internal_pages_require_earlier_and_later_tracing_halos() {
        let generation = generation(13);
        let available_start = 1_024;
        let available_end = 3_072;
        let storage_halo = storage_halo_frames();
        let required_start = available_start - storage_halo;
        let required_end = available_end + storage_halo;
        let left_error = PagedGrooveAsset::new(
            metadata(generation),
            GrooveFrameRange::new(available_start, available_end).unwrap(),
            vec![page(
                generation,
                available_start,
                available_end,
                required_start + 1,
                required_end,
            )],
        )
        .unwrap_err();
        assert!(matches!(
            left_error,
            PagedGrooveError::InsufficientLeftHalo { page_index: 0 }
        ));

        let right_error = PagedGrooveAsset::new(
            metadata(generation),
            GrooveFrameRange::new(available_start, available_end).unwrap(),
            vec![page(
                generation,
                available_start,
                available_end,
                required_start,
                required_end - 1,
            )],
        )
        .unwrap_err();
        assert!(matches!(
            right_error,
            PagedGrooveError::InsufficientRightHalo { page_index: 0 }
        ));
    }

    #[test]
    fn metadata_preserves_total_length_and_record_radius() {
        let metadata = metadata(generation(15));
        assert_eq!(metadata.total_frame_count(), TOTAL_FRAMES);
        assert_eq!(
            metadata.total_duration_seconds(),
            TOTAL_FRAMES as f64 / PHYSICAL_GROOVE_SAMPLE_RATE_HZ as f64
        );
        assert_eq!(
            metadata.radius_at_absolute_frame(0.0).unwrap(),
            metadata.cut().layout().outer_program_radius_m
        );
        assert_eq!(
            metadata
                .radius_at_absolute_frame((TOTAL_FRAMES - 1) as f64)
                .unwrap(),
            metadata.final_program_radius_m()
        );
        assert_eq!(metadata.format().sample_rate_hz(), 192_000);
        assert_eq!(
            metadata.source_content_identity(),
            source_content_identity()
        );
        assert_eq!(
            metadata.format().format_version(),
            PAGED_GROOVE_FORMAT_VERSION
        );
        metadata
            .validate_tracing_geometry(StylusGeometry::default())
            .unwrap();
    }

    #[test]
    fn metadata_identity_is_stable_across_generations_and_binds_canonical_fields() {
        let first = metadata(generation(101));
        let second = metadata(generation(102));
        assert_eq!(first.content_identity(), second.content_identity());

        let changed_source = PhysicalGrooveMetadata::new(
            generation(101),
            GrooveContentIdentity::from_sha256([0x5a; 32]),
            cut_metadata(TOTAL_FRAMES),
            TOTAL_FRAMES,
            HALO_FRAMES,
        )
        .unwrap();
        assert_ne!(first.content_identity(), changed_source.content_identity());

        let changed_halo = PhysicalGrooveMetadata::new(
            generation(101),
            source_content_identity(),
            cut_metadata(TOTAL_FRAMES),
            TOTAL_FRAMES,
            HALO_FRAMES + 1,
        )
        .unwrap();
        assert_ne!(first.content_identity(), changed_halo.content_identity());

        let mut serialized = serde_json::to_value(first).unwrap();
        serialized["tracingHaloFrames"] = serde_json::json!(HALO_FRAMES + 1);
        let changed: PhysicalGrooveMetadata = serde_json::from_value(serialized).unwrap();
        assert!(matches!(
            PagedGrooveCacheProducer::new(changed, PagedGrooveCacheLimits::default()),
            Err(PagedGrooveError::MetadataContentIdentityMismatch)
        ));
    }

    #[test]
    fn metadata_from_contiguous_asset_uses_the_asset_identity() {
        let velocity = vec![0.01_f32; TOTAL_FRAMES as usize];
        let asset = GrooveAsset::from_stereo_wall_velocity_m_s(
            &velocity,
            &velocity,
            GrooveLayout::lp_33_seed(),
        )
        .unwrap();
        let metadata =
            PhysicalGrooveMetadata::from_groove_asset(generation(103), &asset, HALO_FRAMES)
                .unwrap();
        assert_eq!(
            metadata.source_content_identity(),
            asset.provenance().content_identity()
        );
        assert_eq!(metadata.total_frame_count(), asset.frame_count() as u64);
    }

    #[test]
    fn construction_rejects_a_page_from_another_generation() {
        let expected = generation(17);
        let other = generation(18);
        let error = PagedGrooveAsset::new(
            metadata(expected),
            GrooveFrameRange::new(0, TOTAL_FRAMES).unwrap(),
            vec![page(other, 0, TOTAL_FRAMES, 0, TOTAL_FRAMES)],
        )
        .unwrap_err();
        assert!(matches!(
            error,
            PagedGrooveError::GenerationMismatch { page_index: 0 }
        ));
    }

    #[test]
    fn construction_rejects_overlapping_core_ranges_and_changed_seams() {
        let generation = generation(21);
        let storage_halo = storage_halo_frames();
        let overlap_error = PagedGrooveAsset::new(
            metadata(generation),
            GrooveFrameRange::new(0, TOTAL_FRAMES).unwrap(),
            vec![
                page(
                    generation,
                    0,
                    SEAM_FRAME + 1,
                    0,
                    SEAM_FRAME + 1 + storage_halo,
                ),
                page(
                    generation,
                    SEAM_FRAME,
                    TOTAL_FRAMES,
                    SEAM_FRAME - storage_halo,
                    TOTAL_FRAMES,
                ),
            ],
        )
        .unwrap_err();
        assert!(matches!(
            overlap_error,
            PagedGrooveError::CoreOverlap { .. }
        ));

        let first = page(generation, 0, SEAM_FRAME, 0, SEAM_FRAME + storage_halo);
        let mut second_samples: Vec<GrooveSample> = (SEAM_FRAME - storage_halo..TOTAL_FRAMES)
            .map(master_sample)
            .collect();
        second_samples[0].lateral_displacement_m += 1.0e-6;
        let second = PhysicalGroovePage::new(
            metadata(generation),
            GrooveFrameRange::new(SEAM_FRAME, TOTAL_FRAMES).unwrap(),
            GrooveFrameRange::new(SEAM_FRAME - storage_halo, TOTAL_FRAMES).unwrap(),
            second_samples
                .iter()
                .map(|sample| sample.lateral_displacement_m)
                .collect(),
            second_samples
                .iter()
                .map(|sample| sample.vertical_displacement_m)
                .collect(),
        )
        .unwrap();
        let seam_error = PagedGrooveAsset::new(
            metadata(generation),
            GrooveFrameRange::new(0, TOTAL_FRAMES).unwrap(),
            vec![first, second],
        )
        .unwrap_err();
        assert!(matches!(
            seam_error,
            PagedGrooveError::SeamSampleMismatch { frame, .. }
                if frame == SEAM_FRAME - storage_halo
        ));
    }

    #[test]
    fn immutable_asset_is_safe_to_share_between_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PagedGrooveAsset>();
    }

    #[test]
    fn bounded_cache_matches_the_complete_paged_asset() {
        let asset = complete_asset();
        let cache = complete_cache();
        let geometry = StylusGeometry {
            tracing_radius_m: 10.0e-6,
        };
        for position in [
            2.25,
            511.75,
            SEAM_FRAME as f64 - 0.25,
            SEAM_FRAME as f64 + 0.25,
            TOTAL_FRAMES as f64 - 2.25,
        ] {
            for source_frame_advance in [-20.0, -7.5, 0.0, 3.25, 20.0] {
                let cached = cache_trace(&cache, position, source_frame_advance);
                let direction = if source_frame_advance < 0.0 {
                    GrooveTravelDirection::Reverse
                } else {
                    GrooveTravelDirection::Forward
                };
                let view = asset
                    .antialiased_trace_view(position, direction, source_frame_advance)
                    .unwrap();
                let meters_per_source_frame = asset.metadata().cut().layout().meters_per_frame_at(
                    position,
                    asset.metadata().cut().cut().groove_pitch_m_per_revolution,
                );
                let expected = [
                    view.trace_wall_contacts(0, meters_per_source_frame, geometry)
                        .unwrap(),
                    view.trace_wall_contacts(1, meters_per_source_frame, geometry)
                        .unwrap(),
                ];
                assert_eq!(cached.wall_contacts, expected);
                assert_eq!(cached.direction, direction);
                assert_eq!(
                    cached.spatial_filter_lower_step_frames,
                    view.level_selection().lower().source_frame_step()
                );
                assert_eq!(
                    cached.spatial_filter_upper_step_frames,
                    view.level_selection().upper().source_frame_step()
                );
                assert_eq!(
                    cached.spatial_filter_upper_blend.to_bits(),
                    view.level_selection().upper_level_blend().to_bits()
                );
            }
        }
    }

    #[test]
    fn bounded_cache_matches_a_monolithic_groove_asset() {
        let left_velocity: Vec<_> = (0..TOTAL_FRAMES)
            .map(|frame| (std::f64::consts::TAU * frame as f64 / 73.25).sin() as f32 * 0.05)
            .collect();
        let right_velocity: Vec<_> = (0..TOTAL_FRAMES)
            .map(|frame| (std::f64::consts::TAU * frame as f64 / 117.75).cos() as f32 * 0.035)
            .collect();
        let whole = GrooveAsset::from_stereo_wall_velocity_m_s(
            &left_velocity,
            &right_velocity,
            GrooveLayout::lp_33_seed(),
        )
        .unwrap();
        let generation = generation(43);
        let metadata =
            PhysicalGrooveMetadata::from_groove_asset(generation, &whole, HALO_FRAMES).unwrap();
        let storage_halo = storage_halo_frames();
        let mut producer =
            PagedGrooveCacheProducer::new(metadata, PagedGrooveCacheLimits::default()).unwrap();
        for (core_start, core_end, stored_start, stored_end) in [
            (0, SEAM_FRAME, 0, SEAM_FRAME + storage_halo),
            (
                SEAM_FRAME,
                TOTAL_FRAMES,
                SEAM_FRAME - storage_halo,
                TOTAL_FRAMES,
            ),
        ] {
            let stored = stored_start as usize..stored_end as usize;
            producer
                .insert_page(
                    PhysicalGroovePage::new(
                        metadata,
                        GrooveFrameRange::new(core_start, core_end).unwrap(),
                        GrooveFrameRange::new(stored_start, stored_end).unwrap(),
                        whole.lateral_displacement_m()[stored.clone()].to_vec(),
                        whole.vertical_displacement_m()[stored].to_vec(),
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let cache = producer.publish();
        let geometry = StylusGeometry {
            tracing_radius_m: 10.0e-6,
        };
        let whole_admission = whole.validated_trace_admission().unwrap();
        whole_admission
            .validate_for_active_tracing(geometry)
            .unwrap();
        let whole_admission_class = whole_admission.certificate().admission_class();
        assert_eq!(
            whole_admission_class,
            GrooveTraceAdmissionClass::FixedCapPiecewise
        );
        for position in [
            3.25,
            731.75,
            SEAM_FRAME as f64 - 0.25,
            SEAM_FRAME as f64 + 0.25,
            TOTAL_FRAMES as f64 - 3.25,
        ] {
            for source_frame_advance in [-20.0, -3.5, 1.0, 8.0, 20.0] {
                let cached = cache_trace(&cache, position, source_frame_advance);
                let page = cache
                    .pages()
                    .find(|page| page.core_range().contains(position.floor() as u64))
                    .unwrap();
                let page_admission_class = page
                    .validated_trace_admission()
                    .unwrap()
                    .certificate()
                    .admission_class();
                assert_eq!(
                    page_admission_class, whole_admission_class,
                    "position {position}, source advance {source_frame_advance}"
                );
                let selection = whole.spatial_level_selection(source_frame_advance).unwrap();
                let lower = selection.lower();
                let upper = selection.upper();
                let meters_per_source_frame = whole.meters_per_frame_at(position);
                for wall in 0..2 {
                    let expected = match whole_admission_class {
                        GrooveTraceAdmissionClass::StrictConcavity => {
                            trace_spherical_45_45_wall_multiresolution_contacts_certified_concave(
                                lower.lateral_displacement_m(),
                                lower.vertical_displacement_m(),
                                lower.first_source_frame(),
                                lower.source_frame_step(),
                                upper.lateral_displacement_m(),
                                upper.vertical_displacement_m(),
                                upper.first_source_frame(),
                                upper.source_frame_step(),
                                selection.upper_level_blend(),
                                wall,
                                position,
                                meters_per_source_frame,
                                geometry,
                                whole_admission
                                    .certified_concave_trace_bounds(geometry)
                                    .unwrap(),
                            )
                        }
                        GrooveTraceAdmissionClass::FixedCapPiecewise => {
                            trace_spherical_45_45_wall_multiresolution_contacts_certified_piecewise(
                                lower.lateral_displacement_m(),
                                lower.vertical_displacement_m(),
                                lower.first_source_frame(),
                                lower.source_frame_step(),
                                upper.lateral_displacement_m(),
                                upper.vertical_displacement_m(),
                                upper.first_source_frame(),
                                upper.source_frame_step(),
                                selection.upper_level_blend(),
                                wall,
                                position,
                                meters_per_source_frame,
                                geometry,
                                whole_admission
                                    .fixed_cap_piecewise_trace_bounds(geometry)
                                    .unwrap(),
                            )
                        }
                        rejected => {
                            panic!("monolithic fixture was not trace-admitted: {rejected:?}")
                        }
                    }
                    .unwrap();
                    let actual = cached.wall_contacts[wall];
                    assert_same_trace_and_material_cell(actual, expected, cache.generation(), wall);
                }
            }
        }
    }

    #[test]
    fn cache_traces_rapid_reversals_across_page_seams() {
        let cache = complete_cache();
        let mut position = SEAM_FRAME as f64 - 260.75;
        let mut advance = 20.0;
        let mut saw_left = false;
        let mut saw_right = false;
        for _ in 0..4_000 {
            let trace = cache_trace(&cache, position, advance);
            saw_left |= trace.page_core_range.end_frame_exclusive() == SEAM_FRAME;
            saw_right |= trace.page_core_range.start_frame() == SEAM_FRAME;
            let next = position + advance;
            if next >= SEAM_FRAME as f64 + 280.0 || next <= SEAM_FRAME as f64 - 280.0 {
                advance = -advance;
            }
            position += advance;
        }
        assert!(saw_left && saw_right);
    }

    #[test]
    fn trace_admission_rejects_a_halo_that_cannot_support_the_policy() {
        let generation = generation(44);
        let declared_halo = 12;
        let metadata = PhysicalGrooveMetadata::new(
            generation,
            source_content_identity(),
            cut_metadata(TOTAL_FRAMES),
            TOTAL_FRAMES,
            declared_halo,
        )
        .unwrap();
        let geometry = StylusGeometry {
            tracing_radius_m: 10.0e-6,
        };
        assert!(matches!(
            metadata.validate_tracing_geometry(geometry),
            Err(PagedGrooveError::InsufficientDeclaredTracingHalo {
                declared_frames: 12,
                ..
            })
        ));

        let storage_halo = u64::from(metadata.required_storage_halo_frames());
        let stored_end = SEAM_FRAME + storage_halo;
        let samples: Vec<_> = (0..stored_end).map(master_sample).collect();
        assert!(matches!(
            PhysicalGroovePage::new(
                metadata,
                GrooveFrameRange::new(0, SEAM_FRAME).unwrap(),
                GrooveFrameRange::new(0, stored_end).unwrap(),
                samples
                    .iter()
                    .map(|sample| sample.lateral_displacement_m)
                    .collect(),
                samples
                    .iter()
                    .map(|sample| sample.vertical_displacement_m)
                    .collect(),
            ),
            Err(PagedGrooveError::TraceAdmission(
                GrooveTraceAdmissionError::InvalidBinding
            ))
        ));
    }

    #[test]
    fn missing_page_is_an_explicit_underrun_and_does_not_change_the_cache() {
        let generation = generation(7);
        let storage_halo = storage_halo_frames();
        let mut producer =
            PagedGrooveCacheProducer::new(metadata(generation), PagedGrooveCacheLimits::default())
                .unwrap();
        producer
            .insert_page(page(
                generation,
                0,
                SEAM_FRAME,
                0,
                SEAM_FRAME + storage_halo,
            ))
            .unwrap();
        let cache = producer.publish();
        let before = cache_trace(&cache, SEAM_FRAME as f64 - 10.25, 20.0);
        let request =
            PagedGrooveRenderRequest::new(generation, SEAM_FRAME as f64 + 10.25, -20.0).unwrap();
        assert_eq!(
            cache.trace(request, StylusGeometry::default()).unwrap(),
            PagedGrooveTraceResolution::Miss(PagedGrooveRenderMiss::PageUnavailable {
                generation,
                frame: SEAM_FRAME + 10,
            })
        );
        assert_eq!(cache_trace(&cache, SEAM_FRAME as f64 - 10.25, 20.0), before);
        assert_eq!(cache.page_count(), 1);
    }

    #[test]
    fn stale_generations_are_rejected_without_hiding_a_valid_cache() {
        let cache = complete_cache();
        let stale_generation = generation(8);
        let stale_request =
            PagedGrooveRenderRequest::new(stale_generation, 1_000.5, -20.0).unwrap();
        assert!(matches!(
            cache.resolve(stale_request).unwrap(),
            PagedGrooveRenderResolution::Miss(PagedGrooveRenderMiss::StaleGeneration {
                requested_generation,
                cached_generation,
            }) if requested_generation == stale_generation && cached_generation == generation(7)
        ));
        assert_eq!(
            cache_trace(&cache, 1_000.5, -20.0).generation,
            generation(7)
        );

        let mut producer = PagedGrooveCacheProducer::new(
            metadata(generation(7)),
            PagedGrooveCacheLimits::default(),
        )
        .unwrap();
        let error = producer
            .insert_page(page(stale_generation, 0, TOTAL_FRAMES, 0, TOTAL_FRAMES))
            .unwrap_err();
        assert!(matches!(error, PagedGrooveError::GenerationMismatch { .. }));
        assert_eq!(producer.page_count(), 0);
    }

    #[test]
    fn render_misses_round_trip_as_typed_values() {
        let miss = PagedGrooveRenderMiss::PageUnavailable {
            generation: generation(9),
            frame: 123,
        };
        let encoded = serde_json::to_string(&miss).unwrap();
        let decoded: PagedGrooveRenderMiss = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, miss);
        assert_eq!(
            miss.to_string(),
            "groove page for frame 123 in generation GrooveGenerationId(9) is unavailable"
        );
    }

    #[test]
    fn producer_enforces_page_and_byte_limits_transactionally() {
        let generation = generation(31);
        let storage_halo = storage_halo_frames();
        let first = page(generation, 0, SEAM_FRAME, 0, SEAM_FRAME + storage_halo);
        let first_bytes = first.resident_size_bytes();
        let limits = PagedGrooveCacheLimits::new(1, first_bytes).unwrap();
        let mut producer = PagedGrooveCacheProducer::new(metadata(generation), limits).unwrap();
        producer.insert_page(first).unwrap();
        let resident_before = producer.resident_page_bytes();
        let error = producer
            .insert_page(page(
                generation,
                SEAM_FRAME,
                TOTAL_FRAMES,
                SEAM_FRAME - storage_halo,
                TOTAL_FRAMES,
            ))
            .unwrap_err();
        assert!(matches!(
            error,
            PagedGrooveError::CachePageLimitExceeded { maximum_pages: 1 }
        ));
        assert_eq!(producer.page_count(), 1);
        assert_eq!(producer.resident_page_bytes(), resident_before);
        let cache = producer.publish();
        assert!(cache.resident_page_bytes() <= cache.limits().maximum_resident_bytes());

        let candidate = page(generation, 0, TOTAL_FRAMES, 0, TOTAL_FRAMES);
        let candidate_bytes = candidate.resident_size_bytes();
        let mut byte_limited = PagedGrooveCacheProducer::new(
            metadata(generation),
            PagedGrooveCacheLimits::new(1, candidate_bytes - 1).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            byte_limited.insert_page(candidate),
            Err(PagedGrooveError::CacheResidentLimitExceeded { .. })
        ));
        assert_eq!(byte_limited.page_count(), 0);
        assert_eq!(byte_limited.resident_page_bytes(), 0);
    }

    #[test]
    fn producer_validates_halos_and_overlapping_seam_samples() {
        let generation = generation(37);
        let storage_halo = storage_halo_frames();
        let mut producer =
            PagedGrooveCacheProducer::new(metadata(generation), PagedGrooveCacheLimits::default())
                .unwrap();
        let insufficient_halo = page(generation, 0, SEAM_FRAME, 0, SEAM_FRAME + storage_halo - 1);
        assert!(matches!(
            producer.insert_page(insufficient_halo),
            Err(PagedGrooveError::InsufficientRightHalo { .. })
        ));
        assert_eq!(producer.page_count(), 0);

        producer
            .insert_page(page(
                generation,
                0,
                SEAM_FRAME,
                0,
                SEAM_FRAME + storage_halo,
            ))
            .unwrap();
        let mut samples: Vec<_> = (SEAM_FRAME - storage_halo..TOTAL_FRAMES)
            .map(master_sample)
            .collect();
        samples[0].vertical_displacement_m += 1.0e-6;
        let changed = PhysicalGroovePage::new(
            metadata(generation),
            GrooveFrameRange::new(SEAM_FRAME, TOTAL_FRAMES).unwrap(),
            GrooveFrameRange::new(SEAM_FRAME - storage_halo, TOTAL_FRAMES).unwrap(),
            samples
                .iter()
                .map(|sample| sample.lateral_displacement_m)
                .collect(),
            samples
                .iter()
                .map(|sample| sample.vertical_displacement_m)
                .collect(),
        )
        .unwrap();
        let resident_before = producer.resident_page_bytes();
        assert!(matches!(
            producer.insert_page(changed),
            Err(PagedGrooveError::SeamSampleMismatch { .. })
        ));
        assert_eq!(producer.page_count(), 1);
        assert_eq!(producer.resident_page_bytes(), resident_before);
    }

    #[test]
    fn producer_rejects_corrupt_page_payloads_without_changing_its_state() {
        let generation = generation(38);
        let mut producer =
            PagedGrooveCacheProducer::new(metadata(generation), PagedGrooveCacheLimits::default())
                .unwrap();
        let mut corrupt = page(generation, 0, TOTAL_FRAMES, 0, TOTAL_FRAMES);
        let original_identity = corrupt.content_identity();
        corrupt.lateral_displacement_m[100] += 1.0e-6;
        assert_eq!(corrupt.content_identity(), original_identity);

        assert!(matches!(
            producer.insert_page(corrupt),
            Err(PagedGrooveError::PageContentIdentityMismatch { page_index: 0 })
        ));
        assert_eq!(producer.page_count(), 0);
        assert_eq!(producer.resident_page_bytes(), 0);
    }

    #[test]
    fn producer_rejects_a_page_bound_to_different_content() {
        let generation = generation(39);
        let expected = metadata(generation);
        let different = PhysicalGrooveMetadata::new(
            generation,
            GrooveContentIdentity::from_sha256([0x33; 32]),
            cut_metadata(TOTAL_FRAMES),
            TOTAL_FRAMES,
            HALO_FRAMES,
        )
        .unwrap();
        let candidate = page_for_metadata(different, 0, TOTAL_FRAMES, 0, TOTAL_FRAMES);
        let mut producer =
            PagedGrooveCacheProducer::new(expected, PagedGrooveCacheLimits::default()).unwrap();

        assert!(matches!(
            producer.insert_page(candidate),
            Err(PagedGrooveError::AssetContentIdentityMismatch { page_index: 0 })
        ));
        assert_eq!(producer.page_count(), 0);
        assert_eq!(producer.resident_page_bytes(), 0);
    }

    #[test]
    fn producer_can_update_and_publish_a_bounded_immutable_snapshot() {
        let first = complete_cache();
        let first_bytes = first.resident_page_bytes();
        let mut producer = PagedGrooveCacheProducer::from_cache(&first);
        let removed = producer.remove_page_containing(10).unwrap();
        assert_eq!(removed.core_range().start_frame(), 0);
        assert_eq!(producer.page_count(), 1);
        assert!(producer.resident_page_bytes() < first_bytes);
        let second = producer.publish();
        assert_eq!(first.page_count(), 2);
        assert_eq!(second.page_count(), 1);
        assert!(matches!(
            second
                .resolve(PagedGrooveRenderRequest::new(second.generation(), 10.0, 1.0).unwrap())
                .unwrap(),
            PagedGrooveRenderResolution::Miss(PagedGrooveRenderMiss::PageUnavailable {
                frame: 10,
                ..
            })
        ));
    }

    #[test]
    fn bidirectional_prefetch_covers_the_twenty_times_reversal_window() {
        let cache = complete_cache();
        let request = cache
            .bidirectional_prefetch_request(SEAM_FRAME as f64 + 0.75, 10)
            .unwrap();
        assert_eq!(request.generation(), cache.generation());
        assert_eq!(request.core_range().start_frame(), SEAM_FRAME - 200);
        assert_eq!(request.core_range().end_frame_exclusive(), SEAM_FRAME + 202);
    }

    #[test]
    fn prefetch_plan_includes_bounded_explicit_recapture_candidates() {
        let cache = complete_cache();
        let plan = cache
            .bidirectional_prefetch_plan(SEAM_FRAME as f64 + 0.75, 10, &[100.5, 4_000.25])
            .unwrap();
        assert_eq!(plan.generation(), cache.generation());
        assert_eq!(
            plan.core_ranges(),
            &[
                GrooveFrameRange::new(0, 302).unwrap(),
                GrooveFrameRange::new(SEAM_FRAME - 200, SEAM_FRAME + 202).unwrap(),
                GrooveFrameRange::new(3_800, TOTAL_FRAMES).unwrap(),
            ]
        );

        let merged = cache
            .bidirectional_prefetch_plan(SEAM_FRAME as f64, 10, &[SEAM_FRAME as f64 + 50.0])
            .unwrap();
        assert_eq!(
            merged.core_ranges(),
            &[GrooveFrameRange::new(SEAM_FRAME - 200, SEAM_FRAME + 252).unwrap()]
        );
    }

    #[test]
    fn prefetch_plan_rejects_unbounded_or_invalid_candidates() {
        let cache = complete_cache();
        let too_many = vec![10.0; MAX_PAGED_GROOVE_PREFETCH_CANDIDATES + 1];
        assert!(matches!(
            cache.bidirectional_prefetch_plan(10.0, 1, &too_many),
            Err(PagedGrooveError::TooManyPrefetchCandidates {
                maximum_candidates: MAX_PAGED_GROOVE_PREFETCH_CANDIDATES
            })
        ));
        assert!(matches!(
            cache.bidirectional_prefetch_plan(10.0, 1, &[TOTAL_FRAMES as f64]),
            Err(PagedGrooveError::FramePositionOutsideRecord)
        ));
    }

    #[test]
    fn render_speed_limit_is_signed_and_inclusive() {
        let generation = generation(41);
        assert!(PagedGrooveRenderRequest::new(generation, 10.5, -20.0).is_ok());
        assert!(PagedGrooveRenderRequest::new(generation, 10.5, 20.0).is_ok());
        assert!(matches!(
            PagedGrooveRenderRequest::new(generation, 10.5, -20.000_001),
            Err(PagedGrooveError::UnsupportedRenderSpeed)
        ));
        assert!(matches!(
            PagedGrooveRenderRequest::new(generation, 10.5, 20.000_001),
            Err(PagedGrooveError::UnsupportedRenderSpeed)
        ));
    }

    #[test]
    fn immutable_cache_is_safe_to_borrow_on_another_thread() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PagedGrooveCache>();
        assert_send_sync::<PagedGrooveTraceFrame>();
    }

    #[test]
    fn serialized_pages_require_matching_spatial_pyramids_and_identities() {
        let asset = complete_asset();
        let value = serde_json::to_value(&asset).unwrap();
        let round_trip: PagedGrooveAsset = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(round_trip.pages().len(), 2);
        assert_eq!(
            round_trip.pages()[0].spatial_pyramid().unwrap(),
            asset.pages()[0].spatial_pyramid().unwrap()
        );

        let mut absent = value.clone();
        for page in absent["pages"].as_array_mut().unwrap() {
            page.as_object_mut().unwrap().remove("spatialPyramid");
        }
        assert!(serde_json::from_value::<PagedGrooveAsset>(absent).is_err());

        let mut changed = value;
        changed["pages"][0]["spatialPyramid"]["levels"][0]["lateralDisplacementM"][10] =
            serde_json::json!(0.5);
        assert!(serde_json::from_value::<PagedGrooveAsset>(changed).is_err());
    }

    #[test]
    fn direct_page_deserialization_requires_pyramid_and_trace_certificate() {
        let asset = complete_asset();
        let value = serde_json::to_value(&asset.pages()[0]).unwrap();
        let round_trip: PhysicalGroovePage = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(
            round_trip.content_identity(),
            asset.pages()[0].content_identity()
        );

        let mut missing_pyramid = value.clone();
        missing_pyramid
            .as_object_mut()
            .unwrap()
            .remove("spatialPyramid");
        assert!(serde_json::from_value::<PhysicalGroovePage>(missing_pyramid).is_err());

        let mut missing_certificate = value;
        missing_certificate
            .as_object_mut()
            .unwrap()
            .remove("traceAdmissionCertificate");
        assert!(serde_json::from_value::<PhysicalGroovePage>(missing_certificate).is_err());
    }
}
