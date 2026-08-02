//! Verified point solves for the fixed mechanical KKT systems.

use thiserror::Error;

use super::contact::{
    coupled_fixed_contact_families, coupled_fixed_mechanical_modes,
    coupled_fixed_mode_normal_response, CoupledFixedContactFamily, CoupledFixedContactPoint,
    CoupledFixedContactSurface, CoupledFixedDynamicMobility, CoupledFixedMechanicalMode,
    CoupledFixedModeKktLhs, CoupledFixedModeResponseError, CoupledFixedOriginLaw,
    JointDynamicEquation, JointDynamicVelocity, StylusTangentialMode,
    COUPLED_FIXED_CONTACT_FAMILY_COUNT, COUPLED_FIXED_MECHANICAL_CLASS_COUNT,
    COUPLED_FIXED_MODE_FAMILY_SET_VERSION, COUPLED_FIXED_MODE_OPERATOR_VERSION,
    JOINT_DYNAMIC_EQUATIONS, JOINT_DYNAMIC_VELOCITIES,
};
use super::verified_interval::{OutwardInterval, OutwardIntervalError};
use super::PhysicalPlaybackConfig;

const DYNAMIC_VARIABLE_COUNT: usize = 6;
const KKT_CAPACITY: usize = 13;
const AUGMENTED_COLUMN_COUNT: usize = KKT_CAPACITY + 1;
const RHS_COLUMN: usize = KKT_CAPACITY;
const UNUSED_PIVOT_ROW: usize = usize::MAX;
const REFERENCE_RELATIVE_PIVOT_FACTOR: f64 = 128.0;

/// Identifies the point-solve certificate format.
pub(crate) const FIXED_MODE_POINT_MOBILITY_CERTIFICATE_VERSION: u32 = 1;

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
    pub(crate) point: CoupledFixedContactPoint,
    pub(crate) systems: [VerifiedFixedBaseMobility; COUPLED_FIXED_MECHANICAL_CLASS_COUNT],
}

/// Reports why a fixed-mode point solve could not be verified.
#[derive(Debug, Error)]
pub(crate) enum FixedModePointMobilityCertificateError {
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
        point,
        systems,
    })
}

fn verify_base_mobility(
    mechanical_class_index: usize,
    kkt_lhs: CoupledFixedModeKktLhs,
    mechanical: CoupledFixedMechanicalMode,
    base_family: CoupledFixedContactFamily,
    production_mobility: CoupledFixedDynamicMobility,
) -> Result<VerifiedFixedBaseMobility, FixedModePointMobilityCertificateError> {
    if kkt_lhs.system_size < DYNAMIC_VARIABLE_COUNT
        || kkt_lhs.system_size > KKT_CAPACITY
        || kkt_lhs.equality_count != kkt_lhs.system_size - DYNAMIC_VARIABLE_COUNT
    {
        return Err(FixedModePointMobilityCertificateError::KktSolve {
            mechanical_class_index,
            equation_index: 0,
            source: VerifiedPointKktError::InvalidSize,
        });
    }

    let zero = OutwardInterval::point(0.0).map_err(|source| {
        FixedModePointMobilityCertificateError::KktSolve {
            mechanical_class_index,
            equation_index: 0,
            source: source.into(),
        }
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
            .map_err(|source| FixedModePointMobilityCertificateError::KktSolve {
                mechanical_class_index,
                equation_index,
                source,
            })?;
        let path = VerifiedEliminationPath::from(solved.diagnostics);
        if let Some(expected) = common_path {
            if expected != path {
                return Err(
                    FixedModePointMobilityCertificateError::InconsistentSolvePath {
                        mechanical_class_index,
                    },
                );
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
                return Err(
                    FixedModePointMobilityCertificateError::ProductionReplayMismatch {
                        mechanical_class_index,
                        velocity_index,
                        equation_index,
                    },
                );
            }
            let enclosure = solved.solution[velocity_index];
            if !enclosure.contains(production) {
                return Err(
                    FixedModePointMobilityCertificateError::ProductionNotEnclosed {
                        mechanical_class_index,
                        velocity_index,
                        equation_index,
                    },
                );
            }
            verified_mobility.velocity_by_equation_rhs[velocity_index][equation_index] = enclosure;
        }
    }

    let path = common_path.ok_or(FixedModePointMobilityCertificateError::InvalidCatalog)?;
    Ok(VerifiedFixedBaseMobility {
        mechanical,
        base_family,
        kkt_lhs,
        reference_mobility: production_mobility,
        verified_mobility,
        diagnostics: aggregate.finish(kkt_lhs.equality_count, path),
    })
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
    use super::*;
    use num_rational::BigRational;

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
        let config = super::super::PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed().config;
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
    fn repeated_catalog_verification_is_deterministic() {
        let config = super::super::PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed().config;
        let point = CoupledFixedContactPoint {
            groove_radius_m: config.groove.inner_program_radius_m,
            wall_slopes: [0.0; 2],
        };
        let first = verify_fixed_base_mobilities_at_point(config, point).unwrap();
        let second = verify_fixed_base_mobilities_at_point(config, point).unwrap();
        assert_eq!(first, second);
    }
}
