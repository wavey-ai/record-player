# Acoustic Parity Report

Status: superseded on 2026-07-20.

Use [`REFERENCE_ENGINE_GAP_AUDIT.md`](./REFERENCE_ENGINE_GAP_AUDIT.md) for the
current comparison with `../yl.vin/apps/play`.

The old report treated a line-for-line port as the goal. That is no longer the
correct release standard. The target now has deliberate improvements in these
areas:

- grab response
- motor-off platter throw
- native-RPM wow
- high-rate band-limited resampling
- stylus tracing
- programme HF acceleration limiting
- travel-based preset gates
- sample-accurate capture and replay.

The current audit records each difference and its test evidence. It also lists
the remaining hardware and blind-DJ work.
