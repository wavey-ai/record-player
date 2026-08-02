# record-player

This repository owns the canonical YL.VIN record-player engine.

Do not copy the engine into an application repository. Rust, Swift, and browser clients must use this crate revision.

## Physical model

The canonical player uses one coupled signal path:

1. Cut source PCM into a spatial 45/45 groove.
2. Trace each groove wall with finite stylus geometry.
3. Solve stylus, suspension, tonearm, and wall contact.
4. Generate moving-magnet cartridge voltage.
5. Solve the cartridge coil and electrical load.
6. Apply playback RIAA equalization and the phono stage.
7. Convert the 192 kHz physical output to the host rate.
8. Return groove reaction torque to the record mechanics.

The deck model contains separate platter and record bodies. It also contains motor, bearing, slipmat, and hand-contact states.

Control events use absolute 192 kHz sample times. The bounded control mailbox does not use locks.

Render calls use preallocated buffers. A failed render restores all state and does not change caller output.

## Scratch techniques

Rust owns the eight scratch presets, their click settings, motion state, prediction state, and automatic crossfader policy.

The presets are Baby, Stab, Chirp, Transform, Flare, Crab, Orbit, and Drum.

The performance helper uses same-sample intent, rendered rate, and exact signed record-angle travel.

It reports prediction confidence because an unseen first-stroke endpoint is not knowable.

Current technique timings are provisional calibration values.

They require measured user motion and crossfader traces before a human-performance accuracy claim.

Consumer integration will start with iOS after the remaining physical-engine blockers close. Browser integration follows iOS verification.

## Accuracy status

The built-in SL-1200MK7 and Concorde MKII Scratch profile is a seed profile.

Published values, calculated values, and estimates identify each parameter source. Estimates do not authorize a calibrated accuracy claim.

A calibrated profile must register numeric limits for every calibration test. Each measured uncertainty interval must stay inside those limits.

Read [PHYSICS_INVESTIGATION_LOG.md](./PHYSICS_INVESTIGATION_LOG.md) for evidence, uncertainty, rejected ideas, and required measurements.

Read [PHYSICS_VALIDATION_CASES.md](./PHYSICS_VALIDATION_CASES.md) for exact counterexamples, reference methods, artifacts, and reproduction commands.

Read [PERCEPTUAL_ACCURACY_REPORT.md](./PERCEPTUAL_ACCURACY_REPORT.md) for confirmed sound changes, missing evidence, and the focused audible-work plan.

Read [STREAMING_GROOVE_CUTTER.md](./STREAMING_GROOVE_CUTTER.md) for bounded groove cutting and page contracts.

## Crates

- `record-player` contains all physical equations and real-time Rust state.
- `record-player-capi` provides the native C and Swift boundary.

The C ABI does not define physics constants or gesture policy.

Each host renderer must supply the phono volts that map to digital full scale.

The boundary reports unclipped voltage peaks and clipping counts.

## Test the engine

Run this command:

```sh
cargo test --workspace
```

Run the native ABI check with this command:

```sh
scripts/check-record-player-capi.sh
```

Unit tests verify deterministic state, contact constraints, snapshots, rapid controls, paging, resampling, and ABI behavior.

Unit tests do not replace hardware calibration, output-device stress tests, or independent comparative measurements.
