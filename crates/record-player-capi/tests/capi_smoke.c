#include "record_player.h"

int main(void) {
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
        return 1;
    }
    RecordPlayerControl base_control = {0};
    base_control.motor_mode = RECORD_PLAYER_MOTOR_OFF;
    base_control.stylus_lowered = 1u;
    base_control.hand_contact_radius_m = 0.12;
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
        return 2;
    }
    if (gesture_result.event.absolute_frame != 2920u
        || gesture_result.event.control.hand_contact != 1u) {
        return 3;
    }
    pointer.source_time_ns = 1000000u;
    pointer.angle_rad = 0.02;
    if (record_player_scratch_gesture_update(
            gesture, &pointer, 1000u, 2u, &base_control, &gesture_result)
        != RECORD_PLAYER_STATUS_OK) {
        return 4;
    }
    if (gesture_result.event.control.hand_target_angular_velocity_rad_s <= 0.0) {
        return 5;
    }
    if (record_player_scratch_gesture_finish(
            gesture, 7u, 2000000u, 1000u, 3u, &base_control, &gesture_result)
        != RECORD_PLAYER_STATUS_OK) {
        return 6;
    }
    if (gesture_result.event.control.hand_contact != 0u
        || record_player_scratch_gesture_destroy(gesture) != RECORD_PLAYER_STATUS_OK) {
        return 7;
    }

    RecordPlayerCreateOptions options = {
        .abi_version = RECORD_PLAYER_CAPI_ABI_VERSION,
        .output_sample_rate_hz = 48000u,
        .control_mailbox_capacity = 8u,
        .reserved = 0u,
        .volts_per_full_scale = 10.0,
    };
    RecordPlayerHandle *handle = NULL;
    if (record_player_create(&options, &handle) != RECORD_PLAYER_STATUS_OK) {
        return 8;
    }

    RecordPlayerSchedulePoint point;
    if (record_player_schedule_point_after_host_frames(handle, 0u, &point)
        != RECORD_PLAYER_STATUS_OK) {
        return 9;
    }

    RecordPlayerTimedControl event = {0};
    event.absolute_frame = point.absolute_internal_frame;
    event.sequence = 1u;
    event.control.motor_mode = RECORD_PLAYER_MOTOR_OFF;
    if (record_player_submit_timed_control(handle, &event)
        != RECORD_PLAYER_STATUS_OK) {
        return 10;
    }

    float output[128] = {0};
    RecordPlayerRenderReport report;
    if (record_player_render_interleaved(handle, output, 64u, &report)
        != RECORD_PLAYER_STATUS_OK) {
        return 11;
    }
    if (report.control_ingress.enqueued_events != 1u) {
        return 12;
    }

    RecordPlayerTelemetry telemetry;
    if (record_player_get_telemetry(handle, &telemetry)
        != RECORD_PLAYER_STATUS_OK) {
        return 13;
    }
    if (telemetry.rendered_host_frames != 64u) {
        return 14;
    }

    if (record_player_destroy(handle) != RECORD_PLAYER_STATUS_OK) {
        return 15;
    }
    return 0;
}
