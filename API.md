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
```

Canvas code should derive pointer geometry only. It must not access the AudioWorklet, WASM instances, PCM windows, or transport clock directly.

## State

```js
const unsubscribe = player.subscribe(state => {
  state.ready;
  state.playing;
  state.needleLifted;
  state.scratching;
  state.positionSeconds;
  state.durationSeconds;
  state.positionRatio;
  state.rpm;
  state.nativeRpm;
  state.playbackRate;
  state.volume;
  state.crossfader;
  state.recordProfile;
  state.payloadContainer;
  state.releaseId;
  state.currentTrackIndex;
  state.currentTrackTitle;
  state.trackCount;
});
```

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

Scratch performances are stored as normalized engine motion events with audio-frame offsets. Pointer coordinates and rendered audio are not stored.

```js
const player = globalThis.vin.yl.player;

player.startScratchRecording({ name: "flare take 1" });

await player.beginScratch({ positionFrames: player.getState().positionSeconds * 48000 });
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

Replay the same mechanical trajectory without surface or acoustic coloration:

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
  button: "rgba(255,255,255,0.08)",
  buttonActive: "#050505",
  buttonText: "#050505",
  buttonActiveText: "#f00020",
  accent: "#00bfd3",
  recordFallback: "#111111",
  label: "#f6d800",
  syncDot: "#050505",
  syncLit: "#00bfd3"
});
```

The current configuration is available with:

```js
const config = player.canvas.getConfig();
```

The canvas currently provides direct interaction for record scratching, start/stop, needle lift, RPM switching, position seek, volume, and crossfader. All actions call the same public player methods available to custom HTML, SVG, WebGL, or canvas interfaces.

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

- `record-player`: real-time transport, mixer, scratch and acoustic rendering.
- `player-wasm`: PNG decoding, descriptor/header validation, payload extraction, sidecars, ECDC parsing, LM decoding, cache identities and playback metadata.

The decoder worker imports only `./wasm/player-wasm/player_wasm.js`. The AudioWorklet imports only `./wasm/record-player/record_player.js`.


## Rust workspace boundary

The workspace contains two browser-facing Rust crates:

- `record-player`: the real-time transport, mixer, scratch and acoustic engine loaded by the AudioWorklet.
- `player-wasm`: the player-only record reader and ECDC decoder loaded by the decoder worker.

`player-wasm` deliberately exports only the functions used by `app/src/record-decoder-worker.js`. It does not expose record authoring, rendering, sidecar inspection, cache encryption, remote-scratch identity or wallet helpers. Its generated browser package is `app/dist/wasm/player-wasm/player_wasm.js`.

## Progressive startup

Record decoding begins playback-capable windowing after the first completed PCM segment. `loadRecord()` still resolves after the full decode and cache write, but subscribers may observe `loaded: true` and start playback earlier while the remainder continues decoding.


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
