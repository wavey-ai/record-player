use crate::physical::{
    physical_input_frames_for_output_frames, GrooveAsset, GrooveContentIdentity, GrooveError,
    GrooveFrameRange, GrooveGenerationId, PagedGrooveCache, PagedGrooveCacheLimits,
    PagedGrooveCacheProducer, PagedGrooveError, PagedGrooveRenderMiss, PhysicalGrooveMetadata,
    PhysicalGroovePage, PhysicalGrooveSourceKind, PhysicalHostOutputConfig, PhysicalHostRenderer,
    PhysicalHostRendererError, PhysicalHostRendererSnapshot, PhysicalProfile,
    PhysicalRecordPlayerError, PickupContactSurface, RadialContactRegion, RealtimePagedGrooveCache,
    RealtimePagedGrooveCacheConfig, RealtimePagedGrooveChunkReservation, RealtimePagedGrooveError,
    RealtimePagedGrooveLevelLayout, RealtimePagedGroovePageDescriptor,
    RealtimePagedGroovePageFailure, RealtimePagedGroovePageProgress, RealtimePagedGroovePageTicket,
    RealtimePagedGroovePyramidInput, RealtimePagedGrooveSlotPhase, StreamingGrooveCutter,
    StreamingGrooveCutterConfig, StreamingGrooveCutterError, StreamingGrooveCutterProgress,
    StreamingGrooveCutterSnapshot, StreamingGrooveFinalization, StreamingGroovePageChunk,
    StreamingGroovePushResult, MAX_PAGED_GROOVE_PREFETCH_CANDIDATES, PHYSICAL_OUTPUT_INPUT_RATE_HZ,
    STREAMING_GROOVE_PAGE_CHUNK_FORMAT_VERSION, SUPPORTED_PHYSICAL_OUTPUT_RATES_HZ,
};
use crate::timed_control::{ControlTimelinePushError, PlayerControl, TimedPlayerControl};
use crate::{
    ContactMode, DeckMechanicalControl, MotorMode, PhysicalDeckConfig, PlayerConfig, PlayerEngine,
    PlayerEvent, ScheduledScratchHandControl, ScratchGestureConfig, ScratchGestureError,
    ScratchGestureMapper, ScratchGestureSnapshot, ScratchPointerSample,
};
use serde::Deserialize;
use std::sync::Arc;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WasmPlayerEngine {
    inner: PlayerEngine,
}

#[wasm_bindgen]
impl WasmPlayerEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue) -> Result<WasmPlayerEngine, JsValue> {
        let config: PlayerConfig = if config.is_null() || config.is_undefined() {
            PlayerConfig::default()
        } else {
            serde_wasm_bindgen::from_value(config)?
        };
        Ok(Self {
            inner: PlayerEngine::new(config),
        })
    }
    pub fn dispatch(&mut self, event: JsValue) -> Result<(), JsValue> {
        let event: PlayerEvent = serde_wasm_bindgen::from_value(event)?;
        self.inner
            .dispatch(event)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
    #[wasm_bindgen(js_name = drainCommands)]
    pub fn drain_commands(&mut self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.inner.drain_commands()).map_err(Into::into)
    }
    pub fn state(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(self.inner.state()).map_err(Into::into)
    }
    pub fn view(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.inner.view()).map_err(Into::into)
    }
    pub fn revision(&self) -> u64 {
        self.inner.revision()
    }
}

/// Numeric results from scalar scratch gesture calls.
#[wasm_bindgen(js_name = PhysicalScratchGestureStatus)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalScratchGestureStatus {
    Ok = 0,
    InvalidSample = 1,
    InvalidRecordAngle = 2,
    GestureAlreadyActive = 3,
    GestureNotActive = 4,
    PointerMismatch = 5,
    SourceTimeMovedBackward = 6,
    ScheduleOverflow = 7,
    CoreError = 8,
}

/// Exposes the canonical pointer-to-hand mapper to AudioWorklet clients.
#[wasm_bindgen(js_name = PhysicalScratchGestureMapper)]
pub struct WasmPhysicalScratchGestureMapper {
    inner: ScratchGestureMapper,
    last_scalar_result: Option<ScheduledScratchHandControl>,
}

#[wasm_bindgen(js_class = PhysicalScratchGestureMapper)]
impl WasmPhysicalScratchGestureMapper {
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue) -> Result<WasmPhysicalScratchGestureMapper, JsValue> {
        let config: ScratchGestureConfig = if config.is_null() || config.is_undefined() {
            ScratchGestureConfig::for_deck(PhysicalDeckConfig::default(), 192_000, 0)
                .map_err(js_error)?
        } else {
            serde_wasm_bindgen::from_value(config)?
        };
        Self::create(config).map_err(js_error)
    }

    #[wasm_bindgen(js_name = activePointerId)]
    pub fn active_pointer_id(&self) -> Option<u64> {
        self.inner.active_pointer_id()
    }

    #[wasm_bindgen(js_name = totalLateShiftFrames)]
    pub fn total_late_shift_frames(&self) -> u64 {
        self.inner.total_late_shift_frames()
    }

    /// Starts a gesture with scalar input and a cached scalar result.
    ///
    /// This call does not allocate.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = beginScalar)]
    pub fn begin_scalar(
        &mut self,
        pointer_id: u64,
        source_time_ns: u64,
        angle_rad: f64,
        contact_radius_m: f64,
        pressure_present: bool,
        normalized_pressure: f64,
        minimum_render_frame: u64,
        record_angle_rad: f64,
    ) -> WasmPhysicalScratchGestureStatus {
        let sample = scalar_pointer_sample(
            pointer_id,
            source_time_ns,
            angle_rad,
            contact_radius_m,
            pressure_present,
            normalized_pressure,
        );
        self.last_scalar_result = None;
        match self
            .inner
            .begin(sample, minimum_render_frame, record_angle_rad)
        {
            Ok(result) => {
                self.last_scalar_result = Some(result);
                WasmPhysicalScratchGestureStatus::Ok
            }
            Err(error) => scratch_gesture_error_status(&error),
        }
    }

    /// Updates a gesture with scalar input and a cached scalar result.
    ///
    /// This call does not allocate.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = updateScalar)]
    pub fn update_scalar(
        &mut self,
        pointer_id: u64,
        source_time_ns: u64,
        angle_rad: f64,
        contact_radius_m: f64,
        pressure_present: bool,
        normalized_pressure: f64,
        minimum_render_frame: u64,
    ) -> WasmPhysicalScratchGestureStatus {
        let sample = scalar_pointer_sample(
            pointer_id,
            source_time_ns,
            angle_rad,
            contact_radius_m,
            pressure_present,
            normalized_pressure,
        );
        self.last_scalar_result = None;
        match self.inner.update(sample, minimum_render_frame) {
            Ok(result) => {
                self.last_scalar_result = Some(result);
                WasmPhysicalScratchGestureStatus::Ok
            }
            Err(error) => scratch_gesture_error_status(&error),
        }
    }

    /// Finishes a gesture and caches one scalar release result.
    ///
    /// This call does not allocate.
    #[wasm_bindgen(js_name = finishScalar)]
    pub fn finish_scalar(
        &mut self,
        pointer_id: u64,
        source_time_ns: u64,
        minimum_render_frame: u64,
    ) -> WasmPhysicalScratchGestureStatus {
        self.last_scalar_result = None;
        match self
            .inner
            .finish(pointer_id, source_time_ns, minimum_render_frame)
        {
            Ok(result) => {
                self.last_scalar_result = Some(result);
                WasmPhysicalScratchGestureStatus::Ok
            }
            Err(error) => scratch_gesture_error_status(&error),
        }
    }

    #[wasm_bindgen(js_name = scalarResultPresent)]
    pub fn scalar_result_present(&self) -> bool {
        self.last_scalar_result.is_some()
    }

    #[wasm_bindgen(js_name = scalarAbsoluteFrame)]
    pub fn scalar_absolute_frame(&self) -> u64 {
        self.last_scalar_result
            .map_or(0, |result| result.absolute_frame)
    }

    #[wasm_bindgen(js_name = scalarPointerId)]
    pub fn scalar_pointer_id(&self) -> u64 {
        self.last_scalar_result
            .map_or(0, |result| result.pointer_id)
    }

    #[wasm_bindgen(js_name = scalarHandContact)]
    pub fn scalar_hand_contact(&self) -> bool {
        self.last_scalar_result
            .is_some_and(|result| result.hand_contact)
    }

    #[wasm_bindgen(js_name = scalarHandTargetAnglePresent)]
    pub fn scalar_hand_target_angle_present(&self) -> bool {
        self.last_scalar_result
            .is_some_and(|result| result.hand_target_angle_rad.is_some())
    }

    #[wasm_bindgen(js_name = scalarHandTargetAngleRad)]
    pub fn scalar_hand_target_angle_rad(&self) -> f64 {
        self.last_scalar_result
            .and_then(|result| result.hand_target_angle_rad)
            .unwrap_or(0.0)
    }

    #[wasm_bindgen(js_name = scalarHandTargetAngularVelocityRadS)]
    pub fn scalar_hand_target_angular_velocity_rad_s(&self) -> f64 {
        self.last_scalar_result
            .map_or(0.0, |result| result.hand_target_angular_velocity_rad_s)
    }

    #[wasm_bindgen(js_name = scalarHandNormalForceN)]
    pub fn scalar_hand_normal_force_n(&self) -> f64 {
        self.last_scalar_result
            .map_or(0.0, |result| result.hand_normal_force_n)
    }

    #[wasm_bindgen(js_name = scalarHandContactRadiusM)]
    pub fn scalar_hand_contact_radius_m(&self) -> f64 {
        self.last_scalar_result
            .map_or(0.0, |result| result.hand_contact_radius_m)
    }

    #[wasm_bindgen(js_name = scalarRawPointerAngularVelocityRadS)]
    pub fn scalar_raw_pointer_angular_velocity_rad_s(&self) -> f64 {
        self.last_scalar_result
            .map_or(0.0, |result| result.raw_pointer_angular_velocity_rad_s)
    }

    #[wasm_bindgen(js_name = scalarVelocityWasLimited)]
    pub fn scalar_velocity_was_limited(&self) -> bool {
        self.last_scalar_result
            .is_some_and(|result| result.velocity_was_limited)
    }

    #[wasm_bindgen(js_name = scalarWrapWasAmbiguous)]
    pub fn scalar_wrap_was_ambiguous(&self) -> bool {
        self.last_scalar_result
            .is_some_and(|result| result.wrap_was_ambiguous)
    }

    #[wasm_bindgen(js_name = scalarAddedLateShiftFrames)]
    pub fn scalar_added_late_shift_frames(&self) -> u64 {
        self.last_scalar_result
            .map_or(0, |result| result.added_late_shift_frames)
    }

    #[wasm_bindgen(js_name = scalarTotalLateShiftFrames)]
    pub fn scalar_total_late_shift_frames(&self) -> u64 {
        self.last_scalar_result
            .map_or(0, |result| result.total_late_shift_frames)
    }

    /// Serializes one mapped control for lifecycle diagnostics.
    pub fn begin(
        &mut self,
        sample: JsValue,
        minimum_render_frame: u64,
        record_angle_rad: f64,
    ) -> Result<JsValue, JsValue> {
        let sample: ScratchPointerSample = serde_wasm_bindgen::from_value(sample)?;
        let output = self
            .inner
            .begin(sample, minimum_render_frame, record_angle_rad)
            .map_err(js_error)?;
        serde_wasm_bindgen::to_value(&output).map_err(Into::into)
    }

    pub fn update(
        &mut self,
        sample: JsValue,
        minimum_render_frame: u64,
    ) -> Result<JsValue, JsValue> {
        let sample: ScratchPointerSample = serde_wasm_bindgen::from_value(sample)?;
        let output = self
            .inner
            .update(sample, minimum_render_frame)
            .map_err(js_error)?;
        serde_wasm_bindgen::to_value(&output).map_err(Into::into)
    }

    pub fn finish(
        &mut self,
        pointer_id: u64,
        source_time_ns: u64,
        minimum_render_frame: u64,
    ) -> Result<JsValue, JsValue> {
        let output = self
            .inner
            .finish(pointer_id, source_time_ns, minimum_render_frame)
            .map_err(js_error)?;
        serde_wasm_bindgen::to_value(&output).map_err(Into::into)
    }

    /// Serializes mapper state for lifecycle storage.
    pub fn snapshot(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.inner.snapshot()).map_err(Into::into)
    }

    pub fn restore(&mut self, snapshot: JsValue) -> Result<(), JsValue> {
        let snapshot: ScratchGestureSnapshot = serde_wasm_bindgen::from_value(snapshot)?;
        self.inner.restore(&snapshot).map_err(js_error)?;
        self.last_scalar_result = None;
        Ok(())
    }
}

impl WasmPhysicalScratchGestureMapper {
    fn create(config: ScratchGestureConfig) -> Result<Self, ScratchGestureError> {
        Ok(Self {
            inner: ScratchGestureMapper::new(config)?,
            last_scalar_result: None,
        })
    }
}

fn scalar_pointer_sample(
    pointer_id: u64,
    source_time_ns: u64,
    angle_rad: f64,
    contact_radius_m: f64,
    pressure_present: bool,
    normalized_pressure: f64,
) -> ScratchPointerSample {
    ScratchPointerSample {
        pointer_id,
        source_time_ns,
        angle_rad,
        contact_radius_m,
        normalized_pressure: pressure_present.then_some(normalized_pressure),
    }
}

fn scratch_gesture_error_status(error: &ScratchGestureError) -> WasmPhysicalScratchGestureStatus {
    match error {
        ScratchGestureError::InvalidSample { .. } => {
            WasmPhysicalScratchGestureStatus::InvalidSample
        }
        ScratchGestureError::InvalidRecordAngle => {
            WasmPhysicalScratchGestureStatus::InvalidRecordAngle
        }
        ScratchGestureError::GestureAlreadyActive => {
            WasmPhysicalScratchGestureStatus::GestureAlreadyActive
        }
        ScratchGestureError::GestureNotActive => WasmPhysicalScratchGestureStatus::GestureNotActive,
        ScratchGestureError::PointerMismatch { .. } => {
            WasmPhysicalScratchGestureStatus::PointerMismatch
        }
        ScratchGestureError::SourceTimeMovedBackward => {
            WasmPhysicalScratchGestureStatus::SourceTimeMovedBackward
        }
        ScratchGestureError::ScheduleOverflow => WasmPhysicalScratchGestureStatus::ScheduleOverflow,
        _ => WasmPhysicalScratchGestureStatus::CoreError,
    }
}

/// Numeric results from worker-only streaming cutter calls.
#[wasm_bindgen(js_name = StreamingGrooveCutterStatus)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmStreamingGrooveCutterStatus {
    Ok = 0,
    PageReady = 1,
    PagePending = 2,
    Finalized = 3,
    InvalidConfiguration = 4,
    UnexpectedSourceFrame = 5,
    SourceChannelMismatch = 6,
    SourceChannelLengthMismatch = 7,
    SourceRangeInvalid = 8,
    NonfiniteProgramme = 9,
    IncompleteSource = 10,
    AlreadyFinished = 11,
    InvalidSnapshot = 12,
    CoreError = 13,
}

/// Owns one bounded raw page emitted by `StreamingGrooveCutter`.
#[wasm_bindgen(js_name = StreamingGroovePageChunk)]
pub struct WasmStreamingGroovePageChunk {
    inner: StreamingGroovePageChunk,
}

#[wasm_bindgen(js_class = StreamingGroovePageChunk)]
impl WasmStreamingGroovePageChunk {
    #[wasm_bindgen(js_name = formatVersion)]
    pub fn format_version(&self) -> u32 {
        self.inner.format_version()
    }

    #[wasm_bindgen(js_name = totalFrameCount)]
    pub fn total_frame_count(&self) -> u64 {
        self.inner.total_frame_count()
    }

    #[wasm_bindgen(js_name = storageHaloFrames)]
    pub fn storage_halo_frames(&self) -> u32 {
        self.inner.storage_halo_frames()
    }

    #[wasm_bindgen(js_name = coreStartFrame)]
    pub fn core_start_frame(&self) -> u64 {
        self.inner.core_start_frame()
    }

    #[wasm_bindgen(js_name = coreEndFrameExclusive)]
    pub fn core_end_frame_exclusive(&self) -> u64 {
        self.inner.core_end_frame_exclusive()
    }

    #[wasm_bindgen(js_name = storedStartFrame)]
    pub fn stored_start_frame(&self) -> u64 {
        self.inner.stored_start_frame()
    }

    #[wasm_bindgen(js_name = storedEndFrameExclusive)]
    pub fn stored_end_frame_exclusive(&self) -> u64 {
        self.inner.stored_end_frame_exclusive()
    }

    #[wasm_bindgen(js_name = storedFrameCount)]
    pub fn stored_frame_count(&self) -> u32 {
        u32::try_from(self.inner.lateral_displacement_m().len()).unwrap_or(u32::MAX)
    }

    /// Returns the zero-copy lateral channel address in WASM memory.
    #[wasm_bindgen(js_name = lateralDisplacementPtr)]
    pub fn lateral_displacement_ptr(&self) -> *const f32 {
        self.inner.lateral_displacement_m().as_ptr()
    }

    /// Returns the zero-copy vertical channel address in WASM memory.
    #[wasm_bindgen(js_name = verticalDisplacementPtr)]
    pub fn vertical_displacement_ptr(&self) -> *const f32 {
        self.inner.vertical_displacement_m().as_ptr()
    }

    /// Serializes this raw page for loading or storage tools.
    #[wasm_bindgen(js_name = toValue)]
    pub fn to_value(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.inner).map_err(Into::into)
    }

    /// Adds final metadata and constructs the canonical spatial pyramid.
    #[wasm_bindgen(js_name = materialize)]
    pub fn materialize(self, metadata: JsValue) -> Result<WasmPhysicalGroovePage, JsValue> {
        let metadata: PhysicalGrooveMetadata = serde_wasm_bindgen::from_value(metadata)?;
        let inner = self.inner.into_physical_page(metadata).map_err(js_error)?;
        Ok(WasmPhysicalGroovePage { inner })
    }
}

/// Owns one canonical page and its complete spatial pyramid in worker memory.
#[wasm_bindgen(js_name = PhysicalGroovePage)]
pub struct WasmPhysicalGroovePage {
    inner: PhysicalGroovePage,
}

#[wasm_bindgen(js_class = PhysicalGroovePage)]
impl WasmPhysicalGroovePage {
    #[wasm_bindgen(js_name = generation)]
    pub fn generation(&self) -> u64 {
        self.inner.generation().get()
    }

    #[wasm_bindgen(js_name = coreStartFrame)]
    pub fn core_start_frame(&self) -> u64 {
        self.inner.core_range().start_frame()
    }

    #[wasm_bindgen(js_name = coreEndFrameExclusive)]
    pub fn core_end_frame_exclusive(&self) -> u64 {
        self.inner.core_range().end_frame_exclusive()
    }

    #[wasm_bindgen(js_name = storedStartFrame)]
    pub fn stored_start_frame(&self) -> u64 {
        self.inner.stored_range().start_frame()
    }

    #[wasm_bindgen(js_name = storedEndFrameExclusive)]
    pub fn stored_end_frame_exclusive(&self) -> u64 {
        self.inner.stored_range().end_frame_exclusive()
    }

    #[wasm_bindgen(js_name = storedFrameCount)]
    pub fn stored_frame_count(&self) -> u32 {
        self.inner.lateral_displacement_m().len() as u32
    }

    #[wasm_bindgen(js_name = lateralDisplacementPtr)]
    pub fn lateral_displacement_ptr(&self) -> *const f32 {
        self.inner.lateral_displacement_m().as_ptr()
    }

    #[wasm_bindgen(js_name = verticalDisplacementPtr)]
    pub fn vertical_displacement_ptr(&self) -> *const f32 {
        self.inner.vertical_displacement_m().as_ptr()
    }

    #[wasm_bindgen(js_name = assetIdentityVersion)]
    pub fn asset_identity_version(&self) -> u32 {
        self.inner.asset_content_identity().identity_version()
    }

    /// Returns one big-endian 32-bit asset-identity word.
    #[wasm_bindgen(js_name = assetIdentityWord)]
    pub fn asset_identity_word(&self, index: u32) -> u32 {
        sha256_word(self.inner.asset_content_identity().sha256(), index)
    }

    #[wasm_bindgen(js_name = contentIdentityVersion)]
    pub fn content_identity_version(&self) -> u32 {
        self.inner.content_identity().identity_version()
    }

    /// Returns one big-endian 32-bit page-identity word.
    #[wasm_bindgen(js_name = contentIdentityWord)]
    pub fn content_identity_word(&self, index: u32) -> u32 {
        sha256_word(self.inner.content_identity().sha256(), index)
    }

    #[wasm_bindgen(js_name = spatialLevelCount)]
    pub fn spatial_level_count(&self) -> u32 {
        self.inner
            .spatial_pyramid()
            .map_or(0, |pyramid| pyramid.levels().len() as u32)
    }

    #[wasm_bindgen(js_name = spatialLevelFirstSourceFrame)]
    pub fn spatial_level_first_source_frame(&self, level_index: u32) -> u64 {
        self.spatial_level(level_index)
            .map_or(0, |level| level.first_source_frame())
    }

    #[wasm_bindgen(js_name = spatialLevelSourceFrameStep)]
    pub fn spatial_level_source_frame_step(&self, level_index: u32) -> u32 {
        self.spatial_level(level_index)
            .map_or(0, |level| level.source_frame_step())
    }

    #[wasm_bindgen(js_name = spatialLevelFrameCount)]
    pub fn spatial_level_frame_count(&self, level_index: u32) -> u32 {
        self.spatial_level(level_index)
            .map_or(0, |level| level.lateral_displacement_m().len() as u32)
    }

    #[wasm_bindgen(js_name = spatialLevelLateralPtr)]
    pub fn spatial_level_lateral_ptr(&self, level_index: u32) -> *const f32 {
        self.spatial_level(level_index)
            .map_or(std::ptr::null(), |level| {
                level.lateral_displacement_m().as_ptr()
            })
    }

    #[wasm_bindgen(js_name = spatialLevelVerticalPtr)]
    pub fn spatial_level_vertical_ptr(&self, level_index: u32) -> *const f32 {
        self.spatial_level(level_index)
            .map_or(std::ptr::null(), |level| {
                level.vertical_displacement_m().as_ptr()
            })
    }

    #[wasm_bindgen(js_name = residentSizeBytes)]
    pub fn resident_size_bytes(&self) -> u64 {
        self.inner.resident_size_bytes()
    }
}

impl WasmPhysicalGroovePage {
    fn spatial_level(
        &self,
        level_index: u32,
    ) -> Option<&crate::physical::groove::GrooveSpatialLevel> {
        self.inner
            .spatial_pyramid()?
            .levels()
            .get(level_index as usize)
    }
}

/// Numeric results from bounded worker page materialization.
#[wasm_bindgen(js_name = PhysicalGroovePageMaterializerStatus)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalGroovePageMaterializerStatus {
    Ok = 0,
    InvalidMetadata = 1,
    InvalidCapacity = 2,
    InvalidFormatVersion = 3,
    InvalidRange = 4,
    PageTooLarge = 5,
    PagePending = 6,
    NoPreparedPage = 7,
    BuildFailed = 8,
    MemoryViewGenerationExhausted = 9,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WasmPreparedRawGroovePage {
    core_range: GrooveFrameRange,
    stored_range: GrooveFrameRange,
}

/// Reimports and materializes one canonical page at a time in a worker.
#[wasm_bindgen(js_name = PhysicalGroovePageMaterializer)]
pub struct WasmPhysicalGroovePageMaterializer {
    metadata: PhysicalGrooveMetadata,
    lateral_displacement_m: Box<[f32]>,
    vertical_displacement_m: Box<[f32]>,
    prepared: Option<WasmPreparedRawGroovePage>,
    materialized: Option<PhysicalGroovePage>,
    memory_view_generation: u64,
}

#[wasm_bindgen(js_class = PhysicalGroovePageMaterializer)]
impl WasmPhysicalGroovePageMaterializer {
    /// Allocates fixed raw-page staging storage in a worker WASM instance.
    #[wasm_bindgen(constructor)]
    pub fn new(
        metadata: JsValue,
        maximum_stored_frame_count: u32,
    ) -> Result<WasmPhysicalGroovePageMaterializer, JsValue> {
        let metadata: PhysicalGrooveMetadata = serde_wasm_bindgen::from_value(metadata)?;
        metadata.validate().map_err(js_error)?;
        if !(4..=MAXIMUM_WASM_MATERIALIZED_PAGE_FRAMES).contains(&maximum_stored_frame_count) {
            return Err(JsValue::from_str(
                "maximum stored frame count is outside the worker limit",
            ));
        }
        Ok(Self {
            metadata,
            lateral_displacement_m: vec![0.0; maximum_stored_frame_count as usize]
                .into_boxed_slice(),
            vertical_displacement_m: vec![0.0; maximum_stored_frame_count as usize]
                .into_boxed_slice(),
            prepared: None,
            materialized: None,
            memory_view_generation: 1,
        })
    }

    /// Prepares fixed storage for one raw streaming page.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = prepareRawPage)]
    pub fn prepare_raw_page(
        &mut self,
        format_version: u32,
        total_frame_count: u64,
        storage_halo_frames: u32,
        core_start_frame: u64,
        core_end_frame_exclusive: u64,
        stored_start_frame: u64,
        stored_end_frame_exclusive: u64,
    ) -> WasmPhysicalGroovePageMaterializerStatus {
        if self.materialized.is_some() {
            return WasmPhysicalGroovePageMaterializerStatus::PagePending;
        }
        self.prepared = None;
        if format_version != STREAMING_GROOVE_PAGE_CHUNK_FORMAT_VERSION {
            return WasmPhysicalGroovePageMaterializerStatus::InvalidFormatVersion;
        }
        if total_frame_count != self.metadata.total_frame_count()
            || storage_halo_frames != self.metadata.required_storage_halo_frames()
        {
            return WasmPhysicalGroovePageMaterializerStatus::InvalidMetadata;
        }
        let core_range = match GrooveFrameRange::new(core_start_frame, core_end_frame_exclusive) {
            Ok(range) => range,
            Err(_) => return WasmPhysicalGroovePageMaterializerStatus::InvalidRange,
        };
        let stored_range =
            match GrooveFrameRange::new(stored_start_frame, stored_end_frame_exclusive) {
                Ok(range) => range,
                Err(_) => return WasmPhysicalGroovePageMaterializerStatus::InvalidRange,
            };
        let halo = u64::from(storage_halo_frames);
        if stored_range.start_frame() > core_range.start_frame()
            || stored_range.end_frame_exclusive() < core_range.end_frame_exclusive()
            || core_range.end_frame_exclusive() > total_frame_count
            || stored_range.start_frame() != core_range.start_frame().saturating_sub(halo)
            || stored_range.end_frame_exclusive()
                != core_range
                    .end_frame_exclusive()
                    .saturating_add(halo)
                    .min(total_frame_count)
        {
            return WasmPhysicalGroovePageMaterializerStatus::InvalidRange;
        }
        if stored_range.frame_count() > self.lateral_displacement_m.len() as u64 {
            return WasmPhysicalGroovePageMaterializerStatus::PageTooLarge;
        }
        self.prepared = Some(WasmPreparedRawGroovePage {
            core_range,
            stored_range,
        });
        WasmPhysicalGroovePageMaterializerStatus::Ok
    }

    #[wasm_bindgen(js_name = maximumStoredFrameCount)]
    pub fn maximum_stored_frame_count(&self) -> u32 {
        self.lateral_displacement_m.len() as u32
    }

    #[wasm_bindgen(js_name = preparedFrameCount)]
    pub fn prepared_frame_count(&self) -> u32 {
        self.prepared
            .map_or(0, |prepared| prepared.stored_range.frame_count() as u32)
    }

    #[wasm_bindgen(js_name = lateralStagingPtr)]
    pub fn lateral_staging_ptr(&mut self) -> *mut f32 {
        self.lateral_displacement_m.as_mut_ptr()
    }

    #[wasm_bindgen(js_name = verticalStagingPtr)]
    pub fn vertical_staging_ptr(&mut self) -> *mut f32 {
        self.vertical_displacement_m.as_mut_ptr()
    }

    #[wasm_bindgen(js_name = memoryViewGeneration)]
    pub fn memory_view_generation(&self) -> u64 {
        self.memory_view_generation
    }

    #[wasm_bindgen(js_name = materializedPageReady)]
    pub fn materialized_page_ready(&self) -> bool {
        self.materialized.is_some()
    }

    /// Builds one canonical page and its complete spatial pyramid.
    #[wasm_bindgen(js_name = materializePreparedPage)]
    pub fn materialize_prepared_page(&mut self) -> WasmPhysicalGroovePageMaterializerStatus {
        if self.materialized.is_some() {
            return WasmPhysicalGroovePageMaterializerStatus::PagePending;
        }
        let Some(prepared) = self.prepared else {
            return WasmPhysicalGroovePageMaterializerStatus::NoPreparedPage;
        };
        let Some(next_generation) = self.memory_view_generation.checked_add(1) else {
            return WasmPhysicalGroovePageMaterializerStatus::MemoryViewGenerationExhausted;
        };
        self.memory_view_generation = next_generation;
        let frame_count = prepared.stored_range.frame_count() as usize;
        let page = PhysicalGroovePage::new(
            self.metadata,
            prepared.core_range,
            prepared.stored_range,
            self.lateral_displacement_m[..frame_count].to_vec(),
            self.vertical_displacement_m[..frame_count].to_vec(),
        );
        match page {
            Ok(page) => {
                self.materialized = Some(page);
                self.prepared = None;
                WasmPhysicalGroovePageMaterializerStatus::Ok
            }
            Err(_) => WasmPhysicalGroovePageMaterializerStatus::BuildFailed,
        }
    }

    /// Transfers the one canonical page to its zero-copy export wrapper.
    #[wasm_bindgen(js_name = takeMaterializedPage)]
    pub fn take_materialized_page(&mut self) -> Option<WasmPhysicalGroovePage> {
        self.materialized
            .take()
            .map(|inner| WasmPhysicalGroovePage { inner })
    }
}

const WASM_STREAMING_GROOVE_CUTTER_SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WasmStreamingGrooveCutterSnapshot {
    version: u32,
    cutter: StreamingGrooveCutterSnapshot,
    emitted_page: Option<StreamingGroovePageChunk>,
    finalization: Option<StreamingGrooveFinalization>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct WasmStreamingGrooveCutterSnapshotRef<'a> {
    version: u32,
    cutter: StreamingGrooveCutterSnapshot,
    emitted_page: &'a Option<StreamingGroovePageChunk>,
    finalization: Option<StreamingGrooveFinalization>,
}

/// Cuts sequential planar PCM without retaining a complete record.
///
/// Use this class in a loading worker. Do not use it on a render thread.
#[wasm_bindgen(js_name = StreamingGrooveCutter)]
pub struct WasmStreamingGrooveCutter {
    inner: StreamingGrooveCutter,
    last_progress: StreamingGrooveCutterProgress,
    last_consumed_source_frame_count: u64,
    emitted_page: Option<StreamingGroovePageChunk>,
    finalization: Option<StreamingGrooveFinalization>,
    s16_left: Vec<f32>,
    s16_right: Vec<f32>,
}

#[wasm_bindgen(js_class = StreamingGrooveCutter)]
impl WasmStreamingGrooveCutter {
    /// Creates a worker cutter from one complete serialized configuration.
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue) -> Result<WasmStreamingGrooveCutter, JsValue> {
        let config: StreamingGrooveCutterConfig = serde_wasm_bindgen::from_value(config)?;
        Self::create(config).map_err(js_error)
    }

    /// Creates a serialized configuration for the built-in physical profile.
    #[wasm_bindgen(js_name = createSeedConfig)]
    pub fn create_seed_config(
        source_sample_rate_hz: f64,
        source_channel_count: u32,
        total_source_frame_count: u64,
        page_core_frame_count: u32,
        tracing_halo_frames: u32,
    ) -> Result<JsValue, JsValue> {
        let source_channel_count = u8::try_from(source_channel_count)
            .map_err(|_| JsValue::from_str("source channel count is outside the u8 range"))?;
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let config = StreamingGrooveCutterConfig {
            source_sample_rate_hz,
            source_channel_count,
            total_source_frame_count,
            layout: profile.config.groove,
            cut: profile.config.record_cut,
            page_core_frame_count,
            tracing_halo_frames,
        }
        .validate()
        .map_err(js_error)?;
        serde_wasm_bindgen::to_value(&config).map_err(Into::into)
    }

    /// Returns the exact seed-stylus halo for one declared source length.
    #[wasm_bindgen(js_name = seedMinimumTracingHaloFrames)]
    pub fn seed_minimum_tracing_halo_frames(
        source_sample_rate_hz: f64,
        total_source_frame_count: u64,
    ) -> Result<u32, JsValue> {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let config = StreamingGrooveCutterConfig {
            source_sample_rate_hz,
            source_channel_count: 1,
            total_source_frame_count,
            layout: profile.config.groove,
            cut: profile.config.record_cut,
            page_core_frame_count: 1,
            tracing_halo_frames: 1,
        }
        .validate()
        .map_err(js_error)?;
        let final_frame = config
            .expected_output_frame_count()
            .map_err(js_error)?
            .saturating_sub(1) as f64;
        let meters_per_source_frame = config
            .layout
            .meters_per_frame_at(final_frame, config.cut.groove_pitch_m_per_revolution);
        let maximum_level_step = 1_u32 << crate::physical::groove::GROOVE_SPATIAL_PYRAMID_LEVELS;
        Ok(profile
            .config
            .stylus
            .multiresolution_support(meters_per_source_frame, maximum_level_step)
            .map_err(js_error)?
            .symmetric_halo_source_frames())
    }

    /// Returns the validated configuration.
    pub fn config(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.inner.config()).map_err(Into::into)
    }

    #[wasm_bindgen(js_name = expectedOutputFrameCount)]
    pub fn expected_output_frame_count(&self) -> u64 {
        self.last_progress.total_output_frame_count()
    }

    #[wasm_bindgen(js_name = maximumSourceHistoryFrameCount)]
    pub fn maximum_source_history_frame_count(&self) -> u64 {
        self.inner.config().maximum_source_history_frame_count()
    }

    #[wasm_bindgen(js_name = maximumClearanceHistoryFrameCount)]
    pub fn maximum_clearance_history_frame_count(&self) -> u64 {
        self.inner
            .config()
            .maximum_clearance_history_frame_count()
            .unwrap_or(u64::MAX)
    }

    #[wasm_bindgen(js_name = maximumPendingPageFrameCount)]
    pub fn maximum_pending_page_frame_count(&self) -> u64 {
        self.inner
            .config()
            .maximum_pending_page_frame_count()
            .unwrap_or(u64::MAX)
    }

    /// Pushes a sequential planar `Float32Array` source prefix.
    ///
    /// Pass an empty right channel for mono. Read `lastConsumedSourceFrameCount`
    /// before resubmitting the unconsumed suffix.
    #[wasm_bindgen(js_name = pushPlanarF32)]
    pub fn push_planar_f32(
        &mut self,
        absolute_source_frame: u64,
        left: &[f32],
        right: &[f32],
    ) -> WasmStreamingGrooveCutterStatus {
        self.last_consumed_source_frame_count = 0;
        if self.emitted_page.is_some() {
            return WasmStreamingGrooveCutterStatus::PagePending;
        }
        if self.finalization.is_some() {
            return WasmStreamingGrooveCutterStatus::AlreadyFinished;
        }
        if let Some(status) = self.validate_planar_channels(left.len(), right.len()) {
            return status;
        }
        let result = if self.inner.config().source_channel_count == 1 {
            self.inner
                .push_chunk_until_page(absolute_source_frame, &[left])
        } else {
            self.inner
                .push_chunk_until_page(absolute_source_frame, &[left, right])
        };
        match result {
            Ok(result) => self.accept_push_result(result),
            Err(error) => streaming_cutter_error_status(&error),
        }
    }

    /// Pushes a sequential planar signed 16-bit source prefix.
    ///
    /// The conversion divides each sample by 32,768. Pass an empty right
    /// channel for mono.
    #[wasm_bindgen(js_name = pushPlanarS16)]
    pub fn push_planar_s16(
        &mut self,
        absolute_source_frame: u64,
        left: &[i16],
        right: &[i16],
    ) -> WasmStreamingGrooveCutterStatus {
        self.last_consumed_source_frame_count = 0;
        if self.emitted_page.is_some() {
            return WasmStreamingGrooveCutterStatus::PagePending;
        }
        if self.finalization.is_some() {
            return WasmStreamingGrooveCutterStatus::AlreadyFinished;
        }
        if let Some(status) = self.validate_planar_channels(left.len(), right.len()) {
            return status;
        }
        normalize_s16_channel(left, &mut self.s16_left);
        normalize_s16_channel(right, &mut self.s16_right);
        let result = if self.inner.config().source_channel_count == 1 {
            self.inner
                .push_chunk_until_page(absolute_source_frame, &[self.s16_left.as_slice()])
        } else {
            self.inner.push_chunk_until_page(
                absolute_source_frame,
                &[self.s16_left.as_slice(), self.s16_right.as_slice()],
            )
        };
        match result {
            Ok(result) => self.accept_push_result(result),
            Err(error) => streaming_cutter_error_status(&error),
        }
    }

    /// Drains one ready page or completes finalization.
    ///
    /// Take a pending page before calling this method again.
    pub fn finish(&mut self) -> WasmStreamingGrooveCutterStatus {
        self.last_consumed_source_frame_count = 0;
        if self.emitted_page.is_some() {
            return WasmStreamingGrooveCutterStatus::PagePending;
        }
        if self.finalization.is_some() {
            return WasmStreamingGrooveCutterStatus::AlreadyFinished;
        }
        let absolute_source_frame = self.last_progress.accepted_source_frame_count();
        let empty = &[][..];
        let result = if self.inner.config().source_channel_count == 1 {
            self.inner
                .push_chunk_until_page(absolute_source_frame, &[empty])
        } else {
            self.inner
                .push_chunk_until_page(absolute_source_frame, &[empty, empty])
        };
        match result {
            Ok(result) if result.emitted_page().is_some() => {
                return self.accept_push_result(result);
            }
            Ok(result) => {
                self.last_progress = result.progress();
            }
            Err(error) => return streaming_cutter_error_status(&error),
        }
        match self.inner.finish() {
            Ok(finalization) => {
                self.finalization = Some(finalization);
                self.last_progress = self.inner.progress();
                WasmStreamingGrooveCutterStatus::Finalized
            }
            Err(error) => streaming_cutter_error_status(&error),
        }
    }

    #[wasm_bindgen(js_name = lastConsumedSourceFrameCount)]
    pub fn last_consumed_source_frame_count(&self) -> u64 {
        self.last_consumed_source_frame_count
    }

    #[wasm_bindgen(js_name = emittedPageReady)]
    pub fn emitted_page_ready(&self) -> bool {
        self.emitted_page.is_some()
    }

    /// Transfers ownership of the one pending page to JavaScript.
    #[wasm_bindgen(js_name = takeEmittedPage)]
    pub fn take_emitted_page(&mut self) -> Option<WasmStreamingGroovePageChunk> {
        self.emitted_page
            .take()
            .map(|inner| WasmStreamingGroovePageChunk { inner })
    }

    #[wasm_bindgen(js_name = acceptedSourceFrameCount)]
    pub fn accepted_source_frame_count(&self) -> u64 {
        self.last_progress.accepted_source_frame_count()
    }

    #[wasm_bindgen(js_name = totalSourceFrameCount)]
    pub fn total_source_frame_count(&self) -> u64 {
        self.last_progress.total_source_frame_count()
    }

    #[wasm_bindgen(js_name = producedOutputFrameCount)]
    pub fn produced_output_frame_count(&self) -> u64 {
        self.last_progress.produced_output_frame_count()
    }

    #[wasm_bindgen(js_name = emittedPageCount)]
    pub fn emitted_page_count(&self) -> u64 {
        self.last_progress.emitted_page_count()
    }

    #[wasm_bindgen(js_name = sourceHistoryFrameCount)]
    pub fn source_history_frame_count(&self) -> u64 {
        self.last_progress.source_history_frame_count()
    }

    #[wasm_bindgen(js_name = clearanceHistoryFrameCount)]
    pub fn clearance_history_frame_count(&self) -> u64 {
        self.last_progress.clearance_history_frame_count()
    }

    #[wasm_bindgen(js_name = pendingPageFrameCount)]
    pub fn pending_page_frame_count(&self) -> u64 {
        self.last_progress.pending_page_frame_count()
    }

    #[wasm_bindgen(js_name = progressFinished)]
    pub fn progress_finished(&self) -> bool {
        self.last_progress.is_finished()
    }

    #[wasm_bindgen(js_name = lateralPrefixIdentityVersion)]
    pub fn lateral_prefix_identity_version(&self) -> u32 {
        self.last_progress
            .lateral_prefix_identity()
            .identity_version()
    }

    #[wasm_bindgen(js_name = lateralPrefixIdentityByte)]
    pub fn lateral_prefix_identity_byte(&self, index: u32) -> u32 {
        self.last_progress
            .lateral_prefix_identity()
            .sha256()
            .get(index as usize)
            .copied()
            .unwrap_or(0)
            .into()
    }

    #[wasm_bindgen(js_name = verticalPrefixIdentityVersion)]
    pub fn vertical_prefix_identity_version(&self) -> u32 {
        self.last_progress
            .vertical_prefix_identity()
            .identity_version()
    }

    #[wasm_bindgen(js_name = verticalPrefixIdentityByte)]
    pub fn vertical_prefix_identity_byte(&self, index: u32) -> u32 {
        self.last_progress
            .vertical_prefix_identity()
            .sha256()
            .get(index as usize)
            .copied()
            .unwrap_or(0)
            .into()
    }

    #[wasm_bindgen(js_name = finalizationPresent)]
    pub fn finalization_present(&self) -> bool {
        self.finalization.is_some()
    }

    #[wasm_bindgen(js_name = finalOutputFrameCount)]
    pub fn final_output_frame_count(&self) -> u64 {
        self.finalization
            .map_or(0, StreamingGrooveFinalization::output_frame_count)
    }

    #[wasm_bindgen(js_name = finalContentIdentityVersion)]
    pub fn final_content_identity_version(&self) -> u32 {
        self.finalization.map_or(0, |finalization| {
            finalization.content_identity().identity_version()
        })
    }

    #[wasm_bindgen(js_name = finalContentIdentityByte)]
    pub fn final_content_identity_byte(&self, index: u32) -> u32 {
        self.finalization
            .map(|finalization| finalization.content_identity().sha256())
            .and_then(|sha256| sha256.get(index as usize).copied())
            .unwrap_or(0)
            .into()
    }

    #[wasm_bindgen(js_name = finalReport)]
    pub fn final_report(&self) -> Result<JsValue, JsValue> {
        let Some(finalization) = self.finalization else {
            return Err(JsValue::from_str("the streaming cut is not finalized"));
        };
        serde_wasm_bindgen::to_value(&finalization.report()).map_err(Into::into)
    }

    #[wasm_bindgen(js_name = finalMetadata)]
    pub fn final_metadata(&self, generation: u64) -> Result<JsValue, JsValue> {
        let Some(finalization) = self.finalization else {
            return Err(JsValue::from_str("the streaming cut is not finalized"));
        };
        let generation = GrooveGenerationId::new(generation).map_err(js_error)?;
        let metadata = finalization
            .physical_metadata(generation)
            .map_err(js_error)?;
        serde_wasm_bindgen::to_value(&metadata).map_err(Into::into)
    }

    /// Serializes bounded continuation state and an optional pending page.
    pub fn snapshot(&self) -> Result<JsValue, JsValue> {
        let snapshot = WasmStreamingGrooveCutterSnapshotRef {
            version: WASM_STREAMING_GROOVE_CUTTER_SNAPSHOT_VERSION,
            cutter: self.inner.snapshot(),
            emitted_page: &self.emitted_page,
            finalization: self.finalization,
        };
        serde_wasm_bindgen::to_value(&snapshot).map_err(Into::into)
    }

    /// Validates and restores bounded continuation state transactionally.
    pub fn restore(&mut self, snapshot: JsValue) -> Result<(), JsValue> {
        let snapshot: WasmStreamingGrooveCutterSnapshot = serde_wasm_bindgen::from_value(snapshot)?;
        self.restore_state(snapshot).map_err(JsValue::from_str)
    }
}

impl WasmStreamingGrooveCutter {
    fn create(config: StreamingGrooveCutterConfig) -> Result<Self, StreamingGrooveCutterError> {
        let inner = StreamingGrooveCutter::new(config)?;
        let last_progress = inner.progress();
        Ok(Self {
            inner,
            last_progress,
            last_consumed_source_frame_count: 0,
            emitted_page: None,
            finalization: None,
            s16_left: Vec::new(),
            s16_right: Vec::new(),
        })
    }

    fn validate_planar_channels(
        &self,
        left_length: usize,
        right_length: usize,
    ) -> Option<WasmStreamingGrooveCutterStatus> {
        match self.inner.config().source_channel_count {
            1 if right_length != 0 => Some(WasmStreamingGrooveCutterStatus::SourceChannelMismatch),
            2 if left_length != right_length => {
                Some(WasmStreamingGrooveCutterStatus::SourceChannelLengthMismatch)
            }
            1 | 2 => None,
            _ => Some(WasmStreamingGrooveCutterStatus::InvalidConfiguration),
        }
    }

    fn accept_push_result(
        &mut self,
        result: StreamingGroovePushResult,
    ) -> WasmStreamingGrooveCutterStatus {
        self.last_consumed_source_frame_count = result.consumed_source_frame_count();
        self.last_progress = result.progress();
        self.emitted_page = result.into_emitted_page();
        if self.emitted_page.is_some() {
            WasmStreamingGrooveCutterStatus::PageReady
        } else {
            WasmStreamingGrooveCutterStatus::Ok
        }
    }

    #[cfg(test)]
    fn owned_snapshot(&self) -> WasmStreamingGrooveCutterSnapshot {
        WasmStreamingGrooveCutterSnapshot {
            version: WASM_STREAMING_GROOVE_CUTTER_SNAPSHOT_VERSION,
            cutter: self.inner.snapshot(),
            emitted_page: self.emitted_page.clone(),
            finalization: self.finalization,
        }
    }

    fn restore_state(
        &mut self,
        snapshot: WasmStreamingGrooveCutterSnapshot,
    ) -> Result<(), &'static str> {
        let inner = validate_streaming_cutter_snapshot(&snapshot)?;
        let last_progress = inner.progress();
        self.inner = inner;
        self.last_progress = last_progress;
        self.last_consumed_source_frame_count = 0;
        self.emitted_page = snapshot.emitted_page;
        self.finalization = snapshot.finalization;
        self.s16_left.clear();
        self.s16_right.clear();
        Ok(())
    }
}

fn normalize_s16_channel(input: &[i16], output: &mut Vec<f32>) {
    output.clear();
    output.reserve(input.len());
    output.extend(input.iter().map(|sample| f32::from(*sample) / 32_768.0));
}

fn validate_streaming_cutter_snapshot(
    snapshot: &WasmStreamingGrooveCutterSnapshot,
) -> Result<StreamingGrooveCutter, &'static str> {
    if snapshot.version != WASM_STREAMING_GROOVE_CUTTER_SNAPSHOT_VERSION {
        return Err("the WASM streaming cutter snapshot version is not supported");
    }
    let cutter = StreamingGrooveCutter::from_snapshot(&snapshot.cutter)
        .map_err(|_| "the streaming cutter snapshot is invalid")?;
    let config = cutter.config();
    let progress = cutter.progress();
    if let Some(page) = &snapshot.emitted_page {
        validate_streaming_page_snapshot(config, progress, page)?;
    }
    match snapshot.finalization {
        Some(finalization) => {
            if snapshot.emitted_page.is_some()
                || !progress.is_finished()
                || finalization.config() != config
                || finalization.output_frame_count() != progress.total_output_frame_count()
                || finalization
                    .physical_metadata(
                        GrooveGenerationId::new(1)
                            .map_err(|_| "the final generation is invalid")?,
                    )
                    .is_err()
            {
                return Err("the streaming cutter finalization is invalid");
            }
        }
        None if progress.is_finished() => {
            return Err("the finalized cutter snapshot has no finalization");
        }
        None => {}
    }
    Ok(cutter)
}

fn validate_streaming_page_snapshot(
    config: StreamingGrooveCutterConfig,
    progress: StreamingGrooveCutterProgress,
    page: &StreamingGroovePageChunk,
) -> Result<(), &'static str> {
    let expected_total = config
        .expected_output_frame_count()
        .map_err(|_| "the streaming cutter configuration is invalid")?;
    let expected_storage_halo = config
        .storage_halo_frames()
        .map_err(|_| "the streaming cutter configuration is invalid")?;
    let stored_length = page
        .stored_end_frame_exclusive()
        .checked_sub(page.stored_start_frame())
        .ok_or("the pending page range is invalid")?;
    let expected_core_start = progress
        .emitted_page_count()
        .checked_sub(1)
        .and_then(|page_index| page_index.checked_mul(u64::from(config.page_core_frame_count)))
        .ok_or("the pending page index is invalid")?;
    if page.format_version() != STREAMING_GROOVE_PAGE_CHUNK_FORMAT_VERSION
        || page.total_frame_count() != expected_total
        || page.storage_halo_frames() != expected_storage_halo
        || page.core_start_frame() != expected_core_start
        || page.core_start_frame() >= page.core_end_frame_exclusive()
        || page.core_end_frame_exclusive() > expected_total
        || page.stored_start_frame() > page.core_start_frame()
        || page.stored_end_frame_exclusive() < page.core_end_frame_exclusive()
        || page.stored_end_frame_exclusive() > expected_total
        || stored_length != page.lateral_displacement_m().len() as u64
        || page.lateral_displacement_m().len() != page.vertical_displacement_m().len()
        || page
            .lateral_displacement_m()
            .iter()
            .chain(page.vertical_displacement_m())
            .any(|sample| !sample.is_finite())
    {
        return Err("the pending raw page is invalid");
    }
    let halo = u64::from(expected_storage_halo);
    if page.stored_start_frame() != page.core_start_frame().saturating_sub(halo)
        || page.stored_end_frame_exclusive()
            != page
                .core_end_frame_exclusive()
                .saturating_add(halo)
                .min(expected_total)
    {
        return Err("the pending raw page halo is invalid");
    }
    Ok(())
}

fn streaming_cutter_error_status(
    error: &StreamingGrooveCutterError,
) -> WasmStreamingGrooveCutterStatus {
    match error {
        StreamingGrooveCutterError::InvalidConfiguration
        | StreamingGrooveCutterError::UnsupportedGrooveSampleRate
        | StreamingGrooveCutterError::UnsupportedSourceSampleRate
        | StreamingGrooveCutterError::InvalidSourceChannelCount
        | StreamingGrooveCutterError::InvalidTotalSourceFrameCount
        | StreamingGrooveCutterError::InvalidOutputFrameCount
        | StreamingGrooveCutterError::InvalidPageCoreFrameCount
        | StreamingGrooveCutterError::InvalidTracingHaloFrameCount
        | StreamingGrooveCutterError::ClearanceHistoryTooLarge => {
            WasmStreamingGrooveCutterStatus::InvalidConfiguration
        }
        StreamingGrooveCutterError::UnexpectedSourceFrame { .. } => {
            WasmStreamingGrooveCutterStatus::UnexpectedSourceFrame
        }
        StreamingGrooveCutterError::SourceChannelCountMismatch => {
            WasmStreamingGrooveCutterStatus::SourceChannelMismatch
        }
        StreamingGrooveCutterError::SourceChannelLengthMismatch => {
            WasmStreamingGrooveCutterStatus::SourceChannelLengthMismatch
        }
        StreamingGrooveCutterError::SourceFrameRangeOverflow
        | StreamingGrooveCutterError::SourceExceedsDeclaredLength => {
            WasmStreamingGrooveCutterStatus::SourceRangeInvalid
        }
        StreamingGrooveCutterError::NonfiniteProgramme => {
            WasmStreamingGrooveCutterStatus::NonfiniteProgramme
        }
        StreamingGrooveCutterError::IncompleteSource { .. } => {
            WasmStreamingGrooveCutterStatus::IncompleteSource
        }
        StreamingGrooveCutterError::AlreadyFinished => {
            WasmStreamingGrooveCutterStatus::AlreadyFinished
        }
        StreamingGrooveCutterError::UnsupportedSnapshotVersion { .. }
        | StreamingGrooveCutterError::SnapshotConfigMismatch
        | StreamingGrooveCutterError::InvalidSnapshot => {
            WasmStreamingGrooveCutterStatus::InvalidSnapshot
        }
        _ => WasmStreamingGrooveCutterStatus::CoreError,
    }
}

/// Numeric motor modes accepted by `PhysicalHostRenderer.enqueueTimedControl`.
#[wasm_bindgen(js_name = PhysicalMotorMode)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalMotorMode {
    Off = 0,
    Servo = 1,
    Brake = 2,
}

/// Numeric results from a complete timed-control submission.
#[wasm_bindgen(js_name = PhysicalControlStatus)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalControlStatus {
    Ok = 0,
    InvalidMotorMode = 1,
    InvalidControl = 2,
    Late = 3,
    Duplicate = 4,
    NonmonotonicFrame = 5,
    NonmonotonicSequence = 6,
    QueueFull = 7,
    CoreError = 8,
}

/// Numeric results from a host-rate render call.
#[wasm_bindgen(js_name = PhysicalRenderStatus)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalRenderStatus {
    Ok = 0,
    BlockTooLarge = 1,
    PageMiss = 2,
    SourceUnavailable = 3,
    CoreError = 4,
}

/// Numeric results from a source or transport lifecycle call.
#[wasm_bindgen(js_name = PhysicalSourceStatus)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalSourceStatus {
    Ok = 0,
    InvalidArgument = 1,
    GrooveCutFailed = 2,
    GrooveLayoutMismatch = 3,
    GrooveProgrammeDoesNotFit = 4,
    GrooveOvercut = 5,
    GrooveLoadFailed = 6,
    InvalidPosition = 7,
    TransportResetFailed = 8,
    MemoryViewGenerationExhausted = 9,
}

/// Reports whether paged-source lifecycle calls are available.
#[wasm_bindgen(js_name = PhysicalPagedSourceApiStatus)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalPagedSourceApiStatus {
    UnavailablePendingCanonicalApi = 0,
    Available = 1,
}

/// Numeric results from paged-source lifecycle calls.
#[wasm_bindgen(js_name = PhysicalPagedSourceStatus)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalPagedSourceStatus {
    Ok = 0,
    InvalidMetadata = 1,
    InvalidCacheLimits = 2,
    NoCacheProducer = 3,
    InvalidPage = 4,
    CacheLimitExceeded = 5,
    NoPublishedCache = 6,
    InvalidGeneration = 7,
    LoadFailed = 8,
    MemoryViewGenerationExhausted = 9,
    NoLoadedPagedSource = 10,
    RefreshMismatch = 11,
    PrefetchFailed = 12,
    PreparedPageTooLarge = 13,
    NoPreparedPage = 14,
}

/// Numeric results from fixed real-time page-cache calls.
#[wasm_bindgen(js_name = PhysicalRealtimePagedStatus)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalRealtimePagedStatus {
    Ok = 0,
    NoLoadedCache = 1,
    InvalidMetadata = 2,
    InvalidCacheConfig = 3,
    CacheAllocationFailed = 4,
    LoadFailed = 5,
    MemoryViewGenerationExhausted = 6,
    InvalidIdentity = 7,
    InvalidRange = 8,
    NoEmptySlot = 9,
    StaleTicket = 10,
    WrongPhase = 11,
    ChunkReservationActive = 12,
    StaleChunkReservation = 13,
    InvalidChunk = 14,
    NonfiniteDisplacement = 15,
    IncompletePage = 16,
    InvalidSpatialLevel = 17,
    PrecomputedPyramidNotRequested = 18,
    InvalidWorkBudget = 19,
    PageRejected = 20,
    CannotDiscardPublishedPage = 21,
    PageHasStagingDependency = 22,
    CoreError = 23,
}

/// Numeric fixed-cache page phases returned by scalar progress getters.
#[wasm_bindgen(js_name = PhysicalRealtimePagedPhase)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalRealtimePagedPhase {
    None = 0,
    Empty = 1,
    Receiving = 2,
    BuildingPyramid = 3,
    Hashing = 4,
    ValidatingSeams = 5,
    Ready = 6,
    Published = 7,
    Rejected = 8,
    CertifyingTrace = 9,
}

/// Identifies how one fixed-cache page receives its spatial pyramid.
#[wasm_bindgen(js_name = PhysicalRealtimePagedPyramidInput)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalRealtimePagedPyramidInput {
    None = 0,
    BuildFromBase = 1,
    Precomputed = 2,
}

/// Identifies the last completed fixed-cache validation failure.
#[wasm_bindgen(js_name = PhysicalRealtimePagedFailure)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalRealtimePagedFailure {
    None = 0,
    PageContentIdentityMismatch = 1,
    SeamSampleMismatch = 2,
    MissingTraceAdmissionCertificate = 3,
    TraceAdmissionCertificateMismatch = 4,
    SpatialSeamSampleMismatch = 5,
    NoncanonicalSpatialPyramid = 6,
    TraceAdmissionNotAdmitted = 7,
}

/// Identifies the last paged render miss without serialization.
#[wasm_bindgen(js_name = PhysicalPageMissKind)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalPageMissKind {
    None = 0,
    StaleGeneration = 1,
    PageUnavailable = 2,
}

/// Identifies the current source representation.
#[wasm_bindgen(js_name = PhysicalSourceKind)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalSourceKind {
    None = 0,
    Contiguous = 1,
    Paged = 2,
    RealtimePaged = 3,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct WasmPagedPrefetchRange {
    start_frame: u64,
    end_frame_exclusive: u64,
}

const MAXIMUM_WASM_PREPARED_PAGE_FRAMES: u32 = 1_048_576;
const MAXIMUM_WASM_MATERIALIZED_PAGE_FRAMES: u32 = 4 * 1_024 * 1_024;
const MAXIMUM_WASM_REALTIME_PAGE_SLOTS: usize = 64;
const MAXIMUM_WASM_REALTIME_CHUNK_FRAMES: usize = 64 * 1_024;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WasmPhysicalHostRendererOptions {
    volts_per_full_scale: f64,
    #[serde(default)]
    profile: Option<PhysicalProfile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WasmRealtimePagedChunkTarget {
    BaseLateral,
    BaseVertical,
    SpatialLevelLateral(u8),
    SpatialLevelVertical(u8),
}

/// Numeric contact modes returned by the deck telemetry getters.
#[wasm_bindgen(js_name = PhysicalContactMode)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalContactMode {
    Separated = 0,
    Sticking = 1,
    SlidingPositive = 2,
    SlidingNegative = 3,
}

/// Numeric pickup surfaces returned by `telemetryPickupContactSurface`.
#[wasm_bindgen(js_name = PhysicalPickupContactSurface)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalPickupContactSurface {
    None = 0,
    GrooveWalls = 1,
    RecordLand = 2,
}

/// Numeric radial regions returned by `telemetryRadialContactRegion`.
#[wasm_bindgen(js_name = PhysicalRadialContactRegion)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPhysicalRadialContactRegion {
    Groove = 0,
    Land = 1,
    Lifted = 2,
}

/// Owns the canonical physical player and one fixed host-output buffer.
///
/// `render` does not allocate on its successful path. It writes interleaved
/// stereo samples into the prefix reported by `outputFrameCount`.
#[wasm_bindgen(js_name = PhysicalHostRenderer)]
pub struct WasmPhysicalHostRenderer {
    inner: PhysicalHostRenderer,
    output: Box<[f32]>,
    output_frame_count: u32,
    memory_view_generation: u64,
    source_identity_present: bool,
    source_identity_version: u32,
    source_identity_sha256: [u8; 32],
    paged_cache_producer: Option<PagedGrooveCacheProducer>,
    published_paged_cache: Option<Arc<PagedGrooveCache>>,
    last_page_miss_kind: WasmPhysicalPageMissKind,
    last_page_miss_frame: u64,
    last_page_miss_requested_generation: u64,
    last_page_miss_cached_generation: u64,
    paged_prefetch_ranges: Box<[WasmPagedPrefetchRange]>,
    paged_prefetch_range_count: u32,
    paged_prefetch_generation: u64,
    paged_prefetch_available: bool,
    prepared_page_lateral_displacement_m: Box<[f32]>,
    prepared_page_vertical_displacement_m: Box<[f32]>,
    realtime_paged_tickets:
        [Option<RealtimePagedGroovePageTicket>; MAXIMUM_WASM_REALTIME_PAGE_SLOTS],
    realtime_paged_reservations:
        [Option<RealtimePagedGrooveChunkReservation>; MAXIMUM_WASM_REALTIME_PAGE_SLOTS],
    realtime_paged_chunk_staging: Box<[f32]>,
    realtime_paged_last_ticket: Option<RealtimePagedGroovePageTicket>,
    realtime_paged_last_progress: Option<RealtimePagedGroovePageProgress>,
    realtime_paged_last_level_layout: Option<RealtimePagedGrooveLevelLayout>,
    realtime_paged_last_eviction_found: bool,
}

#[wasm_bindgen(js_class = PhysicalHostRenderer)]
impl WasmPhysicalHostRenderer {
    /// Tests whether the physical output converter supports one host rate.
    #[wasm_bindgen(js_name = supportsOutputSampleRate)]
    pub fn supports_output_sample_rate(output_sample_rate_hz: u32) -> bool {
        SUPPORTED_PHYSICAL_OUTPUT_RATES_HZ.contains(&output_sample_rate_hz)
    }

    /// Creates a renderer for one supported host rate.
    ///
    /// `options.voltsPerFullScale` is required. It defines the explicit host
    /// level boundary. `options.profile` can contain a complete profile.
    #[wasm_bindgen(constructor)]
    pub fn new(
        output_sample_rate_hz: u32,
        options: JsValue,
    ) -> Result<WasmPhysicalHostRenderer, JsValue> {
        if options.is_null() || options.is_undefined() {
            return Err(JsValue::from_str(
                "PhysicalHostRenderer options must define voltsPerFullScale",
            ));
        }
        let options: WasmPhysicalHostRendererOptions = serde_wasm_bindgen::from_value(options)?;
        Self::create(
            output_sample_rate_hz,
            options.profile,
            PhysicalHostOutputConfig {
                volts_per_full_scale: options.volts_per_full_scale,
            },
        )
        .map_err(js_error)
    }

    /// Returns the physical solver rate.
    #[wasm_bindgen(js_name = internalSampleRateHz)]
    pub fn internal_sample_rate_hz(&self) -> u32 {
        PHYSICAL_OUTPUT_INPUT_RATE_HZ
    }

    /// Returns the selected host output rate.
    #[wasm_bindgen(js_name = outputSampleRateHz)]
    pub fn output_sample_rate_hz(&self) -> u32 {
        self.inner.output_sample_rate_hz()
    }

    /// Returns the phono voltage that maps to host full scale.
    #[wasm_bindgen(js_name = outputVoltsPerFullScale)]
    pub fn output_volts_per_full_scale(&self) -> f64 {
        self.inner.host_output_config().volts_per_full_scale
    }

    #[wasm_bindgen(js_name = hostOutputPeakUnclippedLeftV)]
    pub fn host_output_peak_unclipped_left_v(&self) -> f64 {
        self.inner
            .host_output_telemetry()
            .peak_unclipped_abs_output_v[0]
    }

    #[wasm_bindgen(js_name = hostOutputPeakUnclippedRightV)]
    pub fn host_output_peak_unclipped_right_v(&self) -> f64 {
        self.inner
            .host_output_telemetry()
            .peak_unclipped_abs_output_v[1]
    }

    #[wasm_bindgen(js_name = hostOutputClippedLeftSamples)]
    pub fn host_output_clipped_left_samples(&self) -> u64 {
        self.inner.host_output_telemetry().clipped_samples[0]
    }

    #[wasm_bindgen(js_name = hostOutputClippedRightSamples)]
    pub fn host_output_clipped_right_samples(&self) -> u64 {
        self.inner.host_output_telemetry().clipped_samples[1]
    }

    #[wasm_bindgen(js_name = hostOutputTotalClippedLeftSamples)]
    pub fn host_output_total_clipped_left_samples(&self) -> u64 {
        self.inner.host_output_telemetry().total_clipped_samples[0]
    }

    #[wasm_bindgen(js_name = hostOutputTotalClippedRightSamples)]
    pub fn host_output_total_clipped_right_samples(&self) -> u64 {
        self.inner.host_output_telemetry().total_clipped_samples[1]
    }

    /// Returns the largest host block accepted by `render`.
    #[wasm_bindgen(js_name = maximumRenderFrames)]
    pub fn maximum_render_frames(&self) -> u32 {
        self.output.len() as u32 / 2
    }

    /// Returns the fixed output allocation address in WASM linear memory.
    ///
    /// Recreate the JavaScript view when `memoryViewGeneration` changes.
    #[wasm_bindgen(js_name = outputBufferPtr)]
    pub fn output_buffer_ptr(&self) -> *const f32 {
        self.output.as_ptr()
    }

    /// Returns the fixed output allocation length in `f32` samples.
    #[wasm_bindgen(js_name = outputBufferLen)]
    pub fn output_buffer_len(&self) -> u32 {
        self.output.len() as u32
    }

    /// Returns the valid host frames from the last successful render.
    #[wasm_bindgen(js_name = outputFrameCount)]
    pub fn output_frame_count(&self) -> u32 {
        self.output_frame_count
    }

    /// Changes after a lifecycle call that can grow WASM memory.
    #[wasm_bindgen(js_name = memoryViewGeneration)]
    pub fn memory_view_generation(&self) -> u64 {
        self.memory_view_generation
    }

    #[wasm_bindgen(js_name = currentInternalFrame)]
    pub fn current_internal_frame(&self) -> u64 {
        self.inner.current_internal_frame()
    }

    #[wasm_bindgen(js_name = renderedHostFrames)]
    pub fn rendered_host_frames(&self) -> u64 {
        self.inner.rendered_host_frames()
    }

    #[wasm_bindgen(js_name = latencyInternalFrames)]
    pub fn latency_internal_frames(&self) -> u32 {
        self.inner.latency_internal_frames() as u32
    }

    #[wasm_bindgen(js_name = latencySeconds)]
    pub fn latency_seconds(&self) -> f64 {
        self.inner.latency_seconds()
    }

    /// Converts a future host offset to one exact 192 kHz frame.
    #[wasm_bindgen(js_name = internalFrameAfterHostFrames)]
    pub fn internal_frame_after_host_frames(&self, host_frame_offset: u64) -> Result<u64, JsValue> {
        let target_host_frame = self
            .inner
            .rendered_host_frames()
            .checked_add(host_frame_offset)
            .ok_or_else(|| JsValue::from_str("host frame offset overflows"))?;
        physical_input_frames_for_output_frames(self.output_sample_rate_hz(), target_host_frame)
            .map_err(js_error)
    }

    /// Enqueues one complete control state at an exact 192 kHz frame.
    ///
    /// This call does not allocate. `absoluteFrame` and `sequence` are JS
    /// `bigint` values in the generated bindings.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = enqueueTimedControl)]
    pub fn enqueue_timed_control(
        &mut self,
        absolute_frame: u64,
        sequence: u64,
        motor_mode: u32,
        motor_target_angular_velocity_rad_s: f64,
        hand_contact: bool,
        hand_target_angle_present: bool,
        hand_target_angle_rad: f64,
        hand_target_angular_velocity_rad_s: f64,
        hand_normal_force_n: f64,
        hand_contact_radius_m: f64,
        stylus_lowered: bool,
    ) -> WasmPhysicalControlStatus {
        let control = match complete_control_from_scalars(
            motor_mode,
            motor_target_angular_velocity_rad_s,
            hand_contact,
            hand_target_angle_present,
            hand_target_angle_rad,
            hand_target_angular_velocity_rad_s,
            hand_normal_force_n,
            hand_contact_radius_m,
            0.0,
            stylus_lowered,
        ) {
            Ok(control) => control,
            Err(status) => return status,
        };
        if control
            .deck
            .validate_for_config(self.inner.profile().config.deck)
            .is_err()
        {
            return WasmPhysicalControlStatus::InvalidControl;
        }
        match self
            .inner
            .enqueue_control(TimedPlayerControl::new(absolute_frame, sequence, control))
        {
            Ok(()) => {
                self.clear_paged_prefetch_plan();
                WasmPhysicalControlStatus::Ok
            }
            Err(error) => control_error_status(error),
        }
    }

    /// Renders a prefix of the fixed output buffer.
    ///
    /// A failed call sets `outputFrameCount` to zero. It does not change the
    /// renderer state or the previous buffer contents.
    pub fn render(&mut self, frame_count: u32) -> WasmPhysicalRenderStatus {
        self.output_frame_count = 0;
        self.clear_page_miss();
        self.clear_paged_prefetch_plan();
        let frame_count = frame_count as usize;
        if frame_count > self.output.len() / 2 {
            return WasmPhysicalRenderStatus::BlockTooLarge;
        }
        let sample_count = frame_count * 2;
        match self
            .inner
            .render_interleaved(&mut self.output[..sample_count])
        {
            Ok(_) => {
                self.output_frame_count = frame_count as u32;
                WasmPhysicalRenderStatus::Ok
            }
            Err(error) => self.record_render_error(&error),
        }
    }

    /// Cuts and loads one complete mono or stereo PCM programme.
    ///
    /// This call allocates. Recreate cached WASM views after this call.
    #[wasm_bindgen(js_name = loadInterleavedPcm)]
    pub fn load_interleaved_pcm(
        &mut self,
        interleaved_pcm: &[f32],
        channel_count: u32,
        source_sample_rate_hz: f64,
    ) -> WasmPhysicalSourceStatus {
        if !self.advance_memory_view_generation() {
            return WasmPhysicalSourceStatus::MemoryViewGenerationExhausted;
        }
        let groove =
            match self.cut_interleaved_pcm(interleaved_pcm, channel_count, source_sample_rate_hz) {
                Ok(groove) => groove,
                Err(status) => return status,
            };
        match self.inner.load_groove(Arc::new(groove)) {
            Ok(_) => {
                self.sync_source_identity();
                self.clear_paged_prefetch_plan();
                WasmPhysicalSourceStatus::Ok
            }
            Err(error) => source_player_error_status(error),
        }
    }

    /// Removes the active source.
    #[wasm_bindgen(js_name = unloadGroove)]
    pub fn unload_groove(&mut self) {
        self.inner.unload_groove();
        self.sync_source_identity();
        self.clear_paged_prefetch_plan();
    }

    /// Sets the source groove frame position.
    #[wasm_bindgen(js_name = setGrooveFramePosition)]
    pub fn set_groove_frame_position(&mut self, position: f64) -> WasmPhysicalSourceStatus {
        match self.inner.set_groove_frame_position(position) {
            Ok(()) => {
                self.clear_paged_prefetch_plan();
                WasmPhysicalSourceStatus::Ok
            }
            Err(error) => source_player_error_status(error),
        }
    }

    /// Resets the deck bodies without changing the renderer clock.
    #[wasm_bindgen(js_name = resetTransport)]
    pub fn reset_transport(
        &mut self,
        platter_rate: f64,
        record_rate: f64,
        platter_angle_turns: f64,
        record_angle_turns: f64,
    ) -> WasmPhysicalSourceStatus {
        match self.inner.reset_transport(
            platter_rate,
            record_rate,
            platter_angle_turns,
            record_angle_turns,
        ) {
            Ok(()) => {
                self.clear_paged_prefetch_plan();
                WasmPhysicalSourceStatus::Ok
            }
            Err(_) => WasmPhysicalSourceStatus::TransportResetFailed,
        }
    }

    #[wasm_bindgen(js_name = sourceIdentityPresent)]
    pub fn source_identity_present(&self) -> bool {
        self.source_identity_present
    }

    #[wasm_bindgen(js_name = sourceIdentityVersion)]
    pub fn source_identity_version(&self) -> u32 {
        self.source_identity_version
    }

    /// Gets one SHA-256 identity byte. An invalid index returns zero.
    #[wasm_bindgen(js_name = sourceIdentityByte)]
    pub fn source_identity_byte(&self, index: u32) -> u32 {
        self.source_identity_sha256
            .get(index as usize)
            .copied()
            .unwrap_or(0)
            .into()
    }

    /// Returns the loaded source representation.
    #[wasm_bindgen(js_name = sourceKind)]
    pub fn source_kind(&self) -> WasmPhysicalSourceKind {
        match self.inner.loaded_source_identity().map(|value| value.kind) {
            None => WasmPhysicalSourceKind::None,
            Some(PhysicalGrooveSourceKind::Contiguous) => WasmPhysicalSourceKind::Contiguous,
            Some(PhysicalGrooveSourceKind::Paged) => WasmPhysicalSourceKind::Paged,
            Some(PhysicalGrooveSourceKind::RealtimePaged) => WasmPhysicalSourceKind::RealtimePaged,
        }
    }

    /// Returns the cached paged generation, or zero for a contiguous source.
    #[wasm_bindgen(js_name = sourceCachedGeneration)]
    pub fn source_cached_generation(&self) -> u64 {
        self.inner
            .loaded_source_identity()
            .and_then(|value| value.cached_generation)
            .map_or(0, GrooveGenerationId::get)
    }

    /// Returns the requested paged generation, or zero for a contiguous source.
    #[wasm_bindgen(js_name = sourceRequestedGeneration)]
    pub fn source_requested_generation(&self) -> u64 {
        self.inner
            .loaded_source_identity()
            .and_then(|value| value.requested_generation)
            .map_or(0, GrooveGenerationId::get)
    }

    /// Allocates and loads one fixed-capacity page cache.
    ///
    /// Call this while rendering is stopped. Recreate cached WASM views after
    /// this call. Later fixed-cache operations do not allocate.
    #[wasm_bindgen(js_name = loadRealtimePagedCache)]
    pub fn load_realtime_paged_cache(
        &mut self,
        metadata: JsValue,
        page_slots: u32,
        maximum_stored_frames_per_page: u32,
        maximum_chunk_frames: u32,
        maximum_work_units_per_call: u32,
        maximum_resident_bytes: u64,
    ) -> WasmPhysicalRealtimePagedStatus {
        if !self.advance_memory_view_generation() {
            return WasmPhysicalRealtimePagedStatus::MemoryViewGenerationExhausted;
        }
        let metadata: PhysicalGrooveMetadata = match serde_wasm_bindgen::from_value(metadata) {
            Ok(metadata) => metadata,
            Err(_) => return WasmPhysicalRealtimePagedStatus::InvalidMetadata,
        };
        let config = if page_slots == 0
            && maximum_stored_frames_per_page == 0
            && maximum_chunk_frames == 0
            && maximum_work_units_per_call == 0
            && maximum_resident_bytes == 0
        {
            RealtimePagedGrooveCacheConfig::default()
        } else {
            RealtimePagedGrooveCacheConfig {
                page_slots,
                maximum_stored_frames_per_page,
                maximum_chunk_frames,
                maximum_work_units_per_call,
                maximum_resident_bytes,
            }
        };
        let cache = match RealtimePagedGrooveCache::new(metadata, config) {
            Ok(cache) => cache,
            Err(error) => return realtime_paged_error_status(&error),
        };
        match self.inner.load_realtime_paged_groove(cache) {
            Ok(_) => {
                self.clear_realtime_paged_handles();
                self.sync_source_identity();
                self.clear_paged_prefetch_plan();
                WasmPhysicalRealtimePagedStatus::Ok
            }
            Err(_) => WasmPhysicalRealtimePagedStatus::LoadFailed,
        }
    }

    /// Starts one page whose pyramid is constructed inside the fixed cache.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = beginRealtimePagedPage)]
    pub fn begin_realtime_paged_page(
        &mut self,
        page_identity_version: u32,
        digest_word_0: u32,
        digest_word_1: u32,
        digest_word_2: u32,
        digest_word_3: u32,
        digest_word_4: u32,
        digest_word_5: u32,
        digest_word_6: u32,
        digest_word_7: u32,
        core_start_frame: u64,
        core_end_frame_exclusive: u64,
        stored_start_frame: u64,
        stored_end_frame_exclusive: u64,
    ) -> WasmPhysicalRealtimePagedStatus {
        self.begin_realtime_paged_page_internal(
            false,
            page_identity_version,
            [
                digest_word_0,
                digest_word_1,
                digest_word_2,
                digest_word_3,
                digest_word_4,
                digest_word_5,
                digest_word_6,
                digest_word_7,
            ],
            core_start_frame,
            core_end_frame_exclusive,
            stored_start_frame,
            stored_end_frame_exclusive,
        )
    }

    /// Starts one page that receives all canonical spatial levels from a worker.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = beginRealtimePagedPagePrecomputed)]
    pub fn begin_realtime_paged_page_precomputed(
        &mut self,
        page_identity_version: u32,
        digest_word_0: u32,
        digest_word_1: u32,
        digest_word_2: u32,
        digest_word_3: u32,
        digest_word_4: u32,
        digest_word_5: u32,
        digest_word_6: u32,
        digest_word_7: u32,
        core_start_frame: u64,
        core_end_frame_exclusive: u64,
        stored_start_frame: u64,
        stored_end_frame_exclusive: u64,
    ) -> WasmPhysicalRealtimePagedStatus {
        self.begin_realtime_paged_page_internal(
            true,
            page_identity_version,
            [
                digest_word_0,
                digest_word_1,
                digest_word_2,
                digest_word_3,
                digest_word_4,
                digest_word_5,
                digest_word_6,
                digest_word_7,
            ],
            core_start_frame,
            core_end_frame_exclusive,
            stored_start_frame,
            stored_end_frame_exclusive,
        )
    }

    /// Reads the canonical layout for one precomputed spatial level.
    #[wasm_bindgen(js_name = readRealtimePagedSpatialLevelLayout)]
    pub fn read_realtime_paged_spatial_level_layout(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
        level_index: u8,
    ) -> WasmPhysicalRealtimePagedStatus {
        self.realtime_paged_last_level_layout = None;
        let ticket = match self.realtime_paged_ticket(slot_index, ticket_sequence) {
            Ok(ticket) => ticket,
            Err(status) => return status,
        };
        let result = self
            .inner
            .realtime_paged_cache()
            .ok_or(WasmPhysicalRealtimePagedStatus::NoLoadedCache)
            .and_then(|cache| {
                cache
                    .spatial_level_layout(ticket, level_index)
                    .map_err(|error| realtime_paged_error_status(&error))
            });
        match result {
            Ok(layout) => {
                self.realtime_paged_last_level_layout = Some(layout);
                WasmPhysicalRealtimePagedStatus::Ok
            }
            Err(status) => status,
        }
    }

    #[wasm_bindgen(js_name = reserveRealtimePagedLateralChunk)]
    pub fn reserve_realtime_paged_lateral_chunk(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
        first_frame_offset: u32,
        frame_count: u32,
    ) -> WasmPhysicalRealtimePagedStatus {
        self.reserve_realtime_paged_chunk(
            slot_index,
            ticket_sequence,
            WasmRealtimePagedChunkTarget::BaseLateral,
            first_frame_offset,
            frame_count,
        )
    }

    #[wasm_bindgen(js_name = reserveRealtimePagedVerticalChunk)]
    pub fn reserve_realtime_paged_vertical_chunk(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
        first_frame_offset: u32,
        frame_count: u32,
    ) -> WasmPhysicalRealtimePagedStatus {
        self.reserve_realtime_paged_chunk(
            slot_index,
            ticket_sequence,
            WasmRealtimePagedChunkTarget::BaseVertical,
            first_frame_offset,
            frame_count,
        )
    }

    #[wasm_bindgen(js_name = reserveRealtimePagedSpatialLateralChunk)]
    pub fn reserve_realtime_paged_spatial_lateral_chunk(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
        level_index: u8,
        first_frame_offset: u32,
        frame_count: u32,
    ) -> WasmPhysicalRealtimePagedStatus {
        self.reserve_realtime_paged_chunk(
            slot_index,
            ticket_sequence,
            WasmRealtimePagedChunkTarget::SpatialLevelLateral(level_index),
            first_frame_offset,
            frame_count,
        )
    }

    #[wasm_bindgen(js_name = reserveRealtimePagedSpatialVerticalChunk)]
    pub fn reserve_realtime_paged_spatial_vertical_chunk(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
        level_index: u8,
        first_frame_offset: u32,
        frame_count: u32,
    ) -> WasmPhysicalRealtimePagedStatus {
        self.reserve_realtime_paged_chunk(
            slot_index,
            ticket_sequence,
            WasmRealtimePagedChunkTarget::SpatialLevelVertical(level_index),
            first_frame_offset,
            frame_count,
        )
    }

    /// Returns one active reservation sequence, or zero when none is active.
    #[wasm_bindgen(js_name = realtimePagedReservationSequence)]
    pub fn realtime_paged_reservation_sequence(
        &self,
        slot_index: u32,
        ticket_sequence: u64,
    ) -> u64 {
        self.realtime_paged_reservation(slot_index, ticket_sequence, None)
            .map_or(0, RealtimePagedGrooveChunkReservation::sequence)
    }

    /// Returns one active reservation length, or zero when none is active.
    #[wasm_bindgen(js_name = realtimePagedReservedChunkLen)]
    pub fn realtime_paged_reserved_chunk_len(
        &self,
        slot_index: u32,
        ticket_sequence: u64,
        reservation_sequence: u64,
    ) -> u32 {
        self.realtime_paged_reservation(slot_index, ticket_sequence, Some(reservation_sequence))
            .map_or(0, RealtimePagedGrooveChunkReservation::frame_count)
    }

    /// Returns one active ingress-staging address in WASM memory.
    ///
    /// Commit copies this data into private cache storage.
    /// Use this pointer only until the matching commit or cancel operation.
    /// Do not retain or write through the pointer after that operation.
    #[wasm_bindgen(js_name = realtimePagedReservedChunkPtr)]
    pub fn realtime_paged_reserved_chunk_ptr(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
        reservation_sequence: u64,
    ) -> *mut f32 {
        let Some(reservation) = self.realtime_paged_reservation(
            slot_index,
            ticket_sequence,
            Some(reservation_sequence),
        ) else {
            return std::ptr::null_mut();
        };
        if reservation.frame_count() as usize > self.realtime_paged_chunk_staging.len() {
            return std::ptr::null_mut();
        }
        self.realtime_paged_chunk_staging.as_mut_ptr()
    }

    /// Validates and commits one completed in-place write.
    #[wasm_bindgen(js_name = commitRealtimePagedReservedChunk)]
    pub fn commit_realtime_paged_reserved_chunk(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
        reservation_sequence: u64,
    ) -> WasmPhysicalRealtimePagedStatus {
        let Some(reservation) = self.realtime_paged_reservation(
            slot_index,
            ticket_sequence,
            Some(reservation_sequence),
        ) else {
            return WasmPhysicalRealtimePagedStatus::StaleChunkReservation;
        };
        let frame_count = reservation.frame_count() as usize;
        let staged = &self.realtime_paged_chunk_staging[..frame_count];
        let result = self
            .inner
            .realtime_paged_cache_mut()
            .ok_or(WasmPhysicalRealtimePagedStatus::NoLoadedCache)
            .and_then(|cache| {
                cache
                    .reserved_chunk_mut(reservation)
                    .map_err(|error| realtime_paged_error_status(&error))?
                    .copy_from_slice(staged);
                cache
                    .commit_reserved_chunk(reservation)
                    .map_err(|error| realtime_paged_error_status(&error))
            });
        match result {
            Ok(progress) => {
                self.realtime_paged_reservations[slot_index as usize] = None;
                self.realtime_paged_last_progress = Some(progress);
                WasmPhysicalRealtimePagedStatus::Ok
            }
            Err(status) => status,
        }
    }

    /// Cancels one incomplete in-place write.
    #[wasm_bindgen(js_name = cancelRealtimePagedReservedChunk)]
    pub fn cancel_realtime_paged_reserved_chunk(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
        reservation_sequence: u64,
    ) -> WasmPhysicalRealtimePagedStatus {
        let Some(reservation) = self.realtime_paged_reservation(
            slot_index,
            ticket_sequence,
            Some(reservation_sequence),
        ) else {
            return WasmPhysicalRealtimePagedStatus::StaleChunkReservation;
        };
        let result = self
            .inner
            .realtime_paged_cache_mut()
            .ok_or(WasmPhysicalRealtimePagedStatus::NoLoadedCache)
            .and_then(|cache| {
                cache
                    .cancel_reserved_chunk(reservation)
                    .map_err(|error| realtime_paged_error_status(&error))
            });
        match result {
            Ok(progress) => {
                self.realtime_paged_reservations[slot_index as usize] = None;
                self.realtime_paged_last_progress = Some(progress);
                WasmPhysicalRealtimePagedStatus::Ok
            }
            Err(status) => status,
        }
    }

    #[wasm_bindgen(js_name = finishRealtimePagedPageIngestion)]
    pub fn finish_realtime_paged_page_ingestion(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
    ) -> WasmPhysicalRealtimePagedStatus {
        let ticket = match self.realtime_paged_ticket(slot_index, ticket_sequence) {
            Ok(ticket) => ticket,
            Err(status) => return status,
        };
        self.apply_realtime_paged_progress(|cache| cache.finish_page_ingestion(ticket))
    }

    #[wasm_bindgen(js_name = advanceRealtimePagedPage)]
    pub fn advance_realtime_paged_page(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
        maximum_work_units: u32,
    ) -> WasmPhysicalRealtimePagedStatus {
        let ticket = match self.realtime_paged_ticket(slot_index, ticket_sequence) {
            Ok(ticket) => ticket,
            Err(status) => return status,
        };
        self.apply_realtime_paged_progress(|cache| cache.advance_page(ticket, maximum_work_units))
    }

    #[wasm_bindgen(js_name = publishRealtimePagedPage)]
    pub fn publish_realtime_paged_page(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
    ) -> WasmPhysicalRealtimePagedStatus {
        let ticket = match self.realtime_paged_ticket(slot_index, ticket_sequence) {
            Ok(ticket) => ticket,
            Err(status) => return status,
        };
        let result = self
            .inner
            .realtime_paged_cache_mut()
            .ok_or(WasmPhysicalRealtimePagedStatus::NoLoadedCache)
            .and_then(|cache| {
                cache
                    .publish_page(ticket)
                    .map_err(|error| realtime_paged_error_status(&error))
            });
        match result {
            Ok(_) => self.read_realtime_paged_progress(slot_index, ticket_sequence),
            Err(status) => status,
        }
    }

    #[wasm_bindgen(js_name = discardRealtimePagedPage)]
    pub fn discard_realtime_paged_page(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
    ) -> WasmPhysicalRealtimePagedStatus {
        let ticket = match self.realtime_paged_ticket(slot_index, ticket_sequence) {
            Ok(ticket) => ticket,
            Err(status) => return status,
        };
        let result = self
            .inner
            .realtime_paged_cache_mut()
            .ok_or(WasmPhysicalRealtimePagedStatus::NoLoadedCache)
            .and_then(|cache| {
                cache
                    .discard_page(ticket)
                    .map_err(|error| realtime_paged_error_status(&error))
            });
        match result {
            Ok(_) => {
                self.realtime_paged_tickets[slot_index as usize] = None;
                self.realtime_paged_reservations[slot_index as usize] = None;
                self.realtime_paged_last_progress = None;
                WasmPhysicalRealtimePagedStatus::Ok
            }
            Err(status) => status,
        }
    }

    #[wasm_bindgen(js_name = evictRealtimePagedPageContaining)]
    pub fn evict_realtime_paged_page_containing(
        &mut self,
        frame: u64,
    ) -> WasmPhysicalRealtimePagedStatus {
        self.realtime_paged_last_eviction_found = false;
        let result = self
            .inner
            .realtime_paged_cache_mut()
            .ok_or(WasmPhysicalRealtimePagedStatus::NoLoadedCache)
            .and_then(|cache| {
                cache
                    .evict_page_containing(frame)
                    .map_err(|error| realtime_paged_error_status(&error))
            });
        match result {
            Ok(descriptor) => {
                self.realtime_paged_last_eviction_found = descriptor.is_some();
                self.clear_empty_realtime_paged_handles();
                WasmPhysicalRealtimePagedStatus::Ok
            }
            Err(status) => status,
        }
    }

    #[wasm_bindgen(js_name = readRealtimePagedProgress)]
    pub fn read_realtime_paged_progress(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
    ) -> WasmPhysicalRealtimePagedStatus {
        if let Err(status) = self.realtime_paged_ticket(slot_index, ticket_sequence) {
            return status;
        }
        let result = self
            .inner
            .realtime_paged_cache()
            .ok_or(WasmPhysicalRealtimePagedStatus::NoLoadedCache)
            .and_then(|cache| {
                cache
                    .slot_progress(slot_index)
                    .map_err(|error| realtime_paged_error_status(&error))
            });
        match result {
            Ok(progress) if progress.sequence == ticket_sequence => {
                self.realtime_paged_last_progress = Some(progress);
                WasmPhysicalRealtimePagedStatus::Ok
            }
            Ok(_) => WasmPhysicalRealtimePagedStatus::StaleTicket,
            Err(status) => status,
        }
    }

    #[wasm_bindgen(js_name = realtimePagedCacheLoaded)]
    pub fn realtime_paged_cache_loaded(&self) -> bool {
        self.inner.realtime_paged_cache().is_some()
    }

    #[wasm_bindgen(js_name = realtimePagedCacheGeneration)]
    pub fn realtime_paged_cache_generation(&self) -> u64 {
        self.inner
            .realtime_paged_cache()
            .map_or(0, |cache| cache.generation().get())
    }

    #[wasm_bindgen(js_name = realtimePagedCachePageSlots)]
    pub fn realtime_paged_cache_page_slots(&self) -> u32 {
        self.inner
            .realtime_paged_cache()
            .map_or(0, |cache| cache.status().page_slots)
    }

    #[wasm_bindgen(js_name = realtimePagedCachePublishedPages)]
    pub fn realtime_paged_cache_published_pages(&self) -> u32 {
        self.inner
            .realtime_paged_cache()
            .map_or(0, |cache| cache.status().published_pages)
    }

    #[wasm_bindgen(js_name = realtimePagedCacheStagingPages)]
    pub fn realtime_paged_cache_staging_pages(&self) -> u32 {
        self.inner
            .realtime_paged_cache()
            .map_or(0, |cache| cache.status().staging_pages)
    }

    #[wasm_bindgen(js_name = realtimePagedCacheRejectedPages)]
    pub fn realtime_paged_cache_rejected_pages(&self) -> u32 {
        self.inner
            .realtime_paged_cache()
            .map_or(0, |cache| cache.status().rejected_pages)
    }

    #[wasm_bindgen(js_name = realtimePagedCacheAllocatedResidentBytes)]
    pub fn realtime_paged_cache_allocated_resident_bytes(&self) -> u64 {
        self.inner
            .realtime_paged_cache()
            .map_or(0, |cache| cache.status().allocated_resident_bytes)
    }

    #[wasm_bindgen(js_name = realtimePagedLastTicketPresent)]
    pub fn realtime_paged_last_ticket_present(&self) -> bool {
        self.realtime_paged_last_ticket.is_some()
    }

    #[wasm_bindgen(js_name = realtimePagedLastTicketSlotIndex)]
    pub fn realtime_paged_last_ticket_slot_index(&self) -> u32 {
        self.realtime_paged_last_ticket
            .map_or(0, RealtimePagedGroovePageTicket::slot_index)
    }

    #[wasm_bindgen(js_name = realtimePagedLastTicketSequence)]
    pub fn realtime_paged_last_ticket_sequence(&self) -> u64 {
        self.realtime_paged_last_ticket
            .map_or(0, RealtimePagedGroovePageTicket::sequence)
    }

    #[wasm_bindgen(js_name = realtimePagedLastLevelLayoutPresent)]
    pub fn realtime_paged_last_level_layout_present(&self) -> bool {
        self.realtime_paged_last_level_layout.is_some()
    }

    #[wasm_bindgen(js_name = realtimePagedLastLevelIndex)]
    pub fn realtime_paged_last_level_index(&self) -> u8 {
        self.realtime_paged_last_level_layout
            .map_or(0, |layout| layout.level_index)
    }

    #[wasm_bindgen(js_name = realtimePagedLastLevelFirstSourceFrame)]
    pub fn realtime_paged_last_level_first_source_frame(&self) -> u64 {
        self.realtime_paged_last_level_layout
            .map_or(0, |layout| layout.first_source_frame)
    }

    #[wasm_bindgen(js_name = realtimePagedLastLevelSourceFrameStep)]
    pub fn realtime_paged_last_level_source_frame_step(&self) -> u32 {
        self.realtime_paged_last_level_layout
            .map_or(0, |layout| layout.source_frame_step)
    }

    #[wasm_bindgen(js_name = realtimePagedLastLevelFrameCount)]
    pub fn realtime_paged_last_level_frame_count(&self) -> u32 {
        self.realtime_paged_last_level_layout
            .map_or(0, |layout| layout.frame_count)
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressPresent)]
    pub fn realtime_paged_last_progress_present(&self) -> bool {
        self.realtime_paged_last_progress.is_some()
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressSlotIndex)]
    pub fn realtime_paged_last_progress_slot_index(&self) -> u32 {
        self.realtime_paged_last_progress
            .map_or(0, |progress| progress.slot_index)
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressSequence)]
    pub fn realtime_paged_last_progress_sequence(&self) -> u64 {
        self.realtime_paged_last_progress
            .map_or(0, |progress| progress.sequence)
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressPhase)]
    pub fn realtime_paged_last_progress_phase(&self) -> WasmPhysicalRealtimePagedPhase {
        self.realtime_paged_last_progress
            .map_or(WasmPhysicalRealtimePagedPhase::None, |progress| {
                realtime_paged_phase(progress.phase)
            })
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressReceivedLateralFrames)]
    pub fn realtime_paged_last_progress_received_lateral_frames(&self) -> u32 {
        self.realtime_paged_last_progress
            .map_or(0, |progress| progress.received_lateral_frames)
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressReceivedVerticalFrames)]
    pub fn realtime_paged_last_progress_received_vertical_frames(&self) -> u32 {
        self.realtime_paged_last_progress
            .map_or(0, |progress| progress.received_vertical_frames)
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressStoredFrames)]
    pub fn realtime_paged_last_progress_stored_frames(&self) -> u32 {
        self.realtime_paged_last_progress
            .map_or(0, |progress| progress.stored_frames)
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressPyramidInput)]
    pub fn realtime_paged_last_progress_pyramid_input(
        &self,
    ) -> WasmPhysicalRealtimePagedPyramidInput {
        self.realtime_paged_last_progress
            .map_or(WasmPhysicalRealtimePagedPyramidInput::None, |progress| {
                realtime_paged_pyramid_input(progress.pyramid_input)
            })
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressReceivedPyramidChannelSamples)]
    pub fn realtime_paged_last_progress_received_pyramid_channel_samples(&self) -> u32 {
        self.realtime_paged_last_progress
            .map_or(0, |progress| progress.received_pyramid_channel_samples)
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressChunkReserved)]
    pub fn realtime_paged_last_progress_chunk_reserved(&self) -> bool {
        self.realtime_paged_last_progress
            .is_some_and(|progress| progress.chunk_reserved)
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressCompletedPyramidLevels)]
    pub fn realtime_paged_last_progress_completed_pyramid_levels(&self) -> u8 {
        self.realtime_paged_last_progress
            .map_or(0, |progress| progress.completed_pyramid_levels)
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressWorkUnitsConsumed)]
    pub fn realtime_paged_last_progress_work_units_consumed(&self) -> u32 {
        self.realtime_paged_last_progress
            .map_or(0, |progress| progress.work_units_consumed)
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressFailure)]
    pub fn realtime_paged_last_progress_failure(&self) -> WasmPhysicalRealtimePagedFailure {
        self.realtime_paged_last_progress
            .and_then(|progress| progress.failure)
            .map_or(WasmPhysicalRealtimePagedFailure::None, |failure| {
                realtime_paged_failure(failure)
            })
    }

    #[wasm_bindgen(js_name = realtimePagedLastProgressFailureFrame)]
    pub fn realtime_paged_last_progress_failure_frame(&self) -> u64 {
        match self
            .realtime_paged_last_progress
            .and_then(|progress| progress.failure)
        {
            Some(RealtimePagedGroovePageFailure::SeamSampleMismatch { frame }) => frame,
            Some(RealtimePagedGroovePageFailure::SpatialSeamSampleMismatch { frame, .. }) => frame,
            Some(RealtimePagedGroovePageFailure::NoncanonicalSpatialPyramid { frame, .. }) => frame,
            _ => 0,
        }
    }

    #[wasm_bindgen(js_name = realtimePagedLastEvictionFound)]
    pub fn realtime_paged_last_eviction_found(&self) -> bool {
        self.realtime_paged_last_eviction_found
    }

    /// Reports the current paged-source API state.
    #[wasm_bindgen(js_name = pagedSourceApiStatus)]
    pub fn paged_source_api_status(&self) -> WasmPhysicalPagedSourceApiStatus {
        WasmPhysicalPagedSourceApiStatus::Available
    }

    /// Starts a paged cache from validated canonical metadata.
    ///
    /// This call allocates. Recreate cached WASM views after this call.
    #[wasm_bindgen(js_name = beginPagedCache)]
    pub fn begin_paged_cache(
        &mut self,
        metadata: JsValue,
        maximum_pages: u32,
        maximum_resident_bytes: u64,
    ) -> WasmPhysicalPagedSourceStatus {
        if !self.advance_memory_view_generation() {
            return WasmPhysicalPagedSourceStatus::MemoryViewGenerationExhausted;
        }
        let metadata: PhysicalGrooveMetadata = match serde_wasm_bindgen::from_value(metadata) {
            Ok(metadata) => metadata,
            Err(_) => return WasmPhysicalPagedSourceStatus::InvalidMetadata,
        };
        let limits = if maximum_pages == 0 && maximum_resident_bytes == 0 {
            PagedGrooveCacheLimits::default()
        } else {
            match PagedGrooveCacheLimits::new(maximum_pages, maximum_resident_bytes) {
                Ok(limits) => limits,
                Err(_) => return WasmPhysicalPagedSourceStatus::InvalidCacheLimits,
            }
        };
        match PagedGrooveCacheProducer::new(metadata, limits) {
            Ok(producer) => {
                self.paged_cache_producer = Some(producer);
                WasmPhysicalPagedSourceStatus::Ok
            }
            Err(error) => paged_error_status(&error),
        }
    }

    /// Starts an update from the last published immutable cache.
    ///
    /// This call allocates. Recreate cached WASM views after this call.
    #[wasm_bindgen(js_name = beginPagedCacheUpdate)]
    pub fn begin_paged_cache_update(&mut self) -> WasmPhysicalPagedSourceStatus {
        if !self.advance_memory_view_generation() {
            return WasmPhysicalPagedSourceStatus::MemoryViewGenerationExhausted;
        }
        let Some(cache) = &self.published_paged_cache else {
            return WasmPhysicalPagedSourceStatus::NoPublishedCache;
        };
        self.paged_cache_producer = Some(PagedGrooveCacheProducer::from_cache(cache));
        WasmPhysicalPagedSourceStatus::Ok
    }

    /// Returns the maximum typed staging-page length.
    #[wasm_bindgen(js_name = maximumPreparedPagedPageFrames)]
    pub fn maximum_prepared_paged_page_frames(&self) -> u32 {
        MAXIMUM_WASM_PREPARED_PAGE_FRAMES
    }

    /// Allocates bounded typed storage for one page payload.
    ///
    /// Stop rendering before this call. Recreate WASM views after this call.
    #[wasm_bindgen(js_name = preparePagedPage)]
    pub fn prepare_paged_page(&mut self, stored_frame_count: u32) -> WasmPhysicalPagedSourceStatus {
        if self.paged_cache_producer.is_none() {
            return WasmPhysicalPagedSourceStatus::NoCacheProducer;
        }
        if stored_frame_count == 0 || stored_frame_count > MAXIMUM_WASM_PREPARED_PAGE_FRAMES {
            return WasmPhysicalPagedSourceStatus::PreparedPageTooLarge;
        }
        if !self.advance_memory_view_generation() {
            return WasmPhysicalPagedSourceStatus::MemoryViewGenerationExhausted;
        }
        self.prepared_page_lateral_displacement_m =
            vec![0.0; stored_frame_count as usize].into_boxed_slice();
        self.prepared_page_vertical_displacement_m =
            vec![0.0; stored_frame_count as usize].into_boxed_slice();
        WasmPhysicalPagedSourceStatus::Ok
    }

    #[wasm_bindgen(js_name = preparedPagedPageFrameCount)]
    pub fn prepared_paged_page_frame_count(&self) -> u32 {
        self.prepared_page_lateral_displacement_m.len() as u32
    }

    /// Returns the typed lateral staging address.
    #[wasm_bindgen(js_name = preparedPagedPageLateralPtr)]
    pub fn prepared_paged_page_lateral_ptr(&mut self) -> *mut f32 {
        if self.prepared_page_lateral_displacement_m.is_empty() {
            std::ptr::null_mut()
        } else {
            self.prepared_page_lateral_displacement_m.as_mut_ptr()
        }
    }

    /// Returns the typed vertical staging address.
    #[wasm_bindgen(js_name = preparedPagedPageVerticalPtr)]
    pub fn prepared_paged_page_vertical_ptr(&mut self) -> *mut f32 {
        if self.prepared_page_vertical_displacement_m.is_empty() {
            std::ptr::null_mut()
        } else {
            self.prepared_page_vertical_displacement_m.as_mut_ptr()
        }
    }

    /// Builds and inserts the prepared page into the staged cache.
    ///
    /// Stop rendering before this call. The call builds the spatial pyramid.
    #[wasm_bindgen(js_name = commitPreparedPagedPage)]
    pub fn commit_prepared_paged_page(
        &mut self,
        core_start_frame: u64,
        core_end_frame_exclusive: u64,
        stored_start_frame: u64,
        stored_end_frame_exclusive: u64,
    ) -> WasmPhysicalPagedSourceStatus {
        if self.prepared_page_lateral_displacement_m.is_empty()
            || self.prepared_page_lateral_displacement_m.len()
                != self.prepared_page_vertical_displacement_m.len()
        {
            return WasmPhysicalPagedSourceStatus::NoPreparedPage;
        }
        let core_range = match GrooveFrameRange::new(core_start_frame, core_end_frame_exclusive) {
            Ok(range) => range,
            Err(_) => return WasmPhysicalPagedSourceStatus::InvalidPage,
        };
        let stored_range =
            match GrooveFrameRange::new(stored_start_frame, stored_end_frame_exclusive) {
                Ok(range) => range,
                Err(_) => return WasmPhysicalPagedSourceStatus::InvalidPage,
            };
        if stored_range.frame_count()
            != u64::try_from(self.prepared_page_lateral_displacement_m.len()).unwrap_or(u64::MAX)
        {
            return WasmPhysicalPagedSourceStatus::InvalidPage;
        }
        let Some(metadata) = self
            .paged_cache_producer
            .as_ref()
            .map(PagedGrooveCacheProducer::metadata)
        else {
            return WasmPhysicalPagedSourceStatus::NoCacheProducer;
        };
        if !self.advance_memory_view_generation() {
            return WasmPhysicalPagedSourceStatus::MemoryViewGenerationExhausted;
        }
        let lateral = std::mem::take(&mut self.prepared_page_lateral_displacement_m).into_vec();
        let vertical = std::mem::take(&mut self.prepared_page_vertical_displacement_m).into_vec();
        let page =
            match PhysicalGroovePage::new(metadata, core_range, stored_range, lateral, vertical) {
                Ok(page) => page,
                Err(error) => return paged_error_status(&error),
            };
        let Some(producer) = &mut self.paged_cache_producer else {
            return WasmPhysicalPagedSourceStatus::NoCacheProducer;
        };
        producer.insert_page(page).map_or_else(
            |error| paged_error_status(&error),
            |_| WasmPhysicalPagedSourceStatus::Ok,
        )
    }

    /// Inserts one validated canonical page into the staged cache.
    ///
    /// Stop rendering before this allocating diagnostic import call.
    #[wasm_bindgen(js_name = insertPagedPage)]
    pub fn insert_paged_page(&mut self, page: JsValue) -> WasmPhysicalPagedSourceStatus {
        if !self.advance_memory_view_generation() {
            return WasmPhysicalPagedSourceStatus::MemoryViewGenerationExhausted;
        }
        let page: PhysicalGroovePage = match serde_wasm_bindgen::from_value(page) {
            Ok(page) => page,
            Err(_) => return WasmPhysicalPagedSourceStatus::InvalidPage,
        };
        let Some(producer) = &mut self.paged_cache_producer else {
            return WasmPhysicalPagedSourceStatus::NoCacheProducer;
        };
        producer.insert_page(page).map_or_else(
            |error| paged_error_status(&error),
            |_| WasmPhysicalPagedSourceStatus::Ok,
        )
    }

    /// Removes the staged page that owns `frame`.
    #[wasm_bindgen(js_name = removePagedPageContaining)]
    pub fn remove_paged_page_containing(&mut self, frame: u64) -> WasmPhysicalPagedSourceStatus {
        let Some(producer) = &mut self.paged_cache_producer else {
            return WasmPhysicalPagedSourceStatus::NoCacheProducer;
        };
        producer.remove_page_containing(frame);
        WasmPhysicalPagedSourceStatus::Ok
    }

    /// Publishes the staged cache for later source loading.
    ///
    /// This call allocates. Recreate cached WASM views after this call.
    #[wasm_bindgen(js_name = publishPagedCache)]
    pub fn publish_paged_cache(&mut self) -> WasmPhysicalPagedSourceStatus {
        if !self.advance_memory_view_generation() {
            return WasmPhysicalPagedSourceStatus::MemoryViewGenerationExhausted;
        }
        let Some(producer) = self.paged_cache_producer.take() else {
            return WasmPhysicalPagedSourceStatus::NoCacheProducer;
        };
        self.published_paged_cache = Some(Arc::new(producer.publish()));
        WasmPhysicalPagedSourceStatus::Ok
    }

    /// Loads the last published cache at its own generation.
    #[wasm_bindgen(js_name = loadPublishedPagedCache)]
    pub fn load_published_paged_cache(&mut self) -> WasmPhysicalPagedSourceStatus {
        let Some(cache) = self.published_paged_cache.as_ref().map(Arc::clone) else {
            return WasmPhysicalPagedSourceStatus::NoPublishedCache;
        };
        match self.inner.load_paged_groove(cache) {
            Ok(_) => {
                self.sync_source_identity();
                self.clear_paged_prefetch_plan();
                WasmPhysicalPagedSourceStatus::Ok
            }
            Err(_) => WasmPhysicalPagedSourceStatus::LoadFailed,
        }
    }

    /// Loads the published cache with an explicit requested generation.
    #[wasm_bindgen(js_name = loadPublishedPagedCacheGeneration)]
    pub fn load_published_paged_cache_generation(
        &mut self,
        requested_generation: u64,
    ) -> WasmPhysicalPagedSourceStatus {
        let Ok(requested_generation) = GrooveGenerationId::new(requested_generation) else {
            return WasmPhysicalPagedSourceStatus::InvalidGeneration;
        };
        let Some(cache) = self.published_paged_cache.as_ref().map(Arc::clone) else {
            return WasmPhysicalPagedSourceStatus::NoPublishedCache;
        };
        match self
            .inner
            .load_paged_groove_generation(cache, requested_generation)
        {
            Ok(_) => {
                self.sync_source_identity();
                self.clear_paged_prefetch_plan();
                WasmPhysicalPagedSourceStatus::Ok
            }
            Err(_) => WasmPhysicalPagedSourceStatus::LoadFailed,
        }
    }

    /// Replaces the loaded page snapshot without resetting physical state.
    #[wasm_bindgen(js_name = refreshLoadedPagedCache)]
    pub fn refresh_loaded_paged_cache(&mut self) -> WasmPhysicalPagedSourceStatus {
        let Some(cache) = self.published_paged_cache.as_ref().map(Arc::clone) else {
            return WasmPhysicalPagedSourceStatus::NoPublishedCache;
        };
        match self.inner.replace_loaded_paged_cache_snapshot(cache) {
            Ok(_) => {
                self.sync_source_identity();
                self.clear_paged_prefetch_plan();
                WasmPhysicalPagedSourceStatus::Ok
            }
            Err(PhysicalRecordPlayerError::PagedCacheReplacementRequiresPagedSource) => {
                WasmPhysicalPagedSourceStatus::NoLoadedPagedSource
            }
            Err(PhysicalRecordPlayerError::PagedCacheReplacementMismatch) => {
                WasmPhysicalPagedSourceStatus::RefreshMismatch
            }
            Err(_) => WasmPhysicalPagedSourceStatus::RefreshMismatch,
        }
    }

    /// Builds one bounded prefetch plan outside rendering.
    ///
    /// This call can allocate. Recreate cached WASM views after this call.
    #[wasm_bindgen(js_name = buildPagedPrefetchPlan)]
    pub fn build_paged_prefetch_plan(
        &mut self,
        render_frame_horizon: u32,
        adjacent_turn_offsets: &[i32],
    ) -> WasmPhysicalPagedSourceStatus {
        if !self.advance_memory_view_generation() {
            return WasmPhysicalPagedSourceStatus::MemoryViewGenerationExhausted;
        }
        self.clear_paged_prefetch_plan();
        let adjacent_turn_offsets = adjacent_turn_offsets
            .iter()
            .copied()
            .map(i64::from)
            .collect::<Vec<_>>();
        let plan = match self
            .inner
            .paged_prefetch_plan(render_frame_horizon, &adjacent_turn_offsets)
        {
            Ok(Some(plan)) => plan,
            Ok(None) => return WasmPhysicalPagedSourceStatus::NoLoadedPagedSource,
            Err(_) => return WasmPhysicalPagedSourceStatus::PrefetchFailed,
        };
        if plan.core_ranges().len() > self.paged_prefetch_ranges.len() {
            return WasmPhysicalPagedSourceStatus::PrefetchFailed;
        }
        for (output, range) in self
            .paged_prefetch_ranges
            .iter_mut()
            .zip(plan.core_ranges())
        {
            *output = WasmPagedPrefetchRange {
                start_frame: range.start_frame(),
                end_frame_exclusive: range.end_frame_exclusive(),
            };
        }
        self.paged_prefetch_range_count = plan.core_ranges().len() as u32;
        self.paged_prefetch_generation = plan.generation().get();
        self.paged_prefetch_available = true;
        WasmPhysicalPagedSourceStatus::Ok
    }

    #[wasm_bindgen(js_name = pagedPrefetchAvailable)]
    pub fn paged_prefetch_available(&self) -> bool {
        self.paged_prefetch_available
    }

    #[wasm_bindgen(js_name = pagedPrefetchGeneration)]
    pub fn paged_prefetch_generation(&self) -> u64 {
        self.paged_prefetch_generation
    }

    #[wasm_bindgen(js_name = pagedPrefetchRangeCount)]
    pub fn paged_prefetch_range_count(&self) -> u32 {
        self.paged_prefetch_range_count
    }

    /// Returns one half-open prefetch range start, or zero for an invalid index.
    #[wasm_bindgen(js_name = pagedPrefetchRangeStart)]
    pub fn paged_prefetch_range_start(&self, index: u32) -> u64 {
        self.paged_prefetch_range(index)
            .map_or(0, |range| range.start_frame)
    }

    /// Returns one half-open prefetch range end, or zero for an invalid index.
    #[wasm_bindgen(js_name = pagedPrefetchRangeEnd)]
    pub fn paged_prefetch_range_end(&self, index: u32) -> u64 {
        self.paged_prefetch_range(index)
            .map_or(0, |range| range.end_frame_exclusive)
    }

    #[wasm_bindgen(js_name = stagedPagedPageCount)]
    pub fn staged_paged_page_count(&self) -> u32 {
        self.paged_cache_producer
            .as_ref()
            .map_or(0, |producer| producer.page_count() as u32)
    }

    #[wasm_bindgen(js_name = stagedPagedResidentBytes)]
    pub fn staged_paged_resident_bytes(&self) -> u64 {
        self.paged_cache_producer
            .as_ref()
            .map_or(0, PagedGrooveCacheProducer::resident_page_bytes)
    }

    #[wasm_bindgen(js_name = stagedPagedGeneration)]
    pub fn staged_paged_generation(&self) -> u64 {
        self.paged_cache_producer
            .as_ref()
            .map_or(0, |producer| producer.metadata().generation().get())
    }

    #[wasm_bindgen(js_name = stagedPagedTotalFrames)]
    pub fn staged_paged_total_frames(&self) -> u64 {
        self.paged_cache_producer
            .as_ref()
            .map_or(0, |producer| producer.metadata().total_frame_count())
    }

    #[wasm_bindgen(js_name = stagedPagedTracingHaloFrames)]
    pub fn staged_paged_tracing_halo_frames(&self) -> u32 {
        self.paged_cache_producer
            .as_ref()
            .map_or(0, |producer| producer.metadata().tracing_halo_frames())
    }

    #[wasm_bindgen(js_name = stagedPagedMaximumPages)]
    pub fn staged_paged_maximum_pages(&self) -> u32 {
        self.paged_cache_producer
            .as_ref()
            .map_or(0, |producer| producer.limits().maximum_pages())
    }

    #[wasm_bindgen(js_name = stagedPagedMaximumResidentBytes)]
    pub fn staged_paged_maximum_resident_bytes(&self) -> u64 {
        self.paged_cache_producer
            .as_ref()
            .map_or(0, |producer| producer.limits().maximum_resident_bytes())
    }

    #[wasm_bindgen(js_name = publishedPagedPageCount)]
    pub fn published_paged_page_count(&self) -> u32 {
        self.published_paged_cache
            .as_ref()
            .map_or(0, |cache| cache.page_count() as u32)
    }

    #[wasm_bindgen(js_name = publishedPagedResidentBytes)]
    pub fn published_paged_resident_bytes(&self) -> u64 {
        self.published_paged_cache
            .as_ref()
            .map_or(0, |cache| cache.resident_page_bytes())
    }

    #[wasm_bindgen(js_name = publishedPagedGeneration)]
    pub fn published_paged_generation(&self) -> u64 {
        self.published_paged_cache
            .as_ref()
            .map_or(0, |cache| cache.generation().get())
    }

    #[wasm_bindgen(js_name = publishedPagedTotalFrames)]
    pub fn published_paged_total_frames(&self) -> u64 {
        self.published_paged_cache
            .as_ref()
            .map_or(0, |cache| cache.metadata().total_frame_count())
    }

    #[wasm_bindgen(js_name = publishedPagedTracingHaloFrames)]
    pub fn published_paged_tracing_halo_frames(&self) -> u32 {
        self.published_paged_cache
            .as_ref()
            .map_or(0, |cache| cache.metadata().tracing_halo_frames())
    }

    #[wasm_bindgen(js_name = publishedPagedMaximumPages)]
    pub fn published_paged_maximum_pages(&self) -> u32 {
        self.published_paged_cache
            .as_ref()
            .map_or(0, |cache| cache.limits().maximum_pages())
    }

    #[wasm_bindgen(js_name = publishedPagedMaximumResidentBytes)]
    pub fn published_paged_maximum_resident_bytes(&self) -> u64 {
        self.published_paged_cache
            .as_ref()
            .map_or(0, |cache| cache.limits().maximum_resident_bytes())
    }

    #[wasm_bindgen(js_name = lastPageMissKind)]
    pub fn last_page_miss_kind(&self) -> WasmPhysicalPageMissKind {
        self.last_page_miss_kind
    }

    #[wasm_bindgen(js_name = lastPageMissFrame)]
    pub fn last_page_miss_frame(&self) -> u64 {
        self.last_page_miss_frame
    }

    #[wasm_bindgen(js_name = lastPageMissRequestedGeneration)]
    pub fn last_page_miss_requested_generation(&self) -> u64 {
        self.last_page_miss_requested_generation
    }

    #[wasm_bindgen(js_name = lastPageMissCachedGeneration)]
    pub fn last_page_miss_cached_generation(&self) -> u64 {
        self.last_page_miss_cached_generation
    }

    /// Serializes a deterministic renderer snapshot.
    ///
    /// This call allocates. Recreate cached WASM views after this call.
    pub fn snapshot(&mut self) -> Result<JsValue, JsValue> {
        if !self.advance_memory_view_generation() {
            return Err(JsValue::from_str("memory view generation is exhausted"));
        }
        serde_wasm_bindgen::to_value(&self.inner.snapshot()).map_err(Into::into)
    }

    /// Restores a deterministic renderer snapshot.
    ///
    /// This call allocates. Recreate cached WASM views after this call.
    pub fn restore(&mut self, snapshot: JsValue) -> Result<(), JsValue> {
        if !self.advance_memory_view_generation() {
            return Err(JsValue::from_str("memory view generation is exhausted"));
        }
        let snapshot: PhysicalHostRendererSnapshot = serde_wasm_bindgen::from_value(snapshot)?;
        self.inner.restore(&snapshot).map_err(js_error)?;
        self.sync_source_identity();
        Ok(())
    }

    #[wasm_bindgen(js_name = telemetryRenderedInternalFrames)]
    pub fn telemetry_rendered_internal_frames(&self) -> u32 {
        self.inner.telemetry().rendered_internal_frames as u32
    }

    #[wasm_bindgen(js_name = telemetryAbsoluteInternalFrame)]
    pub fn telemetry_absolute_internal_frame(&self) -> u64 {
        self.inner.telemetry().absolute_internal_frame
    }

    #[wasm_bindgen(js_name = telemetrySpiralFramePosition)]
    pub fn telemetry_spiral_frame_position(&self) -> f64 {
        self.inner.telemetry().spiral_frame_position
    }

    #[wasm_bindgen(js_name = telemetryGrooveFramePosition)]
    pub fn telemetry_groove_frame_position(&self) -> f64 {
        self.inner.telemetry().groove_frame_position
    }

    #[wasm_bindgen(js_name = telemetryGrooveRadiusM)]
    pub fn telemetry_groove_radius_m(&self) -> f64 {
        self.inner.telemetry().groove_radius_m
    }

    #[wasm_bindgen(js_name = telemetryGrooveLoaded)]
    pub fn telemetry_groove_loaded(&self) -> bool {
        self.inner.telemetry().groove_loaded
    }

    #[wasm_bindgen(js_name = telemetryAtProgrammeBoundary)]
    pub fn telemetry_at_programme_boundary(&self) -> bool {
        self.inner.telemetry().at_programme_boundary
    }

    #[wasm_bindgen(js_name = telemetryPhonoOutputLeftV)]
    pub fn telemetry_phono_output_left_v(&self) -> f64 {
        self.inner.telemetry().phono_output_v[0]
    }

    #[wasm_bindgen(js_name = telemetryPhonoOutputRightV)]
    pub fn telemetry_phono_output_right_v(&self) -> f64 {
        self.inner.telemetry().phono_output_v[1]
    }

    #[wasm_bindgen(js_name = telemetryPhonoInputOverloadLeft)]
    pub fn telemetry_phono_input_overload_left(&self) -> bool {
        self.inner.telemetry().phono_input_overload[0]
    }

    #[wasm_bindgen(js_name = telemetryPhonoInputOverloadRight)]
    pub fn telemetry_phono_input_overload_right(&self) -> bool {
        self.inner.telemetry().phono_input_overload[1]
    }

    #[wasm_bindgen(js_name = telemetryPhonoOutputOverloadLeft)]
    pub fn telemetry_phono_output_overload_left(&self) -> bool {
        self.inner.telemetry().phono_output_overload[0]
    }

    #[wasm_bindgen(js_name = telemetryPhonoOutputOverloadRight)]
    pub fn telemetry_phono_output_overload_right(&self) -> bool {
        self.inner.telemetry().phono_output_overload[1]
    }

    #[wasm_bindgen(js_name = telemetryPlatterRate)]
    pub fn telemetry_platter_rate(&self) -> f64 {
        self.inner.telemetry().mechanics.platter_rate
    }

    #[wasm_bindgen(js_name = telemetryRecordRate)]
    pub fn telemetry_record_rate(&self) -> f64 {
        self.inner.telemetry().mechanics.record_rate
    }

    #[wasm_bindgen(js_name = telemetryPlatterAngleTurns)]
    pub fn telemetry_platter_angle_turns(&self) -> f64 {
        self.inner.telemetry().mechanics.platter_angle_turns
    }

    #[wasm_bindgen(js_name = telemetryRecordAngleTurns)]
    pub fn telemetry_record_angle_turns(&self) -> f64 {
        self.inner.telemetry().mechanics.record_angle_turns
    }

    #[wasm_bindgen(js_name = telemetryMotorTorqueNm)]
    pub fn telemetry_motor_torque_nm(&self) -> f64 {
        self.inner.telemetry().mechanics.motor_torque_nm
    }

    #[wasm_bindgen(js_name = telemetrySlipmatTorqueNm)]
    pub fn telemetry_slipmat_torque_nm(&self) -> f64 {
        self.inner.telemetry().mechanics.slipmat_torque_nm
    }

    #[wasm_bindgen(js_name = telemetryHandTorqueNm)]
    pub fn telemetry_hand_torque_nm(&self) -> f64 {
        self.inner.telemetry().mechanics.hand_torque_nm
    }

    #[wasm_bindgen(js_name = telemetryStylusTorqueNm)]
    pub fn telemetry_stylus_torque_nm(&self) -> f64 {
        self.inner.telemetry().mechanics.stylus_torque_nm
    }

    #[wasm_bindgen(js_name = telemetrySlipmatMode)]
    pub fn telemetry_slipmat_mode(&self) -> WasmPhysicalContactMode {
        contact_mode(self.inner.telemetry().mechanics.slipmat_mode)
    }

    #[wasm_bindgen(js_name = telemetryHandMode)]
    pub fn telemetry_hand_mode(&self) -> WasmPhysicalContactMode {
        contact_mode(self.inner.telemetry().mechanics.hand_mode)
    }

    #[wasm_bindgen(js_name = telemetryPickupContactSurface)]
    pub fn telemetry_pickup_contact_surface(&self) -> WasmPhysicalPickupContactSurface {
        pickup_contact_surface(self.inner.telemetry().pickup.contact_surface)
    }

    #[wasm_bindgen(js_name = telemetryPickupWallContactLeft)]
    pub fn telemetry_pickup_wall_contact_left(&self) -> bool {
        self.inner.telemetry().pickup.wall_contact[0]
    }

    #[wasm_bindgen(js_name = telemetryPickupWallContactRight)]
    pub fn telemetry_pickup_wall_contact_right(&self) -> bool {
        self.inner.telemetry().pickup.wall_contact[1]
    }

    #[wasm_bindgen(js_name = telemetryPickupWallNormalForceLeftN)]
    pub fn telemetry_pickup_wall_normal_force_left_n(&self) -> f64 {
        self.inner.telemetry().pickup.wall_normal_force_n[0]
    }

    #[wasm_bindgen(js_name = telemetryPickupWallNormalForceRightN)]
    pub fn telemetry_pickup_wall_normal_force_right_n(&self) -> f64 {
        self.inner.telemetry().pickup.wall_normal_force_n[1]
    }

    #[wasm_bindgen(js_name = telemetryPickupRecordReactionForceN)]
    pub fn telemetry_pickup_record_reaction_force_n(&self) -> f64 {
        self.inner
            .telemetry()
            .pickup
            .record_reaction_force_tangent_n
    }

    #[wasm_bindgen(js_name = telemetryCartridgeGeneratorVoltageLeftV)]
    pub fn telemetry_cartridge_generator_voltage_left_v(&self) -> f64 {
        self.inner.telemetry().cartridge.generator_voltage_v[0]
    }

    #[wasm_bindgen(js_name = telemetryCartridgeGeneratorVoltageRightV)]
    pub fn telemetry_cartridge_generator_voltage_right_v(&self) -> f64 {
        self.inner.telemetry().cartridge.generator_voltage_v[1]
    }

    #[wasm_bindgen(js_name = telemetryCartridgeLoadOutputLeftV)]
    pub fn telemetry_cartridge_load_output_left_v(&self) -> f64 {
        self.inner.telemetry().cartridge.load_output_voltage_v[0]
    }

    #[wasm_bindgen(js_name = telemetryCartridgeLoadOutputRightV)]
    pub fn telemetry_cartridge_load_output_right_v(&self) -> f64 {
        self.inner.telemetry().cartridge.load_output_voltage_v[1]
    }

    #[wasm_bindgen(js_name = telemetryRadialContactRegion)]
    pub fn telemetry_radial_contact_region(&self) -> WasmPhysicalRadialContactRegion {
        radial_contact_region(self.inner.telemetry().radial_tracking.contact_region)
    }

    #[wasm_bindgen(js_name = telemetryRadialContactLost)]
    pub fn telemetry_radial_contact_lost(&self) -> bool {
        self.inner
            .telemetry()
            .radial_tracking
            .contact_lost_this_step
    }

    #[wasm_bindgen(js_name = telemetryRadialRecaptured)]
    pub fn telemetry_radial_recaptured(&self) -> bool {
        self.inner.telemetry().radial_tracking.recaptured_this_step
    }

    #[wasm_bindgen(js_name = telemetryRadialTurnsSkipped)]
    pub fn telemetry_radial_turns_skipped(&self) -> i64 {
        self.inner
            .telemetry()
            .radial_tracking
            .turns_skipped_this_step
    }

    #[wasm_bindgen(js_name = telemetryRadialTotalTurnsSkipped)]
    pub fn telemetry_radial_total_turns_skipped(&self) -> i64 {
        self.inner.telemetry().radial_tracking.total_turns_skipped
    }
}

impl WasmPhysicalHostRenderer {
    fn create(
        output_sample_rate_hz: u32,
        profile: Option<PhysicalProfile>,
        host_output_config: PhysicalHostOutputConfig,
    ) -> Result<Self, PhysicalHostRendererError> {
        let profile =
            profile.unwrap_or_else(PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed);
        let inner = PhysicalHostRenderer::new(profile, output_sample_rate_hz, host_output_config)?;
        let output = vec![0.0; inner.maximum_host_render_frames() * 2].into_boxed_slice();
        let mut renderer = Self {
            inner,
            output,
            output_frame_count: 0,
            memory_view_generation: 1,
            source_identity_present: false,
            source_identity_version: 0,
            source_identity_sha256: [0; 32],
            paged_cache_producer: None,
            published_paged_cache: None,
            last_page_miss_kind: WasmPhysicalPageMissKind::None,
            last_page_miss_frame: 0,
            last_page_miss_requested_generation: 0,
            last_page_miss_cached_generation: 0,
            paged_prefetch_ranges: vec![
                WasmPagedPrefetchRange::default();
                MAX_PAGED_GROOVE_PREFETCH_CANDIDATES
            ]
            .into_boxed_slice(),
            paged_prefetch_range_count: 0,
            paged_prefetch_generation: 0,
            paged_prefetch_available: false,
            prepared_page_lateral_displacement_m: Box::new([]),
            prepared_page_vertical_displacement_m: Box::new([]),
            realtime_paged_tickets: [None; MAXIMUM_WASM_REALTIME_PAGE_SLOTS],
            realtime_paged_reservations: [None; MAXIMUM_WASM_REALTIME_PAGE_SLOTS],
            realtime_paged_chunk_staging: vec![0.0; MAXIMUM_WASM_REALTIME_CHUNK_FRAMES]
                .into_boxed_slice(),
            realtime_paged_last_ticket: None,
            realtime_paged_last_progress: None,
            realtime_paged_last_level_layout: None,
            realtime_paged_last_eviction_found: false,
        };
        renderer.sync_source_identity();
        Ok(renderer)
    }

    #[allow(clippy::too_many_arguments)]
    fn begin_realtime_paged_page_internal(
        &mut self,
        precomputed_pyramid: bool,
        page_identity_version: u32,
        digest_words: [u32; 8],
        core_start_frame: u64,
        core_end_frame_exclusive: u64,
        stored_start_frame: u64,
        stored_end_frame_exclusive: u64,
    ) -> WasmPhysicalRealtimePagedStatus {
        self.realtime_paged_last_ticket = None;
        self.realtime_paged_last_progress = None;
        if page_identity_version != GrooveContentIdentity::from_sha256([0; 32]).identity_version() {
            return WasmPhysicalRealtimePagedStatus::InvalidIdentity;
        }
        let core_range = match GrooveFrameRange::new(core_start_frame, core_end_frame_exclusive) {
            Ok(range) => range,
            Err(_) => return WasmPhysicalRealtimePagedStatus::InvalidRange,
        };
        let stored_range =
            match GrooveFrameRange::new(stored_start_frame, stored_end_frame_exclusive) {
                Ok(range) => range,
                Err(_) => return WasmPhysicalRealtimePagedStatus::InvalidRange,
            };
        let Some(cache) = self.inner.realtime_paged_cache_mut() else {
            return WasmPhysicalRealtimePagedStatus::NoLoadedCache;
        };
        let descriptor = RealtimePagedGroovePageDescriptor {
            generation: cache.generation(),
            asset_content_identity: cache.content_identity(),
            page_content_identity: GrooveContentIdentity::from_sha256(identity_words_to_sha256(
                digest_words,
            )),
            trace_admission_certificate: None,
            core_range,
            stored_range,
        };
        let result = if precomputed_pyramid {
            cache.begin_raw_page_with_precomputed_pyramid(descriptor)
        } else {
            cache.begin_raw_page(descriptor)
        };
        match result {
            Ok(ticket) => {
                let slot_index = ticket.slot_index() as usize;
                self.realtime_paged_tickets[slot_index] = Some(ticket);
                self.realtime_paged_reservations[slot_index] = None;
                self.realtime_paged_last_ticket = Some(ticket);
                if let Some(cache) = self.inner.realtime_paged_cache() {
                    self.realtime_paged_last_progress =
                        cache.slot_progress(ticket.slot_index()).ok();
                }
                WasmPhysicalRealtimePagedStatus::Ok
            }
            Err(error) => realtime_paged_error_status(&error),
        }
    }

    fn realtime_paged_ticket(
        &self,
        slot_index: u32,
        ticket_sequence: u64,
    ) -> Result<RealtimePagedGroovePageTicket, WasmPhysicalRealtimePagedStatus> {
        let Some(ticket) = self
            .realtime_paged_tickets
            .get(slot_index as usize)
            .copied()
            .flatten()
        else {
            return Err(WasmPhysicalRealtimePagedStatus::StaleTicket);
        };
        if ticket.slot_index() != slot_index || ticket.sequence() != ticket_sequence {
            return Err(WasmPhysicalRealtimePagedStatus::StaleTicket);
        }
        Ok(ticket)
    }

    fn realtime_paged_reservation(
        &self,
        slot_index: u32,
        ticket_sequence: u64,
        reservation_sequence: Option<u64>,
    ) -> Option<RealtimePagedGrooveChunkReservation> {
        let ticket = self
            .realtime_paged_ticket(slot_index, ticket_sequence)
            .ok()?;
        let reservation = self
            .realtime_paged_reservations
            .get(slot_index as usize)
            .copied()
            .flatten()?;
        if reservation.ticket() != ticket
            || reservation_sequence.is_some_and(|sequence| reservation.sequence() != sequence)
        {
            return None;
        }
        Some(reservation)
    }

    fn reserve_realtime_paged_chunk(
        &mut self,
        slot_index: u32,
        ticket_sequence: u64,
        target: WasmRealtimePagedChunkTarget,
        first_frame_offset: u32,
        frame_count: u32,
    ) -> WasmPhysicalRealtimePagedStatus {
        let ticket = match self.realtime_paged_ticket(slot_index, ticket_sequence) {
            Ok(ticket) => ticket,
            Err(status) => return status,
        };
        if self.realtime_paged_reservations.iter().any(Option::is_some) {
            return WasmPhysicalRealtimePagedStatus::ChunkReservationActive;
        }
        let Some(cache) = self.inner.realtime_paged_cache_mut() else {
            return WasmPhysicalRealtimePagedStatus::NoLoadedCache;
        };
        let result = match target {
            WasmRealtimePagedChunkTarget::BaseLateral => {
                cache.reserve_lateral_chunk(ticket, first_frame_offset, frame_count)
            }
            WasmRealtimePagedChunkTarget::BaseVertical => {
                cache.reserve_vertical_chunk(ticket, first_frame_offset, frame_count)
            }
            WasmRealtimePagedChunkTarget::SpatialLevelLateral(level_index) => cache
                .reserve_spatial_level_lateral_chunk(
                    ticket,
                    level_index,
                    first_frame_offset,
                    frame_count,
                ),
            WasmRealtimePagedChunkTarget::SpatialLevelVertical(level_index) => cache
                .reserve_spatial_level_vertical_chunk(
                    ticket,
                    level_index,
                    first_frame_offset,
                    frame_count,
                ),
        };
        match result {
            Ok(reservation) => {
                self.realtime_paged_reservations[slot_index as usize] = Some(reservation);
                self.realtime_paged_last_progress = cache.slot_progress(slot_index).ok();
                WasmPhysicalRealtimePagedStatus::Ok
            }
            Err(error) => realtime_paged_error_status(&error),
        }
    }

    fn apply_realtime_paged_progress(
        &mut self,
        operation: impl FnOnce(
            &mut RealtimePagedGrooveCache,
        )
            -> Result<RealtimePagedGroovePageProgress, RealtimePagedGrooveError>,
    ) -> WasmPhysicalRealtimePagedStatus {
        let Some(cache) = self.inner.realtime_paged_cache_mut() else {
            return WasmPhysicalRealtimePagedStatus::NoLoadedCache;
        };
        match operation(cache) {
            Ok(progress) => {
                self.realtime_paged_last_progress = Some(progress);
                WasmPhysicalRealtimePagedStatus::Ok
            }
            Err(error) => realtime_paged_error_status(&error),
        }
    }

    fn clear_realtime_paged_handles(&mut self) {
        self.realtime_paged_tickets.fill(None);
        self.realtime_paged_reservations.fill(None);
        self.realtime_paged_last_ticket = None;
        self.realtime_paged_last_progress = None;
        self.realtime_paged_last_level_layout = None;
        self.realtime_paged_last_eviction_found = false;
    }

    fn clear_empty_realtime_paged_handles(&mut self) {
        let Some(cache) = self.inner.realtime_paged_cache() else {
            self.clear_realtime_paged_handles();
            return;
        };
        let mut empty = [false; MAXIMUM_WASM_REALTIME_PAGE_SLOTS];
        for slot_index in 0..cache.status().page_slots {
            empty[slot_index as usize] = cache
                .slot_progress(slot_index)
                .is_ok_and(|progress| progress.phase == RealtimePagedGrooveSlotPhase::Empty);
        }
        for (slot_index, is_empty) in empty.into_iter().enumerate() {
            if is_empty {
                self.realtime_paged_tickets[slot_index] = None;
                self.realtime_paged_reservations[slot_index] = None;
            }
        }
    }

    fn advance_memory_view_generation(&mut self) -> bool {
        let Some(next) = self.memory_view_generation.checked_add(1) else {
            return false;
        };
        self.memory_view_generation = next;
        true
    }

    fn cut_interleaved_pcm(
        &self,
        interleaved_pcm: &[f32],
        channel_count: u32,
        source_sample_rate_hz: f64,
    ) -> Result<GrooveAsset, WasmPhysicalSourceStatus> {
        let channel_count = channel_count as usize;
        if !(1..=2).contains(&channel_count)
            || interleaved_pcm.len() % channel_count != 0
            || interleaved_pcm.len() / channel_count < 4
            || !source_sample_rate_hz.is_finite()
            || source_sample_rate_hz <= 0.0
        {
            return Err(WasmPhysicalSourceStatus::InvalidArgument);
        }
        let profile = self.inner.profile();
        let layout = profile.config.groove;
        let cut = profile.config.record_cut;
        let groove = if channel_count == 1 {
            GrooveAsset::cut_from_pcm(&[interleaved_pcm], source_sample_rate_hz, layout, cut)
        } else {
            let frame_count = interleaved_pcm.len() / 2;
            let mut left = Vec::with_capacity(frame_count);
            let mut right = Vec::with_capacity(frame_count);
            for frame in interleaved_pcm.chunks_exact(2) {
                left.push(frame[0]);
                right.push(frame[1]);
            }
            GrooveAsset::cut_from_pcm(
                &[left.as_slice(), right.as_slice()],
                source_sample_rate_hz,
                layout,
                cut,
            )
        };
        groove.map_err(groove_error_status)
    }

    fn sync_source_identity(&mut self) {
        if self.inner.realtime_paged_cache().is_none() {
            self.clear_realtime_paged_handles();
        }
        match self.inner.loaded_groove_identity() {
            Some(identity) => {
                self.source_identity_present = true;
                self.source_identity_version = identity.identity_version();
                self.source_identity_sha256 = identity.sha256();
            }
            None => {
                self.source_identity_present = false;
                self.source_identity_version = 0;
                self.source_identity_sha256 = [0; 32];
            }
        }
    }

    fn clear_page_miss(&mut self) {
        self.last_page_miss_kind = WasmPhysicalPageMissKind::None;
        self.last_page_miss_frame = 0;
        self.last_page_miss_requested_generation = 0;
        self.last_page_miss_cached_generation = 0;
    }

    fn clear_paged_prefetch_plan(&mut self) {
        self.paged_prefetch_range_count = 0;
        self.paged_prefetch_generation = 0;
        self.paged_prefetch_available = false;
    }

    fn paged_prefetch_range(&self, index: u32) -> Option<WasmPagedPrefetchRange> {
        (index < self.paged_prefetch_range_count)
            .then(|| self.paged_prefetch_ranges[index as usize])
    }

    fn record_render_error(
        &mut self,
        error: &PhysicalHostRendererError,
    ) -> WasmPhysicalRenderStatus {
        let miss = match error {
            PhysicalHostRendererError::Player(
                PhysicalRecordPlayerError::PagedGrooveRenderMiss(miss),
            )
            | PhysicalHostRendererError::PlayerRender {
                source: PhysicalRecordPlayerError::PagedGrooveRenderMiss(miss),
                ..
            } => Some(*miss),
            _ => None,
        };
        match miss {
            Some(PagedGrooveRenderMiss::StaleGeneration {
                requested_generation,
                cached_generation,
            }) => {
                self.last_page_miss_kind = WasmPhysicalPageMissKind::StaleGeneration;
                self.last_page_miss_requested_generation = requested_generation.get();
                self.last_page_miss_cached_generation = cached_generation.get();
                WasmPhysicalRenderStatus::PageMiss
            }
            Some(PagedGrooveRenderMiss::PageUnavailable { generation, frame }) => {
                self.last_page_miss_kind = WasmPhysicalPageMissKind::PageUnavailable;
                self.last_page_miss_frame = frame;
                self.last_page_miss_requested_generation = generation.get();
                self.last_page_miss_cached_generation = self
                    .inner
                    .loaded_source_identity()
                    .and_then(|identity| identity.cached_generation)
                    .map_or(0, GrooveGenerationId::get);
                WasmPhysicalRenderStatus::PageMiss
            }
            None => render_error_status(error),
        }
    }
}

fn motor_mode_from_u32(value: u32) -> Option<MotorMode> {
    match value {
        value if value == WasmPhysicalMotorMode::Off as u32 => Some(MotorMode::Off),
        value if value == WasmPhysicalMotorMode::Servo as u32 => Some(MotorMode::Servo),
        value if value == WasmPhysicalMotorMode::Brake as u32 => Some(MotorMode::Brake),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn complete_control_from_scalars(
    motor_mode: u32,
    motor_target_angular_velocity_rad_s: f64,
    hand_contact: bool,
    hand_target_angle_present: bool,
    hand_target_angle_rad: f64,
    hand_target_angular_velocity_rad_s: f64,
    hand_normal_force_n: f64,
    hand_contact_radius_m: f64,
    stylus_torque_nm: f64,
    stylus_lowered: bool,
) -> Result<PlayerControl, WasmPhysicalControlStatus> {
    let Some(motor_mode) = motor_mode_from_u32(motor_mode) else {
        return Err(WasmPhysicalControlStatus::InvalidMotorMode);
    };
    Ok(PlayerControl::new(
        DeckMechanicalControl {
            motor_mode,
            motor_target_angular_velocity_rad_s,
            hand_contact,
            hand_target_angle_rad: hand_target_angle_present.then_some(hand_target_angle_rad),
            hand_target_angular_velocity_rad_s,
            hand_normal_force_n,
            hand_contact_radius_m,
            stylus_torque_nm,
        },
        stylus_lowered,
    ))
}

fn control_error_status(error: PhysicalRecordPlayerError) -> WasmPhysicalControlStatus {
    match error {
        PhysicalRecordPlayerError::TimelinePush(source) => match source {
            ControlTimelinePushError::InvalidControl(_) => {
                WasmPhysicalControlStatus::InvalidControl
            }
            ControlTimelinePushError::Late { .. } => WasmPhysicalControlStatus::Late,
            ControlTimelinePushError::Duplicate { .. } => WasmPhysicalControlStatus::Duplicate,
            ControlTimelinePushError::NonMonotonicFrame { .. } => {
                WasmPhysicalControlStatus::NonmonotonicFrame
            }
            ControlTimelinePushError::NonMonotonicSequence { .. } => {
                WasmPhysicalControlStatus::NonmonotonicSequence
            }
            ControlTimelinePushError::Full { .. } => WasmPhysicalControlStatus::QueueFull,
        },
        _ => WasmPhysicalControlStatus::CoreError,
    }
}

fn render_error_status(error: &PhysicalHostRendererError) -> WasmPhysicalRenderStatus {
    match error {
        PhysicalHostRendererError::HostBlockTooLarge { .. } => {
            WasmPhysicalRenderStatus::BlockTooLarge
        }
        PhysicalHostRendererError::Player(PhysicalRecordPlayerError::Groove(_))
        | PhysicalHostRendererError::Player(PhysicalRecordPlayerError::PagedGroove(_))
        | PhysicalHostRendererError::PlayerRender {
            source: PhysicalRecordPlayerError::Groove(_),
            ..
        }
        | PhysicalHostRendererError::PlayerRender {
            source: PhysicalRecordPlayerError::PagedGroove(_),
            ..
        } => WasmPhysicalRenderStatus::SourceUnavailable,
        _ => WasmPhysicalRenderStatus::CoreError,
    }
}

fn source_player_error_status(error: PhysicalRecordPlayerError) -> WasmPhysicalSourceStatus {
    match error {
        PhysicalRecordPlayerError::GrooveLayoutMismatch
        | PhysicalRecordPlayerError::GrooveCutMismatch => {
            WasmPhysicalSourceStatus::GrooveLayoutMismatch
        }
        PhysicalRecordPlayerError::GrooveProgrammeDoesNotFit => {
            WasmPhysicalSourceStatus::GrooveProgrammeDoesNotFit
        }
        PhysicalRecordPlayerError::GrooveOvercut => WasmPhysicalSourceStatus::GrooveOvercut,
        PhysicalRecordPlayerError::InvalidGroovePosition => {
            WasmPhysicalSourceStatus::InvalidPosition
        }
        _ => WasmPhysicalSourceStatus::GrooveLoadFailed,
    }
}

fn groove_error_status(error: GrooveError) -> WasmPhysicalSourceStatus {
    match error {
        GrooveError::InvalidSourceSampleRate
        | GrooveError::InvalidSourceChannelCount
        | GrooveError::ChannelLengthMismatch
        | GrooveError::InsufficientFrames
        | GrooveError::NonfiniteProgramme => WasmPhysicalSourceStatus::InvalidArgument,
        _ => WasmPhysicalSourceStatus::GrooveCutFailed,
    }
}

fn paged_error_status(error: &PagedGrooveError) -> WasmPhysicalPagedSourceStatus {
    match error {
        PagedGrooveError::InvalidCacheLimits { .. } => {
            WasmPhysicalPagedSourceStatus::InvalidCacheLimits
        }
        PagedGrooveError::CachePageLimitExceeded { .. }
        | PagedGrooveError::CacheResidentLimitExceeded { .. }
        | PagedGrooveError::CacheSizeOverflow => WasmPhysicalPagedSourceStatus::CacheLimitExceeded,
        PagedGrooveError::InvalidGeneration => WasmPhysicalPagedSourceStatus::InvalidGeneration,
        PagedGrooveError::GenerationMismatch { .. }
        | PagedGrooveError::AssetContentIdentityMismatch { .. }
        | PagedGrooveError::PageContentIdentityMismatch { .. }
        | PagedGrooveError::CoreOutsideAvailableRange { .. }
        | PagedGrooveError::StoredRangeOutsideRecord { .. }
        | PagedGrooveError::StoredRangeDoesNotContainCore
        | PagedGrooveError::ChannelLengthMismatch
        | PagedGrooveError::StoredLengthMismatch
        | PagedGrooveError::NonfiniteDisplacement
        | PagedGrooveError::MissingSpatialPyramid
        | PagedGrooveError::InsufficientLeftHalo { .. }
        | PagedGrooveError::ExcessLeftHalo { .. }
        | PagedGrooveError::InsufficientRightHalo { .. }
        | PagedGrooveError::ExcessRightHalo { .. }
        | PagedGrooveError::MissingSeamOverlap { .. }
        | PagedGrooveError::SeamSampleMismatch { .. }
        | PagedGrooveError::CacheCoreOverlap { .. } => WasmPhysicalPagedSourceStatus::InvalidPage,
        _ => WasmPhysicalPagedSourceStatus::InvalidMetadata,
    }
}

fn realtime_paged_error_status(
    error: &RealtimePagedGrooveError,
) -> WasmPhysicalRealtimePagedStatus {
    match error {
        RealtimePagedGrooveError::InvalidConfig { .. } => {
            WasmPhysicalRealtimePagedStatus::InvalidCacheConfig
        }
        RealtimePagedGrooveError::ResidentLimitExceeded { .. }
        | RealtimePagedGrooveError::ResidentSizeOverflow
        | RealtimePagedGrooveError::AllocationFailed => {
            WasmPhysicalRealtimePagedStatus::CacheAllocationFailed
        }
        RealtimePagedGrooveError::NoEmptySlot => WasmPhysicalRealtimePagedStatus::NoEmptySlot,
        RealtimePagedGrooveError::StaleTicket | RealtimePagedGrooveError::InvalidSlot { .. } => {
            WasmPhysicalRealtimePagedStatus::StaleTicket
        }
        RealtimePagedGrooveError::WrongPhase { .. } => WasmPhysicalRealtimePagedStatus::WrongPhase,
        RealtimePagedGrooveError::ChunkReservationActive => {
            WasmPhysicalRealtimePagedStatus::ChunkReservationActive
        }
        RealtimePagedGrooveError::StaleChunkReservation => {
            WasmPhysicalRealtimePagedStatus::StaleChunkReservation
        }
        RealtimePagedGrooveError::GenerationMismatch { .. }
        | RealtimePagedGrooveError::AssetContentIdentityMismatch => {
            WasmPhysicalRealtimePagedStatus::InvalidIdentity
        }
        RealtimePagedGrooveError::StoredRangeDoesNotContainCore
        | RealtimePagedGrooveError::StoredRangeOutsideRecord
        | RealtimePagedGrooveError::PageCapacityExceeded { .. }
        | RealtimePagedGrooveError::IncorrectStorageHalo { .. }
        | RealtimePagedGrooveError::CoreOverlap { .. }
        | RealtimePagedGrooveError::StagingRangeOverlap { .. }
        | RealtimePagedGrooveError::PageLayoutOverflow => {
            WasmPhysicalRealtimePagedStatus::InvalidRange
        }
        RealtimePagedGrooveError::EmptyChunk
        | RealtimePagedGrooveError::ChunkLimitExceeded { .. }
        | RealtimePagedGrooveError::NonsequentialChunk { .. }
        | RealtimePagedGrooveError::ChunkRangeOverflow
        | RealtimePagedGrooveError::ChunkOutsidePage => {
            WasmPhysicalRealtimePagedStatus::InvalidChunk
        }
        RealtimePagedGrooveError::NonfiniteDisplacement => {
            WasmPhysicalRealtimePagedStatus::NonfiniteDisplacement
        }
        RealtimePagedGrooveError::IncompleteChannels { .. }
        | RealtimePagedGrooveError::IncompleteSpatialLevel { .. } => {
            WasmPhysicalRealtimePagedStatus::IncompletePage
        }
        RealtimePagedGrooveError::InvalidSpatialLevel { .. } => {
            WasmPhysicalRealtimePagedStatus::InvalidSpatialLevel
        }
        RealtimePagedGrooveError::PrecomputedPyramidNotRequested => {
            WasmPhysicalRealtimePagedStatus::PrecomputedPyramidNotRequested
        }
        RealtimePagedGrooveError::InvalidWorkBudget { .. } => {
            WasmPhysicalRealtimePagedStatus::InvalidWorkBudget
        }
        RealtimePagedGrooveError::PageRejected(_) => WasmPhysicalRealtimePagedStatus::PageRejected,
        RealtimePagedGrooveError::CannotDiscardPublishedPage => {
            WasmPhysicalRealtimePagedStatus::CannotDiscardPublishedPage
        }
        RealtimePagedGrooveError::PageHasStagingDependency { .. } => {
            WasmPhysicalRealtimePagedStatus::PageHasStagingDependency
        }
        RealtimePagedGrooveError::TicketSequenceExhausted
        | RealtimePagedGrooveError::ChunkReservationSequenceExhausted
        | RealtimePagedGrooveError::InternalPageMap
        | RealtimePagedGrooveError::FramePositionOutsideRecord
        | RealtimePagedGrooveError::InvalidSourceFrameAdvance
        | RealtimePagedGrooveError::PagedGroove(_)
        | RealtimePagedGrooveError::Groove(_)
        | RealtimePagedGrooveError::Stylus(_)
        | RealtimePagedGrooveError::TraceAdmission(_) => WasmPhysicalRealtimePagedStatus::CoreError,
    }
}

fn realtime_paged_phase(value: RealtimePagedGrooveSlotPhase) -> WasmPhysicalRealtimePagedPhase {
    match value {
        RealtimePagedGrooveSlotPhase::Empty => WasmPhysicalRealtimePagedPhase::Empty,
        RealtimePagedGrooveSlotPhase::Receiving => WasmPhysicalRealtimePagedPhase::Receiving,
        RealtimePagedGrooveSlotPhase::BuildingPyramid => {
            WasmPhysicalRealtimePagedPhase::BuildingPyramid
        }
        RealtimePagedGrooveSlotPhase::Hashing => WasmPhysicalRealtimePagedPhase::Hashing,
        RealtimePagedGrooveSlotPhase::CertifyingTrace => {
            WasmPhysicalRealtimePagedPhase::CertifyingTrace
        }
        RealtimePagedGrooveSlotPhase::ValidatingSeams => {
            WasmPhysicalRealtimePagedPhase::ValidatingSeams
        }
        RealtimePagedGrooveSlotPhase::Ready => WasmPhysicalRealtimePagedPhase::Ready,
        RealtimePagedGrooveSlotPhase::Published => WasmPhysicalRealtimePagedPhase::Published,
        RealtimePagedGrooveSlotPhase::Rejected => WasmPhysicalRealtimePagedPhase::Rejected,
    }
}

fn realtime_paged_pyramid_input(
    value: RealtimePagedGroovePyramidInput,
) -> WasmPhysicalRealtimePagedPyramidInput {
    match value {
        RealtimePagedGroovePyramidInput::BuildFromBase => {
            WasmPhysicalRealtimePagedPyramidInput::BuildFromBase
        }
        RealtimePagedGroovePyramidInput::Precomputed => {
            WasmPhysicalRealtimePagedPyramidInput::Precomputed
        }
    }
}

fn realtime_paged_failure(
    value: RealtimePagedGroovePageFailure,
) -> WasmPhysicalRealtimePagedFailure {
    match value {
        RealtimePagedGroovePageFailure::PageContentIdentityMismatch => {
            WasmPhysicalRealtimePagedFailure::PageContentIdentityMismatch
        }
        RealtimePagedGroovePageFailure::SeamSampleMismatch { .. } => {
            WasmPhysicalRealtimePagedFailure::SeamSampleMismatch
        }
        RealtimePagedGroovePageFailure::MissingTraceAdmissionCertificate => {
            WasmPhysicalRealtimePagedFailure::MissingTraceAdmissionCertificate
        }
        RealtimePagedGroovePageFailure::TraceAdmissionCertificateMismatch => {
            WasmPhysicalRealtimePagedFailure::TraceAdmissionCertificateMismatch
        }
        RealtimePagedGroovePageFailure::TraceAdmissionNotAdmitted => {
            WasmPhysicalRealtimePagedFailure::TraceAdmissionNotAdmitted
        }
        RealtimePagedGroovePageFailure::SpatialSeamSampleMismatch { .. } => {
            WasmPhysicalRealtimePagedFailure::SpatialSeamSampleMismatch
        }
        RealtimePagedGroovePageFailure::NoncanonicalSpatialPyramid { .. } => {
            WasmPhysicalRealtimePagedFailure::NoncanonicalSpatialPyramid
        }
    }
}

fn identity_words_to_sha256(words: [u32; 8]) -> [u8; 32] {
    let mut sha256 = [0; 32];
    for (destination, word) in sha256.chunks_exact_mut(4).zip(words) {
        destination.copy_from_slice(&word.to_be_bytes());
    }
    sha256
}

fn sha256_word(sha256: [u8; 32], index: u32) -> u32 {
    let Some(start) = (index as usize).checked_mul(4) else {
        return 0;
    };
    let Some(bytes) = sha256.get(start..start + 4) else {
        return 0;
    };
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn contact_mode(value: ContactMode) -> WasmPhysicalContactMode {
    match value {
        ContactMode::Separated => WasmPhysicalContactMode::Separated,
        ContactMode::Sticking => WasmPhysicalContactMode::Sticking,
        ContactMode::SlidingPositive => WasmPhysicalContactMode::SlidingPositive,
        ContactMode::SlidingNegative => WasmPhysicalContactMode::SlidingNegative,
    }
}

fn pickup_contact_surface(value: PickupContactSurface) -> WasmPhysicalPickupContactSurface {
    match value {
        PickupContactSurface::None => WasmPhysicalPickupContactSurface::None,
        PickupContactSurface::GrooveWalls => WasmPhysicalPickupContactSurface::GrooveWalls,
        PickupContactSurface::RecordLand => WasmPhysicalPickupContactSurface::RecordLand,
    }
}

fn radial_contact_region(value: RadialContactRegion) -> WasmPhysicalRadialContactRegion {
    match value {
        RadialContactRegion::Groove => WasmPhysicalRadialContactRegion::Groove,
        RadialContactRegion::Land => WasmPhysicalRadialContactRegion::Land,
        RadialContactRegion::Lifted => WasmPhysicalRadialContactRegion::Lifted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_HOST_OUTPUT: PhysicalHostOutputConfig = PhysicalHostOutputConfig {
        volts_per_full_scale: 10.0,
    };

    fn renderer(rate: u32) -> WasmPhysicalHostRenderer {
        WasmPhysicalHostRenderer::create(rate, None, TEST_HOST_OUTPUT).unwrap()
    }

    fn scratch_mapper() -> WasmPhysicalScratchGestureMapper {
        let config =
            ScratchGestureConfig::for_deck(PhysicalDeckConfig::default(), 192_000, 0).unwrap();
        WasmPhysicalScratchGestureMapper::create(config).unwrap()
    }

    fn streaming_config(
        sample_rate_hz: f64,
        channel_count: u8,
        total_source_frames: usize,
        page_core_frames: u32,
    ) -> StreamingGrooveCutterConfig {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        StreamingGrooveCutterConfig {
            source_sample_rate_hz: sample_rate_hz,
            source_channel_count: channel_count,
            total_source_frame_count: total_source_frames as u64,
            layout: profile.config.groove,
            cut: profile.config.record_cut,
            page_core_frame_count: page_core_frames,
            tracing_halo_frames: 3,
        }
        .validate()
        .unwrap()
    }

    fn streaming_programme(frame_count: usize, sample_rate_hz: f64) -> (Vec<f32>, Vec<f32>) {
        let left = (0..frame_count)
            .map(|frame| {
                let time = frame as f64 / sample_rate_hz;
                (0.63 * (std::f64::consts::TAU * 997.0 * time).sin()
                    + 0.11 * (std::f64::consts::TAU * 3_101.0 * time).cos()) as f32
            })
            .collect();
        let right = (0..frame_count)
            .map(|frame| {
                let time = frame as f64 / sample_rate_hz;
                (0.47 * (std::f64::consts::TAU * 1_503.0 * time).cos()
                    - 0.09 * (std::f64::consts::TAU * 5_011.0 * time).sin()) as f32
            })
            .collect();
        (left, right)
    }

    fn take_streaming_page(cutter: &mut WasmStreamingGrooveCutter) -> StreamingGroovePageChunk {
        let page = cutter.take_emitted_page().expect("one page must be ready");
        assert_eq!(
            page.format_version(),
            STREAMING_GROOVE_PAGE_CHUNK_FORMAT_VERSION
        );
        assert!(page.stored_frame_count() > 0);
        assert!(!page.lateral_displacement_ptr().is_null());
        assert!(!page.vertical_displacement_ptr().is_null());
        page.inner
    }

    fn canonical_page_fixture() -> (PhysicalGrooveMetadata, PhysicalGroovePage) {
        let renderer = renderer(48_000);
        let pcm = (0..512)
            .map(|frame| (0.5 * (std::f64::consts::TAU * frame as f64 / 37.0).sin()) as f32)
            .collect::<Vec<_>>();
        let groove = renderer.cut_interleaved_pcm(&pcm, 1, 192_000.0).unwrap();
        let generation = GrooveGenerationId::new(701).unwrap();
        let seed_metadata =
            PhysicalGrooveMetadata::from_groove_asset(generation, &groove, 1).unwrap();
        let tracing_halo = seed_metadata
            .minimum_tracing_halo_frames(renderer.inner.profile().config.stylus)
            .unwrap()
            .max(1);
        let metadata =
            PhysicalGrooveMetadata::from_groove_asset(generation, &groove, tracing_halo).unwrap();
        let range = GrooveFrameRange::new(0, metadata.total_frame_count()).unwrap();
        let page = PhysicalGroovePage::new(
            metadata,
            range,
            range,
            groove.lateral_displacement_m().to_vec(),
            groove.vertical_displacement_m().to_vec(),
        )
        .unwrap();
        (metadata, page)
    }

    #[test]
    fn streaming_cutter_facade_preserves_arbitrary_f32_chunk_boundaries() {
        let sample_rate_hz = 44_100.0;
        let (left, right) = streaming_programme(257, sample_rate_hz);
        let config = streaming_config(sample_rate_hz, 2, left.len(), 17);

        let mut expected_cutter = StreamingGrooveCutter::new(config).unwrap();
        let mut expected_pages = Vec::new();
        expected_cutter
            .push_chunk(0, &[&left, &right], |page| expected_pages.push(page))
            .unwrap();
        let expected_finalization = expected_cutter.finish().unwrap();

        let mut cutter = WasmStreamingGrooveCutter::create(config).unwrap();
        let mut pages = Vec::new();
        let chunk_sizes = [1_usize, 11, 3, 29, 2, 47, 5, 17];
        let mut source_offset = 0_usize;
        let mut chunk_index = 0_usize;
        let mut restored_with_pending_page = false;
        while source_offset < left.len() {
            let chunk_end =
                (source_offset + chunk_sizes[chunk_index % chunk_sizes.len()]).min(left.len());
            chunk_index += 1;
            let mut prefix_offset = source_offset;
            loop {
                let status = cutter.push_planar_f32(
                    prefix_offset as u64,
                    &left[prefix_offset..chunk_end],
                    &right[prefix_offset..chunk_end],
                );
                let consumed = cutter.last_consumed_source_frame_count() as usize;
                assert!(consumed <= chunk_end - prefix_offset);
                prefix_offset += consumed;
                match status {
                    WasmStreamingGrooveCutterStatus::PageReady => {
                        assert!(cutter.emitted_page_ready());
                        assert_eq!(
                            cutter.push_planar_f32(
                                prefix_offset as u64,
                                &left[prefix_offset..chunk_end],
                                &right[prefix_offset..chunk_end],
                            ),
                            WasmStreamingGrooveCutterStatus::PagePending
                        );
                        if !restored_with_pending_page {
                            let snapshot = cutter.owned_snapshot();
                            let mut restored = WasmStreamingGrooveCutter::create(config).unwrap();
                            restored.restore_state(snapshot).unwrap();
                            cutter = restored;
                            restored_with_pending_page = true;
                        }
                        pages.push(take_streaming_page(&mut cutter));
                    }
                    WasmStreamingGrooveCutterStatus::Ok => {
                        assert_eq!(prefix_offset, chunk_end);
                        break;
                    }
                    other => panic!("unexpected streaming status: {other:?}"),
                }
            }
            source_offset = chunk_end;
        }

        loop {
            match cutter.finish() {
                WasmStreamingGrooveCutterStatus::PageReady => {
                    pages.push(take_streaming_page(&mut cutter));
                }
                WasmStreamingGrooveCutterStatus::Finalized => break,
                other => panic!("unexpected finalization status: {other:?}"),
            }
        }

        assert!(restored_with_pending_page);
        assert_eq!(pages, expected_pages);
        assert_eq!(cutter.finalization, Some(expected_finalization));
        assert!(cutter.finalization_present());
        assert!(cutter.progress_finished());
        assert_eq!(
            cutter.final_output_frame_count(),
            expected_finalization.output_frame_count()
        );
        assert_eq!(
            cutter.final_content_identity_version(),
            expected_finalization.content_identity().identity_version()
        );
        assert_eq!(
            cutter.finish(),
            WasmStreamingGrooveCutterStatus::AlreadyFinished
        );
    }

    #[test]
    fn streaming_cutter_facade_normalizes_planar_s16_without_retaining_pages() {
        let samples = [
            i16::MIN,
            -24_000,
            -1,
            0,
            1,
            8_000,
            16_000,
            i16::MAX,
            -7_500,
            12_345,
            -22_222,
            30_000,
        ];
        let normalized = samples
            .iter()
            .map(|sample| f32::from(*sample) / 32_768.0)
            .collect::<Vec<_>>();
        let config = streaming_config(192_000.0, 1, samples.len(), 5);

        let mut expected_cutter = StreamingGrooveCutter::new(config).unwrap();
        let mut expected_pages = Vec::new();
        expected_cutter
            .push_chunk(0, &[&normalized], |page| expected_pages.push(page))
            .unwrap();
        let expected_finalization = expected_cutter.finish().unwrap();

        let mut cutter = WasmStreamingGrooveCutter::create(config).unwrap();
        let mut pages = Vec::new();
        let mut offset = 0_usize;
        while offset < samples.len() {
            match cutter.push_planar_s16(offset as u64, &samples[offset..], &[]) {
                WasmStreamingGrooveCutterStatus::PageReady => {
                    offset += cutter.last_consumed_source_frame_count() as usize;
                    pages.push(take_streaming_page(&mut cutter));
                }
                WasmStreamingGrooveCutterStatus::Ok => {
                    offset += cutter.last_consumed_source_frame_count() as usize;
                }
                other => panic!("unexpected signed PCM status: {other:?}"),
            }
        }
        loop {
            match cutter.finish() {
                WasmStreamingGrooveCutterStatus::PageReady => {
                    pages.push(take_streaming_page(&mut cutter));
                }
                WasmStreamingGrooveCutterStatus::Finalized => break,
                other => panic!("unexpected signed PCM finish status: {other:?}"),
            }
        }

        assert_eq!(pages, expected_pages);
        assert_eq!(cutter.finalization, Some(expected_finalization));
        assert!(cutter.s16_left.capacity() >= samples.len());
        assert!(cutter.s16_right.is_empty());
    }

    #[test]
    fn streaming_cutter_snapshot_rejects_corruption_transactionally() {
        let (left, right) = streaming_programme(41, 48_000.0);
        let config = streaming_config(48_000.0, 2, left.len(), 1);
        let mut cutter = WasmStreamingGrooveCutter::create(config).unwrap();
        loop {
            if cutter.push_planar_f32(0, &left, &right)
                == WasmStreamingGrooveCutterStatus::PageReady
            {
                break;
            }
        }
        let before = cutter.owned_snapshot();

        let mut bad_version = before.clone();
        bad_version.version = WASM_STREAMING_GROOVE_CUTTER_SNAPSHOT_VERSION + 1;
        assert!(cutter.restore_state(bad_version).is_err());
        assert_eq!(cutter.owned_snapshot(), before);

        let mut bad_core_value = serde_json::to_value(&before).unwrap();
        bad_core_value["cutter"]["version"] = serde_json::json!(u32::MAX);
        let bad_core: WasmStreamingGrooveCutterSnapshot =
            serde_json::from_value(bad_core_value).unwrap();
        assert!(cutter.restore_state(bad_core).is_err());
        assert_eq!(cutter.owned_snapshot(), before);

        let mut bad_page_value = serde_json::to_value(&before).unwrap();
        bad_page_value["emittedPage"]["coreEndFrameExclusive"] = serde_json::json!(0);
        let bad_page: WasmStreamingGrooveCutterSnapshot =
            serde_json::from_value(bad_page_value).unwrap();
        assert!(cutter.restore_state(bad_page).is_err());
        assert_eq!(cutter.owned_snapshot(), before);
    }

    #[test]
    fn scalar_scratch_mapper_unwraps_the_branch_cut_without_js_objects() {
        let mut mapper = scratch_mapper();
        assert_eq!(
            mapper.begin_scalar(1, 0, std::f64::consts::PI - 0.02, 0.12, true, 0.5, 0, 5.0,),
            WasmPhysicalScratchGestureStatus::Ok
        );
        assert!(mapper.scalar_result_present());
        assert_eq!(
            mapper.update_scalar(
                1,
                10_000_000,
                -std::f64::consts::PI + 0.03,
                0.12,
                true,
                0.5,
                0,
            ),
            WasmPhysicalScratchGestureStatus::Ok
        );
        assert!((mapper.scalar_hand_target_angle_rad() - 5.05).abs() < 1.0e-12);
        assert!(mapper.scalar_hand_target_angular_velocity_rad_s() > 0.0);
        assert_eq!(mapper.scalar_pointer_id(), 1);
        assert!(mapper.scalar_hand_contact());
    }

    #[test]
    fn scalar_scratch_mapper_preserves_128_rapid_reversals() {
        let mut mapper = scratch_mapper();
        assert_eq!(
            mapper.begin_scalar(1, 0, 0.0, 0.12, false, f64::NAN, 100, 0.0),
            WasmPhysicalScratchGestureStatus::Ok
        );
        let mut previous_frame = mapper.scalar_absolute_frame();
        for index in 1..=128_u64 {
            let angle = if index % 2 == 0 { 0.0 } else { 0.02 };
            assert_eq!(
                mapper.update_scalar(1, index * 1_000_000, angle, 0.12, false, f64::NAN, 100,),
                WasmPhysicalScratchGestureStatus::Ok
            );
            assert!(mapper.scalar_absolute_frame() > previous_frame);
            if index % 2 == 0 {
                assert!(mapper.scalar_hand_target_angular_velocity_rad_s() < 0.0);
            } else {
                assert!(mapper.scalar_hand_target_angular_velocity_rad_s() > 0.0);
            }
            previous_frame = mapper.scalar_absolute_frame();
        }
        assert_eq!(
            mapper.finish_scalar(1, 129_000_000, 100),
            WasmPhysicalScratchGestureStatus::Ok
        );
        assert!(!mapper.scalar_hand_contact());
        assert!(!mapper.scalar_hand_target_angle_present());
    }

    #[test]
    fn facade_accepts_every_supported_output_rate() {
        for rate in SUPPORTED_PHYSICAL_OUTPUT_RATES_HZ {
            let renderer = renderer(rate);
            assert_eq!(renderer.output_sample_rate_hz(), rate);
            assert_eq!(renderer.internal_sample_rate_hz(), 192_000);
            assert_eq!(
                renderer.output_buffer_len(),
                renderer.maximum_render_frames() * 2
            );
            assert!(WasmPhysicalHostRenderer::supports_output_sample_rate(rate));
        }
        assert!(!WasmPhysicalHostRenderer::supports_output_sample_rate(
            32_000
        ));
        assert!(WasmPhysicalHostRenderer::create(32_000, None, TEST_HOST_OUTPUT).is_err());
    }

    #[test]
    fn custom_profiles_are_validated_before_the_facade_is_created() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        assert!(
            WasmPhysicalHostRenderer::create(48_000, Some(profile.clone()), TEST_HOST_OUTPUT)
                .is_ok()
        );

        let mut invalid = profile;
        invalid.name.clear();
        assert!(WasmPhysicalHostRenderer::create(48_000, Some(invalid), TEST_HOST_OUTPUT).is_err());
    }

    #[test]
    fn scalar_control_conversion_preserves_the_complete_state() {
        let control = complete_control_from_scalars(
            WasmPhysicalMotorMode::Brake as u32,
            -2.5,
            true,
            true,
            -1.25,
            4.5,
            2.0,
            0.11,
            -0.01,
            true,
        )
        .unwrap();
        assert_eq!(control.deck.motor_mode, MotorMode::Brake);
        assert_eq!(control.deck.motor_target_angular_velocity_rad_s, -2.5);
        assert!(control.deck.hand_contact);
        assert_eq!(control.deck.hand_target_angle_rad, Some(-1.25));
        assert_eq!(control.deck.hand_target_angular_velocity_rad_s, 4.5);
        assert_eq!(control.deck.hand_normal_force_n, 2.0);
        assert_eq!(control.deck.hand_contact_radius_m, 0.11);
        assert_eq!(control.deck.stylus_torque_nm, -0.01);
        assert!(control.stylus_lowered);

        let without_angle = complete_control_from_scalars(
            WasmPhysicalMotorMode::Off as u32,
            0.0,
            false,
            false,
            f64::NAN,
            0.0,
            0.0,
            0.12,
            0.0,
            false,
        )
        .unwrap();
        assert_eq!(without_angle.deck.hand_target_angle_rad, None);
        assert_eq!(
            complete_control_from_scalars(99, 0.0, false, false, 0.0, 0.0, 0.0, 0.12, 0.0, false,),
            Err(WasmPhysicalControlStatus::InvalidMotorMode)
        );
    }

    #[test]
    fn timed_control_statuses_preserve_exact_queue_failures() {
        let mut renderer = renderer(48_000);
        let submit = |renderer: &mut WasmPhysicalHostRenderer, frame, sequence| {
            renderer.enqueue_timed_control(
                frame,
                sequence,
                WasmPhysicalMotorMode::Servo as u32,
                3.0,
                true,
                true,
                0.25,
                1.0,
                1.0,
                0.12,
                true,
            )
        };
        assert_eq!(submit(&mut renderer, 10, 1), WasmPhysicalControlStatus::Ok);
        assert_eq!(
            submit(&mut renderer, 10, 1),
            WasmPhysicalControlStatus::Duplicate
        );
        assert_eq!(
            submit(&mut renderer, 9, 2),
            WasmPhysicalControlStatus::NonmonotonicFrame
        );
        assert_eq!(
            renderer
                .enqueue_timed_control(11, 2, 99, 0.0, false, false, 0.0, 0.0, 0.0, 0.12, false,),
            WasmPhysicalControlStatus::InvalidMotorMode
        );
    }

    #[test]
    fn render_uses_one_fixed_output_allocation_and_scalar_telemetry() {
        let mut renderer = renderer(48_000);
        let output_address = renderer.output_buffer_ptr() as usize;
        let output_length = renderer.output_buffer_len();
        let generation = renderer.memory_view_generation();

        for frame_count in [0, 1, 128, 257] {
            assert_eq!(renderer.render(frame_count), WasmPhysicalRenderStatus::Ok);
            assert_eq!(renderer.output_frame_count(), frame_count);
            assert_eq!(renderer.output_buffer_ptr() as usize, output_address);
            assert_eq!(renderer.output_buffer_len(), output_length);
            assert_eq!(renderer.memory_view_generation(), generation);
            assert!(renderer.output[..frame_count as usize * 2]
                .iter()
                .all(|sample| sample.is_finite()));
            assert_eq!(
                renderer.telemetry_absolute_internal_frame(),
                renderer.current_internal_frame()
            );
        }

        assert_eq!(
            renderer.render(renderer.maximum_render_frames() + 1),
            WasmPhysicalRenderStatus::BlockTooLarge
        );
        assert_eq!(renderer.output_frame_count(), 0);
        assert_eq!(renderer.output_buffer_ptr() as usize, output_address);
    }

    #[test]
    fn full_pcm_load_updates_identity_and_the_memory_view_generation() {
        let mut renderer = renderer(48_000);
        let output_address = renderer.output_buffer_ptr() as usize;
        let generation = renderer.memory_view_generation();
        let pcm = [0.0_f32, 0.25, -0.25, 0.125, -0.125, 0.0, 0.1, -0.1];

        assert_eq!(
            renderer.load_interleaved_pcm(&pcm, 1, 192_000.0),
            WasmPhysicalSourceStatus::Ok
        );
        assert_eq!(renderer.memory_view_generation(), generation + 1);
        assert_eq!(renderer.output_buffer_ptr() as usize, output_address);
        assert!(renderer.source_identity_present());
        assert_ne!(renderer.source_identity_version(), 0);
        assert_eq!(renderer.source_kind(), WasmPhysicalSourceKind::Contiguous);
        assert!(
            (0..32).any(|index| renderer.source_identity_byte(index) != 0),
            "the content digest must not be all zeros"
        );

        renderer.unload_groove();
        assert!(!renderer.source_identity_present());
        assert_eq!(renderer.source_kind(), WasmPhysicalSourceKind::None);
    }

    #[test]
    fn page_miss_status_keeps_typed_scalar_details() {
        let mut renderer = renderer(48_000);
        let requested_generation = GrooveGenerationId::new(7).unwrap();
        let cached_generation = GrooveGenerationId::new(8).unwrap();
        let stale =
            PhysicalHostRendererError::Player(PhysicalRecordPlayerError::PagedGrooveRenderMiss(
                PagedGrooveRenderMiss::StaleGeneration {
                    requested_generation,
                    cached_generation,
                },
            ));
        assert_eq!(
            renderer.record_render_error(&stale),
            WasmPhysicalRenderStatus::PageMiss
        );
        assert_eq!(
            renderer.last_page_miss_kind(),
            WasmPhysicalPageMissKind::StaleGeneration
        );
        assert_eq!(renderer.last_page_miss_requested_generation(), 7);
        assert_eq!(renderer.last_page_miss_cached_generation(), 8);

        renderer.clear_page_miss();
        let unavailable =
            PhysicalHostRendererError::Player(PhysicalRecordPlayerError::PagedGrooveRenderMiss(
                PagedGrooveRenderMiss::PageUnavailable {
                    generation: requested_generation,
                    frame: 12_345,
                },
            ));
        assert_eq!(
            renderer.record_render_error(&unavailable),
            WasmPhysicalRenderStatus::PageMiss
        );
        assert_eq!(
            renderer.last_page_miss_kind(),
            WasmPhysicalPageMissKind::PageUnavailable
        );
        assert_eq!(renderer.last_page_miss_frame(), 12_345);
        assert_eq!(renderer.last_page_miss_requested_generation(), 7);
    }

    #[test]
    fn paged_prefetch_uses_fixed_scalar_range_storage() {
        let mut renderer = renderer(48_000);
        let ranges_address = renderer.paged_prefetch_ranges.as_ptr() as usize;
        let generation = renderer.memory_view_generation();
        assert_eq!(
            renderer.build_paged_prefetch_plan(2_048, &[-1, 0, 1]),
            WasmPhysicalPagedSourceStatus::NoLoadedPagedSource
        );
        assert_eq!(renderer.memory_view_generation(), generation + 1);
        assert_eq!(
            renderer.paged_prefetch_ranges.as_ptr() as usize,
            ranges_address
        );
        assert!(!renderer.paged_prefetch_available());
        assert_eq!(renderer.paged_prefetch_range_count(), 0);
        assert_eq!(renderer.paged_prefetch_range_start(0), 0);
        assert_eq!(renderer.paged_prefetch_range_end(0), 0);
        assert_eq!(
            renderer.refresh_loaded_paged_cache(),
            WasmPhysicalPagedSourceStatus::NoPublishedCache
        );
    }

    #[test]
    fn bounded_typed_page_staging_builds_one_canonical_page() {
        let mut renderer = renderer(48_000);
        assert_eq!(
            renderer.prepare_paged_page(64),
            WasmPhysicalPagedSourceStatus::NoCacheProducer
        );

        let pcm = [0.0_f32; 64];
        let groove = renderer.cut_interleaved_pcm(&pcm, 1, 192_000.0).unwrap();
        let generation = GrooveGenerationId::new(91).unwrap();
        let seed_metadata =
            PhysicalGrooveMetadata::from_groove_asset(generation, &groove, 1).unwrap();
        let tracing_halo = seed_metadata
            .minimum_tracing_halo_frames(renderer.inner.profile().config.stylus)
            .unwrap()
            .max(1);
        let metadata =
            PhysicalGrooveMetadata::from_groove_asset(generation, &groove, tracing_halo).unwrap();
        renderer.paged_cache_producer = Some(
            PagedGrooveCacheProducer::new(metadata, PagedGrooveCacheLimits::default()).unwrap(),
        );

        assert_eq!(renderer.staged_paged_generation(), generation.get());
        assert_eq!(
            renderer.staged_paged_total_frames(),
            metadata.total_frame_count()
        );
        assert_eq!(renderer.staged_paged_tracing_halo_frames(), tracing_halo);
        assert_eq!(renderer.staged_paged_page_count(), 0);
        assert_eq!(renderer.staged_paged_resident_bytes(), 0);

        let stored_frame_count = u32::try_from(metadata.total_frame_count()).unwrap();
        let view_generation = renderer.memory_view_generation();
        assert_eq!(
            renderer.prepare_paged_page(stored_frame_count),
            WasmPhysicalPagedSourceStatus::Ok
        );
        assert_eq!(renderer.memory_view_generation(), view_generation + 1);
        assert_eq!(
            renderer.prepared_paged_page_frame_count(),
            stored_frame_count
        );
        assert!(!renderer.prepared_paged_page_lateral_ptr().is_null());
        assert!(!renderer.prepared_paged_page_vertical_ptr().is_null());
        renderer
            .prepared_page_lateral_displacement_m
            .copy_from_slice(groove.lateral_displacement_m());
        renderer
            .prepared_page_vertical_displacement_m
            .copy_from_slice(groove.vertical_displacement_m());

        assert_eq!(
            renderer.commit_prepared_paged_page(
                0,
                metadata.total_frame_count(),
                0,
                metadata.total_frame_count(),
            ),
            WasmPhysicalPagedSourceStatus::Ok
        );
        assert_eq!(renderer.memory_view_generation(), view_generation + 2);
        assert_eq!(renderer.prepared_paged_page_frame_count(), 0);
        assert_eq!(renderer.staged_paged_page_count(), 1);
        assert!(renderer.staged_paged_resident_bytes() > 0);
        assert_eq!(
            renderer.commit_prepared_paged_page(
                0,
                metadata.total_frame_count(),
                0,
                metadata.total_frame_count(),
            ),
            WasmPhysicalPagedSourceStatus::NoPreparedPage
        );
    }

    #[test]
    fn worker_materializer_exports_canonical_base_pyramid_and_identity() {
        let (metadata, expected) = canonical_page_fixture();
        assert_eq!(
            WasmStreamingGrooveCutter::seed_minimum_tracing_halo_frames(192_000.0, 512).unwrap(),
            metadata
                .minimum_tracing_halo_frames(
                    PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed()
                        .config
                        .stylus,
                )
                .unwrap()
        );
        let frame_count = expected.lateral_displacement_m().len();
        let mut materializer = WasmPhysicalGroovePageMaterializer {
            metadata,
            lateral_displacement_m: vec![0.0; frame_count].into_boxed_slice(),
            vertical_displacement_m: vec![0.0; frame_count].into_boxed_slice(),
            prepared: None,
            materialized: None,
            memory_view_generation: 1,
        };
        assert_eq!(
            materializer.prepare_raw_page(
                STREAMING_GROOVE_PAGE_CHUNK_FORMAT_VERSION,
                metadata.total_frame_count(),
                metadata.required_storage_halo_frames(),
                expected.core_range().start_frame(),
                expected.core_range().end_frame_exclusive(),
                expected.stored_range().start_frame(),
                expected.stored_range().end_frame_exclusive(),
            ),
            WasmPhysicalGroovePageMaterializerStatus::Ok
        );
        materializer
            .lateral_displacement_m
            .copy_from_slice(expected.lateral_displacement_m());
        materializer
            .vertical_displacement_m
            .copy_from_slice(expected.vertical_displacement_m());
        let view_generation = materializer.memory_view_generation();
        assert_eq!(
            materializer.materialize_prepared_page(),
            WasmPhysicalGroovePageMaterializerStatus::Ok
        );
        assert_eq!(materializer.memory_view_generation(), view_generation + 1);
        let exported = materializer.take_materialized_page().unwrap();
        assert_eq!(exported.generation(), metadata.generation().get());
        assert_eq!(exported.stored_frame_count() as usize, frame_count);
        assert_eq!(exported.spatial_level_count(), 4);
        assert_eq!(
            exported.inner.content_identity(),
            expected.content_identity()
        );
        assert_eq!(
            exported.inner.asset_content_identity(),
            expected.asset_content_identity()
        );
        assert_eq!(
            exported.content_identity_word(0),
            sha256_word(expected.content_identity().sha256(), 0)
        );
        assert!(!exported.lateral_displacement_ptr().is_null());
        assert!(!exported.vertical_displacement_ptr().is_null());
        for level_index in 0..exported.spatial_level_count() {
            let expected_level = expected
                .spatial_pyramid()
                .unwrap()
                .levels()
                .get(level_index as usize)
                .unwrap();
            assert_eq!(
                exported.spatial_level_first_source_frame(level_index),
                expected_level.first_source_frame()
            );
            assert_eq!(
                exported.spatial_level_source_frame_step(level_index),
                expected_level.source_frame_step()
            );
            assert_eq!(
                exported.spatial_level_frame_count(level_index) as usize,
                expected_level.lateral_displacement_m().len()
            );
            assert!(!exported.spatial_level_lateral_ptr(level_index).is_null());
            assert!(!exported.spatial_level_vertical_ptr(level_index).is_null());
        }
    }

    #[test]
    fn realtime_facade_allows_only_one_global_wasm_staging_reservation() {
        let (seed_metadata, _) = canonical_page_fixture();
        let seed_storage_halo = seed_metadata.required_storage_halo_frames();
        let fixture_renderer = renderer(48_000);
        let pcm = vec![0.0_f32; seed_storage_halo as usize * 16 + 64];
        let groove = fixture_renderer
            .cut_interleaved_pcm(&pcm, 1, 192_000.0)
            .unwrap();
        let generation = GrooveGenerationId::new(702).unwrap();
        let seed_metadata =
            PhysicalGrooveMetadata::from_groove_asset(generation, &groove, 1).unwrap();
        let tracing_halo = seed_metadata
            .minimum_tracing_halo_frames(fixture_renderer.inner.profile().config.stylus)
            .unwrap()
            .max(1);
        let metadata =
            PhysicalGrooveMetadata::from_groove_asset(generation, &groove, tracing_halo).unwrap();
        let storage_halo = u64::from(metadata.required_storage_halo_frames());
        let maximum_stored_frames = u32::try_from(storage_halo * 2 + 1).unwrap();
        let cache = RealtimePagedGrooveCache::new(
            metadata,
            RealtimePagedGrooveCacheConfig {
                page_slots: 2,
                maximum_stored_frames_per_page: maximum_stored_frames,
                maximum_chunk_frames: 1,
                maximum_work_units_per_call: 262_144,
                maximum_resident_bytes: 128 * 1_024 * 1_024,
            },
        )
        .unwrap();
        let mut renderer = renderer(48_000);
        renderer.inner.load_realtime_paged_groove(cache).unwrap();

        let total_frames = metadata.total_frame_count();
        let second_core_start = storage_halo * 2 + 1;
        let second_core_end = second_core_start + 1;
        assert!(second_core_end + storage_halo <= total_frames);
        let identity_version = GrooveContentIdentity::from_sha256([0; 32]).identity_version();
        let begin = |renderer: &mut WasmPhysicalHostRenderer,
                     digest_byte: u8,
                     core_start: u64,
                     core_end: u64,
                     stored_start: u64,
                     stored_end: u64| {
            let digest = [digest_byte; 32];
            let digest_words = std::array::from_fn(|index| sha256_word(digest, index as u32));
            assert_eq!(
                renderer.begin_realtime_paged_page_internal(
                    true,
                    identity_version,
                    digest_words,
                    core_start,
                    core_end,
                    stored_start,
                    stored_end,
                ),
                WasmPhysicalRealtimePagedStatus::Ok
            );
            (
                renderer.realtime_paged_last_ticket_slot_index(),
                renderer.realtime_paged_last_ticket_sequence(),
            )
        };
        let (first_slot, first_ticket) = begin(&mut renderer, 0x31, 0, 1, 0, storage_halo + 1);
        let (second_slot, second_ticket) = begin(
            &mut renderer,
            0x32,
            second_core_start,
            second_core_end,
            storage_halo + 1,
            second_core_end + storage_halo,
        );
        assert_ne!(first_slot, second_slot);

        assert_eq!(
            renderer.reserve_realtime_paged_chunk(
                first_slot,
                first_ticket,
                WasmRealtimePagedChunkTarget::BaseLateral,
                0,
                1,
            ),
            WasmPhysicalRealtimePagedStatus::Ok
        );
        let first_reservation =
            renderer.realtime_paged_reservation_sequence(first_slot, first_ticket);
        let first_pointer =
            renderer.realtime_paged_reserved_chunk_ptr(first_slot, first_ticket, first_reservation);
        assert!(!first_pointer.is_null());
        assert_eq!(
            renderer.reserve_realtime_paged_chunk(
                second_slot,
                second_ticket,
                WasmRealtimePagedChunkTarget::BaseLateral,
                0,
                1,
            ),
            WasmPhysicalRealtimePagedStatus::ChunkReservationActive
        );
        assert_eq!(
            renderer.realtime_paged_reservation_sequence(second_slot, second_ticket),
            0
        );
        assert_eq!(
            renderer.cancel_realtime_paged_reserved_chunk(
                first_slot,
                first_ticket,
                first_reservation,
            ),
            WasmPhysicalRealtimePagedStatus::Ok
        );

        assert_eq!(
            renderer.reserve_realtime_paged_chunk(
                second_slot,
                second_ticket,
                WasmRealtimePagedChunkTarget::BaseLateral,
                0,
                1,
            ),
            WasmPhysicalRealtimePagedStatus::Ok
        );
        let second_reservation =
            renderer.realtime_paged_reservation_sequence(second_slot, second_ticket);
        let second_pointer = renderer.realtime_paged_reserved_chunk_ptr(
            second_slot,
            second_ticket,
            second_reservation,
        );
        assert_eq!(second_pointer, first_pointer);
        assert_eq!(
            renderer.cancel_realtime_paged_reserved_chunk(
                second_slot,
                second_ticket,
                second_reservation,
            ),
            WasmPhysicalRealtimePagedStatus::Ok
        );
    }

    #[test]
    fn realtime_facade_reserves_wasm_storage_and_publishes_precomputed_page() {
        let (metadata, page) = canonical_page_fixture();
        let stored_frames = page.lateral_displacement_m().len() as u32;
        let cache = RealtimePagedGrooveCache::new(
            metadata,
            RealtimePagedGrooveCacheConfig {
                page_slots: 2,
                maximum_stored_frames_per_page: stored_frames,
                maximum_chunk_frames: stored_frames,
                maximum_work_units_per_call: 262_144,
                maximum_resident_bytes: 8 * 1_024 * 1_024,
            },
        )
        .unwrap();
        let mut renderer = renderer(48_000);
        renderer.inner.load_realtime_paged_groove(cache).unwrap();
        renderer.sync_source_identity();

        let digest = page.content_identity().sha256();
        let digest_words = std::array::from_fn(|index| sha256_word(digest, index as u32));
        assert_eq!(
            renderer.begin_realtime_paged_page_internal(
                true,
                page.content_identity().identity_version(),
                digest_words,
                page.core_range().start_frame(),
                page.core_range().end_frame_exclusive(),
                page.stored_range().start_frame(),
                page.stored_range().end_frame_exclusive(),
            ),
            WasmPhysicalRealtimePagedStatus::Ok
        );
        let slot = renderer.realtime_paged_last_ticket_slot_index();
        let ticket_sequence = renderer.realtime_paged_last_ticket_sequence();

        let mut transfer = |target: WasmRealtimePagedChunkTarget, samples: &[f32]| {
            assert_eq!(
                renderer.reserve_realtime_paged_chunk(
                    slot,
                    ticket_sequence,
                    target,
                    0,
                    samples.len() as u32,
                ),
                WasmPhysicalRealtimePagedStatus::Ok
            );
            let reservation_sequence =
                renderer.realtime_paged_reservation_sequence(slot, ticket_sequence);
            let pointer = renderer.realtime_paged_reserved_chunk_ptr(
                slot,
                ticket_sequence,
                reservation_sequence,
            );
            assert!(!pointer.is_null());
            assert_eq!(pointer, renderer.realtime_paged_chunk_staging.as_mut_ptr());
            assert_eq!(
                renderer.realtime_paged_reserved_chunk_len(
                    slot,
                    ticket_sequence,
                    reservation_sequence,
                ) as usize,
                samples.len()
            );
            renderer.realtime_paged_chunk_staging[..samples.len()].copy_from_slice(samples);
            assert_eq!(
                renderer.commit_realtime_paged_reserved_chunk(
                    slot,
                    ticket_sequence,
                    reservation_sequence,
                ),
                WasmPhysicalRealtimePagedStatus::Ok
            );
            renderer.realtime_paged_chunk_staging[0] = f32::NAN;
            assert!(renderer
                .realtime_paged_reserved_chunk_ptr(slot, ticket_sequence, reservation_sequence,)
                .is_null());
        };

        transfer(
            WasmRealtimePagedChunkTarget::BaseLateral,
            page.lateral_displacement_m(),
        );
        transfer(
            WasmRealtimePagedChunkTarget::BaseVertical,
            page.vertical_displacement_m(),
        );
        for (level_index, level) in page.spatial_pyramid().unwrap().levels().iter().enumerate() {
            transfer(
                WasmRealtimePagedChunkTarget::SpatialLevelLateral(level_index as u8),
                level.lateral_displacement_m(),
            );
            transfer(
                WasmRealtimePagedChunkTarget::SpatialLevelVertical(level_index as u8),
                level.vertical_displacement_m(),
            );
        }
        drop(transfer);

        assert_eq!(
            renderer.finish_realtime_paged_page_ingestion(slot, ticket_sequence),
            WasmPhysicalRealtimePagedStatus::Ok
        );
        while renderer.realtime_paged_last_progress_phase() != WasmPhysicalRealtimePagedPhase::Ready
        {
            assert_eq!(
                renderer.advance_realtime_paged_page(slot, ticket_sequence, 262_144),
                WasmPhysicalRealtimePagedStatus::Ok
            );
        }
        assert_eq!(
            renderer.publish_realtime_paged_page(slot, ticket_sequence),
            WasmPhysicalRealtimePagedStatus::Ok
        );
        assert_eq!(
            renderer.realtime_paged_last_progress_phase(),
            WasmPhysicalRealtimePagedPhase::Published
        );
        assert_eq!(renderer.realtime_paged_cache_published_pages(), 1);
        assert_eq!(
            renderer.source_kind(),
            WasmPhysicalSourceKind::RealtimePaged
        );
        assert_eq!(renderer.render(64), WasmPhysicalRenderStatus::Ok);
        assert_eq!(
            renderer.read_realtime_paged_progress(slot, ticket_sequence + 1),
            WasmPhysicalRealtimePagedStatus::StaleTicket
        );
    }

    #[test]
    fn core_snapshot_restore_keeps_the_output_allocation() {
        let mut renderer = renderer(48_000);
        assert_eq!(renderer.render(128), WasmPhysicalRenderStatus::Ok);
        let snapshot = renderer.inner.snapshot();
        let output_address = renderer.output_buffer_ptr() as usize;
        assert_eq!(renderer.render(256), WasmPhysicalRenderStatus::Ok);
        renderer.inner.restore(&snapshot).unwrap();
        assert_eq!(renderer.output_buffer_ptr() as usize, output_address);
        assert_eq!(
            renderer.current_internal_frame(),
            snapshot.player.current_internal_frame()
        );
    }

    #[test]
    fn header_free_documentation_names_the_realtime_contract() {
        let documentation = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/WASM_API.md"));
        for required in [
            "PhysicalHostRenderer",
            "memoryViewGeneration",
            "enqueueTimedControl",
            "PhysicalRenderStatus.PageMiss",
            "beginPagedCache",
            "preparePagedPage",
            "commitPreparedPagedPage",
            "refreshLoadedPagedCache",
            "buildPagedPrefetchPlan",
            "loadRealtimePagedCache",
            "beginRealtimePagedPagePrecomputed",
            "commitRealtimePagedReservedChunk",
            "publishRealtimePagedPage",
            "MessagePort",
            "rendering is stopped",
            "StreamingGrooveCutter",
            "pushPlanarF32",
            "pushPlanarS16",
            "takeEmittedPage",
            "PhysicalGroovePageMaterializer",
            "materializePreparedPage",
            "Web Worker",
            "scalar values",
            "does not require a C header",
        ] {
            assert!(documentation.contains(required), "missing {required}");
        }
        assert!(!documentation.contains("ScratchAcousticDsp"));
    }
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
