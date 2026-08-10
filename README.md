# record-player

`record-player` is the Rust audio engine behind YL.VIN playback. It turns
decoded programme audio and pointer gestures into the sound of a record on a
deck: motion, scratching, timed controls, and record-surface behavior.

You give the engine bounded PCM windows and sample-timed controls. It gives
you host-rate audio, metering, and serializable deck state. The engine does
not decode source formats, schedule audio callbacks, or draw UI — your host
application owns those.

## Get started

Add the library as a pinned Git revision:

```toml
[dependencies]
record-player = { git = "...", rev = "<pinned commit>" }
```

Build the WASM package for a browser host:

```sh
wasm-pack build --target web --release --features wasm
```

Use the C ABI from a non-Rust host through `record-player-capi`. The header
is [`include/record_player.h`](./include/record_player.h).

Do not copy DSP, mechanics, preset behavior, or gesture policy out of this
repository into an application. Consume a pinned revision instead.

## Entry points

- `ScratchAcousticDsp` — the programme renderer. Accepts bounded PCM windows
  and produces host-rate audio.
- `PlayerEngine` — serializable deck state, events, host commands, and view
  state.
- `ScratchGestureMapper` — converts timestamped pointer input into
  sample-timed mechanical controls.
- `PhysicalHostRenderer` and `StreamingGrooveCutter` — the physical groove
  path, available for reference and validation work.

The workspace contains two crates:

- `record-player` — the Rust library and optional WASM exports;
- `record-player-capi` — ABI version 5 for the physical renderer and the
  scratch gesture mapper.

The WASM-specific physical API is documented in [`WASM_API.md`](./WASM_API.md).

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
- bounded window requests, output metering, and real-time recovery
  diagnostics.

## Real-time contract

The audio path uses bounded, preallocated storage. Timed controls use
absolute frames and a bounded single-producer, single-consumer mailbox. A
failed physical render restores engine state and leaves caller output
unchanged.

Inside an audio callback, hosts must not perform source decoding,
whole-record allocation, network work, or UI work. Hosts provide their own
output scale; the physical boundary reports unclipped voltage peaks and
clipping counts.

## Build and test

Run the complete Rust workspace:

```sh
cargo test --workspace
```

Check the native ABI and header:

```sh
scripts/check-record-player-capi.sh
```

Consumer repositories pin exact commits. After you change a public surface,
update the relevant consumer pin and rebuild its generated native or WASM
artifact. Do not publish from an uncommitted engine worktree.

Current consumers:

- `vin.yl.native` pins this repository and exposes the engine to the Apple
  app through its native bridge;
- `vin.yl.player` builds the WASM engine for its browser host and
  AudioWorklet;
- `vin.yl.web` consumes the shared native/browser runtime for MUSIC.

## Evidence and design records

- [`RENDERER_ARCHITECTURE_DECISION_LOG.md`](./RENDERER_ARCHITECTURE_DECISION_LOG.md)
  explains why the active renderer does not currently perform a virtual 45/45
  cut for transparent playback.
- [`STREAMING_GROOVE_CUTTER.md`](./STREAMING_GROOVE_CUTTER.md) defines the
  bounded physical-groove page protocol.
- [`PHYSICS_VALIDATION_CASES.md`](./PHYSICS_VALIDATION_CASES.md) records
  physical counterexamples and reproduction methods.
- [`PHYSICS_INVESTIGATION_LOG.md`](./PHYSICS_INVESTIGATION_LOG.md) tracks
  model evidence, uncertainty, and rejected assumptions.
- [`PERCEPTUAL_ACCURACY_REPORT.md`](./PERCEPTUAL_ACCURACY_REPORT.md) separates
  confirmed audible behavior from work that still needs listening evidence.

The physical SL-1200MK7 and Concorde MKII Scratch profile is a seed profile,
not a calibrated accuracy claim. Unit tests establish deterministic software
behavior; they do not replace hardware measurements or independent listening
tests.
