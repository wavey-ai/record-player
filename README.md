# record-player

`record-player` is the shared Rust engine behind YL.VIN playback, deck motion,
scratching, timed controls, and record-surface effects.

Applications should consume a pinned revision of this repository. Do not copy
its DSP, mechanics, preset behavior, or gesture policy into an application.

## What ships today

The active YL.VIN programme renderer is `ScratchAcousticDsp`. It accepts bounded
PCM windows and produces host-rate audio while owning:

- platter, record, motor, bearing, slipmat, and hand-contact motion;
- playback, braking, free spin, pitch, seek, needle lift, and run-out behavior;
- Baby, Stab, Chirp, Transform, Flare, Crab, Orbit, and Drum scratch presets;
- click timing, scratch-gate state, crossfader automation, and deterministic replay;
- locked grooves, groove wear, pressing defects, stylus effects, and surface foley;
- bounded window requests, output metering, and real-time recovery diagnostics.

`PlayerEngine` separately owns serializable deck state, events, host commands,
and view state. `ScratchGestureMapper` converts timestamped pointer input into
sample-timed mechanical controls. Hosts remain responsible for decoding source
formats, supplying PCM, scheduling audio callbacks, and drawing their UI.

The current consumers are:

- `vin.yl.native`, which pins this repository and exposes the active engine to
  the Apple app through its native bridge;
- `vin.yl.player`, which builds the WASM engine for its browser host and
  AudioWorklet;
- `vin.yl.web`, which consumes the shared native/browser runtime for MUSIC.

## Does it still use 45/45 grooves?

Not for the active programme-audio path.

The `physical` module still implements a spatial stereo 45/45 groove model. It
includes streaming groove cutting, paged groove storage, stylus and wall
contact, tonearm mechanics, moving-magnet cartridge behavior, RIAA playback,
host-rate conversion, and reaction torque.

That path remains available through `PhysicalHostRenderer`, the WASM physical
APIs, and `record-player-capi`. It is useful for reference work, validation, and
continued integration. The browser prepares its physical groove cache, but its
programme output still comes from `ScratchAcousticDsp`. The shipping iOS player
also uses `ScratchAcousticDsp` through `vin.yl.native`.

Bitneedle picture-record pixels never represent physical groove walls. A host
recovers and decodes the embedded programme first. A virtual 45/45 cut is only
needed when deliberately using the physical renderer.

Do not describe 45/45 tracing as the default playback path until a consumer
selects it for programme output and it passes the real-time and listening gates
recorded in
[`RENDERER_ARCHITECTURE_DECISION_LOG.md`](./RENDERER_ARCHITECTURE_DECISION_LOG.md).

## Public surfaces

The workspace contains two crates:

- `record-player`: the Rust library and optional WASM exports;
- `record-player-capi`: ABI version 5 for the physical renderer and scratch
  gesture mapper.

Important Rust entry points include:

- `ScratchAcousticDsp` for current programme rendering;
- `PlayerEngine` for host-facing state and command transitions;
- `ScratchGestureMapper` for timestamped pointer-to-control mapping;
- `PhysicalHostRenderer` and `StreamingGrooveCutter` for the 45/45 path.

The C ABI is declared in [`include/record_player.h`](./include/record_player.h).
The WASM-specific physical API is documented in [`WASM_API.md`](./WASM_API.md).

## Real-time contract

The audio path uses bounded, preallocated storage. Timed controls use absolute
frames and a bounded single-producer, single-consumer mailbox. A failed physical
render restores engine state and leaves caller output unchanged.

Hosts must not perform source decoding, whole-record allocation, network work,
or UI work inside an audio callback. They must provide their own output scale;
the physical boundary reports unclipped voltage peaks and clipping counts.

## Build and test

Run the complete Rust workspace:

```sh
cargo test --workspace
```

Check the native ABI and header:

```sh
scripts/check-record-player-capi.sh
```

Build the WASM package:

```sh
wasm-pack build --target web --release --features wasm
```

The consumer repositories pin exact commits. After changing a public surface,
update the relevant consumer pin and rebuild its generated native or WASM
artifact. Do not publish from an uncommitted engine worktree.

## Evidence and design records

- [`RENDERER_ARCHITECTURE_DECISION_LOG.md`](./RENDERER_ARCHITECTURE_DECISION_LOG.md)
  explains why the active renderer does not currently perform a virtual 45/45
  cut for transparent playback.
- [`STREAMING_GROOVE_CUTTER.md`](./STREAMING_GROOVE_CUTTER.md) defines the
  bounded physical-groove page protocol.
- [`PHYSICS_VALIDATION_CASES.md`](./PHYSICS_VALIDATION_CASES.md) records physical
  counterexamples and reproduction methods.
- [`PHYSICS_INVESTIGATION_LOG.md`](./PHYSICS_INVESTIGATION_LOG.md) tracks model
  evidence, uncertainty, and rejected assumptions.
- [`PERCEPTUAL_ACCURACY_REPORT.md`](./PERCEPTUAL_ACCURACY_REPORT.md) separates
  confirmed audible behavior from work that still needs listening evidence.

The physical SL-1200MK7 and Concorde MKII Scratch profile is a seed profile, not
a calibrated accuracy claim. Unit tests establish deterministic software
behavior; they do not replace hardware measurements or independent listening
tests.
