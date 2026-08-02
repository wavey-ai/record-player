use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MIN_SCRATCH_CLICKS: u8 = 1;
pub const MAX_SCRATCH_CLICKS: u8 = 8;
pub const SCRATCH_GATE_ALGORITHM_VERSION: u32 = 7;
pub const SCRATCH_GATE_SNAPSHOT_VERSION: u32 = 2;
pub const SCRATCH_PERFORMANCE_SNAPSHOT_VERSION: u32 = 2;
pub const MAXIMUM_SCRATCH_RECORD_RATE: f64 = 20.0;
const MAXIMUM_FRAME_DELTA_SECONDS: f64 = 1.0 / 8_000.0;

const MOTION_ONSET_RATE: f64 = 0.035;
const REST_RATE: f64 = 0.018;
const INTENT_REVERSAL_RATE: f64 = 0.055;
const MAX_PREDICTED_REVERSAL_OUTGOING_RATE: f64 = 0.35;
const ONSET_CONFIRM_SECONDS: f64 = 0.004;
const REVERSAL_CONFIRM_SECONDS: f64 = 0.006;
const REST_CONFIRM_SECONDS: f64 = 0.012;
const MIN_LEARNED_SPAN: f64 = 0.04;
const MAX_LEARNED_SPAN: f64 = 0.80;
const SPAN_OBSERVATION_WEIGHTS: [f64; 4] = [1.0, 0.70, 0.55, 0.40];
const OBSERVATIONS_FOR_FULL_SPAN_CONFIDENCE: u32 = 4;
const DRUM_MAX_OPEN_SECONDS: f64 = 0.055;
const DRUM_OPEN_SPAN_FRACTION: f64 = 0.18;
const DRUM_ACCELERATION_FILTER_SECONDS: f64 = 0.008;
const DRUM_ACCELERATION_TRIGGER: f64 = 6.0;
const DRUM_TRIGGER_MIN_RATE: f64 = 0.14;
const DRUM_REFRACTORY_SECONDS: f64 = 0.045;
const STAB_OPEN_START_FRACTION: f64 = 0.04;
const STAB_OPEN_WIDTH_FRACTION: f64 = 0.24;
const STAB_OPEN_END_FRACTION: f64 = STAB_OPEN_START_FRACTION + STAB_OPEN_WIDTH_FRACTION;
const TRANSFORM_OPEN_FRACTION: f64 = 0.24;
const FLARE_NOTCH_HALF_WIDTH: f64 = 0.07;
const CRAB_BURST_START: f64 = 0.18;
const CRAB_BURST_END: f64 = 0.72;
const CRAB_PULSE_HALF_WIDTH: f64 = 0.035;
const CRAB_PULSE_WIDTH_BUDGET: f64 = 0.22;
const CHIRP_FORWARD_CLOSE_SLOW: f64 = 0.32;
const CHIRP_FORWARD_CLOSE_FAST: f64 = 0.18;
const CHIRP_REVERSE_OPEN_SLOW: f64 = 0.26;
const CHIRP_REVERSE_OPEN_FAST: f64 = 0.14;
const OPEN_SLOW_TIME_CONSTANT_SECONDS: f64 = 0.0015;
const OPEN_FAST_TIME_CONSTANT_SECONDS: f64 = 0.00035;
const CLOSE_SLOW_TIME_CONSTANT_SECONDS: f64 = 0.0020;
const CLOSE_FAST_TIME_CONSTANT_SECONDS: f64 = 0.00045;

/// Intent-aware automatic crossfader patterns for common scratch techniques.
///
/// `Baby` leaves the stored manual crossfader in control. Every other preset
/// owns the real audible crossfader through this gate. The host can still keep
/// the manual position intact for an immediate return to `Baby`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "lowercase")]
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

    pub const fn id(self) -> u8 {
        self as u8
    }

    pub const fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::Baby),
            1 => Some(Self::Stab),
            2 => Some(Self::Chirp),
            3 => Some(Self::Transform),
            4 => Some(Self::Flare),
            5 => Some(Self::Crab),
            6 => Some(Self::Orbit),
            7 => Some(Self::Drum),
            _ => None,
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

    /// Returns the provisional stroke span in source seconds.
    ///
    /// Prediction confidence remains zero until one stroke completes.
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

    pub const fn descriptor(self) -> ScratchPresetDescriptor {
        ScratchPresetDescriptor {
            preset: self,
            id: self.id(),
            name: self.as_str(),
            default_clicks: self.default_clicks(),
            uses_clicks: self.uses_clicks(),
            uses_manual_crossfader: matches!(self, Self::Baby),
        }
    }
}

/// Stable catalog data for native and Web hosts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchPresetDescriptor {
    pub preset: ScratchPreset,
    pub id: u8,
    pub name: &'static str,
    pub default_clicks: u8,
    pub uses_clicks: bool,
    pub uses_manual_crossfader: bool,
}

pub const SCRATCH_PRESET_CATALOG: [ScratchPresetDescriptor; 8] = [
    ScratchPreset::Baby.descriptor(),
    ScratchPreset::Stab.descriptor(),
    ScratchPreset::Chirp.descriptor(),
    ScratchPreset::Transform.descriptor(),
    ScratchPreset::Flare.descriptor(),
    ScratchPreset::Crab.descriptor(),
    ScratchPreset::Orbit.descriptor(),
    ScratchPreset::Drum.descriptor(),
];

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
            "transform" | "transformer" => Ok(Self::Transform),
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
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchGate {
    preset: ScratchPreset,
    clicks: u8,
    contact_active: bool,
    direction: i8,
    moving: bool,
    pending_direction: i8,
    pending_seconds: f64,
    pending_stroke_travel: f64,
    rest_seconds: f64,
    stroke_travel: f64,
    learned_span: f64,
    span_observations: u32,
    phase: f64,
    gate: f64,
    target: f64,
    drum_filtered_rendered_speed: f64,
    drum_filter_delta_seconds: f64,
    drum_filter_alpha: f64,
    drum_attack_armed: bool,
    drum_open: bool,
    drum_elapsed: f64,
    drum_travel: f64,
    drum_refractory: f64,
}

/// Stores all automatic gate state that can affect later audio frames.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchGateSnapshot {
    version: u32,
    state: ScratchGate,
}

/// Identifies which control owns the audible deck gain.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "camelCase")]
pub enum ScratchCrossfaderOwner {
    Manual = 0,
    AutomaticPreset = 1,
}

/// Supplies one same-sample physical motion result to the scratch helper.
///
/// Both rates use nominal playback speed as one. `rendered_record_rate` must
/// come from the physical deck state for the audio frame being gated.
/// `rendered_source_travel_seconds` is the exact signed record-angle change
/// divided by nominal angular velocity.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchPerformanceInput {
    pub delta_seconds: f64,
    pub hand_contact: bool,
    pub intent_record_rate: f64,
    pub rendered_record_rate: f64,
    pub rendered_source_travel_seconds: f64,
    pub manual_crossfader_gain: f64,
}

impl ScratchPerformanceInput {
    fn validate(self) -> Result<Self, ScratchPerformanceError> {
        if !self.delta_seconds.is_finite()
            || self.delta_seconds <= 0.0
            || self.delta_seconds > MAXIMUM_FRAME_DELTA_SECONDS
        {
            return Err(ScratchPerformanceError::InvalidInput {
                field: "deltaSeconds",
            });
        }
        for (field, value) in [
            ("intentRecordRate", self.intent_record_rate),
            ("renderedRecordRate", self.rendered_record_rate),
        ] {
            if !value.is_finite() || value.abs() > MAXIMUM_SCRATCH_RECORD_RATE {
                return Err(ScratchPerformanceError::InvalidInput { field });
            }
        }
        let maximum_travel = MAXIMUM_SCRATCH_RECORD_RATE * self.delta_seconds;
        let travel_roundoff =
            32.0 * f64::EPSILON * maximum_travel.max(self.rendered_source_travel_seconds.abs());
        if !self.rendered_source_travel_seconds.is_finite()
            || self.rendered_source_travel_seconds.abs() > maximum_travel + travel_roundoff
        {
            return Err(ScratchPerformanceError::InvalidInput {
                field: "renderedSourceTravelSeconds",
            });
        }
        if !self.manual_crossfader_gain.is_finite()
            || !(0.0..=1.0).contains(&self.manual_crossfader_gain)
        {
            return Err(ScratchPerformanceError::InvalidInput {
                field: "manualCrossfaderGain",
            });
        }
        Ok(self)
    }
}

/// Reports the exact gain and motion state for one audio frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchPerformanceOutput {
    pub preset: ScratchPreset,
    pub clicks: u8,
    pub audible_gain: f64,
    pub automatic_gate_gain: f64,
    pub automatic_gate_target: f64,
    pub owner: ScratchCrossfaderOwner,
    pub direction: i8,
    pub moving: bool,
    pub phase: f64,
    pub stroke_progress: f64,
    pub span_prediction_confidence: f64,
}

/// Stores all helper state that can affect later audible gain.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchPerformanceSnapshot {
    version: u32,
    gate: ScratchGateSnapshot,
    audible_gain: f64,
}

/// Canonical allocation-free scratch helper for physical player consumers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScratchPerformance {
    gate: ScratchGate,
    audible_gain: f64,
}

impl Default for ScratchPerformance {
    fn default() -> Self {
        Self::new(ScratchPreset::Baby)
    }
}

impl ScratchPerformance {
    pub fn new(preset: ScratchPreset) -> Self {
        Self {
            gate: ScratchGate::new(preset),
            audible_gain: 1.0,
        }
    }

    pub const fn catalog() -> &'static [ScratchPresetDescriptor; 8] {
        &SCRATCH_PRESET_CATALOG
    }

    pub fn preset(&self) -> ScratchPreset {
        self.gate.preset()
    }

    pub fn set_preset(&mut self, preset: ScratchPreset) {
        self.gate.set_preset(preset);
    }

    pub fn clicks(&self) -> u8 {
        self.gate.clicks()
    }

    pub fn set_clicks(&mut self, clicks: u8) {
        self.gate.set_clicks(clicks);
    }

    pub fn audible_gain(&self) -> f64 {
        self.audible_gain
    }

    pub fn gate(&self) -> &ScratchGate {
        &self.gate
    }

    /// Reports the current output state without advancing the helper.
    pub fn output(&self) -> ScratchPerformanceOutput {
        ScratchPerformanceOutput {
            preset: self.gate.preset(),
            clicks: self.gate.clicks(),
            audible_gain: self.audible_gain,
            automatic_gate_gain: self.gate.gate(),
            automatic_gate_target: self.gate.target(),
            owner: self.crossfader_owner(),
            direction: self.gate.direction(),
            moving: self.gate.moving(),
            phase: self.gate.phase(),
            stroke_progress: self.gate.stroke_progress(),
            span_prediction_confidence: self.gate.span_prediction_confidence(),
        }
    }

    /// Processes one frame after the physical mechanics step for that frame.
    pub fn process_frame(
        &mut self,
        input: ScratchPerformanceInput,
    ) -> Result<ScratchPerformanceOutput, ScratchPerformanceError> {
        let input = input.validate()?;
        self.gate.process_with_travel(
            input.delta_seconds,
            input.hand_contact,
            input.intent_record_rate,
            input.rendered_record_rate,
            input.rendered_source_travel_seconds,
        );
        let owner = self.crossfader_owner();
        let target = match owner {
            ScratchCrossfaderOwner::Manual => input.manual_crossfader_gain,
            ScratchCrossfaderOwner::AutomaticPreset => self.gate.target(),
        };
        let speed = input.rendered_record_rate.abs();
        advance_gain_envelope(&mut self.audible_gain, target, input.delta_seconds, speed);
        Ok(self.output())
    }

    pub fn snapshot(&self) -> ScratchPerformanceSnapshot {
        ScratchPerformanceSnapshot {
            version: SCRATCH_PERFORMANCE_SNAPSHOT_VERSION,
            gate: self.gate.snapshot(),
            audible_gain: self.audible_gain,
        }
    }

    /// Restores only after the complete snapshot passes validation.
    pub fn restore(
        &mut self,
        snapshot: &ScratchPerformanceSnapshot,
    ) -> Result<(), ScratchPerformanceError> {
        if snapshot.version != SCRATCH_PERFORMANCE_SNAPSHOT_VERSION {
            return Err(
                ScratchPerformanceError::UnsupportedPerformanceSnapshotVersion {
                    version: snapshot.version,
                },
            );
        }
        if !snapshot.audible_gain.is_finite() || !(0.0..=1.0).contains(&snapshot.audible_gain) {
            return Err(ScratchPerformanceError::InvalidSnapshot {
                field: "audibleGain",
            });
        }
        let mut restored_gate = self.gate.clone();
        restored_gate.restore(&snapshot.gate)?;
        self.gate = restored_gate;
        self.audible_gain = snapshot.audible_gain;
        Ok(())
    }

    fn crossfader_owner(&self) -> ScratchCrossfaderOwner {
        if self.gate.preset() == ScratchPreset::Baby {
            ScratchCrossfaderOwner::Manual
        } else {
            ScratchCrossfaderOwner::AutomaticPreset
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ScratchPerformanceError {
    #[error("scratch performance input field {field} is invalid")]
    InvalidInput { field: &'static str },
    #[error("scratch gate snapshot version {version} is unsupported")]
    UnsupportedGateSnapshotVersion { version: u32 },
    #[error("scratch performance snapshot version {version} is unsupported")]
    UnsupportedPerformanceSnapshotVersion { version: u32 },
    #[error("scratch snapshot field {field} is invalid")]
    InvalidSnapshot { field: &'static str },
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
            contact_active: false,
            direction: 0,
            moving: false,
            pending_direction: 0,
            pending_seconds: 0.0,
            pending_stroke_travel: 0.0,
            rest_seconds: 0.0,
            stroke_travel: 0.0,
            learned_span: preset.initial_stroke_span(),
            span_observations: 0,
            phase: 0.0,
            gate: 1.0,
            target: 1.0,
            drum_filtered_rendered_speed: 0.0,
            drum_filter_delta_seconds: 0.0,
            drum_filter_alpha: 1.0,
            drum_attack_armed: false,
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
            self.span_observations = 0;
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
        SCRATCH_GATE_ALGORITHM_VERSION
    }

    /// Retains source compatibility while legacy consumers migrate.
    ///
    /// The canonical gate does not emulate replaced algorithms.
    pub fn set_algorithm_version(&mut self, _version: u32) {}

    /// Starts a recorded performance from one defined gate state while
    /// retaining its selected technique and click count.
    pub fn reset_for_replay(&mut self) {
        let preset = self.preset;
        let clicks = self.clicks;
        *self = Self::new(preset);
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

    /// Returns zero before the first complete stroke and one after four strokes.
    pub fn span_prediction_confidence(&self) -> f64 {
        f64::from(
            self.span_observations
                .min(OBSERVATIONS_FOR_FULL_SPAN_CONFIDENCE),
        ) / f64::from(OBSERVATIONS_FOR_FULL_SPAN_CONFIDENCE)
    }

    pub fn snapshot(&self) -> ScratchGateSnapshot {
        ScratchGateSnapshot {
            version: SCRATCH_GATE_SNAPSHOT_VERSION,
            state: self.clone(),
        }
    }

    /// Restores only after the complete gate state passes validation.
    pub fn restore(
        &mut self,
        snapshot: &ScratchGateSnapshot,
    ) -> Result<(), ScratchPerformanceError> {
        if snapshot.version != SCRATCH_GATE_SNAPSHOT_VERSION {
            return Err(ScratchPerformanceError::UnsupportedGateSnapshotVersion {
                version: snapshot.version,
            });
        }
        snapshot.state.validate_snapshot_state()?;
        self.clone_from(&snapshot.state);
        Ok(())
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
        let intent_rate = finite_or_zero(intent_rate)
            .clamp(-MAXIMUM_SCRATCH_RECORD_RATE, MAXIMUM_SCRATCH_RECORD_RATE);
        let rendered_rate = finite_or_zero(rendered_rate)
            .clamp(-MAXIMUM_SCRATCH_RECORD_RATE, MAXIMUM_SCRATCH_RECORD_RATE);
        self.process_with_travel(
            dt,
            hand_contact,
            intent_rate,
            rendered_rate,
            rendered_rate * dt,
        )
    }

    fn process_with_travel(
        &mut self,
        dt: f64,
        hand_contact: bool,
        intent_rate: f64,
        rendered_rate: f64,
        rendered_source_travel_seconds: f64,
    ) -> f64 {
        self.drum_refractory = (self.drum_refractory + dt).min(DRUM_REFRACTORY_SECONDS);
        if !hand_contact {
            self.release();
            self.advance_envelope(dt, intent_rate, rendered_rate);
            return self.gate;
        }

        if !self.contact_active {
            self.contact_active = true;
            self.reset_phrase();
        }

        let (event, confirmed_travel) = self.update_motion(
            dt,
            intent_rate,
            rendered_rate,
            rendered_source_travel_seconds,
        );
        if event == MotionEvent::Reversal {
            self.learn_completed_stroke();
            self.stroke_travel = confirmed_travel;
            self.phase = 0.0;
        } else if matches!(event, MotionEvent::Onset | MotionEvent::Resume) {
            self.stroke_travel += confirmed_travel;
        }

        let rendered_stroke_travel =
            if self.moving && sign(rendered_source_travel_seconds) == self.direction {
                rendered_source_travel_seconds.abs()
            } else {
                0.0
            };
        if self.moving {
            let current_frame_is_already_confirmed = matches!(
                event,
                MotionEvent::Onset | MotionEvent::Resume | MotionEvent::Reversal
            );
            if !current_frame_is_already_confirmed {
                self.stroke_travel += rendered_stroke_travel;
            }
            self.update_phase();
        }

        if self.preset == ScratchPreset::Drum {
            let acceleration = self.filter_drum_acceleration(dt, rendered_rate);
            self.update_drum(
                event,
                dt,
                rendered_rate,
                rendered_stroke_travel,
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
        self.drum_filtered_rendered_speed = 0.0;
        self.drum_attack_armed = false;
        self.drum_open = false;
        self.drum_elapsed = 0.0;
        self.drum_travel = 0.0;
        self.drum_refractory = DRUM_REFRACTORY_SECONDS;
    }

    fn validate_snapshot_state(&self) -> Result<(), ScratchPerformanceError> {
        if !(MIN_SCRATCH_CLICKS..=MAX_SCRATCH_CLICKS).contains(&self.clicks) {
            return invalid_snapshot("clicks");
        }
        if ![-1, 0, 1].contains(&self.direction) {
            return invalid_snapshot("direction");
        }
        if ![-1, 0, 1].contains(&self.pending_direction) {
            return invalid_snapshot("pendingDirection");
        }
        if self.moving && self.direction == 0 {
            return invalid_snapshot("moving");
        }
        if self.pending_direction == 0
            && (self.pending_seconds != 0.0 || self.pending_stroke_travel != 0.0)
        {
            return invalid_snapshot("pendingDirection");
        }
        if !self.contact_active
            && (self.direction != 0
                || self.moving
                || self.pending_direction != 0
                || self.stroke_travel != 0.0)
        {
            return invalid_snapshot("contactActive");
        }
        for (field, value) in [
            ("pendingSeconds", self.pending_seconds),
            ("pendingStrokeTravel", self.pending_stroke_travel),
            ("restSeconds", self.rest_seconds),
            ("strokeTravel", self.stroke_travel),
            ("drumFilterDeltaSeconds", self.drum_filter_delta_seconds),
            ("drumElapsed", self.drum_elapsed),
            ("drumTravel", self.drum_travel),
            ("drumRefractory", self.drum_refractory),
        ] {
            if !value.is_finite() || value < 0.0 {
                return invalid_snapshot(field);
            }
        }
        if !self.learned_span.is_finite()
            || !(MIN_LEARNED_SPAN..=MAX_LEARNED_SPAN).contains(&self.learned_span)
        {
            return invalid_snapshot("learnedSpan");
        }
        for (field, value) in [
            ("phase", self.phase),
            ("gate", self.gate),
            ("target", self.target),
            ("drumFilterAlpha", self.drum_filter_alpha),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return invalid_snapshot(field);
            }
        }
        if !self.drum_filtered_rendered_speed.is_finite()
            || !(0.0..=MAXIMUM_SCRATCH_RECORD_RATE).contains(&self.drum_filtered_rendered_speed)
        {
            return invalid_snapshot("drumFilteredRenderedSpeed");
        }
        if self.drum_filter_delta_seconds > 0.1 {
            return invalid_snapshot("drumFilterDeltaSeconds");
        }
        if self.drum_refractory > DRUM_REFRACTORY_SECONDS {
            return invalid_snapshot("drumRefractory");
        }
        if self.drum_open && self.drum_elapsed >= DRUM_MAX_OPEN_SECONDS {
            return invalid_snapshot("drumElapsed");
        }
        Ok(())
    }

    fn update_motion(
        &mut self,
        dt: f64,
        intent_rate: f64,
        rendered_rate: f64,
        rendered_source_travel_seconds: f64,
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
        if sign(rendered_source_travel_seconds) == candidate {
            self.pending_stroke_travel += rendered_source_travel_seconds.abs();
        }
        let physical_motion_confirms_direction =
            sign(rendered_rate) == candidate && rendered_rate.abs() >= MOTION_ONSET_RATE;
        if !physical_motion_confirms_direction
            && (self.pending_seconds < confirmation_seconds
                || !predicted_direction_is_physically_plausible(candidate, rendered_rate))
        {
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
        let observed_weight = SPAN_OBSERVATION_WEIGHTS[self
            .span_observations
            .min(SPAN_OBSERVATION_WEIGHTS.len() as u32 - 1)
            as usize];
        self.learned_span = lerp(self.learned_span, observed, observed_weight)
            .clamp(MIN_LEARNED_SPAN, MAX_LEARNED_SPAN);
        self.span_observations = self.span_observations.saturating_add(1);
    }

    fn update_phase(&mut self) {
        let progress = self.stroke_progress();
        if !self.preset.uses_clicks() {
            self.phase = progress;
            return;
        }
        if progress >= 1.0 {
            self.phase = 1.0;
            return;
        }
        self.phase = (progress * f64::from(self.clicks)).rem_euclid(1.0);
    }

    fn update_drum(
        &mut self,
        event: MotionEvent,
        dt: f64,
        rendered_rate: f64,
        rendered_stroke_travel: f64,
        intent_rate: f64,
        acceleration: f64,
    ) {
        let requested_speed = intent_rate.abs().max(rendered_rate.abs());
        if matches!(event, MotionEvent::Onset | MotionEvent::Reversal)
            && requested_speed >= DRUM_TRIGGER_MIN_RATE
        {
            self.drum_attack_armed = true;
        } else if event == MotionEvent::Rest {
            self.drum_attack_armed = false;
        }

        let rendered_motion_matches = self.moving
            && sign(rendered_rate) == self.direction
            && rendered_rate.abs() >= DRUM_TRIGGER_MIN_RATE;
        if self.drum_attack_armed
            && rendered_motion_matches
            && self.drum_refractory >= DRUM_REFRACTORY_SECONDS
        {
            self.trigger_drum();
            self.drum_attack_armed = false;
        }

        let acceleration_trigger = rendered_motion_matches
            && rendered_rate.abs() >= DRUM_TRIGGER_MIN_RATE
            && acceleration >= DRUM_ACCELERATION_TRIGGER
            && self.drum_refractory >= DRUM_REFRACTORY_SECONDS;
        if acceleration_trigger {
            self.trigger_drum();
        }

        if self.drum_open {
            self.drum_elapsed += dt;
            self.drum_travel += rendered_stroke_travel;
            if self.drum_elapsed >= DRUM_MAX_OPEN_SECONDS
                || self.drum_travel >= self.learned_span * DRUM_OPEN_SPAN_FRACTION
            {
                self.drum_open = false;
            }
        }
    }

    fn filter_drum_acceleration(&mut self, dt: f64, rendered_rate: f64) -> f64 {
        if dt <= 0.0 {
            return 0.0;
        }
        // The physical deck rate can contain solver-scale velocity changes.
        // This time-based filter prevents sample-rate-dependent attacks.
        if dt != self.drum_filter_delta_seconds {
            self.drum_filter_delta_seconds = dt;
            self.drum_filter_alpha = 1.0 - (-dt / DRUM_ACCELERATION_FILTER_SECONDS).exp();
        }
        let previous = self.drum_filtered_rendered_speed;
        self.drum_filtered_rendered_speed +=
            (rendered_rate.abs() - previous) * self.drum_filter_alpha;
        ((self.drum_filtered_rendered_speed - previous) / dt).max(0.0)
    }

    fn trigger_drum(&mut self) {
        self.drum_open = true;
        self.drum_elapsed = 0.0;
        self.drum_travel = 0.0;
        self.drum_refractory = 0.0;
    }

    fn compute_target(&self, _intent_rate: f64, rendered_rate: f64) -> f64 {
        let speed = rendered_rate.abs();
        let confidence = smoothstep(MOTION_ONSET_RATE, 2.0, speed);
        let reversal_is_pending = self.pending_direction != 0
            && self.direction != 0
            && self.pending_direction != self.direction;
        match self.preset {
            ScratchPreset::Baby => 1.0,
            ScratchPreset::Stab => f64::from(
                self.moving
                    && self.direction > 0
                    && !reversal_is_pending
                    && self.stroke_progress() >= STAB_OPEN_START_FRACTION
                    && self.stroke_progress() < STAB_OPEN_END_FRACTION,
            ),
            ScratchPreset::Chirp => {
                if reversal_is_pending {
                    0.0
                } else if self.direction == 0 {
                    1.0
                } else if self.direction > 0 {
                    let close_at = lerp(
                        CHIRP_FORWARD_CLOSE_SLOW,
                        CHIRP_FORWARD_CLOSE_FAST,
                        confidence,
                    );
                    f64::from(self.phase < close_at)
                } else {
                    let open_at =
                        lerp(CHIRP_REVERSE_OPEN_SLOW, CHIRP_REVERSE_OPEN_FAST, confidence);
                    f64::from(self.phase >= open_at)
                }
            }
            ScratchPreset::Transform => {
                f64::from(self.moving && self.phase < TRANSFORM_OPEN_FRACTION)
            }
            ScratchPreset::Flare => {
                if !self.moving || self.direction < 0 {
                    1.0
                } else {
                    f64::from(distance_from_midpoint(self.phase) >= FLARE_NOTCH_HALF_WIDTH)
                }
            }
            ScratchPreset::Crab => {
                f64::from(self.moving && crab_pulse_is_open(self.stroke_progress(), self.clicks))
            }
            ScratchPreset::Orbit => {
                if !self.moving {
                    1.0
                } else {
                    f64::from(distance_from_midpoint(self.phase) >= FLARE_NOTCH_HALF_WIDTH)
                }
            }
            ScratchPreset::Drum => f64::from(self.drum_open),
        }
    }

    fn advance_envelope(&mut self, dt: f64, _intent_rate: f64, rendered_rate: f64) {
        if dt <= 0.0 {
            return;
        }
        let speed = rendered_rate.abs();
        let confidence = smoothstep(MOTION_ONSET_RATE, 2.0, speed);
        let time_constant = if self.target >= self.gate {
            lerp(
                OPEN_SLOW_TIME_CONSTANT_SECONDS,
                OPEN_FAST_TIME_CONSTANT_SECONDS,
                confidence,
            )
        } else {
            lerp(
                CLOSE_SLOW_TIME_CONSTANT_SECONDS,
                CLOSE_FAST_TIME_CONSTANT_SECONDS,
                confidence,
            )
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

fn invalid_snapshot<T>(field: &'static str) -> Result<T, ScratchPerformanceError> {
    Err(ScratchPerformanceError::InvalidSnapshot { field })
}

fn advance_gain_envelope(gain: &mut f64, target: f64, dt: f64, speed: f64) {
    let confidence = smoothstep(MOTION_ONSET_RATE, 2.0, speed);
    let time_constant = if target >= *gain {
        lerp(
            OPEN_SLOW_TIME_CONSTANT_SECONDS,
            OPEN_FAST_TIME_CONSTANT_SECONDS,
            confidence,
        )
    } else {
        lerp(
            CLOSE_SLOW_TIME_CONSTANT_SECONDS,
            CLOSE_FAST_TIME_CONSTANT_SECONDS,
            confidence,
        )
    };
    let alpha = 1.0 - (-dt / time_constant).exp();
    *gain = (*gain + (target - *gain) * alpha).clamp(0.0, 1.0);
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

fn predicted_direction_is_physically_plausible(candidate: i8, rendered_rate: f64) -> bool {
    sign(rendered_rate) == candidate || rendered_rate.abs() <= MAX_PREDICTED_REVERSAL_OUTGOING_RATE
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

fn crab_pulse_is_open(progress: f64, clicks: u8) -> bool {
    let clicks = clicks.clamp(MIN_SCRATCH_CLICKS, MAX_SCRATCH_CLICKS);
    let pulse_half_width = CRAB_PULSE_HALF_WIDTH.min(CRAB_PULSE_WIDTH_BUDGET / f64::from(clicks));
    if progress < CRAB_BURST_START - pulse_half_width
        || progress > CRAB_BURST_END + pulse_half_width
    {
        return false;
    }
    if clicks == 1 {
        let center = (CRAB_BURST_START + CRAB_BURST_END) * 0.5;
        return (progress - center).abs() <= pulse_half_width;
    }
    let spacing = (CRAB_BURST_END - CRAB_BURST_START) / f64::from(clicks - 1);
    for pulse in 0..clicks {
        let center = CRAB_BURST_START + f64::from(pulse) * spacing;
        if (progress - center).abs() <= pulse_half_width {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f64 = 48_000.0;

    fn performance_input(
        contact: bool,
        intent: f64,
        rendered: f64,
        manual_gain: f64,
    ) -> ScratchPerformanceInput {
        ScratchPerformanceInput {
            delta_seconds: 1.0 / SAMPLE_RATE,
            hand_contact: contact,
            intent_record_rate: intent,
            rendered_record_rate: rendered,
            rendered_source_travel_seconds: rendered / SAMPLE_RATE,
            manual_crossfader_gain: manual_gain,
        }
    }

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

    fn target_at_stroke_progress(
        preset: ScratchPreset,
        clicks: u8,
        direction: i8,
        progress: f64,
        speed: f64,
    ) -> f64 {
        let mut gate = ScratchGate::new(preset);
        gate.set_clicks(clicks);
        gate.contact_active = true;
        gate.direction = direction;
        gate.moving = direction != 0;
        gate.stroke_travel = gate.learned_span() * progress;
        gate.update_phase();
        gate.compute_target(f64::from(direction) * speed, f64::from(direction) * speed)
    }

    fn count_maximum_rate_target_runs(
        preset: ScratchPreset,
        clicks: u8,
        counted_target: f64,
    ) -> usize {
        let mut gate = ScratchGate::new(preset);
        gate.set_clicks(clicks);
        let frames = (preset.initial_stroke_span() / MAXIMUM_SCRATCH_RECORD_RATE * SAMPLE_RATE)
            .ceil() as usize
            + 1;
        let mut previous = None;
        let mut runs = 0;
        for _ in 0..frames {
            gate.process(
                1.0 / SAMPLE_RATE,
                true,
                MAXIMUM_SCRATCH_RECORD_RATE,
                MAXIMUM_SCRATCH_RECORD_RATE,
            );
            let target = gate.target();
            if target == counted_target && previous != Some(counted_target) {
                runs += 1;
            }
            previous = Some(target);
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
        assert_eq!(
            "transformer".parse::<ScratchPreset>().unwrap(),
            ScratchPreset::Transform
        );
        assert!("scribble".parse::<ScratchPreset>().is_err());
        assert_eq!(ScratchPerformance::catalog(), &SCRATCH_PRESET_CATALOG);
        for (id, descriptor) in SCRATCH_PRESET_CATALOG.into_iter().enumerate() {
            assert_eq!(descriptor.id, id as u8);
            assert_eq!(
                ScratchPreset::from_id(descriptor.id),
                Some(descriptor.preset)
            );
            assert_eq!(descriptor.name, descriptor.preset.as_str());
            assert_eq!(
                descriptor.default_clicks,
                descriptor.preset.default_clicks()
            );
            assert_eq!(descriptor.uses_clicks, descriptor.preset.uses_clicks());
            assert_eq!(
                descriptor.uses_manual_crossfader,
                descriptor.preset == ScratchPreset::Baby
            );
        }
        assert_eq!(ScratchPreset::from_id(8), None);
    }

    #[test]
    fn pvc_005_fixture_matches_canonical_constants_and_claim_limits() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/pvc_005_scratch_semantics.json"
        ))
        .unwrap();
        assert_eq!(fixture["schemaVersion"], 2);
        assert_eq!(fixture["caseId"], "PVC-005");
        assert_eq!(fixture["sampleRateHz"], SAMPLE_RATE);
        assert_eq!(fixture["algorithmVersion"], SCRATCH_GATE_ALGORITHM_VERSION);
        assert_eq!(
            fixture["snapshotVersions"]["gate"],
            SCRATCH_GATE_SNAPSHOT_VERSION
        );
        assert_eq!(
            fixture["snapshotVersions"]["performance"],
            SCRATCH_PERFORMANCE_SNAPSHOT_VERSION
        );

        let limits = &fixture["limits"];
        assert_eq!(limits["minimumClicks"], MIN_SCRATCH_CLICKS);
        assert_eq!(limits["maximumClicks"], MAX_SCRATCH_CLICKS);
        assert_eq!(limits["maximumRecordRate"], MAXIMUM_SCRATCH_RECORD_RATE);
        assert_eq!(
            limits["maximumFrameDeltaSeconds"],
            MAXIMUM_FRAME_DELTA_SECONDS
        );
        assert_eq!(limits["minimumLearnedSpanSourceSeconds"], MIN_LEARNED_SPAN);
        assert_eq!(limits["maximumLearnedSpanSourceSeconds"], MAX_LEARNED_SPAN);

        let motion = &fixture["motionModel"];
        for (name, value) in [
            ("onsetRate", MOTION_ONSET_RATE),
            ("restRate", REST_RATE),
            ("intentReversalRate", INTENT_REVERSAL_RATE),
            (
                "maximumPredictedReversalOutgoingRate",
                MAX_PREDICTED_REVERSAL_OUTGOING_RATE,
            ),
            ("physicalDirectionConfirmationRate", MOTION_ONSET_RATE),
            ("intentOnsetConfirmSeconds", ONSET_CONFIRM_SECONDS),
            ("intentReversalConfirmSeconds", REVERSAL_CONFIRM_SECONDS),
            ("restConfirmSeconds", REST_CONFIRM_SECONDS),
        ] {
            assert_eq!(motion[name], value, "{name}");
        }
        assert_eq!(
            motion["predictionObservationWeights"],
            serde_json::json!(SPAN_OBSERVATION_WEIGHTS)
        );
        assert_eq!(
            motion["observationsForFullConfidence"],
            OBSERVATIONS_FOR_FULL_SPAN_CONFIDENCE
        );

        let envelope = &fixture["declickEnvelope"];
        for (name, value) in [
            (
                "openSlowTimeConstantSeconds",
                OPEN_SLOW_TIME_CONSTANT_SECONDS,
            ),
            (
                "openFastTimeConstantSeconds",
                OPEN_FAST_TIME_CONSTANT_SECONDS,
            ),
            (
                "closeSlowTimeConstantSeconds",
                CLOSE_SLOW_TIME_CONSTANT_SECONDS,
            ),
            (
                "closeFastTimeConstantSeconds",
                CLOSE_FAST_TIME_CONSTANT_SECONDS,
            ),
        ] {
            assert_eq!(envelope[name], value, "{name}");
        }

        let techniques = fixture["techniques"].as_array().unwrap();
        assert_eq!(techniques.len(), SCRATCH_PRESET_CATALOG.len());
        for descriptor in SCRATCH_PRESET_CATALOG {
            let technique = &techniques[usize::from(descriptor.id)];
            assert_eq!(technique["id"], descriptor.id);
            assert_eq!(technique["name"], descriptor.name);
            assert_eq!(technique["defaultClicks"], descriptor.default_clicks);
            assert_eq!(technique["usesClicks"], descriptor.uses_clicks);
            assert_eq!(
                technique["usesManualCrossfader"],
                descriptor.uses_manual_crossfader
            );
            assert_eq!(
                technique["initialStrokeSpanSourceSeconds"],
                descriptor.preset.initial_stroke_span()
            );
        }
        assert_eq!(
            techniques[1]["forwardOpenStartFraction"],
            STAB_OPEN_START_FRACTION
        );
        assert_eq!(
            techniques[1]["forwardOpenWidthFraction"],
            STAB_OPEN_WIDTH_FRACTION
        );
        assert!(
            (techniques[1]["forwardOpenEndFraction"].as_f64().unwrap() - STAB_OPEN_END_FRACTION)
                .abs()
                <= f64::EPSILON
        );
        assert_eq!(
            techniques[2]["forwardCloseFractionSlow"],
            CHIRP_FORWARD_CLOSE_SLOW
        );
        assert_eq!(
            techniques[2]["forwardCloseFractionFast"],
            CHIRP_FORWARD_CLOSE_FAST
        );
        assert_eq!(
            techniques[2]["reverseOpenFractionSlow"],
            CHIRP_REVERSE_OPEN_SLOW
        );
        assert_eq!(
            techniques[2]["reverseOpenFractionFast"],
            CHIRP_REVERSE_OPEN_FAST
        );
        assert_eq!(techniques[2]["forwardEndpointPauseTarget"], 0.0);
        assert_eq!(techniques[3]["acceptedAlias"], "transformer");
        assert_eq!(techniques[3]["openFraction"], TRANSFORM_OPEN_FRACTION);
        assert_eq!(techniques[4]["notchHalfWidth"], FLARE_NOTCH_HALF_WIDTH);
        assert_eq!(techniques[4]["notchedDirections"], serde_json::json!([1]));
        assert_eq!(techniques[5]["burstFirstCenterFraction"], CRAB_BURST_START);
        assert_eq!(techniques[5]["burstLastCenterFraction"], CRAB_BURST_END);
        assert_eq!(
            techniques[5]["maximumPulseHalfWidthFraction"],
            CRAB_PULSE_HALF_WIDTH
        );
        assert_eq!(
            techniques[5]["pulseWidthBudgetFraction"],
            CRAB_PULSE_WIDTH_BUDGET
        );
        assert_eq!(techniques[6]["notchHalfWidth"], FLARE_NOTCH_HALF_WIDTH);
        assert_eq!(
            techniques[6]["notchedDirections"],
            serde_json::json!([1, -1])
        );
        for (name, value) in [
            ("maximumOpenSeconds", DRUM_MAX_OPEN_SECONDS),
            ("openSpanFraction", DRUM_OPEN_SPAN_FRACTION),
            (
                "accelerationFilterSeconds",
                DRUM_ACCELERATION_FILTER_SECONDS,
            ),
            (
                "accelerationTriggerRatePerSecond",
                DRUM_ACCELERATION_TRIGGER,
            ),
            ("minimumTriggerRate", DRUM_TRIGGER_MIN_RATE),
            ("refractorySeconds", DRUM_REFRACTORY_SECONDS),
        ] {
            assert_eq!(techniques[7][name], value, "{name}");
        }

        let fixtures = &fixture["fixtures"];
        assert_eq!(
            fixtures["sampleRateInvariance"]["sampleRatesHz"],
            serde_json::json!([44_100.0, 48_000.0, 96_000.0])
        );
        assert_eq!(fixtures["sampleRateInvariance"]["durationSeconds"], 0.073);
        assert_eq!(fixtures["sampleRateInvariance"]["recordRate"], 0.9);
        assert_eq!(
            fixtures["sampleRateInvariance"]["maximumPhaseDifference"],
            0.001
        );
        assert_eq!(
            fixtures["sampleRateInvariance"]["maximumAudibleGainDifference"],
            0.01
        );
        assert_eq!(
            fixtures["physicalTravelClock"]["deltaSeconds"],
            1.0 / SAMPLE_RATE
        );
        assert_eq!(fixtures["physicalTravelClock"]["intentRecordRate"], 4.0);
        assert_eq!(fixtures["physicalTravelClock"]["renderedRecordRate"], 0.5);
        assert_eq!(
            fixtures["physicalTravelClock"]["renderedSourceTravelSeconds"],
            0.0
        );
        assert_eq!(fixtures["physicalTravelClock"]["renderedFrames"], 2_000);
        assert_eq!(
            fixtures["physicalTravelClock"]["expectedStrokeProgress"],
            0.0
        );
        assert_eq!(fixtures["firstStrokePrediction"]["initialConfidence"], 0.0);
        assert_eq!(
            fixtures["firstStrokePrediction"]["firstObservedSpanSourceSeconds"],
            0.123
        );
        assert_eq!(
            fixtures["firstStrokePrediction"]["confidenceAfterFirstObservation"],
            0.25
        );
        assert_eq!(
            fixtures["firstStrokePrediction"]["fullConfidenceObservationCount"],
            OBSERVATIONS_FOR_FULL_SPAN_CONFIDENCE
        );
        assert_eq!(
            fixtures["clickEndpoint"]["clickCounts"],
            serde_json::json!([1, 4, 8])
        );
        assert_eq!(fixtures["clickEndpoint"]["renderedPredictedSpans"], 3.0);
        assert_eq!(
            fixtures["clickEndpoint"]["expectedRunCountRule"],
            "selected click count"
        );
        assert_eq!(fixtures["drumPacketization"]["frameCount"], 8_000);
        assert_eq!(
            fixtures["drumPacketization"]["renderedRateBeforeFrame3000"],
            0.5
        );
        assert_eq!(
            fixtures["drumPacketization"]["renderedRateFromFrame3000"],
            1.5
        );
        assert_eq!(
            fixtures["drumPacketization"]["intentRates"],
            serde_json::json!([0.35, 0.85])
        );
        assert_eq!(
            fixtures["drumPacketization"]["intentPacketFrames"],
            serde_json::json!([1, 64])
        );
        assert_eq!(
            fixtures["drumPacketization"]["expectedEquality"],
            "exact target, automatic gate, and audible gain"
        );
        assert_eq!(fixtures["rapidReversal"]["strokeCount"], 32);
        assert_eq!(
            fixtures["rapidReversal"]["recordRates"],
            serde_json::json!([8.0, -8.0])
        );
        assert_eq!(fixtures["rapidReversal"]["framesPerStroke"], 400);
        assert_eq!(fixtures["rapidReversal"]["expectedEndpointTarget"], 0.0);
        assert_eq!(fixtures["maximumRateReversal"]["strokeCount"], 64);
        assert_eq!(
            fixtures["maximumRateReversal"]["recordRates"],
            serde_json::json!([20.0, -20.0])
        );
        assert_eq!(fixtures["maximumRateReversal"]["framesPerStroke"], 300);
        assert_eq!(
            fixtures["maximumRateReversal"]["expectedGainBounds"],
            serde_json::json!([0.0, 1.0])
        );
        let physical_confirmation = &fixtures["physicalMaximumRateConfirmation"];
        assert_eq!(
            physical_confirmation["recordRate"],
            MAXIMUM_SCRATCH_RECORD_RATE
        );
        assert_eq!(
            physical_confirmation["formerIntentOnsetDelaySeconds"],
            ONSET_CONFIRM_SECONDS
        );
        assert_eq!(
            physical_confirmation["formerOnsetBufferedSourceSeconds"],
            ONSET_CONFIRM_SECONDS * MAXIMUM_SCRATCH_RECORD_RATE
        );
        assert_eq!(
            physical_confirmation["formerIntentReversalDelaySeconds"],
            REVERSAL_CONFIRM_SECONDS
        );
        assert_eq!(
            physical_confirmation["formerReversalBufferedSourceSeconds"],
            REVERSAL_CONFIRM_SECONDS * MAXIMUM_SCRATCH_RECORD_RATE
        );
        assert_eq!(
            physical_confirmation["stabSeedSpanSourceSeconds"],
            ScratchPreset::Stab.initial_stroke_span()
        );
        assert_eq!(
            physical_confirmation["formerOnsetSeedSpanFraction"],
            ONSET_CONFIRM_SECONDS * MAXIMUM_SCRATCH_RECORD_RATE
                / ScratchPreset::Stab.initial_stroke_span()
        );
        assert_eq!(
            physical_confirmation["formerReversalSeedSpanFraction"],
            REVERSAL_CONFIRM_SECONDS * MAXIMUM_SCRATCH_RECORD_RATE
                / ScratchPreset::Stab.initial_stroke_span()
        );
        assert_eq!(
            physical_confirmation["expectedFirstStabAttack"],
            "preserved"
        );
        assert_eq!(
            physical_confirmation["expectedEarlyClickRunCountRule"],
            "selected click count"
        );
        assert_eq!(fixtures["snapshotReplay"]["preset"], "crab");
        assert_eq!(fixtures["snapshotReplay"]["clicks"], 5);
        assert_eq!(fixtures["snapshotReplay"]["replayedFrames"], 128);
        assert_eq!(
            fixtures["snapshotReplay"]["expectedEquality"],
            "exact output structure"
        );
        assert_eq!(fixtures["allocation"]["preset"], "crab");
        assert_eq!(fixtures["allocation"]["clicks"], 8);
        assert_eq!(fixtures["allocation"]["renderedFrames"], 10_000);
        assert_eq!(fixtures["allocation"]["expectedAllocations"], 0);

        assert_eq!(
            fixture["claimLimits"],
            serde_json::json!([
                "This case validates helper topology, state, ownership, and deterministic timing.",
                "This case does not validate the timing against measured DJ crossfader traces.",
                "The technique fractions and Drum thresholds are provisional calibration values.",
                "The first-stroke seed is not a measured endpoint prediction.",
                "Physical-player, C ABI, and native Swift tests verify the production gain path.",
                "This case does not prove WASM or browser integration."
            ])
        );
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
    fn technique_contract_has_defining_direction_and_fader_edges() {
        assert_eq!(
            target_at_stroke_progress(ScratchPreset::Baby, 1, 1, 0.5, 2.0),
            1.0
        );
        assert_eq!(
            target_at_stroke_progress(ScratchPreset::Baby, 1, -1, 0.5, 2.0),
            1.0
        );

        for (progress, target) in [(0.039, 0.0), (0.041, 1.0), (0.279, 1.0), (0.281, 0.0)] {
            assert_eq!(
                target_at_stroke_progress(ScratchPreset::Stab, 1, 1, progress, 2.0),
                target,
                "stab at {progress}"
            );
        }
        assert_eq!(
            target_at_stroke_progress(ScratchPreset::Stab, 1, -1, 0.16, 2.0),
            0.0
        );

        for (direction, before, edge) in [(1, 0.179, 0.181), (-1, 0.139, 0.141)] {
            let before_target = if direction > 0 { 1.0 } else { 0.0 };
            let edge_target = 1.0 - before_target;
            assert_eq!(
                target_at_stroke_progress(ScratchPreset::Chirp, 1, direction, before, 2.0,),
                before_target
            );
            assert_eq!(
                target_at_stroke_progress(ScratchPreset::Chirp, 1, direction, edge, 2.0,),
                edge_target
            );
        }

        assert_eq!(
            target_at_stroke_progress(ScratchPreset::Transform, 2, 1, 0.119, 2.0),
            1.0
        );
        assert_eq!(
            target_at_stroke_progress(ScratchPreset::Transform, 2, 1, 0.121, 2.0),
            0.0
        );
        assert_eq!(
            target_at_stroke_progress(ScratchPreset::Transform, 2, 1, 0.500, 2.0),
            1.0
        );

        assert_eq!(
            target_at_stroke_progress(ScratchPreset::Flare, 1, 1, 0.429, 2.0),
            1.0
        );
        assert_eq!(
            target_at_stroke_progress(ScratchPreset::Flare, 1, 1, 0.431, 2.0),
            0.0
        );
        assert_eq!(
            target_at_stroke_progress(ScratchPreset::Flare, 1, -1, 0.500, 2.0),
            1.0,
            "the Flare preset represents one forward flare, not a two-stroke orbit"
        );

        assert_eq!(
            target_at_stroke_progress(ScratchPreset::Crab, 4, 1, 0.10, 2.0),
            0.0
        );
        for center in [0.18, 0.36, 0.54, 0.72] {
            assert_eq!(
                target_at_stroke_progress(ScratchPreset::Crab, 4, 1, center, 2.0),
                1.0,
                "crab pulse at {center}"
            );
        }
        assert_eq!(
            target_at_stroke_progress(ScratchPreset::Crab, 4, 1, 0.80, 2.0),
            0.0
        );

        for direction in [1, -1] {
            assert_eq!(
                target_at_stroke_progress(ScratchPreset::Orbit, 2, direction, 0.26, 2.0),
                0.0
            );
            assert_eq!(
                target_at_stroke_progress(ScratchPreset::Orbit, 2, direction, 0.35, 2.0),
                1.0
            );
        }

        let mut drum = ScratchGate::new(ScratchPreset::Drum);
        settle_direction(&mut drum, 0.5);
        assert_eq!(drum.target(), 1.0);
        run(&mut drum, DRUM_MAX_OPEN_SECONDS, true, 0.5, 0.5);
        assert_eq!(drum.target(), 0.0);
    }

    #[test]
    fn every_technique_tracks_record_direction_with_continuous_same_sample_gain() {
        let maximum_alpha = 1.0 - (-(1.0 / SAMPLE_RATE) / 0.00035_f64).exp();
        for preset in ScratchPreset::ALL {
            let mut performance = ScratchPerformance::new(preset);
            let mut previous_gain = performance.audible_gain();
            let mut last_output = performance.output();
            for direction in [1.0, -1.0] {
                for _ in 0..((0.010 * SAMPLE_RATE) as usize) {
                    last_output = performance
                        .process_frame(performance_input(true, direction, direction, 1.0))
                        .unwrap();
                    assert_eq!(last_output.audible_gain, performance.audible_gain());
                    assert!((0.0..=1.0).contains(&last_output.audible_gain));
                    assert!(
                        (last_output.audible_gain - previous_gain).abs() <= maximum_alpha + 1.0e-12,
                        "{preset:?} introduced an unbounded gain step"
                    );
                    previous_gain = last_output.audible_gain;
                }
                assert_eq!(last_output.direction, sign(direction), "{preset:?}");
                assert_eq!(last_output.preset, preset);
                assert_eq!(last_output.clicks, preset.default_clicks());
            }
        }
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
    fn stab_is_one_short_forward_pulse_not_an_open_forward_stroke() {
        let mut gate = ScratchGate::new(ScratchPreset::Stab);
        settle_direction(&mut gate, 1.0);
        assert_eq!(gate.target(), 1.0);
        let later_in_stroke = gate.learned_span() * 0.30;
        run(&mut gate, later_in_stroke, true, 1.0, 1.0);
        assert_eq!(gate.direction(), 1);
        assert_eq!(gate.target(), 0.0);
        run(&mut gate, 0.020, true, -1.0, -1.0);
        assert_eq!(gate.direction(), -1);
        assert_eq!(gate.target(), 0.0);
    }

    #[test]
    fn physical_maximum_rate_onset_preserves_the_first_stab_attack() {
        let mut gate = ScratchGate::new(ScratchPreset::Stab);
        let mut saw_open = false;
        let frames_through_open_window = (STAB_OPEN_END_FRACTION
            * ScratchPreset::Stab.initial_stroke_span()
            / MAXIMUM_SCRATCH_RECORD_RATE
            * SAMPLE_RATE)
            .ceil() as usize;
        for frame in 0..frames_through_open_window {
            gate.process(
                1.0 / SAMPLE_RATE,
                true,
                MAXIMUM_SCRATCH_RECORD_RATE,
                MAXIMUM_SCRATCH_RECORD_RATE,
            );
            if frame == 0 {
                assert_eq!(gate.direction(), 1);
                assert!(gate.moving());
                assert_eq!(gate.target(), 0.0);
            }
            saw_open |= gate.target() == 1.0;
        }
        assert!(
            saw_open,
            "the confirmation must not consume the first Stab pulse"
        );
        assert_eq!(gate.target(), 0.0);
    }

    #[test]
    fn physical_maximum_rate_onset_preserves_every_early_click_event() {
        let clicks = 4;
        for (preset, counted_target) in [
            (ScratchPreset::Transform, 1.0),
            (ScratchPreset::Flare, 0.0),
            (ScratchPreset::Crab, 1.0),
            (ScratchPreset::Orbit, 0.0),
        ] {
            assert_eq!(
                count_maximum_rate_target_runs(preset, clicks, counted_target),
                usize::from(clicks),
                "{preset:?} lost an early event at maximum rate"
            );
        }
    }

    #[test]
    fn outgoing_physical_motion_rejects_predicted_reversal_until_the_crossing_sample() {
        let mut gate = ScratchGate::new(ScratchPreset::Transform);
        gate.process(
            1.0 / SAMPLE_RATE,
            true,
            MAXIMUM_SCRATCH_RECORD_RATE,
            MAXIMUM_SCRATCH_RECORD_RATE,
        );
        assert_eq!(gate.direction(), 1);

        for _ in 0..((REVERSAL_CONFIRM_SECONDS * SAMPLE_RATE).ceil() as usize + 16) {
            gate.process(
                1.0 / SAMPLE_RATE,
                true,
                -MAXIMUM_SCRATCH_RECORD_RATE,
                MAXIMUM_SCRATCH_RECORD_RATE,
            );
        }
        assert_eq!(gate.direction(), 1);

        gate.process(
            1.0 / SAMPLE_RATE,
            true,
            -MAXIMUM_SCRATCH_RECORD_RATE,
            -MAXIMUM_SCRATCH_RECORD_RATE,
        );
        assert_eq!(gate.direction(), -1);
        assert!(
            gate.stroke_progress()
                < 2.0 * MAXIMUM_SCRATCH_RECORD_RATE
                    / SAMPLE_RATE
                    / ScratchPreset::Transform.initial_stroke_span()
        );
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
        let mut gate = ScratchGate::new(ScratchPreset::Transform);
        settle_direction(&mut gate, 1.0);
        run(&mut gate, 0.030, true, 1.0, 1.0);
        run(&mut gate, 0.0061, true, -8.0, -8.0);
        assert_eq!(gate.direction(), -1);
        assert!(gate.stroke_progress() > 0.25);
        assert!(gate.phase() > 0.5);
    }

    #[test]
    fn confirmed_onset_keeps_audible_travel_from_the_confirmation_window() {
        let mut gate = ScratchGate::new(ScratchPreset::Transform);
        run(&mut gate, 0.0041, true, 8.0, 8.0);
        assert_eq!(gate.direction(), 1);
        assert!(gate.stroke_progress() > 0.15);
    }

    #[test]
    fn rejected_intent_reversal_discards_pending_state_without_changing_direction() {
        let mut gate = ScratchGate::new(ScratchPreset::Transform);
        settle_direction(&mut gate, 1.0);
        run(&mut gate, 0.030, true, 1.0, 1.0);
        let travel_before_jitter = gate.stroke_travel;
        let phase_before_jitter = gate.phase();

        run(&mut gate, 0.003, true, -8.0, 8.0);
        assert_eq!(gate.direction(), 1);
        assert!((gate.stroke_travel - (travel_before_jitter + 0.024)).abs() < 1.0e-12);
        assert_ne!(gate.phase(), phase_before_jitter);
        assert!(gate.pending_seconds > 0.002);
        assert_eq!(gate.pending_stroke_travel, 0.0);

        run(&mut gate, 0.001, true, 1.0, 1.0);
        assert_eq!(gate.pending_stroke_travel, 0.0);
        assert!((gate.stroke_travel - (travel_before_jitter + 0.025)).abs() < 1.0e-12);
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
        assert!(gate.phase() > 0.02 && gate.phase() < 0.06);
        assert!(gate.learned_span() > initial_span);
        assert_eq!(gate.span_prediction_confidence(), 0.25);
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
    fn transform_has_a_closed_baseline_with_brief_uniform_taps() {
        let mut gate = ScratchGate::new(ScratchPreset::Transform);
        gate.set_clicks(2);
        gate.contact_active = true;
        gate.direction = 1;
        gate.moving = true;
        let frames = (gate.learned_span() * SAMPLE_RATE) as usize;
        let mut open = 0;
        let mut closed = 0;
        for _ in 0..frames {
            gate.process(1.0 / SAMPLE_RATE, true, 1.0, 1.0);
            if gate.target() == 1.0 {
                open += 1;
            } else {
                closed += 1;
            }
        }
        assert!(
            closed > open * 2,
            "open {open} frames, closed {closed} frames"
        );
        assert_eq!(count_target_runs(ScratchPreset::Transform, 2, 1.0), 2);
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
    fn click_count_does_not_repeat_after_predicted_stroke_endpoint() {
        for clicks in [1, 4, 8] {
            for (preset, counted_target) in [
                (ScratchPreset::Transform, 1.0),
                (ScratchPreset::Flare, 0.0),
                (ScratchPreset::Crab, 1.0),
                (ScratchPreset::Orbit, 0.0),
            ] {
                let mut gate = ScratchGate::new(preset);
                gate.set_clicks(clicks);
                gate.contact_active = true;
                gate.direction = 1;
                gate.moving = true;
                let mut previous = gate.compute_target(1.0, 1.0);
                let mut runs = usize::from(previous == counted_target);
                let frames = (gate.learned_span() * 3.0 * SAMPLE_RATE).ceil() as usize;
                for _ in 0..frames {
                    gate.process(1.0 / SAMPLE_RATE, true, 1.0, 1.0);
                    let current = gate.target();
                    if current == counted_target && previous != counted_target {
                        runs += 1;
                    }
                    previous = current;
                }
                assert_eq!(runs, usize::from(clicks), "{preset:?} at {clicks}");
                assert_eq!(gate.stroke_progress(), 1.0);
            }
        }
    }

    #[test]
    fn first_stroke_seed_has_zero_confidence_until_one_stroke_is_observed() {
        let mut gate = ScratchGate::new(ScratchPreset::Orbit);
        assert_eq!(gate.span_prediction_confidence(), 0.0);
        assert_eq!(
            gate.learned_span(),
            ScratchPreset::Orbit.initial_stroke_span()
        );

        gate.stroke_travel = 0.123;
        gate.learn_completed_stroke();
        assert_eq!(gate.learned_span(), 0.123);
        assert_eq!(gate.span_prediction_confidence(), 0.25);

        for observed in [0.124, 0.122, 0.123] {
            gate.stroke_travel = observed;
            gate.learn_completed_stroke();
        }
        assert_eq!(gate.span_prediction_confidence(), 1.0);
        assert!((gate.learned_span() - 0.123).abs() < 0.001);
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
    fn gate_reports_only_the_current_algorithm() {
        let mut gate = ScratchGate::new(ScratchPreset::Chirp);
        assert_eq!(gate.algorithm_version(), SCRATCH_GATE_ALGORITHM_VERSION);
        gate.set_algorithm_version(SCRATCH_GATE_ALGORITHM_VERSION);
        assert_eq!(gate.algorithm_version(), SCRATCH_GATE_ALGORITHM_VERSION);
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
    fn chirp_closes_on_reversal_intent_before_the_record_changes_direction() {
        let mut gate = ScratchGate::new(ScratchPreset::Chirp);
        settle_direction(&mut gate, 1.0);
        run(&mut gate, 0.010, true, 1.0, 1.0);
        assert_eq!(gate.target(), 1.0);

        gate.process(1.0 / SAMPLE_RATE, true, -1.0, 0.2);
        assert_eq!(gate.direction(), 1);
        assert_eq!(gate.pending_direction, -1);
        assert_eq!(gate.target(), 0.0);

        run(&mut gate, 0.007, true, -1.0, -0.2);
        assert_eq!(gate.direction(), -1);
        assert_eq!(gate.target(), 0.0);
    }

    #[test]
    fn chirp_pause_does_not_expose_the_forward_endpoint() {
        let mut gate = ScratchGate::new(ScratchPreset::Chirp);
        settle_direction(&mut gate, 1.0);
        run(&mut gate, 0.080, true, 1.0, 1.0);
        assert_eq!(gate.target(), 0.0);
        let phase_at_endpoint = gate.phase();

        run(&mut gate, 0.030, true, 0.0, 0.0);
        assert!(!gate.moving());
        assert_eq!(gate.phase(), phase_at_endpoint);
        assert_eq!(gate.target(), 0.0);

        run(&mut gate, 0.030, false, 0.0, 0.0);
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
    fn flare_is_one_sided_while_orbit_repeats_the_notch_on_return() {
        for (preset, expected) in [(ScratchPreset::Flare, 1.0), (ScratchPreset::Orbit, 0.0)] {
            let mut gate = ScratchGate::new(preset);
            gate.set_clicks(1);
            gate.contact_active = true;
            gate.direction = -1;
            gate.moving = true;
            gate.stroke_travel = gate.learned_span() * 0.5;
            gate.update_phase();
            assert_eq!(gate.compute_target(-1.0, -1.0), expected, "{preset:?}");
        }
    }

    #[test]
    fn crab_is_closed_at_rest_and_pulses_while_moving() {
        let mut gate = ScratchGate::new(ScratchPreset::Crab);
        run(&mut gate, 0.020, true, 0.0, 0.0);
        assert_eq!(gate.target(), 0.0);
        settle_direction(&mut gate, 1.0);
        assert_eq!(gate.target(), 0.0);
        let travel_to_first_pulse = gate.learned_span() * CRAB_BURST_START - gate.stroke_travel;
        run(&mut gate, travel_to_first_pulse, true, 1.0, 1.0);
        assert_eq!(gate.target(), 1.0);
        let travel_past_burst = gate.learned_span() * (CRAB_BURST_END + 0.05) - gate.stroke_travel;
        run(&mut gate, travel_past_burst, true, 1.0, 1.0);
        assert_eq!(gate.target(), 0.0);
    }

    #[test]
    fn crab_is_a_clustered_finger_burst_not_a_transform_duty_variant() {
        let clicks = 4;
        assert!(!crab_pulse_is_open(0.0, clicks));
        assert!(!crab_pulse_is_open(0.90, clicks));
        let samples = 20_000;
        let mut previous = false;
        let mut runs = 0;
        for index in 0..=samples {
            let progress = index as f64 / samples as f64;
            let open = crab_pulse_is_open(progress, clicks);
            if open && !previous {
                runs += 1;
            }
            previous = open;
        }
        assert_eq!(runs, usize::from(clicks));
        assert!(crab_pulse_is_open(CRAB_BURST_START, clicks));
        assert_eq!(
            crab_pulse_is_open(0.10, clicks),
            false,
            "crab must stay closed where transform starts open"
        );
    }

    #[test]
    fn drum_opens_on_attack_then_closes_and_respects_refractory() {
        let mut gate = ScratchGate::new(ScratchPreset::Drum);
        settle_direction(&mut gate, 0.5);
        assert_eq!(gate.target(), 1.0);
        run(&mut gate, 0.034, true, 0.5, 0.5);
        assert_eq!(gate.target(), 0.0);

        gate.process(1.0 / SAMPLE_RATE, true, 1.5, 1.5);
        assert_eq!(gate.target(), 0.0, "early acceleration must not retrigger");
        run(&mut gate, 0.003, true, 1.5, 0.5);
        gate.process(1.0 / SAMPLE_RATE, true, 1.5, 1.5);
        assert_eq!(gate.target(), 1.0);
    }

    #[test]
    fn drum_reversal_does_not_spend_its_hit_on_outgoing_motion() {
        let mut gate = ScratchGate::new(ScratchPreset::Drum);
        settle_direction(&mut gate, 0.5);
        run(&mut gate, 0.070, true, 0.5, 0.5);
        assert_eq!(gate.target(), 0.0);

        run(&mut gate, 0.007, true, -8.0, 0.5);
        assert_eq!(gate.direction(), 1);
        assert_eq!(gate.target(), 0.0);

        run(&mut gate, 0.010, true, -8.0, 0.5);
        assert_eq!(gate.direction(), 1);
        assert_eq!(gate.target(), 0.0);

        gate.process(1.0 / SAMPLE_RATE, true, -8.0, -8.0);
        assert_eq!(gate.target(), 1.0);
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
    fn drum_rendered_acceleration_is_sample_rate_invariant() {
        let trigger_at = |sample_rate: f64| {
            let mut gate = ScratchGate::new(ScratchPreset::Drum);
            let run_at_rate = |gate: &mut ScratchGate, seconds: f64, intent: f64, rendered: f64| {
                let frames = (seconds * sample_rate).round() as usize;
                for _ in 0..frames {
                    gate.process(1.0 / sample_rate, true, intent, rendered);
                }
            };
            run_at_rate(&mut gate, 0.070, 0.5, 0.5);
            assert_eq!(gate.target(), 0.0);
            run_at_rate(&mut gate, 0.001, 0.5, 1.5);
            gate.target()
        };

        assert_eq!(trigger_at(44_100.0), 1.0);
        assert_eq!(trigger_at(48_000.0), 1.0);
        assert_eq!(trigger_at(96_000.0), 1.0);
    }

    #[test]
    fn drum_is_invariant_to_intent_event_packetization_for_same_physical_trajectory() {
        let trace = |packet_frames: usize| {
            let mut performance = ScratchPerformance::new(ScratchPreset::Drum);
            let mut result = Vec::with_capacity(8_000);
            for frame in 0..8_000 {
                let rendered = if frame < 3_000 { 0.5 } else { 1.5 };
                let packet = frame / packet_frames;
                let intent = if packet % 2 == 0 { 0.35 } else { 0.85 };
                let output = performance
                    .process_frame(performance_input(true, intent, rendered, 0.0))
                    .unwrap();
                result.push((
                    output.automatic_gate_target,
                    output.automatic_gate_gain,
                    output.audible_gain,
                ));
            }
            result
        };

        assert_eq!(trace(1), trace(64));
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
            let closed_ratio = measure(&mut ScratchGate::new(ScratchPreset::Stab), -1.0, transient);
            let open_ratio = measure(&mut ScratchGate::new(ScratchPreset::Stab), 1.0, transient);
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

    #[test]
    fn performance_timing_is_sample_rate_invariant() {
        let render = |sample_rate: f64| {
            let mut performance = ScratchPerformance::new(ScratchPreset::Transform);
            let input = ScratchPerformanceInput {
                delta_seconds: 1.0 / sample_rate,
                hand_contact: true,
                intent_record_rate: 0.9,
                rendered_record_rate: 0.9,
                rendered_source_travel_seconds: 0.9 / sample_rate,
                manual_crossfader_gain: 0.0,
            };
            let frames = (sample_rate * 0.073).round() as usize;
            let mut output = performance.process_frame(input).unwrap();
            for _ in 1..frames {
                output = performance.process_frame(input).unwrap();
            }
            output
        };
        let at_44 = render(44_100.0);
        let at_48 = render(48_000.0);
        let at_96 = render(96_000.0);
        assert_eq!(at_44.direction, at_48.direction);
        assert_eq!(at_48.direction, at_96.direction);
        assert!((at_44.phase - at_48.phase).abs() < 0.001);
        assert!((at_48.phase - at_96.phase).abs() < 0.001);
        assert!((at_44.audible_gain - at_96.audible_gain).abs() < 0.01);
    }

    #[test]
    fn physical_rendered_motion_is_the_only_source_of_pattern_travel() {
        let mut performance = ScratchPerformance::new(ScratchPreset::Transform);
        let mut no_travel = performance_input(true, 4.0, 0.5, 1.0);
        no_travel.rendered_source_travel_seconds = 0.0;
        for _ in 0..2_000 {
            performance.process_frame(no_travel).unwrap();
        }
        assert_eq!(performance.gate().stroke_progress(), 0.0);

        for _ in 0..2_000 {
            performance
                .process_frame(performance_input(true, 4.0, 0.5, 1.0))
                .unwrap();
        }
        assert!(performance.gate().stroke_progress() > 0.0);
    }

    #[test]
    fn baby_uses_manual_gain_and_automatic_presets_own_the_output() {
        let mut performance = ScratchPerformance::new(ScratchPreset::Baby);
        let mut output = performance
            .process_frame(performance_input(false, 0.0, 0.0, 0.0))
            .unwrap();
        assert_eq!(output.owner, ScratchCrossfaderOwner::Manual);
        for _ in 0..2_000 {
            output = performance
                .process_frame(performance_input(false, 0.0, 0.0, 0.0))
                .unwrap();
        }
        assert!(output.audible_gain < 1.0e-9);

        performance.set_preset(ScratchPreset::Stab);
        for _ in 0..8_000 {
            output = performance
                .process_frame(performance_input(true, 1.0, 1.0, 0.0))
                .unwrap();
        }
        assert_eq!(output.owner, ScratchCrossfaderOwner::AutomaticPreset);
        assert!(output.audible_gain < 1.0e-9, "stab closes after its pulse");

        performance.set_preset(ScratchPreset::Baby);
        for _ in 0..2_000 {
            output = performance
                .process_frame(performance_input(true, 0.0, 0.0, 0.75))
                .unwrap();
        }
        assert!((output.audible_gain - 0.75).abs() < 1.0e-9);
    }

    #[test]
    fn equivalent_rendered_travel_ignores_positive_intent_jitter_and_coalescing() {
        let trace = |coalesced: bool| {
            let mut performance = ScratchPerformance::new(ScratchPreset::Orbit);
            performance.set_clicks(4);
            for frame in 0..12_000 {
                let intent = if coalesced {
                    if (frame / 64) % 2 == 0 {
                        0.65
                    } else {
                        0.85
                    }
                } else if frame % 2 == 0 {
                    0.70
                } else {
                    0.80
                };
                performance
                    .process_frame(performance_input(true, intent, 0.75, 1.0))
                    .unwrap();
            }
            (
                performance.gate().direction(),
                performance.gate().phase(),
                performance.gate().stroke_progress(),
                performance.gate().target(),
            )
        };
        assert_eq!(trace(false), trace(true));
    }

    #[test]
    fn rapid_physical_reversals_remain_bounded_and_hide_each_endpoint() {
        let mut performance = ScratchPerformance::new(ScratchPreset::Chirp);
        for stroke in 0..32 {
            let direction = if stroke % 2 == 0 { 8.0 } else { -8.0 };
            for _ in 0..400 {
                let output = performance
                    .process_frame(performance_input(true, direction, direction, 1.0))
                    .unwrap();
                assert!((0.0..=1.0).contains(&output.audible_gain));
            }
            let pending_direction = -direction;
            let endpoint = performance
                .process_frame(performance_input(
                    true,
                    pending_direction,
                    direction * 0.02,
                    1.0,
                ))
                .unwrap();
            assert_eq!(endpoint.automatic_gate_target, 0.0);
        }
    }

    #[test]
    fn maximum_rate_reversals_remain_finite_and_bounded() {
        let mut performance = ScratchPerformance::new(ScratchPreset::Crab);
        performance.set_clicks(8);
        for stroke in 0..64 {
            let rate = if stroke % 2 == 0 { 20.0 } else { -20.0 };
            for _ in 0..300 {
                let output = performance
                    .process_frame(performance_input(true, rate, rate, 0.5))
                    .unwrap();
                assert!(output.audible_gain.is_finite());
                assert!((0.0..=1.0).contains(&output.audible_gain));
                assert!(output.phase.is_finite());
                assert!((0.0..=1.0).contains(&output.phase));
            }
        }
    }

    #[test]
    fn performance_snapshot_restore_repeats_every_output_frame() {
        let mut performance = ScratchPerformance::new(ScratchPreset::Crab);
        performance.set_clicks(5);
        for _ in 0..1_000 {
            performance
                .process_frame(performance_input(true, 1.2, 0.9, 0.4))
                .unwrap();
        }
        let snapshot = performance.snapshot();
        let mut expected = [performance
            .process_frame(performance_input(true, -1.4, -0.7, 0.2))
            .unwrap(); 128];
        for output in &mut expected[1..] {
            *output = performance
                .process_frame(performance_input(true, -1.4, -0.7, 0.2))
                .unwrap();
        }
        performance.restore(&snapshot).unwrap();
        for expected_output in expected {
            assert_eq!(
                performance
                    .process_frame(performance_input(true, -1.4, -0.7, 0.2))
                    .unwrap(),
                expected_output
            );
        }
    }

    #[test]
    fn invalid_input_and_snapshot_restore_are_transactional() {
        let mut performance = ScratchPerformance::new(ScratchPreset::Orbit);
        performance
            .process_frame(performance_input(true, 1.0, 1.0, 0.5))
            .unwrap();
        let before = performance.snapshot();
        let mut invalid_input = performance_input(true, 1.0, 1.0, 0.5);
        invalid_input.rendered_record_rate = f64::NAN;
        assert_eq!(
            performance.process_frame(invalid_input),
            Err(ScratchPerformanceError::InvalidInput {
                field: "renderedRecordRate"
            })
        );
        assert_eq!(performance.snapshot(), before);

        let mut excessive_rate = performance_input(true, 1.0, 1.0, 0.5);
        excessive_rate.rendered_record_rate = MAXIMUM_SCRATCH_RECORD_RATE + 0.001;
        assert_eq!(
            performance.process_frame(excessive_rate),
            Err(ScratchPerformanceError::InvalidInput {
                field: "renderedRecordRate"
            })
        );
        assert_eq!(performance.snapshot(), before);

        let mut excessive_travel = performance_input(true, 1.0, 1.0, 0.5);
        excessive_travel.rendered_source_travel_seconds =
            MAXIMUM_SCRATCH_RECORD_RATE * excessive_travel.delta_seconds + f64::EPSILON;
        assert_eq!(
            performance.process_frame(excessive_travel),
            Err(ScratchPerformanceError::InvalidInput {
                field: "renderedSourceTravelSeconds"
            })
        );
        assert_eq!(performance.snapshot(), before);

        let mut previous_performance_version = before;
        previous_performance_version.version = SCRATCH_PERFORMANCE_SNAPSHOT_VERSION - 1;
        assert_eq!(
            performance.restore(&previous_performance_version),
            Err(ScratchPerformanceError::UnsupportedPerformanceSnapshotVersion { version: 1 })
        );
        assert_eq!(performance.snapshot(), before);

        let mut previous_gate_version = before;
        previous_gate_version.gate.version = SCRATCH_GATE_SNAPSHOT_VERSION - 1;
        assert_eq!(
            performance.restore(&previous_gate_version),
            Err(ScratchPerformanceError::UnsupportedGateSnapshotVersion { version: 1 })
        );
        assert_eq!(performance.snapshot(), before);

        let mut invalid_snapshot = before.clone();
        invalid_snapshot.gate.state.phase = f64::NAN;
        assert_eq!(
            performance.restore(&invalid_snapshot),
            Err(ScratchPerformanceError::InvalidSnapshot { field: "phase" })
        );
        assert_eq!(performance.snapshot(), before);
    }

    #[test]
    fn performance_snapshot_round_trips_through_json() {
        let mut performance = ScratchPerformance::new(ScratchPreset::Flare);
        for _ in 0..2_000 {
            performance
                .process_frame(performance_input(true, 0.8, 0.7, 0.3))
                .unwrap();
        }
        let snapshot = performance.snapshot();
        let json = serde_json::to_string(&snapshot).unwrap();
        let decoded: ScratchPerformanceSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn performance_render_path_does_not_allocate() {
        let mut performance = ScratchPerformance::new(ScratchPreset::Crab);
        performance.set_clicks(8);
        let input = performance_input(true, 8.0, 7.5, 0.25);
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..10_000 {
                let output = performance.process_frame(input).unwrap();
                std::hint::black_box(output);
            }
        });
    }

    #[test]
    fn manual_and_automatic_gain_steps_use_a_bounded_declick_envelope() {
        let maximum_alpha = 1.0 - (-(1.0 / SAMPLE_RATE) / 0.00035_f64).exp();
        for preset in [ScratchPreset::Baby, ScratchPreset::Transform] {
            let mut performance = ScratchPerformance::new(preset);
            let input = if preset == ScratchPreset::Baby {
                performance_input(true, 8.0, 8.0, 0.0)
            } else {
                performance_input(true, -8.0, -8.0, 1.0)
            };
            let before = performance.audible_gain();
            let after = performance.process_frame(input).unwrap().audible_gain;
            assert!(after <= before);
            assert!(before - after <= maximum_alpha + 1.0e-12);
        }
    }
}
