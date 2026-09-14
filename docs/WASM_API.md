# WASM API

The browser reaches the deck through this repository's WASM build. The
generated JavaScript module carries its own bindings.

## Build

```sh
wasm-pack build --target web --release --features wasm
```

The output is `record_player.js` with `record_player_bg.wasm`.

## `ScratchAcousticDsp`

One `ScratchAcousticDsp` renders the deck on the audio thread. Construct it
with the host sample rate and an `AcousticConfig`:

```js
const dsp = new ScratchAcousticDsp(sampleRate, config);
```

It owns the programme window, the transport, the scratch gate, the press and
wear dials, and the VFX scene.

The window calls stage bounded PCM and publish it to the engine:
`prepareWindow`, `windowChannelPtr`, `commitWindow`, and `clearWindow`.

The transport and parameter calls share one deck state: `start`, `stop`,
`setEffects`, `setOutputGain`, `setManualCrossfader`, and the scratch preset,
click, press, and wear setters.

Deterministic replay is `captureReplayState`, `restoreReplayState`, and the
`beginDeterministicReplay` entry points.

## `WasmPlayerEngine`

`WasmPlayerEngine` wraps the Rust `PlayerEngine` for hosts that drive the deck
through events: construct it with a `PlayerConfig`, `dispatch` an event, and
read the commands it produces with `drainCommands`.

The design records live in
[`RENDERER_ARCHITECTURE_DECISION_LOG.md`](./RENDERER_ARCHITECTURE_DECISION_LOG.md)
and [`SCRATCH_FEEL_DECISION_LOG.md`](./SCRATCH_FEEL_DECISION_LOG.md).
