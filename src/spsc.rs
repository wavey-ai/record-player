//! A bounded mailbox for one producer and one consumer.
//!
//! The mailbox uses only safe Rust atomic operations. Its storage does not
//! allocate after construction. The built-in player-control codec also does not
//! allocate. A full mailbox rejects the incoming value and keeps all accepted
//! values in order.

use crate::mechanics::{DeckMechanicalControl, MotorMode};
use crate::timed_control::{PlayerControl, TimedPlayerControl};
use std::cell::Cell;
use std::error::Error;
use std::fmt;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

/// Converts one value to and from a fixed-size atomic payload.
///
/// The producer calls `encode`. The consumer calls `decode`. Implementations
/// must decode every payload that they encode. A real-time codec must not
/// allocate in either function.
pub trait SpscCodec<T: Copy, const BYTES: usize>: Send + Sync + 'static {
    fn encode(value: T, destination: &mut [u8; BYTES]);
    fn decode(source: &[u8; BYTES]) -> Option<T>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpscCreateError {
    ZeroCapacity,
    ZeroPayloadBytes,
    CapacityTooLarge,
}

impl fmt::Display for SpscCreateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => formatter.write_str("SPSC mailbox capacity must be positive"),
            Self::ZeroPayloadBytes => {
                formatter.write_str("SPSC mailbox payload size must be positive")
            }
            Self::CapacityTooLarge => formatter.write_str("SPSC mailbox capacity is too large"),
        }
    }
}

impl Error for SpscCreateError {}

/// A rejected push returns ownership of the incoming value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpscPushError<T> {
    Full(T),
    Disconnected(T),
}

impl<T> SpscPushError<T> {
    pub fn into_inner(self) -> T {
        match self {
            Self::Full(value) | Self::Disconnected(value) => value,
        }
    }
}

impl<T> fmt::Display for SpscPushError<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full(_) => formatter.write_str("SPSC mailbox is full"),
            Self::Disconnected(_) => formatter.write_str("SPSC mailbox consumer is disconnected"),
        }
    }
}

impl<T: fmt::Debug> Error for SpscPushError<T> {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpscPopError {
    Empty,
    Disconnected,
    InvalidEncoding,
}

impl fmt::Display for SpscPopError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("SPSC mailbox is empty"),
            Self::Disconnected => formatter.write_str("SPSC mailbox producer is disconnected"),
            Self::InvalidEncoding => formatter.write_str("SPSC mailbox payload is invalid"),
        }
    }
}

impl Error for SpscPopError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpscCommitOutcome {
    Value,
    InvalidEncoding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpscCommitError {
    NothingPeeked,
}

impl fmt::Display for SpscCommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NothingPeeked => formatter.write_str("SPSC mailbox has no peeked payload"),
        }
    }
}

impl Error for SpscCommitError {}

/// Counters can wrap after `usize::MAX` operations.
///
/// A snapshot can observe counters from different instants. Use it for
/// telemetry, not synchronization.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpscCounters {
    pub accepted_pushes: usize,
    pub rejected_full_pushes: usize,
    pub rejected_disconnected_pushes: usize,
    pub successful_pops: usize,
    pub empty_pops: usize,
    pub disconnected_pops: usize,
    pub invalid_payloads: usize,
}

#[repr(align(64))]
struct CacheLine<T>(T);

#[repr(align(64))]
struct Slot<const BYTES: usize> {
    bytes: [AtomicU8; BYTES],
}

impl<const BYTES: usize> Slot<BYTES> {
    fn new() -> Self {
        Self {
            bytes: std::array::from_fn(|_| AtomicU8::new(0)),
        }
    }

    fn store(&self, source: &[u8; BYTES]) {
        for (destination, source) in self.bytes.iter().zip(source.iter().copied()) {
            destination.store(source, Ordering::Relaxed);
        }
    }

    fn load(&self) -> [u8; BYTES] {
        std::array::from_fn(|index| self.bytes[index].load(Ordering::Relaxed))
    }
}

struct ProducerCounters {
    accepted_pushes: AtomicUsize,
    rejected_full_pushes: AtomicUsize,
    rejected_disconnected_pushes: AtomicUsize,
}

struct ConsumerCounters {
    successful_pops: AtomicUsize,
    empty_pops: AtomicUsize,
    disconnected_pops: AtomicUsize,
    invalid_payloads: AtomicUsize,
}

struct Shared<T, Codec, const BYTES: usize> {
    slots: Box<[Slot<BYTES>]>,
    write_count: CacheLine<AtomicUsize>,
    read_count: CacheLine<AtomicUsize>,
    producer_alive: AtomicBool,
    consumer_alive: AtomicBool,
    producer_counters: CacheLine<ProducerCounters>,
    consumer_counters: CacheLine<ConsumerCounters>,
    marker: PhantomData<(T, Codec)>,
}

impl<T, Codec, const BYTES: usize> Shared<T, Codec, BYTES> {
    fn capacity(&self) -> usize {
        self.slots.len()
    }

    fn counters(&self) -> SpscCounters {
        SpscCounters {
            accepted_pushes: self
                .producer_counters
                .0
                .accepted_pushes
                .load(Ordering::Relaxed),
            rejected_full_pushes: self
                .producer_counters
                .0
                .rejected_full_pushes
                .load(Ordering::Relaxed),
            rejected_disconnected_pushes: self
                .producer_counters
                .0
                .rejected_disconnected_pushes
                .load(Ordering::Relaxed),
            successful_pops: self
                .consumer_counters
                .0
                .successful_pops
                .load(Ordering::Relaxed),
            empty_pops: self.consumer_counters.0.empty_pops.load(Ordering::Relaxed),
            disconnected_pops: self
                .consumer_counters
                .0
                .disconnected_pops
                .load(Ordering::Relaxed),
            invalid_payloads: self
                .consumer_counters
                .0
                .invalid_payloads
                .load(Ordering::Relaxed),
        }
    }

    fn approximate_len(&self) -> usize {
        let written = self.write_count.0.load(Ordering::Acquire);
        let read = self.read_count.0.load(Ordering::Acquire);
        written.wrapping_sub(read).min(self.capacity())
    }
}

/// The only producer endpoint for a mailbox.
///
/// The endpoint is `Send` but not `Sync`. Its operations use a bounded number
/// of lock-free atomic loads and stores.
pub struct SpscProducer<T, Codec, const BYTES: usize> {
    shared: Arc<Shared<T, Codec, BYTES>>,
    write_count: usize,
    next_slot: usize,
    not_sync: PhantomData<Cell<()>>,
}

impl<T, Codec, const BYTES: usize> SpscProducer<T, Codec, BYTES>
where
    T: Copy + Send + Sync + 'static,
    Codec: SpscCodec<T, BYTES>,
{
    pub fn try_push(&mut self, value: T) -> Result<(), SpscPushError<T>> {
        if !self.shared.consumer_alive.load(Ordering::Acquire) {
            increment(&self.shared.producer_counters.0.rejected_disconnected_pushes);
            return Err(SpscPushError::Disconnected(value));
        }

        let read_count = self.shared.read_count.0.load(Ordering::Acquire);
        if self.write_count.wrapping_sub(read_count) >= self.shared.capacity() {
            increment(&self.shared.producer_counters.0.rejected_full_pushes);
            return Err(SpscPushError::Full(value));
        }

        let mut encoded = [0; BYTES];
        Codec::encode(value, &mut encoded);
        self.shared.slots[self.next_slot].store(&encoded);

        self.write_count = self.write_count.wrapping_add(1);
        self.next_slot += 1;
        if self.next_slot == self.shared.capacity() {
            self.next_slot = 0;
        }

        // This release publishes every payload byte to the consumer.
        self.shared
            .write_count
            .0
            .store(self.write_count, Ordering::Release);
        increment(&self.shared.producer_counters.0.accepted_pushes);
        Ok(())
    }

    pub fn capacity(&self) -> usize {
        self.shared.capacity()
    }

    pub fn approximate_len(&self) -> usize {
        self.shared.approximate_len()
    }

    pub fn counters(&self) -> SpscCounters {
        self.shared.counters()
    }

    pub fn is_consumer_connected(&self) -> bool {
        self.shared.consumer_alive.load(Ordering::Acquire)
    }
}

impl<T, Codec, const BYTES: usize> Drop for SpscProducer<T, Codec, BYTES> {
    fn drop(&mut self) {
        self.shared.producer_alive.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy)]
enum PeekedPayload<T> {
    None,
    Value(T),
    InvalidEncoding,
}

/// The only consumer endpoint for a mailbox.
///
/// The endpoint is `Send` but not `Sync`. Its operations use a bounded number
/// of lock-free atomic loads and stores.
pub struct SpscConsumer<T, Codec, const BYTES: usize> {
    shared: Arc<Shared<T, Codec, BYTES>>,
    read_count: usize,
    next_slot: usize,
    peeked: PeekedPayload<T>,
    not_sync: PhantomData<Cell<()>>,
}

pub type SpscEndpoints<T, Codec, const BYTES: usize> =
    (SpscProducer<T, Codec, BYTES>, SpscConsumer<T, Codec, BYTES>);

impl<T, Codec, const BYTES: usize> SpscConsumer<T, Codec, BYTES>
where
    T: Copy + Send + Sync + 'static,
    Codec: SpscCodec<T, BYTES>,
{
    /// Returns the current value without permitting slot reuse.
    ///
    /// Repeated calls return the same value. Call `commit_peeked` only after the
    /// downstream consumer accepts the value.
    pub fn try_peek(&mut self) -> Result<T, SpscPopError> {
        match self.peeked {
            PeekedPayload::Value(value) => return Ok(value),
            PeekedPayload::InvalidEncoding => return Err(SpscPopError::InvalidEncoding),
            PeekedPayload::None => {}
        }

        self.require_head()?;
        let encoded = self.shared.slots[self.next_slot].load();
        match Codec::decode(&encoded) {
            Some(value) => {
                self.peeked = PeekedPayload::Value(value);
                Ok(value)
            }
            None => {
                self.peeked = PeekedPayload::InvalidEncoding;
                Err(SpscPopError::InvalidEncoding)
            }
        }
    }

    /// Consumes the value returned by `try_peek`.
    ///
    /// The release store permits the producer to reuse this slot. An invalid
    /// payload can also be committed so later values remain accessible.
    pub fn commit_peeked(&mut self) -> Result<SpscCommitOutcome, SpscCommitError> {
        self.commit_cached().ok_or(SpscCommitError::NothingPeeked)
    }

    pub fn try_pop(&mut self) -> Result<T, SpscPopError> {
        let value = match self.try_peek() {
            Ok(value) => value,
            Err(SpscPopError::Empty) => {
                increment(&self.shared.consumer_counters.0.empty_pops);
                return Err(SpscPopError::Empty);
            }
            Err(SpscPopError::Disconnected) => {
                increment(&self.shared.consumer_counters.0.disconnected_pops);
                return Err(SpscPopError::Disconnected);
            }
            Err(SpscPopError::InvalidEncoding) => {
                let outcome = self
                    .commit_cached()
                    .expect("an invalid peek must reserve the queue head");
                debug_assert_eq!(outcome, SpscCommitOutcome::InvalidEncoding);
                return Err(SpscPopError::InvalidEncoding);
            }
        };

        let outcome = self
            .commit_cached()
            .expect("a successful peek must reserve the queue head");
        debug_assert_eq!(outcome, SpscCommitOutcome::Value);
        Ok(value)
    }

    pub fn capacity(&self) -> usize {
        self.shared.capacity()
    }

    pub fn approximate_len(&self) -> usize {
        self.shared.approximate_len()
    }

    pub fn counters(&self) -> SpscCounters {
        self.shared.counters()
    }

    pub fn is_producer_connected(&self) -> bool {
        self.shared.producer_alive.load(Ordering::Acquire)
    }

    fn require_head(&self) -> Result<(), SpscPopError> {
        let mut write_count = self.shared.write_count.0.load(Ordering::Acquire);
        if self.read_count != write_count {
            return Ok(());
        }
        if self.shared.producer_alive.load(Ordering::Acquire) {
            return Err(SpscPopError::Empty);
        }

        // The second load observes the producer's final publication.
        write_count = self.shared.write_count.0.load(Ordering::Acquire);
        if self.read_count == write_count {
            Err(SpscPopError::Disconnected)
        } else {
            Ok(())
        }
    }

    fn commit_cached(&mut self) -> Option<SpscCommitOutcome> {
        let peeked = std::mem::replace(&mut self.peeked, PeekedPayload::None);
        let outcome = match peeked {
            PeekedPayload::None => return None,
            PeekedPayload::Value(_) => SpscCommitOutcome::Value,
            PeekedPayload::InvalidEncoding => SpscCommitOutcome::InvalidEncoding,
        };

        self.read_count = self.read_count.wrapping_add(1);
        self.next_slot += 1;
        if self.next_slot == self.shared.capacity() {
            self.next_slot = 0;
        }

        // This release permits the producer to reuse the consumed slot.
        self.shared
            .read_count
            .0
            .store(self.read_count, Ordering::Release);
        match outcome {
            SpscCommitOutcome::Value => {
                increment(&self.shared.consumer_counters.0.successful_pops);
            }
            SpscCommitOutcome::InvalidEncoding => {
                increment(&self.shared.consumer_counters.0.invalid_payloads);
            }
        }
        Some(outcome)
    }
}

impl<T, Codec, const BYTES: usize> Drop for SpscConsumer<T, Codec, BYTES> {
    fn drop(&mut self) {
        self.shared.consumer_alive.store(false, Ordering::Release);
    }
}

/// Creates one bounded mailbox and its two unique endpoints.
pub fn spsc_mailbox<T, Codec, const BYTES: usize>(
    capacity: usize,
) -> Result<SpscEndpoints<T, Codec, BYTES>, SpscCreateError>
where
    T: Copy + Send + Sync + 'static,
    Codec: SpscCodec<T, BYTES>,
{
    if capacity == 0 {
        return Err(SpscCreateError::ZeroCapacity);
    }
    if BYTES == 0 {
        return Err(SpscCreateError::ZeroPayloadBytes);
    }
    if capacity > usize::MAX / 2 {
        return Err(SpscCreateError::CapacityTooLarge);
    }

    let slots = std::iter::repeat_with(Slot::new)
        .take(capacity)
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let shared = Arc::new(Shared {
        slots,
        write_count: CacheLine(AtomicUsize::new(0)),
        read_count: CacheLine(AtomicUsize::new(0)),
        producer_alive: AtomicBool::new(true),
        consumer_alive: AtomicBool::new(true),
        producer_counters: CacheLine(ProducerCounters {
            accepted_pushes: AtomicUsize::new(0),
            rejected_full_pushes: AtomicUsize::new(0),
            rejected_disconnected_pushes: AtomicUsize::new(0),
        }),
        consumer_counters: CacheLine(ConsumerCounters {
            successful_pops: AtomicUsize::new(0),
            empty_pops: AtomicUsize::new(0),
            disconnected_pops: AtomicUsize::new(0),
            invalid_payloads: AtomicUsize::new(0),
        }),
        marker: PhantomData,
    });

    Ok((
        SpscProducer {
            shared: Arc::clone(&shared),
            write_count: 0,
            next_slot: 0,
            not_sync: PhantomData,
        },
        SpscConsumer {
            shared,
            read_count: 0,
            next_slot: 0,
            peeked: PeekedPayload::None,
            not_sync: PhantomData,
        },
    ))
}

/// Each counter has one endpoint writer.
fn increment(counter: &AtomicUsize) {
    let next = counter.load(Ordering::Relaxed).wrapping_add(1);
    counter.store(next, Ordering::Relaxed);
}

pub const TIMED_PLAYER_CONTROL_BYTES: usize = 68;

#[derive(Debug, Clone, Copy, Default)]
pub struct TimedPlayerControlCodec;

impl SpscCodec<TimedPlayerControl, TIMED_PLAYER_CONTROL_BYTES> for TimedPlayerControlCodec {
    fn encode(value: TimedPlayerControl, destination: &mut [u8; TIMED_PLAYER_CONTROL_BYTES]) {
        let mut cursor = 0;
        write_u64(destination, &mut cursor, value.absolute_frame);
        write_u64(destination, &mut cursor, value.sequence);
        destination[cursor] = match value.control.deck.motor_mode {
            MotorMode::Off => 0,
            MotorMode::Servo => 1,
            MotorMode::Brake => 2,
        };
        cursor += 1;
        write_f64(
            destination,
            &mut cursor,
            value.control.deck.motor_target_angular_velocity_rad_s,
        );
        destination[cursor] = u8::from(value.control.deck.hand_contact);
        cursor += 1;
        match value.control.deck.hand_target_angle_rad {
            None => {
                destination[cursor] = 0;
                cursor += 1;
                write_f64(destination, &mut cursor, 0.0);
            }
            Some(angle) => {
                destination[cursor] = 1;
                cursor += 1;
                write_f64(destination, &mut cursor, angle);
            }
        }
        write_f64(
            destination,
            &mut cursor,
            value.control.deck.hand_target_angular_velocity_rad_s,
        );
        write_f64(
            destination,
            &mut cursor,
            value.control.deck.hand_normal_force_n,
        );
        write_f64(
            destination,
            &mut cursor,
            value.control.deck.hand_contact_radius_m,
        );
        write_f64(
            destination,
            &mut cursor,
            value.control.deck.stylus_torque_nm,
        );
        destination[cursor] = u8::from(value.control.stylus_lowered);
        cursor += 1;
        debug_assert_eq!(cursor, TIMED_PLAYER_CONTROL_BYTES);
    }

    fn decode(source: &[u8; TIMED_PLAYER_CONTROL_BYTES]) -> Option<TimedPlayerControl> {
        let mut cursor = 0;
        let absolute_frame = read_u64(source, &mut cursor);
        let sequence = read_u64(source, &mut cursor);
        let motor_mode = match source[cursor] {
            0 => MotorMode::Off,
            1 => MotorMode::Servo,
            2 => MotorMode::Brake,
            _ => return None,
        };
        cursor += 1;
        let motor_target_angular_velocity_rad_s = read_f64(source, &mut cursor);
        let hand_contact = match source[cursor] {
            0 => false,
            1 => true,
            _ => return None,
        };
        cursor += 1;
        let hand_target_angle_rad = match source[cursor] {
            0 => {
                cursor += 1;
                let _ = read_f64(source, &mut cursor);
                None
            }
            1 => {
                cursor += 1;
                Some(read_f64(source, &mut cursor))
            }
            _ => return None,
        };
        let hand_target_angular_velocity_rad_s = read_f64(source, &mut cursor);
        let hand_normal_force_n = read_f64(source, &mut cursor);
        let hand_contact_radius_m = read_f64(source, &mut cursor);
        let stylus_torque_nm = read_f64(source, &mut cursor);
        let stylus_lowered = match source[cursor] {
            0 => false,
            1 => true,
            _ => return None,
        };
        cursor += 1;
        debug_assert_eq!(cursor, TIMED_PLAYER_CONTROL_BYTES);

        Some(TimedPlayerControl::new(
            absolute_frame,
            sequence,
            PlayerControl::new(
                DeckMechanicalControl {
                    motor_mode,
                    motor_target_angular_velocity_rad_s,
                    hand_contact,
                    hand_target_angle_rad,
                    hand_target_angular_velocity_rad_s,
                    hand_normal_force_n,
                    hand_contact_radius_m,
                    stylus_torque_nm,
                },
                stylus_lowered,
            ),
        ))
    }
}

pub type TimedPlayerControlProducer =
    SpscProducer<TimedPlayerControl, TimedPlayerControlCodec, TIMED_PLAYER_CONTROL_BYTES>;
pub type TimedPlayerControlConsumer =
    SpscConsumer<TimedPlayerControl, TimedPlayerControlCodec, TIMED_PLAYER_CONTROL_BYTES>;

pub fn timed_player_control_mailbox(
    capacity: usize,
) -> Result<(TimedPlayerControlProducer, TimedPlayerControlConsumer), SpscCreateError> {
    spsc_mailbox::<TimedPlayerControl, TimedPlayerControlCodec, TIMED_PLAYER_CONTROL_BYTES>(
        capacity,
    )
}

fn write_u64<const BYTES: usize>(destination: &mut [u8; BYTES], cursor: &mut usize, value: u64) {
    let end = *cursor + 8;
    destination[*cursor..end].copy_from_slice(&value.to_le_bytes());
    *cursor = end;
}

fn read_u64<const BYTES: usize>(source: &[u8; BYTES], cursor: &mut usize) -> u64 {
    let end = *cursor + 8;
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&source[*cursor..end]);
    *cursor = end;
    u64::from_le_bytes(bytes)
}

fn write_f64<const BYTES: usize>(destination: &mut [u8; BYTES], cursor: &mut usize, value: f64) {
    write_u64(destination, cursor, value.to_bits());
}

fn read_f64<const BYTES: usize>(source: &[u8; BYTES], cursor: &mut usize) -> f64 {
    f64::from_bits(read_u64(source, cursor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timed_control::{ControlTimelinePushError, PlayerControlTimeline};
    use std::thread;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct TestEvent {
        sequence: u64,
        value: u64,
    }

    struct TestCodec;

    impl SpscCodec<TestEvent, 16> for TestCodec {
        fn encode(value: TestEvent, destination: &mut [u8; 16]) {
            destination[..8].copy_from_slice(&value.sequence.to_le_bytes());
            destination[8..].copy_from_slice(&value.value.to_le_bytes());
        }

        fn decode(source: &[u8; 16]) -> Option<TestEvent> {
            let mut sequence = [0; 8];
            let mut value = [0; 8];
            sequence.copy_from_slice(&source[..8]);
            value.copy_from_slice(&source[8..]);
            Some(TestEvent {
                sequence: u64::from_le_bytes(sequence),
                value: u64::from_le_bytes(value),
            })
        }
    }

    fn mailbox(
        capacity: usize,
    ) -> (
        SpscProducer<TestEvent, TestCodec, 16>,
        SpscConsumer<TestEvent, TestCodec, 16>,
    ) {
        spsc_mailbox(capacity).unwrap()
    }

    fn timed_event(absolute_frame: u64, sequence: u64) -> TimedPlayerControl {
        TimedPlayerControl::new(
            absolute_frame,
            sequence,
            PlayerControl::new(DeckMechanicalControl::default(), true),
        )
    }

    #[test]
    fn rejects_invalid_capacities() {
        assert!(matches!(
            spsc_mailbox::<TestEvent, TestCodec, 16>(0),
            Err(SpscCreateError::ZeroCapacity)
        ));

        struct EmptyCodec;
        impl SpscCodec<TestEvent, 0> for EmptyCodec {
            fn encode(_value: TestEvent, _destination: &mut [u8; 0]) {}
            fn decode(_source: &[u8; 0]) -> Option<TestEvent> {
                None
            }
        }
        assert!(matches!(
            spsc_mailbox::<TestEvent, EmptyCodec, 0>(1),
            Err(SpscCreateError::ZeroPayloadBytes)
        ));
    }

    #[test]
    fn rejects_full_push_without_overwriting_values() {
        let (mut producer, mut consumer) = mailbox(2);
        let first = TestEvent {
            sequence: 1,
            value: 11,
        };
        let second = TestEvent {
            sequence: 2,
            value: 22,
        };
        let rejected = TestEvent {
            sequence: 3,
            value: 33,
        };

        producer.try_push(first).unwrap();
        producer.try_push(second).unwrap();
        assert_eq!(
            producer.try_push(rejected),
            Err(SpscPushError::Full(rejected))
        );
        assert_eq!(consumer.try_pop(), Ok(first));
        assert_eq!(consumer.try_pop(), Ok(second));
        assert_eq!(consumer.try_pop(), Err(SpscPopError::Empty));

        let counters = consumer.counters();
        assert_eq!(counters.accepted_pushes, 2);
        assert_eq!(counters.rejected_full_pushes, 1);
        assert_eq!(counters.successful_pops, 2);
        assert_eq!(counters.empty_pops, 1);
    }

    #[test]
    fn repeated_peek_waits_for_explicit_commit() {
        let (mut producer, mut consumer) = mailbox(2);
        let event = TestEvent {
            sequence: 1,
            value: 11,
        };
        producer.try_push(event).unwrap();

        assert_eq!(consumer.try_peek(), Ok(event));
        assert_eq!(consumer.try_peek(), Ok(event));
        assert_eq!(consumer.approximate_len(), 1);
        assert_eq!(consumer.counters().successful_pops, 0);
        assert_eq!(consumer.commit_peeked(), Ok(SpscCommitOutcome::Value));
        assert_eq!(consumer.approximate_len(), 0);
        assert_eq!(consumer.counters().successful_pops, 1);
        assert_eq!(
            consumer.commit_peeked(),
            Err(SpscCommitError::NothingPeeked)
        );
    }

    #[test]
    fn full_timeline_backpressure_does_not_lose_mailbox_event() {
        let initial = PlayerControl::default();
        let mut timeline = PlayerControlTimeline::new(1, 0, initial).unwrap();
        timeline.enqueue(timed_event(0, 1)).unwrap();
        let incoming = timed_event(1, 2);
        let (mut producer, mut consumer) = timed_player_control_mailbox(1).unwrap();
        producer.try_push(incoming).unwrap();

        let peeked = consumer.try_peek().unwrap();
        assert_eq!(
            timeline.enqueue(peeked),
            Err(ControlTimelinePushError::Full { capacity: 1 })
        );
        assert_eq!(consumer.approximate_len(), 1);
        assert_eq!(consumer.try_peek(), Ok(incoming));

        timeline.visit_block(1, |_| {}).unwrap();
        timeline.enqueue(incoming).unwrap();
        assert_eq!(consumer.commit_peeked(), Ok(SpscCommitOutcome::Value));
        assert_eq!(consumer.approximate_len(), 0);
        assert_eq!(timeline.next_event(), Some(incoming));
    }

    #[test]
    fn producer_cannot_overwrite_a_peeked_slot() {
        let (mut producer, mut consumer) = mailbox(1);
        let first = TestEvent {
            sequence: 1,
            value: 11,
        };
        let second = TestEvent {
            sequence: 2,
            value: 22,
        };
        producer.try_push(first).unwrap();
        assert_eq!(consumer.try_peek(), Ok(first));

        assert_eq!(producer.try_push(second), Err(SpscPushError::Full(second)));
        assert_eq!(consumer.try_peek(), Ok(first));
        assert_eq!(consumer.commit_peeked(), Ok(SpscCommitOutcome::Value));
        producer.try_push(second).unwrap();
        assert_eq!(consumer.try_pop(), Ok(second));
    }

    #[test]
    fn try_pop_consumes_an_existing_peek() {
        let (mut producer, mut consumer) = mailbox(1);
        let event = TestEvent {
            sequence: 4,
            value: 44,
        };
        producer.try_push(event).unwrap();
        assert_eq!(consumer.try_peek(), Ok(event));
        assert_eq!(consumer.try_pop(), Ok(event));
        assert_eq!(consumer.approximate_len(), 0);
        assert_eq!(consumer.counters().successful_pops, 1);
    }

    #[test]
    fn wraps_non_power_of_two_capacity_without_reordering() {
        let (mut producer, mut consumer) = mailbox(3);
        for sequence in 0..20_000 {
            let event = TestEvent {
                sequence,
                value: !sequence,
            };
            producer.try_push(event).unwrap();
            assert_eq!(consumer.try_pop(), Ok(event));
        }
        assert_eq!(producer.approximate_len(), 0);
    }

    #[test]
    fn peek_and_commit_wrap_non_power_of_two_capacity() {
        let (mut producer, mut consumer) = mailbox(3);
        for sequence in 0..20_000 {
            let event = TestEvent {
                sequence,
                value: sequence.rotate_right(9),
            };
            producer.try_push(event).unwrap();
            assert_eq!(consumer.try_peek(), Ok(event));
            assert_eq!(consumer.commit_peeked(), Ok(SpscCommitOutcome::Value));
        }
        assert_eq!(producer.approximate_len(), 0);
        assert_eq!(consumer.counters().successful_pops, 20_000);
    }

    #[test]
    fn distinguishes_empty_from_disconnected() {
        let (producer, mut consumer) = mailbox(1);
        assert_eq!(consumer.try_pop(), Err(SpscPopError::Empty));
        drop(producer);
        assert_eq!(consumer.try_pop(), Err(SpscPopError::Disconnected));
        assert!(!consumer.is_producer_connected());
        assert_eq!(consumer.counters().disconnected_pops, 1);
    }

    #[test]
    fn peek_drains_final_value_before_disconnect() {
        let (mut producer, mut consumer) = mailbox(1);
        let event = TestEvent {
            sequence: 5,
            value: 55,
        };
        assert_eq!(consumer.try_peek(), Err(SpscPopError::Empty));
        producer.try_push(event).unwrap();
        drop(producer);

        assert_eq!(consumer.try_peek(), Ok(event));
        assert_eq!(consumer.try_peek(), Ok(event));
        assert_eq!(consumer.commit_peeked(), Ok(SpscCommitOutcome::Value));
        assert_eq!(consumer.try_peek(), Err(SpscPopError::Disconnected));
    }

    #[test]
    fn returns_value_when_consumer_is_disconnected() {
        let (mut producer, consumer) = mailbox(1);
        drop(consumer);
        let event = TestEvent {
            sequence: 7,
            value: 9,
        };
        assert_eq!(
            producer.try_push(event),
            Err(SpscPushError::Disconnected(event))
        );
        assert!(!producer.is_consumer_connected());
        assert_eq!(producer.counters().rejected_disconnected_pushes, 1);
    }

    #[test]
    fn transfers_values_between_threads_in_order() {
        const EVENT_COUNT: u64 = 200_000;
        let (mut producer, mut consumer) = mailbox(64);

        let producer_thread = thread::spawn(move || {
            for sequence in 0..EVENT_COUNT {
                let mut pending = TestEvent {
                    sequence,
                    value: sequence.rotate_left(17),
                };
                loop {
                    match producer.try_push(pending) {
                        Ok(()) => break,
                        Err(SpscPushError::Full(value)) => {
                            pending = value;
                            std::hint::spin_loop();
                        }
                        Err(SpscPushError::Disconnected(_)) => {
                            panic!("consumer disconnected")
                        }
                    }
                }
            }
            producer.counters()
        });

        let consumer_thread = thread::spawn(move || {
            for expected_sequence in 0..EVENT_COUNT {
                loop {
                    match consumer.try_pop() {
                        Ok(event) => {
                            assert_eq!(event.sequence, expected_sequence);
                            assert_eq!(event.value, expected_sequence.rotate_left(17));
                            break;
                        }
                        Err(SpscPopError::Empty) => std::hint::spin_loop(),
                        Err(error) => panic!("unexpected pop error: {error}"),
                    }
                }
            }
            consumer.counters()
        });

        let producer_counters = producer_thread.join().unwrap();
        let consumer_counters = consumer_thread.join().unwrap();
        assert_eq!(producer_counters.accepted_pushes, EVENT_COUNT as usize);
        assert_eq!(consumer_counters.successful_pops, EVENT_COUNT as usize);
    }

    #[test]
    fn timed_control_codec_preserves_every_field() {
        let (mut producer, mut consumer) = timed_player_control_mailbox(3).unwrap();
        let events = [
            TimedPlayerControl::new(
                44_100,
                8,
                PlayerControl::new(
                    DeckMechanicalControl {
                        motor_mode: MotorMode::Off,
                        motor_target_angular_velocity_rad_s: -0.0,
                        hand_contact: false,
                        hand_target_angle_rad: None,
                        hand_target_angular_velocity_rad_s: -12.25,
                        hand_normal_force_n: 0.0,
                        hand_contact_radius_m: 0.149,
                        stylus_torque_nm: -0.000_012,
                    },
                    false,
                ),
            ),
            TimedPlayerControl::new(
                44_101,
                9,
                PlayerControl::new(
                    DeckMechanicalControl {
                        motor_mode: MotorMode::Servo,
                        motor_target_angular_velocity_rad_s: 3.49,
                        hand_contact: true,
                        hand_target_angle_rad: Some(-1.75),
                        hand_target_angular_velocity_rad_s: 48.0,
                        hand_normal_force_n: 4.5,
                        hand_contact_radius_m: 0.08,
                        stylus_torque_nm: 0.000_1,
                    },
                    true,
                ),
            ),
            TimedPlayerControl::new(
                44_102,
                10,
                PlayerControl::new(
                    DeckMechanicalControl {
                        motor_mode: MotorMode::Brake,
                        motor_target_angular_velocity_rad_s: -3.49,
                        hand_contact: true,
                        hand_target_angle_rad: Some(2.25),
                        hand_target_angular_velocity_rad_s: -60.0,
                        hand_normal_force_n: 5.0,
                        hand_contact_radius_m: 0.12,
                        stylus_torque_nm: 0.0,
                    },
                    false,
                ),
            ),
        ];

        for event in events {
            producer.try_push(event).unwrap();
        }
        for expected in events {
            let actual = consumer.try_pop().unwrap();
            assert_eq!(actual.absolute_frame, expected.absolute_frame);
            assert_eq!(actual.sequence, expected.sequence);
            assert_eq!(
                actual.control.stylus_lowered,
                expected.control.stylus_lowered
            );
            assert_eq!(
                actual.control.deck.motor_mode,
                expected.control.deck.motor_mode
            );
            assert_eq!(
                actual.control.deck.hand_contact,
                expected.control.deck.hand_contact
            );
            assert_eq!(
                actual
                    .control
                    .deck
                    .motor_target_angular_velocity_rad_s
                    .to_bits(),
                expected
                    .control
                    .deck
                    .motor_target_angular_velocity_rad_s
                    .to_bits()
            );
            assert_eq!(
                actual.control.deck.hand_target_angle_rad.map(f64::to_bits),
                expected
                    .control
                    .deck
                    .hand_target_angle_rad
                    .map(f64::to_bits)
            );
            assert_eq!(
                actual
                    .control
                    .deck
                    .hand_target_angular_velocity_rad_s
                    .to_bits(),
                expected
                    .control
                    .deck
                    .hand_target_angular_velocity_rad_s
                    .to_bits()
            );
            assert_eq!(
                actual.control.deck.hand_normal_force_n.to_bits(),
                expected.control.deck.hand_normal_force_n.to_bits()
            );
            assert_eq!(
                actual.control.deck.hand_contact_radius_m.to_bits(),
                expected.control.deck.hand_contact_radius_m.to_bits()
            );
            assert_eq!(
                actual.control.deck.stylus_torque_nm.to_bits(),
                expected.control.deck.stylus_torque_nm.to_bits()
            );
        }
    }

    #[test]
    fn invalid_payload_is_consumed_and_counted() {
        struct RejectCodec;
        impl SpscCodec<TestEvent, 1> for RejectCodec {
            fn encode(_value: TestEvent, destination: &mut [u8; 1]) {
                destination[0] = 255;
            }
            fn decode(_source: &[u8; 1]) -> Option<TestEvent> {
                None
            }
        }

        let (mut producer, mut consumer) = spsc_mailbox::<TestEvent, RejectCodec, 1>(1).unwrap();
        producer
            .try_push(TestEvent {
                sequence: 1,
                value: 2,
            })
            .unwrap();
        assert_eq!(consumer.try_pop(), Err(SpscPopError::InvalidEncoding));
        assert_eq!(consumer.approximate_len(), 0);
        assert_eq!(consumer.counters().invalid_payloads, 1);
    }

    #[test]
    fn invalid_peek_remains_reserved_until_commit() {
        struct RejectCodec;
        impl SpscCodec<TestEvent, 1> for RejectCodec {
            fn encode(_value: TestEvent, destination: &mut [u8; 1]) {
                destination[0] = 255;
            }
            fn decode(_source: &[u8; 1]) -> Option<TestEvent> {
                None
            }
        }

        let (mut producer, mut consumer) = spsc_mailbox::<TestEvent, RejectCodec, 1>(1).unwrap();
        let first = TestEvent {
            sequence: 1,
            value: 2,
        };
        let blocked = TestEvent {
            sequence: 2,
            value: 3,
        };
        producer.try_push(first).unwrap();

        assert_eq!(consumer.try_peek(), Err(SpscPopError::InvalidEncoding));
        assert_eq!(consumer.try_peek(), Err(SpscPopError::InvalidEncoding));
        assert_eq!(consumer.counters().invalid_payloads, 0);
        assert_eq!(
            producer.try_push(blocked),
            Err(SpscPushError::Full(blocked))
        );
        assert_eq!(
            consumer.commit_peeked(),
            Ok(SpscCommitOutcome::InvalidEncoding)
        );
        assert_eq!(consumer.approximate_len(), 0);
        assert_eq!(consumer.counters().invalid_payloads, 1);
    }

    #[test]
    fn endpoints_are_send() {
        fn assert_send<T: Send>() {}
        assert_send::<SpscProducer<TestEvent, TestCodec, 16>>();
        assert_send::<SpscConsumer<TestEvent, TestCodec, 16>>();
    }
}
