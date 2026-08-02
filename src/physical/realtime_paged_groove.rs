//! Provides a fixed-capacity groove cache for one real-time thread.
//!
//! Construction allocates all page and pyramid storage. Later operations use bounded work and do not allocate or synchronize.

use serde::{Deserialize, Serialize};
use std::mem::size_of;
use thiserror::Error;

use super::groove::{
    align_up, GrooveContentHasher, GrooveContentIdentity, GROOVE_SPATIAL_DECIMATION_COEFFICIENTS,
    GROOVE_SPATIAL_FILTER_RADIUS_FRAMES, GROOVE_SPATIAL_PYRAMID_FORMAT_VERSION,
    GROOVE_SPATIAL_PYRAMID_LEVELS,
};
use super::paged_groove::{
    GrooveFrameRange, GrooveGenerationId, GrooveSample, GrooveTravelDirection, PagedGrooveError,
    PagedGroovePrefetchPlan, PagedGrooveRenderMiss, PagedGrooveRenderRequest,
    PagedTraceRepresentationManifestHasher, PhysicalGrooveMetadata, PhysicalGroovePage,
    MAX_PAGED_GROOVE_PREFETCH_CANDIDATES, MAX_PAGED_GROOVE_RENDER_SPEED,
    PAGED_GROOVE_FORMAT_VERSION,
};
use super::stylus::{
    trace_spherical_45_45_wall_multiresolution_contacts_certified_concave,
    trace_spherical_45_45_wall_multiresolution_contacts_certified_piecewise, StylusGeometry,
    StylusTraceContactSet, StylusTraceError,
};
use super::trace_admission::{
    GrooveTraceAdmissionBinding, GrooveTraceAdmissionCertificate, GrooveTraceAdmissionClass,
    GrooveTraceAdmissionError, GrooveTraceAdmissionIncrementalState, GrooveTraceAdmissionLevel,
    GrooveTraceEdgeCoverage, GrooveTraceRepresentationKind,
    ValidatedGrooveTraceAdmissionCertificate,
};

const MAX_REALTIME_PAGE_SLOTS: u32 = 64;
const MAX_REALTIME_PAGE_SLOTS_USIZE: usize = MAX_REALTIME_PAGE_SLOTS as usize;
const MAX_REALTIME_STORED_FRAMES_PER_PAGE: u32 = 4 * 1_024 * 1_024;
const MAX_REALTIME_CHUNK_FRAMES: u32 = 64 * 1_024;
const MAX_REALTIME_WORK_UNITS_PER_CALL: u32 = 16 * 1_024 * 1_024;
const MAX_REALTIME_CACHE_BYTES: u64 = 8 * 1_024 * 1_024 * 1_024;
const FILTER_WORK_UNITS_PER_OUTPUT: u32 =
    (GROOVE_SPATIAL_DECIMATION_COEFFICIENTS.len() as u32 * 2 - 1) * 2 * 2;
const HASH_WORK_UNITS_PER_SAMPLE: u32 = 1;
const SEAM_WORK_UNITS_PER_FRAME: u32 = 2;

/// Sets fixed storage and work limits for one real-time cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimePagedGrooveCacheConfig {
    pub page_slots: u32,
    pub maximum_stored_frames_per_page: u32,
    pub maximum_chunk_frames: u32,
    pub maximum_work_units_per_call: u32,
    pub maximum_resident_bytes: u64,
}

impl RealtimePagedGrooveCacheConfig {
    pub fn validate(self) -> Result<Self, RealtimePagedGrooveError> {
        if self.page_slots == 0 || self.page_slots > MAX_REALTIME_PAGE_SLOTS {
            return Err(RealtimePagedGrooveError::InvalidConfig { field: "pageSlots" });
        }
        if self.maximum_stored_frames_per_page < 4
            || self.maximum_stored_frames_per_page > MAX_REALTIME_STORED_FRAMES_PER_PAGE
        {
            return Err(RealtimePagedGrooveError::InvalidConfig {
                field: "maximumStoredFramesPerPage",
            });
        }
        if self.maximum_chunk_frames == 0
            || self.maximum_chunk_frames > self.maximum_stored_frames_per_page
            || self.maximum_chunk_frames > MAX_REALTIME_CHUNK_FRAMES
        {
            return Err(RealtimePagedGrooveError::InvalidConfig {
                field: "maximumChunkFrames",
            });
        }
        if self.maximum_work_units_per_call < FILTER_WORK_UNITS_PER_OUTPUT
            || self.maximum_work_units_per_call > MAX_REALTIME_WORK_UNITS_PER_CALL
        {
            return Err(RealtimePagedGrooveError::InvalidConfig {
                field: "maximumWorkUnitsPerCall",
            });
        }
        if self.maximum_resident_bytes == 0
            || self.maximum_resident_bytes > MAX_REALTIME_CACHE_BYTES
        {
            return Err(RealtimePagedGrooveError::InvalidConfig {
                field: "maximumResidentBytes",
            });
        }
        Ok(self)
    }
}

impl Default for RealtimePagedGrooveCacheConfig {
    fn default() -> Self {
        Self {
            page_slots: 8,
            maximum_stored_frames_per_page: 256 * 1_024,
            maximum_chunk_frames: 16 * 1_024,
            maximum_work_units_per_call: 256 * 1_024,
            maximum_resident_bytes: 128 * 1_024 * 1_024,
        }
    }
}

/// Describes the identity and ranges for one staged page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RealtimePagedGroovePageDescriptor {
    pub generation: GrooveGenerationId,
    pub asset_content_identity: GrooveContentIdentity,
    pub page_content_identity: GrooveContentIdentity,
    pub trace_admission_certificate: Option<GrooveTraceAdmissionCertificate>,
    pub core_range: GrooveFrameRange,
    pub stored_range: GrooveFrameRange,
}

impl RealtimePagedGroovePageDescriptor {
    pub fn from_page(page: &PhysicalGroovePage) -> Self {
        Self {
            generation: page.generation(),
            asset_content_identity: page.asset_content_identity(),
            page_content_identity: page.content_identity(),
            trace_admission_certificate: page.trace_admission_certificate(),
            core_range: page.core_range(),
            stored_range: page.stored_range(),
        }
    }

    pub fn stored_frame_count(self) -> u64 {
        self.stored_range.frame_count()
    }
}

/// Identifies one use of a preallocated slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealtimePagedGroovePageTicket {
    slot_index: u32,
    sequence: u64,
}

/// Identifies one fixed destination within a staged page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealtimePagedGrooveChunkTarget {
    BaseLateral,
    BaseVertical,
    SpatialLevelLateral { level_index: u8 },
    SpatialLevelVertical { level_index: u8 },
}

/// Identifies one uncommitted fixed-storage write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealtimePagedGrooveChunkReservation {
    ticket: RealtimePagedGroovePageTicket,
    sequence: u64,
    target: RealtimePagedGrooveChunkTarget,
    first_frame_offset: u32,
    frame_count: u32,
}

impl RealtimePagedGrooveChunkReservation {
    pub fn ticket(self) -> RealtimePagedGroovePageTicket {
        self.ticket
    }

    pub fn sequence(self) -> u64 {
        self.sequence
    }

    pub fn target(self) -> RealtimePagedGrooveChunkTarget {
        self.target
    }

    pub fn first_frame_offset(self) -> u32 {
        self.first_frame_offset
    }

    pub fn frame_count(self) -> u32 {
        self.frame_count
    }
}

/// Selects how the cache obtains the canonical spatial pyramid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealtimePagedGroovePyramidInput {
    BuildFromBase,
    Precomputed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TraceCertificateSource {
    RequiredExpected,
    RustOwned,
}

/// Describes one preallocated spatial level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealtimePagedGrooveLevelLayout {
    pub level_index: u8,
    pub first_source_frame: u64,
    pub source_frame_step: u32,
    pub frame_count: u32,
}

impl RealtimePagedGroovePageTicket {
    pub fn slot_index(self) -> u32 {
        self.slot_index
    }

    pub fn sequence(self) -> u64 {
        self.sequence
    }
}

/// Identifies the current page lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealtimePagedGrooveSlotPhase {
    Empty,
    Receiving,
    BuildingPyramid,
    Hashing,
    CertifyingTrace,
    ValidatingSeams,
    Ready,
    Published,
    Rejected,
}

/// Identifies a completed validation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealtimePagedGroovePageFailure {
    PageContentIdentityMismatch,
    NoncanonicalSpatialPyramid { level_index: u8, frame: u64 },
    MissingTraceAdmissionCertificate,
    TraceAdmissionCertificateMismatch,
    TraceAdmissionNotAdmitted,
    SeamSampleMismatch { frame: u64 },
    SpatialSeamSampleMismatch { level_index: u8, frame: u64 },
}

/// Reports bounded page progress without allocating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealtimePagedGroovePageProgress {
    pub slot_index: u32,
    pub sequence: u64,
    pub phase: RealtimePagedGrooveSlotPhase,
    pub received_lateral_frames: u32,
    pub received_vertical_frames: u32,
    pub stored_frames: u32,
    pub pyramid_input: RealtimePagedGroovePyramidInput,
    pub received_pyramid_channel_samples: u32,
    pub chunk_reserved: bool,
    pub completed_pyramid_levels: u8,
    pub work_units_consumed: u32,
    pub failure: Option<RealtimePagedGroovePageFailure>,
}

/// Reports fixed cache ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealtimePagedGrooveCacheStatus {
    pub page_slots: u32,
    pub published_pages: u32,
    pub staging_pages: u32,
    pub rejected_pages: u32,
    pub allocated_resident_bytes: u64,
}

/// Reports deterministic work for one page before publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealtimePagedGrooveWorkEstimate {
    pub stored_frames: u64,
    pub pyramid_output_frames: u64,
    pub precomputed_input_channel_samples: u64,
    pub raw_pyramid_work_units: u64,
    pub precomputed_pyramid_validation_work_units: u64,
    pub content_hash_work_units: u64,
    pub trace_admission_work_units: u64,
    pub maximum_seam_work_units: u64,
}

impl RealtimePagedGrooveWorkEstimate {
    pub fn raw_total_work_units(self) -> u64 {
        self.raw_pyramid_work_units
            .saturating_add(self.content_hash_work_units)
            .saturating_add(self.trace_admission_work_units)
            .saturating_add(self.maximum_seam_work_units)
    }

    pub fn precomputed_total_work_units(self) -> u64 {
        self.precomputed_pyramid_validation_work_units
            .saturating_add(self.content_hash_work_units)
            .saturating_add(self.trace_admission_work_units)
            .saturating_add(self.maximum_seam_work_units)
    }

    pub fn source_frames_per_second_for_raw_budget(self, work_units_per_second: u64) -> f64 {
        work_units_per_second as f64 * self.stored_frames as f64
            / self.raw_total_work_units().max(1) as f64
    }

    pub fn source_frames_per_second_for_precomputed_budget(
        self,
        work_units_per_second: u64,
    ) -> f64 {
        work_units_per_second as f64 * self.stored_frames as f64
            / self.precomputed_total_work_units().max(1) as f64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HashPhase {
    BaseLateral,
    BaseVertical,
    LevelLateral(usize),
    LevelVertical(usize),
    Finish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PyramidAdvance {
    Complete,
    OutputProcessed,
    Rejected,
}

struct RealtimeSpatialLevel {
    first_source_frame: u64,
    source_frame_step: u32,
    len: usize,
    lateral_displacement_m: Box<[f32]>,
    vertical_displacement_m: Box<[f32]>,
}

impl RealtimeSpatialLevel {
    fn with_capacity(
        capacity: usize,
        source_frame_step: u32,
    ) -> Result<Self, RealtimePagedGrooveError> {
        Ok(Self {
            first_source_frame: 0,
            source_frame_step,
            len: 0,
            lateral_displacement_m: allocate_zeroed_f32(capacity)?,
            vertical_displacement_m: allocate_zeroed_f32(capacity)?,
        })
    }

    fn lateral(&self) -> &[f32] {
        &self.lateral_displacement_m[..self.len]
    }

    fn vertical(&self) -> &[f32] {
        &self.vertical_displacement_m[..self.len]
    }
}

struct RealtimePageSlot {
    sequence: u64,
    phase: RealtimePagedGrooveSlotPhase,
    descriptor: Option<RealtimePagedGroovePageDescriptor>,
    pyramid_input: RealtimePagedGroovePyramidInput,
    trace_certificate_source: TraceCertificateSource,
    lateral_received: usize,
    vertical_received: usize,
    lateral_displacement_m: Box<[f32]>,
    vertical_displacement_m: Box<[f32]>,
    levels: [RealtimeSpatialLevel; GROOVE_SPATIAL_PYRAMID_LEVELS],
    level_lateral_received: [usize; GROOVE_SPATIAL_PYRAMID_LEVELS],
    level_vertical_received: [usize; GROOVE_SPATIAL_PYRAMID_LEVELS],
    pending_chunk: Option<RealtimePagedGrooveChunkReservation>,
    build_level: usize,
    build_output_index: usize,
    completed_pyramid_levels: u8,
    hash: Option<GrooveContentHasher>,
    hash_phase: HashPhase,
    hash_index: usize,
    trace_admission_state: Option<GrooveTraceAdmissionIncrementalState>,
    validated_trace_admission: Option<ValidatedGrooveTraceAdmissionCertificate>,
    seam_slot_index: usize,
    seam_level_index: usize,
    seam_next_frame: u64,
    seam_frame_initialized: bool,
    failure: Option<RealtimePagedGroovePageFailure>,
}

impl RealtimePageSlot {
    fn new(maximum_stored_frames: usize) -> Result<Self, RealtimePagedGrooveError> {
        let level_capacity = |step: usize| maximum_stored_frames.div_ceil(step);
        Ok(Self {
            sequence: 0,
            phase: RealtimePagedGrooveSlotPhase::Empty,
            descriptor: None,
            pyramid_input: RealtimePagedGroovePyramidInput::BuildFromBase,
            trace_certificate_source: TraceCertificateSource::RequiredExpected,
            lateral_received: 0,
            vertical_received: 0,
            lateral_displacement_m: allocate_zeroed_f32(maximum_stored_frames)?,
            vertical_displacement_m: allocate_zeroed_f32(maximum_stored_frames)?,
            levels: [
                RealtimeSpatialLevel::with_capacity(level_capacity(2), 2)?,
                RealtimeSpatialLevel::with_capacity(level_capacity(4), 4)?,
                RealtimeSpatialLevel::with_capacity(level_capacity(8), 8)?,
                RealtimeSpatialLevel::with_capacity(level_capacity(16), 16)?,
            ],
            level_lateral_received: [0; GROOVE_SPATIAL_PYRAMID_LEVELS],
            level_vertical_received: [0; GROOVE_SPATIAL_PYRAMID_LEVELS],
            pending_chunk: None,
            build_level: 0,
            build_output_index: 0,
            completed_pyramid_levels: 0,
            hash: None,
            hash_phase: HashPhase::BaseLateral,
            hash_index: 0,
            trace_admission_state: None,
            validated_trace_admission: None,
            seam_slot_index: 0,
            seam_level_index: 0,
            seam_next_frame: 0,
            seam_frame_initialized: false,
            failure: None,
        })
    }

    fn descriptor(&self) -> RealtimePagedGroovePageDescriptor {
        self.descriptor.expect("an active page has a descriptor")
    }

    fn stored_len(&self) -> usize {
        self.descriptor()
            .stored_frame_count()
            .try_into()
            .expect("validated page length fits usize")
    }

    fn is_reserved(&self) -> bool {
        !matches!(
            self.phase,
            RealtimePagedGrooveSlotPhase::Empty | RealtimePagedGrooveSlotPhase::Rejected
        )
    }

    fn begin(
        &mut self,
        sequence: u64,
        descriptor: RealtimePagedGroovePageDescriptor,
        pyramid_input: RealtimePagedGroovePyramidInput,
        trace_certificate_source: TraceCertificateSource,
        layouts: [(u64, usize); GROOVE_SPATIAL_PYRAMID_LEVELS],
    ) {
        self.sequence = sequence;
        self.phase = RealtimePagedGrooveSlotPhase::Receiving;
        self.descriptor = Some(descriptor);
        self.pyramid_input = pyramid_input;
        self.trace_certificate_source = trace_certificate_source;
        self.lateral_received = 0;
        self.vertical_received = 0;
        for (level, (first_source_frame, len)) in self.levels.iter_mut().zip(layouts) {
            level.first_source_frame = first_source_frame;
            level.len = len;
        }
        self.level_lateral_received = [0; GROOVE_SPATIAL_PYRAMID_LEVELS];
        self.level_vertical_received = [0; GROOVE_SPATIAL_PYRAMID_LEVELS];
        self.pending_chunk = None;
        self.build_level = 0;
        self.build_output_index = 0;
        self.completed_pyramid_levels = 0;
        self.hash = None;
        self.hash_phase = HashPhase::BaseLateral;
        self.hash_index = 0;
        self.trace_admission_state = None;
        self.validated_trace_admission = None;
        self.seam_slot_index = 0;
        self.seam_level_index = 0;
        self.seam_next_frame = 0;
        self.seam_frame_initialized = false;
        self.failure = None;
    }

    fn clear(&mut self) {
        self.phase = RealtimePagedGrooveSlotPhase::Empty;
        self.descriptor = None;
        self.trace_certificate_source = TraceCertificateSource::RequiredExpected;
        self.lateral_received = 0;
        self.vertical_received = 0;
        self.hash = None;
        self.trace_admission_state = None;
        self.validated_trace_admission = None;
        self.seam_slot_index = 0;
        self.seam_level_index = 0;
        self.seam_next_frame = 0;
        self.seam_frame_initialized = false;
        self.failure = None;
        self.pending_chunk = None;
    }

    fn progress(
        &self,
        slot_index: usize,
        work_units_consumed: u32,
    ) -> RealtimePagedGroovePageProgress {
        RealtimePagedGroovePageProgress {
            slot_index: slot_index as u32,
            sequence: self.sequence,
            phase: self.phase,
            received_lateral_frames: self.lateral_received as u32,
            received_vertical_frames: self.vertical_received as u32,
            stored_frames: self
                .descriptor
                .map_or(0, |value| value.stored_frame_count() as u32),
            pyramid_input: self.pyramid_input,
            received_pyramid_channel_samples: self
                .level_lateral_received
                .iter()
                .chain(self.level_vertical_received.iter())
                .copied()
                .sum::<usize>() as u32,
            chunk_reserved: self.pending_chunk.is_some(),
            completed_pyramid_levels: self.completed_pyramid_levels,
            work_units_consumed,
            failure: self.failure,
        }
    }
}

/// Owns preallocated staged and published page slots.
pub struct RealtimePagedGrooveCache {
    metadata: PhysicalGrooveMetadata,
    config: RealtimePagedGrooveCacheConfig,
    slots: Box<[RealtimePageSlot]>,
    next_ticket_sequence: u64,
    next_chunk_reservation_sequence: u64,
    published_pages: u32,
    manifest_slot_order: [u8; MAX_REALTIME_PAGE_SLOTS_USIZE],
    allocated_resident_bytes: u64,
    trace_admitted_representation_identity: GrooveContentIdentity,
    maximum_certified_absolute_wall_slope: f64,
}

impl std::fmt::Debug for RealtimePagedGrooveCache {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RealtimePagedGrooveCache")
            .field("metadata", &self.metadata)
            .field("config", &self.config)
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

impl RealtimePagedGrooveCache {
    /// Allocates every page and pyramid buffer used by this cache.
    pub fn new(
        metadata: PhysicalGrooveMetadata,
        config: RealtimePagedGrooveCacheConfig,
    ) -> Result<Self, RealtimePagedGrooveError> {
        metadata.validate()?;
        let config = config.validate()?;
        let allocated_resident_bytes = required_resident_bytes(config)?;
        if allocated_resident_bytes > config.maximum_resident_bytes {
            return Err(RealtimePagedGrooveError::ResidentLimitExceeded {
                required_bytes: allocated_resident_bytes,
                maximum_bytes: config.maximum_resident_bytes,
            });
        }

        let mut slots = Vec::new();
        slots
            .try_reserve_exact(config.page_slots as usize)
            .map_err(|_| RealtimePagedGrooveError::AllocationFailed)?;
        for _ in 0..config.page_slots {
            slots.push(RealtimePageSlot::new(
                config.maximum_stored_frames_per_page as usize,
            )?);
        }
        let trace_admitted_representation_identity =
            PagedTraceRepresentationManifestHasher::new(metadata, 0).finish();
        Ok(Self {
            metadata,
            config,
            slots: slots.into_boxed_slice(),
            next_ticket_sequence: 1,
            next_chunk_reservation_sequence: 1,
            published_pages: 0,
            manifest_slot_order: [0; MAX_REALTIME_PAGE_SLOTS_USIZE],
            allocated_resident_bytes,
            trace_admitted_representation_identity,
            maximum_certified_absolute_wall_slope: 0.0,
        })
    }

    pub fn metadata(&self) -> PhysicalGrooveMetadata {
        self.metadata
    }

    pub fn generation(&self) -> GrooveGenerationId {
        self.metadata.generation()
    }

    pub fn content_identity(&self) -> GrooveContentIdentity {
        self.metadata.content_identity()
    }

    pub fn trace_admitted_representation_identity(&self) -> GrooveContentIdentity {
        self.trace_admitted_representation_identity
    }

    pub(crate) fn maximum_certified_absolute_wall_slope(&self) -> f64 {
        self.maximum_certified_absolute_wall_slope
    }

    pub fn config(&self) -> RealtimePagedGrooveCacheConfig {
        self.config
    }

    pub fn status(&self) -> RealtimePagedGrooveCacheStatus {
        let mut staging_pages = 0;
        let mut rejected_pages = 0;
        for slot in &self.slots {
            match slot.phase {
                RealtimePagedGrooveSlotPhase::Empty | RealtimePagedGrooveSlotPhase::Published => {}
                RealtimePagedGrooveSlotPhase::Rejected => rejected_pages += 1,
                _ => staging_pages += 1,
            }
        }
        RealtimePagedGrooveCacheStatus {
            page_slots: self.slots.len() as u32,
            published_pages: self.published_pages,
            staging_pages,
            rejected_pages,
            allocated_resident_bytes: self.allocated_resident_bytes,
        }
    }

    pub fn slot_progress(
        &self,
        slot_index: u32,
    ) -> Result<RealtimePagedGroovePageProgress, RealtimePagedGrooveError> {
        let index = self.slot_index(slot_index)?;
        Ok(self.slots[index].progress(index, 0))
    }

    /// Calculates exact build and hash work plus a caller-selected seam bound.
    pub fn page_work_estimate(
        &self,
        descriptor: RealtimePagedGroovePageDescriptor,
        maximum_seam_frames: u64,
    ) -> Result<RealtimePagedGrooveWorkEstimate, RealtimePagedGrooveError> {
        self.validate_descriptor(descriptor)?;
        let layouts = self.page_level_layouts(descriptor)?;
        let pyramid_output_frames = layouts.iter().try_fold(0_u64, |total, (_, len)| {
            total
                .checked_add(*len as u64)
                .ok_or(RealtimePagedGrooveError::PageLayoutOverflow)
        })?;
        let stored_frames = descriptor.stored_frame_count();
        let precomputed_input_channel_samples = stored_frames
            .checked_add(pyramid_output_frames)
            .and_then(|value| value.checked_mul(2))
            .ok_or(RealtimePagedGrooveError::PageLayoutOverflow)?;
        let active_metric_levels =
            1_u64.saturating_add(layouts.iter().filter(|(_, len)| *len >= 4).count() as u64);
        let edge_sides =
            u64::from(descriptor.stored_range.start_frame() == 0).saturating_add(u64::from(
                descriptor.stored_range.end_frame_exclusive() == self.metadata.total_frame_count(),
            ));
        let metric_work_per_level = stored_frames
            .saturating_sub(1)
            .saturating_mul(2)
            .saturating_add(edge_sides.saturating_mul(2));
        let trace_admission_work_units = precomputed_input_channel_samples
            .checked_add(active_metric_levels.saturating_mul(metric_work_per_level))
            .and_then(|value| value.checked_add(1))
            .ok_or(RealtimePagedGrooveError::PageLayoutOverflow)?;
        let maximum_seam_sample_frames = [1_u64, 2, 4, 8, 16]
            .into_iter()
            .try_fold(0_u64, |total, step| {
                maximum_seam_frames
                    .checked_add(step - 1)
                    .map(|value| value / step)
                    .and_then(|level_frames| total.checked_add(level_frames))
            })
            .ok_or(RealtimePagedGrooveError::PageLayoutOverflow)?;
        Ok(RealtimePagedGrooveWorkEstimate {
            stored_frames,
            pyramid_output_frames,
            precomputed_input_channel_samples,
            raw_pyramid_work_units: pyramid_output_frames
                .checked_mul(u64::from(FILTER_WORK_UNITS_PER_OUTPUT))
                .ok_or(RealtimePagedGrooveError::PageLayoutOverflow)?,
            precomputed_pyramid_validation_work_units: pyramid_output_frames
                .checked_mul(u64::from(FILTER_WORK_UNITS_PER_OUTPUT))
                .ok_or(RealtimePagedGrooveError::PageLayoutOverflow)?,
            content_hash_work_units: precomputed_input_channel_samples,
            trace_admission_work_units,
            maximum_seam_work_units: maximum_seam_sample_frames
                .checked_mul(u64::from(SEAM_WORK_UNITS_PER_FRAME))
                .ok_or(RealtimePagedGrooveError::PageLayoutOverflow)?,
        })
    }

    /// Reserves one empty slot after all descriptor checks succeed.
    pub fn begin_page(
        &mut self,
        descriptor: RealtimePagedGroovePageDescriptor,
    ) -> Result<RealtimePagedGroovePageTicket, RealtimePagedGrooveError> {
        self.begin_page_with_pyramid_input(
            descriptor,
            RealtimePagedGroovePyramidInput::BuildFromBase,
            TraceCertificateSource::RequiredExpected,
        )
    }

    /// Reserves a slot that receives canonical precomputed spatial levels.
    pub fn begin_page_with_precomputed_pyramid(
        &mut self,
        descriptor: RealtimePagedGroovePageDescriptor,
    ) -> Result<RealtimePagedGroovePageTicket, RealtimePagedGrooveError> {
        self.begin_page_with_pyramid_input(
            descriptor,
            RealtimePagedGroovePyramidInput::Precomputed,
            TraceCertificateSource::RequiredExpected,
        )
    }

    /// Reserves raw ingress and computes its certificate inside the cache.
    pub fn begin_raw_page(
        &mut self,
        descriptor: RealtimePagedGroovePageDescriptor,
    ) -> Result<RealtimePagedGroovePageTicket, RealtimePagedGrooveError> {
        self.begin_page_with_pyramid_input(
            descriptor,
            RealtimePagedGroovePyramidInput::BuildFromBase,
            TraceCertificateSource::RustOwned,
        )
    }

    /// Reserves raw ingress with precomputed canonical spatial levels.
    pub fn begin_raw_page_with_precomputed_pyramid(
        &mut self,
        descriptor: RealtimePagedGroovePageDescriptor,
    ) -> Result<RealtimePagedGroovePageTicket, RealtimePagedGrooveError> {
        self.begin_page_with_pyramid_input(
            descriptor,
            RealtimePagedGroovePyramidInput::Precomputed,
            TraceCertificateSource::RustOwned,
        )
    }

    fn begin_page_with_pyramid_input(
        &mut self,
        descriptor: RealtimePagedGroovePageDescriptor,
        pyramid_input: RealtimePagedGroovePyramidInput,
        trace_certificate_source: TraceCertificateSource,
    ) -> Result<RealtimePagedGroovePageTicket, RealtimePagedGrooveError> {
        self.validate_descriptor(descriptor)?;
        let layouts = self.page_level_layouts(descriptor)?;
        let slot_index = self
            .slots
            .iter()
            .position(|slot| slot.phase == RealtimePagedGrooveSlotPhase::Empty)
            .ok_or(RealtimePagedGrooveError::NoEmptySlot)?;
        let sequence = self.next_ticket_sequence;
        let next_sequence = sequence
            .checked_add(1)
            .ok_or(RealtimePagedGrooveError::TicketSequenceExhausted)?;

        self.slots[slot_index].begin(
            sequence,
            descriptor,
            pyramid_input,
            trace_certificate_source,
            layouts,
        );
        self.next_ticket_sequence = next_sequence;
        Ok(RealtimePagedGroovePageTicket {
            slot_index: slot_index as u32,
            sequence,
        })
    }

    /// Returns the exact canonical layout for one staged pyramid level.
    pub fn spatial_level_layout(
        &self,
        ticket: RealtimePagedGroovePageTicket,
        level_index: u8,
    ) -> Result<RealtimePagedGrooveLevelLayout, RealtimePagedGrooveError> {
        let slot_index = self.ticket_index(ticket)?;
        let level_index = self.level_index(level_index)?;
        let level = &self.slots[slot_index].levels[level_index];
        Ok(RealtimePagedGrooveLevelLayout {
            level_index: level_index as u8,
            first_source_frame: level.first_source_frame,
            source_frame_step: level.source_frame_step,
            frame_count: level.len as u32,
        })
    }

    /// Copies one bounded, sequential lateral chunk into fixed storage.
    pub fn ingest_lateral_chunk(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
        first_stored_frame_offset: u32,
        samples: &[f32],
    ) -> Result<RealtimePagedGroovePageProgress, RealtimePagedGrooveError> {
        self.ingest_chunk(ticket, first_stored_frame_offset, samples, true)
    }

    /// Copies one bounded, sequential vertical chunk into fixed storage.
    pub fn ingest_vertical_chunk(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
        first_stored_frame_offset: u32,
        samples: &[f32],
    ) -> Result<RealtimePagedGroovePageProgress, RealtimePagedGrooveError> {
        self.ingest_chunk(ticket, first_stored_frame_offset, samples, false)
    }

    /// Copies one bounded lateral chunk for a precomputed spatial level.
    pub fn ingest_spatial_level_lateral_chunk(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
        level_index: u8,
        first_level_frame_offset: u32,
        samples: &[f32],
    ) -> Result<RealtimePagedGroovePageProgress, RealtimePagedGrooveError> {
        self.ingest_spatial_level_chunk(
            ticket,
            level_index,
            first_level_frame_offset,
            samples,
            true,
        )
    }

    /// Copies one bounded vertical chunk for a precomputed spatial level.
    pub fn ingest_spatial_level_vertical_chunk(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
        level_index: u8,
        first_level_frame_offset: u32,
        samples: &[f32],
    ) -> Result<RealtimePagedGroovePageProgress, RealtimePagedGrooveError> {
        self.ingest_spatial_level_chunk(
            ticket,
            level_index,
            first_level_frame_offset,
            samples,
            false,
        )
    }

    /// Reserves one base lateral destination without copying.
    pub fn reserve_lateral_chunk(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
        first_stored_frame_offset: u32,
        frame_count: u32,
    ) -> Result<RealtimePagedGrooveChunkReservation, RealtimePagedGrooveError> {
        self.reserve_chunk(
            ticket,
            RealtimePagedGrooveChunkTarget::BaseLateral,
            first_stored_frame_offset,
            frame_count,
        )
    }

    /// Reserves one base vertical destination without copying.
    pub fn reserve_vertical_chunk(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
        first_stored_frame_offset: u32,
        frame_count: u32,
    ) -> Result<RealtimePagedGrooveChunkReservation, RealtimePagedGrooveError> {
        self.reserve_chunk(
            ticket,
            RealtimePagedGrooveChunkTarget::BaseVertical,
            first_stored_frame_offset,
            frame_count,
        )
    }

    /// Reserves one precomputed lateral-level destination without copying.
    pub fn reserve_spatial_level_lateral_chunk(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
        level_index: u8,
        first_level_frame_offset: u32,
        frame_count: u32,
    ) -> Result<RealtimePagedGrooveChunkReservation, RealtimePagedGrooveError> {
        self.level_index(level_index)?;
        self.reserve_chunk(
            ticket,
            RealtimePagedGrooveChunkTarget::SpatialLevelLateral { level_index },
            first_level_frame_offset,
            frame_count,
        )
    }

    /// Reserves one precomputed vertical-level destination without copying.
    pub fn reserve_spatial_level_vertical_chunk(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
        level_index: u8,
        first_level_frame_offset: u32,
        frame_count: u32,
    ) -> Result<RealtimePagedGrooveChunkReservation, RealtimePagedGrooveError> {
        self.level_index(level_index)?;
        self.reserve_chunk(
            ticket,
            RealtimePagedGrooveChunkTarget::SpatialLevelVertical { level_index },
            first_level_frame_offset,
            frame_count,
        )
    }

    /// Borrows the fixed destination for one active reservation.
    pub fn reserved_chunk_mut(
        &mut self,
        reservation: RealtimePagedGrooveChunkReservation,
    ) -> Result<&mut [f32], RealtimePagedGrooveError> {
        let slot_index = self.reservation_slot_index(reservation)?;
        let start = reservation.first_frame_offset as usize;
        let end = start + reservation.frame_count as usize;
        let slot = &mut self.slots[slot_index];
        Ok(match reservation.target {
            RealtimePagedGrooveChunkTarget::BaseLateral => {
                &mut slot.lateral_displacement_m[start..end]
            }
            RealtimePagedGrooveChunkTarget::BaseVertical => {
                &mut slot.vertical_displacement_m[start..end]
            }
            RealtimePagedGrooveChunkTarget::SpatialLevelLateral { level_index } => {
                &mut slot.levels[level_index as usize].lateral_displacement_m[start..end]
            }
            RealtimePagedGrooveChunkTarget::SpatialLevelVertical { level_index } => {
                &mut slot.levels[level_index as usize].vertical_displacement_m[start..end]
            }
        })
    }

    /// Validates and commits one in-place write without copying.
    pub fn commit_reserved_chunk(
        &mut self,
        reservation: RealtimePagedGrooveChunkReservation,
    ) -> Result<RealtimePagedGroovePageProgress, RealtimePagedGrooveError> {
        let slot_index = self.reservation_slot_index(reservation)?;
        if self
            .reserved_chunk(reservation)?
            .iter()
            .any(|sample| !sample.is_finite())
        {
            return Err(RealtimePagedGrooveError::NonfiniteDisplacement);
        }
        let end = reservation.first_frame_offset as usize + reservation.frame_count as usize;
        let slot = &mut self.slots[slot_index];
        match reservation.target {
            RealtimePagedGrooveChunkTarget::BaseLateral => slot.lateral_received = end,
            RealtimePagedGrooveChunkTarget::BaseVertical => slot.vertical_received = end,
            RealtimePagedGrooveChunkTarget::SpatialLevelLateral { level_index } => {
                slot.level_lateral_received[level_index as usize] = end;
            }
            RealtimePagedGrooveChunkTarget::SpatialLevelVertical { level_index } => {
                slot.level_vertical_received[level_index as usize] = end;
            }
        }
        slot.pending_chunk = None;
        Ok(slot.progress(slot_index, 0))
    }

    /// Cancels one uncommitted write without advancing page progress.
    pub fn cancel_reserved_chunk(
        &mut self,
        reservation: RealtimePagedGrooveChunkReservation,
    ) -> Result<RealtimePagedGroovePageProgress, RealtimePagedGrooveError> {
        let slot_index = self.reservation_slot_index(reservation)?;
        self.slots[slot_index].pending_chunk = None;
        Ok(self.slots[slot_index].progress(slot_index, 0))
    }

    /// Starts bounded pyramid work after both base channels are complete.
    pub fn finish_page_ingestion(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
    ) -> Result<RealtimePagedGroovePageProgress, RealtimePagedGrooveError> {
        let index = self.ticket_index(ticket)?;
        let slot = &self.slots[index];
        if slot.phase != RealtimePagedGrooveSlotPhase::Receiving {
            return Err(RealtimePagedGrooveError::WrongPhase {
                expected: RealtimePagedGrooveSlotPhase::Receiving,
                actual: slot.phase,
            });
        }
        if slot.pending_chunk.is_some() {
            return Err(RealtimePagedGrooveError::ChunkReservationActive);
        }
        let stored_len = slot.stored_len();
        if slot.lateral_received != stored_len || slot.vertical_received != stored_len {
            return Err(RealtimePagedGrooveError::IncompleteChannels {
                expected_frames: stored_len as u32,
                lateral_frames: slot.lateral_received as u32,
                vertical_frames: slot.vertical_received as u32,
            });
        }
        if slot.pyramid_input == RealtimePagedGroovePyramidInput::Precomputed {
            for level_index in 0..GROOVE_SPATIAL_PYRAMID_LEVELS {
                let expected = slot.levels[level_index].len;
                if slot.level_lateral_received[level_index] != expected
                    || slot.level_vertical_received[level_index] != expected
                {
                    return Err(RealtimePagedGrooveError::IncompleteSpatialLevel {
                        level_index: level_index as u8,
                        expected_frames: expected as u32,
                        lateral_frames: slot.level_lateral_received[level_index] as u32,
                        vertical_frames: slot.level_vertical_received[level_index] as u32,
                    });
                }
            }
        }

        self.slots[index].build_level = 0;
        self.slots[index].build_output_index = 0;
        self.slots[index].completed_pyramid_levels = 0;
        self.slots[index].phase = RealtimePagedGrooveSlotPhase::BuildingPyramid;
        Ok(self.slots[index].progress(index, 0))
    }

    /// Advances one page by at most the specified work units.
    pub fn advance_page(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
        maximum_work_units: u32,
    ) -> Result<RealtimePagedGroovePageProgress, RealtimePagedGrooveError> {
        if maximum_work_units == 0 || maximum_work_units > self.config.maximum_work_units_per_call {
            return Err(RealtimePagedGrooveError::InvalidWorkBudget {
                requested: maximum_work_units,
                maximum: self.config.maximum_work_units_per_call,
            });
        }
        let index = self.ticket_index(ticket)?;
        let mut consumed = 0;
        loop {
            let phase = self.slots[index].phase;
            match phase {
                RealtimePagedGrooveSlotPhase::BuildingPyramid => {
                    if maximum_work_units - consumed < FILTER_WORK_UNITS_PER_OUTPUT {
                        break;
                    }
                    match self.advance_pyramid_one_output(index)? {
                        PyramidAdvance::Complete => self.start_page_validation(index),
                        PyramidAdvance::OutputProcessed => {
                            consumed += FILTER_WORK_UNITS_PER_OUTPUT;
                        }
                        PyramidAdvance::Rejected => {
                            consumed += FILTER_WORK_UNITS_PER_OUTPUT;
                            break;
                        }
                    }
                }
                RealtimePagedGrooveSlotPhase::Hashing => {
                    if maximum_work_units - consumed < HASH_WORK_UNITS_PER_SAMPLE {
                        break;
                    }
                    let used = self.advance_hash(index, maximum_work_units - consumed)?;
                    consumed += used;
                    if used == 0 && self.slots[index].phase == phase {
                        break;
                    }
                }
                RealtimePagedGrooveSlotPhase::CertifyingTrace => {
                    let used = self.advance_trace_admission(
                        index,
                        maximum_work_units.saturating_sub(consumed),
                    )?;
                    consumed += used;
                    if used == 0 && self.slots[index].phase == phase {
                        break;
                    }
                }
                RealtimePagedGrooveSlotPhase::ValidatingSeams => {
                    if maximum_work_units - consumed < SEAM_WORK_UNITS_PER_FRAME {
                        break;
                    }
                    let used =
                        self.advance_seam_validation(index, maximum_work_units - consumed)?;
                    consumed += used;
                    if used == 0 && self.slots[index].phase == phase {
                        break;
                    }
                }
                RealtimePagedGrooveSlotPhase::Receiving => {
                    return Err(RealtimePagedGrooveError::WrongPhase {
                        expected: RealtimePagedGrooveSlotPhase::BuildingPyramid,
                        actual: phase,
                    });
                }
                RealtimePagedGrooveSlotPhase::Empty => {
                    return Err(RealtimePagedGrooveError::StaleTicket);
                }
                RealtimePagedGrooveSlotPhase::Rejected => {
                    return Err(RealtimePagedGrooveError::PageRejected(
                        self.slots[index]
                            .failure
                            .expect("a rejected page has a failure"),
                    ));
                }
                RealtimePagedGrooveSlotPhase::Ready | RealtimePagedGrooveSlotPhase::Published => {
                    break
                }
            }
            if consumed >= maximum_work_units {
                break;
            }
        }
        Ok(self.slots[index].progress(index, consumed))
    }

    /// Publishes one validated slot and rebuilds at most 64 manifest entries without allocation.
    pub fn publish_page(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
    ) -> Result<RealtimePagedGroovePageDescriptor, RealtimePagedGrooveError> {
        let index = self.ticket_index(ticket)?;
        let slot = &self.slots[index];
        if slot.phase != RealtimePagedGrooveSlotPhase::Ready {
            return Err(RealtimePagedGrooveError::WrongPhase {
                expected: RealtimePagedGrooveSlotPhase::Ready,
                actual: slot.phase,
            });
        }
        let descriptor = slot.descriptor();
        self.slots[index].phase = RealtimePagedGrooveSlotPhase::Published;
        self.insert_manifest_slot(index);
        self.published_pages += 1;
        self.recompute_trace_admitted_representation_identity();
        Ok(descriptor)
    }

    /// Discards one staged or rejected page without releasing its storage.
    pub fn discard_page(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
    ) -> Result<RealtimePagedGroovePageDescriptor, RealtimePagedGrooveError> {
        let index = self.ticket_index(ticket)?;
        if self.slots[index].phase == RealtimePagedGrooveSlotPhase::Published {
            return Err(RealtimePagedGrooveError::CannotDiscardPublishedPage);
        }
        if self.slots[index].pending_chunk.is_some() {
            return Err(RealtimePagedGrooveError::ChunkReservationActive);
        }
        let descriptor = self.slots[index].descriptor();
        self.slots[index].clear();
        Ok(descriptor)
    }

    /// Evicts one published core after checking staged seam dependencies.
    pub fn evict_page_containing(
        &mut self,
        frame: u64,
    ) -> Result<Option<RealtimePagedGroovePageDescriptor>, RealtimePagedGrooveError> {
        let Some(index) = self.slots.iter().position(|slot| {
            slot.phase == RealtimePagedGrooveSlotPhase::Published
                && slot.descriptor().core_range.contains(frame)
        }) else {
            return Ok(None);
        };
        let descriptor = self.slots[index].descriptor();
        for (other_index, other) in self.slots.iter().enumerate() {
            if other_index != index
                && other.is_reserved()
                && other.phase != RealtimePagedGrooveSlotPhase::Published
                && ranges_overlap(other.descriptor().stored_range, descriptor.stored_range)
            {
                return Err(RealtimePagedGrooveError::PageHasStagingDependency {
                    slot_index: other_index as u32,
                });
            }
        }
        self.remove_manifest_slot(index);
        self.slots[index].clear();
        self.published_pages -= 1;
        self.recompute_trace_admitted_representation_identity();
        Ok(Some(descriptor))
    }

    fn recompute_trace_admitted_representation_identity(&mut self) {
        let mut manifest = PagedTraceRepresentationManifestHasher::new(
            self.metadata,
            u64::from(self.published_pages),
        );
        let mut maximum_certified_absolute_wall_slope = 0.0_f64;
        for order_index in 0..self.published_pages as usize {
            let slot_index = usize::from(self.manifest_slot_order[order_index]);
            let descriptor = self.slots[slot_index].descriptor();
            let certificate = descriptor
                .trace_admission_certificate
                .expect("a published page has a trace-admission certificate");
            maximum_certified_absolute_wall_slope = maximum_certified_absolute_wall_slope
                .max(certificate.maximum_absolute_wall_slope());
            manifest.append(
                descriptor.core_range,
                descriptor.stored_range,
                descriptor.page_content_identity,
                certificate.certificate_identity(),
            );
        }
        self.trace_admitted_representation_identity = manifest.finish();
        self.maximum_certified_absolute_wall_slope = maximum_certified_absolute_wall_slope;
    }

    fn insert_manifest_slot(&mut self, slot_index: usize) {
        let page_count = self.published_pages as usize;
        debug_assert!(page_count < self.manifest_slot_order.len());
        let descriptor = self.slots[slot_index].descriptor();
        let key = (
            descriptor.core_range.start_frame(),
            descriptor.core_range.end_frame_exclusive(),
            slot_index,
        );
        let insertion_index = (0..page_count)
            .find(|&order_index| {
                let existing_slot_index = usize::from(self.manifest_slot_order[order_index]);
                let existing = self.slots[existing_slot_index].descriptor();
                key < (
                    existing.core_range.start_frame(),
                    existing.core_range.end_frame_exclusive(),
                    existing_slot_index,
                )
            })
            .unwrap_or(page_count);
        self.manifest_slot_order
            .copy_within(insertion_index..page_count, insertion_index + 1);
        self.manifest_slot_order[insertion_index] = slot_index as u8;
    }

    fn remove_manifest_slot(&mut self, slot_index: usize) {
        let page_count = self.published_pages as usize;
        let order_index = self.manifest_slot_order[..page_count]
            .iter()
            .position(|&value| usize::from(value) == slot_index)
            .expect("a published slot is present in the manifest order");
        self.manifest_slot_order
            .copy_within(order_index + 1..page_count, order_index);
        self.manifest_slot_order[page_count - 1] = 0;
    }

    fn validate_descriptor(
        &self,
        descriptor: RealtimePagedGroovePageDescriptor,
    ) -> Result<(), RealtimePagedGrooveError> {
        if descriptor.generation != self.metadata.generation() {
            return Err(RealtimePagedGrooveError::GenerationMismatch {
                expected: self.metadata.generation(),
                actual: descriptor.generation,
            });
        }
        if descriptor.asset_content_identity != self.metadata.content_identity() {
            return Err(RealtimePagedGrooveError::AssetContentIdentityMismatch);
        }
        descriptor.page_content_identity.validate_current()?;
        GrooveFrameRange::new(
            descriptor.core_range.start_frame(),
            descriptor.core_range.end_frame_exclusive(),
        )?;
        GrooveFrameRange::new(
            descriptor.stored_range.start_frame(),
            descriptor.stored_range.end_frame_exclusive(),
        )?;
        if !contains_range(descriptor.stored_range, descriptor.core_range) {
            return Err(RealtimePagedGrooveError::StoredRangeDoesNotContainCore);
        }
        if !contains_range(self.metadata.total_range(), descriptor.stored_range) {
            return Err(RealtimePagedGrooveError::StoredRangeOutsideRecord);
        }
        if descriptor.stored_frame_count() > u64::from(self.config.maximum_stored_frames_per_page) {
            return Err(RealtimePagedGrooveError::PageCapacityExceeded {
                requested_frames: descriptor.stored_frame_count(),
                maximum_frames: self.config.maximum_stored_frames_per_page,
            });
        }

        let halo = u64::from(self.metadata.required_storage_halo_frames());
        let required_start = descriptor.core_range.start_frame().saturating_sub(halo);
        let required_end = descriptor
            .core_range
            .end_frame_exclusive()
            .saturating_add(halo)
            .min(self.metadata.total_frame_count());
        if descriptor.stored_range.start_frame() != required_start
            || descriptor.stored_range.end_frame_exclusive() != required_end
        {
            return Err(RealtimePagedGrooveError::IncorrectStorageHalo {
                required_start,
                required_end_exclusive: required_end,
            });
        }

        for (slot_index, slot) in self.slots.iter().enumerate() {
            if !slot.is_reserved() {
                continue;
            }
            let existing = slot.descriptor();
            if ranges_overlap(existing.core_range, descriptor.core_range) {
                return Err(RealtimePagedGrooveError::CoreOverlap {
                    slot_index: slot_index as u32,
                });
            }
            if slot.phase != RealtimePagedGrooveSlotPhase::Published
                && ranges_overlap(existing.stored_range, descriptor.stored_range)
            {
                return Err(RealtimePagedGrooveError::StagingRangeOverlap {
                    slot_index: slot_index as u32,
                });
            }
        }
        Ok(())
    }

    fn reserve_chunk(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
        target: RealtimePagedGrooveChunkTarget,
        first_frame_offset: u32,
        frame_count: u32,
    ) -> Result<RealtimePagedGrooveChunkReservation, RealtimePagedGrooveError> {
        let slot_index = self.ticket_index(ticket)?;
        let slot = &self.slots[slot_index];
        if slot.phase != RealtimePagedGrooveSlotPhase::Receiving {
            return Err(RealtimePagedGrooveError::WrongPhase {
                expected: RealtimePagedGrooveSlotPhase::Receiving,
                actual: slot.phase,
            });
        }
        if slot.pending_chunk.is_some() {
            return Err(RealtimePagedGrooveError::ChunkReservationActive);
        }
        if frame_count == 0 {
            return Err(RealtimePagedGrooveError::EmptyChunk);
        }
        if frame_count > self.config.maximum_chunk_frames {
            return Err(RealtimePagedGrooveError::ChunkLimitExceeded {
                requested_frames: frame_count,
                maximum_frames: self.config.maximum_chunk_frames,
            });
        }
        let (received, capacity) = match target {
            RealtimePagedGrooveChunkTarget::BaseLateral => {
                (slot.lateral_received, slot.stored_len())
            }
            RealtimePagedGrooveChunkTarget::BaseVertical => {
                (slot.vertical_received, slot.stored_len())
            }
            RealtimePagedGrooveChunkTarget::SpatialLevelLateral { level_index } => {
                if slot.pyramid_input != RealtimePagedGroovePyramidInput::Precomputed {
                    return Err(RealtimePagedGrooveError::PrecomputedPyramidNotRequested);
                }
                let level_index = self.level_index(level_index)?;
                (
                    slot.level_lateral_received[level_index],
                    slot.levels[level_index].len,
                )
            }
            RealtimePagedGrooveChunkTarget::SpatialLevelVertical { level_index } => {
                if slot.pyramid_input != RealtimePagedGroovePyramidInput::Precomputed {
                    return Err(RealtimePagedGrooveError::PrecomputedPyramidNotRequested);
                }
                let level_index = self.level_index(level_index)?;
                (
                    slot.level_vertical_received[level_index],
                    slot.levels[level_index].len,
                )
            }
        };
        if first_frame_offset as usize != received {
            return Err(RealtimePagedGrooveError::NonsequentialChunk {
                expected_offset: received as u32,
                actual_offset: first_frame_offset,
            });
        }
        let end = received
            .checked_add(frame_count as usize)
            .ok_or(RealtimePagedGrooveError::ChunkRangeOverflow)?;
        if end > capacity {
            return Err(RealtimePagedGrooveError::ChunkOutsidePage);
        }
        let sequence = self.next_chunk_reservation_sequence;
        let next_sequence = sequence
            .checked_add(1)
            .ok_or(RealtimePagedGrooveError::ChunkReservationSequenceExhausted)?;
        let reservation = RealtimePagedGrooveChunkReservation {
            ticket,
            sequence,
            target,
            first_frame_offset,
            frame_count,
        };
        self.slots[slot_index].pending_chunk = Some(reservation);
        self.next_chunk_reservation_sequence = next_sequence;
        Ok(reservation)
    }

    fn reservation_slot_index(
        &self,
        reservation: RealtimePagedGrooveChunkReservation,
    ) -> Result<usize, RealtimePagedGrooveError> {
        let slot_index = self.ticket_index(reservation.ticket)?;
        if self.slots[slot_index].pending_chunk != Some(reservation) {
            return Err(RealtimePagedGrooveError::StaleChunkReservation);
        }
        Ok(slot_index)
    }

    fn reserved_chunk(
        &self,
        reservation: RealtimePagedGrooveChunkReservation,
    ) -> Result<&[f32], RealtimePagedGrooveError> {
        let slot_index = self.reservation_slot_index(reservation)?;
        let start = reservation.first_frame_offset as usize;
        let end = start + reservation.frame_count as usize;
        let slot = &self.slots[slot_index];
        Ok(match reservation.target {
            RealtimePagedGrooveChunkTarget::BaseLateral => &slot.lateral_displacement_m[start..end],
            RealtimePagedGrooveChunkTarget::BaseVertical => {
                &slot.vertical_displacement_m[start..end]
            }
            RealtimePagedGrooveChunkTarget::SpatialLevelLateral { level_index } => {
                &slot.levels[level_index as usize].lateral_displacement_m[start..end]
            }
            RealtimePagedGrooveChunkTarget::SpatialLevelVertical { level_index } => {
                &slot.levels[level_index as usize].vertical_displacement_m[start..end]
            }
        })
    }

    fn ingest_chunk(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
        first_stored_frame_offset: u32,
        samples: &[f32],
        lateral: bool,
    ) -> Result<RealtimePagedGroovePageProgress, RealtimePagedGrooveError> {
        let index = self.ticket_index(ticket)?;
        let slot = &self.slots[index];
        if slot.phase != RealtimePagedGrooveSlotPhase::Receiving {
            return Err(RealtimePagedGrooveError::WrongPhase {
                expected: RealtimePagedGrooveSlotPhase::Receiving,
                actual: slot.phase,
            });
        }
        if slot.pending_chunk.is_some() {
            return Err(RealtimePagedGrooveError::ChunkReservationActive);
        }
        if samples.is_empty() {
            return Err(RealtimePagedGrooveError::EmptyChunk);
        }
        if samples.len() > self.config.maximum_chunk_frames as usize {
            return Err(RealtimePagedGrooveError::ChunkLimitExceeded {
                requested_frames: samples.len() as u32,
                maximum_frames: self.config.maximum_chunk_frames,
            });
        }
        let received = if lateral {
            slot.lateral_received
        } else {
            slot.vertical_received
        };
        if first_stored_frame_offset as usize != received {
            return Err(RealtimePagedGrooveError::NonsequentialChunk {
                expected_offset: received as u32,
                actual_offset: first_stored_frame_offset,
            });
        }
        let end = received
            .checked_add(samples.len())
            .ok_or(RealtimePagedGrooveError::ChunkRangeOverflow)?;
        if end > slot.stored_len() {
            return Err(RealtimePagedGrooveError::ChunkOutsidePage);
        }
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(RealtimePagedGrooveError::NonfiniteDisplacement);
        }

        let slot = &mut self.slots[index];
        if lateral {
            slot.lateral_displacement_m[received..end].copy_from_slice(samples);
            slot.lateral_received = end;
        } else {
            slot.vertical_displacement_m[received..end].copy_from_slice(samples);
            slot.vertical_received = end;
        }
        Ok(slot.progress(index, 0))
    }

    fn ingest_spatial_level_chunk(
        &mut self,
        ticket: RealtimePagedGroovePageTicket,
        level_index: u8,
        first_level_frame_offset: u32,
        samples: &[f32],
        lateral: bool,
    ) -> Result<RealtimePagedGroovePageProgress, RealtimePagedGrooveError> {
        let index = self.ticket_index(ticket)?;
        let level_index = self.level_index(level_index)?;
        let slot = &self.slots[index];
        if slot.phase != RealtimePagedGrooveSlotPhase::Receiving {
            return Err(RealtimePagedGrooveError::WrongPhase {
                expected: RealtimePagedGrooveSlotPhase::Receiving,
                actual: slot.phase,
            });
        }
        if slot.pending_chunk.is_some() {
            return Err(RealtimePagedGrooveError::ChunkReservationActive);
        }
        if slot.pyramid_input != RealtimePagedGroovePyramidInput::Precomputed {
            return Err(RealtimePagedGrooveError::PrecomputedPyramidNotRequested);
        }
        if samples.is_empty() {
            return Err(RealtimePagedGrooveError::EmptyChunk);
        }
        if samples.len() > self.config.maximum_chunk_frames as usize {
            return Err(RealtimePagedGrooveError::ChunkLimitExceeded {
                requested_frames: samples.len() as u32,
                maximum_frames: self.config.maximum_chunk_frames,
            });
        }
        let received = if lateral {
            slot.level_lateral_received[level_index]
        } else {
            slot.level_vertical_received[level_index]
        };
        if first_level_frame_offset as usize != received {
            return Err(RealtimePagedGrooveError::NonsequentialChunk {
                expected_offset: received as u32,
                actual_offset: first_level_frame_offset,
            });
        }
        let end = received
            .checked_add(samples.len())
            .ok_or(RealtimePagedGrooveError::ChunkRangeOverflow)?;
        if end > slot.levels[level_index].len {
            return Err(RealtimePagedGrooveError::ChunkOutsidePage);
        }
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(RealtimePagedGrooveError::NonfiniteDisplacement);
        }

        let slot = &mut self.slots[index];
        if lateral {
            slot.levels[level_index].lateral_displacement_m[received..end].copy_from_slice(samples);
            slot.level_lateral_received[level_index] = end;
        } else {
            slot.levels[level_index].vertical_displacement_m[received..end]
                .copy_from_slice(samples);
            slot.level_vertical_received[level_index] = end;
        }
        Ok(slot.progress(index, 0))
    }

    fn page_level_layouts(
        &self,
        descriptor: RealtimePagedGroovePageDescriptor,
    ) -> Result<[(u64, usize); GROOVE_SPATIAL_PYRAMID_LEVELS], RealtimePagedGrooveError> {
        let mut input_first = descriptor.stored_range.start_frame();
        let mut input_step = 1_u32;
        let mut input_len = usize::try_from(descriptor.stored_frame_count())
            .map_err(|_| RealtimePagedGrooveError::PageLayoutOverflow)?;
        let mut layouts = [(0_u64, 0_usize); GROOVE_SPATIAL_PYRAMID_LEVELS];
        for (level_index, layout) in layouts.iter_mut().enumerate() {
            let next_step = input_step * 2;
            let next_first = align_up(input_first, u64::from(next_step))
                .ok_or(RealtimePagedGrooveError::PageLayoutOverflow)?;
            let next_len = if input_len == 0 {
                0
            } else {
                let span = u64::try_from(input_len - 1)
                    .map_err(|_| RealtimePagedGrooveError::PageLayoutOverflow)?
                    .checked_mul(u64::from(input_step))
                    .ok_or(RealtimePagedGrooveError::PageLayoutOverflow)?;
                let input_last = input_first
                    .checked_add(span)
                    .ok_or(RealtimePagedGrooveError::PageLayoutOverflow)?;
                if next_first > input_last {
                    0
                } else {
                    usize::try_from((input_last - next_first) / u64::from(next_step) + 1)
                        .map_err(|_| RealtimePagedGrooveError::PageLayoutOverflow)?
                }
            };
            if next_len
                > self.slots[0].levels[level_index]
                    .lateral_displacement_m
                    .len()
            {
                return Err(RealtimePagedGrooveError::PageLayoutOverflow);
            }
            *layout = (next_first, next_len);
            input_first = next_first;
            input_step = next_step;
            input_len = next_len;
        }
        Ok(layouts)
    }

    fn advance_pyramid_one_output(
        &mut self,
        slot_index: usize,
    ) -> Result<PyramidAdvance, RealtimePagedGrooveError> {
        let slot = &mut self.slots[slot_index];
        while slot.build_level < GROOVE_SPATIAL_PYRAMID_LEVELS
            && slot.build_output_index >= slot.levels[slot.build_level].len
        {
            slot.build_level += 1;
            slot.build_output_index = 0;
            slot.completed_pyramid_levels = slot.build_level as u8;
        }
        if slot.build_level >= GROOVE_SPATIAL_PYRAMID_LEVELS {
            return Ok(PyramidAdvance::Complete);
        }

        let level_index = slot.build_level;
        let output_index = slot.build_output_index;
        let (input_first, input_step, next_first) = if level_index == 0 {
            (
                slot.descriptor().stored_range.start_frame(),
                1_u32,
                slot.levels[0].first_source_frame,
            )
        } else {
            (
                slot.levels[level_index - 1].first_source_frame,
                slot.levels[level_index - 1].source_frame_step,
                slot.levels[level_index].first_source_frame,
            )
        };
        let center_offset = usize::try_from((next_first - input_first) / u64::from(input_step))
            .map_err(|_| RealtimePagedGrooveError::PageLayoutOverflow)?;
        let center = center_offset + output_index * 2;

        let (lateral, vertical) = if level_index == 0 {
            let input_len = slot.stored_len();
            (
                filter_sample(&slot.lateral_displacement_m[..input_len], center),
                filter_sample(&slot.vertical_displacement_m[..input_len], center),
            )
        } else {
            let input = &slot.levels[level_index - 1];
            (
                filter_sample(input.lateral(), center),
                filter_sample(input.vertical(), center),
            )
        };
        if slot.pyramid_input == RealtimePagedGroovePyramidInput::Precomputed {
            let actual_lateral = slot.levels[level_index].lateral_displacement_m[output_index];
            let actual_vertical = slot.levels[level_index].vertical_displacement_m[output_index];
            if actual_lateral.to_bits() != lateral.to_bits()
                || actual_vertical.to_bits() != vertical.to_bits()
            {
                let frame = slot.levels[level_index].first_source_frame.saturating_add(
                    u64::try_from(output_index)
                        .unwrap_or(u64::MAX)
                        .saturating_mul(u64::from(slot.levels[level_index].source_frame_step)),
                );
                slot.phase = RealtimePagedGrooveSlotPhase::Rejected;
                slot.failure = Some(RealtimePagedGroovePageFailure::NoncanonicalSpatialPyramid {
                    level_index: level_index as u8,
                    frame,
                });
                return Ok(PyramidAdvance::Rejected);
            }
        } else {
            slot.levels[level_index].lateral_displacement_m[output_index] = lateral;
            slot.levels[level_index].vertical_displacement_m[output_index] = vertical;
        }
        slot.build_output_index += 1;
        Ok(PyramidAdvance::OutputProcessed)
    }

    fn start_hash(&mut self, slot_index: usize) {
        let slot = &mut self.slots[slot_index];
        let descriptor = slot.descriptor();
        let mut hash = GrooveContentHasher::new(b"record-player-paged-groove-page-v4\0");
        hash.u64(descriptor.generation.get());
        hash.identity(descriptor.asset_content_identity);
        match descriptor.trace_admission_certificate {
            Some(certificate) => {
                hash.u8(1);
                hash.identity(certificate.certificate_identity());
            }
            None => hash.u8(0),
        }
        hash.u64(descriptor.core_range.start_frame());
        hash.u64(descriptor.core_range.end_frame_exclusive());
        hash.u64(descriptor.stored_range.start_frame());
        hash.u64(descriptor.stored_range.end_frame_exclusive());
        hash.u64(slot.stored_len() as u64);
        slot.hash = Some(hash);
        slot.hash_phase = HashPhase::BaseLateral;
        slot.hash_index = 0;
        slot.phase = RealtimePagedGrooveSlotPhase::Hashing;
    }

    fn start_page_validation(&mut self, slot_index: usize) {
        if self.slots[slot_index].trace_certificate_source == TraceCertificateSource::RustOwned {
            if self.start_trace_admission(slot_index).is_err() {
                self.reject(
                    slot_index,
                    RealtimePagedGroovePageFailure::TraceAdmissionCertificateMismatch,
                );
            }
        } else {
            self.start_hash(slot_index);
        }
    }

    fn advance_hash(
        &mut self,
        slot_index: usize,
        available_work_units: u32,
    ) -> Result<u32, RealtimePagedGrooveError> {
        let mut used = 0;
        loop {
            if used >= available_work_units {
                return Ok(used);
            }
            let phase = self.slots[slot_index].hash_phase;
            match phase {
                HashPhase::BaseLateral => {
                    let len = self.slots[slot_index].stored_len();
                    if self.slots[slot_index].hash_index >= len {
                        self.slots[slot_index]
                            .hash
                            .as_mut()
                            .expect("hashing has a state")
                            .u64(len as u64);
                        self.slots[slot_index].hash_phase = HashPhase::BaseVertical;
                        self.slots[slot_index].hash_index = 0;
                        continue;
                    }
                    let index = self.slots[slot_index].hash_index;
                    let sample = self.slots[slot_index].lateral_displacement_m[index];
                    self.slots[slot_index]
                        .hash
                        .as_mut()
                        .expect("hashing has a state")
                        .f32(sample);
                    self.slots[slot_index].hash_index += 1;
                    used += HASH_WORK_UNITS_PER_SAMPLE;
                }
                HashPhase::BaseVertical => {
                    let len = self.slots[slot_index].stored_len();
                    if self.slots[slot_index].hash_index >= len {
                        let hash = self.slots[slot_index]
                            .hash
                            .as_mut()
                            .expect("hashing has a state");
                        hash.u8(1);
                        hash.u32(GROOVE_SPATIAL_PYRAMID_FORMAT_VERSION);
                        hash.u64(GROOVE_SPATIAL_PYRAMID_LEVELS as u64);
                        append_level_header(hash, &self.slots[slot_index].levels[0]);
                        self.slots[slot_index].hash_phase = HashPhase::LevelLateral(0);
                        self.slots[slot_index].hash_index = 0;
                        continue;
                    }
                    let index = self.slots[slot_index].hash_index;
                    let sample = self.slots[slot_index].vertical_displacement_m[index];
                    self.slots[slot_index]
                        .hash
                        .as_mut()
                        .expect("hashing has a state")
                        .f32(sample);
                    self.slots[slot_index].hash_index += 1;
                    used += HASH_WORK_UNITS_PER_SAMPLE;
                }
                HashPhase::LevelLateral(level_index) => {
                    let len = self.slots[slot_index].levels[level_index].len;
                    if self.slots[slot_index].hash_index >= len {
                        self.slots[slot_index]
                            .hash
                            .as_mut()
                            .expect("hashing has a state")
                            .u64(len as u64);
                        self.slots[slot_index].hash_phase = HashPhase::LevelVertical(level_index);
                        self.slots[slot_index].hash_index = 0;
                        continue;
                    }
                    let index = self.slots[slot_index].hash_index;
                    let sample =
                        self.slots[slot_index].levels[level_index].lateral_displacement_m[index];
                    self.slots[slot_index]
                        .hash
                        .as_mut()
                        .expect("hashing has a state")
                        .f32(sample);
                    self.slots[slot_index].hash_index += 1;
                    used += HASH_WORK_UNITS_PER_SAMPLE;
                }
                HashPhase::LevelVertical(level_index) => {
                    let len = self.slots[slot_index].levels[level_index].len;
                    if self.slots[slot_index].hash_index >= len {
                        let next_level = level_index + 1;
                        if next_level < GROOVE_SPATIAL_PYRAMID_LEVELS {
                            let level = &self.slots[slot_index].levels[next_level];
                            let hash = self.slots[slot_index]
                                .hash
                                .as_mut()
                                .expect("hashing has a state");
                            append_level_header(hash, level);
                            self.slots[slot_index].hash_phase = HashPhase::LevelLateral(next_level);
                        } else {
                            self.slots[slot_index].hash_phase = HashPhase::Finish;
                        }
                        self.slots[slot_index].hash_index = 0;
                        continue;
                    }
                    let index = self.slots[slot_index].hash_index;
                    let sample =
                        self.slots[slot_index].levels[level_index].vertical_displacement_m[index];
                    self.slots[slot_index]
                        .hash
                        .as_mut()
                        .expect("hashing has a state")
                        .f32(sample);
                    self.slots[slot_index].hash_index += 1;
                    used += HASH_WORK_UNITS_PER_SAMPLE;
                }
                HashPhase::Finish => {
                    let actual = self.slots[slot_index]
                        .hash
                        .take()
                        .expect("hashing has a state")
                        .finish();
                    if actual != self.slots[slot_index].descriptor().page_content_identity {
                        self.reject(
                            slot_index,
                            RealtimePagedGroovePageFailure::PageContentIdentityMismatch,
                        );
                    } else if self.slots[slot_index].trace_certificate_source
                        == TraceCertificateSource::RustOwned
                    {
                        if self.slots[slot_index].validated_trace_admission.is_none()
                            || self.slots[slot_index]
                                .descriptor()
                                .trace_admission_certificate
                                .is_none()
                        {
                            self.reject(
                                slot_index,
                                RealtimePagedGroovePageFailure::TraceAdmissionCertificateMismatch,
                            );
                        } else {
                            self.start_seam_validation(slot_index);
                        }
                    } else if self.slots[slot_index]
                        .descriptor()
                        .trace_admission_certificate
                        .is_some()
                    {
                        if self.start_trace_admission(slot_index).is_err() {
                            self.reject(
                                slot_index,
                                RealtimePagedGroovePageFailure::TraceAdmissionCertificateMismatch,
                            );
                        }
                    } else {
                        self.reject(
                            slot_index,
                            RealtimePagedGroovePageFailure::MissingTraceAdmissionCertificate,
                        );
                    }
                    return Ok(used);
                }
            }
        }
    }

    fn trace_admission_binding(&self, slot_index: usize) -> GrooveTraceAdmissionBinding {
        let descriptor = self.slots[slot_index].descriptor();
        let final_frame = self.metadata.total_frame_count().saturating_sub(1) as f64;
        GrooveTraceAdmissionBinding {
            representation_kind: GrooveTraceRepresentationKind::Paged,
            representation_format_version: PAGED_GROOVE_FORMAT_VERSION,
            source_content_identity: self.metadata.source_content_identity(),
            generation: descriptor.generation.get(),
            core_start_frame: descriptor.core_range.start_frame(),
            core_end_frame_exclusive: descriptor.core_range.end_frame_exclusive(),
            stored_start_frame: descriptor.stored_range.start_frame(),
            stored_end_frame_exclusive: descriptor.stored_range.end_frame_exclusive(),
            record_end_frame_exclusive: self.metadata.total_frame_count(),
            minimum_meters_per_source_frame: self.metadata.cut().layout().meters_per_frame_at(
                final_frame,
                self.metadata.cut().cut().groove_pitch_m_per_revolution,
            ),
            maximum_geometry: self.metadata.trace_admission_policy().maximum_geometry(),
            edge_coverage: GrooveTraceEdgeCoverage::paged(
                descriptor.stored_range.start_frame() == 0,
                descriptor.stored_range.end_frame_exclusive() == self.metadata.total_frame_count(),
            ),
        }
    }

    fn start_trace_admission(&mut self, slot_index: usize) -> Result<(), RealtimePagedGrooveError> {
        let binding = self.trace_admission_binding(slot_index);
        let state = {
            let slot = &self.slots[slot_index];
            let spatial_levels = slot_trace_admission_levels(slot);
            GrooveTraceAdmissionIncrementalState::new(
                binding,
                slot_base_trace_admission_level(slot),
                &spatial_levels,
            )?
        };
        self.slots[slot_index].trace_admission_state = Some(state);
        self.slots[slot_index].phase = RealtimePagedGrooveSlotPhase::CertifyingTrace;
        Ok(())
    }

    fn advance_trace_admission(
        &mut self,
        slot_index: usize,
        maximum_work_units: u32,
    ) -> Result<u32, RealtimePagedGrooveError> {
        let mut state = self.slots[slot_index]
            .trace_admission_state
            .take()
            .ok_or(RealtimePagedGrooveError::InternalPageMap)?;
        let progress = {
            let slot = &self.slots[slot_index];
            let spatial_levels = slot_trace_admission_levels(slot);
            state.advance(
                slot_base_trace_admission_level(slot),
                &spatial_levels,
                maximum_work_units,
            )
        };
        match progress {
            Ok(progress) => {
                if let Some(recomputed) = progress.certificate {
                    let expected = self.slots[slot_index]
                        .descriptor()
                        .trace_admission_certificate;
                    let certificate = expected.unwrap_or(recomputed);
                    match certificate.validate_recomputed(recomputed) {
                        Ok(validated) => {
                            if validated
                                .validate_for_active_tracing(
                                    self.metadata.trace_admission_policy().maximum_geometry(),
                                )
                                .is_err()
                            {
                                self.reject(
                                    slot_index,
                                    RealtimePagedGroovePageFailure::TraceAdmissionNotAdmitted,
                                );
                            } else {
                                if expected.is_none() {
                                    self.slots[slot_index]
                                        .descriptor
                                        .as_mut()
                                        .expect("an active page has a descriptor")
                                        .trace_admission_certificate = Some(recomputed);
                                }
                                self.slots[slot_index].validated_trace_admission = Some(validated);
                                if self.slots[slot_index].trace_certificate_source
                                    == TraceCertificateSource::RustOwned
                                {
                                    self.start_hash(slot_index);
                                } else {
                                    self.start_seam_validation(slot_index);
                                }
                            }
                        }
                        Err(_) => self.reject(
                            slot_index,
                            RealtimePagedGroovePageFailure::TraceAdmissionCertificateMismatch,
                        ),
                    }
                } else {
                    self.slots[slot_index].trace_admission_state = Some(state);
                }
                Ok(progress.work_units_consumed)
            }
            Err(_) => {
                self.reject(
                    slot_index,
                    RealtimePagedGroovePageFailure::TraceAdmissionCertificateMismatch,
                );
                Ok(0)
            }
        }
    }

    fn start_seam_validation(&mut self, slot_index: usize) {
        self.slots[slot_index].phase = RealtimePagedGrooveSlotPhase::ValidatingSeams;
        self.slots[slot_index].seam_slot_index = 0;
        self.slots[slot_index].seam_level_index = 0;
        self.slots[slot_index].seam_next_frame = 0;
        self.slots[slot_index].seam_frame_initialized = false;
    }

    fn advance_seam_validation(
        &mut self,
        slot_index: usize,
        available_work_units: u32,
    ) -> Result<u32, RealtimePagedGrooveError> {
        let mut used = 0;
        loop {
            let other_index = self.slots[slot_index].seam_slot_index;
            if other_index >= self.slots.len() {
                self.slots[slot_index].phase = RealtimePagedGrooveSlotPhase::Ready;
                return Ok(used);
            }
            if other_index == slot_index
                || self.slots[other_index].phase != RealtimePagedGrooveSlotPhase::Published
            {
                self.advance_to_next_seam_slot(slot_index);
                continue;
            }

            let staged_range = self.slots[slot_index].descriptor().stored_range;
            let published_range = self.slots[other_index].descriptor().stored_range;
            let overlap_start = staged_range
                .start_frame()
                .max(published_range.start_frame());
            let overlap_end = staged_range
                .end_frame_exclusive()
                .min(published_range.end_frame_exclusive());
            if overlap_start >= overlap_end {
                self.advance_to_next_seam_slot(slot_index);
                continue;
            }

            let level_index = self.slots[slot_index].seam_level_index;
            if level_index > GROOVE_SPATIAL_PYRAMID_LEVELS {
                self.advance_to_next_seam_slot(slot_index);
                continue;
            }
            let (validation_start, validation_end, step) = if level_index == 0 {
                (overlap_start, overlap_end, 1_u64)
            } else {
                let spatial_start =
                    overlap_start.saturating_add(u64::from(GROOVE_SPATIAL_FILTER_RADIUS_FRAMES));
                let spatial_end =
                    overlap_end.saturating_sub(u64::from(GROOVE_SPATIAL_FILTER_RADIUS_FRAMES));
                let staged_level = &self.slots[slot_index].levels[level_index - 1];
                let published_level = &self.slots[other_index].levels[level_index - 1];
                let step = u64::from(staged_level.source_frame_step);
                if step == 0 || published_level.source_frame_step != staged_level.source_frame_step
                {
                    return Err(RealtimePagedGrooveError::InternalPageMap);
                }
                (spatial_start, spatial_end, step)
            };

            if validation_start >= validation_end {
                self.advance_to_next_seam_level(slot_index);
                continue;
            }
            if !self.slots[slot_index].seam_frame_initialized {
                self.slots[slot_index].seam_next_frame = if level_index == 0 {
                    validation_start
                } else {
                    align_up(validation_start, step)
                        .ok_or(RealtimePagedGrooveError::InternalPageMap)?
                };
                self.slots[slot_index].seam_frame_initialized = true;
            }
            let frame = self.slots[slot_index].seam_next_frame;
            if frame >= validation_end {
                self.advance_to_next_seam_level(slot_index);
                continue;
            }
            if available_work_units - used < SEAM_WORK_UNITS_PER_FRAME {
                return Ok(used);
            }
            let (staged, published) = if level_index == 0 {
                (
                    sample_from_slot(&self.slots[slot_index], frame),
                    sample_from_slot(&self.slots[other_index], frame),
                )
            } else {
                (
                    spatial_sample_from_slot(&self.slots[slot_index], level_index - 1, frame),
                    spatial_sample_from_slot(&self.slots[other_index], level_index - 1, frame),
                )
            };
            let staged = staged.ok_or(RealtimePagedGrooveError::InternalPageMap)?;
            let published = published.ok_or(RealtimePagedGrooveError::InternalPageMap)?;
            used += SEAM_WORK_UNITS_PER_FRAME;
            if staged.lateral_displacement_m.to_bits() != published.lateral_displacement_m.to_bits()
                || staged.vertical_displacement_m.to_bits()
                    != published.vertical_displacement_m.to_bits()
            {
                let failure = if level_index == 0 {
                    RealtimePagedGroovePageFailure::SeamSampleMismatch { frame }
                } else {
                    RealtimePagedGroovePageFailure::SpatialSeamSampleMismatch {
                        level_index: (level_index - 1) as u8,
                        frame,
                    }
                };
                self.reject(slot_index, failure);
                return Ok(used);
            }
            self.slots[slot_index].seam_next_frame = frame
                .checked_add(step)
                .ok_or(RealtimePagedGrooveError::InternalPageMap)?;
        }
    }

    fn advance_to_next_seam_level(&mut self, slot_index: usize) {
        self.slots[slot_index].seam_level_index += 1;
        self.slots[slot_index].seam_next_frame = 0;
        self.slots[slot_index].seam_frame_initialized = false;
    }

    fn advance_to_next_seam_slot(&mut self, slot_index: usize) {
        self.slots[slot_index].seam_slot_index += 1;
        self.slots[slot_index].seam_level_index = 0;
        self.slots[slot_index].seam_next_frame = 0;
        self.slots[slot_index].seam_frame_initialized = false;
    }

    fn reject(&mut self, slot_index: usize, failure: RealtimePagedGroovePageFailure) {
        let slot = &mut self.slots[slot_index];
        slot.phase = RealtimePagedGrooveSlotPhase::Rejected;
        slot.failure = Some(failure);
        slot.hash = None;
    }

    fn ticket_index(
        &self,
        ticket: RealtimePagedGroovePageTicket,
    ) -> Result<usize, RealtimePagedGrooveError> {
        let index = self.slot_index(ticket.slot_index)?;
        let slot = &self.slots[index];
        if slot.sequence != ticket.sequence
            || slot.phase == RealtimePagedGrooveSlotPhase::Empty
            || slot.descriptor.is_none()
        {
            return Err(RealtimePagedGrooveError::StaleTicket);
        }
        Ok(index)
    }

    fn slot_index(&self, slot_index: u32) -> Result<usize, RealtimePagedGrooveError> {
        let index = slot_index as usize;
        if index >= self.slots.len() {
            return Err(RealtimePagedGrooveError::InvalidSlot {
                slot_index,
                page_slots: self.slots.len() as u32,
            });
        }
        Ok(index)
    }

    fn level_index(&self, level_index: u8) -> Result<usize, RealtimePagedGrooveError> {
        let index = level_index as usize;
        if index >= GROOVE_SPATIAL_PYRAMID_LEVELS {
            return Err(RealtimePagedGrooveError::InvalidSpatialLevel {
                level_index,
                level_count: GROOVE_SPATIAL_PYRAMID_LEVELS as u8,
            });
        }
        Ok(index)
    }

    /// Resolves one published page without allocation or synchronization.
    pub fn resolve(
        &self,
        request: PagedGrooveRenderRequest,
    ) -> Result<RealtimePagedGrooveRenderResolution<'_>, RealtimePagedGrooveError> {
        let request = PagedGrooveRenderRequest::new(
            request.generation(),
            request.absolute_frame_position(),
            request.source_frame_advance(),
        )?;
        if request.generation() != self.metadata.generation() {
            return Ok(RealtimePagedGrooveRenderResolution::Miss(
                PagedGrooveRenderMiss::StaleGeneration {
                    requested_generation: request.generation(),
                    cached_generation: self.metadata.generation(),
                },
            ));
        }
        if request.absolute_frame_position() >= self.metadata.total_frame_count() as f64 {
            return Err(RealtimePagedGrooveError::FramePositionOutsideRecord);
        }
        let frame = request.absolute_frame_position().floor() as u64;
        let Some(slot) = self.slots.iter().find(|slot| {
            slot.phase == RealtimePagedGrooveSlotPhase::Published
                && slot.descriptor().core_range.contains(frame)
        }) else {
            return Ok(RealtimePagedGrooveRenderResolution::Miss(
                PagedGrooveRenderMiss::PageUnavailable {
                    generation: request.generation(),
                    frame,
                },
            ));
        };
        let selection = select_levels(slot, request.source_frame_advance())?;
        Ok(RealtimePagedGrooveRenderResolution::Ready(
            RealtimePagedGrooveTraceView {
                core_range: slot.descriptor().core_range,
                stored_range: slot.descriptor().stored_range,
                absolute_frame_position: request.absolute_frame_position(),
                direction: request.direction(),
                selection,
                available_tracing_halo_frames: self.metadata.tracing_halo_frames(),
                validated_trace_admission: slot
                    .validated_trace_admission
                    .ok_or(RealtimePagedGrooveError::InternalPageMap)?,
            },
        ))
    }

    /// Traces both walls without allocation or synchronization.
    pub fn trace(
        &self,
        request: PagedGrooveRenderRequest,
        geometry: StylusGeometry,
    ) -> Result<RealtimePagedGrooveTraceResolution, RealtimePagedGrooveError> {
        let view = match self.resolve(request)? {
            RealtimePagedGrooveRenderResolution::Ready(view) => view,
            RealtimePagedGrooveRenderResolution::Miss(miss) => {
                return Ok(RealtimePagedGrooveTraceResolution::Miss(miss));
            }
        };
        let cut = self.metadata.cut();
        let groove_radius_m = cut.layout().radius_at_frame(
            request.absolute_frame_position(),
            cut.cut().groove_pitch_m_per_revolution,
        );
        let meters_per_source_frame = cut.layout().meters_per_frame_at(
            request.absolute_frame_position(),
            cut.cut().groove_pitch_m_per_revolution,
        );
        let wall_contacts = [
            view.trace_wall_contacts(0, meters_per_source_frame, geometry)?,
            view.trace_wall_contacts(1, meters_per_source_frame, geometry)?,
        ];
        Ok(RealtimePagedGrooveTraceResolution::Ready(
            RealtimePagedGrooveTraceFrame {
                generation: request.generation(),
                absolute_frame_position: request.absolute_frame_position(),
                source_frame_advance: request.source_frame_advance(),
                direction: request.direction(),
                page_core_range: view.core_range,
                groove_radius_m,
                meters_per_source_frame,
                spatial_filter_lower_step_frames: view.selection.lower.source_frame_step,
                spatial_filter_upper_step_frames: view.selection.upper.source_frame_step,
                spatial_filter_upper_blend: view.selection.upper_level_blend,
                wall_contacts,
            },
        ))
    }

    /// Plans bounded page ranges outside the render callback.
    pub fn bidirectional_prefetch_plan(
        &self,
        absolute_frame_position: f64,
        render_frame_horizon: u32,
        recapture_candidate_frame_positions: &[f64],
    ) -> Result<PagedGroovePrefetchPlan, RealtimePagedGrooveError> {
        if recapture_candidate_frame_positions.len() > MAX_PAGED_GROOVE_PREFETCH_CANDIDATES {
            return Err(PagedGrooveError::TooManyPrefetchCandidates {
                maximum_candidates: MAX_PAGED_GROOVE_PREFETCH_CANDIDATES,
            }
            .into());
        }
        let mut ranges = Vec::with_capacity(recapture_candidate_frame_positions.len() + 1);
        ranges.push(self.prefetch_range(absolute_frame_position, render_frame_horizon)?);
        for &candidate in recapture_candidate_frame_positions {
            ranges.push(self.prefetch_range(candidate, render_frame_horizon)?);
        }
        ranges.sort_unstable_by_key(|range| (range.start_frame(), range.end_frame_exclusive()));
        let mut merged: Vec<GrooveFrameRange> = Vec::with_capacity(ranges.len());
        for range in ranges {
            if let Some(previous) = merged.last_mut() {
                if range.start_frame() <= previous.end_frame_exclusive() {
                    *previous = GrooveFrameRange::new(
                        previous.start_frame(),
                        previous
                            .end_frame_exclusive()
                            .max(range.end_frame_exclusive()),
                    )?;
                    continue;
                }
            }
            merged.push(range);
        }
        Ok(PagedGroovePrefetchPlan::new(
            self.metadata.generation(),
            merged.into_boxed_slice(),
        ))
    }

    fn prefetch_range(
        &self,
        absolute_frame_position: f64,
        render_frame_horizon: u32,
    ) -> Result<GrooveFrameRange, RealtimePagedGrooveError> {
        let request = PagedGrooveRenderRequest::new(
            self.metadata.generation(),
            absolute_frame_position,
            0.0,
        )?;
        if request.absolute_frame_position() >= self.metadata.total_frame_count() as f64 {
            return Err(RealtimePagedGrooveError::FramePositionOutsideRecord);
        }
        let anchor = request.absolute_frame_position().floor() as u64;
        let span = u64::from(render_frame_horizon)
            .checked_mul(MAX_PAGED_GROOVE_RENDER_SPEED as u64)
            .ok_or(RealtimePagedGrooveError::PageLayoutOverflow)?;
        GrooveFrameRange::new(
            anchor.saturating_sub(span),
            anchor
                .saturating_add(span)
                .saturating_add(2)
                .min(self.metadata.total_frame_count()),
        )
        .map_err(Into::into)
    }
}

/// Borrows one spatial level in a published slot.
#[derive(Debug, Clone, Copy)]
pub struct RealtimeGrooveLevelView<'a> {
    first_source_frame: u64,
    source_frame_step: u32,
    lateral_displacement_m: &'a [f32],
    vertical_displacement_m: &'a [f32],
}

impl<'a> RealtimeGrooveLevelView<'a> {
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

/// Borrows two spatial levels selected for one trace.
#[derive(Debug, Clone, Copy)]
pub struct RealtimeGrooveLevelSelection<'a> {
    lower: RealtimeGrooveLevelView<'a>,
    upper: RealtimeGrooveLevelView<'a>,
    upper_level_blend: f64,
}

impl<'a> RealtimeGrooveLevelSelection<'a> {
    pub fn lower(self) -> RealtimeGrooveLevelView<'a> {
        self.lower
    }

    pub fn upper(self) -> RealtimeGrooveLevelView<'a> {
        self.upper
    }

    pub fn upper_level_blend(self) -> f64 {
        self.upper_level_blend
    }
}

/// Borrows one published page and its selected levels.
#[derive(Debug, Clone, Copy)]
pub struct RealtimePagedGrooveTraceView<'a> {
    core_range: GrooveFrameRange,
    stored_range: GrooveFrameRange,
    absolute_frame_position: f64,
    direction: GrooveTravelDirection,
    selection: RealtimeGrooveLevelSelection<'a>,
    available_tracing_halo_frames: u32,
    validated_trace_admission: ValidatedGrooveTraceAdmissionCertificate,
}

impl<'a> RealtimePagedGrooveTraceView<'a> {
    pub fn core_range(self) -> GrooveFrameRange {
        self.core_range
    }

    pub fn stored_range(self) -> GrooveFrameRange {
        self.stored_range
    }

    pub fn absolute_frame_position(self) -> f64 {
        self.absolute_frame_position
    }

    pub fn direction(self) -> GrooveTravelDirection {
        self.direction
    }

    pub fn level_selection(self) -> RealtimeGrooveLevelSelection<'a> {
        self.selection
    }

    pub fn trace_wall_contacts(
        self,
        wall_index: usize,
        meters_per_source_frame: f64,
        geometry: StylusGeometry,
    ) -> Result<StylusTraceContactSet, RealtimePagedGrooveError> {
        let maximum_level_step = self
            .selection
            .lower
            .source_frame_step
            .max(self.selection.upper.source_frame_step);
        let support =
            geometry.multiresolution_support(meters_per_source_frame, maximum_level_step)?;
        let required_frames = support.symmetric_halo_source_frames();
        if required_frames > self.available_tracing_halo_frames {
            return Err(RealtimePagedGrooveError::Stylus(
                StylusTraceError::InsufficientPageHalo {
                    required_frames,
                    available_frames: self.available_tracing_halo_frames,
                },
            ));
        }
        let admission = self.validated_trace_admission;
        admission.validate_for_active_tracing(geometry)?;
        match admission.certificate().admission_class() {
            GrooveTraceAdmissionClass::StrictConcavity => Ok(
                trace_spherical_45_45_wall_multiresolution_contacts_certified_concave(
                    self.selection.lower.lateral_displacement_m,
                    self.selection.lower.vertical_displacement_m,
                    self.selection.lower.first_source_frame,
                    self.selection.lower.source_frame_step,
                    self.selection.upper.lateral_displacement_m,
                    self.selection.upper.vertical_displacement_m,
                    self.selection.upper.first_source_frame,
                    self.selection.upper.source_frame_step,
                    self.selection.upper_level_blend,
                    wall_index,
                    self.absolute_frame_position,
                    meters_per_source_frame,
                    geometry,
                    admission.certified_concave_trace_bounds(geometry)?,
                )?,
            ),
            GrooveTraceAdmissionClass::FixedCapPiecewise => Ok(
                trace_spherical_45_45_wall_multiresolution_contacts_certified_piecewise(
                    self.selection.lower.lateral_displacement_m,
                    self.selection.lower.vertical_displacement_m,
                    self.selection.lower.first_source_frame,
                    self.selection.lower.source_frame_step,
                    self.selection.upper.lateral_displacement_m,
                    self.selection.upper.vertical_displacement_m,
                    self.selection.upper.first_source_frame,
                    self.selection.upper.source_frame_step,
                    self.selection.upper_level_blend,
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

// This result stays inline and `Copy` so realtime tracing does not allocate.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy)]
pub enum RealtimePagedGrooveRenderResolution<'a> {
    Ready(RealtimePagedGrooveTraceView<'a>),
    Miss(PagedGrooveRenderMiss),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RealtimePagedGrooveTraceResolution {
    Ready(RealtimePagedGrooveTraceFrame),
    Miss(PagedGrooveRenderMiss),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RealtimePagedGrooveTraceFrame {
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

fn select_levels(
    slot: &RealtimePageSlot,
    source_frame_advance: f64,
) -> Result<RealtimeGrooveLevelSelection<'_>, RealtimePagedGrooveError> {
    if !source_frame_advance.is_finite() {
        return Err(RealtimePagedGrooveError::InvalidSourceFrameAdvance);
    }
    let descriptor = slot.descriptor();
    let len = slot.stored_len();
    let base = RealtimeGrooveLevelView {
        first_source_frame: descriptor.stored_range.start_frame(),
        source_frame_step: 1,
        lateral_displacement_m: &slot.lateral_displacement_m[..len],
        vertical_displacement_m: &slot.vertical_displacement_m[..len],
    };
    let available_levels = slot
        .levels
        .iter()
        .take_while(|level| level.len >= 4)
        .count();
    if available_levels == 0 || source_frame_advance.abs() <= 1.0 {
        return Ok(RealtimeGrooveLevelSelection {
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
    let view = |index: usize| {
        if index == 0 {
            base
        } else {
            let level = &slot.levels[index - 1];
            RealtimeGrooveLevelView {
                first_source_frame: level.first_source_frame,
                source_frame_step: level.source_frame_step,
                lateral_displacement_m: level.lateral(),
                vertical_displacement_m: level.vertical(),
            }
        }
    };
    Ok(RealtimeGrooveLevelSelection {
        lower: view(lower_index),
        upper: view(upper_index),
        upper_level_blend: octave - lower_index as f64,
    })
}

fn append_level_header(hash: &mut GrooveContentHasher, level: &RealtimeSpatialLevel) {
    hash.u64(level.first_source_frame);
    hash.u32(level.source_frame_step);
    hash.u64(level.len as u64);
}

fn slot_base_trace_admission_level(slot: &RealtimePageSlot) -> GrooveTraceAdmissionLevel<'_> {
    GrooveTraceAdmissionLevel {
        first_source_frame: slot.descriptor().stored_range.start_frame(),
        source_frame_step: 1,
        lateral_displacement_m: &slot.lateral_displacement_m[..slot.stored_len()],
        vertical_displacement_m: &slot.vertical_displacement_m[..slot.stored_len()],
    }
}

fn slot_trace_admission_levels(
    slot: &RealtimePageSlot,
) -> [GrooveTraceAdmissionLevel<'_>; GROOVE_SPATIAL_PYRAMID_LEVELS] {
    std::array::from_fn(|index| GrooveTraceAdmissionLevel {
        first_source_frame: slot.levels[index].first_source_frame,
        source_frame_step: slot.levels[index].source_frame_step,
        lateral_displacement_m: slot.levels[index].lateral(),
        vertical_displacement_m: slot.levels[index].vertical(),
    })
}

fn filter_sample(input: &[f32], center: usize) -> f32 {
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
    filtered as f32
}

fn sample_from_slot(slot: &RealtimePageSlot, frame: u64) -> Option<GrooveSample> {
    let stored_range = slot.descriptor().stored_range;
    if !stored_range.contains(frame) {
        return None;
    }
    let index = usize::try_from(frame - stored_range.start_frame()).ok()?;
    Some(GrooveSample {
        lateral_displacement_m: slot.lateral_displacement_m[index],
        vertical_displacement_m: slot.vertical_displacement_m[index],
    })
}

fn spatial_sample_from_slot(
    slot: &RealtimePageSlot,
    level_index: usize,
    frame: u64,
) -> Option<GrooveSample> {
    let level = slot.levels.get(level_index)?;
    let step = u64::from(level.source_frame_step);
    let offset = frame.checked_sub(level.first_source_frame)?;
    if step == 0 || offset % step != 0 {
        return None;
    }
    let index = usize::try_from(offset / step).ok()?;
    if index >= level.len {
        return None;
    }
    Some(GrooveSample {
        lateral_displacement_m: level.lateral_displacement_m[index],
        vertical_displacement_m: level.vertical_displacement_m[index],
    })
}

fn contains_range(outer: GrooveFrameRange, inner: GrooveFrameRange) -> bool {
    outer.start_frame() <= inner.start_frame()
        && inner.end_frame_exclusive() <= outer.end_frame_exclusive()
}

fn ranges_overlap(left: GrooveFrameRange, right: GrooveFrameRange) -> bool {
    left.start_frame() < right.end_frame_exclusive()
        && right.start_frame() < left.end_frame_exclusive()
}

fn required_resident_bytes(
    config: RealtimePagedGrooveCacheConfig,
) -> Result<u64, RealtimePagedGrooveError> {
    let maximum_frames = u64::from(config.maximum_stored_frames_per_page);
    let mut samples_per_slot = maximum_frames
        .checked_mul(2)
        .ok_or(RealtimePagedGrooveError::ResidentSizeOverflow)?;
    for step in [2_u64, 4, 8, 16] {
        let level_frames = maximum_frames
            .checked_add(step - 1)
            .ok_or(RealtimePagedGrooveError::ResidentSizeOverflow)?
            / step;
        samples_per_slot = samples_per_slot
            .checked_add(
                level_frames
                    .checked_mul(2)
                    .ok_or(RealtimePagedGrooveError::ResidentSizeOverflow)?,
            )
            .ok_or(RealtimePagedGrooveError::ResidentSizeOverflow)?;
    }
    let sample_bytes = samples_per_slot
        .checked_mul(size_of::<f32>() as u64)
        .and_then(|value| value.checked_mul(u64::from(config.page_slots)))
        .ok_or(RealtimePagedGrooveError::ResidentSizeOverflow)?;
    let slot_bytes = (size_of::<RealtimePageSlot>() as u64)
        .checked_mul(u64::from(config.page_slots))
        .ok_or(RealtimePagedGrooveError::ResidentSizeOverflow)?;
    sample_bytes
        .checked_add(slot_bytes)
        .and_then(|value| value.checked_add(size_of::<RealtimePagedGrooveCache>() as u64))
        .ok_or(RealtimePagedGrooveError::ResidentSizeOverflow)
}

fn allocate_zeroed_f32(len: usize) -> Result<Box<[f32]>, RealtimePagedGrooveError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| RealtimePagedGrooveError::AllocationFailed)?;
    values.resize(len, 0.0);
    Ok(values.into_boxed_slice())
}

#[derive(Debug, Error)]
pub enum RealtimePagedGrooveError {
    #[error("real-time groove cache configuration field {field} is invalid")]
    InvalidConfig { field: &'static str },
    #[error("real-time groove cache needs {required_bytes} bytes but permits {maximum_bytes}")]
    ResidentLimitExceeded {
        required_bytes: u64,
        maximum_bytes: u64,
    },
    #[error("real-time groove cache size calculation overflowed")]
    ResidentSizeOverflow,
    #[error("real-time groove cache storage allocation failed")]
    AllocationFailed,
    #[error("real-time groove cache has no empty page slot")]
    NoEmptySlot,
    #[error("real-time groove page ticket sequence is exhausted")]
    TicketSequenceExhausted,
    #[error("real-time groove page ticket is stale")]
    StaleTicket,
    #[error("a page chunk reservation is already active")]
    ChunkReservationActive,
    #[error("real-time groove page chunk reservation sequence is exhausted")]
    ChunkReservationSequenceExhausted,
    #[error("real-time groove page chunk reservation is stale")]
    StaleChunkReservation,
    #[error("slot {slot_index} is outside the cache's {page_slots} slots")]
    InvalidSlot { slot_index: u32, page_slots: u32 },
    #[error("page phase is {actual:?}, but this operation requires {expected:?}")]
    WrongPhase {
        expected: RealtimePagedGrooveSlotPhase,
        actual: RealtimePagedGrooveSlotPhase,
    },
    #[error("page generation {actual:?} does not match cache generation {expected:?}")]
    GenerationMismatch {
        expected: GrooveGenerationId,
        actual: GrooveGenerationId,
    },
    #[error("page asset identity does not match the cache metadata identity")]
    AssetContentIdentityMismatch,
    #[error("stored frame range does not contain the page core")]
    StoredRangeDoesNotContainCore,
    #[error("stored frame range is outside the complete record")]
    StoredRangeOutsideRecord,
    #[error("page needs {requested_frames} frames but its slot permits {maximum_frames}")]
    PageCapacityExceeded {
        requested_frames: u64,
        maximum_frames: u32,
    },
    #[error("page storage must use the exact halo {required_start}..{required_end_exclusive}")]
    IncorrectStorageHalo {
        required_start: u64,
        required_end_exclusive: u64,
    },
    #[error("page core overlaps the core reserved by slot {slot_index}")]
    CoreOverlap { slot_index: u32 },
    #[error("page storage overlaps the unfinished page in slot {slot_index}")]
    StagingRangeOverlap { slot_index: u32 },
    #[error("page chunk must contain at least one frame")]
    EmptyChunk,
    #[error("page chunk has {requested_frames} frames but permits {maximum_frames}")]
    ChunkLimitExceeded {
        requested_frames: u32,
        maximum_frames: u32,
    },
    #[error("page chunk starts at {actual_offset}, but the next offset is {expected_offset}")]
    NonsequentialChunk {
        expected_offset: u32,
        actual_offset: u32,
    },
    #[error("page chunk range calculation overflowed")]
    ChunkRangeOverflow,
    #[error("page chunk extends outside the declared stored range")]
    ChunkOutsidePage,
    #[error("page chunk contains a nonfinite displacement")]
    NonfiniteDisplacement,
    #[error(
        "page needs {expected_frames} frames per channel but has {lateral_frames} lateral and {vertical_frames} vertical frames"
    )]
    IncompleteChannels {
        expected_frames: u32,
        lateral_frames: u32,
        vertical_frames: u32,
    },
    #[error(
        "spatial level {level_index} needs {expected_frames} frames but has {lateral_frames} lateral and {vertical_frames} vertical frames"
    )]
    IncompleteSpatialLevel {
        level_index: u8,
        expected_frames: u32,
        lateral_frames: u32,
        vertical_frames: u32,
    },
    #[error("spatial level {level_index} is outside the cache's {level_count} levels")]
    InvalidSpatialLevel { level_index: u8, level_count: u8 },
    #[error("this page did not request a precomputed spatial pyramid")]
    PrecomputedPyramidNotRequested,
    #[error("page pyramid layout calculation overflowed")]
    PageLayoutOverflow,
    #[error("work budget {requested} is outside the configured maximum {maximum}")]
    InvalidWorkBudget { requested: u32, maximum: u32 },
    #[error("page validation failed: {0:?}")]
    PageRejected(RealtimePagedGroovePageFailure),
    #[error("a published page must use eviction instead of discard")]
    CannotDiscardPublishedPage,
    #[error("slot {slot_index} is validating a seam against this published page")]
    PageHasStagingDependency { slot_index: u32 },
    #[error("groove page map is internally inconsistent")]
    InternalPageMap,
    #[error("absolute groove frame position is outside the complete record")]
    FramePositionOutsideRecord,
    #[error("source frame advance must be finite")]
    InvalidSourceFrameAdvance,
    #[error(transparent)]
    PagedGroove(#[from] PagedGrooveError),
    #[error(transparent)]
    Groove(#[from] super::GrooveError),
    #[error(transparent)]
    Stylus(#[from] StylusTraceError),
    #[error(transparent)]
    TraceAdmission(#[from] GrooveTraceAdmissionError),
}

#[cfg(test)]
mod tests {
    use super::super::trace_admission::certify_groove_trace_representation;
    use super::*;
    use crate::physical::{
        GrooveCutReport, GrooveLayout, PagedGrooveCacheLimits, PagedGrooveCacheProducer,
        PhysicalGrooveCutMetadata, RecordCutConfig,
    };

    const TOTAL_FRAMES: u64 = 4_096;
    const SEAM_FRAME: u64 = 2_048;
    const TRACING_HALO: u32 = 64;

    fn generation() -> GrooveGenerationId {
        GrooveGenerationId::new(901).unwrap()
    }

    fn metadata() -> PhysicalGrooveMetadata {
        let layout = GrooveLayout::lp_33_seed();
        let cut = RecordCutConfig::seed();
        let final_program_radius_m = layout.unclamped_radius_at_frame(
            (TOTAL_FRAMES - 1) as f64,
            cut.groove_pitch_m_per_revolution,
        );
        PhysicalGrooveMetadata::new(
            generation(),
            GrooveContentIdentity::from_sha256([0x71; 32]),
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
            ),
            TOTAL_FRAMES,
            TRACING_HALO,
        )
        .unwrap()
    }

    fn config() -> RealtimePagedGrooveCacheConfig {
        RealtimePagedGrooveCacheConfig {
            page_slots: 4,
            maximum_stored_frames_per_page: 3_000,
            maximum_chunk_frames: 257,
            maximum_work_units_per_call: 2_048,
            maximum_resident_bytes: 1_000_000,
        }
    }

    fn sample(frame: u64) -> GrooveSample {
        GrooveSample {
            lateral_displacement_m: ((frame as f64 * 0.017).sin() * 8.0e-6) as f32,
            vertical_displacement_m: ((frame as f64 * 0.011).cos() * 4.0e-6) as f32,
        }
    }

    fn page(core_start: u64, core_end: u64) -> PhysicalGroovePage {
        let metadata = metadata();
        let halo = u64::from(metadata.required_storage_halo_frames());
        let stored_start = core_start.saturating_sub(halo);
        let stored_end = core_end.saturating_add(halo).min(TOTAL_FRAMES);
        let samples: Vec<_> = (stored_start..stored_end).map(sample).collect();
        PhysicalGroovePage::new(
            metadata,
            GrooveFrameRange::new(core_start, core_end).unwrap(),
            GrooveFrameRange::new(stored_start, stored_end).unwrap(),
            samples
                .iter()
                .map(|value| value.lateral_displacement_m)
                .collect(),
            samples
                .iter()
                .map(|value| value.vertical_displacement_m)
                .collect(),
        )
        .unwrap()
    }

    fn rejected_wall_slope_page() -> PhysicalGroovePage {
        let metadata = metadata();
        let core_start = 0;
        let core_end = SEAM_FRAME;
        let stored_start = 0;
        let stored_end = core_end
            .saturating_add(u64::from(metadata.required_storage_halo_frames()))
            .min(TOTAL_FRAMES);
        let stored_len = usize::try_from(stored_end - stored_start).unwrap();
        let lateral: Vec<f32> = (0..stored_len)
            .map(|index| if index % 2 == 0 { -1.0e-3 } else { 1.0e-3 })
            .collect();
        let page = PhysicalGroovePage::new(
            metadata,
            GrooveFrameRange::new(core_start, core_end).unwrap(),
            GrooveFrameRange::new(stored_start, stored_end).unwrap(),
            lateral,
            vec![0.0; stored_len],
        )
        .unwrap();
        assert_eq!(
            page.trace_admission_certificate()
                .unwrap()
                .admission_class(),
            GrooveTraceAdmissionClass::RejectedWallSlope
        );
        page
    }

    fn ingest_all(
        cache: &mut RealtimePagedGrooveCache,
        page: &PhysicalGroovePage,
    ) -> RealtimePagedGroovePageTicket {
        let ticket = cache
            .begin_page(RealtimePagedGroovePageDescriptor::from_page(page))
            .unwrap();
        for (offset, chunk) in page.lateral_displacement_m().chunks(257).enumerate() {
            cache
                .ingest_lateral_chunk(ticket, (offset * 257) as u32, chunk)
                .unwrap();
        }
        for (offset, chunk) in page.vertical_displacement_m().chunks(257).enumerate() {
            cache
                .ingest_vertical_chunk(ticket, (offset * 257) as u32, chunk)
                .unwrap();
        }
        cache.finish_page_ingestion(ticket).unwrap();
        ticket
    }

    fn ingest_all_raw(
        cache: &mut RealtimePagedGrooveCache,
        page: &PhysicalGroovePage,
    ) -> RealtimePagedGroovePageTicket {
        let mut descriptor = RealtimePagedGroovePageDescriptor::from_page(page);
        descriptor.trace_admission_certificate = None;
        let ticket = cache.begin_raw_page(descriptor).unwrap();
        for (offset, chunk) in page.lateral_displacement_m().chunks(257).enumerate() {
            cache
                .ingest_lateral_chunk(ticket, (offset * 257) as u32, chunk)
                .unwrap();
        }
        for (offset, chunk) in page.vertical_displacement_m().chunks(257).enumerate() {
            cache
                .ingest_vertical_chunk(ticket, (offset * 257) as u32, chunk)
                .unwrap();
        }
        cache.finish_page_ingestion(ticket).unwrap();
        ticket
    }

    fn ingest_all_precomputed(
        cache: &mut RealtimePagedGrooveCache,
        page: &PhysicalGroovePage,
    ) -> RealtimePagedGroovePageTicket {
        let ticket = cache
            .begin_page_with_precomputed_pyramid(RealtimePagedGroovePageDescriptor::from_page(page))
            .unwrap();
        for (offset, chunk) in page.lateral_displacement_m().chunks(257).enumerate() {
            cache
                .ingest_lateral_chunk(ticket, (offset * 257) as u32, chunk)
                .unwrap();
        }
        for (offset, chunk) in page.vertical_displacement_m().chunks(257).enumerate() {
            cache
                .ingest_vertical_chunk(ticket, (offset * 257) as u32, chunk)
                .unwrap();
        }
        for (level_index, level) in page.spatial_pyramid().unwrap().levels().iter().enumerate() {
            let layout = cache
                .spatial_level_layout(ticket, level_index as u8)
                .unwrap();
            assert_eq!(layout.first_source_frame, level.first_source_frame());
            assert_eq!(layout.source_frame_step, level.source_frame_step());
            assert_eq!(
                layout.frame_count as usize,
                level.lateral_displacement_m().len()
            );
            for (offset, chunk) in level.lateral_displacement_m().chunks(257).enumerate() {
                cache
                    .ingest_spatial_level_lateral_chunk(
                        ticket,
                        level_index as u8,
                        (offset * 257) as u32,
                        chunk,
                    )
                    .unwrap();
            }
            for (offset, chunk) in level.vertical_displacement_m().chunks(257).enumerate() {
                cache
                    .ingest_spatial_level_vertical_chunk(
                        ticket,
                        level_index as u8,
                        (offset * 257) as u32,
                        chunk,
                    )
                    .unwrap();
            }
        }
        let progress = cache.finish_page_ingestion(ticket).unwrap();
        assert_eq!(
            progress.phase,
            RealtimePagedGrooveSlotPhase::BuildingPyramid
        );
        ticket
    }

    fn ingest_all_precomputed_reserved(
        cache: &mut RealtimePagedGrooveCache,
        page: &PhysicalGroovePage,
    ) -> RealtimePagedGroovePageTicket {
        let ticket = cache
            .begin_page_with_precomputed_pyramid(RealtimePagedGroovePageDescriptor::from_page(page))
            .unwrap();
        for (offset, chunk) in page.lateral_displacement_m().chunks(257).enumerate() {
            let reservation = cache
                .reserve_lateral_chunk(ticket, (offset * 257) as u32, chunk.len() as u32)
                .unwrap();
            cache
                .reserved_chunk_mut(reservation)
                .unwrap()
                .copy_from_slice(chunk);
            cache.commit_reserved_chunk(reservation).unwrap();
        }
        for (offset, chunk) in page.vertical_displacement_m().chunks(257).enumerate() {
            let reservation = cache
                .reserve_vertical_chunk(ticket, (offset * 257) as u32, chunk.len() as u32)
                .unwrap();
            cache
                .reserved_chunk_mut(reservation)
                .unwrap()
                .copy_from_slice(chunk);
            cache.commit_reserved_chunk(reservation).unwrap();
        }
        for (level_index, level) in page.spatial_pyramid().unwrap().levels().iter().enumerate() {
            for (offset, chunk) in level.lateral_displacement_m().chunks(257).enumerate() {
                let reservation = cache
                    .reserve_spatial_level_lateral_chunk(
                        ticket,
                        level_index as u8,
                        (offset * 257) as u32,
                        chunk.len() as u32,
                    )
                    .unwrap();
                cache
                    .reserved_chunk_mut(reservation)
                    .unwrap()
                    .copy_from_slice(chunk);
                cache.commit_reserved_chunk(reservation).unwrap();
            }
            for (offset, chunk) in level.vertical_displacement_m().chunks(257).enumerate() {
                let reservation = cache
                    .reserve_spatial_level_vertical_chunk(
                        ticket,
                        level_index as u8,
                        (offset * 257) as u32,
                        chunk.len() as u32,
                    )
                    .unwrap();
                cache
                    .reserved_chunk_mut(reservation)
                    .unwrap()
                    .copy_from_slice(chunk);
                cache.commit_reserved_chunk(reservation).unwrap();
            }
        }
        let progress = cache.finish_page_ingestion(ticket).unwrap();
        assert_eq!(
            progress.phase,
            RealtimePagedGrooveSlotPhase::BuildingPyramid
        );
        ticket
    }

    fn finish_and_publish(
        cache: &mut RealtimePagedGrooveCache,
        ticket: RealtimePagedGroovePageTicket,
    ) {
        loop {
            let progress = cache.advance_page(ticket, 2_048).unwrap();
            assert!(progress.work_units_consumed <= 2_048);
            if progress.phase == RealtimePagedGrooveSlotPhase::Ready {
                break;
            }
        }
        cache.publish_page(ticket).unwrap();
    }

    fn staged_page_content_identity(slot: &RealtimePageSlot) -> GrooveContentIdentity {
        let descriptor = slot.descriptor();
        let mut hash = GrooveContentHasher::new(b"record-player-paged-groove-page-v4\0");
        hash.u64(descriptor.generation.get());
        hash.identity(descriptor.asset_content_identity);
        match descriptor.trace_admission_certificate {
            Some(certificate) => {
                hash.u8(1);
                hash.identity(certificate.certificate_identity());
            }
            None => hash.u8(0),
        }
        hash.u64(descriptor.core_range.start_frame());
        hash.u64(descriptor.core_range.end_frame_exclusive());
        hash.u64(descriptor.stored_range.start_frame());
        hash.u64(descriptor.stored_range.end_frame_exclusive());
        hash.u64(slot.stored_len() as u64);
        for sample in &slot.lateral_displacement_m[..slot.stored_len()] {
            hash.f32(*sample);
        }
        hash.u64(slot.stored_len() as u64);
        for sample in &slot.vertical_displacement_m[..slot.stored_len()] {
            hash.f32(*sample);
        }
        hash.u8(1);
        hash.u32(GROOVE_SPATIAL_PYRAMID_FORMAT_VERSION);
        hash.u64(GROOVE_SPATIAL_PYRAMID_LEVELS as u64);
        for level in &slot.levels {
            append_level_header(&mut hash, level);
            for sample in level.lateral() {
                hash.f32(*sample);
            }
            hash.u64(level.len as u64);
            for sample in level.vertical() {
                hash.f32(*sample);
            }
        }
        hash.finish()
    }

    fn immutable_cache(pages: Vec<PhysicalGroovePage>) -> super::super::PagedGrooveCache {
        let mut producer = PagedGrooveCacheProducer::new(
            metadata(),
            PagedGrooveCacheLimits::new(4, 2_000_000).unwrap(),
        )
        .unwrap();
        for page in pages {
            producer.insert_page(page).unwrap();
        }
        producer.publish()
    }

    #[test]
    fn partial_page_is_invisible_and_publication_has_bounded_manifest_work() {
        let page = page(0, SEAM_FRAME);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let ticket = cache
            .begin_page(RealtimePagedGroovePageDescriptor::from_page(&page))
            .unwrap();
        cache
            .ingest_lateral_chunk(ticket, 0, &page.lateral_displacement_m()[..127])
            .unwrap();
        let request = PagedGrooveRenderRequest::new(generation(), 100.25, 1.0).unwrap();
        assert!(matches!(
            cache.resolve(request).unwrap(),
            RealtimePagedGrooveRenderResolution::Miss(PagedGrooveRenderMiss::PageUnavailable {
                frame: 100,
                ..
            })
        ));

        cache.discard_page(ticket).unwrap();
        let ticket = ingest_all(&mut cache, &page);
        finish_and_publish(&mut cache, ticket);
        assert!(matches!(
            cache.resolve(request).unwrap(),
            RealtimePagedGrooveRenderResolution::Ready(_)
        ));
        assert_eq!(cache.status().published_pages, 1);
    }

    #[test]
    fn rejection_class_certificate_never_reaches_ready_or_publication() {
        let page = rejected_wall_slope_page();
        for rust_owned_certificate in [false, true] {
            let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
            let ticket = if rust_owned_certificate {
                ingest_all_raw(&mut cache, &page)
            } else {
                ingest_all(&mut cache, &page)
            };

            loop {
                match cache.advance_page(ticket, 2_048) {
                    Ok(progress) => {
                        assert_ne!(progress.phase, RealtimePagedGrooveSlotPhase::Ready);
                        assert_ne!(progress.phase, RealtimePagedGrooveSlotPhase::Published);
                        if progress.phase == RealtimePagedGrooveSlotPhase::Rejected {
                            break;
                        }
                    }
                    Err(RealtimePagedGrooveError::PageRejected(failure)) => {
                        assert_eq!(
                            failure,
                            RealtimePagedGroovePageFailure::TraceAdmissionNotAdmitted
                        );
                        break;
                    }
                    Err(error) => panic!("unexpected page error: {error:?}"),
                }
            }

            let progress = cache.slot_progress(ticket.slot_index()).unwrap();
            assert_eq!(progress.phase, RealtimePagedGrooveSlotPhase::Rejected);
            assert_eq!(
                progress.failure,
                Some(RealtimePagedGroovePageFailure::TraceAdmissionNotAdmitted)
            );
            assert!(cache.slots[ticket.slot_index() as usize]
                .validated_trace_admission
                .is_none());
            assert!(matches!(
                cache.publish_page(ticket),
                Err(RealtimePagedGrooveError::WrongPhase {
                    expected: RealtimePagedGrooveSlotPhase::Ready,
                    actual: RealtimePagedGrooveSlotPhase::Rejected,
                })
            ));
            assert_eq!(cache.status().published_pages, 0);
        }
    }

    #[test]
    fn published_manifest_binds_ranges_page_hashes_and_certificate_hashes() {
        let immutable_first = immutable_cache(vec![page(0, SEAM_FRAME)]);
        let immutable_second = immutable_cache(vec![page(SEAM_FRAME, TOTAL_FRAMES)]);
        let immutable_both =
            immutable_cache(vec![page(0, SEAM_FRAME), page(SEAM_FRAME, TOTAL_FRAMES)]);
        let first = page(0, SEAM_FRAME);
        let second = page(SEAM_FRAME, TOTAL_FRAMES);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let empty_identity = cache.trace_admitted_representation_identity();

        let second_ticket = ingest_all(&mut cache, &second);
        finish_and_publish(&mut cache, second_ticket);
        let second_identity = cache.trace_admitted_representation_identity();
        assert_ne!(second_identity, empty_identity);
        assert_eq!(
            second_identity,
            immutable_second.trace_admitted_representation_identity()
        );

        let first_ticket = ingest_all(&mut cache, &first);
        finish_and_publish(&mut cache, first_ticket);
        let both_identity = cache.trace_admitted_representation_identity();
        assert_ne!(both_identity, second_identity);
        assert_eq!(
            both_identity,
            immutable_both.trace_admitted_representation_identity()
        );

        cache.evict_page_containing(SEAM_FRAME).unwrap().unwrap();
        assert_eq!(
            cache.trace_admitted_representation_identity(),
            immutable_first.trace_admitted_representation_identity()
        );
    }

    #[test]
    fn ingest_build_publish_trace_and_evict_make_no_allocator_calls() {
        let page = page(0, SEAM_FRAME);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let mut completed = false;
        assert_no_alloc::assert_no_alloc(|| {
            let ticket = cache
                .begin_page(RealtimePagedGroovePageDescriptor::from_page(&page))
                .unwrap();
            for (offset, chunk) in page.lateral_displacement_m().chunks(257).enumerate() {
                cache
                    .ingest_lateral_chunk(ticket, (offset * 257) as u32, chunk)
                    .unwrap();
            }
            for (offset, chunk) in page.vertical_displacement_m().chunks(257).enumerate() {
                cache
                    .ingest_vertical_chunk(ticket, (offset * 257) as u32, chunk)
                    .unwrap();
            }
            cache.finish_page_ingestion(ticket).unwrap();
            loop {
                let progress = cache.advance_page(ticket, 2_048).unwrap();
                if progress.phase == RealtimePagedGrooveSlotPhase::Ready {
                    break;
                }
            }
            cache.publish_page(ticket).unwrap();
            let request = PagedGrooveRenderRequest::new(generation(), 700.25, -20.0).unwrap();
            completed = matches!(
                cache.trace(request, StylusGeometry::default()).unwrap(),
                RealtimePagedGrooveTraceResolution::Ready(_)
            ) && cache.evict_page_containing(700).unwrap().is_some();
        });
        assert!(completed);
    }

    #[test]
    fn reserved_chunk_rejection_is_transactional_and_blocks_page_lifecycle() {
        let page = page(0, SEAM_FRAME);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let ticket = cache
            .begin_page(RealtimePagedGroovePageDescriptor::from_page(&page))
            .unwrap();
        let reservation = cache.reserve_lateral_chunk(ticket, 0, 64).unwrap();
        assert_eq!(reservation.ticket(), ticket);
        assert_eq!(reservation.first_frame_offset(), 0);
        assert_eq!(reservation.frame_count(), 64);
        assert_eq!(
            reservation.target(),
            RealtimePagedGrooveChunkTarget::BaseLateral
        );
        let before = cache.slot_progress(ticket.slot_index()).unwrap();
        assert!(before.chunk_reserved);
        assert_eq!(before.received_lateral_frames, 0);

        assert!(matches!(
            cache.reserve_vertical_chunk(ticket, 0, 1),
            Err(RealtimePagedGrooveError::ChunkReservationActive)
        ));
        assert!(matches!(
            cache.ingest_lateral_chunk(ticket, 0, &page.lateral_displacement_m()[..1]),
            Err(RealtimePagedGrooveError::ChunkReservationActive)
        ));
        assert!(matches!(
            cache.finish_page_ingestion(ticket),
            Err(RealtimePagedGrooveError::ChunkReservationActive)
        ));
        assert!(matches!(
            cache.discard_page(ticket),
            Err(RealtimePagedGrooveError::ChunkReservationActive)
        ));

        cache.reserved_chunk_mut(reservation).unwrap()[17] = f32::NAN;
        assert!(matches!(
            cache.commit_reserved_chunk(reservation),
            Err(RealtimePagedGrooveError::NonfiniteDisplacement)
        ));
        assert_eq!(cache.slot_progress(ticket.slot_index()).unwrap(), before);
        assert!(matches!(
            cache.resolve(PagedGrooveRenderRequest::new(generation(), 17.0, 1.0).unwrap()),
            Ok(RealtimePagedGrooveRenderResolution::Miss(
                PagedGrooveRenderMiss::PageUnavailable { .. }
            ))
        ));

        cache
            .reserved_chunk_mut(reservation)
            .unwrap()
            .copy_from_slice(&page.lateral_displacement_m()[..64]);
        let committed = cache.commit_reserved_chunk(reservation).unwrap();
        assert_eq!(committed.received_lateral_frames, 64);
        assert!(!committed.chunk_reserved);
        assert!(matches!(
            cache.commit_reserved_chunk(reservation),
            Err(RealtimePagedGrooveError::StaleChunkReservation)
        ));

        let cancelled = cache.reserve_lateral_chunk(ticket, 64, 32).unwrap();
        cache
            .reserved_chunk_mut(cancelled)
            .unwrap()
            .copy_from_slice(&page.lateral_displacement_m()[64..96]);
        let progress = cache.cancel_reserved_chunk(cancelled).unwrap();
        assert_eq!(progress.received_lateral_frames, 64);
        assert!(!progress.chunk_reserved);
        assert!(matches!(
            cache.reserved_chunk_mut(cancelled),
            Err(RealtimePagedGrooveError::StaleChunkReservation)
        ));
        let reused = cache.reserve_lateral_chunk(ticket, 64, 32).unwrap();
        assert_ne!(reused.sequence(), cancelled.sequence());
        cache.cancel_reserved_chunk(reused).unwrap();
        cache.discard_page(ticket).unwrap();
    }

    #[test]
    fn reservation_checks_target_order_and_length_before_state_changes() {
        let page = page(0, SEAM_FRAME);
        let mut test_config = config();
        test_config.maximum_chunk_frames = test_config.maximum_stored_frames_per_page;
        let mut cache = RealtimePagedGrooveCache::new(metadata(), test_config).unwrap();
        let ticket = cache
            .begin_page(RealtimePagedGroovePageDescriptor::from_page(&page))
            .unwrap();
        let before = cache.slot_progress(ticket.slot_index()).unwrap();

        assert!(matches!(
            cache.reserve_spatial_level_lateral_chunk(ticket, 0, 0, 1),
            Err(RealtimePagedGrooveError::PrecomputedPyramidNotRequested)
        ));
        assert!(matches!(
            cache.reserve_lateral_chunk(ticket, 1, 1),
            Err(RealtimePagedGrooveError::NonsequentialChunk { .. })
        ));
        assert!(matches!(
            cache.reserve_lateral_chunk(ticket, 0, 0),
            Err(RealtimePagedGrooveError::EmptyChunk)
        ));
        assert!(matches!(
            cache.reserve_lateral_chunk(ticket, 0, test_config.maximum_chunk_frames + 1),
            Err(RealtimePagedGrooveError::ChunkLimitExceeded { .. })
        ));
        assert!(matches!(
            cache.reserve_lateral_chunk(
                ticket,
                0,
                RealtimePagedGroovePageDescriptor::from_page(&page).stored_frame_count() as u32 + 1,
            ),
            Err(RealtimePagedGrooveError::ChunkOutsidePage)
        ));
        assert_eq!(cache.slot_progress(ticket.slot_index()).unwrap(), before);
        cache.discard_page(ticket).unwrap();
    }

    #[test]
    fn reserved_precomputed_ingestion_is_allocation_free_and_bit_exact() {
        let source_page = page(0, SEAM_FRAME);
        let immutable = immutable_cache(vec![page(0, SEAM_FRAME)]);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let mut completed = false;
        assert_no_alloc::assert_no_alloc(|| {
            let ticket = ingest_all_precomputed_reserved(&mut cache, &source_page);
            loop {
                let progress = cache.advance_page(ticket, 2_048).unwrap();
                if progress.phase == RealtimePagedGrooveSlotPhase::Ready {
                    break;
                }
            }
            cache.publish_page(ticket).unwrap();
            completed = true;
        });
        assert!(completed);

        let request = PagedGrooveRenderRequest::new(generation(), 700.25, -20.0).unwrap();
        let actual = match cache.trace(request, StylusGeometry::default()).unwrap() {
            RealtimePagedGrooveTraceResolution::Ready(value) => value,
            RealtimePagedGrooveTraceResolution::Miss(miss) => panic!("miss: {miss:?}"),
        };
        let expected = match immutable.trace(request, StylusGeometry::default()).unwrap() {
            super::super::PagedGrooveTraceResolution::Ready(value) => value,
            super::super::PagedGrooveTraceResolution::Miss(miss) => panic!("miss: {miss:?}"),
        };
        assert_eq!(actual.wall_contacts, expected.wall_contacts);
    }

    #[test]
    fn precomputed_pyramid_ingestion_is_allocation_free_and_bit_exact() {
        let source_page = page(0, SEAM_FRAME);
        let immutable = immutable_cache(vec![page(0, SEAM_FRAME)]);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let mut completed = false;
        assert_no_alloc::assert_no_alloc(|| {
            let ticket = ingest_all_precomputed(&mut cache, &source_page);
            loop {
                let progress = cache.advance_page(ticket, 2_048).unwrap();
                if progress.phase == RealtimePagedGrooveSlotPhase::Ready {
                    break;
                }
            }
            cache.publish_page(ticket).unwrap();
            completed = true;
        });
        assert!(completed);

        let geometry = StylusGeometry::default();
        for advance in [-20.0, -1.0, 1.0, 20.0] {
            let request = PagedGrooveRenderRequest::new(generation(), 700.25, advance).unwrap();
            let actual = match cache.trace(request, geometry).unwrap() {
                RealtimePagedGrooveTraceResolution::Ready(value) => value,
                RealtimePagedGrooveTraceResolution::Miss(miss) => panic!("miss: {miss:?}"),
            };
            let expected = match immutable.trace(request, geometry).unwrap() {
                super::super::PagedGrooveTraceResolution::Ready(value) => value,
                super::super::PagedGrooveTraceResolution::Miss(miss) => panic!("miss: {miss:?}"),
            };
            assert_eq!(actual.wall_contacts, expected.wall_contacts);
        }
    }

    #[test]
    fn self_consistent_noncanonical_precomputed_pyramid_never_becomes_visible() {
        let page = page(0, SEAM_FRAME);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let ticket = ingest_all_precomputed(&mut cache, &page);
        let index = ticket.slot_index() as usize;
        cache.slots[index].levels[1].vertical_displacement_m[13] += 1.0e-7;
        let expected_failure_frame = cache.slots[index].levels[1]
            .first_source_frame
            .saturating_add(13 * u64::from(cache.slots[index].levels[1].source_frame_step));
        let recomputed_certificate = {
            let binding = cache.trace_admission_binding(index);
            let slot = &cache.slots[index];
            let spatial_levels = slot_trace_admission_levels(slot);
            super::super::trace_admission::certify_groove_trace_representation(
                binding,
                slot_base_trace_admission_level(slot),
                &spatial_levels,
            )
            .unwrap()
        };
        cache.slots[index]
            .descriptor
            .as_mut()
            .unwrap()
            .trace_admission_certificate = Some(recomputed_certificate);
        let self_consistent_identity = staged_page_content_identity(&cache.slots[index]);
        cache.slots[index]
            .descriptor
            .as_mut()
            .unwrap()
            .page_content_identity = self_consistent_identity;
        while cache.slots[index].phase != RealtimePagedGrooveSlotPhase::Rejected {
            let _ = cache.advance_page(ticket, 2_048);
        }
        assert_eq!(
            cache.slot_progress(ticket.slot_index()).unwrap().failure,
            Some(RealtimePagedGroovePageFailure::NoncanonicalSpatialPyramid {
                level_index: 1,
                frame: expected_failure_frame,
            })
        );
        let request = PagedGrooveRenderRequest::new(generation(), 700.25, 20.0).unwrap();
        assert!(matches!(
            cache.resolve(request).unwrap(),
            RealtimePagedGrooveRenderResolution::Miss(
                PagedGrooveRenderMiss::PageUnavailable { .. }
            )
        ));
    }

    #[test]
    fn missing_trace_certificate_cannot_pass_a_self_consistent_page_hash() {
        let page = page(0, SEAM_FRAME);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let ticket = ingest_all_precomputed(&mut cache, &page);
        let index = ticket.slot_index() as usize;
        cache.slots[index]
            .descriptor
            .as_mut()
            .unwrap()
            .trace_admission_certificate = None;
        let page_identity = staged_page_content_identity(&cache.slots[index]);
        cache.slots[index]
            .descriptor
            .as_mut()
            .unwrap()
            .page_content_identity = page_identity;
        cache.start_hash(index);

        while cache.slot_progress(ticket.slot_index()).unwrap().phase
            != RealtimePagedGrooveSlotPhase::Rejected
        {
            let _ = cache.advance_page(ticket, 2_048);
        }
        assert_eq!(
            cache.slot_progress(ticket.slot_index()).unwrap().failure,
            Some(RealtimePagedGroovePageFailure::MissingTraceAdmissionCertificate)
        );
        assert!(cache.slots[index].validated_trace_admission.is_none());
    }

    #[test]
    fn stale_page_certificate_cannot_pass_recomputation_after_its_hash_passes() {
        let first = page(0, SEAM_FRAME);
        let second = page(SEAM_FRAME, TOTAL_FRAMES);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let ticket = ingest_all_precomputed(&mut cache, &second);
        let index = ticket.slot_index() as usize;
        cache.slots[index]
            .descriptor
            .as_mut()
            .unwrap()
            .trace_admission_certificate = first.trace_admission_certificate();
        let page_identity = staged_page_content_identity(&cache.slots[index]);
        cache.slots[index]
            .descriptor
            .as_mut()
            .unwrap()
            .page_content_identity = page_identity;
        cache.start_hash(index);

        while cache.slot_progress(ticket.slot_index()).unwrap().phase
            != RealtimePagedGrooveSlotPhase::Rejected
        {
            let _ = cache.advance_page(ticket, 2_048);
        }
        assert_eq!(
            cache.slot_progress(ticket.slot_index()).unwrap().failure,
            Some(RealtimePagedGroovePageFailure::TraceAdmissionCertificateMismatch)
        );
        assert!(cache.slots[index].validated_trace_admission.is_none());
    }

    #[test]
    fn rejected_chunk_leaves_progress_and_next_offset_unchanged() {
        let page = page(0, SEAM_FRAME);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let ticket = cache
            .begin_page(RealtimePagedGroovePageDescriptor::from_page(&page))
            .unwrap();
        cache
            .ingest_lateral_chunk(ticket, 0, &page.lateral_displacement_m()[..64])
            .unwrap();
        let before = cache.slot_progress(ticket.slot_index()).unwrap();
        let corrupt = [0.0_f32, f32::NAN, 0.0];
        assert!(matches!(
            cache.ingest_lateral_chunk(ticket, 64, &corrupt),
            Err(RealtimePagedGrooveError::NonfiniteDisplacement)
        ));
        assert_eq!(cache.slot_progress(ticket.slot_index()).unwrap(), before);
        cache
            .ingest_lateral_chunk(ticket, 64, &page.lateral_displacement_m()[64..67])
            .unwrap();
    }

    #[test]
    fn descriptor_errors_are_transactional_and_work_is_hard_bounded() {
        let page = page(0, SEAM_FRAME);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let before = cache.status();
        let mut wrong_identity = RealtimePagedGroovePageDescriptor::from_page(&page);
        wrong_identity.asset_content_identity = GrooveContentIdentity::from_sha256([0x42; 32]);
        assert!(matches!(
            cache.begin_page(wrong_identity),
            Err(RealtimePagedGrooveError::AssetContentIdentityMismatch)
        ));
        assert_eq!(cache.status(), before);

        let mut wrong_halo = RealtimePagedGroovePageDescriptor::from_page(&page);
        wrong_halo.stored_range = GrooveFrameRange::new(
            wrong_halo.stored_range.start_frame(),
            wrong_halo.stored_range.end_frame_exclusive() - 1,
        )
        .unwrap();
        assert!(matches!(
            cache.begin_page(wrong_halo),
            Err(RealtimePagedGrooveError::IncorrectStorageHalo { .. })
        ));
        assert_eq!(cache.status(), before);

        let ticket = ingest_all(&mut cache, &page);
        let allocated = cache.status().allocated_resident_bytes;
        let progress = cache
            .advance_page(ticket, FILTER_WORK_UNITS_PER_OUTPUT)
            .unwrap();
        assert_eq!(progress.work_units_consumed, FILTER_WORK_UNITS_PER_OUTPUT);
        assert_eq!(
            progress.phase,
            RealtimePagedGrooveSlotPhase::BuildingPyramid
        );
        assert_eq!(cache.status().allocated_resident_bytes, allocated);
        assert!(matches!(
            cache.advance_page(ticket, FILTER_WORK_UNITS_PER_OUTPUT - 1),
            Ok(RealtimePagedGroovePageProgress {
                work_units_consumed: 0,
                ..
            })
        ));
        assert_eq!(cache.status().allocated_resident_bytes, allocated);
    }

    #[test]
    fn trace_certification_is_budgeted_and_finished_pages_cannot_mix_lifecycle_bytes() {
        let page = page(0, SEAM_FRAME);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let descriptor = RealtimePagedGroovePageDescriptor::from_page(&page);
        let estimate = cache.page_work_estimate(descriptor, 0).unwrap();
        let ticket = ingest_all_precomputed(&mut cache, &page);

        let mut canonical_validation_work = 0_u64;
        while cache.slot_progress(ticket.slot_index()).unwrap().phase
            == RealtimePagedGrooveSlotPhase::BuildingPyramid
        {
            let progress = cache
                .advance_page(ticket, FILTER_WORK_UNITS_PER_OUTPUT)
                .unwrap();
            assert!(progress.work_units_consumed <= FILTER_WORK_UNITS_PER_OUTPUT);
            if progress.phase == RealtimePagedGrooveSlotPhase::BuildingPyramid {
                canonical_validation_work += u64::from(progress.work_units_consumed);
            }
        }
        assert_eq!(
            canonical_validation_work,
            estimate.precomputed_pyramid_validation_work_units
        );
        assert_eq!(
            cache.slot_progress(ticket.slot_index()).unwrap().phase,
            RealtimePagedGrooveSlotPhase::Hashing
        );

        let mut certification_work = 0_u64;
        while cache.slot_progress(ticket.slot_index()).unwrap().phase
            == RealtimePagedGrooveSlotPhase::Hashing
        {
            let progress = cache.advance_page(ticket, 1).unwrap();
            assert!(progress.work_units_consumed <= 1);
            if progress.phase == RealtimePagedGrooveSlotPhase::CertifyingTrace {
                certification_work += u64::from(progress.work_units_consumed);
            }
        }
        let index = ticket.slot_index() as usize;
        assert_eq!(
            cache.slot_progress(ticket.slot_index()).unwrap().phase,
            RealtimePagedGrooveSlotPhase::CertifyingTrace
        );
        assert!(cache.slots[index].validated_trace_admission.is_none());
        let first_sample_bits = cache.slots[index].lateral_displacement_m[0].to_bits();
        assert!(matches!(
            cache.ingest_lateral_chunk(ticket, 0, &[1.0e-6]),
            Err(RealtimePagedGrooveError::WrongPhase { .. })
        ));
        assert_eq!(
            cache.slots[index].lateral_displacement_m[0].to_bits(),
            first_sample_bits
        );

        let first = cache.advance_page(ticket, 1).unwrap();
        assert_eq!(first.work_units_consumed, 1);
        assert_eq!(first.phase, RealtimePagedGrooveSlotPhase::CertifyingTrace);
        assert!(cache.slots[index].validated_trace_admission.is_none());

        certification_work += 1;
        while cache.slot_progress(ticket.slot_index()).unwrap().phase
            == RealtimePagedGrooveSlotPhase::CertifyingTrace
        {
            let progress = cache.advance_page(ticket, 1).unwrap();
            assert!(progress.work_units_consumed <= 1);
            certification_work += u64::from(progress.work_units_consumed);
        }
        assert_eq!(certification_work, estimate.trace_admission_work_units);
        assert!(cache.slots[index].validated_trace_admission.is_some());
        assert_eq!(
            cache.slot_progress(ticket.slot_index()).unwrap().phase,
            RealtimePagedGrooveSlotPhase::ValidatingSeams
        );
    }

    #[test]
    fn deterministic_budget_audit_exposes_raw_build_throughput_limits() {
        let page = page(0, SEAM_FRAME);
        let cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let estimate = cache
            .page_work_estimate(
                RealtimePagedGroovePageDescriptor::from_page(&page),
                u64::from(metadata().required_storage_halo_frames()) * 2,
            )
            .unwrap();
        let work_calls_per_second = 48_000_u64 / 128;
        let old_default_work_per_second = 65_536_u64 * work_calls_per_second;
        let current_default_work_per_second =
            u64::from(RealtimePagedGrooveCacheConfig::default().maximum_work_units_per_call)
                * work_calls_per_second;
        assert!(
            estimate.source_frames_per_second_for_raw_budget(old_default_work_per_second)
                < 192_000.0
        );
        assert!(
            estimate.source_frames_per_second_for_raw_budget(current_default_work_per_second)
                > 192_000.0
        );
        assert!(
            estimate.source_frames_per_second_for_raw_budget(current_default_work_per_second)
                < 192_000.0 * 20.0
        );
        assert_eq!(
            estimate.precomputed_pyramid_validation_work_units,
            estimate.raw_pyramid_work_units
        );
        assert_eq!(
            estimate.precomputed_total_work_units(),
            estimate.raw_total_work_units()
        );
        assert!(estimate.precomputed_input_channel_samples > estimate.stored_frames * 2);
    }

    #[test]
    fn content_identity_rejects_changed_base_data_and_generated_pyramid_data() {
        let page = page(0, SEAM_FRAME);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let ticket = cache
            .begin_page(RealtimePagedGroovePageDescriptor::from_page(&page))
            .unwrap();
        let mut lateral = page.lateral_displacement_m().to_vec();
        lateral[700] += 1.0e-7;
        for (offset, chunk) in lateral.chunks(257).enumerate() {
            cache
                .ingest_lateral_chunk(ticket, (offset * 257) as u32, chunk)
                .unwrap();
        }
        for (offset, chunk) in page.vertical_displacement_m().chunks(257).enumerate() {
            cache
                .ingest_vertical_chunk(ticket, (offset * 257) as u32, chunk)
                .unwrap();
        }
        cache.finish_page_ingestion(ticket).unwrap();
        while cache.slots[ticket.slot_index() as usize].phase
            != RealtimePagedGrooveSlotPhase::Rejected
        {
            let _ = cache.advance_page(ticket, 2_048);
        }
        assert_eq!(
            cache.slot_progress(ticket.slot_index()).unwrap().failure,
            Some(RealtimePagedGroovePageFailure::PageContentIdentityMismatch)
        );

        cache.discard_page(ticket).unwrap();
        let ticket = ingest_all(&mut cache, &page);
        let index = ticket.slot_index() as usize;
        while cache.advance_pyramid_one_output(index).unwrap() == PyramidAdvance::OutputProcessed {}
        cache.slots[index].levels[2].lateral_displacement_m[7] += 1.0e-7;
        cache.start_hash(index);
        while cache.slots[index].phase == RealtimePagedGrooveSlotPhase::Hashing {
            let _ = cache.advance_hash(index, 2_048).unwrap();
        }
        assert_eq!(
            cache.slot_progress(ticket.slot_index()).unwrap().failure,
            Some(RealtimePagedGroovePageFailure::PageContentIdentityMismatch)
        );
    }

    #[test]
    fn traces_match_immutable_pages_at_seams_in_both_directions_and_twenty_times() {
        let first = page(0, SEAM_FRAME);
        let second = page(SEAM_FRAME, TOTAL_FRAMES);
        let immutable = immutable_cache(vec![page(0, SEAM_FRAME), page(SEAM_FRAME, TOTAL_FRAMES)]);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let ticket = ingest_all(&mut cache, &first);
        finish_and_publish(&mut cache, ticket);
        let ticket = ingest_all(&mut cache, &second);
        finish_and_publish(&mut cache, ticket);
        let geometry = StylusGeometry {
            tracing_radius_m: 10.0e-6,
        };
        for (position, advance) in [
            (SEAM_FRAME as f64 - 0.25, 20.0),
            (SEAM_FRAME as f64 - 0.25, -20.0),
            (SEAM_FRAME as f64 + 0.25, 20.0),
            (SEAM_FRAME as f64 + 0.25, -20.0),
        ] {
            let request = PagedGrooveRenderRequest::new(generation(), position, advance).unwrap();
            let actual = match cache.trace(request, geometry).unwrap() {
                RealtimePagedGrooveTraceResolution::Ready(value) => value,
                RealtimePagedGrooveTraceResolution::Miss(miss) => panic!("miss: {miss:?}"),
            };
            let expected = match immutable.trace(request, geometry).unwrap() {
                super::super::PagedGrooveTraceResolution::Ready(value) => value,
                super::super::PagedGrooveTraceResolution::Miss(miss) => panic!("miss: {miss:?}"),
            };
            assert_eq!(actual.wall_contacts, expected.wall_contacts);
            for wall in 0..2 {
                let resolve = |contacts| {
                    super::super::tangential_identity::resolve_tangential_contact_identity(
                        metadata().source_content_identity(),
                        generation().get(),
                        TOTAL_FRAMES,
                        wall,
                        contacts,
                    )
                    .unwrap()
                };
                assert_eq!(
                    resolve(actual.wall_contacts[wall]),
                    resolve(expected.wall_contacts[wall])
                );
            }
            assert_eq!(
                actual.spatial_filter_lower_step_frames,
                expected.spatial_filter_lower_step_frames
            );
            assert_eq!(
                actual.spatial_filter_upper_step_frames,
                expected.spatial_filter_upper_step_frames
            );
            assert_eq!(
                actual.spatial_filter_upper_blend.to_bits(),
                expected.spatial_filter_upper_blend.to_bits()
            );
        }
    }

    #[test]
    fn canonical_page_hash_can_pass_while_a_changed_seam_is_rejected() {
        let first = page(0, SEAM_FRAME);
        let second = page(SEAM_FRAME, TOTAL_FRAMES);
        let mut changed_lateral = second.lateral_displacement_m().to_vec();
        changed_lateral[0] += 1.0e-7;
        let changed_second = PhysicalGroovePage::new(
            metadata(),
            second.core_range(),
            second.stored_range(),
            changed_lateral,
            second.vertical_displacement_m().to_vec(),
        )
        .unwrap();
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let ticket = ingest_all(&mut cache, &first);
        finish_and_publish(&mut cache, ticket);
        let changed_ticket = ingest_all(&mut cache, &changed_second);
        while cache.slots[changed_ticket.slot_index() as usize].phase
            != RealtimePagedGrooveSlotPhase::Rejected
        {
            let _ = cache.advance_page(changed_ticket, 2_048);
        }
        assert_eq!(
            cache
                .slot_progress(changed_ticket.slot_index())
                .unwrap()
                .failure,
            Some(RealtimePagedGroovePageFailure::SeamSampleMismatch {
                frame: changed_second.stored_range().start_frame()
            })
        );
        assert_eq!(cache.status().published_pages, 1);
    }

    #[test]
    fn self_consistent_spatial_page_is_rejected_when_its_active_seam_differs() {
        let first = page(0, SEAM_FRAME);
        let second = page(SEAM_FRAME, TOTAL_FRAMES);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let first_ticket = ingest_all(&mut cache, &first);
        finish_and_publish(&mut cache, first_ticket);
        let second_ticket = ingest_all_precomputed(&mut cache, &second);
        let index = second_ticket.slot_index() as usize;
        let overlap_start = first
            .stored_range()
            .start_frame()
            .max(second.stored_range().start_frame());
        let overlap_end = first
            .stored_range()
            .end_frame_exclusive()
            .min(second.stored_range().end_frame_exclusive());
        let spatial_start = overlap_start + u64::from(GROOVE_SPATIAL_FILTER_RADIUS_FRAMES);
        let spatial_end = overlap_end - u64::from(GROOVE_SPATIAL_FILTER_RADIUS_FRAMES);
        let changed_frame = align_up(spatial_start, 2).unwrap();
        assert!(changed_frame < spatial_end);
        let level = &mut cache.slots[index].levels[0];
        let changed_index = usize::try_from(
            (changed_frame - level.first_source_frame) / u64::from(level.source_frame_step),
        )
        .unwrap();
        level.lateral_displacement_m[changed_index] += 1.0e-8;

        let certificate = {
            let binding = cache.trace_admission_binding(index);
            let slot = &cache.slots[index];
            let levels = slot_trace_admission_levels(slot);
            certify_groove_trace_representation(
                binding,
                slot_base_trace_admission_level(slot),
                &levels,
            )
            .unwrap()
        };
        cache.slots[index]
            .descriptor
            .as_mut()
            .unwrap()
            .trace_admission_certificate = Some(certificate);
        let page_identity = staged_page_content_identity(&cache.slots[index]);
        cache.slots[index]
            .descriptor
            .as_mut()
            .unwrap()
            .page_content_identity = page_identity;
        cache.start_hash(index);

        while cache
            .slot_progress(second_ticket.slot_index())
            .unwrap()
            .phase
            != RealtimePagedGrooveSlotPhase::Rejected
        {
            let _ = cache.advance_page(second_ticket, 2_048);
        }
        assert_eq!(
            cache
                .slot_progress(second_ticket.slot_index())
                .unwrap()
                .failure,
            Some(RealtimePagedGroovePageFailure::SpatialSeamSampleMismatch {
                level_index: 0,
                frame: changed_frame,
            })
        );
        assert_eq!(cache.status().published_pages, 1);
    }

    #[test]
    fn ingestion_progresses_between_reversal_quanta_without_changing_fixed_memory() {
        let first = page(0, SEAM_FRAME);
        let second = page(SEAM_FRAME, TOTAL_FRAMES);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let allocated = cache.status().allocated_resident_bytes;
        let first_ticket = ingest_all(&mut cache, &first);
        finish_and_publish(&mut cache, first_ticket);
        let second_ticket = ingest_all(&mut cache, &second);
        let geometry = StylusGeometry::default();
        let mut position = SEAM_FRAME as f64 - 300.25;
        let mut advance = 20.0;
        for _ in 0..512 {
            let request = PagedGrooveRenderRequest::new(generation(), position, advance).unwrap();
            assert!(matches!(
                cache.trace(request, geometry).unwrap(),
                RealtimePagedGrooveTraceResolution::Ready(_)
            ));
            if cache.slots[second_ticket.slot_index() as usize].phase
                != RealtimePagedGrooveSlotPhase::Ready
            {
                cache.advance_page(second_ticket, 260).unwrap();
            }
            let next = position + advance;
            if next >= SEAM_FRAME as f64 - 40.0 || next <= SEAM_FRAME as f64 - 320.0 {
                advance = -advance;
            }
            position += advance;
            assert_eq!(cache.status().allocated_resident_bytes, allocated);
        }
        while cache.slots[second_ticket.slot_index() as usize].phase
            != RealtimePagedGrooveSlotPhase::Ready
        {
            cache.advance_page(second_ticket, 2_048).unwrap();
        }
        cache.publish_page(second_ticket).unwrap();
    }

    #[test]
    fn eviction_is_blocked_during_seam_work_and_reuses_the_slot() {
        let first = page(0, SEAM_FRAME);
        let second = page(SEAM_FRAME, TOTAL_FRAMES);
        let mut cache = RealtimePagedGrooveCache::new(metadata(), config()).unwrap();
        let first_ticket = ingest_all(&mut cache, &first);
        finish_and_publish(&mut cache, first_ticket);
        let second_ticket = ingest_all(&mut cache, &second);
        assert!(matches!(
            cache.evict_page_containing(100),
            Err(RealtimePagedGrooveError::PageHasStagingDependency { .. })
        ));
        finish_and_publish(&mut cache, second_ticket);
        assert!(cache.evict_page_containing(100).unwrap().is_some());
        assert_eq!(cache.status().published_pages, 1);
        let replacement = page(0, SEAM_FRAME);
        let replacement_ticket = ingest_all(&mut cache, &replacement);
        assert_eq!(replacement_ticket.slot_index(), first_ticket.slot_index());
    }
}
