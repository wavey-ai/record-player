#ifndef RECORD_PLAYER_H
#define RECORD_PLAYER_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RECORD_PLAYER_CAPI_ABI_VERSION 5u

typedef int32_t RecordPlayerStatus;

#define RECORD_PLAYER_STATUS_OK 0
#define RECORD_PLAYER_STATUS_NULL_POINTER 1
#define RECORD_PLAYER_STATUS_MISALIGNED_POINTER 2
#define RECORD_PLAYER_STATUS_INVALID_ARGUMENT 3
#define RECORD_PLAYER_STATUS_UNSUPPORTED_ABI_VERSION 4
#define RECORD_PLAYER_STATUS_UNSUPPORTED_OUTPUT_SAMPLE_RATE 5
#define RECORD_PLAYER_STATUS_ALLOCATION_FAILED 6
#define RECORD_PLAYER_STATUS_CREATE_FAILED 7
#define RECORD_PLAYER_STATUS_BUSY 8
#define RECORD_PLAYER_STATUS_GROOVE_CUT_FAILED 10
#define RECORD_PLAYER_STATUS_GROOVE_LAYOUT_MISMATCH 11
#define RECORD_PLAYER_STATUS_GROOVE_PROGRAMME_DOES_NOT_FIT 12
#define RECORD_PLAYER_STATUS_GROOVE_OVERCUT 13
#define RECORD_PLAYER_STATUS_GROOVE_LOAD_FAILED 14
#define RECORD_PLAYER_STATUS_CONTROL_INVALID 20
#define RECORD_PLAYER_STATUS_CONTROL_LATE 21
#define RECORD_PLAYER_STATUS_CONTROL_DUPLICATE 22
#define RECORD_PLAYER_STATUS_CONTROL_NONMONOTONIC_FRAME 23
#define RECORD_PLAYER_STATUS_CONTROL_NONMONOTONIC_SEQUENCE 24
#define RECORD_PLAYER_STATUS_CONTROL_QUEUE_FULL 25
#define RECORD_PLAYER_STATUS_RENDER_BLOCK_TOO_LARGE 30
#define RECORD_PLAYER_STATUS_RENDER_FAILED 31
#define RECORD_PLAYER_STATUS_CORE_ERROR 40
#define RECORD_PLAYER_STATUS_GESTURE_INVALID 50
#define RECORD_PLAYER_STATUS_GESTURE_STATE 51
#define RECORD_PLAYER_STATUS_PANIC 127

typedef uint32_t RecordPlayerMotorMode;
#define RECORD_PLAYER_MOTOR_OFF 0u
#define RECORD_PLAYER_MOTOR_SERVO 1u
#define RECORD_PLAYER_MOTOR_BRAKE 2u

typedef uint32_t RecordPlayerContactMode;
#define RECORD_PLAYER_CONTACT_SEPARATED 0u
#define RECORD_PLAYER_CONTACT_STICKING 1u
#define RECORD_PLAYER_CONTACT_SLIDING_POSITIVE 2u
#define RECORD_PLAYER_CONTACT_SLIDING_NEGATIVE 3u

typedef uint32_t RecordPlayerScratchPreset;
#define RECORD_PLAYER_SCRATCH_PRESET_BABY 0u
#define RECORD_PLAYER_SCRATCH_PRESET_STAB 1u
#define RECORD_PLAYER_SCRATCH_PRESET_CHIRP 2u
#define RECORD_PLAYER_SCRATCH_PRESET_TRANSFORM 3u
#define RECORD_PLAYER_SCRATCH_PRESET_FLARE 4u
#define RECORD_PLAYER_SCRATCH_PRESET_CRAB 5u
#define RECORD_PLAYER_SCRATCH_PRESET_ORBIT 6u
#define RECORD_PLAYER_SCRATCH_PRESET_DRUM 7u

typedef uint32_t RecordPlayerScratchCrossfaderOwner;
#define RECORD_PLAYER_SCRATCH_CROSSFADER_MANUAL 0u
#define RECORD_PLAYER_SCRATCH_CROSSFADER_AUTOMATIC_PRESET 1u

typedef uint32_t RecordPlayerPickupContactSurface;
#define RECORD_PLAYER_PICKUP_CONTACT_NONE 0u
#define RECORD_PLAYER_PICKUP_CONTACT_GROOVE_WALLS 1u
#define RECORD_PLAYER_PICKUP_CONTACT_RECORD_LAND 2u

typedef uint32_t RecordPlayerRadialContactRegion;
#define RECORD_PLAYER_RADIAL_REGION_GROOVE 0u
#define RECORD_PLAYER_RADIAL_REGION_LAND 1u
#define RECORD_PLAYER_RADIAL_REGION_LIFTED 2u

typedef struct RecordPlayerHandle RecordPlayerHandle;
typedef struct RecordPlayerScratchGestureHandle RecordPlayerScratchGestureHandle;

typedef struct RecordPlayerCreateOptions {
    uint32_t abi_version;
    uint32_t output_sample_rate_hz;
    /* Zero selects the control-timeline capacity from the core profile. */
    uint32_t control_mailbox_capacity;
    uint32_t reserved;
    /* The renderer maps this many phono volts to host full scale. */
    double volts_per_full_scale;
} RecordPlayerCreateOptions;

/* This value contains one complete control state. It is not a control delta. */
typedef struct RecordPlayerControl {
    RecordPlayerMotorMode motor_mode;
    uint8_t hand_contact;
    uint8_t hand_target_angle_present;
    uint8_t stylus_lowered;
    uint8_t reserved;
    double motor_target_angular_velocity_rad_s;
    double hand_target_angle_rad;
    double hand_target_angular_velocity_rad_s;
    double hand_normal_force_n;
    double hand_contact_radius_m;
    double manual_crossfader_gain;
    RecordPlayerScratchPreset scratch_preset;
    /* Zero selects the preset's default click count. */
    uint8_t scratch_clicks;
    uint8_t scratch_reserved[3];
} RecordPlayerControl;

typedef struct RecordPlayerTimedControl {
    uint64_t absolute_frame;
    uint64_t sequence;
    RecordPlayerControl control;
} RecordPlayerTimedControl;

typedef struct RecordPlayerScratchGestureOptions {
    uint32_t abi_version;
    uint32_t lookahead_frames;
    uint32_t reserved[2];
    double zero_pressure_force_n;
    double unit_pressure_force_n;
    double unreported_pressure_force_n;
} RecordPlayerScratchGestureOptions;

typedef struct RecordPlayerScratchPointerSample {
    uint64_t pointer_id;
    uint64_t source_time_ns;
    double angle_rad;
    double contact_radius_m;
    double normalized_pressure;
    uint8_t pressure_present;
    uint8_t reserved[7];
} RecordPlayerScratchPointerSample;

typedef struct RecordPlayerScratchControlResult {
    RecordPlayerTimedControl event;
    double raw_pointer_angular_velocity_rad_s;
    uint64_t added_late_shift_frames;
    uint64_t total_late_shift_frames;
    uint8_t velocity_was_limited;
    uint8_t wrap_was_ambiguous;
    uint8_t reserved[6];
} RecordPlayerScratchControlResult;

typedef struct RecordPlayerInfo {
    uint32_t abi_version;
    uint32_t output_sample_rate_hz;
    uint32_t internal_sample_rate_hz;
    uint32_t control_mailbox_capacity;
    uint64_t maximum_render_frames;
    uint64_t latency_internal_frames;
    double latency_seconds;
    double volts_per_full_scale;
    uint64_t current_internal_frame;
    uint64_t rendered_host_frames;
    uint64_t accepted_control_submissions;
    uint64_t rejected_full_control_submissions;
    uint64_t inspected_control_submissions;
} RecordPlayerInfo;

typedef struct RecordPlayerSchedulePoint {
    uint64_t observed_rendered_host_frames;
    uint64_t current_internal_frame;
    uint64_t host_frame_offset;
    uint64_t absolute_internal_frame;
} RecordPlayerSchedulePoint;

typedef struct RecordPlayerControlIngressReport {
    uint64_t inspected_events;
    uint64_t enqueued_events;
    uint64_t retimed_late_events;
    uint64_t rejected_invalid_encodings;
    uint64_t rejected_invalid_controls;
    uint64_t rejected_duplicates;
    uint64_t rejected_nonmonotonic_frames;
    uint64_t rejected_nonmonotonic_sequences;
    uint64_t protocol_errors;
    uint8_t stopped_for_timeline_backpressure;
    uint8_t producer_disconnected;
    uint8_t reserved[6];
} RecordPlayerControlIngressReport;

typedef struct RecordPlayerRenderReport {
    uint32_t output_sample_rate_hz;
    uint32_t reserved;
    uint64_t rendered_host_frames;
    uint64_t rendered_internal_frames;
    uint64_t absolute_internal_frame;
    double volts_per_full_scale;
    double peak_unclipped_abs_output_v[2];
    uint64_t clipped_samples[2];
    uint64_t total_clipped_samples[2];
    RecordPlayerControlIngressReport control_ingress;
} RecordPlayerRenderReport;

typedef struct RecordPlayerDeckTelemetry {
    double mechanical_time_seconds;
    double platter_rate;
    double record_rate;
    double platter_angle_turns;
    double record_angle_turns;
    double motor_torque_nm;
    double slipmat_torque_nm;
    double hand_torque_nm;
    double bearing_torque_nm;
    double stylus_torque_nm;
    RecordPlayerContactMode slipmat_mode;
    RecordPlayerContactMode hand_mode;
    uint8_t bearing_sticking;
    uint8_t reserved[7];
} RecordPlayerDeckTelemetry;

typedef struct RecordPlayerPickupTelemetry {
    double tip_displacement_m[2];
    double tip_velocity_m_s[2];
    double body_displacement_m[2];
    double body_velocity_m_s[2];
    double relative_displacement_m[2];
    double relative_velocity_m_s[2];
    double suspension_force_on_tip_n[2];
    double electromagnetic_force_on_tip_n[2];
    double wall_gap_m[2];
    double wall_normal_force_n[2];
    uint8_t wall_contact[2];
    uint8_t land_contact;
    uint8_t stylus_lowered;
    RecordPlayerPickupContactSurface contact_surface;
    double land_gap_m;
    double land_normal_force_n;
    double coulomb_friction_force_n;
    double modulation_reaction_force_n;
    double record_reaction_force_tangent_n;
    double groove_radius_m;
    double skating_force_n;
    double bearing_friction_force_n;
    double groove_lateral_force_on_tip_n;
    double kinetic_energy_j;
    double suspension_energy_j;
    uint64_t completed_steps;
} RecordPlayerPickupTelemetry;

typedef struct RecordPlayerCartridgeTelemetry {
    double magnet_velocity_m_s[2];
    double generator_voltage_v[2];
    double coil_current_a[2];
    double load_output_voltage_v[2];
    double electromagnetic_reaction_force_n[2];
    double generator_electrical_power_w;
    double coil_loss_power_w;
    double load_power_w;
    double stored_electrical_energy_j;
    uint64_t completed_steps;
} RecordPlayerCartridgeTelemetry;

typedef struct RecordPlayerRadialTrackingTelemetry {
    double spiral_reference_radius_m;
    double stylus_radius_m;
    double radial_velocity_m_s;
    double groove_pitch_m_per_revolution;
    double captured_groove_radius_m;
    double radial_error_to_retained_turn_m;
    double groove_radial_velocity_m_s;
    double guide_force_n;
    double bearing_friction_force_n;
    double applied_radial_force_n;
    uint8_t captured_groove_radius_present;
    uint8_t groove_contact;
    uint8_t land_contact;
    uint8_t contact_lost_this_step;
    uint8_t recaptured_this_step;
    uint8_t recapture_limit_reached;
    uint8_t stylus_lowered;
    uint8_t macro_contact_available;
    RecordPlayerRadialContactRegion contact_region;
    uint32_t reserved;
    int64_t captured_turn_index;
    int64_t turns_skipped_this_step;
    int64_t total_turns_skipped;
    uint64_t completed_steps;
} RecordPlayerRadialTrackingTelemetry;

typedef struct RecordPlayerScratchTelemetry {
    RecordPlayerScratchPreset preset;
    RecordPlayerScratchCrossfaderOwner crossfader_owner;
    uint8_t clicks;
    int8_t direction;
    uint8_t moving;
    uint8_t reserved[5];
    double audible_gain;
    double automatic_gate_gain;
    double automatic_gate_target;
    double phase;
    double stroke_progress;
    double span_prediction_confidence;
} RecordPlayerScratchTelemetry;

typedef struct RecordPlayerTelemetry {
    uint32_t abi_version;
    uint32_t output_sample_rate_hz;
    uint64_t rendered_host_frames;
    uint64_t rendered_internal_frames;
    uint64_t absolute_internal_frame;
    double spiral_frame_position;
    double groove_frame_position;
    double groove_radius_m;
    uint32_t spatial_filter_lower_step_frames;
    uint32_t spatial_filter_upper_step_frames;
    double spatial_filter_upper_blend;
    double phono_output_v[2];
    uint8_t groove_loaded;
    uint8_t at_programme_boundary;
    uint8_t phono_input_overload[2];
    uint8_t phono_output_overload[2];
    uint8_t swept_contact_substeps;
    uint8_t reserved[1];
    RecordPlayerRadialTrackingTelemetry radial_tracking;
    RecordPlayerDeckTelemetry deck;
    RecordPlayerPickupTelemetry pickup;
    RecordPlayerCartridgeTelemetry cartridge;
    RecordPlayerScratchTelemetry scratch;
} RecordPlayerTelemetry;

uint32_t record_player_capi_abi_version(void);
const char *record_player_status_message(RecordPlayerStatus status);

/* The gesture mapper converts pointer samples to complete timed controls. */
/* Call each gesture handle from only one thread at a time. */
RecordPlayerStatus record_player_scratch_gesture_create(
    const RecordPlayerScratchGestureOptions *options,
    RecordPlayerScratchGestureHandle **out_handle);
RecordPlayerStatus record_player_scratch_gesture_destroy(
    RecordPlayerScratchGestureHandle *handle);
RecordPlayerStatus record_player_scratch_gesture_begin(
    RecordPlayerScratchGestureHandle *handle,
    const RecordPlayerScratchPointerSample *sample,
    uint64_t minimum_render_frame,
    double record_angle_rad,
    uint64_t sequence,
    const RecordPlayerControl *base_control,
    RecordPlayerScratchControlResult *out_result);
RecordPlayerStatus record_player_scratch_gesture_update(
    RecordPlayerScratchGestureHandle *handle,
    const RecordPlayerScratchPointerSample *sample,
    uint64_t minimum_render_frame,
    uint64_t sequence,
    const RecordPlayerControl *base_control,
    RecordPlayerScratchControlResult *out_result);
RecordPlayerStatus record_player_scratch_gesture_finish(
    RecordPlayerScratchGestureHandle *handle,
    uint64_t pointer_id,
    uint64_t source_time_ns,
    uint64_t minimum_render_frame,
    uint64_t sequence,
    const RecordPlayerControl *base_control,
    RecordPlayerScratchControlResult *out_result);

/* Create and destroy can allocate. Call them outside the audio thread. */
RecordPlayerStatus record_player_create(
    const RecordPlayerCreateOptions *options,
    RecordPlayerHandle **out_handle);
RecordPlayerStatus record_player_destroy(RecordPlayerHandle *handle);

RecordPlayerStatus record_player_get_info(
    RecordPlayerHandle *handle,
    RecordPlayerInfo *out_info);

/* This function uses one atomic snapshot of the rendered host-frame count. */
/* Offset zero returns the first physical frame that is not rendered. */
RecordPlayerStatus record_player_schedule_point_after_host_frames(
    RecordPlayerHandle *handle,
    uint64_t host_frame_offset,
    RecordPlayerSchedulePoint *out_schedule_point);

/* This function builds a groove. Call it outside the audio thread. */
RecordPlayerStatus record_player_load_interleaved_pcm(
    RecordPlayerHandle *handle,
    const float *interleaved_pcm,
    size_t frame_count,
    uint32_t channel_count,
    double source_sample_rate_hz);
RecordPlayerStatus record_player_unload_groove(RecordPlayerHandle *handle);
RecordPlayerStatus record_player_set_groove_frame_position(
    RecordPlayerHandle *handle,
    double position);
RecordPlayerStatus record_player_reset_transport(
    RecordPlayerHandle *handle,
    double platter_rate,
    double record_rate,
    double platter_angle_turns,
    double record_angle_turns);

/* The control frame uses the internal_sample_rate_hz clock from RecordPlayerInfo. */
/* A control applies before the engine renders its absolute_frame. */
/* A late control moves to the first physical frame that is not rendered. */
/* The next render report identifies late or rejected controls. */
/* This function does not allocate. */
RecordPlayerStatus record_player_submit_timed_control(
    RecordPlayerHandle *handle,
    const RecordPlayerTimedControl *event);

/* Output has frame_count interleaved stereo host samples. */
/* The renderer divides phono volts by volts_per_full_scale. */
/* The renderer clips each host sample to the inclusive range [-1, 1]. */
/* The report retains the peak voltage before host clipping. */
/* This function does not allocate after successful construction. */
RecordPlayerStatus record_player_render_interleaved(
    RecordPlayerHandle *handle,
    float *output,
    size_t frame_count,
    RecordPlayerRenderReport *out_report);

RecordPlayerStatus record_player_get_telemetry(
    RecordPlayerHandle *handle,
    RecordPlayerTelemetry *out_telemetry);

/* Submit, get_info, and schedule_point use the producer lane. */
/* Render and get_telemetry use the render lane. */
/* One producer-lane call and one render-lane call can run together. */
/* Another call on an active lane returns RECORD_PLAYER_STATUS_BUSY. */
/* Lifecycle calls require both lanes to be idle. */
/* A lifecycle conflict returns RECORD_PLAYER_STATUS_BUSY. */
/* Destroy returns BUSY while a guarded call is active. */
/* Call destroy from one thread after the application prevents new calls. */
/* Pass zero for all reserved fields. Pass only zero or one for boolean fields. */
/* Destroy each successful handle exactly one time. */

#ifdef __cplusplus
}
#endif

#if defined(__cplusplus)
static_assert(sizeof(RecordPlayerCreateOptions) == 24, "RecordPlayerCreateOptions layout changed");
static_assert(offsetof(RecordPlayerCreateOptions, volts_per_full_scale) == 16, "RecordPlayerCreateOptions volts-per-full-scale offset changed");
static_assert(sizeof(RecordPlayerControl) == 64, "RecordPlayerControl layout changed");
static_assert(sizeof(RecordPlayerTimedControl) == 80, "RecordPlayerTimedControl layout changed");
static_assert(sizeof(RecordPlayerScratchGestureOptions) == 40, "RecordPlayerScratchGestureOptions layout changed");
static_assert(sizeof(RecordPlayerScratchPointerSample) == 48, "RecordPlayerScratchPointerSample layout changed");
static_assert(sizeof(RecordPlayerScratchControlResult) == 112, "RecordPlayerScratchControlResult layout changed");
static_assert(offsetof(RecordPlayerScratchControlResult, raw_pointer_angular_velocity_rad_s) == 80, "RecordPlayerScratchControlResult raw velocity offset changed");
static_assert(sizeof(RecordPlayerInfo) == 88, "RecordPlayerInfo layout changed");
static_assert(offsetof(RecordPlayerInfo, volts_per_full_scale) == 40, "RecordPlayerInfo volts-per-full-scale offset changed");
static_assert(sizeof(RecordPlayerSchedulePoint) == 32, "RecordPlayerSchedulePoint layout changed");
static_assert(sizeof(RecordPlayerControlIngressReport) == 80, "RecordPlayerControlIngressReport layout changed");
static_assert(sizeof(RecordPlayerRenderReport) == 168, "RecordPlayerRenderReport layout changed");
static_assert(offsetof(RecordPlayerRenderReport, volts_per_full_scale) == 32, "RecordPlayerRenderReport volts-per-full-scale offset changed");
static_assert(offsetof(RecordPlayerRenderReport, peak_unclipped_abs_output_v) == 40, "RecordPlayerRenderReport peak offset changed");
static_assert(offsetof(RecordPlayerRenderReport, clipped_samples) == 56, "RecordPlayerRenderReport clipped-sample offset changed");
static_assert(offsetof(RecordPlayerRenderReport, total_clipped_samples) == 72, "RecordPlayerRenderReport total-clipped-sample offset changed");
static_assert(offsetof(RecordPlayerRenderReport, control_ingress) == 88, "RecordPlayerRenderReport ingress offset changed");
static_assert(sizeof(RecordPlayerDeckTelemetry) == 96, "RecordPlayerDeckTelemetry layout changed");
static_assert(sizeof(RecordPlayerPickupTelemetry) == 264, "RecordPlayerPickupTelemetry layout changed");
static_assert(sizeof(RecordPlayerCartridgeTelemetry) == 120, "RecordPlayerCartridgeTelemetry layout changed");
static_assert(sizeof(RecordPlayerRadialTrackingTelemetry) == 128, "RecordPlayerRadialTrackingTelemetry layout changed");
static_assert(offsetof(RecordPlayerRadialTrackingTelemetry, captured_turn_index) == 96, "RecordPlayerRadialTrackingTelemetry captured_turn_index offset changed");
static_assert(sizeof(RecordPlayerScratchTelemetry) == 64, "RecordPlayerScratchTelemetry layout changed");
static_assert(offsetof(RecordPlayerScratchTelemetry, audible_gain) == 16, "RecordPlayerScratchTelemetry audible gain offset changed");
static_assert(sizeof(RecordPlayerTelemetry) == 768, "RecordPlayerTelemetry layout changed");
static_assert(offsetof(RecordPlayerTelemetry, spiral_frame_position) == 32, "RecordPlayerTelemetry spiral_frame_position offset changed");
static_assert(offsetof(RecordPlayerTelemetry, groove_frame_position) == 40, "RecordPlayerTelemetry groove_frame_position offset changed");
static_assert(offsetof(RecordPlayerTelemetry, radial_tracking) == 96, "RecordPlayerTelemetry radial_tracking offset changed");
static_assert(offsetof(RecordPlayerTelemetry, deck) == 224, "RecordPlayerTelemetry deck offset changed");
static_assert(offsetof(RecordPlayerTelemetry, pickup) == 320, "RecordPlayerTelemetry pickup offset changed");
static_assert(offsetof(RecordPlayerTelemetry, cartridge) == 584, "RecordPlayerTelemetry cartridge offset changed");
static_assert(offsetof(RecordPlayerTelemetry, scratch) == 704, "RecordPlayerTelemetry scratch offset changed");
#else
_Static_assert(sizeof(RecordPlayerCreateOptions) == 24, "RecordPlayerCreateOptions layout changed");
_Static_assert(offsetof(RecordPlayerCreateOptions, volts_per_full_scale) == 16, "RecordPlayerCreateOptions volts-per-full-scale offset changed");
_Static_assert(sizeof(RecordPlayerControl) == 64, "RecordPlayerControl layout changed");
_Static_assert(sizeof(RecordPlayerTimedControl) == 80, "RecordPlayerTimedControl layout changed");
_Static_assert(sizeof(RecordPlayerScratchGestureOptions) == 40, "RecordPlayerScratchGestureOptions layout changed");
_Static_assert(sizeof(RecordPlayerScratchPointerSample) == 48, "RecordPlayerScratchPointerSample layout changed");
_Static_assert(sizeof(RecordPlayerScratchControlResult) == 112, "RecordPlayerScratchControlResult layout changed");
_Static_assert(offsetof(RecordPlayerScratchControlResult, raw_pointer_angular_velocity_rad_s) == 80, "RecordPlayerScratchControlResult raw velocity offset changed");
_Static_assert(sizeof(RecordPlayerInfo) == 88, "RecordPlayerInfo layout changed");
_Static_assert(offsetof(RecordPlayerInfo, volts_per_full_scale) == 40, "RecordPlayerInfo volts-per-full-scale offset changed");
_Static_assert(sizeof(RecordPlayerSchedulePoint) == 32, "RecordPlayerSchedulePoint layout changed");
_Static_assert(sizeof(RecordPlayerControlIngressReport) == 80, "RecordPlayerControlIngressReport layout changed");
_Static_assert(sizeof(RecordPlayerRenderReport) == 168, "RecordPlayerRenderReport layout changed");
_Static_assert(offsetof(RecordPlayerRenderReport, volts_per_full_scale) == 32, "RecordPlayerRenderReport volts-per-full-scale offset changed");
_Static_assert(offsetof(RecordPlayerRenderReport, peak_unclipped_abs_output_v) == 40, "RecordPlayerRenderReport peak offset changed");
_Static_assert(offsetof(RecordPlayerRenderReport, clipped_samples) == 56, "RecordPlayerRenderReport clipped-sample offset changed");
_Static_assert(offsetof(RecordPlayerRenderReport, total_clipped_samples) == 72, "RecordPlayerRenderReport total-clipped-sample offset changed");
_Static_assert(offsetof(RecordPlayerRenderReport, control_ingress) == 88, "RecordPlayerRenderReport ingress offset changed");
_Static_assert(sizeof(RecordPlayerDeckTelemetry) == 96, "RecordPlayerDeckTelemetry layout changed");
_Static_assert(sizeof(RecordPlayerPickupTelemetry) == 264, "RecordPlayerPickupTelemetry layout changed");
_Static_assert(sizeof(RecordPlayerCartridgeTelemetry) == 120, "RecordPlayerCartridgeTelemetry layout changed");
_Static_assert(sizeof(RecordPlayerRadialTrackingTelemetry) == 128, "RecordPlayerRadialTrackingTelemetry layout changed");
_Static_assert(offsetof(RecordPlayerRadialTrackingTelemetry, captured_turn_index) == 96, "RecordPlayerRadialTrackingTelemetry captured_turn_index offset changed");
_Static_assert(sizeof(RecordPlayerScratchTelemetry) == 64, "RecordPlayerScratchTelemetry layout changed");
_Static_assert(offsetof(RecordPlayerScratchTelemetry, audible_gain) == 16, "RecordPlayerScratchTelemetry audible gain offset changed");
_Static_assert(sizeof(RecordPlayerTelemetry) == 768, "RecordPlayerTelemetry layout changed");
_Static_assert(offsetof(RecordPlayerTelemetry, spiral_frame_position) == 32, "RecordPlayerTelemetry spiral_frame_position offset changed");
_Static_assert(offsetof(RecordPlayerTelemetry, groove_frame_position) == 40, "RecordPlayerTelemetry groove_frame_position offset changed");
_Static_assert(offsetof(RecordPlayerTelemetry, radial_tracking) == 96, "RecordPlayerTelemetry radial_tracking offset changed");
_Static_assert(offsetof(RecordPlayerTelemetry, deck) == 224, "RecordPlayerTelemetry deck offset changed");
_Static_assert(offsetof(RecordPlayerTelemetry, pickup) == 320, "RecordPlayerTelemetry pickup offset changed");
_Static_assert(offsetof(RecordPlayerTelemetry, cartridge) == 584, "RecordPlayerTelemetry cartridge offset changed");
_Static_assert(offsetof(RecordPlayerTelemetry, scratch) == 704, "RecordPlayerTelemetry scratch offset changed");
#endif

#endif
