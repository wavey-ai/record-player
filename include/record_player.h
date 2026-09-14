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
#define RECORD_PLAYER_STATUS_BUSY 8
#define RECORD_PLAYER_STATUS_CONTROL_INVALID 20
#define RECORD_PLAYER_STATUS_GESTURE_INVALID 50
#define RECORD_PLAYER_STATUS_GESTURE_STATE 51
#define RECORD_PLAYER_STATUS_PANIC 127

typedef uint32_t RecordPlayerMotorMode;
#define RECORD_PLAYER_MOTOR_OFF 0u
#define RECORD_PLAYER_MOTOR_SERVO 1u
#define RECORD_PLAYER_MOTOR_BRAKE 2u

typedef uint32_t RecordPlayerScratchPreset;
#define RECORD_PLAYER_SCRATCH_PRESET_BABY 0u
#define RECORD_PLAYER_SCRATCH_PRESET_STAB 1u
#define RECORD_PLAYER_SCRATCH_PRESET_CHIRP 2u
#define RECORD_PLAYER_SCRATCH_PRESET_TRANSFORM 3u
#define RECORD_PLAYER_SCRATCH_PRESET_FLARE 4u
#define RECORD_PLAYER_SCRATCH_PRESET_CRAB 5u
#define RECORD_PLAYER_SCRATCH_PRESET_ORBIT 6u
#define RECORD_PLAYER_SCRATCH_PRESET_DRUM 7u

typedef struct RecordPlayerScratchGestureHandle RecordPlayerScratchGestureHandle;

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
/* Call each handle from one thread at a time; a second call returns BUSY. */
/* Pass zero for all reserved fields. Pass only zero or one for boolean fields. */
/* Destroy each successful handle exactly one time. */

#ifdef __cplusplus
}
#endif

#if defined(__cplusplus)
static_assert(sizeof(RecordPlayerControl) == 64, "RecordPlayerControl layout changed");
static_assert(sizeof(RecordPlayerTimedControl) == 80, "RecordPlayerTimedControl layout changed");
static_assert(sizeof(RecordPlayerScratchGestureOptions) == 40, "RecordPlayerScratchGestureOptions layout changed");
static_assert(sizeof(RecordPlayerScratchPointerSample) == 48, "RecordPlayerScratchPointerSample layout changed");
static_assert(sizeof(RecordPlayerScratchControlResult) == 112, "RecordPlayerScratchControlResult layout changed");
static_assert(offsetof(RecordPlayerScratchControlResult, raw_pointer_angular_velocity_rad_s) == 80, "RecordPlayerScratchControlResult raw velocity offset changed");
#else
_Static_assert(sizeof(RecordPlayerControl) == 64, "RecordPlayerControl layout changed");
_Static_assert(sizeof(RecordPlayerTimedControl) == 80, "RecordPlayerTimedControl layout changed");
_Static_assert(sizeof(RecordPlayerScratchGestureOptions) == 40, "RecordPlayerScratchGestureOptions layout changed");
_Static_assert(sizeof(RecordPlayerScratchPointerSample) == 48, "RecordPlayerScratchPointerSample layout changed");
_Static_assert(sizeof(RecordPlayerScratchControlResult) == 112, "RecordPlayerScratchControlResult layout changed");
_Static_assert(offsetof(RecordPlayerScratchControlResult, raw_pointer_angular_velocity_rad_s) == 80, "RecordPlayerScratchControlResult raw velocity offset changed");
#endif

#endif
