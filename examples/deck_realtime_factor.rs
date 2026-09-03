use record_player::{
    DeckMechanicalControl, DeckMechanicalState, MotorMode, NormalizedDeckControl, PhysicalDeckConfig,
};
use std::time::Instant;

const OUTPUT_HZ: f64 = 48_000.0;
const NATIVE_RPM: f64 = 33.333_333_333_333_336;

fn deck_variant(integration_hz: f64, stabilization_seconds: f64, correction_rad_s: f64) -> PhysicalDeckConfig {
    let mut c = PhysicalDeckConfig::high_torque_dj_seed();
    c.nominal_rpm = NATIVE_RPM.clamp(16.0, 90.0);
    c.integration_hz = integration_hz;
    c.hand_position_stabilization_seconds = stabilization_seconds;
    c.hand_max_position_correction_rad_s = correction_rad_s;
    c
}

struct Variant {
    name: &'static str,
    config: PhysicalDeckConfig,
}

fn run_variant(v: &Variant, frames: usize, scratch_from: f64) -> f64 {
    let mut deck = DeckMechanicalState::new(v.config.clone())
        .expect("valid config");
    let nominal = v.config.nominal_angular_velocity_rad_s();
    let mut hand_angle_turns = 0.0_f64;
    let mut phase = 0.0_f64;
    let start = Instant::now();
    for frame in 0..frames {
        let t = frame as f64 / OUTPUT_HZ;
        let scratching = t >= scratch_from;
        let mut hand_rate = 0.0;
        let mut speed = nominal;
        let motor_mode;
        let motor_rate;
        if scratching {
            phase += 2.0 * std::f64::consts::PI * 2.0 / OUTPUT_HZ;
            hand_rate = phase.sin() * 2.0;
            speed = (phase.cos().abs() * 2.0 + 1.0) * nominal; // hand spins with the stroke
            motor_mode = MotorMode::Off;
            motor_rate = 0.0;
        } else {
            motor_mode = MotorMode::Servo;
            motor_rate = nominal;
        }
        // advance the hand's angle target by its rate (record follows hand)
        hand_angle_turns += speed * (1.0 / OUTPUT_HZ) / (60.0 / NATIVE_RPM);
        let normalized = NormalizedDeckControl {
            motor_mode,
            motor_rate,
            hand_contact: scratching,
            hand_target_angle_turns: if scratching { Some(hand_angle_turns) } else { None },
            hand_rate,
            grip: if scratching { 1.0 } else { 0.0 },
            stylus_torque_nm: 0.0,
        };
        let control = DeckMechanicalControl::from_normalized(v.config.clone(), normalized);
        deck.advance(1.0 / OUTPUT_HZ, control).expect("advance");
    }
    let elapsed = start.elapsed().as_secs_f64();
    (frames as f64 / OUTPUT_HZ) / elapsed
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let render_seconds: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let frames = (render_seconds * OUTPUT_HZ) as usize;

    let variants = [
        Variant {
            name: "baseline (loose servo, 48k)",
            config: deck_variant(OUTPUT_HZ, 0.28, 0.12 * NATIVE_RPM * std::f64::consts::TAU / 60.0),
        },
        Variant {
            name: "tight servo, 48k",
            config: deck_variant(OUTPUT_HZ, 0.004, 25.0),
        },
        Variant {
            name: "tight servo + 192k (patched)",
            config: deck_variant(192_000.0, 0.004, 25.0),
        },
    ];

    println!(
        "deck-solver realtime factor over {:.1}s of audio (scratch from t=2s):",
        render_seconds
    );
    for v in &variants {
        let f = run_variant(v, frames, 2.0);
        println!("  {:>28}: {:.2}x  ({:.1}% of callback budget)", v.name, f, 100.0 / f);
    }
}
