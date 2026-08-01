use serde::{Deserialize, Serialize};
use thiserror::Error;

const SNAPSHOT_VERSION: u32 = 2;
const MAX_ADVANCE_SECONDS: f64 = 10.0;
const MAX_GENERATOR_COEFFICIENT_V_S_PER_M: f64 = 1.0e6;
const MIN_RESISTANCE_OHM: f64 = 1.0e-6;
const MAX_RESISTANCE_OHM: f64 = 1.0e12;
const MIN_INDUCTANCE_H: f64 = 1.0e-9;
const MAX_INDUCTANCE_H: f64 = 1.0e4;
const MIN_CAPACITANCE_F: f64 = 1.0e-15;
const MAX_CAPACITANCE_F: f64 = 1.0;
const MAX_CHANNEL_SEPARATION_DB: f64 = 200.0;
const MAX_ABS_CHANNEL_BALANCE_DB: f64 = 24.0;

/// Identifies the source of the generator coefficient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GeneratorCoefficientSource {
    DirectMeasurement,
    DerivedFromLoadedOutputSpecification,
    UserSupplied,
}

/// Defines a two-channel moving-magnet cartridge and its electrical load.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovingMagnetCartridgeConfig {
    /// Converts coil-axis magnet velocity to open-circuit generator voltage.
    pub generator_coefficient_v_s_per_m: f64,
    pub generator_coefficient_source: GeneratorCoefficientSource,
    pub coil_resistance_ohm: f64,
    pub coil_inductance_h: f64,
    pub load_resistance_ohm: f64,
    pub load_capacitance_f: f64,
    /// Sets the magnitude difference between the two direct channel gains.
    /// A positive value makes the left direct gain larger.
    pub channel_balance_db: f64,
    /// Sets the direct-to-crosstalk voltage ratio for each driven axis.
    pub channel_separation_db: f64,
}

impl MovingMagnetCartridgeConfig {
    /// Returns a seed for the Ortofon Concorde MKII Scratch cartridge.
    ///
    /// Ortofon publishes the circuit values and loaded output specification.
    /// The generator coefficient is derived, not published or measured directly.
    /// The load capacitance uses the midpoint of the published 200 pF to 400 pF range.
    /// The balance sign and in-phase crosstalk are representative assumptions.
    pub fn concorde_mkii_scratch_seed() -> Self {
        let coil_resistance_ohm = 1_200.0;
        let coil_inductance_h = 0.850;
        let load_resistance_ohm = 47_000.0;
        let load_capacitance_f = 300.0e-12;
        let loaded_ratio = loaded_voltage_ratio(
            coil_resistance_ohm,
            coil_inductance_h,
            load_resistance_ohm,
            load_capacitance_f,
            1_000.0,
        );

        // Ortofon specifies 10 mV RMS at 1 kHz and 5 cm/s.
        // This value refers to the nominal channel before balance variation.
        let generator_coefficient_v_s_per_m = 10.0e-3 / (0.05 * loaded_ratio);

        Self {
            generator_coefficient_v_s_per_m,
            generator_coefficient_source:
                GeneratorCoefficientSource::DerivedFromLoadedOutputSpecification,
            coil_resistance_ohm,
            coil_inductance_h,
            load_resistance_ohm,
            load_capacitance_f,
            channel_balance_db: 1.0,
            channel_separation_db: 22.0,
        }
    }

    pub fn validate(self) -> Result<Self, MovingMagnetCartridgeConfigError> {
        validate_bounded_positive(
            "generatorCoefficientVSPerM",
            self.generator_coefficient_v_s_per_m,
            f64::MIN_POSITIVE,
            MAX_GENERATOR_COEFFICIENT_V_S_PER_M,
        )?;
        validate_bounded_positive(
            "coilResistanceOhm",
            self.coil_resistance_ohm,
            MIN_RESISTANCE_OHM,
            MAX_RESISTANCE_OHM,
        )?;
        validate_bounded_positive(
            "coilInductanceH",
            self.coil_inductance_h,
            MIN_INDUCTANCE_H,
            MAX_INDUCTANCE_H,
        )?;
        validate_bounded_positive(
            "loadResistanceOhm",
            self.load_resistance_ohm,
            MIN_RESISTANCE_OHM,
            MAX_RESISTANCE_OHM,
        )?;
        validate_bounded_positive(
            "loadCapacitanceF",
            self.load_capacitance_f,
            MIN_CAPACITANCE_F,
            MAX_CAPACITANCE_F,
        )?;
        validate_bounded(
            "channelSeparationDb",
            self.channel_separation_db,
            0.0,
            MAX_CHANNEL_SEPARATION_DB,
        )?;
        validate_bounded(
            "channelBalanceDb",
            self.channel_balance_db,
            -MAX_ABS_CHANNEL_BALANCE_DB,
            MAX_ABS_CHANNEL_BALANCE_DB,
        )?;

        let matrix = self.channel_matrix();
        if matrix.into_iter().flatten().any(|value| !value.is_finite()) {
            return Err(MovingMagnetCartridgeConfigError::InvalidDerivedValue {
                field: "channelMatrix",
            });
        }

        Ok(self)
    }

    /// Returns the source-voltage matrix for the left and right coil axes.
    /// Rows select generator circuits. Columns select magnet-velocity axes.
    pub fn channel_matrix(self) -> [[f64; 2]; 2] {
        let left_gain = 10.0_f64.powf(self.channel_balance_db / 40.0);
        let right_gain = 10.0_f64.powf(-self.channel_balance_db / 40.0);
        let crosstalk_gain = 10.0_f64.powf(-self.channel_separation_db / 20.0);

        [
            [left_gain, crosstalk_gain * right_gain],
            [crosstalk_gain * left_gain, right_gain],
        ]
    }

    /// Returns the load-voltage ratio for a generator voltage at one frequency.
    pub fn loaded_voltage_ratio_at_hz(
        self,
        frequency_hz: f64,
    ) -> Result<f64, MovingMagnetCartridgeConfigError> {
        self.validate()?;
        validate_bounded("frequencyHz", frequency_hz, 0.0, f64::MAX)?;
        let ratio = loaded_voltage_ratio(
            self.coil_resistance_ohm,
            self.coil_inductance_h,
            self.load_resistance_ohm,
            self.load_capacitance_f,
            frequency_hz,
        );
        if !ratio.is_finite() {
            return Err(MovingMagnetCartridgeConfigError::InvalidDerivedValue {
                field: "loadedVoltageRatio",
            });
        }
        Ok(ratio)
    }
}

impl Default for MovingMagnetCartridgeConfig {
    fn default() -> Self {
        Self::concorde_mkii_scratch_seed()
    }
}

/// Reports the electrical and reciprocal mechanical state after one step.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovingMagnetCartridgeTelemetry {
    pub magnet_velocity_m_s: [f64; 2],
    pub generator_voltage_v: [f64; 2],
    pub coil_current_a: [f64; 2],
    pub load_output_voltage_v: [f64; 2],
    /// These values are the trapezoidal port averages for the completed interval.
    pub interval_average_generator_voltage_v: [f64; 2],
    pub interval_average_coil_current_a: [f64; 2],
    pub interval_average_load_output_voltage_v: [f64; 2],
    pub electromagnetic_reaction_force_n: [f64; 2],
    pub generator_electrical_power_w: f64,
    pub coil_loss_power_w: f64,
    pub load_power_w: f64,
    pub stored_electrical_energy_j: f64,
    pub completed_steps: u64,
}

impl Default for MovingMagnetCartridgeTelemetry {
    fn default() -> Self {
        Self {
            magnet_velocity_m_s: [0.0; 2],
            generator_voltage_v: [0.0; 2],
            coil_current_a: [0.0; 2],
            load_output_voltage_v: [0.0; 2],
            interval_average_generator_voltage_v: [0.0; 2],
            interval_average_coil_current_a: [0.0; 2],
            interval_average_load_output_voltage_v: [0.0; 2],
            electromagnetic_reaction_force_n: [0.0; 2],
            generator_electrical_power_w: 0.0,
            coil_loss_power_w: 0.0,
            load_power_w: 0.0,
            stored_electrical_energy_j: 0.0,
            completed_steps: 0,
        }
    }
}

/// Stores all values that affect later cartridge output.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovingMagnetCartridgeSnapshot {
    pub version: u32,
    pub config: MovingMagnetCartridgeConfig,
    pub coil_current_a: [f64; 2],
    pub load_output_voltage_v: [f64; 2],
    pub previous_generator_voltage_v: [f64; 2],
    pub last_magnet_velocity_m_s: [f64; 2],
    pub interval_average_generator_voltage_v: [f64; 2],
    pub interval_average_coil_current_a: [f64; 2],
    pub interval_average_load_output_voltage_v: [f64; 2],
    pub completed_steps: u64,
}

/// Describes one trapezoidal circuit step as an affine velocity relation.
///
/// Velocity and force use the two cartridge coil axes. The reciprocal damping
/// matrix is positive semidefinite for every valid cartridge configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovingMagnetCartridgeAffineStep {
    source: MovingMagnetCartridgeSnapshot,
    duration_seconds: f64,
    generator_voltage_per_velocity_v_s_per_m: [[f64; 2]; 2],
    current_bias_a: [f64; 2],
    current_per_generator_voltage_a_per_v: [f64; 2],
    load_voltage_bias_v: [f64; 2],
    load_voltage_per_generator_voltage: [f64; 2],
    reaction_force_bias_n: [f64; 2],
    reciprocal_damping_n_s_per_m: [[f64; 2]; 2],
}

/// Contains the evaluated values for one affine cartridge step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovingMagnetCartridgeAffineOutput {
    pub magnet_velocity_m_s: [f64; 2],
    pub generator_voltage_v: [f64; 2],
    pub coil_current_a: [f64; 2],
    pub load_output_voltage_v: [f64; 2],
    pub interval_average_generator_voltage_v: [f64; 2],
    pub interval_average_coil_current_a: [f64; 2],
    pub interval_average_load_output_voltage_v: [f64; 2],
    pub electromagnetic_reaction_force_n: [f64; 2],
}

impl MovingMagnetCartridgeAffineStep {
    pub const fn duration_seconds(self) -> f64 {
        self.duration_seconds
    }

    /// Returns the force at zero current-sample magnet velocity.
    pub const fn reaction_force_bias_n(self) -> [f64; 2] {
        self.reaction_force_bias_n
    }

    /// Returns the passive force coefficient in `force = bias - damping * velocity`.
    pub const fn reciprocal_damping_n_s_per_m(self) -> [[f64; 2]; 2] {
        self.reciprocal_damping_n_s_per_m
    }

    pub fn evaluate(
        self,
        magnet_velocity_m_s: [f64; 2],
    ) -> Result<MovingMagnetCartridgeAffineOutput, MovingMagnetCartridgeError> {
        validate_velocity(magnet_velocity_m_s)?;
        let generator_voltage_v = matrix_vector(
            self.generator_voltage_per_velocity_v_s_per_m,
            magnet_velocity_m_s,
        );
        let coil_current_a = [
            self.current_bias_a[0]
                + self.current_per_generator_voltage_a_per_v[0] * generator_voltage_v[0],
            self.current_bias_a[1]
                + self.current_per_generator_voltage_a_per_v[1] * generator_voltage_v[1],
        ];
        let load_output_voltage_v = [
            self.load_voltage_bias_v[0]
                + self.load_voltage_per_generator_voltage[0] * generator_voltage_v[0],
            self.load_voltage_bias_v[1]
                + self.load_voltage_per_generator_voltage[1] * generator_voltage_v[1],
        ];
        let interval_average_generator_voltage_v = [
            0.5 * (self.source.previous_generator_voltage_v[0] + generator_voltage_v[0]),
            0.5 * (self.source.previous_generator_voltage_v[1] + generator_voltage_v[1]),
        ];
        let interval_average_coil_current_a = [
            0.5 * (self.source.coil_current_a[0] + coil_current_a[0]),
            0.5 * (self.source.coil_current_a[1] + coil_current_a[1]),
        ];
        let interval_average_load_output_voltage_v = [
            0.5 * (self.source.load_output_voltage_v[0] + load_output_voltage_v[0]),
            0.5 * (self.source.load_output_voltage_v[1] + load_output_voltage_v[1]),
        ];
        let electromagnetic_reaction_force_n = subtract(
            self.reaction_force_bias_n,
            matrix_vector(self.reciprocal_damping_n_s_per_m, magnet_velocity_m_s),
        );
        if generator_voltage_v
            .into_iter()
            .chain(coil_current_a)
            .chain(load_output_voltage_v)
            .chain(interval_average_generator_voltage_v)
            .chain(interval_average_coil_current_a)
            .chain(interval_average_load_output_voltage_v)
            .chain(electromagnetic_reaction_force_n)
            .any(|value| !value.is_finite())
        {
            return Err(MovingMagnetCartridgeError::NumericalFailure);
        }
        Ok(MovingMagnetCartridgeAffineOutput {
            magnet_velocity_m_s,
            generator_voltage_v,
            coil_current_a,
            load_output_voltage_v,
            interval_average_generator_voltage_v,
            interval_average_coil_current_a,
            interval_average_load_output_voltage_v,
            electromagnetic_reaction_force_n,
        })
    }
}

/// Simulates two moving-magnet generators with identical loaded circuits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovingMagnetCartridge {
    config: MovingMagnetCartridgeConfig,
    coil_current_a: [f64; 2],
    load_output_voltage_v: [f64; 2],
    previous_generator_voltage_v: [f64; 2],
    last_magnet_velocity_m_s: [f64; 2],
    interval_average_generator_voltage_v: [f64; 2],
    interval_average_coil_current_a: [f64; 2],
    interval_average_load_output_voltage_v: [f64; 2],
    completed_steps: u64,
}

impl MovingMagnetCartridge {
    pub fn new(
        config: MovingMagnetCartridgeConfig,
    ) -> Result<Self, MovingMagnetCartridgeConfigError> {
        let config = config.validate()?;
        Ok(Self::from_valid_config(config))
    }

    fn from_valid_config(config: MovingMagnetCartridgeConfig) -> Self {
        Self {
            config,
            coil_current_a: [0.0; 2],
            load_output_voltage_v: [0.0; 2],
            previous_generator_voltage_v: [0.0; 2],
            last_magnet_velocity_m_s: [0.0; 2],
            interval_average_generator_voltage_v: [0.0; 2],
            interval_average_coil_current_a: [0.0; 2],
            interval_average_load_output_voltage_v: [0.0; 2],
            completed_steps: 0,
        }
    }

    pub fn config(&self) -> MovingMagnetCartridgeConfig {
        self.config
    }

    /// Prepares one fixed-size affine relation without changing circuit state.
    pub fn prepare_affine_step(
        &self,
        duration_seconds: f64,
    ) -> Result<MovingMagnetCartridgeAffineStep, MovingMagnetCartridgeError> {
        validate_duration(duration_seconds)?;
        let mut current_bias_a = [0.0; 2];
        let mut current_per_generator_voltage_a_per_v = [0.0; 2];
        let mut load_voltage_bias_v = [0.0; 2];
        let mut load_voltage_per_generator_voltage = [0.0; 2];
        for channel in 0..2 {
            let coefficients = trapezoidal_circuit_affine_step(
                self.config,
                duration_seconds,
                self.coil_current_a[channel],
                self.load_output_voltage_v[channel],
                self.previous_generator_voltage_v[channel],
            )
            .ok_or(MovingMagnetCartridgeError::NumericalFailure)?;
            current_bias_a[channel] = coefficients.current_bias_a;
            current_per_generator_voltage_a_per_v[channel] =
                coefficients.current_per_generator_voltage_a_per_v;
            load_voltage_bias_v[channel] = coefficients.load_voltage_bias_v;
            load_voltage_per_generator_voltage[channel] =
                coefficients.load_voltage_per_generator_voltage;
        }

        let channel_matrix = self.config.channel_matrix();
        let coefficient = self.config.generator_coefficient_v_s_per_m;
        let generator_voltage_per_velocity_v_s_per_m =
            channel_matrix.map(|row| [coefficient * row[0], coefficient * row[1]]);
        let mut reaction_force_bias_n = [0.0; 2];
        let mut reciprocal_damping_n_s_per_m = [[0.0; 2]; 2];
        for velocity_axis in 0..2 {
            for circuit in 0..2 {
                let interval_average_current_bias_a =
                    0.5 * (self.coil_current_a[circuit] + current_bias_a[circuit]);
                reaction_force_bias_n[velocity_axis] -= generator_voltage_per_velocity_v_s_per_m
                    [circuit][velocity_axis]
                    * interval_average_current_bias_a;
            }
            for other_axis in 0..2 {
                for circuit in 0..2 {
                    reciprocal_damping_n_s_per_m[velocity_axis][other_axis] +=
                        generator_voltage_per_velocity_v_s_per_m[circuit][velocity_axis]
                            * (0.5 * current_per_generator_voltage_a_per_v[circuit])
                            * generator_voltage_per_velocity_v_s_per_m[circuit][other_axis];
                }
            }
        }
        let mut off_diagonal =
            0.5 * (reciprocal_damping_n_s_per_m[0][1] + reciprocal_damping_n_s_per_m[1][0]);
        reciprocal_damping_n_s_per_m[0][1] = off_diagonal;
        reciprocal_damping_n_s_per_m[1][0] = off_diagonal;
        let maximum_passive_off_diagonal =
            reciprocal_damping_n_s_per_m[0][0].sqrt() * reciprocal_damping_n_s_per_m[1][1].sqrt();
        if off_diagonal.abs() > maximum_passive_off_diagonal {
            let excess = off_diagonal.abs() - maximum_passive_off_diagonal;
            let roundoff_tolerance =
                64.0 * f64::EPSILON * off_diagonal.abs().max(maximum_passive_off_diagonal);
            if excess > roundoff_tolerance {
                return Err(MovingMagnetCartridgeError::NumericalFailure);
            }
            off_diagonal = off_diagonal.signum() * maximum_passive_off_diagonal;
            reciprocal_damping_n_s_per_m[0][1] = off_diagonal;
            reciprocal_damping_n_s_per_m[1][0] = off_diagonal;
        }
        if generator_voltage_per_velocity_v_s_per_m
            .into_iter()
            .flatten()
            .chain(current_bias_a)
            .chain(current_per_generator_voltage_a_per_v)
            .chain(load_voltage_bias_v)
            .chain(load_voltage_per_generator_voltage)
            .chain(reaction_force_bias_n)
            .chain(reciprocal_damping_n_s_per_m.into_iter().flatten())
            .any(|value| !value.is_finite())
            || reciprocal_damping_n_s_per_m[0][0] < 0.0
            || reciprocal_damping_n_s_per_m[1][1] < 0.0
        {
            return Err(MovingMagnetCartridgeError::NumericalFailure);
        }

        Ok(MovingMagnetCartridgeAffineStep {
            source: self.snapshot(),
            duration_seconds,
            generator_voltage_per_velocity_v_s_per_m,
            current_bias_a,
            current_per_generator_voltage_a_per_v,
            load_voltage_bias_v,
            load_voltage_per_generator_voltage,
            reaction_force_bias_n,
            reciprocal_damping_n_s_per_m,
        })
    }

    /// Commits a prepared affine relation for its solved magnet velocity.
    pub fn commit_affine_step(
        &mut self,
        step: MovingMagnetCartridgeAffineStep,
        magnet_velocity_m_s: [f64; 2],
    ) -> Result<MovingMagnetCartridgeTelemetry, MovingMagnetCartridgeError> {
        if self.snapshot() != step.source {
            return Err(MovingMagnetCartridgeError::StaleAffineStep);
        }
        let completed_steps = self
            .completed_steps
            .checked_add(1)
            .ok_or(MovingMagnetCartridgeError::StepCounterOverflow)?;
        let output = step.evaluate(magnet_velocity_m_s)?;
        self.coil_current_a = output.coil_current_a;
        self.load_output_voltage_v = output.load_output_voltage_v;
        self.previous_generator_voltage_v = output.generator_voltage_v;
        self.last_magnet_velocity_m_s = output.magnet_velocity_m_s;
        self.interval_average_generator_voltage_v = output.interval_average_generator_voltage_v;
        self.interval_average_coil_current_a = output.interval_average_coil_current_a;
        self.interval_average_load_output_voltage_v = output.interval_average_load_output_voltage_v;
        self.completed_steps = completed_steps;
        Ok(self.telemetry())
    }

    /// Advances the circuit by one interval.
    /// The input values are magnet velocities along the two coil axes.
    pub fn advance(
        &mut self,
        duration_seconds: f64,
        magnet_velocity_m_s: [f64; 2],
    ) -> Result<MovingMagnetCartridgeTelemetry, MovingMagnetCartridgeError> {
        let step = self.prepare_affine_step(duration_seconds)?;
        self.commit_affine_step(step, magnet_velocity_m_s)
    }

    pub fn telemetry(&self) -> MovingMagnetCartridgeTelemetry {
        let matrix = self.config.channel_matrix();
        let coefficient = self.config.generator_coefficient_v_s_per_m;
        let reaction_force = [
            -coefficient
                * (matrix[0][0] * self.interval_average_coil_current_a[0]
                    + matrix[1][0] * self.interval_average_coil_current_a[1]),
            -coefficient
                * (matrix[0][1] * self.interval_average_coil_current_a[0]
                    + matrix[1][1] * self.interval_average_coil_current_a[1]),
        ];
        let generator_power = dot(
            self.interval_average_generator_voltage_v,
            self.interval_average_coil_current_a,
        );
        let coil_loss = self.config.coil_resistance_ohm
            * self
                .interval_average_coil_current_a
                .into_iter()
                .map(|value| value * value)
                .sum::<f64>();
        let load_power = self
            .interval_average_load_output_voltage_v
            .into_iter()
            .map(|value| value * value / self.config.load_resistance_ohm)
            .sum();
        let stored_energy = 0.5
            * self.config.coil_inductance_h
            * self
                .coil_current_a
                .into_iter()
                .map(|value| value * value)
                .sum::<f64>()
            + 0.5
                * self.config.load_capacitance_f
                * self
                    .load_output_voltage_v
                    .into_iter()
                    .map(|value| value * value)
                    .sum::<f64>();

        MovingMagnetCartridgeTelemetry {
            magnet_velocity_m_s: self.last_magnet_velocity_m_s,
            generator_voltage_v: self.previous_generator_voltage_v,
            coil_current_a: self.coil_current_a,
            load_output_voltage_v: self.load_output_voltage_v,
            interval_average_generator_voltage_v: self.interval_average_generator_voltage_v,
            interval_average_coil_current_a: self.interval_average_coil_current_a,
            interval_average_load_output_voltage_v: self.interval_average_load_output_voltage_v,
            electromagnetic_reaction_force_n: reaction_force,
            generator_electrical_power_w: generator_power,
            coil_loss_power_w: coil_loss,
            load_power_w: load_power,
            stored_electrical_energy_j: stored_energy,
            completed_steps: self.completed_steps,
        }
    }

    pub fn output_voltage_v(&self) -> [f64; 2] {
        self.load_output_voltage_v
    }

    pub fn reset(&mut self) {
        self.coil_current_a = [0.0; 2];
        self.load_output_voltage_v = [0.0; 2];
        self.previous_generator_voltage_v = [0.0; 2];
        self.last_magnet_velocity_m_s = [0.0; 2];
        self.interval_average_generator_voltage_v = [0.0; 2];
        self.interval_average_coil_current_a = [0.0; 2];
        self.interval_average_load_output_voltage_v = [0.0; 2];
        self.completed_steps = 0;
    }

    pub fn snapshot(&self) -> MovingMagnetCartridgeSnapshot {
        MovingMagnetCartridgeSnapshot {
            version: SNAPSHOT_VERSION,
            config: self.config,
            coil_current_a: self.coil_current_a,
            load_output_voltage_v: self.load_output_voltage_v,
            previous_generator_voltage_v: self.previous_generator_voltage_v,
            last_magnet_velocity_m_s: self.last_magnet_velocity_m_s,
            interval_average_generator_voltage_v: self.interval_average_generator_voltage_v,
            interval_average_coil_current_a: self.interval_average_coil_current_a,
            interval_average_load_output_voltage_v: self.interval_average_load_output_voltage_v,
            completed_steps: self.completed_steps,
        }
    }

    pub fn restore(
        &mut self,
        snapshot: MovingMagnetCartridgeSnapshot,
    ) -> Result<(), MovingMagnetCartridgeError> {
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(MovingMagnetCartridgeError::UnsupportedSnapshotVersion {
                version: snapshot.version,
            });
        }
        let config = snapshot.config.validate()?;
        for values in [
            snapshot.coil_current_a,
            snapshot.load_output_voltage_v,
            snapshot.previous_generator_voltage_v,
            snapshot.last_magnet_velocity_m_s,
            snapshot.interval_average_generator_voltage_v,
            snapshot.interval_average_coil_current_a,
            snapshot.interval_average_load_output_voltage_v,
        ] {
            if values.into_iter().any(|value| !value.is_finite()) {
                return Err(MovingMagnetCartridgeError::InvalidSnapshot);
            }
        }
        let voltage_matrix = config.channel_matrix().map(|row| {
            [
                config.generator_coefficient_v_s_per_m * row[0],
                config.generator_coefficient_v_s_per_m * row[1],
            ]
        });
        let expected_generator_voltage_v =
            matrix_vector(voltage_matrix, snapshot.last_magnet_velocity_m_s);
        if !vectors_nearly_equal(
            snapshot.previous_generator_voltage_v,
            expected_generator_voltage_v,
        ) {
            return Err(MovingMagnetCartridgeError::InvalidSnapshot);
        }
        let stored_energy_j = 0.5
            * config.coil_inductance_h
            * snapshot
                .coil_current_a
                .into_iter()
                .map(|value| value * value)
                .sum::<f64>()
            + 0.5
                * config.load_capacitance_f
                * snapshot
                    .load_output_voltage_v
                    .into_iter()
                    .map(|value| value * value)
                    .sum::<f64>();
        let generator_power_w = dot(
            snapshot.interval_average_generator_voltage_v,
            snapshot.interval_average_coil_current_a,
        );
        let coil_loss_power_w = config.coil_resistance_ohm
            * snapshot
                .interval_average_coil_current_a
                .into_iter()
                .map(|value| value * value)
                .sum::<f64>();
        let load_power_w = snapshot
            .interval_average_load_output_voltage_v
            .into_iter()
            .map(|value| value * value / config.load_resistance_ohm)
            .sum::<f64>();
        if [
            stored_energy_j,
            generator_power_w,
            coil_loss_power_w,
            load_power_w,
        ]
        .into_iter()
        .any(|value| !value.is_finite())
        {
            return Err(MovingMagnetCartridgeError::InvalidSnapshot);
        }

        self.config = config;
        self.coil_current_a = snapshot.coil_current_a;
        self.load_output_voltage_v = snapshot.load_output_voltage_v;
        self.previous_generator_voltage_v = snapshot.previous_generator_voltage_v;
        self.last_magnet_velocity_m_s = snapshot.last_magnet_velocity_m_s;
        self.interval_average_generator_voltage_v = snapshot.interval_average_generator_voltage_v;
        self.interval_average_coil_current_a = snapshot.interval_average_coil_current_a;
        self.interval_average_load_output_voltage_v =
            snapshot.interval_average_load_output_voltage_v;
        self.completed_steps = snapshot.completed_steps;
        Ok(())
    }
}

impl Default for MovingMagnetCartridge {
    fn default() -> Self {
        Self::from_valid_config(MovingMagnetCartridgeConfig::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum MovingMagnetCartridgeConfigError {
    #[error("{field} must be finite and at least {minimum}")]
    BelowMinimum { field: &'static str, minimum: f64 },
    #[error("{field} must be finite and at most {maximum}")]
    AboveMaximum { field: &'static str, maximum: f64 },
    #[error("the derived {field} value is invalid")]
    InvalidDerivedValue { field: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum MovingMagnetCartridgeError {
    #[error(transparent)]
    InvalidConfig(#[from] MovingMagnetCartridgeConfigError),
    #[error("durationSeconds must be finite, positive, and at most 10 seconds")]
    InvalidDuration,
    #[error("magnet velocity for channel {channel} must be finite")]
    InvalidMagnetVelocity { channel: usize },
    #[error("the cartridge circuit produced an invalid numerical result")]
    NumericalFailure,
    #[error("the prepared cartridge step no longer matches the circuit state")]
    StaleAffineStep,
    #[error("the cartridge step counter exceeded its supported range")]
    StepCounterOverflow,
    #[error("snapshot state is invalid")]
    InvalidSnapshot,
    #[error("snapshot version {version} is unsupported")]
    UnsupportedSnapshotVersion { version: u32 },
}

#[derive(Debug, Clone, Copy)]
struct TrapezoidalCircuitAffineStep {
    current_bias_a: f64,
    current_per_generator_voltage_a_per_v: f64,
    load_voltage_bias_v: f64,
    load_voltage_per_generator_voltage: f64,
}

fn trapezoidal_circuit_affine_step(
    config: MovingMagnetCartridgeConfig,
    duration_seconds: f64,
    previous_current_a: f64,
    previous_load_voltage_v: f64,
    previous_generator_voltage_v: f64,
) -> Option<TrapezoidalCircuitAffineStep> {
    let half_step = 0.5 * duration_seconds;
    let resistance_over_inductance = config.coil_resistance_ohm / config.coil_inductance_h;
    let inverse_inductance = 1.0 / config.coil_inductance_h;
    let inverse_capacitance = 1.0 / config.load_capacitance_f;
    let inverse_load_time_constant = 1.0 / (config.load_resistance_ohm * config.load_capacitance_f);

    // Solve the two-state trapezoidal update without an iterative solver.
    let left_00 = 1.0 + half_step * resistance_over_inductance;
    let left_01 = half_step * inverse_inductance;
    let left_10 = -half_step * inverse_capacitance;
    let left_11 = 1.0 + half_step * inverse_load_time_constant;

    let right_0_bias = (1.0 - half_step * resistance_over_inductance) * previous_current_a
        - half_step * inverse_inductance * previous_load_voltage_v
        + half_step * inverse_inductance * previous_generator_voltage_v;
    let right_0_per_generator_voltage = half_step * inverse_inductance;
    let right_1 = half_step * inverse_capacitance * previous_current_a
        + (1.0 - half_step * inverse_load_time_constant) * previous_load_voltage_v;

    let determinant = left_00 * left_11 - left_01 * left_10;
    if !determinant.is_finite() || determinant <= 0.0 {
        return None;
    }
    let current_bias_a = (right_0_bias * left_11 - left_01 * right_1) / determinant;
    let current_per_generator_voltage_a_per_v =
        right_0_per_generator_voltage * left_11 / determinant;
    let load_voltage_bias_v = (left_00 * right_1 - left_10 * right_0_bias) / determinant;
    let load_voltage_per_generator_voltage = -left_10 * right_0_per_generator_voltage / determinant;
    if [
        current_bias_a,
        current_per_generator_voltage_a_per_v,
        load_voltage_bias_v,
        load_voltage_per_generator_voltage,
    ]
    .into_iter()
    .any(|value| !value.is_finite())
        || current_per_generator_voltage_a_per_v < 0.0
        || load_voltage_per_generator_voltage < 0.0
    {
        return None;
    }
    Some(TrapezoidalCircuitAffineStep {
        current_bias_a,
        current_per_generator_voltage_a_per_v,
        load_voltage_bias_v,
        load_voltage_per_generator_voltage,
    })
}

fn validate_duration(duration_seconds: f64) -> Result<(), MovingMagnetCartridgeError> {
    if duration_seconds.is_finite()
        && duration_seconds > 0.0
        && duration_seconds <= MAX_ADVANCE_SECONDS
    {
        Ok(())
    } else {
        Err(MovingMagnetCartridgeError::InvalidDuration)
    }
}

fn validate_velocity(magnet_velocity_m_s: [f64; 2]) -> Result<(), MovingMagnetCartridgeError> {
    for (channel, velocity) in magnet_velocity_m_s.into_iter().enumerate() {
        if !velocity.is_finite() {
            return Err(MovingMagnetCartridgeError::InvalidMagnetVelocity { channel });
        }
    }
    Ok(())
}

fn matrix_vector(matrix: [[f64; 2]; 2], vector: [f64; 2]) -> [f64; 2] {
    [
        matrix[0][0] * vector[0] + matrix[0][1] * vector[1],
        matrix[1][0] * vector[0] + matrix[1][1] * vector[1],
    ]
}

fn subtract(left: [f64; 2], right: [f64; 2]) -> [f64; 2] {
    [left[0] - right[0], left[1] - right[1]]
}

fn loaded_voltage_ratio(
    coil_resistance_ohm: f64,
    coil_inductance_h: f64,
    load_resistance_ohm: f64,
    load_capacitance_f: f64,
    frequency_hz: f64,
) -> f64 {
    let angular_frequency = std::f64::consts::TAU * frequency_hz;
    let load_conductance = 1.0 / load_resistance_ohm;
    let load_susceptance = angular_frequency * load_capacitance_f;
    let admittance_magnitude_squared =
        load_conductance * load_conductance + load_susceptance * load_susceptance;
    let load_real = load_conductance / admittance_magnitude_squared;
    let load_imaginary = -load_susceptance / admittance_magnitude_squared;
    let total_real = coil_resistance_ohm + load_real;
    let total_imaginary = angular_frequency * coil_inductance_h + load_imaginary;
    load_real.hypot(load_imaginary) / total_real.hypot(total_imaginary)
}

fn dot(left: [f64; 2], right: [f64; 2]) -> f64 {
    left[0] * right[0] + left[1] * right[1]
}

fn vectors_nearly_equal(left: [f64; 2], right: [f64; 2]) -> bool {
    (0..2).all(|axis| {
        let scale = left[axis].abs().max(right[axis].abs()).max(1.0e-30);
        (left[axis] - right[axis]).abs() <= 32.0 * f64::EPSILON * scale + 1.0e-30
    })
}

fn validate_bounded_positive(
    field: &'static str,
    value: f64,
    minimum: f64,
    maximum: f64,
) -> Result<(), MovingMagnetCartridgeConfigError> {
    validate_bounded(field, value, minimum, maximum)
}

fn validate_bounded(
    field: &'static str,
    value: f64,
    minimum: f64,
    maximum: f64,
) -> Result<(), MovingMagnetCartridgeConfigError> {
    if !value.is_finite() || value < minimum {
        return Err(MovingMagnetCartridgeConfigError::BelowMinimum { field, minimum });
    }
    if value > maximum {
        return Err(MovingMagnetCartridgeConfigError::AboveMaximum { field, maximum });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    const TEST_SAMPLE_RATE_HZ: f64 = 192_000.0;

    #[test]
    fn seed_uses_published_values_and_a_derived_generator_coefficient() {
        let config = MovingMagnetCartridgeConfig::concorde_mkii_scratch_seed();
        assert_eq!(
            config.generator_coefficient_source,
            GeneratorCoefficientSource::DerivedFromLoadedOutputSpecification
        );
        assert_eq!(config.coil_resistance_ohm, 1_200.0);
        assert_eq!(config.coil_inductance_h, 0.850);
        assert_eq!(config.load_resistance_ohm, 47_000.0);
        assert_eq!(config.load_capacitance_f, 300.0e-12);
        assert_eq!(config.channel_separation_db, 22.0);
        assert_eq!(config.channel_balance_db, 1.0);
        assert_relative_eq!(
            config.generator_coefficient_v_s_per_m,
            0.01 / (0.05 * config.loaded_voltage_ratio_at_hz(1_000.0).unwrap()),
            epsilon = 1.0e-12
        );
    }

    #[test]
    fn loaded_one_kilohertz_output_matches_the_nominal_specification() {
        let left_rms = measure_direct_output_rms(0);
        let right_rms = measure_direct_output_rms(1);
        let nominal_rms = (left_rms * right_rms).sqrt();
        assert_relative_eq!(nominal_rms, 10.0e-3, max_relative = 2.0e-3);
        assert_relative_eq!(20.0 * (left_rms / right_rms).log10(), 1.0, epsilon = 1.0e-6);
    }

    #[test]
    fn reversed_velocity_reverses_all_signed_electrical_values() {
        let mut positive = MovingMagnetCartridge::default();
        let mut negative = MovingMagnetCartridge::default();
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let mut phase = 0.0;
        let phase_step = std::f64::consts::TAU * 1_337.0 / TEST_SAMPLE_RATE_HZ;

        for _ in 0..10_000 {
            let velocity = 0.08 * f64::sin(phase);
            let a = positive.advance(dt, [velocity, -0.3 * velocity]).unwrap();
            let b = negative.advance(dt, [-velocity, 0.3 * velocity]).unwrap();
            for channel in 0..2 {
                assert_relative_eq!(
                    a.generator_voltage_v[channel],
                    -b.generator_voltage_v[channel],
                    epsilon = 1.0e-14
                );
                assert_relative_eq!(
                    a.coil_current_a[channel],
                    -b.coil_current_a[channel],
                    epsilon = 1.0e-14
                );
                assert_relative_eq!(
                    a.load_output_voltage_v[channel],
                    -b.load_output_voltage_v[channel],
                    epsilon = 1.0e-14
                );
                assert_relative_eq!(
                    a.electromagnetic_reaction_force_n[channel],
                    -b.electromagnetic_reaction_force_n[channel],
                    epsilon = 1.0e-14
                );
            }
            phase += phase_step;
        }
    }

    #[test]
    fn electromagnetic_force_is_reciprocal_with_generator_power() {
        let mut cartridge = MovingMagnetCartridge::default();
        let velocity = [0.071, -0.043];
        let telemetry = cartridge
            .advance(1.0 / TEST_SAMPLE_RATE_HZ, velocity)
            .unwrap();
        let interval_average_velocity = [0.5 * velocity[0], 0.5 * velocity[1]];
        let mechanical_power = dot(
            telemetry.electromagnetic_reaction_force_n,
            interval_average_velocity,
        );
        assert_relative_eq!(
            mechanical_power,
            -telemetry.generator_electrical_power_w,
            epsilon = 1.0e-18
        );
    }

    #[test]
    fn affine_step_is_reciprocal_and_matches_standalone_advance() {
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let mut standalone = MovingMagnetCartridge::default();
        for sample in 0..2_000 {
            let phase = std::f64::consts::TAU * sample as f64 / 83.25;
            standalone
                .advance(dt, [0.04 * phase.sin(), -0.03 * phase.cos()])
                .unwrap();
        }
        let mut affine_cartridge = standalone;
        let velocity = [0.071, -0.043];
        let affine = affine_cartridge.prepare_affine_step(dt).unwrap();
        let evaluated = affine.evaluate(velocity).unwrap();
        let previous_velocity = affine.source.last_magnet_velocity_m_s;
        let interval_average_velocity = [
            0.5 * (previous_velocity[0] + velocity[0]),
            0.5 * (previous_velocity[1] + velocity[1]),
        ];
        let mechanical_power = dot(
            evaluated.electromagnetic_reaction_force_n,
            interval_average_velocity,
        );
        let generator_power = dot(
            evaluated.interval_average_generator_voltage_v,
            evaluated.interval_average_coil_current_a,
        );
        let scale = mechanical_power
            .abs()
            .max(generator_power.abs())
            .max(1.0e-24);
        assert!((mechanical_power + generator_power).abs() <= 1.0e-10 * scale + 1.0e-18);

        let expected = standalone.advance(dt, velocity).unwrap();
        let actual = affine_cartridge
            .commit_affine_step(affine, velocity)
            .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(affine_cartridge.snapshot(), standalone.snapshot());
    }

    #[test]
    fn reciprocal_affine_damping_cannot_supply_power() {
        let cartridge = MovingMagnetCartridge::default();
        let affine = cartridge
            .prepare_affine_step(1.0 / TEST_SAMPLE_RATE_HZ)
            .unwrap();
        let damping = affine.reciprocal_damping_n_s_per_m();
        assert_eq!(damping[0][1], damping[1][0]);
        for velocity in [[1.0, 0.0], [0.0, 1.0], [1.0, -2.0], [-17.0, 31.0]] {
            assert!(dot(velocity, matrix_vector(damping, velocity)) >= 0.0);
        }
    }

    #[test]
    fn trapezoidal_port_closes_the_energy_balance_during_sample_reversals() {
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let mut cartridge = MovingMagnetCartridge::default();
        let mut previous_energy_j = cartridge.telemetry().stored_electrical_energy_j;
        for sample in 0..20_000 {
            let polarity = if sample & 1 == 0 { 1.0 } else { -1.0 };
            let phase = sample as f64 * 0.371;
            let velocity = [
                polarity * (0.15 + 0.11 * phase.sin()),
                -polarity * (0.09 + 0.07 * (phase * 1.73).cos()),
            ];
            let telemetry = cartridge.advance(dt, velocity).unwrap();
            let energy_change_j = telemetry.stored_electrical_energy_j - previous_energy_j;
            let port_energy_j = dt
                * (telemetry.generator_electrical_power_w
                    - telemetry.coil_loss_power_w
                    - telemetry.load_power_w);
            let scale = energy_change_j.abs().max(port_energy_j.abs()).max(1.0e-30);
            assert!(
                (energy_change_j - port_energy_j).abs() <= 2.0e-12 * scale + 1.0e-27,
                "sample {sample}: {energy_change_j} != {port_energy_j}"
            );
            previous_energy_j = telemetry.stored_electrical_energy_j;
        }
    }

    #[test]
    fn stale_and_overflowing_affine_commits_are_transactional() {
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let mut cartridge = MovingMagnetCartridge::default();
        let stale = cartridge.prepare_affine_step(dt).unwrap();
        cartridge.advance(dt, [0.01, -0.02]).unwrap();
        let before_stale = cartridge.snapshot();
        assert_eq!(
            cartridge.commit_affine_step(stale, [0.0; 2]),
            Err(MovingMagnetCartridgeError::StaleAffineStep)
        );
        assert_eq!(cartridge.snapshot(), before_stale);

        let mut overflow_snapshot = cartridge.snapshot();
        overflow_snapshot.completed_steps = u64::MAX;
        cartridge.restore(overflow_snapshot).unwrap();
        let step = cartridge.prepare_affine_step(dt).unwrap();
        let before_overflow = cartridge.snapshot();
        assert_eq!(
            cartridge.commit_affine_step(step, [0.0; 2]),
            Err(MovingMagnetCartridgeError::StepCounterOverflow)
        );
        assert_eq!(cartridge.snapshot(), before_overflow);

        let mut extreme_snapshot = MovingMagnetCartridge::default().snapshot();
        extreme_snapshot.coil_current_a = [f64::MAX, -f64::MAX];
        extreme_snapshot.load_output_voltage_v = [f64::MAX, -f64::MAX];
        extreme_snapshot.previous_generator_voltage_v = [f64::MAX, -f64::MAX];
        assert_eq!(
            cartridge.restore(extreme_snapshot),
            Err(MovingMagnetCartridgeError::InvalidSnapshot)
        );
        assert_eq!(cartridge.snapshot(), before_overflow);
    }

    #[test]
    fn electrical_state_decays_after_motion_stops() {
        let mut cartridge = MovingMagnetCartridge::default();
        let dt = 1.0 / 48_000.0;
        for _ in 0..4_800 {
            cartridge.advance(dt, [0.05, -0.02]).unwrap();
        }
        let excited_energy = cartridge.telemetry().stored_electrical_energy_j;
        assert!(excited_energy > 0.0);

        for _ in 0..24_000 {
            cartridge.advance(dt, [0.0, 0.0]).unwrap();
        }
        let settled = cartridge.telemetry();
        assert!(settled.stored_electrical_energy_j < excited_energy * 1.0e-12);
        assert!(settled.load_output_voltage_v[0].abs() < 1.0e-12);
        assert!(settled.load_output_voltage_v[1].abs() < 1.0e-12);
    }

    #[test]
    fn trapezoidal_state_remains_finite_during_a_full_band_sweep() {
        let sample_rate_hz = 96_000.0;
        let dt = 1.0 / sample_rate_hz;
        let duration_seconds = 2.0;
        let sample_count = (sample_rate_hz * duration_seconds) as usize;
        let start_hz: f64 = 10.0;
        let end_hz: f64 = 40_000.0;
        let frequency_ratio = end_hz / start_hz;
        let mut phase = 0.0;
        let mut cartridge = MovingMagnetCartridge::default();
        let mut maximum_output: f64 = 0.0;

        for index in 0..sample_count {
            let progress = index as f64 / sample_count as f64;
            let frequency = start_hz * frequency_ratio.powf(progress);
            phase += std::f64::consts::TAU * frequency * dt;
            let velocity = 0.25 * phase.sin();
            let telemetry = cartridge.advance(dt, [velocity, -0.7 * velocity]).unwrap();
            for value in telemetry
                .generator_voltage_v
                .into_iter()
                .chain(telemetry.coil_current_a)
                .chain(telemetry.load_output_voltage_v)
                .chain(telemetry.electromagnetic_reaction_force_n)
            {
                assert!(value.is_finite());
            }
            maximum_output = maximum_output.max(telemetry.load_output_voltage_v[0].abs());
        }

        assert!(maximum_output > 1.0e-3);
        assert!(maximum_output < 1.0);
    }

    #[test]
    fn channel_leakage_matches_the_published_separation() {
        let mut cartridge = MovingMagnetCartridge::default();
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let mut left_sum = 0.0;
        let mut right_sum = 0.0;
        let mut phase: f64 = 0.0;
        let phase_step = std::f64::consts::TAU * 1_000.0 / TEST_SAMPLE_RATE_HZ;

        for index in 0..TEST_SAMPLE_RATE_HZ as usize {
            let velocity = 0.05 * 2.0_f64.sqrt() * phase.sin();
            let output = cartridge.advance(dt, [velocity, 0.0]).unwrap();
            if index >= TEST_SAMPLE_RATE_HZ as usize / 2 {
                left_sum += output.load_output_voltage_v[0].powi(2);
                right_sum += output.load_output_voltage_v[1].powi(2);
            }
            phase += phase_step;
        }

        let separation_db = 10.0 * (left_sum / right_sum).log10();
        assert_relative_eq!(separation_db, 22.0, epsilon = 1.0e-8);
    }

    #[test]
    fn invalid_input_does_not_put_nan_in_the_state() {
        let mut cartridge = MovingMagnetCartridge::default();
        let before = cartridge.snapshot();
        assert_eq!(
            cartridge.advance(1.0 / 48_000.0, [f64::NAN, 0.0]),
            Err(MovingMagnetCartridgeError::InvalidMagnetVelocity { channel: 0 })
        );
        assert_eq!(cartridge.snapshot(), before);

        let mut invalid_snapshot = before;
        invalid_snapshot.coil_current_a[1] = f64::NAN;
        assert_eq!(
            cartridge.restore(invalid_snapshot),
            Err(MovingMagnetCartridgeError::InvalidSnapshot)
        );
        assert_eq!(cartridge.snapshot(), before);
    }

    #[test]
    fn snapshot_restore_continues_with_identical_output() {
        let mut original = MovingMagnetCartridge::default();
        let dt = 1.0 / 48_000.0;
        for index in 0..3_000 {
            let velocity = 0.04 * (index as f64 * 0.031).sin();
            original.advance(dt, [velocity, -velocity]).unwrap();
        }

        let snapshot = original.snapshot();
        let mut restored = MovingMagnetCartridge::default();
        restored.restore(snapshot).unwrap();
        for index in 0..2_000 {
            let velocity = 0.07 * (index as f64 * 0.017).cos();
            let expected = original.advance(dt, [velocity, 0.25 * velocity]).unwrap();
            let actual = restored.advance(dt, [velocity, 0.25 * velocity]).unwrap();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn reset_clears_each_dynamic_value() {
        let mut cartridge = MovingMagnetCartridge::default();
        cartridge.advance(1.0 / 48_000.0, [0.1, -0.2]).unwrap();
        cartridge.reset();
        assert_eq!(
            cartridge.telemetry(),
            MovingMagnetCartridgeTelemetry::default()
        );
        assert_eq!(cartridge.output_voltage_v(), [0.0; 2]);
    }

    #[test]
    fn configuration_rejects_nonphysical_and_nonfinite_values() {
        let config = MovingMagnetCartridgeConfig {
            coil_inductance_h: 0.0,
            ..MovingMagnetCartridgeConfig::default()
        };
        assert!(config.validate().is_err());

        let config = MovingMagnetCartridgeConfig {
            load_capacitance_f: f64::INFINITY,
            ..MovingMagnetCartridgeConfig::default()
        };
        assert!(config.validate().is_err());

        let config = MovingMagnetCartridgeConfig {
            channel_balance_db: f64::NAN,
            ..MovingMagnetCartridgeConfig::default()
        };
        assert!(config.validate().is_err());
    }

    fn measure_direct_output_rms(channel: usize) -> f64 {
        let mut cartridge = MovingMagnetCartridge::default();
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let mut sum = 0.0;
        let mut count = 0;
        let mut phase: f64 = 0.0;
        let phase_step = std::f64::consts::TAU * 1_000.0 / TEST_SAMPLE_RATE_HZ;
        for index in 0..TEST_SAMPLE_RATE_HZ as usize {
            let velocity = 0.05 * 2.0_f64.sqrt() * phase.sin();
            let mut input = [0.0; 2];
            input[channel] = velocity;
            let output = cartridge.advance(dt, input).unwrap();
            if index >= TEST_SAMPLE_RATE_HZ as usize / 2 {
                sum += output.load_output_voltage_v[channel].powi(2);
                count += 1;
            }
            phase += phase_step;
        }
        (sum / count as f64).sqrt()
    }
}
