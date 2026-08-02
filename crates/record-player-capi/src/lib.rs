//! Provides a thin C ABI for the physical record-player engine.
//!
//! This crate validates ABI values and transfers them to `record-player`.
//! It does not define physics values or gesture behavior.

#![deny(unsafe_op_in_unsafe_fn)]

use std::cell::UnsafeCell;
use std::ffi::c_char;
use std::mem::{align_of, size_of};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

use record_player::physical::{
    physical_input_frames_for_output_frames, GrooveAsset, GrooveError, PhysicalHostOutputConfig,
    PhysicalHostRenderReport, PhysicalHostRenderer, PhysicalHostRendererError, PhysicalProfile,
    PhysicalRecordPlayerError, PhysicalRenderTelemetry, PickupContactSurface,
    PlayerControlIngressReport, RadialContactRegion, StereoOutputResamplerError,
    PHYSICAL_OUTPUT_INPUT_RATE_HZ,
};
use record_player::spsc::{
    timed_player_control_mailbox, SpscPushError, TimedPlayerControlConsumer,
    TimedPlayerControlProducer,
};
use record_player::timed_control::{ControlTimelinePushError, PlayerControl, TimedPlayerControl};
use record_player::{
    ContactMode, DeckMechanicalControl, MotorMode, ScheduledScratchHandControl,
    ScratchGestureConfig, ScratchGestureError, ScratchGestureMapper, ScratchPointerSample,
    ScratchPreset, ScratchPressureCalibration,
};

pub const RECORD_PLAYER_CAPI_ABI_VERSION: u32 = 5;

pub type RecordPlayerStatus = i32;

pub const RECORD_PLAYER_STATUS_OK: RecordPlayerStatus = 0;
pub const RECORD_PLAYER_STATUS_NULL_POINTER: RecordPlayerStatus = 1;
pub const RECORD_PLAYER_STATUS_MISALIGNED_POINTER: RecordPlayerStatus = 2;
pub const RECORD_PLAYER_STATUS_INVALID_ARGUMENT: RecordPlayerStatus = 3;
pub const RECORD_PLAYER_STATUS_UNSUPPORTED_ABI_VERSION: RecordPlayerStatus = 4;
pub const RECORD_PLAYER_STATUS_UNSUPPORTED_OUTPUT_SAMPLE_RATE: RecordPlayerStatus = 5;
pub const RECORD_PLAYER_STATUS_ALLOCATION_FAILED: RecordPlayerStatus = 6;
pub const RECORD_PLAYER_STATUS_CREATE_FAILED: RecordPlayerStatus = 7;
pub const RECORD_PLAYER_STATUS_BUSY: RecordPlayerStatus = 8;
pub const RECORD_PLAYER_STATUS_GROOVE_CUT_FAILED: RecordPlayerStatus = 10;
pub const RECORD_PLAYER_STATUS_GROOVE_LAYOUT_MISMATCH: RecordPlayerStatus = 11;
pub const RECORD_PLAYER_STATUS_GROOVE_PROGRAMME_DOES_NOT_FIT: RecordPlayerStatus = 12;
pub const RECORD_PLAYER_STATUS_GROOVE_OVERCUT: RecordPlayerStatus = 13;
pub const RECORD_PLAYER_STATUS_GROOVE_LOAD_FAILED: RecordPlayerStatus = 14;
pub const RECORD_PLAYER_STATUS_CONTROL_INVALID: RecordPlayerStatus = 20;
pub const RECORD_PLAYER_STATUS_CONTROL_LATE: RecordPlayerStatus = 21;
pub const RECORD_PLAYER_STATUS_CONTROL_DUPLICATE: RecordPlayerStatus = 22;
pub const RECORD_PLAYER_STATUS_CONTROL_NONMONOTONIC_FRAME: RecordPlayerStatus = 23;
pub const RECORD_PLAYER_STATUS_CONTROL_NONMONOTONIC_SEQUENCE: RecordPlayerStatus = 24;
pub const RECORD_PLAYER_STATUS_CONTROL_QUEUE_FULL: RecordPlayerStatus = 25;
pub const RECORD_PLAYER_STATUS_RENDER_BLOCK_TOO_LARGE: RecordPlayerStatus = 30;
pub const RECORD_PLAYER_STATUS_RENDER_FAILED: RecordPlayerStatus = 31;
pub const RECORD_PLAYER_STATUS_CORE_ERROR: RecordPlayerStatus = 40;
pub const RECORD_PLAYER_STATUS_GESTURE_INVALID: RecordPlayerStatus = 50;
pub const RECORD_PLAYER_STATUS_GESTURE_STATE: RecordPlayerStatus = 51;
pub const RECORD_PLAYER_STATUS_PANIC: RecordPlayerStatus = 127;

pub const RECORD_PLAYER_MOTOR_OFF: u32 = 0;
pub const RECORD_PLAYER_MOTOR_SERVO: u32 = 1;
pub const RECORD_PLAYER_MOTOR_BRAKE: u32 = 2;

pub const RECORD_PLAYER_CONTACT_SEPARATED: u32 = 0;
pub const RECORD_PLAYER_CONTACT_STICKING: u32 = 1;
pub const RECORD_PLAYER_CONTACT_SLIDING_POSITIVE: u32 = 2;
pub const RECORD_PLAYER_CONTACT_SLIDING_NEGATIVE: u32 = 3;

pub const RECORD_PLAYER_SCRATCH_PRESET_BABY: u32 = 0;
pub const RECORD_PLAYER_SCRATCH_PRESET_STAB: u32 = 1;
pub const RECORD_PLAYER_SCRATCH_PRESET_CHIRP: u32 = 2;
pub const RECORD_PLAYER_SCRATCH_PRESET_TRANSFORM: u32 = 3;
pub const RECORD_PLAYER_SCRATCH_PRESET_FLARE: u32 = 4;
pub const RECORD_PLAYER_SCRATCH_PRESET_CRAB: u32 = 5;
pub const RECORD_PLAYER_SCRATCH_PRESET_ORBIT: u32 = 6;
pub const RECORD_PLAYER_SCRATCH_PRESET_DRUM: u32 = 7;

pub const RECORD_PLAYER_SCRATCH_CROSSFADER_MANUAL: u32 = 0;
pub const RECORD_PLAYER_SCRATCH_CROSSFADER_AUTOMATIC_PRESET: u32 = 1;

pub const RECORD_PLAYER_PICKUP_CONTACT_NONE: u32 = 0;
pub const RECORD_PLAYER_PICKUP_CONTACT_GROOVE_WALLS: u32 = 1;
pub const RECORD_PLAYER_PICKUP_CONTACT_RECORD_LAND: u32 = 2;

pub const RECORD_PLAYER_RADIAL_REGION_GROOVE: u32 = 0;
pub const RECORD_PLAYER_RADIAL_REGION_LAND: u32 = 1;
pub const RECORD_PLAYER_RADIAL_REGION_LIFTED: u32 = 2;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerCreateOptions {
    pub abi_version: u32,
    pub output_sample_rate_hz: u32,
    pub control_mailbox_capacity: u32,
    pub reserved: u32,
    pub volts_per_full_scale: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerControl {
    pub motor_mode: u32,
    pub hand_contact: u8,
    pub hand_target_angle_present: u8,
    pub stylus_lowered: u8,
    pub reserved: u8,
    pub motor_target_angular_velocity_rad_s: f64,
    pub hand_target_angle_rad: f64,
    pub hand_target_angular_velocity_rad_s: f64,
    pub hand_normal_force_n: f64,
    pub hand_contact_radius_m: f64,
    pub manual_crossfader_gain: f64,
    pub scratch_preset: u32,
    /// Zero selects the preset's default click count.
    pub scratch_clicks: u8,
    pub scratch_reserved: [u8; 3],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerTimedControl {
    pub absolute_frame: u64,
    pub sequence: u64,
    pub control: RecordPlayerControl,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerScratchGestureOptions {
    pub abi_version: u32,
    pub lookahead_frames: u32,
    pub reserved: [u32; 2],
    pub zero_pressure_force_n: f64,
    pub unit_pressure_force_n: f64,
    pub unreported_pressure_force_n: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerScratchPointerSample {
    pub pointer_id: u64,
    pub source_time_ns: u64,
    pub angle_rad: f64,
    pub contact_radius_m: f64,
    pub normalized_pressure: f64,
    pub pressure_present: u8,
    pub reserved: [u8; 7],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerScratchControlResult {
    pub event: RecordPlayerTimedControl,
    pub raw_pointer_angular_velocity_rad_s: f64,
    pub added_late_shift_frames: u64,
    pub total_late_shift_frames: u64,
    pub velocity_was_limited: u8,
    pub wrap_was_ambiguous: u8,
    pub reserved: [u8; 6],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerInfo {
    pub abi_version: u32,
    pub output_sample_rate_hz: u32,
    pub internal_sample_rate_hz: u32,
    pub control_mailbox_capacity: u32,
    pub maximum_render_frames: u64,
    pub latency_internal_frames: u64,
    pub latency_seconds: f64,
    pub volts_per_full_scale: f64,
    pub current_internal_frame: u64,
    pub rendered_host_frames: u64,
    pub accepted_control_submissions: u64,
    pub rejected_full_control_submissions: u64,
    pub inspected_control_submissions: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordPlayerSchedulePoint {
    pub observed_rendered_host_frames: u64,
    pub current_internal_frame: u64,
    pub host_frame_offset: u64,
    pub absolute_internal_frame: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordPlayerControlIngressReport {
    pub inspected_events: u64,
    pub enqueued_events: u64,
    pub retimed_late_events: u64,
    pub rejected_invalid_encodings: u64,
    pub rejected_invalid_controls: u64,
    pub rejected_duplicates: u64,
    pub rejected_nonmonotonic_frames: u64,
    pub rejected_nonmonotonic_sequences: u64,
    pub protocol_errors: u64,
    pub stopped_for_timeline_backpressure: u8,
    pub producer_disconnected: u8,
    pub reserved: [u8; 6],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerRenderReport {
    pub output_sample_rate_hz: u32,
    pub reserved: u32,
    pub rendered_host_frames: u64,
    pub rendered_internal_frames: u64,
    pub absolute_internal_frame: u64,
    pub volts_per_full_scale: f64,
    pub peak_unclipped_abs_output_v: [f64; 2],
    pub clipped_samples: [u64; 2],
    pub total_clipped_samples: [u64; 2],
    pub control_ingress: RecordPlayerControlIngressReport,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerDeckTelemetry {
    pub mechanical_time_seconds: f64,
    pub platter_rate: f64,
    pub record_rate: f64,
    pub platter_angle_turns: f64,
    pub record_angle_turns: f64,
    pub motor_torque_nm: f64,
    pub slipmat_torque_nm: f64,
    pub hand_torque_nm: f64,
    pub bearing_torque_nm: f64,
    pub stylus_torque_nm: f64,
    pub slipmat_mode: u32,
    pub hand_mode: u32,
    pub bearing_sticking: u8,
    pub reserved: [u8; 7],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerPickupTelemetry {
    pub tip_displacement_m: [f64; 2],
    pub tip_velocity_m_s: [f64; 2],
    pub body_displacement_m: [f64; 2],
    pub body_velocity_m_s: [f64; 2],
    pub relative_displacement_m: [f64; 2],
    pub relative_velocity_m_s: [f64; 2],
    pub suspension_force_on_tip_n: [f64; 2],
    pub electromagnetic_force_on_tip_n: [f64; 2],
    pub wall_gap_m: [f64; 2],
    pub wall_normal_force_n: [f64; 2],
    pub wall_contact: [u8; 2],
    pub land_contact: u8,
    pub stylus_lowered: u8,
    pub contact_surface: u32,
    pub land_gap_m: f64,
    pub land_normal_force_n: f64,
    pub coulomb_friction_force_n: f64,
    pub modulation_reaction_force_n: f64,
    pub record_reaction_force_tangent_n: f64,
    pub groove_radius_m: f64,
    pub skating_force_n: f64,
    pub bearing_friction_force_n: f64,
    pub groove_lateral_force_on_tip_n: f64,
    pub kinetic_energy_j: f64,
    pub suspension_energy_j: f64,
    pub completed_steps: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerCartridgeTelemetry {
    pub magnet_velocity_m_s: [f64; 2],
    pub generator_voltage_v: [f64; 2],
    pub coil_current_a: [f64; 2],
    pub load_output_voltage_v: [f64; 2],
    pub electromagnetic_reaction_force_n: [f64; 2],
    pub generator_electrical_power_w: f64,
    pub coil_loss_power_w: f64,
    pub load_power_w: f64,
    pub stored_electrical_energy_j: f64,
    pub completed_steps: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerRadialTrackingTelemetry {
    pub spiral_reference_radius_m: f64,
    pub stylus_radius_m: f64,
    pub radial_velocity_m_s: f64,
    pub groove_pitch_m_per_revolution: f64,
    pub captured_groove_radius_m: f64,
    pub radial_error_to_retained_turn_m: f64,
    pub groove_radial_velocity_m_s: f64,
    pub guide_force_n: f64,
    pub bearing_friction_force_n: f64,
    pub applied_radial_force_n: f64,
    pub captured_groove_radius_present: u8,
    pub groove_contact: u8,
    pub land_contact: u8,
    pub contact_lost_this_step: u8,
    pub recaptured_this_step: u8,
    pub recapture_limit_reached: u8,
    pub stylus_lowered: u8,
    pub macro_contact_available: u8,
    pub contact_region: u32,
    pub reserved: u32,
    pub captured_turn_index: i64,
    pub turns_skipped_this_step: i64,
    pub total_turns_skipped: i64,
    pub completed_steps: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerScratchTelemetry {
    pub preset: u32,
    pub crossfader_owner: u32,
    pub clicks: u8,
    pub direction: i8,
    pub moving: u8,
    pub reserved: [u8; 5],
    pub audible_gain: f64,
    pub automatic_gate_gain: f64,
    pub automatic_gate_target: f64,
    pub phase: f64,
    pub stroke_progress: f64,
    pub span_prediction_confidence: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordPlayerTelemetry {
    pub abi_version: u32,
    pub output_sample_rate_hz: u32,
    pub rendered_host_frames: u64,
    pub rendered_internal_frames: u64,
    pub absolute_internal_frame: u64,
    pub spiral_frame_position: f64,
    pub groove_frame_position: f64,
    pub groove_radius_m: f64,
    pub spatial_filter_lower_step_frames: u32,
    pub spatial_filter_upper_step_frames: u32,
    pub spatial_filter_upper_blend: f64,
    pub phono_output_v: [f64; 2],
    pub groove_loaded: u8,
    pub at_programme_boundary: u8,
    pub phono_input_overload: [u8; 2],
    pub phono_output_overload: [u8; 2],
    pub swept_contact_substeps: u8,
    pub reserved: [u8; 1],
    pub radial_tracking: RecordPlayerRadialTrackingTelemetry,
    pub deck: RecordPlayerDeckTelemetry,
    pub pickup: RecordPlayerPickupTelemetry,
    pub cartridge: RecordPlayerCartridgeTelemetry,
    pub scratch: RecordPlayerScratchTelemetry,
}

struct RenderSide {
    renderer: PhysicalHostRenderer,
    control_consumer: TimedPlayerControlConsumer,
}

/// Owns one producer endpoint and one render endpoint.
/// C code treats this type as opaque.
pub struct RecordPlayerHandle {
    control_producer: UnsafeCell<TimedPlayerControlProducer>,
    render_side: UnsafeCell<RenderSide>,
    output_sample_rate_hz: u32,
    control_mailbox_capacity: u32,
    maximum_render_frames: u64,
    latency_internal_frames: u64,
    latency_seconds: f64,
    volts_per_full_scale: f64,
    deck_config: record_player::PhysicalDeckConfig,
    rendered_host_frames: AtomicU64,
    accepted_control_submissions: AtomicU64,
    rejected_full_control_submissions: AtomicU64,
    inspected_control_submissions: AtomicU64,
    access_state: AtomicU8,
}

/// Owns the canonical pointer-to-hand mapper for one gesture stream.
pub struct RecordPlayerScratchGestureHandle {
    mapper: UnsafeCell<ScratchGestureMapper>,
    access_state: AtomicU8,
}

// SAFETY: The atomic access state permits at most one reference to each cell.
// It excludes lifecycle access from both cells. Cross-thread values use the
// core SPSC mailbox or atomics.
unsafe impl Sync for RecordPlayerHandle {}

// SAFETY: The atomic guard permits only one mapper reference at a time.
unsafe impl Sync for RecordPlayerScratchGestureHandle {}

const PRODUCER_ACTIVE: u8 = 1 << 0;
const RENDER_ACTIVE: u8 = 1 << 1;
const LIFECYCLE_ACTIVE: u8 = 1 << 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccessLane {
    Producer,
    Render,
    Lifecycle,
}

impl AccessLane {
    const fn bit(self) -> u8 {
        match self {
            Self::Producer => PRODUCER_ACTIVE,
            Self::Render => RENDER_ACTIVE,
            Self::Lifecycle => LIFECYCLE_ACTIVE,
        }
    }

    const fn conflict_mask(self) -> u8 {
        match self {
            Self::Producer => PRODUCER_ACTIVE | LIFECYCLE_ACTIVE,
            Self::Render => RENDER_ACTIVE | LIFECYCLE_ACTIVE,
            Self::Lifecycle => PRODUCER_ACTIVE | RENDER_ACTIVE | LIFECYCLE_ACTIVE,
        }
    }
}

struct AccessGuard<'a> {
    state: &'a AtomicU8,
    bit: u8,
    release_on_drop: bool,
}

fn acquire_scratch_access(
    handle: &RecordPlayerScratchGestureHandle,
) -> Result<AccessGuard<'_>, RecordPlayerStatus> {
    handle
        .access_state
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| RECORD_PLAYER_STATUS_BUSY)?;
    Ok(AccessGuard {
        state: &handle.access_state,
        bit: 1,
        release_on_drop: true,
    })
}

impl AccessGuard<'_> {
    fn keep_locked(mut self) {
        self.release_on_drop = false;
    }
}

impl Drop for AccessGuard<'_> {
    fn drop(&mut self) {
        if self.release_on_drop {
            let previous = self.state.fetch_and(!self.bit, Ordering::Release);
            debug_assert_ne!(previous & self.bit, 0);
        }
    }
}

fn acquire_access(
    handle: &RecordPlayerHandle,
    lane: AccessLane,
) -> Result<AccessGuard<'_>, RecordPlayerStatus> {
    let bit = lane.bit();
    let conflict_mask = lane.conflict_mask();
    let mut current = handle.access_state.load(Ordering::Acquire);
    loop {
        if current & conflict_mask != 0 {
            return Err(RECORD_PLAYER_STATUS_BUSY);
        }
        match handle.access_state.compare_exchange_weak(
            current,
            current | bit,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                return Ok(AccessGuard {
                    state: &handle.access_state,
                    bit,
                    release_on_drop: true,
                });
            }
            Err(observed) => current = observed,
        }
    }
}

fn run_ffi(action: impl FnOnce() -> Result<(), RecordPlayerStatus>) -> RecordPlayerStatus {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(Ok(())) => RECORD_PLAYER_STATUS_OK,
        Ok(Err(status)) => status,
        Err(_) => RECORD_PLAYER_STATUS_PANIC,
    }
}

fn pointer_status<T>(pointer: *const T) -> Result<(), RecordPlayerStatus> {
    if pointer.is_null() {
        return Err(RECORD_PLAYER_STATUS_NULL_POINTER);
    }
    if !(pointer as usize).is_multiple_of(align_of::<T>()) {
        return Err(RECORD_PLAYER_STATUS_MISALIGNED_POINTER);
    }
    Ok(())
}

fn slice_size_status<T>(length: usize) -> Result<(), RecordPlayerStatus> {
    let bytes = length
        .checked_mul(size_of::<T>())
        .ok_or(RECORD_PLAYER_STATUS_INVALID_ARGUMENT)?;
    if bytes > isize::MAX as usize {
        return Err(RECORD_PLAYER_STATUS_INVALID_ARGUMENT);
    }
    Ok(())
}

unsafe fn handle_ref<'a>(
    handle: *mut RecordPlayerHandle,
) -> Result<&'a RecordPlayerHandle, RecordPlayerStatus> {
    pointer_status(handle)?;
    // SAFETY: The C contract requires a live handle for the call duration.
    Ok(unsafe { &*handle })
}

unsafe fn scratch_handle_ref<'a>(
    handle: *mut RecordPlayerScratchGestureHandle,
) -> Result<&'a RecordPlayerScratchGestureHandle, RecordPlayerStatus> {
    pointer_status(handle)?;
    // SAFETY: The C contract requires a live handle for the call duration.
    Ok(unsafe { &*handle })
}

fn producer_pointer(handle: &RecordPlayerHandle) -> *mut TimedPlayerControlProducer {
    handle.control_producer.get()
}

fn render_side_pointer(handle: &RecordPlayerHandle) -> *mut RenderSide {
    handle.render_side.get()
}

fn scratch_mapper_pointer(handle: &RecordPlayerScratchGestureHandle) -> *mut ScratchGestureMapper {
    handle.mapper.get()
}

unsafe fn input_ref<'a, T>(pointer: *const T) -> Result<&'a T, RecordPlayerStatus> {
    pointer_status(pointer)?;
    // SAFETY: The C contract requires a readable value for the call duration.
    Ok(unsafe { &*pointer })
}

unsafe fn output_pointer<T>(pointer: *mut T) -> Result<*mut T, RecordPlayerStatus> {
    pointer_status(pointer)?;
    Ok(pointer)
}

unsafe fn input_slice<'a, T>(
    pointer: *const T,
    length: usize,
) -> Result<&'a [T], RecordPlayerStatus> {
    pointer_status(pointer)?;
    slice_size_status::<T>(length)?;
    // SAFETY: The C contract requires `length` readable consecutive values.
    Ok(unsafe { slice::from_raw_parts(pointer, length) })
}

unsafe fn output_slice<'a, T>(
    pointer: *mut T,
    length: usize,
) -> Result<&'a mut [T], RecordPlayerStatus> {
    pointer_status(pointer)?;
    slice_size_status::<T>(length)?;
    // SAFETY: The C contract requires `length` writable consecutive values.
    Ok(unsafe { slice::from_raw_parts_mut(pointer, length) })
}

fn motor_mode(value: u32) -> Result<MotorMode, RecordPlayerStatus> {
    match value {
        RECORD_PLAYER_MOTOR_OFF => Ok(MotorMode::Off),
        RECORD_PLAYER_MOTOR_SERVO => Ok(MotorMode::Servo),
        RECORD_PLAYER_MOTOR_BRAKE => Ok(MotorMode::Brake),
        _ => Err(RECORD_PLAYER_STATUS_CONTROL_INVALID),
    }
}

fn boolean(value: u8) -> Result<bool, RecordPlayerStatus> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(RECORD_PLAYER_STATUS_CONTROL_INVALID),
    }
}

fn scratch_preset(value: u32) -> Result<ScratchPreset, RecordPlayerStatus> {
    let id = u8::try_from(value).map_err(|_| RECORD_PLAYER_STATUS_CONTROL_INVALID)?;
    ScratchPreset::from_id(id).ok_or(RECORD_PLAYER_STATUS_CONTROL_INVALID)
}

fn timed_control(
    value: RecordPlayerTimedControl,
) -> Result<TimedPlayerControl, RecordPlayerStatus> {
    if value.control.reserved != 0 || value.control.scratch_reserved != [0; 3] {
        return Err(RECORD_PLAYER_STATUS_CONTROL_INVALID);
    }
    let target_angle_present = boolean(value.control.hand_target_angle_present)?;
    let deck = DeckMechanicalControl {
        motor_mode: motor_mode(value.control.motor_mode)?,
        motor_target_angular_velocity_rad_s: value.control.motor_target_angular_velocity_rad_s,
        hand_contact: boolean(value.control.hand_contact)?,
        hand_target_angle_rad: target_angle_present.then_some(value.control.hand_target_angle_rad),
        hand_target_angular_velocity_rad_s: value.control.hand_target_angular_velocity_rad_s,
        hand_normal_force_n: value.control.hand_normal_force_n,
        hand_contact_radius_m: value.control.hand_contact_radius_m,
        // The physical player owns stylus reaction torque. Hosts cannot inject it.
        stylus_torque_nm: 0.0,
    };
    let preset = scratch_preset(value.control.scratch_preset)?;
    let clicks = if value.control.scratch_clicks == 0 {
        preset.default_clicks()
    } else {
        value.control.scratch_clicks
    };
    if !(record_player::MIN_SCRATCH_CLICKS..=record_player::MAX_SCRATCH_CLICKS).contains(&clicks)
        || !value.control.manual_crossfader_gain.is_finite()
        || !(0.0..=1.0).contains(&value.control.manual_crossfader_gain)
    {
        return Err(RECORD_PLAYER_STATUS_CONTROL_INVALID);
    }
    Ok(TimedPlayerControl::new(
        value.absolute_frame,
        value.sequence,
        PlayerControl::new(deck, boolean(value.control.stylus_lowered)?).with_scratch(
            preset,
            clicks,
            value.control.manual_crossfader_gain,
        ),
    ))
}

fn record_motor_mode(value: MotorMode) -> u32 {
    match value {
        MotorMode::Off => RECORD_PLAYER_MOTOR_OFF,
        MotorMode::Servo => RECORD_PLAYER_MOTOR_SERVO,
        MotorMode::Brake => RECORD_PLAYER_MOTOR_BRAKE,
    }
}

fn record_control(value: PlayerControl) -> RecordPlayerControl {
    RecordPlayerControl {
        motor_mode: record_motor_mode(value.deck.motor_mode),
        hand_contact: u8::from(value.deck.hand_contact),
        hand_target_angle_present: u8::from(value.deck.hand_target_angle_rad.is_some()),
        stylus_lowered: u8::from(value.stylus_lowered),
        reserved: 0,
        motor_target_angular_velocity_rad_s: value.deck.motor_target_angular_velocity_rad_s,
        hand_target_angle_rad: value.deck.hand_target_angle_rad.unwrap_or(0.0),
        hand_target_angular_velocity_rad_s: value.deck.hand_target_angular_velocity_rad_s,
        hand_normal_force_n: value.deck.hand_normal_force_n,
        hand_contact_radius_m: value.deck.hand_contact_radius_m,
        manual_crossfader_gain: value.manual_crossfader_gain,
        scratch_preset: u32::from(value.scratch_preset.id()),
        scratch_clicks: value.scratch_clicks,
        scratch_reserved: [0; 3],
    }
}

fn record_timed_control(value: TimedPlayerControl) -> RecordPlayerTimedControl {
    RecordPlayerTimedControl {
        absolute_frame: value.absolute_frame,
        sequence: value.sequence,
        control: record_control(value.control),
    }
}

fn scratch_pointer_sample(
    value: RecordPlayerScratchPointerSample,
) -> Result<ScratchPointerSample, RecordPlayerStatus> {
    if value.reserved != [0; 7] {
        return Err(RECORD_PLAYER_STATUS_INVALID_ARGUMENT);
    }
    let pressure_present =
        boolean(value.pressure_present).map_err(|_| RECORD_PLAYER_STATUS_INVALID_ARGUMENT)?;
    if !pressure_present && value.normalized_pressure != 0.0 {
        return Err(RECORD_PLAYER_STATUS_INVALID_ARGUMENT);
    }
    Ok(ScratchPointerSample {
        pointer_id: value.pointer_id,
        source_time_ns: value.source_time_ns,
        angle_rad: value.angle_rad,
        contact_radius_m: value.contact_radius_m,
        normalized_pressure: pressure_present.then_some(value.normalized_pressure),
    })
}

fn scratch_error_status(error: ScratchGestureError) -> RecordPlayerStatus {
    match error {
        ScratchGestureError::GestureAlreadyActive
        | ScratchGestureError::GestureNotActive
        | ScratchGestureError::PointerMismatch { .. } => RECORD_PLAYER_STATUS_GESTURE_STATE,
        _ => RECORD_PLAYER_STATUS_GESTURE_INVALID,
    }
}

fn scratch_base_control(
    value: RecordPlayerControl,
    deck_config: record_player::PhysicalDeckConfig,
) -> Result<PlayerControl, RecordPlayerStatus> {
    let control = timed_control(RecordPlayerTimedControl {
        absolute_frame: 0,
        sequence: 0,
        control: value,
    })?
    .control;
    control
        .deck
        .validate_for_config(deck_config)
        .map_err(|_| RECORD_PLAYER_STATUS_CONTROL_INVALID)?;
    Ok(control)
}

fn scratch_result(
    output: ScheduledScratchHandControl,
    sequence: u64,
    base_control: PlayerControl,
) -> RecordPlayerScratchControlResult {
    RecordPlayerScratchControlResult {
        event: record_timed_control(output.merge(sequence, base_control)),
        raw_pointer_angular_velocity_rad_s: output.raw_pointer_angular_velocity_rad_s,
        added_late_shift_frames: output.added_late_shift_frames,
        total_late_shift_frames: output.total_late_shift_frames,
        velocity_was_limited: u8::from(output.velocity_was_limited),
        wrap_was_ambiguous: u8::from(output.wrap_was_ambiguous),
        reserved: [0; 6],
    }
}

fn contact_mode(value: ContactMode) -> u32 {
    match value {
        ContactMode::Separated => RECORD_PLAYER_CONTACT_SEPARATED,
        ContactMode::Sticking => RECORD_PLAYER_CONTACT_STICKING,
        ContactMode::SlidingPositive => RECORD_PLAYER_CONTACT_SLIDING_POSITIVE,
        ContactMode::SlidingNegative => RECORD_PLAYER_CONTACT_SLIDING_NEGATIVE,
    }
}

fn pickup_contact_surface(value: PickupContactSurface) -> u32 {
    match value {
        PickupContactSurface::None => RECORD_PLAYER_PICKUP_CONTACT_NONE,
        PickupContactSurface::GrooveWalls => RECORD_PLAYER_PICKUP_CONTACT_GROOVE_WALLS,
        PickupContactSurface::RecordLand => RECORD_PLAYER_PICKUP_CONTACT_RECORD_LAND,
    }
}

fn radial_contact_region(value: RadialContactRegion) -> u32 {
    match value {
        RadialContactRegion::Groove => RECORD_PLAYER_RADIAL_REGION_GROOVE,
        RadialContactRegion::Land => RECORD_PLAYER_RADIAL_REGION_LAND,
        RadialContactRegion::Lifted => RECORD_PLAYER_RADIAL_REGION_LIFTED,
    }
}

fn scratch_crossfader_owner(value: record_player::ScratchCrossfaderOwner) -> u32 {
    match value {
        record_player::ScratchCrossfaderOwner::Manual => RECORD_PLAYER_SCRATCH_CROSSFADER_MANUAL,
        record_player::ScratchCrossfaderOwner::AutomaticPreset => {
            RECORD_PLAYER_SCRATCH_CROSSFADER_AUTOMATIC_PRESET
        }
    }
}

fn player_error_status(error: &PhysicalRecordPlayerError) -> RecordPlayerStatus {
    match error {
        PhysicalRecordPlayerError::TimelinePush(source) => match source {
            ControlTimelinePushError::InvalidControl(_) => RECORD_PLAYER_STATUS_CONTROL_INVALID,
            ControlTimelinePushError::Late { .. } => RECORD_PLAYER_STATUS_CONTROL_LATE,
            ControlTimelinePushError::Duplicate { .. } => RECORD_PLAYER_STATUS_CONTROL_DUPLICATE,
            ControlTimelinePushError::NonMonotonicFrame { .. } => {
                RECORD_PLAYER_STATUS_CONTROL_NONMONOTONIC_FRAME
            }
            ControlTimelinePushError::NonMonotonicSequence { .. } => {
                RECORD_PLAYER_STATUS_CONTROL_NONMONOTONIC_SEQUENCE
            }
            ControlTimelinePushError::Full { .. } => RECORD_PLAYER_STATUS_CONTROL_QUEUE_FULL,
        },
        PhysicalRecordPlayerError::GrooveLayoutMismatch => {
            RECORD_PLAYER_STATUS_GROOVE_LAYOUT_MISMATCH
        }
        PhysicalRecordPlayerError::GrooveProgrammeDoesNotFit => {
            RECORD_PLAYER_STATUS_GROOVE_PROGRAMME_DOES_NOT_FIT
        }
        PhysicalRecordPlayerError::GrooveOvercut => RECORD_PLAYER_STATUS_GROOVE_OVERCUT,
        _ => RECORD_PLAYER_STATUS_CORE_ERROR,
    }
}

fn renderer_error_status(error: &PhysicalHostRendererError) -> RecordPlayerStatus {
    match error {
        PhysicalHostRendererError::HostBlockTooLarge { .. } => {
            RECORD_PLAYER_STATUS_RENDER_BLOCK_TOO_LARGE
        }
        PhysicalHostRendererError::Resampler(
            StereoOutputResamplerError::UnsupportedOutputRate { .. },
        ) => RECORD_PLAYER_STATUS_UNSUPPORTED_OUTPUT_SAMPLE_RATE,
        PhysicalHostRendererError::HostOutputConfig(_) => RECORD_PLAYER_STATUS_INVALID_ARGUMENT,
        PhysicalHostRendererError::Player(source)
        | PhysicalHostRendererError::PlayerRender { source, .. } => player_error_status(source),
        _ => RECORD_PLAYER_STATUS_RENDER_FAILED,
    }
}

fn groove_error_status(error: &GrooveError) -> RecordPlayerStatus {
    match error {
        GrooveError::InvalidSourceSampleRate
        | GrooveError::InvalidSourceChannelCount
        | GrooveError::InsufficientFrames
        | GrooveError::NonfiniteProgramme => RECORD_PLAYER_STATUS_INVALID_ARGUMENT,
        _ => RECORD_PLAYER_STATUS_GROOVE_CUT_FAILED,
    }
}

fn control_ingress_report(value: PlayerControlIngressReport) -> RecordPlayerControlIngressReport {
    RecordPlayerControlIngressReport {
        inspected_events: value.inspected_events as u64,
        enqueued_events: value.enqueued_events as u64,
        retimed_late_events: value.retimed_late_events as u64,
        rejected_invalid_encodings: value.rejected_invalid_encodings as u64,
        rejected_invalid_controls: value.rejected_invalid_controls as u64,
        rejected_duplicates: value.rejected_duplicates as u64,
        rejected_nonmonotonic_frames: value.rejected_nonmonotonic_frames as u64,
        rejected_nonmonotonic_sequences: value.rejected_nonmonotonic_sequences as u64,
        protocol_errors: value.protocol_errors as u64,
        stopped_for_timeline_backpressure: u8::from(value.stopped_for_timeline_backpressure),
        producer_disconnected: u8::from(value.producer_disconnected),
        reserved: [0; 6],
    }
}

fn render_report(
    value: PhysicalHostRenderReport,
    ingress: PlayerControlIngressReport,
) -> RecordPlayerRenderReport {
    RecordPlayerRenderReport {
        output_sample_rate_hz: value.output_sample_rate_hz,
        reserved: 0,
        rendered_host_frames: value.rendered_host_frames as u64,
        rendered_internal_frames: value.rendered_internal_frames as u64,
        absolute_internal_frame: value.player.absolute_internal_frame,
        volts_per_full_scale: value.host_output.volts_per_full_scale,
        peak_unclipped_abs_output_v: value.host_output.peak_unclipped_abs_output_v,
        clipped_samples: value.host_output.clipped_samples,
        total_clipped_samples: value.host_output.total_clipped_samples,
        control_ingress: control_ingress_report(ingress),
    }
}

fn telemetry(
    output_sample_rate_hz: u32,
    rendered_host_frames: u64,
    value: PhysicalRenderTelemetry,
) -> RecordPlayerTelemetry {
    let mechanics = value.mechanics;
    let pickup = value.pickup;
    let cartridge = value.cartridge;
    let scratch = value.scratch;
    let radial_tracking = value.radial_tracking;
    let (captured_groove_radius_m, captured_groove_radius_present) = radial_tracking
        .captured_groove_radius_m
        .map_or((0.0, 0), |radius| (radius, 1));
    RecordPlayerTelemetry {
        abi_version: RECORD_PLAYER_CAPI_ABI_VERSION,
        output_sample_rate_hz,
        rendered_host_frames,
        rendered_internal_frames: value.rendered_internal_frames as u64,
        absolute_internal_frame: value.absolute_internal_frame,
        spiral_frame_position: value.spiral_frame_position,
        groove_frame_position: value.groove_frame_position,
        groove_radius_m: value.groove_radius_m,
        spatial_filter_lower_step_frames: value.spatial_filter_lower_step_frames,
        spatial_filter_upper_step_frames: value.spatial_filter_upper_step_frames,
        spatial_filter_upper_blend: value.spatial_filter_upper_blend,
        phono_output_v: value.phono_output_v,
        groove_loaded: u8::from(value.groove_loaded),
        at_programme_boundary: u8::from(value.at_programme_boundary),
        phono_input_overload: value.phono_input_overload.map(u8::from),
        phono_output_overload: value.phono_output_overload.map(u8::from),
        swept_contact_substeps: value.swept_contact_substeps,
        reserved: [0; 1],
        radial_tracking: RecordPlayerRadialTrackingTelemetry {
            spiral_reference_radius_m: radial_tracking.spiral_reference_radius_m,
            stylus_radius_m: radial_tracking.stylus_radius_m,
            radial_velocity_m_s: radial_tracking.radial_velocity_m_s,
            groove_pitch_m_per_revolution: radial_tracking.groove_pitch_m_per_revolution,
            captured_groove_radius_m,
            radial_error_to_retained_turn_m: radial_tracking.radial_error_to_retained_turn_m,
            groove_radial_velocity_m_s: radial_tracking.groove_radial_velocity_m_s,
            guide_force_n: radial_tracking.guide_force_n,
            bearing_friction_force_n: radial_tracking.bearing_friction_force_n,
            applied_radial_force_n: radial_tracking.applied_radial_force_n,
            captured_groove_radius_present,
            groove_contact: u8::from(radial_tracking.groove_contact),
            land_contact: u8::from(radial_tracking.land_contact),
            contact_lost_this_step: u8::from(radial_tracking.contact_lost_this_step),
            recaptured_this_step: u8::from(radial_tracking.recaptured_this_step),
            recapture_limit_reached: u8::from(radial_tracking.recapture_limit_reached),
            stylus_lowered: u8::from(radial_tracking.stylus_lowered),
            macro_contact_available: u8::from(radial_tracking.macro_contact_available),
            contact_region: radial_contact_region(radial_tracking.contact_region),
            reserved: 0,
            captured_turn_index: radial_tracking.captured_turn_index,
            turns_skipped_this_step: radial_tracking.turns_skipped_this_step,
            total_turns_skipped: radial_tracking.total_turns_skipped,
            completed_steps: radial_tracking.completed_steps,
        },
        deck: RecordPlayerDeckTelemetry {
            mechanical_time_seconds: mechanics.mechanical_time_seconds,
            platter_rate: mechanics.platter_rate,
            record_rate: mechanics.record_rate,
            platter_angle_turns: mechanics.platter_angle_turns,
            record_angle_turns: mechanics.record_angle_turns,
            motor_torque_nm: mechanics.motor_torque_nm,
            slipmat_torque_nm: mechanics.slipmat_torque_nm,
            hand_torque_nm: mechanics.hand_torque_nm,
            bearing_torque_nm: mechanics.bearing_torque_nm,
            stylus_torque_nm: mechanics.stylus_torque_nm,
            slipmat_mode: contact_mode(mechanics.slipmat_mode),
            hand_mode: contact_mode(mechanics.hand_mode),
            bearing_sticking: u8::from(mechanics.bearing_sticking),
            reserved: [0; 7],
        },
        pickup: RecordPlayerPickupTelemetry {
            tip_displacement_m: pickup.tip_displacement_m,
            tip_velocity_m_s: pickup.tip_velocity_m_s,
            body_displacement_m: pickup.body_displacement_m,
            body_velocity_m_s: pickup.body_velocity_m_s,
            relative_displacement_m: pickup.relative_displacement_m,
            relative_velocity_m_s: pickup.relative_velocity_m_s,
            suspension_force_on_tip_n: pickup.suspension_force_on_tip_n,
            electromagnetic_force_on_tip_n: pickup.electromagnetic_force_on_tip_n,
            wall_gap_m: pickup.wall_gap_m,
            wall_normal_force_n: pickup.wall_normal_force_n,
            wall_contact: pickup.wall_contact.map(u8::from),
            land_contact: u8::from(pickup.land_contact),
            stylus_lowered: u8::from(pickup.stylus_lowered),
            contact_surface: pickup_contact_surface(pickup.contact_surface),
            land_gap_m: pickup.land_gap_m,
            land_normal_force_n: pickup.land_normal_force_n,
            coulomb_friction_force_n: pickup.coulomb_friction_force_n,
            modulation_reaction_force_n: pickup.modulation_reaction_force_n,
            record_reaction_force_tangent_n: pickup.record_reaction_force_tangent_n,
            groove_radius_m: pickup.groove_radius_m,
            skating_force_n: pickup.skating_force_n,
            bearing_friction_force_n: pickup.bearing_friction_force_n,
            groove_lateral_force_on_tip_n: pickup.groove_lateral_force_on_tip_n,
            kinetic_energy_j: pickup.kinetic_energy_j,
            suspension_energy_j: pickup.suspension_energy_j,
            completed_steps: pickup.completed_steps,
        },
        cartridge: RecordPlayerCartridgeTelemetry {
            magnet_velocity_m_s: cartridge.magnet_velocity_m_s,
            generator_voltage_v: cartridge.generator_voltage_v,
            coil_current_a: cartridge.coil_current_a,
            load_output_voltage_v: cartridge.load_output_voltage_v,
            electromagnetic_reaction_force_n: cartridge.electromagnetic_reaction_force_n,
            generator_electrical_power_w: cartridge.generator_electrical_power_w,
            coil_loss_power_w: cartridge.coil_loss_power_w,
            load_power_w: cartridge.load_power_w,
            stored_electrical_energy_j: cartridge.stored_electrical_energy_j,
            completed_steps: cartridge.completed_steps,
        },
        scratch: RecordPlayerScratchTelemetry {
            preset: u32::from(scratch.preset.id()),
            crossfader_owner: scratch_crossfader_owner(scratch.owner),
            clicks: scratch.clicks,
            direction: scratch.direction,
            moving: u8::from(scratch.moving),
            reserved: [0; 5],
            audible_gain: scratch.audible_gain,
            automatic_gate_gain: scratch.automatic_gate_gain,
            automatic_gate_target: scratch.automatic_gate_target,
            phase: scratch.phase,
            stroke_progress: scratch.stroke_progress,
            span_prediction_confidence: scratch.span_prediction_confidence,
        },
    }
}

#[no_mangle]
pub extern "C" fn record_player_capi_abi_version() -> u32 {
    RECORD_PLAYER_CAPI_ABI_VERSION
}

#[no_mangle]
pub extern "C" fn record_player_status_message(status: RecordPlayerStatus) -> *const c_char {
    let message: &'static [u8] = match status {
        RECORD_PLAYER_STATUS_OK => b"success\0",
        RECORD_PLAYER_STATUS_NULL_POINTER => b"a required pointer is null\0",
        RECORD_PLAYER_STATUS_MISALIGNED_POINTER => b"a pointer has invalid alignment\0",
        RECORD_PLAYER_STATUS_INVALID_ARGUMENT => b"an argument is invalid\0",
        RECORD_PLAYER_STATUS_UNSUPPORTED_ABI_VERSION => b"the ABI version is unsupported\0",
        RECORD_PLAYER_STATUS_UNSUPPORTED_OUTPUT_SAMPLE_RATE => {
            b"the output sample rate is unsupported\0"
        }
        RECORD_PLAYER_STATUS_ALLOCATION_FAILED => b"memory allocation failed\0",
        RECORD_PLAYER_STATUS_CREATE_FAILED => b"player construction failed\0",
        RECORD_PLAYER_STATUS_BUSY => b"the requested handle lane is busy\0",
        RECORD_PLAYER_STATUS_GROOVE_CUT_FAILED => b"groove construction failed\0",
        RECORD_PLAYER_STATUS_GROOVE_LAYOUT_MISMATCH => b"the groove layout does not match\0",
        RECORD_PLAYER_STATUS_GROOVE_PROGRAMME_DOES_NOT_FIT => b"the programme does not fit\0",
        RECORD_PLAYER_STATUS_GROOVE_OVERCUT => b"the groove clearance check failed\0",
        RECORD_PLAYER_STATUS_GROOVE_LOAD_FAILED => b"groove loading failed\0",
        RECORD_PLAYER_STATUS_CONTROL_INVALID => b"the control is invalid\0",
        RECORD_PLAYER_STATUS_CONTROL_LATE => b"the control frame is late\0",
        RECORD_PLAYER_STATUS_CONTROL_DUPLICATE => b"the control key is duplicate\0",
        RECORD_PLAYER_STATUS_CONTROL_NONMONOTONIC_FRAME => b"control frames are not monotonic\0",
        RECORD_PLAYER_STATUS_CONTROL_NONMONOTONIC_SEQUENCE => {
            b"control sequences are not monotonic\0"
        }
        RECORD_PLAYER_STATUS_CONTROL_QUEUE_FULL => b"the control queue is full\0",
        RECORD_PLAYER_STATUS_RENDER_BLOCK_TOO_LARGE => b"the render block is too large\0",
        RECORD_PLAYER_STATUS_RENDER_FAILED => b"rendering failed\0",
        RECORD_PLAYER_STATUS_CORE_ERROR => b"the physical engine rejected the operation\0",
        RECORD_PLAYER_STATUS_GESTURE_INVALID => b"the scratch gesture input is invalid\0",
        RECORD_PLAYER_STATUS_GESTURE_STATE => b"the scratch gesture state is invalid\0",
        RECORD_PLAYER_STATUS_PANIC => b"Rust stopped an unwind at the ABI boundary\0",
        _ => b"unknown record-player status\0",
    };
    message.as_ptr().cast()
}

/// Creates one canonical pointer-to-hand gesture mapper.
///
/// # Safety
///
/// `options` and `out_handle` must point to valid values for this call.
#[no_mangle]
pub unsafe extern "C" fn record_player_scratch_gesture_create(
    options: *const RecordPlayerScratchGestureOptions,
    out_handle: *mut *mut RecordPlayerScratchGestureHandle,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let options = unsafe { *input_ref(options)? };
        let out_handle = unsafe { output_pointer(out_handle)? };
        // SAFETY: The output pointer is writable under the C contract.
        unsafe { out_handle.write(ptr::null_mut()) };
        if options.abi_version != RECORD_PLAYER_CAPI_ABI_VERSION {
            return Err(RECORD_PLAYER_STATUS_UNSUPPORTED_ABI_VERSION);
        }
        if options.reserved != [0; 2] {
            return Err(RECORD_PLAYER_STATUS_INVALID_ARGUMENT);
        }
        let config = ScratchGestureConfig {
            internal_sample_rate_hz: PHYSICAL_OUTPUT_INPUT_RATE_HZ,
            lookahead_frames: options.lookahead_frames,
            deck: record_player::PhysicalDeckConfig::default(),
            pressure: ScratchPressureCalibration {
                zero_pressure_force_n: options.zero_pressure_force_n,
                unit_pressure_force_n: options.unit_pressure_force_n,
                unreported_pressure_force_n: options.unreported_pressure_force_n,
            },
        };
        let mapper = ScratchGestureMapper::new(config).map_err(scratch_error_status)?;
        let handle = Box::into_raw(Box::new(RecordPlayerScratchGestureHandle {
            mapper: UnsafeCell::new(mapper),
            access_state: AtomicU8::new(0),
        }));
        // SAFETY: The output pointer is writable under the C contract.
        unsafe { out_handle.write(handle) };
        Ok(())
    })
}

/// Destroys one gesture mapper.
///
/// # Safety
///
/// `handle` must be a live handle from `record_player_scratch_gesture_create`.
#[no_mangle]
pub unsafe extern "C" fn record_player_scratch_gesture_destroy(
    handle: *mut RecordPlayerScratchGestureHandle,
) -> RecordPlayerStatus {
    run_ffi(|| {
        {
            let handle_ref = unsafe { scratch_handle_ref(handle)? };
            let access = acquire_scratch_access(handle_ref)?;
            access.keep_locked();
        }
        // SAFETY: The C contract transfers the live handle to this function.
        unsafe { drop(Box::from_raw(handle)) };
        Ok(())
    })
}

/// Starts one gesture and returns a complete timed player control.
///
/// # Safety
///
/// The pointers must meet the public header contract.
#[no_mangle]
pub unsafe extern "C" fn record_player_scratch_gesture_begin(
    handle: *mut RecordPlayerScratchGestureHandle,
    sample: *const RecordPlayerScratchPointerSample,
    minimum_render_frame: u64,
    record_angle_rad: f64,
    sequence: u64,
    base_control: *const RecordPlayerControl,
    out_result: *mut RecordPlayerScratchControlResult,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let handle = unsafe { scratch_handle_ref(handle)? };
        let _access = acquire_scratch_access(handle)?;
        let sample = scratch_pointer_sample(unsafe { *input_ref(sample)? })?;
        let base_control_value = unsafe { *input_ref(base_control)? };
        let out_result = unsafe { output_pointer(out_result)? };
        // SAFETY: The access guard gives this call the only mapper reference.
        let mapper = unsafe { &mut *scratch_mapper_pointer(handle) };
        let base_control = scratch_base_control(base_control_value, mapper.config().deck)?;
        let output = mapper
            .begin(sample, minimum_render_frame, record_angle_rad)
            .map_err(scratch_error_status)?;
        let result = scratch_result(output, sequence, base_control);
        // SAFETY: The output pointer is writable under the C contract.
        unsafe { out_result.write(result) };
        Ok(())
    })
}

/// Maps one gesture move and returns a complete timed player control.
///
/// # Safety
///
/// The pointers must meet the public header contract.
#[no_mangle]
pub unsafe extern "C" fn record_player_scratch_gesture_update(
    handle: *mut RecordPlayerScratchGestureHandle,
    sample: *const RecordPlayerScratchPointerSample,
    minimum_render_frame: u64,
    sequence: u64,
    base_control: *const RecordPlayerControl,
    out_result: *mut RecordPlayerScratchControlResult,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let handle = unsafe { scratch_handle_ref(handle)? };
        let _access = acquire_scratch_access(handle)?;
        let sample = scratch_pointer_sample(unsafe { *input_ref(sample)? })?;
        let base_control_value = unsafe { *input_ref(base_control)? };
        let out_result = unsafe { output_pointer(out_result)? };
        // SAFETY: The access guard gives this call the only mapper reference.
        let mapper = unsafe { &mut *scratch_mapper_pointer(handle) };
        let base_control = scratch_base_control(base_control_value, mapper.config().deck)?;
        let output = mapper
            .update(sample, minimum_render_frame)
            .map_err(scratch_error_status)?;
        let result = scratch_result(output, sequence, base_control);
        // SAFETY: The output pointer is writable under the C contract.
        unsafe { out_result.write(result) };
        Ok(())
    })
}

/// Ends one gesture and returns a complete release control.
///
/// # Safety
///
/// The pointers must meet the public header contract.
#[no_mangle]
pub unsafe extern "C" fn record_player_scratch_gesture_finish(
    handle: *mut RecordPlayerScratchGestureHandle,
    pointer_id: u64,
    source_time_ns: u64,
    minimum_render_frame: u64,
    sequence: u64,
    base_control: *const RecordPlayerControl,
    out_result: *mut RecordPlayerScratchControlResult,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let handle = unsafe { scratch_handle_ref(handle)? };
        let _access = acquire_scratch_access(handle)?;
        let base_control_value = unsafe { *input_ref(base_control)? };
        let out_result = unsafe { output_pointer(out_result)? };
        // SAFETY: The access guard gives this call the only mapper reference.
        let mapper = unsafe { &mut *scratch_mapper_pointer(handle) };
        let base_control = scratch_base_control(base_control_value, mapper.config().deck)?;
        let output = mapper
            .finish(pointer_id, source_time_ns, minimum_render_frame)
            .map_err(scratch_error_status)?;
        let result = scratch_result(output, sequence, base_control);
        // SAFETY: The output pointer is writable under the C contract.
        unsafe { out_result.write(result) };
        Ok(())
    })
}

/// Creates a renderer with the core engine's default physical profile.
///
/// # Safety
///
/// `options` and `out_handle` must point to valid values for this call.
#[no_mangle]
pub unsafe extern "C" fn record_player_create(
    options: *const RecordPlayerCreateOptions,
    out_handle: *mut *mut RecordPlayerHandle,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let options = unsafe { *input_ref(options)? };
        let out_handle = unsafe { output_pointer(out_handle)? };
        // SAFETY: The output pointer is writable under the C contract.
        unsafe { out_handle.write(ptr::null_mut()) };
        if options.abi_version != RECORD_PLAYER_CAPI_ABI_VERSION {
            return Err(RECORD_PLAYER_STATUS_UNSUPPORTED_ABI_VERSION);
        }
        if options.reserved != 0 {
            return Err(RECORD_PLAYER_STATUS_INVALID_ARGUMENT);
        }
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let mailbox_capacity = if options.control_mailbox_capacity == 0 {
            u32::try_from(profile.config.solver.control_timeline_capacity)
                .map_err(|_| RECORD_PLAYER_STATUS_CREATE_FAILED)?
        } else {
            options.control_mailbox_capacity
        };
        let (control_producer, control_consumer) =
            timed_player_control_mailbox(mailbox_capacity as usize)
                .map_err(|_| RECORD_PLAYER_STATUS_CREATE_FAILED)?;
        let deck_config = profile.config.deck;
        let host_output_config = PhysicalHostOutputConfig {
            volts_per_full_scale: options.volts_per_full_scale,
        };
        let renderer =
            PhysicalHostRenderer::new(profile, options.output_sample_rate_hz, host_output_config)
                .map_err(|error| match error {
                PhysicalHostRendererError::Resampler(
                    StereoOutputResamplerError::UnsupportedOutputRate { .. },
                ) => RECORD_PLAYER_STATUS_UNSUPPORTED_OUTPUT_SAMPLE_RATE,
                PhysicalHostRendererError::HostOutputConfig(_) => {
                    RECORD_PLAYER_STATUS_INVALID_ARGUMENT
                }
                _ => RECORD_PLAYER_STATUS_CREATE_FAILED,
            })?;
        let maximum_render_frames = renderer.maximum_host_render_frames() as u64;
        let latency_internal_frames = renderer.latency_internal_frames() as u64;
        let latency_seconds = renderer.latency_seconds();
        let handle = Box::into_raw(Box::new(RecordPlayerHandle {
            control_producer: UnsafeCell::new(control_producer),
            render_side: UnsafeCell::new(RenderSide {
                renderer,
                control_consumer,
            }),
            output_sample_rate_hz: options.output_sample_rate_hz,
            control_mailbox_capacity: mailbox_capacity,
            maximum_render_frames,
            latency_internal_frames,
            latency_seconds,
            volts_per_full_scale: options.volts_per_full_scale,
            deck_config,
            rendered_host_frames: AtomicU64::new(0),
            accepted_control_submissions: AtomicU64::new(0),
            rejected_full_control_submissions: AtomicU64::new(0),
            inspected_control_submissions: AtomicU64::new(0),
            access_state: AtomicU8::new(0),
        }));
        // SAFETY: The output pointer is writable under the C contract.
        unsafe { out_handle.write(handle) };
        Ok(())
    })
}

/// Destroys one handle.
///
/// # Safety
///
/// `handle` must be a live handle from `record_player_create`.
#[no_mangle]
pub unsafe extern "C" fn record_player_destroy(
    handle: *mut RecordPlayerHandle,
) -> RecordPlayerStatus {
    run_ffi(|| {
        {
            let handle_ref = unsafe { handle_ref(handle)? };
            let access = acquire_access(handle_ref, AccessLane::Lifecycle)?;
            access.keep_locked();
        }
        // SAFETY: The C contract transfers the live handle to this function.
        unsafe { drop(Box::from_raw(handle)) };
        Ok(())
    })
}

/// Gets fixed renderer properties and current frame counters.
///
/// # Safety
///
/// The pointers must meet the public header contract.
#[no_mangle]
pub unsafe extern "C" fn record_player_get_info(
    handle: *mut RecordPlayerHandle,
    out_info: *mut RecordPlayerInfo,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let handle = unsafe { handle_ref(handle)? };
        let _access = acquire_access(handle, AccessLane::Producer)?;
        let out_info = unsafe { output_pointer(out_info)? };
        let rendered_host_frames = handle.rendered_host_frames.load(Ordering::Acquire);
        let current_internal_frame = physical_input_frames_for_output_frames(
            handle.output_sample_rate_hz,
            rendered_host_frames,
        )
        .map_err(|_| RECORD_PLAYER_STATUS_CORE_ERROR)?;
        let info = RecordPlayerInfo {
            abi_version: RECORD_PLAYER_CAPI_ABI_VERSION,
            output_sample_rate_hz: handle.output_sample_rate_hz,
            internal_sample_rate_hz: PHYSICAL_OUTPUT_INPUT_RATE_HZ,
            control_mailbox_capacity: handle.control_mailbox_capacity,
            maximum_render_frames: handle.maximum_render_frames,
            latency_internal_frames: handle.latency_internal_frames,
            latency_seconds: handle.latency_seconds,
            volts_per_full_scale: handle.volts_per_full_scale,
            current_internal_frame,
            rendered_host_frames,
            accepted_control_submissions: handle
                .accepted_control_submissions
                .load(Ordering::Relaxed),
            rejected_full_control_submissions: handle
                .rejected_full_control_submissions
                .load(Ordering::Relaxed),
            inspected_control_submissions: handle
                .inspected_control_submissions
                .load(Ordering::Relaxed),
        };
        // SAFETY: The output pointer is writable under the C contract.
        unsafe { out_info.write(info) };
        Ok(())
    })
}

/// Converts a host-frame offset to an exact physical-frame boundary.
///
/// # Safety
///
/// The pointers must meet the public header contract.
#[no_mangle]
pub unsafe extern "C" fn record_player_schedule_point_after_host_frames(
    handle: *mut RecordPlayerHandle,
    host_frame_offset: u64,
    out_schedule_point: *mut RecordPlayerSchedulePoint,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let handle = unsafe { handle_ref(handle)? };
        let _access = acquire_access(handle, AccessLane::Producer)?;
        let out_schedule_point = unsafe { output_pointer(out_schedule_point)? };
        let observed_rendered_host_frames = handle.rendered_host_frames.load(Ordering::Acquire);
        let target_host_frames = observed_rendered_host_frames
            .checked_add(host_frame_offset)
            .ok_or(RECORD_PLAYER_STATUS_INVALID_ARGUMENT)?;
        let current_internal_frame = physical_input_frames_for_output_frames(
            handle.output_sample_rate_hz,
            observed_rendered_host_frames,
        )
        .map_err(|_| RECORD_PLAYER_STATUS_CORE_ERROR)?;
        let absolute_internal_frame = physical_input_frames_for_output_frames(
            handle.output_sample_rate_hz,
            target_host_frames,
        )
        .map_err(|_| RECORD_PLAYER_STATUS_INVALID_ARGUMENT)?;
        let point = RecordPlayerSchedulePoint {
            observed_rendered_host_frames,
            current_internal_frame,
            host_frame_offset,
            absolute_internal_frame,
        };
        // SAFETY: The output pointer is writable under the C contract.
        unsafe { out_schedule_point.write(point) };
        Ok(())
    })
}

/// Builds and loads a groove from interleaved PCM.
///
/// This function allocates. Do not call it from the audio thread.
///
/// # Safety
///
/// The pointers must meet the public header contract.
#[no_mangle]
pub unsafe extern "C" fn record_player_load_interleaved_pcm(
    handle: *mut RecordPlayerHandle,
    interleaved_pcm: *const f32,
    frame_count: usize,
    channel_count: u32,
    source_sample_rate_hz: f64,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let handle = unsafe { handle_ref(handle)? };
        let _access = acquire_access(handle, AccessLane::Lifecycle)?;
        if !(1..=2).contains(&channel_count) || frame_count < 4 {
            return Err(RECORD_PLAYER_STATUS_INVALID_ARGUMENT);
        }
        let sample_count = frame_count
            .checked_mul(channel_count as usize)
            .ok_or(RECORD_PLAYER_STATUS_INVALID_ARGUMENT)?;
        let samples = unsafe { input_slice(interleaved_pcm, sample_count)? };
        // SAFETY: Lifecycle access is exclusive under the C contract.
        let render_side = unsafe { &mut *render_side_pointer(handle) };
        let layout = render_side.renderer.profile().config.groove;
        let cut = render_side.renderer.profile().config.record_cut;

        let groove = if channel_count == 1 {
            GrooveAsset::cut_from_pcm(&[samples], source_sample_rate_hz, layout, cut)
        } else {
            let mut left = Vec::new();
            let mut right = Vec::new();
            left.try_reserve_exact(frame_count)
                .map_err(|_| RECORD_PLAYER_STATUS_ALLOCATION_FAILED)?;
            right
                .try_reserve_exact(frame_count)
                .map_err(|_| RECORD_PLAYER_STATUS_ALLOCATION_FAILED)?;
            for frame in samples.chunks_exact(2) {
                left.push(frame[0]);
                right.push(frame[1]);
            }
            GrooveAsset::cut_from_pcm(
                &[left.as_slice(), right.as_slice()],
                source_sample_rate_hz,
                layout,
                cut,
            )
        }
        .map_err(|error| groove_error_status(&error))?;

        render_side
            .renderer
            .load_groove(Arc::new(groove))
            .map_err(|error| {
                let status = player_error_status(&error);
                if status == RECORD_PLAYER_STATUS_CORE_ERROR {
                    RECORD_PLAYER_STATUS_GROOVE_LOAD_FAILED
                } else {
                    status
                }
            })?;
        Ok(())
    })
}

/// Unloads the current groove.
///
/// # Safety
///
/// `handle` must be a live, exclusively borrowed handle.
#[no_mangle]
pub unsafe extern "C" fn record_player_unload_groove(
    handle: *mut RecordPlayerHandle,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let handle = unsafe { handle_ref(handle)? };
        let _access = acquire_access(handle, AccessLane::Lifecycle)?;
        // SAFETY: Lifecycle access is exclusive under the C contract.
        unsafe { &mut *render_side_pointer(handle) }
            .renderer
            .unload_groove();
        Ok(())
    })
}

/// Sets the source groove frame position.
///
/// # Safety
///
/// `handle` must be a live, exclusively borrowed handle.
#[no_mangle]
pub unsafe extern "C" fn record_player_set_groove_frame_position(
    handle: *mut RecordPlayerHandle,
    position: f64,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let handle = unsafe { handle_ref(handle)? };
        let _access = acquire_access(handle, AccessLane::Lifecycle)?;
        // SAFETY: Lifecycle access is exclusive under the C contract.
        unsafe { &mut *render_side_pointer(handle) }
            .renderer
            .set_groove_frame_position(position)
            .map_err(|error| player_error_status(&error))?;
        Ok(())
    })
}

/// Resets the platter and record transport state.
///
/// # Safety
///
/// `handle` must be a live, exclusively borrowed handle.
#[no_mangle]
pub unsafe extern "C" fn record_player_reset_transport(
    handle: *mut RecordPlayerHandle,
    platter_rate: f64,
    record_rate: f64,
    platter_angle_turns: f64,
    record_angle_turns: f64,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let handle = unsafe { handle_ref(handle)? };
        let _access = acquire_access(handle, AccessLane::Lifecycle)?;
        // SAFETY: Lifecycle access is exclusive under the C contract.
        unsafe { &mut *render_side_pointer(handle) }
            .renderer
            .reset_transport(
                platter_rate,
                record_rate,
                platter_angle_turns,
                record_angle_turns,
            )
            .map_err(|error| player_error_status(&error))?;
        Ok(())
    })
}

/// Submits one complete control state at an exact internal frame.
///
/// This function does not allocate.
///
/// # Safety
///
/// The pointers must meet the public header contract.
#[no_mangle]
pub unsafe extern "C" fn record_player_submit_timed_control(
    handle: *mut RecordPlayerHandle,
    event: *const RecordPlayerTimedControl,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let handle = unsafe { handle_ref(handle)? };
        let _access = acquire_access(handle, AccessLane::Producer)?;
        let event = timed_control(unsafe { *input_ref(event)? })?;
        event
            .control
            .deck
            .validate_for_config(handle.deck_config)
            .map_err(|_| RECORD_PLAYER_STATUS_CONTROL_INVALID)?;
        // SAFETY: The C contract assigns this endpoint to one producer thread.
        match unsafe { &mut *producer_pointer(handle) }.try_push(event) {
            Ok(()) => {
                handle
                    .accepted_control_submissions
                    .fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(SpscPushError::Full(_)) => {
                handle
                    .rejected_full_control_submissions
                    .fetch_add(1, Ordering::Relaxed);
                Err(RECORD_PLAYER_STATUS_CONTROL_QUEUE_FULL)
            }
            Err(SpscPushError::Disconnected(_)) => Err(RECORD_PLAYER_STATUS_CORE_ERROR),
        }
    })
}

/// Renders one interleaved stereo host-rate block.
///
/// This function does not allocate on its successful path.
/// `out_report` can be null.
///
/// # Safety
///
/// The pointers must meet the public header contract.
#[no_mangle]
pub unsafe extern "C" fn record_player_render_interleaved(
    handle: *mut RecordPlayerHandle,
    output: *mut f32,
    frame_count: usize,
    out_report: *mut RecordPlayerRenderReport,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let handle = unsafe { handle_ref(handle)? };
        let _access = acquire_access(handle, AccessLane::Render)?;
        let sample_count = frame_count
            .checked_mul(2)
            .ok_or(RECORD_PLAYER_STATUS_INVALID_ARGUMENT)?;
        let out_report = if out_report.is_null() {
            None
        } else {
            Some(unsafe { output_pointer(out_report)? })
        };
        if frame_count as u64 > handle.maximum_render_frames {
            return Err(RECORD_PLAYER_STATUS_RENDER_BLOCK_TOO_LARGE);
        }
        if frame_count != 0 {
            pointer_status(output)?;
            slice_size_status::<f32>(sample_count)?;
        }
        // SAFETY: The C contract assigns this endpoint to one render thread.
        let render_side = unsafe { &mut *render_side_pointer(handle) };
        let ingress = render_side
            .renderer
            .drain_control_ingress(&mut render_side.control_consumer);
        handle
            .inspected_control_submissions
            .fetch_add(ingress.inspected_events as u64, Ordering::Relaxed);
        let report = if frame_count == 0 {
            render_side
                .renderer
                .render_interleaved(&mut [])
                .map_err(|error| renderer_error_status(&error))?
        } else {
            let output = unsafe { output_slice(output, sample_count)? };
            render_side
                .renderer
                .render_interleaved(output)
                .map_err(|error| renderer_error_status(&error))?
        };
        handle.rendered_host_frames.store(
            render_side.renderer.rendered_host_frames(),
            Ordering::Release,
        );
        if let Some(out_report) = out_report {
            // SAFETY: The optional output pointer is writable under the C contract.
            unsafe { out_report.write(render_report(report, ingress)) };
        }
        Ok(())
    })
}

/// Gets the latest physical telemetry.
///
/// # Safety
///
/// The pointers must meet the public header contract.
#[no_mangle]
pub unsafe extern "C" fn record_player_get_telemetry(
    handle: *mut RecordPlayerHandle,
    out_telemetry: *mut RecordPlayerTelemetry,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let handle = unsafe { handle_ref(handle)? };
        let _access = acquire_access(handle, AccessLane::Render)?;
        let out_telemetry = unsafe { output_pointer(out_telemetry)? };
        // SAFETY: The C contract assigns this endpoint to one render thread.
        let render_side = unsafe { &mut *render_side_pointer(handle) };
        let value = telemetry(
            handle.output_sample_rate_hz,
            handle.rendered_host_frames.load(Ordering::Acquire),
            render_side.renderer.telemetry(),
        );
        // SAFETY: The output pointer is writable under the C contract.
        unsafe { out_telemetry.write(value) };
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, MaybeUninit};
    use std::sync::{Arc as TestArc, Barrier};

    #[derive(Clone, Copy)]
    struct SendHandle(*mut RecordPlayerHandle);

    // SAFETY: Each test keeps the allocation live and uses the public lane
    // guards before a thread can touch one endpoint.
    unsafe impl Send for SendHandle {}

    impl SendHandle {
        const fn pointer(self) -> *mut RecordPlayerHandle {
            self.0
        }
    }

    fn hold_lane(
        handle: *mut RecordPlayerHandle,
        lane: AccessLane,
    ) -> (
        TestArc<Barrier>,
        TestArc<Barrier>,
        std::thread::JoinHandle<()>,
    ) {
        let entered = TestArc::new(Barrier::new(2));
        let release = TestArc::new(Barrier::new(2));
        let thread_entered = TestArc::clone(&entered);
        let thread_release = TestArc::clone(&release);
        let send_handle = SendHandle(handle);
        let thread = std::thread::spawn(move || {
            let handle = unsafe { handle_ref(send_handle.pointer()) }.unwrap();
            let _access = acquire_access(handle, lane).unwrap();
            thread_entered.wait();
            thread_release.wait();
        });
        (entered, release, thread)
    }

    fn create_with_capacity(control_mailbox_capacity: u32) -> *mut RecordPlayerHandle {
        let options = RecordPlayerCreateOptions {
            abi_version: RECORD_PLAYER_CAPI_ABI_VERSION,
            output_sample_rate_hz: 48_000,
            control_mailbox_capacity,
            reserved: 0,
            volts_per_full_scale: 10.0,
        };
        let mut handle = ptr::null_mut();
        let status = unsafe { record_player_create(&options, &mut handle) };
        assert_eq!(status, RECORD_PLAYER_STATUS_OK);
        assert!(!handle.is_null());
        handle
    }

    fn create() -> *mut RecordPlayerHandle {
        create_with_capacity(0)
    }

    fn stopped_control(frame: u64, sequence: u64) -> RecordPlayerTimedControl {
        RecordPlayerTimedControl {
            absolute_frame: frame,
            sequence,
            control: RecordPlayerControl {
                motor_mode: RECORD_PLAYER_MOTOR_OFF,
                hand_contact: 0,
                hand_target_angle_present: 0,
                stylus_lowered: 1,
                reserved: 0,
                motor_target_angular_velocity_rad_s: 0.0,
                hand_target_angle_rad: 0.0,
                hand_target_angular_velocity_rad_s: 0.0,
                hand_normal_force_n: 0.0,
                hand_contact_radius_m: 0.12,
                manual_crossfader_gain: 1.0,
                scratch_preset: RECORD_PLAYER_SCRATCH_PRESET_BABY,
                scratch_clicks: 0,
                scratch_reserved: [0; 3],
            },
        }
    }

    fn scratch_options(lookahead_frames: u32) -> RecordPlayerScratchGestureOptions {
        let pressure = ScratchPressureCalibration::default();
        RecordPlayerScratchGestureOptions {
            abi_version: RECORD_PLAYER_CAPI_ABI_VERSION,
            lookahead_frames,
            reserved: [0; 2],
            zero_pressure_force_n: pressure.zero_pressure_force_n,
            unit_pressure_force_n: pressure.unit_pressure_force_n,
            unreported_pressure_force_n: pressure.unreported_pressure_force_n,
        }
    }

    fn create_scratch(lookahead_frames: u32) -> *mut RecordPlayerScratchGestureHandle {
        let mut handle = ptr::null_mut();
        assert_eq!(
            unsafe {
                record_player_scratch_gesture_create(
                    &scratch_options(lookahead_frames),
                    &mut handle,
                )
            },
            RECORD_PLAYER_STATUS_OK
        );
        assert!(!handle.is_null());
        handle
    }

    fn scratch_sample(
        pointer_id: u64,
        source_time_ns: u64,
        angle_rad: f64,
    ) -> RecordPlayerScratchPointerSample {
        RecordPlayerScratchPointerSample {
            pointer_id,
            source_time_ns,
            angle_rad,
            contact_radius_m: 0.12,
            normalized_pressure: 0.5,
            pressure_present: 1,
            reserved: [0; 7],
        }
    }

    #[test]
    fn abi_layout_is_pinned() {
        assert_eq!(size_of::<RecordPlayerCreateOptions>(), 24);
        assert_eq!(align_of::<RecordPlayerCreateOptions>(), 8);
        assert_eq!(
            offset_of!(RecordPlayerCreateOptions, volts_per_full_scale),
            16
        );
        assert_eq!(size_of::<RecordPlayerControl>(), 64);
        assert_eq!(align_of::<RecordPlayerControl>(), 8);
        assert_eq!(
            offset_of!(RecordPlayerControl, motor_target_angular_velocity_rad_s),
            8
        );
        assert_eq!(size_of::<RecordPlayerTimedControl>(), 80);
        assert_eq!(offset_of!(RecordPlayerTimedControl, control), 16);
        assert_eq!(size_of::<RecordPlayerScratchGestureOptions>(), 40);
        assert_eq!(size_of::<RecordPlayerScratchPointerSample>(), 48);
        assert_eq!(size_of::<RecordPlayerScratchControlResult>(), 112);
        assert_eq!(offset_of!(RecordPlayerScratchControlResult, event), 0);
        assert_eq!(
            offset_of!(
                RecordPlayerScratchControlResult,
                raw_pointer_angular_velocity_rad_s
            ),
            80
        );
        assert_eq!(size_of::<RecordPlayerInfo>(), 88);
        assert_eq!(offset_of!(RecordPlayerInfo, volts_per_full_scale), 40);
        assert_eq!(size_of::<RecordPlayerSchedulePoint>(), 32);
        assert_eq!(size_of::<RecordPlayerControlIngressReport>(), 80);
        assert_eq!(size_of::<RecordPlayerRenderReport>(), 168);
        assert_eq!(
            offset_of!(RecordPlayerRenderReport, volts_per_full_scale),
            32
        );
        assert_eq!(
            offset_of!(RecordPlayerRenderReport, peak_unclipped_abs_output_v),
            40
        );
        assert_eq!(offset_of!(RecordPlayerRenderReport, clipped_samples), 56);
        assert_eq!(
            offset_of!(RecordPlayerRenderReport, total_clipped_samples),
            72
        );
        assert_eq!(offset_of!(RecordPlayerRenderReport, control_ingress), 88);
        assert_eq!(size_of::<RecordPlayerDeckTelemetry>(), 96);
        assert_eq!(size_of::<RecordPlayerPickupTelemetry>(), 264);
        assert_eq!(size_of::<RecordPlayerCartridgeTelemetry>(), 120);
        assert_eq!(size_of::<RecordPlayerRadialTrackingTelemetry>(), 128);
        assert_eq!(
            offset_of!(RecordPlayerRadialTrackingTelemetry, captured_turn_index),
            96
        );
        assert_eq!(size_of::<RecordPlayerScratchTelemetry>(), 64);
        assert_eq!(offset_of!(RecordPlayerScratchTelemetry, audible_gain), 16);
        assert_eq!(size_of::<RecordPlayerTelemetry>(), 768);
        assert_eq!(offset_of!(RecordPlayerTelemetry, spiral_frame_position), 32);
        assert_eq!(offset_of!(RecordPlayerTelemetry, groove_frame_position), 40);
        assert_eq!(
            offset_of!(RecordPlayerTelemetry, swept_contact_substeps),
            94
        );
        assert_eq!(offset_of!(RecordPlayerTelemetry, radial_tracking), 96);
        assert_eq!(offset_of!(RecordPlayerTelemetry, deck), 224);
        assert_eq!(offset_of!(RecordPlayerTelemetry, pickup), 320);
        assert_eq!(offset_of!(RecordPlayerTelemetry, cartridge), 584);
        assert_eq!(offset_of!(RecordPlayerTelemetry, scratch), 704);
    }

    #[test]
    fn header_declares_the_pinned_abi() {
        let header = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../include/record_player.h"
        ));
        assert!(header.contains("RECORD_PLAYER_CAPI_ABI_VERSION 5u"));
        assert!(header.contains("double volts_per_full_scale;"));
        assert!(header.contains("RECORD_PLAYER_STATUS_BUSY 8"));
        assert!(header.contains("RECORD_PLAYER_PICKUP_CONTACT_RECORD_LAND 2u"));
        assert!(header.contains("RECORD_PLAYER_RADIAL_REGION_LIFTED 2u"));
        assert!(header.contains("typedef struct RecordPlayerHandle RecordPlayerHandle;"));
        assert!(header.contains("record_player_submit_timed_control"));
        assert!(header.contains("record_player_schedule_point_after_host_frames"));
        assert!(header.contains("record_player_render_interleaved"));
        assert!(header.contains("record_player_scratch_gesture_begin"));
        assert!(header.contains("sizeof(RecordPlayerScratchControlResult) == 112"));
        assert!(header.contains("sizeof(RecordPlayerScratchTelemetry) == 64"));
        assert!(header.contains("sizeof(RecordPlayerTelemetry) == 768"));
        assert!(header.contains("uint8_t swept_contact_substeps;"));
    }

    #[test]
    fn physical_contact_enums_have_complete_stable_c_mappings() {
        assert_eq!(pickup_contact_surface(PickupContactSurface::None), 0);
        assert_eq!(pickup_contact_surface(PickupContactSurface::GrooveWalls), 1);
        assert_eq!(pickup_contact_surface(PickupContactSurface::RecordLand), 2);
        assert_eq!(radial_contact_region(RadialContactRegion::Groove), 0);
        assert_eq!(radial_contact_region(RadialContactRegion::Land), 1);
        assert_eq!(radial_contact_region(RadialContactRegion::Lifted), 2);
    }

    #[test]
    fn scratch_mapper_preserves_rapid_reversals_and_complete_control_state() {
        let handle = create_scratch(1_920);
        let mut base = stopped_control(0, 0).control;
        base.motor_mode = RECORD_PLAYER_MOTOR_BRAKE;
        base.motor_target_angular_velocity_rad_s = -2.0;
        base.manual_crossfader_gain = 0.37;
        base.scratch_preset = RECORD_PLAYER_SCRATCH_PRESET_CRAB;
        base.scratch_clicks = 4;
        let mut result = MaybeUninit::<RecordPlayerScratchControlResult>::uninit();
        assert_eq!(
            unsafe {
                record_player_scratch_gesture_begin(
                    handle,
                    &scratch_sample(7, 0, 0.0),
                    10_000,
                    -4.0,
                    1,
                    &base,
                    result.as_mut_ptr(),
                )
            },
            RECORD_PLAYER_STATUS_OK
        );
        let begin = unsafe { result.assume_init() };
        assert_eq!(begin.event.absolute_frame, 11_920);
        assert_eq!(begin.event.control.motor_mode, RECORD_PLAYER_MOTOR_BRAKE);
        assert_eq!(
            begin.event.control.motor_target_angular_velocity_rad_s,
            -2.0
        );
        assert_eq!(begin.event.control.hand_contact, 1);
        assert_eq!(begin.event.control.hand_target_angle_rad, -4.0);
        assert_eq!(begin.event.control.manual_crossfader_gain, 0.37);
        assert_eq!(
            begin.event.control.scratch_preset,
            RECORD_PLAYER_SCRATCH_PRESET_CRAB
        );
        assert_eq!(begin.event.control.scratch_clicks, 4);

        let mut previous_frame = begin.event.absolute_frame;
        for index in 1..=128_u64 {
            let angle = if index % 2 == 0 { 0.0 } else { 0.02 };
            let mut result = MaybeUninit::<RecordPlayerScratchControlResult>::uninit();
            assert_eq!(
                unsafe {
                    record_player_scratch_gesture_update(
                        handle,
                        &scratch_sample(7, index * 1_000_000, angle),
                        10_000,
                        index + 1,
                        &base,
                        result.as_mut_ptr(),
                    )
                },
                RECORD_PLAYER_STATUS_OK
            );
            let update = unsafe { result.assume_init() };
            assert!(update.event.absolute_frame > previous_frame);
            if index % 2 == 0 {
                assert!(update.event.control.hand_target_angular_velocity_rad_s < 0.0);
            } else {
                assert!(update.event.control.hand_target_angular_velocity_rad_s > 0.0);
            }
            previous_frame = update.event.absolute_frame;
        }

        let mut release = MaybeUninit::<RecordPlayerScratchControlResult>::uninit();
        assert_eq!(
            unsafe {
                record_player_scratch_gesture_finish(
                    handle,
                    7,
                    129_000_000,
                    10_000,
                    130,
                    &base,
                    release.as_mut_ptr(),
                )
            },
            RECORD_PLAYER_STATUS_OK
        );
        let release = unsafe { release.assume_init() };
        assert_eq!(release.event.control.hand_contact, 0);
        assert_eq!(release.event.control.hand_target_angle_present, 0);
        assert_eq!(
            unsafe { record_player_scratch_gesture_destroy(handle) },
            RECORD_PLAYER_STATUS_OK
        );
    }

    #[test]
    fn scratch_mapper_errors_leave_the_gesture_usable() {
        let handle = create_scratch(0);
        let base = stopped_control(0, 0).control;
        let mut invalid = scratch_sample(1, 0, 0.0);
        invalid.normalized_pressure = f64::NAN;
        let mut result = MaybeUninit::<RecordPlayerScratchControlResult>::uninit();
        assert_eq!(
            unsafe {
                record_player_scratch_gesture_begin(
                    handle,
                    &invalid,
                    0,
                    0.0,
                    1,
                    &base,
                    result.as_mut_ptr(),
                )
            },
            RECORD_PLAYER_STATUS_GESTURE_INVALID
        );
        assert_eq!(
            unsafe {
                record_player_scratch_gesture_begin(
                    handle,
                    &scratch_sample(1, 0, 0.0),
                    0,
                    0.0,
                    1,
                    &base,
                    result.as_mut_ptr(),
                )
            },
            RECORD_PLAYER_STATUS_OK
        );
        assert_eq!(
            unsafe {
                record_player_scratch_gesture_update(
                    handle,
                    &scratch_sample(2, 1_000_000, 0.1),
                    0,
                    2,
                    &base,
                    result.as_mut_ptr(),
                )
            },
            RECORD_PLAYER_STATUS_GESTURE_STATE
        );
        assert_eq!(
            unsafe {
                record_player_scratch_gesture_update(
                    handle,
                    &scratch_sample(1, 1_000_000, 0.1),
                    0,
                    2,
                    &base,
                    result.as_mut_ptr(),
                )
            },
            RECORD_PLAYER_STATUS_OK
        );
        assert_eq!(
            unsafe { record_player_scratch_gesture_destroy(handle) },
            RECORD_PLAYER_STATUS_OK
        );
    }

    #[test]
    fn scratch_mapper_rejects_concurrent_access_before_pointer_use() {
        let handle = create_scratch(0);
        let handle_ref = unsafe { scratch_handle_ref(handle) }.unwrap();
        let access = acquire_scratch_access(handle_ref).unwrap();
        assert_eq!(
            unsafe {
                record_player_scratch_gesture_update(
                    handle,
                    ptr::null(),
                    0,
                    0,
                    ptr::null(),
                    ptr::null_mut(),
                )
            },
            RECORD_PLAYER_STATUS_BUSY
        );
        assert_eq!(
            unsafe { record_player_scratch_gesture_destroy(handle) },
            RECORD_PLAYER_STATUS_BUSY
        );
        drop(access);
        assert_eq!(
            unsafe { record_player_scratch_gesture_destroy(handle) },
            RECORD_PLAYER_STATUS_OK
        );
    }

    #[test]
    fn scratch_mapper_validates_options_and_absent_pressure_encoding() {
        let mut options = scratch_options(0);
        options.unit_pressure_force_n = 0.1;
        options.zero_pressure_force_n = 0.2;
        let mut handle = ptr::null_mut();
        assert_eq!(
            unsafe { record_player_scratch_gesture_create(&options, &mut handle) },
            RECORD_PLAYER_STATUS_GESTURE_INVALID
        );
        assert!(handle.is_null());

        let handle = create_scratch(0);
        let base = stopped_control(0, 0).control;
        let mut sample = scratch_sample(1, 0, 0.0);
        sample.pressure_present = 0;
        let mut result = MaybeUninit::<RecordPlayerScratchControlResult>::uninit();
        assert_eq!(
            unsafe {
                record_player_scratch_gesture_begin(
                    handle,
                    &sample,
                    0,
                    0.0,
                    1,
                    &base,
                    result.as_mut_ptr(),
                )
            },
            RECORD_PLAYER_STATUS_INVALID_ARGUMENT
        );
        sample.normalized_pressure = 0.0;
        assert_eq!(
            unsafe {
                record_player_scratch_gesture_begin(
                    handle,
                    &sample,
                    0,
                    0.0,
                    1,
                    &base,
                    result.as_mut_ptr(),
                )
            },
            RECORD_PLAYER_STATUS_OK
        );
        assert_eq!(
            unsafe { record_player_scratch_gesture_destroy(handle) },
            RECORD_PLAYER_STATUS_OK
        );
    }

    #[test]
    fn explicit_argument_and_version_errors_are_stable() {
        let options = RecordPlayerCreateOptions {
            abi_version: RECORD_PLAYER_CAPI_ABI_VERSION + 1,
            output_sample_rate_hz: 48_000,
            control_mailbox_capacity: 8,
            reserved: 0,
            volts_per_full_scale: 10.0,
        };
        let mut handle = 1usize as *mut RecordPlayerHandle;
        assert_eq!(
            unsafe { record_player_create(&options, &mut handle) },
            RECORD_PLAYER_STATUS_UNSUPPORTED_ABI_VERSION
        );
        assert!(handle.is_null());
        assert_eq!(
            unsafe { record_player_create(ptr::null(), &mut handle) },
            RECORD_PLAYER_STATUS_NULL_POINTER
        );

        let options = RecordPlayerCreateOptions {
            abi_version: RECORD_PLAYER_CAPI_ABI_VERSION,
            output_sample_rate_hz: 12_345,
            control_mailbox_capacity: 8,
            reserved: 0,
            volts_per_full_scale: 10.0,
        };
        assert_eq!(
            unsafe { record_player_create(&options, &mut handle) },
            RECORD_PLAYER_STATUS_UNSUPPORTED_OUTPUT_SAMPLE_RATE
        );
        for volts_per_full_scale in [0.0, -1.0, f64::NAN, f64::INFINITY, 1_001.0] {
            let invalid_level = RecordPlayerCreateOptions {
                abi_version: RECORD_PLAYER_CAPI_ABI_VERSION,
                output_sample_rate_hz: 48_000,
                control_mailbox_capacity: 8,
                reserved: 0,
                volts_per_full_scale,
            };
            assert_eq!(
                unsafe { record_player_create(&invalid_level, &mut handle) },
                RECORD_PLAYER_STATUS_INVALID_ARGUMENT
            );
            assert!(handle.is_null());
        }
        assert_eq!(
            unsafe { record_player_destroy(ptr::null_mut()) },
            RECORD_PLAYER_STATUS_NULL_POINTER
        );
    }

    #[test]
    fn lifecycle_load_control_render_and_telemetry() {
        let handle = create();
        let mut pcm = vec![0.0f32; 64 * 2];
        for frame in 0..64 {
            let sample = ((frame as f64) * 0.17).sin() as f32 * 0.05;
            pcm[frame * 2] = sample;
            pcm[frame * 2 + 1] = -sample;
        }
        assert_eq!(
            unsafe { record_player_load_interleaved_pcm(handle, pcm.as_ptr(), 64, 2, 48_000.0) },
            RECORD_PLAYER_STATUS_OK
        );
        assert_eq!(
            unsafe { record_player_set_groove_frame_position(handle, 12.0) },
            RECORD_PLAYER_STATUS_OK
        );

        let event = stopped_control(0, 1);
        assert_eq!(
            unsafe { record_player_submit_timed_control(handle, &event) },
            RECORD_PLAYER_STATUS_OK
        );

        let mut info = MaybeUninit::<RecordPlayerInfo>::uninit();
        assert_eq!(
            unsafe { record_player_get_info(handle, info.as_mut_ptr()) },
            RECORD_PLAYER_STATUS_OK
        );
        let info = unsafe { info.assume_init() };
        assert_eq!(info.abi_version, RECORD_PLAYER_CAPI_ABI_VERSION);
        assert_eq!(info.output_sample_rate_hz, 48_000);
        assert_eq!(info.internal_sample_rate_hz, 192_000);
        assert_eq!(info.volts_per_full_scale, 10.0);
        assert!(info.maximum_render_frames >= 64);

        let mut point = MaybeUninit::<RecordPlayerSchedulePoint>::uninit();
        assert_eq!(
            unsafe {
                record_player_schedule_point_after_host_frames(handle, 64, point.as_mut_ptr())
            },
            RECORD_PLAYER_STATUS_OK
        );
        let point = unsafe { point.assume_init() };
        assert_eq!(point.observed_rendered_host_frames, 0);
        assert_eq!(point.current_internal_frame, 0);
        assert_eq!(point.absolute_internal_frame, 253);

        let mut output = vec![f32::NAN; 64 * 2];
        let mut report = MaybeUninit::<RecordPlayerRenderReport>::uninit();
        assert_eq!(
            unsafe {
                record_player_render_interleaved(
                    handle,
                    output.as_mut_ptr(),
                    64,
                    report.as_mut_ptr(),
                )
            },
            RECORD_PLAYER_STATUS_OK
        );
        assert!(output.iter().all(|sample| sample.is_finite()));
        let report = unsafe { report.assume_init() };
        assert_eq!(report.rendered_host_frames, 64);
        assert!(report.rendered_internal_frames > 0);
        assert_eq!(report.volts_per_full_scale, 10.0);
        assert!(report
            .peak_unclipped_abs_output_v
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0));
        assert_eq!(report.control_ingress.enqueued_events, 1);

        let mut telemetry = MaybeUninit::<RecordPlayerTelemetry>::uninit();
        assert_eq!(
            unsafe { record_player_get_telemetry(handle, telemetry.as_mut_ptr()) },
            RECORD_PLAYER_STATUS_OK
        );
        let telemetry = unsafe { telemetry.assume_init() };
        assert_eq!(telemetry.groove_loaded, 1);
        assert_eq!(telemetry.output_sample_rate_hz, 48_000);
        assert_eq!(telemetry.rendered_host_frames, 64);
        assert_eq!(telemetry.scratch.preset, RECORD_PLAYER_SCRATCH_PRESET_BABY);
        assert_eq!(telemetry.scratch.clicks, 1);
        assert_eq!(
            telemetry.scratch.crossfader_owner,
            RECORD_PLAYER_SCRATCH_CROSSFADER_MANUAL
        );
        assert!((0.0..=1.0).contains(&telemetry.scratch.audible_gain));
        assert!((0.0..=1.0).contains(&telemetry.scratch.automatic_gate_gain));
        assert!((0.0..=1.0).contains(&telemetry.scratch.automatic_gate_target));
        assert_eq!(telemetry.pickup.stylus_lowered, 1);
        assert_eq!(
            telemetry.pickup.contact_surface,
            RECORD_PLAYER_PICKUP_CONTACT_GROOVE_WALLS
        );
        assert_eq!(
            telemetry.radial_tracking.contact_region,
            RECORD_PLAYER_RADIAL_REGION_GROOVE
        );
        assert_eq!(telemetry.pickup.land_contact, 0);
        assert_eq!(telemetry.radial_tracking.land_contact, 0);
        assert!(telemetry.pickup.land_gap_m.is_finite());
        assert!(telemetry.pickup.land_normal_force_n.is_finite());
        assert!(telemetry.pickup.bearing_friction_force_n.is_finite());
        assert!(telemetry.pickup.groove_lateral_force_on_tip_n.is_finite());
        assert!(telemetry.spiral_frame_position.is_finite());
        assert!(telemetry.groove_frame_position.is_finite());
        assert!((1..=4).contains(&telemetry.swept_contact_substeps));
        assert!(telemetry
            .radial_tracking
            .spiral_reference_radius_m
            .is_finite());

        assert_eq!(
            unsafe { record_player_unload_groove(handle) },
            RECORD_PLAYER_STATUS_OK
        );
        assert_eq!(
            unsafe { record_player_destroy(handle) },
            RECORD_PLAYER_STATUS_OK
        );
    }

    #[test]
    fn control_and_render_errors_preserve_core_categories() {
        let handle = create();
        let mut invalid = stopped_control(0, 1);
        invalid.control.hand_contact = 2;
        assert_eq!(
            unsafe { record_player_submit_timed_control(handle, &invalid) },
            RECORD_PLAYER_STATUS_CONTROL_INVALID
        );

        for invalid_control in [
            {
                let mut value = stopped_control(0, 1);
                value.control.scratch_preset = RECORD_PLAYER_SCRATCH_PRESET_DRUM + 1;
                value
            },
            {
                let mut value = stopped_control(0, 1);
                value.control.scratch_clicks = 9;
                value
            },
            {
                let mut value = stopped_control(0, 1);
                value.control.manual_crossfader_gain = f64::NAN;
                value
            },
            {
                let mut value = stopped_control(0, 1);
                value.control.manual_crossfader_gain = -0.01;
                value
            },
        ] {
            assert_eq!(
                unsafe { record_player_submit_timed_control(handle, &invalid_control) },
                RECORD_PLAYER_STATUS_CONTROL_INVALID
            );
        }

        let mut output = vec![0.0; 2];
        assert_eq!(
            unsafe {
                record_player_render_interleaved(handle, output.as_mut_ptr(), 1, ptr::null_mut())
            },
            RECORD_PLAYER_STATUS_OK
        );
        let late = stopped_control(0, 2);
        assert_eq!(
            unsafe { record_player_submit_timed_control(handle, &late) },
            RECORD_PLAYER_STATUS_OK
        );
        let mut report = MaybeUninit::<RecordPlayerRenderReport>::uninit();
        assert_eq!(
            unsafe {
                record_player_render_interleaved(
                    handle,
                    output.as_mut_ptr(),
                    1,
                    report.as_mut_ptr(),
                )
            },
            RECORD_PLAYER_STATUS_OK
        );
        let report = unsafe { report.assume_init() };
        assert_eq!(report.control_ingress.retimed_late_events, 1);
        assert_eq!(report.control_ingress.enqueued_events, 1);

        let mut info = MaybeUninit::<RecordPlayerInfo>::uninit();
        assert_eq!(
            unsafe { record_player_get_info(handle, info.as_mut_ptr()) },
            RECORD_PLAYER_STATUS_OK
        );
        let info = unsafe { info.assume_init() };
        let too_large = info.maximum_render_frames as usize + 1;
        let mut large_output = vec![0.0; too_large * 2];
        assert_eq!(
            unsafe {
                record_player_render_interleaved(
                    handle,
                    large_output.as_mut_ptr(),
                    too_large,
                    ptr::null_mut(),
                )
            },
            RECORD_PLAYER_STATUS_RENDER_BLOCK_TOO_LARGE
        );
        assert_eq!(
            unsafe { record_player_destroy(handle) },
            RECORD_PLAYER_STATUS_OK
        );
    }

    #[test]
    fn mailbox_backpressure_rejects_without_overwriting() {
        let handle = create_with_capacity(1);
        let first = stopped_control(100, 1);
        let second = stopped_control(101, 2);
        assert_eq!(
            unsafe { record_player_submit_timed_control(handle, &first) },
            RECORD_PLAYER_STATUS_OK
        );
        assert_eq!(
            unsafe { record_player_submit_timed_control(handle, &second) },
            RECORD_PLAYER_STATUS_CONTROL_QUEUE_FULL
        );

        let mut report = MaybeUninit::<RecordPlayerRenderReport>::uninit();
        assert_eq!(
            unsafe {
                record_player_render_interleaved(handle, ptr::null_mut(), 0, report.as_mut_ptr())
            },
            RECORD_PLAYER_STATUS_OK
        );
        let report = unsafe { report.assume_init() };
        assert_eq!(report.control_ingress.enqueued_events, 1);
        assert_eq!(
            unsafe { record_player_submit_timed_control(handle, &second) },
            RECORD_PLAYER_STATUS_OK
        );

        let mut info = MaybeUninit::<RecordPlayerInfo>::uninit();
        assert_eq!(
            unsafe { record_player_get_info(handle, info.as_mut_ptr()) },
            RECORD_PLAYER_STATUS_OK
        );
        let info = unsafe { info.assume_init() };
        assert_eq!(info.control_mailbox_capacity, 1);
        assert_eq!(info.accepted_control_submissions, 2);
        assert_eq!(info.rejected_full_control_submissions, 1);
        assert_eq!(
            unsafe { record_player_destroy(handle) },
            RECORD_PLAYER_STATUS_OK
        );
    }

    #[test]
    fn two_producers_return_busy_before_endpoint_access() {
        let handle = create_with_capacity(2);
        let (entered, release, first_producer) = hold_lane(handle, AccessLane::Producer);
        entered.wait();

        let event = stopped_control(0, 1);
        assert_eq!(
            unsafe { record_player_submit_timed_control(handle, ptr::null()) },
            RECORD_PLAYER_STATUS_BUSY
        );
        assert_eq!(
            unsafe { record_player_submit_timed_control(handle, &event) },
            RECORD_PLAYER_STATUS_BUSY
        );
        let mut info = MaybeUninit::<RecordPlayerInfo>::uninit();
        assert_eq!(
            unsafe { record_player_get_info(handle, info.as_mut_ptr()) },
            RECORD_PLAYER_STATUS_BUSY
        );

        release.wait();
        first_producer.join().unwrap();
        assert_eq!(
            unsafe { record_player_submit_timed_control(handle, &event) },
            RECORD_PLAYER_STATUS_OK
        );
        assert_eq!(
            unsafe { record_player_destroy(handle) },
            RECORD_PLAYER_STATUS_OK
        );
    }

    #[test]
    fn two_renderers_return_busy_before_render_state_access() {
        let handle = create();
        let (entered, release, first_renderer) = hold_lane(handle, AccessLane::Render);
        entered.wait();

        let mut output = [0.0f32; 2];
        assert_eq!(
            unsafe {
                record_player_render_interleaved(handle, ptr::null_mut(), 1, ptr::null_mut())
            },
            RECORD_PLAYER_STATUS_BUSY
        );
        let mut telemetry = MaybeUninit::<RecordPlayerTelemetry>::uninit();
        assert_eq!(
            unsafe { record_player_get_telemetry(handle, telemetry.as_mut_ptr()) },
            RECORD_PLAYER_STATUS_BUSY
        );

        release.wait();
        first_renderer.join().unwrap();
        assert_eq!(
            unsafe {
                record_player_render_interleaved(handle, output.as_mut_ptr(), 1, ptr::null_mut())
            },
            RECORD_PLAYER_STATUS_OK
        );
        assert_eq!(
            unsafe { record_player_destroy(handle) },
            RECORD_PLAYER_STATUS_OK
        );
    }

    #[test]
    fn lifecycle_and_active_lanes_return_busy_without_deadlock() {
        let handle = create();
        let (entered, release, renderer) = hold_lane(handle, AccessLane::Render);
        entered.wait();
        assert_eq!(
            unsafe { record_player_unload_groove(handle) },
            RECORD_PLAYER_STATUS_BUSY
        );
        assert_eq!(
            unsafe { record_player_destroy(handle) },
            RECORD_PLAYER_STATUS_BUSY
        );
        release.wait();
        renderer.join().unwrap();

        let (entered, release, lifecycle) = hold_lane(handle, AccessLane::Lifecycle);
        entered.wait();
        let event = stopped_control(0, 1);
        assert_eq!(
            unsafe { record_player_submit_timed_control(handle, &event) },
            RECORD_PLAYER_STATUS_BUSY
        );
        let mut output = [0.0f32; 2];
        assert_eq!(
            unsafe {
                record_player_render_interleaved(handle, output.as_mut_ptr(), 1, ptr::null_mut())
            },
            RECORD_PLAYER_STATUS_BUSY
        );
        release.wait();
        lifecycle.join().unwrap();

        assert_eq!(
            unsafe { record_player_destroy(handle) },
            RECORD_PLAYER_STATUS_OK
        );
    }

    #[test]
    fn one_producer_and_one_renderer_run_concurrently() {
        const EVENT_COUNT: u64 = if cfg!(miri) { 32 } else { 1_000 };
        let handle = create_with_capacity(8);
        let send_handle = SendHandle(handle);
        let producer = std::thread::spawn(move || {
            let handle = send_handle.pointer();
            let mut full_rejections = 0u64;
            for sequence in 1..=EVENT_COUNT {
                let event = stopped_control(sequence * 8, sequence);
                loop {
                    match unsafe { record_player_submit_timed_control(handle, &event) } {
                        RECORD_PLAYER_STATUS_OK => break,
                        RECORD_PLAYER_STATUS_CONTROL_QUEUE_FULL => {
                            full_rejections += 1;
                            std::thread::yield_now();
                        }
                        status => panic!("unexpected producer status {status}"),
                    }
                }
            }
            full_rejections
        });

        let mut inspected = 0u64;
        let mut rejected = 0u64;
        let mut output = [0.0f32; 16];
        while !producer.is_finished() || inspected < EVENT_COUNT {
            let mut report = MaybeUninit::<RecordPlayerRenderReport>::uninit();
            assert_eq!(
                unsafe {
                    record_player_render_interleaved(
                        handle,
                        output.as_mut_ptr(),
                        output.len() / 2,
                        report.as_mut_ptr(),
                    )
                },
                RECORD_PLAYER_STATUS_OK
            );
            let report = unsafe { report.assume_init() };
            inspected += report.control_ingress.inspected_events;
            rejected += report.control_ingress.rejected_invalid_encodings
                + report.control_ingress.rejected_invalid_controls
                + report.control_ingress.rejected_duplicates
                + report.control_ingress.rejected_nonmonotonic_frames
                + report.control_ingress.rejected_nonmonotonic_sequences
                + report.control_ingress.protocol_errors;
            std::thread::yield_now();
        }
        let _full_rejections = producer.join().unwrap();
        assert_eq!(inspected, EVENT_COUNT);
        assert_eq!(rejected, 0);

        let mut info = MaybeUninit::<RecordPlayerInfo>::uninit();
        assert_eq!(
            unsafe { record_player_get_info(handle, info.as_mut_ptr()) },
            RECORD_PLAYER_STATUS_OK
        );
        let info = unsafe { info.assume_init() };
        assert_eq!(info.accepted_control_submissions, EVENT_COUNT);
        assert_eq!(info.inspected_control_submissions, EVENT_COUNT);
        assert_eq!(
            unsafe { record_player_destroy(handle) },
            RECORD_PLAYER_STATUS_OK
        );
    }
}
