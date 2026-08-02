use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::groove::GrooveContentIdentity;
use super::stylus::{
    trace_spherical_45_45_wall_multiresolution_contacts_certified_concave,
    trace_spherical_45_45_wall_multiresolution_contacts_certified_piecewise, StylusTraceContactSet,
};
use super::{
    contact::{
        groove_friction_geometry_is_well_conditioned,
        interior_spiral_origin_shift_per_record_velocity_m_s, MidpointPickupGeometry,
        MAX_MIDPOINT_CANDIDATE_BRANCHES, MAX_MIDPOINT_LINEAR_SOLVES,
    },
    electromechanical::{process_coupled_record_player_midpoint, CoupledRecordPlayerStepError},
};
use super::{
    GrooveAsset, GrooveCutReport, GrooveError, GrooveGenerationId, GrooveLayout,
    GrooveTraceAdmissionClass, GrooveTraceAdmissionError, MovingMagnetCartridge,
    MovingMagnetCartridgeSnapshot, MovingMagnetCartridgeTelemetry, PagedGrooveCache,
    PagedGrooveError, PagedGroovePrefetchPlan, PagedGrooveRenderMiss, PagedGrooveRenderRequest,
    PagedGrooveTraceResolution, PhysicalPhonoStage, PhysicalPhonoStageSnapshot,
    PhysicalPhonoStageTelemetry, PhysicalPlaybackConfig, PhysicalProfile, PickupContactSurface,
    PickupMechanicalInput, PickupMechanicalSnapshot, PickupMechanicalState,
    PickupMechanicalTelemetry, RadialContactRegion, RadialTrackingInput, RadialTrackingObservation,
    RadialTrackingSnapshot, RadialTrackingState, RadialTrackingTelemetry, RealtimePagedGrooveCache,
    RealtimePagedGrooveError, RealtimePagedGrooveTraceResolution, RecordCutConfig, StylusGeometry,
    MAX_ABS_PHYSICAL_OUTPUT_SAMPLE, MAX_PAGED_GROOVE_PREFETCH_CANDIDATES,
    MAX_PAGED_GROOVE_RENDER_SPEED,
};
use crate::scratch_gate::{
    ScratchPerformance, ScratchPerformanceError, ScratchPerformanceInput, ScratchPerformanceOutput,
    ScratchPerformanceSnapshot,
};
use crate::spsc::{SpscCommitOutcome, SpscPopError, TimedPlayerControlConsumer};
use crate::timed_control::{
    ControlBlockItem, ControlTimelineAdvanceError, ControlTimelineCheckpointRestoreError,
    ControlTimelineCreateError, ControlTimelinePushError, ControlTimelineRestoreError,
    ControlTimelineSnapshot, PlayerControl, PlayerControlTimeline, PlayerControlTimelineCheckpoint,
    TimedPlayerControl,
};
use crate::{
    DeckMechanicalError, DeckMechanicalSnapshot, DeckMechanicalState, DeckMechanicalTelemetry,
};

const PHYSICAL_RECORD_PLAYER_SNAPSHOT_VERSION: u32 = 11;
const MAX_EXACT_GROOVE_FRAME_COUNT: u64 = 1_u64 << 53;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PhysicalGrooveSourceKind {
    Contiguous,
    Paged,
    RealtimePaged,
}

/// Identifies the loaded representation and its requested page generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalGrooveSourceIdentity {
    pub kind: PhysicalGrooveSourceKind,
    pub canonical_content_identity: GrooveContentIdentity,
    pub trace_admitted_representation_identity: GrooveContentIdentity,
    pub paged_metadata_identity: Option<GrooveContentIdentity>,
    pub cached_generation: Option<GrooveGenerationId>,
    pub requested_generation: Option<GrooveGenerationId>,
}

enum PhysicalGrooveSourceInner {
    Contiguous(Arc<GrooveAsset>),
    Paged {
        cache: Arc<PagedGrooveCache>,
        requested_generation: GrooveGenerationId,
    },
    RealtimePaged {
        cache: Box<RealtimePagedGrooveCache>,
        requested_generation: GrooveGenerationId,
    },
}

/// Owns one immutable contiguous or paged groove representation.
pub struct PhysicalGrooveSource {
    inner: PhysicalGrooveSourceInner,
}

impl std::fmt::Debug for PhysicalGrooveSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PhysicalGrooveSource")
            .field("identity", &self.identity())
            .finish_non_exhaustive()
    }
}

impl PhysicalGrooveSource {
    pub fn contiguous(groove: Arc<GrooveAsset>) -> Self {
        Self {
            inner: PhysicalGrooveSourceInner::Contiguous(groove),
        }
    }

    pub fn paged(cache: Arc<PagedGrooveCache>) -> Self {
        let requested_generation = cache.generation();
        Self {
            inner: PhysicalGrooveSourceInner::Paged {
                cache,
                requested_generation,
            },
        }
    }

    /// Creates an explicit generation request for publication and stale tests.
    pub fn paged_generation(
        cache: Arc<PagedGrooveCache>,
        requested_generation: GrooveGenerationId,
    ) -> Self {
        Self {
            inner: PhysicalGrooveSourceInner::Paged {
                cache,
                requested_generation,
            },
        }
    }

    pub fn realtime_paged(cache: RealtimePagedGrooveCache) -> Self {
        let requested_generation = cache.generation();
        Self {
            inner: PhysicalGrooveSourceInner::RealtimePaged {
                cache: Box::new(cache),
                requested_generation,
            },
        }
    }

    /// Creates an explicit generation request for stale-generation tests.
    pub fn realtime_paged_generation(
        cache: RealtimePagedGrooveCache,
        requested_generation: GrooveGenerationId,
    ) -> Self {
        Self {
            inner: PhysicalGrooveSourceInner::RealtimePaged {
                cache: Box::new(cache),
                requested_generation,
            },
        }
    }

    pub fn identity(&self) -> PhysicalGrooveSourceIdentity {
        match &self.inner {
            PhysicalGrooveSourceInner::Contiguous(groove) => PhysicalGrooveSourceIdentity {
                kind: PhysicalGrooveSourceKind::Contiguous,
                canonical_content_identity: groove.provenance().content_identity(),
                trace_admitted_representation_identity: groove.trace_admission_identity(),
                paged_metadata_identity: None,
                cached_generation: None,
                requested_generation: None,
            },
            PhysicalGrooveSourceInner::Paged {
                cache,
                requested_generation,
            } => PhysicalGrooveSourceIdentity {
                kind: PhysicalGrooveSourceKind::Paged,
                canonical_content_identity: cache.metadata().source_content_identity(),
                trace_admitted_representation_identity: cache
                    .trace_admitted_representation_identity(),
                paged_metadata_identity: Some(cache.content_identity()),
                cached_generation: Some(cache.generation()),
                requested_generation: Some(*requested_generation),
            },
            PhysicalGrooveSourceInner::RealtimePaged {
                cache,
                requested_generation,
            } => PhysicalGrooveSourceIdentity {
                kind: PhysicalGrooveSourceKind::RealtimePaged,
                canonical_content_identity: cache.metadata().source_content_identity(),
                trace_admitted_representation_identity: cache
                    .trace_admitted_representation_identity(),
                paged_metadata_identity: Some(cache.content_identity()),
                cached_generation: Some(cache.generation()),
                requested_generation: Some(*requested_generation),
            },
        }
    }

    pub fn as_contiguous(&self) -> Option<&Arc<GrooveAsset>> {
        match &self.inner {
            PhysicalGrooveSourceInner::Contiguous(groove) => Some(groove),
            PhysicalGrooveSourceInner::Paged { .. }
            | PhysicalGrooveSourceInner::RealtimePaged { .. } => None,
        }
    }

    pub fn as_paged_cache(&self) -> Option<&Arc<PagedGrooveCache>> {
        match &self.inner {
            PhysicalGrooveSourceInner::Contiguous(_) => None,
            PhysicalGrooveSourceInner::Paged { cache, .. } => Some(cache),
            PhysicalGrooveSourceInner::RealtimePaged { .. } => None,
        }
    }

    pub fn as_realtime_paged_cache(&self) -> Option<&RealtimePagedGrooveCache> {
        match &self.inner {
            PhysicalGrooveSourceInner::RealtimePaged { cache, .. } => Some(cache),
            _ => None,
        }
    }

    pub fn as_realtime_paged_cache_mut(&mut self) -> Option<&mut RealtimePagedGrooveCache> {
        match &mut self.inner {
            PhysicalGrooveSourceInner::RealtimePaged { cache, .. } => Some(cache),
            _ => None,
        }
    }

    pub fn into_contiguous(self) -> Option<Arc<GrooveAsset>> {
        match self.inner {
            PhysicalGrooveSourceInner::Contiguous(groove) => Some(groove),
            PhysicalGrooveSourceInner::Paged { .. }
            | PhysicalGrooveSourceInner::RealtimePaged { .. } => None,
        }
    }

    pub fn into_realtime_paged_cache(self) -> Option<RealtimePagedGrooveCache> {
        match self.inner {
            PhysicalGrooveSourceInner::RealtimePaged { cache, .. } => Some(*cache),
            _ => None,
        }
    }

    fn descriptor(&self) -> GrooveSourceDescriptor {
        match &self.inner {
            PhysicalGrooveSourceInner::Contiguous(groove) => GrooveSourceDescriptor {
                layout: groove.layout(),
                cut: groove.provenance().cut(),
                report: groove.report(),
                frame_count: groove.frame_count() as u64,
                canonical_content_identity: groove.provenance().content_identity(),
            },
            PhysicalGrooveSourceInner::Paged { cache, .. } => {
                let metadata = cache.metadata();
                GrooveSourceDescriptor {
                    layout: metadata.cut().layout(),
                    cut: metadata.cut().cut(),
                    report: metadata.cut().report(),
                    frame_count: metadata.total_frame_count(),
                    canonical_content_identity: metadata.source_content_identity(),
                }
            }
            PhysicalGrooveSourceInner::RealtimePaged { cache, .. } => {
                let metadata = cache.metadata();
                GrooveSourceDescriptor {
                    layout: metadata.cut().layout(),
                    cut: metadata.cut().cut(),
                    report: metadata.cut().report(),
                    frame_count: metadata.total_frame_count(),
                    canonical_content_identity: metadata.source_content_identity(),
                }
            }
        }
    }

    fn maximum_certified_absolute_wall_slope(&self) -> f64 {
        match &self.inner {
            PhysicalGrooveSourceInner::Contiguous(groove) => groove
                .trace_admission_certificate()
                .maximum_absolute_wall_slope(),
            PhysicalGrooveSourceInner::Paged { cache, .. } => {
                cache.maximum_certified_absolute_wall_slope()
            }
            PhysicalGrooveSourceInner::RealtimePaged { cache, .. } => {
                cache.maximum_certified_absolute_wall_slope()
            }
        }
    }

    fn trace(
        &self,
        absolute_frame_position: f64,
        source_frame_advance: f64,
        geometry: StylusGeometry,
    ) -> Result<GrooveSourceTrace, PhysicalRecordPlayerError> {
        match &self.inner {
            PhysicalGrooveSourceInner::Contiguous(groove) => {
                let meters_per_source_frame = groove.meters_per_frame_at(absolute_frame_position);
                let selection = groove.spatial_level_selection(source_frame_advance.abs())?;
                let lower = selection.lower();
                let upper = selection.upper();
                let admission = groove.validated_trace_admission()?;
                admission.validate_for_active_tracing(geometry)?;
                let wall_contacts =
                    [0, 1].map(|wall| match admission.certificate().admission_class() {
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
                                absolute_frame_position,
                                meters_per_source_frame,
                                geometry,
                                admission.certified_concave_trace_bounds(geometry)?,
                            )
                            .map_err(PhysicalRecordPlayerError::from)
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
                                absolute_frame_position,
                                meters_per_source_frame,
                                geometry,
                                admission.fixed_cap_piecewise_trace_bounds(geometry)?,
                            )
                            .map_err(PhysicalRecordPlayerError::from)
                        }
                        rejected => Err(PhysicalRecordPlayerError::TraceAdmission(
                            rejected
                                .rejection_error()
                                .expect("non-trace admission classes have a typed rejection"),
                        )),
                    });
                let [left_wall_contacts, right_wall_contacts] = wall_contacts;
                Ok(GrooveSourceTrace {
                    groove_radius_m: groove.radius_at_frame(absolute_frame_position),
                    spatial_filter_lower_step_frames: lower.source_frame_step(),
                    spatial_filter_upper_step_frames: upper.source_frame_step(),
                    spatial_filter_upper_blend: selection.upper_level_blend(),
                    wall_contacts: [left_wall_contacts?, right_wall_contacts?],
                })
            }
            PhysicalGrooveSourceInner::Paged {
                cache,
                requested_generation,
            } => {
                let request = PagedGrooveRenderRequest::new(
                    *requested_generation,
                    absolute_frame_position,
                    source_frame_advance,
                )?;
                match cache.trace(request, geometry)? {
                    PagedGrooveTraceResolution::Ready(trace) => Ok(GrooveSourceTrace {
                        groove_radius_m: trace.groove_radius_m,
                        spatial_filter_lower_step_frames: trace.spatial_filter_lower_step_frames,
                        spatial_filter_upper_step_frames: trace.spatial_filter_upper_step_frames,
                        spatial_filter_upper_blend: trace.spatial_filter_upper_blend,
                        wall_contacts: trace.wall_contacts,
                    }),
                    PagedGrooveTraceResolution::Miss(miss) => {
                        Err(PhysicalRecordPlayerError::PagedGrooveRenderMiss(miss))
                    }
                }
            }
            PhysicalGrooveSourceInner::RealtimePaged {
                cache,
                requested_generation,
            } => {
                let request = PagedGrooveRenderRequest::new(
                    *requested_generation,
                    absolute_frame_position,
                    source_frame_advance,
                )?;
                match cache.trace(request, geometry)? {
                    RealtimePagedGrooveTraceResolution::Ready(trace) => Ok(GrooveSourceTrace {
                        groove_radius_m: trace.groove_radius_m,
                        spatial_filter_lower_step_frames: trace.spatial_filter_lower_step_frames,
                        spatial_filter_upper_step_frames: trace.spatial_filter_upper_step_frames,
                        spatial_filter_upper_blend: trace.spatial_filter_upper_blend,
                        wall_contacts: trace.wall_contacts,
                    }),
                    RealtimePagedGrooveTraceResolution::Miss(miss) => {
                        Err(PhysicalRecordPlayerError::PagedGrooveRenderMiss(miss))
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct GrooveSourceDescriptor {
    layout: GrooveLayout,
    cut: RecordCutConfig,
    report: GrooveCutReport,
    frame_count: u64,
    canonical_content_identity: GrooveContentIdentity,
}

impl GrooveSourceDescriptor {
    fn maximum_frame_position(self) -> f64 {
        self.frame_count.saturating_sub(1) as f64
    }

    fn radius_at_frame(self, frame: f64) -> f64 {
        self.layout
            .radius_at_frame(frame, self.cut.groove_pitch_m_per_revolution)
    }
}

fn validate_source_friction_geometry(
    source: &PhysicalGrooveSource,
    friction_coefficient: f64,
) -> Result<(), PhysicalRecordPlayerError> {
    if groove_friction_geometry_is_well_conditioned(
        friction_coefficient,
        source.maximum_certified_absolute_wall_slope(),
    ) {
        Ok(())
    } else {
        Err(super::PickupMechanicalError::IllConditionedGrooveWallFrictionGeometry.into())
    }
}

#[derive(Debug, Clone, Copy)]
struct GrooveSourceTrace {
    groove_radius_m: f64,
    spatial_filter_lower_step_frames: u32,
    spatial_filter_upper_step_frames: u32,
    spatial_filter_upper_blend: f64,
    wall_contacts: [StylusTraceContactSet; 2],
}

fn transform_wall_contact_set(
    mut contacts: StylusTraceContactSet,
    center_normal_offset_m: f64,
    hold_position: bool,
) -> StylusTraceContactSet {
    contacts.center_displacement_m += center_normal_offset_m;
    if hold_position {
        for contact in &mut contacts.contacts[..usize::from(contacts.contact_count)] {
            contact.groove_slope = 0.0;
        }
    }
    contacts
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalRenderTelemetry {
    pub rendered_internal_frames: usize,
    pub absolute_internal_frame: u64,
    pub spiral_frame_position: f64,
    pub groove_frame_position: f64,
    pub groove_radius_m: f64,
    pub groove_loaded: bool,
    pub at_programme_boundary: bool,
    pub spatial_filter_lower_step_frames: u32,
    pub spatial_filter_upper_step_frames: u32,
    pub spatial_filter_upper_blend: f64,
    pub midpoint_candidate_branches: u32,
    pub midpoint_linear_solves: u32,
    pub phono_output_v: [f64; 2],
    pub phono_input_overload: [bool; 2],
    pub phono_output_overload: [bool; 2],
    pub scratch: ScratchPerformanceOutput,
    pub phono: PhysicalPhonoStageTelemetry,
    pub radial_tracking: RadialTrackingTelemetry,
    pub mechanics: DeckMechanicalTelemetry,
    pub pickup: PickupMechanicalTelemetry,
    pub cartridge: MovingMagnetCartridgeTelemetry,
}

/// Stores all mutable state that affects later physical output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalRecordPlayerSnapshot {
    version: u32,
    config: PhysicalPlaybackConfig,
    loaded_groove_identity: Option<GrooveContentIdentity>,
    loaded_source_identity: Option<PhysicalGrooveSourceIdentity>,
    spiral_frame_position: f64,
    groove_frame_position: f64,
    deck: DeckMechanicalSnapshot,
    controls: ControlTimelineSnapshot,
    pickup: PickupMechanicalSnapshot,
    cartridge: MovingMagnetCartridgeSnapshot,
    phono: PhysicalPhonoStageSnapshot,
    scratch: ScratchPerformanceSnapshot,
    radial_tracking: RadialTrackingSnapshot,
    stylus_torque_nm: f64,
    last_telemetry: PhysicalRenderTelemetry,
}

/// Captures one player's render state without allocation.
///
/// The checkpoint is valid only for the player that created it. Enqueuing a
/// control invalidates it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicalRecordPlayerRenderCheckpoint {
    controls: PlayerControlTimelineCheckpoint,
    deck: DeckMechanicalSnapshot,
    pickup: PickupMechanicalSnapshot,
    cartridge: MovingMagnetCartridgeSnapshot,
    phono: PhysicalPhonoStageSnapshot,
    scratch: ScratchPerformanceSnapshot,
    radial_tracking: RadialTrackingSnapshot,
    spiral_frame_position: f64,
    groove_frame_position: f64,
    stylus_torque_nm: f64,
    last_telemetry: PhysicalRenderTelemetry,
}

/// Reports one bounded drain from the real-time control mailbox.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerControlIngressReport {
    pub inspected_events: usize,
    pub enqueued_events: usize,
    pub retimed_late_events: usize,
    pub rejected_invalid_encodings: usize,
    pub rejected_invalid_controls: usize,
    pub rejected_duplicates: usize,
    pub rejected_nonmonotonic_frames: usize,
    pub rejected_nonmonotonic_sequences: usize,
    pub protocol_errors: usize,
    pub stopped_for_timeline_backpressure: bool,
    pub producer_disconnected: bool,
}

impl PhysicalRecordPlayerSnapshot {
    pub const fn current_internal_frame(&self) -> u64 {
        self.controls.render_frame()
    }

    pub const fn loaded_groove_identity(&self) -> Option<GrooveContentIdentity> {
        self.loaded_groove_identity
    }

    pub const fn loaded_source_identity(&self) -> Option<PhysicalGrooveSourceIdentity> {
        self.loaded_source_identity
    }
}

pub struct PhysicalRecordPlayer {
    profile: PhysicalProfile,
    source: Option<PhysicalGrooveSource>,
    spiral_frame_position: f64,
    groove_frame_position: f64,
    deck: DeckMechanicalState,
    controls: PlayerControlTimeline,
    pickup: PickupMechanicalState,
    cartridge: MovingMagnetCartridge,
    phono: PhysicalPhonoStage,
    scratch: ScratchPerformance,
    radial_tracking: RadialTrackingState,
    stylus_torque_nm: f64,
    control_items: Vec<ControlBlockItem>,
    render_scratch: Vec<f32>,
    last_telemetry: PhysicalRenderTelemetry,
    #[cfg(test)]
    injected_failure_at_completed_step: Option<u64>,
}

impl PhysicalRecordPlayer {
    pub fn new(profile: PhysicalProfile) -> Result<Self, PhysicalRecordPlayerError> {
        profile.validate()?;
        let config = profile.config;
        let deck = DeckMechanicalState::new(config.deck)?;
        let controls = PlayerControlTimeline::new(
            config.solver.control_timeline_capacity,
            0,
            PlayerControl::default(),
        )?;
        let pickup = PickupMechanicalState::new(
            config.contact,
            config.tonearm,
            config.solver.internal_sample_rate_hz,
        )?;
        let cartridge = MovingMagnetCartridge::new(config.cartridge)?;
        let phono = PhysicalPhonoStage::new(
            config.phono,
            config.solver.internal_sample_rate_hz,
            config.record_cut.cutter_bandwidth_hz,
        )?;
        let scratch = ScratchPerformance::default();
        let radial_tracking = RadialTrackingState::new(
            config.radial_tracking,
            config.solver.internal_sample_rate_hz,
            config.groove.outer_program_radius_m,
            config.record_cut.groove_pitch_m_per_revolution,
        )?;
        let control_item_capacity = config
            .solver
            .control_timeline_capacity
            .saturating_mul(2)
            .saturating_add(1);
        let last_telemetry = PhysicalRenderTelemetry {
            rendered_internal_frames: 0,
            absolute_internal_frame: 0,
            spiral_frame_position: 0.0,
            groove_frame_position: 0.0,
            groove_radius_m: config.groove.outer_program_radius_m,
            groove_loaded: false,
            at_programme_boundary: false,
            spatial_filter_lower_step_frames: 1,
            spatial_filter_upper_step_frames: 1,
            spatial_filter_upper_blend: 0.0,
            midpoint_candidate_branches: 0,
            midpoint_linear_solves: 0,
            phono_output_v: [0.0; 2],
            phono_input_overload: [false; 2],
            phono_output_overload: [false; 2],
            scratch: scratch.output(),
            phono: phono.telemetry(),
            radial_tracking: radial_tracking.telemetry(),
            mechanics: deck.telemetry(),
            pickup: pickup.telemetry(),
            cartridge: cartridge.telemetry(),
        };
        Ok(Self {
            profile,
            source: None,
            spiral_frame_position: 0.0,
            groove_frame_position: 0.0,
            deck,
            controls,
            pickup,
            cartridge,
            phono,
            scratch,
            radial_tracking,
            stylus_torque_nm: 0.0,
            control_items: Vec::with_capacity(control_item_capacity),
            render_scratch: vec![0.0; config.solver.maximum_render_frames * 2],
            last_telemetry,
            #[cfg(test)]
            injected_failure_at_completed_step: None,
        })
    }

    pub fn profile(&self) -> &PhysicalProfile {
        &self.profile
    }

    pub fn load_groove(
        &mut self,
        groove: Arc<GrooveAsset>,
    ) -> Result<Option<Arc<GrooveAsset>>, PhysicalRecordPlayerError> {
        Ok(self
            .load_source(PhysicalGrooveSource::contiguous(groove))?
            .and_then(PhysicalGrooveSource::into_contiguous))
    }

    pub fn load_paged_groove(
        &mut self,
        cache: Arc<PagedGrooveCache>,
    ) -> Result<Option<PhysicalGrooveSource>, PhysicalRecordPlayerError> {
        self.load_source(PhysicalGrooveSource::paged(cache))
    }

    /// Loads an explicit requested generation for stale-publication handling.
    pub fn load_paged_groove_generation(
        &mut self,
        cache: Arc<PagedGrooveCache>,
        requested_generation: GrooveGenerationId,
    ) -> Result<Option<PhysicalGrooveSource>, PhysicalRecordPlayerError> {
        self.load_source(PhysicalGrooveSource::paged_generation(
            cache,
            requested_generation,
        ))
    }

    pub fn load_realtime_paged_groove(
        &mut self,
        cache: RealtimePagedGrooveCache,
    ) -> Result<Option<PhysicalGrooveSource>, PhysicalRecordPlayerError> {
        self.load_source(PhysicalGrooveSource::realtime_paged(cache))
    }

    /// Loads an explicit requested generation for stale-generation handling.
    pub fn load_realtime_paged_groove_generation(
        &mut self,
        cache: RealtimePagedGrooveCache,
        requested_generation: GrooveGenerationId,
    ) -> Result<Option<PhysicalGrooveSource>, PhysicalRecordPlayerError> {
        self.load_source(PhysicalGrooveSource::realtime_paged_generation(
            cache,
            requested_generation,
        ))
    }

    pub fn load_source(
        &mut self,
        source: PhysicalGrooveSource,
    ) -> Result<Option<PhysicalGrooveSource>, PhysicalRecordPlayerError> {
        let descriptor = source.descriptor();
        if descriptor.layout != self.profile.config.groove {
            return Err(PhysicalRecordPlayerError::GrooveLayoutMismatch);
        }
        if descriptor.cut != self.profile.config.record_cut {
            return Err(PhysicalRecordPlayerError::GrooveCutMismatch);
        }
        if descriptor.report.programme_exceeds_available_radius {
            return Err(PhysicalRecordPlayerError::GrooveProgrammeDoesNotFit);
        }
        if descriptor.report.adjacent_turn_clearance_failed {
            return Err(PhysicalRecordPlayerError::GrooveOvercut);
        }
        if descriptor.frame_count > MAX_EXACT_GROOVE_FRAME_COUNT {
            return Err(PhysicalRecordPlayerError::GrooveFrameCountNotExactlyRepresentable);
        }
        validate_source_friction_geometry(
            &source,
            self.profile.config.contact.groove_friction_coefficient,
        )?;
        if let Some(groove) = source.as_contiguous() {
            groove
                .validated_trace_admission()?
                .validate_for_active_tracing(self.profile.config.stylus)?;
        }
        if let Some(cache) = source.as_paged_cache() {
            cache
                .metadata()
                .validate_tracing_geometry(self.profile.config.stylus)?;
        }
        if let Some(cache) = source.as_realtime_paged_cache() {
            cache
                .metadata()
                .validate_tracing_geometry(self.profile.config.stylus)?;
        }
        let spiral_frame_position = self
            .spiral_frame_position
            .clamp(0.0, descriptor.maximum_frame_position());
        let radius = descriptor.radius_at_frame(spiral_frame_position);
        let (tip_lateral_position_m, _) = self.pickup.lateral_tip_kinematics();
        self.pickup
            .shift_lateral_coordinate_origin(tip_lateral_position_m)?;
        self.radial_tracking
            .reset(radius, descriptor.cut.groove_pitch_m_per_revolution)?;
        self.spiral_frame_position = spiral_frame_position;
        self.groove_frame_position = spiral_frame_position;
        let previous = self.source.replace(source);
        self.last_telemetry.spiral_frame_position = self.spiral_frame_position;
        self.last_telemetry.groove_frame_position = self.groove_frame_position;
        self.last_telemetry.groove_radius_m = radius;
        self.last_telemetry.groove_loaded = true;
        self.last_telemetry.at_programme_boundary = false;
        self.last_telemetry.spatial_filter_lower_step_frames = 1;
        self.last_telemetry.spatial_filter_upper_step_frames = 1;
        self.last_telemetry.spatial_filter_upper_blend = 0.0;
        self.last_telemetry.radial_tracking = self.radial_tracking.telemetry();
        Ok(previous)
    }

    /// Replaces one page publication with an immutable extension.
    /// The operation keeps player state and changes the loaded representation identity.
    pub fn replace_loaded_paged_cache_snapshot(
        &mut self,
        new_cache: Arc<PagedGrooveCache>,
    ) -> Result<Arc<PagedGrooveCache>, PhysicalRecordPlayerError> {
        new_cache
            .metadata()
            .validate_tracing_geometry(self.profile.config.stylus)?;
        if !groove_friction_geometry_is_well_conditioned(
            self.profile.config.contact.groove_friction_coefficient,
            new_cache.maximum_certified_absolute_wall_slope(),
        ) {
            return Err(
                super::PickupMechanicalError::IllConditionedGrooveWallFrictionGeometry.into(),
            );
        }
        let (old_cache, requested_generation, old_descriptor) = match self.source.as_ref() {
            Some(
                source @ PhysicalGrooveSource {
                    inner:
                        PhysicalGrooveSourceInner::Paged {
                            cache,
                            requested_generation,
                        },
                },
            ) => (cache, *requested_generation, source.descriptor()),
            _ => {
                return Err(PhysicalRecordPlayerError::PagedCacheReplacementRequiresPagedSource);
            }
        };
        let old_metadata = old_cache.metadata();
        let new_metadata = new_cache.metadata();
        let new_descriptor =
            PhysicalGrooveSource::paged_generation(Arc::clone(&new_cache), requested_generation)
                .descriptor();
        if old_metadata.content_identity() != new_metadata.content_identity()
            || old_metadata.source_content_identity() != new_metadata.source_content_identity()
            || old_metadata.generation() != new_metadata.generation()
            || old_descriptor.layout != new_descriptor.layout
            || old_descriptor.cut != new_descriptor.cut
            || old_descriptor.report != new_descriptor.report
            || old_descriptor.frame_count != new_descriptor.frame_count
            || old_descriptor.canonical_content_identity
                != new_descriptor.canonical_content_identity
            || !new_cache.is_immutable_extension_of(old_cache)
        {
            return Err(PhysicalRecordPlayerError::PagedCacheReplacementMismatch);
        }

        match self.source.as_mut() {
            Some(PhysicalGrooveSource {
                inner: PhysicalGrooveSourceInner::Paged { cache, .. },
            }) => Ok(std::mem::replace(cache, new_cache)),
            _ => Err(PhysicalRecordPlayerError::PagedCacheReplacementRequiresPagedSource),
        }
    }

    pub fn unload_groove(&mut self) -> Option<Arc<GrooveAsset>> {
        self.unload_source()
            .and_then(PhysicalGrooveSource::into_contiguous)
    }

    pub fn unload_source(&mut self) -> Option<PhysicalGrooveSource> {
        let previous = self.source.take();
        self.stylus_torque_nm = 0.0;
        self.last_telemetry.groove_radius_m = self.profile.config.groove.outer_program_radius_m;
        self.last_telemetry.groove_loaded = false;
        self.last_telemetry.at_programme_boundary = false;
        self.last_telemetry.spatial_filter_lower_step_frames = 1;
        self.last_telemetry.spatial_filter_upper_step_frames = 1;
        self.last_telemetry.spatial_filter_upper_blend = 0.0;
        previous
    }

    /// Borrows the fixed cache between render calls.
    pub fn realtime_paged_cache(&self) -> Option<&RealtimePagedGrooveCache> {
        self.source
            .as_ref()
            .and_then(PhysicalGrooveSource::as_realtime_paged_cache)
    }

    /// Mutably borrows the fixed cache between render calls.
    pub fn realtime_paged_cache_mut(&mut self) -> Option<&mut RealtimePagedGrooveCache> {
        self.source
            .as_mut()
            .and_then(PhysicalGrooveSource::as_realtime_paged_cache_mut)
    }

    pub fn set_groove_frame_position(
        &mut self,
        position: f64,
    ) -> Result<(), PhysicalRecordPlayerError> {
        if !position.is_finite() || position < 0.0 {
            return Err(PhysicalRecordPlayerError::InvalidGroovePosition);
        }
        let position = self.source.as_ref().map_or(position, |source| {
            position.min(source.descriptor().maximum_frame_position())
        });
        let (radius, pitch) = self.source.as_ref().map_or(
            (
                self.profile.config.groove.outer_program_radius_m,
                self.profile.config.record_cut.groove_pitch_m_per_revolution,
            ),
            |source| {
                let descriptor = source.descriptor();
                (
                    descriptor.radius_at_frame(position),
                    descriptor.cut.groove_pitch_m_per_revolution,
                )
            },
        );
        let (tip_lateral_position_m, _) = self.pickup.lateral_tip_kinematics();
        self.pickup
            .shift_lateral_coordinate_origin(tip_lateral_position_m)?;
        self.radial_tracking.reset(radius, pitch)?;
        self.spiral_frame_position = position;
        self.groove_frame_position = position;
        self.last_telemetry.spiral_frame_position = position;
        self.last_telemetry.groove_frame_position = self.groove_frame_position;
        self.last_telemetry.groove_radius_m = radius;
        self.last_telemetry.at_programme_boundary = false;
        self.last_telemetry.spatial_filter_lower_step_frames = 1;
        self.last_telemetry.spatial_filter_upper_step_frames = 1;
        self.last_telemetry.spatial_filter_upper_blend = 0.0;
        self.last_telemetry.radial_tracking = self.radial_tracking.telemetry();
        Ok(())
    }

    pub fn reset_transport(
        &mut self,
        platter_rate: f64,
        record_rate: f64,
        platter_angle_turns: f64,
        record_angle_turns: f64,
    ) -> Result<(), PhysicalRecordPlayerError> {
        self.deck.reset(
            platter_rate,
            record_rate,
            platter_angle_turns,
            record_angle_turns,
        )?;
        self.last_telemetry.mechanics = self.deck.telemetry();
        Ok(())
    }

    pub fn enqueue_control(
        &mut self,
        event: TimedPlayerControl,
    ) -> Result<(), PhysicalRecordPlayerError> {
        event
            .control
            .deck
            .validate_for_config(self.profile.config.deck)?;
        self.controls.enqueue(event)?;
        Ok(())
    }

    /// Moves available mailbox controls into the sample timeline.
    ///
    /// The function leaves the mailbox head in place when the timeline is full.
    /// A late event moves to the current frame because past output cannot change.
    pub fn drain_control_ingress(
        &mut self,
        consumer: &mut TimedPlayerControlConsumer,
    ) -> PlayerControlIngressReport {
        let mut report = PlayerControlIngressReport::default();
        loop {
            if self.controls.is_full() {
                report.stopped_for_timeline_backpressure = true;
                break;
            }
            let mut event = match consumer.try_peek() {
                Ok(event) => event,
                Err(SpscPopError::Empty) => break,
                Err(SpscPopError::Disconnected) => {
                    report.producer_disconnected = true;
                    break;
                }
                Err(SpscPopError::InvalidEncoding) => {
                    report.inspected_events = report.inspected_events.saturating_add(1);
                    match consumer.commit_peeked() {
                        Ok(SpscCommitOutcome::InvalidEncoding) => {
                            report.rejected_invalid_encodings =
                                report.rejected_invalid_encodings.saturating_add(1);
                        }
                        _ => {
                            report.protocol_errors = report.protocol_errors.saturating_add(1);
                            break;
                        }
                    }
                    continue;
                }
            };
            report.inspected_events = report.inspected_events.saturating_add(1);
            if event.absolute_frame < self.controls.render_frame() {
                event.absolute_frame = self.controls.render_frame();
                report.retimed_late_events = report.retimed_late_events.saturating_add(1);
            }

            let deck_control_is_valid = event
                .control
                .deck
                .validate_for_config(self.profile.config.deck)
                .is_ok();
            let enqueue_result = deck_control_is_valid.then(|| self.controls.enqueue(event));
            match consumer.commit_peeked() {
                Ok(SpscCommitOutcome::Value) => {}
                _ => {
                    report.protocol_errors = report.protocol_errors.saturating_add(1);
                    break;
                }
            }
            match enqueue_result {
                None | Some(Err(ControlTimelinePushError::InvalidControl(_))) => {
                    report.rejected_invalid_controls =
                        report.rejected_invalid_controls.saturating_add(1);
                }
                Some(Ok(())) => {
                    report.enqueued_events = report.enqueued_events.saturating_add(1);
                }
                Some(Err(ControlTimelinePushError::Duplicate { .. })) => {
                    report.rejected_duplicates = report.rejected_duplicates.saturating_add(1);
                }
                Some(Err(ControlTimelinePushError::NonMonotonicFrame { .. })) => {
                    report.rejected_nonmonotonic_frames =
                        report.rejected_nonmonotonic_frames.saturating_add(1);
                }
                Some(Err(ControlTimelinePushError::NonMonotonicSequence { .. })) => {
                    report.rejected_nonmonotonic_sequences =
                        report.rejected_nonmonotonic_sequences.saturating_add(1);
                }
                Some(Err(ControlTimelinePushError::Late { .. }))
                | Some(Err(ControlTimelinePushError::Full { .. })) => {
                    report.protocol_errors = report.protocol_errors.saturating_add(1);
                }
            }
        }
        report
    }

    pub fn current_internal_frame(&self) -> u64 {
        self.controls.render_frame()
    }

    pub fn groove_frame_position(&self) -> f64 {
        self.groove_frame_position
    }

    pub fn spiral_frame_position(&self) -> f64 {
        self.spiral_frame_position
    }

    pub fn deck_state(&self) -> &DeckMechanicalState {
        &self.deck
    }

    pub fn telemetry(&self) -> PhysicalRenderTelemetry {
        self.last_telemetry
    }

    pub fn loaded_groove_identity(&self) -> Option<GrooveContentIdentity> {
        self.source
            .as_ref()
            .map(|source| source.descriptor().canonical_content_identity)
    }

    pub fn loaded_source_identity(&self) -> Option<PhysicalGrooveSourceIdentity> {
        self.source.as_ref().map(PhysicalGrooveSource::identity)
    }

    /// Plans bounded page coverage outside the render thread.
    ///
    /// Each turn offset identifies a groove that the radial tracker can capture.
    /// The plan covers the current source, projected travel, and both directions.
    pub fn paged_prefetch_plan(
        &self,
        render_frame_horizon: u32,
        adjacent_turn_offsets: &[i64],
    ) -> Result<Option<PagedGroovePrefetchPlan>, PhysicalRecordPlayerError> {
        let Some(source) = self.source.as_ref() else {
            return Ok(None);
        };
        let (metadata, requested_generation, cached_generation) = match &source.inner {
            PhysicalGrooveSourceInner::Paged {
                cache,
                requested_generation,
            } => (cache.metadata(), *requested_generation, cache.generation()),
            PhysicalGrooveSourceInner::RealtimePaged {
                cache,
                requested_generation,
            } => (cache.metadata(), *requested_generation, cache.generation()),
            PhysicalGrooveSourceInner::Contiguous(_) => return Ok(None),
        };
        if requested_generation != cached_generation {
            return Err(PhysicalRecordPlayerError::PagedGrooveRenderMiss(
                PagedGrooveRenderMiss::StaleGeneration {
                    requested_generation,
                    cached_generation,
                },
            ));
        }
        if adjacent_turn_offsets.len() >= MAX_PAGED_GROOVE_PREFETCH_CANDIDATES {
            return Err(PagedGrooveError::TooManyPrefetchCandidates {
                maximum_candidates: MAX_PAGED_GROOVE_PREFETCH_CANDIDATES,
            }
            .into());
        }
        if adjacent_turn_offsets.iter().any(|offset| {
            offset.unsigned_abs()
                > u64::from(
                    self.profile
                        .config
                        .radial_tracking
                        .maximum_turns_per_recapture,
                )
        }) {
            return Err(PhysicalRecordPlayerError::InvalidRadialSelection);
        }

        let maximum = metadata.total_frame_count().saturating_sub(1) as f64;
        let current = self.groove_frame_position.clamp(0.0, maximum);
        let signed_rate = self.deck.telemetry().record_rate.clamp(
            -MAX_PAGED_GROOVE_RENDER_SPEED,
            MAX_PAGED_GROOVE_RENDER_SPEED,
        );
        let projected =
            (current + signed_rate * f64::from(render_frame_horizon)).clamp(0.0, maximum);
        let frames_per_turn = self.profile.config.groove.groove_sample_rate_hz * 60.0
            / self.profile.config.groove.nominal_rpm;
        let mut candidates = Vec::with_capacity(adjacent_turn_offsets.len().saturating_add(1));
        if projected != current {
            candidates.push(projected);
        }
        for &turn_offset in adjacent_turn_offsets {
            let candidate = self.spiral_frame_position + turn_offset as f64 * frames_per_turn;
            if (0.0..=maximum).contains(&candidate) {
                candidates.push(candidate);
            }
        }
        let plan = match &source.inner {
            PhysicalGrooveSourceInner::Paged { cache, .. } => {
                cache.bidirectional_prefetch_plan(current, render_frame_horizon, &candidates)?
            }
            PhysicalGrooveSourceInner::RealtimePaged { cache, .. } => {
                cache.bidirectional_prefetch_plan(current, render_frame_horizon, &candidates)?
            }
            PhysicalGrooveSourceInner::Contiguous(_) => unreachable!(),
        };
        Ok(Some(plan))
    }

    #[cfg(test)]
    pub(crate) fn inject_render_failure_at_completed_step(&mut self, completed_step: u64) {
        self.injected_failure_at_completed_step = Some(completed_step);
    }

    pub fn snapshot(&self) -> PhysicalRecordPlayerSnapshot {
        PhysicalRecordPlayerSnapshot {
            version: PHYSICAL_RECORD_PLAYER_SNAPSHOT_VERSION,
            config: self.profile.config,
            loaded_groove_identity: self.loaded_groove_identity(),
            loaded_source_identity: self.loaded_source_identity(),
            spiral_frame_position: self.spiral_frame_position,
            groove_frame_position: self.groove_frame_position,
            deck: self.deck.snapshot(),
            controls: self.controls.snapshot(),
            pickup: self.pickup.snapshot(),
            cartridge: self.cartridge.snapshot(),
            phono: self.phono.snapshot(),
            scratch: self.scratch.snapshot(),
            radial_tracking: self.radial_tracking.snapshot(),
            stylus_torque_nm: self.stylus_torque_nm,
            last_telemetry: self.last_telemetry,
        }
    }

    pub fn render_checkpoint(&self) -> PhysicalRecordPlayerRenderCheckpoint {
        PhysicalRecordPlayerRenderCheckpoint {
            controls: self.controls.checkpoint(),
            deck: self.deck.snapshot(),
            pickup: self.pickup.snapshot(),
            cartridge: self.cartridge.snapshot(),
            phono: self.phono.snapshot(),
            scratch: self.scratch.snapshot(),
            radial_tracking: self.radial_tracking.snapshot(),
            spiral_frame_position: self.spiral_frame_position,
            groove_frame_position: self.groove_frame_position,
            stylus_torque_nm: self.stylus_torque_nm,
            last_telemetry: self.last_telemetry,
        }
    }

    pub fn restore_render_checkpoint(
        &mut self,
        checkpoint: PhysicalRecordPlayerRenderCheckpoint,
    ) -> Result<(), PhysicalRecordPlayerError> {
        self.controls.restore_checkpoint(checkpoint.controls)?;
        self.deck.restore(checkpoint.deck)?;
        self.pickup.restore(checkpoint.pickup)?;
        self.cartridge.restore(checkpoint.cartridge)?;
        self.phono.restore(&checkpoint.phono)?;
        self.scratch.restore(&checkpoint.scratch)?;
        self.radial_tracking.restore(checkpoint.radial_tracking)?;
        self.spiral_frame_position = checkpoint.spiral_frame_position;
        self.groove_frame_position = checkpoint.groove_frame_position;
        self.stylus_torque_nm = checkpoint.stylus_torque_nm;
        self.last_telemetry = checkpoint.last_telemetry;
        Ok(())
    }

    /// Restores a validated snapshot without changing the loaded groove asset.
    pub fn restore(
        &mut self,
        snapshot: &PhysicalRecordPlayerSnapshot,
    ) -> Result<(), PhysicalRecordPlayerError> {
        if snapshot.version != PHYSICAL_RECORD_PLAYER_SNAPSHOT_VERSION {
            return Err(PhysicalRecordPlayerError::UnsupportedSnapshotVersion {
                version: snapshot.version,
            });
        }
        if snapshot.config != self.profile.config {
            return Err(PhysicalRecordPlayerError::SnapshotProfileMismatch);
        }
        if snapshot.loaded_groove_identity != self.loaded_groove_identity() {
            return Err(PhysicalRecordPlayerError::SnapshotGrooveMismatch);
        }
        if snapshot.loaded_source_identity != self.loaded_source_identity()
            || snapshot
                .loaded_source_identity
                .map(|identity| identity.canonical_content_identity)
                != snapshot.loaded_groove_identity
        {
            return Err(PhysicalRecordPlayerError::SnapshotSourceMismatch);
        }
        if !snapshot.spiral_frame_position.is_finite()
            || snapshot.spiral_frame_position < 0.0
            || !snapshot.groove_frame_position.is_finite()
            || snapshot.groove_frame_position < 0.0
            || !snapshot.stylus_torque_nm.is_finite()
        {
            return Err(PhysicalRecordPlayerError::InvalidSnapshot);
        }
        if let Some(source) = &self.source {
            let maximum = source.descriptor().maximum_frame_position();
            if snapshot.spiral_frame_position > maximum || snapshot.groove_frame_position > maximum
            {
                return Err(PhysicalRecordPlayerError::InvalidSnapshot);
            }
        }

        // Build and validate all replacement components before changing self.
        let config = self.profile.config;
        let mut deck = DeckMechanicalState::new(config.deck)?;
        deck.restore(snapshot.deck)?;
        if deck.config() != config.deck {
            return Err(PhysicalRecordPlayerError::SnapshotProfileMismatch);
        }
        let controls = PlayerControlTimeline::from_snapshot(&snapshot.controls)?;
        if controls.capacity() != config.solver.control_timeline_capacity {
            return Err(PhysicalRecordPlayerError::SnapshotProfileMismatch);
        }
        let mut pickup = PickupMechanicalState::new(
            config.contact,
            config.tonearm,
            config.solver.internal_sample_rate_hz,
        )?;
        pickup.restore(snapshot.pickup)?;
        if pickup.contact_config() != config.contact
            || pickup.tonearm_config() != config.tonearm
            || pickup.sample_rate_hz() != config.solver.internal_sample_rate_hz
        {
            return Err(PhysicalRecordPlayerError::SnapshotProfileMismatch);
        }
        let mut cartridge = MovingMagnetCartridge::new(config.cartridge)?;
        cartridge.restore(snapshot.cartridge)?;
        if cartridge.config() != config.cartridge {
            return Err(PhysicalRecordPlayerError::SnapshotProfileMismatch);
        }
        let phono = PhysicalPhonoStage::from_snapshot(&snapshot.phono)?;
        if phono.config() != config.phono
            || phono.sample_rate_hz() != config.solver.internal_sample_rate_hz
            || phono.cutter_bandwidth_hz() != config.record_cut.cutter_bandwidth_hz
        {
            return Err(PhysicalRecordPlayerError::SnapshotProfileMismatch);
        }
        let mut scratch = ScratchPerformance::default();
        scratch.restore(&snapshot.scratch)?;
        let mut radial_tracking = RadialTrackingState::new(
            config.radial_tracking,
            config.solver.internal_sample_rate_hz,
            config.groove.outer_program_radius_m,
            config.record_cut.groove_pitch_m_per_revolution,
        )?;
        radial_tracking.restore(snapshot.radial_tracking)?;
        if radial_tracking.config() != config.radial_tracking
            || radial_tracking.sample_rate_hz() != config.solver.internal_sample_rate_hz
            || radial_tracking.telemetry().groove_pitch_m_per_revolution
                != config.record_cut.groove_pitch_m_per_revolution
        {
            return Err(PhysicalRecordPlayerError::SnapshotProfileMismatch);
        }
        if let Some(source) = &self.source {
            let descriptor = source.descriptor();
            let frames_per_turn =
                config.groove.groove_sample_rate_hz * 60.0 / config.groove.nominal_rpm;
            let radial_telemetry = radial_tracking.telemetry();
            let selected_position = if radial_telemetry.groove_contact {
                snapshot.spiral_frame_position
                    + radial_telemetry.source_turn_offset() as f64 * frames_per_turn
            } else {
                snapshot.spiral_frame_position
            };
            if !(0.0..=descriptor.maximum_frame_position()).contains(&selected_position) {
                return Err(PhysicalRecordPlayerError::InvalidSnapshot);
            }
            if snapshot.groove_frame_position != selected_position {
                return Err(PhysicalRecordPlayerError::InvalidSnapshot);
            }
        } else if snapshot.groove_frame_position != snapshot.spiral_frame_position {
            return Err(PhysicalRecordPlayerError::InvalidSnapshot);
        }
        let expected_stylus_torque =
            if snapshot.loaded_groove_identity.is_some() && pickup.telemetry().stylus_lowered {
                pickup.telemetry().record_reaction_torque_nm()
            } else {
                0.0
            };
        if snapshot.stylus_torque_nm != expected_stylus_torque {
            return Err(PhysicalRecordPlayerError::InvalidSnapshot);
        }
        if !render_telemetry_matches_snapshot(
            snapshot.last_telemetry,
            snapshot.spiral_frame_position,
            snapshot.groove_frame_position,
            snapshot.loaded_groove_identity.is_some(),
            &deck,
            &controls,
            &pickup,
            &cartridge,
            &phono,
            &scratch,
            &radial_tracking,
            config,
            self.source.as_ref(),
        ) {
            return Err(PhysicalRecordPlayerError::InvalidSnapshot);
        }

        self.spiral_frame_position = snapshot.spiral_frame_position;
        self.groove_frame_position = snapshot.groove_frame_position;
        self.deck = deck;
        self.controls = controls;
        self.pickup = pickup;
        self.cartridge = cartridge;
        self.phono = phono;
        self.scratch = scratch;
        self.radial_tracking = radial_tracking;
        self.stylus_torque_nm = snapshot.stylus_torque_nm;
        self.control_items.clear();
        self.last_telemetry = snapshot.last_telemetry;
        Ok(())
    }

    /// Renders stereo voltage at the fixed physical processing rate.
    pub fn render_internal_interleaved(
        &mut self,
        output_v: &mut [f32],
    ) -> Result<PhysicalRenderTelemetry, PhysicalRecordPlayerError> {
        if !output_v.len().is_multiple_of(2) {
            return Err(PhysicalRecordPlayerError::OutputMustBeStereo);
        }
        let frame_count = output_v.len() / 2;
        if frame_count > self.profile.config.solver.maximum_render_frames {
            return Err(PhysicalRecordPlayerError::RenderBlockTooLarge {
                maximum: self.profile.config.solver.maximum_render_frames,
            });
        }
        let frame_count_u32 = u32::try_from(frame_count).map_err(|_| {
            PhysicalRecordPlayerError::RenderBlockTooLarge {
                maximum: self.profile.config.solver.maximum_render_frames,
            }
        })?;
        if let Some(source) = &self.source {
            validate_source_friction_geometry(
                source,
                self.profile.config.contact.groove_friction_coefficient,
            )?;
        }
        let render_checkpoint = self.render_checkpoint();

        self.control_items.clear();
        let control_items = &mut self.control_items;
        self.controls
            .visit_block(frame_count_u32, |item| control_items.push(item))?;

        let render_result = (|| {
            for item_index in 0..self.control_items.len() {
                let item = self.control_items[item_index];
                let ControlBlockItem::Span {
                    frame_offset,
                    frame_count,
                    control,
                } = item
                else {
                    continue;
                };
                for local_frame in 0..frame_count as usize {
                    let frame = frame_offset as usize + local_frame;
                    let voltage = self.process_internal_sample(control)?;
                    let voltage_f32 = [voltage[0] as f32, voltage[1] as f32];
                    if voltage_f32.iter().any(|sample| {
                        !sample.is_finite() || sample.abs() > MAX_ABS_PHYSICAL_OUTPUT_SAMPLE
                    }) {
                        return Err(PhysicalRecordPlayerError::NonfiniteOutput);
                    }
                    self.render_scratch[frame * 2] = voltage_f32[0];
                    self.render_scratch[frame * 2 + 1] = voltage_f32[1];
                }
            }
            Ok(())
        })();
        if let Err(error) = render_result {
            self.restore_render_checkpoint(render_checkpoint)?;
            return Err(error);
        }
        output_v.copy_from_slice(&self.render_scratch[..output_v.len()]);
        self.last_telemetry.rendered_internal_frames = frame_count;
        self.last_telemetry.absolute_internal_frame = self.controls.render_frame();
        Ok(self.last_telemetry)
    }

    fn process_internal_sample(
        &mut self,
        mut control: PlayerControl,
    ) -> Result<[f64; 2], PhysicalRecordPlayerError> {
        #[cfg(test)]
        if self.injected_failure_at_completed_step == Some(self.pickup.telemetry().completed_steps)
        {
            return Err(PhysicalRecordPlayerError::InjectedTestFailure);
        }
        let config = self.profile.config;
        let dt = 1.0 / config.solver.internal_sample_rate_hz;
        control.deck.stylus_torque_nm = 0.0;
        let previous_mechanics = self.deck.telemetry();
        let previous_record_turns = previous_mechanics.record_angle_turns;
        let previous_record_velocity_rad_s =
            previous_mechanics.record_rate * config.groove.nominal_angular_velocity_rad_s();
        let frames_per_turn =
            config.groove.groove_sample_rate_hz * 60.0 / config.groove.nominal_rpm;
        let predicted_midpoint_delta_turns =
            0.5 * previous_record_velocity_rad_s * dt / std::f64::consts::TAU;
        let previous_spiral_frame_position = self.spiral_frame_position;

        let mut groove_loaded = false;
        let mut at_programme_boundary = false;
        let mut groove_radius_m = config.groove.outer_program_radius_m;
        let mut radial_tracking = self.radial_tracking.telemetry();
        let mut wall_contacts = [StylusTraceContactSet::default(); 2];
        let mut spatial_filter_lower_step_frames = 1;
        let mut spatial_filter_upper_step_frames = 1;
        let mut spatial_filter_upper_blend = 0.0;
        let mut contact_surface = PickupContactSurface::None;
        let mut radial_preparation = None;
        let mut lateral_origin_shift_bias_m = 0.0;
        let mut lateral_origin_shift_per_record_velocity_m_s = 0.0;
        if let Some(source) = &self.source {
            let descriptor = source.descriptor();
            let maximum = descriptor.maximum_frame_position();
            let unclamped_midpoint_position =
                previous_spiral_frame_position + predicted_midpoint_delta_turns * frames_per_turn;
            let midpoint_spiral_position = unclamped_midpoint_position.clamp(0.0, maximum);
            let midpoint_at_boundary = midpoint_spiral_position != unclamped_midpoint_position;
            let applied_midpoint_delta_turns =
                (midpoint_spiral_position - previous_spiral_frame_position) / frames_per_turn;
            let (minimum_available_turn_index, maximum_available_turn_index) =
                available_turn_index_bounds(midpoint_spiral_position, maximum, frames_per_turn)?;
            let (stylus_lateral_position_m, stylus_radial_velocity_m_s) =
                self.pickup.lateral_tip_kinematics();
            let preparation = self.radial_tracking.prepare_midpoint(RadialTrackingInput {
                record_angle_delta_turns: applied_midpoint_delta_turns,
                groove_pitch_m_per_revolution: descriptor.cut.groove_pitch_m_per_revolution,
                groove_top_width_m: config.record_cut.groove_top_width_m,
                stylus_lateral_position_m,
                stylus_radial_velocity_m_s,
                stylus_lowered: control.stylus_lowered,
                minimum_available_turn_index,
                maximum_available_turn_index,
            })?;
            let selection = preparation.selection();
            if midpoint_at_boundary {
                let fixed_boundary_delta_turns =
                    (midpoint_spiral_position - previous_spiral_frame_position) / frames_per_turn;
                lateral_origin_shift_bias_m =
                    -descriptor.cut.groove_pitch_m_per_revolution * fixed_boundary_delta_turns;
            } else {
                lateral_origin_shift_bias_m = -descriptor.cut.groove_pitch_m_per_revolution
                    * 0.5
                    * previous_record_velocity_rad_s
                    * dt
                    / std::f64::consts::TAU;
                lateral_origin_shift_per_record_velocity_m_s =
                    interior_spiral_origin_shift_per_record_velocity_m_s(
                        descriptor.cut.groove_pitch_m_per_revolution,
                        dt,
                    );
            }
            match selection.contact_region {
                RadialContactRegion::Groove => {
                    let source_turn_offset = selection
                        .source_turn_offset()
                        .ok_or(PhysicalRecordPlayerError::InvalidRadialSelection)?;
                    let selected_position =
                        midpoint_spiral_position + source_turn_offset as f64 * frames_per_turn;
                    if !(0.0..=maximum).contains(&selected_position) {
                        return Err(PhysicalRecordPlayerError::InvalidRadialSelection);
                    }
                    let source_frame_advance =
                        2.0 * (midpoint_spiral_position - previous_spiral_frame_position);
                    let trace =
                        source.trace(selected_position, source_frame_advance, config.stylus)?;
                    groove_radius_m = trace.groove_radius_m;
                    spatial_filter_lower_step_frames = trace.spatial_filter_lower_step_frames;
                    spatial_filter_upper_step_frames = trace.spatial_filter_upper_step_frames;
                    spatial_filter_upper_blend = trace.spatial_filter_upper_blend;
                    for wall in 0..2 {
                        let center_normal_offset_m =
                            groove_wall_center_offsets(selection.groove_center_lateral_m)[wall];
                        wall_contacts[wall] = transform_wall_contact_set(
                            trace.wall_contacts[wall],
                            center_normal_offset_m,
                            midpoint_at_boundary,
                        );
                    }
                    contact_surface = PickupContactSurface::GrooveWalls;
                }
                RadialContactRegion::Land => {
                    contact_surface = PickupContactSurface::RecordLand;
                }
                RadialContactRegion::Lifted => {}
            }
            if selection.contact_region != RadialContactRegion::Groove {
                groove_radius_m = descriptor.radius_at_frame(midpoint_spiral_position);
            }
            radial_preparation = Some(preparation);
            groove_loaded = true;
        }

        let previous_tangential_mode = self.pickup.telemetry().tangential_mode;
        let coupled = process_coupled_record_player_midpoint(
            &mut self.deck,
            &mut self.pickup,
            &mut self.cartridge,
            dt,
            control.deck,
            MidpointPickupGeometry {
                input: PickupMechanicalInput {
                    wall_contacts,
                    wall_contact_qualification: Default::default(),
                    land_displacement_m: config.record_cut.groove_top_width_m * 0.5,
                    contact_surface,
                    groove_radius_m,
                    groove_tangential_velocity_m_s: previous_record_velocity_rad_s
                        * groove_radius_m,
                    stylus_lowered: groove_loaded && control.stylus_lowered,
                    electromagnetic_force_n: [0.0; 2],
                },
                lateral_origin_shift_bias_m,
                lateral_origin_shift_per_record_velocity_m_s,
            },
            previous_tangential_mode,
        )?;
        let mechanics = coupled.mechanics;
        let pickup = coupled.pickup;
        let cartridge = coupled.cartridge;
        let delta_record_turns = mechanics.record_angle_turns - previous_record_turns;
        let unclamped_spiral_position =
            previous_spiral_frame_position + delta_record_turns * frames_per_turn;
        if let (Some(source), Some(preparation)) = (&self.source, radial_preparation) {
            let maximum = source.descriptor().maximum_frame_position();
            self.spiral_frame_position = unclamped_spiral_position.clamp(0.0, maximum);
            at_programme_boundary = self.spiral_frame_position != unclamped_spiral_position;
            let applied_record_delta_turns =
                (self.spiral_frame_position - previous_spiral_frame_position) / frames_per_turn;
            let selection = self
                .radial_tracking
                .commit_midpoint(preparation, applied_record_delta_turns)?;
            self.groove_frame_position = match selection.source_turn_offset() {
                Some(source_turn_offset) => {
                    let selected_position =
                        self.spiral_frame_position + source_turn_offset as f64 * frames_per_turn;
                    if !(0.0..=maximum).contains(&selected_position) {
                        return Err(PhysicalRecordPlayerError::InvalidRadialSelection);
                    }
                    selected_position
                }
                None => self.spiral_frame_position,
            };
            radial_tracking = self.radial_tracking.observe_pickup(
                selection,
                RadialTrackingObservation {
                    stylus_lateral_position_m: pickup.tip_displacement_m[0],
                    stylus_radial_velocity_m_s: pickup.tip_velocity_m_s[0],
                    groove_lateral_force_on_tip_n: pickup.groove_lateral_force_on_tip_n,
                    bearing_friction_force_n: pickup.bearing_friction_force_n,
                    anti_skate_force_n: config.tonearm.anti_skate_force_n,
                    skating_force_n: pickup.skating_force_n,
                    physical_surface_contact: pickup.wall_contact.into_iter().any(|value| value)
                        || pickup.land_contact,
                    land_contact: pickup.land_contact,
                },
            )?;
            groove_radius_m = if radial_tracking.groove_contact {
                source
                    .descriptor()
                    .radius_at_frame(self.groove_frame_position)
            } else {
                radial_tracking
                    .captured_groove_radius_m
                    .unwrap_or(radial_tracking.spiral_reference_radius_m)
            };
        } else {
            self.spiral_frame_position = unclamped_spiral_position.max(0.0);
            self.groove_frame_position = self.spiral_frame_position;
        }
        if groove_loaded && control.stylus_lowered && contact_surface != PickupContactSurface::None
        {
            self.stylus_torque_nm = pickup.record_reaction_torque_nm();
        } else {
            self.stylus_torque_nm = 0.0;
        }

        let phono = self.phono.process_frame(cartridge.load_output_voltage_v)?;
        if self.scratch.preset() != control.scratch_preset {
            self.scratch.set_preset(control.scratch_preset);
        }
        if self.scratch.clicks() != control.scratch_clicks {
            self.scratch.set_clicks(control.scratch_clicks);
        }
        let nominal_angular_velocity_rad_s = config.deck.nominal_angular_velocity_rad_s();
        let rendered_source_travel_seconds =
            delta_record_turns * std::f64::consts::TAU / nominal_angular_velocity_rad_s;
        let rendered_record_rate = rendered_source_travel_seconds / dt;
        let intent_record_rate =
            control.deck.hand_target_angular_velocity_rad_s / nominal_angular_velocity_rad_s;
        let scratch = self.scratch.process_frame(ScratchPerformanceInput {
            delta_seconds: dt,
            hand_contact: control.deck.hand_contact,
            intent_record_rate,
            rendered_record_rate,
            rendered_source_travel_seconds,
            manual_crossfader_gain: control.manual_crossfader_gain,
        })?;
        let phono_output_v = phono.output_v.map(|sample| sample * scratch.audible_gain);
        let phono_input_overload = phono.input_overload;
        let phono_output_overload = phono.output_overload;
        self.last_telemetry = PhysicalRenderTelemetry {
            rendered_internal_frames: self.last_telemetry.rendered_internal_frames,
            absolute_internal_frame: self.last_telemetry.absolute_internal_frame,
            spiral_frame_position: self.spiral_frame_position,
            groove_frame_position: self.groove_frame_position,
            groove_radius_m,
            groove_loaded,
            at_programme_boundary,
            spatial_filter_lower_step_frames,
            spatial_filter_upper_step_frames,
            spatial_filter_upper_blend,
            midpoint_candidate_branches: coupled.evaluated_branches,
            midpoint_linear_solves: coupled.attempted_linear_solves,
            phono_output_v,
            phono_input_overload,
            phono_output_overload,
            scratch,
            phono,
            radial_tracking,
            mechanics,
            pickup,
            cartridge,
        };
        Ok(phono_output_v)
    }
}

fn groove_wall_center_offsets(center_lateral_m: f64) -> [f64; 2] {
    let offset = center_lateral_m * std::f64::consts::FRAC_1_SQRT_2;
    [offset, -offset]
}

fn available_turn_index_bounds(
    spiral_frame_position: f64,
    maximum_frame_position: f64,
    frames_per_turn: f64,
) -> Result<(i64, i64), PhysicalRecordPlayerError> {
    if !spiral_frame_position.is_finite()
        || !maximum_frame_position.is_finite()
        || !frames_per_turn.is_finite()
        || spiral_frame_position < 0.0
        || maximum_frame_position < spiral_frame_position
        || frames_per_turn <= 0.0
    {
        return Err(PhysicalRecordPlayerError::InvalidRadialSelection);
    }
    let minimum = ((spiral_frame_position - maximum_frame_position) / frames_per_turn).ceil();
    let maximum = (spiral_frame_position / frames_per_turn).floor();
    if minimum < i64::MIN as f64
        || minimum > i64::MAX as f64
        || maximum < i64::MIN as f64
        || maximum > i64::MAX as f64
    {
        return Err(PhysicalRecordPlayerError::InvalidRadialSelection);
    }
    Ok((minimum as i64, maximum as i64))
}

#[allow(clippy::too_many_arguments)]
fn render_telemetry_matches_snapshot(
    telemetry: PhysicalRenderTelemetry,
    spiral_frame_position: f64,
    groove_frame_position: f64,
    groove_loaded: bool,
    deck: &DeckMechanicalState,
    controls: &PlayerControlTimeline,
    pickup: &PickupMechanicalState,
    cartridge: &MovingMagnetCartridge,
    phono: &PhysicalPhonoStage,
    scratch: &ScratchPerformance,
    radial_tracking: &RadialTrackingState,
    config: PhysicalPlaybackConfig,
    source: Option<&PhysicalGrooveSource>,
) -> bool {
    let radial_telemetry = radial_tracking.telemetry();
    let expected_radius = if let Some(source) = source {
        if radial_telemetry.groove_contact {
            source.descriptor().radius_at_frame(groove_frame_position)
        } else {
            radial_telemetry
                .captured_groove_radius_m
                .unwrap_or(radial_telemetry.spiral_reference_radius_m)
        }
    } else {
        config.groove.outer_program_radius_m
    };
    telemetry.rendered_internal_frames <= config.solver.maximum_render_frames
        && telemetry.absolute_internal_frame == controls.render_frame()
        && telemetry.spiral_frame_position == spiral_frame_position
        && telemetry.groove_frame_position == groove_frame_position
        && telemetry.groove_radius_m == expected_radius
        && telemetry.groove_loaded == groove_loaded
        && telemetry.spatial_filter_lower_step_frames > 0
        && telemetry.spatial_filter_upper_step_frames > 0
        && (0.0..=1.0).contains(&telemetry.spatial_filter_upper_blend)
        && telemetry.midpoint_candidate_branches <= MAX_MIDPOINT_CANDIDATE_BRANCHES
        && telemetry.midpoint_linear_solves <= MAX_MIDPOINT_LINEAR_SOLVES
        && telemetry
            .phono_output_v
            .iter()
            .all(|value| value.is_finite())
        && telemetry.mechanics == deck.telemetry()
        && telemetry.pickup == pickup.telemetry()
        && telemetry.cartridge == cartridge.telemetry()
        && telemetry.phono == phono.telemetry()
        && telemetry.scratch == scratch.output()
        && telemetry.radial_tracking == radial_tracking.telemetry()
        && telemetry.phono_output_v
            == phono
                .telemetry()
                .output_v
                .map(|sample| sample * scratch.audible_gain())
        && telemetry.phono_input_overload == phono.telemetry().input_overload
        && telemetry.phono_output_overload == phono.telemetry().output_overload
}

#[derive(Debug, Error)]
pub enum PhysicalRecordPlayerError {
    #[error(transparent)]
    Profile(#[from] super::PhysicalProfileError),
    #[error(transparent)]
    DeckConfig(#[from] crate::PhysicalDeckConfigError),
    #[error(transparent)]
    Deck(#[from] DeckMechanicalError),
    #[error(transparent)]
    TimelineCreate(#[from] ControlTimelineCreateError),
    #[error(transparent)]
    TimelinePush(#[from] ControlTimelinePushError),
    #[error(transparent)]
    TimelineAdvance(#[from] ControlTimelineAdvanceError),
    #[error(transparent)]
    TimelineCheckpointRestore(#[from] ControlTimelineCheckpointRestoreError),
    #[error(transparent)]
    TimelineRestore(#[from] ControlTimelineRestoreError),
    #[error(transparent)]
    Pickup(#[from] super::PickupMechanicalError),
    #[error(transparent)]
    CartridgeConfig(#[from] super::MovingMagnetCartridgeConfigError),
    #[error(transparent)]
    Cartridge(#[from] super::MovingMagnetCartridgeError),
    #[error(transparent)]
    CoupledPickupCartridge(#[from] super::CoupledPickupCartridgeError),
    #[error(transparent)]
    CoupledRecordPlayerStep(#[from] CoupledRecordPlayerStepError),
    #[error(transparent)]
    Phono(#[from] super::PhysicalPhonoStageError),
    #[error(transparent)]
    Scratch(#[from] ScratchPerformanceError),
    #[error(transparent)]
    RadialTracking(#[from] super::RadialTrackingError),
    #[error(transparent)]
    Stylus(#[from] super::StylusTraceError),
    #[error(transparent)]
    TraceAdmission(#[from] GrooveTraceAdmissionError),
    #[error(transparent)]
    Groove(#[from] GrooveError),
    #[error(transparent)]
    PagedGroove(#[from] PagedGrooveError),
    #[error(transparent)]
    RealtimePagedGroove(#[from] RealtimePagedGrooveError),
    #[error(transparent)]
    PagedGrooveRenderMiss(PagedGrooveRenderMiss),
    #[error("groove layout does not match the physical profile")]
    GrooveLayoutMismatch,
    #[error("groove cut does not match the physical profile")]
    GrooveCutMismatch,
    #[error("groove programme extends inside the configured playable radius")]
    GrooveProgrammeDoesNotFit,
    #[error("adjacent groove turns do not have the configured minimum land clearance")]
    GrooveOvercut,
    #[error("groove frame count cannot be represented exactly by the render coordinate")]
    GrooveFrameCountNotExactlyRepresentable,
    #[error("cache replacement requires a loaded paged groove source")]
    PagedCacheReplacementRequiresPagedSource,
    #[error("cache replacement does not describe the loaded paged groove generation")]
    PagedCacheReplacementMismatch,
    #[error("groove position must be finite and nonnegative")]
    InvalidGroovePosition,
    #[error("radial turn selection is inconsistent with the loaded groove")]
    InvalidRadialSelection,
    #[error("unsupported physical record-player snapshot version {version}")]
    UnsupportedSnapshotVersion { version: u32 },
    #[error("physical record-player snapshot uses a different profile")]
    SnapshotProfileMismatch,
    #[error("physical record-player snapshot uses a different loaded groove")]
    SnapshotGrooveMismatch,
    #[error("physical record-player snapshot uses a different groove representation")]
    SnapshotSourceMismatch,
    #[error("physical record-player snapshot is invalid")]
    InvalidSnapshot,
    #[error("render output must contain interleaved stereo frames")]
    OutputMustBeStereo,
    #[error("render block exceeds {maximum} frames")]
    RenderBlockTooLarge { maximum: usize },
    #[error("physical render output is not finite")]
    NonfiniteOutput,
    #[cfg(test)]
    #[error("injected physical render failure")]
    InjectedTestFailure,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physical::{
        GrooveFrameRange, PagedGrooveCacheLimits, PagedGrooveCacheProducer, ParameterValue,
        PhysicalGrooveMetadata, PhysicalGroovePage, PhysicalHostOutputConfig, PhysicalHostRenderer,
        PhysicalHostRendererError, RealtimePagedGrooveCacheConfig,
        RealtimePagedGroovePageDescriptor, RealtimePagedGrooveSlotPhase,
    };
    use crate::spsc::timed_player_control_mailbox;
    use crate::{DeckMechanicalControl, MotorMode, NormalizedDeckControl, PhysicalDeckConfig};

    const TEST_HOST_OUTPUT: PhysicalHostOutputConfig = PhysicalHostOutputConfig {
        volts_per_full_scale: 10.0,
    };

    #[test]
    fn transform_wall_contact_set_preserves_certified_material_lineage() {
        let samples = [0.0_f32; 128];
        let original = crate::physical::trace_spherical_uniform_contacts(
            &samples,
            64.375,
            2.0e-6,
            StylusGeometry::default(),
        )
        .unwrap();
        let transformed = transform_wall_contact_set(original, 7.0e-6, true);
        assert_eq!(
            transformed.contacts[0].certified_position_interval,
            original.contacts[0].certified_position_interval
        );
        assert_eq!(
            transformed.center_displacement_m,
            original.center_displacement_m + 7.0e-6
        );
        assert_eq!(transformed.contacts[0].groove_slope, 0.0);
        let resolve = |contacts| {
            super::super::tangential_identity::resolve_tangential_contact_identity(
                GrooveContentIdentity::from_sha256([0x63; 32]),
                0,
                samples.len() as u64,
                0,
                contacts,
            )
            .unwrap()
        };
        assert_eq!(resolve(transformed), resolve(original));
    }

    fn sine_groove(profile: &PhysicalProfile, frequency_hz: f64) -> Arc<GrooveAsset> {
        stereo_sine_groove(profile, frequency_hz, 1.0)
    }

    fn stereo_sine_groove(
        profile: &PhysicalProfile,
        frequency_hz: f64,
        right_polarity: f64,
    ) -> Arc<GrooveAsset> {
        stereo_sine_groove_with_gain(profile, frequency_hz, right_polarity, 1.0)
    }

    fn stereo_sine_groove_with_gain(
        profile: &PhysicalProfile,
        frequency_hz: f64,
        right_polarity: f64,
        velocity_gain: f64,
    ) -> Arc<GrooveAsset> {
        let sample_rate = profile.config.groove.groove_sample_rate_hz;
        let frames = sample_rate as usize;
        let zero_edge_frames = 1_024;
        let peak_wall_velocity_m_s = profile.config.record_cut.full_scale_sine_velocity_rms_m_s
            * std::f64::consts::SQRT_2
            * 0.5;
        let left: Vec<f32> = (0..frames)
            .map(|frame| {
                if frame < zero_edge_frames || frame + zero_edge_frames >= frames {
                    0.0
                } else {
                    (peak_wall_velocity_m_s
                        * velocity_gain
                        * (std::f64::consts::TAU * frequency_hz * frame as f64 / sample_rate).sin())
                        as f32
                }
            })
            .collect();
        let right: Vec<f32> = left
            .iter()
            .map(|sample| (*sample as f64 * right_polarity) as f32)
            .collect();
        Arc::new(
            GrooveAsset::from_stereo_wall_velocity_m_s_with_cut(
                &left,
                &right,
                profile.config.groove,
                profile.config.record_cut,
            )
            .unwrap(),
        )
    }

    #[test]
    fn sine_fixture_has_exact_c1_record_clamps_at_every_spatial_level() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 997.0);
        let assert_clamped = |label: &str, lateral: &[f32], vertical: &[f32]| {
            for index in 1..4 {
                assert_eq!(lateral[0], lateral[index], "{label} left lateral {index}");
                assert_eq!(
                    vertical[0], vertical[index],
                    "{label} left vertical {index}"
                );
                assert_eq!(
                    lateral[lateral.len() - 1],
                    lateral[lateral.len() - 1 - index],
                    "{label} right lateral {index}"
                );
                assert_eq!(
                    vertical[vertical.len() - 1],
                    vertical[vertical.len() - 1 - index],
                    "{label} right vertical {index}"
                );
            }
        };
        assert_clamped(
            "base",
            groove.lateral_displacement_m(),
            groove.vertical_displacement_m(),
        );
        for (index, level) in groove.spatial_pyramid().levels().iter().enumerate() {
            assert_clamped(
                match index {
                    0 => "level 0",
                    1 => "level 1",
                    2 => "level 2",
                    _ => "level 3",
                },
                level.lateral_displacement_m(),
                level.vertical_displacement_m(),
            );
        }
        assert!(groove
            .trace_admission_certificate()
            .clamped_edges_c1_certified());
    }

    fn motor_control(config: PhysicalDeckConfig, rate: f64) -> DeckMechanicalControl {
        DeckMechanicalControl::from_normalized(
            config,
            NormalizedDeckControl {
                motor_mode: MotorMode::Servo,
                motor_rate: rate,
                hand_contact: false,
                hand_target_angle_turns: None,
                hand_rate: 0.0,
                grip: 0.0,
                stylus_torque_nm: 0.0,
            },
        )
    }

    fn player_control(config: PhysicalDeckConfig, rate: f64) -> PlayerControl {
        PlayerControl::new(motor_control(config, rate), true)
    }

    fn scratch_player_control(
        config: PhysicalDeckConfig,
        motor_rate: f64,
        hand_rate: f64,
        preset: crate::ScratchPreset,
        clicks: u8,
        manual_crossfader_gain: f64,
    ) -> PlayerControl {
        let mut deck = motor_control(config, motor_rate);
        deck.hand_contact = true;
        deck.hand_target_angular_velocity_rad_s =
            hand_rate * config.nominal_angular_velocity_rad_s();
        deck.hand_normal_force_n = 5.0;
        PlayerControl::new(deck, true).with_scratch(preset, clicks, manual_crossfader_gain)
    }

    fn closed_automatic_scratch(
        preset: crate::ScratchPreset,
        internal_sample_rate_hz: f64,
    ) -> ScratchPerformance {
        let mut scratch = ScratchPerformance::new(preset);
        let input = ScratchPerformanceInput {
            delta_seconds: 1.0 / internal_sample_rate_hz,
            hand_contact: true,
            intent_record_rate: 0.0,
            rendered_record_rate: 0.0,
            rendered_source_travel_seconds: 0.0,
            manual_crossfader_gain: 1.0,
        };
        for _ in 0..8_192 {
            scratch.process_frame(input).unwrap();
        }
        assert!(scratch.audible_gain() < 1.0e-9);
        scratch
    }

    fn assert_player_state_matches_ignoring_source_representation(
        left: &PhysicalRecordPlayer,
        right: &PhysicalRecordPlayer,
    ) {
        let mut left = left.snapshot();
        let mut right = right.snapshot();
        left.loaded_source_identity = None;
        right.loaded_source_identity = None;
        assert_eq!(left, right);
    }

    fn paged_cache_for_ranges(
        profile: &PhysicalProfile,
        groove: &GrooveAsset,
        generation_value: u64,
        core_ranges: &[GrooveFrameRange],
    ) -> Arc<PagedGrooveCache> {
        let generation = GrooveGenerationId::new(generation_value).unwrap();
        let seed_metadata =
            PhysicalGrooveMetadata::from_groove_asset(generation, groove, 1).unwrap();
        let tracing_halo = seed_metadata
            .minimum_tracing_halo_frames(profile.config.stylus)
            .unwrap()
            .max(1);
        let metadata =
            PhysicalGrooveMetadata::from_groove_asset(generation, groove, tracing_halo).unwrap();
        metadata
            .validate_tracing_geometry(profile.config.stylus)
            .unwrap();
        let limits = PagedGrooveCacheLimits::new(
            u32::try_from(core_ranges.len().max(1)).unwrap(),
            512 * 1_024 * 1_024,
        )
        .unwrap();
        let mut producer = PagedGrooveCacheProducer::new(metadata, limits).unwrap();
        let storage_halo = u64::from(metadata.required_storage_halo_frames());
        let total_frames = metadata.total_frame_count();
        for &core_range in core_ranges {
            let stored_start = core_range.start_frame().saturating_sub(storage_halo);
            let stored_end = core_range
                .end_frame_exclusive()
                .saturating_add(storage_halo)
                .min(total_frames);
            let stored_range = GrooveFrameRange::new(stored_start, stored_end).unwrap();
            let stored_start = usize::try_from(stored_start).unwrap();
            let stored_end = usize::try_from(stored_end).unwrap();
            producer
                .insert_page(
                    PhysicalGroovePage::new(
                        metadata,
                        core_range,
                        stored_range,
                        groove.lateral_displacement_m()[stored_start..stored_end].to_vec(),
                        groove.vertical_displacement_m()[stored_start..stored_end].to_vec(),
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        Arc::new(producer.publish())
    }

    fn full_paged_cache(
        profile: &PhysicalProfile,
        groove: &GrooveAsset,
        generation_value: u64,
        page_frames: u64,
    ) -> Arc<PagedGrooveCache> {
        let total_frames = groove.frame_count() as u64;
        let mut ranges = Vec::new();
        let mut start = 0;
        while start < total_frames {
            let end = start.saturating_add(page_frames).min(total_frames);
            ranges.push(GrooveFrameRange::new(start, end).unwrap());
            start = end;
        }
        paged_cache_for_ranges(profile, groove, generation_value, &ranges)
    }

    fn realtime_cache_from_paged(
        paged: &PagedGrooveCache,
        minimum_slots: u32,
        minimum_stored_frames: u32,
    ) -> RealtimePagedGrooveCache {
        let maximum_stored_frames = paged
            .pages()
            .map(|page| u32::try_from(page.stored_range().frame_count()).unwrap())
            .max()
            .unwrap_or(4)
            .max(minimum_stored_frames);
        let page_slots = u32::try_from(paged.page_count())
            .unwrap()
            .max(minimum_slots);
        let mut cache = RealtimePagedGrooveCache::new(
            paged.metadata(),
            RealtimePagedGrooveCacheConfig {
                page_slots,
                maximum_stored_frames_per_page: maximum_stored_frames,
                maximum_chunk_frames: maximum_stored_frames.min(8_192),
                maximum_work_units_per_call: 65_536,
                maximum_resident_bytes: 512 * 1_024 * 1_024,
            },
        )
        .unwrap();
        for page in paged.pages() {
            publish_realtime_page(&mut cache, page);
        }
        cache
    }

    fn publish_realtime_page(cache: &mut RealtimePagedGrooveCache, page: &PhysicalGroovePage) {
        let ticket = cache
            .begin_page(RealtimePagedGroovePageDescriptor::from_page(page))
            .unwrap();
        let chunk_frames = cache.config().maximum_chunk_frames as usize;
        for (chunk_index, chunk) in page
            .lateral_displacement_m()
            .chunks(chunk_frames)
            .enumerate()
        {
            cache
                .ingest_lateral_chunk(ticket, (chunk_index * chunk_frames) as u32, chunk)
                .unwrap();
        }
        for (chunk_index, chunk) in page
            .vertical_displacement_m()
            .chunks(chunk_frames)
            .enumerate()
        {
            cache
                .ingest_vertical_chunk(ticket, (chunk_index * chunk_frames) as u32, chunk)
                .unwrap();
        }
        cache.finish_page_ingestion(ticket).unwrap();
        loop {
            let progress = cache.advance_page(ticket, 65_536).unwrap();
            if progress.phase == RealtimePagedGrooveSlotPhase::Ready {
                break;
            }
        }
        cache.publish_page(ticket).unwrap();
    }

    fn synchronize_profile_evidence(profile: &mut PhysicalProfile) {
        let manifest = profile.parameter_manifest().unwrap();
        for evidence in &mut profile.evidence {
            evidence.value = manifest
                .iter()
                .find(|entry| entry.parameter == evidence.parameter)
                .unwrap()
                .value
                .clone();
        }
        profile.validate().unwrap();
    }

    #[test]
    fn complete_chain_renders_finite_voltage_and_advances_the_groove() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 1_000.0);
        let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        player.load_groove(groove).unwrap();
        player
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                player_control(profile.config.deck, 1.0),
            ))
            .unwrap();
        let mut output = vec![0.0_f32; 2 * 2_048];
        for _ in 0..100 {
            player.render_internal_interleaved(&mut output).unwrap();
        }
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(output.iter().any(|sample| sample.abs() > 1.0e-8));
        assert!(player.groove_frame_position() > 1_000.0);
        assert!(player.telemetry().groove_loaded);
        assert_eq!(player.telemetry().radial_tracking.captured_turn_index, 0);
        assert_eq!(player.telemetry().radial_tracking.total_turns_skipped, 0);
    }

    #[test]
    #[ignore = "release-build callback timing characterization"]
    fn midpoint_player_release_block_benchmark() {
        use std::time::Instant;

        fn percentile(values: &[u128], numerator: usize) -> u128 {
            let mut sorted = values.to_vec();
            sorted.sort_unstable();
            sorted[(sorted.len() - 1) * numerator / 100]
        }

        fn run(label: &str, profile: &PhysicalProfile, groove: Arc<GrooveAsset>, reversal: bool) {
            const BLOCK_FRAMES: usize = 128;
            const BLOCK_COUNT: usize = 512;
            const BLOCK_DEADLINE_NS: u128 =
                1_000_000_000_u128 * BLOCK_FRAMES as u128 / 192_000_u128;
            let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
            player.load_groove(groove).unwrap();
            player.reset_transport(1.0, 1.0, 0.0, 0.0).unwrap();
            player
                .enqueue_control(TimedPlayerControl::new(
                    0,
                    1,
                    player_control(profile.config.deck, 1.0),
                ))
                .unwrap();
            player
                .render_internal_interleaved(&mut [0.0_f32; 2 * 1_024])
                .unwrap();

            let mut sequence = 2_u64;
            let mut elapsed_ns = Vec::with_capacity(BLOCK_COUNT);
            let mut output = [0.0_f32; 2 * BLOCK_FRAMES];
            for block in 0..BLOCK_COUNT {
                if reversal {
                    let current_frame = player.current_internal_frame();
                    for (offset, direction) in [(0_u64, 20.0), (64_u64, -20.0)] {
                        let direction = if block % 2 == 0 {
                            direction
                        } else {
                            -direction
                        };
                        let mut control = motor_control(profile.config.deck, 1.0);
                        control.hand_contact = true;
                        control.hand_target_angular_velocity_rad_s =
                            direction * profile.config.deck.nominal_angular_velocity_rad_s();
                        control.hand_normal_force_n = 5.0;
                        control.hand_contact_radius_m = 0.12;
                        player
                            .enqueue_control(TimedPlayerControl::new(
                                current_frame + offset,
                                sequence,
                                PlayerControl::new(control, true),
                            ))
                            .unwrap();
                        sequence += 1;
                    }
                }
                let started = Instant::now();
                player.render_internal_interleaved(&mut output).unwrap();
                elapsed_ns.push(started.elapsed().as_nanos());
            }
            let total: u128 = elapsed_ns.iter().sum();
            let missed = elapsed_ns
                .iter()
                .filter(|elapsed| **elapsed > BLOCK_DEADLINE_NS)
                .count();
            eprintln!(
                "midpoint-player-benchmark {label}: blocks={BLOCK_COUNT} frames_per_block={BLOCK_FRAMES} deadline_ns={BLOCK_DEADLINE_NS} elapsed_ns[min/p50/p95/p99/max/mean]={}/{}/{}/{}/{}/{} misses={missed}",
                *elapsed_ns.iter().min().unwrap(),
                percentile(&elapsed_ns, 50),
                percentile(&elapsed_ns, 95),
                percentile(&elapsed_ns, 99),
                *elapsed_ns.iter().max().unwrap(),
                total / elapsed_ns.len() as u128,
            );
        }

        assert!(!cfg!(debug_assertions), "run this benchmark with --release");
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 3_000.0);
        run("normal", &profile, Arc::clone(&groove), false);
        run("reversal", &profile, groove, true);
    }

    #[test]
    fn positive_lateral_groove_center_has_opposite_wall_normal_offsets() {
        let center_m = 125.0e-6;
        let offsets = groove_wall_center_offsets(center_m);
        assert!(offsets[0] > 0.0);
        assert!(offsets[1] < 0.0);
        assert_eq!(offsets[0], -offsets[1]);
        assert_eq!(offsets[0], center_m * std::f64::consts::FRAC_1_SQRT_2);
    }

    #[test]
    fn land_render_does_not_read_programme_walls() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let frame_count = 8_192;
        let make_groove = |cycles: f64| {
            let left: Vec<f32> = (0..frame_count)
                .map(|frame| {
                    (0.001
                        * (std::f64::consts::TAU * cycles * frame as f64 / frame_count as f64)
                            .sin()) as f32
                })
                .collect();
            let right: Vec<f32> = left.iter().map(|sample| -*sample).collect();
            Arc::new(
                GrooveAsset::from_stereo_wall_velocity_m_s_with_cut(
                    &left,
                    &right,
                    profile.config.groove,
                    profile.config.record_cut,
                )
                .unwrap(),
            )
        };
        let grooves = [make_groove(16.0), make_groove(31.0)];
        assert_ne!(
            grooves[0].provenance().content_identity(),
            grooves[1].provenance().content_identity()
        );
        let pitch_m = profile.config.record_cut.groove_pitch_m_per_revolution;
        let land_height_m = profile.config.record_cut.groove_top_width_m * 0.5;
        let mut rendered = Vec::new();
        for groove in grooves {
            let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
            player.load_groove(groove).unwrap();
            player
                .pickup
                .reset(
                    [0.55 * pitch_m, land_height_m],
                    [0.0; 2],
                    [0.55 * pitch_m, land_height_m],
                    [0.0; 2],
                )
                .unwrap();
            player
                .enqueue_control(TimedPlayerControl::new(
                    0,
                    1,
                    PlayerControl::new(DeckMechanicalControl::default(), true),
                ))
                .unwrap();
            let mut output = [0.0_f32; 64];
            player.render_internal_interleaved(&mut output).unwrap();
            assert_eq!(
                player.telemetry().radial_tracking.contact_region,
                RadialContactRegion::Land
            );
            assert!(player.telemetry().pickup.land_contact);
            assert_eq!(
                player.groove_frame_position(),
                player.spiral_frame_position()
            );
            rendered.push(output);
        }
        assert_eq!(rendered[0], rendered[1]);
    }

    #[test]
    fn radial_recapture_selects_the_physical_adjacent_turn() {
        let mut profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        profile.config.contact.groove_friction_coefficient = 0.0;
        profile.config.contact.record_surface_friction_coefficient = 0.0;
        profile.config.tonearm.lateral_bearing_static_friction_n = 0.0;
        profile.config.tonearm.lateral_bearing_kinetic_friction_n = 0.0;
        profile
            .config
            .tonearm
            .lateral_bearing_viscous_damping_n_s_per_m = 0.0;
        profile.config.radial_tracking.contact_release_margin_m = 0.1e-6;
        profile.config.radial_tracking.recapture_inset_m = 0.1e-6;
        profile.config.radial_tracking.maximum_turns_per_recapture = 4;
        synchronize_profile_evidence(&mut profile);

        let frames_per_turn = (profile.config.groove.groove_sample_rate_hz * 60.0
            / profile.config.groove.nominal_rpm)
            .round() as usize;
        let silence = vec![0.0_f32; frames_per_turn * 2 + 4];
        let groove = Arc::new(
            GrooveAsset::from_stereo_wall_velocity_m_s_with_cut(
                &silence,
                &silence,
                profile.config.groove,
                profile.config.record_cut,
            )
            .unwrap(),
        );
        let pitch_m = profile.config.record_cut.groove_pitch_m_per_revolution;
        let land_height_m = profile.config.record_cut.groove_top_width_m * 0.5;
        for target_turn in [1_i64, -1_i64] {
            let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
            player.load_groove(Arc::clone(&groove)).unwrap();
            player
                .set_groove_frame_position(frames_per_turn as f64 + 2.0)
                .unwrap();
            player
                .enqueue_control(TimedPlayerControl::new(
                    0,
                    1,
                    PlayerControl::new(DeckMechanicalControl::default(), true),
                ))
                .unwrap();

            let release_m = 0.55 * target_turn as f64 * pitch_m;
            player
                .pickup
                .reset(
                    [release_m, land_height_m],
                    [0.0; 2],
                    [release_m, land_height_m],
                    [0.0; 2],
                )
                .unwrap();
            let mut output = [0.0_f32; 2];
            player.render_internal_interleaved(&mut output).unwrap();
            assert_eq!(
                player.telemetry().radial_tracking.contact_region,
                RadialContactRegion::Land
            );
            assert_eq!(
                player.groove_frame_position(),
                player.spiral_frame_position()
            );

            let capture_m = target_turn as f64 * pitch_m;
            player
                .pickup
                .reset(
                    [capture_m, land_height_m],
                    [0.0; 2],
                    [capture_m, land_height_m],
                    [0.0; 2],
                )
                .unwrap();
            player.render_internal_interleaved(&mut output).unwrap();
            let telemetry = player.telemetry();
            assert!(telemetry.radial_tracking.recaptured_this_step);
            assert_eq!(telemetry.radial_tracking.captured_turn_index, target_turn);
            assert_eq!(telemetry.radial_tracking.total_turns_skipped, target_turn);
            let separation = telemetry.spiral_frame_position - telemetry.groove_frame_position;
            assert!(
                (separation - target_turn as f64 * frames_per_turn as f64).abs() < 1.0e-9,
                "{target_turn}: {separation}"
            );
            assert!(output.iter().all(|sample| sample.is_finite()));
        }
    }

    #[test]
    fn sample_timed_manual_crossfader_waveform_is_partition_invariant() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 997.0);
        let control = player_control(profile.config.deck, 1.0);
        let muted = control.with_scratch(crate::ScratchPreset::Baby, 1, 0.0);
        let transition_frame = 257_usize;
        let mut whole = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        let mut split = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        let mut open = PhysicalRecordPlayer::new(profile).unwrap();
        whole.load_groove(Arc::clone(&groove)).unwrap();
        split.load_groove(Arc::clone(&groove)).unwrap();
        open.load_groove(groove).unwrap();
        for player in [&mut whole, &mut split, &mut open] {
            player.set_groove_frame_position(10_000.0).unwrap();
            player.reset_transport(1.0, 1.0, 0.0, 0.0).unwrap();
            player
                .enqueue_control(TimedPlayerControl::new(0, 1, control))
                .unwrap();
        }
        for player in [&mut whole, &mut split] {
            player
                .enqueue_control(TimedPlayerControl::new(transition_frame as u64, 2, muted))
                .unwrap();
        }
        let mut whole_output = vec![0.0_f32; 2 * 512];
        whole
            .render_internal_interleaved(&mut whole_output)
            .unwrap();
        let mut split_output = Vec::with_capacity(whole_output.len());
        for _ in 0..4 {
            let mut block = vec![0.0_f32; 2 * 128];
            split.render_internal_interleaved(&mut block).unwrap();
            split_output.extend(block);
        }
        let mut open_output = vec![0.0_f32; whole_output.len()];
        open.render_internal_interleaved(&mut open_output).unwrap();
        assert_eq!(whole_output, split_output);
        assert_eq!(
            &whole_output[..transition_frame * 2],
            &open_output[..transition_frame * 2]
        );
        assert!(whole_output[transition_frame * 2..]
            .iter()
            .zip(&open_output[transition_frame * 2..])
            .any(|(gated, open)| gated.to_bits() != open.to_bits()));
        assert_eq!(
            whole.telemetry().scratch.owner,
            crate::ScratchCrossfaderOwner::Manual
        );
        assert!(whole.telemetry().scratch.audible_gain < 1.0);
        assert_eq!(
            whole.telemetry().phono_output_v,
            whole
                .telemetry()
                .phono
                .output_v
                .map(|sample| sample * whole.telemetry().scratch.audible_gain)
        );
        let whole_telemetry = whole.telemetry();
        let split_telemetry = split.telemetry();
        assert_eq!(whole_telemetry.rendered_internal_frames, 512);
        assert_eq!(split_telemetry.rendered_internal_frames, 128);
        assert_eq!(
            PhysicalRenderTelemetry {
                rendered_internal_frames: 0,
                ..whole_telemetry
            },
            PhysicalRenderTelemetry {
                rendered_internal_frames: 0,
                ..split_telemetry
            }
        );
    }

    #[test]
    fn automatic_preset_uses_clicks_and_same_sample_record_travel() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 1_337.0);
        let mut one_click = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        let mut four_clicks = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        one_click.load_groove(Arc::clone(&groove)).unwrap();
        four_clicks.load_groove(groove).unwrap();
        for player in [&mut one_click, &mut four_clicks] {
            player.set_groove_frame_position(10_000.0).unwrap();
            player.reset_transport(10.0, 10.0, 0.0, 0.0).unwrap();
        }
        one_click
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                scratch_player_control(
                    profile.config.deck,
                    10.0,
                    10.0,
                    crate::ScratchPreset::Transform,
                    1,
                    1.0,
                ),
            ))
            .unwrap();
        four_clicks
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                scratch_player_control(
                    profile.config.deck,
                    10.0,
                    10.0,
                    crate::ScratchPreset::Transform,
                    4,
                    1.0,
                ),
            ))
            .unwrap();

        let mut one_click_output = vec![0.0_f32; 2 * 820];
        let mut four_click_output = vec![0.0_f32; one_click_output.len()];
        one_click
            .render_internal_interleaved(&mut one_click_output)
            .unwrap();
        four_clicks
            .render_internal_interleaved(&mut four_click_output)
            .unwrap();

        let one = one_click.telemetry();
        let four = four_clicks.telemetry();
        assert_eq!(one.mechanics, four.mechanics);
        assert_eq!(one.pickup, four.pickup);
        assert_eq!(one.cartridge, four.cartridge);
        assert_eq!(one.phono, four.phono);
        assert_eq!(one.scratch.preset, crate::ScratchPreset::Transform);
        assert_eq!(one.scratch.clicks, 1);
        assert_eq!(four.scratch.clicks, 4);
        assert_eq!(
            one.scratch.owner,
            crate::ScratchCrossfaderOwner::AutomaticPreset
        );
        assert_eq!(one.scratch.direction, 1);
        assert!(one.scratch.moving);
        assert_eq!(one.scratch.automatic_gate_target, 1.0);
        assert_eq!(four.scratch.automatic_gate_target, 0.0);
        assert!(one_click_output
            .iter()
            .zip(&four_click_output)
            .any(|(left, right)| left.to_bits() != right.to_bits()));

        let exact_travel_seconds = one.mechanics.record_angle_turns * std::f64::consts::TAU
            / profile.config.deck.nominal_angular_velocity_rad_s();
        let expected_progress =
            exact_travel_seconds / crate::ScratchPreset::Transform.initial_stroke_span();
        assert!((one.scratch.stroke_progress - expected_progress).abs() < 1.0e-10);
        assert_eq!(one.scratch.stroke_progress, four.scratch.stroke_progress);
    }

    #[test]
    fn stab_and_chirp_change_phono_audio_at_one_eight_and_twenty_times() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 1_337.0);
        let sample_rate = profile.config.solver.internal_sample_rate_hz;
        let maximum_render_frames = profile.config.solver.maximum_render_frames;

        for preset in [crate::ScratchPreset::Stab, crate::ScratchPreset::Chirp] {
            for rate in [1.0, 8.0, 20.0] {
                let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
                player.load_groove(Arc::clone(&groove)).unwrap();
                player.set_groove_frame_position(10_000.0).unwrap();
                player.reset_transport(rate, rate, 0.0, 0.0).unwrap();
                player.scratch = closed_automatic_scratch(preset, sample_rate);
                player
                    .enqueue_control(TimedPlayerControl::new(
                        0,
                        1,
                        scratch_player_control(
                            profile.config.deck,
                            rate,
                            rate,
                            preset,
                            preset.default_clicks(),
                            1.0,
                        ),
                    ))
                    .unwrap();

                let mut first_frame = [0.0_f32; 2];
                player
                    .render_internal_interleaved(&mut first_frame)
                    .unwrap();
                let closed = player.telemetry();
                assert_eq!(closed.scratch.preset, preset);
                assert_eq!(closed.scratch.automatic_gate_target, 0.0);
                assert!(closed.scratch.audible_gain < 1.0e-8);

                let frames_to_open =
                    (0.10 * preset.initial_stroke_span() / rate * sample_rate).ceil() as usize;
                let mut remaining = frames_to_open.saturating_sub(1);
                let mut opened_output_peak = 0.0_f32;
                while remaining > 0 {
                    let frames = remaining.min(maximum_render_frames);
                    let mut block = vec![0.0_f32; frames * 2];
                    player.render_internal_interleaved(&mut block).unwrap();
                    opened_output_peak = block
                        .into_iter()
                        .fold(opened_output_peak, |peak, sample| peak.max(sample.abs()));
                    remaining -= frames;
                }
                let opened = player.telemetry();
                assert_eq!(
                    opened.scratch.automatic_gate_target, 1.0,
                    "{preset:?} at {rate}x"
                );
                assert!(opened.scratch.audible_gain > 0.35, "{preset:?} at {rate}x");
                assert!(opened_output_peak > 0.0, "{preset:?} at {rate}x");
                assert_eq!(
                    opened.phono_output_v,
                    opened
                        .phono
                        .output_v
                        .map(|sample| sample * opened.scratch.audible_gain)
                );

                let reversal_frame = player.current_internal_frame();
                player
                    .enqueue_control(TimedPlayerControl::new(
                        reversal_frame,
                        2,
                        scratch_player_control(
                            profile.config.deck,
                            rate,
                            -rate,
                            preset,
                            preset.default_clicks(),
                            1.0,
                        ),
                    ))
                    .unwrap();
                let mut reversal_frame_output = [0.0_f32; 2];
                player
                    .render_internal_interleaved(&mut reversal_frame_output)
                    .unwrap();
                let closing = player.telemetry();
                assert_eq!(
                    closing.scratch.automatic_gate_target, 0.0,
                    "{preset:?} at {rate}x"
                );
                assert_eq!(
                    closing.phono_output_v,
                    closing
                        .phono
                        .output_v
                        .map(|sample| sample * closing.scratch.audible_gain)
                );
            }
        }
    }

    #[test]
    fn rapid_reversals_remain_finite_and_each_event_is_applied() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 3_000.0);
        let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        player.load_groove(groove).unwrap();
        let nominal = profile.config.deck.nominal_angular_velocity_rad_s();
        for sequence in 1..=200_u64 {
            let rate = if sequence % 2 == 0 { 10.0 } else { -10.0 };
            let mut control = motor_control(profile.config.deck, 1.0);
            control.hand_contact = true;
            control.hand_target_angular_velocity_rad_s = rate * nominal;
            control.hand_normal_force_n = 5.0;
            control.hand_contact_radius_m = 0.12;
            player
                .enqueue_control(TimedPlayerControl::new(
                    sequence * 5,
                    sequence,
                    PlayerControl::new(control, true),
                ))
                .unwrap();
        }
        let mut output = vec![0.0_f32; 2 * 1_100];
        player.render_internal_interleaved(&mut output).unwrap();
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert_eq!(player.current_internal_frame(), 1_100);
        assert!(player.telemetry().mechanics.record_rate.is_finite());
        assert!(player.controls.next_event().is_none());
    }

    #[test]
    fn stationary_record_has_no_sustained_magnetic_output() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 1_000.0);
        let mut player = PhysicalRecordPlayer::new(profile).unwrap();
        player.load_groove(groove).unwrap();
        let mut output = vec![0.0_f32; 2 * 2_048];
        for _ in 0..100 {
            player.render_internal_interleaved(&mut output).unwrap();
        }
        let cartridge_peak = player
            .telemetry()
            .cartridge
            .load_output_voltage_v
            .into_iter()
            .fold(0.0_f64, |peak, sample| peak.max(sample.abs()));
        assert!(cartridge_peak < 1.0e-12, "{cartridge_peak}");
    }

    #[test]
    fn mono_and_antiphase_cuts_preserve_45_45_channel_polarity() {
        fn render_steady(right_polarity: f64) -> (f64, f64) {
            let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
            let frequency_hz = 1_000.0;
            let groove = stereo_sine_groove(&profile, frequency_hz, right_polarity);
            let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
            player.load_groove(groove).unwrap();
            player
                .enqueue_control(TimedPlayerControl::new(
                    0,
                    1,
                    player_control(profile.config.deck, 1.0),
                ))
                .unwrap();
            let mut output = vec![0.0_f32; 2 * 2_048];
            for _ in 0..100 {
                player.render_internal_interleaved(&mut output).unwrap();
            }
            let mut left = [0.0_f64; 2];
            let mut right = [0.0_f64; 2];
            let mut frame = [0.0_f32; 2];
            for _ in 0..8_192 {
                player.render_internal_interleaved(&mut frame).unwrap();
                let phase = std::f64::consts::TAU * frequency_hz * player.groove_frame_position()
                    / profile.config.groove.groove_sample_rate_hz;
                let basis = [phase.cos(), phase.sin()];
                for component in 0..2 {
                    left[component] += f64::from(frame[0]) * basis[component];
                    right[component] += f64::from(frame[1]) * basis[component];
                }
            }
            let common = [left[0] + right[0], left[1] + right[1]];
            let difference = [left[0] - right[0], left[1] - right[1]];
            (
                common[0] * common[0] + common[1] * common[1],
                difference[0] * difference[0] + difference[1] * difference[1],
            )
        }

        let mono = render_steady(1.0);
        assert!(mono.0 > mono.1 * 16.0, "{mono:?}");
        let antiphase = render_steady(-1.0);
        assert!(antiphase.1 > antiphase.0 * 16.0, "{antiphase:?}");
    }

    #[test]
    fn clamped_programme_boundary_does_not_repeat_endpoint_slope() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 997.0);
        let final_frame = groove.frame_count() - 1;
        let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        player.load_groove(groove).unwrap();
        player
            .set_groove_frame_position(final_frame as f64)
            .unwrap();
        player
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                player_control(profile.config.deck, 1.0),
            ))
            .unwrap();
        let mut output = vec![0.0_f32; 2 * 2_048];
        for _ in 0..100 {
            player.render_internal_interleaved(&mut output).unwrap();
        }
        assert_eq!(player.groove_frame_position(), final_frame as f64);
        assert!(player.telemetry().at_programme_boundary);
        assert!(player.telemetry().mechanics.platter_rate > 0.9);
        assert_eq!(
            player.telemetry().pickup.modulation_reaction_force_n,
            0.0,
            "the programme endpoint must not create a repeated modulation force"
        );
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn unloaded_deck_has_no_stylus_drag() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        player
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                player_control(profile.config.deck, 1.0),
            ))
            .unwrap();
        let mut output = vec![0.0_f32; 2 * 2_048];
        player.render_internal_interleaved(&mut output).unwrap();
        assert_eq!(player.telemetry().mechanics.stylus_torque_nm, 0.0);
        assert_eq!(player.stylus_torque_nm, 0.0);
    }

    #[test]
    fn snapshot_round_trip_continues_with_identical_samples_and_controls() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 1_337.0);
        let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        player.load_groove(groove).unwrap();
        player
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                scratch_player_control(
                    profile.config.deck,
                    1.0,
                    1.0,
                    crate::ScratchPreset::Transform,
                    4,
                    0.3,
                ),
            ))
            .unwrap();
        player
            .enqueue_control(TimedPlayerControl::new(
                400,
                2,
                scratch_player_control(
                    profile.config.deck,
                    1.0,
                    -1.0,
                    crate::ScratchPreset::Transform,
                    4,
                    0.3,
                ),
            ))
            .unwrap();
        player
            .enqueue_control(TimedPlayerControl::new(
                700,
                3,
                scratch_player_control(
                    profile.config.deck,
                    1.0,
                    1.0,
                    crate::ScratchPreset::Transform,
                    4,
                    0.3,
                ),
            ))
            .unwrap();

        let mut preroll = vec![0.0_f32; 2 * 256];
        player.render_internal_interleaved(&mut preroll).unwrap();
        let snapshot = player.snapshot();
        assert_eq!(snapshot.version, 11);
        assert_eq!(
            snapshot.last_telemetry.scratch.preset,
            crate::ScratchPreset::Transform
        );
        assert_eq!(snapshot.last_telemetry.scratch.clicks, 4);
        assert_eq!(
            snapshot.last_telemetry.scratch.owner,
            crate::ScratchCrossfaderOwner::AutomaticPreset
        );
        assert!(snapshot.last_telemetry.scratch.audible_gain < 1.0);
        let json = serde_json::to_string(&snapshot).unwrap();
        let checkpoint: PhysicalRecordPlayerSnapshot = serde_json::from_str(&json).unwrap();

        let mut expected = vec![0.0_f32; 2 * 768];
        player.render_internal_interleaved(&mut expected).unwrap();
        let expected_snapshot = player.snapshot();

        player.restore(&checkpoint).unwrap();
        let mut actual = vec![0.0_f32; expected.len()];
        player.render_internal_interleaved(&mut actual).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(player.snapshot(), expected_snapshot);
    }

    #[test]
    fn initial_lifted_snapshot_is_json_serializable_and_restorable() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let mut player = PhysicalRecordPlayer::new(profile).unwrap();
        let snapshot = player.snapshot();
        let json = serde_json::to_string(&snapshot).unwrap();
        let decoded: PhysicalRecordPlayerSnapshot = serde_json::from_str(&json).unwrap();
        player.restore(&decoded).unwrap();
        assert_eq!(player.snapshot(), snapshot);
    }

    #[test]
    fn invalid_snapshot_does_not_mutate_the_player() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 500.0);
        let mut player = PhysicalRecordPlayer::new(profile).unwrap();
        player.load_groove(groove).unwrap();
        let before = player.snapshot();
        let mut invalid = before.clone();
        invalid.groove_frame_position = f64::NAN;
        assert!(matches!(
            player.restore(&invalid),
            Err(PhysicalRecordPlayerError::InvalidSnapshot)
        ));
        assert_eq!(player.snapshot(), before);
    }

    #[test]
    fn failed_render_restores_all_state_and_leaves_output_untouched() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 1_000.0);
        let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        player.load_groove(groove).unwrap();
        player
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                scratch_player_control(
                    profile.config.deck,
                    1.0,
                    1.0,
                    crate::ScratchPreset::Transform,
                    4,
                    0.25,
                ),
            ))
            .unwrap();
        player.inject_render_failure_at_completed_step(4);

        let before = player.snapshot();
        let mut output = [7.0_f32; 32];
        assert!(player.render_internal_interleaved(&mut output).is_err());
        assert_eq!(output, [7.0_f32; 32]);
        assert_eq!(player.snapshot(), before);
    }

    #[test]
    fn land_snapshot_and_failed_render_are_transactional() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 777.0);
        let pitch_m = profile.config.record_cut.groove_pitch_m_per_revolution;
        let land_height_m = profile.config.record_cut.groove_top_width_m * 0.5;
        let mut player = PhysicalRecordPlayer::new(profile).unwrap();
        player.load_groove(groove).unwrap();
        player
            .pickup
            .reset(
                [0.55 * pitch_m, land_height_m],
                [0.0; 2],
                [0.55 * pitch_m, land_height_m],
                [0.0; 2],
            )
            .unwrap();
        player
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                PlayerControl::new(DeckMechanicalControl::default(), true),
            ))
            .unwrap();
        player
            .render_internal_interleaved(&mut [0.0_f32; 2])
            .unwrap();
        assert_eq!(
            player.telemetry().radial_tracking.contact_region,
            RadialContactRegion::Land
        );

        let encoded = serde_json::to_string(&player.snapshot()).unwrap();
        let decoded: PhysicalRecordPlayerSnapshot = serde_json::from_str(&encoded).unwrap();
        player.restore(&decoded).unwrap();
        let before = player.snapshot();
        player.inject_render_failure_at_completed_step(
            player.telemetry().pickup.completed_steps.saturating_add(2),
        );
        let mut output = [9.0_f32; 16];
        assert!(player.render_internal_interleaved(&mut output).is_err());
        assert_eq!(output, [9.0_f32; 16]);
        assert_eq!(player.snapshot(), before);
    }

    #[test]
    fn control_ingress_retimes_late_events_to_the_next_rendered_sample() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        let mut advance = [0.0_f32; 16];
        player.render_internal_interleaved(&mut advance).unwrap();
        assert_eq!(player.current_internal_frame(), 8);

        let (mut producer, mut consumer) = timed_player_control_mailbox(2).unwrap();
        producer
            .try_push(TimedPlayerControl::new(
                2,
                1,
                player_control(profile.config.deck, 1.0),
            ))
            .unwrap();
        let report = player.drain_control_ingress(&mut consumer);
        assert_eq!(report.enqueued_events, 1);
        assert_eq!(report.retimed_late_events, 1);
        assert_eq!(player.controls.next_event().unwrap().absolute_frame, 8);

        let mut output = [0.0_f32; 2];
        player.render_internal_interleaved(&mut output).unwrap();
        assert!(player.controls.current_control().stylus_lowered);
    }

    #[test]
    fn control_ingress_preserves_the_mailbox_head_during_backpressure() {
        let mut profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        profile.config.solver.control_timeline_capacity = 1;
        profile
            .evidence
            .iter_mut()
            .find(|entry| entry.parameter == "solver.controlTimelineCapacity")
            .unwrap()
            .value = ParameterValue::Unsigned(1);
        let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        player
            .enqueue_control(TimedPlayerControl::new(
                100,
                1,
                player_control(profile.config.deck, 1.0),
            ))
            .unwrap();
        let (mut producer, mut consumer) = timed_player_control_mailbox(1).unwrap();
        producer
            .try_push(TimedPlayerControl::new(
                101,
                2,
                player_control(profile.config.deck, -1.0),
            ))
            .unwrap();

        let blocked = player.drain_control_ingress(&mut consumer);
        assert!(blocked.stopped_for_timeline_backpressure);
        assert_eq!(blocked.inspected_events, 0);
        assert_eq!(consumer.approximate_len(), 1);

        let mut output = vec![0.0_f32; 2 * 101];
        player.render_internal_interleaved(&mut output).unwrap();
        let drained = player.drain_control_ingress(&mut consumer);
        assert_eq!(drained.enqueued_events, 1);
        assert_eq!(consumer.approximate_len(), 0);
    }

    #[test]
    fn control_ingress_discards_and_reports_an_invalid_control() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        let (mut producer, mut consumer) = timed_player_control_mailbox(1).unwrap();
        let mut control = player_control(profile.config.deck, 1.0);
        control.deck.hand_normal_force_n = f64::NAN;
        producer
            .try_push(TimedPlayerControl::new(0, 1, control))
            .unwrap();

        let report = player.drain_control_ingress(&mut consumer);
        assert_eq!(report.rejected_invalid_controls, 1);
        assert_eq!(report.enqueued_events, 0);
        assert_eq!(consumer.approximate_len(), 0);
    }

    #[test]
    fn mailbox_and_direct_control_paths_apply_identical_sample_timing() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let mut direct = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        let mut mailbox = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        let events = [
            TimedPlayerControl::new(0, 1, player_control(profile.config.deck, 1.0)),
            TimedPlayerControl::new(5, 2, player_control(profile.config.deck, -1.0)),
            TimedPlayerControl::new(6, 3, player_control(profile.config.deck, 1.0)),
        ];
        let (mut producer, mut consumer) = timed_player_control_mailbox(events.len()).unwrap();
        for event in events {
            direct.enqueue_control(event).unwrap();
            producer.try_push(event).unwrap();
        }
        assert_eq!(
            mailbox.drain_control_ingress(&mut consumer).enqueued_events,
            events.len()
        );

        let mut direct_output = [0.0_f32; 32];
        let mut mailbox_output = [0.0_f32; 32];
        direct
            .render_internal_interleaved(&mut direct_output)
            .unwrap();
        mailbox
            .render_internal_interleaved(&mut mailbox_output)
            .unwrap();
        assert_eq!(direct_output, mailbox_output);
        assert_eq!(direct.snapshot(), mailbox.snapshot());
    }

    #[test]
    fn ten_times_speed_selects_prefiltered_spatial_levels() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 10_000.0);
        let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        player.load_groove(groove).unwrap();
        player
            .set_groove_frame_position(profile.config.groove.groove_sample_rate_hz / 2.0)
            .unwrap();
        player.reset_transport(10.0, 10.0, 0.0, 0.0).unwrap();
        player
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                PlayerControl::new(DeckMechanicalControl::default(), true),
            ))
            .unwrap();
        let mut output = [0.0_f32; 2];
        player.render_internal_interleaved(&mut output).unwrap();
        let telemetry = player.telemetry();
        assert!(telemetry.spatial_filter_lower_step_frames >= 8);
        assert!(telemetry.spatial_filter_upper_step_frames >= 8);
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn paged_and_contiguous_sources_are_bit_exact_across_bidirectional_seams() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 1_237.0);
        let cache = full_paged_cache(&profile, &groove, 71, 32_768);
        let cases = [(32_760.0, 1.0), (32_776.0, -1.0)];
        for (position, rate) in cases {
            let mut contiguous = PhysicalRecordPlayer::new(profile.clone()).unwrap();
            let mut paged = PhysicalRecordPlayer::new(profile.clone()).unwrap();
            contiguous.load_groove(Arc::clone(&groove)).unwrap();
            paged.load_paged_groove(Arc::clone(&cache)).unwrap();
            contiguous.set_groove_frame_position(position).unwrap();
            paged.set_groove_frame_position(position).unwrap();
            contiguous.reset_transport(rate, rate, 0.0, 0.0).unwrap();
            paged.reset_transport(rate, rate, 0.0, 0.0).unwrap();
            let control = PlayerControl::new(DeckMechanicalControl::default(), true);
            contiguous
                .enqueue_control(TimedPlayerControl::new(0, 1, control))
                .unwrap();
            paged
                .enqueue_control(TimedPlayerControl::new(0, 1, control))
                .unwrap();

            let mut contiguous_output = [0.0_f32; 128];
            let mut paged_output = [0.0_f32; 128];
            contiguous
                .render_internal_interleaved(&mut contiguous_output)
                .unwrap();
            paged
                .render_internal_interleaved(&mut paged_output)
                .unwrap();
            assert_eq!(paged_output, contiguous_output, "rate {rate}");
            let paged_telemetry = paged.telemetry();
            let contiguous_telemetry = contiguous.telemetry();
            assert_eq!(
                paged_telemetry.groove_frame_position,
                contiguous_telemetry.groove_frame_position
            );
            assert_eq!(
                paged_telemetry.spatial_filter_lower_step_frames,
                contiguous_telemetry.spatial_filter_lower_step_frames
            );
            assert_eq!(
                paged_telemetry.spatial_filter_upper_step_frames,
                contiguous_telemetry.spatial_filter_upper_step_frames
            );
            for wall in 0..2 {
                let resolve = |contacts| {
                    super::super::tangential_identity::resolve_tangential_contact_identity(
                        groove.provenance().content_identity(),
                        cache.generation().get(),
                        groove.frame_count() as u64,
                        wall,
                        contacts,
                    )
                    .unwrap()
                };
                assert_eq!(
                    resolve(paged_telemetry.pickup.wall_longitudinal_contact[wall].geometry),
                    resolve(contiguous_telemetry.pickup.wall_longitudinal_contact[wall].geometry),
                    "rate {rate}, wall {wall}"
                );
            }
            assert!(
                (paged_telemetry.pickup.tip_displacement_m[0]
                    - contiguous_telemetry.pickup.tip_displacement_m[0])
                    .abs()
                    < 1.0e-18,
                "rate {rate}"
            );
        }
    }

    #[test]
    fn paged_render_misses_are_typed_and_transactional_at_both_render_layers() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 911.0);
        let empty_cache = paged_cache_for_ranges(&profile, &groove, 72, &[]);
        let generation = empty_cache.generation();

        let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        player.load_paged_groove(Arc::clone(&empty_cache)).unwrap();
        player.set_groove_frame_position(50_000.0).unwrap();
        player.reset_transport(1.0, 1.0, 0.0, 0.0).unwrap();
        player
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                PlayerControl::new(DeckMechanicalControl::default(), true),
            ))
            .unwrap();
        let before = player.snapshot();
        let mut output = [7.0_f32; 32];
        let error = player.render_internal_interleaved(&mut output).unwrap_err();
        assert!(matches!(
            error,
            PhysicalRecordPlayerError::PagedGrooveRenderMiss(
                PagedGrooveRenderMiss::PageUnavailable {
                    generation: missed_generation,
                    ..
                }
            ) if missed_generation == generation
        ));
        assert_eq!(output, [7.0_f32; 32]);
        assert_eq!(player.snapshot(), before);

        let stale_generation = GrooveGenerationId::new(73).unwrap();
        let mut stale = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        stale
            .load_paged_groove_generation(Arc::clone(&empty_cache), stale_generation)
            .unwrap();
        stale.set_groove_frame_position(50_000.0).unwrap();
        stale.reset_transport(1.0, 1.0, 0.0, 0.0).unwrap();
        stale
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                PlayerControl::new(DeckMechanicalControl::default(), true),
            ))
            .unwrap();
        let stale_before = stale.snapshot();
        let mut stale_output = [11.0_f32; 2];
        assert!(matches!(
            stale.render_internal_interleaved(&mut stale_output),
            Err(PhysicalRecordPlayerError::PagedGrooveRenderMiss(
                PagedGrooveRenderMiss::StaleGeneration {
                    requested_generation,
                    cached_generation,
                }
            )) if requested_generation == stale_generation && cached_generation == generation
        ));
        assert_eq!(stale_output, [11.0_f32; 2]);
        assert_eq!(stale.snapshot(), stale_before);
        assert!(matches!(
            stale.paged_prefetch_plan(512, &[0]),
            Err(PhysicalRecordPlayerError::PagedGrooveRenderMiss(
                PagedGrooveRenderMiss::StaleGeneration { .. }
            ))
        ));
        assert_eq!(stale.snapshot(), stale_before);

        let mut renderer = PhysicalHostRenderer::new(profile, 48_000, TEST_HOST_OUTPUT).unwrap();
        renderer.load_paged_groove(empty_cache).unwrap();
        renderer.set_groove_frame_position(50_000.0).unwrap();
        renderer.reset_transport(1.0, 1.0, 0.0, 0.0).unwrap();
        renderer
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                PlayerControl::new(DeckMechanicalControl::default(), true),
            ))
            .unwrap();
        let renderer_before = renderer.snapshot();
        let mut host_output = [13.0_f32; 64];
        assert!(matches!(
            renderer.render_interleaved(&mut host_output),
            Err(PhysicalHostRendererError::PlayerRender {
                source: PhysicalRecordPlayerError::PagedGrooveRenderMiss(
                    PagedGrooveRenderMiss::PageUnavailable { .. }
                ),
                ..
            })
        ));
        assert_eq!(host_output, [13.0_f32; 64]);
        assert_eq!(renderer.snapshot(), renderer_before);
    }

    #[test]
    fn realtime_page_miss_rolls_back_and_later_publication_preserves_player_state() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 917.0);
        let core = GrooveFrameRange::new(0, 65_536).unwrap();
        let page_cache = paged_cache_for_ranges(&profile, &groove, 174, &[core]);
        let empty_paged = paged_cache_for_ranges(&profile, &groove, 174, &[]);
        let empty_realtime = realtime_cache_from_paged(&empty_paged, 2, 70_000);
        let full_realtime = realtime_cache_from_paged(&page_cache, 2, 70_000);

        let mut incremental = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        incremental
            .load_realtime_paged_groove(empty_realtime)
            .unwrap();
        incremental.set_groove_frame_position(50_000.0).unwrap();
        incremental.reset_transport(1.0, 1.0, 0.0, 0.0).unwrap();
        incremental
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                PlayerControl::new(DeckMechanicalControl::default(), true),
            ))
            .unwrap();
        let before_miss = incremental.snapshot();
        let mut missed_output = [19.0_f32; 64];
        assert!(matches!(
            incremental.render_internal_interleaved(&mut missed_output),
            Err(PhysicalRecordPlayerError::PagedGrooveRenderMiss(
                PagedGrooveRenderMiss::PageUnavailable { frame: 50_000, .. }
            ))
        ));
        assert_eq!(missed_output, [19.0_f32; 64]);
        assert_eq!(incremental.snapshot(), before_miss);

        let mut resident = PhysicalRecordPlayer::new(profile).unwrap();
        resident.load_realtime_paged_groove(full_realtime).unwrap();
        resident.set_groove_frame_position(50_000.0).unwrap();
        resident.reset_transport(1.0, 1.0, 0.0, 0.0).unwrap();
        resident
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                PlayerControl::new(DeckMechanicalControl::default(), true),
            ))
            .unwrap();
        assert_player_state_matches_ignoring_source_representation(&incremental, &resident);

        let before_publication = incremental.snapshot();
        let before_publication_checkpoint = incremental.render_checkpoint();
        publish_realtime_page(
            incremental.realtime_paged_cache_mut().unwrap(),
            page_cache.pages().next().unwrap(),
        );
        assert_ne!(incremental.snapshot(), before_publication);
        assert_eq!(
            incremental.render_checkpoint(),
            before_publication_checkpoint
        );
        assert_eq!(
            incremental.loaded_source_identity(),
            resident.loaded_source_identity()
        );
        let mut incremental_output = [0.0_f32; 128];
        let mut resident_output = [0.0_f32; 128];
        incremental
            .render_internal_interleaved(&mut incremental_output)
            .unwrap();
        resident
            .render_internal_interleaved(&mut resident_output)
            .unwrap();
        assert_eq!(incremental_output, resident_output);
        assert_eq!(incremental.snapshot(), resident.snapshot());
    }

    #[test]
    fn certified_source_slope_and_friction_are_bound_at_every_publication_boundary() {
        let base_profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = stereo_sine_groove_with_gain(&base_profile, 10_007.0, 1.0, 8.0);
        let full_range = GrooveFrameRange::new(0, groove.frame_count() as u64).unwrap();
        let full_paged = paged_cache_for_ranges(&base_profile, &groove, 175, &[full_range]);
        let empty_paged = paged_cache_for_ranges(&base_profile, &groove, 175, &[]);
        let maximum_absolute_wall_slope = groove
            .trace_admission_certificate()
            .maximum_absolute_wall_slope()
            .max(full_paged.maximum_certified_absolute_wall_slope());
        assert!(maximum_absolute_wall_slope > 0.5);
        assert!(maximum_absolute_wall_slope < 16.0);

        let mut rejected_friction = (1.0 - 1.0e-6) / maximum_absolute_wall_slope;
        while groove_friction_geometry_is_well_conditioned(
            rejected_friction,
            maximum_absolute_wall_slope,
        ) {
            rejected_friction = f64::from_bits(rejected_friction.to_bits() + 1);
        }
        let mut supported_friction = f64::from_bits(rejected_friction.to_bits() - 1);
        while !groove_friction_geometry_is_well_conditioned(
            supported_friction,
            maximum_absolute_wall_slope,
        ) {
            supported_friction = f64::from_bits(supported_friction.to_bits() - 1);
        }
        assert!(rejected_friction <= 2.0);

        let mut supported_profile = base_profile.clone();
        supported_profile.config.contact.groove_friction_coefficient = supported_friction;
        synchronize_profile_evidence(&mut supported_profile);
        let mut supported = PhysicalRecordPlayer::new(supported_profile).unwrap();
        supported.load_groove(Arc::clone(&groove)).unwrap();

        let mut rejected_profile = base_profile.clone();
        rejected_profile.config.contact.groove_friction_coefficient = rejected_friction;
        synchronize_profile_evidence(&mut rejected_profile);
        let mut rejected_contiguous = PhysicalRecordPlayer::new(rejected_profile.clone()).unwrap();
        let before_contiguous = rejected_contiguous.snapshot();
        assert!(matches!(
            rejected_contiguous.load_groove(Arc::clone(&groove)),
            Err(PhysicalRecordPlayerError::Pickup(
                crate::physical::PickupMechanicalError::IllConditionedGrooveWallFrictionGeometry
            ))
        ));
        assert_eq!(rejected_contiguous.snapshot(), before_contiguous);

        let mut rejected_paged = PhysicalRecordPlayer::new(rejected_profile.clone()).unwrap();
        let before_paged = rejected_paged.snapshot();
        assert!(matches!(
            rejected_paged.load_paged_groove(Arc::clone(&full_paged)),
            Err(PhysicalRecordPlayerError::Pickup(
                crate::physical::PickupMechanicalError::IllConditionedGrooveWallFrictionGeometry
            ))
        ));
        assert_eq!(rejected_paged.snapshot(), before_paged);

        rejected_paged
            .load_paged_groove(Arc::clone(&empty_paged))
            .unwrap();
        let before_replacement = rejected_paged.snapshot();
        assert!(matches!(
            rejected_paged.replace_loaded_paged_cache_snapshot(Arc::clone(&full_paged)),
            Err(PhysicalRecordPlayerError::Pickup(
                crate::physical::PickupMechanicalError::IllConditionedGrooveWallFrictionGeometry
            ))
        ));
        assert_eq!(rejected_paged.snapshot(), before_replacement);

        let realtime = realtime_cache_from_paged(
            &empty_paged,
            1,
            u32::try_from(groove.frame_count()).unwrap(),
        );
        let mut rejected_realtime = PhysicalRecordPlayer::new(rejected_profile).unwrap();
        rejected_realtime
            .load_realtime_paged_groove(realtime)
            .unwrap();
        publish_realtime_page(
            rejected_realtime.realtime_paged_cache_mut().unwrap(),
            full_paged.pages().next().unwrap(),
        );
        let after_publication = rejected_realtime.snapshot();
        let mut output = [23.0_f32; 2];
        assert!(matches!(
            rejected_realtime.render_internal_interleaved(&mut output),
            Err(PhysicalRecordPlayerError::Pickup(
                crate::physical::PickupMechanicalError::IllConditionedGrooveWallFrictionGeometry
            ))
        ));
        assert_eq!(output, [23.0_f32; 2]);
        assert_eq!(rejected_realtime.snapshot(), after_publication);
    }

    #[test]
    fn paged_load_rejects_an_insufficient_stylus_halo_without_mutation() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 733.0);
        let generation = GrooveGenerationId::new(74).unwrap();
        let metadata = PhysicalGrooveMetadata::from_groove_asset(generation, &groove, 1).unwrap();
        assert!(
            metadata
                .minimum_tracing_halo_frames(profile.config.stylus)
                .unwrap()
                > 1
        );
        let cache = Arc::new(
            PagedGrooveCacheProducer::new(metadata, PagedGrooveCacheLimits::default())
                .unwrap()
                .publish(),
        );
        let mut player = PhysicalRecordPlayer::new(profile).unwrap();
        let before = player.snapshot();
        assert!(matches!(
            player.load_paged_groove(cache),
            Err(PhysicalRecordPlayerError::PagedGroove(
                PagedGrooveError::InsufficientDeclaredTracingHalo { .. }
            ))
        ));
        assert_eq!(player.snapshot(), before);
        assert!(player.loaded_source_identity().is_none());
    }

    #[test]
    fn snapshots_require_the_exact_source_representation_and_generation() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 1_113.0);
        let cache = full_paged_cache(&profile, &groove, 75, 32_768);
        let mut contiguous = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        contiguous.load_groove(Arc::clone(&groove)).unwrap();
        contiguous.set_groove_frame_position(32_750.0).unwrap();
        contiguous.reset_transport(1.0, 1.0, 0.0, 0.0).unwrap();
        contiguous
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                PlayerControl::new(DeckMechanicalControl::default(), true),
            ))
            .unwrap();
        contiguous
            .render_internal_interleaved(&mut [0.0_f32; 16])
            .unwrap();
        let checkpoint = contiguous.snapshot();

        let mut paged = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        paged.load_paged_groove(Arc::clone(&cache)).unwrap();
        assert!(matches!(
            paged.restore(&checkpoint),
            Err(PhysicalRecordPlayerError::SnapshotSourceMismatch)
        ));

        let realtime_cache = realtime_cache_from_paged(&cache, 2, 40_000);
        let mut realtime = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        realtime.load_realtime_paged_groove(realtime_cache).unwrap();
        assert!(matches!(
            realtime.restore(&paged.snapshot()),
            Err(PhysicalRecordPlayerError::SnapshotSourceMismatch)
        ));
        let realtime_snapshot = realtime.snapshot();
        realtime.restore(&realtime_snapshot).unwrap();
        assert_eq!(
            realtime_snapshot.loaded_source_identity().unwrap().kind,
            PhysicalGrooveSourceKind::RealtimePaged
        );

        let mut source_renderer =
            PhysicalHostRenderer::new(profile.clone(), 48_000, TEST_HOST_OUTPUT).unwrap();
        source_renderer
            .load_paged_groove(Arc::clone(&cache))
            .unwrap();
        source_renderer.set_groove_frame_position(40_000.0).unwrap();
        source_renderer.reset_transport(1.0, 1.0, 0.0, 0.0).unwrap();
        source_renderer
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                PlayerControl::new(DeckMechanicalControl::default(), true),
            ))
            .unwrap();
        source_renderer
            .render_interleaved(&mut [0.0_f32; 64])
            .unwrap();
        let renderer_snapshot = source_renderer.snapshot();

        let mut exact =
            PhysicalHostRenderer::new(profile.clone(), 48_000, TEST_HOST_OUTPUT).unwrap();
        exact.load_paged_groove(cache).unwrap();
        exact.restore(&renderer_snapshot).unwrap();
        let mut expected_host = [0.0_f32; 128];
        let mut exact_host = [0.0_f32; 128];
        source_renderer
            .render_interleaved(&mut expected_host)
            .unwrap();
        exact.render_interleaved(&mut exact_host).unwrap();
        assert_eq!(exact_host, expected_host);
        assert_eq!(exact.snapshot(), source_renderer.snapshot());

        let mut wrong_representation =
            PhysicalHostRenderer::new(profile, 48_000, TEST_HOST_OUTPUT).unwrap();
        wrong_representation.load_groove(groove).unwrap();
        assert!(matches!(
            wrong_representation.restore(&renderer_snapshot),
            Err(PhysicalHostRendererError::SnapshotSourceMismatch)
        ));
    }

    #[test]
    fn paged_manifest_rejects_a_self_consistent_page_substitution() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 1_271.0);
        let core = GrooveFrameRange::new(0, 32_768).unwrap();
        let original = paged_cache_for_ranges(&profile, &groove, 175, &[core]);
        let original_page = original.pages().next().unwrap();
        let mut lateral = original_page.lateral_displacement_m().to_vec();
        let changed_index = 5_000;
        lateral[changed_index] = f32::from_bits(lateral[changed_index].to_bits() + 1);
        let altered_page = PhysicalGroovePage::new(
            original.metadata(),
            original_page.core_range(),
            original_page.stored_range(),
            lateral,
            original_page.vertical_displacement_m().to_vec(),
        )
        .unwrap();
        assert_ne!(
            altered_page.content_identity(),
            original_page.content_identity()
        );
        assert_ne!(
            altered_page
                .trace_admission_certificate()
                .unwrap()
                .certificate_identity(),
            original_page
                .trace_admission_certificate()
                .unwrap()
                .certificate_identity()
        );
        let mut producer = PagedGrooveCacheProducer::new(
            original.metadata(),
            PagedGrooveCacheLimits::new(1, 512 * 1_024 * 1_024).unwrap(),
        )
        .unwrap();
        producer.insert_page(altered_page).unwrap();
        let altered = Arc::new(producer.publish());
        assert_eq!(altered.content_identity(), original.content_identity());
        assert_eq!(altered.generation(), original.generation());
        assert_ne!(
            altered.trace_admitted_representation_identity(),
            original.trace_admitted_representation_identity()
        );

        let mut original_player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        original_player
            .load_paged_groove(Arc::clone(&original))
            .unwrap();
        let original_snapshot = original_player.snapshot();
        let mut altered_player = PhysicalRecordPlayer::new(profile).unwrap();
        altered_player
            .load_paged_groove(Arc::clone(&altered))
            .unwrap();
        let altered_snapshot = altered_player.snapshot();
        assert!(matches!(
            altered_player.restore(&original_snapshot),
            Err(PhysicalRecordPlayerError::SnapshotSourceMismatch)
        ));
        assert_eq!(altered_player.snapshot(), altered_snapshot);
        assert!(matches!(
            original_player.replace_loaded_paged_cache_snapshot(altered),
            Err(PhysicalRecordPlayerError::PagedCacheReplacementMismatch)
        ));
        assert_eq!(original_player.snapshot(), original_snapshot);
    }

    #[test]
    fn paged_publication_replacement_preserves_state_and_matches_a_full_cache() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = sine_groove(&profile, 1_427.0);
        let first_core = GrooveFrameRange::new(0, 32_768).unwrap();
        let second_core = GrooveFrameRange::new(32_768, 65_536).unwrap();
        let initial = paged_cache_for_ranges(&profile, &groove, 76, &[first_core]);
        let refreshed = paged_cache_for_ranges(&profile, &groove, 76, &[first_core, second_core]);
        let full = full_paged_cache(&profile, &groove, 76, 32_768);

        let mut incremental = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        let mut resident = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        incremental.load_paged_groove(Arc::clone(&initial)).unwrap();
        resident.load_paged_groove(full).unwrap();
        for player in [&mut incremental, &mut resident] {
            player.set_groove_frame_position(32_740.0).unwrap();
            player.reset_transport(1.0, 1.0, 0.0, 0.0).unwrap();
            player
                .enqueue_control(TimedPlayerControl::new(
                    0,
                    1,
                    PlayerControl::new(DeckMechanicalControl::default(), true),
                ))
                .unwrap();
        }
        let mut incremental_preroll = [0.0_f32; 32];
        let mut resident_preroll = [0.0_f32; 32];
        incremental
            .render_internal_interleaved(&mut incremental_preroll)
            .unwrap();
        resident
            .render_internal_interleaved(&mut resident_preroll)
            .unwrap();
        assert_eq!(incremental_preroll, resident_preroll);
        assert_player_state_matches_ignoring_source_representation(&incremental, &resident);
        assert_ne!(
            incremental.loaded_source_identity(),
            resident.loaded_source_identity()
        );

        let before_refresh = incremental.snapshot();
        let before_refresh_checkpoint = incremental.render_checkpoint();
        let old_cache = incremental
            .replace_loaded_paged_cache_snapshot(Arc::clone(&refreshed))
            .unwrap();
        assert!(Arc::ptr_eq(&old_cache, &initial));
        let after_refresh = incremental.snapshot();
        assert_ne!(after_refresh, before_refresh);
        assert_eq!(incremental.render_checkpoint(), before_refresh_checkpoint);
        assert!(matches!(
            incremental.restore(&before_refresh),
            Err(PhysicalRecordPlayerError::SnapshotSourceMismatch)
        ));
        assert_eq!(incremental.snapshot(), after_refresh);

        let mut incremental_forward = [0.0_f32; 96];
        let mut resident_forward = [0.0_f32; 96];
        incremental
            .render_internal_interleaved(&mut incremental_forward)
            .unwrap();
        resident
            .render_internal_interleaved(&mut resident_forward)
            .unwrap();
        assert_eq!(incremental_forward, resident_forward);
        incremental.reset_transport(-1.0, -1.0, 0.0, 0.0).unwrap();
        resident.reset_transport(-1.0, -1.0, 0.0, 0.0).unwrap();
        let mut incremental_reverse = [0.0_f32; 128];
        let mut resident_reverse = [0.0_f32; 128];
        incremental
            .render_internal_interleaved(&mut incremental_reverse)
            .unwrap();
        resident
            .render_internal_interleaved(&mut resident_reverse)
            .unwrap();
        assert_eq!(incremental_reverse, resident_reverse);
        assert_player_state_matches_ignoring_source_representation(&incremental, &resident);

        let continuity = incremental.snapshot();
        let mut restored = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        restored.load_paged_groove(Arc::clone(&refreshed)).unwrap();
        restored.restore(&continuity).unwrap();
        let mut expected = [0.0_f32; 64];
        let mut actual = [0.0_f32; 64];
        incremental
            .render_internal_interleaved(&mut expected)
            .unwrap();
        restored.render_internal_interleaved(&mut actual).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(restored.snapshot(), incremental.snapshot());

        let mismatch = paged_cache_for_ranges(&profile, &groove, 77, &[]);
        let before_mismatch = incremental.snapshot();
        assert!(matches!(
            incremental.replace_loaded_paged_cache_snapshot(mismatch),
            Err(PhysicalRecordPlayerError::PagedCacheReplacementMismatch)
        ));
        assert_eq!(incremental.snapshot(), before_mismatch);
        assert!(Arc::ptr_eq(
            incremental
                .source
                .as_ref()
                .unwrap()
                .as_paged_cache()
                .unwrap(),
            &refreshed
        ));

        let mut renderer = PhysicalHostRenderer::new(profile, 48_000, TEST_HOST_OUTPUT).unwrap();
        renderer.load_paged_groove(initial).unwrap();
        let renderer_before = renderer.snapshot();
        let renderer_identity_before = renderer.loaded_source_identity();
        let expected_refreshed_identity = refreshed.trace_admitted_representation_identity();
        let renderer_old = renderer
            .replace_loaded_paged_cache_snapshot(refreshed)
            .unwrap();
        assert_eq!(renderer_old.page_count(), 1);
        assert_ne!(renderer.snapshot(), renderer_before);
        assert_ne!(renderer.loaded_source_identity(), renderer_identity_before);
        assert_eq!(
            renderer
                .loaded_source_identity()
                .unwrap()
                .trace_admitted_representation_identity,
            expected_refreshed_identity
        );
    }

    #[test]
    fn prefetch_plan_covers_reversals_and_recapture_without_render_allocation() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let frames_per_turn = (profile.config.groove.groove_sample_rate_hz * 60.0
            / profile.config.groove.nominal_rpm)
            .round() as usize;
        let silence = vec![0.0_f32; frames_per_turn * 4 + 4];
        let groove = Arc::new(
            GrooveAsset::from_stereo_wall_velocity_m_s_with_cut(
                &silence,
                &silence,
                profile.config.groove,
                profile.config.record_cut,
            )
            .unwrap(),
        );
        let sparse = paged_cache_for_ranges(&profile, &groove, 78, &[]);
        let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
        player.load_paged_groove(Arc::clone(&sparse)).unwrap();
        let center = (frames_per_turn * 2) as f64;
        player.set_groove_frame_position(center).unwrap();
        player.reset_transport(20.0, 20.0, 0.0, 0.0).unwrap();

        let horizon = 2_048_u32;
        let plan = player
            .paged_prefetch_plan(horizon, &[-1, 1])
            .unwrap()
            .unwrap();
        assert_eq!(plan.generation(), sparse.generation());
        let contains = |position: f64| {
            let frame = position.floor() as u64;
            plan.core_ranges().iter().any(|range| range.contains(frame))
        };
        let maximum_travel = MAX_PAGED_GROOVE_RENDER_SPEED * f64::from(horizon);
        assert!(contains(center));
        assert!(contains(center - maximum_travel));
        assert!(contains(center + maximum_travel));
        assert!(contains(center - frames_per_turn as f64));
        assert!(contains(center + frames_per_turn as f64));

        let before_invalid = player.snapshot();
        let invalid_turn =
            i64::from(profile.config.radial_tracking.maximum_turns_per_recapture) + 1;
        assert!(matches!(
            player.paged_prefetch_plan(horizon, &[invalid_turn]),
            Err(PhysicalRecordPlayerError::InvalidRadialSelection)
        ));
        assert_eq!(player.snapshot(), before_invalid);

        let prefetched = paged_cache_for_ranges(&profile, &groove, 78, plan.core_ranges());
        let old = player
            .replace_loaded_paged_cache_snapshot(Arc::clone(&prefetched))
            .unwrap();
        assert!(Arc::ptr_eq(&old, &sparse));
        player
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                PlayerControl::new(DeckMechanicalControl::default(), true),
            ))
            .unwrap();
        let scratch_pointer = player.render_scratch.as_ptr();
        let cache_pointer = Arc::as_ptr(&prefetched);
        let page_count = prefetched.page_count();
        let resident_bytes = prefetched.resident_page_bytes();
        for quantum in 0..16 {
            let rate = if quantum % 2 == 0 { 20.0 } else { -20.0 };
            player.reset_transport(rate, rate, 0.0, 0.0).unwrap();
            let mut output = [0.0_f32; 256];
            player.render_internal_interleaved(&mut output).unwrap();
            assert!(output.iter().all(|sample| sample.is_finite()));
        }
        assert_eq!(player.render_scratch.as_ptr(), scratch_pointer);
        let loaded_cache = player.source.as_ref().unwrap().as_paged_cache().unwrap();
        assert_eq!(Arc::as_ptr(loaded_cache), cache_pointer);
        assert_eq!(loaded_cache.page_count(), page_count);
        assert_eq!(loaded_cache.resident_page_bytes(), resident_bytes);
    }

    #[test]
    fn paged_source_traces_both_adjacent_turn_recaptures() {
        let mut profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        profile.config.contact.groove_friction_coefficient = 0.0;
        profile.config.contact.record_surface_friction_coefficient = 0.0;
        profile.config.tonearm.lateral_bearing_static_friction_n = 0.0;
        profile.config.tonearm.lateral_bearing_kinetic_friction_n = 0.0;
        profile
            .config
            .tonearm
            .lateral_bearing_viscous_damping_n_s_per_m = 0.0;
        profile.config.radial_tracking.contact_release_margin_m = 0.1e-6;
        profile.config.radial_tracking.recapture_inset_m = 0.1e-6;
        profile.config.radial_tracking.maximum_turns_per_recapture = 4;
        synchronize_profile_evidence(&mut profile);

        let frames_per_turn = (profile.config.groove.groove_sample_rate_hz * 60.0
            / profile.config.groove.nominal_rpm)
            .round() as usize;
        let silence = vec![0.0_f32; frames_per_turn * 2 + 4];
        let groove = Arc::new(
            GrooveAsset::from_stereo_wall_velocity_m_s_with_cut(
                &silence,
                &silence,
                profile.config.groove,
                profile.config.record_cut,
            )
            .unwrap(),
        );
        let cache = full_paged_cache(&profile, &groove, 79, 32_768);
        let pitch_m = profile.config.record_cut.groove_pitch_m_per_revolution;
        let land_height_m = profile.config.record_cut.groove_top_width_m * 0.5;
        for target_turn in [1_i64, -1_i64] {
            let mut player = PhysicalRecordPlayer::new(profile.clone()).unwrap();
            player.load_paged_groove(Arc::clone(&cache)).unwrap();
            player
                .set_groove_frame_position(frames_per_turn as f64 + 2.0)
                .unwrap();
            player
                .enqueue_control(TimedPlayerControl::new(
                    0,
                    1,
                    PlayerControl::new(DeckMechanicalControl::default(), true),
                ))
                .unwrap();

            let release_m = 0.55 * target_turn as f64 * pitch_m;
            player
                .pickup
                .reset(
                    [release_m, land_height_m],
                    [0.0; 2],
                    [release_m, land_height_m],
                    [0.0; 2],
                )
                .unwrap();
            let mut output = [0.0_f32; 2];
            player.render_internal_interleaved(&mut output).unwrap();
            assert_eq!(
                player.telemetry().radial_tracking.contact_region,
                RadialContactRegion::Land
            );

            let capture_m = target_turn as f64 * pitch_m;
            player
                .pickup
                .reset(
                    [capture_m, land_height_m],
                    [0.0; 2],
                    [capture_m, land_height_m],
                    [0.0; 2],
                )
                .unwrap();
            player.render_internal_interleaved(&mut output).unwrap();
            let telemetry = player.telemetry();
            assert!(telemetry.radial_tracking.recaptured_this_step);
            assert_eq!(telemetry.radial_tracking.captured_turn_index, target_turn);
            assert_eq!(telemetry.radial_tracking.total_turns_skipped, target_turn);
            assert!(output.iter().all(|sample| sample.is_finite()));
        }
    }
}
