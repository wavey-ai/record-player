use js_sys::{Array, Float32Array};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::vinyl_vfx::{VinylVfxContext, VinylVfxProcessor, VINYL_VFX_MAX_SCENE};
use wasm_bindgen::prelude::*;

use crate::{
    mechanics::{
        DeckMechanicalControl, DeckMechanicalError, DeckMechanicalState, DeckMechanicalTelemetry,
        MotorMode, NormalizedDeckControl, PhysicalDeckConfig,
    },
    mixer::{sharp_crossfader_gains, DEFAULT_SHARP_CROSSFADER_WIDTH},
    resampler::adaptive_sample,
    scratch_gate::{ScratchGate, ScratchPreset},
};

const OUTPUT_GAIN: f64 = 1.0;
const MAX_FINAL_OUTPUT_GAIN: f64 = 4.0;
const MAX_FINAL_OUTPUT_GAIN_RAMP_MS: f64 = 60_000.0;
const POSITION_CATCHUP_SECONDS: f64 = 0.28;
const MOTION_HOLD_SECONDS: f64 = 0.05;
const MOTION_HOLD_RELEASE_SECONDS: f64 = 0.06;
// A landing finger develops its force in single-digit milliseconds; a
// 12 ms attack was the floor under every live catch, gating the hand's
// weight no matter how hard it pressed.
const GRIP_ATTACK_SECONDS: f64 = 0.004;
const GRIP_RELEASE_SECONDS: f64 = 0.045;
/// Below this residual force a released hand counts as fully separated.
const GRIP_CONTACT_EPSILON: f64 = 0.02;
/// A lifting finger's normal force collapses over this span.
const HAND_RELEASE_SECONDS: f64 = 0.008;
/// The stylus fades over this span approaching a pinned record edge so a
/// clamped scratch cannot hold a full-level frozen sample.
const EDGE_FADE_SECONDS: f64 = 0.01;
/// The stop gain follows the rate through this smoothing so a hard catch
/// cannot step the output in one sample. Well under the mechanical
/// reversal time, so it adds no feelable latency.
const MOVEMENT_GAIN_SECONDS: f64 = 0.003;
const GRIP_OWNERSHIP: f64 = 0.5;
const DEADZONE_RATE: f64 = 0.006;
/// Full music gain is reached at 2% of nominal speed, not 10%. A cartridge
/// outputs full-spectrum signal at slow groove velocity (pitched down into
/// bass — the dub body), going silent only at true standstill. The old 10%
/// knee muted exactly the gentle-motion region, chopping every turnaround of
/// a slow scratch. Velocity-responsive: full body for any real motion, taper
/// only into the deadzone at rest.
const STOP_GAIN_FULL_RATE: f64 = 0.02;
/// RIAA time constants (IEC 60098): 3180 us, 318 us, 75 us.
const RIAA_T1_SECONDS: f64 = 3180.0e-6;
const RIAA_T2_SECONDS: f64 = 318.0e-6;
const RIAA_T3_SECONDS: f64 = 75.0e-6;
/// The speed tilt's high-frequency asymptote is 1/rate, so the rate that
/// shapes it is held above a floor. Its partner, the cartridge velocity gain,
/// is `rate`, so the pair's product stays bounded at unity.
const RIAA_TILT_MIN_RATE: f64 = 0.1;
const RIAA_TILT_MAX_RATE: f64 = 4.0;
/// A cartridge really does put out more voltage the faster the groove passes,
/// without limit. Bound it so a runaway rate cannot blow up the programme.
const MAX_CARTRIDGE_VELOCITY_GAIN: f64 = 4.0;
/// Opt-in vinyl voicing seed: a fixed, deliberately non-RIAA curve blended
/// over the transparent master. A matched cut/playback RIAA pair is exactly
/// identity, so warmth cannot come from the standard curve. What is left is
/// the parts that do not cancel — the cartridge and arm losing the top end,
/// and a real preamp departing from the textbook curve. These are seed
/// values, not a calibrated hardware profile.
///
/// `AcousticConfig.vinyl_voicing_curve` selects one of these shapes. Each is
/// a real phono-chain mechanism, not an arbitrary EQ:
#[derive(Clone, Copy, Debug, PartialEq)]
struct VinylVoicingCurve {
    /// The cartridge/arm mechanical top-end loss.
    cartridge_hz: f64,
    cartridge_q: f64,
    /// The preamp's departure from flat below the mids.
    low_hz: f64,
    low_db: f64,
    /// The preamp's departure from flat above the mids.
    high_hz: f64,
    high_db: f64,
    shelf_q: f64,
}

const VINYL_VOICING_CURVES: [VinylVoicingCurve; 3] = [
    // COIL LOAD: a moving-magnet cartridge's inductance loaded by the cable's
    // capacitance — a broad top-end shelf with a little body under it.
    VinylVoicingCurve {
        cartridge_hz: 16_000.0,
        cartridge_q: 0.6,
        low_hz: 150.0,
        low_db: 4.0,
        high_hz: 4_500.0,
        high_db: -4.5,
        shelf_q: 0.707,
    },
    // TIP MASS: the stylus's own mass and compliance, which mostly costs the
    // extreme top and leaves the body nearly alone.
    VinylVoicingCurve {
        cartridge_hz: 13_000.0,
        cartridge_q: 0.5,
        low_hz: 100.0,
        low_db: 2.0,
        high_hz: 6_000.0,
        high_db: -5.0,
        shelf_q: 0.707,
    },
    // CURVE DRIFT: a preamp whose feedback network departed from the RIAA
    // curve — a low-mid lift and a broad presence dip, no cartridge pole.
    VinylVoicingCurve {
        cartridge_hz: 20_000.0,
        cartridge_q: 0.707,
        low_hz: 300.0,
        low_db: 2.5,
        high_hz: 3_000.0,
        high_db: -2.0,
        shelf_q: 0.707,
    },
];
const DRAG_LOWPASS_MAX_HZ: f64 = 19_000.0;
const DRAG_LOWPASS_RATE_KNEE: f64 = 0.95;
const TRACING_LOSS_START_RATE: f64 = 2.5;
const STYLUS_TRACING_CURVATURE_THRESHOLD: f64 = 0.65;
const STYLUS_TRACING_CURVATURE_FULL_SCALE: f64 = 3.0;
const PROGRAMME_UPPER_CROSSOVER_HZ: f64 = 5_200.0;
const PROGRAMME_ACCELERATION_THRESHOLD: f64 = 0.18;
const PROGRAMME_ACCELERATION_FULL_SCALE: f64 = 0.85;
const PROGRAMME_DIRECTION_CHANGE_WEIGHT: f64 = 0.65;
const PROGRAMME_LIMITER_MIN_UPPER_GAIN: f64 = 0.16;
const PROGRAMME_LIMITER_ATTACK_SECONDS: f64 = 0.00012;
const PROGRAMME_LIMITER_RELEASE_SECONDS: f64 = 0.032;
const WOW_REV_SECONDS: f64 = 1.8;
const FLUTTER_HZ: f64 = 6.4;
const FREE_PLAYBACK_WOW_DEPTH: f64 = 0.000_24;
const HAND_SLIP_WOW_DEPTH: f64 = 0.000_18;
const CONTACT_NOISE_GAIN: f64 = 0.00008;
const SOURCE_TEXTURE_GAIN: f64 = 0.00018;
const DUST_FLECK_GAIN: f64 = 0.000045;
const CONTACT_IMPULSE_DECAY: f64 = 0.985;
const WINDOW_REQUEST_MARGIN_SECONDS: f64 = 0.75;
const WINDOW_REQUEST_PROJECT_SECONDS: f64 = 0.18;
const WINDOW_MISS_FADE_SECONDS: f64 = 0.006;
const MOMENTARY_CROSSFADER_TRANSITION_SECONDS: f64 = 0.00045;
const PROGRAMME_END_POSITION_EPSILON_FRAMES: f64 = 1.0e-7;
const DEFAULT_REPLAY_NOISE_SEED: u32 = 0x9e37_79b9;
/// Matches EnCodec's fixed-context seam repair: twelve samples on either
/// side of a join are replaced by one cubic Hermite bridge.
const SEAM_REPAIR_SAMPLES: usize = 24;
const LOOSE_SLIPMAT_COUPLING_SCALE: f64 = 0.65;
const TIGHT_SLIPMAT_COUPLING_SCALE: f64 = 2.0;

// Needle-surface bed and needle-drop foley (original: player.js 3915–4249).
const LEAD_IN_STATIC_GAIN: f64 = 0.048;
const DEADWAX_STATIC_GAIN: f64 = 0.052;
const NEEDLE_SURFACE_SAMPLE_PAD_SECONDS: f64 = 0.05;
const SURFACE_BED_ATTACK_SECONDS: f64 = 0.08;
const SURFACE_BED_RELEASE_SECONDS: f64 = 0.16;
const SURFACE_ENV_FLOOR: f64 = 0.0001;
const NEEDLE_DROP_BURST_SECONDS: f64 = 0.34;
const NEEDLE_DROP_BURST_FILTER_HZ: f64 = 6200.0;
const NEEDLE_DROP_BURST_FILTER_Q: f64 = 0.5;
const NEEDLE_DROP_THUMP_GAIN: f64 = 0.045;
const NEEDLE_LIFT_THUMP_GAIN: f64 = 0.022;
pub const SURFACE_REGION_LEAD_IN: u8 = 0;
pub const SURFACE_REGION_DEADWAX: u8 = 1;

// RBJ lowpass biquad — matches the Web Audio BiquadFilterNode "lowpass" response.
#[derive(Clone, Copy, Debug, Default)]
struct BiquadLowpass {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl BiquadLowpass {
    fn new(cutoff_hz: f64, q: f64, sample_rate: f64) -> Self {
        let w0 = std::f64::consts::TAU * (cutoff_hz / sample_rate).clamp(0.0, 0.5);
        let alpha = w0.sin() / (2.0 * q.max(1e-4));
        let cos_w0 = w0.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: ((1.0 - cos_w0) / 2.0) / a0,
            b1: (1.0 - cos_w0) / a0,
            b2: ((1.0 - cos_w0) / 2.0) / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha) / a0,
            ..Default::default()
        }
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Stereo-linked limiter for physically demanding programme upper-band motion.
///
/// The speed-dependent half of a phono chain.
///
/// A lacquer is cut with RIAA pre-emphasis `P` and played back through the
/// preamp's fixed de-emphasis `D = 1/P`. At nominal speed the two cancel
/// exactly and the master comes back untouched. Off speed they no longer
/// cancel: the groove's content shifts in frequency by the play rate while
/// the preamp's curve stays where it is, so what comes out carries a genuine
/// speed-dependent tilt
///
/// ```text
///     T(f) = D(f) / D(f/r)
/// ```
///
/// where `D(s) = (1 + s*T2) / ((1 + s*T1)(1 + s*T3))`. Expanding the ratio
/// gives three first-order sections, each a zero over a pole:
///
/// ```text
///     (1 + s*T2)   (1 + s*T1/r)   (1 + s*T3/r)
///     ---------- * ------------ * ------------
///     (1 + s*T2/r) (1 + s*T1)     (1 + s*T3)
/// ```
///
/// This is reproduction, not colour. At `r == 1` every section has its zero
/// on its pole, so the response is exactly unity and a settled filter passes
/// the programme through bit-exact — the transparent-master rule holds. The
/// tilt exists only while the record is off speed, which is the whole point:
/// a scratched record genuinely does not read the same spectrum as a record
/// running at 33.
///
/// Its high-frequency asymptote is `1/r`, which pairs with the cartridge
/// velocity gain of `r` to leave presence roughly intact while the bass
/// scales with speed — slow strokes read thin and quiet, fast strokes read
/// loud and full, as a real deck does.
#[derive(Clone, Debug, PartialEq)]
struct RiaaSpeedTilt {
    /// `(b0, b1, a1)` per section.
    sections: [(f64, f64, f64); 3],
    /// `(x[n-1], y[n-1])` per section, per channel.
    state: [[(f64, f64); 3]; 2],
    rate: f64,
    /// The rate the coefficients are actually built from. `T(f)` is derived
    /// for a record held at a steady speed, so it is applied quasi-statically
    /// and its control is eased rather than snapped. Without that, a reversal
    /// restructures a resonant filter sample by sample and the modulation
    /// itself lands in the programme as a step.
    control_rate: f64,
    sample_rate: f64,
}

impl RiaaSpeedTilt {
    fn new(sample_rate: f64) -> Self {
        let mut tilt = Self {
            sections: [(1.0, 0.0, 0.0); 3],
            state: [[(0.0, 0.0); 3]; 2],
            rate: f64::NAN,
            control_rate: f64::NAN,
            sample_rate: if sample_rate.is_finite() && sample_rate > 0.0 {
                sample_rate
            } else {
                48_000.0
            },
        };
        tilt.set_rate(1.0);
        tilt
    }

    fn reset(&mut self) {
        self.state = [[(0.0, 0.0); 3]; 2];
        self.control_rate = f64::NAN;
    }

    /// Ease the control toward the record's actual rate. A deck seeded at
    /// speed starts converged, so steady playback is transparent immediately.
    fn follow_rate(&mut self, abs_rate: f64, alpha: f64) {
        let target = finite_or_zero(abs_rate).abs();
        if self.control_rate.is_nan() {
            self.control_rate = target;
        } else {
            self.control_rate += (target - self.control_rate) * alpha;
            if (self.control_rate - target).abs() < 1.0e-6 {
                self.control_rate = target;
            }
        }
        self.set_rate(self.control_rate);
    }

    /// Bilinear transform of `(1 + s*zero) / (1 + s*pole)`.
    fn first_order(zero_seconds: f64, pole_seconds: f64, k: f64) -> (f64, f64, f64) {
        let zero = zero_seconds * k;
        let pole = pole_seconds * k;
        let denominator = 1.0 + pole;
        (
            (1.0 + zero) / denominator,
            (1.0 - zero) / denominator,
            (1.0 - pole) / denominator,
        )
    }

    fn set_rate(&mut self, rate: f64) {
        let rate = finite_or_zero(rate)
            .abs()
            .clamp(RIAA_TILT_MIN_RATE, RIAA_TILT_MAX_RATE);
        // Coefficients only move when the rate does. Steady playback recomputes
        // nothing, and a scratch resolves at whatever resolution it moves with.
        if (rate - self.rate).abs() < 1.0e-9 {
            return;
        }
        self.rate = rate;
        let k = 2.0 * self.sample_rate;
        self.sections = [
            Self::first_order(RIAA_T2_SECONDS, RIAA_T2_SECONDS / rate, k),
            Self::first_order(RIAA_T1_SECONDS / rate, RIAA_T1_SECONDS, k),
            Self::first_order(RIAA_T3_SECONDS / rate, RIAA_T3_SECONDS, k),
        ];
    }

    fn process(&mut self, channel: usize, sample: f64) -> f64 {
        let Some(state) = self.state.get_mut(channel) else {
            return sample;
        };
        let mut value = sample;
        for (section, memory) in self.sections.iter().zip(state.iter_mut()) {
            let (b0, b1, a1) = *section;
            let (previous_input, previous_output) = *memory;
            // The two state terms are summed with each other before they
            // reach the input term. At nominal speed a section's zero sits on
            // its pole, so `b1 == a1` and the pair cancels to exactly zero,
            // leaving `b0 * value` with `b0 == 1.0` — bit-exact transparency.
            // Adding the input first would round that cancellation away.
            let output = b0 * value + (b1 * previous_input - a1 * previous_output);
            *memory = (value, output);
            value = output;
        }
        finite_or_zero(value)
    }
}

/// A direct-form-I RBJ biquad, used only by the opt-in voicing stage. The
/// coefficients come from the Audio EQ Cookbook forms so the seed curve is a
/// plain, inspectable filter rather than a fitted table.
#[derive(Clone, Copy, Debug, Default)]
struct VoicingBiquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl VoicingBiquad {
    fn from_coefficients(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            ..Default::default()
        }
    }

    fn lowpass(cutoff_hz: f64, q: f64, sample_rate: f64) -> Self {
        let w0 = std::f64::consts::TAU * (cutoff_hz / sample_rate).clamp(0.0, 0.5);
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q.max(1e-4));
        Self::from_coefficients(
            (1.0 - cos_w0) / 2.0,
            1.0 - cos_w0,
            (1.0 - cos_w0) / 2.0,
            1.0 + alpha,
            -2.0 * cos_w0,
            1.0 - alpha,
        )
    }

    fn low_shelf(freq_hz: f64, q: f64, gain_db: f64, sample_rate: f64) -> Self {
        let a = 10.0_f64.powf(gain_db / 40.0);
        let w0 = std::f64::consts::TAU * (freq_hz / sample_rate).clamp(0.0, 0.5);
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q.max(1e-4));
        let root = 2.0 * a.sqrt() * alpha;
        Self::from_coefficients(
            a * ((a + 1.0) - (a - 1.0) * cos_w0 + root),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0),
            a * ((a + 1.0) - (a - 1.0) * cos_w0 - root),
            (a + 1.0) + (a - 1.0) * cos_w0 + root,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0),
            (a + 1.0) + (a - 1.0) * cos_w0 - root,
        )
    }

    fn high_shelf(freq_hz: f64, q: f64, gain_db: f64, sample_rate: f64) -> Self {
        let a = 10.0_f64.powf(gain_db / 40.0);
        let w0 = std::f64::consts::TAU * (freq_hz / sample_rate).clamp(0.0, 0.5);
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q.max(1e-4));
        let root = 2.0 * a.sqrt() * alpha;
        Self::from_coefficients(
            a * ((a + 1.0) + (a - 1.0) * cos_w0 + root),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0),
            a * ((a + 1.0) + (a - 1.0) * cos_w0 - root),
            (a + 1.0) - (a - 1.0) * cos_w0 + root,
            2.0 * ((a - 1.0) - (a + 1.0) * cos_w0),
            (a + 1.0) - (a - 1.0) * cos_w0 - root,
        )
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    /// New coefficients over the existing delay state, so changing curve does
    /// not zero a running filter and click.
    fn retuned(mut self, previous: &Self) -> Self {
        self.x1 = previous.x1;
        self.x2 = previous.x2;
        self.y1 = previous.y1;
        self.y2 = previous.y2;
        self
    }
}

/// The opt-in "vinyl voicing" seed curve: the non-cancelling half of a real
/// phono chain. A matched RIAA cut/playback pair is exactly the identity, so
/// the audible character of a record is not the standard curve — it is the
/// mismatch left after it. One inspectable seed is blended over the
/// transparent master by `amount`. At `amount == 0` the stage adds no
/// arithmetic to the signal. The shape is one of [`VINYL_VOICING_CURVES`].
#[derive(Clone, Debug)]
struct VinylVoicingFilter {
    curve: usize,
    sample_rate: f64,
    cartridge: [VoicingBiquad; 2],
    low_shelf: [VoicingBiquad; 2],
    high_shelf: [VoicingBiquad; 2],
}

impl VinylVoicingFilter {
    fn new(sample_rate: f64) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let mut filter = Self {
            curve: usize::MAX,
            sample_rate,
            cartridge: [VoicingBiquad::default(); 2],
            low_shelf: [VoicingBiquad::default(); 2],
            high_shelf: [VoicingBiquad::default(); 2],
        };
        filter.set_curve(0);
        filter
    }

    fn curve(&self) -> usize {
        self.curve
    }

    /// Rebuild the coefficients for one of [`VINYL_VOICING_CURVES`], keeping
    /// the running delay so a curve change does not click. Unknown indices
    /// clamp to the last curve; the host validates before it gets here.
    fn set_curve(&mut self, curve: usize) {
        let curve = curve.min(VINYL_VOICING_CURVES.len() - 1);
        if curve == self.curve {
            return;
        }
        self.curve = curve;
        let spec = VINYL_VOICING_CURVES[curve];
        for channel in 0..2 {
            let previous = self.cartridge[channel];
            self.cartridge[channel] = VoicingBiquad::lowpass(
                spec.cartridge_hz,
                spec.cartridge_q,
                self.sample_rate,
            )
            .retuned(&previous);
            let previous = self.low_shelf[channel];
            self.low_shelf[channel] = VoicingBiquad::low_shelf(
                spec.low_hz,
                spec.shelf_q,
                spec.low_db,
                self.sample_rate,
            )
            .retuned(&previous);
            let previous = self.high_shelf[channel];
            self.high_shelf[channel] = VoicingBiquad::high_shelf(
                spec.high_hz,
                spec.shelf_q,
                spec.high_db,
                self.sample_rate,
            )
            .retuned(&previous);
        }
    }

    fn reset(&mut self) {
        for filter in self
            .cartridge
            .iter_mut()
            .chain(self.low_shelf.iter_mut())
            .chain(self.high_shelf.iter_mut())
        {
            filter.reset();
        }
    }

    /// Blend the fixed curve over the dry signal. `amount == 0` returns the
    /// input unchanged for any finite `wet`, so the default path is bit-exact.
    fn process(&mut self, channel: usize, sample: f64, amount: f64) -> f64 {
        if channel >= 2 {
            return sample;
        }
        let dry = sample;
        let cartridge = self.cartridge[channel].process(dry);
        let body = self.low_shelf[channel].process(cartridge);
        let wet = self.high_shelf[channel].process(body);
        dry + amount * (wet - dry)
    }
}

/// A one-pole low-pass and its exact residual form a complementary split. The
/// shared envelope only scales that residual; the base band is never run
/// through a blanket low-pass or full-band gain stage.
#[derive(Clone, Debug, PartialEq)]
struct HighFrequencyAccelerationLimiter {
    lowpass: [f64; 2],
    previous_upper: [f64; 2],
    previous_velocity: [f64; 2],
    initialized: [bool; 2],
    linked_gain: f64,
    coefficient_sample_rate: f64,
    split_alpha: f64,
    attack_alpha: f64,
    release_alpha: f64,
    first_derivative_scale: f64,
    second_derivative_scale: f64,
}

impl Default for HighFrequencyAccelerationLimiter {
    fn default() -> Self {
        Self {
            lowpass: [0.0; 2],
            previous_upper: [0.0; 2],
            previous_velocity: [0.0; 2],
            initialized: [false; 2],
            linked_gain: 1.0,
            coefficient_sample_rate: 0.0,
            split_alpha: 1.0,
            attack_alpha: 1.0,
            release_alpha: 1.0,
            first_derivative_scale: 1.0,
            second_derivative_scale: 1.0,
        }
    }
}

impl HighFrequencyAccelerationLimiter {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn process_frame(
        &mut self,
        samples: [f64; 2],
        channel_count: usize,
        sample_rate: f64,
        strength: f64,
    ) -> [f64; 2] {
        let channel_count = channel_count.clamp(1, 2);
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.prepare_sample_rate(sample_rate);
        let strength = finite_or_zero(strength).clamp(0.0, 1.0);
        let mut base = samples;
        let mut upper = [0.0; 2];
        let mut linked_demand = 0.0_f64;

        for channel in 0..channel_count {
            if !self.initialized[channel] {
                self.lowpass[channel] = samples[channel];
                self.previous_upper[channel] = 0.0;
                self.previous_velocity[channel] = 0.0;
                self.initialized[channel] = true;
                continue;
            }

            self.lowpass[channel] += (samples[channel] - self.lowpass[channel]) * self.split_alpha;
            base[channel] = self.lowpass[channel];
            upper[channel] = samples[channel] - base[channel];
            let velocity = upper[channel] - self.previous_upper[channel];
            let acceleration = velocity - self.previous_velocity[channel];
            let direction_change = if velocity * self.previous_velocity[channel] < 0.0 {
                velocity.abs().min(self.previous_velocity[channel].abs())
            } else {
                0.0
            };
            let demand = acceleration.abs() * self.second_derivative_scale
                + direction_change
                    * self.first_derivative_scale
                    * PROGRAMME_DIRECTION_CHANGE_WEIGHT;
            linked_demand = linked_demand.max(demand);
            self.previous_upper[channel] = upper[channel];
            self.previous_velocity[channel] = velocity;
        }
        for channel in channel_count..2 {
            self.initialized[channel] = false;
            self.lowpass[channel] = 0.0;
            self.previous_upper[channel] = 0.0;
            self.previous_velocity[channel] = 0.0;
        }

        if strength <= 0.0 {
            self.linked_gain = 1.0;
            return samples;
        }

        let overload = smoothstep_unit(
            (linked_demand - PROGRAMME_ACCELERATION_THRESHOLD)
                / (PROGRAMME_ACCELERATION_FULL_SCALE - PROGRAMME_ACCELERATION_THRESHOLD),
        );
        let target_gain = 1.0 - strength * overload * (1.0 - PROGRAMME_LIMITER_MIN_UPPER_GAIN);
        let envelope_alpha = if target_gain < self.linked_gain {
            self.attack_alpha
        } else {
            self.release_alpha
        };
        self.linked_gain = (self.linked_gain + (target_gain - self.linked_gain) * envelope_alpha)
            .clamp(PROGRAMME_LIMITER_MIN_UPPER_GAIN, 1.0);

        let mut output = samples;
        for channel in 0..channel_count {
            output[channel] = base[channel] + upper[channel] * self.linked_gain;
        }
        output
    }

    fn prepare_sample_rate(&mut self, sample_rate: f64) {
        if self.coefficient_sample_rate == sample_rate {
            return;
        }
        self.coefficient_sample_rate = sample_rate;
        self.split_alpha =
            1.0 - (-std::f64::consts::TAU * PROGRAMME_UPPER_CROSSOVER_HZ / sample_rate).exp();
        self.attack_alpha = 1.0 - (-1.0 / (sample_rate * PROGRAMME_LIMITER_ATTACK_SECONDS)).exp();
        self.release_alpha = 1.0 - (-1.0 / (sample_rate * PROGRAMME_LIMITER_RELEASE_SECONDS)).exp();
        self.first_derivative_scale = sample_rate / 48_000.0;
        self.second_derivative_scale = self.first_derivative_scale * self.first_derivative_scale;
    }
}

// Continuous needle-surface bed for lead-in / deadwax traversal.
#[derive(Clone, Debug)]
struct SurfaceBed {
    region: u8,
    position: f64,
    looping: bool,
    elapsed_frames: f64,
    duration_seconds: f64,
    gain: f64,
    filters: [BiquadLowpass; 2],
}

// One-shot stylus thump: sine 130 Hz → exp → 52 Hz over 70 ms, exp gain envelope.
#[derive(Clone, Copy, Debug)]
struct NeedleThump {
    elapsed_seconds: f64,
    phase: f64,
    gain: f64,
}

// One-shot crackle burst from the surface asset as the stylus settles.
#[derive(Clone, Debug)]
struct SurfaceBurst {
    position: f64,
    elapsed_frames: f64,
    peak: f64,
    filters: [BiquadLowpass; 2],
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcousticConfig {
    #[serde(default = "default_max_rate")]
    pub max_rate: f64,
    #[serde(default = "default_wow_rev_seconds")]
    pub wow_rev_seconds: f64,
    #[serde(default = "default_flutter_hz")]
    pub flutter_hz: f64,
    #[serde(default)]
    pub acoustic_enabled: bool,
    #[serde(default)]
    pub surface_enabled: bool,
    /// Soft cartridge tracing limit derived from source curvature and travel
    /// velocity. This preserves the existing speed-dependent stylus model.
    #[serde(default = "default_stylus_tracing_limit")]
    pub stylus_tracing_limit: f64,
    /// Stereo-linked upper-band programme acceleration limiter. `0` bypasses
    /// it exactly; `1` applies the full soft-knee reduction.
    #[serde(default = "default_high_frequency_acceleration_limit")]
    pub high_frequency_acceleration_limit: f64,
    /// Scales the scratch-excited friction terms: contact noise (with its
    /// acceleration lift), needle-drop impulse noise, and the
    /// slope/curvature source texture. `1` is the historical level, bit for
    /// bit — multiplying by exactly one changes no sample. The surface bed,
    /// dust, groove position noise and wear crackle are untouched.
    #[serde(default = "default_texture_scale")]
    pub texture_scale: f64,
    /// Replaces the final hard clamp with a tanh saturator. `false` keeps
    /// the historical digital clamp, bit for bit. `tanh` has unity slope at
    /// silence, so small signals render identically and only would-be-clipped
    /// peaks fold over — the mechanical saturation a real groove has and a
    /// clamp does not.
    #[serde(default)]
    pub soft_clip: bool,
    /// A magnetic cartridge is a velocity transducer: its output is
    /// proportional to how fast the groove passes the stylus, so playing at
    /// rate `r` yields `r * m(r*t)`. The rate factor is the whole law. It is
    /// exactly 1 at nominal speed, and it reaches silence continuously at
    /// rest, which is why a stopped record is silent — no stop knee needed.
    #[serde(default = "default_true")]
    pub cartridge_velocity_gain: bool,
    /// The speed-dependent half of the phono chain: a groove cut with RIAA
    /// pre-emphasis and replayed off speed no longer cancels the preamp's
    /// fixed de-emphasis. See [`RiaaSpeedTilt`]. Physically the partner of
    /// `cartridge_velocity_gain`; the two are meant to run together.
    #[serde(default = "default_true")]
    pub riaa_speed_tilt: bool,
    /// A fixed, deliberate mismatch of the RIAA pair: the same pre-emphasis /
    /// de-emphasis residue as [`RiaaSpeedTilt`], but held at a constant,
    /// caller-chosen rate rather than following the record. `1.0` is exactly
    /// the standard curve and is bit-exact transparent. Above `1.0` it trades
    /// top end for body — the warmth a matched cut and playback cannot
    /// otherwise produce. Off by default; a seed, not a calibrated hardware
    /// profile.
    #[serde(default = "default_riaa_voicing_rate")]
    pub riaa_voicing_rate: f64,
    /// Opt-in vinyl voicing amount in `[0, 1]`: blends a fixed seed curve
    /// (a low-end preamp shelf plus cartridge/arm top-end loss) over the
    /// transparent master. `0` is bypassed bit-exactly. The full amount is a
    /// clearly audible warmth, not a subtle shelf. See
    /// [`VinylVoicingFilter`].
    #[serde(default)]
    pub vinyl_voicing: f64,
    /// Which [`VINYL_VOICING_CURVES`] shape `vinyl_voicing` blends: `0` coil
    /// load, `1` tip mass, `2` curve drift. Out-of-range values are rejected.
    #[serde(default)]
    pub vinyl_voicing_curve: u32,
}

fn default_true() -> bool {
    true
}

fn default_riaa_voicing_rate() -> f64 {
    1.0
}

fn default_max_rate() -> f64 {
    10.0
}
fn default_wow_rev_seconds() -> f64 {
    WOW_REV_SECONDS
}
fn default_flutter_hz() -> f64 {
    FLUTTER_HZ
}
fn default_stylus_tracing_limit() -> f64 {
    0.0
}
fn default_high_frequency_acceleration_limit() -> f64 {
    0.0
}
fn default_texture_scale() -> f64 {
    1.0
}

impl Default for AcousticConfig {
    fn default() -> Self {
        Self {
            max_rate: default_max_rate(),
            wow_rev_seconds: default_wow_rev_seconds(),
            flutter_hz: default_flutter_hz(),
            acoustic_enabled: false,
            surface_enabled: false,
            cartridge_velocity_gain: default_true(),
            riaa_speed_tilt: default_true(),
            riaa_voicing_rate: default_riaa_voicing_rate(),
            vinyl_voicing: 0.0,
            vinyl_voicing_curve: 0,
            stylus_tracing_limit: default_stylus_tracing_limit(),
            high_frequency_acceleration_limit: default_high_frequency_acceleration_limit(),
            texture_scale: default_texture_scale(),
            soft_clip: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcousticStatus {
    pub position: f64,
    pub effective_rate: f64,
    pub requested_window_position: Option<f64>,
    pub ended: bool,
    pub output_length: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum DeckRecoveryOperation {
    RestReset = 1,
    LockedPlaybackReset = 2,
    MechanicalAdvance = 3,
    ServoCaptureReset = 4,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeckRecoveryDiagnostic {
    pub count: u64,
    pub operation: DeckRecoveryOperation,
    pub error: DeckMechanicalError,
    pub output_sample_rate: f64,
    pub source_sample_rate: f64,
    pub position: f64,
    pub target_position: f64,
    pub requested_hand_rate: f64,
    pub motor_rate: f64,
    pub grip: f64,
    pub platter_rate_before: f64,
    pub record_rate_before: f64,
    pub platter_turns_before: f64,
    pub record_turns_before: f64,
}

#[derive(Clone, Debug)]
struct AcousticReplaySnapshot {
    restore_pending: bool,
    config: AcousticConfig,
    native_rpm: f64,
    deck_state: DeckMechanicalState,
    position: f64,
    target_position: f64,
    rate: f64,
    rate_velocity: f64,
    target_rate: f64,
    wow_phase: f64,
    flutter_phase: f64,
    platter_rotation_turns: f64,
    drag_lowpass_state: Vec<f64>,
    high_frequency_acceleration_limiter: HighFrequencyAccelerationLimiter,
    riaa_tilt: RiaaSpeedTilt,
    riaa_voicing: RiaaSpeedTilt,
    vinyl_voicing: VinylVoicingFilter,
    voicing_mix: f64,
    active: bool,
    needle_lifted: bool,
    hand_contact: bool,
    grip: f64,
    grip_target: f64,
    release_grip: f64,
    movement_gain_state: f64,
    motor_rate: f64,
    motor_delivered_rate: f64,
    unpowered_throw_rate: f64,
    ended: bool,
    contact_impulse: f64,
    last_effective_rate: f64,
    noise_seed: u32,
    last_noise: f64,
    last_output_samples: Vec<f64>,
    last_emitted_samples: Vec<f64>,
    seam_repair_from: Vec<f64>,
    seam_repair_remaining: usize,
    window_miss_frames: usize,
    window_programme_gain: f64,
    frames_since_motion: usize,
    /// Frames between the last two motion samples: how often the host is
    /// sampling the hand, so the target is dead-reckoned for about that
    /// long and no longer.
    motion_interval_frames: usize,
    /// The rate the sample before the last one carried, so the rate's
    /// slope across the last interval can be carried through the next.
    previous_target_rate: f64,
    frames_since_window_request: usize,
    scratch_gate: ScratchGate,
    manual_fader_gain: f64,
    momentary_crossfader_gain: f64,
    momentary_crossfader_mix: f64,
    momentary_crossfader_mix_target: f64,
    audible_crossfader_gain: f64,
    output_gain_current: f64,
    output_gain_target: f64,
    output_gain_step: f64,
    output_gain_remaining_frames: usize,
    surface_bed: Option<SurfaceBed>,
    needle_thump: Option<NeedleThump>,
    needle_burst: Option<SurfaceBurst>,
    // The record's character and its history. A replay is a transaction on
    // the live deck: it plays a take's world — its press dials, its wear —
    // and hands the platter back with the record's own. Without these in
    // the snapshot a replayed take left its dials on the live record, and
    // its wear on the live maps.
    eccentricity_mm: f64,
    warp_mm: f64,
    stylus_tap_degrees: f64,
    stylus_tap_level: f64,
    tap_lowpass_state: [f64; 2],
    angle_gate_sectors: u32,
    angle_gate_depth: f64,
    angle_gate_gain: f64,
    locked_groove_start: f64,
    groove_wear_rate: f64,
    groove_wear: Vec<f32>,
    pressing_seed: u32,
    free_spin_drive_per_second: f64,
    vinyl_vfx: VinylVfxProcessor,
}

/// One revolution of the record, lifted out of the render by platter angle.
///
/// A locked groove is one turn, and a groove cut from it has to be that
/// turn exactly: from the ring's start, for one revolution of the platter,
/// at whatever speed and wow the platter has. Cut by time it is wrong by
/// the pitch, and cut from a tap it is wrong by a buffer. So the render loop
/// watches the platter's angle frame by frame, begins the capture on the
/// frame the ring's start comes round, reseeds the take there (a replay of
/// the log begins from the same seed at the same frame), and ends it on the
/// frame the angle has advanced by one turn. The block is copied after the
/// scene and every gain, so what is kept is what was heard.
struct RevolutionCapture {
    /// The platter angle the ring starts at, as a fraction of a turn: how a
    /// free cut (no ring) finds its start and its end.
    target_phase: f64,
    /// The ring's first source frame, when there is one. A locked groove's
    /// seam is where the *position* comes back to its start, and an
    /// off-centre hole makes the groove lead or lag the platter's angle
    /// within a turn, so a ring is cut on its own seam rather than on the
    /// platter's.
    ring_start: Option<f64>,
    previous_position: f64,
    replay_seed: u32,
    previous_turns: f64,
    begin_turns: Option<f64>,
    start_frame: u64,
    /// The engine's frame the turn closed on (exclusive); zero until then.
    end_frame: u64,
    start_position: f64,
    start_rotation_turns: f64,
    block_start: Option<usize>,
    block_end: Option<usize>,
    channels: usize,
    samples: Vec<f32>,
    done: bool,
    overflow: bool,
}

#[wasm_bindgen]
pub struct ScratchAcousticDsp {
    config: AcousticConfig,
    output_sample_rate: f64,
    source_sample_rate: f64,
    native_rpm: f64,
    deck_state: DeckMechanicalState,
    deck_recovery_count: u64,
    last_deck_recovery: Option<DeckRecoveryDiagnostic>,
    channels: Arc<Vec<Vec<f32>>>,
    /// The vinyl Vfx scene riding this platter: geometry-driven, phase
    /// locked to the record's angle like every other press effect.
    vinyl_vfx: VinylVfxProcessor,
    total_frames: usize,
    window_start: usize,
    window_end: usize,
    position: f64,
    target_position: f64,
    rate: f64,
    rate_velocity: f64,
    target_rate: f64,
    wow_phase: f64,
    flutter_phase: f64,
    platter_rotation_turns: f64,
    drag_lowpass_state: Vec<f64>,
    high_frequency_acceleration_limiter: HighFrequencyAccelerationLimiter,
    riaa_tilt: RiaaSpeedTilt,
    /// Constant-rate RIAA mismatch for optional vinyl warmth. Always run so a
    /// change eases from a warm state and `1.0` stays bit-exact.
    riaa_voicing: RiaaSpeedTilt,
    /// The fixed seed curve blended by `voicing_mix`.
    vinyl_voicing: VinylVoicingFilter,
    /// Eased blend of `vinyl_voicing`, so enabling the stage does not click.
    voicing_mix: f64,
    active: bool,
    needle_lifted: bool,
    hand_contact: bool,
    grip: f64,
    grip_target: f64,
    release_grip: f64,
    movement_gain_state: f64,
    motor_rate: f64,
    motor_delivered_rate: f64,
    unpowered_throw_rate: f64,
    /// Motor-off thrust while coasting, in e-folds of rate per second.
    /// Zero is a bearing and nothing else.
    free_spin_drive_per_second: f64,
    /// How far the spindle hole is punched off centre, in millimetres. The
    /// groove the stylus reads oscillates once per revolution, and the
    /// warble deepens toward the label as the groove radius shrinks —
    /// exactly as a mis-punched pressing behaves.
    eccentricity_mm: f64,
    /// Vertical warp height in millimetres: a once-per-revolution dip in
    /// level as the stylus rides over the high spot.
    warp_mm: f64,
    /// A second stylus this many degrees behind the first. Zero is off.
    /// Its delay is angle, not time, so it tightens with pitch and chases a
    /// scratch correctly.
    stylus_tap_degrees: f64,
    stylus_tap_level: f64,
    tap_lowpass_state: [f64; 2],
    /// Sectors per revolution the angle gate cuts. Zero is off.
    angle_gate_sectors: u32,
    angle_gate_depth: f64,
    angle_gate_gain: f64,
    /// A locked groove's first frame, or negative for none: playback wraps
    /// each revolution inside [start, start + frames-per-turn) until the
    /// host seeks out or clears it.
    locked_groove_start: f64,
    /// Wear accumulation-and-audibility scale. Zero is a mint pressing.
    groove_wear_rate: f64,
    /// Position-indexed wear, one bucket per WEAR_BUCKET_FRAMES of source.
    /// The record remembers where the stylus has been.
    groove_wear: Vec<f32>,
    /// Perturbs the surface-noise hashes so each pressing crackles like its
    /// own copy. Zero is the classic pattern.
    pressing_seed: u32,
    ended: bool,
    contact_impulse: f64,
    last_effective_rate: f64,
    noise_seed: u32,
    last_noise: f64,
    last_output_samples: Vec<f64>,
    last_emitted_samples: Vec<f64>,
    seam_repair_from: Vec<f64>,
    seam_repair_remaining: usize,
    window_miss_frames: usize,
    window_programme_gain: f64,
    frames_since_motion: usize,
    /// Frames between the last two motion samples: how often the host is
    /// sampling the hand, so the target is dead-reckoned for about that
    /// long and no longer.
    motion_interval_frames: usize,
    /// The rate the sample before the last one carried, so the rate's
    /// slope across the last interval can be carried through the next.
    previous_target_rate: f64,
    frames_since_window_request: usize,
    output: Vec<f32>,
    scratch_gate: ScratchGate,
    scratch_gate_trace: Vec<f32>,
    manual_fader_gain: f64,
    momentary_crossfader_gain: f64,
    momentary_crossfader_mix: f64,
    momentary_crossfader_mix_target: f64,
    audible_crossfader_gain: f64,
    output_gain_current: f64,
    output_gain_target: f64,
    output_gain_step: f64,
    output_gain_remaining_frames: usize,
    requested_window_position: Option<f64>,
    surface_asset: Arc<Vec<Vec<f32>>>,
    surface_asset_rate: f64,
    surface_gain_multiplier: f64,
    surface_bed: Option<SurfaceBed>,
    needle_thump: Option<NeedleThump>,
    needle_burst: Option<SurfaceBurst>,
    replay_snapshot: Option<Box<AcousticReplaySnapshot>>,
    revolution_capture: Option<RevolutionCapture>,
    /// Output frames this engine has rendered, the clock a capture's start
    /// is stamped on. The host converts it to its own clock by the frames
    /// rendered since.
    rendered_frame_counter: u64,
}

#[wasm_bindgen]
impl ScratchAcousticDsp {
    #[wasm_bindgen(constructor)]
    pub fn new(output_sample_rate: f64, config: JsValue) -> Result<ScratchAcousticDsp, JsValue> {
        if !output_sample_rate.is_finite() || output_sample_rate <= 0.0 {
            return Err(JsValue::from_str("outputSampleRate must be positive"));
        }
        let config = if config.is_null() || config.is_undefined() {
            AcousticConfig::default()
        } else {
            serde_wasm_bindgen::from_value(config)
                .map_err(|error| JsValue::from_str(&error.to_string()))?
        };
        if !config.max_rate.is_finite() || config.max_rate <= 0.0 {
            return Err(JsValue::from_str("maxRate must be positive"));
        }
        if !valid_unit_interval(config.high_frequency_acceleration_limit) {
            return Err(JsValue::from_str(
                "highFrequencyAccelerationLimit must be between 0 and 1",
            ));
        }
        if !valid_unit_interval(config.stylus_tracing_limit) {
            return Err(JsValue::from_str(
                "stylusTracingLimit must be between 0 and 1",
            ));
        }
        if !valid_texture_scale(config.texture_scale) {
            return Err(JsValue::from_str(
                "textureScale must be between 0 and 4",
            ));
        }
        if !valid_riaa_voicing_rate(config.riaa_voicing_rate) {
            return Err(JsValue::from_str("riaaVoicing must be positive"));
        }
        if !valid_unit_interval(config.vinyl_voicing) {
            return Err(JsValue::from_str(
                "vinylVoicing must be between 0 and 1",
            ));
        }
        if config.vinyl_voicing_curve as usize >= VINYL_VOICING_CURVES.len() {
            return Err(JsValue::from_str("vinylVoicingCurve is out of range"));
        }
        Ok(Self::new_internal(output_sample_rate, config))
    }

    fn new_internal(output_sample_rate: f64, config: AcousticConfig) -> Self {
        let native_rpm = (60.0 / config.wow_rev_seconds.max(1e-6)).clamp(16.0, 90.0);
        let deck_state =
            DeckMechanicalState::new(production_deck_config(output_sample_rate, native_rpm))
                .expect("production deck configuration must be valid");
        Self {
            config,
            output_sample_rate,
            source_sample_rate: 48_000.0,
            native_rpm,
            deck_state,
            deck_recovery_count: 0,
            last_deck_recovery: None,
            channels: Arc::new(Vec::new()),
            total_frames: 0,
            window_start: 0,
            window_end: 0,
            position: 0.0,
            target_position: 0.0,
            rate: 0.0,
            rate_velocity: 0.0,
            target_rate: 0.0,
            wow_phase: 0.0,
            flutter_phase: 0.0,
            platter_rotation_turns: 0.0,
            drag_lowpass_state: Vec::new(),
            high_frequency_acceleration_limiter: HighFrequencyAccelerationLimiter::default(),
            riaa_tilt: RiaaSpeedTilt::new(output_sample_rate),
            riaa_voicing: RiaaSpeedTilt::new(output_sample_rate),
            vinyl_voicing: {
                let mut filter = VinylVoicingFilter::new(output_sample_rate);
                filter.set_curve(config.vinyl_voicing_curve as usize);
                filter
            },
            voicing_mix: config.vinyl_voicing,
            active: false,
            needle_lifted: false,
            hand_contact: false,
            grip: 0.0,
            grip_target: 0.0,
            release_grip: 0.0,
            movement_gain_state: f64::NAN,
            motor_rate: 0.0,
            motor_delivered_rate: 0.0,
            unpowered_throw_rate: 0.0,
            free_spin_drive_per_second: 0.0,
            eccentricity_mm: 0.0,
            warp_mm: 0.0,
            stylus_tap_degrees: 0.0,
            stylus_tap_level: 0.0,
            tap_lowpass_state: [0.0; 2],
            angle_gate_sectors: 0,
            angle_gate_depth: 0.0,
            angle_gate_gain: 1.0,
            locked_groove_start: -1.0,
            groove_wear_rate: 0.0,
            groove_wear: Vec::new(),
            pressing_seed: 0,
            vinyl_vfx: VinylVfxProcessor::new(),
            ended: false,
            contact_impulse: 0.0,
            last_effective_rate: 0.0,
            noise_seed: DEFAULT_REPLAY_NOISE_SEED,
            last_noise: 0.0,
            last_output_samples: Vec::new(),
            last_emitted_samples: Vec::new(),
            seam_repair_from: Vec::new(),
            seam_repair_remaining: 0,
            window_miss_frames: 0,
            window_programme_gain: 1.0,
            frames_since_motion: output_sample_rate as usize,
            motion_interval_frames: 0,
            previous_target_rate: 0.0,
            frames_since_window_request: output_sample_rate as usize,
            output: Vec::new(),
            scratch_gate: ScratchGate::default(),
            scratch_gate_trace: Vec::new(),
            manual_fader_gain: 1.0,
            momentary_crossfader_gain: 1.0,
            momentary_crossfader_mix: 0.0,
            momentary_crossfader_mix_target: 0.0,
            audible_crossfader_gain: 1.0,
            output_gain_current: 1.0,
            output_gain_target: 1.0,
            output_gain_step: 0.0,
            output_gain_remaining_frames: 0,
            requested_window_position: None,
            surface_asset: Arc::new(Vec::new()),
            surface_asset_rate: 48_000.0,
            surface_gain_multiplier: 1.0,
            surface_bed: None,
            needle_thump: None,
            needle_burst: None,
            replay_snapshot: None,
            revolution_capture: None,
            rendered_frame_counter: 0,
        }
    }

    /// Prepares stable Rust-owned channel storage for a direct AudioWorklet
    /// copy. This removes the wasm-bindgen Array traversal from the realtime
    /// window replacement path while retaining Rust ownership of source PCM.
    #[wasm_bindgen(js_name = prepareWindow)]
    pub fn prepare_window(&mut self, channel_count: u32, length: u32) -> Result<(), JsValue> {
        let channel_count = channel_count as usize;
        let length = length as usize;
        if !(1..=2).contains(&channel_count) {
            return Err(JsValue::from_str("window channelCount must be 1 or 2"));
        }
        if length == 0 {
            return Err(JsValue::from_str("window length must be positive"));
        }

        // Window swaps are realtime control work. Reuse the active channel
        // allocations when geometry is stable so a normal progressive swap is
        // one bounded copy per channel rather than allocation + copy + drop.
        let channels = Arc::make_mut(&mut self.channels);
        channels.resize_with(channel_count, Vec::new);
        for channel in channels {
            channel.resize(length, 0.0);
        }
        Ok(())
    }

    #[wasm_bindgen(js_name = windowChannelPtr)]
    pub fn window_channel_ptr(&mut self, channel_index: u32) -> *mut f32 {
        Arc::make_mut(&mut self.channels)
            .get_mut(channel_index as usize)
            .map_or(std::ptr::null_mut(), |channel| channel.as_mut_ptr())
    }

    /// Publishes a fully copied prepared window. No allocation occurs on the
    /// successful path.
    #[wasm_bindgen(js_name = commitWindow)]
    pub fn commit_window(
        &mut self,
        source_sample_rate: f64,
        window_start: u32,
        total_frames: u32,
        reset_position: Option<f64>,
    ) -> Result<(), JsValue> {
        if !source_sample_rate.is_finite() || source_sample_rate <= 0.0 {
            return Err(JsValue::from_str("sourceSampleRate must be positive"));
        }
        let Some(length) = self
            .channels
            .first()
            .map(Vec::len)
            .filter(|length| *length > 0)
        else {
            return Err(JsValue::from_str("window has not been prepared"));
        };
        if self.channels.iter().any(|channel| channel.len() != length) {
            return Err(JsValue::from_str(
                "prepared window channels must have equal lengths",
            ));
        }

        self.source_sample_rate = source_sample_rate;
        self.locked_groove_start = -1.0;
        // Committing a window is a *page*, not a record: a streamed side
        // commits one of these every few seconds, and wear that is
        // reallocated on each of them can never reach the fifty passes it is
        // scaled for. So the map is only rebuilt when its shape changes,
        // which is a differently sized source; wear on the record that is
        // playing survives paging.
        //
        // Clearing wear for a *new* record is the host's call —
        // `resetWear("all")` — because only the host knows that the pressing
        // changed rather than the window.
        let wanted_buckets = if self.groove_wear_rate > 0.0 {
            (total_frames as usize) / WEAR_BUCKET_FRAMES + 1
        } else {
            0
        };
        if self.groove_wear.len() != wanted_buckets {
            self.groove_wear = vec![0.0; wanted_buckets];
        }
        self.window_start = window_start as usize;
        self.window_end = self.window_start.saturating_add(length);
        self.total_frames = (total_frames as usize).max(self.window_end);
        if let Some(position) = reset_position {
            self.reset_position(position);
        }
        Ok(())
    }

    /// Clear the phono-stage filters together. The voicing blend returns to
    /// its configured target so a deck that starts with voicing on is at the
    /// target immediately rather than fading in.
    fn reset_phono_filters(&mut self) {
        self.riaa_tilt.reset();
        self.riaa_voicing.reset();
        self.vinyl_voicing.reset();
        self.voicing_mix = self.config.vinyl_voicing;
    }

    #[wasm_bindgen(js_name = clearWindow)]
    pub fn clear_window(&mut self) {
        self.channels = Arc::new(Vec::new());
        self.total_frames = 0;
        self.window_start = 0;
        self.window_end = 0;
        // Loading a new groove clears the read head, not the physical platter.
        // Keep motor velocity and absolute phase continuous while the next PCM
        // window becomes available.
        self.position = 0.0;
        self.target_position = 0.0;
        self.target_rate = 0.0;
        self.frames_since_motion = 0;
        self.last_output_samples.clear();
        self.high_frequency_acceleration_limiter.reset();
        self.reset_phono_filters();
        self.window_miss_frames = 0;
        self.window_programme_gain = 1.0;
        self.ended = false;
    }

    #[wasm_bindgen(js_name = start)]
    pub fn start(&mut self) {
        self.active = true;
        self.grip = 0.0;
        self.release_grip = 0.0;
        self.movement_gain_state = f64::NAN;
        self.grip_target = 1.0;
        self.motor_delivered_rate = 0.0;
        self.hand_contact = true;
        // Original: `this.position || this.targetPosition || 0` — first non-zero wins.
        let seed_position = if self.position != 0.0 {
            self.position
        } else {
            self.target_position
        };
        self.position = self.clamp_source_position(seed_position);
        self.target_position = self.position;
        self.rate = 0.0;
        self.rate_velocity = 0.0;
        self.target_rate = 0.0;
        self.last_effective_rate = 0.0;
        self.reset_deck_to_rest_at_current_turns();
        self.frames_since_motion = 0;
        self.contact_impulse = 0.0;
        self.last_output_samples.clear();
        self.high_frequency_acceleration_limiter.reset();
        self.reset_phono_filters();
        self.window_miss_frames = 0;
        self.window_programme_gain = 1.0;
        self.ended = false;
    }

    #[wasm_bindgen(js_name = stop)]
    pub fn stop(&mut self) {
        self.active = false;
        self.hand_contact = false;
        self.grip_target = 0.0;
        self.scratch_gate.release();
        self.target_rate = 0.0;
        self.unpowered_throw_rate = 0.0;
        self.contact_impulse = 0.0;
        self.last_effective_rate = 0.0;
    }

    #[wasm_bindgen(js_name = setEffects)]
    pub fn set_effects(&mut self, acoustic_enabled: bool, surface_enabled: bool) {
        self.config.acoustic_enabled = acoustic_enabled;
        self.config.surface_enabled = surface_enabled;
        if !surface_enabled {
            self.contact_impulse = 0.0;
            self.last_noise = 0.0;
            self.surface_bed = None;
            self.needle_thump = None;
            self.needle_burst = None;
        }
    }

    #[wasm_bindgen(js_name = setManualFaderGain)]
    pub fn set_manual_fader_gain(&mut self, gain: f64) -> Result<(), JsValue> {
        if !valid_unit_interval(gain) {
            return Err(JsValue::from_str("manualFaderGain must be between 0 and 1"));
        }
        self.manual_fader_gain = gain;
        Ok(())
    }

    #[wasm_bindgen(js_name = setManualCrossfader)]
    pub fn set_manual_crossfader(&mut self, position: f64) -> Result<(), JsValue> {
        if !valid_unit_interval(position) {
            return Err(JsValue::from_str(
                "manualCrossfader must be between 0 and 1",
            ));
        }
        self.manual_fader_gain =
            f64::from(sharp_crossfader_gains(position as f32, DEFAULT_SHARP_CROSSFADER_WIDTH).0);
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = manualFaderGain)]
    pub fn manual_fader_gain(&self) -> f64 {
        self.manual_fader_gain
    }

    /// Overrides the selected technique while the host control is active.
    /// The release returns control to the technique through a de-click ramp.
    #[wasm_bindgen(js_name = setMomentaryCrossfaderOverride)]
    pub fn set_momentary_crossfader_override(&mut self, active: bool, open: bool) {
        self.set_crossfader_touch_override(active, f64::from(open));
    }

    /// Gives a touched host fader temporary control of the audible gate.
    /// Releasing the fader returns control to the selected scratch technique.
    pub fn set_crossfader_touch_override(&mut self, active: bool, gain: f64) {
        if active && gain.is_finite() {
            self.momentary_crossfader_gain = gain.clamp(0.0, 1.0);
        }
        self.momentary_crossfader_mix_target = f64::from(active);
    }

    /// Reports the final audible fader gain after all technique and host input.
    #[wasm_bindgen(getter, js_name = audibleCrossfaderGain)]
    pub fn audible_crossfader_gain(&self) -> f64 {
        self.audible_crossfader_gain
    }

    /// Final post-mix gain used by the host for packet and mixer level. A
    /// linear ramp starts from the gain active at the next rendered frame.
    #[wasm_bindgen(js_name = setOutputGain)]
    pub fn set_output_gain(&mut self, gain: f64, ramp_ms: f64) -> Result<(), JsValue> {
        if !gain.is_finite() || !(0.0..=MAX_FINAL_OUTPUT_GAIN).contains(&gain) {
            return Err(JsValue::from_str("outputGain must be between 0 and 4"));
        }
        if !ramp_ms.is_finite() || !(0.0..=MAX_FINAL_OUTPUT_GAIN_RAMP_MS).contains(&ramp_ms) {
            return Err(JsValue::from_str(
                "outputGain rampMs must be between 0 and 60000",
            ));
        }
        if ramp_ms == 0.0 || gain == self.output_gain_current {
            self.output_gain_current = gain;
            self.output_gain_target = gain;
            self.output_gain_step = 0.0;
            self.output_gain_remaining_frames = 0;
            return Ok(());
        }

        let ramp_frames = (self.output_sample_rate * ramp_ms / 1_000.0)
            .round()
            .max(1.0);
        if !ramp_frames.is_finite() || ramp_frames > usize::MAX as f64 {
            return Err(JsValue::from_str(
                "outputGain ramp exceeds the supported frame count",
            ));
        }
        self.output_gain_target = gain;
        self.output_gain_remaining_frames = ramp_frames as usize;
        self.output_gain_step =
            (gain - self.output_gain_current) / self.output_gain_remaining_frames.max(1) as f64;
        Ok(())
    }

    #[wasm_bindgen(js_name = captureReplayState)]
    pub fn capture_replay_state(&mut self) {
        if let Some(snapshot) = self.replay_snapshot.as_mut() {
            snapshot.config = self.config;
            snapshot.native_rpm = self.native_rpm;
            snapshot.deck_state = self.deck_state;
            snapshot.position = self.position;
            snapshot.target_position = self.target_position;
            snapshot.rate = self.rate;
            snapshot.rate_velocity = self.rate_velocity;
            snapshot.target_rate = self.target_rate;
            snapshot.wow_phase = self.wow_phase;
            snapshot.flutter_phase = self.flutter_phase;
            snapshot.platter_rotation_turns = self.platter_rotation_turns;
            snapshot
                .drag_lowpass_state
                .clone_from(&self.drag_lowpass_state);
            snapshot
                .high_frequency_acceleration_limiter
                .clone_from(&self.high_frequency_acceleration_limiter);
            snapshot.riaa_tilt.clone_from(&self.riaa_tilt);
            snapshot.riaa_voicing.clone_from(&self.riaa_voicing);
            snapshot.vinyl_voicing.clone_from(&self.vinyl_voicing);
            snapshot.voicing_mix = self.voicing_mix;
            snapshot.active = self.active;
            snapshot.needle_lifted = self.needle_lifted;
            snapshot.hand_contact = self.hand_contact;
            snapshot.grip = self.grip;
            snapshot.grip_target = self.grip_target;
            snapshot.motor_rate = self.motor_rate;
            snapshot.motor_delivered_rate = self.motor_delivered_rate;
            snapshot.unpowered_throw_rate = self.unpowered_throw_rate;
            snapshot.ended = self.ended;
            snapshot.contact_impulse = self.contact_impulse;
            snapshot.last_effective_rate = self.last_effective_rate;
            snapshot.noise_seed = self.noise_seed;
            snapshot.last_noise = self.last_noise;
            snapshot
                .last_output_samples
                .clone_from(&self.last_output_samples);
            snapshot
                .last_emitted_samples
                .clone_from(&self.last_emitted_samples);
            snapshot
                .seam_repair_from
                .clone_from(&self.seam_repair_from);
            snapshot.seam_repair_remaining = self.seam_repair_remaining;
            snapshot.window_miss_frames = self.window_miss_frames;
            snapshot.window_programme_gain = self.window_programme_gain;
            snapshot.frames_since_motion = self.frames_since_motion;
            snapshot.motion_interval_frames = self.motion_interval_frames;
            snapshot.previous_target_rate = self.previous_target_rate;
            snapshot.frames_since_window_request = self.frames_since_window_request;
            snapshot.scratch_gate.clone_from(&self.scratch_gate);
            snapshot.manual_fader_gain = self.manual_fader_gain;
            snapshot.momentary_crossfader_gain = self.momentary_crossfader_gain;
            snapshot.momentary_crossfader_mix = self.momentary_crossfader_mix;
            snapshot.momentary_crossfader_mix_target = self.momentary_crossfader_mix_target;
            snapshot.audible_crossfader_gain = self.audible_crossfader_gain;
            snapshot.output_gain_current = self.output_gain_current;
            snapshot.output_gain_target = self.output_gain_target;
            snapshot.output_gain_step = self.output_gain_step;
            snapshot.output_gain_remaining_frames = self.output_gain_remaining_frames;
            snapshot.surface_bed.clone_from(&self.surface_bed);
            snapshot.needle_thump = self.needle_thump;
            snapshot.needle_burst.clone_from(&self.needle_burst);
            snapshot.eccentricity_mm = self.eccentricity_mm;
            snapshot.warp_mm = self.warp_mm;
            snapshot.stylus_tap_degrees = self.stylus_tap_degrees;
            snapshot.stylus_tap_level = self.stylus_tap_level;
            snapshot.tap_lowpass_state = self.tap_lowpass_state;
            snapshot.angle_gate_sectors = self.angle_gate_sectors;
            snapshot.angle_gate_depth = self.angle_gate_depth;
            snapshot.angle_gate_gain = self.angle_gate_gain;
            snapshot.locked_groove_start = self.locked_groove_start;
            snapshot.groove_wear_rate = self.groove_wear_rate;
            snapshot.groove_wear.clone_from(&self.groove_wear);
            snapshot.pressing_seed = self.pressing_seed;
            snapshot.free_spin_drive_per_second = self.free_spin_drive_per_second;
            snapshot.vinyl_vfx.clone_from(&self.vinyl_vfx);
            snapshot.restore_pending = true;
            return;
        }

        self.replay_snapshot = Some(Box::new(AcousticReplaySnapshot {
            restore_pending: true,
            config: self.config,
            native_rpm: self.native_rpm,
            deck_state: self.deck_state,
            position: self.position,
            target_position: self.target_position,
            rate: self.rate,
            rate_velocity: self.rate_velocity,
            target_rate: self.target_rate,
            wow_phase: self.wow_phase,
            flutter_phase: self.flutter_phase,
            platter_rotation_turns: self.platter_rotation_turns,
            drag_lowpass_state: self.drag_lowpass_state.clone(),
            high_frequency_acceleration_limiter: self.high_frequency_acceleration_limiter.clone(),
            riaa_tilt: self.riaa_tilt.clone(),
            riaa_voicing: self.riaa_voicing.clone(),
            vinyl_voicing: self.vinyl_voicing.clone(),
            voicing_mix: self.voicing_mix,
            active: self.active,
            needle_lifted: self.needle_lifted,
            hand_contact: self.hand_contact,
            grip: self.grip,
            grip_target: self.grip_target,
            release_grip: self.release_grip,
            movement_gain_state: self.movement_gain_state,
            motor_rate: self.motor_rate,
            motor_delivered_rate: self.motor_delivered_rate,
            unpowered_throw_rate: self.unpowered_throw_rate,
            ended: self.ended,
            contact_impulse: self.contact_impulse,
            last_effective_rate: self.last_effective_rate,
            noise_seed: self.noise_seed,
            last_noise: self.last_noise,
            last_output_samples: self.last_output_samples.clone(),
            last_emitted_samples: self.last_emitted_samples.clone(),
            seam_repair_from: self.seam_repair_from.clone(),
            seam_repair_remaining: self.seam_repair_remaining,
            window_miss_frames: self.window_miss_frames,
            window_programme_gain: self.window_programme_gain,
            frames_since_motion: self.frames_since_motion,
            motion_interval_frames: self.motion_interval_frames,
            previous_target_rate: self.previous_target_rate,
            frames_since_window_request: self.frames_since_window_request,
            scratch_gate: self.scratch_gate.clone(),
            manual_fader_gain: self.manual_fader_gain,
            momentary_crossfader_gain: self.momentary_crossfader_gain,
            momentary_crossfader_mix: self.momentary_crossfader_mix,
            momentary_crossfader_mix_target: self.momentary_crossfader_mix_target,
            audible_crossfader_gain: self.audible_crossfader_gain,
            output_gain_current: self.output_gain_current,
            output_gain_target: self.output_gain_target,
            output_gain_step: self.output_gain_step,
            output_gain_remaining_frames: self.output_gain_remaining_frames,
            surface_bed: self.surface_bed.clone(),
            needle_thump: self.needle_thump,
            needle_burst: self.needle_burst.clone(),
            eccentricity_mm: self.eccentricity_mm,
            warp_mm: self.warp_mm,
            stylus_tap_degrees: self.stylus_tap_degrees,
            stylus_tap_level: self.stylus_tap_level,
            tap_lowpass_state: self.tap_lowpass_state,
            angle_gate_sectors: self.angle_gate_sectors,
            angle_gate_depth: self.angle_gate_depth,
            angle_gate_gain: self.angle_gate_gain,
            locked_groove_start: self.locked_groove_start,
            groove_wear_rate: self.groove_wear_rate,
            groove_wear: self.groove_wear.clone(),
            pressing_seed: self.pressing_seed,
            free_spin_drive_per_second: self.free_spin_drive_per_second,
            vinyl_vfx: self.vinyl_vfx.clone(),
        }));
    }

    #[wasm_bindgen(js_name = restoreReplayState)]
    pub fn restore_replay_state(&mut self) -> bool {
        let Some(snapshot) = self.replay_snapshot.as_mut() else {
            return false;
        };
        if !snapshot.restore_pending {
            return false;
        }

        // Keep both ownership slots alive. The audio callback only swaps
        // values and buffer handles; it never drops the snapshot or its Vecs.
        macro_rules! swap_replay_field {
            ($field:ident) => {
                std::mem::swap(&mut self.$field, &mut snapshot.$field)
            };
        }
        swap_replay_field!(config);
        swap_replay_field!(native_rpm);
        swap_replay_field!(deck_state);
        swap_replay_field!(position);
        swap_replay_field!(target_position);
        swap_replay_field!(rate);
        swap_replay_field!(rate_velocity);
        swap_replay_field!(target_rate);
        swap_replay_field!(wow_phase);
        swap_replay_field!(flutter_phase);
        swap_replay_field!(platter_rotation_turns);
        swap_replay_field!(drag_lowpass_state);
        swap_replay_field!(high_frequency_acceleration_limiter);
        swap_replay_field!(riaa_tilt);
        swap_replay_field!(riaa_voicing);
        swap_replay_field!(vinyl_voicing);
        swap_replay_field!(voicing_mix);
        swap_replay_field!(active);
        swap_replay_field!(needle_lifted);
        swap_replay_field!(hand_contact);
        swap_replay_field!(grip);
        swap_replay_field!(release_grip);
        swap_replay_field!(movement_gain_state);
        swap_replay_field!(grip_target);
        swap_replay_field!(motor_rate);
        swap_replay_field!(motor_delivered_rate);
        swap_replay_field!(unpowered_throw_rate);
        swap_replay_field!(ended);
        swap_replay_field!(contact_impulse);
        swap_replay_field!(last_effective_rate);
        swap_replay_field!(noise_seed);
        swap_replay_field!(last_noise);
        swap_replay_field!(last_output_samples);
        swap_replay_field!(last_emitted_samples);
        swap_replay_field!(seam_repair_from);
        swap_replay_field!(seam_repair_remaining);
        swap_replay_field!(window_miss_frames);
        swap_replay_field!(window_programme_gain);
        swap_replay_field!(frames_since_motion);
        swap_replay_field!(motion_interval_frames);
        swap_replay_field!(previous_target_rate);
        swap_replay_field!(frames_since_window_request);
        swap_replay_field!(scratch_gate);
        swap_replay_field!(manual_fader_gain);
        swap_replay_field!(momentary_crossfader_gain);
        swap_replay_field!(momentary_crossfader_mix);
        swap_replay_field!(momentary_crossfader_mix_target);
        swap_replay_field!(audible_crossfader_gain);
        swap_replay_field!(output_gain_current);
        swap_replay_field!(output_gain_target);
        swap_replay_field!(output_gain_step);
        swap_replay_field!(output_gain_remaining_frames);
        swap_replay_field!(surface_bed);
        swap_replay_field!(needle_thump);
        swap_replay_field!(needle_burst);
        swap_replay_field!(eccentricity_mm);
        swap_replay_field!(warp_mm);
        swap_replay_field!(stylus_tap_degrees);
        swap_replay_field!(stylus_tap_level);
        swap_replay_field!(tap_lowpass_state);
        swap_replay_field!(angle_gate_sectors);
        swap_replay_field!(angle_gate_depth);
        swap_replay_field!(angle_gate_gain);
        swap_replay_field!(locked_groove_start);
        swap_replay_field!(groove_wear_rate);
        swap_replay_field!(groove_wear);
        swap_replay_field!(pressing_seed);
        swap_replay_field!(free_spin_drive_per_second);
        swap_replay_field!(vinyl_vfx);
        snapshot.restore_pending = false;
        true
    }

    /// Reinitializes every dynamic input that can color a recorded take.
    /// The caller must capture the live state first and restore it after the
    /// replay transaction. Static PCM, surface assets and selected controls
    /// remain in place.
    #[wasm_bindgen(js_name = beginDeterministicReplay)]
    pub fn begin_deterministic_replay(
        &mut self,
        position: f64,
        rotation_turns: f64,
        replay_seed: u32,
    ) -> Result<(), JsValue> {
        self.begin_deterministic_replay_from(position, rotation_turns, replay_seed, 0.0)
    }

    /// `beginDeterministicReplay`, with the platter already turning.
    ///
    /// A take punched in on a running record starts at speed. Beginning its
    /// replay from rest put a spin-up under the first beat that the take
    /// never had: the motor model ramped from zero toward the transport the
    /// first events set. `rate` is the platter's rate at punch-in, in units
    /// of the native speed, and the platter is reset *to* it rather than
    /// toward it.
    #[wasm_bindgen(js_name = beginDeterministicReplayFrom)]
    pub fn begin_deterministic_replay_from(
        &mut self,
        position: f64,
        rotation_turns: f64,
        replay_seed: u32,
        rate: f64,
    ) -> Result<(), JsValue> {
        self.begin_replay(position, rotation_turns, replay_seed, rate)
            .map_err(JsValue::from_str)
    }
}

impl ScratchAcousticDsp {
    /// Takes another engine's record as this engine's own, without a copy.
    ///
    /// The source PCM sits behind an `Arc`; a headless engine that renders a
    /// take's log to audio shares the live deck's record for the length of
    /// the render and lets it go. The record's speed, seed and wear come
    /// with it, so a log with no world replays on the record as it is.
    /// Transport starts from rest at the top of the side.
    pub fn share_source(&mut self, other: &ScratchAcousticDsp) {
        self.channels = Arc::clone(&other.channels);
        self.source_sample_rate = other.source_sample_rate;
        self.window_start = other.window_start;
        self.window_end = other.window_end;
        self.total_frames = other.total_frames;
        self.native_rpm = other.native_rpm;
        self.pressing_seed = other.pressing_seed;
        self.groove_wear_rate = other.groove_wear_rate;
        self.groove_wear = other.groove_wear.clone();
        self.vinyl_vfx.restore_halo_wear(&other.vinyl_vfx.halo_wear_map());
        self.locked_groove_start = -1.0;
        self.reset_position(0.0);
    }

    /// `armRevolutionCapture`, off the wasm binding.
    pub fn arm_revolution(
        &mut self,
        start_position: f64,
        max_frames: u32,
        replay_seed: u32,
    ) -> Result<(), &'static str> {
        if !start_position.is_finite() {
            return Err("revolution start must be finite");
        }
        // `max_frames` of zero is a stamp: the capture marks where the ring
        // came round and when it closed, and keeps no audio — a groove that
        // is its log wants the punch-in, not the wav.
        let frames_per_turn = self.source_sample_rate * 60.0 / self.native_rpm.max(f64::EPSILON);
        if !(frames_per_turn.is_finite() && frames_per_turn > 0.0) {
            return Err("the record has no revolution");
        }
        // The ring's start as a platter angle: where the platter is now,
        // less how far into the ring the needle has got.
        let target_phase = if start_position < 0.0 {
            self.platter_rotation_turns.rem_euclid(1.0)
        } else {
            let into_ring = (self.position - start_position).rem_euclid(frames_per_turn);
            (self.platter_rotation_turns - into_ring / frames_per_turn).rem_euclid(1.0)
        };
        self.revolution_capture = Some(RevolutionCapture {
            target_phase,
            ring_start: if start_position < 0.0 { None } else { Some(start_position) },
            previous_position: self.position,
            replay_seed,
            previous_turns: self.platter_rotation_turns,
            begin_turns: None,
            start_frame: 0,
            end_frame: 0,
            start_position: 0.0,
            start_rotation_turns: 0.0,
            block_start: None,
            block_end: None,
            channels: 0,
            samples: Vec::with_capacity(max_frames as usize * 2),
            done: false,
            overflow: false,
        });
        Ok(())
    }

    /// One render frame, after the platter has stepped: does the ring's
    /// start come round on this frame, or has a full turn gone by?
    fn revolution_capture_frame(&mut self, frame: usize) {
        let turns = self.platter_rotation_turns;
        let position = self.position;
        let counter = self.rendered_frame_counter;
        let mut reseed = None;
        if let Some(capture) = self.revolution_capture.as_mut() {
            if capture.done {
                return;
            }
            let previous = capture.previous_turns;
            capture.previous_turns = turns;
            let previous_position = capture.previous_position;
            capture.previous_position = position;
            // Did the start come round on this frame? On a ring, that is
            // the position reaching the ring's first frame going forward —
            // by passing it, or by the ring wrapping back onto it. Free,
            // it is the platter reaching the angle it was armed at.
            let crossed = match capture.ring_start {
                Some(start) => {
                    let wrapped = position < previous_position
                        && previous_position - position > 1.0;
                    let passed = previous_position < start && position >= start;
                    (wrapped && (position - start).abs() < 1.0) || passed
                }
                None => {
                    if turns <= previous {
                        false
                    } else {
                        let target = capture.target_phase + (previous - capture.target_phase).ceil();
                        turns >= target
                    }
                }
            };
            match capture.begin_turns {
                None => {
                    if crossed {
                        capture.begin_turns = Some(turns);
                        capture.start_frame = counter + frame as u64;
                        capture.start_position = position;
                        capture.start_rotation_turns = turns;
                        capture.block_start = Some(frame);
                        reseed = Some(capture.replay_seed);
                    }
                }
                Some(begin) => {
                    // One turn on: the ring's seam again, or a full turn of
                    // the platter for a free cut.
                    let closed = match capture.ring_start {
                        Some(_) => crossed && turns > begin + 0.5,
                        None => turns >= begin + 1.0,
                    };
                    if closed {
                        capture.block_end = Some(frame);
                        capture.end_frame = counter + frame as u64;
                        capture.done = true;
                    }
                }
            }
        }
        if let Some(seed) = reseed {
            self.seed_take_capture(seed);
        }
    }

    /// After the block is final: the frames the capture covers, into it.
    fn revolution_capture_copy(&mut self, frames: usize, channels: usize) {
        let Some(capture) = self.revolution_capture.as_mut() else {
            return;
        };
        if capture.begin_turns.is_none() || (capture.done && capture.block_end.is_none()) {
            return;
        }
        let from = capture.block_start.take().unwrap_or(0).min(frames);
        let to = capture.block_end.take().unwrap_or(frames).min(frames);
        if capture.channels == 0 {
            capture.channels = channels.max(1);
        }
        if capture.channels != channels {
            capture.overflow = true;
            capture.done = true;
            return;
        }
        if capture.samples.capacity() == 0 {
            return;
        }
        let wanted = (to.saturating_sub(from)) * channels;
        let room = capture.samples.capacity() - capture.samples.len();
        if wanted > room {
            capture.overflow = true;
            capture.done = true;
        }
        let take = wanted.min(room);
        let start = (from * channels).min(self.output.len());
        let end = (start + take).min(self.output.len());
        capture.samples.extend_from_slice(&self.output[start..end]);
    }

    /// `beginDeterministicReplayFrom`, off the wasm binding.
    ///
    /// The C ABI and the tests come in here: a `JsValue` cannot be built on
    /// a host target, so an invalid argument on the binding's path is an
    /// abort on the phone rather than a refused call.
    pub fn begin_replay(
        &mut self,
        position: f64,
        rotation_turns: f64,
        replay_seed: u32,
        rate: f64,
    ) -> Result<(), &'static str> {
        if !position.is_finite() {
            return Err("replay position must be finite");
        }
        if !rotation_turns.is_finite() {
            return Err("replay rotationTurns must be finite");
        }
        if !rate.is_finite() || rate.abs() > self.config.max_rate {
            return Err("replay rate must be finite and within maxRate");
        }

        self.active = true;
        self.position = self.clamp_source_position(position);
        self.target_position = self.position;
        self.rate = rate;
        self.rate_velocity = 0.0;
        self.target_rate = rate;
        self.wow_phase = rotation_turns.rem_euclid(1.0);
        self.flutter_phase = f64::from(replay_seed) / (f64::from(u32::MAX) + 1.0);
        self.platter_rotation_turns = rotation_turns;
        self.drag_lowpass_state.clear();
        self.high_frequency_acceleration_limiter.reset();
        self.reset_phono_filters();
        self.hand_contact = false;
        self.grip = 0.0;
        self.release_grip = 0.0;
        self.movement_gain_state = f64::NAN;
        self.grip_target = 0.0;
        self.motor_rate = rate;
        self.motor_delivered_rate = rate;
        self.unpowered_throw_rate = 0.0;
        self.ended = false;
        self.contact_impulse = 0.0;
        self.last_effective_rate = rate;
        self.deck_state
            .reset(rate, rate, rotation_turns, rotation_turns)
            .map_err(|_| "replay could not reset the platter")?;
        self.noise_seed = if replay_seed == 0 {
            DEFAULT_REPLAY_NOISE_SEED
        } else {
            replay_seed
        };
        self.last_noise = 0.0;
        self.last_output_samples.clear();
        self.window_miss_frames = 0;
        self.window_programme_gain = 1.0;
        self.frames_since_motion = 0;
        self.frames_since_window_request = self.output_sample_rate as usize;
        self.requested_window_position = None;
        self.scratch_gate.reset_for_replay();
        self.scratch_gate_trace.clear();
        self.momentary_crossfader_gain = 1.0;
        self.momentary_crossfader_mix = 0.0;
        self.momentary_crossfader_mix_target = 0.0;
        self.audible_crossfader_gain = if self.scratch_gate.preset() == ScratchPreset::Baby {
            self.manual_fader_gain
        } else {
            self.scratch_gate.gate()
        };
        self.surface_bed = None;
        self.needle_thump = None;
        self.needle_burst = None;
        Ok(())
    }
}

#[wasm_bindgen]
impl ScratchAcousticDsp {
    /// Arms a one-revolution capture from the ring starting at
    /// `start_position` (source frames; negative means from wherever the
    /// platter is), holding at most `max_frames`, reseeding the take with
    /// `replay_seed` on the frame it begins. See `RevolutionCapture`.
    #[wasm_bindgen(js_name = armRevolutionCapture)]
    pub fn arm_revolution_capture(
        &mut self,
        start_position: f64,
        max_frames: u32,
        replay_seed: u32,
    ) -> Result<(), JsValue> {
        self.arm_revolution(start_position, max_frames, replay_seed)
            .map_err(JsValue::from_str)
    }

    #[wasm_bindgen(js_name = cancelRevolutionCapture)]
    pub fn cancel_revolution_capture(&mut self) {
        self.revolution_capture = None;
    }

    #[wasm_bindgen(getter, js_name = revolutionCaptureArmed)]
    pub fn revolution_capture_armed(&self) -> bool {
        self.revolution_capture.is_some()
    }

    #[wasm_bindgen(getter, js_name = revolutionCaptureBegan)]
    pub fn revolution_capture_began(&self) -> bool {
        self.revolution_capture
            .as_ref()
            .is_some_and(|capture| capture.begin_turns.is_some())
    }

    #[wasm_bindgen(getter, js_name = revolutionCaptureDone)]
    pub fn revolution_capture_done(&self) -> bool {
        self.revolution_capture
            .as_ref()
            .is_some_and(|capture| capture.done)
    }

    #[wasm_bindgen(getter, js_name = revolutionCaptureOverflowed)]
    pub fn revolution_capture_overflowed(&self) -> bool {
        self.revolution_capture
            .as_ref()
            .is_some_and(|capture| capture.overflow)
    }

    /// This engine's rendered-frame counter at the frame the capture began.
    #[wasm_bindgen(getter, js_name = revolutionCaptureStartFrame)]
    pub fn revolution_capture_start_frame(&self) -> f64 {
        self.revolution_capture
            .as_ref()
            .map_or(0.0, |capture| capture.start_frame as f64)
    }

    /// This engine's rendered-frame counter at the frame the capture closed
    /// on (exclusive); zero until it has.
    #[wasm_bindgen(getter, js_name = revolutionCaptureEndFrame)]
    pub fn revolution_capture_end_frame(&self) -> f64 {
        self.revolution_capture
            .as_ref()
            .map_or(0.0, |capture| capture.end_frame as f64)
    }

    #[wasm_bindgen(getter, js_name = revolutionCaptureStartPosition)]
    pub fn revolution_capture_start_position(&self) -> f64 {
        self.revolution_capture
            .as_ref()
            .map_or(0.0, |capture| capture.start_position)
    }

    #[wasm_bindgen(getter, js_name = revolutionCaptureStartRotationTurns)]
    pub fn revolution_capture_start_rotation_turns(&self) -> f64 {
        self.revolution_capture
            .as_ref()
            .map_or(0.0, |capture| capture.start_rotation_turns)
    }

    #[wasm_bindgen(getter, js_name = revolutionCaptureChannels)]
    pub fn revolution_capture_channels(&self) -> u32 {
        self.revolution_capture
            .as_ref()
            .map_or(0, |capture| capture.channels as u32)
    }

    #[wasm_bindgen(getter, js_name = revolutionCaptureFrames)]
    pub fn revolution_capture_frames(&self) -> u32 {
        self.revolution_capture
            .as_ref()
            .map_or(0, |capture| (capture.samples.len() / capture.channels.max(1)) as u32)
    }

    /// The captured revolution, interleaved, and the capture is over.
    #[wasm_bindgen(js_name = takeRevolutionCapture)]
    pub fn take_revolution_capture(&mut self) -> Vec<f32> {
        self.revolution_capture
            .take()
            .map_or_else(Vec::new, |capture| capture.samples)
    }

    /// Output frames rendered so far, the clock `revolutionCaptureStartFrame`
    /// is on.
    #[wasm_bindgen(getter, js_name = renderedFrames)]
    pub fn rendered_frames(&self) -> f64 {
        self.rendered_frame_counter as f64
    }

    #[wasm_bindgen(js_name = setHighFrequencyAccelerationLimit)]
    pub fn set_high_frequency_acceleration_limit(&mut self, strength: f64) -> Result<(), JsValue> {
        if !valid_unit_interval(strength) {
            return Err(JsValue::from_str(
                "highFrequencyAccelerationLimit must be between 0 and 1",
            ));
        }
        self.config.high_frequency_acceleration_limit = strength;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = highFrequencyAccelerationLimit)]
    pub fn high_frequency_acceleration_limit(&self) -> f64 {
        self.config.high_frequency_acceleration_limit
    }

    #[wasm_bindgen(js_name = setStylusTracingLimit)]
    pub fn set_stylus_tracing_limit(&mut self, strength: f64) -> Result<(), JsValue> {
        if !valid_unit_interval(strength) {
            return Err(JsValue::from_str(
                "stylusTracingLimit must be between 0 and 1",
            ));
        }
        self.config.stylus_tracing_limit = strength;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = stylusTracingLimit)]
    pub fn stylus_tracing_limit(&self) -> f64 {
        self.config.stylus_tracing_limit
    }

    #[wasm_bindgen(js_name = setTextureScale)]
    pub fn set_texture_scale(&mut self, scale: f64) -> Result<(), JsValue> {
        if !valid_texture_scale(scale) {
            return Err(JsValue::from_str(
                "textureScale must be between 0 and 4",
            ));
        }
        self.config.texture_scale = scale;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = textureScale)]
    pub fn texture_scale(&self) -> f64 {
        self.config.texture_scale
    }

    /// The speed-dependent half of the phono chain, live. At nominal speed
    /// the tilt is exactly unity, so toggling it only changes off-speed
    /// content — which makes it a clean A/B for how much of scratch's edge
    /// is the pre-emphasis mismatch rather than the groove itself.
    #[wasm_bindgen(js_name = setRiaaSpeedTilt)]
    pub fn set_riaa_speed_tilt(&mut self, enabled: bool) {
        self.config.riaa_speed_tilt = enabled;
    }

    #[wasm_bindgen(getter, js_name = riaaSpeedTilt)]
    pub fn riaa_speed_tilt(&self) -> bool {
        self.config.riaa_speed_tilt
    }

    /// Optional constant-rate RIAA mismatch for vinyl warmth, live. `1.0` is
    /// the standard curve and is bit-exact transparent; values above it trade
    /// top end for body. The rate is clamped to the same `[0.1, 4.0]` window
    /// as the speed tilt.
    #[wasm_bindgen(js_name = setRiaaVoicing)]
    pub fn set_riaa_voicing(&mut self, rate: f64) -> Result<(), JsValue> {
        if !valid_riaa_voicing_rate(rate) {
            return Err(JsValue::from_str("riaaVoicing must be positive"));
        }
        self.config.riaa_voicing_rate = rate;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = riaaVoicing)]
    pub fn riaa_voicing(&self) -> f64 {
        self.config.riaa_voicing_rate
    }

    /// Optional vinyl voicing amount in `[0, 1]`, live. `0` is bypassed
    /// bit-exactly and the blend eases, so toggling it does not click.
    #[wasm_bindgen(js_name = setVinylVoicing)]
    pub fn set_vinyl_voicing(&mut self, amount: f64) -> Result<(), JsValue> {
        if !valid_unit_interval(amount) {
            return Err(JsValue::from_str(
                "vinylVoicing must be between 0 and 1",
            ));
        }
        self.config.vinyl_voicing = amount;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = vinylVoicing)]
    pub fn vinyl_voicing(&self) -> f64 {
        self.config.vinyl_voicing
    }

    /// Selects one of the voicing shapes live: `0` coil load, `1` tip mass,
    /// `2` curve drift. The running filter keeps its delay, so switching does
    /// not click.
    #[wasm_bindgen(js_name = setVinylVoicingCurve)]
    pub fn set_vinyl_voicing_curve(&mut self, curve: u32) -> Result<(), JsValue> {
        if curve as usize >= VINYL_VOICING_CURVES.len() {
            return Err(JsValue::from_str("vinylVoicingCurve is out of range"));
        }
        self.config.vinyl_voicing_curve = curve;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = vinylVoicingCurve)]
    pub fn vinyl_voicing_curve(&self) -> u32 {
        self.config.vinyl_voicing_curve
    }

    #[wasm_bindgen(js_name = setSoftClip)]
    pub fn set_soft_clip(&mut self, enabled: bool) {
        self.config.soft_clip = enabled;
    }

    #[wasm_bindgen(getter, js_name = softClip)]
    pub fn soft_clip(&self) -> bool {
        self.config.soft_clip
    }

    #[wasm_bindgen(js_name = setScratchPreset)]
    pub fn set_scratch_preset(&mut self, name: &str) -> Result<(), JsValue> {
        let preset = name
            .parse::<ScratchPreset>()
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.scratch_gate.set_preset(preset);
        Ok(())
    }

    #[wasm_bindgen(js_name = setScratchClicks)]
    pub fn set_scratch_clicks(&mut self, clicks: u8) {
        self.scratch_gate.set_clicks(clicks);
    }

    #[wasm_bindgen(js_name = setScratchGateAlgorithmVersion)]
    pub fn set_scratch_gate_algorithm_version(&mut self, version: u32) {
        self.scratch_gate.set_algorithm_version(version);
    }

    #[wasm_bindgen(getter, js_name = scratchPreset)]
    pub fn scratch_preset(&self) -> String {
        self.scratch_gate.preset().as_str().to_owned()
    }

    #[wasm_bindgen(getter, js_name = scratchClicks)]
    pub fn scratch_clicks(&self) -> u8 {
        self.scratch_gate.clicks()
    }

    #[wasm_bindgen(getter, js_name = scratchGateAlgorithmVersion)]
    pub fn scratch_gate_algorithm_version(&self) -> u32 {
        self.scratch_gate.algorithm_version()
    }

    #[wasm_bindgen(getter, js_name = scratchGate)]
    pub fn scratch_gate(&self) -> f64 {
        self.scratch_gate.gate()
    }

    #[wasm_bindgen(getter, js_name = scratchGateTarget)]
    pub fn scratch_gate_target(&self) -> f64 {
        self.scratch_gate.target()
    }

    #[wasm_bindgen(getter, js_name = scratchDirection)]
    pub fn scratch_direction(&self) -> i32 {
        i32::from(self.scratch_gate.direction())
    }

    #[wasm_bindgen(getter, js_name = scratchMoving)]
    pub fn scratch_moving(&self) -> bool {
        self.scratch_gate.moving()
    }

    #[wasm_bindgen(getter, js_name = scratchGatePhase)]
    pub fn scratch_gate_phase(&self) -> f64 {
        self.scratch_gate.phase()
    }

    #[wasm_bindgen(getter, js_name = scratchStrokeProgress)]
    pub fn scratch_stroke_progress(&self) -> f64 {
        self.scratch_gate.stroke_progress()
    }

    #[wasm_bindgen(js_name = setNeedleLifted)]
    pub fn set_needle_lifted(&mut self, lifted: bool) {
        self.needle_lifted = lifted;
    }

    /// How firmly the hand owns the record's position: the seconds the
    /// servo takes to close a position error, and the most it may correct
    /// by in rad/s. Non-finite or non-positive values leave that half alone.
    #[wasm_bindgen(js_name = setHandServo)]
    pub fn set_hand_servo(&mut self, stabilization_seconds: f64, max_correction_rad_s: f64) {
        let mut deck_config = self.deck_state.config();
        if stabilization_seconds.is_finite() && stabilization_seconds > 0.0 {
            deck_config.hand_position_stabilization_seconds = stabilization_seconds.clamp(0.001, 2.0);
        }
        if max_correction_rad_s.is_finite() && max_correction_rad_s > 0.0 {
            deck_config.hand_max_position_correction_rad_s = max_correction_rad_s.clamp(0.01, 200.0);
        }
        let telemetry = self.deck_state.telemetry();
        if let Err(error) = self.deck_state.reconfigure(deck_config) {
            self.record_deck_recovery(
                DeckRecoveryOperation::MechanicalAdvance,
                DeckMechanicalError::InvalidConfig(error),
                telemetry,
                self.target_rate,
            );
        }
    }

    #[wasm_bindgen(js_name = setNativeRpm)]
    pub fn set_native_rpm(&mut self, native_rpm: f64) -> Result<(), JsValue> {
        if !native_rpm.is_finite() || native_rpm <= 0.0 {
            return Err(JsValue::from_str("nativeRpm must be positive"));
        }
        let native_rpm = native_rpm.clamp(16.0, 90.0);
        let telemetry = self.deck_state.telemetry();
        let mut deck_config = self.deck_state.config();
        deck_config.nominal_rpm = native_rpm;
        deck_config.hand_max_position_correction_rad_s =
            0.12 * deck_config.nominal_angular_velocity_rad_s();
        self.deck_state
            .reconfigure(deck_config)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.deck_state
            .reset(
                telemetry.platter_rate,
                telemetry.record_rate,
                telemetry.platter_angle_turns,
                telemetry.record_angle_turns,
            )
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.native_rpm = native_rpm;
        Ok(())
    }

    /// Sets how much the record slips: none at zero, loosest mat at one.
    ///
    /// The mat is named for what it does, so the number runs with the name.
    /// Zero is the tightest coupling the deck has — the record turns with the
    /// platter — and one is the loosest mat, which lets the record slide on
    /// after the hand leaves it.
    #[wasm_bindgen(js_name = setSlipmatResponse)]
    pub fn set_slipmat_response(&mut self, slip: f64) -> Result<(), JsValue> {
        if !valid_unit_interval(slip) {
            return Err(JsValue::from_str("slipmatResponse must be between 0 and 1"));
        }
        let reference = production_deck_config(self.output_sample_rate, self.native_rpm);
        let scale = TIGHT_SLIPMAT_COUPLING_SCALE
            + slip * (LOOSE_SLIPMAT_COUPLING_SCALE - TIGHT_SLIPMAT_COUPLING_SCALE);
        let mut deck_config = self.deck_state.config();
        deck_config.slipmat_static_torque_nm = reference.slipmat_static_torque_nm * scale;
        deck_config.slipmat_kinetic_torque_nm = reference.slipmat_kinetic_torque_nm * scale;
        deck_config.slipmat_viscous_torque_nm_per_rad_s =
            reference.slipmat_viscous_torque_nm_per_rad_s * scale;
        // Reconfigure only: a reset here would zero the motor integrator and
        // contact modes on every slider tick, wobbling live playback.
        self.deck_state
            .reconfigure(deck_config)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        Ok(())
    }

    /// Scales the platter bearing's friction: frictionless at zero, the
    /// stock deck at one, heavier beyond.
    ///
    /// This is the only brake on a platter thrown by hand with the motor
    /// off, so it is the knob that decides how long a free spin coasts —
    /// from forever at zero to a fast die-off at the top of the range.
    #[wasm_bindgen(js_name = setBearingFriction)]
    pub fn set_bearing_friction(&mut self, scale: f64) -> Result<(), JsValue> {
        if !scale.is_finite() || scale < 0.0 {
            return Err(JsValue::from_str("bearingFriction must be zero or more"));
        }
        let scale = scale.min(16.0);
        let reference = production_deck_config(self.output_sample_rate, self.native_rpm);
        let mut deck_config = self.deck_state.config();
        deck_config.bearing_static_torque_nm = reference.bearing_static_torque_nm * scale;
        deck_config.bearing_kinetic_torque_nm = reference.bearing_kinetic_torque_nm * scale;
        deck_config.bearing_viscous_torque_nm_per_rad_s =
            reference.bearing_viscous_torque_nm_per_rad_s * scale;
        // Reconfigure only, as the slipmat setter does: a reset would zero
        // the motor integrator and contact modes mid-flight.
        self.deck_state
            .reconfigure(deck_config)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        Ok(())
    }

    /// Sets the motor-off thrust: e-folds of rate per second while the
    /// platter coasts. Zero is a plain bearing; 0.02 is a solar sail's
    /// patience; anything near two doubles the spin faster than a hand
    /// could. Clamped there because beyond it the platter is a turbine.
    #[wasm_bindgen(js_name = setFreeSpinDrive)]
    pub fn set_free_spin_drive(&mut self, per_second: f64) -> Result<(), JsValue> {
        if !per_second.is_finite() || per_second < 0.0 {
            return Err(JsValue::from_str("freeSpinDrive must be zero or more"));
        }
        self.free_spin_drive_per_second = per_second.min(2.0);
        Ok(())
    }

    /// Press defects: the spindle hole's eccentricity and the disc's warp,
    /// both in millimetres, both once-per-revolution and angle-indexed.
    #[wasm_bindgen(js_name = setPressDefects)]
    pub fn set_press_defects(
        &mut self,
        eccentricity_mm: f64,
        warp_mm: f64,
    ) -> Result<(), JsValue> {
        if !eccentricity_mm.is_finite() || eccentricity_mm < 0.0 {
            return Err(JsValue::from_str("eccentricity must be zero or more"));
        }
        if !warp_mm.is_finite() || warp_mm < 0.0 {
            return Err(JsValue::from_str("warp must be zero or more"));
        }
        self.eccentricity_mm = eccentricity_mm.min(3.0);
        self.warp_mm = warp_mm.min(4.0);
        Ok(())
    }

    /// A second stylus riding the same groove `degrees` behind the first,
    /// mixed at `level`. Zero level lifts it off.
    #[wasm_bindgen(js_name = setStylusTap)]
    pub fn set_stylus_tap(
        &mut self,
        degrees: f64,
        level: f64,
    ) -> Result<(), JsValue> {
        if !degrees.is_finite() || !(0.0..=359.0).contains(&degrees) {
            return Err(JsValue::from_str("tap degrees must be 0..=359"));
        }
        if !level.is_finite() || !(0.0..=1.0).contains(&level) {
            return Err(JsValue::from_str("tap level must be 0..=1"));
        }
        self.stylus_tap_degrees = degrees;
        self.stylus_tap_level = level;
        Ok(())
    }

    /// The angle gate: `sectors` openings per revolution, cut to `depth`.
    /// Zero sectors is off. Geometry, not tempo-sync: it stays locked under
    /// scratching and free-spin decay because it is read off the platter.
    #[wasm_bindgen(js_name = setAngleGate)]
    pub fn set_angle_gate(
        &mut self,
        sectors: u32,
        depth: f64,
    ) -> Result<(), JsValue> {
        if sectors > 32 {
            return Err(JsValue::from_str("gate sectors must be 0..=32"));
        }
        if !depth.is_finite() || !(0.0..=1.0).contains(&depth) {
            return Err(JsValue::from_str("gate depth must be 0..=1"));
        }
        self.angle_gate_sectors = sectors;
        self.angle_gate_depth = depth;
        Ok(())
    }

    /// The live vinyl Vfx scene: the shared geometry-driven processor the
    /// offline REMIX render and the iOS deck already play. Scene zero is
    /// off; amount rides 0..=1.
    #[wasm_bindgen(js_name = setVinylVfx)]
    pub fn set_vinyl_vfx(&mut self, scene: u32, amount: f64) -> Result<(), JsValue> {
        if scene > VINYL_VFX_MAX_SCENE {
            return Err(JsValue::from_str("vinyl vfx scene is out of range"));
        }
        if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
            return Err(JsValue::from_str("vinyl vfx amount must be 0..=1"));
        }
        self.vinyl_vfx.set_scene(scene, amount);
        Ok(())
    }

    /// Drops the needle into a locked groove starting at `start_frame`:
    /// playback wraps every revolution inside that ring until the host
    /// passes a negative frame to clear it. Seeks and hand motion are phase
    /// changes inside the ring; they cannot escape it.
    #[wasm_bindgen(js_name = setLockedGroove)]
    pub fn set_locked_groove(&mut self, start_frame: f64) -> Result<(), JsValue> {
        if start_frame.is_nan() {
            return Err(JsValue::from_str("locked groove start must be a number"));
        }
        let previous_position = self.position;
        self.locked_groove_start = if start_frame < 0.0 {
            -1.0
        } else {
            start_frame
        };
        self.enforce_locked_groove();
        if (self.position - previous_position).abs() > f64::EPSILON {
            self.begin_output_seam_repair();
        }
        Ok(())
    }

    /// Groove wear: every pass of the stylus wears where it passed, adding
    /// crackle and dulling the highs in the bars that have been played
    /// most. `rate` scales both how fast wear accrues and how loudly it
    /// reads; zero is a mint pressing. At one, a region is fully worn after
    /// roughly fifty passes.
    #[wasm_bindgen(js_name = setGrooveWear)]
    pub fn set_groove_wear(&mut self, rate: f64) -> Result<(), JsValue> {
        if !rate.is_finite() || rate < 0.0 {
            return Err(JsValue::from_str("wear rate must be zero or more"));
        }
        self.groove_wear_rate = rate.min(8.0);
        if self.groove_wear_rate > 0.0 && self.groove_wear.is_empty() && self.total_frames > 0 {
            self.groove_wear =
                vec![0.0; self.total_frames / WEAR_BUCKET_FRAMES + 1];
        }
        Ok(())
    }

    /// The wear map, for persistence: a record's biography rides with it.
    #[wasm_bindgen(js_name = grooveWearMap)]
    pub fn groove_wear_map(&self) -> Vec<f32> {
        self.groove_wear.clone()
    }

    /// Restores a persisted wear map. Length is reconciled to the loaded
    /// source; a map from another pressing simply wears the wrong places,
    /// which is the caller's mistake to avoid via the record hash.
    #[wasm_bindgen(js_name = restoreGrooveWearMap)]
    pub fn restore_groove_wear_map(&mut self, map: &[f32]) {
        let len = if self.total_frames > 0 {
            self.total_frames / WEAR_BUCKET_FRAMES + 1
        } else {
            map.len()
        };
        let mut restored = vec![0.0_f32; len];
        for (slot, value) in restored.iter_mut().zip(map.iter()) {
            *slot = value.clamp(0.0, 1.0);
        }
        self.groove_wear = restored;
    }

    /// The halo, for a take's world.
    #[wasm_bindgen(js_name = haloWearMap)]
    pub fn halo_wear_map(&self) -> Vec<f32> {
        self.vinyl_vfx.halo_wear_map()
    }

    #[wasm_bindgen(js_name = restoreHaloWearMap)]
    pub fn restore_halo_wear_map(&mut self, map: &[f32]) {
        self.vinyl_vfx.restore_halo_wear(map);
    }

    /// The seed a take is cut under.
    ///
    /// A replay begins from `replay_seed` — its surface noise and its flutter
    /// phase are derived from the take's identity — so the recording has to
    /// have begun from the same place or the two can never agree. Called at
    /// punch-in with the take's seed: the noise generator and the flutter
    /// phase are re-seeded, and nothing else moves, because the platter is
    /// live and a hand may be on it. Noise is noise, so the join is silent.
    #[wasm_bindgen(js_name = seedTakeCapture)]
    pub fn seed_take_capture(&mut self, replay_seed: u32) {
        self.noise_seed = if replay_seed == 0 {
            DEFAULT_REPLAY_NOISE_SEED
        } else {
            replay_seed
        };
        self.last_noise = 0.0;
        self.flutter_phase = f64::from(replay_seed) / (f64::from(u32::MAX) + 1.0);
        // The wow runs on its own clock, and a replay begins it at the
        // platter's angle — so the recording begins it there too. A once-
        // per-revolution phase step, taken while the record is live: a
        // fraction of a cent for one turn.
        self.wow_phase = self.platter_rotation_turns.rem_euclid(1.0);
    }

    /// Clears accumulated wear, by scope.
    ///
    /// Wear is the one thing here meant to outlive a pass, so it only goes
    /// away when something says so — one of these scopes, or a new record
    /// on the platter. The scopes are the three things that actually
    /// accumulate, and nothing else in the engine does:
    ///
    /// - `groove` — the WEAR dial's map, one bucket per
    ///   `WEAR_BUCKET_FRAMES` of source, so it wears where the stylus went.
    /// - `halo` — WORN HALO's bins, indexed by phase within one revolution,
    ///   so it wears where on the *turn* the stylus went. A different
    ///   quantity from `groove`, and cleared separately.
    /// - `polar` — the revolution memory ADJACENT GHOST, THREE NEEDLES and
    ///   SPLIT WALLS read back from, and the filters riding on it.
    /// - `all` — the three above.
    #[wasm_bindgen(js_name = resetWear)]
    pub fn reset_wear(&mut self, scope: &str) -> Result<(), JsValue> {
        let Some(scope) = WearScope::parse(scope) else {
            return Err(JsValue::from_str(
                "wear scope must be groove, halo, polar or all",
            ));
        };
        self.reset_wear_scope(scope);
        Ok(())
    }

    /// The meters' numbers, one scalar at a time.
    ///
    /// `wearSummary` allocates a string, which the render thread must not do
    /// at telemetry rate, so the worklet reads these instead and the summary
    /// is kept for one-shot queries. Each walks its buffer and allocates
    /// nothing.
    #[wasm_bindgen(getter, js_name = grooveWearLevel)]
    pub fn groove_wear_level(&self) -> f64 {
        if self.groove_wear.is_empty() {
            return 0.0;
        }
        self.groove_wear
            .iter()
            .map(|value| f64::from(*value))
            .sum::<f64>()
            / self.groove_wear.len() as f64
    }

    #[wasm_bindgen(getter, js_name = grooveWearPeak)]
    pub fn groove_wear_peak(&self) -> f64 {
        self.groove_wear
            .iter()
            .fold(0.0_f64, |peak, value| peak.max(f64::from(*value)))
    }

    #[wasm_bindgen(getter, js_name = grooveWearBuckets)]
    pub fn groove_wear_buckets(&self) -> u32 {
        self.groove_wear.len() as u32
    }

    #[wasm_bindgen(getter, js_name = haloWearLevel)]
    pub fn halo_wear_level(&self) -> f64 {
        self.vinyl_vfx.wear_level()
    }

    #[wasm_bindgen(getter, js_name = haloWearPeak)]
    pub fn halo_wear_peak(&self) -> f64 {
        self.vinyl_vfx.wear_peak()
    }

    #[wasm_bindgen(getter, js_name = polarFill)]
    pub fn polar_fill(&self) -> f64 {
        self.vinyl_vfx.polar_fill_ratio()
    }

    /// Every accumulator's real allocation, in bytes.
    #[wasm_bindgen(getter, js_name = wearBytes)]
    pub fn wear_bytes(&self) -> u32 {
        (self.vinyl_vfx.memory_bytes()
            + self.groove_wear.len() * std::mem::size_of::<f32>()) as u32
    }

    /// What each accumulator is holding — the numbers behind the meters.
    ///
    /// Levels are 0..=1 and bytes are the real allocation, so a deck can
    /// report what the revolution memory actually costs rather than quoting
    /// a constant that drifts when the bin counts change.
    #[wasm_bindgen(js_name = wearSummary)]
    pub fn wear_summary(&self) -> String {
        let groove_level = if self.groove_wear.is_empty() {
            0.0
        } else {
            self.groove_wear
                .iter()
                .map(|value| f64::from(*value))
                .sum::<f64>()
                / self.groove_wear.len() as f64
        };
        let groove_peak = self
            .groove_wear
            .iter()
            .fold(0.0_f64, |peak, value| peak.max(f64::from(*value)));
        serde_json::json!({
            "grooveRate": self.groove_wear_rate,
            "grooveLevel": groove_level,
            "groovePeak": groove_peak,
            "grooveBuckets": self.groove_wear.len(),
            "grooveBytes": self.groove_wear.len() * std::mem::size_of::<f32>(),
            "grooveBucketFrames": WEAR_BUCKET_FRAMES,
            "haloLevel": self.vinyl_vfx.wear_level(),
            "haloPeak": self.vinyl_vfx.wear_peak(),
            "haloBins": VinylVfxProcessor::wear_bin_count(),
            "polarFill": self.vinyl_vfx.polar_fill_ratio(),
            "polarBins": VinylVfxProcessor::polar_bin_count(),
            "vfxBytes": self.vinyl_vfx.memory_bytes(),
            "scene": self.vinyl_vfx.scene(),
            "totalBytes": self.vinyl_vfx.memory_bytes()
                + self.groove_wear.len() * std::mem::size_of::<f32>(),
        })
        .to_string()
    }

    /// Seeds this pressing's surface character. Two pressings of the same
    /// track crackle like two copies, not two files; zero keeps the classic
    /// pattern.
    #[wasm_bindgen(js_name = setPressingSeed)]
    pub fn set_pressing_seed(&mut self, seed: u32) {
        self.pressing_seed = seed;
    }

    #[wasm_bindgen(getter, js_name = nativeRpm)]
    pub fn native_rpm(&self) -> f64 {
        self.native_rpm
    }

    #[wasm_bindgen(getter, js_name = platterRotationTurns)]
    pub fn platter_rotation_turns(&self) -> f64 {
        self.platter_rotation_turns
    }

    /// Counts rejected deck steps that used the last valid motion state.
    #[wasm_bindgen(getter, js_name = deckRecoveryCount)]
    pub fn deck_recovery_count(&self) -> u64 {
        self.deck_recovery_count
    }

    #[wasm_bindgen(js_name = setMotion)]
    pub fn set_motion(&mut self, position: f64, rate: f64, impulse: f64) {
        self.target_position = self.normalize_locked_groove_position(
            self.clamp_source_position(position),
        );
        self.previous_target_rate = self.target_rate;
        self.target_rate = self.map_rate(rate);
        self.motion_interval_frames = self.frames_since_motion;
        self.frames_since_motion = 0;
        if impulse > 0.0 {
            self.contact_impulse = self.contact_impulse.max(impulse).clamp(0.0, 1.0);
        }
    }

    #[wasm_bindgen(js_name = setTransport)]
    pub fn set_transport(
        &mut self,
        hand_contact: bool,
        motor_rate: f64,
        hand_rate: f64,
        grip: f64,
    ) {
        let released_hand = self.hand_contact && !hand_contact;
        if released_hand {
            // A lifting finger's normal force collapses over a few
            // milliseconds rather than in a single sample.
            self.release_grip = self.grip;
        } else if hand_contact {
            self.release_grip = 0.0;
            self.movement_gain_state = f64::NAN;
        }
        let motor_rate =
            finite_or_zero(motor_rate).clamp(-self.config.max_rate, self.config.max_rate);
        if released_hand && motor_rate.abs() < DEADZONE_RATE {
            self.unpowered_throw_rate = self
                .last_effective_rate
                .clamp(-self.config.max_rate, self.config.max_rate);
        } else if hand_contact || motor_rate.abs() >= DEADZONE_RATE {
            self.unpowered_throw_rate = 0.0;
        }
        self.hand_contact = hand_contact;
        self.grip_target = if hand_contact {
            if grip.is_finite() {
                grip.clamp(0.0, 1.0)
            } else {
                1.0
            }
        } else {
            0.0
        };
        if !hand_contact {
            self.scratch_gate.release();
        }
        self.motor_rate = motor_rate;
        if self.motor_rate != 0.0 {
            self.ended = false;
        }
        if hand_contact {
            self.target_position = self.position;
            self.target_rate = self.map_rate(hand_rate);
            self.frames_since_motion = 0;
        } else {
            self.target_position = self.position;
        }
    }

    #[wasm_bindgen(js_name = setPosition)]
    pub fn set_position(&mut self, position: f64, impulse: f64) {
        self.begin_output_seam_repair();
        self.position = self.normalize_locked_groove_position(
            self.clamp_source_position(position),
        );
        self.target_position = self.position;
        self.high_frequency_acceleration_limiter.reset();
        self.window_miss_frames = 0;
        self.window_programme_gain = 1.0;
        self.ended = false;
        if impulse > 0.0 {
            self.contact_impulse = self.contact_impulse.max(impulse).clamp(0.0, 1.0);
        }
    }

    #[wasm_bindgen(js_name = resetPosition)]
    pub fn reset_position_export(&mut self, position: f64) {
        self.reset_position(position);
    }

    #[wasm_bindgen(js_name = render)]
    pub fn render(&mut self, frame_count: u32, output_channel_count: u32) -> u32 {
        let frame_count = frame_count as usize;
        let output_channel_count = (output_channel_count as usize).clamp(1, 2);
        let vfx_start_turns = self.platter_rotation_turns;
        let vfx_start_position = self.position;
        self.output
            .resize(frame_count.saturating_mul(output_channel_count), 0.0);
        self.output.fill(0.0);
        self.scratch_gate_trace.resize(frame_count, 1.0);
        self.requested_window_position = None;
        if frame_count == 0 {
            return 0;
        }
        if !self.active || self.channels.is_empty() || self.total_frames <= 1 {
            let gate_contact = self.active && self.hand_contact;
            let intent_rate = if gate_contact { self.target_rate } else { 0.0 };
            let rendered_rate = if gate_contact {
                self.last_effective_rate
            } else {
                0.0
            };
            self.advance_scratch_gate_trace(frame_count, gate_contact, intent_rate, rendered_rate);
            self.mix_foley(frame_count, output_channel_count);
            self.apply_crossfader_trace(frame_count, output_channel_count);
            self.apply_output_gain(frame_count, output_channel_count);
            self.apply_vinyl_vfx(
                frame_count,
                output_channel_count,
                vfx_start_turns,
                vfx_start_position,
            );
            self.revolution_capture_copy(frame_count, output_channel_count);
            self.rendered_frame_counter += frame_count as u64;
            return u32::try_from(frame_count).unwrap_or(u32::MAX);
        }
        self.drag_lowpass_state.resize(output_channel_count, 0.0);
        self.last_output_samples.resize(output_channel_count, 0.0);
        let dt = 1.0 / self.output_sample_rate;
        let hold_frames = (self.output_sample_rate * MOTION_HOLD_SECONDS).max(1.0) as usize;
        let hold_release_frames = (self.output_sample_rate * MOTION_HOLD_RELEASE_SECONDS).max(1.0);
        // The hand is trusted to keep moving until the hold says it has
        // stopped — the same span the rate feed-forward was always trusted
        // for. A shorter reach froze the target while the rate still ran,
        // and a firm hand's servo then balanced the two and stalled the
        // record a few frames short of the target.
        let reach_frames = hold_frames;
        let grip_seconds = if self.grip_target > self.grip {
            GRIP_ATTACK_SECONDS
        } else {
            GRIP_RELEASE_SECONDS
        };
        let grip_alpha = 1.0 - (-1.0 / (self.output_sample_rate * grip_seconds)).exp();
        let rate_scale = self.source_sample_rate / self.output_sample_rate;
        let miss_fade_frames = (self.output_sample_rate * WINDOW_MISS_FADE_SECONDS)
            .round()
            .max(1.0);

        let mut rendered_frames = frame_count;
        let mut ended_this_render = false;
        for frame in 0..frame_count {
            // Lock ownership is checked before the source is sampled. A seek
            // or scratch target outside the ring therefore cannot leak even
            // one sample from another revolution.
            self.enforce_locked_groove();
            self.frames_since_motion = self.frames_since_motion.saturating_add(1);
            // The host samples the hand at its pointer rate, and the hand
            // keeps moving in between. Held still until the next sample
            // arrived, the target dragged the record back to where the hand
            // *was*: at sixty hertz a steady stroke became a stop and a
            // lurch every sixteen milliseconds, which is the chop heard
            // under every scratch. The target moves at the hand's own rate
            // until the next sample says otherwise, so a steady hand asks
            // nothing of the servo and only a change of speed does.
            // A hand that is speeding up or slowing down carries on doing so
            // until the next sample: the rate's slope across the last
            // interval is carried through this one. Held flat, a changing
            // hand accrued half the acceleration times the interval squared
            // every sample, which the servo then had to remove as a jerk.
            let reckoned_rate = if self.hand_contact
                && self.frames_since_motion <= reach_frames
                && self.motion_interval_frames > 0
            {
                let slope = (self.target_rate - self.previous_target_rate)
                    / self.motion_interval_frames as f64;
                self.map_rate(self.target_rate + slope * self.frames_since_motion as f64)
            } else {
                self.target_rate
            };
            if self.hand_contact && self.frames_since_motion <= reach_frames {
                self.target_position = self.clamp_source_position(
                    self.target_position + reckoned_rate * rate_scale,
                );
            }
            self.grip += (self.grip_target - self.grip) * grip_alpha;
            // A still hand cannot reclaim angle that slipped underneath it.
            // Once motion input stops, the anchor follows the record instead
            // of winching it back to the original grab frame.
            if self.hand_contact && self.frames_since_motion > hold_frames {
                self.target_position +=
                    (self.position - self.target_position) / (hold_release_frames * 0.25);
            }
            let hand_rate = if self.frames_since_motion > hold_frames {
                self.target_rate
                    * (-((self.frames_since_motion - hold_frames) as f64) / hold_release_frames)
                        .exp()
            } else {
                reckoned_rate
            };
            let held_target_rate = if self.hand_contact {
                hand_rate
            } else if self.motor_rate.abs() >= DEADZONE_RATE {
                self.motor_rate
            } else {
                self.unpowered_throw_rate
            };
            let corrected_rate = self.advance_deck_mechanics(hand_rate);
            self.revolution_capture_frame(frame);
            let abs_rate = corrected_rate.abs();
            let effective_rate = if self.config.acoustic_enabled {
                corrected_rate
                    + sign_nonzero(corrected_rate, held_target_rate)
                        * self.advance_wow_flutter(corrected_rate, rate_scale, abs_rate)
            } else {
                corrected_rate
            };
            // An off-centre hole swings the groove radius the stylus reads
            // once per revolution: pitch deviation is eccentricity over
            // groove radius, so the warble deepens toward the label. Angle-
            // indexed, not clocked — scrub the platter and the warble
            // scrubs with it; a decaying free spin slows its own wobble.
            let effective_rate = if self.eccentricity_mm > 0.0 {
                let walked = if self.total_frames > 0 {
                    (self.position / self.total_frames as f64).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let groove_radius_mm = SINGLE_OUTER_GROOVE_MM
                    + (SINGLE_INNER_GROOVE_MM - SINGLE_OUTER_GROOVE_MM) * walked;
                let deviation = self.eccentricity_mm / groove_radius_mm;
                effective_rate
                    * (1.0
                        + deviation
                            * (self.platter_rotation_turns
                                * std::f64::consts::TAU)
                                .sin())
            } else {
                effective_rate
            };
            // Warp lifts the stylus over the high spot once per revolution;
            // the gate cuts sectors out of the same rotation. Both read the
            // record's angle, and both are smoothed a little so an edge is
            // a chop, not a click.
            let warp_gain = if self.warp_mm > 0.0 {
                let lift = ((self.platter_rotation_turns * std::f64::consts::TAU).sin()
                    * 0.5
                    + 0.5)
                    .powi(3);
                1.0 - (self.warp_mm * 0.18).min(0.8) * lift
            } else {
                1.0
            };
            let gate_target = if self.angle_gate_sectors > 0 {
                let sector_phase = (self.platter_rotation_turns
                    * f64::from(self.angle_gate_sectors))
                .rem_euclid(1.0);
                if sector_phase < 0.5 {
                    1.0
                } else {
                    1.0 - self.angle_gate_depth
                }
            } else {
                1.0
            };
            let gate_alpha =
                1.0 - (-1.0 / (self.output_sample_rate * 0.0015)).exp();
            self.angle_gate_gain += (gate_target - self.angle_gate_gain) * gate_alpha;
            // Wear: the stylus takes a little from wherever it passes, and
            // reads back what every earlier pass has taken.
            let worn = if self.groove_wear_rate > 0.0 && !self.groove_wear.is_empty() {
                let bucket = ((self.position.max(0.0) as usize)
                    / WEAR_BUCKET_FRAMES)
                    .min(self.groove_wear.len() - 1);
                if !self.needle_lifted && abs_rate > DEADZONE_RATE {
                    let accumulated = f64::from(self.groove_wear[bucket])
                        + abs_rate * dt * self.groove_wear_rate;
                    self.groove_wear[bucket] = accumulated.min(1.0) as f32;
                }
                f64::from(self.groove_wear[bucket]) * self.groove_wear_rate.min(1.0)
            } else {
                0.0
            };
            self.scratch_gate_trace[frame] =
                self.scratch_gate
                    .process(dt, self.hand_contact, hand_rate, effective_rate)
                    as f32;
            let movement_gain_target =
                compute_movement_gain(
                    abs_rate,
                    self.config.acoustic_enabled,
                    self.config.cartridge_velocity_gain,
                );
            if self.movement_gain_state.is_nan() {
                // First render after a start or reset: the deck is already
                // wherever it is, so the gain begins there — a deck seeded
                // at speed renders bit-exact from its first frame.
                self.movement_gain_state = movement_gain_target;
            } else {
                let movement_gain_alpha =
                    1.0 - (-1.0 / (self.output_sample_rate * MOVEMENT_GAIN_SECONDS)).exp();
                self.movement_gain_state +=
                    (movement_gain_target - self.movement_gain_state) * movement_gain_alpha;
                // Converged is equal: steady playback must stay bit-exact.
                if (self.movement_gain_state - movement_gain_target).abs() < 1.0e-4 {
                    self.movement_gain_state = movement_gain_target;
                }
            }
            let movement_gain = self.movement_gain_state;
            let surface_noise = if self.config.surface_enabled {
                self.next_noise()
            } else {
                0.0
            };
            let highpassed_noise = if self.config.surface_enabled {
                surface_noise - self.last_noise
            } else {
                0.0
            };
            self.last_noise = surface_noise;
            let near_realtime_distance = (abs_rate - 1.0).abs();
            let realtime_acceleration_dip =
                1.0 - 0.88 * (-(near_realtime_distance * near_realtime_distance) / 0.16).exp();
            let rate_delta = (corrected_rate - self.last_effective_rate).abs();
            let acceleration_noise =
                (rate_delta * 0.00028 * realtime_acceleration_dip).clamp(0.0, 0.0007);
            let contact_noise_gain =
                (compute_contact_noise_gain(abs_rate) + acceleration_noise)
                    * self.config.texture_scale;
            let impulse_noise = if self.config.surface_enabled && self.contact_impulse > 0.0001 {
                self.next_noise() * self.contact_impulse * 0.004 * self.config.texture_scale
            } else {
                0.0
            };
            let groove_surface = if self.config.surface_enabled {
                self.compute_position_surface_noise(self.position, abs_rate)
            } else {
                0.0
            };
            let source_texture_gain = if self.config.acoustic_enabled {
                compute_source_texture_gain(abs_rate, rate_delta) * self.config.texture_scale
            } else {
                0.0
            };
            let dust_fleck = if self.config.surface_enabled {
                self.compute_dust_fleck(self.position, abs_rate)
            } else {
                0.0
            };
            let contact_texture = if self.config.surface_enabled {
                (groove_surface * 0.76 + highpassed_noise * 0.18) * contact_noise_gain
            } else {
                0.0
            };
            // A stylus pinned against a clamped record edge reads nothing:
            // fade the programme out approaching the pin instead of holding
            // a full-level frozen sample there.
            let edge_fade_frames = (self.source_sample_rate * EDGE_FADE_SECONDS).max(1.0);
            let programme_end = self.total_frames.max(self.window_end).saturating_sub(2) as f64;
            let start_distance = self.position.max(0.0);
            let end_distance = (programme_end - self.position).max(0.0);
            // Only the edge being pushed into fades; playing away from an
            // edge stays bit-exact.
            let pinned_distance = if effective_rate < 0.0 {
                start_distance
            } else if effective_rate > 0.0 {
                end_distance
            } else {
                start_distance.min(end_distance)
            };
            let edge_gain = if self.surface_bed.is_some() {
                1.0
            } else {
                smoothstep_unit(pinned_distance / edge_fade_frames)
            };
            let source_direction = sign_nonzero(effective_rate, held_target_rate);
            let drag_alpha = if self.config.acoustic_enabled {
                self.drag_lowpass_alpha(abs_rate)
            } else {
                1.0
            };
            let mut missed_window = false;
            let mut programme = [0.0_f64; 2];
            let mut source_textures = [0.0_f64; 2];

            // The preamp's de-emphasis curve does not move with the record, so
            // the tilt is shaped by how fast the groove is actually passing.
            if self.config.riaa_speed_tilt {
                let alpha =
                    1.0 - (-1.0 / (self.output_sample_rate * MOVEMENT_GAIN_SECONDS)).exp();
                self.riaa_tilt.follow_rate(abs_rate, alpha);
            }
            // The opt-in fixed mismatch eases toward its configured rate from
            // the same warm state, so a change never restructures the filter in
            // one sample. At the default 1.0 it is bit-exact and costless.
            {
                let alpha =
                    1.0 - (-1.0 / (self.output_sample_rate * MOVEMENT_GAIN_SECONDS)).exp();
                self.riaa_voicing
                    .follow_rate(self.config.riaa_voicing_rate, alpha);
            }
            // Same easing for the voicing blend, so switching the stage on or
            // off ramps over a few milliseconds instead of stepping.
            let voicing_target = self.config.vinyl_voicing.clamp(0.0, 1.0);
            if self.voicing_mix.is_nan() {
                self.voicing_mix = voicing_target;
            } else {
                let voicing_alpha =
                    1.0 - (-1.0 / (self.output_sample_rate * MOVEMENT_GAIN_SECONDS)).exp();
                self.voicing_mix += (voicing_target - self.voicing_mix) * voicing_alpha;
                // Converged is equal: a fully-on or fully-off stage is exact.
                if (self.voicing_mix - voicing_target).abs() < 1.0e-6 {
                    self.voicing_mix = voicing_target;
                }
            }
            let voicing_mix = self.voicing_mix;
            // The curve only moves when the host selects another character.
            let voicing_curve = self.config.vinyl_voicing_curve as usize;
            if self.vinyl_voicing.curve() != voicing_curve {
                self.vinyl_voicing.set_curve(voicing_curve);
            }

            for channel_index in 0..output_channel_count {
                if self.needle_lifted {
                    continue;
                }
                let source_index = channel_index.min(self.channels.len() - 1);
                // Original: a stationary stylus (movementGain 0) never reads the window and
                // never flags a window miss — the sample is a plain 0 through the drag filter.
                let detail = if movement_gain > 0.0 {
                    self.sample_channel(source_index, self.position, effective_rate * rate_scale)
                } else {
                    Some((0.0, 0.0, 0.0))
                };
                let (music, source_texture) = match detail {
                    None => {
                        missed_window = true;
                        (self.last_output_samples[channel_index], 0.0)
                    }
                    Some((sampled, slope, curvature)) => {
                        let drag_state = self.drag_lowpass_state[channel_index];
                        let tracing_alpha = stylus_tracing_alpha(
                            drag_alpha,
                            curvature,
                            abs_rate,
                            self.config.stylus_tracing_limit,
                        )
                        // A worn groove reads dull before it reads noisy.
                            * (1.0 - 0.45 * worn);
                        let filtered = drag_state + (sampled - drag_state) * tracing_alpha;
                        self.drag_lowpass_state[channel_index] = filtered;
                        let music = filtered * movement_gain * OUTPUT_GAIN;
                        self.last_output_samples[channel_index] = music;
                        let texture = ((slope * 0.48 + curvature * 0.86) * source_direction)
                            .clamp(-1.0, 1.0)
                            * source_texture_gain;
                        (music, texture)
                    }
                };
                // The second stylus reads the same spiral a fixed angle
                // behind: delay measured in degrees, so it tightens with
                // pitch and chases a scratch. A touch duller than the first
                // stylus, as a trailing needle is.
                let music = if self.stylus_tap_level > 0.0
                    && movement_gain > 0.0
                    && !self.needle_lifted
                {
                    let frames_per_turn = self.source_sample_rate * 60.0
                        / self.native_rpm.max(1.0);
                    let tap_position = self.position
                        - self.stylus_tap_degrees / 360.0 * frames_per_turn;
                    let tap = if tap_position >= 0.0 {
                        self.sample_channel(
                            source_index,
                            tap_position,
                            effective_rate * rate_scale,
                        )
                        .map(|(sample, _, _)| sample)
                        .unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    let state = self.tap_lowpass_state[channel_index];
                    let dulled = state + (tap - state) * 0.35;
                    self.tap_lowpass_state[channel_index] = dulled;
                    music
                        + dulled
                            * self.stylus_tap_level
                            * movement_gain
                            * OUTPUT_GAIN
                } else {
                    music
                };
                // Both styli feed one phono stage, so the tilt lands on their
                // sum rather than on each pickup separately.
                let mut staged = if self.config.riaa_speed_tilt {
                    self.riaa_tilt.process(channel_index, music)
                } else {
                    music
                };
                // Then the optional constant-rate RIAA mismatch and the seed
                // voicing curve, both part of the same phono stage.
                staged = self.riaa_voicing.process(channel_index, staged);
                if voicing_mix > 0.0 {
                    staged = self
                        .vinyl_voicing
                        .process(channel_index, staged, voicing_mix);
                }
                programme[channel_index] = staged;
                source_textures[channel_index] = source_texture;
            }

            let programme = self.high_frequency_acceleration_limiter.process_frame(
                programme,
                output_channel_count,
                self.output_sample_rate,
                self.config.high_frequency_acceleration_limit,
            );
            for channel_index in 0..output_channel_count {
                let output_index = frame * output_channel_count + channel_index;
                if self.needle_lifted {
                    self.output[output_index] = 0.0;
                    continue;
                }
                // Wear's crackle rides outside the gate — the groove's
                // damage keeps hissing while the gate chops the music,
                // which is what a gated worn record does.
                let wear_crackle = if worn > 0.0 && self.config.surface_enabled {
                    self.next_noise()
                        * worn
                        * 0.012
                        * (abs_rate / 1.4).clamp(0.1, 1.0)
                } else {
                    0.0
                };
                let mixed = (programme[channel_index]
                    + source_textures[channel_index])
                    * self.window_programme_gain
                    * edge_gain
                    * warp_gain
                    * self.angle_gate_gain
                    + contact_texture
                    + dust_fleck
                    + impulse_noise
                    + wear_crackle;
                // A clamp rectifies overs into broadband grit; tanh folds
                // them the way a saturating stage does. Unity slope at
                // silence keeps small signals identical either way.
                self.output[output_index] = if self.config.soft_clip {
                    mixed.tanh()
                } else {
                    mixed.clamp(-1.0, 1.0)
                } as f32;
            }

            // A lifted stylus is not in the groove, so nothing is reading the
            // programme. The platter keeps turning underneath — its angle
            // still advances, and the revolution counters with it — but the
            // read head holds where it was left. Dropping the needle back at
            // the same radius lands at the same point in the programme, not
            // wherever playback would have run on to in the meantime.
            let advanced = if self.needle_lifted {
                self.position
            } else {
                self.position + effective_rate * rate_scale
            };
            self.position = if self.locked_groove_start >= 0.0 {
                self.normalize_locked_groove_position(advanced)
            } else {
                self.clamp_source_position(advanced)
            };
            if self.grip < GRIP_OWNERSHIP {
                self.target_position = self.position;
                let physical_surface_region_active = self.surface_bed.is_some();
                if !physical_surface_region_active
                    && !self.ended
                    && self.motor_rate > 0.0
                    && self.position + PROGRAMME_END_POSITION_EPSILON_FRAMES
                        >= self.total_frames.saturating_sub(3) as f64
                {
                    self.ended = true;
                    self.motor_rate = 0.0;
                    rendered_frames = frame + 1;
                    ended_this_render = true;
                }
            }
            self.last_effective_rate = effective_rate;
            self.contact_impulse *= CONTACT_IMPULSE_DECAY;
            self.window_miss_frames = if missed_window {
                self.window_miss_frames.saturating_add(1)
            } else {
                0
            };
            let programme_fade_step = 1.0 / miss_fade_frames;
            self.window_programme_gain = if missed_window {
                (self.window_programme_gain - programme_fade_step).max(0.0)
            } else {
                (self.window_programme_gain + programme_fade_step).min(1.0)
            };
            if ended_this_render {
                break;
            }
        }
        self.mix_foley(rendered_frames, output_channel_count);
        self.apply_crossfader_trace(rendered_frames, output_channel_count);
        self.apply_output_gain(rendered_frames, output_channel_count);
        self.apply_output_seam_repair(rendered_frames, output_channel_count);
        self.maybe_request_window(rendered_frames);
        self.apply_vinyl_vfx(
            rendered_frames,
            output_channel_count,
            vfx_start_turns,
            vfx_start_position,
        );
        self.revolution_capture_copy(rendered_frames, output_channel_count);
        self.rendered_frame_counter += rendered_frames as u64;
        u32::try_from(rendered_frames).unwrap_or(u32::MAX)
    }

    /// The scene rides the finished block in record coordinates — the
    /// same call the shared bridge makes after its own render.
    fn apply_vinyl_vfx(
        &mut self,
        frame_count: usize,
        channel_count: usize,
        start_turns: f64,
        start_position: f64,
    ) {
        if frame_count == 0 {
            return;
        }
        let context = VinylVfxContext {
            sample_rate: self.output_sample_rate,
            rpm: self.native_rpm,
            start_turns,
            end_turns: self.platter_rotation_turns,
            start_position,
            end_position: self.position,
            total_frames: self.total_frames.max(1),
            pressing_seed: self.pressing_seed,
        };
        let sample_count = frame_count
            .saturating_mul(channel_count)
            .min(self.output.len());
        let output = std::mem::take(&mut self.output);
        let mut output = output;
        self.vinyl_vfx.process_interleaved(
            &mut output[..sample_count],
            channel_count,
            context,
        );
        self.output = output;
    }

    #[wasm_bindgen(js_name = renderWindowMissing)]
    pub fn render_window_missing(&mut self, frame_count: u32, output_channel_count: u32) {
        let frame_count = frame_count as usize;
        let output_channel_count = (output_channel_count as usize).clamp(1, 2);
        self.output
            .resize(frame_count.saturating_mul(output_channel_count), 0.0);
        self.scratch_gate_trace.resize(frame_count, 1.0);
        let fade_frames = (self.output_sample_rate * WINDOW_MISS_FADE_SECONDS)
            .round()
            .max(1.0);
        self.last_output_samples.resize(output_channel_count, 0.0);
        let fade_step = 1.0 / fade_frames;
        for frame in 0..frame_count {
            let fade = self.window_programme_gain;
            for channel_index in 0..output_channel_count {
                self.output[frame * output_channel_count + channel_index] =
                    (self.last_output_samples[channel_index] * fade) as f32;
            }
            self.window_programme_gain = (self.window_programme_gain - fade_step).max(0.0);
            self.window_miss_frames = self.window_miss_frames.saturating_add(1);
        }
        let gate_contact = self.active && self.hand_contact;
        let intent_rate = if gate_contact { self.target_rate } else { 0.0 };
        let rendered_rate = if gate_contact {
            self.last_effective_rate
        } else {
            0.0
        };
        self.advance_scratch_gate_trace(frame_count, gate_contact, intent_rate, rendered_rate);
        self.mix_foley(frame_count, output_channel_count);
        self.apply_crossfader_trace(frame_count, output_channel_count);
        self.apply_output_gain(frame_count, output_channel_count);
    }

    /// Render only cartridge/surface foley while keeping the programme readhead
    /// fixed. Lead-in and run-out are physical platter regions, not permission
    /// to sample the first or last seconds of programme audio underneath them.
    #[wasm_bindgen(js_name = renderSurface)]
    pub fn render_surface(&mut self, frame_count: u32, output_channel_count: u32) {
        let frame_count = frame_count as usize;
        let output_channel_count = (output_channel_count as usize).clamp(1, 2);
        self.output
            .resize(frame_count.saturating_mul(output_channel_count), 0.0);
        self.output.fill(0.0);
        self.scratch_gate_trace.resize(frame_count, 1.0);
        if frame_count == 0 {
            return;
        }

        let dt = 1.0 / self.output_sample_rate;
        let rate_scale = self.source_sample_rate / self.output_sample_rate;
        for frame in 0..frame_count {
            self.advance_deck_mechanics(0.0);
            let abs_rate = self.rate.abs();
            self.last_effective_rate = if self.config.acoustic_enabled {
                self.rate
                    + sign_nonzero(self.rate, self.motor_delivered_rate)
                        * self.advance_wow_flutter(self.rate, rate_scale, abs_rate)
            } else {
                self.rate
            };
            self.scratch_gate_trace[frame] = self.scratch_gate.process(dt, false, 0.0, 0.0) as f32;
        }
        self.mix_foley(frame_count, output_channel_count);
        self.apply_crossfader_trace(frame_count, output_channel_count);
        self.apply_output_gain(frame_count, output_channel_count);
    }

    #[wasm_bindgen(getter, js_name = outputPtr)]
    pub fn output_ptr(&self) -> *const f32 {
        self.output.as_ptr()
    }

    #[wasm_bindgen(getter, js_name = outputLen)]
    pub fn output_len(&self) -> usize {
        self.output.len()
    }

    #[wasm_bindgen(getter)]
    pub fn position(&self) -> f64 {
        self.position
    }

    #[wasm_bindgen(getter, js_name = effectiveRate)]
    pub fn effective_rate(&self) -> f64 {
        self.last_effective_rate
    }

    #[wasm_bindgen(js_name = takeWindowRequest)]
    pub fn take_window_request(&mut self) -> f64 {
        self.requested_window_position.take().unwrap_or(-1.0)
    }

    #[wasm_bindgen(js_name = takeEnded)]
    pub fn take_ended(&mut self) -> bool {
        let ended = self.ended;
        self.ended = false;
        ended
    }

    /// Decoded needle-surface asset PCM (original `assets/audio/needle-surface.opus`),
    /// provided by the host off the real-time thread.
    #[wasm_bindgen(js_name = setSurfaceAsset)]
    pub fn set_surface_asset(&mut self, channels: Array, sample_rate: f64) -> Result<(), JsValue> {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err(JsValue::from_str(
                "surface asset sampleRate must be positive",
            ));
        }
        let mut copied = Vec::with_capacity(channels.length() as usize);
        for value in channels.iter() {
            if !value.is_instance_of::<Float32Array>() {
                return Err(JsValue::from_str(
                    "surface asset channels must be Float32Array values",
                ));
            }
            let typed = Float32Array::new(&value);
            let mut samples = vec![0.0_f32; typed.length() as usize];
            typed.copy_to(&mut samples);
            copied.push(samples);
        }
        if copied.is_empty() || copied[0].is_empty() {
            return Err(JsValue::from_str(
                "surface asset requires at least one non-empty channel",
            ));
        }
        self.surface_asset = Arc::new(copied);
        self.surface_asset_rate = sample_rate;
        Ok(())
    }

    /// Mobile speaker compensation (original `resolveNeedleSurfaceGain`: ×2.25 on mobile).
    #[wasm_bindgen(js_name = setSurfaceGainMultiplier)]
    pub fn set_surface_gain_multiplier(&mut self, multiplier: f64) {
        self.surface_gain_multiplier = if multiplier.is_finite() && multiplier > 0.0 {
            multiplier
        } else {
            1.0
        };
    }

    /// Start the lead-in (region 0) or deadwax (region 1) surface bed.
    #[wasm_bindgen(js_name = startSurfaceRegion)]
    pub fn start_surface_region(&mut self, region: u8, duration_seconds: f64) {
        if !(duration_seconds > 0.0) || self.needle_lifted || !self.config.surface_enabled {
            return;
        }
        self.high_frequency_acceleration_limiter.reset();
        let (gain, filter_hz, filter_q) = if region == SURFACE_REGION_DEADWAX {
            (DEADWAX_STATIC_GAIN, 4600.0, 0.4)
        } else {
            (LEAD_IN_STATIC_GAIN, 5200.0, 0.45)
        };
        let (offset, selected_looping) = self.select_surface_sample(duration_seconds);
        let looping = if region == SURFACE_REGION_DEADWAX {
            true
        } else {
            selected_looping
        };
        let filter = BiquadLowpass::new(filter_hz, filter_q, self.output_sample_rate);
        if region == SURFACE_REGION_DEADWAX {
            let end = self.total_frames.saturating_sub(2) as f64;
            self.position = self.position.max(end);
            self.target_position = self.position;
        }
        self.ended = false;
        self.surface_bed = Some(SurfaceBed {
            region,
            position: offset * self.surface_asset_rate,
            looping,
            elapsed_frames: 0.0,
            duration_seconds,
            gain: gain * self.surface_gain_multiplier,
            filters: [filter, filter],
        });
    }

    #[wasm_bindgen(js_name = stopSurfaceRegion)]
    pub fn stop_surface_region(&mut self) {
        self.surface_bed = None;
    }

    /// One-shot needle-drop foley: stylus thump plus a settling crackle burst.
    #[wasm_bindgen(js_name = triggerNeedleDrop)]
    pub fn trigger_needle_drop(&mut self) {
        if self.needle_lifted || !self.config.surface_enabled {
            return;
        }
        self.needle_thump = Some(NeedleThump {
            elapsed_seconds: 0.0,
            phase: 0.0,
            gain: NEEDLE_DROP_THUMP_GAIN * self.surface_gain_multiplier,
        });
        let (offset, _) = self.select_surface_sample(NEEDLE_DROP_BURST_SECONDS);
        let filter = BiquadLowpass::new(
            NEEDLE_DROP_BURST_FILTER_HZ,
            NEEDLE_DROP_BURST_FILTER_Q,
            self.output_sample_rate,
        );
        self.needle_burst = Some(SurfaceBurst {
            position: offset * self.surface_asset_rate,
            elapsed_frames: 0.0,
            peak: LEAD_IN_STATIC_GAIN * 1.9 * self.surface_gain_multiplier,
            filters: [filter, filter],
        });
    }

    /// Starts a lighter stylus-release thump and a short crackle burst.
    #[wasm_bindgen(js_name = triggerNeedleLift)]
    pub fn trigger_needle_lift(&mut self) {
        if !self.config.surface_enabled {
            return;
        }
        self.needle_thump = Some(NeedleThump {
            elapsed_seconds: 0.0,
            phase: 0.0,
            gain: NEEDLE_LIFT_THUMP_GAIN * self.surface_gain_multiplier,
        });
        let (offset, _) = self.select_surface_sample(NEEDLE_DROP_BURST_SECONDS);
        let filter = BiquadLowpass::new(
            NEEDLE_DROP_BURST_FILTER_HZ,
            NEEDLE_DROP_BURST_FILTER_Q,
            self.output_sample_rate,
        );
        self.needle_burst = Some(SurfaceBurst {
            position: offset * self.surface_asset_rate,
            elapsed_frames: 0.0,
            peak: LEAD_IN_STATIC_GAIN * 0.8 * self.surface_gain_multiplier,
            filters: [filter, filter],
        });
    }
}

impl ScratchAcousticDsp {
    /// Creates the shared player for a native host.
    pub fn new_native(output_sample_rate: f64, config: AcousticConfig) -> Result<Self, String> {
        if !output_sample_rate.is_finite() || output_sample_rate <= 0.0 {
            return Err("output sample rate must be positive".to_owned());
        }
        if !config.max_rate.is_finite() || config.max_rate <= 0.0 {
            return Err("maximum rate must be positive".to_owned());
        }
        if !valid_unit_interval(config.high_frequency_acceleration_limit) {
            return Err("high-frequency acceleration limit must be between 0 and 1".to_owned());
        }
        if !valid_unit_interval(config.stylus_tracing_limit) {
            return Err("stylus tracing limit must be between 0 and 1".to_owned());
        }
        if !valid_texture_scale(config.texture_scale) {
            return Err("texture scale must be between 0 and 4".to_owned());
        }
        Ok(Self::new_internal(output_sample_rate, config))
    }

    pub fn deck_recovery_diagnostic(&self) -> Option<DeckRecoveryDiagnostic> {
        self.last_deck_recovery
    }

    /// Installs host-decoded surface PCM without routing native audio through
    /// JavaScript typed arrays. Native mono renderers use the first channel,
    /// while stereo renderers preserve both channels exactly as the WASM host
    /// does through `setSurfaceAsset`.
    pub fn set_surface_asset_native(
        &mut self,
        channels: &[&[f32]],
        sample_rate: f64,
    ) -> Result<(), String> {
        self.set_surface_asset_owned_native(
            channels.iter().map(|channel| channel.to_vec()).collect(),
            sample_rate,
        )
    }

    /// Installs already-owned native surface PCM with an O(1) audio-state
    /// swap. Hosts can allocate and copy the large asset before taking their
    /// render-state lock.
    pub fn set_surface_asset_owned_native(
        &mut self,
        channels: Vec<Vec<f32>>,
        sample_rate: f64,
    ) -> Result<(), String> {
        self.set_surface_asset_owned_native_deferred(channels, sample_rate)
            .map(drop)
    }

    /// Installs owned surface PCM and returns the previous allocation so a
    /// native host can retire it after releasing its realtime-state mutex.
    pub fn set_surface_asset_owned_native_deferred(
        &mut self,
        channels: Vec<Vec<f32>>,
        sample_rate: f64,
    ) -> Result<Arc<Vec<Vec<f32>>>, String> {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err("surface asset sample rate must be positive".to_owned());
        }
        if !(1..=2).contains(&channels.len()) {
            return Err("surface asset must have one or two channels".to_owned());
        }
        let length = channels[0].len();
        if length == 0 {
            return Err("surface asset must contain samples".to_owned());
        }
        if channels.iter().any(|channel| channel.len() != length) {
            return Err("surface asset channels must have equal lengths".to_owned());
        }

        let retired = std::mem::replace(&mut self.surface_asset, Arc::new(channels));
        self.surface_asset_rate = sample_rate;
        Ok(retired)
    }

    /// Replaces the complete source window with host-owned PCM.
    pub fn replace_window_native(
        &mut self,
        channels: &[&[f32]],
        source_sample_rate: f64,
        reset_position: Option<f64>,
    ) -> Result<(), String> {
        self.replace_window_owned_native(
            channels.iter().map(|channel| channel.to_vec()).collect(),
            source_sample_rate,
            reset_position,
        )
    }

    /// Replaces native source PCM with an O(1) ownership swap. The host must
    /// build the channel vectors before entering its real-time state lock.
    pub fn replace_window_owned_native(
        &mut self,
        channels: Vec<Vec<f32>>,
        source_sample_rate: f64,
        reset_position: Option<f64>,
    ) -> Result<(), String> {
        self.replace_window_owned_native_deferred(channels, source_sample_rate, reset_position)
            .map(drop)
    }

    /// Publishes a complete native source window and returns the previous Arc
    /// so its potentially large backing allocation can be dropped after the
    /// host releases the realtime-state mutex.
    pub fn replace_window_owned_native_deferred(
        &mut self,
        channels: Vec<Vec<f32>>,
        source_sample_rate: f64,
        reset_position: Option<f64>,
    ) -> Result<Arc<Vec<Vec<f32>>>, String> {
        if !source_sample_rate.is_finite() || source_sample_rate <= 0.0 {
            return Err("source sample rate must be positive".to_owned());
        }
        if !(1..=2).contains(&channels.len()) {
            return Err("source must have one or two channels".to_owned());
        }
        let length = channels[0].len();
        if length == 0 {
            return Err("source must contain samples".to_owned());
        }
        if channels.iter().any(|channel| channel.len() != length) {
            return Err("source channels must have equal lengths".to_owned());
        }

        let retired = std::mem::replace(&mut self.channels, Arc::new(channels));
        self.source_sample_rate = source_sample_rate;
        self.window_start = 0;
        self.window_end = length;
        self.total_frames = length;
        if let Some(position) = reset_position {
            self.reset_position(position);
        }
        Ok(retired)
    }

    /// Extends the current native source without resetting transport state.
    pub fn extend_window_native(
        &mut self,
        channels: &[&[f32]],
        source_sample_rate: f64,
    ) -> Result<(), String> {
        self.extend_window_owned_native(
            channels.iter().map(|channel| channel.to_vec()).collect(),
            source_sample_rate,
        )
    }

    /// Extends the current native source from already-owned PCM. Native hosts
    /// can copy each decode chunk before entering their render-state lock. If
    /// the initial window reserved the final programme capacity, publication
    /// is a bounded O(chunk) copy with no allocation or prefix rebuild.
    pub fn extend_window_owned_native(
        &mut self,
        mut channels: Vec<Vec<f32>>,
        source_sample_rate: f64,
    ) -> Result<(), String> {
        if !source_sample_rate.is_finite() || source_sample_rate <= 0.0 {
            return Err("source sample rate must be positive".to_owned());
        }
        if channels.len() != self.channels.len() || channels.is_empty() {
            return Err("source channel count must match the current window".to_owned());
        }
        if (source_sample_rate - self.source_sample_rate).abs() > f64::EPSILON {
            return Err("source sample rate must match the current window".to_owned());
        }
        let length = channels[0].len();
        if length == 0 {
            return Err("source must contain samples".to_owned());
        }
        if channels.iter().any(|channel| channel.len() != length) {
            return Err("source channels must have equal lengths".to_owned());
        }

        for (destination, source) in Arc::make_mut(&mut self.channels)
            .iter_mut()
            .zip(&mut channels)
        {
            destination.append(source);
        }
        self.window_end = self.window_end.saturating_add(length);
        self.total_frames = self.total_frames.saturating_add(length);
        if self.active && self.motor_rate != 0.0 {
            self.ended = false;
        }
        Ok(())
    }

    /// Replaces a range in the current native source without resetting transport.
    ///
    /// The range can extend the source. Missing frames between the old source
    /// end and the new range are silent.
    pub fn replace_window_range_native(
        &mut self,
        channels: &[&[f32]],
        start_frame: usize,
        source_sample_rate: f64,
    ) -> Result<(), String> {
        if !source_sample_rate.is_finite() || source_sample_rate <= 0.0 {
            return Err("source sample rate must be positive".to_owned());
        }
        if channels.len() != self.channels.len() || channels.is_empty() {
            return Err("source channel count must match the current window".to_owned());
        }
        if (source_sample_rate - self.source_sample_rate).abs() > f64::EPSILON {
            return Err("source sample rate must match the current window".to_owned());
        }
        let length = channels[0].len();
        if length == 0 {
            return Err("source must contain samples".to_owned());
        }
        if channels.iter().any(|channel| channel.len() != length) {
            return Err("source channels must have equal lengths".to_owned());
        }
        let end_frame = start_frame
            .checked_add(length)
            .ok_or_else(|| "source range is too large".to_owned())?;

        for (destination, source) in Arc::make_mut(&mut self.channels).iter_mut().zip(channels) {
            if destination.len() < end_frame {
                destination.resize(end_frame, 0.0);
            }
            destination[start_frame..end_frame].copy_from_slice(source);
        }
        self.window_end = self
            .window_start
            .saturating_add(self.channels.first().map_or(0, Vec::len));
        self.total_frames = self.total_frames.max(self.window_end);
        if self.active && self.motor_rate != 0.0 {
            self.ended = false;
        }
        Ok(())
    }

    /// Returns an immutable, constant-time snapshot of the native PCM window.
    /// Native hosts use it to build append/range replacements away from their
    /// realtime render mutex, then publish the result with
    /// `replace_window_owned_native` as one ownership swap.
    pub fn native_window_snapshot(&self) -> (Arc<Vec<Vec<f32>>>, f64) {
        (Arc::clone(&self.channels), self.source_sample_rate)
    }

    /// Returns the interleaved output from the most recent render call.
    pub fn rendered_samples(&self) -> &[f32] {
        &self.output
    }

    // Original `selectNeedleSurfaceSample`: pad 0.05 s, loop when the asset is shorter
    // than the requested duration + pad, random offset within the remaining span.
    // Divergence noted in the audit: uses the DSP LCG instead of Math.random().
    fn select_surface_sample(&mut self, duration_seconds: f64) -> (f64, bool) {
        if self.surface_asset.is_empty() {
            // Synthetic fallback (original: "needle surface asset unavailable;
            // synthesizing groove noise") — noise has no meaningful offset.
            return (0.0, true);
        }
        let buffer_duration = self.surface_asset[0].len() as f64 / self.surface_asset_rate;
        let requested = duration_seconds.max(0.0);
        let looping = buffer_duration <= requested + NEEDLE_SURFACE_SAMPLE_PAD_SECONDS;
        let max_offset = if looping {
            (buffer_duration - NEEDLE_SURFACE_SAMPLE_PAD_SECONDS).max(0.0)
        } else {
            (buffer_duration - requested - NEEDLE_SURFACE_SAMPLE_PAD_SECONDS).max(0.0)
        };
        let random01 = (self.next_noise() + 1.0) * 0.5;
        (
            if max_offset > 0.0 {
                random01 * max_offset
            } else {
                0.0
            },
            looping,
        )
    }

    fn surface_asset_sample(&self, channel_index: usize, position: f64, looping: bool) -> f64 {
        if self.surface_asset.is_empty() {
            return 0.0;
        }
        let channel = &self.surface_asset[channel_index.min(self.surface_asset.len() - 1)];
        let len = channel.len();
        if len == 0 {
            return 0.0;
        }
        let mut index = position.floor() as i64;
        if looping {
            index = index.rem_euclid(len as i64);
        } else if index < 0 || index >= len as i64 {
            return 0.0;
        }
        channel[index as usize] as f64
    }

    // Original bed gain automation: setValue(0.0001) → linearRamp(gain, +80 ms) →
    // hold → linearRamp(0.0001) over the final 160 ms.
    fn surface_bed_envelope(elapsed_seconds: f64, duration_seconds: f64, gain: f64) -> f64 {
        let fade_start =
            (duration_seconds - SURFACE_BED_RELEASE_SECONDS).max(SURFACE_BED_ATTACK_SECONDS);
        if elapsed_seconds < SURFACE_BED_ATTACK_SECONDS {
            SURFACE_ENV_FLOOR
                + (gain - SURFACE_ENV_FLOOR) * (elapsed_seconds / SURFACE_BED_ATTACK_SECONDS)
        } else if elapsed_seconds < fade_start {
            gain
        } else if elapsed_seconds < duration_seconds {
            let t = (elapsed_seconds - fade_start) / (duration_seconds - fade_start).max(1e-9);
            gain + (SURFACE_ENV_FLOOR - gain) * t
        } else {
            0.0
        }
    }

    // Original burst automation: 0.0001 → peak @14 ms → peak×0.32 @120 ms → 0.0001 @340 ms.
    fn burst_envelope(elapsed_seconds: f64, peak: f64) -> f64 {
        if elapsed_seconds < 0.014 {
            SURFACE_ENV_FLOOR + (peak - SURFACE_ENV_FLOOR) * (elapsed_seconds / 0.014)
        } else if elapsed_seconds < 0.12 {
            let t = (elapsed_seconds - 0.014) / (0.12 - 0.014);
            peak + (peak * 0.32 - peak) * t
        } else if elapsed_seconds < NEEDLE_DROP_BURST_SECONDS {
            let t = (elapsed_seconds - 0.12) / (NEEDLE_DROP_BURST_SECONDS - 0.12);
            (peak * 0.32) + (SURFACE_ENV_FLOOR - peak * 0.32) * t
        } else {
            0.0
        }
    }

    // Original thump: sine 130 Hz exponentialRamp→ 52 Hz @70 ms; gain 0.0001
    // exponentialRamp→ gain @6 ms exponentialRamp→ 0.0001 @95 ms; stops at 100 ms.
    fn thump_value(thump: &mut NeedleThump, dt: f64) -> Option<f64> {
        let t = thump.elapsed_seconds;
        if t >= 0.1 {
            return None;
        }
        let frequency = if t < 0.07 {
            130.0 * (52.0_f64 / 130.0).powf(t / 0.07)
        } else {
            52.0
        };
        let envelope = if t < 0.006 {
            SURFACE_ENV_FLOOR * (thump.gain / SURFACE_ENV_FLOOR).powf(t / 0.006)
        } else if t < 0.095 {
            thump.gain * (SURFACE_ENV_FLOOR / thump.gain).powf((t - 0.006) / (0.095 - 0.006))
        } else {
            SURFACE_ENV_FLOOR
        };
        let value = (thump.phase * std::f64::consts::TAU).sin() * envelope;
        thump.phase += frequency * dt;
        thump.elapsed_seconds += dt;
        Some(value)
    }

    // Mixes the surface bed, thump, and burst into the interleaved output buffer.
    // These run regardless of transport state — the original routed them as
    // independent WebAudio nodes into the same output mix.
    fn advance_scratch_gate_trace(
        &mut self,
        frame_count: usize,
        hand_contact: bool,
        intent_rate: f64,
        rendered_rate: f64,
    ) {
        let dt = 1.0 / self.output_sample_rate;
        for frame in 0..frame_count {
            self.scratch_gate_trace[frame] =
                self.scratch_gate
                    .process(dt, hand_contact, intent_rate, rendered_rate) as f32;
        }
    }

    fn apply_crossfader_trace(&mut self, frame_count: usize, output_channel_count: usize) {
        let dt = 1.0 / self.output_sample_rate;
        let alpha = if dt.is_finite() && dt > 0.0 {
            1.0 - (-dt / MOMENTARY_CROSSFADER_TRANSITION_SECONDS).exp()
        } else {
            1.0
        };
        for frame in 0..frame_count {
            self.momentary_crossfader_mix = (self.momentary_crossfader_mix
                + (self.momentary_crossfader_mix_target - self.momentary_crossfader_mix) * alpha)
                .clamp(0.0, 1.0);
            let technique_gain = if self.scratch_gate.preset() == ScratchPreset::Baby {
                self.manual_fader_gain
            } else {
                f64::from(self.scratch_gate_trace[frame])
            };
            let gain = (technique_gain * (1.0 - self.momentary_crossfader_mix)
                + self.momentary_crossfader_gain * self.momentary_crossfader_mix)
                .clamp(0.0, 1.0);
            self.scratch_gate_trace[frame] = gain as f32;
            self.audible_crossfader_gain = gain;
            for channel_index in 0..output_channel_count {
                self.output[frame * output_channel_count + channel_index] *= gain as f32;
            }
        }
    }

    fn apply_output_gain(&mut self, frame_count: usize, output_channel_count: usize) {
        if frame_count == 0
            || (self.output_gain_remaining_frames == 0 && self.output_gain_current == 1.0)
        {
            return;
        }

        for frame in 0..frame_count {
            let gain = self.output_gain_current as f32;
            if gain != 1.0 {
                for channel_index in 0..output_channel_count {
                    self.output[frame * output_channel_count + channel_index] *= gain;
                }
            }
            if self.output_gain_remaining_frames > 0 {
                self.output_gain_remaining_frames -= 1;
                if self.output_gain_remaining_frames == 0 {
                    self.output_gain_current = self.output_gain_target;
                    self.output_gain_step = 0.0;
                } else {
                    self.output_gain_current += self.output_gain_step;
                }
            }
        }
    }

    fn mix_foley(&mut self, frame_count: usize, output_channel_count: usize) {
        if !self.config.surface_enabled
            || (self.surface_bed.is_none()
                && self.needle_thump.is_none()
                && self.needle_burst.is_none())
        {
            return;
        }
        let dt = 1.0 / self.output_sample_rate;
        let asset_step = self.surface_asset_rate / self.output_sample_rate;
        for frame in 0..frame_count {
            let mut per_channel = [0.0_f64; 2];
            let synthetic_surface = self.surface_asset.is_empty();
            let mut fallback_bed = [0.0_f64; 2];
            let mut fallback_burst = [0.0_f64; 2];
            if synthetic_surface && self.surface_bed.is_some() {
                for sample in fallback_bed.iter_mut().take(output_channel_count.min(2)) {
                    *sample = self.next_noise();
                }
            }
            if synthetic_surface && self.needle_burst.is_some() {
                for sample in fallback_burst.iter_mut().take(output_channel_count.min(2)) {
                    *sample = self.next_noise();
                }
            }
            if let Some(bed) = self.surface_bed.clone() {
                let elapsed_seconds = bed.elapsed_frames * dt;
                let hold_deadwax_end =
                    bed.region == SURFACE_REGION_DEADWAX && elapsed_seconds >= bed.duration_seconds;
                if bed.region != SURFACE_REGION_DEADWAX
                    && elapsed_seconds >= bed.duration_seconds + 0.02
                {
                    self.surface_bed = None;
                } else {
                    let envelope = if hold_deadwax_end {
                        bed.gain * 0.72
                    } else {
                        Self::surface_bed_envelope(elapsed_seconds, bed.duration_seconds, bed.gain)
                    };
                    for channel_index in 0..output_channel_count.min(2) {
                        let raw = if synthetic_surface {
                            fallback_bed[channel_index]
                        } else {
                            self.surface_asset_sample(channel_index, bed.position, bed.looping)
                        };
                        if let Some(active_bed) = self.surface_bed.as_mut() {
                            per_channel[channel_index] +=
                                active_bed.filters[channel_index].process(raw) * envelope;
                        }
                    }
                    if let Some(active_bed) = self.surface_bed.as_mut() {
                        active_bed.position += asset_step;
                        active_bed.elapsed_frames += 1.0;
                    }
                }
            }
            if let Some(mut thump) = self.needle_thump.take() {
                if let Some(value) = Self::thump_value(&mut thump, dt) {
                    for channel_value in per_channel.iter_mut().take(output_channel_count.min(2)) {
                        *channel_value += value;
                    }
                    self.needle_thump = Some(thump);
                }
            }
            if let Some(burst) = self.needle_burst.clone() {
                let elapsed_seconds = burst.elapsed_frames * dt;
                if elapsed_seconds >= NEEDLE_DROP_BURST_SECONDS + 0.02 {
                    self.needle_burst = None;
                } else {
                    let envelope = Self::burst_envelope(elapsed_seconds, burst.peak);
                    for channel_index in 0..output_channel_count.min(2) {
                        let raw = if synthetic_surface {
                            fallback_burst[channel_index]
                        } else {
                            self.surface_asset_sample(channel_index, burst.position, false)
                        };
                        if let Some(active_burst) = self.needle_burst.as_mut() {
                            per_channel[channel_index] +=
                                active_burst.filters[channel_index].process(raw) * envelope;
                        }
                    }
                    if let Some(active_burst) = self.needle_burst.as_mut() {
                        active_burst.position += asset_step;
                        active_burst.elapsed_frames += 1.0;
                    }
                }
            }
            for channel_index in 0..output_channel_count.min(2) {
                let output_index = frame * output_channel_count + channel_index;
                if let Some(slot) = self.output.get_mut(output_index) {
                    *slot = (*slot as f64 + per_channel[channel_index]).clamp(-1.0, 1.0) as f32;
                }
            }
        }
    }

    fn reset_deck_to_rest_at_current_turns(&mut self) {
        let before = self.deck_state.telemetry();
        let turns = if self.platter_rotation_turns.is_finite() {
            self.platter_rotation_turns
        } else {
            self.record_deck_recovery(
                DeckRecoveryOperation::RestReset,
                DeckMechanicalError::InvalidControl {
                    field: "platterRotationTurns",
                },
                before,
                0.0,
            );
            self.platter_rotation_turns = 0.0;
            0.0
        };
        if let Err(error) = self.deck_state.reset(0.0, 0.0, turns, turns) {
            self.record_deck_recovery(DeckRecoveryOperation::RestReset, error, before, 0.0);
            self.platter_rotation_turns = 0.0;
            let _ = self.deck_state.reset(0.0, 0.0, 0.0, 0.0);
        }
    }

    fn record_deck_recovery(
        &mut self,
        operation: DeckRecoveryOperation,
        error: DeckMechanicalError,
        before: DeckMechanicalTelemetry,
        requested_hand_rate: f64,
    ) {
        self.deck_recovery_count = self.deck_recovery_count.saturating_add(1);
        self.last_deck_recovery = Some(DeckRecoveryDiagnostic {
            count: self.deck_recovery_count,
            operation,
            error,
            output_sample_rate: self.output_sample_rate,
            source_sample_rate: self.source_sample_rate,
            position: self.position,
            target_position: self.target_position,
            requested_hand_rate,
            motor_rate: self.motor_rate,
            grip: self.grip,
            platter_rate_before: before.platter_rate,
            record_rate_before: before.record_rate,
            platter_turns_before: before.platter_angle_turns,
            record_turns_before: before.record_angle_turns,
        });
    }

    fn reset_position(&mut self, position: f64) {
        self.position = self.clamp_source_position(position);
        self.target_position = self.position;
        self.rate = 0.0;
        self.rate_velocity = 0.0;
        self.target_rate = 0.0;
        self.motor_delivered_rate = 0.0;
        self.unpowered_throw_rate = 0.0;
        self.last_effective_rate = 0.0;
        self.reset_deck_to_rest_at_current_turns();
        self.frames_since_motion = 0;
        self.last_output_samples.clear();
        self.high_frequency_acceleration_limiter.reset();
        self.window_miss_frames = 0;
        self.window_programme_gain = 1.0;
    }

    fn map_rate(&self, rate: f64) -> f64 {
        if !rate.is_finite() || rate.abs() < DEADZONE_RATE {
            0.0
        } else {
            rate.clamp(-self.config.max_rate, self.config.max_rate)
        }
    }

    fn clamp_source_position(&self, position: f64) -> f64 {
        let programme_end = self.total_frames.max(self.window_end).saturating_sub(2) as f64;
        let max_position = match &self.surface_bed {
            Some(bed) if bed.region == SURFACE_REGION_DEADWAX => {
                let overrun = (bed.duration_seconds.max(0.0) * self.source_sample_rate).ceil();
                programme_end + overrun.max(0.0)
            }
            _ => programme_end,
        };
        position.clamp(0.0, max_position)
    }

    fn normalize_locked_groove_position(&self, position: f64) -> f64 {
        if self.locked_groove_start < 0.0 {
            return position;
        }
        let frames_per_turn =
            self.source_sample_rate * 60.0 / self.native_rpm.max(1.0);
        self.locked_groove_start
            + (position - self.locked_groove_start).rem_euclid(frames_per_turn)
    }

    /// Signed shortest distance between two positions in the groove domain.
    ///
    /// A locked groove is circular. Once playback crosses its seam, a small
    /// forward hand movement has a numerically low target and a numerically
    /// high current position. Subtracting those values directly makes the
    /// hand servo demand almost one full revolution backwards. Pointer
    /// samples are incremental and stay below half a turn, so the shortest
    /// circular displacement preserves their physical direction.
    fn locked_groove_position_delta(&self, target: f64, current: f64) -> f64 {
        if self.locked_groove_start < 0.0 {
            return target - current;
        }
        let frames_per_turn =
            self.source_sample_rate * 60.0 / self.native_rpm.max(1.0);
        let forward = (target - current).rem_euclid(frames_per_turn);
        if forward > frames_per_turn * 0.5 {
            forward - frames_per_turn
        } else {
            forward
        }
    }

    fn enforce_locked_groove(&mut self) {
        if self.locked_groove_start < 0.0 {
            return;
        }
        self.position = self.normalize_locked_groove_position(self.position);
        self.target_position =
            self.normalize_locked_groove_position(self.target_position);
    }

    fn next_noise(&mut self) -> f64 {
        self.noise_seed = self
            .noise_seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.noise_seed as f64 / 2_147_483_648.0 - 1.0
    }

    fn hash_noise(index: i64, salt: i32) -> f64 {
        let mut value = (index as i32) ^ salt;
        value = (value ^ ((value as u32 >> 16) as i32)).wrapping_mul(0x7feb_352d_u32 as i32);
        value = (value ^ ((value as u32 >> 15) as i32)).wrapping_mul(0x846c_a68b_u32 as i32);
        let unsigned = (value ^ ((value as u32 >> 16) as i32)) as u32;
        unsigned as f64 / 2_147_483_648.0 - 1.0
    }

    fn position_noise(&self, position: f64, spacing: f64, salt: i32) -> f64 {
        let scaled = position.max(0.0) / spacing.max(1.0);
        let index = scaled.floor() as i64;
        let t = scaled - index as f64;
        let smooth = t * t * (3.0 - 2.0 * t);
        let a = Self::hash_noise(index, salt);
        let b = Self::hash_noise(index + 1, salt);
        a + (b - a) * smooth
    }

    fn compute_position_surface_noise(&self, position: f64, abs_rate: f64) -> f64 {
        if abs_rate <= DEADZONE_RATE {
            return 0.0;
        }
        let speed_weight = (abs_rate / 2.4).clamp(0.14, 1.0);
        // The pressing seed folds into every position hash, so each copy
        // carries its own crackle — always the same crackle for that copy.
        let groove_grain =
            self.position_noise(position, 3.7, 0x0051_f15e ^ self.pressing_seed as i32);
        let groove_bed =
            self.position_noise(position, 37.0, 0x002d_4a11 ^ self.pressing_seed as i32);
        (groove_grain * 0.72 + groove_bed * 0.22) * speed_weight
    }

    fn compute_dust_fleck(&self, position: f64, abs_rate: f64) -> f64 {
        if abs_rate <= 0.03 {
            return 0.0;
        }
        let cell_frames = (self.source_sample_rate * 0.12).round().max(1.0);
        let cell = (position.max(0.0) / cell_frames).floor() as i64;
        let chance =
            (Self::hash_noise(cell, 0x006d_2b79 ^ self.pressing_seed as i32) + 1.0) * 0.5;
        if chance < 0.996 {
            return 0.0;
        }
        let center = (cell as f64
            + 0.5
            + Self::hash_noise(cell, 0x004f_1bbc ^ self.pressing_seed as i32) * 0.28)
            * cell_frames;
        let width = cell_frames * 0.028;
        let distance = (position - center).abs() / width.max(1.0);
        if distance >= 1.0 {
            return 0.0;
        }
        let envelope = (1.0 - distance).powi(2);
        let speed_weight = (abs_rate / 1.4).clamp(0.12, 1.0);
        Self::hash_noise(cell, 0x0073_c4d9 ^ self.pressing_seed as i32)
            * envelope
            * speed_weight
            * DUST_FLECK_GAIN
    }

    fn sample_channel(
        &self,
        channel_index: usize,
        position: f64,
        source_step: f64,
    ) -> Option<(f64, f64, f64)> {
        let channel = self.channels.get(channel_index)?;
        let local = position - self.window_start as f64;
        if local < 0.0 || local >= channel.len().saturating_sub(1) as f64 {
            return None;
        }
        let index = local.floor() as usize;
        let t = local - index as f64;
        let global_index = self.window_start as f64 + index as f64;
        let p0 = self.repaired_source_sample(
            channel_index,
            global_index - 1.0,
            source_step,
        )?;
        let p1 = self.repaired_source_sample(channel_index, global_index, source_step)?;
        let p2 = self.repaired_source_sample(
            channel_index,
            global_index + 1.0,
            source_step,
        )?;
        let p3 = self.repaired_source_sample(
            channel_index,
            global_index + 2.0,
            source_step,
        )?;
        let a = p2 - p0;
        let b = 2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3;
        let c = 3.0 * (p1 - p2) + p3 - p0;
        let slope = 0.5 * (a + 2.0 * b * t + 3.0 * c * t * t);
        let curvature = (p0 - 2.0 * p1 + p2) * (1.0 - t) + (p1 - 2.0 * p2 + p3) * t;
        let sample = self.repaired_source_sample(channel_index, position, source_step)?;
        Some((sample, slope, curvature))
    }

    /// Reads a locked revolution through the same 24-sample cubic Hermite
    /// bridge as fixed-context EnCodec chunks. Its anchor and duration stay
    /// exact because only samples around the circular join are replaced.
    fn repaired_source_sample(
        &self,
        channel_index: usize,
        position: f64,
        source_step: f64,
    ) -> Option<f64> {
        let raw = |source_position: f64| {
            let channel = self.channels.get(channel_index)?;
            let local = (source_position - self.window_start as f64)
                .clamp(0.0, channel.len().saturating_sub(2) as f64);
            adaptive_sample(channel, local, source_step)
        };
        if self.locked_groove_start < 0.0 {
            return raw(position);
        }

        let turn = self.source_sample_rate * 60.0 / self.native_rpm.max(1.0);
        let phase = (position - self.locked_groove_start).rem_euclid(turn);
        let each_side = SEAM_REPAIR_SAMPLES as f64 / 2.0;
        let offset = if phase >= turn - each_side {
            phase - turn
        } else if phase < each_side {
            phase
        } else {
            return raw(position);
        };

        let tail = self.locked_groove_start + turn;
        let y0 = raw(tail - each_side - 1.0)?;
        let m0 = raw(tail - each_side)? - y0;
        let y1 = raw(self.locked_groove_start + each_side)?;
        let m1 = y1 - raw(self.locked_groove_start + each_side - 1.0)?;
        let span = SEAM_REPAIR_SAMPLES as f64;
        let t = (offset + each_side + 1.0) / (span + 1.0);
        let t2 = t * t;
        let t3 = t2 * t;
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        Some(h00 * y0 + h10 * span * m0 + h01 * y1 + h11 * span * m1)
    }

    fn begin_output_seam_repair(&mut self) {
        if self.last_emitted_samples.is_empty() {
            return;
        }
        self.seam_repair_from
            .clone_from(&self.last_emitted_samples);
        self.seam_repair_remaining = SEAM_REPAIR_SAMPLES;
    }

    /// A nudge is a transport join rather than a source join. Ease from the
    /// final emitted value into the new stream with a zero-slope cubic, with
    /// no added frames or callback latency.
    fn apply_output_seam_repair(&mut self, frames: usize, channels: usize) {
        if frames == 0 || channels == 0 {
            return;
        }
        let repair_frames = frames.min(self.seam_repair_remaining);
        for frame in 0..repair_frames {
            let completed = SEAM_REPAIR_SAMPLES - self.seam_repair_remaining + frame + 1;
            let t = completed as f64 / SEAM_REPAIR_SAMPLES as f64;
            let weight = t * t * (3.0 - 2.0 * t);
            for channel in 0..channels {
                let index = frame * channels + channel;
                let from = self
                    .seam_repair_from
                    .get(channel)
                    .copied()
                    .unwrap_or(0.0);
                let next = f64::from(self.output[index]);
                self.output[index] = (from + (next - from) * weight) as f32;
            }
        }
        self.seam_repair_remaining -= repair_frames;
        self.last_emitted_samples.resize(channels, 0.0);
        let last = (frames - 1) * channels;
        for channel in 0..channels {
            self.last_emitted_samples[channel] = f64::from(self.output[last + channel]);
        }
    }

    fn advance_deck_mechanics(&mut self, hand_rate: f64) -> f64 {
        let before = self.deck_state.telemetry();
        if !self.hand_contact
            && self.motor_rate.abs() >= DEADZONE_RATE
            && before.platter_rate == self.motor_rate
            && before.record_rate == self.motor_rate
        {
            let turn_step = self.motor_rate * self.native_rpm / (60.0 * self.output_sample_rate);
            if let Err(error) = self.deck_state.reset(
                self.motor_rate,
                self.motor_rate,
                before.platter_angle_turns + turn_step,
                before.record_angle_turns + turn_step,
            ) {
                self.record_deck_recovery(
                    DeckRecoveryOperation::LockedPlaybackReset,
                    error,
                    before,
                    hand_rate,
                );
            } else {
                self.rate_velocity = 0.0;
                self.rate = self.motor_rate;
                self.motor_delivered_rate = self.motor_rate;
                self.platter_rotation_turns = before.record_angle_turns + turn_step;
                return self.motor_rate;
            }
        }
        // A lifted finger eases off over a short force collapse instead of
        // dropping its normal force in a single sample. The position servo
        // ends at release; only the fading friction remains.
        if !self.hand_contact && self.release_grip > 0.0 {
            self.release_grip *= (-(1.0 / self.output_sample_rate) / HAND_RELEASE_SECONDS).exp();
            if self.release_grip <= GRIP_CONTACT_EPSILON {
                self.release_grip = 0.0;
            }
        }
        let hand_engaged = self.hand_contact || self.release_grip > 0.0;
        let hand_target_angle_turns = if self.hand_contact && self.grip > 0.0 {
            let frames_per_turn =
                self.source_sample_rate * 60.0 / self.native_rpm.max(f64::EPSILON);
            Some(
                before.record_angle_turns
                    + self.locked_groove_position_delta(
                        self.target_position,
                        self.position,
                    ) / frames_per_turn,
            )
        } else {
            None
        };
        // A drive only acts on a free coast: hand off, motor off, platter
        // still turning. It multiplies the throw rate by e^(drive·dt) and
        // lets the motor servo chase the growing target, so the bearing's
        // friction still pushes back through the same mechanics.
        let coasting_drive = self.free_spin_drive_per_second > 0.0
            && !hand_engaged
            && self.motor_rate.abs() < DEADZONE_RATE
            && self.unpowered_throw_rate.abs() >= DEADZONE_RATE;
        if coasting_drive {
            let growth =
                (self.free_spin_drive_per_second / self.output_sample_rate).exp();
            self.unpowered_throw_rate = (self.unpowered_throw_rate * growth)
                .clamp(-self.config.max_rate, self.config.max_rate);
        }
        let motor_mode = if self.motor_rate.abs() >= DEADZONE_RATE {
            MotorMode::Servo
        } else if coasting_drive {
            MotorMode::Servo
        } else if hand_engaged || self.unpowered_throw_rate.abs() >= DEADZONE_RATE {
            MotorMode::Off
        } else {
            MotorMode::Brake
        };
        let normalized = NormalizedDeckControl {
            motor_mode,
            motor_rate: if coasting_drive {
                self.unpowered_throw_rate
            } else {
                self.motor_rate
            },
            hand_contact: hand_engaged,
            hand_target_angle_turns,
            hand_rate,
            grip: if self.hand_contact {
                self.grip
            } else {
                self.release_grip
            },
            stylus_torque_nm: 0.0,
        };
        let control = DeckMechanicalControl::from_normalized(self.deck_state.config(), normalized);
        let mut telemetry = match self
            .deck_state
            .advance(1.0 / self.output_sample_rate, control)
        {
            Ok(telemetry) => telemetry,
            Err(error) => {
                // The mechanical step is transactional. Keep its last valid state
                // for this sample, then retry the current control on the next one.
                // A rejected step must not unwind through a real-time callback.
                self.record_deck_recovery(
                    DeckRecoveryOperation::MechanicalAdvance,
                    error,
                    before,
                    hand_rate,
                );
                self.rate_velocity = 0.0;
                self.rate = before.record_rate;
                self.motor_delivered_rate = before.platter_rate;
                self.platter_rotation_turns = before.record_angle_turns;
                return before.record_rate;
            }
        };
        let servo_capture_error = 1.0e-5;
        if !self.hand_contact
            && self.motor_rate.abs() >= DEADZONE_RATE
            && (telemetry.platter_rate - self.motor_rate).abs() < servo_capture_error
            && (telemetry.record_rate - self.motor_rate).abs() < servo_capture_error
        {
            if let Err(error) = self.deck_state.reset(
                self.motor_rate,
                self.motor_rate,
                telemetry.platter_angle_turns,
                telemetry.record_angle_turns,
            ) {
                self.record_deck_recovery(
                    DeckRecoveryOperation::ServoCaptureReset,
                    error,
                    telemetry,
                    hand_rate,
                );
            } else {
                telemetry = self.deck_state.telemetry();
            }
        }
        self.rate_velocity = (telemetry.record_rate - self.rate) * self.output_sample_rate;
        self.rate = telemetry.record_rate;
        self.motor_delivered_rate = telemetry.platter_rate;
        self.platter_rotation_turns = telemetry.record_angle_turns;
        if self.unpowered_throw_rate.abs() >= DEADZONE_RATE {
            self.unpowered_throw_rate = telemetry.record_rate;
            if self.unpowered_throw_rate.abs() < DEADZONE_RATE {
                self.unpowered_throw_rate = 0.0;
            }
        }
        telemetry.record_rate
    }

    fn advance_wow_flutter(&mut self, corrected_rate: f64, rate_scale: f64, abs_rate: f64) -> f64 {
        if self.source_sample_rate <= 0.0 {
            return 0.0;
        }
        let frames_per_rev = (60.0 / self.native_rpm.max(1e-6)) * self.source_sample_rate;
        self.wow_phase += corrected_rate * rate_scale / frames_per_rev;
        self.flutter_phase +=
            self.config.flutter_hz / self.output_sample_rate * abs_rate.clamp(0.0, 1.4);
        if abs_rate <= 0.18 {
            return 0.0;
        }
        let free_depth = abs_rate.clamp(0.0, 1.2) * FREE_PLAYBACK_WOW_DEPTH;
        let hand_slip = if self.hand_contact {
            self.grip * (self.motor_delivered_rate - corrected_rate).abs().min(2.0)
        } else {
            0.0
        };
        let depth = free_depth + hand_slip * HAND_SLIP_WOW_DEPTH;
        (self.wow_phase * std::f64::consts::TAU).sin() * depth
            + (self.flutter_phase * std::f64::consts::TAU).sin() * depth * 0.22
    }

    fn drag_lowpass_alpha(&self, abs_rate: f64) -> f64 {
        let speed = (abs_rate / DRAG_LOWPASS_RATE_KNEE).clamp(0.045, 1.0);
        let mut cutoff = DRAG_LOWPASS_MAX_HZ * speed.powf(1.3);
        if abs_rate > TRACING_LOSS_START_RATE {
            cutoff *= (TRACING_LOSS_START_RATE / abs_rate).clamp(0.55, 1.0);
        }
        1.0 - (-std::f64::consts::TAU * cutoff / self.output_sample_rate).exp()
    }

    fn maybe_request_window(&mut self, frame_count: usize) {
        self.frames_since_window_request =
            self.frames_since_window_request.saturating_add(frame_count);
        let speed = self.last_effective_rate.abs().max(1.0);
        let throttle = if speed > 2.0 { 0.03 } else { 0.08 };
        if self.frames_since_window_request < (self.output_sample_rate * throttle) as usize
            || self.channels.is_empty()
        {
            return;
        }
        let start = self.window_start as f64;
        let end = self.window_end as f64;
        let window_span = (end - start).max(1.0);
        // A fixed high-rate margin can consume half of a smaller bounded
        // window and request a replacement every throttle interval. Keep both
        // the edge margin and directional look-ahead within one-sixth of the
        // active span: normal-speed values stay unchanged, while ±8–16x still
        // retain a useful reverse runway after a centered swap.
        let directional_runway = (window_span / 6.0).max(256.0);
        let margin =
            (WINDOW_REQUEST_MARGIN_SECONDS * self.source_sample_rate * (speed * 0.5).max(1.0))
                .max(256.0)
                .min(directional_runway);
        let projected_offset =
            (self.last_effective_rate * self.source_sample_rate * WINDOW_REQUEST_PROJECT_SECONDS)
                .clamp(-directional_runway, directional_runway);
        let projected = self.clamp_source_position(self.position + projected_offset);
        let request = if self.last_effective_rate < 0.0 {
            self.position.min(projected)
        } else {
            self.position.max(projected)
        };
        let approaching_active_edge = if self.last_effective_rate < 0.0 {
            self.window_start > 0 && (self.position < start + margin || request < start + margin)
        } else if self.last_effective_rate > 0.0 {
            self.window_end < self.total_frames
                && (self.position > end - margin || request > end - margin)
        } else {
            false
        };
        if approaching_active_edge {
            self.frames_since_window_request = 0;
            self.requested_window_position = Some(request);
        }
    }
}

fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

/// The 7-inch single's groove band, outer to inner, in millimetres. The
/// off-centre warble is eccentricity over groove radius, so it deepens as
/// the stylus walks in — the number a mis-punched 45 actually produces.
const SINGLE_OUTER_GROOVE_MM: f64 = 84.0;
const SINGLE_INNER_GROOVE_MM: f64 = 54.0;

/// The three things in the engine that accumulate, and the word each is
/// asked for by.
///
/// Nothing else in the deck holds history: every other effect is a filter
/// or a gain that starts from wherever the signal leaves it. These are the
/// exceptions, and they are the reason a take cannot be reproduced from its
/// gesture stream alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WearScope {
    /// The WEAR dial's map, one bucket per `WEAR_BUCKET_FRAMES` of source:
    /// it wears where along the record the stylus went.
    Groove,
    /// WORN HALO's bins, indexed by phase within one revolution: it wears
    /// where around the *turn* the stylus went. A different quantity from
    /// `Groove`, and cleared separately.
    Halo,
    /// The revolution memory ADJACENT GHOST, THREE NEEDLES and SPLIT WALLS
    /// read back from, and the filters riding on it.
    Polar,
    /// The three above.
    All,
}

impl WearScope {
    pub fn parse(scope: &str) -> Option<Self> {
        match scope {
            "groove" => Some(Self::Groove),
            "halo" => Some(Self::Halo),
            "polar" => Some(Self::Polar),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Groove => "groove",
            Self::Halo => "halo",
            Self::Polar => "polar",
            Self::All => "all",
        }
    }
}

impl ScratchAcousticDsp {
    /// Clears one accumulator, off the wasm binding.
    ///
    /// `resetWear` is the browser's door onto this; the C ABI and the tests
    /// come in here instead, because a `JsValue` cannot be built off wasm32.
    pub fn reset_wear_scope(&mut self, scope: WearScope) {
        match scope {
            WearScope::Groove => self.groove_wear.fill(0.0),
            WearScope::Halo => self.vinyl_vfx.reset_wear(),
            WearScope::Polar => self.vinyl_vfx.reset_transient_state(),
            WearScope::All => {
                self.groove_wear.fill(0.0);
                self.vinyl_vfx.reset_all();
            }
        }
    }
}

/// Source frames per wear bucket. At 48k this is about 21 ms of groove —
/// fine enough that a scratched bar wears where the scratching happened.
pub const WEAR_BUCKET_FRAMES: usize = 1024;

fn production_deck_config(output_sample_rate: f64, native_rpm: f64) -> PhysicalDeckConfig {
    let mut config = PhysicalDeckConfig::high_torque_dj_seed();
    config.nominal_rpm = native_rpm.clamp(16.0, 90.0);
    config.integration_hz = output_sample_rate.clamp(1_000.0, 768_000.0);
    // The loose hand servo the deck shipped with, restored: the tight
    // physical seed (4 ms, 25 rad/s) won `hand-spin.mjs` offline and lost
    // in the hand on the phone. Measured-better, felt-worse — so the pair
    // chosen by ear stands until something felt beats it, and the seed's
    // stays reachable through `set_hand_servo`. The dead-reckoned target
    // that shipped alongside it is kept; only the servo goes back.
    config.hand_position_stabilization_seconds = POSITION_CATCHUP_SECONDS;
    config.hand_max_position_correction_rad_s = 0.12 * config.nominal_angular_velocity_rad_s();
    config
}

fn valid_unit_interval(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

/// The friction terms scale from silent to four times the historical level.
fn valid_texture_scale(value: f64) -> bool {
    value.is_finite() && (0.0..=4.0).contains(&value)
}

/// The fixed RIAA voicing is a rate, so it only has to be positive and
/// finite; the tilt clamps it to its own `[0.1, 4.0]` window.
fn valid_riaa_voicing_rate(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn sign_nonzero(primary: f64, fallback: f64) -> f64 {
    if primary != 0.0 {
        primary.signum()
    } else if fallback != 0.0 {
        fallback.signum()
    } else {
        1.0
    }
}

fn compute_movement_gain(
    abs_rate: f64,
    acoustic_enabled: bool,
    cartridge_velocity_gain: bool,
) -> f64 {
    // A magnetic cartridge is a velocity transducer, so playing the groove at
    // rate `r` puts out `r * m(r*t)`: the level rides the rate. That is one
    // law for the whole range, exactly 1.0 at nominal speed and continuously
    // silent at rest, so it needs no stop knee — a stationary record is quiet
    // because nothing is moving past the coils, not because a gate closed.
    let stop_gain = if cartridge_velocity_gain {
        abs_rate.min(MAX_CARTRIDGE_VELOCITY_GAIN)
    } else {
        smoothstep_unit(abs_rate / STOP_GAIN_FULL_RATE)
    };
    if !acoustic_enabled {
        return stop_gain;
    }
    let normalized = abs_rate.clamp(0.0, 10.0);
    let underspeed = 0.78 + 0.22 * normalized.max(DEADZONE_RATE).powf(0.1);
    let overspeed = 1.0 + (normalized - 1.0).max(0.0) * 0.014;
    let acoustic = if normalized <= 1.0 {
        underspeed
    } else {
        overspeed
    };
    let ceiling = if cartridge_velocity_gain {
        MAX_CARTRIDGE_VELOCITY_GAIN * 1.08
    } else {
        1.08
    };
    (acoustic * stop_gain).clamp(0.0, ceiling)
}

/// Approximate the finite acceleration a cartridge can trace. Curvature is the
/// local second difference of groove displacement; traversing it faster raises
/// acceleration with velocity squared. Instead of hard clipping that demand,
/// reduce the existing tracing-filter cutoff through a smooth knee.
fn stylus_tracing_alpha(base_alpha: f64, curvature: f64, abs_rate: f64, strength: f64) -> f64 {
    let base_alpha = finite_or_zero(base_alpha).clamp(0.0, 1.0);
    let strength = finite_or_zero(strength).clamp(0.0, 1.0);
    if strength <= 0.0 || abs_rate <= 0.75 || curvature == 0.0 {
        return base_alpha;
    }
    let demand = curvature.abs() * abs_rate * abs_rate;
    let overload = smoothstep_unit(
        (demand - STYLUS_TRACING_CURVATURE_THRESHOLD)
            / (STYLUS_TRACING_CURVATURE_FULL_SCALE - STYLUS_TRACING_CURVATURE_THRESHOLD),
    );
    let velocity_presence = smoothstep_unit((abs_rate - 0.75) / (4.0 - 0.75));
    let cutoff_scale = (1.0 - strength * overload * velocity_presence).clamp(0.16, 1.0);
    1.0 - (1.0 - base_alpha).powf(cutoff_scale)
}

fn smoothstep_unit(value: f64) -> f64 {
    let value = finite_or_zero(value).clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

fn compute_contact_noise_gain(abs_rate: f64) -> f64 {
    if abs_rate <= DEADZONE_RATE {
        return 0.0;
    }
    let distance = (abs_rate - 1.0).abs();
    let realtime_dip = 1.0 - 0.94 * (-(distance * distance) / 0.18).exp();
    let slow_rub = ((0.26 - abs_rate) / 0.26).clamp(0.0, 1.0) * 0.36;
    let fast_friction = ((abs_rate - 2.2) / 5.5).clamp(0.0, 1.0) * 0.72;
    CONTACT_NOISE_GAIN * realtime_dip * (0.24 + slow_rub + fast_friction).clamp(0.08, 1.08)
}

fn compute_source_texture_gain(abs_rate: f64, rate_delta: f64) -> f64 {
    if abs_rate <= DEADZONE_RATE {
        return 0.0;
    }
    let distance = (abs_rate - 1.0).abs();
    let realtime_dip = 1.0 - 0.72 * (-(distance * distance) / 0.14).exp();
    let slow_rub = ((0.42 - abs_rate) / 0.42).clamp(0.0, 1.0);
    let speed_lift = (abs_rate / 2.2).clamp(0.0, 1.0);
    let acceleration_lift = (rate_delta / 1.6).clamp(0.0, 1.0);
    SOURCE_TEXTURE_GAIN
        * realtime_dip
        * (0.18 + slow_rub * 0.72 + speed_lift * 0.28 + acceleration_lift * 0.38)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_deck_rates(
        dsp: &mut ScratchAcousticDsp,
        platter_rate: f64,
        record_rate: f64,
        turns: f64,
    ) {
        dsp.deck_state
            .reset(platter_rate, record_rate, turns, turns)
            .unwrap();
        dsp.motor_delivered_rate = platter_rate;
        dsp.rate = record_rate;
        dsp.rate_velocity = 0.0;
        dsp.last_effective_rate = record_rate;
        dsp.platter_rotation_turns = turns;
    }



    #[test]
    fn soft_clip_folds_peaks_and_preserves_silence() {
        assert!(!AcousticConfig::default().soft_clip);
        // Pinned tanh behaviour: unity slope at silence, folded peaks.
        assert_eq!(0.0_f64.tanh(), 0.0);
        assert!((0.5_f64.tanh() - 0.462_117_157_260_009_74).abs() < 1e-15);
        assert!(3.0_f64.tanh() < 1.0 && 3.0_f64.tanh() > 0.99);
        assert_eq!((-2.0_f64.tanh()), -(2.0_f64.tanh()));
        let mut dsp = simulation_dsp();
        dsp.set_soft_clip(true);
        assert!(dsp.soft_clip());
        dsp.set_soft_clip(false);
        assert!(!dsp.soft_clip());
    }

    #[test]
    fn riaa_speed_tilt_toggles_live() {
        assert!(AcousticConfig::default().riaa_speed_tilt);
        let mut dsp = simulation_dsp();
        dsp.set_riaa_speed_tilt(false);
        assert!(!dsp.riaa_speed_tilt());
        dsp.set_riaa_speed_tilt(true);
        assert!(dsp.riaa_speed_tilt());
    }

    #[test]
    fn texture_scale_defaults_to_the_historical_level() {
        assert_eq!(AcousticConfig::default().texture_scale, 1.0);
        // The friction predicate admits silence through four times history.
        assert!(valid_texture_scale(0.0));
        assert!(valid_texture_scale(1.0));
        assert!(valid_texture_scale(4.0));
        assert!(!valid_texture_scale(-0.1));
        assert!(!valid_texture_scale(4.1));
        assert!(!valid_texture_scale(f64::NAN));
        assert!(!valid_texture_scale(f64::INFINITY));
        // The live setter wires through on the accept path.
        let mut dsp = simulation_dsp();
        assert!(dsp.set_texture_scale(0.5).is_ok());
        assert_eq!(dsp.texture_scale(), 0.5);
    }



    fn simulation_dsp() -> ScratchAcousticDsp {
        let mut config = AcousticConfig::default();
        config.acoustic_enabled = true;
        config.surface_enabled = true;
        config.stylus_tracing_limit = 0.72;
        config.high_frequency_acceleration_limit = 0.35;
        let mut dsp = ScratchAcousticDsp::new_internal(48_000.0, config);
        dsp.source_sample_rate = 48_000.0;
        dsp.channels = Arc::new(vec![vec![0.0_f32; 4_800_000]]);
        dsp.window_start = 0;
        dsp.window_end = 4_800_000;
        dsp.total_frames = 4_800_000;
        dsp
    }

    /// Runs the motor up to speed so the effect tests measure a settled
    /// platter, not the spin-up.
    fn settle_motor(dsp: &mut ScratchAcousticDsp) {
        dsp.start();
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        for _ in 0..375 {
            dsp.render(128, 1); // one second
        }
    }

    #[test]
    fn off_centre_hole_swings_the_rate_once_per_revolution() {
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(90.0).unwrap();
        dsp.set_press_defects(1.5, 0.0).unwrap();
        settle_motor(&mut dsp);

        // One revolution at 90 rpm and 48k is 32,000 source frames. Walk it
        // frame by frame and watch the instantaneous advance breathe.
        let mut minimum = f64::INFINITY;
        let mut maximum = f64::NEG_INFINITY;
        let mut total = 0.0;
        let frames = 32_000;
        for _ in 0..frames {
            let before = dsp.position;
            dsp.render(1, 1);
            let advance = dsp.position - before;
            minimum = minimum.min(advance);
            maximum = maximum.max(advance);
            total += advance;
        }
        // Eccentricity 1.5 mm over the 84→54 mm groove band is a ±1.8%-ish
        // warble at the outer edge; the mean over a whole turn cancels.
        assert!(maximum - minimum > 0.02, "spread {}", maximum - minimum);
        let mean = total / frames as f64;
        assert!((mean - 1.0).abs() < 0.01, "mean advance {mean}");
    }

    #[test]
    fn angle_gate_cuts_its_sectors_out_of_the_turn() {
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(90.0).unwrap();
        dsp.set_angle_gate(4, 1.0).unwrap();
        // A steady tone so the gate has something to chop.
        let tone: Vec<f32> = (0..4_800_000)
            .map(|i| ((i as f64 * 0.05).sin() * 0.5) as f32)
            .collect();
        dsp.channels = Arc::new(vec![tone]);
        settle_motor(&mut dsp);

        // Walk to a sector boundary first — the spin-up leaves the platter
        // at an arbitrary angle — then split one revolution into its eight
        // half-sectors. The mean level must alternate around the turn.
        for _ in 0..40_000 {
            let phase = (dsp.platter_rotation_turns * 4.0).rem_euclid(1.0);
            if phase < 0.005 {
                break;
            }
            dsp.render(1, 1);
        }
        let mut spans = Vec::new();
        for _ in 0..8 {
            let mut energy = 0.0_f64;
            for _ in 0..4_000 {
                dsp.render(1, 1);
                energy += f64::from(dsp.rendered_samples()[0]).abs();
            }
            spans.push(energy / 4_000.0);
        }
        let even: Vec<f64> = spans.iter().copied().step_by(2).collect();
        let odd: Vec<f64> = spans.iter().copied().skip(1).step_by(2).collect();
        let floor = |values: &[f64]| values.iter().cloned().fold(f64::INFINITY, f64::min);
        let ceiling = |values: &[f64]| values.iter().cloned().fold(0.0, f64::max);
        // One parity is the open sectors, the other the cut — which is
        // which depends only on where the boundary walk landed.
        let alternates = floor(&even) > ceiling(&odd) * 3.0
            || floor(&odd) > ceiling(&even) * 3.0;
        assert!(alternates, "spans did not alternate: {spans:?}");
    }

    #[test]
    fn locked_groove_holds_until_cleared() {
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(90.0).unwrap();
        settle_motor(&mut dsp);
        let frames_per_turn = 32_000.0;
        let start = dsp.position;
        dsp.set_locked_groove(start).unwrap();

        // Three revolutions of rendering never leave the ring.
        for _ in 0..750 {
            dsp.render(128, 1);
        }
        assert!(
            dsp.position >= start && dsp.position < start + frames_per_turn,
            "escaped to {} from a ring at {start}",
            dsp.position,
        );

        // Only an explicit clear lets the next revolution walk out.
        dsp.set_locked_groove(-1.0).unwrap();
        for _ in 0..300 {
            dsp.render(128, 1);
        }
        assert!(
            dsp.position >= start + frames_per_turn,
            "still inside at {}",
            dsp.position,
        );
    }

    #[test]
    fn locked_groove_keeps_exact_anchor_and_wraps_every_transport_target() {
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(90.0).unwrap();
        let frames_per_turn = 32_000.0;
        dsp.set_position(190_000.375, 0.0);
        let start = dsp.position;
        dsp.set_locked_groove(start).unwrap();

        assert_eq!(dsp.position, start, "arming moved the needle");

        dsp.set_position(start + frames_per_turn * 2.25, 0.0);
        assert!((dsp.position - (start + frames_per_turn * 0.25)).abs() < 1e-9);

        dsp.set_position(start - frames_per_turn * 0.25, 0.0);
        assert!((dsp.position - (start + frames_per_turn * 0.75)).abs() < 1e-9);

        dsp.set_motion(start + frames_per_turn * 3.5, -1.0, 0.0);
        assert!((dsp.target_position - (start + frames_per_turn * 0.5)).abs() < 1e-9);

        dsp.render(1, 1);
        assert!(dsp.position >= start && dsp.position < start + frames_per_turn);
        assert!(
            dsp.target_position >= start
                && dsp.target_position < start + frames_per_turn
        );
    }

    #[test]
    fn locked_groove_scratch_servo_uses_circular_distance_across_the_seam() {
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(90.0).unwrap();
        let frames_per_turn = 32_000.0;
        let start = 190_000.375;
        dsp.set_position(start, 0.0);
        dsp.set_locked_groove(start).unwrap();

        let tail = start + frames_per_turn - 4.0;
        let head = start + 6.0;
        assert_eq!(dsp.locked_groove_position_delta(head, tail), 10.0);
        assert_eq!(dsp.locked_groove_position_delta(tail, head), -10.0);

        dsp.set_locked_groove(-1.0).unwrap();
        assert_eq!(dsp.locked_groove_position_delta(head, tail), 10.0 - frames_per_turn);
    }

    #[test]
    fn locked_groove_uses_codec_hermite_repair_at_its_circular_seam() {
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(90.0).unwrap();
        let start = 190_000.0;
        let turn = 32_000.0;
        let channel = Arc::make_mut(&mut dsp.channels)
            .first_mut()
            .unwrap();
        channel[start as usize..(start + turn / 2.0) as usize].fill(1.0);
        channel[(start + turn / 2.0) as usize..(start + turn) as usize]
            .fill(-1.0);
        dsp.set_position(start, 0.0);
        dsp.set_locked_groove(start).unwrap();

        let tail = dsp
            .repaired_source_sample(0, start + turn - 0.001, 1.0)
            .unwrap();
        let head = dsp.repaired_source_sample(0, start, 1.0).unwrap();

        assert!((head - tail).abs() < 0.2, "repaired jump was {}", head - tail);
        assert_eq!(
            dsp.repaired_source_sample(0, start + 100.0, 1.0),
            Some(1.0)
        );
    }

    #[test]
    fn transport_jump_eases_over_the_codec_repair_span() {
        let mut dsp = simulation_dsp();
        dsp.last_emitted_samples = vec![0.8];
        dsp.begin_output_seam_repair();
        dsp.output = vec![-0.8; SEAM_REPAIR_SAMPLES];

        dsp.apply_output_seam_repair(SEAM_REPAIR_SAMPLES, 1);

        assert!(dsp.output[0] > 0.79);
        assert_eq!(dsp.output[SEAM_REPAIR_SAMPLES - 1], -0.8);
        assert_eq!(dsp.seam_repair_remaining, 0);
    }

    #[test]
    fn second_stylus_echoes_at_its_angle() {
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(90.0).unwrap();
        // 90 degrees behind is a quarter turn: 8,000 source frames.
        dsp.set_stylus_tap(90.0, 0.9).unwrap();
        let mut source = vec![0.0_f32; 4_800_000];
        for value in source.iter_mut().skip(200_000).take(64) {
            *value = 0.9;
        }
        dsp.channels = Arc::new(vec![source]);
        settle_motor(&mut dsp);
        // Drop the needle just short of the impulse so the render reaches
        // both the strike and its echo a quarter turn later.
        dsp.set_position(190_000.0, 0.0);

        let start = dsp.position;
        let mut peaks: Vec<(usize, f64)> = Vec::new();
        for frame in 0..40_000_usize {
            dsp.render(1, 1);
            let level = f64::from(dsp.rendered_samples()[0]).abs();
            if level > 0.02 {
                peaks.push((frame, level));
            }
        }
        assert!(!peaks.is_empty(), "the impulse never played");
        let first = peaks.first().unwrap().0;
        let expected_gap = 8_000.0;
        let echo = peaks
            .iter()
            .find(|(frame, _)| (*frame as f64 - first as f64) > expected_gap * 0.5)
            .map(|(frame, _)| *frame as f64 - first as f64);
        let gap = echo.expect("no echo followed the stylus");
        assert!(
            (gap - expected_gap).abs() < 400.0,
            "echo landed {gap} frames behind, wanted ~{expected_gap} (start {start})",
        );
    }

    #[test]
    fn wear_accrues_where_the_stylus_passes_and_survives_restore() {
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(90.0).unwrap();
        dsp.set_groove_wear(4.0).unwrap();
        settle_motor(&mut dsp);

        let bucket = (dsp.position as usize) / WEAR_BUCKET_FRAMES;
        for _ in 0..75 {
            dsp.render(128, 1); // a quarter second onward from here
        }
        let walked_end = (dsp.position as usize) / WEAR_BUCKET_FRAMES;
        let worn: f32 = dsp.groove_wear[bucket..=walked_end]
            .iter()
            .copied()
            .fold(0.0, f32::max);
        assert!(worn > 0.0, "the pass left no wear");
        let far = dsp.groove_wear[walked_end + 500];
        assert_eq!(far, 0.0, "unplayed groove wore anyway");

        // The biography survives a save and restore.
        let map = dsp.groove_wear_map();
        let mut fresh = simulation_dsp();
        fresh.set_groove_wear(4.0).unwrap();
        fresh.restore_groove_wear_map(&map);
        assert_eq!(fresh.groove_wear[bucket], dsp.groove_wear[bucket]);
    }

    #[test]
    fn each_accumulator_clears_on_its_own_scope() {
        use crate::VINYL_VFX_WORN_HALO;
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(90.0).unwrap();
        dsp.set_groove_wear(4.0).unwrap();
        dsp.set_vinyl_vfx(VINYL_VFX_WORN_HALO, 1.0).unwrap();
        settle_motor(&mut dsp);
        for _ in 0..75 {
            dsp.render(128, 1);
        }

        assert!(dsp.vinyl_vfx.wear_level() > 0.0, "the halo never wore");
        let groove_before = dsp.groove_wear_map();
        assert!(
            groove_before.iter().any(|value| *value > 0.0),
            "the groove never wore"
        );

        // A scope clears its own accumulator and leaves the others standing.
        dsp.reset_wear_scope(WearScope::Halo);
        assert_eq!(dsp.vinyl_vfx.wear_level(), 0.0, "halo survived its reset");
        assert_eq!(
            dsp.groove_wear_map(),
            groove_before,
            "the groove map was cleared by the halo's scope"
        );

        dsp.reset_wear_scope(WearScope::Groove);
        assert!(
            dsp.groove_wear_map().iter().all(|value| *value == 0.0),
            "groove survived its reset"
        );

        assert_eq!(WearScope::parse("nonsense"), None, "an unknown scope parsed");
        assert_eq!(WearScope::parse("halo"), Some(WearScope::Halo));
    }

    #[test]
    fn a_replay_hands_back_the_record_it_found() {
        use crate::{VINYL_VFX_ADJACENT_GHOST, VINYL_VFX_WORN_HALO};
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(90.0).unwrap();
        dsp.set_groove_wear(4.0).unwrap();
        dsp.set_press_defects(1.25, 0.5).unwrap();
        dsp.set_stylus_tap(90.0, 0.4).unwrap();
        dsp.set_angle_gate(8, 0.6).unwrap();
        dsp.set_pressing_seed(77);
        dsp.set_free_spin_drive(0.1).unwrap();
        dsp.set_vinyl_vfx(VINYL_VFX_WORN_HALO, 1.0).unwrap();
        settle_motor(&mut dsp);
        for _ in 0..75 {
            dsp.render(128, 1);
        }
        let groove_before = dsp.groove_wear_map();
        let halo_before = dsp.halo_wear_map();
        assert!(groove_before.iter().any(|value| *value > 0.0));
        assert!(halo_before.iter().any(|value| *value > 0.0));

        // The take's world: a flat record, a different scene, no wear.
        dsp.capture_replay_state();
        dsp.begin_deterministic_replay_from(0.0, 0.0, 12_345, 1.0)
            .unwrap();
        dsp.set_press_defects(0.0, 0.0).unwrap();
        dsp.set_stylus_tap(0.0, 0.0).unwrap();
        dsp.set_angle_gate(0, 0.0).unwrap();
        dsp.set_pressing_seed(0);
        dsp.set_free_spin_drive(0.0).unwrap();
        dsp.set_vinyl_vfx(VINYL_VFX_ADJACENT_GHOST, 0.5).unwrap();
        dsp.reset_wear_scope(WearScope::All);
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        for _ in 0..40 {
            dsp.render(128, 1);
        }
        assert_ne!(dsp.groove_wear_map(), groove_before);
        assert_eq!(dsp.vinyl_vfx.scene(), VINYL_VFX_ADJACENT_GHOST);

        assert!(dsp.restore_replay_state());
        assert_eq!(dsp.eccentricity_mm, 1.25, "the replay left its hole on the record");
        assert_eq!(dsp.warp_mm, 0.5);
        assert_eq!(dsp.stylus_tap_degrees, 90.0);
        assert_eq!(dsp.stylus_tap_level, 0.4);
        assert_eq!(dsp.angle_gate_sectors, 8);
        assert_eq!(dsp.angle_gate_depth, 0.6);
        assert_eq!(dsp.pressing_seed, 77);
        assert_eq!(dsp.free_spin_drive_per_second, 0.1);
        assert_eq!(dsp.vinyl_vfx.scene(), VINYL_VFX_WORN_HALO);
        assert_eq!(dsp.groove_wear_map(), groove_before, "the replay wore the live record");
        assert_eq!(dsp.halo_wear_map(), halo_before, "the replay cleared the live halo");
    }

    #[test]
    fn a_replay_from_a_rate_starts_at_speed() {
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(45.0).unwrap();
        settle_motor(&mut dsp);
        dsp.capture_replay_state();
        dsp.begin_deterministic_replay_from(1_000.0, 0.25, 9, 1.0).unwrap();
        assert_eq!(dsp.rate, 1.0);
        assert_eq!(dsp.motor_rate, 1.0);
        assert_eq!(dsp.motor_delivered_rate, 1.0);
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        let before = dsp.position;
        dsp.render(128, 1);
        // One quantum on, the platter has moved a full quantum's worth at
        // speed rather than a spin-up's worth.
        assert!(dsp.position - before > 100.0, "the platter spun up from rest");
        assert!(dsp.restore_replay_state());

        // From rest is still from rest.
        dsp.capture_replay_state();
        dsp.begin_deterministic_replay(1_000.0, 0.25, 9).unwrap();
        assert_eq!(dsp.rate, 0.0);
        assert!(dsp.restore_replay_state());
        assert!(dsp.begin_replay(0.0, 0.0, 1, 99.0).is_err());
    }

    #[test]
    fn a_take_is_cut_from_its_own_seed() {
        let mut dsp = simulation_dsp();
        dsp.seed_take_capture(0xdead_beef);
        assert_eq!(dsp.noise_seed, 0xdead_beef);
        assert_eq!(dsp.flutter_phase, f64::from(0xdead_beef_u32) / (f64::from(u32::MAX) + 1.0));
        dsp.seed_take_capture(0);
        assert_eq!(dsp.noise_seed, DEFAULT_REPLAY_NOISE_SEED);
        dsp.platter_rotation_turns = 3.25;
        dsp.wow_phase = 0.9;
        dsp.seed_take_capture(5);
        assert!((dsp.wow_phase - 0.25).abs() < 1e-12, "the wow was not brought to the platter");

        let mut halo = vec![0.0_f32; 4];
        halo[2] = 0.5;
        dsp.restore_halo_wear_map(&halo);
        let restored = dsp.halo_wear_map();
        assert_eq!(restored[2], 0.5);
        assert_eq!(restored.len(), VinylVfxProcessor::wear_bin_count());
    }

    #[test]
    fn a_revolution_is_cut_by_angle_from_the_ring_start() {
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(45.0).unwrap();
        settle_motor(&mut dsp);
        let frames_per_turn = dsp.source_sample_rate * 60.0 / 45.0;
        // A ring a little way in, the needle a third of a turn past its
        // start when CUT lands.
        let ring = 4_000.0;
        dsp.set_locked_groove(ring).unwrap();
        dsp.reset_position(ring + frames_per_turn / 3.0);
        for _ in 0..20 {
            dsp.render(128, 2);
        }
        let seed = 0x1234_5678;
        dsp.arm_revolution(ring, 200_000, seed).unwrap();
        assert!(dsp.revolution_capture_armed());

        // Render on, keeping every block, until the turn is in.
        let mut rendered = Vec::new();
        let mut counter_before = dsp.rendered_frame_counter;
        let mut blocks = 0;
        let mut begin_seen_at = None;
        while !dsp.revolution_capture_done() && blocks < 2_000 {
            dsp.render(128, 2);
            rendered.extend_from_slice(&dsp.output[..256]);
            if begin_seen_at.is_none() && dsp.revolution_capture_began() {
                begin_seen_at = Some((counter_before, dsp.revolution_capture_start_frame() as u64));
            }
            counter_before = dsp.rendered_frame_counter;
            blocks += 1;
        }
        assert!(dsp.revolution_capture_done(), "the turn never closed");
        assert!(!dsp.revolution_capture_overflowed());
        let start_frame = dsp.revolution_capture_start_frame() as u64;
        let start_position = dsp.revolution_capture_start_position();
        // Began on the ring's start, not where CUT landed: within a frame's
        // travel of the ring.
        assert!(
            (start_position - ring).abs() < 1.0,
            "began at {start_position}, ring at {ring}"
        );
        // Not on the first frame: the needle had two thirds of a turn to go.
        let first_counter = begin_seen_at.expect("began").0;
        assert!(start_frame > first_counter, "began before the ring came round");
        // Exactly one turn long at this rate, to the frame.
        let frames = dsp.revolution_capture_frames() as f64;
        let expected = frames_per_turn / dsp.rate.max(f64::EPSILON);
        assert!(
            (frames - expected).abs() <= 2.0,
            "captured {frames} frames for a turn of {expected}"
        );
        // The seed went on at the crossing.
        assert_eq!(dsp.noise_seed != seed, true, "noise has advanced past the seed");
        // And what was kept is what was rendered, frame for frame.
        let offset = ((start_frame - (dsp.rendered_frame_counter - rendered.len() as u64 / 2)) * 2) as usize;
        let kept = dsp.take_revolution_capture();
        assert_eq!(kept.len(), frames as usize * 2);
        assert_eq!(&kept[..], &rendered[offset..offset + kept.len()]);
        assert!(!dsp.revolution_capture_armed());
    }

    #[test]
    fn paging_a_window_keeps_the_wear_the_record_has_earned() {
        use crate::VINYL_VFX_ADJACENT_GHOST;
        const WINDOW_FRAMES: usize = 48_000 * 6;
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(90.0).unwrap();
        dsp.set_groove_wear(4.0).unwrap();
        dsp.set_vinyl_vfx(VINYL_VFX_ADJACENT_GHOST, 1.0).unwrap();
        settle_motor(&mut dsp);
        for _ in 0..75 {
            dsp.render(128, 1);
        }
        let groove_before = dsp.groove_wear_map();
        let polar_before = dsp.vinyl_vfx.polar_fill_ratio();
        assert!(groove_before.iter().any(|value| *value > 0.0));
        assert!(polar_before > 0.0);

        // A streamed side commits one of these every few seconds. Wear that
        // reset here could never reach the fifty passes it is scaled for.
        let total = dsp.total_frames as u32;
        dsp.prepare_window(1, WINDOW_FRAMES as u32).unwrap();
        dsp.commit_window(48_000.0, 0, total, None).unwrap();

        assert_eq!(
            dsp.groove_wear_map(),
            groove_before,
            "paging a window wiped the groove's wear"
        );
        assert_eq!(
            dsp.vinyl_vfx.polar_fill_ratio(),
            polar_before,
            "paging a window wiped the revolution memory"
        );

        // A new record is the host's call, and clears all three.
        dsp.reset_wear_scope(WearScope::All);
        assert!(dsp.groove_wear_map().iter().all(|value| *value == 0.0));
        assert_eq!(dsp.vinyl_vfx.polar_fill_ratio(), 0.0);
        assert_eq!(dsp.vinyl_vfx.wear_level(), 0.0);
    }

    #[test]
    fn the_wear_summary_reports_what_the_meters_show() {
        let mut dsp = simulation_dsp();
        dsp.set_groove_wear(1.0).unwrap();
        let summary: serde_json::Value =
            serde_json::from_str(&dsp.wear_summary()).expect("summary is not JSON");
        assert_eq!(summary["haloBins"], 2_048);
        assert_eq!(summary["polarBins"], 131_072);
        assert_eq!(summary["grooveBucketFrames"], WEAR_BUCKET_FRAMES);
        // 1 MiB of samples, 1 MiB of write tags, 8 KiB of wear bins.
        assert_eq!(summary["vfxBytes"], 2 * 1_048_576 + 2_048 * 4);
        assert_eq!(summary["polarFill"], 0.0);
    }

    #[test]
    fn pressing_seed_gives_each_copy_its_own_crackle() {
        let mut dsp = simulation_dsp();
        let a = dsp.compute_position_surface_noise(96_000.0, 1.0);
        dsp.set_pressing_seed(0x5eed_1234);
        let b = dsp.compute_position_surface_noise(96_000.0, 1.0);
        dsp.set_pressing_seed(0x5eed_1234);
        let c = dsp.compute_position_surface_noise(96_000.0, 1.0);
        assert_ne!(a, b, "the seed changed nothing");
        assert_eq!(b, c, "the same copy must always crackle the same");
    }

    #[test]
    fn rejected_deck_step_keeps_last_valid_motion_without_panicking() {
        let mut dsp = simulation_dsp();
        seed_deck_rates(&mut dsp, 0.42, 0.37, 12.0);
        dsp.hand_contact = true;
        dsp.grip = 1.0;
        dsp.motor_rate = 1.0;
        dsp.output_sample_rate = f64::NAN;

        let rate = dsp.advance_deck_mechanics(-1.0);

        assert!((rate - 0.37).abs() < 1.0e-12);
        assert!((dsp.rate - 0.37).abs() < 1.0e-12);
        assert!((dsp.motor_delivered_rate - 0.42).abs() < 1.0e-12);
        assert!((dsp.platter_rotation_turns - 12.0).abs() < 1.0e-12);
        assert_eq!(dsp.deck_recovery_count(), 1);
        let diagnostic = dsp
            .deck_recovery_diagnostic()
            .expect("a rejected step must retain its exact diagnostic");
        assert_eq!(diagnostic.count, 1);
        assert_eq!(
            diagnostic.operation,
            DeckRecoveryOperation::MechanicalAdvance
        );
        assert_eq!(diagnostic.error, DeckMechanicalError::InvalidDuration);
        assert_eq!(diagnostic.requested_hand_rate, -1.0);
        assert!((diagnostic.platter_rate_before - 0.42).abs() < 1.0e-12);
        assert!((diagnostic.record_rate_before - 0.37).abs() < 1.0e-12);
        assert_eq!(diagnostic.platter_turns_before, 12.0);
        assert_eq!(diagnostic.record_turns_before, 12.0);
    }

    #[test]
    fn nominal_playback_remains_valid_after_many_record_turns() {
        let mut dsp = simulation_dsp();
        seed_deck_rates(&mut dsp, 1.0, 1.0, 2_000.0);
        dsp.hand_contact = false;
        dsp.motor_rate = 1.0;

        for _ in 0..48_000 {
            assert_eq!(dsp.advance_deck_mechanics(0.0), 1.0);
        }

        assert_eq!(dsp.deck_recovery_count(), 0);
        assert!(dsp.platter_rotation_turns > 2_000.5);
    }

    #[test]
    fn stopped_contact_from_build_23_diagnostic_never_recovers() {
        let mut dsp = simulation_dsp();
        let turns = 61.250_890_548_885_84;
        let residual_rate = -2.246_824_675_286_976e-23;
        seed_deck_rates(&mut dsp, residual_rate, residual_rate, turns);
        dsp.position = 3_714_943.0;
        dsp.target_position = 3_714_943.0;
        dsp.hand_contact = true;
        dsp.grip = 0.988_256_371_542_977_2;
        dsp.grip_target = dsp.grip;
        dsp.motor_rate = 0.0;
        dsp.active = true;

        assert_eq!(dsp.render(12_000, 2), 12_000);

        assert_eq!(dsp.deck_recovery_count(), 0);
        assert!(dsp.deck_recovery_diagnostic().is_none());
        assert_eq!(dsp.effective_rate(), 0.0);
    }

    fn scratch_signal_dsp(preset: ScratchPreset, rate: f64) -> ScratchAcousticDsp {
        let mut dsp = ScratchAcousticDsp::new_internal(48_000.0, AcousticConfig::default());
        dsp.source_sample_rate = 48_000.0;
        dsp.channels = Arc::new(vec![vec![0.5_f32; 48_000]]);
        dsp.window_start = 0;
        dsp.window_end = 48_000;
        dsp.total_frames = 48_000;
        dsp.set_effects(false, false);
        dsp.set_scratch_preset(preset.as_str()).unwrap();
        dsp.start();
        dsp.set_position(24_000.0, 0.0);
        dsp.set_transport(true, 0.0, rate, 1.0);
        dsp.set_motion(24_000.0, rate, 0.0);
        dsp.grip = 1.0;
        seed_deck_rates(&mut dsp, rate, rate, 0.0);
        dsp
    }

    #[test]
    fn native_host_uses_the_shared_transport_and_renderer() {
        let mut dsp = ScratchAcousticDsp::new_native(48_000.0, AcousticConfig::default()).unwrap();
        let source = vec![0.25_f32; 48_000];

        dsp.replace_window_native(&[source.as_slice()], 48_000.0, Some(24_000.0))
            .unwrap();
        dsp.set_effects(false, false);
        dsp.start();
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        let mut rendered_programme = false;
        for _ in 0..32 {
            assert_eq!(dsp.render(512, 1), 512);
            rendered_programme |= dsp.rendered_samples().iter().any(|sample| *sample != 0.0);
        }

        assert_eq!(dsp.rendered_samples().len(), 512);
        assert!(rendered_programme);
        assert!(dsp.position() > 0.0);
        assert!(dsp.platter_rotation_turns() > 0.0);
    }

    #[test]
    fn prepared_window_reuses_rust_channel_allocations() {
        const WINDOW_FRAMES: usize = 48_000 * 6;
        let mut dsp = ScratchAcousticDsp::new_internal(48_000.0, AcousticConfig::default());
        dsp.prepare_window(2, WINDOW_FRAMES as u32).unwrap();
        let first_pointers = [dsp.channels[0].as_ptr(), dsp.channels[1].as_ptr()];
        Arc::make_mut(&mut dsp.channels)[0][17] = 0.25;
        Arc::make_mut(&mut dsp.channels)[1][17] = -0.25;
        dsp.commit_window(48_000.0, 500, 2_000_000, Some(144_000.0))
            .unwrap();

        assert_eq!(dsp.window_start, 500);
        assert_eq!(dsp.window_end, 500 + WINDOW_FRAMES);
        assert_eq!(dsp.total_frames, 2_000_000);
        assert_eq!(dsp.position, 144_000.0);

        dsp.prepare_window(2, WINDOW_FRAMES as u32).unwrap();
        assert_eq!(dsp.channels[0].as_ptr(), first_pointers[0]);
        assert_eq!(dsp.channels[1].as_ptr(), first_pointers[1]);
        assert_eq!(dsp.channels[0][17], 0.25);
        assert_eq!(dsp.channels[1][17], -0.25);
    }

    #[test]
    fn six_second_window_prefetch_is_bounded_without_high_rate_request_churn() {
        const WINDOW_FRAMES: usize = 48_000 * 6;
        const WINDOW_START: usize = 1_000_000;
        let half_window = WINDOW_FRAMES as f64 / 2.0;
        let runway = WINDOW_FRAMES as f64 / 6.0;

        for rate in [8.0, 10.0, 16.0, -8.0, -10.0, -16.0] {
            let mut dsp = ScratchAcousticDsp::new_internal(48_000.0, AcousticConfig::default());
            dsp.source_sample_rate = 48_000.0;
            dsp.channels = Arc::new(vec![vec![0.0; WINDOW_FRAMES]]);
            dsp.window_start = WINDOW_START;
            dsp.window_end = WINDOW_START + WINDOW_FRAMES;
            dsp.total_frames = 8_000_000;
            dsp.position = WINDOW_START as f64 + half_window;
            dsp.last_effective_rate = rate;
            dsp.frames_since_window_request = 48_000;

            dsp.maybe_request_window(0);
            assert!(
                dsp.requested_window_position.is_none(),
                "{rate}x requested immediately from the centre"
            );

            dsp.position = if rate > 0.0 {
                dsp.window_end as f64 - runway + 1.0
            } else {
                dsp.window_start as f64 + runway - 1.0
            };
            dsp.frames_since_window_request = 48_000;
            dsp.maybe_request_window(0);
            let request = dsp
                .requested_window_position
                .take()
                .unwrap_or_else(|| panic!("{rate}x did not request near its travel edge"));
            assert!(
                (request - dsp.position).abs() <= runway + f64::EPSILON,
                "{rate}x projected beyond its bounded runway"
            );

            dsp.window_start = (request - half_window).round() as usize;
            dsp.window_end = dsp.window_start + WINDOW_FRAMES;
            dsp.frames_since_window_request = 48_000;
            dsp.maybe_request_window(0);
            assert!(
                dsp.requested_window_position.is_none(),
                "{rate}x immediately churned after a centered replacement"
            );
        }
    }

    #[test]
    fn window_prefetch_ignores_trailing_and_terminal_physical_edges() {
        const WINDOW_FRAMES: usize = 48_000 * 6;
        let mut dsp = ScratchAcousticDsp::new_internal(48_000.0, AcousticConfig::default());
        dsp.source_sample_rate = 48_000.0;
        dsp.channels = Arc::new(vec![vec![0.0; WINDOW_FRAMES]]);
        dsp.total_frames = 2_000_000;

        dsp.window_start = 0;
        dsp.window_end = WINDOW_FRAMES;
        dsp.position = 1_000.0;
        dsp.last_effective_rate = 1.0;
        dsp.frames_since_window_request = 48_000;
        dsp.maybe_request_window(0);
        assert!(
            dsp.requested_window_position.is_none(),
            "forward playback churned against the start-anchored edge"
        );

        dsp.window_start = dsp.total_frames - WINDOW_FRAMES;
        dsp.window_end = dsp.total_frames;
        dsp.position = dsp.window_end as f64 - 1_000.0;
        dsp.last_effective_rate = -1.0;
        dsp.frames_since_window_request = 48_000;
        dsp.maybe_request_window(0);
        assert!(
            dsp.requested_window_position.is_none(),
            "reverse playback churned against the end-anchored edge"
        );

        dsp.position = dsp.window_end as f64 - 1_000.0;
        dsp.last_effective_rate = 1.0;
        dsp.frames_since_window_request = 48_000;
        dsp.maybe_request_window(0);
        assert!(
            dsp.requested_window_position.is_none(),
            "forward playback requested beyond the physical programme end"
        );

        dsp.window_start = 0;
        dsp.window_end = WINDOW_FRAMES;
        dsp.position = 1_000.0;
        dsp.last_effective_rate = -1.0;
        dsp.frames_since_window_request = 48_000;
        dsp.maybe_request_window(0);
        assert!(
            dsp.requested_window_position.is_none(),
            "reverse playback requested before the physical programme start"
        );
    }

    fn output_rms(dsp: &ScratchAcousticDsp) -> f64 {
        (dsp.output
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>()
            / dsp.output.len().max(1) as f64)
            .sqrt()
    }

    fn rms(samples: &[f64]) -> f64 {
        (samples.iter().map(|sample| sample * sample).sum::<f64>() / samples.len().max(1) as f64)
            .sqrt()
    }

    fn second_difference_rms(samples: &[f64]) -> f64 {
        let differences = samples
            .windows(3)
            .map(|window| window[2] - 2.0 * window[1] + window[0])
            .collect::<Vec<_>>();
        rms(&differences)
    }

    fn tone_amplitude(samples: &[f64], sample_rate: f64, frequency: f64) -> f64 {
        let (sine, cosine) = samples.iter().enumerate().fold(
            (0.0, 0.0),
            |(sine_sum, cosine_sum), (index, sample)| {
                let phase = std::f64::consts::TAU * frequency * index as f64 / sample_rate;
                (
                    sine_sum + sample * phase.sin(),
                    cosine_sum + sample * phase.cos(),
                )
            },
        );
        2.0 * sine.hypot(cosine) / samples.len().max(1) as f64
    }

    fn limit_mono(samples: &[f64], strength: f64) -> (Vec<f64>, f64) {
        let mut limiter = HighFrequencyAccelerationLimiter::default();
        let mut minimum_gain = 1.0_f64;
        let output = samples
            .iter()
            .map(|sample| {
                let output = limiter.process_frame([*sample, 0.0], 1, 48_000.0, strength)[0];
                minimum_gain = minimum_gain.min(limiter.linked_gain);
                output
            })
            .collect();
        (output, minimum_gain)
    }

    // Mirrors the worklet's exact message sequence for a canvas scratch:
    // play (motor 1×), settle, hand grab, drag backwards at −1× with motion
    // updates every 16 ms. The rendered groove must follow the hand.
    #[test]
    fn hand_drag_backwards_overrides_the_motor() {
        let mut dsp = simulation_dsp();
        dsp.start();
        dsp.set_position(2_400_000.0, 0.0);
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        for _ in 0..375 {
            dsp.render(128, 2); // 1 s: motor reaches nominal speed
        }
        assert!(
            dsp.last_effective_rate > 0.9,
            "motor should be at speed, got {}",
            dsp.last_effective_rate
        );
        let grab_position = dsp.position;
        dsp.set_transport(true, 1.0, 0.0, 1.0);
        let mut hand_position = grab_position;
        let mut min_rate = f64::MAX;
        for step in 0..60 {
            hand_position -= 768.0; // −1× for 16 ms
            dsp.set_transport(true, 1.0, -1.0, 1.0);
            dsp.set_motion(hand_position, -1.0, 0.0);
            for _ in 0..6 {
                dsp.render(128, 2);
            }
            if step >= 30 {
                min_rate = min_rate.min(dsp.last_effective_rate);
            }
        }
        assert!(
            dsp.last_effective_rate < -0.7,
            "hand should own the record after ~1 s of dragging, got rate {}",
            dsp.last_effective_rate
        );
        assert!(
            dsp.position < grab_position,
            "groove should have moved backwards: grab {} now {}",
            grab_position,
            dsp.position
        );
        let _ = min_rate;
    }

    #[test]
    fn deliberate_grab_reaches_platter_ownership_without_a_hundred_ms_lag() {
        let mut dsp = simulation_dsp();
        dsp.start();
        dsp.set_position(2_400_000.0, 0.0);
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        dsp.render(48_000, 1);
        dsp.set_transport(true, 1.0, -1.0, 1.0);
        dsp.set_motion(dsp.position - 960.0, -1.0, 0.0);
        dsp.render(960, 1);
        assert!(dsp.grip > 0.80, "20 ms grab grip was {}", dsp.grip);
    }

    #[test]
    fn motor_start_grab_and_release_are_directionally_symmetric() {
        let mut traces = Vec::new();
        for direction in [-1.0, 1.0] {
            let mut dsp = simulation_dsp();
            dsp.set_effects(false, false);
            dsp.start();
            dsp.set_position(2_400_000.0, 0.0);
            let initial_turns = dsp.platter_rotation_turns;
            dsp.set_transport(false, direction, 0.0, 0.0);
            dsp.render(9_600, 1);
            let spinup_rate = dsp.last_effective_rate;
            let spinup_turns = dsp.platter_rotation_turns - initial_turns;
            assert_eq!(spinup_rate.signum(), direction);
            assert!(
                (0.99..1.015).contains(&spinup_rate.abs()),
                "200 ms startup rate was {spinup_rate}",
            );
            assert_eq!(spinup_turns.signum(), direction);

            dsp.render(38_400, 1);
            let steady_rate = dsp.last_effective_rate;
            assert_eq!(steady_rate.signum(), direction);
            assert!(steady_rate.abs() > 0.94);

            let grab_position = dsp.position;
            dsp.set_transport(true, direction, 0.0, 1.0);
            dsp.set_motion(grab_position, 0.0, 0.0);
            dsp.render(2_400, 1);
            let grabbed_rate = dsp.last_effective_rate;
            assert!(dsp.grip > 0.98);
            assert!(
                grabbed_rate.abs() < steady_rate.abs() * 0.20,
                "50 ms full grab retained rate {grabbed_rate}",
            );

            dsp.set_transport(false, direction, 0.0, 0.0);
            dsp.render(4_800, 1);
            let caught_rate = dsp.last_effective_rate;
            assert_eq!(caught_rate.signum(), direction);
            assert!(
                caught_rate.abs() > 0.75,
                "100 ms motor recovery reached only {caught_rate}",
            );
            traces.push((
                spinup_rate,
                spinup_turns,
                steady_rate,
                grabbed_rate,
                caught_rate,
            ));
        }

        let reverse = traces[0];
        let forward = traces[1];
        for (reverse_value, forward_value) in [
            (reverse.0, forward.0),
            (reverse.1, forward.1),
            (reverse.2, forward.2),
            (reverse.3, forward.3),
            (reverse.4, forward.4),
        ] {
            assert!(
                (reverse_value + forward_value).abs() < 1e-10,
                "directional mechanics differed: reverse {reverse_value}, forward {forward_value}",
            );
        }
    }

    #[test]
    fn less_slip_catches_the_powered_platter_sooner() {
        fn rate_after_release(response: f64) -> f64 {
            let mut dsp = simulation_dsp();
            dsp.set_effects(false, false);
            dsp.set_slipmat_response(response).unwrap();
            dsp.start();
            dsp.set_position(2_400_000.0, 0.0);
            dsp.set_transport(false, 1.0, 0.0, 0.0);
            dsp.render(48_000, 1);
            dsp.set_transport(true, 1.0, -1.0, 1.0);
            dsp.set_motion(dsp.position - 9_600.0, -1.0, 0.0);
            dsp.render(9_600, 1);
            dsp.set_transport(false, 1.0, 0.0, 0.0);
            dsp.render(2_400, 1);
            dsp.last_effective_rate
        }

        // Zero slip is the tightest mat, so it catches soonest.
        let tight = rate_after_release(0.0);
        let loose = rate_after_release(1.0);
        assert!(
            tight > loose + 0.20,
            "no-slip {tight} did not clear full-slip {loose}",
        );
    }

    #[test]
    fn powered_start_uses_a_high_torque_ramp_before_servo_capture() {
        let mut dsp = simulation_dsp();
        dsp.set_effects(false, false);
        dsp.start();
        dsp.set_position(2_400_000.0, 0.0);
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        let mut rates = Vec::new();
        for _ in 0..4 {
            dsp.render(2_400, 1);
            rates.push(dsp.last_effective_rate);
        }
        assert!((0.22..0.34).contains(&rates[0]), "50 ms: {}", rates[0]);
        assert!((0.48..0.64).contains(&rates[1]), "100 ms: {}", rates[1]);
        assert!((0.75..0.91).contains(&rates[2]), "150 ms: {}", rates[2]);
        assert!((0.99..1.015).contains(&rates[3]), "200 ms: {}", rates[3]);
        let first_increment = rates[1] - rates[0];
        let second_increment = rates[2] - rates[1];
        assert!((first_increment - second_increment).abs() < 0.04);
    }

    #[test]
    fn partial_pressure_changes_takeover_acceleration() {
        fn rate_after_grab(grip: f64) -> f64 {
            let mut dsp = simulation_dsp();
            dsp.set_effects(false, false);
            dsp.start();
            dsp.set_position(2_400_000.0, 0.0);
            dsp.set_transport(false, 1.0, 0.0, 0.0);
            dsp.render(48_000, 1);
            dsp.set_transport(true, 1.0, -1.0, grip);
            dsp.set_motion(dsp.position - 2_400.0, -1.0, 0.0);
            dsp.render(2_400, 1);
            dsp.last_effective_rate
        }

        // A single fingertip (0.45) bears ~4 N and is still mid-takeover
        // at the window's edge; a full-grip hand bears 40 N and has long
        // since reversed the record and matched the stroke.
        let partial = rate_after_grab(0.45);
        let full = rate_after_grab(1.0);
        assert!(partial > -0.55, "partial pressure reached {partial}");
        assert!(full < -0.75, "full pressure reached {full}");
        assert!(partial - full > 0.3);
    }

    #[test]
    fn commanded_grip_controls_slipmat_coupling() {
        fn drag_with_grip(grip: f64) -> ScratchAcousticDsp {
            let mut dsp = simulation_dsp();
            dsp.start();
            dsp.set_position(2_400_000.0, 0.0);
            dsp.set_transport(false, 1.0, 0.0, 0.0);
            dsp.render(48_000, 1);
            let mut hand_position = dsp.position;
            for _ in 0..40 {
                hand_position -= 768.0;
                dsp.set_transport(true, 1.0, -1.0, grip);
                dsp.set_motion(hand_position, -1.0, 0.0);
                dsp.render(768, 1);
            }
            dsp
        }

        let light = drag_with_grip(0.2);
        let firm = drag_with_grip(1.0);
        assert!(
            light.last_effective_rate > 0.25,
            "light contact should let the powered platter slip forward, got {}",
            light.last_effective_rate,
        );
        assert!(
            firm.last_effective_rate < -0.65,
            "firm contact should reverse the record, got {}",
            firm.last_effective_rate,
        );
        assert!((light.grip_target - 0.2).abs() < f64::EPSILON);
        assert!((firm.grip_target - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn hand_rate_uses_only_the_stop_deadzone_and_safety_limit() {
        let dsp = simulation_dsp();
        assert_eq!(dsp.map_rate(DEADZONE_RATE * 0.5), 0.0);
        assert_eq!(dsp.map_rate(1.0), 1.0);
        assert_eq!(dsp.map_rate(-1.0), -1.0);
        assert_eq!(dsp.map_rate(0.70), 0.70);
        assert_eq!(dsp.map_rate(-0.70), -0.70);
        assert_eq!(dsp.map_rate(100.0), dsp.config.max_rate);
    }

    /// A hand turning the record at exactly nominal speed, sampled at the
    /// pointer rate, has to turn it steadily: the read advances the same
    /// amount in every block and never stands still. Before the target was
    /// dead-reckoned between samples the record stopped and lurched at sixty
    /// hertz — over a sixteen-millisecond period the smallest block advanced
    /// under a tenth of the largest.
    #[test]
    fn a_steady_hand_sampled_at_sixty_hertz_turns_the_record_steadily() {
        for updates_per_second in [30.0_f64, 60.0, 120.0] {
            let mut dsp = simulation_dsp();
            dsp.set_effects(false, false);
            dsp.start();
            let start = 2_400_000.0;
            dsp.set_position(start, 0.0);
            dsp.set_transport(true, 0.0, 1.0, 1.0);
            dsp.set_motion(start, 1.0, 0.22);
            dsp.grip = 1.0;
            seed_deck_rates(&mut dsp, 1.0, 1.0, 0.0);
            let period = (48_000.0 / updates_per_second).round() as usize;
            let block = 128;
            let mut elapsed = 0usize;
            let mut advances = Vec::new();
            let mut worst_error = 0.0_f64;
            // Two seconds: the first half settles the grab, the second is
            // measured.
            while elapsed < 96_000 {
                dsp.set_motion(start + elapsed as f64, 1.0, 0.0);
                let mut within = 0usize;
                while within < period {
                    let before = dsp.position;
                    dsp.render(block as u32, 1);
                    within += block;
                    elapsed += block;
                    if elapsed >= 48_000 {
                        advances.push(dsp.position - before);
                        worst_error = worst_error.max((dsp.position - (start + elapsed as f64)).abs());
                    }
                }
            }
            let smallest = advances.iter().cloned().fold(f64::INFINITY, f64::min);
            let largest = advances.iter().cloned().fold(0.0, f64::max);
            assert!(
                smallest > largest * 0.8,
                "{updates_per_second} Hz: the record stopped and lurched — blocks advanced between {smallest:.1} and {largest:.1} frames",
            );
            assert!(
                worst_error < 48.0,
                "{updates_per_second} Hz: the read fell {worst_error:.1} frames from the hand",
            );
        }
    }

    /// A hand that changes speed — a stroke that swings a quarter turn
    /// either way at half a hertz — is followed by the record within a
    /// millisecond at full grip. This is a probe as much as a test: the
    /// message says how far the read fell behind the hand.
    #[test]
    fn a_stroking_hand_sampled_at_sixty_hertz_is_followed_without_a_lurch() {
        // A lazy quarter-turn swing at half a hertz, and a tenth-of-a-turn
        // flick at two hertz — a scratch stroke.
        //
        // These budgets are the loose servo's measured standing, not a
        // target, and they are wider than the tight physical seed's: the
        // seed held the swing inside a quarter of a millisecond and the
        // flick inside one, where the loose pair reads 0.37 ms and 3.1 ms.
        // The seed was reverted anyway, because it lost in the hand on the
        // phone — the deck ships the pair chosen by ear, so these are the
        // numbers that can be regressed against.
        //
        // What the test actually pins is the dead-reckoned target. A frozen
        // one put the record seven milliseconds — 336 frames — behind a
        // stroking hand and lurched every sixteen, and no amount of servo
        // stiffness fixes that.
        for (turns, hertz, within_frames) in [(0.25, 0.5, 24.0), (0.1, 2.0, 168.0)] {
            stroke_is_followed(turns, hertz, within_frames);
        }
    }

    fn stroke_is_followed(turns: f64, hertz: f64, within_frames: f64) {
        let mut dsp = simulation_dsp();
        dsp.set_effects(false, false);
        dsp.start();
        let start = 2_400_000.0;
        let frames_per_turn = 48_000.0 * 60.0 / 45.0;
        let amplitude = turns * frames_per_turn;
        let omega = 2.0 * std::f64::consts::PI * hertz;
        let p = |t: f64| amplitude * (omega * t).sin();
        let r = |t: f64| amplitude * omega * (omega * t).cos() / 48_000.0;
        dsp.set_position(start, 0.0);
        dsp.set_transport(true, 0.0, r(0.0), 1.0);
        dsp.set_motion(start + p(0.0), r(0.0), 0.22);
        dsp.grip = 1.0;
        let period = 800usize;
        let block = 128usize;
        let mut elapsed = 0usize;
        let mut worst = 0.0_f64;
        let mut worst_at = 0.0;
        let mut trace = String::new();
        while elapsed < 4 * 48_000 {
            let t = elapsed as f64 / 48_000.0;
            dsp.set_motion(start + p(t), r(t), 0.0);
            let mut within = 0usize;
            while within < period {
                dsp.render(block as u32, 1);
                within += block;
                elapsed += block;
                let now = elapsed as f64 / 48_000.0;
                if now > 1.0 {
                    let signed = dsp.position - (start + p(now));
                    let error = signed.abs();
                    if error > worst { worst = error; worst_at = now; }
                    // A row every fifty milliseconds over one stroke: the
                    // signed error against the hand's rate and acceleration,
                    // so a lag can be read as viscous, inertial or constant.
                    if now > 2.30 && now <= 2.80 && elapsed % 480 == 0 {
                        trace.push_str(&format!(
                            "\n  t {now:.3}  err {:+7.2} ms  hand {:+.3}  record {:+.3}  platter {:+.3}  target {:+.3}",
                            signed / 48.0, r(now), dsp.rate, dsp.motor_delivered_rate, dsp.target_rate
                        ));
                    }
                }
            }
        }
        assert!(
            worst < within_frames,
            "{turns} turn at {hertz} Hz: the read fell {worst:.1} frames ({:.2} ms) behind the hand at {worst_at:.3} s{trace}",
            worst / 48.0,
        );
    }

    #[test]
    fn signed_unpowered_throw_coasts_while_explicit_motor_stop_brakes() {
        let mut traces = Vec::new();
        for direction in [-1.0, 1.0] {
            let mut thrown = simulation_dsp();
            thrown.set_effects(false, false);
            thrown.start();
            thrown.set_position(2_400_000.0, 0.0);
            thrown.set_transport(true, 0.0, direction, 1.0);
            thrown.set_motion(thrown.position + direction * 24_000.0, direction, 0.0);
            thrown.grip = 1.0;
            seed_deck_rates(&mut thrown, direction, direction, 0.0);
            let throw_turns = thrown.platter_rotation_turns;
            thrown.set_transport(false, 0.0, 0.0, 0.0);
            thrown.render(9_600, 1);
            let coast_rate = thrown.last_effective_rate;
            let coast_turns = thrown.platter_rotation_turns - throw_turns;
            assert_eq!(coast_rate.signum(), direction);
            assert!(
                coast_rate.abs() > 0.55,
                "{direction} bearing throw lost momentum too quickly: {coast_rate}",
            );
            assert_eq!(coast_turns.signum(), direction);

            let mut braked = simulation_dsp();
            braked.set_effects(false, false);
            braked.start();
            braked.set_position(2_400_000.0, 0.0);
            braked.set_transport(false, direction, 0.0, 0.0);
            braked.render(48_000, 1);
            braked.set_transport(false, 0.0, 0.0, 0.0);
            braked.render(19_200, 1);
            let brake_rate = braked.last_effective_rate;
            assert!(
                brake_rate.abs() < 0.05,
                "{direction} powered brake retained rate {brake_rate}",
            );
            traces.push((coast_rate, coast_turns, brake_rate));
        }

        let reverse = traces[0];
        let forward = traces[1];
        for (reverse_value, forward_value) in [
            (reverse.0, forward.0),
            (reverse.1, forward.1),
            (reverse.2, forward.2),
        ] {
            assert!(
                (reverse_value + forward_value).abs() < 1e-10,
                "directional mechanics differed: reverse {reverse_value}, forward {forward_value}",
            );
        }
    }

    #[test]
    fn wow_phase_follows_the_configured_physical_revolution() {
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(45.0).unwrap();
        let frames_per_revolution = (dsp.source_sample_rate * 60.0 / 45.0).round() as usize;
        for _ in 0..frames_per_revolution {
            dsp.advance_wow_flutter(1.0, 1.0, 1.0);
        }
        assert!((dsp.wow_phase - 1.0).abs() < 1e-9);
    }

    #[test]
    fn residual_wow_flutter_is_subtle_and_hand_slip_can_increase_it() {
        fn peak_modulation(dsp: &mut ScratchAcousticDsp, rate: f64) -> f64 {
            let mut peak = 0.0_f64;
            for _ in 0..96_000 {
                peak = peak.max(dsp.advance_wow_flutter(rate, 1.0, rate.abs()).abs());
            }
            peak
        }

        let mut free = simulation_dsp();
        free.hand_contact = false;
        free.motor_delivered_rate = 1.0;
        let free_peak = peak_modulation(&mut free, 1.0);
        assert!((0.000_20..0.000_31).contains(&free_peak), "{free_peak}");

        let mut slipping = simulation_dsp();
        slipping.hand_contact = true;
        slipping.grip = 1.0;
        slipping.motor_delivered_rate = 2.0;
        let slip_peak = peak_modulation(&mut slipping, 1.0);
        assert!(slip_peak > free_peak * 1.5, "{free_peak} -> {slip_peak}");
        assert!(slip_peak < 0.000_55, "{slip_peak}");
    }

    #[test]
    fn needle_interaction_texture_is_quiet_at_one_x_and_rises_during_drag() {
        let one_x_contact = compute_contact_noise_gain(1.0);
        let slow_contact = compute_contact_noise_gain(0.20);
        let one_x_texture = compute_source_texture_gain(1.0, 0.0);
        let slow_drag_texture = compute_source_texture_gain(0.20, 0.04);
        assert!(slow_contact > one_x_contact * 8.0);
        assert!(slow_drag_texture > one_x_texture * 4.0);
        assert!(one_x_contact < 2.0e-6);
        assert!(one_x_texture < 2.0e-5);
    }

    #[test]
    fn surface_only_render_spins_platter_without_advancing_or_leaking_programme() {
        let mut dsp = scratch_signal_dsp(ScratchPreset::Baby, 0.0);
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        let programme_position = dsp.position;
        dsp.render_surface(48_000, 1);
        assert_eq!(dsp.position, programme_position);
        assert!(dsp.last_effective_rate > 0.9);
        assert_eq!(output_rms(&dsp), 0.0);
    }

    #[test]
    fn platter_rotation_telemetry_integrates_rendered_rate() {
        let mut dsp = simulation_dsp();
        dsp.set_native_rpm(45.0).unwrap();
        dsp.set_effects(false, false);
        dsp.hand_contact = false;
        dsp.motor_rate = 1.0;
        seed_deck_rates(&mut dsp, 1.0, 1.0, 0.0);
        dsp.render_surface(48_000, 1);
        assert!(
            (dsp.platter_rotation_turns() - 0.75).abs() < 1e-6,
            "integrated {} turns",
            dsp.platter_rotation_turns(),
        );
    }

    #[test]
    fn movement_gain_reaches_silence_continuously_at_rest() {
        assert_eq!(compute_movement_gain(0.0, false, false), 0.0);
        assert!(compute_movement_gain(DEADZONE_RATE * 0.5, false, false) > 0.0);
        assert!(
            compute_movement_gain(DEADZONE_RATE, false, false)
                > compute_movement_gain(DEADZONE_RATE * 0.5, false, false)
        );
        assert_eq!(compute_movement_gain(STOP_GAIN_FULL_RATE, false, false), 1.0);
    }

    /// Steady-state gain of the tilt at DC and at Nyquist, by driving it.
    fn tilt_gain(rate: f64, alternating: bool) -> f64 {
        let mut tilt = RiaaSpeedTilt::new(48_000.0);
        tilt.set_rate(rate);
        let mut last = 0.0;
        for n in 0..400_000 {
            let input = if alternating && n % 2 == 1 { -1.0 } else { 1.0 };
            last = tilt.process(0, input) * input;
        }
        last
    }

    /// A lifted stylus reads nothing, so the programme position holds while
    /// the platter keeps turning underneath it.
    #[test]
    fn lifted_needle_holds_the_programme_position() {
        let mut dsp = scratch_signal_dsp(ScratchPreset::Baby, 0.0);
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        seed_deck_rates(&mut dsp, 1.0, 1.0, 0.0);
        dsp.render(480, 1);
        let playing_from = dsp.position;
        dsp.render(4_800, 1);
        assert!(
            dsp.position > playing_from,
            "a tracking stylus must advance the programme"
        );

        dsp.set_needle_lifted(true);
        let lifted_at = dsp.position;
        let turns_at = dsp.platter_rotation_turns();
        dsp.render(48_000, 1);
        assert_eq!(
            dsp.position, lifted_at,
            "a lifted stylus advanced the programme it is not touching"
        );
        assert!(
            dsp.platter_rotation_turns() > turns_at + 0.5,
            "the platter should keep turning under a lifted stylus"
        );

        // Dropped back down, it reads on from where it was left.
        dsp.set_needle_lifted(false);
        dsp.render(4_800, 1);
        assert!(dsp.position > lifted_at, "the stylus did not resume reading");
    }

    /// Magnitude of the analog RIAA playback curve `D` at an angular
    /// frequency: `(1 + s*T2) / ((1 + s*T1)(1 + s*T3))`.
    fn riaa_playback_magnitude(angular_frequency: f64) -> f64 {
        let term = |time_constant: f64| {
            (1.0 + (angular_frequency * time_constant).powi(2)).sqrt()
        };
        term(RIAA_T2_SECONDS) / (term(RIAA_T1_SECONDS) * term(RIAA_T3_SECONDS))
    }

    /// Magnitude of the built filter at a digital frequency, straight from the
    /// coefficients: `|b0 + b1 e^-jw| / |1 + a1 e^-jw|` per section.
    fn tilt_magnitude(tilt: &RiaaSpeedTilt, frequency_hz: f64) -> f64 {
        let omega = std::f64::consts::TAU * frequency_hz / 48_000.0;
        let cos_omega = omega.cos();
        tilt.sections.iter().fold(1.0, |gain, (b0, b1, a1)| {
            let numerator = (b0 * b0 + b1 * b1 + 2.0 * b0 * b1 * cos_omega).sqrt();
            let denominator = (1.0 + a1 * a1 + 2.0 * a1 * cos_omega).sqrt();
            gain * numerator / denominator
        })
    }

    /// The coefficients are checked against the analog prototype they claim to
    /// be, not merely against themselves. A bilinear transform maps the
    /// digital frequency `f` to the analog frequency `2*fs*tan(pi*f/fs)`, so
    /// the built filter must match `|D(w)/D(w/r)|` evaluated there exactly.
    ///
    /// Harvested from `physical/riaa.rs` before that module was removed: it
    /// used the same unprewarped bilinear form (`scale = 2*sample_rate`, and
    /// `b0/b1/a1` in this same layout), so this pins the house convention
    /// rather than leaving the tilt to vouch for itself.
    #[test]
    fn riaa_speed_tilt_matches_the_analog_curve_it_claims_to_be() {
        for rate in [0.25, 0.5, 2.0, 4.0] {
            let mut tilt = RiaaSpeedTilt::new(48_000.0);
            tilt.set_rate(rate);
            for frequency in [20.0, 50.0, 100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0] {
                // The bilinear frequency mapping, applied honestly rather than
                // assuming the analog and digital axes coincide.
                let analog = 2.0
                    * 48_000.0
                    * (std::f64::consts::PI * frequency / 48_000.0).tan();
                let expected = riaa_playback_magnitude(analog)
                    / riaa_playback_magnitude(analog / rate);
                let measured = tilt_magnitude(&tilt, frequency);
                assert!(
                    (measured - expected).abs() < 1.0e-9,
                    "rate {rate} at {frequency} Hz: built {measured}, analog curve {expected}"
                );
            }
        }
    }

    /// The whole justification for the tilt: at nominal speed the cut
    /// pre-emphasis and the preamp de-emphasis cancel exactly, so a settled
    /// filter is bit-exact and the transparent-master rule holds.
    #[test]
    fn riaa_speed_tilt_is_exactly_unity_at_nominal_speed() {
        let mut tilt = RiaaSpeedTilt::new(48_000.0);
        tilt.set_rate(1.0);
        let mut phase = 0.0_f64;
        for _ in 0..10_000 {
            phase += 0.1;
            let input = phase.sin() * 0.7;
            assert_eq!(
                tilt.process(0, input),
                input,
                "nominal speed must pass the programme through untouched"
            );
        }
    }

    /// Off speed the two curves no longer cancel. DC is untouched (both
    /// curves are flat there), while the top end scales as 1/rate — which is
    /// what pairs with the cartridge's own rate gain.
    #[test]
    fn riaa_speed_tilt_shapes_only_off_speed_content() {
        assert!((tilt_gain(1.0, false) - 1.0).abs() < 1.0e-9);
        assert!((tilt_gain(1.0, true) - 1.0).abs() < 1.0e-9);
        for rate in [0.25, 0.5, 2.0, 4.0] {
            assert!(
                (tilt_gain(rate, false) - 1.0).abs() < 1.0e-6,
                "rate {rate} shifted DC, but both curves are flat there"
            );
            assert!(
                (tilt_gain(rate, true) - 1.0 / rate).abs() < 1.0e-6,
                "rate {rate} did not scale the top end as 1/rate"
            );
        }
    }

    /// The pair is the point: velocity gain scales everything by rate and the
    /// tilt takes the top back by 1/rate, so a slow stroke reads thin and
    /// quiet while a fast one reads loud and full.
    #[test]
    fn cartridge_and_tilt_together_leave_presence_and_scale_body() {
        for rate in [0.25, 0.5, 2.0] {
            let velocity = compute_movement_gain(rate, false, true);
            let body = velocity * tilt_gain(rate, false);
            let presence = velocity * tilt_gain(rate, true);
            assert!(
                (body - rate).abs() < 1.0e-6,
                "body at rate {rate} should scale with speed"
            );
            assert!(
                (presence - 1.0).abs() < 1.0e-6,
                "presence at rate {rate} should survive the speed change"
            );
        }
    }

    /// Both opt-in voicing stages must be neutral out of the box, or the
    /// transparent-master rule is broken by default.
    #[test]
    fn default_config_leaves_voicing_transparent() {
        let config = AcousticConfig::default();
        assert_eq!(config.riaa_voicing_rate, 1.0);
        assert_eq!(config.vinyl_voicing, 0.0);
    }

    /// The fixed-rate voicing is the same filter as the speed tilt, so at a
    /// rate above nominal it holds DC flat and softens the top by `1/rate` —
    /// the only way a matched RIAA pair can colour anything.
    #[test]
    fn riaa_voicing_matches_the_fixed_riaa_curve() {
        assert!((tilt_gain(1.3, false) - 1.0).abs() < 1.0e-6);
        assert!((tilt_gain(1.3, true) - 1.0 / 1.3).abs() < 1.0e-6);
        // And the default rate is the identity, not merely close to it.
        assert!((tilt_gain(1.0, true) - 1.0).abs() < 1.0e-9);
    }

    #[test]
    fn riaa_voicing_setter_validates_and_round_trips() {
        let mut dsp = simulation_dsp();
        assert_eq!(dsp.riaa_voicing(), 1.0);
        dsp.set_riaa_voicing(1.25).unwrap();
        assert_eq!(dsp.riaa_voicing(), 1.25);
        // The rejection predicate is checked directly: building the
        // wasm-bindgen error is not possible off the wasm target.
        assert!(!valid_riaa_voicing_rate(0.0));
        assert!(!valid_riaa_voicing_rate(-1.0));
        assert!(!valid_riaa_voicing_rate(f64::NAN));
        assert!(valid_riaa_voicing_rate(0.5));
    }

    /// Steady-state gain of the seed curve, measured by driving a sine and
    /// comparing RMS in and out. Phase is irrelevant to a magnitude read.
    fn voicing_gain(filter: &mut VinylVoicingFilter, frequency_hz: f64, amount: f64) -> f64 {
        let sample_rate = 48_000.0;
        let omega = std::f64::consts::TAU * frequency_hz / sample_rate;
        let period = (sample_rate / frequency_hz).max(1.0) as usize;
        let settle = period * 40;
        let measure = period * 200;
        let mut phase = 0.0_f64;
        let mut input_energy = 0.0_f64;
        let mut output_energy = 0.0_f64;
        for n in 0..(settle + measure) {
            phase += omega;
            let input = phase.sin();
            let output = filter.process(0, input, amount);
            if n >= settle {
                input_energy += input * input;
                output_energy += output * output;
            }
        }
        (output_energy / input_energy).sqrt()
    }

    /// The seed curve must actually colour the programme: a real lift in the
    /// body and a real dulling of the top, not a shelf so small the ear
    /// cannot tell it moved. Measured on the pray4me reference, the old
    /// ±1.5 dB seed was under 1 dB of change on the track and read as no
    /// effect; this pins a clearly audible tilt.
    #[test]
    fn vinyl_voicing_seed_curve_adds_body_and_softens_the_top() {
        let mut filter = VinylVoicingFilter::new(48_000.0);
        let body = voicing_gain(&mut filter, 60.0, 1.0);
        let upper_mid = voicing_gain(&mut filter, 1_000.0, 1.0);
        let top = voicing_gain(&mut filter, 15_000.0, 1.0);
        let body_db = 20.0 * body.log10();
        let mid_db = 20.0 * upper_mid.log10();
        let top_db = 20.0 * top.log10();
        assert!(
            (2.5..=6.0).contains(&body_db),
            "voicing body {body_db} dB is not a usable lift"
        );
        assert!(
            (-10.0..=-3.5).contains(&top_db),
            "voicing top {top_db} dB is not a usable dulling"
        );
        assert!(
            mid_db.abs() < 1.0,
            "voicing moved the midrange {mid_db} dB; it should leave it alone"
        );
    }

    /// `amount == 0` must be bit-exact, not approximately transparent, for
    /// every curve.
    #[test]
    fn vinyl_voicing_is_bit_exact_at_zero() {
        for curve in 0..VINYL_VOICING_CURVES.len() {
            let mut filter = VinylVoicingFilter::new(48_000.0);
            filter.set_curve(curve);
            let mut phase = 0.0_f64;
            for _ in 0..10_000 {
                phase += 0.13;
                let input = (phase.sin() * 0.8).clamp(-1.0, 1.0);
                assert_eq!(filter.process(0, input, 0.0), input);
                assert_eq!(filter.process(1, input, 0.0), input);
            }
        }
    }

    #[test]
    fn vinyl_voicing_setter_validates_and_round_trips() {
        let mut dsp = simulation_dsp();
        assert_eq!(dsp.vinyl_voicing(), 0.0);
        dsp.set_vinyl_voicing(0.5).unwrap();
        assert_eq!(dsp.vinyl_voicing(), 0.5);
        assert!(!valid_unit_interval(1.5));
        assert!(!valid_unit_interval(-0.1));
        assert!(!valid_unit_interval(f64::NAN));
        assert!(valid_unit_interval(0.5));
    }

    /// Every curve is a usable tilt: body up, top down, the mids alone.
    #[test]
    fn every_voicing_curve_tilts_body_up_and_top_down() {
        for curve in 0..VINYL_VOICING_CURVES.len() {
            let mut filter = VinylVoicingFilter::new(48_000.0);
            filter.set_curve(curve);
            let body = 20.0 * voicing_gain(&mut filter, 60.0, 1.0).log10();
            let mid = 20.0 * voicing_gain(&mut filter, 1_000.0, 1.0).log10();
            let top = 20.0 * voicing_gain(&mut filter, 15_000.0, 1.0).log10();
            assert!(body > 0.5, "curve {curve} body {body} dB");
            assert!(top < -1.0, "curve {curve} top {top} dB");
            assert!(body - top > 4.0, "curve {curve} tilt {} dB too small", body - top);
            assert!(mid.abs() < 3.0, "curve {curve} mid {mid} dB");
        }
    }

    /// The curves are genuinely different shapes, not one shape at three
    /// amounts: tip mass costs the top and leaves the body, curve drift is
    /// the mildest, and coil load sits between them at the very top.
    #[test]
    fn voicing_curves_are_distinct_shapes() {
        let mut coil = VinylVoicingFilter::new(48_000.0);
        coil.set_curve(0);
        let mut tip = VinylVoicingFilter::new(48_000.0);
        tip.set_curve(1);
        let mut drift = VinylVoicingFilter::new(48_000.0);
        drift.set_curve(2);
        let coil_top = voicing_gain(&mut coil, 15_000.0, 1.0);
        let tip_top = voicing_gain(&mut tip, 15_000.0, 1.0);
        let drift_top = voicing_gain(&mut drift, 15_000.0, 1.0);
        assert!(
            tip_top < coil_top,
            "tip mass {tip_top} should dull more than coil load {coil_top}"
        );
        assert!(
            drift_top > tip_top,
            "curve drift {drift_top} should dull less than tip mass {tip_top}"
        );
        // Tip mass is the top-only shape: its body lift is the smallest.
        let tip_body = voicing_gain(&mut tip, 60.0, 1.0);
        let coil_body = voicing_gain(&mut coil, 60.0, 1.0);
        assert!(
            tip_body < coil_body,
            "tip mass body {tip_body} should sit under coil load {coil_body}"
        );
    }

    #[test]
    fn vinyl_voicing_curve_setter_round_trips() {
        let mut dsp = simulation_dsp();
        assert_eq!(dsp.vinyl_voicing_curve(), 0);
        for curve in 0..VINYL_VOICING_CURVES.len() as u32 {
            dsp.set_vinyl_voicing_curve(curve).unwrap();
            assert_eq!(dsp.vinyl_voicing_curve(), curve);
        }
        assert!(VINYL_VOICING_CURVES.len() >= 3);
    }

    /// The stage really is in the programme path, not just unit-tested. A
    /// settled constant programme is the low shelf's easiest target: with the
    /// seed curve off it renders the source sample unchanged, and with it on
    /// it comes out lifted but bounded.
    #[test]
    fn vinyl_voicing_reaches_the_rendered_programme() {
        fn render_settled_dc(voicing: f64) -> f32 {
            let mut config = AcousticConfig::default();
            config.vinyl_voicing = voicing;
            let mut dsp = ScratchAcousticDsp::new_internal(48_000.0, config);
            dsp.source_sample_rate = 48_000.0;
            dsp.channels = Arc::new(vec![vec![0.5_f32; 96_000]]);
            dsp.window_start = 0;
            dsp.window_end = 96_000;
            dsp.total_frames = 96_000;
            dsp.set_effects(false, false);
            dsp.start();
            dsp.set_transport(false, 1.0, 0.0, 0.0);
            dsp.render(48_000, 1);
            dsp.render(512, 1);
            *dsp.rendered_samples().last().unwrap()
        }

        let dry = render_settled_dc(0.0);
        let voiced = render_settled_dc(1.0);
        assert!(
            (dry - 0.5).abs() < 1.0e-4,
            "the default path must pass the source through, got {dry}"
        );
        assert!(
            voiced > dry + 1.0e-3,
            "the voicing stage did not reach the programme: {dry} vs {voiced}"
        );
        assert!(
            voiced < 0.5 * 1.7,
            "the voicing lifted a settled programme past its seed bound: {voiced}"
        );
    }

    /// A cartridge is a velocity transducer: output rides the rate, exactly
    /// 1.0 at nominal speed and continuously silent at rest.
    #[test]
    fn cartridge_velocity_gain_is_linear_in_rate() {
        assert_eq!(compute_movement_gain(0.0, false, true), 0.0);
        assert_eq!(compute_movement_gain(1.0, false, true), 1.0);
        for rate in [0.02, 0.1, 0.25, 0.5, 1.0, 2.0, 3.0] {
            assert!(
                (compute_movement_gain(rate, false, true) - rate).abs() < 1.0e-12,
                "rate {rate} did not read back as its own gain"
            );
        }
        // Bounded, so a runaway rate cannot blow up the programme.
        assert_eq!(
            compute_movement_gain(50.0, false, true),
            MAX_CARTRIDGE_VELOCITY_GAIN
        );
    }

    /// The velocity law needs no stop knee: it is already continuous to
    /// silence, so nothing has to gate the programme off at rest.
    #[test]
    fn cartridge_velocity_gain_needs_no_stop_knee() {
        let mut previous = 0.0;
        for step in 0..64 {
            let rate = f64::from(step) / 64.0 * STOP_GAIN_FULL_RATE * 2.0;
            let gain = compute_movement_gain(rate, false, true);
            assert!(gain >= previous, "gain went backwards at rate {rate}");
            assert!(gain - previous < 0.01, "gain stepped at rate {rate}");
            previous = gain;
        }
    }

    #[test]
    fn movement_gain_stays_bounded() {
        for rate in [0.01, 0.1, 1.0, 3.0, 10.0] {
            let gain = compute_movement_gain(rate, true, false);
            assert!((0.0..=1.08).contains(&gain));
            let velocity = compute_movement_gain(rate, true, true);
            assert!((0.0..=MAX_CARTRIDGE_VELOCITY_GAIN * 1.08).contains(&velocity));
        }
        assert_eq!(compute_movement_gain(1.0, true, false), 1.0);
        assert_eq!(compute_movement_gain(1.0, true, true), 1.0);
    }

    #[test]
    fn default_moving_playback_has_no_unmeasured_speed_gain() {
        // With the cartridge law off, the dry path is flat above the knee:
        // no invented speed colour, which is what this has always pinned.
        for rate in [0.1, 0.5, 1.0, 2.0, 8.0] {
            assert_eq!(compute_movement_gain(rate, false, false), 1.0);
        }
    }

    #[test]
    fn default_rapid_reversal_has_no_stop_deadzone_click() {
        let mut dsp = scratch_signal_dsp(ScratchPreset::Baby, 1.0);
        dsp.render(512, 1);
        dsp.set_motion(dsp.position, -1.0, 0.0);

        let mut prior = dsp.rendered_samples().last().copied().unwrap_or_default();
        let mut maximum_step = 0.0_f32;
        let mut crossed_zero = false;
        for _ in 0..4_800 {
            dsp.render(1, 1);
            let sample = dsp.rendered_samples()[0];
            maximum_step = maximum_step.max((sample - prior).abs());
            prior = sample;
            crossed_zero |= dsp.effective_rate() < 0.0;
        }

        assert!(crossed_zero, "the test motion did not reverse the record");
        assert!(
            maximum_step < 0.01,
            "the stop deadzone produced a {maximum_step} full-scale sample step"
        );
    }

    #[test]
    fn stylus_tracing_limit_preserves_the_existing_curvature_velocity_model() {
        let base_alpha = 0.90;
        assert_eq!(stylus_tracing_alpha(base_alpha, 3.0, 0.5, 1.0), base_alpha,);
        assert_eq!(stylus_tracing_alpha(base_alpha, 3.0, 4.0, 0.0), base_alpha,);
        let moderate = stylus_tracing_alpha(base_alpha, 1.0, 2.0, 0.72);
        let demanding = stylus_tracing_alpha(base_alpha, 3.0, 4.0, 0.72);
        assert!((0.0..base_alpha).contains(&moderate));
        assert!((0.0..moderate).contains(&demanding));
    }

    #[test]
    fn limiter_defaults_serde_names_and_strengths_are_distinct_and_validated() {
        let defaults = AcousticConfig::default();
        assert!(!defaults.acoustic_enabled);
        assert!(!defaults.surface_enabled);
        assert_eq!(defaults.stylus_tracing_limit, 0.0);
        assert_eq!(defaults.high_frequency_acceleration_limit, 0.0);

        let decoded: AcousticConfig = serde_json::from_value(serde_json::json!({
            "stylusTracingLimit": 0.44,
            "highFrequencyAccelerationLimit": 0.66
        }))
        .unwrap();
        assert_eq!(decoded.stylus_tracing_limit, 0.44);
        assert_eq!(decoded.high_frequency_acceleration_limit, 0.66);

        let mut dsp = simulation_dsp();
        dsp.set_stylus_tracing_limit(0.25).unwrap();
        assert_eq!(dsp.stylus_tracing_limit(), 0.25);
        dsp.set_high_frequency_acceleration_limit(0.0).unwrap();
        assert_eq!(dsp.high_frequency_acceleration_limit(), 0.0);
        dsp.set_high_frequency_acceleration_limit(1.0).unwrap();
        assert_eq!(dsp.high_frequency_acceleration_limit(), 1.0);
        assert!(!valid_unit_interval(-0.01));
        assert!(!valid_unit_interval(f64::NAN));
        assert!(!valid_unit_interval(1.1));
    }

    #[test]
    fn default_nominal_playback_preserves_aligned_pcm_samples_exactly() {
        const START: usize = 64;
        const FRAMES: usize = 256;
        let left = (0..1_024)
            .map(|frame| ((frame as i32 % 97) - 48) as f32 / 64.0)
            .collect::<Vec<_>>();
        let right = (0..1_024)
            .map(|frame| ((frame as i32 % 83) - 41) as f32 / 64.0)
            .collect::<Vec<_>>();
        let mut dsp = ScratchAcousticDsp::new_internal(48_000.0, AcousticConfig::default());
        dsp.replace_window_owned_native(
            vec![left.clone(), right.clone()],
            48_000.0,
            Some(START as f64),
        )
        .unwrap();
        dsp.active = true;
        dsp.hand_contact = false;
        dsp.grip = 0.0;
        dsp.grip_target = 0.0;
        dsp.motor_rate = 1.0;
        seed_deck_rates(&mut dsp, 1.0, 1.0, 0.0);

        assert_eq!(dsp.render(FRAMES as u32, 2), FRAMES as u32);

        let expected = (START..START + FRAMES)
            .flat_map(|frame| [left[frame], right[frame]])
            .collect::<Vec<_>>();
        assert_eq!(dsp.rendered_samples(), expected);
        assert_eq!(dsp.position(), (START + FRAMES) as f64);
        assert_eq!(dsp.effective_rate(), 1.0);
    }

    #[test]
    fn high_frequency_acceleration_limit_zero_is_an_exact_bypass() {
        let samples = (0..8_192)
            .map(|index| {
                let time = index as f64 / 48_000.0;
                0.31 * (std::f64::consts::TAU * 437.0 * time).sin()
                    + 0.47 * (std::f64::consts::TAU * 11_300.0 * time).sin()
            })
            .collect::<Vec<_>>();
        let (output, minimum_gain) = limit_mono(&samples, 0.0);
        assert_eq!(output, samples);
        assert_eq!(minimum_gain, 1.0);
    }

    #[test]
    fn high_frequency_acceleration_limit_preserves_low_frequency_programme() {
        let samples = (0..12_000)
            .map(|index| 0.65 * (std::f64::consts::TAU * 440.0 * index as f64 / 48_000.0).sin())
            .collect::<Vec<_>>();
        let (output, minimum_gain) = limit_mono(&samples, 1.0);
        let error = output
            .iter()
            .zip(samples.iter())
            .map(|(output, input)| (output - input).abs())
            .fold(0.0_f64, f64::max);
        assert!(error < 1e-10, "low-frequency peak error was {error}");
        assert_eq!(minimum_gain, 1.0);
    }

    #[test]
    fn high_frequency_acceleration_limit_keeps_benign_brightness() {
        let samples = (0..12_000)
            .map(|index| 0.12 * (std::f64::consts::TAU * 7_000.0 * index as f64 / 48_000.0).sin())
            .collect::<Vec<_>>();
        let (output, minimum_gain) = limit_mono(&samples, 1.0);
        let input_rms = rms(&samples[1_024..]);
        let output_rms = rms(&output[1_024..]);
        assert!(
            output_rms > input_rms * 0.96,
            "benign HF changed from {input_rms} to {output_rms}"
        );
        assert!(minimum_gain > 0.94, "benign HF gain reached {minimum_gain}");
    }

    #[test]
    fn high_frequency_acceleration_limit_reduces_harsh_burst_without_full_band_collapse() {
        let samples = (0..9_600)
            .map(|index| {
                let time = index as f64 / 48_000.0;
                let low = 0.34 * (std::f64::consts::TAU * 440.0 * time).sin();
                let high = if (2_400..7_200).contains(&index) {
                    0.50 * (std::f64::consts::TAU * 11_000.0 * time).sin()
                } else {
                    0.0
                };
                low + high
            })
            .collect::<Vec<_>>();
        let (default_output, default_minimum_gain) = limit_mono(&samples, 0.35);
        let (output, minimum_gain) = limit_mono(&samples, 1.0);
        let analysis = 3_000..6_600;
        let input_burst = &samples[analysis.clone()];
        let default_burst = &default_output[analysis.clone()];
        let output_burst = &output[analysis];
        let input_acceleration = second_difference_rms(input_burst);
        let default_acceleration = second_difference_rms(default_burst);
        let output_acceleration = second_difference_rms(output_burst);
        assert!(
            default_acceleration < input_acceleration * 0.90,
            "default burst acceleration {input_acceleration} -> {default_acceleration}"
        );
        assert!(
            default_minimum_gain < 0.88,
            "default harsh-burst gain only reached {default_minimum_gain}"
        );
        assert!(
            output_acceleration < input_acceleration * 0.72,
            "burst acceleration {input_acceleration} -> {output_acceleration}"
        );
        assert!(
            rms(output_burst) > rms(input_burst) * 0.50,
            "programme RMS collapsed from {} to {}",
            rms(input_burst),
            rms(output_burst),
        );
        let input_low = tone_amplitude(input_burst, 48_000.0, 440.0);
        let output_low = tone_amplitude(output_burst, 48_000.0, 440.0);
        assert!(
            output_low > input_low * 0.97,
            "440 Hz component collapsed from {input_low} to {output_low}"
        );
        assert!(
            minimum_gain < 0.65,
            "harsh burst only reached {minimum_gain}"
        );
    }

    #[test]
    fn high_frequency_acceleration_limit_is_bounded_and_stereo_linked() {
        let mut stereo = HighFrequencyAccelerationLimiter::default();
        let mut right_only = HighFrequencyAccelerationLimiter::default();
        let mut stereo_right = Vec::new();
        let mut solo_right = Vec::new();
        let mut minimum_gain = 1.0_f64;
        for index in 0..7_200 {
            let time = index as f64 / 48_000.0;
            let left = if index % 2 == 0 { 0.72 } else { -0.72 };
            let right = 0.12 * (std::f64::consts::TAU * 7_000.0 * time).sin();
            let linked = stereo.process_frame([left, right], 2, 48_000.0, 1.0);
            let solo = right_only.process_frame([right, 0.0], 1, 48_000.0, 1.0);
            minimum_gain = minimum_gain.min(stereo.linked_gain);
            assert!((PROGRAMME_LIMITER_MIN_UPPER_GAIN..=1.0).contains(&stereo.linked_gain));
            assert!(linked.into_iter().all(f64::is_finite));
            stereo_right.push(linked[1]);
            solo_right.push(solo[0]);
        }
        assert!(minimum_gain < 0.40);
        assert!(
            rms(&stereo_right[1_024..]) < rms(&solo_right[1_024..]) * 0.70,
            "linked right RMS {} vs solo {}",
            rms(&stereo_right[1_024..]),
            rms(&solo_right[1_024..]),
        );
    }

    #[test]
    fn high_frequency_acceleration_limiter_releases_transparently() {
        let mut limiter = HighFrequencyAccelerationLimiter::default();
        for index in 0..2_400 {
            let sample = if index % 2 == 0 { 0.8 } else { -0.8 };
            limiter.process_frame([sample, 0.0], 1, 48_000.0, 1.0);
        }
        assert!(limiter.linked_gain < 0.40);
        for _ in 0..9_600 {
            limiter.process_frame([0.0, 0.0], 1, 48_000.0, 1.0);
        }
        assert!(
            limiter.linked_gain > 0.99,
            "release ended at {}",
            limiter.linked_gain,
        );
    }

    #[test]
    fn surface_only_render_bypasses_programme_acceleration_limiter() {
        let mut bypass = simulation_dsp();
        let mut limited = simulation_dsp();
        let surface = (0..48_000)
            .map(|index| {
                (0.2 * (std::f64::consts::TAU * 8_000.0 * index as f64 / 48_000.0).sin()) as f32
            })
            .collect::<Vec<_>>();
        bypass.surface_asset = Arc::new(vec![surface.clone(), surface.clone()]);
        limited.surface_asset = Arc::new(vec![surface.clone(), surface]);
        bypass.set_high_frequency_acceleration_limit(0.0).unwrap();
        limited.set_high_frequency_acceleration_limit(1.0).unwrap();
        bypass.trigger_needle_drop();
        limited.trigger_needle_drop();
        bypass.render_surface(4_096, 2);
        limited.render_surface(4_096, 2);
        assert_eq!(bypass.output, limited.output);
        assert_eq!(
            bypass.high_frequency_acceleration_limiter,
            limited.high_frequency_acceleration_limiter,
        );
    }

    #[test]
    fn manual_fader_defaults_to_an_exact_noop_and_validates_range() {
        let mut dsp = simulation_dsp();
        assert_eq!(dsp.manual_fader_gain(), 1.0);
        dsp.output = vec![0.8, -0.4, 0.25, -1.0];
        let unchanged = dsp.output.clone();
        dsp.scratch_gate_trace = vec![1.0, 1.0];
        dsp.apply_crossfader_trace(2, 2);
        assert_eq!(dsp.output, unchanged);

        dsp.output.clone_from(&unchanged);
        dsp.scratch_gate_trace = vec![0.25, 0.5];
        dsp.set_manual_fader_gain(0.4).unwrap();
        dsp.apply_crossfader_trace(2, 2);
        let mut expected = unchanged;
        for frame in 0..2 {
            for channel in 0..2 {
                expected[frame * 2 + channel] *= 0.4;
            }
        }
        assert_eq!(dsp.output, expected);
        assert_eq!(dsp.manual_fader_gain(), 0.4);
        assert!(!valid_unit_interval(-0.01));
        assert!(!valid_unit_interval(f64::INFINITY));
        assert!(!valid_unit_interval(1.01));
    }

    #[test]
    fn manual_crossfader_uses_the_shared_sharp_rust_curve() {
        let mut dsp = simulation_dsp();
        for (position, expected) in [(0.0, 0.0), (0.04, 0.5), (0.08, 1.0), (0.5, 1.0)] {
            dsp.set_manual_crossfader(position).unwrap();
            assert!(
                (dsp.manual_fader_gain() - expected).abs() < 1e-6,
                "position {position} produced {}",
                dsp.manual_fader_gain(),
            );
        }
        assert_eq!(
            crate::PlayerConfig::default().sharp_crossfader_width,
            DEFAULT_SHARP_CROSSFADER_WIDTH,
        );
    }

    #[test]
    fn automatic_preset_owns_the_real_fader_while_baby_uses_manual_control() {
        let mut baby = simulation_dsp();
        baby.output = vec![0.8, -0.4];
        baby.scratch_gate_trace = vec![1.0];
        baby.set_manual_fader_gain(0.0).unwrap();
        baby.apply_crossfader_trace(1, 2);
        assert_eq!(baby.output, vec![0.0, -0.0]);

        let mut automatic = simulation_dsp();
        automatic.set_scratch_preset("stab").unwrap();
        automatic.output = vec![0.8, -0.4];
        automatic.scratch_gate_trace = vec![0.25];
        automatic.set_manual_fader_gain(0.0).unwrap();
        automatic.apply_crossfader_trace(1, 2);
        assert_eq!(automatic.output, vec![0.2, -0.1]);
    }

    #[test]
    fn held_momentary_crossfader_overrides_every_selected_technique() {
        let mut close = simulation_dsp();
        close.set_scratch_preset("stab").unwrap();
        close.set_momentary_crossfader_override(true, false);
        close.output = vec![1.0; 2_048];
        close.scratch_gate_trace = vec![1.0; 1_024];
        close.apply_crossfader_trace(1_024, 2);
        assert!(close.audible_crossfader_gain() < 1.0e-12);
        assert!(close.output[2_046].abs() < 1.0e-12);

        let mut open = simulation_dsp();
        open.set_scratch_preset("crab").unwrap();
        open.set_momentary_crossfader_override(true, true);
        open.output = vec![1.0; 2_048];
        open.scratch_gate_trace = vec![0.0; 1_024];
        open.apply_crossfader_trace(1_024, 2);
        assert!(1.0 - open.audible_crossfader_gain() < 1.0e-12);
        assert!(1.0 - open.output[2_046] < 1.0e-12);

        open.set_momentary_crossfader_override(false, false);
        open.output.fill(1.0);
        open.scratch_gate_trace.fill(0.0);
        open.apply_crossfader_trace(1_024, 2);
        assert!(open.audible_crossfader_gain() < 1.0e-12);
        assert!(open.output[2_046].abs() < 1.0e-12);
    }

    #[test]
    fn clearing_media_preserves_live_platter_velocity_and_phase() {
        let mut dsp = simulation_dsp();
        dsp.motor_delivered_rate = 0.82;
        dsp.rate = 0.79;
        dsp.rate_velocity = 0.03;
        dsp.last_effective_rate = 0.8;
        dsp.platter_rotation_turns = 17.25;

        dsp.clear_window();

        assert_eq!(dsp.motor_delivered_rate, 0.82);
        assert_eq!(dsp.rate, 0.79);
        assert_eq!(dsp.rate_velocity, 0.03);
        assert_eq!(dsp.last_effective_rate, 0.8);
        assert_eq!(dsp.platter_rotation_turns, 17.25);
        assert_eq!(dsp.position, 0.0);
        assert_eq!(dsp.target_position, 0.0);
    }

    #[test]
    fn programme_end_returns_the_exact_rendered_prefix_and_zeroes_the_suffix() {
        let mut dsp = ScratchAcousticDsp::new_internal(48_000.0, AcousticConfig::default());
        dsp.source_sample_rate = 48_000.0;
        dsp.channels = Arc::new(vec![vec![0.5_f32; 512], vec![-0.5_f32; 512]]);
        dsp.window_start = 0;
        dsp.window_end = 512;
        dsp.total_frames = 512;
        dsp.set_effects(false, false);
        dsp.start();
        dsp.set_position(472.0, 0.0);
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        seed_deck_rates(&mut dsp, 1.0, 1.0, 0.0);
        let quantum_ramp_ms = 128.0 * 1_000.0 / dsp.output_sample_rate;
        dsp.set_output_gain(0.0, quantum_ramp_ms).unwrap();

        let turns_before = dsp.platter_rotation_turns;
        let rendered = dsp.render(128, 2);

        assert_eq!(rendered, 37);
        assert!(dsp.take_ended());
        assert!(!dsp.take_ended());
        assert_eq!(dsp.position, 509.0);
        assert!(dsp.output[..rendered as usize * 2]
            .iter()
            .any(|sample| *sample != 0.0));
        assert!(dsp.output[rendered as usize * 2..]
            .iter()
            .all(|sample| *sample == 0.0));
        let expected_turns = 37.0 * dsp.native_rpm / (60.0 * dsp.output_sample_rate);
        assert!((dsp.platter_rotation_turns - turns_before - expected_turns).abs() < 1e-12);
        assert_eq!(dsp.output_gain_remaining_frames, 128 - rendered as usize);
        assert!((dsp.output_gain_current - 91.0 / 128.0).abs() < 1e-12);
    }

    #[test]
    fn replay_snapshot_restores_dynamic_dsp_state_without_restarting_inertia() {
        let mut dsp = simulation_dsp();
        Arc::make_mut(&mut dsp.channels)[0].fill(0.5);
        dsp.set_effects(false, false);
        dsp.start();
        dsp.set_position(2_400_000.0, 0.0);
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        seed_deck_rates(&mut dsp, 1.0, 1.0, 12.5);
        dsp.manual_fader_gain = 0.73;
        dsp.window_miss_frames = 17;
        dsp.window_programme_gain = 0.42;
        dsp.capture_replay_state();

        dsp.start();
        dsp.set_position(10.0, 0.0);
        dsp.set_transport(true, 0.0, -4.0, 1.0);
        dsp.platter_rotation_turns = -3.0;
        dsp.manual_fader_gain = 0.0;
        dsp.window_miss_frames = 0;
        dsp.window_programme_gain = 1.0;

        assert!(dsp.restore_replay_state());
        assert!(!dsp.restore_replay_state());
        assert_eq!(dsp.position, 2_400_000.0);
        assert_eq!(dsp.motor_delivered_rate, 1.0);
        assert_eq!(dsp.rate, 1.0);
        assert_eq!(dsp.last_effective_rate, 1.0);
        assert_eq!(dsp.platter_rotation_turns, 12.5);
        assert_eq!(dsp.manual_fader_gain, 0.73);
        assert_eq!(dsp.window_miss_frames, 17);
        assert_eq!(dsp.window_programme_gain, 0.42);
        dsp.render(32, 2);
        assert!(dsp.last_effective_rate > 0.99);
        assert!(dsp.output.iter().any(|sample| *sample != 0.0));
    }

    #[test]
    fn replay_restore_retains_snapshot_and_swaps_heap_storage() {
        let mut dsp = ScratchAcousticDsp::new_internal(48_000.0, AcousticConfig::default());
        dsp.drag_lowpass_state = vec![0.1, 0.2];
        dsp.last_output_samples = vec![0.3, 0.4];
        dsp.capture_replay_state();

        let snapshot = dsp.replay_snapshot.as_ref().unwrap();
        let snapshot_address = (&**snapshot) as *const AcousticReplaySnapshot;
        let captured_drag_address = snapshot.drag_lowpass_state.as_ptr();
        let captured_output_address = snapshot.last_output_samples.as_ptr();

        dsp.begin_deterministic_replay(0.0, 0.0, 1).unwrap();
        let replay_drag_address = dsp.drag_lowpass_state.as_ptr();
        let replay_output_address = dsp.last_output_samples.as_ptr();
        assert_ne!(captured_drag_address, replay_drag_address);
        assert_ne!(captured_output_address, replay_output_address);

        assert!(dsp.restore_replay_state());
        assert_eq!(dsp.drag_lowpass_state, vec![0.1, 0.2]);
        assert_eq!(dsp.last_output_samples, vec![0.3, 0.4]);
        assert_eq!(dsp.drag_lowpass_state.as_ptr(), captured_drag_address);
        assert_eq!(dsp.last_output_samples.as_ptr(), captured_output_address);

        let snapshot = dsp.replay_snapshot.as_ref().unwrap();
        assert_eq!(
            (&**snapshot) as *const AcousticReplaySnapshot,
            snapshot_address
        );
        assert_eq!(snapshot.drag_lowpass_state.as_ptr(), replay_drag_address);
        assert_eq!(snapshot.last_output_samples.as_ptr(), replay_output_address);
        assert!(!snapshot.restore_pending);
        assert!(!dsp.restore_replay_state());

        dsp.capture_replay_state();
        let snapshot = dsp.replay_snapshot.as_ref().unwrap();
        assert_eq!(
            (&**snapshot) as *const AcousticReplaySnapshot,
            snapshot_address
        );
        assert!(snapshot.restore_pending);
    }

    #[test]
    #[ignore = "manual replay-restore microbenchmark"]
    fn benchmark_replay_restore_without_reclamation() {
        const ITERATIONS: u32 = 1_000_000;

        let mut dsp = ScratchAcousticDsp::new_internal(48_000.0, AcousticConfig::default());
        dsp.drag_lowpass_state = vec![0.1, 0.2];
        dsp.last_output_samples = vec![0.3, 0.4];
        dsp.capture_replay_state();
        dsp.begin_deterministic_replay(0.0, 0.0, 1).unwrap();

        let started = std::time::Instant::now();
        for _ in 0..ITERATIONS {
            dsp.replay_snapshot.as_mut().unwrap().restore_pending = true;
            std::hint::black_box(dsp.restore_replay_state());
        }
        let nanoseconds_per_restore =
            started.elapsed().as_secs_f64() * 1_000_000_000.0 / f64::from(ITERATIONS);
        eprintln!("replay restore: {nanoseconds_per_restore:.2} ns/operation");
    }

    #[test]
    fn deterministic_replay_initialization_resets_dynamic_state_and_restores_live_state() {
        let mut dsp = simulation_dsp();
        dsp.start();
        dsp.set_position(2_400_000.0, 0.0);
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        dsp.motor_delivered_rate = 0.81;
        dsp.rate = 0.77;
        dsp.wow_phase = 0.63;
        dsp.flutter_phase = 0.42;
        dsp.noise_seed = 17;
        dsp.manual_fader_gain = 0.73;
        dsp.capture_replay_state();

        dsp.set_scratch_preset("crab").unwrap();
        dsp.set_scratch_clicks(8);
        dsp.set_manual_fader_gain(0.4).unwrap();
        dsp.set_output_gain(0.75, 0.0).unwrap();
        dsp.begin_deterministic_replay(24_000.0, -2.25, 0x4d2c_6df3)
            .unwrap();
        let first = (
            dsp.position,
            dsp.wow_phase,
            dsp.flutter_phase,
            dsp.platter_rotation_turns,
            dsp.noise_seed,
            dsp.scratch_gate(),
            dsp.scratch_gate_phase(),
            dsp.scratch_direction(),
        );
        assert_eq!(dsp.scratch_preset(), "crab");
        assert_eq!(dsp.scratch_clicks(), 8);
        assert_eq!(dsp.manual_fader_gain(), 0.4);
        assert_eq!(dsp.output_gain_current, 0.75);
        assert!(dsp.drag_lowpass_state.is_empty());
        assert!(dsp.last_output_samples.is_empty());
        assert!(dsp.surface_bed.is_none());
        assert!(dsp.needle_thump.is_none());
        assert!(dsp.needle_burst.is_none());

        dsp.set_transport(true, 0.0, -4.0, 1.0);
        dsp.set_motion(23_000.0, -4.0, 0.8);
        dsp.render(2_048, 2);
        assert_ne!(dsp.noise_seed, first.4);
        dsp.begin_deterministic_replay(24_000.0, -2.25, 0x4d2c_6df3)
            .unwrap();
        let second = (
            dsp.position,
            dsp.wow_phase,
            dsp.flutter_phase,
            dsp.platter_rotation_turns,
            dsp.noise_seed,
            dsp.scratch_gate(),
            dsp.scratch_gate_phase(),
            dsp.scratch_direction(),
        );
        assert_eq!(second, first);

        assert!(dsp.restore_replay_state());
        assert_eq!(dsp.position, 2_400_000.0);
        assert_eq!(dsp.motor_delivered_rate, 0.81);
        assert_eq!(dsp.rate, 0.77);
        assert_eq!(dsp.wow_phase, 0.63);
        assert_eq!(dsp.flutter_phase, 0.42);
        assert_eq!(dsp.noise_seed, 17);
        assert_eq!(dsp.manual_fader_gain, 0.73);
        assert_eq!(dsp.scratch_preset(), "baby");
    }

    #[test]
    fn output_gain_unity_preserves_normal_render_bit_for_bit() {
        let mut default = scratch_signal_dsp(ScratchPreset::Baby, 1.0);
        let mut explicit_unity = scratch_signal_dsp(ScratchPreset::Baby, 1.0);
        explicit_unity.set_output_gain(1.0, 12.0).unwrap();
        default.render(2_048, 2);
        explicit_unity.render(2_048, 2);
        assert_eq!(default.output, explicit_unity.output);
    }

    #[test]
    fn output_gain_reaches_its_linear_ramp_target() {
        let mut dsp = simulation_dsp();
        let four_frames_ms = 4.0 * 1_000.0 / dsp.output_sample_rate;
        dsp.set_output_gain(0.0, four_frames_ms).unwrap();
        dsp.output = vec![1.0; 8];
        dsp.apply_output_gain(4, 2);

        assert_eq!(dsp.output, vec![1.0, 1.0, 0.75, 0.75, 0.5, 0.5, 0.25, 0.25],);
        assert_eq!(dsp.output_gain_current, 0.0);
        assert_eq!(dsp.output_gain_target, 0.0);
        assert_eq!(dsp.output_gain_step, 0.0);
        assert_eq!(dsp.output_gain_remaining_frames, 0);

        dsp.output = vec![1.0; 2];
        dsp.apply_output_gain(1, 2);
        assert_eq!(dsp.output, vec![0.0, 0.0]);
    }

    #[test]
    fn replay_snapshot_restores_output_gain_mid_ramp() {
        let mut dsp = simulation_dsp();
        let four_frames_ms = 4.0 * 1_000.0 / dsp.output_sample_rate;
        dsp.set_output_gain(0.25, four_frames_ms).unwrap();
        dsp.output = vec![1.0; 2];
        dsp.apply_output_gain(2, 1);
        dsp.capture_replay_state();

        assert_eq!(dsp.output_gain_current, 0.625);
        assert_eq!(dsp.output_gain_target, 0.25);
        assert_eq!(dsp.output_gain_step, -0.1875);
        assert_eq!(dsp.output_gain_remaining_frames, 2);

        dsp.set_output_gain(2.0, 0.0).unwrap();
        assert!(dsp.restore_replay_state());
        assert_eq!(dsp.output_gain_current, 0.625);
        assert_eq!(dsp.output_gain_target, 0.25);
        assert_eq!(dsp.output_gain_step, -0.1875);
        assert_eq!(dsp.output_gain_remaining_frames, 2);

        dsp.output = vec![1.0; 2];
        dsp.apply_output_gain(2, 1);
        assert_eq!(dsp.output, vec![0.625, 0.4375]);
        assert_eq!(dsp.output_gain_current, 0.25);
        assert_eq!(dsp.output_gain_remaining_frames, 0);
    }

    #[test]
    fn manual_fader_gain_one_preserves_normal_render_bit_for_bit() {
        let mut default = scratch_signal_dsp(ScratchPreset::Baby, 1.0);
        let mut explicit_unity = scratch_signal_dsp(ScratchPreset::Baby, 1.0);
        explicit_unity.set_manual_fader_gain(1.0).unwrap();
        default.render(2_048, 2);
        explicit_unity.render(2_048, 2);
        assert_eq!(default.output, explicit_unity.output);
    }

    #[test]
    fn window_miss_holds_position_and_resumes_with_a_bounded_fade() {
        let mut dsp = scratch_signal_dsp(ScratchPreset::Baby, 1.0);
        dsp.render(512, 1);
        let full_level = *dsp.output.last().unwrap();
        // The hand target in this fixture never advances, so the record eases
        // off against it and the cartridge's output eases with it. The resume
        // is measured against the programme level the deck's own rate implies
        // at that moment, not against a level captured at a faster one.
        let full_gain = dsp.movement_gain_state;
        let held_position = dsp.position;

        dsp.render_window_missing(64, 1);
        let short_miss_tail = *dsp.output.last().unwrap();
        assert_eq!(dsp.position, held_position);
        assert!(short_miss_tail.abs() < full_level.abs());

        dsp.render(128, 1);
        let short_resume_head = dsp.output[0];
        assert!((short_resume_head - short_miss_tail).abs() < 0.01);
        assert!(dsp.position > held_position);
        assert!(dsp.output[127].abs() > short_resume_head.abs());

        let second_held_position = dsp.position;
        dsp.render_window_missing(512, 1);
        let long_miss_tail = *dsp.output.last().unwrap();
        assert_eq!(dsp.position, second_held_position);
        assert!(long_miss_tail.abs() < 1e-7);

        dsp.render(128, 1);
        assert!(dsp.output[0].abs() < 1e-7);
        assert!(dsp.output[127].abs() > dsp.output[0].abs());
        assert!(dsp.position > second_held_position);
        dsp.render(512, 1);
        let recovered =
            f64::from(full_level.abs()) * (dsp.movement_gain_state / full_gain);
        assert!(f64::from((*dsp.output.last().unwrap()).abs()) > recovered * 0.95);
    }

    #[test]
    fn manual_fader_scales_window_miss_and_surface_outputs() {
        let mut miss_unity = simulation_dsp();
        let mut miss_scaled = simulation_dsp();
        miss_unity.last_output_samples = vec![0.8, -0.4];
        miss_scaled.last_output_samples = vec![0.8, -0.4];
        miss_scaled.set_manual_fader_gain(0.5).unwrap();
        miss_unity.render_window_missing(32, 2);
        miss_scaled.render_window_missing(32, 2);
        for (unity, scaled) in miss_unity.output.iter().zip(&miss_scaled.output) {
            assert_eq!(*scaled, *unity * 0.5);
        }

        let mut surface_unity = simulation_dsp();
        let mut surface_scaled = simulation_dsp();
        surface_scaled.set_manual_fader_gain(0.25).unwrap();
        surface_unity.trigger_needle_drop();
        surface_scaled.trigger_needle_drop();
        surface_unity.render_surface(4_096, 2);
        surface_scaled.render_surface(4_096, 2);
        assert!(surface_unity
            .output
            .iter()
            .any(|sample| sample.abs() > 1e-6));
        for (unity, scaled) in surface_unity.output.iter().zip(&surface_scaled.output) {
            assert_eq!(*scaled, *unity * 0.25);
        }
    }

    #[test]
    fn output_gain_scales_window_miss_and_surface_outputs() {
        let mut miss_unity = simulation_dsp();
        let mut miss_scaled = simulation_dsp();
        miss_unity.last_output_samples = vec![0.8, -0.4];
        miss_scaled.last_output_samples = vec![0.8, -0.4];
        miss_scaled.set_output_gain(0.5, 0.0).unwrap();
        miss_unity.render_window_missing(32, 2);
        miss_scaled.render_window_missing(32, 2);
        for (unity, scaled) in miss_unity.output.iter().zip(&miss_scaled.output) {
            assert_eq!(*scaled, *unity * 0.5);
        }

        let mut surface_unity = simulation_dsp();
        let mut surface_scaled = simulation_dsp();
        surface_scaled.set_output_gain(0.25, 0.0).unwrap();
        surface_unity.trigger_needle_drop();
        surface_scaled.trigger_needle_drop();
        surface_unity.render_surface(4_096, 2);
        surface_scaled.render_surface(4_096, 2);
        assert!(surface_unity
            .output
            .iter()
            .any(|sample| sample.abs() > 1e-6));
        for (unity, scaled) in surface_unity.output.iter().zip(&surface_scaled.output) {
            assert_eq!(*scaled, *unity * 0.25);
        }
    }

    #[test]
    fn missing_surface_asset_uses_bounded_synthetic_bed_and_burst_without_panicking() {
        let mut bed = simulation_dsp();
        assert!(bed.surface_asset.is_empty());
        bed.start_surface_region(SURFACE_REGION_LEAD_IN, 0.20);
        bed.render_surface(4_096, 2);
        assert!(bed.output.iter().any(|sample| sample.abs() > 1e-7));
        assert!(bed
            .output
            .iter()
            .all(|sample| sample.is_finite() && (-1.0..=1.0).contains(sample)));

        let mut drop = simulation_dsp();
        assert!(drop.surface_asset.is_empty());
        drop.trigger_needle_drop();
        drop.render_surface(8_192, 2);
        assert!(drop
            .output
            .iter()
            .all(|sample| sample.is_finite() && (-1.0..=1.0).contains(sample)));
        let after_thump = 5_280 * 2;
        assert!(
            drop.output[after_thump..]
                .iter()
                .any(|sample| sample.abs() > 1e-7),
            "synthetic crackle burst should outlive the 100 ms thump"
        );
    }

    #[test]
    fn needle_lift_foley_remains_audible_while_programme_is_silent() {
        let mut lift = simulation_dsp();
        lift.set_needle_lifted(true);
        lift.trigger_needle_lift();
        lift.render_surface(4_096, 2);
        assert!(lift.output.iter().any(|sample| sample.abs() > 1e-7));
        assert!(lift
            .output
            .iter()
            .all(|sample| sample.is_finite() && (-1.0..=1.0).contains(sample)));
    }

    #[test]
    fn disabling_surface_effects_clears_and_suppresses_all_foley() {
        let mut dsp = simulation_dsp();
        dsp.start_surface_region(SURFACE_REGION_LEAD_IN, 0.20);
        dsp.trigger_needle_drop();
        assert!(dsp.surface_bed.is_some());
        assert!(dsp.needle_thump.is_some());
        assert!(dsp.needle_burst.is_some());

        dsp.set_effects(true, false);
        assert!(dsp.surface_bed.is_none());
        assert!(dsp.needle_thump.is_none());
        assert!(dsp.needle_burst.is_none());
        dsp.start_surface_region(SURFACE_REGION_LEAD_IN, 0.20);
        dsp.trigger_needle_drop();
        assert!(dsp.surface_bed.is_none());
        assert!(dsp.needle_thump.is_none());
        assert!(dsp.needle_burst.is_none());
        dsp.render_surface(4_096, 2);
        assert!(dsp.output.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn deterministic_hash_noise_is_stable() {
        assert_eq!(
            ScratchAcousticDsp::hash_noise(42, 7),
            ScratchAcousticDsp::hash_noise(42, 7)
        );
        assert_ne!(
            ScratchAcousticDsp::hash_noise(42, 7),
            ScratchAcousticDsp::hash_noise(43, 7)
        );
    }

    #[test]
    fn scratch_gate_is_applied_to_rendered_deck_audio() {
        let mut baby = scratch_signal_dsp(ScratchPreset::Baby, -1.0);
        baby.render(512, 1);
        baby.render(1024, 1);
        let baby_rms = output_rms(&baby);

        let mut stab = scratch_signal_dsp(ScratchPreset::Stab, -1.0);
        stab.render(512, 1);
        stab.render(1024, 1);
        let stab_rms = output_rms(&stab);

        assert!(
            baby_rms > 0.35,
            "baby should pass the groove, got {baby_rms}"
        );
        assert!(
            stab_rms < baby_rms * 0.02,
            "reverse stab should cut the groove: baby={baby_rms}, stab={stab_rms}"
        );
    }

    #[test]
    fn scratch_gate_reversal_commits_on_the_rendered_motion_crossing() {
        let mut dsp = scratch_signal_dsp(ScratchPreset::Transform, 8.0);
        dsp.render(512, 1);
        assert_eq!(dsp.scratch_direction(), 1);

        dsp.set_motion(dsp.position, -8.0, 0.0);
        dsp.render(320, 1);
        assert_eq!(dsp.scratch_direction(), 1);
        assert!(dsp.last_effective_rate > 0.0);

        let mut confirmation_frames = 0;
        while dsp.scratch_direction() > 0 && confirmation_frames < 24_000 {
            dsp.render(1, 1);
            confirmation_frames += 1;
        }
        assert_eq!(dsp.scratch_direction(), -1);
        assert!(dsp.last_effective_rate < 0.0);
        assert!(dsp.scratch_gate_phase() > 0.0);
        assert!(confirmation_frames < 24_000);
    }

    #[test]
    fn releasing_the_record_reopens_gate_for_motor_handoff() {
        let mut dsp = scratch_signal_dsp(ScratchPreset::Stab, -1.0);
        dsp.render(1024, 1);
        assert!(dsp.scratch_gate() < 0.01);

        dsp.set_transport(false, 1.0, 0.0, 0.0);
        dsp.render(1024, 1);
        assert!(dsp.scratch_gate() > 0.99);
        assert!(output_rms(&dsp) > 0.25);
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationAnchor {
    pub sample: f64,
    pub radial: f64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgrammeCalibrationGap {
    start_sample: f64,
    end_sample: f64,
    radial_start_normalized: f64,
    radial_end_normalized: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgrammeCalibrationMap {
    total_samples: f64,
    #[serde(default)]
    gaps: Vec<ProgrammeCalibrationGap>,
}

#[derive(Clone, Debug)]
struct MonotoneInterpolant {
    xs: Vec<f64>,
    ys: Vec<f64>,
    widths: Vec<f64>,
    tangents: Vec<f64>,
}

impl MonotoneInterpolant {
    fn new(xs: Vec<f64>, ys: Vec<f64>) -> Result<Self, String> {
        if xs.len() != ys.len() {
            return Err("monotone interpolant requires equal-length xs/ys".to_owned());
        }
        if xs.len() < 2 {
            return Err("monotone interpolant requires at least two anchors".to_owned());
        }
        for index in 1..xs.len() {
            if !xs[index].is_finite() || xs[index] <= xs[index - 1] {
                return Err("monotone interpolant requires strictly increasing xs".to_owned());
            }
            if !ys[index].is_finite() || ys[index] <= ys[index - 1] {
                return Err("monotone interpolant requires strictly increasing ys".to_owned());
            }
        }
        let widths = xs
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect::<Vec<_>>();
        let deltas = ys
            .windows(2)
            .zip(widths.iter())
            .map(|(pair, width)| (pair[1] - pair[0]) / width)
            .collect::<Vec<_>>();
        let mut tangents = vec![0.0; xs.len()];
        for index in 1..xs.len() - 1 {
            if deltas[index - 1] * deltas[index] <= 0.0 {
                tangents[index] = 0.0;
            } else {
                let w1 = 2.0 * widths[index] + widths[index - 1];
                let w2 = widths[index] + 2.0 * widths[index - 1];
                tangents[index] = (w1 + w2) / (w1 / deltas[index - 1] + w2 / deltas[index]);
            }
        }
        tangents[0] = endpoint_slope(
            widths[0],
            widths.get(1).copied(),
            deltas[0],
            deltas.get(1).copied(),
        );
        let last = xs.len() - 1;
        tangents[last] = endpoint_slope(
            widths[last - 1],
            last.checked_sub(2)
                .and_then(|index| widths.get(index).copied()),
            deltas[last - 1],
            last.checked_sub(2)
                .and_then(|index| deltas.get(index).copied()),
        );
        Ok(Self {
            xs,
            ys,
            widths,
            tangents,
        })
    }

    fn segment_for_x(&self, value: f64) -> usize {
        if value <= self.xs[0] {
            return 0;
        }
        if value >= self.xs[self.xs.len() - 1] {
            return self.xs.len() - 2;
        }
        self.xs
            .partition_point(|candidate| *candidate <= value)
            .saturating_sub(1)
    }

    fn segment_for_y(&self, value: f64) -> usize {
        if value <= self.ys[0] {
            return 0;
        }
        if value >= self.ys[self.ys.len() - 1] {
            return self.ys.len() - 2;
        }
        self.ys
            .partition_point(|candidate| *candidate <= value)
            .saturating_sub(1)
    }

    fn hermite(&self, index: usize, t: f64) -> f64 {
        let t2 = t * t;
        let t3 = t2 * t;
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        h00 * self.ys[index]
            + h10 * self.widths[index] * self.tangents[index]
            + h01 * self.ys[index + 1]
            + h11 * self.widths[index] * self.tangents[index + 1]
    }

    fn evaluate(&self, value: f64) -> f64 {
        if value <= self.xs[0] {
            return self.ys[0];
        }
        if value >= self.xs[self.xs.len() - 1] {
            return self.ys[self.ys.len() - 1];
        }
        let index = self.segment_for_x(value);
        let t = (value - self.xs[index]) / self.widths[index];
        self.hermite(index, t)
    }

    fn evaluate_inverse(&self, value: f64) -> f64 {
        if value <= self.ys[0] {
            return self.xs[0];
        }
        if value >= self.ys[self.ys.len() - 1] {
            return self.xs[self.xs.len() - 1];
        }
        let index = self.segment_for_y(value);
        let mut low = 0.0;
        let mut high = 1.0;
        for _ in 0..40 {
            let mid = (low + high) * 0.5;
            if self.hermite(index, mid) < value {
                low = mid;
            } else {
                high = mid;
            }
        }
        self.xs[index] + (low + high) * 0.5 * self.widths[index]
    }
}

fn endpoint_slope(ha: f64, hb: Option<f64>, da: f64, db: Option<f64>) -> f64 {
    let (Some(hb), Some(db)) = (hb, db) else {
        return da;
    };
    let slope = ((2.0 * ha + hb) * da - ha * db) / (ha + hb);
    if slope.signum() != da.signum() {
        return 0.0;
    }
    if da.signum() != db.signum() && slope.abs() > (3.0 * da).abs() {
        return 3.0 * da;
    }
    slope
}

#[wasm_bindgen]
pub struct StylusCalibration {
    total_samples: f64,
    interpolant: Option<MonotoneInterpolant>,
}

#[wasm_bindgen]
impl StylusCalibration {
    #[wasm_bindgen(constructor)]
    pub fn new(total_samples: f64, anchors: JsValue) -> Result<StylusCalibration, JsValue> {
        if !total_samples.is_finite() || total_samples <= 0.0 {
            return Err(JsValue::from_str("totalSamples must be positive"));
        }
        let anchors: Vec<CalibrationAnchor> = serde_wasm_bindgen::from_value(anchors)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let interpolant = if anchors.is_empty() {
            None
        } else {
            validate_anchors(total_samples, &anchors).map_err(|error| JsValue::from_str(&error))?;
            Some(
                MonotoneInterpolant::new(
                    anchors.iter().map(|anchor| anchor.sample).collect(),
                    anchors.iter().map(|anchor| anchor.radial).collect(),
                )
                .map_err(|error| JsValue::from_str(&error))?,
            )
        };
        Ok(Self {
            total_samples,
            interpolant,
        })
    }

    #[wasm_bindgen(js_name = fromProgrammeMap)]
    pub fn from_programme_map(programme: JsValue) -> Result<StylusCalibration, JsValue> {
        let programme: ProgrammeCalibrationMap = serde_wasm_bindgen::from_value(programme)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        Self::try_from_programme_map(programme).map_err(|error| JsValue::from_str(&error))
    }

    #[wasm_bindgen(getter, js_name = hasGaps)]
    pub fn has_gaps(&self) -> bool {
        self.interpolant.is_some()
    }

    #[wasm_bindgen(getter, js_name = totalSamples)]
    pub fn total_samples(&self) -> f64 {
        self.total_samples
    }

    #[wasm_bindgen(js_name = sampleToGroove)]
    pub fn sample_to_groove(&self, sample: f64) -> f64 {
        let sample = sample.clamp(0.0, self.total_samples);
        self.interpolant
            .as_ref()
            .map(|interpolant| interpolant.evaluate(sample).clamp(0.0, 1.0))
            .unwrap_or_else(|| (sample / self.total_samples).clamp(0.0, 1.0))
    }

    #[wasm_bindgen(js_name = grooveToSample)]
    pub fn groove_to_sample(&self, groove: f64) -> f64 {
        let groove = groove.clamp(0.0, 1.0);
        self.interpolant
            .as_ref()
            .map(|interpolant| {
                interpolant
                    .evaluate_inverse(groove)
                    .clamp(0.0, self.total_samples)
            })
            .unwrap_or(groove * self.total_samples)
    }
}

impl StylusCalibration {
    /// Creates the shared stylus calibration from a programme-map JSON object.
    pub fn from_programme_map_json_native(value: &str) -> Result<StylusCalibration, String> {
        let programme: ProgrammeCalibrationMap =
            serde_json::from_str(value).map_err(|error| error.to_string())?;
        Self::try_from_programme_map(programme)
    }

    fn try_from_programme_map(
        programme: ProgrammeCalibrationMap,
    ) -> Result<StylusCalibration, String> {
        let total_samples = programme.total_samples;
        if !total_samples.is_finite() || total_samples <= 0.0 || total_samples.fract() != 0.0 {
            return Err("programme map requires a positive integer totalSamples".to_owned());
        }
        if programme.gaps.is_empty() {
            return Ok(Self {
                total_samples,
                interpolant: None,
            });
        }

        let mut indexed_gaps = programme.gaps.into_iter().enumerate().collect::<Vec<_>>();
        for (index, gap) in &indexed_gaps {
            if !gap.start_sample.is_finite()
                || !gap.end_sample.is_finite()
                || gap.start_sample.fract() != 0.0
                || gap.end_sample.fract() != 0.0
            {
                return Err(format!(
                    "gap {index}: startSample and endSample must be finite integers"
                ));
            }
            if !gap.radial_start_normalized.is_finite() || !gap.radial_end_normalized.is_finite() {
                return Err(format!(
                    "gap {index}: radialStartNormalized and radialEndNormalized must be finite"
                ));
            }
        }
        indexed_gaps.sort_by(|left, right| left.1.start_sample.total_cmp(&right.1.start_sample));

        let mut anchors = Vec::with_capacity(indexed_gaps.len() * 2 + 2);
        anchors.push(CalibrationAnchor {
            sample: 0.0,
            radial: 0.0,
        });
        let mut previous_end_sample = 0.0;
        let mut previous_radial_end = 0.0;
        for (index, gap) in indexed_gaps {
            if gap.start_sample < previous_end_sample {
                return Err(format!("gap {index}: sample regions must not overlap"));
            }
            if !(gap.start_sample > 0.0
                && gap.start_sample < gap.end_sample
                && gap.end_sample <= total_samples)
            {
                return Err(format!(
                    "gap {index}: requires 0 < startSample < endSample <= totalSamples"
                ));
            }
            if gap.radial_start_normalized < previous_radial_end {
                return Err(format!("gap {index}: radial regions must not overlap"));
            }
            if !(gap.radial_start_normalized > 0.0
                && gap.radial_start_normalized < gap.radial_end_normalized
                && gap.radial_end_normalized <= 1.0)
            {
                return Err(format!(
                    "gap {index}: requires 0 < radialStartNormalized < radialEndNormalized <= 1"
                ));
            }
            anchors.push(CalibrationAnchor {
                sample: gap.start_sample,
                radial: gap.radial_start_normalized,
            });
            anchors.push(CalibrationAnchor {
                sample: gap.end_sample,
                radial: gap.radial_end_normalized,
            });
            previous_end_sample = gap.end_sample;
            previous_radial_end = gap.radial_end_normalized;
        }
        anchors.push(CalibrationAnchor {
            sample: total_samples,
            radial: 1.0,
        });
        validate_anchors(total_samples, &anchors)?;
        let interpolant = MonotoneInterpolant::new(
            anchors.iter().map(|anchor| anchor.sample).collect(),
            anchors.iter().map(|anchor| anchor.radial).collect(),
        )?;
        Ok(Self {
            total_samples,
            interpolant: Some(interpolant),
        })
    }
}

fn validate_anchors(total_samples: f64, anchors: &[CalibrationAnchor]) -> Result<(), String> {
    if anchors.len() < 2 {
        return Err("calibration requires at least two anchors".to_owned());
    }
    if anchors[0].sample != 0.0 || anchors[0].radial != 0.0 {
        return Err("calibration must begin at sample 0 and radial 0".to_owned());
    }
    let last = anchors[anchors.len() - 1];
    if last.sample != total_samples || last.radial != 1.0 {
        return Err("calibration must end at totalSamples and radial 1".to_owned());
    }
    for pair in anchors.windows(2) {
        if !pair[0].sample.is_finite()
            || !pair[1].sample.is_finite()
            || pair[1].sample <= pair[0].sample
        {
            return Err("sample anchors must be strictly increasing".to_owned());
        }
        if !pair[0].radial.is_finite()
            || !pair[1].radial.is_finite()
            || pair[1].radial <= pair[0].radial
        {
            return Err("radial anchors must be strictly increasing".to_owned());
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchConfig {
    pub max_playback_rate: f64,
    pub deadzone_rate: f64,
    pub lock_center_rate: f64,
    pub lock_width: f64,
    pub lock_strength: f64,
    pub pointer_filter_seconds: f64,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchMotion {
    pub delta_angle_radians: f64,
    pub rotation_degrees: f64,
    pub current_time: f64,
    pub sample_position: f64,
    pub raw_playback_rate: f64,
    pub filtered_playback_rate: f64,
    pub physical_playback_rate: f64,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchWindowPlan {
    pub start: u32,
    pub end: u32,
    pub length: u32,
    pub needs_update: bool,
}

#[wasm_bindgen]
pub struct ScratchSimulation {
    config: ScratchConfig,
    active: bool,
    pointer_id: i32,
    last_angle: f64,
    last_time_ms: f64,
    filtered_pointer_rate: f64,
    current_time: f64,
    sample_position: f64,
    rotation_degrees: f64,
}

#[wasm_bindgen]
impl ScratchSimulation {
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue) -> Result<ScratchSimulation, JsValue> {
        let config: ScratchConfig = serde_wasm_bindgen::from_value(config)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        validate_scratch_config(config).map_err(|error| JsValue::from_str(&error))?;
        Ok(Self {
            config,
            active: false,
            pointer_id: -1,
            last_angle: 0.0,
            last_time_ms: 0.0,
            filtered_pointer_rate: 0.0,
            current_time: 0.0,
            sample_position: 0.0,
            rotation_degrees: 0.0,
        })
    }

    #[wasm_bindgen(js_name = begin)]
    pub fn begin(
        &mut self,
        pointer_id: i32,
        angle_radians: f64,
        time_ms: f64,
        current_time: f64,
        rotation_degrees: f64,
        sample_rate: f64,
        duration: f64,
    ) -> Result<(), JsValue> {
        validate_motion_inputs(angle_radians, time_ms, sample_rate, duration)?;
        self.active = true;
        self.pointer_id = pointer_id;
        self.last_angle = angle_radians;
        self.last_time_ms = time_ms;
        self.filtered_pointer_rate = 0.0;
        self.current_time = current_time.clamp(0.0, duration);
        self.sample_position = (self.current_time * sample_rate).clamp(0.0, duration * sample_rate);
        self.rotation_degrees = rotation_degrees;
        Ok(())
    }

    #[wasm_bindgen(js_name = update)]
    pub fn update(
        &mut self,
        pointer_id: i32,
        angle_radians: f64,
        time_ms: f64,
        duration: f64,
        sample_rate: f64,
        seconds_per_turn: f64,
        needle_lifted: bool,
    ) -> Result<JsValue, JsValue> {
        if !self.active || self.pointer_id != pointer_id {
            return Err(JsValue::from_str("scratch pointer is not active"));
        }
        validate_motion_inputs(angle_radians, time_ms, sample_rate, duration)?;
        if !seconds_per_turn.is_finite() || seconds_per_turn <= 0.0 {
            return Err(JsValue::from_str("secondsPerTurn must be positive"));
        }
        let delta_angle = normalize_angle_delta(angle_radians - self.last_angle);
        let elapsed_seconds = ((time_ms - self.last_time_ms).max(1.0) / 1000.0).max(0.004);
        self.last_angle = angle_radians;
        self.last_time_ms = time_ms;
        self.rotation_degrees += delta_angle.to_degrees();
        let mut raw_playback_rate = 0.0;
        let mut physical_playback_rate = 0.0;
        if !needle_lifted {
            let mapped_delta_seconds = delta_angle / std::f64::consts::TAU * seconds_per_turn;
            self.current_time = (self.current_time + mapped_delta_seconds).clamp(0.0, duration);
            raw_playback_rate = mapped_delta_seconds / elapsed_seconds;
            let alpha = 1.0 - (-elapsed_seconds / self.config.pointer_filter_seconds).exp();
            self.filtered_pointer_rate += (raw_playback_rate - self.filtered_pointer_rate) * alpha;
            physical_playback_rate =
                map_physical_playback_rate(self.filtered_pointer_rate, self.config);
            self.sample_position =
                (self.current_time * sample_rate).clamp(0.0, duration * sample_rate);
        }
        serde_wasm_bindgen::to_value(&ScratchMotion {
            delta_angle_radians: delta_angle,
            rotation_degrees: self.rotation_degrees,
            current_time: self.current_time,
            sample_position: self.sample_position,
            raw_playback_rate,
            filtered_playback_rate: self.filtered_pointer_rate,
            physical_playback_rate,
        })
        .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = finish)]
    pub fn finish(&mut self) {
        self.active = false;
        self.pointer_id = -1;
        self.filtered_pointer_rate = 0.0;
    }

    #[wasm_bindgen(js_name = mapPhysicalPlaybackRate)]
    pub fn map_physical_playback_rate(&self, playback_rate: f64) -> f64 {
        map_physical_playback_rate(playback_rate, self.config)
    }

    #[wasm_bindgen(js_name = planWindow)]
    pub fn plan_window(
        &self,
        center_sample_position: f64,
        frame_length: u32,
        window_frames: u32,
        current_window_start: u32,
        current_window_end: u32,
        margin_frames: u32,
        force: bool,
    ) -> Result<JsValue, JsValue> {
        let frame_length = frame_length.max(1);
        let window_frames = window_frames.max(1).min(frame_length);
        let center = center_sample_position
            .round()
            .clamp(0.0, f64::from(frame_length.saturating_sub(1))) as u32;
        let half = window_frames / 2;
        let max_start = frame_length.saturating_sub(window_frames);
        let start = center.saturating_sub(half).min(max_start);
        let end = start.saturating_add(window_frames).min(frame_length);
        let needs_update = force
            || center <= current_window_start.saturating_add(margin_frames)
            || center >= current_window_end.saturating_sub(margin_frames);
        serde_wasm_bindgen::to_value(&ScratchWindowPlan {
            start,
            end,
            length: end.saturating_sub(start),
            needs_update,
        })
        .map_err(|error| JsValue::from_str(&error.to_string()))
    }
}

fn validate_scratch_config(config: ScratchConfig) -> Result<(), String> {
    if !config.max_playback_rate.is_finite() || config.max_playback_rate <= 0.0 {
        return Err("maxPlaybackRate must be positive".to_owned());
    }
    if !config.deadzone_rate.is_finite() || config.deadzone_rate < 0.0 {
        return Err("deadzoneRate must be non-negative".to_owned());
    }
    if !config.lock_center_rate.is_finite() || config.lock_center_rate < 0.0 {
        return Err("lockCenterRate must be non-negative".to_owned());
    }
    if !config.lock_width.is_finite() || config.lock_width <= 0.0 {
        return Err("lockWidth must be positive".to_owned());
    }
    if !config.lock_strength.is_finite() || !(0.0..=1.0).contains(&config.lock_strength) {
        return Err("lockStrength must be between 0 and 1".to_owned());
    }
    if !config.pointer_filter_seconds.is_finite() || config.pointer_filter_seconds <= 0.0 {
        return Err("pointerFilterSeconds must be positive".to_owned());
    }
    Ok(())
}

fn validate_motion_inputs(
    angle_radians: f64,
    time_ms: f64,
    sample_rate: f64,
    duration: f64,
) -> Result<(), JsValue> {
    if !angle_radians.is_finite() {
        return Err(JsValue::from_str("angleRadians must be finite"));
    }
    if !time_ms.is_finite() {
        return Err(JsValue::from_str("timeMs must be finite"));
    }
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return Err(JsValue::from_str("sampleRate must be positive"));
    }
    if !duration.is_finite() || duration < 0.0 {
        return Err(JsValue::from_str("duration must be non-negative"));
    }
    Ok(())
}

fn normalize_angle_delta(delta: f64) -> f64 {
    let mut normalized = delta;
    while normalized > std::f64::consts::PI {
        normalized -= std::f64::consts::TAU;
    }
    while normalized < -std::f64::consts::PI {
        normalized += std::f64::consts::TAU;
    }
    normalized
}

fn map_physical_playback_rate(playback_rate: f64, config: ScratchConfig) -> f64 {
    if !playback_rate.is_finite() || playback_rate.abs() < config.deadzone_rate {
        return 0.0;
    }
    let direction = playback_rate.signum();
    let magnitude = playback_rate.abs();
    let lock_distance = (magnitude - config.lock_center_rate).abs();
    let lock_amount = (-(lock_distance / config.lock_width).powi(2)).exp() * config.lock_strength;
    let stabilized = magnitude + (config.lock_center_rate - magnitude) * lock_amount;
    (direction * stabilized).clamp(-config.max_playback_rate, config.max_playback_rate)
}

#[cfg(test)]
mod scratch_tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn monotone_mapping_round_trips() {
        let interpolant =
            MonotoneInterpolant::new(vec![0.0, 100.0, 200.0, 300.0], vec![0.0, 0.2, 0.8, 1.0])
                .unwrap();
        for sample in [0.0, 25.0, 100.0, 175.0, 250.0, 300.0] {
            let radial = interpolant.evaluate(sample);
            assert_abs_diff_eq!(interpolant.evaluate_inverse(radial), sample, epsilon = 1e-8);
        }
    }

    #[test]
    fn programme_gap_calibration_pins_both_visible_edges_and_round_trips() {
        let calibration = StylusCalibration::try_from_programme_map(ProgrammeCalibrationMap {
            total_samples: 9_000_000.0,
            gaps: vec![ProgrammeCalibrationGap {
                start_sample: 4_000_000.0,
                end_sample: 4_096_000.0,
                radial_start_normalized: 0.421,
                radial_end_normalized: 0.429,
            }],
        })
        .unwrap();

        assert!(calibration.has_gaps());
        assert_abs_diff_eq!(
            calibration.sample_to_groove(4_000_000.0),
            0.421,
            epsilon = 1e-12
        );
        assert_abs_diff_eq!(
            calibration.sample_to_groove(4_096_000.0),
            0.429,
            epsilon = 1e-12
        );
        for sample in [0.0, 1_000_000.0, 4_000_000.0, 4_048_000.0, 8_000_000.0] {
            let groove = calibration.sample_to_groove(sample);
            assert_abs_diff_eq!(calibration.groove_to_sample(groove), sample, epsilon = 1e-4);
        }
        let gap_midpoint = calibration.groove_to_sample(0.425);
        assert!((4_000_000.0..=4_096_000.0).contains(&gap_midpoint));
    }

    #[test]
    fn programme_gap_calibration_rejects_overlapping_or_flat_anchors() {
        let overlapping = StylusCalibration::try_from_programme_map(ProgrammeCalibrationMap {
            total_samples: 10_000.0,
            gaps: vec![
                ProgrammeCalibrationGap {
                    start_sample: 2_000.0,
                    end_sample: 3_000.0,
                    radial_start_normalized: 0.2,
                    radial_end_normalized: 0.3,
                },
                ProgrammeCalibrationGap {
                    start_sample: 2_500.0,
                    end_sample: 4_000.0,
                    radial_start_normalized: 0.4,
                    radial_end_normalized: 0.5,
                },
            ],
        });
        assert_eq!(
            overlapping.err().unwrap(),
            "gap 1: sample regions must not overlap"
        );

        let flat = StylusCalibration::try_from_programme_map(ProgrammeCalibrationMap {
            total_samples: 10_000.0,
            gaps: vec![ProgrammeCalibrationGap {
                start_sample: 2_000.0,
                end_sample: 3_000.0,
                radial_start_normalized: 0.2,
                radial_end_normalized: 0.2,
            }],
        });
        assert_eq!(
            flat.err().unwrap(),
            "gap 0: requires 0 < radialStartNormalized < radialEndNormalized <= 1"
        );
    }

    #[test]
    fn playback_rate_deadzone_and_lock_are_preserved() {
        let config = ScratchConfig {
            max_playback_rate: 4.0,
            deadzone_rate: 0.02,
            lock_center_rate: 1.0,
            lock_width: 0.1,
            lock_strength: 0.5,
            pointer_filter_seconds: 0.035,
        };
        assert_eq!(map_physical_playback_rate(0.01, config), 0.0);
        assert_abs_diff_eq!(
            map_physical_playback_rate(1.0, config),
            1.0,
            epsilon = 1e-12
        );
        assert_eq!(map_physical_playback_rate(10.0, config), 4.0);
    }
}
