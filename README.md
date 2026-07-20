# vin.yl.player

This standalone player plays Bitneedle picture records. It includes a Rust
transport model, an acoustic scratch engine, and a player-only record decoder.
It also includes shared-memory PCM windows and frame-timed scratch replay. An
optional canvas UI shows the turntable.

The repository deliberately separates the real-time renderer from record parsing:

- **`record-player`** is the main crate. It owns transport state, commands, playback state, mixing decisions, stylus calibration, and the `ScratchAcousticDsp` used by the AudioWorklet.
- **`player-wasm`** is the decoder-worker crate. It reads Bitneedle PNG records and exposes the record/ECDC functions required by this player.
- **`app`** is the browser host, decoder worker, PCM-window worker, AudioWorklet, IndexedDB stores, public JavaScript API, and optional canvas renderer.

The workspace is:

```toml
[workspace]
members = [".", "player-wasm"]
resolver = "2"
```

## Status

This is a focused standalone player rather than a copy of the legacy play application. It currently includes:

- Bitneedle PNG inspection and ECDC decoding
- Rust/WASM transport and acoustic rendering
- motor playback, braking, pitch/RPM changes, scratching and needle lift
- double-buffered shared-memory PCM windows
- coalesced, multi-pointer canvas gesture tracking
- an audio-rate, travel-aware scratch gate with eight technique profiles
- adaptive band-limited interpolation for high-speed playback
- a default-on Rust HF acceleration limiter, kept distinct from stylus tracing,
  with both strengths in a collapsed Advanced Controls panel
- a build-bound DJ validation console for hardware and blinded-study evidence
- participant-bound, condition-free ABX packages and a blind listening runner
- IndexedDB PCM caching
- frame-timed scratch-performance capture, persistence and replay
- a configurable radial canvas UI with a stylus, concentric turntable, Technics-style pitch control, strobe rows and lamp.

Published Bitneedle records use the physical record ending by default: after
the programme finishes, the stylus traverses two turns of deadwax and then
settles into a persistent run-out lock until the transport is stopped.
Conventional files loaded with `loadAudioFile(...)` default to a clean end with
no deadwax; either loader can override that policy with its `cleanEnd` option.

It does **not** currently include:

- remote scratch sessions or two-deck playback
- an HTML-audio fallback
- a waveform UI, sample library, or authoring tools.

## Requirements

Install:

- Node.js 20 or newer
- Rust and Cargo
- the `wasm32-unknown-unknown` Rust target
- `wasm-pack`
- a browser with AudioWorklet, WebAssembly, IndexedDB and `SharedArrayBuffer` support.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

`SharedArrayBuffer` requires a cross-origin-isolated page. The bundled development server sends:

```text
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

A production host must send equivalent headers for the player page and its worker/WASM assets.

## Repository dependencies

`player-wasm/Cargo.toml` currently references Bitneedle Rust crates through local paths such as:

```text
../bitneedle/record-core
../bitneedle/record-decode
../bitneedle/record-descriptor
../bitneedle/record-sidecar
../bitneedle/encodec-rs
```

With this repository at `/path/to/vin.yl.player`, the expected sibling layout is therefore:

```text
/path/to/
├── vin.yl.player/
└── bitneedle/
```

The decoder helper scripts live directly in `web`:

- `browser-formatting.js`
- `encodec-bundle-names.js`
- `onnx-runtime-session.js`
- `onnx-worker-tensors.js`
- `ecdc-pcm-layout.js`
- `player-cache-config.js`
- `player-cache.js`
- `player-pcm-helpers.js`.

This repo owns these scripts. They are not vendored from another source.

The browser build additionally needs ONNX Runtime Web assets and the EnCodec ONNX bundles. Their locations can be supplied with environment variables. By default they are read from `vendor/wasm/` in this repo.

## Environment variables

| Variable | Required | Default | Purpose |
| --- | --- | --- | --- |
| `BITNEEDLE_ONNX_RUNTIME_DIR` | No | `vendor/wasm/onnxruntime-web` | ONNX Runtime Web distribution copied into `dist/wasm/onnxruntime-web`. |
| `BITNEEDLE_ENCODEC_BUNDLES_DIR` | No | `vendor/wasm/encodec-rs/bundles` | EnCodec ONNX bundles; small `bundle.json` manifests are copied into `dist/wasm/encodec-rs/onnx-bundles`, the large model weights are uploaded to R2 separately (see `make sync-onnx-assets`). |
| `PORT` | No | `5193` | Port used by the development server. A positional argument to `npm run dev -- 8000` also works. |

## Build and run

```bash
cd app
npm run build
npm run dev
```

Open the URL printed by the development server, normally:

```text
http://localhost:5193
```

The build:

1. clears `dist`
2. copies `web` into `dist`
3. writes `dist/player-build-info.json` with the Git commit and worktree state
4. builds the root `record-player` crate into `dist/wasm/record-player`
5. builds `player-wasm` into `dist/wasm/player-wasm`
6. copies the shared decoder scripts
7. copies ONNX Runtime Web and the EnCodec bundles.

Generated browser modules:

```text
dist/wasm/record-player/record_player.js
dist/wasm/record-player/record_player_bg.wasm
dist/wasm/player-wasm/player_wasm.js
dist/wasm/player-wasm/player_wasm_bg.wasm
```

## Runtime WASM Bundles

The player uses separate browser contexts, so the runtime is split across a few focused WASM bundles:

- **`record-player`**
  - files:
    - `dist/wasm/record-player/record_player.js`
    - `dist/wasm/record-player/record_player_bg.wasm`
  - loaded by:
    - `web/player-host.js`
    - `web/player-worklet.js`
  - responsibility:
    - transport state
    - playback engine init
    - `ScratchAcousticDsp` in the AudioWorklet

- **`player-wasm`**
  - files:
    - `dist/wasm/player-wasm/player_wasm.js`
    - `dist/wasm/player-wasm/player_wasm_bg.wasm`
  - loaded by:
    - `web/record-decoder-worker.js`
    - `web/opus-cache.js`
  - responsibility:
    - record header and descriptor decode
    - playback metadata helpers
    - cache/tape encryption helpers
    - per-packet SoundKit v2 header build and parse

- **`onnxruntime-web`**
  - files under:
    - `dist/wasm/onnxruntime-web/`
  - loaded by:
    - `web/record-decoder-worker.js`
  - responsibility:
    - ONNX execution for the decoder worker

- **EnCodec ONNX bundles**
  - files under:
    - `dist/wasm/encodec-rs/onnx-bundles/`
  - loaded by:
    - `web/record-decoder-worker.js`
  - responsibility:
    - model/data bundles used by the ECDC decode path

In practice:

- the **main thread** coordinates UI, state, and startup
- the **decoder worker** loads `player-wasm`, `onnxruntime-web`, and the EnCodec bundles
- the **AudioWorklet** loads `record-player`
- the **tape/cache helper path** loads `player-wasm`.

The performance boundary is deliberate. All per-sample DSP and the
state-critical transport, scratch, gate, final packet/mixer gain, fader-replay
and acoustic models run in Rust/WASM. `player-worklet.js` remains the output-frame scheduler,
bounded-window coordinator, direct bank-to-prepared-Rust copy path and required
interleaved-WASM-to-planar-Web-Audio output layer. Gesture tracking and UI
remain in JavaScript because they consume browser pointer APIs; signed-16-bit
PCM retention, seam repair and bounded Float32 bank assembly remain in a
JavaScript worker to avoid moving a whole record across the WASM boundary.
Language placement follows measured real-time cost: engine policy defaults to
Rust unless the browser boundary would add work or latency.

In the current real-WASM timing smoke benchmark, p95 render-call time was
`0.0388 ms` (`1.45%` of budget) for normal playback and `0.2127 ms` (`7.97%`)
for alternating `±8×` scratching with `crab`/8 clicks. Applying a fresh
six-second stereo PCM window measured p95 `0.1020 ms` (`3.83%`) and maximum
`0.2696 ms` (`10.11%`). A 128-frame quantum at 48 kHz is `2.667 ms`. These are
engine timing measurements, not perceptual-validation results.

`npm run bench:worklet` first rebuilds release WASM, then runs the deterministic
Node worklet harness and enforces a render p95 gate below 50% of the 128-frame
budget. Fresh-window p95 must stay below 25%, and its maximum must stay below
50% of a quantum.
The figures above are a representative 2026-07-20 run on Apple Silicon macOS
26.5 with Node 26.3.0. The harness uses real release WASM and a mocked
`AudioWorkletProcessor`; it is a regression smoke test, not a browser audio-thread
deadline measurement.

`npm run test:browser` supplies the browser-side counterpart. It samples
Chrome's realtime Web Audio render-capacity metric during lead-in, steady
playback, a PCM window swap, every scratch preset and replay. It fails if a
p95 sample uses `50%` or more of the callback interval, or if any sampled
callback reaches its full deadline. It also transfers captured audio packets to
a dedicated worker and checks monotonic timestamps, cumulative timeline
continuity and unintended silence during steady playback. Repeated 2026-07-20
headless Chrome 150 runs had a worst sampled callback of `62.66%`; the three-run
stress batch had a worst run-level p95 of `10.65%`. A later three-run device
statistics batch reported zero underrun events and zero underrun duration over
`12.05–13.05 s` per run. These results cover the real browser AudioWorklet and
release WASM, but they do not replace physical output-device xrun tests.

## Architecture

```text
Bitneedle PNG
    │
    ▼
record-decoder-worker.js
    │  player-wasm + ONNX Runtime + EnCodec bundles
    │  decoded signed 16-bit PCM channels
    ▼
IndexedDB PCM cache
    │
    ▼
pcm-window-worker.js
    │  one-second source chunks
    │  alternating 6-second SharedArrayBuffer banks
    ▼
player-worklet.js
    │  record-player / ScratchAcousticDsp
    │  motor, hand control, interpolation, acoustic texture, final gain
    ▼
unity GainNode (routing and capture only)
    ▼
AudioContext destination
```

The main thread routes commands and publishes state. It does not assemble a
whole floating-point record for the AudioWorklet. The PCM-window worker retains
the decoded programme as signed 16-bit PCM and converts only the requested
region. Under cross-origin isolation it alternates between two six-second
`SharedArrayBuffer` banks: one bank may be filled while the other remains
available to the renderer. The worklet applies only completed generations, and
Rust owns a copy of only the active source window rather than a full-record
floating-point allocation.

Source chunks are normally one second. The Rust engine requests a replacement
window before the stylus reaches a bank edge, with extra projection in the
current direction of travel. A six-millisecond fade masks a temporary window
miss. If shared memory is unavailable, the worker can transfer an individual
bounded window, although production deployments should provide the isolation
headers listed above.

## JavaScript API

The player publishes itself at:

```js
globalThis.vin.yl.player
```

It also dispatches `vin.yl.player.ready` after the Rust core worker is ready:

```js
window.addEventListener("vin.yl.player.ready", event => {
  const player = event.detail;
  console.log(player.getState());
});
```

### Load and transport

```js
const player = globalThis.vin.yl.player;

await player.loadRecord(file);
await player.play();
await player.pause();
await player.togglePlayback();

player.seekSeconds(42.5);
player.seekRatio(0.5);
await player.setNeedleLifted(false);
```

During active programme playback, a seek lands `50–140 ms` before its visual
aim and adds needle-drop foley. The host resolves one landing position and uses
it for both the immediate worklet update and the core seek. This prevents a
brief exact-target sound before the physical landing. A paused or lifted-needle
seek remains exact.

`loadRecord` expects a browser `File` containing a Bitneedle PNG. Its
`cleanEnd` default is `false`, so published records play two deadwax turns and
then hold the run-out lock. Conventional audio previews default the other way:

```js
await player.loadAudioFile(audioFile, {
  artworkUrl,
  title: "Afterglow",
  artist: "Mara Vela",
  cleanEnd: true
});
```

Pass `{ cleanEnd: true }` to `loadRecord`, or `{ cleanEnd: false }` to
`loadAudioFile`, to override those defaults.

### Deck controls

```js
await player.setRpm(45);
await player.setVolume(0.8);
await player.setCrossfader(0.5);
player.setScratchPreset("flare");
player.setScratchClicks(2);
player.setHighFrequencyAccelerationLimit(0.35);
player.setStylusTracingLimit(0.72);
```

- RPM is clamped to `16..90` by the host.
- Volume is clamped to `0..1`.
- Crossfader is clamped to `0..1`.
- Scratch clicks are rounded and clamped to `1..8`.
- Both advanced-control strengths accept finite values clamped to `0..1`; non-finite
  input throws.

The canvas presents pitch as a Technics-style ±8% control around the record's native RPM, but the lower-level API accepts an absolute RPM.

Scratch presets are `baby`, `stab`, `chirp`, `transform`, `flare`, `crab`,
`orbit` and `drum`. Selecting one restores its preset-specific default click
count; `setScratchClicks(n)` can then override that count without moving the
manual crossfader. A new player starts on `baby`/1 click. The full defaults
table appears under
[Manual crossfader and scratch gate](#manual-crossfader-and-scratch-gate).

The same controls are available on the canvas and under **ADVANCED CONTROLS**;
keyboard, pointer, touch and API paths all preserve the independent manual
fader value.

### Programmatic scratching

```js
await player.beginScratch({
  pointerId: 1,
  positionFrames: player.getState().positionFrames,
  rotationDegrees: 0,
  rate: 0,
  impulse: 0.22,
  grip: 0.65
});

await player.updateScratch({
  positionFrames: 120000,
  rotationDegrees: -18,
  rate: -0.8,
  impulse: 0.2,
  grip: 0.9
});

await player.endScratch({
  rotationDegrees: -18,
  resumePlayback: true,
  cancelled: false
});
```

Pointer geometry belongs in the UI layer. The player API accepts normalized
engine commands expressed in source frames, rates, rotation degrees and a
`0..1` grip. Grip controls slipmat coupling: light contact lets the powered
platter bleed through; firm contact gives the hand record ownership. Rust owns
the smoothing and physical blend.

Canvas gestures also supply `inputTimeMs`. The host projects that timestamp
once onto the output audio clock and carries the resulting integer
`outputFrame` through the worklet message, Rust begin/move/end protocol and
performance capture. Programmatic callers may supply either field; omitted
timing means the current output frame. The requested input frame and later
worklet-applied frame remain separate latency telemetry.
Pointer cancellation uses the same safe physical release as pointer-up but is
retained as `cancelled: true` in the performance trace for hardware audits.

### State subscription

```js
const unsubscribe = player.subscribe(state => {
  console.log({
    ready: state.ready,
    playing: state.playing,
    motorRunning: state.motorRunning,
    leadInActive: state.leadInActive,
    deadwaxActive: state.deadwaxActive,
    deadwaxProgress: state.deadwaxProgress,
    needleLifted: state.needleLifted,
    scratching: state.scratching,
    buffering: state.buffering,
    positionSeconds: state.positionSeconds,
    durationSeconds: state.durationSeconds,
    positionRatio: state.positionRatio,
    rpm: state.rpm,
    nativeRpm: state.nativeRpm,
    playbackRate: state.playbackRate,
    volume: state.volume,
    crossfader: state.crossfader,
    scratchPreset: state.scratchPreset,
    scratchClicks: state.scratchClicks,
    scratchGate: state.scratchGate,
    scratchGateTarget: state.scratchGateTarget,
    scratchDirection: state.scratchDirection,
    scratchMoving: state.scratchMoving,
    scratchGatePhase: state.scratchGatePhase,
    scratchStrokeProgress: state.scratchStrokeProgress,
    effectiveRate: state.effectiveRate,
    highFrequencyAccelerationLimit: state.highFrequencyAccelerationLimit,
    stylusTracingLimit: state.stylusTracingLimit,
    acousticEffects: state.acousticEffects,
    surfaceEffects: state.surfaceEffects,
    pointerToAudioLatencyMs: state.pointerToAudioLatencyMs,
    audioBaseLatencyMs: state.audioBaseLatencyMs,
    audioOutputLatencyMs: state.audioOutputLatencyMs,
    audioPlaybackStats: state.audioPlaybackStats,
    recordProfile: state.recordProfile,
    payloadContainer: state.payloadContainer,
    releaseId: state.releaseId,
    recordHash: state.recordHash,
    recordImageUrl: state.recordImageUrl,
    rotationDegrees: state.rotationDegrees,
    sampleRate: state.sampleRate,
    outputSampleRate: state.outputSampleRate,
    cleanEnd: state.cleanEnd,
    positionFrames: state.positionFrames
  });
});

unsubscribe();
```

`player.getState()` returns the same immutable snapshot immediately.

`sampleRate` is the source-PCM clock; `outputSampleRate` is the AudioContext
clock. `effectiveRate` is the signed rendered rate (`1` is nominal and a
negative value is reverse). Gate gain/target are in `0..1`, direction is
`-1`, `0` or `1`, and gate phase/stroke progress are travel-derived rather than
wall-clock-derived. The latency fields are telemetry snapshots and may be
`null` until the browser or an input command supplies the corresponding
measurement. During a published record ending, `deadwaxProgress` reaches `1`
after the two-turn traversal while `deadwaxActive` remains true for the
persistent lock.

`audioPlaybackStats` normalizes Chromium's current `playbackStats` and legacy
`playoutStats` APIs. Its stable shape is:

```js
{
  supported: true,
  api: "playbackStats",
  underrunEvents: 0,
  underrunDurationMs: 0,
  totalDurationMs: 12046.202,
  averageLatencyMs: 35.019,
  minimumLatencyMs: 0,
  maximumLatencyMs: 35.054
}
```

Unsupported browsers return `supported: false` and `null` measurements. Do not
interpret missing statistics as zero underruns.

## Scratch performances

A saved scratch is a versioned stream of engine commands, not rendered audio
and not raw pointer coordinates. Schema v2 names both clocks explicitly:
`sourceSampleRate` defines `positionFrames`, while `outputSampleRate` defines
event `frameOffset` and `durationFrames`. The AudioWorklet schedules those
events at their exact output-frame boundaries, including boundaries inside a
128-frame render quantum. Validation caps a take at 65,536 events and rejects
more than 128 events in any 128-frame output window so an imported performance
cannot monopolize the realtime thread.

A v2 recording also carries a stable replay seed, initial platter angle, gate
algorithm version, preset and click count, manual-crossfader position, fader
curve, and both HF/stylus limit strengths. Preset, click and
manual-crossfader changes are frame-timed events. The essential shape is:

```js
{
  schemaVersion: 2,
  sourceSampleRate: 48000,
  outputSampleRate: 48000,
  durationFrames,
  replaySeed,
  engine: { version: 5, gateAlgorithmVersion: 3 },
  initialState: {
    positionFrames,
    rotationDegrees,
    preset: "flare",
    clicks: 1,
    manualCrossfader: 0.5,
    highFrequencyAccelerationLimit: 0.35,
    stylusTracingLimit: 0.72
  },
  events: [
    { type: "scratch-motion", frameOffset, positionFrames, rate, impulse, grip },
    { type: "manual-crossfader", frameOffset, value }
  ]
}
```

Replay snapshots the live Rust DSP, then starts the take from its recorded seed
and platter angle. Rust resets the replay-only mechanical, wow/flutter, surface,
filter, limiter and gate dynamics. The same take therefore produces the same
gate trace and rendered bytes even after live playback advances. Completion or
cancellation restores the exact live snapshot.

Schema v1 remains readable. Migration treats its single `sampleRate` as both
legacy clocks, then scales source positions and output event offsets/duration
independently onto the current clocks. Missing gate data becomes `baby` with
one click and the sharp manual-fader curve; the two newer limit strengths are
set to exact bypass (`0`) so an old take does not acquire new coloration. An
older take derives a stable replay seed from its ID and defaults its missing
platter angle to zero.

Start and stop the default recorder:

```js
player.startScratchRecording({ name: "flare take 1" });

// Scratch through the canvas or the programmatic scratch API.

const performance = await player.stopScratchRecording({ save: true });
```

Manual recorder lifecycle:

```js
const recorder = player.createScratchRecorder({ name: "orbit" });
recorder.start();
const performance = recorder.stop();
await player.scratches.save(performance);
```

Replay modes:

```js
await player.replayScratch(performance, { effects: "original" });
await player.replayScratch(performance, { effects: "dry" });
await player.replayScratch(performance, {
  effects: { acoustic: true, surface: false }
});

player.cancelScratchReplay();
```

`dry` retains the recorded mechanics—position, direction, rate, motor handoff,
spring, gate, manual-fader events and interpolation—but disables the selectable
acoustic and surface effect groups. The independently recorded programme HF
limit remains governed by its stored strength; set that strength to `0` for an
exact bypass.

Persistence API:

```js
await player.scratches.save(performance);
const saved = await player.scratches.get(performance.id);
const list = await player.scratches.list({ limit: 50 });
await player.scratches.delete(performance.id);
await player.scratches.clear();

const json = player.scratches.export(performance);
const imported = player.scratches.import(json);
```

IndexedDB details:

```text
database: vin.yl.player
store:    scratch-performances
version:  2
indexes:  recordHash, createdAt, [recordHash, createdAt]
```

`list()` and `clear()` are scoped to the currently loaded record unless a query overrides `recordHash`.

## Canvas UI

The canvas is optional and consumes only the public player API. It does not reach into WASM, PCM banks, decoder state or the AudioWorklet.

```js
const canvas = document.querySelector("#player-canvas");
const controller = player.canvas.mount(canvas, {
  components: {
    crossfader: false
  },
  theme: {
    background: "transparent"
  },
  strobeLightOn: true
});
```

The bundled page mounts its canvas automatically. Radial `SCRATCH` and `CLICKS`
buttons cycle through every preset and click count with pointer or touch input.
The advanced dropdown contains the equivalent native HTML controls plus
keyboard-operable `NEXT` and `+1` buttons. It starts collapsed and remains in
the initial viewport in the default embed.

### Components

Every visual/control group can be toggled independently:

```js
player.canvas.configure({
  components: {
    record: true,
    syncRings: true,
    strobeLamp: true,
    spindle: true,
    stylus: true,
    needlePoint: true,
    tonearmGuide: true,
    startStop: true,
    needle: true,
    rpm: true,
    volume: true,
    crossfader: true,
    scratchPreset: true,
    scratchClicks: true,
    seek: true,
    labels: true
  }
});
```

Or individually:

```js
player.canvas.setComponentVisible("crossfader", false);
player.canvas.setComponentVisible("tonearmGuide", false);
player.canvas.setStrobeLight(true);
```

### Theme

```js
player.canvas.setTheme({
  background: "transparent",
  line: "#050505",
  mutedLine: "rgba(5,5,5,0.28)",
  controlFill: "rgba(255,255,255,0.08)",
  controlActive: "#050505",
  controlText: "#050505",
  controlActiveText: "#f00020",
  accent: "#00bfd3",
  accentSecondary: "#ef035c",
  accentTertiary: "#f3b511",
  recordFallback: "transparent",
  turntableRing: "rgba(5,5,5,0.18)",
  turntableRingStrong: "rgba(5,5,5,0.34)",
  label: "transparent",
  syncDot: "rgba(0,0,0,0.23)",
  syncLit: "#00bfd3",
  lamp: "#00bfd3",
  tonearm: "#050505",
  tonearmGuide: "rgba(5,5,5,0.35)",
  stylus: "#00bfd3",
  stylusGlow: "rgba(0,191,211,0.65)"
});
```

The current canvas configuration is available through `player.canvas.getConfig()`. Call `player.canvas.destroy()` to unmount it.

Record motion is processed by the DOM-free tracker in
`web/scratch-gesture.js`. The canvas feeds every sample returned by
`getCoalescedEvents()` through it in order, using incremental angle unwrap so a
gesture can span any number of turns. It uses a 4 ms differentiation floor,
approximately 35 ms steady-state smoothing with a faster reversal path,
direction hysteresis and a near-spindle guard. A lifted needle permits visual
rotation without advancing the groove readhead. Grip reaches Rust slipmat
mechanics and is stored in deterministic takes. Explicit grip wins; pen
pressure supplies it when available. Mouse and finger touch, including iPhone
Haptic Touch, default to full grip because generic touch pressure is not a
reliable force signal.

The canvas is the only bundled record-gesture surface. The pages do not retain
a hidden alternate platter or a second rate differentiator, so multi-turn
unwrap, impulse qualification, pressure policy and cancellation cannot diverge
between bundled input paths.

Canvas pointer ownership is keyed by pointer ID rather than a single global
gesture. One pointer can therefore hold the record while another adjusts XFADE,
CH or PITCH; releasing the control pointer does not release the record pointer.

The physical strobe dots always rotate. A separate calibrated sample appears in
the diffuse lamp beam. The matching row appears stationary in the beam. The same
dots remain visibly in motion outside the beam.

## Acoustics

The acoustic engine is implemented in `src/acoustic.rs` and exported as `ScratchAcousticDsp` by the main `record-player` crate. The AudioWorklet is the only browser component that instantiates it.

### One transport and acoustic clock

Normal playback and scratching use the same rendered groove position. Motor spin-up, braking, grabbing, reversing, releasing and seeking do not switch between unrelated audio engines. The Rust DSP position is the authoritative audio clock. The host mirrors it into public state and the canvas uses that state to move the record and stylus.

There are two control conditions:

- **motor control**: the platter moves toward a motor-delivered target rate
- **hand control**: the target position and rate come from scratch events.

Both feed the same damped rate spring:

| Constant | Current value | Role |
| --- | ---: | --- |
| `RATE_SPRING_OMEGA` | `70 rad/s` | Rate coupling stiffness. |
| `RATE_SPRING_ZETA` | `0.85` | Slightly underdamped response for a small reversal snap. |
| `MOTOR_SPINUP_SECONDS` | `0.30 s` | Motor delivery ramp from rest. |
| `MOTOR_BRAKE_SECONDS` | `0.32 s` | Motor delivery ramp toward zero. |
| `GRIP_ATTACK_SECONDS` | `0.012 s` | Fast hand ownership for a deliberate grab. |
| `GRIP_RELEASE_SECONDS` | `0.045 s` | Hand ownership release. |
| `POSITION_CATCHUP_SECONDS` | `0.28 s` | Gentle position-error correction while scratching. |
| `STILL_SNAP_SECONDS` | `0.03 s` | Collapses residual target error when the hand becomes still. |
| `DEADZONE_RATE` | `0.006` | Below this rate the cartridge output is silent. |

The old acoustics document listed a `0.22 s` motion hold. The current standalone engine uses `MOTION_HOLD_SECONDS = 0.05 s` and `MOTION_HOLD_RELEASE_SECONDS = 0.06 s`.

Releasing a hand while the motor is off preserves the signed platter throw and
lets it decay through the separate `0.85 s` bearing-friction model. It is not
treated as an explicit powered brake.

### Stylus sampling

At cueing and normal speeds, each output frame samples the fractional groove
position with four-point Catmull-Rom interpolation. Above `1.05×` source-frame
step, the renderer blends toward a 24-tap Blackman-windowed sinc whose cutoff
tracks the actual source step; at `1.5×` and above, the band-limited path is used
fully. This adaptive path works in both directions and suppresses high-speed
aliasing while retaining the low-latency cubic response near normal speed. The
local cubic neighborhood also supplies the slope and curvature estimates used
by the source-texture layer.

A separate stylus-tracing stage models the physical readhead rather than the
programme master. Its speed-dependent one-pole low-pass has:

- maximum cutoff: `19 kHz`
- nominal-speed knee: `0.95×`
- slow movement becomes progressively duller
- tracing loss above `2.5×`, with additional soft reduction driven by source
  curvature × squared travel velocity.

`stylusTracingLimit` controls only that curvature/velocity contribution. Its
default is `0.72`; `setStylusTracingLimit(strength)` accepts `0..1`, where `0`
bypasses the added soft tracing limit.

Movement gain is zero inside the deadzone and otherwise remains bounded between `0.68` and `1.08`, with a small presence lift near true speed.

### Programme upper-band acceleration limiter

`highFrequencyAccelerationLimit` is a different process. It is a true
programme-signal limiter, not a playback-speed or cartridge-tracing model. A
complementary split at `5.2 kHz` leaves the base band unchanged while a
stereo-linked detector measures upper-band second-difference energy and rapid
reversals of the upper signal's sample-to-sample velocity. Only the
complementary upper residual is reduced
through a soft knee, with a `0.12 ms` attack, `32 ms` release and a bounded
minimum upper gain of `0.16` at full strength/overload.

The default strength is `0.35`. `setHighFrequencyAccelerationLimit(strength)`
accepts `0..1`; `0` is an exact sample-for-sample bypass. Source texture,
surface noise and needle foley are mixed outside this limiter, so a harsh
programme transient cannot pull down those layers or the full-band lows and
mids. The bundled page exposes the scratch technique, click count and both
independent strength controls under **ADVANCED CONTROLS**.

### Wow and flutter

Wow and flutter alter the sampled source position rather than running as a post-effect:

- wow is revolution-locked: `1.8 s` at 33⅓ RPM and `1.333… s` at 45 RPM
- flutter defaults to `6.4 Hz`
- wow phase follows record motion, so it slows and reverses with the groove
- depth is `0.0012 × clamp(|rate|, 0, 1.2)`
- flutter depth is `0.22` of wow depth
- modulation is disabled below `|rate| = 0.18`.

`AcousticConfig` allows the maximum rate, wow period, flutter frequency,
acoustic/surface effect groups, `stylusTracingLimit` and
`highFrequencyAccelerationLimit` to be configured when the DSP is constructed.
The browser host publishes runtime setters for the two strengths and switches
effect groups during scratch replay through `setEffects`.

### Manual crossfader and scratch gate

The manual crossfader and assisted scratch gate are independent gain stages:

```text
deck gain = channel gain × manual crossfader curve × scratch-technique gate
```

Changing a technique does not move or overwrite the manual XFADE value. The
gate runs once per output frame inside `ScratchAcousticDsp`; it uses filtered
hand intent for responsive direction changes. Confirmed-direction rendered
travel controls pattern phase. Residual outgoing motion cannot spend the new
stroke pattern before the audible groove reverses. Phase freezes at rest and
resets on a confirmed reversal. It does not use `requestAnimationFrame` or wall
clock time.
Its short speed-adaptive envelope removes discontinuities at gate edges. The
gate is applied after programme and foley are mixed. During performance replay,
frame-timed manual-fader events are converted through the recorded sharp curve
and applied as a separate post-gate Rust gain, rather than being folded into the
technique state.

The eight profiles and their click defaults are:

| Preset | Default clicks | Intent |
| --- | ---: | --- |
| `baby` | 1 | Gate remains open; the manual fader is authoritative. |
| `stab` | 1 | Forward travel opens, reverse/hold cuts. |
| `chirp` | 1 | Direction-aware opening and closing within each stroke. |
| `transform` | 2 | Repeated travel-locked chops. |
| `flare` | 1 | An open phrase with short closed notches. |
| `crab` | 4 | Rapid travel-locked open pulses. |
| `orbit` | 2 | Symmetric flare-style notches in both directions. |
| `drum` | 1 | Short velocity-qualified onset, reversal and high-acceleration attacks. |

Click counts are integer-clamped to `1..8`. Selecting a preset restores that
preset's default click count; a later click-count change adjusts the pattern
without moving the manual crossfader.

### Surface and handling layers

All layers are deliberately reduced near true-speed playback and become more apparent during handling:

| Layer | Current source | Current tuning |
| --- | --- | --- |
| Contact bed | Filtered pseudo-random noise | `CONTACT_NOISE_GAIN = 8e-5`, with a 94% dip around `1×`. |
| Groove grain | Position-keyed deterministic noise | Repeats at the same groove position when scrubbed backwards and forwards. |
| Source texture | Local waveform slope and curvature | `SOURCE_TEXTURE_GAIN = 1.8e-4`. |
| Dust flecks | Sparse position-keyed cells | `DUST_FLECK_GAIN = 4.5e-5`. |
| Acceleration texture | Difference between current and previous effective rates | Mixed into the source-texture response. |
| Contact impulse | Explicit gesture impulse plus decay | `CONTACT_IMPULSE_DECAY = 0.985`. |

Because groove grain and dust are keyed to source position, their texture is spatially stable rather than being unrelated white noise on every pass.

### Effect groups

The DSP exposes two replay-selectable groups:

- **acoustic**: wow/flutter, stylus drag/tracing response, movement gain and programme-correlated source texture
- **surface**: contact bed, deterministic groove grain, dust and contact impulses.

Original replay enables both. Dry replay disables both while retaining
mechanical motion, gate/fader actions and interpolation. A custom object can
enable either group independently. The programme upper-band acceleration
limiter is a separate mastering/safety control restored from the recording's
initial state; it is not switched by these two effect flags.

### Needle lift and needle point

Needle lift mutes cartridge output without requiring the visual platter to stop. The canvas tonearm and stylus are presentation components driven from player state. Dragging the needle point seeks through `player.seekRatio`. It does not directly mutate the AudioWorklet or transport internals.

During active programme playback, the cue resolves one randomized landing
`50–140 ms` before the visual aim. The immediate worklet update and the queued
core seek use that same immutable landing. Paused or lifted-needle seeks land
exactly on the visual aim.

The host decodes `web/assets/audio/needle-surface.opus` off the real-time thread
and gives it to the Rust DSP. Needle placement adds a short synthesized thump
and a filtered crackle excerpt. The DSP provides filtered surface beds for
lead-in and deadwax traversal. Mobile speaker compensation follows the original
`2.25×` surface-gain rule. If the asset is unavailable, a bounded synthetic
bed/burst path remains available.

Lead-in and run-out durations are converted to output frames before the region
starts, so their boundaries are driven by the audio clock rather than a main
thread timer. Starting playback runs two lead-in turns. Surface-only rendering
advances platter motion but freezes the programme readhead and never samples
the first or last programme audio under the foley. The worklet pre-arms the end
policy and splits the final programme quantum at the exact terminal frame:
published playback renders deadwax in the remaining frames without resetting
the Rust platter model, while a clean preview zeros that suffix. Published
records default to two deadwax turns followed by a persistent run-out lock;
`loadAudioFile(..., { cleanEnd: true })` instead stops at programme end.

The Rust renderer returns the exact programme-prefix length from that final
quantum, so terminal detection remains one Rust/WASM render call rather than a
per-frame interop loop. Scratch replay likewise snapshots its small dynamic DSP
state inside Rust (never the PCM window) and restores platter inertia, filters,
gate, fader and final output-gain state at the scheduled completion frame. A replay remains an
explicit host transaction until the worklet acknowledges restoration. Live
transport, seek, fader, RPM and advanced-control actions interrupt replay first,
so a restored snapshot cannot silently overwrite the user's newer command.

### Window stability

The DSP owns only the current source window, not the full decoded record. It requests a replacement window when the rendered or projected stylus position approaches an edge.

Current tuning:

| Constant | Value |
| --- | ---: |
| Shared bank duration | `12 s` |
| Source chunk duration | `1 s` |
| `WINDOW_REQUEST_MARGIN_SECONDS` | `0.75 s`, speed-scaled |
| `WINDOW_REQUEST_PROJECT_SECONDS` | `0.18 s` |
| `WINDOW_MISS_FADE_SECONDS` | `0.006 s` |

The PCM worker performs the authoritative 24-sample Hermite seam repair before
copying a region into either bank. Progressive availability is tracked as a
contiguous written range, so an undecoded tail is never exposed as valid
zero-filled PCM. At rates above `2×`, request checks are throttled to roughly
`30 ms`; otherwise they run at roughly `80 ms`. Requests are coalesced while a
bank is being filled or applied. The worklet fades through a short window miss
instead of abruptly holding or exposing incomplete data.

### Scratch replay resolution

Source positions and event timing use distinct clocks. `positionFrames` is in
`sourceSampleRate` frames; `frameOffset` and `durationFrames` are in
`outputSampleRate` frames on the AudioContext clock. The worklet receives the
complete normalized performance and divides processing at any event boundary
inside the current Web Audio render quantum. It does not wait for a main-thread
timer or animation frame.

This is at least as precise as the legacy telemetry format, which intentionally
throttled pointer-derived events. The current format records the normalized
commands actually sent to the engine. Pointer commands use the pointer event's
projected output frame, not the later main-thread send or worklet-application
time, so capture preserves gesture intent while latency remains measurable.
New captures identify this timing contract as engine version `5`; earlier takes
remain valid because stored frame offsets replay unchanged.

### Motion and canvas sync

Rust integrates platter phase from the rate that actually rendered. The host
publishes that phase and effective rate. The canvas anchors to the phase and
uses the effective rate only to draw smoothly between worklet messages. It
therefore follows spin-up, braking, pitch slew, reverse motion, release and wow
instead of advancing at an assumed nominal RPM. A live pointer remains the
immediate visual authority while a hand owns the record. The canvas never
writes directly to DSP memory. The strobe renderer is visual calibration rather
than an audio clock:

- physical rows rotate continuously
- only the dots under the lamp receive the calibrated stroboscopic sample
- the row matching the current pitch appears steady under the lamp
- the same dots remain visibly moving outside the beam.

Published records with programme gaps use their decoded radial gap anchors for
the default tonearm. Rust validates the anchors and owns the monotone
sample-to-groove map and its inverse. Needle dragging projects the pointer onto
the physical arm length before recovering groove radius, so a visible gap band
maps back into the matching silent PCM interval. Records without gaps retain
the linear path and do not load the extra presentation-side WASM instance.

## Human validation status

Automated tests can establish deterministic state transitions, bounded memory,
interpolation response and gate timing, but they cannot establish that the deck
feels like vinyl under a DJ's hand or that its surface/acoustic treatment is
perceptually preferable. No human-study result is claimed here. The outstanding
blind listening, control-task and free-performance procedure is defined in
[`DJ_VALIDATION_PROTOCOL.md`](./DJ_VALIDATION_PROTOCOL.md), including hardware,
level matching, failure reporting and acceptance criteria.

The repository includes a build-bound collection console. Build from a clean
worktree, start the development server, and open the console:

```sh
npm run build
npm run dev
```

```text
http://localhost:5193/dj-validation.html
```

The console embeds the real player. It records the hardware chain, physical
loopback, participant blocks, browser playback statistics, pointer-command
latency, ABX trials, live routines, preflight declarations and artifact hashes.
It saves a local draft and exports schema-version-3 JSON. It also exports the
captured movement trace with a browser-computed SHA-256 hash.

The console refuses release measurements and audio blocks if the build came
from a dirty worktree, if the draft commit differs from the running build, or
if a shipped acoustic, surface, limiter, or fader setting has changed. The
console invalidates a block if these settings change during collection.

Blind listening uses a separate coordinator/operator workflow. The coordinator
prepares one package per participant from fresh, matched physical and player WAV
captures:

```sh
npm run validation:prepare-abx -- dj-01-spec.json \
  --build-info dist/player-build-info.json \
  --out dj-01-blind-package \
  --codebook private/dj-01-codebook.json
```

The build metadata must come from the clean candidate that made the player
captures. The package binds its commit, settings, and metadata hash. The operator
opens `http://localhost:5193/dj-abx.html` and selects the blind package directory.
The runner never loads the private codebook. It verifies all opaque audio-file
hashes, saves a condition-free local draft, and requires A, B, and X playback
before it accepts a response.

Preserve the exact `dist/player-build-info.json` bytes in the evidence directory.
Before the first decode, record that file and its SHA-256 digest as the
`candidate-build-info` artifact in the collection console. The decoder matches
the digest; the final analyzer also parses the file and verifies its clean
commit and settings.

After responses, exclusions and cue coding are frozen, create the cue-code file
and decode the participant into the main collection:

```sh
npm run validation:cue-template -- blind-abx-dj-01.json \
  --out dj-01-cue-codes.json
npm run validation:decode-abx -- private/dj-01-codebook.json \
  blind-abx-dj-01.json \
  --cue-codes dj-01-cue-codes.json \
  --results dj-validation-results.json \
  --out dj-validation-results.dj-01.json
```

Two blinded coders must complete any empty cue codes before the decode step.
The decoder rejects a different candidate and audio reuse across participants.
The audio check ignores WAV metadata changes.
The detailed package schema and coordinator procedure are in
[`DJ_VALIDATION_PROTOCOL.md`](./DJ_VALIDATION_PROTOCOL.md).

The command-line template remains available for offline collection. Generate a
template and analyze a frozen result file with:

```sh
npm run validation:template > dj-validation-results.json
npm run validation:analyze -- dj-validation-results.json
```

The analyzer validates pinned engine settings, streams and verifies artifact
hashes, preflight results and zero-underrun audio blocks. It also checks trial
balance, exact ABX statistics, repeated cues and live-control criteria. Its
report includes the SHA-256 digest of the input file.

The player also includes a physical-loopback latency probe. Stop the transport,
route output to the selected interface input or microphone and run:

```js
const loopback = await player.measureAcousticLoopbackLatency({
  inputDeviceId,
  repetitions: 5,
});
```

The probe uses an AudioWorklet frame clock and does not route microphone audio
to the output. The study file records its full result as
`environment.acousticLoopback`. The Chrome smoke covers a software loopback;
only a physical run can supply release evidence.

The result is round-trip output-to-input latency. It is not a one-way output
latency estimate. The registered physical gate is p95 at or below `30 ms`, with
jitter at or below `3 ms`.

The current engineering gap audit is in
[`REFERENCE_ENGINE_GAP_AUDIT.md`](./REFERENCE_ENGINE_GAP_AUDIT.md). It compares
this engine with `../yl.vin/apps/play` and separates deliberate improvements
from remaining proof work.

## Rust API

The main crate exports the transport types plus:

```rust
pub use acoustic::{
    AcousticConfig,
    AcousticStatus,
    ScratchAcousticDsp,
};
pub use scratch_gate::{
    ScratchGate,
    ScratchPreset,
    MAX_SCRATCH_CLICKS,
    MIN_SCRATCH_CLICKS,
};
```

The root crate can be tested natively:

```bash
cargo test
```

Run the real Chrome AudioWorklet and two-touch smoke test with:

```bash
npm run test:browser
```

Build the complete browser application through `scripts/build.mjs`. It supplies the `wasm` feature and correct output names.

## Deployment checklist

A static deployment must preserve:

- `application/wasm` for `.wasm` files
- JavaScript MIME types for `.js` and `.mjs`
- COOP `same-origin`
- COEP `require-corp`
- same-origin access to workers, WASM, ONNX models and `.data` files
- all files generated under `dist/wasm`
- the copied decoder helper scripts at the root of `dist`.

Do not open `dist/index.html` with `file://`. Workers, modules, AudioWorklet and cross-origin isolation require an HTTP server.

## Additional API reference

See [`API.md`](./API.md) for the method-by-method API notes. The source of truth remains `web/player-host.js`, `web/player-canvas.js`, `web/player-worklet.js` and `src/acoustic.rs`.

## Publishing `record-player`

The root crate is the workspace default and is publishable. The local `player-wasm` workspace member is marked `publish = false`, so it cannot be accidentally uploaded to crates.io.

```bash
cargo package -p record-player
cargo publish -p record-player
```

Publishing `record-player` does not package or publish `player-wasm`. It is not a dependency of the root crate.

## Motor and needle behavior

The platter motor is independent from program playback. `START - STOP` can always start or stop the turntable, including before a record finishes decoding and while the needle is raised. The canvas record and strobe rings follow motor state rather than the audio readhead. Once the first decoded PCM chunk is available, lowering the needle onto a running platter begins playback. Lifting it silences/freezes the groove position without stopping the visible platter.

## Message tracing

Structured message tracing is enabled by default in this diagnostic build. It
logs each host action and worker message. Each entry contains a sequence number,
subsystem, direction, type, elapsed time, and compact payload summary.

Disable it before loading the player with:

```html
<script>globalThis.__VIN_YL_PLAYER_LOGGING__ = false;</script>
```

Or append `?player_log=0` to the player URL. It can also be changed at runtime:

```js
vin.yl.player.setLogging(false);
vin.yl.player.setLogging(true);
console.log(vin.yl.player.loggingEnabled);
```

The shared implementation is `web/player-message-logger.js`. Payload summaries report buffer types and byte lengths rather than printing PCM or PNG contents.


## Shared message logger loading

`player-message-logger-global.js` contains the export-free logger implementation used by classic workers. `player-message-logger.js` is the ES-module adapter used by the host, module workers, and AudioWorklet. Both share `globalThis.VinylPlayerMessageLogger`. Do not load the ES-module adapter with `importScripts()`.

### EnCodec chunk seam repair

`player-wasm` delivers decoded revolution chunks without the cropped
encoder-side ±10 ms context. The PCM-window worker places each owned chunk at
its exact source timeline offset and applies a deterministic 24-sample cubic
Hermite repair at each contiguous chunk boundary before any Float32 window is
published. At 48 kHz, this repair is 0.5 ms.

The repair replaces 12 samples on each side of the join. It preserves total
length and later chunk offsets. It estimates endpoint slopes from samples
outside the repair span. It clamps interpolation overshoot to the PCM range.
