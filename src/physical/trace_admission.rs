//! Certifies groove representations before a physical tracer can use them.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::groove::{
    align_up, GrooveContentHasher, GrooveContentIdentity, GROOVE_SPATIAL_FILTER_RADIUS_FRAMES,
    GROOVE_SPATIAL_PYRAMID_FORMAT_VERSION, GROOVE_SPATIAL_PYRAMID_LEVELS,
};
use super::stylus::{
    StylusGeometry, SPHERICAL_TRACE_CONTACT_POSITION_ERROR_BOUND_M,
    SPHERICAL_TRACE_GROOVE_SLOPE_ERROR_BOUND, SPHERICAL_TRACE_HEIGHT_ERROR_BOUND_M,
    SPHERICAL_TRACE_MAX_GLOBAL_CONTENDERS, SPHERICAL_TRACE_MAX_MONOTONE_BRANCHES_PER_PIECE,
    SPHERICAL_TRACE_MAX_PIECES, SPHERICAL_TRACE_MAX_ROOT_BRACKETS_PER_PIECE,
    SPHERICAL_TRACE_MAX_ROOT_NODES_PER_PIECE, SPHERICAL_TRACE_TANGENT_RESIDUAL_ERROR_BOUND,
};

/// Identifies the certificate schema and its proof rules.
pub const GROOVE_TRACE_ADMISSION_CERTIFICATE_VERSION: u32 = 3;

/// Identifies the certified-concave spherical tracing algorithm.
pub const CERTIFIED_CONCAVE_TRACER_ALGORITHM_VERSION: u32 = 2;

/// Identifies the contiguous byte representation covered by this schema.
pub const CONTIGUOUS_TRACE_REPRESENTATION_FORMAT_VERSION: u32 = 4;

/// Identifies the paged byte representation covered by this schema.
pub const PAGED_TRACE_REPRESENTATION_FORMAT_VERSION: u32 = 4;

/// Defines the complete speed domain covered by one certificate.
pub const CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_SOURCE_FRAME_ADVANCE: f64 = 20.0;

/// Covers both one-sided endpoints at every nonexact spline join.
pub const CERTIFIED_TRACE_MAXIMUM_ENDPOINT_CONTENDERS_PER_PIECE: u8 = 2;

/// Prevents admitted walls from approaching a vertical tangent.
pub const CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_WALL_SLOPE: f64 = 16.0;

/// Bounds numerical C0 mismatch at an internal spline join.
pub const CERTIFIED_TRACE_MAXIMUM_INTERNAL_DISPLACEMENT_JOIN_ENCLOSURE_M: f64 =
    SPHERICAL_TRACE_HEIGHT_ERROR_BOUND_M;

/// Reserves half the sphere curvature for deterministic numerical isolation.
pub const CERTIFIED_TRACE_MAXIMUM_CONCAVITY_RATIO: f64 = 0.5;

const BASE_LEVEL_COVERAGE_BIT: u8 = 1;
const ALL_SPATIAL_LEVEL_COVERAGE_BITS: u8 = ((1_u16 << GROOVE_SPATIAL_PYRAMID_LEVELS) - 1) as u8;

const WALL_SCALES: [(f64, f64); 2] = [
    (
        std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
    ),
    (
        -std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
    ),
];

/// Identifies the immutable representation covered by one certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GrooveTraceRepresentationKind {
    Contiguous,
    Paged,
}

/// Selects the numerical proof that authorizes an active trace path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GrooveTraceAdmissionClass {
    /// Every covered spherical envelope is strictly concave.
    StrictConcavity,
    /// Fixed caps authorize bounded runtime isolation of the global height order.
    FixedCapPiecewise,
    /// A wall exceeds the certified finite-slope domain.
    RejectedWallSlope,
    /// The geometry and sampling support do not fit the fixed work caps.
    RejectedFixedWorkSupport,
    /// A representation join exceeds the certified numerical enclosure.
    RejectedRepresentationJoin,
}

impl GrooveTraceAdmissionClass {
    pub(crate) fn rejection_error(self) -> Option<GrooveTraceAdmissionError> {
        match self {
            Self::StrictConcavity | Self::FixedCapPiecewise => None,
            Self::RejectedWallSlope => Some(GrooveTraceAdmissionError::WallSlopeLimitExceeded),
            Self::RejectedFixedWorkSupport => Some(GrooveTraceAdmissionError::FixedWorkNotProved),
            Self::RejectedRepresentationJoin => {
                Some(GrooveTraceAdmissionError::RepresentationJoinNotAdmitted)
            }
        }
    }
}

/// Declares the exact trace domain for one paged source.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveTraceAdmissionPolicy {
    certificate_version: u32,
    tracer_algorithm_version: u32,
    maximum_geometry: StylusGeometry,
    maximum_source_frame_advance: f64,
    spatial_pyramid_format_version: u32,
    spatial_level_count: u8,
    policy_identity: GrooveContentIdentity,
}

impl GrooveTraceAdmissionPolicy {
    pub(crate) fn standard(
        source_content_identity: GrooveContentIdentity,
    ) -> Result<Self, GrooveTraceAdmissionError> {
        let mut policy = Self {
            certificate_version: GROOVE_TRACE_ADMISSION_CERTIFICATE_VERSION,
            tracer_algorithm_version: CERTIFIED_CONCAVE_TRACER_ALGORITHM_VERSION,
            maximum_geometry: StylusGeometry::default(),
            maximum_source_frame_advance: CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_SOURCE_FRAME_ADVANCE,
            spatial_pyramid_format_version: GROOVE_SPATIAL_PYRAMID_FORMAT_VERSION,
            spatial_level_count: GROOVE_SPATIAL_PYRAMID_LEVELS as u8,
            policy_identity: GrooveContentIdentity::from_sha256([0; 32]),
        };
        policy.policy_identity = policy.calculate_identity(source_content_identity);
        policy.validate(source_content_identity)?;
        Ok(policy)
    }

    pub fn certificate_version(self) -> u32 {
        self.certificate_version
    }

    pub fn tracer_algorithm_version(self) -> u32 {
        self.tracer_algorithm_version
    }

    pub fn maximum_geometry(self) -> StylusGeometry {
        self.maximum_geometry
    }

    pub fn maximum_source_frame_advance(self) -> f64 {
        self.maximum_source_frame_advance
    }

    pub fn policy_identity(self) -> GrooveContentIdentity {
        self.policy_identity
    }

    pub(crate) fn validate(
        self,
        source_content_identity: GrooveContentIdentity,
    ) -> Result<(), GrooveTraceAdmissionError> {
        self.maximum_geometry
            .validate()
            .map_err(|_| GrooveTraceAdmissionError::InvalidGeometry)?;
        self.policy_identity
            .validate_current()
            .map_err(|_| GrooveTraceAdmissionError::InvalidPolicy)?;
        if self.certificate_version != GROOVE_TRACE_ADMISSION_CERTIFICATE_VERSION
            || self.tracer_algorithm_version != CERTIFIED_CONCAVE_TRACER_ALGORITHM_VERSION
            || self.maximum_source_frame_advance
                != CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_SOURCE_FRAME_ADVANCE
            || self.spatial_pyramid_format_version != GROOVE_SPATIAL_PYRAMID_FORMAT_VERSION
            || self.spatial_level_count != GROOVE_SPATIAL_PYRAMID_LEVELS as u8
            || self.policy_identity != self.calculate_identity(source_content_identity)
        {
            return Err(GrooveTraceAdmissionError::InvalidPolicy);
        }
        Ok(())
    }

    fn calculate_identity(
        self,
        source_content_identity: GrooveContentIdentity,
    ) -> GrooveContentIdentity {
        let mut hash = GrooveContentHasher::new(b"record-player-trace-admission-policy-v1\0");
        hash.identity(source_content_identity);
        hash.u32(self.certificate_version);
        hash.u32(self.tracer_algorithm_version);
        hash.f64(self.maximum_geometry.tracing_radius_m);
        hash.f64(self.maximum_source_frame_advance);
        hash.u32(self.spatial_pyramid_format_version);
        hash.u8(self.spatial_level_count);
        hash.finish()
    }
}

/// States how a certified representation handles spline-domain edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveTraceEdgeCoverage {
    /// The proof includes the constant extension before record frame zero.
    record_left_clamp: bool,
    /// The proof includes the constant extension after the final record frame.
    record_right_clamp: bool,
    /// A validated halo excludes the left page-storage clamp from tracing.
    page_left_clamp_excluded_by_halo: bool,
    /// A validated halo excludes the right page-storage clamp from tracing.
    page_right_clamp_excluded_by_halo: bool,
}

impl GrooveTraceEdgeCoverage {
    pub(crate) fn contiguous() -> Self {
        Self {
            record_left_clamp: true,
            record_right_clamp: true,
            page_left_clamp_excluded_by_halo: false,
            page_right_clamp_excluded_by_halo: false,
        }
    }

    pub(crate) fn paged(stored_starts_at_record: bool, stored_ends_at_record: bool) -> Self {
        Self {
            record_left_clamp: stored_starts_at_record,
            record_right_clamp: stored_ends_at_record,
            page_left_clamp_excluded_by_halo: !stored_starts_at_record,
            page_right_clamp_excluded_by_halo: !stored_ends_at_record,
        }
    }

    pub fn record_left_clamp(self) -> bool {
        self.record_left_clamp
    }

    pub fn record_right_clamp(self) -> bool {
        self.record_right_clamp
    }

    pub fn page_left_clamp_excluded_by_halo(self) -> bool {
        self.page_left_clamp_excluded_by_halo
    }

    pub fn page_right_clamp_excluded_by_halo(self) -> bool {
        self.page_right_clamp_excluded_by_halo
    }

    fn is_complete(self, kind: GrooveTraceRepresentationKind) -> bool {
        match kind {
            GrooveTraceRepresentationKind::Contiguous => {
                self.record_left_clamp
                    && self.record_right_clamp
                    && !self.page_left_clamp_excluded_by_halo
                    && !self.page_right_clamp_excluded_by_halo
            }
            GrooveTraceRepresentationKind::Paged => {
                (self.record_left_clamp || self.page_left_clamp_excluded_by_halo)
                    && (self.record_right_clamp || self.page_right_clamp_excluded_by_halo)
            }
        }
    }
}

/// Contains the recomputed proof data used by the fixed-work tracer.
///
/// Fields remain private so unvalidated serialized data cannot authorize tracing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveTraceAdmissionCertificate {
    certificate_version: u32,
    tracer_algorithm_version: u32,
    admission_class: GrooveTraceAdmissionClass,
    representation_kind: GrooveTraceRepresentationKind,
    representation_format_version: u32,
    spatial_pyramid_format_version: u32,
    source_content_identity: GrooveContentIdentity,
    representation_content_identity: GrooveContentIdentity,
    generation: u64,
    core_start_frame: u64,
    core_end_frame_exclusive: u64,
    stored_start_frame: u64,
    stored_end_frame_exclusive: u64,
    record_end_frame_exclusive: u64,
    maximum_source_frame_advance: f64,
    maximum_tracing_radius_m: f64,
    maximum_wall_curvature_per_m: f64,
    strict_concavity_margin_per_m: f64,
    maximum_absolute_wall_slope: f64,
    maximum_absolute_wall_curvature_per_m: f64,
    maximum_upward_wall_slope_jump: f64,
    maximum_upward_internal_displacement_jump_m: f64,
    maximum_absolute_internal_displacement_jump_m: f64,
    maximum_absolute_internal_wall_slope_jump: f64,
    certified_piece_count: u64,
    maximum_trace_pieces: u32,
    maximum_monotone_branches_per_piece: u8,
    maximum_endpoint_contenders_per_piece: u8,
    maximum_root_brackets_per_piece: u8,
    maximum_root_nodes_per_piece: u32,
    maximum_retained_contenders: u16,
    runtime_height_order_required: bool,
    base_level_coverage: u8,
    spatial_level_coverage: u8,
    c1_joins_certified: bool,
    exact_internal_c0_c1_joins_certified: bool,
    clamped_edges_c1_certified: bool,
    edge_coverage: GrooveTraceEdgeCoverage,
    certificate_identity: GrooveContentIdentity,
}

impl GrooveTraceAdmissionCertificate {
    pub fn certificate_version(self) -> u32 {
        self.certificate_version
    }

    pub fn tracer_algorithm_version(self) -> u32 {
        self.tracer_algorithm_version
    }

    pub fn admission_class(self) -> GrooveTraceAdmissionClass {
        self.admission_class
    }

    pub fn representation_kind(self) -> GrooveTraceRepresentationKind {
        self.representation_kind
    }

    pub fn representation_content_identity(self) -> GrooveContentIdentity {
        self.representation_content_identity
    }

    pub fn source_content_identity(self) -> GrooveContentIdentity {
        self.source_content_identity
    }

    pub fn generation(self) -> u64 {
        self.generation
    }

    pub fn certificate_identity(self) -> GrooveContentIdentity {
        self.certificate_identity
    }

    pub fn maximum_tracing_radius_m(self) -> f64 {
        self.maximum_tracing_radius_m
    }

    pub fn maximum_source_frame_advance(self) -> f64 {
        self.maximum_source_frame_advance
    }

    pub fn maximum_wall_curvature_per_m(self) -> f64 {
        self.maximum_wall_curvature_per_m
    }

    pub fn strict_concavity_margin_per_m(self) -> f64 {
        self.strict_concavity_margin_per_m
    }

    pub(crate) fn maximum_absolute_wall_slope(self) -> f64 {
        self.maximum_absolute_wall_slope
    }

    pub fn maximum_absolute_wall_curvature_per_m(self) -> f64 {
        self.maximum_absolute_wall_curvature_per_m
    }

    pub fn maximum_upward_wall_slope_jump(self) -> f64 {
        self.maximum_upward_wall_slope_jump
    }

    pub fn maximum_upward_internal_displacement_jump_m(self) -> f64 {
        self.maximum_upward_internal_displacement_jump_m
    }

    pub fn maximum_absolute_internal_displacement_jump_m(self) -> f64 {
        self.maximum_absolute_internal_displacement_jump_m
    }

    pub fn maximum_absolute_internal_wall_slope_jump(self) -> f64 {
        self.maximum_absolute_internal_wall_slope_jump
    }

    pub fn certified_piece_count(self) -> u64 {
        self.certified_piece_count
    }

    pub fn maximum_trace_pieces(self) -> u32 {
        self.maximum_trace_pieces
    }

    pub fn maximum_monotone_branches_per_piece(self) -> u8 {
        self.maximum_monotone_branches_per_piece
    }

    pub fn maximum_endpoint_contenders_per_piece(self) -> u8 {
        self.maximum_endpoint_contenders_per_piece
    }

    pub fn runtime_height_order_required(self) -> bool {
        self.runtime_height_order_required
    }

    pub fn c1_joins_certified(self) -> bool {
        self.c1_joins_certified
    }

    pub fn exact_internal_c0_c1_joins_certified(self) -> bool {
        self.exact_internal_c0_c1_joins_certified
    }

    pub fn clamped_edges_c1_certified(self) -> bool {
        self.clamped_edges_c1_certified
    }

    pub fn edge_coverage(self) -> GrooveTraceEdgeCoverage {
        self.edge_coverage
    }

    fn supports_geometry(self, geometry: StylusGeometry) -> Result<(), GrooveTraceAdmissionError> {
        let geometry = geometry
            .validate()
            .map_err(|_| GrooveTraceAdmissionError::InvalidGeometry)?;
        self.validate_static()?;
        if geometry.tracing_radius_m > self.maximum_tracing_radius_m {
            return Err(GrooveTraceAdmissionError::GeometryOutsideCertificate);
        }
        Ok(())
    }

    fn validate_for_active_tracing(
        self,
        geometry: StylusGeometry,
    ) -> Result<(), GrooveTraceAdmissionError> {
        self.supports_geometry(geometry)?;
        if let Some(error) = self.admission_class.rejection_error() {
            Err(error)
        } else {
            Ok(())
        }
    }

    pub(crate) fn validate_static(self) -> Result<(), GrooveTraceAdmissionError> {
        self.source_content_identity
            .validate_current()
            .map_err(|_| GrooveTraceAdmissionError::InvalidCertificate)?;
        self.representation_content_identity
            .validate_current()
            .map_err(|_| GrooveTraceAdmissionError::InvalidCertificate)?;
        self.certificate_identity
            .validate_current()
            .map_err(|_| GrooveTraceAdmissionError::InvalidCertificate)?;
        let finite = [
            self.maximum_source_frame_advance,
            self.maximum_tracing_radius_m,
            self.maximum_wall_curvature_per_m,
            self.strict_concavity_margin_per_m,
            self.maximum_absolute_wall_slope,
            self.maximum_absolute_wall_curvature_per_m,
            self.maximum_upward_wall_slope_jump,
            self.maximum_upward_internal_displacement_jump_m,
            self.maximum_absolute_internal_displacement_jump_m,
            self.maximum_absolute_internal_wall_slope_jump,
        ]
        .iter()
        .all(|value| value.is_finite());
        if self.certificate_version != GROOVE_TRACE_ADMISSION_CERTIFICATE_VERSION
            || self.tracer_algorithm_version != CERTIFIED_CONCAVE_TRACER_ALGORITHM_VERSION
            || self.spatial_pyramid_format_version != GROOVE_SPATIAL_PYRAMID_FORMAT_VERSION
            || self.maximum_source_frame_advance
                != CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_SOURCE_FRAME_ADVANCE
            || self.maximum_tracing_radius_m <= 0.0
            || self.maximum_absolute_wall_slope < 0.0
            || self.maximum_absolute_wall_curvature_per_m < 0.0
            || self.strict_concavity_margin_per_m < 0.0
            || self.maximum_upward_wall_slope_jump < 0.0
            || self.maximum_upward_internal_displacement_jump_m < 0.0
            || self.maximum_absolute_internal_displacement_jump_m < 0.0
            || self.maximum_absolute_internal_wall_slope_jump < 0.0
            || self.certified_piece_count == 0
            || self.base_level_coverage != BASE_LEVEL_COVERAGE_BIT
            || self.spatial_level_coverage != ALL_SPATIAL_LEVEL_COVERAGE_BITS
            || !self.edge_coverage.is_complete(self.representation_kind)
            || self.edge_coverage.record_left_clamp != (self.stored_start_frame == 0)
            || self.edge_coverage.record_right_clamp
                != (self.stored_end_frame_exclusive == self.record_end_frame_exclusive)
            || self.core_start_frame >= self.core_end_frame_exclusive
            || self.stored_start_frame > self.core_start_frame
            || self.stored_end_frame_exclusive < self.core_end_frame_exclusive
            || self.stored_end_frame_exclusive > self.record_end_frame_exclusive
            || self.record_end_frame_exclusive == 0
            || !finite
            || self.certificate_identity != self.calculate_certificate_identity()
        {
            return Err(GrooveTraceAdmissionError::InvalidCertificate);
        }
        match self.representation_kind {
            GrooveTraceRepresentationKind::Contiguous => {
                if self.representation_format_version
                    != CONTIGUOUS_TRACE_REPRESENTATION_FORMAT_VERSION
                    || self.generation != 0
                    || self.core_start_frame != 0
                    || self.stored_start_frame != 0
                    || self.core_end_frame_exclusive != self.stored_end_frame_exclusive
                    || self.record_end_frame_exclusive != self.stored_end_frame_exclusive
                {
                    return Err(GrooveTraceAdmissionError::InvalidCertificate);
                }
            }
            GrooveTraceRepresentationKind::Paged => {
                if self.representation_format_version != PAGED_TRACE_REPRESENTATION_FORMAT_VERSION
                    || self.generation == 0
                {
                    return Err(GrooveTraceAdmissionError::InvalidCertificate);
                }
            }
        }
        let curvature_limit = 1.0 / self.maximum_tracing_radius_m;
        match self.admission_class {
            GrooveTraceAdmissionClass::StrictConcavity => {
                if self.maximum_wall_curvature_per_m >= curvature_limit
                    || self.strict_concavity_margin_per_m <= 0.0
                    || self.strict_concavity_margin_per_m
                        > curvature_limit - self.maximum_wall_curvature_per_m
                    || self.maximum_absolute_wall_slope
                        > CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_WALL_SLOPE
                    || self.maximum_upward_wall_slope_jump > 0.0
                    || self.maximum_upward_internal_displacement_jump_m > 0.0
                    || !self.exact_internal_c0_c1_joins_certified
                    || !self.clamped_edges_c1_certified
                    || self.runtime_height_order_required
                {
                    return Err(GrooveTraceAdmissionError::InvalidCertificate);
                }
            }
            GrooveTraceAdmissionClass::FixedCapPiecewise => {
                if self.strict_concavity_margin_per_m != 0.0
                    || self.maximum_absolute_wall_slope
                        > CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_WALL_SLOPE
                    || self.maximum_absolute_internal_displacement_jump_m
                        > CERTIFIED_TRACE_MAXIMUM_INTERNAL_DISPLACEMENT_JOIN_ENCLOSURE_M
                    || self.maximum_trace_pieces == 0
                    || self.maximum_trace_pieces as usize > SPHERICAL_TRACE_MAX_PIECES
                    || self.maximum_monotone_branches_per_piece as usize
                        != SPHERICAL_TRACE_MAX_MONOTONE_BRANCHES_PER_PIECE
                    || self.maximum_endpoint_contenders_per_piece
                        != CERTIFIED_TRACE_MAXIMUM_ENDPOINT_CONTENDERS_PER_PIECE
                    || self.maximum_root_brackets_per_piece as usize
                        != SPHERICAL_TRACE_MAX_ROOT_BRACKETS_PER_PIECE
                    || self.maximum_root_nodes_per_piece as usize
                        != SPHERICAL_TRACE_MAX_ROOT_NODES_PER_PIECE
                    || self.maximum_retained_contenders as usize
                        != SPHERICAL_TRACE_MAX_GLOBAL_CONTENDERS
                    || !self.runtime_height_order_required
                {
                    return Err(GrooveTraceAdmissionError::InvalidCertificate);
                }
            }
            GrooveTraceAdmissionClass::RejectedWallSlope
            | GrooveTraceAdmissionClass::RejectedFixedWorkSupport
            | GrooveTraceAdmissionClass::RejectedRepresentationJoin => {
                if self.maximum_trace_pieces != 0
                    || self.maximum_monotone_branches_per_piece != 0
                    || self.maximum_endpoint_contenders_per_piece != 0
                    || self.maximum_root_brackets_per_piece != 0
                    || self.maximum_root_nodes_per_piece != 0
                    || self.maximum_retained_contenders != 0
                    || self.runtime_height_order_required
                {
                    return Err(GrooveTraceAdmissionError::InvalidCertificate);
                }
                match self.admission_class {
                    GrooveTraceAdmissionClass::RejectedWallSlope
                        if self.maximum_absolute_wall_slope
                            <= CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_WALL_SLOPE =>
                    {
                        return Err(GrooveTraceAdmissionError::InvalidCertificate);
                    }
                    GrooveTraceAdmissionClass::RejectedRepresentationJoin
                        if self.maximum_absolute_wall_slope
                            > CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_WALL_SLOPE
                            || self.maximum_absolute_internal_displacement_jump_m
                                <= CERTIFIED_TRACE_MAXIMUM_INTERNAL_DISPLACEMENT_JOIN_ENCLOSURE_M =>
                    {
                        return Err(GrooveTraceAdmissionError::InvalidCertificate);
                    }
                    GrooveTraceAdmissionClass::RejectedFixedWorkSupport
                        if self.maximum_absolute_wall_slope
                            > CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_WALL_SLOPE
                            || self.maximum_absolute_internal_displacement_jump_m
                                > CERTIFIED_TRACE_MAXIMUM_INTERNAL_DISPLACEMENT_JOIN_ENCLOSURE_M =>
                    {
                        return Err(GrooveTraceAdmissionError::InvalidCertificate);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn calculate_certificate_identity(self) -> GrooveContentIdentity {
        let mut hash = GrooveContentHasher::new(b"record-player-trace-admission-certificate-v2\0");
        hash.u32(self.certificate_version);
        hash.u32(self.tracer_algorithm_version);
        hash.u8(match self.admission_class {
            GrooveTraceAdmissionClass::StrictConcavity => 0,
            GrooveTraceAdmissionClass::FixedCapPiecewise => 1,
            GrooveTraceAdmissionClass::RejectedWallSlope => 2,
            GrooveTraceAdmissionClass::RejectedFixedWorkSupport => 3,
            GrooveTraceAdmissionClass::RejectedRepresentationJoin => 4,
        });
        hash.u8(match self.representation_kind {
            GrooveTraceRepresentationKind::Contiguous => 0,
            GrooveTraceRepresentationKind::Paged => 1,
        });
        hash.u32(self.representation_format_version);
        hash.u32(self.spatial_pyramid_format_version);
        hash.identity(self.source_content_identity);
        hash.identity(self.representation_content_identity);
        hash.u64(self.generation);
        hash.u64(self.core_start_frame);
        hash.u64(self.core_end_frame_exclusive);
        hash.u64(self.stored_start_frame);
        hash.u64(self.stored_end_frame_exclusive);
        hash.u64(self.record_end_frame_exclusive);
        hash.f64(self.maximum_source_frame_advance);
        hash.f64(self.maximum_tracing_radius_m);
        hash.f64(self.maximum_wall_curvature_per_m);
        hash.f64(self.strict_concavity_margin_per_m);
        hash.f64(self.maximum_absolute_wall_slope);
        hash.f64(self.maximum_absolute_wall_curvature_per_m);
        hash.f64(self.maximum_upward_wall_slope_jump);
        hash.f64(self.maximum_upward_internal_displacement_jump_m);
        hash.f64(self.maximum_absolute_internal_displacement_jump_m);
        hash.f64(self.maximum_absolute_internal_wall_slope_jump);
        hash.u64(self.certified_piece_count);
        hash.u32(self.maximum_trace_pieces);
        hash.u8(self.maximum_monotone_branches_per_piece);
        hash.u8(self.maximum_endpoint_contenders_per_piece);
        hash.u8(self.maximum_root_brackets_per_piece);
        hash.u32(self.maximum_root_nodes_per_piece);
        hash.u32(u32::from(self.maximum_retained_contenders));
        hash.bool(self.runtime_height_order_required);
        hash.u8(self.base_level_coverage);
        hash.u8(self.spatial_level_coverage);
        hash.bool(self.c1_joins_certified);
        hash.bool(self.exact_internal_c0_c1_joins_certified);
        hash.bool(self.clamped_edges_c1_certified);
        hash.bool(self.edge_coverage.record_left_clamp);
        hash.bool(self.edge_coverage.record_right_clamp);
        hash.bool(self.edge_coverage.page_left_clamp_excluded_by_halo);
        hash.bool(self.edge_coverage.page_right_clamp_excluded_by_halo);
        hash.finish()
    }

    pub(crate) fn validate_recomputed(
        self,
        recomputed: Self,
    ) -> Result<ValidatedGrooveTraceAdmissionCertificate, GrooveTraceAdmissionError> {
        self.validate_static()?;
        recomputed.validate_static()?;
        if self != recomputed {
            return Err(GrooveTraceAdmissionError::CertificateMismatch);
        }
        Ok(ValidatedGrooveTraceAdmissionCertificate(self))
    }
}

/// Proves that the crate recomputed a certificate from the bound representation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ValidatedGrooveTraceAdmissionCertificate(GrooveTraceAdmissionCertificate);

impl ValidatedGrooveTraceAdmissionCertificate {
    pub(crate) fn certificate(self) -> GrooveTraceAdmissionCertificate {
        self.0
    }

    pub(crate) fn validate_for_active_tracing(
        self,
        geometry: StylusGeometry,
    ) -> Result<(), GrooveTraceAdmissionError> {
        self.0.validate_for_active_tracing(geometry)
    }

    /// Returns the proof contract for the crate-private certified tracer.
    pub(crate) fn certified_concave_trace_bounds(
        self,
        geometry: StylusGeometry,
    ) -> Result<CertifiedConcaveTraceBounds, GrooveTraceAdmissionError> {
        self.0.supports_geometry(geometry)?;
        if self.0.admission_class != GrooveTraceAdmissionClass::StrictConcavity {
            return Err(GrooveTraceAdmissionError::NotCertifiedConcave);
        }
        CertifiedConcaveTraceBounds {
            representation_kind: self.0.representation_kind,
            algorithm_version: self.0.tracer_algorithm_version,
            maximum_tracing_radius_m: self.0.maximum_tracing_radius_m,
            maximum_wall_curvature_per_m: self.0.maximum_wall_curvature_per_m,
            strict_concavity_margin_per_m: self.0.strict_concavity_margin_per_m,
            maximum_absolute_wall_slope: self.0.maximum_absolute_wall_slope,
            maximum_absolute_wall_curvature_per_m: self.0.maximum_absolute_wall_curvature_per_m,
            maximum_upward_wall_slope_jump: self.0.maximum_upward_wall_slope_jump,
            maximum_upward_internal_displacement_jump_m: self
                .0
                .maximum_upward_internal_displacement_jump_m,
            maximum_source_frame_advance: self.0.maximum_source_frame_advance,
            c1_joins_certified: self.0.c1_joins_certified,
            exact_internal_c0_c1_joins_certified: self.0.exact_internal_c0_c1_joins_certified,
            clamped_edges_c1_certified: self.0.clamped_edges_c1_certified,
            base_level_coverage: self.0.base_level_coverage,
            spatial_level_coverage: self.0.spatial_level_coverage,
            edge_coverage: self.0.edge_coverage,
        }
        .for_geometry(geometry)
    }

    pub(crate) fn fixed_cap_piecewise_trace_bounds(
        self,
        geometry: StylusGeometry,
    ) -> Result<CertifiedPiecewiseTraceBounds, GrooveTraceAdmissionError> {
        self.0.supports_geometry(geometry)?;
        if self.0.admission_class != GrooveTraceAdmissionClass::FixedCapPiecewise
            || !self.0.runtime_height_order_required
        {
            return Err(GrooveTraceAdmissionError::NotCertifiedPiecewise);
        }
        CertifiedPiecewiseTraceBounds {
            algorithm_version: self.0.tracer_algorithm_version,
            maximum_tracing_radius_m: self.0.maximum_tracing_radius_m,
            maximum_trace_pieces: self.0.maximum_trace_pieces,
            maximum_monotone_branches_per_piece: self.0.maximum_monotone_branches_per_piece,
            maximum_endpoint_contenders_per_piece: self.0.maximum_endpoint_contenders_per_piece,
            maximum_root_brackets_per_piece: self.0.maximum_root_brackets_per_piece,
            maximum_root_nodes_per_piece: self.0.maximum_root_nodes_per_piece,
            maximum_retained_contenders: self.0.maximum_retained_contenders,
            height_error_bound_m: SPHERICAL_TRACE_HEIGHT_ERROR_BOUND_M,
            maximum_internal_displacement_join_enclosure_m: self
                .0
                .maximum_absolute_internal_displacement_jump_m,
            contact_position_error_bound_m: SPHERICAL_TRACE_CONTACT_POSITION_ERROR_BOUND_M,
            groove_slope_error_bound: SPHERICAL_TRACE_GROOVE_SLOPE_ERROR_BOUND,
            tangent_residual_error_bound: SPHERICAL_TRACE_TANGENT_RESIDUAL_ERROR_BOUND,
            runtime_height_order_required: self.0.runtime_height_order_required,
        }
        .for_geometry(geometry)
    }
}

/// Borrows one canonical base or spatial representation level.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GrooveTraceAdmissionLevel<'a> {
    pub(crate) first_source_frame: u64,
    pub(crate) source_frame_step: u32,
    pub(crate) lateral_displacement_m: &'a [f32],
    pub(crate) vertical_displacement_m: &'a [f32],
}

/// Binds proof data to one immutable source or page generation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GrooveTraceAdmissionBinding {
    pub(crate) representation_kind: GrooveTraceRepresentationKind,
    pub(crate) representation_format_version: u32,
    pub(crate) source_content_identity: GrooveContentIdentity,
    pub(crate) generation: u64,
    pub(crate) core_start_frame: u64,
    pub(crate) core_end_frame_exclusive: u64,
    pub(crate) stored_start_frame: u64,
    pub(crate) stored_end_frame_exclusive: u64,
    pub(crate) record_end_frame_exclusive: u64,
    pub(crate) minimum_meters_per_source_frame: f64,
    pub(crate) maximum_geometry: StylusGeometry,
    pub(crate) edge_coverage: GrooveTraceEdgeCoverage,
}

/// Recomputes a complete strict-concavity certificate without render-time state.
pub(crate) fn certify_groove_trace_representation(
    binding: GrooveTraceAdmissionBinding,
    base: GrooveTraceAdmissionLevel<'_>,
    spatial_levels: &[GrooveTraceAdmissionLevel<'_>; GROOVE_SPATIAL_PYRAMID_LEVELS],
) -> Result<GrooveTraceAdmissionCertificate, GrooveTraceAdmissionError> {
    let maximum_geometry = validate_certification_inputs(binding, base, spatial_levels)?;
    let representation_content_identity =
        trace_representation_content_identity(binding, base, spatial_levels);
    let mut metrics = TraceAdmissionMetrics::default();
    accumulate_level_metrics(binding, base, &mut metrics)?;
    for level in spatial_levels {
        accumulate_level_metrics(binding, *level, &mut metrics)?;
    }
    finish_groove_trace_admission_certificate(
        binding,
        maximum_geometry,
        representation_content_identity,
        metrics,
    )
}

fn validate_certification_inputs(
    binding: GrooveTraceAdmissionBinding,
    base: GrooveTraceAdmissionLevel<'_>,
    spatial_levels: &[GrooveTraceAdmissionLevel<'_>; GROOVE_SPATIAL_PYRAMID_LEVELS],
) -> Result<StylusGeometry, GrooveTraceAdmissionError> {
    let maximum_geometry = validate_certification_binding(binding)?;
    validate_level(base)?;
    for level in spatial_levels {
        validate_level(*level)?;
    }
    validate_level_layout(binding, base, spatial_levels)?;
    validate_certified_trace_halo(binding, maximum_geometry)?;
    Ok(maximum_geometry)
}

fn validate_certification_binding(
    binding: GrooveTraceAdmissionBinding,
) -> Result<StylusGeometry, GrooveTraceAdmissionError> {
    binding
        .source_content_identity
        .validate_current()
        .map_err(|_| GrooveTraceAdmissionError::InvalidBinding)?;
    let maximum_geometry = binding
        .maximum_geometry
        .validate()
        .map_err(|_| GrooveTraceAdmissionError::InvalidGeometry)?;
    if !binding.minimum_meters_per_source_frame.is_finite()
        || binding.minimum_meters_per_source_frame <= 0.0
        || binding.core_start_frame >= binding.core_end_frame_exclusive
        || binding.stored_start_frame > binding.core_start_frame
        || binding.stored_end_frame_exclusive < binding.core_end_frame_exclusive
        || binding.stored_end_frame_exclusive > binding.record_end_frame_exclusive
        || binding.record_end_frame_exclusive == 0
        || !binding
            .edge_coverage
            .is_complete(binding.representation_kind)
        || binding.edge_coverage.record_left_clamp != (binding.stored_start_frame == 0)
        || binding.edge_coverage.record_right_clamp
            != (binding.stored_end_frame_exclusive == binding.record_end_frame_exclusive)
        || (binding.representation_kind == GrooveTraceRepresentationKind::Contiguous
            && binding.generation != 0)
        || (binding.representation_kind == GrooveTraceRepresentationKind::Paged
            && binding.generation == 0)
    {
        return Err(GrooveTraceAdmissionError::InvalidBinding);
    }
    Ok(maximum_geometry)
}

fn validate_level_layout(
    binding: GrooveTraceAdmissionBinding,
    base: GrooveTraceAdmissionLevel<'_>,
    spatial_levels: &[GrooveTraceAdmissionLevel<'_>; GROOVE_SPATIAL_PYRAMID_LEVELS],
) -> Result<(), GrooveTraceAdmissionError> {
    let expected_base_len = usize::try_from(
        binding
            .stored_end_frame_exclusive
            .checked_sub(binding.stored_start_frame)
            .ok_or(GrooveTraceAdmissionError::InvalidBinding)?,
    )
    .map_err(|_| GrooveTraceAdmissionError::InvalidRepresentation)?;
    if expected_base_len < 4
        || base.first_source_frame != binding.stored_start_frame
        || base.source_frame_step != 1
        || base.lateral_displacement_m.len() != expected_base_len
    {
        return Err(GrooveTraceAdmissionError::InvalidRepresentation);
    }

    let mut current_first = base.first_source_frame;
    let mut current_step = base.source_frame_step;
    let mut current_len = base.lateral_displacement_m.len();
    for level in spatial_levels {
        let expected_step = current_step
            .checked_mul(2)
            .ok_or(GrooveTraceAdmissionError::InvalidRepresentation)?;
        let expected_first = align_up(current_first, u64::from(expected_step))
            .ok_or(GrooveTraceAdmissionError::InvalidRepresentation)?;
        let expected_len = if current_len == 0 {
            0
        } else {
            let span = u64::try_from(current_len.saturating_sub(1))
                .map_err(|_| GrooveTraceAdmissionError::InvalidRepresentation)?
                .checked_mul(u64::from(current_step))
                .ok_or(GrooveTraceAdmissionError::InvalidRepresentation)?;
            let current_last = current_first
                .checked_add(span)
                .ok_or(GrooveTraceAdmissionError::InvalidRepresentation)?;
            if expected_first > current_last {
                0
            } else {
                usize::try_from((current_last - expected_first) / u64::from(expected_step) + 1)
                    .map_err(|_| GrooveTraceAdmissionError::InvalidRepresentation)?
            }
        };
        if level.first_source_frame != expected_first
            || level.source_frame_step != expected_step
            || level.lateral_displacement_m.len() != expected_len
        {
            return Err(GrooveTraceAdmissionError::InvalidRepresentation);
        }
        current_first = expected_first;
        current_step = expected_step;
        current_len = expected_len;
    }
    Ok(())
}

fn validate_certified_trace_halo(
    binding: GrooveTraceAdmissionBinding,
    maximum_geometry: StylusGeometry,
) -> Result<(), GrooveTraceAdmissionError> {
    if binding.representation_kind == GrooveTraceRepresentationKind::Contiguous {
        return Ok(());
    }
    let trace_halo = maximum_geometry
        .multiresolution_support(binding.minimum_meters_per_source_frame, 16)
        .map_err(|_| GrooveTraceAdmissionError::FixedWorkNotProved)?
        .symmetric_halo_source_frames();
    let required_storage_halo = u64::from(
        trace_halo
            .checked_add(GROOVE_SPATIAL_FILTER_RADIUS_FRAMES)
            .ok_or(GrooveTraceAdmissionError::InvalidBinding)?,
    );
    let left_halo = binding
        .core_start_frame
        .saturating_sub(binding.stored_start_frame);
    let right_halo = binding
        .stored_end_frame_exclusive
        .saturating_sub(binding.core_end_frame_exclusive);
    if (!binding.edge_coverage.record_left_clamp && left_halo < required_storage_halo)
        || (!binding.edge_coverage.record_right_clamp && right_halo < required_storage_halo)
    {
        return Err(GrooveTraceAdmissionError::InvalidBinding);
    }
    Ok(())
}

fn finish_groove_trace_admission_certificate(
    binding: GrooveTraceAdmissionBinding,
    maximum_geometry: StylusGeometry,
    representation_content_identity: GrooveContentIdentity,
    metrics: TraceAdmissionMetrics,
) -> Result<GrooveTraceAdmissionCertificate, GrooveTraceAdmissionError> {
    if metrics.certified_piece_count == 0 {
        return Err(GrooveTraceAdmissionError::InsufficientPieces);
    }
    let curvature_limit = next_down(1.0 / maximum_geometry.tracing_radius_m);
    let admitted_curvature_limit =
        next_down(curvature_limit * CERTIFIED_TRACE_MAXIMUM_CONCAVITY_RATIO);
    let strict_concavity_proved = metrics.maximum_wall_curvature_per_m <= admitted_curvature_limit
        && metrics.maximum_absolute_wall_slope <= CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_WALL_SLOPE
        && metrics.maximum_upward_wall_slope_jump <= 0.0
        && metrics.maximum_upward_internal_displacement_jump_m <= 0.0
        && metrics.clamped_edges_c1_certified
        && metrics.exact_internal_c0_c1_joins_certified;
    let strict_concavity_margin_per_m = if strict_concavity_proved {
        next_down(curvature_limit - metrics.maximum_wall_curvature_per_m)
    } else {
        0.0
    };
    let maximum_trace_pieces = maximum_geometry
        .multiresolution_support(binding.minimum_meters_per_source_frame, 16)
        .ok()
        .and_then(|support| {
            support
                .search_half_span_source_frames()
                .checked_mul(2)
                .and_then(|value| value.checked_add(2))
        })
        .filter(|pieces| *pieces as usize <= SPHERICAL_TRACE_MAX_PIECES);
    let admission_class = if strict_concavity_proved
        && strict_concavity_margin_per_m.is_finite()
        && strict_concavity_margin_per_m > 0.0
    {
        GrooveTraceAdmissionClass::StrictConcavity
    } else if metrics.maximum_absolute_wall_slope > CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_WALL_SLOPE {
        GrooveTraceAdmissionClass::RejectedWallSlope
    } else if metrics.maximum_absolute_internal_displacement_jump_m
        > CERTIFIED_TRACE_MAXIMUM_INTERNAL_DISPLACEMENT_JOIN_ENCLOSURE_M
    {
        GrooveTraceAdmissionClass::RejectedRepresentationJoin
    } else if maximum_trace_pieces.is_none() {
        GrooveTraceAdmissionClass::RejectedFixedWorkSupport
    } else {
        GrooveTraceAdmissionClass::FixedCapPiecewise
    };
    let fixed_cap_piecewise = admission_class == GrooveTraceAdmissionClass::FixedCapPiecewise;

    let mut certificate = GrooveTraceAdmissionCertificate {
        certificate_version: GROOVE_TRACE_ADMISSION_CERTIFICATE_VERSION,
        tracer_algorithm_version: CERTIFIED_CONCAVE_TRACER_ALGORITHM_VERSION,
        admission_class,
        representation_kind: binding.representation_kind,
        representation_format_version: binding.representation_format_version,
        spatial_pyramid_format_version: GROOVE_SPATIAL_PYRAMID_FORMAT_VERSION,
        source_content_identity: binding.source_content_identity,
        representation_content_identity,
        generation: binding.generation,
        core_start_frame: binding.core_start_frame,
        core_end_frame_exclusive: binding.core_end_frame_exclusive,
        stored_start_frame: binding.stored_start_frame,
        stored_end_frame_exclusive: binding.stored_end_frame_exclusive,
        record_end_frame_exclusive: binding.record_end_frame_exclusive,
        maximum_source_frame_advance: CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_SOURCE_FRAME_ADVANCE,
        maximum_tracing_radius_m: maximum_geometry.tracing_radius_m,
        maximum_wall_curvature_per_m: metrics.maximum_wall_curvature_per_m,
        strict_concavity_margin_per_m,
        maximum_absolute_wall_slope: metrics.maximum_absolute_wall_slope,
        maximum_absolute_wall_curvature_per_m: metrics.maximum_absolute_wall_curvature_per_m,
        maximum_upward_wall_slope_jump: metrics.maximum_upward_wall_slope_jump,
        maximum_upward_internal_displacement_jump_m: metrics
            .maximum_upward_internal_displacement_jump_m,
        maximum_absolute_internal_displacement_jump_m: metrics
            .maximum_absolute_internal_displacement_jump_m,
        maximum_absolute_internal_wall_slope_jump: metrics
            .maximum_absolute_internal_wall_slope_jump,
        certified_piece_count: metrics.certified_piece_count,
        maximum_trace_pieces: if fixed_cap_piecewise {
            maximum_trace_pieces.expect("fixed-cap admission has a piece bound")
        } else if admission_class == GrooveTraceAdmissionClass::StrictConcavity {
            1
        } else {
            0
        },
        maximum_monotone_branches_per_piece: if fixed_cap_piecewise {
            SPHERICAL_TRACE_MAX_MONOTONE_BRANCHES_PER_PIECE as u8
        } else {
            0
        },
        maximum_endpoint_contenders_per_piece: if fixed_cap_piecewise {
            CERTIFIED_TRACE_MAXIMUM_ENDPOINT_CONTENDERS_PER_PIECE
        } else {
            0
        },
        maximum_root_brackets_per_piece: if fixed_cap_piecewise {
            SPHERICAL_TRACE_MAX_ROOT_BRACKETS_PER_PIECE as u8
        } else if admission_class == GrooveTraceAdmissionClass::StrictConcavity {
            1
        } else {
            0
        },
        maximum_root_nodes_per_piece: if fixed_cap_piecewise {
            SPHERICAL_TRACE_MAX_ROOT_NODES_PER_PIECE as u32
        } else if admission_class == GrooveTraceAdmissionClass::StrictConcavity {
            1
        } else {
            0
        },
        maximum_retained_contenders: if fixed_cap_piecewise {
            SPHERICAL_TRACE_MAX_GLOBAL_CONTENDERS as u16
        } else if admission_class == GrooveTraceAdmissionClass::StrictConcavity {
            1
        } else {
            0
        },
        runtime_height_order_required: fixed_cap_piecewise,
        base_level_coverage: BASE_LEVEL_COVERAGE_BIT,
        spatial_level_coverage: ALL_SPATIAL_LEVEL_COVERAGE_BITS,
        c1_joins_certified: metrics.c1_joins_certified,
        exact_internal_c0_c1_joins_certified: metrics.exact_internal_c0_c1_joins_certified,
        clamped_edges_c1_certified: metrics.clamped_edges_c1_certified,
        edge_coverage: binding.edge_coverage,
        certificate_identity: GrooveContentIdentity::from_sha256([0; 32]),
    };
    certificate.certificate_identity = certificate.calculate_certificate_identity();
    certificate.validate_static()?;
    Ok(certificate)
}

fn trace_representation_content_identity(
    binding: GrooveTraceAdmissionBinding,
    base: GrooveTraceAdmissionLevel<'_>,
    spatial_levels: &[GrooveTraceAdmissionLevel<'_>; GROOVE_SPATIAL_PYRAMID_LEVELS],
) -> GrooveContentIdentity {
    let mut hash = new_trace_representation_hasher(binding);
    append_level_identity(&mut hash, base);
    hash.u64(spatial_levels.len() as u64);
    for level in spatial_levels {
        append_level_identity(&mut hash, *level);
    }
    hash.finish()
}

fn new_trace_representation_hasher(binding: GrooveTraceAdmissionBinding) -> GrooveContentHasher {
    let mut hash = GrooveContentHasher::new(b"record-player-trace-representation-v1\0");
    hash.u8(match binding.representation_kind {
        GrooveTraceRepresentationKind::Contiguous => 0,
        GrooveTraceRepresentationKind::Paged => 1,
    });
    hash.u32(binding.representation_format_version);
    hash.u32(GROOVE_SPATIAL_PYRAMID_FORMAT_VERSION);
    hash.identity(binding.source_content_identity);
    hash.u64(binding.generation);
    hash.u64(binding.core_start_frame);
    hash.u64(binding.core_end_frame_exclusive);
    hash.u64(binding.stored_start_frame);
    hash.u64(binding.stored_end_frame_exclusive);
    hash.u64(binding.record_end_frame_exclusive);
    hash
}

fn append_level_identity(hash: &mut GrooveContentHasher, level: GrooveTraceAdmissionLevel<'_>) {
    hash.u64(level.first_source_frame);
    hash.u32(level.source_frame_step);
    hash.u64(level.lateral_displacement_m.len() as u64);
    for sample in level.lateral_displacement_m {
        hash.f32(*sample);
    }
    hash.u64(level.vertical_displacement_m.len() as u64);
    for sample in level.vertical_displacement_m {
        hash.f32(*sample);
    }
}

fn validate_level(level: GrooveTraceAdmissionLevel<'_>) -> Result<(), GrooveTraceAdmissionError> {
    validate_level_shape(level)?;
    if level
        .lateral_displacement_m
        .iter()
        .chain(level.vertical_displacement_m)
        .any(|sample| !sample.is_finite())
    {
        return Err(GrooveTraceAdmissionError::InvalidRepresentation);
    }
    Ok(())
}

fn validate_level_shape(
    level: GrooveTraceAdmissionLevel<'_>,
) -> Result<(), GrooveTraceAdmissionError> {
    if level.source_frame_step == 0
        || level.lateral_displacement_m.len() != level.vertical_displacement_m.len()
    {
        return Err(GrooveTraceAdmissionError::InvalidRepresentation);
    }
    Ok(())
}

const TRACE_ADMISSION_FINISH_WORK_UNITS: u32 = 1;
const TRACE_ADMISSION_LEVEL_COUNT: usize = GROOVE_SPATIAL_PYRAMID_LEVELS + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncrementalTraceAdmissionPhase {
    HashLateral {
        level_index: usize,
        sample_index: usize,
    },
    HashVertical {
        level_index: usize,
        sample_index: usize,
    },
    Metrics {
        level_index: usize,
        wall_index: usize,
        absolute_segment: u64,
    },
    Edges {
        level_index: usize,
        edge_index: usize,
    },
    Finish,
    Complete,
}

/// Holds fixed-size trace-certificate work between bounded ingestion calls.
#[derive(Debug, Clone)]
pub(crate) struct GrooveTraceAdmissionIncrementalState {
    binding: GrooveTraceAdmissionBinding,
    maximum_geometry: StylusGeometry,
    physical_step: f64,
    representation_hash: Option<GrooveContentHasher>,
    representation_content_identity: Option<GrooveContentIdentity>,
    metrics: TraceAdmissionMetrics,
    previous_runtime: Option<RuntimeCubic>,
    phase: IncrementalTraceAdmissionPhase,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GrooveTraceAdmissionIncrementalProgress {
    pub(crate) work_units_consumed: u32,
    pub(crate) certificate: Option<GrooveTraceAdmissionCertificate>,
}

impl GrooveTraceAdmissionIncrementalState {
    pub(crate) fn new(
        binding: GrooveTraceAdmissionBinding,
        base: GrooveTraceAdmissionLevel<'_>,
        spatial_levels: &[GrooveTraceAdmissionLevel<'_>; GROOVE_SPATIAL_PYRAMID_LEVELS],
    ) -> Result<Self, GrooveTraceAdmissionError> {
        let maximum_geometry = validate_certification_binding(binding)?;
        validate_level_shape(base)?;
        for level in spatial_levels {
            validate_level_shape(*level)?;
        }
        validate_level_layout(binding, base, spatial_levels)?;
        validate_certified_trace_halo(binding, maximum_geometry)?;
        let physical_step = next_down(binding.minimum_meters_per_source_frame);
        if binding.stored_end_frame_exclusive > (1_u64 << 53)
            || !physical_step.is_finite()
            || physical_step <= 0.0
        {
            return Err(GrooveTraceAdmissionError::InvalidBinding);
        }
        let mut representation_hash = new_trace_representation_hasher(binding);
        append_incremental_level_header(&mut representation_hash, base);
        Ok(Self {
            binding,
            maximum_geometry,
            physical_step,
            representation_hash: Some(representation_hash),
            representation_content_identity: None,
            metrics: TraceAdmissionMetrics::default(),
            previous_runtime: None,
            phase: IncrementalTraceAdmissionPhase::HashLateral {
                level_index: 0,
                sample_index: 0,
            },
        })
    }

    pub(crate) fn advance(
        &mut self,
        base: GrooveTraceAdmissionLevel<'_>,
        spatial_levels: &[GrooveTraceAdmissionLevel<'_>; GROOVE_SPATIAL_PYRAMID_LEVELS],
        maximum_work_units: u32,
    ) -> Result<GrooveTraceAdmissionIncrementalProgress, GrooveTraceAdmissionError> {
        if self.phase == IncrementalTraceAdmissionPhase::Complete {
            return Ok(GrooveTraceAdmissionIncrementalProgress {
                work_units_consumed: 0,
                certificate: None,
            });
        }
        let mut used = 0_u32;
        while used < maximum_work_units {
            match self.phase {
                IncrementalTraceAdmissionPhase::HashLateral {
                    level_index,
                    sample_index,
                } => {
                    let level = trace_admission_level(base, spatial_levels, level_index);
                    if sample_index >= level.lateral_displacement_m.len() {
                        self.representation_hash
                            .as_mut()
                            .expect("incremental trace hashing has a state")
                            .u64(level.vertical_displacement_m.len() as u64);
                        self.phase = IncrementalTraceAdmissionPhase::HashVertical {
                            level_index,
                            sample_index: 0,
                        };
                        continue;
                    }
                    let sample = level.lateral_displacement_m[sample_index];
                    if !sample.is_finite() {
                        return Err(GrooveTraceAdmissionError::InvalidRepresentation);
                    }
                    self.representation_hash
                        .as_mut()
                        .expect("incremental trace hashing has a state")
                        .f32(sample);
                    self.phase = IncrementalTraceAdmissionPhase::HashLateral {
                        level_index,
                        sample_index: sample_index + 1,
                    };
                    used += 1;
                }
                IncrementalTraceAdmissionPhase::HashVertical {
                    level_index,
                    sample_index,
                } => {
                    let level = trace_admission_level(base, spatial_levels, level_index);
                    if sample_index >= level.vertical_displacement_m.len() {
                        let next_level = level_index + 1;
                        if next_level < TRACE_ADMISSION_LEVEL_COUNT {
                            if level_index == 0 {
                                self.representation_hash
                                    .as_mut()
                                    .expect("incremental trace hashing has a state")
                                    .u64(GROOVE_SPATIAL_PYRAMID_LEVELS as u64);
                            }
                            let next = trace_admission_level(base, spatial_levels, next_level);
                            append_incremental_level_header(
                                self.representation_hash
                                    .as_mut()
                                    .expect("incremental trace hashing has a state"),
                                next,
                            );
                            self.phase = IncrementalTraceAdmissionPhase::HashLateral {
                                level_index: next_level,
                                sample_index: 0,
                            };
                        } else {
                            self.representation_content_identity = Some(
                                self.representation_hash
                                    .take()
                                    .expect("incremental trace hashing has a state")
                                    .finish(),
                            );
                            self.phase = IncrementalTraceAdmissionPhase::Metrics {
                                level_index: 0,
                                wall_index: 0,
                                absolute_segment: self.binding.stored_start_frame,
                            };
                        }
                        continue;
                    }
                    let sample = level.vertical_displacement_m[sample_index];
                    if !sample.is_finite() {
                        return Err(GrooveTraceAdmissionError::InvalidRepresentation);
                    }
                    self.representation_hash
                        .as_mut()
                        .expect("incremental trace hashing has a state")
                        .f32(sample);
                    self.phase = IncrementalTraceAdmissionPhase::HashVertical {
                        level_index,
                        sample_index: sample_index + 1,
                    };
                    used += 1;
                }
                IncrementalTraceAdmissionPhase::Metrics {
                    level_index,
                    wall_index,
                    absolute_segment,
                } => {
                    let level = trace_admission_level(base, spatial_levels, level_index);
                    let segment_end = self.binding.stored_end_frame_exclusive.saturating_sub(1);
                    if level.lateral_displacement_m.len() < 4 || absolute_segment >= segment_end {
                        if level.lateral_displacement_m.len() >= 4
                            && wall_index + 1 < WALL_SCALES.len()
                        {
                            self.previous_runtime = None;
                            self.phase = IncrementalTraceAdmissionPhase::Metrics {
                                level_index,
                                wall_index: wall_index + 1,
                                absolute_segment: self.binding.stored_start_frame,
                            };
                        } else {
                            self.previous_runtime = None;
                            self.phase = IncrementalTraceAdmissionPhase::Edges {
                                level_index,
                                edge_index: 0,
                            };
                        }
                        continue;
                    }
                    let runtime = accumulate_runtime_segment_metrics(
                        level,
                        wall_index,
                        absolute_segment,
                        self.physical_step,
                        self.previous_runtime,
                        &mut self.metrics,
                    )?;
                    self.previous_runtime = Some(runtime);
                    self.phase = IncrementalTraceAdmissionPhase::Metrics {
                        level_index,
                        wall_index,
                        absolute_segment: absolute_segment + 1,
                    };
                    used += 1;
                }
                IncrementalTraceAdmissionPhase::Edges {
                    level_index,
                    edge_index,
                } => {
                    let level = trace_admission_level(base, spatial_levels, level_index);
                    if level.lateral_displacement_m.len() < 4 || edge_index >= 4 {
                        let next_level = level_index + 1;
                        if next_level < TRACE_ADMISSION_LEVEL_COUNT {
                            self.phase = IncrementalTraceAdmissionPhase::Metrics {
                                level_index: next_level,
                                wall_index: 0,
                                absolute_segment: self.binding.stored_start_frame,
                            };
                        } else {
                            self.phase = IncrementalTraceAdmissionPhase::Finish;
                        }
                        continue;
                    }
                    let right_edge = edge_index >= WALL_SCALES.len();
                    let wall_index = edge_index % WALL_SCALES.len();
                    let covered = if right_edge {
                        self.binding.edge_coverage.record_right_clamp
                    } else {
                        self.binding.edge_coverage.record_left_clamp
                    };
                    self.phase = IncrementalTraceAdmissionPhase::Edges {
                        level_index,
                        edge_index: edge_index + 1,
                    };
                    if !covered {
                        continue;
                    }
                    accumulate_record_edge_metrics(
                        self.binding,
                        level,
                        wall_index,
                        right_edge,
                        self.physical_step,
                        &mut self.metrics,
                    );
                    used += 1;
                }
                IncrementalTraceAdmissionPhase::Finish => {
                    let certificate = finish_groove_trace_admission_certificate(
                        self.binding,
                        self.maximum_geometry,
                        self.representation_content_identity
                            .ok_or(GrooveTraceAdmissionError::InvalidRepresentation)?,
                        self.metrics,
                    )?;
                    self.phase = IncrementalTraceAdmissionPhase::Complete;
                    used = used.saturating_add(TRACE_ADMISSION_FINISH_WORK_UNITS);
                    return Ok(GrooveTraceAdmissionIncrementalProgress {
                        work_units_consumed: used,
                        certificate: Some(certificate),
                    });
                }
                IncrementalTraceAdmissionPhase::Complete => break,
            }
        }
        Ok(GrooveTraceAdmissionIncrementalProgress {
            work_units_consumed: used,
            certificate: None,
        })
    }
}

#[cfg(test)]
pub(crate) fn groove_trace_admission_work_units(
    binding: GrooveTraceAdmissionBinding,
    base: GrooveTraceAdmissionLevel<'_>,
    spatial_levels: &[GrooveTraceAdmissionLevel<'_>; GROOVE_SPATIAL_PYRAMID_LEVELS],
) -> Result<u64, GrooveTraceAdmissionError> {
    validate_certification_inputs(binding, base, spatial_levels)?;
    let segment_count = binding
        .stored_end_frame_exclusive
        .saturating_sub(binding.stored_start_frame)
        .saturating_sub(1);
    let edge_count = u64::from(binding.edge_coverage.record_left_clamp)
        .saturating_add(u64::from(binding.edge_coverage.record_right_clamp))
        .saturating_mul(WALL_SCALES.len() as u64);
    let mut work = u64::from(TRACE_ADMISSION_FINISH_WORK_UNITS);
    for level_index in 0..TRACE_ADMISSION_LEVEL_COUNT {
        let level = trace_admission_level(base, spatial_levels, level_index);
        work = work
            .checked_add((level.lateral_displacement_m.len() as u64).saturating_mul(2))
            .ok_or(GrooveTraceAdmissionError::PieceCountOverflow)?;
        if level.lateral_displacement_m.len() >= 4 {
            work = work
                .checked_add(segment_count.saturating_mul(WALL_SCALES.len() as u64))
                .and_then(|value| value.checked_add(edge_count))
                .ok_or(GrooveTraceAdmissionError::PieceCountOverflow)?;
        }
    }
    Ok(work)
}

fn trace_admission_level<'a>(
    base: GrooveTraceAdmissionLevel<'a>,
    spatial_levels: &[GrooveTraceAdmissionLevel<'a>; GROOVE_SPATIAL_PYRAMID_LEVELS],
    level_index: usize,
) -> GrooveTraceAdmissionLevel<'a> {
    if level_index == 0 {
        base
    } else {
        spatial_levels[level_index - 1]
    }
}

fn append_incremental_level_header(
    hash: &mut GrooveContentHasher,
    level: GrooveTraceAdmissionLevel<'_>,
) {
    hash.u64(level.first_source_frame);
    hash.u32(level.source_frame_step);
    hash.u64(level.lateral_displacement_m.len() as u64);
}

#[derive(Debug, Clone, Copy)]
struct TraceAdmissionMetrics {
    maximum_wall_curvature_per_m: f64,
    maximum_absolute_wall_slope: f64,
    maximum_absolute_wall_curvature_per_m: f64,
    maximum_upward_wall_slope_jump: f64,
    maximum_upward_internal_displacement_jump_m: f64,
    maximum_absolute_internal_displacement_jump_m: f64,
    maximum_absolute_internal_wall_slope_jump: f64,
    certified_piece_count: u64,
    clamped_edges_c1_certified: bool,
    c1_joins_certified: bool,
    exact_internal_c0_c1_joins_certified: bool,
}

impl Default for TraceAdmissionMetrics {
    fn default() -> Self {
        Self {
            maximum_wall_curvature_per_m: 0.0,
            maximum_absolute_wall_slope: 0.0,
            maximum_absolute_wall_curvature_per_m: 0.0,
            maximum_upward_wall_slope_jump: 0.0,
            maximum_upward_internal_displacement_jump_m: 0.0,
            maximum_absolute_internal_displacement_jump_m: 0.0,
            maximum_absolute_internal_wall_slope_jump: 0.0,
            certified_piece_count: 0,
            clamped_edges_c1_certified: true,
            c1_joins_certified: true,
            exact_internal_c0_c1_joins_certified: true,
        }
    }
}

fn accumulate_level_metrics(
    binding: GrooveTraceAdmissionBinding,
    level: GrooveTraceAdmissionLevel<'_>,
    metrics: &mut TraceAdmissionMetrics,
) -> Result<(), GrooveTraceAdmissionError> {
    let len = level.lateral_displacement_m.len();
    if len < 4 {
        return Ok(());
    }
    if binding.stored_end_frame_exclusive > (1_u64 << 53) {
        return Err(GrooveTraceAdmissionError::InvalidBinding);
    }
    let physical_step = next_down(binding.minimum_meters_per_source_frame);
    if !physical_step.is_finite() || physical_step <= 0.0 {
        return Err(GrooveTraceAdmissionError::InvalidBinding);
    }
    for wall_index in 0..WALL_SCALES.len() {
        let mut previous: Option<RuntimeCubic> = None;
        for absolute_segment in
            binding.stored_start_frame..binding.stored_end_frame_exclusive.saturating_sub(1)
        {
            let runtime = accumulate_runtime_segment_metrics(
                level,
                wall_index,
                absolute_segment,
                physical_step,
                previous,
                metrics,
            )?;
            previous = Some(runtime);
        }
    }

    if binding.edge_coverage.record_left_clamp {
        for wall_index in 0..WALL_SCALES.len() {
            accumulate_record_edge_metrics(
                binding,
                level,
                wall_index,
                false,
                physical_step,
                metrics,
            );
        }
    }
    if binding.edge_coverage.record_right_clamp {
        for wall_index in 0..WALL_SCALES.len() {
            accumulate_record_edge_metrics(
                binding,
                level,
                wall_index,
                true,
                physical_step,
                metrics,
            );
        }
    }
    Ok(())
}

fn accumulate_runtime_segment_metrics(
    level: GrooveTraceAdmissionLevel<'_>,
    wall_index: usize,
    absolute_segment: u64,
    physical_step: f64,
    previous: Option<RuntimeCubic>,
    metrics: &mut TraceAdmissionMetrics,
) -> Result<RuntimeCubic, GrooveTraceAdmissionError> {
    let runtime = runtime_wall_cubic_for_source_segment(level, wall_index, absolute_segment);
    let cubic = runtime.bounds();
    let slope = cubic.slope_bounds();
    let curvature = cubic.curvature_bounds();
    let physical_slope = slope.divide_positive(Interval::point(physical_step));
    let physical_step_squared = Interval::point(physical_step).square();
    let physical_curvature = curvature.divide_positive(physical_step_squared);
    metrics.maximum_wall_curvature_per_m = metrics
        .maximum_wall_curvature_per_m
        .max(physical_curvature.upper);
    metrics.maximum_absolute_wall_slope = metrics
        .maximum_absolute_wall_slope
        .max(physical_slope.maximum_absolute());
    metrics.maximum_absolute_wall_curvature_per_m = metrics
        .maximum_absolute_wall_curvature_per_m
        .max(physical_curvature.maximum_absolute());
    metrics.certified_piece_count = metrics
        .certified_piece_count
        .checked_add(1)
        .ok_or(GrooveTraceAdmissionError::PieceCountOverflow)?;
    if let Some(left) = previous {
        let displacement_jump = runtime.value(0.0) - left.value(1.0);
        let slope_jump = (runtime.slope_per_frame(0.0) - left.slope_per_frame(1.0)) / physical_step;
        metrics.maximum_upward_wall_slope_jump = metrics
            .maximum_upward_wall_slope_jump
            .max(slope_jump.max(0.0));
        let len = level.lateral_displacement_m.len();
        let last_level_frame = level.first_source_frame.saturating_add(
            u64::try_from(len.saturating_sub(1))
                .unwrap_or(u64::MAX)
                .saturating_mul(u64::from(level.source_frame_step)),
        );
        if absolute_segment > level.first_source_frame && absolute_segment < last_level_frame {
            metrics.maximum_upward_internal_displacement_jump_m = metrics
                .maximum_upward_internal_displacement_jump_m
                .max(displacement_jump.max(0.0));
            metrics.maximum_absolute_internal_displacement_jump_m = metrics
                .maximum_absolute_internal_displacement_jump_m
                .max(displacement_jump.abs());
            metrics.maximum_absolute_internal_wall_slope_jump = metrics
                .maximum_absolute_internal_wall_slope_jump
                .max(slope_jump.abs());
            metrics.c1_joins_certified &= displacement_jump.abs()
                <= SPHERICAL_TRACE_HEIGHT_ERROR_BOUND_M
                && slope_jump.abs() <= SPHERICAL_TRACE_GROOVE_SLOPE_ERROR_BOUND;
            metrics.exact_internal_c0_c1_joins_certified &=
                displacement_jump == 0.0 && slope_jump == 0.0;
        }
    }
    Ok(runtime)
}

fn accumulate_record_edge_metrics(
    binding: GrooveTraceAdmissionBinding,
    level: GrooveTraceAdmissionLevel<'_>,
    wall_index: usize,
    right_edge: bool,
    physical_step: f64,
    metrics: &mut TraceAdmissionMetrics,
) {
    let (absolute_segment, fraction, outward_sign) = if right_edge {
        (
            binding.stored_end_frame_exclusive.saturating_sub(2),
            1.0,
            -1.0,
        )
    } else {
        (binding.stored_start_frame, 0.0, 1.0)
    };
    let slope = runtime_wall_cubic_for_source_segment(level, wall_index, absolute_segment)
        .slope_per_frame(fraction)
        / physical_step;
    metrics.clamped_edges_c1_certified &= slope == 0.0;
    metrics.maximum_upward_wall_slope_jump = metrics
        .maximum_upward_wall_slope_jump
        .max((outward_sign * slope).max(0.0));
}

fn wall_sample(level: GrooveTraceAdmissionLevel<'_>, wall_index: usize, index: usize) -> f64 {
    let (lateral_scale, vertical_scale) = WALL_SCALES[wall_index];
    f64::from(level.lateral_displacement_m[index]) * lateral_scale
        + f64::from(level.vertical_displacement_m[index]) * vertical_scale
}

fn runtime_wall_cubic_for_source_segment(
    level: GrooveTraceAdmissionLevel<'_>,
    wall_index: usize,
    absolute_segment: u64,
) -> RuntimeCubic {
    let level_position = (absolute_segment as f64 - level.first_source_frame as f64)
        / f64::from(level.source_frame_step);
    let level_segment = level_position.floor();
    let level_fraction = level_position - level_segment;
    let maximum = level.lateral_displacement_m.len() - 1;
    let coarse = if level_segment < 0.0 {
        RuntimeCubic::constant(wall_sample(level, wall_index, 0))
    } else if level_segment >= maximum as f64 {
        RuntimeCubic::constant(wall_sample(level, wall_index, maximum))
    } else {
        let segment_index = level_segment as isize;
        let index = |offset: isize| (segment_index + offset).clamp(0, maximum as isize) as usize;
        RuntimeCubic::from_catmull_rom_points(
            wall_sample(level, wall_index, index(-1)),
            wall_sample(level, wall_index, index(0)),
            wall_sample(level, wall_index, index(1)),
            wall_sample(level, wall_index, index(2)),
        )
    };
    coarse.compose_affine(level_fraction, 1.0 / f64::from(level.source_frame_step))
}

#[derive(Debug, Clone, Copy)]
struct RuntimeCubic {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

impl RuntimeCubic {
    fn constant(value: f64) -> Self {
        Self {
            a: 0.0,
            b: 0.0,
            c: 0.0,
            d: value,
        }
    }

    fn from_catmull_rom_points(y0: f64, y1: f64, y2: f64, y3: f64) -> Self {
        let d0 = y0 - y1;
        let d2 = y2 - y1;
        let d3 = y3 - y1;
        Self {
            a: -0.5 * d0 - 1.5 * d2 + 0.5 * d3,
            b: d0 + 2.0 * d2 - 0.5 * d3,
            c: 0.5 * (d2 - d0),
            d: y1,
        }
    }

    fn compose_affine(self, offset: f64, scale: f64) -> Self {
        let scale_squared = scale * scale;
        Self {
            a: self.a * scale_squared * scale,
            b: (3.0 * self.a * offset + self.b) * scale_squared,
            c: (3.0 * self.a * offset * offset + 2.0 * self.b * offset + self.c) * scale,
            d: self.value(offset),
        }
    }

    fn value(self, fraction: f64) -> f64 {
        ((self.a * fraction + self.b) * fraction + self.c) * fraction + self.d
    }

    fn slope_per_frame(self, fraction: f64) -> f64 {
        (3.0 * self.a * fraction + 2.0 * self.b) * fraction + self.c
    }

    fn bounds(self) -> CubicBounds {
        CubicBounds {
            a: Interval::point(self.a),
            b: Interval::point(self.b),
            c: Interval::point(self.c),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CubicBounds {
    a: Interval,
    b: Interval,
    c: Interval,
}

impl CubicBounds {
    fn slope_bounds(self) -> Interval {
        let unit = Interval::new(0.0, 1.0);
        self.a
            .multiply(Interval::point(3.0))
            .multiply(unit.square())
            .add(self.b.multiply(Interval::point(2.0)).multiply(unit))
            .add(self.c)
    }

    fn curvature_bounds(self) -> Interval {
        self.a
            .multiply(Interval::point(6.0))
            .multiply(Interval::new(0.0, 1.0))
            .add(self.b.multiply(Interval::point(2.0)))
    }
}

#[derive(Debug, Clone, Copy)]
struct Interval {
    lower: f64,
    upper: f64,
}

impl Interval {
    fn new(lower: f64, upper: f64) -> Self {
        Self { lower, upper }
    }

    fn point(value: f64) -> Self {
        Self::new(value, value)
    }

    fn add(self, other: Self) -> Self {
        Self::new(
            next_down(self.lower + other.lower),
            next_up(self.upper + other.upper),
        )
    }

    fn multiply(self, other: Self) -> Self {
        let products = [
            self.lower * other.lower,
            self.lower * other.upper,
            self.upper * other.lower,
            self.upper * other.upper,
        ];
        let lower = products.iter().copied().fold(f64::INFINITY, f64::min);
        let upper = products.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Self::new(next_down(lower), next_up(upper))
    }

    fn square(self) -> Self {
        if self.lower <= 0.0 && self.upper >= 0.0 {
            Self::new(0.0, next_up(self.lower.abs().max(self.upper.abs()).powi(2)))
        } else {
            let lower = self.lower.abs().min(self.upper.abs()).powi(2);
            let upper = self.lower.abs().max(self.upper.abs()).powi(2);
            Self::new(next_down(lower), next_up(upper))
        }
    }

    fn divide_positive(self, positive: Self) -> Self {
        debug_assert!(positive.lower > 0.0);
        Self::new(
            next_down(self.lower / positive.upper),
            next_up(self.upper / positive.lower),
        )
    }

    fn maximum_absolute(self) -> f64 {
        next_up(self.lower.abs().max(self.upper.abs()))
    }
}

fn next_up(value: f64) -> f64 {
    if value.is_nan() || value == f64::INFINITY {
        return value;
    }
    if value == 0.0 {
        return f64::from_bits(1);
    }
    if value > 0.0 {
        f64::from_bits(value.to_bits() + 1)
    } else {
        f64::from_bits(value.to_bits() - 1)
    }
}

fn next_down(value: f64) -> f64 {
    if value.is_nan() || value == f64::NEG_INFINITY {
        return value;
    }
    if value == 0.0 {
        return -f64::from_bits(1);
    }
    if value > 0.0 {
        f64::from_bits(value.to_bits() - 1)
    } else {
        f64::from_bits(value.to_bits() + 1)
    }
}

/// Supplies the validated numerical contract to the fixed-work tracer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CertifiedConcaveTraceBounds {
    representation_kind: GrooveTraceRepresentationKind,
    algorithm_version: u32,
    maximum_tracing_radius_m: f64,
    maximum_wall_curvature_per_m: f64,
    strict_concavity_margin_per_m: f64,
    maximum_absolute_wall_slope: f64,
    maximum_absolute_wall_curvature_per_m: f64,
    maximum_upward_wall_slope_jump: f64,
    maximum_upward_internal_displacement_jump_m: f64,
    maximum_source_frame_advance: f64,
    c1_joins_certified: bool,
    exact_internal_c0_c1_joins_certified: bool,
    clamped_edges_c1_certified: bool,
    base_level_coverage: u8,
    spatial_level_coverage: u8,
    edge_coverage: GrooveTraceEdgeCoverage,
}

impl CertifiedConcaveTraceBounds {
    pub(crate) fn strict_concavity_margin_per_m(self) -> f64 {
        self.strict_concavity_margin_per_m
    }

    pub(crate) fn covers_all_levels(self) -> bool {
        self.base_level_coverage == BASE_LEVEL_COVERAGE_BIT
            && self.spatial_level_coverage == ALL_SPATIAL_LEVEL_COVERAGE_BITS
    }

    pub(crate) fn for_geometry(
        self,
        geometry: StylusGeometry,
    ) -> Result<Self, GrooveTraceAdmissionError> {
        let geometry = geometry
            .validate()
            .map_err(|_| GrooveTraceAdmissionError::InvalidGeometry)?;
        if self.algorithm_version != CERTIFIED_CONCAVE_TRACER_ALGORITHM_VERSION
            || geometry.tracing_radius_m > self.maximum_tracing_radius_m
            || self.strict_concavity_margin_per_m <= 0.0
            || self.maximum_upward_wall_slope_jump > 0.0
            || self.maximum_upward_internal_displacement_jump_m > 0.0
            || !self.c1_joins_certified
            || !self.exact_internal_c0_c1_joins_certified
            || !self.clamped_edges_c1_certified
            || !self.covers_all_levels()
            || !self.edge_coverage.is_complete(self.representation_kind)
        {
            return Err(GrooveTraceAdmissionError::InvalidCertificate);
        }
        Ok(self)
    }
}

/// Supplies authorized fixed-cap work and output bounds to the class-B tracer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CertifiedPiecewiseTraceBounds {
    algorithm_version: u32,
    maximum_tracing_radius_m: f64,
    maximum_trace_pieces: u32,
    maximum_monotone_branches_per_piece: u8,
    maximum_endpoint_contenders_per_piece: u8,
    maximum_root_brackets_per_piece: u8,
    maximum_root_nodes_per_piece: u32,
    maximum_retained_contenders: u16,
    height_error_bound_m: f64,
    maximum_internal_displacement_join_enclosure_m: f64,
    contact_position_error_bound_m: f64,
    groove_slope_error_bound: f64,
    tangent_residual_error_bound: f64,
    runtime_height_order_required: bool,
}

impl CertifiedPiecewiseTraceBounds {
    pub(crate) fn maximum_trace_pieces(self) -> usize {
        self.maximum_trace_pieces as usize
    }

    pub(crate) fn maximum_monotone_branches_per_piece(self) -> usize {
        self.maximum_monotone_branches_per_piece as usize
    }

    pub(crate) fn maximum_endpoint_contenders_per_piece(self) -> usize {
        self.maximum_endpoint_contenders_per_piece as usize
    }

    pub(crate) fn maximum_root_brackets_per_piece(self) -> usize {
        self.maximum_root_brackets_per_piece as usize
    }

    pub(crate) fn maximum_root_nodes_per_piece(self) -> usize {
        self.maximum_root_nodes_per_piece as usize
    }

    pub(crate) fn maximum_retained_contenders(self) -> usize {
        self.maximum_retained_contenders as usize
    }

    pub(crate) fn height_error_bound_m(self) -> f64 {
        self.height_error_bound_m
    }

    pub(crate) fn maximum_internal_displacement_join_enclosure_m(self) -> f64 {
        self.maximum_internal_displacement_join_enclosure_m
    }

    pub(crate) fn contact_position_error_bound_m(self) -> f64 {
        self.contact_position_error_bound_m
    }

    pub(crate) fn groove_slope_error_bound(self) -> f64 {
        self.groove_slope_error_bound
    }

    pub(crate) fn tangent_residual_error_bound(self) -> f64 {
        self.tangent_residual_error_bound
    }

    fn for_geometry(self, geometry: StylusGeometry) -> Result<Self, GrooveTraceAdmissionError> {
        let geometry = geometry
            .validate()
            .map_err(|_| GrooveTraceAdmissionError::InvalidGeometry)?;
        if self.algorithm_version != CERTIFIED_CONCAVE_TRACER_ALGORITHM_VERSION
            || geometry.tracing_radius_m > self.maximum_tracing_radius_m
            || self.maximum_trace_pieces == 0
            || self.maximum_trace_pieces as usize > SPHERICAL_TRACE_MAX_PIECES
            || self.maximum_monotone_branches_per_piece as usize
                != SPHERICAL_TRACE_MAX_MONOTONE_BRANCHES_PER_PIECE
            || self.maximum_endpoint_contenders_per_piece
                != CERTIFIED_TRACE_MAXIMUM_ENDPOINT_CONTENDERS_PER_PIECE
            || self.maximum_root_brackets_per_piece as usize
                != SPHERICAL_TRACE_MAX_ROOT_BRACKETS_PER_PIECE
            || self.maximum_root_nodes_per_piece as usize
                != SPHERICAL_TRACE_MAX_ROOT_NODES_PER_PIECE
            || self.maximum_retained_contenders as usize != SPHERICAL_TRACE_MAX_GLOBAL_CONTENDERS
            || self.height_error_bound_m != SPHERICAL_TRACE_HEIGHT_ERROR_BOUND_M
            || !self
                .maximum_internal_displacement_join_enclosure_m
                .is_finite()
            || self.maximum_internal_displacement_join_enclosure_m < 0.0
            || self.maximum_internal_displacement_join_enclosure_m
                > CERTIFIED_TRACE_MAXIMUM_INTERNAL_DISPLACEMENT_JOIN_ENCLOSURE_M
            || self.contact_position_error_bound_m != SPHERICAL_TRACE_CONTACT_POSITION_ERROR_BOUND_M
            || self.groove_slope_error_bound != SPHERICAL_TRACE_GROOVE_SLOPE_ERROR_BOUND
            || self.tangent_residual_error_bound != SPHERICAL_TRACE_TANGENT_RESIDUAL_ERROR_BOUND
            || !self.runtime_height_order_required
        {
            return Err(GrooveTraceAdmissionError::InvalidCertificate);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum GrooveTraceAdmissionError {
    #[error("trace-admission certificate is invalid")]
    InvalidCertificate,
    #[error("trace-admission policy is invalid")]
    InvalidPolicy,
    #[error("trace-admission geometry is invalid")]
    InvalidGeometry,
    #[error("stylus geometry is outside the trace-admission certificate")]
    GeometryOutsideCertificate,
    #[error("trace-admission certificate does not prove strict concavity")]
    NotCertifiedConcave,
    #[error("trace-admission certificate does not prove fixed-cap piecewise tracing")]
    NotCertifiedPiecewise,
    #[error("fixed trace work could not be proved for this representation")]
    FixedWorkNotProved,
    #[error("trace-admission binding is invalid")]
    InvalidBinding,
    #[error("trace-admission representation is invalid")]
    InvalidRepresentation,
    #[error("trace-admission representation has no spline pieces")]
    InsufficientPieces,
    #[error("trace-admission piece count overflowed")]
    PieceCountOverflow,
    #[error("wall slope exceeds the trace-admission limit")]
    WallSlopeLimitExceeded,
    #[error("a representation join exceeds the trace-admission enclosure")]
    RepresentationJoinNotAdmitted,
    #[error("trace-admission certificate does not match the representation")]
    CertificateMismatch,
}

#[cfg(test)]
mod tests {
    use super::super::groove::GrooveSpatialPyramid;
    use super::*;

    fn flat_certificate() -> GrooveTraceAdmissionCertificate {
        let lateral = [0.0_f32; 16];
        let vertical = [0.0_f32; 16];
        let level_2 = [0.0_f32; 8];
        let level_4 = [0.0_f32; 4];
        let level_8 = [0.0_f32; 2];
        let level_16 = [0.0_f32; 1];
        let base = GrooveTraceAdmissionLevel {
            first_source_frame: 0,
            source_frame_step: 1,
            lateral_displacement_m: &lateral,
            vertical_displacement_m: &vertical,
        };
        let levels = [
            GrooveTraceAdmissionLevel {
                first_source_frame: 0,
                source_frame_step: 2,
                lateral_displacement_m: &level_2,
                vertical_displacement_m: &level_2,
            },
            GrooveTraceAdmissionLevel {
                first_source_frame: 0,
                source_frame_step: 4,
                lateral_displacement_m: &level_4,
                vertical_displacement_m: &level_4,
            },
            GrooveTraceAdmissionLevel {
                first_source_frame: 0,
                source_frame_step: 8,
                lateral_displacement_m: &level_8,
                vertical_displacement_m: &level_8,
            },
            GrooveTraceAdmissionLevel {
                first_source_frame: 0,
                source_frame_step: 16,
                lateral_displacement_m: &level_16,
                vertical_displacement_m: &level_16,
            },
        ];
        certify_groove_trace_representation(
            GrooveTraceAdmissionBinding {
                representation_kind: GrooveTraceRepresentationKind::Contiguous,
                representation_format_version: CONTIGUOUS_TRACE_REPRESENTATION_FORMAT_VERSION,
                source_content_identity: GrooveContentIdentity::from_sha256([0x51; 32]),
                generation: 0,
                core_start_frame: 0,
                core_end_frame_exclusive: lateral.len() as u64,
                stored_start_frame: 0,
                stored_end_frame_exclusive: lateral.len() as u64,
                record_end_frame_exclusive: lateral.len() as u64,
                minimum_meters_per_source_frame: 1.0e-6,
                maximum_geometry: StylusGeometry::default(),
                edge_coverage: GrooveTraceEdgeCoverage::contiguous(),
            },
            base,
            &levels,
        )
        .unwrap()
    }

    fn class_b_certificate() -> GrooveTraceAdmissionCertificate {
        let binding = GrooveTraceAdmissionBinding {
            representation_kind: GrooveTraceRepresentationKind::Contiguous,
            representation_format_version: CONTIGUOUS_TRACE_REPRESENTATION_FORMAT_VERSION,
            source_content_identity: GrooveContentIdentity::from_sha256([0x58; 32]),
            generation: 0,
            core_start_frame: 0,
            core_end_frame_exclusive: 16,
            stored_start_frame: 0,
            stored_end_frame_exclusive: 16,
            record_end_frame_exclusive: 16,
            minimum_meters_per_source_frame: 1.0e-6,
            maximum_geometry: StylusGeometry::default(),
            edge_coverage: GrooveTraceEdgeCoverage::contiguous(),
        };
        let enclosed_jump = CERTIFIED_TRACE_MAXIMUM_INTERNAL_DISPLACEMENT_JOIN_ENCLOSURE_M * 0.5;
        let certificate = finish_groove_trace_admission_certificate(
            binding,
            StylusGeometry::default(),
            GrooveContentIdentity::from_sha256([0x59; 32]),
            TraceAdmissionMetrics {
                maximum_upward_internal_displacement_jump_m: enclosed_jump,
                maximum_absolute_internal_displacement_jump_m: enclosed_jump,
                certified_piece_count: 1,
                c1_joins_certified: false,
                exact_internal_c0_c1_joins_certified: false,
                ..TraceAdmissionMetrics::default()
            },
        )
        .unwrap();
        assert_eq!(
            certificate.admission_class(),
            GrooveTraceAdmissionClass::FixedCapPiecewise
        );
        certificate
    }

    #[test]
    fn flat_representation_gets_a_recomputed_strict_concavity_token() {
        let certificate = flat_certificate();
        assert_eq!(
            certificate.admission_class(),
            GrooveTraceAdmissionClass::StrictConcavity
        );
        let validated = certificate.validate_recomputed(certificate).unwrap();
        let bounds = validated
            .certified_concave_trace_bounds(StylusGeometry::default())
            .unwrap();
        assert!(bounds.strict_concavity_margin_per_m() > 0.0);
    }

    #[test]
    fn certificate_binds_the_exact_certified_wall_slope_boundary() {
        let binding = GrooveTraceAdmissionBinding {
            representation_kind: GrooveTraceRepresentationKind::Contiguous,
            representation_format_version: CONTIGUOUS_TRACE_REPRESENTATION_FORMAT_VERSION,
            source_content_identity: GrooveContentIdentity::from_sha256([0x61; 32]),
            generation: 0,
            core_start_frame: 0,
            core_end_frame_exclusive: 16,
            stored_start_frame: 0,
            stored_end_frame_exclusive: 16,
            record_end_frame_exclusive: 16,
            minimum_meters_per_source_frame: 1.0e-6,
            maximum_geometry: StylusGeometry::default(),
            edge_coverage: GrooveTraceEdgeCoverage::contiguous(),
        };
        let friction_boundary_slope = (1.0 - 1.0e-6) / 0.25;
        let certificate = finish_groove_trace_admission_certificate(
            binding,
            StylusGeometry::default(),
            GrooveContentIdentity::from_sha256([0x62; 32]),
            TraceAdmissionMetrics {
                maximum_absolute_wall_slope: friction_boundary_slope,
                certified_piece_count: 1,
                ..TraceAdmissionMetrics::default()
            },
        )
        .unwrap();
        assert_eq!(
            certificate.maximum_absolute_wall_slope(),
            friction_boundary_slope
        );
        assert_eq!(
            certificate.admission_class(),
            GrooveTraceAdmissionClass::StrictConcavity
        );

        let mut tampered = certificate;
        tampered.maximum_absolute_wall_slope =
            f64::from_bits(friction_boundary_slope.to_bits() + 1);
        assert_eq!(
            tampered.validate_static(),
            Err(GrooveTraceAdmissionError::InvalidCertificate)
        );
    }

    #[test]
    fn self_consistent_forgery_cannot_create_an_authorization_token() {
        let certificate = flat_certificate();
        let mut forged = certificate;
        forged.maximum_absolute_wall_slope += 1.0e-12;
        forged.certificate_identity = forged.calculate_certificate_identity();
        assert!(forged.validate_static().is_ok());
        assert_eq!(
            forged.validate_recomputed(certificate),
            Err(GrooveTraceAdmissionError::CertificateMismatch)
        );
    }

    #[test]
    fn corrupt_certificate_identity_is_rejected_before_recomputation_match() {
        let certificate = flat_certificate();
        let mut corrupt = certificate;
        corrupt.certificate_identity = GrooveContentIdentity::from_sha256([0xA5; 32]);
        assert_eq!(
            corrupt.validate_recomputed(certificate),
            Err(GrooveTraceAdmissionError::InvalidCertificate)
        );
    }

    #[test]
    fn incremental_certificate_matches_the_full_pass_and_accounts_for_each_work_unit() {
        let lateral = [
            0.0_f32, 1.0e-7, -2.0e-7, 3.0e-7, -1.0e-7, 2.0e-7, -3.0e-7, 1.0e-7, 0.0, -1.0e-7,
            2.0e-7, -2.0e-7, 1.0e-7, 0.0, 1.0e-7, 0.0,
        ];
        let vertical = [
            0.0_f32, -1.0e-7, 1.0e-7, -2.0e-7, 2.0e-7, -1.0e-7, 1.0e-7, 0.0, -1.0e-7, 1.0e-7,
            -2.0e-7, 2.0e-7, -1.0e-7, 1.0e-7, 0.0, 0.0,
        ];
        let base = GrooveTraceAdmissionLevel {
            first_source_frame: 0,
            source_frame_step: 1,
            lateral_displacement_m: &lateral,
            vertical_displacement_m: &vertical,
        };
        let levels = [
            GrooveTraceAdmissionLevel {
                first_source_frame: 0,
                source_frame_step: 2,
                lateral_displacement_m: &lateral[..8],
                vertical_displacement_m: &vertical[..8],
            },
            GrooveTraceAdmissionLevel {
                first_source_frame: 0,
                source_frame_step: 4,
                lateral_displacement_m: &lateral[..4],
                vertical_displacement_m: &vertical[..4],
            },
            GrooveTraceAdmissionLevel {
                first_source_frame: 0,
                source_frame_step: 8,
                lateral_displacement_m: &lateral[..2],
                vertical_displacement_m: &vertical[..2],
            },
            GrooveTraceAdmissionLevel {
                first_source_frame: 0,
                source_frame_step: 16,
                lateral_displacement_m: &lateral[..1],
                vertical_displacement_m: &vertical[..1],
            },
        ];
        let binding = GrooveTraceAdmissionBinding {
            representation_kind: GrooveTraceRepresentationKind::Contiguous,
            representation_format_version: CONTIGUOUS_TRACE_REPRESENTATION_FORMAT_VERSION,
            source_content_identity: GrooveContentIdentity::from_sha256([0x61; 32]),
            generation: 0,
            core_start_frame: 0,
            core_end_frame_exclusive: lateral.len() as u64,
            stored_start_frame: 0,
            stored_end_frame_exclusive: lateral.len() as u64,
            record_end_frame_exclusive: lateral.len() as u64,
            minimum_meters_per_source_frame: 1.0e-6,
            maximum_geometry: StylusGeometry::default(),
            edge_coverage: GrooveTraceEdgeCoverage::contiguous(),
        };
        let full = certify_groove_trace_representation(binding, base, &levels).unwrap();
        let expected_work = groove_trace_admission_work_units(binding, base, &levels).unwrap();
        let mut incremental =
            GrooveTraceAdmissionIncrementalState::new(binding, base, &levels).unwrap();

        let first = incremental.advance(base, &levels, 1).unwrap();
        assert_eq!(first.work_units_consumed, 1);
        assert!(first.certificate.is_none());

        let mut total_work = u64::from(first.work_units_consumed);
        let incremental_certificate = loop {
            let progress = incremental.advance(base, &levels, 1).unwrap();
            assert!(progress.work_units_consumed <= 1);
            total_work += u64::from(progress.work_units_consumed);
            if let Some(certificate) = progress.certificate {
                break certificate;
            }
        };

        assert_eq!(incremental_certificate, full);
        assert_eq!(total_work, expected_work);
    }

    fn assert_paged_incremental_certificate_matches(
        binding: GrooveTraceAdmissionBinding,
        budgets: &[u32],
    ) {
        assert!(!budgets.is_empty());
        assert!(budgets.iter().all(|budget| *budget > 0));
        let stored_len =
            usize::try_from(binding.stored_end_frame_exclusive - binding.stored_start_frame)
                .unwrap();
        let lateral: Vec<f32> = (0..stored_len)
            .map(|offset| {
                let frame = binding.stored_start_frame + offset as u64;
                (((frame * 17 + 3) % 29) as i64 - 14) as f32 * 1.0e-9
            })
            .collect();
        let vertical: Vec<f32> = (0..stored_len)
            .map(|offset| {
                let frame = binding.stored_start_frame + offset as u64;
                (((frame * 11 + 5) % 31) as i64 - 15) as f32 * 0.75e-9
            })
            .collect();
        let pyramid =
            GrooveSpatialPyramid::build_window(binding.stored_start_frame, &lateral, &vertical)
                .unwrap();
        let base = GrooveTraceAdmissionLevel {
            first_source_frame: binding.stored_start_frame,
            source_frame_step: 1,
            lateral_displacement_m: &lateral,
            vertical_displacement_m: &vertical,
        };
        let levels = std::array::from_fn(|level_index| {
            let level = &pyramid.levels()[level_index];
            GrooveTraceAdmissionLevel {
                first_source_frame: level.first_source_frame(),
                source_frame_step: level.source_frame_step(),
                lateral_displacement_m: level.lateral_displacement_m(),
                vertical_displacement_m: level.vertical_displacement_m(),
            }
        });
        let full = certify_groove_trace_representation(binding, base, &levels).unwrap();
        let expected_work = groove_trace_admission_work_units(binding, base, &levels).unwrap();
        let mut incremental =
            GrooveTraceAdmissionIncrementalState::new(binding, base, &levels).unwrap();
        let mut total_work = 0_u64;
        let mut advance_count = 0_u64;

        let incremental_certificate = loop {
            let budget = budgets[advance_count as usize % budgets.len()];
            let progress = incremental.advance(base, &levels, budget).unwrap();
            assert!(progress.work_units_consumed <= budget);
            total_work += u64::from(progress.work_units_consumed);
            advance_count += 1;
            assert!(advance_count <= expected_work + 1);
            if let Some(certificate) = progress.certificate {
                break certificate;
            }
            assert!(progress.work_units_consumed > 0);
        };

        assert_eq!(incremental_certificate, full);
        assert_eq!(total_work, expected_work);
    }

    #[test]
    fn paged_incremental_certification_matches_full_pass_for_unaligned_and_boundary_windows() {
        let minimum_meters_per_source_frame = 1.0e-3;
        let required_halo = u64::from(
            StylusGeometry::default()
                .multiresolution_support(minimum_meters_per_source_frame, 16)
                .unwrap()
                .symmetric_halo_source_frames()
                + GROOVE_SPATIAL_FILTER_RADIUS_FRAMES,
        );
        let record_end_frame_exclusive = 4_099;
        let source_content_identity = GrooveContentIdentity::from_sha256([0x68; 32]);
        let interior_stored_start = 37;
        let interior_core_start = interior_stored_start + required_halo;
        let interior_core_end = interior_core_start + 127;
        let interior_stored_end = interior_core_end + required_halo;
        let final_core_start = record_end_frame_exclusive - 127;
        let final_stored_start = final_core_start - required_halo;
        assert_ne!(interior_stored_start % 16, 0);
        assert_ne!(final_stored_start % 16, 0);

        let windows = [
            (
                interior_core_start,
                interior_core_end,
                interior_stored_start,
                interior_stored_end,
            ),
            (0, 127, 0, 127 + required_halo),
            (
                final_core_start,
                record_end_frame_exclusive,
                final_stored_start,
                record_end_frame_exclusive,
            ),
        ];
        for (core_start, core_end, stored_start, stored_end) in windows {
            let binding = GrooveTraceAdmissionBinding {
                representation_kind: GrooveTraceRepresentationKind::Paged,
                representation_format_version: PAGED_TRACE_REPRESENTATION_FORMAT_VERSION,
                source_content_identity,
                generation: 7,
                core_start_frame: core_start,
                core_end_frame_exclusive: core_end,
                stored_start_frame: stored_start,
                stored_end_frame_exclusive: stored_end,
                record_end_frame_exclusive,
                minimum_meters_per_source_frame,
                maximum_geometry: StylusGeometry::default(),
                edge_coverage: GrooveTraceEdgeCoverage::paged(
                    stored_start == 0,
                    stored_end == record_end_frame_exclusive,
                ),
            };
            assert_paged_incremental_certificate_matches(binding, &[1]);
            assert_paged_incremental_certificate_matches(binding, &[17, 2, 61, 1, 127, 5]);
        }
    }

    #[test]
    fn class_b_only_admits_internal_c0_mismatch_inside_the_registered_enclosure() {
        let binding = GrooveTraceAdmissionBinding {
            representation_kind: GrooveTraceRepresentationKind::Contiguous,
            representation_format_version: CONTIGUOUS_TRACE_REPRESENTATION_FORMAT_VERSION,
            source_content_identity: GrooveContentIdentity::from_sha256([0x71; 32]),
            generation: 0,
            core_start_frame: 0,
            core_end_frame_exclusive: 16,
            stored_start_frame: 0,
            stored_end_frame_exclusive: 16,
            record_end_frame_exclusive: 16,
            minimum_meters_per_source_frame: 1.0e-6,
            maximum_geometry: StylusGeometry::default(),
            edge_coverage: GrooveTraceEdgeCoverage::contiguous(),
        };
        let representation_content_identity = GrooveContentIdentity::from_sha256([0x72; 32]);
        let enclosed_jump = CERTIFIED_TRACE_MAXIMUM_INTERNAL_DISPLACEMENT_JOIN_ENCLOSURE_M * 0.5;
        let enclosed_metrics = TraceAdmissionMetrics {
            maximum_upward_internal_displacement_jump_m: enclosed_jump,
            maximum_absolute_internal_displacement_jump_m: enclosed_jump,
            certified_piece_count: 1,
            c1_joins_certified: false,
            exact_internal_c0_c1_joins_certified: false,
            ..TraceAdmissionMetrics::default()
        };
        let enclosed = finish_groove_trace_admission_certificate(
            binding,
            StylusGeometry::default(),
            representation_content_identity,
            enclosed_metrics,
        )
        .unwrap();
        assert_eq!(
            enclosed.admission_class(),
            GrooveTraceAdmissionClass::FixedCapPiecewise
        );
        let bounds = enclosed
            .validate_recomputed(enclosed)
            .unwrap()
            .fixed_cap_piecewise_trace_bounds(StylusGeometry::default())
            .unwrap();
        assert_eq!(
            bounds.maximum_internal_displacement_join_enclosure_m(),
            enclosed_jump
        );

        let material_jump = CERTIFIED_TRACE_MAXIMUM_INTERNAL_DISPLACEMENT_JOIN_ENCLOSURE_M * 2.0;
        let material_metrics = TraceAdmissionMetrics {
            maximum_upward_internal_displacement_jump_m: material_jump,
            maximum_absolute_internal_displacement_jump_m: material_jump,
            certified_piece_count: 1,
            c1_joins_certified: false,
            exact_internal_c0_c1_joins_certified: false,
            ..TraceAdmissionMetrics::default()
        };
        let material = finish_groove_trace_admission_certificate(
            binding,
            StylusGeometry::default(),
            representation_content_identity,
            material_metrics,
        )
        .unwrap();
        assert_eq!(
            material.admission_class(),
            GrooveTraceAdmissionClass::RejectedRepresentationJoin
        );
        assert_eq!(
            material.validate_for_active_tracing(StylusGeometry::default()),
            Err(GrooveTraceAdmissionError::RepresentationJoinNotAdmitted)
        );
    }

    #[test]
    fn class_b_certificate_rejects_noncanonical_runtime_capacities_with_a_matching_digest() {
        let certificate = class_b_certificate();
        assert!(certificate.validate_static().is_ok());

        macro_rules! assert_invalid_capacity {
            ($field:ident, $value:expr) => {{
                let mut invalid = certificate;
                invalid.$field = $value;
                invalid.certificate_identity = invalid.calculate_certificate_identity();
                assert_eq!(
                    invalid.validate_static(),
                    Err(GrooveTraceAdmissionError::InvalidCertificate),
                    "{} accepted {}",
                    stringify!($field),
                    $value
                );
            }};
        }

        assert_invalid_capacity!(maximum_monotone_branches_per_piece, 0);
        assert_invalid_capacity!(
            maximum_monotone_branches_per_piece,
            SPHERICAL_TRACE_MAX_MONOTONE_BRANCHES_PER_PIECE as u8 - 1
        );
        assert_invalid_capacity!(maximum_root_brackets_per_piece, 0);
        assert_invalid_capacity!(
            maximum_root_brackets_per_piece,
            SPHERICAL_TRACE_MAX_ROOT_BRACKETS_PER_PIECE as u8 - 1
        );
        assert_invalid_capacity!(maximum_root_nodes_per_piece, 0);
        assert_invalid_capacity!(
            maximum_root_nodes_per_piece,
            SPHERICAL_TRACE_MAX_ROOT_NODES_PER_PIECE as u32 - 1
        );
        assert_invalid_capacity!(maximum_retained_contenders, 0);
        assert_invalid_capacity!(
            maximum_retained_contenders,
            SPHERICAL_TRACE_MAX_GLOBAL_CONTENDERS as u16 - 1
        );
    }

    #[test]
    fn class_b_bounds_reject_noncanonical_runtime_capacities() {
        let geometry = StylusGeometry::default();
        let bounds = class_b_certificate()
            .validate_recomputed(class_b_certificate())
            .unwrap()
            .fixed_cap_piecewise_trace_bounds(geometry)
            .unwrap();

        macro_rules! assert_invalid_capacity {
            ($field:ident, $value:expr) => {{
                let mut invalid = bounds;
                invalid.$field = $value;
                assert_eq!(
                    invalid.for_geometry(geometry),
                    Err(GrooveTraceAdmissionError::InvalidCertificate),
                    "{} accepted {}",
                    stringify!($field),
                    $value
                );
            }};
        }

        assert_invalid_capacity!(maximum_monotone_branches_per_piece, 0);
        assert_invalid_capacity!(
            maximum_monotone_branches_per_piece,
            SPHERICAL_TRACE_MAX_MONOTONE_BRANCHES_PER_PIECE as u8 - 1
        );
        assert_invalid_capacity!(maximum_root_brackets_per_piece, 0);
        assert_invalid_capacity!(
            maximum_root_brackets_per_piece,
            SPHERICAL_TRACE_MAX_ROOT_BRACKETS_PER_PIECE as u8 - 1
        );
        assert_invalid_capacity!(maximum_root_nodes_per_piece, 0);
        assert_invalid_capacity!(
            maximum_root_nodes_per_piece,
            SPHERICAL_TRACE_MAX_ROOT_NODES_PER_PIECE as u32 - 1
        );
        assert_invalid_capacity!(maximum_retained_contenders, 0);
        assert_invalid_capacity!(
            maximum_retained_contenders,
            SPHERICAL_TRACE_MAX_GLOBAL_CONTENDERS as u16 - 1
        );
    }

    #[test]
    fn rejected_certificates_report_wall_slope_and_fixed_work_separately() {
        let base_binding = GrooveTraceAdmissionBinding {
            representation_kind: GrooveTraceRepresentationKind::Contiguous,
            representation_format_version: CONTIGUOUS_TRACE_REPRESENTATION_FORMAT_VERSION,
            source_content_identity: GrooveContentIdentity::from_sha256([0x73; 32]),
            generation: 0,
            core_start_frame: 0,
            core_end_frame_exclusive: 16,
            stored_start_frame: 0,
            stored_end_frame_exclusive: 16,
            record_end_frame_exclusive: 16,
            minimum_meters_per_source_frame: 1.0e-6,
            maximum_geometry: StylusGeometry::default(),
            edge_coverage: GrooveTraceEdgeCoverage::contiguous(),
        };
        let identity = GrooveContentIdentity::from_sha256([0x74; 32]);
        let wall_slope = finish_groove_trace_admission_certificate(
            base_binding,
            StylusGeometry::default(),
            identity,
            TraceAdmissionMetrics {
                maximum_absolute_wall_slope: CERTIFIED_TRACE_MAXIMUM_ABSOLUTE_WALL_SLOPE + 1.0,
                certified_piece_count: 1,
                ..TraceAdmissionMetrics::default()
            },
        )
        .unwrap();
        assert_eq!(
            wall_slope.admission_class(),
            GrooveTraceAdmissionClass::RejectedWallSlope
        );
        assert_eq!(
            wall_slope.validate_for_active_tracing(StylusGeometry::default()),
            Err(GrooveTraceAdmissionError::WallSlopeLimitExceeded)
        );

        let fixed_work = finish_groove_trace_admission_certificate(
            GrooveTraceAdmissionBinding {
                minimum_meters_per_source_frame: 0.01e-6,
                ..base_binding
            },
            StylusGeometry::default(),
            identity,
            TraceAdmissionMetrics {
                certified_piece_count: 1,
                exact_internal_c0_c1_joins_certified: false,
                ..TraceAdmissionMetrics::default()
            },
        )
        .unwrap();
        assert_eq!(
            fixed_work.admission_class(),
            GrooveTraceAdmissionClass::RejectedFixedWorkSupport
        );
        assert_eq!(
            fixed_work.validate_for_active_tracing(StylusGeometry::default()),
            Err(GrooveTraceAdmissionError::FixedWorkNotProved)
        );
    }

    #[test]
    fn certification_rejects_a_noncanonical_spatial_level_layout() {
        let zero = [0.0_f32; 16];
        let base = GrooveTraceAdmissionLevel {
            first_source_frame: 0,
            source_frame_step: 1,
            lateral_displacement_m: &zero,
            vertical_displacement_m: &zero,
        };
        let mut levels = [
            GrooveTraceAdmissionLevel {
                first_source_frame: 0,
                source_frame_step: 2,
                lateral_displacement_m: &zero[..8],
                vertical_displacement_m: &zero[..8],
            },
            GrooveTraceAdmissionLevel {
                first_source_frame: 0,
                source_frame_step: 4,
                lateral_displacement_m: &zero[..4],
                vertical_displacement_m: &zero[..4],
            },
            GrooveTraceAdmissionLevel {
                first_source_frame: 0,
                source_frame_step: 8,
                lateral_displacement_m: &zero[..2],
                vertical_displacement_m: &zero[..2],
            },
            GrooveTraceAdmissionLevel {
                first_source_frame: 0,
                source_frame_step: 16,
                lateral_displacement_m: &zero[..1],
                vertical_displacement_m: &zero[..1],
            },
        ];
        levels[2].first_source_frame = 8;
        let binding = GrooveTraceAdmissionBinding {
            representation_kind: GrooveTraceRepresentationKind::Contiguous,
            representation_format_version: CONTIGUOUS_TRACE_REPRESENTATION_FORMAT_VERSION,
            source_content_identity: GrooveContentIdentity::from_sha256([0x81; 32]),
            generation: 0,
            core_start_frame: 0,
            core_end_frame_exclusive: 16,
            stored_start_frame: 0,
            stored_end_frame_exclusive: 16,
            record_end_frame_exclusive: 16,
            minimum_meters_per_source_frame: 1.0e-6,
            maximum_geometry: StylusGeometry::default(),
            edge_coverage: GrooveTraceEdgeCoverage::contiguous(),
        };
        assert_eq!(
            certify_groove_trace_representation(binding, base, &levels),
            Err(GrooveTraceAdmissionError::InvalidRepresentation)
        );
        assert!(matches!(
            GrooveTraceAdmissionIncrementalState::new(binding, base, &levels),
            Err(GrooveTraceAdmissionError::InvalidRepresentation)
        ));
    }

    #[test]
    fn certification_rejects_a_hidden_page_clamp_one_frame_inside_the_trace_halo() {
        let geometry = StylusGeometry::default();
        let minimum_meters_per_source_frame = 1.0e-6;
        let tracing_halo = geometry
            .multiresolution_support(minimum_meters_per_source_frame, 16)
            .unwrap()
            .symmetric_halo_source_frames();
        let required_halo = u64::from(tracing_halo + GROOVE_SPATIAL_FILTER_RADIUS_FRAMES);
        let record_end = 8_192;
        let core_start = 2_048;
        let core_end = 3_072;
        assert!(required_halo < core_start);
        let stored_start = core_start - (required_halo - 1);
        let stored_end = core_end + required_halo;
        let base_values = vec![0.0_f32; (stored_end - stored_start) as usize];
        let base = GrooveTraceAdmissionLevel {
            first_source_frame: stored_start,
            source_frame_step: 1,
            lateral_displacement_m: &base_values,
            vertical_displacement_m: &base_values,
        };
        let mut layouts = [(0_u64, 0_u32, 0_usize); GROOVE_SPATIAL_PYRAMID_LEVELS];
        let mut first = stored_start;
        let mut step = 1_u32;
        let mut len = base_values.len();
        for layout in &mut layouts {
            let next_step = step * 2;
            let next_first = align_up(first, u64::from(next_step)).unwrap();
            let last = first + (len.saturating_sub(1) as u64) * u64::from(step);
            let next_len = if next_first > last {
                0
            } else {
                ((last - next_first) / u64::from(next_step) + 1) as usize
            };
            *layout = (next_first, next_step, next_len);
            first = next_first;
            step = next_step;
            len = next_len;
        }
        let level_values: [Vec<f32>; GROOVE_SPATIAL_PYRAMID_LEVELS] =
            std::array::from_fn(|index| vec![0.0; layouts[index].2]);
        let levels = std::array::from_fn(|index| GrooveTraceAdmissionLevel {
            first_source_frame: layouts[index].0,
            source_frame_step: layouts[index].1,
            lateral_displacement_m: &level_values[index],
            vertical_displacement_m: &level_values[index],
        });
        let binding = GrooveTraceAdmissionBinding {
            representation_kind: GrooveTraceRepresentationKind::Paged,
            representation_format_version: PAGED_TRACE_REPRESENTATION_FORMAT_VERSION,
            source_content_identity: GrooveContentIdentity::from_sha256([0x91; 32]),
            generation: 1,
            core_start_frame: core_start,
            core_end_frame_exclusive: core_end,
            stored_start_frame: stored_start,
            stored_end_frame_exclusive: stored_end,
            record_end_frame_exclusive: record_end,
            minimum_meters_per_source_frame,
            maximum_geometry: geometry,
            edge_coverage: GrooveTraceEdgeCoverage::paged(false, false),
        };
        assert_eq!(
            certify_groove_trace_representation(binding, base, &levels),
            Err(GrooveTraceAdmissionError::InvalidBinding)
        );
        assert!(matches!(
            GrooveTraceAdmissionIncrementalState::new(binding, base, &levels),
            Err(GrooveTraceAdmissionError::InvalidBinding)
        ));
    }
}
