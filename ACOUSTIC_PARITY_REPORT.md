# Acoustic Parity Report

Honest status per subsystem after this migration pass. "By construction" means every
constant, equation, and state transition was compared line-by-line against
`../yl.vin/apps/play` source (see ACOUSTIC_MIGRATION_AUDIT.md); "by test" means a
deterministic automated comparison exists. Golden JS-vs-Rust output comparison is
**deferred** with the rest of the test work until the migration proper is finished (per
project direction) — no subsystem below claims output-sample parity on that basis.

| Subsystem | Status | Basis |
|---|---|---|
| Motor spin-up / linear brake / delivered-rate model | pass | by construction, constants exact |
| Scratch grip/spring/still-snap/catch-up physics | pass | by construction |
| Scratch does not alter motor state; release = slipmat catch | pass | by construction + existing engine tests |
| STOP is the only motor authority | pass (fixed) | `toggle_playback` STOP now turns the motor off + lifts needle, matching original; regression test `explicit_stop_clears_suspended_resume_bookmark` now passes |
| Catmull–Rom interpolation + slope/curvature rasp | pass | identical coefficients |
| Drag lowpass / tracing loss | pass | identical formula |
| Wow & flutter (rotation-locked / fixed-frequency) | pass | identical constants and phase advance |
| Position-locked groove grain, bed, dust flecks | pass | identical hashes, salts, cells — same groove position reproduces the same imperfections |
| Seeded randomness (LCG + hash mix) | pass | identical integer arithmetic |
| `start()` position seeding | pass (fixed) | was `max()`, now first-non-zero like the original `\|\|` chain |
| Zero-speed window handling | pass (fixed) | stationary stylus no longer flags spurious window misses |
| Needle-surface asset (opus) | pass | asset copied into repo; host decodes off-thread, PCM handed to Rust; synthetic fallback retained |
| Lead-in / deadwax surface beds | implemented | Rust `SurfaceBed`, filters/gains/envelopes exact by construction; lead-in trigger remains dormant exactly as in the current original |
| Deadwax at programme end | implemented | host dispatches `start_timed_region(deadwax, 2 turns × 60/rpm)` on `ended`, engine guards mirror the original |
| Needle-drop foley (thump + burst) | implemented | constants exact; biquad is RBJ (Web Audio spec) — float-level differences vs browsers expected |
| 50–140 ms early landing on live cue | implemented | host seek path |
| Mobile ×2.25 surface gain | pass | same UA detection, multiplier applied in Rust |
| Gain ramps | pass (fixed) | mixer ramps corrected 12 ms → 5 ms to match original default; 6 ms window-miss fade already exact. Historical "4 ms / 8 ms" values are not in the current original |
| 24-sample Hermite seam repair | pass | length/timeline preserved, contiguous-only, per channel |
| Progressive playback (first-chunk start, boundary wait/resume, progress text) | pass | verified present in host/worklet |
| Golden JS↔Rust output comparison | **not run** | deferred with test work; parity above is not claimed at output-sample level |

## Remaining mismatches / deliberate divergences

1. Random slice offsets use the DSP LCG (deterministic) instead of `Math.random()`.
2. Rust biquad and exponential-ramp emulation may differ from browser float behaviour at
   ~1e-7; audibly identical, not bit-identical.
3. HTML-audio fallback bed path not migrated (unsupported-environment path out of scope).
4. Shared-SAB worklet transport not migrated (message path is used; acoustics unaffected).

## Verdict

Exact migration is **not yet provable** — the deterministic golden harness has not been
built (deferred). Everything traceable by inspection is either exact or documented above.
