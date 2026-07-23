# record-player

This repository owns the shared Bitneedle record-player engine.

The Rust crate supplies transport state, record physics, scratch motion, acoustic effects, mixing, and stylus calibration.

The browser builds this crate with the `wasm` feature.

Apple clients link this crate through the native Bitneedle library.

## Test the engine

Run this command:

```sh
cargo test
```

The tests check transport behavior, scratch motion, acoustic output, mixing, and calibration.
