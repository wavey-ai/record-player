use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::stylus::{StylusTraceContactSet, MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL};
use super::TonearmConfig;
use crate::mechanics::{
    coupled_deck_mode_is_valid, CoupledDeckFrictionMode, DeckMechanicalError, DeckMechanicalState,
    DeckMidpointPreparation, DeckMidpointSolution,
};

const SNAPSHOT_VERSION: u32 = 5;
const MIN_SAMPLE_RATE_HZ: f64 = 44_100.0;
const MAX_SAMPLE_RATE_HZ: f64 = 768_000.0;
const MIN_MOVING_MASS_KG: f64 = 1.0e-9;
const MAX_MOVING_MASS_KG: f64 = 1.0e-2;
const CONTACT_TOLERANCE_M: f64 = 1.0e-11;
const BEARING_VELOCITY_TOLERANCE_M_S: f64 = 1.0e-10;
const TANGENTIAL_VELOCITY_TOLERANCE_M_S: f64 = 1.0e-12;
const ZERO_SLIP_REGULARIZATION_VELOCITY_M_S: f64 = 1.0e-9;
const TANGENTIAL_FORCE_TOLERANCE_N: f64 = 1.0e-10;
const FRICTION_GEOMETRY_PRODUCT_MARGIN: f64 = 1.0e-6;
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy)]
enum WallFrictionDistribution {
    None,
    Sliding { coefficient: f64, direction: f64 },
    FlatSticking { record_force_n: f64 },
}

// A tangential stylus force maps to the lateral body through the tonearm
// skating factor K. Its conjugate stylus velocity is therefore K * v_body.
// For a compatible wall, v_wall = p * (v_record - K * v_body). Sliding uses
// F_record = -sign(gamma_dot) * mu * lambda and F_wall = p * F_record.

fn distribute_wall_contact_forces(
    input: PickupMechanicalInput,
    wall_projected_force_n: [f64; 2],
    friction: WallFrictionDistribution,
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
    let total_projected_force_n = wall_projected_force_n.into_iter().sum::<f64>();
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
            if projected_force_n > 0.0 {
                final_loaded_contact = Some((wall, contact_index));
            }
        }
    }
    match friction {
        WallFrictionDistribution::None => {}
        WallFrictionDistribution::Sliding {
            coefficient,
            direction,
        } => {
            let target_record_force_n = -direction * coefficient * total_projected_force_n;
            let mut assigned_record_force_n = 0.0;
            for (wall, wall_telemetry) in telemetry.iter_mut().enumerate() {
                let count = wall_contact_count(input.wall_contacts[wall]).unwrap_or(1);
                for contact_index in 0..count {
                    let projected_force_n = wall_telemetry.projected_normal_force_n[contact_index];
                    let record_force_n = if final_loaded_contact == Some((wall, contact_index)) {
                        target_record_force_n - assigned_record_force_n
                    } else {
                        -direction * coefficient * projected_force_n
                    };
                    let wall_force_on_tip_n = record_force_n
                        * input.wall_contacts[wall].contacts[contact_index].groove_slope;
                    wall_telemetry.coulomb_friction_force_n[contact_index] = record_force_n;
                    wall_telemetry.coulomb_wall_force_on_tip_n[contact_index] = wall_force_on_tip_n;
                    assigned_record_force_n += record_force_n;
                }
            }
        }
        WallFrictionDistribution::FlatSticking { record_force_n } => {
            let mut assigned_record_force_n = 0.0;
            for (wall, wall_telemetry) in telemetry.iter_mut().enumerate() {
                let count = wall_contact_count(input.wall_contacts[wall]).unwrap_or(1);
                for contact_index in 0..count {
                    let friction_force_n = if final_loaded_contact == Some((wall, contact_index)) {
                        record_force_n - assigned_record_force_n
                    } else if total_projected_force_n > 0.0 {
                        record_force_n * wall_telemetry.projected_normal_force_n[contact_index]
                            / total_projected_force_n
                    } else {
                        0.0
                    };
                    wall_telemetry.coulomb_friction_force_n[contact_index] = friction_force_n;
                    assigned_record_force_n += friction_force_n;
                }
            }
        }
    }
    let coulomb_friction_force_n = telemetry
        .into_iter()
        .flat_map(|wall| wall.coulomb_friction_force_n)
        .sum::<f64>();
    let target_record_reaction_force_tangent_n = telemetry
        .into_iter()
        .flat_map(|wall| wall.modulation_reaction_force_n)
        .sum::<f64>()
        + coulomb_friction_force_n;
    let mut assigned_record_reaction_force_tangent_n = 0.0;
    for (wall, wall_telemetry) in telemetry.iter_mut().enumerate() {
        let count = wall_contact_count(input.wall_contacts[wall]).unwrap_or(1);
        for contact_index in 0..count {
            let reaction_force_n = if final_loaded_contact == Some((wall, contact_index)) {
                target_record_reaction_force_tangent_n - assigned_record_reaction_force_tangent_n
            } else {
                wall_telemetry.modulation_reaction_force_n[contact_index]
                    + wall_telemetry.coulomb_friction_force_n[contact_index]
            };
            wall_telemetry.record_reaction_force_tangent_n[contact_index] = reaction_force_n;
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

fn wall_friction_distribution(
    mode: StylusTangentialMode,
    friction_coefficient: f64,
    record_friction_force_n: f64,
) -> WallFrictionDistribution {
    match mode {
        StylusTangentialMode::SlidingPositive => WallFrictionDistribution::Sliding {
            coefficient: friction_coefficient,
            direction: 1.0,
        },
        StylusTangentialMode::SlidingNegative => WallFrictionDistribution::Sliding {
            coefficient: friction_coefficient,
            direction: -1.0,
        },
        StylusTangentialMode::Sticking => WallFrictionDistribution::FlatSticking {
            record_force_n: record_friction_force_n,
        },
        StylusTangentialMode::Separated => WallFrictionDistribution::None,
    }
}

fn wall_coulomb_force_on_tip_n(telemetry: [PickupWallContactTelemetry; 2]) -> [f64; 2] {
    let wall_force_n = telemetry.map(PickupWallContactTelemetry::total_coulomb_wall_force_on_tip_n);
    [
        (wall_force_n[0] - wall_force_n[1]) * INVERSE_SQRT_2,
        (wall_force_n[0] + wall_force_n[1]) * INVERSE_SQRT_2,
    ]
}

fn wall_coulomb_friction_power_w(
    telemetry: [PickupWallContactTelemetry; 2],
    along_groove_slip_velocity_m_s: f64,
    wall_coordinate_velocity_m_s: [f64; 2],
) -> f64 {
    telemetry
        .into_iter()
        .enumerate()
        .map(|(wall_index, wall)| {
            (0..wall.contact_count())
                .map(|contact_index| {
                    let groove_slope = wall.geometry.contacts[contact_index].groove_slope;
                    wall.coulomb_friction_force_n[contact_index]
                        * (along_groove_slip_velocity_m_s
                            + groove_slope * wall_coordinate_velocity_m_s[wall_index])
                })
                .sum::<f64>()
        })
        .sum()
}

fn wall_coordinate_velocity_m_s(tip_velocity_m_s: [f64; 2]) -> [f64; 2] {
    WALL_NORMALS.map(|normal| dot(normal, tip_velocity_m_s))
}

fn along_groove_slip_velocity_m_s(
    record_velocity_m_s: f64,
    skating_factor: f64,
    body_lateral_velocity_m_s: f64,
) -> f64 {
    record_velocity_m_s - skating_factor * body_lateral_velocity_m_s
}

fn loaded_wall_has_nonzero_slope(
    input: PickupMechanicalInput,
    wall_projected_force_n: [f64; 2],
) -> bool {
    (0..2).any(|wall| {
        wall_projected_force_n[wall] > TANGENTIAL_FORCE_TOLERANCE_N
            && input.wall_contacts[wall].contacts
                [..wall_contact_count(input.wall_contacts[wall]).unwrap_or(1)]
                .iter()
                .any(|contact| contact.groove_slope != 0.0)
    })
}

fn active_wall_has_nonzero_slope(input: PickupMechanicalInput, active_mask: u8) -> bool {
    (0..2).any(|wall| {
        active_mask & (1 << wall) != 0
            && input.wall_contacts[wall].contacts
                [..wall_contact_count(input.wall_contacts[wall]).unwrap_or(1)]
                .iter()
                .any(|contact| contact.groove_slope != 0.0)
    })
}

/// Checks only the local inward direction of each sliding wall force.
///
/// This predicate does not prove that the coupled contact problem is well posed.
pub(crate) fn groove_friction_geometry_is_well_conditioned(
    friction_coefficient: f64,
    maximum_absolute_wall_slope: f64,
) -> bool {
    friction_coefficient.is_finite()
        && friction_coefficient >= 0.0
        && maximum_absolute_wall_slope.is_finite()
        && maximum_absolute_wall_slope >= 0.0
        && friction_coefficient * maximum_absolute_wall_slope
            < 1.0 - FRICTION_GEOMETRY_PRODUCT_MARGIN
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
    /// This force acts on the tip along the inward 45-degree wall coordinate.
    pub coulomb_wall_force_on_tip_n: [f64; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
    /// This force acts on the record along the groove.
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
            coulomb_wall_force_on_tip_n: [0.0; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
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

    pub fn total_coulomb_wall_force_on_tip_n(self) -> f64 {
        self.coulomb_wall_force_on_tip_n.into_iter().sum()
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

    /// Returns the complete cross-plane Coulomb force on the stylus tip.
    pub fn coulomb_wall_force_on_tip_n(self) -> [f64; 2] {
        wall_coulomb_force_on_tip_n(self.wall_longitudinal_contact)
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
        validate_friction_geometry(self.contact, input)?;
        let relation = PickupElectromagneticForceRelation::constant(input.electromagnetic_force_n)?;
        self.process_with_validated_electromagnetic_relation(input, relation)
    }

    /// Advances one bounded internal step without changing the public rate.
    #[cfg(test)]
    pub(crate) fn process_bounded_substep(
        &mut self,
        input: PickupMechanicalInput,
        duration_seconds: f64,
    ) -> Result<PickupMechanicalTelemetry, PickupMechanicalError> {
        let nominal_dt = 1.0 / self.sample_rate_hz;
        if !duration_seconds.is_finite()
            || duration_seconds <= 0.0
            || duration_seconds > nominal_dt * (1.0 + 16.0 * f64::EPSILON)
        {
            return Err(PickupMechanicalError::NumericalFailure);
        }
        let nominal_sample_rate_hz = self.sample_rate_hz;
        let mut next = *self;
        next.sample_rate_hz = 1.0 / duration_seconds;
        let telemetry = next.process(input)?;
        next.sample_rate_hz = nominal_sample_rate_hz;
        *self = next;
        Ok(telemetry)
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
        validate_friction_geometry(self.contact, input)?;
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
        let skating_factor = if input.stylus_lowered {
            self.tonearm
                .geometry
                .equivalent_radial_force_n(input.groove_radius_m, 1.0)?
        } else {
            0.0
        };
        let along_groove_slip_velocity_m_s = along_groove_slip_velocity_m_s(
            input.groove_tangential_velocity_m_s,
            skating_factor,
            step.body_velocity_m_s[0],
        );
        let wall_projected_force_n = step.wall_projected_force_n;
        let land_normal_force_n = step.land_normal_force_n;
        let (friction_normal_force_n, friction_coefficient) = match input.contact_surface {
            PickupContactSurface::GrooveWalls => (
                wall_projected_force_n[0] + wall_projected_force_n[1],
                self.contact.groove_friction_coefficient,
            ),
            PickupContactSurface::RecordLand => (
                land_normal_force_n,
                self.contact.record_surface_friction_coefficient,
            ),
            PickupContactSurface::None => (0.0, 0.0),
        };
        let tangential_mode = step.tangential_mode;
        if tangential_mode == StylusTangentialMode::Sticking
            && input.contact_surface == PickupContactSurface::GrooveWalls
            && friction_coefficient > 0.0
            && loaded_wall_has_nonzero_slope(input, wall_projected_force_n)
        {
            return Err(PickupMechanicalError::UnsupportedGrooveWallSticking);
        }
        let coulomb_friction_force_n = match tangential_mode {
            StylusTangentialMode::SlidingPositive => {
                -friction_coefficient * friction_normal_force_n
            }
            StylusTangentialMode::SlidingNegative => friction_coefficient * friction_normal_force_n,
            StylusTangentialMode::Sticking => step.sticking_friction_force_n,
            StylusTangentialMode::Separated => 0.0,
        };
        let wall_contact_telemetry = distribute_wall_contact_forces(
            input,
            wall_projected_force_n,
            wall_friction_distribution(
                tangential_mode,
                friction_coefficient,
                coulomb_friction_force_n,
            ),
        );
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
        self.commit_pickup_step(
            input,
            relation,
            step,
            PickupTangentialForces {
                coulomb_friction_force_n,
                modulation_reaction_force_n,
                record_reaction_force_tangent_n,
                relative_velocity_m_s: along_groove_slip_velocity_m_s,
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
        if tangential.mode == StylusTangentialMode::Sticking
            && input.contact_surface == PickupContactSurface::GrooveWalls
            && self.contact.groove_friction_coefficient > 0.0
            && loaded_wall_has_nonzero_slope(input, wall_projected_force_n)
        {
            return Err(PickupMechanicalError::UnsupportedGrooveWallSticking);
        }
        let wall_longitudinal_contact = distribute_wall_contact_forces(
            input,
            wall_projected_force_n,
            if input.contact_surface == PickupContactSurface::GrooveWalls {
                wall_friction_distribution(
                    tangential.mode,
                    self.contact.groove_friction_coefficient,
                    tangential.coulomb_friction_force_n,
                )
            } else {
                WallFrictionDistribution::None
            },
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
        let wall_coordinate_force_on_tip_n = wall_longitudinal_contact.map(|wall| {
            wall.total_projected_normal_force_n() + wall.total_coulomb_wall_force_on_tip_n()
        });
        let groove_lateral_force_on_tip_n = (wall_coordinate_force_on_tip_n[0]
            - wall_coordinate_force_on_tip_n[1])
            * INVERSE_SQRT_2;
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
            if input.contact_surface == PickupContactSurface::GrooveWalls {
                wall_coulomb_friction_power_w(
                    wall_longitudinal_contact,
                    tangential.relative_velocity_m_s,
                    wall_coordinate_velocity_m_s(tip_velocity),
                )
            } else {
                tangential.coulomb_friction_force_n * tangential.relative_velocity_m_s
            };
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

/// Calculates the interior spiral-origin response to record velocity.
pub(crate) fn interior_spiral_origin_shift_per_record_velocity_m_s(
    groove_pitch_m_per_revolution: f64,
    dt: f64,
) -> f64 {
    -groove_pitch_m_per_revolution * 0.5 * dt / std::f64::consts::TAU
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
pub(crate) const COUPLED_FIXED_KKT_CAPACITY: usize = JOINT_MAX_VARIABLES;
pub(crate) const COUPLED_FIXED_MODE_OPERATOR_VERSION: u32 = 1;
pub(crate) const COUPLED_FIXED_MODE_FAMILY_SET_VERSION: u32 = 1;
pub(crate) const COUPLED_FIXED_SOLVE_ONLY_SUBJECT_SET_VERSION: u32 = 1;
pub(crate) const COUPLED_FIXED_MECHANICAL_CLASS_COUNT: usize = 24;
pub(crate) const COUPLED_FIXED_CONTACT_FAMILY_COUNT: usize = 288;
pub(crate) const COUPLED_FIXED_SOLVE_ONLY_SUBJECT_COUNT: usize = 48;
// 3 bearing * 3 slipmat * 3 hand * 3 pickup * 4 masks * 4 tangent modes.
pub(crate) const MAX_MIDPOINT_CANDIDATE_BRANCHES: u32 = 1_296;
pub(crate) const MAX_MIDPOINT_LINEAR_SOLVES: u32 = 1_296;

/// Identifies a fixed friction mobility without a kinetic direction.
///
/// Positive and negative sliding use the same production left-hand side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FixedModeFrictionMobility {
    Sticking,
    Sliding,
}

/// Identifies a fixed hand-contact mobility without a kinetic direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FixedModeHandMobility {
    Separated,
    Sticking,
    Sliding,
}

/// Identifies one of the 24 mechanical left-hand-side classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CoupledFixedMechanicalMode {
    pub(crate) deck_bearing: FixedModeFrictionMobility,
    pub(crate) slipmat: FixedModeFrictionMobility,
    pub(crate) hand: FixedModeHandMobility,
    pub(crate) pickup_bearing: FixedModeFrictionMobility,
}

/// Describes whether the stylus sticking row adds a mechanical constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FixedStylusConstraintRelation {
    AddsRank,
    DependentRuntimeRhs,
}

impl CoupledFixedMechanicalMode {
    fn deck_bearing_mode(self) -> CoupledDeckFrictionMode {
        fixed_deck_mode(self.deck_bearing)
    }

    fn slipmat_mode(self) -> CoupledDeckFrictionMode {
        fixed_deck_mode(self.slipmat)
    }

    fn hand_mode(self) -> CoupledDeckFrictionMode {
        match self.hand {
            FixedModeHandMobility::Separated => CoupledDeckFrictionMode::Separated,
            FixedModeHandMobility::Sticking => CoupledDeckFrictionMode::Sticking,
            FixedModeHandMobility::Sliding => CoupledDeckFrictionMode::SlidingPositive,
        }
    }

    fn pickup_bearing_mode(self) -> BearingMode {
        match self.pickup_bearing {
            FixedModeFrictionMobility::Sticking => BearingMode::Stick,
            FixedModeFrictionMobility::Sliding => BearingMode::Positive,
        }
    }
}

/// Classifies the production stylus sticking row for one fixed mechanical mode.
pub(crate) fn fixed_stylus_constraint_relation(
    mechanical: CoupledFixedMechanicalMode,
    stylus_body_velocity_coefficient: f64,
) -> Option<FixedStylusConstraintRelation> {
    let selected = select_independent_deck_constraints(
        mechanical.deck_bearing_mode(),
        mechanical.slipmat_mode(),
        mechanical.hand_mode(),
        StylusTangentialMode::Sticking,
        mechanical.pickup_bearing_mode(),
        0.0,
        0.0,
        stylus_body_velocity_coefficient,
    )?;
    Some(if selected[3] {
        FixedStylusConstraintRelation::AddsRank
    } else {
        FixedStylusConstraintRelation::DependentRuntimeRhs
    })
}

const fn fixed_deck_mode(mobility: FixedModeFrictionMobility) -> CoupledDeckFrictionMode {
    match mobility {
        FixedModeFrictionMobility::Sticking => CoupledDeckFrictionMode::Sticking,
        FixedModeFrictionMobility::Sliding => CoupledDeckFrictionMode::SlidingPositive,
    }
}

const FIXED_FRICTION_MOBILITIES: [FixedModeFrictionMobility; 2] = [
    FixedModeFrictionMobility::Sticking,
    FixedModeFrictionMobility::Sliding,
];
const FIXED_HAND_MOBILITIES: [FixedModeHandMobility; 3] = [
    FixedModeHandMobility::Separated,
    FixedModeHandMobility::Sticking,
    FixedModeHandMobility::Sliding,
];
const FIXED_STYLUS_MODES: [StylusTangentialMode; 4] = [
    StylusTangentialMode::Separated,
    StylusTangentialMode::Sticking,
    StylusTangentialMode::SlidingPositive,
    StylusTangentialMode::SlidingNegative,
];

/// Returns all mechanical classes in stable certificate order.
pub(crate) fn coupled_fixed_mechanical_modes(
) -> impl Iterator<Item = CoupledFixedMechanicalMode> + Clone {
    FIXED_FRICTION_MOBILITIES
        .into_iter()
        .flat_map(|deck_bearing| {
            FIXED_FRICTION_MOBILITIES
                .into_iter()
                .flat_map(move |slipmat| {
                    FIXED_HAND_MOBILITIES.into_iter().flat_map(move |hand| {
                        FIXED_FRICTION_MOBILITIES
                            .into_iter()
                            .map(move |pickup_bearing| CoupledFixedMechanicalMode {
                                deck_bearing,
                                slipmat,
                                hand,
                                pickup_bearing,
                            })
                    })
                })
        })
}

/// Identifies the pickup support state for a solve-only mechanical system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CoupledFixedPickupSupport {
    /// The stylus is lowered, but no normal-contact constraint is active.
    LoweredNoContact,
    /// The cue support acts on the lifted vertical tonearm body.
    CueSupported,
}

const FIXED_PICKUP_SUPPORTS: [CoupledFixedPickupSupport; 2] = [
    CoupledFixedPickupSupport::LoweredNoContact,
    CoupledFixedPickupSupport::CueSupported,
];

/// Identifies one of the 48 source-independent solve-only subjects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CoupledFixedSolveOnlySubject {
    subject_set_version: u32,
    pub(crate) mechanical: CoupledFixedMechanicalMode,
    pub(crate) support: CoupledFixedPickupSupport,
}

impl CoupledFixedSolveOnlySubject {
    pub(crate) const fn subject_set_version(self) -> u32 {
        self.subject_set_version
    }
}

/// Returns 24 lowered subjects followed by 24 cue-supported subjects.
pub(crate) fn coupled_fixed_solve_only_subjects(
) -> impl Iterator<Item = CoupledFixedSolveOnlySubject> + Clone {
    FIXED_PICKUP_SUPPORTS.into_iter().flat_map(|support| {
        coupled_fixed_mechanical_modes().map(move |mechanical| CoupledFixedSolveOnlySubject {
            subject_set_version: COUPLED_FIXED_SOLVE_ONLY_SUBJECT_SET_VERSION,
            mechanical,
            support,
        })
    })
}

/// Identifies a contacting surface in the fixed-mode catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CoupledFixedContactSurface {
    GrooveWalls,
    RecordLand,
}

/// Identifies the production spiral-origin law for a contacting surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CoupledFixedOriginLaw {
    InteriorSpiral,
    HeldProgramBoundary,
    SurfaceIndependent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CoupledFixedContactGeometryLabel {
    surface: CoupledFixedContactSurface,
    origin_law: CoupledFixedOriginLaw,
}

const FIXED_CONTACT_GEOMETRY_LABELS: [CoupledFixedContactGeometryLabel; 3] = [
    CoupledFixedContactGeometryLabel {
        surface: CoupledFixedContactSurface::GrooveWalls,
        origin_law: CoupledFixedOriginLaw::InteriorSpiral,
    },
    CoupledFixedContactGeometryLabel {
        surface: CoupledFixedContactSurface::GrooveWalls,
        origin_law: CoupledFixedOriginLaw::HeldProgramBoundary,
    },
    CoupledFixedContactGeometryLabel {
        surface: CoupledFixedContactSurface::RecordLand,
        origin_law: CoupledFixedOriginLaw::SurfaceIndependent,
    },
];

/// Identifies one versioned fixed-mode contacting family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CoupledFixedContactFamily {
    family_set_version: u32,
    pub(crate) mechanical: CoupledFixedMechanicalMode,
    pub(crate) surface: CoupledFixedContactSurface,
    pub(crate) origin_law: CoupledFixedOriginLaw,
    pub(crate) stylus: StylusTangentialMode,
}

impl CoupledFixedContactFamily {
    pub(crate) const fn family_set_version(self) -> u32 {
        self.family_set_version
    }
}

/// Returns all 288 contacting labels in stable certificate order.
pub(crate) fn coupled_fixed_contact_families(
) -> impl Iterator<Item = CoupledFixedContactFamily> + Clone {
    coupled_fixed_mechanical_modes().flat_map(|mechanical| {
        FIXED_CONTACT_GEOMETRY_LABELS
            .into_iter()
            .flat_map(move |geometry| {
                FIXED_STYLUS_MODES
                    .into_iter()
                    .map(move |stylus| CoupledFixedContactFamily {
                        family_set_version: COUPLED_FIXED_MODE_FAMILY_SET_VERSION,
                        mechanical,
                        surface: geometry.surface,
                        origin_law: geometry.origin_law,
                        stylus,
                    })
            })
    })
}

/// Supplies the source-dependent coordinates for one fixed-mode response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CoupledFixedContactPoint {
    pub(crate) groove_radius_m: f64,
    pub(crate) wall_slopes: [f64; 2],
}

/// Describes whether a labeled operator can represent a loaded runtime mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoupledFixedModeReachability {
    RuntimeConditional,
    /// Production admits this mode only inside its normal-force tolerance band.
    NormalForceToleranceBandOnly,
    /// A nonzero slope rejects sticking only when that wall is active.
    ActiveWallDependent {
        nonzero_active_wall: CoupledFixedModeInfeasibility,
        wall_is_nonzero: [bool; 2],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoupledFixedModeInfeasibility {
    SlopedGrooveStickingWithFriction,
    SlopedGrooveStickingAtHeldBoundary,
}

/// Reports the equality rank after production removes redundant constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CoupledFixedEqualityDiagnostics {
    pub(crate) requested_count: usize,
    pub(crate) rank: usize,
    pub(crate) dependent_count: usize,
    pub(crate) runtime_rhs_compatibility_required: bool,
}

/// Reports scaled-pivot and backward-error checks from the production solver.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CoupledFixedSolveDiagnostics {
    pub(crate) system_size: usize,
    pub(crate) minimum_scaled_pivot: f64,
    pub(crate) maximum_scaled_pivot: f64,
    pub(crate) scaled_pivot_ratio: f64,
    pub(crate) maximum_backward_error: f64,
}

/// Stores the compact production KKT left-hand side for one fixed mode.
///
/// The first six rows use dynamic equation order. The first six columns use
/// dynamic velocity order. Remaining rows and columns use equality-basis order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CoupledFixedModeKktLhs {
    pub(crate) system_size: usize,
    pub(crate) equality_count: usize,
    pub(crate) coefficients: [[f64; JOINT_MAX_VARIABLES]; JOINT_MAX_VARIABLES],
    pub(crate) equality_basis: [JointDynamicVelocityRow; 5],
}

impl CoupledFixedModeKktLhs {
    pub(crate) const fn dynamic_coefficient(
        self,
        equation: JointDynamicEquation,
        velocity: JointDynamicVelocity,
    ) -> f64 {
        self.coefficients[equation as usize][velocity as usize]
    }
}

/// Maps dynamic equation RHS values to dynamic velocities for one fixed mode.
///
/// The outer index uses velocity order. The inner index uses equation-row order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CoupledFixedDynamicMobility {
    pub(crate) velocity_by_equation_rhs: [[f64; JOINT_DYNAMIC_VARIABLES]; JOINT_DYNAMIC_VARIABLES],
}

/// Reports every profile-derived scalar used by the point-valued assembly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CoupledFixedDerivedCoefficients {
    pub(crate) dt: f64,
    pub(crate) groove_pitch_m_per_revolution: f64,
    pub(crate) friction_coefficient: f64,
    pub(crate) skating_factor: f64,
    pub(crate) stylus_body_velocity_coefficient: f64,
    pub(crate) lateral_origin_shift_per_record_velocity_m_s: f64,
    pub(crate) platter_inertial_coefficient: f64,
    pub(crate) record_inertial_coefficient: f64,
    pub(crate) stylus_inertial_coefficient: f64,
    pub(crate) body_inertial_coefficients: [f64; 2],
    pub(crate) suspension_stiffness_n_per_m: [f64; 2],
    pub(crate) suspension_viscous_damping_n_s_per_m: [f64; 2],
    pub(crate) suspension_coupling_n_s_per_m: [f64; 2],
    pub(crate) deck_bearing_viscous_coefficient: f64,
    pub(crate) slipmat_viscous_coefficient: f64,
    pub(crate) hand_viscous_coefficient: f64,
    pub(crate) pickup_bearing_viscous_coefficient: f64,
    pub(crate) reciprocal_cartridge_damping_n_s_per_m: [[f64; 2]; 2],
}

impl CoupledFixedDynamicMobility {
    pub(crate) const fn coefficient(
        self,
        velocity: JointDynamicVelocity,
        equation: JointDynamicEquation,
    ) -> f64 {
        self.velocity_by_equation_rhs[velocity as usize][equation as usize]
    }

    pub(crate) fn response_for_equation_rhs(
        self,
        rhs: JointDynamicEquationRhs,
    ) -> [f64; JOINT_DYNAMIC_VARIABLES] {
        let rhs = rhs.coefficients_in_equation_row_order();
        self.velocity_by_equation_rhs.map(|row| {
            row.into_iter()
                .zip(rhs)
                .fold(0.0, |sum, (coefficient, value)| {
                    coefficient.mul_add(value, sum)
                })
        })
    }
}

/// Contains a typed one-wall or two-wall normal response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CoupledFixedNormalResponse {
    RecordLand { w: f64 },
    GrooveWalls { w: [[f64; 2]; 2] },
}

/// Contains the fixed-mode response and its production assembly diagnostics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CoupledFixedModeNormalResponse {
    pub(crate) operator_version: u32,
    pub(crate) family: CoupledFixedContactFamily,
    pub(crate) reachability: CoupledFixedModeReachability,
    pub(crate) equality: CoupledFixedEqualityDiagnostics,
    pub(crate) solve: CoupledFixedSolveDiagnostics,
    pub(crate) derived: CoupledFixedDerivedCoefficients,
    pub(crate) kkt_lhs: CoupledFixedModeKktLhs,
    pub(crate) dynamic_mobility: CoupledFixedDynamicMobility,
    pub(crate) contact_operator: CoupledContactHgOperator,
    pub(crate) normal_response: CoupledFixedNormalResponse,
}

/// Contains one source-independent lowered or cue-supported solve response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CoupledFixedSolveOnlyResponse {
    pub(crate) operator_version: u32,
    pub(crate) subject: CoupledFixedSolveOnlySubject,
    pub(crate) equality: CoupledFixedEqualityDiagnostics,
    pub(crate) solve: CoupledFixedSolveDiagnostics,
    pub(crate) kkt_lhs: CoupledFixedModeKktLhs,
    pub(crate) kkt_inverse: CoupledFixedKktInverse,
    pub(crate) dynamic_mobility: CoupledFixedDynamicMobility,
}

#[derive(Debug, Error)]
pub(crate) enum CoupledFixedModeResponseError {
    #[error(transparent)]
    InvalidConfig(#[from] super::PhysicalProfileError),
    #[error("the fixed-mode point is outside the validated profile domain")]
    InvalidContactPoint,
    #[error("the fixed-mode family label is invalid")]
    InvalidFamily,
    #[error("the fixed-mode solve-only subject label is invalid")]
    InvalidSolveOnlySubject,
    #[error(transparent)]
    Cartridge(#[from] super::MovingMagnetCartridgeError),
    #[error("the fixed-mode left-hand side is singular or fails its residual check")]
    SingularOrIllConditioned,
}

/// Stores six dynamic equation RHS values in equation-row order.
///
/// The order is platter, record, tip-x, body-x, tip-z, body-z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub(crate) enum JointDynamicEquation {
    Platter = 0,
    Record = 1,
    TipX = 2,
    BodyX = 3,
    TipZ = 4,
    BodyZ = 5,
}

pub(crate) const JOINT_DYNAMIC_EQUATIONS: [JointDynamicEquation; JOINT_DYNAMIC_VARIABLES] = [
    JointDynamicEquation::Platter,
    JointDynamicEquation::Record,
    JointDynamicEquation::TipX,
    JointDynamicEquation::BodyX,
    JointDynamicEquation::TipZ,
    JointDynamicEquation::BodyZ,
];

/// Identifies one dynamic velocity column.
///
/// The order is platter, record, tip-x, tip-z, body-x, body-z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub(crate) enum JointDynamicVelocity {
    Platter = 0,
    Record = 1,
    TipX = 2,
    TipZ = 3,
    BodyX = 4,
    BodyZ = 5,
}

pub(crate) const JOINT_DYNAMIC_VELOCITIES: [JointDynamicVelocity; JOINT_DYNAMIC_VARIABLES] = [
    JointDynamicVelocity::Platter,
    JointDynamicVelocity::Record,
    JointDynamicVelocity::TipX,
    JointDynamicVelocity::TipZ,
    JointDynamicVelocity::BodyX,
    JointDynamicVelocity::BodyZ,
];

/// Identifies one active KKT solution coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum JointKktSolutionCoordinate {
    DynamicVelocity(JointDynamicVelocity),
    EqualityMultiplier(usize),
}

impl JointKktSolutionCoordinate {
    pub(crate) fn from_active_index(index: usize, equality_count: usize) -> Option<Self> {
        if index < JOINT_DYNAMIC_VARIABLES {
            Some(Self::DynamicVelocity(JOINT_DYNAMIC_VELOCITIES[index]))
        } else if index < JOINT_DYNAMIC_VARIABLES + equality_count {
            Some(Self::EqualityMultiplier(index - JOINT_DYNAMIC_VARIABLES))
        } else {
            None
        }
    }

    pub(crate) fn active_index(self, equality_count: usize) -> Option<usize> {
        match self {
            Self::DynamicVelocity(velocity) => Some(velocity as usize),
            Self::EqualityMultiplier(index) if index < equality_count => {
                Some(JOINT_DYNAMIC_VARIABLES + index)
            }
            Self::EqualityMultiplier(_) => None,
        }
    }
}

/// Identifies one active KKT right-hand-side coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum JointKktRhsCoordinate {
    DynamicEquation(JointDynamicEquation),
    EqualityConstraint(usize),
}

impl JointKktRhsCoordinate {
    pub(crate) fn from_active_index(index: usize, equality_count: usize) -> Option<Self> {
        if index < JOINT_DYNAMIC_VARIABLES {
            Some(Self::DynamicEquation(JOINT_DYNAMIC_EQUATIONS[index]))
        } else if index < JOINT_DYNAMIC_VARIABLES + equality_count {
            Some(Self::EqualityConstraint(index - JOINT_DYNAMIC_VARIABLES))
        } else {
            None
        }
    }

    pub(crate) fn active_index(self, equality_count: usize) -> Option<usize> {
        match self {
            Self::DynamicEquation(equation) => Some(equation as usize),
            Self::EqualityConstraint(index) if index < equality_count => {
                Some(JOINT_DYNAMIC_VARIABLES + index)
            }
            Self::EqualityConstraint(_) => None,
        }
    }
}

/// Stores the complete inverse of one active fixed KKT system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CoupledFixedKktInverse {
    pub(crate) system_size: usize,
    pub(crate) equality_count: usize,
    pub(crate) solution_by_rhs: [[f64; JOINT_MAX_VARIABLES]; JOINT_MAX_VARIABLES],
}

impl CoupledFixedKktInverse {
    /// Gets one coefficient through typed active coordinates.
    pub(crate) fn coefficient(
        &self,
        solution: JointKktSolutionCoordinate,
        rhs: JointKktRhsCoordinate,
    ) -> Option<f64> {
        let solution_index = solution.active_index(self.equality_count)?;
        let rhs_index = rhs.active_index(self.equality_count)?;
        (solution_index < self.system_size && rhs_index < self.system_size)
            .then_some(self.solution_by_rhs[solution_index][rhs_index])
    }

    pub(crate) fn has_valid_inactive_storage(&self) -> bool {
        self.system_size >= JOINT_DYNAMIC_VARIABLES
            && self.system_size <= JOINT_MAX_VARIABLES
            && self.equality_count == self.system_size - JOINT_DYNAMIC_VARIABLES
            && self
                .solution_by_rhs
                .iter()
                .enumerate()
                .all(|(row, values)| {
                    values.iter().enumerate().all(|(column, value)| {
                        (row < self.system_size && column < self.system_size)
                            || value.to_bits() == 0.0_f64.to_bits()
                    })
                })
    }

    pub(crate) fn dynamic_mobility(&self) -> CoupledFixedDynamicMobility {
        let mut mobility = CoupledFixedDynamicMobility {
            velocity_by_equation_rhs: [[0.0; JOINT_DYNAMIC_VARIABLES]; JOINT_DYNAMIC_VARIABLES],
        };
        for velocity in 0..JOINT_DYNAMIC_VARIABLES {
            mobility.velocity_by_equation_rhs[velocity]
                .copy_from_slice(&self.solution_by_rhs[velocity][..JOINT_DYNAMIC_VARIABLES]);
        }
        mobility
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct JointDynamicEquationRhs {
    platter: f64,
    record: f64,
    tip_x: f64,
    body_x: f64,
    tip_z: f64,
    body_z: f64,
}

impl JointDynamicEquationRhs {
    pub(crate) const fn coefficient(self, equation: JointDynamicEquation) -> f64 {
        match equation {
            JointDynamicEquation::Platter => self.platter,
            JointDynamicEquation::Record => self.record,
            JointDynamicEquation::TipX => self.tip_x,
            JointDynamicEquation::BodyX => self.body_x,
            JointDynamicEquation::TipZ => self.tip_z,
            JointDynamicEquation::BodyZ => self.body_z,
        }
    }

    pub(crate) const fn coefficients_in_equation_row_order(self) -> [f64; JOINT_DYNAMIC_VARIABLES] {
        [
            self.platter,
            self.record,
            self.tip_x,
            self.body_x,
            self.tip_z,
            self.body_z,
        ]
    }

    fn write_contact_column(
        self,
        augmented: &mut [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
        column: usize,
    ) {
        let [_, record, tip_x, body_x, tip_z, _] = self.coefficients_in_equation_row_order();
        // Keep the former assignment and accumulation operations. They preserve
        // the established signed-zero patterns for inactive force components.
        augmented[1][column] -= record;
        augmented[2][column] = -tip_x;
        augmented[3][column] -= body_x;
        augmented[4][column] = -tip_z;
    }

    #[cfg(test)]
    fn write_rhs(self, augmented: &mut [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES]) {
        for (row, coefficient) in self
            .coefficients_in_equation_row_order()
            .into_iter()
            .enumerate()
        {
            augmented[row][JOINT_RHS_COLUMN] = coefficient;
        }
    }
}

/// Stores six gap coefficients in dynamic velocity-column order.
///
/// The order is platter, record, tip-x, tip-z, body-x, body-z.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct JointDynamicVelocityRow {
    platter: f64,
    record: f64,
    tip_x: f64,
    tip_z: f64,
    body_x: f64,
    body_z: f64,
}

impl JointDynamicVelocityRow {
    pub(crate) const fn coefficient(self, velocity: JointDynamicVelocity) -> f64 {
        match velocity {
            JointDynamicVelocity::Platter => self.platter,
            JointDynamicVelocity::Record => self.record,
            JointDynamicVelocity::TipX => self.tip_x,
            JointDynamicVelocity::TipZ => self.tip_z,
            JointDynamicVelocity::BodyX => self.body_x,
            JointDynamicVelocity::BodyZ => self.body_z,
        }
    }

    pub(crate) const fn coefficients_in_velocity_column_order(
        self,
    ) -> [f64; JOINT_DYNAMIC_VARIABLES] {
        [
            self.platter,
            self.record,
            self.tip_x,
            self.tip_z,
            self.body_x,
            self.body_z,
        ]
    }

    fn write_constraint_row(self, row: &mut [f64; JOINT_MAX_VARIABLES + 1]) {
        let [_, record, tip_x, tip_z, body_x, _] = self.coefficients_in_velocity_column_order();
        // Keep the former assignment and accumulation operations. They preserve
        // the established signed-zero patterns for inactive velocity components.
        row[1] = record;
        row[2] = tip_x;
        row[3] = tip_z;
        row[4] += body_x;
    }

    fn response_for_velocity(self, velocity: [f64; JOINT_DYNAMIC_VARIABLES]) -> f64 {
        self.coefficients_in_velocity_column_order()
            .into_iter()
            .zip(velocity)
            .fold(0.0, |sum, (coefficient, value)| {
                coefficient.mul_add(value, sum)
            })
    }
}

/// Defines the force and gap operators for one fixed midpoint contact mode.
///
/// The normal-contact response is `W = H * M * G`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CoupledContactHgOperator {
    constraint_count: usize,
    /// Columns of `G` map projected normal force to the dynamic equation RHS.
    normal_force_rhs: [JointDynamicEquationRhs; 2],
    /// Rows of `H` map dynamic velocity to endpoint gap velocity.
    normal_gap_velocity: [JointDynamicVelocityRow; 2],
}

impl CoupledContactHgOperator {
    pub(crate) const fn constraint_count(self) -> usize {
        self.constraint_count
    }

    pub(crate) const fn normal_force_rhs(self, constraint: usize) -> JointDynamicEquationRhs {
        self.normal_force_rhs[constraint]
    }

    pub(crate) const fn normal_gap_velocity(self, constraint: usize) -> JointDynamicVelocityRow {
        self.normal_gap_velocity[constraint]
    }
}

/// Builds the exact contact operator that the midpoint branch solve uses.
pub(crate) fn coupled_contact_hg_operator(
    geometry: MidpointPickupGeometry,
    dt: f64,
    friction_coefficient: f64,
    stylus_mode: StylusTangentialMode,
    skating_factor: f64,
) -> CoupledContactHgOperator {
    let input = geometry.input;
    let constraint_count = active_constraint_count(input);
    let effective_slope = wall_effective_slope(input);
    let sliding_direction = match stylus_mode {
        StylusTangentialMode::SlidingPositive => 1.0,
        StylusTangentialMode::SlidingNegative => -1.0,
        StylusTangentialMode::Separated | StylusTangentialMode::Sticking => 0.0,
    };
    let mut normal_force_rhs = [JointDynamicEquationRhs::default(); 2];
    let mut normal_gap_velocity = [JointDynamicVelocityRow::default(); 2];
    for constraint in 0..constraint_count {
        let normal = constraint_normal(input.contact_surface, constraint);
        let tangential_force_per_projected_normal = match input.contact_surface {
            PickupContactSurface::GrooveWalls => {
                effective_slope[constraint] + sliding_direction * friction_coefficient
            }
            PickupContactSurface::RecordLand => sliding_direction * friction_coefficient,
            PickupContactSurface::None => 0.0,
        };
        let wall_force_scale = if input.contact_surface == PickupContactSurface::GrooveWalls {
            1.0 - sliding_direction * friction_coefficient * effective_slope[constraint]
        } else {
            1.0
        };
        let (sliding_record_rhs, sliding_body_x_rhs) =
            if stylus_mode != StylusTangentialMode::Sticking {
                (
                    -input.groove_radius_m * tangential_force_per_projected_normal,
                    skating_factor * tangential_force_per_projected_normal,
                )
            } else {
                (0.0, 0.0)
            };
        normal_force_rhs[constraint] = JointDynamicEquationRhs {
            platter: 0.0,
            record: sliding_record_rhs,
            tip_x: wall_force_scale * normal[0],
            body_x: sliding_body_x_rhs,
            tip_z: wall_force_scale * normal[1],
            body_z: 0.0,
        };

        let mut record_gap_coefficient =
            -normal[0] * geometry.lateral_origin_shift_per_record_velocity_m_s / dt;
        let mut body_x_gap_coefficient = 0.0;
        if input.contact_surface == PickupContactSurface::GrooveWalls {
            record_gap_coefficient -= 0.5 * effective_slope[constraint] * input.groove_radius_m;
            body_x_gap_coefficient = effective_slope[constraint] * skating_factor;
        }
        normal_gap_velocity[constraint] = JointDynamicVelocityRow {
            platter: 0.0,
            record: record_gap_coefficient,
            tip_x: normal[0],
            tip_z: normal[1],
            body_x: body_x_gap_coefficient,
            body_z: 0.0,
        };
    }
    CoupledContactHgOperator {
        constraint_count,
        normal_force_rhs,
        normal_gap_velocity,
    }
}

/// Builds the production point-valued KKT mobility and normal response.
///
/// The configuration supplies every physical coefficient. The contact point
/// supplies only the source-dependent radius and wall slopes.
pub(crate) fn coupled_fixed_mode_normal_response(
    config: super::PhysicalPlaybackConfig,
    family: CoupledFixedContactFamily,
    point: CoupledFixedContactPoint,
) -> Result<CoupledFixedModeNormalResponse, CoupledFixedModeResponseError> {
    let config = config.validate()?;
    validate_fixed_family(family)?;
    if !point.groove_radius_m.is_finite()
        || !(config.groove.inner_program_radius_m..=config.groove.outer_program_radius_m)
            .contains(&point.groove_radius_m)
        || point
            .wall_slopes
            .into_iter()
            .any(|slope| !slope.is_finite())
    {
        return Err(CoupledFixedModeResponseError::InvalidContactPoint);
    }

    let dt = 1.0 / config.solver.internal_sample_rate_hz;
    let surface = match family.surface {
        CoupledFixedContactSurface::GrooveWalls => PickupContactSurface::GrooveWalls,
        CoupledFixedContactSurface::RecordLand => PickupContactSurface::RecordLand,
    };
    let wall_slopes = if surface == PickupContactSurface::GrooveWalls {
        point.wall_slopes
    } else {
        [0.0; 2]
    };
    let wall_contacts = wall_slopes.map(|groove_slope| {
        let mut set = StylusTraceContactSet {
            contact_count: 1,
            ..StylusTraceContactSet::default()
        };
        set.contacts[0].groove_slope = groove_slope;
        set
    });
    let input = PickupMechanicalInput {
        wall_contacts,
        contact_surface: surface,
        groove_radius_m: point.groove_radius_m,
        stylus_lowered: true,
        ..PickupMechanicalInput::default()
    };
    validate_input(input).map_err(|_| CoupledFixedModeResponseError::InvalidContactPoint)?;
    validate_friction_geometry(config.contact, input)
        .map_err(|_| CoupledFixedModeResponseError::InvalidContactPoint)?;

    let lateral_origin_shift_per_record_velocity_m_s = match family.origin_law {
        CoupledFixedOriginLaw::InteriorSpiral => {
            interior_spiral_origin_shift_per_record_velocity_m_s(
                config.record_cut.groove_pitch_m_per_revolution,
                dt,
            )
        }
        CoupledFixedOriginLaw::HeldProgramBoundary | CoupledFixedOriginLaw::SurfaceIndependent => {
            0.0
        }
    };
    let geometry = MidpointPickupGeometry {
        input,
        lateral_origin_shift_bias_m: 0.0,
        lateral_origin_shift_per_record_velocity_m_s,
    };
    let skating_factor = config
        .tonearm
        .geometry
        .equivalent_radial_force_n(point.groove_radius_m, 1.0)
        .map_err(|_| CoupledFixedModeResponseError::InvalidContactPoint)?;
    let stylus_body_velocity_coefficient = -2.0 * skating_factor / point.groove_radius_m;

    let reciprocal_damping_n_s_per_m =
        super::electromechanical::cartridge_mechanical_reciprocal_damping_n_s_per_m(
            config.cartridge,
            dt,
        )?;
    let mechanical = family.mechanical;
    let deck_bearing_mode = mechanical.deck_bearing_mode();
    let slipmat_mode = mechanical.slipmat_mode();
    let hand_mode = mechanical.hand_mode();
    let pickup_bearing_mode = mechanical.pickup_bearing_mode();
    let static_constraints = select_independent_deck_constraints(
        deck_bearing_mode,
        slipmat_mode,
        hand_mode,
        family.stylus,
        pickup_bearing_mode,
        0.0,
        0.0,
        stylus_body_velocity_coefficient,
    )
    .ok_or(CoupledFixedModeResponseError::SingularOrIllConditioned)?;

    let mut next_column = JOINT_DYNAMIC_VARIABLES;
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
    let stylus_column = static_constraints[3].then(|| {
        let column = next_column;
        next_column += 1;
        column
    });
    let static_columns = JointStaticConstraintColumns {
        pickup_bearing: pickup_bearing_column,
        deck_bearing: deck_bearing_column,
        slipmat: slipmat_column,
        hand: hand_column,
        stylus: stylus_column,
    };
    if next_column > JOINT_MAX_VARIABLES {
        return Err(CoupledFixedModeResponseError::SingularOrIllConditioned);
    }

    let mut augmented = [[0.0_f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES];
    assemble_deck_lhs(
        &mut augmented,
        config.deck,
        dt,
        deck_bearing_mode,
        slipmat_mode,
        hand_mode,
        deck_bearing_column,
        slipmat_column,
        hand_column,
    )
    .ok_or(CoupledFixedModeResponseError::SingularOrIllConditioned)?;
    assemble_pickup_lhs(
        &mut augmented,
        config.contact,
        config.tonearm,
        dt,
        true,
        reciprocal_damping_n_s_per_m,
        pickup_bearing_column,
    );
    write_joint_static_force_columns(&mut augmented, input, skating_factor, static_columns);
    let static_rows = append_joint_static_constraint_rows(
        &mut augmented,
        JOINT_DYNAMIC_VARIABLES,
        stylus_body_velocity_coefficient,
        static_columns,
    );
    if static_rows.next_row != next_column {
        return Err(CoupledFixedModeResponseError::SingularOrIllConditioned);
    }
    for row in &mut augmented {
        row[JOINT_RHS_COLUMN] = 0.0;
    }

    let requested_count =
        usize::from(mechanical.deck_bearing == FixedModeFrictionMobility::Sticking)
            + usize::from(mechanical.slipmat == FixedModeFrictionMobility::Sticking)
            + usize::from(mechanical.hand == FixedModeHandMobility::Sticking)
            + usize::from(mechanical.pickup_bearing == FixedModeFrictionMobility::Sticking)
            + usize::from(family.stylus == StylusTangentialMode::Sticking);
    let (kkt_lhs, equality) = fixed_kkt_lhs_and_equality(augmented, next_column, requested_count);
    let (dynamic_mobility, solve) = fixed_dynamic_mobility_and_solve(augmented, next_column)
        .ok_or(CoupledFixedModeResponseError::SingularOrIllConditioned)?;

    let friction_coefficient = match surface {
        PickupContactSurface::GrooveWalls => config.contact.groove_friction_coefficient,
        PickupContactSurface::RecordLand => config.contact.record_surface_friction_coefficient,
        PickupContactSurface::None => 0.0,
    };
    let contact_operator = coupled_contact_hg_operator(
        geometry,
        dt,
        friction_coefficient,
        family.stylus,
        skating_factor,
    );
    let suspension_axes = [config.tonearm.lateral, config.tonearm.vertical];
    let suspension_stiffness_n_per_m = suspension_axes.map(|axis| axis.stiffness_n_per_m());
    let suspension_viscous_damping_n_s_per_m =
        suspension_axes.map(|axis| axis.viscous_damping_n_s_per_m());
    let derived = CoupledFixedDerivedCoefficients {
        dt,
        groove_pitch_m_per_revolution: config.record_cut.groove_pitch_m_per_revolution,
        friction_coefficient,
        skating_factor,
        stylus_body_velocity_coefficient,
        lateral_origin_shift_per_record_velocity_m_s,
        platter_inertial_coefficient: config.deck.platter_inertia_kg_m2 / dt,
        record_inertial_coefficient: config.deck.record_inertia_kg_m2 / dt,
        stylus_inertial_coefficient: config.contact.moving_mass_kg / dt,
        body_inertial_coefficients: suspension_axes.map(|axis| axis.effective_mass_kg / dt),
        suspension_stiffness_n_per_m,
        suspension_viscous_damping_n_s_per_m,
        suspension_coupling_n_s_per_m: [0, 1].map(|axis| {
            suspension_stiffness_n_per_m[axis] * dt + suspension_viscous_damping_n_s_per_m[axis]
        }),
        deck_bearing_viscous_coefficient: config.deck.bearing_viscous_torque_nm_per_rad_s,
        slipmat_viscous_coefficient: config.deck.slipmat_viscous_torque_nm_per_rad_s,
        hand_viscous_coefficient: config.deck.hand_viscous_torque_nm_per_rad_s,
        pickup_bearing_viscous_coefficient: config
            .tonearm
            .lateral_bearing_viscous_damping_n_s_per_m,
        reciprocal_cartridge_damping_n_s_per_m: reciprocal_damping_n_s_per_m,
    };
    let mut w = [[0.0; 2]; 2];
    for source in 0..contact_operator.constraint_count() {
        let velocity =
            dynamic_mobility.response_for_equation_rhs(contact_operator.normal_force_rhs(source));
        for (target, response_row) in w
            .iter_mut()
            .enumerate()
            .take(contact_operator.constraint_count())
        {
            response_row[source] = contact_operator
                .normal_gap_velocity(target)
                .response_for_velocity(velocity);
        }
    }
    let normal_response = match family.surface {
        CoupledFixedContactSurface::GrooveWalls => CoupledFixedNormalResponse::GrooveWalls { w },
        CoupledFixedContactSurface::RecordLand => {
            CoupledFixedNormalResponse::RecordLand { w: w[0][0] }
        }
    };
    let wall_is_nonzero = point.wall_slopes.map(|slope| slope != 0.0);
    let nonzero_slope = wall_is_nonzero.into_iter().any(|value| value);
    let reachability = match family.stylus {
        StylusTangentialMode::Separated if friction_coefficient > 0.0 => {
            CoupledFixedModeReachability::NormalForceToleranceBandOnly
        }
        StylusTangentialMode::Sticking
            if family.surface == CoupledFixedContactSurface::GrooveWalls
                && family.origin_law == CoupledFixedOriginLaw::HeldProgramBoundary
                && nonzero_slope =>
        {
            CoupledFixedModeReachability::ActiveWallDependent {
                nonzero_active_wall:
                    CoupledFixedModeInfeasibility::SlopedGrooveStickingAtHeldBoundary,
                wall_is_nonzero,
            }
        }
        StylusTangentialMode::Sticking
            if family.surface == CoupledFixedContactSurface::GrooveWalls
                && friction_coefficient > 0.0
                && nonzero_slope =>
        {
            CoupledFixedModeReachability::ActiveWallDependent {
                nonzero_active_wall:
                    CoupledFixedModeInfeasibility::SlopedGrooveStickingWithFriction,
                wall_is_nonzero,
            }
        }
        _ => CoupledFixedModeReachability::RuntimeConditional,
    };
    Ok(CoupledFixedModeNormalResponse {
        operator_version: COUPLED_FIXED_MODE_OPERATOR_VERSION,
        family,
        reachability,
        equality,
        solve,
        derived,
        kkt_lhs,
        dynamic_mobility,
        contact_operator,
        normal_response,
    })
}

/// Builds one source-independent production KKT mobility.
pub(crate) fn coupled_fixed_solve_only_response(
    config: super::PhysicalPlaybackConfig,
    subject: CoupledFixedSolveOnlySubject,
) -> Result<CoupledFixedSolveOnlyResponse, CoupledFixedModeResponseError> {
    let config = config.validate()?;
    if subject.subject_set_version != COUPLED_FIXED_SOLVE_ONLY_SUBJECT_SET_VERSION
        || !coupled_fixed_solve_only_subjects().any(|candidate| candidate == subject)
    {
        return Err(CoupledFixedModeResponseError::InvalidSolveOnlySubject);
    }

    let dt = 1.0 / config.solver.internal_sample_rate_hz;
    let reciprocal_damping_n_s_per_m =
        super::electromechanical::cartridge_mechanical_reciprocal_damping_n_s_per_m(
            config.cartridge,
            dt,
        )?;
    let mechanical = subject.mechanical;
    let mut augmented = [[0.0_f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES];
    let layout = assemble_joint_no_contact_kkt_lhs(
        &mut augmented,
        config.deck,
        config.contact,
        config.tonearm,
        dt,
        dt,
        reciprocal_damping_n_s_per_m,
        mechanical.deck_bearing_mode(),
        mechanical.slipmat_mode(),
        mechanical.hand_mode(),
        mechanical.pickup_bearing_mode(),
        subject.support == CoupledFixedPickupSupport::LoweredNoContact,
        0.0,
    )
    .ok_or(CoupledFixedModeResponseError::SingularOrIllConditioned)?;

    let requested_count =
        usize::from(mechanical.deck_bearing == FixedModeFrictionMobility::Sticking)
            + usize::from(mechanical.slipmat == FixedModeFrictionMobility::Sticking)
            + usize::from(mechanical.hand == FixedModeHandMobility::Sticking)
            + usize::from(mechanical.pickup_bearing == FixedModeFrictionMobility::Sticking);
    let (kkt_lhs, equality) =
        fixed_kkt_lhs_and_equality(augmented, layout.system_size, requested_count);
    let (kkt_inverse, dynamic_mobility, solve) =
        fixed_complete_kkt_inverse_and_solve(augmented, layout.system_size)
            .ok_or(CoupledFixedModeResponseError::SingularOrIllConditioned)?;

    Ok(CoupledFixedSolveOnlyResponse {
        operator_version: COUPLED_FIXED_MODE_OPERATOR_VERSION,
        subject,
        equality,
        solve,
        kkt_lhs,
        kkt_inverse,
        dynamic_mobility,
    })
}

fn validate_fixed_family(
    family: CoupledFixedContactFamily,
) -> Result<(), CoupledFixedModeResponseError> {
    let origin_is_valid = matches!(
        (family.surface, family.origin_law),
        (
            CoupledFixedContactSurface::GrooveWalls,
            CoupledFixedOriginLaw::InteriorSpiral | CoupledFixedOriginLaw::HeldProgramBoundary
        ) | (
            CoupledFixedContactSurface::RecordLand,
            CoupledFixedOriginLaw::SurfaceIndependent
        )
    );
    if family.family_set_version != COUPLED_FIXED_MODE_FAMILY_SET_VERSION || !origin_is_valid {
        return Err(CoupledFixedModeResponseError::InvalidFamily);
    }
    Ok(())
}

fn dynamic_velocity_row_from_kkt_row(
    row: [f64; JOINT_MAX_VARIABLES + 1],
) -> JointDynamicVelocityRow {
    JointDynamicVelocityRow {
        platter: row[0],
        record: row[1],
        tip_x: row[2],
        tip_z: row[3],
        body_x: row[4],
        body_z: row[5],
    }
}

fn fixed_kkt_lhs_and_equality(
    augmented: [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    system_size: usize,
    requested_count: usize,
) -> (CoupledFixedModeKktLhs, CoupledFixedEqualityDiagnostics) {
    let equality_count = system_size - JOINT_DYNAMIC_VARIABLES;
    let dependent_count = requested_count.saturating_sub(equality_count);
    let equality = CoupledFixedEqualityDiagnostics {
        requested_count,
        rank: equality_count,
        dependent_count,
        runtime_rhs_compatibility_required: dependent_count != 0,
    };
    let mut equality_basis = [JointDynamicVelocityRow::default(); 5];
    for (basis, row) in equality_basis
        .iter_mut()
        .zip(JOINT_DYNAMIC_VARIABLES..system_size)
    {
        *basis = dynamic_velocity_row_from_kkt_row(augmented[row]);
    }
    let mut coefficients = [[0.0; JOINT_MAX_VARIABLES]; JOINT_MAX_VARIABLES];
    for row in 0..system_size {
        coefficients[row][..system_size].copy_from_slice(&augmented[row][..system_size]);
    }
    (
        CoupledFixedModeKktLhs {
            system_size,
            equality_count,
            coefficients,
            equality_basis,
        },
        equality,
    )
}

fn fixed_dynamic_mobility_and_solve(
    augmented: [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    system_size: usize,
) -> Option<(CoupledFixedDynamicMobility, CoupledFixedSolveDiagnostics)> {
    let mut dynamic_mobility = CoupledFixedDynamicMobility {
        velocity_by_equation_rhs: [[0.0; JOINT_DYNAMIC_VARIABLES]; JOINT_DYNAMIC_VARIABLES],
    };
    let mut minimum_scaled_pivot = f64::INFINITY;
    let mut maximum_scaled_pivot = 0.0_f64;
    let mut maximum_backward_error = 0.0_f64;
    for equation_rhs in 0..JOINT_DYNAMIC_VARIABLES {
        let mut system = augmented;
        system[equation_rhs][JOINT_RHS_COLUMN] = 1.0;
        let solved = solve_joint_linear_system_with_diagnostics(&mut system, system_size)?;
        for velocity in 0..JOINT_DYNAMIC_VARIABLES {
            dynamic_mobility.velocity_by_equation_rhs[velocity][equation_rhs] =
                solved.solution[velocity];
        }
        minimum_scaled_pivot = minimum_scaled_pivot.min(solved.diagnostics.minimum_scaled_pivot);
        maximum_scaled_pivot = maximum_scaled_pivot.max(solved.diagnostics.maximum_scaled_pivot);
        maximum_backward_error = maximum_backward_error.max(solved.diagnostics.backward_error);
    }
    Some((
        dynamic_mobility,
        CoupledFixedSolveDiagnostics {
            system_size,
            minimum_scaled_pivot,
            maximum_scaled_pivot,
            scaled_pivot_ratio: minimum_scaled_pivot / maximum_scaled_pivot,
            maximum_backward_error,
        },
    ))
}

fn fixed_complete_kkt_inverse_and_solve(
    augmented: [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    system_size: usize,
) -> Option<(
    CoupledFixedKktInverse,
    CoupledFixedDynamicMobility,
    CoupledFixedSolveDiagnostics,
)> {
    if !(JOINT_DYNAMIC_VARIABLES..=JOINT_MAX_VARIABLES).contains(&system_size) {
        return None;
    }
    let mut solution_by_rhs = [[0.0; JOINT_MAX_VARIABLES]; JOINT_MAX_VARIABLES];
    let mut minimum_scaled_pivot = f64::INFINITY;
    let mut maximum_scaled_pivot = 0.0_f64;
    let mut maximum_backward_error = 0.0_f64;
    for rhs_index in 0..system_size {
        let mut system = augmented;
        system[rhs_index][JOINT_RHS_COLUMN] = 1.0;
        let solved = solve_joint_linear_system_with_diagnostics(&mut system, system_size)?;
        for (solution_index, solution_row) in
            solution_by_rhs.iter_mut().enumerate().take(system_size)
        {
            solution_row[rhs_index] = solved.solution[solution_index];
        }
        minimum_scaled_pivot = minimum_scaled_pivot.min(solved.diagnostics.minimum_scaled_pivot);
        maximum_scaled_pivot = maximum_scaled_pivot.max(solved.diagnostics.maximum_scaled_pivot);
        maximum_backward_error = maximum_backward_error.max(solved.diagnostics.backward_error);
    }
    let kkt_inverse = CoupledFixedKktInverse {
        system_size,
        equality_count: system_size.checked_sub(JOINT_DYNAMIC_VARIABLES)?,
        solution_by_rhs,
    };
    if !kkt_inverse.has_valid_inactive_storage() {
        return None;
    }
    let dynamic_mobility = kkt_inverse.dynamic_mobility();
    Some((
        kkt_inverse,
        dynamic_mobility,
        CoupledFixedSolveDiagnostics {
            system_size,
            minimum_scaled_pivot,
            maximum_scaled_pivot,
            scaled_pivot_ratio: minimum_scaled_pivot / maximum_scaled_pivot,
            maximum_backward_error,
        },
    ))
}

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
    validate_friction_geometry(pickup.contact, geometry.input)?;
    let nominal_pickup_sample_rate_hz = pickup.sample_rate_hz;
    let nominal_pickup_dt = 1.0 / nominal_pickup_sample_rate_hz;
    if !geometry.lateral_origin_shift_bias_m.is_finite()
        || !geometry
            .lateral_origin_shift_per_record_velocity_m_s
            .is_finite()
        || deck.dt > nominal_pickup_dt * (1.0 + 16.0 * f64::EPSILON)
    {
        return Err(CoupledDeckPickupError::InvalidMidpointGeometry);
    }
    // The public pickup state keeps the output processing rate. A bounded
    // swept-contact solve may use shorter internal steps. All pickup equations
    // and the atomic commit must use the prepared deck duration.
    let mut pickup = pickup;
    pickup.sample_rate_hz = 1.0 / deck.dt;
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
    let skating_factor = if input.stylus_lowered {
        pickup
            .tonearm
            .geometry
            .equivalent_radial_force_n(input.groove_radius_m, 1.0)
            .map_err(PickupMechanicalError::from)?
    } else {
        0.0
    };
    let pickup_bearing_modes = pickup_bearing_mode_order(pickup.body_velocity_m_s[0]);
    let predicted_relative_velocity_m_s = along_groove_slip_velocity_m_s(
        0.5 * input.groove_radius_m
            * (deck.previous_record_velocity_rad_s + deck.predicted_record_velocity_rad_s),
        skating_factor,
        pickup.body_velocity_m_s[0],
    );
    let stylus_modes =
        tangential_mode_order(previous_tangential_mode, predicted_relative_velocity_m_s);
    let mut evaluated_branches = 0_u32;
    let mut attempted_linear_solves = 0_u32;
    let mut rejected_unsupported_groove_wall_sticking = false;

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
                            if stylus_mode == StylusTangentialMode::Sticking
                                && input.contact_surface == PickupContactSurface::GrooveWalls
                                && pickup.contact.groove_friction_coefficient > 0.0
                                && active_wall_has_nonzero_slope(input, active_mask)
                            {
                                rejected_unsupported_groove_wall_sticking = true;
                                continue;
                            }
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
                            next_pickup.sample_rate_hz = nominal_pickup_sample_rate_hz;
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
    if rejected_unsupported_groove_wall_sticking
        && predicted_relative_velocity_m_s.abs() <= ZERO_SLIP_REGULARIZATION_VELOCITY_M_S
    {
        // Ideal Coulomb friction does not select one force at zero slip.
        // Use the zero-traction member for this sample. The next signed-slip
        // sample returns to the complete kinetic wall-friction solve.
        let original_contact = pickup.contact;
        let mut regularized_pickup = pickup;
        regularized_pickup.contact.groove_friction_coefficient = 0.0;
        let mut step = solve_coupled_deck_pickup_midpoint(
            deck,
            regularized_pickup,
            geometry,
            relation,
            StylusTangentialMode::Separated,
        )?;
        step.pickup.contact = original_contact;
        step.pickup.sample_rate_hz = nominal_pickup_sample_rate_hz;
        return Ok(step);
    }
    if rejected_unsupported_groove_wall_sticking {
        return Err(CoupledDeckPickupError::Pickup(
            PickupMechanicalError::UnsupportedGrooveWallSticking,
        ));
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct JointStaticConstraintColumns {
    pickup_bearing: Option<usize>,
    deck_bearing: Option<usize>,
    slipmat: Option<usize>,
    hand: Option<usize>,
    stylus: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct JointStaticConstraintRows {
    next_row: usize,
    hand: Option<usize>,
    stylus: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JointNoContactKktLayout {
    system_size: usize,
    columns: JointStaticConstraintColumns,
    rows: JointStaticConstraintRows,
}

/// Builds the production sticking-force column in dynamic equation order.
pub(crate) fn stylus_sticking_force_column(
    groove_radius_m: f64,
    skating_factor: f64,
) -> JointDynamicEquationRhs {
    JointDynamicEquationRhs {
        record: -groove_radius_m,
        body_x: skating_factor,
        ..JointDynamicEquationRhs::default()
    }
}

/// Builds the production sticking equality in dynamic velocity order.
pub(crate) fn stylus_sticking_equality_row(
    stylus_body_velocity_coefficient: f64,
) -> JointDynamicVelocityRow {
    JointDynamicVelocityRow {
        record: 1.0,
        body_x: stylus_body_velocity_coefficient,
        ..JointDynamicVelocityRow::default()
    }
}

fn write_joint_static_force_columns(
    augmented: &mut [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    input: PickupMechanicalInput,
    skating_factor: f64,
    columns: JointStaticConstraintColumns,
) {
    if let Some(column) = columns.stylus {
        let force = stylus_sticking_force_column(input.groove_radius_m, skating_factor);
        augmented[1][column] = force.coefficient(JointDynamicEquation::Record);
        augmented[3][column] = force.coefficient(JointDynamicEquation::BodyX);
    }
}

fn append_joint_static_constraint_rows(
    augmented: &mut [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    mut next_row: usize,
    stylus_body_velocity_coefficient: f64,
    columns: JointStaticConstraintColumns,
) -> JointStaticConstraintRows {
    if columns.pickup_bearing.is_some() {
        augmented[next_row][4] = 1.0;
        next_row += 1;
    }
    if columns.deck_bearing.is_some() {
        augmented[next_row][0] = 1.0;
        next_row += 1;
    }
    if columns.slipmat.is_some() {
        augmented[next_row][0] = 1.0;
        augmented[next_row][1] = -1.0;
        next_row += 1;
    }
    let hand = columns.hand.map(|_| {
        let row = next_row;
        augmented[row][1] = 1.0;
        next_row += 1;
        row
    });
    let stylus = columns.stylus.map(|_| {
        let row = next_row;
        let equality = stylus_sticking_equality_row(stylus_body_velocity_coefficient);
        augmented[row][1] = equality.coefficient(JointDynamicVelocity::Record);
        augmented[row][4] = equality.coefficient(JointDynamicVelocity::BodyX);
        next_row += 1;
        row
    });
    JointStaticConstraintRows {
        next_row,
        hand,
        stylus,
    }
}

/// Assembles the canonical source-independent KKT for a separated stylus.
#[allow(clippy::too_many_arguments)]
fn assemble_joint_no_contact_kkt_lhs(
    augmented: &mut [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    deck_config: crate::PhysicalDeckConfig,
    contact: StylusContactConfig,
    tonearm: TonearmConfig,
    deck_dt: f64,
    pickup_dt: f64,
    reciprocal_damping_n_s_per_m: [[f64; 2]; 2],
    deck_bearing_mode: CoupledDeckFrictionMode,
    slipmat_mode: CoupledDeckFrictionMode,
    hand_mode: CoupledDeckFrictionMode,
    pickup_bearing_mode: BearingMode,
    stylus_lowered: bool,
    hand_velocity_rad_s: f64,
) -> Option<JointNoContactKktLayout> {
    let static_constraints = select_independent_deck_constraints(
        deck_bearing_mode,
        slipmat_mode,
        hand_mode,
        StylusTangentialMode::Separated,
        pickup_bearing_mode,
        hand_velocity_rad_s,
        0.0,
        0.0,
    )?;

    let mut next_column = JOINT_DYNAMIC_VARIABLES;
    let pickup_bearing = (pickup_bearing_mode == BearingMode::Stick).then(|| {
        let column = next_column;
        next_column += 1;
        column
    });
    let deck_bearing = static_constraints[0].then(|| {
        let column = next_column;
        next_column += 1;
        column
    });
    let slipmat = static_constraints[1].then(|| {
        let column = next_column;
        next_column += 1;
        column
    });
    let hand = static_constraints[2].then(|| {
        let column = next_column;
        next_column += 1;
        column
    });
    let columns = JointStaticConstraintColumns {
        pickup_bearing,
        deck_bearing,
        slipmat,
        hand,
        stylus: None,
    };
    if next_column > JOINT_MAX_VARIABLES {
        return None;
    }

    assemble_deck_lhs(
        augmented,
        deck_config,
        deck_dt,
        deck_bearing_mode,
        slipmat_mode,
        hand_mode,
        columns.deck_bearing,
        columns.slipmat,
        columns.hand,
    )?;
    assemble_pickup_lhs(
        augmented,
        contact,
        tonearm,
        pickup_dt,
        stylus_lowered,
        reciprocal_damping_n_s_per_m,
        columns.pickup_bearing,
    );
    let rows =
        append_joint_static_constraint_rows(augmented, JOINT_DYNAMIC_VARIABLES, 0.0, columns);
    if rows.next_row != next_column {
        return None;
    }
    Some(JointNoContactKktLayout {
        system_size: next_column,
        columns,
        rows,
    })
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
    if active_count == 0 && stylus_mode == StylusTangentialMode::Separated {
        return solve_joint_no_contact_branch(
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
            skating_factor,
            attempted_linear_solves,
        );
    }
    let stylus_body_velocity_coefficient = -2.0 * skating_factor / input.groove_radius_m;
    let static_constraints = select_independent_deck_constraints(
        deck_bearing_mode,
        slipmat_mode,
        hand_mode,
        stylus_mode,
        pickup_bearing_mode,
        deck.hand_velocity_rad_s,
        -deck.previous_record_velocity_rad_s,
        stylus_body_velocity_coefficient,
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
    let static_columns = JointStaticConstraintColumns {
        pickup_bearing: pickup_bearing_column,
        deck_bearing: deck_bearing_column,
        slipmat: slipmat_column,
        hand: hand_column,
        stylus: stylus_force_column,
    };
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

    let friction_coefficient = match input.contact_surface {
        PickupContactSurface::GrooveWalls => pickup.contact.groove_friction_coefficient,
        PickupContactSurface::RecordLand => pickup.contact.record_surface_friction_coefficient,
        PickupContactSurface::None => 0.0,
    };
    let contact_operator = coupled_contact_hg_operator(
        geometry,
        deck.dt,
        friction_coefficient,
        stylus_mode,
        skating_factor,
    );
    write_joint_static_force_columns(&mut augmented, input, skating_factor, static_columns);
    for (constraint, lambda_column) in lambda_columns
        .into_iter()
        .enumerate()
        .take(contact_operator.constraint_count())
    {
        if let Some(column) = lambda_column {
            contact_operator
                .normal_force_rhs(constraint)
                .write_contact_column(&mut augmented, column);
        }
    }

    let mut next_row = JOINT_DYNAMIC_VARIABLES;
    for &constraint in &active_constraints[..active_count] {
        let normal = constraint_normal(input.contact_surface, constraint);
        let displacement = constraint_midpoint_displacement(input, constraint);
        contact_operator
            .normal_gap_velocity(constraint)
            .write_constraint_row(&mut augmented[next_row]);
        augmented[next_row][JOINT_RHS_COLUMN] = (displacement
            - dot(normal, pickup.tip_displacement_m)
            + normal[0] * geometry.lateral_origin_shift_bias_m)
            / deck.dt;
        next_row += 1;
    }
    let static_rows = append_joint_static_constraint_rows(
        &mut augmented,
        next_row,
        stylus_body_velocity_coefficient,
        static_columns,
    );
    if let Some(row) = static_rows.hand {
        augmented[row][JOINT_RHS_COLUMN] = deck.hand_velocity_rad_s;
    }
    if let Some(row) = static_rows.stylus {
        augmented[row][JOINT_RHS_COLUMN] = -deck.previous_record_velocity_rad_s;
    }
    next_row = static_rows.next_row;
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
        skating_factor,
    )
}

#[allow(clippy::too_many_arguments)]
fn solve_joint_no_contact_branch(
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
    skating_factor: f64,
    attempted_linear_solves: &mut u32,
) -> Option<JointCandidate> {
    let input = geometry.input;
    let mut augmented = [[0.0_f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES];
    let layout = assemble_joint_no_contact_kkt_lhs(
        &mut augmented,
        deck.config,
        pickup.contact,
        pickup.tonearm,
        deck.dt,
        1.0 / pickup.sample_rate_hz,
        relation.reciprocal_damping_n_s_per_m,
        deck_bearing_mode,
        slipmat_mode,
        hand_mode,
        pickup_bearing_mode,
        input.stylus_lowered,
        deck.hand_velocity_rad_s,
    )?;
    assemble_deck_rhs(
        &mut augmented,
        deck,
        deck_bearing_mode,
        slipmat_mode,
        hand_mode,
    );
    assemble_pickup_rhs(&mut augmented, pickup, input, relation, pickup_bearing_mode);
    if let Some(row) = layout.rows.hand {
        augmented[row][JOINT_RHS_COLUMN] = deck.hand_velocity_rad_s;
    }

    *attempted_linear_solves = attempted_linear_solves.saturating_add(1);
    let solution = solve_joint_linear_system(&mut augmented, layout.system_size)?;
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
        0,
        [None; 2],
        layout.columns.pickup_bearing,
        layout.columns.deck_bearing,
        layout.columns.slipmat,
        layout.columns.hand,
        None,
        StylusTangentialMode::Separated,
        skating_factor,
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

#[allow(clippy::too_many_arguments)]
fn select_independent_deck_constraints(
    bearing_mode: CoupledDeckFrictionMode,
    slipmat_mode: CoupledDeckFrictionMode,
    hand_mode: CoupledDeckFrictionMode,
    stylus_mode: StylusTangentialMode,
    pickup_bearing_mode: BearingMode,
    hand_velocity_rad_s: f64,
    stylus_velocity_rad_s: f64,
    stylus_body_velocity_coefficient: f64,
) -> Option<[bool; 4]> {
    let stylus_has_independent_body_velocity = stylus_mode == StylusTangentialMode::Sticking
        && pickup_bearing_mode != BearingMode::Stick
        && stylus_body_velocity_coefficient != 0.0;
    let requested = [
        bearing_mode.is_sticking().then_some(([1.0, 0.0], 0.0)),
        slipmat_mode.is_sticking().then_some(([1.0, -1.0], 0.0)),
        hand_mode
            .is_sticking()
            .then_some(([0.0, 1.0], hand_velocity_rad_s)),
        (stylus_mode == StylusTangentialMode::Sticking && !stylus_has_independent_body_velocity)
            .then_some(([0.0, 1.0], stylus_velocity_rad_s)),
    ];
    let mut basis_vectors = [[0.0_f64; 2]; 2];
    let mut basis_rhs = [0.0_f64; 2];
    let mut rank = 0;
    let mut selected = [false, false, false, stylus_has_independent_body_velocity];
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
    assemble_deck_lhs(
        augmented,
        deck.config,
        deck.dt,
        bearing_mode,
        slipmat_mode,
        hand_mode,
        bearing_column,
        slipmat_column,
        hand_column,
    )?;
    assemble_deck_rhs(augmented, deck, bearing_mode, slipmat_mode, hand_mode);
    Some(())
}

#[allow(clippy::too_many_arguments)]
fn assemble_deck_lhs(
    augmented: &mut [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    config: crate::PhysicalDeckConfig,
    dt: f64,
    bearing_mode: CoupledDeckFrictionMode,
    slipmat_mode: CoupledDeckFrictionMode,
    hand_mode: CoupledDeckFrictionMode,
    bearing_column: Option<usize>,
    slipmat_column: Option<usize>,
    hand_column: Option<usize>,
) -> Option<()> {
    let platter_mass = config.platter_inertia_kg_m2 / dt;
    let record_mass = config.record_inertia_kg_m2 / dt;
    augmented[0][0] = platter_mass;
    augmented[1][1] = record_mass;

    match bearing_mode {
        CoupledDeckFrictionMode::Sticking => {
            if let Some(column) = bearing_column {
                augmented[0][column] = -1.0;
            }
        }
        CoupledDeckFrictionMode::SlidingPositive => {
            augmented[0][0] += config.bearing_viscous_torque_nm_per_rad_s;
        }
        CoupledDeckFrictionMode::SlidingNegative => {
            augmented[0][0] += config.bearing_viscous_torque_nm_per_rad_s;
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
            let damping = config.slipmat_viscous_torque_nm_per_rad_s;
            augmented[0][0] += damping;
            augmented[0][1] -= damping;
            augmented[1][0] -= damping;
            augmented[1][1] += damping;
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
            let damping = config.hand_viscous_torque_nm_per_rad_s;
            augmented[1][1] += damping;
        }
        CoupledDeckFrictionMode::Separated => {}
    }
    Some(())
}

fn assemble_deck_rhs(
    augmented: &mut [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    deck: DeckMidpointPreparation,
    bearing_mode: CoupledDeckFrictionMode,
    slipmat_mode: CoupledDeckFrictionMode,
    hand_mode: CoupledDeckFrictionMode,
) {
    let platter_mass = deck.config.platter_inertia_kg_m2 / deck.dt;
    let record_mass = deck.config.record_inertia_kg_m2 / deck.dt;
    augmented[0][JOINT_RHS_COLUMN] =
        platter_mass * deck.previous_platter_velocity_rad_s + deck.motor_torque_nm;
    augmented[1][JOINT_RHS_COLUMN] = record_mass * deck.previous_record_velocity_rad_s;
    match bearing_mode {
        CoupledDeckFrictionMode::SlidingPositive => {
            augmented[0][JOINT_RHS_COLUMN] -= deck.config.bearing_kinetic_torque_nm;
        }
        CoupledDeckFrictionMode::SlidingNegative => {
            augmented[0][JOINT_RHS_COLUMN] += deck.config.bearing_kinetic_torque_nm;
        }
        CoupledDeckFrictionMode::Sticking | CoupledDeckFrictionMode::Separated => {}
    }
    if matches!(
        slipmat_mode,
        CoupledDeckFrictionMode::SlidingPositive | CoupledDeckFrictionMode::SlidingNegative
    ) {
        let bias = if slipmat_mode == CoupledDeckFrictionMode::SlidingPositive {
            deck.config.slipmat_kinetic_torque_nm
        } else {
            -deck.config.slipmat_kinetic_torque_nm
        };
        augmented[0][JOINT_RHS_COLUMN] -= bias;
        augmented[1][JOINT_RHS_COLUMN] += bias;
    }
    if matches!(
        hand_mode,
        CoupledDeckFrictionMode::SlidingPositive | CoupledDeckFrictionMode::SlidingNegative
    ) {
        let bias = if hand_mode == CoupledDeckFrictionMode::SlidingPositive {
            deck.hand_kinetic_limit_nm
        } else {
            -deck.hand_kinetic_limit_nm
        };
        let damping = deck.config.hand_viscous_torque_nm_per_rad_s;
        augmented[1][JOINT_RHS_COLUMN] += bias + damping * deck.hand_velocity_rad_s;
    }
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
    assemble_pickup_lhs(
        augmented,
        pickup.contact,
        pickup.tonearm,
        dt,
        input.stylus_lowered,
        relation.reciprocal_damping_n_s_per_m,
        bearing_column,
    );
    assemble_pickup_rhs(augmented, pickup, input, relation, bearing_mode);
}

#[allow(clippy::too_many_arguments)]
fn assemble_pickup_lhs(
    augmented: &mut [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    contact: StylusContactConfig,
    tonearm: TonearmConfig,
    dt: f64,
    stylus_lowered: bool,
    reciprocal_damping_n_s_per_m: [[f64; 2]; 2],
    bearing_column: Option<usize>,
) {
    let axes = [tonearm.lateral, tonearm.vertical];
    for axis in 0..2 {
        let tip_row = 2 + axis * 2;
        let body_row = tip_row + 1;
        let tip_column = 2 + axis;
        let body_column = 4 + axis;
        let stiffness = axes[axis].stiffness_n_per_m();
        let damping = axes[axis].viscous_damping_n_s_per_m();
        let coupling = stiffness * dt + damping;
        augmented[tip_row][tip_column] = contact.moving_mass_kg / dt + coupling;
        augmented[tip_row][body_column] = -coupling;
        augmented[body_row][tip_column] = -coupling;
        augmented[body_row][body_column] = axes[axis].effective_mass_kg / dt + coupling;
        for velocity_axis in 0..2 {
            let reciprocal_damping = reciprocal_damping_n_s_per_m[axis][velocity_axis];
            augmented[tip_row][2 + velocity_axis] += reciprocal_damping;
            augmented[tip_row][4 + velocity_axis] -= reciprocal_damping;
            augmented[body_row][2 + velocity_axis] -= reciprocal_damping;
            augmented[body_row][4 + velocity_axis] += reciprocal_damping;
        }
        if axis == 0 {
            augmented[body_row][body_column] += tonearm.lateral_bearing_viscous_damping_n_s_per_m;
            if let Some(column) = bearing_column {
                augmented[body_row][column] = -1.0;
            }
        }
        if axis == 1 && !stylus_lowered {
            let cue_coupling =
                tonearm.cue_support_stiffness_n_per_m * dt + tonearm.cue_support_damping_n_s_per_m;
            augmented[body_row][body_column] += cue_coupling;
        }
    }
}

fn assemble_pickup_rhs(
    augmented: &mut [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    pickup: PickupMechanicalState,
    input: PickupMechanicalInput,
    relation: PickupElectromagneticForceRelation,
    bearing_mode: BearingMode,
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
        let stiffness = axes[axis].stiffness_n_per_m();
        let relative_displacement =
            pickup.tip_displacement_m[axis] - pickup.body_displacement_m[axis];
        augmented[tip_row][JOINT_RHS_COLUMN] = pickup.contact.moving_mass_kg / dt
            * pickup.tip_velocity_m_s[axis]
            - stiffness * relative_displacement
            + relation.force_bias_n[axis];
        augmented[body_row][JOINT_RHS_COLUMN] = axes[axis].effective_mass_kg / dt
            * pickup.body_velocity_m_s[axis]
            + stiffness * relative_displacement
            + body_external_force_n[axis]
            - relation.force_bias_n[axis];
        if axis == 0 {
            augmented[body_row][JOINT_RHS_COLUMN] += match bearing_mode {
                BearingMode::Stick => 0.0,
                BearingMode::Positive => -pickup.tonearm.lateral_bearing_kinetic_friction_n,
                BearingMode::Negative => pickup.tonearm.lateral_bearing_kinetic_friction_n,
            };
        }
        if axis == 1 && !input.stylus_lowered {
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
    skating_factor: f64,
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
            + effective_slope[wall]
                * deck.dt
                * (0.5 * input.groove_radius_m * record_velocity_rad_s
                    - skating_factor * body_velocity_m_s[0])
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
    let zero_friction_wall_contact = distribute_wall_contact_forces(
        input,
        wall_projected_force_n,
        WallFrictionDistribution::None,
    );
    let (friction_normal_force_n, friction_coefficient) = match input.contact_surface {
        PickupContactSurface::GrooveWalls => (
            wall_projected_force_n[0] + wall_projected_force_n[1],
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
    let tangential_relative_velocity_m_s = along_groove_slip_velocity_m_s(
        0.5 * input.groove_radius_m * (deck.previous_record_velocity_rad_s + record_velocity_rad_s),
        skating_factor,
        body_velocity_m_s[0],
    );
    let (record_reaction_force_tangent_n, coulomb_friction_force_n) = match stylus_mode {
        StylusTangentialMode::Sticking => {
            if friction_normal_force_n <= TANGENTIAL_FORCE_TOLERANCE_N
                || tangential_relative_velocity_m_s.abs() > 1.0e-12
            {
                return None;
            }
            let total = stylus_force_column.map_or(0.0, |column| solution[column]);
            let friction = total - modulation_reaction_force_n;
            if friction.abs()
                > friction_coefficient * friction_normal_force_n + TANGENTIAL_FORCE_TOLERANCE_N
            {
                return None;
            }
            (total, friction)
        }
        StylusTangentialMode::SlidingPositive => {
            if friction_normal_force_n <= TANGENTIAL_FORCE_TOLERANCE_N
                || tangential_relative_velocity_m_s <= 0.0
            {
                return None;
            }
            let friction = -friction_coefficient * friction_normal_force_n;
            (modulation_reaction_force_n + friction, friction)
        }
        StylusTangentialMode::SlidingNegative => {
            if friction_normal_force_n <= TANGENTIAL_FORCE_TOLERANCE_N
                || tangential_relative_velocity_m_s >= 0.0
            {
                return None;
            }
            let friction = friction_coefficient * friction_normal_force_n;
            (modulation_reaction_force_n + friction, friction)
        }
        StylusTangentialMode::Separated => {
            if input.stylus_lowered
                && input.contact_surface != PickupContactSurface::None
                && friction_normal_force_n > TANGENTIAL_FORCE_TOLERANCE_N
                && friction_coefficient > 0.0
            {
                return None;
            }
            (modulation_reaction_force_n, 0.0)
        }
    };
    let friction_power_w = if input.contact_surface == PickupContactSurface::GrooveWalls {
        wall_coulomb_friction_power_w(
            distribute_wall_contact_forces(
                input,
                wall_projected_force_n,
                wall_friction_distribution(
                    stylus_mode,
                    friction_coefficient,
                    coulomb_friction_force_n,
                ),
            ),
            tangential_relative_velocity_m_s,
            wall_coordinate_velocity_m_s(tip_velocity_m_s),
        )
    } else {
        coulomb_friction_force_n * tangential_relative_velocity_m_s
    };
    if friction_power_w > 1.0e-18 {
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
            tangential_mode: stylus_mode,
            sticking_friction_force_n: 0.0,
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
    solve_joint_linear_system_with_diagnostics(augmented, size).map(|solved| solved.solution)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct JointLinearSolveDiagnostics {
    minimum_scaled_pivot: f64,
    maximum_scaled_pivot: f64,
    backward_error: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct JointLinearSolveResult {
    solution: [f64; JOINT_MAX_VARIABLES],
    diagnostics: JointLinearSolveDiagnostics,
}

fn solve_joint_linear_system_with_diagnostics(
    augmented: &mut [[f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES],
    size: usize,
) -> Option<JointLinearSolveResult> {
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
    let mut minimum_scaled_pivot = f64::INFINITY;
    let mut maximum_scaled_pivot = 0.0_f64;
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
        minimum_scaled_pivot = minimum_scaled_pivot.min(pivot.abs());
        maximum_scaled_pivot = maximum_scaled_pivot.max(pivot.abs());
        augmented.swap(pivot_column, pivot_row);
        for coefficient in &mut augmented[pivot_column][pivot_column..size] {
            *coefficient /= pivot;
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
    (backward_error.is_finite() && backward_error <= 1.0e-10).then_some(JointLinearSolveResult {
        solution,
        diagnostics: JointLinearSolveDiagnostics {
            minimum_scaled_pivot,
            maximum_scaled_pivot,
            backward_error,
        },
    })
}

#[derive(Debug, Clone, Copy)]
struct PickupStepSolution {
    tip_velocity_m_s: [f64; 2],
    body_velocity_m_s: [f64; 2],
    wall_projected_force_n: [f64; 2],
    wall_contact: [bool; 2],
    land_normal_force_n: f64,
    bearing_friction_force_n: f64,
    tangential_mode: StylusTangentialMode,
    sticking_friction_force_n: f64,
}

#[derive(Debug, Clone, Copy)]
struct PickupTangentialForces {
    coulomb_friction_force_n: f64,
    modulation_reaction_force_n: f64,
    record_reaction_force_tangent_n: f64,
    relative_velocity_m_s: f64,
    mode: StylusTangentialMode,
}

fn classify_tangential_velocity(velocity_m_s: f64) -> StylusTangentialMode {
    if velocity_m_s > TANGENTIAL_VELOCITY_TOLERANCE_M_S {
        StylusTangentialMode::SlidingPositive
    } else if velocity_m_s < -TANGENTIAL_VELOCITY_TOLERANCE_M_S {
        StylusTangentialMode::SlidingNegative
    } else {
        StylusTangentialMode::Sticking
    }
}

fn standalone_tangential_mode(
    input: PickupMechanicalInput,
    wall_projected_force_n: [f64; 2],
    land_normal_force_n: f64,
    tip_velocity_m_s: [f64; 2],
    body_velocity_m_s: [f64; 2],
    skating_factor: f64,
) -> Option<StylusTangentialMode> {
    if !input.stylus_lowered || input.contact_surface == PickupContactSurface::None {
        return Some(StylusTangentialMode::Separated);
    }
    let slip_velocity_m_s = along_groove_slip_velocity_m_s(
        input.groove_tangential_velocity_m_s,
        skating_factor,
        body_velocity_m_s[0],
    );
    match input.contact_surface {
        PickupContactSurface::None => Some(StylusTangentialMode::Separated),
        PickupContactSurface::RecordLand => {
            if land_normal_force_n <= TANGENTIAL_FORCE_TOLERANCE_N {
                Some(StylusTangentialMode::Separated)
            } else {
                Some(classify_tangential_velocity(slip_velocity_m_s))
            }
        }
        PickupContactSurface::GrooveWalls => {
            let wall_velocity_m_s = wall_coordinate_velocity_m_s(tip_velocity_m_s);
            let mut common_mode = None;
            for wall in 0..2 {
                if wall_projected_force_n[wall] <= TANGENTIAL_FORCE_TOLERANCE_N {
                    continue;
                }
                let count = wall_contact_count(input.wall_contacts[wall]).unwrap_or(1);
                for contact in &input.wall_contacts[wall].contacts[..count] {
                    let gamma_dot_m_s =
                        slip_velocity_m_s + contact.groove_slope * wall_velocity_m_s[wall];
                    let mode = classify_tangential_velocity(gamma_dot_m_s);
                    if common_mode.is_some_and(|common| common != mode) {
                        return None;
                    }
                    common_mode = Some(mode);
                }
            }
            Some(common_mode.unwrap_or(StylusTangentialMode::Separated))
        }
    }
}

fn sliding_direction_matches_mode(direction: f64, mode: StylusTangentialMode) -> bool {
    match mode {
        StylusTangentialMode::SlidingPositive => direction == 1.0,
        StylusTangentialMode::SlidingNegative => direction == -1.0,
        StylusTangentialMode::Sticking | StylusTangentialMode::Separated => direction == 0.0,
    }
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
    let effective_slope = wall_effective_slope(input);
    let constraint_count = if !input.stylus_lowered {
        0
    } else {
        match input.contact_surface {
            PickupContactSurface::None => 0,
            PickupContactSurface::GrooveWalls => {
                constraint_normals = WALL_NORMALS;
                constraint_displacement_m = wall_displacement_m(input);
                2
            }
            PickupContactSurface::RecordLand => {
                constraint_normals[0] = [0.0, 1.0];
                constraint_displacement_m[0] = input.land_displacement_m;
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
    let predicted_slip_velocity_m_s = along_groove_slip_velocity_m_s(
        input.groove_tangential_velocity_m_s,
        skating_factor,
        body_velocity_m_s[0],
    );
    let sliding_directions = if predicted_slip_velocity_m_s > 0.0 {
        [1.0, 0.0, -1.0]
    } else if predicted_slip_velocity_m_s < 0.0 {
        [-1.0, 0.0, 1.0]
    } else {
        [0.0, 1.0, -1.0]
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
            let candidate_directions = if active_count == 0 {
                [0.0; 3]
            } else {
                sliding_directions
            };
            let direction_count = if active_count == 0 { 1 } else { 3 };
            for &sliding_direction in &candidate_directions[..direction_count] {
                let mut tangential_force_per_normal = [0.0_f64; 2];
                let mut wall_force_scale_per_normal = [1.0_f64; 2];
                for constraint in 0..constraint_count {
                    match input.contact_surface {
                        PickupContactSurface::GrooveWalls => {
                            tangential_force_per_normal[constraint] = effective_slope[constraint]
                                + sliding_direction * contact.groove_friction_coefficient;
                            wall_force_scale_per_normal[constraint] = 1.0
                                - sliding_direction
                                    * contact.groove_friction_coefficient
                                    * effective_slope[constraint];
                        }
                        PickupContactSurface::RecordLand => {
                            tangential_force_per_normal[constraint] =
                                sliding_direction * contact.record_surface_friction_coefficient;
                        }
                        PickupContactSurface::None => {}
                    }
                }
                let bearing_sticks = bearing_mode == BearingMode::Stick;
                let friction_coefficient = match input.contact_surface {
                    PickupContactSurface::GrooveWalls => contact.groove_friction_coefficient,
                    PickupContactSurface::RecordLand => contact.record_surface_friction_coefficient,
                    PickupContactSurface::None => 0.0,
                };
                let flat_sticking_requested = sliding_direction == 0.0
                    && active_count > 0
                    && friction_coefficient > 0.0
                    && (input.contact_surface != PickupContactSurface::GrooveWalls
                        || !active_wall_has_nonzero_slope(input, active_mask));
                if flat_sticking_requested
                    && bearing_sticks
                    && input.groove_tangential_velocity_m_s.abs()
                        > TANGENTIAL_VELOCITY_TOLERANCE_M_S
                {
                    continue;
                }
                let flat_sticking_branch = flat_sticking_requested && !bearing_sticks;
                let bearing_force_column = bearing_sticks.then_some(4 + active_count);
                let sticking_force_column =
                    flat_sticking_branch.then_some(4 + active_count + usize::from(bearing_sticks));
                let variable_count = 4
                    + active_count
                    + usize::from(bearing_sticks)
                    + usize::from(flat_sticking_branch);
                let mut augmented = [[0.0_f64; 9]; 8];
                let axes = [tonearm.lateral, tonearm.vertical];
                for axis in 0..2 {
                    let tip_row = axis * 2;
                    let body_row = tip_row + 1;
                    let tip_velocity_column = axis;
                    let body_velocity_column = 2 + axis;
                    let stiffness = axes[axis].stiffness_n_per_m();
                    let damping = axes[axis].viscous_damping_n_s_per_m();
                    let coupling = stiffness * dt + damping;
                    let relative_displacement =
                        tip_displacement_m[axis] - body_displacement_m[axis];

                    augmented[tip_row][tip_velocity_column] =
                        contact.moving_mass_kg / dt + coupling;
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
                        let reciprocal_damping = electromagnetic_relation
                            .reciprocal_damping_n_s_per_m[axis][velocity_axis];
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
                        if let Some(bearing_force_column) = bearing_force_column {
                            augmented[body_row][bearing_force_column] = -1.0;
                        }
                        if let Some(sticking_force_column) = sticking_force_column {
                            augmented[body_row][sticking_force_column] = skating_factor;
                        }
                    }

                    if axis == 1 && !input.stylus_lowered {
                        let cue_coupling = tonearm.cue_support_stiffness_n_per_m * dt
                            + tonearm.cue_support_damping_n_s_per_m;
                        augmented[body_row][body_velocity_column] += cue_coupling;
                        augmented[body_row][variable_count] += tonearm
                            .cue_support_stiffness_n_per_m
                            * (tonearm.cue_lift_height_m - body_displacement_m[1]);
                    }

                    for (active_index, &constraint) in
                        active_constraints[..active_count].iter().enumerate()
                    {
                        let lambda_column = 4 + active_index;
                        augmented[tip_row][lambda_column] = -wall_force_scale_per_normal
                            [constraint]
                            * constraint_normals[constraint][axis];
                        if axis == 0 {
                            augmented[body_row][lambda_column] =
                                -skating_factor * tangential_force_per_normal[constraint];
                        }
                    }
                }

                for (active_index, &constraint) in
                    active_constraints[..active_count].iter().enumerate()
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
                if sticking_force_column.is_some() {
                    let row = 4 + active_count + usize::from(bearing_sticks);
                    augmented[row][2] = skating_factor;
                    augmented[row][variable_count] = input.groove_tangential_velocity_m_s;
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
                for (active_index, &constraint) in
                    active_constraints[..active_count].iter().enumerate()
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
                            - tonearm.lateral_bearing_viscous_damping_n_s_per_m
                                * next_body_velocity[0]
                    }
                    BearingMode::Negative => {
                        valid &= next_body_velocity[0] <= BEARING_VELOCITY_TOLERANCE_M_S;
                        tonearm.lateral_bearing_kinetic_friction_n
                            - tonearm.lateral_bearing_viscous_damping_n_s_per_m
                                * next_body_velocity[0]
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
                    let (wall_projected_force_n, land_normal_force_n) = match input.contact_surface
                    {
                        PickupContactSurface::GrooveWalls if input.stylus_lowered => {
                            (constraint_force_n, 0.0)
                        }
                        PickupContactSurface::RecordLand if input.stylus_lowered => {
                            ([0.0; 2], constraint_force_n[0])
                        }
                        _ => ([0.0; 2], 0.0),
                    };
                    let friction_normal_force_n = match input.contact_surface {
                        PickupContactSurface::GrooveWalls => {
                            wall_projected_force_n[0] + wall_projected_force_n[1]
                        }
                        PickupContactSurface::RecordLand => land_normal_force_n,
                        PickupContactSurface::None => 0.0,
                    };
                    let sticking_friction_force_n =
                        sticking_force_column.map_or(0.0, |column| solution[column]);
                    if sticking_friction_force_n.abs()
                        > friction_coefficient * friction_normal_force_n
                            + TANGENTIAL_FORCE_TOLERANCE_N
                    {
                        continue;
                    }
                    let Some(tangential_mode) = standalone_tangential_mode(
                        input,
                        wall_projected_force_n,
                        land_normal_force_n,
                        next_tip_velocity,
                        next_body_velocity,
                        skating_factor,
                    ) else {
                        continue;
                    };
                    if !sliding_direction_matches_mode(sliding_direction, tangential_mode) {
                        continue;
                    }
                    if tangential_mode == StylusTangentialMode::Separated
                        && sticking_friction_force_n.abs() > TANGENTIAL_FORCE_TOLERANCE_N
                    {
                        continue;
                    }
                    return Ok(PickupStepSolution {
                        tip_velocity_m_s: next_tip_velocity,
                        body_velocity_m_s: next_body_velocity,
                        wall_projected_force_n,
                        wall_contact: wall_projected_force_n.map(|force| force > 0.0),
                        land_normal_force_n,
                        bearing_friction_force_n,
                        tangential_mode,
                        sticking_friction_force_n,
                    });
                }
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

fn solve_linear_system(augmented: &mut [[f64; 9]; 8], size: usize) -> Option<[f64; 8]> {
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
    let mut solution = [0.0; 8];
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

fn validate_friction_geometry(
    contact: StylusContactConfig,
    input: PickupMechanicalInput,
) -> Result<(), PickupMechanicalError> {
    if input.stylus_lowered
        && input.contact_surface == PickupContactSurface::GrooveWalls
        && input.wall_contacts.into_iter().any(|set| {
            set.contacts[..wall_contact_count(set).unwrap_or(1)]
                .iter()
                .any(|wall_contact| {
                    !groove_friction_geometry_is_well_conditioned(
                        contact.groove_friction_coefficient,
                        wall_contact.groove_slope.abs(),
                    )
                })
        })
    {
        Err(PickupMechanicalError::IllConditionedGrooveWallFrictionGeometry)
    } else {
        Ok(())
    }
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
            telemetry.coulomb_wall_force_on_tip_n[contact_index],
            telemetry.coulomb_friction_force_n[contact_index],
            telemetry.record_reaction_force_tangent_n[contact_index],
        ];
        if values.into_iter().any(|value| !value.is_finite()) {
            return false;
        }
        if contact_index >= count {
            if values != [0.0; 6] {
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
                telemetry.coulomb_wall_force_on_tip_n[contact_index],
                telemetry.coulomb_friction_force_n[contact_index] * contact.groove_slope,
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

fn snapshot_tangential_state_is_valid(
    contact: StylusContactConfig,
    telemetry: PickupMechanicalTelemetry,
) -> bool {
    let mode_matches_velocity = |velocity_m_s: f64| match telemetry.tangential_mode {
        StylusTangentialMode::SlidingPositive => velocity_m_s > TANGENTIAL_VELOCITY_TOLERANCE_M_S,
        StylusTangentialMode::SlidingNegative => velocity_m_s < -TANGENTIAL_VELOCITY_TOLERANCE_M_S,
        StylusTangentialMode::Sticking => velocity_m_s.abs() <= TANGENTIAL_VELOCITY_TOLERANCE_M_S,
        StylusTangentialMode::Separated => true,
    };
    match telemetry.contact_surface {
        PickupContactSurface::None => {
            telemetry.tangential_mode == StylusTangentialMode::Separated
                && telemetry.coulomb_friction_force_n == 0.0
        }
        PickupContactSurface::RecordLand => {
            let normal_force_n = telemetry.land_normal_force_n;
            let friction_limit_n = contact.record_surface_friction_coefficient * normal_force_n;
            match telemetry.tangential_mode {
                StylusTangentialMode::SlidingPositive => {
                    normal_force_n > TANGENTIAL_FORCE_TOLERANCE_N
                        && mode_matches_velocity(telemetry.tangential_relative_velocity_m_s)
                        && nearly_equal_force(telemetry.coulomb_friction_force_n, -friction_limit_n)
                }
                StylusTangentialMode::SlidingNegative => {
                    normal_force_n > TANGENTIAL_FORCE_TOLERANCE_N
                        && mode_matches_velocity(telemetry.tangential_relative_velocity_m_s)
                        && nearly_equal_force(telemetry.coulomb_friction_force_n, friction_limit_n)
                }
                StylusTangentialMode::Sticking => {
                    normal_force_n > TANGENTIAL_FORCE_TOLERANCE_N
                        && mode_matches_velocity(telemetry.tangential_relative_velocity_m_s)
                        && telemetry.coulomb_friction_force_n.abs()
                            <= friction_limit_n + TANGENTIAL_FORCE_TOLERANCE_N
                }
                StylusTangentialMode::Separated => {
                    (normal_force_n <= TANGENTIAL_FORCE_TOLERANCE_N
                        || contact.record_surface_friction_coefficient == 0.0)
                        && telemetry.coulomb_friction_force_n == 0.0
                }
            }
        }
        PickupContactSurface::GrooveWalls => {
            let wall_velocity_m_s = wall_coordinate_velocity_m_s(telemetry.tip_velocity_m_s);
            let mut loaded_contact_count = 0;
            for (wall_velocity_m_s, wall_telemetry) in wall_velocity_m_s
                .into_iter()
                .zip(telemetry.wall_longitudinal_contact)
            {
                for contact_index in 0..wall_telemetry.contact_count() {
                    let projected_force_n = wall_telemetry.projected_normal_force_n[contact_index];
                    let friction_force_n = wall_telemetry.coulomb_friction_force_n[contact_index];
                    if projected_force_n <= TANGENTIAL_FORCE_TOLERANCE_N {
                        if friction_force_n.abs() > TANGENTIAL_FORCE_TOLERANCE_N {
                            return false;
                        }
                        continue;
                    }
                    loaded_contact_count += 1;
                    let slope = wall_telemetry.geometry.contacts[contact_index].groove_slope;
                    let gamma_dot_m_s =
                        telemetry.tangential_relative_velocity_m_s + slope * wall_velocity_m_s;
                    let friction_limit_n = contact.groove_friction_coefficient * projected_force_n;
                    let valid_contact = match telemetry.tangential_mode {
                        StylusTangentialMode::SlidingPositive => {
                            mode_matches_velocity(gamma_dot_m_s)
                                && nearly_equal_force(friction_force_n, -friction_limit_n)
                        }
                        StylusTangentialMode::SlidingNegative => {
                            mode_matches_velocity(gamma_dot_m_s)
                                && nearly_equal_force(friction_force_n, friction_limit_n)
                        }
                        StylusTangentialMode::Sticking => {
                            mode_matches_velocity(gamma_dot_m_s)
                                && (contact.groove_friction_coefficient == 0.0 || slope == 0.0)
                                && friction_force_n.abs()
                                    <= friction_limit_n + TANGENTIAL_FORCE_TOLERANCE_N
                        }
                        StylusTangentialMode::Separated => {
                            contact.groove_friction_coefficient == 0.0 && friction_force_n == 0.0
                        }
                    };
                    if !valid_contact {
                        return false;
                    }
                }
            }
            match telemetry.tangential_mode {
                StylusTangentialMode::Separated => {
                    (loaded_contact_count == 0 || contact.groove_friction_coefficient == 0.0)
                        && telemetry.coulomb_friction_force_n == 0.0
                }
                _ => loaded_contact_count > 0,
            }
        }
    }
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
    let tangential_state_is_valid = snapshot_tangential_state_is_valid(snapshot.contact, telemetry);
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
    let expected_tangential_friction_power_w =
        if telemetry.contact_surface == PickupContactSurface::GrooveWalls {
            wall_coulomb_friction_power_w(
                telemetry.wall_longitudinal_contact,
                telemetry.tangential_relative_velocity_m_s,
                wall_coordinate_velocity_m_s(telemetry.tip_velocity_m_s),
            )
        } else {
            telemetry.coulomb_friction_force_n * telemetry.tangential_relative_velocity_m_s
        };
    let wall_coordinate_force_on_tip_n = telemetry.wall_longitudinal_contact.map(|wall| {
        wall.total_projected_normal_force_n() + wall.total_coulomb_wall_force_on_tip_n()
    });
    let expected_groove_lateral_force_on_tip_n =
        (wall_coordinate_force_on_tip_n[0] - wall_coordinate_force_on_tip_n[1]) * INVERSE_SQRT_2;
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
        || !tangential_state_is_valid
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
        || !nearly_equal_force(
            telemetry.tangential_friction_power_w,
            expected_tangential_friction_power_w,
        )
        || !nearly_equal_force(
            telemetry.groove_lateral_force_on_tip_n,
            expected_groove_lateral_force_on_tip_n,
        )
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
    #[error("nonzero-slope groove-wall sticking needs a resolved tangential contact model")]
    UnsupportedGrooveWallSticking,
    #[error("groove-wall sliding would direct a local Coulomb force out of the wall")]
    IllConditionedGrooveWallFrictionGeometry,
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
                ..super::super::stylus::StylusTraceContact::default()
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

    fn fixed_mechanical_mode(
        deck_bearing: FixedModeFrictionMobility,
        slipmat: FixedModeFrictionMobility,
        hand: FixedModeHandMobility,
        pickup_bearing: FixedModeFrictionMobility,
    ) -> CoupledFixedMechanicalMode {
        coupled_fixed_mechanical_modes()
            .find(|mode| {
                mode.deck_bearing == deck_bearing
                    && mode.slipmat == slipmat
                    && mode.hand == hand
                    && mode.pickup_bearing == pickup_bearing
            })
            .unwrap()
    }

    fn fixed_contact_family(
        mechanical: CoupledFixedMechanicalMode,
        surface: CoupledFixedContactSurface,
        origin_law: CoupledFixedOriginLaw,
        stylus: StylusTangentialMode,
    ) -> CoupledFixedContactFamily {
        coupled_fixed_contact_families()
            .find(|family| {
                family.mechanical == mechanical
                    && family.surface == surface
                    && family.origin_law == origin_law
                    && family.stylus == stylus
            })
            .unwrap()
    }

    fn seed_playback_config() -> super::super::PhysicalPlaybackConfig {
        super::super::PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed().config
    }

    fn fixed_response_matrix(response: CoupledFixedModeNormalResponse) -> [[f64; 2]; 2] {
        match response.normal_response {
            CoupledFixedNormalResponse::GrooveWalls { w } => w,
            CoupledFixedNormalResponse::RecordLand { w } => [[w, 0.0], [0.0; 2]],
        }
    }

    fn assert_operator_vector_close(
        actual: [f64; JOINT_DYNAMIC_VARIABLES],
        expected: [f64; JOINT_DYNAMIC_VARIABLES],
    ) {
        for (index, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
            let tolerance = 16.0 * f64::EPSILON * actual.abs().max(expected.abs()).max(1.0);
            assert!(
                (actual - expected).abs() <= tolerance,
                "operator coefficient {index}: actual={actual:e}, expected={expected:e}"
            );
        }
    }

    fn probe_coupled_normal_response(
        deck: DeckMidpointPreparation,
        pickup: PickupMechanicalState,
        input: PickupMechanicalInput,
        relation: PickupElectromagneticForceRelation,
        operator: CoupledContactHgOperator,
    ) -> [[f64; 2]; 2] {
        let mut mobility = [[0.0; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES];
        assemble_deck_dynamics(
            &mut mobility,
            deck,
            CoupledDeckFrictionMode::SlidingPositive,
            CoupledDeckFrictionMode::SlidingPositive,
            CoupledDeckFrictionMode::Separated,
            None,
            None,
            None,
        )
        .unwrap();
        assemble_pickup_dynamics(
            &mut mobility,
            pickup,
            input,
            relation,
            BearingMode::Stick,
            Some(JOINT_DYNAMIC_VARIABLES),
        );
        mobility[JOINT_DYNAMIC_VARIABLES][4] = 1.0;
        for row in &mut mobility {
            row[JOINT_RHS_COLUMN] = 0.0;
        }

        let mut response = [[0.0; 2]; 2];
        for source in 0..operator.constraint_count() {
            let mut augmented = mobility;
            operator.normal_force_rhs(source).write_rhs(&mut augmented);
            let velocity =
                solve_joint_linear_system(&mut augmented, JOINT_DYNAMIC_VARIABLES + 1).unwrap();
            let velocity: [f64; JOINT_DYNAMIC_VARIABLES] =
                velocity[..JOINT_DYNAMIC_VARIABLES].try_into().unwrap();
            for (target, target_response) in response
                .iter_mut()
                .enumerate()
                .take(operator.constraint_count())
            {
                target_response[source] = operator
                    .normal_gap_velocity(target)
                    .response_for_velocity(velocity);
            }
        }
        response
    }

    fn explicit_hand_stylus_sticking_branch(
        previous_record_velocity_rad_s: f64,
        hand_velocity_rad_s: f64,
        pickup_bearing_mode: BearingMode,
    ) -> (JointCandidate, DeckMidpointPreparation, f64) {
        let sample_rate_hz = 192_000.0;
        let dt = 1.0 / sample_rate_hz;
        let deck_config = crate::PhysicalDeckConfig::default();
        let mut deck = DeckMechanicalState::new(deck_config).unwrap();
        deck.reset(
            1.0,
            previous_record_velocity_rad_s / deck_config.nominal_angular_velocity_rad_s(),
            0.0,
            0.0,
        )
        .unwrap();
        let preparation = deck
            .prepare_midpoint_step(
                dt,
                crate::DeckMechanicalControl {
                    hand_contact: true,
                    hand_target_angular_velocity_rad_s: hand_velocity_rad_s,
                    hand_normal_force_n: 100.0,
                    hand_contact_radius_m: 0.20,
                    ..crate::DeckMechanicalControl::default()
                },
            )
            .unwrap();
        let pickup = PickupMechanicalState::new(
            StylusContactConfig {
                record_surface_friction_coefficient: 2.0,
                ..StylusContactConfig::default()
            },
            TonearmConfig::default(),
            sample_rate_hz,
        )
        .unwrap();
        let input = PickupMechanicalInput {
            land_displacement_m: 1.0e-8,
            contact_surface: PickupContactSurface::RecordLand,
            ..PickupMechanicalInput::default()
        };
        let geometry = MidpointPickupGeometry {
            input,
            lateral_origin_shift_bias_m: 0.0,
            lateral_origin_shift_per_record_velocity_m_s: 0.0,
        };
        let skating_factor = pickup
            .tonearm
            .geometry
            .equivalent_radial_force_n(input.groove_radius_m, 1.0)
            .unwrap();
        let mut attempted_linear_solves = 0;
        let candidate = solve_joint_branch(
            preparation,
            pickup,
            geometry,
            PickupElectromagneticForceRelation::constant([0.0; 2]).unwrap(),
            CoupledDeckFrictionMode::SlidingPositive,
            CoupledDeckFrictionMode::SlidingPositive,
            CoupledDeckFrictionMode::Sticking,
            pickup_bearing_mode,
            0b01,
            [0, 0],
            1,
            StylusTangentialMode::Sticking,
            skating_factor,
            &mut attempted_linear_solves,
        );
        assert_eq!(attempted_linear_solves, 1);
        (
            candidate.unwrap_or_else(|| {
                panic!(
                    "expected hand/stylus sticking branch: previous={previous_record_velocity_rad_s:e}, hand={hand_velocity_rad_s:e}, bearing={pickup_bearing_mode:?}"
                )
            }),
            preparation,
            skating_factor,
        )
    }

    #[test]
    fn fixed_mode_catalog_enumerates_24_mechanical_and_288_contacting_labels() {
        let mechanical = coupled_fixed_mechanical_modes().collect::<Vec<_>>();
        assert_eq!(mechanical.len(), COUPLED_FIXED_MECHANICAL_CLASS_COUNT);
        let mechanical_unique = mechanical
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(mechanical_unique.len(), mechanical.len());

        let families = coupled_fixed_contact_families().collect::<Vec<_>>();
        assert_eq!(families.len(), COUPLED_FIXED_CONTACT_FAMILY_COUNT);
        let unique = families
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), families.len());
        assert!(families.iter().all(|family| {
            family.family_set_version() == COUPLED_FIXED_MODE_FAMILY_SET_VERSION
        }));
        assert_eq!(
            families
                .iter()
                .filter(|family| family.surface == CoupledFixedContactSurface::GrooveWalls)
                .count(),
            192
        );
        assert_eq!(
            families
                .iter()
                .filter(|family| family.surface == CoupledFixedContactSurface::RecordLand)
                .count(),
            96
        );
        for stylus in FIXED_STYLUS_MODES {
            assert_eq!(
                families
                    .iter()
                    .filter(|family| family.stylus == stylus)
                    .count(),
                72
            );
        }
    }

    #[test]
    fn fixed_stylus_constraint_relation_matches_the_production_rank_selector() {
        let mut adds_rank = 0;
        let mut dependent = 0;
        let mut zero_coefficient_adds_rank = 0;
        let mut zero_coefficient_dependent = 0;
        for mechanical in coupled_fixed_mechanical_modes() {
            let expected_dependent = mechanical.pickup_bearing
                == FixedModeFrictionMobility::Sticking
                && (mechanical.hand == FixedModeHandMobility::Sticking
                    || (mechanical.deck_bearing == FixedModeFrictionMobility::Sticking
                        && mechanical.slipmat == FixedModeFrictionMobility::Sticking));
            let relation = fixed_stylus_constraint_relation(mechanical, -1.0).unwrap();
            if expected_dependent {
                assert_eq!(relation, FixedStylusConstraintRelation::DependentRuntimeRhs);
                dependent += 1;
            } else {
                assert_eq!(relation, FixedStylusConstraintRelation::AddsRank);
                adds_rank += 1;
            }

            let zero_coefficient_relation =
                fixed_stylus_constraint_relation(mechanical, 0.0).unwrap();
            let zero_coefficient_expected_dependent = mechanical.hand
                == FixedModeHandMobility::Sticking
                || (mechanical.deck_bearing == FixedModeFrictionMobility::Sticking
                    && mechanical.slipmat == FixedModeFrictionMobility::Sticking);
            if zero_coefficient_expected_dependent {
                assert_eq!(
                    zero_coefficient_relation,
                    FixedStylusConstraintRelation::DependentRuntimeRhs
                );
                zero_coefficient_dependent += 1;
            } else {
                assert_eq!(
                    zero_coefficient_relation,
                    FixedStylusConstraintRelation::AddsRank
                );
                zero_coefficient_adds_rank += 1;
            }
        }
        assert_eq!(adds_rank, 18);
        assert_eq!(dependent, 6);
        assert_eq!(zero_coefficient_adds_rank, 12);
        assert_eq!(zero_coefficient_dependent, 12);
    }

    #[test]
    fn sticking_operator_helpers_keep_typed_coordinates_and_signed_zero() {
        let force = stylus_sticking_force_column(2.0, 3.0);
        assert_eq!(
            JOINT_DYNAMIC_EQUATIONS.map(|equation| force.coefficient(equation)),
            [0.0, -2.0, 0.0, 3.0, 0.0, 0.0]
        );
        let equality = stylus_sticking_equality_row(4.0);
        assert_eq!(
            JOINT_DYNAMIC_VELOCITIES.map(|velocity| equality.coefficient(velocity)),
            [0.0, 1.0, 0.0, 0.0, 4.0, 0.0]
        );
        assert_eq!(
            stylus_sticking_equality_row(-0.0)
                .coefficient(JointDynamicVelocity::BodyX)
                .to_bits(),
            (-0.0_f64).to_bits()
        );
    }

    #[test]
    fn typed_joint_coordinates_keep_equation_and_velocity_orders_distinct() {
        let rhs = JointDynamicEquationRhs {
            platter: 1.0,
            record: 2.0,
            tip_x: 3.0,
            body_x: 4.0,
            tip_z: 5.0,
            body_z: 6.0,
        };
        assert_eq!(
            JOINT_DYNAMIC_EQUATIONS.map(|equation| rhs.coefficient(equation)),
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
        );

        let velocity_row = JointDynamicVelocityRow {
            platter: 10.0,
            record: 20.0,
            tip_x: 30.0,
            tip_z: 40.0,
            body_x: 50.0,
            body_z: 60.0,
        };
        assert_eq!(
            JOINT_DYNAMIC_VELOCITIES.map(|velocity| velocity_row.coefficient(velocity)),
            [10.0, 20.0, 30.0, 40.0, 50.0, 60.0]
        );

        let mut mobility = CoupledFixedDynamicMobility {
            velocity_by_equation_rhs: [[0.0; JOINT_DYNAMIC_VARIABLES]; JOINT_DYNAMIC_VARIABLES],
        };
        mobility.velocity_by_equation_rhs[JointDynamicVelocity::TipZ as usize]
            [JointDynamicEquation::TipZ as usize] = 34.0;
        mobility.velocity_by_equation_rhs[JointDynamicVelocity::BodyX as usize]
            [JointDynamicEquation::BodyX as usize] = 43.0;
        assert_eq!(
            mobility.coefficient(JointDynamicVelocity::TipZ, JointDynamicEquation::TipZ),
            34.0
        );
        assert_eq!(
            mobility.coefficient(JointDynamicVelocity::BodyX, JointDynamicEquation::BodyX),
            43.0
        );
        assert_eq!(
            mobility.coefficient(JointDynamicVelocity::TipZ, JointDynamicEquation::BodyX),
            0.0
        );
        assert_eq!(
            mobility.coefficient(JointDynamicVelocity::BodyX, JointDynamicEquation::TipZ),
            0.0
        );

        let mut kkt = CoupledFixedModeKktLhs {
            system_size: JOINT_DYNAMIC_VARIABLES,
            equality_count: 0,
            coefficients: [[0.0; JOINT_MAX_VARIABLES]; JOINT_MAX_VARIABLES],
            equality_basis: [JointDynamicVelocityRow::default(); 5],
        };
        kkt.coefficients[JointDynamicEquation::TipZ as usize]
            [JointDynamicVelocity::TipZ as usize] = 53.0;
        kkt.coefficients[JointDynamicEquation::BodyX as usize]
            [JointDynamicVelocity::BodyX as usize] = 45.0;
        assert_eq!(
            kkt.dynamic_coefficient(JointDynamicEquation::TipZ, JointDynamicVelocity::TipZ),
            53.0
        );
        assert_eq!(
            kkt.dynamic_coefficient(JointDynamicEquation::BodyX, JointDynamicVelocity::BodyX),
            45.0
        );
        assert_eq!(
            kkt.dynamic_coefficient(JointDynamicEquation::TipZ, JointDynamicVelocity::BodyX),
            0.0
        );
        assert_eq!(
            kkt.dynamic_coefficient(JointDynamicEquation::BodyX, JointDynamicVelocity::TipZ),
            0.0
        );
    }

    #[test]
    fn solve_only_subject_catalog_has_stable_versioned_order() {
        let subjects: Vec<_> = coupled_fixed_solve_only_subjects().collect();
        let mechanical: Vec<_> = coupled_fixed_mechanical_modes().collect();
        assert_eq!(subjects.len(), COUPLED_FIXED_SOLVE_ONLY_SUBJECT_COUNT);
        assert_eq!(mechanical.len(), COUPLED_FIXED_MECHANICAL_CLASS_COUNT);
        for (index, subject) in subjects.into_iter().enumerate() {
            assert_eq!(
                subject.subject_set_version(),
                COUPLED_FIXED_SOLVE_ONLY_SUBJECT_SET_VERSION
            );
            assert_eq!(subject.mechanical, mechanical[index % mechanical.len()]);
            assert_eq!(
                subject.support,
                if index < mechanical.len() {
                    CoupledFixedPickupSupport::LoweredNoContact
                } else {
                    CoupledFixedPickupSupport::CueSupported
                }
            );
        }
    }

    #[test]
    fn all_lowered_solve_only_systems_match_the_separated_land_base_exactly() {
        let config = seed_playback_config();
        let point = CoupledFixedContactPoint {
            groove_radius_m: config.groove.outer_program_radius_m,
            wall_slopes: [0.0; 2],
        };
        for subject in coupled_fixed_solve_only_subjects()
            .filter(|subject| subject.support == CoupledFixedPickupSupport::LoweredNoContact)
        {
            let family = fixed_contact_family(
                subject.mechanical,
                CoupledFixedContactSurface::RecordLand,
                CoupledFixedOriginLaw::SurfaceIndependent,
                StylusTangentialMode::Separated,
            );
            let base = coupled_fixed_mode_normal_response(config, family, point).unwrap();
            let solve_only = coupled_fixed_solve_only_response(config, subject).unwrap();
            assert_eq!(solve_only.kkt_lhs, base.kkt_lhs, "{subject:?}");
            assert_eq!(solve_only.equality, base.equality, "{subject:?}");
            assert_eq!(
                solve_only.dynamic_mobility, base.dynamic_mobility,
                "{subject:?}"
            );
            assert_eq!(solve_only.solve.system_size, base.solve.system_size);
            assert_eq!(
                solve_only.solve.minimum_scaled_pivot.to_bits(),
                base.solve.minimum_scaled_pivot.to_bits()
            );
            assert_eq!(
                solve_only.solve.maximum_scaled_pivot.to_bits(),
                base.solve.maximum_scaled_pivot.to_bits()
            );
            assert_eq!(
                solve_only.solve.scaled_pivot_ratio.to_bits(),
                base.solve.scaled_pivot_ratio.to_bits()
            );
            assert!(
                solve_only.solve.maximum_backward_error >= base.solve.maximum_backward_error,
                "{subject:?}"
            );
        }
    }

    #[test]
    fn all_solve_only_complete_inverses_replay_with_typed_active_layout() {
        let config = seed_playback_config();
        for subject in coupled_fixed_solve_only_subjects() {
            let response = coupled_fixed_solve_only_response(config, subject).unwrap();
            let inverse = response.kkt_inverse;
            assert_eq!(inverse.system_size, response.kkt_lhs.system_size);
            assert_eq!(inverse.equality_count, response.kkt_lhs.equality_count);
            assert!(inverse.has_valid_inactive_storage());
            assert_eq!(inverse.dynamic_mobility(), response.dynamic_mobility);

            for rhs_index in 0..inverse.system_size {
                let rhs_coordinate =
                    JointKktRhsCoordinate::from_active_index(rhs_index, inverse.equality_count)
                        .unwrap();
                let mut replay = [[0.0; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES];
                for (row, coefficients) in response
                    .kkt_lhs
                    .coefficients
                    .iter()
                    .enumerate()
                    .take(inverse.system_size)
                {
                    replay[row][..inverse.system_size]
                        .copy_from_slice(&coefficients[..inverse.system_size]);
                }
                replay[rhs_index][JOINT_RHS_COLUMN] = 1.0;
                let solved =
                    solve_joint_linear_system_with_diagnostics(&mut replay, inverse.system_size)
                        .unwrap();
                for solution_index in 0..inverse.system_size {
                    let solution_coordinate = JointKktSolutionCoordinate::from_active_index(
                        solution_index,
                        inverse.equality_count,
                    )
                    .unwrap();
                    let stored = inverse
                        .coefficient(solution_coordinate, rhs_coordinate)
                        .unwrap();
                    assert_eq!(
                        stored.to_bits(),
                        solved.solution[solution_index].to_bits(),
                        "{subject:?}, solution={solution_index}, rhs={rhs_index}"
                    );
                    assert_eq!(
                        stored.to_bits(),
                        inverse.solution_by_rhs[solution_index][rhs_index].to_bits()
                    );
                }
            }

            for row in 0..JOINT_MAX_VARIABLES {
                for column in 0..JOINT_MAX_VARIABLES {
                    if row >= inverse.system_size || column >= inverse.system_size {
                        assert_eq!(
                            inverse.solution_by_rhs[row][column].to_bits(),
                            0.0_f64.to_bits(),
                            "{subject:?}, inverse [{row}][{column}]"
                        );
                        assert_eq!(
                            response.kkt_lhs.coefficients[row][column].to_bits(),
                            0.0_f64.to_bits(),
                            "{subject:?}, KKT [{row}][{column}]"
                        );
                    }
                }
            }
            assert!(inverse
                .coefficient(
                    JointKktSolutionCoordinate::EqualityMultiplier(inverse.equality_count),
                    JointKktRhsCoordinate::DynamicEquation(JointDynamicEquation::Platter),
                )
                .is_none());
            assert!(inverse
                .coefficient(
                    JointKktSolutionCoordinate::DynamicVelocity(JointDynamicVelocity::Platter),
                    JointKktRhsCoordinate::EqualityConstraint(inverse.equality_count),
                )
                .is_none());
        }
    }

    #[test]
    fn cue_support_changes_only_the_typed_vertical_body_coefficient() {
        let config = seed_playback_config();
        let dt = 1.0 / config.solver.internal_sample_rate_hz;
        let cue_coupling = config.tonearm.cue_support_stiffness_n_per_m * dt
            + config.tonearm.cue_support_damping_n_s_per_m;
        let subjects: Vec<_> = coupled_fixed_solve_only_subjects().collect();
        for index in 0..COUPLED_FIXED_MECHANICAL_CLASS_COUNT {
            let lowered = coupled_fixed_solve_only_response(config, subjects[index]).unwrap();
            let cue = coupled_fixed_solve_only_response(
                config,
                subjects[index + COUPLED_FIXED_MECHANICAL_CLASS_COUNT],
            )
            .unwrap();
            assert_eq!(lowered.equality, cue.equality);
            assert_eq!(lowered.kkt_lhs.system_size, cue.kkt_lhs.system_size);
            assert_eq!(lowered.kkt_lhs.equality_basis, cue.kkt_lhs.equality_basis);
            for row in 0..lowered.kkt_lhs.system_size {
                for column in 0..lowered.kkt_lhs.system_size {
                    let lowered_value = lowered.kkt_lhs.coefficients[row][column];
                    let cue_value = cue.kkt_lhs.coefficients[row][column];
                    if row == JointDynamicEquation::BodyZ as usize
                        && column == JointDynamicVelocity::BodyZ as usize
                    {
                        assert_eq!(
                            cue_value.to_bits(),
                            (lowered_value + cue_coupling).to_bits()
                        );
                        assert_eq!(
                            cue.kkt_lhs.dynamic_coefficient(
                                JointDynamicEquation::BodyZ,
                                JointDynamicVelocity::BodyZ,
                            ),
                            cue_value
                        );
                    } else {
                        assert_eq!(cue_value.to_bits(), lowered_value.to_bits());
                    }
                }
            }
        }
    }

    #[test]
    fn no_contact_builder_preserves_dependent_runtime_rhs_rejection() {
        let config = seed_playback_config();
        let dt = 1.0 / config.solver.internal_sample_rate_hz;
        let reciprocal_damping_n_s_per_m =
            super::super::electromechanical::cartridge_mechanical_reciprocal_damping_n_s_per_m(
                config.cartridge,
                dt,
            )
            .unwrap();
        let mechanical = fixed_mechanical_mode(
            FixedModeFrictionMobility::Sticking,
            FixedModeFrictionMobility::Sticking,
            FixedModeHandMobility::Sticking,
            FixedModeFrictionMobility::Sticking,
        );
        let subject = coupled_fixed_solve_only_subjects()
            .find(|subject| {
                subject.mechanical == mechanical
                    && subject.support == CoupledFixedPickupSupport::LoweredNoContact
            })
            .unwrap();
        let response = coupled_fixed_solve_only_response(config, subject).unwrap();
        assert!(response.equality.runtime_rhs_compatibility_required);

        let mut compatible = [[0.0; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES];
        assert!(assemble_joint_no_contact_kkt_lhs(
            &mut compatible,
            config.deck,
            config.contact,
            config.tonearm,
            dt,
            dt,
            reciprocal_damping_n_s_per_m,
            mechanical.deck_bearing_mode(),
            mechanical.slipmat_mode(),
            mechanical.hand_mode(),
            mechanical.pickup_bearing_mode(),
            true,
            0.0,
        )
        .is_some());
        let mut incompatible = [[0.0; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES];
        assert!(assemble_joint_no_contact_kkt_lhs(
            &mut incompatible,
            config.deck,
            config.contact,
            config.tonearm,
            dt,
            dt,
            reciprocal_damping_n_s_per_m,
            mechanical.deck_bearing_mode(),
            mechanical.slipmat_mode(),
            mechanical.hand_mode(),
            mechanical.pickup_bearing_mode(),
            true,
            1.0,
        )
        .is_none());
    }

    #[test]
    fn all_288_fixed_mode_labels_build_finite_production_mobilities() {
        let config = seed_playback_config();
        let point = CoupledFixedContactPoint {
            groove_radius_m: config.groove.outer_program_radius_m,
            wall_slopes: [0.0; 2],
        };
        let mut evaluated = 0;
        for family in coupled_fixed_contact_families() {
            let response = coupled_fixed_mode_normal_response(config, family, point)
                .unwrap_or_else(|error| panic!("{family:?}: {error}"));
            evaluated += 1;
            assert_eq!(
                response.operator_version,
                COUPLED_FIXED_MODE_OPERATOR_VERSION
            );
            assert_eq!(response.family, family);
            assert_eq!(response.solve.system_size, response.kkt_lhs.system_size);
            assert_eq!(response.equality.rank, response.kkt_lhs.equality_count);
            assert_eq!(
                response.equality.dependent_count,
                response
                    .equality
                    .requested_count
                    .saturating_sub(response.equality.rank)
            );
            assert!(response.solve.minimum_scaled_pivot > 0.0);
            assert!(response.solve.maximum_scaled_pivot.is_finite());
            assert!(response.solve.scaled_pivot_ratio > 0.0);
            assert!(response.solve.maximum_backward_error <= 1.0e-10);
            assert!(response
                .dynamic_mobility
                .velocity_by_equation_rhs
                .into_iter()
                .flatten()
                .all(f64::is_finite));
            assert!(fixed_response_matrix(response)
                .into_iter()
                .flatten()
                .all(f64::is_finite));
            match family.stylus {
                StylusTangentialMode::Separated => assert_eq!(
                    response.reachability,
                    CoupledFixedModeReachability::NormalForceToleranceBandOnly
                ),
                _ => assert_eq!(
                    response.reachability,
                    CoupledFixedModeReachability::RuntimeConditional
                ),
            }
        }
        assert_eq!(evaluated, COUPLED_FIXED_CONTACT_FAMILY_COUNT);
    }

    #[test]
    fn fixed_mode_builder_reuses_the_production_lhs_and_hg_operators() {
        let config = seed_playback_config();
        let mechanical = fixed_mechanical_mode(
            FixedModeFrictionMobility::Sliding,
            FixedModeFrictionMobility::Sliding,
            FixedModeHandMobility::Separated,
            FixedModeFrictionMobility::Sticking,
        );
        let family = fixed_contact_family(
            mechanical,
            CoupledFixedContactSurface::GrooveWalls,
            CoupledFixedOriginLaw::InteriorSpiral,
            StylusTangentialMode::SlidingNegative,
        );
        let point = CoupledFixedContactPoint {
            groove_radius_m: config.groove.outer_program_radius_m,
            wall_slopes: [0.35, -0.20],
        };
        let response = coupled_fixed_mode_normal_response(config, family, point).unwrap();

        let dt = 1.0 / config.solver.internal_sample_rate_hz;
        let mut deck = DeckMechanicalState::new(config.deck).unwrap();
        deck.reset(1.0, 1.0, 0.0, 0.0).unwrap();
        let preparation = deck
            .prepare_midpoint_step(dt, crate::DeckMechanicalControl::default())
            .unwrap();
        let pickup = PickupMechanicalState::new(config.contact, config.tonearm, 1.0 / dt).unwrap();
        let cartridge = crate::physical::MovingMagnetCartridge::new(config.cartridge).unwrap();
        let affine = cartridge.prepare_affine_step(dt).unwrap();
        let relation = PickupElectromagneticForceRelation::new(
            crate::physical::electromechanical::transform_vector_from_coil(
                affine.reaction_force_bias_n(),
            ),
            crate::physical::electromechanical::transform_damping_from_coil(
                affine.reciprocal_damping_n_s_per_m(),
            ),
        )
        .unwrap();
        let input = PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts([0.0; 2], point.wall_slopes),
            groove_radius_m: point.groove_radius_m,
            ..PickupMechanicalInput::default()
        };
        let geometry = MidpointPickupGeometry {
            input,
            lateral_origin_shift_bias_m: 0.0,
            lateral_origin_shift_per_record_velocity_m_s: -config
                .record_cut
                .groove_pitch_m_per_revolution
                * 0.5
                * dt
                / std::f64::consts::TAU,
        };
        let skating_factor = config
            .tonearm
            .geometry
            .equivalent_radial_force_n(point.groove_radius_m, 1.0)
            .unwrap();
        let operator = coupled_contact_hg_operator(
            geometry,
            dt,
            config.contact.groove_friction_coefficient,
            family.stylus,
            skating_factor,
        );
        assert_eq!(response.contact_operator, operator);
        assert_eq!(response.derived.dt, dt);
        assert_eq!(
            response.derived.groove_pitch_m_per_revolution,
            config.record_cut.groove_pitch_m_per_revolution
        );
        assert_eq!(
            response.derived.friction_coefficient,
            config.contact.groove_friction_coefficient
        );
        assert_eq!(response.derived.skating_factor, skating_factor);
        assert_eq!(
            response.derived.reciprocal_cartridge_damping_n_s_per_m,
            relation.reciprocal_damping_n_s_per_m()
        );
        let expected =
            probe_coupled_normal_response(preparation, pickup, input, relation, operator);
        let actual = fixed_response_matrix(response);
        for row in 0..2 {
            for column in 0..2 {
                assert!(
                    (actual[row][column] - expected[row][column]).abs() < 1.0e-15,
                    "actual={actual:?}, expected={expected:?}"
                );
            }
        }

        let mut production_lhs = [[0.0_f64; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES];
        assemble_deck_dynamics(
            &mut production_lhs,
            preparation,
            CoupledDeckFrictionMode::SlidingPositive,
            CoupledDeckFrictionMode::SlidingPositive,
            CoupledDeckFrictionMode::Separated,
            None,
            None,
            None,
        )
        .unwrap();
        assemble_pickup_dynamics(
            &mut production_lhs,
            pickup,
            input,
            relation,
            BearingMode::Stick,
            Some(JOINT_DYNAMIC_VARIABLES),
        );
        production_lhs[JOINT_DYNAMIC_VARIABLES][4] = 1.0;
        for (row, (actual_row, expected_row)) in response
            .kkt_lhs
            .coefficients
            .iter()
            .zip(production_lhs.iter())
            .take(response.kkt_lhs.system_size)
            .enumerate()
        {
            for (column, (actual, expected)) in actual_row
                .iter()
                .zip(expected_row.iter())
                .take(response.kkt_lhs.system_size)
                .enumerate()
            {
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "coefficient [{row}][{column}]"
                );
            }
        }
    }

    #[test]
    fn fixed_mode_builder_confirms_the_negative_normal_minor() {
        let mut config = seed_playback_config();
        config.deck.record_inertia_kg_m2 = 1.0e-7;
        config.contact.moving_mass_kg = 1.0e-2;
        config.cartridge.generator_coefficient_v_s_per_m = 1.0e-12;
        config.cartridge.generator_coefficient_source =
            crate::physical::GeneratorCoefficientSource::UserSupplied;
        let mechanical = fixed_mechanical_mode(
            FixedModeFrictionMobility::Sliding,
            FixedModeFrictionMobility::Sliding,
            FixedModeHandMobility::Separated,
            FixedModeFrictionMobility::Sticking,
        );
        let family = fixed_contact_family(
            mechanical,
            CoupledFixedContactSurface::GrooveWalls,
            CoupledFixedOriginLaw::InteriorSpiral,
            StylusTangentialMode::SlidingPositive,
        );
        let response = coupled_fixed_mode_normal_response(
            config,
            family,
            CoupledFixedContactPoint {
                groove_radius_m: 0.146_05,
                wall_slopes: [-0.125; 2],
            },
        )
        .unwrap();
        let w = fixed_response_matrix(response);
        assert!(w[0][0] < 0.0, "W={w:?}");
        assert!(
            (w[0][0] + 0.007_329_826_622_361_105).abs() < 1.0e-12,
            "W={w:?}"
        );
    }

    #[test]
    fn fixed_mode_builder_keeps_the_origin_laws_distinct() {
        let config = seed_playback_config();
        let mechanical = fixed_mechanical_mode(
            FixedModeFrictionMobility::Sliding,
            FixedModeFrictionMobility::Sliding,
            FixedModeHandMobility::Separated,
            FixedModeFrictionMobility::Sliding,
        );
        let point = CoupledFixedContactPoint {
            groove_radius_m: config.groove.outer_program_radius_m,
            wall_slopes: [0.0; 2],
        };
        let interior = coupled_fixed_mode_normal_response(
            config,
            fixed_contact_family(
                mechanical,
                CoupledFixedContactSurface::GrooveWalls,
                CoupledFixedOriginLaw::InteriorSpiral,
                StylusTangentialMode::SlidingPositive,
            ),
            point,
        )
        .unwrap();
        let held = coupled_fixed_mode_normal_response(
            config,
            fixed_contact_family(
                mechanical,
                CoupledFixedContactSurface::GrooveWalls,
                CoupledFixedOriginLaw::HeldProgramBoundary,
                StylusTangentialMode::SlidingPositive,
            ),
            point,
        )
        .unwrap();
        assert_ne!(fixed_response_matrix(interior), fixed_response_matrix(held));
        assert_eq!(interior.kkt_lhs, held.kkt_lhs);
        assert_eq!(interior.dynamic_mobility, held.dynamic_mobility);
    }

    #[test]
    fn fixed_mode_reachability_separates_force_and_slope_conditions() {
        let config = seed_playback_config();
        let mechanical = fixed_mechanical_mode(
            FixedModeFrictionMobility::Sliding,
            FixedModeFrictionMobility::Sliding,
            FixedModeHandMobility::Separated,
            FixedModeFrictionMobility::Sliding,
        );
        let sloped = CoupledFixedContactPoint {
            groove_radius_m: config.groove.outer_program_radius_m,
            wall_slopes: [0.1, -0.1],
        };
        let interior_sticking = fixed_contact_family(
            mechanical,
            CoupledFixedContactSurface::GrooveWalls,
            CoupledFixedOriginLaw::InteriorSpiral,
            StylusTangentialMode::Sticking,
        );
        assert_eq!(
            coupled_fixed_mode_normal_response(config, interior_sticking, sloped)
                .unwrap()
                .reachability,
            CoupledFixedModeReachability::ActiveWallDependent {
                nonzero_active_wall:
                    CoupledFixedModeInfeasibility::SlopedGrooveStickingWithFriction,
                wall_is_nonzero: [true, true],
            }
        );
        let asymmetric = CoupledFixedContactPoint {
            groove_radius_m: sloped.groove_radius_m,
            wall_slopes: [0.1, 0.0],
        };
        assert_eq!(
            coupled_fixed_mode_normal_response(config, interior_sticking, asymmetric)
                .unwrap()
                .reachability,
            CoupledFixedModeReachability::ActiveWallDependent {
                nonzero_active_wall:
                    CoupledFixedModeInfeasibility::SlopedGrooveStickingWithFriction,
                wall_is_nonzero: [true, false],
            }
        );

        let mut frictionless = config;
        frictionless.contact.groove_friction_coefficient = 0.0;
        assert_eq!(
            coupled_fixed_mode_normal_response(frictionless, interior_sticking, sloped)
                .unwrap()
                .reachability,
            CoupledFixedModeReachability::RuntimeConditional
        );
        let held_sticking = fixed_contact_family(
            mechanical,
            CoupledFixedContactSurface::GrooveWalls,
            CoupledFixedOriginLaw::HeldProgramBoundary,
            StylusTangentialMode::Sticking,
        );
        assert_eq!(
            coupled_fixed_mode_normal_response(frictionless, held_sticking, sloped)
                .unwrap()
                .reachability,
            CoupledFixedModeReachability::ActiveWallDependent {
                nonzero_active_wall:
                    CoupledFixedModeInfeasibility::SlopedGrooveStickingAtHeldBoundary,
                wall_is_nonzero: [true, true],
            }
        );
        let separated = fixed_contact_family(
            mechanical,
            CoupledFixedContactSurface::GrooveWalls,
            CoupledFixedOriginLaw::InteriorSpiral,
            StylusTangentialMode::Separated,
        );
        assert_eq!(
            coupled_fixed_mode_normal_response(config, separated, sloped)
                .unwrap()
                .reachability,
            CoupledFixedModeReachability::NormalForceToleranceBandOnly
        );
        assert_eq!(
            coupled_fixed_mode_normal_response(frictionless, separated, sloped)
                .unwrap()
                .reachability,
            CoupledFixedModeReachability::RuntimeConditional
        );
    }

    #[test]
    fn fixed_mode_builder_reports_dependent_equalities_without_runtime_rhs() {
        let config = seed_playback_config();
        let mechanical = fixed_mechanical_mode(
            FixedModeFrictionMobility::Sticking,
            FixedModeFrictionMobility::Sticking,
            FixedModeHandMobility::Sticking,
            FixedModeFrictionMobility::Sticking,
        );
        let family = fixed_contact_family(
            mechanical,
            CoupledFixedContactSurface::RecordLand,
            CoupledFixedOriginLaw::SurfaceIndependent,
            StylusTangentialMode::Sticking,
        );
        let response = coupled_fixed_mode_normal_response(
            config,
            family,
            CoupledFixedContactPoint {
                groove_radius_m: config.groove.outer_program_radius_m,
                wall_slopes: [0.0; 2],
            },
        )
        .unwrap();
        assert_eq!(response.equality.requested_count, 5);
        assert!(response.equality.rank < response.equality.requested_count);
        assert_eq!(
            response.equality.dependent_count,
            response.equality.requested_count - response.equality.rank
        );
        assert!(response.equality.runtime_rhs_compatibility_required);
    }

    #[test]
    fn midpoint_contact_operator_matches_the_branch_equations() {
        let dt = 1.0 / 192_000.0;
        let skating_factor = 0.23;
        let friction_coefficient = 0.25;
        let input = PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts([0.0; 2], [0.35, -0.20]),
            ..PickupMechanicalInput::default()
        };
        let geometry = MidpointPickupGeometry {
            input,
            lateral_origin_shift_bias_m: 0.0,
            lateral_origin_shift_per_record_velocity_m_s: -1.5e-10,
        };
        let operator = coupled_contact_hg_operator(
            geometry,
            dt,
            friction_coefficient,
            StylusTangentialMode::SlidingNegative,
            skating_factor,
        );
        assert_eq!(operator.constraint_count(), 2);
        for (wall, slope) in [0.35, -0.20].into_iter().enumerate() {
            let normal = WALL_NORMALS[wall];
            let tangential_scale = slope - friction_coefficient;
            let wall_force_scale = 1.0 + friction_coefficient * slope;
            assert_operator_vector_close(
                operator
                    .normal_force_rhs(wall)
                    .coefficients_in_equation_row_order(),
                [
                    0.0,
                    -input.groove_radius_m * tangential_scale,
                    wall_force_scale * normal[0],
                    skating_factor * tangential_scale,
                    wall_force_scale * normal[1],
                    0.0,
                ],
            );
            assert_operator_vector_close(
                operator
                    .normal_gap_velocity(wall)
                    .coefficients_in_velocity_column_order(),
                [
                    0.0,
                    -normal[0] * geometry.lateral_origin_shift_per_record_velocity_m_s / dt
                        - 0.5 * slope * input.groove_radius_m,
                    normal[0],
                    normal[1],
                    slope * skating_factor,
                    0.0,
                ],
            );
        }

        let land_input = PickupMechanicalInput {
            contact_surface: PickupContactSurface::RecordLand,
            ..input
        };
        let land_operator = coupled_contact_hg_operator(
            MidpointPickupGeometry {
                input: land_input,
                ..geometry
            },
            dt,
            friction_coefficient,
            StylusTangentialMode::SlidingPositive,
            skating_factor,
        );
        assert_eq!(land_operator.constraint_count(), 1);
        assert_operator_vector_close(
            land_operator
                .normal_force_rhs(0)
                .coefficients_in_equation_row_order(),
            [
                0.0,
                -input.groove_radius_m * friction_coefficient,
                0.0,
                skating_factor * friction_coefficient,
                1.0,
                0.0,
            ],
        );
        assert_operator_vector_close(
            land_operator
                .normal_gap_velocity(0)
                .coefficients_in_velocity_column_order(),
            [0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        );

        let sticking_operator = coupled_contact_hg_operator(
            geometry,
            dt,
            friction_coefficient,
            StylusTangentialMode::Sticking,
            skating_factor,
        );
        for (wall, normal) in WALL_NORMALS.into_iter().enumerate() {
            assert_operator_vector_close(
                sticking_operator
                    .normal_force_rhs(wall)
                    .coefficients_in_equation_row_order(),
                [0.0, 0.0, normal[0], 0.0, normal[1], 0.0],
            );
        }
    }

    #[test]
    fn contact_operator_writers_preserve_the_signed_zero_layout() {
        let dt = 1.0 / 192_000.0;
        let input = PickupMechanicalInput {
            contact_surface: PickupContactSurface::RecordLand,
            ..PickupMechanicalInput::default()
        };
        let operator = coupled_contact_hg_operator(
            MidpointPickupGeometry {
                input,
                lateral_origin_shift_bias_m: 0.0,
                lateral_origin_shift_per_record_velocity_m_s: 0.0,
            },
            dt,
            StylusContactConfig::default().record_surface_friction_coefficient,
            StylusTangentialMode::Sticking,
            0.25,
        );
        let mut augmented = [[0.0; JOINT_MAX_VARIABLES + 1]; JOINT_MAX_VARIABLES];
        operator
            .normal_force_rhs(0)
            .write_contact_column(&mut augmented, JOINT_DYNAMIC_VARIABLES);
        let force_column_bits =
            [0, 1, 2, 3, 4, 5].map(|row| augmented[row][JOINT_DYNAMIC_VARIABLES].to_bits());
        assert_eq!(
            force_column_bits,
            [0.0, 0.0, -0.0, 0.0, -1.0, 0.0].map(f64::to_bits),
        );

        operator
            .normal_gap_velocity(0)
            .write_constraint_row(&mut augmented[JOINT_DYNAMIC_VARIABLES]);
        let gap_row: [f64; JOINT_DYNAMIC_VARIABLES] = augmented[JOINT_DYNAMIC_VARIABLES]
            [..JOINT_DYNAMIC_VARIABLES]
            .try_into()
            .unwrap();
        let gap_row_bits = gap_row.map(f64::to_bits);
        assert_eq!(
            gap_row_bits,
            [0.0, -0.0, 0.0, 1.0, 0.0, 0.0].map(f64::to_bits),
        );
    }

    #[test]
    fn admitted_midpoint_configuration_has_a_negative_normal_minor() {
        let sample_rate_hz = 192_000.0;
        let dt = 1.0 / sample_rate_hz;
        let deck_config = crate::PhysicalDeckConfig {
            record_inertia_kg_m2: 1.0e-7,
            ..crate::PhysicalDeckConfig::default()
        };
        let mut deck = DeckMechanicalState::new(deck_config).unwrap();
        deck.reset(1.0, 1.0, 0.0, 0.0).unwrap();
        let preparation = deck
            .prepare_midpoint_step(dt, crate::DeckMechanicalControl::default())
            .unwrap();

        let contact = StylusContactConfig {
            moving_mass_kg: 1.0e-2,
            ..StylusContactConfig::default()
        };
        let pickup =
            PickupMechanicalState::new(contact, TonearmConfig::default(), sample_rate_hz).unwrap();
        let input = PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts([0.0; 2], [-0.125; 2]),
            ..PickupMechanicalInput::default()
        };
        validate_input(input).unwrap();
        validate_friction_geometry(contact, input).unwrap();
        assert!(groove_friction_geometry_is_well_conditioned(
            contact.groove_friction_coefficient,
            0.125,
        ));

        let geometry = MidpointPickupGeometry {
            input,
            lateral_origin_shift_bias_m: 0.0,
            lateral_origin_shift_per_record_velocity_m_s: -125.0e-6 * 0.5 * dt
                / std::f64::consts::TAU,
        };
        let skating_factor = pickup
            .tonearm
            .geometry
            .equivalent_radial_force_n(input.groove_radius_m, 1.0)
            .unwrap();
        let operator = coupled_contact_hg_operator(
            geometry,
            dt,
            contact.groove_friction_coefficient,
            StylusTangentialMode::SlidingPositive,
            skating_factor,
        );
        let cartridge_config = crate::physical::MovingMagnetCartridgeConfig {
            generator_coefficient_v_s_per_m: 1.0e-12,
            generator_coefficient_source: crate::physical::GeneratorCoefficientSource::UserSupplied,
            ..crate::physical::MovingMagnetCartridgeConfig::default()
        };
        let cartridge = crate::physical::MovingMagnetCartridge::new(cartridge_config).unwrap();
        let affine = cartridge.prepare_affine_step(dt).unwrap();
        let mechanical_bias = crate::physical::electromechanical::transform_vector_from_coil(
            affine.reaction_force_bias_n(),
        );
        let mechanical_damping = crate::physical::electromechanical::transform_damping_from_coil(
            affine.reciprocal_damping_n_s_per_m(),
        );
        let relation =
            PickupElectromagneticForceRelation::new(mechanical_bias, mechanical_damping).unwrap();
        assert_eq!(relation.force_bias_n(), mechanical_bias);
        assert_eq!(relation.reciprocal_damping_n_s_per_m(), mechanical_damping,);
        assert!(mechanical_damping[0][0] > 0.0);
        assert!(mechanical_damping[1][1] > 0.0);
        assert!(
            mechanical_damping[0][0] * mechanical_damping[1][1]
                - mechanical_damping[0][1] * mechanical_damping[1][0]
                >= 0.0
        );
        let response =
            probe_coupled_normal_response(preparation, pickup, input, relation, operator);

        assert!(
            response[0][0] < 0.0,
            "expected a negative principal minor, W={response:?}"
        );
        assert!(
            (response[0][0] + 0.007_329_826_622_361_105).abs() < 1.0e-12,
            "unexpected witness response, W={response:?}"
        );
    }

    #[test]
    fn hand_and_stylus_sticking_keep_the_independent_body_constraint() {
        let selected = select_independent_deck_constraints(
            CoupledDeckFrictionMode::SlidingPositive,
            CoupledDeckFrictionMode::SlidingPositive,
            CoupledDeckFrictionMode::Sticking,
            StylusTangentialMode::Sticking,
            BearingMode::Positive,
            0.25,
            -0.50,
            -1.0,
        )
        .unwrap();
        assert_eq!(selected, [false, false, true, true]);

        assert_eq!(
            select_independent_deck_constraints(
                CoupledDeckFrictionMode::SlidingPositive,
                CoupledDeckFrictionMode::SlidingPositive,
                CoupledDeckFrictionMode::Sticking,
                StylusTangentialMode::Sticking,
                BearingMode::Stick,
                0.25,
                -0.50,
                -1.0,
            ),
            None,
        );
        assert_eq!(
            select_independent_deck_constraints(
                CoupledDeckFrictionMode::SlidingPositive,
                CoupledDeckFrictionMode::SlidingPositive,
                CoupledDeckFrictionMode::Sticking,
                StylusTangentialMode::Sticking,
                BearingMode::Positive,
                0.25,
                -0.50,
                0.0,
            ),
            None,
        );
    }

    #[test]
    fn joint_branch_enforces_independent_and_dependent_sticking_equalities() {
        let groove_radius_m = PickupMechanicalInput::default().groove_radius_m;
        let target_body_velocity_m_s = 1.0e-8;
        let skating_factor = TonearmConfig::default()
            .geometry
            .equivalent_radial_force_n(groove_radius_m, 1.0)
            .unwrap();
        let hand_velocity_rad_s = 2.0 * skating_factor * target_body_velocity_m_s / groove_radius_m;
        let (independent, independent_deck, _) =
            explicit_hand_stylus_sticking_branch(0.0, hand_velocity_rad_s, BearingMode::Positive);
        assert!(independent.pickup_solution.body_velocity_m_s[0] > 0.0);
        assert!(
            (independent.deck_solution.record_velocity_rad_s
                - independent_deck.hand_velocity_rad_s)
                .abs()
                < 1.0e-12
        );
        assert!(
            (0.5 * groove_radius_m
                * (independent_deck.previous_record_velocity_rad_s
                    + independent.deck_solution.record_velocity_rad_s)
                - skating_factor * independent.pickup_solution.body_velocity_m_s[0])
                .abs()
                < 1.0e-12
        );

        let (dependent, dependent_deck, dependent_skating_factor) =
            explicit_hand_stylus_sticking_branch(
                -hand_velocity_rad_s,
                hand_velocity_rad_s,
                BearingMode::Stick,
            );
        assert_eq!(dependent.pickup_solution.body_velocity_m_s[0], 0.0);
        assert!(
            (dependent.deck_solution.record_velocity_rad_s - dependent_deck.hand_velocity_rad_s)
                .abs()
                < 1.0e-12
        );
        assert!(
            (0.5 * groove_radius_m
                * (dependent_deck.previous_record_velocity_rad_s
                    + dependent.deck_solution.record_velocity_rad_s)
                - dependent_skating_factor * dependent.pickup_solution.body_velocity_m_s[0])
                .abs()
                < 1.0e-12
        );
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
        let distributed = distribute_wall_contact_forces(
            input,
            [2.0, 0.0],
            WallFrictionDistribution::Sliding {
                coefficient: 0.2,
                direction: 1.0,
            },
        );
        let wall = distributed[0];
        assert_eq!(wall.contact_count(), 2);
        assert_eq!(wall.projected_normal_force_n[..2], [1.0, 1.0]);
        assert_eq!(
            wall.surface_normal_force_n[0],
            wall.surface_normal_force_n[1]
        );
        assert_eq!(wall.coulomb_friction_force_n[..2], [-0.2, -0.2]);
        assert_eq!(wall.coulomb_wall_force_on_tip_n[..2], [-0.05, 0.05]);
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
        let distributed = distribute_wall_contact_forces(
            input,
            [2.0, 0.0],
            WallFrictionDistribution::Sliding {
                coefficient: 0.2,
                direction: 1.0,
            },
        );
        let wall = distributed[0];
        assert_eq!(wall.contact_count(), 1);
        assert_eq!(wall.projected_normal_force_n[0], 2.0);
        assert_eq!(wall.coulomb_friction_force_n[0], -0.4);
        assert_eq!(wall.coulomb_wall_force_on_tip_n[0], 0.1);
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
        let sequence = [left, bridge, right].map(|input| {
            distribute_wall_contact_forces(input, [1.25, 0.0], WallFrictionDistribution::None)[0]
        });
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
        let friction = WallFrictionDistribution::Sliding {
            coefficient: 0.175,
            direction: 1.0,
        };
        let distributed = distribute_wall_contact_forces(input, [1.2, 0.8], friction);
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
        assert!(
            wall_coulomb_friction_power_w(
                distributed,
                input.groove_tangential_velocity_m_s,
                wall_effective_slope(input)
                    .map(|slope| slope * input.groove_tangential_velocity_m_s),
            ) < friction_force_n * input.groove_tangential_velocity_m_s
        );
        let radius_m = 0.082_505_922_498_838_55;
        assert_eq!(
            reaction_force_n * radius_m,
            (modulation_force_n - 0.35) * radius_m
        );
        assert_no_alloc::assert_no_alloc(|| {
            let _ = distribute_wall_contact_forces(input, [1.2, 0.8], friction);
        });
    }

    #[test]
    fn zero_slope_preserves_the_scalar_coulomb_branch_exactly() {
        let input = PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts([0.0; 2], [0.0; 2]),
            groove_tangential_velocity_m_s: 0.5,
            ..PickupMechanicalInput::default()
        };
        let distributed = distribute_wall_contact_forces(
            input,
            [1.25, 0.75],
            WallFrictionDistribution::Sliding {
                coefficient: 0.25,
                direction: 1.0,
            },
        );
        assert_eq!(distributed[0].surface_normal_force_n[0], 1.25);
        assert_eq!(distributed[1].surface_normal_force_n[0], 0.75);
        assert_eq!(distributed[0].coulomb_friction_force_n[0], -0.3125);
        assert_eq!(distributed[1].coulomb_friction_force_n[0], -0.1875);
        assert_eq!(wall_coulomb_force_on_tip_n(distributed), [0.0, 0.0],);
        assert_eq!(
            wall_coulomb_friction_power_w(distributed, 0.5, [0.0; 2]),
            -0.25,
        );
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
    fn signed_wall_slopes_use_the_complete_coulomb_vector() {
        let mut state = state();
        for _ in 0..20_000 {
            state.process(PickupMechanicalInput::default()).unwrap();
        }
        let input = PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts([0.0; 2], [0.5, -0.5]),
            groove_tangential_velocity_m_s: 0.5,
            ..PickupMechanicalInput::default()
        };
        let telemetry = state.process(input).unwrap();
        let projected_force_n = telemetry
            .wall_longitudinal_contact
            .map(PickupWallContactTelemetry::total_projected_normal_force_n);
        let expected_coulomb = -state.contact.groove_friction_coefficient
            * (projected_force_n[0] + projected_force_n[1]);
        assert!((telemetry.coulomb_friction_force_n - expected_coulomb).abs() < 1.0e-12);

        let incorrect_scalar_coulomb = -state.contact.groove_friction_coefficient
            * (telemetry.wall_normal_force_n[0] + telemetry.wall_normal_force_n[1]);
        assert!(
            (telemetry.coulomb_friction_force_n - incorrect_scalar_coulomb).abs()
                > telemetry.coulomb_friction_force_n.abs() * 0.1
        );
        let expected_modulation = -(projected_force_n[0] * 0.5 - projected_force_n[1] * 0.5);
        assert!((telemetry.modulation_reaction_force_n - expected_modulation).abs() < 1.0e-12);
        for wall in 0..2 {
            let contact = telemetry.wall_longitudinal_contact[wall];
            assert!(nearly_equal_force(
                contact.coulomb_wall_force_on_tip_n[0],
                contact.coulomb_friction_force_n[0]
                    * input.wall_contacts[wall].contacts[0].groove_slope,
            ));
        }
        let skating_factor = state
            .tonearm
            .geometry
            .equivalent_radial_force_n(input.groove_radius_m, 1.0)
            .unwrap();
        let expected_power_w = telemetry.coulomb_friction_force_n
            * input.groove_tangential_velocity_m_s
            - skating_factor * telemetry.coulomb_friction_force_n * telemetry.body_velocity_m_s[0]
            + dot(
                telemetry.coulomb_wall_force_on_tip_n(),
                telemetry.tip_velocity_m_s,
            );
        assert!(nearly_equal_force(
            telemetry.tangential_friction_power_w,
            expected_power_w,
        ));
    }

    #[test]
    fn standalone_sliding_uses_the_reciprocal_body_velocity_and_all_force_ports() {
        let mut state = state();
        for _ in 0..20_000 {
            state.process(PickupMechanicalInput::default()).unwrap();
        }
        state.body_velocity_m_s[0] = 0.25;
        let skating_factor = state
            .tonearm
            .geometry
            .equivalent_radial_force_n(PickupMechanicalInput::default().groove_radius_m, 1.0)
            .unwrap();
        let input = PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts([0.0; 2], [0.35, -0.20]),
            groove_tangential_velocity_m_s: 0.5 * skating_factor * state.body_velocity_m_s[0],
            ..PickupMechanicalInput::default()
        };
        assert!(input.groove_tangential_velocity_m_s < 0.0);

        let telemetry = state.process(input).unwrap();
        assert!(telemetry.body_velocity_m_s[0] > 0.1);
        assert!(telemetry.tangential_relative_velocity_m_s > 0.0);
        assert_eq!(
            telemetry.tangential_mode,
            StylusTangentialMode::SlidingPositive,
        );

        let record_power_w =
            telemetry.coulomb_friction_force_n * input.groove_tangential_velocity_m_s;
        let reciprocal_body_power_w =
            -skating_factor * telemetry.coulomb_friction_force_n * telemetry.body_velocity_m_s[0];
        let cross_plane_tip_power_w = dot(
            telemetry.coulomb_wall_force_on_tip_n(),
            telemetry.tip_velocity_m_s,
        );
        assert!(reciprocal_body_power_w.abs() > 1.0e-8);
        assert!(nearly_equal_force(
            telemetry.tangential_friction_power_w,
            record_power_w + reciprocal_body_power_w + cross_plane_tip_power_w,
        ));
        assert!(telemetry.tangential_friction_power_w < 0.0);
    }

    #[test]
    fn wall_coulomb_force_is_inside_the_pickup_momentum_solve() {
        let mut state = state();
        for _ in 0..20_000 {
            state.process(PickupMechanicalInput::default()).unwrap();
        }
        let before_momentum = [
            state.contact.moving_mass_kg * state.tip_velocity_m_s[0]
                + state.tonearm.lateral.effective_mass_kg * state.body_velocity_m_s[0],
            state.contact.moving_mass_kg * state.tip_velocity_m_s[1]
                + state.tonearm.vertical.effective_mass_kg * state.body_velocity_m_s[1],
        ];
        let input = PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts([0.0; 2], [0.5, -0.5]),
            groove_tangential_velocity_m_s: 0.7,
            ..PickupMechanicalInput::default()
        };
        let telemetry = state.process(input).unwrap();
        let after_momentum = [
            state.contact.moving_mass_kg * state.tip_velocity_m_s[0]
                + state.tonearm.lateral.effective_mass_kg * state.body_velocity_m_s[0],
            state.contact.moving_mass_kg * state.tip_velocity_m_s[1]
                + state.tonearm.vertical.effective_mass_kg * state.body_velocity_m_s[1],
        ];
        let wall_coordinate_force_on_tip_n = [0, 1].map(|wall| {
            telemetry.wall_longitudinal_contact[wall].total_projected_normal_force_n()
                + telemetry.wall_longitudinal_contact[wall].total_coulomb_wall_force_on_tip_n()
        });
        let contact_force_on_tip_n = [
            (wall_coordinate_force_on_tip_n[0] - wall_coordinate_force_on_tip_n[1])
                * INVERSE_SQRT_2,
            (wall_coordinate_force_on_tip_n[0] + wall_coordinate_force_on_tip_n[1])
                * INVERSE_SQRT_2,
        ];
        let expected_external_force_n = [
            state.tonearm.anti_skate_force_n
                + telemetry.bearing_friction_force_n
                + telemetry.skating_force_n
                + contact_force_on_tip_n[0],
            -state.tonearm.vertical_tracking_force_n + contact_force_on_tip_n[1],
        ];
        let dt = 1.0 / state.sample_rate_hz;
        for axis in 0..2 {
            assert!(
                (after_momentum[axis]
                    - before_momentum[axis]
                    - expected_external_force_n[axis] * dt)
                    .abs()
                    < 1.0e-12,
                "axis={axis} telemetry={telemetry:?}",
            );
        }
        assert!(nearly_equal_force(
            telemetry.groove_lateral_force_on_tip_n,
            contact_force_on_tip_n[0],
        ));
    }

    #[test]
    fn sliding_reversal_flips_both_coulomb_components_and_remains_passive() {
        let mut state = state();
        for _ in 0..20_000 {
            state.process(PickupMechanicalInput::default()).unwrap();
        }
        let mut input = PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts([0.0; 2], [0.5, -0.5]),
            groove_tangential_velocity_m_s: 0.7,
            ..PickupMechanicalInput::default()
        };
        let forward = state.process(input).unwrap();
        input.groove_tangential_velocity_m_s = -0.7;
        let reverse = state.process(input).unwrap();
        assert_eq!(
            forward.tangential_mode,
            StylusTangentialMode::SlidingPositive
        );
        assert_eq!(
            reverse.tangential_mode,
            StylusTangentialMode::SlidingNegative
        );
        assert!(forward.coulomb_friction_force_n < 0.0);
        assert!(reverse.coulomb_friction_force_n > 0.0);
        assert!(forward.tangential_friction_power_w < 0.0);
        assert!(reverse.tangential_friction_power_w < 0.0);
        for telemetry in [forward, reverse] {
            for wall in telemetry.wall_longitudinal_contact {
                for contact_index in 0..wall.contact_count() {
                    assert!(nearly_equal_force(
                        wall.coulomb_wall_force_on_tip_n[contact_index],
                        wall.coulomb_friction_force_n[contact_index]
                            * wall.geometry.contacts[contact_index].groove_slope,
                    ));
                }
            }
        }
    }

    #[test]
    fn unsupported_sloped_sticking_rolls_back_without_allocation() {
        let mut state = state();
        for _ in 0..20_000 {
            state.process(PickupMechanicalInput::default()).unwrap();
        }
        let before = state;
        let input = PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts([0.0; 2], [0.5, -0.5]),
            groove_tangential_velocity_m_s: 0.0,
            ..PickupMechanicalInput::default()
        };
        let result = assert_no_alloc::assert_no_alloc(|| state.process(input));
        assert_eq!(
            result,
            Err(PickupMechanicalError::UnsupportedGrooveWallSticking),
        );
        assert_eq!(state, before);
    }

    #[test]
    fn friction_geometry_requires_a_strict_inward_force_margin() {
        let contact = StylusContactConfig::default();
        let maximum_product = 1.0 - FRICTION_GEOMETRY_PRODUCT_MARGIN;
        for sign in [-1.0, 1.0] {
            let supported_slope =
                sign * (maximum_product - 1.0e-9) / contact.groove_friction_coefficient;
            let rejected_slope =
                sign * (maximum_product + 1.0e-9) / contact.groove_friction_coefficient;
            let supported = PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts([0.0; 2], [supported_slope, 0.0]),
                groove_tangential_velocity_m_s: 0.7,
                ..PickupMechanicalInput::default()
            };
            validate_friction_geometry(contact, supported).unwrap();

            let rejected = PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts([0.0; 2], [rejected_slope, 0.0]),
                ..supported
            };
            assert_eq!(
                validate_friction_geometry(contact, rejected),
                Err(PickupMechanicalError::IllConditionedGrooveWallFrictionGeometry),
            );
            let mut state = state();
            let before = state;
            assert_eq!(
                state.process(rejected),
                Err(PickupMechanicalError::IllConditionedGrooveWallFrictionGeometry),
            );
            assert_eq!(state, before);
        }
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
        assert_eq!(snapshot.version, 5);
        snapshot.tip_velocity_m_s[0] = f64::INFINITY;
        assert_eq!(
            state.restore(snapshot),
            Err(PickupMechanicalError::InvalidSnapshot)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn snapshot_rejects_forged_coulomb_mode_and_magnitude() {
        let mut state = state();
        for _ in 0..20_000 {
            state.process(PickupMechanicalInput::default()).unwrap();
        }
        state
            .process(PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts([0.0; 2], [0.35, -0.20]),
                groove_tangential_velocity_m_s: 0.5,
                ..PickupMechanicalInput::default()
            })
            .unwrap();
        let snapshot = state.snapshot();
        assert_eq!(
            snapshot.last_telemetry.tangential_mode,
            StylusTangentialMode::SlidingPositive,
        );

        let before = state;
        let mut forged_mode = snapshot;
        forged_mode.last_telemetry.tangential_mode = StylusTangentialMode::SlidingNegative;
        assert_eq!(
            state.restore(forged_mode),
            Err(PickupMechanicalError::InvalidSnapshot),
        );
        assert_eq!(state, before);

        let mut forged_magnitude = snapshot;
        forged_magnitude.contact.groove_friction_coefficient *= 0.5;
        assert_eq!(
            state.restore(forged_magnitude),
            Err(PickupMechanicalError::InvalidSnapshot),
        );
        assert_eq!(state, before);
        state.restore(snapshot).unwrap();
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

    fn try_coupled_midpoint_step(
        record_rate: f64,
        wall_slope: [f64; 2],
        previous_mode: StylusTangentialMode,
    ) -> Result<CoupledDeckPickupStep, CoupledDeckPickupError> {
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
    }

    fn coupled_midpoint_step(
        record_rate: f64,
        wall_slope: [f64; 2],
        previous_mode: StylusTangentialMode,
    ) -> CoupledDeckPickupStep {
        try_coupled_midpoint_step(record_rate, wall_slope, previous_mode).unwrap()
    }

    #[test]
    fn midpoint_static_contact_uses_bounded_traction_without_power() {
        let step = coupled_midpoint_step(0.0, [0.0; 2], StylusTangentialMode::Separated);
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
        for (record_rate, previous_mode, expected_mode) in [
            (
                20.0,
                StylusTangentialMode::SlidingNegative,
                StylusTangentialMode::SlidingPositive,
            ),
            (
                -20.0,
                StylusTangentialMode::SlidingPositive,
                StylusTangentialMode::SlidingNegative,
            ),
        ] {
            let step = coupled_midpoint_step(record_rate, [0.5, -0.5], previous_mode);
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
    fn midpoint_sliding_closes_record_body_and_cross_plane_coulomb_power() {
        let mut pickup = state();
        pickup.body_velocity_m_s[0] = 0.25;
        let base_input = PickupMechanicalInput::default();
        let skating_factor = pickup
            .tonearm
            .geometry
            .equivalent_radial_force_n(base_input.groove_radius_m, 1.0)
            .unwrap();
        let previous_record_velocity_rad_s =
            0.5 * skating_factor * pickup.body_velocity_m_s[0] / base_input.groove_radius_m;
        let previous_midpoint_travel_m =
            0.5 * base_input.groove_radius_m * previous_record_velocity_rad_s / 192_000.0;
        let input = PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts(
                [
                    0.5 * previous_midpoint_travel_m,
                    -0.5 * previous_midpoint_travel_m,
                ],
                [0.5, -0.5],
            ),
            ..base_input
        };
        let deck_config = crate::PhysicalDeckConfig::default();
        let previous_record_rate =
            previous_record_velocity_rad_s / deck_config.nominal_angular_velocity_rad_s();
        let mut deck = DeckMechanicalState::new(deck_config).unwrap();
        deck.reset(previous_record_rate, previous_record_rate, 0.0, 0.0)
            .unwrap();
        let preparation = deck
            .prepare_midpoint_step(1.0 / 192_000.0, crate::DeckMechanicalControl::default())
            .unwrap();
        let step = solve_coupled_deck_pickup_midpoint(
            preparation,
            pickup,
            MidpointPickupGeometry {
                input,
                lateral_origin_shift_bias_m: 0.0,
                lateral_origin_shift_per_record_velocity_m_s: 0.0,
            },
            PickupElectromagneticForceRelation::constant([0.0; 2]).unwrap(),
            StylusTangentialMode::SlidingPositive,
        )
        .unwrap();
        let telemetry = step.pickup_telemetry;
        assert!(telemetry.body_velocity_m_s[0] > 0.1);
        let expected_mode = if telemetry.tangential_relative_velocity_m_s > 0.0 {
            StylusTangentialMode::SlidingPositive
        } else {
            StylusTangentialMode::SlidingNegative
        };
        assert_eq!(telemetry.tangential_mode, expected_mode);

        let record_velocity_m_s = telemetry.tangential_relative_velocity_m_s
            + skating_factor * telemetry.body_velocity_m_s[0];
        let record_power_w = telemetry.coulomb_friction_force_n * record_velocity_m_s;
        let reciprocal_body_power_w =
            -skating_factor * telemetry.coulomb_friction_force_n * telemetry.body_velocity_m_s[0];
        let cross_plane_tip_power_w = dot(
            telemetry.coulomb_wall_force_on_tip_n(),
            telemetry.tip_velocity_m_s,
        );
        let compatible_power_w = telemetry
            .wall_longitudinal_contact
            .into_iter()
            .flat_map(|wall| {
                (0..wall.contact_count()).map(move |contact_index| {
                    let slope = wall.geometry.contacts[contact_index].groove_slope;
                    wall.coulomb_friction_force_n[contact_index]
                        * (1.0 + slope * slope)
                        * telemetry.tangential_relative_velocity_m_s
                })
            })
            .sum::<f64>();
        assert!(reciprocal_body_power_w.abs() > 1.0e-8);
        assert!(nearly_equal_force(
            telemetry.tangential_friction_power_w,
            record_power_w + reciprocal_body_power_w + cross_plane_tip_power_w,
        ));
        assert!(nearly_equal_force(
            telemetry.tangential_friction_power_w,
            compatible_power_w,
        ), "telemetry={telemetry:?} compatible={compatible_power_w} record={record_velocity_m_s} K={skating_factor}");
        assert!(telemetry.tangential_friction_power_w < 0.0);
    }

    #[test]
    fn midpoint_nonzero_slope_zero_speed_uses_bounded_zero_traction() {
        for previous_mode in [
            StylusTangentialMode::Separated,
            StylusTangentialMode::Sticking,
            StylusTangentialMode::SlidingPositive,
            StylusTangentialMode::SlidingNegative,
        ] {
            let step = try_coupled_midpoint_step(0.0, [0.5, -0.5], previous_mode).unwrap();
            assert_eq!(step.tangential_mode, StylusTangentialMode::Sticking);
            assert_eq!(step.pickup_telemetry.coulomb_friction_force_n, 0.0);
            assert_eq!(step.pickup_telemetry.tangential_friction_power_w, 0.0);
            assert_eq!(
                step.pickup.contact.groove_friction_coefficient,
                StylusContactConfig::default().groove_friction_coefficient,
            );
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
