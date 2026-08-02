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
| `PVC-001` | `RP-013`, `RP-023`, `RP-027`, `RP-033` | Spherical envelope selects the wrong local maximum | Certified replacement and bounded asset admission pass |
| `PVC-002` | `RP-012`, `RP-016`, `RP-028`, `RP-030` | Finalized-page publication exceeds the fixed cache | Confirmed; worker correction implemented |
| `PVC-003` | `RP-008`, `RP-013`, `RP-028` | Midpoint active-mode solve misses a rapid-reversal callback deadline | Confirmed; core tail improved, complete path still fails |
| `PVC-004` | `RP-009`, `RP-013`, `RP-023`, `RP-033` | One wall has two separated near-equal envelope maxima | Confirmed; tracer rejects unresolved height order |
| `PVC-005` | `RP-003`, `RP-005`, `RP-034` | Named scratch presets do not match their intended technique topology | Replacement topology passes; human timing calibration remains open |
| `PVC-006` | `RP-008`, `RP-020`, `RP-024` | Passive cartridge magnetic-loss invariants | Implemented and green; cartridge measurement remains open |
| `PVC-007` | `RP-008`, `RP-013`, `RP-026`, `RP-033`, `RP-035`, `RP-036`, `RP-038`, `RP-039` | Passive groove compliance, coupled patches, and vector friction | Certified coordinates pass; continuous state, sloped sticking, compliance, and measurements remain open |

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

- **Center displacement**: 0.00000004480387360042518 meters.
- **Center bits**: `0x3e680dcc28e73c00`.
- **Contact offset**: 0.00000227562708721225 meters.
- **Contact bits**: `0x3ec316df38b006c0`.
- **Groove displacement**: 0.00000018922991587098192 meters.
- **Groove bits**: `0x3e8965e3f14d4cbe`.
- **Groove slope**: 0.12744631410894303.
- **Slope bits**: `0x3fc050292b8bfcd8`.
- **Tangent residual**: Positive zero.
- **Residual bits**: `0x0000000000000000`.

The replacement height and position are inside the outward reference bounds.

The secondary spatial fixture also passes its outward bounds.

- **Spatial center displacement**: 0.00000023703855781733048 meters.
- **Spatial center bits**: `0x3e8fd09534544c00`.
- **Spatial contact offset**: 0.0000008345522958362521 meters.
- **Spatial contact bits**: `0x3eac00bfe811942b`.
- **Spatial groove displacement**: 0.0000002563955641556607 meters.
- **Spatial groove bits**: `0x3e9134d79dcc3768`.
- **Spatial groove slope**: 0.046413929475732675.
- **Spatial slope bits**: `0x3fa7c3910a5aafc5`.
- **Spatial tangent residual**: -0.00000000000000625888230132432.
- **Spatial residual bits**: `0xbcfc300000000000`.

### Earlier High-Rate Audit Observation

- **Status**: Retained observation without a recovered exact artifact.
- **Reported mechanism**: One realistic Catmull-Rom groove produced two envelope maxima.
- **Reported former result**: The scan-plus-golden tracer selected the lower maximum.
- **Reported height error**: Approximately 7.07 micrometers.
- **Reported contact-position error**: Approximately 3.74 micrometers.
- **Interpretation**: Both reported errors are physically material if the values and units are correct.
- **Uncertainty**: The current fixtures do not reproduce the reported 7.07-micrometer height error.
- **Possible explanation**: The observation can refer to a different fixture or a different height convention.
- **Possible explanation**: The reported height unit can be wrong.
- **Requirement**: Keep this observation until an exact fixture confirms or disproves it.
- **Requirement**: Do not use these approximate values as acceptance bounds.
- **Decision**: The independently reproduced fixtures already require a certified global tracer.

### Constant-Segment Catmull-Rom Micro-Kink

- **Status**: Confirmed numerical defect and corrected production arithmetic.
- **Observation date**: 2026-08-02.
- **Input point value**: `2.0205539476957236e-10` meters.
- **Input point bits**: `0x3debc5313380da57`.
- **Input topology**: All four Catmull-Rom points have that exact value.
- **Required result**: The cubic is constant, and all three derivative coefficients are positive zero.
- **Former cubic coefficient `a`**: `-1.2924697071141057e-26` meters.
- **Former coefficient `a` bits**: `0xba90000000000000`.
- **Former cubic coefficient `b`**: `6.462348535570529e-26` meters.
- **Former coefficient `b` bits**: `0x3ab4000000000000`.
- **Former cubic coefficient `c`**: Positive zero.
- **Mechanism**: The former absolute-value formula canceled repeated nonzero terms after separate multiplication.
- **Effect**: The false derivative prevented a bit-exact constant-edge C1 certificate.
- **Physical significance**: The coefficient size is not a material groove displacement.
- **Engineering significance**: A false kink can reject valid data or hide a real fixture discontinuity.
- **Correction**: Form the cubic from `y0-y1`, `y2-y1`, and `y3-y1` differences.
- **Correction result**: Coefficients `a`, `b`, and `c` are bit-exact positive zero.
- **Consistency requirement**: The production tracer and admission code must use identical arithmetic.
- **Permanent test**: `constant_catmull_points_produce_bit_exact_zero_derivatives`.

### Certified Class-A Checkpoint

- **Status**: The isolated class-A tracer tests pass before complete player integration.
- **Flat test**: `certified_concave_trace_matches_exhaustive_flat_and_clamped_edges`.
- **Flat centers**: 0.0, 64.375, 127.0 source frames.
- **Flat spatial step**: 0.000002 meters per frame.
- **Flat tracing radius**: 0.000018 meters.
- **Tone test**: `certified_concave_trace_matches_exhaustive_inner_groove_tone`.
- **Tone source**: 1024 `f32` samples at 192000 hertz.
- **Tone frequency**: 3000 hertz.
- **Tone displacement peak**: 0.000001 meters.
- **Tone edge treatment**: A 128-frame raised-cosine fade has two exact-zero endpoint samples.
- **Tone spatial step**: 0.0000011 meters per frame.
- **Tone centers**: 128.0, 255.375, 511.875, 800.125 source frames.
- **Maximum positive Catmull-Rom curvature**: 8035.028657119736 per meter.
- **Sphere-curvature limit**: 55555.555555555555 per meter.
- **Available strict margin**: 47520.52689843582 per meter.
- **Curvature use**: 14.4631 percent of the sphere-curvature limit.
- **Supplied test margin**: 23760.26344921791 per meter.
- **Height agreement**: The certified and exhaustive results differ by no more than one picometer.
- **Position agreement**: The results differ by no more than 0.1 nanometers.
- **Slope and residual agreement**: The results differ by no more than 0.000001.
- **Allocation test**: The successful certified trace makes no allocator calls.
- **Failure test**: A forced numerical-bound failure reads one cubic and returns `StationaryContactNotIsolated`.
- **Fallback result**: The forced failure does not call the exhaustive tracer.

### Trace-Admission Coverage Checkpoint

- **Status**: Class A does not cover the rapid seed fixture or `PVC-004`.
- **Permanent test**: `trace_admission_classifies_seed_and_pvc_004_without_rejecting_asset_construction`.
- **Rapid source**: `SEED_DOMAIN_WALL_VELOCITY_BITS` supplies the left wall.
- **Rapid right wall**: Each sample is the exact negation of the left-wall sample.
- **Rapid outer radius**: 0.08250592249883855 meters.
- **Rapid inner radius**: 0.060325 meters.
- **Rapid speed**: 33.333333333333336 revolutions per minute.
- **Rapid sample rate**: 192000 hertz.
- **Rapid cut**: The fixture uses the default cut and an 18-micrometer sphere.
- **Rapid class**: `FixedCapPiecewise`.
- **Rapid maximum signed wall curvature**: 270051.552 per meter.
- **Rapid maximum absolute wall slope**: 0.491673.
- **Rapid runtime support**: One wall trace examines at most 28 source-frame spline pieces.
- **Rapid aggregate count**: 950 pieces across two walls and five representation levels.
- **PVC-004 source**: `pvc_004_inner_groove_sine()` supplies `spatial_asset_for_wall`.
- **PVC-004 class**: `FixedCapPiecewise`.
- **PVC-004 maximum signed wall curvature**: 236652.492 per meter.
- **PVC-004 maximum absolute wall slope**: 0.440791.
- **PVC-004 runtime support**: One wall trace examines at most 28 source-frame spline pieces.
- **PVC-004 aggregate count**: 40950 pieces across two walls and five representation levels.
- **Clamp result**: Neither fixture has a C1 record clamp.
- **Control result**: Flat, clamped-flat, and tapered 3-kilohertz data pass class A.
- **Requirement**: Keep both realistic assets constructible when class A does not apply.
- **Requirement**: Use the bounded class-B tracer for active height-order decisions.

Run the coverage test:

```sh
cargo test --lib physical::rapid_scratch_reference::trace_admission_classifies_seed_and_pvc_004_without_rejecting_asset_construction -- --exact --nocapture
```

### Trace-Ingress Adversarial Cases

The WASM facade has one fixed staging buffer.

The former facade permitted one reservation for each page slot.

Therefore, two slots could receive the same pointer at the same time.

The second write could replace the first slot's uncommitted samples.

The correction permits only one global WASM staging reservation.

The permanent test uses two live page slots.

The second reservation must return `ChunkReservationActive`.

After commit, the facade copies samples into private cache storage.

A stale JavaScript view can then change only the staging buffer.

Precomputed spatial levels now have a separate canonical-content test.

A supplied level can be finite and trace-admissible without being canonical decimation.

A page hash proves identity for those supplied bytes.

It does not prove that the canonical filter produced those bytes.

The adversarial page changes one spatial level.

It also recomputes the trace certificate and page identity.

The cache rejects that page as `NoncanonicalSpatialPyramid`.

The comparison uses fixed work and leaves the page invisible.

Class-B certificates must contain the four exact runtime capacities.

The certificate validator rejects zero and canonical-minus-one values.
This rejection also applies after a caller refreshes the certificate digest.

Full and incremental certification produce equal certificates and work totals.
The test covers unaligned interior pages and both record boundaries.

The test uses one-unit and varied work-budget partitions.

A rejection-class certificate cannot enter the real-time `Ready` state.
This rule applies to expected certificates and Rust-owned raw ingress.

The failure is `TraceAdmissionNotAdmitted`.
It occurs before hashing, seam validation, or publication.

The resident manifest has a separate claim limit.

It binds resident ranges, page identities, and certificate identities.

It does not bind every page in one authoritative full-record catalog.

Until that catalog exists, the page producer remains an explicit trust boundary.

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
- SHA-256: `4b016372341b86e4c9cfd47d07a3a4a407f9f8ed942723cd10172974f7536e92`
- `tests/fixtures/pvc_001_catmull_rom_global_envelope.json`
- SHA-256: `c2af509dcd93b303d877e51369fb8e6e42fc8d0e5a55e3e20384ceddf30d5528`

### Correction Status

- **Production fix**: The bounded global tracer passes both exact fixtures.
- **Permanent replacement tests**: Implemented.
- **Outward interval oracle**: Implemented for uniform Catmull-Rom test data.
- **Page-bound certificate**: Implemented for contiguous, paged, and real-time paged representations.
- **Fixed-work admission**: Implemented for strict-concavity and fixed-cap piecewise classes.
- **Canonical pyramid check**: Implemented with bounded, bit-exact recomputation.
- **Rapid-contact comparison**: Implemented as an offline reduced-system stress test.
- **Correction gate**: The numerical and admission correction passes.
- **Product gate**: Device callback limits and the full-record catalog remain open.

### Uncertainty and Contrary Evidence

- The primary case does not prove cutter-head, lacquer, plating, pressing, or PVC feasibility.
- A short source cannot test adjacent-turn clearance.
- The public sample-closure helper is not an admitted product source.
- The production sources require a private validated admission token.
- A correct result for this case will not prove correctness for all groove data.

The accepted numeric-domain stress fixture now returns a certified result.

The result lies inside the outward height and position enclosures.

A 1,048,576-point dense search corroborates the result.

The rapid-scratch reference fixture now completes without a trace error.

Its passing result does not prove the callback deadline.

The certificate rejects unsupported work, slope, joins, geometry, and stale representation data.

The correction applies at each current production trace boundary.

The page producer remains trusted until an authoritative full-record catalog exists.

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

## PVC-006: Passive Cartridge Magnetic-Loss Invariants

### Claim Under Test

A frequency-dependent coil-loss network can preserve passivity, reciprocity, fixed work, and exact zero-network behavior.

### Exact Fixture

- **Observation date**: 2026-08-02.
- **Sample rate**: 192000 hertz.
- **Declared self-inductance**: 0.850 henry.
- **First relaxation inductance**: 0.020 henry.
- **First loss resistance**: 4000 ohms.
- **Second relaxation inductance**: 0.180 henry.
- **Second loss resistance**: 1800 ohms.
- **Residual self-inductance**: 0.650 henry.
- **Unused slots**: Two zero-valued slots.
- **Energy-test coupling ratios**: -0.72, 0.0, and 0.72 of the residual self-inductance.
- **Energy-test length**: 20000 alternating rapid reversals for each coupling ratio.

The two active relaxation times are strictly increasing.

### Network

The coil has a residual coupled inductor in series with direct-current coil resistance.

Each active relaxation section contains a resistor in parallel with an inductor.

The self impedance is:

```text
Z(s) = Rdc + s * (L0 - sum(Lk)) + sum(Rk * s * Lk / (Rk + s * Lk))
```

The signed mutual term is `s * M` in the off-diagonal entries.

Validation requires both residual modal inductances to remain positive.

### Zero-Network Result

`zero_magnetic_loss_slots_are_bit_identical_to_the_legacy_coupled_solver` uses 10000 samples.

The test compares each affine bias and matrix coefficient with `f64::to_bits`.

All compared values are bit-identical to the `6791e90` equations.

The existing uncoupled 10000-sample end-to-end bit test also passes.

### Energy and Reciprocity Result

`magnetic_loss_network_closes_the_exact_discrete_energy_balance` covers all three coupling ratios.

For each sample, the test compares stored-energy change with interval port energy and all resistor losses.

The accepted residual is:

```text
8e-11 * max(abs(energy change), abs(port energy), 1e-30) + 3e-27 J
```

`magnetic_loss_network_is_reciprocal_in_the_same_sample_without_allocation` covers 10000 coupled steps.

It checks generator power against the opposite mechanical reaction power.

It also checks that the coupled processing path allocates no memory.

### Frequency-Domain Result

`passive_magnetic_loss_impedance_has_the_required_low_and_high_frequency_limits` checks the limiting impedance.

At low frequency, the direct inductance approaches 0.850 henry.

At high frequency, the direct inductance approaches 0.650 henry.

At high frequency, both active loss resistances add to the direct coil resistance.

The test also checks positive common-mode and differential-mode resistance and reactance.

`magnetic_loss_transfer_matrix_agrees_with_the_complex_circuit_equations` checks five frequencies.

`time_domain_magnetic_loss_response_matches_the_warped_complex_transfer` checks 1000 and 12000 hertz.

The time-domain comparison includes the trapezoidal rule's bilinear frequency warp.

### State and Configuration Result

`magnetic_loss_json_snapshot_and_rollback_preserve_all_branch_state` covers JSON continuation and failed-step rollback.

`magnetic_loss_configuration_enforces_bounds_slots_and_canonical_order` checks incomplete, gapped, unordered, and excessive networks.

`magnetic_loss_realtime_steps_allocate_no_memory` checks the active cartridge path.

The seed profile keeps all four branch slots at zero and marks them `Estimated`.

Each branch parameter requires `CartridgeMagneticLossLevelAndTemperature` calibration evidence.

### Test Results

Debug results:

- Cartridge tests: 32 passed.
- Electromechanical tests: 11 passed.
- Profile tests: 21 passed.

Release results:

- Cartridge tests: 32 passed.
- Electromechanical tests: 11 passed.
- Profile tests: 21 passed.

### Reproduction

Run the focused debug tests:

```sh
cargo test physical::cartridge::tests -- --nocapture
cargo test physical::electromechanical::tests -- --nocapture
cargo test physical::profile::tests -- --nocapture
```

Run the focused release tests:

```sh
cargo test --release physical::cartridge::tests -- --nocapture
cargo test --release physical::electromechanical::tests -- --nocapture
cargo test --release physical::profile::tests -- --nocapture
```

### Artifacts

- **Commit**: `49cd9424b129d79ee7d3ec40f8d5bf11543e30a2`.
- **Cartridge source SHA-256**: `682f0e1832c49772e3c5f9e2669550b0f6a7b8bea2b6962e8d5bba58e2e25d5d`.
- **Electromechanical source SHA-256**: `3c54358f987934f1edc7820a4ec051162a1c83445b6ef255607f56361da38365`.
- **Profile source SHA-256**: `1919b02933869b5a200a535ca4a612e86e0f4c3779000610d071b47b4a6ad903`.

### Claim Limit

The tests prove the implemented network's numerical and passive invariants.

They do not identify real Concorde magnetic-loss values.

The network does not model saturation, nonlinear hysteresis, temperature drift, or channel-asymmetric loss.

The four-pole limit can omit additional magnetic modes.

Loaded response alone cannot separate coil loss from cable and preamplifier loading.

Hardware measurements remain necessary before a calibrated cartridge claim.

## PVC-007: Passive Groove Compliance and Coupled Contact

### Status

- **Observation date**: 2026-08-02.
- **Implementation status**: Open.
- **Validation status**: The model contract and rejected alternatives are registered.
- **Claim status**: No calibrated groove-compliance or multiple-patch claim is permitted.

### Current Production Reduction

`StylusTraceContactSet` stores as many as eight positions for each wall.

The production mechanics still solves one normal multiplier for each wall.

It averages same-wall slopes and normal-force scales.

It then divides one wall multiplier equally between retained positions.

That equal division is a minimum-norm arithmetic rule.

It is not an identified contact-pressure law.

The solver has no candidate height interval, gap interval, footprint, or maximum-penetration certificate.

The solver also has no material state at an absolute groove coordinate.

Therefore, a reversal cannot recover deformation from an earlier visit.

### Contact Port and Complementarity

For wall `w`, let `n_w` be its inward 45-degree normal.

Candidate `i` has rigid sphere-center threshold `h_i` and along-groove slope `p_i`.

Let `y_w = n_w dot q_tip`.

Let positive `u_i` mean wall recession or indentation.

Use the projected wall multiplier `lambda_i` as the material force port.

The unilateral contact equations are:

```text
g_i = y_w - h_i + u_i
lambda_i >= 0
g_i >= 0
lambda_i * g_i = 0
```

For active contact, `u_i = h_i - y_w`.

The material power is `lambda_i * u_dot_i`.

Candidate slope `p_i` supplies the opposite record and stylus generalized forces.

The complete discrete power balance must include the material port.

### Registered Passive Material Topology

Use a generalized Kelvin creep realization on a groove-coordinate material field.

Let `B(s)` map candidate patch forces into fixed groove cells.

Define the cell load as:

```text
f = B(s) * lambda
```

Use one instantaneous compliance matrix `C_0`.

Use a fixed number of retardation branches `C_r` and `tau_r`.

```text
d_0 = C_0 * f
tau_r * x_dot_r + x_r = C_r * f
d = d_0 + sum(x_r)
u = transpose(B) * d
```

Require `tau_r > 0`.

Require symmetric positive-semidefinite `C_0` and `C_r` matrices.

Store each matrix through a positive-semidefinite factor or a certified reciprocal kernel.

The fitted creep compliance is:

```text
J(s) = C_0 + sum(C_r / (1 + s * tau_r))
```

This topology has finite recoverable static compliance `J(0)`.

Reject a fit that requires a negative branch.

Do not add an unmeasured Hertz spring to repair a rejected fit.

### Measured Nonlinear Elastic Extension

A recoverable static penetration curve can be nonlinear without plastic memory.

For one qualified patch, let measured equilibrium penetration be `u_eq(lambda)`.

After fitting positive retardation weights, define:

```text
u_0(lambda) = u_eq(lambda) - sum(C_r) * lambda
```

Accept this curve only when `u_0(0) = 0`.

Also require `du_0 / d(lambda) > 0` across the declared load range.

Then the elastic force derives from a convex stored-energy potential.

The retardation branches continue to supply nonnegative loss.

At preload `lambda_0`, the small-signal compliance is:

```text
du_0 / d(lambda) at lambda_0
  + sum(C_r / (1 + s * tau_r))
```

A fixed monotone piecewise-linear curve can provide bounded nonlinear elasticity.

Split a sample at each crossed breakpoint.

Alternatively, use a discrete-gradient force that closes the registered energy identity.

For multiple patches, require a measured convex complementary potential `Psi_0(f)`.

Its Hessian must be positive semidefinite and reciprocal.

Reject a fit when relaxation subtraction makes the instantaneous curve non-monotone.

Keep permanent deformation in a separate later model.

### Exact Trapezoidal State Update

Let `h = dt / 2`.

Let `alpha_r = h / tau_r`.

Let `beta_r = alpha_r / (1 + alpha_r)`.

The endpoint branch update is:

```text
x_r,n = (1 - 2 * beta_r) * x_r,p
      + beta_r * C_r * (f_p + f_n)
```

For a fixed contact map, condense endpoint material compliance into:

```text
A = transpose(B) * (C_0 + sum(beta_r * C_r)) * B
```

The previous branch state supplies an explicit gap bias.

Solve this bias and `A` inside the same normal complementarity step.

Do not decay the state first and clamp a tensile result later.

### Discrete Energy Identity

On each active positive-semidefinite subspace, stored energy is:

```text
E = 0.5 * transpose(f) * C_0 * f
  + sum(0.5 * transpose(x_r) * inverse(C_r) * x_r)
```

The required trapezoidal identity is:

```text
transpose(f_bar) * (d_n - d_p)
  = E_n - E_p
  + sum((tau_r / dt)
      * transpose(dx_r) * inverse(C_r) * dx_r)
```

The final sum must be nonnegative.

Divide this loss energy by `dt` to report loss power.

Complementarity must select `lambda = 0` for a tensile trial force.

The retardation state must then recover under zero load.

The same sample can reactivate contact when recovery closes the gap.

### Moving Footprint Requirement

A moving contact changes `B`.

Exact power then includes work from `B_n - B_p`.

That work contributes to the record and stylus tangential reaction.

The first implementation can freeze midpoint geometry only with a work-conjugate discrete map.

The validation must measure the residual from this frozen map.

A scalar filter attached to the stylus moves memory with the stylus.

It does not represent groove material during a reversal or revisit.

### Candidate Admission Contract

Define measured maximum penetration as `delta_max`.

The tracer must retain every candidate within `delta_max` of the certified rigid global height.

Each retained row must include these values:

- Absolute groove-material coordinate.
- Stable wall identity.
- Ordered contact-position interval.
- Certified height interval.
- Certified gap interval.
- Slope bounds.
- Curvature or footprint data.

The certificate must prove that every omitted candidate is farther than `delta_max`.

The certificate must also bind the fixed candidate capacity.

Reject force, pressure, indentation, uncertainty, or candidate count outside the measured domain.

Do not truncate a candidate set.

Keep `PVC-004` rejected until the compliant multiple-contact gate passes.

### Coupled Patch Requirement

The groove wall is one continuous body.

Nearby footprints require reciprocal cross-compliance `C_ij`.

Independent springs can count the same deforming volume more than once.

Do not enable independent local springs as calibrated multiple-patch physics.

Do not average candidate slopes.

Do not split force equally.

Use each candidate's multiplier, slope, footprint, friction vector, and cross terms.

Merge columns only when a certificate proves that they represent one equivalent contact.

Report only the unique resultant when the measured operator remains rank deficient.

Do not report an arbitrary multiplier division as pressure.

### Fixed-Cap Solve

The frictionless endpoint system has the form:

```text
g = q + W * lambda
```

`W` contains the mechanical Delassus compliance and material matrix `A`.

Positive-semidefinite material matrices make this a monotone linear complementarity problem.

A positive instantaneous contact compliance can make the result unique.

Material memory and cross-coupling can produce non-prefix active sets.

Therefore, the proposed `N + 1` height-prefix shortcut has no current proof.

Use a deterministic fixed-cap principal-pivot or active-set solver.

Alternatively, exhaust every active set within a declared small capacity.

Sliding and sticking remain mixed complementarity modes.

Candidate-specific friction can remove matrix symmetry.

Verify passivity for every accepted mode.

### Groove-Coordinate State

Add a per-player `GrooveDeformationState`.

Keep it separate from immutable cut geometry and irreversible damage.

Key each cell by source identity, generation, wall, and canonical absolute groove coordinate.

Record direction and stylus position must not move the state.

Forward and reverse visits to one cell must see the same state.

Separate player instances must not share deformation.

Use fixed page-aligned slots and a fixed relaxation count.

Use fixed kernel support and a fixed contact capacity.

Store branch states, prior distributed load, last-update sample, slot identity, and energy.

Do not silently evict a cell with recoverable energy.

Reject a profile when measured recovery exceeds its bounded residency horizon.

Snapshots must store all material identities and states.

Rollback must prepare changes in a fixed journal.

Publish the journal only after all same-sample systems succeed.

### Friction Vector Counterexample

Let `e_t` be the along-groove unit vector.

The surface normal is proportional to `n_w - p * e_t`.

One longitudinal unit tangent is:

```text
(e_t + p * n_w) / hypot(p, 1)
```

For projected wall force `lambda`, the physical normal magnitude is `lambda * hypot(p, 1)`.

The Coulomb friction magnitude is `mu * lambda * hypot(p, 1)`.

Its along-groove component has magnitude `mu * lambda`.

Its wall-coordinate component has magnitude `mu * lambda * p`.

The former code used the complete Coulomb magnitude as the along-groove force.

At `abs(p) = 0.5`, the record-torque factor is `1.118033988749895`.

The possible torque error is approximately `11.8034` percent.

The former reduction also omitted the wall-coordinate friction component.

This finding does not apply if `mu` is an identified effective horizontal coefficient.

No current measurement establishes that alternative definition.

### Sliding Vector Correction Checkpoint

- **Observation date**: 2026-08-02.
- **Implementation scope**: The correction covers nonzero sliding velocity.
- **Projected normal force**: The contact multiplier is `lambda`.
- **Record-tangent force**: The value is `-sign(v) * mu * lambda`.
- **Wall-coordinate force**: The value is the record-tangent force multiplied by `p`.
- **Local friction power**: The value is `-mu * lambda * (1 + p * p) * abs(v)`.
- **Coupling result**: Both force components are inside the same pickup and deck solve.
- **Force accounting result**: Telemetry uses the complete per-wall resultant.
- **Zero-slope result**: The corrected branch is bit-identical to the former scalar branch.
- **Reversal result**: Both friction components change sign for nonzero forward and reverse velocities.
- **Local passivity result**: Local friction power is negative for both sliding directions.
- **Focused debug suite**: 39 tests pass, zero tests fail, and one test is ignored.
- **Focused release suite**: 39 tests pass, zero tests fail, and one test is ignored.
- **Command**: `cargo test --lib physical::contact::tests`.

### Skating-Port Power Audit

- **Observation date**: 2026-08-02.
- **Status**: Confirmed scope gap and corrected reduced-model coupling.
- **Definition**: Let `K` be the skating-force conversion for one newton of along-groove force.
- **Slip velocity**: `v_slip = v_record - K * v_body_lateral`.
- **Contact velocity**: `gamma_dot_i = v_slip + p_i * v_wall_i`.
- **Correction**: Joint normal rows include the reciprocal `p_i * K` body-velocity term.
- **Correction**: The flat sticking row uses the same power-conjugate arm mapping.
- **Correction**: Sliding direction uses the solved contact velocity.
- **Correction**: Mixed nonzero contact directions reject without state change.
- **Correction**: Telemetry sums record, body, and cross-plane friction power.
- **Snapshot result**: Restore rejects impossible Coulomb modes, signs, and magnitudes.
- **Test**: `midpoint_sliding_closes_record_body_and_cross_plane_coulomb_power`.
- **Test**: `standalone_sliding_uses_the_reciprocal_body_velocity_and_all_force_ports`.
- **Limit**: This result applies to the reduced rigid-longitudinal sliding model.
- **Limit**: It does not prove coupled uniqueness over the complete profile domain.

### Same-Wall Reduction Power Audit

- **Status**: Confirmed limit for the test-qualified multiple-contact path.
- **Finding**: The momentum solve uses the same-wall mean slope `p_bar`.
- **Finding**: The telemetry sums each contact's squared slope.
- **Solve power term**: The reduced term is `f_total * (1 + p_bar * p_bar) * v`.
- **Reported power term**: The detailed term uses `f_i * (1 + p_i * p_i) * v`.
- **Counterexample**: A reflected pair with slopes `+p` and `-p` has `p_bar = 0`.
- **Effect**: Its reduced solve has no wall-friction component, but detailed telemetry reports one.
- **Current protection**: Production rejects same-wall sets without a physical qualification.
- **Requirement**: Keep that rejection until the solve represents each accepted contact port.

The solve needs a strict inward force-direction margin.

For each certified wall, require:

```text
mu * maximum_abs_slope < 1 - 1e-6
```

The current general trace cap permits `maximum_abs_slope = 16`.

The default friction coefficient is `0.25`.

Therefore, the general cap and default coefficient are not compatible by themselves.

The player must use each certificate's actual maximum absolute slope.

The rapid fixture product is approximately `0.12291825`.

The `PVC-004` product is approximately `0.11019775`.

Both products are inside the required margin for the default coefficient.

This scalar test is necessary for the local wall-force direction.

It is not a sufficient coupled Painlevé or uniqueness proof.

The coupled normal response is the Delassus matrix `W = J * A^-1 * G`.

The response also depends on skating geometry, masses, damping, deck inertia, and both contacts.

The accepted profile domain needs a P-matrix proof or an equivalent bounded proof.

### Confirmed Coupled Normal-Response Counterexample

- **Observation date**: 2026-08-02.
- **Status**: The canonical production operator confirms that the scalar gate admits a negative minor.
- **Timestep**: Use `1 / 192000` seconds.
- **Record inertia**: Use `1e-7` kilogram square meters.
- **Stylus moving mass**: Use `0.01` kilograms.
- **Friction coefficient**: Use `0.25`.
- **Wall slope**: Use `-0.125`.
- **Groove radius**: Use `0.14605` meters.
- **Groove pitch**: Use `125e-6` meters per revolution.
- **Tonearm**: Use the default tonearm geometry and axis values.
- **Generator coefficient**: Use a small valid positive value, such as `1e-12` volt seconds per meter.
- **Deck mode**: The deck bearing and slipmat slide.
- **Hand mode**: The hand is separated.
- **Pickup mode**: The pickup bearing sticks.
- **Tangential mode**: The stylus uses positive sliding.
- **Scalar admission**: `mu * abs(p)` is `0.03125`.
- **Deck contribution**: The value is `-0.007866686227009534`.
- **Pickup contribution**: The value is `0.000536859604648429`.
- **One-wall minor**: `W_00` is `-0.007329826622361105`.
- **Confirmed conclusion**: The one-contact Delassus operator is not positive for this witness.
- **Possible physical effect**: A one-dimensional complementarity problem can have two solutions or no solution.
- **Canonical builder**: Production and the witness use the same midpoint `H` and `G` coefficients.
- **Order protection**: Separate types identify equation-row and velocity-column coordinate orders.
- **Signed-zero test**: `contact_operator_writers_preserve_the_signed_zero_layout` protects bit-level assembly parity.
- **Permanent test**: `admitted_midpoint_configuration_has_a_negative_normal_minor` reproduces `W_00`.
- **Former mode count**: The 48 signed families cover only groove-wall sliding.
- **Mechanical basis**: There are 24 nominal mobility classes after kinetic signs share one left-hand side.
- **Land addition**: Record-land sliding adds 48 labeled families.
- **Sticking addition**: Groove and land sticking add 24 labeled families for each surface.
- **Boundary addition**: Held-boundary groove contact has a different spiral-origin row.
- **Separated addition**: The solver also evaluates separated labels for active masks.
- **Groove catalog**: Four stylus labels, two origin laws, and 24 mechanical classes give 192 families.
- **Land catalog**: Four stylus labels and 24 mechanical classes give 96 families.
- **Contacting total**: The structurally different labeled catalog contains 288 families.
- **Mask total**: Principal-minor coverage gives 768 structurally distinct mode-mask families.
- **Runtime maximum**: One hand-active groove sample evaluates 1,053 current branches.
- **Registered bound**: The 1,296 bound is safe, but it is not the exact current fallback count.
- **Implementation status**: Production code does not yet generate the versioned certificate catalog.
- **Required proof**: Prove both diagonal minors and the determinant for every family.
- **Required domain**: Bind radius, both slopes, pitch, tonearm geometry, damping, profile, source, and generation.
- **Required failure**: Reject an uncertified operator before any state change.
- **Current uncertainty**: The test confirms one point. It does not yet certify a bounded source domain.
- **Required method**: Use outward interval bounds over radius, slope, and every configuration-dependent coefficient.

### Fixed-Mode and Hybrid Uniqueness Scope

- **Fixed-mode theorem**: A P-matrix gives one solution for every right-hand side of one fixed LCP.
- **Scope limit**: Different fixed systems can each have one solution and still overlap.
- **Exact production overlap**: Set the stylus friction coefficient to zero.
- **Contact state**: Use a loaded contact with positive relative slip.
- **Sliding result**: `SlidingPositive` uses the zero-friction normal operator.
- **Separated result**: `Separated` uses the same augmented system and passes its current force check.
- **Consequence**: Both mode labels can describe the same accepted mechanical state.
- **Additional overlap**: Static limits above kinetic force can make stick and slide branches both feasible.
- **Deck witness start**: Set platter and record velocity to zero.
- **Deck witness control**: Separate the hand and stylus, and make the slipmat stick.
- **Deck witness torque**: Apply `0.00020 N m` of motor torque.
- **Default bearing limits**: Kinetic torque is `0.00018 N m` and static torque is `0.00024 N m`.
- **Sticking result**: Zero platter speed and `0.00020 N m` bearing torque pass the static limit.
- **Sliding result**: Positive sliding gives `3.88313554466e-9 rad/s`.
- **Sliding slipmat torque**: The result is `1.50976309976e-6 N m`.
- **Consequence**: Both branches pass and produce different next states.
- **Other overlaps**: The slipmat, hand, and pickup bearing also have static limits above kinetic limits.
- **Mask witness**: Let one free gap be `-delta`, where `0 < delta <= 1e-11 m`.
- **Inactive result**: The inactive mask accepts this penetration through `CONTACT_TOLERANCE_M`.
- **Active result**: The active mask can close the gap with a positive normal force.
- **Negative-force case**: The solver accepts forces down to `-1e-10 N` and clamps them to zero.
- **Theorem mismatch**: These tolerance bands are not the exact complementarity law used by the P-matrix theorem.
- **Current selection**: Branch order and prior-state hints select the first accepted mode.
- **Interpretation**: This is a deterministic algorithmic selection law.
- **Claim limit**: It is not evidence for one unique physical hybrid mode.
- **Requirement**: Add a constitutive transition law or prove that valid mode interiors cannot overlap.
- **Alternative**: Form one global mixed complementarity problem and prove a global uniqueness property.
- **Alternative**: Evaluate all valid candidates and reject materially different results.
- **Common requirement**: Use outward-certified guards and define redundant static-force selection.
- **Reference**: The fixed-LCP P-matrix result is at <https://doi.org/10.1137/0120041>.
- **Reference**: Rigid frictional contact limits are reviewed at <https://arxiv.org/abs/1601.03545>.

### Midpoint Constraint-Rank Counterexample

- **Former projection**: Hand sticking and stylus sticking both used the deck vector `[0, 1]`.
- **Production stylus row**: The full row also contains `-2 * K / r` times lateral body velocity.
- **Former effect**: The selector could reject unequal targets before a solve.
- **Former effect**: Equal targets could remove a physically independent stylus constraint.
- **Correction**: Keep both rows when the pickup bearing slides and `K` is nonzero.
- **Dependent case**: Deduplicate the rows when the pickup bearing sticks or `K` is zero.
- **Permanent rank test**: `hand_and_stylus_sticking_keep_the_independent_body_constraint` covers both cases.
- **Permanent branch test**: `joint_branch_enforces_independent_and_dependent_sticking_equalities` covers both cases.

### Coupled Contact Certificate Design Record

- **Owner**: `PhysicalRecordPlayer` must own the validated certificate with the loaded source.
- **Exclusion**: Do not store this profile-dependent proof in the profile-independent trace certificate.
- **Config binding**: Hash every exact `PhysicalPlaybackConfig` field with versioned tags.
- **Source binding**: Bind the complete `PhysicalGrooveSourceIdentity` and current generation.
- **Domain binding**: Bind radius bounds, slope bounds, origin regime, surface, and family catalog.
- **Algorithm binding**: Bind certificate, operator, family-set, and numerical-solver versions.
- **Margins**: Store lower bounds for both diagonal minors and the determinant.
- **Land margin**: Store the one-dimensional land response lower bound.
- **Numerical margin**: Store a solve-conditioning margin in addition to physical minors.
- **Load gate**: Build the proof before `load_source` changes player state.
- **Replacement gate**: Build a prospective proof before a paged cache replacement commits.
- **Publication gate**: Realtime page publication must update source identity and proof atomically.
- **Render gate**: Compare cached source and certificate identities once for each render block.
- **Sample gate**: Check only radius and slope enclosure bounds in the sample loop.
- **Snapshot gate**: Bind and verify the certificate identity before restore changes state.
- **Thread rule**: Run interval subdivision and family enumeration outside the audio thread.
- **Realtime limit**: Resident-page slope maxima do not bound an unpublished whole record.
- **Requirement**: Add an authoritative whole-record domain or certify each publication transactionally.

### Verified Mobility Proof Plan

- **Observation date**: 2026-08-02.
- **Status**: Reviewed design. Production admission does not use it yet.
- **Base systems**: Build one exact KKT system for each of 24 mechanical mobility classes.
- **Base verification**: Replay the production pivot schedule with outward intervals.
- **Pivot rule**: Each scaled pivot interval must exclude zero and pass the production tolerance.
- **Mobility definition**: Store response as velocity rows by equation columns.
- **Equation order**: `[platter, record, tip-x, body-x, tip-z, body-z]`.
- **Velocity order**: `[platter, record, tip-x, tip-z, body-x, body-z]`.
- **Permutation test**: Use six distinct values to detect exchanged coordinates.
- **Contact evaluation**: Evaluate the shared `H` and `G` algebra over radius and slope boxes.
- **Skating algebra**: Production uses only arithmetic and square root for the force factor.
- **Skating domain**: Require `-1 < c < 1` and `1 - c * c > 0`.
- **Endpoint result**: Both program-radius factors are within one ULP of 100-digit calculations.
- **Bit change**: The outer result changed by one ULP. The inner result changed by four ULP.
- **Boundary result**: Exact geometric reach boundaries reject as unreachable.
- **Permanent test**: `algebraic_skating_factor_matches_independent_references` checks 257 radii and three high-precision results.
- **Sticking update**: Use the production tangential force column and equality row.
- **Sticking proof**: Require the nonsymmetric Schur denominator interval to exclude zero.
- **Catalog**: Generate 288 contacting labels from production mode mappings.
- **Solve-only addition**: Prove 24 lowered and 24 cue-supported systems.
- **Initial record count**: Prove 336 records before safe equivalence aggregation.
- **Groove proof**: Require positive lower bounds for both diagonals and the determinant.
- **Land proof**: Require a positive lower bound for its scalar response.
- **Subdivision**: Split the largest normalized radius or slope width when a result is inconclusive.
- **Work cap**: Fail closed when the fixed box or depth limit is exhausted.
- **Arithmetic**: Expand round-to-nearest results with adjacent finite `f64` values.
- **Arithmetic status**: The finite outward interval core is implemented.
- **Supported operations**: It includes add, subtract, multiply, divide, square, square root, negation, and interval queries.
- **Finite rule**: An operation rejects if its outward result would require an infinite bound.
- **Identity rule**: Exact zero, one, and negative-one operations preserve maximum finite operands.
- **Concurrency rule**: Do not change the process rounding mode.
- **Exact oracle**: Decode finite `f64` values into exact dyadic rationals in tests.
- **Square-root oracle**: Verify each returned bound by exact rational squaring.
- **Random oracle**: Check 4,096 deterministic normal and subnormal bit-pattern cases.
- **Focused result**: Thirteen interval tests pass with no failure.
- **Build result**: Workspace, all-target, and WASM checks pass.
- **Conditioning gate**: Require dimensionless solve margins above `6.4e-9` initially.
- **Failure classes**: Distinguish bad minors, singular mobility, singular Schur updates, weak margins, and exhausted work.
- **Scope**: The result proves fixed-mode normal-contact uniqueness only.
- **Exclusion**: It does not prove unique selection across overlapping hybrid modes.

### Playback Configuration Identity Checkpoint

- **Status**: Implemented as a non-gating building block.
- **Identity version**: `1`.
- **Leaf count**: The identity binds all 84 validated playback manifest leaves.
- **Field encoding**: Each leaf includes its stable path and explicit value-type tag.
- **Float encoding**: Each floating-point value contributes its exact `f64` bits.
- **Text encoding**: Each enum value uses a length-delimited byte string.
- **Hash protocol**: The protocol uses a versioned, domain-separated SHA-256 hash.
- **Rejected protocol**: The implementation does not hash JSON text.
- **Validation rule**: An invalid playback configuration cannot create an identity.
- **Permanent test**: Every manifest leaf changes the identity after one controlled value change.
- **Permanent test**: Field or value-type tag changes cause a manifest mismatch.
- **Permanent test**: Serialization and restoration preserve the complete identity.
- **Current limit**: The player does not bind this identity to a contact certificate yet.

This result concerns the reduced rigid sliding model.

It does not resolve sloped sticking or measured material behavior.

### Exact-Zero Sloped Sticking Blocker

- **Status**: Confirmed model gap.
- **Production behavior**: The current implementation returns `UnsupportedGrooveWallSticking`.
- **Transaction result**: The failed step does not change state or allocate memory.
- **Physical reason**: Static friction has one unknown traction for each loaded wall.
- **Constraint count**: The reduced model has only one global along-groove sticking constraint.
- **Consequence**: The static wall-traction distribution is not unique.
- **Rejected rule**: Do not split traction in proportion to projected normal force.
- **Rejected rule**: Do not use a minimum-norm traction split.
- **Reason**: Neither rule is an identified material law.
- **Additional limit**: One new longitudinal pickup coordinate is not sufficient by itself.
- **Requirement**: Add local tangential compliance or another measured history law for each contact.
- **Requirement**: Add an along-groove pickup coordinate and velocity.
- **Requirement**: Store tangential state at stable groove coordinates.
- **Requirement**: Reconcile contact identities when the traced contact set changes.
- **Requirement**: Keep stored-energy, loss, snapshot, rollback, and fixed-work accounting exact.
- **Calibration requirement**: Fit stiffness, loss, and static yield from vector reversal measurements.
- **Claim limit**: Rapid scratch fidelity remains open until exact-zero reversals pass this model.

### Unique-Contact Tangential-State Hypothesis

- **Status**: Working design hypothesis.
- **Confidence**: Medium for passivity and identifiability.
- **Scope**: Permit one uniquely certified contact on each wall.
- **Exclusion**: Keep same-wall multiple-patch playback closed.
- **Reason**: The current certificate does not bind footprint, cross-compliance, gaps, or maximum penetration.

Add one power-conjugate along-groove pickup coordinate `x_t`.

Also add its velocity `v_t` and suspension deflection `d_t`.

Let `rho` be the groove radius.

Let `kappa(rho)` map lateral arm-body velocity to along-groove body velocity.

Freeze `kappa(r)` for one coupled sample.

Then:

```text
d_dot = v_t - kappa(r) * v_body_lateral
F_s = -k_t * d - c_t * d_dot
```

Put `F_s` in the along-groove tip equation.

Put `-kappa(r) * F_s` in the lateral body equation.

This force pair must close the suspension power port.

Do not reuse the lateral suspension parameters without measurements.

For contact `i`, let `r` be the interval slip velocity.

The current deck uses trapezoidal record velocity.

The pickup uses endpoint backward Euler velocity.

Therefore, use:

```text
r = 0.5 * (v_record,previous + v_record,new) - v_t,new
```

Use the same interval slip for travel, force, work, and material identity.

Let `p_i` be the certified groove slope.

Let `f_i` be the friction force on the record along the groove.

The active normal relation is:

```text
v_wall_i = p_i * r
```

Define generalized tangential deformation by:

```text
gamma_dot_i = (1 + p_i * p_i) * r
```

The candidate force components are:

```text
tip wall-coordinate force = lambda_i + p_i * f_i
tip along-groove force = p_i * lambda_i - f_i
record along-groove force = -p_i * lambda_i + f_i
```

The normal and modulation power terms then cancel.

The remaining contact power is `f_i * gamma_dot_i`.

### Proposed Passive Return Map

Use one Jenkins element for each uniquely certified wall contact.

This element combines an elastic tangential spring with a Coulomb slider.

Store generalized elastic displacement `z_i`.

Require measured stiffness `k_i > 0`.

For one endpoint update:

```text
delta_gamma_i = dt * (1 + p_i * p_i) * r_n
z_trial = z_previous + delta_gamma_i
f_trial = -k_i * z_trial
```

Use the elastic branch when `abs(f_trial) <= mu * lambda_i`.

Then set `z_i = z_trial` and `f_i = f_trial`.

For positive sliding, set `f_i = -mu * lambda_i`.

For negative sliding, set `f_i = mu * lambda_i`.

Set `z_i = -f_i / k_i` on either sliding branch.

Validate the plastic-increment sign before commit.

Use one measured coefficient for yield and sliding in the first model.

Separate static and kinetic coefficients need an identified release law.

An instantaneous unmeasured force drop can destroy stored energy.

For stored energy `E_i = 0.5 * k_i * z_i * z_i`, backward Euler gives:

```text
f_i * delta_gamma_i
  = -(E_i,new - E_i,previous)
    - 0.5 * k_i * delta_z_i * delta_z_i
    - plastic_loss_i
```

Each accepted branch must have nonnegative numerical and plastic loss.

Define `delta_z_i = z_i,new - z_i,previous`.

Define `delta_gamma_pl_i = delta_gamma_i - delta_z_i`.

Define `plastic_loss_i = -f_i,new * delta_gamma_pl_i`.

Positive sliding requires `delta_gamma_pl_i >= 0` and `f_i = -mu * lambda_i`.

Negative sliding requires `delta_gamma_pl_i <= 0` and `f_i = mu * lambda_i`.

### Fixed-Work Tangential Modes

Enumerate `Elastic`, `SlidingPositive`, and `SlidingNegative` for each wall.

Two loaded walls give at most nine tangential combinations.

The former Cartesian estimate gave 2,916 candidates.

Mask-aware enumeration gives 1,296 candidates when all hand modes are active.

The four wall masks contribute `1 + 3 + 3 + 9 = 16` material-mode combinations.

Recalculate this cap after the final compliance operator exists.

Validate these properties before commit:

- Each normal multiplier is nonnegative.
- Each elastic force stays inside its yield limit.
- Each plastic increment has the selected sliding sign.
- Force and torque remain reciprocal.
- Stored energy is nonnegative.
- Loss is nonnegative.
- The solve count does not exceed the registered cap.

### Tangential Material Identity

Do not key material state by array order or midpoint floating-point bits.

Return an outward absolute contact-coordinate interval from the certified tracer.

Map this interval into measured canonical material cells.

Use this state key:

```text
(source identity, generation, wall, canonical cell index)
```

Use the same fixed footprint map for deformation and transpose force transfer.

This pairing is necessary for discrete power closure.

Reject an interval that cannot isolate all required cell weights.

Use the typed error `TangentialContactIdentityNotIsolated` for that case.

The certified algorithms now return their outward coordinate intervals.

The result uses an exact `u64` origin and two local `f64` bounds.

This split preserves large origins without one lossy floating-point addition.

Do not reconstruct it from a midpoint, slope, page, or travel direction.

Exclude page identity and representation identity from the material key.

Those identities can change at seams or after immutable cache extension.

Use base spline cells for the first deterministic plumbing slice.

This grid does not claim a measured physical footprint.

Both closed interval bounds must select the same cell.

The final record coordinate belongs to the last real spline cell.

The provisional resolver rejects forged, nonordered, outside-source, and multiple-contact input.

Deserialization removes the private live-trace seal.

Keep the state bank outside `PickupMechanicalState`.

The solver receives only prior values and proposed updates for the two walls.

Commit the updates only after the complete coupled candidate passes.

Use fixed page-aligned or set-associative state slots.

Do not evict a slot that contains recoverable energy.

Return `TangentialStateCapacityExceeded` before any state change.

### Certified Identity Plumbing Checkpoint

- **Observation date**: 2026-08-02.
- **Schema version**: `TANGENTIAL_CONTACT_IDENTITY_VERSION` is `1`.
- **Trace coverage**: Class A, Class B, exhaustive, scalar, and multiresolution paths retain certified bounds.
- **Path coverage**: Immutable pages, real-time pages, player transforms, pickup input, and telemetry preserve the bounds.
- **Large-origin result**: An exactly representable center above `2^53` preserves its `u64` origin.
- **Large-origin limit**: General unrepresentable centers above `2^53` are not supported.
- **Cell rule**: An exact singleton at an internal join selects the right cell.
- **End rule**: Exact frame `N - 1` selects cell `N - 2`.
- **Edge blocker**: Actual outward record-edge intervals still extend outside the source and reject.
- **Seam result**: Page and whole-record traces resolve the same cell at both seam sides.
- **Direction result**: The cell remains equal for advances of `-20` and `+20` frames.
- **Raw-bounds result**: Valid page and whole-record enclosures can have different origins and widths.
- **Test rule**: Compare certified trace limits and the resolved key across representations.
- **Rejected rule**: Do not require raw bound bits to match across different coordinate origins.
- **Allocation result**: Live tracing and key resolution do not allocate memory.
- **Snapshot versions**: Pickup is `5`, player is `10`, and renderer is `4`.

Reproduction commands:

```text
cargo test --lib physical::stylus::tests
cargo test --lib physical::tangential_identity::tests
cargo test --lib physical::paged_groove::tests::bounded_cache_matches_a_monolithic_groove_asset -- --exact
cargo test --lib physical::realtime_paged_groove::tests::traces_match_immutable_pages_at_seams_in_both_directions_and_twenty_times -- --exact
```

### Hard-Cell Liveness Counterexample

- **Input**: Use a flat Class A wall and center the stylus at source frame `64`.
- **Certified result**: The lower bound is below `64`, and the upper bound is above `64`.
- **Cell result**: The interval covers base spline cells `63` and `64`.
- **Resolver result**: The resolver returns `TangentialContactIdentityNotIsolated`.
- **Phase A result**: Rigid playback continues because it does not request a material key.
- **General result**: A continuous trace must cross every finite hard cell boundary.
- **Consequence**: A hard one-cell key cannot guarantee uninterrupted rapid scratching.
- **Rejected workaround**: Do not select a cell from the interval midpoint or travel direction.
- **Requirement**: Add a continuous partition of unity with the same transpose force map.
- **Claim limit**: The current key proves identity plumbing only.

Permanent test:

```text
physical::stylus::tests::class_a_flat_integer_contact_remains_fail_closed_at_a_cell_join
```

### Continuous Material-Map Hypothesis

- **Status**: Design hypothesis only. Production does not use this map.
- **Selected coordinate**: Let `x = k + u`, where `0 <= u <= 1`.
- **Normalization**: Let `d = (1 - u)^2 + u^2`.
- **Left amplitude**: Let `a_0 = (1 - u) / sqrt(d)`.
- **Right amplitude**: Let `a_1 = u / sqrt(d)`.
- **Identity rule**: Derive possible keys from the certified interval only.
- **Weight rule**: Derive amplitudes from a tracer-selected coordinate inside that interval.
- **Boundary capacity**: A narrow interval can require three possible coefficient keys.
- **Reciprocity rule**: Use the same amplitude bits for state scatter and force gather.

The normalized amplitudes satisfy:

```text
a_0^2 + a_1^2 = 1
```

Raw linear amplitudes do not satisfy this condition.

Their effective self stiffness is:

```text
k * ((1 - u)^2 + u^2)
```

This value falls from `k` at a node to `k / 2` at a cell center.

Therefore, raw linear amplitudes are rejected for the uniform Jenkins seed.

Use one scalar Jenkins yield surface for each wall.

For coefficient state `q`, use:

```text
q_trial = q_0 + a * DeltaGamma
f_trial = a dot (-k * q_trial)
DeltaGamma_pl = (f_target - f_trial) / (k * (a dot a))
q_1 = q_trial - a * DeltaGamma_pl
```

The proposed backward-Euler energy identity is:

```text
f * DeltaGamma
  = -(E_1 - E_0)
    - 0.5 * k * norm_squared(Deltaq)
    - D_pl
```

Require `D_pl = -f * DeltaGamma_pl >= 0`.

### Rapid-Sweep Material-State Blocker

- **Admission limit**: One physics sample can advance by `-20` or `+20` source frames.
- **Center-sweep capacity**: A 20-frame move can require at least 23 possible coefficient keys.
- **Root-motion risk**: Contact offset can move within the spherical support during that sample.
- **Branch risk**: The global contact can change envelope branches during that sample.
- **Certificate gap**: No current certificate bounds contact-root travel between physics samples.
- **Consequence**: One midpoint stencil can skip material state during intensive scratching.
- **Requirement**: Add a certified contact-trajectory bound.
- **Requirement**: Use deterministic event substeps or a proved swept return map.
- **Requirement**: Register a fixed work cap for the complete sweep.
- **Reference gate**: Compare against converged microsteps at `+20`, `-20`, stop, and one-sample reversals.
- **Claim limit**: Do not activate Jenkins state until these tests pass.

Cache generation also fails to identify one physical record instance.

Add `GrooveMaterialInstanceId` before state can survive representation changes correctly.

### Tangential-State Persistence Limit

A finite state bank cannot retain energetic cells for an unlimited record.

The model needs a measured recovery law and a bounded residency horizon.

One permitted policy retains every energetic cell until it recovers.

Another policy retires a cell below a registered residual-energy bound.

That policy must add the retired energy to reported loss.

Do not use silent least-recently-used eviction.

Do not attach material history to the moving stylus.

Without recovery measurements, long-play calibrated persistence is not possible.

### Tangential-State Required Tests

- Exact-zero sloped reversal produces unique wall tractions.
- Unequal wall histories produce unequal deterministic tractions.
- The result does not use proportional or minimum-norm splitting.
- Load, hold, reverse, slide, and unload close the energy balance.
- A forward and reverse revisit retrieves the same material state.
- Root reordering and page seams do not swap material state.
- An ambiguous cell map fails without changing state or output.
- Slot exhaustion fails without changing state or output.
- Snapshot restore and render partitioning continue bit-identically.
- Success and failure paths allocate no memory.
- The active-set count stays inside 2,916 candidates.
- The longitudinal natural frequency passes the temporal-resolution gate.

### Tangential-State Required Measurements

1. Measure along-groove moving mass, stiffness, damping, and arm transfer impedance.
2. Measure pre-sliding stiffness against normal load, wall, slope, speed, and temperature.
3. Measure stop and reversal force-displacement hysteresis.
4. Measure yield and sliding traction for the identified PVC and stylus pair.
5. Measure contact footprint, kernel support, and material-cell pitch.
6. Measure unloaded recovery and the residual-energy residency horizon.
7. Measure cross-wall transfer before any coupled two-wall material claim.
8. Measure separated-patch transfer before any same-wall multiple-contact claim.
9. Bound contact-coordinate uncertainty below the material-map tolerance.

The current groove coefficient `0.25` is estimated.

Therefore, a correct tangential-state implementation will still need calibration evidence.

### Non-smooth Normal-Cone Counterexample

A clamp can have zero slope on one side and slope `-0.5` on the other side.

The contact position and height can be unique while the force direction remains non-unique.

The spherical-envelope tangent inside that cone is not a unique physical force direction.

The scalar tracer must return `GrooveSlopeBoundNotMet` for this fixture.

Future mechanics can replace this rejection with explicit nonnegative one-sided multipliers.

### Required Measurements

1. Measure static load, penetration, unloading, and recovery for the identified PVC compound.
2. Record the wall angle, stylus profile, temperature, speed, and preload for each measurement.
3. Identify the recoverable and permanent deformation limits separately.
4. Measure phase-calibrated complex force and velocity impedance across frequency.
5. Repeat the impedance measurement at several preloads, temperatures, and groove speeds.
6. Include forward, reverse, stop, hold, and release motions.
7. Measure the fixture and pickup impedance for de-embedding.
8. Measure two-point and multiple-indenter transfer impedance against separation.
9. Measure contact footprints or pressure when the fixture permits it.
10. Measure transfer between the two 45-degree groove walls.
11. Measure revisit and reversal responses at controlled separations and delays.
12. Measure vector friction against load, speed, direction, temperature, and slope.
13. Perform before-and-after metrology during repeated-pass tests.

### Identification Risks

- Poles outside the measured band can collapse into one aggregate compliance.
- Nearby poles can be strongly correlated.
- Preload and temperature can make a linear fit local.
- Moving speed can confound temporal relaxation with spatial footprint.
- One-point data cannot separate temporal and spatial kernels.
- Height uncertainty can be comparable to indentation.
- Scalar impedance cannot identify patch or cross-wall coupling.
- Permanent deformation cannot use a recoverable branch state.

### Implementation Gates

1. Correct vector friction and state the longitudinal stylus limit.
2. Extend trace certificates with bounded candidates, gaps, and `delta_max`.
3. Keep rigid production behavior unchanged during the certificate extension.
4. Implement one qualified candidate per wall with groove-coordinate passive state.
5. Make zero material branches and `delta_max = 0` bit-identical to rigid contact.
6. Calibrate and validate single-patch compliance inside a declared domain.
7. Enable multiple patches only after spatial transfer measurements identify cross-compliance.
8. Add nonlinear convex elasticity only after measured preload sweeps require it.
9. Keep plastic damage and wear in a separate groove-coordinate overlay.

### Planned Permanent Tests

- `zero_material_branches_and_zero_penetration_are_bit_identical_to_rigid_contact`.
- `groove_material_energy_closes_for_load_hold_reverse_unload_and_revisit`.
- `deformation_stays_with_absolute_groove_coordinates_across_reversal`.
- `candidate_admission_rejects_an_omitted_gap_inside_maximum_penetration`.
- `coupled_patch_compliance_does_not_equal_independent_springs`.
- `friction_vector_closes_force_torque_and_power_for_signed_slopes`.
- `non_smooth_normal_cone_rejects_scalar_contact`.

### Explicit Rejections

- No independent local springs presented as calibrated physics.
- No stylus-attached viscoelastic memory.
- No equal force split presented as pressure.
- No `N + 1` prefix reduction without a theorem for the identified operator.
- No rigid acceptance from overlapping height intervals.
- No silent eviction of material state.
