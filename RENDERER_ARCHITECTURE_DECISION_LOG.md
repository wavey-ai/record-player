# Renderer Architecture Decision Log

## Purpose

This document records the renderer investigation and its product decision.

The decision date is August 2, 2026.

The decision applies to `record-player`, `vin.yl.native`, `vin.yl.app`, `vin.yl.web`, and `vin.yl.player`.

## Product Target

The decoded master is the reference signal.

Stable playback at positive 1× speed must preserve that signal.

The default 1× path must not add wow, flutter, surface noise, tracing loss, cartridge color, or phono color.

This requirement is a product reference. It does not claim that physical records have zero error.

The interaction model must reproduce a high-quality modern turntable during manual record motion.

The user controls record position and motion. A separate crossfader control gates the output.

Scratch technique automation is outside this decision.

## Decision

Do not activate `PhysicalHostRenderer` as the production audio renderer.

Keep `ScratchAcousticDsp` as the only production renderer during the replacement work.

Refactor that renderer around a transparent nominal-speed path and a motion-dependent interaction path.

The product must expose one renderer and one transport state.

The engine can use different internal equations for different physical states.

Do not maintain two competing host renderers.

Move all canonical renderer behavior into `record-player`.

Native Swift code must use `record-player` through the existing thin native interface.

Web code must use the same Rust behavior through WebAssembly.

## Derivation and Efficiency Rule

Prefer the least expensive implementation that produces the required physical result.

Do not add processing only because it represents more simulated components.

Each audible program signal must come from decoded PCM, an explicit audio asset, or measured hardware data.

A physical state can control a signal transform when tests validate the transform.

Generated effects cannot claim hardware authenticity without measured calibration.

Use the offline physical model to calculate references when this method reduces callback work.

Compare the efficient renderer with those references through automated signal tests.

Do not use manual listening as a release gate.

Use captured hardware signals only when automated tests can measure their applicable features.

## Required Signal Model

Treat decoded PCM as the nominal program signal.

The user-controlled record position selects the source position.

Signed record velocity controls time direction, pitch, and motion-dependent output level.

Record acceleration affects output only through a measured tracking, contact, or transport response.

High-speed playback requires direction-symmetric anti-alias filtering.

Stops and reversals must preserve source position and transport continuity.

The crossfader gates the result after the record-motion renderer.

Do not simulate an internal voltage only because physical hardware contains that voltage.

Use a cheaper transfer model when it reproduces the same audible output within the declared error limit.

Use detailed cartridge or contact simulation only to generate references for that transfer model.

Off-speed RIAA behavior can be audible during scratching.

Model that behavior with a measured speed-dependent filter when automated tests show a material error.

Do not require a complete virtual cut and cartridge solve in the audio callback.

Ideal linear 45/45 decoding does not require runtime wall geometry.

Add stereo wall interaction only when measured nonlinear behavior justifies it.

## Wow and Flutter Policy

Wow and flutter are record-speed errors.

They existed in `ScratchAcousticDsp` before this investigation.

The current implementation uses fixed oscillators and a heuristic depth.

Do not enable that implementation during clean 1× playback.

Manual record motion already contains user speed changes.

The transport model also adds platter, slipmat, and release behavior.

An added oscillator can count the same variation twice during scratching.

Keep wow and flutter only as an optional measured hardware profile.

Apply that profile only when its input motion does not already contain the measured variation.

## Why the Full Physical Renderer Is a No-Go

The current physical renderer fails the rapid-reversal callback deadline.

It also adds an uncalibrated virtual record chain during nominal playback.

That chain conflicts with the transparent master requirement.

No controlled listening corpus proves that the physical renderer sounds more authentic.

Many physical profile values are seed values. Hardware measurements do not validate the complete profile.

The full renderer therefore adds audible risk and deadline risk at the same time.

## Thirty-Minute Viability Test

The release benchmark used 128 internal frames at 192 kilohertz.

The callback deadline was 666,666 nanoseconds.

The test injected one previously calculated contact trace.

This injection removed runtime groove tracing from the measured callback.

The injected contact stayed constant. It was not valid audio output.

The test only measured a favorable lower bound for the remaining physical work.

| Fixture | p50 ns | p95 ns | p99 ns | Maximum ns | Mean ns | Misses |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Normal, full trace | 1,621,042 | 2,149,708 | 2,190,250 | 2,224,833 | 1,805,098 | 512/512 |
| Normal, fixed pretrace | 382,208 | 399,333 | 416,125 | 454,083 | 385,112 | 0/512 |
| Rapid reversal, full trace | 4,655,583 | 8,541,166 | 8,672,000 | 8,700,292 | 5,180,395 | 512/512 |
| Rapid reversal, fixed pretrace | 1,353,916 | 1,399,084 | 1,428,917 | 1,493,166 | 1,360,658 | 512/512 |

Precomputed contact made the normal fixture fit the measured deadline.

Its p99 used approximately 62 percent of the callback period.

Rapid reversal still missed every deadline without runtime tracing.

Its p99 used approximately 214 percent of the callback period.

The four swept-contact mechanics steps caused most of the remaining reversal cost.

Precomputation alone cannot make the present full physical design safe for intensive scratching.

## Existing Production Performance

The existing WebAssembly worklet measured normal p95 at 1.49 percent of a 2.667-millisecond quantum.

The same report measured reversing Crab/8 p95 at 8.70 percent.

These WebAssembly results use a different fixture and host environment.

They do not form a strict direct comparison with the native physical benchmark.

They show that the existing production design has much more callback headroom.

## Existing Renderer Signal Audit

The current renderer reads the decoded PCM at a position controlled by record motion.

That variable-rate PCM path is audio-derived and must remain.

The current default configuration also enables several heuristic additions.

At exactly 1× speed, `compute_movement_gain` applies a 1.025 program gain.

The default path also applies a 19-kilohertz drag filter at 48 kilohertz.

It adds fixed-depth wow and flutter when `acoustic_enabled` is true.

It enables surface effects, a stylus tracing limit, and a high-frequency acceleration limit by default.

The source slope and curvature come from decoded PCM.

However, their texture gain and limiter thresholds use heuristic constants.

The efficient renderer is therefore not yet a transparent audio-derived reference.

Removing these default additions will reduce work and improve reference fidelity.

## How the Work Reached This State

The work did not establish the transparent 1× invariant before physical implementation began.

It treated more simulated components as a possible measure of better sound.

The work built a virtual cut, groove tracer, contact solver, cartridge, and phono stage before a listening gate.

It also built those components before an end-to-end callback gate.

The workspace then contained an active renderer and a staged physical renderer with different behavior.

The host integration plan did not select one renderer early enough.

Some solver proof work had no production call site.

That work added no measured runtime gain and no demonstrated audible gain.

It delayed the product-level performance and listening decision.

Documentation also recorded stronger accuracy ambitions than the available evidence supported.

The audit found no proof that a better cartridge implementation was lost.

It found a difference between documentation claims, staged code, and the active production renderer.

## Important Technical Findings

An earlier audit reported a wrong local contact branch on a realistic Catmull–Rom groove.

The reported difference was approximately 7.07 micrometers in height.

The reported contact-position difference was approximately 3.74 micrometers.

The retained fixtures do not reproduce that exact result.

Treat the numbers as an unresolved audit record, not as current proof.

The branch-selection risk remains important for any future geometry tracer.

The 45/45 work correctly describes a virtual stereo groove made from decoded audio.

It remains useful as an offline reference and a research model.

The visual groove does not contain physical wall geometry.

Its pixels contain encoded bytes, which decode to the master signal.

The production renderer does not need a virtual cut for transparent 1× playback.

A future interaction model can use validated 45/45 effects only when they produce a measured perceptual improvement.

The swept-contact work exposed a real rapid-motion sampling problem.

Its current four-step implementation is too expensive for the callback.

Use this implementation as a reference, not as the production reversal loop.

## Work That Remains Useful

The canonical Rust migration reduced behavior differences between native and web hosts.

The deterministic replay and transport tests remain useful.

The callback benchmarks now provide a hard performance gate.

The contact cases record geometry and solver failure modes.

The global tracer can act as an offline reference for cheaper interaction models.

The external Bitneedle record provides a realistic future stress fixture without entering the repository.

`PHYSICS_VALIDATION_CASES.md` and `PHYSICS_INVESTIGATION_LOG.md` were not wholly wasted.

They preserve failures that the production interaction model must avoid.

They do not prove that the full physical renderer is suitable for release.

## Required Replacement Shape

The renderer must use direct master playback during stable positive 1× operation.

It must use an exact source sample when source and output rates match and positions align.

It must use transparent sample-rate conversion when sample rates differ.

The renderer must enter the interaction path smoothly when motion leaves the nominal state.

The transition must not click, change level, or change stereo balance.

The interaction path must support stops, slow drags, reversals, fast throws, and repeated direction changes.

It must keep source position continuous across all state changes.

It must keep high-frequency energy bounded during fast motion.

It must not invent constant record color to demonstrate physical complexity.

Surface noise and needle events require explicit product controls.

They must remain inactive during clean nominal playback by default.

## Release Gates

1. Prove sample identity at stable 1× when both sample rates match.
2. Measure transparent conversion error when the sample rates differ.
3. Record zero callback deadline misses during the declared stress suite.
4. Keep p99 below 50 percent of the callback deadline on the minimum supported device.
5. Compare rapid motion features with captured signals from the selected reference hardware.
6. Measure pitch, timing, spectrum, transient energy, continuity, and stereo behavior automatically.
7. Reject a complex path when it does not reduce a measured error.
8. Reject an efficient path when tests show that it fabricates or omits required behavior.
9. Use the external Bitneedle record for long-form native and web stress tests.
10. Activate the same `record-player` renderer in iOS and web hosts.
11. Remove `PhysicalHostRenderer` from production host APIs after validated parts are extracted.

## August 2 Production Checkpoint

The default `AcousticConfig` now disables acoustic coloration and surface effects.

The default stylus tracing and high-frequency acceleration strengths are zero.

The nominal movement gain is now exactly one.

An automated stereo test starts on aligned 48-kilohertz PCM.

It renders 256 frames at stable positive 1× speed.

Every rendered `f32` sample equals its decoded source sample.

The test also proves exact source-position advance and an exact effective rate of one.

The WebAssembly host now uses the same transparent defaults.

The iOS capture metadata now records the transparent limiter defaults.

The optional effect controls remain available for explicit experiments.

They do not define the production reference sound.

The release WebAssembly benchmark used stereo 128-frame blocks at 48 kilohertz.

Normal playback measured p95 at 0.0367 milliseconds.

That value used 1.38 percent of the callback period.

Reversing positive and negative 8× motion measured p95 at 0.1815 milliseconds.

That value used 6.80 percent of the callback period.

The largest measured callback used 13.04 percent of the period.

## Claim Limit

Do not claim that this is the most accurate record player simulation yet.

The current evidence proves selected mechanics, trace behavior, and performance failures.

It does not prove end-to-end perceptual superiority against current hardware.

Make an accuracy claim only after the performance, hardware, and listening gates pass.
