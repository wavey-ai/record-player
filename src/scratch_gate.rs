use std::fmt;
use std::str::FromStr;

pub const MIN_SCRATCH_CLICKS: u8 = 1;
pub const MAX_SCRATCH_CLICKS: u8 = 8;
pub const SCRATCH_GATE_ALGORITHM_VERSION: u32 = 5;
const TECHNIQUE_SCOPED_CLICKS_GATE_VERSION: u32 = 4;
const CONFIRMED_TRAVEL_GATE_VERSION: u32 = 5;

const MOTION_ONSET_RATE: f64 = 0.035;
const REST_RATE: f64 = 0.018;
const INTENT_REVERSAL_RATE: f64 = 0.055;
const ONSET_CONFIRM_SECONDS: f64 = 0.004;
const REVERSAL_CONFIRM_SECONDS: f64 = 0.006;
const REST_CONFIRM_SECONDS: f64 = 0.012;
const MIN_LEARNED_SPAN: f64 = 0.04;
const MAX_LEARNED_SPAN: f64 = 0.80;
const DRUM_MAX_OPEN_SECONDS: f64 = 0.055;
const DRUM_OPEN_SPAN_FRACTION: f64 = 0.18;
const DRUM_ACCELERATION_FILTER_SECONDS: f64 = 0.008;
const DRUM_ACCELERATION_TRIGGER: f64 = 6.0;
const DRUM_TRIGGER_MIN_RATE: f64 = 0.14;
const DRUM_REFRACTORY_SECONDS: f64 = 0.045;

/// Intent-aware automatic crossfader patterns for common scratch techniques.
///
/// `Baby` leaves the stored manual crossfader in control. Every other preset
/// owns the real audible crossfader through this gate. The host can still keep
/// the manual position intact for an immediate return to `Baby`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScratchPreset {
    #[default]
    Baby,
    Stab,
    Chirp,
    Transform,
    Flare,
    Crab,
    Orbit,
    Drum,
}

impl ScratchPreset {
    pub const ALL: [Self; 8] = [
        Self::Baby,
        Self::Stab,
        Self::Chirp,
        Self::Transform,
        Self::Flare,
        Self::Crab,
        Self::Orbit,
        Self::Drum,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Baby => "baby",
            Self::Stab => "stab",
            Self::Chirp => "chirp",
            Self::Transform => "transform",
            Self::Flare => "flare",
            Self::Crab => "crab",
            Self::Orbit => "orbit",
            Self::Drum => "drum",
        }
    }

    pub const fn default_clicks(self) -> u8 {
        match self {
            Self::Baby | Self::Stab | Self::Chirp | Self::Flare | Self::Drum => 1,
            Self::Transform | Self::Orbit => 2,
            Self::Crab => 4,
        }
    }

    /// Reports whether click count changes this technique's audible pattern.
    pub const fn uses_clicks(self) -> bool {
        matches!(
            self,
            Self::Transform | Self::Flare | Self::Crab | Self::Orbit
        )
    }

    /// Seed stroke span in source seconds. It adapts at each confirmed
    /// reversal, but a useful seed makes the first stroke musical too.
    pub const fn initial_stroke_span(self) -> f64 {
        match self {
            Self::Baby | Self::Stab | Self::Chirp => 0.22,
            Self::Transform => 2.0 / 9.5,
            Self::Flare => 1.0 / 6.4,
            Self::Crab => 4.0 / 18.0,
            Self::Orbit => 2.0 / 7.2,
            Self::Drum => 0.12,
        }
    }
}

impl fmt::Display for ScratchPreset {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScratchPresetParseError(String);

impl fmt::Display for ScratchPresetParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown scratch preset {:?}", self.0)
    }
}

impl std::error::Error for ScratchPresetParseError {}

impl FromStr for ScratchPreset {
    type Err = ScratchPresetParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "baby" => Ok(Self::Baby),
            "stab" => Ok(Self::Stab),
            "chirp" => Ok(Self::Chirp),
            "transform" => Ok(Self::Transform),
            "flare" => Ok(Self::Flare),
            "crab" => Ok(Self::Crab),
            "orbit" => Ok(Self::Orbit),
            "drum" => Ok(Self::Drum),
            _ => Err(ScratchPresetParseError(value.to_owned())),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MotionEvent {
    None,
    Onset,
    Resume,
    Reversal,
    Rest,
}

/// Audio-rate, travel-based scratch crossfader gate.
///
/// `intent_rate` should be the filtered/target hand rate and
/// `rendered_rate` should be the rate actually used to advance audible PCM.
/// Direction changes use intent for responsiveness and rendered motion as a
/// fallback, while pattern phase follows audible travel in the confirmed
/// stroke direction.
#[derive(Clone, Debug)]
pub struct ScratchGate {
    preset: ScratchPreset,
    clicks: u8,
    algorithm_version: u32,
    contact_active: bool,
    direction: i8,
    moving: bool,
    pending_direction: i8,
    pending_seconds: f64,
    pending_stroke_travel: f64,
    rest_seconds: f64,
    stroke_travel: f64,
    learned_span: f64,
    phase: f64,
    gate: f64,
    target: f64,
    drum_filtered_intent_rate: f64,
    drum_filter_delta_seconds: f64,
    drum_filter_alpha: f64,
    drum_open: bool,
    drum_elapsed: f64,
    drum_travel: f64,
    drum_refractory: f64,
}

impl Default for ScratchGate {
    fn default() -> Self {
        Self::new(ScratchPreset::Baby)
    }
}

impl ScratchGate {
    pub fn new(preset: ScratchPreset) -> Self {
        Self {
            preset,
            clicks: preset.default_clicks(),
            algorithm_version: SCRATCH_GATE_ALGORITHM_VERSION,
            contact_active: false,
            direction: 0,
            moving: false,
            pending_direction: 0,
            pending_seconds: 0.0,
            pending_stroke_travel: 0.0,
            rest_seconds: 0.0,
            stroke_travel: 0.0,
            learned_span: preset.initial_stroke_span(),
            phase: 0.0,
            gate: 1.0,
            target: 1.0,
            drum_filtered_intent_rate: 0.0,
            drum_filter_delta_seconds: 0.0,
            drum_filter_alpha: 1.0,
            drum_open: false,
            drum_elapsed: 0.0,
            drum_travel: 0.0,
            drum_refractory: DRUM_REFRACTORY_SECONDS,
        }
    }

    pub fn preset(&self) -> ScratchPreset {
        self.preset
    }

    pub fn set_preset(&mut self, preset: ScratchPreset) {
        if preset != self.preset {
            self.learned_span = preset.initial_stroke_span();
        }
        self.preset = preset;
        self.clicks = preset.default_clicks();
        self.reset_phrase();
        self.target = 1.0;
    }

    pub fn clicks(&self) -> u8 {
        self.clicks
    }

    pub fn set_clicks(&mut self, clicks: u8) {
        self.clicks = clicks.clamp(MIN_SCRATCH_CLICKS, MAX_SCRATCH_CLICKS);
        self.update_phase();
    }

    pub fn algorithm_version(&self) -> u32 {
        self.algorithm_version
    }

    pub fn set_algorithm_version(&mut self, version: u32) {
        self.algorithm_version = version.clamp(1, SCRATCH_GATE_ALGORITHM_VERSION);
        self.update_phase();
    }

    /// Starts a recorded performance from one defined gate state while
    /// retaining its selected technique and click count.
    pub fn reset_for_replay(&mut self) {
        let preset = self.preset;
        let clicks = self.clicks;
        let algorithm_version = self.algorithm_version;
        *self = Self::new(preset);
        self.set_algorithm_version(algorithm_version);
        self.set_clicks(clicks);
    }

    pub fn gate(&self) -> f64 {
        self.gate
    }

    pub fn target(&self) -> f64 {
        self.target
    }

    pub fn direction(&self) -> i8 {
        self.direction
    }

    pub fn moving(&self) -> bool {
        self.moving
    }

    pub fn phase(&self) -> f64 {
        self.phase
    }

    pub fn stroke_progress(&self) -> f64 {
        (self.stroke_travel / self.learned_span.max(MIN_LEARNED_SPAN)).clamp(0.0, 1.0)
    }

    pub fn learned_span(&self) -> f64 {
        self.learned_span
    }

    /// Marks the record as released immediately. The target opens at once;
    /// the returned gain still reaches it through the de-click envelope.
    pub fn release(&mut self) {
        if self.contact_active {
            self.reset_phrase();
        }
        self.contact_active = false;
        self.target = 1.0;
    }

    /// Advances the gate by one audio frame and returns its de-clicked gain.
    pub fn process(
        &mut self,
        delta_seconds: f64,
        hand_contact: bool,
        intent_rate: f64,
        rendered_rate: f64,
    ) -> f64 {
        let dt = finite_nonnegative(delta_seconds).min(0.1);
        let intent_rate = finite_or_zero(intent_rate);
        let rendered_rate = finite_or_zero(rendered_rate);

        self.drum_refractory += dt;
        if !hand_contact {
            self.release();
            self.advance_envelope(dt, intent_rate, rendered_rate);
            return self.gate;
        }

        if !self.contact_active {
            self.contact_active = true;
            self.reset_phrase();
        }

        let (event, confirmed_travel) = self.update_motion(dt, intent_rate, rendered_rate);
        let preserves_confirmed_travel = self.algorithm_version >= CONFIRMED_TRAVEL_GATE_VERSION;
        if event == MotionEvent::Reversal {
            self.learn_completed_stroke();
            self.stroke_travel = if preserves_confirmed_travel {
                confirmed_travel
            } else {
                0.0
            };
            self.phase = 0.0;
        } else if preserves_confirmed_travel
            && matches!(event, MotionEvent::Onset | MotionEvent::Resume)
        {
            self.stroke_travel += confirmed_travel;
        }

        let rendered_stroke_speed = if self.moving && sign(rendered_rate) == self.direction {
            rendered_rate.abs()
        } else {
            0.0
        };
        if self.moving {
            let current_frame_is_already_confirmed = preserves_confirmed_travel
                && matches!(
                    event,
                    MotionEvent::Onset | MotionEvent::Resume | MotionEvent::Reversal
                );
            if !current_frame_is_already_confirmed {
                self.stroke_travel += rendered_stroke_speed * dt;
            }
            self.update_phase();
        }

        if self.preset == ScratchPreset::Drum {
            let acceleration = self.filter_drum_acceleration(dt, intent_rate);
            self.update_drum(
                event,
                dt,
                rendered_rate,
                rendered_stroke_speed,
                intent_rate,
                acceleration,
            );
        }
        self.target = self.compute_target(intent_rate, rendered_rate);
        self.advance_envelope(dt, intent_rate, rendered_rate);
        self.gate
    }

    fn reset_phrase(&mut self) {
        self.direction = 0;
        self.moving = false;
        self.pending_direction = 0;
        self.pending_seconds = 0.0;
        self.pending_stroke_travel = 0.0;
        self.rest_seconds = 0.0;
        self.stroke_travel = 0.0;
        self.phase = 0.0;
        self.drum_filtered_intent_rate = 0.0;
        self.drum_open = false;
        self.drum_elapsed = 0.0;
        self.drum_travel = 0.0;
        self.drum_refractory = DRUM_REFRACTORY_SECONDS;
    }

    fn update_motion(
        &mut self,
        dt: f64,
        intent_rate: f64,
        rendered_rate: f64,
    ) -> (MotionEvent, f64) {
        let both_at_rest = intent_rate.abs() <= REST_RATE && rendered_rate.abs() <= REST_RATE;
        if both_at_rest {
            self.rest_seconds += dt;
            self.clear_pending_direction();
            if self.moving && self.rest_seconds >= REST_CONFIRM_SECONDS {
                self.moving = false;
                return (MotionEvent::Rest, 0.0);
            }
            return (MotionEvent::None, 0.0);
        }
        self.rest_seconds = 0.0;

        let candidate = self.direction_candidate(intent_rate, rendered_rate);
        if candidate == 0 {
            self.clear_pending_direction();
            return (MotionEvent::None, 0.0);
        }

        if self.moving && candidate == self.direction {
            self.clear_pending_direction();
            return (MotionEvent::None, 0.0);
        }

        let confirmation_seconds = if self.direction != 0 && candidate != self.direction {
            REVERSAL_CONFIRM_SECONDS
        } else {
            ONSET_CONFIRM_SECONDS
        };
        if self.pending_direction == candidate {
            self.pending_seconds += dt;
        } else {
            self.pending_direction = candidate;
            self.pending_seconds = dt;
            self.pending_stroke_travel = 0.0;
        }
        if sign(rendered_rate) == candidate {
            self.pending_stroke_travel += rendered_rate.abs() * dt;
        }
        if self.pending_seconds < confirmation_seconds {
            return (MotionEvent::None, 0.0);
        }

        let confirmed_travel = self.pending_stroke_travel;
        self.clear_pending_direction();
        let event = if self.direction == 0 {
            self.direction = candidate;
            self.moving = true;
            MotionEvent::Onset
        } else if candidate != self.direction {
            self.direction = candidate;
            self.moving = true;
            MotionEvent::Reversal
        } else {
            self.moving = true;
            MotionEvent::Resume
        };
        (event, confirmed_travel)
    }

    fn direction_candidate(&self, intent_rate: f64, rendered_rate: f64) -> i8 {
        let intent_threshold =
            if self.direction != 0 && sign(intent_rate) != 0 && sign(intent_rate) != self.direction
            {
                INTENT_REVERSAL_RATE
            } else {
                MOTION_ONSET_RATE
            };
        if intent_rate.abs() >= intent_threshold {
            return sign(intent_rate);
        }
        if rendered_rate.abs() >= MOTION_ONSET_RATE {
            return sign(rendered_rate);
        }
        0
    }

    fn clear_pending_direction(&mut self) {
        self.pending_direction = 0;
        self.pending_seconds = 0.0;
        self.pending_stroke_travel = 0.0;
    }

    fn learn_completed_stroke(&mut self) {
        if self.stroke_travel <= 0.0 {
            return;
        }
        let observed = self.stroke_travel.clamp(MIN_LEARNED_SPAN, MAX_LEARNED_SPAN);
        self.learned_span =
            (self.learned_span * 0.70 + observed * 0.30).clamp(MIN_LEARNED_SPAN, MAX_LEARNED_SPAN);
    }

    fn update_phase(&mut self) {
        // Versions 1–3 applied the click multiplier to every preset. Preserve
        // that behavior only while replaying a take recorded by those gates.
        let phase_clicks = if self.algorithm_version < TECHNIQUE_SCOPED_CLICKS_GATE_VERSION
            || self.preset.uses_clicks()
        {
            self.clicks
        } else {
            1
        };
        let cycles =
            self.stroke_travel / self.learned_span.max(MIN_LEARNED_SPAN) * f64::from(phase_clicks);
        self.phase = cycles.rem_euclid(1.0);
    }

    fn update_drum(
        &mut self,
        event: MotionEvent,
        dt: f64,
        rendered_rate: f64,
        rendered_stroke_speed: f64,
        intent_rate: f64,
        acceleration: f64,
    ) {
        let speed = intent_rate.abs().max(rendered_rate.abs());
        let direction_trigger = matches!(event, MotionEvent::Onset | MotionEvent::Reversal)
            && speed >= DRUM_TRIGGER_MIN_RATE;
        let acceleration_trigger = self.moving
            && speed >= DRUM_TRIGGER_MIN_RATE
            && acceleration >= DRUM_ACCELERATION_TRIGGER
            && self.drum_refractory >= DRUM_REFRACTORY_SECONDS;
        if direction_trigger || acceleration_trigger {
            self.trigger_drum();
        }

        if self.drum_open {
            self.drum_elapsed += dt;
            self.drum_travel += rendered_stroke_speed * dt;
            if self.drum_elapsed >= DRUM_MAX_OPEN_SECONDS
                || self.drum_travel >= self.learned_span * DRUM_OPEN_SPAN_FRACTION
            {
                self.drum_open = false;
            }
        }
    }

    fn filter_drum_acceleration(&mut self, dt: f64, intent_rate: f64) -> f64 {
        if dt <= 0.0 {
            return 0.0;
        }
        // Pointer targets are piecewise constant between browser events. A raw
        // per-sample derivative would turn every target step into an impulse
        // whose size grows with the output sample rate. This one-pole intent
        // model gives the step a physical time base before differentiation.
        if dt != self.drum_filter_delta_seconds {
            self.drum_filter_delta_seconds = dt;
            self.drum_filter_alpha = 1.0 - (-dt / DRUM_ACCELERATION_FILTER_SECONDS).exp();
        }
        let previous = self.drum_filtered_intent_rate;
        self.drum_filtered_intent_rate += (intent_rate - previous) * self.drum_filter_alpha;
        ((self.drum_filtered_intent_rate - previous) / dt).abs()
    }

    fn trigger_drum(&mut self) {
        self.drum_open = true;
        self.drum_elapsed = 0.0;
        self.drum_travel = 0.0;
        self.drum_refractory = 0.0;
    }

    fn compute_target(&self, intent_rate: f64, rendered_rate: f64) -> f64 {
        let speed = intent_rate.abs().max(rendered_rate.abs());
        let confidence = smoothstep(MOTION_ONSET_RATE, 2.0, speed);
        match self.preset {
            ScratchPreset::Baby => 1.0,
            ScratchPreset::Stab => f64::from(self.moving && self.direction > 0),
            ScratchPreset::Chirp => {
                if !self.moving {
                    1.0
                } else if self.direction > 0 {
                    let close_at = lerp(0.32, 0.18, confidence);
                    f64::from(self.phase < close_at)
                } else {
                    let open_at = lerp(0.26, 0.14, confidence);
                    f64::from(self.phase >= open_at)
                }
            }
            ScratchPreset::Transform => f64::from(self.moving && self.phase < 0.48),
            ScratchPreset::Flare => {
                if !self.moving {
                    1.0
                } else {
                    f64::from(distance_from_midpoint(self.phase) >= 0.07)
                }
            }
            ScratchPreset::Crab => f64::from(self.moving && self.phase < 0.38),
            ScratchPreset::Orbit => {
                if !self.moving {
                    1.0
                } else {
                    f64::from(distance_from_midpoint(self.phase) >= 0.07)
                }
            }
            ScratchPreset::Drum => f64::from(self.drum_open),
        }
    }

    fn advance_envelope(&mut self, dt: f64, intent_rate: f64, rendered_rate: f64) {
        if dt <= 0.0 {
            return;
        }
        let speed = intent_rate.abs().max(rendered_rate.abs());
        let confidence = smoothstep(MOTION_ONSET_RATE, 2.0, speed);
        let time_constant = if self.target >= self.gate {
            lerp(0.0015, 0.00035, confidence)
        } else {
            lerp(0.0020, 0.00045, confidence)
        };
        let alpha = 1.0 - (-dt / time_constant).exp();
        self.gate = (self.gate + (self.target - self.gate) * alpha).clamp(0.0, 1.0);
    }
}

fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

fn finite_nonnegative(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

fn sign(value: f64) -> i8 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

fn smoothstep(edge0: f64, edge1: f64, value: f64) -> f64 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(start: f64, end: f64, amount: f64) -> f64 {
    start + (end - start) * amount
}

fn distance_from_midpoint(phase: f64) -> f64 {
    (phase - 0.5).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f64 = 48_000.0;

    fn run(gate: &mut ScratchGate, seconds: f64, contact: bool, intent: f64, rendered: f64) {
        let frames = (seconds * SAMPLE_RATE).round() as usize;
        for _ in 0..frames {
            gate.process(1.0 / SAMPLE_RATE, contact, intent, rendered);
        }
    }

    fn settle_direction(gate: &mut ScratchGate, direction: f64) {
        run(gate, 0.010, true, direction, direction);
        assert!(gate.moving());
        assert_eq!(gate.direction(), sign(direction));
    }

    fn count_target_runs(preset: ScratchPreset, clicks: u8, target: f64) -> usize {
        let mut gate = ScratchGate::new(preset);
        gate.set_clicks(clicks);
        gate.contact_active = true;
        gate.direction = 1;
        gate.moving = true;

        let rate = 1.0;
        let mut previous = gate.compute_target(rate, rate);
        let mut runs = usize::from(previous == target);
        let frames = (gate.learned_span() * SAMPLE_RATE).floor() as usize;
        for _ in 0..frames {
            gate.process(1.0 / SAMPLE_RATE, true, rate, rate);
            let current = gate.target();
            if current == target && previous != target {
                runs += 1;
            }
            previous = current;
        }
        runs
    }

    fn audio_fixture_sample(frame: usize, transient: bool) -> f64 {
        if transient {
            let age = frame % 257;
            if age < 96 {
                (-(age as f64) / 13.0).exp()
            } else {
                0.0
            }
        } else {
            (std::f64::consts::TAU * 997.0 * frame as f64 / SAMPLE_RATE).sin()
        }
    }

    #[test]
    fn parses_every_preset_and_exposes_defaults() {
        let expected_clicks = [1, 1, 1, 2, 1, 4, 2, 1];
        let uses_clicks = [false, false, false, true, true, true, true, false];
        for ((preset, clicks), expected_uses_clicks) in ScratchPreset::ALL
            .into_iter()
            .zip(expected_clicks)
            .zip(uses_clicks)
        {
            assert_eq!(preset.as_str().parse::<ScratchPreset>().unwrap(), preset);
            assert_eq!(preset.default_clicks(), clicks);
            assert_eq!(preset.uses_clicks(), expected_uses_clicks);
            assert!(preset.initial_stroke_span() >= MIN_LEARNED_SPAN);
        }
        assert!("scribble".parse::<ScratchPreset>().is_err());
    }

    #[test]
    fn clicks_are_clamped_to_the_supported_range() {
        let mut gate = ScratchGate::default();
        gate.set_clicks(0);
        assert_eq!(gate.clicks(), MIN_SCRATCH_CLICKS);
        gate.set_clicks(u8::MAX);
        assert_eq!(gate.clicks(), MAX_SCRATCH_CLICKS);
    }

    #[test]
    fn baby_stays_open_in_both_directions_and_at_rest() {
        let mut gate = ScratchGate::new(ScratchPreset::Baby);
        run(&mut gate, 0.05, true, 1.0, 1.0);
        assert_eq!(gate.target(), 1.0);
        run(&mut gate, 0.05, true, -1.0, -1.0);
        assert_eq!(gate.target(), 1.0);
        run(&mut gate, 0.05, true, 0.0, 0.0);
        assert_eq!(gate.target(), 1.0);
        assert!(gate.gate() > 0.999);
    }

    #[test]
    fn stab_opens_only_on_confirmed_forward_motion() {
        let mut gate = ScratchGate::new(ScratchPreset::Stab);
        settle_direction(&mut gate, 1.0);
        run(&mut gate, 0.008, true, 1.0, 1.0);
        assert_eq!(gate.target(), 1.0);
        assert!(gate.gate() > 0.95);

        run(&mut gate, 0.012, true, -1.0, -1.0);
        assert_eq!(gate.direction(), -1);
        assert_eq!(gate.target(), 0.0);
        run(&mut gate, 0.008, true, -1.0, -1.0);
        assert!(gate.gate() < 0.05);
    }

    #[test]
    fn jitter_does_not_create_a_false_reversal() {
        let mut gate = ScratchGate::new(ScratchPreset::Transform);
        settle_direction(&mut gate, 0.8);
        for _ in 0..20 {
            run(&mut gate, 0.001, true, -0.08, 0.4);
            run(&mut gate, 0.001, true, 0.4, 0.4);
        }
        assert_eq!(gate.direction(), 1);
        assert!(gate.moving());
    }

    #[test]
    fn intent_can_confirm_reversal_before_rendered_rate_crosses_zero() {
        let mut gate = ScratchGate::new(ScratchPreset::Transform);
        settle_direction(&mut gate, 0.8);
        run(&mut gate, 0.007, true, -0.7, 0.2);
        assert_eq!(gate.direction(), -1);
        assert!(gate.phase() < 0.02);
    }

    #[test]
    fn reversed_stroke_waits_for_audible_motion_in_the_new_direction() {
        let mut gate = ScratchGate::new(ScratchPreset::Transform);
        settle_direction(&mut gate, 0.8);
        run(&mut gate, 0.030, true, 0.8, 0.8);

        run(&mut gate, 0.007, true, -0.8, 0.3);
        assert_eq!(gate.direction(), -1);
        assert_eq!(gate.phase(), 0.0);

        run(&mut gate, 0.020, true, -0.8, 0.3);
        assert_eq!(gate.phase(), 0.0);

        run(&mut gate, 0.006, true, -0.8, -0.3);
        assert!(gate.phase() > 0.0);
    }

    #[test]
    fn confirmed_reversal_keeps_audible_travel_from_the_confirmation_window() {
        let trace = |version| {
            let mut gate = ScratchGate::new(ScratchPreset::Transform);
            gate.set_algorithm_version(version);
            settle_direction(&mut gate, 1.0);
            run(&mut gate, 0.030, true, 1.0, 1.0);
            run(&mut gate, 0.0061, true, -8.0, -8.0);
            assert_eq!(gate.direction(), -1);
            (gate.phase(), gate.stroke_progress())
        };

        let historical = trace(4);
        let current = trace(SCRATCH_GATE_ALGORITHM_VERSION);
        assert!(
            historical.1 < 0.02,
            "historical progress was {}",
            historical.1
        );
        assert!(current.1 > 0.25, "current progress was {}", current.1);
        assert!(current.0 > historical.0 + 0.35);
    }

    #[test]
    fn confirmed_onset_keeps_audible_travel_from_the_confirmation_window() {
        let trace = |version| {
            let mut gate = ScratchGate::new(ScratchPreset::Transform);
            gate.set_algorithm_version(version);
            run(&mut gate, 0.0041, true, 8.0, 8.0);
            assert_eq!(gate.direction(), 1);
            gate.stroke_progress()
        };

        let historical = trace(4);
        let current = trace(SCRATCH_GATE_ALGORITHM_VERSION);
        assert!(historical < 0.02, "historical progress was {historical}");
        assert!(current > 0.15, "current progress was {current}");
    }

    #[test]
    fn rejected_reversal_discards_its_buffered_travel() {
        let mut gate = ScratchGate::new(ScratchPreset::Transform);
        settle_direction(&mut gate, 1.0);
        run(&mut gate, 0.030, true, 1.0, 1.0);
        let travel_before_jitter = gate.stroke_travel;
        let phase_before_jitter = gate.phase();

        run(&mut gate, 0.003, true, -8.0, -8.0);
        assert_eq!(gate.direction(), 1);
        assert_eq!(gate.stroke_travel, travel_before_jitter);
        assert_eq!(gate.phase(), phase_before_jitter);
        assert!(gate.pending_stroke_travel > 0.02);

        run(&mut gate, 0.001, true, 1.0, 1.0);
        assert_eq!(gate.pending_stroke_travel, 0.0);
        assert!(gate.stroke_travel > travel_before_jitter);
        assert!(gate.stroke_travel < travel_before_jitter + 0.002);
    }

    #[test]
    fn confirmed_reversal_travel_is_sample_rate_invariant() {
        let trace = |sample_rate: f64| {
            let run_at_rate = |gate: &mut ScratchGate, seconds: f64, rate: f64| {
                let frames = (seconds * sample_rate).round() as usize;
                for _ in 0..frames {
                    gate.process(1.0 / sample_rate, true, rate, rate);
                }
            };
            let mut gate = ScratchGate::new(ScratchPreset::Transform);
            run_at_rate(&mut gate, 0.010, 1.0);
            run_at_rate(&mut gate, 0.030, 1.0);
            run_at_rate(&mut gate, 0.0065, -8.0);
            assert_eq!(gate.direction(), -1);
            (gate.phase(), gate.stroke_progress())
        };

        let at_44 = trace(44_100.0);
        let at_48 = trace(48_000.0);
        let at_96 = trace(96_000.0);
        for (left, right) in [(at_44, at_48), (at_48, at_96)] {
            assert!((left.0 - right.0).abs() < 0.004, "{left:?} != {right:?}");
            assert!((left.1 - right.1).abs() < 0.002, "{left:?} != {right:?}");
        }
    }

    #[test]
    fn rendered_rate_is_a_direction_fallback() {
        let mut gate = ScratchGate::new(ScratchPreset::Transform);
        run(&mut gate, 0.006, true, 0.0, -0.7);
        assert_eq!(gate.direction(), -1);
        assert!(gate.moving());
    }

    #[test]
    fn rest_freezes_phase_and_same_direction_resume_preserves_it() {
        let mut gate = ScratchGate::new(ScratchPreset::Transform);
        settle_direction(&mut gate, 0.8);
        run(&mut gate, 0.025, true, 0.8, 0.8);
        let phase_before_rest = gate.phase();
        run(&mut gate, 0.030, true, 0.0, 0.0);
        assert!(!gate.moving());
        assert_eq!(gate.phase(), phase_before_rest);

        run(&mut gate, 0.006, true, 0.8, 0.8);
        assert_eq!(gate.direction(), 1);
        assert!(gate.phase() >= phase_before_rest);
    }

    #[test]
    fn reversal_resets_phase_and_adapts_stroke_span() {
        let mut gate = ScratchGate::new(ScratchPreset::Transform);
        settle_direction(&mut gate, 1.0);
        run(&mut gate, 0.30, true, 1.0, 1.0);
        let initial_span = gate.learned_span();
        run(&mut gate, 0.0061, true, -1.0, -1.0);
        assert_eq!(gate.direction(), -1);
        assert!(gate.phase() > 0.04 && gate.phase() < 0.08);
        assert!(gate.learned_span() > initial_span);
    }

    #[test]
    fn transform_phase_tracks_travel_and_click_count() {
        let mut one_click = ScratchGate::new(ScratchPreset::Transform);
        one_click.set_clicks(1);
        settle_direction(&mut one_click, 1.0);
        run(&mut one_click, 0.030, true, 1.0, 1.0);

        let mut two_clicks = ScratchGate::new(ScratchPreset::Transform);
        two_clicks.set_clicks(2);
        settle_direction(&mut two_clicks, 1.0);
        run(&mut two_clicks, 0.030, true, 1.0, 1.0);
        assert!((two_clicks.phase() - (2.0 * one_click.phase()).rem_euclid(1.0)).abs() < 1e-9);

        let phase_before = one_click.phase();
        run(&mut one_click, 0.010, true, 2.0, 2.0);
        assert!(one_click.phase() - phase_before > 0.08);
    }

    #[test]
    fn click_driven_techniques_create_one_pulse_or_notch_per_selected_click() {
        for clicks in [1, 4, 8] {
            for preset in [ScratchPreset::Transform, ScratchPreset::Crab] {
                assert_eq!(
                    count_target_runs(preset, clicks, 1.0),
                    usize::from(clicks),
                    "{preset:?} at {clicks} clicks"
                );
            }
            for preset in [ScratchPreset::Flare, ScratchPreset::Orbit] {
                assert_eq!(
                    count_target_runs(preset, clicks, 0.0),
                    usize::from(clicks),
                    "{preset:?} at {clicks} clicks"
                );
            }
        }
    }

    #[test]
    fn current_gate_ignores_click_count_for_non_click_techniques() {
        for preset in [
            ScratchPreset::Baby,
            ScratchPreset::Stab,
            ScratchPreset::Chirp,
            ScratchPreset::Drum,
        ] {
            let trace = |clicks| {
                let mut gate = ScratchGate::new(preset);
                gate.set_clicks(clicks);
                run(&mut gate, 0.043, true, 0.9, 0.9);
                (gate.phase(), gate.target(), gate.gate())
            };
            let one = trace(1);
            let eight = trace(8);
            assert_eq!(eight, one, "{preset:?}");
        }
    }

    #[test]
    fn historical_gate_replay_retains_global_click_phase() {
        let trace = |version, clicks| {
            let mut gate = ScratchGate::new(ScratchPreset::Chirp);
            gate.set_algorithm_version(version);
            gate.set_clicks(clicks);
            run(&mut gate, 0.043, true, 0.9, 0.9);
            gate.phase()
        };
        let current_one = trace(SCRATCH_GATE_ALGORITHM_VERSION, 1);
        let current_eight = trace(SCRATCH_GATE_ALGORITHM_VERSION, 8);
        assert_eq!(current_eight, current_one);

        let historical_one = trace(3, 1);
        let historical_eight = trace(3, 8);
        assert_ne!(historical_eight, historical_one);
    }

    #[test]
    fn travel_patterns_keep_groove_landmarks_at_different_velocities() {
        for preset in [
            ScratchPreset::Transform,
            ScratchPreset::Flare,
            ScratchPreset::Crab,
            ScratchPreset::Orbit,
        ] {
            let trace = |rate: f64| {
                let mut gate = ScratchGate::new(preset);
                gate.contact_active = true;
                gate.direction = 1;
                gate.moving = true;
                run(&mut gate, 0.020 / rate, true, rate, rate);
                (gate.phase(), gate.target())
            };
            let slow = trace(0.5);
            let fast = trace(2.0);
            assert!((slow.0 - fast.0).abs() < 1e-9, "{preset:?}");
            assert_eq!(slow.1, fast.1, "{preset:?}");
        }
    }

    #[test]
    fn chirp_uses_velocity_and_direction_to_tighten_its_cut() {
        let mut gate = ScratchGate::new(ScratchPreset::Chirp);
        gate.contact_active = true;
        gate.moving = true;
        gate.phase = 0.24;

        gate.direction = 1;
        assert_eq!(gate.compute_target(0.04, 0.04), 1.0);
        assert_eq!(gate.compute_target(2.0, 2.0), 0.0);

        gate.direction = -1;
        assert_eq!(gate.compute_target(-0.04, -0.04), 0.0);
        assert_eq!(gate.compute_target(-2.0, -2.0), 1.0);
    }

    #[test]
    fn chirp_has_direction_specific_cut_order() {
        let mut gate = ScratchGate::new(ScratchPreset::Chirp);
        settle_direction(&mut gate, 1.0);
        assert_eq!(gate.target(), 1.0);
        run(&mut gate, 0.08, true, 1.0, 1.0);
        assert_eq!(gate.target(), 0.0);

        run(&mut gate, 0.007, true, -1.0, -1.0);
        assert_eq!(gate.direction(), -1);
        assert_eq!(gate.target(), 0.0);
        run(&mut gate, 0.05, true, -1.0, -1.0);
        assert_eq!(gate.target(), 1.0);
    }

    #[test]
    fn flare_and_orbit_are_open_at_rest_and_cut_mid_cycle() {
        for preset in [ScratchPreset::Flare, ScratchPreset::Orbit] {
            let mut gate = ScratchGate::new(preset);
            run(&mut gate, 0.02, true, 0.0, 0.0);
            assert_eq!(gate.target(), 1.0);
            gate.set_clicks(1);
            settle_direction(&mut gate, 1.0);
            let travel_to_midpoint = gate.learned_span() * 0.5 - gate.stroke_travel;
            run(&mut gate, travel_to_midpoint, true, 1.0, 1.0);
            assert_eq!(gate.target(), 0.0, "{preset:?}");
        }
    }

    #[test]
    fn crab_is_closed_at_rest_and_pulses_while_moving() {
        let mut gate = ScratchGate::new(ScratchPreset::Crab);
        run(&mut gate, 0.020, true, 0.0, 0.0);
        assert_eq!(gate.target(), 0.0);
        settle_direction(&mut gate, 1.0);
        assert_eq!(gate.target(), 1.0);
        run(&mut gate, 0.025, true, 1.0, 1.0);
        assert_eq!(gate.target(), 0.0);
    }

    #[test]
    fn drum_opens_on_attack_then_closes_and_respects_refractory() {
        let mut gate = ScratchGate::new(ScratchPreset::Drum);
        settle_direction(&mut gate, 0.5);
        assert_eq!(gate.target(), 1.0);
        run(&mut gate, 0.038, true, 0.5, 0.5);
        assert_eq!(gate.target(), 0.0);

        gate.process(1.0 / SAMPLE_RATE, true, 1.5, 0.5);
        assert_eq!(gate.target(), 0.0, "early acceleration must not retrigger");
        run(&mut gate, 0.003, true, 1.5, 0.5);
        gate.process(1.0 / SAMPLE_RATE, true, 0.5, 0.5);
        assert_eq!(gate.target(), 1.0);
    }

    #[test]
    fn drum_reversal_does_not_spend_its_hit_on_outgoing_motion() {
        let mut gate = ScratchGate::new(ScratchPreset::Drum);
        settle_direction(&mut gate, 0.5);
        run(&mut gate, 0.070, true, 0.5, 0.5);
        assert_eq!(gate.target(), 0.0);

        run(&mut gate, 0.007, true, -8.0, 8.0);
        assert_eq!(gate.direction(), -1);
        assert_eq!(gate.target(), 1.0);

        run(&mut gate, 0.010, true, -8.0, 8.0);
        assert_eq!(gate.target(), 1.0);

        run(&mut gate, 0.004, true, -8.0, -8.0);
        assert_eq!(gate.target(), 0.0);
    }

    #[test]
    fn drum_ignores_slow_onsets_and_small_pointer_rate_steps() {
        let mut slow = ScratchGate::new(ScratchPreset::Drum);
        settle_direction(&mut slow, 0.05);
        assert_eq!(slow.target(), 0.0);

        let mut jitter = ScratchGate::new(ScratchPreset::Drum);
        settle_direction(&mut jitter, 0.5);
        run(&mut jitter, 0.065, true, 0.5, 0.5);
        assert_eq!(jitter.target(), 0.0);
        run(&mut jitter, 0.012, true, 0.51, 0.5);
        assert_eq!(jitter.target(), 0.0);
    }

    #[test]
    fn drum_acceleration_intent_is_sample_rate_invariant() {
        let trigger_at = |sample_rate: f64| {
            let mut gate = ScratchGate::new(ScratchPreset::Drum);
            let run_at_rate = |gate: &mut ScratchGate, seconds: f64, rate: f64| {
                let frames = (seconds * sample_rate).round() as usize;
                for _ in 0..frames {
                    gate.process(1.0 / sample_rate, true, rate, 0.5);
                }
            };
            run_at_rate(&mut gate, 0.070, 0.5);
            assert_eq!(gate.target(), 0.0);
            run_at_rate(&mut gate, 0.001, 1.5);
            gate.target()
        };

        assert_eq!(trigger_at(44_100.0), 1.0);
        assert_eq!(trigger_at(48_000.0), 1.0);
        assert_eq!(trigger_at(96_000.0), 1.0);
    }

    #[test]
    fn releasing_contact_always_reopens_the_gate() {
        let mut gate = ScratchGate::new(ScratchPreset::Stab);
        settle_direction(&mut gate, -1.0);
        run(&mut gate, 0.010, true, -1.0, -1.0);
        assert!(gate.gate() < 0.05);
        gate.release();
        assert_eq!(gate.target(), 1.0);
        assert_eq!(gate.direction(), 0);
        run(&mut gate, 0.012, false, 0.0, 0.0);
        assert!(gate.gate() > 0.99);
    }

    #[test]
    fn declick_envelope_is_bounded_and_not_instantaneous() {
        let mut gate = ScratchGate::new(ScratchPreset::Stab);
        settle_direction(&mut gate, -1.0);
        assert!(gate.gate() > 0.0 && gate.gate() < 1.0);
        for _ in 0..20_000 {
            let value = gate.process(1.0 / SAMPLE_RATE, true, -1.0, -1.0);
            assert!((0.0..=1.0).contains(&value));
        }
    }

    #[test]
    fn click_gate_envelope_bounds_added_discontinuity_on_audio_fixtures() {
        let fastest_attack_alpha = 1.0 - (-(1.0 / SAMPLE_RATE) / 0.00035_f64).exp();

        for transient in [false, true] {
            for clicks in [1, 4, 8] {
                for preset in [
                    ScratchPreset::Transform,
                    ScratchPreset::Flare,
                    ScratchPreset::Crab,
                    ScratchPreset::Orbit,
                ] {
                    let mut gate = ScratchGate::new(preset);
                    gate.set_clicks(clicks);
                    gate.contact_active = true;
                    gate.direction = 1;
                    gate.moving = true;

                    let rate = 8.0;
                    let frames = (gate.learned_span() / rate * SAMPLE_RATE).floor() as usize;
                    let mut previous_input = audio_fixture_sample(0, transient);
                    let mut previous_output = previous_input * gate.gate();
                    for frame in 1..frames {
                        let gain = gate.process(1.0 / SAMPLE_RATE, true, rate, rate);
                        let input = audio_fixture_sample(frame, transient);
                        let output = input * gain;
                        let input_step = (input - previous_input).abs();
                        let output_step = (output - previous_output).abs();
                        assert!(
                            output_step <= input_step + fastest_attack_alpha + 1e-12,
                            "{preset:?} at {clicks} clicks added too much discontinuity: \
                             output {output_step}, input {input_step}"
                        );
                        previous_input = input;
                        previous_output = output;
                    }
                }
            }
        }
    }

    #[test]
    fn stable_open_and_closed_gate_rms_is_bounded_on_audio_fixtures() {
        let measure = |gate: &mut ScratchGate, direction: f64, transient: bool| {
            run(gate, 0.020, true, direction, direction);
            let frames = (0.012 * SAMPLE_RATE) as usize;
            let mut input_energy = 0.0;
            let mut output_energy = 0.0;
            for frame in 0..frames {
                let input = audio_fixture_sample(frame, transient);
                let output = input * gate.process(1.0 / SAMPLE_RATE, true, direction, direction);
                input_energy += input * input;
                output_energy += output * output;
            }
            (output_energy / input_energy).sqrt()
        };

        for transient in [false, true] {
            let mut gate = ScratchGate::new(ScratchPreset::Stab);
            let closed_ratio = measure(&mut gate, -1.0, transient);
            let open_ratio = measure(&mut gate, 1.0, transient);
            assert!(closed_ratio < 1e-6, "closed RMS ratio was {closed_ratio}");
            assert!(open_ratio > 0.999, "open RMS ratio was {open_ratio}");
        }
    }

    #[test]
    fn gate_timing_is_sample_rate_invariant() {
        let render = |sample_rate: f64| {
            let mut gate = ScratchGate::new(ScratchPreset::Transform);
            let frames = (sample_rate * 0.073).round() as usize;
            for _ in 0..frames {
                gate.process(1.0 / sample_rate, true, 0.9, 0.9);
            }
            (gate.phase(), gate.gate(), gate.direction())
        };
        let at_44 = render(44_100.0);
        let at_48 = render(48_000.0);
        let at_96 = render(96_000.0);
        assert_eq!(at_44.2, at_48.2);
        assert_eq!(at_48.2, at_96.2);
        assert!((at_44.0 - at_48.0).abs() < 0.001);
        assert!((at_48.0 - at_96.0).abs() < 0.001);
        assert!((at_44.1 - at_96.1).abs() < 0.01);
    }
}
