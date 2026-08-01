//! Same-sample coupling for the pickup mechanics and cartridge circuit.
//!
//! The cartridge supplies an affine reciprocal force for the current sample.
//! The contact solver includes that force before it solves wall constraints.
//! Both state machines commit only after both candidate steps succeed.

use thiserror::Error;

use super::{
    contact::{
        solve_coupled_deck_pickup_midpoint, CoupledDeckPickupError, MidpointPickupGeometry,
        StylusTangentialMode,
    },
    MovingMagnetCartridge, MovingMagnetCartridgeError, MovingMagnetCartridgeTelemetry,
    PickupElectromagneticForceRelation, PickupMechanicalError, PickupMechanicalInput,
    PickupMechanicalState, PickupMechanicalTelemetry,
};
use crate::{DeckMechanicalControl, DeckMechanicalState, DeckMechanicalTelemetry};

const INVERSE_SQRT_2: f64 = std::f64::consts::FRAC_1_SQRT_2;
const COIL_FROM_MECHANICAL: [[f64; 2]; 2] = [
    [INVERSE_SQRT_2, INVERSE_SQRT_2],
    [INVERSE_SQRT_2, -INVERSE_SQRT_2],
];

/// Reports one atomically committed coupled sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoupledPickupCartridgeTelemetry {
    pub pickup: PickupMechanicalTelemetry,
    pub cartridge: MovingMagnetCartridgeTelemetry,
}

/// Reports one atomically committed deck, pickup, and cartridge sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CoupledRecordPlayerStepTelemetry {
    pub(crate) mechanics: DeckMechanicalTelemetry,
    pub(crate) pickup: PickupMechanicalTelemetry,
    pub(crate) cartridge: MovingMagnetCartridgeTelemetry,
    pub(crate) tangential_mode: StylusTangentialMode,
    pub(crate) evaluated_branches: u32,
    pub(crate) attempted_linear_solves: u32,
}

impl CoupledPickupCartridgeTelemetry {
    pub fn magnet_velocity_m_s(self) -> [f64; 2] {
        let velocity = self.pickup.magnet_velocity_45_45_m_s();
        [velocity.0, velocity.1]
    }

    pub fn electromagnetic_mechanical_power_w(self) -> f64 {
        dot(
            self.pickup.electromagnetic_force_on_tip_n,
            self.pickup.electromagnetic_port_velocity_m_s,
        )
    }
}

/// Advances the pickup and cartridge through one same-sample implicit step.
///
/// Set `input.electromagnetic_force_n` to zero. The cartridge relation supplies
/// the complete electromagnetic force for this function.
pub fn process_coupled_pickup_cartridge(
    pickup: &mut PickupMechanicalState,
    cartridge: &mut MovingMagnetCartridge,
    input: PickupMechanicalInput,
) -> Result<CoupledPickupCartridgeTelemetry, CoupledPickupCartridgeError> {
    if input.electromagnetic_force_n != [0.0; 2] {
        return Err(CoupledPickupCartridgeError::ExternalElectromagneticForce);
    }
    let dt = 1.0 / pickup.sample_rate_hz();
    let affine = cartridge.prepare_affine_step(dt)?;
    let mechanical_bias = transform_vector_from_coil(affine.reaction_force_bias_n());
    let mechanical_damping = transform_damping_from_coil(affine.reciprocal_damping_n_s_per_m());
    let relation = PickupElectromagneticForceRelation::new(mechanical_bias, mechanical_damping)?;

    let mut next_pickup = *pickup;
    let pickup_telemetry = next_pickup.process_with_electromagnetic_relation(input, relation)?;
    let magnet_velocity = pickup_telemetry.magnet_velocity_45_45_m_s();
    let magnet_velocity = [magnet_velocity.0, magnet_velocity.1];
    let mut next_cartridge = *cartridge;
    let cartridge_telemetry = next_cartridge.commit_affine_step(affine, magnet_velocity)?;

    let expected_mechanical_force =
        transform_vector_from_coil(cartridge_telemetry.electromagnetic_reaction_force_n);
    if !vectors_nearly_equal(
        pickup_telemetry.electromagnetic_force_on_tip_n,
        expected_mechanical_force,
    ) {
        return Err(CoupledPickupCartridgeError::ReciprocityMismatch);
    }

    *pickup = next_pickup;
    *cartridge = next_cartridge;
    Ok(CoupledPickupCartridgeTelemetry {
        pickup: pickup_telemetry,
        cartridge: cartridge_telemetry,
    })
}

/// Advances the deck, frozen midpoint contact, pickup, and cartridge together.
///
/// This function commits all three states only after every constraint and
/// reciprocity check succeeds.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_coupled_record_player_midpoint(
    deck: &mut DeckMechanicalState,
    pickup: &mut PickupMechanicalState,
    cartridge: &mut MovingMagnetCartridge,
    duration_seconds: f64,
    deck_control: DeckMechanicalControl,
    geometry: MidpointPickupGeometry,
    previous_tangential_mode: StylusTangentialMode,
) -> Result<CoupledRecordPlayerStepTelemetry, CoupledRecordPlayerStepError> {
    if geometry.input.electromagnetic_force_n != [0.0; 2] {
        return Err(CoupledRecordPlayerStepError::ExternalElectromagneticForce);
    }
    let preparation = deck
        .prepare_midpoint_step(duration_seconds, deck_control)
        .map_err(CoupledDeckPickupError::from)?;
    let affine = cartridge.prepare_affine_step(duration_seconds)?;
    let mechanical_bias = transform_vector_from_coil(affine.reaction_force_bias_n());
    let mechanical_damping = transform_damping_from_coil(affine.reciprocal_damping_n_s_per_m());
    let relation = PickupElectromagneticForceRelation::new(mechanical_bias, mechanical_damping)?;
    let coupled = solve_coupled_deck_pickup_midpoint(
        preparation,
        *pickup,
        geometry,
        relation,
        previous_tangential_mode,
    )?;
    let magnet_velocity = coupled.pickup_telemetry.magnet_velocity_45_45_m_s();
    let magnet_velocity = [magnet_velocity.0, magnet_velocity.1];
    let mut next_cartridge = *cartridge;
    let cartridge_telemetry = next_cartridge.commit_affine_step(affine, magnet_velocity)?;
    let expected_mechanical_force =
        transform_vector_from_coil(cartridge_telemetry.electromagnetic_reaction_force_n);
    if !vectors_nearly_equal(
        coupled.pickup_telemetry.electromagnetic_force_on_tip_n,
        expected_mechanical_force,
    ) {
        return Err(CoupledRecordPlayerStepError::ReciprocityMismatch);
    }

    *deck = coupled.deck;
    *pickup = coupled.pickup;
    *cartridge = next_cartridge;
    Ok(CoupledRecordPlayerStepTelemetry {
        mechanics: deck.telemetry(),
        pickup: coupled.pickup_telemetry,
        cartridge: cartridge_telemetry,
        tangential_mode: coupled.tangential_mode,
        evaluated_branches: coupled.evaluated_branches,
        attempted_linear_solves: coupled.attempted_linear_solves,
    })
}

fn transform_vector_from_coil(coil: [f64; 2]) -> [f64; 2] {
    matrix_vector(COIL_FROM_MECHANICAL, coil)
}

fn transform_damping_from_coil(coil: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    let mut mechanical = [[0.0; 2]; 2];
    for row in 0..2 {
        for column in 0..2 {
            for coil_row in 0..2 {
                for coil_column in 0..2 {
                    mechanical[row][column] += COIL_FROM_MECHANICAL[coil_row][row]
                        * coil[coil_row][coil_column]
                        * COIL_FROM_MECHANICAL[coil_column][column];
                }
            }
        }
    }
    let symmetric_off_diagonal = 0.5 * (mechanical[0][1] + mechanical[1][0]);
    mechanical[0][1] = symmetric_off_diagonal;
    mechanical[1][0] = symmetric_off_diagonal;
    mechanical
}

fn matrix_vector(matrix: [[f64; 2]; 2], vector: [f64; 2]) -> [f64; 2] {
    [
        matrix[0][0] * vector[0] + matrix[0][1] * vector[1],
        matrix[1][0] * vector[0] + matrix[1][1] * vector[1],
    ]
}

fn dot(left: [f64; 2], right: [f64; 2]) -> f64 {
    left[0] * right[0] + left[1] * right[1]
}

fn vectors_nearly_equal(left: [f64; 2], right: [f64; 2]) -> bool {
    (0..2).all(|axis| {
        let scale = left[axis].abs().max(right[axis].abs()).max(1.0e-30);
        (left[axis] - right[axis]).abs() <= 1.0e-10 * scale + 1.0e-18
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum CoupledPickupCartridgeError {
    #[error(transparent)]
    Pickup(#[from] PickupMechanicalError),
    #[error(transparent)]
    Cartridge(#[from] MovingMagnetCartridgeError),
    #[error("the coupled input must not contain a separate electromagnetic force")]
    ExternalElectromagneticForce,
    #[error("the mechanical and electrical reciprocal forces do not match")]
    ReciprocityMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum CoupledRecordPlayerStepError {
    #[error(transparent)]
    Coupling(#[from] CoupledDeckPickupError),
    #[error(transparent)]
    Pickup(#[from] PickupMechanicalError),
    #[error(transparent)]
    Cartridge(#[from] MovingMagnetCartridgeError),
    #[error("the coupled input must not contain a separate electromagnetic force")]
    ExternalElectromagneticForce,
    #[error("the mechanical and electrical reciprocal forces do not match")]
    ReciprocityMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physical::contact::test_single_wall_contacts;
    use crate::physical::stylus::{
        StylusTraceContact, StylusTraceContactSet, MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL,
    };
    use crate::physical::{MovingMagnetCartridgeConfig, StylusContactConfig, TonearmConfig};

    const SAMPLE_RATE_HZ: f64 = 192_000.0;

    fn states() -> (PickupMechanicalState, MovingMagnetCartridge) {
        (
            PickupMechanicalState::new(
                StylusContactConfig::default(),
                TonearmConfig::default(),
                SAMPLE_RATE_HZ,
            )
            .unwrap(),
            MovingMagnetCartridge::new(MovingMagnetCartridgeConfig::default()).unwrap(),
        )
    }

    fn record_player_states() -> (
        DeckMechanicalState,
        PickupMechanicalState,
        MovingMagnetCartridge,
    ) {
        let mut deck = DeckMechanicalState::new(crate::PhysicalDeckConfig::default()).unwrap();
        deck.reset(20.0, 20.0, 0.0, 0.0).unwrap();
        let (pickup, cartridge) = states();
        (deck, pickup, cartridge)
    }

    fn midpoint_geometry(input: PickupMechanicalInput) -> MidpointPickupGeometry {
        MidpointPickupGeometry {
            input,
            lateral_origin_shift_bias_m: 0.0,
            lateral_origin_shift_per_record_velocity_m_s: 0.0,
        }
    }

    fn symmetric_bridge(center_displacement_m: f64, slope: f64) -> StylusTraceContactSet {
        let mut contacts = [StylusTraceContact::default(); MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL];
        contacts[0] = StylusTraceContact {
            contact_offset_m: -1.754_028e-6,
            groove_displacement_m: center_displacement_m,
            groove_slope: slope,
            tangent_residual: 0.0,
        };
        contacts[1] = StylusTraceContact {
            contact_offset_m: 1.754_028e-6,
            groove_displacement_m: center_displacement_m,
            groove_slope: -slope,
            tangent_residual: 0.0,
        };
        StylusTraceContactSet {
            center_displacement_m,
            contact_count: 2,
            contacts,
        }
    }

    fn signal(sample: usize) -> PickupMechanicalInput {
        let phase = std::f64::consts::TAU * sample as f64 / 197.5;
        PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts(
                [2.0e-6 * phase.sin(), 1.3e-6 * (phase * 1.31).cos()],
                [0.03 * phase.cos(), -0.02 * (phase * 0.7).sin()],
            ),
            groove_tangential_velocity_m_s: if sample % 211 < 105 { 0.7 } else { -0.7 },
            electromagnetic_force_n: [0.0; 2],
            ..PickupMechanicalInput::default()
        }
    }

    #[test]
    fn first_impulse_has_same_sample_electrical_force() {
        let (mut pickup, mut cartridge) = states();
        let telemetry = process_coupled_pickup_cartridge(
            &mut pickup,
            &mut cartridge,
            PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts([2.0e-6, -2.0e-6], [0.0; 2]),
                electromagnetic_force_n: [0.0; 2],
                ..PickupMechanicalInput::default()
            },
        )
        .unwrap();
        assert_eq!(telemetry.pickup.completed_steps, 1);
        assert_eq!(telemetry.cartridge.completed_steps, 1);
        assert_ne!(telemetry.cartridge.generator_voltage_v, [0.0; 2]);
        assert_ne!(telemetry.pickup.electromagnetic_force_on_tip_n, [0.0; 2]);
        assert_eq!(
            telemetry.cartridge.magnet_velocity_m_s,
            telemetry.magnet_velocity_m_s()
        );
    }

    #[test]
    fn a_one_sample_reversal_uses_the_new_velocity_without_delay() {
        let (mut pickup, mut cartridge) = states();
        let positive = process_coupled_pickup_cartridge(
            &mut pickup,
            &mut cartridge,
            PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts([2.0e-6, -2.0e-6], [0.0; 2]),
                electromagnetic_force_n: [0.0; 2],
                ..PickupMechanicalInput::default()
            },
        )
        .unwrap();
        let negative = process_coupled_pickup_cartridge(
            &mut pickup,
            &mut cartridge,
            PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts([-2.0e-6, 2.0e-6], [0.0; 2]),
                electromagnetic_force_n: [0.0; 2],
                ..PickupMechanicalInput::default()
            },
        )
        .unwrap();
        assert!(positive.magnet_velocity_m_s()[0] > 0.0);
        assert!(negative.magnet_velocity_m_s()[0] < 0.0);
        assert!(positive.cartridge.generator_voltage_v[0] > 0.0);
        assert!(negative.cartridge.generator_voltage_v[0] < 0.0);
        assert_eq!(
            negative.cartridge.magnet_velocity_m_s,
            negative.magnet_velocity_m_s()
        );
    }

    #[test]
    fn reciprocal_damping_is_passive_for_adversarial_velocities() {
        let (_, cartridge) = states();
        let affine = cartridge.prepare_affine_step(1.0 / SAMPLE_RATE_HZ).unwrap();
        let damping = affine.reciprocal_damping_n_s_per_m();
        let mut random = 0x5a17_f00d_cafe_babe_u64;
        for _ in 0..20_000 {
            random = random
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let a = ((random >> 11) as f64 / ((1_u64 << 53) as f64) - 0.5) * 2.0e3;
            random = random
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let b = ((random >> 11) as f64 / ((1_u64 << 53) as f64) - 0.5) * 2.0e3;
            let velocity = [a, b];
            let damping_power = -dot(velocity, matrix_vector(damping, velocity));
            assert!(damping_power <= 1.0e-18, "{damping_power}");
        }
    }

    #[test]
    fn mechanical_and_generator_power_have_opposite_signs() {
        let (mut pickup, mut cartridge) = states();
        for sample in 0..10_000 {
            let telemetry =
                process_coupled_pickup_cartridge(&mut pickup, &mut cartridge, signal(sample))
                    .unwrap();
            let scale = telemetry
                .electromagnetic_mechanical_power_w()
                .abs()
                .max(telemetry.cartridge.generator_electrical_power_w.abs())
                .max(1.0e-24);
            assert!(
                (telemetry.electromagnetic_mechanical_power_w()
                    + telemetry.cartridge.generator_electrical_power_w)
                    .abs()
                    <= 1.0e-10 * scale + 1.0e-18
            );
        }
    }

    #[test]
    fn partitions_and_snapshots_preserve_exact_continuation() {
        let inputs = (0..4_096).map(signal).collect::<Vec<_>>();
        let (mut whole_pickup, mut whole_cartridge) = states();
        let mut whole = Vec::with_capacity(inputs.len());
        for input in &inputs {
            whole.push(
                process_coupled_pickup_cartridge(&mut whole_pickup, &mut whole_cartridge, *input)
                    .unwrap(),
            );
        }

        let (mut split_pickup, mut split_cartridge) = states();
        let mut split = Vec::with_capacity(inputs.len());
        let mut offset = 0;
        for block in [1, 31, 7, 509, 2, 127].into_iter().cycle() {
            if offset == inputs.len() {
                break;
            }
            let end = (offset + block).min(inputs.len());
            for input in &inputs[offset..end] {
                split.push(
                    process_coupled_pickup_cartridge(
                        &mut split_pickup,
                        &mut split_cartridge,
                        *input,
                    )
                    .unwrap(),
                );
            }
            offset = end;
        }
        assert_eq!(split, whole);
        assert_eq!(split_pickup.snapshot(), whole_pickup.snapshot());
        assert_eq!(split_cartridge.snapshot(), whole_cartridge.snapshot());

        let pickup_snapshot = split_pickup.snapshot();
        let cartridge_snapshot = split_cartridge.snapshot();
        let mut restored_pickup = states().0;
        let mut restored_cartridge = states().1;
        restored_pickup.restore(pickup_snapshot).unwrap();
        restored_cartridge.restore(cartridge_snapshot).unwrap();
        for sample in 4_096..5_000 {
            let expected = process_coupled_pickup_cartridge(
                &mut split_pickup,
                &mut split_cartridge,
                signal(sample),
            )
            .unwrap();
            let actual = process_coupled_pickup_cartridge(
                &mut restored_pickup,
                &mut restored_cartridge,
                signal(sample),
            )
            .unwrap();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn failures_leave_both_states_unchanged() {
        let (mut pickup, mut cartridge) = states();
        let pickup_before = pickup.snapshot();
        let cartridge_before = cartridge.snapshot();
        let error = process_coupled_pickup_cartridge(
            &mut pickup,
            &mut cartridge,
            PickupMechanicalInput {
                electromagnetic_force_n: [1.0, 0.0],
                ..PickupMechanicalInput::default()
            },
        )
        .unwrap_err();
        assert_eq!(
            error,
            CoupledPickupCartridgeError::ExternalElectromagneticForce
        );
        assert_eq!(pickup.snapshot(), pickup_before);
        assert_eq!(cartridge.snapshot(), cartridge_before);

        let error = process_coupled_pickup_cartridge(
            &mut pickup,
            &mut cartridge,
            PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts([f64::NAN, 0.0], [0.0; 2]),
                electromagnetic_force_n: [0.0; 2],
                ..PickupMechanicalInput::default()
            },
        )
        .unwrap_err();
        assert!(matches!(error, CoupledPickupCartridgeError::Pickup(_)));
        assert_eq!(pickup.snapshot(), pickup_before);
        assert_eq!(cartridge.snapshot(), cartridge_before);
    }

    #[test]
    fn record_player_midpoint_commits_reciprocal_deck_and_cartridge_ports() {
        let (mut deck, mut pickup, mut cartridge) = record_player_states();
        let telemetry = process_coupled_record_player_midpoint(
            &mut deck,
            &mut pickup,
            &mut cartridge,
            1.0 / SAMPLE_RATE_HZ,
            DeckMechanicalControl::default(),
            midpoint_geometry(PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts([0.0; 2], [0.3, -0.2]),
                electromagnetic_force_n: [0.0; 2],
                ..PickupMechanicalInput::default()
            }),
            StylusTangentialMode::SlidingPositive,
        )
        .unwrap();

        assert_eq!(
            telemetry.mechanics.mechanical_time_seconds,
            1.0 / SAMPLE_RATE_HZ
        );
        assert_eq!(telemetry.pickup.completed_steps, 1);
        assert_eq!(telemetry.cartridge.completed_steps, 1);
        assert_eq!(
            telemetry.mechanics.stylus_torque_nm,
            telemetry.pickup.record_reaction_torque_nm()
        );
        let expected_mechanical_force =
            transform_vector_from_coil(telemetry.cartridge.electromagnetic_reaction_force_n);
        assert!(vectors_nearly_equal(
            telemetry.pickup.electromagnetic_force_on_tip_n,
            expected_mechanical_force,
        ));
        let mechanical_power = dot(
            telemetry.pickup.electromagnetic_force_on_tip_n,
            telemetry.pickup.electromagnetic_port_velocity_m_s,
        );
        let power_scale = mechanical_power
            .abs()
            .max(telemetry.cartridge.generator_electrical_power_w.abs())
            .max(1.0e-24);
        assert!(
            (mechanical_power + telemetry.cartridge.generator_electrical_power_w).abs()
                <= 1.0e-10 * power_scale + 1.0e-18
        );
    }

    #[test]
    fn record_player_midpoint_commits_a_symmetric_bridge_atomically() {
        let (mut deck, mut pickup, mut cartridge) = record_player_states();
        let mut input = PickupMechanicalInput::default();
        input
            .set_certified_reflection_symmetric_wall_contacts([symmetric_bridge(2.0e-6, 0.25); 2])
            .unwrap();
        let telemetry = assert_no_alloc::assert_no_alloc(|| {
            process_coupled_record_player_midpoint(
                &mut deck,
                &mut pickup,
                &mut cartridge,
                1.0 / SAMPLE_RATE_HZ,
                DeckMechanicalControl::default(),
                midpoint_geometry(input),
                StylusTangentialMode::SlidingPositive,
            )
            .unwrap()
        });
        for wall in telemetry.pickup.wall_longitudinal_contact {
            assert_eq!(wall.contact_count(), 2);
            assert_eq!(
                wall.projected_normal_force_n[0],
                wall.projected_normal_force_n[1]
            );
        }
        assert_eq!(
            telemetry.mechanics.stylus_torque_nm,
            telemetry.pickup.longitudinal_record_reaction_torque_nm()
        );
        assert_eq!(
            telemetry.pickup.record_reaction_force_tangent_n,
            telemetry
                .pickup
                .longitudinal_record_reaction_force_tangent_n()
        );
        assert!(telemetry.pickup.tangential_friction_power_w <= 0.0);
    }

    #[test]
    fn record_player_midpoint_failure_is_transactional() {
        let (mut deck, mut pickup, mut cartridge) = record_player_states();
        let deck_before = deck;
        let pickup_before = pickup;
        let cartridge_before = cartridge;
        let error = process_coupled_record_player_midpoint(
            &mut deck,
            &mut pickup,
            &mut cartridge,
            1.0 / SAMPLE_RATE_HZ,
            DeckMechanicalControl::default(),
            midpoint_geometry(PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts([f64::NAN, 0.0], [0.0; 2]),
                electromagnetic_force_n: [0.0; 2],
                ..PickupMechanicalInput::default()
            }),
            StylusTangentialMode::Separated,
        )
        .unwrap_err();

        assert!(matches!(error, CoupledRecordPlayerStepError::Coupling(_)));
        assert_eq!(deck, deck_before);
        assert_eq!(pickup, pickup_before);
        assert_eq!(cartridge, cartridge_before);
    }
}
