use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::mechanics::{
    DeckMechanicalControl, PhysicalDeckConfig, MAXIMUM_DECK_RATE, MAXIMUM_HAND_CONTACT_RADIUS_M,
    MAXIMUM_HAND_NORMAL_FORCE_N,
};
use crate::timed_control::{PlayerControl, TimedPlayerControl};

const SNAPSHOT_VERSION: u32 = 1;
const NANOSECONDS_PER_SECOND: u128 = 1_000_000_000;
const MAXIMUM_SAMPLE_RATE_HZ: u32 = 768_000;

/// Maps a normalized pointer-pressure value to a normal hand force.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchPressureCalibration {
    pub zero_pressure_force_n: f64,
    pub unit_pressure_force_n: f64,
    pub unreported_pressure_force_n: f64,
}

impl ScratchPressureCalibration {
    pub fn validate(self) -> Result<Self, ScratchGestureError> {
        validate_force("zeroPressureForceN", self.zero_pressure_force_n)?;
        validate_force("unitPressureForceN", self.unit_pressure_force_n)?;
        validate_force("unreportedPressureForceN", self.unreported_pressure_force_n)?;
        if self.unit_pressure_force_n < self.zero_pressure_force_n {
            return Err(ScratchGestureError::InvalidConfig {
                field: "unitPressureForceN",
            });
        }
        Ok(self)
    }

    pub fn normal_force_n(
        self,
        normalized_pressure: Option<f64>,
    ) -> Result<f64, ScratchGestureError> {
        self.validate()?;
        match normalized_pressure {
            Some(value) if value.is_finite() && (0.0..=1.0).contains(&value) => Ok(self
                .zero_pressure_force_n
                + value * (self.unit_pressure_force_n - self.zero_pressure_force_n)),
            Some(_) => Err(ScratchGestureError::InvalidSample {
                field: "normalizedPressure",
            }),
            None => Ok(self.unreported_pressure_force_n),
        }
    }
}

impl Default for ScratchPressureCalibration {
    fn default() -> Self {
        Self {
            zero_pressure_force_n: 0.5,
            unit_pressure_force_n: 5.0,
            unreported_pressure_force_n: 2.5,
        }
    }
}

/// Configures sample-clock scheduling and pointer calibration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchGestureConfig {
    pub internal_sample_rate_hz: u32,
    pub lookahead_frames: u32,
    pub deck: PhysicalDeckConfig,
    pub pressure: ScratchPressureCalibration,
}

impl ScratchGestureConfig {
    pub fn for_deck(
        deck: PhysicalDeckConfig,
        internal_sample_rate_hz: u32,
        lookahead_frames: u32,
    ) -> Result<Self, ScratchGestureError> {
        deck.validate()
            .map_err(|_| ScratchGestureError::InvalidConfig { field: "deck" })?;
        Self {
            internal_sample_rate_hz,
            lookahead_frames,
            deck,
            pressure: ScratchPressureCalibration::default(),
        }
        .validate()
    }

    pub fn validate(self) -> Result<Self, ScratchGestureError> {
        if self.internal_sample_rate_hz == 0
            || self.internal_sample_rate_hz > MAXIMUM_SAMPLE_RATE_HZ
        {
            return Err(ScratchGestureError::InvalidConfig {
                field: "internalSampleRateHz",
            });
        }
        self.deck
            .validate()
            .map_err(|_| ScratchGestureError::InvalidConfig { field: "deck" })?;
        self.pressure.validate()?;
        Ok(self)
    }

    pub fn maximum_hand_angular_velocity_rad_s(self) -> f64 {
        MAXIMUM_DECK_RATE * self.deck.nominal_angular_velocity_rad_s()
    }
}

/// Describes one pointer sample in the host's monotonic time domain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchPointerSample {
    pub pointer_id: u64,
    pub source_time_ns: u64,
    pub angle_rad: f64,
    pub contact_radius_m: f64,
    pub normalized_pressure: Option<f64>,
}

impl ScratchPointerSample {
    fn validate(self, pressure: ScratchPressureCalibration) -> Result<Self, ScratchGestureError> {
        if !self.angle_rad.is_finite() {
            return Err(ScratchGestureError::InvalidSample { field: "angleRad" });
        }
        if !self.contact_radius_m.is_finite()
            || self.contact_radius_m <= 0.0
            || self.contact_radius_m > MAXIMUM_HAND_CONTACT_RADIUS_M
        {
            return Err(ScratchGestureError::InvalidSample {
                field: "contactRadiusM",
            });
        }
        pressure.normal_force_n(self.normalized_pressure)?;
        Ok(self)
    }
}

/// Contains one hand control that is ready for the player timeline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledScratchHandControl {
    pub absolute_frame: u64,
    pub pointer_id: u64,
    pub hand_contact: bool,
    pub hand_target_angle_rad: Option<f64>,
    pub hand_target_angular_velocity_rad_s: f64,
    pub hand_normal_force_n: f64,
    pub hand_contact_radius_m: f64,
    pub raw_pointer_angular_velocity_rad_s: f64,
    pub velocity_was_limited: bool,
    pub wrap_was_ambiguous: bool,
    pub added_late_shift_frames: u64,
    pub total_late_shift_frames: u64,
}

impl ScheduledScratchHandControl {
    /// Replaces only the hand fields in a complete player control.
    pub fn merge(self, sequence: u64, mut control: PlayerControl) -> TimedPlayerControl {
        control.deck = self.merge_deck(control.deck);
        TimedPlayerControl::new(self.absolute_frame, sequence, control)
    }

    /// Replaces only the hand fields in a deck control.
    pub fn merge_deck(self, mut control: DeckMechanicalControl) -> DeckMechanicalControl {
        control.hand_contact = self.hand_contact;
        control.hand_target_angle_rad = self.hand_target_angle_rad;
        control.hand_target_angular_velocity_rad_s = self.hand_target_angular_velocity_rad_s;
        control.hand_normal_force_n = self.hand_normal_force_n;
        control.hand_contact_radius_m = self.hand_contact_radius_m;
        control
    }
}

/// Stores all gesture state that can affect later controls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchGestureSnapshot {
    version: u32,
    config: ScratchGestureConfig,
    active_pointer_id: Option<u64>,
    anchor_source_time_ns: u64,
    anchor_schedule_frame: u64,
    last_source_time_ns: u64,
    last_pointer_angle_rad: f64,
    unwrapped_pointer_delta_rad: f64,
    record_angle_at_begin_rad: f64,
    last_scheduled_frame: u64,
    total_late_shift_frames: u64,
}

/// Converts pointer motion to physical hand setpoints.
#[derive(Debug, Clone, PartialEq)]
pub struct ScratchGestureMapper {
    config: ScratchGestureConfig,
    active_pointer_id: Option<u64>,
    anchor_source_time_ns: u64,
    anchor_schedule_frame: u64,
    last_source_time_ns: u64,
    last_pointer_angle_rad: f64,
    unwrapped_pointer_delta_rad: f64,
    record_angle_at_begin_rad: f64,
    last_scheduled_frame: u64,
    total_late_shift_frames: u64,
}

impl ScratchGestureMapper {
    pub fn new(config: ScratchGestureConfig) -> Result<Self, ScratchGestureError> {
        let config = config.validate()?;
        Ok(Self {
            config,
            active_pointer_id: None,
            anchor_source_time_ns: 0,
            anchor_schedule_frame: 0,
            last_source_time_ns: 0,
            last_pointer_angle_rad: 0.0,
            unwrapped_pointer_delta_rad: 0.0,
            record_angle_at_begin_rad: 0.0,
            last_scheduled_frame: 0,
            total_late_shift_frames: 0,
        })
    }

    pub const fn config(&self) -> ScratchGestureConfig {
        self.config
    }

    pub const fn active_pointer_id(&self) -> Option<u64> {
        self.active_pointer_id
    }

    pub const fn total_late_shift_frames(&self) -> u64 {
        self.total_late_shift_frames
    }

    /// Starts a gesture without moving the record at contact time.
    pub fn begin(
        &mut self,
        sample: ScratchPointerSample,
        minimum_render_frame: u64,
        record_angle_rad: f64,
    ) -> Result<ScheduledScratchHandControl, ScratchGestureError> {
        if self.active_pointer_id.is_some() {
            return Err(ScratchGestureError::GestureAlreadyActive);
        }
        let sample = sample.validate(self.config.pressure)?;
        if !record_angle_rad.is_finite() {
            return Err(ScratchGestureError::InvalidRecordAngle);
        }
        let frame = minimum_render_frame
            .checked_add(u64::from(self.config.lookahead_frames))
            .ok_or(ScratchGestureError::ScheduleOverflow)?;
        let normal_force_n = self
            .config
            .pressure
            .normal_force_n(sample.normalized_pressure)?;
        self.active_pointer_id = Some(sample.pointer_id);
        self.anchor_source_time_ns = sample.source_time_ns;
        self.anchor_schedule_frame = frame;
        self.last_source_time_ns = sample.source_time_ns;
        self.last_pointer_angle_rad = sample.angle_rad;
        self.unwrapped_pointer_delta_rad = 0.0;
        self.record_angle_at_begin_rad = record_angle_rad;
        self.last_scheduled_frame = frame;
        self.total_late_shift_frames = 0;
        Ok(ScheduledScratchHandControl {
            absolute_frame: frame,
            pointer_id: sample.pointer_id,
            hand_contact: true,
            hand_target_angle_rad: Some(record_angle_rad),
            hand_target_angular_velocity_rad_s: 0.0,
            hand_normal_force_n: normal_force_n,
            hand_contact_radius_m: sample.contact_radius_m,
            raw_pointer_angular_velocity_rad_s: 0.0,
            velocity_was_limited: false,
            wrap_was_ambiguous: false,
            added_late_shift_frames: 0,
            total_late_shift_frames: 0,
        })
    }

    /// Maps one move without low-pass filtering a reversal.
    pub fn update(
        &mut self,
        sample: ScratchPointerSample,
        minimum_render_frame: u64,
    ) -> Result<ScheduledScratchHandControl, ScratchGestureError> {
        self.validate_active_sample(sample)?;
        let sample = sample.validate(self.config.pressure)?;
        if sample.source_time_ns < self.last_source_time_ns {
            return Err(ScratchGestureError::SourceTimeMovedBackward);
        }

        let elapsed_ns = sample.source_time_ns - self.last_source_time_ns;
        let delta_angle_rad = wrapped_delta(sample.angle_rad - self.last_pointer_angle_rad);
        let raw_velocity = if elapsed_ns == 0 {
            0.0
        } else {
            delta_angle_rad * NANOSECONDS_PER_SECOND as f64 / elapsed_ns as f64
        };
        let maximum_velocity = self.config.maximum_hand_angular_velocity_rad_s();
        let limited_velocity = raw_velocity.clamp(-maximum_velocity, maximum_velocity);
        let wrap_was_ambiguous = elapsed_ns > 0
            && maximum_velocity * elapsed_ns as f64 / NANOSECONDS_PER_SECOND as f64
                >= std::f64::consts::PI;
        let (absolute_frame, added_late_shift_frames) =
            self.schedule_frame(sample.source_time_ns, minimum_render_frame)?;
        let normal_force_n = self
            .config
            .pressure
            .normal_force_n(sample.normalized_pressure)?;

        self.last_source_time_ns = sample.source_time_ns;
        self.last_pointer_angle_rad = sample.angle_rad;
        self.unwrapped_pointer_delta_rad += delta_angle_rad;
        self.last_scheduled_frame = absolute_frame;

        Ok(ScheduledScratchHandControl {
            absolute_frame,
            pointer_id: sample.pointer_id,
            hand_contact: true,
            hand_target_angle_rad: Some(
                self.record_angle_at_begin_rad + self.unwrapped_pointer_delta_rad,
            ),
            hand_target_angular_velocity_rad_s: limited_velocity,
            hand_normal_force_n: normal_force_n,
            hand_contact_radius_m: sample.contact_radius_m,
            raw_pointer_angular_velocity_rad_s: raw_velocity,
            velocity_was_limited: limited_velocity != raw_velocity,
            wrap_was_ambiguous,
            added_late_shift_frames,
            total_late_shift_frames: self.total_late_shift_frames,
        })
    }

    /// Schedules release and makes the mapper available for another pointer.
    pub fn finish(
        &mut self,
        pointer_id: u64,
        source_time_ns: u64,
        minimum_render_frame: u64,
    ) -> Result<ScheduledScratchHandControl, ScratchGestureError> {
        self.validate_pointer(pointer_id)?;
        if source_time_ns < self.last_source_time_ns {
            return Err(ScratchGestureError::SourceTimeMovedBackward);
        }
        let (absolute_frame, added_late_shift_frames) =
            self.schedule_frame(source_time_ns, minimum_render_frame)?;
        let contact_radius_m = MAXIMUM_HAND_CONTACT_RADIUS_M.min(0.12);
        self.active_pointer_id = None;
        self.last_source_time_ns = source_time_ns;
        self.last_scheduled_frame = absolute_frame;
        Ok(ScheduledScratchHandControl {
            absolute_frame,
            pointer_id,
            hand_contact: false,
            hand_target_angle_rad: None,
            hand_target_angular_velocity_rad_s: 0.0,
            hand_normal_force_n: 0.0,
            hand_contact_radius_m: contact_radius_m,
            raw_pointer_angular_velocity_rad_s: 0.0,
            velocity_was_limited: false,
            wrap_was_ambiguous: false,
            added_late_shift_frames,
            total_late_shift_frames: self.total_late_shift_frames,
        })
    }

    pub fn snapshot(&self) -> ScratchGestureSnapshot {
        ScratchGestureSnapshot {
            version: SNAPSHOT_VERSION,
            config: self.config,
            active_pointer_id: self.active_pointer_id,
            anchor_source_time_ns: self.anchor_source_time_ns,
            anchor_schedule_frame: self.anchor_schedule_frame,
            last_source_time_ns: self.last_source_time_ns,
            last_pointer_angle_rad: self.last_pointer_angle_rad,
            unwrapped_pointer_delta_rad: self.unwrapped_pointer_delta_rad,
            record_angle_at_begin_rad: self.record_angle_at_begin_rad,
            last_scheduled_frame: self.last_scheduled_frame,
            total_late_shift_frames: self.total_late_shift_frames,
        }
    }

    pub fn restore(
        &mut self,
        snapshot: &ScratchGestureSnapshot,
    ) -> Result<(), ScratchGestureError> {
        validate_snapshot(snapshot, self.config)?;
        self.active_pointer_id = snapshot.active_pointer_id;
        self.anchor_source_time_ns = snapshot.anchor_source_time_ns;
        self.anchor_schedule_frame = snapshot.anchor_schedule_frame;
        self.last_source_time_ns = snapshot.last_source_time_ns;
        self.last_pointer_angle_rad = snapshot.last_pointer_angle_rad;
        self.unwrapped_pointer_delta_rad = snapshot.unwrapped_pointer_delta_rad;
        self.record_angle_at_begin_rad = snapshot.record_angle_at_begin_rad;
        self.last_scheduled_frame = snapshot.last_scheduled_frame;
        self.total_late_shift_frames = snapshot.total_late_shift_frames;
        Ok(())
    }

    fn validate_active_sample(
        &self,
        sample: ScratchPointerSample,
    ) -> Result<(), ScratchGestureError> {
        self.validate_pointer(sample.pointer_id)
    }

    fn validate_pointer(&self, pointer_id: u64) -> Result<(), ScratchGestureError> {
        match self.active_pointer_id {
            None => Err(ScratchGestureError::GestureNotActive),
            Some(active_pointer_id) if active_pointer_id != pointer_id => {
                Err(ScratchGestureError::PointerMismatch {
                    active_pointer_id,
                    received_pointer_id: pointer_id,
                })
            }
            Some(_) => Ok(()),
        }
    }

    fn schedule_frame(
        &mut self,
        source_time_ns: u64,
        minimum_render_frame: u64,
    ) -> Result<(u64, u64), ScratchGestureError> {
        let elapsed_ns = source_time_ns
            .checked_sub(self.anchor_source_time_ns)
            .ok_or(ScratchGestureError::SourceTimeMovedBackward)?;
        let elapsed_frames =
            nanoseconds_to_frames(elapsed_ns, self.config.internal_sample_rate_hz)?;
        let nominal_frame = self
            .anchor_schedule_frame
            .checked_add(elapsed_frames)
            .and_then(|value| value.checked_add(self.total_late_shift_frames))
            .ok_or(ScratchGestureError::ScheduleOverflow)?;
        let minimum_frame = minimum_render_frame
            .checked_add(u64::from(self.config.lookahead_frames))
            .ok_or(ScratchGestureError::ScheduleOverflow)?;
        let added_late_shift_frames = minimum_frame.saturating_sub(nominal_frame);
        self.total_late_shift_frames = self
            .total_late_shift_frames
            .checked_add(added_late_shift_frames)
            .ok_or(ScratchGestureError::ScheduleOverflow)?;
        let shifted_frame = nominal_frame
            .checked_add(added_late_shift_frames)
            .ok_or(ScratchGestureError::ScheduleOverflow)?;
        Ok((
            shifted_frame.max(self.last_scheduled_frame),
            added_late_shift_frames,
        ))
    }
}

fn nanoseconds_to_frames(
    nanoseconds: u64,
    sample_rate_hz: u32,
) -> Result<u64, ScratchGestureError> {
    let numerator = u128::from(nanoseconds)
        .checked_mul(u128::from(sample_rate_hz))
        .and_then(|value| value.checked_add(NANOSECONDS_PER_SECOND / 2))
        .ok_or(ScratchGestureError::ScheduleOverflow)?;
    u64::try_from(numerator / NANOSECONDS_PER_SECOND)
        .map_err(|_| ScratchGestureError::ScheduleOverflow)
}

fn wrapped_delta(delta_rad: f64) -> f64 {
    delta_rad.sin().atan2(delta_rad.cos())
}

fn validate_force(field: &'static str, value: f64) -> Result<(), ScratchGestureError> {
    if value.is_finite() && (0.0..=MAXIMUM_HAND_NORMAL_FORCE_N).contains(&value) {
        Ok(())
    } else {
        Err(ScratchGestureError::InvalidConfig { field })
    }
}

fn validate_snapshot(
    snapshot: &ScratchGestureSnapshot,
    expected_config: ScratchGestureConfig,
) -> Result<(), ScratchGestureError> {
    if snapshot.version != SNAPSHOT_VERSION {
        return Err(ScratchGestureError::UnsupportedSnapshotVersion {
            version: snapshot.version,
        });
    }
    snapshot.config.validate()?;
    if snapshot.config != expected_config {
        return Err(ScratchGestureError::SnapshotConfigMismatch);
    }
    for (field, value) in [
        ("lastPointerAngleRad", snapshot.last_pointer_angle_rad),
        (
            "unwrappedPointerDeltaRad",
            snapshot.unwrapped_pointer_delta_rad,
        ),
        ("recordAngleAtBeginRad", snapshot.record_angle_at_begin_rad),
    ] {
        if !value.is_finite() {
            return Err(ScratchGestureError::InvalidSnapshot { field });
        }
    }
    if snapshot.active_pointer_id.is_some()
        && snapshot.last_source_time_ns < snapshot.anchor_source_time_ns
    {
        return Err(ScratchGestureError::InvalidSnapshot {
            field: "lastSourceTimeNs",
        });
    }
    if snapshot.active_pointer_id.is_some()
        && snapshot.last_scheduled_frame < snapshot.anchor_schedule_frame
    {
        return Err(ScratchGestureError::InvalidSnapshot {
            field: "lastScheduledFrame",
        });
    }
    Ok(())
}

#[derive(Debug, Error, PartialEq)]
pub enum ScratchGestureError {
    #[error("scratch gesture configuration field {field} is invalid")]
    InvalidConfig { field: &'static str },
    #[error("scratch pointer sample field {field} is invalid")]
    InvalidSample { field: &'static str },
    #[error("record angle must be finite")]
    InvalidRecordAngle,
    #[error("a scratch gesture is already active")]
    GestureAlreadyActive,
    #[error("no scratch gesture is active")]
    GestureNotActive,
    #[error(
        "scratch pointer {received_pointer_id} does not match active pointer {active_pointer_id}"
    )]
    PointerMismatch {
        active_pointer_id: u64,
        received_pointer_id: u64,
    },
    #[error("scratch pointer time moved backward")]
    SourceTimeMovedBackward,
    #[error("scratch gesture schedule overflowed")]
    ScheduleOverflow,
    #[error("scratch gesture snapshot version {version} is not supported")]
    UnsupportedSnapshotVersion { version: u32 },
    #[error("scratch gesture snapshot uses a different configuration")]
    SnapshotConfigMismatch,
    #[error("scratch gesture snapshot field {field} is invalid")]
    InvalidSnapshot { field: &'static str },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mechanics::MotorMode;
    use approx::assert_abs_diff_eq;

    fn config(lookahead_frames: u32) -> ScratchGestureConfig {
        ScratchGestureConfig::for_deck(PhysicalDeckConfig::default(), 192_000, lookahead_frames)
            .unwrap()
    }

    fn sample(pointer_id: u64, time_ns: u64, angle_rad: f64) -> ScratchPointerSample {
        ScratchPointerSample {
            pointer_id,
            source_time_ns: time_ns,
            angle_rad,
            contact_radius_m: 0.12,
            normalized_pressure: Some(0.5),
        }
    }

    #[test]
    fn begin_anchors_the_hand_without_moving_the_record() {
        let mut mapper = ScratchGestureMapper::new(config(1_920)).unwrap();
        let output = mapper
            .begin(sample(7, 1_000_000, 2.75), 30_000, -18.0)
            .unwrap();
        assert_eq!(output.absolute_frame, 31_920);
        assert_eq!(output.hand_target_angle_rad, Some(-18.0));
        assert_eq!(output.hand_target_angular_velocity_rad_s, 0.0);
        assert!(output.hand_contact);
    }

    #[test]
    fn branch_cut_motion_is_unwrapped_in_the_short_direction() {
        let mut mapper = ScratchGestureMapper::new(config(0)).unwrap();
        mapper
            .begin(sample(1, 0, std::f64::consts::PI - 0.02), 0, 5.0)
            .unwrap();
        let output = mapper
            .update(sample(1, 10_000_000, -std::f64::consts::PI + 0.03), 0)
            .unwrap();
        assert_abs_diff_eq!(
            output.hand_target_angle_rad.unwrap(),
            5.05,
            epsilon = 1.0e-12
        );
        assert!(output.hand_target_angular_velocity_rad_s > 0.0);
    }

    #[test]
    fn rapid_reversals_keep_their_sign_and_sample_order() {
        let mut mapper = ScratchGestureMapper::new(config(0)).unwrap();
        mapper.begin(sample(1, 0, 0.0), 100, 0.0).unwrap();
        let mut previous_frame = 100;
        for index in 1..=128_u64 {
            let angle = if index % 2 == 0 { 0.0 } else { 0.02 };
            let output = mapper
                .update(sample(1, index * 1_000_000, angle), 100)
                .unwrap();
            assert!(output.absolute_frame > previous_frame);
            if index % 2 == 0 {
                assert!(output.hand_target_angular_velocity_rad_s < 0.0);
            } else {
                assert!(output.hand_target_angular_velocity_rad_s > 0.0);
            }
            previous_frame = output.absolute_frame;
        }
    }

    #[test]
    fn late_shift_preserves_later_event_intervals() {
        let mut mapper = ScratchGestureMapper::new(config(100)).unwrap();
        mapper.begin(sample(1, 0, 0.0), 1_000, 0.0).unwrap();
        let late = mapper.update(sample(1, 1_000_000, 0.1), 10_000).unwrap();
        let next = mapper.update(sample(1, 2_000_000, 0.0), 10_000).unwrap();
        assert_eq!(late.absolute_frame, 10_100);
        assert_eq!(next.absolute_frame - late.absolute_frame, 192);
        assert!(late.added_late_shift_frames > 0);
        assert_eq!(next.added_late_shift_frames, 0);
    }

    #[test]
    fn excessive_pointer_velocity_is_limited_without_losing_position() {
        let mut mapper = ScratchGestureMapper::new(config(0)).unwrap();
        mapper.begin(sample(1, 0, 0.0), 0, 8.0).unwrap();
        let output = mapper.update(sample(1, 1_000, 0.5), 0).unwrap();
        assert!(output.velocity_was_limited);
        assert_eq!(
            output.hand_target_angular_velocity_rad_s,
            mapper.config.maximum_hand_angular_velocity_rad_s()
        );
        assert_abs_diff_eq!(
            output.hand_target_angle_rad.unwrap(),
            8.5,
            epsilon = 1.0e-12
        );
    }

    #[test]
    fn long_sampling_gap_marks_turn_count_as_ambiguous() {
        let mut mapper = ScratchGestureMapper::new(config(0)).unwrap();
        mapper.begin(sample(1, 0, 0.0), 0, 0.0).unwrap();
        let output = mapper.update(sample(1, 1_000_000_000, 0.1), 0).unwrap();
        assert!(output.wrap_was_ambiguous);
    }

    #[test]
    fn finish_schedules_a_complete_release_control() {
        let mut mapper = ScratchGestureMapper::new(config(200)).unwrap();
        mapper.begin(sample(4, 0, 0.0), 50, 0.0).unwrap();
        let release = mapper.finish(4, 5_000_000, 50).unwrap();
        assert_eq!(release.absolute_frame, 1_210);
        assert!(!release.hand_contact);
        assert_eq!(release.hand_target_angle_rad, None);
        assert_eq!(release.hand_normal_force_n, 0.0);
        assert_eq!(mapper.active_pointer_id(), None);
    }

    #[test]
    fn wrong_pointer_does_not_change_active_state() {
        let mut mapper = ScratchGestureMapper::new(config(0)).unwrap();
        mapper.begin(sample(3, 0, 0.0), 0, 0.0).unwrap();
        let before = mapper.snapshot();
        assert_eq!(
            mapper.update(sample(9, 1_000_000, 0.1), 0),
            Err(ScratchGestureError::PointerMismatch {
                active_pointer_id: 3,
                received_pointer_id: 9,
            })
        );
        assert_eq!(mapper.snapshot(), before);
    }

    #[test]
    fn invalid_update_is_transactional() {
        let mut mapper = ScratchGestureMapper::new(config(0)).unwrap();
        mapper.begin(sample(1, 10, 0.0), 0, 0.0).unwrap();
        let before = mapper.snapshot();
        let mut invalid = sample(1, 20, 0.1);
        invalid.normalized_pressure = Some(f64::NAN);
        assert!(mapper.update(invalid, 0).is_err());
        assert_eq!(mapper.snapshot(), before);
    }

    #[test]
    fn source_time_cannot_move_backward() {
        let mut mapper = ScratchGestureMapper::new(config(0)).unwrap();
        mapper.begin(sample(1, 10, 0.0), 0, 0.0).unwrap();
        assert_eq!(
            mapper.update(sample(1, 9, 0.1), 0),
            Err(ScratchGestureError::SourceTimeMovedBackward)
        );
    }

    #[test]
    fn equal_timestamps_do_not_invent_velocity() {
        let mut mapper = ScratchGestureMapper::new(config(0)).unwrap();
        mapper.begin(sample(1, 10, 0.0), 0, 2.0).unwrap();
        let output = mapper.update(sample(1, 10, 0.2), 0).unwrap();
        assert_eq!(output.hand_target_angular_velocity_rad_s, 0.0);
        assert_abs_diff_eq!(
            output.hand_target_angle_rad.unwrap(),
            2.2,
            epsilon = 1.0e-12
        );
    }

    #[test]
    fn unreported_pressure_uses_the_calibrated_force() {
        let mut mapper = ScratchGestureMapper::new(config(0)).unwrap();
        let mut pointer = sample(1, 0, 0.0);
        pointer.normalized_pressure = None;
        let output = mapper.begin(pointer, 0, 0.0).unwrap();
        assert_eq!(
            output.hand_normal_force_n,
            ScratchPressureCalibration::default().unreported_pressure_force_n
        );
    }

    #[test]
    fn merge_changes_only_hand_fields() {
        let mut mapper = ScratchGestureMapper::new(config(0)).unwrap();
        let output = mapper.begin(sample(1, 0, 0.0), 100, 0.0).unwrap();
        let mut base = PlayerControl::default();
        base.stylus_lowered = true;
        base.deck.motor_mode = MotorMode::Brake;
        base.deck.motor_target_angular_velocity_rad_s = -2.0;
        base.deck.stylus_torque_nm = 0.01;
        let merged = output.merge(77, base);
        assert_eq!(merged.absolute_frame, 100);
        assert_eq!(merged.sequence, 77);
        assert!(merged.control.stylus_lowered);
        assert_eq!(merged.control.deck.motor_mode, MotorMode::Brake);
        assert_eq!(
            merged.control.deck.motor_target_angular_velocity_rad_s,
            -2.0
        );
        assert_eq!(merged.control.deck.stylus_torque_nm, 0.01);
        assert!(merged.control.deck.hand_contact);
    }

    #[test]
    fn snapshot_restore_repeats_the_next_mapping() {
        let mut mapper = ScratchGestureMapper::new(config(100)).unwrap();
        mapper.begin(sample(1, 0, 0.0), 1_000, 3.0).unwrap();
        mapper.update(sample(1, 1_000_000, 0.1), 1_000).unwrap();
        let snapshot = mapper.snapshot();
        let expected = mapper.update(sample(1, 2_000_000, -0.1), 1_000).unwrap();
        mapper.restore(&snapshot).unwrap();
        assert_eq!(
            mapper.update(sample(1, 2_000_000, -0.1), 1_000).unwrap(),
            expected
        );
    }

    #[test]
    fn restore_rejects_a_different_configuration_transactionally() {
        let first = ScratchGestureMapper::new(config(0)).unwrap();
        let snapshot = first.snapshot();
        let mut second = ScratchGestureMapper::new(config(10)).unwrap();
        let before = second.snapshot();
        assert_eq!(
            second.restore(&snapshot),
            Err(ScratchGestureError::SnapshotConfigMismatch)
        );
        assert_eq!(second.snapshot(), before);
    }

    #[test]
    fn invalid_pressure_calibration_is_rejected() {
        let mut invalid = config(0);
        invalid.pressure.unit_pressure_force_n = 0.1;
        invalid.pressure.zero_pressure_force_n = 0.2;
        assert_eq!(
            ScratchGestureMapper::new(invalid),
            Err(ScratchGestureError::InvalidConfig {
                field: "unitPressureForceN"
            })
        );
    }

    #[test]
    fn begin_reports_schedule_overflow_without_starting() {
        let mut mapper = ScratchGestureMapper::new(config(10)).unwrap();
        assert_eq!(
            mapper.begin(sample(1, 0, 0.0), u64::MAX - 5, 0.0),
            Err(ScratchGestureError::ScheduleOverflow)
        );
        assert_eq!(mapper.active_pointer_id(), None);
    }
}
