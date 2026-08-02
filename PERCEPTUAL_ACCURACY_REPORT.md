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
| Does bounded swept contact reduce reduced-reference phono error? | Yes |
| Has the reduced rapid-scratch candidate converged to its high-rate reference? | No |
| Has an A/B render shown a clear full-output improvement? | No |
| Has a controlled listening test shown a perceptual improvement? | No |
| Are the Rust scratch presets active in native Swift? | Yes |
| Does the iOS decoded-audio cache preserve exact PCM? | Yes in app commit `c320e5b` |
| Does the active iOS app use the full physical 45/45 renderer? | No |
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

The picture-record spiral stores encoded digital bytes.

The physical engine does not interpret its visible pixels as micrometer wall displacement.

The host must recover ECDC, decode stereo PCM, and then make the virtual 45/45 groove.

### Exact Decoded-Programme Cache

- **Output-changing**: Yes on repeated iOS programme loads.
- **Default active**: Yes in app commit `c320e5b`.
- **Expected relevance**: Medium to high for source consistency.
- **Listening evidence**: None.

The former iOS cache encoded direct ECDC output as 68-kilobit-per-second Opus.

Later loads could therefore use different samples from the first direct decode.

The replacement cache stores 32-bit floating-point PCM.

Its test requires exact left and right samples after storage and restoration.

This removes one lossy generation before groove construction and scratch playback.

It does not remove losses already present in the ECDC programme.

The cache uses approximately 23.04 megabytes for each stereo programme minute.

The product cache-size and eviction policy remain unverified.

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

### Needle-Lift Foley

- **Output-changing**: Yes during a lift event.
- **Default active**: Yes when surface effects are active.
- **Expected relevance**: Medium for interaction realism.
- **Listening evidence**: None.

The canonical engine now produces a lighter thump and short crackle burst during needle lift.

This behavior previously existed only in the native vendored copy.

The migration removes that engine divergence.

The gains are estimates and do not identify current hardware.

Therefore, this change improves event continuity but does not prove hardware accuracy.

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

The exact `20x` path now accepts floating-point drift of at most `1e-10` and clamps to the physical limit.

The technique fractions and crossfader envelope remain unmeasured.

### Direction-Specific Scratch Learning

- **Output-changing**: Yes for asymmetric forward and reverse strokes.
- **Default active**: Yes for automatic presets.
- **Expected relevance**: High for repeated scratch timing.
- **Listening evidence**: None.

Forward and reverse strokes now keep independent span estimates.

A short push no longer compresses the fader pattern for a longer pull.

Each direction also keeps an independent observation count and confidence value.

The first unseen stroke in either direction still uses a provisional seed.

### Bounded Swept Contact

- **Output-changing**: Yes above five predicted source frames for each physical sample.
- **Reference-improving**: Yes in the reduced rapid-scratch reference.
- **Default active**: Yes for all physical groove source types.
- **Expected relevance**: High during rapid playback and scratching.
- **Listening evidence**: None.

Production now divides rapid contact travel into as many as four bounded steps.

Each step runs the certified tracer and the reciprocal player solve.

Travel through `20x` uses four steps instead of one.

The reduced signed rate sweep shows phono normalized RMS error reductions from approximately `43%` through `47%`.

The stop and reversal fixture shows approximately `44%` lower phono normalized RMS error.

This is the strongest measured electrical-output improvement in this audit.

The evidence is open-loop and does not include hardware listening tests.

The contact-transition count becomes worse in the rate-sweep fixture.

The stop and reversal cartridge absolute-integral error also becomes worse.

The current certified runtime tracer misses every measured callback deadline.

Therefore, the correction is physically useful but not ready for real-time product activation.

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

## Rapid-Scratch Reference Result and Contrary Evidence

The offline reference uses a reduced symmetric vertical contact model.

It is not a complete player or hardware measurement.

However, its convergence checks show a large difference between the reduced candidate and reference.

The tested run covers signed integer rates from `1x` through `20x`.

It includes forward motion, reverse motion, contact loss, an impulse, and retracking.

The former one-step trace and contact candidate has these reported differences:

| Quantity | Reduced candidate error |
|---|---:|
| Wall-height normalized RMS error | `1.3655193973757973` |
| Signed wall-height integral error | `93.98733854738484%` |
| Wall-force normalized RMS error | `1.0158164015715798` |
| Wall-force impulse error | `26.03553334882623%` |
| Reaction-torque normalized RMS error | `1.0024104080839553` |
| Signed torque-impulse error | `76.41911593334955%` |
| Absolute torque-impulse error | `64.18499271665087%` |
| Contact-occupancy mean absolute error | `0.21010711785380817` |
| Left cartridge-voltage normalized RMS error | `0.5863220800818523` |
| Right cartridge-voltage normalized RMS error | `0.584662684658089` |
| Left phono-voltage normalized RMS error | `0.7743228579914596` |
| Right phono-voltage normalized RMS error | `0.7494342423073957` |

The former candidate produced 265 macro contact transitions.

The reference produced 269 macro contact transitions.

The reference transition count is not fully converged.

The force and impulse differences remain materially larger than the reference convergence changes.

The mechanical reference excludes a coupled deck and reciprocal cartridge force.

They show a structural contact-bandwidth defect.

Identical cartridge and phono observers process both mechanical trajectories.

These observers show that the mechanical difference reaches voltage output.

The observers do not model a complete coupled player.

A ramped stop and reversal fixture gives phono-voltage normalized RMS errors near `0.98`.

That fixture loses approximately `48%` of reference absolute phono output.

The bounded correction improves the primary reference measures:

| Quantity | Former one-step error | Bounded-sweep error |
|---|---:|---:|
| Wall-height normalized RMS | `1.3655193973757973` | `0.5122237543419718` |
| Wall-force normalized RMS | `1.0158164015715798` | `0.4924456991670574` |
| Reaction-torque normalized RMS | `1.0024104080839553` | `0.6568966477312397` |
| Contact-occupancy mean absolute | `0.21010711785380817` | `0.0911330880694634` |
| Left phono normalized RMS | `0.7743228579914596` | `0.43714264111544443` |
| Right phono normalized RMS | `0.7494342423073957` | `0.3959241852448356` |

The stop and reversal phono normalized RMS error decreases from approximately `0.98` to `0.55`.

Its phono absolute-integral error decreases from approximately `48%` to approximately `39%`.

However, the bounded candidate produces only 225 macro contact transitions.

The reference produces 269 transitions.

The stop and reversal cartridge absolute-integral error increases from approximately `36%` to `52.5%`.

This evidence still prevents a physically faithful rapid-scratch claim.

The bounded correction does not resolve all swept contact dynamics.

## Missing High-Impact Perceptual Work

### Swept Contact Bandwidth

One 192-kilohertz sample can cross 20 base source frames at `20x`.

Production now traces and solves as many as four bounded contact steps.

This correction materially lowers reduced-reference error.

It still misses reference contact transitions and some electrical integrals.

The current runtime tracer also prevents real-time product activation.

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

The certified one-step normal path misses 512 of 512 measured callback deadlines.

Its measured median is approximately `2.35` milliseconds for a `0.667`-millisecond deadline.

The bounded rapid path also misses 512 of 512 measured callback deadlines.

Its measured median is approximately `7.37` milliseconds.

Precompute certified contact-envelope data before the callback.

Then pass complete callbacks on each minimum supported device.

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

Native commit `b86dc4f` reads preset identifiers and metadata from canonical Rust.

App commit `c320e5b` verifies all eight presets through the iOS binary.

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

1. Precompute certified contact-envelope data outside the audio callback.
2. Extend the reference through the coupled deck, pickup, cartridge, phono stage, and host output.
3. Compare complete production voltage against converged microsteps.
4. Make span confidence decrease when observed stroke lengths are inconsistent.
5. Add tangential reversal state and identified sloped sticking.
6. Render matched stop, `+20x`, `-20x`, and reversal audio fixtures.
7. Run controlled, level-matched listening tests.

Freeze unrelated proof work during this focused pass.

Implement proof that the audible change requires in the same pass.

Return to formal solver certification before a public physical-leadership claim.

### Callback Proof-Validation Correction

The callback no longer recalculates immutable trace-certificate SHA-256 identities.

Normal mean render time decreases by approximately 27.7 percent in the measured fixture.

Rapid-reversal mean render time decreases by approximately 34.1 percent.

This correction does not change the rendered sound.

It is an activation and scratch-resilience improvement only.

The complete physical path still misses every measured callback deadline.

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

It also contains bounded swept contact, tested spatial filters, and sample-timed Rust control plumbing.

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
