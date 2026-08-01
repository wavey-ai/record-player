use crate::mechanics::DeckMechanicalControl;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

const SNAPSHOT_VERSION: u32 = 2;
static NEXT_TIMELINE_ID: AtomicUsize = AtomicUsize::new(1);

/// The timeline rejects an incoming event when all slots are in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FullCapacityPolicy {
    RejectIncoming,
}

/// The timeline rejects an event before the current render frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LateEventPolicy {
    Reject,
}

/// The timeline rejects a repeated frame and sequence key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DuplicateEventPolicy {
    Reject,
}

/// These policies do not discard, replace, or combine accepted transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlTimelinePolicy {
    pub full_capacity: FullCapacityPolicy,
    pub late_event: LateEventPolicy,
    pub duplicate_event: DuplicateEventPolicy,
}

impl ControlTimelinePolicy {
    pub const fn lossless() -> Self {
        Self {
            full_capacity: FullCapacityPolicy::RejectIncoming,
            late_event: LateEventPolicy::Reject,
            duplicate_event: DuplicateEventPolicy::Reject,
        }
    }
}

impl Default for ControlTimelinePolicy {
    fn default() -> Self {
        Self::lossless()
    }
}

/// One complete player control state at an exact render frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerControl {
    pub deck: DeckMechanicalControl,
    pub stylus_lowered: bool,
}

impl PlayerControl {
    pub const fn new(deck: DeckMechanicalControl, stylus_lowered: bool) -> Self {
        Self {
            deck,
            stylus_lowered,
        }
    }
}

impl Default for PlayerControl {
    fn default() -> Self {
        Self::new(DeckMechanicalControl::default(), false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedPlayerControl {
    pub absolute_frame: u64,
    pub sequence: u64,
    pub control: PlayerControl,
}

impl TimedPlayerControl {
    pub const fn new(absolute_frame: u64, sequence: u64, control: PlayerControl) -> Self {
        Self {
            absolute_frame,
            sequence,
            control,
        }
    }

    const fn key(self) -> TimedControlKey {
        TimedControlKey {
            absolute_frame: self.absolute_frame,
            sequence: self.sequence,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimedControlKey {
    absolute_frame: u64,
    sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlBlockItem {
    /// Render these frames with one unchanged control value.
    Span {
        frame_offset: u32,
        frame_count: u32,
        control: PlayerControl,
    },
    /// Apply this transition before rendering the frame at `frame_offset`.
    Transition {
        frame_offset: u32,
        event: TimedPlayerControl,
    },
}

impl ControlBlockItem {
    pub const fn frame_offset(self) -> u32 {
        match self {
            Self::Span { frame_offset, .. } | Self::Transition { frame_offset, .. } => frame_offset,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlTimelineSnapshot {
    version: u32,
    capacity: usize,
    policy: ControlTimelinePolicy,
    render_frame: u64,
    current_control: PlayerControl,
    pending_events: Vec<TimedPlayerControl>,
    last_submitted: Option<TimedControlKey>,
}

impl ControlTimelineSnapshot {
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub const fn render_frame(&self) -> u64 {
        self.render_frame
    }

    pub fn pending_events(&self) -> &[TimedPlayerControl] {
        &self.pending_events
    }
}

/// A fixed-size marker for one timeline's logical render state.
///
/// A checkpoint remains valid after event consumption. A successful enqueue or
/// snapshot restore invalidates every earlier checkpoint.
#[derive(Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct PlayerControlTimelineCheckpoint {
    timeline_id: usize,
    slot_revision: u64,
    head: usize,
    len: usize,
    render_frame: u64,
    current_control: PlayerControl,
    last_submitted: Option<TimedControlKey>,
}

/// This queue allocates its complete event storage during construction.
#[derive(Debug)]
pub struct PlayerControlTimeline {
    slots: Box<[Option<TimedPlayerControl>]>,
    timeline_id: usize,
    slot_revision: u64,
    head: usize,
    len: usize,
    policy: ControlTimelinePolicy,
    render_frame: u64,
    current_control: PlayerControl,
    last_submitted: Option<TimedControlKey>,
}

impl PlayerControlTimeline {
    pub fn new(
        capacity: usize,
        render_frame: u64,
        initial_control: PlayerControl,
    ) -> Result<Self, ControlTimelineCreateError> {
        Self::with_policy(
            capacity,
            render_frame,
            initial_control,
            ControlTimelinePolicy::lossless(),
        )
    }

    pub fn with_policy(
        capacity: usize,
        render_frame: u64,
        initial_control: PlayerControl,
        policy: ControlTimelinePolicy,
    ) -> Result<Self, ControlTimelineCreateError> {
        if capacity == 0 {
            return Err(ControlTimelineCreateError::ZeroCapacity);
        }
        validate_control(initial_control.deck)
            .map_err(ControlTimelineCreateError::InvalidControl)?;

        Ok(Self {
            slots: vec![None; capacity].into_boxed_slice(),
            timeline_id: NEXT_TIMELINE_ID.fetch_add(1, Ordering::Relaxed),
            slot_revision: 0,
            head: 0,
            len: 0,
            policy,
            render_frame,
            current_control: initial_control,
            last_submitted: None,
        })
    }

    pub const fn capacity(&self) -> usize {
        self.slots.len()
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub const fn is_full(&self) -> bool {
        self.len == self.slots.len()
    }

    pub const fn policy(&self) -> ControlTimelinePolicy {
        self.policy
    }

    pub const fn render_frame(&self) -> u64 {
        self.render_frame
    }

    pub const fn current_control(&self) -> PlayerControl {
        self.current_control
    }

    pub fn next_event(&self) -> Option<TimedPlayerControl> {
        self.peek()
    }

    /// Captures the logical queue and render state in constant time.
    ///
    /// This function does not allocate. Event consumption does not invalidate
    /// the result.
    pub const fn checkpoint(&self) -> PlayerControlTimelineCheckpoint {
        PlayerControlTimelineCheckpoint {
            timeline_id: self.timeline_id,
            slot_revision: self.slot_revision,
            head: self.head,
            len: self.len,
            render_frame: self.render_frame,
            current_control: self.current_control,
            last_submitted: self.last_submitted,
        }
    }

    /// Restores a checkpoint in constant time.
    ///
    /// This function does not allocate. It rejects a marker from another
    /// timeline. It also rejects a marker after a slot write.
    pub fn restore_checkpoint(
        &mut self,
        checkpoint: PlayerControlTimelineCheckpoint,
    ) -> Result<(), ControlTimelineCheckpointRestoreError> {
        if checkpoint.timeline_id != self.timeline_id {
            return Err(ControlTimelineCheckpointRestoreError::TimelineMismatch);
        }
        if checkpoint.slot_revision != self.slot_revision {
            return Err(ControlTimelineCheckpointRestoreError::SlotsChanged);
        }

        self.head = checkpoint.head;
        self.len = checkpoint.len;
        self.render_frame = checkpoint.render_frame;
        self.current_control = checkpoint.current_control;
        self.last_submitted = checkpoint.last_submitted;
        Ok(())
    }

    /// Returns the next transition inside the half-open render block.
    pub fn next_transition_offset(&self, block_frame_count: u32) -> Option<u32> {
        let event = self.peek()?;
        let offset = event.absolute_frame.checked_sub(self.render_frame)?;
        (offset < u64::from(block_frame_count)).then_some(offset as u32)
    }

    /// Adds one event without allocating or changing an accepted event.
    pub fn enqueue(&mut self, event: TimedPlayerControl) -> Result<(), ControlTimelinePushError> {
        validate_control(event.control.deck).map_err(ControlTimelinePushError::InvalidControl)?;

        let key = event.key();
        if self.last_submitted == Some(key) {
            return Err(ControlTimelinePushError::Duplicate {
                absolute_frame: key.absolute_frame,
                sequence: key.sequence,
            });
        }
        if event.absolute_frame < self.render_frame {
            return Err(ControlTimelinePushError::Late {
                absolute_frame: event.absolute_frame,
                render_frame: self.render_frame,
            });
        }
        if let Some(previous) = self.last_submitted {
            if event.absolute_frame < previous.absolute_frame {
                return Err(ControlTimelinePushError::NonMonotonicFrame {
                    previous: previous.absolute_frame,
                    incoming: event.absolute_frame,
                });
            }
            if event.sequence <= previous.sequence {
                return Err(ControlTimelinePushError::NonMonotonicSequence {
                    previous: previous.sequence,
                    incoming: event.sequence,
                });
            }
        }
        if self.is_full() {
            return Err(ControlTimelinePushError::Full {
                capacity: self.capacity(),
            });
        }

        let tail = (self.head + self.len) % self.capacity();
        // A consumed event can remain here. The logical tail owns this slot.
        self.slots[tail] = Some(event);
        self.slot_revision = self.slot_revision.wrapping_add(1);
        self.len += 1;
        self.last_submitted = Some(key);
        Ok(())
    }

    /// Visits every transition and render span in one half-open block.
    ///
    /// Events at the block end remain pending for the next block.
    pub fn visit_block(
        &mut self,
        block_frame_count: u32,
        mut visit: impl FnMut(ControlBlockItem),
    ) -> Result<(), ControlTimelineAdvanceError> {
        let block_start = self.render_frame;
        let block_end = block_start
            .checked_add(u64::from(block_frame_count))
            .ok_or(ControlTimelineAdvanceError::FrameOverflow)?;
        let mut span_start = block_start;

        while let Some(event) = self.peek() {
            if event.absolute_frame >= block_end {
                break;
            }
            debug_assert!(event.absolute_frame >= span_start);

            if event.absolute_frame > span_start {
                visit(ControlBlockItem::Span {
                    frame_offset: (span_start - block_start) as u32,
                    frame_count: (event.absolute_frame - span_start) as u32,
                    control: self.current_control,
                });
                span_start = event.absolute_frame;
            }

            let event = self.pop().expect("the queue head must exist");
            self.current_control = event.control;
            visit(ControlBlockItem::Transition {
                frame_offset: (event.absolute_frame - block_start) as u32,
                event,
            });
        }

        if span_start < block_end {
            visit(ControlBlockItem::Span {
                frame_offset: (span_start - block_start) as u32,
                frame_count: (block_end - span_start) as u32,
                control: self.current_control,
            });
        }
        self.render_frame = block_end;
        Ok(())
    }

    pub fn snapshot(&self) -> ControlTimelineSnapshot {
        let mut pending_events = Vec::with_capacity(self.len);
        for logical_index in 0..self.len {
            let slot = (self.head + logical_index) % self.capacity();
            pending_events
                .push(self.slots[slot].expect("an occupied queue slot must contain an event"));
        }
        ControlTimelineSnapshot {
            version: SNAPSHOT_VERSION,
            capacity: self.capacity(),
            policy: self.policy,
            render_frame: self.render_frame,
            current_control: self.current_control,
            pending_events,
            last_submitted: self.last_submitted,
        }
    }

    /// Restore validates all data before it changes the current timeline.
    pub fn restore(
        &mut self,
        snapshot: &ControlTimelineSnapshot,
    ) -> Result<(), ControlTimelineRestoreError> {
        validate_snapshot(snapshot, self.capacity())?;

        self.slots.fill(None);
        self.head = 0;
        self.len = snapshot.pending_events.len();
        for (slot, event) in self
            .slots
            .iter_mut()
            .zip(snapshot.pending_events.iter().copied())
        {
            *slot = Some(event);
        }
        self.slot_revision = self.slot_revision.wrapping_add(1);
        self.policy = snapshot.policy;
        self.render_frame = snapshot.render_frame;
        self.current_control = snapshot.current_control;
        self.last_submitted = snapshot.last_submitted;
        Ok(())
    }

    pub fn from_snapshot(
        snapshot: &ControlTimelineSnapshot,
    ) -> Result<Self, ControlTimelineRestoreError> {
        validate_snapshot(snapshot, snapshot.capacity)?;
        let mut timeline = Self::with_policy(
            snapshot.capacity,
            snapshot.render_frame,
            snapshot.current_control,
            snapshot.policy,
        )
        .map_err(|_| ControlTimelineRestoreError::InvalidSnapshot)?;
        timeline.restore(snapshot)?;
        Ok(timeline)
    }

    fn peek(&self) -> Option<TimedPlayerControl> {
        (self.len > 0).then(|| self.slots[self.head]).flatten()
    }

    fn pop(&mut self) -> Option<TimedPlayerControl> {
        if self.len == 0 {
            return None;
        }
        // Keep the value so a render checkpoint can restore this queue view.
        let event = self.slots[self.head];
        self.head = (self.head + 1) % self.capacity();
        self.len -= 1;
        event
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlValueRequirement {
    Finite,
    Nonnegative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidControlValue {
    pub field: &'static str,
    pub requirement: ControlValueRequirement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlTimelineCreateError {
    ZeroCapacity,
    InvalidControl(InvalidControlValue),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlTimelinePushError {
    InvalidControl(InvalidControlValue),
    Late {
        absolute_frame: u64,
        render_frame: u64,
    },
    Duplicate {
        absolute_frame: u64,
        sequence: u64,
    },
    NonMonotonicFrame {
        previous: u64,
        incoming: u64,
    },
    NonMonotonicSequence {
        previous: u64,
        incoming: u64,
    },
    Full {
        capacity: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlTimelineAdvanceError {
    FrameOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlTimelineCheckpointRestoreError {
    TimelineMismatch,
    SlotsChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlTimelineRestoreError {
    UnsupportedVersion { version: u32 },
    CapacityMismatch { expected: usize, actual: usize },
    InvalidSnapshot,
}

impl fmt::Display for ControlTimelineCreateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => formatter.write_str("control timeline capacity must be positive"),
            Self::InvalidControl(value) => write_invalid_control(formatter, *value),
        }
    }
}

impl Error for ControlTimelineCreateError {}

impl fmt::Display for ControlTimelinePushError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidControl(value) => write_invalid_control(formatter, *value),
            Self::Late {
                absolute_frame,
                render_frame,
            } => write!(
                formatter,
                "control event frame {absolute_frame} is before render frame {render_frame}"
            ),
            Self::Duplicate {
                absolute_frame,
                sequence,
            } => write!(
                formatter,
                "control event frame {absolute_frame} and sequence {sequence} are duplicates"
            ),
            Self::NonMonotonicFrame { previous, incoming } => write!(
                formatter,
                "control event frame {incoming} is before submitted frame {previous}"
            ),
            Self::NonMonotonicSequence { previous, incoming } => write!(
                formatter,
                "control event sequence {incoming} does not follow sequence {previous}"
            ),
            Self::Full { capacity } => {
                write!(formatter, "control timeline capacity {capacity} is full")
            }
        }
    }
}

impl Error for ControlTimelinePushError {}

impl fmt::Display for ControlTimelineAdvanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameOverflow => formatter.write_str("control timeline frame overflow"),
        }
    }
}

impl Error for ControlTimelineAdvanceError {}

impl fmt::Display for ControlTimelineCheckpointRestoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimelineMismatch => {
                formatter.write_str("checkpoint belongs to a different control timeline")
            }
            Self::SlotsChanged => {
                formatter.write_str("control timeline slots changed after the checkpoint")
            }
        }
    }
}

impl Error for ControlTimelineCheckpointRestoreError {}

impl fmt::Display for ControlTimelineRestoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion { version } => {
                write!(
                    formatter,
                    "unsupported control timeline snapshot version {version}"
                )
            }
            Self::CapacityMismatch { expected, actual } => write!(
                formatter,
                "control timeline snapshot capacity {actual} does not match {expected}"
            ),
            Self::InvalidSnapshot => formatter.write_str("invalid control timeline snapshot"),
        }
    }
}

impl Error for ControlTimelineRestoreError {}

fn write_invalid_control(
    formatter: &mut fmt::Formatter<'_>,
    value: InvalidControlValue,
) -> fmt::Result {
    let requirement = match value.requirement {
        ControlValueRequirement::Finite => "finite",
        ControlValueRequirement::Nonnegative => "nonnegative",
    };
    write!(
        formatter,
        "control field {} must be {requirement}",
        value.field
    )
}

fn validate_control(control: DeckMechanicalControl) -> Result<(), InvalidControlValue> {
    validate_finite(
        "motorTargetAngularVelocityRadS",
        control.motor_target_angular_velocity_rad_s,
    )?;
    if let Some(angle) = control.hand_target_angle_rad {
        validate_finite("handTargetAngleRad", angle)?;
    }
    validate_finite(
        "handTargetAngularVelocityRadS",
        control.hand_target_angular_velocity_rad_s,
    )?;
    validate_nonnegative("handNormalForceN", control.hand_normal_force_n)?;
    validate_nonnegative("handContactRadiusM", control.hand_contact_radius_m)?;
    validate_finite("stylusTorqueNm", control.stylus_torque_nm)?;
    Ok(())
}

fn validate_finite(field: &'static str, value: f64) -> Result<(), InvalidControlValue> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(InvalidControlValue {
            field,
            requirement: ControlValueRequirement::Finite,
        })
    }
}

fn validate_nonnegative(field: &'static str, value: f64) -> Result<(), InvalidControlValue> {
    validate_finite(field, value)?;
    if value >= 0.0 {
        Ok(())
    } else {
        Err(InvalidControlValue {
            field,
            requirement: ControlValueRequirement::Nonnegative,
        })
    }
}

fn validate_snapshot(
    snapshot: &ControlTimelineSnapshot,
    expected_capacity: usize,
) -> Result<(), ControlTimelineRestoreError> {
    if snapshot.version != SNAPSHOT_VERSION {
        return Err(ControlTimelineRestoreError::UnsupportedVersion {
            version: snapshot.version,
        });
    }
    if snapshot.capacity != expected_capacity {
        return Err(ControlTimelineRestoreError::CapacityMismatch {
            expected: expected_capacity,
            actual: snapshot.capacity,
        });
    }
    if snapshot.capacity == 0 || snapshot.pending_events.len() > snapshot.capacity {
        return Err(ControlTimelineRestoreError::InvalidSnapshot);
    }
    if validate_control(snapshot.current_control.deck).is_err() {
        return Err(ControlTimelineRestoreError::InvalidSnapshot);
    }

    let mut previous: Option<TimedControlKey> = None;
    for event in snapshot.pending_events.iter().copied() {
        if validate_control(event.control.deck).is_err()
            || event.absolute_frame < snapshot.render_frame
        {
            return Err(ControlTimelineRestoreError::InvalidSnapshot);
        }
        if let Some(previous) = previous {
            if event.absolute_frame < previous.absolute_frame || event.sequence <= previous.sequence
            {
                return Err(ControlTimelineRestoreError::InvalidSnapshot);
            }
        }
        previous = Some(event.key());
    }

    match (previous, snapshot.last_submitted) {
        (Some(tail), Some(last)) if tail == last => {}
        (None, Some(last)) if last.absolute_frame < snapshot.render_frame => {}
        (None, None) => {}
        _ => return Err(ControlTimelineRestoreError::InvalidSnapshot),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mechanics::MotorMode;

    fn control(rate: f64) -> PlayerControl {
        PlayerControl::new(
            DeckMechanicalControl {
                motor_mode: MotorMode::Servo,
                motor_target_angular_velocity_rad_s: rate,
                hand_contact: rate != 0.0,
                hand_target_angle_rad: None,
                hand_target_angular_velocity_rad_s: rate,
                hand_normal_force_n: rate.abs(),
                hand_contact_radius_m: 0.12,
                stylus_torque_nm: 0.0,
            },
            rate.is_sign_positive(),
        )
    }

    #[test]
    fn partitions_block_at_exact_event_frames() {
        let mut timeline = PlayerControlTimeline::new(8, 1_000, control(0.0)).unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(1_003, 1, control(1.0)))
            .unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(1_007, 2, control(-1.0)))
            .unwrap();

        assert_eq!(timeline.next_transition_offset(8), Some(3));
        let mut items = Vec::new();
        timeline.visit_block(10, |item| items.push(item)).unwrap();

        assert_eq!(
            items,
            vec![
                ControlBlockItem::Span {
                    frame_offset: 0,
                    frame_count: 3,
                    control: control(0.0),
                },
                ControlBlockItem::Transition {
                    frame_offset: 3,
                    event: TimedPlayerControl::new(1_003, 1, control(1.0)),
                },
                ControlBlockItem::Span {
                    frame_offset: 3,
                    frame_count: 4,
                    control: control(1.0),
                },
                ControlBlockItem::Transition {
                    frame_offset: 7,
                    event: TimedPlayerControl::new(1_007, 2, control(-1.0)),
                },
                ControlBlockItem::Span {
                    frame_offset: 7,
                    frame_count: 3,
                    control: control(-1.0),
                },
            ]
        );
        assert_eq!(timeline.render_frame(), 1_010);
        assert_eq!(timeline.current_control(), control(-1.0));
    }

    #[test]
    fn preserves_several_transitions_in_one_quantum() {
        let mut timeline = PlayerControlTimeline::new(8, 200, control(0.0)).unwrap();
        for (frame, sequence, rate) in [
            (200, 10, 1.0),
            (200, 11, -1.0),
            (201, 12, 0.5),
            (205, 13, -0.5),
        ] {
            timeline
                .enqueue(TimedPlayerControl::new(frame, sequence, control(rate)))
                .unwrap();
        }

        let mut transitions = Vec::new();
        timeline
            .visit_block(8, |item| {
                if let ControlBlockItem::Transition {
                    frame_offset,
                    event,
                } = item
                {
                    transitions.push((frame_offset, event.sequence));
                }
            })
            .unwrap();

        assert_eq!(transitions, vec![(0, 10), (0, 11), (1, 12), (5, 13)]);
        assert!(timeline.is_empty());
    }

    #[test]
    fn preserves_stylus_transitions_at_one_frame() {
        let initial = PlayerControl::new(control(0.0).deck, true);
        let mut lifted = initial;
        lifted.stylus_lowered = false;
        let mut lowered = lifted;
        lowered.stylus_lowered = true;
        let mut timeline = PlayerControlTimeline::new(4, 300, initial).unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(302, 1, lifted))
            .unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(302, 2, lowered))
            .unwrap();

        let mut observed = Vec::new();
        timeline
            .visit_block(4, |item| {
                if let ControlBlockItem::Transition { event, .. } = item {
                    observed.push(event.control.stylus_lowered);
                }
            })
            .unwrap();

        assert_eq!(observed, [false, true]);
        assert!(timeline.current_control().stylus_lowered);
    }

    #[test]
    fn event_at_block_end_starts_the_next_block() {
        let mut timeline = PlayerControlTimeline::new(2, 50, control(0.0)).unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(58, 1, control(1.0)))
            .unwrap();
        assert_eq!(timeline.next_transition_offset(8), None);

        let mut first = Vec::new();
        timeline.visit_block(8, |item| first.push(item)).unwrap();
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline.next_transition_offset(1), Some(0));

        let mut second = Vec::new();
        timeline.visit_block(1, |item| second.push(item)).unwrap();
        assert!(matches!(
            second.first(),
            Some(ControlBlockItem::Transition {
                frame_offset: 0,
                ..
            })
        ));
    }

    #[test]
    fn rejects_late_event_without_changing_queue() {
        let mut timeline = PlayerControlTimeline::new(2, 500, control(0.0)).unwrap();
        let error = timeline
            .enqueue(TimedPlayerControl::new(499, 1, control(1.0)))
            .unwrap_err();
        assert_eq!(
            error,
            ControlTimelinePushError::Late {
                absolute_frame: 499,
                render_frame: 500,
            }
        );
        assert!(timeline.is_empty());
    }

    #[test]
    fn rejects_new_event_at_full_capacity() {
        let mut timeline = PlayerControlTimeline::new(2, 0, control(0.0)).unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(1, 1, control(1.0)))
            .unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(2, 2, control(-1.0)))
            .unwrap();

        assert_eq!(
            timeline
                .enqueue(TimedPlayerControl::new(3, 3, control(0.5)))
                .unwrap_err(),
            ControlTimelinePushError::Full { capacity: 2 }
        );
        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline.next_event().unwrap().sequence, 1);
    }

    #[test]
    fn validates_timestamp_sequence_and_duplicate_order() {
        let mut timeline = PlayerControlTimeline::new(8, 0, control(0.0)).unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(10, 10, control(1.0)))
            .unwrap();

        assert!(matches!(
            timeline.enqueue(TimedPlayerControl::new(10, 10, control(1.0))),
            Err(ControlTimelinePushError::Duplicate { .. })
        ));
        assert_eq!(
            timeline
                .enqueue(TimedPlayerControl::new(9, 11, control(-1.0)))
                .unwrap_err(),
            ControlTimelinePushError::NonMonotonicFrame {
                previous: 10,
                incoming: 9,
            }
        );
        assert_eq!(
            timeline
                .enqueue(TimedPlayerControl::new(11, 9, control(-1.0)))
                .unwrap_err(),
            ControlTimelinePushError::NonMonotonicSequence {
                previous: 10,
                incoming: 9,
            }
        );
        timeline
            .enqueue(TimedPlayerControl::new(10, 11, control(-1.0)))
            .unwrap();
    }

    #[test]
    fn rapid_reversals_remain_distinct_and_ordered() {
        let mut timeline = PlayerControlTimeline::new(32, 8_000, control(0.0)).unwrap();
        for index in 0..24_u64 {
            let rate = if index % 2 == 0 { 18.0 } else { -18.0 };
            timeline
                .enqueue(TimedPlayerControl::new(
                    8_000 + index,
                    100 + index,
                    control(rate),
                ))
                .unwrap();
        }

        let mut observed = Vec::new();
        timeline
            .visit_block(24, |item| {
                if let ControlBlockItem::Transition { event, .. } = item {
                    observed.push(event.control.deck.hand_target_angular_velocity_rad_s);
                }
            })
            .unwrap();

        assert_eq!(observed.len(), 24);
        assert!(observed
            .windows(2)
            .all(|rates| rates[0].is_sign_positive() != rates[1].is_sign_positive()));
    }

    #[test]
    fn rejects_invalid_control_values() {
        let mut timeline = PlayerControlTimeline::new(2, 0, control(0.0)).unwrap();
        let mut invalid = control(1.0);
        invalid.deck.hand_target_angle_rad = Some(f64::NAN);
        assert_eq!(
            timeline
                .enqueue(TimedPlayerControl::new(0, 1, invalid))
                .unwrap_err(),
            ControlTimelinePushError::InvalidControl(InvalidControlValue {
                field: "handTargetAngleRad",
                requirement: ControlValueRequirement::Finite,
            })
        );

        invalid = control(1.0);
        invalid.deck.hand_normal_force_n = -0.1;
        assert!(matches!(
            timeline.enqueue(TimedPlayerControl::new(0, 1, invalid)),
            Err(ControlTimelinePushError::InvalidControl(
                InvalidControlValue {
                    requirement: ControlValueRequirement::Nonnegative,
                    ..
                }
            ))
        ));
    }

    #[test]
    fn snapshot_restore_replays_identically_after_ring_wrap() {
        let mut timeline = PlayerControlTimeline::new(4, 1_000, control(0.0)).unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(1_001, 1, control(1.0)))
            .unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(1_002, 2, control(-1.0)))
            .unwrap();
        timeline.visit_block(3, |_| {}).unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(1_004, 3, control(0.5)))
            .unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(1_006, 4, control(-0.5)))
            .unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(1_007, 5, control(0.25)))
            .unwrap();

        let snapshot = timeline.snapshot();
        let mut restored = PlayerControlTimeline::from_snapshot(&snapshot).unwrap();
        assert_eq!(restored.snapshot(), snapshot);

        let mut original_items = Vec::new();
        let mut restored_items = Vec::new();
        timeline
            .visit_block(5, |item| original_items.push(item))
            .unwrap();
        restored
            .visit_block(5, |item| restored_items.push(item))
            .unwrap();
        assert_eq!(restored_items, original_items);
        assert_eq!(restored.snapshot(), timeline.snapshot());
    }

    #[test]
    fn checkpoint_restores_consumed_events_and_render_state() {
        let initial = control(0.0);
        let mut timeline = PlayerControlTimeline::new(4, 100, initial).unwrap();
        for (frame, sequence, rate) in [(101, 1, 1.0), (103, 2, -1.0), (110, 3, 0.5)] {
            timeline
                .enqueue(TimedPlayerControl::new(frame, sequence, control(rate)))
                .unwrap();
        }
        let checkpoint = timeline.checkpoint();

        let mut first_render = Vec::new();
        timeline
            .visit_block(5, |item| first_render.push(item))
            .unwrap();
        assert_eq!(timeline.render_frame(), 105);
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline.current_control(), control(-1.0));

        timeline.restore_checkpoint(checkpoint).unwrap();
        assert_eq!(timeline.render_frame(), 100);
        assert_eq!(timeline.len(), 3);
        assert_eq!(timeline.current_control(), initial);
        assert_eq!(timeline.next_event().unwrap().sequence, 1);

        let mut repeated_render = Vec::new();
        timeline
            .visit_block(5, |item| repeated_render.push(item))
            .unwrap();
        assert_eq!(repeated_render, first_render);

        timeline.restore_checkpoint(checkpoint).unwrap();
        assert_eq!(timeline.next_event().unwrap().sequence, 1);
    }

    #[test]
    fn checkpoint_restores_wrapped_full_queue() {
        let mut timeline = PlayerControlTimeline::new(3, 0, control(0.0)).unwrap();
        for (frame, sequence, rate) in [(1, 1, 1.0), (2, 2, -1.0), (9, 3, 0.5)] {
            timeline
                .enqueue(TimedPlayerControl::new(frame, sequence, control(rate)))
                .unwrap();
        }
        timeline.visit_block(3, |_| {}).unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(10, 4, control(-0.5)))
            .unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(11, 5, control(0.25)))
            .unwrap();
        assert!(timeline.is_full());

        let checkpoint = timeline.checkpoint();
        let before = timeline.snapshot();
        timeline.visit_block(9, |_| {}).unwrap();
        assert!(timeline.is_empty());
        timeline.restore_checkpoint(checkpoint).unwrap();
        assert_eq!(timeline.snapshot(), before);
        assert_eq!(
            timeline
                .enqueue(TimedPlayerControl::new(12, 6, control(-0.25)))
                .unwrap_err(),
            ControlTimelinePushError::Full { capacity: 3 }
        );

        let mut sequences = Vec::new();
        timeline
            .visit_block(9, |item| {
                if let ControlBlockItem::Transition { event, .. } = item {
                    sequences.push(event.sequence);
                }
            })
            .unwrap();
        assert_eq!(sequences, [3, 4, 5]);
    }

    #[test]
    fn checkpoint_rejects_enqueue_that_overwrites_consumed_slot() {
        let mut timeline = PlayerControlTimeline::new(2, 0, control(0.0)).unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(1, 1, control(1.0)))
            .unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(10, 2, control(-1.0)))
            .unwrap();
        let checkpoint = timeline.checkpoint();

        timeline.visit_block(2, |_| {}).unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(20, 3, control(0.5)))
            .unwrap();
        let before_failed_restore = timeline.snapshot();

        assert_eq!(
            timeline.restore_checkpoint(checkpoint),
            Err(ControlTimelineCheckpointRestoreError::SlotsChanged)
        );
        assert_eq!(timeline.snapshot(), before_failed_restore);
    }

    #[test]
    fn checkpoint_rejects_another_timeline() {
        let first = PlayerControlTimeline::new(2, 0, control(0.0)).unwrap();
        let checkpoint = first.checkpoint();
        let mut second = PlayerControlTimeline::new(2, 0, control(0.0)).unwrap();

        assert_eq!(
            second.restore_checkpoint(checkpoint),
            Err(ControlTimelineCheckpointRestoreError::TimelineMismatch)
        );
    }

    #[test]
    fn snapshot_restore_invalidates_checkpoint() {
        let mut timeline = PlayerControlTimeline::new(2, 0, control(0.0)).unwrap();
        timeline
            .enqueue(TimedPlayerControl::new(1, 1, control(1.0)))
            .unwrap();
        let checkpoint = timeline.checkpoint();
        let snapshot = timeline.snapshot();
        timeline.restore(&snapshot).unwrap();

        assert_eq!(
            timeline.restore_checkpoint(checkpoint),
            Err(ControlTimelineCheckpointRestoreError::SlotsChanged)
        );
    }

    #[test]
    fn restore_rejects_capacity_change_without_mutation() {
        let source = PlayerControlTimeline::new(3, 0, control(0.0)).unwrap();
        let snapshot = source.snapshot();
        let mut destination = PlayerControlTimeline::new(2, 50, control(1.0)).unwrap();
        let before = destination.snapshot();

        assert_eq!(
            destination.restore(&snapshot).unwrap_err(),
            ControlTimelineRestoreError::CapacityMismatch {
                expected: 2,
                actual: 3,
            }
        );
        assert_eq!(destination.snapshot(), before);
    }

    #[test]
    fn block_advance_reports_frame_overflow_without_mutation() {
        let mut timeline = PlayerControlTimeline::new(2, u64::MAX - 1, control(0.0)).unwrap();
        assert_eq!(
            timeline.visit_block(2, |_| {}).unwrap_err(),
            ControlTimelineAdvanceError::FrameOverflow
        );
        assert_eq!(timeline.render_frame(), u64::MAX - 1);
    }
}
