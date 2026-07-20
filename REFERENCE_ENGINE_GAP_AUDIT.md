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
- `src/scratch-source.js`
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
| Hand grip | Binary contact, `0.100 s` attack | `0..1` slipmat coupling, `0.012 s` attack | Beyond reference |
| Hand release | `0.045 s` | Same | Covered |
| Cue position chase | `0.28 s`, bounded correction | Same | Covered |
| Motion hold | `0.05 s` plus `0.06 s` release | Same | Covered |
| Motor-off throw | No separate signed throw model | `0.85 s` bearing decay | Beyond reference |
| True-speed lock | Gaussian pull near `1×` | Same in Rust command path | Covered |
| Wow | Motion-locked, `1.8 s` basis | Native-RPM revolution lock | Beyond reference |
| Flutter | `6.4 Hz`, speed-aware | Same | Covered |
| Visible platter phase | Programme-time and pointer-derived | Rust-rendered phase plus rate extrapolation | Beyond reference |
| Basic interpolation | Four-point cubic | Same below the high-rate blend | Covered |
| High-rate interpolation | Cubic at all rates | Adaptive 24-tap band-limited path | Beyond reference |
| Stylus drag | Speed-aware low-pass | Same base model | Covered |
| Stylus tracing limit | Fixed formula | Separate adjustable Rust model | Beyond reference |
| HF programme limit | Not present | Stereo-linked upper-band limiter | Beyond reference |
| Groove and dust texture | Position-keyed | Same hashes and tuning | Covered |
| Contact impulses | Grab, reversal and acceleration | Canonical gesture events into Rust | Covered |
| Surface asset and foley | Lead-in, deadwax and drop | Same assets and Rust rendering | Covered |
| Needle cue landing | `50–140 ms` early during playback | Same immutable landing for preview and commit | Covered |
| Preset names and click defaults | Eight presets, `1..8` clicks | Same | Covered |
| Preset phase | Wall-clock oscillator | Audible groove travel | Beyond reference |
| Direction logic | Rate sign | Hysteretic intent plus rendered fallback | Beyond reference |
| Drum acceleration | Event-time rate changes | Filtered audio-rate intent | Beyond reference |
| Gate application | Main-thread fader automation | Rust per output sample | Beyond reference |
| Gesture input | One pointer EMA | Coalesced multi-pointer tracker | Beyond reference |
| Programme-gap tonearm | Monotone visible-gap calibration | Same Rust-owned map with radius-exact inverse cueing | Beyond reference |
| PCM windows | Four-second reference windows | Two six-second bounded banks | Covered |
| Seam repair | Reference window assembly | Authoritative 24-sample repair | Covered |
| Capture | Throttled telemetry | Versioned output-frame events | Beyond reference |
| Replay | Frame-driven re-performance | Deterministic seed/phase, sub-quantum events and Rust snapshot | Beyond reference |
| TAPE master source | Sample-aligned HQ sidecar switch | Same local/remote source and window replacement | Covered |
| Manual fader | Mixer automation | Independent Rust post-gate gain | Beyond reference |
| Advanced controls | Not present as this pair | HF limit and stylus limit dropdown | Beyond reference |
| Device underrun telemetry | Not public | Normalized browser playback statistics | Beyond reference |
| Acoustic-loopback latency | Not present | Frame-tagged output-to-input correlation probe | Beyond reference |
| DJ-study analysis | Not present | Versioned exact acceptance analyzer | Beyond reference |
| DJ evidence collection | Not present | Build-bound browser session console | Beyond reference |
| Blind ABX execution | Not present | Clean-build-bound opaque packages and condition-free runner | Beyond reference |

## Important deliberate differences

The `0.012 s` grip attack replaces the reference `0.100 s` value. A deliberate
grab reaches platter ownership in about 20 ms. This prevents a long motor bleed
through the first part of a scratch. The target also accepts continuous grip:
light contact preserves powered slipmat motion while firm contact gives the hand
record ownership. Rust smooths the coupling. Pen pressure or explicit API input
can drive it; mouse and finger touch default to full grip. Native tests cover
both response and coupling.

The automatic gate does not copy the reference time oscillator. Travel controls
its phase. Faster hand motion creates faster cuts over the same groove distance.
A held record freezes the pattern. A confirmed reversal starts a new stroke.
Residual motion in the old direction cannot advance the new stroke phase.

Drum intent uses an `8 ms` audio-rate one-pole model before differentiation.
This removes sample-rate-dependent impulses from small browser target steps.
Slow motion below `0.14×` cannot trigger a Drum onset or reversal attack.
New recordings identify this behavior as gate algorithm version `3`.

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
- programme-gap anchor validation and monotone stylus calibration
- preset intent, gate timing and de-clicking
- manual replay fader and final output gain
- replay state snapshots.

JavaScript owns browser-bound work:

- DOM pointer collection and coalesced sample order
- lazy presentation-WASM lifecycle and tonearm pointer projection
- AudioWorklet output-frame event scheduling
- bounded bank coordination, direct WASM-window input copies and planar output
  copies
- off-thread signed-16-bit retention and seam repair
- UI, storage and public API plumbing
- browser playback/underrun telemetry normalization.

Moving pointer collection into a worker would add a message hop. Moving each
coalesced point through a second WASM boundary would also add work. There is no
measured audio-thread reason to make either change.

## Automated evidence

- `cargo test --workspace`: 92 tests.
- `node --test test/*.test.mjs`: 75 tests.
- The canvas regression anchors the visible platter to audio-owned phase and
  covers half-speed forward motion, full-speed reverse motion and zero-rate
  hold. It rejects nominal-RPM animation.
- Both pages now expose only the canonical canvas record gesture. The
  permanently hidden legacy platter and its single-turn, event-rate-dependent
  differentiator were removed; no `abs(rate) / 3` move-impulse path remains.
- Rust programme-gap calibration tests pin both visible gap edges, reject
  overlapping or flat anchors and round-trip source samples through the
  monotone inverse. Canvas tests recover exact groove radius from the physical
  arm projection. Chrome mapped samples `4,000,000..4,096,000` to visible
  progress `0.421..0.429` and mapped the band midpoint back to sample
  `4,048,382.81` through release WASM.
- Three current worklet runs measured normal p95 at `1.47%` of a quantum.
- They measured `8.10–8.14%` for alternating `±8×` Crab/8.
- The post-calibration release run measured p95 `1.49%` normal and `8.70%`
  under the same stress. Deterministic audio hashes remained unchanged.
- Direct copy into prepared Rust window storage reduced fresh six-second window
  application to p95 `2.90–3.08%` across the same runs.
- Regression gates now require window-application p95 below `25%` and maximum
  below `50%` of a quantum.
- Release WASM replays the same mixed preset/click/fader take twice with 2,176
  live playback frames between runs. It requires identical SHA-256 output and
  gate traces. It also verifies controls at frame offsets `83`, `91`, `155` and
  `301`. The variable-grip take hashes to `5cada29f...`; a full-grip control
  hashes to the prior `890df8bd...` output and must differ.
- `npm run test:browser`: real Chrome 150 and real release WASM passed.
- The Chrome run exercised every preset in both directions.
- Chrome intercepted the actual worklet seek messages. It verified that active
  playback used one `50–140 ms` early landing and never exposed the exact visual
  aim before that landing.
- It captured rendered output and replayed an engine-version-5 take with more
  than 400 events. Capture preserved grip values `0.25`, `0.35` and `0.85`, and
  public grip returned to zero on release.
- Browser scratch begin used `-1.4×` intent and a `0.37` grab impulse. Reverse
  intent remained active after the Rust-core round trip, and capture retained
  both values. This rejects a later zero-rate command overwrite.
- Browser pointer timestamps projected onto explicit output frames carried by
  host, worklet and Rust begin/move/end commands. A `40 ms` delayed marker
  must record within 512 frames of its independent browser-clock projection.
  Requested and applied output frames are exposed separately and must agree
  with latency telemetry.
- Pointer-cancel and canvas-destroy paths release contact and grip, free record
  ownership for the next pointer and retain `cancelled: true` in the normalized
  performance trace. A valid zero DOM timestamp is preserved rather than
  replaced by main-thread time.
- It restored the pre-replay Rust state.
- A second trusted touch moved and released XFADE.
- The record touch remained active until its own release.
- Programmatic, trusted mouse, trusted touch and trusted keyboard controls each
  selected all eight presets and all eight click counts without moving the
  manual fader from `0.37`.
- The default embed showed the canvas technique selectors and collapsed
  advanced dropdown in its initial viewport.
- Three independent Chrome runs sampled Web Audio render capacity during
  lead-in, normal playback, a PCM window swap, all presets and replay.
- The three-run stress batch had a worst sampled render capacity of `13.33%`
  and a worst run-level p95 of `10.65%`. A later full build-and-test invocation
  produced a `62.66%` callback sample. It stayed below the full callback
  deadline.
- The harness enforces p95 below `50%` and every sampled callback below `100%`.
- Three current Chrome runs observed `12.05–13.05 s` through the browser's audio
  playback statistics. All three reported zero underrun events and zero
  underrun duration. The measured average device-path latency was about
  `35.02 ms` in this headless environment.
- The capture checks covered about `12.7 s` per run. All timestamps were
  monotonic. Cumulative timeline error stayed at or below `3.18 ms`. Steady
  playback did not contain a silent packet.
- The DJ validation analyzer computes the exact two-sided binomial test and a
  95% Clopper-Pearson interval. Tests reject identification, repeated cues and
  audio underruns. It verifies the exact clean-build metadata and rejects
  reused capture audio across the full study. A complete synthetic study passes
  all registered rules.
- Chrome opened the dedicated collection console, bound it to generated build
  metadata and added a participant through the real form. The console refuses
  release blocks from a dirty or mismatched candidate. It invalidates an active
  block if an acoustic effect or limiter setting changes.
- Chrome loaded an opaque ABX package, verified its manifest-bound WAV files,
  played A, B and X, and froze a condition-free response. The package is bound
  to the clean candidate commit, settings and build-info digest. Package tests
  cover participant binding, balanced conditions, private decoding, cue coding
  and audible-content freshness despite WAV metadata changes.
- Chrome detected three software-loopback probes at `20.667 ms`. Minimum
  correlation was above `0.994`. This checks scheduling and correlation only;
  it is not a physical-path result.

## Remaining proof gaps

1. Repeat deadline, capture-continuity and zero-underrun checks on the supported
   physical hardware matrix. The current Chrome result uses a headless output
   device.
2. Test actual touchscreens, pens, trackpads and mouse devices.
3. Run the acoustic-loopback probe through each target interface and speaker or
   electrical-loopback path. Attach the result to the study file.
4. Collect the frozen live data in `/dj-validation.html` and blind responses in
   `/dj-abx.html`. Decode each participant only after freeze. Analyze the merged
   data with `npm run validation:analyze -- <file>`.
5. Tune only the variables that fail those tests.

An exact JavaScript-to-Rust output hash is not a useful release gate. The target
intentionally changes grip, resampling, preset timing and mastering protection.
Mechanical traces, spectral bounds and human comparison are the correct gates.

## Release decision

The software implementation can move to hardware validation. No additional
reference feature should be copied before that run. New DSP work without a
measured failure would increase tuning risk and reduce the value of the blind
comparison.
