# Perceptual Accuracy Report

## Report Status

- **Report date**: 2026-08-02.
- **Repository**: `record-player`.
- **Reviewed checkpoint**: Current production integration worktree.
- **Scope**: Physical engine, scratch behavior, C ABI, and native Swift integration.
- **Purpose**: Identify work that can improve audible accuracy.
- **Limit**: This report does not claim a completed listening result.

## Executive Verdict

The work has produced real waveform-changing corrections.

The strongest independently reference-verified geometric correction is the certified global stylus tracer.

The physical player, vector friction, reciprocal cartridge load, and high-speed filters can also change output.

However, no controlled listening test has proved a perceptual improvement.

The reduced one-step trace and contact candidate still differs materially from the high-rate reduced reference.

Therefore, the work does not yet support a claim of superior perceived accuracy.

The recent KKT certificate work adds proof, admission, and fail-closed design building blocks.

No production call site uses the complete block solver.

Therefore, this work does not yet improve production failure handling or rendered audio.

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
| Has the reduced rapid-scratch candidate converged to its high-rate reference? | No |
| Has an A/B render shown a clear full-output improvement? | No |
| Has a controlled listening test shown a perceptual improvement? | No |
| Are the Rust scratch presets active in native Swift? | Yes |
| Do corrected Stab and Chirp gates change physical phono voltage? | Yes |
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
- **Output-changing**: Yes during unique nonzero sliding.
- **Default active**: Yes.
- **Expected relevance**: High during intensive scratching.
- **Listening evidence**: None.

Sliding now uses the complete local groove-wall friction vector.

This correction applies when nonzero sliding is unique in the reduced contact branch.

The solver returns reciprocal force to the tonearm and reciprocal torque to the record.

Signed slope and sliding direction now affect both modulation reaction and Coulomb reaction.

This correction can change scratch transients, reversal torque, tracking loss, and cartridge motion.

Tests verify force, torque, power, direction, and same-sample reciprocity.

At an absolute slope of `0.5`, former reduced friction could over-scale force and record torque by about `11.8034%`.

This constructed counterexample is not a measured default-playback error.

The tests do not prove a preferred or more realistic sound.

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

Tests confirm alias reduction in linear signals.

It also removes groove detail before nonlinear contact at high speed.

That order can hide real contact loss and impulse forces.

Therefore, the net audible and physical benefit remains unproved.

### Stateful Output Conversion

- **Output-changing**: Yes at host sample rates below 192 kilohertz.
- **Default active**: Yes in the physical renderer.
- **Expected relevance**: High for alias control and continuity.
- **Listening evidence**: None.

One stateful converter produces supported host rates from the 192-kilohertz physical stream.

Tests cover passband response, stopband rejection, partition invariance, and snapshot continuation.

The filter design targets `100` decibels of rejection.

The permanent test requires RMS below `2e-4`, or approximately `-74` decibels relative to full scale.

That test uses one stopband tone for each supported downsample rate.

A full frequency sweep, group-delay test, and alias acceptance gate remain open.

This work can reduce audible folding and block-boundary artifacts.

It does not correct contact physics that was absent before conversion.

### Sample-Timed Gesture and Control Ingress

- **Output-changing**: Yes during interactive control.
- **Default active**: Only when the host supplies every event.
- **Expected relevance**: High for scratch timing and feel.
- **Listening evidence**: None.

Rust retains ordered control events at exact physical frames.

Multiple reversals in one host quantum no longer collapse into one untimed tuple.

The gesture mapper preserves branch cuts, signed travel, pressure, and late-event spacing.

This work can improve timing and repeatability.

Native Swift event delivery is proved through the canonical C ABI.

Browser event delivery remains open.

It does not add missing contact bandwidth or material hysteresis.

### Sample-Timed Rust Scratch Gain

- **Output-changing**: Yes during manual or preset crossfader use.
- **Default active**: Yes in the physical player and native Swift path.
- **Expected relevance**: High for scratch timing and attack shape.
- **Listening evidence**: None.

The physical player now processes the Rust scratch helper for each physical sample.

It applies the result directly to phono output volts before host conversion.

At `20x`, the former `0.004`-second onset delay buffered `0.080` source seconds.

That travel was `36.4` percent of the initial Stab span.

The delay could remove the first Stab attack.

Same-sample physical confirmation now preserves that attack and the early click events.

Player, C ABI, and native Swift tests prove that scratch gain changes rendered samples.

This result is a real production waveform change.

Stab now passes the forward stroke and mutes the return.

Chirp now cuts each physical direction edge and closes before a predicted reversal.

Opposite intent can close the fader, but it cannot commit the new physical stroke.

A physical-player test verifies Stab and Chirp phono output at `1x`, `8x`, and `20x`.

The exact `20x` path now accepts floating-point drift of at most `1e-12` and clamps to the physical limit.

The technique fractions and crossfader envelope remain unmeasured.

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

The complete block solver has no production call site.

Some production checks can prevent invalid output in exceptional cases.

Certificate-only items do not change current production failure handling.

They do not provide a normal-listening sound improvement by themselves.

## Contrary Evidence: The Reduced Rapid-Scratch Candidate Still Differs Materially

The offline reference uses a reduced symmetric vertical contact model.

It is not a complete player or hardware measurement.

However, its convergence checks show a large difference between the reduced candidate and reference.

The tested run covers signed integer rates from `1x` through `20x`.

It includes forward motion, reverse motion, contact loss, an impulse, and retracking.

The reduced one-step trace and contact candidate has these reported differences:

| Quantity | Reduced candidate error |
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

These metrics exclude the coupled deck, cartridge, phono stage, and host output.

They show a structural contact-bandwidth defect.

They are not measurements of complete output-voltage error.

This evidence prevents a physically faithful rapid-scratch claim.

The certified global tracer does not resolve the missing swept contact dynamics.

## Missing High-Impact Perceptual Work

### Swept Contact Bandwidth

One 192-kilohertz sample can cross 20 base source frames at `20x`.

The production path traces and solves contact once for that complete travel.

It can skip contact loss, recapture, force impulses, and envelope-branch changes.

This is the largest measured rapid-scratch accuracy gap.

### Tangential Reversal Memory

The model has no groove-coordinate material state.

It cannot recover pre-sliding deformation after a reversal or revisit.

This gap affects stop, catch, release, and zero-speed scratch behavior.

### Sloped Sticking

Production now selects zero traction when relative speed is at most `1.0e-9` meters per second.

This fallback prevents an ordinary stopped, modulated groove from aborting playback.

The threshold is a numerical regularization value.

The model still lacks identified static traction for sloped multi-wall contact.

This gap can change reversal onset and release.

### Contact Compliance

The current normal contact is rigid.

It does not model PVC indentation, recovery, footprint, pressure, or local loss.

It can make recapture forces and contact transitions too abrupt.

Enable compliance only after measurements or passive fitting identify its parameters.

Keep an estimated compliance model disabled by default.

### Real-Time Callback Deadline

The stable rapid-reversal checkpoint missed two of 512 complete callback deadlines on one test computer.

A later provisional changing-tracer run missed all 512 deadlines.

A bounded swept-contact change must pass complete callbacks on each minimum supported device.

### Output Level and Headroom Calibration

The engine keeps physical output in volts and requires an explicit host full-scale value.

The product value, measurement source, uncertainty, and intended headroom remain unregistered.

The phono gain, noise, overload, and recovery model also lacks measured hardware calibration.

### Tonearm Pivot and Radial Motion

The pickup uses a local two-axis equivalent-mass model.

It does not integrate the complete tonearm pivot angle.

This gap can change radial travel, skating response, and skip recovery.

### Friction Calibration

The seed groove friction coefficient is a constant `0.25`.

Measurements do not yet identify friction across load, speed, direction, temperature, slope, contamination, and wear.

This gap limits claims about stop, reversal, sliding, and record damage.

### Stylus Tip Geometry

Production tracing supports a spherical tip.

It does not yet support measured elliptical or line-contact tip profiles.

Tip shape can change tracing, contact pressure, phase, and high-frequency response.

### Native Scratch-Preset Calibration

Rust owns the preset catalog and its learned-span gate behavior.

It learns a stroke span only after one completed stroke.

The model cannot predict the first unseen endpoint.

It does not model a measured crossfader curve, cut-in, latency, bleed, bounce, or noise.

`ScratchPerformance` now has physical-player, renderer, C ABI, and native Swift call sites.

Swift contains adapter code and no technique equation.

The integration changes actual phono output samples.

It is a plausible perceptual improvement, but no listening test proves it.

The identified Stab and Chirp topology defects are corrected.

Stab, Chirp, Flare, Crab, and Drum still have calibration gaps.

The Skipproof paper strengthens this limitation.

Its authors preserved expert record and crossfader recordings as paired lookup tables.

They report that precise coordination is necessary to preserve each technique.

Our current fixed stroke fractions do not provide equivalent calibration evidence.

The paper does not justify a new preset in this pass.

Rolltear, Forward, Uzi, and Twiddle remain later research candidates.

### Browser Canonical Activation

The browser cache and demand path use the Rust and WASM page interfaces.

The canonical physical program output is not active across the complete browser path.

Browser activation must remove duplicate technique equations after Rust integration.

## Focused Next Pass

The next pass must prioritize measured output changes.

1. Extend the reference through the coupled deck, pickup, cartridge, phono stage, and host output.
2. Add bounded, event-aware swept contact for high signed travel.
3. Compare force, torque, contact, and voltage against converged microsteps.
4. Add direction-specific span prediction with error-aware confidence.
5. Add tangential reversal state and identified sloped sticking.
6. Render matched stop, `+20x`, `-20x`, and reversal audio fixtures.
7. Run controlled, level-matched listening tests.

Freeze unrelated proof work during this focused pass.

Implement proof that the audible change requires in the same pass.

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

Include expert scratch performers and listeners who do not scratch records.

Record preference, difference detection, and confidence separately.

Keep negative and inconclusive results.

## Permitted Claim Today

The engine contains a causal, reduced, spherical-tip seed model for the groove, pickup, cartridge, and player.

It includes a certified global spherical stylus tracer and reciprocal contact forces.

It also contains tested linear spatial filters and sample-timed Rust control plumbing.

## Claims Not Permitted Today

Do not claim proven superior sound quality.

Do not claim proven perceptual accuracy.

Do not claim calibrated Concorde magnetic loss.

Do not claim physically faithful `20x` scratch contact.

Do not claim the most accurate player recreation before comparative measurements and listening tests pass.

## Supporting Evidence

- [`PHYSICS_INVESTIGATION_LOG.md`](./PHYSICS_INVESTIGATION_LOG.md)
- [`PHYSICS_VALIDATION_CASES.md`](./PHYSICS_VALIDATION_CASES.md)
- [`REFERENCE_ENGINE_GAP_AUDIT.md`](./REFERENCE_ENGINE_GAP_AUDIT.md) is historical and audits a retired repository. This report supersedes it.
- `src/physical/rapid_scratch_reference.rs`
- `src/physical/stylus.rs`
- `src/physical/contact.rs`
- `src/physical/cartridge.rs`
- `src/physical/output.rs`
