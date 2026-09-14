#include "record_player.h"

int main(void) {
    if (record_player_capi_abi_version() != RECORD_PLAYER_CAPI_ABI_VERSION) {
        return 1;
    }
    if (record_player_status_message(RECORD_PLAYER_STATUS_OK) == NULL) {
        return 2;
    }

    RecordPlayerScratchGestureOptions gesture_options = {
        RECORD_PLAYER_CAPI_ABI_VERSION,
        1920u,
        {0u, 0u},
        0.5,
        5.0,
        2.5,
    };
    RecordPlayerScratchGestureHandle *gesture = NULL;
    if (record_player_scratch_gesture_create(&gesture_options, &gesture)
        != RECORD_PLAYER_STATUS_OK) {
        return 3;
    }
    RecordPlayerControl base_control = {0};
    base_control.motor_mode = RECORD_PLAYER_MOTOR_OFF;
    base_control.stylus_lowered = 1u;
    base_control.hand_contact_radius_m = 0.12;
    base_control.manual_crossfader_gain = 1.0;
    RecordPlayerScratchPointerSample pointer = {
        7u,
        0u,
        0.0,
        0.12,
        0.5,
        1u,
        {0u, 0u, 0u, 0u, 0u, 0u, 0u},
    };
    RecordPlayerScratchControlResult gesture_result;
    if (record_player_scratch_gesture_begin(
            gesture, &pointer, 1000u, 2.0, 1u, &base_control, &gesture_result)
        != RECORD_PLAYER_STATUS_OK) {
        return 4;
    }
    if (gesture_result.event.absolute_frame != 2920u
        || gesture_result.event.control.hand_contact != 1u) {
        return 5;
    }
    pointer.source_time_ns = 1000000u;
    pointer.angle_rad = 0.02;
    if (record_player_scratch_gesture_update(
            gesture, &pointer, 1000u, 2u, &base_control, &gesture_result)
        != RECORD_PLAYER_STATUS_OK) {
        return 6;
    }
    if (gesture_result.event.control.hand_target_angular_velocity_rad_s <= 0.0) {
        return 7;
    }
    if (record_player_scratch_gesture_finish(
            gesture, 7u, 2000000u, 1000u, 3u, &base_control, &gesture_result)
        != RECORD_PLAYER_STATUS_OK) {
        return 8;
    }
    if (gesture_result.event.control.hand_contact != 0u) {
        return 9;
    }
    if (record_player_scratch_gesture_destroy(gesture) != RECORD_PLAYER_STATUS_OK) {
        return 10;
    }
    return 0;
}
