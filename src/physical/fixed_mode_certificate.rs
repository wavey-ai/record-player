//! Verified point solves for the fixed mechanical KKT systems.

use thiserror::Error;

use super::contact::{
    coupled_fixed_contact_families, coupled_fixed_mechanical_modes,
    coupled_fixed_mode_normal_response, coupled_fixed_solve_only_response,
    coupled_fixed_solve_only_subjects, fixed_stylus_constraint_relation,
    groove_friction_geometry_is_well_conditioned,
    interior_spiral_origin_shift_per_record_velocity_m_s, stylus_sticking_equality_row,
    stylus_sticking_force_column, CoupledFixedContactFamily, CoupledFixedContactPoint,
    CoupledFixedContactSurface, CoupledFixedDynamicMobility, CoupledFixedEqualityDiagnostics,
    CoupledFixedKktInverse, CoupledFixedMechanicalMode, CoupledFixedModeInfeasibility,
    CoupledFixedModeKktLhs, CoupledFixedModeResponseError, CoupledFixedOriginLaw,
    CoupledFixedPickupSupport, CoupledFixedSolveOnlySubject, FixedStylusConstraintRelation,
    JointDynamicEquation, JointDynamicVelocity, JointKktRhsCoordinate, JointKktSolutionCoordinate,
    StylusTangentialMode, COUPLED_FIXED_CONTACT_FAMILY_COUNT, COUPLED_FIXED_KKT_CAPACITY,
    COUPLED_FIXED_MECHANICAL_CLASS_COUNT, COUPLED_FIXED_MODE_FAMILY_SET_VERSION,
    COUPLED_FIXED_MODE_OPERATOR_VERSION, COUPLED_FIXED_SOLVE_ONLY_SUBJECT_COUNT,
    COUPLED_FIXED_SOLVE_ONLY_SUBJECT_SET_VERSION, JOINT_DYNAMIC_EQUATIONS,
    JOINT_DYNAMIC_VELOCITIES,
};
use super::verified_interval::{OutwardInterval, OutwardIntervalError};
use super::{PhysicalPlaybackConfig, PhysicalPlaybackConfigIdentity, PhysicalProfileError};

const DYNAMIC_VARIABLE_COUNT: usize = 6;
const KKT_CAPACITY: usize = COUPLED_FIXED_KKT_CAPACITY;
const AUGMENTED_COLUMN_COUNT: usize = KKT_CAPACITY + 1;
const RHS_COLUMN: usize = KKT_CAPACITY;
const UNUSED_PIVOT_ROW: usize = usize::MAX;
const REFERENCE_RELATIVE_PIVOT_FACTOR: f64 = 128.0;

/// Identifies the point-solve certificate format.
pub(crate) const FIXED_MODE_POINT_MOBILITY_CERTIFICATE_VERSION: u32 = 2;
pub(crate) const FIXED_MODE_SOLVE_ONLY_MOBILITY_CERTIFICATE_VERSION: u32 = 2;
pub(crate) const FIXED_MODE_CONTACT_BOX_CERTIFICATE_VERSION: u32 = 1;

/// Contains outward enclosures of one dynamic mobility matrix.
///
/// Rows use velocity order. Columns use dynamic equation order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VerifiedFixedDynamicMobility {
    velocity_by_equation_rhs: [[OutwardInterval; DYNAMIC_VARIABLE_COUNT]; DYNAMIC_VARIABLE_COUNT],
}

impl VerifiedFixedDynamicMobility {
    /// Gets one verified mobility coefficient.
    pub(crate) const fn coefficient(
        self,
        velocity: JointDynamicVelocity,
        equation: JointDynamicEquation,
    ) -> OutwardInterval {
        self.velocity_by_equation_rhs[velocity as usize][equation as usize]
    }
}

/// Contains the verified complete inverse of one active KKT system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VerifiedFixedKktInverse {
    pub(crate) system_size: usize,
    pub(crate) equality_count: usize,
    solution_by_rhs: [[OutwardInterval; KKT_CAPACITY]; KKT_CAPACITY],
}

impl VerifiedFixedKktInverse {
    /// Gets one coefficient through typed active coordinates.
    pub(crate) fn coefficient(
        &self,
        solution: JointKktSolutionCoordinate,
        rhs: JointKktRhsCoordinate,
    ) -> Option<OutwardInterval> {
        let solution_index = solution.active_index(self.equality_count)?;
        let rhs_index = rhs.active_index(self.equality_count)?;
        (solution_index < self.system_size && rhs_index < self.system_size)
            .then_some(self.solution_by_rhs[solution_index][rhs_index])
    }

    fn has_valid_inactive_storage(&self) -> bool {
        self.system_size >= DYNAMIC_VARIABLE_COUNT
            && self.system_size <= KKT_CAPACITY
            && self.equality_count == self.system_size - DYNAMIC_VARIABLE_COUNT
            && self
                .solution_by_rhs
                .iter()
                .enumerate()
                .all(|(row, values)| {
                    values.iter().enumerate().all(|(column, value)| {
                        (row < self.system_size && column < self.system_size)
                            || (value.lower() == 0.0 && value.upper() == 0.0)
                    })
                })
    }

    fn dynamic_mobility(&self) -> VerifiedFixedDynamicMobility {
        VerifiedFixedDynamicMobility {
            velocity_by_equation_rhs: std::array::from_fn(|velocity| {
                std::array::from_fn(|equation| self.solution_by_rhs[velocity][equation])
            }),
        }
    }
}

/// Reports the deterministic interval elimination path for one KKT system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VerifiedBaseKktDiagnostics {
    pub(crate) system_size: usize,
    pub(crate) equality_count: usize,
    pub(crate) row_scales: [f64; KKT_CAPACITY],
    pub(crate) column_scales: [f64; KKT_CAPACITY],
    pub(crate) pivot_rows: [usize; KKT_CAPACITY],
    pub(crate) minimum_verified_scaled_pivot: f64,
    pub(crate) maximum_verified_scaled_pivot_width: f64,
    pub(crate) maximum_dynamic_solution_width: f64,
    pub(crate) maximum_full_solution_width: f64,
    pub(crate) maximum_residual_width: f64,
    pub(crate) maximum_residual_absolute_bound: f64,
}

/// Contains one verified point mobility for a mechanical class.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VerifiedFixedBaseMobility {
    pub(crate) mechanical: CoupledFixedMechanicalMode,
    pub(crate) base_family: CoupledFixedContactFamily,
    pub(crate) kkt_lhs: CoupledFixedModeKktLhs,
    pub(crate) equality: CoupledFixedEqualityDiagnostics,
    pub(crate) reference_mobility: CoupledFixedDynamicMobility,
    pub(crate) verified_mobility: VerifiedFixedDynamicMobility,
    pub(crate) diagnostics: VerifiedBaseKktDiagnostics,
}

/// Contains the 24 verified base mobilities at one radius.
///
/// This result only proves point KKT solve containment. It does not prove a
/// contact P-matrix property or global hybrid-mode uniqueness.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VerifiedFixedBaseMobilityCatalog {
    pub(crate) certificate_version: u32,
    pub(crate) operator_version: u32,
    pub(crate) family_set_version: u32,
    pub(crate) config_identity: PhysicalPlaybackConfigIdentity,
    pub(crate) config_identity_version: u32,
    pub(crate) config_sha256: [u8; 32],
    pub(crate) point: CoupledFixedContactPoint,
    pub(crate) systems: [VerifiedFixedBaseMobility; COUPLED_FIXED_MECHANICAL_CLASS_COUNT],
}

/// Contains one verified lowered or cue-supported mobility.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VerifiedFixedSolveOnlyMobility {
    pub(crate) subject: CoupledFixedSolveOnlySubject,
    pub(crate) kkt_lhs: CoupledFixedModeKktLhs,
    pub(crate) equality: CoupledFixedEqualityDiagnostics,
    pub(crate) reference_inverse: CoupledFixedKktInverse,
    pub(crate) verified_inverse: VerifiedFixedKktInverse,
    pub(crate) reference_mobility: CoupledFixedDynamicMobility,
    pub(crate) verified_mobility: VerifiedFixedDynamicMobility,
    pub(crate) diagnostics: VerifiedBaseKktDiagnostics,
}

/// Contains all 48 source-independent lowered and cue-supported systems.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VerifiedFixedSolveOnlyMobilityCatalog {
    pub(crate) certificate_version: u32,
    pub(crate) operator_version: u32,
    pub(crate) subject_set_version: u32,
    pub(crate) config_identity: PhysicalPlaybackConfigIdentity,
    pub(crate) config_identity_version: u32,
    pub(crate) config_sha256: [u8; 32],
    pub(crate) systems:
        Box<[VerifiedFixedSolveOnlyMobility; COUPLED_FIXED_SOLVE_ONLY_SUBJECT_COUNT]>,
}

impl VerifiedFixedSolveOnlyMobilityCatalog {
    fn metadata_is_valid(&self) -> bool {
        self.certificate_version == FIXED_MODE_SOLVE_ONLY_MOBILITY_CERTIFICATE_VERSION
            && self.operator_version == COUPLED_FIXED_MODE_OPERATOR_VERSION
            && self.subject_set_version == COUPLED_FIXED_SOLVE_ONLY_SUBJECT_SET_VERSION
            && self.config_identity_version == self.config_identity.identity_version()
            && self.config_sha256 == self.config_identity.sha256()
    }

    /// Gets a verified subject when its catalog label is valid.
    pub(crate) fn get(
        &self,
        subject: CoupledFixedSolveOnlySubject,
    ) -> Option<&VerifiedFixedSolveOnlyMobility> {
        if !self.metadata_is_valid()
            || subject.subject_set_version() != COUPLED_FIXED_SOLVE_ONLY_SUBJECT_SET_VERSION
        {
            return None;
        }
        self.systems.iter().find(|system| system.subject == subject)
    }

    /// Gets the lowered no-contact system for one mechanical class.
    pub(crate) fn lowered_base(
        &self,
        mechanical: CoupledFixedMechanicalMode,
    ) -> Option<&VerifiedFixedSolveOnlyMobility> {
        if !self.metadata_is_valid() {
            return None;
        }
        self.systems.iter().find(|system| {
            system.subject.mechanical == mechanical
                && system.subject.support == CoupledFixedPickupSupport::LoweredNoContact
        })
    }
}

/// Contains one closed radius-and-slope domain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FixedModeContactIntervalBox {
    pub(crate) groove_radius_m: OutwardInterval,
    pub(crate) wall_slopes: [OutwardInterval; 2],
}

impl FixedModeContactIntervalBox {
    /// Makes one domain from finite ordered bounds.
    pub(crate) fn hull(
        radius_lower_m: f64,
        radius_upper_m: f64,
        wall_slope_lower: [f64; 2],
        wall_slope_upper: [f64; 2],
    ) -> Result<Self, OutwardIntervalError> {
        Ok(Self {
            groove_radius_m: OutwardInterval::hull(radius_lower_m, radius_upper_m)?,
            wall_slopes: [
                OutwardInterval::hull(wall_slope_lower[0], wall_slope_upper[0])?,
                OutwardInterval::hull(wall_slope_lower[1], wall_slope_upper[1])?,
            ],
        })
    }

    /// Makes a zero-width domain from one production point.
    pub(crate) fn point(point: CoupledFixedContactPoint) -> Result<Self, OutwardIntervalError> {
        Self::hull(
            point.groove_radius_m,
            point.groove_radius_m,
            point.wall_slopes,
            point.wall_slopes,
        )
    }
}

/// Contains one interval force column in dynamic equation order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VerifiedDynamicEquationRhs {
    coefficients: [OutwardInterval; DYNAMIC_VARIABLE_COUNT],
}

impl VerifiedDynamicEquationRhs {
    pub(crate) const fn coefficient(self, equation: JointDynamicEquation) -> OutwardInterval {
        self.coefficients[equation as usize]
    }
}

/// Contains one interval gap row in dynamic velocity order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VerifiedDynamicVelocityRow {
    coefficients: [OutwardInterval; DYNAMIC_VARIABLE_COUNT],
}

impl VerifiedDynamicVelocityRow {
    pub(crate) const fn coefficient(self, velocity: JointDynamicVelocity) -> OutwardInterval {
        self.coefficients[velocity as usize]
    }
}

/// Contains the interval H and G operators for one contact family.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VerifiedContactHgOperator {
    constraint_count: usize,
    normal_force_rhs: [VerifiedDynamicEquationRhs; 2],
    normal_gap_velocity: [VerifiedDynamicVelocityRow; 2],
}

impl VerifiedContactHgOperator {
    pub(crate) const fn constraint_count(self) -> usize {
        self.constraint_count
    }

    pub(crate) const fn normal_force_rhs(self, constraint: usize) -> VerifiedDynamicEquationRhs {
        self.normal_force_rhs[constraint]
    }

    pub(crate) const fn normal_gap_velocity(self, constraint: usize) -> VerifiedDynamicVelocityRow {
        self.normal_gap_velocity[constraint]
    }
}

/// Describes the interval treatment of the stylus sticking equality.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum VerifiedStylusEqualityDisposition {
    NotRequested,
    AddsRank { schur_denominator: OutwardInterval },
    DependentRuntimeRhs,
}

/// Describes production reachability over one complete interval box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerifiedContactBoxReachability {
    RuntimeConditional,
    /// Production admits this mode only inside its normal-force tolerance band.
    NormalForceToleranceBandOnly,
    ActiveWallDependent {
        nonzero_active_wall: CoupledFixedModeInfeasibility,
        wall_may_be_nonzero: [bool; 2],
        wall_is_always_nonzero: [bool; 2],
    },
}

/// Contains verified groove response and raw principal-minor signs.
///
/// The signs are not normalized production-solver robustness margins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VerifiedGrooveNormalResponse {
    pub(crate) w: [[OutwardInterval; 2]; 2],
    pub(crate) diagonal: [OutwardInterval; 2],
    pub(crate) determinant: OutwardInterval,
    pub(crate) diagonal_is_strictly_positive: [bool; 2],
    pub(crate) determinant_is_strictly_positive: bool,
}

/// Contains verified land response and its raw scalar sign.
///
/// The sign is not a normalized production-solver robustness margin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VerifiedLandNormalResponse {
    pub(crate) w: OutwardInterval,
    pub(crate) is_strictly_positive: bool,
}

/// Contains the verified normal response for one contact surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum VerifiedFixedNormalResponse {
    GrooveWalls(VerifiedGrooveNormalResponse),
    RecordLand(VerifiedLandNormalResponse),
}

/// Contains one fixed-family evaluation over one interval box.
///
/// This diagnostic cannot gate profile admission. It does not prove hybrid
/// uniqueness, mode selection, or runtime right-hand-side compatibility.
/// It does not link raw margins to the full-mask scaled production solve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VerifiedFixedContactBoxEvaluation {
    pub(crate) certificate_version: u32,
    pub(crate) family: CoupledFixedContactFamily,
    pub(crate) domain: FixedModeContactIntervalBox,
    pub(crate) skating_factor: OutwardInterval,
    pub(crate) stylus_equality: VerifiedStylusEqualityDisposition,
    pub(crate) runtime_rhs_compatibility_required: bool,
    pub(crate) reachability: VerifiedContactBoxReachability,
    pub(crate) effective_mobility: VerifiedFixedDynamicMobility,
    pub(crate) contact_operator: VerifiedContactHgOperator,
    pub(crate) normal_response: VerifiedFixedNormalResponse,
}

/// Reports why one contact box could not be verified.
#[derive(Debug, Error)]
pub(crate) enum FixedModeContactBoxError {
    #[error(transparent)]
    InvalidConfig(#[from] PhysicalProfileError),
    #[error("the point-mobility catalog version is not supported")]
    InvalidCatalog,
    #[error("the contact family is not in the versioned catalog")]
    InvalidFamily,
    #[error("the point-mobility catalog belongs to a different playback configuration")]
    ConfigIdentityMismatch,
    #[error("the contact box is outside the validated playback domain")]
    InvalidDomain,
    #[error("a held program boundary requires zero wall slopes")]
    HeldBoundaryRequiresZeroSlope,
    #[error("the groove friction geometry is not valid over the complete box")]
    IllConditionedGrooveFrictionGeometry,
    #[error("the tonearm skating factor is not verified over the complete box")]
    UnresolvedSkatingGeometry,
    #[error("the stylus sticking rank is not constant over the complete box")]
    UnresolvedStickingRank,
    #[error("the stylus sticking Schur denominator contains zero")]
    SchurDenominatorContainsZero,
    #[error("the complete point KKT builder failed")]
    PointBuilder(#[source] CoupledFixedModeResponseError),
    #[error("the complete point KKT solve failed for equation {equation_index}")]
    PointKktSolve {
        equation_index: usize,
        #[source]
        source: VerifiedPointKktError,
    },
    #[error("a production point coefficient is outside its interval enclosure")]
    ProductionPointNotEnclosed,
    #[error("verified interval arithmetic failed")]
    Interval(#[from] OutwardIntervalError),
}

/// Reports why a fixed-mode point solve could not be verified.
#[derive(Debug, Error)]
pub(crate) enum FixedModePointMobilityCertificateError {
    #[error(transparent)]
    InvalidConfig(#[from] PhysicalProfileError),
    #[error("the fixed-mode catalog does not contain the required 24 base systems")]
    InvalidCatalog,
    #[error("mechanical class {mechanical_class_index} has no separated land base family")]
    MissingBaseFamily { mechanical_class_index: usize },
    #[error("the point builder failed for mechanical class {mechanical_class_index}")]
    PointBuilder {
        mechanical_class_index: usize,
        #[source]
        source: CoupledFixedModeResponseError,
    },
    #[error(
        "the verified KKT solve failed for mechanical class {mechanical_class_index}, equation {equation_index}"
    )]
    KktSolve {
        mechanical_class_index: usize,
        equation_index: usize,
        #[source]
        source: VerifiedPointKktError,
    },
    #[error(
        "the verified solve path changed between right-hand sides for mechanical class {mechanical_class_index}"
    )]
    InconsistentSolvePath { mechanical_class_index: usize },
    #[error(
        "the point replay differs from production for mechanical class {mechanical_class_index}, velocity {velocity_index}, equation {equation_index}"
    )]
    ProductionReplayMismatch {
        mechanical_class_index: usize,
        velocity_index: usize,
        equation_index: usize,
    },
    #[error("the verified mobility does not contain production for mechanical class {mechanical_class_index}, velocity {velocity_index}, equation {equation_index}")]
    ProductionNotEnclosed {
        mechanical_class_index: usize,
        velocity_index: usize,
        equation_index: usize,
    },
}

/// Reports why the solve-only mobility catalog could not be verified.
#[derive(Debug, Error)]
pub(crate) enum FixedModeSolveOnlyMobilityCertificateError {
    #[error(transparent)]
    InvalidConfig(#[from] PhysicalProfileError),
    #[error("the fixed-mode catalog does not contain the required 48 solve-only subjects")]
    InvalidCatalog,
    #[error("the solve-only builder failed for subject {subject_index}")]
    SubjectBuilder {
        subject_index: usize,
        #[source]
        source: CoupledFixedModeResponseError,
    },
    #[error("the verified KKT solve failed for subject {subject_index}, RHS {rhs_index}")]
    KktSolve {
        subject_index: usize,
        rhs_index: usize,
        #[source]
        source: VerifiedPointKktError,
    },
    #[error(
        "the verified solve path changed between right-hand sides for subject {subject_index}"
    )]
    InconsistentSolvePath { subject_index: usize },
    #[error("the complete KKT replay differs from production for subject {subject_index}, solution {solution_index}, RHS {rhs_index}")]
    ProductionInverseReplayMismatch {
        subject_index: usize,
        solution_index: usize,
        rhs_index: usize,
    },
    #[error("the verified complete KKT inverse does not contain production for subject {subject_index}, solution {solution_index}, RHS {rhs_index}")]
    ProductionInverseNotEnclosed {
        subject_index: usize,
        solution_index: usize,
        rhs_index: usize,
    },
    #[error("the complete KKT inverse storage is invalid for subject {subject_index}")]
    InvalidInverseStorage { subject_index: usize },
    #[error("the dynamic mobility projection differs from the complete KKT inverse for subject {subject_index}")]
    DynamicProjectionMismatch { subject_index: usize },
}

/// Reports why one scaled point system could not be verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum VerifiedPointKktError {
    #[error("the KKT size is invalid")]
    InvalidSize,
    #[error("the KKT input contains a nonfinite value")]
    NonFiniteInput,
    #[error("a deterministic row or column scale is zero")]
    ZeroScale,
    #[error("the reference elimination rejected a scaled pivot")]
    ReferencePivotRejected,
    #[error("a verified scaled pivot contains zero")]
    VerifiedPivotContainsZero,
    #[error("a verified scaled pivot does not clear the production tolerance")]
    VerifiedPivotBelowTolerance,
    #[error("verified interval arithmetic failed")]
    Interval(#[from] OutwardIntervalError),
    #[error("the verified solution does not contain the replayed point solution")]
    PointSolutionNotEnclosed,
    #[error("the verified residual does not contain zero")]
    ResidualNotEnclosed,
}

/// Verifies all 24 mechanical base mobilities at one contact point.
///
/// The base family uses a separated stylus on the record land. This choice
/// excludes the later tangential rank-one constraint from the base KKT system.
pub(crate) fn verify_fixed_base_mobilities_at_point(
    config: PhysicalPlaybackConfig,
    point: CoupledFixedContactPoint,
) -> Result<VerifiedFixedBaseMobilityCatalog, FixedModePointMobilityCertificateError> {
    let config_identity = config.identity()?;
    if coupled_fixed_mechanical_modes().count() != COUPLED_FIXED_MECHANICAL_CLASS_COUNT
        || coupled_fixed_contact_families().count() != COUPLED_FIXED_CONTACT_FAMILY_COUNT
    {
        return Err(FixedModePointMobilityCertificateError::InvalidCatalog);
    }
    let mut systems = Vec::with_capacity(COUPLED_FIXED_MECHANICAL_CLASS_COUNT);
    for (mechanical_class_index, mechanical) in coupled_fixed_mechanical_modes().enumerate() {
        let base_family = coupled_fixed_contact_families()
            .find(|family| {
                family.mechanical == mechanical
                    && family.surface == CoupledFixedContactSurface::RecordLand
                    && family.origin_law == CoupledFixedOriginLaw::SurfaceIndependent
                    && family.stylus == StylusTangentialMode::Separated
            })
            .ok_or(FixedModePointMobilityCertificateError::MissingBaseFamily {
                mechanical_class_index,
            })?;
        let response =
            coupled_fixed_mode_normal_response(config, base_family, point).map_err(|source| {
                FixedModePointMobilityCertificateError::PointBuilder {
                    mechanical_class_index,
                    source,
                }
            })?;
        if base_family.family_set_version() != COUPLED_FIXED_MODE_FAMILY_SET_VERSION
            || response.operator_version != COUPLED_FIXED_MODE_OPERATOR_VERSION
            || response.family != base_family
        {
            return Err(FixedModePointMobilityCertificateError::InvalidCatalog);
        }
        systems.push(verify_base_mobility(
            mechanical_class_index,
            response.kkt_lhs,
            response.equality,
            mechanical,
            base_family,
            response.dynamic_mobility,
        )?);
    }
    let systems = systems
        .try_into()
        .map_err(|_| FixedModePointMobilityCertificateError::InvalidCatalog)?;
    Ok(VerifiedFixedBaseMobilityCatalog {
        certificate_version: FIXED_MODE_POINT_MOBILITY_CERTIFICATE_VERSION,
        operator_version: COUPLED_FIXED_MODE_OPERATOR_VERSION,
        family_set_version: COUPLED_FIXED_MODE_FAMILY_SET_VERSION,
        config_identity,
        config_identity_version: config_identity.identity_version(),
        config_sha256: config_identity.sha256(),
        point,
        systems,
    })
}

/// Verifies all 48 source-independent lowered and cue-supported systems.
pub(crate) fn verify_fixed_solve_only_mobilities(
    config: PhysicalPlaybackConfig,
) -> Result<VerifiedFixedSolveOnlyMobilityCatalog, FixedModeSolveOnlyMobilityCertificateError> {
    let config_identity = config.identity()?;
    if coupled_fixed_mechanical_modes().count() != COUPLED_FIXED_MECHANICAL_CLASS_COUNT
        || coupled_fixed_solve_only_subjects().count() != COUPLED_FIXED_SOLVE_ONLY_SUBJECT_COUNT
    {
        return Err(FixedModeSolveOnlyMobilityCertificateError::InvalidCatalog);
    }

    let mut systems = Vec::with_capacity(COUPLED_FIXED_SOLVE_ONLY_SUBJECT_COUNT);
    for (subject_index, subject) in coupled_fixed_solve_only_subjects().enumerate() {
        let response = coupled_fixed_solve_only_response(config, subject).map_err(|source| {
            FixedModeSolveOnlyMobilityCertificateError::SubjectBuilder {
                subject_index,
                source,
            }
        })?;
        if subject.subject_set_version() != COUPLED_FIXED_SOLVE_ONLY_SUBJECT_SET_VERSION
            || response.operator_version != COUPLED_FIXED_MODE_OPERATOR_VERSION
            || response.subject != subject
        {
            return Err(FixedModeSolveOnlyMobilityCertificateError::InvalidCatalog);
        }
        systems.push(verify_solve_only_mobility(
            subject_index,
            subject,
            response.kkt_lhs,
            response.equality,
            response.kkt_inverse,
            response.dynamic_mobility,
        )?);
    }
    let systems = systems
        .into_boxed_slice()
        .try_into()
        .map_err(|_| FixedModeSolveOnlyMobilityCertificateError::InvalidCatalog)?;
    Ok(VerifiedFixedSolveOnlyMobilityCatalog {
        certificate_version: FIXED_MODE_SOLVE_ONLY_MOBILITY_CERTIFICATE_VERSION,
        operator_version: COUPLED_FIXED_MODE_OPERATOR_VERSION,
        subject_set_version: COUPLED_FIXED_SOLVE_ONLY_SUBJECT_SET_VERSION,
        config_identity,
        config_identity_version: config_identity.identity_version(),
        config_sha256: config_identity.sha256(),
        systems,
    })
}

/// Evaluates one fixed contact family over one radius-and-slope box.
pub(crate) fn evaluate_fixed_contact_family_box(
    catalog: &VerifiedFixedBaseMobilityCatalog,
    config: PhysicalPlaybackConfig,
    family: CoupledFixedContactFamily,
    domain: FixedModeContactIntervalBox,
) -> Result<VerifiedFixedContactBoxEvaluation, FixedModeContactBoxError> {
    let config_identity = config.identity()?;
    if catalog.certificate_version != FIXED_MODE_POINT_MOBILITY_CERTIFICATE_VERSION
        || catalog.operator_version != COUPLED_FIXED_MODE_OPERATOR_VERSION
        || catalog.family_set_version != COUPLED_FIXED_MODE_FAMILY_SET_VERSION
        || catalog.systems.len() != COUPLED_FIXED_MECHANICAL_CLASS_COUNT
        || catalog.config_identity_version != catalog.config_identity.identity_version()
        || catalog.config_sha256 != catalog.config_identity.sha256()
    {
        return Err(FixedModeContactBoxError::InvalidCatalog);
    }
    if catalog.config_identity != config_identity {
        return Err(FixedModeContactBoxError::ConfigIdentityMismatch);
    }
    if family.family_set_version() != COUPLED_FIXED_MODE_FAMILY_SET_VERSION
        || !coupled_fixed_contact_families().any(|candidate| candidate == family)
    {
        return Err(FixedModeContactBoxError::InvalidFamily);
    }
    validate_contact_box(config, family, domain)?;

    let base = catalog
        .systems
        .iter()
        .find(|system| system.mechanical == family.mechanical)
        .copied()
        .ok_or(FixedModeContactBoxError::InvalidCatalog)?;
    let skating_factor = verified_skating_factor(config, domain.groove_radius_m)?;
    let reference_radius_m = domain.groove_radius_m.lower();
    let reference_skating_factor = config
        .tonearm
        .geometry
        .skating_force_factor(reference_radius_m)
        .map_err(|_| FixedModeContactBoxError::UnresolvedSkatingGeometry)?;
    if !skating_factor.contains(reference_skating_factor) {
        return Err(FixedModeContactBoxError::ProductionPointNotEnclosed);
    }

    let (effective_mobility, stylus_equality, dependent_stylus_rhs) = verified_effective_mobility(
        base.verified_mobility,
        family,
        domain.groove_radius_m,
        skating_factor,
        reference_radius_m,
        reference_skating_factor,
    )?;
    let effective_mobility =
        include_complete_point_kkt_mobility(effective_mobility, config, family, domain)?;
    let contact_operator = verified_contact_hg_operator(config, family, domain, skating_factor)?;
    let normal_response =
        verified_normal_response(family.surface, effective_mobility, contact_operator)?;
    Ok(VerifiedFixedContactBoxEvaluation {
        certificate_version: FIXED_MODE_CONTACT_BOX_CERTIFICATE_VERSION,
        family,
        domain,
        skating_factor,
        stylus_equality,
        runtime_rhs_compatibility_required: base.equality.runtime_rhs_compatibility_required
            || dependent_stylus_rhs,
        reachability: classify_contact_box_reachability(config, family, domain),
        effective_mobility,
        contact_operator,
        normal_response,
    })
}

fn include_complete_point_kkt_mobility(
    mut mobility: VerifiedFixedDynamicMobility,
    config: PhysicalPlaybackConfig,
    family: CoupledFixedContactFamily,
    domain: FixedModeContactIntervalBox,
) -> Result<VerifiedFixedDynamicMobility, FixedModeContactBoxError> {
    let Some(point) = contact_box_point(domain) else {
        return Ok(mobility);
    };
    let response = coupled_fixed_mode_normal_response(config, family, point)
        .map_err(FixedModeContactBoxError::PointBuilder)?;
    if response.operator_version != COUPLED_FIXED_MODE_OPERATOR_VERSION || response.family != family
    {
        return Err(FixedModeContactBoxError::ProductionPointNotEnclosed);
    }

    for equation in JOINT_DYNAMIC_EQUATIONS {
        let equation_index = equation as usize;
        let mut rhs = [0.0; KKT_CAPACITY];
        rhs[equation_index] = 1.0;
        let solved = verify_point_kkt_system(
            response.kkt_lhs.coefficients,
            response.kkt_lhs.system_size,
            rhs,
        )
        .map_err(|source| FixedModeContactBoxError::PointKktSolve {
            equation_index,
            source,
        })?;
        for velocity in JOINT_DYNAMIC_VELOCITIES {
            let velocity_index = velocity as usize;
            let production = response.dynamic_mobility.coefficient(velocity, equation);
            let complete_kkt = solved.solution[velocity_index];
            let reduced = mobility.coefficient(velocity, equation);
            if solved.reference_solution[velocity_index].to_bits() != production.to_bits()
                || !complete_kkt.contains(production)
                || !intervals_overlap(reduced, complete_kkt)
            {
                return Err(FixedModeContactBoxError::ProductionPointNotEnclosed);
            }
            mobility.velocity_by_equation_rhs[velocity_index][equation_index] =
                reduced.hull_with(complete_kkt);
        }
    }
    Ok(mobility)
}

fn contact_box_point(domain: FixedModeContactIntervalBox) -> Option<CoupledFixedContactPoint> {
    if domain.groove_radius_m.lower() != domain.groove_radius_m.upper()
        || domain
            .wall_slopes
            .into_iter()
            .any(|slope| slope.lower() != slope.upper())
    {
        return None;
    }
    Some(CoupledFixedContactPoint {
        groove_radius_m: domain.groove_radius_m.lower(),
        wall_slopes: domain.wall_slopes.map(OutwardInterval::lower),
    })
}

fn intervals_overlap(left: OutwardInterval, right: OutwardInterval) -> bool {
    left.lower() <= right.upper() && right.lower() <= left.upper()
}

fn validate_contact_box(
    config: PhysicalPlaybackConfig,
    family: CoupledFixedContactFamily,
    domain: FixedModeContactIntervalBox,
) -> Result<(), FixedModeContactBoxError> {
    if domain.groove_radius_m.lower() < config.groove.inner_program_radius_m
        || domain.groove_radius_m.upper() > config.groove.outer_program_radius_m
        || domain.groove_radius_m.lower() <= 0.0
    {
        return Err(FixedModeContactBoxError::InvalidDomain);
    }
    if family.surface != CoupledFixedContactSurface::GrooveWalls {
        return Ok(());
    }
    if family.origin_law == CoupledFixedOriginLaw::HeldProgramBoundary
        && domain
            .wall_slopes
            .into_iter()
            .any(|slope| !is_exact_zero(slope))
    {
        return Err(FixedModeContactBoxError::HeldBoundaryRequiresZeroSlope);
    }
    if domain.wall_slopes.into_iter().any(|slope| {
        !groove_friction_geometry_is_well_conditioned(
            config.contact.groove_friction_coefficient,
            slope.maximum_absolute(),
        )
    }) {
        return Err(FixedModeContactBoxError::IllConditionedGrooveFrictionGeometry);
    }
    Ok(())
}

fn verified_skating_factor(
    config: PhysicalPlaybackConfig,
    radius: OutwardInterval,
) -> Result<OutwardInterval, FixedModeContactBoxError> {
    let geometry = config.tonearm.geometry;
    let length = OutwardInterval::point(geometry.effective_length_m)?;
    let pivot = OutwardInterval::point(geometry.pivot_to_spindle_m)?;
    let two = OutwardInterval::point(2.0)?;
    let one = OutwardInterval::point(1.0)?;
    let radius_squared = radius.square()?;
    let pivot_squared = pivot.square()?;
    let length_squared = length.square()?;
    let cosine_numerator = radius_squared
        .add(pivot_squared)?
        .subtract(length_squared)?;
    let cosine_denominator = two.multiply(radius)?.multiply(pivot)?;
    let cosine = cosine_numerator.divide(cosine_denominator)?;
    if cosine.lower() <= -1.0 || cosine.upper() >= 1.0 {
        return Err(FixedModeContactBoxError::UnresolvedSkatingGeometry);
    }
    let sine_squared = one.subtract(cosine.square()?)?;
    if sine_squared.lower() <= 0.0 {
        return Err(FixedModeContactBoxError::UnresolvedSkatingGeometry);
    }
    let sine = sine_squared.sqrt()?;
    let radial_moment_arm = pivot.multiply(sine)?;
    if radial_moment_arm.lower() < 1.0e-12 {
        return Err(FixedModeContactBoxError::UnresolvedSkatingGeometry);
    }
    pivot
        .multiply(cosine)?
        .subtract(radius)?
        .divide(radial_moment_arm)
        .map_err(Into::into)
}

fn verified_effective_mobility(
    base: VerifiedFixedDynamicMobility,
    family: CoupledFixedContactFamily,
    radius: OutwardInterval,
    skating_factor: OutwardInterval,
    reference_radius_m: f64,
    reference_skating_factor: f64,
) -> Result<
    (
        VerifiedFixedDynamicMobility,
        VerifiedStylusEqualityDisposition,
        bool,
    ),
    FixedModeContactBoxError,
> {
    if family.stylus != StylusTangentialMode::Sticking {
        return Ok((base, VerifiedStylusEqualityDisposition::NotRequested, false));
    }

    let minus_two = OutwardInterval::point(-2.0)?;
    let body_coefficient = minus_two.multiply(skating_factor)?.divide(radius)?;
    if body_coefficient.contains_zero() {
        return Err(FixedModeContactBoxError::UnresolvedStickingRank);
    }
    let reference_body_coefficient = -2.0 * reference_skating_factor / reference_radius_m;
    if !body_coefficient.contains(reference_body_coefficient) {
        return Err(FixedModeContactBoxError::ProductionPointNotEnclosed);
    }
    let relation = fixed_stylus_constraint_relation(family.mechanical, reference_body_coefficient)
        .ok_or(FixedModeContactBoxError::UnresolvedStickingRank)?;

    let force = verified_stylus_force_column(radius, skating_factor)?;
    let equality = verified_stylus_equality_row(body_coefficient)?;
    let reference_force =
        stylus_sticking_force_column(reference_radius_m, reference_skating_factor);
    let reference_equality = stylus_sticking_equality_row(reference_body_coefficient);
    for equation in JOINT_DYNAMIC_EQUATIONS {
        if !force
            .coefficient(equation)
            .contains(reference_force.coefficient(equation))
        {
            return Err(FixedModeContactBoxError::ProductionPointNotEnclosed);
        }
    }
    for velocity in JOINT_DYNAMIC_VELOCITIES {
        if !equality
            .coefficient(velocity)
            .contains(reference_equality.coefficient(velocity))
        {
            return Err(FixedModeContactBoxError::ProductionPointNotEnclosed);
        }
    }

    match relation {
        FixedStylusConstraintRelation::AddsRank => {
            let (mobility, denominator) =
                verified_nonsymmetric_schur_update(base, force, equality)?;
            Ok((
                mobility,
                VerifiedStylusEqualityDisposition::AddsRank {
                    schur_denominator: denominator,
                },
                false,
            ))
        }
        FixedStylusConstraintRelation::DependentRuntimeRhs => Ok((
            base,
            VerifiedStylusEqualityDisposition::DependentRuntimeRhs,
            true,
        )),
    }
}

fn verified_stylus_force_column(
    radius: OutwardInterval,
    skating_factor: OutwardInterval,
) -> Result<VerifiedDynamicEquationRhs, OutwardIntervalError> {
    let zero = OutwardInterval::point(0.0)?;
    let mut coefficients = [zero; DYNAMIC_VARIABLE_COUNT];
    coefficients[JointDynamicEquation::Record as usize] = radius.negate();
    coefficients[JointDynamicEquation::BodyX as usize] = skating_factor;
    Ok(VerifiedDynamicEquationRhs { coefficients })
}

fn verified_stylus_equality_row(
    body_coefficient: OutwardInterval,
) -> Result<VerifiedDynamicVelocityRow, OutwardIntervalError> {
    let zero = OutwardInterval::point(0.0)?;
    let mut coefficients = [zero; DYNAMIC_VARIABLE_COUNT];
    coefficients[JointDynamicVelocity::Record as usize] = OutwardInterval::point(1.0)?;
    coefficients[JointDynamicVelocity::BodyX as usize] = body_coefficient;
    Ok(VerifiedDynamicVelocityRow { coefficients })
}

fn verified_nonsymmetric_schur_update(
    base: VerifiedFixedDynamicMobility,
    force: VerifiedDynamicEquationRhs,
    equality: VerifiedDynamicVelocityRow,
) -> Result<(VerifiedFixedDynamicMobility, OutwardInterval), FixedModeContactBoxError> {
    let zero = OutwardInterval::point(0.0)?;
    let mut mobility_force = [zero; DYNAMIC_VARIABLE_COUNT];
    for velocity in JOINT_DYNAMIC_VELOCITIES {
        let mut sum = zero;
        for equation in JOINT_DYNAMIC_EQUATIONS {
            let term = base
                .coefficient(velocity, equation)
                .multiply(force.coefficient(equation))?;
            sum = term.add(sum)?;
        }
        mobility_force[velocity as usize] = sum;
    }

    let mut equality_mobility = [zero; DYNAMIC_VARIABLE_COUNT];
    for equation in JOINT_DYNAMIC_EQUATIONS {
        let mut sum = zero;
        for velocity in JOINT_DYNAMIC_VELOCITIES {
            let term = equality
                .coefficient(velocity)
                .multiply(base.coefficient(velocity, equation))?;
            sum = term.add(sum)?;
        }
        equality_mobility[equation as usize] = sum;
    }

    let mut denominator = zero;
    for velocity in JOINT_DYNAMIC_VELOCITIES {
        let term = equality
            .coefficient(velocity)
            .multiply(mobility_force[velocity as usize])?;
        denominator = term.add(denominator)?;
    }
    if denominator.contains_zero() {
        return Err(FixedModeContactBoxError::SchurDenominatorContainsZero);
    }

    let mut velocity_by_equation_rhs = [[zero; DYNAMIC_VARIABLE_COUNT]; DYNAMIC_VARIABLE_COUNT];
    for velocity in JOINT_DYNAMIC_VELOCITIES {
        for equation in JOINT_DYNAMIC_EQUATIONS {
            let correction = mobility_force[velocity as usize]
                .multiply(equality_mobility[equation as usize])?
                .divide(denominator)?;
            velocity_by_equation_rhs[velocity as usize][equation as usize] =
                base.coefficient(velocity, equation).subtract(correction)?;
        }
    }
    Ok((
        VerifiedFixedDynamicMobility {
            velocity_by_equation_rhs,
        },
        denominator,
    ))
}

fn verified_contact_hg_operator(
    config: PhysicalPlaybackConfig,
    family: CoupledFixedContactFamily,
    domain: FixedModeContactIntervalBox,
    skating_factor: OutwardInterval,
) -> Result<VerifiedContactHgOperator, FixedModeContactBoxError> {
    let zero = OutwardInterval::point(0.0)?;
    let one = OutwardInterval::point(1.0)?;
    let half = OutwardInterval::point(0.5)?;
    let dt_value = 1.0 / config.solver.internal_sample_rate_hz;
    let dt = OutwardInterval::point(dt_value)?;
    let friction_value = match family.surface {
        CoupledFixedContactSurface::GrooveWalls => config.contact.groove_friction_coefficient,
        CoupledFixedContactSurface::RecordLand => {
            config.contact.record_surface_friction_coefficient
        }
    };
    let friction = OutwardInterval::point(friction_value)?;
    let sliding_direction = match family.stylus {
        StylusTangentialMode::SlidingPositive => 1.0,
        StylusTangentialMode::SlidingNegative => -1.0,
        StylusTangentialMode::Separated | StylusTangentialMode::Sticking => 0.0,
    };
    let sigma_mu = OutwardInterval::point(sliding_direction)?.multiply(friction)?;
    let origin_shift = match family.origin_law {
        CoupledFixedOriginLaw::InteriorSpiral => {
            let pitch = OutwardInterval::point(config.record_cut.groove_pitch_m_per_revolution)?;
            let tau = OutwardInterval::point(std::f64::consts::TAU)?;
            pitch.negate().multiply(half)?.multiply(dt)?.divide(tau)?
        }
        CoupledFixedOriginLaw::HeldProgramBoundary | CoupledFixedOriginLaw::SurfaceIndependent => {
            zero
        }
    };
    let reference_origin_shift = match family.origin_law {
        CoupledFixedOriginLaw::InteriorSpiral => {
            interior_spiral_origin_shift_per_record_velocity_m_s(
                config.record_cut.groove_pitch_m_per_revolution,
                dt_value,
            )
        }
        CoupledFixedOriginLaw::HeldProgramBoundary | CoupledFixedOriginLaw::SurfaceIndependent => {
            0.0
        }
    };
    if !origin_shift.contains(reference_origin_shift) {
        return Err(FixedModeContactBoxError::ProductionPointNotEnclosed);
    }

    let empty_rhs = VerifiedDynamicEquationRhs {
        coefficients: [zero; DYNAMIC_VARIABLE_COUNT],
    };
    let empty_row = VerifiedDynamicVelocityRow {
        coefficients: [zero; DYNAMIC_VARIABLE_COUNT],
    };
    let mut normal_force_rhs = [empty_rhs; 2];
    let mut normal_gap_velocity = [empty_row; 2];
    let constraint_count = match family.surface {
        CoupledFixedContactSurface::GrooveWalls => 2,
        CoupledFixedContactSurface::RecordLand => 1,
    };
    for constraint in 0..constraint_count {
        let (normal_x_value, normal_z_value, slope) = match family.surface {
            CoupledFixedContactSurface::GrooveWalls => {
                let normal_x = if constraint == 0 {
                    std::f64::consts::FRAC_1_SQRT_2
                } else {
                    -std::f64::consts::FRAC_1_SQRT_2
                };
                (
                    normal_x,
                    std::f64::consts::FRAC_1_SQRT_2,
                    domain.wall_slopes[constraint],
                )
            }
            CoupledFixedContactSurface::RecordLand => (0.0, 1.0, zero),
        };
        let normal_x = OutwardInterval::point(normal_x_value)?;
        let normal_z = OutwardInterval::point(normal_z_value)?;
        let tangential = match family.surface {
            CoupledFixedContactSurface::GrooveWalls => slope.add(sigma_mu)?,
            CoupledFixedContactSurface::RecordLand => sigma_mu,
        };
        let wall_force_scale = match family.surface {
            CoupledFixedContactSurface::GrooveWalls => one.subtract(sigma_mu.multiply(slope)?)?,
            CoupledFixedContactSurface::RecordLand => one,
        };

        let mut force_coefficients = [zero; DYNAMIC_VARIABLE_COUNT];
        if family.stylus != StylusTangentialMode::Sticking {
            force_coefficients[JointDynamicEquation::Record as usize] =
                domain.groove_radius_m.negate().multiply(tangential)?;
            force_coefficients[JointDynamicEquation::BodyX as usize] =
                skating_factor.multiply(tangential)?;
        }
        force_coefficients[JointDynamicEquation::TipX as usize] =
            wall_force_scale.multiply(normal_x)?;
        force_coefficients[JointDynamicEquation::TipZ as usize] =
            wall_force_scale.multiply(normal_z)?;
        normal_force_rhs[constraint] = VerifiedDynamicEquationRhs {
            coefficients: force_coefficients,
        };

        let mut gap_coefficients = [zero; DYNAMIC_VARIABLE_COUNT];
        let mut record_gap = normal_x.negate().multiply(origin_shift)?.divide(dt)?;
        if family.surface == CoupledFixedContactSurface::GrooveWalls {
            record_gap =
                record_gap.subtract(half.multiply(slope)?.multiply(domain.groove_radius_m)?)?;
            gap_coefficients[JointDynamicVelocity::BodyX as usize] =
                slope.multiply(skating_factor)?;
        }
        gap_coefficients[JointDynamicVelocity::Record as usize] = record_gap;
        gap_coefficients[JointDynamicVelocity::TipX as usize] = normal_x;
        gap_coefficients[JointDynamicVelocity::TipZ as usize] = normal_z;
        normal_gap_velocity[constraint] = VerifiedDynamicVelocityRow {
            coefficients: gap_coefficients,
        };
    }
    Ok(VerifiedContactHgOperator {
        constraint_count,
        normal_force_rhs,
        normal_gap_velocity,
    })
}

fn verified_normal_response(
    surface: CoupledFixedContactSurface,
    mobility: VerifiedFixedDynamicMobility,
    operator: VerifiedContactHgOperator,
) -> Result<VerifiedFixedNormalResponse, FixedModeContactBoxError> {
    let zero = OutwardInterval::point(0.0)?;
    let mut w = [[zero; 2]; 2];
    let mut velocity_responses = [[zero; DYNAMIC_VARIABLE_COUNT]; 2];
    for (source, velocity_response) in velocity_responses
        .iter_mut()
        .enumerate()
        .take(operator.constraint_count())
    {
        let force = operator.normal_force_rhs(source);
        for velocity in JOINT_DYNAMIC_VELOCITIES {
            let mut sum = zero;
            for equation in JOINT_DYNAMIC_EQUATIONS {
                let term = mobility
                    .coefficient(velocity, equation)
                    .multiply(force.coefficient(equation))?;
                sum = term.add(sum)?;
            }
            velocity_response[velocity as usize] = sum;
        }
        for (target, response_row) in w.iter_mut().enumerate().take(operator.constraint_count()) {
            let gap = operator.normal_gap_velocity(target);
            let mut sum = zero;
            for velocity in JOINT_DYNAMIC_VELOCITIES {
                let term = gap
                    .coefficient(velocity)
                    .multiply(velocity_response[velocity as usize])?;
                sum = term.add(sum)?;
            }
            response_row[source] = sum;
        }
    }
    match surface {
        CoupledFixedContactSurface::GrooveWalls => {
            let diagonal = [w[0][0], w[1][1]];
            let determinant = w[0][0]
                .multiply(w[1][1])?
                .subtract(w[0][1].multiply(w[1][0])?)?;
            Ok(VerifiedFixedNormalResponse::GrooveWalls(
                VerifiedGrooveNormalResponse {
                    w,
                    diagonal,
                    determinant,
                    diagonal_is_strictly_positive: [
                        diagonal[0].lower() > 0.0,
                        diagonal[1].lower() > 0.0,
                    ],
                    determinant_is_strictly_positive: determinant.lower() > 0.0,
                },
            ))
        }
        CoupledFixedContactSurface::RecordLand => Ok(VerifiedFixedNormalResponse::RecordLand(
            VerifiedLandNormalResponse {
                w: w[0][0],
                is_strictly_positive: w[0][0].lower() > 0.0,
            },
        )),
    }
}

fn classify_contact_box_reachability(
    config: PhysicalPlaybackConfig,
    family: CoupledFixedContactFamily,
    domain: FixedModeContactIntervalBox,
) -> VerifiedContactBoxReachability {
    let friction_coefficient = match family.surface {
        CoupledFixedContactSurface::GrooveWalls => config.contact.groove_friction_coefficient,
        CoupledFixedContactSurface::RecordLand => {
            config.contact.record_surface_friction_coefficient
        }
    };
    if family.stylus == StylusTangentialMode::Separated && friction_coefficient > 0.0 {
        return VerifiedContactBoxReachability::NormalForceToleranceBandOnly;
    }
    if family.stylus != StylusTangentialMode::Sticking
        || family.surface != CoupledFixedContactSurface::GrooveWalls
        || friction_coefficient == 0.0
    {
        return VerifiedContactBoxReachability::RuntimeConditional;
    }
    if domain.wall_slopes.into_iter().all(is_exact_zero) {
        return VerifiedContactBoxReachability::RuntimeConditional;
    }
    VerifiedContactBoxReachability::ActiveWallDependent {
        nonzero_active_wall: CoupledFixedModeInfeasibility::SlopedGrooveStickingWithFriction,
        wall_may_be_nonzero: domain.wall_slopes.map(|slope| !is_exact_zero(slope)),
        wall_is_always_nonzero: domain.wall_slopes.map(|slope| !slope.contains_zero()),
    }
}

fn is_exact_zero(interval: OutwardInterval) -> bool {
    interval.lower() == 0.0 && interval.upper() == 0.0
}

fn verify_base_mobility(
    mechanical_class_index: usize,
    kkt_lhs: CoupledFixedModeKktLhs,
    equality: CoupledFixedEqualityDiagnostics,
    mechanical: CoupledFixedMechanicalMode,
    base_family: CoupledFixedContactFamily,
    production_mobility: CoupledFixedDynamicMobility,
) -> Result<VerifiedFixedBaseMobility, FixedModePointMobilityCertificateError> {
    let verified = verify_mobility(kkt_lhs, production_mobility)
        .map_err(|error| map_base_mobility_error(mechanical_class_index, error))?;
    Ok(VerifiedFixedBaseMobility {
        mechanical,
        base_family,
        kkt_lhs,
        equality,
        reference_mobility: production_mobility,
        verified_mobility: verified.mobility,
        diagnostics: verified.diagnostics,
    })
}

fn verify_solve_only_mobility(
    subject_index: usize,
    subject: CoupledFixedSolveOnlySubject,
    kkt_lhs: CoupledFixedModeKktLhs,
    equality: CoupledFixedEqualityDiagnostics,
    reference_inverse: CoupledFixedKktInverse,
    production_mobility: CoupledFixedDynamicMobility,
) -> Result<VerifiedFixedSolveOnlyMobility, FixedModeSolveOnlyMobilityCertificateError> {
    let verified = verify_complete_kkt_inverse(subject_index, kkt_lhs, &reference_inverse)?;
    if reference_inverse.dynamic_mobility() != production_mobility
        || verified.inverse.dynamic_mobility() != verified.mobility
    {
        return Err(
            FixedModeSolveOnlyMobilityCertificateError::DynamicProjectionMismatch { subject_index },
        );
    }
    Ok(VerifiedFixedSolveOnlyMobility {
        subject,
        kkt_lhs,
        equality,
        reference_inverse,
        verified_inverse: verified.inverse,
        reference_mobility: production_mobility,
        verified_mobility: verified.mobility,
        diagnostics: verified.diagnostics,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct VerifiedCompleteKktInverse {
    inverse: VerifiedFixedKktInverse,
    mobility: VerifiedFixedDynamicMobility,
    diagnostics: VerifiedBaseKktDiagnostics,
}

fn verify_complete_kkt_inverse(
    subject_index: usize,
    kkt_lhs: CoupledFixedModeKktLhs,
    reference_inverse: &CoupledFixedKktInverse,
) -> Result<VerifiedCompleteKktInverse, FixedModeSolveOnlyMobilityCertificateError> {
    if reference_inverse.system_size != kkt_lhs.system_size
        || reference_inverse.equality_count != kkt_lhs.equality_count
        || !reference_inverse.has_valid_inactive_storage()
        || !kkt_lhs_has_valid_inactive_storage(&kkt_lhs)
    {
        return Err(
            FixedModeSolveOnlyMobilityCertificateError::InvalidInverseStorage { subject_index },
        );
    }
    let zero = OutwardInterval::point(0.0).map_err(|source| {
        FixedModeSolveOnlyMobilityCertificateError::KktSolve {
            subject_index,
            rhs_index: 0,
            source: source.into(),
        }
    })?;
    let mut solution_by_rhs = [[zero; KKT_CAPACITY]; KKT_CAPACITY];
    let mut common_path = None;
    let mut aggregate = VerifiedSolveAggregate::default();

    for rhs_index in 0..kkt_lhs.system_size {
        let mut rhs = [0.0; KKT_CAPACITY];
        rhs[rhs_index] = 1.0;
        let rhs_coordinate =
            JointKktRhsCoordinate::from_active_index(rhs_index, kkt_lhs.equality_count).ok_or(
                FixedModeSolveOnlyMobilityCertificateError::InvalidInverseStorage { subject_index },
            )?;
        let solved = verify_point_kkt_system(kkt_lhs.coefficients, kkt_lhs.system_size, rhs)
            .map_err(
                |source| FixedModeSolveOnlyMobilityCertificateError::KktSolve {
                    subject_index,
                    rhs_index,
                    source,
                },
            )?;
        let path = VerifiedEliminationPath::from(solved.diagnostics);
        if let Some(expected) = common_path {
            if expected != path {
                return Err(
                    FixedModeSolveOnlyMobilityCertificateError::InconsistentSolvePath {
                        subject_index,
                    },
                );
            }
        } else {
            common_path = Some(path);
        }
        aggregate.include(solved.diagnostics);

        for (solution_index, solution_row) in solution_by_rhs
            .iter_mut()
            .enumerate()
            .take(kkt_lhs.system_size)
        {
            let solution_coordinate = JointKktSolutionCoordinate::from_active_index(
                solution_index,
                kkt_lhs.equality_count,
            )
            .ok_or(
                FixedModeSolveOnlyMobilityCertificateError::InvalidInverseStorage { subject_index },
            )?;
            let production = reference_inverse
                .coefficient(solution_coordinate, rhs_coordinate)
                .ok_or(
                    FixedModeSolveOnlyMobilityCertificateError::InvalidInverseStorage {
                        subject_index,
                    },
                )?;
            if solved.reference_solution[solution_index].to_bits() != production.to_bits() {
                return Err(
                    FixedModeSolveOnlyMobilityCertificateError::ProductionInverseReplayMismatch {
                        subject_index,
                        solution_index,
                        rhs_index,
                    },
                );
            }
            let enclosure = solved.solution[solution_index];
            if !enclosure.contains(production) {
                return Err(
                    FixedModeSolveOnlyMobilityCertificateError::ProductionInverseNotEnclosed {
                        subject_index,
                        solution_index,
                        rhs_index,
                    },
                );
            }
            solution_row[rhs_index] = enclosure;
        }
    }

    let path = common_path.ok_or(FixedModeSolveOnlyMobilityCertificateError::InvalidCatalog)?;
    let inverse = VerifiedFixedKktInverse {
        system_size: kkt_lhs.system_size,
        equality_count: kkt_lhs.equality_count,
        solution_by_rhs,
    };
    if !inverse.has_valid_inactive_storage() {
        return Err(
            FixedModeSolveOnlyMobilityCertificateError::InvalidInverseStorage { subject_index },
        );
    }
    Ok(VerifiedCompleteKktInverse {
        inverse,
        mobility: inverse.dynamic_mobility(),
        diagnostics: aggregate.finish(kkt_lhs.equality_count, path),
    })
}

fn kkt_lhs_has_valid_inactive_storage(kkt_lhs: &CoupledFixedModeKktLhs) -> bool {
    kkt_lhs.system_size >= DYNAMIC_VARIABLE_COUNT
        && kkt_lhs.system_size <= KKT_CAPACITY
        && kkt_lhs.equality_count == kkt_lhs.system_size - DYNAMIC_VARIABLE_COUNT
        && kkt_lhs
            .coefficients
            .iter()
            .enumerate()
            .all(|(row, values)| {
                values.iter().enumerate().all(|(column, value)| {
                    (row < kkt_lhs.system_size && column < kkt_lhs.system_size)
                        || value.to_bits() == 0.0_f64.to_bits()
                })
            })
        && kkt_lhs
            .equality_basis
            .iter()
            .skip(kkt_lhs.equality_count)
            .all(|basis| {
                basis
                    .coefficients_in_velocity_column_order()
                    .into_iter()
                    .all(|value| value.to_bits() == 0.0_f64.to_bits())
            })
}

#[derive(Debug)]
enum MobilityVerificationError {
    InvalidCatalog,
    KktSolve {
        equation_index: usize,
        source: VerifiedPointKktError,
    },
    InconsistentSolvePath,
    ProductionReplayMismatch {
        velocity_index: usize,
        equation_index: usize,
    },
    ProductionNotEnclosed {
        velocity_index: usize,
        equation_index: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct VerifiedMobility {
    mobility: VerifiedFixedDynamicMobility,
    diagnostics: VerifiedBaseKktDiagnostics,
}

fn verify_mobility(
    kkt_lhs: CoupledFixedModeKktLhs,
    production_mobility: CoupledFixedDynamicMobility,
) -> Result<VerifiedMobility, MobilityVerificationError> {
    if kkt_lhs.system_size < DYNAMIC_VARIABLE_COUNT
        || kkt_lhs.system_size > KKT_CAPACITY
        || kkt_lhs.equality_count != kkt_lhs.system_size - DYNAMIC_VARIABLE_COUNT
    {
        return Err(MobilityVerificationError::KktSolve {
            equation_index: 0,
            source: VerifiedPointKktError::InvalidSize,
        });
    }

    let zero =
        OutwardInterval::point(0.0).map_err(|source| MobilityVerificationError::KktSolve {
            equation_index: 0,
            source: source.into(),
        })?;
    let mut verified_mobility = VerifiedFixedDynamicMobility {
        velocity_by_equation_rhs: [[zero; DYNAMIC_VARIABLE_COUNT]; DYNAMIC_VARIABLE_COUNT],
    };
    let mut common_path = None;
    let mut aggregate = VerifiedSolveAggregate::default();

    for equation in JOINT_DYNAMIC_EQUATIONS {
        let equation_index = equation as usize;
        let mut rhs = [0.0; KKT_CAPACITY];
        rhs[equation_index] = 1.0;
        let solved = verify_point_kkt_system(kkt_lhs.coefficients, kkt_lhs.system_size, rhs)
            .map_err(|source| MobilityVerificationError::KktSolve {
                equation_index,
                source,
            })?;
        let path = VerifiedEliminationPath::from(solved.diagnostics);
        if let Some(expected) = common_path {
            if expected != path {
                return Err(MobilityVerificationError::InconsistentSolvePath);
            }
        } else {
            common_path = Some(path);
        }
        aggregate.include(solved.diagnostics);

        for velocity in JOINT_DYNAMIC_VELOCITIES {
            let velocity_index = velocity as usize;
            let production = production_mobility.coefficient(velocity, equation);
            let replay = solved.reference_solution[velocity_index];
            if replay.to_bits() != production.to_bits() {
                return Err(MobilityVerificationError::ProductionReplayMismatch {
                    velocity_index,
                    equation_index,
                });
            }
            let enclosure = solved.solution[velocity_index];
            if !enclosure.contains(production) {
                return Err(MobilityVerificationError::ProductionNotEnclosed {
                    velocity_index,
                    equation_index,
                });
            }
            verified_mobility.velocity_by_equation_rhs[velocity_index][equation_index] = enclosure;
        }
    }

    let path = common_path.ok_or(MobilityVerificationError::InvalidCatalog)?;
    Ok(VerifiedMobility {
        mobility: verified_mobility,
        diagnostics: aggregate.finish(kkt_lhs.equality_count, path),
    })
}

fn map_base_mobility_error(
    mechanical_class_index: usize,
    error: MobilityVerificationError,
) -> FixedModePointMobilityCertificateError {
    match error {
        MobilityVerificationError::InvalidCatalog => {
            FixedModePointMobilityCertificateError::InvalidCatalog
        }
        MobilityVerificationError::KktSolve {
            equation_index,
            source,
        } => FixedModePointMobilityCertificateError::KktSolve {
            mechanical_class_index,
            equation_index,
            source,
        },
        MobilityVerificationError::InconsistentSolvePath => {
            FixedModePointMobilityCertificateError::InconsistentSolvePath {
                mechanical_class_index,
            }
        }
        MobilityVerificationError::ProductionReplayMismatch {
            velocity_index,
            equation_index,
        } => FixedModePointMobilityCertificateError::ProductionReplayMismatch {
            mechanical_class_index,
            velocity_index,
            equation_index,
        },
        MobilityVerificationError::ProductionNotEnclosed {
            velocity_index,
            equation_index,
        } => FixedModePointMobilityCertificateError::ProductionNotEnclosed {
            mechanical_class_index,
            velocity_index,
            equation_index,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct VerifiedLinearSolve {
    reference_solution: [f64; KKT_CAPACITY],
    solution: [OutwardInterval; KKT_CAPACITY],
    diagnostics: VerifiedSingleSolveDiagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct VerifiedSingleSolveDiagnostics {
    system_size: usize,
    row_scales: [f64; KKT_CAPACITY],
    column_scales: [f64; KKT_CAPACITY],
    pivot_rows: [usize; KKT_CAPACITY],
    minimum_verified_scaled_pivot: f64,
    maximum_verified_scaled_pivot_width: f64,
    maximum_dynamic_solution_width: f64,
    maximum_full_solution_width: f64,
    maximum_residual_width: f64,
    maximum_residual_absolute_bound: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct VerifiedEliminationPath {
    system_size: usize,
    row_scales: [f64; KKT_CAPACITY],
    column_scales: [f64; KKT_CAPACITY],
    pivot_rows: [usize; KKT_CAPACITY],
    minimum_verified_scaled_pivot: f64,
    maximum_verified_scaled_pivot_width: f64,
}

impl From<VerifiedSingleSolveDiagnostics> for VerifiedEliminationPath {
    fn from(diagnostics: VerifiedSingleSolveDiagnostics) -> Self {
        Self {
            system_size: diagnostics.system_size,
            row_scales: diagnostics.row_scales,
            column_scales: diagnostics.column_scales,
            pivot_rows: diagnostics.pivot_rows,
            minimum_verified_scaled_pivot: diagnostics.minimum_verified_scaled_pivot,
            maximum_verified_scaled_pivot_width: diagnostics.maximum_verified_scaled_pivot_width,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct VerifiedSolveAggregate {
    maximum_dynamic_solution_width: f64,
    maximum_full_solution_width: f64,
    maximum_residual_width: f64,
    maximum_residual_absolute_bound: f64,
}

impl VerifiedSolveAggregate {
    fn include(&mut self, diagnostics: VerifiedSingleSolveDiagnostics) {
        self.maximum_dynamic_solution_width = self
            .maximum_dynamic_solution_width
            .max(diagnostics.maximum_dynamic_solution_width);
        self.maximum_full_solution_width = self
            .maximum_full_solution_width
            .max(diagnostics.maximum_full_solution_width);
        self.maximum_residual_width = self
            .maximum_residual_width
            .max(diagnostics.maximum_residual_width);
        self.maximum_residual_absolute_bound = self
            .maximum_residual_absolute_bound
            .max(diagnostics.maximum_residual_absolute_bound);
    }

    fn finish(
        self,
        equality_count: usize,
        path: VerifiedEliminationPath,
    ) -> VerifiedBaseKktDiagnostics {
        VerifiedBaseKktDiagnostics {
            system_size: path.system_size,
            equality_count,
            row_scales: path.row_scales,
            column_scales: path.column_scales,
            pivot_rows: path.pivot_rows,
            minimum_verified_scaled_pivot: path.minimum_verified_scaled_pivot,
            maximum_verified_scaled_pivot_width: path.maximum_verified_scaled_pivot_width,
            maximum_dynamic_solution_width: self.maximum_dynamic_solution_width,
            maximum_full_solution_width: self.maximum_full_solution_width,
            maximum_residual_width: self.maximum_residual_width,
            maximum_residual_absolute_bound: self.maximum_residual_absolute_bound,
        }
    }
}

fn verify_point_kkt_system(
    matrix: [[f64; KKT_CAPACITY]; KKT_CAPACITY],
    size: usize,
    rhs: [f64; KKT_CAPACITY],
) -> Result<VerifiedLinearSolve, VerifiedPointKktError> {
    if size == 0 || size > KKT_CAPACITY {
        return Err(VerifiedPointKktError::InvalidSize);
    }
    if matrix
        .iter()
        .take(size)
        .flat_map(|row| row.iter().take(size))
        .chain(rhs.iter().take(size))
        .any(|value| !value.is_finite())
    {
        return Err(VerifiedPointKktError::NonFiniteInput);
    }

    let zero = OutwardInterval::point(0.0)?;
    let one = OutwardInterval::point(1.0)?;
    let mut reference = [[0.0; AUGMENTED_COLUMN_COUNT]; KKT_CAPACITY];
    let mut verified = [[zero; AUGMENTED_COLUMN_COUNT]; KKT_CAPACITY];
    for row in 0..size {
        reference[row][..size].copy_from_slice(&matrix[row][..size]);
        reference[row][RHS_COLUMN] = rhs[row];
        for column in 0..size {
            verified[row][column] = OutwardInterval::point(matrix[row][column])?;
        }
        verified[row][RHS_COLUMN] = OutwardInterval::point(rhs[row])?;
    }

    let original_matrix = matrix;
    let mut row_scales = [1.0; KKT_CAPACITY];
    for row in 0..size {
        let row_scale = reference[row][..size]
            .iter()
            .map(|value| value.abs())
            .fold(0.0_f64, f64::max);
        if !row_scale.is_finite() || row_scale == 0.0 {
            return Err(VerifiedPointKktError::ZeroScale);
        }
        row_scales[row] = row_scale;
        let verified_scale = OutwardInterval::point(row_scale)?;
        for column in 0..size {
            reference[row][column] /= row_scale;
            verified[row][column] = verified[row][column].divide(verified_scale)?;
        }
        reference[row][RHS_COLUMN] /= row_scale;
        verified[row][RHS_COLUMN] = verified[row][RHS_COLUMN].divide(verified_scale)?;
    }

    let mut column_scales = [1.0; KKT_CAPACITY];
    for column in 0..size {
        let column_scale = reference[..size]
            .iter()
            .map(|row| row[column].abs())
            .fold(0.0_f64, f64::max);
        if !column_scale.is_finite() || column_scale == 0.0 {
            return Err(VerifiedPointKktError::ZeroScale);
        }
        column_scales[column] = column_scale;
        let verified_scale = OutwardInterval::point(column_scale)?;
        for row in 0..size {
            reference[row][column] /= column_scale;
            verified[row][column] = verified[row][column].divide(verified_scale)?;
        }
    }

    let relative_pivot_tolerance = REFERENCE_RELATIVE_PIVOT_FACTOR * f64::EPSILON * size as f64;
    let mut pivot_rows = [UNUSED_PIVOT_ROW; KKT_CAPACITY];
    let mut minimum_verified_scaled_pivot = f64::INFINITY;
    let mut maximum_verified_scaled_pivot_width = 0.0_f64;
    for pivot_column in 0..size {
        let pivot_row = (pivot_column..size)
            .max_by(|left, right| {
                reference[*left][pivot_column]
                    .abs()
                    .total_cmp(&reference[*right][pivot_column].abs())
            })
            .ok_or(VerifiedPointKktError::ReferencePivotRejected)?;
        let reference_pivot = reference[pivot_row][pivot_column];
        if !reference_pivot.is_finite() || reference_pivot.abs() <= relative_pivot_tolerance {
            return Err(VerifiedPointKktError::ReferencePivotRejected);
        }
        let verified_pivot = verified[pivot_row][pivot_column];
        let minimum_verified_pivot =
            minimum_admitted_verified_pivot(verified_pivot, relative_pivot_tolerance)?;
        minimum_verified_scaled_pivot = minimum_verified_scaled_pivot.min(minimum_verified_pivot);
        maximum_verified_scaled_pivot_width =
            maximum_verified_scaled_pivot_width.max(verified_pivot.width()?);
        pivot_rows[pivot_column] = pivot_row;

        reference.swap(pivot_column, pivot_row);
        verified.swap(pivot_column, pivot_row);
        let verified_pivot = verified[pivot_column][pivot_column];
        for column in pivot_column..size {
            reference[pivot_column][column] /= reference_pivot;
            if column == pivot_column {
                // The numerator and denominator are the same exact intermediate.
                verified[pivot_column][column] = one;
            } else {
                verified[pivot_column][column] =
                    verified[pivot_column][column].divide(verified_pivot)?;
            }
        }
        reference[pivot_column][RHS_COLUMN] /= reference_pivot;
        verified[pivot_column][RHS_COLUMN] =
            verified[pivot_column][RHS_COLUMN].divide(verified_pivot)?;
        let reference_pivot_values = reference[pivot_column];
        let verified_pivot_values = verified[pivot_column];
        for row in 0..size {
            if row == pivot_column {
                continue;
            }
            let reference_scale = reference[row][pivot_column];
            let verified_scale = verified[row][pivot_column];
            for column in pivot_column..size {
                reference[row][column] -= reference_scale * reference_pivot_values[column];
                if column == pivot_column {
                    // The target coefficient and the scale are the same value.
                    verified[row][column] = zero;
                } else {
                    verified[row][column] = verified[row][column]
                        .subtract(verified_scale.multiply(verified_pivot_values[column])?)?;
                }
            }
            reference[row][RHS_COLUMN] -= reference_scale * reference_pivot_values[RHS_COLUMN];
            verified[row][RHS_COLUMN] = verified[row][RHS_COLUMN]
                .subtract(verified_scale.multiply(verified_pivot_values[RHS_COLUMN])?)?;
        }
    }

    let mut reference_solution = [0.0; KKT_CAPACITY];
    let mut solution = [zero; KKT_CAPACITY];
    let mut maximum_dynamic_solution_width = 0.0_f64;
    let mut maximum_full_solution_width = 0.0_f64;
    for row in 0..size {
        reference_solution[row] = reference[row][RHS_COLUMN] / column_scales[row];
        solution[row] =
            verified[row][RHS_COLUMN].divide(OutwardInterval::point(column_scales[row])?)?;
        if !solution[row].contains(reference_solution[row]) {
            return Err(VerifiedPointKktError::PointSolutionNotEnclosed);
        }
        let width = solution[row].width()?;
        maximum_full_solution_width = maximum_full_solution_width.max(width);
        if row < DYNAMIC_VARIABLE_COUNT {
            maximum_dynamic_solution_width = maximum_dynamic_solution_width.max(width);
        }
    }

    let mut maximum_residual_width = 0.0_f64;
    let mut maximum_residual_absolute_bound = 0.0_f64;
    for row in 0..size {
        let mut residual = zero;
        for column in 0..size {
            residual = residual.add(
                OutwardInterval::point(original_matrix[row][column])?.multiply(solution[column])?,
            )?;
        }
        residual = residual.subtract(OutwardInterval::point(rhs[row])?)?;
        if !residual.contains_zero() {
            return Err(VerifiedPointKktError::ResidualNotEnclosed);
        }
        maximum_residual_width = maximum_residual_width.max(residual.width()?);
        maximum_residual_absolute_bound =
            maximum_residual_absolute_bound.max(residual.maximum_absolute());
    }

    Ok(VerifiedLinearSolve {
        reference_solution,
        solution,
        diagnostics: VerifiedSingleSolveDiagnostics {
            system_size: size,
            row_scales,
            column_scales,
            pivot_rows,
            minimum_verified_scaled_pivot,
            maximum_verified_scaled_pivot_width,
            maximum_dynamic_solution_width,
            maximum_full_solution_width,
            maximum_residual_width,
            maximum_residual_absolute_bound,
        },
    })
}

fn minimum_admitted_verified_pivot(
    pivot: OutwardInterval,
    tolerance: f64,
) -> Result<f64, VerifiedPointKktError> {
    if pivot.contains_zero() {
        return Err(VerifiedPointKktError::VerifiedPivotContainsZero);
    }
    let minimum = pivot.minimum_absolute();
    if minimum <= tolerance {
        return Err(VerifiedPointKktError::VerifiedPivotBelowTolerance);
    }
    Ok(minimum)
}

#[cfg(test)]
mod tests {
    use super::super::contact::{
        CoupledFixedModeReachability, CoupledFixedNormalResponse, FixedModeFrictionMobility,
        FixedModeHandMobility,
    };
    use super::*;
    use num_rational::BigRational;

    fn seed_config() -> PhysicalPlaybackConfig {
        super::super::PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed().config
    }

    fn assert_point_response_enclosed(
        evaluation: VerifiedFixedContactBoxEvaluation,
        response: super::super::contact::CoupledFixedModeNormalResponse,
    ) {
        assert_eq!(evaluation.family, response.family);
        assert!(evaluation
            .skating_factor
            .contains(response.derived.skating_factor));
        assert_eq!(
            evaluation.runtime_rhs_compatibility_required,
            response.equality.runtime_rhs_compatibility_required
        );
        match (
            evaluation.stylus_equality,
            response.family.stylus,
            fixed_stylus_constraint_relation(
                response.family.mechanical,
                response.derived.stylus_body_velocity_coefficient,
            ),
        ) {
            (
                VerifiedStylusEqualityDisposition::NotRequested,
                StylusTangentialMode::Separated
                | StylusTangentialMode::SlidingPositive
                | StylusTangentialMode::SlidingNegative,
                _,
            ) => {}
            (
                VerifiedStylusEqualityDisposition::AddsRank { schur_denominator },
                StylusTangentialMode::Sticking,
                Some(FixedStylusConstraintRelation::AddsRank),
            ) => assert!(!schur_denominator.contains_zero()),
            (
                VerifiedStylusEqualityDisposition::DependentRuntimeRhs,
                StylusTangentialMode::Sticking,
                Some(FixedStylusConstraintRelation::DependentRuntimeRhs),
            ) => assert!(evaluation.runtime_rhs_compatibility_required),
            disposition => panic!("unexpected stylus equality disposition: {disposition:?}"),
        }
        if contact_box_point(evaluation.domain).is_some() {
            for velocity in JOINT_DYNAMIC_VELOCITIES {
                for equation in JOINT_DYNAMIC_EQUATIONS {
                    assert!(evaluation
                        .effective_mobility
                        .coefficient(velocity, equation)
                        .contains(response.dynamic_mobility.coefficient(velocity, equation)));
                }
            }
        }
        let expected_reachability = match response.reachability {
            CoupledFixedModeReachability::RuntimeConditional => {
                VerifiedContactBoxReachability::RuntimeConditional
            }
            CoupledFixedModeReachability::NormalForceToleranceBandOnly => {
                VerifiedContactBoxReachability::NormalForceToleranceBandOnly
            }
            CoupledFixedModeReachability::ActiveWallDependent {
                nonzero_active_wall,
                wall_is_nonzero,
            } => VerifiedContactBoxReachability::ActiveWallDependent {
                nonzero_active_wall,
                wall_may_be_nonzero: wall_is_nonzero,
                wall_is_always_nonzero: wall_is_nonzero,
            },
        };
        assert_eq!(evaluation.reachability, expected_reachability);

        assert_eq!(
            evaluation.contact_operator.constraint_count(),
            response.contact_operator.constraint_count()
        );
        for constraint in 0..evaluation.contact_operator.constraint_count() {
            let verified_force = evaluation.contact_operator.normal_force_rhs(constraint);
            let production_force = response.contact_operator.normal_force_rhs(constraint);
            for equation in JOINT_DYNAMIC_EQUATIONS {
                assert!(verified_force
                    .coefficient(equation)
                    .contains(production_force.coefficient(equation)));
            }
            let verified_gap = evaluation.contact_operator.normal_gap_velocity(constraint);
            let production_gap = response.contact_operator.normal_gap_velocity(constraint);
            for velocity in JOINT_DYNAMIC_VELOCITIES {
                assert!(verified_gap
                    .coefficient(velocity)
                    .contains(production_gap.coefficient(velocity)));
            }
        }

        match (evaluation.normal_response, response.normal_response) {
            (
                VerifiedFixedNormalResponse::GrooveWalls(verified),
                CoupledFixedNormalResponse::GrooveWalls { w },
            ) => {
                for (verified_row, production_row) in verified.w.iter().zip(w) {
                    for (verified_value, production_value) in
                        verified_row.iter().zip(production_row)
                    {
                        assert!(verified_value.contains(production_value));
                    }
                }
                let determinant = w[0][0] * w[1][1] - w[0][1] * w[1][0];
                assert!(verified.determinant.contains(determinant));
            }
            (
                VerifiedFixedNormalResponse::RecordLand(verified),
                CoupledFixedNormalResponse::RecordLand { w },
            ) => assert!(verified.w.contains(w)),
            (verified, production) => {
                panic!("surface mismatch: verified={verified:?}, production={production:?}")
            }
        }
    }

    fn exact(value: f64) -> BigRational {
        BigRational::from_float(value).expect("finite dyadic test value")
    }

    fn exact_solve(
        matrix: [[f64; KKT_CAPACITY]; KKT_CAPACITY],
        size: usize,
        rhs: [f64; KKT_CAPACITY],
    ) -> Vec<BigRational> {
        let mut augmented = vec![vec![exact(0.0); size + 1]; size];
        for row in 0..size {
            for column in 0..size {
                augmented[row][column] = exact(matrix[row][column]);
            }
            augmented[row][size] = exact(rhs[row]);
        }
        for pivot_column in 0..size {
            let pivot_row = (pivot_column..size)
                .find(|row| augmented[*row][pivot_column] != exact(0.0))
                .expect("the dyadic test matrix must be nonsingular");
            augmented.swap(pivot_column, pivot_row);
            let pivot = augmented[pivot_column][pivot_column].clone();
            for value in &mut augmented[pivot_column][pivot_column..=size] {
                *value /= &pivot;
            }
            let pivot_values = augmented[pivot_column].clone();
            for (row, row_values) in augmented.iter_mut().enumerate() {
                if row == pivot_column {
                    continue;
                }
                let scale = row_values[pivot_column].clone();
                for column in pivot_column..=size {
                    row_values[column] -= &scale * &pivot_values[column];
                }
            }
        }
        augmented.into_iter().map(|row| row[size].clone()).collect()
    }

    fn assert_contains_exact(interval: OutwardInterval, value: &BigRational) {
        let lower = exact(interval.lower());
        let upper = exact(interval.upper());
        assert!(lower <= *value, "lower bound exceeds {value}");
        assert!(upper >= *value, "upper bound is below {value}");
    }

    #[test]
    fn dyadic_scaled_elimination_encloses_exact_solutions() {
        let mut matrix = [[0.0; KKT_CAPACITY]; KKT_CAPACITY];
        let coefficients = [
            [0.5, -0.25, 0.0, 1.0],
            [2.0, 4.0, 1.0, 0.0],
            [0.0, 1.0, 8.0, -2.0],
            [1.0, 0.0, -0.5, 2.0],
        ];
        for (row, coefficients) in coefficients.into_iter().enumerate() {
            matrix[row][..4].copy_from_slice(&coefficients);
        }
        for rhs_column in 0..4 {
            let mut rhs = [0.0; KKT_CAPACITY];
            rhs[rhs_column] = 1.0;
            let solved = verify_point_kkt_system(matrix, 4, rhs).unwrap();
            let oracle = exact_solve(matrix, 4, rhs);
            for (interval, exact_value) in solved.solution[..4].iter().zip(oracle.iter()) {
                assert_contains_exact(*interval, exact_value);
            }
            assert!(solved.diagnostics.minimum_verified_scaled_pivot > 0.0);
            assert!(solved.diagnostics.maximum_residual_width.is_finite());
        }
    }

    #[test]
    fn dyadic_diagonal_system_keeps_exact_power_of_two_solutions() {
        let mut matrix = [[0.0; KKT_CAPACITY]; KKT_CAPACITY];
        matrix[0][0] = 0.25;
        matrix[1][1] = -2.0;
        matrix[2][2] = 8.0;
        let mut rhs = [0.0; KKT_CAPACITY];
        rhs[..3].copy_from_slice(&[1.0, 1.0, 1.0]);
        let solved = verify_point_kkt_system(matrix, 3, rhs).unwrap();
        for (interval, expected) in solved.solution[..3].iter().zip([4.0, -0.5, 0.125]) {
            assert_contains_exact(*interval, &exact(expected));
            assert!(interval.width().unwrap() <= 16.0 * f64::EPSILON * expected.abs());
        }
    }

    #[test]
    fn singular_point_system_fails_closed() {
        let mut matrix = [[0.0; KKT_CAPACITY]; KKT_CAPACITY];
        matrix[0][..2].copy_from_slice(&[1.0, 1.0]);
        matrix[1][..2].copy_from_slice(&[2.0, 2.0]);
        let mut rhs = [0.0; KKT_CAPACITY];
        rhs[0] = 1.0;
        assert_eq!(
            verify_point_kkt_system(matrix, 2, rhs),
            Err(VerifiedPointKktError::ReferencePivotRejected)
        );
    }

    #[test]
    fn verified_pivot_requires_its_complete_interval_to_clear_tolerance() {
        let tolerance = 1.0e-12;
        assert_eq!(
            minimum_admitted_verified_pivot(
                OutwardInterval::hull(-2.0 * tolerance, 2.0 * tolerance).unwrap(),
                tolerance,
            ),
            Err(VerifiedPointKktError::VerifiedPivotContainsZero)
        );
        assert_eq!(
            minimum_admitted_verified_pivot(
                OutwardInterval::hull(tolerance, 2.0 * tolerance).unwrap(),
                tolerance,
            ),
            Err(VerifiedPointKktError::VerifiedPivotBelowTolerance)
        );
        assert_eq!(
            minimum_admitted_verified_pivot(
                OutwardInterval::hull(2.0 * tolerance, 3.0 * tolerance).unwrap(),
                tolerance,
            ),
            Ok(2.0 * tolerance)
        );
    }

    #[test]
    fn all_24_production_base_mobilities_are_point_enclosed() {
        let config = seed_config();
        let point = CoupledFixedContactPoint {
            groove_radius_m: config.groove.outer_program_radius_m,
            wall_slopes: [0.0; 2],
        };
        let catalog = verify_fixed_base_mobilities_at_point(config, point).unwrap();
        assert_eq!(
            catalog.certificate_version,
            FIXED_MODE_POINT_MOBILITY_CERTIFICATE_VERSION
        );
        assert_eq!(catalog.systems.len(), COUPLED_FIXED_MECHANICAL_CLASS_COUNT);
        assert_eq!(catalog.config_identity, config.identity().unwrap());
        assert_eq!(catalog.config_identity_version, 1);
        assert_eq!(catalog.config_sha256, catalog.config_identity.sha256());
        for system in catalog.systems {
            let production_pivot_tolerance = REFERENCE_RELATIVE_PIVOT_FACTOR
                * f64::EPSILON
                * system.diagnostics.system_size as f64;
            assert!(system.diagnostics.minimum_verified_scaled_pivot > production_pivot_tolerance);
            assert!(system
                .diagnostics
                .maximum_dynamic_solution_width
                .is_finite());
            assert!(system.diagnostics.maximum_residual_width.is_finite());
            for velocity in JOINT_DYNAMIC_VELOCITIES {
                for equation in JOINT_DYNAMIC_EQUATIONS {
                    let production = system.reference_mobility.coefficient(velocity, equation);
                    assert!(system
                        .verified_mobility
                        .coefficient(velocity, equation)
                        .contains(production));
                }
            }
        }
    }

    #[test]
    fn all_48_solve_only_mobilities_are_verified_and_lookup_is_exact() {
        let config = seed_config();
        let catalog = verify_fixed_solve_only_mobilities(config).unwrap();
        let subjects: Vec<_> = coupled_fixed_solve_only_subjects().collect();
        assert_eq!(
            catalog.certificate_version,
            FIXED_MODE_SOLVE_ONLY_MOBILITY_CERTIFICATE_VERSION
        );
        assert_eq!(
            catalog.subject_set_version,
            COUPLED_FIXED_SOLVE_ONLY_SUBJECT_SET_VERSION
        );
        assert_eq!(
            catalog.systems.len(),
            COUPLED_FIXED_SOLVE_ONLY_SUBJECT_COUNT
        );
        assert_eq!(catalog.config_identity, config.identity().unwrap());
        assert_eq!(catalog.config_identity_version, 1);
        assert_eq!(catalog.config_sha256, catalog.config_identity.sha256());
        for (index, system) in catalog.systems.iter().enumerate() {
            assert_eq!(system.subject, subjects[index]);
            assert_eq!(catalog.get(subjects[index]), Some(system));
            let response = coupled_fixed_solve_only_response(config, system.subject).unwrap();
            assert_eq!(system.reference_inverse, response.kkt_inverse);
            assert_eq!(
                system.reference_inverse.system_size,
                system.kkt_lhs.system_size
            );
            assert_eq!(
                system.reference_inverse.equality_count,
                system.kkt_lhs.equality_count
            );
            assert_eq!(
                system.verified_inverse.system_size,
                system.kkt_lhs.system_size
            );
            assert_eq!(
                system.verified_inverse.equality_count,
                system.kkt_lhs.equality_count
            );
            assert!(system.reference_inverse.has_valid_inactive_storage());
            assert!(system.verified_inverse.has_valid_inactive_storage());
            let production_pivot_tolerance = REFERENCE_RELATIVE_PIVOT_FACTOR
                * f64::EPSILON
                * system.diagnostics.system_size as f64;
            assert!(system.diagnostics.minimum_verified_scaled_pivot > production_pivot_tolerance);
            let mut common_path = None;
            for rhs_index in 0..system.kkt_lhs.system_size {
                let rhs_coordinate = JointKktRhsCoordinate::from_active_index(
                    rhs_index,
                    system.kkt_lhs.equality_count,
                )
                .unwrap();
                let mut rhs = [0.0; KKT_CAPACITY];
                rhs[rhs_index] = 1.0;
                let replay = verify_point_kkt_system(
                    system.kkt_lhs.coefficients,
                    system.kkt_lhs.system_size,
                    rhs,
                )
                .unwrap();
                let path = VerifiedEliminationPath::from(replay.diagnostics);
                if let Some(expected) = common_path {
                    assert_eq!(path, expected);
                } else {
                    common_path = Some(path);
                }
                for solution_index in 0..system.kkt_lhs.system_size {
                    let solution_coordinate = JointKktSolutionCoordinate::from_active_index(
                        solution_index,
                        system.kkt_lhs.equality_count,
                    )
                    .unwrap();
                    let production = system
                        .reference_inverse
                        .coefficient(solution_coordinate, rhs_coordinate)
                        .unwrap();
                    let verified = system
                        .verified_inverse
                        .coefficient(solution_coordinate, rhs_coordinate)
                        .unwrap();
                    assert_eq!(
                        replay.reference_solution[solution_index].to_bits(),
                        production.to_bits()
                    );
                    assert!(verified.contains(production));
                }
            }
            let common_path = common_path.unwrap();
            assert_eq!(common_path.system_size, system.diagnostics.system_size);
            assert_eq!(common_path.row_scales, system.diagnostics.row_scales);
            assert_eq!(common_path.column_scales, system.diagnostics.column_scales);
            assert_eq!(common_path.pivot_rows, system.diagnostics.pivot_rows);
            assert_eq!(
                common_path.minimum_verified_scaled_pivot,
                system.diagnostics.minimum_verified_scaled_pivot
            );
            assert_eq!(
                common_path.maximum_verified_scaled_pivot_width,
                system.diagnostics.maximum_verified_scaled_pivot_width
            );
            for row in 0..KKT_CAPACITY {
                for column in 0..KKT_CAPACITY {
                    if row >= system.kkt_lhs.system_size || column >= system.kkt_lhs.system_size {
                        let reference = system.reference_inverse.solution_by_rhs[row][column];
                        let verified = system.verified_inverse.solution_by_rhs[row][column];
                        assert_eq!(reference.to_bits(), 0.0_f64.to_bits());
                        assert_eq!(verified.lower(), 0.0);
                        assert_eq!(verified.upper(), 0.0);
                    }
                }
            }
            assert_eq!(
                system.reference_inverse.dynamic_mobility(),
                system.reference_mobility
            );
            assert_eq!(
                system.verified_inverse.dynamic_mobility(),
                system.verified_mobility
            );
            for velocity in JOINT_DYNAMIC_VELOCITIES {
                for equation in JOINT_DYNAMIC_EQUATIONS {
                    let production = system.reference_mobility.coefficient(velocity, equation);
                    assert!(system
                        .verified_mobility
                        .coefficient(velocity, equation)
                        .contains(production));
                }
            }
        }
    }

    #[test]
    fn solve_only_verifier_rejects_nonzero_inactive_storage() {
        let config = seed_config();
        let subject = coupled_fixed_solve_only_subjects().next().unwrap();
        let response = coupled_fixed_solve_only_response(config, subject).unwrap();
        assert!(response.kkt_lhs.system_size < KKT_CAPACITY);

        let mut inverse = response.kkt_inverse;
        inverse.solution_by_rhs[KKT_CAPACITY - 1][KKT_CAPACITY - 1] = 1.0;
        assert!(matches!(
            verify_solve_only_mobility(
                0,
                subject,
                response.kkt_lhs,
                response.equality,
                inverse,
                response.dynamic_mobility,
            ),
            Err(
                FixedModeSolveOnlyMobilityCertificateError::InvalidInverseStorage {
                    subject_index: 0
                }
            )
        ));

        let mut kkt_lhs = response.kkt_lhs;
        kkt_lhs.coefficients[KKT_CAPACITY - 1][KKT_CAPACITY - 1] = 1.0;
        assert!(matches!(
            verify_solve_only_mobility(
                0,
                subject,
                kkt_lhs,
                response.equality,
                response.kkt_inverse,
                response.dynamic_mobility,
            ),
            Err(
                FixedModeSolveOnlyMobilityCertificateError::InvalidInverseStorage {
                    subject_index: 0
                }
            )
        ));
    }

    #[test]
    fn solve_only_lowered_lookup_matches_the_existing_base_bits() {
        let config = seed_config();
        let point = CoupledFixedContactPoint {
            groove_radius_m: config.groove.inner_program_radius_m,
            wall_slopes: [0.0; 2],
        };
        let base = verify_fixed_base_mobilities_at_point(config, point).unwrap();
        let solve_only = verify_fixed_solve_only_mobilities(config).unwrap();
        for base_system in base.systems {
            let lowered = solve_only.lowered_base(base_system.mechanical).unwrap();
            assert_eq!(
                lowered.subject.support,
                CoupledFixedPickupSupport::LoweredNoContact
            );
            assert_eq!(lowered.kkt_lhs, base_system.kkt_lhs);
            assert_eq!(lowered.equality, base_system.equality);
            assert_eq!(lowered.reference_mobility, base_system.reference_mobility);
            assert_eq!(lowered.verified_mobility, base_system.verified_mobility);
            assert_eq!(
                lowered.diagnostics.system_size,
                base_system.diagnostics.system_size
            );
            assert_eq!(
                lowered.diagnostics.equality_count,
                base_system.diagnostics.equality_count
            );
            assert_eq!(
                lowered.diagnostics.row_scales,
                base_system.diagnostics.row_scales
            );
            assert_eq!(
                lowered.diagnostics.column_scales,
                base_system.diagnostics.column_scales
            );
            assert_eq!(
                lowered.diagnostics.pivot_rows,
                base_system.diagnostics.pivot_rows
            );
            assert_eq!(
                lowered.diagnostics.minimum_verified_scaled_pivot,
                base_system.diagnostics.minimum_verified_scaled_pivot
            );
            assert_eq!(
                lowered.diagnostics.maximum_verified_scaled_pivot_width,
                base_system.diagnostics.maximum_verified_scaled_pivot_width
            );
            assert!(
                lowered.diagnostics.maximum_dynamic_solution_width
                    >= base_system.diagnostics.maximum_dynamic_solution_width
            );
            assert!(
                lowered.diagnostics.maximum_full_solution_width
                    >= base_system.diagnostics.maximum_full_solution_width
            );
            assert!(
                lowered.diagnostics.maximum_residual_width
                    >= base_system.diagnostics.maximum_residual_width
            );
            assert!(
                lowered.diagnostics.maximum_residual_absolute_bound
                    >= base_system.diagnostics.maximum_residual_absolute_bound
            );
        }
    }

    #[test]
    fn repeated_solve_only_catalog_verification_is_deterministic() {
        let config = seed_config();
        let first = verify_fixed_solve_only_mobilities(config).unwrap();
        let second = verify_fixed_solve_only_mobilities(config).unwrap();
        assert_eq!(first, second);

        let subject = coupled_fixed_solve_only_subjects().next().unwrap();
        let mut invalid = first;
        invalid.certificate_version += 1;
        assert!(invalid.get(subject).is_none());
        assert!(invalid.lowered_base(subject.mechanical).is_none());
    }

    #[test]
    fn repeated_catalog_verification_is_deterministic() {
        let config = seed_config();
        let point = CoupledFixedContactPoint {
            groove_radius_m: config.groove.inner_program_radius_m,
            wall_slopes: [0.0; 2],
        };
        let first = verify_fixed_base_mobilities_at_point(config, point).unwrap();
        let second = verify_fixed_base_mobilities_at_point(config, point).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn separated_base_kkt_and_mobility_are_source_coordinate_independent() {
        let config = seed_config();
        let outer_point = CoupledFixedContactPoint {
            groove_radius_m: config.groove.outer_program_radius_m,
            wall_slopes: [0.75, -0.50],
        };
        let inner_point = CoupledFixedContactPoint {
            groove_radius_m: config.groove.inner_program_radius_m,
            wall_slopes: [-0.40, 0.60],
        };
        for mechanical in coupled_fixed_mechanical_modes() {
            let family = coupled_fixed_contact_families()
                .find(|family| {
                    family.mechanical == mechanical
                        && family.surface == CoupledFixedContactSurface::RecordLand
                        && family.stylus == StylusTangentialMode::Separated
                })
                .unwrap();
            let outer = coupled_fixed_mode_normal_response(config, family, outer_point).unwrap();
            let inner = coupled_fixed_mode_normal_response(config, family, inner_point).unwrap();
            assert_eq!(outer.kkt_lhs, inner.kkt_lhs);
            assert_eq!(outer.dynamic_mobility, inner.dynamic_mobility);
        }
    }

    #[test]
    fn point_boxes_enclose_all_288_production_contact_labels() {
        let config = seed_config();
        let catalog_point = CoupledFixedContactPoint {
            groove_radius_m: 0.100,
            wall_slopes: [0.0; 2],
        };
        let catalog = verify_fixed_base_mobilities_at_point(config, catalog_point).unwrap();
        let mut evaluated = 0;
        for family in coupled_fixed_contact_families() {
            let wall_slopes = if family.origin_law == CoupledFixedOriginLaw::HeldProgramBoundary {
                [0.0; 2]
            } else {
                [0.11, -0.07]
            };
            let point = CoupledFixedContactPoint {
                groove_radius_m: 0.100,
                wall_slopes,
            };
            let domain = FixedModeContactIntervalBox::point(point).unwrap();
            let evaluation =
                evaluate_fixed_contact_family_box(&catalog, config, family, domain).unwrap();
            let production = coupled_fixed_mode_normal_response(config, family, point).unwrap();
            assert_point_response_enclosed(evaluation, production);
            evaluated += 1;
        }
        assert_eq!(evaluated, COUPLED_FIXED_CONTACT_FAMILY_COUNT);
    }

    #[test]
    fn bounded_interior_groove_boxes_enclose_sampled_production_points() {
        let config = seed_config();
        let catalog = verify_fixed_base_mobilities_at_point(
            config,
            CoupledFixedContactPoint {
                groove_radius_m: 0.100,
                wall_slopes: [0.0; 2],
            },
        )
        .unwrap();
        let domain =
            FixedModeContactIntervalBox::hull(0.099, 0.101, [0.10, -0.08], [0.12, -0.06]).unwrap();
        let mut family_count = 0;
        let mut point_count = 0;
        for family in coupled_fixed_contact_families().filter(|family| {
            family.surface == CoupledFixedContactSurface::GrooveWalls
                && family.origin_law == CoupledFixedOriginLaw::InteriorSpiral
        }) {
            let evaluation =
                evaluate_fixed_contact_family_box(&catalog, config, family, domain).unwrap();
            family_count += 1;
            for radius in [0.099, 0.101] {
                for left_slope in [0.10, 0.12] {
                    for right_slope in [-0.08, -0.06] {
                        let point = CoupledFixedContactPoint {
                            groove_radius_m: radius,
                            wall_slopes: [left_slope, right_slope],
                        };
                        let production =
                            coupled_fixed_mode_normal_response(config, family, point).unwrap();
                        assert_point_response_enclosed(evaluation, production);
                        point_count += 1;
                    }
                }
            }
        }
        assert_eq!(family_count, 96);
        assert_eq!(point_count, 768);
    }

    #[test]
    fn bounded_land_boxes_enclose_sampled_production_points() {
        let config = seed_config();
        let catalog = verify_fixed_base_mobilities_at_point(
            config,
            CoupledFixedContactPoint {
                groove_radius_m: 0.100,
                wall_slopes: [0.0; 2],
            },
        )
        .unwrap();
        let domain =
            FixedModeContactIntervalBox::hull(0.099, 0.101, [-0.5, -0.25], [0.5, 0.25]).unwrap();
        let mut point_count = 0;
        for family in coupled_fixed_contact_families()
            .filter(|family| family.surface == CoupledFixedContactSurface::RecordLand)
        {
            let evaluation =
                evaluate_fixed_contact_family_box(&catalog, config, family, domain).unwrap();
            for radius in [0.099, 0.100, 0.101] {
                let point = CoupledFixedContactPoint {
                    groove_radius_m: radius,
                    wall_slopes: [0.37, -0.19],
                };
                let production = coupled_fixed_mode_normal_response(config, family, point).unwrap();
                assert_point_response_enclosed(evaluation, production);
                point_count += 1;
            }
        }
        assert_eq!(point_count, 288);
    }

    #[test]
    fn nonsticking_mobility_accepts_a_zero_skating_factor() {
        let config = seed_config();
        let catalog = verify_fixed_base_mobilities_at_point(
            config,
            CoupledFixedContactPoint {
                groove_radius_m: 0.100,
                wall_slopes: [0.0; 2],
            },
        )
        .unwrap();
        let radius = OutwardInterval::point(0.100).unwrap();
        let zero = OutwardInterval::point(0.0).unwrap();
        let mut evaluated = 0;
        for family in coupled_fixed_contact_families()
            .filter(|family| family.stylus != StylusTangentialMode::Sticking)
        {
            let base = catalog
                .systems
                .iter()
                .find(|system| system.mechanical == family.mechanical)
                .unwrap()
                .verified_mobility;
            let (mobility, disposition, runtime_rhs) =
                verified_effective_mobility(base, family, radius, zero, 0.100, 0.0).unwrap();
            assert_eq!(mobility, base);
            assert_eq!(disposition, VerifiedStylusEqualityDisposition::NotRequested);
            assert!(!runtime_rhs);
            evaluated += 1;
        }
        assert_eq!(evaluated, COUPLED_FIXED_CONTACT_FAMILY_COUNT * 3 / 4);
    }

    #[test]
    fn sticking_rank_classification_has_18_added_and_6_dependent_classes() {
        let config = seed_config();
        let catalog = verify_fixed_base_mobilities_at_point(
            config,
            CoupledFixedContactPoint {
                groove_radius_m: 0.100,
                wall_slopes: [0.0; 2],
            },
        )
        .unwrap();
        let domain = FixedModeContactIntervalBox::hull(0.099, 0.101, [0.0; 2], [0.0; 2]).unwrap();
        let mut added = 0;
        let mut dependent = 0;
        for family in coupled_fixed_contact_families().filter(|family| {
            family.surface == CoupledFixedContactSurface::RecordLand
                && family.stylus == StylusTangentialMode::Sticking
        }) {
            let evaluation =
                evaluate_fixed_contact_family_box(&catalog, config, family, domain).unwrap();
            match evaluation.stylus_equality {
                VerifiedStylusEqualityDisposition::AddsRank { schur_denominator } => {
                    assert!(!schur_denominator.contains_zero());
                    added += 1;
                }
                VerifiedStylusEqualityDisposition::DependentRuntimeRhs => {
                    assert!(evaluation.runtime_rhs_compatibility_required);
                    dependent += 1;
                }
                VerifiedStylusEqualityDisposition::NotRequested => panic!("sticking was ignored"),
            }
        }
        assert_eq!(added, 18);
        assert_eq!(dependent, 6);
    }

    #[test]
    fn contact_evaluation_rejects_a_different_config_identity() {
        let config = seed_config();
        let catalog = verify_fixed_base_mobilities_at_point(
            config,
            CoupledFixedContactPoint {
                groove_radius_m: 0.100,
                wall_slopes: [0.0; 2],
            },
        )
        .unwrap();
        let family = coupled_fixed_contact_families()
            .find(|family| family.surface == CoupledFixedContactSurface::RecordLand)
            .unwrap();
        let domain = FixedModeContactIntervalBox::hull(0.100, 0.100, [0.0; 2], [0.0; 2]).unwrap();
        let mut changed = config;
        changed.contact.record_surface_friction_coefficient = 0.21;
        assert!(matches!(
            evaluate_fixed_contact_family_box(&catalog, changed, family, domain),
            Err(FixedModeContactBoxError::ConfigIdentityMismatch)
        ));
    }

    #[test]
    fn held_boundary_rejects_every_nonzero_slope_domain() {
        let config = seed_config();
        let catalog = verify_fixed_base_mobilities_at_point(
            config,
            CoupledFixedContactPoint {
                groove_radius_m: 0.100,
                wall_slopes: [0.0; 2],
            },
        )
        .unwrap();
        let family = coupled_fixed_contact_families()
            .find(|family| family.origin_law == CoupledFixedOriginLaw::HeldProgramBoundary)
            .unwrap();
        let domain =
            FixedModeContactIntervalBox::hull(0.100, 0.100, [0.0, -0.01], [0.0, 0.01]).unwrap();
        assert!(matches!(
            evaluate_fixed_contact_family_box(&catalog, config, family, domain),
            Err(FixedModeContactBoxError::HeldBoundaryRequiresZeroSlope)
        ));
    }

    #[test]
    fn interior_sticking_box_marks_zero_and_nonzero_slope_reachability() {
        let config = seed_config();
        let catalog = verify_fixed_base_mobilities_at_point(
            config,
            CoupledFixedContactPoint {
                groove_radius_m: 0.100,
                wall_slopes: [0.0; 2],
            },
        )
        .unwrap();
        let family = coupled_fixed_contact_families()
            .find(|family| {
                family.origin_law == CoupledFixedOriginLaw::InteriorSpiral
                    && family.stylus == StylusTangentialMode::Sticking
            })
            .unwrap();
        let zero = FixedModeContactIntervalBox::hull(0.100, 0.100, [0.0; 2], [0.0; 2]).unwrap();
        let mixed =
            FixedModeContactIntervalBox::hull(0.100, 0.100, [-0.1, -0.2], [0.1, 0.2]).unwrap();
        let nonzero =
            FixedModeContactIntervalBox::hull(0.100, 0.100, [0.05, -0.2], [0.1, -0.1]).unwrap();
        assert_eq!(
            evaluate_fixed_contact_family_box(&catalog, config, family, zero)
                .unwrap()
                .reachability,
            VerifiedContactBoxReachability::RuntimeConditional
        );
        assert_eq!(
            evaluate_fixed_contact_family_box(&catalog, config, family, mixed)
                .unwrap()
                .reachability,
            VerifiedContactBoxReachability::ActiveWallDependent {
                nonzero_active_wall:
                    CoupledFixedModeInfeasibility::SlopedGrooveStickingWithFriction,
                wall_may_be_nonzero: [true, true],
                wall_is_always_nonzero: [false, false],
            }
        );
        assert_eq!(
            evaluate_fixed_contact_family_box(&catalog, config, family, nonzero)
                .unwrap()
                .reachability,
            VerifiedContactBoxReachability::ActiveWallDependent {
                nonzero_active_wall:
                    CoupledFixedModeInfeasibility::SlopedGrooveStickingWithFriction,
                wall_may_be_nonzero: [true, true],
                wall_is_always_nonzero: [true, true],
            }
        );
    }

    #[test]
    fn asymmetric_sticking_slopes_require_active_wall_qualification() {
        let config = seed_config();
        let catalog = verify_fixed_base_mobilities_at_point(
            config,
            CoupledFixedContactPoint {
                groove_radius_m: 0.100,
                wall_slopes: [0.0; 2],
            },
        )
        .unwrap();
        let family = coupled_fixed_contact_families()
            .find(|family| {
                family.origin_law == CoupledFixedOriginLaw::InteriorSpiral
                    && family.stylus == StylusTangentialMode::Sticking
            })
            .unwrap();
        let domain =
            FixedModeContactIntervalBox::hull(0.100, 0.100, [0.125, 0.0], [0.125, 0.0]).unwrap();
        let evaluation =
            evaluate_fixed_contact_family_box(&catalog, config, family, domain).unwrap();
        assert_eq!(
            evaluation.reachability,
            VerifiedContactBoxReachability::ActiveWallDependent {
                nonzero_active_wall:
                    CoupledFixedModeInfeasibility::SlopedGrooveStickingWithFriction,
                wall_may_be_nonzero: [true, false],
                wall_is_always_nonzero: [true, false],
            }
        );
    }

    #[test]
    fn seed_point_has_strict_positive_groove_and_land_predicates() {
        let config = seed_config();
        let point = CoupledFixedContactPoint {
            groove_radius_m: 0.100,
            wall_slopes: [0.0; 2],
        };
        let catalog = verify_fixed_base_mobilities_at_point(config, point).unwrap();
        let domain = FixedModeContactIntervalBox::point(point).unwrap();
        let mut groove_count = 0;
        let mut positive_groove_count = 0;
        let mut land_count = 0;
        let mut positive_land_count = 0;
        for family in coupled_fixed_contact_families() {
            let evaluation =
                evaluate_fixed_contact_family_box(&catalog, config, family, domain).unwrap();
            match evaluation.normal_response {
                VerifiedFixedNormalResponse::GrooveWalls(response) => {
                    groove_count += 1;
                    if response
                        .diagonal_is_strictly_positive
                        .into_iter()
                        .all(|value| value)
                        && response.determinant_is_strictly_positive
                    {
                        positive_groove_count += 1;
                    }
                }
                VerifiedFixedNormalResponse::RecordLand(response) => {
                    land_count += 1;
                    if response.is_strictly_positive {
                        positive_land_count += 1;
                    }
                }
            }
        }
        assert_eq!(groove_count, 192);
        assert_eq!(land_count, 96);
        assert_eq!(positive_groove_count, groove_count);
        assert_eq!(positive_land_count, land_count);
    }

    #[test]
    fn strict_groove_predicate_rejects_the_known_negative_minor() {
        let mut config = seed_config();
        config.deck.record_inertia_kg_m2 = 1.0e-7;
        config.contact.moving_mass_kg = 1.0e-2;
        config.cartridge.generator_coefficient_v_s_per_m = 1.0e-12;
        config.cartridge.generator_coefficient_source =
            super::super::GeneratorCoefficientSource::UserSupplied;
        let point = CoupledFixedContactPoint {
            groove_radius_m: 0.146_05,
            wall_slopes: [-0.125; 2],
        };
        let catalog = verify_fixed_base_mobilities_at_point(config, point).unwrap();
        let family = coupled_fixed_contact_families()
            .find(|family| {
                family.mechanical.deck_bearing == FixedModeFrictionMobility::Sliding
                    && family.mechanical.slipmat == FixedModeFrictionMobility::Sliding
                    && family.mechanical.hand == FixedModeHandMobility::Separated
                    && family.mechanical.pickup_bearing == FixedModeFrictionMobility::Sticking
                    && family.surface == CoupledFixedContactSurface::GrooveWalls
                    && family.origin_law == CoupledFixedOriginLaw::InteriorSpiral
                    && family.stylus == StylusTangentialMode::SlidingPositive
            })
            .unwrap();
        let domain = FixedModeContactIntervalBox::point(point).unwrap();
        let evaluation =
            evaluate_fixed_contact_family_box(&catalog, config, family, domain).unwrap();
        let production = coupled_fixed_mode_normal_response(config, family, point).unwrap();
        assert_point_response_enclosed(evaluation, production);
        let VerifiedFixedNormalResponse::GrooveWalls(response) = evaluation.normal_response else {
            panic!("expected a groove response");
        };
        assert!(response.diagonal[0].upper() < 0.0);
        assert!(!response.diagonal_is_strictly_positive[0]);
    }
}
