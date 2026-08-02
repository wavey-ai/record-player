use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    output::PHYSICAL_OUTPUT_INPUT_RATE_HZ, GeneratorCoefficientSource, GrooveLayout,
    MovingMagnetCartridgeConfig, PhonoStageConfig, RadialTrackingConfig, RecordCutConfig,
    RiaaConfig, StylusContactConfig, StylusGeometry, TonearmConfig,
};
use crate::PhysicalDeckConfig;

const TECHNICS_SOURCE: &str =
    "https://www.technics.com/sg/products/dj-series/sl-1200mk7.specs.html";
const ORTOFON_SOURCE: &str = "https://ortofon.com/products/concorde-mkii-scratch";
const AES_TRACING_SOURCE: &str = "https://www.aes.org/e-lib/download.cfm/22236.pdf?ID=22236";

/// The largest accepted 192 kHz render block.
pub const MAXIMUM_PHYSICAL_RENDER_FRAMES: usize = 16_384;

/// The largest accepted preallocated control timeline.
pub const MAXIMUM_PHYSICAL_CONTROL_TIMELINE_CAPACITY: usize = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceKind {
    Published,
    Calculated,
    Estimated,
    Measured,
    Defined,
}

/// Stores one exact scalar or enum configuration value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum ParameterValue {
    Float(f64),
    Unsigned(u64),
    Enum(String),
}

impl ParameterValue {
    fn is_valid(&self) -> bool {
        match self {
            Self::Float(value) => value.is_finite(),
            Self::Unsigned(_) => true,
            Self::Enum(value) => !value.trim().is_empty(),
        }
    }

    fn exactly_matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Float(left), Self::Float(right)) => left.to_bits() == right.to_bits(),
            (Self::Unsigned(left), Self::Unsigned(right)) => left == right,
            (Self::Enum(left), Self::Enum(right)) => left == right,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterEvidence {
    pub parameter: String,
    pub value: ParameterValue,
    pub unit: String,
    pub kind: EvidenceKind,
    pub calibration_test: Option<CalibrationTest>,
    pub source: Option<String>,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CalibrationEvidenceRequirement {
    DirectMeasurement,
    DeclaredSetting,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterManifestEntry {
    pub parameter: String,
    pub value: ParameterValue,
    pub unit: String,
    pub calibration_requirement: CalibrationEvidenceRequirement,
    pub calibration_test: Option<CalibrationTest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CalibrationTest {
    PlatterTorqueSpeedAndStartup,
    PlatterSpeedStability,
    MotorTorqueRippleAndControlLatency,
    BearingDragAndCoastdown,
    SlipmatFriction,
    HandContactFriction,
    RecordWarpEccentricityAndSpindle,
    GrooveGeometryAndCut,
    GrooveMaterialContactAndWear,
    StylusGeometry,
    PickupMechanicalResponse,
    TonearmGeometryAndResonance,
    CartridgeImpedanceSweep,
    CartridgeMagneticLossLevelAndTemperature,
    CartridgeOutputBalanceAndSeparation,
    CartridgeElectromechanicalReciprocity,
    RiaaAmplitudeAndPhase,
    PhonoGainNoiseAndOverload,
    RapidScratchTrackingAndRecovery,
    EndToEndTestRecord,
}

pub const REQUIRED_CALIBRATION_TESTS: [CalibrationTest; 20] = [
    CalibrationTest::PlatterTorqueSpeedAndStartup,
    CalibrationTest::PlatterSpeedStability,
    CalibrationTest::MotorTorqueRippleAndControlLatency,
    CalibrationTest::BearingDragAndCoastdown,
    CalibrationTest::SlipmatFriction,
    CalibrationTest::HandContactFriction,
    CalibrationTest::RecordWarpEccentricityAndSpindle,
    CalibrationTest::GrooveGeometryAndCut,
    CalibrationTest::GrooveMaterialContactAndWear,
    CalibrationTest::StylusGeometry,
    CalibrationTest::PickupMechanicalResponse,
    CalibrationTest::TonearmGeometryAndResonance,
    CalibrationTest::CartridgeImpedanceSweep,
    CalibrationTest::CartridgeMagneticLossLevelAndTemperature,
    CalibrationTest::CartridgeOutputBalanceAndSeparation,
    CalibrationTest::CartridgeElectromechanicalReciprocity,
    CalibrationTest::RiaaAmplitudeAndPhase,
    CalibrationTest::PhonoGainNoiseAndOverload,
    CalibrationTest::RapidScratchTrackingAndRecovery,
    CalibrationTest::EndToEndTestRecord,
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationTestDeclaration {
    pub test: CalibrationTest,
    pub procedure: String,
    pub acceptance_criterion: String,
    pub acceptance_limits: Vec<CalibrationAcceptanceLimit>,
}

/// Defines one registered numeric limit for a measured metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationAcceptanceLimit {
    pub metric: String,
    pub unit: String,
    pub minimum_inclusive: f64,
    pub maximum_inclusive: f64,
    pub maximum_uncertainty: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationMetric {
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub uncertainty: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationValidationResult {
    pub test: CalibrationTest,
    pub kind: EvidenceKind,
    pub passed: bool,
    pub artifact: String,
    pub artifact_sha256: String,
    pub measurements: Vec<CalibrationMetric>,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CalibrationStatus {
    Seed,
    PartiallyMeasured,
    Calibrated,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalSolverConfig {
    pub internal_sample_rate_hz: f64,
    pub maximum_render_frames: usize,
    pub control_timeline_capacity: usize,
}

impl PhysicalSolverConfig {
    pub fn validate(self) -> Result<Self, PhysicalProfileError> {
        if self.internal_sample_rate_hz != f64::from(PHYSICAL_OUTPUT_INPUT_RATE_HZ) {
            return Err(PhysicalProfileError::InvalidSolver {
                field: "internalSampleRateHz",
            });
        }
        if !(1..=MAXIMUM_PHYSICAL_RENDER_FRAMES).contains(&self.maximum_render_frames) {
            return Err(PhysicalProfileError::InvalidSolver {
                field: "maximumRenderFrames",
            });
        }
        if !(1..=MAXIMUM_PHYSICAL_CONTROL_TIMELINE_CAPACITY)
            .contains(&self.control_timeline_capacity)
        {
            return Err(PhysicalProfileError::InvalidSolver {
                field: "controlTimelineCapacity",
            });
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalPlaybackConfig {
    pub deck: PhysicalDeckConfig,
    pub groove: GrooveLayout,
    pub stylus: StylusGeometry,
    pub contact: StylusContactConfig,
    pub tonearm: TonearmConfig,
    pub radial_tracking: RadialTrackingConfig,
    pub cartridge: MovingMagnetCartridgeConfig,
    pub record_cut: RecordCutConfig,
    pub phono: PhonoStageConfig,
    pub solver: PhysicalSolverConfig,
}

impl PhysicalPlaybackConfig {
    pub fn validate(self) -> Result<Self, PhysicalProfileError> {
        self.deck.validate()?;
        self.groove.validate()?;
        self.stylus.validate()?;
        self.contact.validate()?;
        self.tonearm.validate()?;
        self.radial_tracking
            .validate_for_groove(self.record_cut.groove_top_width_m)?;
        self.cartridge.validate()?;
        self.phono.validate()?;
        self.solver.validate()?;
        let sample_rate = self.solver.internal_sample_rate_hz;
        if (self.deck.integration_hz - sample_rate).abs() > f64::EPSILON
            || (self.groove.groove_sample_rate_hz - sample_rate).abs() > f64::EPSILON
        {
            return Err(PhysicalProfileError::InconsistentInternalSampleRate);
        }
        if (self.deck.nominal_rpm - self.groove.nominal_rpm).abs() > f64::EPSILON {
            return Err(PhysicalProfileError::InconsistentNominalRpm);
        }
        self.record_cut
            .validate(self.groove.groove_sample_rate_hz)?;
        RiaaConfig::new(
            self.solver.internal_sample_rate_hz,
            self.record_cut.cutter_bandwidth_hz,
        )?;
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalProfile {
    pub name: String,
    pub status: CalibrationStatus,
    pub config: PhysicalPlaybackConfig,
    pub evidence: Vec<ParameterEvidence>,
    pub calibration_tests: Vec<CalibrationTestDeclaration>,
    pub calibration_results: Vec<CalibrationValidationResult>,
}

impl PhysicalProfile {
    pub fn sl_1200mk7_concorde_mkii_scratch_seed() -> Self {
        let config = PhysicalPlaybackConfig {
            deck: PhysicalDeckConfig::sl_1200mk7_seed(),
            groove: GrooveLayout::lp_33_seed(),
            stylus: StylusGeometry::concorde_mkii_scratch(),
            contact: StylusContactConfig::concorde_mkii_scratch_seed(),
            tonearm: TonearmConfig::sl_1200mk7_concorde_mkii_scratch_seed(),
            radial_tracking: RadialTrackingConfig::sl_1200mk7_estimated_seed(),
            cartridge: MovingMagnetCartridgeConfig::concorde_mkii_scratch_seed(),
            record_cut: RecordCutConfig::seed(),
            phono: PhonoStageConfig {
                gain_db: 40.0,
                input_headroom_v_peak: 0.100,
                output_headroom_v_peak: 10.0,
                input_overload_attack_seconds: 50.0e-6,
                overload_recovery_seconds: 20.0e-3,
                overload_gain_reduction_db: 24.0,
                output_slew_rate_v_per_s: 500_000.0,
                input_referred_noise_v_rms: 500.0e-9,
                noise_seed: 0x56_49_4e_59_4c,
            },
            solver: PhysicalSolverConfig {
                internal_sample_rate_hz: 192_000.0,
                maximum_render_frames: 2_048,
                control_timeline_capacity: 4_096,
            },
        };
        let manifest = build_parameter_manifest(config)
            .expect("the built-in physical profile schema must match its configuration");
        Self {
            name: "SL-1200MK7 and Concorde MKII Scratch seed".to_owned(),
            status: CalibrationStatus::Seed,
            config,
            evidence: seed_evidence(&manifest),
            calibration_tests: seed_calibration_tests(),
            calibration_results: Vec::new(),
        }
    }

    /// Returns every stable scalar and enum configuration parameter.
    pub fn parameter_manifest(&self) -> Result<Vec<ParameterManifestEntry>, PhysicalProfileError> {
        self.config.validate()?;
        build_parameter_manifest(self.config)
    }

    pub fn validate(&self) -> Result<(), PhysicalProfileError> {
        if self.name.trim().is_empty() {
            return Err(PhysicalProfileError::EmptyName);
        }
        self.config.validate()?;
        let manifest = build_parameter_manifest(self.config)?;
        validate_evidence(&self.evidence, &manifest, self.status)?;
        validate_calibration_tests(&self.calibration_tests, self.status)?;
        validate_calibration_results(
            &self.calibration_results,
            &self.calibration_tests,
            self.status,
        )?;
        if self.status == CalibrationStatus::Calibrated
            && self.config.cartridge.generator_coefficient_source
                != GeneratorCoefficientSource::DirectMeasurement
        {
            return Err(PhysicalProfileError::GeneratorCoefficientNotDirectlyMeasured);
        }
        Ok(())
    }

    pub fn permits_calibrated_claim(&self) -> bool {
        self.status == CalibrationStatus::Calibrated && self.validate().is_ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParameterValueKind {
    Float,
    Unsigned,
    Enum,
}

#[derive(Debug, Clone, Copy)]
struct ParameterSchema {
    path: &'static str,
    unit: &'static str,
    value_kind: ParameterValueKind,
    requirement: CalibrationEvidenceRequirement,
}

const fn schema(
    path: &'static str,
    unit: &'static str,
    value_kind: ParameterValueKind,
    requirement: CalibrationEvidenceRequirement,
) -> ParameterSchema {
    ParameterSchema {
        path,
        unit,
        value_kind,
        requirement,
    }
}

const FLOAT: ParameterValueKind = ParameterValueKind::Float;
const UNSIGNED: ParameterValueKind = ParameterValueKind::Unsigned;
const ENUM: ParameterValueKind = ParameterValueKind::Enum;
const MEASURE: CalibrationEvidenceRequirement = CalibrationEvidenceRequirement::DirectMeasurement;
const DEFINE: CalibrationEvidenceRequirement = CalibrationEvidenceRequirement::DeclaredSetting;

const PARAMETER_SCHEMA: &[ParameterSchema] = &[
    schema("deck.nominalRpm", "rpm", FLOAT, MEASURE),
    schema("deck.platterInertiaKgM2", "kg m2", FLOAT, MEASURE),
    schema("deck.recordInertiaKgM2", "kg m2", FLOAT, MEASURE),
    schema("deck.motorStartingTorqueNm", "N m", FLOAT, MEASURE),
    schema(
        "deck.motorServoKpNmPerRadS",
        "N m per rad/s",
        FLOAT,
        MEASURE,
    ),
    schema("deck.motorServoKiNmPerRad", "N m per rad", FLOAT, MEASURE),
    schema("deck.motorIntegralLimitNm", "N m", FLOAT, MEASURE),
    schema("deck.motorBrakeTorqueNm", "N m", FLOAT, MEASURE),
    schema(
        "deck.motorBrakeGainNmPerRadS",
        "N m per rad/s",
        FLOAT,
        MEASURE,
    ),
    schema("deck.bearingStaticTorqueNm", "N m", FLOAT, MEASURE),
    schema("deck.bearingKineticTorqueNm", "N m", FLOAT, MEASURE),
    schema(
        "deck.bearingViscousTorqueNmPerRadS",
        "N m per rad/s",
        FLOAT,
        MEASURE,
    ),
    schema("deck.slipmatStaticTorqueNm", "N m", FLOAT, MEASURE),
    schema("deck.slipmatKineticTorqueNm", "N m", FLOAT, MEASURE),
    schema(
        "deck.slipmatViscousTorqueNmPerRadS",
        "N m per rad/s",
        FLOAT,
        MEASURE,
    ),
    schema(
        "deck.handStaticFrictionCoefficient",
        "ratio",
        FLOAT,
        MEASURE,
    ),
    schema(
        "deck.handKineticFrictionCoefficient",
        "ratio",
        FLOAT,
        MEASURE,
    ),
    schema(
        "deck.handViscousTorqueNmPerRadS",
        "N m per rad/s",
        FLOAT,
        MEASURE,
    ),
    schema("deck.handPositionStabilizationSeconds", "s", FLOAT, MEASURE),
    schema(
        "deck.handMaxPositionCorrectionRadS",
        "rad/s",
        FLOAT,
        MEASURE,
    ),
    schema("deck.integrationHz", "Hz", FLOAT, DEFINE),
    schema("groove.outerProgramRadiusM", "m", FLOAT, MEASURE),
    schema("groove.innerProgramRadiusM", "m", FLOAT, MEASURE),
    schema("groove.nominalRpm", "rpm", FLOAT, MEASURE),
    schema("groove.grooveSampleRateHz", "Hz", FLOAT, DEFINE),
    schema("stylus.tracingRadiusM", "m", FLOAT, MEASURE),
    schema("contact.movingMassKg", "kg", FLOAT, MEASURE),
    schema("contact.grooveFrictionCoefficient", "ratio", FLOAT, MEASURE),
    schema(
        "contact.recordSurfaceFrictionCoefficient",
        "ratio",
        FLOAT,
        MEASURE,
    ),
    schema("tonearm.geometry.effectiveLengthM", "m", FLOAT, MEASURE),
    schema("tonearm.geometry.pivotToSpindleM", "m", FLOAT, MEASURE),
    schema("tonearm.geometry.offsetAngleDegrees", "deg", FLOAT, MEASURE),
    schema("tonearm.lateral.effectiveMassKg", "kg", FLOAT, MEASURE),
    schema("tonearm.lateral.complianceMPerN", "m/N", FLOAT, MEASURE),
    schema("tonearm.lateral.dampingRatio", "ratio", FLOAT, MEASURE),
    schema("tonearm.vertical.effectiveMassKg", "kg", FLOAT, MEASURE),
    schema("tonearm.vertical.complianceMPerN", "m/N", FLOAT, MEASURE),
    schema("tonearm.vertical.dampingRatio", "ratio", FLOAT, MEASURE),
    schema("tonearm.verticalTrackingForceN", "N", FLOAT, MEASURE),
    schema("tonearm.antiSkateForceN", "N", FLOAT, MEASURE),
    schema("tonearm.lateralBearingStaticFrictionN", "N", FLOAT, MEASURE),
    schema(
        "tonearm.lateralBearingKineticFrictionN",
        "N",
        FLOAT,
        MEASURE,
    ),
    schema(
        "tonearm.lateralBearingViscousDampingNSPerM",
        "N s/m",
        FLOAT,
        MEASURE,
    ),
    schema("tonearm.cueLiftHeightM", "m", FLOAT, MEASURE),
    schema("tonearm.cueSupportStiffnessNPerM", "N/m", FLOAT, MEASURE),
    schema("tonearm.cueSupportDampingNSPerM", "N s/m", FLOAT, MEASURE),
    schema("radialTracking.contactReleaseMarginM", "m", FLOAT, MEASURE),
    schema("radialTracking.recaptureInsetM", "m", FLOAT, MEASURE),
    schema(
        "radialTracking.maximumTurnsPerRecapture",
        "turns",
        UNSIGNED,
        DEFINE,
    ),
    schema(
        "cartridge.generatorCoefficientVSPerM",
        "V s/m",
        FLOAT,
        MEASURE,
    ),
    schema(
        "cartridge.generatorCoefficientSource",
        "variant",
        ENUM,
        MEASURE,
    ),
    schema("cartridge.coilResistanceOhm", "ohm", FLOAT, MEASURE),
    schema("cartridge.coilInductanceH", "H", FLOAT, MEASURE),
    schema("cartridge.coilMutualInductanceH", "H", FLOAT, MEASURE),
    schema("cartridge.loadResistanceOhm", "ohm", FLOAT, MEASURE),
    schema("cartridge.loadCapacitanceF", "F", FLOAT, MEASURE),
    schema("cartridge.channelBalanceDb", "dB", FLOAT, MEASURE),
    schema("cartridge.channelSeparationDb", "dB", FLOAT, MEASURE),
    schema(
        "recordCut.fullScaleSineVelocityRmsMS",
        "m/s RMS",
        FLOAT,
        MEASURE,
    ),
    schema("recordCut.cutterHighpassHz", "Hz", FLOAT, MEASURE),
    schema("recordCut.cutterBandwidthHz", "Hz", FLOAT, MEASURE),
    schema(
        "recordCut.groovePitchMPerRevolution",
        "m/revolution",
        FLOAT,
        MEASURE,
    ),
    schema("recordCut.grooveTopWidthM", "m", FLOAT, MEASURE),
    schema("recordCut.minimumLandWidthM", "m", FLOAT, MEASURE),
    schema("phono.gainDb", "dB", FLOAT, MEASURE),
    schema("phono.inputHeadroomVPeak", "V peak", FLOAT, MEASURE),
    schema("phono.outputHeadroomVPeak", "V peak", FLOAT, MEASURE),
    schema("phono.inputOverloadAttackSeconds", "s", FLOAT, MEASURE),
    schema("phono.overloadRecoverySeconds", "s", FLOAT, MEASURE),
    schema("phono.overloadGainReductionDb", "dB", FLOAT, MEASURE),
    schema("phono.outputSlewRateVPerS", "V/s", FLOAT, MEASURE),
    schema("phono.inputReferredNoiseVRms", "V RMS", FLOAT, MEASURE),
    schema("phono.noiseSeed", "integer", UNSIGNED, DEFINE),
    schema("solver.internalSampleRateHz", "Hz", FLOAT, DEFINE),
    schema("solver.maximumRenderFrames", "frames", UNSIGNED, DEFINE),
    schema("solver.controlTimelineCapacity", "events", UNSIGNED, DEFINE),
];

fn build_parameter_manifest(
    config: PhysicalPlaybackConfig,
) -> Result<Vec<ParameterManifestEntry>, PhysicalProfileError> {
    let serialized = serde_json::to_value(config)
        .map_err(|_| PhysicalProfileError::ConfigurationManifestMismatch)?;
    let mut leaves = BTreeMap::new();
    collect_scalar_leaves("", &serialized, &mut leaves)?;
    let schema_paths = PARAMETER_SCHEMA
        .iter()
        .map(|entry| entry.path)
        .collect::<BTreeSet<_>>();
    if schema_paths.len() != PARAMETER_SCHEMA.len()
        || leaves.len() != PARAMETER_SCHEMA.len()
        || leaves
            .keys()
            .any(|path| !schema_paths.contains(path.as_str()))
    {
        return Err(PhysicalProfileError::ConfigurationManifestMismatch);
    }

    PARAMETER_SCHEMA
        .iter()
        .map(|entry| {
            let serialized = leaves
                .get(entry.path)
                .ok_or(PhysicalProfileError::ConfigurationManifestMismatch)?;
            let value =
                match entry.value_kind {
                    ParameterValueKind::Float => serialized
                        .as_f64()
                        .map(ParameterValue::Float)
                        .ok_or(PhysicalProfileError::ConfigurationManifestMismatch)?,
                    ParameterValueKind::Unsigned => serialized
                        .as_u64()
                        .map(ParameterValue::Unsigned)
                        .ok_or(PhysicalProfileError::ConfigurationManifestMismatch)?,
                    ParameterValueKind::Enum => serialized
                        .as_str()
                        .map(|value| ParameterValue::Enum(value.to_owned()))
                        .ok_or(PhysicalProfileError::ConfigurationManifestMismatch)?,
                };
            Ok(ParameterManifestEntry {
                parameter: entry.path.to_owned(),
                value,
                unit: entry.unit.to_owned(),
                calibration_requirement: entry.requirement,
                calibration_test: match entry.requirement {
                    CalibrationEvidenceRequirement::DirectMeasurement => Some(
                        required_test_for_parameter(entry.path)
                            .ok_or(PhysicalProfileError::ConfigurationManifestMismatch)?,
                    ),
                    CalibrationEvidenceRequirement::DeclaredSetting => None,
                },
            })
        })
        .collect()
}

fn collect_scalar_leaves(
    prefix: &str,
    value: &serde_json::Value,
    leaves: &mut BTreeMap<String, serde_json::Value>,
) -> Result<(), PhysicalProfileError> {
    match value {
        serde_json::Value::Object(object) => {
            for (field, value) in object {
                let path = if prefix.is_empty() {
                    field.clone()
                } else {
                    format!("{prefix}.{field}")
                };
                collect_scalar_leaves(&path, value, leaves)?;
            }
            Ok(())
        }
        serde_json::Value::Number(_) | serde_json::Value::String(_) => {
            if prefix.is_empty() || leaves.insert(prefix.to_owned(), value.clone()).is_some() {
                Err(PhysicalProfileError::ConfigurationManifestMismatch)
            } else {
                Ok(())
            }
        }
        _ => Err(PhysicalProfileError::ConfigurationManifestMismatch),
    }
}

fn required_test_for_parameter(parameter: &str) -> Option<CalibrationTest> {
    if parameter == "deck.nominalRpm" {
        Some(CalibrationTest::PlatterSpeedStability)
    } else if parameter.starts_with("deck.bearing") {
        Some(CalibrationTest::BearingDragAndCoastdown)
    } else if parameter.starts_with("deck.slipmat") {
        Some(CalibrationTest::SlipmatFriction)
    } else if parameter.starts_with("deck.hand") {
        Some(CalibrationTest::HandContactFriction)
    } else if parameter.starts_with("deck.") {
        Some(CalibrationTest::PlatterTorqueSpeedAndStartup)
    } else if parameter.starts_with("groove.") || parameter.starts_with("recordCut.") {
        Some(CalibrationTest::GrooveGeometryAndCut)
    } else if parameter.starts_with("stylus.") {
        Some(CalibrationTest::StylusGeometry)
    } else if parameter.starts_with("contact.") {
        Some(CalibrationTest::PickupMechanicalResponse)
    } else if parameter.starts_with("tonearm.lateralBearing") {
        Some(CalibrationTest::RapidScratchTrackingAndRecovery)
    } else if parameter.starts_with("tonearm.") {
        Some(CalibrationTest::TonearmGeometryAndResonance)
    } else if parameter.starts_with("radialTracking.") {
        Some(CalibrationTest::RapidScratchTrackingAndRecovery)
    } else if matches!(
        parameter,
        "cartridge.coilResistanceOhm"
            | "cartridge.coilInductanceH"
            | "cartridge.coilMutualInductanceH"
            | "cartridge.loadResistanceOhm"
            | "cartridge.loadCapacitanceF"
    ) {
        Some(CalibrationTest::CartridgeImpedanceSweep)
    } else if matches!(
        parameter,
        "cartridge.channelBalanceDb" | "cartridge.channelSeparationDb"
    ) {
        Some(CalibrationTest::CartridgeOutputBalanceAndSeparation)
    } else if parameter.starts_with("cartridge.") {
        Some(CalibrationTest::CartridgeElectromechanicalReciprocity)
    } else if parameter.starts_with("phono.") {
        Some(CalibrationTest::PhonoGainNoiseAndOverload)
    } else {
        None
    }
}

fn seed_evidence(manifest: &[ParameterManifestEntry]) -> Vec<ParameterEvidence> {
    manifest
        .iter()
        .map(|entry| {
            let (kind, source, note) = seed_evidence_metadata(&entry.parameter);
            ParameterEvidence {
                parameter: entry.parameter.clone(),
                value: entry.value.clone(),
                unit: entry.unit.clone(),
                kind,
                calibration_test: entry.calibration_test,
                source: source.map(str::to_owned),
                note: note.to_owned(),
            }
        })
        .collect()
}

fn seed_evidence_metadata(parameter: &str) -> (EvidenceKind, Option<&'static str>, &'static str) {
    match parameter {
        "deck.nominalRpm" | "deck.motorStartingTorqueNm" => (
            EvidenceKind::Published,
            Some(TECHNICS_SOURCE),
            "Technics publishes this nominal value.",
        ),
        "groove.outerProgramRadiusM" | "groove.innerProgramRadiusM" | "groove.nominalRpm" => (
            EvidenceKind::Published,
            Some(AES_TRACING_SOURCE),
            "The source publishes this standard groove value.",
        ),
        "stylus.tracingRadiusM" => (
            EvidenceKind::Published,
            Some(ORTOFON_SOURCE),
            "Ortofon publishes the nominal stylus size.",
        ),
        "tonearm.geometry.effectiveLengthM" => (
            EvidenceKind::Published,
            Some(TECHNICS_SOURCE),
            "Technics publishes the effective length.",
        ),
        "tonearm.geometry.pivotToSpindleM" => (
            EvidenceKind::Calculated,
            Some(TECHNICS_SOURCE),
            "The value uses published length and overhang data.",
        ),
        "tonearm.lateral.complianceMPerN" => (
            EvidenceKind::Published,
            Some(ORTOFON_SOURCE),
            "Ortofon publishes the nominal lateral compliance.",
        ),
        "tonearm.verticalTrackingForceN" => (
            EvidenceKind::Calculated,
            Some(ORTOFON_SOURCE),
            "The value converts the published tracking-force setting.",
        ),
        "cartridge.coilResistanceOhm"
        | "cartridge.coilInductanceH"
        | "cartridge.loadResistanceOhm"
        | "cartridge.channelSeparationDb" => (
            EvidenceKind::Published,
            Some(ORTOFON_SOURCE),
            "Ortofon publishes this nominal cartridge value.",
        ),
        "cartridge.generatorCoefficientVSPerM" | "cartridge.generatorCoefficientSource" => (
            EvidenceKind::Calculated,
            Some(ORTOFON_SOURCE),
            "The seed derives this value from loaded output.",
        ),
        "deck.integrationHz"
        | "groove.grooveSampleRateHz"
        | "radialTracking.maximumTurnsPerRecapture"
        | "solver.internalSampleRateHz"
        | "solver.maximumRenderFrames"
        | "solver.controlTimelineCapacity" => (
            EvidenceKind::Defined,
            None,
            "The engine defines this processing setting.",
        ),
        _ => (
            EvidenceKind::Estimated,
            None,
            "A direct measurement must replace this seed value.",
        ),
    }
}

fn seed_calibration_tests() -> Vec<CalibrationTestDeclaration> {
    REQUIRED_CALIBRATION_TESTS
        .into_iter()
        .map(|test| {
            let (procedure, acceptance_criterion) = calibration_test_text(test);
            CalibrationTestDeclaration {
                test,
                procedure: procedure.to_owned(),
                acceptance_criterion: acceptance_criterion.to_owned(),
                acceptance_limits: Vec::new(),
            }
        })
        .collect()
}

fn calibration_test_text(test: CalibrationTest) -> (&'static str, &'static str) {
    match test {
        CalibrationTest::PlatterTorqueSpeedAndStartup => (
            "Measure torque and speed from rest through steady operation.",
            "The model must remain inside the registered uncertainty bands.",
        ),
        CalibrationTest::PlatterSpeedStability => (
            "Measure speed error, wow, and flutter during steady operation.",
            "All reported spectra must satisfy the registered limits.",
        ),
        CalibrationTest::MotorTorqueRippleAndControlLatency => (
            "Measure torque ripple, sensor response, and control latency across speed.",
            "All torque and timing results must satisfy the registered limits.",
        ),
        CalibrationTest::BearingDragAndCoastdown => (
            "Measure breakaway torque and coastdown at several speeds.",
            "The fitted drag curve must satisfy the registered residual limit.",
        ),
        CalibrationTest::SlipmatFriction => (
            "Measure slipmat breakaway, sliding torque, and speed dependence.",
            "The fitted contact model must satisfy the registered residual limit.",
        ),
        CalibrationTest::HandContactFriction => (
            "Measure record response for controlled finger force and motion.",
            "The fitted hand model must satisfy the registered transient limits.",
        ),
        CalibrationTest::RecordWarpEccentricityAndSpindle => (
            "Measure warp, eccentricity, spindle clearance, and hole offset for each record.",
            "All geometric motion results must satisfy the registered limits.",
        ),
        CalibrationTest::GrooveGeometryAndCut => (
            "Measure groove radii, pitch, displacement, and recorded velocity.",
            "Each geometric and velocity result must satisfy its registered limit.",
        ),
        CalibrationTest::GrooveMaterialContactAndWear => (
            "Measure compliance, friction, temperature, contamination, and repeated-pass wear.",
            "All material and wear results must satisfy the registered limits.",
        ),
        CalibrationTest::StylusGeometry => (
            "Measure the stylus contact geometry with calibrated imaging.",
            "The measured geometry must satisfy the registered uncertainty limit.",
        ),
        CalibrationTest::PickupMechanicalResponse => (
            "Measure pickup response, contact loss, and recapture behavior.",
            "Frequency and transient results must satisfy the registered limits.",
        ),
        CalibrationTest::TonearmGeometryAndResonance => (
            "Measure arm geometry, effective mass, compliance, damping, and resonance.",
            "All fitted values must satisfy the registered uncertainty limits.",
        ),
        CalibrationTest::CartridgeImpedanceSweep => (
            "Measure complex cartridge impedance across the registered frequency range.",
            "Magnitude and phase residuals must satisfy the registered limits.",
        ),
        CalibrationTest::CartridgeMagneticLossLevelAndTemperature => (
            "Measure magnetic loss and transfer response across level and temperature.",
            "All impedance and transfer changes must satisfy the registered limits.",
        ),
        CalibrationTest::CartridgeOutputBalanceAndSeparation => (
            "Measure output, balance, separation, and phase across frequency.",
            "Each channel result must satisfy the registered limits.",
        ),
        CalibrationTest::CartridgeElectromechanicalReciprocity => (
            "Measure generator output and reciprocal mechanical loading.",
            "Electrical and mechanical power residuals must satisfy registered limits.",
        ),
        CalibrationTest::RiaaAmplitudeAndPhase => (
            "Measure record and playback amplitude and phase responses.",
            "Both responses must satisfy the registered frequency limits.",
        ),
        CalibrationTest::PhonoGainNoiseAndOverload => (
            "Measure gain, noise, headroom, and overload recovery.",
            "All results must satisfy the registered operating limits.",
        ),
        CalibrationTest::RapidScratchTrackingAndRecovery => (
            "Measure tracking during rapid reversals, stops, throws, and recapture.",
            "No result can exceed the registered tracking or recovery limits.",
        ),
        CalibrationTest::EndToEndTestRecord => (
            "Compare rendered and measured output from a traceable test record.",
            "The complete response must satisfy the registered uncertainty envelope.",
        ),
    }
}

fn validate_evidence(
    evidence: &[ParameterEvidence],
    manifest: &[ParameterManifestEntry],
    status: CalibrationStatus,
) -> Result<(), PhysicalProfileError> {
    let manifest_by_path = manifest
        .iter()
        .map(|entry| (entry.parameter.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut evidence_by_path = BTreeMap::new();
    for entry in evidence {
        if entry.parameter.trim().is_empty()
            || entry.unit.trim().is_empty()
            || entry.note.trim().is_empty()
            || !entry.value.is_valid()
            || entry
                .source
                .as_ref()
                .is_some_and(|source| source.trim().is_empty())
        {
            return Err(PhysicalProfileError::InvalidEvidence);
        }
        let manifest_entry = manifest_by_path
            .get(entry.parameter.as_str())
            .ok_or_else(|| PhysicalProfileError::UnknownParameterEvidence {
                parameter: entry.parameter.clone(),
            })?;
        if evidence_by_path
            .insert(entry.parameter.as_str(), entry)
            .is_some()
        {
            return Err(PhysicalProfileError::DuplicateParameterEvidence {
                parameter: entry.parameter.clone(),
            });
        }
        if !entry.value.exactly_matches(&manifest_entry.value) {
            return Err(PhysicalProfileError::EvidenceValueMismatch {
                parameter: entry.parameter.clone(),
            });
        }
        if entry.unit != manifest_entry.unit {
            return Err(PhysicalProfileError::EvidenceUnitMismatch {
                parameter: entry.parameter.clone(),
            });
        }
        if entry.calibration_test != manifest_entry.calibration_test {
            return Err(PhysicalProfileError::EvidenceCalibrationTestMismatch {
                parameter: entry.parameter.clone(),
            });
        }
        if matches!(
            entry.kind,
            EvidenceKind::Published | EvidenceKind::Calculated | EvidenceKind::Measured
        ) && entry.source.is_none()
        {
            return Err(PhysicalProfileError::EvidenceSourceMissing {
                parameter: entry.parameter.clone(),
            });
        }
    }
    for manifest_entry in manifest {
        if !evidence_by_path.contains_key(manifest_entry.parameter.as_str()) {
            return Err(PhysicalProfileError::MissingParameterEvidence {
                parameter: manifest_entry.parameter.clone(),
            });
        }
    }

    if status == CalibrationStatus::Calibrated {
        for manifest_entry in manifest {
            let entry = evidence_by_path[manifest_entry.parameter.as_str()];
            if entry.kind == EvidenceKind::Estimated {
                return Err(PhysicalProfileError::CalibratedProfileContainsEstimate {
                    parameter: manifest_entry.parameter.clone(),
                });
            }
            if manifest_entry.calibration_requirement
                == CalibrationEvidenceRequirement::DirectMeasurement
                && entry.kind != EvidenceKind::Measured
            {
                return Err(PhysicalProfileError::DirectMeasurementRequired {
                    parameter: manifest_entry.parameter.clone(),
                });
            }
        }
    }
    Ok(())
}

fn validate_calibration_tests(
    declarations: &[CalibrationTestDeclaration],
    status: CalibrationStatus,
) -> Result<(), PhysicalProfileError> {
    let required = REQUIRED_CALIBRATION_TESTS
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut declared = BTreeSet::new();
    for declaration in declarations {
        if declaration.procedure.trim().is_empty()
            || declaration.acceptance_criterion.trim().is_empty()
        {
            return Err(PhysicalProfileError::InvalidCalibrationTest {
                test: declaration.test,
            });
        }
        let mut limit_metrics = BTreeSet::new();
        for limit in &declaration.acceptance_limits {
            if limit.metric.trim().is_empty()
                || limit.unit.trim().is_empty()
                || !limit.minimum_inclusive.is_finite()
                || !limit.maximum_inclusive.is_finite()
                || !limit.maximum_uncertainty.is_finite()
                || limit.minimum_inclusive > limit.maximum_inclusive
                || limit.maximum_uncertainty < 0.0
            {
                return Err(PhysicalProfileError::InvalidCalibrationLimit {
                    test: declaration.test,
                    metric: limit.metric.clone(),
                });
            }
            if !limit_metrics.insert(limit.metric.as_str()) {
                return Err(PhysicalProfileError::DuplicateCalibrationLimit {
                    test: declaration.test,
                    metric: limit.metric.clone(),
                });
            }
        }
        if status == CalibrationStatus::Calibrated && declaration.acceptance_limits.is_empty() {
            return Err(PhysicalProfileError::MissingCalibrationLimits {
                test: declaration.test,
            });
        }
        if !declared.insert(declaration.test) {
            return Err(PhysicalProfileError::DuplicateCalibrationTest {
                test: declaration.test,
            });
        }
    }
    if declared != required {
        return Err(PhysicalProfileError::IncompleteCalibrationTestManifest);
    }
    Ok(())
}

fn validate_calibration_results(
    results: &[CalibrationValidationResult],
    declarations: &[CalibrationTestDeclaration],
    status: CalibrationStatus,
) -> Result<(), PhysicalProfileError> {
    let declared = declarations
        .iter()
        .map(|declaration| (declaration.test, declaration))
        .collect::<BTreeMap<_, _>>();
    let mut results_by_test = BTreeMap::new();
    for result in results {
        let Some(declaration) = declared.get(&result.test) else {
            return Err(PhysicalProfileError::UnknownCalibrationResult { test: result.test });
        };
        if results_by_test.insert(result.test, result).is_some() {
            return Err(PhysicalProfileError::DuplicateCalibrationResult { test: result.test });
        }
        if result.artifact.trim().is_empty()
            || result.note.trim().is_empty()
            || !is_sha256(&result.artifact_sha256)
            || result.measurements.is_empty()
            || result.measurements.iter().any(|metric| {
                metric.name.trim().is_empty()
                    || metric.unit.trim().is_empty()
                    || !metric.value.is_finite()
                    || !metric.uncertainty.is_finite()
                    || metric.uncertainty < 0.0
            })
        {
            return Err(PhysicalProfileError::InvalidCalibrationResult { test: result.test });
        }
        let mut measurements = BTreeMap::new();
        for measurement in &result.measurements {
            if measurements
                .insert(measurement.name.as_str(), measurement)
                .is_some()
            {
                return Err(PhysicalProfileError::DuplicateCalibrationMetric {
                    test: result.test,
                    metric: measurement.name.clone(),
                });
            }
        }
        let mut computed_pass = !declaration.acceptance_limits.is_empty();
        for limit in &declaration.acceptance_limits {
            let measurement = measurements.get(limit.metric.as_str()).ok_or_else(|| {
                PhysicalProfileError::MissingCalibrationMetric {
                    test: result.test,
                    metric: limit.metric.clone(),
                }
            })?;
            if measurement.unit != limit.unit {
                return Err(PhysicalProfileError::CalibrationMetricUnitMismatch {
                    test: result.test,
                    metric: limit.metric.clone(),
                });
            }
            let measured_minimum = measurement.value - measurement.uncertainty;
            let measured_maximum = measurement.value + measurement.uncertainty;
            computed_pass &= measured_minimum.is_finite()
                && measured_maximum.is_finite()
                && measurement.uncertainty <= limit.maximum_uncertainty
                && measured_minimum >= limit.minimum_inclusive
                && measured_maximum <= limit.maximum_inclusive;
        }
        if result.passed != computed_pass {
            return Err(PhysicalProfileError::CalibrationVerdictMismatch { test: result.test });
        }
    }

    if status == CalibrationStatus::Calibrated {
        for declaration in declarations {
            let result = results_by_test.get(&declaration.test).ok_or(
                PhysicalProfileError::MissingCalibrationResult {
                    test: declaration.test,
                },
            )?;
            if result.kind != EvidenceKind::Measured {
                return Err(PhysicalProfileError::MeasuredCalibrationResultRequired {
                    test: declaration.test,
                });
            }
            if !result.passed {
                return Err(PhysicalProfileError::CalibrationTestFailed {
                    test: declaration.test,
                });
            }
        }
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug, Error)]
pub enum PhysicalProfileError {
    #[error(transparent)]
    Deck(#[from] crate::PhysicalDeckConfigError),
    #[error(transparent)]
    Groove(#[from] super::GrooveError),
    #[error(transparent)]
    Stylus(#[from] super::StylusTraceError),
    #[error(transparent)]
    Pickup(#[from] super::PickupMechanicalError),
    #[error(transparent)]
    Tonearm(#[from] super::TonearmError),
    #[error(transparent)]
    RadialTracking(#[from] super::RadialTrackingError),
    #[error(transparent)]
    Cartridge(#[from] super::MovingMagnetCartridgeConfigError),
    #[error(transparent)]
    Phono(#[from] super::PhonoStageConfigError),
    #[error(transparent)]
    Riaa(#[from] super::RiaaError),
    #[error("solver field {field} is invalid")]
    InvalidSolver { field: &'static str },
    #[error("internal sample rates do not agree")]
    InconsistentInternalSampleRate,
    #[error("deck and groove nominal RPM values do not agree")]
    InconsistentNominalRpm,
    #[error("profile name is empty")]
    EmptyName,
    #[error("configuration fields do not match the parameter manifest")]
    ConfigurationManifestMismatch,
    #[error("parameter evidence is invalid")]
    InvalidEvidence,
    #[error("parameter evidence path {parameter} is unknown")]
    UnknownParameterEvidence { parameter: String },
    #[error("parameter evidence path {parameter} occurs more than once")]
    DuplicateParameterEvidence { parameter: String },
    #[error("parameter evidence is missing for {parameter}")]
    MissingParameterEvidence { parameter: String },
    #[error("parameter evidence value does not match {parameter}")]
    EvidenceValueMismatch { parameter: String },
    #[error("parameter evidence unit does not match {parameter}")]
    EvidenceUnitMismatch { parameter: String },
    #[error("parameter evidence calibration test does not match {parameter}")]
    EvidenceCalibrationTestMismatch { parameter: String },
    #[error("parameter evidence source is missing for {parameter}")]
    EvidenceSourceMissing { parameter: String },
    #[error("calibrated parameter {parameter} still contains an estimate")]
    CalibratedProfileContainsEstimate { parameter: String },
    #[error("calibrated parameter {parameter} requires direct measurement")]
    DirectMeasurementRequired { parameter: String },
    #[error("the cartridge generator coefficient is not a direct measurement")]
    GeneratorCoefficientNotDirectlyMeasured,
    #[error("calibration test {test:?} is invalid")]
    InvalidCalibrationTest { test: CalibrationTest },
    #[error("calibration test {test:?} occurs more than once")]
    DuplicateCalibrationTest { test: CalibrationTest },
    #[error("the calibration test manifest is incomplete")]
    IncompleteCalibrationTestManifest,
    #[error("calibration test {test:?} has an invalid limit for {metric}")]
    InvalidCalibrationLimit {
        test: CalibrationTest,
        metric: String,
    },
    #[error("calibration test {test:?} has more than one limit for {metric}")]
    DuplicateCalibrationLimit {
        test: CalibrationTest,
        metric: String,
    },
    #[error("calibration test {test:?} has no registered numeric limits")]
    MissingCalibrationLimits { test: CalibrationTest },
    #[error("calibration result {test:?} is not declared")]
    UnknownCalibrationResult { test: CalibrationTest },
    #[error("calibration result {test:?} occurs more than once")]
    DuplicateCalibrationResult { test: CalibrationTest },
    #[error("calibration result {test:?} is invalid")]
    InvalidCalibrationResult { test: CalibrationTest },
    #[error("calibration result {test:?} has more than one measurement for {metric}")]
    DuplicateCalibrationMetric {
        test: CalibrationTest,
        metric: String,
    },
    #[error("calibration result {test:?} is missing the measurement for {metric}")]
    MissingCalibrationMetric {
        test: CalibrationTest,
        metric: String,
    },
    #[error("calibration result {test:?} uses the wrong unit for {metric}")]
    CalibrationMetricUnitMismatch {
        test: CalibrationTest,
        metric: String,
    },
    #[error("calibration result {test:?} has a verdict that does not match its numeric limits")]
    CalibrationVerdictMismatch { test: CalibrationTest },
    #[error("calibration result {test:?} is missing")]
    MissingCalibrationResult { test: CalibrationTest },
    #[error("calibration result {test:?} must contain measured evidence")]
    MeasuredCalibrationResultRequired { test: CalibrationTest },
    #[error("calibration test {test:?} did not pass")]
    CalibrationTestFailed { test: CalibrationTest },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physical::{MAXIMUM_PHONO_GAIN_DB, MAXIMUM_PHONO_HEADROOM_V_PEAK};

    fn synthetic_result(test: CalibrationTest) -> CalibrationValidationResult {
        CalibrationValidationResult {
            test,
            kind: EvidenceKind::Measured,
            passed: true,
            artifact: format!("test-artifact://{test:?}"),
            artifact_sha256: "a".repeat(64),
            measurements: vec![CalibrationMetric {
                name: "residual".to_owned(),
                value: 0.0,
                unit: "ratio".to_owned(),
                uncertainty: 0.001,
            }],
            note: "Synthetic validation fixture.".to_owned(),
        }
    }

    fn synthetic_calibrated_profile() -> PhysicalProfile {
        let mut profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        profile.status = CalibrationStatus::Calibrated;
        for declaration in &mut profile.calibration_tests {
            declaration
                .acceptance_limits
                .push(CalibrationAcceptanceLimit {
                    metric: "residual".to_owned(),
                    unit: "ratio".to_owned(),
                    minimum_inclusive: -0.01,
                    maximum_inclusive: 0.01,
                    maximum_uncertainty: 0.002,
                });
        }
        profile.config.cartridge.generator_coefficient_source =
            GeneratorCoefficientSource::DirectMeasurement;
        let manifest = profile.parameter_manifest().unwrap();
        for evidence in &mut profile.evidence {
            let requirement = manifest
                .iter()
                .find(|entry| entry.parameter == evidence.parameter)
                .unwrap();
            evidence.value = requirement.value.clone();
            evidence.kind = if requirement.calibration_requirement
                == CalibrationEvidenceRequirement::DirectMeasurement
            {
                EvidenceKind::Measured
            } else {
                EvidenceKind::Defined
            };
            evidence.source = (evidence.kind == EvidenceKind::Measured)
                .then(|| format!("test-measurement://{}", evidence.parameter));
            evidence.note = "Synthetic calibration fixture.".to_owned();
        }
        profile.calibration_results = REQUIRED_CALIBRATION_TESTS
            .into_iter()
            .map(synthetic_result)
            .collect();
        profile
    }

    #[test]
    fn seed_manifest_covers_every_serialized_configuration_leaf() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let manifest = profile.parameter_manifest().unwrap();
        assert_eq!(manifest.len(), 76);
        assert_eq!(profile.evidence.len(), manifest.len());
        assert!(profile.validate().is_ok());
        assert_eq!(profile.status, CalibrationStatus::Seed);
        assert!(!profile.permits_calibrated_claim());
    }

    #[test]
    fn mutual_inductance_seed_is_zero_and_requires_an_impedance_measurement() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let evidence = profile
            .evidence
            .iter()
            .find(|entry| entry.parameter == "cartridge.coilMutualInductanceH")
            .unwrap();
        assert_eq!(evidence.value, ParameterValue::Float(0.0));
        assert_eq!(evidence.kind, EvidenceKind::Estimated);
        assert_eq!(
            evidence.calibration_test,
            Some(CalibrationTest::CartridgeImpedanceSweep)
        );
        assert!(evidence.source.is_none());
    }

    #[test]
    fn every_new_phono_seed_value_is_estimated() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        for parameter in [
            "phono.inputOverloadAttackSeconds",
            "phono.overloadRecoverySeconds",
            "phono.overloadGainReductionDb",
            "phono.outputSlewRateVPerS",
            "phono.inputReferredNoiseVRms",
            "phono.noiseSeed",
        ] {
            let evidence = profile
                .evidence
                .iter()
                .find(|entry| entry.parameter == parameter)
                .unwrap();
            assert_eq!(evidence.kind, EvidenceKind::Estimated, "{parameter}");
            let expected_test = if parameter == "phono.noiseSeed" {
                None
            } else {
                Some(CalibrationTest::PhonoGainNoiseAndOverload)
            };
            assert_eq!(evidence.calibration_test, expected_test, "{parameter}");
            assert!(evidence.source.is_none(), "{parameter}");
        }
    }

    #[test]
    fn exact_float_enum_and_unsigned_values_are_bound_to_configuration() {
        let mut profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let float = profile
            .evidence
            .iter_mut()
            .find(|entry| entry.parameter == "deck.nominalRpm")
            .unwrap();
        let ParameterValue::Float(value) = &mut float.value else {
            panic!("expected a float value");
        };
        *value = f64::from_bits(value.to_bits() + 1);
        assert!(matches!(
            profile.validate(),
            Err(PhysicalProfileError::EvidenceValueMismatch { .. })
        ));

        let mut profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        profile
            .evidence
            .iter_mut()
            .find(|entry| entry.parameter == "cartridge.generatorCoefficientSource")
            .unwrap()
            .value = ParameterValue::Enum("directMeasurement".to_owned());
        assert!(matches!(
            profile.validate(),
            Err(PhysicalProfileError::EvidenceValueMismatch { .. })
        ));

        let mut profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        profile
            .evidence
            .iter_mut()
            .find(|entry| entry.parameter == "solver.maximumRenderFrames")
            .unwrap()
            .value = ParameterValue::Unsigned(2_049);
        assert!(matches!(
            profile.validate(),
            Err(PhysicalProfileError::EvidenceValueMismatch { .. })
        ));
    }

    #[test]
    fn missing_duplicate_unknown_and_wrong_unit_evidence_are_rejected() {
        let mut missing = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        missing.evidence.pop();
        assert!(matches!(
            missing.validate(),
            Err(PhysicalProfileError::MissingParameterEvidence { .. })
        ));

        let mut duplicate = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        duplicate.evidence.push(duplicate.evidence[0].clone());
        assert!(matches!(
            duplicate.validate(),
            Err(PhysicalProfileError::DuplicateParameterEvidence { .. })
        ));

        let mut unknown = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        unknown.evidence[0].parameter = "deck.unknownTorque".to_owned();
        assert!(matches!(
            unknown.validate(),
            Err(PhysicalProfileError::UnknownParameterEvidence { .. })
        ));

        let mut wrong_unit = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        wrong_unit.evidence[0].unit = "rad/s".to_owned();
        assert!(matches!(
            wrong_unit.validate(),
            Err(PhysicalProfileError::EvidenceUnitMismatch { .. })
        ));

        let mut wrong_test = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        wrong_test.evidence[0].calibration_test = Some(CalibrationTest::StylusGeometry);
        assert!(matches!(
            wrong_test.validate(),
            Err(PhysicalProfileError::EvidenceCalibrationTestMismatch { .. })
        ));
    }

    #[test]
    fn changing_configuration_invalidates_its_old_evidence() {
        let mut profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        profile.config.phono.gain_db = 41.0;
        assert!(matches!(
            profile.validate(),
            Err(PhysicalProfileError::EvidenceValueMismatch { .. })
        ));
    }

    #[test]
    fn phono_engineering_bounds_reject_numeric_hazards() {
        let mut excessive_gain = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        excessive_gain.config.phono.gain_db = MAXIMUM_PHONO_GAIN_DB + 1.0;
        assert!(matches!(
            excessive_gain.validate(),
            Err(PhysicalProfileError::Phono(
                super::super::PhonoStageConfigError::InvalidField { field: "gainDb" }
            ))
        ));

        let mut excessive_headroom = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        excessive_headroom.config.phono.output_headroom_v_peak =
            MAXIMUM_PHONO_HEADROOM_V_PEAK + 1.0;
        assert!(matches!(
            excessive_headroom.validate(),
            Err(PhysicalProfileError::Phono(
                super::super::PhonoStageConfigError::InvalidField {
                    field: "outputHeadroomVPeak"
                }
            ))
        ));
    }

    #[test]
    fn one_measured_parameter_row_cannot_authorize_a_claim() {
        let mut profile = synthetic_calibrated_profile();
        for evidence in &mut profile.evidence {
            if evidence.kind == EvidenceKind::Measured {
                evidence.kind = EvidenceKind::Published;
            }
        }
        profile.evidence[0].kind = EvidenceKind::Measured;
        assert!(matches!(
            profile.validate(),
            Err(PhysicalProfileError::DirectMeasurementRequired { .. })
        ));
        assert!(!profile.permits_calibrated_claim());
    }

    #[test]
    fn published_or_calculated_rows_cannot_replace_direct_measurements() {
        for kind in [EvidenceKind::Published, EvidenceKind::Calculated] {
            let mut profile = synthetic_calibrated_profile();
            let entry = profile
                .evidence
                .iter_mut()
                .find(|entry| entry.parameter == "contact.movingMassKg")
                .unwrap();
            entry.kind = kind;
            assert!(matches!(
                profile.validate(),
                Err(PhysicalProfileError::DirectMeasurementRequired { .. })
            ));
        }
    }

    #[test]
    fn one_measured_validation_row_cannot_authorize_a_claim() {
        let mut profile = synthetic_calibrated_profile();
        profile.calibration_results.truncate(1);
        assert!(matches!(
            profile.validate(),
            Err(PhysicalProfileError::MissingCalibrationResult { .. })
        ));
        assert!(!profile.permits_calibrated_claim());
    }

    #[test]
    fn calibrated_profiles_require_registered_numeric_limits() {
        let mut profile = synthetic_calibrated_profile();
        profile.calibration_tests[0].acceptance_limits.clear();
        assert!(matches!(
            profile.validate(),
            Err(PhysicalProfileError::MissingCalibrationLimits { .. })
        ));
        assert!(!profile.permits_calibrated_claim());
    }

    #[test]
    fn numeric_limits_bind_metric_names_units_and_uncertainty() {
        let mut missing = synthetic_calibrated_profile();
        missing.calibration_results[0].measurements[0].name = "other".to_owned();
        assert!(matches!(
            missing.validate(),
            Err(PhysicalProfileError::MissingCalibrationMetric { .. })
        ));

        let mut wrong_unit = synthetic_calibrated_profile();
        wrong_unit.calibration_results[0].measurements[0].unit = "dB".to_owned();
        assert!(matches!(
            wrong_unit.validate(),
            Err(PhysicalProfileError::CalibrationMetricUnitMismatch { .. })
        ));

        let mut excessive_uncertainty = synthetic_calibrated_profile();
        excessive_uncertainty.calibration_results[0].measurements[0].uncertainty = 0.003;
        assert!(matches!(
            excessive_uncertainty.validate(),
            Err(PhysicalProfileError::CalibrationVerdictMismatch { .. })
        ));
    }

    #[test]
    fn a_reported_verdict_must_equal_the_registered_limit_result() {
        let mut false_positive = synthetic_calibrated_profile();
        false_positive.calibration_results[0].measurements[0].value = 0.02;
        assert!(matches!(
            false_positive.validate(),
            Err(PhysicalProfileError::CalibrationVerdictMismatch { .. })
        ));

        let mut false_negative = synthetic_calibrated_profile();
        false_negative.calibration_results[0].passed = false;
        assert!(matches!(
            false_negative.validate(),
            Err(PhysicalProfileError::CalibrationVerdictMismatch { .. })
        ));
    }

    #[test]
    fn duplicate_and_invalid_numeric_limits_are_rejected() {
        let mut duplicate = synthetic_calibrated_profile();
        let repeated = duplicate.calibration_tests[0].acceptance_limits[0].clone();
        duplicate.calibration_tests[0]
            .acceptance_limits
            .push(repeated);
        assert!(matches!(
            duplicate.validate(),
            Err(PhysicalProfileError::DuplicateCalibrationLimit { .. })
        ));

        let mut invalid = synthetic_calibrated_profile();
        invalid.calibration_tests[0].acceptance_limits[0].minimum_inclusive = 1.0;
        invalid.calibration_tests[0].acceptance_limits[0].maximum_inclusive = -1.0;
        assert!(matches!(
            invalid.validate(),
            Err(PhysicalProfileError::InvalidCalibrationLimit { .. })
        ));
    }

    #[test]
    fn every_declared_test_requires_a_unique_measured_passing_result() {
        let mut duplicate_test = synthetic_calibrated_profile();
        duplicate_test
            .calibration_tests
            .push(duplicate_test.calibration_tests[0].clone());
        assert!(matches!(
            duplicate_test.validate(),
            Err(PhysicalProfileError::DuplicateCalibrationTest { .. })
        ));

        let mut duplicate_result = synthetic_calibrated_profile();
        duplicate_result
            .calibration_results
            .push(duplicate_result.calibration_results[0].clone());
        assert!(matches!(
            duplicate_result.validate(),
            Err(PhysicalProfileError::DuplicateCalibrationResult { .. })
        ));

        let mut published = synthetic_calibrated_profile();
        published.calibration_results[0].kind = EvidenceKind::Published;
        assert!(matches!(
            published.validate(),
            Err(PhysicalProfileError::MeasuredCalibrationResultRequired { .. })
        ));

        let mut failed = synthetic_calibrated_profile();
        failed.calibration_results[0].passed = false;
        failed.calibration_results[0].measurements[0].value = 0.02;
        assert!(matches!(
            failed.validate(),
            Err(PhysicalProfileError::CalibrationTestFailed { .. })
        ));
    }

    #[test]
    fn generator_source_must_identify_a_direct_measurement() {
        let mut profile = synthetic_calibrated_profile();
        profile.config.cartridge.generator_coefficient_source =
            GeneratorCoefficientSource::UserSupplied;
        let manifest = profile.parameter_manifest().unwrap();
        profile
            .evidence
            .iter_mut()
            .find(|entry| entry.parameter == "cartridge.generatorCoefficientSource")
            .unwrap()
            .value = manifest
            .iter()
            .find(|entry| entry.parameter == "cartridge.generatorCoefficientSource")
            .unwrap()
            .value
            .clone();
        assert!(matches!(
            profile.validate(),
            Err(PhysicalProfileError::GeneratorCoefficientNotDirectlyMeasured)
        ));
    }

    #[test]
    fn complete_synthetic_manifest_passes_the_structural_gate() {
        let profile = synthetic_calibrated_profile();
        assert!(profile.validate().is_ok());
        assert!(profile.permits_calibrated_claim());
    }

    #[test]
    fn cross_component_rates_and_rpm_must_agree() {
        let mut profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        profile.config.groove.groove_sample_rate_hz = 96_000.0;
        assert!(matches!(
            profile.validate(),
            Err(PhysicalProfileError::InconsistentInternalSampleRate)
        ));
        let mut profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        profile.config.groove.nominal_rpm = 45.0;
        assert!(matches!(
            profile.validate(),
            Err(PhysicalProfileError::InconsistentNominalRpm)
        ));
    }

    #[test]
    fn a_self_consistent_96_khz_profile_is_not_canonical() {
        let mut profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        profile.config.solver.internal_sample_rate_hz = 96_000.0;
        profile.config.deck.integration_hz = 96_000.0;
        profile.config.groove.groove_sample_rate_hz = 96_000.0;
        assert!(matches!(
            profile.validate(),
            Err(PhysicalProfileError::InvalidSolver {
                field: "internalSampleRateHz"
            })
        ));
    }

    #[test]
    fn solver_capacity_limits_accept_boundaries_and_reject_larger_values() {
        let canonical_rate = f64::from(PHYSICAL_OUTPUT_INPUT_RATE_HZ);
        assert!(PhysicalSolverConfig {
            internal_sample_rate_hz: canonical_rate,
            maximum_render_frames: 1,
            control_timeline_capacity: 1,
        }
        .validate()
        .is_ok());
        assert!(PhysicalSolverConfig {
            internal_sample_rate_hz: canonical_rate,
            maximum_render_frames: MAXIMUM_PHYSICAL_RENDER_FRAMES,
            control_timeline_capacity: MAXIMUM_PHYSICAL_CONTROL_TIMELINE_CAPACITY,
        }
        .validate()
        .is_ok());
        assert!(matches!(
            PhysicalSolverConfig {
                internal_sample_rate_hz: canonical_rate,
                maximum_render_frames: MAXIMUM_PHYSICAL_RENDER_FRAMES + 1,
                control_timeline_capacity: 1,
            }
            .validate(),
            Err(PhysicalProfileError::InvalidSolver {
                field: "maximumRenderFrames"
            })
        ));
        assert!(matches!(
            PhysicalSolverConfig {
                internal_sample_rate_hz: canonical_rate,
                maximum_render_frames: 0,
                control_timeline_capacity: 1,
            }
            .validate(),
            Err(PhysicalProfileError::InvalidSolver {
                field: "maximumRenderFrames"
            })
        ));
        assert!(matches!(
            PhysicalSolverConfig {
                internal_sample_rate_hz: canonical_rate,
                maximum_render_frames: 1,
                control_timeline_capacity: MAXIMUM_PHYSICAL_CONTROL_TIMELINE_CAPACITY + 1,
            }
            .validate(),
            Err(PhysicalProfileError::InvalidSolver {
                field: "controlTimelineCapacity"
            })
        ));
        assert!(matches!(
            PhysicalSolverConfig {
                internal_sample_rate_hz: canonical_rate,
                maximum_render_frames: 1,
                control_timeline_capacity: 0,
            }
            .validate(),
            Err(PhysicalProfileError::InvalidSolver {
                field: "controlTimelineCapacity"
            })
        ));
    }
}
