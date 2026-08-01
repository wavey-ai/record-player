//! Offline convergence and disproof cases for rapid nonlinear groove contact.
//!
//! This module is test-only. Its high-rate path is a reference, not a render
//! implementation. It keeps each integration step allocation-free after the
//! fixture and spatial pyramid exist.

use super::contact::{
    test_single_wall_contacts, PickupContactSurface, PickupMechanicalInput, PickupMechanicalState,
    StylusContactConfig,
};
use super::groove::{
    GrooveAsset, GrooveCutReport, GrooveLayout, GrooveSourceKind, GrooveSpatialLevelSelection,
    GrooveSpatialPyramid, RecordCutConfig,
};
use super::stylus::{
    trace_spherical_45_45_wall_multiresolution, trace_spherical_uniform,
    trace_spherical_uniform_contacts, StylusGeometry, StylusTraceError, StylusTraceSample,
};
use super::tonearm::{SuspensionAxisConfig, TonearmConfig};

const OUTPUT_SAMPLE_RATE_HZ: f64 = 192_000.0;
const OUTPUT_DT_SECONDS: f64 = 1.0 / OUTPUT_SAMPLE_RATE_HZ;
const GROOVE_RADIUS_M: f64 = 0.082_505_922_498_838_55;
const METERS_PER_SOURCE_FRAME: f64 = 1.5e-6;
const MAX_REFERENCE_SWEEP_STEP_FRAMES: f64 = 0.125;
const MIN_REFERENCE_SUBSTEPS: usize = 8;
const WALL_SCALE: f64 = std::f64::consts::FRAC_1_SQRT_2;
const TRACE_HEIGHT_CERTIFICATE_M: f64 = 1.0e-10;
const TRACE_POSITION_CELL_M: f64 = 1.0e-10;
const MAX_CERTIFIED_DEPTH: u32 = 56;
const PVC_001_SPATIAL_ARTIFACT: &str =
    include_str!("../../tests/fixtures/pvc_001_catmull_rom_global_envelope.json");
const PVC_001_WALL_VELOCITY_ARTIFACT: &str =
    include_str!("../../tests/fixtures/pvc_001_wall_velocity_global_envelope.json");
const PVC_004_HEIGHT_ORDER_ARTIFACT: &str =
    include_str!("../../tests/fixtures/pvc_004_inner_groove_height_order_ambiguity.json");

// This stress fixture is accepted by the asset API. Its derivatives exceed
// the current seed cut by orders of magnitude. Keep it as contrary evidence.
const ADVERSARIAL_ENVELOPE_COUNTEREXAMPLE: [f32; 34] = [
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

// Exact f32 values are asserted by bit pattern in the permanent PVC-001 test.
const PLAUSIBLE_ENVELOPE_COUNTEREXAMPLE: [f32; 34] = [
    f32::from_bits(0x350b_792b),
    f32::from_bits(0x354c_ae8b),
    f32::from_bits(0x3531_31ae),
    f32::from_bits(0x34b3_e821),
    f32::from_bits(0x350d_0364),
    f32::from_bits(0x356a_6ec7),
    f32::from_bits(0x352c_aa1c),
    f32::from_bits(0x34bc_758c),
    f32::from_bits(0x3278_e910),
    f32::from_bits(0xb484_c5cd),
    f32::from_bits(0xb3c1_b6bd),
    f32::from_bits(0x3415_598a),
    f32::from_bits(0x344f_598a),
    f32::from_bits(0x34fe_f80c),
    f32::from_bits(0x34f3_994e),
    f32::from_bits(0x3441_5be1),
    f32::from_bits(0x325b_12cf),
    f32::from_bits(0x348a_612a),
    f32::from_bits(0xb379_72a0),
    f32::from_bits(0x3382_926b),
    f32::from_bits(0x34a0_d853),
    f32::from_bits(0x34bc_9a0f),
    f32::from_bits(0x3510_8297),
    f32::from_bits(0x3476_5ba1),
    f32::from_bits(0x34ab_3a42),
    f32::from_bits(0x33d7_7969),
    f32::from_bits(0xb494_79ac),
    f32::from_bits(0xb4db_14dd),
    f32::from_bits(0xb50f_3b6f),
    f32::from_bits(0xb568_7635),
    f32::from_bits(0xb5a1_cdd4),
    f32::from_bits(0xb5b9_9f59),
    f32::from_bits(0xb5d5_e8e4),
    f32::from_bits(0xb5db_2666),
];

// This accepted wall-velocity fixture was derived from seven sinusoids. The
// highest construction frequency was 0.22917498438472408 cycles per frame.
// The exact f32 wall velocities reproduce the production integration path.
const SEED_DOMAIN_WALL_VELOCITY_BITS: [u32; 96] = [
    0xbcea_6c67,
    0x3ca9_3222,
    0x3d08_92ca,
    0x3c0b_bcfe,
    0xbd0e_3491,
    0xbb15_773a,
    0x3cb2_2f56,
    0x3c23_3dd9,
    0xbd35_a384,
    0xbcc4_e6e2,
    0x3c99_a40c,
    0x3d19_0047,
    0xbc91_4d93,
    0xbcc3_a0dd,
    0x3c3f_7ae7,
    0x3d4b_2b47,
    0xbad7_5489,
    0xbd0f_cfc9,
    0xbc89_b2e8,
    0x3d27_4309,
    0x3c1b_0cec,
    0xbd29_0938,
    0xbd3c_4d11,
    0x3d05_0049,
    0x3d33_44c0,
    0xbbde_308a,
    0xbd59_6912,
    0x3c3a_10d8,
    0x3d73_c27e,
    0x3cfc_e810,
    0xbd65_5c39,
    0xbd24_e749,
    0x3cc6_af38,
    0x3d4f_e66e,
    0xbcdd_7e8d,
    0xbd65_f2a1,
    0xbc33_ff19,
    0x3d6e_0148,
    0x3ca8_4e4a,
    0xbcf9_9971,
    0xbcfb_a50f,
    0x3d23_8165,
    0x3d24_d586,
    0xbbbd_e2b3,
    0xbd46_ce1d,
    0xbbf2_9c6f,
    0x3c82_1169,
    0x3c1a_0258,
    0xbcd6_3d03,
    0xbb8a_41d7,
    0x3c22_0dc1,
    0x3c54_7295,
    0xbc54_aa2d,
    0x3c1a_8a27,
    0x3ca1_ddfe,
    0x3c71_b32c,
    0xbcc7_47db,
    0xbc0f_80f2,
    0x3c09_50be,
    0x3c1c_7fad,
    0xbd13_ee4e,
    0xbcb3_17cd,
    0x3c9c_37ff,
    0x3d3b_b47f,
    0xbc4f_c3c7,
    0xbd19_cf27,
    0xbb94_2f31,
    0x3d73_c27e,
    0x3cc6_ff26,
    0xbd0b_b703,
    0xbd40_c04a,
    0x3cbe_e0a3,
    0x3cfb_315e,
    0xbc88_c166,
    0xbd6d_8747,
    0x3bc0_3a2d,
    0x3d51_7dba,
    0x3ce4_fb3e,
    0xbd39_326c,
    0xbcb5_9a66,
    0x3d0d_b996,
    0x3d4e_ca44,
    0xbccc_17a2,
    0xbd37_f28e,
    0xbba9_bd51,
    0x3d2b_3c61,
    0xbc0c_f6f3,
    0xbd36_17eb,
    0xbca5_00ad,
    0x3d54_bb87,
    0x3d01_b31a,
    0xbcae_c77d,
    0xbd20_5ea0,
    0x3cec_f7a3,
    0x3d2e_5e05,
    0xba9b_9dcf,
];

#[derive(Debug, Clone, Copy)]
struct CubicSegment {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    source_index: isize,
}

#[derive(Debug, Clone, Copy)]
struct Interval {
    lower: f64,
    upper: f64,
}

impl Interval {
    fn point(value: f64) -> Self {
        assert!(value.is_finite());
        Self {
            lower: value,
            upper: value,
        }
    }

    fn ordered(lower: f64, upper: f64) -> Self {
        assert!(lower.is_finite() && upper.is_finite() && lower <= upper);
        Self { lower, upper }
    }

    fn add(self, other: Self) -> Self {
        Self {
            lower: (self.lower + other.lower).next_down(),
            upper: (self.upper + other.upper).next_up(),
        }
    }

    fn negate(self) -> Self {
        Self {
            lower: (-self.upper).next_down(),
            upper: (-self.lower).next_up(),
        }
    }

    fn subtract(self, other: Self) -> Self {
        self.add(other.negate())
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
            lower: lower.next_down(),
            upper: upper.next_up(),
        }
    }

    fn divide(self, positive: Self) -> Self {
        assert!(positive.lower > 0.0);
        let reciprocal = Self {
            lower: (1.0 / positive.upper).next_down(),
            upper: (1.0 / positive.lower).next_up(),
        };
        self.multiply(reciprocal)
    }

    fn square(self) -> Self {
        let endpoint_squares = [self.lower * self.lower, self.upper * self.upper];
        let lower = if self.lower <= 0.0 && self.upper >= 0.0 {
            0.0
        } else {
            endpoint_squares.into_iter().fold(f64::INFINITY, f64::min)
        };
        let upper = endpoint_squares.into_iter().fold(0.0, f64::max);
        Self {
            lower: if lower == 0.0 { 0.0 } else { lower.next_down() },
            upper: upper.next_up(),
        }
    }

    fn sqrt_nonnegative(self) -> Self {
        assert!(self.upper >= 0.0);
        Self {
            lower: self.lower.max(0.0).sqrt().next_down(),
            upper: self.upper.max(0.0).sqrt().next_up(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct IntervalCubicSegment {
    a: Interval,
    b: Interval,
    c: Interval,
    d: Interval,
    source_index: isize,
}

impl IntervalCubicSegment {
    fn values(self, fraction: Interval) -> Interval {
        self.a
            .multiply(fraction)
            .add(self.b)
            .multiply(fraction)
            .add(self.c)
            .multiply(fraction)
            .add(self.d)
    }
}

impl CubicSegment {
    fn value(self, fraction: f64) -> f64 {
        ((self.a * fraction + self.b) * fraction + self.c) * fraction + self.d
    }

    fn slope_per_frame(self, fraction: f64) -> f64 {
        (3.0 * self.a * fraction + 2.0 * self.b) * fraction + self.c
    }

    fn curvature_per_frame_squared(self, fraction: f64) -> f64 {
        6.0 * self.a * fraction + 2.0 * self.b
    }

    fn stationary_points(self) -> [Option<f64>; 2] {
        let quadratic = 3.0 * self.a;
        let linear = 2.0 * self.b;
        if quadratic.abs() <= f64::EPSILON {
            if linear.abs() <= f64::EPSILON {
                return [None, None];
            }
            return [Some(-self.c / linear), None];
        }
        let discriminant = linear * linear - 4.0 * quadratic * self.c;
        if discriminant < 0.0 {
            return [None, None];
        }
        let root = discriminant.sqrt();
        [
            Some((-linear - root) / (2.0 * quadratic)),
            Some((-linear + root) / (2.0 * quadratic)),
        ]
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CubicKinematicBounds {
    peak_displacement_m: f64,
    peak_adjacent_step_m: f64,
    peak_slope_per_frame_m: f64,
    peak_curvature_per_frame_squared_m: f64,
}

impl CubicKinematicBounds {
    fn peak_dimensionless_slope(self, meters_per_frame: f64) -> f64 {
        self.peak_slope_per_frame_m / meters_per_frame
    }

    fn peak_velocity_m_s(self, frames_per_second: f64) -> f64 {
        self.peak_slope_per_frame_m * frames_per_second
    }

    fn peak_acceleration_m_s2(self, frames_per_second: f64) -> f64 {
        self.peak_curvature_per_frame_squared_m * frames_per_second.powi(2)
    }
}

fn cubic_kinematic_bounds(samples: &[f32]) -> CubicKinematicBounds {
    let mut bounds = CubicKinematicBounds::default();
    for &sample in samples {
        bounds.peak_displacement_m = bounds.peak_displacement_m.max(f64::from(sample).abs());
    }
    for pair in samples.windows(2) {
        bounds.peak_adjacent_step_m = bounds
            .peak_adjacent_step_m
            .max((f64::from(pair[1]) - f64::from(pair[0])).abs());
    }
    for source_index in 0..samples.len() {
        let segment = cubic_segment(samples, source_index as isize);
        let mut displacement_fractions = [None; 4];
        displacement_fractions[0] = Some(0.0);
        displacement_fractions[1] = Some(1.0);
        displacement_fractions[2..].copy_from_slice(&segment.stationary_points());
        for fraction in displacement_fractions.into_iter().flatten() {
            if (0.0..=1.0).contains(&fraction) {
                bounds.peak_displacement_m = bounds
                    .peak_displacement_m
                    .max(segment.value(fraction).abs());
            }
        }
        let slope_stationary = if segment.a.abs() > f64::EPSILON {
            Some(-segment.b / (3.0 * segment.a))
        } else {
            None
        };
        for fraction in [Some(0.0), Some(1.0), slope_stationary]
            .into_iter()
            .flatten()
        {
            if (0.0..=1.0).contains(&fraction) {
                bounds.peak_slope_per_frame_m = bounds
                    .peak_slope_per_frame_m
                    .max(segment.slope_per_frame(fraction).abs());
            }
        }
        for fraction in [0.0, 1.0] {
            bounds.peak_curvature_per_frame_squared_m = bounds
                .peak_curvature_per_frame_squared_m
                .max(segment.curvature_per_frame_squared(fraction).abs());
        }
    }
    bounds
}

#[derive(Debug, Clone, Copy)]
struct OutwardIntervalTrace {
    sample: StylusTraceSample,
    height_error_bound_m: f64,
    contact_offset_enclosure_m: [f64; 2],
    searched_cells: u64,
}

#[derive(Debug)]
struct EnvelopeSearch {
    best_height_m: f64,
    best_offset_m: f64,
    terminal_upper_m: f64,
    searched_cells: u64,
    height_tolerance_m: f64,
}

#[derive(Debug)]
struct PositionEnclosure {
    left_m: f64,
    right_m: f64,
    searched_cells: u64,
    cell_tolerance_m: f64,
}

impl EnvelopeSearch {
    fn consider(
        &mut self,
        segment: CubicSegment,
        interval_segment: IntervalCubicSegment,
        center_frame: f64,
        meters_per_frame: f64,
        radius_m: f64,
        offset_m: f64,
    ) {
        let fraction = center_frame + offset_m / meters_per_frame - segment.source_index as f64;
        let interval_fraction = interval_fraction_for_offsets(
            interval_segment.source_index,
            center_frame,
            meters_per_frame,
            Interval::point(offset_m),
        );
        let height_lower_m = interval_segment
            .values(interval_fraction)
            .add(interval_circle_height(radius_m, Interval::point(offset_m)))
            .lower;
        debug_assert!(
            height_lower_m <= segment.value(fraction) + circle_height(radius_m, offset_m)
        );
        if height_lower_m > self.best_height_m {
            self.best_height_m = height_lower_m;
            self.best_offset_m = offset_m;
        }
    }
}

fn cubic_segment(samples: &[f32], source_index: isize) -> CubicSegment {
    let sample = |offset: isize| -> f64 {
        let index = (source_index + offset).clamp(0, samples.len() as isize - 1) as usize;
        f64::from(samples[index])
    };
    let y0 = sample(-1);
    let y1 = sample(0);
    let y2 = sample(1);
    let y3 = sample(2);
    CubicSegment {
        a: -0.5 * y0 + 1.5 * y1 - 1.5 * y2 + 0.5 * y3,
        b: y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3,
        c: -0.5 * y0 + 0.5 * y2,
        d: y1,
        source_index,
    }
}

fn interval_cubic_segment(samples: &[f32], source_index: isize) -> IntervalCubicSegment {
    let sample = |offset: isize| -> Interval {
        let index = (source_index + offset).clamp(0, samples.len() as isize - 1) as usize;
        Interval::point(f64::from(samples[index]))
    };
    let y0 = sample(-1);
    let y1 = sample(0);
    let y2 = sample(1);
    let y3 = sample(2);
    let half = Interval::point(0.5);
    let one_and_half = Interval::point(1.5);
    let two = Interval::point(2.0);
    let two_and_half = Interval::point(2.5);
    IntervalCubicSegment {
        a: y0
            .multiply(half)
            .negate()
            .add(y1.multiply(one_and_half))
            .subtract(y2.multiply(one_and_half))
            .add(y3.multiply(half)),
        b: y0
            .subtract(y1.multiply(two_and_half))
            .add(y2.multiply(two))
            .subtract(y3.multiply(half)),
        c: y0.multiply(half).negate().add(y2.multiply(half)),
        d: y1,
        source_index,
    }
}

fn interval_fraction_for_offsets(
    source_index: isize,
    center_frame: f64,
    meters_per_frame: f64,
    offsets_m: Interval,
) -> Interval {
    Interval::point(center_frame)
        .add(offsets_m.divide(Interval::point(meters_per_frame)))
        .subtract(Interval::point(source_index as f64))
}

fn interval_circle_height(radius_m: f64, offsets_m: Interval) -> Interval {
    let radius = Interval::point(radius_m);
    let radius_squared = radius.square();
    let offset_squared = offsets_m.square();
    radius_squared.subtract(offset_squared).sqrt_nonnegative()
}

fn circle_height(radius_m: f64, offset_m: f64) -> f64 {
    (radius_m * radius_m - offset_m * offset_m).max(0.0).sqrt()
}

fn maximum_cubic_in_cell(
    segment: CubicSegment,
    center_frame: f64,
    meters_per_frame: f64,
    left_m: f64,
    right_m: f64,
) -> (f64, f64) {
    let left_fraction = center_frame + left_m / meters_per_frame - segment.source_index as f64;
    let right_fraction = center_frame + right_m / meters_per_frame - segment.source_index as f64;
    let mut maximum = segment.value(left_fraction);
    let mut maximum_fraction = left_fraction;
    let right_value = segment.value(right_fraction);
    if right_value > maximum {
        maximum = right_value;
        maximum_fraction = right_fraction;
    }
    for root in segment.stationary_points().into_iter().flatten() {
        if root > left_fraction && root < right_fraction {
            let value = segment.value(root);
            if value > maximum {
                maximum = value;
                maximum_fraction = root;
            }
        }
    }
    let offset_m =
        (segment.source_index as f64 + maximum_fraction - center_frame) * meters_per_frame;
    (maximum, offset_m.clamp(left_m, right_m))
}

fn envelope_upper_bound_m(
    segment: CubicSegment,
    interval_segment: IntervalCubicSegment,
    center_frame: f64,
    meters_per_frame: f64,
    radius_m: f64,
    left_m: f64,
    right_m: f64,
) -> (f64, f64, f64) {
    let (_, cubic_maximum_offset_m) =
        maximum_cubic_in_cell(segment, center_frame, meters_per_frame, left_m, right_m);
    let circle_maximum_offset_m = 0.0_f64.clamp(left_m, right_m);
    let offsets_m = Interval::ordered(left_m, right_m);
    let fractions = interval_fraction_for_offsets(
        interval_segment.source_index,
        center_frame,
        meters_per_frame,
        offsets_m,
    );
    let upper_bound_m = interval_segment
        .values(fractions)
        .add(interval_circle_height(radius_m, offsets_m))
        .upper;
    (
        upper_bound_m,
        cubic_maximum_offset_m,
        circle_maximum_offset_m,
    )
}

#[allow(clippy::too_many_arguments)]
fn search_envelope_cell(
    search: &mut EnvelopeSearch,
    segment: CubicSegment,
    interval_segment: IntervalCubicSegment,
    center_frame: f64,
    meters_per_frame: f64,
    radius_m: f64,
    left_m: f64,
    right_m: f64,
    depth: u32,
) {
    search.searched_cells += 1;
    let (upper_bound_m, cubic_maximum_offset_m, circle_maximum_offset_m) = envelope_upper_bound_m(
        segment,
        interval_segment,
        center_frame,
        meters_per_frame,
        radius_m,
        left_m,
        right_m,
    );

    let midpoint_m = 0.5 * (left_m + right_m);
    for offset_m in [
        left_m,
        midpoint_m,
        right_m,
        cubic_maximum_offset_m,
        circle_maximum_offset_m,
    ] {
        search.consider(
            segment,
            interval_segment,
            center_frame,
            meters_per_frame,
            radius_m,
            offset_m,
        );
    }
    if upper_bound_m <= (search.best_height_m + search.height_tolerance_m).next_up() {
        search.terminal_upper_m = search.terminal_upper_m.max(upper_bound_m);
        return;
    }
    if depth >= MAX_CERTIFIED_DEPTH || midpoint_m == left_m || midpoint_m == right_m {
        search.terminal_upper_m = search.terminal_upper_m.max(upper_bound_m);
        return;
    }
    search_envelope_cell(
        search,
        segment,
        interval_segment,
        center_frame,
        meters_per_frame,
        radius_m,
        left_m,
        midpoint_m,
        depth + 1,
    );
    search_envelope_cell(
        search,
        segment,
        interval_segment,
        center_frame,
        meters_per_frame,
        radius_m,
        midpoint_m,
        right_m,
        depth + 1,
    );
}

#[allow(clippy::too_many_arguments)]
fn enclose_contact_position_cell(
    enclosure: &mut PositionEnclosure,
    segment: CubicSegment,
    interval_segment: IntervalCubicSegment,
    center_frame: f64,
    meters_per_frame: f64,
    radius_m: f64,
    best_height_m: f64,
    left_m: f64,
    right_m: f64,
    depth: u32,
) {
    enclosure.searched_cells += 1;
    let (upper_bound_m, _, _) = envelope_upper_bound_m(
        segment,
        interval_segment,
        center_frame,
        meters_per_frame,
        radius_m,
        left_m,
        right_m,
    );
    if upper_bound_m < best_height_m {
        return;
    }
    let midpoint_m = 0.5 * (left_m + right_m);
    if right_m - left_m <= enclosure.cell_tolerance_m
        || depth >= MAX_CERTIFIED_DEPTH
        || midpoint_m == left_m
        || midpoint_m == right_m
    {
        enclosure.left_m = enclosure.left_m.min(left_m);
        enclosure.right_m = enclosure.right_m.max(right_m);
        return;
    }
    enclose_contact_position_cell(
        enclosure,
        segment,
        interval_segment,
        center_frame,
        meters_per_frame,
        radius_m,
        best_height_m,
        left_m,
        midpoint_m,
        depth + 1,
    );
    enclose_contact_position_cell(
        enclosure,
        segment,
        interval_segment,
        center_frame,
        meters_per_frame,
        radius_m,
        best_height_m,
        midpoint_m,
        right_m,
        depth + 1,
    );
}

fn outward_interval_trace_spherical_uniform(
    samples: &[f32],
    center_frame: f64,
    meters_per_frame: f64,
    geometry: StylusGeometry,
    height_tolerance_m: f64,
) -> OutwardIntervalTrace {
    let radius_m = geometry.validate().unwrap().tracing_radius_m;
    outward_interval_trace_spherical_uniform_in_domain(
        samples,
        center_frame,
        meters_per_frame,
        geometry,
        height_tolerance_m,
        [-radius_m, radius_m],
    )
}

fn outward_interval_trace_spherical_uniform_in_domain(
    samples: &[f32],
    center_frame: f64,
    meters_per_frame: f64,
    geometry: StylusGeometry,
    height_tolerance_m: f64,
    offset_domain_m: [f64; 2],
) -> OutwardIntervalTrace {
    assert!(samples.len() >= 4);
    assert!(center_frame.is_finite());
    assert!(meters_per_frame.is_finite() && meters_per_frame > 0.0);
    assert!(height_tolerance_m.is_finite() && height_tolerance_m > 0.0);
    let radius_m = geometry.validate().unwrap().tracing_radius_m;
    assert!(offset_domain_m[0].is_finite() && offset_domain_m[1].is_finite());
    assert!(-radius_m <= offset_domain_m[0]);
    assert!(offset_domain_m[0] < offset_domain_m[1]);
    assert!(offset_domain_m[1] <= radius_m);
    let mut search = EnvelopeSearch {
        best_height_m: f64::NEG_INFINITY,
        best_offset_m: 0.0,
        terminal_upper_m: f64::NEG_INFINITY,
        searched_cells: 0,
        height_tolerance_m,
    };

    let mut left_m = offset_domain_m[0];
    while left_m < offset_domain_m[1] {
        let source_position = center_frame + left_m / meters_per_frame;
        let source_index = source_position.floor() as isize;
        let next_boundary_m = (source_index as f64 + 1.0 - center_frame) * meters_per_frame;
        let right_m = offset_domain_m[1].min(next_boundary_m.max(left_m + f64::EPSILON));
        search_envelope_cell(
            &mut search,
            cubic_segment(samples, source_index),
            interval_cubic_segment(samples, source_index),
            center_frame,
            meters_per_frame,
            radius_m,
            left_m,
            right_m,
            0,
        );
        left_m = right_m;
    }

    let certified_upper_m = search.terminal_upper_m.max(search.best_height_m);
    let height_error_bound_m = (certified_upper_m - search.best_height_m)
        .next_up()
        .max(0.0);
    assert!(height_error_bound_m <= height_tolerance_m);

    let mut enclosure = PositionEnclosure {
        left_m: f64::INFINITY,
        right_m: f64::NEG_INFINITY,
        searched_cells: 0,
        cell_tolerance_m: TRACE_POSITION_CELL_M,
    };
    let mut left_m = offset_domain_m[0];
    while left_m < offset_domain_m[1] {
        let source_position = center_frame + left_m / meters_per_frame;
        let source_index = source_position.floor() as isize;
        let next_boundary_m = (source_index as f64 + 1.0 - center_frame) * meters_per_frame;
        let right_m = offset_domain_m[1].min(next_boundary_m.max(left_m + f64::EPSILON));
        enclose_contact_position_cell(
            &mut enclosure,
            cubic_segment(samples, source_index),
            interval_cubic_segment(samples, source_index),
            center_frame,
            meters_per_frame,
            radius_m,
            search.best_height_m,
            left_m,
            right_m,
            0,
        );
        left_m = right_m;
    }
    assert!(enclosure.left_m.is_finite() && enclosure.right_m.is_finite());
    assert!(enclosure.left_m <= search.best_offset_m);
    assert!(enclosure.right_m >= search.best_offset_m);

    let contact_frame = center_frame + search.best_offset_m / meters_per_frame;
    let source_index = contact_frame.floor() as isize;
    let segment = cubic_segment(samples, source_index);
    let fraction = contact_frame - source_index as f64;
    let groove_displacement_m = segment.value(fraction);
    let groove_slope = segment.slope_per_frame(fraction) / meters_per_frame;
    let circle_slope =
        search.best_offset_m / circle_height(radius_m, search.best_offset_m).max(f64::MIN_POSITIVE);
    OutwardIntervalTrace {
        sample: StylusTraceSample {
            center_displacement_m: (search.best_height_m - radius_m).next_down(),
            contact_offset_m: search.best_offset_m,
            groove_displacement_m,
            groove_slope,
            tangent_residual: groove_slope - circle_slope,
        },
        height_error_bound_m,
        contact_offset_enclosure_m: [enclosure.left_m, enclosure.right_m],
        searched_cells: search.searched_cells + enclosure.searched_cells,
    }
}

#[derive(Debug, Clone, Copy)]
struct VerticalReferenceState {
    tip_displacement_m: f64,
    tip_velocity_m_s: f64,
    body_displacement_m: f64,
    body_velocity_m_s: f64,
}

#[derive(Debug, Clone, Copy)]
struct VerticalReferenceOutput {
    vertical_normal_force_n: f64,
    wall_normal_force_sum_n: f64,
    record_reaction_torque_nm: f64,
    contact: bool,
}

impl VerticalReferenceState {
    fn equilibrium(tonearm: TonearmConfig) -> Self {
        let stiffness = tonearm.vertical.stiffness_n_per_m();
        Self {
            tip_displacement_m: 0.0,
            tip_velocity_m_s: 0.0,
            body_displacement_m: -tonearm.vertical_tracking_force_n / stiffness,
            body_velocity_m_s: 0.0,
        }
    }

    fn step(
        &mut self,
        contact: StylusContactConfig,
        tonearm: TonearmConfig,
        wall_height_m: f64,
        vertical_wall_slope: f64,
        tangential_velocity_m_s: f64,
        dt_seconds: f64,
    ) -> VerticalReferenceOutput {
        let axis = tonearm.vertical;
        let stiffness = axis.stiffness_n_per_m();
        let damping = axis.viscous_damping_n_s_per_m();
        let coupling = stiffness * dt_seconds + damping;
        let tip_mass_rate = contact.moving_mass_kg / dt_seconds;
        let body_mass_rate = axis.effective_mass_kg / dt_seconds;
        let relative_displacement_m = self.tip_displacement_m - self.body_displacement_m;
        let tip_rhs = tip_mass_rate * self.tip_velocity_m_s - stiffness * relative_displacement_m;
        let body_rhs = body_mass_rate * self.body_velocity_m_s
            + stiffness * relative_displacement_m
            - tonearm.vertical_tracking_force_n;
        let tip_diagonal = tip_mass_rate + coupling;
        let body_diagonal = body_mass_rate + coupling;
        let determinant = tip_diagonal * body_diagonal - coupling * coupling;
        let free_tip_velocity_m_s = (tip_rhs * body_diagonal + coupling * body_rhs) / determinant;
        let free_body_velocity_m_s = (body_rhs * tip_diagonal + coupling * tip_rhs) / determinant;
        let free_tip_displacement_m = self.tip_displacement_m + free_tip_velocity_m_s * dt_seconds;

        let (tip_velocity_m_s, body_velocity_m_s, vertical_normal_force_n) =
            if free_tip_displacement_m >= wall_height_m {
                (free_tip_velocity_m_s, free_body_velocity_m_s, 0.0)
            } else {
                let constrained_tip_velocity_m_s =
                    (wall_height_m - self.tip_displacement_m) / dt_seconds;
                let constrained_body_velocity_m_s =
                    (body_rhs + coupling * constrained_tip_velocity_m_s) / body_diagonal;
                let required_normal_force_n = tip_diagonal * constrained_tip_velocity_m_s
                    - coupling * constrained_body_velocity_m_s
                    - tip_rhs;
                if required_normal_force_n >= 0.0 {
                    (
                        constrained_tip_velocity_m_s,
                        constrained_body_velocity_m_s,
                        required_normal_force_n,
                    )
                } else {
                    (free_tip_velocity_m_s, free_body_velocity_m_s, 0.0)
                }
            };

        self.tip_velocity_m_s = tip_velocity_m_s;
        self.body_velocity_m_s = body_velocity_m_s;
        self.tip_displacement_m += tip_velocity_m_s * dt_seconds;
        self.body_displacement_m += body_velocity_m_s * dt_seconds;

        let wall_slope = vertical_wall_slope * WALL_SCALE;
        let wall_normal_force_sum_n =
            std::f64::consts::SQRT_2 * vertical_normal_force_n * wall_slope.hypot(1.0);
        let direction = if tangential_velocity_m_s > 0.0 {
            1.0
        } else if tangential_velocity_m_s < 0.0 {
            -1.0
        } else {
            0.0
        };
        let coulomb_force_n =
            -direction * contact.groove_friction_coefficient * wall_normal_force_sum_n;
        let modulation_force_n = -vertical_normal_force_n * vertical_wall_slope;
        VerticalReferenceOutput {
            vertical_normal_force_n,
            wall_normal_force_sum_n,
            record_reaction_torque_nm: (coulomb_force_n + modulation_force_n) * GROOVE_RADIUS_M,
            contact: vertical_normal_force_n > 0.0,
        }
    }
}

fn candidate_trace(
    selection: GrooveSpatialLevelSelection<'_>,
    wall_index: usize,
    center_frame: f64,
    geometry: StylusGeometry,
) -> Result<StylusTraceSample, StylusTraceError> {
    let lower = selection.lower();
    let upper = selection.upper();
    trace_spherical_45_45_wall_multiresolution(
        lower.lateral_displacement_m(),
        lower.vertical_displacement_m(),
        lower.first_source_frame(),
        lower.source_frame_step(),
        upper.lateral_displacement_m(),
        upper.vertical_displacement_m(),
        upper.first_source_frame(),
        upper.source_frame_step(),
        selection.upper_level_blend(),
        wall_index,
        center_frame,
        METERS_PER_SOURCE_FRAME,
        geometry,
    )
}

fn equilibrium_candidate(
    contact: StylusContactConfig,
    tonearm: TonearmConfig,
) -> PickupMechanicalState {
    let mut state = PickupMechanicalState::new(contact, tonearm, OUTPUT_SAMPLE_RATE_HZ).unwrap();
    state
        .reset(
            [0.0, 0.0],
            [0.0; 2],
            [
                0.0,
                -tonearm.vertical_tracking_force_n / tonearm.vertical.stiffness_n_per_m(),
            ],
            [0.0; 2],
        )
        .unwrap();
    state
}

#[derive(Debug, Clone, Copy, Default)]
struct ErrorAccumulator {
    squared_error: f64,
    squared_reference: f64,
    maximum_absolute_error: f64,
    maximum_reference: f64,
    candidate_integral: f64,
    reference_integral: f64,
    candidate_absolute_integral: f64,
    reference_absolute_integral: f64,
    count: u64,
}

impl ErrorAccumulator {
    fn observe(&mut self, candidate: f64, reference: f64, dt_seconds: f64) {
        let error = candidate - reference;
        self.squared_error += error * error;
        self.squared_reference += reference * reference;
        self.maximum_absolute_error = self.maximum_absolute_error.max(error.abs());
        self.maximum_reference = self.maximum_reference.max(reference.abs());
        self.candidate_integral += candidate * dt_seconds;
        self.reference_integral += reference * dt_seconds;
        self.candidate_absolute_integral += candidate.abs() * dt_seconds;
        self.reference_absolute_integral += reference.abs() * dt_seconds;
        self.count += 1;
    }

    fn normalized_rms_error(self) -> f64 {
        (self.squared_error / self.squared_reference.max(f64::MIN_POSITIVE)).sqrt()
    }

    fn relative_integral_error(self) -> f64 {
        (self.candidate_integral - self.reference_integral).abs()
            / self.reference_integral.abs().max(f64::MIN_POSITIVE)
    }

    fn relative_absolute_integral_error(self) -> f64 {
        (self.candidate_absolute_integral - self.reference_absolute_integral).abs()
            / self.reference_absolute_integral.max(f64::MIN_POSITIVE)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RapidScratchMetrics {
    wall_height: ErrorAccumulator,
    wall_normal_force: ErrorAccumulator,
    reaction_torque: ErrorAccumulator,
    contact_occupancy_absolute_error: f64,
    maximum_contact_occupancy_error: f64,
    candidate_contact_transitions: u64,
    reference_contact_transitions: u64,
    candidate_trace_calls: u64,
    candidate_contact_steps: u64,
    reference_trace_calls: u64,
    reference_contact_steps: u64,
    reference_substeps_max: usize,
    candidate_peak_wall_height_m: f64,
    reference_peak_substep_wall_height_m: f64,
    candidate_peak_wall_normal_force_n: f64,
    reference_peak_substep_wall_normal_force_n: f64,
    candidate_peak_reaction_torque_nm: f64,
    reference_peak_substep_reaction_torque_nm: f64,
}

#[derive(Debug)]
struct RapidScratchFixture {
    zero_lateral_m: Vec<f32>,
    vertical_m: Vec<f32>,
    wall_m: Vec<f32>,
    pyramid: GrooveSpatialPyramid,
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
enum RapidScratchCaseError {
    #[error("candidate trace failed at macro step {macro_index}, wall {wall_index}: {source}")]
    CandidateTrace {
        macro_index: usize,
        wall_index: usize,
        source: StylusTraceError,
    },
    #[error(
        "reference trace failed at macro step {macro_index}, substep {substep_index}: {source}"
    )]
    ReferenceTrace {
        macro_index: usize,
        substep_index: usize,
        source: StylusTraceError,
    },
}

impl RapidScratchFixture {
    fn new(vertical_m: Vec<f32>) -> Self {
        let zero_lateral_m = vec![0.0; vertical_m.len()];
        let wall_m = vertical_m
            .iter()
            .map(|value| *value * WALL_SCALE as f32)
            .collect::<Vec<_>>();
        let pyramid = GrooveSpatialPyramid::build(&zero_lateral_m, &vertical_m).unwrap();
        Self {
            zero_lateral_m,
            vertical_m,
            wall_m,
            pyramid,
        }
    }
}

fn run_rapid_scratch_case(
    fixture: &RapidScratchFixture,
    advances_source_frames: &[f64],
    initial_position_source_frames: f64,
    reference_step_frames: f64,
) -> Result<RapidScratchMetrics, RapidScratchCaseError> {
    let contact = StylusContactConfig::default();
    let tonearm = TonearmConfig::default();
    let geometry = StylusGeometry::default();
    let mut candidate = equilibrium_candidate(contact, tonearm);
    let mut reference = VerticalReferenceState::equilibrium(tonearm);
    let mut metrics = RapidScratchMetrics::default();
    let mut position = initial_position_source_frames;
    let mut previous_candidate_contact = true;
    let mut previous_reference_contact = true;

    for (macro_index, &advance) in advances_source_frames.iter().enumerate() {
        let next_position = position + advance;
        assert!(next_position >= 64.0);
        assert!(next_position <= fixture.vertical_m.len() as f64 - 65.0);
        let selection = fixture
            .pyramid
            .select(0, &fixture.zero_lateral_m, &fixture.vertical_m, advance)
            .unwrap();
        let candidate_walls = [
            candidate_trace(selection, 0, next_position, geometry).map_err(|source| {
                RapidScratchCaseError::CandidateTrace {
                    macro_index,
                    wall_index: 0,
                    source,
                }
            })?,
            candidate_trace(selection, 1, next_position, geometry).map_err(|source| {
                RapidScratchCaseError::CandidateTrace {
                    macro_index,
                    wall_index: 1,
                    source,
                }
            })?,
        ];
        metrics.candidate_trace_calls += 2;
        let tangential_velocity_m_s = advance * METERS_PER_SOURCE_FRAME / OUTPUT_DT_SECONDS;
        let candidate_output = candidate
            .process(PickupMechanicalInput {
                wall_contacts: test_single_wall_contacts(
                    [
                        candidate_walls[0].center_displacement_m,
                        candidate_walls[1].center_displacement_m,
                    ],
                    [
                        candidate_walls[0].groove_slope,
                        candidate_walls[1].groove_slope,
                    ],
                ),
                contact_surface: PickupContactSurface::GrooveWalls,
                groove_radius_m: GROOVE_RADIUS_M,
                groove_tangential_velocity_m_s: tangential_velocity_m_s,
                stylus_lowered: true,
                ..PickupMechanicalInput::default()
            })
            .unwrap();
        metrics.candidate_contact_steps += 1;
        metrics.candidate_peak_wall_height_m = metrics
            .candidate_peak_wall_height_m
            .max((std::f64::consts::SQRT_2 * candidate_walls[0].center_displacement_m).abs());
        metrics.candidate_peak_wall_normal_force_n = metrics
            .candidate_peak_wall_normal_force_n
            .max(candidate_output.wall_normal_force_n.iter().sum());
        metrics.candidate_peak_reaction_torque_nm = metrics
            .candidate_peak_reaction_torque_nm
            .max(candidate_output.record_reaction_torque_nm().abs());

        let substeps = MIN_REFERENCE_SUBSTEPS
            .max((advance.abs() / reference_step_frames).ceil().max(1.0) as usize);
        metrics.reference_substeps_max = metrics.reference_substeps_max.max(substeps);
        let substep_dt = OUTPUT_DT_SECONDS / substeps as f64;
        let mut reference_force_sum = 0.0;
        let mut reference_torque_sum = 0.0;
        let mut reference_height_sum = 0.0;
        let mut reference_contact_count = 0_usize;
        for substep in 0..substeps {
            let fraction = (substep + 1) as f64 / substeps as f64;
            let substep_position = position + advance * fraction;
            let trace = trace_spherical_uniform(
                &fixture.wall_m,
                substep_position,
                METERS_PER_SOURCE_FRAME,
                geometry,
            )
            .map_err(|source| RapidScratchCaseError::ReferenceTrace {
                macro_index,
                substep_index: substep,
                source,
            })?;
            metrics.reference_trace_calls += 2;
            let reference_output = reference.step(
                contact,
                tonearm,
                std::f64::consts::SQRT_2 * trace.center_displacement_m,
                std::f64::consts::SQRT_2 * trace.groove_slope,
                tangential_velocity_m_s,
                substep_dt,
            );
            metrics.reference_contact_steps += 1;
            reference_force_sum += reference_output.wall_normal_force_sum_n;
            reference_torque_sum += reference_output.record_reaction_torque_nm;
            reference_height_sum += std::f64::consts::SQRT_2 * trace.center_displacement_m;
            reference_contact_count += usize::from(reference_output.contact);
            metrics.reference_peak_substep_wall_height_m = metrics
                .reference_peak_substep_wall_height_m
                .max((std::f64::consts::SQRT_2 * trace.center_displacement_m).abs());
            metrics.reference_peak_substep_wall_normal_force_n = metrics
                .reference_peak_substep_wall_normal_force_n
                .max(reference_output.wall_normal_force_sum_n);
            metrics.reference_peak_substep_reaction_torque_nm = metrics
                .reference_peak_substep_reaction_torque_nm
                .max(reference_output.record_reaction_torque_nm.abs());
            if reference_output.contact != previous_reference_contact {
                metrics.reference_contact_transitions += 1;
                previous_reference_contact = reference_output.contact;
            }
        }
        let inverse_substeps = 1.0 / substeps as f64;
        let reference_force = reference_force_sum * inverse_substeps;
        let reference_torque = reference_torque_sum * inverse_substeps;
        let reference_height = reference_height_sum * inverse_substeps;
        let reference_occupancy = reference_contact_count as f64 * inverse_substeps;
        let candidate_contact = candidate_output.wall_contact.into_iter().any(|value| value);
        if candidate_contact != previous_candidate_contact {
            metrics.candidate_contact_transitions += 1;
            previous_candidate_contact = candidate_contact;
        }
        let candidate_occupancy = f64::from(candidate_contact);
        let occupancy_error = (candidate_occupancy - reference_occupancy).abs();
        metrics.contact_occupancy_absolute_error += occupancy_error;
        metrics.maximum_contact_occupancy_error =
            metrics.maximum_contact_occupancy_error.max(occupancy_error);
        metrics.wall_height.observe(
            std::f64::consts::SQRT_2 * candidate_walls[0].center_displacement_m,
            reference_height,
            OUTPUT_DT_SECONDS,
        );
        metrics.wall_normal_force.observe(
            candidate_output.wall_normal_force_n.iter().sum(),
            reference_force,
            OUTPUT_DT_SECONDS,
        );
        metrics.reaction_torque.observe(
            candidate_output.record_reaction_torque_nm(),
            reference_torque,
            OUTPUT_DT_SECONDS,
        );
        position = next_position;
    }
    metrics.contact_occupancy_absolute_error /= advances_source_frames.len() as f64;
    Ok(metrics)
}

fn programme_fixture(frame_count: usize) -> RapidScratchFixture {
    let center = frame_count as f64 * 0.5;
    let vertical_m = (0..frame_count)
        .map(|frame| {
            let x = frame as f64;
            let chirp_phase = 0.012 * (x - center).powi(2);
            let multitone = 2.0e-6 * (std::f64::consts::TAU * x / 31.7).sin()
                + 0.8e-6 * (std::f64::consts::TAU * x / 7.3).sin()
                + 0.45e-6 * chirp_phase.sin();
            let impulse = 9.0e-6 * (-0.5 * ((x - center - 173.4) / 0.55).powi(2)).exp();
            let contact_drop = -45.0e-6 * (-0.5 * ((x - center + 137.0) / 4.0).powi(2)).exp();
            let retrack_ridge = 16.0e-6 * (-0.5 * ((x - center + 121.0) / 1.4).powi(2)).exp();
            (multitone + impulse + contact_drop + retrack_ridge) as f32
        })
        .collect();
    RapidScratchFixture::new(vertical_m)
}

fn rate_sweep_advances() -> Vec<f64> {
    let mut advances = Vec::new();
    for rate in 1..=20 {
        advances.extend(std::iter::repeat_n(rate as f64, 9));
        advances.extend(std::iter::repeat_n(-(rate as f64), 18));
        advances.extend(std::iter::repeat_n(rate as f64, 9));
    }
    advances
}

fn validation_layout() -> GrooveLayout {
    GrooveLayout {
        outer_program_radius_m: GROOVE_RADIUS_M,
        inner_program_radius_m: 0.060_325,
        nominal_rpm: 33.333_333_333_333_336,
        groove_sample_rate_hz: OUTPUT_SAMPLE_RATE_HZ,
    }
}

fn spatial_asset_for_wall(samples: &[f32]) -> GrooveAsset {
    let layout = validation_layout();
    let cut = RecordCutConfig::default();
    let lateral = vec![0.0; samples.len()];
    let vertical = samples
        .iter()
        .map(|sample| (std::f64::consts::SQRT_2 * f64::from(*sample)) as f32)
        .collect::<Vec<_>>();
    let peak_vertical_displacement_m = vertical
        .iter()
        .map(|sample| f64::from(*sample).abs())
        .fold(0.0, f64::max);
    let wall_bounds = cubic_kinematic_bounds(samples);
    let peak_wall_velocity_m_s = wall_bounds.peak_velocity_m_s(layout.groove_sample_rate_hz);
    let final_program_radius_m = layout.unclamped_radius_at_frame(
        samples.len().saturating_sub(1) as f64,
        cut.groove_pitch_m_per_revolution,
    );
    let report = GrooveCutReport {
        peak_left_velocity_m_s: peak_wall_velocity_m_s,
        peak_right_velocity_m_s: peak_wall_velocity_m_s,
        // The direct spatial API accepts caller-supplied report values. It
        // does not derive an RMS velocity from displacement samples.
        rms_left_velocity_m_s: 0.0,
        rms_right_velocity_m_s: 0.0,
        peak_lateral_displacement_m: 0.0,
        peak_vertical_displacement_m,
        final_lateral_drift_m: 0.0,
        final_vertical_drift_m: f64::from(*vertical.last().unwrap()),
        groove_pitch_m_per_revolution: cut.groove_pitch_m_per_revolution,
        final_program_radius_m,
        programme_exceeds_available_radius: final_program_radius_m < layout.inner_program_radius_m,
        minimum_adjacent_turn_clearance_m: None,
        first_failing_clearance_frame_pair: None,
        adjacent_turn_clearance_failed: false,
    };
    GrooveAsset::from_displacement_m_with_cut(lateral, vertical, layout, cut, report).unwrap()
}

fn seed_domain_asset() -> GrooveAsset {
    let left_velocity = SEED_DOMAIN_WALL_VELOCITY_BITS.map(f32::from_bits);
    let right_velocity = left_velocity.map(|velocity| -velocity);
    GrooveAsset::from_stereo_wall_velocity_m_s(&left_velocity, &right_velocity, validation_layout())
        .unwrap()
}

fn vertical_asset_wall(asset: &GrooveAsset) -> Vec<f32> {
    asset
        .vertical_displacement_m()
        .iter()
        .map(|sample| *sample * WALL_SCALE as f32)
        .collect()
}

fn assert_trace_enclosure_contains_best_sample(reference: OutwardIntervalTrace) {
    assert!(reference.height_error_bound_m <= TRACE_HEIGHT_CERTIFICATE_M);
    assert!(
        reference.contact_offset_enclosure_m[0] <= reference.sample.contact_offset_m
            && reference.sample.contact_offset_m <= reference.contact_offset_enclosure_m[1]
    );
}

fn assert_trace_artifact_matches(
    artifact: &serde_json::Value,
    replacement: StylusTraceSample,
    reference: OutwardIntervalTrace,
) {
    for (field, expected) in [
        ("centerDisplacementM", replacement.center_displacement_m),
        ("contactOffsetM", replacement.contact_offset_m),
        ("grooveDisplacementM", replacement.groove_displacement_m),
        ("grooveSlope", replacement.groove_slope),
        ("tangentResidual", replacement.tangent_residual),
    ] {
        assert_eq!(
            artifact["replacement"][field].as_f64().unwrap().to_bits(),
            expected.to_bits(),
            "replacement.{field}",
        );
    }
    let recorded_reference = &artifact["outwardIntervalReference"];
    for (field, expected) in [
        (
            "centerDisplacementLowerM",
            reference.sample.center_displacement_m,
        ),
        ("heightErrorBoundM", reference.height_error_bound_m),
        ("chosenContactOffsetM", reference.sample.contact_offset_m),
    ] {
        assert_eq!(
            recorded_reference[field].as_f64().unwrap().to_bits(),
            expected.to_bits(),
            "outwardIntervalReference.{field}",
        );
    }
    for (recorded, expected) in recorded_reference["contactOffsetEnclosureM"]
        .as_array()
        .unwrap()
        .iter()
        .zip(reference.contact_offset_enclosure_m)
    {
        assert_eq!(recorded.as_f64().unwrap().to_bits(), expected.to_bits());
    }
    assert_eq!(
        recorded_reference["searchedCells"].as_u64().unwrap(),
        reference.searched_cells,
    );
    for (field, expected) in [
        (
            "grooveDisplacementM",
            reference.sample.groove_displacement_m,
        ),
        ("grooveSlope", reference.sample.groove_slope),
        ("tangentResidual", reference.sample.tangent_residual),
    ] {
        if let Some(recorded) = recorded_reference.get(field) {
            assert_eq!(
                recorded.as_f64().unwrap().to_bits(),
                expected.to_bits(),
                "outwardIntervalReference.{field}",
            );
        }
    }
}

fn assert_pvc_001_artifact_matches_fixture(expected_bits: [u32; 34]) {
    let artifact: serde_json::Value = serde_json::from_str(PVC_001_SPATIAL_ARTIFACT).unwrap();
    assert_eq!(artifact["caseId"], "PVC-001");
    let bits = artifact["wallDisplacementF32BitsHex"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| u32::from_str_radix(value.as_str().unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(bits, expected_bits);
    for (value, expected) in artifact["wallDisplacementM"]
        .as_array()
        .unwrap()
        .iter()
        .zip(expected_bits)
    {
        assert_eq!((value.as_f64().unwrap() as f32).to_bits(), expected);
    }
    for (value_field, bits_field, expected) in [
        ("centerFrame", "centerFrameF64BitsHex", 16.37_f64),
        (
            "metersPerSourceFrame",
            "metersPerSourceFrameF64BitsHex",
            METERS_PER_SOURCE_FRAME,
        ),
        (
            "tracingRadiusM",
            "tracingRadiusF64BitsHex",
            StylusGeometry::default().tracing_radius_m,
        ),
        (
            "nominalGrooveRadiusM",
            "nominalGrooveRadiusF64BitsHex",
            GROOVE_RADIUS_M,
        ),
    ] {
        assert_eq!(
            artifact[value_field].as_f64().unwrap().to_bits(),
            expected.to_bits()
        );
        assert_eq!(
            u64::from_str_radix(artifact[bits_field].as_str().unwrap(), 16).unwrap(),
            expected.to_bits()
        );
    }
}

fn assert_pvc_001_wall_velocity_artifact_matches(asset: &GrooveAsset, wall: &[f32]) {
    let artifact: serde_json::Value = serde_json::from_str(PVC_001_WALL_VELOCITY_ARTIFACT).unwrap();
    assert_eq!(artifact["caseId"], "PVC-001");
    let velocity_bits = artifact["leftWallVelocityF32BitsHex"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| u32::from_str_radix(value.as_str().unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(velocity_bits, SEED_DOMAIN_WALL_VELOCITY_BITS);
    let wall_bits = artifact["generatedScalarWallDisplacementF32BitsHex"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| u32::from_str_radix(value.as_str().unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        wall_bits,
        wall.iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        artifact["centerFrame"].as_f64().unwrap().to_bits(),
        40.37_f64.to_bits()
    );
    assert_eq!(
        artifact["metersPerSourceFrame"].as_f64().unwrap().to_bits(),
        METERS_PER_SOURCE_FRAME.to_bits()
    );
    let report = asset.report();
    for (field, expected) in [
        ("peakLeftVelocityMPerS", report.peak_left_velocity_m_s),
        ("peakRightVelocityMPerS", report.peak_right_velocity_m_s),
        ("rmsLeftVelocityMPerS", report.rms_left_velocity_m_s),
        ("rmsRightVelocityMPerS", report.rms_right_velocity_m_s),
        (
            "peakVerticalDisplacementM",
            report.peak_vertical_displacement_m,
        ),
        ("finalVerticalDriftM", report.final_vertical_drift_m),
        ("finalProgramRadiusM", report.final_program_radius_m),
    ] {
        assert_eq!(
            artifact["cutReport"][field].as_f64().unwrap().to_bits(),
            expected.to_bits()
        );
    }
    let bounds = cubic_kinematic_bounds(wall);
    for (field, expected) in [
        ("peakDisplacementM", bounds.peak_displacement_m),
        ("peakAdjacentStepM", bounds.peak_adjacent_step_m),
        ("peakSlopeMPerFrame", bounds.peak_slope_per_frame_m),
        (
            "peakDimensionlessSlope",
            bounds.peak_dimensionless_slope(METERS_PER_SOURCE_FRAME),
        ),
        (
            "peakWallVelocityMPerSAt192kHz",
            bounds.peak_velocity_m_s(OUTPUT_SAMPLE_RATE_HZ),
        ),
        (
            "peakCurvatureMPerFrameSquared",
            bounds.peak_curvature_per_frame_squared_m,
        ),
        (
            "peakWallAccelerationMPerS2At192kHz",
            bounds.peak_acceleration_m_s2(OUTPUT_SAMPLE_RATE_HZ),
        ),
    ] {
        assert_eq!(
            artifact["generatedWallKinematics"][field]
                .as_f64()
                .unwrap()
                .to_bits(),
            expected.to_bits(),
            "generatedWallKinematics.{field}",
        );
    }
}

#[test]
fn pvc_001_replacement_contains_global_envelope_for_catmull_rom_fixture() {
    const EXPECTED_BITS: [u32; 34] = [
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
    assert_eq!(
        PLAUSIBLE_ENVELOPE_COUNTEREXAMPLE.map(f32::to_bits),
        EXPECTED_BITS
    );
    assert_pvc_001_artifact_matches_fixture(EXPECTED_BITS);
    let geometry = StylusGeometry {
        tracing_radius_m: 18.0e-6,
    };
    let center_frame = 16.37;
    let replacement = trace_spherical_uniform(
        &PLAUSIBLE_ENVELOPE_COUNTEREXAMPLE,
        center_frame,
        METERS_PER_SOURCE_FRAME,
        geometry,
    )
    .unwrap();
    let reference = outward_interval_trace_spherical_uniform(
        &PLAUSIBLE_ENVELOPE_COUNTEREXAMPLE,
        center_frame,
        METERS_PER_SOURCE_FRAME,
        geometry,
        TRACE_HEIGHT_CERTIFICATE_M,
    );
    let artifact: serde_json::Value = serde_json::from_str(PVC_001_SPATIAL_ARTIFACT).unwrap();
    assert_trace_artifact_matches(&artifact, replacement, reference);
    assert_trace_enclosure_contains_best_sample(reference);
    assert!(reference.searched_cells <= 32_768);
    assert!(
        reference.contact_offset_enclosure_m[0] <= replacement.contact_offset_m
            && replacement.contact_offset_m <= reference.contact_offset_enclosure_m[1]
    );
    assert!(
        reference.sample.center_displacement_m <= replacement.center_displacement_m
            && replacement.center_displacement_m
                <= reference.sample.center_displacement_m + reference.height_error_bound_m
    );
    let bounds = cubic_kinematic_bounds(&PLAUSIBLE_ENVELOPE_COUNTEREXAMPLE);
    assert!(bounds.peak_displacement_m < 1.7e-6);
    assert!(bounds.peak_adjacent_step_m < 0.4e-6);
    assert!(bounds.peak_dimensionless_slope(METERS_PER_SOURCE_FRAME) < 0.31);

    let accepted = spatial_asset_for_wall(&PLAUSIBLE_ENVELOPE_COUNTEREXAMPLE);
    assert_eq!(
        accepted.provenance().source().kind(),
        GrooveSourceKind::SpatialDisplacement
    );
    assert!(!accepted.report().adjacent_turn_clearance_failed);
}

#[test]
fn adversarial_tracer_domain_case_selects_the_certified_global_envelope() {
    let geometry = StylusGeometry::default();
    let center_frame = 16.37;
    let replacement = trace_spherical_uniform(
        &ADVERSARIAL_ENVELOPE_COUNTEREXAMPLE,
        center_frame,
        METERS_PER_SOURCE_FRAME,
        geometry,
    )
    .unwrap();
    let reference = outward_interval_trace_spherical_uniform(
        &ADVERSARIAL_ENVELOPE_COUNTEREXAMPLE,
        center_frame,
        METERS_PER_SOURCE_FRAME,
        geometry,
        TRACE_HEIGHT_CERTIFICATE_M,
    );
    assert_trace_enclosure_contains_best_sample(reference);
    assert!(
        reference.sample.center_displacement_m <= replacement.center_displacement_m
            && replacement.center_displacement_m
                <= reference.sample.center_displacement_m + reference.height_error_bound_m
    );
    assert!(
        reference.contact_offset_enclosure_m[0] <= replacement.contact_offset_m
            && replacement.contact_offset_m <= reference.contact_offset_enclosure_m[1]
    );

    let mut dense_height_m = f64::NEG_INFINITY;
    let mut dense_offset_m = 0.0;
    for index in 0..=1_048_576 {
        let offset_m = -geometry.tracing_radius_m
            + 2.0 * geometry.tracing_radius_m * index as f64 / 1_048_576.0;
        let height_m = cubic_value_at(
            &ADVERSARIAL_ENVELOPE_COUNTEREXAMPLE,
            center_frame + offset_m / METERS_PER_SOURCE_FRAME,
        ) + circle_height(geometry.tracing_radius_m, offset_m)
            - geometry.tracing_radius_m;
        if height_m > dense_height_m {
            dense_height_m = height_m;
            dense_offset_m = offset_m;
        }
    }
    assert!((replacement.center_displacement_m - dense_height_m).abs() < 1.0e-12);
    assert!((replacement.contact_offset_m - dense_offset_m).abs() < 1.0e-10);
    assert!(
        reference.contact_offset_enclosure_m[0] <= dense_offset_m
            && dense_offset_m <= reference.contact_offset_enclosure_m[1]
    );
    let bounds = cubic_kinematic_bounds(&ADVERSARIAL_ENVELOPE_COUNTEREXAMPLE);
    assert!(bounds.peak_velocity_m_s(OUTPUT_SAMPLE_RATE_HZ) > 16.0);
    assert!(bounds.peak_acceleration_m_s2(OUTPUT_SAMPLE_RATE_HZ) > 11.0e6);
}

#[test]
fn pvc_001_replacement_contains_seed_domain_global_envelope() {
    let asset = seed_domain_asset();
    assert_eq!(
        asset.provenance().source().kind(),
        GrooveSourceKind::StereoWallVelocity
    );
    let cut = asset.provenance().cut();
    let declared_sine_peak_m_s = cut.full_scale_sine_velocity_rms_m_s * std::f64::consts::SQRT_2;
    assert!(asset.report().peak_left_velocity_m_s < declared_sine_peak_m_s);
    assert!(asset.report().peak_right_velocity_m_s < declared_sine_peak_m_s);
    assert!(!asset.report().adjacent_turn_clearance_failed);

    let wall = vertical_asset_wall(&asset);
    assert_pvc_001_wall_velocity_artifact_matches(&asset, &wall);
    let geometry = StylusGeometry::default();
    let replacement =
        trace_spherical_uniform(&wall, 40.37, METERS_PER_SOURCE_FRAME, geometry).unwrap();
    let reference = outward_interval_trace_spherical_uniform(
        &wall,
        40.37,
        METERS_PER_SOURCE_FRAME,
        geometry,
        TRACE_HEIGHT_CERTIFICATE_M,
    );
    let artifact: serde_json::Value = serde_json::from_str(PVC_001_WALL_VELOCITY_ARTIFACT).unwrap();
    assert_trace_artifact_matches(&artifact, replacement, reference);
    assert_trace_enclosure_contains_best_sample(reference);
    assert!(
        reference.sample.center_displacement_m <= replacement.center_displacement_m
            && replacement.center_displacement_m
                <= reference.sample.center_displacement_m + reference.height_error_bound_m
    );
    assert!(
        reference.contact_offset_enclosure_m[0] <= replacement.contact_offset_m
            && replacement.contact_offset_m <= reference.contact_offset_enclosure_m[1]
    );
}

#[test]
fn pvc_001_outward_interval_enclosure_is_reciprocal_and_contains_a_dense_search() {
    let asset = seed_domain_asset();
    let wall = vertical_asset_wall(&asset);
    let center_frame = 40.37;
    let geometry = StylusGeometry::default();
    let forward = outward_interval_trace_spherical_uniform(
        &wall,
        center_frame,
        METERS_PER_SOURCE_FRAME,
        geometry,
        TRACE_HEIGHT_CERTIFICATE_M,
    );
    let mut reversed_wall = wall.clone();
    reversed_wall.reverse();
    let reversed = outward_interval_trace_spherical_uniform(
        &reversed_wall,
        wall.len() as f64 - 1.0 - center_frame,
        METERS_PER_SOURCE_FRAME,
        geometry,
        TRACE_HEIGHT_CERTIFICATE_M,
    );
    let height_difference_m =
        (forward.sample.center_displacement_m - reversed.sample.center_displacement_m).abs();
    assert!(height_difference_m <= forward.height_error_bound_m + reversed.height_error_bound_m);
    let reflected_reversed_enclosure_m = [
        -reversed.contact_offset_enclosure_m[1],
        -reversed.contact_offset_enclosure_m[0],
    ];
    assert!(
        forward.contact_offset_enclosure_m[0].max(reflected_reversed_enclosure_m[0])
            <= forward.contact_offset_enclosure_m[1].min(reflected_reversed_enclosure_m[1]),
        "forward={:?}, reflected reversed={reflected_reversed_enclosure_m:?}",
        forward.contact_offset_enclosure_m,
    );

    let mut dense_height_m = f64::NEG_INFINITY;
    let mut dense_offset_m = 0.0;
    for index in 0..=262_144 {
        let offset_m =
            -geometry.tracing_radius_m + 2.0 * geometry.tracing_radius_m * index as f64 / 262_144.0;
        let height_m = cubic_value_at(&wall, center_frame + offset_m / METERS_PER_SOURCE_FRAME)
            + circle_height(geometry.tracing_radius_m, offset_m)
            - geometry.tracing_radius_m;
        if height_m > dense_height_m {
            dense_height_m = height_m;
            dense_offset_m = offset_m;
        }
    }
    // The uniform grid need not land on the branch-and-bound candidate.
    // This tolerance is only a dense-search cross-check, not an enclosure bound.
    assert!(dense_height_m + 1.0e-15 >= forward.sample.center_displacement_m);
    assert!(
        dense_height_m
            <= forward.sample.center_displacement_m + forward.height_error_bound_m + 1.0e-18
    );
    assert!(
        forward.contact_offset_enclosure_m[0] <= dense_offset_m
            && dense_offset_m <= forward.contact_offset_enclosure_m[1]
    );
}

fn cubic_value_at(samples: &[f32], position: f64) -> f64 {
    let source_index = position.floor() as isize;
    cubic_segment(samples, source_index).value(position - source_index as f64)
}

fn pvc_004_inner_groove_sine() -> (Vec<f32>, f64) {
    let sample_rate_hz = 192_000.0;
    let frequency_hz = 8_000.0;
    let velocity_peak_m_s = 0.05;
    let amplitude_m = velocity_peak_m_s / (std::f64::consts::TAU * frequency_hz);
    let samples = (0..4_096)
        .map(|index| {
            (amplitude_m
                * (std::f64::consts::TAU * frequency_hz * index as f64 / sample_rate_hz).sin())
                as f32
        })
        .collect();
    let angular_velocity_rad_s = std::f64::consts::TAU * (33.333_333_333_333_336 / 60.0);
    let meters_per_frame = angular_velocity_rad_s * 0.060 / sample_rate_hz;
    (samples, meters_per_frame)
}

#[test]
fn pvc_004_inner_groove_sine_has_unresolved_separated_height_candidates() {
    let (samples, meters_per_frame) = pvc_004_inner_groove_sine();
    let artifact: serde_json::Value = serde_json::from_str(PVC_004_HEIGHT_ORDER_ARTIFACT).unwrap();
    assert_eq!(artifact["caseId"], "PVC-004");
    assert_eq!(
        artifact["trace"]["metersPerSourceFrame"]
            .as_f64()
            .unwrap()
            .to_bits(),
        meters_per_frame.to_bits()
    );
    let support = &artifact["support"];
    let first_frame = support["firstFrame"].as_u64().unwrap() as usize;
    let expected_bits = support["wallDisplacementF32BitsHex"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| u32::from_str_radix(value.as_str().unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        samples[first_frame..first_frame + expected_bits.len()]
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        expected_bits
    );
    let geometry = StylusGeometry::default();
    let center_frame = 258.0;
    let radius_m = geometry.tracing_radius_m;
    let left = outward_interval_trace_spherical_uniform_in_domain(
        &samples,
        center_frame,
        meters_per_frame,
        geometry,
        1.0e-13,
        [-radius_m, 0.0],
    );
    let right = outward_interval_trace_spherical_uniform_in_domain(
        &samples,
        center_frame,
        meters_per_frame,
        geometry,
        1.0e-13,
        [0.0, radius_m],
    );
    let left_height_m = [
        left.sample.center_displacement_m,
        left.sample.center_displacement_m + left.height_error_bound_m,
    ];
    let right_height_m = [
        right.sample.center_displacement_m,
        right.sample.center_displacement_m + right.height_error_bound_m,
    ];
    for (name, trace) in [("left", left), ("right", right)] {
        let recorded = &artifact["outwardIntervalReference"][name];
        assert_eq!(
            recorded["centerDisplacementLowerM"]
                .as_f64()
                .unwrap()
                .to_bits(),
            trace.sample.center_displacement_m.to_bits()
        );
        assert_eq!(
            recorded["heightErrorBoundM"].as_f64().unwrap().to_bits(),
            trace.height_error_bound_m.to_bits()
        );
        for (value, expected) in recorded["contactOffsetEnclosureM"]
            .as_array()
            .unwrap()
            .iter()
            .zip(trace.contact_offset_enclosure_m)
        {
            assert_eq!(value.as_f64().unwrap().to_bits(), expected.to_bits());
        }
    }

    assert!(left.contact_offset_enclosure_m[1] < right.contact_offset_enclosure_m[0]);
    assert!(left_height_m[0].max(right_height_m[0]) <= left_height_m[1].min(right_height_m[1]));
    assert!(right.contact_offset_enclosure_m[0] - left.contact_offset_enclosure_m[1] > 3.3e-6);
    assert_eq!(
        trace_spherical_uniform(&samples, center_frame, meters_per_frame, geometry),
        Err(StylusTraceError::GlobalContactNotIsolated),
    );
    assert_eq!(
        trace_spherical_uniform_contacts(&samples, center_frame, meters_per_frame, geometry),
        Err(StylusTraceError::ContactHeightOrderNotIsolated),
    );
    assert_eq!(
        artifact["productionResult"]["scalarApi"],
        "GlobalContactNotIsolated"
    );
    assert_eq!(
        artifact["productionResult"]["contactSetApi"],
        "ContactHeightOrderNotIsolated"
    );
    assert_eq!(artifact["claimLimits"].as_array().unwrap().len(), 5);
}

#[test]
#[ignore = "prints the PVC-004 unresolved height-order evidence"]
fn report_pvc_004_unresolved_height_order() {
    let (samples, meters_per_frame) = pvc_004_inner_groove_sine();
    let geometry = StylusGeometry::default();
    let radius_m = geometry.tracing_radius_m;
    let traces = [[-radius_m, 0.0], [0.0, radius_m]].map(|domain| {
        outward_interval_trace_spherical_uniform_in_domain(
            &samples,
            258.0,
            meters_per_frame,
            geometry,
            1.0e-13,
            domain,
        )
    });
    let support_bits = samples[240..=277]
        .iter()
        .map(|sample| format!("{:08x}", sample.to_bits()))
        .collect::<Vec<_>>();
    let mut dense = [(f64::NEG_INFINITY, 0.0); 2];
    for index in 0..=1_000_000 {
        let offset_m = -radius_m + 2.0 * radius_m * index as f64 / 1_000_000.0;
        let height_m = cubic_value_at(&samples, 258.0 + offset_m / meters_per_frame)
            + circle_height(radius_m, offset_m)
            - radius_m;
        let half = usize::from(offset_m >= 0.0);
        if height_m > dense[half].0 {
            dense[half] = (height_m, offset_m);
        }
    }
    eprintln!(
        "meters_per_frame={meters_per_frame:?} bits={:016x}",
        meters_per_frame.to_bits()
    );
    let amplitude_m = 0.05 / (std::f64::consts::TAU * 8_000.0);
    eprintln!(
        "amplitude={amplitude_m:?} bits={:016x}",
        amplitude_m.to_bits()
    );
    eprintln!("wavelength={:?}", 24.0 * meters_per_frame);
    eprintln!("left={:?} right={:?}", traces[0], traces[1]);
    eprintln!(
        "height_overlap={:?} separation_bounds={:?} dense={dense:?} dense_height_difference={:?}",
        [
            traces[0]
                .sample
                .center_displacement_m
                .max(traces[1].sample.center_displacement_m),
            (traces[0].sample.center_displacement_m + traces[0].height_error_bound_m)
                .min(traces[1].sample.center_displacement_m + traces[1].height_error_bound_m,),
        ],
        [
            traces[1].contact_offset_enclosure_m[0] - traces[0].contact_offset_enclosure_m[1],
            traces[1].contact_offset_enclosure_m[1] - traces[0].contact_offset_enclosure_m[0],
        ],
        (dense[0].0 - dense[1].0).abs(),
    );
    eprintln!("support_start=240 support_bits={support_bits:?}");
    eprintln!(
        "production={:?}",
        trace_spherical_uniform(&samples, 258.0, meters_per_frame, geometry)
    );
}

#[test]
fn independent_longitudinal_wall_offsets_are_exact_for_the_rigid_sphere_reduction() {
    let center_frame = 64.0;
    let wall_zero = (0..128)
        .map(|frame| {
            let distance = (frame as f64 - (center_frame - 5.2)) / 1.1;
            (4.0e-6 * (-0.5 * distance * distance).exp()) as f32
        })
        .collect::<Vec<_>>();
    let wall_one = (0..128)
        .map(|frame| {
            let distance = (frame as f64 - (center_frame + 4.7)) / 1.3;
            (3.8e-6 * (-0.5 * distance * distance).exp()) as f32
        })
        .collect::<Vec<_>>();
    let geometry = StylusGeometry::default();
    let traces = [&wall_zero, &wall_one].map(|wall| {
        outward_interval_trace_spherical_uniform(
            wall,
            center_frame,
            METERS_PER_SOURCE_FRAME,
            geometry,
            TRACE_HEIGHT_CERTIFICATE_M,
        )
    });
    assert!(traces[0].contact_offset_enclosure_m[1] < traces[1].contact_offset_enclosure_m[0]);

    let lateral_tip_m = WALL_SCALE
        * (traces[0].sample.center_displacement_m - traces[1].sample.center_displacement_m);
    let vertical_tip_m = WALL_SCALE
        * (traces[0].sample.center_displacement_m + traces[1].sample.center_displacement_m);
    let projected_tip_m = [
        WALL_SCALE * (lateral_tip_m + vertical_tip_m),
        WALL_SCALE * (-lateral_tip_m + vertical_tip_m),
    ];
    for wall_index in 0..2 {
        assert!(
            (projected_tip_m[wall_index] - traces[wall_index].sample.center_displacement_m).abs()
                < 1.0e-20
        );
        let wall = if wall_index == 0 {
            &wall_zero
        } else {
            &wall_one
        };
        for sample_index in 0..=1_024 {
            let offset_m = -geometry.tracing_radius_m
                + 2.0 * geometry.tracing_radius_m * sample_index as f64 / 1_024.0;
            let groove = cubic_value_at(wall, center_frame + offset_m / METERS_PER_SOURCE_FRAME);
            let required_projection_m = groove + circle_height(geometry.tracing_radius_m, offset_m)
                - geometry.tracing_radius_m;
            assert!(
                required_projection_m
                    <= projected_tip_m[wall_index] + traces[wall_index].height_error_bound_m
            );
        }
    }
}

#[test]
fn vertical_reference_reduces_the_symmetric_two_wall_contact_equations() {
    let contact = StylusContactConfig::default();
    let tonearm = TonearmConfig::default();
    let mut production = equilibrium_candidate(contact, tonearm);
    let mut reference = VerticalReferenceState::equilibrium(tonearm);
    let slope = 0.07;
    let velocity = 0.25;
    let production_output = production
        .process(PickupMechanicalInput {
            wall_contacts: test_single_wall_contacts([0.0; 2], [slope * WALL_SCALE; 2]),
            contact_surface: PickupContactSurface::GrooveWalls,
            groove_radius_m: GROOVE_RADIUS_M,
            groove_tangential_velocity_m_s: velocity,
            stylus_lowered: true,
            ..PickupMechanicalInput::default()
        })
        .unwrap();
    let reference_output =
        reference.step(contact, tonearm, 0.0, slope, velocity, OUTPUT_DT_SECONDS);
    let production_vertical_normal_force_n =
        production_output.wall_normal_force_n.iter().sum::<f64>()
            / (1.0 + 0.5 * slope * slope).sqrt()
            * WALL_SCALE;
    assert!(
        (production_vertical_normal_force_n - reference_output.vertical_normal_force_n).abs()
            < 1.0e-12
    );
    assert!(
        (production_output.wall_normal_force_n.iter().sum::<f64>()
            - reference_output.wall_normal_force_sum_n)
            .abs()
            < 1.0e-12
    );
    assert!(
        (production_output.record_reaction_torque_nm()
            - reference_output.record_reaction_torque_nm)
            .abs()
            < 1.0e-12
    );
}

#[test]
fn rapid_scratch_reference_covers_signed_rates_impulse_loss_and_retracking() {
    let fixture = programme_fixture(16_384);
    let advances = rate_sweep_advances();
    let metrics = assert_no_alloc::assert_no_alloc(|| {
        run_rapid_scratch_case(
            &fixture,
            &advances,
            8_192.0 - advances.iter().sum::<f64>() * 0.5,
            MAX_REFERENCE_SWEEP_STEP_FRAMES,
        )
    })
    .unwrap();

    assert_eq!(metrics.candidate_trace_calls, 2 * advances.len() as u64);
    assert_eq!(metrics.candidate_contact_steps, advances.len() as u64);
    assert!(metrics.reference_substeps_max <= 160);
    assert!(metrics.reference_trace_calls <= 320 * advances.len() as u64);
    assert_eq!(
        metrics.reference_trace_calls,
        2 * metrics.reference_contact_steps
    );
    assert!(metrics.reference_contact_transitions >= 2);
    assert!(metrics.wall_height.normalized_rms_error().is_finite());
    assert!(metrics.wall_normal_force.normalized_rms_error().is_finite());
    assert!(metrics.reaction_torque.normalized_rms_error().is_finite());
    assert!(metrics
        .reaction_torque
        .relative_integral_error()
        .is_finite());
}

#[test]
#[ignore = "prints the offline RP-013 work and error report"]
fn report_rapid_scratch_reference_metrics() {
    let geometry = StylusGeometry::default();
    for (name, samples, center_frame) in [
        (
            "PVC-001",
            PLAUSIBLE_ENVELOPE_COUNTEREXAMPLE.as_slice(),
            16.37,
        ),
        (
            "adversarial",
            ADVERSARIAL_ENVELOPE_COUNTEREXAMPLE.as_slice(),
            16.37,
        ),
    ] {
        let production =
            trace_spherical_uniform(samples, center_frame, METERS_PER_SOURCE_FRAME, geometry);
        let reference = outward_interval_trace_spherical_uniform(
            samples,
            center_frame,
            METERS_PER_SOURCE_FRAME,
            geometry,
            TRACE_HEIGHT_CERTIFICATE_M,
        );
        eprintln!("{name} production={production:?} reference={reference:?}");
        eprintln!("{name} kinematics={:?}", cubic_kinematic_bounds(samples));
    }
    let seed_asset = seed_domain_asset();
    let seed_wall = vertical_asset_wall(&seed_asset);
    let seed_production =
        trace_spherical_uniform(&seed_wall, 40.37, METERS_PER_SOURCE_FRAME, geometry);
    let seed_reference = outward_interval_trace_spherical_uniform(
        &seed_wall,
        40.37,
        METERS_PER_SOURCE_FRAME,
        geometry,
        TRACE_HEIGHT_CERTIFICATE_M,
    );
    eprintln!(
        "PVC-001-emitted production={seed_production:?} reference={seed_reference:?} report={:?} kinematics={:?}",
        seed_asset.report(),
        cubic_kinematic_bounds(&seed_wall),
    );

    let fixture = programme_fixture(16_384);
    let advances = rate_sweep_advances();
    let start = std::time::Instant::now();
    let metrics = run_rapid_scratch_case(
        &fixture,
        &advances,
        8_192.0 - advances.iter().sum::<f64>() * 0.5,
        MAX_REFERENCE_SWEEP_STEP_FRAMES,
    )
    .unwrap();
    let elapsed = start.elapsed();
    let fine_start = std::time::Instant::now();
    let fine = run_rapid_scratch_case(
        &fixture,
        &advances,
        8_192.0 - advances.iter().sum::<f64>() * 0.5,
        0.5 * MAX_REFERENCE_SWEEP_STEP_FRAMES,
    )
    .unwrap();
    let fine_elapsed = fine_start.elapsed();
    eprintln!("rapid_metrics={metrics:#?}");
    eprintln!(
        "height_nrmse={} height_integral_error={} force_nrmse={} force_integral_error={} torque_nrmse={} torque_integral_error={} torque_absolute_impulse_error={} elapsed={elapsed:?}",
        metrics.wall_height.normalized_rms_error(),
        metrics.wall_height.relative_integral_error(),
        metrics.wall_normal_force.normalized_rms_error(),
        metrics.wall_normal_force.relative_integral_error(),
        metrics.reaction_torque.normalized_rms_error(),
        metrics.reaction_torque.relative_integral_error(),
        metrics.reaction_torque.relative_absolute_integral_error(),
    );
    eprintln!(
        "fine_reference force_integral={} torque_integral={} height_integral={} contact_transitions={} trace_calls={} contact_steps={} elapsed={fine_elapsed:?}",
        fine.wall_normal_force.reference_integral,
        fine.reaction_torque.reference_integral,
        fine.wall_height.reference_integral,
        fine.reference_contact_transitions,
        fine.reference_trace_calls,
        fine.reference_contact_steps,
    );
}

#[test]
fn swept_reference_converges_when_the_maximum_spatial_step_halves() {
    let fixture = programme_fixture(4_096);
    let advances = [1.0, 4.0, 12.0, 20.0, -20.0, -12.0, -4.0, -1.0, 20.0, -20.0];
    let coarse = run_rapid_scratch_case(
        &fixture,
        &advances,
        2_048.0,
        MAX_REFERENCE_SWEEP_STEP_FRAMES,
    )
    .unwrap();
    let fine = run_rapid_scratch_case(
        &fixture,
        &advances,
        2_048.0,
        0.5 * MAX_REFERENCE_SWEEP_STEP_FRAMES,
    )
    .unwrap();

    // These bounds detect a broken reference harness. They are not release
    // accuracy limits for the physical player.
    assert!(
        (coarse.reaction_torque.reference_absolute_integral
            - fine.reaction_torque.reference_absolute_integral)
            .abs()
            <= 0.02
                * fine
                    .reaction_torque
                    .reference_absolute_integral
                    .max(1.0e-15)
    );
    assert!(
        (coarse.wall_normal_force.reference_integral - fine.wall_normal_force.reference_integral)
            .abs()
            <= 0.02 * fine.wall_normal_force.reference_integral.abs().max(1.0e-12)
    );
}

#[test]
fn swept_reference_resolves_a_one_frame_impulse_between_twenty_x_endpoints() {
    let mut vertical_m = vec![0.0_f32; 2_048];
    vertical_m[1_024] = 12.0e-6;
    let fixture = RapidScratchFixture::new(vertical_m);
    let metrics = run_rapid_scratch_case(
        &fixture,
        &[20.0; 4],
        973.25,
        MAX_REFERENCE_SWEEP_STEP_FRAMES,
    )
    .unwrap();

    assert!(metrics.reference_peak_substep_wall_height_m > 5.0e-6);
    assert!(
        metrics.reference_peak_substep_wall_height_m
            > 1.5 * metrics.candidate_peak_wall_height_m.max(1.0e-12)
    );
    assert!(metrics.reference_peak_substep_wall_normal_force_n > 0.0);
    assert!(metrics.reference_trace_calls >= 160 * metrics.candidate_trace_calls);
}

#[test]
fn swept_reference_observes_contact_loss_and_retracking_inside_coarse_samples() {
    let fixture = programme_fixture(4_096);
    let metrics = run_rapid_scratch_case(
        &fixture,
        &[20.0; 20],
        2_048.0 - 220.0,
        MAX_REFERENCE_SWEEP_STEP_FRAMES,
    )
    .unwrap();

    assert!(metrics.reference_contact_transitions >= 2);
    assert!(metrics.maximum_contact_occupancy_error > 0.5);
    assert!(metrics.reference_contact_transitions > metrics.candidate_contact_transitions);
}

#[test]
fn default_vertical_axis_remains_the_reference_configuration() {
    let vertical = TonearmConfig::default().vertical;
    assert_eq!(
        vertical,
        SuspensionAxisConfig {
            effective_mass_kg: 0.030_5,
            compliance_m_per_n: 0.010,
            damping_ratio: 0.30,
        }
    );
}
