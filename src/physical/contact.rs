use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::stylus::{StylusTraceContactSet, MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL};
use super::TonearmConfig;
use crate::mechanics::{
    coupled_deck_mode_is_valid, CoupledDeckFrictionMode, DeckMechanicalError, DeckMechanicalState,
    DeckMidpointPreparation, DeckMidpointSolution,
};

const SNAPSHOT_VERSION: u32 = 3;
const MIN_SAMPLE_RATE_HZ: f64 = 44_100.0;
const MAX_SAMPLE_RATE_HZ: f64 = 768_000.0;
const MIN_MOVING_MASS_KG: f64 = 1.0e-9;
const MAX_MOVING_MASS_KG: f64 = 1.0e-2;
const CONTACT_TOLERANCE_M: f64 = 1.0e-11;
const BEARING_VELOCITY_TOLERANCE_M_S: f64 = 1.0e-10;
const TANGENTIAL_FORCE_TOLERANCE_N: f64 = 1.0e-10;
const INVERSE_SQRT_2: f64 = std::f64::consts::FRAC_1_SQRT_2;
const WALL_NORMALS: [[f64; 2]; 2] = [
    [INVERSE_SQRT_2, INVERSE_SQRT_2],
    [-INVERSE_SQRT_2, INVERSE_SQRT_2],
];

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StylusContactConfig {
    pub moving_mass_kg: f64,
    pub groove_friction_coefficient: f64,
    pub record_surface_friction_coefficient: f64,
}

impl StylusContactConfig {
    /// This seed contains an estimated moving mass and friction coefficient.
    pub fn concorde_mkii_scratch_seed() -> Self {
        Self {
            moving_mass_kg: 0.60e-6,
            groove_friction_coefficient: 0.25,
            // This coefficient needs a direct measurement for each material pair.
            record_surface_friction_coefficient: 0.20,
        }
    }

    pub fn validate(self) -> Result<Self, PickupMechanicalError> {
        if !self.moving_mass_kg.is_finite()
            || !(MIN_MOVING_MASS_KG..=MAX_MOVING_MASS_KG).contains(&self.moving_mass_kg)
        {
            return Err(PickupMechanicalError::InvalidConfig {
                field: "movingMassKg",
            });
        }
        if !self.groove_friction_coefficient.is_finite()
            || !(0.0..=2.0).contains(&self.groove_friction_coefficient)
        {
            return Err(PickupMechanicalError::InvalidConfig {
                field: "grooveFrictionCoefficient",
            });
        }
        if !self.record_surface_friction_coefficient.is_finite()
            || !(0.0..=2.0).contains(&self.record_surface_friction_coefficient)
        {
            return Err(PickupMechanicalError::InvalidConfig {
                field: "recordSurfaceFrictionCoefficient",
            });
        }
        Ok(self)
    }
}

/// Selects the record feature below the stylus for one fixed sample.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PickupContactSurface {
    None,
    #[default]
    GrooveWalls,
    RecordLand,
}

/// Identifies the fixed tangential contact branch for one pickup sample.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StylusTangentialMode {
    #[default]
    Separated,
    Sticking,
    SlidingPositive,
    SlidingNegative,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WallContactQualification {
    #[default]
    Unique,
    CertifiedReflectionSymmetry,
}

impl Default for StylusContactConfig {
    fn default() -> Self {
        Self::concorde_mkii_scratch_seed()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickupMechanicalInput {
    /// Wall coordinates use the two inward 45-degree normals.
    pub wall_contacts: [StylusTraceContactSet; 2],
    #[serde(skip)]
    pub(crate) wall_contact_qualification: [WallContactQualification; 2],
    /// The land plane uses the vertical pickup coordinate.
    pub land_displacement_m: f64,
    pub contact_surface: PickupContactSurface,
    pub groove_radius_m: f64,
    pub groove_tangential_velocity_m_s: f64,
    pub stylus_lowered: bool,
    /// This force acts on the moving magnet. The opposite force acts on the body.
    pub electromagnetic_force_n: [f64; 2],
}

impl Default for PickupMechanicalInput {
    fn default() -> Self {
        Self {
            wall_contacts: [StylusTraceContactSet::default(); 2],
            wall_contact_qualification: [WallContactQualification::Unique; 2],
            land_displacement_m: 25.0e-6,
            contact_surface: PickupContactSurface::GrooveWalls,
            groove_radius_m: 0.146_05,
            groove_tangential_velocity_m_s: 0.0,
            stylus_lowered: true,
            electromagnetic_force_n: [0.0; 2],
        }
    }
}

impl PickupMechanicalInput {
    pub fn set_unique_wall_contacts(
        &mut self,
        wall_contacts: [StylusTraceContactSet; 2],
    ) -> Result<(), PickupMechanicalError> {
        if wall_contacts
            .into_iter()
            .any(|contacts| contacts.contact_count != 1 || !wall_contact_set_is_valid(contacts))
        {
            return Err(PickupMechanicalError::UnqualifiedMultipleContacts);
        }
        self.wall_contacts = wall_contacts;
        self.wall_contact_qualification = [WallContactQualification::Unique; 2];
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_certified_reflection_symmetric_wall_contacts(
        &mut self,
        wall_contacts: [StylusTraceContactSet; 2],
    ) -> Result<(), PickupMechanicalError> {
        if wall_contacts
            .into_iter()
            .any(|contacts| !reflection_symmetry_is_valid(contacts))
        {
            return Err(PickupMechanicalError::InvalidCommonHeightCertificate);
        }
        self.wall_contacts = wall_contacts;
        self.wall_contact_qualification =
            [WallContactQualification::CertifiedReflectionSymmetry; 2];
        Ok(())
    }
}

fn wall_contact_count(set: StylusTraceContactSet) -> Option<usize> {
    let count = usize::from(set.contact_count);
    (1..=MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL)
        .contains(&count)
        .then_some(count)
}

fn wall_displacement_m(input: PickupMechanicalInput) -> [f64; 2] {
    input.wall_contacts.map(|set| set.center_displacement_m)
}

fn wall_effective_slope(input: PickupMechanicalInput) -> [f64; 2] {
    input.wall_contacts.map(|set| {
        let count = wall_contact_count(set).unwrap_or(1);
        set.contacts[..count]
            .iter()
            .map(|contact| contact.groove_slope)
            .sum::<f64>()
            / count as f64
    })
}

fn wall_effective_surface_normal_scale(input: PickupMechanicalInput) -> [f64; 2] {
    input.wall_contacts.map(|set| {
        let count = wall_contact_count(set).unwrap_or(1);
        set.contacts[..count]
            .iter()
            .map(|contact| contact.groove_slope.hypot(1.0))
            .sum::<f64>()
            / count as f64
    })
}

fn distribute_wall_contact_forces(
    input: PickupMechanicalInput,
    wall_projected_force_n: [f64; 2],
    coulomb_friction_force_n: f64,
) -> [PickupWallContactTelemetry; 2] {
    // The reduced pickup has no stylus rotation or local contact compliance.
    // Its certified common-height rigid constraint has non-unique multipliers. The
    // equal projected-force split is the unique minimum-Euclidean-norm split.
    // The final contact receives the arithmetic residual for exact accounting.
    let mut telemetry = input
        .wall_contacts
        .map(|geometry| PickupWallContactTelemetry {
            geometry,
            ..PickupWallContactTelemetry::default()
        });
    let mut total_surface_normal_force_n = 0.0;
    let mut final_loaded_contact = None;
    for wall in 0..2 {
        let set = input.wall_contacts[wall];
        let count = wall_contact_count(set).unwrap_or(1);
        let projected_share_n = wall_projected_force_n[wall] / count as f64;
        let mut assigned_projected_force_n = 0.0;
        for contact_index in 0..count {
            let projected_force_n = if contact_index + 1 == count {
                wall_projected_force_n[wall] - assigned_projected_force_n
            } else {
                projected_share_n
            };
            assigned_projected_force_n += projected_force_n;
            let surface_normal_force_n =
                projected_force_n * set.contacts[contact_index].groove_slope.hypot(1.0);
            let modulation_reaction_force_n =
                -projected_force_n * set.contacts[contact_index].groove_slope;
            telemetry[wall].projected_normal_force_n[contact_index] = projected_force_n;
            telemetry[wall].surface_normal_force_n[contact_index] = surface_normal_force_n;
            telemetry[wall].modulation_reaction_force_n[contact_index] =
                modulation_reaction_force_n;
            total_surface_normal_force_n += surface_normal_force_n;
            if surface_normal_force_n > 0.0 {
                final_loaded_contact = Some((wall, contact_index));
            }
        }
    }
    if total_surface_normal_force_n > 0.0 && coulomb_friction_force_n != 0.0 {
        let mut assigned_friction_force_n = 0.0;
        for wall in 0..2 {
            let count = wall_contact_count(input.wall_contacts[wall]).unwrap_or(1);
            for contact_index in 0..count {
                let friction_force_n = if final_loaded_contact == Some((wall, contact_index)) {
                    coulomb_friction_force_n - assigned_friction_force_n
                } else {
                    coulomb_friction_force_n * telemetry[wall].surface_normal_force_n[contact_index]
                        / total_surface_normal_force_n
                };
                telemetry[wall].coulomb_friction_force_n[contact_index] = friction_force_n;
                assigned_friction_force_n += friction_force_n;
            }
        }
    }
    let target_record_reaction_force_tangent_n = telemetry
        .into_iter()
        .flat_map(|wall| wall.modulation_reaction_force_n)
        .sum::<f64>()
        + coulomb_friction_force_n;
    let mut assigned_record_reaction_force_tangent_n = 0.0;
    for wall in 0..2 {
        let count = wall_contact_count(input.wall_contacts[wall]).unwrap_or(1);
        for contact_index in 0..count {
            let reaction_force_n = if final_loaded_contact == Some((wall, contact_index)) {
                target_record_reaction_force_tangent_n - assigned_record_reaction_force_tangent_n
            } else {
                telemetry[wall].modulation_reaction_force_n[contact_index]
                    + telemetry[wall].coulomb_friction_force_n[contact_index]
            };
            telemetry[wall].record_reaction_force_tangent_n[contact_index] = reaction_force_n;
            assigned_record_reaction_force_tangent_n += reaction_force_n;
        }
    }
    telemetry
}

fn sum_wall_contact_field(
    telemetry: [PickupWallContactTelemetry; 2],
    select: impl Fn(PickupWallContactTelemetry) -> [f64; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
) -> f64 {
    telemetry.into_iter().flat_map(select).sum::<f64>()
}

#[cfg(test)]
pub(crate) fn test_single_wall_contacts(
    center_displacement_m: [f64; 2],
    groove_slope: [f64; 2],
) -> [StylusTraceContactSet; 2] {
    [0, 1].map(|wall| {
        let mut set = StylusTraceContactSet {
            center_displacement_m: center_displacement_m[wall],
            ..StylusTraceContactSet::default()
        };
        set.contacts[0].groove_displacement_m = center_displacement_m[wall];
        set.contacts[0].groove_slope = groove_slope[wall];
        set
    })
}

/// Reports the force distribution for all same-wall longitudinal contacts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickupWallContactTelemetry {
    pub geometry: StylusTraceContactSet,
    /// This force uses the 45-degree wall-normal projection.
    pub projected_normal_force_n: [f64; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
    /// This force uses each local three-dimensional surface normal.
    pub surface_normal_force_n: [f64; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
    pub modulation_reaction_force_n: [f64; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
    pub coulomb_friction_force_n: [f64; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
    pub record_reaction_force_tangent_n: [f64; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
}

impl Default for PickupWallContactTelemetry {
    fn default() -> Self {
        Self {
            geometry: StylusTraceContactSet::default(),
            projected_normal_force_n: [0.0; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
            surface_normal_force_n: [0.0; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
            modulation_reaction_force_n: [0.0; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
            coulomb_friction_force_n: [0.0; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
            record_reaction_force_tangent_n: [0.0; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
        }
    }
}

impl PickupWallContactTelemetry {
    pub fn contact_count(self) -> usize {
        usize::from(self.geometry.contact_count)
    }

    pub fn total_projected_normal_force_n(self) -> f64 {
        self.projected_normal_force_n.into_iter().sum()
    }

    pub fn total_surface_normal_force_n(self) -> f64 {
        self.surface_normal_force_n.into_iter().sum()
    }

    pub fn total_modulation_reaction_force_n(self) -> f64 {
        self.modulation_reaction_force_n.into_iter().sum()
    }

    pub fn total_coulomb_friction_force_n(self) -> f64 {
        self.coulomb_friction_force_n.into_iter().sum()
    }

    pub fn total_record_reaction_force_tangent_n(self) -> f64 {
        self.record_reaction_force_tangent_n.into_iter().sum()
    }
}

/// Defines one same-sample electromagnetic force relation.
///
/// Force uses lateral and vertical coordinates. The relation is
/// `force = bias - damping * relative_velocity`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickupElectromagneticForceRelation {
    force_bias_n: [f64; 2],
    reciprocal_damping_n_s_per_m: [[f64; 2]; 2],
}

impl PickupElectromagneticForceRelation {
    pub fn new(
        force_bias_n: [f64; 2],
        reciprocal_damping_n_s_per_m: [[f64; 2]; 2],
    ) -> Result<Self, PickupMechanicalError> {
        if force_bias_n
            .into_iter()
            .chain(reciprocal_damping_n_s_per_m.into_iter().flatten())
            .any(|value| !value.is_finite())
        {
            return Err(PickupMechanicalError::InvalidElectromagneticRelation);
        }
        let asymmetry =
            (reciprocal_damping_n_s_per_m[0][1] - reciprocal_damping_n_s_per_m[1][0]).abs();
        let symmetry_scale = reciprocal_damping_n_s_per_m[0][1]
            .abs()
            .max(reciprocal_damping_n_s_per_m[1][0].abs())
            .max(1.0);
        if asymmetry > 32.0 * f64::EPSILON * symmetry_scale {
            return Err(PickupMechanicalError::InvalidElectromagneticRelation);
        }
        let mut off_diagonal =
            0.5 * (reciprocal_damping_n_s_per_m[0][1] + reciprocal_damping_n_s_per_m[1][0]);
        let mut damping = [
            [reciprocal_damping_n_s_per_m[0][0], off_diagonal],
            [off_diagonal, reciprocal_damping_n_s_per_m[1][1]],
        ];
        if damping[0][0] < 0.0 || damping[1][1] < 0.0 {
            return Err(PickupMechanicalError::InvalidElectromagneticRelation);
        }
        let maximum_passive_off_diagonal = damping[0][0].sqrt() * damping[1][1].sqrt();
        if off_diagonal.abs() > maximum_passive_off_diagonal {
            let excess = off_diagonal.abs() - maximum_passive_off_diagonal;
            let roundoff_tolerance =
                64.0 * f64::EPSILON * off_diagonal.abs().max(maximum_passive_off_diagonal);
            if excess > roundoff_tolerance {
                return Err(PickupMechanicalError::InvalidElectromagneticRelation);
            }
            off_diagonal = off_diagonal.signum() * maximum_passive_off_diagonal;
            damping[0][1] = off_diagonal;
            damping[1][0] = off_diagonal;
        }
        Ok(Self {
            force_bias_n,
            reciprocal_damping_n_s_per_m: damping,
        })
    }

    pub fn constant(force_n: [f64; 2]) -> Result<Self, PickupMechanicalError> {
        Self::new(force_n, [[0.0; 2]; 2])
    }

    pub const fn force_bias_n(self) -> [f64; 2] {
        self.force_bias_n
    }

    pub const fn reciprocal_damping_n_s_per_m(self) -> [[f64; 2]; 2] {
        self.reciprocal_damping_n_s_per_m
    }

    pub fn force_at_relative_velocity(
        self,
        relative_velocity_m_s: [f64; 2],
    ) -> Result<[f64; 2], PickupMechanicalError> {
        if relative_velocity_m_s
            .into_iter()
            .any(|value| !value.is_finite())
        {
            return Err(PickupMechanicalError::InvalidElectromagneticRelation);
        }
        let damping_force = matrix_vector(self.reciprocal_damping_n_s_per_m, relative_velocity_m_s);
        let force = subtract(self.force_bias_n, damping_force);
        force
            .into_iter()
            .all(f64::is_finite)
            .then_some(force)
            .ok_or(PickupMechanicalError::NumericalFailure)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickupMechanicalTelemetry {
    pub tip_displacement_m: [f64; 2],
    pub tip_velocity_m_s: [f64; 2],
    pub body_displacement_m: [f64; 2],
    pub body_velocity_m_s: [f64; 2],
    pub relative_displacement_m: [f64; 2],
    pub relative_velocity_m_s: [f64; 2],
    /// This midpoint velocity is power-conjugate with the interval force.
    pub electromagnetic_port_velocity_m_s: [f64; 2],
    pub suspension_force_on_tip_n: [f64; 2],
    pub electromagnetic_force_on_tip_n: [f64; 2],
    pub wall_gap_m: [f64; 2],
    pub wall_normal_force_n: [f64; 2],
    pub wall_contact: [bool; 2],
    pub wall_longitudinal_contact: [PickupWallContactTelemetry; 2],
    pub land_gap_m: f64,
    pub land_normal_force_n: f64,
    pub land_contact: bool,
    pub contact_surface: PickupContactSurface,
    pub coulomb_friction_force_n: f64,
    pub modulation_reaction_force_n: f64,
    pub record_reaction_force_tangent_n: f64,
    pub tangential_mode: StylusTangentialMode,
    pub tangential_relative_velocity_m_s: f64,
    /// This value cannot be positive for a valid kinetic Coulomb branch.
    pub tangential_friction_power_w: f64,
    pub groove_radius_m: f64,
    pub skating_force_n: f64,
    pub bearing_friction_force_n: f64,
    pub groove_lateral_force_on_tip_n: f64,
    pub kinetic_energy_j: f64,
    pub suspension_energy_j: f64,
    pub stylus_lowered: bool,
    pub completed_steps: u64,
}

impl PickupMechanicalTelemetry {
    pub fn magnet_velocity_45_45_m_s(self) -> (f64, f64) {
        let lateral = self.relative_velocity_m_s[0];
        let vertical = self.relative_velocity_m_s[1];
        (
            (lateral + vertical) * INVERSE_SQRT_2,
            (lateral - vertical) * INVERSE_SQRT_2,
        )
    }

    pub fn record_reaction_torque_nm(self) -> f64 {
        self.record_reaction_force_tangent_n * self.groove_radius_m
    }

    pub fn longitudinal_record_reaction_force_tangent_n(self) -> f64 {
        self.wall_longitudinal_contact
            .into_iter()
            .flat_map(|wall| wall.record_reaction_force_tangent_n)
            .sum()
    }

    pub fn longitudinal_record_reaction_torque_nm(self) -> f64 {
        self.longitudinal_record_reaction_force_tangent_n() * self.groove_radius_m
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickupMechanicalSnapshot {
    version: u32,
    contact: StylusContactConfig,
    tonearm: TonearmConfig,
    sample_rate_hz: f64,
    tip_displacement_m: [f64; 2],
    tip_velocity_m_s: [f64; 2],
    body_displacement_m: [f64; 2],
    body_velocity_m_s: [f64; 2],
    completed_steps: u64,
    last_telemetry: PickupMechanicalTelemetry,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickupMechanicalState {
    contact: StylusContactConfig,
    tonearm: TonearmConfig,
    sample_rate_hz: f64,
    tip_displacement_m: [f64; 2],
    tip_velocity_m_s: [f64; 2],
    body_displacement_m: [f64; 2],
    body_velocity_m_s: [f64; 2],
    completed_steps: u64,
    last_telemetry: PickupMechanicalTelemetry,
}

impl PickupMechanicalState {
    pub fn new(
        contact: StylusContactConfig,
        tonearm: TonearmConfig,
        sample_rate_hz: f64,
    ) -> Result<Self, PickupMechanicalError> {
        let contact = contact.validate()?;
        let tonearm = tonearm.validate()?;
        validate_sample_rate(sample_rate_hz)?;
        validate_temporal_resolution(contact, tonearm, sample_rate_hz)?;
        Ok(Self {
            contact,
            tonearm,
            sample_rate_hz,
            tip_displacement_m: [0.0; 2],
            tip_velocity_m_s: [0.0; 2],
            body_displacement_m: [0.0; 2],
            body_velocity_m_s: [0.0; 2],
            completed_steps: 0,
            last_telemetry: zero_telemetry(),
        })
    }

    pub fn contact_config(&self) -> StylusContactConfig {
        self.contact
    }

    pub fn tonearm_config(&self) -> TonearmConfig {
        self.tonearm
    }

    pub fn sample_rate_hz(&self) -> f64 {
        self.sample_rate_hz
    }

    pub(crate) fn lateral_tip_kinematics(&self) -> (f64, f64) {
        (self.tip_displacement_m[0], self.tip_velocity_m_s[0])
    }

    pub fn reset(
        &mut self,
        tip_displacement_m: [f64; 2],
        tip_velocity_m_s: [f64; 2],
        body_displacement_m: [f64; 2],
        body_velocity_m_s: [f64; 2],
    ) -> Result<(), PickupMechanicalError> {
        if tip_displacement_m
            .iter()
            .chain(&tip_velocity_m_s)
            .chain(&body_displacement_m)
            .chain(&body_velocity_m_s)
            .any(|value| !value.is_finite())
        {
            return Err(PickupMechanicalError::InvalidInput);
        }
        self.tip_displacement_m = tip_displacement_m;
        self.tip_velocity_m_s = tip_velocity_m_s;
        self.body_displacement_m = body_displacement_m;
        self.body_velocity_m_s = body_velocity_m_s;
        self.completed_steps = 0;
        let relative_displacement_m = subtract(tip_displacement_m, body_displacement_m);
        let relative_velocity_m_s = subtract(tip_velocity_m_s, body_velocity_m_s);
        let axes = [self.tonearm.lateral, self.tonearm.vertical];
        let suspension_force_on_tip_n = [
            -axes[0].stiffness_n_per_m() * relative_displacement_m[0]
                - axes[0].viscous_damping_n_s_per_m() * relative_velocity_m_s[0],
            -axes[1].stiffness_n_per_m() * relative_displacement_m[1]
                - axes[1].viscous_damping_n_s_per_m() * relative_velocity_m_s[1],
        ];
        let mut telemetry = zero_telemetry();
        telemetry.tip_displacement_m = tip_displacement_m;
        telemetry.tip_velocity_m_s = tip_velocity_m_s;
        telemetry.body_displacement_m = body_displacement_m;
        telemetry.body_velocity_m_s = body_velocity_m_s;
        telemetry.relative_displacement_m = relative_displacement_m;
        telemetry.relative_velocity_m_s = relative_velocity_m_s;
        telemetry.electromagnetic_port_velocity_m_s = relative_velocity_m_s;
        telemetry.suspension_force_on_tip_n = suspension_force_on_tip_n;
        telemetry.kinetic_energy_j = 0.5
            * (self.contact.moving_mass_kg * dot(tip_velocity_m_s, tip_velocity_m_s)
                + axes[0].effective_mass_kg * body_velocity_m_s[0] * body_velocity_m_s[0]
                + axes[1].effective_mass_kg * body_velocity_m_s[1] * body_velocity_m_s[1]);
        telemetry.suspension_energy_j = 0.5
            * (axes[0].stiffness_n_per_m()
                * relative_displacement_m[0]
                * relative_displacement_m[0]
                + axes[1].stiffness_n_per_m()
                    * relative_displacement_m[1]
                    * relative_displacement_m[1]);
        self.last_telemetry = telemetry;
        Ok(())
    }

    /// Moves the local lateral origin without moving the physical pickup.
    ///
    /// A positive origin shift subtracts the same distance from the tip and body.
    pub(crate) fn shift_lateral_coordinate_origin(
        &mut self,
        origin_shift_m: f64,
    ) -> Result<(), PickupMechanicalError> {
        if !origin_shift_m.is_finite() {
            return Err(PickupMechanicalError::InvalidInput);
        }
        let tip_displacement_m = self.tip_displacement_m[0] - origin_shift_m;
        let body_displacement_m = self.body_displacement_m[0] - origin_shift_m;
        let telemetry_tip_displacement_m =
            self.last_telemetry.tip_displacement_m[0] - origin_shift_m;
        let telemetry_body_displacement_m =
            self.last_telemetry.body_displacement_m[0] - origin_shift_m;
        if [
            tip_displacement_m,
            body_displacement_m,
            telemetry_tip_displacement_m,
            telemetry_body_displacement_m,
        ]
        .into_iter()
        .any(|value| !value.is_finite())
        {
            return Err(PickupMechanicalError::NumericalFailure);
        }
        self.tip_displacement_m[0] = tip_displacement_m;
        self.body_displacement_m[0] = body_displacement_m;
        self.last_telemetry.tip_displacement_m[0] = telemetry_tip_displacement_m;
        self.last_telemetry.body_displacement_m[0] = telemetry_body_displacement_m;
        Ok(())
    }

    pub fn snapshot(&self) -> PickupMechanicalSnapshot {
        PickupMechanicalSnapshot {
            version: SNAPSHOT_VERSION,
            contact: self.contact,
            tonearm: self.tonearm,
            sample_rate_hz: self.sample_rate_hz,
            tip_displacement_m: self.tip_displacement_m,
            tip_velocity_m_s: self.tip_velocity_m_s,
            body_displacement_m: self.body_displacement_m,
            body_velocity_m_s: self.body_velocity_m_s,
            completed_steps: self.completed_steps,
            last_telemetry: self.last_telemetry,
        }
    }

    pub fn restore(
        &mut self,
        snapshot: PickupMechanicalSnapshot,
    ) -> Result<(), PickupMechanicalError> {
        validate_snapshot(snapshot)?;
        self.contact = snapshot.contact;
        self.tonearm = snapshot.tonearm;
        self.sample_rate_hz = snapshot.sample_rate_hz;
        self.tip_displacement_m = snapshot.tip_displacement_m;
        self.tip_velocity_m_s = snapshot.tip_velocity_m_s;
        self.body_displacement_m = snapshot.body_displacement_m;
        self.body_velocity_m_s = snapshot.body_velocity_m_s;
        self.completed_steps = snapshot.completed_steps;
        self.last_telemetry = snapshot.last_telemetry;
        Ok(())
    }

    /// Advances the coupled moving system by one fixed sample.
    pub fn process(
        &mut self,
        input: PickupMechanicalInput,
    ) -> Result<PickupMechanicalTelemetry, PickupMechanicalError> {
        validate_input(input)?;
        let relation = PickupElectromagneticForceRelation::constant(input.electromagnetic_force_n)?;
        self.process_with_validated_electromagnetic_relation(input, relation)
    }

    /// Advances one sample with an implicit affine electromagnetic force.
    ///
    /// The relation replaces `input.electromagnetic_force_n` for this call.
    pub fn process_with_electromagnetic_relation(
        &mut self,
        input: PickupMechanicalInput,
        relation: PickupElectromagneticForceRelation,
    ) -> Result<PickupMechanicalTelemetry, PickupMechanicalError> {
        validate_input(input)?;
        let relation = PickupElectromagneticForceRelation::new(
            relation.force_bias_n,
            relation.reciprocal_damping_n_s_per_m,
        )?;
        self.process_with_validated_electromagnetic_relation(input, relation)
    }

    fn process_with_validated_electromagnetic_relation(
        &mut self,
        input: PickupMechanicalInput,
        relation: PickupElectromagneticForceRelation,
    ) -> Result<PickupMechanicalTelemetry, PickupMechanicalError> {
        let dt = 1.0 / self.sample_rate_hz;
        let step = solve_pickup_step(
            self.contact,
            self.tonearm,
            self.tip_displacement_m,
            self.tip_velocity_m_s,
            self.body_displacement_m,
            self.body_velocity_m_s,
            [
                self.tonearm.anti_skate_force_n,
                -self.tonearm.vertical_tracking_force_n,
            ],
            input,
            relation,
            dt,
        )?;
        let wall_projected_force_n = step.wall_projected_force_n;
        let zero_friction_wall_contact =
            distribute_wall_contact_forces(input, wall_projected_force_n, 0.0);
        let wall_normal_force_n = zero_friction_wall_contact
            .map(PickupWallContactTelemetry::total_surface_normal_force_n);
        let land_normal_force_n = step.land_normal_force_n;
        let (normal_force_sum, friction_coefficient) = match input.contact_surface {
            PickupContactSurface::GrooveWalls => (
                wall_normal_force_n[0] + wall_normal_force_n[1],
                self.contact.groove_friction_coefficient,
            ),
            PickupContactSurface::RecordLand => (
                land_normal_force_n,
                self.contact.record_surface_friction_coefficient,
            ),
            PickupContactSurface::None => (0.0, 0.0),
        };
        let coulomb_friction_force_n = if input.stylus_lowered {
            -signed_direction(input.groove_tangential_velocity_m_s)
                * friction_coefficient
                * normal_force_sum
        } else {
            0.0
        };
        let wall_contact_telemetry =
            distribute_wall_contact_forces(input, wall_projected_force_n, coulomb_friction_force_n);
        let modulation_reaction_force_n =
            if input.stylus_lowered && input.contact_surface == PickupContactSurface::GrooveWalls {
                sum_wall_contact_field(wall_contact_telemetry, |wall| {
                    wall.modulation_reaction_force_n
                })
            } else {
                0.0
            };
        let record_reaction_force_tangent_n =
            coulomb_friction_force_n + modulation_reaction_force_n;
        let tangential_mode = if !input.stylus_lowered
            || input.contact_surface == PickupContactSurface::None
            || normal_force_sum <= TANGENTIAL_FORCE_TOLERANCE_N
        {
            StylusTangentialMode::Separated
        } else if input.groove_tangential_velocity_m_s > 0.0 {
            StylusTangentialMode::SlidingPositive
        } else if input.groove_tangential_velocity_m_s < 0.0 {
            StylusTangentialMode::SlidingNegative
        } else {
            StylusTangentialMode::Sticking
        };
        self.commit_pickup_step(
            input,
            relation,
            step,
            PickupTangentialForces {
                coulomb_friction_force_n,
                modulation_reaction_force_n,
                record_reaction_force_tangent_n,
                relative_velocity_m_s: input.groove_tangential_velocity_m_s,
                mode: tangential_mode,
            },
            0.0,
            wall_displacement_m(input),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_pickup_step(
        &mut self,
        input: PickupMechanicalInput,
        relation: PickupElectromagneticForceRelation,
        step: PickupStepSolution,
        tangential: PickupTangentialForces,
        lateral_origin_shift_m: f64,
        wall_endpoint_displacement_m: [f64; 2],
    ) -> Result<PickupMechanicalTelemetry, PickupMechanicalError> {
        let dt = 1.0 / self.sample_rate_hz;
        let tip_mass = self.contact.moving_mass_kg;
        let axes = [self.tonearm.lateral, self.tonearm.vertical];
        let previous_relative_velocity = subtract(self.tip_velocity_m_s, self.body_velocity_m_s);
        let tip_velocity = step.tip_velocity_m_s;
        let body_velocity = step.body_velocity_m_s;
        let tip_displacement = [
            self.tip_displacement_m[0] + tip_velocity[0] * dt - lateral_origin_shift_m,
            self.tip_displacement_m[1] + tip_velocity[1] * dt,
        ];
        let body_displacement = [
            self.body_displacement_m[0] + body_velocity[0] * dt - lateral_origin_shift_m,
            self.body_displacement_m[1] + body_velocity[1] * dt,
        ];
        let relative_displacement = subtract(tip_displacement, body_displacement);
        let relative_velocity = subtract(tip_velocity, body_velocity);
        let electromagnetic_port_velocity_m_s = [
            0.5 * (previous_relative_velocity[0] + relative_velocity[0]),
            0.5 * (previous_relative_velocity[1] + relative_velocity[1]),
        ];
        let electromagnetic_force_on_tip_n =
            relation.force_at_relative_velocity(relative_velocity)?;
        let suspension_force_on_tip = [
            -axes[0].stiffness_n_per_m() * relative_displacement[0]
                - axes[0].viscous_damping_n_s_per_m() * relative_velocity[0],
            -axes[1].stiffness_n_per_m() * relative_displacement[1]
                - axes[1].viscous_damping_n_s_per_m() * relative_velocity[1],
        ];
        let wall_projected_force_n = step.wall_projected_force_n;
        let land_normal_force_n = step.land_normal_force_n;
        let wall_longitudinal_contact = distribute_wall_contact_forces(
            input,
            wall_projected_force_n,
            tangential.coulomb_friction_force_n,
        );
        let wall_normal_force_n = wall_longitudinal_contact
            .map(|wall| wall.surface_normal_force_n.into_iter().sum::<f64>());
        let distributed_modulation_reaction_force_n =
            sum_wall_contact_field(wall_longitudinal_contact, |wall| {
                wall.modulation_reaction_force_n
            });
        let distributed_coulomb_friction_force_n =
            sum_wall_contact_field(wall_longitudinal_contact, |wall| {
                wall.coulomb_friction_force_n
            });
        let distributed_record_reaction_force_tangent_n =
            sum_wall_contact_field(wall_longitudinal_contact, |wall| {
                wall.record_reaction_force_tangent_n
            });
        if input.contact_surface == PickupContactSurface::GrooveWalls
            && (!nearly_equal_force(
                distributed_modulation_reaction_force_n,
                tangential.modulation_reaction_force_n,
            ) || !nearly_equal_force(
                distributed_record_reaction_force_tangent_n,
                tangential.record_reaction_force_tangent_n,
            ))
        {
            return Err(PickupMechanicalError::NumericalFailure);
        }
        if input.contact_surface == PickupContactSurface::GrooveWalls
            && !nearly_equal_force(
                distributed_coulomb_friction_force_n,
                tangential.coulomb_friction_force_n,
            )
        {
            return Err(PickupMechanicalError::NumericalFailure);
        }
        let force_on_stylus_tangent_n = -tangential.record_reaction_force_tangent_n;
        let skating_force_n = if input.stylus_lowered {
            self.tonearm
                .geometry
                .equivalent_radial_force_n(input.groove_radius_m, force_on_stylus_tangent_n)?
        } else {
            0.0
        };
        let wall_gap_m =
            if input.stylus_lowered && input.contact_surface == PickupContactSurface::GrooveWalls {
                [
                    dot(WALL_NORMALS[0], tip_displacement) - wall_endpoint_displacement_m[0],
                    dot(WALL_NORMALS[1], tip_displacement) - wall_endpoint_displacement_m[1],
                ]
            } else {
                [0.0; 2]
            };
        let land_gap_m =
            if input.stylus_lowered && input.contact_surface == PickupContactSurface::RecordLand {
                tip_displacement[1] - input.land_displacement_m
            } else {
                0.0
            };
        if wall_gap_m
            .iter()
            .any(|gap| gap.is_finite() && *gap < -CONTACT_TOLERANCE_M)
            || (land_gap_m.is_finite() && land_gap_m < -CONTACT_TOLERANCE_M)
        {
            return Err(PickupMechanicalError::ConstraintFailure);
        }
        let groove_lateral_force_on_tip_n =
            (wall_projected_force_n[0] - wall_projected_force_n[1]) * INVERSE_SQRT_2;
        let kinetic_energy_j = 0.5 * tip_mass * dot(tip_velocity, tip_velocity)
            + 0.5
                * axes
                    .iter()
                    .enumerate()
                    .map(|(axis, config)| {
                        config.effective_mass_kg * body_velocity[axis] * body_velocity[axis]
                    })
                    .sum::<f64>();
        let suspension_energy_j = 0.5
            * axes
                .iter()
                .enumerate()
                .map(|(axis, config)| {
                    config.stiffness_n_per_m()
                        * relative_displacement[axis]
                        * relative_displacement[axis]
                })
                .sum::<f64>();
        let tangential_friction_power_w =
            tangential.coulomb_friction_force_n * tangential.relative_velocity_m_s;
        if tangential_friction_power_w > 1.0e-18
            || tip_displacement
                .iter()
                .chain(&tip_velocity)
                .chain(&body_displacement)
                .chain(&body_velocity)
                .chain(&electromagnetic_force_on_tip_n)
                .chain(&wall_normal_force_n)
                .chain(
                    [
                        land_gap_m,
                        land_normal_force_n,
                        tangential.coulomb_friction_force_n,
                        tangential.modulation_reaction_force_n,
                        tangential.record_reaction_force_tangent_n,
                        tangential.relative_velocity_m_s,
                        tangential_friction_power_w,
                        skating_force_n,
                        step.bearing_friction_force_n,
                        groove_lateral_force_on_tip_n,
                        kinetic_energy_j,
                        suspension_energy_j,
                    ]
                    .iter(),
                )
                .any(|value| !value.is_finite())
        {
            return Err(PickupMechanicalError::NumericalFailure);
        }

        self.tip_displacement_m = tip_displacement;
        self.tip_velocity_m_s = tip_velocity;
        self.body_displacement_m = body_displacement;
        self.body_velocity_m_s = body_velocity;
        self.completed_steps = self.completed_steps.saturating_add(1);
        self.last_telemetry = PickupMechanicalTelemetry {
            tip_displacement_m: tip_displacement,
            tip_velocity_m_s: tip_velocity,
            body_displacement_m: body_displacement,
            body_velocity_m_s: body_velocity,
            relative_displacement_m: relative_displacement,
            relative_velocity_m_s: relative_velocity,
            electromagnetic_port_velocity_m_s,
            suspension_force_on_tip_n: suspension_force_on_tip,
            electromagnetic_force_on_tip_n,
            wall_gap_m,
            wall_normal_force_n,
            wall_contact: step.wall_contact,
            wall_longitudinal_contact,
            land_gap_m,
            land_normal_force_n,
            land_contact: land_normal_force_n > 0.0,
            contact_surface: if input.stylus_lowered {
                input.contact_surface
            } else {
                PickupContactSurface::None
            },
            coulomb_friction_force_n: tangential.coulomb_friction_force_n,
            modulation_reaction_force_n: tangential.modulation_reaction_force_n,
            record_reaction_force_tangent_n: tangential.record_reaction_force_tangent_n,
            tangential_mode: tangential.mode,
            tangential_relative_velocity_m_s: tangential.relative_velocity_m_s,
            tangential_friction_power_w,
            groove_radius_m: input.groove_radius_m,
            skating_force_n,
            bearing_friction_force_n: step.bearing_friction_force_n,
            groove_lateral_force_on_tip_n,
            kinetic_energy_j,
            suspension_energy_j,
            stylus_lowered: input.stylus_lowered,
            completed_steps: self.completed_steps,
        };
        Ok(self.last_telemetry)
    }

    pub fn telemetry(&self) -> PickupMechanicalTelemetry {
        self.last_telemetry
    }
}

/// Defines the frozen midpoint geometry for one joint deck and pickup step.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MidpointPickupGeometry {
    pub(crate) input: PickupMechanicalInput,
    /// This is the torque-independent part of the new spiral-origin shift.
    pub(crate) lateral_origin_shift_bias_m: f64,
    /// Multiplication by the record endpoint velocity gives the remaining shift.
    pub(crate) lateral_origin_shift_per_record_velocity_m_s: f64,
}

/// Contains one atomically prepared deck and pickup result.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CoupledDeckPickupStep {
    pub(crate) deck: DeckMechanicalState,
    pub(crate) pickup: PickupMechanicalState,
    pub(crate) pickup_telemetry: PickupMechanicalTelemetry,
    pub(crate) tangential_mode: StylusTangentialMode,
    pub(crate) evaluated_branches: u32,
    pub(crate) attempted_linear_solves: u32,
}

const JOINT_DYNAMIC_VARIABLES: usize = 6;
const JOINT_MAX_VARIABLES: usize = 13;
const JOINT_RHS_COLUMN: usize = JOINT_MAX_VARIABLES;
// 3 bearing * 3 slipmat * 3 hand * 3 pickup * 4 masks * 4 tangent modes.
pub(crate) const MAX_MIDPOINT_CANDIDATE_BRANCHES: u32 = 1_296;
pub(crate) const MAX_MIDPOINT_LINEAR_SOLVES: u32 = 1_296;

/// Solves the frozen tangent plane, deck, pickup, and cartridge force relation.
///
/// The caller must trace geometry at the explicit angular midpoint based on the
/// previous record velocity. The solver then includes the endpoint correction
/// from the solved record velocity. No state changes if every mode fails.
pub(crate) fn solve_coupled_deck_pickup_midpoint(
    deck: DeckMidpointPreparation,
    pickup: PickupMechanicalState,
    geometry: MidpointPickupGeometry,
    electromagnetic_relation: PickupElectromagneticForceRelation,
    previous_tangential_mode: StylusTangentialMode,
) -> Result<CoupledDeckPickupStep, CoupledDeckPickupError> {
    validate_input(geometry.input)?;
    if !geometry.lateral_origin_shift_bias_m.is_finite()
        || !geometry
            .lateral_origin_shift_per_record_velocity_m_s
            .is_finite()
        || (deck.dt - 1.0 / pickup.sample_rate_hz).abs()
            > 16.0 * f64::EPSILON * deck.dt.max(1.0 / pickup.sample_rate_hz)
    {
        return Err(CoupledDeckPickupError::InvalidMidpointGeometry);
    }
    let relation = PickupElectromagneticForceRelation::new(
        electromagnetic_relation.force_bias_n,
        electromagnetic_relation.reciprocal_damping_n_s_per_m,
    )?;
    let input = geometry.input;
    let constraint_count = active_constraint_count(input);
    let (active_masks, active_mask_count) = match constraint_count {
        0 => ([0b00, 0b00, 0b00, 0b00], 1),
        1 if pickup.last_telemetry.land_contact => ([0b01, 0b00, 0b00, 0b00], 2),
        1 => ([0b00, 0b01, 0b00, 0b00], 2),
        2 => match pickup.last_telemetry.wall_contact {
            [true, true] => ([0b11, 0b01, 0b10, 0b00], 4),
            [true, false] => ([0b01, 0b11, 0b00, 0b10], 4),
            [false, true] => ([0b10, 0b11, 0b00, 0b01], 4),
            [false, false] => ([0b00, 0b11, 0b01, 0b10], 4),
        },
        _ => {
            return Err(CoupledDeckPickupError::Pickup(
                PickupMechanicalError::NumericalFailure,
            ))
        }
    };
    let pickup_bearing_modes = pickup_bearing_mode_order(pickup.body_velocity_m_s[0]);
    let stylus_modes = tangential_mode_order(
        previous_tangential_mode,
        0.5 * input.groove_radius_m
            * (deck.previous_record_velocity_rad_s + deck.predicted_record_velocity_rad_s),
    );
    let skating_factor = if input.stylus_lowered {
        pickup
            .tonearm
            .geometry
            .equivalent_radial_force_n(input.groove_radius_m, 1.0)
            .map_err(PickupMechanicalError::from)?
    } else {
        0.0
    };
    let mut evaluated_branches = 0_u32;
    let mut attempted_linear_solves = 0_u32;

    // This order is part of the deterministic discrete selection law. Predictor
    // and continuation hints change the first candidate only. The fallback
    // still examines every declared active mode without a silent work cap.
    for &deck_bearing_mode in &deck.bearing_modes {
        for &slipmat_mode in &deck.slipmat_modes {
            for &hand_mode in &deck.hand_modes[..deck.hand_mode_count] {
                for pickup_bearing_mode in pickup_bearing_modes {
                    for &active_mask in &active_masks[..active_mask_count] {
                        let mut active_constraints = [0_usize; 2];
                        let mut active_count = 0;
                        for constraint in 0..constraint_count {
                            if active_mask & (1 << constraint) != 0 {
                                active_constraints[active_count] = constraint;
                                active_count += 1;
                            }
                        }
                        let candidate_modes: &[StylusTangentialMode] = if active_mask == 0
                            || !input.stylus_lowered
                            || input.contact_surface == PickupContactSurface::None
                        {
                            &[StylusTangentialMode::Separated]
                        } else {
                            &stylus_modes
                        };
                        for &stylus_mode in candidate_modes {
                            evaluated_branches = evaluated_branches.saturating_add(1);
                            let Some(candidate) = solve_joint_branch(
                                deck,
                                pickup,
                                geometry,
                                relation,
                                deck_bearing_mode,
                                slipmat_mode,
                                hand_mode,
                                pickup_bearing_mode,
                                active_mask,
                                active_constraints,
                                active_count,
                                stylus_mode,
                                skating_factor,
                                &mut attempted_linear_solves,
                            ) else {
                                continue;
                            };
                            let next_deck = deck.commit(candidate.deck_solution)?;
                            let mut next_pickup = pickup;
                            let pickup_telemetry = next_pickup.commit_pickup_step(
                                input,
                                relation,
                                candidate.pickup_solution,
                                candidate.tangential,
                                candidate.lateral_origin_shift_m,
                                candidate.wall_endpoint_displacement_m,
                            )?;
                            if pickup_telemetry.record_reaction_torque_nm()
                                != next_deck.telemetry().stylus_torque_nm
                            {
                                return Err(CoupledDeckPickupError::ReciprocityMismatch);
                            }
                            return Ok(CoupledDeckPickupStep {
                                deck: next_deck,
                                pickup: next_pickup,
                                pickup_telemetry,
                                tangential_mode: stylus_mode,
                                evaluated_branches,
                                attempted_linear_solves,
                            });
                        }
                    }
                }
            }
        }
    }
    Err(CoupledDeckPickupError::NoConsistentMode {
        evaluated_branches,
        attempted_linear_solves,
    })
}

#[derive(Debug, Clone, Copy)]
struct JointCandidate {
    deck_solution: DeckMidpointSolution,
    pickup_solution: PickupStepSolution,
    tangential: PickupTangentialForces,
    lateral_origin_shift_m: f64,
    wall_endpoint_displacement_m: [f64; 2],
}

#[allow(clippy::too_many_arguments)]
fn solve_joint_branch(
    deck: DeckMidpointPreparation,
    pickup: PickupMechanicalState,
    geometry: MidpointPickupGeometry,
    relation: PickupElectromagneticForceRelation,
    deck_bearing_mode: CoupledDeckFrictionMode,
    slipmat_mode: CoupledDeckFrictionMode,
    hand_mode: CoupledDeckFrictionMode,
    pickup_bearing_mode: BearingMode,
    active_mask: u8,
    active_constraints: [usize; 2],
    active_count: usize,
    stylus_mode: StylusTangentialMode,
    skating_factor: f64,
    attempted_linear_solves: &mut u32,
) -> Option<JointCandidate> {
    let input = geometry.input;
    let static_constraints = select_independent_deck_constraints(
        deck_bearing_mode,
        slipmat_mode,
        hand_mode,
        stylus_mode,
        deck.hand_velocity_rad_s,
        -deck.previous_record_velocity_rad_s,
    )?;
    let mut augmented = [[0.0_f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES];
    let mut next_column = JOINT_DYNAMIC_VARIABLES;
    let mut lambda_columns = [None; 2];
    for &constraint in &active_constraints[..active_count] {
        lambda_columns[constraint] = Some(next_column);
        next_column += 1;
    }
    let pickup_bearing_column = (pickup_bearing_mode == BearingMode::Stick).then(|| {
        let column = next_column;
        next_column += 1;
        column
    });
    let deck_bearing_column = static_constraints[0].then(|| {
        let column = next_column;
        next_column += 1;
        column
    });
    let slipmat_column = static_constraints[1].then(|| {
        let column = next_column;
        next_column += 1;
        column
    });
    let hand_column = static_constraints[2].then(|| {
        let column = next_column;
        next_column += 1;
        column
    });
    let stylus_force_column = static_constraints[3].then(|| {
        let column = next_column;
        next_column += 1;
        column
    });
    if next_column > JOINT_MAX_VARIABLES {
        return None;
    }

    assemble_deck_dynamics(
        &mut augmented,
        deck,
        deck_bearing_mode,
        slipmat_mode,
        hand_mode,
        deck_bearing_column,
        slipmat_column,
        hand_column,
    )?;
    assemble_pickup_dynamics(
        &mut augmented,
        pickup,
        input,
        relation,
        pickup_bearing_mode,
        pickup_bearing_column,
    );

    let surface_normal_scale = wall_effective_surface_normal_scale(input);
    let effective_slope = wall_effective_slope(input);
    let friction_coefficient = match input.contact_surface {
        PickupContactSurface::GrooveWalls => pickup.contact.groove_friction_coefficient,
        PickupContactSurface::RecordLand => pickup.contact.record_surface_friction_coefficient,
        PickupContactSurface::None => 0.0,
    };
    let sliding_direction = match stylus_mode {
        StylusTangentialMode::SlidingPositive => 1.0,
        StylusTangentialMode::SlidingNegative => -1.0,
        StylusTangentialMode::Separated | StylusTangentialMode::Sticking => 0.0,
    };
    if stylus_mode == StylusTangentialMode::Sticking {
        if let Some(column) = stylus_force_column {
            augmented[1][column] = -input.groove_radius_m;
            augmented[3][column] = skating_factor;
        }
    } else {
        for constraint in 0..active_constraint_count(input) {
            let Some(column) = lambda_columns[constraint] else {
                continue;
            };
            let tangential_force_per_projected_normal = match input.contact_surface {
                PickupContactSurface::GrooveWalls => {
                    effective_slope[constraint]
                        + sliding_direction
                            * friction_coefficient
                            * surface_normal_scale[constraint]
                }
                PickupContactSurface::RecordLand => sliding_direction * friction_coefficient,
                PickupContactSurface::None => 0.0,
            };
            augmented[1][column] += input.groove_radius_m * tangential_force_per_projected_normal;
            augmented[3][column] -= skating_factor * tangential_force_per_projected_normal;
        }
    }
    for constraint in 0..active_constraint_count(input) {
        if let Some(column) = lambda_columns[constraint] {
            let normal = constraint_normal(input.contact_surface, constraint);
            augmented[2][column] = -normal[0];
            augmented[4][column] = -normal[1];
        }
    }

    let mut next_row = JOINT_DYNAMIC_VARIABLES;
    for &constraint in &active_constraints[..active_count] {
        let normal = constraint_normal(input.contact_surface, constraint);
        let displacement = constraint_midpoint_displacement(input, constraint);
        augmented[next_row][1] =
            -normal[0] * geometry.lateral_origin_shift_per_record_velocity_m_s / deck.dt;
        if input.contact_surface == PickupContactSurface::GrooveWalls {
            augmented[next_row][1] -= 0.5 * effective_slope[constraint] * input.groove_radius_m;
        }
        augmented[next_row][2] = normal[0];
        augmented[next_row][3] = normal[1];
        augmented[next_row][JOINT_RHS_COLUMN] = (displacement
            - dot(normal, pickup.tip_displacement_m)
            + normal[0] * geometry.lateral_origin_shift_bias_m)
            / deck.dt;
        next_row += 1;
    }
    if pickup_bearing_column.is_some() {
        augmented[next_row][4] = 1.0;
        next_row += 1;
    }
    if deck_bearing_column.is_some() {
        augmented[next_row][0] = 1.0;
        next_row += 1;
    }
    if slipmat_column.is_some() {
        augmented[next_row][0] = 1.0;
        augmented[next_row][1] = -1.0;
        next_row += 1;
    }
    if hand_column.is_some() {
        augmented[next_row][1] = 1.0;
        augmented[next_row][JOINT_RHS_COLUMN] = deck.hand_velocity_rad_s;
        next_row += 1;
    }
    if stylus_force_column.is_some() {
        augmented[next_row][1] = 1.0;
        augmented[next_row][JOINT_RHS_COLUMN] = -deck.previous_record_velocity_rad_s;
        next_row += 1;
    }
    if next_row != next_column {
        return None;
    }
    *attempted_linear_solves = attempted_linear_solves.saturating_add(1);
    let solution = solve_joint_linear_system(&mut augmented, next_column)?;
    validate_joint_candidate(
        deck,
        pickup,
        geometry,
        solution,
        deck_bearing_mode,
        slipmat_mode,
        hand_mode,
        pickup_bearing_mode,
        active_mask,
        active_constraints,
        active_count,
        lambda_columns,
        pickup_bearing_column,
        deck_bearing_column,
        slipmat_column,
        hand_column,
        stylus_force_column,
        stylus_mode,
    )
}

fn active_constraint_count(input: PickupMechanicalInput) -> usize {
    if !input.stylus_lowered {
        0
    } else {
        match input.contact_surface {
            PickupContactSurface::None => 0,
            PickupContactSurface::GrooveWalls => 2,
            PickupContactSurface::RecordLand => 1,
        }
    }
}

fn constraint_normal(surface: PickupContactSurface, constraint: usize) -> [f64; 2] {
    match surface {
        PickupContactSurface::GrooveWalls => WALL_NORMALS[constraint],
        PickupContactSurface::RecordLand => [0.0, 1.0],
        PickupContactSurface::None => [0.0; 2],
    }
}

fn constraint_midpoint_displacement(input: PickupMechanicalInput, constraint: usize) -> f64 {
    match input.contact_surface {
        PickupContactSurface::GrooveWalls => input.wall_contacts[constraint].center_displacement_m,
        PickupContactSurface::RecordLand => input.land_displacement_m,
        PickupContactSurface::None => 0.0,
    }
}

fn pickup_bearing_mode_order(previous_velocity_m_s: f64) -> [BearingMode; 3] {
    if previous_velocity_m_s > BEARING_VELOCITY_TOLERANCE_M_S {
        [
            BearingMode::Positive,
            BearingMode::Stick,
            BearingMode::Negative,
        ]
    } else if previous_velocity_m_s < -BEARING_VELOCITY_TOLERANCE_M_S {
        [
            BearingMode::Negative,
            BearingMode::Stick,
            BearingMode::Positive,
        ]
    } else {
        [
            BearingMode::Stick,
            BearingMode::Positive,
            BearingMode::Negative,
        ]
    }
}

fn tangential_mode_order(
    previous: StylusTangentialMode,
    previous_relative_velocity_m_s: f64,
) -> [StylusTangentialMode; 4] {
    if previous_relative_velocity_m_s > 1.0e-12 {
        [
            StylusTangentialMode::SlidingPositive,
            StylusTangentialMode::Sticking,
            StylusTangentialMode::SlidingNegative,
            StylusTangentialMode::Separated,
        ]
    } else if previous_relative_velocity_m_s < -1.0e-12 {
        [
            StylusTangentialMode::SlidingNegative,
            StylusTangentialMode::Sticking,
            StylusTangentialMode::SlidingPositive,
            StylusTangentialMode::Separated,
        ]
    } else {
        let second = match previous {
            StylusTangentialMode::SlidingNegative => StylusTangentialMode::SlidingNegative,
            _ => StylusTangentialMode::SlidingPositive,
        };
        let third = match second {
            StylusTangentialMode::SlidingPositive => StylusTangentialMode::SlidingNegative,
            _ => StylusTangentialMode::SlidingPositive,
        };
        [
            StylusTangentialMode::Sticking,
            second,
            third,
            StylusTangentialMode::Separated,
        ]
    }
}

fn select_independent_deck_constraints(
    bearing_mode: CoupledDeckFrictionMode,
    slipmat_mode: CoupledDeckFrictionMode,
    hand_mode: CoupledDeckFrictionMode,
    stylus_mode: StylusTangentialMode,
    hand_velocity_rad_s: f64,
    stylus_velocity_rad_s: f64,
) -> Option<[bool; 4]> {
    let requested = [
        bearing_mode.is_sticking().then_some(([1.0, 0.0], 0.0)),
        slipmat_mode.is_sticking().then_some(([1.0, -1.0], 0.0)),
        hand_mode
            .is_sticking()
            .then_some(([0.0, 1.0], hand_velocity_rad_s)),
        (stylus_mode == StylusTangentialMode::Sticking)
            .then_some(([0.0, 1.0], stylus_velocity_rad_s)),
    ];
    let mut basis_vectors = [[0.0_f64; 2]; 2];
    let mut basis_rhs = [0.0_f64; 2];
    let mut rank = 0;
    let mut selected = [false; 4];
    for (index, constraint) in requested.into_iter().enumerate() {
        let Some((vector, rhs)) = constraint else {
            continue;
        };
        if rank == 0 {
            basis_vectors[0] = vector;
            basis_rhs[0] = rhs;
            rank = 1;
            selected[index] = true;
            continue;
        }
        if rank == 1 {
            let determinant = basis_vectors[0][0] * vector[1] - basis_vectors[0][1] * vector[0];
            if determinant.abs() > 1.0e-14 {
                basis_vectors[1] = vector;
                basis_rhs[1] = rhs;
                rank = 2;
                selected[index] = true;
                continue;
            }
            let scale = if basis_vectors[0][0].abs() >= basis_vectors[0][1].abs() {
                vector[0] / basis_vectors[0][0]
            } else {
                vector[1] / basis_vectors[0][1]
            };
            if (rhs - scale * basis_rhs[0]).abs()
                > 1.0e-12 * rhs.abs().max((scale * basis_rhs[0]).abs()).max(1.0)
            {
                return None;
            }
            continue;
        }
        let determinant =
            basis_vectors[0][0] * basis_vectors[1][1] - basis_vectors[0][1] * basis_vectors[1][0];
        let left_weight =
            (vector[0] * basis_vectors[1][1] - vector[1] * basis_vectors[1][0]) / determinant;
        let right_weight =
            (basis_vectors[0][0] * vector[1] - basis_vectors[0][1] * vector[0]) / determinant;
        let implied_rhs = left_weight * basis_rhs[0] + right_weight * basis_rhs[1];
        if (rhs - implied_rhs).abs() > 1.0e-12 * rhs.abs().max(implied_rhs.abs()).max(1.0) {
            return None;
        }
    }
    Some(selected)
}

#[allow(clippy::too_many_arguments)]
fn assemble_deck_dynamics(
    augmented: &mut [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    deck: DeckMidpointPreparation,
    bearing_mode: CoupledDeckFrictionMode,
    slipmat_mode: CoupledDeckFrictionMode,
    hand_mode: CoupledDeckFrictionMode,
    bearing_column: Option<usize>,
    slipmat_column: Option<usize>,
    hand_column: Option<usize>,
) -> Option<()> {
    let platter_mass = deck.config.platter_inertia_kg_m2 / deck.dt;
    let record_mass = deck.config.record_inertia_kg_m2 / deck.dt;
    augmented[0][0] = platter_mass;
    augmented[0][JOINT_RHS_COLUMN] =
        platter_mass * deck.previous_platter_velocity_rad_s + deck.motor_torque_nm;
    augmented[1][1] = record_mass;
    augmented[1][JOINT_RHS_COLUMN] = record_mass * deck.previous_record_velocity_rad_s;

    match bearing_mode {
        CoupledDeckFrictionMode::Sticking => {
            if let Some(column) = bearing_column {
                augmented[0][column] = -1.0;
            }
        }
        CoupledDeckFrictionMode::SlidingPositive => {
            augmented[0][0] += deck.config.bearing_viscous_torque_nm_per_rad_s;
            augmented[0][JOINT_RHS_COLUMN] -= deck.config.bearing_kinetic_torque_nm;
        }
        CoupledDeckFrictionMode::SlidingNegative => {
            augmented[0][0] += deck.config.bearing_viscous_torque_nm_per_rad_s;
            augmented[0][JOINT_RHS_COLUMN] += deck.config.bearing_kinetic_torque_nm;
        }
        CoupledDeckFrictionMode::Separated => return None,
    }
    match slipmat_mode {
        CoupledDeckFrictionMode::Sticking => {
            if let Some(column) = slipmat_column {
                augmented[0][column] = 1.0;
                augmented[1][column] = -1.0;
            }
        }
        CoupledDeckFrictionMode::SlidingPositive | CoupledDeckFrictionMode::SlidingNegative => {
            let bias = if slipmat_mode == CoupledDeckFrictionMode::SlidingPositive {
                deck.config.slipmat_kinetic_torque_nm
            } else {
                -deck.config.slipmat_kinetic_torque_nm
            };
            let damping = deck.config.slipmat_viscous_torque_nm_per_rad_s;
            augmented[0][0] += damping;
            augmented[0][1] -= damping;
            augmented[0][JOINT_RHS_COLUMN] -= bias;
            augmented[1][0] -= damping;
            augmented[1][1] += damping;
            augmented[1][JOINT_RHS_COLUMN] += bias;
        }
        CoupledDeckFrictionMode::Separated => return None,
    }
    match hand_mode {
        CoupledDeckFrictionMode::Sticking => {
            if let Some(column) = hand_column {
                augmented[1][column] = -1.0;
            }
        }
        CoupledDeckFrictionMode::SlidingPositive | CoupledDeckFrictionMode::SlidingNegative => {
            let bias = if hand_mode == CoupledDeckFrictionMode::SlidingPositive {
                deck.hand_kinetic_limit_nm
            } else {
                -deck.hand_kinetic_limit_nm
            };
            let damping = deck.config.hand_viscous_torque_nm_per_rad_s;
            augmented[1][1] += damping;
            augmented[1][JOINT_RHS_COLUMN] += bias + damping * deck.hand_velocity_rad_s;
        }
        CoupledDeckFrictionMode::Separated => {}
    }
    Some(())
}

fn assemble_pickup_dynamics(
    augmented: &mut [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    pickup: PickupMechanicalState,
    input: PickupMechanicalInput,
    relation: PickupElectromagneticForceRelation,
    bearing_mode: BearingMode,
    bearing_column: Option<usize>,
) {
    let dt = 1.0 / pickup.sample_rate_hz;
    let axes = [pickup.tonearm.lateral, pickup.tonearm.vertical];
    let body_external_force_n = [
        pickup.tonearm.anti_skate_force_n,
        -pickup.tonearm.vertical_tracking_force_n,
    ];
    for axis in 0..2 {
        let tip_row = 2 + axis * 2;
        let body_row = tip_row + 1;
        let tip_column = 2 + axis;
        let body_column = 4 + axis;
        let stiffness = axes[axis].stiffness_n_per_m();
        let damping = axes[axis].viscous_damping_n_s_per_m();
        let coupling = stiffness * dt + damping;
        let relative_displacement =
            pickup.tip_displacement_m[axis] - pickup.body_displacement_m[axis];
        augmented[tip_row][tip_column] = pickup.contact.moving_mass_kg / dt + coupling;
        augmented[tip_row][body_column] = -coupling;
        augmented[tip_row][JOINT_RHS_COLUMN] = pickup.contact.moving_mass_kg / dt
            * pickup.tip_velocity_m_s[axis]
            - stiffness * relative_displacement
            + relation.force_bias_n[axis];
        augmented[body_row][tip_column] = -coupling;
        augmented[body_row][body_column] = axes[axis].effective_mass_kg / dt + coupling;
        augmented[body_row][JOINT_RHS_COLUMN] = axes[axis].effective_mass_kg / dt
            * pickup.body_velocity_m_s[axis]
            + stiffness * relative_displacement
            + body_external_force_n[axis]
            - relation.force_bias_n[axis];
        for velocity_axis in 0..2 {
            let reciprocal_damping = relation.reciprocal_damping_n_s_per_m[axis][velocity_axis];
            augmented[tip_row][2 + velocity_axis] += reciprocal_damping;
            augmented[tip_row][4 + velocity_axis] -= reciprocal_damping;
            augmented[body_row][2 + velocity_axis] -= reciprocal_damping;
            augmented[body_row][4 + velocity_axis] += reciprocal_damping;
        }
        if axis == 0 {
            augmented[body_row][body_column] +=
                pickup.tonearm.lateral_bearing_viscous_damping_n_s_per_m;
            augmented[body_row][JOINT_RHS_COLUMN] += match bearing_mode {
                BearingMode::Stick => 0.0,
                BearingMode::Positive => -pickup.tonearm.lateral_bearing_kinetic_friction_n,
                BearingMode::Negative => pickup.tonearm.lateral_bearing_kinetic_friction_n,
            };
            if let Some(column) = bearing_column {
                augmented[body_row][column] = -1.0;
            }
        }
        if axis == 1 && !input.stylus_lowered {
            let cue_coupling = pickup.tonearm.cue_support_stiffness_n_per_m * dt
                + pickup.tonearm.cue_support_damping_n_s_per_m;
            augmented[body_row][body_column] += cue_coupling;
            augmented[body_row][JOINT_RHS_COLUMN] += pickup.tonearm.cue_support_stiffness_n_per_m
                * (pickup.tonearm.cue_lift_height_m - pickup.body_displacement_m[1]);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_joint_candidate(
    deck: DeckMidpointPreparation,
    pickup: PickupMechanicalState,
    geometry: MidpointPickupGeometry,
    solution: [f64; JOINT_MAX_VARIABLES],
    deck_bearing_mode: CoupledDeckFrictionMode,
    slipmat_mode: CoupledDeckFrictionMode,
    hand_mode: CoupledDeckFrictionMode,
    pickup_bearing_mode: BearingMode,
    active_mask: u8,
    active_constraints: [usize; 2],
    active_count: usize,
    lambda_columns: [Option<usize>; 2],
    pickup_bearing_column: Option<usize>,
    deck_bearing_column: Option<usize>,
    slipmat_column: Option<usize>,
    hand_column: Option<usize>,
    stylus_force_column: Option<usize>,
    stylus_mode: StylusTangentialMode,
) -> Option<JointCandidate> {
    let input = geometry.input;
    let platter_velocity_rad_s = solution[0];
    let record_velocity_rad_s = solution[1];
    let tip_velocity_m_s = [solution[2], solution[3]];
    let body_velocity_m_s = [solution[4], solution[5]];
    let lateral_origin_shift_m = geometry.lateral_origin_shift_bias_m
        + geometry.lateral_origin_shift_per_record_velocity_m_s * record_velocity_rad_s;
    let effective_slope = wall_effective_slope(input);
    let wall_endpoint_displacement_m = [0, 1].map(|wall| {
        input.wall_contacts[wall].center_displacement_m
            + 0.5 * effective_slope[wall] * input.groove_radius_m * deck.dt * record_velocity_rad_s
    });
    let tip_displacement_m = [
        pickup.tip_displacement_m[0] + tip_velocity_m_s[0] * deck.dt - lateral_origin_shift_m,
        pickup.tip_displacement_m[1] + tip_velocity_m_s[1] * deck.dt,
    ];
    let mut constraint_force_n = [0.0; 2];
    let constraint_count = active_constraint_count(input);
    for constraint in 0..constraint_count {
        let normal = constraint_normal(input.contact_surface, constraint);
        let endpoint_displacement = match input.contact_surface {
            PickupContactSurface::GrooveWalls => wall_endpoint_displacement_m[constraint],
            PickupContactSurface::RecordLand => input.land_displacement_m,
            PickupContactSurface::None => 0.0,
        };
        let gap = dot(normal, tip_displacement_m) - endpoint_displacement;
        if active_mask & (1 << constraint) == 0 && gap < -CONTACT_TOLERANCE_M {
            return None;
        }
        if let Some(column) = lambda_columns[constraint] {
            let force = solution[column];
            if force < -TANGENTIAL_FORCE_TOLERANCE_N {
                return None;
            }
            constraint_force_n[constraint] = force.max(0.0);
        }
    }
    debug_assert_eq!(
        active_constraints[..active_count]
            .iter()
            .filter(|constraint| lambda_columns[**constraint].is_some())
            .count(),
        active_count
    );
    let (wall_projected_force_n, land_normal_force_n) = match input.contact_surface {
        PickupContactSurface::GrooveWalls if input.stylus_lowered => (constraint_force_n, 0.0),
        PickupContactSurface::RecordLand if input.stylus_lowered => {
            ([0.0; 2], constraint_force_n[0])
        }
        _ => ([0.0; 2], 0.0),
    };
    let zero_friction_wall_contact =
        distribute_wall_contact_forces(input, wall_projected_force_n, 0.0);
    let wall_normal_force_n =
        zero_friction_wall_contact.map(PickupWallContactTelemetry::total_surface_normal_force_n);
    let (normal_force_sum, friction_coefficient) = match input.contact_surface {
        PickupContactSurface::GrooveWalls => (
            wall_normal_force_n[0] + wall_normal_force_n[1],
            pickup.contact.groove_friction_coefficient,
        ),
        PickupContactSurface::RecordLand => (
            land_normal_force_n,
            pickup.contact.record_surface_friction_coefficient,
        ),
        PickupContactSurface::None => (0.0, 0.0),
    };
    let modulation_reaction_force_n =
        if input.stylus_lowered && input.contact_surface == PickupContactSurface::GrooveWalls {
            sum_wall_contact_field(zero_friction_wall_contact, |wall| {
                wall.modulation_reaction_force_n
            })
        } else {
            0.0
        };
    let tangential_relative_velocity_m_s =
        0.5 * input.groove_radius_m * (deck.previous_record_velocity_rad_s + record_velocity_rad_s);
    let (record_reaction_force_tangent_n, coulomb_friction_force_n) = match stylus_mode {
        StylusTangentialMode::Sticking => {
            if normal_force_sum <= TANGENTIAL_FORCE_TOLERANCE_N
                || tangential_relative_velocity_m_s.abs() > 1.0e-12
            {
                return None;
            }
            let total = stylus_force_column.map_or(0.0, |column| solution[column]);
            let friction = total - modulation_reaction_force_n;
            if friction.abs()
                > friction_coefficient * normal_force_sum + TANGENTIAL_FORCE_TOLERANCE_N
            {
                return None;
            }
            (total, friction)
        }
        StylusTangentialMode::SlidingPositive => {
            if normal_force_sum <= TANGENTIAL_FORCE_TOLERANCE_N
                || tangential_relative_velocity_m_s <= 0.0
            {
                return None;
            }
            let friction = -friction_coefficient * normal_force_sum;
            (modulation_reaction_force_n + friction, friction)
        }
        StylusTangentialMode::SlidingNegative => {
            if normal_force_sum <= TANGENTIAL_FORCE_TOLERANCE_N
                || tangential_relative_velocity_m_s >= 0.0
            {
                return None;
            }
            let friction = friction_coefficient * normal_force_sum;
            (modulation_reaction_force_n + friction, friction)
        }
        StylusTangentialMode::Separated => {
            if input.stylus_lowered
                && input.contact_surface != PickupContactSurface::None
                && normal_force_sum > TANGENTIAL_FORCE_TOLERANCE_N
                && friction_coefficient > 0.0
            {
                return None;
            }
            (modulation_reaction_force_n, 0.0)
        }
    };
    if coulomb_friction_force_n * tangential_relative_velocity_m_s > 1.0e-18 {
        return None;
    }

    let pickup_bearing_friction_force_n = match pickup_bearing_mode {
        BearingMode::Stick => {
            let force = solution[pickup_bearing_column?];
            if force.abs() > pickup.tonearm.lateral_bearing_static_friction_n + 1.0e-10 {
                return None;
            }
            force
        }
        BearingMode::Positive => {
            if body_velocity_m_s[0] <= 0.0 {
                return None;
            }
            -pickup.tonearm.lateral_bearing_kinetic_friction_n
                - pickup.tonearm.lateral_bearing_viscous_damping_n_s_per_m * body_velocity_m_s[0]
        }
        BearingMode::Negative => {
            if body_velocity_m_s[0] >= 0.0 {
                return None;
            }
            pickup.tonearm.lateral_bearing_kinetic_friction_n
                - pickup.tonearm.lateral_bearing_viscous_damping_n_s_per_m * body_velocity_m_s[0]
        }
    };
    let bearing_torque_nm = deck_bearing_column.map_or_else(
        || match deck_bearing_mode {
            CoupledDeckFrictionMode::SlidingPositive => {
                -deck.config.bearing_kinetic_torque_nm
                    - deck.config.bearing_viscous_torque_nm_per_rad_s * platter_velocity_rad_s
            }
            CoupledDeckFrictionMode::SlidingNegative => {
                deck.config.bearing_kinetic_torque_nm
                    - deck.config.bearing_viscous_torque_nm_per_rad_s * platter_velocity_rad_s
            }
            CoupledDeckFrictionMode::Sticking | CoupledDeckFrictionMode::Separated => 0.0,
        },
        |column| solution[column],
    );
    let slipmat_torque_nm = slipmat_column.map_or_else(
        || match slipmat_mode {
            CoupledDeckFrictionMode::Sticking => 0.0,
            CoupledDeckFrictionMode::SlidingPositive | CoupledDeckFrictionMode::SlidingNegative => {
                let sign = if slipmat_mode == CoupledDeckFrictionMode::SlidingPositive {
                    1.0
                } else {
                    -1.0
                };
                sign * deck.config.slipmat_kinetic_torque_nm
                    + deck.config.slipmat_viscous_torque_nm_per_rad_s
                        * (platter_velocity_rad_s - record_velocity_rad_s)
            }
            CoupledDeckFrictionMode::Separated => 0.0,
        },
        |column| solution[column],
    );
    let hand_torque_nm = match hand_mode {
        CoupledDeckFrictionMode::Separated => 0.0,
        CoupledDeckFrictionMode::Sticking => hand_column.map_or(0.0, |column| solution[column]),
        CoupledDeckFrictionMode::SlidingPositive | CoupledDeckFrictionMode::SlidingNegative => {
            let sign = if hand_mode == CoupledDeckFrictionMode::SlidingPositive {
                1.0
            } else {
                -1.0
            };
            sign * deck.hand_kinetic_limit_nm
                + deck.config.hand_viscous_torque_nm_per_rad_s
                    * (deck.hand_velocity_rad_s - record_velocity_rad_s)
        }
    };
    if !coupled_deck_mode_is_valid(
        deck_bearing_mode,
        platter_velocity_rad_s,
        bearing_torque_nm,
        deck.bearing_static_limit_nm,
    ) || !coupled_deck_mode_is_valid(
        slipmat_mode,
        platter_velocity_rad_s - record_velocity_rad_s,
        slipmat_torque_nm,
        deck.slipmat_static_limit_nm,
    ) || !coupled_deck_mode_is_valid(
        hand_mode,
        deck.hand_velocity_rad_s - record_velocity_rad_s,
        hand_torque_nm,
        deck.hand_static_limit_nm,
    ) {
        return None;
    }
    let stylus_torque_nm = input.groove_radius_m * record_reaction_force_tangent_n;
    if [
        platter_velocity_rad_s,
        record_velocity_rad_s,
        tip_velocity_m_s[0],
        tip_velocity_m_s[1],
        body_velocity_m_s[0],
        body_velocity_m_s[1],
        lateral_origin_shift_m,
        pickup_bearing_friction_force_n,
        bearing_torque_nm,
        slipmat_torque_nm,
        hand_torque_nm,
        stylus_torque_nm,
        record_reaction_force_tangent_n,
        coulomb_friction_force_n,
        modulation_reaction_force_n,
    ]
    .into_iter()
    .chain(wall_endpoint_displacement_m)
    .any(|value| !value.is_finite())
    {
        return None;
    }
    Some(JointCandidate {
        deck_solution: DeckMidpointSolution {
            platter_velocity_rad_s,
            record_velocity_rad_s,
            bearing_torque_nm,
            slipmat_torque_nm,
            hand_torque_nm,
            stylus_torque_nm,
            bearing_mode: deck_bearing_mode,
            slipmat_mode,
            hand_mode,
        },
        pickup_solution: PickupStepSolution {
            tip_velocity_m_s,
            body_velocity_m_s,
            wall_projected_force_n,
            wall_contact: wall_projected_force_n.map(|force| force > 0.0),
            land_normal_force_n,
            bearing_friction_force_n: pickup_bearing_friction_force_n,
        },
        tangential: PickupTangentialForces {
            coulomb_friction_force_n,
            modulation_reaction_force_n,
            record_reaction_force_tangent_n,
            relative_velocity_m_s: tangential_relative_velocity_m_s,
            mode: stylus_mode,
        },
        lateral_origin_shift_m,
        wall_endpoint_displacement_m,
    })
}

fn solve_joint_linear_system(
    augmented: &mut [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    size: usize,
) -> Option<[f64; JOINT_MAX_VARIABLES]> {
    if size == 0 || size > JOINT_MAX_VARIABLES {
        return None;
    }
    let original = *augmented;
    for row in augmented.iter_mut().take(size) {
        let row_scale = row[..size]
            .iter()
            .map(|value| value.abs())
            .fold(0.0_f64, f64::max);
        if !row_scale.is_finite() || row_scale == 0.0 {
            return None;
        }
        for value in &mut row[..size] {
            *value /= row_scale;
        }
        row[JOINT_RHS_COLUMN] /= row_scale;
    }
    let mut column_scales = [1.0_f64; JOINT_MAX_VARIABLES];
    for column in 0..size {
        let column_scale = augmented[..size]
            .iter()
            .map(|row| row[column].abs())
            .fold(0.0_f64, f64::max);
        if !column_scale.is_finite() || column_scale == 0.0 {
            return None;
        }
        column_scales[column] = column_scale;
        for row in augmented.iter_mut().take(size) {
            row[column] /= column_scale;
        }
    }
    let relative_pivot_tolerance = 128.0 * f64::EPSILON * size as f64;
    for pivot_column in 0..size {
        let pivot_row = (pivot_column..size).max_by(|left, right| {
            augmented[*left][pivot_column]
                .abs()
                .total_cmp(&augmented[*right][pivot_column].abs())
        })?;
        let pivot = augmented[pivot_row][pivot_column];
        if !pivot.is_finite() || pivot.abs() <= relative_pivot_tolerance {
            return None;
        }
        augmented.swap(pivot_column, pivot_row);
        for column in pivot_column..size {
            augmented[pivot_column][column] /= pivot;
        }
        augmented[pivot_column][JOINT_RHS_COLUMN] /= pivot;
        let pivot_values = augmented[pivot_column];
        for (row, augmented_row) in augmented.iter_mut().enumerate().take(size) {
            if row == pivot_column {
                continue;
            }
            let scale = augmented_row[pivot_column];
            for column in pivot_column..size {
                augmented_row[column] -= scale * pivot_values[column];
            }
            augmented_row[JOINT_RHS_COLUMN] -= scale * pivot_values[JOINT_RHS_COLUMN];
        }
    }
    let mut solution = [0.0; JOINT_MAX_VARIABLES];
    for row in 0..size {
        solution[row] = augmented[row][JOINT_RHS_COLUMN] / column_scales[row];
    }
    if !solution.iter().take(size).all(|value| value.is_finite()) {
        return None;
    }
    let mut residual_norm = 0.0_f64;
    let mut matrix_norm = 0.0_f64;
    let solution_norm = solution[..size]
        .iter()
        .map(|value| value.abs())
        .fold(0.0_f64, f64::max);
    let mut right_hand_side_norm = 0.0_f64;
    for row in original.iter().take(size) {
        let computed = row[..size]
            .iter()
            .zip(&solution[..size])
            .fold(0.0, |sum, (coefficient, value)| {
                coefficient.mul_add(*value, sum)
            });
        residual_norm = residual_norm.max((computed - row[JOINT_RHS_COLUMN]).abs());
        matrix_norm = matrix_norm.max(row[..size].iter().map(|value| value.abs()).sum());
        right_hand_side_norm = right_hand_side_norm.max(row[JOINT_RHS_COLUMN].abs());
    }
    let normalization = matrix_norm
        .mul_add(solution_norm, right_hand_side_norm)
        .max(f64::MIN_POSITIVE);
    let backward_error = residual_norm / normalization;
    (backward_error.is_finite() && backward_error <= 1.0e-10).then_some(solution)
}

#[derive(Debug, Clone, Copy)]
struct PickupStepSolution {
    tip_velocity_m_s: [f64; 2],
    body_velocity_m_s: [f64; 2],
    wall_projected_force_n: [f64; 2],
    wall_contact: [bool; 2],
    land_normal_force_n: f64,
    bearing_friction_force_n: f64,
}

#[derive(Debug, Clone, Copy)]
struct PickupTangentialForces {
    coulomb_friction_force_n: f64,
    modulation_reaction_force_n: f64,
    record_reaction_force_tangent_n: f64,
    relative_velocity_m_s: f64,
    mode: StylusTangentialMode,
}

#[allow(clippy::too_many_arguments)]
fn solve_pickup_step(
    contact: StylusContactConfig,
    tonearm: TonearmConfig,
    tip_displacement_m: [f64; 2],
    tip_velocity_m_s: [f64; 2],
    body_displacement_m: [f64; 2],
    body_velocity_m_s: [f64; 2],
    body_external_force_n: [f64; 2],
    input: PickupMechanicalInput,
    electromagnetic_relation: PickupElectromagneticForceRelation,
    dt: f64,
) -> Result<PickupStepSolution, PickupMechanicalError> {
    let mut constraint_normals = [[0.0_f64; 2]; 2];
    let mut constraint_displacement_m = [0.0_f64; 2];
    let mut tangential_force_per_normal = [0.0_f64; 2];
    let constraint_count = if !input.stylus_lowered {
        0
    } else {
        match input.contact_surface {
            PickupContactSurface::None => 0,
            PickupContactSurface::GrooveWalls => {
                constraint_normals = WALL_NORMALS;
                constraint_displacement_m = wall_displacement_m(input);
                let effective_slope = wall_effective_slope(input);
                let surface_normal_scale = wall_effective_surface_normal_scale(input);
                for (constraint, tangential_force) in
                    tangential_force_per_normal.iter_mut().enumerate()
                {
                    *tangential_force = signed_direction(input.groove_tangential_velocity_m_s)
                        * contact.groove_friction_coefficient
                        * surface_normal_scale[constraint]
                        + effective_slope[constraint];
                }
                2
            }
            PickupContactSurface::RecordLand => {
                constraint_normals[0] = [0.0, 1.0];
                constraint_displacement_m[0] = input.land_displacement_m;
                tangential_force_per_normal[0] =
                    signed_direction(input.groove_tangential_velocity_m_s)
                        * contact.record_surface_friction_coefficient;
                1
            }
        }
    };
    let active_masks: &[u8] = match constraint_count {
        0 => &[0b00],
        1 => &[0b01, 0b00],
        2 => &[0b11, 0b01, 0b10, 0b00],
        _ => return Err(PickupMechanicalError::NumericalFailure),
    };
    let skating_factor = if input.stylus_lowered {
        tonearm
            .geometry
            .equivalent_radial_force_n(input.groove_radius_m, 1.0)?
    } else {
        0.0
    };

    let bearing_modes = if body_velocity_m_s[0] > BEARING_VELOCITY_TOLERANCE_M_S {
        [
            BearingMode::Positive,
            BearingMode::Stick,
            BearingMode::Negative,
        ]
    } else if body_velocity_m_s[0] < -BEARING_VELOCITY_TOLERANCE_M_S {
        [
            BearingMode::Negative,
            BearingMode::Stick,
            BearingMode::Positive,
        ]
    } else {
        [
            BearingMode::Stick,
            BearingMode::Positive,
            BearingMode::Negative,
        ]
    };
    for bearing_mode in bearing_modes {
        for &active_mask in active_masks {
            let mut active_constraints = [0_usize; 2];
            let mut active_count = 0;
            for constraint in 0..constraint_count {
                if active_mask & (1 << constraint) != 0 {
                    active_constraints[active_count] = constraint;
                    active_count += 1;
                }
            }
            let bearing_sticks = bearing_mode == BearingMode::Stick;
            let variable_count = 4 + active_count + usize::from(bearing_sticks);
            let mut augmented = [[0.0_f64; 8]; 7];
            let axes = [tonearm.lateral, tonearm.vertical];
            for axis in 0..2 {
                let tip_row = axis * 2;
                let body_row = tip_row + 1;
                let tip_velocity_column = axis;
                let body_velocity_column = 2 + axis;
                let stiffness = axes[axis].stiffness_n_per_m();
                let damping = axes[axis].viscous_damping_n_s_per_m();
                let coupling = stiffness * dt + damping;
                let relative_displacement = tip_displacement_m[axis] - body_displacement_m[axis];

                augmented[tip_row][tip_velocity_column] = contact.moving_mass_kg / dt + coupling;
                augmented[tip_row][body_velocity_column] = -coupling;
                augmented[tip_row][variable_count] = contact.moving_mass_kg / dt
                    * tip_velocity_m_s[axis]
                    - stiffness * relative_displacement
                    + electromagnetic_relation.force_bias_n[axis];

                augmented[body_row][tip_velocity_column] = -coupling;
                augmented[body_row][body_velocity_column] =
                    axes[axis].effective_mass_kg / dt + coupling;
                augmented[body_row][variable_count] = axes[axis].effective_mass_kg / dt
                    * body_velocity_m_s[axis]
                    + stiffness * relative_displacement
                    + body_external_force_n[axis]
                    - electromagnetic_relation.force_bias_n[axis];

                for velocity_axis in 0..2 {
                    let reciprocal_damping =
                        electromagnetic_relation.reciprocal_damping_n_s_per_m[axis][velocity_axis];
                    augmented[tip_row][velocity_axis] += reciprocal_damping;
                    augmented[tip_row][2 + velocity_axis] -= reciprocal_damping;
                    augmented[body_row][velocity_axis] -= reciprocal_damping;
                    augmented[body_row][2 + velocity_axis] += reciprocal_damping;
                }

                if axis == 0 {
                    augmented[body_row][body_velocity_column] +=
                        tonearm.lateral_bearing_viscous_damping_n_s_per_m;
                    augmented[body_row][variable_count] += match bearing_mode {
                        BearingMode::Stick => 0.0,
                        BearingMode::Positive => -tonearm.lateral_bearing_kinetic_friction_n,
                        BearingMode::Negative => tonearm.lateral_bearing_kinetic_friction_n,
                    };
                    if bearing_sticks {
                        let bearing_force_column = 4 + active_count;
                        augmented[body_row][bearing_force_column] = -1.0;
                    }
                }

                if axis == 1 && !input.stylus_lowered {
                    let cue_coupling = tonearm.cue_support_stiffness_n_per_m * dt
                        + tonearm.cue_support_damping_n_s_per_m;
                    augmented[body_row][body_velocity_column] += cue_coupling;
                    augmented[body_row][variable_count] += tonearm.cue_support_stiffness_n_per_m
                        * (tonearm.cue_lift_height_m - body_displacement_m[1]);
                }

                for (active_index, &constraint) in
                    active_constraints[..active_count].iter().enumerate()
                {
                    let lambda_column = 4 + active_index;
                    augmented[tip_row][lambda_column] = -constraint_normals[constraint][axis];
                    if axis == 0 {
                        augmented[body_row][lambda_column] =
                            -skating_factor * tangential_force_per_normal[constraint];
                    }
                }
            }

            for (active_index, &constraint) in active_constraints[..active_count].iter().enumerate()
            {
                let row = 4 + active_index;
                augmented[row][0] = constraint_normals[constraint][0];
                augmented[row][1] = constraint_normals[constraint][1];
                augmented[row][variable_count] = (constraint_displacement_m[constraint]
                    - dot(constraint_normals[constraint], tip_displacement_m))
                    / dt;
            }
            if bearing_sticks {
                let row = 4 + active_count;
                augmented[row][2] = 1.0;
                augmented[row][variable_count] = 0.0;
            }

            let Some(solution) = solve_linear_system(&mut augmented, variable_count) else {
                continue;
            };
            let next_tip_velocity = [solution[0], solution[1]];
            let next_body_velocity = [solution[2], solution[3]];
            let next_tip_displacement = [
                tip_displacement_m[0] + next_tip_velocity[0] * dt,
                tip_displacement_m[1] + next_tip_velocity[1] * dt,
            ];
            let mut constraint_force_n = [0.0; 2];
            let mut valid = true;
            for constraint in 0..constraint_count {
                let gap = dot(constraint_normals[constraint], next_tip_displacement)
                    - constraint_displacement_m[constraint];
                if active_mask & (1 << constraint) == 0 {
                    valid &= gap >= -CONTACT_TOLERANCE_M;
                }
            }
            for (active_index, &constraint) in active_constraints[..active_count].iter().enumerate()
            {
                let force = solution[4 + active_index];
                valid &= force >= -1.0e-10;
                constraint_force_n[constraint] = force.max(0.0);
            }
            let bearing_friction_force_n = match bearing_mode {
                BearingMode::Stick => {
                    let force = solution[4 + active_count];
                    valid &= force.abs() <= tonearm.lateral_bearing_static_friction_n + 1.0e-10;
                    force
                }
                BearingMode::Positive => {
                    valid &= next_body_velocity[0] >= -BEARING_VELOCITY_TOLERANCE_M_S;
                    -tonearm.lateral_bearing_kinetic_friction_n
                        - tonearm.lateral_bearing_viscous_damping_n_s_per_m * next_body_velocity[0]
                }
                BearingMode::Negative => {
                    valid &= next_body_velocity[0] <= BEARING_VELOCITY_TOLERANCE_M_S;
                    tonearm.lateral_bearing_kinetic_friction_n
                        - tonearm.lateral_bearing_viscous_damping_n_s_per_m * next_body_velocity[0]
                }
            };
            if valid
                && next_tip_velocity
                    .iter()
                    .chain(&next_body_velocity)
                    .chain(&constraint_force_n)
                    .chain([bearing_friction_force_n].iter())
                    .all(|value| value.is_finite())
            {
                let (wall_projected_force_n, land_normal_force_n) = match input.contact_surface {
                    PickupContactSurface::GrooveWalls if input.stylus_lowered => {
                        (constraint_force_n, 0.0)
                    }
                    PickupContactSurface::RecordLand if input.stylus_lowered => {
                        ([0.0; 2], constraint_force_n[0])
                    }
                    _ => ([0.0; 2], 0.0),
                };
                return Ok(PickupStepSolution {
                    tip_velocity_m_s: next_tip_velocity,
                    body_velocity_m_s: next_body_velocity,
                    wall_projected_force_n,
                    wall_contact: wall_projected_force_n.map(|force| force > 0.0),
                    land_normal_force_n,
                    bearing_friction_force_n,
                });
            }
        }
    }
    Err(PickupMechanicalError::ConstraintFailure)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BearingMode {
    Stick,
    Positive,
    Negative,
}

fn solve_linear_system(augmented: &mut [[f64; 8]; 7], size: usize) -> Option<[f64; 7]> {
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
        for value in &mut augmented[pivot_column][pivot_column..=size] {
            *value /= pivot;
        }
        let pivot_values = augmented[pivot_column];
        for (row, augmented_row) in augmented.iter_mut().enumerate().take(size) {
            if row == pivot_column {
                continue;
            }
            let scale = augmented_row[pivot_column];
            for (value, pivot_value) in augmented_row[pivot_column..=size]
                .iter_mut()
                .zip(&pivot_values[pivot_column..=size])
            {
                *value -= scale * pivot_value;
            }
        }
    }
    let mut solution = [0.0; 7];
    for row in 0..size {
        solution[row] = augmented[row][size];
    }
    solution
        .iter()
        .take(size)
        .all(|value| value.is_finite())
        .then_some(solution)
}

fn validate_sample_rate(sample_rate_hz: f64) -> Result<(), PickupMechanicalError> {
    if sample_rate_hz.is_finite()
        && (MIN_SAMPLE_RATE_HZ..=MAX_SAMPLE_RATE_HZ).contains(&sample_rate_hz)
    {
        Ok(())
    } else {
        Err(PickupMechanicalError::InvalidSampleRate)
    }
}

fn validate_temporal_resolution(
    contact: StylusContactConfig,
    tonearm: TonearmConfig,
    sample_rate_hz: f64,
) -> Result<(), PickupMechanicalError> {
    let dt = 1.0 / sample_rate_hz;
    for (axis, config) in [("lateral", tonearm.lateral), ("vertical", tonearm.vertical)] {
        let reduced_mass = contact.moving_mass_kg * config.effective_mass_kg
            / (contact.moving_mass_kg + config.effective_mass_kg);
        let natural_angular_frequency = (config.stiffness_n_per_m() / reduced_mass).sqrt();
        if natural_angular_frequency * dt >= 0.5 {
            return Err(PickupMechanicalError::InsufficientTemporalResolution { axis });
        }
    }
    Ok(())
}

fn wall_contact_set_is_valid(set: StylusTraceContactSet) -> bool {
    let Some(count) = wall_contact_count(set) else {
        return false;
    };
    set.center_displacement_m.is_finite()
        && set.contacts[..count].iter().all(|contact| {
            [
                contact.contact_offset_m,
                contact.groove_displacement_m,
                contact.groove_slope,
                contact.tangent_residual,
            ]
            .into_iter()
            .all(f64::is_finite)
        })
        && set.contacts[..count]
            .windows(2)
            .all(|pair| pair[0].contact_offset_m < pair[1].contact_offset_m)
        && set.contacts[count..]
            .iter()
            .all(|contact| *contact == Default::default())
}

fn reflection_symmetry_is_valid(set: StylusTraceContactSet) -> bool {
    if !wall_contact_set_is_valid(set) || set.contact_count != 2 {
        return false;
    }
    let left = set.contacts[0];
    let right = set.contacts[1];
    left.contact_offset_m < 0.0
        && right.contact_offset_m > 0.0
        && left.contact_offset_m == -right.contact_offset_m
        && left.groove_displacement_m == right.groove_displacement_m
        && left.groove_slope == -right.groove_slope
        && left.tangent_residual == -right.tangent_residual
}

fn validate_input(input: PickupMechanicalInput) -> Result<(), PickupMechanicalError> {
    let wall_contacts_are_valid = input
        .wall_contacts
        .into_iter()
        .all(wall_contact_set_is_valid);
    if !wall_contacts_are_valid
        || input
            .electromagnetic_force_n
            .iter()
            .chain(
                [
                    input.land_displacement_m,
                    input.groove_radius_m,
                    input.groove_tangential_velocity_m_s,
                ]
                .iter(),
            )
            .any(|value| !value.is_finite())
        || input.groove_radius_m <= 0.0
    {
        return Err(PickupMechanicalError::InvalidInput);
    }
    for (contacts, qualification) in input
        .wall_contacts
        .into_iter()
        .zip(input.wall_contact_qualification)
    {
        match qualification {
            WallContactQualification::Unique if contacts.contact_count != 1 => {
                return Err(PickupMechanicalError::UnqualifiedMultipleContacts);
            }
            WallContactQualification::CertifiedReflectionSymmetry
                if !reflection_symmetry_is_valid(contacts) =>
            {
                return Err(PickupMechanicalError::InvalidCommonHeightCertificate);
            }
            _ => {}
        }
    }
    Ok(())
}

fn wall_contact_telemetry_is_valid(telemetry: PickupWallContactTelemetry) -> bool {
    if !wall_contact_set_is_valid(telemetry.geometry)
        || (telemetry.geometry.contact_count != 1
            && !reflection_symmetry_is_valid(telemetry.geometry))
    {
        return false;
    }
    let count = telemetry.contact_count();
    for contact_index in 0..MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL {
        let values = [
            telemetry.projected_normal_force_n[contact_index],
            telemetry.surface_normal_force_n[contact_index],
            telemetry.modulation_reaction_force_n[contact_index],
            telemetry.coulomb_friction_force_n[contact_index],
            telemetry.record_reaction_force_tangent_n[contact_index],
        ];
        if values.into_iter().any(|value| !value.is_finite()) {
            return false;
        }
        if contact_index >= count {
            if values != [0.0; 5] {
                return false;
            }
            continue;
        }
        let contact = telemetry.geometry.contacts[contact_index];
        let projected_force_n = telemetry.projected_normal_force_n[contact_index];
        if projected_force_n < 0.0
            || telemetry.surface_normal_force_n[contact_index] < 0.0
            || !nearly_equal_force(
                telemetry.surface_normal_force_n[contact_index],
                projected_force_n * contact.groove_slope.hypot(1.0),
            )
            || !nearly_equal_force(
                telemetry.modulation_reaction_force_n[contact_index],
                -projected_force_n * contact.groove_slope,
            )
            || !nearly_equal_force(
                telemetry.record_reaction_force_tangent_n[contact_index],
                telemetry.modulation_reaction_force_n[contact_index]
                    + telemetry.coulomb_friction_force_n[contact_index],
            )
        {
            return false;
        }
    }
    true
}

fn validate_snapshot(snapshot: PickupMechanicalSnapshot) -> Result<(), PickupMechanicalError> {
    if snapshot.version != SNAPSHOT_VERSION {
        return Err(PickupMechanicalError::InvalidSnapshot);
    }
    snapshot.contact.validate()?;
    snapshot.tonearm.validate()?;
    validate_sample_rate(snapshot.sample_rate_hz)?;
    validate_temporal_resolution(snapshot.contact, snapshot.tonearm, snapshot.sample_rate_hz)?;
    if snapshot
        .tip_displacement_m
        .iter()
        .chain(&snapshot.tip_velocity_m_s)
        .chain(&snapshot.body_displacement_m)
        .chain(&snapshot.body_velocity_m_s)
        .any(|value| !value.is_finite())
    {
        return Err(PickupMechanicalError::InvalidSnapshot);
    }
    let telemetry = snapshot.last_telemetry;
    let wall_longitudinal_is_valid = telemetry
        .wall_longitudinal_contact
        .into_iter()
        .all(wall_contact_telemetry_is_valid);
    let longitudinal_modulation_reaction_force_n = telemetry
        .wall_longitudinal_contact
        .into_iter()
        .map(PickupWallContactTelemetry::total_modulation_reaction_force_n)
        .sum::<f64>();
    let longitudinal_coulomb_friction_force_n = telemetry
        .wall_longitudinal_contact
        .into_iter()
        .map(PickupWallContactTelemetry::total_coulomb_friction_force_n)
        .sum::<f64>();
    let telemetry_is_finite = telemetry
        .tip_displacement_m
        .iter()
        .chain(&telemetry.tip_velocity_m_s)
        .chain(&telemetry.body_displacement_m)
        .chain(&telemetry.body_velocity_m_s)
        .chain(&telemetry.relative_displacement_m)
        .chain(&telemetry.relative_velocity_m_s)
        .chain(&telemetry.electromagnetic_port_velocity_m_s)
        .chain(&telemetry.suspension_force_on_tip_n)
        .chain(&telemetry.electromagnetic_force_on_tip_n)
        .chain(&telemetry.wall_gap_m)
        .chain(&telemetry.wall_normal_force_n)
        .chain(
            [
                telemetry.land_gap_m,
                telemetry.land_normal_force_n,
                telemetry.coulomb_friction_force_n,
                telemetry.modulation_reaction_force_n,
                telemetry.record_reaction_force_tangent_n,
                telemetry.tangential_relative_velocity_m_s,
                telemetry.tangential_friction_power_w,
                telemetry.groove_radius_m,
                telemetry.skating_force_n,
                telemetry.bearing_friction_force_n,
                telemetry.groove_lateral_force_on_tip_n,
                telemetry.kinetic_energy_j,
                telemetry.suspension_energy_j,
            ]
            .iter(),
        )
        .all(|value| value.is_finite());
    if !telemetry_is_finite
        || !wall_longitudinal_is_valid
        || telemetry.completed_steps != snapshot.completed_steps
        || telemetry.tip_displacement_m != snapshot.tip_displacement_m
        || telemetry.tip_velocity_m_s != snapshot.tip_velocity_m_s
        || telemetry.body_displacement_m != snapshot.body_displacement_m
        || telemetry.body_velocity_m_s != snapshot.body_velocity_m_s
        || telemetry
            .wall_normal_force_n
            .iter()
            .any(|force| *force < 0.0)
        || !(0..2).all(|wall| {
            nearly_equal_force(
                telemetry.wall_normal_force_n[wall],
                telemetry.wall_longitudinal_contact[wall].total_surface_normal_force_n(),
            ) && telemetry.wall_contact[wall]
                == (telemetry.wall_longitudinal_contact[wall].total_projected_normal_force_n()
                    > 0.0)
        })
        || !nearly_equal_force(
            telemetry.modulation_reaction_force_n,
            longitudinal_modulation_reaction_force_n,
        )
        || (telemetry.contact_surface == PickupContactSurface::GrooveWalls
            && (!nearly_equal_force(
                telemetry.record_reaction_force_tangent_n,
                telemetry.longitudinal_record_reaction_force_tangent_n(),
            ) || !nearly_equal_force(
                telemetry.coulomb_friction_force_n,
                longitudinal_coulomb_friction_force_n,
            )))
        || telemetry.land_normal_force_n < 0.0
        || telemetry.kinetic_energy_j < 0.0
        || telemetry.suspension_energy_j < 0.0
        || telemetry.tangential_friction_power_w > 1.0e-18
        || (!telemetry.stylus_lowered
            && (telemetry.contact_surface != PickupContactSurface::None
                || telemetry.wall_contact != [false; 2]
                || telemetry.wall_normal_force_n != [0.0; 2]
                || telemetry.wall_gap_m != [0.0; 2]
                || telemetry.land_contact
                || telemetry.land_normal_force_n != 0.0
                || telemetry.land_gap_m != 0.0))
        || (telemetry.contact_surface == PickupContactSurface::GrooveWalls
            && (telemetry.land_contact
                || telemetry.land_normal_force_n != 0.0
                || telemetry.land_gap_m != 0.0))
        || (telemetry.contact_surface == PickupContactSurface::RecordLand
            && (telemetry.wall_contact != [false; 2]
                || telemetry.wall_normal_force_n != [0.0; 2]
                || telemetry.wall_gap_m != [0.0; 2]))
    {
        return Err(PickupMechanicalError::InvalidSnapshot);
    }
    Ok(())
}

fn subtract(left: [f64; 2], right: [f64; 2]) -> [f64; 2] {
    [left[0] - right[0], left[1] - right[1]]
}

fn matrix_vector(matrix: [[f64; 2]; 2], vector: [f64; 2]) -> [f64; 2] {
    [
        matrix[0][0] * vector[0] + matrix[0][1] * vector[1],
        matrix[1][0] * vector[0] + matrix[1][1] * vector[1],
    ]
}

fn signed_direction(value: f64) -> f64 {
    if value > 0.0 {
        1.0
    } else if value < 0.0 {
        -1.0
    } else {
        0.0
    }
}

fn dot(left: [f64; 2], right: [f64; 2]) -> f64 {
    left[0] * right[0] + left[1] * right[1]
}

fn nearly_equal_force(left: f64, right: f64) -> bool {
    let scale = left.abs().max(right.abs()).max(1.0e-30);
    (left - right).abs() <= 1.0e-12 * scale + 1.0e-18
}

fn zero_telemetry() -> PickupMechanicalTelemetry {
    PickupMechanicalTelemetry {
        tip_displacement_m: [0.0; 2],
        tip_velocity_m_s: [0.0; 2],
        body_displacement_m: [0.0; 2],
        body_velocity_m_s: [0.0; 2],
        relative_displacement_m: [0.0; 2],
        relative_velocity_m_s: [0.0; 2],
        electromagnetic_port_velocity_m_s: [0.0; 2],
        suspension_force_on_tip_n: [0.0; 2],
        electromagnetic_force_on_tip_n: [0.0; 2],
        wall_gap_m: [0.0; 2],
        wall_normal_force_n: [0.0; 2],
        wall_contact: [false; 2],
        wall_longitudinal_contact: [PickupWallContactTelemetry::default(); 2],
        land_gap_m: 0.0,
        land_normal_force_n: 0.0,
        land_contact: false,
        contact_surface: PickupContactSurface::None,
        coulomb_friction_force_n: 0.0,
        modulation_reaction_force_n: 0.0,
        record_reaction_force_tangent_n: 0.0,
        tangential_mode: StylusTangentialMode::Separated,
        tangential_relative_velocity_m_s: 0.0,
        tangential_friction_power_w: 0.0,
        groove_radius_m: 0.146_05,
        skating_force_n: 0.0,
        bearing_friction_force_n: 0.0,
        groove_lateral_force_on_tip_n: 0.0,
        kinetic_energy_j: 0.0,
        suspension_energy_j: 0.0,
        stylus_lowered: false,
        completed_steps: 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum PickupMechanicalError {
    #[error("pickup configuration field {field} is invalid")]
    InvalidConfig { field: &'static str },
    #[error("pickup sample rate is outside the supported range")]
    InvalidSampleRate,
    #[error("pickup timestep does not resolve the {axis} resonance")]
    InsufficientTemporalResolution { axis: &'static str },
    #[error("pickup input contains an invalid value")]
    InvalidInput,
    #[error("multiple wall contacts require a validated common-height qualification")]
    UnqualifiedMultipleContacts,
    #[error("the common-height contact qualification is invalid")]
    InvalidCommonHeightCertificate,
    #[error("the electromagnetic force relation is nonfinite or nonpassive")]
    InvalidElectromagneticRelation,
    #[error("pickup snapshot is invalid")]
    InvalidSnapshot,
    #[error("pickup contact constraint failed")]
    ConstraintFailure,
    #[error("pickup integration produced a nonfinite value")]
    NumericalFailure,
    #[error(transparent)]
    Tonearm(#[from] super::TonearmError),
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum CoupledDeckPickupError {
    #[error(transparent)]
    Pickup(#[from] PickupMechanicalError),
    #[error(transparent)]
    Deck(#[from] DeckMechanicalError),
    #[error("the midpoint geometry is nonfinite or uses a different timestep")]
    InvalidMidpointGeometry,
    #[error(
        "the joint deck and pickup constraints have no consistent mode after {evaluated_branches} branches and {attempted_linear_solves} linear solves"
    )]
    NoConsistentMode {
        evaluated_branches: u32,
        attempted_linear_solves: u32,
    },
    #[error("the deck and pickup use different stylus reaction torques")]
    ReciprocityMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall_contact_set(
        center_displacement_m: f64,
        contacts: &[(f64, f64)],
    ) -> StylusTraceContactSet {
        assert!(!contacts.is_empty());
        assert!(contacts.len() <= MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL);
        let mut set = StylusTraceContactSet {
            center_displacement_m,
            contact_count: contacts.len() as u8,
            ..StylusTraceContactSet::default()
        };
        for (index, (contact_offset_m, groove_slope)) in contacts.iter().copied().enumerate() {
            set.contacts[index] = super::super::stylus::StylusTraceContact {
                contact_offset_m,
                groove_displacement_m: center_displacement_m,
                groove_slope,
                tangent_residual: 0.0,
            };
        }
        set
    }

    fn bridged_input(
        center_displacement_m: [f64; 2],
        left_slope: [f64; 2],
        right_slope: [f64; 2],
        groove_tangential_velocity_m_s: f64,
    ) -> PickupMechanicalInput {
        let wall_contacts = [0, 1].map(|wall| {
            wall_contact_set(
                center_displacement_m[wall],
                &[
                    (-1.754_028e-6, left_slope[wall]),
                    (1.754_028e-6, right_slope[wall]),
                ],
            )
        });
        PickupMechanicalInput {
            wall_contacts,
            groove_tangential_velocity_m_s,
            ..PickupMechanicalInput::default()
        }
    }

    fn changing_contact_input(sample: usize) -> PickupMechanicalInput {
        let phase = std::f64::consts::TAU * sample as f64 / 197.5;
        let center = [0.8e-6 * phase.sin(), 0.6e-6 * (phase * 0.73).cos()];
        let contact_phase = sample % 96;
        let wall_contacts = [0, 1].map(|wall| {
            let slope = if wall == 0 { 0.03 } else { -0.02 };
            if contact_phase < 32 {
                wall_contact_set(center[wall], &[(-1.754_028e-6, slope)])
            } else if contact_phase < 64 {
                wall_contact_set(
                    center[wall],
                    &[(-1.754_028e-6, slope), (1.754_028e-6, -slope)],
                )
            } else {
                wall_contact_set(center[wall], &[(1.754_028e-6, -slope)])
            }
        });
        let mut input = PickupMechanicalInput::default();
        if wall_contacts[0].contact_count == 1 {
            input.set_unique_wall_contacts(wall_contacts).unwrap();
        } else {
            input
                .set_certified_reflection_symmetric_wall_contacts(wall_contacts)
                .unwrap();
        }
        input.groove_tangential_velocity_m_s = if sample % 128 < 64 { 0.7 } else { -0.7 };
        input
    }

    fn state() -> PickupMechanicalState {
        PickupMechanicalState::new(
            StylusContactConfig::default(),
            TonearmConfig::default(),
            192_000.0,
        )
        .unwrap()
    }

    #[test]
    fn static_groove_supports_tracking_force_on_both_walls() {
        let mut state = state();
        let mut telemetry = state.process(PickupMechanicalInput::default()).unwrap();
        for _ in 0..200_000 {
            telemetry = state.process(PickupMechanicalInput::default()).unwrap();
        }
        assert_eq!(telemetry.wall_contact, [true, true]);
        let supported_vertical =
            (telemetry.wall_normal_force_n[0] + telemetry.wall_normal_force_n[1]) * INVERSE_SQRT_2;
        assert!(
            (supported_vertical - state.tonearm.vertical_tracking_force_n).abs() < 2.0e-4,
            "{telemetry:?}"
        );
    }

    #[test]
    fn symmetric_bridge_uses_the_minimum_norm_force_split() {
        let input = PickupMechanicalInput {
            wall_contacts: [
                wall_contact_set(0.0, &[(-1.754_028e-6, 0.25), (1.754_028e-6, -0.25)]),
                StylusTraceContactSet::default(),
            ],
            ..PickupMechanicalInput::default()
        };
        let distributed = distribute_wall_contact_forces(input, [2.0, 0.0], -0.4);
        let wall = distributed[0];
        assert_eq!(wall.contact_count(), 2);
        assert_eq!(wall.projected_normal_force_n[..2], [1.0, 1.0]);
        assert_eq!(
            wall.surface_normal_force_n[0],
            wall.surface_normal_force_n[1]
        );
        assert_eq!(wall.coulomb_friction_force_n[..2], [-0.2, -0.2]);
        assert_eq!(wall.total_projected_normal_force_n(), 2.0);
        assert_eq!(wall.total_modulation_reaction_force_n(), 0.0);
        assert_eq!(wall.total_coulomb_friction_force_n(), -0.4);
        assert_eq!(wall.total_record_reaction_force_tangent_n(), -0.4);
    }

    #[test]
    fn a_perturbed_bridge_assigns_the_complete_load_to_the_unique_contact() {
        let input = PickupMechanicalInput {
            wall_contacts: [
                wall_contact_set(1.0e-12, &[(1.754_028e-6, -0.25)]),
                StylusTraceContactSet::default(),
            ],
            ..PickupMechanicalInput::default()
        };
        let distributed = distribute_wall_contact_forces(input, [2.0, 0.0], -0.4);
        let wall = distributed[0];
        assert_eq!(wall.contact_count(), 1);
        assert_eq!(wall.projected_normal_force_n[0], 2.0);
        assert_eq!(wall.coulomb_friction_force_n[0], -0.4);
        assert!(nearly_equal_force(
            wall.total_record_reaction_force_tangent_n(),
            0.1,
        ));
        assert_eq!(wall.projected_normal_force_n[1..], [0.0; 7]);
    }

    #[test]
    fn bridge_merge_and_split_preserve_force_across_a_reversal() {
        let left = PickupMechanicalInput {
            wall_contacts: [
                wall_contact_set(0.0, &[(-1.754_028e-6, 0.4)]),
                StylusTraceContactSet::default(),
            ],
            ..PickupMechanicalInput::default()
        };
        let bridge = PickupMechanicalInput {
            wall_contacts: [
                wall_contact_set(0.0, &[(-1.754_028e-6, 0.4), (1.754_028e-6, -0.4)]),
                StylusTraceContactSet::default(),
            ],
            ..PickupMechanicalInput::default()
        };
        let right = PickupMechanicalInput {
            wall_contacts: [
                wall_contact_set(0.0, &[(1.754_028e-6, -0.4)]),
                StylusTraceContactSet::default(),
            ],
            ..PickupMechanicalInput::default()
        };
        let sequence = [left, bridge, right]
            .map(|input| distribute_wall_contact_forces(input, [1.25, 0.0], 0.0)[0]);
        for wall in sequence {
            assert_eq!(wall.total_projected_normal_force_n(), 1.25);
            assert_eq!(
                wall.total_surface_normal_force_n(),
                1.25 * 0.4_f64.hypot(1.0)
            );
        }
        assert_eq!(sequence[0].total_modulation_reaction_force_n(), -0.5);
        assert_eq!(sequence[1].total_modulation_reaction_force_n(), 0.0);
        assert_eq!(sequence[2].total_modulation_reaction_force_n(), 0.5);
    }

    #[test]
    fn longitudinal_force_torque_and_friction_power_balance() {
        let input = bridged_input([0.0; 2], [0.3, -0.2], [-0.1, 0.4], 0.7);
        let distributed = distribute_wall_contact_forces(input, [1.2, 0.8], -0.35);
        let projected_force_n = distributed
            .into_iter()
            .map(PickupWallContactTelemetry::total_projected_normal_force_n)
            .sum::<f64>();
        let friction_force_n = distributed
            .into_iter()
            .map(PickupWallContactTelemetry::total_coulomb_friction_force_n)
            .sum::<f64>();
        let reaction_force_n = distributed
            .into_iter()
            .map(PickupWallContactTelemetry::total_record_reaction_force_tangent_n)
            .sum::<f64>();
        let modulation_force_n = distributed
            .into_iter()
            .map(PickupWallContactTelemetry::total_modulation_reaction_force_n)
            .sum::<f64>();
        assert_eq!(projected_force_n, 2.0);
        assert_eq!(friction_force_n, -0.35);
        assert_eq!(reaction_force_n, modulation_force_n + friction_force_n);
        assert!(friction_force_n * input.groove_tangential_velocity_m_s <= 0.0);
        let radius_m = 0.082_505_922_498_838_55;
        assert_eq!(
            reaction_force_n * radius_m,
            (modulation_force_n - 0.35) * radius_m
        );
        assert_no_alloc::assert_no_alloc(|| {
            let _ = distribute_wall_contact_forces(input, [1.2, 0.8], -0.35);
        });
    }

    #[test]
    fn over_capacity_contact_count_rejects_without_state_change() {
        let mut state = state();
        let before = state;
        let mut input = PickupMechanicalInput::default();
        input.wall_contacts[0].contact_count = (MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL + 1) as u8;
        assert_eq!(
            state.process(input),
            Err(PickupMechanicalError::InvalidInput)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn fabricated_multi_contact_input_cannot_claim_certification() {
        let contacts = wall_contact_set(0.0, &[(-1.754_028e-6, 0.25), (1.754_028e-6, -0.25)]);
        let mut input = PickupMechanicalInput {
            wall_contacts: [contacts; 2],
            ..PickupMechanicalInput::default()
        };
        let mut encoded = serde_json::to_value(input).unwrap();
        encoded.as_object_mut().unwrap().insert(
            "wallContactQualification".to_owned(),
            serde_json::json!(["certifiedReflectionSymmetry", "certifiedReflectionSymmetry"]),
        );
        input = serde_json::from_value(encoded).unwrap();
        assert_eq!(
            input.wall_contact_qualification,
            [WallContactQualification::Unique; 2]
        );
        let mut state = state();
        let before = state;
        assert_eq!(
            state.process(input),
            Err(PickupMechanicalError::UnqualifiedMultipleContacts)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn conventional_inner_groove_height_order_ambiguity_stays_transactional() {
        let sample_rate_hz = 192_000.0;
        let frequency_hz = 8_000.0;
        let amplitude_m = 0.05 / (std::f64::consts::TAU * frequency_hz);
        let samples: Vec<f32> = (0..512)
            .map(|index| {
                (amplitude_m
                    * (std::f64::consts::TAU * frequency_hz * index as f64 / sample_rate_hz).sin())
                    as f32
            })
            .collect();
        let meters_per_frame =
            std::f64::consts::TAU * (33.333_333_333_333_336 / 60.0) * 0.060 / sample_rate_hz;
        assert_eq!(
            super::super::stylus::trace_spherical_uniform_contacts(
                &samples,
                258.0,
                meters_per_frame,
                super::super::stylus::StylusGeometry::default(),
            ),
            Err(super::super::stylus::StylusTraceError::ContactHeightOrderNotIsolated)
        );
    }

    #[test]
    fn opposite_wall_offsets_produce_lateral_motion() {
        let mut state = state();
        let wall = 10.0e-6;
        let input = PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts([wall, -wall], [0.0; 2]),
            ..PickupMechanicalInput::default()
        };
        let mut telemetry = state.process(input).unwrap();
        for _ in 0..200_000 {
            telemetry = state.process(input).unwrap();
        }
        assert!(
            (telemetry.tip_displacement_m[0] - std::f64::consts::SQRT_2 * wall).abs() < 2.0e-8,
            "{telemetry:?}"
        );
        assert!(telemetry.tip_displacement_m[1].abs() < 2.0e-8);
    }

    #[test]
    fn suspension_and_electromagnetic_forces_are_internal_pairs() {
        let mut state = state();
        state
            .reset([2.0e-6, -1.0e-6], [0.1, -0.2], [0.0; 2], [0.0; 2])
            .unwrap();
        let before_momentum = [
            state.contact.moving_mass_kg * state.tip_velocity_m_s[0]
                + state.tonearm.lateral.effective_mass_kg * state.body_velocity_m_s[0],
            state.contact.moving_mass_kg * state.tip_velocity_m_s[1]
                + state.tonearm.vertical.effective_mass_kg * state.body_velocity_m_s[1],
        ];
        let mut input = PickupMechanicalInput {
            stylus_lowered: false,
            electromagnetic_force_n: [0.002, -0.003],
            ..PickupMechanicalInput::default()
        };
        let tonearm = state.tonearm;
        input.electromagnetic_force_n = [0.002, -0.003];
        let telemetry = state.process(input).unwrap();
        let after_momentum = [
            state.contact.moving_mass_kg * state.tip_velocity_m_s[0]
                + tonearm.lateral.effective_mass_kg * state.body_velocity_m_s[0],
            state.contact.moving_mass_kg * state.tip_velocity_m_s[1]
                + tonearm.vertical.effective_mass_kg * state.body_velocity_m_s[1],
        ];
        let dt = 1.0 / state.sample_rate_hz;
        let cue_force_n = tonearm.cue_support_stiffness_n_per_m
            * (tonearm.cue_lift_height_m - state.body_displacement_m[1])
            - tonearm.cue_support_damping_n_s_per_m * state.body_velocity_m_s[1];
        let expected_external = [
            (tonearm.anti_skate_force_n + telemetry.bearing_friction_force_n) * dt,
            (-tonearm.vertical_tracking_force_n + cue_force_n) * dt,
        ];
        assert!((after_momentum[0] - before_momentum[0] - expected_external[0]).abs() < 1.0e-12);
        assert!((after_momentum[1] - before_momentum[1] - expected_external[1]).abs() < 1.0e-9);
    }

    #[test]
    fn affine_electromagnetic_force_is_solved_in_the_current_step() {
        let mut state = state();
        state
            .reset([0.0; 2], [0.5, -0.25], [0.0; 2], [0.0; 2])
            .unwrap();
        let relation =
            PickupElectromagneticForceRelation::new([0.0; 2], [[0.004, 0.001], [0.001, 0.003]])
                .unwrap();
        let telemetry = state
            .process_with_electromagnetic_relation(
                PickupMechanicalInput {
                    stylus_lowered: false,
                    electromagnetic_force_n: [123.0, -456.0],
                    ..PickupMechanicalInput::default()
                },
                relation,
            )
            .unwrap();
        let expected = relation
            .force_at_relative_velocity(telemetry.relative_velocity_m_s)
            .unwrap();
        assert_eq!(telemetry.electromagnetic_force_on_tip_n, expected);
        assert!(
            dot(
                telemetry.electromagnetic_force_on_tip_n,
                telemetry.relative_velocity_m_s
            ) <= 0.0
        );
    }

    #[test]
    fn nonfinite_asymmetric_and_nonpassive_relations_are_rejected() {
        assert_eq!(
            PickupElectromagneticForceRelation::new([f64::NAN, 0.0], [[0.0; 2]; 2]),
            Err(PickupMechanicalError::InvalidElectromagneticRelation)
        );
        assert_eq!(
            PickupElectromagneticForceRelation::new([0.0; 2], [[1.0, 0.2], [0.1, 1.0]]),
            Err(PickupMechanicalError::InvalidElectromagneticRelation)
        );
        assert_eq!(
            PickupElectromagneticForceRelation::new([0.0; 2], [[1.0, 2.0], [2.0, 1.0]]),
            Err(PickupMechanicalError::InvalidElectromagneticRelation)
        );
        assert_eq!(
            PickupElectromagneticForceRelation::new([0.0; 2], [[0.0, 1.0e-200], [1.0e-200, 0.0]],),
            Err(PickupMechanicalError::InvalidElectromagneticRelation)
        );
    }

    #[test]
    fn moving_modulated_wall_returns_signed_record_force() {
        let mut state = state();
        let mut input = PickupMechanicalInput::default();
        for _ in 0..20_000 {
            state.process(input).unwrap();
        }
        input.groove_tangential_velocity_m_s = 0.5;
        input.wall_contacts = test_single_wall_contacts([0.0; 2], [0.1, -0.04]);
        let telemetry = state.process(input).unwrap();
        let expected_modulation = -(telemetry.wall_normal_force_n[0] * 0.1 / 0.1_f64.hypot(1.0)
            + telemetry.wall_normal_force_n[1] * -0.04 / (-0.04_f64).hypot(1.0));
        assert!((telemetry.modulation_reaction_force_n - expected_modulation).abs() < 1.0e-12);
        assert!(telemetry.coulomb_friction_force_n < 0.0);
    }

    #[test]
    fn steep_wall_friction_uses_the_true_surface_normal_force() {
        let mut state = state();
        for _ in 0..20_000 {
            state.process(PickupMechanicalInput::default()).unwrap();
        }
        let input = PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts([0.0; 2], [3.0, -2.0]),
            groove_tangential_velocity_m_s: 0.5,
            ..PickupMechanicalInput::default()
        };
        let telemetry = state.process(input).unwrap();
        let expected_coulomb = -state.contact.groove_friction_coefficient
            * (telemetry.wall_normal_force_n[0] + telemetry.wall_normal_force_n[1]);
        assert!((telemetry.coulomb_friction_force_n - expected_coulomb).abs() < 1.0e-12);

        let projected_force = telemetry.wall_normal_force_n[0] / 3.0_f64.hypot(1.0)
            + telemetry.wall_normal_force_n[1] / (-2.0_f64).hypot(1.0);
        let incorrectly_projected_coulomb =
            -state.contact.groove_friction_coefficient * projected_force;
        assert!(
            (telemetry.coulomb_friction_force_n - incorrectly_projected_coulomb).abs()
                > telemetry.coulomb_friction_force_n.abs() * 0.25
        );
        let expected_modulation = -(telemetry.wall_normal_force_n[0] * 3.0 / 3.0_f64.hypot(1.0)
            + telemetry.wall_normal_force_n[1] * -2.0 / (-2.0_f64).hypot(1.0));
        assert!((telemetry.modulation_reaction_force_n - expected_modulation).abs() < 1.0e-12);
    }

    #[test]
    fn a_descending_wall_cannot_pull_the_stylus() {
        let mut state = state();
        for _ in 0..20_000 {
            state.process(PickupMechanicalInput::default()).unwrap();
        }
        let telemetry = state
            .process(PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts([-100.0e-6; 2], [0.0; 2]),
                ..PickupMechanicalInput::default()
            })
            .unwrap();
        assert_eq!(telemetry.wall_contact, [false, false]);
        assert_eq!(telemetry.wall_normal_force_n, [0.0, 0.0]);
        assert!(telemetry.wall_gap_m.into_iter().all(|gap| gap > 0.0));
    }

    #[test]
    fn a_rising_wall_pushes_without_penetration() {
        let mut state = state();
        let telemetry = state
            .process(PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts([2.0e-6; 2], [0.0; 2]),
                ..PickupMechanicalInput::default()
            })
            .unwrap();
        assert_eq!(telemetry.wall_contact, [true, true]);
        assert!(telemetry
            .wall_normal_force_n
            .into_iter()
            .all(|force| force > 0.0));
        assert!(telemetry
            .wall_gap_m
            .into_iter()
            .all(|gap| gap.abs() <= CONTACT_TOLERANCE_M));
    }

    #[test]
    fn contact_force_and_gap_obey_complementarity_during_reversals() {
        let mut state = state();
        let mut random = 0x8b8b_8b8b_1234_5678_u64;
        for sample in 0..20_000 {
            random = random
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let noise = ((random >> 11) as f64 / ((1_u64 << 53) as f64) - 0.5) * 0.2e-6;
            let phase = std::f64::consts::TAU * sample as f64 / 37.25;
            let input = PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts(
                    [
                        1.5e-6 * phase.sin() + noise,
                        0.8e-6 * (phase * 1.7).cos() - noise,
                    ],
                    [0.03 * phase.cos(), -0.02 * phase.sin()],
                ),
                groove_tangential_velocity_m_s: if sample % 97 < 48 { 0.7 } else { -0.7 },
                ..PickupMechanicalInput::default()
            };
            let telemetry = state.process(input).unwrap();
            for wall in 0..2 {
                assert!(telemetry.wall_gap_m[wall] >= -CONTACT_TOLERANCE_M);
                assert!(telemetry.wall_normal_force_n[wall] >= 0.0);
                assert!(telemetry.wall_gap_m[wall] * telemetry.wall_normal_force_n[wall] < 1.0e-10);
            }
        }
    }

    #[test]
    fn lift_removes_wall_force_and_cue_support_raises_body() {
        let mut state = state();
        let input = PickupMechanicalInput {
            stylus_lowered: false,
            ..PickupMechanicalInput::default()
        };
        let mut telemetry = state.process(input).unwrap();
        for _ in 0..200_000 {
            telemetry = state.process(input).unwrap();
        }
        assert_eq!(telemetry.wall_contact, [false, false]);
        assert_eq!(telemetry.wall_normal_force_n, [0.0, 0.0]);
        assert!(telemetry.body_displacement_m[1] > 0.0);
        assert_eq!(telemetry.record_reaction_force_tangent_n, 0.0);
    }

    #[test]
    fn moving_spiral_origin_has_the_documented_lateral_sign() {
        let mut state = state();
        state
            .reset([10.0e-6, 0.0], [0.2, 0.0], [4.0e-6, 0.0], [-0.1, 0.0])
            .unwrap();
        let relative_before = state.tip_displacement_m[0] - state.body_displacement_m[0];
        state.shift_lateral_coordinate_origin(-3.0e-6).unwrap();
        assert!((state.tip_displacement_m[0] - 13.0e-6).abs() < 1.0e-20);
        assert!((state.body_displacement_m[0] - 7.0e-6).abs() < 1.0e-20);
        assert_eq!(state.tip_velocity_m_s[0], 0.2);
        assert_eq!(state.body_velocity_m_s[0], -0.1);
        assert_eq!(
            state.tip_displacement_m[0] - state.body_displacement_m[0],
            relative_before
        );
    }

    #[test]
    fn failed_origin_shift_does_not_mutate_pickup_state() {
        let mut state = state();
        state.tip_displacement_m[0] = f64::MAX;
        state.body_displacement_m[0] = f64::MAX;
        state.last_telemetry.tip_displacement_m[0] = f64::MAX;
        state.last_telemetry.body_displacement_m[0] = f64::MAX;
        let before = state;
        assert_eq!(
            state.shift_lateral_coordinate_origin(-f64::MAX),
            Err(PickupMechanicalError::NumericalFailure)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn horizontal_land_support_does_not_constrain_lateral_position() {
        let tonearm = TonearmConfig {
            anti_skate_force_n: 0.0,
            lateral_bearing_static_friction_n: 0.0,
            lateral_bearing_kinetic_friction_n: 0.0,
            lateral_bearing_viscous_damping_n_s_per_m: 0.0,
            ..TonearmConfig::default()
        };
        let mut state =
            PickupMechanicalState::new(StylusContactConfig::default(), tonearm, 192_000.0).unwrap();
        let lateral_m = 40.0e-6;
        let land_height_m = 25.0e-6;
        state
            .reset(
                [lateral_m, land_height_m],
                [0.0; 2],
                [lateral_m, land_height_m],
                [0.0; 2],
            )
            .unwrap();
        let input = PickupMechanicalInput {
            contact_surface: PickupContactSurface::RecordLand,
            land_displacement_m: land_height_m,
            ..PickupMechanicalInput::default()
        };
        let mut telemetry = state.process(input).unwrap();
        for _ in 0..20_000 {
            telemetry = state.process(input).unwrap();
        }
        assert!(telemetry.land_contact);
        assert!(telemetry.land_gap_m.abs() <= CONTACT_TOLERANCE_M);
        assert!(telemetry.land_normal_force_n > 0.0);
        assert_eq!(telemetry.wall_contact, [false; 2]);
        assert_eq!(telemetry.wall_normal_force_n, [0.0; 2]);
        assert!((telemetry.tip_displacement_m[0] - lateral_m).abs() < 1.0e-12);
    }

    #[test]
    fn land_friction_applies_skating_to_the_arm_once() {
        let tonearm = TonearmConfig {
            anti_skate_force_n: 0.0,
            lateral_bearing_static_friction_n: 0.0,
            lateral_bearing_kinetic_friction_n: 0.0,
            lateral_bearing_viscous_damping_n_s_per_m: 0.0,
            ..TonearmConfig::default()
        };
        let mut state =
            PickupMechanicalState::new(StylusContactConfig::default(), tonearm, 192_000.0).unwrap();
        let land_height_m = 25.0e-6;
        state
            .reset(
                [0.0, land_height_m],
                [0.0; 2],
                [0.0, land_height_m],
                [0.0; 2],
            )
            .unwrap();
        let before_momentum = state.contact.moving_mass_kg * state.tip_velocity_m_s[0]
            + tonearm.lateral.effective_mass_kg * state.body_velocity_m_s[0];
        let telemetry = state
            .process(PickupMechanicalInput {
                contact_surface: PickupContactSurface::RecordLand,
                land_displacement_m: land_height_m,
                groove_tangential_velocity_m_s: 0.5,
                ..PickupMechanicalInput::default()
            })
            .unwrap();
        let after_momentum = state.contact.moving_mass_kg * state.tip_velocity_m_s[0]
            + tonearm.lateral.effective_mass_kg * state.body_velocity_m_s[0];
        let measured_impulse = after_momentum - before_momentum;
        let expected_impulse = telemetry.skating_force_n / state.sample_rate_hz;
        assert!(telemetry.land_contact);
        assert!(telemetry.coulomb_friction_force_n < 0.0);
        assert!(telemetry.record_reaction_torque_nm() < 0.0);
        assert!(telemetry.skating_force_n < 0.0);
        assert!((measured_impulse - expected_impulse).abs() < 1.0e-12);
    }

    #[test]
    fn invalid_input_and_snapshot_do_not_mutate_state() {
        let mut state = state();
        let before = state;
        let error = state
            .process(PickupMechanicalInput {
                groove_radius_m: f64::NAN,
                ..PickupMechanicalInput::default()
            })
            .unwrap_err();
        assert_eq!(error, PickupMechanicalError::InvalidInput);
        assert_eq!(state, before);

        let mut snapshot = state.snapshot();
        snapshot.tip_velocity_m_s[0] = f64::INFINITY;
        assert_eq!(
            state.restore(snapshot),
            Err(PickupMechanicalError::InvalidSnapshot)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn snapshot_restore_continues_identically() {
        let mut a = state();
        for step in 0..1_000 {
            let phase = std::f64::consts::TAU * step as f64 / 137.0;
            a.process(PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts(
                    [phase.sin() * 2.0e-6, phase.cos() * 1.0e-6],
                    [phase.cos() * 0.02, -phase.sin() * 0.01],
                ),
                groove_tangential_velocity_m_s: 0.5,
                ..PickupMechanicalInput::default()
            })
            .unwrap();
        }
        let mut b = state();
        b.restore(a.snapshot()).unwrap();
        let input = PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts([1.0e-6, -0.5e-6], [0.03, -0.02]),
            groove_tangential_velocity_m_s: -0.7,
            ..PickupMechanicalInput::default()
        };
        assert_eq!(a.process(input).unwrap(), b.process(input).unwrap());
    }

    #[test]
    fn multi_contact_block_partitions_and_snapshot_replay_are_exact() {
        let mut whole = state();
        let mut expected = Vec::with_capacity(1_024);
        for sample in 0..1_024 {
            expected.push(whole.process(changing_contact_input(sample)).unwrap());
        }

        let mut partitioned = state();
        let mut sample = 0;
        for block_size in [1, 31, 7, 127, 2, 64].into_iter().cycle() {
            if sample == expected.len() {
                break;
            }
            let end = (sample + block_size).min(expected.len());
            while sample < end {
                assert_eq!(
                    partitioned.process(changing_contact_input(sample)).unwrap(),
                    expected[sample]
                );
                sample += 1;
            }
        }
        assert_eq!(partitioned.snapshot(), whole.snapshot());

        let snapshot = partitioned.snapshot();
        let mut replay = state();
        replay.restore(snapshot).unwrap();
        for sample in 1_024..1_280 {
            assert_eq!(
                replay.process(changing_contact_input(sample)).unwrap(),
                partitioned.process(changing_contact_input(sample)).unwrap()
            );
        }
        assert_eq!(replay.snapshot(), partitioned.snapshot());
    }

    fn coupled_midpoint_step(
        record_rate: f64,
        wall_slope: [f64; 2],
        previous_mode: StylusTangentialMode,
    ) -> CoupledDeckPickupStep {
        let mut deck = DeckMechanicalState::new(crate::PhysicalDeckConfig::default()).unwrap();
        deck.reset(record_rate, record_rate, 0.0, 0.0).unwrap();
        let preparation = deck
            .prepare_midpoint_step(1.0 / 192_000.0, crate::DeckMechanicalControl::default())
            .unwrap();
        solve_coupled_deck_pickup_midpoint(
            preparation,
            state(),
            MidpointPickupGeometry {
                input: PickupMechanicalInput {
                    wall_contacts: test_single_wall_contacts([0.0; 2], wall_slope),
                    electromagnetic_force_n: [0.0; 2],
                    ..PickupMechanicalInput::default()
                },
                lateral_origin_shift_bias_m: 0.0,
                lateral_origin_shift_per_record_velocity_m_s: 0.0,
            },
            PickupElectromagneticForceRelation::constant([0.0; 2]).unwrap(),
            previous_mode,
        )
        .unwrap()
    }

    #[test]
    fn midpoint_static_contact_uses_bounded_traction_without_power() {
        let step = coupled_midpoint_step(0.0, [0.04, -0.03], StylusTangentialMode::Separated);
        assert_eq!(step.tangential_mode, StylusTangentialMode::Sticking);
        assert!(step.evaluated_branches > 0);
        assert!(step.evaluated_branches <= MAX_MIDPOINT_CANDIDATE_BRANCHES);
        assert!(step.attempted_linear_solves <= MAX_MIDPOINT_LINEAR_SOLVES);
        assert_eq!(step.pickup_telemetry.tangential_relative_velocity_m_s, 0.0);
        assert_eq!(step.pickup_telemetry.tangential_friction_power_w, 0.0);
        assert_eq!(
            step.deck.telemetry().stylus_torque_nm,
            step.pickup_telemetry.record_reaction_torque_nm()
        );
    }

    #[test]
    fn midpoint_active_mode_solve_allocates_no_memory() {
        let mut deck = DeckMechanicalState::new(crate::PhysicalDeckConfig::default()).unwrap();
        deck.reset(20.0, 20.0, 0.0, 0.0).unwrap();
        let preparation = deck
            .prepare_midpoint_step(1.0 / 192_000.0, crate::DeckMechanicalControl::default())
            .unwrap();
        let geometry = MidpointPickupGeometry {
            input: PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts([0.0; 2], [0.3, -0.2]),
                electromagnetic_force_n: [0.0; 2],
                ..PickupMechanicalInput::default()
            },
            lateral_origin_shift_bias_m: 0.0,
            lateral_origin_shift_per_record_velocity_m_s: 0.0,
        };
        let relation = PickupElectromagneticForceRelation::constant([0.0; 2]).unwrap();
        let pickup = state();

        let result = assert_no_alloc::assert_no_alloc(|| {
            solve_coupled_deck_pickup_midpoint(
                preparation,
                pickup,
                geometry,
                relation,
                StylusTangentialMode::SlidingPositive,
            )
        });
        assert!(result.is_ok());
    }

    #[test]
    fn midpoint_sliding_is_sign_strict_passive_and_reciprocal() {
        for (record_rate, expected_mode) in [
            (20.0, StylusTangentialMode::SlidingPositive),
            (-20.0, StylusTangentialMode::SlidingNegative),
        ] {
            let step = coupled_midpoint_step(record_rate, [0.3, -0.2], expected_mode);
            assert_eq!(step.tangential_mode, expected_mode);
            assert!(step.evaluated_branches <= MAX_MIDPOINT_CANDIDATE_BRANCHES);
            assert!(step.attempted_linear_solves <= MAX_MIDPOINT_LINEAR_SOLVES);
            assert!(step.pickup_telemetry.tangential_friction_power_w < 0.0);
            assert_eq!(
                step.pickup_telemetry
                    .tangential_relative_velocity_m_s
                    .is_sign_positive(),
                record_rate.is_sign_positive()
            );
            assert_eq!(
                step.deck.telemetry().stylus_torque_nm,
                step.pickup_telemetry.record_reaction_torque_nm()
            );
        }
    }

    #[test]
    fn midpoint_zero_velocity_tie_selects_the_same_static_state() {
        let expected = coupled_midpoint_step(0.0, [0.04, -0.03], StylusTangentialMode::Sticking);
        assert_eq!(expected.tangential_mode, StylusTangentialMode::Sticking);
        for previous_mode in [
            StylusTangentialMode::Separated,
            StylusTangentialMode::Sticking,
            StylusTangentialMode::SlidingPositive,
            StylusTangentialMode::SlidingNegative,
        ] {
            let actual = coupled_midpoint_step(0.0, [0.04, -0.03], previous_mode);
            assert_eq!(actual.tangential_mode, StylusTangentialMode::Sticking);
            assert_eq!(actual.deck, expected.deck);
            assert_eq!(actual.pickup, expected.pickup);
            assert_eq!(actual.pickup_telemetry, expected.pickup_telemetry);
        }
    }

    #[test]
    fn joint_linear_solve_scales_mixed_units_and_checks_the_residual() {
        let expected = [1.25, -2.5, 4.0];
        let mut augmented = [[0.0_f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES];
        let coefficients = [
            [1.0e-12, 2.0e-12, -1.0e-12],
            [2.0e3, -3.0e3, 4.0e3],
            [-5.0e12, 2.0e12, 1.0e12],
        ];
        for row in 0..3 {
            augmented[row][..3].copy_from_slice(&coefficients[row]);
            augmented[row][JOINT_RHS_COLUMN] = coefficients[row]
                .iter()
                .zip(expected)
                .map(|(coefficient, value)| coefficient * value)
                .sum();
        }

        let solution = solve_joint_linear_system(&mut augmented, 3).unwrap();
        for index in 0..3 {
            assert!((solution[index] - expected[index]).abs() < 1.0e-12);
        }
    }

    #[test]
    fn joint_linear_solve_rejects_an_inconsistent_singular_system() {
        let mut augmented = [[0.0_f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES];
        augmented[0][0] = 1.0;
        augmented[0][1] = 1.0;
        augmented[0][JOINT_RHS_COLUMN] = 2.0;
        augmented[1][0] = 1.0;
        augmented[1][1] = 1.0 + f64::EPSILON;
        augmented[1][JOINT_RHS_COLUMN] = 3.0;

        assert!(solve_joint_linear_system(&mut augmented, 2).is_none());
    }

    #[test]
    #[ignore = "release-build work and timing characterization"]
    fn midpoint_release_work_benchmark() {
        use std::time::Instant;

        #[derive(Default)]
        struct Samples {
            branches: Vec<u32>,
            solves: Vec<u32>,
            elapsed_ns: Vec<u128>,
        }

        impl Samples {
            fn push(&mut self, step: CoupledDeckPickupStep, elapsed_ns: u128) {
                self.branches.push(step.evaluated_branches);
                self.solves.push(step.attempted_linear_solves);
                self.elapsed_ns.push(elapsed_ns);
            }

            fn report(&self, label: &str) {
                fn percentile<T: Ord + Copy>(values: &[T], numerator: usize) -> T {
                    let mut sorted = values.to_vec();
                    sorted.sort_unstable();
                    sorted[(sorted.len() - 1) * numerator / 100]
                }
                let elapsed_total: u128 = self.elapsed_ns.iter().sum();
                eprintln!(
                    "midpoint-benchmark {label}: samples={} branches[min/p50/p95/p99/max]={}/{}/{}/{}/{} solves[min/p50/p95/p99/max]={}/{}/{}/{}/{} elapsed_ns[min/p50/p95/p99/max/mean]={}/{}/{}/{}/{}/{}",
                    self.branches.len(),
                    *self.branches.iter().min().unwrap(),
                    percentile(&self.branches, 50),
                    percentile(&self.branches, 95),
                    percentile(&self.branches, 99),
                    *self.branches.iter().max().unwrap(),
                    *self.solves.iter().min().unwrap(),
                    percentile(&self.solves, 50),
                    percentile(&self.solves, 95),
                    percentile(&self.solves, 99),
                    *self.solves.iter().max().unwrap(),
                    *self.elapsed_ns.iter().min().unwrap(),
                    percentile(&self.elapsed_ns, 50),
                    percentile(&self.elapsed_ns, 95),
                    percentile(&self.elapsed_ns, 99),
                    *self.elapsed_ns.iter().max().unwrap(),
                    elapsed_total / self.elapsed_ns.len() as u128,
                );
                assert!(self
                    .branches
                    .iter()
                    .all(|count| { *count > 0 && *count <= MAX_MIDPOINT_CANDIDATE_BRANCHES }));
                assert!(self
                    .solves
                    .iter()
                    .all(|count| *count <= MAX_MIDPOINT_LINEAR_SOLVES));
            }
        }

        fn run_sequence(reversal: bool) -> Samples {
            let config = crate::PhysicalDeckConfig::default();
            let mut deck = DeckMechanicalState::new(config).unwrap();
            deck.reset(1.0, 1.0, 0.0, 0.0).unwrap();
            let mut pickup = state();
            let mut previous_mode = StylusTangentialMode::SlidingPositive;
            let mut samples = Samples::default();
            for sample in 0..8_192 {
                let phase = std::f64::consts::TAU * sample as f64 / 197.5;
                let mut control = crate::DeckMechanicalControl::default();
                if reversal {
                    let direction = if sample % 64 < 32 { 20.0 } else { -20.0 };
                    control.hand_contact = true;
                    control.hand_target_angular_velocity_rad_s =
                        direction * config.nominal_angular_velocity_rad_s();
                    control.hand_normal_force_n = 5.0;
                    control.hand_contact_radius_m = 0.12;
                }
                let started = Instant::now();
                let preparation = deck
                    .prepare_midpoint_step(1.0 / 192_000.0, control)
                    .unwrap();
                let step = solve_coupled_deck_pickup_midpoint(
                    preparation,
                    pickup,
                    MidpointPickupGeometry {
                        input: PickupMechanicalInput {
                            wall_contacts: test_single_wall_contacts(
                                [2.0e-6 * phase.sin(), 1.3e-6 * (phase * 1.31).cos()],
                                [0.03 * phase.cos(), -0.02 * (phase * 0.7).sin()],
                            ),
                            electromagnetic_force_n: [0.0; 2],
                            ..PickupMechanicalInput::default()
                        },
                        lateral_origin_shift_bias_m: 0.0,
                        lateral_origin_shift_per_record_velocity_m_s: 0.0,
                    },
                    PickupElectromagneticForceRelation::constant([0.0; 2]).unwrap(),
                    previous_mode,
                )
                .unwrap();
                let elapsed_ns = started.elapsed().as_nanos();
                deck = step.deck;
                pickup = step.pickup;
                previous_mode = step.tangential_mode;
                samples.push(step, elapsed_ns);
            }
            samples
        }

        assert!(!cfg!(debug_assertions), "run this benchmark with --release");
        run_sequence(false).report("normal");
        run_sequence(true).report("reversal");

        let mut worst: Option<(
            CoupledDeckPickupStep,
            f64,
            [f64; 2],
            [f64; 2],
            StylusTangentialMode,
        )> = None;
        for record_rate in [-20.0, -1.0, 0.0, 1.0, 20.0] {
            for wall_slope in [
                [-3.0, -3.0],
                [-3.0, 3.0],
                [-0.3, 0.2],
                [0.0, 0.0],
                [0.3, -0.2],
                [3.0, -3.0],
                [3.0, 3.0],
            ] {
                for wall_displacement_m in [
                    [-25.0e-6, -25.0e-6],
                    [-25.0e-6, 25.0e-6],
                    [0.0, 0.0],
                    [25.0e-6, -25.0e-6],
                    [25.0e-6, 25.0e-6],
                ] {
                    for previous_mode in [
                        StylusTangentialMode::Separated,
                        StylusTangentialMode::Sticking,
                        StylusTangentialMode::SlidingPositive,
                        StylusTangentialMode::SlidingNegative,
                    ] {
                        let mut deck =
                            DeckMechanicalState::new(crate::PhysicalDeckConfig::default()).unwrap();
                        deck.reset(record_rate, record_rate, 0.0, 0.0).unwrap();
                        let preparation = deck
                            .prepare_midpoint_step(
                                1.0 / 192_000.0,
                                crate::DeckMechanicalControl::default(),
                            )
                            .unwrap();
                        let Ok(step) = solve_coupled_deck_pickup_midpoint(
                            preparation,
                            state(),
                            MidpointPickupGeometry {
                                input: PickupMechanicalInput {
                                    wall_contacts: test_single_wall_contacts(
                                        wall_displacement_m,
                                        wall_slope,
                                    ),
                                    electromagnetic_force_n: [0.0; 2],
                                    ..PickupMechanicalInput::default()
                                },
                                lateral_origin_shift_bias_m: 0.0,
                                lateral_origin_shift_per_record_velocity_m_s: 0.0,
                            },
                            PickupElectromagneticForceRelation::constant([0.0; 2]).unwrap(),
                            previous_mode,
                        ) else {
                            continue;
                        };
                        if worst.as_ref().is_none_or(|current| {
                            (step.evaluated_branches, step.attempted_linear_solves)
                                > (
                                    current.0.evaluated_branches,
                                    current.0.attempted_linear_solves,
                                )
                        }) {
                            worst = Some((
                                step,
                                record_rate,
                                wall_slope,
                                wall_displacement_m,
                                previous_mode,
                            ));
                        }
                    }
                }
            }
        }
        let (worst, record_rate, wall_slope, wall_displacement_m, previous_mode) = worst.unwrap();
        eprintln!(
            "midpoint-benchmark constructed-search: record_rate={record_rate:?} wall_slope={wall_slope:?} wall_displacement_m={wall_displacement_m:?} previous_mode={previous_mode:?} branches={} solves={}",
            worst.evaluated_branches,
            worst.attempted_linear_solves,
        );
    }
}
