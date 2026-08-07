use serde::{Deserialize, Serialize};
use thiserror::Error;

const SNAPSHOT_VERSION: u32 = 2;
const MIN_INERTIA_KG_M2: f64 = 1.0e-7;
const MIN_INTEGRATION_HZ: f64 = 1_000.0;
const MAX_INTEGRATION_HZ: f64 = 768_000.0;
const MAX_ADVANCE_SECONDS: f64 = 10.0;
const REST_ANGULAR_VELOCITY_RAD_S: f64 = 1.0e-6;
const REST_TORQUE_NM: f64 = 1.0e-9;
const STATIC_ENTRY_RATIO: f64 = 0.98;
const MAX_EXPLICIT_STEP_RATIO: f64 = 0.5;

/// These limits protect the solver from invalid host controls.
/// They are engineering limits, not measured hardware properties.
pub const MAXIMUM_DECK_RATE: f64 = 20.0;
pub const MAXIMUM_HAND_NORMAL_FORCE_N: f64 = 100.0;
/// The moving hand's normal force splits into a fingertip square law that
/// keeps light pressure slipping, and a cube term that brings the palm's
/// weight in toward full grip. At full grip the hand bears 40 N — a firm
/// palm planted on the record — so a live catch of a playing record
/// reverses inside a hand's-breadth of samples instead of a perceptible
/// dead gap; at 5 N flat it took ~67 ms.
const FINGERTIP_GRIP_FORCE_N: f64 = 5.0;
const PALM_GRIP_FORCE_N: f64 = 35.0;
/// Extra normal force from a planted full-grip press, on top of the
/// square-law fingertip force.
const STATIONARY_PRESS_FORCE_N: f64 = 15.0;
/// Hand speed (in units of nominal rate) at which the planted-press
/// force boost has fully faded back to the square law.
const STATIONARY_PRESS_FADE_RATE: f64 = 0.25;
/// Fraction of the hand's angular speed available as extra position
/// catch-up authority beyond the configured floor.
const HAND_CATCHUP_RATE_SHARE: f64 = 0.25;
pub const MAXIMUM_HAND_CONTACT_RADIUS_M: f64 = 0.20;
pub const MAXIMUM_STYLUS_TORQUE_NM: f64 = 1.0;
const MAXIMUM_DECK_SNAPSHOT_RATE: f64 = 40.0;
const MAXIMUM_DECK_SNAPSHOT_CONTACT_TORQUE_NM: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MotorMode {
    Off,
    Servo,
    Brake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContactMode {
    Separated,
    Sticking,
    SlidingPositive,
    SlidingNegative,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalDeckConfig {
    pub nominal_rpm: f64,
    pub platter_inertia_kg_m2: f64,
    pub record_inertia_kg_m2: f64,
    pub motor_starting_torque_nm: f64,
    pub motor_servo_kp_nm_per_rad_s: f64,
    pub motor_servo_ki_nm_per_rad: f64,
    pub motor_integral_limit_nm: f64,
    pub motor_brake_torque_nm: f64,
    pub motor_brake_gain_nm_per_rad_s: f64,
    pub bearing_static_torque_nm: f64,
    pub bearing_kinetic_torque_nm: f64,
    pub bearing_viscous_torque_nm_per_rad_s: f64,
    pub slipmat_static_torque_nm: f64,
    pub slipmat_kinetic_torque_nm: f64,
    pub slipmat_viscous_torque_nm_per_rad_s: f64,
    pub hand_static_friction_coefficient: f64,
    pub hand_kinetic_friction_coefficient: f64,
    pub hand_viscous_torque_nm_per_rad_s: f64,
    pub hand_position_stabilization_seconds: f64,
    pub hand_max_position_correction_rad_s: f64,
    pub integration_hz: f64,
}

impl PhysicalDeckConfig {
    /// This seed combines published deck values with explicit estimates.
    /// It is not a calibrated hardware profile.
    pub fn sl_1200mk7_seed() -> Self {
        Self {
            nominal_rpm: 33.333_333_333_333_336,
            // Technics publishes 1.8 kg and 332 mm for the platter assembly.
            // This estimate treats the assembly as a uniform disc.
            platter_inertia_kg_m2: 0.5 * 1.8 * 0.166 * 0.166,
            // This estimate treats a 180 g, 300 mm record as a uniform disc.
            record_inertia_kg_m2: 0.5 * 0.180 * 0.150 * 0.150,
            // Technics publishes this value as starting torque.
            motor_starting_torque_nm: 0.18,
            // The servo values target the published 0.7 second startup time.
            motor_servo_kp_nm_per_rad_s: 0.50,
            motor_servo_ki_nm_per_rad: 12.0,
            motor_integral_limit_nm: 0.08,
            motor_brake_torque_nm: 0.12,
            motor_brake_gain_nm_per_rad_s: 0.50,
            bearing_static_torque_nm: 0.000_24,
            bearing_kinetic_torque_nm: 0.000_18,
            bearing_viscous_torque_nm_per_rad_s: 0.000_12,
            // These values estimate a felt DJ slipmat.
            slipmat_static_torque_nm: 0.032,
            slipmat_kinetic_torque_nm: 0.022,
            slipmat_viscous_torque_nm_per_rad_s: 0.002,
            // These values estimate dry finger contact on a record.
            hand_static_friction_coefficient: 0.75,
            hand_kinetic_friction_coefficient: 0.55,
            hand_viscous_torque_nm_per_rad_s: 0.002,
            hand_position_stabilization_seconds: 0.004,
            hand_max_position_correction_rad_s: 25.0,
            integration_hz: 192_000.0,
        }
    }

    /// This seed represents a current high-torque DJ turntable.
    /// Published values come from the Reloop RP-8000 MK2 specification.
    /// Friction and servo values remain engineering estimates.
    pub fn high_torque_dj_seed() -> Self {
        let mut config = Self::sl_1200mk7_seed();
        // Reloop publishes a 1.5 kg platter and a 332 mm diameter.
        config.platter_inertia_kg_m2 = 0.5 * 1.5 * 0.166 * 0.166;
        // Reloop publishes a maximum starting torque of 4.5 kg/cm.
        config.motor_starting_torque_nm = 4.5 * 0.098_066_5;
        // These values target the published startup time below 0.2 seconds.
        config.motor_servo_kp_nm_per_rad_s = 0.80;
        config.motor_servo_ki_nm_per_rad = 20.0;
        config.motor_integral_limit_nm = 0.12;
        config.motor_brake_torque_nm = 0.20;
        config.motor_brake_gain_nm_per_rad_s = 0.80;
        // These estimates keep the record coupled during motor startup.
        // They also give a short, continuous take-up after hand release.
        config.slipmat_static_torque_nm = 0.075;
        config.slipmat_kinetic_torque_nm = 0.060;
        config
    }

    pub fn validate(self) -> Result<Self, PhysicalDeckConfigError> {
        validate_positive("nominalRpm", self.nominal_rpm)?;
        validate_minimum(
            "platterInertiaKgM2",
            self.platter_inertia_kg_m2,
            MIN_INERTIA_KG_M2,
        )?;
        validate_minimum(
            "recordInertiaKgM2",
            self.record_inertia_kg_m2,
            MIN_INERTIA_KG_M2,
        )?;
        validate_nonnegative("motorStartingTorqueNm", self.motor_starting_torque_nm)?;
        validate_nonnegative("motorServoKpNmPerRadS", self.motor_servo_kp_nm_per_rad_s)?;
        validate_nonnegative("motorServoKiNmPerRad", self.motor_servo_ki_nm_per_rad)?;
        validate_nonnegative("motorIntegralLimitNm", self.motor_integral_limit_nm)?;
        validate_nonnegative("motorBrakeTorqueNm", self.motor_brake_torque_nm)?;
        validate_nonnegative(
            "motorBrakeGainNmPerRadS",
            self.motor_brake_gain_nm_per_rad_s,
        )?;
        validate_nonnegative("bearingStaticTorqueNm", self.bearing_static_torque_nm)?;
        validate_nonnegative("bearingKineticTorqueNm", self.bearing_kinetic_torque_nm)?;
        validate_nonnegative(
            "bearingViscousTorqueNmPerRadS",
            self.bearing_viscous_torque_nm_per_rad_s,
        )?;
        validate_nonnegative("slipmatStaticTorqueNm", self.slipmat_static_torque_nm)?;
        validate_nonnegative("slipmatKineticTorqueNm", self.slipmat_kinetic_torque_nm)?;
        validate_nonnegative(
            "slipmatViscousTorqueNmPerRadS",
            self.slipmat_viscous_torque_nm_per_rad_s,
        )?;
        validate_nonnegative(
            "handStaticFrictionCoefficient",
            self.hand_static_friction_coefficient,
        )?;
        validate_nonnegative(
            "handKineticFrictionCoefficient",
            self.hand_kinetic_friction_coefficient,
        )?;
        validate_nonnegative(
            "handViscousTorqueNmPerRadS",
            self.hand_viscous_torque_nm_per_rad_s,
        )?;
        validate_positive(
            "handPositionStabilizationSeconds",
            self.hand_position_stabilization_seconds,
        )?;
        validate_nonnegative(
            "handMaxPositionCorrectionRadS",
            self.hand_max_position_correction_rad_s,
        )?;
        validate_minimum("integrationHz", self.integration_hz, MIN_INTEGRATION_HZ)?;
        if self.integration_hz > MAX_INTEGRATION_HZ {
            return Err(PhysicalDeckConfigError::AboveMaximum {
                field: "integrationHz",
                maximum: MAX_INTEGRATION_HZ,
            });
        }
        if self.bearing_kinetic_torque_nm > self.bearing_static_torque_nm {
            return Err(PhysicalDeckConfigError::KineticExceedsStatic { contact: "bearing" });
        }
        if self.slipmat_kinetic_torque_nm > self.slipmat_static_torque_nm {
            return Err(PhysicalDeckConfigError::KineticExceedsStatic { contact: "slipmat" });
        }
        if self.hand_kinetic_friction_coefficient > self.hand_static_friction_coefficient {
            return Err(PhysicalDeckConfigError::KineticExceedsStatic { contact: "hand" });
        }

        let dt = 1.0 / self.integration_hz;
        validate_step_ratio(
            "motorServoKpNmPerRadS",
            self.motor_servo_kp_nm_per_rad_s * dt / self.platter_inertia_kg_m2,
        )?;
        validate_step_ratio(
            "motorServoKiNmPerRad",
            self.motor_servo_ki_nm_per_rad * dt * dt / self.platter_inertia_kg_m2,
        )?;
        validate_step_ratio(
            "motorBrakeGainNmPerRadS",
            self.motor_brake_gain_nm_per_rad_s * dt / self.platter_inertia_kg_m2,
        )?;
        validate_step_ratio(
            "bearingViscousTorqueNmPerRadS",
            self.bearing_viscous_torque_nm_per_rad_s * dt / self.platter_inertia_kg_m2,
        )?;
        validate_step_ratio(
            "slipmatViscousTorqueNmPerRadS",
            self.slipmat_viscous_torque_nm_per_rad_s
                * dt
                * (1.0 / self.platter_inertia_kg_m2 + 1.0 / self.record_inertia_kg_m2),
        )?;
        Ok(self)
    }

    pub fn nominal_angular_velocity_rad_s(self) -> f64 {
        self.nominal_rpm * std::f64::consts::TAU / 60.0
    }
}

impl Default for PhysicalDeckConfig {
    fn default() -> Self {
        Self::sl_1200mk7_seed()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckMechanicalControl {
    pub motor_mode: MotorMode,
    pub motor_target_angular_velocity_rad_s: f64,
    pub hand_contact: bool,
    pub hand_target_angle_rad: Option<f64>,
    pub hand_target_angular_velocity_rad_s: f64,
    pub hand_normal_force_n: f64,
    pub hand_contact_radius_m: f64,
    /// The pickup solver supplies signed torque on the record.
    pub stylus_torque_nm: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedDeckControl {
    pub motor_mode: MotorMode,
    pub motor_rate: f64,
    pub hand_contact: bool,
    pub hand_target_angle_turns: Option<f64>,
    pub hand_rate: f64,
    pub grip: f64,
    pub stylus_torque_nm: f64,
}

impl DeckMechanicalControl {
    pub fn from_normalized(config: PhysicalDeckConfig, input: NormalizedDeckControl) -> Self {
        let nominal = config.nominal_angular_velocity_rad_s();
        let normalized_grip = finite_or_zero(input.grip).clamp(0.0, 1.0);
        Self {
            motor_mode: input.motor_mode,
            motor_target_angular_velocity_rad_s: finite_or_zero(input.motor_rate)
                .clamp(-MAXIMUM_DECK_RATE, MAXIMUM_DECK_RATE)
                * nominal,
            hand_contact: input.hand_contact,
            hand_target_angle_rad: input
                .hand_target_angle_turns
                .filter(|value| value.is_finite())
                .map(|turns| turns * std::f64::consts::TAU),
            hand_target_angular_velocity_rad_s: finite_or_zero(input.hand_rate)
                .clamp(-MAXIMUM_DECK_RATE, MAXIMUM_DECK_RATE)
                * nominal,
            // Touch pressure has little useful resolution near zero.
            // The square law preserves light slip and firm record ownership.
            // A planted press adds the steep top term: a full-grip hand that
            // is not moving clamps the record so a touch-stop reads as
            // immediate. The boost fades out with hand speed, leaving active
            // scratching under the original square law.
            hand_normal_force_n: {
                let stationary = (1.0
                    - finite_or_zero(input.hand_rate).abs() / STATIONARY_PRESS_FADE_RATE)
                    .clamp(0.0, 1.0);
                normalized_grip * normalized_grip * FINGERTIP_GRIP_FORCE_N
                    + normalized_grip.powi(3) * PALM_GRIP_FORCE_N
                    + normalized_grip.powi(8) * STATIONARY_PRESS_FORCE_N * stationary
            },
            hand_contact_radius_m: 0.12,
            stylus_torque_nm: finite_or_zero(input.stylus_torque_nm)
                .clamp(-MAXIMUM_STYLUS_TORQUE_NM, MAXIMUM_STYLUS_TORQUE_NM),
        }
    }

    fn validate(self) -> Result<Self, DeckMechanicalError> {
        validate_control_finite(
            "motorTargetAngularVelocityRadS",
            self.motor_target_angular_velocity_rad_s,
        )?;
        if self
            .hand_target_angle_rad
            .is_some_and(|value| !value.is_finite())
        {
            return Err(DeckMechanicalError::InvalidControl {
                field: "handTargetAngleRad",
            });
        }
        validate_control_finite(
            "handTargetAngularVelocityRadS",
            self.hand_target_angular_velocity_rad_s,
        )?;
        validate_control_nonnegative("handNormalForceN", self.hand_normal_force_n)?;
        validate_control_nonnegative("handContactRadiusM", self.hand_contact_radius_m)?;
        validate_control_finite("stylusTorqueNm", self.stylus_torque_nm)?;
        Ok(self)
    }

    pub fn validate_for_config(
        self,
        config: PhysicalDeckConfig,
    ) -> Result<Self, DeckMechanicalError> {
        self.validate()?;
        let maximum_angular_velocity = MAXIMUM_DECK_RATE * config.nominal_angular_velocity_rad_s();
        if self.motor_target_angular_velocity_rad_s.abs() > maximum_angular_velocity {
            return Err(DeckMechanicalError::InvalidControl {
                field: "motorTargetAngularVelocityRadS",
            });
        }
        if self.hand_target_angular_velocity_rad_s.abs() > maximum_angular_velocity {
            return Err(DeckMechanicalError::InvalidControl {
                field: "handTargetAngularVelocityRadS",
            });
        }
        if self.hand_normal_force_n > MAXIMUM_HAND_NORMAL_FORCE_N {
            return Err(DeckMechanicalError::InvalidControl {
                field: "handNormalForceN",
            });
        }
        if self.hand_contact_radius_m > MAXIMUM_HAND_CONTACT_RADIUS_M {
            return Err(DeckMechanicalError::InvalidControl {
                field: "handContactRadiusM",
            });
        }
        if self.stylus_torque_nm.abs() > MAXIMUM_STYLUS_TORQUE_NM {
            return Err(DeckMechanicalError::InvalidControl {
                field: "stylusTorqueNm",
            });
        }
        Ok(self)
    }
}

impl Default for DeckMechanicalControl {
    fn default() -> Self {
        Self {
            motor_mode: MotorMode::Off,
            motor_target_angular_velocity_rad_s: 0.0,
            hand_contact: false,
            hand_target_angle_rad: None,
            hand_target_angular_velocity_rad_s: 0.0,
            hand_normal_force_n: 0.0,
            hand_contact_radius_m: 0.12,
            stylus_torque_nm: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckMechanicalTelemetry {
    pub mechanical_time_seconds: f64,
    pub platter_rate: f64,
    pub record_rate: f64,
    pub platter_angle_turns: f64,
    pub record_angle_turns: f64,
    pub motor_torque_nm: f64,
    pub slipmat_torque_nm: f64,
    pub hand_torque_nm: f64,
    pub bearing_torque_nm: f64,
    pub stylus_torque_nm: f64,
    pub slipmat_mode: ContactMode,
    pub hand_mode: ContactMode,
    pub bearing_sticking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckMechanicalState {
    config: PhysicalDeckConfig,
    platter_angle_rad: f64,
    record_angle_rad: f64,
    platter_angular_velocity_rad_s: f64,
    record_angular_velocity_rad_s: f64,
    motor_integral_torque_nm: f64,
    integration_remainder_seconds: f64,
    completed_steps: u64,
    slipmat_mode: ContactMode,
    hand_mode: ContactMode,
    last_control: DeckMechanicalControl,
    last_motor_torque_nm: f64,
    last_slipmat_torque_nm: f64,
    last_hand_torque_nm: f64,
    last_bearing_torque_nm: f64,
    last_stylus_torque_nm: f64,
    bearing_sticking: bool,
    last_telemetry: DeckMechanicalTelemetry,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckMechanicalSnapshot {
    version: u32,
    config: PhysicalDeckConfig,
    platter_angle_rad: f64,
    record_angle_rad: f64,
    platter_angular_velocity_rad_s: f64,
    record_angular_velocity_rad_s: f64,
    motor_integral_torque_nm: f64,
    integration_remainder_seconds: f64,
    completed_steps: u64,
    slipmat_mode: ContactMode,
    hand_mode: ContactMode,
    last_control: DeckMechanicalControl,
    last_motor_torque_nm: f64,
    last_slipmat_torque_nm: f64,
    last_hand_torque_nm: f64,
    last_bearing_torque_nm: f64,
    last_stylus_torque_nm: f64,
    bearing_sticking: bool,
}

impl DeckMechanicalState {
    pub fn new(config: PhysicalDeckConfig) -> Result<Self, PhysicalDeckConfigError> {
        let config = config.validate()?;
        let mut state = Self {
            config,
            platter_angle_rad: 0.0,
            record_angle_rad: 0.0,
            platter_angular_velocity_rad_s: 0.0,
            record_angular_velocity_rad_s: 0.0,
            motor_integral_torque_nm: 0.0,
            integration_remainder_seconds: 0.0,
            completed_steps: 0,
            slipmat_mode: ContactMode::Sticking,
            hand_mode: ContactMode::Separated,
            last_control: DeckMechanicalControl::default(),
            last_motor_torque_nm: 0.0,
            last_slipmat_torque_nm: 0.0,
            last_hand_torque_nm: 0.0,
            last_bearing_torque_nm: 0.0,
            last_stylus_torque_nm: 0.0,
            bearing_sticking: true,
            last_telemetry: zero_telemetry(),
        };
        state.publish_telemetry();
        Ok(state)
    }

    pub fn config(&self) -> PhysicalDeckConfig {
        self.config
    }

    pub fn reconfigure(
        &mut self,
        config: PhysicalDeckConfig,
    ) -> Result<(), PhysicalDeckConfigError> {
        let config = config.validate()?;
        self.config = config;
        self.motor_integral_torque_nm = self.motor_integral_torque_nm.clamp(
            -config.motor_integral_limit_nm,
            config.motor_integral_limit_nm,
        );
        self.integration_remainder_seconds %= 1.0 / config.integration_hz;
        self.publish_telemetry();
        Ok(())
    }

    pub fn reset(
        &mut self,
        platter_rate: f64,
        record_rate: f64,
        platter_angle_turns: f64,
        record_angle_turns: f64,
    ) -> Result<(), DeckMechanicalError> {
        for (field, value) in [
            ("platterRate", platter_rate),
            ("recordRate", record_rate),
            ("platterAngleTurns", platter_angle_turns),
            ("recordAngleTurns", record_angle_turns),
        ] {
            validate_control_finite(field, value)?;
        }
        if platter_rate.abs() > MAXIMUM_DECK_RATE || record_rate.abs() > MAXIMUM_DECK_RATE {
            return Err(DeckMechanicalError::InvalidControl { field: "resetRate" });
        }
        let nominal = self.config.nominal_angular_velocity_rad_s();
        self.platter_angular_velocity_rad_s = platter_rate * nominal;
        self.record_angular_velocity_rad_s = record_rate * nominal;
        self.platter_angle_rad = platter_angle_turns * std::f64::consts::TAU;
        self.record_angle_rad = record_angle_turns * std::f64::consts::TAU;
        self.motor_integral_torque_nm = 0.0;
        self.integration_remainder_seconds = 0.0;
        self.completed_steps = 0;
        self.slipmat_mode = ContactMode::Sticking;
        self.hand_mode = ContactMode::Separated;
        self.clear_last_torques();
        self.publish_telemetry();
        Ok(())
    }

    /// Prepares one integration step for the joint midpoint contact solver.
    pub(crate) fn prepare_midpoint_step(
        &self,
        duration_seconds: f64,
        mut control: DeckMechanicalControl,
    ) -> Result<DeckMidpointPreparation, DeckMechanicalError> {
        if !duration_seconds.is_finite() || duration_seconds < 0.0 {
            return Err(DeckMechanicalError::InvalidDuration);
        }
        control.stylus_torque_nm = 0.0;
        let control = control.validate_for_config(self.config)?;
        let mut next = *self;
        next.last_control = control;
        next.integration_remainder_seconds += duration_seconds;
        let integration_dt = 1.0 / next.config.integration_hz;
        if duration_seconds == 0.0
            || duration_seconds > integration_dt * (1.0 + 16.0 * f64::EPSILON)
            || next.integration_remainder_seconds > integration_dt * (1.0 + 16.0 * f64::EPSILON)
        {
            return Err(DeckMechanicalError::MidpointStepRequiresOneIntegrationStep);
        }
        let completes_integration_step =
            next.integration_remainder_seconds >= integration_dt * (1.0 - 16.0 * f64::EPSILON);
        if completes_integration_step {
            next.integration_remainder_seconds -= integration_dt;
        }
        if next.integration_remainder_seconds.abs() <= 16.0 * f64::EPSILON * integration_dt {
            next.integration_remainder_seconds = 0.0;
        }
        let dt = duration_seconds;

        let config = next.config;
        let motor_torque_nm = next.motor_torque(control, dt);
        let hand_active = control.hand_contact
            && control.hand_normal_force_n > 0.0
            && control.hand_contact_radius_m > 0.0;
        let hand_velocity_rad_s = effective_hand_velocity(control, config, next.record_angle_rad);
        let bearing_static_limit_nm = if next.bearing_sticking {
            config.bearing_static_torque_nm
        } else {
            config.bearing_static_torque_nm * STATIC_ENTRY_RATIO
        };
        let predicted_bearing_torque_nm = if next.platter_angular_velocity_rad_s
            > REST_ANGULAR_VELOCITY_RAD_S
        {
            -config.bearing_kinetic_torque_nm
                - config.bearing_viscous_torque_nm_per_rad_s * next.platter_angular_velocity_rad_s
        } else if next.platter_angular_velocity_rad_s < -REST_ANGULAR_VELOCITY_RAD_S {
            config.bearing_kinetic_torque_nm
                - config.bearing_viscous_torque_nm_per_rad_s * next.platter_angular_velocity_rad_s
        } else {
            0.0
        };
        let hand_relative_velocity_rad_s = hand_velocity_rad_s - next.record_angular_velocity_rad_s;
        let predicted_hand_torque_nm = if hand_active {
            let kinetic_torque_nm = config.hand_kinetic_friction_coefficient
                * control.hand_normal_force_n
                * control.hand_contact_radius_m;
            let kinetic_torque_nm = if hand_relative_velocity_rad_s > REST_ANGULAR_VELOCITY_RAD_S {
                kinetic_torque_nm
            } else if hand_relative_velocity_rad_s < -REST_ANGULAR_VELOCITY_RAD_S {
                -kinetic_torque_nm
            } else {
                0.0
            };
            kinetic_torque_nm
                + config.hand_viscous_torque_nm_per_rad_s * hand_relative_velocity_rad_s
        } else {
            0.0
        };
        let predicted_platter_velocity_rad_s = next.platter_angular_velocity_rad_s
            + dt * (motor_torque_nm + predicted_bearing_torque_nm) / config.platter_inertia_kg_m2;
        let predicted_record_velocity_rad_s = next.record_angular_velocity_rad_s
            + dt * (predicted_hand_torque_nm + next.last_stylus_torque_nm)
                / config.record_inertia_kg_m2;
        let predicted_slipmat_relative_velocity_rad_s = if hand_active {
            predicted_platter_velocity_rad_s - predicted_record_velocity_rad_s
        } else {
            next.platter_angular_velocity_rad_s - next.record_angular_velocity_rad_s
        };
        let slipmat_static_limit_nm =
            entry_limit(config.slipmat_static_torque_nm, next.slipmat_mode);
        let slipmat_required_static_torque_nm = predicted_slipmat_relative_velocity_rad_s
            / (dt * (config.platter_inertia_kg_m2.recip() + config.record_inertia_kg_m2.recip()));
        let slipmat_static_is_predicted =
            slipmat_required_static_torque_nm.abs() <= slipmat_static_limit_nm + REST_TORQUE_NM;
        let bearing_modes = if next.bearing_sticking {
            [
                FrictionMode::Stick,
                FrictionMode::SlidingPositive,
                FrictionMode::SlidingNegative,
            ]
        } else if next.platter_angular_velocity_rad_s >= 0.0 {
            [
                FrictionMode::SlidingPositive,
                FrictionMode::Stick,
                FrictionMode::SlidingNegative,
            ]
        } else {
            [
                FrictionMode::SlidingNegative,
                FrictionMode::Stick,
                FrictionMode::SlidingPositive,
            ]
        }
        .map(CoupledDeckFrictionMode::from);
        let slipmat_modes = contact_mode_order_with_static_prediction(
            predicted_slipmat_relative_velocity_rad_s,
            next.slipmat_mode,
            slipmat_static_is_predicted,
        )
        .map(Into::into);
        let (hand_modes, hand_mode_count) = if hand_active {
            (
                contact_mode_order_for_relative_velocity(
                    hand_velocity_rad_s - next.record_angular_velocity_rad_s,
                    next.hand_mode,
                )
                .map(Into::into),
                3,
            )
        } else {
            ([CoupledDeckFrictionMode::Separated; 3], 1)
        };
        Ok(DeckMidpointPreparation {
            next,
            config,
            dt,
            completes_integration_step,
            previous_platter_velocity_rad_s: self.platter_angular_velocity_rad_s,
            previous_record_velocity_rad_s: self.record_angular_velocity_rad_s,
            predicted_record_velocity_rad_s,
            motor_torque_nm,
            hand_velocity_rad_s,
            hand_kinetic_limit_nm: config.hand_kinetic_friction_coefficient
                * control.hand_normal_force_n
                * control.hand_contact_radius_m,
            bearing_static_limit_nm,
            slipmat_static_limit_nm,
            hand_static_limit_nm: entry_limit(
                config.hand_static_friction_coefficient
                    * control.hand_normal_force_n
                    * control.hand_contact_radius_m,
                next.hand_mode,
            ),
            bearing_modes,
            slipmat_modes,
            hand_modes,
            hand_mode_count,
        })
    }

    pub fn advance(
        &mut self,
        duration_seconds: f64,
        control: DeckMechanicalControl,
    ) -> Result<DeckMechanicalTelemetry, DeckMechanicalError> {
        if !duration_seconds.is_finite() || duration_seconds < 0.0 {
            return Err(DeckMechanicalError::InvalidDuration);
        }
        if duration_seconds > MAX_ADVANCE_SECONDS {
            return Err(DeckMechanicalError::DurationAboveMaximum {
                maximum: MAX_ADVANCE_SECONDS,
            });
        }
        let control = control.validate_for_config(self.config)?;
        let mut next = *self;
        next.last_control = control;
        next.integration_remainder_seconds += duration_seconds;
        let dt = 1.0 / next.config.integration_hz;
        let steps = ((next.integration_remainder_seconds / dt) + 1.0e-10).floor() as u64;
        next.integration_remainder_seconds -= steps as f64 * dt;
        if next.integration_remainder_seconds < 0.0 {
            next.integration_remainder_seconds = 0.0;
        }
        for _ in 0..steps {
            next.advance_step(dt, control)?;
        }
        next.publish_telemetry();
        *self = next;
        Ok(self.last_telemetry)
    }

    pub fn telemetry(&self) -> DeckMechanicalTelemetry {
        self.last_telemetry
    }

    pub fn snapshot(&self) -> DeckMechanicalSnapshot {
        DeckMechanicalSnapshot {
            version: SNAPSHOT_VERSION,
            config: self.config,
            platter_angle_rad: self.platter_angle_rad,
            record_angle_rad: self.record_angle_rad,
            platter_angular_velocity_rad_s: self.platter_angular_velocity_rad_s,
            record_angular_velocity_rad_s: self.record_angular_velocity_rad_s,
            motor_integral_torque_nm: self.motor_integral_torque_nm,
            integration_remainder_seconds: self.integration_remainder_seconds,
            completed_steps: self.completed_steps,
            slipmat_mode: self.slipmat_mode,
            hand_mode: self.hand_mode,
            last_control: self.last_control,
            last_motor_torque_nm: self.last_motor_torque_nm,
            last_slipmat_torque_nm: self.last_slipmat_torque_nm,
            last_hand_torque_nm: self.last_hand_torque_nm,
            last_bearing_torque_nm: self.last_bearing_torque_nm,
            last_stylus_torque_nm: self.last_stylus_torque_nm,
            bearing_sticking: self.bearing_sticking,
        }
    }

    pub fn restore(&mut self, snapshot: DeckMechanicalSnapshot) -> Result<(), DeckMechanicalError> {
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(DeckMechanicalError::UnsupportedSnapshotVersion {
                version: snapshot.version,
            });
        }
        let config = snapshot.config.validate()?;
        snapshot.last_control.validate_for_config(config)?;
        for (field, value) in [
            ("platterAngleRad", snapshot.platter_angle_rad),
            ("recordAngleRad", snapshot.record_angle_rad),
            (
                "platterAngularVelocityRadS",
                snapshot.platter_angular_velocity_rad_s,
            ),
            (
                "recordAngularVelocityRadS",
                snapshot.record_angular_velocity_rad_s,
            ),
            ("motorIntegralTorqueNm", snapshot.motor_integral_torque_nm),
            (
                "integrationRemainderSeconds",
                snapshot.integration_remainder_seconds,
            ),
            ("lastMotorTorqueNm", snapshot.last_motor_torque_nm),
            ("lastSlipmatTorqueNm", snapshot.last_slipmat_torque_nm),
            ("lastHandTorqueNm", snapshot.last_hand_torque_nm),
            ("lastBearingTorqueNm", snapshot.last_bearing_torque_nm),
            ("lastStylusTorqueNm", snapshot.last_stylus_torque_nm),
        ] {
            validate_snapshot_finite(field, value)?;
        }
        let dt = 1.0 / config.integration_hz;
        let maximum_snapshot_angular_velocity =
            MAXIMUM_DECK_SNAPSHOT_RATE * config.nominal_angular_velocity_rad_s();
        if snapshot.integration_remainder_seconds < 0.0
            || snapshot.integration_remainder_seconds >= dt
            || snapshot.motor_integral_torque_nm.abs() > config.motor_integral_limit_nm
            || snapshot.platter_angular_velocity_rad_s.abs() > maximum_snapshot_angular_velocity
            || snapshot.record_angular_velocity_rad_s.abs() > maximum_snapshot_angular_velocity
            || snapshot.last_motor_torque_nm.abs() > config.motor_starting_torque_nm
            || snapshot.last_stylus_torque_nm.abs() > MAXIMUM_STYLUS_TORQUE_NM
            || snapshot.last_slipmat_torque_nm.abs() > MAXIMUM_DECK_SNAPSHOT_CONTACT_TORQUE_NM
            || snapshot.last_hand_torque_nm.abs() > MAXIMUM_DECK_SNAPSHOT_CONTACT_TORQUE_NM
            || snapshot.last_bearing_torque_nm.abs() > MAXIMUM_DECK_SNAPSHOT_CONTACT_TORQUE_NM
        {
            return Err(DeckMechanicalError::InvalidSnapshot);
        }

        self.config = config;
        self.platter_angle_rad = snapshot.platter_angle_rad;
        self.record_angle_rad = snapshot.record_angle_rad;
        self.platter_angular_velocity_rad_s = snapshot.platter_angular_velocity_rad_s;
        self.record_angular_velocity_rad_s = snapshot.record_angular_velocity_rad_s;
        self.motor_integral_torque_nm = snapshot.motor_integral_torque_nm;
        self.integration_remainder_seconds = snapshot.integration_remainder_seconds;
        self.completed_steps = snapshot.completed_steps;
        self.slipmat_mode = snapshot.slipmat_mode;
        self.hand_mode = snapshot.hand_mode;
        self.last_control = snapshot.last_control;
        self.last_motor_torque_nm = snapshot.last_motor_torque_nm;
        self.last_slipmat_torque_nm = snapshot.last_slipmat_torque_nm;
        self.last_hand_torque_nm = snapshot.last_hand_torque_nm;
        self.last_bearing_torque_nm = snapshot.last_bearing_torque_nm;
        self.last_stylus_torque_nm = snapshot.last_stylus_torque_nm;
        self.bearing_sticking = snapshot.bearing_sticking;
        self.publish_telemetry();
        Ok(())
    }

    fn advance_step(
        &mut self,
        dt: f64,
        control: DeckMechanicalControl,
    ) -> Result<(), DeckMechanicalError> {
        let config = self.config;
        let motor_torque = self.motor_torque(control, dt);
        let stylus_torque = control.stylus_torque_nm;
        let previous_platter_velocity = self.platter_angular_velocity_rad_s;
        let previous_record_velocity = self.record_angular_velocity_rad_s;
        let contact = solve_friction_step(
            previous_platter_velocity,
            previous_record_velocity,
            self.record_angle_rad,
            motor_torque,
            stylus_torque,
            control,
            config,
            self.slipmat_mode,
            self.hand_mode,
            self.bearing_sticking,
            dt,
        )?;

        self.platter_angular_velocity_rad_s = contact.platter_velocity_rad_s;
        self.record_angular_velocity_rad_s = contact.record_velocity_rad_s;
        self.platter_angle_rad +=
            0.5 * (previous_platter_velocity + self.platter_angular_velocity_rad_s) * dt;
        self.record_angle_rad +=
            0.5 * (previous_record_velocity + self.record_angular_velocity_rad_s) * dt;
        self.completed_steps = self
            .completed_steps
            .checked_add(1)
            .ok_or(DeckMechanicalError::StepCounterOverflow)?;
        self.slipmat_mode = contact.slipmat_mode;
        self.hand_mode = contact.hand_mode;
        self.last_motor_torque_nm = motor_torque;
        self.last_slipmat_torque_nm = contact.slipmat_torque_nm;
        self.last_hand_torque_nm = contact.hand_torque_nm;
        self.last_bearing_torque_nm = contact.bearing_torque_nm;
        self.last_stylus_torque_nm = stylus_torque;
        self.bearing_sticking = contact.bearing_sticking;
        if [
            self.platter_angle_rad,
            self.record_angle_rad,
            self.platter_angular_velocity_rad_s,
            self.record_angular_velocity_rad_s,
            self.motor_integral_torque_nm,
            self.last_motor_torque_nm,
            self.last_slipmat_torque_nm,
            self.last_hand_torque_nm,
            self.last_bearing_torque_nm,
            self.last_stylus_torque_nm,
        ]
        .into_iter()
        .any(|value| !value.is_finite())
        {
            return Err(DeckMechanicalError::NumericalFailure);
        }
        Ok(())
    }

    fn motor_torque(&mut self, control: DeckMechanicalControl, dt: f64) -> f64 {
        let config = self.config;
        match control.motor_mode {
            MotorMode::Off => {
                self.motor_integral_torque_nm = 0.0;
                0.0
            }
            MotorMode::Brake => {
                self.motor_integral_torque_nm = 0.0;
                (-self.platter_angular_velocity_rad_s * config.motor_brake_gain_nm_per_rad_s)
                    .clamp(-config.motor_brake_torque_nm, config.motor_brake_torque_nm)
            }
            MotorMode::Servo => {
                let error = control.motor_target_angular_velocity_rad_s
                    - self.platter_angular_velocity_rad_s;
                let feed_forward = opposing_torque(
                    -control.motor_target_angular_velocity_rad_s,
                    config.bearing_kinetic_torque_nm
                        + config.bearing_viscous_torque_nm_per_rad_s
                            * control.motor_target_angular_velocity_rad_s.abs(),
                );
                let candidate_integral = (self.motor_integral_torque_nm
                    + config.motor_servo_ki_nm_per_rad * error * dt)
                    .clamp(
                        -config.motor_integral_limit_nm,
                        config.motor_integral_limit_nm,
                    );
                let candidate =
                    config.motor_servo_kp_nm_per_rad_s * error + candidate_integral + feed_forward;
                let saturated = candidate.clamp(
                    -config.motor_starting_torque_nm,
                    config.motor_starting_torque_nm,
                );
                if candidate == saturated
                    || (candidate > saturated && error < 0.0)
                    || (candidate < saturated && error > 0.0)
                {
                    self.motor_integral_torque_nm = candidate_integral;
                }
                (config.motor_servo_kp_nm_per_rad_s * error
                    + self.motor_integral_torque_nm
                    + feed_forward)
                    .clamp(
                        -config.motor_starting_torque_nm,
                        config.motor_starting_torque_nm,
                    )
            }
        }
    }

    fn clear_last_torques(&mut self) {
        self.last_motor_torque_nm = 0.0;
        self.last_slipmat_torque_nm = 0.0;
        self.last_hand_torque_nm = 0.0;
        self.last_bearing_torque_nm = 0.0;
        self.last_stylus_torque_nm = 0.0;
        self.bearing_sticking = false;
    }

    fn publish_telemetry(&mut self) {
        let nominal = self.config.nominal_angular_velocity_rad_s();
        self.last_telemetry = DeckMechanicalTelemetry {
            mechanical_time_seconds: self.completed_steps as f64 / self.config.integration_hz,
            platter_rate: self.platter_angular_velocity_rad_s / nominal,
            record_rate: self.record_angular_velocity_rad_s / nominal,
            platter_angle_turns: self.platter_angle_rad / std::f64::consts::TAU,
            record_angle_turns: self.record_angle_rad / std::f64::consts::TAU,
            motor_torque_nm: self.last_motor_torque_nm,
            slipmat_torque_nm: self.last_slipmat_torque_nm,
            hand_torque_nm: self.last_hand_torque_nm,
            bearing_torque_nm: self.last_bearing_torque_nm,
            stylus_torque_nm: self.last_stylus_torque_nm,
            slipmat_mode: self.slipmat_mode,
            hand_mode: self.hand_mode,
            bearing_sticking: self.bearing_sticking,
        };
    }
}

impl DeckMidpointPreparation {
    /// Builds the next deck state after all joint constraints pass.
    pub(crate) fn commit(
        mut self,
        solution: DeckMidpointSolution,
    ) -> Result<DeckMechanicalState, DeckMechanicalError> {
        if solution.stylus_torque_nm.abs() > MAXIMUM_STYLUS_TORQUE_NM
            || !coupled_mode_is_valid(
                solution.bearing_mode,
                solution.platter_velocity_rad_s,
                solution.bearing_torque_nm,
                self.bearing_static_limit_nm,
            )
            || !coupled_mode_is_valid(
                solution.slipmat_mode,
                solution.platter_velocity_rad_s - solution.record_velocity_rad_s,
                solution.slipmat_torque_nm,
                self.slipmat_static_limit_nm,
            )
            || !coupled_mode_is_valid(
                solution.hand_mode,
                self.hand_velocity_rad_s - solution.record_velocity_rad_s,
                solution.hand_torque_nm,
                self.hand_static_limit_nm,
            )
        {
            return Err(DeckMechanicalError::ContactSolveFailure);
        }
        if [
            solution.platter_velocity_rad_s,
            solution.record_velocity_rad_s,
            solution.bearing_torque_nm,
            solution.slipmat_torque_nm,
            solution.hand_torque_nm,
            solution.stylus_torque_nm,
        ]
        .into_iter()
        .any(|value| !value.is_finite())
        {
            return Err(DeckMechanicalError::NumericalFailure);
        }

        self.next.platter_angle_rad += 0.5
            * (self.previous_platter_velocity_rad_s + solution.platter_velocity_rad_s)
            * self.dt;
        self.next.record_angle_rad +=
            0.5 * (self.previous_record_velocity_rad_s + solution.record_velocity_rad_s) * self.dt;
        self.next.platter_angular_velocity_rad_s = solution.platter_velocity_rad_s;
        self.next.record_angular_velocity_rad_s = solution.record_velocity_rad_s;
        if self.completes_integration_step {
            self.next.completed_steps = self
                .next
                .completed_steps
                .checked_add(1)
                .ok_or(DeckMechanicalError::StepCounterOverflow)?;
        }
        self.next.slipmat_mode = coupled_contact_mode(solution.slipmat_mode);
        self.next.hand_mode = coupled_contact_mode(solution.hand_mode);
        self.next.last_control.stylus_torque_nm = solution.stylus_torque_nm;
        self.next.last_motor_torque_nm = self.motor_torque_nm;
        self.next.last_slipmat_torque_nm = solution.slipmat_torque_nm;
        self.next.last_hand_torque_nm = solution.hand_torque_nm;
        self.next.last_bearing_torque_nm = solution.bearing_torque_nm;
        self.next.last_stylus_torque_nm = solution.stylus_torque_nm;
        self.next.bearing_sticking = solution.bearing_mode.is_sticking();
        self.next.publish_telemetry();
        Ok(self.next)
    }
}

#[derive(Debug, Clone, Copy)]
struct ContactSolution {
    platter_velocity_rad_s: f64,
    record_velocity_rad_s: f64,
    bearing_torque_nm: f64,
    slipmat_torque_nm: f64,
    hand_torque_nm: f64,
    slipmat_mode: ContactMode,
    hand_mode: ContactMode,
    bearing_sticking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrictionMode {
    Stick,
    SlidingPositive,
    SlidingNegative,
    Separated,
}

/// Identifies one deck friction branch for the midpoint coupled solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoupledDeckFrictionMode {
    Sticking,
    SlidingPositive,
    SlidingNegative,
    Separated,
}

impl CoupledDeckFrictionMode {
    pub(crate) fn is_sticking(self) -> bool {
        self == Self::Sticking
    }
}

impl From<FrictionMode> for CoupledDeckFrictionMode {
    fn from(mode: FrictionMode) -> Self {
        match mode {
            FrictionMode::Stick => Self::Sticking,
            FrictionMode::SlidingPositive => Self::SlidingPositive,
            FrictionMode::SlidingNegative => Self::SlidingNegative,
            FrictionMode::Separated => Self::Separated,
        }
    }
}

/// Contains one prepared one-sample deck step before stylus coupling.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DeckMidpointPreparation {
    next: DeckMechanicalState,
    pub(crate) config: PhysicalDeckConfig,
    pub(crate) dt: f64,
    completes_integration_step: bool,
    pub(crate) previous_platter_velocity_rad_s: f64,
    pub(crate) previous_record_velocity_rad_s: f64,
    /// Gives a torque-predictor ordering hint. It does not remove any branch.
    pub(crate) predicted_record_velocity_rad_s: f64,
    pub(crate) motor_torque_nm: f64,
    pub(crate) hand_velocity_rad_s: f64,
    pub(crate) hand_kinetic_limit_nm: f64,
    pub(crate) bearing_static_limit_nm: f64,
    pub(crate) slipmat_static_limit_nm: f64,
    pub(crate) hand_static_limit_nm: f64,
    pub(crate) bearing_modes: [CoupledDeckFrictionMode; 3],
    pub(crate) slipmat_modes: [CoupledDeckFrictionMode; 3],
    pub(crate) hand_modes: [CoupledDeckFrictionMode; 3],
    pub(crate) hand_mode_count: usize,
}

/// Returns the deck variables selected by one joint active-set solve.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DeckMidpointSolution {
    pub(crate) platter_velocity_rad_s: f64,
    pub(crate) record_velocity_rad_s: f64,
    pub(crate) bearing_torque_nm: f64,
    pub(crate) slipmat_torque_nm: f64,
    pub(crate) hand_torque_nm: f64,
    pub(crate) stylus_torque_nm: f64,
    pub(crate) bearing_mode: CoupledDeckFrictionMode,
    pub(crate) slipmat_mode: CoupledDeckFrictionMode,
    pub(crate) hand_mode: CoupledDeckFrictionMode,
}

#[allow(clippy::too_many_arguments)]
fn solve_friction_step(
    previous_platter_velocity_rad_s: f64,
    previous_record_velocity_rad_s: f64,
    record_angle_rad: f64,
    motor_torque_nm: f64,
    stylus_torque_nm: f64,
    control: DeckMechanicalControl,
    config: PhysicalDeckConfig,
    previous_slipmat_mode: ContactMode,
    previous_hand_mode: ContactMode,
    previous_bearing_sticking: bool,
    dt: f64,
) -> Result<ContactSolution, DeckMechanicalError> {
    let slip_static_limit = entry_limit(config.slipmat_static_torque_nm, previous_slipmat_mode);
    let hand_active = control.hand_contact
        && control.hand_normal_force_n > 0.0
        && control.hand_contact_radius_m > 0.0;
    let hand_velocity = effective_hand_velocity(control, config, record_angle_rad);
    let hand_static_limit = entry_limit(
        config.hand_static_friction_coefficient
            * control.hand_normal_force_n
            * control.hand_contact_radius_m,
        previous_hand_mode,
    );
    let hand_kinetic_limit = config.hand_kinetic_friction_coefficient
        * control.hand_normal_force_n
        * control.hand_contact_radius_m;
    if hand_active
        && motor_torque_nm == 0.0
        && stylus_torque_nm == 0.0
        && previous_platter_velocity_rad_s.abs() <= REST_ANGULAR_VELOCITY_RAD_S
        && previous_record_velocity_rad_s.abs() <= REST_ANGULAR_VELOCITY_RAD_S
        && hand_velocity.abs() <= REST_ANGULAR_VELOCITY_RAD_S
    {
        // Bearing, slipmat, and hand sticking constraints are redundant here.
        // The unique physical motion is rest, but the full multiplier system
        // has no unique torque distribution. Select the zero-load equilibrium.
        return Ok(ContactSolution {
            platter_velocity_rad_s: 0.0,
            record_velocity_rad_s: 0.0,
            bearing_torque_nm: 0.0,
            slipmat_torque_nm: 0.0,
            hand_torque_nm: 0.0,
            slipmat_mode: ContactMode::Sticking,
            hand_mode: ContactMode::Sticking,
            bearing_sticking: true,
        });
    }
    let bearing_static_limit = if previous_bearing_sticking {
        config.bearing_static_torque_nm
    } else {
        config.bearing_static_torque_nm * STATIC_ENTRY_RATIO
    };
    let bearing_modes = if previous_bearing_sticking {
        [
            FrictionMode::Stick,
            FrictionMode::SlidingPositive,
            FrictionMode::SlidingNegative,
        ]
    } else if previous_platter_velocity_rad_s >= 0.0 {
        [
            FrictionMode::SlidingPositive,
            FrictionMode::Stick,
            FrictionMode::SlidingNegative,
        ]
    } else {
        [
            FrictionMode::SlidingNegative,
            FrictionMode::Stick,
            FrictionMode::SlidingPositive,
        ]
    };
    let slipmat_modes = contact_mode_order(previous_slipmat_mode);
    let active_hand_modes = contact_mode_order(previous_hand_mode);
    let hand_modes: &[FrictionMode] = if hand_active {
        &active_hand_modes
    } else {
        &[FrictionMode::Separated]
    };

    for bearing_mode in bearing_modes {
        for slipmat_mode in slipmat_modes {
            for &hand_mode in hand_modes {
                let Some(solution) = solve_friction_modes(
                    previous_platter_velocity_rad_s,
                    previous_record_velocity_rad_s,
                    motor_torque_nm,
                    stylus_torque_nm,
                    hand_velocity,
                    bearing_mode,
                    slipmat_mode,
                    hand_mode,
                    hand_kinetic_limit,
                    config,
                    dt,
                ) else {
                    continue;
                };
                if mode_is_valid(
                    bearing_mode,
                    solution.platter_velocity_rad_s,
                    solution.bearing_torque_nm,
                    bearing_static_limit,
                ) && mode_is_valid(
                    slipmat_mode,
                    solution.platter_velocity_rad_s - solution.record_velocity_rad_s,
                    solution.slipmat_torque_nm,
                    slip_static_limit,
                ) && mode_is_valid(
                    hand_mode,
                    hand_velocity - solution.record_velocity_rad_s,
                    solution.hand_torque_nm,
                    hand_static_limit,
                ) {
                    return Ok(solution);
                }
            }
        }
    }

    Err(DeckMechanicalError::ContactSolveFailure)
}

#[allow(clippy::too_many_arguments)]
fn solve_friction_modes(
    previous_platter_velocity_rad_s: f64,
    previous_record_velocity_rad_s: f64,
    motor_torque_nm: f64,
    stylus_torque_nm: f64,
    hand_velocity_rad_s: f64,
    bearing_mode: FrictionMode,
    slipmat_mode: FrictionMode,
    hand_mode: FrictionMode,
    hand_kinetic_torque_nm: f64,
    config: PhysicalDeckConfig,
    dt: f64,
) -> Option<ContactSolution> {
    let mut augmented = [[0.0_f64; 6]; 5];
    let mut variable_count = 2;
    let platter_mass = config.platter_inertia_kg_m2 / dt;
    let record_mass = config.record_inertia_kg_m2 / dt;
    augmented[0][0] = platter_mass;
    augmented[0][5] = platter_mass * previous_platter_velocity_rad_s + motor_torque_nm;
    augmented[1][1] = record_mass;
    augmented[1][5] = record_mass * previous_record_velocity_rad_s + stylus_torque_nm;

    let mut bearing_static_column = None;
    match bearing_mode {
        FrictionMode::Stick => {
            bearing_static_column = Some(variable_count);
            augmented[0][variable_count] = -1.0;
            variable_count += 1;
        }
        FrictionMode::SlidingPositive => {
            augmented[0][0] += config.bearing_viscous_torque_nm_per_rad_s;
            augmented[0][5] -= config.bearing_kinetic_torque_nm;
        }
        FrictionMode::SlidingNegative => {
            augmented[0][0] += config.bearing_viscous_torque_nm_per_rad_s;
            augmented[0][5] += config.bearing_kinetic_torque_nm;
        }
        FrictionMode::Separated => return None,
    }

    let mut slipmat_static_column = None;
    match slipmat_mode {
        FrictionMode::Stick => {
            slipmat_static_column = Some(variable_count);
            augmented[0][variable_count] = 1.0;
            augmented[1][variable_count] = -1.0;
            variable_count += 1;
        }
        FrictionMode::SlidingPositive | FrictionMode::SlidingNegative => {
            let bias = if slipmat_mode == FrictionMode::SlidingPositive {
                config.slipmat_kinetic_torque_nm
            } else {
                -config.slipmat_kinetic_torque_nm
            };
            let damping = config.slipmat_viscous_torque_nm_per_rad_s;
            augmented[0][0] += damping;
            augmented[0][1] -= damping;
            augmented[0][5] -= bias;
            augmented[1][0] -= damping;
            augmented[1][1] += damping;
            augmented[1][5] += bias;
        }
        FrictionMode::Separated => return None,
    }

    let mut hand_static_column = None;
    match hand_mode {
        FrictionMode::Stick => {
            hand_static_column = Some(variable_count);
            augmented[1][variable_count] = -1.0;
            variable_count += 1;
        }
        FrictionMode::SlidingPositive | FrictionMode::SlidingNegative => {
            let bias = if hand_mode == FrictionMode::SlidingPositive {
                hand_kinetic_torque_nm
            } else {
                -hand_kinetic_torque_nm
            };
            let damping = config.hand_viscous_torque_nm_per_rad_s;
            augmented[1][1] += damping;
            augmented[1][5] += bias + damping * hand_velocity_rad_s;
        }
        FrictionMode::Separated => {}
    }

    if let Some(column) = bearing_static_column {
        let row = 2;
        augmented[row][0] = 1.0;
        augmented[row][5] = 0.0;
        debug_assert_eq!(column, 2);
    }
    if let Some(column) = slipmat_static_column {
        let row = column;
        augmented[row][0] = 1.0;
        augmented[row][1] = -1.0;
        augmented[row][5] = 0.0;
    }
    if let Some(column) = hand_static_column {
        let row = column;
        augmented[row][1] = 1.0;
        augmented[row][5] = hand_velocity_rad_s;
    }

    let solution = solve_contact_linear_system(&mut augmented, variable_count)?;
    let platter_velocity_rad_s = solution[0];
    let record_velocity_rad_s = solution[1];
    let bearing_torque_nm = bearing_static_column.map_or_else(
        || {
            let sign = if bearing_mode == FrictionMode::SlidingPositive {
                -1.0
            } else {
                1.0
            };
            sign * config.bearing_kinetic_torque_nm
                - config.bearing_viscous_torque_nm_per_rad_s * platter_velocity_rad_s
        },
        |column| solution[column],
    );
    let slipmat_torque_nm = slipmat_static_column.map_or_else(
        || {
            let sign = if slipmat_mode == FrictionMode::SlidingPositive {
                1.0
            } else {
                -1.0
            };
            sign * config.slipmat_kinetic_torque_nm
                + config.slipmat_viscous_torque_nm_per_rad_s
                    * (platter_velocity_rad_s - record_velocity_rad_s)
        },
        |column| solution[column],
    );
    let hand_torque_nm = match hand_mode {
        FrictionMode::Separated => 0.0,
        FrictionMode::Stick => solution[hand_static_column?],
        FrictionMode::SlidingPositive | FrictionMode::SlidingNegative => {
            let sign = if hand_mode == FrictionMode::SlidingPositive {
                1.0
            } else {
                -1.0
            };
            sign * hand_kinetic_torque_nm
                + config.hand_viscous_torque_nm_per_rad_s
                    * (hand_velocity_rad_s - record_velocity_rad_s)
        }
    };
    Some(ContactSolution {
        platter_velocity_rad_s,
        record_velocity_rad_s,
        bearing_torque_nm,
        slipmat_torque_nm,
        hand_torque_nm,
        slipmat_mode: contact_mode(slipmat_mode),
        hand_mode: contact_mode(hand_mode),
        bearing_sticking: bearing_mode == FrictionMode::Stick,
    })
}

fn effective_hand_velocity(
    control: DeckMechanicalControl,
    config: PhysicalDeckConfig,
    record_angle_rad: f64,
) -> f64 {
    let position_correction = control.hand_target_angle_rad.map_or(0.0, |target| {
        // A fast-moving hand can also correct fast: catch-up authority grows
        // with stroke speed so tracking error from a hard stroke does not
        // linger for seconds under the fixed low cap.
        let correction_limit = config.hand_max_position_correction_rad_s.max(
            HAND_CATCHUP_RATE_SHARE * control.hand_target_angular_velocity_rad_s.abs(),
        );
        ((target - record_angle_rad) / config.hand_position_stabilization_seconds)
            .clamp(-correction_limit, correction_limit)
    });
    let maximum_velocity = MAXIMUM_DECK_RATE * config.nominal_angular_velocity_rad_s();
    (control.hand_target_angular_velocity_rad_s + position_correction)
        .clamp(-maximum_velocity, maximum_velocity)
}

fn solve_contact_linear_system(augmented: &mut [[f64; 6]; 5], size: usize) -> Option<[f64; 5]> {
    for pivot_column in 0..size {
        let pivot_row = (pivot_column..size).max_by(|left, right| {
            augmented[*left][pivot_column]
                .abs()
                .total_cmp(&augmented[*right][pivot_column].abs())
        })?;
        let pivot = augmented[pivot_row][pivot_column];
        if !pivot.is_finite() || pivot.abs() < 1.0e-18 {
            return None;
        }
        augmented.swap(pivot_column, pivot_row);
        for value in &mut augmented[pivot_column][pivot_column..size] {
            *value /= pivot;
        }
        augmented[pivot_column][5] /= pivot;
        let pivot_values = augmented[pivot_column];
        for (row, augmented_row) in augmented.iter_mut().enumerate().take(size) {
            if row == pivot_column {
                continue;
            }
            let scale = augmented_row[pivot_column];
            for (value, pivot_value) in augmented_row[pivot_column..size]
                .iter_mut()
                .zip(&pivot_values[pivot_column..size])
            {
                *value -= scale * pivot_value;
            }
            augmented_row[5] -= scale * pivot_values[5];
        }
    }
    let mut solution = [0.0; 5];
    for row in 0..size {
        solution[row] = augmented[row][5];
    }
    solution
        .iter()
        .take(size)
        .all(|value| value.is_finite())
        .then_some(solution)
}

fn mode_is_valid(
    mode: FrictionMode,
    relative_velocity_rad_s: f64,
    torque_nm: f64,
    static_limit_nm: f64,
) -> bool {
    if !relative_velocity_rad_s.is_finite() || !torque_nm.is_finite() {
        return false;
    }
    match mode {
        FrictionMode::Stick => {
            relative_velocity_rad_s.abs() <= REST_ANGULAR_VELOCITY_RAD_S
                && torque_nm.abs() <= static_limit_nm + REST_TORQUE_NM
        }
        FrictionMode::SlidingPositive => relative_velocity_rad_s > 0.0,
        FrictionMode::SlidingNegative => relative_velocity_rad_s < 0.0,
        FrictionMode::Separated => torque_nm == 0.0,
    }
}

pub(crate) fn coupled_deck_mode_is_valid(
    mode: CoupledDeckFrictionMode,
    relative_velocity_rad_s: f64,
    torque_nm: f64,
    static_limit_nm: f64,
) -> bool {
    if !relative_velocity_rad_s.is_finite() || !torque_nm.is_finite() {
        return false;
    }
    match mode {
        CoupledDeckFrictionMode::Sticking => {
            relative_velocity_rad_s.abs() <= REST_ANGULAR_VELOCITY_RAD_S
                && torque_nm.abs() <= static_limit_nm + REST_TORQUE_NM
        }
        CoupledDeckFrictionMode::SlidingPositive => relative_velocity_rad_s > 0.0,
        CoupledDeckFrictionMode::SlidingNegative => relative_velocity_rad_s < 0.0,
        CoupledDeckFrictionMode::Separated => torque_nm == 0.0,
    }
}

fn coupled_mode_is_valid(
    mode: CoupledDeckFrictionMode,
    relative_velocity_rad_s: f64,
    torque_nm: f64,
    static_limit_nm: f64,
) -> bool {
    coupled_deck_mode_is_valid(mode, relative_velocity_rad_s, torque_nm, static_limit_nm)
}

fn coupled_contact_mode(mode: CoupledDeckFrictionMode) -> ContactMode {
    match mode {
        CoupledDeckFrictionMode::Sticking => ContactMode::Sticking,
        CoupledDeckFrictionMode::SlidingPositive => ContactMode::SlidingPositive,
        CoupledDeckFrictionMode::SlidingNegative => ContactMode::SlidingNegative,
        CoupledDeckFrictionMode::Separated => ContactMode::Separated,
    }
}

fn contact_mode(mode: FrictionMode) -> ContactMode {
    match mode {
        FrictionMode::Stick => ContactMode::Sticking,
        FrictionMode::SlidingPositive => ContactMode::SlidingPositive,
        FrictionMode::SlidingNegative => ContactMode::SlidingNegative,
        FrictionMode::Separated => ContactMode::Separated,
    }
}

fn contact_mode_order(previous: ContactMode) -> [FrictionMode; 3] {
    match previous {
        ContactMode::Sticking | ContactMode::Separated => [
            FrictionMode::Stick,
            FrictionMode::SlidingPositive,
            FrictionMode::SlidingNegative,
        ],
        ContactMode::SlidingPositive => [
            FrictionMode::SlidingPositive,
            FrictionMode::Stick,
            FrictionMode::SlidingNegative,
        ],
        ContactMode::SlidingNegative => [
            FrictionMode::SlidingNegative,
            FrictionMode::Stick,
            FrictionMode::SlidingPositive,
        ],
    }
}

fn contact_mode_order_for_relative_velocity(
    relative_velocity_rad_s: f64,
    previous: ContactMode,
) -> [FrictionMode; 3] {
    if relative_velocity_rad_s > REST_ANGULAR_VELOCITY_RAD_S {
        [
            FrictionMode::SlidingPositive,
            FrictionMode::Stick,
            FrictionMode::SlidingNegative,
        ]
    } else if relative_velocity_rad_s < -REST_ANGULAR_VELOCITY_RAD_S {
        [
            FrictionMode::SlidingNegative,
            FrictionMode::Stick,
            FrictionMode::SlidingPositive,
        ]
    } else {
        contact_mode_order(previous)
    }
}

fn contact_mode_order_with_static_prediction(
    relative_velocity_rad_s: f64,
    previous: ContactMode,
    static_is_predicted: bool,
) -> [FrictionMode; 3] {
    if !static_is_predicted {
        return contact_mode_order_for_relative_velocity(relative_velocity_rad_s, previous);
    }
    if relative_velocity_rad_s < 0.0 {
        [
            FrictionMode::Stick,
            FrictionMode::SlidingNegative,
            FrictionMode::SlidingPositive,
        ]
    } else {
        [
            FrictionMode::Stick,
            FrictionMode::SlidingPositive,
            FrictionMode::SlidingNegative,
        ]
    }
}

fn opposing_torque(angular_velocity: f64, magnitude: f64) -> f64 {
    if angular_velocity.abs() <= REST_ANGULAR_VELOCITY_RAD_S {
        0.0
    } else {
        -angular_velocity.signum() * magnitude
    }
}

fn entry_limit(static_limit: f64, mode: ContactMode) -> f64 {
    if mode == ContactMode::Sticking {
        static_limit
    } else {
        static_limit * STATIC_ENTRY_RATIO
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum PhysicalDeckConfigError {
    #[error("{field} must be finite and nonnegative")]
    InvalidNonnegative { field: &'static str },
    #[error("{field} must be finite and positive")]
    InvalidPositive { field: &'static str },
    #[error("{field} is below its minimum {minimum}")]
    BelowMinimum { field: &'static str, minimum: f64 },
    #[error("{field} is above its maximum {maximum}")]
    AboveMaximum { field: &'static str, maximum: f64 },
    #[error("{contact} kinetic friction exceeds static friction")]
    KineticExceedsStatic { contact: &'static str },
    #[error("{field} is unstable at the configured integration rate")]
    UnstableIntegration { field: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum DeckMechanicalError {
    #[error(transparent)]
    InvalidConfig(#[from] PhysicalDeckConfigError),
    #[error("duration must be finite and nonnegative")]
    InvalidDuration,
    #[error("duration exceeds {maximum} seconds")]
    DurationAboveMaximum { maximum: f64 },
    #[error("the midpoint solver requires exactly one deck integration step")]
    MidpointStepRequiresOneIntegrationStep,
    #[error("control field {field} is invalid")]
    InvalidControl { field: &'static str },
    #[error("snapshot field {field} is invalid")]
    InvalidSnapshotField { field: &'static str },
    #[error("snapshot state is inconsistent")]
    InvalidSnapshot,
    #[error("snapshot version {version} is unsupported")]
    UnsupportedSnapshotVersion { version: u32 },
    #[error("the deck friction constraints have no consistent solution")]
    ContactSolveFailure,
    #[error("the deck step counter exceeded its supported range")]
    StepCounterOverflow,
    #[error("the deck integration produced a nonfinite value")]
    NumericalFailure,
}

fn validate_nonnegative(field: &'static str, value: f64) -> Result<(), PhysicalDeckConfigError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(PhysicalDeckConfigError::InvalidNonnegative { field })
    }
}

fn validate_positive(field: &'static str, value: f64) -> Result<(), PhysicalDeckConfigError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(PhysicalDeckConfigError::InvalidPositive { field })
    }
}

fn validate_minimum(
    field: &'static str,
    value: f64,
    minimum: f64,
) -> Result<(), PhysicalDeckConfigError> {
    validate_positive(field, value)?;
    if value < minimum {
        Err(PhysicalDeckConfigError::BelowMinimum { field, minimum })
    } else {
        Ok(())
    }
}

fn validate_step_ratio(field: &'static str, ratio: f64) -> Result<(), PhysicalDeckConfigError> {
    if ratio.is_finite() && ratio <= MAX_EXPLICIT_STEP_RATIO {
        Ok(())
    } else {
        Err(PhysicalDeckConfigError::UnstableIntegration { field })
    }
}

fn validate_control_finite(field: &'static str, value: f64) -> Result<(), DeckMechanicalError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(DeckMechanicalError::InvalidControl { field })
    }
}

fn validate_control_nonnegative(
    field: &'static str,
    value: f64,
) -> Result<(), DeckMechanicalError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(DeckMechanicalError::InvalidControl { field })
    }
}

fn validate_snapshot_finite(field: &'static str, value: f64) -> Result<(), DeckMechanicalError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(DeckMechanicalError::InvalidSnapshotField { field })
    }
}

fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

fn zero_telemetry() -> DeckMechanicalTelemetry {
    DeckMechanicalTelemetry {
        mechanical_time_seconds: 0.0,
        platter_rate: 0.0,
        record_rate: 0.0,
        platter_angle_turns: 0.0,
        record_angle_turns: 0.0,
        motor_torque_nm: 0.0,
        slipmat_torque_nm: 0.0,
        hand_torque_nm: 0.0,
        bearing_torque_nm: 0.0,
        stylus_torque_nm: 0.0,
        slipmat_mode: ContactMode::Sticking,
        hand_mode: ContactMode::Separated,
        bearing_sticking: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running_motor(config: PhysicalDeckConfig, rate: f64) -> DeckMechanicalControl {
        DeckMechanicalControl::from_normalized(
            config,
            NormalizedDeckControl {
                motor_mode: MotorMode::Servo,
                motor_rate: rate,
                hand_contact: false,
                hand_target_angle_turns: None,
                hand_rate: 0.0,
                grip: 0.0,
                stylus_torque_nm: 0.0,
            },
        )
    }

    fn scratching(
        config: PhysicalDeckConfig,
        motor_rate: f64,
        hand_rate: f64,
        normal_force_n: f64,
    ) -> DeckMechanicalControl {
        let mut control = DeckMechanicalControl::from_normalized(
            config,
            NormalizedDeckControl {
                motor_mode: MotorMode::Servo,
                motor_rate,
                hand_contact: true,
                hand_target_angle_turns: None,
                hand_rate,
                grip: 1.0,
                stylus_torque_nm: 0.0,
            },
        );
        control.hand_normal_force_n = normal_force_n;
        control
    }

    fn advance_seconds(
        state: &mut DeckMechanicalState,
        seconds: f64,
        control: DeckMechanicalControl,
    ) -> DeckMechanicalTelemetry {
        let mut remaining = seconds;
        while remaining > 0.0 {
            let duration = remaining.min(0.01);
            state.advance(duration, control).unwrap();
            remaining -= duration;
        }
        state.telemetry()
    }

    #[test]
    fn seed_uses_published_deck_scale_without_claiming_calibration() {
        let config = PhysicalDeckConfig::default();
        assert!((config.motor_starting_torque_nm - 0.18).abs() < f64::EPSILON);
        assert!((config.platter_inertia_kg_m2 - 0.024_800_4).abs() < 1.0e-7);
        assert!((config.nominal_angular_velocity_rad_s() - 3.490_658_503_988_659).abs() < 1.0e-12);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn motor_reaches_nominal_speed_within_the_published_start_time() {
        let config = PhysicalDeckConfig::default();
        let mut state = DeckMechanicalState::new(config).unwrap();
        let at_700_ms = advance_seconds(&mut state, 0.7, running_motor(config, 1.0));
        assert!(at_700_ms.platter_rate > 0.99, "{}", at_700_ms.platter_rate);
        assert!(at_700_ms.record_rate > 0.99, "{}", at_700_ms.record_rate);
        assert!((at_700_ms.platter_rate - at_700_ms.record_rate).abs() < 1.0e-9);
    }

    #[test]
    fn high_torque_seed_reaches_nominal_speed_within_two_hundred_ms() {
        let config = PhysicalDeckConfig::high_torque_dj_seed();
        let mut state = DeckMechanicalState::new(config).unwrap();
        let at_200_ms = advance_seconds(&mut state, 0.2, running_motor(config, 1.0));
        assert!(at_200_ms.platter_rate > 0.99, "{at_200_ms:?}");
        assert!(at_200_ms.record_rate > 0.99, "{}", at_200_ms.record_rate);
        assert!(at_200_ms.platter_rate < 1.015, "{}", at_200_ms.platter_rate);
    }

    #[test]
    fn pi_servo_removes_steady_load_droop() {
        let config = PhysicalDeckConfig::default();
        let mut state = DeckMechanicalState::new(config).unwrap();
        let mut control = running_motor(config, 1.0);
        control.stylus_torque_nm = -0.000_4;
        let settled = advance_seconds(&mut state, 3.0, control);
        assert!(
            (settled.platter_rate - 1.0).abs() < 1.0e-5,
            "{}",
            settled.platter_rate
        );
    }

    #[test]
    fn firm_hand_separates_and_reverses_record_over_platter() {
        let config = PhysicalDeckConfig::default();
        let mut state = DeckMechanicalState::new(config).unwrap();
        advance_seconds(&mut state, 1.0, running_motor(config, 1.0));
        let grabbed = advance_seconds(&mut state, 0.045, scratching(config, 1.0, -1.0, 5.0));
        assert!(grabbed.record_rate < -0.75, "{}", grabbed.record_rate);
        assert!(grabbed.platter_rate > 0.75, "{}", grabbed.platter_rate);
        assert_ne!(grabbed.slipmat_mode, ContactMode::Sticking);
    }

    #[test]
    fn releasing_hand_recouples_without_resetting_phase() {
        let config = PhysicalDeckConfig::default();
        let mut state = DeckMechanicalState::new(config).unwrap();
        advance_seconds(&mut state, 1.0, running_motor(config, 1.0));
        advance_seconds(&mut state, 0.04, scratching(config, 1.0, -0.8, 5.0));
        let phase_before = state.telemetry().record_angle_turns;
        let caught = advance_seconds(&mut state, 0.5, running_motor(config, 1.0));
        assert!(caught.record_rate > 0.99, "{}", caught.record_rate);
        assert!((caught.platter_rate - caught.record_rate).abs() < 1.0e-9);
        assert_ne!(caught.record_angle_turns, phase_before);
    }

    #[test]
    fn simultaneous_static_constraints_produce_equal_velocities() {
        let config = PhysicalDeckConfig::default();
        let mut state = DeckMechanicalState::new(config).unwrap();
        state.reset(1.0, 1.0, 0.0, 0.0).unwrap();
        let control = scratching(config, 1.0, 1.0, 5.0);
        let telemetry = state.advance(1.0 / config.integration_hz, control).unwrap();
        assert_eq!(telemetry.slipmat_mode, ContactMode::Sticking);
        assert_eq!(telemetry.hand_mode, ContactMode::Sticking);
        assert_eq!(telemetry.platter_rate, telemetry.record_rate);
    }

    #[test]
    fn stationary_hand_holds_an_unpowered_stopped_deck_without_rank_failure() {
        let mut config = PhysicalDeckConfig::high_torque_dj_seed();
        config.nominal_rpm = 45.0;
        config.integration_hz = 48_000.0;
        let mut state = DeckMechanicalState::new(config).unwrap();
        let turns = 61.250_890_548_885_84;
        let residual_rate = -2.246_824_675_286_976e-23;
        state
            .reset(residual_rate, residual_rate, turns, turns)
            .unwrap();
        let control = DeckMechanicalControl::from_normalized(
            config,
            NormalizedDeckControl {
                motor_mode: MotorMode::Off,
                motor_rate: 0.0,
                hand_contact: true,
                hand_target_angle_turns: Some(turns),
                hand_rate: 0.0,
                grip: 0.988_256_371_542_977_2,
                stylus_torque_nm: 0.0,
            },
        );

        for _ in 0..12_000 {
            let telemetry = state.advance(1.0 / 48_000.0, control).unwrap();
            assert_eq!(telemetry.platter_rate, 0.0);
            assert_eq!(telemetry.record_rate, 0.0);
            assert_eq!(telemetry.slipmat_mode, ContactMode::Sticking);
            assert_eq!(telemetry.hand_mode, ContactMode::Sticking);
            assert!(telemetry.bearing_sticking);
        }
    }

    #[test]
    fn bearing_and_slipmat_balance_stylus_torque_in_the_same_sample() {
        let config = PhysicalDeckConfig::default();
        let mut state = DeckMechanicalState::new(config).unwrap();
        let control = DeckMechanicalControl {
            stylus_torque_nm: 0.000_1,
            ..DeckMechanicalControl::default()
        };
        let telemetry = state.advance(1.0 / config.integration_hz, control).unwrap();
        assert_eq!(telemetry.platter_rate, 0.0);
        assert_eq!(telemetry.record_rate, 0.0);
        assert_eq!(telemetry.slipmat_mode, ContactMode::Sticking);
        assert!(telemetry.bearing_sticking);
        assert!((telemetry.slipmat_torque_nm + control.stylus_torque_nm).abs() < 1.0e-15);
        assert!((telemetry.bearing_torque_nm - telemetry.slipmat_torque_nm).abs() < 1.0e-15);
    }

    #[test]
    fn kinetic_bearing_friction_stops_without_reversing_at_zero() {
        let config = PhysicalDeckConfig {
            integration_hz: 1_000.0,
            platter_inertia_kg_m2: MIN_INERTIA_KG_M2,
            record_inertia_kg_m2: MIN_INERTIA_KG_M2,
            motor_servo_kp_nm_per_rad_s: 0.0,
            motor_servo_ki_nm_per_rad: 0.0,
            motor_brake_gain_nm_per_rad_s: 0.0,
            bearing_viscous_torque_nm_per_rad_s: 0.0,
            slipmat_viscous_torque_nm_per_rad_s: 0.0,
            ..PhysicalDeckConfig::default()
        };
        let mut state = DeckMechanicalState::new(config).unwrap();
        let initial_velocity = 2.0 * REST_ANGULAR_VELOCITY_RAD_S;
        let initial_rate = initial_velocity / config.nominal_angular_velocity_rad_s();
        state.reset(initial_rate, initial_rate, 0.0, 0.0).unwrap();
        let telemetry = state
            .advance(
                1.0 / config.integration_hz,
                DeckMechanicalControl::default(),
            )
            .unwrap();
        assert_eq!(telemetry.platter_rate, 0.0);
        assert_eq!(telemetry.record_rate, 0.0);
        assert!(telemetry.bearing_sticking);
    }

    #[test]
    fn angle_uses_the_interval_average_velocity_under_constant_torque() {
        let config = PhysicalDeckConfig {
            bearing_static_torque_nm: 0.0,
            bearing_kinetic_torque_nm: 0.0,
            bearing_viscous_torque_nm_per_rad_s: 0.0,
            slipmat_static_torque_nm: 0.0,
            slipmat_kinetic_torque_nm: 0.0,
            slipmat_viscous_torque_nm_per_rad_s: 0.0,
            ..PhysicalDeckConfig::default()
        };
        let mut state = DeckMechanicalState::new(config).unwrap();
        let control = DeckMechanicalControl {
            stylus_torque_nm: 0.01,
            ..DeckMechanicalControl::default()
        };
        let dt = 1.0 / config.integration_hz;
        let telemetry = state.advance(dt, control).unwrap();
        let expected_velocity = control.stylus_torque_nm / config.record_inertia_kg_m2 * dt;
        let expected_angle = 0.5 * expected_velocity * dt;
        assert!((state.record_angular_velocity_rad_s - expected_velocity).abs() < 1.0e-15);
        assert!(
            (telemetry.record_angle_turns * std::f64::consts::TAU - expected_angle).abs() < 1.0e-18
        );
    }

    #[test]
    fn every_friction_port_opposes_relative_motion_during_rapid_reversals() {
        let config = PhysicalDeckConfig::default();
        let mut state = DeckMechanicalState::new(config).unwrap();
        state.reset(1.0, 1.0, 0.0, 0.0).unwrap();
        let dt = 1.0 / config.integration_hz;
        for sample in 0..20_000 {
            let hand_rate = if sample / 61 % 2 == 0 {
                MAXIMUM_DECK_RATE
            } else {
                -MAXIMUM_DECK_RATE
            };
            let control = scratching(config, 0.0, hand_rate, 100.0);
            let hand_velocity = effective_hand_velocity(control, config, state.record_angle_rad);
            let telemetry = state.advance(dt, control).unwrap();
            let platter_velocity = telemetry.platter_rate * config.nominal_angular_velocity_rad_s();
            let record_velocity = telemetry.record_rate * config.nominal_angular_velocity_rad_s();
            let bearing_power = telemetry.bearing_torque_nm * platter_velocity;
            let slipmat_power = telemetry.slipmat_torque_nm * (record_velocity - platter_velocity);
            let hand_friction_power = telemetry.hand_torque_nm * (record_velocity - hand_velocity);
            assert!(bearing_power <= 1.0e-12, "{sample}: {bearing_power}");
            assert!(slipmat_power <= 1.0e-12, "{sample}: {slipmat_power}");
            assert!(
                hand_friction_power <= 1.0e-12,
                "{sample}: {hand_friction_power}"
            );
            assert!(telemetry.platter_rate.is_finite());
            assert!(telemetry.record_rate.is_finite());
        }
    }

    #[test]
    fn step_counter_overflow_does_not_mutate_the_deck() {
        let config = PhysicalDeckConfig::default();
        let mut state = DeckMechanicalState::new(config).unwrap();
        let mut snapshot = state.snapshot();
        snapshot.completed_steps = u64::MAX;
        state.restore(snapshot).unwrap();
        let before = state;

        let error = state
            .advance(
                1.0 / config.integration_hz,
                DeckMechanicalControl::default(),
            )
            .unwrap_err();

        assert_eq!(error, DeckMechanicalError::StepCounterOverflow);
        assert_eq!(state, before);
    }

    #[test]
    fn hand_position_correction_cannot_exceed_the_deck_rate_limit() {
        let config = PhysicalDeckConfig::default();
        let maximum_velocity = MAXIMUM_DECK_RATE * config.nominal_angular_velocity_rad_s();
        let mut forward = scratching(config, 0.0, MAXIMUM_DECK_RATE, 5.0);
        forward.hand_target_angle_rad = Some(10.0);
        assert_eq!(
            effective_hand_velocity(forward, config, 0.0),
            maximum_velocity
        );

        let mut reverse = scratching(config, 0.0, -MAXIMUM_DECK_RATE, 5.0);
        reverse.hand_target_angle_rad = Some(-10.0);
        assert_eq!(
            effective_hand_velocity(reverse, config, 0.0),
            -maximum_velocity
        );
    }

    #[test]
    fn internal_slipmat_torque_conserves_angular_momentum() {
        let config = PhysicalDeckConfig {
            motor_starting_torque_nm: 0.0,
            motor_brake_torque_nm: 0.0,
            bearing_static_torque_nm: 0.0,
            bearing_kinetic_torque_nm: 0.0,
            bearing_viscous_torque_nm_per_rad_s: 0.0,
            ..PhysicalDeckConfig::default()
        };
        let mut state = DeckMechanicalState::new(config).unwrap();
        state.reset(1.0, -1.0, 0.0, 0.0).unwrap();
        let before = config.platter_inertia_kg_m2 * state.platter_angular_velocity_rad_s
            + config.record_inertia_kg_m2 * state.record_angular_velocity_rad_s;
        state
            .advance(0.25, DeckMechanicalControl::default())
            .unwrap();
        let after = config.platter_inertia_kg_m2 * state.platter_angular_velocity_rad_s
            + config.record_inertia_kg_m2 * state.record_angular_velocity_rad_s;
        assert!((after - before).abs() < 1.0e-10, "{before} -> {after}");
    }

    #[test]
    fn friction_never_increases_isolated_mechanical_energy() {
        let config = PhysicalDeckConfig {
            motor_starting_torque_nm: 0.0,
            motor_brake_torque_nm: 0.0,
            ..PhysicalDeckConfig::default()
        };
        let mut state = DeckMechanicalState::new(config).unwrap();
        state.reset(1.0, -1.0, 0.0, 0.0).unwrap();
        let energy = |state: &DeckMechanicalState| {
            0.5 * config.platter_inertia_kg_m2 * state.platter_angular_velocity_rad_s.powi(2)
                + 0.5 * config.record_inertia_kg_m2 * state.record_angular_velocity_rad_s.powi(2)
        };
        let before = energy(&state);
        state
            .advance(0.5, DeckMechanicalControl::default())
            .unwrap();
        assert!(energy(&state) <= before + 1.0e-12);
    }

    #[test]
    fn fixed_clock_is_invariant_to_call_partitioning() {
        let config = PhysicalDeckConfig::default();
        let control = running_motor(config, 1.0);
        let mut whole = DeckMechanicalState::new(config).unwrap();
        let mut partitioned = DeckMechanicalState::new(config).unwrap();
        whole.advance(0.731, control).unwrap();
        for duration in [0.001, 0.017, 0.000_3, 0.2, 0.111, 0.401_7] {
            partitioned.advance(duration, control).unwrap();
        }
        let whole = whole.telemetry();
        let partitioned = partitioned.telemetry();
        assert_eq!(
            whole.mechanical_time_seconds,
            partitioned.mechanical_time_seconds
        );
        assert!((whole.platter_rate - partitioned.platter_rate).abs() < 1.0e-12);
        assert!((whole.record_angle_turns - partitioned.record_angle_turns).abs() < 1.0e-12);
    }

    #[test]
    fn two_seconds_in_one_call_matches_two_one_second_calls() {
        let config = PhysicalDeckConfig::default();
        let control = running_motor(config, 1.0);
        let mut whole = DeckMechanicalState::new(config).unwrap();
        let mut split = DeckMechanicalState::new(config).unwrap();
        whole.advance(2.0, control).unwrap();
        split.advance(1.0, control).unwrap();
        split.advance(1.0, control).unwrap();
        assert_eq!(whole.snapshot(), split.snapshot());
    }

    #[test]
    fn invalid_control_does_not_change_valid_state() {
        let config = PhysicalDeckConfig::default();
        let mut state = DeckMechanicalState::new(config).unwrap();
        state.advance(0.1, running_motor(config, 1.0)).unwrap();
        let before = state.snapshot();
        let mut invalid = running_motor(config, 1.0);
        invalid.hand_normal_force_n = f64::NAN;
        assert!(state.advance(0.1, invalid).is_err());
        assert_eq!(state.snapshot(), before);
    }

    #[test]
    fn engineering_control_limits_reject_numeric_hazards_without_mutation() {
        let config = PhysicalDeckConfig::default();
        let mut state = DeckMechanicalState::new(config).unwrap();
        let before = state.snapshot();
        let mut invalid = running_motor(config, 1.0);
        invalid.motor_target_angular_velocity_rad_s =
            (MAXIMUM_DECK_RATE + 1.0) * config.nominal_angular_velocity_rad_s();
        invalid.hand_normal_force_n = MAXIMUM_HAND_NORMAL_FORCE_N + 1.0;
        assert!(state.advance(1.0 / config.integration_hz, invalid).is_err());
        assert_eq!(state.snapshot(), before);
        assert!(state.reset(MAXIMUM_DECK_RATE + 1.0, 0.0, 0.0, 0.0).is_err());
        assert_eq!(state.snapshot(), before);
    }

    #[test]
    fn snapshot_restores_the_coupling_memory_used_by_the_next_step() {
        let config = PhysicalDeckConfig::default();
        let control = scratching(config, 1.0, -1.0, 5.0);
        let mut source = DeckMechanicalState::new(config).unwrap();
        source.reset(1.0, 1.0, 0.0, 0.0).unwrap();
        source.advance(0.01, control).unwrap();
        let snapshot = source.snapshot();
        source
            .advance(1.0 / config.integration_hz, control)
            .unwrap();
        let expected = source.snapshot();

        let mut restored = DeckMechanicalState::new(config).unwrap();
        restored.restore(snapshot).unwrap();
        restored
            .advance(1.0 / config.integration_hz, control)
            .unwrap();
        assert_eq!(restored.snapshot(), expected);
    }

    #[test]
    fn snapshot_restores_independent_record_and_platter_phase() {
        let config = PhysicalDeckConfig::default();
        let mut state = DeckMechanicalState::new(config).unwrap();
        state.reset(1.0, -0.5, 17.0, -4.0).unwrap();
        state
            .advance(0.123_456, scratching(config, 1.0, -0.5, 5.0))
            .unwrap();
        let snapshot = state.snapshot();
        state.reset(0.0, 0.0, 0.0, 0.0).unwrap();
        state.restore(snapshot).unwrap();
        assert_eq!(state.snapshot(), snapshot);
        assert_ne!(
            state.telemetry().platter_angle_turns,
            state.telemetry().record_angle_turns
        );
    }

    #[test]
    fn invalid_configuration_is_rejected() {
        let invalid = PhysicalDeckConfig {
            platter_inertia_kg_m2: f64::NAN,
            ..PhysicalDeckConfig::default()
        };
        assert!(matches!(
            invalid.validate(),
            Err(PhysicalDeckConfigError::InvalidPositive { .. })
        ));

        let unstable = PhysicalDeckConfig {
            bearing_viscous_torque_nm_per_rad_s: 1.0e9,
            ..PhysicalDeckConfig::default()
        };
        assert!(matches!(
            unstable.validate(),
            Err(PhysicalDeckConfigError::UnstableIntegration { .. })
        ));

        let unstable_brake = PhysicalDeckConfig {
            motor_brake_gain_nm_per_rad_s: 1.0e9,
            ..PhysicalDeckConfig::default()
        };
        assert!(matches!(
            unstable_brake.validate(),
            Err(PhysicalDeckConfigError::UnstableIntegration {
                field: "motorBrakeGainNmPerRadS"
            })
        ));
    }
}
