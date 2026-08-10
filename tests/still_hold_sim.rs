use record_player::{AcousticConfig, ScratchAcousticDsp};

/// The scratch-deck symptom: touch stops the record, the host's display
/// loop stalls (no repins), and 200 ms later the record edges forward,
/// then a late repin yanks it back. CUT's host never stalls, so it
/// never shows this. This reproduces the engine's share of it: what
/// does a still, gripped hand do when events stop arriving?
#[test]
fn still_hold_through_event_silence() {
    let sr = 48_000.0;
    let mut dsp = ScratchAcousticDsp::new_native(sr, AcousticConfig::default()).unwrap();
    let source = vec![0.25_f32; 48_000 * 30];
    dsp.replace_window_native(&[source.as_slice()], sr, None)
        .unwrap();
    dsp.set_effects(false, false);
    dsp.set_slipmat_response(1.0).unwrap();
    dsp.start();
    dsp.set_transport(false, 1.0, 0.0, 0.0);
    for _ in 0..(48_000 / 256) {
        dsp.render(256, 1);
    }

    // Touch lands, still finger, full grip. A few 120 Hz still events
    // arrive while the record stops, then the host stalls completely.
    let read_position = dsp.position();
    dsp.set_transport(true, 1.0, 0.0, 1.0);
    dsp.set_motion(read_position, 0.0, 0.22);
    let event_frames = (sr / 120.0) as u32;
    let mut host_position = read_position;
    for _ in 0..6 {
        dsp.render(event_frames, 1);
        dsp.set_transport(true, 1.0, 0.0, 1.0);
        dsp.set_motion(host_position, 0.0, 0.0);
    }
    let stopped_position = dsp.position();
    host_position = stopped_position;

    // 600 ms of dead host: no events at all. The finger has not moved.
    let mut creep_frames: f64 = 0.0;
    let mut peak_rate: f64 = 0.0;
    let mut last = stopped_position;
    for step in 1..=72 {
        dsp.render((sr * 0.00833) as u32, 1);
        let position = dsp.position();
        let rate = (position - last) / (sr * 0.00833);
        peak_rate = peak_rate.max(rate.abs());
        creep_frames += position - last;
        if step % 12 == 0 {
            println!(
                "t={:5.0}ms creep={:7.1} frames ({:5.1} ms audio) rate={:7.4}",
                step as f64 * 8.33,
                creep_frames,
                creep_frames / 48.0,
                rate
            );
        }
        last = position;
    }

    // The stalled host wakes and repins the hold at its old frame.
    dsp.set_transport(true, 1.0, 0.0, 1.0);
    dsp.set_motion(host_position, 0.0, 0.0);
    let before_repin = dsp.position();
    let mut yank_frames: f64 = 0.0;
    let mut last = before_repin;
    for _ in 1..=24 {
        dsp.render((sr * 0.00833) as u32, 1);
        let position = dsp.position();
        if position < last {
            yank_frames += last - position;
        }
        last = position;
    }
    println!(
        "silence creep: {:.1} frames ({:.1} ms of audio), peak rate {:.4}",
        creep_frames,
        creep_frames / 48.0,
        peak_rate
    );
    println!(
        "late repin yanked back {:.1} frames ({:.1} ms of audio)",
        yank_frames,
        yank_frames / 48.0
    );

    assert!(
        creep_frames.abs() <= 96.0,
        "a still gripped hand let the record creep {creep_frames} frames during event silence"
    );
    assert!(
        yank_frames <= 96.0,
        "a late repin rewound {yank_frames} frames"
    );
}
