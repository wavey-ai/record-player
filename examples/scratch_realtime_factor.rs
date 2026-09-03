use record_player::{AcousticConfig, ScratchAcousticDsp};
use std::time::Instant;

const OUTPUT_SAMPLE_RATE: f64 = 48_000.0;
const BLOCK: usize = 128;

fn fill_window(dsp: &mut ScratchAcousticDsp, length: usize) {
    dsp.prepare_window(1, length as u32).expect("prepare");
    let ptr = dsp.window_channel_ptr(0);
    assert!(!ptr.is_null(), "window channel pointer must be non-null");
    unsafe {
        for i in 0..length {
            let phase = (i as f64 * 2.0 * std::f64::consts::PI * 997.0 / OUTPUT_SAMPLE_RATE).sin();
            std::ptr::write(ptr.add(i), (phase * 0.5) as f32);
        }
    }
    dsp.commit_window(OUTPUT_SAMPLE_RATE, 0, length as u32, Some(0.0))
        .expect("commit");
}

fn run(dsp: &mut ScratchAcousticDsp, frames: usize, scratch_from_second: f64) -> f64 {
    dsp.render(1, 2); // warm-up
    let pos = 60_000.0_f64;
    let mut phase = 0.0_f64;
    let blocks = frames / BLOCK;
    let start = Instant::now();
    for b in 0..blocks {
        let t = (b * BLOCK) as f64 / OUTPUT_SAMPLE_RATE;
        let scratching = t >= scratch_from_second;
        let rate: f64;
        if scratching {
            let stroke_rate = 2.0; // Hz
            phase += stroke_rate * (BLOCK as f64 / OUTPUT_SAMPLE_RATE);
            rate = (phase * std::f64::consts::TAU).sin() * 2.5;
            if (phase * std::f64::consts::TAU).sin().signum()
                != ((phase - BLOCK as f64 / OUTPUT_SAMPLE_RATE * stroke_rate)
                    * std::f64::consts::TAU)
                    .sin()
                    .signum()
            {
                // sudden reversal impulse
                dsp.set_motion(pos, rate, 0.6);
            }
            dsp.set_transport(true, 0.0, rate, 1.0);
        } else {
            rate = 1.0;
            dsp.set_transport(false, 1.0, 0.0, 0.0);
        }
        dsp.set_motion(pos, rate, 0.0);
        dsp.render(BLOCK as u32, 2);
    }
    let elapsed = start.elapsed().as_secs_f64();
    let audio_seconds = frames as f64 / OUTPUT_SAMPLE_RATE;
    audio_seconds / elapsed
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let scratch_from_second: f64 = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.0);
    let render_seconds: f64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30.0);

    let mut dsp = ScratchAcousticDsp::new_native(OUTPUT_SAMPLE_RATE, AcousticConfig::default())
        .expect("construct");
    fill_window(&mut dsp, (OUTPUT_SAMPLE_RATE * 120.0) as usize);
    dsp.start();
    dsp.set_needle_lifted(false);

    let frames = (render_seconds * OUTPUT_SAMPLE_RATE) as usize;
    let factor = run(&mut dsp, frames, scratch_from_second);
    println!(
        "scratch realtime factor: {:.2}x  (scratch from t={}s, {}s audio)",
        factor, scratch_from_second, render_seconds
    );
    println!("=> render budget available: {:.1}% of realtime", 100.0 * factor);

    if factor < 1.0 {
        eprintln!("WARNING: slower than realtime; cannot run in a live callback");
    }
}
