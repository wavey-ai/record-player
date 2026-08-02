use serde::{Deserialize, Serialize};
use thiserror::Error;

const MIN_MASS_KG: f64 = 1.0e-5;
const MIN_COMPLIANCE_M_PER_N: f64 = 1.0e-6;
const MAX_COMPLIANCE_M_PER_N: f64 = 0.2;
const MAX_BEARING_FRICTION_N: f64 = 1.0;
const MAX_BEARING_DAMPING_N_S_PER_M: f64 = 1_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TonearmGeometry {
    pub effective_length_m: f64,
    pub pivot_to_spindle_m: f64,
    pub offset_angle_degrees: f64,
}

impl TonearmGeometry {
    pub fn sl_1200mk7() -> Self {
        Self {
            effective_length_m: 0.230,
            pivot_to_spindle_m: 0.215,
            offset_angle_degrees: 22.0,
        }
    }

    pub fn validate(self) -> Result<Self, TonearmError> {
        if !self.effective_length_m.is_finite() || self.effective_length_m <= 0.0 {
            return Err(TonearmError::InvalidGeometry {
                field: "effectiveLengthM",
            });
        }
        if !self.pivot_to_spindle_m.is_finite()
            || self.pivot_to_spindle_m <= 0.0
            || self.pivot_to_spindle_m >= self.effective_length_m
        {
            return Err(TonearmError::InvalidGeometry {
                field: "pivotToSpindleM",
            });
        }
        if !self.offset_angle_degrees.is_finite()
            || !(0.0..=45.0).contains(&self.offset_angle_degrees)
        {
            return Err(TonearmError::InvalidGeometry {
                field: "offsetAngleDegrees",
            });
        }
        Ok(self)
    }

    pub fn overhang_m(self) -> f64 {
        self.effective_length_m - self.pivot_to_spindle_m
    }

    pub fn tracking_error_degrees(self, groove_radius_m: f64) -> Result<f64, TonearmError> {
        let (groove_angle, stylus_x, stylus_y) = self.stylus_geometry(groove_radius_m)?;
        let pivot = self.pivot_to_spindle_m;
        let arm_angle = stylus_y.atan2(stylus_x - pivot);
        let tangent_angle = groove_angle + std::f64::consts::FRAC_PI_2;
        let cartridge_angle = arm_angle + self.offset_angle_degrees.to_radians();
        Ok(normalize_angle(cartridge_angle - tangent_angle).to_degrees())
    }

    /// Converts tangential force to a radial force with the same pivot moment.
    pub fn equivalent_radial_force_n(
        self,
        groove_radius_m: f64,
        tangential_force_on_stylus_n: f64,
    ) -> Result<f64, TonearmError> {
        if !tangential_force_on_stylus_n.is_finite() {
            return Err(TonearmError::InvalidForce);
        }
        Ok(self.skating_force_factor(groove_radius_m)? * tangential_force_on_stylus_n)
    }

    /// Returns K where radial force equals K times tangential stylus force.
    pub(crate) fn skating_force_factor(self, groove_radius_m: f64) -> Result<f64, TonearmError> {
        self.validate()?;
        if !groove_radius_m.is_finite() || groove_radius_m <= 0.0 {
            return Err(TonearmError::InvalidGrooveRadius);
        }
        let length = self.effective_length_m;
        let pivot = self.pivot_to_spindle_m;
        let cosine = (groove_radius_m * groove_radius_m + pivot * pivot - length * length)
            / (2.0 * groove_radius_m * pivot);
        if !cosine.is_finite() || cosine <= -1.0 || cosine >= 1.0 {
            return Err(TonearmError::UnreachableGrooveRadius);
        }
        let sine_squared = 1.0 - cosine * cosine;
        if !sine_squared.is_finite() || sine_squared <= 0.0 {
            return Err(TonearmError::UnreachableGrooveRadius);
        }
        let sine = sine_squared.sqrt();
        let radial_moment_arm = pivot * sine;
        if !radial_moment_arm.is_finite() || radial_moment_arm < 1.0e-12 {
            return Err(TonearmError::SingularForceGeometry);
        }
        let factor = (pivot * cosine - groove_radius_m) / radial_moment_arm;
        if !factor.is_finite() {
            return Err(TonearmError::SingularForceGeometry);
        }
        Ok(factor)
    }

    fn stylus_geometry(self, groove_radius_m: f64) -> Result<(f64, f64, f64), TonearmError> {
        self.validate()?;
        if !groove_radius_m.is_finite() || groove_radius_m <= 0.0 {
            return Err(TonearmError::InvalidGrooveRadius);
        }
        let length = self.effective_length_m;
        let pivot = self.pivot_to_spindle_m;
        let cosine = (groove_radius_m * groove_radius_m + pivot * pivot - length * length)
            / (2.0 * groove_radius_m * pivot);
        if !(-1.0..=1.0).contains(&cosine) {
            return Err(TonearmError::UnreachableGrooveRadius);
        }
        let groove_angle = cosine.acos();
        Ok((
            groove_angle,
            groove_radius_m * groove_angle.cos(),
            groove_radius_m * groove_angle.sin(),
        ))
    }
}

impl Default for TonearmGeometry {
    fn default() -> Self {
        Self::sl_1200mk7()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuspensionAxisConfig {
    pub effective_mass_kg: f64,
    pub compliance_m_per_n: f64,
    pub damping_ratio: f64,
}

impl SuspensionAxisConfig {
    pub fn validate(self, axis: &'static str) -> Result<Self, TonearmError> {
        if !self.effective_mass_kg.is_finite() || self.effective_mass_kg < MIN_MASS_KG {
            return Err(TonearmError::InvalidAxis {
                axis,
                field: "mass",
            });
        }
        if !self.compliance_m_per_n.is_finite()
            || !(MIN_COMPLIANCE_M_PER_N..=MAX_COMPLIANCE_M_PER_N).contains(&self.compliance_m_per_n)
        {
            return Err(TonearmError::InvalidAxis {
                axis,
                field: "compliance",
            });
        }
        if !self.damping_ratio.is_finite() || !(0.0..=4.0).contains(&self.damping_ratio) {
            return Err(TonearmError::InvalidAxis {
                axis,
                field: "dampingRatio",
            });
        }
        Ok(self)
    }

    pub fn stiffness_n_per_m(self) -> f64 {
        1.0 / self.compliance_m_per_n
    }

    pub fn natural_frequency_hz(self) -> f64 {
        (self.stiffness_n_per_m() / self.effective_mass_kg).sqrt() / std::f64::consts::TAU
    }

    pub fn viscous_damping_n_s_per_m(self) -> f64 {
        2.0 * self.damping_ratio * (self.stiffness_n_per_m() * self.effective_mass_kg).sqrt()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TonearmConfig {
    pub geometry: TonearmGeometry,
    pub lateral: SuspensionAxisConfig,
    pub vertical: SuspensionAxisConfig,
    pub vertical_tracking_force_n: f64,
    pub anti_skate_force_n: f64,
    pub lateral_bearing_static_friction_n: f64,
    pub lateral_bearing_kinetic_friction_n: f64,
    pub lateral_bearing_viscous_damping_n_s_per_m: f64,
    pub cue_lift_height_m: f64,
    pub cue_support_stiffness_n_per_m: f64,
    pub cue_support_damping_n_s_per_m: f64,
}

impl TonearmConfig {
    /// This seed combines published cartridge data with arm-mass estimates.
    pub fn sl_1200mk7_concorde_mkii_scratch_seed() -> Self {
        Self {
            geometry: TonearmGeometry::sl_1200mk7(),
            lateral: SuspensionAxisConfig {
                // Technics does not publish effective arm mass for this model.
                effective_mass_kg: 0.030_5,
                // Ortofon publishes 14 micrometers per mN laterally.
                compliance_m_per_n: 0.014,
                damping_ratio: 0.25,
            },
            vertical: SuspensionAxisConfig {
                effective_mass_kg: 0.030_5,
                // Vertical compliance is an estimate.
                compliance_m_per_n: 0.010,
                damping_ratio: 0.30,
            },
            vertical_tracking_force_n: 0.004 * 9.806_65,
            // This scratch seed starts without anti-skate force.
            anti_skate_force_n: 0.0,
            // Technics does not publish horizontal bearing friction data.
            lateral_bearing_static_friction_n: 0.000_12,
            lateral_bearing_kinetic_friction_n: 0.000_08,
            lateral_bearing_viscous_damping_n_s_per_m: 0.018,
            // The cue geometry and support coefficients are estimates.
            cue_lift_height_m: 0.003,
            cue_support_stiffness_n_per_m: 2_000.0,
            cue_support_damping_n_s_per_m: 12.0,
        }
    }

    pub fn validate(self) -> Result<Self, TonearmError> {
        self.geometry.validate()?;
        self.lateral.validate("lateral")?;
        self.vertical.validate("vertical")?;
        if !self.vertical_tracking_force_n.is_finite() || self.vertical_tracking_force_n <= 0.0 {
            return Err(TonearmError::InvalidTrackingForce);
        }
        if !self.anti_skate_force_n.is_finite() || self.anti_skate_force_n < 0.0 {
            return Err(TonearmError::InvalidAntiSkateForce);
        }
        for (field, value, maximum) in [
            (
                "lateralBearingStaticFrictionN",
                self.lateral_bearing_static_friction_n,
                MAX_BEARING_FRICTION_N,
            ),
            (
                "lateralBearingKineticFrictionN",
                self.lateral_bearing_kinetic_friction_n,
                MAX_BEARING_FRICTION_N,
            ),
            (
                "lateralBearingViscousDampingNSPerM",
                self.lateral_bearing_viscous_damping_n_s_per_m,
                MAX_BEARING_DAMPING_N_S_PER_M,
            ),
        ] {
            if !value.is_finite() || !(0.0..=maximum).contains(&value) {
                return Err(TonearmError::InvalidBearing { field });
            }
        }
        if self.lateral_bearing_kinetic_friction_n > self.lateral_bearing_static_friction_n {
            return Err(TonearmError::InvalidBearing {
                field: "lateralBearingKineticFrictionN",
            });
        }
        for (field, value, strictly_positive) in [
            ("cueLiftHeightM", self.cue_lift_height_m, true),
            (
                "cueSupportStiffnessNPerM",
                self.cue_support_stiffness_n_per_m,
                true,
            ),
            (
                "cueSupportDampingNSPerM",
                self.cue_support_damping_n_s_per_m,
                false,
            ),
        ] {
            if !value.is_finite()
                || (strictly_positive && value <= 0.0)
                || (!strictly_positive && value < 0.0)
            {
                return Err(TonearmError::InvalidCue { field });
            }
        }
        Ok(self)
    }
}

impl Default for TonearmConfig {
    fn default() -> Self {
        Self::sl_1200mk7_concorde_mkii_scratch_seed()
    }
}

fn normalize_angle(angle: f64) -> f64 {
    (angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum TonearmError {
    #[error("tonearm geometry field {field} is invalid")]
    InvalidGeometry { field: &'static str },
    #[error("groove radius must be finite and positive")]
    InvalidGrooveRadius,
    #[error("the tonearm cannot reach this groove radius")]
    UnreachableGrooveRadius,
    #[error("{axis} suspension {field} is invalid")]
    InvalidAxis {
        axis: &'static str,
        field: &'static str,
    },
    #[error("vertical tracking force must be finite and positive")]
    InvalidTrackingForce,
    #[error("anti-skate force must be finite and nonnegative")]
    InvalidAntiSkateForce,
    #[error("tonearm bearing field {field} is invalid")]
    InvalidBearing { field: &'static str },
    #[error("cue configuration field {field} is invalid")]
    InvalidCue { field: &'static str },
    #[error("tonearm force must be finite")]
    InvalidForce,
    #[error("tonearm force geometry is singular")]
    SingularForceGeometry,
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTER_PROGRAM_RADIUS_M: f64 = 0.146_05;
    const INNER_PROGRAM_RADIUS_M: f64 = 0.060_325;

    fn trigonometric_skating_factor_reference(
        geometry: TonearmGeometry,
        groove_radius_m: f64,
    ) -> f64 {
        let length = geometry.effective_length_m;
        let pivot = geometry.pivot_to_spindle_m;
        let cosine = (groove_radius_m * groove_radius_m + pivot * pivot - length * length)
            / (2.0 * groove_radius_m * pivot);
        let groove_angle = cosine.acos();
        let stylus_x = groove_radius_m * groove_angle.cos();
        let stylus_y = groove_radius_m * groove_angle.sin();
        let arm_x = stylus_x - pivot;
        let tangent_x = -groove_angle.sin();
        let tangent_y = groove_angle.cos();
        let radial_x = groove_angle.cos();
        let radial_y = groove_angle.sin();
        let tangential_moment_arm = arm_x * tangent_y - stylus_y * tangent_x;
        let radial_moment_arm = arm_x * radial_y - stylus_y * radial_x;
        tangential_moment_arm / radial_moment_arm
    }

    fn ordered_bits(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1_u64 << 63) == 0 {
            bits | (1_u64 << 63)
        } else {
            !bits
        }
    }

    fn ulp_distance(left: f64, right: f64) -> u64 {
        ordered_bits(left).abs_diff(ordered_bits(right))
    }

    #[test]
    fn geometry_uses_published_length_overhang_and_offset() {
        let geometry = TonearmGeometry::default();
        assert_eq!(geometry.effective_length_m, 0.230);
        assert!((geometry.overhang_m() - 0.015).abs() < f64::EPSILON);
        assert_eq!(geometry.offset_angle_degrees, 22.0);
        let outer_error = geometry.tracking_error_degrees(0.146_05).unwrap().abs();
        let inner_error = geometry.tracking_error_degrees(0.060_325).unwrap().abs();
        assert!((outer_error - (2.0 + 32.0 / 60.0)).abs() < 0.15);
        assert!((inner_error - 32.0 / 60.0).abs() < 0.40);
    }

    #[test]
    fn lateral_resonance_matches_the_mass_compliance_equation() {
        let axis = TonearmConfig::default().lateral;
        let mass_g = axis.effective_mass_kg * 1_000.0;
        let compliance_um_per_mn = axis.compliance_m_per_n * 1_000.0;
        let expected = 1_000.0 / (std::f64::consts::TAU * (mass_g * compliance_um_per_mn).sqrt());
        assert!((axis.natural_frequency_hz() - expected).abs() < 1.0e-12);
    }

    #[test]
    fn equivalent_radial_force_reverses_with_record_direction() {
        let geometry = TonearmGeometry::default();
        let radius_m = 0.100;
        let forward = geometry.equivalent_radial_force_n(radius_m, 0.010).unwrap();
        let reverse = geometry
            .equivalent_radial_force_n(radius_m, -0.010)
            .unwrap();
        assert!(forward < 0.0, "{forward}");
        assert!(reverse > 0.0, "{reverse}");
        assert!((forward + reverse).abs() < 1.0e-15);
    }

    #[test]
    fn equivalent_radial_force_is_finite_at_programme_radii() {
        let geometry = TonearmGeometry::default();
        for radius_m in [OUTER_PROGRAM_RADIUS_M, INNER_PROGRAM_RADIUS_M] {
            let forward = geometry.equivalent_radial_force_n(radius_m, 0.010).unwrap();
            let reverse = geometry
                .equivalent_radial_force_n(radius_m, -0.010)
                .unwrap();
            assert!(forward.is_finite(), "{radius_m}: {forward}");
            assert!(reverse.is_finite(), "{radius_m}: {reverse}");
            assert!(forward < 0.0, "{radius_m}: {forward}");
            assert!(reverse > 0.0, "{radius_m}: {reverse}");
        }
    }

    #[test]
    fn skating_factor_accepts_the_program_radius_endpoints() {
        let geometry = TonearmGeometry::default();
        for radius_m in [INNER_PROGRAM_RADIUS_M, OUTER_PROGRAM_RADIUS_M] {
            let factor = geometry.skating_force_factor(radius_m).unwrap();
            assert!(factor.is_finite(), "{radius_m}: {factor}");
        }
    }

    #[test]
    fn skating_factor_rejects_invalid_and_unreachable_geometry() {
        let invalid_geometry = TonearmGeometry {
            effective_length_m: 1.0,
            pivot_to_spindle_m: 1.0,
            offset_angle_degrees: 0.0,
        };
        assert!(matches!(
            invalid_geometry.skating_force_factor(1.0),
            Err(TonearmError::InvalidGeometry {
                field: "pivotToSpindleM"
            })
        ));

        let geometry = TonearmGeometry {
            effective_length_m: 2.0,
            pivot_to_spindle_m: 1.0,
            offset_angle_degrees: 0.0,
        };
        for radius_m in [1.0, 3.0, 0.5, 3.5] {
            assert_eq!(
                geometry.skating_force_factor(radius_m),
                Err(TonearmError::UnreachableGrooveRadius),
                "{radius_m}"
            );
        }
        for radius_m in [0.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                geometry.skating_force_factor(radius_m),
                Err(TonearmError::InvalidGrooveRadius),
                "{radius_m}"
            );
        }
    }

    #[test]
    fn skating_factor_is_negative_and_nonzero_across_the_program_radius() {
        let geometry = TonearmGeometry::default();
        for index in 0..=1_024 {
            let fraction = f64::from(index) / 1_024.0;
            let radius_m = INNER_PROGRAM_RADIUS_M
                + fraction * (OUTER_PROGRAM_RADIUS_M - INNER_PROGRAM_RADIUS_M);
            let factor = geometry.skating_force_factor(radius_m).unwrap();
            assert!(factor.is_finite(), "{radius_m}: {factor}");
            assert!(factor < 0.0, "{radius_m}: {factor}");
            assert_ne!(factor, 0.0, "{radius_m}");
        }
    }

    #[test]
    fn algebraic_skating_factor_matches_independent_references() {
        let geometry = TonearmGeometry::default();
        for index in 0..=256 {
            let fraction = f64::from(index) / 256.0;
            let radius_m = INNER_PROGRAM_RADIUS_M
                + fraction * (OUTER_PROGRAM_RADIUS_M - INNER_PROGRAM_RADIUS_M);
            let actual = geometry.skating_force_factor(radius_m).unwrap();
            let reference = trigonometric_skating_factor_reference(geometry, radius_m);
            assert!(
                (actual - reference).abs() <= 16.0 * f64::EPSILON,
                "{radius_m}: actual={actual}, reference={reference}"
            );
        }

        // These values come from 100-digit arithmetic over the exact binary64 inputs.
        for (radius_m, reference_bits) in [
            (INNER_PROGRAM_RADIUS_M, 0xbfd9_9fa6_62af_758e),
            (0.100, 0xbfd8_e4a4_a5be_de8e),
            (OUTER_PROGRAM_RADIUS_M, 0xbfdd_59b6_7c2f_6cf0),
        ] {
            let actual = geometry.skating_force_factor(radius_m).unwrap();
            let reference = f64::from_bits(reference_bits);
            assert!(
                ulp_distance(actual, reference) <= 1,
                "{radius_m}: actual={actual}, reference={reference}"
            );
        }
    }
}
