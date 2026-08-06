use record_player::{AcousticConfig, ScratchAcousticDsp};

struct StopOutcome {
    stop_ms: Option<f64>,
    backwards_frames: f64,
}

fn simulate(grip: f64, stale_seconds: f64, motor_rate_during_hold: f64) -> StopOutcome {
    let sr = 48_000.0;
    let mut dsp = ScratchAcousticDsp::new_native(sr, AcousticConfig::default()).unwrap();
    let source = vec![0.25_f32; 48_000 * 30];
    dsp.replace_window_native(&[source.as_slice()], sr, None)
        .unwrap();
    dsp.set_effects(false, false);
    dsp.start();
    dsp.set_transport(false, 1.0, 0.0, 0.0);

    // Let the deck lock at rate 1.
    for _ in 0..(48_000 / 256) {
        dsp.render(256, 1);
    }

    // Swift reads the playhead here...
    let read_position = dsp.position();

    // ...but the engine keeps rendering while the message is in flight.
    let stale_frames = (sr * stale_seconds) as usize;
    if stale_frames > 0 {
        dsp.render(stale_frames as u32, 1);
    }

    // Touch lands: hand contact, hand rate 0.
    dsp.set_transport(true, motor_rate_during_hold, 0.0, grip);
    dsp.set_motion(read_position, 0.0, 0.22);

    let grab_position = dsp.position();
    let mut minimum_position = grab_position;
    let mut last_position = grab_position;
    let mut stop_ms: Option<f64> = None;
    let mut backwards_frames: f64 = 0.0;
    let block = 240; // 5 ms
    for step in 1..=400 {
        dsp.render(block, 1);
        let position = dsp.position();
        let rate = (position - last_position) / block as f64;
        let ms = step as f64 * 5.0;
        if position < last_position {
            backwards_frames += last_position - position;
        }
        minimum_position = minimum_position.min(position);
        if stop_ms.is_none() && rate <= 0.0 {
            stop_ms = Some(ms);
        }
        if step <= 40 || step % 40 == 0 {
            println!(
                "t={ms:7.1}ms rate={rate:8.4} pos-grab={:9.1} recoveries={}",
                position - grab_position,
                dsp.deck_recovery_count()
            );
        }
        last_position = position;
    }
    println!(
        "grip={grip} stale={}ms motor={motor_rate_during_hold} -> stop~{:?}ms, backwards total {:.0} frames ({:.1} ms of audio), min pos-grab {:.0}",
        stale_seconds * 1_000.0,
        stop_ms,
        backwards_frames,
        backwards_frames / 48.0,
        minimum_position - grab_position
    );
    StopOutcome {
        stop_ms,
        backwards_frames,
    }
}

#[test]
fn touch_stop_full_grip() {
    println!("== grip 1.0, stale 15ms, motor 1 ==");
    let outcome = simulate(1.0, 0.015, 1.0);
    let stop_ms = outcome.stop_ms.expect("a full-grip touch must stop the record");
    assert!(
        stop_ms <= 60.0,
        "a full-grip touch took {stop_ms}ms to stop the record"
    );
    assert!(
        outcome.backwards_frames <= 480.0,
        "a full-grip hold rewound {} frames",
        outcome.backwards_frames
    );
}

#[test]
fn touch_stop_light_grip() {
    println!("== grip 0.45, stale 15ms, motor 1 ==");
    let outcome = simulate(0.45, 0.015, 1.0);
    assert!(
        outcome.backwards_frames <= 480.0,
        "a light-grip hold rewound {} frames",
        outcome.backwards_frames
    );
}

#[test]
fn mechanics_only_touch_stop() {
    use record_player::{
        DeckMechanicalControl, DeckMechanicalState, MotorMode, NormalizedDeckControl,
        PhysicalDeckConfig,
    };
    let mut config = PhysicalDeckConfig::high_torque_dj_seed();
    config.integration_hz = 192_000.0;
    let mut state = DeckMechanicalState::new(config).unwrap();
    state.reset(1.0, 1.0, 0.0, 0.0).unwrap();
    let dt = 1.0 / 48_000.0;
    for step in 1..=(48_000 / 4) {
        let control = DeckMechanicalControl::from_normalized(
            state.config(),
            NormalizedDeckControl {
                motor_mode: MotorMode::Servo,
                motor_rate: 1.0,
                hand_contact: true,
                hand_target_angle_turns: None,
                hand_rate: 0.0,
                grip: 1.0,
                stylus_torque_nm: 0.0,
            },
        );
        let telemetry = state.advance(dt, control).unwrap();
        if step % 480 == 0 {
            println!(
                "t={:6.1}ms record_rate={:8.4} platter_rate={:8.4}",
                step as f64 * dt * 1_000.0,
                telemetry.record_rate,
                telemetry.platter_rate
            );
        }
        if telemetry.record_rate.abs() < 0.005 {
            println!("stopped at {:.1}ms", step as f64 * dt * 1_000.0);
            return;
        }
    }
    println!("did not stop within 250ms");
}
