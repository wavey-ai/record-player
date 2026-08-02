# Physical Record Player Investigation Log

## Purpose

This file records the investigation of the physical record player model.

The log does not prove that a conclusion is correct. It records evidence, uncertainty, and possible tests.

Update this file when evidence changes a conclusion. Keep rejected ideas because they can prevent repeated errors.

Store exact fixtures, artifacts, error bounds, and reproduction commands in `PHYSICS_VALIDATION_CASES.md`.

## Terms

- **Published**: A manufacturer or a primary technical source gives the value.
- **Calculated**: A stated equation and published inputs give the value.
- **Estimated**: The value has no applicable measurement or published specification.
- **Measured**: A repeatable test on identified hardware gives the value.
- **Seed profile**: A usable parameter set that contains estimates and needs measurement.
- **Calibrated profile**: A parameter set that passes its specified hardware measurements.
- **C1 join**: A join where displacement and its first derivative match.
- **Class A**: The trace class that proves strict spherical-envelope concavity.
- **Class B**: The trace class that uses bounded piecewise isolation during each trace.
- **Normal cone**: The set of possible surface-normal directions at a non-smooth contact.
- **Projected wall force**: The contact multiplier along one 45-degree wall coordinate.

## Confidence Scale

- **High**: Independent evidence supports the conclusion, and no known evidence conflicts with it.
- **Medium**: The evidence supports the conclusion, but an important test or source is absent.
- **Low**: The conclusion is a working hypothesis.

## Requirement Ledger

Use this ledger to track requirements across the chronological findings.

- **RP-001 — Canonical source**: Keep all player physics in the top-level `record-player` crate. **Status**: In progress.
- **RP-002 — Consumer boundary**: Make Swift and other hosts depend on the canonical Rust crate or its generated binary interface. **Status**: In progress.
- **RP-003 — Full replacement**: Remove the previous acoustic heuristics and JavaScript gesture physics. Do not keep a legacy rendering mode. **Status**: Open.
- **RP-004 — Timed controls**: Apply every control at an exact physical frame. Preserve all ordered gesture samples. **Status**: Implemented in Rust.
- **RP-005 — Canonical gestures**: Convert raw pointer angle, time, radius, and pressure in Rust. Support bounded travel through repeated reversals. **Status**: Implemented in Rust.
- **RP-006 — Physical units**: Use explicit SI units for mechanical, contact, electrical, and geometric quantities. **Status**: Implemented in Rust.
- **RP-007 — Transactional state**: Leave state and output unchanged after an invalid input, page miss, or failed physical step. **Status**: Implemented; final audit remains open.
- **RP-008 — Passive coupling**: Couple contact, suspension, generator, electrical load, and record reaction in the same sample. **Status**: Implemented for the midpoint tangent-plane model; curved-geometry validation remains open.
- **RP-009 — Groove geometry**: Trace both 45/45 walls with declared stylus geometry and actual groove radius. **Status**: Implemented for a spherical tip.
- **RP-010 — Tip geometry**: Add measured elliptical and line-contact profiles when product profiles require them. **Status**: Open.
- **RP-011 — Bounded streaming cut**: Cut arbitrary PCM chunks into deterministic, bounded pages with exact restore. **Status**: Implemented in Rust.
- **RP-012 — Fixed render cache**: Publish complete, identified pages without locks or render-time allocation. Reject stale or corrupt pages. **Status**: Implemented in Rust, WASM, and the browser worklet ingestion path.
- **RP-013 — Rapid-scratch contact**: Preserve nonlinear contact effects during sustained travel and reversals through plus or minus 20x. **Status**: Open.
- **RP-014 — Output conversion**: Use one stateful, anti-aliased converter from 192 kHz to each supported host rate. **Status**: Implemented in Rust.
- **RP-015 — Snapshot identity**: Restore bit-identical state only with the exact profile, source, representation, generation, rate, and clock. **Status**: Implemented in Rust.
- **RP-016 — Browser activation**: Cut pages in a worker, validate them in the worklet, and render with the canonical WASM engine. **Status**: Cache and demand path implemented; canonical programme output remains open.
- **RP-017 — Native activation**: Call the canonical crate from Swift through the generated C interface. Keep physics out of Swift. **Status**: Implemented in `vin.yl.native`.
- **RP-018 — Remaining consumers**: Migrate `vin.yl.app`, `vin.yl.native`, `vin.yl.web`, `vin.yl.player`, and requested Bitneedle Rust cores. **Status**: Open.
- **RP-019 — Cutter scope**: Describe the current cutter as an ideal virtual cut until measured lathe and pressing stages exist. **Status**: Required.
- **RP-020 — Calibration gate**: Do not call a seed profile calibrated. Require identified measurements, uncertainties, artifacts, and digests. **Status**: Implemented in Rust.
- **RP-021 — Leadership claim**: Do not claim physical leadership until comparative hardware tests pass with stated uncertainty. **Status**: Open.
- **RP-022 — Contrary evidence**: Retain failed tests, rejected hypotheses, and conflicting sources. Update their status when evidence changes. **Status**: Required.
- **RP-023 — Certified tracing**: Find the global stylus envelope with bounded work. Do not depend on an unproved local optimum. **Status**: Implemented for current spherical tracing sources.
- **RP-024 — Cartridge detail**: Model complex impedance, crosstalk phase, magnetic loss, level dependence, and temperature dependence. **Status**: Linear passive coupling and magnetic loss are implemented. Measurements and nonlinear effects remain open.
- **RP-025 — Structural motion**: Add profile-required arm geometry, spindle play, eccentricity, warp, record flexure, and distributed hand contact. **Status**: Open.
- **RP-026 — Material contact**: Add measured wall compliance, speed-dependent friction, temperature, wear, damage, and contamination when required. **Status**: Open.
- **RP-027 — Reference convergence**: Compare rapid contact with a converged high-rate solver. Register numeric error limits before release. **Status**: In progress.
- **RP-028 — Device deadlines**: Pass callback, memory, paging, reversal, and throttling tests on each minimum supported device. **Status**: Core tail improved; complete callbacks still fail. See `PVC-003`.
- **RP-029 — Portable dependency**: Pin native consumers to one published crate or versioned binary artifact. Remove workspace-only path assumptions. **Status**: Open.
- **RP-030 — Durable page backing**: Retain canonical pages after cache eviction. Serve seeks and prefetch without recutting the record. **Status**: Implemented and tested through the browser worklet; product retention limits remain open.
- **RP-031 — Three-dimensional contact reduction**: State when independent 45/45 wall envelopes are exact. Add longitudinal and rotational dynamics when profiles require them. **Status**: Reduced rigid-sphere geometry verified; finite-patch and extra dynamics remain open.
- **RP-032 — Host voltage calibration**: Keep phono output in volts inside physics. Convert volts through an explicit host full-scale boundary. **Status**: Boundary implemented in Rust, C, WASM, and native Swift. Product calibration evidence remains open.
- **RP-033 — Same-wall multiple contact**: Detect separated possible global positions. Do not treat interval overlap as equality. **Status**: Contact sets propagate through production paths. Unqualified sets reject safely. Certified equality and measured compliance remain open.
- **RP-034 — Canonical scratch techniques**: Keep the catalog, prediction, and automatic crossfader policy in Rust. Use exact same-sample physical travel. **Status**: Rust core implemented. Player, calibration, iOS, and browser integration remain open.
- **RP-035 — Non-smooth contact**: Reject unresolved normal cones until the contact solve represents their complete force set. **Status**: In progress.
- **RP-036 — Surface friction geometry**: Resolve friction in the complete local surface basis. Couple all components in the same contact solve. **Status**: Sliding surface and skating-port coupling are implemented. Sloped sticking, coupled uniqueness proof, and measurements remain open.
- **RP-037 — Trace asset admission**: Bind fixed trace work, representation bytes, page identity, geometry, and actual wall-slope bounds. **Status**: Implemented. An authoritative full-record catalog remains open.
- **RP-038 — Tangential contact memory**: Add identified along-groove pickup motion and local tangential material state. Preserve that state through reversals. **Status**: Certified coordinate plumbing is implemented. Continuous state mapping and sloped sticking remain open.
- **RP-039 — Fixed-mode admission**: Certify each fixed normal-contact operator over its complete profile and source domain. **Status**: Config identity and point mobility are implemented. Interval proofs remain open.

## 2026-08-01: Repository and History Investigation

### Claim: No richer cartridge model was lost from the inspected history

- **Status**: Working conclusion.
- **Confidence**: High for the inspected workspace and local Git data.
- **Evidence**: The search covered branches, tags, reflogs, historical blobs, stashes, Rust code, and JavaScript code.
- **Evidence**: Commit `d55931f3374` added a PCM-curvature filter and platter-phase telemetry.
- **Evidence**: Commit `4c43dfa7b4` renamed that filter and added a separate high-frequency limiter.
- **Evidence**: Commit `e767d2f` extracted the engine without removing an electromechanical cartridge model.
- **Possible error**: An unpushed remote branch, deleted external repository, or unavailable local object can contain other code.
- **Disproof test**: Find code that models groove geometry, stylus contact, suspension, generator voltage, cartridge loading, or RIAA playback.

### Claim: The previous acoustics documentation overstates the cartridge implementation

- **Status**: Working conclusion.
- **Confidence**: High.
- **Historical evidence**: The dead `yl.vin/apps/play/ACOUSTICS.md` called the previous path a physical model.
- **Evidence**: The previous stylus function uses local PCM curvature and playback rate.
- **Evidence**: The implementation has no groove displacement, 45/45 geometry, cantilever state, generator state, or cartridge electrical load.
- **Possible error**: Product terms can use a broader meaning than the physical terms used in this investigation.
- **Disproof test**: Trace each README claim to an implemented state equation and a physical-unit test.

### Claim: The Rust source copies currently contain equal engine bytes

- **Status**: Observed during the audit.
- **Confidence**: High at the observation time.
- **Evidence**: The top-level, native, application, and Infidelity copies matched during byte comparison.
- **Possible error**: The copies can diverge after any later edit.
- **Disproof test**: Compare all copies again after each change.
- **Decision**: Remove executable copies and make top-level `record-player` authoritative.

### Historical claim: The dead `yl.vin` route used an independent JavaScript engine

- **Status**: Historical and out of scope.
- **Confidence**: High at the observation time.
- **Evidence**: The dead route contained a separate JavaScript transport and acoustic path.
- **Decision**: Do not treat this route as an active consumer or release blocker.

## 2026-08-01: Physical Gaps in the Previous Engine

### Direct PCM sampling

- **Claim**: Direct PCM resampling is not a physical cartridge signal path.
- **Confidence**: High.
- **Evidence**: A magnetic cartridge responds to motion, while the old path reads PCM amplitude directly.
- **Possible error**: Direct PCM can be an intentional perceptual approximation.
- **Disproof test**: Demonstrate correct zero-speed output, reverse polarity, RIAA response, and electrical loading from the direct-amplitude model.

### Movement gate

- **Claim**: The old movement-gain threshold is not physical.
- **Confidence**: High.
- **Evidence**: The gain changes abruptly near a fixed normalized playback rate.
- **Possible error**: A real noise floor can hide low-speed output, but it does not create this transfer function.
- **Disproof test**: Measure cartridge output through a controlled approach to zero speed.

### Stylus tracing

- **Claim**: The old curvature filter is not a finite-radius stylus contact model.
- **Confidence**: High.
- **Evidence**: It does not use tip radius, groove radius, groove-wall geometry, or a contact equation.
- **Possible error**: It can approximate one audible consequence over a limited signal set.
- **Disproof test**: Compare its harmonic distortion against finite-tip tracing equations across record radii.

### Platter and hand motion

- **Claim**: The old spring and rate blend do not model separate platter and record bodies.
- **Confidence**: High.
- **Evidence**: The old state has one rendered rate and no moment of inertia in physical units.
- **Possible error**: Tuned state interpolation can match a small set of measured gestures.
- **Disproof test**: Compare startup, grab, slip, reverse, release, and coast traces against identified hardware.

### Rapid control

- **Claim**: One untimestamped control tuple per audio quantum loses rapid gesture detail.
- **Confidence**: High.
- **Evidence**: Multiple events in one quantum can collapse into the last visible tuple.
- **Possible error**: A host can already limit events to one update per quantum.
- **Disproof test**: Send multiple reversals within one quantum and compare intended and rendered event times.

### Render contention

- **Claim**: Returning a silent quantum after a failed lock is not physically faithful or resilient.
- **Confidence**: High.
- **Evidence**: The behavior removes all cartridge output for that quantum.
- **Possible error**: The lock can be unreachable in a correctly configured host.
- **Disproof test**: Measure lock failures and output discontinuities during load, seeking, and rapid scratching.

## 2026-08-01: New Mechanics Foundation

### Implemented

- The engine now has separate platter and record angular states.
- The configuration uses SI units for inertia, torque, velocity, force, radius, and time.
- The motor has explicit `Off`, `Servo`, and `Brake` modes.
- The servo uses proportional and integral control with torque limits and anti-windup.
- The slipmat and hand constraints use a simultaneous active-set solve.
- Hand control accepts normal force, contact radius, velocity, and an optional position target.
- The solver retains fractional integration time between calls.
- Invalid controls leave the previous physical state unchanged.
- Versioned snapshots validate configuration and independent body phases.

### Test results

- Fifteen mechanics tests pass on 2026-08-01.
- One-call and partitioned advances produce equal state for the tested case.
- A two-second call equals two one-second calls for the tested case.
- Internal slipmat torque conserves angular momentum when external torques are zero.
- Friction does not increase isolated mechanical energy in the tested case.
- A firm estimated hand contact reverses the record while the platter continues forward.

### Important uncertainty

- The mechanics profile is a seed profile.
- The platter inertia uses a uniform-disc calculation, not a measured Technics inertia.
- The motor transfer function is estimated from the published startup time.
- The bearing, slipmat, and hand parameters are estimates.
- The tests show numerical behavior, not hardware calibration.
- The active-set solver still needs randomized complementarity and chatter tests.
- Minimum supported devices still need real-time performance tests.

### Tests that can reject the mechanics model

- Measure platter acceleration with a high-rate optical encoder.
- Measure powered braking and motor-off coast separately.
- Measure platter and record motion during a force-controlled grab.
- Repeat the test with identified records and slipmats.
- Measure hand force and touch radius during representative scratches.
- Compare measured traces with uncertainty bands before profile calibration.

## Physical Signal Path

The canonical path replaces direct PCM output. It does not preserve the previous acoustic heuristics.

1. Apply record RIAA equalization to the source outside the audio callback.
2. Convert left and right velocity signals to 45/45 groove displacement.
3. Store the groove as a spatial asset in meters.
4. Trace both groove walls with the configured stylus geometry.
5. Solve stylus, cantilever, tonearm, and groove contact together.
6. Generate moving-magnet voltage from relative magnet velocity.
7. Solve cartridge resistance, inductance, load resistance, and load capacitance.
8. Apply causal playback RIAA equalization and the phono stage.
9. Feed stylus reaction torque back into record mechanics.

## Published Reference Values

### Technics SL-1200MK7

- Nominal speeds include 33 1/3 RPM and 45 RPM.
- Starting torque is 0.18 N m.
- Startup time to 33 1/3 RPM is 0.7 seconds.
- The platter assembly mass is approximately 1.8 kg.
- The platter diameter is 332 mm.
- The tonearm effective length is 230 mm.
- Source: <https://www.technics.com/sg/products/dj-series/sl-1200mk7.specs.html>

### Ortofon Concorde MKII Scratch

- Output is 10 mV at 1 kHz and 5 cm/s.
- Tracking ability is 120 micrometers at 315 Hz.
- The spherical stylus radius is 18 micrometers.
- Dynamic lateral compliance is 14 micrometers per mN.
- Recommended vertical tracking force is 4 g.
- Channel separation is 22 dB at 1 kHz.
- Internal resistance is 1200 ohms.
- Internal inductance is 850 mH.
- Recommended load resistance is 47 kilohms.
- Recommended load capacitance is 200 pF to 400 pF.
- Source: <https://ortofon.com/products/concorde-mkii-scratch>
- Test-record source: <https://ortofon.com/products/ortofon-test-record>

### RIAA time constants

- The model will use 3180 microseconds, 318 microseconds, and 75 microseconds.
- Record and playback filters will use reciprocal normalized responses.
- An explicit cutter bandwidth will make the record filter proper.
- Source: <https://lyngdorf.steinwaylyngdorf.com/wp-content/uploads/2020/07/Lyngdorf-Millenium-ADC-Owners-Manual.pdf>

### Groove and stylus tracing

- The tracing model will use rounded-tip tangency equations.
- The test radii are 14.6 cm, 10.3 cm, and 6.0 cm.
- Source: <https://www.aes.org/e-lib/download.cfm/22236.pdf?ID=22236>

## Calculated Seed Values

- A 4 g tracking force is 0.0392266 N under standard gravity.
- A 14 micrometer per mN compliance is 0.014 m/N.
- The corresponding lateral suspension stiffness is approximately 71.43 N/m.
- A 22 dB separation gives an amplitude leakage ratio of approximately 0.07943.
- A 300 pF load gives an approximate electrical resonance near 9.97 kHz with 850 mH.
- These calculations do not make the seed profile calibrated.

## Values That Need Measurement

- Platter moment of inertia.
- Motor torque against speed and error.
- Powered braking torque.
- Bearing static and kinetic friction.
- Slipmat static and kinetic friction for each supported setup.
- Stylus and magnet effective mass.
- Mechanical damping and vertical compliance.
- Tonearm effective mass and bearing friction.
- Groove-wall compliance and friction.
- Cable and input capacitance on each target device.
- Generator crosstalk phase and frequency response.
- Cutter bandwidth and virtual cutting level.
- Phono overload behavior and headroom.

## Claim Gate

Do not call a profile calibrated while any required measurement remains estimated.

Do not claim physical leadership from code complexity or passing unit tests.

Require measured hardware traces, published test data, and blind listening results for comparative claims.

Record failed tests and contrary evidence in this file. Do not remove them when the model changes.

## 2026-08-01: Implemented Calibration Gate

### Implemented checks

- The seed manifest contains every serialized scalar and enum configuration value.
- Each evidence row must use one known path.
- Each path must occur exactly once.
- Each evidence value and unit must match its current configuration value.
- Each physical parameter identifies its required calibration test.
- A calibrated profile requires direct measurement for each physical parameter.
- A calibrated cartridge coefficient must identify `DirectMeasurement` as its source type.
- The required suite contains 20 hardware validation tests.
- The added tests cover motor ripple, record geometry, material wear, and cartridge magnetic loss.
- Each test requires one measured and passing result.
- Each result requires measurements, uncertainty values, an artifact location, and a SHA-256 digest.
- Each calibrated test requires registered numeric limits for its named metrics and units.
- Each uncertainty interval must stay inside the registered range.
- A reported verdict must equal the result calculated from the registered limits.
- The physical solver accepts only the canonical 192 kHz internal rate.

## 2026-08-01: Physical Engine Implementation Checkpoint

This checkpoint records implemented behavior. It does not record a calibrated accuracy claim.

### Groove representation

- The cutter applies a causal record RIAA response before displacement integration.
- The groove stores lateral and vertical displacement in meters at 192 kHz.
- The encoder converts stereo wall velocity with the 45/45 matrix.
- The decoder uses the exact inverse matrix.
- The cut report checks radial fit and adjacent-turn land clearance.
- The asset records source type, cut settings, format version, and a SHA-256 content identity.
- Custom deserialization rejects changed samples, metadata, reports, and spatial levels.

### High-speed tracing

- The asset contains deterministic 2x, 4x, 8x, and 16x spatial levels.
- Each octave uses a symmetric 65-tap low-pass filter before decimation.
- The renderer selects levels from the absolute source-frame advance.
- The renderer blends adjacent levels before nonlinear stylus tracing.
- The paged path uses absolute 64-bit frame coordinates and validated halos.
- Tests compare contiguous and paged results across page seams in both directions.
- The tested speed range is -20x through +20x.

### High-speed tracing limits

- **Claim**: The spatial pyramid reduces aliasing during rapid scratching.
- **Confidence**: High within the tested signals and speed range.
- **Possible error**: A narrow transition band can still fold at an untested speed or programme frequency.
- **Disproof test**: Sweep programme frequency and speed continuously, then measure all folded components.
- **Limit**: Advances above 16 frames use the 16x level.
- **Limit**: The level blend is not a continuously variable ideal low-pass filter.
- **Limit**: Filter tests are engineering tests, not hardware measurements or listening tests.

### Stylus and pickup

- The implemented tracer uses a finite spherical tip radius.
- It traces both 45-degree groove walls independently.
- The pickup solver contains one stylus mass and one tonearm body for each motion axis.
- One suspension connects the stylus mass and tonearm bodies.
- The implicit active-set solve enforces nonnegative wall force and nonpenetration.
- The cue control removes wall constraints and supports the raised body.
- Vertical tracking force and anti-skate force act on the tonearm bodies.
- Groove friction and modulation reaction feed torque into record mechanics.

### Cartridge and electrical load

- The moving-magnet model generates voltage from relative magnet velocity.
- Coil resistance and inductance form part of the electrical state.
- Load resistance and capacitance form part of the electrical state.
- The pickup and cartridge solve reciprocal force in the same internal sample.
- The cartridge supplies an affine current-step force to the contact solve.
- The configured channel matrix represents balance and separation.
- The stateful phono stage applies causal playback RIAA after the cartridge load.
- The profile owns the complete `RecordCutConfig` for groove and phono consistency.

### Deck, controls, and rendering

- Separate platter and record bodies use SI-unit inertia and torque.
- Motor `Off`, `Servo`, and `Brake` modes have explicit torque limits.
- Slipmat and hand constraints use one simultaneous active-set solve.
- Controls carry absolute internal sample times and sequence numbers.
- A bounded single-producer, single-consumer queue moves controls without locks.
- Queue backpressure leaves an event pending instead of dropping it.
- A late event moves to the current sample because rendered output cannot change.
- Render blocks use preallocated memory and do not acquire a mutex.
- A render error restores all future-affecting state and leaves caller output unchanged.
- The host-rate renderer keeps the same transaction across all internal chunks.
- The output resampler uses exact rational clocks and a stateful polyphase filter.

## 2026-08-01: Defects Found During Implementation

### Incomplete mechanics snapshot

- **Status**: Fixed.
- **Evidence**: Slipmat torque from one step affects the next active-set solve.
- **Evidence**: The first snapshot format did not store that torque.
- **Effect**: A restore could diverge on the first coupled step.
- **Fix**: Snapshot version 2 stores previous contact torques and bearing state.
- **Test**: An exact continuation test compares the first step after restore.

### Floating-point snapshot round trip

- **Status**: Fixed.
- **Evidence**: Default JSON formatting changed some configuration values by one ULP.
- **Effect**: Exact snapshot identity checks rejected valid persisted snapshots.
- **Fix**: JSON serialization now uses round-trip float formatting.
- **Test**: A persisted snapshot resumes with bit-equal state and output.

### Partial host render commit

- **Status**: Fixed.
- **Evidence**: A later internal chunk could fail after an earlier chunk advanced state.
- **Effect**: The host block was not one transaction.
- **Fix**: The host renderer checkpoints player and resampler state before all chunks.
- **Test**: An injected later-chunk failure preserves state and caller output.

### Untimestamped control collapse

- **Status**: Fixed in the canonical physical API.
- **Evidence**: The previous bridge stored one atomic control tuple per render quantum.
- **Effect**: Multiple reversals could collapse before the audio thread observed them.
- **Fix**: The canonical API accepts bounded sample-timestamped control events.
- **Evidence**: The top-level native consumer now uses the canonical event queue.
- **Remaining work**: Browser and nested application consumers must use the canonical event queue.

### Combined hand velocity limit

- **Status**: Fixed.
- **Evidence**: Hand velocity added position correction after it limited pointer velocity.
- **Effect**: A maximum-speed pointer and correction could request more than 20x speed.
- **Fix**: The mechanics solver limits their sum to the signed deck-rate limit.
- **Test**: Forward and reverse maximum targets remain inside the 20x limit.

### Pointer timing ambiguity

- **Status**: Reduced, not eliminated.
- **Evidence**: The canonical mapper unwraps angles and maps monotonic timestamps to internal frames.
- **Evidence**: Persistent late shifts preserve event spacing and reversal order.
- **Evidence**: The mapper does not smooth a reversal before physical friction acts.
- **Limit**: A long sampling gap can hide one or more complete pointer turns.
- **Limit**: Pressure calibration remains an estimated mapping until hardware measurements exist.
- **Test**: Tests cover branch cuts, late events, 128 reversals, pressure errors, and snapshot restore.

## 2026-08-01: Remaining Physical Divergences

### Macroscopic groove following and skips

- **Status**: Integrated with one lateral pickup state.
- **Confidence**: High.
- **Evidence**: The pickup solver owns the only lateral tip, body, mass, and force state.
- **Evidence**: Each sample moves the local origin with the intended spiral radius.
- **Evidence**: The transform changes local positions by the negative origin change.
- **Evidence**: The transform does not change deck-frame velocities.
- **Evidence**: A selected turn adds opposite lateral offsets to the two wall normals.
- **Evidence**: Anti-skate, skating, and bearing friction act once in the contact solve.
- **Evidence**: The bounded selector changes turns only inside the target groove aperture.
- **Evidence**: The player does not read programme walls while the stylus is on land.
- **Evidence**: A horizontal unilateral constraint supports the stylus on land.
- **Evidence**: Tests cover both skip directions, land travel, snapshots, and render rollback.
- **Assumption**: Ideal 45-degree walls put the land plane at half the groove top width.
- **Limit**: The model does not include transverse stylus shape or groove-edge deformation.
- **Limit**: Capture hysteresis uses estimated aperture margins.
- **Disproof test**: Force mistracking and verify measured loss, land travel, and adjacent-turn recapture.

### Groove-wall compliance

- **Status**: Missing.
- **Confidence**: High.
- **Evidence**: The active-set solver treats each groove wall as rigid.
- **Physical effect**: Vinyl deformation changes high-frequency response, force, and resonance.
- **Published evidence**: Bastiaans reports elastic and plastic wall penetration under a playback stylus.
- Source: <https://secure.aes.org/forum/pubs/journal/?elib=1082>
- **Published evidence**: White measured complex groove impedance against tracking force and groove speed.
- Source: <https://secure.aes.org/forum/pubs/journal/?elib=1480>
- **Published warning**: White states that classical elasticity assumptions do not represent stylus-groove contact sufficiently.
- **Published evidence**: Barlow and Garside measured load-dependent penetration for multiple indenter profiles.
- Source: <https://secure.aes.org/forum/pubs/journal/?elib=3264>
- **Effect**: Their reported deformation changes tracing distortion differently across the treble range.
- **Decision**: Do not use an unmeasured Hertz spring as the production compliance law.
- **Requirement**: Measure complex wall impedance across force, speed, frequency, temperature, and identified vinyl compound.
- **Requirement**: Measure static penetration and the elastic-to-plastic transition separately.
- **Model hypothesis**: Fit a passive positive-real wall impedance to the measured complex response.
- **Model hypothesis**: Use positive spring and dashpot elements so stored energy and loss remain explicit.
- **Registered topology**: Use a groove-coordinate generalized Kelvin creep field with positive-semidefinite reciprocal compliance matrices.
- **Registered port**: Use projected wall force and positive indentation as one conjugate material port.
- **Registered solve**: Condense endpoint compliance into the same normal complementarity step.
- **Registered energy**: Report elastic storage and nonnegative retardation loss with an exact trapezoidal identity.
- **Nonlinear extension**: Use a measured monotone elastic curve derived from a convex stored-energy potential.
- **Fit rejection**: Reject the model if relaxation subtraction makes the instantaneous elastic curve non-monotone.
- **Risk**: A linear impedance can miss force-dependent penetration and plastic flow.
- **State-location risk**: Viscoelastic memory belongs to the contacted groove material.
- A stylus-attached state moves old wall deformation into new groove material.
- Rapid scratching can revisit material before its deformation decays.
- **Requirement**: Bind recoverable material state to certified groove position when measurements show material memory is significant.
- **Alternative**: Calibrate a moving-contact impedance and state its speed and revisit-time limits.
- **Requirement**: Keep the immutable cut geometry separate from reversible deformation and irreversible damage.
- **Requirement**: Keep irreversible damage outside the elastic compliance state until repeated-pass measurements identify it.
- **Possible error**: A calibrated suspension can absorb part of the missing response over a narrow band.
- **Disproof test**: Match force and frequency sweeps across tracking force and groove velocity.
- **Validation case**: `PVC-007` in `PHYSICS_VALIDATION_CASES.md` contains the complete registered contract.

### Multiple-contact compliance reduction

- **Status**: The `N+1` prefix reduction is rejected as a general solution.
- **Confidence**: High for coupled material memory.
- Each isolated contact candidate has a rigid center-height threshold.
- A compliant wall can load candidates below the highest rigid threshold.
- With linear local compliance, contact force is zero below its penetration threshold.
- Above that threshold, contact force increases monotonically with penetration.
- **Historical hypothesis**: Active contacts can form prefixes ordered by rigid threshold height.
- **Historical benefit**: One wall then has only `N+1` active-count regimes for `N` candidates.
- **Contrary evidence**: Viscoelastic state and cross-compliance can produce non-prefix active sets.
- **Contrary evidence**: Independent local springs can count one deforming volume more than once.
- **Decision**: Do not use the prefix reduction without a theorem for the identified material operator.
- **Requirement**: Use a deterministic fixed-cap complementarity solver for the identified coupled operator.
- **Requirement**: Include contact-height gaps and local slopes in the certified tracer output.
- **Requirement**: Retain each candidate within the profile's maximum certified penetration below the rigid global height.
- **Reason**: A lower rigid candidate can load after a higher patch deforms.
- **Requirement**: Bind the maximum penetration and candidate capacity to the trace certificate.
- **Requirement**: Prove nonnegative force, nonpositive loss power, fixed work, and transactional failure.
- **Requirement**: Measure spatial transfer impedance before enabling calibrated multiple-patch compliance.
- **Disproof test**: Compare the reduced solver with a resolved finite-element or measured multi-contact case.

### Wear, damage, dust, and temperature

- **Status**: Missing.
- **Confidence**: High.
- **Evidence**: Groove data is immutable during playback.
- **Physical effect**: Repeated scratching can change friction, noise, and groove shape.
- **Disproof test**: Compare repeated-pass measurements on identified vinyl under controlled temperature and contamination.

### Stylus shape and alignment

- **Status**: Spherical tracing only.
- **Confidence**: High.
- **Evidence**: The geometry has one spherical tracing radius.
- **Physical effect**: Elliptical and line-contact tips have different tracing and pressure distributions.
- **Physical effect**: Azimuth, vertical tracking angle, and stylus rake angle change channel response.
- **Disproof test**: Match the same test record with measured tip profiles and alignment errors.

### Spherical envelope global maximum

- **Status**: The replacement passes both exact counterexamples.
- **Confidence**: High from exact fixtures and an outward-rounded interval oracle.
- **Historical evidence**: The former tracer scanned integer frames and refined one selected interval.
- **Evidence**: A Catmull-Rom segment can create more than one local envelope maximum.
- **Counterexample**: A production wall-velocity source produces the primary failure.
- **Counterexample**: The tested spatial step is 1.5 micrometers per source frame.
- **Counterexample**: The height miss is between 21.285598205846543 and 21.384231327363397 nanometers.
- **Counterexample**: The contact-position separation is between 4.078637504583612 and 4.106286430364061 micrometers.
- **Reference**: Outward interval arithmetic encloses every applicable cubic and spherical arc.
- **Reference**: The height enclosure is 0.09863312151685398 nanometers wide.
- **Reference**: The contact-position enclosure is 27.6489257812497 nanometers wide.
- **Reference**: Dense and reversed-input checks agree with the interval enclosures.
- **Effect**: The error can change wall displacement, slope, force, torque, and contact state.
- **Correction**: The replacement uses a bounded global envelope search.
- **Test**: Executable assertions call only the replacement tracer.
- **Correction**: Each admitted representation binds its trace certificate to its identity.
- **Validation case**: `PVC-001` in `PHYSICS_VALIDATION_CASES.md`.
- **Limit**: The source is valid production input, but physical cutter feasibility is not proved.
- **Limit**: The test oracle remains independent from the page-bound production certificate.
- **Result**: The extreme numeric-domain fixture now passes the certified tracer and outward reference bounds.
- **Limit**: Its kinematics remain outside a supported physical-record claim.
- **Result**: The rapid-scratch reference fixture now completes without a trace error.
- **Limit**: This passing fixture does not prove the callback deadline.
- **Result**: Asset admission rejects content that exceeds the fixed trace caps.
- **Result**: Contiguous, paged, and real-time paged sources require validated admission.
- **Conclusion**: The exact local-maximum defect and its current production boundary are corrected.
- **Limit**: The page producer remains trusted until an authoritative full-record catalog exists.

### Constant-segment Catmull-Rom arithmetic

- **Status**: Confirmed numerical defect and corrected production arithmetic.
- **Confidence**: High from exact coefficient bits and a permanent test.
- **Fixture**: Four equal points each have value `2.0205539476957236e-10` meters.
- **Former result**: Coefficient `a` was `-1.2924697071141057e-26` meters.
- **Former result**: Coefficient `b` was `6.462348535570529e-26` meters.
- **Required result**: A constant segment has bit-exact positive-zero derivative coefficients.
- **Effect**: The false derivative blocked a valid bit-exact C1 edge certificate.
- **Limit**: This coefficient size is not a material physical groove error.
- **Correction**: Construct the cubic from differences relative to `y1`.
- **Requirement**: Use the same coefficient arithmetic in tracing and admission.
- **Validation case**: `PVC-001` in `PHYSICS_VALIDATION_CASES.md`.

### Friction law

- **Status**: Separate constant Coulomb coefficients for groove walls and record land.
- **Confidence**: High.
- **Evidence**: Friction does not depend on speed, normal load, material state, or temperature.
- **Possible error**: One coefficient can fit a small operating region.
- **Disproof test**: Measure tangential force during forward, stop, and reverse sweeps at several tracking forces.

### Three-dimensional groove friction

- **Status**: Local surface-vector sliding is implemented.
- **Confidence**: High for force signs at one unique contact.
- **Definition**: Let `p` be wall displacement change divided by along-groove distance.
- **Definition**: Let `lambda` be the projected wall force.
- **Derivation**: The surface-normal force magnitude is `lambda * sqrt(1 + p^2)`.
- **Derivation**: Isotropic Coulomb friction has magnitude `mu * lambda * sqrt(1 + p^2)`.
- **Derivation**: Its along-groove component has magnitude `mu * lambda`.
- **Derivation**: Its signed wall-coordinate component is the along-groove component multiplied by `p`.
- **Former finding**: The solve used the complete friction magnitude as an along-groove force.
- **Former effect**: This increased record torque by `sqrt(1 + p^2)`.
- **Example**: A slope magnitude of `0.5` gives a torque factor of `1.118033988749895`.
- **Example**: The former possible torque error was approximately `11.8034` percent.
- **Correction**: The solve now uses projected force for the record-tangent component.
- **Correction**: It puts the signed wall-coordinate component inside the same momentum solve.
- **Correction**: Local friction power includes the record and wall-coordinate components.
- **Correction**: Forward and reverse sliding produce nonpositive local friction power.
- **Alternative**: The existing coefficient can represent an effective along-groove coefficient.
- **Rejected alternative**: Do not reinterpret the coefficient without measurements and a profile-schema change.
- **Local inward condition**: Require `mu * maximum_abs_slope < 1 - 1e-6`.
- **Reason**: This condition keeps each local wall-force component strictly inward.
- **Limit**: This scalar condition is not a coupled Painlevé well-posedness proof.
- **Reason**: The coupled response also depends on arm geometry, mass, damping, deck inertia, and both contacts.
- **Requirement**: Prove the active Delassus matrix is a P-matrix over the admitted profile domain.
- **Confirmed calculation**: The current admitted domain contains a negative one-contact principal minor.
- **Witness**: Use record inertia `1e-7 kg m^2` and stylus moving mass `0.01 kg`.
- **Witness**: Use friction `0.25`, wall slope `-0.125`, and radius `0.14605 m`.
- **Witness**: Use the default tonearm and a small valid positive generator coefficient.
- **Witness modes**: Deck and slipmat slide, the hand separates, and the pickup bearing sticks.
- **Scalar result**: The current test accepts `mu * abs(p) = 0.03125`.
- **Calculated result**: The deck contribution is `-0.007866686227009534`.
- **Calculated result**: The pickup contribution is `0.000536859604648429`.
- **Confirmed result**: The one-wall minor is `-0.007329826622361105`.
- **Consequence**: The current scalar condition does not prove existence or uniqueness.
- **Production parity**: Playback and the witness now use one canonical midpoint `H` and `G` builder.
- **Order protection**: Typed vectors separate equation-row order from velocity-column order.
- **Bit protection**: Production writers retain the former signed-zero coefficient layout.
- **Permanent test**: `admitted_midpoint_configuration_has_a_negative_normal_minor` reproduces the negative minor.
- **Family correction**: The former 48-family count covers groove-wall sliding only.
- **Family basis**: There are 24 nominal mechanical mobility classes and two sliding signs.
- **Additional surfaces**: Record-land sliding adds another 48 labeled families.
- **Additional modes**: Groove and land sticking add 24 labeled families for each surface.
- **Origin regimes**: Interior spiral motion and a held program boundary use different groove rows.
- **Separated mode**: Zero-friction separation can duplicate a sliding operator with a different mode label.
- **Labeled catalog**: Groove contact has 192 families across four stylus labels and two origin laws.
- **Labeled catalog**: Record-land contact adds 96 families across four stylus labels.
- **Mask catalog**: Principal-minor coverage reduces the structurally distinct mode-mask catalog to 768 entries.
- **Runtime count**: One hand-active groove sample evaluates at most 1,053 current branches.
- **Bound correction**: The registered 1,296 limit is safe, but it is not the exact fallback count.
- **Implementation requirement**: Generate the versioned family catalog from production logic.
- **Implementation result**: The crate now generates all 24 mechanical classes and 288 contacting labels.
- **Version rule**: The operator and family-set versions are explicit inputs to later certificates.
- **Assembly result**: The point builder reuses the production deck, pickup, constraint, `H`, and `G` writers.
- **Parity result**: A permanent test compares every active KKT coefficient with production assembly bits.
- **Coverage result**: All 288 labels build finite point-valued mobilities with bounded residuals.
- **Diagnostic result**: Each response reports equality rank, dependent constraints, scaled pivots, and backward error.
- **Dependency limit**: Dependent equalities still require a runtime right-hand-side compatibility check.
- **Coordinate protection**: Typed accessors keep dynamic equation rows separate from dynamic velocity columns.
- **Permanent test**: `typed_joint_coordinates_keep_equation_and_velocity_orders_distinct` uses distinct coordinate values.
- **Witness result**: The shared builder keeps `W_00` equal to `-0.007329826622361105` for the known witness.
- **Behavior result**: This checkpoint does not change the runtime contact result.
- **Proof limit**: These point values are not outward interval bounds.
- **Admission limit**: The player does not use this builder as a source admission gate.
- **Requirement**: Certify every permitted contact family for each profile and source.
- **Requirement**: Reject an unproved operator before playback changes state.
- **Proof decision**: Verify 24 point-valued base mobility systems before applying interval contact operators.
- **Reason**: The mechanical left-hand sides do not depend on groove radius or wall slope.
- **Proof decision**: Apply stylus sticking with a verified nonsymmetric rank-one Schur update.
- **Rejected design**: Do not interval-solve one parameterized 13-by-13 system for every domain box.
- **Reason**: That design repeats work and introduces unnecessary interval dependency.
- **Coordinate rule**: Store mobility with typed velocity rows and equation columns.
- **Equation order**: Use platter, record, tip-x, body-x, tip-z, and body-z.
- **Velocity order**: Use platter, record, tip-x, tip-z, body-x, and body-z.
- **Permanent requirement**: A six-value sentinel must detect any coordinate permutation.
- **Cartridge rule**: Extract state-independent reciprocal damping from the state-dependent cartridge bias.
- **Geometry rule**: Calculate the skating factor with algebraic operations and square root.
- **Reason**: Portable directed rounding is not available for the current trigonometric functions.
- **Implementation status**: Production now uses the algebraic skating factor.
- **Boundary rule**: The cosine must remain strictly between negative one and positive one.
- **Boundary rule**: The calculated squared sine must remain positive and finite.
- **Result**: Exact geometric reach boundaries now reject as unreachable.
- **Reference result**: The new program-radius endpoint values are within one ULP of 100-digit calculations.
- **Change size**: The outer endpoint changed by one ULP. The inner endpoint changed by four ULP.
- **Interpretation**: The bit changes replace less accurate trigonometric reconstruction results.
- **Proof catalog**: Add 48 solve-only systems to the 288 contacting labels.
- **Proof total**: The initial catalog therefore contains 336 records before safe equivalence aggregation.
- **Interval method**: Use outward arithmetic and adaptive radius-and-slope subdivision.
- **Implementation status**: Finite outward interval arithmetic is implemented as a non-gating core.
- **Arithmetic result**: Addition, subtraction, multiplication, division, square, and square root have outward bounds.
- **Zero result**: Algebraically exact zero and identity operations keep exact finite bounds.
- **Overflow rule**: An operation rejects when finite outward widening cannot contain its result.
- **Rounding rule**: The implementation does not change the process rounding mode.
- **Exact tests**: Rational oracles cover exhaustive dyadic pairs and 4,096 deterministic random cases.
- **Edge tests**: The tests cover subnormals, cancellation, sign quadrants, overflow, and maximum finite identities.
- **Portability result**: Native workspace and `wasm32-unknown-unknown` checks pass.
- **Work rule**: Reject the profile when the fixed depth or box limit cannot resolve a proof.
- **Oracle rule**: Use exact dyadic rational arithmetic in tests for interval operations and representative solves.
- **Physical gate**: Require strict positive diagonal and determinant lower bounds.
- **Numerical gate**: Also require scale-aware conditioning margins for the production floating-point solver.
- **Initial robustness floor**: Use `6.4e-9`, derived from 64 times the current backward-error limit.
- **Config checkpoint**: The versioned config identity binds all 84 validated manifest leaves.
- **Config encoding**: It hashes stable paths, explicit value tags, exact float bits, and length-delimited values.
- **Rejected encoding**: Do not hash serialized JSON text.
- **Current status**: Config identity tests pass. The identity does not gate playback yet.
- **Source-domain risk**: Resident realtime pages do not prove a whole-record slope bound.
- **Required response**: Reject those sources or publish source metadata and proof atomically.
- **Theorem scope**: A P-matrix proves uniqueness only for one fixed linear complementarity problem.
- **Hybrid limit**: It does not prove that two different discrete modes cannot both pass.
- **Exact overlap**: At zero stylus friction, loaded positive sliding and separation can use the same system.
- **Deck overlap**: Static-to-kinetic force gaps can also make stick and slide branches both feasible.
- **Exact deck witness**: A `0.00020 N m` motor torque permits both default bearing modes from rest.
- **Sliding witness result**: The alternate platter speed is `3.88313554466e-9 rad/s`.
- **Mask overlap**: Contact tolerances can accept both active and inactive masks near zero gap.
- **Clamp overlap**: A small negative multiplier can pass the force tolerance and then clamp to zero.
- **Theorem mismatch**: These tolerance bands are outside the exact LCP covered by the P-matrix theorem.
- **Selection result**: The current branch order gives a deterministic result, not a unique physical mode.
- **Claim limit**: Do not describe fixed-mode certification as complete hybrid uniqueness.
- **Reference**: Murty proves the fixed-LCP P-matrix equivalence at <https://doi.org/10.1137/0120041>.
- **Reference**: Frictional rigid contact can remain indeterminate; see <https://arxiv.org/abs/1601.03545>.
- **Former defect**: The constraint selector projected stylus sticking onto record velocity only.
- **Effect**: It could discard the independent lateral body term from the full sticking equality.
- **Correction**: The selector now retains that term when the pickup bearing permits lateral body motion.
- **Permanent test**: `hand_and_stylus_sticking_keep_the_independent_body_constraint` covers the rank decision.
- **Permanent test**: `joint_branch_enforces_independent_and_dependent_sticking_equalities` covers complete branch solves.
- **Direct-path gap**: The public standalone pickup solver still has a separate contact-operator assembly.
- **Requirement**: Give that path its own canonical operator and bounded admission proof.
- **Resolution option**: Add a measured transition law with mutually exclusive stick, slide, and release guards.
- **Resolution option**: Prove one global mixed complementarity problem is strongly monotone.
- **Resolution option**: Compare all valid candidates and reject materially different results.
- **Common requirement**: Use outward-certified complementarity tests and a rule for redundant static multipliers.
- **Finding**: The general trace slope cap is `16`.
- **Finding**: The default groove friction coefficient is `0.25`.
- **Consequence**: Those two general limits do not prove the friction condition.
- **Requirement**: Bind the selected coefficient to the certificate's actual maximum absolute slope.
- **Reference result**: The rapid fixture maximum slope is approximately `0.491673`.
- **Reference result**: Its product with `0.25` is approximately `0.12291825`.
- **Reference result**: `PVC-004` has a maximum slope of approximately `0.440791`.
- **Reference result**: Its product with `0.25` is approximately `0.11019775`.
- **Limit**: The present pickup has no along-groove coordinate or tangential material state.
- **Correction**: The solve maps along-groove force into lateral arm-body force through `skating_factor`.
- **Correction**: Relative velocity includes the reciprocal lateral arm-body velocity.
- **Correction**: Friction power includes record, body, and cross-plane ports.
- **Result**: Snapshot restore rejects a Coulomb mode, sign, or magnitude that the configured law cannot produce.
- **Finding**: The test-qualified same-wall reduction uses one mean slope in the momentum solve.
- **Finding**: Its detailed power telemetry uses each contact's squared slope.
- **Consequence**: Those power values disagree when same-wall slopes differ.
- **Current protection**: Production rejects same-wall sets without a physical qualification.
- **Requirement**: Keep that rejection until each accepted contact has one represented port.
- **Result**: Exact-zero sloped sticking returns `UnsupportedGrooveWallSticking`.
- **Reason**: Two wall tractions have one global sticking constraint.
- **Reason**: A single longitudinal coordinate does not identify their distribution.
- **Rejected hypothesis**: Divide static force in proportion to normal force.
- **Reason**: That rule has no measured tangential constitutive law.
- **Requirement**: Add measured per-contact tangential compliance and relaxation state.
- **Requirement**: Keep this state at stable groove coordinates across reversals.
- **Requirement**: Add an along-groove pickup coordinate and its power-conjugate force.
- **Claim limit**: Sliding geometry is corrected, but rapid reversal fidelity is not complete.
- **Working hypothesis**: Use one passive Jenkins tangential element for each unique wall contact.
- **Working hypothesis**: Enumerate elastic and two sliding branches independently for both walls.
- **Former loose cap**: The Cartesian estimate was 2,916 candidate branches.
- **Corrected cap**: Mask-aware two-wall Jenkins enumeration gives 1,296 candidates with active hand modes.
- **Reason**: The four wall masks have `1 + 3 + 3 + 9 = 16` material-mode combinations.
- **Open count**: Recalculate the cap after the final compliance equations and production enumeration exist.
- **Identity requirement**: Key material state by source, generation, wall, and canonical groove cell.
- **Rejected identity**: Do not key material state by contact-array order or floating-point midpoint bits.
- **Persistence limit**: A finite state bank cannot retain energetic cells for unlimited playback.
- **Requirement**: Measure recovery and register a residual-energy retirement rule.
- **Requirement**: Account retired recoverable energy as loss.
- **Reference**: `PVC-007` records equations, state rules, measurements, and disproof tests.

### Certified contact identity audit

- **Status**: Certified interval propagation and a fail-closed cell resolver are implemented.
- **Finding**: Class A now returns its final outward root interval.
- **Finding**: Class B and exhaustive tracing return each selected or merged contender interval.
- **Coordinate form**: One exact `u64` origin and two local `f64` bounds define the absolute interval.
- **Reason**: Converting a large source origin to `f64` can remove the certified interval width.
- **Production bound**: The player still limits absolute floating-point addressing to `2^53` frames.
- **Test limit**: The large-origin trace test uses an exactly representable center above `2^53`.
- **Open limit**: General odd origins above `2^53` need a split addressing API.
- **Propagation**: Paged, real-time, player, transform, input, and telemetry paths preserve the interval.
- **Trust rule**: Deserialization removes the private live-trace seal.
- **Result**: Forged or nonordered bounds cannot authorize a material key.
- **Authority limit**: The provisional resolver is crate-private and has no production caller.
- **Lineage gap**: The resolver does not bind one interval to its supplied source, generation, or wall.
- **Requirement**: A production capability must bind trace admission, lineage, and material instance.
- **Material key**: Use source identity, generation, wall, and canonical cell index.
- **Exclusion**: Do not include page, direction, representation kind, contact order, or midpoint bits.
- **Isolation rule**: Both closed interval bounds must select one versioned canonical cell.
- **Failure**: Return `TangentialContactIdentityNotIsolated` before state changes when the interval straddles a cell boundary.
- **Endpoint rule**: The final coordinate maps to the last real spline cell.
- **Endpoint blocker**: Actual outward edge intervals extend beyond the source and still reject.
- **Requirement**: Future edge clipping must use trace-domain provenance.
- **Seam result**: Paged and whole-record bounds can use different valid decompositions.
- **Seam result**: Their physical trace and resolved material cell agree in both directions at plus or minus 20x.
- **Rejected test rule**: Do not require raw interval-bit equality across different coordinate origins.
- **First implementation grid**: Base spline cells support deterministic plumbing tests only.
- **Claim limit**: Base spline cells do not prove a physical contact footprint.
- **Liveness counterexample**: A flat Class A trace at integer frame 64 straddles cells 63 and 64.
- **Result**: The resolver returns `TangentialContactIdentityNotIsolated` for that valid trace.
- **General result**: Every finite hard cell partition has a non-isolating boundary neighborhood.
- **Consequence**: The hard cell resolver cannot support uninterrupted scratching or activate Jenkins state.
- **Requirement**: Use certified continuous weights and the transpose force map for production material state.
- **Continuous-map hypothesis**: Use two normalized linear-hat amplitudes for one selected coordinate.
- **Boundary capacity**: One narrow certified interval can require three possible coefficient keys.
- **Rejected map**: Raw linear amplitudes change self stiffness by a factor of two across one cell.
- **Power rule**: Use the same amplitude bits for state scatter and force gather.
- **Yield rule**: Use one Jenkins yield surface per wall, not one slider per coefficient.
- **Identity gap**: Cache generation does not identify one physical record instance.
- **Requirement**: Add `GrooveMaterialInstanceId` before material-state activation.
- **Rapid-sweep blocker**: A 20-frame endpoint move can cross at least 23 possible coefficient keys.
- **Stronger blocker**: The certificate does not bound contact-root travel between physics samples.
- **Requirement**: Certify root travel and use bounded event substeps or a proved swept return map.
- **State placement**: Keep the material bank outside the copied pickup candidate state.
- **Commit rule**: Commit at most two material updates after the complete coupled candidate passes.
- **Capacity rule**: Prefer an exact key, then an empty slot, then a measured recovered slot.
- **Rejected policy**: Do not evict energetic state by proximity, direction, page, or recency.
- **Snapshot result**: Contact, player, and renderer snapshot versions are now 5, 10, and 4.
- **Rollback requirement**: A persistent bank needs a bounded block undo journal before activation.
- **Trust limit**: The key lineage still depends on the page producer until a full-record root exists.

### Non-smooth contact normal

- **Status**: A scalar shortcut was rejected before the trace checkpoint.
- **Confidence**: High for the representational gap.
- **Finding**: A clamp junction can have one contact position and more than one valid normal direction.
- **Finding**: Its one-sided groove slopes define a normal cone.
- **Rejected hypothesis**: Use the spherical-envelope tangent inside the cone as one effective groove slope.
- **Reason**: Geometry does not make that tangent the unique downstream force direction.
- **Effect**: A selected scalar can change modulation force, record torque, skating force, and friction.
- **Requirement**: Return `GrooveSlopeBoundNotMet` when one scalar slope cannot represent the contact.
- **Requirement**: Keep smooth duplicate endpoint roots as one contact only when their slope enclosure passes its registered bound.
- **Future correction**: Carry the complete normal cone into the contact active set.
- **Future correction**: Solve nonnegative one-sided contact multipliers with the other same-sample modes.
- **Permanent fixture**: One clamp side has zero slope, and the adjacent side has slope `-0.5`.
- **Expected result**: The scalar tracer rejects the fixture with `GrooveSlopeBoundNotMet`.

### Tonearm motion

- **Status**: Local two-axis equivalent-mass model.
- **Confidence**: High.
- **Evidence**: Geometry computes tracking error and skating-force conversion.
- **Evidence**: The contact solve includes horizontal bearing stick, slip, and viscous friction.
- **Evidence**: The dynamic state does not yet integrate the full pivot angle.
- **Physical effect**: Full pivot geometry can change radial travel and skip recovery.
- **Disproof test**: Compare pivot-angle and stylus-radius traces during forced mistracking.

### Numerical integration

- **Status**: Backward-Euler pickup solve.
- **Confidence**: High.
- **Evidence**: Backward Euler adds numerical damping.
- **Possible error**: Physical damping can dominate the error at 192 kHz.
- **Disproof test**: Compare frequency and energy response against a converged smaller-step reference.

### Electromechanical coupling delay

- **Status**: Fixed.
- **Confidence**: High.
- **Evidence**: The cartridge prepares an affine trapezoidal current-step relation.
- **Evidence**: The contact active-set solve includes the current-step reciprocal force.
- **Evidence**: Impulse and reversal tests show no algorithmic one-sample force delay.
- **Limit**: Contact uses backward Euler while the circuit uses trapezoidal integration.
- **Limit**: The solve does not claim exact discrete energy conservation.
- **Disproof test**: Compare energy and phase against a converged monolithic integrator.

### Cartridge magnetic and loading detail

- **Status**: Linear seed model only.
- **Confidence**: High from code inspection.
- **Evidence**: The model uses constant coil resistance, constant inductance, and one ideal parallel load.
- **Evidence**: The channel matrix uses constant real balance and crosstalk values.
- **Evidence**: Holman reports frequency-dependent resistive loss from eddy currents and hysteresis in magnetic cartridge material.
- Source: <https://www.aes.org/e-lib/download.cfm?ID=2623>
- **Missing**: The model has no frequency-dependent magnetic loss or measured complex impedance table.
- **Missing**: The model has no coil mutual inductance or frequency-dependent crosstalk phase.
- **Missing**: The model has no magnetic saturation, level dependence, or coil-temperature state.
- **Missing**: The load does not represent a measured cable and preamplifier input network.
- **Possible error**: The ideal RLC model can match a specified cartridge over a limited level and frequency range.
- **Disproof test**: Measure complex impedance and output at several levels and temperatures.
- **Disproof test**: Measure both channel transfer matrices across frequency, load resistance, and load capacitance.
- **Conclusion**: The new cartridge is materially stronger than the previous curvature heuristic.
- **Conclusion**: The current evidence does not support a complete or calibrated cartridge claim.

### Deck and record structural motion

- **Status**: Lumped rigid-body seed model.
- **Confidence**: High from code inspection.
- **Evidence**: The platter and record each use one angle and one angular velocity.
- **Missing**: The model has no spindle clearance, hole offset, eccentricity, or record warp.
- **Missing**: The model has no platter, mat, or record flexural modes.
- **Missing**: The motor has no torque ripple, commutation, sensor quantization, or pitch-control delay.
- **Missing**: Hand contact uses one radius, one force, and one target velocity.
- **Possible error**: Lumped states can match the dominant response of one identified setup.
- **Disproof test**: Measure radial and vertical motion for several records through complete revolutions.
- **Disproof test**: Measure motor torque ripple and servo response with a high-rate angular encoder.
- **Disproof test**: Measure pressure distribution and record deformation during representative scratches.

### Phono stage

- **Status**: Superseded by the stateful phono-stage checkpoint below.
- **Confidence**: High.
- **Previous evidence**: The stage only applied linear gain and reported headroom flags.
- **New evidence**: The bounded stage now models noise, slew, overload, and recovery.
- **Disproof test**: Match measured gain, phase, noise, overload, and recovery sweeps.

### Additional primary evidence

- Shiga models elastic stylus contact with Hertz equations and pickup impedance.
- Source: <https://doi.org/10.20697/jasj.18.1_1>
- Bastiaans reports elastic penetration, plastic flow, translation loss, resonance, and scanning loss.
- Source: <https://secure.aes.org/forum/pubs/journal/?elib=1082>
- Barlow and Garside report measured vinyl deformation and treble distortion changes.
- Source: <https://secure.aes.org/forum/pubs/journal/?elib=3264>
- Holman reports complex cartridge loading and cartridge-inductance interaction in tested preamplifiers.
- Source: <https://www.aes.org/e-lib/download.cfm?ID=2623>
- Kilmanas and Rabinow report tonearm-geometry effects on frequency-modulation distortion.
- Source: <https://secure.aes.org/forum/pubs/journal/?elib=3825>
- **Conclusion**: The rigid-wall and static-geometry approximations omit documented playback mechanisms.
- **Uncertainty**: Abstracts do not supply enough coefficients for a calibrated seed.

## 2026-08-01: Source Ownership and Consumer Risk

### Canonical repository

- **Claim**: Top-level `record-player` must be the only physics implementation.
- **Status**: Decision accepted, migration incomplete.
- **Evidence**: Top-level `vin.yl.native` depends on the top-level crate through a local path.
- **Evidence**: Top-level `vin.yl.native` removed its vendored player and legacy bridge.
- **Evidence**: The nested `vin.yl.app` native submodule still uses its vendored legacy engine.
- **Evidence**: `infidelity/Native/PressCore` still uses its vendored legacy engine.
- **Evidence**: `bitneedle-native` uses the top-level crate but still wraps the legacy renderer.
- **Evidence**: Browser routes still use legacy Rust or independent JavaScript physics.
- **Risk**: Equal copied bytes do not prevent later divergence.
- **Test**: Build every consumer from one crate revision and compare known input vectors.

### Inspected Rust dependency inventory

- `record-player` is the canonical engine crate.
- `record-player/crates/record-player-capi` exposes its native C ABI.
- `vin.yl.native` uses the canonical path dependency.
- `bitneedle-native` uses the canonical path dependency but calls the old API.
- `vin.yl.app/Native/vin.yl.native` uses a vendored copy.
- `infidelity/Native/PressCore` uses a vendored copy.
- Build products and archived release worktrees are not source owners.

### Portable dependency source

- **Status**: Unresolved at this checkpoint.
- **Evidence**: The top-level repository has no configured Git remote.
- **Evidence**: The expected `wavey-ai/record-player` repository did not exist during the check.
- **Effect**: A local path can unify this workspace but cannot support independent clones.
- **Possible resolution**: Publish the authoritative repository and pin consumers to one revision.

## 2026-08-01: Claim Review

### Claim: This is the most accurate record-player simulation ever made

- **Status**: Not established.
- **Confidence**: High.
- **Evidence**: No comparative measurement corpus or independent benchmark is present.
- **Evidence**: Important parameters remain estimates.
- **Evidence**: Several physical effects remain absent or incomplete.
- **Possible error**: Competing systems can have fewer implemented states or weaker real-time behavior.
- **Disproof test**: Publish repeatable comparisons against hardware and relevant competing systems.

### Claim: The architecture can support a leading physical simulation

- **Status**: Plausible working conclusion.
- **Confidence**: Medium.
- **Evidence**: The signal path now uses explicit physical units and coupled state.
- **Evidence**: Real-time control, paging, rollback, and resampling have deterministic tests.
- **Uncertainty**: Hardware identification and complete consumer integration are unfinished.
- **Disproof test**: Fail the required measurement suite or minimum-device stress tests.

### Calibration gate limitation

- The gate checks structure, declared evidence, hashes, and required result coverage.
- It does not prove artifact authenticity.
- It does not prove that an acceptance criterion is scientifically sufficient.
- It does not replace independent replication or blind comparison.
- Render-block and control-timeline capacities have explicit construction limits.

### Calibration-gate test results

- Nineteen profile tests pass on 2026-08-01.
- One measured parameter row cannot authorize a calibrated claim.
- One measured validation row cannot authorize a calibrated claim.
- Published or calculated evidence cannot replace a required direct measurement.
- A self-consistent 96 kHz profile cannot enter the canonical physical path.

### Important limits

- The gate checks structure and traceability.
- The gate does not prove that an artifact is authentic.
- A SHA-256 digest checks artifact integrity, not artifact provenance.
- The gate does not evaluate the scientific quality of a procedure.
- The seed declarations intentionally contain no invented numeric limits.
- A calibrated profile must add numeric limits from an approved measurement plan.
- The gate checks metric names, units, ranges, maximum uncertainty, and reported verdicts.
- The 16,384-frame and 16,384-event limits are engineering safety choices.
- A valid claim still needs specimen identifiers, traceable equipment, uncertainty budgets, and independent review.
- External verification must compare each stored artifact with its declared result.

## 2026-08-01: Implemented Host Output Path

### Implemented behavior

- The physical player runs at the fixed 192 kHz internal rate.
- The output converter supports 44.1, 48, 88.2, 96, 176.4, and 192 kHz.
- An exact rational clock calculates each input and output frame count.
- A Kaiser-windowed polyphase filter limits downsampling aliases.
- The filter uses a fixed 641-frame maximum history.
- The host renderer allocates its complete internal scratch buffer during construction.
- Large host blocks use bounded internal chunks.
- Arbitrary host partitions preserve the physical clock and output sequence.
- Renderer snapshots contain the player state and output-converter state.
- Each snapshot contains the loaded groove content identity.
- Restore rejects a different groove, profile, output rate, frame clock, or snapshot version.
- JSON parsing uses exact floating-point round trips for snapshot continuity.

### Test results

- Eleven output-converter tests pass on 2026-08-01.
- Nine host-renderer tests pass on 2026-08-01.
- One internal second produces the exact declared host-frame count at each supported rate.
- Tested arbitrary partitions produce bit-identical nonzero physical output.
- Tested snapshot restore produces bit-identical continuation.
- A host block larger than the scratch capacity does not change the scratch allocation.
- Invalid stereo shape, rate, clock, version, and groove identity are rejected.

### Important limits

- The 100 dB Kaiser value is a filter-design target.
- The automated stopband threshold is weaker than 100 dB.
- These tests use generated signals, not calibrated converter measurements.
- The tests do not measure real-time CPU use on supported devices.
- The snapshots contain groove identity but do not contain groove samples.
- Restore requires the matching groove asset before it restores the player.
- The 1e30 sample limit is a numeric safety bound, not a physical voltage claim.
- An oversized host block fails before state or caller output changes.
- A later chunk error restores the complete host block transaction.
- Device callback tests must verify timing, underruns, and long-session clock stability.

## 2026-08-01: Implemented Stateful Phono Stage

This checkpoint records implemented behavior. It does not show calibration against a phono preamplifier.

### Implemented behavior

- Cartridge input noise enters before playback RIAA equalization.
- A symmetric input limit operates before playback RIAA equalization.
- The overload envelope has separate attack and recovery times.
- The overload envelope reduces gain by a configured maximum amount.
- A symmetric output limit represents the positive and negative output rails.
- A configured slew rate limits each output change.
- A fixed-size generator supplies deterministic two-channel noise.
- The generator does not allocate memory during processing.
- The stage owns both playback RIAA filter states.
- The snapshot stores RIAA, overload, slew, output, noise, clock, and telemetry state.
- Restore rejects changed settings and invalid state before it changes the stage.

### Estimated seed values

- The input-overload attack time is 50 microseconds.
- The overload recovery time is 20 milliseconds.
- The maximum overload gain reduction is 24 dB.
- The output slew rate is 500,000 V/s.
- The input-referred noise is 500 nV RMS per internal sample.
- The deterministic noise-sequence seed is `0x56_49_4e_59_4c`.
- The manifest marks all six new seed values as `Estimated`.
- No measurement source supports these seed values.

### Test results

- Seven focused phono-stage tests pass on 2026-08-01.
- The small-signal result matches the existing playback RIAA and gain chain within floating-point precision.
- Tested block partitions produce bit-identical output and state.
- A JSON snapshot produces bit-identical noise and overload continuation.
- Tested input overload causes causal gain reduction and later recovery.
- Every tested output change satisfies the configured slew limit.
- Invalid input, output size, configuration, and snapshot state leave the stage unchanged.

### Important uncertainty

- The noise generator gives bounded approximate Gaussian white noise.
- The noise value is a discrete-sample RMS value, not a measured noise density.
- Real noise can include hum, interference, flicker noise, and frequency-dependent device noise.
- The input limit is one symmetric hard limit before the RIAA network.
- A physical preamplifier can distribute overload across several equalization and amplifier stages.
- The output limit uses equal positive and negative rails.
- The overload state uses one envelope value for each channel.
- The model omits supply sag, thermal drift, component tolerances, and topology-specific distortion.
- Unit tests show deterministic numerical behavior only.
- Hardware sweeps must identify every phono parameter before a calibrated claim is valid.

## 2026-08-01: Native C Boundary

This checkpoint records the native boundary and its Swift integration. Device callback tests remain necessary.

### Implemented behavior

- The C ABI crate has `record-player` as its only runtime crate dependency.
- The build produces `librecord_player_capi.a` and a public C header.
- One opaque handle owns one bounded control producer and one render consumer.
- The endpoints communicate through the core SPSC mailbox.
- Atomic lane guards reject concurrent access to the same endpoint.
- Producer and render access can occur at the same time.
- Lifecycle access excludes producer and render access.
- A full mailbox rejects the incoming control and keeps all accepted controls.
- The producer and renderer can operate at the same time.
- Groove load, unload, seek, reset, and destruction require exclusive lifecycle access.
- Each control contains one complete `PlayerControl` value and one exact internal frame.
- The render report identifies late, invalid, duplicate, and nonmonotonic controls.
- An exact rational conversion maps host-frame offsets to physical-frame boundaries.
- The render and control paths do not allocate after successful construction.
- The ABI exports physical deck, radial, pickup, cartridge, groove, phono, and overload telemetry.
- ABI version 3 exports land force, contact surface, bearing force, and actual groove guide force.
- ABI version 3 does not let a player host supply stylus reaction torque.
- The profile supplies the complete record-cut configuration to groove loading.
- Swift owns the Rust player and gesture handles.
- Swift contains data conversion and ownership code, but it contains no scratch physics.

### Test results

- Fifteen ABI tests pass on 2026-08-01.
- A two-thread test transfers 1,000 controls while the renderer advances.
- A capacity-one test confirms rejection without overwrite.
- Adversarial tests verify producer, renderer, and lifecycle `BUSY` results.
- C11 and C++ compilers accept the public header.
- A native C program links the release static library and completes its lifecycle test.
- Twenty-nine native Rust tests pass, with one fixture test ignored.
- Seven Swift tests pass.
- Native tests cover concurrent rendering and 128 sample-timed reversals.

### Important limits

- The concurrent test does not prove correct use by every Swift caller.
- Thread Sanitizer and device callback tests remain necessary.
- The ABI returns `BUSY` before a conflicting lane can access mutable state.
- Full telemetry access belongs to the render thread.
- Clock information uses atomic snapshots and can become old before submission.
- The ingress moves a late event to the first physical frame that is not rendered.
- A consumer must inspect every ingress report for asynchronous control rejection.
- The ABI selects the core seed profile during construction.
- That seed profile does not permit a calibrated hardware claim.
- The current groove-loading function constructs a contiguous in-memory asset.
- The Swift package still needs a versioned XCFramework for remote distribution.
- The Swift host still needs device callback and Thread Sanitizer tests.

## 2026-08-01: Paged Groove Integrity

This checkpoint records data-integrity and seam-safety behavior. It does not authenticate an asset publisher.

### Implemented behavior

- Canonical metadata has a SHA-256 identity bound to its source identity and physical fields.
- Each page identity covers its asset identity, ranges, samples, and spatial pyramid.
- Cache publication rejects a changed page before it replaces a valid cache.
- Stylus support calculations include search radius and cubic interpolation support.
- Paged tracing rejects a declared halo that cannot support the selected stylus.
- The render trace performs no allocation or synchronization.
- Prefetch plans include explicit adjacent-turn recapture candidates.
- A stale generation or missing page has a typed, copyable error.

### Test results

- Thirty paged-groove tests pass on 2026-08-01.
- Eleven stylus tests pass on 2026-08-01.
- The complete library suite passes 353 tests, with one manual benchmark ignored.
- Tests cover corrupt pages, changed metadata, seam parity, reverse travel, and 20x motion.

### Important limits

- SHA-256 checks integrity, not publisher authenticity.
- A trusted manifest or signature must identify hostile remote assets.
- Prefetch planning returns ranges but does not perform input or output.
- Resident-byte limits exclude allocator overhead, decoder buffers, and retired cache snapshots.

## 2026-08-01: Consumer Activation Audit

### Claim: The stronger physical cartridge is active in shipped consumers

- **Status**: False for the inspected browser and nested application paths.
- **Confidence**: High for the inspected source.
- **Evidence**: `vin.yl.player` constructs the legacy Rust `ScratchAcousticDsp`.
- **Evidence**: `vin.yl.app` links its nested vendored native engine.
- **Evidence**: The top-level native checkout uses the stronger canonical cartridge.
- **Scope**: The dead `yl.vin` route is not an active consumer or release blocker.
- **Possible error**: Deployment can select assets that differ from the inspected source.
- **Disproof test**: Capture loaded modules and identify the renderer in each production route.

### Consequence

- The main problem is activation, not evidence of deleted cartridge code.
- Browser and nested application migration must replace their actual renderers.
- Dependency changes alone cannot activate the physical cartridge.

## 2026-08-01: Browser Groove Paging Design

This section records a design hypothesis. Tests can reject this design.

### Stable cut identity

- **Decision**: Monolithic and streaming cuts must produce the same content identity.
- **Reason**: Processing partitions do not change physical groove content.
- **Implementation**: Identity version 3 uses separate streaming hashes for both base displacement channels.
- **Implementation**: The final identity combines fixed settings, the cut report, and both channel hashes.
- **Implementation**: The identity excludes the spatial pyramid because the pyramid is validated derived data.
- **Possible error**: Excluding derived data can hide a faulty pyramid unless page validation remains mandatory.
- **Disproof test**: Corrupt one pyramid sample and confirm that page publication fails.

### Progressive playback tension

- **Observation**: The current browser can start after a partial PCM decode.
- **Observation**: A final cut report requires the complete source programme.
- **Observation**: Current paged metadata includes that final report.
- **Effect**: The first physical implementation can require a complete cut before playback starts.
- **Risk**: This requirement can increase startup time for long records or slow devices.
- **Possible alternative**: Separate stable render metadata from the final audit report.
- **Possible error**: Provisional metadata can weaken identity checks or invalidate resident pages.
- **Disproof test**: Measure startup time, peak memory, and identity continuity on minimum supported devices.

### Real-time page safety

- **Hypothesis**: A bounded resident window can prevent misses during continuous motion at the 20x limit.
- **Requirement**: Prefetch must cover both directions before a reversal occurs.
- **Requirement**: Prefetch must include each bounded adjacent-turn recapture candidate.
- **Requirement**: A seek must wait for its required pages before render state changes.
- **Requirement**: A page miss must restore the complete player and output-converter state.
- **Possible error**: Worker delay or browser throttling can exceed the resident safety horizon.
- **Disproof test**: Apply repeated 20x reversals while delaying page delivery beyond normal scheduling jitter.
- **Disproof test**: Confirm that successful renders allocate no memory and acquire no lock.

### AudioWorklet page publication

- **Observation**: The Web Audio specification runs rendering-side `MessagePort` work on the rendering thread.
- Source: <https://webaudio.github.io/web-audio-api/#AudioWorkletGlobalScope-section>
- **Effect**: A page message does not provide an off-render cache insertion thread.
- **Requirement**: The worklet must not deserialize, hash, or build a large page during active rendering.
- **Requirement**: An off-thread cutter must finish page data before publication.
- **Requirement**: The render thread must accept data through bounded work or a preallocated transfer protocol.
- **Possible error**: A browser can schedule message work between callbacks with sufficient time on some devices.
- **Disproof test**: Measure every publication interval on the minimum supported devices during 20x reversals.

### Durable browser page backing

- **Finding**: One initial page publication cannot support later eviction, seek, or distant reversal.
- **Requirement**: Store every canonical page outside the worker JavaScript heap.
- **Requirement**: Keep a page available after the fixed AudioWorklet cache evicts it.
- **Requirement**: Treat initial publication as cache warm-up, not asset ownership transfer.
- **Requirement**: A seek must wait for retrieval, validation, and atomic publication.
- **Requirement**: Retrieval must preserve the canonical page and asset identities.
- **Status**: The worker stores canonical pages by asset identity, generation, and core range.
- **Status**: The worker commits the complete manifest after all page writes succeed.
- **Status**: The default initial warm-up publishes one page.
- **Status**: Page, distant-seek, reverse-boundary, and eviction-reload requests use demand retrieval.
- **Status**: The worklet accepts bounded base and pyramid channel chunks into fixed Rust storage.
- **Status**: Each chunk, validation step, and publication has one matching acknowledgment.
- **Status**: The host uses deterministic least-recently-used eviction.
- **Evidence**: `PVC-002` records the exact eight-slot and nine-page capacity failure.
- **Evidence**: The production WASM test returns `PhysicalRealtimePagedStatus.NoEmptySlot` for page nine.
- **Evidence**: The browser smoke uses the production IndexedDB store.
- **Evidence**: The smoke preserves a committed asset during replacement preparation.
- **Evidence**: The smoke deletes one exact asset generation through explicit pruning.
- **Open requirement**: Define a bounded product retention policy for stored asset generations.

### Cross-agent WebAssembly memory

- **Observation**: The current WebAssembly JavaScript interface assigns each agent a separate store.
- Source: <https://webassembly.github.io/spec/js-api/#stores>
- **Effect**: A worker cannot pass an ordinary instantiated Rust `Arc` to an AudioWorklet.
- **Possible alternative**: A shared-memory build can transfer bounded page bytes through an atomic ring.
- **Limit**: Shared bytes do not transfer Rust ownership or validated cache state.
- **Requirement**: The consumer must validate page identity before the page can become render-visible.
- **Disproof test**: Prove that one browser-supported instance can safely own state across both agents.

## 2026-08-01: Reference Velocity Convention

### Claim: The Ortofon seed uses a 5 cm/s RMS reference

- **Status**: Resolved for this seed profile.
- **Confidence**: High for the identified Ortofon source.
- **Evidence**: Ortofon specifies its 1 kHz reference tracks as 5 cm/s RMS.
- Source: <https://ortofon.com/products/ortofon-test-record>
- **Conflicting context**: The AES tracing review states that many standards use 5 cm/s peak.
- Source: <https://www.aes.org/e-lib/download.cfm/22236.pdf?ID=22236>
- **Decision**: Keep `full_scale_sine_velocity_rms_m_s` at 0.05 for the Ortofon seed.
- **Decision**: Keep the square-root-of-two conversion from sine RMS velocity to peak velocity.
- **Limit**: Do not apply this convention to another test record without its specification.
- **Possible error**: A manufacturer document can use a different reference for the cartridge output value.
- **Disproof test**: Measure the identified reference track with a traceable velocity calibration.

### Cartridge output reference remains unresolved

- **Date**: 2026-08-02.
- **Status**: Open generator-scale ambiguity.
- **Evidence**: Ortofon specifies 10 millivolts at 1 kilohertz and 5 centimeters per second for the Concorde MKII Scratch.
- Source: <https://ortofon.com/products/concorde-mkii-scratch>
- **Limit**: The product page does not identify the velocity value as peak or RMS.
- **Limit**: The test-record convention does not prove the product-rating convention.
- **Effect**: The wrong convention changes the derived generator coefficient by a factor of square root of two.
- **Decision**: Do not change the seed coefficient without primary evidence or a traceable cartridge measurement.
- **Disproof test**: Measure loaded output with a groove that has traceable peak and RMS velocity.

### Passive cartridge magnetic-loss checkpoint

- **Date**: 2026-08-02.
- **Commit**: `49cd9424b129d79ee7d3ec40f8d5bf11543e30a2`.
- **Status**: A passive parameterized topology is implemented.
- The coil uses one residual coupled inductor and as many as four series relaxation sections.
- Each relaxation section contains a resistor in parallel with an inductor.
- At low frequency, the relaxation inductors restore the declared total self-inductance.
- At high frequency, each section adds its loss resistance.
- The apparent self-inductance then approaches the positive residual inductance.
- **Passivity rule**: `L0 - sum(Lk) - abs(M)` must be at least 0.000000001 henry.
- **Canonical rule**: Active slots are contiguous and have increasing relaxation time.
- **Seed result**: All four Concorde magnetic-loss slots are zero and `Estimated`.
- **Replacement rule**: Zero slots use the checkpoint `6791e90` circuit equations without numerical changes.
- **Evidence**: One test compares every affine coefficient with `f64::to_bits` for 10000 coupled samples.
- **Energy test**: Three coupling ratios each use 20000 alternating rapid reversals.
- **Coupling ratios**: -0.72, 0.0, and 0.72.
- **Energy bound**: The residual is at most `8e-11 * scale + 3e-27` joules.
- **Frequency test**: The complex impedance matches its low-frequency and high-frequency limits.
- **Transfer test**: The public two-by-two transfer agrees with the independent complex circuit equations.
- **Time test**: The trapezoidal response agrees with the frequency response after bilinear frequency warping.
- **Allocation test**: The active network and coupled pickup path allocate no memory during processing.
- **Snapshot result**: Snapshot schema version 4 stores every branch current and interval average.
- **Calibration rule**: Branch values require `CartridgeMagneticLossLevelAndTemperature` evidence.
- **Limit**: The model is a linear small-signal network with identical branches in both channels.
- **Limit**: It does not model saturation, nonlinear hysteresis, thermal drift, or channel-asymmetric magnetic loss.
- **Limit**: Four fitted poles can omit additional eddy-current or magnetic spatial modes.
- **Limit**: Loaded response alone confounds coil loss with the cable and preamplifier network.
- **Limit**: Total separation cannot identify generator leakage separately from mutual inductance.
- **Requirement**: Fit complex coil impedance first under a known resistance and capacitance fixture.
- **Requirement**: Then measure generator reciprocity, level, temperature, balance, separation magnitude, and separation phase.

## 2026-08-01: Real-Time Page Ingestion Requirements

### Raw pyramid construction

- **Finding**: The first default work budget could not construct pyramid data at the 192 kHz source rate.
- **Cause**: One output needs all filter taps for two channels.
- **Decision**: Count filter arithmetic in the work estimate.
- **Decision**: Increase the default bounded work budget for the raw construction path.
- **Limit**: Raw construction cannot support sustained 20x travel with a practical render-thread budget.
- **Disproof test**: Compare page construction throughput with 192 kHz and 3.84 MHz source demand.

### Precomputed pyramid transfer

- **Requirement**: A worker must construct the canonical 2x, 4x, 8x, and 16x levels.
- **Requirement**: The worklet must copy each level into fixed storage with bounded work.
- **Requirement**: The worklet must hash every base and pyramid sample before publication.
- **Requirement**: The worklet must validate the page identity and overlapping seams.
- **Requirement**: A partial, corrupt, or stale page must remain invisible to the renderer.
- **Requirement**: One state change must publish a fully validated page.
- **Requirement**: JavaScript must write into existing WASM storage without an allocating slice conversion.
- **Possible error**: Browser scheduling can still delay ingestion beyond the resident safety horizon.
- **Disproof test**: Delay the worker while repeated 20x reversals consume the resident window.

### Fixed-storage reservation result

- **Status**: Implemented and tested in Rust.
- One page slot can own one pending chunk reservation.
- A reservation validates its phase, target, next offset, length, and fixed destination before state changes.
- The host writes directly into the reserved WASM storage.
- Commit validates each value in place before it advances page progress.
- A failed commit keeps the reservation available for correction or cancellation.
- Partial pages remain unavailable to the renderer.
- Sequence-checked tokens reject stale and duplicate operations.
- Tests prohibit allocation during complete precomputed page ingestion and tracing.
- **Limit**: The browser worklet path does not use this interface until the WASM and JavaScript migration is complete.

## 2026-08-01: Rapid-Scratch Contact Bandwidth

### Claim: Output anti-aliasing does not fully preserve nonlinear contact physics

- **Status**: Open physical gap.
- **Confidence**: High from the implemented signal flow.
- **Evidence**: The renderer selects a filtered spatial level before it solves stylus contact.
- **Effect**: High-frequency groove features cannot cause their original contact loss or impulse forces.
- **Effect**: The error grows when groove travel exceeds one source frame per physical sample.
- **Limit**: A 192 kHz step cannot directly resolve a 3.84 MHz path at 20x travel.
- **Possible fix**: Use rate-adaptive contact substeps and decimate the electrical output separately.
- **Possible fix**: Use a validated swept-contact solver with conservative bounds.
- **Risk**: Full 20x substeps can exceed the AudioWorklet CPU budget.
- **Possible error**: Mechanical and electrical bandwidth can make some omitted features negligible.
- **Disproof test**: Compare the renderer with a high-rate reference on repeated reversals and tracking-loss cases.
- **Requirement**: Do not close this gap with listening tests alone.

### Profile-bound traced-page hypothesis

- **Hypothesis**: Materialize the spherical envelope before the high-speed spatial pyramid.
- **Benefit**: Spatial filtering would occur after nonlinear tracing distortion.
- **Benefit**: Worker construction could use a slower certified global search.
- **Cost**: The page identity must bind the stylus geometry and trace algorithm.
- **Cost**: A different tip profile requires different derived pages.
- **Limit**: Fractional runtime positions still need a certified interpolation error.
- **Limit**: Filtered traced geometry can still hide dynamic contact loss and force impulses.
- **Decision**: Test this option against swept high-rate dynamic contact before adoption.

### Rapid-contact piece-event hypothesis

- **Date**: 2026-08-02.
- **Status**: Solver design hypothesis.
- **Confidence**: Low until a convergence test passes.
- A 20x sample crosses at most 20 base source-frame intervals under the declared speed limit.
- The current reference instead uses as many as 160 uniform substeps for one physical sample.
- **Hypothesis**: Split work at certified spline, envelope-branch, contact, and friction events.
- **Hypothesis**: Integrate each smooth interval with a passive higher-order step and a certified local error bound.
- **Benefit**: Work can follow physical events instead of a fixed eighth-frame spatial step.
- **Benefit**: Smooth intervals can span more than one reference substep without removing nonlinear contact.
- **Requirement**: The tracer certificate must bound every crossed piece and branch transition.
- **Requirement**: The contact integrator must retain contact loss, recapture, and impulse timing.
- **Requirement**: The electrical and mechanical ports must use the same interval-average work.
- **Open problem**: Record reaction impulse changes the record motion that selects the traversed path.
- **Risk**: Holding deck speed during pickup substeps can break same-sample reciprocity.
- **Risk**: A branch event can occur between all selected quadrature points.
- **Risk**: Coulomb friction can change mode without a smooth force root.
- **Requirement**: Couple total stylus impulse into the deck solve without a delayed torque.
- **Requirement**: Reject a step when the certified event capacity or error bound is exceeded.
- **Disproof test**: Compare force, torque, mode, and band-limited voltage against the converged high-rate reference.

### Offline reduced-system reference

- **Status**: Implemented as a test-only, allocation-free reference harness.
- **Scope**: The harness uses the symmetric vertical reduction of the two-wall contact equations.
- **Scope**: It does not include the complete coupled player, cartridge, or phono stage.
- **Limit**: The scalar reduction requires both walls to remain loaded.
- **Limit**: It is not exact with lateral and vertical cross-coupling.
- **Fixture**: The artificial stress source contains multitone motion, an impulse, contact loss, and retracking.
- **Sweep**: The run covers every signed integer rate from 1x through 20x.
- **Sweep**: Each rate includes forward motion, reverse motion, and another forward motion.
- **Work**: The run contains 720 output samples at 192 kilohertz.
- **Work**: The reference uses a maximum 0.125-source-frame spatial step.
- **Work**: The reference uses 84 substeps per output sample on average.
- **Work**: The reference uses 160 substeps at 20x.
- **Work**: The reference makes 120960 trace calls and 60480 contact steps.
- **Candidate**: The current path makes 1440 trace calls and 720 contact steps.
- **Property**: The timed comparison allocates no memory after fixture and state construction.

The current path has a 1.3655775345324883 normalized height RMS error.
Its signed height integral error is 93.98129343476366 percent.

The wall-force normalized RMS error is 1.034338433310777.
The wall-force impulse error is 16.672264726920533 percent.

The reaction-torque normalized RMS error is 1.0034541959889511.
The signed torque-impulse error is 72.38062132681622 percent.

The absolute torque-impulse error is 60.03927910396862 percent.
The contact-occupancy mean absolute error is 0.21032718713637744.

The candidate has 265 macro contact transitions.
The reference has 281 macro contact transitions.

The force and torque substep peaks increase when the reference step decreases.
Therefore, the peak substep values are not physical acceptance values.

### Reference convergence check

Halving the maximum spatial step changes the wall-force impulse by approximately 0.193 percent.

It changes the signed torque impulse by approximately 0.314 percent.

It changes the height integral by approximately 0.0103 percent.

The macro contact-transition count changes from 281 to 279.
Therefore, the transition count has not converged.

These values show a material candidate-reference difference.
They are not registered release error limits.

### Algorithm decisions

- **Finding**: Linear spatial filtering and nonlinear spherical tracing do not commute.
- **Finding**: Linear spatial filtering and unilateral contact do not commute.
- **Decision**: Do not justify prefilter-before-contact with a linear filter bound alone.
- **Adaptive substeps**: A fixed cap gives deterministic work.
- **Adaptive substeps**: The current reference costs 84x work on average and 160x at 20x.
- **Swept enclosure**: A conservative enclosure can detect missed envelope and contact events.
- **Swept enclosure**: It does not integrate state or contact impulse by itself.
- **Multirate integration**: A substep pickup with a macro deck can break reciprocal same-sample work.
- **Requirement**: Derive a coupled passive method before production multirate integration.
- **Decision**: Keep `RP-013` and `RP-027` open.

### Production trace-certificate boundary

`trace_spherical_sampled` remains a general helper for direct sample closures.
Production trace sources do not use that helper without admission.

Uniform, blended, and multiresolution sources use different breakpoints.
The admission pass recomputes bounds for every current representation level.

The production certificate contains these items:

- A format version.
- The complete asset, page, representation, and generation identity.
- Each spatial level and 45/45 wall combination.
- Each globally aligned source segment and its source-frame interval.
- Recomputed displacement, derivative, curvature, join, slope, and work bounds.
- The supported stylus-radius domain.
- The maximum node count, height tolerance, and position tolerance.
- A digest included in the page identity.
- Base, pyramid, overlap, and page-seam validation.

The renderer rejects a missing, stale, corrupt, or incompatible certificate.

The renderer rejects a trace that exceeds the declared fixed caps.
That failure leaves state and output unchanged.

Class B binds each runtime capacity to its exact canonical constant.
Smaller self-consistent capacity claims reject before tracing.

Incremental and full certification produce identical certificates and work totals.
Tests cover unaligned interior pages and both record boundaries.

Rejection-class certificates cannot enter the real-time `Ready` state.
They reject before hashing, seam validation, or publication.

Precomputed page levels receive bit-exact canonical-filter validation.
The validation occurs before publication.

The resident manifest binds the current page identities and certificate identities.
It is not an authoritative full-record catalog.

**Decision**: Close the current `RP-023` implementation task.
Keep comparative validation and full-record source authentication open.

### Strict-concavity coverage risk

- **Date**: 2026-08-02.
- **Status**: Resolved by two certified production classes.
- **Fast-path theorem**: A wall-curvature upper bound below inverse tip radius proves one global envelope maximum.
- **Limit**: This condition is sufficient, but it is not necessary.
- **Example radius**: 0.060 meters.
- **Example speed**: 33.333333333333336 revolutions per minute.
- **Example frequency**: 8000 hertz.
- **Example wall-velocity peak**: 0.05 meters per second.
- **Example tip radius**: 0.000018 meters.
- **Calculated tangential speed**: 0.20943951023931953 meters per second.
- **Calculated displacement amplitude**: 0.000000994718394324346 meters.
- **Calculated wall-curvature amplitude**: 57295.779513082336 per meter.
- **Calculated inverse tip radius**: 55555.555555555555 per meter.
- **Calculated curvature ratio**: 1.031324031235482.
- **Effect**: The strict-concavity proof can fail for a nominal inner-groove signal.
- **Critical limit**: Failure of this sufficient proof does not prove ambiguous contact.
- **Requirement**: Keep strict concavity as one certified fast-path class.
- **Result**: A fixed-cap piecewise path handles admitted non-concave envelopes.
- **Requirement**: Measure certificate coverage on tones, representative cuts, radii, and spatial levels.
- **Requirement**: Report admission rejection causes separately from height-order ambiguity.
- **Possible error**: Catmull interpolation and spatial filtering can change the continuous curvature bound.
- **Possible error**: Representative program material can use lower high-frequency wall velocity.
- **Disproof test**: Show that all supported assets pass strict concavity with registered headroom.

### Class-B contact-coordinate design

- **Date**: 2026-08-02.
- **Status**: Implemented with bounded interval isolation and height ordering.
- **Confidence**: High for the tested numeric domain.
- Let `x` be wall position, `g(x)` be wall displacement, and `p(x)` be `g'(x)`.
- Let `R` be the spherical tracing radius.
- Define the stylus-center coordinate as `C(x) = x - R p(x) / sqrt(1 + p(x)^2)`.
- Define the stylus-center height as `H(x) = g(x) + R / sqrt(1 + p(x)^2)`.
- A contact for center coordinate `x0` satisfies `C(x) = x0`.
- Squaring gives `(x-x0)^2 + ((x-x0)^2-R^2) p(x)^2 = 0`.
- The derivative `p(x)` is quadratic on one Catmull-Rom piece.
- Therefore, the squared contact equation has degree six or less on one piece.
- **Benefit**: This form does not evaluate the circle-slope singularity at horizontal offset `R`.
- **Possible bound**: One cubic piece has no more than six roots of the squared equation.
- **Required filter**: The signs of `x-x0` and `p(x)` must agree.
- **Risk**: Squaring introduces roots that do not satisfy the original equation.
- **Risk**: A multiple root can defeat a fixed-precision isolation test.
- **Risk**: A floating-point polynomial does not supply an outward proof by itself.
- **Requirement**: Certify every accepted root against the unsquared equation with outward intervals.
- **Requirement**: Keep a fixed root and work capacity for every piece.
- **Requirement**: Compare this method with the existing interval tracer on `PVC-001` and `PVC-004`.
- **Height-order limit**: Two winning branches can exchange order as the stylus center moves.
- **Effect**: An asset-wide unique-height guarantee can exclude a physically valid branch transition.
- **Requirement**: Prove height order for the active trace, or return a certified multiple-contact set.
- **Requirement**: Do not interpret numerical interval overlap as physical contact equality.
- **Compliance link**: Retain near-height contenders for the measured wall-compliance model.
- **Disproof test**: Find more than six valid contacts on one cubic piece.
- **Disproof test**: Find an admitted trace where the fixed solver cannot certify all valid roots.

### Strict-concavity coverage measurement

- **Date**: 2026-08-02.
- **Status**: The strict-concavity class does not cover the realistic benchmark assets.
- **Rapid fixture class**: `FixedCapPiecewise`.
- **Rapid fixture maximum signed wall curvature**: 270051.552 per meter.
- **Rapid fixture maximum absolute wall slope**: 0.491673.
- **Rapid fixture runtime support**: One wall trace examines at most 28 source-frame spline pieces.
- **Rapid fixture aggregate certificate count**: 950 pieces across two walls and five representation levels.
- **PVC-004 class**: `FixedCapPiecewise`.
- **PVC-004 maximum signed wall curvature**: 236652.492 per meter.
- **PVC-004 maximum absolute wall slope**: 0.440791.
- **PVC-004 runtime support**: One wall trace examines at most 28 source-frame spline pieces.
- **PVC-004 aggregate certificate count**: 40950 pieces across two walls and five representation levels.
- **Edge result**: Neither fixture has a C1 record clamp.
- **Passing controls**: Flat data, flat-clamped data, and a tapered 3-kilohertz inner-groove fixture pass class A.
- **Conclusion**: Class A cannot serve as the only production trace class.
- **Result**: The fixed-cap piecewise root and height-order path is active.
- **Requirement**: Do not reject these assets only because class A does not apply.
- **Requirement**: Keep `PVC-004` constructible and reject only its unresolved active height order.

## 2026-08-01: Three-Dimensional Contact Reduction

### Exact rigid-sphere case

- **Status**: Verified for the stated ideal assumptions.
- **Assumption**: The tip is a rigid sphere.
- **Assumption**: The model prescribes the sphere center along the groove direction.
- **Assumption**: Each rigid wall is a height field along one fixed 45-degree wall normal.
- **Assumption**: The test considers nonpenetration and normal reaction only.

Under these assumptions, each wall gives one independent spherical-envelope inequality.

The two wall projections reconstruct the lateral and vertical sphere-center coordinates.

The two walls can have different along-groove contact offsets.
This difference does not contradict rigid-sphere geometry.

Each frictionless normal force passes through the sphere center.
Therefore, the normal force creates no tip-center moment.

The permanent test uses two walls with separated envelope maxima.
It checks the reconstructed center against dense three-dimensional nonpenetration inequalities.

The test is `independent_longitudinal_wall_offsets_are_exact_for_the_rigid_sphere_reduction`.

### Missing geometry and dynamics

- Finite contact patches and elastic or plastic deformation.
- Longitudinal tip compliance, inertia, and displacement.
- Tip pitch, roll, yaw, and spin.
- Friction forces and moments about the tip center.
- Azimuth, vertical tracking angle, and stylus rake angle.
- Non-spherical tip orientation.
- Exact curved-groove and spindle lever-arm corrections at each contact offset.

These effects can couple the two contact locations through force and moment balance.
They can invalidate the reduced translational model for some profiles.

### Multiple contacts on one wall

- **Finding**: One wall can have two distinct possible global spherical-envelope positions.
- **Exact case**: `PVC-004` uses an 8000-hertz sine at a 0.060-meter groove radius.
- **Exact case**: The wall-velocity peak is 0.05 meters per second.
- **Reference**: Independent outward interval searches prove two separated position intervals.
- **Reference**: The separation is 3.4110433032007397 to 3.6026577436319407 micrometers.
- **Reference**: The two certified height intervals overlap.
- **Corroboration**: A 1000001-point search finds symmetric maxima near plus or minus 1.754028 micrometers.
- **Corroboration**: The dense heights differ by approximately 6.78e-21 meters.
- **Correction**: The scalar tracer returns `GlobalContactNotIsolated` for this case.
- **Critical limit**: Interval overlap does not prove exact equal height.
- **Critical limit**: The generated `f32` sine does not have proved algebraic symmetry.
- **Correction**: Contact-set APIs return `ContactHeightOrderNotIsolated` for distinct overlapping contenders.
- **Correction**: Scalar APIs map that error to `GlobalContactNotIsolated`.
- **Requirement**: Do not pass ambiguous contenders to the mechanics solver as simultaneous contacts.
- **Requirement**: Require an algebraic equality certificate before rigid simultaneous contact.
- **Alternative**: Supply certified height gaps and a measured local-compliance law.
- **Implemented**: `StylusTraceContactSet` stores a fixed-capacity set for each wall.
- **Implemented**: Contiguous, paged, and realtime-paged paths preserve these contact sets.
- **Implemented**: Public mechanical inputs accept only one contact without an internal qualification.
- **Implemented**: Serialized input cannot create the internal contact qualification.
- **Test scope**: A test-only symmetry qualification exercises multiple-contact force distribution.
- **Production limit**: No production tracer emits a qualified multiple-contact set.
- **Gap**: Equal force splitting is only a minimum-norm numerical choice.
- **Gap**: Equal force splitting is not a physical law for this contact.
- **Gap**: Longitudinal rotation and local compliance are absent.
- **Possible error**: Compliance can select one patch, but the current rigid model does not calculate that selection.
- **Requirement**: The tracer must prove one isolated global contact position.
- **Requirement**: Unresolved height order must fail transactionally.
- **Requirement**: The failure must preserve player state and output.
- **Short-term decision**: Return `GlobalContactNotIsolated` instead of selecting an arbitrary position.
- **Long-term requirement**: Test measured and admitted grooves for multi-contact force distribution.
- **Disproof test**: Use separated equal maxima and nearly equal interval-overlap cases.
- **Limit**: The case is synthetic and does not prove a cut or pressed-record topology.
- **Limit**: A player-level rollback test remains open.

### Safe contact-set handoff

- **Date**: 2026-08-02.
- **Production behavior**: Unresolved height order returns `ContactHeightOrderNotIsolated`.
- **Production behavior**: Scalar tracing maps this result to `GlobalContactNotIsolated`.
- **Production behavior**: Unqualified multiple contacts return `UnqualifiedMultipleContacts`.
- **Transaction rule**: A rejected mechanical input does not change mechanical state.
- **Test-only behavior**: Certified reflection symmetry uses fixed-capacity minimum-norm force sharing.
- **Test-only limit**: This qualification is not available through a public or serialized input.
- **Telemetry**: Snapshots contain fixed-capacity force and reaction data for each contact.
- **Verification**: Contact tests passed 31 tests and ignored one benchmark.
- **Verification**: Electromechanical tests passed all nine tests.
- **Verification**: Player tests passed 28 tests and ignored one benchmark.
- **Verification**: Renderer tests passed all 13 tests.
- **Verification**: Compile-only, format, and whitespace checks passed.
- **Decision**: Keep production multiple-contact playback closed until physical qualification exists.

### Required disproof test

Build a full three-dimensional force-and-moment oracle with measured profile inputs.

Compare it with the current reduction through sustained motion and rapid reversals.

Reject the reduction when longitudinal deflection or moment exceeds a registered measured limit.

## 2026-08-01: Host Voltage and Digital Full Scale

### Finding

- **Status**: Open host-boundary defect.
- **Confidence**: High from the active interfaces.
- **Evidence**: `PhysicalRecordPlayer::render_internal_interleaved` documents stereo volts.
- **Evidence**: It returns `PhysicalPhonoStageTelemetry::output_v` without unit conversion.
- **Evidence**: `PhysicalHostRenderer` only resamples these values.
- **Evidence**: The C output buffer has no unit or full-scale parameter.
- **Evidence**: The WASM output buffer also has no unit or full-scale parameter.
- **Evidence**: The canonical Swift wrapper returns the buffer without a conversion.
- **Context**: The browser does not use this physical buffer as programme output yet.

A phono output value in volts is not a digital full-scale audio sample.
Direct host playback gives an implicit and undocumented volts-to-full-scale ratio.

### Requirement

Keep phono-stage values in volts inside the physical model and telemetry.

Add one explicit calibrated conversion at the host-output boundary.

The boundary must declare volts per digital full scale.
It must also declare gain, headroom, and clipping behavior.

The profile or host contract must identify the calibration source and uncertainty.

Tests must map known 1-volt and 10-volt inputs to exact dimensionless outputs.

Tests must cover positive clipping, negative clipping, and unclipped telemetry.

**Decision**: Keep `RP-032` open before canonical programme activation.

### Correction checkpoint

- `PhysicalHostRenderer` now requires a finite positive `volts_per_full_scale` value.
- The boundary divides phono volts by that value and clips the result from -1 through 1.
- Renderer telemetry retains each channel's unclipped voltage peak.
- Renderer telemetry reports block and cumulative clipping counts.
- Renderer snapshots bind the level configuration and clipping state.
- C ABI version 4 requires the same value and exports the same telemetry.
- The WASM constructor requires `options.voltsPerFullScale`.
- The native Swift initializer requires `voltsPerFullScale`.
- Exact renderer tests cover 1-volt and 10-volt mapping.
- Exact renderer tests cover clipping, unclipped peaks, invalid values, and snapshot validation.
- All 15 C ABI tests pass.
- Both native Rust boundary tests pass.
- All seven native Swift package tests pass.

The value 10 volts per full scale is a test configuration.

It is not a measured product calibration value.

**Remaining requirement**: Register the product value, its source, its uncertainty, and its intended headroom before programme activation.

## 2026-08-01: Same-Sample Reaction Coupling

### Stylus-to-record feedback

- **Finding**: The player applies the previous sample's stylus reaction torque to the deck.
- **Cause**: The deck advances before the pickup calculates the current reaction torque.
- **Effect**: A torque change reaches record motion one physical sample late.
- **Risk**: The error is most important at contact onset, zero speed, and rapid reversal.
- **Requirement**: Couple deck motion and pickup reaction in one transactional physical step.
- **Possible fix**: Use a bounded predictor-corrector with complete state rollback.
- **Possible error**: The delay can be below measurement uncertainty in steady playback.
- **Disproof test**: Compare onset and reversal impulses with a converged small-step reference.

### Other same-sample findings

- The deck friction solver used the previous slipmat torque in one bearing decision.
- The cartridge reciprocal force used an endpoint current in a trapezoidal circuit step.
- The contact telemetry treated a projected wall multiplier as the full surface-normal force.
- The electromagnetic matrix check could accept a tiny indefinite matrix after numeric underflow.
- Local fixes must include passivity, work, complementarity, reversal, and snapshot tests.

### Completed local corrections

- The deck now solves bearing, slipmat, and hand friction in one implicit active-set step.
- Sliding modes cannot cross zero while they apply kinetic friction.
- The deck uses interval-average angular velocity for its angle update.
- The damping-matrix test now rejects small indefinite matrices without determinant underflow.
- Wall-normal telemetry and Coulomb friction now use the true three-dimensional normal force.
- The cartridge reciprocal force now uses the interval-average circuit current.
- The electrical and mechanical energy tests now use matching discrete-time ports.
- Focused mechanics, contact, cartridge, electromechanical, and stylus tests pass.

### Rejected scalar torque iteration

- **Status**: Rejected as a general solution.
- **Possible idea**: Restore all sample state and iterate the stylus torque to a small residual.
- **Evidence against**: Kinetic Coulomb reaction changes discontinuously when tangential velocity changes sign.
- The simplified map `F(tau) = -C sign(v_free + a tau)` can have no fixed point near reversal.
- The minimum allowed record inertia also permits a much larger torque-to-velocity sensitivity.
- Groove slope and curvature do not currently have a finite content bound.
- **Decision**: Do not ship a predictor-corrector or scalar root retry as exact coupling.
- **Requirement**: Use one complementarity solve for deck, pickup, cartridge, and reaction torque.
- **Requirement**: Include static tangential stylus traction bounded by the Coulomb limit at zero velocity.
- **Disproof test**: Prove existence, uniqueness, passivity, and bounded work for all allowed states and groove inputs.

### Fully implicit geometry boundary

- **Former finding**: The previous spherical tracer did not provide a certified interval enclosure.
- **Former finding**: Its scan and golden-section refinement could change optimization branches.
- **Finding**: Spatial levels, page seams, and radial recapture add more branch boundaries.
- **Effect**: A fixed number of black-box torque probes cannot certify a reaction root for arbitrary content.
- **Requirement**: A fully implicit solver needs certified groove curvature and stylus-envelope bounds.
- **Requirement**: Asset admission must reject content that exceeds the fixed real-time branch limit.
- **Requirement**: Page identity must bind any stored trace certificates.
- **Requirement**: The solver must enumerate deck, radial, filter, pickup, and tangential active modes.
- **Requirement**: Interval branch-and-bound must reject every torque interval that excludes zero residual.
- **Requirement**: A deterministic continuation rule must resolve an exact multiple solution.
- **Status**: Trace certification is implemented. The fully implicit coupled solver remains open.

### Midpoint discrete replacement

- **Decision**: Replace the delayed torque path with a bounded midpoint discrete solve first.
- **Method**: Evaluate groove geometry at a torque-independent midpoint.
- **Method**: Solve deck, stylus tangential contact, pickup, and cartridge in the current sample.
- **Method**: Include static traction and both kinetic directions in the active-mode enumeration.
- **Property**: Deck and pickup torque work must cancel inside the declared tangent-plane model.
- **Property**: Static contact power must be zero.
- **Property**: Sliding friction power must not be positive.
- **Limit**: Midpoint geometry is a discrete approximation to the curved continuous groove.
- **Limit**: Contact and Coulomb events reduce the local convergence order.
- **Requirement**: Compare aligned 192, 384, and 768 kHz reference results.
- **Requirement**: Register limits for impulse, reversal time, force, mode, work, and band-limited output.
- **Status**: The discrete tangent-plane path is implemented. Validation and deadline work remain open.

### Midpoint active-mode work bound

- **Status**: The initial algorithmic bound is confirmed. Deadline validation remains open.
- **Deck modes**: The solver examines up to three bearing, three slipmat, and three hand modes.
- **Pickup modes**: The solver examines three bearing modes, four wall masks, and four tangential modes.
- **Bound**: The Cartesian product contains at most 1,296 candidate branches for one physical sample.
- **Instrumentation**: The implementation counts examined branches and attempted linear solves for each sample.
- **Risk**: A fixed bound can still miss a 192 kHz callback deadline.
- **Requirement**: Measure steady playback, reversal, and adversarial branch counts in a release build.
- **Requirement**: Record the elapsed-time distribution with the hardware, toolchain, build, and fixture identity.
- **Requirement**: Reduce the practical bound or prove a smaller asset and state admission bound.
- **Decision**: Do not call the midpoint replacement production-ready before the deadline requirement passes.
- **Escalation**: Create `PVC-003` if release measurements show an impractical rate or a callback miss.

### Midpoint deterministic continuation

- **Decision**: Use a deterministic continuation order before the complete fallback search.
- **Deck order**: Use the velocity predictor for active hand and slipmat directions.
- **Static-slip order**: Estimate the torque that would remove predicted slip during one sample.
- **Static-slip equation**: Use `tau = delta_omega / (dt * (1 / I_platter + 1 / I_record))`.
- **Static-slip gate**: Try static slip first when the required torque is inside the applicable static limit.
- **Pickup order**: Start with the previous bearing direction and previous wall-contact mask.
- **Contact order**: Prefer the previous wall mask, then test every remaining mask.
- **Tangential order**: Use the predicted midpoint direction. Prefer static contact at exact zero speed.
- **Redundant static order**: Retain bearing, then slipmat, then hand, then stylus constraints.
- **Property**: The predictor changes only search order. It does not remove an active mode.
- **Property**: The complete fallback still examines all 1,296 declared branches.
- **Test**: All four previous tangential modes produce the same static zero-speed state.
- **Test**: The active-mode solve makes no allocator call after fixture construction.
- **Uncertainty**: Contact and force tolerances can make more than one branch admissible.
- **Uncertainty**: The current tests do not prove equal outputs for every tolerance overlap.
- **Requirement**: Bound observable branch differences or compare all admissible branches near each overlap.
- **Decision**: Do not describe the current order as a global maximum-dissipation proof.

### Midpoint linear-system validation

- **Method**: Scale each equation by its largest coefficient.
- **Method**: Scale each variable column after row scaling.
- **Method**: Use relative partial-pivot rejection on the scaled matrix.
- **Method**: Recalculate the residual with the original matrix and right-hand side.
- **Gate**: Reject a solution when its normalized backward error exceeds `1e-10`.
- **Test**: A mixed-scale system from `1e-12` through `1e12` recovers its declared solution.
- **Test**: The solver rejects an inconsistent singular system.
- **Uncertainty**: These tests do not establish conditioning bounds for every admitted physical profile.
- **Requirement**: Add profile-bound condition estimates if future profiles increase the accepted scale range.

### Midpoint release deadline result

- **Status**: Failed on the first identified test computer.
- **Baseline**: Rapid reversal p95 took 42,708 nanoseconds in the core solve.
- **Baseline**: Rapid reversal p99 took 144,083 nanoseconds in the core solve.
- **Improvement**: Continuation reduced rapid reversal p95 to 959 nanoseconds.
- **Improvement**: Continuation reduced rapid reversal p99 to 2,875 nanoseconds.
- **Later improvement**: Static-slip prediction reduced the sequential reversal maximum to 28 branches and 28 solves.
- **Repeatability**: Five release runs produced the same maximum branch and solve counts.
- **Core timing**: Reversal p99 ranged from 1,083 through 2,417 nanoseconds across those runs.
- **Callback result**: Two of 512 rapid-reversal blocks exceeded the 666,667 nanosecond deadline.
- **Callback maximum**: The longest measured rapid-reversal block took 812,750 nanoseconds.
- **Cross-track result**: A later changing tracer checkpoint missed all 512 blocks in both fixtures.
- **Cross-track interpretation**: The later complete-player mean was approximately 2.25 milliseconds. The approximately 1-microsecond core solve did not dominate this result.
- **Evidence status**: The later callback result is provisional because tracer correction continued during measurement.
- **Decision**: Treat the callback misses as a release blocker.
- **Evidence**: `PVC-003` records the fixtures, distributions, computer, toolchain, and reproduction commands.
- **Requirement**: Reduce the tail without weaker tolerances, removed modes, or silent work caps.

### Certified-tracer deadline checkpoint

- **State**: Safe height-order ambiguity handling and the blend-endpoint fast path are active.
- **Normal p50**: 1,130,958 nanoseconds.
- **Normal p99**: 1,219,625 nanoseconds.
- **Normal maximum**: 1,248,417 nanoseconds.
- **Normal misses**: 512 of 512 callbacks.
- **Reversal p50**: 1,103,084 nanoseconds.
- **Reversal p99**: 1,258,042 nanoseconds.
- **Reversal maximum**: 1,833,125 nanoseconds.
- **Reversal misses**: 512 of 512 callbacks.
- **Profile**: Tracer work represented approximately 55 percent of sampled render work.
- **Hot operations**: Circle bounds, cubic-slope bounds, and interval root isolation dominate tracer samples.
- **Rejected idea**: Specialized generic interval multiplication increased release time by approximately seven percent.
- **Decision**: Keep the callback deadline open.
- **Validation case**: See the identified checkpoint in `PVC-003`.

### Host torque boundary

- **Finding**: The first C control structure exposes `stylus_torque_nm` to hosts.
- **Finding**: `PhysicalRecordPlayer` replaces that value with its internal reaction torque.
- **Risk**: A public host field implies that Swift or JavaScript owns part of the physics.
- **Requirement**: Player-facing host APIs must not calculate or inject stylus reaction torque.
- **Correction**: C ABI version 3 removes the player-host torque field.
- **Correction**: The Swift control type no longer contains a stylus torque input.
- **Decision**: Keep the deck-only torque field inside Rust.
- **Disproof test**: Trace every active host control path and confirm that no host supplies reaction torque.

## 2026-08-01: Virtual Cutter Scope

### Claim: The current cutter is not a complete mastering-lathe simulation

- **Status**: Confirmed scope limit.
- **Confidence**: High from code inspection and primary literature.
- **Evidence**: The cutter applies idealized RIAA processing and a fixed spiral pitch.
- **Missing**: It does not model a cutter-head transducer or its motional feedback loop.
- **Missing**: It does not enforce the frequency-dependent peak-velocity limits of a physical cut.
- **Missing**: It does not use programme-dependent groove pitch or depth.
- **Missing**: It does not model optional tracing compensation.
- **Missing**: It does not model electroforming, pressing deformation, wear, or groove echo.
- Source: <https://secure.aes.org/forum/pubs/journal/?elib=19872>
- Source: <https://secure.aes.org/forum/pubs/journal/?elib=508>
- Source: <https://secure.aes.org/forum/pubs/journal/?elib=781>
- Source: <https://secure.aes.org/forum/pubs/journal/?elib=3846>
- **Decision**: Describe the current source as an ideal virtual cut with declared settings.
- **Decision**: Do not claim a complete record-manufacturing recreation from this cutter.
- **Possible error**: The product claim can intentionally cover playback from a defined virtual groove only.
- **Disproof test**: Match a measured lathe output and pressed record across level, frequency, and radius.

## 2026-08-01: Streaming Snapshot Correction

### Finding

- The bounded one-page API can stop with four Catmull-Rom support frames retained.
- The first snapshot validator allowed only three retained source frames.
- A 44.1 kHz stereo cut with a 17-frame core exposed the mismatch.
- The invalid bound rejected a valid resumable load after the first emitted page.

### Correction

- The exact source-history bound is now four frames.
- The snapshot validator uses the corrected bound.
- A focused test restores the previously rejected state.
- The worker facade test preserves arbitrary source chunk boundaries after restore.

## 2026-08-01: Fixed-Cache WASM Boundary

### Implemented behavior

- The worker cutter emits no more than one raw page for each push call.
- The worker materializer exports canonical base data and four spatial levels.
- Each page export includes its ranges, layouts, asset identity, and page identity.
- The renderer allocates all fixed page slots before active playback.
- The host reserves a bounded destination in the renderer WASM memory.
- The host writes each base or pyramid chunk into that destination.
- A sequence-checked commit validates the values before it advances progress.
- Page publication validates the identity, derived levels, ranges, and overlapping seams.
- A partial, stale, corrupt, or rejected page stays unavailable to rendering.
- The player-facing WASM control does not accept stylus reaction torque.

### Verification

- Eighteen focused WASM facade tests pass.
- The `wasm32-unknown-unknown` check passes.
- The release WASM build and exact `wasm-bindgen` generation pass.
- A Node smoke test uses the generated JavaScript bindings.
- The smoke test cuts, materializes, reserves, commits, validates, publishes, loads, controls, and renders a page.
- The seed smoke source requires a 39-frame tracing halo.

### Remaining risks

- The browser worker and AudioWorklet use this boundary for cache preparation.
- `ScratchAcousticDsp` still supplies browser programme output.
- Canonical programme output must not activate until the recorded API gaps close.
- The worker and AudioWorklet require separate WASM instances.
- A reservation pointer is valid only until its synchronous commit or cancel.
- Browser scheduling can still exhaust the resident safety horizon.
- The worklet cannot build raw pyramids fast enough for sustained 20x travel.

### Trace-admission ingress audit

- **Date**: 2026-08-02.
- **Status**: Byte, identity, and canonical-pyramid corrections are implemented.
- **Confidence**: High for the identified byte and identity paths.
- **Finding**: The public WASM facade used one staging buffer for all page slots.
- **Finding**: Two slots could reserve that same pointer before either slot committed.
- **Effect**: One ordinary ingestion sequence could overwrite another slot's staged bytes.
- **Correction**: The facade now permits only one global staging reservation.
- **Permanent test**: Two slots cannot hold the shared staging pointer at the same time.
- **Correction**: Commit copies staged samples into private cache storage.
- **Permanent test**: A stale view changes only staging after commit.
- **Trust limit**: Shared-memory mutation during one synchronous commit remains a host-process trust boundary.

- **Former finding**: Precomputed levels proved only their own bytes and numeric trace bounds.
- **Former finding**: The cache did not compare those levels with canonical decimation from base data.
- **Effect**: A self-consistent altered pyramid could change high-speed groove geometry.
- **Rejected argument**: A recomputed page hash does not prove canonical filtering.
- **Correction**: Compare every supplied level with the canonical bounded filter result.
- **Correction**: Include that comparison in fixed work accounting.
- **Permanent test**: Recompute the certificate and page hash after a level change.
- **Result**: The page rejects as `NoncanonicalSpatialPyramid`.

- **Finding**: A resident-page manifest is not an authoritative full-record catalog.
- **Finding**: It binds only the pages that are resident at that time.
- **Effect**: A self-consistent first page can claim an unverified full-source identity.
- **Possible interpretation**: The page producer can be an explicitly trusted process.
- **Requirement**: State that trust boundary until a durable catalog or Merkle root exists.
- **Future correction**: Bind ordered page identities to one authoritative full-record root.
- **Disproof test**: Substitute a self-consistent page while retaining the claimed source root.

- **Former finding**: Direct `PhysicalGroovePage` deserialization accepted absent optional proof fields.
- **Limit**: Validated cache insertion rejected the incomplete page before rendering.
- **Conflict**: The direct wire envelope retained a removed legacy shape.
- **Correction**: The pyramid and trace certificate are mandatory during deserialization.
- **Permanent test**: Remove either field and require deserialization failure.
- The current prefetch planner allocates and requires stopped rendering.
- The replay snapshot methods allocate on the AudioWorklet thread.
- The replay schema stores derived motion instead of raw pointer samples.
- The browser has no canonical pointer-radius calibration value from the profile.
- Minimum-device deadline and memory tests remain necessary.

### Current Rust workspace result

- The canonical library passes 536 tests.
- Five manual benchmarks and evidence reports remain ignored.
- The native C boundary passes 15 tests.
- The full Rust workspace has no failed test at this checkpoint.
- The actual `wasm32-unknown-unknown` feature build passes.

## 2026-08-01: Scratch Technique Ownership and Prediction

### Existing ownership

- **Finding**: Rust already defined eight preset names and default click counts.
- **Finding**: The browser also defined the catalog and defaults.
- **Finding**: Only the previous `ScratchAcousticDsp` path applied the Rust gate.
- **Finding**: `PhysicalRecordPlayer` did not apply a scratch gate.
- **Effect**: The catalog and audible behavior did not have one canonical owner.
- **Requirement**: Rust must own the catalog, state, prediction, gate, and automatic crossfader policy.
- **Requirement**: Swift and JavaScript must use the Rust catalog and results.

### Motion input

- **Finding**: The previous integration held one rendered-rate value across a host block.
- **Effect**: Gate timing could differ from the motion that produced each audible sample.
- **Requirement**: Process each physical sample with its same-sample rendered record rate.
- **Requirement**: Clock technique phase from exact signed record-angle travel.
- **Definition**: Divide signed record-angle change by nominal angular velocity to get source travel seconds.
- **Requirement**: Supply intent rate separately from rendered rate.
- **Requirement**: Use intent only for bounded event anticipation.
- **Requirement**: Use rendered travel for audible phase and completed-stroke learning.

### Technique counterexamples

- **Stab**: The previous law stayed open for the complete forward stroke.
- **Required behavior**: Produce a short open pulse during forward motion.
- **Flare and Orbit**: The previous laws used equal equations.
- **Required behavior**: Flare applies notches within one stroke. Orbit repeats the flare over both directions.
- **Crab and Transform**: The previous laws differed only by duty and default click count.
- **Required behavior**: Crab produces a bounded rapid click burst. Transform produces separate rhythmic taps.
- **Drum**: The previous law differentiated stepwise host intent.
- **Effect**: Event packetization could create attacks.
- **Effect**: A trigger could open while the record still moved in the outgoing direction.
- **Requirement**: Drum attacks must follow confirmed same-sample physical motion.
- **Click limit**: Previous patterns could repeat after the predicted endpoint.
- **Requirement**: One stroke must not produce more than the selected click count.
- **Technique source**: <https://lcme.uwl.ac.uk/media/tmpfvcmu/ttm-guide-may-2021.pdf>
- **Technique source**: <https://www.speech.kth.se/~hansen/files/thesis/Hansen02_JNMR.pdf>

### Prediction limits

- **Finding**: The first stroke used an unverified seed span.
- **Physical limit**: Software cannot infer an unseen endpoint without a trained or supplied motion model.
- **Requirement**: Report prediction confidence.
- **Requirement**: Mark first-stroke endpoint prediction as unverified.
- **Requirement**: Adapt from the first completed stroke.
- **Requirement**: Do not claim first-stroke prediction accuracy without measured evidence.
- **Finding**: `ScratchGestureMapper` is a delayed deterministic scheduler, not an endpoint predictor.
- **Limit**: Late input events shift its scheduled timeline.
- **Limit**: Ambiguous angular wraps report ambiguity but still select the shortest arc.
- **Requirement**: Validate gesture delay, wrap handling, and technique timing with captured user input.

### Integration order

- Complete certified tracing, multiple-contact mechanics, convergence, and deadline work first.
- Apply scratch audible gain to physical phono volts at 192 kilohertz.
- Apply gain before host resampling and volts-to-full-scale conversion.
- Store scratch performance state in player checkpoints.
- Add preset, click count, and manual gain to sample-timed controls.
- Prove that same-sample rendered rate stays inside the scratch helper input bound.
- Include floating-point boundary tolerance in the plus or minus 20 validation.
- Integrate the canonical scratch performance API into iOS next.
- Integrate the same API into the browser after iOS verification.
- Remove the browser catalog and behavior equations after the Rust boundary is active.
- Record exact counterexamples and permanent tests in `PVC-005`.

### Replacement checkpoint

- Rust exposes one fixed catalog with stable numeric identifiers.
- Rust exposes separate manual and automatic crossfader ownership.
- The helper processes same-sample intent, rendered rate, and exact signed source travel.
- The helper reports endpoint prediction confidence.
- The helper uses the first completed stroke immediately.
- Invalid input and invalid snapshots leave state unchanged.
- Snapshot replay produces exact output structures.
- The render path makes no allocator call.
- Packetized and per-frame Drum intent produce equal output for `PVC-005`.
- Tests cover 44.1, 48, and 96 kilohertz timing invariance.
- Tests cover rapid reversal at the maximum plus or minus 20 record rate.
- The focused scratch suite passes 53 tests.

### Calibration boundary

- Stab opens from 0.04 through 0.28 of the forward stroke.
- The Stab opening width is 0.24 stroke.
- The Transform open fraction is 0.24.
- The Flare notch half-width is 0.07 stroke.
- Crab centers its burst from 0.18 through 0.72 stroke.
- The Crab maximum pulse half-width is 0.035 stroke.
- Drum uses a 6.0 record-rate-per-second trigger.
- Drum uses a 0.045-second refractory interval.
- Drum limits one opening to 0.055 seconds.
- Initial stroke spans remain estimated values.

These values are provisional calibration parameters.

They require measured DJ motion, crossfader traces, or blinded expert calibration.

The current tests do not prove human technique timing.

The gain envelope is an idealized automatic crossfader.

It does not model a specific fader curve, cut-in distance, latency, bleed, bounce, or electrical noise.

Add those effects only from identified device measurements.
