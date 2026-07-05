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
await player.play();
await player.pause();
await player.togglePlayback();
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
});
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

Dragging the record scratches. Dragging the needle point seeks along the visible groove path. Radial PITCH, CH, XFADE and POSITION controls call the same player methods as non-canvas controls.

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
