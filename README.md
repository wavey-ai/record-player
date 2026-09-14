# record-player

`record-player` is the Rust deck engine behind BITNEEDLE. It turns decoded
programme audio and pointer/hand motion into the sound of a record on a deck:
platter and slipmat motion, scratching, timed controls, and record-surface
behavior.

You give the engine bounded PCM windows and sample-timed controls. It gives
you host-rate audio, metering, and serializable deck state. The engine does
not decode source formats, schedule audio callbacks, or draw UI — your host
application owns those.

## One renderer, no grooves

`ScratchAcousticDsp` is the only production renderer.

The physical groove stack is gone. Commit `89443b9` removed `src/physical/`
(~48,800 lines), `PhysicalHostRenderer`, `StreamingGrooveCutter`, and the
physical host renderer's C surface. Cutting a groove and reading it back at
rate *r* is a transfer function, and it is integrated into the acoustic
renderer as `cartridge_velocity_gain` and `riaa_speed_tilt`: exactly 1.0 at
nominal speed, so a settled 1× pass stays bit-exact. The geometric effects a
groove would still buy — needle skip, adjacent-groove pre-echo, wear as
deformation — are features to design against this engine, not reasons to
keep the old one.

## Entry points

- `ScratchAcousticDsp` — the production renderer. Bounded PCM windows in,
  host-rate audio out, with the deck mechanics and scratch gate inside it.
- `ScratchGestureMapper` — pointer samples to sample-timed mechanical
  controls. The C ABI (`record-player-capi`) wraps this and this alone.
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

`PlayerEngine` and the gesture-mapper C ABI are not on the live audio path
today; they ship for hosts that drive the deck through them. Consume a
revision rather than copying DSP, mechanics, preset behavior, or gesture
policy out of this repository into an application.

## What the renderer owns

`ScratchAcousticDsp` models:

- platter, record, motor, bearing, slipmat, and hand-contact motion;
- playback, braking, free spin, pitch, seek, needle lift, and run-out;
- the Baby, Stab, Chirp, Transform, Flare, Crab, Orbit, and Drum scratch
  presets;
- click timing, scratch-gate state, crossfader automation, and deterministic
  replay;
- locked grooves, groove wear, pressing defects, stylus effects, and surface
  foley;
- velocity gain and RIAA speed tilt off nominal speed;
- bounded window requests, output metering, and real-time recovery
  diagnostics.

## Real-time contract

The audio path is `#![forbid(unsafe_code)]` and uses bounded, preallocated
storage; under `cargo test` an allocation guard fails a test that allocates
on it. Timed controls use absolute frames. Inside an audio callback, hosts
must not decode, allocate whole-record buffers, or do network or UI work.

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
  explains why the active renderer does not perform a virtual 45/45 cut for
  transparent playback.
- [`docs/SCRATCH_FEEL_DECISION_LOG.md`](./docs/SCRATCH_FEEL_DECISION_LOG.md)
  records the scratch-feel investigation and the decision to remove the
  physical stack.
- [`docs/REFERENCE_ENGINE_GAP_AUDIT.md`](./docs/REFERENCE_ENGINE_GAP_AUDIT.md)
  is the current source comparison against the reference engine.
- [`docs/PERCEPTUAL_ACCURACY_REPORT.md`](./docs/PERCEPTUAL_ACCURACY_REPORT.md)
  separates confirmed audible behavior from work that still needs listening
  evidence.
- [`docs/PHYSICS_INVESTIGATION_LOG.md`](./docs/PHYSICS_INVESTIGATION_LOG.md)
  tracks model evidence, uncertainty, and rejected assumptions.

The SL-1200MK7 deck mechanics profile is a seed profile, not a calibrated
accuracy claim. Unit tests establish deterministic software behavior; they do
not replace hardware measurements or independent listening tests.
