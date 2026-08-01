# Physical Validation Cases

## Purpose

This file stores exact evidence for material physical-model findings.

The investigation log records the requirement and decision. This file records the reproducible technical case.

Do not remove a failed case after a correction. Change its status and retain the original evidence.

## Required Case Data

Each case must contain these items:

- A stable case identifier.
- The requirement identifiers that the case tests.
- The exact input values and physical units.
- The input-admission result from the production engine.
- The reference method and its error bound.
- The production result and its error values.
- The physical effect of the error.
- The reproduction command.
- The artifact location and SHA-256 digest.
- The correction status and permanent test name.
- Contrary evidence and unresolved uncertainty.

## Case Index

| Case | Requirement | Subject | Status |
| --- | --- | --- | --- |
| `PVC-001` | `RP-013`, `RP-023`, `RP-027`, `RP-033` | Spherical envelope selects the wrong local maximum | Replacement passes exact fixtures; work-limit admission remains open |
| `PVC-002` | `RP-012`, `RP-016`, `RP-028`, `RP-030` | Finalized-page publication exceeds the fixed cache | Confirmed; worker correction implemented |
| `PVC-003` | `RP-008`, `RP-013`, `RP-028` | Midpoint active-mode solve misses a rapid-reversal callback deadline | Confirmed; core tail improved, complete path still fails |
| `PVC-004` | `RP-009`, `RP-013`, `RP-023`, `RP-033` | One wall has two separated near-equal envelope maxima | Confirmed; tracer rejects unresolved height order |
| `PVC-005` | `RP-003`, `RP-005`, `RP-034` | Named scratch presets do not match their intended technique topology | Replacement topology passes; human timing calibration remains open |

## PVC-001: Spherical Envelope Global-Maximum Failure

### Claim Under Test

The integer scan and one golden-section refinement find the global spherical stylus envelope.

### Result

- **Result**: A production wall-velocity source disproves the claim.
- **Observation date**: 2026-08-01.
- **Mechanism**: Competing local maxima occur inside the spherical tracing radius.
- **Former behavior**: The local scan refined only the interval with the best sampled value.

### Primary Exact Fixture

- **Source API**: `GrooveAsset::from_stereo_wall_velocity_m_s`.
- **Source kind**: `StereoWallVelocity`.
- **Source frames**: 96.
- **Left wall**: The artifact stores every `f32` bit pattern.
- **Right wall**: Each value is the exact negation of the left value.
- **Scalar wall**: The vertical channel produces the stored scalar wall bit patterns.
- **Center**: 40.37 source frames.
- **Spatial step**: 0.0000015 meters per source frame.
- **Tracing radius**: 0.000018 meters.
- **Nominal groove radius**: 0.08250592249883855 meters.
- **Nominal speed**: 33.333333333333336 revolutions per minute.
- **Internal sample rate**: 192000 hertz.

The artifact stores exact `f64` bit patterns for the center, spatial step, and tracing radius.

### Cutter Admission

- **Peak wall velocity**: 0.05951165407896042 meters per second.
- **Wall velocity RMS**: 0.03214256359840551 meters per second.
- **Declared sine peak**: 0.07071067811865477 meters per second.
- **Maximum construction frequency**: 44001.59700186703 hertz.
- **Seed cutter bandwidth**: 50000 hertz.
- **Peak vertical displacement**: 0.0000004604399854912308 meters.
- **Generated wall peak displacement**: 0.0000003348184079767935 meters.
- **Generated wall peak slope**: 0.20000236690167938.
- **Generated wall peak velocity**: 0.05760068166768366 meters per second.
- **Generated wall peak acceleration**: 24051.018030149862 meters per second squared.
- **Final vertical drift**: 0.0000004151899796497212 meters.
- **Final program radius**: 0.08250588813830614 meters.
- **Admission result**: The production constructor accepts the source.

The source peak is below the declared sine peak.

The constructed frequencies are below the seed cutter bandwidth.

The source contains less than one revolution.
Therefore, the cutter does not test adjacent-turn clearance.

The seed API does not enforce acceleration or material limits.
Therefore, this case does not prove that a physical lathe can cut the source.

### Former Local-Scan Result

- **Center displacement**: 0.000000023518275394551524 meters.
- **Contact offset**: -0.0000018168345260679822 meters.
- **Groove displacement**: 0.00000011544433359703447 meters.
- **Groove slope**: -0.1014059134888234.
- **Tangent residual**: 0.00004746066926979153.

The executable tests do not call the former local scan.

### Replacement Checkpoint Result

- **Center displacement**: 0.00000004480387360042857 meters.
- **Center bits**: `0x3e680dcc28e73e00`.
- **Contact offset**: 0.0000022756270872122465 meters.
- **Contact bits**: `0x3ec316df38b006b8`.
- **Groove displacement**: 0.00000018922991587098205 meters.
- **Groove bits**: `0x3e8965e3f14d4cc3`.
- **Groove slope**: 0.12744631410894294.
- **Slope bits**: `0x3fc050292b8bfcd5`.
- **Tangent residual**: 0.00000000000000011102230246251565.
- **Residual bits**: `0x3ca0000000000000`.

The replacement height and position are inside the outward reference bounds.

The secondary spatial fixture also passes its outward bounds.

- **Spatial center displacement**: 0.00000023703855781733048 meters.
- **Spatial center bits**: `0x3e8fd09534544c00`.
- **Spatial contact offset**: 0.0000008345522958362502 meters.
- **Spatial contact bits**: `0x3eac00bfe8119419`.
- **Spatial groove displacement**: 0.0000002563955641556607 meters.
- **Spatial groove bits**: `0x3e9134d79dcc3768`.
- **Spatial groove slope**: 0.046413929475732675.
- **Spatial slope bits**: `0x3fa7c3910a5aafc5`.
- **Spatial tangent residual**: -0.0000000000000061547988927657116.
- **Spatial residual bits**: `0xbcfbb80000000000`.

### Reference Method

The test oracle examines every Catmull-Rom segment inside the spherical radius.

The oracle uses outward-rounded interval arithmetic for each cubic and spherical arc.

Branch-and-bound stops at a 0.0000000001-meter height tolerance.

The contact-position cells have a maximum width of 0.0000000001 meters.

The oracle visits 2704 cells for the primary fixture.

A 262145-point dense search falls inside the interval result.

The reversed source produces an overlapping reflected contact-position enclosure.

The method is a test-only oracle.
It is not a production page certificate.

### Reference Result and Error

- **Center displacement lower bound**: 0.00000004480387360039807 meters.
- **Height error bound**: 0.00000000009863312151685398 meters.
- **Chosen contact offset**: 0.0000022756274414062537 meters.
- **Contact enclosure lower bound**: 0.0000022618029785156293 meters.
- **Contact enclosure upper bound**: 0.000002289451904296879 meters.
- **Height miss lower bound**: 0.000000021285598205846543 meters.
- **Height miss upper bound**: 0.000000021384231327363397 meters.
- **Position separation lower bound**: 0.0000040786375045836115 meters.
- **Position separation upper bound**: 0.000004106286430364061 meters.
- **Height enclosure width**: 0.09863312151685398 nanometers.
- **Contact enclosure width**: 27.6489257812497 nanometers.

### Physical Significance

The height error can change the wall constraint position.

The contact-position error can change the groove slope and tangential reaction.

These changes can alter force, torque, contact loss, recapture timing, and cartridge voltage.

The case is valid for the production source domain.
It is not a measured physical-record case.

### Required Correction

Replace the local optimum search with a bounded global envelope method.

The method must examine all applicable segments and competing maxima.

The method must return declared height and position bounds.

The real-time path must have a fixed work limit.

Asset admission must reject content that cannot satisfy that work limit.

Each page identity must include the exact certificate digest.

The certificate must cover each representation level and page seam.

The renderer must fail transactionally when the certificate is absent, stale, corrupt, or too costly.

### Acceptance Tests

- The permanent tests execute only the replacement tracer.
- The replacement tracer must contain the outward interval enclosure.
- Forward and reversed input must produce reciprocal contact positions.
- Contiguous, paged, and real-time paged sources must produce the same result.
- The test must cover each spatial-pyramid level used by rapid scratching.
- Separated contenders with unresolved height order must return `GlobalContactNotIsolated`.
- A failed trace must leave player state and output unchanged.

### Reproduction

Run the focused suite:

```sh
cargo test --lib physical::rapid_scratch_reference::pvc_001_ -- --test-threads=1
```

Print the offline results:

```sh
cargo test --release --lib physical::rapid_scratch_reference::report_rapid_scratch_reference_metrics -- --ignored --nocapture --test-threads=1
```

The primary test is `pvc_001_replacement_contains_seed_domain_global_envelope`.

### Artifacts

- `tests/fixtures/pvc_001_wall_velocity_global_envelope.json`
- SHA-256: `fbb55c6d6489358efa1d5d02397ca1528bb2d45cbc6a60421dfdc6e132437760`
- `tests/fixtures/pvc_001_catmull_rom_global_envelope.json`
- SHA-256: `00e7eebebe1791f5761306b09a728300e7350cd458b35ddfb365228ce36156fb`

### Correction Status

- **Production fix**: The bounded global tracer passes both exact fixtures.
- **Permanent replacement tests**: Implemented.
- **Outward interval oracle**: Implemented for uniform Catmull-Rom test data.
- **Page-bound certificate**: Not implemented.
- **Rapid-contact comparison**: Implemented as an offline reduced-system stress test.
- **Correction gate**: Open because admission and callback work limits do not pass.

### Uncertainty and Contrary Evidence

- The primary case does not prove cutter-head, lacquer, plating, pressing, or PVC feasibility.
- A short source cannot test adjacent-turn clearance.
- The current production tracer accepts an arbitrary sample closure without interval coefficients.
- A fixed callback limit cannot certify all accepted content without stronger asset admission.
- A correct result for this case will not prove correctness for all groove data.

The accepted numeric-domain stress fixture now returns a certified result.

The result lies inside the outward height and position enclosures.

A 1,048,576-point dense search corroborates the result.

The rapid-scratch reference fixture now completes without a trace error.

Its passing result does not prove the callback deadline.

No asset certificate rejects the content before rendering.

Therefore, `PVC-001` is not corrected at the product boundary.

The secondary spatial fixture is accepted by `from_displacement_m_with_cut`.

Its peak displacement is 1.6356867675952984 micrometers.
Its maximum dimensionless slope is 0.3000049385757358.

The former local tracer missed the reference height by at least 0.11957397830317671 micrometers.

Its contact position was outside the reference enclosure by at least 3.996725512525235 micrometers.

The source needs approximately 0.0864014223098119 meters per second of wall velocity at the nominal speed.

This value exceeds the seed source's declared sine peak.
Therefore, the fixture is accepted spatial data.
Its physical feasibility is not proved.

The extreme numeric-domain fixture remains a scope warning.
It needs more than 16 meters per second of wall velocity.
It also needs 11 million meters per second squared of acceleration.

That fixture demonstrates an accepted numeric domain.
It does not support a physical-record claim.

## PVC-004: Two Near-Equal Envelope Maxima on One Wall

### Claim Under Test

One spherical wall trace can always prove one physical contact position.

### Exact Fixture

- **Observation date**: 2026-08-01.
- **Samples**: 4096 `f32` displacement samples.
- **Sample rate**: 192000 hertz.
- **Tone frequency**: 8000 hertz.
- **Wall-velocity peak**: 0.05 meters per second.
- **Displacement amplitude**: 0.000000994718394324346 meters.
- **Center**: 258 source frames.
- **Nominal groove radius**: 0.060 meters.
- **Nominal speed**: 33.333333333333336 revolutions per minute.
- **Spatial step**: 0.0000010908307824964558 meters per frame.
- **Tracing radius**: 0.000018 meters.

The artifact stores the exact generator inputs and the applicable `f32` support bits.

### Reference Method

The outward interval oracle searches the negative and positive half-radii independently.

Each search has a 0.0000000000001-meter height tolerance.

Each contact-position cell has a maximum width of 0.0000000001 meters.

The two height intervals overlap.

This overlap does not prove that the two exact heights are equal.

The two position intervals do not overlap.

### Reference Result

- **Common height lower bound**: -0.000000993385894471199 meters.
- **Common height upper bound**: -0.0000009933857944761103 meters.
- **Left position interval**: -1.8021611096566203 to -1.7042233605689562 micrometers.
- **Right position interval**: 1.7068199426317835 to 1.8004966339753207 micrometers.
- **Position separation**: 3.4110433032007397 to 3.6026577436319407 micrometers.

A 1000001-point dense search corroborates the result.

Its two heights differ by 0.000000000006776263578034403 nanometers.

### Production Result

The scalar replacement tracer returns `GlobalContactNotIsolated`.

This result prevents an arbitrary single-position force calculation.

The contact-set API returns `ContactHeightOrderNotIsolated`.

It must not convert interval overlap into simultaneous rigid contact.

Contact sets now propagate through contiguous, paged, and realtime-paged player paths.

Public mechanical inputs reject multiple contacts without an internal qualification.

Serialized inputs cannot create this qualification.

A test-only symmetry qualification exercises fixed-capacity force distribution.

No production tracer emits that qualification.

### Physical Scope

The source uses a nominal wall velocity inside the seed profile range.

The case does not prove cutter, lacquer, plating, pressing, or PVC feasibility.

The source is synthetic and not a measured record.

### Acceptance and Remaining Work

- The tracer must keep the two position intervals separate.
- The contact-set tracer must return `ContactHeightOrderNotIsolated`.
- The scalar API must return `GlobalContactNotIsolated`.
- A failed player step must preserve state and output.
- A future rigid multi-contact solver requires a certified common-height equality.
- A compliant solver requires certified height gaps and measured compliance.
- Hardware measurements must determine the prevalence of this topology.

The first three requirements pass.

The player-level transaction test and force distribution remain open.

Mechanical rejection is transactional and preserves its state.

Rigid symmetric force sharing passes test-only mechanics and replay tests.

That result does not qualify conventional near-co-contact for production playback.

### Reproduction

```sh
cargo test --lib physical::rapid_scratch_reference::pvc_004_inner_groove_sine_has_unresolved_separated_height_candidates -- --test-threads=1
```

### Artifact

- `tests/fixtures/pvc_004_inner_groove_height_order_ambiguity.json`
- SHA-256: `c5c3464cfe52d3d00e2f0552d8ef6a09922efcd92a3cbc68c76ed11d9bebd1ad`

## PVC-005: Scratch Technique Topology and Prediction

### Claim Under Test

The existing Rust presets faithfully represent their named scratch techniques.

The existing predictor also remains stable during coalesced input and rapid reversals.

### Audit Result

- **Result**: Code inspection and exact fixtures disprove the claim.
- **Observation date**: 2026-08-01.
- **Ownership defect**: The browser duplicated the preset catalog and defaults.
- **Integration defect**: Only `ScratchAcousticDsp` applied the Rust gate.
- **Timing defect**: The previous integration held one rendered rate across a host block.
- **Prediction defect**: The first stroke used an unverified seed span.

### Former Technique Counterexamples

- Stab stayed open for the complete forward stroke.
- Flare and Orbit used equal gate equations.
- Crab was a Transform duty variant with another default click count.
- Drum differentiated stepwise host intent.
- Therefore, event packetization could create a Drum attack.
- Drum could open before the record completed its outgoing motion.
- Click patterns could repeat after the predicted endpoint.
- Therefore, one stroke could exceed the selected click count.

### Replacement Technique Fixture

- **Algorithm version**: 6.
- **Sample rate**: 48,000 hertz.
- **Supported record-rate range**: -20 through 20.
- **Supported click count**: 1 through 8.
- **Maximum frame interval**: 0.000125 seconds.
- **Stab forward-open start**: 0.04 stroke.
- **Stab forward-open width**: 0.24 stroke.
- **Stab forward-open end**: 0.28 stroke.
- **Transform open fraction**: 0.24.
- **Flare notch half-width**: 0.07 stroke.
- **Flare notched direction**: Forward only.
- **Orbit notched directions**: Forward and reverse.
- **Crab burst centers**: 0.18 through 0.72 stroke.
- **Crab maximum pulse half-width**: 0.035 stroke.
- **Drum acceleration trigger**: 6.0 record-rate units per second.
- **Drum refractory interval**: 0.045 seconds.
- **Drum maximum opening**: 0.055 seconds.

These values define the current deterministic model.

They are provisional calibration values, not measured physical facts.

### Prediction Fixture

- Initial endpoint confidence is zero.
- The first completed stroke has a span of 0.123 source seconds.
- The model uses that span immediately.
- Confidence becomes 0.25 after that observation.
- Confidence becomes 1.0 after four observations.
- The permitted learned span is 0.04 through 0.8 source seconds.

No code can know an unseen first endpoint without a supplied or trained motion model.

Therefore, this case does not claim first-stroke endpoint accuracy.

### Packetization Fixture

- The fixture contains 8,000 frames at 48,000 hertz.
- Rendered rate changes from 0.5 to 1.5 at frame 3,000.
- Intent alternates between 0.35 and 0.85.
- One path updates intent every frame.
- The other path updates intent every 64 frames.
- Both paths produce exact target, gate, and audible-gain equality.

### Rapid-Reversal Fixture

- The fixture contains 32 strokes.
- Each stroke contains 400 frames.
- Record rate alternates between plus 8 and minus 8.
- Each confirmed endpoint has a closed automatic target.
- Every output stays finite and inside the unit interval.

The maximum-rate fixture also covers plus and minus 20 record rate.

### Physical Travel Fixture

- Technique phase uses exact signed record-angle travel.
- Source travel seconds equal signed record-angle change divided by nominal angular velocity.
- Final rendered rate does not approximate travel across a reversal.
- Supported timing checks use 44.1, 48, and 96 kilohertz.

### Permanent Tests

- `stab_is_one_short_forward_pulse_not_an_open_forward_stroke`
- `flare_is_one_sided_while_orbit_repeats_the_notch_on_return`
- `crab_is_a_clustered_finger_burst_not_a_transform_duty_variant`
- `transform_has_a_closed_baseline_with_brief_uniform_taps`
- `click_count_does_not_repeat_after_predicted_stroke_endpoint`
- `first_stroke_seed_has_zero_confidence_until_one_stroke_is_observed`
- `drum_is_invariant_to_intent_event_packetization_for_same_physical_trajectory`
- `drum_reversal_does_not_spend_its_hit_on_outgoing_motion`
- `baby_uses_manual_gain_and_automatic_presets_own_the_output`
- `invalid_input_and_snapshot_restore_are_transactional`
- `performance_snapshot_restore_repeats_every_output_frame`
- `performance_render_path_does_not_allocate`
- `rapid_physical_reversals_remain_bounded_and_hide_each_endpoint`
- `maximum_rate_reversals_remain_finite_and_bounded`
- `performance_timing_is_sample_rate_invariant`
- `pvc_005_fixture_matches_canonical_constants_and_claim_limits`

### Result and Claim Limit

The focused suite passes 53 tests.

The tests validate topology, ownership, deterministic state, packetization behavior, sample-rate behavior, and numeric bounds.

The tests do not validate timing against measured DJ motion or crossfader traces.

The tests do not model a specific crossfader's curve, cut-in, latency, bleed, bounce, or noise.

The tests do not prove blinded expert acceptance.

The tests do not prove physical-player, C ABI, WASM, iOS, or browser integration.

### Integration Decision

Complete the remaining physical-engine blockers before consumer integration.

Integrate iOS first. Integrate the browser after iOS verification.

Do not retain browser technique equations after the Rust API becomes active.

### Reproduction

```sh
cargo test --lib scratch_gate::tests --no-fail-fast -- --test-threads=1
```

### Artifact

- `tests/fixtures/pvc_005_scratch_semantics.json`
- SHA-256: `a9f27b64d6c4524e91f4926f9155f8b546f77176abcb380a1bc43f6262c3de99`
- `src/scratch_gate.rs` SHA-256: `5c4d6aa92344f533f0d861e50f281cb0d017a73eefb7119cadfd271a354cef0c`

## PVC-002: Finalized-Page Publication Exceeds the Fixed Cache

### Claim Under Test

The browser can publish every finalized page into the fixed AudioWorklet cache without demand paging.

### Exact Fixture

- **Observation date**: 2026-08-01.
- **Source format**: Stereo signed 16-bit PCM.
- **Source sample rate**: 48,000 Hz.
- **Source frame count**: 4,609 frames.
- **Source byte count**: 18,436 bytes.
- **PCM generator seed**: `0x13579bdf`.
- **PCM generator**: The permanent Node test defines the exact integer generator and channel equations.
- **Output sample rate**: 192,000 Hz.
- **Output frame count**: 18,432 frames.
- **Page core length**: 2,048 frames.
- **Tracing halo**: 39 frames.
- **Finalized page count**: 9 pages.
- **Default fixed-cache capacity**: 8 page slots.

The production WASM cutter accepts this source.
It produces nine contiguous canonical pages with exact final identities.

### Prior Publication Policy

The rejected draft policy published every finalized page during initial loading.
It did not evict a page between these initial publications.

The test fully ingests, validates, and publishes the first eight pages.
The ninth `beginRealtimePagedPagePrecomputed()` call returns `PhysicalRealtimePagedStatus.NoEmptySlot`.

`PhysicalRealtimePagedStatus.NoEmptySlot` has numeric value `9`.
The result comes from the production Rust fixed cache through the WASM interface.

### Reference Method

The reference is an exact capacity count.
Nine resident pages cannot fit in eight fixed slots without eviction.

The page-count and slot-count error bound is zero.
No numeric approximation affects this result.

### Physical and Product Effect

The ninth page cannot become render-visible.
A later seek or fast scratch can then produce a page miss.

The error can stop audio or delay a transport change.
It does not directly change the physical equations.

### Correction

The browser worker stores every canonical page in durable IndexedDB storage.
Each page key contains the asset identity, generation, and core range.

The worker awaits the storage transaction before it transfers page buffers.
It commits the complete asset manifest after all page writes succeed.

The default initial warm-up publishes one page.
Seeks and prefetch requests load later pages on demand.

Each demand request returns a fresh transferable page value.
The AudioWorklet must still validate identity, range, and overlapping seams.

### Acceptance Tests

- The real WASM fixture must always produce 18,432 output frames and nine pages.
- The real default cache must report eight slots.
- The ninth un-evicted page must return `NoEmptySlot`.
- One-chunk and irregular-chunk cuts must produce identical pages and identities.
- A distant seek must retrieve the last page without a new cut.
- A reverse boundary request must select the preceding page.
- An evicted page must load again with fresh transferable buffers.
- A stale generation must fail before publication.
- A storage failure must remove raw and partial new-asset pages.
- The publication protocol must permit only one unacknowledged page.

### Reproduction

Run this command:

```sh
cd /Users/jamie/wavey.ai/vin.yl.player
npm run test:physical-groove
```

The permanent production-WASM test is:

```text
the default fixed cache rejects a ninth page without eviction
```

### Artifacts

- **Fixture and test**: `vin.yl.player/test/physical-groove-worker-pipeline.test.mjs`.
- **SHA-256**: `73659fa340ebe9af7af2173d7958d01fcc46e3b572376415c6c70ea7330fa8fc`.
- **Worker design**: `vin.yl.player/PHYSICAL_GROOVE_WORKER.md`.

### Evidence Limits

The cutter, materializer, and fixed-cache overflow test use production WASM.

The Node durable-store test uses a filesystem-backed store double.
It does not execute browser IndexedDB.

The quota test injects a synthetic `QuotaExceededError`.
It does not force a real browser quota failure.

The one-publication lifecycle test uses a synthetic worker.
It does not execute an AudioWorklet.

### Correction Status

- **Worker durable store**: Implemented.
- **One-page warm-up**: Implemented as the default.
- **Demand retrieval**: Implemented for page, seek, and reverse requests.
- **Stored-asset attachment**: Implemented.
- **Production AudioWorklet activation**: Not implemented.
- **Browser IndexedDB smoke test**: Not implemented.
- **Minimum-device deadline test**: Not implemented.

### Uncertainty and Contrary Evidence

- A larger cache can hold this short fixture, but it cannot hold an arbitrary record.
- An eviction policy can fail when browser scheduling exceeds the prefetch horizon.
- IndexedDB can reject writes because of quota, privacy mode, or browser policy.
- The current tests do not prove timely page delivery during sustained 20-times scratching.
- The current tests do not prove safe page publication on each supported browser.

## PVC-003: Midpoint Solver Deadline Miss

### Claim Under Test

The bounded midpoint solver can complete each 128-frame callback during rapid reversal.

### Result

- **Result**: Disproved on the identified test computer.
- **Observation date**: 2026-08-01.
- **Internal sample rate**: 192,000 Hz.
- **One-sample budget**: Approximately 5,208.33 nanoseconds.
- **128-frame deadline**: Approximately 666,666.67 nanoseconds.
- **Observed reversal misses**: 2 of 512 callback blocks.
- **Observed reversal maximum**: 812,750 nanoseconds.
- **Release status**: The midpoint replacement is not production-ready.

### Test Computer

- **Model**: MacBook Air `MacBookAir10,1`.
- **Processor**: Apple M1 with four performance cores and four efficiency cores.
- **Memory**: 16 GB.
- **Operating system**: macOS 26.5, build `25F71`.
- **Rust compiler**: `rustc 1.96.0 (ac68faa20 2026-05-25)`.
- **LLVM**: 22.1.2.
- **Cargo**: `cargo 1.96.0 (30a34c682 2026-05-25)`.
- **Target**: `aarch64-apple-darwin`.
- **Build**: Cargo default release profile with no `RUSTFLAGS` value.

The test did not reserve a real-time core.

The test did not disable other operating-system work.

These limits can add timing noise. They cannot make the observed deadline miss acceptable.

### Core-Solver Fixture

The core test starts the platter and record at one normalized playback rate.

The normal fixture changes both wall displacement and wall slope with deterministic sine functions.

The normal fixture processes 8,192 sequential samples.

The reversal fixture uses the same deterministic wall input.

The hand target alternates between plus and minus 20 normalized rates every 32 samples.

The hand normal force is 5 N. The contact radius is 0.12 m.

The constructed search uses these record rates:

```text
-20, -1, 0, 1, 20
```

It combines seven wall-slope pairs, five displacement pairs, and four prior tangential modes.

The production input checks accepted every reported sequential sample.

### Failed Baseline

The table shows `minimum / p50 / p95 / p99 / maximum`.

| Core fixture | Candidate branches | Linear solves | Elapsed nanoseconds | Mean nanoseconds |
| --- | --- | --- | --- | ---: |
| Normal | `1 / 1 / 13 / 13 / 41` | `1 / 1 / 13 / 13 / 41` | `916 / 1,000 / 8,666 / 8,792 / 49,500` | 1,905 |
| Reversal | `1 / 1 / 97 / 289 / 369` | `1 / 1 / 85 / 265 / 333` | `708 / 833 / 42,708 / 144,083 / 212,125` | 7,397 |

The constructed search found one successful case with 311 branches and 311 linear solves.

The formal algorithmic ceiling remains 1,296 branches and 1,296 linear solves.

### Continuation Optimization Checkpoint

The solver now orders hand and slip modes with a torque predictor.

It keeps every active mode in the fallback search.

The solver also starts with the previous wall-contact mask.

It requires positive normal force for a kinetic tangential mode.

| Core fixture | Candidate branches | Linear solves | Elapsed nanoseconds | Mean nanoseconds |
| --- | --- | --- | --- | ---: |
| Normal | `1 / 1 / 1 / 1 / 28` | `1 / 1 / 1 / 1 / 28` | `666 / 1,000 / 1,125 / 1,209 / 55,208` | 1,009 |
| Reversal | `1 / 1 / 1 / 1 / 118` | `1 / 1 / 1 / 1 / 109` | `625 / 916 / 959 / 2,875 / 69,541` | 1,182 |

The constructed search found one successful case with 255 branches and 255 linear solves.

These results improve the common path. They do not close the callback deadline failure.

### Static-Slip Prediction Checkpoint

The solver estimates the torque that would remove predicted slip during one sample.

It tries static slip first when that torque is inside the applicable static limit.

This predictor changes only the search order. The complete fallback remains available.

The table shows `minimum / p50 / p95 / p99 / maximum`.

| Run | Core fixture | Candidate branches | Linear solves | Elapsed nanoseconds | Mean nanoseconds |
| ---: | --- | --- | --- | --- | ---: |
| 1 | Normal | `1 / 1 / 1 / 1 / 28` | `1 / 1 / 1 / 1 / 28` | `791 / 1,167 / 2,625 / 2,709 / 73,875` | 1,342 |
| 1 | Reversal | `1 / 1 / 1 / 1 / 28` | `1 / 1 / 1 / 1 / 28` | `708 / 1,042 / 1,125 / 1,542 / 34,791` | 1,102 |
| 2 | Normal | `1 / 1 / 1 / 1 / 28` | `1 / 1 / 1 / 1 / 28` | `667 / 1,000 / 1,042 / 1,125 / 56,583` | 992 |
| 2 | Reversal | `1 / 1 / 1 / 1 / 28` | `1 / 1 / 1 / 1 / 28` | `625 / 917 / 959 / 1,083 / 27,209` | 932 |
| 3 | Normal | `1 / 1 / 1 / 1 / 28` | `1 / 1 / 1 / 1 / 28` | `791 / 1,083 / 1,125 / 1,625 / 39,083` | 1,118 |
| 3 | Reversal | `1 / 1 / 1 / 1 / 28` | `1 / 1 / 1 / 1 / 28` | `708 / 1,041 / 1,125 / 1,167 / 51,042` | 1,041 |
| 4 | Normal | `1 / 1 / 1 / 1 / 28` | `1 / 1 / 1 / 1 / 28` | `791 / 1,083 / 1,167 / 2,291 / 75,917` | 1,124 |
| 4 | Reversal | `1 / 1 / 1 / 1 / 28` | `1 / 1 / 1 / 1 / 28` | `708 / 959 / 1,042 / 2,417 / 25,167` | 1,033 |
| 5 | Normal | `1 / 1 / 1 / 1 / 28` | `1 / 1 / 1 / 1 / 28` | `791 / 1,167 / 1,209 / 1,292 / 34,958` | 1,185 |
| 5 | Reversal | `1 / 1 / 1 / 1 / 28` | `1 / 1 / 1 / 1 / 28` | `750 / 1,083 / 1,167 / 1,250 / 24,750` | 1,084 |

Each run processed 8,192 normal samples and 8,192 reversal samples.

All five constructed searches found the same successful maximum of 255 branches and 255 solves.

The allocation guard detected no allocator call during one complete active-mode solve.

This checkpoint closes the observed sequential core branch tail. It does not prove the complete callback deadline.

### Complete Player Callback Fixture

The complete player uses the seed profile and a deterministic 3 kHz sine groove.

Each player renders 1,024 warm-up frames before measurement.

Each measurement contains 512 blocks. Each block contains 128 frames.

The reversal fixture changes the hand target every 64 frames.

The hand target alternates between plus and minus 20 normalized rates.

| Player fixture | Elapsed nanoseconds: minimum / p50 / p95 / p99 / maximum | Mean nanoseconds | Deadline misses |
| --- | --- | ---: | ---: |
| Normal | `477,458 / 495,250 / 551,458 / 567,292 / 602,125` | 500,997 | 0 of 512 |
| Reversal | `476,834 / 507,375 / 592,750 / 621,417 / 812,750` | 524,789 | 2 of 512 |

### Provisional Cross-Track Callback Measurement

A later shared checkpoint included changing certified-tracer work.

The complete-player benchmark measured that checkpoint once.

| Player fixture | Elapsed nanoseconds: minimum / p50 / p95 / p99 / maximum | Mean nanoseconds | Deadline misses |
| --- | --- | ---: | ---: |
| Normal | `1,997,166 / 2,256,167 / 2,421,583 / 2,501,500 / 2,638,667` | 2,250,275 | 512 of 512 |
| Reversal | `2,002,791 / 2,249,375 / 2,422,542 / 2,660,709 / 5,076,833` | 2,242,475 | 512 of 512 |

The approximately 1-microsecond core solve did not dominate this complete-player result.

This result identifies a tracer-dominated cross-track risk. It is not a final certified-tracer checkpoint.

Do not use this provisional result to replace the stable callback checkpoint above.

### Ambiguity-Safe Certified-Tracer Checkpoint

This checkpoint includes the safe height-order ambiguity contract.

It also includes the spatial-blend endpoint fast path.

The table reports p50, p95, p99, maximum, and deadline misses.

| Player fixture | p50 ns | p95 ns | p99 ns | Maximum ns | Deadline misses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Normal | 1,130,958 | 1,195,584 | 1,219,625 | 1,248,417 | 512 of 512 |
| Reversal | 1,103,084 | 1,191,667 | 1,258,042 | 1,833,125 | 512 of 512 |

The fixture uses 1,024 warm-up frames.

It then measures 512 blocks of 128 frames at 192 kilohertz.

The reversal target alternates between plus and minus 20 every 64 frames.

A sampling profile attributed approximately 55 percent of render samples to the tracer.

The hottest tracer operations were circle-height bounds, cubic-slope bounds, circle-slope bounds, and root isolation.

One specialized interval-multiplication attempt preserved correctness but increased release time by approximately seven percent.

That optimization was rejected and reverted.

Shared contact edits and operating-system noise can move later measurements.

Retain this exact distribution as one identified source-state checkpoint.

### Reference Method

`std::time::Instant` measures each core step and each complete player block.

The deadline equals the block frame count divided by the internal sample rate.

This comparison has no model error. Operating-system scheduling adds measurement variation.

Repeat tests must report the full distribution and all deadline misses.

### Physical and Product Effect

A callback miss can cause an audio underrun on a host with no remaining timing margin.

An underrun breaks the continuous physical output even when the state equations remain valid.

The risk increases during fast hand reversals and active-mode changes.

### Required Correction

Reduce the practical branch tail without removing a physically admissible mode.

Keep deterministic continuation, complementarity checks, passivity checks, and residual validation.

Measure the complete callback on every minimum supported device.

Do not call the path real-time safe until all declared callback deadlines pass.

### Reproduction

Run the core benchmark:

```sh
cd /Users/jamie/wavey.ai/record-player
cargo test --release --lib physical::contact::tests::midpoint_release_work_benchmark -- --ignored --nocapture
```

Run the complete player benchmark:

```sh
cd /Users/jamie/wavey.ai/record-player
cargo test --release --lib physical::player::tests::midpoint_player_release_block_benchmark -- --exact --ignored --nocapture --test-threads=1
```

### Artifacts

- **Measured result record**: `tests/fixtures/PVC-003-midpoint-deadline-m1.txt`.
- **Measured result SHA-256**: `8955beb7e63f1250ba50fa28453a7ba156e7a1ef079e73c29c7deb74d623a4e8`.
- **Core fixture**: `src/physical/contact.rs`.
- **Core fixture SHA-256 at this checkpoint**: `dce3ab59d2b32a64d6c922fca2a91f41364d32df3b61723df55e1b7cac652bb6`.
- **Player fixture**: `src/physical/player.rs`.
- **Player fixture SHA-256 at this checkpoint**: `00df3f834505495842b231c9940809395b55bb4a9f2402d0ef512239ccbc441e`.

The source digests identify the checkpoint before later tail-reduction work.

- **Current static-slip mechanics SHA-256**: `e812f74edc616b2652ccd41bcaf6e2a0e05822bc451f89f933f7255afb9425a7`.
- **Current static-slip contact SHA-256**: `2fc537815e1cb990384e76361c8124b367a61a78114d9b6ddfdb6674970ac862`.
- **Provisional tracer SHA-256**: `4d62068171287ea1dd6f4c2a1e1c4ce4daf9363587e59bd99842c0aaa2428ce7`.

The provisional tracer digest identifies only the measured cross-track checkpoint.

Formatting and an allocation test followed the five core measurements.

The production solver tokens did not change. The current hashes do not identify those measured files byte-for-byte.

### Correction Status

- **Failed baseline retained**: Complete.
- **Continuation ordering**: Implemented.
- **Contact-mask continuation**: Implemented.
- **Static-slip prediction**: Implemented.
- **Scaled residual validation**: Implemented.
- **Active-mode allocation test**: Passed.
- **Complete callback deadline**: Failed.
- **Minimum-device validation**: Not started.

### Uncertainty and Contrary Evidence

- Normal playback met the measured 128-frame deadline on this computer.
- The optimized reversal p99 was below the deadline.
- Two optimized reversal blocks still missed the deadline.
- A later changing tracer checkpoint missed all measured callback deadlines.
- The later measurement is provisional and tracer-dominated.
- The five-run tail record lacks a byte-for-byte source digest.
- A faster computer cannot establish the minimum-device requirement.
- A non-isolated computer can add scheduling noise.
- No test in this case proves hard real-time behavior on macOS.
