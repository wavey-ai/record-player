use js_sys::{Array, Float32Array, Int16Array};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::{
    resampler::adaptive_sample,
    scratch_gate::{ScratchGate, ScratchPreset},
};

const OUTPUT_GAIN: f64 = 1.0;
const RATE_SPRING_OMEGA: f64 = 70.0;
const RATE_SPRING_ZETA: f64 = 0.85;
const POSITION_CATCHUP_SECONDS: f64 = 0.28;
const MOTION_HOLD_SECONDS: f64 = 0.05;
const MOTION_HOLD_RELEASE_SECONDS: f64 = 0.06;
const GRIP_ATTACK_SECONDS: f64 = 0.012;
const GRIP_RELEASE_SECONDS: f64 = 0.045;
const MOTOR_SPINUP_SECONDS: f64 = 0.3;
const MOTOR_BRAKE_SECONDS: f64 = 0.32;
const GRIP_OWNERSHIP: f64 = 0.5;
const STILL_SNAP_SECONDS: f64 = 0.03;
const DEADZONE_RATE: f64 = 0.006;
const PLATTER_LOCK_CENTER_RATE: f64 = 1.0;
const PLATTER_LOCK_WIDTH: f64 = 0.42;
const PLATTER_LOCK_STRENGTH: f64 = 0.68;
const BEARING_THROW_DECAY_SECONDS: f64 = 0.85;
const DRAG_LOWPASS_MAX_HZ: f64 = 19_000.0;
const DRAG_LOWPASS_RATE_KNEE: f64 = 0.95;
const TRACING_LOSS_START_RATE: f64 = 2.5;
const WOW_REV_SECONDS: f64 = 1.8;
const FLUTTER_HZ: f64 = 6.4;
const CONTACT_NOISE_GAIN: f64 = 0.00008;
const SOURCE_TEXTURE_GAIN: f64 = 0.00018;
const DUST_FLECK_GAIN: f64 = 0.000045;
const CONTACT_IMPULSE_DECAY: f64 = 0.985;
const WINDOW_REQUEST_MARGIN_SECONDS: f64 = 0.75;
const WINDOW_REQUEST_PROJECT_SECONDS: f64 = 0.18;
const WINDOW_MISS_FADE_SECONDS: f64 = 0.006;

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
    #[serde(default = "default_true")]
    pub acoustic_enabled: bool,
    #[serde(default = "default_true")]
    pub surface_enabled: bool,
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
fn default_true() -> bool {
    true
}

impl Default for AcousticConfig {
    fn default() -> Self {
        Self {
            max_rate: default_max_rate(),
            wow_rev_seconds: default_wow_rev_seconds(),
            flutter_hz: default_flutter_hz(),
            acoustic_enabled: true,
            surface_enabled: true,
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

#[wasm_bindgen]
pub struct ScratchAcousticDsp {
    config: AcousticConfig,
    output_sample_rate: f64,
    source_sample_rate: f64,
    native_rpm: f64,
    channels: Vec<Vec<f32>>,
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
    drag_lowpass_state: Vec<f64>,
    active: bool,
    needle_lifted: bool,
    hand_contact: bool,
    grip: f64,
    motor_rate: f64,
    motor_delivered_rate: f64,
    unpowered_throw_rate: f64,
    ended: bool,
    contact_impulse: f64,
    last_effective_rate: f64,
    noise_seed: u32,
    last_noise: f64,
    last_output_samples: Vec<f64>,
    window_miss_frames: usize,
    frames_since_motion: usize,
    frames_since_window_request: usize,
    output: Vec<f32>,
    scratch_gate: ScratchGate,
    scratch_gate_trace: Vec<f32>,
    requested_window_position: Option<f64>,
    surface_asset: Vec<Vec<f32>>,
    surface_asset_rate: f64,
    surface_gain_multiplier: f64,
    surface_bed: Option<SurfaceBed>,
    needle_thump: Option<NeedleThump>,
    needle_burst: Option<SurfaceBurst>,
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
        Ok(Self::new_internal(output_sample_rate, config))
    }

    fn new_internal(output_sample_rate: f64, config: AcousticConfig) -> Self {
        let native_rpm = (60.0 / config.wow_rev_seconds.max(1e-6)).clamp(16.0, 90.0);
        Self {
            config,
            output_sample_rate,
            source_sample_rate: 48_000.0,
            native_rpm,
            channels: Vec::new(),
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
            drag_lowpass_state: Vec::new(),
            active: false,
            needle_lifted: false,
            hand_contact: false,
            grip: 0.0,
            motor_rate: 0.0,
            motor_delivered_rate: 0.0,
            unpowered_throw_rate: 0.0,
            ended: false,
            contact_impulse: 0.0,
            last_effective_rate: 0.0,
            noise_seed: 0x9e37_79b9,
            last_noise: 0.0,
            last_output_samples: Vec::new(),
            window_miss_frames: 0,
            frames_since_motion: output_sample_rate as usize,
            frames_since_window_request: output_sample_rate as usize,
            output: Vec::new(),
            scratch_gate: ScratchGate::default(),
            scratch_gate_trace: Vec::new(),
            requested_window_position: None,
            surface_asset: Vec::new(),
            surface_asset_rate: 48_000.0,
            surface_gain_multiplier: 1.0,
            surface_bed: None,
            needle_thump: None,
            needle_burst: None,
        }
    }

    #[wasm_bindgen(js_name = setWindow)]
    pub fn set_window(
        &mut self,
        channels: Array,
        source_sample_rate: f64,
        window_start: u32,
        total_frames: u32,
        reset_position: Option<f64>,
    ) -> Result<(), JsValue> {
        if !source_sample_rate.is_finite() || source_sample_rate <= 0.0 {
            return Err(JsValue::from_str("sourceSampleRate must be positive"));
        }
        let mut copied_channels = Vec::with_capacity(channels.length() as usize);
        for value in channels.iter() {
            if !value.is_instance_of::<Float32Array>() {
                return Err(JsValue::from_str(
                    "source channels must be Float32Array values",
                ));
            }
            let typed = Float32Array::new(&value);
            let mut samples = vec![0.0_f32; typed.length() as usize];
            typed.copy_to(&mut samples);
            copied_channels.push(samples);
        }
        if copied_channels.is_empty() || copied_channels[0].is_empty() {
            return Err(JsValue::from_str(
                "at least one non-empty source channel is required",
            ));
        }
        let length = copied_channels[0].len();
        if copied_channels
            .iter()
            .any(|channel| channel.len() != length)
        {
            return Err(JsValue::from_str("source channels must have equal lengths"));
        }
        self.source_sample_rate = source_sample_rate;
        self.window_start = window_start as usize;
        self.window_end = self.window_start.saturating_add(length);
        self.total_frames = (total_frames as usize).max(self.window_end);
        self.channels = copied_channels;
        if let Some(position) = reset_position {
            self.reset_position(position);
        }
        Ok(())
    }

    #[wasm_bindgen(js_name = startStreamWindow)]
    pub fn start_stream_window(
        &mut self,
        channel_count: u32,
        source_sample_rate: f64,
        total_frames: u32,
    ) -> Result<(), JsValue> {
        if !source_sample_rate.is_finite() || source_sample_rate <= 0.0 {
            return Err(JsValue::from_str("sourceSampleRate must be positive"));
        }
        let channel_count = channel_count.max(1) as usize;
        let total_frames = total_frames.max(1) as usize;
        self.source_sample_rate = source_sample_rate;
        self.window_start = 0;
        self.window_end = 0;
        self.total_frames = total_frames;
        self.channels = (0..channel_count)
            .map(|_| vec![0.0_f32; total_frames])
            .collect();
        self.reset_position(0.0);
        Ok(())
    }

    #[wasm_bindgen(js_name = appendPcmI16)]
    pub fn append_pcm_i16(
        &mut self,
        channel_buffers: Array,
        start_frame: u32,
        end_frame: u32,
    ) -> Result<(), JsValue> {
        let start_frame = start_frame as usize;
        let end_frame = end_frame as usize;
        if end_frame <= start_frame {
            return Err(JsValue::from_str(
                "endFrame must be greater than startFrame",
            ));
        }
        if self.channels.is_empty() {
            return Err(JsValue::from_str("stream window has not been initialised"));
        }
        if channel_buffers.length() as usize != self.channels.len() {
            return Err(JsValue::from_str(
                "PCM channel count does not match stream window",
            ));
        }
        if end_frame > self.total_frames {
            return Err(JsValue::from_str(
                "PCM segment exceeds stream window length",
            ));
        }
        let frame_count = end_frame - start_frame;
        for (channel_index, value) in channel_buffers.iter().enumerate() {
            if !value.is_instance_of::<Int16Array>() {
                return Err(JsValue::from_str(
                    "PCM channel buffers must be Int16Array values",
                ));
            }
            let typed = Int16Array::new(&value);
            if typed.length() as usize != frame_count {
                return Err(JsValue::from_str(
                    "PCM channel buffer length does not match frame range",
                ));
            }
            let mut samples = vec![0_i16; frame_count];
            typed.copy_to(&mut samples);
            let channel = &mut self.channels[channel_index];
            for (offset, sample) in samples.into_iter().enumerate() {
                channel[start_frame + offset] = sample as f32 / 32768.0;
            }
        }
        self.window_start = 0;
        self.window_end = self.window_end.max(end_frame);
        Ok(())
    }

    #[wasm_bindgen(js_name = clearWindow)]
    pub fn clear_window(&mut self) {
        self.channels.clear();
        self.total_frames = 0;
        self.window_start = 0;
        self.window_end = 0;
        self.reset_position(0.0);
    }

    #[wasm_bindgen(js_name = start)]
    pub fn start(&mut self) {
        self.active = true;
        self.grip = 0.0;
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
        self.frames_since_motion = 0;
        self.contact_impulse = 0.0;
        self.last_output_samples.clear();
        self.window_miss_frames = 0;
        self.ended = false;
    }

    #[wasm_bindgen(js_name = stop)]
    pub fn stop(&mut self) {
        self.active = false;
        self.hand_contact = false;
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
        }
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

    #[wasm_bindgen(getter, js_name = scratchPreset)]
    pub fn scratch_preset(&self) -> String {
        self.scratch_gate.preset().as_str().to_owned()
    }

    #[wasm_bindgen(getter, js_name = scratchClicks)]
    pub fn scratch_clicks(&self) -> u8 {
        self.scratch_gate.clicks()
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

    #[wasm_bindgen(js_name = setNativeRpm)]
    pub fn set_native_rpm(&mut self, native_rpm: f64) -> Result<(), JsValue> {
        if !native_rpm.is_finite() || native_rpm <= 0.0 {
            return Err(JsValue::from_str("nativeRpm must be positive"));
        }
        self.native_rpm = native_rpm.clamp(16.0, 90.0);
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = nativeRpm)]
    pub fn native_rpm(&self) -> f64 {
        self.native_rpm
    }

    #[wasm_bindgen(js_name = setMotion)]
    pub fn set_motion(&mut self, position: f64, rate: f64, impulse: f64) {
        self.target_position = self.clamp_source_position(position);
        self.target_rate = self.map_rate(rate);
        self.frames_since_motion = 0;
        if impulse > 0.0 {
            self.contact_impulse = self.contact_impulse.max(impulse).clamp(0.0, 1.0);
        }
    }

    #[wasm_bindgen(js_name = setTransport)]
    pub fn set_transport(&mut self, hand_contact: bool, motor_rate: f64, hand_rate: f64) {
        let released_hand = self.hand_contact && !hand_contact;
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
        self.position = self.clamp_source_position(position);
        self.target_position = self.position;
        self.last_output_samples.clear();
        self.window_miss_frames = 0;
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
    pub fn render(&mut self, frame_count: u32, output_channel_count: u32) {
        let frame_count = frame_count as usize;
        let output_channel_count = (output_channel_count as usize).clamp(1, 2);
        self.output
            .resize(frame_count.saturating_mul(output_channel_count), 0.0);
        self.output.fill(0.0);
        self.scratch_gate_trace.resize(frame_count, 1.0);
        self.requested_window_position = None;
        if frame_count == 0 {
            return;
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
            self.apply_scratch_gate_trace(frame_count, output_channel_count);
            return;
        }
        self.drag_lowpass_state.resize(output_channel_count, 0.0);
        self.last_output_samples.resize(output_channel_count, 0.0);
        let dt = 1.0 / self.output_sample_rate;
        let catchup_frames = (self.source_sample_rate * POSITION_CATCHUP_SECONDS).max(1.0);
        let hold_frames = (self.output_sample_rate * MOTION_HOLD_SECONDS).max(1.0) as usize;
        let hold_release_frames = (self.output_sample_rate * MOTION_HOLD_RELEASE_SECONDS).max(1.0);
        let still_snap_alpha = 1.0 - (-1.0 / (self.output_sample_rate * STILL_SNAP_SECONDS)).exp();
        let grip_target = if self.hand_contact { 1.0 } else { 0.0 };
        let grip_seconds = if self.hand_contact {
            GRIP_ATTACK_SECONDS
        } else {
            GRIP_RELEASE_SECONDS
        };
        let grip_alpha = 1.0 - (-1.0 / (self.output_sample_rate * grip_seconds)).exp();
        let motor_spin_alpha =
            1.0 - (-1.0 / (self.output_sample_rate * MOTOR_SPINUP_SECONDS)).exp();
        let motor_brake_step = 1.0 / (self.output_sample_rate * MOTOR_BRAKE_SECONDS);
        let rate_scale = self.source_sample_rate / self.output_sample_rate;
        let miss_fade_frames = (self.output_sample_rate * WINDOW_MISS_FADE_SECONDS)
            .round()
            .max(1.0);

        for frame in 0..frame_count {
            self.frames_since_motion = self.frames_since_motion.saturating_add(1);
            self.grip += (grip_target - self.grip) * grip_alpha;
            if self.motor_rate.abs() > self.motor_delivered_rate.abs() {
                self.motor_delivered_rate +=
                    (self.motor_rate - self.motor_delivered_rate) * motor_spin_alpha;
            } else if self.motor_delivered_rate > self.motor_rate {
                self.motor_delivered_rate =
                    (self.motor_delivered_rate - motor_brake_step).max(self.motor_rate);
            } else {
                self.motor_delivered_rate =
                    (self.motor_delivered_rate + motor_brake_step).min(self.motor_rate);
            }
            if !self.hand_contact
                && self.motor_rate.abs() < DEADZONE_RATE
                && self.unpowered_throw_rate.abs() >= DEADZONE_RATE
            {
                self.unpowered_throw_rate *= (-dt / BEARING_THROW_DECAY_SECONDS).exp();
                if self.unpowered_throw_rate.abs() < DEADZONE_RATE {
                    self.unpowered_throw_rate = 0.0;
                }
            }
            let free_platter_rate = if !self.hand_contact
                && self.motor_rate.abs() < DEADZONE_RATE
                && self.unpowered_throw_rate.abs() >= DEADZONE_RATE
            {
                self.unpowered_throw_rate
            } else {
                self.motor_delivered_rate
            };
            let hand_rate = if self.frames_since_motion > hold_frames {
                self.target_rate
                    * (-((self.frames_since_motion - hold_frames) as f64) / hold_release_frames)
                        .exp()
            } else {
                self.target_rate
            };
            let held_target_rate = free_platter_rate + self.grip * (hand_rate - free_platter_rate);
            self.rate_velocity +=
                (((held_target_rate - self.rate) * RATE_SPRING_OMEGA * RATE_SPRING_OMEGA)
                    - (2.0 * RATE_SPRING_ZETA * RATE_SPRING_OMEGA * self.rate_velocity))
                    * dt;
            self.rate += self.rate_velocity * dt;
            let position_error = self.target_position - self.position;
            let mut correction_rate =
                ((position_error / catchup_frames) * self.grip).clamp(-0.12, 0.12);
            if self.grip > GRIP_OWNERSHIP
                && hand_rate.abs() < DEADZONE_RATE
                && self.rate.abs() < DEADZONE_RATE
            {
                self.position += position_error * still_snap_alpha;
                correction_rate = 0.0;
            }
            let corrected_rate = self.rate + correction_rate;
            let abs_rate = corrected_rate.abs();
            let effective_rate = if self.config.acoustic_enabled {
                corrected_rate
                    + sign_nonzero(corrected_rate, held_target_rate)
                        * self.advance_wow_flutter(corrected_rate, rate_scale, abs_rate)
            } else {
                corrected_rate
            };
            self.scratch_gate_trace[frame] =
                self.scratch_gate
                    .process(dt, self.hand_contact, hand_rate, effective_rate)
                    as f32;
            let movement_gain = compute_movement_gain(abs_rate);
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
            let contact_noise_gain = compute_contact_noise_gain(abs_rate) + acceleration_noise;
            let impulse_noise = if self.config.surface_enabled && self.contact_impulse > 0.0001 {
                self.next_noise() * self.contact_impulse * 0.004
            } else {
                0.0
            };
            let groove_surface = if self.config.surface_enabled {
                self.compute_position_surface_noise(self.position, abs_rate)
            } else {
                0.0
            };
            let source_texture_gain = if self.config.acoustic_enabled {
                compute_source_texture_gain(abs_rate, rate_delta)
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
            let source_direction = sign_nonzero(effective_rate, held_target_rate);
            let drag_alpha = if self.config.acoustic_enabled {
                self.drag_lowpass_alpha(abs_rate)
            } else {
                1.0
            };
            let miss_fade = if self.window_miss_frames > 0 {
                (1.0 - self.window_miss_frames as f64 / miss_fade_frames).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let mut missed_window = false;

            for channel_index in 0..output_channel_count {
                let output_index = frame * output_channel_count + channel_index;
                if self.needle_lifted {
                    self.output[output_index] = 0.0;
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
                        (self.last_output_samples[channel_index] * miss_fade, 0.0)
                    }
                    Some((sampled, slope, curvature)) => {
                        let drag_state = self.drag_lowpass_state[channel_index];
                        let filtered = drag_state + (sampled - drag_state) * drag_alpha;
                        self.drag_lowpass_state[channel_index] = filtered;
                        let music = filtered * movement_gain * OUTPUT_GAIN;
                        self.last_output_samples[channel_index] = music;
                        let texture = ((slope * 0.48 + curvature * 0.86) * source_direction)
                            .clamp(-1.0, 1.0)
                            * source_texture_gain;
                        (music, texture)
                    }
                };
                self.output[output_index] =
                    (music + source_texture + contact_texture + dust_fleck + impulse_noise)
                        .clamp(-1.0, 1.0) as f32;
            }

            self.position = self.clamp_source_position(self.position + effective_rate * rate_scale);
            if self.grip < GRIP_OWNERSHIP {
                self.target_position = self.position;
                let physical_surface_region_active = self.surface_bed.is_some();
                if !physical_surface_region_active
                    && !self.ended
                    && self.motor_rate > 0.0
                    && self.position >= self.total_frames.saturating_sub(3) as f64
                {
                    self.ended = true;
                    self.motor_rate = 0.0;
                }
            }
            self.last_effective_rate = effective_rate;
            self.contact_impulse *= CONTACT_IMPULSE_DECAY;
            self.window_miss_frames = if missed_window {
                self.window_miss_frames.saturating_add(1)
            } else {
                0
            };
        }
        self.mix_foley(frame_count, output_channel_count);
        self.apply_scratch_gate_trace(frame_count, output_channel_count);
        self.maybe_request_window(frame_count);
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
        for frame in 0..frame_count {
            let fade = (1.0 - self.window_miss_frames as f64 / fade_frames).clamp(0.0, 1.0);
            for channel_index in 0..output_channel_count {
                self.output[frame * output_channel_count + channel_index] =
                    (self.last_output_samples[channel_index] * fade) as f32;
            }
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
        self.apply_scratch_gate_trace(frame_count, output_channel_count);
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
        let motor_spin_alpha =
            1.0 - (-1.0 / (self.output_sample_rate * MOTOR_SPINUP_SECONDS)).exp();
        let motor_brake_step = 1.0 / (self.output_sample_rate * MOTOR_BRAKE_SECONDS);
        let rate_scale = self.source_sample_rate / self.output_sample_rate;
        for frame in 0..frame_count {
            if self.motor_rate.abs() > self.motor_delivered_rate.abs() {
                self.motor_delivered_rate +=
                    (self.motor_rate - self.motor_delivered_rate) * motor_spin_alpha;
            } else if self.motor_delivered_rate > self.motor_rate {
                self.motor_delivered_rate =
                    (self.motor_delivered_rate - motor_brake_step).max(self.motor_rate);
            } else {
                self.motor_delivered_rate =
                    (self.motor_delivered_rate + motor_brake_step).min(self.motor_rate);
            }
            self.rate_velocity +=
                (((self.motor_delivered_rate - self.rate) * RATE_SPRING_OMEGA * RATE_SPRING_OMEGA)
                    - (2.0 * RATE_SPRING_ZETA * RATE_SPRING_OMEGA * self.rate_velocity))
                    * dt;
            self.rate += self.rate_velocity * dt;
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
        self.apply_scratch_gate_trace(frame_count, output_channel_count);
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
        self.surface_asset = copied;
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
        if !(duration_seconds > 0.0) || self.needle_lifted {
            return;
        }
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
        if self.needle_lifted {
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
}

impl ScratchAcousticDsp {
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

    fn apply_scratch_gate_trace(&mut self, frame_count: usize, output_channel_count: usize) {
        for frame in 0..frame_count {
            let gain = self.scratch_gate_trace[frame];
            for channel_index in 0..output_channel_count {
                self.output[frame * output_channel_count + channel_index] *= gain;
            }
        }
    }

    fn mix_foley(&mut self, frame_count: usize, output_channel_count: usize) {
        if self.surface_bed.is_none() && self.needle_thump.is_none() && self.needle_burst.is_none()
        {
            return;
        }
        let dt = 1.0 / self.output_sample_rate;
        let asset_step = self.surface_asset_rate / self.output_sample_rate;
        for frame in 0..frame_count {
            let mut per_channel = [0.0_f64; 2];
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
                        let raw =
                            self.surface_asset_sample(channel_index, bed.position, bed.looping);
                        let bed = self.surface_bed.as_mut().unwrap();
                        per_channel[channel_index] +=
                            bed.filters[channel_index].process(raw) * envelope;
                    }
                    let bed = self.surface_bed.as_mut().unwrap();
                    bed.position += asset_step;
                    bed.elapsed_frames += 1.0;
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
                        let raw = self.surface_asset_sample(channel_index, burst.position, false);
                        let burst = self.needle_burst.as_mut().unwrap();
                        per_channel[channel_index] +=
                            burst.filters[channel_index].process(raw) * envelope;
                    }
                    let burst = self.needle_burst.as_mut().unwrap();
                    burst.position += asset_step;
                    burst.elapsed_frames += 1.0;
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

    fn reset_position(&mut self, position: f64) {
        self.position = self.clamp_source_position(position);
        self.target_position = self.position;
        self.rate = 0.0;
        self.rate_velocity = 0.0;
        self.target_rate = 0.0;
        self.motor_delivered_rate = 0.0;
        self.unpowered_throw_rate = 0.0;
        self.last_effective_rate = 0.0;
        self.frames_since_motion = 0;
        self.last_output_samples.clear();
        self.window_miss_frames = 0;
    }

    fn map_rate(&self, rate: f64) -> f64 {
        if !rate.is_finite() || rate.abs() < DEADZONE_RATE {
            0.0
        } else {
            let direction = rate.signum();
            let magnitude = rate.abs();
            let lock_distance = (magnitude - PLATTER_LOCK_CENTER_RATE).abs();
            let lock_amount =
                (-(lock_distance / PLATTER_LOCK_WIDTH).powi(2)).exp() * PLATTER_LOCK_STRENGTH;
            let stabilized = magnitude + (PLATTER_LOCK_CENTER_RATE - magnitude) * lock_amount;
            (direction * stabilized).clamp(-self.config.max_rate, self.config.max_rate)
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
        let groove_grain = self.position_noise(position, 3.7, 0x0051_f15e);
        let groove_bed = self.position_noise(position, 37.0, 0x002d_4a11);
        (groove_grain * 0.72 + groove_bed * 0.22) * speed_weight
    }

    fn compute_dust_fleck(&self, position: f64, abs_rate: f64) -> f64 {
        if abs_rate <= 0.03 {
            return 0.0;
        }
        let cell_frames = (self.source_sample_rate * 0.12).round().max(1.0);
        let cell = (position.max(0.0) / cell_frames).floor() as i64;
        let chance = (Self::hash_noise(cell, 0x006d_2b79) + 1.0) * 0.5;
        if chance < 0.996 {
            return 0.0;
        }
        let center = (cell as f64 + 0.5 + Self::hash_noise(cell, 0x004f_1bbc) * 0.28) * cell_frames;
        let width = cell_frames * 0.028;
        let distance = (position - center).abs() / width.max(1.0);
        if distance >= 1.0 {
            return 0.0;
        }
        let envelope = (1.0 - distance).powi(2);
        let speed_weight = (abs_rate / 1.4).clamp(0.12, 1.0);
        Self::hash_noise(cell, 0x0073_c4d9) * envelope * speed_weight * DUST_FLECK_GAIN
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
        let p0 = channel[index.saturating_sub(1)] as f64;
        let p1 = channel[index] as f64;
        let p2 = channel[(index + 1).min(channel.len() - 1)] as f64;
        let p3 = channel[(index + 2).min(channel.len() - 1)] as f64;
        let a = p2 - p0;
        let b = 2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3;
        let c = 3.0 * (p1 - p2) + p3 - p0;
        let slope = 0.5 * (a + 2.0 * b * t + 3.0 * c * t * t);
        let curvature = (p0 - 2.0 * p1 + p2) * (1.0 - t) + (p1 - 2.0 * p2 + p3) * t;
        let sample = adaptive_sample(channel, local, source_step)?;
        Some((sample, slope, curvature))
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
        let depth = abs_rate.clamp(0.0, 1.2) * 0.0012;
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
        let margin =
            (WINDOW_REQUEST_MARGIN_SECONDS * self.source_sample_rate * (speed * 0.5).max(1.0))
                .max(256.0);
        let projected = self.clamp_source_position(
            self.position
                + self.last_effective_rate
                    * self.source_sample_rate
                    * WINDOW_REQUEST_PROJECT_SECONDS,
        );
        let request = if self.last_effective_rate < 0.0 {
            self.position.min(projected)
        } else {
            self.position.max(projected)
        };
        let start = self.window_start as f64;
        let end = self.window_end as f64;
        if self.position < start + margin
            || self.position > end - margin
            || request < start + margin
            || request > end - margin
        {
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

fn sign_nonzero(primary: f64, fallback: f64) -> f64 {
    if primary != 0.0 {
        primary.signum()
    } else if fallback != 0.0 {
        fallback.signum()
    } else {
        1.0
    }
}

fn compute_movement_gain(abs_rate: f64) -> f64 {
    if abs_rate <= DEADZONE_RATE {
        return 0.0;
    }
    let normalized = abs_rate.clamp(0.0, 10.0);
    let realtime_presence = (-((normalized - 1.0) / 0.38).powi(2)).exp();
    let underspeed = 0.78 + 0.22 * normalized.max(DEADZONE_RATE).powf(0.1);
    let overspeed = 1.0 + (normalized - 1.0).max(0.0) * 0.014;
    let acoustic = if normalized <= 1.0 {
        underspeed
    } else {
        overspeed
    };
    (acoustic + realtime_presence * 0.025).clamp(0.68, 1.08)
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

    fn simulation_dsp() -> ScratchAcousticDsp {
        let mut dsp = ScratchAcousticDsp::new_internal(48_000.0, AcousticConfig::default());
        dsp.source_sample_rate = 48_000.0;
        dsp.channels = vec![vec![0.0_f32; 4_800_000]];
        dsp.window_start = 0;
        dsp.window_end = 4_800_000;
        dsp.total_frames = 4_800_000;
        dsp
    }

    fn scratch_signal_dsp(preset: ScratchPreset, rate: f64) -> ScratchAcousticDsp {
        let mut dsp = ScratchAcousticDsp::new_internal(48_000.0, AcousticConfig::default());
        dsp.source_sample_rate = 48_000.0;
        dsp.channels = vec![vec![0.5_f32; 48_000]];
        dsp.window_start = 0;
        dsp.window_end = 48_000;
        dsp.total_frames = 48_000;
        dsp.set_effects(false, false);
        dsp.set_scratch_preset(preset.as_str()).unwrap();
        dsp.start();
        dsp.set_position(24_000.0, 0.0);
        dsp.set_transport(true, 0.0, rate);
        dsp.set_motion(24_000.0, rate, 0.0);
        dsp.grip = 1.0;
        dsp.rate = rate;
        dsp.rate_velocity = 0.0;
        dsp
    }

    fn output_rms(dsp: &ScratchAcousticDsp) -> f64 {
        (dsp.output
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>()
            / dsp.output.len().max(1) as f64)
            .sqrt()
    }

    // Mirrors the worklet's exact message sequence for a canvas scratch:
    // play (motor 1×), settle, hand grab, drag backwards at −1× with motion
    // updates every 16 ms. The rendered groove must follow the hand.
    #[test]
    fn hand_drag_backwards_overrides_the_motor() {
        let mut dsp = simulation_dsp();
        dsp.start();
        dsp.set_position(2_400_000.0, 0.0);
        dsp.set_transport(false, 1.0, 0.0);
        for _ in 0..375 {
            dsp.render(128, 2); // 1 s: motor reaches nominal speed
        }
        assert!(
            dsp.last_effective_rate > 0.9,
            "motor should be at speed, got {}",
            dsp.last_effective_rate
        );
        let grab_position = dsp.position;
        dsp.set_transport(true, 1.0, 0.0);
        let mut hand_position = grab_position;
        let mut min_rate = f64::MAX;
        for step in 0..60 {
            hand_position -= 768.0; // −1× for 16 ms
            dsp.set_transport(true, 1.0, -1.0);
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
        dsp.set_transport(false, 1.0, 0.0);
        dsp.render(48_000, 1);
        dsp.set_transport(true, 1.0, -1.0);
        dsp.set_motion(dsp.position - 960.0, -1.0, 0.0);
        dsp.render(960, 1);
        assert!(dsp.grip > 0.80, "20 ms grab grip was {}", dsp.grip);
    }

    #[test]
    fn hand_rate_uses_reference_deadzone_and_gaussian_one_x_lock() {
        let dsp = simulation_dsp();
        assert_eq!(dsp.map_rate(DEADZONE_RATE * 0.5), 0.0);
        assert_eq!(dsp.map_rate(1.0), 1.0);
        assert_eq!(dsp.map_rate(-1.0), -1.0);
        assert!(dsp.map_rate(0.70) > 0.70);
        assert!(dsp.map_rate(-0.70) < -0.70);
        assert_eq!(dsp.map_rate(100.0), dsp.config.max_rate);
    }

    #[test]
    fn unpowered_hand_throw_coasts_but_explicit_motor_stop_brakes() {
        let mut thrown = simulation_dsp();
        thrown.start();
        thrown.set_position(2_400_000.0, 0.0);
        thrown.set_transport(true, 0.0, 1.0);
        thrown.set_motion(thrown.position + 24_000.0, 1.0, 0.0);
        thrown.grip = 1.0;
        thrown.rate = 1.0;
        thrown.last_effective_rate = 1.0;
        thrown.set_transport(false, 0.0, 0.0);
        thrown.render(9_600, 1);
        assert!(
            thrown.last_effective_rate > 0.55,
            "bearing throw lost momentum too quickly: {}",
            thrown.last_effective_rate,
        );

        let mut braked = simulation_dsp();
        braked.start();
        braked.set_position(2_400_000.0, 0.0);
        braked.set_transport(false, 1.0, 0.0);
        braked.render(48_000, 1);
        braked.set_transport(false, 0.0, 0.0);
        braked.render(19_200, 1);
        assert!(
            braked.last_effective_rate.abs() < 0.05,
            "powered brake retained rate {}",
            braked.last_effective_rate,
        );
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
    fn surface_only_render_spins_platter_without_advancing_or_leaking_programme() {
        let mut dsp = scratch_signal_dsp(ScratchPreset::Baby, 0.0);
        dsp.set_transport(false, 1.0, 0.0);
        let programme_position = dsp.position;
        dsp.render_surface(48_000, 1);
        assert_eq!(dsp.position, programme_position);
        assert!(dsp.last_effective_rate > 0.9);
        assert_eq!(output_rms(&dsp), 0.0);
    }

    #[test]
    fn movement_gain_is_silent_in_deadzone() {
        assert_eq!(compute_movement_gain(DEADZONE_RATE * 0.5), 0.0);
    }

    #[test]
    fn movement_gain_stays_bounded() {
        for rate in [0.01, 0.1, 1.0, 3.0, 10.0] {
            let gain = compute_movement_gain(rate);
            assert!((0.0..=1.08).contains(&gain));
        }
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
    fn releasing_the_record_reopens_gate_for_motor_handoff() {
        let mut dsp = scratch_signal_dsp(ScratchPreset::Stab, -1.0);
        dsp.render(1024, 1);
        assert!(dsp.scratch_gate() < 0.01);

        dsp.set_transport(false, 1.0, 0.0);
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
