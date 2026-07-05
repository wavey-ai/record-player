mod acoustic;
pub use acoustic::{AcousticConfig, AcousticStatus, ScratchAcousticDsp};

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

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
        let widths = xs.windows(2).map(|pair| pair[1] - pair[0]).collect::<Vec<_>>();
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
            last.checked_sub(2).and_then(|index| widths.get(index).copied()),
            deltas[last - 1],
            last.checked_sub(2).and_then(|index| deltas.get(index).copied()),
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
        self.xs.partition_point(|candidate| *candidate <= value).saturating_sub(1)
    }

    fn segment_for_y(&self, value: f64) -> usize {
        if value <= self.ys[0] {
            return 0;
        }
        if value >= self.ys[self.ys.len() - 1] {
            return self.ys.len() - 2;
        }
        self.ys.partition_point(|candidate| *candidate <= value).saturating_sub(1)
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
            .map(|interpolant| interpolant.evaluate_inverse(groove).clamp(0.0, self.total_samples))
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
            self.filtered_pointer_rate +=
                (raw_playback_rate - self.filtered_pointer_rate) * alpha;
            physical_playback_rate = map_physical_playback_rate(self.filtered_pointer_rate, self.config);
            self.sample_position = (self.current_time * sample_rate).clamp(0.0, duration * sample_rate);
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
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn monotone_mapping_round_trips() {
        let interpolant = MonotoneInterpolant::new(
            vec![0.0, 100.0, 200.0, 300.0],
            vec![0.0, 0.2, 0.8, 1.0],
        )
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
        assert_abs_diff_eq!(map_physical_playback_rate(1.0, config), 1.0, epsilon = 1e-12);
        assert_eq!(map_physical_playback_rate(10.0, config), 4.0);
    }
}
