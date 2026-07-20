# Reference Engine Gap Audit

Date: 2026-07-20

Reference: `../yl.vin/apps/play`

Target: this repository

## Verdict

No known reference behavior is missing from the main Rust playback and scratch
path. The target includes all high-value reference mechanics. It also replaces
some reference approximations with stronger audio-clock models.

The target is an engineering release candidate. It is not yet proven to be
indistinguishable from vinyl. That claim needs target hardware and blind DJ
results.

## Source review

The audit inspected the current reference source. The primary files were:

- `ACOUSTICS.md`
- `src/scratch-varispeed-worklet.js`
- `src/scratch-audio-runtime.js`
- `src/scratch-gesture-controller.js`
- `src/scratch-techniques.js`
- `src/player-audio-mixer.js`
- the relevant transport, surface and cue sections in `src/player.js`.

The audit compared those files with:

- `src/acoustic.rs`
- `src/scratch_gate.rs`
- `src/resampler.rs`
- `src/engine.rs`
- `web/scratch-gesture.js`
- `web/player-worklet.js`
- `web/player-host.js`
- `web/player-canvas.js`.

## Behavior matrix

| Area | Reference | Target | Result |
| --- | --- | --- | --- |
| Playback and scratch clock | One varispeed worklet clock | One Rust DSP clock | Covered |
| Rate spring | `70 rad/s`, damping `0.85` | Same | Covered |
| Motor start and brake | `0.30 s` start, `0.32 s` brake | Same | Covered |
| Hand grip | `0.100 s` attack | `0.012 s` attack | Deliberate improvement |
| Hand release | `0.045 s` | Same | Covered |
| Cue position chase | `0.28 s`, bounded correction | Same | Covered |
| Motion hold | `0.05 s` plus `0.06 s` release | Same | Covered |
| Motor-off throw | No separate signed throw model | `0.85 s` bearing decay | Beyond reference |
| True-speed lock | Gaussian pull near `1×` | Same in Rust command path | Covered |
| Wow | Motion-locked, `1.8 s` basis | Native-RPM revolution lock | Beyond reference |
| Flutter | `6.4 Hz`, speed-aware | Same | Covered |
| Basic interpolation | Four-point cubic | Same below the high-rate blend | Covered |
| High-rate interpolation | Cubic at all rates | Adaptive 24-tap band-limited path | Beyond reference |
| Stylus drag | Speed-aware low-pass | Same base model | Covered |
| Stylus tracing limit | Fixed formula | Separate adjustable Rust model | Beyond reference |
| HF programme limit | Not present | Stereo-linked upper-band limiter | Beyond reference |
| Groove and dust texture | Position-keyed | Same hashes and tuning | Covered |
| Contact impulses | Grab, reversal and acceleration | Canonical gesture events into Rust | Covered |
| Surface asset and foley | Lead-in, deadwax and drop | Same assets and Rust rendering | Covered |
| Preset names and click defaults | Eight presets, `1..8` clicks | Same | Covered |
| Preset phase | Wall-clock oscillator | Audible groove travel | Beyond reference |
| Direction logic | Rate sign | Hysteretic intent plus rendered fallback | Beyond reference |
| Drum acceleration | Event-time rate changes | Filtered audio-rate intent | Beyond reference |
| Gate application | Main-thread fader automation | Rust per output sample | Beyond reference |
| Gesture input | One pointer EMA | Coalesced multi-pointer tracker | Beyond reference |
| PCM windows | Four-second reference windows | Two six-second bounded banks | Covered |
| Seam repair | Reference window assembly | Authoritative 24-sample repair | Covered |
| Capture | Throttled telemetry | Versioned output-frame events | Beyond reference |
| Replay | Frame-driven re-performance | Sub-quantum events plus Rust snapshot | Beyond reference |
| Manual fader | Mixer automation | Independent Rust post-gate gain | Beyond reference |
| Advanced controls | Not present as this pair | HF limit and stylus limit dropdown | Beyond reference |

## Important deliberate differences

The `0.012 s` grip attack replaces the reference `0.100 s` value. A deliberate
grab reaches platter ownership in about 20 ms. This prevents a long motor bleed
through the first part of a scratch. Native tests cover this response.

The automatic gate does not copy the reference time oscillator. Travel controls
its phase. Faster hand motion creates faster cuts over the same groove distance.
A held record freezes the pattern. A confirmed reversal starts a new stroke.

Drum intent uses an `8 ms` audio-rate one-pole model before differentiation.
This removes sample-rate-dependent impulses from small browser target steps.
Slow motion below `0.14×` cannot trigger a Drum onset or reversal attack.
New recordings identify this behavior as gate algorithm version `2`.

The sharp manual crossfader uses width `0.08`. The reference mixer uses a wider
curve. Public curve adjustment stays deferred until DJ tests show a need.

Surface slice selection uses deterministic Rust random state. The reference
uses `Math.random()`. The distribution is equivalent, but output is repeatable.

The target starts a physical lead-in for a loaded record. The current reference
contains lead-in code, but its normal start path does not call it. This target
uses the physical behavior by design.

## Rust and JavaScript boundary

Rust owns all audio-rate and state-critical behavior:

- platter, motor, grip and throw mechanics
- sampling, resampling and cartridge response
- wow, flutter, surface texture and foley
- the HF acceleration limiter and stylus limit
- preset intent, gate timing and de-clicking
- manual replay fader and final output gain
- replay state snapshots.

JavaScript owns browser-bound work:

- DOM pointer collection and coalesced sample order
- AudioWorklet output-frame event scheduling
- bounded bank coordination and planar output copies
- off-thread signed-16-bit retention and seam repair
- UI, storage and public API plumbing.

Moving pointer collection into a worker would add a message hop. Moving each
coalesced point through a second WASM boundary would also add work. There is no
measured audio-thread reason to make either change.

## Automated evidence

- `cargo test --workspace`: 80 tests.
- `node --test test/*.test.mjs`: 22 tests.
- `npm run bench:worklet`: normal p95 `1.45%` of a quantum.
- The same benchmark measured `7.88%` for alternating `±8×` Crab/8.
- Fresh six-second window application measured p95 `25.01%` and maximum
  `53.30%`.
- `npm run test:browser`: real Chrome 150 and real release WASM passed.
- The Chrome run exercised every preset in both directions.
- It captured rendered output and replayed a take with more than 400 events.
- It restored the pre-replay Rust state.
- A second trusted touch moved and released XFADE.
- The record touch remained active until its own release.
- Three independent Chrome runs sampled Web Audio render capacity during
  lead-in, normal playback, a PCM window swap, all presets and replay.
- The three-run stress batch had a worst sampled render capacity of `13.33%`
  and a worst run-level p95 of `10.65%`. A later full build-and-test invocation
  produced a `62.66%` callback sample. It stayed below the full callback
  deadline.
- The harness enforces p95 below `50%` and every sampled callback below `100%`.
- The capture checks covered about `12.7 s` per run. All timestamps were
  monotonic. Cumulative timeline error stayed at or below `3.18 ms`. Steady
  playback did not contain a silent packet.

## Remaining proof gaps

1. Repeat deadline, capture-continuity and device-xrun checks on the supported
   hardware matrix. The headless Chrome result does not exercise a physical
   output device.
2. Test actual touchscreens, pens, trackpads and mouse devices.
3. Measure end-to-end acoustic latency through the interface and speakers.
4. Run the pre-registered blind test in `DJ_VALIDATION_PROTOCOL.md`.
5. Tune only the variables that fail those tests.

An exact JavaScript-to-Rust output hash is not a useful release gate. The target
intentionally changes grip, resampling, preset timing and mastering protection.
Mechanical traces, spectral bounds and human comparison are the correct gates.

## Release decision

The software implementation can move to hardware validation. No additional
reference feature should be copied before that run. New DSP work without a
measured failure would increase tuning risk and reduce the value of the blind
comparison.
