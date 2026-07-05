use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

const OUTPUT_GAIN: f64 = 1.0;
const RATE_SPRING_OMEGA: f64 = 70.0;
const RATE_SPRING_ZETA: f64 = 0.85;
const POSITION_CATCHUP_SECONDS: f64 = 0.28;
const MOTION_HOLD_SECONDS: f64 = 0.05;
const MOTION_HOLD_RELEASE_SECONDS: f64 = 0.06;
const GRIP_ATTACK_SECONDS: f64 = 0.1;
const GRIP_RELEASE_SECONDS: f64 = 0.045;
const MOTOR_SPINUP_SECONDS: f64 = 0.3;
const MOTOR_BRAKE_SECONDS: f64 = 0.32;
const GRIP_OWNERSHIP: f64 = 0.5;
const STILL_SNAP_SECONDS: f64 = 0.03;
const DEADZONE_RATE: f64 = 0.006;
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

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcousticConfig {
    #[serde(default = "default_max_rate")]
    pub max_rate: f64,
    #[serde(default = "default_wow_rev_seconds")]
    pub wow_rev_seconds: f64,
    #[serde(default = "default_flutter_hz")]
    pub flutter_hz: f64,
}

fn default_max_rate() -> f64 { 10.0 }
fn default_wow_rev_seconds() -> f64 { WOW_REV_SECONDS }
fn default_flutter_hz() -> f64 { FLUTTER_HZ }

impl Default for AcousticConfig {
    fn default() -> Self {
        Self {
            max_rate: default_max_rate(),
            wow_rev_seconds: default_wow_rev_seconds(),
            flutter_hz: default_flutter_hz(),
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
    requested_window_position: Option<f64>,
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
        Ok(Self {
            config,
            output_sample_rate,
            source_sample_rate: 48_000.0,
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
            requested_window_position: None,
        })
    }

    #[wasm_bindgen(js_name = setWindow)]
    pub fn set_window(
        &mut self,
        channels: JsValue,
        source_sample_rate: f64,
        window_start: u32,
        total_frames: u32,
        reset_position: Option<f64>,
    ) -> Result<(), JsValue> {
        if !source_sample_rate.is_finite() || source_sample_rate <= 0.0 {
            return Err(JsValue::from_str("sourceSampleRate must be positive"));
        }
        let channels: Vec<Vec<f32>> = serde_wasm_bindgen::from_value(channels)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        if channels.is_empty() || channels[0].is_empty() {
            return Err(JsValue::from_str("at least one non-empty source channel is required"));
        }
        let length = channels[0].len();
        if channels.iter().any(|channel| channel.len() != length) {
            return Err(JsValue::from_str("source channels must have equal lengths"));
        }
        self.source_sample_rate = source_sample_rate;
        self.window_start = window_start as usize;
        self.window_end = self.window_start.saturating_add(length);
        self.total_frames = (total_frames as usize).max(self.window_end);
        self.channels = channels;
        if let Some(position) = reset_position {
            self.reset_position(position);
        }
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
        self.position = self.clamp_source_position(self.position.max(self.target_position));
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
        self.target_rate = 0.0;
        self.contact_impulse = 0.0;
        self.last_effective_rate = 0.0;
    }

    #[wasm_bindgen(js_name = setNeedleLifted)]
    pub fn set_needle_lifted(&mut self, lifted: bool) {
        self.needle_lifted = lifted;
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
        self.hand_contact = hand_contact;
        self.motor_rate = finite_or_zero(motor_rate).clamp(-self.config.max_rate, self.config.max_rate);
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
        self.output.resize(frame_count.saturating_mul(output_channel_count), 0.0);
        self.output.fill(0.0);
        self.requested_window_position = None;
        if frame_count == 0 {
            return;
        }
        if !self.active || self.channels.is_empty() || self.total_frames <= 1 {
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
        let grip_seconds = if self.hand_contact { GRIP_ATTACK_SECONDS } else { GRIP_RELEASE_SECONDS };
        let grip_alpha = 1.0 - (-1.0 / (self.output_sample_rate * grip_seconds)).exp();
        let motor_spin_alpha = 1.0 - (-1.0 / (self.output_sample_rate * MOTOR_SPINUP_SECONDS)).exp();
        let motor_brake_step = 1.0 / (self.output_sample_rate * MOTOR_BRAKE_SECONDS);
        let rate_scale = self.source_sample_rate / self.output_sample_rate;
        let miss_fade_frames = (self.output_sample_rate * WINDOW_MISS_FADE_SECONDS).round().max(1.0);

        for frame in 0..frame_count {
            self.frames_since_motion = self.frames_since_motion.saturating_add(1);
            self.grip += (grip_target - self.grip) * grip_alpha;
            if self.motor_rate.abs() > self.motor_delivered_rate.abs() {
                self.motor_delivered_rate += (self.motor_rate - self.motor_delivered_rate) * motor_spin_alpha;
            } else if self.motor_delivered_rate > self.motor_rate {
                self.motor_delivered_rate = (self.motor_delivered_rate - motor_brake_step).max(self.motor_rate);
            } else {
                self.motor_delivered_rate = (self.motor_delivered_rate + motor_brake_step).min(self.motor_rate);
            }
            let hand_rate = if self.frames_since_motion > hold_frames {
                self.target_rate * (-((self.frames_since_motion - hold_frames) as f64) / hold_release_frames).exp()
            } else {
                self.target_rate
            };
            let held_target_rate = self.motor_delivered_rate + self.grip * (hand_rate - self.motor_delivered_rate);
            self.rate_velocity += (((held_target_rate - self.rate) * RATE_SPRING_OMEGA * RATE_SPRING_OMEGA)
                - (2.0 * RATE_SPRING_ZETA * RATE_SPRING_OMEGA * self.rate_velocity)) * dt;
            self.rate += self.rate_velocity * dt;
            let position_error = self.target_position - self.position;
            let mut correction_rate = ((position_error / catchup_frames) * self.grip).clamp(-0.12, 0.12);
            if self.grip > GRIP_OWNERSHIP && hand_rate.abs() < DEADZONE_RATE && self.rate.abs() < DEADZONE_RATE {
                self.position += position_error * still_snap_alpha;
                correction_rate = 0.0;
            }
            let corrected_rate = self.rate + correction_rate;
            let abs_rate = corrected_rate.abs();
            let effective_rate = corrected_rate
                + sign_nonzero(corrected_rate, held_target_rate)
                    * self.advance_wow_flutter(corrected_rate, rate_scale, abs_rate);
            let movement_gain = compute_movement_gain(abs_rate);
            let surface_noise = self.next_noise();
            let highpassed_noise = surface_noise - self.last_noise;
            self.last_noise = surface_noise;
            let near_realtime_distance = (abs_rate - 1.0).abs();
            let realtime_acceleration_dip = 1.0
                - 0.88 * (-(near_realtime_distance * near_realtime_distance) / 0.16).exp();
            let rate_delta = (corrected_rate - self.last_effective_rate).abs();
            let acceleration_noise = (rate_delta * 0.00028 * realtime_acceleration_dip).clamp(0.0, 0.0007);
            let contact_noise_gain = compute_contact_noise_gain(abs_rate) + acceleration_noise;
            let impulse_noise = if self.contact_impulse > 0.0001 {
                self.next_noise() * self.contact_impulse * 0.004
            } else {
                0.0
            };
            let groove_surface = self.compute_position_surface_noise(self.position, abs_rate);
            let source_texture_gain = compute_source_texture_gain(abs_rate, rate_delta);
            let dust_fleck = self.compute_dust_fleck(self.position, abs_rate);
            let contact_texture = (groove_surface * 0.76 + highpassed_noise * 0.18) * contact_noise_gain;
            let source_direction = sign_nonzero(effective_rate, held_target_rate);
            let drag_alpha = self.drag_lowpass_alpha(abs_rate);
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
                let detail = self.sample_channel(source_index, self.position);
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
                self.output[output_index] = (music + source_texture + contact_texture + dust_fleck + impulse_noise)
                    .clamp(-1.0, 1.0) as f32;
            }

            self.position = self.clamp_source_position(self.position + effective_rate * rate_scale);
            if self.grip < GRIP_OWNERSHIP {
                self.target_position = self.position;
                if !self.ended && self.motor_rate > 0.0 && self.position >= self.total_frames.saturating_sub(3) as f64 {
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
        self.maybe_request_window(frame_count);
    }

    #[wasm_bindgen(js_name = renderWindowMissing)]
    pub fn render_window_missing(&mut self, frame_count: u32, output_channel_count: u32) {
        let frame_count = frame_count as usize;
        let output_channel_count = (output_channel_count as usize).clamp(1, 2);
        self.output.resize(frame_count.saturating_mul(output_channel_count), 0.0);
        let fade_frames = (self.output_sample_rate * WINDOW_MISS_FADE_SECONDS).round().max(1.0);
        self.last_output_samples.resize(output_channel_count, 0.0);
        for frame in 0..frame_count {
            let fade = (1.0 - self.window_miss_frames as f64 / fade_frames).clamp(0.0, 1.0);
            for channel_index in 0..output_channel_count {
                self.output[frame * output_channel_count + channel_index] =
                    (self.last_output_samples[channel_index] * fade) as f32;
            }
            self.window_miss_frames = self.window_miss_frames.saturating_add(1);
        }
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
}

impl ScratchAcousticDsp {
    fn reset_position(&mut self, position: f64) {
        self.position = self.clamp_source_position(position);
        self.target_position = self.position;
        self.rate = 0.0;
        self.rate_velocity = 0.0;
        self.target_rate = 0.0;
        self.motor_delivered_rate = 0.0;
        self.last_effective_rate = 0.0;
        self.frames_since_motion = 0;
        self.last_output_samples.clear();
        self.window_miss_frames = 0;
    }

    fn map_rate(&self, rate: f64) -> f64 {
        if !rate.is_finite() || rate.abs() < DEADZONE_RATE {
            0.0
        } else {
            rate.clamp(-self.config.max_rate, self.config.max_rate)
        }
    }

    fn clamp_source_position(&self, position: f64) -> f64 {
        position.clamp(0.0, self.total_frames.max(self.window_end).saturating_sub(2) as f64)
    }

    fn next_noise(&mut self) -> f64 {
        self.noise_seed = self.noise_seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
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

    fn sample_channel(&self, channel_index: usize, position: f64) -> Option<(f64, f64, f64)> {
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
        let sample = p1 + 0.5 * t * (a + t * (b + t * c));
        Some((sample, slope, curvature))
    }

    fn advance_wow_flutter(&mut self, corrected_rate: f64, rate_scale: f64, abs_rate: f64) -> f64 {
        if self.source_sample_rate <= 0.0 {
            return 0.0;
        }
        let frames_per_rev = self.config.wow_rev_seconds * self.source_sample_rate;
        self.wow_phase += corrected_rate * rate_scale / frames_per_rev;
        self.flutter_phase += self.config.flutter_hz / self.output_sample_rate * abs_rate.clamp(0.0, 1.4);
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
        self.frames_since_window_request = self.frames_since_window_request.saturating_add(frame_count);
        let speed = self.last_effective_rate.abs().max(1.0);
        let throttle = if speed > 2.0 { 0.03 } else { 0.08 };
        if self.frames_since_window_request < (self.output_sample_rate * throttle) as usize || self.channels.is_empty() {
            return;
        }
        let margin = (WINDOW_REQUEST_MARGIN_SECONDS * self.source_sample_rate * (speed * 0.5).max(1.0)).max(256.0);
        let projected = self.clamp_source_position(
            self.position + self.last_effective_rate * self.source_sample_rate * WINDOW_REQUEST_PROJECT_SECONDS,
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
    if value.is_finite() { value } else { 0.0 }
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
    let acoustic = if normalized <= 1.0 { underspeed } else { overspeed };
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
        assert_eq!(ScratchAcousticDsp::hash_noise(42, 7), ScratchAcousticDsp::hash_noise(42, 7));
        assert_ne!(ScratchAcousticDsp::hash_noise(42, 7), ScratchAcousticDsp::hash_noise(43, 7));
    }
}
