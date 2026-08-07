use record_player::{AcousticConfig, ScratchAcousticDsp};

/// Simulates: record playing at rate 1, finger lands with full grip,
/// dwells briefly, then drags backward with touch events at 120 Hz.
/// Measures how long after the backward drag begins the rendered
/// position actually moves backward, and how far the record kept
/// creeping forward after contact.
fn simulate_spinback(
    grip: f64,
    slipmat_response: f64,
    dwell_seconds: f64,
    finger_rate: f64,
    reported_rate_lags: bool,
) {
    let sr = 48_000.0;
    let mut dsp = ScratchAcousticDsp::new_native(sr, AcousticConfig::default()).unwrap();
    let source = vec![0.25_f32; 48_000 * 30];
    dsp.replace_window_native(&[source.as_slice()], sr, None)
        .unwrap();
    dsp.set_effects(false, false);
    let _ = slipmat_response; // pre-feel-slider revs: fixed coupling
    dsp.start();
    dsp.set_transport(false, 1.0, 0.0, 0.0);
    for _ in 0..(48_000 / 256) {
        dsp.render(256, 1);
    }

    // Touch lands (15 ms stale read like the app), finger still.
    let read_position = dsp.position();
    dsp.render((sr * 0.015) as u32, 1);
    dsp.set_transport(true, 1.0, 0.0, grip);
    dsp.set_motion(read_position, 0.0, 0.22);

    // Dwell: finger holds still, host keeps sending still events at 120 Hz.
    let event_frames = (sr / 120.0) as u32;
    let dwell_events = (dwell_seconds * 120.0).round() as usize;
    let mut host_position = read_position;
    for _ in 0..dwell_events {
        dsp.render(event_frames, 1);
        dsp.set_transport(true, 1.0, 0.0, grip);
        dsp.set_motion(host_position, 0.0, 0.0);
    }

    let contact_position = dsp.position();
    println!(
        "grip={grip} slipmat={slipmat_response} dwell={dwell_seconds}s finger_rate={finger_rate} lag={reported_rate_lags}"
    );
    println!(
        "  forward creep after contact: {:.0} frames ({:.1} ms of audio)",
        contact_position - read_position,
        (contact_position - read_position) / 48.0
    );

    // Backward drag begins: finger moves at -finger_rate. When
    // reported_rate_lags is set, the host's smoothed rate reads zero for
    // the first events the way a fresh rate filter does.
    let frames_per_event = finger_rate * sr / 120.0;
    let mut reversal_ms: Option<f64> = None;
    let mut half_speed_ms: Option<f64> = None;
    let mut last_position = contact_position;
    for event in 1..=120 {
        host_position -= frames_per_event;
        let reported_rate = if reported_rate_lags && event <= 12 {
            -finger_rate * f64::from(event) / 12.0
        } else {
            -finger_rate
        };
        dsp.set_transport(true, 1.0, reported_rate, grip);
        dsp.set_motion(host_position, reported_rate, 0.0);
        dsp.render(event_frames, 1);
        let position = dsp.position();
        let rate = (position - last_position) / f64::from(event_frames);
        let ms = event as f64 * 1_000.0 / 120.0;
        if reversal_ms.is_none() && position < last_position {
            reversal_ms = Some(ms);
        }
        if half_speed_ms.is_none() && rate <= -0.5 * finger_rate {
            half_speed_ms = Some(ms);
        }
        if event <= 24 {
            println!(
                "  t={ms:6.1}ms rendered_rate={rate:8.4} pos-contact={:9.1} recoveries={}",
                position - contact_position,
                0
            );
        }
        last_position = position;
    }
    println!(
        "  => reversal after {:?} ms, half finger speed after {:?} ms\n",
        reversal_ms, half_speed_ms
    );
}

#[test]
fn spinback_latency_report() {
    // Full touch mode: grip 1. Tight slipmat. Immediate spin-back and
    // after a short hold; fast and tiny-slow gestures.
    simulate_spinback(1.0, 1.0, 0.0, 1.0, false);
    simulate_spinback(1.0, 1.0, 0.0, 1.0, true);
    simulate_spinback(1.0, 1.0, 0.0, 0.15, true);
    simulate_spinback(1.0, 1.0, 0.25, 1.0, true);
    simulate_spinback(0.45, 1.0, 0.0, 1.0, false);
    simulate_spinback(0.45, 0.5, 0.0, 1.0, false);
    simulate_spinback(0.45, 0.0, 0.0, 1.0, false);
}
