use serde::{Deserialize, Serialize};
use thiserror::Error;

const SNAPSHOT_VERSION: u32 = 2;
const MIN_SAMPLE_RATE_HZ: f64 = 8_000.0;
const MAX_SAMPLE_RATE_HZ: f64 = 768_000.0;
const MIN_GROOVE_RADIUS_M: f64 = 0.03;
const MAX_GROOVE_RADIUS_M: f64 = 0.20;
const MIN_GROOVE_PITCH_M_PER_REVOLUTION: f64 = 20.0e-6;
const MAX_GROOVE_PITCH_M_PER_REVOLUTION: f64 = 2.0e-3;
const MIN_GROOVE_TOP_WIDTH_M: f64 = 1.0e-6;
const MAX_GROOVE_TOP_WIDTH_M: f64 = 1.0e-3;
const MAX_APERTURE_MARGIN_M: f64 = 1.0e-3;
const MAX_ABSOLUTE_RADIUS_M: f64 = 2.0;
const MAX_ABSOLUTE_VELOCITY_M_S: f64 = 100.0;
const MAX_ABSOLUTE_FORCE_N: f64 = 100.0;
const MAX_RECORD_ANGLE_DELTA_TURNS: f64 = 64.0;
const MAX_ABSOLUTE_TURN_INDEX: i64 = 1_000_000;
const MAX_TURNS_PER_RECAPTURE: u32 = 1_024;
const HALF_TURN_TIE_TOLERANCE: f64 = 1.0e-9;

/// Defines hysteresis and work limits for adjacent-turn selection.
///
/// The pickup contact solver owns all lateral mass, force, and bearing state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadialTrackingConfig {
    /// Contact releases outside the measured groove aperture plus this margin.
    pub contact_release_margin_m: f64,
    /// Contact captures inside the measured groove aperture by this distance.
    pub recapture_inset_m: f64,
    pub maximum_turns_per_recapture: u32,
}

impl RadialTrackingConfig {
    /// Returns estimated aperture margins and a defined search limit.
    pub fn sl_1200mk7_estimated_seed() -> Self {
        Self {
            contact_release_margin_m: 2.0e-6,
            recapture_inset_m: 5.0e-6,
            maximum_turns_per_recapture: 32,
        }
    }

    pub fn validate(self) -> Result<Self, RadialTrackingError> {
        for (field, value) in [
            ("contactReleaseMarginM", self.contact_release_margin_m),
            ("recaptureInsetM", self.recapture_inset_m),
        ] {
            if !value.is_finite() || !(0.0..=MAX_APERTURE_MARGIN_M).contains(&value) {
                return Err(RadialTrackingError::InvalidConfig { field });
            }
        }
        if !(1..=MAX_TURNS_PER_RECAPTURE).contains(&self.maximum_turns_per_recapture) {
            return Err(RadialTrackingError::InvalidConfig {
                field: "maximumTurnsPerRecapture",
            });
        }
        Ok(self)
    }

    pub fn validate_for_groove(self, groove_top_width_m: f64) -> Result<Self, RadialTrackingError> {
        self.validate()?;
        validate_groove_top_width(groove_top_width_m)?;
        if self.recapture_inset_m >= 0.5 * groove_top_width_m {
            return Err(RadialTrackingError::InvalidConfig {
                field: "recaptureInsetM",
            });
        }
        Ok(self)
    }
}

impl Default for RadialTrackingConfig {
    fn default() -> Self {
        Self::sl_1200mk7_estimated_seed()
    }
}

/// Identifies the physical record feature below the stylus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RadialContactRegion {
    #[default]
    Groove,
    Land,
    Lifted,
}

/// Inputs for one bounded turn-selection sample.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadialTrackingInput {
    /// Positive turns move the intended spiral inward.
    pub record_angle_delta_turns: f64,
    pub groove_pitch_m_per_revolution: f64,
    pub groove_top_width_m: f64,
    /// This coordinate uses the previous sample's spiral origin.
    pub stylus_lateral_position_m: f64,
    /// Velocity uses the stationary deck frame.
    pub stylus_radial_velocity_m_s: f64,
    pub stylus_lowered: bool,
    /// These inclusive bounds identify source turns that exist in the asset.
    pub minimum_available_turn_index: i64,
    pub maximum_available_turn_index: i64,
}

impl Default for RadialTrackingInput {
    fn default() -> Self {
        Self {
            record_angle_delta_turns: 0.0,
            groove_pitch_m_per_revolution: 125.0e-6,
            groove_top_width_m: 50.0e-6,
            stylus_lateral_position_m: 0.0,
            stylus_radial_velocity_m_s: 0.0,
            stylus_lowered: true,
            minimum_available_turn_index: -1_000,
            maximum_available_turn_index: 1_000,
        }
    }
}

/// Reports same-sample forces after the pickup solver completes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadialTrackingObservation {
    pub stylus_lateral_position_m: f64,
    pub stylus_radial_velocity_m_s: f64,
    pub groove_lateral_force_on_tip_n: f64,
    pub bearing_friction_force_n: f64,
    pub anti_skate_force_n: f64,
    pub skating_force_n: f64,
    pub physical_surface_contact: bool,
    pub land_contact: bool,
}

/// Selects the record surface and source turn for one fixed sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadialTrackingSelection {
    pub coordinate_origin_shift_m: f64,
    pub contact_region: RadialContactRegion,
    pub selected_turn_index: Option<i64>,
    pub groove_center_lateral_m: f64,
    pub completed_steps: u64,
}

/// Stores a torque-independent radial branch for one midpoint sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RadialTrackingMidpointPreparation {
    source: RadialTrackingState,
    midpoint: RadialTrackingState,
    input: RadialTrackingInput,
    selection: RadialTrackingSelection,
}

impl RadialTrackingMidpointPreparation {
    pub(crate) const fn selection(self) -> RadialTrackingSelection {
        self.selection
    }
}

impl RadialTrackingSelection {
    pub fn source_turn_offset(self) -> Option<i64> {
        self.selected_turn_index.map(|index| -index)
    }
}

/// Output from one fixed turn-selection sample.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadialTrackingTelemetry {
    pub spiral_reference_radius_m: f64,
    pub stylus_radius_m: f64,
    pub radial_velocity_m_s: f64,
    pub groove_pitch_m_per_revolution: f64,
    pub captured_groove_radius_m: Option<f64>,
    pub radial_error_to_retained_turn_m: f64,
    pub groove_radial_velocity_m_s: f64,
    /// This is the lateral component of the groove-wall force on the stylus.
    pub guide_force_n: f64,
    pub bearing_friction_force_n: f64,
    /// This sum contains anti-skate, skating, and bearing forces on the arm body.
    pub applied_radial_force_n: f64,
    pub groove_contact: bool,
    pub land_contact: bool,
    pub contact_region: RadialContactRegion,
    pub contact_lost_this_step: bool,
    pub recaptured_this_step: bool,
    pub recapture_limit_reached: bool,
    /// Positive indices select turns outside the intended spiral turn.
    pub captured_turn_index: i64,
    /// Positive values are outward skips. Negative values are inward skips.
    pub turns_skipped_this_step: i64,
    pub total_turns_skipped: i64,
    pub stylus_lowered: bool,
    /// This reports actual contact with a groove wall or the land surface.
    pub macro_contact_available: bool,
    pub completed_steps: u64,
}

impl RadialTrackingTelemetry {
    /// Returns the source-turn offset for the captured groove.
    ///
    /// An outer physical turn contains earlier source material.
    pub fn source_turn_offset(self) -> i64 {
        -self.captured_turn_index
    }

    pub fn source_turn_delta_this_step(self) -> i64 {
        -self.turns_skipped_this_step
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadialTrackingSnapshot {
    version: u32,
    config: RadialTrackingConfig,
    sample_rate_hz: f64,
    spiral_reference_radius_m: f64,
    groove_pitch_m_per_revolution: f64,
    retained_turn_index: i64,
    contact_region: RadialContactRegion,
    total_turns_skipped: i64,
    completed_steps: u64,
    last_telemetry: RadialTrackingTelemetry,
}

/// Tracks the moving spiral frame and bounded adjacent-turn selection.
///
/// Lateral +x points toward the record edge. The spiral origin uses deck
/// coordinates. If it moves by `delta_r`, local pickup positions move by
/// `-delta_r`. Pickup velocities remain in deck coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadialTrackingState {
    config: RadialTrackingConfig,
    sample_rate_hz: f64,
    spiral_reference_radius_m: f64,
    groove_pitch_m_per_revolution: f64,
    retained_turn_index: i64,
    contact_region: RadialContactRegion,
    total_turns_skipped: i64,
    completed_steps: u64,
    last_telemetry: RadialTrackingTelemetry,
}

impl RadialTrackingState {
    pub fn new(
        config: RadialTrackingConfig,
        sample_rate_hz: f64,
        initial_groove_radius_m: f64,
        groove_pitch_m_per_revolution: f64,
    ) -> Result<Self, RadialTrackingError> {
        let config = config.validate()?;
        validate_sample_rate(sample_rate_hz)?;
        validate_initial_radius(initial_groove_radius_m)?;
        validate_groove_pitch(groove_pitch_m_per_revolution)?;
        let last_telemetry =
            initial_telemetry(initial_groove_radius_m, groove_pitch_m_per_revolution);
        Ok(Self {
            config,
            sample_rate_hz,
            spiral_reference_radius_m: initial_groove_radius_m,
            groove_pitch_m_per_revolution,
            retained_turn_index: 0,
            contact_region: RadialContactRegion::Groove,
            total_turns_skipped: 0,
            completed_steps: 0,
            last_telemetry,
        })
    }

    pub fn config(&self) -> RadialTrackingConfig {
        self.config
    }

    pub fn sample_rate_hz(&self) -> f64 {
        self.sample_rate_hz
    }

    pub fn telemetry(&self) -> RadialTrackingTelemetry {
        self.last_telemetry
    }

    pub fn reset(
        &mut self,
        initial_groove_radius_m: f64,
        groove_pitch_m_per_revolution: f64,
    ) -> Result<(), RadialTrackingError> {
        validate_initial_radius(initial_groove_radius_m)?;
        validate_groove_pitch(groove_pitch_m_per_revolution)?;
        self.spiral_reference_radius_m = initial_groove_radius_m;
        self.groove_pitch_m_per_revolution = groove_pitch_m_per_revolution;
        self.retained_turn_index = 0;
        self.contact_region = RadialContactRegion::Groove;
        self.total_turns_skipped = 0;
        self.completed_steps = 0;
        self.last_telemetry =
            initial_telemetry(initial_groove_radius_m, groove_pitch_m_per_revolution);
        Ok(())
    }

    pub fn snapshot(&self) -> RadialTrackingSnapshot {
        RadialTrackingSnapshot {
            version: SNAPSHOT_VERSION,
            config: self.config,
            sample_rate_hz: self.sample_rate_hz,
            spiral_reference_radius_m: self.spiral_reference_radius_m,
            groove_pitch_m_per_revolution: self.groove_pitch_m_per_revolution,
            retained_turn_index: self.retained_turn_index,
            contact_region: self.contact_region,
            total_turns_skipped: self.total_turns_skipped,
            completed_steps: self.completed_steps,
            last_telemetry: self.last_telemetry,
        }
    }

    pub fn restore(&mut self, snapshot: RadialTrackingSnapshot) -> Result<(), RadialTrackingError> {
        validate_snapshot(snapshot)?;
        self.config = snapshot.config;
        self.sample_rate_hz = snapshot.sample_rate_hz;
        self.spiral_reference_radius_m = snapshot.spiral_reference_radius_m;
        self.groove_pitch_m_per_revolution = snapshot.groove_pitch_m_per_revolution;
        self.retained_turn_index = snapshot.retained_turn_index;
        self.contact_region = snapshot.contact_region;
        self.total_turns_skipped = snapshot.total_turns_skipped;
        self.completed_steps = snapshot.completed_steps;
        self.last_telemetry = snapshot.last_telemetry;
        Ok(())
    }

    /// Selects a radial branch without changing the live state.
    pub(crate) fn prepare_midpoint(
        &self,
        input: RadialTrackingInput,
    ) -> Result<RadialTrackingMidpointPreparation, RadialTrackingError> {
        let mut midpoint = *self;
        let selection = midpoint.process(input)?;
        Ok(RadialTrackingMidpointPreparation {
            source: *self,
            midpoint,
            input,
            selection,
        })
    }

    /// Commits an actual angular advance while it keeps the midpoint branch.
    pub(crate) fn commit_midpoint(
        &mut self,
        preparation: RadialTrackingMidpointPreparation,
        actual_record_angle_delta_turns: f64,
    ) -> Result<RadialTrackingSelection, RadialTrackingError> {
        if *self != preparation.source {
            return Err(RadialTrackingError::StaleMidpointPreparation);
        }
        if !actual_record_angle_delta_turns.is_finite()
            || actual_record_angle_delta_turns.abs() > MAX_RECORD_ANGLE_DELTA_TURNS
        {
            return Err(RadialTrackingError::InvalidInput);
        }
        let old_reference_radius_m = self.spiral_reference_radius_m;
        let actual_reference_radius_m = old_reference_radius_m
            - preparation.input.groove_pitch_m_per_revolution * actual_record_angle_delta_turns;
        let actual_origin_shift_m = actual_reference_radius_m - old_reference_radius_m;
        let midpoint_origin_shift_m = preparation.selection.coordinate_origin_shift_m;
        let origin_correction_m = actual_origin_shift_m - midpoint_origin_shift_m;
        let mut next = preparation.midpoint;
        next.spiral_reference_radius_m = actual_reference_radius_m;
        let mut telemetry = next.last_telemetry;
        telemetry.spiral_reference_radius_m = actual_reference_radius_m;
        telemetry.radial_error_to_retained_turn_m -= origin_correction_m;
        telemetry.captured_groove_radius_m =
            preparation.selection.selected_turn_index.map(|turn| {
                actual_reference_radius_m
                    + preparation.input.groove_pitch_m_per_revolution * turn as f64
            });
        let dt = 1.0 / self.sample_rate_hz;
        telemetry.groove_radial_velocity_m_s =
            preparation
                .selection
                .selected_turn_index
                .map_or(actual_origin_shift_m / dt, |turn| {
                    let old_center =
                        old_reference_radius_m + self.groove_pitch_m_per_revolution * turn as f64;
                    let new_center = actual_reference_radius_m
                        + preparation.input.groove_pitch_m_per_revolution * turn as f64;
                    (new_center - old_center) / dt
                });
        validate_telemetry_values(telemetry).map_err(|_| RadialTrackingError::NumericalFailure)?;
        next.last_telemetry = telemetry;
        *self = next;
        Ok(RadialTrackingSelection {
            coordinate_origin_shift_m: actual_origin_shift_m,
            ..preparation.selection
        })
    }

    /// Advances the spiral origin and selects one physical record region.
    pub fn process(
        &mut self,
        input: RadialTrackingInput,
    ) -> Result<RadialTrackingSelection, RadialTrackingError> {
        validate_input(self.config, input)?;
        let dt = 1.0 / self.sample_rate_hz;
        let old_reference_radius_m = self.spiral_reference_radius_m;
        let old_pitch_m = self.groove_pitch_m_per_revolution;
        let spiral_reference_radius_m = old_reference_radius_m
            - input.groove_pitch_m_per_revolution * input.record_angle_delta_turns;
        let coordinate_origin_shift_m = spiral_reference_radius_m - old_reference_radius_m;
        let stylus_lateral_position_m = input.stylus_lateral_position_m - coordinate_origin_shift_m;
        if !valid_state_scalar(spiral_reference_radius_m) || !stylus_lateral_position_m.is_finite()
        {
            return Err(RadialTrackingError::NumericalFailure);
        }

        let was_region = self.contact_region;
        let mut contact_region = self.contact_region;
        let mut retained_turn_index = self.retained_turn_index;
        let mut total_turns_skipped = self.total_turns_skipped;
        let mut contact_lost_this_step = false;
        let mut recaptured_this_step = false;
        let mut recapture_limit_reached = false;
        let mut turns_skipped_this_step = 0_i64;
        let release_half_width_m =
            0.5 * input.groove_top_width_m + self.config.contact_release_margin_m;
        let capture_half_width_m = 0.5 * input.groove_top_width_m - self.config.recapture_inset_m;

        if !input.stylus_lowered {
            contact_lost_this_step = was_region == RadialContactRegion::Groove;
            contact_region = RadialContactRegion::Lifted;
        } else {
            let retained_available = turn_is_available(retained_turn_index, input);
            let retained_center_m =
                input.groove_pitch_m_per_revolution * retained_turn_index as f64;
            let retained_error_m = stylus_lateral_position_m - retained_center_m;
            if contact_region == RadialContactRegion::Groove
                && (!retained_available || retained_error_m.abs() > release_half_width_m)
            {
                contact_region = RadialContactRegion::Land;
                contact_lost_this_step = true;
            }

            if contact_region != RadialContactRegion::Groove && !contact_lost_this_step {
                let recapture = find_recapture(
                    stylus_lateral_position_m,
                    input.groove_pitch_m_per_revolution,
                    retained_turn_index,
                    capture_half_width_m,
                    input,
                    self.config,
                )?;
                recapture_limit_reached = recapture.limit_reached;
                if let Some(candidate) = recapture.turn_index {
                    turns_skipped_this_step = turn_delta(candidate, retained_turn_index)?;
                    retained_turn_index = candidate;
                    total_turns_skipped =
                        total_turns_skipped.saturating_add(turns_skipped_this_step);
                    contact_region = RadialContactRegion::Groove;
                    recaptured_this_step = true;
                } else {
                    contact_region = RadialContactRegion::Land;
                }
            }
        }

        let selected_turn_index =
            (contact_region == RadialContactRegion::Groove).then_some(retained_turn_index);
        let groove_center_lateral_m =
            input.groove_pitch_m_per_revolution * retained_turn_index as f64;
        let captured_groove_radius_m = selected_turn_index.map(|turn| {
            spiral_reference_radius_m + input.groove_pitch_m_per_revolution * turn as f64
        });
        let groove_radial_velocity_m_s =
            selected_turn_index.map_or(coordinate_origin_shift_m / dt, |turn| {
                let old_center = old_reference_radius_m + old_pitch_m * turn as f64;
                let new_center =
                    spiral_reference_radius_m + input.groove_pitch_m_per_revolution * turn as f64;
                (new_center - old_center) / dt
            });
        let completed_steps = self.completed_steps.saturating_add(1);
        let telemetry = RadialTrackingTelemetry {
            spiral_reference_radius_m,
            stylus_radius_m: spiral_reference_radius_m + stylus_lateral_position_m,
            radial_velocity_m_s: input.stylus_radial_velocity_m_s,
            groove_pitch_m_per_revolution: input.groove_pitch_m_per_revolution,
            captured_groove_radius_m,
            radial_error_to_retained_turn_m: stylus_lateral_position_m - groove_center_lateral_m,
            groove_radial_velocity_m_s,
            guide_force_n: 0.0,
            bearing_friction_force_n: 0.0,
            applied_radial_force_n: 0.0,
            groove_contact: contact_region == RadialContactRegion::Groove,
            land_contact: false,
            contact_region,
            contact_lost_this_step,
            recaptured_this_step,
            recapture_limit_reached,
            captured_turn_index: retained_turn_index,
            turns_skipped_this_step,
            total_turns_skipped,
            stylus_lowered: input.stylus_lowered,
            macro_contact_available: false,
            completed_steps,
        };
        validate_telemetry_values(telemetry).map_err(|_| RadialTrackingError::NumericalFailure)?;

        self.spiral_reference_radius_m = spiral_reference_radius_m;
        self.groove_pitch_m_per_revolution = input.groove_pitch_m_per_revolution;
        self.retained_turn_index = retained_turn_index;
        self.contact_region = contact_region;
        self.total_turns_skipped = total_turns_skipped;
        self.completed_steps = completed_steps;
        self.last_telemetry = telemetry;
        Ok(RadialTrackingSelection {
            coordinate_origin_shift_m,
            contact_region,
            selected_turn_index,
            groove_center_lateral_m,
            completed_steps,
        })
    }

    /// Adds the same-sample pickup result without changing turn selection.
    pub fn observe_pickup(
        &mut self,
        selection: RadialTrackingSelection,
        observation: RadialTrackingObservation,
    ) -> Result<RadialTrackingTelemetry, RadialTrackingError> {
        validate_observation(observation)?;
        if selection.completed_steps != self.completed_steps
            || selection.contact_region != self.contact_region
            || selection.selected_turn_index
                != (self.contact_region == RadialContactRegion::Groove)
                    .then_some(self.retained_turn_index)
        {
            return Err(RadialTrackingError::StaleSelection);
        }
        let mut telemetry = self.last_telemetry;
        telemetry.stylus_radius_m =
            self.spiral_reference_radius_m + observation.stylus_lateral_position_m;
        telemetry.radial_velocity_m_s = observation.stylus_radial_velocity_m_s;
        telemetry.radial_error_to_retained_turn_m =
            observation.stylus_lateral_position_m - selection.groove_center_lateral_m;
        telemetry.guide_force_n = observation.groove_lateral_force_on_tip_n;
        telemetry.bearing_friction_force_n = observation.bearing_friction_force_n;
        telemetry.applied_radial_force_n = observation.anti_skate_force_n
            + observation.skating_force_n
            + observation.bearing_friction_force_n;
        telemetry.land_contact =
            selection.contact_region == RadialContactRegion::Land && observation.land_contact;
        telemetry.macro_contact_available = observation.physical_surface_contact;
        validate_telemetry_values(telemetry).map_err(|_| RadialTrackingError::NumericalFailure)?;
        self.last_telemetry = telemetry;
        Ok(telemetry)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Recapture {
    turn_index: Option<i64>,
    limit_reached: bool,
}

fn find_recapture(
    stylus_lateral_position_m: f64,
    groove_pitch_m_per_revolution: f64,
    retained_turn_index: i64,
    capture_half_width_m: f64,
    input: RadialTrackingInput,
    config: RadialTrackingConfig,
) -> Result<Recapture, RadialTrackingError> {
    let relative_turns = stylus_lateral_position_m / groove_pitch_m_per_revolution;
    if !relative_turns.is_finite() || relative_turns.abs() > MAX_ABSOLUTE_TURN_INDEX as f64 + 0.5 {
        return Err(RadialTrackingError::NumericalFailure);
    }
    let candidate = nearest_turn_index(relative_turns);
    if candidate.unsigned_abs() > MAX_ABSOLUTE_TURN_INDEX as u64 {
        return Err(RadialTrackingError::NumericalFailure);
    }
    let delta = turn_delta(candidate, retained_turn_index)?;
    if delta.unsigned_abs() > u64::from(config.maximum_turns_per_recapture) {
        return Ok(Recapture {
            turn_index: None,
            limit_reached: true,
        });
    }
    if !turn_is_available(candidate, input) {
        return Ok(Recapture {
            turn_index: None,
            limit_reached: false,
        });
    }
    let candidate_center_m = groove_pitch_m_per_revolution * candidate as f64;
    Ok(Recapture {
        turn_index: ((stylus_lateral_position_m - candidate_center_m).abs()
            <= capture_half_width_m)
            .then_some(candidate),
        limit_reached: false,
    })
}

fn turn_is_available(turn_index: i64, input: RadialTrackingInput) -> bool {
    (input.minimum_available_turn_index..=input.maximum_available_turn_index).contains(&turn_index)
}

fn nearest_turn_index(relative_turns: f64) -> i64 {
    let lower_turn = relative_turns.floor();
    let fraction = relative_turns - lower_turn;
    if (fraction - 0.5).abs() <= HALF_TURN_TIE_TOLERANCE {
        if relative_turns.is_sign_negative() {
            lower_turn as i64
        } else {
            (lower_turn + 1.0) as i64
        }
    } else {
        relative_turns.round() as i64
    }
}

fn turn_delta(candidate: i64, retained: i64) -> Result<i64, RadialTrackingError> {
    candidate
        .checked_sub(retained)
        .ok_or(RadialTrackingError::NumericalFailure)
}

fn validate_sample_rate(sample_rate_hz: f64) -> Result<(), RadialTrackingError> {
    if sample_rate_hz.is_finite()
        && (MIN_SAMPLE_RATE_HZ..=MAX_SAMPLE_RATE_HZ).contains(&sample_rate_hz)
    {
        Ok(())
    } else {
        Err(RadialTrackingError::InvalidSampleRate)
    }
}

fn validate_initial_radius(radius_m: f64) -> Result<(), RadialTrackingError> {
    if radius_m.is_finite() && (MIN_GROOVE_RADIUS_M..=MAX_GROOVE_RADIUS_M).contains(&radius_m) {
        Ok(())
    } else {
        Err(RadialTrackingError::InvalidInitialRadius)
    }
}

fn validate_groove_pitch(pitch_m: f64) -> Result<(), RadialTrackingError> {
    if pitch_m.is_finite()
        && (MIN_GROOVE_PITCH_M_PER_REVOLUTION..=MAX_GROOVE_PITCH_M_PER_REVOLUTION)
            .contains(&pitch_m)
    {
        Ok(())
    } else {
        Err(RadialTrackingError::InvalidGroovePitch)
    }
}

fn validate_groove_top_width(width_m: f64) -> Result<(), RadialTrackingError> {
    if width_m.is_finite() && (MIN_GROOVE_TOP_WIDTH_M..=MAX_GROOVE_TOP_WIDTH_M).contains(&width_m) {
        Ok(())
    } else {
        Err(RadialTrackingError::InvalidGrooveTopWidth)
    }
}

fn validate_input(
    config: RadialTrackingConfig,
    input: RadialTrackingInput,
) -> Result<(), RadialTrackingError> {
    config.validate_for_groove(input.groove_top_width_m)?;
    validate_groove_pitch(input.groove_pitch_m_per_revolution)?;
    if input.groove_top_width_m >= input.groove_pitch_m_per_revolution
        || !input.record_angle_delta_turns.is_finite()
        || input.record_angle_delta_turns.abs() > MAX_RECORD_ANGLE_DELTA_TURNS
        || !input.stylus_lateral_position_m.is_finite()
        || !input.stylus_radial_velocity_m_s.is_finite()
        || input.stylus_radial_velocity_m_s.abs() > MAX_ABSOLUTE_VELOCITY_M_S
        || input.minimum_available_turn_index > input.maximum_available_turn_index
        || input.minimum_available_turn_index.unsigned_abs() > MAX_ABSOLUTE_TURN_INDEX as u64
        || input.maximum_available_turn_index.unsigned_abs() > MAX_ABSOLUTE_TURN_INDEX as u64
    {
        return Err(RadialTrackingError::InvalidInput);
    }
    Ok(())
}

fn validate_observation(observation: RadialTrackingObservation) -> Result<(), RadialTrackingError> {
    if !observation.stylus_lateral_position_m.is_finite()
        || !observation.stylus_radial_velocity_m_s.is_finite()
        || observation.stylus_radial_velocity_m_s.abs() > MAX_ABSOLUTE_VELOCITY_M_S
        || [
            observation.groove_lateral_force_on_tip_n,
            observation.bearing_friction_force_n,
            observation.anti_skate_force_n,
            observation.skating_force_n,
        ]
        .into_iter()
        .any(|force| !force.is_finite() || force.abs() > MAX_ABSOLUTE_FORCE_N)
    {
        return Err(RadialTrackingError::InvalidObservation);
    }
    Ok(())
}

fn valid_state_scalar(value: f64) -> bool {
    value.is_finite() && value.abs() <= MAX_ABSOLUTE_RADIUS_M
}

fn validate_snapshot(snapshot: RadialTrackingSnapshot) -> Result<(), RadialTrackingError> {
    if snapshot.version != SNAPSHOT_VERSION {
        return Err(RadialTrackingError::InvalidSnapshot);
    }
    snapshot
        .config
        .validate()
        .map_err(|_| RadialTrackingError::InvalidSnapshot)?;
    validate_sample_rate(snapshot.sample_rate_hz)
        .map_err(|_| RadialTrackingError::InvalidSnapshot)?;
    validate_groove_pitch(snapshot.groove_pitch_m_per_revolution)
        .map_err(|_| RadialTrackingError::InvalidSnapshot)?;
    if !valid_state_scalar(snapshot.spiral_reference_radius_m)
        || snapshot.retained_turn_index.unsigned_abs() > MAX_ABSOLUTE_TURN_INDEX as u64
    {
        return Err(RadialTrackingError::InvalidSnapshot);
    }
    validate_telemetry_values(snapshot.last_telemetry)
        .map_err(|_| RadialTrackingError::InvalidSnapshot)?;
    let telemetry = snapshot.last_telemetry;
    let expected_captured_radius_m = (snapshot.contact_region == RadialContactRegion::Groove)
        .then_some(
            snapshot.spiral_reference_radius_m
                + snapshot.groove_pitch_m_per_revolution * snapshot.retained_turn_index as f64,
        );
    if telemetry.spiral_reference_radius_m != snapshot.spiral_reference_radius_m
        || telemetry.groove_pitch_m_per_revolution != snapshot.groove_pitch_m_per_revolution
        || telemetry.captured_groove_radius_m != expected_captured_radius_m
        || telemetry.groove_contact != (snapshot.contact_region == RadialContactRegion::Groove)
        || telemetry.contact_region != snapshot.contact_region
        || telemetry.captured_turn_index != snapshot.retained_turn_index
        || telemetry.total_turns_skipped != snapshot.total_turns_skipped
        || telemetry.completed_steps != snapshot.completed_steps
    {
        return Err(RadialTrackingError::InvalidSnapshot);
    }
    Ok(())
}

fn validate_telemetry_values(
    telemetry: RadialTrackingTelemetry,
) -> Result<(), RadialTrackingError> {
    let values_are_finite = [
        telemetry.spiral_reference_radius_m,
        telemetry.stylus_radius_m,
        telemetry.radial_velocity_m_s,
        telemetry.groove_pitch_m_per_revolution,
        telemetry.radial_error_to_retained_turn_m,
        telemetry.groove_radial_velocity_m_s,
        telemetry.guide_force_n,
        telemetry.bearing_friction_force_n,
        telemetry.applied_radial_force_n,
    ]
    .iter()
    .all(|value| value.is_finite());
    if !values_are_finite
        || telemetry
            .captured_groove_radius_m
            .is_some_and(|radius_m| !radius_m.is_finite())
        || telemetry.groove_contact != telemetry.captured_groove_radius_m.is_some()
        || telemetry.groove_contact != (telemetry.contact_region == RadialContactRegion::Groove)
        || telemetry.land_contact && telemetry.contact_region != RadialContactRegion::Land
        || telemetry.captured_turn_index.unsigned_abs() > MAX_ABSOLUTE_TURN_INDEX as u64
        || telemetry.groove_pitch_m_per_revolution < MIN_GROOVE_PITCH_M_PER_REVOLUTION
        || telemetry.groove_pitch_m_per_revolution > MAX_GROOVE_PITCH_M_PER_REVOLUTION
    {
        return Err(RadialTrackingError::NumericalFailure);
    }
    Ok(())
}

fn initial_telemetry(radius_m: f64, pitch_m: f64) -> RadialTrackingTelemetry {
    RadialTrackingTelemetry {
        spiral_reference_radius_m: radius_m,
        stylus_radius_m: radius_m,
        radial_velocity_m_s: 0.0,
        groove_pitch_m_per_revolution: pitch_m,
        captured_groove_radius_m: Some(radius_m),
        radial_error_to_retained_turn_m: 0.0,
        groove_radial_velocity_m_s: 0.0,
        guide_force_n: 0.0,
        bearing_friction_force_n: 0.0,
        applied_radial_force_n: 0.0,
        groove_contact: true,
        land_contact: false,
        contact_region: RadialContactRegion::Groove,
        contact_lost_this_step: false,
        recaptured_this_step: false,
        recapture_limit_reached: false,
        captured_turn_index: 0,
        turns_skipped_this_step: 0,
        total_turns_skipped: 0,
        stylus_lowered: true,
        macro_contact_available: false,
        completed_steps: 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum RadialTrackingError {
    #[error("radial tracking configuration field {field} is invalid")]
    InvalidConfig { field: &'static str },
    #[error("radial tracking sample rate is outside the supported range")]
    InvalidSampleRate,
    #[error("initial groove radius is outside the supported range")]
    InvalidInitialRadius,
    #[error("groove pitch is outside the supported range")]
    InvalidGroovePitch,
    #[error("groove top width is outside the supported range")]
    InvalidGrooveTopWidth,
    #[error("radial tracking input contains an invalid value")]
    InvalidInput,
    #[error("radial tracking observation contains an invalid value")]
    InvalidObservation,
    #[error("radial tracking selection does not match the current sample")]
    StaleSelection,
    #[error("radial tracking midpoint preparation does not match the current state")]
    StaleMidpointPreparation,
    #[error("radial tracking snapshot is invalid")]
    InvalidSnapshot,
    #[error("radial tracking produced an invalid value")]
    NumericalFailure,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE_HZ: f64 = 192_000.0;
    const INITIAL_RADIUS_M: f64 = 0.146_05;
    const PITCH_M: f64 = 125.0e-6;

    fn state() -> RadialTrackingState {
        RadialTrackingState::new(
            RadialTrackingConfig::default(),
            SAMPLE_RATE_HZ,
            INITIAL_RADIUS_M,
            PITCH_M,
        )
        .unwrap()
    }

    fn input() -> RadialTrackingInput {
        RadialTrackingInput {
            minimum_available_turn_index: -100,
            maximum_available_turn_index: 100,
            ..RadialTrackingInput::default()
        }
    }

    fn observe(
        state: &mut RadialTrackingState,
        selection: RadialTrackingSelection,
        lateral_m: f64,
    ) -> RadialTrackingTelemetry {
        state
            .observe_pickup(
                selection,
                RadialTrackingObservation {
                    stylus_lateral_position_m: lateral_m,
                    physical_surface_contact: true,
                    land_contact: selection.contact_region == RadialContactRegion::Land,
                    ..RadialTrackingObservation::default()
                },
            )
            .unwrap()
    }

    #[test]
    fn inward_origin_motion_moves_a_stationary_stylus_outward_in_local_coordinates() {
        let mut state = state();
        let turn_delta = 33.0 / 60.0 / SAMPLE_RATE_HZ;
        let selection = state
            .process(RadialTrackingInput {
                record_angle_delta_turns: turn_delta,
                ..input()
            })
            .unwrap();
        assert!(selection.coordinate_origin_shift_m < 0.0);
        let rebased_stylus_m = -selection.coordinate_origin_shift_m;
        assert!(rebased_stylus_m > 0.0);
        let telemetry = observe(&mut state, selection, rebased_stylus_m);
        assert!((telemetry.stylus_radius_m - INITIAL_RADIUS_M).abs() < 1.0e-15);
    }

    #[test]
    fn normal_following_stays_on_turn_zero_forward_and_reverse() {
        let mut state = state();
        let turn_delta = 20.0 * 33.0 / 60.0 / SAMPLE_RATE_HZ;
        for direction in [1.0, -1.0] {
            for _ in 0..20_000 {
                let selection = state
                    .process(RadialTrackingInput {
                        record_angle_delta_turns: direction * turn_delta,
                        stylus_lateral_position_m: 0.0,
                        ..input()
                    })
                    .unwrap();
                assert_eq!(selection.selected_turn_index, Some(0));
                observe(&mut state, selection, 0.0);
            }
        }
        assert_eq!(state.telemetry().captured_turn_index, 0);
        assert_eq!(state.telemetry().total_turns_skipped, 0);
    }

    #[test]
    fn release_enters_land_without_selecting_programme_audio() {
        let mut state = state();
        let release_half = 25.0e-6 + state.config.contact_release_margin_m;
        let selection = state
            .process(RadialTrackingInput {
                stylus_lateral_position_m: release_half + 1.0e-6,
                ..input()
            })
            .unwrap();
        assert_eq!(selection.contact_region, RadialContactRegion::Land);
        assert_eq!(selection.selected_turn_index, None);
        assert_eq!(selection.source_turn_offset(), None);
        let telemetry = observe(&mut state, selection, release_half + 1.0e-6);
        assert!(telemetry.contact_lost_this_step);
        assert!(!telemetry.groove_contact);
        assert!(telemetry.land_contact);
        assert!(telemetry.captured_groove_radius_m.is_none());
    }

    #[test]
    fn outward_and_inward_apertures_recapture_adjacent_turns() {
        for expected_turn in [1_i64, -1_i64] {
            let mut state = state();
            let release_position = expected_turn as f64 * PITCH_M * 0.55;
            let release = state
                .process(RadialTrackingInput {
                    stylus_lateral_position_m: release_position,
                    ..input()
                })
                .unwrap();
            assert_eq!(release.contact_region, RadialContactRegion::Land);
            observe(&mut state, release, release_position);

            let capture_position = expected_turn as f64 * PITCH_M;
            let capture = state
                .process(RadialTrackingInput {
                    stylus_lateral_position_m: capture_position,
                    ..input()
                })
                .unwrap();
            assert_eq!(capture.selected_turn_index, Some(expected_turn));
            let telemetry = observe(&mut state, capture, capture_position);
            assert!(telemetry.recaptured_this_step);
            assert_eq!(telemetry.turns_skipped_this_step, expected_turn);
            assert_eq!(telemetry.total_turns_skipped, expected_turn);
        }
    }

    #[test]
    fn recapture_requires_the_target_aperture_and_available_source_turn() {
        let mut state = state();
        let release = state
            .process(RadialTrackingInput {
                stylus_lateral_position_m: 0.55 * PITCH_M,
                ..input()
            })
            .unwrap();
        observe(&mut state, release, 0.55 * PITCH_M);

        let between = state
            .process(RadialTrackingInput {
                stylus_lateral_position_m: 0.5 * PITCH_M,
                ..input()
            })
            .unwrap();
        assert_eq!(between.contact_region, RadialContactRegion::Land);
        observe(&mut state, between, 0.5 * PITCH_M);

        let unavailable = state
            .process(RadialTrackingInput {
                stylus_lateral_position_m: PITCH_M,
                minimum_available_turn_index: -100,
                maximum_available_turn_index: 0,
                ..input()
            })
            .unwrap();
        assert_eq!(unavailable.contact_region, RadialContactRegion::Land);
        assert_eq!(unavailable.selected_turn_index, None);
    }

    #[test]
    fn force_observation_has_no_second_dynamics_or_force_application() {
        let mut state = state();
        let selection = state.process(input()).unwrap();
        let telemetry = state
            .observe_pickup(
                selection,
                RadialTrackingObservation {
                    stylus_lateral_position_m: 2.0e-6,
                    stylus_radial_velocity_m_s: 0.03,
                    groove_lateral_force_on_tip_n: 0.004,
                    bearing_friction_force_n: -0.001,
                    anti_skate_force_n: 0.002,
                    skating_force_n: -0.003,
                    physical_surface_contact: true,
                    land_contact: false,
                },
            )
            .unwrap();
        assert_eq!(telemetry.guide_force_n, 0.004);
        assert_eq!(telemetry.applied_radial_force_n, -0.002);
        assert_eq!(telemetry.radial_velocity_m_s, 0.03);
    }

    #[test]
    fn snapshot_restore_continues_bit_identically() {
        let mut a = state();
        for step in 0..2_000 {
            let selection = a
                .process(RadialTrackingInput {
                    record_angle_delta_turns: if step % 173 < 90 {
                        20.0 * 33.0 / 60.0 / SAMPLE_RATE_HZ
                    } else {
                        -20.0 * 33.0 / 60.0 / SAMPLE_RATE_HZ
                    },
                    stylus_lateral_position_m: 0.0,
                    ..input()
                })
                .unwrap();
            observe(&mut a, selection, 0.0);
        }
        let json = serde_json::to_string(&a.snapshot()).unwrap();
        let snapshot: RadialTrackingSnapshot = serde_json::from_str(&json).unwrap();
        let mut b = state();
        b.restore(snapshot).unwrap();
        let next = RadialTrackingInput {
            record_angle_delta_turns: -20.0 * 33.0 / 60.0 / SAMPLE_RATE_HZ,
            ..input()
        };
        let a_selection = a.process(next).unwrap();
        let b_selection = b.process(next).unwrap();
        assert_eq!(a_selection, b_selection);
        assert_eq!(
            observe(&mut a, a_selection, 0.0),
            observe(&mut b, b_selection, 0.0)
        );
        assert_eq!(a.snapshot(), b.snapshot());
    }

    #[test]
    fn rapid_twenty_x_reversals_remain_finite_and_bounded() {
        let mut state = state();
        for step in 0..20_000 {
            let direction = if step % 311 < 155 { 20.0 } else { -20.0 };
            let selection = state
                .process(RadialTrackingInput {
                    record_angle_delta_turns: direction * 33.0 / 60.0 / SAMPLE_RATE_HZ,
                    stylus_lateral_position_m: 0.0,
                    ..input()
                })
                .unwrap();
            let telemetry = observe(&mut state, selection, 0.0);
            assert!(telemetry.stylus_radius_m.is_finite());
            assert_eq!(telemetry.captured_turn_index, 0);
        }
    }

    #[test]
    fn midpoint_commit_matches_the_direct_step_when_the_advance_matches() {
        let step_input = RadialTrackingInput {
            record_angle_delta_turns: 20.0 * 33.0 / 60.0 / SAMPLE_RATE_HZ,
            ..input()
        };
        let mut direct = state();
        let direct_selection = direct.process(step_input).unwrap();

        let mut midpoint = state();
        let preparation = midpoint.prepare_midpoint(step_input).unwrap();
        let midpoint_selection = midpoint
            .commit_midpoint(preparation, step_input.record_angle_delta_turns)
            .unwrap();

        assert_eq!(midpoint_selection, direct_selection);
        assert_eq!(midpoint, direct);
    }

    #[test]
    fn midpoint_commit_keeps_the_prepared_branch_for_a_different_advance() {
        let midpoint_input = RadialTrackingInput {
            record_angle_delta_turns: 0.24,
            ..input()
        };
        let mut tracker = state();
        let preparation = tracker.prepare_midpoint(midpoint_input).unwrap();
        assert_eq!(
            preparation.selection().contact_region,
            RadialContactRegion::Land
        );

        let selection = tracker.commit_midpoint(preparation, 0.0).unwrap();
        assert_eq!(selection.coordinate_origin_shift_m, 0.0);
        assert_eq!(selection.contact_region, RadialContactRegion::Land);
        assert_eq!(
            tracker.telemetry().contact_region,
            RadialContactRegion::Land
        );

        let mut direct = state();
        assert_eq!(
            direct.process(input()).unwrap().contact_region,
            RadialContactRegion::Groove
        );
    }

    #[test]
    fn stale_midpoint_commit_preserves_the_newer_state() {
        let mut tracker = state();
        let preparation = tracker.prepare_midpoint(input()).unwrap();
        tracker
            .process(RadialTrackingInput {
                record_angle_delta_turns: 1.0e-4,
                ..input()
            })
            .unwrap();
        let before = tracker;

        assert_eq!(
            tracker.commit_midpoint(preparation, 0.0),
            Err(RadialTrackingError::StaleMidpointPreparation)
        );
        assert_eq!(tracker, before);
    }

    #[test]
    fn invalid_input_and_snapshot_preserve_state() {
        let mut state = state();
        let before = state;
        assert_eq!(
            state.process(RadialTrackingInput {
                record_angle_delta_turns: f64::NAN,
                ..input()
            }),
            Err(RadialTrackingError::InvalidInput)
        );
        assert_eq!(state, before);

        let mut snapshot = state.snapshot();
        snapshot.spiral_reference_radius_m = f64::INFINITY;
        assert_eq!(
            state.restore(snapshot),
            Err(RadialTrackingError::InvalidSnapshot)
        );
        assert_eq!(state, before);
    }
}
