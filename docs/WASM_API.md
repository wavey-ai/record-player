# Physical player WASM API (removed)

Status: historical record. The physical renderer this document described —
`PhysicalHostRenderer`, `StreamingGrooveCutter`, and the physical C and WASM
surface — was removed in commit `89443b9`. The behaviour it carried lives in
`ScratchAcousticDsp` now, as `cartridge_velocity_gain` and `riaa_speed_tilt`.

The live WASM surface is built from this repository with:

```sh
wasm-pack build --target web --release --features wasm
```

`ScratchAcousticDsp` exports the window, transport, parameter, replay, wear,
and VFX calls the browser's AudioWorklet and take renderer drive.
`WasmPlayerEngine` wraps the Rust `PlayerEngine` for hosts that drive the deck
through it.

See [`SCRATCH_FEEL_DECISION_LOG.md`](./SCRATCH_FEEL_DECISION_LOG.md) for the
removal and [`RENDERER_ARCHITECTURE_DECISION_LOG.md`](./RENDERER_ARCHITECTURE_DECISION_LOG.md)
for the renderer choice.
