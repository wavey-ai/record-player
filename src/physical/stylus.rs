use serde::{Deserialize, Serialize};
use thiserror::Error;

const MIN_TRACING_RADIUS_M: f64 = 0.5e-6;
const MAX_TRACING_RADIUS_M: f64 = 100.0e-6;
const MIN_METERS_PER_FRAME: f64 = 1.0e-9;
const MAX_SEARCH_SAMPLES: usize = 256;
const MAX_TRACE_PIECES: usize = MAX_SEARCH_SAMPLES * 2 + 2;
const MAX_ROOT_SEARCH_NODES_PER_PIECE: usize = 4_095;
const MAX_ROOT_BRACKETS_PER_PIECE: usize = 8;
const ROOT_SEARCH_STACK_CAPACITY: usize = 64;
const ROOT_BRACKET_WIDTH_M: f64 = 1.0e-15;
const MAX_GLOBAL_CONTENDERS: usize = 64;

/// Maximum envelope-height error accepted by the spherical tracer.
pub const SPHERICAL_TRACE_HEIGHT_ERROR_BOUND_M: f64 = 1.0e-12;

/// Maximum contact-position error accepted by the spherical tracer.
pub const SPHERICAL_TRACE_CONTACT_POSITION_ERROR_BOUND_M: f64 = 1.0e-10;

/// Maximum groove-slope error accepted by the spherical tracer.
pub const SPHERICAL_TRACE_GROOVE_SLOPE_ERROR_BOUND: f64 = 1.0e-6;

/// Maximum tangent-residual error accepted by the spherical tracer.
pub const SPHERICAL_TRACE_TANGENT_RESIDUAL_ERROR_BOUND: f64 = 1.0e-6;

/// Maximum number of spline pieces examined by one spherical trace.
pub const SPHERICAL_TRACE_MAX_PIECES: usize = MAX_TRACE_PIECES;

/// Maximum interval-search nodes examined for one spline piece.
pub const SPHERICAL_TRACE_MAX_ROOT_NODES_PER_PIECE: usize = MAX_ROOT_SEARCH_NODES_PER_PIECE;

/// Maximum certified same-wall contacts returned by one trace.
pub const MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StylusGeometry {
    /// This is the rounded tracing radius along the groove.
    pub tracing_radius_m: f64,
}

impl StylusGeometry {
    pub fn concorde_mkii_scratch() -> Self {
        Self {
            tracing_radius_m: 18.0e-6,
        }
    }

    pub fn validate(self) -> Result<Self, StylusTraceError> {
        if !self.tracing_radius_m.is_finite()
            || !(MIN_TRACING_RADIUS_M..=MAX_TRACING_RADIUS_M).contains(&self.tracing_radius_m)
        {
            return Err(StylusTraceError::InvalidTracingRadius);
        }
        Ok(self)
    }

    /// Returns exact source-frame support for spherical multiresolution tracing.
    pub fn multiresolution_support(
        self,
        meters_per_source_frame: f64,
        maximum_level_step_source_frames: u32,
    ) -> Result<StylusTraceSupport, StylusTraceError> {
        let geometry = self.validate()?;
        if !meters_per_source_frame.is_finite() || meters_per_source_frame < MIN_METERS_PER_FRAME {
            return Err(StylusTraceError::InvalidSpatialStep);
        }
        if maximum_level_step_source_frames == 0 {
            return Err(StylusTraceError::InvalidLevelCoordinates);
        }
        let search_half_span_source_frames =
            (geometry.tracing_radius_m / meters_per_source_frame).ceil() as usize;
        if search_half_span_source_frames > MAX_SEARCH_SAMPLES {
            return Err(StylusTraceError::SearchLimitExceeded);
        }
        let search_half_span_source_frames = u32::try_from(search_half_span_source_frames)
            .map_err(|_| StylusTraceError::InvalidLevelCoordinates)?;
        let left_source_frames = search_half_span_source_frames
            .checked_add(maximum_level_step_source_frames)
            .ok_or(StylusTraceError::InvalidLevelCoordinates)?;
        let right_source_frames = search_half_span_source_frames
            .checked_add(
                maximum_level_step_source_frames
                    .checked_mul(2)
                    .ok_or(StylusTraceError::InvalidLevelCoordinates)?,
            )
            .ok_or(StylusTraceError::InvalidLevelCoordinates)?;
        Ok(StylusTraceSupport {
            search_half_span_source_frames,
            left_source_frames,
            right_source_frames,
        })
    }
}

impl Default for StylusGeometry {
    fn default() -> Self {
        Self::concorde_mkii_scratch()
    }
}

/// Describes all source frames read by one spherical trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StylusTraceSupport {
    search_half_span_source_frames: u32,
    left_source_frames: u32,
    right_source_frames: u32,
}

impl StylusTraceSupport {
    pub fn search_half_span_source_frames(self) -> u32 {
        self.search_half_span_source_frames
    }

    pub fn left_source_frames(self) -> u32 {
        self.left_source_frames
    }

    pub fn right_source_frames(self) -> u32 {
        self.right_source_frames
    }

    pub fn symmetric_halo_source_frames(self) -> u32 {
        self.left_source_frames.max(self.right_source_frames)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StylusTraceSample {
    /// This displacement excludes the static stylus-radius offset.
    pub center_displacement_m: f64,
    pub contact_offset_m: f64,
    pub groove_displacement_m: f64,
    pub groove_slope: f64,
    pub tangent_residual: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StylusTraceContact {
    pub contact_offset_m: f64,
    pub groove_displacement_m: f64,
    pub groove_slope: f64,
    pub tangent_residual: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StylusTraceContactSet {
    /// This displacement excludes the static stylus-radius offset.
    pub center_displacement_m: f64,
    pub contact_count: u8,
    pub contacts: [StylusTraceContact; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
}

impl Default for StylusTraceContactSet {
    fn default() -> Self {
        Self {
            center_displacement_m: 0.0,
            contact_count: 1,
            contacts: [StylusTraceContact::default(); MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
        }
    }
}

impl StylusTraceContactSet {
    fn into_single_sample(self) -> Result<StylusTraceSample, StylusTraceError> {
        if self.contact_count != 1 {
            return Err(StylusTraceError::GlobalContactNotIsolated);
        }
        let contact = self.contacts[0];
        Ok(StylusTraceSample {
            center_displacement_m: self.center_displacement_m,
            contact_offset_m: contact.contact_offset_m,
            groove_displacement_m: contact.groove_displacement_m,
            groove_slope: contact.groove_slope,
            tangent_residual: contact.tangent_residual,
        })
    }
}

fn single_sample_from_contacts(
    result: Result<StylusTraceContactSet, StylusTraceError>,
) -> Result<StylusTraceSample, StylusTraceError> {
    match result {
        Ok(contacts) => contacts.into_single_sample(),
        Err(StylusTraceError::ContactHeightOrderNotIsolated) => {
            Err(StylusTraceError::GlobalContactNotIsolated)
        }
        Err(error) => Err(error),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum StylusTraceError {
    #[error("tracing radius is outside the supported range")]
    InvalidTracingRadius,
    #[error("meters per frame must be finite and positive")]
    InvalidSpatialStep,
    #[error("groove displacement requires at least four samples")]
    InsufficientSamples,
    #[error("contact search exceeds the fixed real-time limit")]
    SearchLimitExceeded,
    #[error("center frame must be finite")]
    InvalidCenterFrame,
    #[error("45/45 groove channels must have equal lengths")]
    ChannelLengthMismatch,
    #[error("45/45 wall index must be zero or one")]
    InvalidWallIndex,
    #[error("spatial anti-alias blend must be between zero and one")]
    InvalidLevelBlend,
    #[error("spatial anti-alias level coordinates are invalid")]
    InvalidLevelCoordinates,
    #[error("groove displacement contains a non-finite value in the tracing support")]
    InvalidGrooveDisplacement,
    #[error("the fixed solver could not isolate all stationary contacts")]
    StationaryContactNotIsolated,
    #[error("the fixed solver could not identify one global contact position")]
    GlobalContactNotIsolated,
    #[error("the fixed solver could not certify the height order of distinct contact candidates")]
    ContactHeightOrderNotIsolated,
    #[error("the certified same-wall contact set exceeds its fixed capacity")]
    ContactCapacityExceeded,
    #[error("the fixed solver could not establish a stable contact order")]
    ContactOrderNotIsolated,
    #[error("the fixed solver could not meet the envelope-height error bound")]
    EnvelopeHeightBoundNotMet,
    #[error("the fixed solver could not meet the groove-slope error bound")]
    GrooveSlopeBoundNotMet,
    #[error("the fixed solver could not meet the tangent-residual error bound")]
    TangentResidualBoundNotMet,
    #[error(
        "page tracing halo has {available_frames} frames but this trace requires {required_frames}"
    )]
    InsufficientPageHalo {
        required_frames: u32,
        available_frames: u32,
    },
}

/// Traces one rounded groove section with a spherical stylus cross-section.
///
/// The solver examines every Catmull-Rom piece under the stylus. It uses
/// outward-rounded intervals and fixed work limits.
pub fn trace_spherical_uniform(
    groove_displacement_m: &[f32],
    center_frame: f64,
    meters_per_frame: f64,
    geometry: StylusGeometry,
) -> Result<StylusTraceSample, StylusTraceError> {
    single_sample_from_contacts(trace_spherical_uniform_contacts(
        groove_displacement_m,
        center_frame,
        meters_per_frame,
        geometry,
    ))
}

/// Traces one certified global contact for one rounded groove section.
///
/// The function returns an ambiguity error if it cannot certify height order.
pub fn trace_spherical_uniform_contacts(
    groove_displacement_m: &[f32],
    center_frame: f64,
    meters_per_frame: f64,
    geometry: StylusGeometry,
) -> Result<StylusTraceContactSet, StylusTraceError> {
    if groove_displacement_m.len() < 4 {
        return Err(StylusTraceError::InsufficientSamples);
    }
    trace_spherical_piecewise_contacts(
        groove_displacement_m.len(),
        center_frame,
        meters_per_frame,
        geometry,
        |segment_start| catmull_rom_cubic(groove_displacement_m, segment_start),
    )
}

/// Traces one physical wall from lateral and vertical 45/45 groove data.
///
/// Wall zero uses the inward normal `(lateral + vertical) / sqrt(2)`.
/// Wall one uses the inward normal `(-lateral + vertical) / sqrt(2)`.
pub fn trace_spherical_45_45_wall_uniform(
    lateral_displacement_m: &[f32],
    vertical_displacement_m: &[f32],
    wall_index: usize,
    center_frame: f64,
    meters_per_frame: f64,
    geometry: StylusGeometry,
) -> Result<StylusTraceSample, StylusTraceError> {
    single_sample_from_contacts(trace_spherical_45_45_wall_uniform_contacts(
        lateral_displacement_m,
        vertical_displacement_m,
        wall_index,
        center_frame,
        meters_per_frame,
        geometry,
    ))
}

/// Traces one certified global contact for one physical 45/45 wall.
///
/// The function returns an ambiguity error if it cannot certify height order.
#[allow(clippy::too_many_arguments)]
pub fn trace_spherical_45_45_wall_uniform_contacts(
    lateral_displacement_m: &[f32],
    vertical_displacement_m: &[f32],
    wall_index: usize,
    center_frame: f64,
    meters_per_frame: f64,
    geometry: StylusGeometry,
) -> Result<StylusTraceContactSet, StylusTraceError> {
    if lateral_displacement_m.len() != vertical_displacement_m.len() {
        return Err(StylusTraceError::ChannelLengthMismatch);
    }
    if lateral_displacement_m.len() < 4 {
        return Err(StylusTraceError::InsufficientSamples);
    }
    let (lateral_sign, vertical_sign) = match wall_index {
        0 => (
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        1 => (
            -std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        _ => return Err(StylusTraceError::InvalidWallIndex),
    };
    trace_spherical_piecewise_contacts(
        lateral_displacement_m.len(),
        center_frame,
        meters_per_frame,
        geometry,
        |segment_start| {
            catmull_rom_combined_cubic(
                lateral_displacement_m,
                lateral_sign,
                vertical_displacement_m,
                vertical_sign,
                segment_start,
            )
        },
    )
}

/// Traces one wall through a continuous blend of two prefiltered groove levels.
#[allow(clippy::too_many_arguments)]
pub fn trace_spherical_45_45_wall_blended_uniform(
    lower_lateral_displacement_m: &[f32],
    lower_vertical_displacement_m: &[f32],
    upper_lateral_displacement_m: &[f32],
    upper_vertical_displacement_m: &[f32],
    upper_level_blend: f64,
    wall_index: usize,
    center_frame: f64,
    meters_per_frame: f64,
    geometry: StylusGeometry,
) -> Result<StylusTraceSample, StylusTraceError> {
    single_sample_from_contacts(trace_spherical_45_45_wall_blended_uniform_contacts(
        lower_lateral_displacement_m,
        lower_vertical_displacement_m,
        upper_lateral_displacement_m,
        upper_vertical_displacement_m,
        upper_level_blend,
        wall_index,
        center_frame,
        meters_per_frame,
        geometry,
    ))
}

/// Traces one certified global contact through two blended levels.
///
/// The function returns an ambiguity error if it cannot certify height order.
#[allow(clippy::too_many_arguments)]
pub fn trace_spherical_45_45_wall_blended_uniform_contacts(
    lower_lateral_displacement_m: &[f32],
    lower_vertical_displacement_m: &[f32],
    upper_lateral_displacement_m: &[f32],
    upper_vertical_displacement_m: &[f32],
    upper_level_blend: f64,
    wall_index: usize,
    center_frame: f64,
    meters_per_frame: f64,
    geometry: StylusGeometry,
) -> Result<StylusTraceContactSet, StylusTraceError> {
    let sample_count = lower_lateral_displacement_m.len();
    if sample_count != lower_vertical_displacement_m.len()
        || sample_count != upper_lateral_displacement_m.len()
        || sample_count != upper_vertical_displacement_m.len()
    {
        return Err(StylusTraceError::ChannelLengthMismatch);
    }
    if sample_count < 4 {
        return Err(StylusTraceError::InsufficientSamples);
    }
    if !upper_level_blend.is_finite() || !(0.0..=1.0).contains(&upper_level_blend) {
        return Err(StylusTraceError::InvalidLevelBlend);
    }
    if upper_level_blend == 0.0 {
        return trace_spherical_45_45_wall_uniform_contacts(
            lower_lateral_displacement_m,
            lower_vertical_displacement_m,
            wall_index,
            center_frame,
            meters_per_frame,
            geometry,
        );
    }
    if upper_level_blend == 1.0 {
        return trace_spherical_45_45_wall_uniform_contacts(
            upper_lateral_displacement_m,
            upper_vertical_displacement_m,
            wall_index,
            center_frame,
            meters_per_frame,
            geometry,
        );
    }
    let (lateral_sign, vertical_sign) = match wall_index {
        0 => (
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        1 => (
            -std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        _ => return Err(StylusTraceError::InvalidWallIndex),
    };
    trace_spherical_piecewise_contacts(
        sample_count,
        center_frame,
        meters_per_frame,
        geometry,
        |segment_start| {
            let lower_wall = catmull_rom_combined_cubic(
                lower_lateral_displacement_m,
                lateral_sign,
                lower_vertical_displacement_m,
                vertical_sign,
                segment_start,
            );
            let upper_wall = catmull_rom_combined_cubic(
                upper_lateral_displacement_m,
                lateral_sign,
                upper_vertical_displacement_m,
                vertical_sign,
                segment_start,
            );
            lower_wall
                .scale(1.0 - upper_level_blend)
                .add(upper_wall.scale(upper_level_blend))
        },
    )
}

/// Traces one wall through two globally aligned multiresolution levels.
#[allow(clippy::too_many_arguments)]
pub fn trace_spherical_45_45_wall_multiresolution(
    lower_lateral_displacement_m: &[f32],
    lower_vertical_displacement_m: &[f32],
    lower_first_source_frame: u64,
    lower_source_frame_step: u32,
    upper_lateral_displacement_m: &[f32],
    upper_vertical_displacement_m: &[f32],
    upper_first_source_frame: u64,
    upper_source_frame_step: u32,
    upper_level_blend: f64,
    wall_index: usize,
    absolute_center_frame: f64,
    meters_per_source_frame: f64,
    geometry: StylusGeometry,
) -> Result<StylusTraceSample, StylusTraceError> {
    single_sample_from_contacts(trace_spherical_45_45_wall_multiresolution_contacts(
        lower_lateral_displacement_m,
        lower_vertical_displacement_m,
        lower_first_source_frame,
        lower_source_frame_step,
        upper_lateral_displacement_m,
        upper_vertical_displacement_m,
        upper_first_source_frame,
        upper_source_frame_step,
        upper_level_blend,
        wall_index,
        absolute_center_frame,
        meters_per_source_frame,
        geometry,
    ))
}

/// Traces one certified global contact through aligned spatial levels.
///
/// The function returns an ambiguity error if it cannot certify height order.
#[allow(clippy::too_many_arguments)]
pub fn trace_spherical_45_45_wall_multiresolution_contacts(
    lower_lateral_displacement_m: &[f32],
    lower_vertical_displacement_m: &[f32],
    lower_first_source_frame: u64,
    lower_source_frame_step: u32,
    upper_lateral_displacement_m: &[f32],
    upper_vertical_displacement_m: &[f32],
    upper_first_source_frame: u64,
    upper_source_frame_step: u32,
    upper_level_blend: f64,
    wall_index: usize,
    absolute_center_frame: f64,
    meters_per_source_frame: f64,
    geometry: StylusGeometry,
) -> Result<StylusTraceContactSet, StylusTraceError> {
    if lower_lateral_displacement_m.len() != lower_vertical_displacement_m.len()
        || upper_lateral_displacement_m.len() != upper_vertical_displacement_m.len()
    {
        return Err(StylusTraceError::ChannelLengthMismatch);
    }
    if lower_lateral_displacement_m.len() < 4 || upper_lateral_displacement_m.len() < 4 {
        return Err(StylusTraceError::InsufficientSamples);
    }
    if lower_source_frame_step == 0 || upper_source_frame_step == 0 {
        return Err(StylusTraceError::InvalidLevelCoordinates);
    }
    if !upper_level_blend.is_finite() || !(0.0..=1.0).contains(&upper_level_blend) {
        return Err(StylusTraceError::InvalidLevelBlend);
    }
    if !absolute_center_frame.is_finite() {
        return Err(StylusTraceError::InvalidCenterFrame);
    }
    let (lateral_sign, vertical_sign) = wall_normal(wall_index)?;
    let lower_last_source_frame = level_last_source_frame(
        lower_first_source_frame,
        lower_source_frame_step,
        lower_lateral_displacement_m.len(),
    )?;
    let upper_last_source_frame = level_last_source_frame(
        upper_first_source_frame,
        upper_source_frame_step,
        upper_lateral_displacement_m.len(),
    )?;
    if upper_level_blend == 0.0 {
        return trace_spherical_45_45_wall_level_contacts(
            lower_lateral_displacement_m,
            lower_vertical_displacement_m,
            lower_first_source_frame,
            lower_source_frame_step,
            lower_last_source_frame,
            lateral_sign,
            vertical_sign,
            absolute_center_frame,
            meters_per_source_frame,
            geometry,
        );
    }
    if upper_level_blend == 1.0 {
        return trace_spherical_45_45_wall_level_contacts(
            upper_lateral_displacement_m,
            upper_vertical_displacement_m,
            upper_first_source_frame,
            upper_source_frame_step,
            upper_last_source_frame,
            lateral_sign,
            vertical_sign,
            absolute_center_frame,
            meters_per_source_frame,
            geometry,
        );
    }
    let domain_first_source_frame = lower_first_source_frame.min(upper_first_source_frame);
    let domain_last_source_frame = lower_last_source_frame.max(upper_last_source_frame);
    let domain_sample_count = usize::try_from(
        domain_last_source_frame
            .saturating_sub(domain_first_source_frame)
            .saturating_add(1),
    )
    .unwrap_or(usize::MAX);
    let local_center_frame = absolute_center_frame - domain_first_source_frame as f64;
    trace_spherical_piecewise_contacts(
        domain_sample_count,
        local_center_frame,
        meters_per_source_frame,
        geometry,
        |local_segment_start| {
            let absolute_segment_start = domain_first_source_frame as f64 + local_segment_start;
            let lower_wall = level_combined_cubic_for_source_segment(
                lower_lateral_displacement_m,
                lateral_sign,
                lower_vertical_displacement_m,
                vertical_sign,
                lower_first_source_frame,
                lower_source_frame_step,
                absolute_segment_start,
            );
            let upper_wall = level_combined_cubic_for_source_segment(
                upper_lateral_displacement_m,
                lateral_sign,
                upper_vertical_displacement_m,
                vertical_sign,
                upper_first_source_frame,
                upper_source_frame_step,
                absolute_segment_start,
            );
            lower_wall
                .scale(1.0 - upper_level_blend)
                .add(upper_wall.scale(upper_level_blend))
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn trace_spherical_45_45_wall_level_contacts(
    lateral_displacement_m: &[f32],
    vertical_displacement_m: &[f32],
    first_source_frame: u64,
    source_frame_step: u32,
    last_source_frame: u64,
    lateral_sign: f64,
    vertical_sign: f64,
    absolute_center_frame: f64,
    meters_per_source_frame: f64,
    geometry: StylusGeometry,
) -> Result<StylusTraceContactSet, StylusTraceError> {
    let domain_sample_count = usize::try_from(
        last_source_frame
            .saturating_sub(first_source_frame)
            .saturating_add(1),
    )
    .unwrap_or(usize::MAX);
    let local_center_frame = absolute_center_frame - first_source_frame as f64;
    trace_spherical_piecewise_contacts(
        domain_sample_count,
        local_center_frame,
        meters_per_source_frame,
        geometry,
        |local_segment_start| {
            level_combined_cubic_for_source_segment(
                lateral_displacement_m,
                lateral_sign,
                vertical_displacement_m,
                vertical_sign,
                first_source_frame,
                source_frame_step,
                first_source_frame as f64 + local_segment_start,
            )
        },
    )
}

fn wall_normal(wall_index: usize) -> Result<(f64, f64), StylusTraceError> {
    match wall_index {
        0 => Ok((
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        )),
        1 => Ok((
            -std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        )),
        _ => Err(StylusTraceError::InvalidWallIndex),
    }
}

fn level_last_source_frame(
    first_source_frame: u64,
    source_frame_step: u32,
    sample_count: usize,
) -> Result<u64, StylusTraceError> {
    let final_index = u64::try_from(sample_count.saturating_sub(1))
        .map_err(|_| StylusTraceError::InvalidLevelCoordinates)?;
    let source_span = final_index
        .checked_mul(u64::from(source_frame_step))
        .ok_or(StylusTraceError::InvalidLevelCoordinates)?;
    first_source_frame
        .checked_add(source_span)
        .ok_or(StylusTraceError::InvalidLevelCoordinates)
}

#[derive(Debug, Clone, Copy)]
struct Cubic {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

impl Cubic {
    fn constant(value: f64) -> Self {
        Self {
            a: 0.0,
            b: 0.0,
            c: 0.0,
            d: value,
        }
    }

    fn add(self, other: Self) -> Self {
        Self {
            a: self.a + other.a,
            b: self.b + other.b,
            c: self.c + other.c,
            d: self.d + other.d,
        }
    }

    fn scale(self, scale: f64) -> Self {
        Self {
            a: self.a * scale,
            b: self.b * scale,
            c: self.c * scale,
            d: self.d * scale,
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

    fn value_bounds(self, fraction: OutwardInterval) -> OutwardInterval {
        OutwardInterval::point(self.a)
            .multiply(fraction)
            .add(OutwardInterval::point(self.b))
            .multiply(fraction)
            .add(OutwardInterval::point(self.c))
            .multiply(fraction)
            .add(OutwardInterval::point(self.d))
    }

    fn slope_bounds(self, fraction: OutwardInterval) -> OutwardInterval {
        OutwardInterval::point(self.a)
            .multiply(OutwardInterval::point(3.0))
            .multiply(fraction)
            .add(OutwardInterval::point(self.b).multiply(OutwardInterval::point(2.0)))
            .multiply(fraction)
            .add(OutwardInterval::point(self.c))
    }

    fn is_finite(self) -> bool {
        self.a.is_finite() && self.b.is_finite() && self.c.is_finite() && self.d.is_finite()
    }
}

#[derive(Debug, Clone, Copy)]
struct OutwardInterval {
    lower: f64,
    upper: f64,
}

impl OutwardInterval {
    const ZERO: Self = Self {
        lower: 0.0,
        upper: 0.0,
    };

    fn point(value: f64) -> Self {
        Self {
            lower: value,
            upper: value,
        }
    }

    fn ordered(lower: f64, upper: f64) -> Self {
        Self { lower, upper }
    }

    fn add(self, other: Self) -> Self {
        Self {
            lower: outward_lower(self.lower + other.lower),
            upper: outward_upper(self.upper + other.upper),
        }
    }

    fn subtract(self, other: Self) -> Self {
        Self {
            lower: outward_lower(self.lower - other.upper),
            upper: outward_upper(self.upper - other.lower),
        }
    }

    fn multiply(self, other: Self) -> Self {
        let products = [
            self.lower * other.lower,
            self.lower * other.upper,
            self.upper * other.lower,
            self.upper * other.upper,
        ];
        let lower = products.into_iter().fold(f64::INFINITY, f64::min);
        let upper = products.into_iter().fold(f64::NEG_INFINITY, f64::max);
        Self {
            lower: outward_lower(lower),
            upper: outward_upper(upper),
        }
    }

    fn divide_by_positive(self, other: Self) -> Self {
        if !(other.lower > 0.0) {
            return Self::ordered(f64::NEG_INFINITY, f64::INFINITY);
        }
        let reciprocal = Self {
            lower: outward_lower(1.0 / other.upper),
            upper: outward_upper(1.0 / other.lower),
        };
        self.multiply(reciprocal)
    }

    fn divide_by_nonzero(self, other: Self) -> Option<Self> {
        if other.contains_zero() {
            return None;
        }
        let first = 1.0 / other.lower;
        let second = 1.0 / other.upper;
        let reciprocal = Self {
            lower: outward_lower(first.min(second)),
            upper: outward_upper(first.max(second)),
        };
        Some(self.multiply(reciprocal))
    }

    fn square(self) -> Self {
        let lower_square = self.lower * self.lower;
        let upper_square = self.upper * self.upper;
        let lower = if self.contains_zero() {
            0.0
        } else {
            lower_square.min(upper_square)
        };
        let upper = lower_square.max(upper_square);
        Self {
            lower: if lower == 0.0 {
                0.0
            } else {
                outward_lower(lower)
            },
            upper: outward_upper(upper),
        }
    }

    fn sqrt_nonnegative(self) -> Self {
        Self {
            lower: outward_lower(self.lower.max(0.0).sqrt()).max(0.0),
            upper: outward_upper(self.upper.max(0.0).sqrt()),
        }
    }

    fn clamp(self, lower: f64, upper: f64) -> Self {
        Self {
            lower: self.lower.max(lower),
            upper: self.upper.min(upper),
        }
    }

    fn midpoint(self) -> f64 {
        self.lower + (self.upper - self.lower) * 0.5
    }

    fn intersection(self, other: Self) -> Option<Self> {
        let lower = self.lower.max(other.lower);
        let upper = self.upper.min(other.upper);
        (lower <= upper).then_some(Self { lower, upper })
    }

    fn contains_zero(self) -> bool {
        self.lower <= 0.0 && self.upper >= 0.0
    }
}

fn outward_lower(value: f64) -> f64 {
    if value.is_finite() {
        value.next_down()
    } else {
        value
    }
}

fn outward_upper(value: f64) -> f64 {
    if value.is_finite() {
        value.next_up()
    } else {
        value
    }
}

#[derive(Debug, Clone, Copy)]
struct TraceCandidate {
    segment_start: f64,
    fraction: OutwardInterval,
    cubic: Cubic,
}

#[derive(Debug, Clone, Copy)]
struct BestCandidate {
    candidate: TraceCandidate,
    fraction: f64,
    contact_offset_m: f64,
    height_m: f64,
}

#[derive(Debug, Clone, Copy)]
struct CertifiedContender {
    segment_start: f64,
    height: OutwardInterval,
    offset_m: OutwardInterval,
    groove_slope: OutwardInterval,
    tangent_residual: OutwardInterval,
    contact: StylusTraceContact,
}

impl CertifiedContender {
    const ZERO: Self = Self {
        segment_start: 0.0,
        height: OutwardInterval::ZERO,
        offset_m: OutwardInterval::ZERO,
        groove_slope: OutwardInterval::ZERO,
        tangent_residual: OutwardInterval::ZERO,
        contact: StylusTraceContact {
            contact_offset_m: 0.0,
            groove_displacement_m: 0.0,
            groove_slope: 0.0,
            tangent_residual: 0.0,
        },
    };
}

#[derive(Debug, Clone, Copy)]
struct ContenderSet {
    entries: [CertifiedContender; MAX_GLOBAL_CONTENDERS],
    len: usize,
    omitted_height_upper: f64,
}

impl ContenderSet {
    fn new() -> Self {
        Self {
            entries: [CertifiedContender::ZERO; MAX_GLOBAL_CONTENDERS],
            len: 0,
            omitted_height_upper: f64::NEG_INFINITY,
        }
    }

    fn insert(&mut self, contender: CertifiedContender) {
        if self.len < self.entries.len() {
            self.entries[self.len] = contender;
            self.len += 1;
            return;
        }
        let mut lowest_index = 0;
        for index in 1..self.len {
            if self.entries[index].height.upper < self.entries[lowest_index].height.upper {
                lowest_index = index;
            }
        }
        if contender.height.upper > self.entries[lowest_index].height.upper {
            self.omitted_height_upper = self
                .omitted_height_upper
                .max(self.entries[lowest_index].height.upper);
            self.entries[lowest_index] = contender;
        } else {
            self.omitted_height_upper = self.omitted_height_upper.max(contender.height.upper);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RootBrackets {
    brackets: [OutwardInterval; MAX_ROOT_BRACKETS_PER_PIECE],
    len: usize,
    searched_nodes: usize,
}

impl RootBrackets {
    fn new() -> Self {
        Self {
            brackets: [OutwardInterval::ZERO; MAX_ROOT_BRACKETS_PER_PIECE],
            len: 0,
            searched_nodes: 0,
        }
    }

    fn push_or_merge(&mut self, bracket: OutwardInterval) -> Result<(), StylusTraceError> {
        if self.len > 0 && bracket.lower <= self.brackets[self.len - 1].upper {
            self.brackets[self.len - 1].upper =
                self.brackets[self.len - 1].upper.max(bracket.upper);
            return Ok(());
        }
        if self.len == self.brackets.len() {
            return Err(StylusTraceError::StationaryContactNotIsolated);
        }
        self.brackets[self.len] = bracket;
        self.len += 1;
        Ok(())
    }
}

fn trace_spherical_piecewise_contacts<F>(
    sample_count: usize,
    center_frame: f64,
    meters_per_frame: f64,
    geometry: StylusGeometry,
    cubic_for_segment: F,
) -> Result<StylusTraceContactSet, StylusTraceError>
where
    F: Fn(f64) -> Cubic,
{
    let geometry = geometry.validate()?;
    if sample_count < 4 {
        return Err(StylusTraceError::InsufficientSamples);
    }
    if !center_frame.is_finite() {
        return Err(StylusTraceError::InvalidCenterFrame);
    }
    if !meters_per_frame.is_finite() || meters_per_frame < MIN_METERS_PER_FRAME {
        return Err(StylusTraceError::InvalidSpatialStep);
    }

    let radius = geometry.tracing_radius_m;
    let half_span_frames = (radius / meters_per_frame).ceil() as usize;
    if half_span_frames > MAX_SEARCH_SAMPLES {
        return Err(StylusTraceError::SearchLimitExceeded);
    }

    let mut best: Option<BestCandidate> = None;
    let mut contenders = ContenderSet::new();
    let mut process_candidate = |candidate: TraceCandidate| {
        let fraction = candidate.fraction.midpoint();
        let contact_offset_m = candidate_offset_m(
            candidate.segment_start,
            fraction,
            center_frame,
            meters_per_frame,
            radius,
        );
        let height_m = candidate.cubic.value(fraction) + circle_height(radius, contact_offset_m);
        if !height_m.is_finite() {
            return Err(StylusTraceError::InvalidGrooveDisplacement);
        }
        let contender = certified_contender(candidate, center_frame, meters_per_frame, radius);
        if !contender.height.lower.is_finite() || !contender.height.upper.is_finite() {
            return Err(StylusTraceError::InvalidGrooveDisplacement);
        }
        contenders.insert(contender);
        let replace = best.is_none_or(|current| {
            height_m > current.height_m
                || (height_m == current.height_m && contact_offset_m < current.contact_offset_m)
        });
        if replace {
            best = Some(BestCandidate {
                candidate,
                fraction,
                contact_offset_m,
                height_m,
            });
        }
        Ok(())
    };
    let center_segment = center_frame.floor();
    let center_cubic = cubic_for_segment(center_segment);
    if !center_cubic.is_finite() {
        return Err(StylusTraceError::InvalidGrooveDisplacement);
    }
    process_candidate(TraceCandidate {
        segment_start: center_segment,
        fraction: OutwardInterval::point(center_frame - center_segment),
        cubic: center_cubic,
    })?;
    for_each_trace_candidate(
        center_frame,
        meters_per_frame,
        radius,
        &cubic_for_segment,
        &mut process_candidate,
    )?;
    let best = best.ok_or(StylusTraceError::StationaryContactNotIsolated)?;
    let selected_height_bounds = candidate_height_bounds(
        TraceCandidate {
            fraction: OutwardInterval::point(best.fraction),
            ..best.candidate
        },
        center_frame,
        meters_per_frame,
        radius,
    );
    if !selected_height_bounds.lower.is_finite() || !selected_height_bounds.upper.is_finite() {
        return Err(StylusTraceError::InvalidGrooveDisplacement);
    }

    if contenders.omitted_height_upper >= selected_height_bounds.lower {
        return Err(StylusTraceError::ContactCapacityExceeded);
    }
    let mut global_height_upper = selected_height_bounds.upper;
    let mut selected = [CertifiedContender::ZERO; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL];
    let mut selected_len = 0;
    for contender in contenders.entries[..contenders.len].iter().copied() {
        global_height_upper = global_height_upper.max(contender.height.upper);
        if contender.height.upper < selected_height_bounds.lower {
            continue;
        }
        if interval_error_from_value(contender.offset_m, contender.contact.contact_offset_m)
            > SPHERICAL_TRACE_CONTACT_POSITION_ERROR_BOUND_M
        {
            return Err(StylusTraceError::StationaryContactNotIsolated);
        }
        if interval_error_from_value(contender.groove_slope, contender.contact.groove_slope)
            > SPHERICAL_TRACE_GROOVE_SLOPE_ERROR_BOUND
        {
            return Err(StylusTraceError::GrooveSlopeBoundNotMet);
        }
        if interval_error_from_value(
            contender.tangent_residual,
            contender.contact.tangent_residual,
        ) > SPHERICAL_TRACE_TANGENT_RESIDUAL_ERROR_BOUND
        {
            return Err(StylusTraceError::TangentResidualBoundNotMet);
        }
        insert_global_contact(
            &mut selected,
            &mut selected_len,
            contender,
            center_frame,
            meters_per_frame,
            radius,
        )?;
        if selected_len > 1 {
            return Err(StylusTraceError::ContactHeightOrderNotIsolated);
        }
    }
    if global_height_upper - selected_height_bounds.lower > SPHERICAL_TRACE_HEIGHT_ERROR_BOUND_M {
        return Err(StylusTraceError::EnvelopeHeightBoundNotMet);
    }
    if selected_len == 0 {
        return Err(StylusTraceError::StationaryContactNotIsolated);
    }
    let mut contacts = [StylusTraceContact::default(); MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL];
    for index in 0..selected_len {
        contacts[index] = selected[index].contact;
    }
    Ok(StylusTraceContactSet {
        center_displacement_m: best.height_m - radius,
        contact_count: selected_len as u8,
        contacts,
    })
}

fn insert_global_contact(
    selected: &mut [CertifiedContender; MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL],
    selected_len: &mut usize,
    contender: CertifiedContender,
    center_frame: f64,
    meters_per_frame: f64,
    radius: f64,
) -> Result<(), StylusTraceError> {
    for existing in &mut selected[..*selected_len] {
        if existing.offset_m.intersection(contender.offset_m).is_none() {
            continue;
        }
        if !same_stationary_contact(*existing, contender, center_frame, meters_per_frame, radius) {
            return Err(StylusTraceError::ContactOrderNotIsolated);
        }
        let existing_height = existing.contact.groove_displacement_m
            + circle_height(radius, existing.contact.contact_offset_m);
        let contender_height = contender.contact.groove_displacement_m
            + circle_height(radius, contender.contact.contact_offset_m);
        if contender_height > existing_height {
            existing.contact = contender.contact;
            existing.segment_start = contender.segment_start;
        }
        existing.height.lower = existing.height.lower.min(contender.height.lower);
        existing.height.upper = existing.height.upper.max(contender.height.upper);
        existing.offset_m.lower = existing.offset_m.lower.min(contender.offset_m.lower);
        existing.offset_m.upper = existing.offset_m.upper.max(contender.offset_m.upper);
        existing.groove_slope.lower = existing
            .groove_slope
            .lower
            .min(contender.groove_slope.lower);
        existing.groove_slope.upper = existing
            .groove_slope
            .upper
            .max(contender.groove_slope.upper);
        existing.tangent_residual.lower = existing
            .tangent_residual
            .lower
            .min(contender.tangent_residual.lower);
        existing.tangent_residual.upper = existing
            .tangent_residual
            .upper
            .max(contender.tangent_residual.upper);
        return Ok(());
    }
    if *selected_len == selected.len() {
        return Err(StylusTraceError::ContactCapacityExceeded);
    }
    selected[*selected_len] = contender;
    *selected_len += 1;
    let mut index = *selected_len - 1;
    while index > 0
        && selected[index].contact.contact_offset_m < selected[index - 1].contact.contact_offset_m
    {
        selected.swap(index, index - 1);
        index -= 1;
    }
    if index > 0 && selected[index - 1].offset_m.upper >= selected[index].offset_m.lower {
        return Err(StylusTraceError::ContactOrderNotIsolated);
    }
    if index + 1 < *selected_len
        && selected[index].offset_m.upper >= selected[index + 1].offset_m.lower
    {
        return Err(StylusTraceError::ContactOrderNotIsolated);
    }
    Ok(())
}

fn same_stationary_contact(
    first: CertifiedContender,
    second: CertifiedContender,
    center_frame: f64,
    meters_per_frame: f64,
    radius: f64,
) -> bool {
    if first.segment_start == second.segment_start {
        return true;
    }
    if (first.segment_start - second.segment_start).abs() != 1.0 {
        return false;
    }
    let boundary_frame = first.segment_start.max(second.segment_start);
    let boundary_offset =
        ((boundary_frame - center_frame) * meters_per_frame).clamp(-radius, radius);
    first.offset_m.lower <= boundary_offset
        && boundary_offset <= first.offset_m.upper
        && second.offset_m.lower <= boundary_offset
        && boundary_offset <= second.offset_m.upper
}

fn for_each_trace_candidate<F, V>(
    center_frame: f64,
    meters_per_frame: f64,
    radius: f64,
    cubic_for_segment: &F,
    mut visit: V,
) -> Result<(), StylusTraceError>
where
    F: Fn(f64) -> Cubic,
    V: FnMut(TraceCandidate) -> Result<(), StylusTraceError>,
{
    let radius_frames = radius / meters_per_frame;
    let search_lower = center_frame - radius_frames;
    let search_upper = center_frame + radius_frames;
    if !search_lower.is_finite() || !search_upper.is_finite() {
        return Err(StylusTraceError::InvalidCenterFrame);
    }
    let mut segment_start = search_lower.floor();
    let lower_cubic = cubic_for_segment(segment_start);
    if !lower_cubic.is_finite() {
        return Err(StylusTraceError::InvalidGrooveDisplacement);
    }
    visit(TraceCandidate {
        segment_start,
        fraction: OutwardInterval::point(search_lower - segment_start),
        cubic: lower_cubic,
    })?;
    for piece_index in 0..MAX_TRACE_PIECES {
        let fraction_lower = (search_lower - segment_start).clamp(0.0, 1.0);
        let fraction_upper = (search_upper - segment_start).clamp(0.0, 1.0);
        if fraction_upper > fraction_lower {
            let cubic = cubic_for_segment(segment_start);
            if !cubic.is_finite() {
                return Err(StylusTraceError::InvalidGrooveDisplacement);
            }
            let roots = isolate_stationary_contacts(
                cubic,
                segment_start,
                center_frame,
                meters_per_frame,
                radius,
                OutwardInterval::ordered(fraction_lower, fraction_upper),
            )?;
            for root_index in 0..roots.len {
                visit(TraceCandidate {
                    segment_start,
                    fraction: roots.brackets[root_index],
                    cubic,
                })?;
            }
        }
        let next_segment = segment_start + 1.0;
        if next_segment >= search_upper {
            let upper_segment = search_upper.floor();
            let upper_cubic = cubic_for_segment(upper_segment);
            if !upper_cubic.is_finite() {
                return Err(StylusTraceError::InvalidGrooveDisplacement);
            }
            visit(TraceCandidate {
                segment_start: upper_segment,
                fraction: OutwardInterval::point(search_upper - upper_segment),
                cubic: upper_cubic,
            })?;
            return Ok(());
        }
        if next_segment == segment_start {
            return Err(StylusTraceError::InvalidCenterFrame);
        }
        segment_start = next_segment;
        if piece_index + 1 == MAX_TRACE_PIECES {
            return Err(StylusTraceError::SearchLimitExceeded);
        }
    }
    Err(StylusTraceError::SearchLimitExceeded)
}

fn isolate_stationary_contacts(
    cubic: Cubic,
    segment_start: f64,
    center_frame: f64,
    meters_per_frame: f64,
    radius: f64,
    domain: OutwardInterval,
) -> Result<RootBrackets, StylusTraceError> {
    let mut roots = RootBrackets::new();
    let mut stack = [OutwardInterval::ZERO; ROOT_SEARCH_STACK_CAPACITY];
    stack[0] = domain;
    let mut stack_len = 1;
    while stack_len > 0 {
        stack_len -= 1;
        let mut interval = stack[stack_len];
        roots.searched_nodes += 1;
        if roots.searched_nodes > MAX_ROOT_SEARCH_NODES_PER_PIECE {
            return Err(StylusTraceError::StationaryContactNotIsolated);
        }
        let stationary = stationary_function_bounds(
            cubic,
            segment_start,
            interval,
            center_frame,
            meters_per_frame,
            radius,
        );
        if !stationary.contains_zero() {
            continue;
        }
        let midpoint = interval.midpoint();
        let stationary_derivative = stationary_derivative_bounds(
            cubic,
            segment_start,
            interval,
            center_frame,
            meters_per_frame,
            radius,
        );
        if stationary_derivative.lower.is_finite() && stationary_derivative.upper.is_finite() {
            if let Some(quotient) = stationary_function_bounds(
                cubic,
                segment_start,
                OutwardInterval::point(midpoint),
                center_frame,
                meters_per_frame,
                radius,
            )
            .divide_by_nonzero(stationary_derivative)
            {
                let newton = OutwardInterval::point(midpoint).subtract(quotient);
                let Some(contracted) = interval.intersection(newton) else {
                    continue;
                };
                if contracted.upper - contracted.lower < (interval.upper - interval.lower) * 0.875 {
                    interval = contracted;
                }
            }
        }
        let physical_width = (interval.upper - interval.lower) * meters_per_frame;
        let midpoint = interval.midpoint();
        if physical_width <= ROOT_BRACKET_WIDTH_M
            || midpoint == interval.lower
            || midpoint == interval.upper
        {
            roots.push_or_merge(interval)?;
            continue;
        }
        if stack_len + 2 > stack.len() {
            return Err(StylusTraceError::StationaryContactNotIsolated);
        }
        stack[stack_len] = OutwardInterval::ordered(midpoint, interval.upper);
        stack[stack_len + 1] = OutwardInterval::ordered(interval.lower, midpoint);
        stack_len += 2;
    }
    Ok(roots)
}

fn stationary_function_bounds(
    cubic: Cubic,
    segment_start: f64,
    fraction: OutwardInterval,
    center_frame: f64,
    meters_per_frame: f64,
    radius: f64,
) -> OutwardInterval {
    let groove_slope = cubic.slope_bounds(fraction);
    let offset = candidate_offset_bounds(
        segment_start,
        fraction,
        center_frame,
        meters_per_frame,
        radius,
    );
    let circle_slope = circle_slope_per_frame_bounds(offset, meters_per_frame, radius);
    groove_slope.subtract(circle_slope)
}

fn stationary_derivative_bounds(
    cubic: Cubic,
    segment_start: f64,
    fraction: OutwardInterval,
    center_frame: f64,
    meters_per_frame: f64,
    radius: f64,
) -> OutwardInterval {
    let groove_curvature = OutwardInterval::point(cubic.a)
        .multiply(OutwardInterval::point(6.0))
        .multiply(fraction)
        .add(OutwardInterval::point(cubic.b).multiply(OutwardInterval::point(2.0)));
    let offset = candidate_offset_bounds(
        segment_start,
        fraction,
        center_frame,
        meters_per_frame,
        radius,
    );
    groove_curvature.subtract(circle_curvature_per_frame_squared_bounds(
        offset,
        meters_per_frame,
        radius,
    ))
}

fn circle_curvature_per_frame_squared_bounds(
    offset_m: OutwardInterval,
    meters_per_frame: f64,
    radius: f64,
) -> OutwardInterval {
    let maximum_absolute_offset = offset_m.lower.abs().max(offset_m.upper.abs()).min(radius);
    let minimum_absolute_offset = if offset_m.lower <= 0.0 && offset_m.upper >= 0.0 {
        0.0
    } else {
        offset_m.lower.abs().min(offset_m.upper.abs()).min(radius)
    };
    let curvature_at = |absolute_offset_m: f64| -> OutwardInterval {
        if absolute_offset_m >= radius {
            return OutwardInterval::ordered(0.0, f64::INFINITY);
        }
        let meters_per_frame = OutwardInterval::point(meters_per_frame);
        let numerator = meters_per_frame
            .multiply(meters_per_frame)
            .multiply(OutwardInterval::point(radius).multiply(OutwardInterval::point(radius)));
        let height = circle_height_point_bounds(radius, absolute_offset_m);
        let height_cubed = height.multiply(height).multiply(height);
        numerator.divide_by_positive(height_cubed)
    };
    let lower = curvature_at(minimum_absolute_offset);
    let upper = curvature_at(maximum_absolute_offset);
    OutwardInterval::ordered(lower.lower, upper.upper)
}

fn circle_slope_per_frame_bounds(
    offset_m: OutwardInterval,
    meters_per_frame: f64,
    radius: f64,
) -> OutwardInterval {
    let slope_at = |offset_m: f64| -> OutwardInterval {
        if offset_m <= -radius {
            OutwardInterval::ordered(f64::NEG_INFINITY, f64::NEG_INFINITY)
        } else if offset_m >= radius {
            OutwardInterval::ordered(f64::INFINITY, f64::INFINITY)
        } else {
            OutwardInterval::point(meters_per_frame)
                .multiply(OutwardInterval::point(offset_m))
                .divide_by_positive(circle_height_point_bounds(radius, offset_m))
        }
    };
    let lower = slope_at(offset_m.lower);
    let upper = slope_at(offset_m.upper);
    OutwardInterval::ordered(lower.lower, upper.upper)
}

fn candidate_height_bounds(
    candidate: TraceCandidate,
    center_frame: f64,
    meters_per_frame: f64,
    radius: f64,
) -> OutwardInterval {
    let groove = candidate.cubic.value_bounds(candidate.fraction);
    let offset = candidate_offset_bounds(
        candidate.segment_start,
        candidate.fraction,
        center_frame,
        meters_per_frame,
        radius,
    );
    groove.add(circle_height_bounds(radius, offset))
}

fn certified_contender(
    candidate: TraceCandidate,
    center_frame: f64,
    meters_per_frame: f64,
    radius: f64,
) -> CertifiedContender {
    let offset_m = candidate_offset_bounds(
        candidate.segment_start,
        candidate.fraction,
        center_frame,
        meters_per_frame,
        radius,
    );
    let groove_slope = candidate
        .cubic
        .slope_bounds(candidate.fraction)
        .divide_by_positive(OutwardInterval::point(meters_per_frame));
    let circle_slope = circle_slope_bounds(offset_m, radius);
    let fraction = candidate.fraction.midpoint();
    let contact_offset_m = candidate_offset_m(
        candidate.segment_start,
        fraction,
        center_frame,
        meters_per_frame,
        radius,
    );
    let groove_displacement_m = candidate.cubic.value(fraction);
    let point_groove_slope = candidate.cubic.slope_per_frame(fraction) / meters_per_frame;
    let circle_height = circle_height(radius, contact_offset_m);
    let point_circle_slope = if circle_height > f64::EPSILON {
        contact_offset_m / circle_height
    } else {
        contact_offset_m.signum() * f64::INFINITY
    };
    CertifiedContender {
        segment_start: candidate.segment_start,
        height: candidate_height_bounds(candidate, center_frame, meters_per_frame, radius),
        offset_m,
        groove_slope,
        tangent_residual: groove_slope.subtract(circle_slope),
        contact: StylusTraceContact {
            contact_offset_m,
            groove_displacement_m,
            groove_slope: point_groove_slope,
            tangent_residual: point_groove_slope - point_circle_slope,
        },
    }
}

fn interval_error_from_value(interval: OutwardInterval, value: f64) -> f64 {
    (value - interval.lower)
        .abs()
        .max((interval.upper - value).abs())
}

fn candidate_offset_bounds(
    segment_start: f64,
    fraction: OutwardInterval,
    center_frame: f64,
    meters_per_frame: f64,
    radius: f64,
) -> OutwardInterval {
    OutwardInterval::point(segment_start)
        .add(fraction)
        .subtract(OutwardInterval::point(center_frame))
        .multiply(OutwardInterval::point(meters_per_frame))
        .clamp(-radius, radius)
}

fn candidate_offset_m(
    segment_start: f64,
    fraction: f64,
    center_frame: f64,
    meters_per_frame: f64,
    radius: f64,
) -> f64 {
    ((segment_start + fraction - center_frame) * meters_per_frame).clamp(-radius, radius)
}

fn circle_height(radius: f64, offset_m: f64) -> f64 {
    (radius * radius - offset_m * offset_m).max(0.0).sqrt()
}

fn circle_slope_bounds(offset_m: OutwardInterval, radius: f64) -> OutwardInterval {
    let slope_at = |offset_m: f64| -> OutwardInterval {
        if offset_m <= -radius {
            OutwardInterval::ordered(f64::NEG_INFINITY, f64::NEG_INFINITY)
        } else if offset_m >= radius {
            OutwardInterval::ordered(f64::INFINITY, f64::INFINITY)
        } else {
            OutwardInterval::point(offset_m)
                .divide_by_positive(circle_height_point_bounds(radius, offset_m))
        }
    };
    let lower = slope_at(offset_m.lower);
    let upper = slope_at(offset_m.upper);
    OutwardInterval::ordered(lower.lower, upper.upper)
}

fn circle_height_bounds(radius: f64, offset_m: OutwardInterval) -> OutwardInterval {
    let maximum_absolute_offset = offset_m.lower.abs().max(offset_m.upper.abs()).min(radius);
    let minimum_absolute_offset = if offset_m.lower <= 0.0 && offset_m.upper >= 0.0 {
        0.0
    } else {
        offset_m.lower.abs().min(offset_m.upper.abs()).min(radius)
    };
    let lower = circle_height_point_bounds(radius, maximum_absolute_offset);
    let upper = circle_height_point_bounds(radius, minimum_absolute_offset);
    OutwardInterval::ordered(lower.lower, upper.upper)
}

fn circle_height_point_bounds(radius: f64, offset_m: f64) -> OutwardInterval {
    let radius_squared = OutwardInterval::point(radius).square();
    let offset_squared = OutwardInterval::point(offset_m).square();
    radius_squared
        .subtract(offset_squared)
        .clamp(0.0, f64::INFINITY)
        .sqrt_nonnegative()
}

fn catmull_rom_cubic(samples: &[f32], segment_start: f64) -> Cubic {
    let maximum = samples.len().saturating_sub(1) as f64;
    if segment_start < 0.0 {
        return Cubic::constant(samples[0] as f64);
    }
    if segment_start >= maximum {
        return Cubic::constant(samples[samples.len() - 1] as f64);
    }
    let index = segment_start as isize;
    let sample = |offset: isize| -> f64 {
        let sample_index = (index + offset).clamp(0, samples.len() as isize - 1) as usize;
        samples[sample_index] as f64
    };
    let y0 = sample(-1);
    let y1 = sample(0);
    let y2 = sample(1);
    let y3 = sample(2);
    cubic_from_catmull_rom_points(y0, y1, y2, y3)
}

fn catmull_rom_combined_cubic(
    first: &[f32],
    first_scale: f64,
    second: &[f32],
    second_scale: f64,
    segment_start: f64,
) -> Cubic {
    let maximum = first.len().saturating_sub(1) as f64;
    let combined =
        |index: usize| first[index] as f64 * first_scale + second[index] as f64 * second_scale;
    if segment_start < 0.0 {
        return Cubic::constant(combined(0));
    }
    if segment_start >= maximum {
        return Cubic::constant(combined(first.len() - 1));
    }
    let index = segment_start as isize;
    let sample = |offset: isize| -> f64 {
        let sample_index = (index + offset).clamp(0, first.len() as isize - 1) as usize;
        combined(sample_index)
    };
    cubic_from_catmull_rom_points(sample(-1), sample(0), sample(1), sample(2))
}

fn cubic_from_catmull_rom_points(y0: f64, y1: f64, y2: f64, y3: f64) -> Cubic {
    let a = -0.5 * y0 + 1.5 * y1 - 1.5 * y2 + 0.5 * y3;
    let b = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c = -0.5 * y0 + 0.5 * y2;
    Cubic { a, b, c, d: y1 }
}

#[allow(clippy::too_many_arguments)]
fn level_combined_cubic_for_source_segment(
    first: &[f32],
    first_scale: f64,
    second: &[f32],
    second_scale: f64,
    first_source_frame: u64,
    source_frame_step: u32,
    absolute_segment_start: f64,
) -> Cubic {
    let level_position =
        (absolute_segment_start - first_source_frame as f64) / f64::from(source_frame_step);
    let level_segment = level_position.floor();
    let level_fraction = level_position - level_segment;
    catmull_rom_combined_cubic(first, first_scale, second, second_scale, level_segment)
        .compose_affine(level_fraction, 1.0 / f64::from(source_frame_step))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PVC_001_BITS: [u32; 34] = [
        0x350b_792b,
        0x354c_ae8b,
        0x3531_31ae,
        0x34b3_e821,
        0x350d_0364,
        0x356a_6ec7,
        0x352c_aa1c,
        0x34bc_758c,
        0x3278_e910,
        0xb484_c5cd,
        0xb3c1_b6bd,
        0x3415_598a,
        0x344f_598a,
        0x34fe_f80c,
        0x34f3_994e,
        0x3441_5be1,
        0x325b_12cf,
        0x348a_612a,
        0xb379_72a0,
        0x3382_926b,
        0x34a0_d853,
        0x34bc_9a0f,
        0x3510_8297,
        0x3476_5ba1,
        0x34ab_3a42,
        0x33d7_7969,
        0xb494_79ac,
        0xb4db_14dd,
        0xb50f_3b6f,
        0xb568_7635,
        0xb5a1_cdd4,
        0xb5b9_9f59,
        0xb5d5_e8e4,
        0xb5db_2666,
    ];

    const ADVERSARIAL_ENVELOPE: [f32; 34] = [
        3.440_432_2e-5,
        -1.002_237_15e-5,
        -2.237_869_4e-5,
        -8.709_653e-6,
        3.775_332_5e-5,
        1.620_262_1e-5,
        -3.961_649_4e-5,
        -2.871_819_3e-6,
        -2.625_256_1e-5,
        -6.408_326_5e-6,
        2.442_017e-5,
        -2.072_401e-5,
        2.692_459_7e-5,
        -3.579_148e-5,
        -6.109_582e-6,
        -2.664_838_4e-5,
        2.519_981_7e-5,
        2.737_662e-5,
        1.418_165_8e-5,
        3.758_499_2e-5,
        -6.410_559e-6,
        -2.563_184_9e-5,
        2.062_723_1e-5,
        -3.092_471_5e-5,
        2.621_338_9e-5,
        -2.542_806e-6,
        6.139_507_6e-6,
        6.717_547e-6,
        -9.724_627e-6,
        -1.032_875_6e-5,
        6.769_909_3e-6,
        2.057_065_1e-5,
        2.060_638e-5,
        4.273_317_2e-6,
    ];

    fn dense_envelope(
        samples: &[f32],
        center_frame: f64,
        meters_per_frame: f64,
        radius: f64,
        steps: usize,
    ) -> (f64, f64) {
        let mut best_height = f64::NEG_INFINITY;
        let mut best_offset = 0.0;
        for step in 0..=steps {
            let offset = -radius + 2.0 * radius * step as f64 / steps as f64;
            let position = center_frame + offset / meters_per_frame;
            let segment = position.floor();
            let groove = catmull_rom_cubic(samples, segment).value(position - segment);
            let height = groove + circle_height(radius, offset);
            if height > best_height {
                best_height = height;
                best_offset = offset;
            }
        }
        (best_height, best_offset)
    }

    #[test]
    fn multiresolution_support_includes_search_and_cubic_interpolation() {
        let support = StylusGeometry {
            tracing_radius_m: 18.0e-6,
        }
        .multiresolution_support(2.0e-6, 16)
        .unwrap();
        assert_eq!(support.search_half_span_source_frames(), 9);
        assert_eq!(support.left_source_frames(), 25);
        assert_eq!(support.right_source_frames(), 41);
        assert_eq!(support.symmetric_halo_source_frames(), 41);
    }

    #[test]
    fn multiresolution_support_applies_the_fixed_search_limit() {
        assert!(matches!(
            StylusGeometry {
                tracing_radius_m: 100.0e-6,
            }
            .multiresolution_support(0.25e-6, 1),
            Err(StylusTraceError::SearchLimitExceeded)
        ));
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn level_coordinate_multiplication_rejects_overflow() {
        assert_eq!(
            level_last_source_frame(0, u32::MAX, usize::MAX),
            Err(StylusTraceError::InvalidLevelCoordinates)
        );
    }

    #[test]
    fn flat_groove_places_contact_below_the_stylus_center() {
        let samples = vec![0.0_f32; 128];
        let traced =
            trace_spherical_uniform(&samples, 64.0, 2.0e-6, StylusGeometry::default()).unwrap();
        assert!(traced.center_displacement_m.abs() < 1.0e-15);
        assert!(traced.contact_offset_m.abs() < 1.0e-10);
        assert!(traced.groove_slope.abs() < 1.0e-12);
    }

    #[test]
    fn linear_groove_matches_the_circle_tangent_solution() {
        let meters_per_frame = 1.0e-6;
        let slope = 0.08;
        let samples: Vec<f32> = (0..256)
            .map(|index| ((index as f64 - 128.0) * meters_per_frame * slope) as f32)
            .collect();
        let radius = 18.0e-6;
        let traced = trace_spherical_uniform(
            &samples,
            128.0,
            meters_per_frame,
            StylusGeometry {
                tracing_radius_m: radius,
            },
        )
        .unwrap();
        let expected_offset = slope * radius / (1.0 + slope * slope).sqrt();
        let expected_height = radius * (1.0 + slope * slope).sqrt() - radius;
        assert!((traced.contact_offset_m - expected_offset).abs() < 2.0e-9);
        assert!((traced.center_displacement_m - expected_height).abs() < 2.0e-10);
        assert!(traced.tangent_residual.abs() < 2.0e-3);
    }

    #[test]
    fn reversed_spatial_data_is_reciprocal() {
        let samples: Vec<f32> = (0..512)
            .map(|index| {
                let phase = std::f64::consts::TAU * index as f64 / 31.7;
                (2.0e-6 * phase.sin() + 0.4e-6 * (2.0 * phase).cos()) as f32
            })
            .collect();
        let mut reversed = samples.clone();
        reversed.reverse();
        let forward =
            trace_spherical_uniform(&samples, 211.25, 1.2e-6, StylusGeometry::default()).unwrap();
        let backward = trace_spherical_uniform(
            &reversed,
            samples.len() as f64 - 1.0 - 211.25,
            1.2e-6,
            StylusGeometry::default(),
        )
        .unwrap();
        assert!((forward.center_displacement_m - backward.center_displacement_m).abs() < 1.0e-12);
        assert!((forward.contact_offset_m + backward.contact_offset_m).abs() < 1.0e-9);
        assert!((forward.groove_slope + backward.groove_slope).abs() < 1.0e-5);
    }

    #[test]
    fn inner_groove_has_more_tracing_error_for_the_same_recorded_tone() {
        let sample_rate = 192_000.0;
        let frequency = 8_000.0;
        let velocity_peak = 0.05;
        let amplitude = velocity_peak / (std::f64::consts::TAU * frequency);
        let samples: Vec<f32> = (0..4096)
            .map(|index| {
                (amplitude * (std::f64::consts::TAU * frequency * index as f64 / sample_rate).sin())
                    as f32
            })
            .collect();
        let omega = std::f64::consts::TAU * (33.333_333_333_333_336 / 60.0);
        let outer_step = omega * 0.146 / sample_rate;
        let inner_step = omega * 0.060 / sample_rate;
        let mut outer_error = 0.0;
        let mut inner_error = 0.0;
        for index in 256..3840 {
            let center = index as f64 + 0.137;
            let segment = center.floor();
            let source = catmull_rom_cubic(&samples, segment).value(center - segment);
            let outer =
                trace_spherical_uniform(&samples, center, outer_step, StylusGeometry::default())
                    .unwrap();
            let inner =
                trace_spherical_uniform(&samples, center, inner_step, StylusGeometry::default())
                    .unwrap_or_else(|error| {
                        panic!("inner trace failed at frame {center}: {error:?}")
                    });
            outer_error += (outer.center_displacement_m - source).powi(2);
            inner_error += (inner.center_displacement_m - source).powi(2);
        }
        assert!(
            inner_error > outer_error * 1.5,
            "{inner_error} <= {outer_error}"
        );
    }

    #[test]
    fn fixed_search_limit_rejects_unbounded_work() {
        let samples = vec![0.0_f32; 1024];
        let error = trace_spherical_uniform(&samples, 512.0, 1.0e-9, StylusGeometry::default())
            .unwrap_err();
        assert_eq!(error, StylusTraceError::SearchLimitExceeded);
    }

    #[test]
    fn physical_wall_normals_preserve_lateral_and_vertical_45_45_motion() {
        let lateral: Vec<f32> = (0..128)
            .map(|index| (index as f64 * 1.0e-8) as f32)
            .collect();
        let vertical = vec![0.0_f32; lateral.len()];
        let left = trace_spherical_45_45_wall_uniform(
            &lateral,
            &vertical,
            0,
            64.0,
            1.0e-6,
            StylusGeometry::default(),
        )
        .unwrap();
        let right = trace_spherical_45_45_wall_uniform(
            &lateral,
            &vertical,
            1,
            64.0,
            1.0e-6,
            StylusGeometry::default(),
        )
        .unwrap();
        assert!(left.groove_slope > 0.0);
        assert!(right.groove_slope < 0.0);

        let lateral = vec![0.0_f32; vertical.len()];
        let vertical: Vec<f32> = (0..128)
            .map(|index| (index as f64 * 1.0e-8) as f32)
            .collect();
        let left = trace_spherical_45_45_wall_uniform(
            &lateral,
            &vertical,
            0,
            64.0,
            1.0e-6,
            StylusGeometry::default(),
        )
        .unwrap();
        let right = trace_spherical_45_45_wall_uniform(
            &lateral,
            &vertical,
            1,
            64.0,
            1.0e-6,
            StylusGeometry::default(),
        )
        .unwrap();
        assert!(left.groove_slope > 0.0);
        assert!(right.groove_slope > 0.0);
    }

    #[test]
    fn uniform_level_blend_has_exact_endpoints() {
        let lower_lateral: Vec<f32> = (0..512)
            .map(|frame| (1.0e-6 * (std::f64::consts::TAU * frame as f64 / 31.25).sin()) as f32)
            .collect();
        let lower_vertical = vec![0.0_f32; lower_lateral.len()];
        let upper_lateral: Vec<f32> = lower_lateral
            .iter()
            .enumerate()
            .map(|(frame, sample)| sample * 0.4 + frame as f32 * 1.0e-10)
            .collect();
        let upper_vertical = vec![0.0_f32; upper_lateral.len()];
        let geometry = StylusGeometry::default();
        let lower = trace_spherical_45_45_wall_uniform(
            &lower_lateral,
            &lower_vertical,
            0,
            245.75,
            1.5e-6,
            geometry,
        )
        .unwrap();
        let upper = trace_spherical_45_45_wall_uniform(
            &upper_lateral,
            &upper_vertical,
            0,
            245.75,
            1.5e-6,
            geometry,
        )
        .unwrap();
        assert_eq!(
            trace_spherical_45_45_wall_blended_uniform(
                &lower_lateral,
                &lower_vertical,
                &upper_lateral,
                &upper_vertical,
                0.0,
                0,
                245.75,
                1.5e-6,
                geometry,
            )
            .unwrap(),
            lower
        );
        assert_eq!(
            trace_spherical_45_45_wall_blended_uniform(
                &lower_lateral,
                &lower_vertical,
                &upper_lateral,
                &upper_vertical,
                1.0,
                0,
                245.75,
                1.5e-6,
                geometry,
            )
            .unwrap(),
            upper
        );
    }

    #[test]
    fn multiresolution_wall_trace_is_reciprocal() {
        let lower_lateral: Vec<f32> = (0..1_025)
            .map(|frame| {
                let phase = std::f64::consts::TAU * frame as f64 / 41.7;
                (1.3e-6 * phase.sin() + 0.2e-6 * (2.0 * phase).cos()) as f32
            })
            .collect();
        let lower_vertical: Vec<f32> = (0..1_025)
            .map(|frame| (0.25e-6 * (std::f64::consts::TAU * frame as f64 / 67.2).cos()) as f32)
            .collect();
        let upper_lateral: Vec<f32> = (0..513)
            .map(|frame| lower_lateral[frame * 2] * 0.85)
            .collect();
        let upper_vertical: Vec<f32> = (0..513)
            .map(|frame| lower_vertical[frame * 2] * 0.85)
            .collect();
        let mut reversed_lower_lateral = lower_lateral.clone();
        let mut reversed_lower_vertical = lower_vertical.clone();
        let mut reversed_upper_lateral = upper_lateral.clone();
        let mut reversed_upper_vertical = upper_vertical.clone();
        reversed_lower_lateral.reverse();
        reversed_lower_vertical.reverse();
        reversed_upper_lateral.reverse();
        reversed_upper_vertical.reverse();
        let center = 411.25;
        let forward = trace_spherical_45_45_wall_multiresolution(
            &lower_lateral,
            &lower_vertical,
            0,
            1,
            &upper_lateral,
            &upper_vertical,
            0,
            2,
            0.375,
            0,
            center,
            1.2e-6,
            StylusGeometry::default(),
        )
        .unwrap();
        let reverse = trace_spherical_45_45_wall_multiresolution(
            &reversed_lower_lateral,
            &reversed_lower_vertical,
            0,
            1,
            &reversed_upper_lateral,
            &reversed_upper_vertical,
            0,
            2,
            0.375,
            0,
            1_024.0 - center,
            1.2e-6,
            StylusGeometry::default(),
        )
        .unwrap();
        assert!((forward.center_displacement_m - reverse.center_displacement_m).abs() < 1.0e-12);
        assert!((forward.contact_offset_m + reverse.contact_offset_m).abs() < 1.0e-9);
        assert!((forward.groove_slope + reverse.groove_slope).abs() < 1.0e-5);
    }

    #[test]
    fn multiresolution_trace_clamps_prefiltered_levels_at_record_ends() {
        let lower = vec![0.0_f32; 257];
        let upper = vec![0.0_f32; 16];
        let traced = trace_spherical_45_45_wall_multiresolution(
            &lower,
            &lower,
            0,
            1,
            &upper,
            &upper,
            0,
            16,
            0.5,
            0,
            256.0,
            2.0e-6,
            StylusGeometry::default(),
        )
        .unwrap();
        assert!(traced.center_displacement_m.abs() < 1.0e-15);
    }

    #[test]
    fn pvc_001_exact_bits_select_the_global_reference_peak() {
        let samples = PVC_001_BITS.map(f32::from_bits);
        assert_eq!(samples.map(f32::to_bits), PVC_001_BITS);
        let traced =
            trace_spherical_uniform(&samples, 16.37, 1.5e-6, StylusGeometry::default()).unwrap();
        let reference_lower = 2.370_385_574_217_050_8e-7;
        let reference_upper = reference_lower + 9.826_400_058_430_392e-11;
        assert!(traced.center_displacement_m >= reference_lower);
        assert!(traced.center_displacement_m <= reference_upper);
        assert!((8.248_828_124_999_985e-7..=8.442_004_394_531_236e-7)
            .contains(&traced.contact_offset_m));

        let (dense_height, dense_offset) = dense_envelope(
            &samples,
            16.37,
            1.5e-6,
            StylusGeometry::default().tracing_radius_m,
            262_144,
        );
        assert!((traced.center_displacement_m + 18.0e-6 - dense_height).abs() < 1.0e-12);
        assert!((traced.contact_offset_m - dense_offset).abs() < 2.0e-10);
    }

    #[test]
    fn adversarial_fixture_still_selects_the_dense_global_peak() {
        let traced = trace_spherical_uniform(
            &ADVERSARIAL_ENVELOPE,
            16.37,
            1.5e-6,
            StylusGeometry::default(),
        )
        .unwrap();
        let (dense_height, dense_offset) = dense_envelope(
            &ADVERSARIAL_ENVELOPE,
            16.37,
            1.5e-6,
            StylusGeometry::default().tracing_radius_m,
            1_048_576,
        );
        assert!((traced.center_displacement_m + 18.0e-6 - dense_height).abs() < 1.0e-12);
        assert!((traced.contact_offset_m - dense_offset).abs() < 1.0e-10);
    }

    #[test]
    fn unresolved_separated_maxima_return_typed_height_order_errors() {
        let sample_rate = 192_000.0;
        let frequency = 8_000.0;
        let amplitude = 0.05 / (std::f64::consts::TAU * frequency);
        let samples: Vec<f32> = (0..512)
            .map(|index| {
                (amplitude * (std::f64::consts::TAU * frequency * index as f64 / sample_rate).sin())
                    as f32
            })
            .collect();
        let meters_per_frame =
            std::f64::consts::TAU * (33.333_333_333_333_336 / 60.0) * 0.060 / sample_rate;
        assert_eq!(
            trace_spherical_uniform(&samples, 258.0, meters_per_frame, StylusGeometry::default(),),
            Err(StylusTraceError::GlobalContactNotIsolated)
        );
        assert_eq!(
            trace_spherical_uniform_contacts(
                &samples,
                258.0,
                meters_per_frame,
                StylusGeometry::default(),
            ),
            Err(StylusTraceError::ContactHeightOrderNotIsolated)
        );
    }

    #[test]
    fn a_resolved_near_tie_is_deterministic() {
        let sample_rate = 192_000.0;
        let frequency = 8_000.0;
        let amplitude = 0.05 / (std::f64::consts::TAU * frequency);
        let mut samples: Vec<f32> = (0..512)
            .map(|index| {
                (amplitude * (std::f64::consts::TAU * frequency * index as f64 / sample_rate).sin())
                    as f32
            })
            .collect();
        samples[260] += 1.0e-9;
        let meters_per_frame =
            std::f64::consts::TAU * (33.333_333_333_333_336 / 60.0) * 0.060 / sample_rate;
        let first =
            trace_spherical_uniform(&samples, 258.0, meters_per_frame, StylusGeometry::default())
                .unwrap();
        let second =
            trace_spherical_uniform(&samples, 258.0, meters_per_frame, StylusGeometry::default())
                .unwrap();
        assert_eq!(first, second);
        assert!(first.contact_offset_m > 0.0);
    }

    #[test]
    fn all_public_trace_paths_select_the_pvc_001_peak() {
        let wall = PVC_001_BITS.map(f32::from_bits);
        let zero = [0.0_f32; PVC_001_BITS.len()];
        let uniform = trace_spherical_45_45_wall_uniform(
            &wall,
            &zero,
            0,
            16.37,
            1.5e-6,
            StylusGeometry::default(),
        )
        .unwrap();
        let blended = trace_spherical_45_45_wall_blended_uniform(
            &wall,
            &zero,
            &wall,
            &zero,
            0.375,
            0,
            16.37,
            1.5e-6,
            StylusGeometry::default(),
        )
        .unwrap();
        let multiresolution = trace_spherical_45_45_wall_multiresolution(
            &wall,
            &zero,
            10,
            1,
            &wall,
            &zero,
            10,
            1,
            0.375,
            0,
            26.37,
            1.5e-6,
            StylusGeometry::default(),
        )
        .unwrap();
        assert!(uniform.contact_offset_m > 0.0);
        assert_eq!(uniform, blended);
        assert!(
            (uniform.center_displacement_m - multiresolution.center_displacement_m).abs()
                < SPHERICAL_TRACE_HEIGHT_ERROR_BOUND_M
        );
        assert!(
            (uniform.contact_offset_m - multiresolution.contact_offset_m).abs()
                < SPHERICAL_TRACE_CONTACT_POSITION_ERROR_BOUND_M
        );
    }

    #[test]
    fn successful_trace_does_not_allocate() {
        let samples = PVC_001_BITS.map(f32::from_bits);
        let mut result = None;
        assert_no_alloc::assert_no_alloc(|| {
            result = Some(trace_spherical_uniform(
                &samples,
                16.37,
                1.5e-6,
                StylusGeometry::default(),
            ));
        });
        assert!(result.unwrap().is_ok());
    }

    #[test]
    fn non_finite_support_returns_a_typed_error() {
        let mut samples = [0.0_f32; 64];
        samples[31] = f32::NAN;
        assert_eq!(
            trace_spherical_uniform(&samples, 31.0, 1.5e-6, StylusGeometry::default()),
            Err(StylusTraceError::InvalidGrooveDisplacement)
        );
    }

    #[test]
    fn stationary_search_obeys_its_declared_node_cap() {
        let meters_per_frame = 1.5e-6;
        let radius = 18.0e-6;
        let linear = meters_per_frame * meters_per_frame / radius;
        let cubic = Cubic {
            a: 0.0,
            b: linear * 0.5,
            c: -linear * 0.5,
            d: 0.0,
        };
        let result = isolate_stationary_contacts(
            cubic,
            0.0,
            0.5,
            meters_per_frame,
            radius,
            OutwardInterval::ordered(0.0, 1.0),
        );
        match result {
            Ok(roots) => {
                assert!(roots.searched_nodes <= SPHERICAL_TRACE_MAX_ROOT_NODES_PER_PIECE);
                assert!(roots.brackets[..roots.len]
                    .iter()
                    .any(|root| root.lower <= 0.5 && root.upper >= 0.5));
            }
            Err(error) => assert_eq!(error, StylusTraceError::StationaryContactNotIsolated),
        }
    }
}
