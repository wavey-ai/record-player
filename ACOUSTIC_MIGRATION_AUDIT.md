# Acoustic Migration Audit

Source of truth: `../yl.vin/apps/play` (inspected directly — not READMEs or prior notes).
Destination: this repository — Rust engine (`src/acoustic.rs`, `src/engine.rs`) + worklet
(`web/player-worklet.js`) + host (`web/player-host.js`).

Original files inspected in full or in the relevant regions:

- `src/scratch-varispeed-worklet.js` (774 lines, entire file) — the authoritative real-time
  transport + acoustic DSP.
- `src/player.js` — needle-surface asset loading (3915–3964), surface beds (3966–4249),
  needle-drop foley (4021–4087), mobile gain (4109–4111), lead-in/deadwax orchestration
  (12147–12274), transport toggle (12276–12356), needle-drop seek (12878–12961),
  programme-end → deadwax (11267–11273).
- `src/player-audio-source-config.js`, `src/player-playback-config.js`,
  `src/player-environment-config.js`, `src/player-record-profile.js`,
  `src/player-audio-mixer.js` (gain ramps).
- `../yl.vin/record-player-split/player-wasm/src/lib.rs` — profile geometry
  (`profile_turns`, rpm, seconds-per-turn).

## 1. Real-time transport + acoustic DSP (varispeed worklet → `ScratchAcousticDsp`)

The Rust `ScratchAcousticDsp` is a line-for-line port of
`BitneedleScratchVarispeedProcessor.process()`. Every constant was compared by value:

| Behaviour | Original symbol / constants | Rust equivalent | Status |
|---|---|---|---|
| Stylus/vinyl damped spring | `RATE_SPRING_OMEGA=70`, `ZETA=0.85` | same consts, same integration (`rate_velocity`) | exact |
| Position catch-up (cueing) | `POSITION_CATCHUP_SECONDS=0.28`, clamp ±0.12, scaled by grip | same | exact |
| Motion hold + release decay | `0.05` s hold, `0.06` s exp release | same | exact |
| Grip attack / release (slipmat) | `0.1` / `0.045` s one-pole | same | exact |
| Motor spin-up (asymptotic) | `MOTOR_SPINUP_SECONDS=0.3`, exp toward `motorRate` when speeding up | same | exact |
| Motor brake (linear, constant torque) | `MOTOR_BRAKE_SECONDS=0.32`, linear step to stop | same | exact |
| Grip ownership threshold | `GRIP_OWNERSHIP=0.5` | same | exact |
| Still-snap (hand pins record) | `STILL_SNAP_SECONDS=0.03`, requires grip>0.5, hand & rate below deadzone | same | exact |
| Deadzone | `DEADZONE_RATE=0.006` | same | exact |
| Speed-dependent drag lowpass | max 19 kHz, knee 0.95, pow 1.3, tracing loss above 2.5×, one-pole per channel | same | exact |
| Wow (rotation-locked) | `WOW_REV_SECONDS=1.8`, phase advances with `correctedRate*rateScale/framesPerRev`, depth `clamp(absRate,0,1.2)*0.0012`, gate absRate>0.18 | same | exact¹ |
| Flutter (fixed-frequency) | `FLUTTER_HZ=6.4`, phase scaled by `clamp(absRate,0,1.4)`, amplitude ×0.22 | same | exact |
| Interpolation | 4-point Catmull–Rom with slope + curvature outputs | identical coefficients | exact |
| Waveform slope/curvature rasp | `slope*0.48 + curvature*0.86`, signed by direction, `SOURCE_TEXTURE_GAIN=0.00018`, realtime dip `0.72/0.14`, slowRub/speedLift/accelLift weights `0.18/0.72/0.28/0.38` | same | exact |
| Contact noise | `CONTACT_NOISE_GAIN=0.00008`, dip `0.94/0.18`, slowRub `0.26→×0.36`, fastFriction `(x−2.2)/5.5→×0.72`, contact clamp `0.08..1.08` | same | exact |
| Acceleration noise | `rateDelta*0.00028`, dip `0.88/0.16`, clamp 0.0007 | same | exact |
| Movement gain | Gaussian presence σ=0.38, underspeed `0.78+0.22·x^0.1`, overspeed `1+0.014(x−1)`, clamp `0.68..1.08` | same | exact |
| Position-locked groove grain | hash-noise cells at spacing 3.7 / 37, weights 0.72/0.22, salts `0x51f15e`/`0x2d4a11`, speedWeight `absRate/2.4` clamp `0.14..1` | same hashes, same salts | exact |
| Dust flecks (position-locked, repeatable) | 0.12 s cells, chance ≥ 0.996, salts `0x6d2b79/0x4f1bbc/0x73c4d9`, quadratic envelope, `DUST_FLECK_GAIN=0.000045` | same | exact |
| Seeded randomness | LCG `imul(1664525)+1013904223`, seed `0x9e3779b9`; hash `0x7feb352d`/`0x846ca68b` mix | same wrapping arithmetic | exact |
| White contact texture | first-difference highpass of LCG noise ×0.18, groove surface ×0.76 | same | exact |
| Contact impulse | clamp to 1, decay 0.985/sample, noise ×0.004 | same | exact |
| Window miss fade | `0.006` s fade holding last output samples | same (`render_window_missing` + in-loop missFade) | exact |
| Window request pacing | margin 0.75 s × max(1, speed·0.5), projection 0.18 s, throttle 0.03/0.08 s | same | exact |
| End-of-record | grip<0.5 && motorRate>0 && pos ≥ total−3 → ended once, motorRate=0 | same | exact |
| Needle lift mutes cartridge, platter keeps turning | `muted = needleLifted`, position still advances | same | exact |
| Scratch never restarts motor | `applyTransport("hand")` leaves `motorRate` untouched; release only flips `handContact` | `set_transport` identical | exact |
| Rate clamp | ±10 | `max_rate` config, default 10 | exact (default) |
| `start()` position fallback | `position \|\| targetPosition \|\| 0` (first *non-zero*) | was `position.max(target_position)` | **fixed in this migration** |
| Zero-speed sampling | `sampled = movementGain > 0 ? sampleChannel(...) : 0` — a stationary stylus never reads the window, never flags a window miss | was: always sampled → spurious miss/fade when parked outside the window | **fixed in this migration** |

¹ Original advances flutter phase with the *context* sample rate (`sampleRate`) and wow with
source frames; Rust uses `output_sample_rate` for flutter identically.

Shared-SAB transport (`shared-init`, Atomics control block) is an original transport-plumbing
alternative to the message path, not an acoustic behaviour; the standalone player uses the
message path. Not migrated by design.

## 2. Scratch pointer mechanics

Original pointer → angle mapping, EMA (`pointer_filter_seconds`), deadzone, true-speed
lock (`lock_center_rate/width/strength` Gaussian pull) live in
`scratch-gesture-controller.js`/`player-stylus-calibration.js` and were previously ported as
`ScratchSimulation` + `StylusCalibration` (monotone Hermite groove calibration with bisection
inverse) in `src/acoustic.rs`. Angle unwrap (±π), `elapsed = max(dt,1ms)/1000 → min 4 ms`,
`mapped_delta = Δangle/2π × secondsPerTurn` verified identical. Status: exact.

## 3. Lead-in / programme / deadwax phases

| Behaviour | Original | Standalone | Status |
|---|---|---|---|
| Profile revolution duration | 60/45 = 1.3333 s (single45), 60/33.3333 = 1.8 s (LP); pitch multiplies rpm | host `state.rpm`/`baseRpm` | exact |
| Lead-in turns / deadwax turns | `profile_turns()` = 2.0 / 2.0 for both profiles → duration `turns × 60/physicalRpm` | engine `TimedRegionState` + host duration computation | migrated |
| **Lead-in trigger** | `startLeadInPlayback()` is **dead code in the current original** — nothing sets `state.leadIn.active = true` (verified: only assignment is inside the never-called function). Playback starts directly at the programme. | engine supports `StartTimedRegion(LeadIn)` but host never triggers it | exact (both dormant) |
| **Deadwax trigger** | programme end (`nextOffset >= duration − 0.02`) → `startDeadwaxPlayback()` when needle down, not scratching, not looping (player.js 11267–11273) | **was missing** — host never dispatched any timed region | **implemented** |
| Deadwax bed | needle-surface asset slice, lowpass 4600 Hz Q 0.4, gain 0.052 (×2.25 mobile), envelope: 0.0001 → linear 80 ms → hold → linear fade last 160 ms → 0.0001 | **was missing** | **implemented in Rust** |
| Lead-in bed (kept for parity with the dormant original path) | lowpass 5200 Hz Q 0.45, gain 0.048 (×2.25 mobile), same envelope | **was missing** | **implemented in Rust** |
| Deadwax completion | after duration, playback considered ended, `deadwax.completed` holds; next START restarts from 0 | engine `finish_region` sets playing=false | exact |
| STOP during region | lifts needle, stops bed, publishes transport intent | engine `toggle_playback` region branches | exact |
| Scratch interrupts region | `stop_regions()` in `begin_scratch` | same | exact |

## 4. Needle-surface asset

Original: `assets/audio/needle-surface.opus`, decoded once via `decodeAudioData`, cached;
slice selection `selectNeedleSurfaceSample`: pad 0.05 s, loop if asset ≤ duration+pad,
random offset in `[0, len − duration − pad]`.

Migration: the asset is copied into this repo and decoded by the host (off the real-time
thread) with `decodeAudioData`; Float32 PCM channels are transferred to the worklet once and
handed to the Rust DSP (`setSurfaceAsset`). All real-time slicing, filtering, envelopes and
mixing happen inside the single worklet's Rust render — no second clocked engine. The random
offset uses the DSP's deterministic LCG (original used `Math.random()`; this is the one
deliberate divergence, chosen for testability — distribution is identical).
The original synthetic fallback (`console.warn` … "synthesizing groove noise") only kicks in
when the asset fails to load; the Rust DSP's position-locked groove noise remains the fallback.

## 5. Needle-drop foley (`playNeedleDropContact`)

| Component | Original constants | Status |
|---|---|---|
| Thump oscillator | sine 130 Hz → exp ramp → 52 Hz at +0.07 s; gain 0.0001 → exp → `0.045×mobile` at +6 ms → exp → 0.0001 at +95 ms; stop at +0.1 s | implemented in Rust (`trigger_needle_drop`) |
| Crackle burst | asset slice, 0.34 s, lowpass 6200 Hz Q 0.5, linear envelope 0.0001 → peak (`0.048×1.9×mobile`) @14 ms → peak×0.32 @120 ms → 0.0001 @340 ms | implemented in Rust |
| Gate | skipped when `state.needleLifted` at trigger time (n.b. original's post-await check reads `state.eedleLifted` — a typo that never blocks; behaviour preserved by only gating at trigger) | implemented |
| Triggers | (a) needle lowered after lift (live re-cue), (b) seek while playing (waveform/tonearm) | host posts `needle-drop` on both |
| Early landing | seek while playing lands `0.05 + random()*0.09` s early (50–140 ms) | implemented in host seek path |
| Settle delay | 110 ms only on the non-platter (buffer-source) path; the platter path (which this player uses exclusively) re-positions immediately with impulse 0.22 | exact (platter path) |
| Retrigger guard | duplicate `needle down` messages don't retrigger (engine `set_needle` early-returns when state unchanged) | exact |

## 6. Mobile-specific behaviour

- `resolveNeedleSurfaceGain`: ×2.25 on mobile — applies to lead-in bed, deadwax bed, thump,
  and burst. Detection: `/Mobi|Android|iPhone|iPad|iPod|Mobile/i` on UA, or iOS WebKit
  (`iP(ad|hone|od)` or MacIntel with >1 touch points). Reproduced exactly in the host;
  multiplier passed to Rust via `setSurfaceGainMultiplier`.
- No other mobile acoustic tuning found in the original DSP paths (mobile branches elsewhere
  are layout/waveform-bucket concerns).

## 7. Gain and smoothing

| Item | Original | Standalone | Status |
|---|---|---|---|
| Mixer track/master ramps | `player-audio-mixer.js` default 5 ms linear ramps | engine emits `ramp_ms: 12` for mixer moves; host linear-ramps | approximate² |
| Scratch renderer start ramps | 12 ms in `scratch-audio-runtime.js` (0→1) | platter transport handles starts inside DSP spring | n/a (buffer path not migrated) |
| Renderer stop fade | 18 ms fadeOut / 1 ms hard | DSP `stop()` + window-miss fade | approximate² |
| Unavailable-PCM fade | `WINDOW_MISS_FADE_SECONDS = 0.006` (6 ms) | same constant | exact |
| Needle gate | packet gain 0/1 instantaneous + worklet mute | same (SetPacketGain 0 ramp) | exact |
| Sharp crossfader | width-based full-middle curve | `sharp_crossfader_gains` | exact |
| Clipping | final `clamp(-1, 1)` per sample | same | exact |

² The previously remembered "4 ms fader / 8 ms transport stop" values do **not** appear in the
current original source; current values are 5 ms mixer ramps, 12 ms renderer starts, 18 ms
renderer stop fade, 6 ms window-miss fade. The 6 ms value is in Rust; mixer ramps remain host
JS WebAudio (per architecture split — AudioContext gain nodes). The 12 ms engine ramp for
mixer moves predates this audit and differs from the original's 5 ms default; changed to 5 ms.

## 8. Chunked PCM assembly and seam repair

- Decoder strips ±480-sample guards (verified `player-wasm/src/codec.rs` guard logic); the
  worklet does not strip again.
- 24-sample centred cubic Hermite seam repair (12 either side) in `player-worklet.js`
  `repairChunkSeam`: applies only when `boundaryFrame === decodedLength` (contiguous), per
  channel, uses endpoints + one-sample slopes outside the replaced region, clamps to ±1,
  replaces `leftAnchor+1 .. rightAnchor−1` in place — no length or timeline change, and the
  repaired data is part of the single window the DSP samples, so forward/reverse/scratch all
  read the same repaired samples. Retained.
- Progressive playback: first `append-pcm` → `streamReady` → `markLoadedReady`; boundary
  reach with incomplete stream → `waitingForData` + `buffering` message, resume without a
  cold restart when the next chunk lands (worklet `append-pcm` handler). Decode progress
  string preserved via `renderDecodeStatus` (`Ready · …`). Verified present.

## 9. Architecture / duplicate implementations

- All acoustic DSP is in Rust. The worklet JS holds: PCM window assembly + seam repair,
  message plumbing, replay sequencing. No JS duplicate of any effect. The surface beds and
  needle-drop foley were **added to Rust**, not JS, so there is exactly one implementation.
- Host JS holds only: DOM/pointer, AudioContext + worklet setup, asset decode, decode
  progress UI, mixer gain nodes, timed-region wall-clock timers (the original also ran these
  on `window.setTimeout`).

## 10. Remaining risks / known divergences

1. Random slice offsets use the DSP LCG instead of `Math.random()` (deterministic on purpose).
2. Biquad lowpass in Rust follows the Web Audio spec's RBJ lowpass; float rounding will
   differ from browser implementations at the ~1e-7 level.
3. Exponential gain ramps are emulated per-sample multiplicatively — equivalent formula to
   `exponentialRampToValueAtTime`, but Rust computes in f64 (browsers vary).
4. The original HTML-audio fallback bed path (`startHtmlNeedleSurfaceStatic`) is not
   migrated: it exists only for environments without worklet support, which this standalone
   player does not target.
5. Golden JS-vs-Rust comparison harness is deferred until the migration is complete (per
   project direction); parity above is by construction (verified constants/equations), not
   yet by recorded output comparison. See ACOUSTIC_PARITY_REPORT.md.
