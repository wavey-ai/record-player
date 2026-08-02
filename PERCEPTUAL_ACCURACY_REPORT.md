# Perceptual Accuracy Report

## Report Status

- **Report date**: 2026-08-02.
- **Repository**: `record-player`.
- **Reviewed checkpoint**: `1c78352`.
- **Scope**: Changes from `9cdfe80` through `1c78352`.
- **Purpose**: Identify work that can improve audible accuracy.
- **Limit**: This report does not claim a completed listening result.

## Executive Verdict

The work has produced real waveform-changing corrections.

The strongest confirmed correction is the certified global stylus tracer.

The physical player, vector friction, reciprocal cartridge load, and high-speed filters can also change output.

However, no controlled listening test has proved a perceptual improvement.

The current rapid-scratch path still differs materially from the high-rate reduced reference.

Therefore, the work does not yet support a claim of superior perceived accuracy.

The recent KKT certificate work improves evidence and failure handling.

It does not directly improve normal rendered audio.

## Evidence Terms

- **Output-changing**: The production equation or selected physical state can change rendered voltage.
- **Reference-improving**: A numerical or physical reference shows lower error.
- **Perceptually plausible**: The change affects a quantity that can influence audible output.
- **Perceptually demonstrated**: A controlled listening test shows a repeatable listener result.
- **Default active**: The seed product profile enables the change without new calibration data.

## Current Scorecard

| Question | Result |
|---|---|
| Has production audio behavior changed? | Yes |
| Has one tracing error decreased against a numerical reference? | Yes |
| Has rapid-scratch output converged to the high-rate reference? | No |
| Has an A/B render shown a clear full-output improvement? | No |
| Has a controlled listening test shown a perceptual improvement? | No |
| Are the Rust scratch presets active in iOS? | No |
| Can the product claim the most accurate recreation? | No |

## Production Changes With Perceptual Potential

### Canonical Physical Signal Path

- **Commit**: `9cdfe80`.
- **Output-changing**: Yes.
- **Default active**: Yes in the physical player path.
- **Expected relevance**: High.
- **Listening evidence**: None.

The engine now derives voltage from groove geometry, stylus motion, suspension, cartridge state, and the electrical load.

This path replaces direct PCM-style curvature and movement heuristics.

It also couples platter, record, pickup, and cartridge forces in the same physical sample.

This architecture is more physically causal than the previous acoustic approximation.

The seed parameters are not hardware-calibrated.

Therefore, architecture alone does not prove a more accurate perceived result.

### Certified Global Stylus Tracing

- **Commit**: `eb20909`.
- **Output-changing**: Yes.
- **Reference-improving**: Yes.
- **Default active**: Yes for admitted physical groove sources.
- **Expected relevance**: Medium to high on difficult groove geometry.
- **Listening evidence**: None.

The former tracer could select a local envelope maximum.

The replacement examines all applicable Catmull-Rom pieces with bounded work.

One independently reproduced accepted fixture had these former errors:

- Height error of at least `0.11957397830317671` micrometers.
- Contact-position error of at least `3.996725512525235` micrometers.

These errors can change groove height, local slope, contact force, and cartridge voltage.

The replacement result lies inside the outward numerical reference enclosure.

An earlier audit reported a `7.07`-micrometer height error and a `3.74`-micrometer position error.

The retained fixtures do not reproduce the reported `7.07`-micrometer height error.

Do not use those approximate values as verified acceptance evidence.

### Complete Vector Sliding Friction

- **Commit**: `eb20909`.
- **Output-changing**: Yes during contact motion.
- **Default active**: Yes.
- **Expected relevance**: High during intensive scratching.
- **Listening evidence**: None.

Sliding now uses the complete local groove-wall friction vector.

The solver returns reciprocal force to the tonearm and reciprocal torque to the record.

Signed slope and sliding direction now affect both modulation reaction and Coulomb reaction.

This correction can change scratch transients, reversal torque, tracking loss, and cartridge motion.

Tests verify force, torque, power, direction, and same-sample reciprocity.

They do not prove a preferred or more realistic sound.

### Reciprocal Cartridge Coil Coupling

- **Commit**: `6791e90`.
- **Output-changing**: Yes when coupled parameters are nonzero.
- **Default active**: Profile-dependent.
- **Expected relevance**: Medium for frequency response, phase, and channel interaction.
- **Listening evidence**: None.

The cartridge includes reciprocal two-channel coil coupling and a finite electrical load.

The model generates voltage from relative magnet velocity.

The electrical state returns the matching electromagnetic force in the same sample.

Tests cover complex transfer, mutual polarity, crosstalk, passivity, and time-domain energy.

The seed generator scale still has a peak-versus-RMS ambiguity.

No measured complex cartridge transfer has calibrated the complete seed model.

### Passive Magnetic-Loss Topology

- **Commit**: `49cd942`.
- **Output-changing**: Only when a profile enables nonzero loss branches.
- **Default active**: No for the current Concorde seed.
- **Expected relevance**: Potentially medium after calibration.
- **Listening evidence**: None.

The cartridge can represent frequency-dependent magnetic loss with as many as four passive relaxation branches.

All four Concorde seed branches are zero and estimated.

Therefore, this topology produces no new default-seed sound change.

It is capability work until measurements supply nonzero values.

### High-Speed Spatial Filtering

- **Output-changing**: Yes during high-rate playback and scratching.
- **Default active**: Yes.
- **Expected relevance**: High for alias reduction.
- **Listening evidence**: None.

Each groove asset contains validated `2x`, `4x`, `8x`, and `16x` spatial levels.

Each level uses a 65-tap low-pass filter before decimation.

The renderer selects and blends levels from signed source-frame travel.

Tests cover forward and reverse motion from `-20x` through `+20x`.

The filter reduces aliasing in tested signals.

It also removes groove detail before nonlinear contact at high speed.

That order can hide real contact loss and impulse forces.

Therefore, the filter is an audible anti-aliasing win with a physical-contact tradeoff.

### Stateful Output Conversion

- **Output-changing**: Yes at host sample rates below 192 kilohertz.
- **Default active**: Yes in the physical renderer.
- **Expected relevance**: High for alias control and continuity.
- **Listening evidence**: None.

One stateful converter produces supported host rates from the 192-kilohertz physical stream.

Tests cover passband response, stopband rejection, partition invariance, and snapshot continuation.

This work can reduce audible folding and block-boundary artifacts.

It does not correct contact physics that was absent before conversion.

### Sample-Timed Gesture and Control Ingress

- **Output-changing**: Yes during interactive control.
- **Default active**: Implemented in Rust.
- **Expected relevance**: High for scratch timing and feel.
- **Listening evidence**: None.

Rust retains ordered control events at exact physical frames.

Multiple reversals in one host quantum no longer collapse into one untimed tuple.

The gesture mapper preserves branch cuts, signed travel, pressure, and late-event spacing.

This work can improve timing and repeatability.

It does not add missing contact bandwidth or material hysteresis.

### Deterministic Branch Continuation

- **Output-changing**: Normally no when the same branch wins.
- **Stress relevance**: High when missed deadlines would cause silence or discontinuity.
- **Default active**: Yes.
- **Listening evidence**: No dropout listening test.

Continuation reduced the measured rapid-reversal core-solver tail substantially.

The later complete callback still missed two of 512 reversal deadlines on one test computer.

The complete changing-tracer path later exceeded all tested callback deadlines.

Therefore, this work improves resilience but does not close the audible-dropout risk.

## Work Without A Direct Perceptual Win

The following checkpoints do not normally change rendered voltage:

- Certified contact-coordinate identity.
- Snapshot value-semantic restoration.
- Canonical contact-operator extraction.
- Playback-configuration hashing.
- Verified interval arithmetic.
- Fixed-mode response catalogs.
- Point mobility enclosures.
- Radius-and-slope box diagnostics.
- Lowered and cue KKT catalogs.
- Complete KKT base-inverse certificates.

These changes support correctness claims and safe rejection.

They can prevent invalid output in exceptional cases.

They do not provide a normal-listening sound improvement by themselves.

## Contrary Evidence: Rapid Scratching Is Still Materially Wrong

The offline reference uses a reduced symmetric vertical contact model.

It is not a complete player or hardware measurement.

However, its convergence checks show a large current-path error.

The tested run covers signed integer rates from `1x` through `20x`.

It includes forward motion, reverse motion, contact loss, an impulse, and retracking.

The current single-step path has these reported differences:

| Quantity | Current error |
|---|---:|
| Wall-height normalized RMS error | `1.3655775345324883` |
| Signed wall-height integral error | `93.98129343476366%` |
| Wall-force normalized RMS error | `1.034338433310777` |
| Wall-force impulse error | `16.672264726920533%` |
| Reaction-torque normalized RMS error | `1.0034541959889511` |
| Signed torque-impulse error | `72.38062132681622%` |
| Absolute torque-impulse error | `60.03927910396862%` |
| Contact-occupancy mean absolute error | `0.21032718713637744` |

The candidate produced 265 macro contact transitions.

The reference produced 281 macro contact transitions.

The reference transition count is not fully converged.

The force and impulse differences remain materially larger than the reference convergence changes.

This evidence means the rapid-scratch path is not yet physically faithful.

The certified global tracer does not resolve the missing swept contact dynamics.

## Missing High-Impact Perceptual Work

### Swept Contact Bandwidth

One 192-kilohertz sample can cross 20 base source frames at `20x`.

The current path traces and solves contact once for that complete travel.

It can skip contact loss, recapture, force impulses, and envelope-branch changes.

This is the largest measured rapid-scratch accuracy gap.

### Tangential Reversal Memory

The model has no groove-coordinate material state.

It cannot recover pre-sliding deformation after a reversal or revisit.

This gap affects stop, catch, release, and zero-speed scratch behavior.

### Sloped Sticking

Production rejects positive-friction sticking on a sloped groove wall.

This omission removes one physically important state near zero relative speed.

It can change the onset and release of reversal transients.

### Contact Compliance

The current normal contact is rigid.

It does not model PVC indentation, recovery, footprint, pressure, or local loss.

It can make recapture forces and contact transitions too abrupt.

### iOS Scratch-Preset Activation

Rust owns the preset catalog and predictive gate behavior.

The physical iOS player does not yet use this complete Rust path.

Therefore, the preset work has not produced an iOS perceptual win.

## Focused Next Pass

The next pass must prioritize measured output changes.

1. Add bounded swept contact for high signed travel.
2. Compare its force, torque, contact, and voltage against converged microsteps.
3. Add tangential reversal state and sloped sticking.
4. Add a compliant single-contact model only with declared seed parameters.
5. Render matched stop, `+20x`, `-20x`, and reversal audio fixtures.
6. Run controlled, level-matched listening tests.
7. Move the proven Rust scratch behavior into iOS.

Do not expand the proof framework during this focused pass.

Return to formal solver certification before a public physical-leadership claim.

## Required Perceptual Evidence

Each before-and-after render must use the same source, controls, gain, and output conversion.

Store exact profile, source, control, and engine identities with each render.

Measure these values before listening:

- Band-limited voltage error against the high-rate reference.
- Force and torque impulse error.
- Contact occupancy and recapture timing.
- Reversal discontinuity energy.
- High-frequency alias energy.
- Interchannel phase and crosstalk error.
- Clipping, overload, and output level.

Use level-matched, randomized, blind comparisons for listening.

Include expert scratch performers and normal programme listeners.

Record preference, difference detection, and confidence separately.

Keep negative and inconclusive results.

## Permitted Claim Today

The engine contains a causal physical groove, pickup, cartridge, and player model.

It includes a certified global spherical stylus tracer and reciprocal contact forces.

It also contains tested high-speed anti-aliasing and exact timed-control plumbing.

## Claims Not Permitted Today

Do not claim proven superior sound quality.

Do not claim proven perceptual accuracy.

Do not claim calibrated Concorde magnetic loss.

Do not claim physically faithful `20x` scratch contact.

Do not claim the most accurate player recreation before comparative measurements and listening tests pass.

## Supporting Evidence

- [`PHYSICS_INVESTIGATION_LOG.md`](./PHYSICS_INVESTIGATION_LOG.md)
- [`PHYSICS_VALIDATION_CASES.md`](./PHYSICS_VALIDATION_CASES.md)
- [`REFERENCE_ENGINE_GAP_AUDIT.md`](./REFERENCE_ENGINE_GAP_AUDIT.md)
- `src/physical/rapid_scratch_reference.rs`
- `src/physical/stylus.rs`
- `src/physical/contact.rs`
- `src/physical/cartridge.rs`
- `src/physical/output.rs`
