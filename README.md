# vin.yl.player

A standalone Bitneedle picture-record player with a Rust transport model, a Rust acoustic scratch engine, a player-only record decoder, shared-memory PCM windowing, frame-timed scratch-performance replay, and an optional canvas turntable UI.

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

- Bitneedle PNG inspection and ECDC decoding;
- Rust/WASM transport and acoustic rendering;
- motor playback, braking, pitch/RPM changes, scratching and needle lift;
- double-buffered shared-memory PCM windows;
- IndexedDB PCM caching;
- frame-timed scratch-performance capture, persistence and replay;
- a configurable radial canvas UI with a stylus, concentric turntable, Technics-style pitch control, strobe rows and lamp.

It does **not** currently include the legacy application's TAPE master monitor, remote scratch sessions, two-deck playback, lead-in/deadwax foley asset, HTML-audio fallback, waveform UI, sample library, or authoring tools.

## Requirements

Install:

- Node.js 20 or newer;
- Rust and Cargo;
- the `wasm32-unknown-unknown` Rust target;
- `wasm-pack`;
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

The decoder helper scripts (`browser-formatting.js`, `encodec-bundle-names.js`, `onnx-runtime-session.js`, `onnx-worker-tensors.js`, `ecdc-pcm-layout.js`, `player-cache-config.js`, `player-cache.js`, `player-pcm-helpers.js`) live directly in `app/src` — this repo owns them, they are not vendored from elsewhere.

The browser build additionally needs ONNX Runtime Web assets and the EnCodec ONNX bundles. Their locations can be supplied with environment variables; by default they're read from `vendor/wasm/` in this repo.

## Environment variables

| Variable | Required | Default | Purpose |
| --- | --- | --- | --- |
| `BITNEEDLE_ONNX_RUNTIME_DIR` | No | `vendor/wasm/onnxruntime-web` | ONNX Runtime Web distribution copied into `app/dist/wasm/onnxruntime-web`. |
| `BITNEEDLE_ENCODEC_BUNDLES_DIR` | No | `vendor/wasm/encodec-rs/bundles` | EnCodec ONNX bundles; small `bundle.json` manifests are copied into `app/dist/wasm/encodec-rs/onnx-bundles`, the large model weights are uploaded to R2 separately (see `make sync-onnx-assets`). |
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

1. clears `app/dist`;
2. copies `app/src` into `app/dist`;
3. builds the root `record-player` crate into `app/dist/wasm/record-player`;
4. builds `player-wasm` into `app/dist/wasm/player-wasm`;
5. copies the shared decoder scripts;
6. copies ONNX Runtime Web and the EnCodec bundles.

Generated browser modules:

```text
app/dist/wasm/record-player/record_player.js
app/dist/wasm/record-player/record_player_bg.wasm
app/dist/wasm/player-wasm/player_wasm.js
app/dist/wasm/player-wasm/player_wasm_bg.wasm
```

## Runtime WASM Bundles

The player uses separate browser contexts, so the runtime is split across a few focused WASM bundles:

- **`record-player`**
  - files:
    - `app/dist/wasm/record-player/record_player.js`
    - `app/dist/wasm/record-player/record_player_bg.wasm`
  - loaded by:
    - `app/src/player-host.js`
    - `app/src/player-worklet.js`
  - responsibility:
    - transport state
    - playback engine init
    - `ScratchAcousticDsp` in the AudioWorklet

- **`player-wasm`**
  - files:
    - `app/dist/wasm/player-wasm/player_wasm.js`
    - `app/dist/wasm/player-wasm/player_wasm_bg.wasm`
  - loaded by:
    - `app/src/record-decoder-worker.js`
    - `app/src/opus-cache.js`
  - responsibility:
    - record header and descriptor decode
    - playback metadata helpers
    - cache/tape encryption helpers
    - per-packet SoundKit v2 header build and parse

- **`onnxruntime-web`**
  - files under:
    - `app/dist/wasm/onnxruntime-web/`
  - loaded by:
    - `app/src/record-decoder-worker.js`
  - responsibility:
    - ONNX execution for the decoder worker

- **EnCodec ONNX bundles**
  - files under:
    - `app/dist/wasm/encodec-rs/onnx-bundles/`
  - loaded by:
    - `app/src/record-decoder-worker.js`
  - responsibility:
    - model/data bundles used by the ECDC decode path

In practice:

- the **main thread** coordinates UI, state, and startup;
- the **decoder worker** loads `player-wasm`, `onnxruntime-web`, and the EnCodec bundles;
- the **AudioWorklet** loads `record-player`;
- the **tape/cache helper path** loads `player-wasm`.

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
    │  alternating 12-second SharedArrayBuffer banks
    ▼
player-worklet.js
    │  record-player / ScratchAcousticDsp
    │  motor, hand control, interpolation, acoustic texture
    ▼
GainNode
    ▼
AudioContext destination
```

The main thread routes commands and publishes state. It does not assemble a whole floating-point record for the AudioWorklet. The PCM-window worker fills one inactive shared bank while the worklet reads the other, then announces a completed window swap.

The current bank length is twelve seconds at the source sample rate. Source chunks are one second. The Rust engine requests replacement windows before the stylus reaches a bank edge, with extra projection in the current direction of travel. A six-millisecond fade masks a temporary window miss rather than producing a hard discontinuity.

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

`loadRecord` expects a browser `File` containing a Bitneedle PNG.

### Deck controls

```js
await player.setRpm(45);
await player.setVolume(0.8);
await player.setCrossfader(0.5);
```

- RPM is clamped to `16..90` by the host.
- Volume is clamped to `0..1`.
- Crossfader is clamped to `0..1`.

The canvas presents pitch as a Technics-style ±8% control around the record's native RPM, but the lower-level API accepts an absolute RPM.

### Programmatic scratching

```js
await player.beginScratch({
  pointerId: 1,
  positionFrames: player.getState().positionFrames,
  rotationDegrees: 0,
  rate: 0,
  impulse: 0.22
});

await player.updateScratch({
  positionFrames: 120000,
  rotationDegrees: -18,
  rate: -0.8,
  impulse: 0.2
});

await player.endScratch({
  rotationDegrees: -18,
  resumePlayback: true
});
```

Pointer geometry belongs in the UI layer. The player API accepts normalized engine commands expressed in source frames, rates and rotation degrees.

### State subscription

```js
const unsubscribe = player.subscribe(state => {
  console.log({
    ready: state.ready,
    playing: state.playing,
    needleLifted: state.needleLifted,
    scratching: state.scratching,
    positionSeconds: state.positionSeconds,
    durationSeconds: state.durationSeconds,
    positionRatio: state.positionRatio,
    rpm: state.rpm,
    nativeRpm: state.nativeRpm,
    playbackRate: state.playbackRate,
    volume: state.volume,
    crossfader: state.crossfader,
    recordProfile: state.recordProfile,
    payloadContainer: state.payloadContainer,
    releaseId: state.releaseId,
    recordHash: state.recordHash,
    recordImageUrl: state.recordImageUrl,
    rotationDegrees: state.rotationDegrees,
    sampleRate: state.sampleRate,
    positionFrames: state.positionFrames
  });
});

unsubscribe();
```

`player.getState()` returns the same immutable snapshot immediately.

## Scratch performances

A saved scratch is a versioned stream of engine commands, not rendered audio and not raw pointer coordinates. Events are timestamped in audio frames and replayed inside the AudioWorklet, including events that fall partway through a 128-frame render quantum.

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

`dry` retains the recorded mechanics—position, direction, rate, motor handoff, spring and interpolation—but disables acoustic coloration and surface layers.

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

The bundled page mounts its canvas automatically.

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

The physical strobe dots always rotate. Inside the diffuse lamp beam, a separately calibrated sample is drawn so the matching row appears stationary while the same dots remain visibly in motion outside the light.

## Acoustics

The acoustic engine is implemented in `src/acoustic.rs` and exported as `ScratchAcousticDsp` by the main `record-player` crate. The AudioWorklet is the only browser component that instantiates it.

### One transport and acoustic clock

Normal playback and scratching use the same rendered groove position. Motor spin-up, braking, grabbing, reversing, releasing and seeking do not switch between unrelated audio engines. The Rust DSP position is the authoritative audio clock; the host mirrors it into public state and the canvas uses that state to move the record and stylus.

There are two control conditions:

- **motor control**: the platter moves toward a motor-delivered target rate;
- **hand control**: the target position and rate come from scratch events.

Both feed the same damped rate spring:

| Constant | Current value | Role |
| --- | ---: | --- |
| `RATE_SPRING_OMEGA` | `70 rad/s` | Rate coupling stiffness. |
| `RATE_SPRING_ZETA` | `0.85` | Slightly underdamped response for a small reversal snap. |
| `MOTOR_SPINUP_SECONDS` | `0.30 s` | Motor delivery ramp from rest. |
| `MOTOR_BRAKE_SECONDS` | `0.32 s` | Motor delivery ramp toward zero. |
| `GRIP_ATTACK_SECONDS` | `0.10 s` | Hand ownership fade-in. |
| `GRIP_RELEASE_SECONDS` | `0.045 s` | Hand ownership release. |
| `POSITION_CATCHUP_SECONDS` | `0.28 s` | Gentle position-error correction while scratching. |
| `STILL_SNAP_SECONDS` | `0.03 s` | Collapses residual target error when the hand becomes still. |
| `DEADZONE_RATE` | `0.006` | Below this rate the cartridge output is silent. |

The old acoustics document listed a `0.22 s` motion hold. The current standalone engine uses `MOTION_HOLD_SECONDS = 0.05 s` and `MOTION_HOLD_RELEASE_SECONDS = 0.06 s`.

### Stylus sampling

Each output frame samples the source at a fractional groove position using four-point Catmull-Rom interpolation. The same neighborhood produces slope and curvature estimates used by the source-texture layer.

A speed-dependent one-pole low-pass models tracing and drag:

- maximum cutoff: `19 kHz`;
- nominal-speed knee: `0.95×`;
- slow movement becomes progressively duller;
- tracing loss starts above `2.5×` and reduces the high-frequency cutoff.

Movement gain is zero inside the deadzone and otherwise remains bounded between `0.68` and `1.08`, with a small presence lift near true speed.

### Wow and flutter

Wow and flutter alter the sampled source position rather than running as a post-effect:

- wow period defaults to `1.8 s`;
- flutter defaults to `6.4 Hz`;
- wow phase follows record motion, so it slows and reverses with the groove;
- depth is `0.0012 × clamp(|rate|, 0, 1.2)`;
- flutter depth is `0.22` of wow depth;
- modulation is disabled below `|rate| = 0.18`.

`AcousticConfig` allows the maximum rate, wow period, flutter frequency, acoustic effects and surface effects to be configured when the DSP is constructed. The browser host currently uses the defaults and switches effect groups during scratch replay through `setEffects`.

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

- **acoustic**: wow/flutter, drag/tracing response, movement gain and program-correlated source texture;
- **surface**: contact bed, deterministic groove grain, dust and contact impulses.

Original replay enables both. Dry replay disables both while retaining mechanical motion and interpolation. A custom object can enable either group independently.

### Needle lift and needle point

Needle lift mutes cartridge output without requiring the visual platter to stop. The canvas tonearm and stylus are presentation components driven from player state. Dragging the needle point seeks through `player.seekRatio`; it does not directly mutate the AudioWorklet or transport internals.

The standalone build does not currently synthesize the legacy needle-drop thump/crackle asset, lead-in static or deadwax loop described in the old application document.

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

At rates above `2×`, request checks are throttled to roughly `30 ms`; otherwise they run at roughly `80 ms`. The worklet fades through a short window miss instead of abruptly holding or zeroing a sample.

### Scratch replay resolution

Scratch events are captured against the AudioContext clock in source-sample frames. The worklet receives the complete performance and applies events at their declared frame offsets. If an event falls inside the current Web Audio render quantum, the worklet divides processing at that event boundary rather than waiting for a main-thread timer or animation frame.

This is at least as precise as the legacy telemetry format, which intentionally throttled pointer-derived events. The current format records the normalized commands actually sent to the engine and preserves their audio-frame timing.

### Motion and canvas sync

The canvas advances the visible record from the published RPM during ordinary motor playback and follows explicit rotation state during scratching. Audio remains authoritative; the canvas never writes directly to DSP memory. The strobe renderer is visual calibration rather than an audio clock:

- physical rows rotate continuously;
- only the dots under the lamp receive the calibrated stroboscopic sample;
- the row matching the current pitch appears steady under the lamp;
- the same dots remain visibly moving outside the beam.

## Rust API

The main crate exports the transport types plus:

```rust
pub use acoustic::{
    AcousticConfig,
    AcousticStatus,
    ScratchAcousticDsp,
};
```

The root crate can be tested natively:

```bash
cargo test
```

Build the complete browser application through `app/scripts/build.mjs`; it supplies the `wasm` feature and correct output names.

## Deployment checklist

A static deployment must preserve:

- `application/wasm` for `.wasm` files;
- JavaScript MIME types for `.js` and `.mjs`;
- COOP `same-origin`;
- COEP `require-corp`;
- same-origin access to workers, WASM, ONNX models and `.data` files;
- all files generated under `app/dist/wasm`;
- the copied decoder helper scripts at the root of `app/dist`.

Do not open `app/dist/index.html` with `file://`; workers, modules, AudioWorklet and cross-origin isolation require an HTTP server.

## Additional API reference

See [`API.md`](./API.md) for the method-by-method API notes. The source of truth remains `app/src/player-host.js`, `app/src/player-canvas.js`, `app/src/player-worklet.js` and `src/acoustic.rs`.
## Publishing `record-player`

The root crate is the workspace default and is publishable. The local `player-wasm` workspace member is marked `publish = false`, so it cannot be accidentally uploaded to crates.io.

```bash
cargo package -p record-player
cargo publish -p record-player
```

Publishing `record-player` does not package or publish `player-wasm`; it is not a dependency of the root crate.



### Motor and needle behaviour

The platter motor is independent from programme playback. `START - STOP` can always start or stop the turntable, including before a record finishes decoding and while the needle is raised. The canvas record and strobe rings follow motor state rather than the audio readhead. Once the first decoded PCM chunk is available, lowering the needle onto a running platter begins playback; lifting it silences/freezes the groove position without stopping the visible platter.

## Message tracing

Structured message tracing is enabled by default in this diagnostic build. Every host action and every message sent to or received from the core worker, decoder worker, and AudioWorklet is logged with a sequence number, subsystem, direction, type, elapsed time, and a compact payload summary.

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

The shared implementation is `app/src/player-message-logger.js`. Payload summaries report buffer types and byte lengths rather than printing PCM or PNG contents.


## Shared message logger loading

`player-message-logger-global.js` contains the export-free logger implementation used by classic workers. `player-message-logger.js` is the ES-module adapter used by the host, module workers, and AudioWorklet. Both share `globalThis.VinylPlayerMessageLogger`; do not load the ES-module adapter with `importScripts()`.

### EnCodec chunk seam repair

`player-wasm` delivers decoded revolution chunks after the encoder-side ±10 ms context has already been cropped away. The AudioWorklet therefore places every owned chunk at its exact source timeline offset and applies a deterministic 24-sample (0.5 ms at 48 kHz) cubic Hermite repair centred on each contiguous chunk boundary. The repair replaces 12 samples on either side of the join, preserves total length and later-chunk offsets, estimates endpoint slopes from samples outside the repair span, and clamps interpolation overshoot to the PCM range.
