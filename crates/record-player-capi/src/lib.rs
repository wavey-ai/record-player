//! Provides a thin C ABI for the record-player scratch gesture mapper.
//!
//! This crate validates ABI values and transfers them to `record-player`.
//! It does not define physics values or gesture behavior.
//!
//! The physical host renderer's C surface (`record_player_create` and the
//! render/telemetry calls that hung off it) went with the physical module it
//! wrapped; the gesture mapper is what remains and what callers use.

#![deny(unsafe_op_in_unsafe_fn)]

use std::cell::UnsafeCell;
use std::ffi::c_char;
use std::mem::align_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU8, Ordering};

use record_player::timed_control::{PlayerControl, TimedPlayerControl};
use record_player::{
    DeckMechanicalControl, MotorMode, ScheduledScratchHandControl,
    ScratchGestureConfig, ScratchGestureError, ScratchGestureMapper, ScratchPointerSample,
    ScratchPreset,
};
use record_player::gesture::ScratchPressureCalibration;

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

/// Owns the canonical pointer-to-hand mapper for one gesture stream.
pub struct RecordPlayerScratchGestureHandle {
    mapper: UnsafeCell<ScratchGestureMapper>,
    access_state: AtomicU8,
}

// SAFETY: The atomic guard permits only one mapper reference at a time.
unsafe impl Sync for RecordPlayerScratchGestureHandle {}

/// The rate the gesture mapper resolves hand motion at. It was
/// `physical::PHYSICAL_OUTPUT_INPUT_RATE_HZ` before that module was removed;
/// the mapper is the only thing that still needs the value, so it lives here
/// now rather than pulling a renderer back in for one constant.
const GESTURE_INTERNAL_RATE_HZ: u32 = 192_000;

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

unsafe fn scratch_handle_ref<'a>(
    handle: *mut RecordPlayerScratchGestureHandle,
) -> Result<&'a RecordPlayerScratchGestureHandle, RecordPlayerStatus> {
    pointer_status(handle)?;
    // SAFETY: The C contract requires a live handle for the call duration.
    Ok(unsafe { &*handle })
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

/// Creates one gesture mapper.
///
/// # Safety
///
/// `options` must point at a valid options struct and `out_handle` must be
/// writable under the C contract.
#[no_mangle]
pub unsafe extern "C" fn record_player_scratch_gesture_create(
    options: *const RecordPlayerScratchGestureOptions,
    out_handle: *mut *mut RecordPlayerScratchGestureHandle,
) -> RecordPlayerStatus {
    run_ffi(|| {
        let options = unsafe { *input_ref(options)? };
        let out_handle = unsafe { output_pointer(out_handle)? };
        // SAFETY: The output pointer is writable under the C contract.
        unsafe { out_handle.write(std::ptr::null_mut()) };
        if options.abi_version != RECORD_PLAYER_CAPI_ABI_VERSION {
            return Err(RECORD_PLAYER_STATUS_UNSUPPORTED_ABI_VERSION);
        }
        if options.reserved != [0; 2] {
            return Err(RECORD_PLAYER_STATUS_INVALID_ARGUMENT);
        }
        let config = ScratchGestureConfig {
            internal_sample_rate_hz: GESTURE_INTERNAL_RATE_HZ,
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

