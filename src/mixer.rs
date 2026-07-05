pub fn sharp_crossfader_gains(value: f32, width: f32) -> (f32, f32) {
    let x = value.clamp(0.0, 1.0);
    let width = width.clamp(0.0001, 1.0);
    ((x / width).clamp(0.0, 1.0), ((1.0 - x) / width).clamp(0.0, 1.0))
}
