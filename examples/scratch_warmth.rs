//! Automated "dub feel" check: gentle back-and-forth scratch warmth.
//!
//! Renders a slow sinusoidal hand motion (the kind of gentle transform a DJ
//! uses for dub feel) over a bass-rich signal and reports:
//!   - output RMS vs the physically-expected level (cartridge out ~ rate)
//!   - low-frequency energy fraction (warmth: < 250 Hz share)
//!   - reversal dropout depth (min envelope / mean envelope)
//!   - mean |rate| actually achieved vs commanded (servo tracking)
//! No listening required: numbers tell whether slow motion keeps its body.

use record_player::{AcousticConfig, ScratchAcousticDsp};

const SR: f64 = 48_000.0;

fn block() -> usize {
    std::env::var("WARMTH_BLOCK")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(128)
}
// Gentle dub motion: 0.5 Hz stroke, +-0.3 peak rate.
// Seed the read head mid-window so a multi-second drag in either direction
// never reaches a window edge, and so the hand's commanded target position
// starts *aligned* with the read head. Starting them apart leaves the hand
// position servo saturated at its correction clamp for the whole run, which
// measures the clamp rather than the deck.
const START_POS: f64 = SR * 60.0;
const STROKE_HZ: f64 = 0.5;
const PEAK_RATE: f64 = 0.3;
const SECONDS: f64 = 8.0;

fn fill_window(dsp: &mut ScratchAcousticDsp, length: usize) {
    fill_window_at(dsp, length, START_POS)
}

fn fill_window_at(dsp: &mut ScratchAcousticDsp, length: usize, start: f64) {
    // Optional real-track window: first CLI arg is a raw f32le 48 kHz mono
    // file (e.g. ffmpeg-decoded reference track). Falls back to the synth
    // bass stack when absent.
    let file_samples: Option<Vec<f32>> = std::env::args().nth(1).and_then(|path| {
        std::fs::read(&path).ok().map(|bytes| {
            bytes
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect()
        })
    });
    dsp.prepare_window(1, length as u32).expect("prepare");
    let ptr = dsp.window_channel_ptr(0);
    assert!(!ptr.is_null(), "window channel pointer must be non-null");
    unsafe {
        for i in 0..length {
            let s = if let Some(ref file) = file_samples {
                // Window index maps straight to file index, so a read head
                // seeded at N seconds really is N seconds into the track.
                file[i % file.len()] as f64 * 0.9
            } else {
                let t = i as f64 / SR;
                // Dub-like bass stack: 55 / 82.5 / 110 / 165 / 220 Hz.
                (0.45 * (2.0 * std::f64::consts::PI * 55.0 * t).sin()
                    + 0.28 * (2.0 * std::f64::consts::PI * 82.5 * t).sin()
                    + 0.20 * (2.0 * std::f64::consts::PI * 110.0 * t).sin()
                    + 0.12 * (2.0 * std::f64::consts::PI * 165.0 * t).sin()
                    + 0.08 * (2.0 * std::f64::consts::PI * 220.0 * t).sin())
                    * 0.5
            };
            std::ptr::write(ptr.add(i), s as f32);
        }
    }
    dsp.commit_window(SR, 0, length as u32, Some(start))
        .expect("commit");
    if file_samples.is_some() {
        eprintln!("window: real reference track");
    } else {
        eprintln!("window: synthetic bass stack");
    }
}

fn goertzel_power(samples: &[f32], target_hz: f64) -> f64 {
    let n = samples.len() as f64;
    let k = (0.5 + n * target_hz / SR).floor();
    let omega = 2.0 * std::f64::consts::PI * k / n;
    let (sin_o, cos_o) = omega.sin_cos();
    let coeff = 2.0 * cos_o;
    let (mut s0, mut s1, mut s2) = (0.0_f64, 0.0_f64, 0.0_f64);
    for &x in samples {
        s0 = x as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    s1 * s1 + s2 * s2 - coeff * s1 * s2
}

fn main() {
    let mode = std::env::var("WARMTH_MODE").unwrap_or_else(|_| "stroke".to_string());
    match mode.as_str() {
        "const" => run_const_drag(),
        "slip" => run_slip_sweep(),
        "brake" => run_brake(),
        "grab" => run_grab(),
        "stab" => run_stab(),
        "purity" => run_purity(),
        "centroid" => run_centroid(),
        "render" => run_render(),
        "pointer" => run_pointer(),
        "organic" => run_organic(),
        "release" => run_release(),
        _ => run_stroke(),
    }
}

fn flag(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| value != "0" && value != "false")
        .unwrap_or(default)
}

fn drive_common() -> ScratchAcousticDsp {
    drive_common_at(START_POS, 33.333_333)
}

fn drive_common_at(start: f64, rpm: f64) -> ScratchAcousticDsp {
    let mut config = AcousticConfig::default();
    config.cartridge_velocity_gain = flag("WARMTH_VELOCITY", config.cartridge_velocity_gain);
    config.riaa_speed_tilt = flag("WARMTH_TILT", config.riaa_speed_tilt);
    // The live web runs surface:true / acoustic:false, which the engine
    // default does not match. Measuring the dry default was measuring a
    // player nobody is listening to.
    config.surface_enabled = flag("WARMTH_SURFACE", config.surface_enabled);
    config.acoustic_enabled = flag("WARMTH_ACOUSTIC", config.acoustic_enabled);
    eprintln!(
        "config: cartridge_velocity_gain={} riaa_speed_tilt={}",
        config.cartridge_velocity_gain, config.riaa_speed_tilt
    );
    let mut dsp = ScratchAcousticDsp::new_native(SR, config).expect("construct");
    dsp.set_native_rpm(rpm);
    fill_window_at(&mut dsp, (SR * 180.0) as usize, start);
    dsp.start();
    dsp.set_needle_lifted(false);
    // Full-pressure test conditions: slipmat at its tightest so measured
    // slip is hand-vs-record only, not slipmat sliding.
    let slipmat: f64 = std::env::var("WARMTH_SLIPMAT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    dsp.set_slipmat_response(slipmat).expect("slipmat");
    dsp
}

fn run_stroke() {
    let mut dsp = drive_common();
    // Live-path fidelity: the host's idea of the position is stale by the
    // worklet's publish interval (1024 frames) plus two message hops, so the
    // hand's absolute anchor lands behind where the record actually is.
    // WARMTH_STALE_MS reproduces that offset; 0 = a hand that grabs the
    // record exactly where the record is.
    let stale_ms: f64 = std::env::var("WARMTH_STALE_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let stroke_motor: f64 = std::env::var("WARMTH_MOTOR")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let stroke_grip: f64 = std::env::var("WARMTH_GRIP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    eprintln!("stroke: motor {stroke_motor}, grip {stroke_grip}");
    let start_pos = dsp.position() - stale_ms * 0.001 * SR;
    let mut pos = start_pos;
    let mut out: Vec<f32> = Vec::new();
    let mut sum_abs_rate = 0.0_f64;
    let mut rate_samples = 0_u64;

    let block = block();
    let total_blocks = (SECONDS * SR) as usize / block;
    let mut cmd_rates: Vec<f64> = Vec::with_capacity(total_blocks);
    let mut ach_rates: Vec<f64> = Vec::with_capacity(total_blocks);
    for b in 0..total_blocks {
        let t = (b * block) as f64 / SR;
        let rate = (2.0 * std::f64::consts::PI * STROKE_HZ * t).sin() * PEAK_RATE;
        pos += rate * SR * (block as f64 / SR);
        sum_abs_rate += rate.abs();
        rate_samples += 1;
        dsp.set_transport(true, stroke_motor, rate, stroke_grip);
        dsp.set_motion(pos, rate, 0.0);
        dsp.render(block as u32, 2);
        cmd_rates.push(rate);
        ach_rates.push(dsp.effective_rate());
        let len = dsp.output_len() as usize;
        assert!(len >= block * 2, "output shorter than block");
        let ptr = dsp.output_ptr();
        unsafe {
            let slice = std::slice::from_raw_parts(ptr, block * 2);
            // Left channel only for metrics.
            for f in 0..block {
                out.push(slice[f * 2]);
            }
        }
    }

    let n = out.len() as f64;
    let rms = (out.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / n).sqrt();
    let mean_abs_cmd = sum_abs_rate / rate_samples as f64;

    // Warmth: share of energy below ~250 Hz (one-pole lowpass split —
    // content-agnostic, works for synth stack and real tracks alike).
    let alpha = 1.0 - (-2.0 * std::f64::consts::PI * 250.0 / SR).exp();
    let mut low = 0.0_f64;
    let (mut e_low, mut e_tot) = (0.0_f64, 0.0_f64);
    for &x in out.iter() {
        low += alpha * (x as f64 - low);
        e_low += low * low;
        e_tot += (x as f64).powi(2);
    }
    let warmth = if e_tot > 1e-12 { e_low / e_tot } else { 0.0 };

    // Reversal dropout: envelope minima vs mean over 50 ms windows.
    let win = (SR * 0.05) as usize;
    let mut env_min = f64::MAX;
    let mut env_sum = 0.0_f64;
    let mut env_n = 0_u64;
    for chunk in out.chunks(win) {
        let peak = chunk.iter().map(|x| (x.abs() as f64)).fold(0.0_f64, f64::max);
        env_min = env_min.min(peak);
        env_sum += peak;
        env_n += 1;
    }
    let dropout = env_min / (env_sum / env_n as f64).max(1e-9);

    println!("gentle dub motion: {STROKE_HZ} Hz stroke, peak rate {PEAK_RATE}");
    println!("mean |commanded rate| : {:.4}", mean_abs_cmd);
    println!("output RMS            : {:.5}", rms);
    println!("RMS / mean|rate|      : {:.5}  (physics ~ const: cartridge out scales with rate)",
        rms / mean_abs_cmd.max(1e-9));
    println!("warmth (LF share)     : {:.3}", warmth);
    println!("reversal dropout depth: {:.3}  (1.0 = no dip at turnarounds)", dropout);

    // Turnaround tracking: RMS relative error + zero-crossing lag.
    let rms_err = (cmd_rates
        .iter()
        .zip(ach_rates.iter())
        .map(|(c, a)| (c - a).powi(2))
        .sum::<f64>()
        / cmd_rates.len() as f64)
        .sqrt();
    println!("tracking RMS error    : {:.4}  (vs peak rate {})", rms_err, PEAK_RATE);
    let cmd_x: Vec<f64> = cmd_rates
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] <= 0.0 && w[1] > 0.0 || w[0] >= 0.0 && w[1] < 0.0)
        .map(|(i, _)| i as f64)
        .collect();
    let ach_x: Vec<f64> = ach_rates
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] <= 0.0 && w[1] > 0.0 || w[0] >= 0.0 && w[1] < 0.0)
        .map(|(i, _)| i as f64)
        .collect();
    if !cmd_x.is_empty() && cmd_x.len() == ach_x.len() {
        let lag_blocks: f64 =
            ach_x.iter().zip(cmd_x.iter()).map(|(a, c)| a - c).sum::<f64>() / cmd_x.len() as f64;
        println!(
            "reversal lag           : {:.2} ms ({} crossings)",
            lag_blocks * block as f64 / SR * 1000.0,
            cmd_x.len()
        );
    } else {
        println!(
            "reversal lag           : n/a (cmd {} vs achieved {} crossings)",
            cmd_x.len(),
            ach_x.len()
        );
    }
    // Post-crossing overshoot amplitude: max |achieved - commanded) in the
    // 100 ms after each commanded crossing. Large = full-swing overshoot,
    // tiny = contact chatter around the turnaround.
    let win_blocks = (0.1 * SR / block as f64).round() as usize;
    let mut overshoot = 0.0_f64;
    for &cx in cmd_x.iter() {
        let cxi = cx as usize;
        for k in 0..win_blocks {
            if cxi + k < cmd_rates.len() {
                overshoot = overshoot.max((ach_rates[cxi + k] - cmd_rates[cxi + k]).abs());
            }
        }
    }
    println!("post-cross overshoot   : {:.4} (peak rate {})", overshoot, PEAK_RATE);
}

/// Constant-velocity drag: hand pulls at a steady rate, no reversals.
/// Separates systematic hand-vs-record slip (friction loads: slipmat drag +
/// stylus drag vs hand grip) from turnaround chatter. Fresh deck per cell —
/// no state carryover between rate/grip combinations.
fn run_const_drag() {
    let block = block();
    for (rate, grip) in [(-0.25, 1.0), (-0.5, 1.0), (-1.0, 1.0), (-0.5, 0.6)] {
        let mut dsp = drive_common();
        let mut pos = dsp.position();
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        dsp.set_motion(pos, 1.0, 0.0);
        dsp.render(SR as u32, 2);
        // The record advanced during the settle; the hand takes hold where the
        // record actually is, not where it was a second ago.
        pos = dsp.position();
        let mut ach = Vec::new();
        let drag_blocks = (2.0 * SR) as usize / block;
        for _ in 0..drag_blocks {
            pos += rate * block as f64;
            dsp.set_transport(true, 1.0, rate, grip);
            dsp.set_motion(pos, rate, 0.0);
            dsp.render(block as u32, 2);
            ach.push(dsp.effective_rate());
        }
        // Skip the first 0.5 s (grab transient), measure steady slip.
        let steady = &ach[ach.len() * 1 / 4..];
        let mean = steady.iter().sum::<f64>() / steady.len() as f64;
        let min = steady.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = steady.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        println!(
            "rate {:+.2} grip {:.1}: mean {:+.4} (ratio {:.3}) band [{:+.4}, {:+.4}]",
            rate, grip, mean, mean / rate, min, max
        );
    }
}

/// Release ease-in: hold the record still against a spinning motor, let go,
/// and time the slipmat re-coupling back to full speed.
fn run_release() {
    let mut dsp = drive_common();
    let block = block();
    let mut pos = dsp.position();
    dsp.set_transport(false, 1.0, 0.0, 0.0);
    dsp.set_motion(pos, 1.0, 0.0);
    dsp.render(SR as u32, 2);
    pos = dsp.position();
    // Grab and hold still for 1 s.
    let hold_blocks = (1.0 * SR) as usize / block;
    for _ in 0..hold_blocks {
        dsp.set_transport(true, 1.0, 0.0, 1.0);
        dsp.set_motion(pos, 0.0, 0.0);
        dsp.render(block as u32, 2);
    }
    println!("held rate: {:.4}", dsp.effective_rate());
    // Release.
    dsp.set_transport(false, 1.0, 0.0, 0.0);
    let t0 = std::time::Instant::now();
    let mut t90: Option<f64> = None;
    let mut t99: Option<f64> = None;
    let rel_blocks = (3.0 * SR) as usize / block;
    for b in 0..rel_blocks {
        dsp.render(block as u32, 2);
        let r = dsp.effective_rate();
        let t = (b * block) as f64 / SR;
        if t90.is_none() && r >= 0.9 {
            t90 = Some(t);
        }
        if t99.is_none() && r >= 0.99 {
            t99 = Some(t);
        }
    }
    let _ = t0;
    println!(
        "release ease-in: 90% in {:.3}s, 99% in {:.3}s",
        t90.unwrap_or(-1.0),
        t99.unwrap_or(-1.0)
    );
}


/// Steady-state slip sweep: hand fully down at max grip, commanded a constant
/// rate, motor on or off. A real record at full grip goes exactly where the
/// hand goes -> achieved should equal commanded and the offset should be 0.
fn run_slip_sweep() {
    let block = block();
    let motor: f64 = std::env::var("WARMTH_MOTOR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    println!("motor {motor}, grip 1.0, block {block}");
    println!("{:>8} {:>10} {:>10}", "cmd", "achieved", "offset");
    for cmd in [-1.5, -1.0, -0.5, -0.25, -0.1, 0.0, 0.1, 0.25, 0.5, 1.0, 1.5] {
        let mut dsp = drive_common();
        let mut pos = dsp.position();
        // Settle at the motor state first.
        dsp.set_transport(false, motor, 0.0, 0.0);
        dsp.set_motion(pos, motor, 0.0);
        dsp.render(SR as u32, 2);
        pos = dsp.position();
        let blocks = (3.0 * SR) as usize / block;
        let mut ach = Vec::new();
        for _ in 0..blocks {
            pos += cmd * block as f64;
            dsp.set_transport(true, motor, cmd, 1.0);
            dsp.set_motion(pos, cmd, 0.0);
            dsp.render(block as u32, 2);
            ach.push(dsp.effective_rate());
        }
        let steady = &ach[ach.len() / 2..];
        let mean = steady.iter().sum::<f64>() / steady.len() as f64;
        println!("{:>8.2} {:>10.4} {:>+10.4}", cmd, mean, mean - cmd);
    }
}


/// Motor stop: how the platter actually winds down once the power is cut.
fn run_brake() {
    let mut dsp = drive_common();
    let block = 128;
    dsp.set_transport(false, 1.0, 0.0, 0.0);
    dsp.set_motion(dsp.position(), 1.0, 0.0);
    dsp.render(SR as u32, 2);
    println!("running at {:.4}", dsp.effective_rate());
    dsp.set_transport(false, 0.0, 0.0, 0.0);
    let mut elapsed_ms = 0.0;
    let mut printed = 0.0;
    for _ in 0..((2.0 * SR) as usize / block) {
        dsp.render(block as u32, 2);
        elapsed_ms += block as f64 / SR * 1000.0;
        if elapsed_ms - printed >= 40.0 {
            printed = elapsed_ms;
            println!("  {:6.0} ms  rate {:+.4}", elapsed_ms, dsp.effective_rate());
            if dsp.effective_rate().abs() < 0.006 {
                println!("  at rest after {:.0} ms", elapsed_ms);
                break;
            }
        }
    }
}


/// Interrupting a playing record: the hand lands on a platter running at 1x
/// and takes it somewhere. How long the record takes to actually obey the
/// hand is how long the programme is smeared through a pitch sweep, which is
/// what a "blurry" grab is.
fn run_grab() {
    let grip: f64 = std::env::var("WARMTH_GRIP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    println!("grip {grip}");
    for target in [0.0, -0.5, 0.5] {
        let mut dsp = drive_common();
        let block = 16;
        // Run at speed, needle down, nobody touching it.
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        dsp.set_motion(dsp.position(), 1.0, 0.0);
        dsp.render(SR as u32, 2);
        let mut pos = dsp.position();
        let entry_pos = pos;
        assert!((dsp.effective_rate() - 1.0).abs() < 0.02);

        // The hand lands and commands `target`.
        let mut settle_ms = f64::NAN;
        let mut elapsed = 0.0;
        let mut trace = Vec::new();
        for step in 0..((0.5 * SR) as usize / block) {
            pos += target * block as f64;
            dsp.set_transport(true, 1.0, target, grip);
            dsp.set_motion(pos, target, 0.0);
            dsp.render(block as u32, 2);
            elapsed = (step + 1) as f64 * block as f64 / SR * 1000.0;
            let rate = dsp.effective_rate();
            if step % 12 == 0 && trace.len() < 12 {
                trace.push((elapsed, rate));
            }
            if settle_ms.is_nan() && (rate - target).abs() <= 0.05 {
                settle_ms = elapsed;
            }
        }
        let swept = dsp.position() - entry_pos;
        println!(
            "grab -> {:+.2}: settled in {:>6} , swept {:.1} ms of programme",
            target,
            if settle_ms.is_nan() {
                ">500ms".to_string()
            } else {
                format!("{settle_ms:.0}ms")
            },
            swept / SR * 1000.0,
        );
        let points: Vec<String> = trace
            .iter()
            .map(|(ms, r)| format!("{ms:.0}ms {r:+.2}"))
            .collect();
        println!("    {}", points.join("  "));
    }
}


/// Interrupting a playing vocal and scratching it: the gesture the deck is
/// actually judged on. Reports how much of the programme survives, and how
/// much of what comes out is not programme at all.
fn run_stab() {
    let mut dsp = drive_common();
    let block = 32;
    dsp.set_transport(false, 1.0, 0.0, 0.0);
    dsp.set_motion(dsp.position(), 1.0, 0.0);
    dsp.render(SR as u32, 2);
    let mut pos = dsp.position();

    let mut out: Vec<f32> = Vec::new();
    let mut render = |dsp: &mut ScratchAcousticDsp, out: &mut Vec<f32>, blocks: usize,
                      pos: &mut f64, rate: f64, hand: bool| {
        for _ in 0..blocks {
            if hand {
                *pos += rate * block as f64;
                dsp.set_transport(true, 1.0, rate, 1.0);
                dsp.set_motion(*pos, rate, 0.0);
            }
            dsp.render(block as u32, 2);
            let ptr = dsp.output_ptr();
            unsafe {
                let slice = std::slice::from_raw_parts(ptr, block * 2);
                for f in 0..block {
                    out.push(slice[f * 2]);
                }
            }
        }
    };
    // Grab a running record and pull it back, then push it forward: a stab.
    let per = (0.12 * SR) as usize / block;
    render(&mut dsp, &mut out, per, &mut pos, -0.9, true);
    render(&mut dsp, &mut out, per, &mut pos, 0.9, true);
    render(&mut dsp, &mut out, per, &mut pos, -0.6, true);
    render(&mut dsp, &mut out, per, &mut pos, 0.6, true);

    let n = out.len() as f64;
    let rms = (out.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / n).sqrt();
    println!(
        "stab:      RMS {:.5}   presence(>2k) share {:.4}",
        rms,
        presence_share(&out)
    );

    // Baseline 1: the same deck simply playing, no hand anywhere near it.
    let mut plain = drive_common();
    plain.set_transport(false, 1.0, 0.0, 0.0);
    plain.set_motion(plain.position(), 1.0, 0.0);
    plain.render(SR as u32, 2);
    let mut played: Vec<f32> = Vec::new();
    for _ in 0..((0.48 * SR) as usize / block) {
        plain.render(block as u32, 2);
        let ptr = plain.output_ptr();
        unsafe {
            let slice = std::slice::from_raw_parts(ptr, block * 2);
            for f in 0..block {
                played.push(slice[f * 2]);
            }
        }
    }
    println!(
        "playing 1x: RMS {:.5}   presence(>2k) share {:.4}",
        (played.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / played.len() as f64).sqrt(),
        presence_share(&played)
    );

    // Baseline 2: the source window itself, untouched by the deck.
    let length = (SR * 120.0) as usize;
    let mut source = vec![0.0_f32; 0];
    let file: Option<Vec<f32>> = std::env::args().nth(1).and_then(|path| {
        std::fs::read(&path).ok().map(|bytes| {
            bytes
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect()
        })
    });
    if let Some(file) = file {
        for i in 0..(0.48 * SR) as usize {
            source.push((file[(file.len() / 2 + i) % file.len()] * 0.9) as f32);
        }
        println!(
            "source:     RMS {:.5}   presence(>2k) share {:.4}",
            (source.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / source.len() as f64).sqrt(),
            presence_share(&source)
        );
    }
    let _ = length;
}

/// Share of energy above ~2 kHz: where a vocal is understood or lost.
fn presence_share(samples: &[f32]) -> f64 {
    let alpha = 1.0 - (-2.0 * std::f64::consts::PI * 2_000.0 / SR).exp();
    let mut low = 0.0_f64;
    let (mut e_hi, mut e_tot) = (0.0_f64, 0.0_f64);
    for &x in samples {
        low += alpha * (x as f64 - low);
        let hi = x as f64 - low;
        e_hi += hi * hi;
        e_tot += (x as f64).powi(2);
    }
    if e_tot > 1e-12 { e_hi / e_tot } else { 0.0 }
}


/// Read-path fidelity, with the pitch shift factored out.
///
/// A pure tone played at rate `r` must come back as a pure tone at `r*f0`.
/// Anything else the deck puts out — interpolation error, aliases, imaging —
/// is what a scratched vocal hears as blur. Measuring a music spectrum
/// instead only measures the pitch shift, which is supposed to happen.
fn run_purity() {
    let f0 = 3_000.0_f64;
    for rate in [1.0, 0.9, 0.75, 0.6, 0.5, 0.25, 1.5, 2.0] {
        let mut dsp =
            ScratchAcousticDsp::new_native(SR, AcousticConfig::default()).expect("construct");
        let length = (SR * 4.0) as usize;
        dsp.prepare_window(1, length as u32).expect("prepare");
        let ptr = dsp.window_channel_ptr(0);
        unsafe {
            for i in 0..length {
                let t = i as f64 / SR;
                std::ptr::write(
                    ptr.add(i),
                    (0.5 * (2.0 * std::f64::consts::PI * f0 * t).sin()) as f32,
                );
            }
        }
        dsp.commit_window(SR, 0, length as u32, Some(SR * 1.0))
            .expect("commit");
        dsp.start();
        dsp.set_needle_lifted(false);
        dsp.set_slipmat_response(1.0).expect("slipmat");
        let mut pos = dsp.position();
        let block = 128;
        let mut out: Vec<f32> = Vec::new();
        for _ in 0..((1.0 * SR) as usize / block) {
            pos += rate * block as f64;
            dsp.set_transport(true, 1.0, rate, 1.0);
            dsp.set_motion(pos, rate, 0.0);
            dsp.render(block as u32, 2);
            let optr = dsp.output_ptr();
            unsafe {
                let slice = std::slice::from_raw_parts(optr, block * 2);
                for f in 0..block {
                    out.push(slice[f * 2]);
                }
            }
        }
        // Discard the grab transient, measure the steady tone.
        let tail = &out[out.len() / 2..];
        let total: f64 = tail.iter().map(|x| (*x as f64).powi(2)).sum();
        let expected_hz = f0 * dsp.effective_rate().abs();
        let signal = goertzel_power(tail, expected_hz) * 2.0 / (tail.len() as f64).powi(2)
            * tail.len() as f64;
        let purity = (signal / total.max(1e-30)).min(1.0);
        println!(
            "rate {:>4.2}: tone should sit at {:>7.1} Hz   in-band {:>6.2}%   junk {:>6.2}%",
            rate,
            expected_hz,
            purity * 100.0,
            (1.0 - purity) * 100.0
        );
    }
}


fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0_f64, 0.0_f64);
            for k in 0..len / 2 {
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (
                    re[i + k + len / 2] * cr - im[i + k + len / 2] * ci,
                    re[i + k + len / 2] * ci + im[i + k + len / 2] * cr,
                );
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr;
                im[i + k + len / 2] = ui - vi;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
            i += len;
        }
        len <<= 1;
    }
}

/// Spectral centroid of a signal, in Hz.
fn centroid(samples: &[f32]) -> f64 {
    let n = 16_384usize;
    let mut sum_num = 0.0;
    let mut sum_den = 0.0;
    let mut offset = 0;
    while offset + n <= samples.len() {
        let mut re: Vec<f64> = (0..n)
            .map(|i| {
                let w = 0.5
                    - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
                samples[offset + i] as f64 * w
            })
            .collect();
        let mut im = vec![0.0; n];
        fft(&mut re, &mut im);
        for k in 1..n / 2 {
            let mag = (re[k] * re[k] + im[k] * im[k]).sqrt();
            let hz = k as f64 * SR / n as f64;
            sum_num += mag * hz;
            sum_den += mag;
        }
        offset += n;
    }
    if sum_den > 0.0 { sum_num / sum_den } else { 0.0 }
}

/// Rate-compensated read-path check on real music.
///
/// Playing the same passage at rate `r` must move the spectral centroid by
/// exactly `r` — that is what a pitch shift is. A centroid that moves *less*
/// than `r` means the deck is losing top on top of the shift, which is the
/// blur a scratched vocal actually suffers. Comparing raw spectra instead
/// only measures the shift, which is supposed to happen.
fn run_centroid() {
    let start: f64 = std::env::var("WARMTH_START_FRAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(SR * 60.0);
    let rpm: f64 = std::env::var("WARMTH_RPM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(33.333_333);
    println!("start {:.2}s, {rpm} rpm", start / SR);
    let mut reference = 0.0_f64;
    for rate in [1.0, 0.9, 0.75, 0.5, 0.25, 1.5, 2.0] {
        let mut dsp = drive_common_at(start, rpm);
        let block = 128;
        let mut pos = dsp.position();
        let mut out: Vec<f32> = Vec::new();
        for _ in 0..((1.5 * SR) as usize / block) {
            pos += rate * block as f64;
            dsp.set_transport(true, 1.0, rate, 1.0);
            dsp.set_motion(pos, rate, 0.0);
            dsp.render(block as u32, 2);
            let optr = dsp.output_ptr();
            unsafe {
                let slice = std::slice::from_raw_parts(optr, block * 2);
                for f in 0..block {
                    out.push(slice[f * 2]);
                }
            }
        }
        let tail = &out[out.len() / 3..];
        let c = centroid(tail);
        if rate == 1.0 {
            reference = c;
        }
        let expected = reference * rate;
        println!(
            "rate {:>4.2}: centroid {:>8.1} Hz   expected {:>8.1} Hz   ratio {:>5.2} (1.00 = faithful)",
            rate,
            c,
            expected,
            if expected > 0.0 { c / expected } else { 0.0 }
        );
    }
}


fn write_wav(path: &str, samples: &[f32]) {
    let mut bytes: Vec<u8> = Vec::new();
    let data_len = (samples.len() * 2) as u32;
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&(SR as u32).to_le_bytes());
    bytes.extend_from_slice(&((SR as u32) * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write wav");
}

/// Render the same scratch at different stroke sizes, so the question "is
/// there a word under the finger" can be answered by ear.
fn run_render() {
    let start: f64 = std::env::var("WARMTH_START_FRAMES")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(SR * 50.0);
    let rpm: f64 = std::env::var("WARMTH_RPM")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(45.0);
    let out_dir = std::env::var("WARMTH_OUT").unwrap_or_else(|_| ".".to_string());
    let frames_per_turn = SR * 60.0 / rpm;
    for degrees in [30.0_f64, 60.0, 120.0, 240.0] {
        let mut dsp = drive_common_at(start, rpm);
        let block = 32;
        // Let it play a beat first, then interrupt it.
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        dsp.set_motion(dsp.position(), 1.0, 0.0);
        let mut out: Vec<f32> = Vec::new();
        for _ in 0..((0.6 * SR) as usize / block) {
            dsp.render(block as u32, 2);
            let ptr = dsp.output_ptr();
            unsafe {
                let sl = std::slice::from_raw_parts(ptr, block * 2);
                for f in 0..block { out.push(sl[f * 2]); }
            }
        }
        // Interrupt and scratch: two strokes covering `degrees` of arc each.
        let sweep = frames_per_turn * degrees / 360.0;
        let stroke_seconds = 0.28;
        let rate = sweep / (stroke_seconds * SR);
        let mut pos = dsp.position();
        for stroke in 0..4 {
            let dir = if stroke % 2 == 0 { -1.0 } else { 1.0 };
            for _ in 0..((stroke_seconds * SR) as usize / block) {
                pos += dir * rate * block as f64;
                dsp.set_transport(true, 1.0, dir * rate, 1.0);
                dsp.set_motion(pos, dir * rate, 0.0);
                dsp.render(block as u32, 2);
                let ptr = dsp.output_ptr();
                unsafe {
                    let sl = std::slice::from_raw_parts(ptr, block * 2);
                    for f in 0..block { out.push(sl[f * 2]); }
                }
            }
        }
        let path = format!("{out_dir}/scratch_{:.0}deg.wav", degrees);
        write_wav(&path, &out);
        println!(
            "{:>3}deg sweep: {:>6.0} ms of audio per stroke, peak rate {:.2}x  -> {}",
            degrees,
            sweep / SR * 1000.0,
            rate,
            path
        );
    }
}


/// The same quarter-turn scratch, delivered the way a browser delivers it.
///
/// A pointer reports every 8-16 ms with jittery timestamps; the harness fed
/// the deck a fresh position every 0.67 ms. Between events the host freezes
/// the hand target and then jumps it a whole event's worth of travel, so the
/// deck chases a staircase the hand never walked.
///
/// `reckon` is the candidate fix: carry the target forward at the rate last
/// reported, so the deck chases a ramp. With a hand at constant speed the
/// reckoned target arrives exactly where the next event reports it, and the
/// re-anchor is a no-op.
///
/// Wobble is measured mid-stroke only. A stroke reversal is a real, wanted
/// discontinuity in the commanded rate; counting it as error was what made
/// even the continuous case look bad.
fn run_pointer() {
    let start: f64 = std::env::var("WARMTH_START_FRAMES")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(SR * 50.0);
    let rpm: f64 = std::env::var("WARMTH_RPM")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(45.0);
    let out_dir = std::env::var("WARMTH_OUT").unwrap_or_else(|_| ".".to_string());
    let frames_per_turn = SR * 60.0 / rpm;
    let sweep = frames_per_turn * 0.25;
    let stroke_seconds = 1.0 / 3.0;
    let rate = sweep / (stroke_seconds * SR);

    // (label, interval ms, jitter ms, reckon, rate EMA seconds)
    // The live tracker sends an exact position with a rate low-passed at
    // 0.035 s (0.008 s once it decides a reversal is under way).
    let cases = [
        ("continuous-raw", 0.67, 0.0, false, 0.0),
        ("60hz-raw", 16.7, 0.0, false, 0.0),
        ("60hz-ema35-LIVE", 16.7, 0.0, false, 0.035),
        ("60hz-ema8", 16.7, 0.0, false, 0.008),
        ("120hz-ema35", 8.3, 0.0, false, 0.035),
        // Adaptive: fast whenever the hand disagrees with the filter, slow
        // when it is steady. Encoded as a negative marker below.
        ("60hz-adaptive", 16.7, 0.0, false, -1.0),
        ("60hz-jitter-adaptive", 16.7, 6.0, false, -1.0),
    ];
    let mut seed = 0x9e3779b9u64;
    let mut rand = move || {
        seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for (label, interval_ms, jitter_ms, reckon, ema_seconds) in cases {
        let mut dsp = drive_common_at(start, rpm);
        let block = 32;
        let block_ms = block as f64 / SR * 1000.0;
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        dsp.set_motion(dsp.position(), 1.0, 0.0);
        let mut out: Vec<f32> = Vec::new();
        for _ in 0..((0.6 * SR) as usize / block) {
            dsp.render(block as u32, 2);
            let ptr = dsp.output_ptr();
            unsafe { let sl = std::slice::from_raw_parts(ptr, block*2);
                     for f in 0..block { out.push(sl[f*2]); } }
        }
        // The hand's true continuous motion, and what the host knows of it.
        let mut hand = dsp.position();
        let mut target = hand;
        let mut last_seen_pos = hand;
        let mut last_seen_ms = 0.0_f64;
        let mut now_ms = 0.0_f64;
        let mut next_event_ms = interval_ms;
        let mut rate_est = 0.0_f64;
        let mut err_sum = 0.0_f64;
        let mut err_n = 0u64;
        let mut trace: Vec<f64> = Vec::new();
        let steps = (stroke_seconds * SR) as usize / block;
        for stroke in 0..4 {
            let dir = if stroke % 2 == 0 { -1.0 } else { 1.0 };
            for step in 0..steps {
                hand += dir * rate * block as f64;
                now_ms += block_ms;
                if reckon {
                    target += rate_est * block as f64;
                }
                if now_ms >= next_event_ms {
                    let elapsed_frames = ((now_ms - last_seen_ms) / 1000.0 * SR).max(1.0);
                    let raw = (hand - last_seen_pos) / elapsed_frames;
                    let dt = (now_ms - last_seen_ms) / 1000.0;
                    rate_est = if ema_seconds > 0.0 {
                        let alpha = 1.0 - (-dt / ema_seconds).exp();
                        rate_est + (raw - rate_est) * alpha
                    } else if ema_seconds < 0.0 {
                        // Blend toward the fast constant in proportion to how
                        // far the hand has departed from the filtered rate, so
                        // a pivot is followed and a steady stroke is smoothed.
                        // No direction test, so it still applies through zero.
                        let departure = ((raw - rate_est).abs() / 0.25).clamp(0.0, 1.0);
                        let tau = 0.035 + (0.008 - 0.035) * departure;
                        let alpha = 1.0 - (-dt / tau).exp();
                        rate_est + (raw - rate_est) * alpha
                    } else {
                        raw
                    };
                    target = hand;
                    last_seen_pos = hand;
                    last_seen_ms = now_ms;
                    next_event_ms = now_ms + interval_ms
                        + (rand() - 0.5) * 2.0 * jitter_ms;
                }
                dsp.set_transport(true, 1.0, rate_est, 1.0);
                dsp.set_motion(target, rate_est, 0.0);
                dsp.render(block as u32, 2);
                // Mid-stroke only: a reversal is a wanted discontinuity.
                trace.push(dsp.effective_rate());
                let phase = step as f64 / steps as f64;
                if (0.25..0.85).contains(&phase) {
                    err_sum += (dsp.effective_rate() - dir * rate).powi(2);
                    err_n += 1;
                }
                let ptr = dsp.output_ptr();
                unsafe { let sl = std::slice::from_raw_parts(ptr, block*2);
                         for f in 0..block { out.push(sl[f*2]); } }
            }
        }
        // Reversal sharpness: samples spent between +-0.8 of the stroke rate.
        let mut crossing_frames = 0u64;
        {
            let mut inside = false;
            let mut runs = Vec::new();
            let mut run = 0u64;
            for w in trace.windows(1) {
                let r = w[0];
                if r.abs() < rate * 0.8 {
                    inside = true;
                    run += 1;
                } else if inside {
                    runs.push(run);
                    run = 0;
                    inside = false;
                }
            }
            if !runs.is_empty() {
                crossing_frames = runs.iter().sum::<u64>() / runs.len() as u64;
            }
        }
        println!(
            "{:>18}: wobble {:>5.1}%   reversal takes {:>5.1} ms",
            label,
            (err_sum / err_n.max(1) as f64).sqrt() / rate * 100.0,
            crossing_frames as f64 * block as f64 / SR * 1000.0
        );
        let path = format!("{out_dir}/pointer_{label}.wav");
        write_wav(&path, &out);
    }
}

struct Take {
    dsp: ScratchAcousticDsp,
    out: Vec<f32>,
    pos: f64,
    block: usize,
}

impl Take {
    fn new(start: f64, rpm: f64) -> Self {
        let mut dsp = drive_common_at(start, rpm);
        dsp.set_transport(false, 1.0, 0.0, 0.0);
        dsp.set_motion(dsp.position(), 1.0, 0.0);
        let pos = dsp.position();
        Self { dsp, out: Vec::new(), pos, block: 32 }
    }
    fn pump(&mut self, seconds: f64) {
        for _ in 0..((seconds * SR) as usize / self.block) {
            self.dsp.render(self.block as u32, 2);
            let ptr = self.dsp.output_ptr();
            unsafe {
                let sl = std::slice::from_raw_parts(ptr, self.block * 2);
                for f in 0..self.block { self.out.push(sl[f * 2]); }
            }
        }
    }
    /// Let it play: no hand on the record.
    fn play(&mut self, seconds: f64) {
        self.dsp.set_transport(false, 1.0, 0.0, 0.0);
        self.pump(seconds);
        self.pos = self.dsp.position();
    }
    /// Hand on the record, moving at `rate` for `seconds`.
    fn hand(&mut self, rate: f64, seconds: f64, grip: f64) {
        for _ in 0..((seconds * SR) as usize / self.block) {
            self.pos += rate * self.block as f64;
            self.dsp.set_transport(true, 1.0, rate, grip);
            self.dsp.set_motion(self.pos, rate, 0.0);
            self.dsp.render(self.block as u32, 2);
            let ptr = self.dsp.output_ptr();
            unsafe {
                let sl = std::slice::from_raw_parts(ptr, self.block * 2);
                for f in 0..self.block { self.out.push(sl[f * 2]); }
            }
        }
    }
    /// Let go: the slipmat re-couples the record to the running platter.
    fn release(&mut self, seconds: f64) {
        self.dsp.set_transport(false, 1.0, 0.0, 0.0);
        self.pump(seconds);
        self.pos = self.dsp.position();
    }
}

/// A set of real turntable moves on the same vocal, so the organic behaviours
/// can be judged by ear rather than by metric.
fn run_organic() {
    let start: f64 = std::env::var("WARMTH_START_FRAMES")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(SR * 50.0);
    let rpm: f64 = std::env::var("WARMTH_RPM")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(45.0);
    let out = std::env::var("WARMTH_OUT").unwrap_or_else(|_| ".".to_string());
    let quarter = SR * 60.0 / rpm * 0.25;
    // A quarter turn in a third of a second: a normal scratch, rate ~1x.
    let natural = quarter / (SR / 3.0);

    // Baby scratch: back and forth at playing speed, fader open.
    let mut t = Take::new(start, rpm);
    t.play(0.7);
    for _ in 0..3 {
        t.hand(-natural, 1.0 / 3.0, 1.0);
        t.hand(natural, 1.0 / 3.0, 1.0);
    }
    t.release(1.2);
    write_wav(&format!("{out}/organic_baby.wav"), &t.out);

    // Interrupt and hold, then let the slipmat pull it back up to speed.
    let mut t = Take::new(start, rpm);
    t.play(0.9);
    t.hand(0.0, 0.6, 1.0);
    t.release(1.6);
    write_wav(&format!("{out}/organic_touchstop.wav"), &t.out);

    // Dub pull: drag it down slowly against the platter, then let go.
    let mut t = Take::new(start, rpm);
    t.play(0.7);
    t.hand(0.55, 0.5, 0.55);
    t.hand(0.25, 0.5, 0.55);
    t.release(1.6);
    write_wav(&format!("{out}/organic_dubpull.wav"), &t.out);

    // Backspin: throw it hard in reverse and let it go.
    let mut t = Take::new(start, rpm);
    t.play(0.7);
    t.hand(-3.0, 0.45, 1.0);
    t.release(1.6);
    write_wav(&format!("{out}/organic_backspin.wav"), &t.out);

    // Stab: pull back off the beat, push it forward hard, release on the one.
    let mut t = Take::new(start, rpm);
    t.play(0.7);
    t.hand(-natural * 1.6, 0.22, 1.0);
    t.hand(natural * 2.2, 0.22, 1.0);
    t.release(1.4);
    write_wav(&format!("{out}/organic_stab.wav"), &t.out);

    // Pull the power mid-vocal: the platter brakes and the pitch falls away.
    let mut t = Take::new(start, rpm);
    t.play(0.8);
    t.dsp.set_transport(false, 0.0, 0.0, 0.0);
    t.pump(1.4);
    write_wav(&format!("{out}/organic_motorstop.wav"), &t.out);

    // Lift the needle mid-vocal and drop it back: the programme waits.
    let mut t = Take::new(start, rpm);
    t.play(0.8);
    t.dsp.set_needle_lifted(true);
    t.pump(0.7);
    t.dsp.set_needle_lifted(false);
    t.pump(1.2);
    write_wav(&format!("{out}/organic_needle.wav"), &t.out);

    println!("wrote 7 organic takes to {out}");
}
