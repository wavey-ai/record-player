use serde::{Deserialize, Serialize};
use thiserror::Error;

const SNAPSHOT_VERSION: u32 = 4;
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
const INVERSE_SQRT_2: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// Sets the fixed storage limit for passive magnetic-loss relaxation branches.
pub const MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES: usize = 4;

/// Identifies the source of the generator coefficient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GeneratorCoefficientSource {
    DirectMeasurement,
    DerivedFromLoadedOutputSpecification,
    UserSupplied,
}

/// Contains one complex voltage transfer ratio.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartridgeComplexVoltageRatio {
    pub real: f64,
    pub imaginary: f64,
}

/// Contains one complex cartridge-coil impedance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartridgeComplexImpedanceOhm {
    pub resistance_ohm: f64,
    pub reactance_ohm: f64,
}

impl CartridgeComplexImpedanceOhm {
    pub fn magnitude_ohm(self) -> f64 {
        self.resistance_ohm.hypot(self.reactance_ohm)
    }

    pub fn phase_radians(self) -> f64 {
        self.reactance_ohm.atan2(self.resistance_ohm)
    }
}

/// Defines one passive series relaxation term as a parallel resistor and inductor.
///
/// Set both values to zero to disable the slot. Active slots must be contiguous.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MagneticLossRelaxationBranchConfig {
    pub relaxation_inductance_h: f64,
    pub loss_resistance_ohm: f64,
}

impl MagneticLossRelaxationBranchConfig {
    const fn is_inactive(self) -> bool {
        self.relaxation_inductance_h == 0.0 && self.loss_resistance_ohm == 0.0
    }
}

impl CartridgeComplexVoltageRatio {
    pub fn magnitude(self) -> f64 {
        self.real.hypot(self.imaginary)
    }

    pub fn phase_radians(self) -> f64 {
        self.imaginary.atan2(self.real)
    }
}

/// Reports the loaded circuit response before generator-axis mixing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovingMagnetCartridgeCircuitFrequencyResponse {
    /// Rows select coil voltage outputs. Columns select coil currents.
    pub coil_impedance_ohm: [[CartridgeComplexImpedanceOhm; 2]; 2],
    /// Rows select load outputs. Columns select generator voltage sources.
    pub loaded_voltage_per_generator_voltage: [[CartridgeComplexVoltageRatio; 2]; 2],
    /// Rows select load outputs. Columns select magnet-velocity axes.
    pub loaded_voltage_per_magnet_velocity_v_s_per_m: [[CartridgeComplexVoltageRatio; 2]; 2],
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
    /// Couples the two coil flux linkages.
    ///
    /// The signed value selects the winding polarity. Its magnitude must keep
    /// both common-mode and differential-mode inductances positive.
    /// A total-separation measurement cannot identify this value separately from generator leakage.
    #[serde(default)]
    pub coil_mutual_inductance_h: f64,
    /// Adds passive, frequency-dependent magnetic loss to both coil circuits.
    ///
    /// Each active slot is a series `R || L` term. Slots use increasing
    /// relaxation time so that one physical network has one representation.
    #[serde(default)]
    pub magnetic_loss_branches:
        [MagneticLossRelaxationBranchConfig; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES],
    pub load_resistance_ohm: f64,
    pub load_capacitance_f: f64,
    /// Sets the magnitude difference between the two direct channel gains.
    /// A positive value makes the left direct gain larger.
    pub channel_balance_db: f64,
    /// Sets the direct-to-crosstalk voltage ratio for each driven axis.
    /// Electrical mutual inductance also changes the loaded channel separation.
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

        // Ortofon specifies 10 mV at 1 kHz and 5 cm/s.
        // The source does not identify peak or RMS conventions for both values.
        // This seed preserves the former numeric scale and assumes 5 cm/s RMS.
        // This value refers to the nominal channel before balance variation.
        let generator_coefficient_v_s_per_m = 10.0e-3 / (0.05 * loaded_ratio);

        Self {
            generator_coefficient_v_s_per_m,
            generator_coefficient_source:
                GeneratorCoefficientSource::DerivedFromLoadedOutputSpecification,
            coil_resistance_ohm,
            coil_inductance_h,
            // Ortofon does not publish this value. Keep the seed uncoupled.
            coil_mutual_inductance_h: 0.0,
            // No source publishes magnetic-loss branch values for this cartridge.
            magnetic_loss_branches: [MagneticLossRelaxationBranchConfig::default();
                MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES],
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
        let mut relaxation_inductance_sum_h = 0.0;
        let mut found_inactive_branch = false;
        let mut previous_relaxation_time_seconds = None;
        for (branch, parameters) in self.magnetic_loss_branches.into_iter().enumerate() {
            if parameters.is_inactive() {
                found_inactive_branch = true;
                continue;
            }
            if parameters.relaxation_inductance_h == 0.0 || parameters.loss_resistance_ohm == 0.0 {
                return Err(
                    MovingMagnetCartridgeConfigError::IncompleteMagneticLossBranch { branch },
                );
            }
            if found_inactive_branch {
                return Err(
                    MovingMagnetCartridgeConfigError::NonContiguousMagneticLossBranch { branch },
                );
            }
            if !parameters.relaxation_inductance_h.is_finite()
                || parameters.relaxation_inductance_h < MIN_INDUCTANCE_H
                || parameters.relaxation_inductance_h > MAX_INDUCTANCE_H
                || !parameters.loss_resistance_ohm.is_finite()
                || parameters.loss_resistance_ohm < MIN_RESISTANCE_OHM
                || parameters.loss_resistance_ohm > MAX_RESISTANCE_OHM
            {
                return Err(MovingMagnetCartridgeConfigError::InvalidMagneticLossBranch { branch });
            }
            let relaxation_time_seconds =
                parameters.relaxation_inductance_h / parameters.loss_resistance_ohm;
            if previous_relaxation_time_seconds
                .is_some_and(|previous| relaxation_time_seconds <= previous)
            {
                return Err(
                    MovingMagnetCartridgeConfigError::NonCanonicalMagneticLossBranchOrder {
                        branch,
                    },
                );
            }
            previous_relaxation_time_seconds = Some(relaxation_time_seconds);
            relaxation_inductance_sum_h += parameters.relaxation_inductance_h;
        }
        let residual_self_inductance_h = self.coil_inductance_h - relaxation_inductance_sum_h;
        if !relaxation_inductance_sum_h.is_finite() || residual_self_inductance_h < MIN_INDUCTANCE_H
        {
            return Err(
                MovingMagnetCartridgeConfigError::InvalidMagneticLossInductanceBudget {
                    maximum_sum_h: (self.coil_inductance_h - MIN_INDUCTANCE_H).max(0.0),
                },
            );
        }
        let maximum_mutual_inductance_h = residual_self_inductance_h - MIN_INDUCTANCE_H;
        if !self.coil_mutual_inductance_h.is_finite()
            || self.coil_mutual_inductance_h.abs() > maximum_mutual_inductance_h
        {
            return Err(MovingMagnetCartridgeConfigError::InvalidMutualInductance {
                maximum_abs_h: maximum_mutual_inductance_h.max(0.0),
            });
        }
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
        let response = self.loaded_circuit_frequency_response_at_hz(frequency_hz)?;
        Ok(response.loaded_voltage_per_generator_voltage[0][0].magnitude())
    }

    /// Returns the complete complex loaded-circuit transfer matrix.
    pub fn loaded_circuit_frequency_response_at_hz(
        self,
        frequency_hz: f64,
    ) -> Result<MovingMagnetCartridgeCircuitFrequencyResponse, MovingMagnetCartridgeConfigError>
    {
        self.validate()?;
        validate_bounded("frequencyHz", frequency_hz, 0.0, f64::MAX)?;
        let coil_impedance_ohm = coupled_coil_impedance(self, frequency_hz);
        let loaded_voltage_per_generator_voltage = if magnetic_loss_branch_count(self) == 0 {
            coupled_loaded_voltage_transfer(
                self.coil_resistance_ohm,
                self.coil_inductance_h,
                self.coil_mutual_inductance_h,
                self.load_resistance_ohm,
                self.load_capacitance_f,
                frequency_hz,
            )
        } else {
            coupled_loaded_voltage_transfer_with_magnetic_loss(self, frequency_hz)
        };
        let generator_voltage_per_velocity_v_s_per_m = self.channel_matrix().map(|row| {
            [
                self.generator_coefficient_v_s_per_m * row[0],
                self.generator_coefficient_v_s_per_m * row[1],
            ]
        });
        let loaded_voltage_per_magnet_velocity_v_s_per_m = complex_real_matrix_product(
            loaded_voltage_per_generator_voltage,
            generator_voltage_per_velocity_v_s_per_m,
        );
        if coil_impedance_ohm
            .into_iter()
            .flatten()
            .flat_map(|impedance| [impedance.resistance_ohm, impedance.reactance_ohm])
            .chain(
                loaded_voltage_per_generator_voltage
                    .into_iter()
                    .flatten()
                    .flat_map(|ratio| [ratio.real, ratio.imaginary]),
            )
            .chain(
                loaded_voltage_per_magnet_velocity_v_s_per_m
                    .into_iter()
                    .flatten()
                    .flat_map(|ratio| [ratio.real, ratio.imaginary]),
            )
            .any(|value| !value.is_finite())
        {
            return Err(MovingMagnetCartridgeConfigError::InvalidDerivedValue {
                field: "loadedCircuitFrequencyResponse",
            });
        }
        Ok(MovingMagnetCartridgeCircuitFrequencyResponse {
            coil_impedance_ohm,
            loaded_voltage_per_generator_voltage,
            loaded_voltage_per_magnet_velocity_v_s_per_m,
        })
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
    pub magnetic_loss_inductor_current_a: [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
    pub load_output_voltage_v: [f64; 2],
    /// These values are the trapezoidal port averages for the completed interval.
    pub interval_average_generator_voltage_v: [f64; 2],
    pub interval_average_coil_current_a: [f64; 2],
    pub interval_average_magnetic_loss_inductor_current_a:
        [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
    pub interval_average_load_output_voltage_v: [f64; 2],
    pub electromagnetic_reaction_force_n: [f64; 2],
    pub generator_electrical_power_w: f64,
    pub coil_loss_power_w: f64,
    pub magnetic_loss_power_w: f64,
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
            magnetic_loss_inductor_current_a: [[0.0; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
            load_output_voltage_v: [0.0; 2],
            interval_average_generator_voltage_v: [0.0; 2],
            interval_average_coil_current_a: [0.0; 2],
            interval_average_magnetic_loss_inductor_current_a: [[0.0;
                MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES];
                2],
            interval_average_load_output_voltage_v: [0.0; 2],
            electromagnetic_reaction_force_n: [0.0; 2],
            generator_electrical_power_w: 0.0,
            coil_loss_power_w: 0.0,
            magnetic_loss_power_w: 0.0,
            load_power_w: 0.0,
            stored_electrical_energy_j: 0.0,
            completed_steps: 0,
        }
    }
}

/// Returns the state-independent reciprocal damping in cartridge-coil coordinates.
pub(crate) fn moving_magnet_coil_reciprocal_damping_n_s_per_m(
    config: MovingMagnetCartridgeConfig,
    duration_seconds: f64,
) -> Result<[[f64; 2]; 2], MovingMagnetCartridgeError> {
    validate_duration(duration_seconds)?;
    let config = config.validate()?;
    let circuit = coupled_trapezoidal_circuit_affine_step(
        config,
        duration_seconds,
        [0.0; 2],
        [0.0; 2],
        [0.0; 2],
        [[0.0; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
    )
    .ok_or(MovingMagnetCartridgeError::NumericalFailure)?;
    let coefficient = config.generator_coefficient_v_s_per_m;
    let generator_voltage_per_velocity_v_s_per_m = config
        .channel_matrix()
        .map(|row| [coefficient * row[0], coefficient * row[1]]);
    reciprocal_damping_from_current_response(
        config,
        generator_voltage_per_velocity_v_s_per_m,
        circuit.current_per_generator_voltage_a_per_v,
    )
}

/// Stores all values that affect later cartridge output.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovingMagnetCartridgeSnapshot {
    pub version: u32,
    pub config: MovingMagnetCartridgeConfig,
    pub coil_current_a: [f64; 2],
    #[serde(default)]
    pub magnetic_loss_inductor_current_a: [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
    pub load_output_voltage_v: [f64; 2],
    pub previous_generator_voltage_v: [f64; 2],
    pub last_magnet_velocity_m_s: [f64; 2],
    pub interval_average_generator_voltage_v: [f64; 2],
    pub interval_average_coil_current_a: [f64; 2],
    #[serde(default)]
    pub interval_average_magnetic_loss_inductor_current_a:
        [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
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
    current_per_generator_voltage_a_per_v: [[f64; 2]; 2],
    load_voltage_bias_v: [f64; 2],
    load_voltage_per_generator_voltage: [[f64; 2]; 2],
    reaction_force_bias_n: [f64; 2],
    reciprocal_damping_n_s_per_m: [[f64; 2]; 2],
}

/// Contains the evaluated values for one affine cartridge step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovingMagnetCartridgeAffineOutput {
    pub magnet_velocity_m_s: [f64; 2],
    pub generator_voltage_v: [f64; 2],
    pub coil_current_a: [f64; 2],
    pub magnetic_loss_inductor_current_a: [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
    pub load_output_voltage_v: [f64; 2],
    pub interval_average_generator_voltage_v: [f64; 2],
    pub interval_average_coil_current_a: [f64; 2],
    pub interval_average_magnetic_loss_inductor_current_a:
        [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
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
        let coil_current_response_a = matrix_vector(
            self.current_per_generator_voltage_a_per_v,
            generator_voltage_v,
        );
        let coil_current_a = add(self.current_bias_a, coil_current_response_a);
        let load_voltage_response_v =
            matrix_vector(self.load_voltage_per_generator_voltage, generator_voltage_v);
        let load_output_voltage_v = add(self.load_voltage_bias_v, load_voltage_response_v);
        let (magnetic_loss_inductor_current_a, interval_average_magnetic_loss_inductor_current_a) =
            advance_magnetic_loss_inductor_currents(
                self.source.config,
                self.duration_seconds,
                self.source.coil_current_a,
                coil_current_a,
                self.source.magnetic_loss_inductor_current_a,
            );
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
            .chain(magnetic_loss_inductor_current_a.into_iter().flatten())
            .chain(load_output_voltage_v)
            .chain(interval_average_generator_voltage_v)
            .chain(interval_average_coil_current_a)
            .chain(
                interval_average_magnetic_loss_inductor_current_a
                    .into_iter()
                    .flatten(),
            )
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
            magnetic_loss_inductor_current_a,
            load_output_voltage_v,
            interval_average_generator_voltage_v,
            interval_average_coil_current_a,
            interval_average_magnetic_loss_inductor_current_a,
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
    magnetic_loss_inductor_current_a: [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
    load_output_voltage_v: [f64; 2],
    previous_generator_voltage_v: [f64; 2],
    last_magnet_velocity_m_s: [f64; 2],
    interval_average_generator_voltage_v: [f64; 2],
    interval_average_coil_current_a: [f64; 2],
    interval_average_magnetic_loss_inductor_current_a:
        [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
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
            magnetic_loss_inductor_current_a: [[0.0; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
            load_output_voltage_v: [0.0; 2],
            previous_generator_voltage_v: [0.0; 2],
            last_magnet_velocity_m_s: [0.0; 2],
            interval_average_generator_voltage_v: [0.0; 2],
            interval_average_coil_current_a: [0.0; 2],
            interval_average_magnetic_loss_inductor_current_a: [[0.0;
                MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES];
                2],
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
        let circuit = coupled_trapezoidal_circuit_affine_step(
            self.config,
            duration_seconds,
            self.coil_current_a,
            self.load_output_voltage_v,
            self.previous_generator_voltage_v,
            self.magnetic_loss_inductor_current_a,
        )
        .ok_or(MovingMagnetCartridgeError::NumericalFailure)?;
        let current_bias_a = circuit.current_bias_a;
        let current_per_generator_voltage_a_per_v = circuit.current_per_generator_voltage_a_per_v;
        let load_voltage_bias_v = circuit.load_voltage_bias_v;
        let load_voltage_per_generator_voltage = circuit.load_voltage_per_generator_voltage;

        let channel_matrix = self.config.channel_matrix();
        let coefficient = self.config.generator_coefficient_v_s_per_m;
        let generator_voltage_per_velocity_v_s_per_m =
            channel_matrix.map(|row| [coefficient * row[0], coefficient * row[1]]);
        let mut reaction_force_bias_n = [0.0; 2];
        for velocity_axis in 0..2 {
            for circuit in 0..2 {
                let interval_average_current_bias_a =
                    0.5 * (self.coil_current_a[circuit] + current_bias_a[circuit]);
                reaction_force_bias_n[velocity_axis] -= generator_voltage_per_velocity_v_s_per_m
                    [circuit][velocity_axis]
                    * interval_average_current_bias_a;
            }
        }
        let reciprocal_damping_n_s_per_m = reciprocal_damping_from_current_response(
            self.config,
            generator_voltage_per_velocity_v_s_per_m,
            current_per_generator_voltage_a_per_v,
        )?;
        if generator_voltage_per_velocity_v_s_per_m
            .into_iter()
            .flatten()
            .chain(current_bias_a)
            .chain(current_per_generator_voltage_a_per_v.into_iter().flatten())
            .chain(load_voltage_bias_v)
            .chain(load_voltage_per_generator_voltage.into_iter().flatten())
            .chain(reaction_force_bias_n)
            .chain(reciprocal_damping_n_s_per_m.into_iter().flatten())
            .any(|value| !value.is_finite())
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
        self.magnetic_loss_inductor_current_a = output.magnetic_loss_inductor_current_a;
        self.load_output_voltage_v = output.load_output_voltage_v;
        self.previous_generator_voltage_v = output.generator_voltage_v;
        self.last_magnet_velocity_m_s = output.magnet_velocity_m_s;
        self.interval_average_generator_voltage_v = output.interval_average_generator_voltage_v;
        self.interval_average_coil_current_a = output.interval_average_coil_current_a;
        self.interval_average_magnetic_loss_inductor_current_a =
            output.interval_average_magnetic_loss_inductor_current_a;
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
        let mut magnetic_loss = 0.0;
        for channel in 0..2 {
            for (branch, parameters) in self.config.magnetic_loss_branches.into_iter().enumerate() {
                if parameters.is_inactive() {
                    break;
                }
                let resistor_current_a = self.interval_average_coil_current_a[channel]
                    - self.interval_average_magnetic_loss_inductor_current_a[channel][branch];
                magnetic_loss +=
                    parameters.loss_resistance_ohm * resistor_current_a * resistor_current_a;
            }
        }
        let load_power = self
            .interval_average_load_output_voltage_v
            .into_iter()
            .map(|value| value * value / self.config.load_resistance_ohm)
            .sum();
        let stored_energy = stored_electrical_energy_j(
            self.config,
            self.coil_current_a,
            self.magnetic_loss_inductor_current_a,
            self.load_output_voltage_v,
        );

        MovingMagnetCartridgeTelemetry {
            magnet_velocity_m_s: self.last_magnet_velocity_m_s,
            generator_voltage_v: self.previous_generator_voltage_v,
            coil_current_a: self.coil_current_a,
            magnetic_loss_inductor_current_a: self.magnetic_loss_inductor_current_a,
            load_output_voltage_v: self.load_output_voltage_v,
            interval_average_generator_voltage_v: self.interval_average_generator_voltage_v,
            interval_average_coil_current_a: self.interval_average_coil_current_a,
            interval_average_magnetic_loss_inductor_current_a: self
                .interval_average_magnetic_loss_inductor_current_a,
            interval_average_load_output_voltage_v: self.interval_average_load_output_voltage_v,
            electromagnetic_reaction_force_n: reaction_force,
            generator_electrical_power_w: generator_power,
            coil_loss_power_w: coil_loss,
            magnetic_loss_power_w: magnetic_loss,
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
        self.magnetic_loss_inductor_current_a = [[0.0; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2];
        self.load_output_voltage_v = [0.0; 2];
        self.previous_generator_voltage_v = [0.0; 2];
        self.last_magnet_velocity_m_s = [0.0; 2];
        self.interval_average_generator_voltage_v = [0.0; 2];
        self.interval_average_coil_current_a = [0.0; 2];
        self.interval_average_magnetic_loss_inductor_current_a =
            [[0.0; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2];
        self.interval_average_load_output_voltage_v = [0.0; 2];
        self.completed_steps = 0;
    }

    pub fn snapshot(&self) -> MovingMagnetCartridgeSnapshot {
        MovingMagnetCartridgeSnapshot {
            version: SNAPSHOT_VERSION,
            config: self.config,
            coil_current_a: self.coil_current_a,
            magnetic_loss_inductor_current_a: self.magnetic_loss_inductor_current_a,
            load_output_voltage_v: self.load_output_voltage_v,
            previous_generator_voltage_v: self.previous_generator_voltage_v,
            last_magnet_velocity_m_s: self.last_magnet_velocity_m_s,
            interval_average_generator_voltage_v: self.interval_average_generator_voltage_v,
            interval_average_coil_current_a: self.interval_average_coil_current_a,
            interval_average_magnetic_loss_inductor_current_a: self
                .interval_average_magnetic_loss_inductor_current_a,
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
        if snapshot
            .magnetic_loss_inductor_current_a
            .into_iter()
            .flatten()
            .chain(
                snapshot
                    .interval_average_magnetic_loss_inductor_current_a
                    .into_iter()
                    .flatten(),
            )
            .any(|value| !value.is_finite())
        {
            return Err(MovingMagnetCartridgeError::InvalidSnapshot);
        }
        let active_branches = magnetic_loss_branch_count(config);
        for channel in 0..2 {
            for branch in active_branches..MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES {
                if snapshot.magnetic_loss_inductor_current_a[channel][branch] != 0.0
                    || snapshot.interval_average_magnetic_loss_inductor_current_a[channel][branch]
                        != 0.0
                {
                    return Err(MovingMagnetCartridgeError::InvalidSnapshot);
                }
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
        let stored_energy_j = stored_electrical_energy_j(
            config,
            snapshot.coil_current_a,
            snapshot.magnetic_loss_inductor_current_a,
            snapshot.load_output_voltage_v,
        );
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
        let mut magnetic_loss_power_w = 0.0;
        for channel in 0..2 {
            for branch in 0..active_branches {
                let resistor_current_a = snapshot.interval_average_coil_current_a[channel]
                    - snapshot.interval_average_magnetic_loss_inductor_current_a[channel][branch];
                magnetic_loss_power_w += config.magnetic_loss_branches[branch].loss_resistance_ohm
                    * resistor_current_a
                    * resistor_current_a;
            }
        }
        let load_power_w = snapshot
            .interval_average_load_output_voltage_v
            .into_iter()
            .map(|value| value * value / config.load_resistance_ohm)
            .sum::<f64>();
        if [
            stored_energy_j,
            generator_power_w,
            coil_loss_power_w,
            magnetic_loss_power_w,
            load_power_w,
        ]
        .into_iter()
        .any(|value| !value.is_finite())
        {
            return Err(MovingMagnetCartridgeError::InvalidSnapshot);
        }

        self.config = config;
        self.coil_current_a = snapshot.coil_current_a;
        self.magnetic_loss_inductor_current_a = snapshot.magnetic_loss_inductor_current_a;
        self.load_output_voltage_v = snapshot.load_output_voltage_v;
        self.previous_generator_voltage_v = snapshot.previous_generator_voltage_v;
        self.last_magnet_velocity_m_s = snapshot.last_magnet_velocity_m_s;
        self.interval_average_generator_voltage_v = snapshot.interval_average_generator_voltage_v;
        self.interval_average_coil_current_a = snapshot.interval_average_coil_current_a;
        self.interval_average_magnetic_loss_inductor_current_a =
            snapshot.interval_average_magnetic_loss_inductor_current_a;
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
    #[error("coil mutual inductance magnitude must be finite and at most {maximum_abs_h} H")]
    InvalidMutualInductance { maximum_abs_h: f64 },
    #[error("magnetic-loss branch {branch} must set both inductance and resistance")]
    IncompleteMagneticLossBranch { branch: usize },
    #[error("magnetic-loss branch {branch} follows an inactive slot")]
    NonContiguousMagneticLossBranch { branch: usize },
    #[error("magnetic-loss branch {branch} has an invalid value")]
    InvalidMagneticLossBranch { branch: usize },
    #[error("magnetic-loss branch {branch} is not in increasing relaxation-time order")]
    NonCanonicalMagneticLossBranchOrder { branch: usize },
    #[error("magnetic-loss branch inductance sum must be at most {maximum_sum_h} H")]
    InvalidMagneticLossInductanceBudget { maximum_sum_h: f64 },
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

#[derive(Debug, Clone, Copy)]
struct CoupledTrapezoidalCircuitAffineStep {
    current_bias_a: [f64; 2],
    current_per_generator_voltage_a_per_v: [[f64; 2]; 2],
    load_voltage_bias_v: [f64; 2],
    load_voltage_per_generator_voltage: [[f64; 2]; 2],
}

fn coupled_trapezoidal_circuit_affine_step(
    config: MovingMagnetCartridgeConfig,
    duration_seconds: f64,
    previous_current_a: [f64; 2],
    previous_load_voltage_v: [f64; 2],
    previous_generator_voltage_v: [f64; 2],
    previous_magnetic_loss_inductor_current_a: [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
) -> Option<CoupledTrapezoidalCircuitAffineStep> {
    if magnetic_loss_branch_count(config) != 0 {
        return coupled_trapezoidal_circuit_affine_step_with_magnetic_loss(
            config,
            duration_seconds,
            previous_current_a,
            previous_load_voltage_v,
            previous_generator_voltage_v,
            previous_magnetic_loss_inductor_current_a,
        );
    }
    if config.coil_mutual_inductance_h == 0.0 {
        let mut current_bias_a = [0.0; 2];
        let mut current_per_generator_voltage_a_per_v = [[0.0; 2]; 2];
        let mut load_voltage_bias_v = [0.0; 2];
        let mut load_voltage_per_generator_voltage = [[0.0; 2]; 2];
        for channel in 0..2 {
            let coefficients = trapezoidal_circuit_affine_step(
                config,
                duration_seconds,
                previous_current_a[channel],
                previous_load_voltage_v[channel],
                previous_generator_voltage_v[channel],
            )?;
            current_bias_a[channel] = coefficients.current_bias_a;
            current_per_generator_voltage_a_per_v[channel][channel] =
                coefficients.current_per_generator_voltage_a_per_v;
            load_voltage_bias_v[channel] = coefficients.load_voltage_bias_v;
            load_voltage_per_generator_voltage[channel][channel] =
                coefficients.load_voltage_per_generator_voltage;
        }
        return Some(CoupledTrapezoidalCircuitAffineStep {
            current_bias_a,
            current_per_generator_voltage_a_per_v,
            load_voltage_bias_v,
            load_voltage_per_generator_voltage,
        });
    }

    let previous_current_modes_a = to_symmetric_modes(previous_current_a);
    let previous_load_voltage_modes_v = to_symmetric_modes(previous_load_voltage_v);
    let previous_generator_voltage_modes_v = to_symmetric_modes(previous_generator_voltage_v);
    let common = trapezoidal_circuit_affine_step_with_inductance(
        config,
        config.coil_inductance_h + config.coil_mutual_inductance_h,
        duration_seconds,
        previous_current_modes_a[0],
        previous_load_voltage_modes_v[0],
        previous_generator_voltage_modes_v[0],
    )?;
    let differential = trapezoidal_circuit_affine_step_with_inductance(
        config,
        config.coil_inductance_h - config.coil_mutual_inductance_h,
        duration_seconds,
        previous_current_modes_a[1],
        previous_load_voltage_modes_v[1],
        previous_generator_voltage_modes_v[1],
    )?;

    Some(CoupledTrapezoidalCircuitAffineStep {
        current_bias_a: from_symmetric_modes([common.current_bias_a, differential.current_bias_a]),
        current_per_generator_voltage_a_per_v: symmetric_modal_matrix(
            common.current_per_generator_voltage_a_per_v,
            differential.current_per_generator_voltage_a_per_v,
        ),
        load_voltage_bias_v: from_symmetric_modes([
            common.load_voltage_bias_v,
            differential.load_voltage_bias_v,
        ]),
        load_voltage_per_generator_voltage: symmetric_modal_matrix(
            common.load_voltage_per_generator_voltage,
            differential.load_voltage_per_generator_voltage,
        ),
    })
}

fn coupled_trapezoidal_circuit_affine_step_with_magnetic_loss(
    config: MovingMagnetCartridgeConfig,
    duration_seconds: f64,
    previous_current_a: [f64; 2],
    previous_load_voltage_v: [f64; 2],
    previous_generator_voltage_v: [f64; 2],
    previous_magnetic_loss_inductor_current_a: [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
) -> Option<CoupledTrapezoidalCircuitAffineStep> {
    let residual_self_inductance_h = residual_self_inductance_h(config);
    if config.coil_mutual_inductance_h == 0.0 {
        let mut current_bias_a = [0.0; 2];
        let mut current_per_generator_voltage_a_per_v = [[0.0; 2]; 2];
        let mut load_voltage_bias_v = [0.0; 2];
        let mut load_voltage_per_generator_voltage = [[0.0; 2]; 2];
        for channel in 0..2 {
            let coefficients = trapezoidal_circuit_affine_step_with_magnetic_loss(
                config,
                residual_self_inductance_h,
                duration_seconds,
                previous_current_a[channel],
                previous_load_voltage_v[channel],
                previous_generator_voltage_v[channel],
                previous_magnetic_loss_inductor_current_a[channel],
            )?;
            current_bias_a[channel] = coefficients.current_bias_a;
            current_per_generator_voltage_a_per_v[channel][channel] =
                coefficients.current_per_generator_voltage_a_per_v;
            load_voltage_bias_v[channel] = coefficients.load_voltage_bias_v;
            load_voltage_per_generator_voltage[channel][channel] =
                coefficients.load_voltage_per_generator_voltage;
        }
        return Some(CoupledTrapezoidalCircuitAffineStep {
            current_bias_a,
            current_per_generator_voltage_a_per_v,
            load_voltage_bias_v,
            load_voltage_per_generator_voltage,
        });
    }

    let previous_current_modes_a = to_symmetric_modes(previous_current_a);
    let previous_load_voltage_modes_v = to_symmetric_modes(previous_load_voltage_v);
    let previous_generator_voltage_modes_v = to_symmetric_modes(previous_generator_voltage_v);
    let mut previous_branch_current_modes_a = [[0.0; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2];
    for branch in 0..magnetic_loss_branch_count(config) {
        let modes = to_symmetric_modes([
            previous_magnetic_loss_inductor_current_a[0][branch],
            previous_magnetic_loss_inductor_current_a[1][branch],
        ]);
        previous_branch_current_modes_a[0][branch] = modes[0];
        previous_branch_current_modes_a[1][branch] = modes[1];
    }
    let common = trapezoidal_circuit_affine_step_with_magnetic_loss(
        config,
        residual_self_inductance_h + config.coil_mutual_inductance_h,
        duration_seconds,
        previous_current_modes_a[0],
        previous_load_voltage_modes_v[0],
        previous_generator_voltage_modes_v[0],
        previous_branch_current_modes_a[0],
    )?;
    let differential = trapezoidal_circuit_affine_step_with_magnetic_loss(
        config,
        residual_self_inductance_h - config.coil_mutual_inductance_h,
        duration_seconds,
        previous_current_modes_a[1],
        previous_load_voltage_modes_v[1],
        previous_generator_voltage_modes_v[1],
        previous_branch_current_modes_a[1],
    )?;

    Some(CoupledTrapezoidalCircuitAffineStep {
        current_bias_a: from_symmetric_modes([common.current_bias_a, differential.current_bias_a]),
        current_per_generator_voltage_a_per_v: symmetric_modal_matrix(
            common.current_per_generator_voltage_a_per_v,
            differential.current_per_generator_voltage_a_per_v,
        ),
        load_voltage_bias_v: from_symmetric_modes([
            common.load_voltage_bias_v,
            differential.load_voltage_bias_v,
        ]),
        load_voltage_per_generator_voltage: symmetric_modal_matrix(
            common.load_voltage_per_generator_voltage,
            differential.load_voltage_per_generator_voltage,
        ),
    })
}

fn trapezoidal_circuit_affine_step_with_magnetic_loss(
    config: MovingMagnetCartridgeConfig,
    residual_modal_inductance_h: f64,
    duration_seconds: f64,
    previous_current_a: f64,
    previous_load_voltage_v: f64,
    previous_generator_voltage_v: f64,
    previous_magnetic_loss_inductor_current_a: [f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES],
) -> Option<TrapezoidalCircuitAffineStep> {
    let half_step = 0.5 * duration_seconds;
    let inverse_inductance = 1.0 / residual_modal_inductance_h;
    let inverse_capacitance = 1.0 / config.load_capacitance_f;
    let inverse_load_time_constant = 1.0 / (config.load_resistance_ohm * config.load_capacitance_f);

    let mut left_00 = 1.0 + half_step * config.coil_resistance_ohm * inverse_inductance;
    let left_01 = half_step * inverse_inductance;
    let left_10 = -half_step * inverse_capacitance;
    let left_11 = 1.0 + half_step * inverse_load_time_constant;
    let mut right_0_bias = (1.0 - half_step * config.coil_resistance_ohm * inverse_inductance)
        * previous_current_a
        - half_step * inverse_inductance * previous_load_voltage_v
        + half_step * inverse_inductance * previous_generator_voltage_v;
    let right_0_per_generator_voltage = half_step * inverse_inductance;
    let right_1 = half_step * inverse_capacitance * previous_current_a
        + (1.0 - half_step * inverse_load_time_constant) * previous_load_voltage_v;

    for (branch, parameters) in config.magnetic_loss_branches.into_iter().enumerate() {
        if parameters.is_inactive() {
            break;
        }
        let alpha = half_step * parameters.loss_resistance_ohm / parameters.relaxation_inductance_h;
        let beta = alpha / (1.0 + alpha);
        let previous_branch_current_a = previous_magnetic_loss_inductor_current_a[branch];
        let branch_current_static_a =
            (1.0 - 2.0 * beta) * previous_branch_current_a + beta * previous_current_a;
        let previous_branch_voltage_v =
            parameters.loss_resistance_ohm * (previous_current_a - previous_branch_current_a);
        left_00 += half_step * inverse_inductance * parameters.loss_resistance_ohm * (1.0 - beta);
        right_0_bias -= half_step
            * inverse_inductance
            * (previous_branch_voltage_v
                - parameters.loss_resistance_ohm * branch_current_static_a);
    }

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

fn trapezoidal_circuit_affine_step(
    config: MovingMagnetCartridgeConfig,
    duration_seconds: f64,
    previous_current_a: f64,
    previous_load_voltage_v: f64,
    previous_generator_voltage_v: f64,
) -> Option<TrapezoidalCircuitAffineStep> {
    trapezoidal_circuit_affine_step_with_inductance(
        config,
        config.coil_inductance_h,
        duration_seconds,
        previous_current_a,
        previous_load_voltage_v,
        previous_generator_voltage_v,
    )
}

fn trapezoidal_circuit_affine_step_with_inductance(
    config: MovingMagnetCartridgeConfig,
    coil_inductance_h: f64,
    duration_seconds: f64,
    previous_current_a: f64,
    previous_load_voltage_v: f64,
    previous_generator_voltage_v: f64,
) -> Option<TrapezoidalCircuitAffineStep> {
    let half_step = 0.5 * duration_seconds;
    let resistance_over_inductance = config.coil_resistance_ohm / coil_inductance_h;
    let inverse_inductance = 1.0 / coil_inductance_h;
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

fn reciprocal_damping_from_current_response(
    config: MovingMagnetCartridgeConfig,
    generator_voltage_per_velocity_v_s_per_m: [[f64; 2]; 2],
    current_per_generator_voltage_a_per_v: [[f64; 2]; 2],
) -> Result<[[f64; 2]; 2], MovingMagnetCartridgeError> {
    let mut reciprocal_damping_n_s_per_m = [[0.0; 2]; 2];
    for velocity_axis in 0..2 {
        for other_axis in 0..2 {
            if config.coil_mutual_inductance_h == 0.0 {
                for circuit in 0..2 {
                    reciprocal_damping_n_s_per_m[velocity_axis][other_axis] +=
                        generator_voltage_per_velocity_v_s_per_m[circuit][velocity_axis]
                            * (0.5 * current_per_generator_voltage_a_per_v[circuit][circuit])
                            * generator_voltage_per_velocity_v_s_per_m[circuit][other_axis];
                }
            } else {
                for circuit in 0..2 {
                    for driven_circuit in 0..2 {
                        reciprocal_damping_n_s_per_m[velocity_axis][other_axis] +=
                            generator_voltage_per_velocity_v_s_per_m[circuit][velocity_axis]
                                * (0.5
                                    * current_per_generator_voltage_a_per_v[circuit]
                                        [driven_circuit])
                                * generator_voltage_per_velocity_v_s_per_m[driven_circuit]
                                    [other_axis];
                    }
                }
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
    if reciprocal_damping_n_s_per_m
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite())
        || reciprocal_damping_n_s_per_m[0][0] < 0.0
        || reciprocal_damping_n_s_per_m[1][1] < 0.0
    {
        return Err(MovingMagnetCartridgeError::NumericalFailure);
    }
    Ok(reciprocal_damping_n_s_per_m)
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

fn complex_real_matrix_product(
    left: [[CartridgeComplexVoltageRatio; 2]; 2],
    right: [[f64; 2]; 2],
) -> [[CartridgeComplexVoltageRatio; 2]; 2] {
    let mut product = [[CartridgeComplexVoltageRatio::default(); 2]; 2];
    for row in 0..2 {
        for column in 0..2 {
            for inner in 0..2 {
                product[row][column].real += left[row][inner].real * right[inner][column];
                product[row][column].imaginary += left[row][inner].imaginary * right[inner][column];
            }
        }
    }
    product
}

fn add(left: [f64; 2], right: [f64; 2]) -> [f64; 2] {
    [left[0] + right[0], left[1] + right[1]]
}

fn subtract(left: [f64; 2], right: [f64; 2]) -> [f64; 2] {
    [left[0] - right[0], left[1] - right[1]]
}

fn to_symmetric_modes(values: [f64; 2]) -> [f64; 2] {
    [
        (values[0] + values[1]) * INVERSE_SQRT_2,
        (values[0] - values[1]) * INVERSE_SQRT_2,
    ]
}

fn from_symmetric_modes(modes: [f64; 2]) -> [f64; 2] {
    [
        (modes[0] + modes[1]) * INVERSE_SQRT_2,
        (modes[0] - modes[1]) * INVERSE_SQRT_2,
    ]
}

fn symmetric_modal_matrix(common: f64, differential: f64) -> [[f64; 2]; 2] {
    let direct = 0.5 * (common + differential);
    let coupled = 0.5 * (common - differential);
    [[direct, coupled], [coupled, direct]]
}

fn magnetic_loss_branch_count(config: MovingMagnetCartridgeConfig) -> usize {
    config
        .magnetic_loss_branches
        .into_iter()
        .take_while(|branch| !branch.is_inactive())
        .count()
}

fn relaxation_inductance_sum_h(config: MovingMagnetCartridgeConfig) -> f64 {
    config
        .magnetic_loss_branches
        .into_iter()
        .take_while(|branch| !branch.is_inactive())
        .map(|branch| branch.relaxation_inductance_h)
        .sum()
}

fn residual_self_inductance_h(config: MovingMagnetCartridgeConfig) -> f64 {
    config.coil_inductance_h - relaxation_inductance_sum_h(config)
}

fn advance_magnetic_loss_inductor_currents(
    config: MovingMagnetCartridgeConfig,
    duration_seconds: f64,
    previous_coil_current_a: [f64; 2],
    coil_current_a: [f64; 2],
    previous_branch_current_a: [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
) -> (
    [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
    [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
) {
    let mut branch_current_a = [[0.0; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2];
    let mut interval_average_branch_current_a = [[0.0; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2];
    let half_step = 0.5 * duration_seconds;
    for channel in 0..2 {
        for (branch, parameters) in config.magnetic_loss_branches.into_iter().enumerate() {
            if parameters.is_inactive() {
                break;
            }
            let alpha =
                half_step * parameters.loss_resistance_ohm / parameters.relaxation_inductance_h;
            let beta = alpha / (1.0 + alpha);
            let next = (1.0 - 2.0 * beta) * previous_branch_current_a[channel][branch]
                + beta * (previous_coil_current_a[channel] + coil_current_a[channel]);
            branch_current_a[channel][branch] = next;
            interval_average_branch_current_a[channel][branch] =
                0.5 * (previous_branch_current_a[channel][branch] + next);
        }
    }
    (branch_current_a, interval_average_branch_current_a)
}

fn stored_electrical_energy_j(
    config: MovingMagnetCartridgeConfig,
    coil_current_a: [f64; 2],
    magnetic_loss_inductor_current_a: [[f64; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
    load_output_voltage_v: [f64; 2],
) -> f64 {
    if magnetic_loss_branch_count(config) == 0 {
        return 0.5
            * config.coil_inductance_h
            * coil_current_a
                .into_iter()
                .map(|value| value * value)
                .sum::<f64>()
            + config.coil_mutual_inductance_h * coil_current_a[0] * coil_current_a[1]
            + 0.5
                * config.load_capacitance_f
                * load_output_voltage_v
                    .into_iter()
                    .map(|value| value * value)
                    .sum::<f64>();
    }

    let mut energy_j = 0.5
        * residual_self_inductance_h(config)
        * coil_current_a
            .into_iter()
            .map(|value| value * value)
            .sum::<f64>()
        + config.coil_mutual_inductance_h * coil_current_a[0] * coil_current_a[1]
        + 0.5
            * config.load_capacitance_f
            * load_output_voltage_v
                .into_iter()
                .map(|value| value * value)
                .sum::<f64>();
    for channel in 0..2 {
        for (branch, parameters) in config.magnetic_loss_branches.into_iter().enumerate() {
            if parameters.is_inactive() {
                break;
            }
            let current_a = magnetic_loss_inductor_current_a[channel][branch];
            energy_j += 0.5 * parameters.relaxation_inductance_h * current_a * current_a;
        }
    }
    energy_j
}

fn coupled_coil_impedance(
    config: MovingMagnetCartridgeConfig,
    frequency_hz: f64,
) -> [[CartridgeComplexImpedanceOhm; 2]; 2] {
    let angular_frequency = std::f64::consts::TAU * frequency_hz;
    if magnetic_loss_branch_count(config) == 0 {
        let direct = CartridgeComplexImpedanceOhm {
            resistance_ohm: config.coil_resistance_ohm,
            reactance_ohm: angular_frequency * config.coil_inductance_h,
        };
        let coupled = CartridgeComplexImpedanceOhm {
            resistance_ohm: 0.0,
            reactance_ohm: angular_frequency * config.coil_mutual_inductance_h,
        };
        return [[direct, coupled], [coupled, direct]];
    }

    let residual_self_inductance_h = residual_self_inductance_h(config);
    let common = coil_modal_impedance(
        config,
        residual_self_inductance_h + config.coil_mutual_inductance_h,
        angular_frequency,
    );
    let differential = coil_modal_impedance(
        config,
        residual_self_inductance_h - config.coil_mutual_inductance_h,
        angular_frequency,
    );
    let direct = CartridgeComplexImpedanceOhm {
        resistance_ohm: 0.5 * (common.resistance_ohm + differential.resistance_ohm),
        reactance_ohm: 0.5 * (common.reactance_ohm + differential.reactance_ohm),
    };
    let coupled = CartridgeComplexImpedanceOhm {
        resistance_ohm: 0.5 * (common.resistance_ohm - differential.resistance_ohm),
        reactance_ohm: 0.5 * (common.reactance_ohm - differential.reactance_ohm),
    };
    [[direct, coupled], [coupled, direct]]
}

fn coil_modal_impedance(
    config: MovingMagnetCartridgeConfig,
    residual_modal_inductance_h: f64,
    angular_frequency: f64,
) -> CartridgeComplexImpedanceOhm {
    let mut impedance = CartridgeComplexImpedanceOhm {
        resistance_ohm: config.coil_resistance_ohm,
        reactance_ohm: angular_frequency * residual_modal_inductance_h,
    };
    for parameters in config.magnetic_loss_branches {
        if parameters.is_inactive() {
            break;
        }
        let inductive_reactance_ohm = angular_frequency * parameters.relaxation_inductance_h;
        let denominator = parameters.loss_resistance_ohm * parameters.loss_resistance_ohm
            + inductive_reactance_ohm * inductive_reactance_ohm;
        impedance.resistance_ohm +=
            parameters.loss_resistance_ohm * inductive_reactance_ohm * inductive_reactance_ohm
                / denominator;
        impedance.reactance_ohm += parameters.loss_resistance_ohm
            * parameters.loss_resistance_ohm
            * inductive_reactance_ohm
            / denominator;
    }
    impedance
}

fn coupled_loaded_voltage_transfer_with_magnetic_loss(
    config: MovingMagnetCartridgeConfig,
    frequency_hz: f64,
) -> [[CartridgeComplexVoltageRatio; 2]; 2] {
    let angular_frequency = std::f64::consts::TAU * frequency_hz;
    let residual_self_inductance_h = residual_self_inductance_h(config);
    let common = loaded_voltage_complex_ratio_from_coil_impedance(
        coil_modal_impedance(
            config,
            residual_self_inductance_h + config.coil_mutual_inductance_h,
            angular_frequency,
        ),
        config.load_resistance_ohm,
        config.load_capacitance_f,
        angular_frequency,
    );
    let differential = loaded_voltage_complex_ratio_from_coil_impedance(
        coil_modal_impedance(
            config,
            residual_self_inductance_h - config.coil_mutual_inductance_h,
            angular_frequency,
        ),
        config.load_resistance_ohm,
        config.load_capacitance_f,
        angular_frequency,
    );
    let direct = CartridgeComplexVoltageRatio {
        real: 0.5 * (common.real + differential.real),
        imaginary: 0.5 * (common.imaginary + differential.imaginary),
    };
    let coupled = CartridgeComplexVoltageRatio {
        real: 0.5 * (common.real - differential.real),
        imaginary: 0.5 * (common.imaginary - differential.imaginary),
    };
    [[direct, coupled], [coupled, direct]]
}

fn loaded_voltage_complex_ratio_from_coil_impedance(
    coil: CartridgeComplexImpedanceOhm,
    load_resistance_ohm: f64,
    load_capacitance_f: f64,
    angular_frequency: f64,
) -> CartridgeComplexVoltageRatio {
    let load_conductance = 1.0 / load_resistance_ohm;
    let load_susceptance = angular_frequency * load_capacitance_f;
    let admittance_magnitude_squared =
        load_conductance * load_conductance + load_susceptance * load_susceptance;
    let load_real = load_conductance / admittance_magnitude_squared;
    let load_imaginary = -load_susceptance / admittance_magnitude_squared;
    let total_real = coil.resistance_ohm + load_real;
    let total_imaginary = coil.reactance_ohm + load_imaginary;
    let denominator = total_real * total_real + total_imaginary * total_imaginary;
    CartridgeComplexVoltageRatio {
        real: (load_real * total_real + load_imaginary * total_imaginary) / denominator,
        imaginary: (load_imaginary * total_real - load_real * total_imaginary) / denominator,
    }
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

fn coupled_loaded_voltage_transfer(
    coil_resistance_ohm: f64,
    coil_inductance_h: f64,
    coil_mutual_inductance_h: f64,
    load_resistance_ohm: f64,
    load_capacitance_f: f64,
    frequency_hz: f64,
) -> [[CartridgeComplexVoltageRatio; 2]; 2] {
    if coil_mutual_inductance_h == 0.0 {
        let direct = loaded_voltage_complex_ratio(
            coil_resistance_ohm,
            coil_inductance_h,
            load_resistance_ohm,
            load_capacitance_f,
            frequency_hz,
        );
        return [
            [direct, CartridgeComplexVoltageRatio::default()],
            [CartridgeComplexVoltageRatio::default(), direct],
        ];
    }
    let common = loaded_voltage_complex_ratio(
        coil_resistance_ohm,
        coil_inductance_h + coil_mutual_inductance_h,
        load_resistance_ohm,
        load_capacitance_f,
        frequency_hz,
    );
    let differential = loaded_voltage_complex_ratio(
        coil_resistance_ohm,
        coil_inductance_h - coil_mutual_inductance_h,
        load_resistance_ohm,
        load_capacitance_f,
        frequency_hz,
    );
    let direct = CartridgeComplexVoltageRatio {
        real: 0.5 * (common.real + differential.real),
        imaginary: 0.5 * (common.imaginary + differential.imaginary),
    };
    let coupled = CartridgeComplexVoltageRatio {
        real: 0.5 * (common.real - differential.real),
        imaginary: 0.5 * (common.imaginary - differential.imaginary),
    };
    [[direct, coupled], [coupled, direct]]
}

fn loaded_voltage_complex_ratio(
    coil_resistance_ohm: f64,
    coil_inductance_h: f64,
    load_resistance_ohm: f64,
    load_capacitance_f: f64,
    frequency_hz: f64,
) -> CartridgeComplexVoltageRatio {
    let angular_frequency = std::f64::consts::TAU * frequency_hz;
    let load_conductance = 1.0 / load_resistance_ohm;
    let load_susceptance = angular_frequency * load_capacitance_f;
    let admittance_magnitude_squared =
        load_conductance * load_conductance + load_susceptance * load_susceptance;
    let load_real = load_conductance / admittance_magnitude_squared;
    let load_imaginary = -load_susceptance / admittance_magnitude_squared;
    let total_real = coil_resistance_ohm + load_real;
    let total_imaginary = angular_frequency * coil_inductance_h + load_imaginary;
    let denominator = total_real * total_real + total_imaginary * total_imaginary;
    CartridgeComplexVoltageRatio {
        real: (load_real * total_real + load_imaginary * total_imaginary) / denominator,
        imaginary: (load_imaginary * total_real - load_real * total_imaginary) / denominator,
    }
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

    fn mutual_config(coupling_ratio: f64) -> MovingMagnetCartridgeConfig {
        let seed = MovingMagnetCartridgeConfig::default();
        MovingMagnetCartridgeConfig {
            coil_mutual_inductance_h: coupling_ratio * seed.coil_inductance_h,
            channel_balance_db: 0.0,
            channel_separation_db: 200.0,
            ..seed
        }
    }

    fn magnetic_loss_config(coupling_ratio: f64) -> MovingMagnetCartridgeConfig {
        let seed = MovingMagnetCartridgeConfig::default();
        let magnetic_loss_branches = [
            MagneticLossRelaxationBranchConfig {
                relaxation_inductance_h: 0.020,
                loss_resistance_ohm: 4_000.0,
            },
            MagneticLossRelaxationBranchConfig {
                relaxation_inductance_h: 0.180,
                loss_resistance_ohm: 1_800.0,
            },
            MagneticLossRelaxationBranchConfig::default(),
            MagneticLossRelaxationBranchConfig::default(),
        ];
        let residual_inductance_h = seed.coil_inductance_h
            - magnetic_loss_branches
                .into_iter()
                .map(|branch| branch.relaxation_inductance_h)
                .sum::<f64>();
        MovingMagnetCartridgeConfig {
            coil_mutual_inductance_h: coupling_ratio * residual_inductance_h,
            magnetic_loss_branches,
            channel_balance_db: 0.0,
            channel_separation_db: 200.0,
            ..seed
        }
    }

    #[test]
    fn seed_uses_published_values_and_a_derived_generator_coefficient() {
        let config = MovingMagnetCartridgeConfig::concorde_mkii_scratch_seed();
        assert_eq!(
            config.generator_coefficient_source,
            GeneratorCoefficientSource::DerivedFromLoadedOutputSpecification
        );
        assert_eq!(config.coil_resistance_ohm, 1_200.0);
        assert_eq!(config.coil_inductance_h, 0.850);
        assert_eq!(config.coil_mutual_inductance_h, 0.0);
        assert_eq!(
            config.magnetic_loss_branches,
            [MagneticLossRelaxationBranchConfig::default(); MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]
        );
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
    fn zero_mutual_inductance_is_bit_identical_to_two_independent_circuits() {
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let mut cartridge = MovingMagnetCartridge::default();
        for sample in 0..10_000 {
            let phase = sample as f64 * 0.137;
            let velocity = [0.09 * phase.sin(), -0.07 * (phase * 1.31).cos()];
            let expected = independent_circuit_reference(cartridge.snapshot(), dt, velocity);
            let actual = cartridge.advance(dt, velocity).unwrap();
            assert_array_bits_eq(actual.generator_voltage_v, expected.generator_voltage_v);
            assert_array_bits_eq(actual.coil_current_a, expected.coil_current_a);
            assert_array_bits_eq(actual.load_output_voltage_v, expected.load_output_voltage_v);
            assert_array_bits_eq(
                actual.interval_average_generator_voltage_v,
                expected.interval_average_generator_voltage_v,
            );
            assert_array_bits_eq(
                actual.interval_average_coil_current_a,
                expected.interval_average_coil_current_a,
            );
            assert_array_bits_eq(
                actual.interval_average_load_output_voltage_v,
                expected.interval_average_load_output_voltage_v,
            );
            assert_array_bits_eq(
                actual.electromagnetic_reaction_force_n,
                expected.electromagnetic_reaction_force_n,
            );
        }
    }

    #[test]
    fn zero_magnetic_loss_slots_are_bit_identical_to_the_legacy_coupled_solver() {
        let config = mutual_config(0.63);
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let mut previous_current_a = [0.0; 2];
        let mut previous_load_voltage_v = [0.0; 2];
        let mut previous_generator_voltage_v = [0.0; 2];
        for sample in 0..10_000 {
            let actual = coupled_trapezoidal_circuit_affine_step(
                config,
                dt,
                previous_current_a,
                previous_load_voltage_v,
                previous_generator_voltage_v,
                [[0.0; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
            )
            .unwrap();
            let expected = legacy_coupled_trapezoidal_circuit_affine_step(
                config,
                dt,
                previous_current_a,
                previous_load_voltage_v,
                previous_generator_voltage_v,
            )
            .unwrap();
            assert_array_bits_eq(actual.current_bias_a, expected.current_bias_a);
            assert_matrix_bits_eq(
                actual.current_per_generator_voltage_a_per_v,
                expected.current_per_generator_voltage_a_per_v,
            );
            assert_array_bits_eq(actual.load_voltage_bias_v, expected.load_voltage_bias_v);
            assert_matrix_bits_eq(
                actual.load_voltage_per_generator_voltage,
                expected.load_voltage_per_generator_voltage,
            );

            let phase = sample as f64 * 0.073;
            let generator_voltage_v = [0.17 * phase.sin(), -0.11 * (phase * 1.31).cos()];
            previous_current_a = add(
                actual.current_bias_a,
                matrix_vector(
                    actual.current_per_generator_voltage_a_per_v,
                    generator_voltage_v,
                ),
            );
            previous_load_voltage_v = add(
                actual.load_voltage_bias_v,
                matrix_vector(
                    actual.load_voltage_per_generator_voltage,
                    generator_voltage_v,
                ),
            );
            previous_generator_voltage_v = generator_voltage_v;
        }
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
    fn coupled_coil_affine_damping_is_reciprocal_and_passive() {
        for coupling_ratio in [-0.95, -0.5, 0.5, 0.95] {
            let mut cartridge = MovingMagnetCartridge::new(mutual_config(coupling_ratio)).unwrap();
            for sample_rate_hz in [44_100.0, 48_000.0, 96_000.0, 192_000.0, 768_000.0] {
                let dt = 1.0 / sample_rate_hz;
                cartridge
                    .advance(dt, [0.071 * coupling_ratio, -0.043])
                    .unwrap();
                let damping = cartridge
                    .prepare_affine_step(dt)
                    .unwrap()
                    .reciprocal_damping_n_s_per_m();
                assert_eq!(damping[0][1], damping[1][0]);
                assert!(damping[0][0] >= 0.0);
                assert!(damping[1][1] >= 0.0);
                let determinant = damping[0][0] * damping[1][1] - damping[0][1] * damping[1][0];
                let scale = (damping[0][0] * damping[1][1]).max(1.0e-30);
                assert!(determinant >= -128.0 * f64::EPSILON * scale);
                for velocity in [
                    [1.0, 0.0],
                    [0.0, 1.0],
                    [1.0, -2.0],
                    [-17.0, 31.0],
                    [coupling_ratio, -coupling_ratio],
                ] {
                    let absorbed_power = dot(velocity, matrix_vector(damping, velocity));
                    assert!(absorbed_power >= -128.0 * f64::EPSILON * scale);
                }
            }
        }
    }

    #[test]
    fn reciprocal_damping_bits_depend_only_on_config_and_duration() {
        let duration_seconds = 1.0 / TEST_SAMPLE_RATE_HZ;
        let configs = [
            MovingMagnetCartridgeConfig::default(),
            mutual_config(-0.63),
            mutual_config(0.71),
            magnetic_loss_config(-0.58),
            magnetic_loss_config(0.67),
        ];
        for config in configs {
            let expected =
                moving_magnet_coil_reciprocal_damping_n_s_per_m(config, duration_seconds).unwrap();
            let mut cartridge = MovingMagnetCartridge::new(config).unwrap();
            for sample in 0..257 {
                let snapshot = cartridge.snapshot();
                let circuit = coupled_trapezoidal_circuit_affine_step(
                    config,
                    duration_seconds,
                    snapshot.coil_current_a,
                    snapshot.load_output_voltage_v,
                    snapshot.previous_generator_voltage_v,
                    snapshot.magnetic_loss_inductor_current_a,
                )
                .unwrap();
                let coefficient = config.generator_coefficient_v_s_per_m;
                let generator_voltage_per_velocity_v_s_per_m = config
                    .channel_matrix()
                    .map(|row| [coefficient * row[0], coefficient * row[1]]);
                let state_derived = reciprocal_damping_from_current_response(
                    config,
                    generator_voltage_per_velocity_v_s_per_m,
                    circuit.current_per_generator_voltage_a_per_v,
                )
                .unwrap();
                assert_matrix_bits_eq(state_derived, expected);
                assert_matrix_bits_eq(
                    cartridge
                        .prepare_affine_step(duration_seconds)
                        .unwrap()
                        .reciprocal_damping_n_s_per_m(),
                    expected,
                );

                let phase = sample as f64 * 0.173;
                let advance_duration_seconds = 1.0 / [44_100.0, 96_000.0, 384_000.0][sample % 3];
                cartridge
                    .advance(
                        advance_duration_seconds,
                        [0.19 * phase.sin(), -0.13 * (phase * 1.37).cos()],
                    )
                    .unwrap();
            }
        }
    }

    #[test]
    fn reciprocal_damping_helper_rejects_invalid_inputs() {
        for duration_seconds in [f64::NAN, f64::INFINITY, 0.0, -1.0, 10.1] {
            assert_eq!(
                moving_magnet_coil_reciprocal_damping_n_s_per_m(
                    MovingMagnetCartridgeConfig::default(),
                    duration_seconds,
                ),
                Err(MovingMagnetCartridgeError::InvalidDuration)
            );
        }

        let invalid_config = MovingMagnetCartridgeConfig {
            coil_resistance_ohm: f64::NAN,
            ..MovingMagnetCartridgeConfig::default()
        };
        assert!(matches!(
            moving_magnet_coil_reciprocal_damping_n_s_per_m(
                invalid_config,
                1.0 / TEST_SAMPLE_RATE_HZ,
            ),
            Err(MovingMagnetCartridgeError::InvalidConfig(_))
        ));
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
    fn coupled_coil_port_closes_the_energy_balance_during_sample_reversals() {
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        for coupling_ratio in [-0.72, 0.72] {
            let mut cartridge = MovingMagnetCartridge::new(mutual_config(coupling_ratio)).unwrap();
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
                    (energy_change_j - port_energy_j).abs() <= 5.0e-11 * scale + 2.0e-27,
                    "ratio {coupling_ratio}, sample {sample}: {energy_change_j} != {port_energy_j}"
                );
                previous_energy_j = telemetry.stored_electrical_energy_j;
            }
        }
    }

    #[test]
    fn magnetic_loss_network_closes_the_exact_discrete_energy_balance() {
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        for coupling_ratio in [-0.72, 0.0, 0.72] {
            let mut cartridge =
                MovingMagnetCartridge::new(magnetic_loss_config(coupling_ratio)).unwrap();
            let mut previous_energy_j = cartridge.telemetry().stored_electrical_energy_j;
            let mut observed_magnetic_loss = false;
            for sample in 0..20_000 {
                let polarity = if sample & 1 == 0 { 1.0 } else { -1.0 };
                let phase = sample as f64 * 0.371;
                let velocity = [
                    polarity * (0.15 + 0.11 * phase.sin()),
                    -polarity * (0.09 + 0.07 * (phase * 1.73).cos()),
                ];
                let telemetry = cartridge.advance(dt, velocity).unwrap();
                observed_magnetic_loss |= telemetry.magnetic_loss_power_w > 0.0;
                let energy_change_j = telemetry.stored_electrical_energy_j - previous_energy_j;
                let port_energy_j = dt
                    * (telemetry.generator_electrical_power_w
                        - telemetry.coil_loss_power_w
                        - telemetry.magnetic_loss_power_w
                        - telemetry.load_power_w);
                let scale = energy_change_j.abs().max(port_energy_j.abs()).max(1.0e-30);
                assert!(
                    (energy_change_j - port_energy_j).abs() <= 8.0e-11 * scale + 3.0e-27,
                    "ratio {coupling_ratio}, sample {sample}: {energy_change_j} != {port_energy_j}"
                );
                previous_energy_j = telemetry.stored_electrical_energy_j;
            }
            assert!(observed_magnetic_loss);
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
    fn full_complex_loaded_transfer_exposes_phase_and_mutual_polarity() {
        let uncoupled = mutual_config(0.0)
            .loaded_circuit_frequency_response_at_hz(1_000.0)
            .unwrap()
            .loaded_voltage_per_generator_voltage;
        assert_eq!(uncoupled[0][1], CartridgeComplexVoltageRatio::default());
        assert_eq!(uncoupled[1][0], CartridgeComplexVoltageRatio::default());
        assert_eq!(uncoupled[0][0], uncoupled[1][1]);

        for frequency_hz in [20.0, 1_000.0, 12_000.0, 40_000.0] {
            let positive = mutual_config(0.42)
                .loaded_circuit_frequency_response_at_hz(frequency_hz)
                .unwrap()
                .loaded_voltage_per_generator_voltage;
            let negative = mutual_config(-0.42)
                .loaded_circuit_frequency_response_at_hz(frequency_hz)
                .unwrap()
                .loaded_voltage_per_generator_voltage;
            assert_eq!(positive[0][0], positive[1][1]);
            assert_eq!(positive[0][1], positive[1][0]);
            assert_relative_eq!(positive[0][0].real, negative[0][0].real, epsilon = 1.0e-15);
            assert_relative_eq!(
                positive[0][0].imaginary,
                negative[0][0].imaginary,
                epsilon = 1.0e-15
            );
            assert_relative_eq!(positive[0][1].real, -negative[0][1].real, epsilon = 1.0e-15);
            assert_relative_eq!(
                positive[0][1].imaginary,
                -negative[0][1].imaginary,
                epsilon = 1.0e-15
            );
            assert!(positive[0][1].magnitude() > 0.0);
            assert!(positive[0][1].phase_radians().is_finite());
        }

        let low = mutual_config(0.42)
            .loaded_circuit_frequency_response_at_hz(1_000.0)
            .unwrap()
            .loaded_voltage_per_generator_voltage;
        let high = mutual_config(0.42)
            .loaded_circuit_frequency_response_at_hz(12_000.0)
            .unwrap()
            .loaded_voltage_per_generator_voltage;
        let low_relative = complex_divide(low[1][0], low[0][0]);
        let high_relative = complex_divide(high[1][0], high[0][0]);
        assert!((low_relative.magnitude() - high_relative.magnitude()).abs() > 0.01);
        assert!((low_relative.phase_radians() - high_relative.phase_radians()).abs() > 0.05);
    }

    #[test]
    fn passive_magnetic_loss_impedance_has_the_required_low_and_high_frequency_limits() {
        let config = magnetic_loss_config(0.42).validate().unwrap();
        let low_frequency_hz = 1.0e-3;
        let low = config
            .loaded_circuit_frequency_response_at_hz(low_frequency_hz)
            .unwrap()
            .coil_impedance_ohm;
        let low_angular_frequency = std::f64::consts::TAU * low_frequency_hz;
        assert_relative_eq!(
            low[0][0].resistance_ohm,
            config.coil_resistance_ohm,
            max_relative = 1.0e-10
        );
        assert_relative_eq!(
            low[0][0].reactance_ohm / low_angular_frequency,
            config.coil_inductance_h,
            max_relative = 1.0e-10
        );
        assert_relative_eq!(
            low[0][1].reactance_ohm / low_angular_frequency,
            config.coil_mutual_inductance_h,
            max_relative = 1.0e-12
        );

        let high_frequency_hz = 1.0e10;
        let high = config
            .loaded_circuit_frequency_response_at_hz(high_frequency_hz)
            .unwrap()
            .coil_impedance_ohm;
        let expected_high_resistance_ohm = config.coil_resistance_ohm
            + config
                .magnetic_loss_branches
                .into_iter()
                .map(|branch| branch.loss_resistance_ohm)
                .sum::<f64>();
        assert_relative_eq!(
            high[0][0].resistance_ohm,
            expected_high_resistance_ohm,
            max_relative = 1.0e-10
        );
        assert_relative_eq!(
            high[0][0].reactance_ohm / (std::f64::consts::TAU * high_frequency_hz),
            residual_self_inductance_h(config),
            max_relative = 1.0e-10
        );

        for frequency_hz in [0.0, 1.0, 20.0, 1_000.0, 20_000.0, 1.0e6] {
            let impedance = config
                .loaded_circuit_frequency_response_at_hz(frequency_hz)
                .unwrap()
                .coil_impedance_ohm;
            for sign in [-1.0, 1.0] {
                let modal_resistance_ohm =
                    impedance[0][0].resistance_ohm + sign * impedance[0][1].resistance_ohm;
                let modal_reactance_ohm =
                    impedance[0][0].reactance_ohm + sign * impedance[0][1].reactance_ohm;
                assert!(modal_resistance_ohm >= config.coil_resistance_ohm);
                if frequency_hz > 0.0 {
                    assert!(modal_reactance_ohm > 0.0);
                }
            }
            assert_eq!(impedance[0][0], impedance[1][1]);
            assert_eq!(impedance[0][1], impedance[1][0]);
        }
    }

    #[test]
    fn magnetic_loss_transfer_matrix_agrees_with_the_complex_circuit_equations() {
        let config = magnetic_loss_config(-0.37);
        for frequency_hz in [0.0, 20.0, 1_000.0, 12_000.0, 100_000.0] {
            let response = config
                .loaded_circuit_frequency_response_at_hz(frequency_hz)
                .unwrap();
            let direct = impedance_as_complex(response.coil_impedance_ohm[0][0]);
            let coupled = impedance_as_complex(response.coil_impedance_ohm[0][1]);
            let load = load_impedance_as_complex(config, frequency_hz);
            let diagonal = complex_add(direct, load);
            let determinant = complex_subtract(
                complex_multiply(diagonal, diagonal),
                complex_multiply(coupled, coupled),
            );
            let expected_direct = complex_multiply(load, complex_divide(diagonal, determinant));
            let expected_coupled = complex_multiply(
                load,
                complex_divide(
                    CartridgeComplexVoltageRatio {
                        real: -coupled.real,
                        imaginary: -coupled.imaginary,
                    },
                    determinant,
                ),
            );
            let actual = response.loaded_voltage_per_generator_voltage;
            assert_relative_eq!(actual[0][0].real, expected_direct.real, epsilon = 2.0e-15);
            assert_relative_eq!(
                actual[0][0].imaginary,
                expected_direct.imaginary,
                epsilon = 2.0e-15
            );
            assert_relative_eq!(actual[1][0].real, expected_coupled.real, epsilon = 2.0e-15);
            assert_relative_eq!(
                actual[1][0].imaginary,
                expected_coupled.imaginary,
                epsilon = 2.0e-15
            );
            assert_eq!(actual[0][0], actual[1][1]);
            assert_eq!(actual[0][1], actual[1][0]);
        }
    }

    #[test]
    fn time_domain_coupled_coil_response_matches_the_complex_transfer() {
        let config = mutual_config(0.42);
        for frequency_hz in [1_000.0, 12_000.0] {
            let measured = measure_cross_to_direct_response(config, frequency_hz);
            let effective_frequency_hz = TEST_SAMPLE_RATE_HZ / std::f64::consts::PI
                * (std::f64::consts::PI * frequency_hz / TEST_SAMPLE_RATE_HZ).tan();
            let response = config
                .loaded_circuit_frequency_response_at_hz(effective_frequency_hz)
                .unwrap();
            let total = response.loaded_voltage_per_magnet_velocity_v_s_per_m;
            let expected = complex_divide(total[1][0], total[0][0]);
            assert_relative_eq!(measured.real, expected.real, epsilon = 2.0e-10);
            assert_relative_eq!(measured.imaginary, expected.imaginary, epsilon = 2.0e-10);
            assert_eq!(
                measured,
                measure_cross_to_direct_response(config, frequency_hz)
            );
        }
    }

    #[test]
    fn time_domain_magnetic_loss_response_matches_the_warped_complex_transfer() {
        let config = magnetic_loss_config(0.42);
        for frequency_hz in [1_000.0, 12_000.0] {
            let measured = measure_cross_to_direct_response(config, frequency_hz);
            let effective_frequency_hz = TEST_SAMPLE_RATE_HZ / std::f64::consts::PI
                * (std::f64::consts::PI * frequency_hz / TEST_SAMPLE_RATE_HZ).tan();
            let total = config
                .loaded_circuit_frequency_response_at_hz(effective_frequency_hz)
                .unwrap()
                .loaded_voltage_per_magnet_velocity_v_s_per_m;
            let expected = complex_divide(total[1][0], total[0][0]);
            assert_relative_eq!(measured.real, expected.real, epsilon = 2.0e-10);
            assert_relative_eq!(measured.imaginary, expected.imaginary, epsilon = 2.0e-10);
        }
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
    fn coupled_coil_failures_roll_back_all_dynamic_state() {
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let mut cartridge = MovingMagnetCartridge::new(mutual_config(0.63)).unwrap();
        for sample in 0..2_000 {
            let phase = sample as f64 * 0.031;
            cartridge
                .advance(dt, [0.04 * phase.sin(), -0.03 * phase.cos()])
                .unwrap();
        }

        let before_nonfinite = cartridge.snapshot();
        assert_eq!(
            cartridge.advance(dt, [0.01, f64::INFINITY]),
            Err(MovingMagnetCartridgeError::InvalidMagnetVelocity { channel: 1 })
        );
        assert_eq!(cartridge.snapshot(), before_nonfinite);

        let stale = cartridge.prepare_affine_step(dt).unwrap();
        cartridge.advance(dt, [0.02, -0.01]).unwrap();
        let before_stale = cartridge.snapshot();
        assert_eq!(
            cartridge.commit_affine_step(stale, [0.0; 2]),
            Err(MovingMagnetCartridgeError::StaleAffineStep)
        );
        assert_eq!(cartridge.snapshot(), before_stale);

        let mut invalid_snapshot = before_stale;
        invalid_snapshot.config.coil_mutual_inductance_h =
            invalid_snapshot.config.coil_inductance_h;
        assert!(matches!(
            cartridge.restore(invalid_snapshot),
            Err(MovingMagnetCartridgeError::InvalidConfig(
                MovingMagnetCartridgeConfigError::InvalidMutualInductance { .. }
            ))
        ));
        assert_eq!(cartridge.snapshot(), before_stale);

        let mut old_snapshot = before_stale;
        old_snapshot.version = SNAPSHOT_VERSION - 1;
        assert_eq!(
            cartridge.restore(old_snapshot),
            Err(MovingMagnetCartridgeError::UnsupportedSnapshotVersion {
                version: SNAPSHOT_VERSION - 1,
            })
        );
        assert_eq!(cartridge.snapshot(), before_stale);
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
    fn coupled_coil_json_snapshot_continues_bit_identically() {
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let mut original = MovingMagnetCartridge::new(mutual_config(-0.58)).unwrap();
        for index in 0..3_000 {
            let phase = index as f64 * 0.031;
            original
                .advance(dt, [0.04 * phase.sin(), -0.03 * phase.cos()])
                .unwrap();
        }

        let encoded = serde_json::to_string(&original.snapshot()).unwrap();
        let decoded: MovingMagnetCartridgeSnapshot = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.version, SNAPSHOT_VERSION);
        assert_eq!(decoded.config.coil_mutual_inductance_h, -0.58 * 0.850);
        let mut restored = MovingMagnetCartridge::default();
        restored.restore(decoded).unwrap();
        for index in 0..5_000 {
            let phase = index as f64 * 0.017;
            let velocity = [0.07 * phase.cos(), 0.025 * (phase * 1.43).sin()];
            assert_eq!(
                original.advance(dt, velocity),
                restored.advance(dt, velocity)
            );
            assert_eq!(original.snapshot(), restored.snapshot());
        }
    }

    #[test]
    fn magnetic_loss_json_snapshot_and_rollback_preserve_all_branch_state() {
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let mut original = MovingMagnetCartridge::new(magnetic_loss_config(-0.58)).unwrap();
        for index in 0..3_000 {
            let phase = index as f64 * 0.031;
            original
                .advance(dt, [0.04 * phase.sin(), -0.03 * phase.cos()])
                .unwrap();
        }
        assert!(original
            .snapshot()
            .magnetic_loss_inductor_current_a
            .into_iter()
            .flatten()
            .any(|current| current != 0.0));

        let encoded = serde_json::to_string(&original.snapshot()).unwrap();
        let decoded: MovingMagnetCartridgeSnapshot = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.version, SNAPSHOT_VERSION);
        let mut restored = MovingMagnetCartridge::default();
        restored.restore(decoded).unwrap();
        for index in 0..5_000 {
            let phase = index as f64 * 0.017;
            let velocity = [0.07 * phase.cos(), 0.025 * (phase * 1.43).sin()];
            assert_eq!(
                original.advance(dt, velocity),
                restored.advance(dt, velocity)
            );
            assert_eq!(original.snapshot(), restored.snapshot());
        }

        let stale = restored.prepare_affine_step(dt).unwrap();
        restored.advance(dt, [0.02, -0.01]).unwrap();
        let before_stale = restored.snapshot();
        assert_eq!(
            restored.commit_affine_step(stale, [0.0; 2]),
            Err(MovingMagnetCartridgeError::StaleAffineStep)
        );
        assert_eq!(restored.snapshot(), before_stale);

        let mut invalid = before_stale;
        invalid.magnetic_loss_inductor_current_a[0][3] = 1.0e-6;
        assert_eq!(
            restored.restore(invalid),
            Err(MovingMagnetCartridgeError::InvalidSnapshot)
        );
        assert_eq!(restored.snapshot(), before_stale);
    }

    #[test]
    fn coupled_coil_realtime_steps_allocate_no_memory() {
        let mut cartridge = MovingMagnetCartridge::new(mutual_config(0.67)).unwrap();
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let mut completed = false;
        assert_no_alloc::assert_no_alloc(|| {
            for sample in 0..20_000 {
                let phase = sample as f64 * 0.071;
                cartridge
                    .advance(dt, [0.08 * phase.sin(), -0.06 * (phase * 1.19).cos()])
                    .unwrap();
            }
            completed = true;
        });
        assert!(completed);
    }

    #[test]
    fn magnetic_loss_realtime_steps_allocate_no_memory() {
        let mut cartridge = MovingMagnetCartridge::new(magnetic_loss_config(0.67)).unwrap();
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let mut completed = false;
        assert_no_alloc::assert_no_alloc(|| {
            for sample in 0..20_000 {
                let phase = sample as f64 * 0.071;
                cartridge
                    .advance(dt, [0.08 * phase.sin(), -0.06 * (phase * 1.19).cos()])
                    .unwrap();
            }
            completed = true;
        });
        assert!(completed);
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

        for coil_mutual_inductance_h in [f64::NAN, f64::INFINITY, 0.850, -0.850] {
            let config = MovingMagnetCartridgeConfig {
                coil_mutual_inductance_h,
                ..MovingMagnetCartridgeConfig::default()
            };
            assert!(matches!(
                config.validate(),
                Err(MovingMagnetCartridgeConfigError::InvalidMutualInductance { .. })
            ));
        }

        let boundary = MovingMagnetCartridgeConfig {
            coil_mutual_inductance_h: 0.850 - MIN_INDUCTANCE_H,
            ..MovingMagnetCartridgeConfig::default()
        };
        assert!(boundary.validate().is_ok());
    }

    #[test]
    fn magnetic_loss_configuration_enforces_bounds_slots_and_canonical_order() {
        let mut incomplete = MovingMagnetCartridgeConfig::default();
        incomplete.magnetic_loss_branches[0].relaxation_inductance_h = 0.1;
        assert_eq!(
            incomplete.validate(),
            Err(MovingMagnetCartridgeConfigError::IncompleteMagneticLossBranch { branch: 0 })
        );

        let mut nonfinite = MovingMagnetCartridgeConfig::default();
        nonfinite.magnetic_loss_branches[0] = MagneticLossRelaxationBranchConfig {
            relaxation_inductance_h: f64::NAN,
            loss_resistance_ohm: 1_000.0,
        };
        assert_eq!(
            nonfinite.validate(),
            Err(MovingMagnetCartridgeConfigError::InvalidMagneticLossBranch { branch: 0 })
        );

        let active = MagneticLossRelaxationBranchConfig {
            relaxation_inductance_h: 0.1,
            loss_resistance_ohm: 1_000.0,
        };
        let mut gap = MovingMagnetCartridgeConfig::default();
        gap.magnetic_loss_branches[0] = active;
        gap.magnetic_loss_branches[2] = MagneticLossRelaxationBranchConfig {
            relaxation_inductance_h: 0.2,
            loss_resistance_ohm: 1_000.0,
        };
        assert_eq!(
            gap.validate(),
            Err(MovingMagnetCartridgeConfigError::NonContiguousMagneticLossBranch { branch: 2 })
        );

        let mut unordered = MovingMagnetCartridgeConfig::default();
        unordered.magnetic_loss_branches[0] = active;
        unordered.magnetic_loss_branches[1] = MagneticLossRelaxationBranchConfig {
            relaxation_inductance_h: 0.01,
            loss_resistance_ohm: 1_000.0,
        };
        assert_eq!(
            unordered.validate(),
            Err(
                MovingMagnetCartridgeConfigError::NonCanonicalMagneticLossBranchOrder { branch: 1 }
            )
        );

        let mut excessive = MovingMagnetCartridgeConfig::default();
        excessive.magnetic_loss_branches[0] = MagneticLossRelaxationBranchConfig {
            relaxation_inductance_h: excessive.coil_inductance_h,
            loss_resistance_ohm: 1_000.0,
        };
        assert!(matches!(
            excessive.validate(),
            Err(MovingMagnetCartridgeConfigError::InvalidMagneticLossInductanceBudget { .. })
        ));
    }

    #[test]
    fn residual_modal_inductance_boundary_is_strictly_passive() {
        let mut boundary = MovingMagnetCartridgeConfig::default();
        boundary.magnetic_loss_branches[0] = MagneticLossRelaxationBranchConfig {
            relaxation_inductance_h: 0.2,
            loss_resistance_ohm: 2_000.0,
        };
        let residual_inductance_h = boundary.coil_inductance_h - 0.2;
        boundary.coil_mutual_inductance_h = residual_inductance_h - MIN_INDUCTANCE_H;
        assert!(boundary.validate().is_ok());

        let invalid = MovingMagnetCartridgeConfig {
            coil_mutual_inductance_h: residual_inductance_h,
            ..boundary
        };
        assert!(matches!(
            invalid.validate(),
            Err(MovingMagnetCartridgeConfigError::InvalidMutualInductance { .. })
        ));
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

    fn independent_circuit_reference(
        snapshot: MovingMagnetCartridgeSnapshot,
        duration_seconds: f64,
        magnet_velocity_m_s: [f64; 2],
    ) -> MovingMagnetCartridgeAffineOutput {
        assert_eq!(snapshot.config.coil_mutual_inductance_h, 0.0);
        let config = snapshot.config;
        let matrix = config.channel_matrix();
        let coefficient = config.generator_coefficient_v_s_per_m;
        let generator_voltage_per_velocity_v_s_per_m =
            matrix.map(|row| [coefficient * row[0], coefficient * row[1]]);
        let generator_voltage_v = matrix_vector(
            generator_voltage_per_velocity_v_s_per_m,
            magnet_velocity_m_s,
        );
        let mut coil_current_a = [0.0; 2];
        let mut load_output_voltage_v = [0.0; 2];
        for channel in 0..2 {
            let affine = trapezoidal_circuit_affine_step(
                config,
                duration_seconds,
                snapshot.coil_current_a[channel],
                snapshot.load_output_voltage_v[channel],
                snapshot.previous_generator_voltage_v[channel],
            )
            .unwrap();
            coil_current_a[channel] = affine.current_bias_a
                + affine.current_per_generator_voltage_a_per_v * generator_voltage_v[channel];
            load_output_voltage_v[channel] = affine.load_voltage_bias_v
                + affine.load_voltage_per_generator_voltage * generator_voltage_v[channel];
        }
        let interval_average_generator_voltage_v = [
            0.5 * (snapshot.previous_generator_voltage_v[0] + generator_voltage_v[0]),
            0.5 * (snapshot.previous_generator_voltage_v[1] + generator_voltage_v[1]),
        ];
        let interval_average_coil_current_a = [
            0.5 * (snapshot.coil_current_a[0] + coil_current_a[0]),
            0.5 * (snapshot.coil_current_a[1] + coil_current_a[1]),
        ];
        let interval_average_load_output_voltage_v = [
            0.5 * (snapshot.load_output_voltage_v[0] + load_output_voltage_v[0]),
            0.5 * (snapshot.load_output_voltage_v[1] + load_output_voltage_v[1]),
        ];
        let electromagnetic_reaction_force_n = [
            -coefficient
                * (matrix[0][0] * interval_average_coil_current_a[0]
                    + matrix[1][0] * interval_average_coil_current_a[1]),
            -coefficient
                * (matrix[0][1] * interval_average_coil_current_a[0]
                    + matrix[1][1] * interval_average_coil_current_a[1]),
        ];
        MovingMagnetCartridgeAffineOutput {
            magnet_velocity_m_s,
            generator_voltage_v,
            coil_current_a,
            magnetic_loss_inductor_current_a: [[0.0; MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES]; 2],
            load_output_voltage_v,
            interval_average_generator_voltage_v,
            interval_average_coil_current_a,
            interval_average_magnetic_loss_inductor_current_a: [[0.0;
                MAX_MAGNETIC_LOSS_RELAXATION_BRANCHES];
                2],
            interval_average_load_output_voltage_v,
            electromagnetic_reaction_force_n,
        }
    }

    fn assert_array_bits_eq(actual: [f64; 2], expected: [f64; 2]) {
        assert_eq!(actual.map(f64::to_bits), expected.map(f64::to_bits));
    }

    fn assert_matrix_bits_eq(actual: [[f64; 2]; 2], expected: [[f64; 2]; 2]) {
        for row in 0..2 {
            assert_array_bits_eq(actual[row], expected[row]);
        }
    }

    fn legacy_coupled_trapezoidal_circuit_affine_step(
        config: MovingMagnetCartridgeConfig,
        duration_seconds: f64,
        previous_current_a: [f64; 2],
        previous_load_voltage_v: [f64; 2],
        previous_generator_voltage_v: [f64; 2],
    ) -> Option<CoupledTrapezoidalCircuitAffineStep> {
        assert_eq!(magnetic_loss_branch_count(config), 0);
        if config.coil_mutual_inductance_h == 0.0 {
            let mut current_bias_a = [0.0; 2];
            let mut current_per_generator_voltage_a_per_v = [[0.0; 2]; 2];
            let mut load_voltage_bias_v = [0.0; 2];
            let mut load_voltage_per_generator_voltage = [[0.0; 2]; 2];
            for channel in 0..2 {
                let coefficients = trapezoidal_circuit_affine_step(
                    config,
                    duration_seconds,
                    previous_current_a[channel],
                    previous_load_voltage_v[channel],
                    previous_generator_voltage_v[channel],
                )?;
                current_bias_a[channel] = coefficients.current_bias_a;
                current_per_generator_voltage_a_per_v[channel][channel] =
                    coefficients.current_per_generator_voltage_a_per_v;
                load_voltage_bias_v[channel] = coefficients.load_voltage_bias_v;
                load_voltage_per_generator_voltage[channel][channel] =
                    coefficients.load_voltage_per_generator_voltage;
            }
            return Some(CoupledTrapezoidalCircuitAffineStep {
                current_bias_a,
                current_per_generator_voltage_a_per_v,
                load_voltage_bias_v,
                load_voltage_per_generator_voltage,
            });
        }

        let previous_current_modes_a = to_symmetric_modes(previous_current_a);
        let previous_load_voltage_modes_v = to_symmetric_modes(previous_load_voltage_v);
        let previous_generator_voltage_modes_v = to_symmetric_modes(previous_generator_voltage_v);
        let common = trapezoidal_circuit_affine_step_with_inductance(
            config,
            config.coil_inductance_h + config.coil_mutual_inductance_h,
            duration_seconds,
            previous_current_modes_a[0],
            previous_load_voltage_modes_v[0],
            previous_generator_voltage_modes_v[0],
        )?;
        let differential = trapezoidal_circuit_affine_step_with_inductance(
            config,
            config.coil_inductance_h - config.coil_mutual_inductance_h,
            duration_seconds,
            previous_current_modes_a[1],
            previous_load_voltage_modes_v[1],
            previous_generator_voltage_modes_v[1],
        )?;
        Some(CoupledTrapezoidalCircuitAffineStep {
            current_bias_a: from_symmetric_modes([
                common.current_bias_a,
                differential.current_bias_a,
            ]),
            current_per_generator_voltage_a_per_v: symmetric_modal_matrix(
                common.current_per_generator_voltage_a_per_v,
                differential.current_per_generator_voltage_a_per_v,
            ),
            load_voltage_bias_v: from_symmetric_modes([
                common.load_voltage_bias_v,
                differential.load_voltage_bias_v,
            ]),
            load_voltage_per_generator_voltage: symmetric_modal_matrix(
                common.load_voltage_per_generator_voltage,
                differential.load_voltage_per_generator_voltage,
            ),
        })
    }

    fn measure_cross_to_direct_response(
        config: MovingMagnetCartridgeConfig,
        frequency_hz: f64,
    ) -> CartridgeComplexVoltageRatio {
        let mut cartridge = MovingMagnetCartridge::new(config).unwrap();
        let dt = 1.0 / TEST_SAMPLE_RATE_HZ;
        let phase_step = std::f64::consts::TAU * frequency_hz * dt;
        let mut phase: f64 = 0.0;
        for _ in 0..8_192 {
            cartridge.advance(dt, [0.01 * phase.sin(), 0.0]).unwrap();
            phase += phase_step;
        }
        let mut direct_in_phase = 0.0;
        let mut direct_quadrature = 0.0;
        let mut cross_in_phase = 0.0;
        let mut cross_quadrature = 0.0;
        let sample_count = 19_200;
        for _ in 0..sample_count {
            let sine = phase.sin();
            let cosine = phase.cos();
            let output = cartridge.advance(dt, [0.01 * sine, 0.0]).unwrap();
            direct_in_phase += output.load_output_voltage_v[0] * sine;
            direct_quadrature += output.load_output_voltage_v[0] * cosine;
            cross_in_phase += output.load_output_voltage_v[1] * sine;
            cross_quadrature += output.load_output_voltage_v[1] * cosine;
            phase += phase_step;
        }
        complex_divide(
            CartridgeComplexVoltageRatio {
                real: cross_in_phase,
                imaginary: cross_quadrature,
            },
            CartridgeComplexVoltageRatio {
                real: direct_in_phase,
                imaginary: direct_quadrature,
            },
        )
    }

    fn complex_divide(
        numerator: CartridgeComplexVoltageRatio,
        denominator: CartridgeComplexVoltageRatio,
    ) -> CartridgeComplexVoltageRatio {
        let scale =
            denominator.real * denominator.real + denominator.imaginary * denominator.imaginary;
        CartridgeComplexVoltageRatio {
            real: (numerator.real * denominator.real + numerator.imaginary * denominator.imaginary)
                / scale,
            imaginary: (numerator.imaginary * denominator.real
                - numerator.real * denominator.imaginary)
                / scale,
        }
    }

    fn impedance_as_complex(
        impedance: CartridgeComplexImpedanceOhm,
    ) -> CartridgeComplexVoltageRatio {
        CartridgeComplexVoltageRatio {
            real: impedance.resistance_ohm,
            imaginary: impedance.reactance_ohm,
        }
    }

    fn load_impedance_as_complex(
        config: MovingMagnetCartridgeConfig,
        frequency_hz: f64,
    ) -> CartridgeComplexVoltageRatio {
        let conductance = 1.0 / config.load_resistance_ohm;
        let susceptance = std::f64::consts::TAU * frequency_hz * config.load_capacitance_f;
        let scale = conductance * conductance + susceptance * susceptance;
        CartridgeComplexVoltageRatio {
            real: conductance / scale,
            imaginary: -susceptance / scale,
        }
    }

    fn complex_add(
        left: CartridgeComplexVoltageRatio,
        right: CartridgeComplexVoltageRatio,
    ) -> CartridgeComplexVoltageRatio {
        CartridgeComplexVoltageRatio {
            real: left.real + right.real,
            imaginary: left.imaginary + right.imaginary,
        }
    }

    fn complex_subtract(
        left: CartridgeComplexVoltageRatio,
        right: CartridgeComplexVoltageRatio,
    ) -> CartridgeComplexVoltageRatio {
        CartridgeComplexVoltageRatio {
            real: left.real - right.real,
            imaginary: left.imaginary - right.imaginary,
        }
    }

    fn complex_multiply(
        left: CartridgeComplexVoltageRatio,
        right: CartridgeComplexVoltageRatio,
    ) -> CartridgeComplexVoltageRatio {
        CartridgeComplexVoltageRatio {
            real: left.real * right.real - left.imaginary * right.imaginary,
            imaginary: left.real * right.imaginary + left.imaginary * right.real,
        }
    }
}
