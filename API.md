# vin.yl.player JavaScript API

The standalone player publishes its controller at `globalThis.vin.yl.player` and dispatches `vin.yl.player.ready` once the Rust core worker is ready.

```js
window.addEventListener("vin.yl.player.ready", event => {
  const player = event.detail;
  player.subscribe(state => renderCanvas(state));
});
```

## Record and transport

```js
await player.loadRecord(file);
await player.loadRecordFromUrl("./test.png");
await player.play();
await player.pause();
await player.togglePlayback();
player.stepTrack(1);
player.stepTrack(-1);
player.seekSeconds(42.5);
player.seekRatio(0.5);
await player.setNeedleLifted(false);
```

`loadRecord(file, { cleanEnd: false })` and `loadRecordFromUrl(url, options)`
default to the published-record ending: two output-clock-timed deadwax turns,
then a persistent run-out lock until the transport is stopped. Pass
`cleanEnd: true` to stop at the programme boundary instead.

Authoring and presave surfaces can load a conventional audio file before a
Bitneedle PNG exists. It is decoded to PCM and sent through the same Rust
transport, AudioWorklet and acoustic scratch renderer as a published record:

```js
await player.loadAudioFile(audioFile, {
  artworkUrl: URL.createObjectURL(artworkFile),
  title: "Afterglow",
  artist: "Mara Vela",
  cleanEnd: true,
});
```

`loadAudioFile` defaults `cleanEnd` to `true`; pass `false` to preview the
published deadwax behavior. Lead-in and deadwax use the surface-only Rust render
path: their boundaries are scheduled in output frames, platter motion continues,
and the programme readhead is neither advanced nor sampled under the foley.
At programme EOF the worklet splits the final quantum at the terminal frame;
deadwax fills the published-record suffix without resetting platter inertia,
while `cleanEnd` fills the suffix with zeroes. Starting either kind of loaded
record runs two lead-in turns.

To combine the rendered player audio with a canvas or camera track, request the
player's capture stream. This is the post-gain output from the existing player
graph, including acoustic scratch and surface sound:

```js
const audioStream = await player.getCaptureStream();
const socialStream = new MediaStream([
  ...socialCanvas.captureStream(30).getVideoTracks(),
  ...audioStream.getAudioTracks(),
]);
```

## Deck controls

```js
await player.setRpm(45);
await player.setVolume(0.8);
await player.setCrossfader(1);
```

RPM accepts a continuous value from 16 through 90. Volume and crossfader accept values from 0 through 1.

## Scratch controls

```js
await player.beginScratch({ pointerId, rotationDegrees });
await player.updateScratch({ positionFrames, rate, rotationDegrees, impulse });
await player.endScratch({ rotationDegrees, resumePlayback: true });

player.setScratchPreset("flare");
player.setScratchClicks(2);
```

The manual crossfader and the audio-rate technique gate are independent:

```text
deck gain = channel gain × manual crossfader curve × scratch-technique gate
```

`setScratchPreset(name)` returns the selected normalized name and resets its
default click count. `setScratchClicks(n)` returns the rounded integer click
count after clamping it to `1..8`; it never moves the manual crossfader. A new
player starts on `baby`/1 click.

| Preset | Default clicks | Gate behavior |
| --- | ---: | --- |
| `baby` | 1 | Open gate; manual fader remains authoritative. |
| `stab` | 1 | Forward travel opens; reverse/hold cuts. |
| `chirp` | 1 | Direction-aware opening/closing within a stroke. |
| `transform` | 2 | Repeated travel-locked chops. |
| `flare` | 1 | Open phrase with short closed notches. |
| `crab` | 4 | Rapid travel-locked open pulses. |
| `orbit` | 2 | Symmetric flare-style notches in both directions. |
| `drum` | 1 | Short velocity-qualified onset, reversal and acceleration attacks. |

The gate is evaluated for every output frame in Rust. Filtered hand intent
drives responsive direction decisions, while audible rendered travel drives
phase; phase freezes at rest and resets only on a confirmed reversal.

Two independent advanced controls expose different HF processes:

```js
player.setHighFrequencyAccelerationLimit(0.35); // default
player.setStylusTracingLimit(0.72);              // default
```

Both accept finite strengths clamped to `0..1`; a non-finite value throws. The
first is a stereo-linked,
`5.2 kHz` complementary programme upper-band acceleration limiter: it reacts to
rapid upper-band second differences and reversals of the upper signal's
sample-to-sample velocity, and attenuates only the upper residual through a
soft knee with a `0.12 ms` attack and `32 ms` release. It is an exact bypass at
`0` and is not driven by playback speed; lows/mids, source texture, foley and
surface-only renders remain outside it. The second controls the
separate source-curvature × travel-velocity stylus-tracing model. The bundled
page puts both sliders under **ADVANCED CONTROLS**.

Canvas code should derive pointer geometry only. It must not access the AudioWorklet, WASM instances, PCM windows, or transport clock directly.

The bundled canvas feeds `getCoalescedEvents()` samples to the canonical
`web/scratch-gesture.js` tracker in timestamp order. The tracker performs
incremental multi-turn angle unwrap, a 4 ms differentiation floor, adaptive
35 ms/fast-reversal smoothing, direction hysteresis, source-position clamping
and lifted-needle visual-only motion. Pressure and grip are additive telemetry;
callers using the programmatic methods remain responsible for deriving their own
pointer geometry.

## State

```js
const unsubscribe = player.subscribe(state => {
  state.ready;
  state.playing;
  state.motorRunning;
  state.leadInActive;
  state.deadwaxActive;
  state.deadwaxProgress;
  state.needleLifted;
  state.scratching;
  state.scratchReplayActive;
  state.buffering;
  state.positionSeconds;
  state.durationSeconds;
  state.positionRatio;
  state.rpm;
  state.nativeRpm;
  state.playbackRate;
  state.volume;
  state.crossfader;
  state.scratchPreset;
  state.scratchClicks;
  state.scratchGate;
  state.scratchGateTarget;
  state.scratchDirection;
  state.scratchMoving;
  state.scratchGatePhase;
  state.scratchStrokeProgress;
  state.effectiveRate;
  state.highFrequencyAccelerationLimit;
  state.stylusTracingLimit;
  state.pointerToAudioLatencyMs;
  state.audioBaseLatencyMs;
  state.audioOutputLatencyMs;
  state.audioPlaybackStats;
  state.sampleRate;
  state.outputSampleRate;
  state.cleanEnd;
  state.recordProfile;
  state.payloadContainer;
  state.releaseId;
  state.currentTrackIndex;
  state.currentTrackTitle;
  state.trackCount;
});
```

`sampleRate` is the source-PCM clock and `outputSampleRate` is the AudioContext
clock (or `null` before audio initialization). `effectiveRate` is signed (`1`
is nominal; negative is reverse).
`scratchReplayActive` remains true through replay setup, playback and the
worklet's Rust-state restoration acknowledgement, so live scratch input and
overlapping replays cannot race it.
`scratchGate` is the de-clicked applied gain, `scratchGateTarget` is its current
target, and `scratchDirection` is `-1`, `0` or `1`. Gate phase and stroke
progress are travel-derived values; the worklet publishes snapshots rather than
calling JavaScript once per audio frame. Browser/command latency values can be
`null` until measurable. `deadwaxProgress` reaches `1` at the end of the
two-turn traversal; `deadwaxActive` stays true during the persistent lock.

`audioPlaybackStats` has `supported`, `api`, `underrunEvents`,
`underrunDurationMs`, `totalDurationMs`, `averageLatencyMs`, `minimumLatencyMs`
and `maximumLatencyMs`. It normalizes Chromium's current `playbackStats` and
legacy `playoutStats` units. When `supported` is false, all measurements are
`null`; unavailable statistics never masquerade as zero underruns.

## Cache handler

Decoded-chunk caching is injectable and optional.

```js
const cache = player.createPcmChunkCacheHandler();
player.configureCache(cache);
await player.loadRecord(file, { cache });
```

Custom handlers implement:

```js
const cacheHandler = {
  async get(key, meta) {
    return {
      chunkIndex: meta.chunkIndex,
      startFrame: 0,
      endFrame: 65536,
      sampleRate: 48000,
      channels: 2,
      channelBuffers: [leftBuffer, rightBuffer],
    };
  },
  async put(key, pcm) {
    // pcm.channelBuffers are transferable ArrayBuffers, one per channel.
  },
};
```

Remote yl.vin-compatible Opus caching is available as a built-in handler:

```js
const cache = player.createRemoteOpusChunkCacheHandler({
  apiBaseUrl: "https://yl.vin/api/play/tape",
});

player.configureCache(cache);
await player.loadRecord(file);
```

That handler uses `soundkit` packet framing plus `libopus-rs` for encode/decode. No C/`libopusjs` path is used.

Remote cache encryption needs record context. `loadRecord(...)` wires this up automatically. For manual use you can set it explicitly:

```js
await cache.setRecordContext({
  descriptorJson,
  recordHeaderProof,
  recordProfile: "single45",
});
```

There is also a standalone precache helper which drives the existing decode worker and fills the cache through the same `{ get, put }` surface:

```js
const precache = player.createRemoteOpusPrecache({
  apiBaseUrl: "https://yl.vin/api/play/tape",
});

await precache.precacheRecord(pngBytes);
```

## Cross-frame postMessage bridge

The player can expose a generic iframe bridge for embedders.

```js
player.configurePostMessageBridge({
  enabled: true,
  targetOrigin: "*",
  targetWindow: () => window.parent,
});
```

Send an arbitrary bridge message:

```js
player.postMessageBridgeSend("bitneedle-custom", { value: 1 });
```

### Default outbound messages

Playback state:

```js
{
  type: "bitneedle-embed-playback",
  isPlaying: true,
  currentTime: 12.34,
  duration: 185.2,
  volume: 0.8
}
```

Loaded-record summary:

```js
{
  type: "bitneedle-embed-record",
  record: {
    title: "01H...",
    releaseId: "01H...",
    recordProfile: "single45",
    payloadContainer: "ECDC",
    recordHash: "…sha256…",
    trackIndex: 0,
    trackCount: 3,
    trackTitle: "Side A"
  }
}
```

### Default inbound messages

Set playback state:

```js
iframe.contentWindow.postMessage({ type: "bitneedle-set-playing", playing: true }, "*");
```

Set volume:

```js
iframe.contentWindow.postMessage({ type: "bitneedle-set-volume", volume: 0.8 }, "*");
```

Seek by ratio:

```js
iframe.contentWindow.postMessage({ type: "bitneedle-seek", ratio: 0.5 }, "*");
```

Step between programme tracks:

```js
iframe.contentWindow.postMessage({ type: "bitneedle-track-step", direction: 1 }, "*");
iframe.contentWindow.postMessage({ type: "bitneedle-track-step", direction: -1 }, "*");
```

Set RPM, crossfader and needle-lift:

```js
iframe.contentWindow.postMessage({ type: "bitneedle-set-rpm", rpm: 45 }, "*");
iframe.contentWindow.postMessage({ type: "bitneedle-set-crossfader", crossfader: 1 }, "*");
iframe.contentWindow.postMessage({ type: "bitneedle-set-needle-lifted", lifted: true }, "*");
```

Set scratch technique and the two independent HF strengths:

```js
iframe.contentWindow.postMessage({ type: "bitneedle-set-scratch-preset", preset: "crab" }, "*");
iframe.contentWindow.postMessage({ type: "bitneedle-set-scratch-clicks", clicks: 8 }, "*");
iframe.contentWindow.postMessage({ type: "bitneedle-set-hf-acceleration-limit", strength: 0.35 }, "*");
iframe.contentWindow.postMessage({ type: "bitneedle-set-stylus-tracing-limit", strength: 0.72 }, "*");
```

Change embed options live — any of the `bg`/`tone`/`turntable`/`controls`/`status`/`light`/`strobe`/`dots`/`arm`/`arc`/`load` query params supported by `/embed.html` (see "Embed URL parameters" below) can also be changed after the iframe has already loaded, without reloading it:

```js
iframe.contentWindow.postMessage({
  type: "bitneedle-set-embed-options",
  options: { controls: "0", tone: "ff00aa", turntable: "ff00aa" },
}, "*");
```

This is the preferred way for an embedder to change controls visibility or colour theme after the initial load — reloading the iframe (changing its `src`) drops any record that was only handed over via `bitneedle-load-record-bytes` (e.g. an unpublished local file), so live options should always be used instead of rebuilding the `src` URL when the player is already showing a record.

All message type names and payload formatters are overridable through `configurePostMessageBridge(...)`.

## Embed URL parameters

The dedicated embed entrypoint is:

```txt
/embed.html
```

Supported query parameters:

```txt
bg=HEX
```

Sets the page/background color.

```txt
tone=HEX
```

Sets the control and chrome color used across the embedded turntable UI.

```txt
turntable=HEX
```

Sets the turntable ring color separately from the general tone.

```txt
controls=0
controls=1
```

`controls=0` hides the canvas radial controls and the fallback HTML controls. Any other value leaves controls visible.

```txt
status=0
status=1
```

A single status line (decode progress, remote cache fetch/write progress, errors) is shown bottom-left in the embedded tone color by default. `status=0` hides it. Any other value leaves it visible.

```txt
load=1
```

Opt in to opening the file picker when the empty turntable is clicked or tapped. This is off by default.

```txt
src=URL
```

Automatically loads a record on startup from a relative or absolute URL.

```txt
tape_url=URL
```

Enables the remote Opus tape store and points it at a relative or absolute tape API base.

Example:

```txt
/embed.html?src=./test.png&tape_url=/api/play/tape&bg=0a0a0a&tone=ff00aa&turntable=ff00aa&controls=0&load=1
```

## Scratch performance recording and replay

Scratch performances are stored as normalized engine commands. Pointer
coordinates and rendered audio are not stored. Schema v2 separates its clocks:

- `sourceSampleRate` defines every `positionFrames` value.
- `outputSampleRate` defines event `frameOffset` and `durationFrames`.
- `engine.gateAlgorithmVersion` identifies the gate behavior.
- `initialState` captures preset, clicks, manual crossfader/fader curve, and the
  programme-HF and stylus-tracing strengths.
- `scratch-preset`, `scratch-clicks` and `manual-crossfader` changes are stored
  as output-frame-timed events alongside scratch start/motion/end.

```js
{
  schemaVersion: 2,
  sourceSampleRate: 48000,
  outputSampleRate: 48000,
  durationFrames,
  engine: { gateAlgorithmVersion: 2 },
  initialState: {
    positionFrames,
    preset: "flare",
    clicks: 1,
    manualCrossfader: 0.5,
    faderCurve: "sharp-0.08",
    highFrequencyAccelerationLimit: 0.35,
    stylusTracingLimit: 0.72
  },
  events: [
    { type: "scratch-motion", frameOffset, positionFrames, rate, impulse },
    { type: "manual-crossfader", frameOffset, value }
  ]
}
```

Schema v1 is migrated when normalized, saved, loaded, imported or replayed.
Its single `sampleRate` is interpreted as both legacy clocks; source positions
and output offsets/duration are then rescaled independently to the current
record and AudioContext. Missing technique data defaults to `baby`, one click
and the sharp fader curve. Both newer limit strengths migrate to `0` (exact
bypass), preserving the sound of takes recorded before those controls existed.
Normalization also caps a take at 65,536 events and at 128 events in any
128-frame output window; denser imports are rejected before they reach the
AudioWorklet.

```js
const player = globalThis.vin.yl.player;

player.startScratchRecording({ name: "flare take 1" });

await player.beginScratch({ positionFrames: player.getState().positionFrames });
await player.updateScratch({ positionFrames: 120000, rate: -0.8, impulse: 0.2 });
await player.endScratch({ resumePlayback: true });

const performance = await player.stopScratchRecording({ save: true });
```

Manual recorder usage is also supported:

```js
const recorder = player.createScratchRecorder({ name: "orbit" });
recorder.start();
const performance = recorder.stop();
await player.scratches.save(performance);
```

List saved performances for the currently loaded record:

```js
const performances = await player.scratches.list();
```

Replay through the original acoustic model:

```js
await player.replayScratch(performances[0], { effects: "original" });
```

Replay the same mechanical trajectory without the selectable surface or
acoustic effect groups:

```js
await player.replayScratch(performances[0], { effects: "dry" });
```

Effects can also be selected independently:

```js
await player.replayScratch(performances[0], {
  effects: { acoustic: true, surface: false }
});
```

Replay events are scheduled inside the AudioWorklet using audio-frame offsets. Events can be applied within a render quantum rather than being delayed to animation frames or main-thread timers.

Replay duration is also an exact output-frame boundary. Rust keeps a lightweight
snapshot of the pre-replay dynamic DSP state (not PCM), allowing platter inertia,
filters, gate, fader and final output-gain state to resume for the suffix of the
same render quantum. The public replay promise remains active until the worklet
acknowledges that Rust restoration. A live transport, seek, RPM, volume,
crossfader, preset or advanced-limiter command interrupts replay first; replay
telemetry is never fed back into the persistent player-core position.

The worklet applies preset/click changes and converts the recorded sharp manual
crossfader position at the scheduled frame; Rust applies the manual fader as a
separate post-gate gain. The programme upper-band limiter is independent of the
`acoustic`/`surface` flags and uses the stored initial strength, including exact
bypass for migrated v1 takes.

## Canvas renderer

The canvas is a separate consumer of the player API. It does not access WASM, PCM windows, the AudioWorklet, or decoder internals.

Mount it manually when embedding the player:

```js
const canvas = document.querySelector("#player-canvas");
const controller = player.canvas.mount(canvas);
```

The bundled test page mounts its canvas automatically.

By default, tapping an empty turntable does not open the file picker. Enable it explicitly when that behavior is wanted:

```js
player.canvas.configure({
  loadOnEmptyRecordTap: true
});
```

### Component visibility

Every visual/control group can be enabled or disabled independently:

```js
player.canvas.setComponentVisible("record", true);
player.canvas.setComponentVisible("syncRings", true);
player.canvas.setComponentVisible("spindle", true);
player.canvas.setComponentVisible("startStop", true);
player.canvas.setComponentVisible("needle", true);
player.canvas.setComponentVisible("rpm", true);
player.canvas.setComponentVisible("volume", true);
player.canvas.setComponentVisible("crossfader", true);
player.canvas.setComponentVisible("seek", true);
player.canvas.setComponentVisible("labels", true);
```

Multiple components can be changed together:

```js
player.canvas.configure({
  components: {
    volume: false,
    crossfader: false,
    labels: true
  }
});
```

### Canvas colours

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
  recordFallback: "transparent",
  label: "transparent",
  syncDot: "rgba(0,0,0,0.23)",
  syncLit: "#00bfd3"
});
```

The current configuration is available with:

```js
const config = player.canvas.getConfig();
```

The canvas currently provides direct interaction for record scratching, start/stop, needle lift, RPM switching, position seek, volume, and crossfader. All actions call the same public player methods available to custom HTML, SVG, WebGL, or canvas interfaces. Pointer ownership is held in a pointer-ID map, so one pointer can keep control of the record while another adjusts XFADE, CH or PITCH; releasing the second pointer leaves the scratch gesture active.

## Radial canvas controls

The canvas layer uses the same public player API and can be mounted or themed independently.

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

```js
player.canvas.setTheme({
  line: "#050505",
  controlFill: "rgba(255,255,255,0.08)",
  controlActive: "#050505",
  controlText: "#050505",
  controlActiveText: "#f00020",
  accent: "#00bfd3",
  syncDot: "rgba(0,0,0,0.23)",
  syncLit: "#00bfd3",
  lamp: "#00bfd3",
  tonearm: "#050505",
  tonearmGuide: "rgba(5,5,5,0.35)",
  stylus: "#00bfd3",
  stylusGlow: "rgba(0,191,211,0.65)"
});
```

```js
player.canvas.setStrobeLight(false);
player.canvas.setComponentVisible("stylus", false);
player.canvas.setComponentVisible("needlePoint", true);
```

The physical sync dots continuously rotate. Inside the lamp beam, the renderer draws the calibrated stroboscopic sample so the matching pitch row appears stationary while the same dots continue moving elsewhere.

Dragging the record scratches. Dragging either the subtle stylus point or its dotted travel arc seeks through the record. Radial PITCH, CH and XFADE controls call the same player methods as non-canvas controls; pitch also has a dedicated RESET button for the 0% detent.

## WASM crate boundary

The workspace contains two browser-facing Rust crates:

- `record-player`: the real-time transport, mixer, scratch and acoustic engine loaded by the AudioWorklet.
- `player-wasm`: the player-only record reader and ECDC decoder loaded by the decoder worker.

The decoder worker imports `./player-wasm/player_wasm.js`. The AudioWorklet
imports `./record-player/record_player.js`; the host compiles its matching WASM
module and passes that compiled module into the worklet.

All per-sample DSP and the state-critical transport, scratch, gate, final
packet/mixer gain, replay-fader and acoustic models are inside `record-player` Rust/WASM. The
AudioWorklet JavaScript owns output-frame scheduling, bounded-window
coordination, direct bank-to-prepared-Rust copies and the required
interleaved-WASM-to-planar-Web-Audio output copy. Gesture/UI code stays in
JavaScript for direct browser pointer access; the off-thread PCM worker retains
signed-16-bit source data and repairs and assembles banks there to avoid
whole-record WASM crossings. Engine policy defaults to Rust unless measurement
shows that crossing the browser boundary would add work or latency.

The decoder and tape/cache paths use `player-wasm` for record/ECDC parsing and
the required cache encryption helpers; the real-time AudioWorklet does not
import that crate.

The current real-WASM timing smoke measured p95 render calls of `0.0389 ms`
(`1.46%` of budget) for normal playback and `0.2100 ms` (`7.87%`) for
alternating `±8×` `crab`/8-click scratching. Applying a fresh six-second stereo
PCM window measured p95 `0.0740 ms` (`2.77%`) and maximum `0.2569 ms`
(`9.63%`). For comparison, a 128-frame quantum at 48 kHz lasts `2.667 ms`.
Those measurements cover engine timing, not subjective feel or sound quality.

Run `npm run bench:worklet` to rebuild release WASM before timing it. The figures
above are a representative 2026-07-20 Apple Silicon macOS 26.5 / Node 26.3.0
run. This is a deterministic Node harness with real release WASM and a mocked
`AudioWorkletProcessor`, with render p95 guarded below 50%, PCM-window p95 below
25%, and PCM-window maximum below 50% of one quantum. It is not a browser
audio-thread deadline measurement.

Run `npm run test:browser` for the real Chrome AudioWorklet smoke. It loads a
synthetic source, exercises all presets in both directions, captures output,
replays and restores a take, injects independent record and XFADE touches, and
fails on any browser-reported audio underrun.

## Progressive startup

Record decoding begins playback-capable windowing after the first completed PCM
segments. `loadRecord()` resolves once the first contiguous playback window
(currently up to three seconds) is ready; the remaining decode and cache work
continues in the background. Subscribers observe `ready: true` at that same
playback-ready boundary.

The PCM worker retains signed 16-bit source storage and publishes bounded
Float32 regions rather than transferring a full floating-point record into the
AudioWorklet. On a cross-origin-isolated page, two six-second
`SharedArrayBuffer` banks alternate between fill and render use. The worklet
applies only completed bank generations, Rust copies only the active source
window, and replacement requests are coalesced while a fill or apply is in
flight. A transferable bounded-window fallback exists when shared memory is not
available.

Progressive availability covers only the contiguous decoded prefix. Seeking or
scratching cannot expose an undecoded zero-filled tail as valid PCM. The worker
also performs the authoritative 24-sample Hermite repair at contiguous chunk
joins before a window is copied.

## Audio rendering and validation

Fractional sampling uses four-point Catmull-Rom near normal speed. From a
`1.05×` source-frame step it blends toward a 24-tap Blackman-windowed sinc, and
uses the band-limited path fully from `1.5×`. The cutoff follows the absolute
source step, so the same anti-alias behavior applies to fast forward and reverse
motion.

Automated checks do not establish subjective vinyl similarity or tactile
quality. The outstanding blind listening, control-task and free-performance
procedure is specified in
[`DJ_VALIDATION_PROTOCOL.md`](./DJ_VALIDATION_PROTOCOL.md); this API reference
does not claim results from that human validation.


## Transport motor

The platter motor is independent from programme audio and the needle.

```js
await player.startTransport();
await player.stopTransport();
await player.toggleTransport();
```

`START - STOP` uses this motor API. With the needle raised, the record and strobe continue to rotate silently. Lowering the needle while the motor is running starts audio as soon as at least one decoded PCM chunk is available.

## Logging

```js
vin.yl.player.setLogging(true);
vin.yl.player.setLogging(false);
vin.yl.player.loggingEnabled;
```

Logging is shared across the host, core worker, decoder worker, and AudioWorklet. The URL parameter `player_log=0` disables it globally at startup; `player_log=1` enables it.
