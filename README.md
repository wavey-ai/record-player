# record-player

`record-player` is the Rust audio engine behind BITNEEDLE. It turns decoded
programme audio and hand motion into the sound of a record on a deck: platter
and slipmat motion, scratching, timed controls, and record-surface behavior.

You give the engine bounded PCM windows and sample-timed controls. It gives
you host-rate audio, metering, and serializable deck state. The engine does
not decode source formats, schedule audio callbacks, or draw UI — your host
application owns those.

`ScratchAcousticDsp` is the renderer. It owns the deck, the record's acoustic
behavior, and the scratch gate in one host-rate path, so a settled record at
nominal speed passes the programme through untouched while a hand on the
record reads it the way a needle would.

## Entry points

- `ScratchAcousticDsp` — the production renderer. Bounded PCM windows in,
  host-rate audio out.
- `ScratchGestureMapper` — converts timestamped pointer samples into
  sample-timed mechanical controls. The C ABI wraps this and this alone.
- `PlayerEngine` (exported as `WasmPlayerEngine` on the web target) —
  serializable transport and deck state, events, host commands, and view
  state.
- `DeckMechanicalState`, `ScratchGate`, and the `VinylVfx*` processor are the
  pieces the renderer is built from.

The workspace contains two crates:

- `record-player` — the Rust library and its optional WASM exports;
- `record-player-capi` — ABI version 5, for the scratch gesture mapper. The
  header is [`include/record_player.h`](./include/record_player.h).

## How BITNEEDLE consumes it

- **Phone.** `bitneedle-native-core` wraps one `ScratchAcousticDsp` in
  `record_player_bridge.rs` and exposes it through the
  `bitneedle_native_record_player_*` C ABI; the `Deck` Swift package drives
  that. The live pointer tracker is BITNEEDLE's own, not the capi's gesture
  mapper.
- **Browser.** `web.mk` builds this crate's WASM, and the AudioWorklet and
  take-render worker instantiate `ScratchAcousticDsp` directly.

`PlayerEngine` and the gesture-mapper C ABI ship for hosts that drive the deck
through them. Consume a revision rather than copying DSP, mechanics, preset
behavior, or gesture policy out of this repository into an application.

## What the engine models

`ScratchAcousticDsp` models:

- platter, record, motor, bearing, slipmat, and hand-contact motion;
- playback, braking, free spin, pitch, seek, needle lift, and run-out;
- the Baby, Stab, Chirp, Transform, Flare, Crab, Orbit, and Drum scratch
  presets;
- click timing, scratch-gate state, crossfader automation, and deterministic
  replay;
- locked grooves, groove wear, pressing defects, stylus effects, and surface
  foley;
- velocity gain and RIAA speed tilt away from nominal speed, so a settled 1×
  pass is bit-exact;
- bounded window requests, output metering, and real-time recovery
  diagnostics.

## Real-time contract

The audio path is `#![forbid(unsafe_code)]` and uses bounded, preallocated
storage; under `cargo test` an allocation guard fails a test that allocates on
it. Timed controls use absolute frames. Inside an audio callback, hosts must
not decode, allocate whole-record buffers, or do network or UI work.

## Build and test

Run the complete Rust workspace:

```sh
cargo test --workspace
```

Build the WASM the browser instantiates:

```sh
wasm-pack build --target web --release --features wasm
```

Check the native ABI and header:

```sh
scripts/check-record-player-capi.sh
```

## Evidence and design records

- [`docs/RENDERER_ARCHITECTURE_DECISION_LOG.md`](./docs/RENDERER_ARCHITECTURE_DECISION_LOG.md)
  records the renderer's design and the transparent-playback rule.
- [`docs/SCRATCH_FEEL_DECISION_LOG.md`](./docs/SCRATCH_FEEL_DECISION_LOG.md)
  is the scratch-feel investigation and its live A/B evidence.
- [`docs/REFERENCE_ENGINE_GAP_AUDIT.md`](./docs/REFERENCE_ENGINE_GAP_AUDIT.md)
  compares this engine against the reference implementation.
- [`docs/PERCEPTUAL_ACCURACY_REPORT.md`](./docs/PERCEPTUAL_ACCURACY_REPORT.md)
  separates confirmed audible behavior from work that still needs listening
  evidence.
- [`docs/PHYSICS_INVESTIGATION_LOG.md`](./docs/PHYSICS_INVESTIGATION_LOG.md)
  tracks model evidence, uncertainty, and rejected assumptions.

The SL-1200MK7 deck mechanics profile is a seed profile, not a calibrated
accuracy claim. Unit tests establish deterministic software behavior; they do
not replace hardware measurements or independent listening tests.
