//! Geometry-driven vinyl VFX shared by live playback and offline REMIX.
//!
//! The processors in this module use record coordinates rather than wall
//! clock time. A scene therefore keeps its phase when the platter slows,
//! reverses, or moves under a hand.
//!
//! The polar memory stores one revolution of audio indexed by angle at
//! full fidelity: 2^17 bins per turn oversamples a 33 1/3 RPM revolution
//! at 48 kHz (86,400 frames) by ~1.5x, writes fill every bin the stylus
//! crosses, and reads interpolate between bins. Each bin is tagged with
//! the unwrapped turn time it was written, so a read only returns audio
//! the stylus actually cut on the pass being asked for.

use std::f64::consts::TAU;

pub const VINYL_VFX_NONE: u32 = 0;
pub const VINYL_VFX_ADJACENT_GHOST: u32 = 1;
pub const VINYL_VFX_THREE_NEEDLES: u32 = 2;
pub const VINYL_VFX_CUT_CONSTELLATION: u32 = 3;
pub const VINYL_VFX_INNER_FIRE: u32 = 4;
pub const VINYL_VFX_SPLIT_WALLS: u32 = 5;
pub const VINYL_VFX_WORN_HALO: u32 = 6;
pub const VINYL_VFX_PINCH: u32 = 7;
/// Retired from the picker on 2026-09-09. The code stays reserved and the
/// branch stays in `process_frame`: a take cut with this scene, and a share
/// code carrying it, still render as they were made. See `docs/DECK_VFX.md`.
pub const VINYL_VFX_NULL_POINTS: u32 = 8;
pub const VINYL_VFX_OVERCUT: u32 = 9;
pub const VINYL_VFX_MAX_SCENE: u32 = VINYL_VFX_OVERCUT;

/// Where THREE NEEDLES stands its other two heads, as a fraction of one
/// revolution — the near head at the spacing, the far one at twice it.
///
/// SPREAD steps this ladder rather than sweeping it. What makes the scene
/// work is the heads landing on the beat, and a locked groove is exactly one
/// turn, so these are fractions of the loop itself and are musical without
/// anything knowing the tempo. Even thirds — where the scene was nailed
/// down, and right only for a loop holding a multiple of three beats — is
/// one rung among them now.
///
/// Nothing reaches half a turn. The far head sits at twice the near one, so
/// at a half it would land a full revolution back, which on a locked groove
/// is the needle itself.
const THREE_NEEDLE_SPACINGS: [f64; 6] = [
    1.0 / 16.0,
    1.0 / 8.0,
    3.0 / 16.0,
    1.0 / 4.0,
    1.0 / 3.0,
    3.0 / 8.0,
];

/// Where each rung of the ladder stands on the SPREAD's throw: the centre
/// of its sixth, so the amounts that landed the rungs when the ladder was
/// stepped land them still.
fn three_needle_anchor(index: usize) -> f64 {
    (index as f64 + 0.5) / THREE_NEEDLE_SPACINGS.len() as f64
}

/// The spacing SPREAD is standing on. The ladder's six rungs are anchors
/// and the throw between them is continuous: a record's own beat divides a
/// turn where it divides it — `cut_rpm ÷ (k × BPM₀)`, which is 0.267 on an
/// 84 BPM pressing and on no rung — and the heads have to be able to stand
/// there, or they beat against every record that is not cut at a round
/// number. Below the first anchor is the first rung, above the last the
/// last; zero never arrives, an amount that low having already turned the
/// processor off upstream.
pub(crate) fn three_needle_spacing(amount: f64) -> f64 {
    let count = THREE_NEEDLE_SPACINGS.len();
    let amount = if amount.is_finite() { amount.clamp(0.0, 1.0) } else { three_needle_anchor(4) };
    if amount <= three_needle_anchor(0) {
        return THREE_NEEDLE_SPACINGS[0];
    }
    for index in 1..count {
        let (low, high) = (three_needle_anchor(index - 1), three_needle_anchor(index));
        if amount <= high {
            let along = (amount - low) / (high - low);
            return THREE_NEEDLE_SPACINGS[index - 1] + (THREE_NEEDLE_SPACINGS[index] - THREE_NEEDLE_SPACINGS[index - 1]) * along;
        }
    }
    THREE_NEEDLE_SPACINGS[count - 1]
}

/// The inverse: the amount that lands a spacing, on a rung or between two.
pub(crate) fn three_needle_amount(spacing: f64) -> f64 {
    let count = THREE_NEEDLE_SPACINGS.len();
    let spacing = if spacing.is_finite() { spacing } else { THREE_NEEDLE_SPACINGS[4] };
    if spacing <= THREE_NEEDLE_SPACINGS[0] {
        return three_needle_anchor(0);
    }
    for index in 1..count {
        let (low, high) = (THREE_NEEDLE_SPACINGS[index - 1], THREE_NEEDLE_SPACINGS[index]);
        if spacing <= high {
            let along = (spacing - low) / (high - low);
            return three_needle_anchor(index - 1) + (three_needle_anchor(index) - three_needle_anchor(index - 1)) * along;
        }
    }
    three_needle_anchor(count - 1)
}

/// The swing that pinches: only the slow, wide lateral motion carries the
/// stylus far enough up the walls to matter, and band-limiting to it keeps
/// the octave it generates well clear of Nyquist.
const PINCH_SWING_HZ: f64 = 900.0;
/// How hard the ride is driven before it is bounded.
///
/// At 3.4 a mastered track reached full scale and clipped. The scene stands
/// clear of the ceiling at this figure and still widens the record.
const PINCH_RIDE: f64 = 2.5;
/// The room the ride needs, as a fraction of the cut, at full RIDE.
///
/// The ride is added to one wall and taken off the other, so it costs
/// headroom. A record mastered to the ceiling has none to give: played at
/// unity it clipped 1,584 samples of a twenty second passage whatever the
/// drive was set to, because the cut was already at full scale before the
/// scene added anything. So the cut steps back by what the ride can put on
/// top of it, which costs about 2 dB at full RIDE and clips nine samples of
/// the same passage.
const PINCH_HEADROOM: f64 = 0.35;
/// The floor under the ride, and how many poles hold it.
///
/// Squaring a band returns the difference of every pair of partials as well
/// as their sum. The sums are the octave the scene is for. The differences
/// run down to zero, and dense music puts most of its pairs close together,
/// so most of the square's energy lands below the music.
///
/// The ride enters the groove antiphase, which makes all of it side channel.
/// A lathe cannot cut bass into the sides — the stylus lifts out of the
/// groove — so a cutting chain takes the bass to the middle first, and an
/// elliptical equaliser does it between 150 and 300 Hz. This floor stands at
/// the top of that range. Measured on a mastered track, the ride carried 467
/// times the record's own side-channel bass at 70 Hz and 16 times at 300 Hz.
const PINCH_RIDE_FLOOR_HZ: f64 = 300.0;
/// The floor in OVERCUT's loop, for the same reason and against a longer
/// arithmetic: the loop repeats once a revolution, so what it sums under
/// this floor is slower than the turn itself.
const OVERCUT_FLOOR_HZ: f64 = 28.0;

/// The lathe's protection circuit, and where it holds the low end.
///
/// A cutter head moves the stylus vertically for the side channel, and it can
/// only move so far before the stylus leaves the groove. So a cutting chain
/// carries an elliptical equaliser, which takes the low end toward the middle
/// as it approaches that limit. It is a dynamic circuit and not a fixed
/// filter: it acts on the passages that ask for it and leaves the rest alone.
///
/// Two scenes here manufacture side content. PINCH makes it by construction,
/// because the ride enters the groove antiphase. OVERCUT makes it by
/// compounding, because one generation's difference between the walls is fed
/// into the next. Both end on this, so the gain each is set to is a question
/// about what it sounds like rather than about what it does to the sides.
///
/// `ELLIPTICAL_RATIO` is how much low side the circuit allows against the low
/// middle beside it. Below that figure nothing happens at all.
const ELLIPTICAL_HZ: f64 = 200.0;
const ELLIPTICAL_RATIO: f64 = 0.50;
const ELLIPTICAL_ATTACK_HZ: f64 = 60.0;
const ELLIPTICAL_RELEASE_HZ: f64 = 3.0;
/// The gain the previous revolution is mixed back at, from LAYERS at zero to
/// LAYERS at full.
///
/// A locked groove hands the loop the same audio every turn, so the returns
/// add to each other instead of decaying and the sum converges on
/// `1 / (1 - gain)` times the cut. The gain used to run to 0.92 into a hard
/// clamp, which on a mastered track came back 4.8 dB above the record with
/// sixty times its side-channel bass.
///
/// The ceiling under it is a knee now, so the loop has room to be worth
/// hearing. Measured on a locked groove cut from a master already at 0.98,
/// with LAYERS at full:
///
/// | gain | build | peak | clipped | side/mid bass |
/// | ---- | ----- | ---- | ------- | ------------- |
/// | 0.40 | 1.1dB | 0.87 |       0 |         0.009 |
/// | 0.85 | 3.9dB | 0.92 |       0 |         0.031 |
/// | 1.15 | 4.7dB | 0.93 |       0 |         0.053 |
/// | 1.45 | 5.9dB | 0.95 |       0 |         0.060 |
///
/// Neither the ceiling nor the sides set this. The knee holds the peak well
/// past a gain of one, and the elliptical circuit acts at a side-to-middle
/// figure of 0.5, which the loop does not approach.
///
/// Stability sets it. What one revolution returns is `gain` less what the
/// 7 kHz lowpass, the floor and the interpolation of the polar read take —
/// about half of it for ordinary programme, but nearly all of it for content
/// that is low, coherent and repeating, which is exactly what a locked
/// groove hands back. A gain of one against a return of one is a loop that
/// never decays, so the range stops short of it with room to spare.
const OVERCUT_LAYER_MINIMUM: f64 = 0.15;
const OVERCUT_LAYER_RANGE: f64 = 0.70;

/// Where a pivoted arm is tangent to the groove, as a fraction of the way
/// through the programme — the outer null first, because a record plays
/// outward-in. Baerwald's two radii on a twelve inch, over the band a record
/// is actually cut in.
const NULL_POINT_OUTER: f64 = 0.29;
const NULL_POINT_INNER: f64 = 0.93;
/// The tangency error at its worst — the outer edge — brought to one.
const NULL_POINT_ERROR_SCALE: f64 = 3.6;

const POLAR_BINS: usize = 1 << 17;
const WEAR_BINS: usize = 2_048;
/// How long one groove position takes to wear through, at full amount and
/// under a stylus that keeps returning to it.
const WEAR_FULL_SECONDS: f64 = 20.0;
/// A bin tag may differ from the requested time by a couple of bins of
/// rounding before the content is someone else's revolution.
const POLAR_TAG_TOLERANCE: f64 = 2.5 / POLAR_BINS as f64;
const CONSTELLATION_SECTORS: u32 = 12;
const CONSTELLATION_DEFAULT_PATTERN: u32 = 0b1011_0100_1101;

#[derive(Clone, Copy, Debug)]
pub struct VinylVfxContext {
    pub sample_rate: f64,
    pub rpm: f64,
    pub start_turns: f64,
    pub end_turns: f64,
    pub start_position: f64,
    pub end_position: f64,
    pub total_frames: usize,
    pub pressing_seed: u32,
}

impl Default for VinylVfxContext {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            rpm: 33.333_333,
            start_turns: 0.0,
            end_turns: 0.0,
            start_position: 0.0,
            end_position: 0.0,
            total_frames: 0,
            pressing_seed: 0,
        }
    }
}

/// Preallocated state for one graph instance.
///
/// Live players keep one instance beside the acoustic DSP. Offline buses use
/// the same type, so a scene's block order and state transitions are shared.
///
/// `Clone` is for the replay snapshot: a replay is a transaction on the live
/// deck, and the revolution memory and the halo it leaves behind belong to
/// the record that was playing, not to the take that was replayed.
#[derive(Clone)]
pub struct VinylVfxProcessor {
    scene: u32,
    amount: f64,
    polar_samples: Vec<[f32; 2]>,
    polar_written: Vec<f64>,
    /// How many polar bins hold audio from this record. Counted as it is
    /// written rather than walked when asked: the Vfx row reports fill on
    /// every publish, and the buffer is 131,072 bins wide.
    polar_filled: usize,
    last_write: Option<(f64, [f32; 2])>,
    wear: Vec<f32>,
    lowpass: [f64; 2],
    /// The floor PINCH and OVERCUT put under what they add. PINCH stands
    /// three of its four poles here, the first being `lowpass[0]`; OVERCUT
    /// stands one per channel.
    highpass: [f64; 3],
    /// The elliptical circuit: the low band of the side and of the middle,
    /// then an envelope on each.
    elliptical: [f64; 4],
    gate_gain: f64,
}

/// Two million bins are not a debug line. The scene, the amount and how
/// much of each buffer is holding something are.
impl std::fmt::Debug for VinylVfxProcessor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VinylVfxProcessor")
            .field("scene", &self.scene)
            .field("amount", &self.amount)
            .field("polar_filled", &self.polar_filled)
            .field("wear_level", &self.wear_level())
            .finish_non_exhaustive()
    }
}

impl Default for VinylVfxProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl VinylVfxProcessor {
    pub fn new() -> Self {
        Self {
            scene: VINYL_VFX_NONE,
            amount: 1.0,
            polar_samples: vec![[0.0; 2]; POLAR_BINS],
            polar_written: vec![f64::NEG_INFINITY; POLAR_BINS],
            polar_filled: 0,
            last_write: None,
            wear: vec![0.0; WEAR_BINS],
            lowpass: [0.0; 2],
            highpass: [0.0; 3],
            elliptical: [0.0; 4],
            gate_gain: 1.0,
        }
    }

    pub fn set_scene(&mut self, scene: u32, amount: f64) {
        let scene = scene.min(VINYL_VFX_MAX_SCENE);
        let amount = finite_or(amount, 1.0).clamp(0.0, 1.0);
        if self.scene != scene {
            self.scene = scene;
            self.reset_transient_state();
        }
        self.amount = amount;
    }

    pub fn scene(&self) -> u32 {
        self.scene
    }

    pub fn amount(&self) -> f64 {
        self.amount
    }

    pub fn reset_transient_state(&mut self) {
        self.polar_samples.fill([0.0; 2]);
        self.polar_written.fill(f64::NEG_INFINITY);
        self.polar_filled = 0;
        self.last_write = None;
        self.lowpass = [0.0; 2];
        self.highpass = [0.0; 3];
        self.elliptical = [0.0; 4];
        self.gate_gain = 1.0;
    }

    /// Clears the wear bins.
    ///
    /// Wear deliberately survives a scene change — `set_scene` resets the
    /// transient state and leaves this standing, because a worn record is
    /// worn whichever scene is reading it. So this is the only way it goes
    /// away, short of a new record on the platter.
    pub fn reset_wear(&mut self) {
        self.wear.fill(0.0);
    }

    /// Everything this processor has accumulated: the revolution memory,
    /// the wear, and the filters riding on both.
    pub fn reset_all(&mut self) {
        self.reset_transient_state();
        self.reset_wear();
    }

    /// The halo, for a take's world: WORN HALO's bins, 0..=1 by phase
    /// within one revolution.
    pub fn halo_wear_map(&self) -> Vec<f32> {
        self.wear.clone()
    }

    /// Restores a halo. Shorter maps leave the rest of the turn mint;
    /// longer ones are cut to the bins there are.
    pub fn restore_halo_wear(&mut self, map: &[f32]) {
        self.wear.fill(0.0);
        for (slot, value) in self.wear.iter_mut().zip(map.iter()) {
            *slot = if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 };
        }
    }

    /// Mean wear around the revolution, 0..=1 — the number the meter fills to.
    pub fn wear_level(&self) -> f64 {
        if self.wear.is_empty() {
            return 0.0;
        }
        let total: f64 = self.wear.iter().map(|value| f64::from(*value)).sum();
        total / self.wear.len() as f64
    }

    /// The worst-worn bin, 0..=1 — the meter's tick. A record worn through
    /// in one bar reads far worse here than its mean admits.
    pub fn wear_peak(&self) -> f64 {
        self.wear
            .iter()
            .fold(0.0_f64, |peak, value| peak.max(f64::from(*value)))
    }

    /// How much of the revolution memory holds this record, 0..=1.
    pub fn polar_fill_ratio(&self) -> f64 {
        self.polar_filled as f64 / POLAR_BINS as f64
    }

    /// What this processor is holding, in bytes. Reported rather than
    /// assumed: the Vfx row shows the real cost of the memory a scene reads
    /// back from, and the buffers are the largest thing on a deck.
    pub fn memory_bytes(&self) -> usize {
        self.polar_samples.len() * std::mem::size_of::<[f32; 2]>()
            + self.polar_written.len() * std::mem::size_of::<f64>()
            + self.wear.len() * std::mem::size_of::<f32>()
    }

    pub const fn wear_bin_count() -> usize {
        WEAR_BINS
    }

    pub const fn polar_bin_count() -> usize {
        POLAR_BINS
    }

    pub fn process_interleaved(
        &mut self,
        samples: &mut [f32],
        channel_count: usize,
        context: VinylVfxContext,
    ) {
        if self.scene == VINYL_VFX_NONE
            || self.amount <= f64::EPSILON
            || !(1..=2).contains(&channel_count)
        {
            return;
        }
        let frame_count = samples.len() / channel_count;
        if frame_count == 0 {
            return;
        }
        for frame_index in 0..frame_count {
            let offset = frame_index * channel_count;
            let mut frame = [samples[offset], samples[offset]];
            if channel_count == 2 {
                frame[1] = samples[offset + 1];
            }
            self.process_frame(&mut frame, channel_count, frame_index, frame_count, context);
            samples[offset] = frame[0];
            if channel_count == 2 {
                samples[offset + 1] = frame[1];
            }
        }
    }

    pub fn process_planar(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        context: VinylVfxContext,
    ) {
        if self.scene == VINYL_VFX_NONE || self.amount <= f64::EPSILON {
            return;
        }
        let frame_count = left.len().min(right.len());
        for frame_index in 0..frame_count {
            let mut frame = [left[frame_index], right[frame_index]];
            self.process_frame(&mut frame, 2, frame_index, frame_count, context);
            left[frame_index] = frame[0];
            right[frame_index] = frame[1];
        }
    }

    /// Takes the low end toward the middle when the sides ask for more than a
    /// groove will hold. See `ELLIPTICAL_HZ`.
    fn bass_to_middle(&mut self, frame: &mut [f32; 2], sample_rate: f64) {
        let mid = (f64::from(frame[0]) + f64::from(frame[1])) * 0.5;
        let side = (f64::from(frame[0]) - f64::from(frame[1])) * 0.5;
        let corner = 1.0 - (-TAU * ELLIPTICAL_HZ / sample_rate).exp();
        self.elliptical[0] += (side - self.elliptical[0]) * corner;
        self.elliptical[1] += (mid - self.elliptical[1]) * corner;
        let side_low = self.elliptical[0];
        let mid_low = self.elliptical[1];

        // The envelope rises quickly and falls slowly, so the circuit answers
        // a bass note as it arrives and does not chatter between them.
        let attack = 1.0 - (-TAU * ELLIPTICAL_ATTACK_HZ / sample_rate).exp();
        let release = 1.0 - (-TAU * ELLIPTICAL_RELEASE_HZ / sample_rate).exp();
        for (slot, level) in [(2usize, side_low.abs()), (3usize, mid_low.abs())] {
            let rate = if level > self.elliptical[slot] { attack } else { release };
            self.elliptical[slot] += (level - self.elliptical[slot]) * rate;
        }

        let allowed = self.elliptical[3] * ELLIPTICAL_RATIO;
        let carried = self.elliptical[2];
        // Only what is over the allowance moves, and only the low band moves:
        // the sides keep everything above the corner.
        let held = if carried > allowed && carried > 1.0e-9 {
            side_low * (1.0 - allowed / carried)
        } else {
            0.0
        };
        let corrected = side - held;
        frame[0] = soft_limit(mid + corrected);
        frame[1] = soft_limit(mid - corrected);
    }

    fn process_frame(
        &mut self,
        frame: &mut [f32; 2],
        channel_count: usize,
        frame_index: usize,
        frame_count: usize,
        context: VinylVfxContext,
    ) {
        let progress = if frame_count > 1 {
            frame_index as f64 / (frame_count - 1) as f64
        } else {
            0.0
        };
        let turns = interpolate(context.start_turns, context.end_turns, progress);
        let position = interpolate(context.start_position, context.end_position, progress);
        let total_frames = context.total_frames.max(1) as f64;
        let radius = (position / total_frames).clamp(0.0, 1.0);
        let signed_motion = context.end_turns - context.start_turns;
        let direction = if signed_motion < 0.0 { -1.0 } else { 1.0 };
        let dry = *frame;

        match self.scene {
            VINYL_VFX_ADJACENT_GHOST => {
                let previous_turn = self.read_polar(turns - direction, channel_count);
                let transfer = self.amount * 0.72;
                for channel in 0..channel_count {
                    self.lowpass[channel] +=
                        (f64::from(previous_turn[channel]) - self.lowpass[channel]) * 0.22;
                    frame[channel] =
                        soft_limit(f64::from(dry[channel]) + self.lowpass[channel] * transfer);
                }
            }
            VINYL_VFX_THREE_NEEDLES => {
                // SPREAD moves the heads, not their level: a needle that is
                // merely quieter is the same needle in the same wrong place.
                let spacing = three_needle_spacing(self.amount);
                let second = self.read_polar(turns - direction * spacing, channel_count);
                let third = self.read_polar(turns - direction * spacing * 2.0, channel_count);
                let level = 0.52;
                for channel in 0..channel_count {
                    let head_mix =
                        f64::from(second[channel]) * 0.58 + f64::from(third[channel]) * 0.42;
                    frame[channel] = soft_limit(f64::from(dry[channel]) + head_mix * level);
                }
            }
            VINYL_VFX_CUT_CONSTELLATION => {
                let phase = turns.rem_euclid(1.0);
                let sector = ((phase * f64::from(CONSTELLATION_SECTORS)).floor() as u32)
                    .min(CONSTELLATION_SECTORS - 1);
                let pattern = constellation_pattern(context.pressing_seed);
                let open = pattern & (1 << sector) != 0;
                let target = if open { 1.0 } else { 1.0 - self.amount * 0.96 };
                let sample_rate = finite_or(context.sample_rate, 48_000.0).max(1.0);
                let alpha = 1.0 - (-1.0 / (sample_rate * 0.0018)).exp();
                self.gate_gain += (target - self.gate_gain) * alpha;
                for sample in frame.iter_mut().take(channel_count) {
                    *sample = (f64::from(*sample) * self.gate_gain) as f32;
                }
            }
            VINYL_VFX_INNER_FIRE => {
                let heat = self.amount * (0.35 + 0.65 * radius.powf(1.35));
                let sample_rate = finite_or(context.sample_rate, 48_000.0).max(1.0);
                let cutoff = 18_000.0 - heat * 11_000.0;
                let alpha = 1.0 - (-TAU * cutoff / sample_rate).exp();
                for channel in 0..channel_count {
                    let driven = (f64::from(dry[channel]) * (1.0 + heat * 1.8)).tanh();
                    self.lowpass[channel] += (driven - self.lowpass[channel]) * alpha;
                    frame[channel] = soft_limit(self.lowpass[channel] * (1.0 - heat * 0.22));
                }
                if channel_count == 2 {
                    narrow_stereo(frame, heat * 0.6);
                }
            }
            VINYL_VFX_SPLIT_WALLS => {
                let left_head = self.read_polar(turns - direction * 0.25, channel_count);
                let right_head = self.read_polar(turns - direction * 0.625, channel_count);
                let wall = self.amount * 0.68;
                if channel_count == 2 {
                    let lateral = (f64::from(dry[0]) + f64::from(dry[1])) * 0.5;
                    let vertical = (f64::from(dry[0]) - f64::from(dry[1])) * 0.5;
                    let delayed_lateral = (f64::from(left_head[0]) + f64::from(left_head[1])) * 0.5;
                    let delayed_vertical =
                        (f64::from(right_head[0]) - f64::from(right_head[1])) * 0.5;
                    frame[0] = soft_limit(lateral + vertical + delayed_lateral * wall);
                    frame[1] = soft_limit(lateral - vertical - delayed_vertical * wall);
                } else {
                    frame[0] = soft_limit(
                        f64::from(dry[0])
                            + (f64::from(left_head[0]) - f64::from(right_head[0])) * wall,
                    );
                }
            }
            VINYL_VFX_WORN_HALO => {
                let wear_index = wear_bin(turns);
                let sample_rate = finite_or(context.sample_rate, 48_000.0).max(1.0);
                // A bin is one angle of the revolution, so the stylus
                // is over it for `frames per turn / WEAR_BINS` frames of
                // each pass rather than for the whole pass. The rate
                // carries that factor: without it the twenty seconds is
                // twenty seconds times the bin count, which is eleven hours
                // on one locked groove.
                let increment =
                    self.amount * WEAR_BINS as f64 / (sample_rate * WEAR_FULL_SECONDS);
                self.wear[wear_index] =
                    (f64::from(self.wear[wear_index]) + increment).clamp(0.0, 1.0) as f32;
                let worn = f64::from(self.wear[wear_index]).sqrt() * self.amount;
                let cutoff = 19_000.0 - worn * 11_000.0;
                let alpha = 1.0 - (-TAU * cutoff / sample_rate).exp();
                let crackle = deterministic_crackle(
                    wear_index as u64,
                    turns.floor() as i64,
                    context.pressing_seed,
                    worn,
                );
                for channel in 0..channel_count {
                    self.lowpass[channel] +=
                        (f64::from(dry[channel]) - self.lowpass[channel]) * alpha;
                    frame[channel] = soft_limit(self.lowpass[channel] + crackle);
                }
            }
            VINYL_VFX_PINCH => {
                // A round stylus sitting in a V is pushed *up* as the walls
                // close on it, either way it swings. So a lateral cut
                // generates a vertical one at twice the frequency — the
                // record's own way of making width out of something cut in
                // mono. Real; it is why pinch effect is something cutting
                // engineers have to allow for.
                //
                // The rise is smooth. It goes as the *square* of the lateral
                // displacement, not as its absolute value: rectifying has a
                // corner at zero, and a corner is not a second harmonic but
                // every even harmonic at once, most of them above Nyquist
                // and folding back as grit. Squaring has no corner.
                //
                // And the swing that pinches is the big slow one. A stylus
                // is only carried far enough up the walls to matter by bass,
                // so the lateral is band-limited before it is squared, which
                // also puts the octave it generates nowhere near Nyquist.
                let sample_rate = finite_or(context.sample_rate, 48_000.0).max(1.0);
                let lateral = (f64::from(dry[0]) + f64::from(dry[1])) * 0.5;
                let vertical = (f64::from(dry[0]) - f64::from(dry[1])) * 0.5;
                let swing_alpha = 1.0 - (-TAU * PINCH_SWING_HZ / sample_rate).exp();
                self.lowpass[1] += (lateral - self.lowpass[1]) * swing_alpha;
                let squared = self.lowpass[1] * self.lowpass[1];
                // Squaring leaves a standing offset behind, and under it
                // the difference of every pair of partials in the band. The
                // stylus rides on none of that: the offset is not music, and
                // the differences run below the octave the scene is for.
                // Four poles at PINCH_RIDE_FLOOR_HZ take them together.
                let floor_alpha = 1.0 - (-TAU * PINCH_RIDE_FLOOR_HZ / sample_rate).exp();
                self.lowpass[0] += (squared - self.lowpass[0]) * floor_alpha;
                let mut ridden = squared - self.lowpass[0];
                for pole in 0..3 {
                    self.highpass[pole] += (ridden - self.highpass[pole]) * floor_alpha;
                    ridden -= self.highpass[pole];
                }
                // Bounded rather than clipped: a stylus can only ride so far
                // up a wall before it leaves it, and a clamp here is a
                // fuzzbox.
                let ride = (ridden * self.amount * PINCH_RIDE).tanh();
                let headroom = 1.0 / (1.0 + self.amount * PINCH_HEADROOM);
                frame[0] = soft_limit((lateral + vertical + ride) * headroom);
                frame[1] = soft_limit((lateral - vertical - ride) * headroom);
                if channel_count == 2 {
                    self.bass_to_middle(frame, sample_rate);
                }
            }
            VINYL_VFX_NULL_POINTS => {
                // A pivoted arm is tangent to the groove at exactly two
                // radii and wrong everywhere else, and the error it leaves
                // is mostly second harmonic. So the grit is a *place on the
                // record*: it blooms at the edge, falls to nothing at the
                // outer null, rises again through the middle and dies at the
                // inner one. The two walls do not meet the error at the same
                // angle, so they do not take the same amount of it.
                let sample_rate = finite_or(context.sample_rate, 48_000.0).max(1.0);
                let tangency = (radius - NULL_POINT_OUTER) * (radius - NULL_POINT_INNER);
                let bend = (tangency.abs() * NULL_POINT_ERROR_SCALE).min(1.0)
                    * self.amount
                    * 0.9;
                let dc_alpha = 1.0 - (-TAU * 18.0 / sample_rate).exp();
                let lean = [bend, bend * 0.72];
                for channel in 0..channel_count {
                    let sample = f64::from(dry[channel]);
                    // Squaring is the second harmonic, and it arrives with a
                    // standing offset that has to go back out.
                    let squared = sample * sample;
                    self.lowpass[channel] += (squared - self.lowpass[channel]) * dc_alpha;
                    frame[channel] =
                        soft_limit(sample + (squared - self.lowpass[channel]) * lean[channel] * 2.4);
                }
            }
            VINYL_VFX_OVERCUT => {
                // Every other scene reads the groove and leaves it as it
                // found it. This one cuts back into it: what comes out is
                // written where it was read, so the next revolution arrives
                // carrying the last one, and the one before that. A delay
                // whose line is the record and whose time is a turn, by
                // construction rather than by setting.
                //
                // Each pass loses its air, the way a plate cut from a plate
                // does, and the sum is taken through tanh rather than
                // clipped: it thickens and compresses into itself instead of
                // shattering.
                let sample_rate = finite_or(context.sample_rate, 48_000.0).max(1.0);
                let previous = self.read_polar(turns - direction, channel_count);
                let layer = OVERCUT_LAYER_MINIMUM + self.amount * OVERCUT_LAYER_RANGE;
                let alpha = 1.0 - (-TAU * 7_000.0 / sample_rate).exp();
                // Each generation loses its bottom as well as its air. A
                // one-pole lowpass passes DC at unity, so with no floor the
                // loop sums its own offset and everything under the turn
                // rate with it, at `layer` a revolution.
                let floor_alpha = 1.0 - (-TAU * OVERCUT_FLOOR_HZ / sample_rate).exp();
                for channel in 0..channel_count {
                    self.lowpass[channel] +=
                        (f64::from(previous[channel]) - self.lowpass[channel]) * alpha;
                    self.highpass[channel] +=
                        (self.lowpass[channel] - self.highpass[channel]) * floor_alpha;
                    let layered = self.lowpass[channel] - self.highpass[channel];
                    frame[channel] =
                        (f64::from(dry[channel]) + layered * layer).tanh() as f32;
                }
                // Before the pass is written back, so a generation cannot
                // hand its excess to the next one.
                if channel_count == 2 {
                    self.bass_to_middle(frame, sample_rate);
                }
            }
            _ => {}
        }

        // OVERCUT is the one scene that keeps what it made rather than what
        // it was handed. Everything else leaves the groove as it found it.
        let cut = if self.scene == VINYL_VFX_OVERCUT {
            *frame
        } else {
            dry
        };
        self.write_polar(turns, cut);
    }

    /// Interpolated read at an unwrapped turn coordinate. Bins whose write
    /// tag disagrees with the requested time contribute silence rather than
    /// another revolution's audio.
    fn read_polar(&self, turns: f64, channel_count: usize) -> [f32; 2] {
        let turns = finite_or(turns, 0.0);
        let bin_position = turns * POLAR_BINS as f64;
        let base = bin_position.floor();
        let fraction = bin_position - base;
        let mut value = [0.0_f64; 2];
        for (step, weight) in [(0.0, 1.0 - fraction), (1.0, fraction)] {
            let unwrapped = base + step;
            let index = wrap_bin(unwrapped);
            let expected = unwrapped / POLAR_BINS as f64;
            if (self.polar_written[index] - expected).abs() <= POLAR_TAG_TOLERANCE {
                value[0] += f64::from(self.polar_samples[index][0]) * weight;
                value[1] += f64::from(self.polar_samples[index][1]) * weight;
            }
        }
        let mut frame = [value[0] as f32, value[1] as f32];
        if channel_count == 1 {
            frame[1] = frame[0];
        }
        frame
    }

    /// Records the stylus pass, filling every bin crossed since the last
    /// write with linearly interpolated audio so the memory has no holes at
    /// any platter speed, in either direction.
    fn write_polar(&mut self, turns: f64, frame: [f32; 2]) {
        let turns = finite_or(turns, 0.0);
        let bin_position = turns * POLAR_BINS as f64;
        if let Some((previous_turns, previous_frame)) = self.last_write {
            let previous_position = previous_turns * POLAR_BINS as f64;
            let span = bin_position - previous_position;
            // A jump wider than a quarter turn is a seek, not a pass.
            if span != 0.0 && span.abs() <= POLAR_BINS as f64 * 0.25 {
                let low = previous_position.min(bin_position);
                let high = previous_position.max(bin_position);
                let mut bin = low.ceil();
                while bin <= high {
                    let t = ((bin - previous_position) / span).clamp(0.0, 1.0);
                    let index = wrap_bin(bin);
                    self.polar_samples[index] = [
                        lerp(previous_frame[0], frame[0], t),
                        lerp(previous_frame[1], frame[1], t),
                    ];
                    if !self.polar_written[index].is_finite() {
                        self.polar_filled += 1;
                    }
                    self.polar_written[index] = bin / POLAR_BINS as f64;
                    bin += 1.0;
                }
                self.last_write = Some((turns, frame));
                return;
            }
        }
        let index = wrap_bin(bin_position.floor());
        self.polar_samples[index] = frame;
        if !self.polar_written[index].is_finite() {
            self.polar_filled += 1;
        }
        self.polar_written[index] = bin_position.floor() / POLAR_BINS as f64;
        self.last_write = Some((turns, frame));
    }
}

fn wrap_bin(unwrapped: f64) -> usize {
    (unwrapped.rem_euclid(POLAR_BINS as f64) as usize).min(POLAR_BINS - 1)
}

fn wear_bin(turns: f64) -> usize {
    let phase = finite_or(turns, 0.0).rem_euclid(1.0);
    ((phase * WEAR_BINS as f64).floor() as usize).min(WEAR_BINS - 1)
}

fn lerp(start: f32, end: f32, t: f64) -> f32 {
    (f64::from(start) + (f64::from(end) - f64::from(start)) * t) as f32
}

/// The sector map is a property of the pressing: seed zero keeps the house
/// cut, any other pressing carves its own — always with at least three open
/// and three closed sectors so the gate stays a rhythm, not a mute.
fn constellation_pattern(seed: u32) -> u32 {
    if seed == 0 {
        return CONSTELLATION_DEFAULT_PATTERN;
    }
    let mut hash = u64::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for _ in 0..8 {
        hash ^= hash >> 30;
        hash = hash.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        hash ^= hash >> 27;
        let pattern = (hash as u32) & 0xFFF;
        let open = pattern.count_ones();
        if (3..=9).contains(&open) {
            return pattern;
        }
        hash = hash.wrapping_add(0x9E37_79B9);
    }
    CONSTELLATION_DEFAULT_PATTERN
}

fn interpolate(start: f64, end: f64, progress: f64) -> f64 {
    finite_or(start, 0.0) + (finite_or(end, start) - finite_or(start, 0.0)) * progress
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

/// The ceiling every scene ends on.
///
/// This was `clamp(-1.0, 1.0)`, which is a square corner, and a corner is
/// every odd harmonic at once — the harshest sound a limiter can make, and
/// the one a scene reaches for exactly when it is already loud. Below the
/// knee the value passes through unchanged. Above it the curve carries it
/// toward one and never past, with the slope matched at the knee so there is
/// no corner anywhere.
fn soft_limit(value: f64) -> f32 {
    const KNEE: f64 = 0.75;
    let magnitude = value.abs();
    if magnitude <= KNEE {
        return value as f32;
    }
    let over = (magnitude - KNEE) / (1.0 - KNEE);
    (value.signum() * (KNEE + (1.0 - KNEE) * over.tanh())) as f32
}

fn narrow_stereo(frame: &mut [f32; 2], amount: f64) {
    let amount = amount.clamp(0.0, 1.0);
    let mid = (f64::from(frame[0]) + f64::from(frame[1])) * 0.5;
    let side = (f64::from(frame[0]) - f64::from(frame[1])) * 0.5 * (1.0 - amount);
    frame[0] = soft_limit(mid + side);
    frame[1] = soft_limit(mid - side);
}

fn deterministic_crackle(index: u64, turn: i64, seed: u32, worn: f64) -> f64 {
    if worn <= f64::EPSILON {
        return 0.0;
    }
    let mut hash = index
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(turn as u64)
        .wrapping_add(u64::from(seed));
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    hash ^= hash >> 27;
    let chance = hash & 0x7ff;
    if chance >= (worn * 22.0) as u64 {
        return 0.0;
    }
    let bipolar = ((hash >> 16) & 0xffff) as f64 / 32_767.5 - 1.0;
    bipolar * worn * 0.09
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_walks_the_three_needle_ladder_and_still_reaches_even_thirds() {
        // Every rung is reachable, in order, at its anchor on the throw —
        // the same amounts that landed it when the ladder was stepped.
        let walked = (0..THREE_NEEDLE_SPACINGS.len())
            .map(|index| three_needle_spacing(three_needle_anchor(index)))
            .collect::<Vec<_>>();
        assert_eq!(walked, THREE_NEEDLE_SPACINGS.to_vec());
        // Between two rungs the throw is continuous and monotonic, and the
        // inverse lands back where it started: a record's own division of a
        // turn, 0.267 on an 84 BPM pressing, is a place the heads can stand.
        let mut last = 0.0;
        for step in 0..=200 {
            let spacing = three_needle_spacing(step as f64 / 200.0);
            assert!(spacing >= last, "the ladder went backwards at {step}");
            last = spacing;
            assert!((three_needle_spacing(three_needle_amount(spacing)) - spacing).abs() < 1e-9);
        }
        assert!((three_needle_spacing(three_needle_amount(0.267)) - 0.267).abs() < 1e-9);
        // The placement the scene was nailed down at is one of them.
        assert!(THREE_NEEDLE_SPACINGS.contains(&(1.0 / 3.0)));
        // The bottom of the throw is the closest spacing, not silence: an
        // amount that low has already turned the processor off upstream.
        assert_eq!(three_needle_spacing(0.0), THREE_NEEDLE_SPACINGS[0]);
        // The far head never lands a whole revolution back, which on a
        // locked groove would be the needle itself.
        for spacing in THREE_NEEDLE_SPACINGS {
            assert!(spacing * 2.0 < 1.0);
        }
    }

    fn context(turns: f64, frames: usize) -> VinylVfxContext {
        VinylVfxContext {
            start_turns: turns,
            end_turns: turns + frames as f64 / 4_800.0,
            end_position: frames as f64,
            total_frames: frames * 4,
            ..VinylVfxContext::default()
        }
    }

    #[test]
    fn bypass_is_bit_exact() {
        let mut processor = VinylVfxProcessor::new();
        let mut samples = vec![0.25_f32, -0.5, 0.75, -0.125];
        let expected = samples.clone();
        processor.process_interleaved(&mut samples, 2, context(0.0, 2));
        assert_eq!(samples, expected);
    }

    #[test]
    fn constellation_is_painted_on_turns_not_clock_time() {
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_CUT_CONSTELLATION, 1.0);
        let mut first = vec![0.5_f32; 9_600];
        processor.process_interleaved(&mut first, 2, context(0.0, 4_800));
        assert!(first.iter().any(|sample| sample.abs() < 0.2));
        assert!(first.iter().any(|sample| sample.abs() > 0.45));
    }

    #[test]
    fn constellation_repeats_the_same_cut_every_revolution() {
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_CUT_CONSTELLATION, 1.0);
        let mut first = vec![0.5_f32; 9_600];
        processor.process_interleaved(&mut first, 2, context(0.0, 4_800));
        let mut second = vec![0.5_f32; 9_600];
        processor.process_interleaved(&mut second, 2, context(1.0, 4_800));
        let mut third = vec![0.5_f32; 9_600];
        processor.process_interleaved(&mut third, 2, context(2.0, 4_800));
        // Once the gate slew has settled, revolution three must retrace
        // revolution two sample for sample.
        let drift = second
            .iter()
            .zip(third.iter())
            .skip(1_000)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(drift < 0.01, "cut drifted between revolutions: {drift}");
    }

    #[test]
    fn constellation_cut_belongs_to_the_pressing() {
        let mut house = VinylVfxProcessor::new();
        house.set_scene(VINYL_VFX_CUT_CONSTELLATION, 1.0);
        let mut pressed = VinylVfxProcessor::new();
        pressed.set_scene(VINYL_VFX_CUT_CONSTELLATION, 1.0);
        let mut house_output = vec![0.5_f32; 9_600];
        house.process_interleaved(&mut house_output, 2, context(0.0, 4_800));
        let mut pressed_output = vec![0.5_f32; 9_600];
        let seeded = VinylVfxContext {
            pressing_seed: 0xB17_5EED,
            ..context(0.0, 4_800)
        };
        pressed.process_interleaved(&mut pressed_output, 2, seeded);
        assert!(house_output
            .iter()
            .zip(pressed_output.iter())
            .any(|(a, b)| (a - b).abs() > 0.1));
    }

    #[test]
    fn adjacent_ghost_reads_the_previous_revolution() {
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_ADJACENT_GHOST, 1.0);
        let mut first = vec![0.4_f32; 9_600];
        processor.process_interleaved(&mut first, 2, context(0.0, 4_800));
        let mut second = vec![0.0_f32; 9_600];
        processor.process_interleaved(&mut second, 2, context(1.0, 4_800));
        assert!(second.iter().any(|sample| sample.abs() > 0.05));
    }

    #[test]
    fn ghost_echo_is_smooth_not_stair_stepped() {
        // A sine cut on revolution one must come back on revolution two as
        // a smooth (filtered) wave. The old 4,096-bin memory returned a
        // zero-order-hold staircase whose sample-to-sample jumps rival the
        // signal itself.
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_ADJACENT_GHOST, 1.0);
        let frames = 4_800_usize;
        let mut first: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let value = (i as f64 / frames as f64 * TAU * 96.0).sin() as f32 * 0.6;
                [value, value]
            })
            .collect();
        processor.process_interleaved(&mut first, 2, context(0.0, frames));
        let mut second = vec![0.0_f32; frames * 2];
        processor.process_interleaved(&mut second, 2, context(1.0, frames));
        let ghost: Vec<f32> = second.chunks_exact(2).map(|f| f[0]).collect();
        let peak = ghost.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
        assert!(peak > 0.05, "no ghost came back: {peak}");
        let max_jump = ghost
            .windows(2)
            .skip(200)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);
        // 96 cycles per 4,800-frame turn moves at most ~7.5% of the peak
        // per sample; allow filter transients double that.
        assert!(
            max_jump < peak * 0.15,
            "ghost is stair-stepped: jump {max_jump} against peak {peak}"
        );
    }

    #[test]
    fn pinch_makes_width_out_of_a_mono_cut() {
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_PINCH, 1.0);
        let frames = 4_800_usize;
        // Dead centre: nothing but lateral motion, no side at all.
        let mut samples: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let value = (i as f64 / frames as f64 * TAU * 40.0).sin() as f32 * 0.6;
                [value, value]
            })
            .collect();
        processor.process_interleaved(&mut samples, 2, context(0.0, frames));
        let side = samples
            .chunks_exact(2)
            .fold(0.0_f32, |peak, frame| peak.max((frame[0] - frame[1]).abs()));
        assert!(side > 0.05, "a mono cut should ride its way into width");
        let peak = samples.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
        assert!(peak <= 1.0);

        // The width it makes is an octave, not a fuzzbox. Rectifying used to
        // put a corner in the signal, and a corner is every even harmonic at
        // once — most of them past Nyquist and folding back. A squared,
        // band-limited swing keeps the side smooth: consecutive samples move
        // by about what a 80 Hz tone moves by, not by a step.
        let side_track: Vec<f32> = samples
            .chunks_exact(2)
            .map(|frame| (frame[0] - frame[1]) * 0.5)
            .collect();
        //
        // How fast a signal moves against how big it is places where its
        // energy sits. A clean octave of a 400 Hz cut is 800 Hz, which moves
        // about a tenth of its own height per sample; a rectified corner
        // sprays energy far above that.
        let energy = |track: &[f32]| -> f64 {
            (track.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>()
                / track.len() as f64)
                .sqrt()
        };
        let steps: Vec<f32> = side_track
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect();
        let brightness = energy(&steps) / energy(&side_track).max(1.0e-9);
        assert!(
            brightness < 0.2,
            "the ride should be an octave, not a fuzzbox: {brightness}"
        );
    }

    #[test]
    fn null_points_are_clean_and_the_edge_is_not() {
        fn grit(radius_progress: f64) -> f32 {
            let mut processor = VinylVfxProcessor::new();
            processor.set_scene(VINYL_VFX_NULL_POINTS, 1.0);
            let frames = 4_800_usize;
            let mut samples: Vec<f32> = (0..frames)
                .flat_map(|i| {
                    let value = (i as f64 / frames as f64 * TAU * 40.0).sin() as f32 * 0.6;
                    [value, value]
                })
                .collect();
            let dry = samples.clone();
            let mut context = context(0.0, frames);
            // Park the needle at one radius for the whole block.
            context.start_position = radius_progress * frames as f64 * 4.0;
            context.end_position = context.start_position;
            processor.process_interleaved(&mut samples, 2, context);
            samples
                .iter()
                .zip(dry.iter())
                .fold(0.0_f32, |peak, (wet, dry)| peak.max((wet - dry).abs()))
        }
        // Tangent at the nulls, and worst out at the edge.
        assert!(grit(NULL_POINT_OUTER) < 0.001);
        assert!(grit(NULL_POINT_INNER) < 0.001);
        assert!(grit(0.0) > grit(0.6));
        assert!(grit(0.6) > grit(NULL_POINT_OUTER));
    }

    #[test]
    fn overcut_cuts_its_own_pass_back_into_the_groove() {
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_OVERCUT, 1.0);
        let frames = 4_800_usize;
        let loud = vec![0.4_f32; frames * 2];
        // One turn of programme, then silence over the same groove twice.
        let mut first = loud.clone();
        processor.process_interleaved(&mut first, 2, context(0.0, frames));
        let mut second = vec![0.0_f32; frames * 2];
        processor.process_interleaved(&mut second, 2, context(1.0, frames));
        let mut third = vec![0.0_f32; frames * 2];
        processor.process_interleaved(&mut third, 2, context(2.0, frames));
        let second_peak = second.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
        let third_peak = third.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
        // The second turn hears the first, and the third hears the second —
        // which it could only do if the pass was written back.
        assert!(second_peak > 0.05);
        assert!(third_peak > 0.05);
        assert!(third.iter().all(|sample| sample.abs() <= 1.0));
    }

    #[test]
    fn split_walls_keeps_stereo_channels_distinct() {
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_SPLIT_WALLS, 1.0);
        let mut warmup = Vec::with_capacity(9_600);
        for _ in 0..4_800 {
            warmup.extend_from_slice(&[0.7, -0.2]);
        }
        processor.process_interleaved(&mut warmup, 2, context(0.0, 4_800));
        let mut output = warmup.clone();
        processor.process_interleaved(&mut output, 2, context(1.0, 4_800));
        assert!(output.chunks_exact(2).any(|frame| frame[0] != frame[1]));
    }

    #[test]
    fn inner_fire_is_audible_but_not_blown_out() {
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_INNER_FIRE, 1.0);
        let frames = 4_800_usize;
        let mut samples: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let value = (i as f64 / frames as f64 * TAU * 48.0).sin() as f32 * 0.5;
                [value, value]
            })
            .collect();
        let dry = samples.clone();
        processor.process_interleaved(&mut samples, 2, context(0.0, frames));
        let wet_peak = samples.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
        let dry_peak = dry.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
        assert!(
            samples
                .iter()
                .zip(dry.iter())
                .any(|(a, b)| (a - b).abs() > 0.02),
            "fire did nothing"
        );
        assert!(
            wet_peak < dry_peak * 1.6,
            "fire blew out: {wet_peak} against dry {dry_peak}"
        );
    }

    /// One revolution at 33 1/3 rpm and 48 kHz, and a programme whose every
    /// partial is a whole number of cycles in it. The loop joins seamlessly,
    /// so anything sub-audio in the output was made by the scene.
    const REVOLUTION: usize = 86_400;

    fn revolution_programme() -> Vec<f32> {
        let bin = |k: f64| k / 1.8;
        (0..REVOLUTION)
            .flat_map(|i| {
                let t = i as f64 / 48_000.0;
                let mono = (t * TAU * bin(99.0)).sin() * 0.45
                    + (t * TAU * bin(148.0)).sin() * 0.20
                    + (t * TAU * bin(396.0)).sin() * 0.16
                    + (t * TAU * bin(594.0)).sin() * 0.12;
                let side = (t * TAU * bin(5_580.0)).sin() * 0.05;
                [(mono + side) as f32, (mono - side) as f32]
            })
            .collect()
    }

    fn locked_groove(turn: usize) -> VinylVfxContext {
        VinylVfxContext {
            start_turns: turn as f64,
            end_turns: turn as f64 + 1.0,
            start_position: 0.55 * REVOLUTION as f64 * 40.0,
            end_position: 0.55 * REVOLUTION as f64 * 40.0,
            total_frames: REVOLUTION * 40,
            ..VinylVfxContext::default()
        }
    }

    /// What is left of a track under 20 Hz. Six poles, so a 55 Hz bass does
    /// not leak into the reading.
    fn sub_audio(track: &[f64]) -> f64 {
        let alpha = 1.0 - (-TAU * 20.0 / 48_000.0f64).exp();
        let mut state = [0.0f64; 6];
        let mut sum = 0.0;
        for sample in track {
            state[0] += (sample - state[0]) * alpha;
            for stage in 1..6 {
                state[stage] += (state[stage - 1] - state[stage]) * alpha;
            }
            sum += state[5] * state[5];
        }
        (sum / track.len() as f64).sqrt()
    }

    fn mid_of(frames: &[f32]) -> Vec<f64> {
        frames
            .chunks_exact(2)
            .map(|f| (f64::from(f[0]) + f64::from(f[1])) * 0.5)
            .collect()
    }

    fn side_of(frames: &[f32]) -> Vec<f64> {
        frames
            .chunks_exact(2)
            .map(|f| (f64::from(f[0]) - f64::from(f[1])) * 0.5)
            .collect()
    }

    fn last_turn_of(scene: u32, turns: usize) -> Vec<f32> {
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(scene, 1.0);
        let mut last = Vec::new();
        for turn in 0..turns {
            let mut wet = revolution_programme();
            processor.process_interleaved(&mut wet, 2, locked_groove(turn));
            last = wet;
        }
        last
    }

    #[test]
    fn pinch_keeps_the_sub_audio_out_of_the_sides() {
        // Squaring a band returns every difference as well as every sum, and
        // a dense band puts most of its pairs close together. The ride goes
        // in antiphase, so without a floor under it that pile arrives as
        // sub-audio in the side channel — which is the one place a record
        // may never carry bass, and the one a headphone reproduces.
        let wet = last_turn_of(VINYL_VFX_PINCH, 6);
        let side = sub_audio(&side_of(&wet));
        let mid = sub_audio(&mid_of(&wet));
        assert!(
            side < 0.005,
            "pinch put sub-audio in the sides: {side} against {mid} in the middle"
        );
        // And it is still the scene. Width is how much side the cut carries
        // against its own middle, not against the level it came in at: the
        // scene steps the cut back by `PINCH_HEADROOM` to make room for the
        // ride, so a reading taken against the input measures that trim
        // rather than the widening.
        let spread = |frames: &[f32]| -> f64 {
            let sum = |v: Vec<f64>| v.iter().map(|s| s.abs()).sum::<f64>();
            sum(side_of(frames)) / sum(mid_of(frames)).max(1.0e-12)
        };
        let width = spread(&wet) / spread(&revolution_programme());
        assert!(width > 1.2, "pinch stopped making width: {width}");
    }

    #[test]
    fn overcut_does_not_sum_its_own_offset() {
        // The loop's lowpass passes DC at unity. With no floor the pass adds
        // its own offset and everything under the turn rate to itself, at
        // `layer` a revolution, and twelve turns of that is a rumble.
        let wet = last_turn_of(VINYL_VFX_OVERCUT, 12);
        let dry = revolution_programme();
        let mid = sub_audio(&mid_of(&wet));
        let dry_mid = sub_audio(&mid_of(&dry));
        assert!(
            mid < dry_mid * 3.0,
            "overcut piled up under the music: {mid} against {dry_mid} dry"
        );
        // The layers are still there: the scene departs from what it was
        // handed, and stays bounded while it does.
        let departure = wet
            .iter()
            .zip(dry.iter())
            .fold(0.0_f32, |peak, (w, d)| peak.max((w - d).abs()));
        assert!(departure > 0.1, "overcut stopped layering: {departure}");
        assert!(wet.iter().all(|sample| sample.abs() <= 1.0));
    }

    #[test]
    fn worn_halo_wears_in_seconds_rather_than_hours() {
        // A wear bin is one angle of the revolution, so the stylus is over
        // it for a fraction of each pass. The rate has to carry that factor:
        // without it the scene needs eleven hours on one locked groove to
        // reach the wear its constant asks for in twenty seconds.
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_WORN_HALO, 1.0);
        let dry = revolution_programme();
        let rms = |v: &[f32]| -> f64 {
            (v.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / v.len() as f64).sqrt()
        };
        let departure = |wet: &[f32]| -> f64 {
            let delta: Vec<f32> = wet.iter().zip(dry.iter()).map(|(w, d)| w - d).collect();
            rms(&delta) / rms(&dry)
        };
        // A revolution is 1.8 s, so twelve of them is a little over twenty.
        let mut early = 0.0;
        let mut late = 0.0;
        for turn in 0..12 {
            let mut wet = revolution_programme();
            processor.process_interleaved(&mut wet, 2, locked_groove(turn));
            if turn == 0 {
                early = departure(&wet);
            }
            late = departure(&wet);
        }
        assert!(early > 0.004, "the first pass left no mark at all: {early}");
        assert!(
            late > early * 2.5,
            "the halo stopped building: {early} then {late}"
        );
    }

    #[test]
    fn overcut_converges_instead_of_running_away() {
        // The scene is meant to build: LAYERS asks the groove to carry more
        // of its own history, and on a locked groove these sines repeat
        // exactly, so this is the most the loop can ever be handed.
        //
        // What must hold is that it arrives somewhere. A loop whose return
        // reaches one never decays, and the old arithmetic was still
        // climbing at the twelfth revolution with the clamp doing the rest.
        let dry: Vec<f32> = revolution_programme().iter().map(|s| s * 0.55).collect();
        let rms = |v: &[f32]| -> f64 {
            (v.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / v.len() as f64).sqrt()
        };
        let peak = |v: &[f32]| v.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_OVERCUT, 1.0);
        let mut first = 0.0;
        let mut sixth = 0.0;
        let mut twelfth = 0.0;
        let mut top = 0.0_f32;
        for turn in 0..12 {
            let mut wet = dry.clone();
            processor.process_interleaved(&mut wet, 2, locked_groove(turn));
            top = top.max(peak(&wet));
            match turn {
                0 => first = rms(&wet) / rms(&dry),
                5 => sixth = rms(&wet) / rms(&dry),
                11 => twelfth = rms(&wet) / rms(&dry),
                _ => {}
            }
        }
        // Settled: the last half of the run is going nowhere.
        let drift = (twelfth - sixth).abs() / sixth;
        assert!(
            drift < 0.06,
            "overcut had not settled by the twelfth turn: {sixth} then {twelfth}"
        );
        // It builds, and it arrives.
        assert!(twelfth > first * 1.2, "overcut stopped layering: {first} then {twelfth}");
        // The knee holds it clear of the ceiling without clamping.
        assert!(top < 0.99, "overcut reached the ceiling: {top}");
    }
    #[test]
    fn the_elliptical_circuit_takes_wide_bass_to_the_middle() {
        // A cutter head cannot move the stylus far enough vertically to hold
        // bass that exists only in the sides. This is a record that asks it
        // to: a 55 Hz tone in antiphase, with the rest of the programme down
        // the middle where it belongs.
        const TURN: usize = 86_400;
        let wide: Vec<f32> = (0..TURN)
            .flat_map(|i| {
                let t = i as f64 / 48_000.0;
                let middle = (t * TAU * 220.0).sin() * 0.30 + (t * TAU * 660.0).sin() * 0.12;
                let bass = (t * TAU * 55.0).sin() * 0.40;
                [(middle + bass) as f32, (middle - bass) as f32]
            })
            .collect();

        // Everything under 200 Hz that the sides are carrying.
        let low_side = |frames: &[f32]| -> f64 {
            let alpha = 1.0 - (-TAU * 200.0 / 48_000.0f64).exp();
            let mut state = [0.0f64; 4];
            let mut sum = 0.0;
            for pair in frames.chunks_exact(2) {
                let side = (f64::from(pair[0]) - f64::from(pair[1])) * 0.5;
                state[0] += (side - state[0]) * alpha;
                for stage in 1..4 {
                    state[stage] += (state[stage - 1] - state[stage]) * alpha;
                }
                sum += state[3] * state[3];
            }
            (sum / (frames.len() / 2) as f64).sqrt()
        };

        let before = low_side(&wide);
        let mut processor = VinylVfxProcessor::new();
        processor.set_scene(VINYL_VFX_OVERCUT, 1.0);
        let mut wet = wide.clone();
        processor.process_interleaved(&mut wet, 2, locked_groove(0));
        let after = low_side(&wet);

        // The loop would otherwise add to this, not take from it.
        assert!(
            after < before * 0.5,
            "the sides kept their bass: {before} then {after}"
        );
        // And the middle is left alone: this is a circuit for the sides.
        let middle = |frames: &[f32]| -> f64 {
            let m: Vec<f64> = frames
                .chunks_exact(2)
                .map(|p| (f64::from(p[0]) + f64::from(p[1])) * 0.5)
                .collect();
            (m.iter().map(|s| s * s).sum::<f64>() / m.len() as f64).sqrt()
        };
        let kept = middle(&wet) / middle(&wide);
        assert!(kept > 0.8, "the circuit took the middle with it: {kept}");
    }
}
