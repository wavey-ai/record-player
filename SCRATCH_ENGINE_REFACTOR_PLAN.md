# Playback and Scratch Engine Refactor Plan

Status: automated engine and browser implementation is complete as of
2026-07-20. The original phases below remain as the implementation record from
checkpoint `5eccda9` (`Checkpoint audio preview and capture support`). Hardware
measurements and the blind DJ protocol remain outstanding. No perceptual claim
is made.

Current automated evidence:

- `cargo test --workspace`: 108 tests (103 `record-player`, 5 `player-wasm`).
- Rust gate conformance traverses one learned stroke for Transform, Flare, Crab
  and Orbit. Click values 1, 4 and 8 produce exactly that many pulses or notches.
  At `8x`, the de-click envelope stays within its analytical fastest-attack
  adjacent-sample bound on sine and transient fixtures. Stable Stab closed/open
  RMS ratios remain below `1e-6` and above `0.999`.
- `node --test test/*.test.mjs`: 80 tests. Canvas tests check audio-owned phase
  at half-speed forward, full-speed reverse and rest, plus all eight preset and
  click-count selections through independent mouse and touch gestures. The
  click control remains absent and non-interactive for techniques that ignore it.
- The default tonearm now consumes decoded programme-gap anchors through the
  Rust monotone calibration. Its inverse projects the pointer onto physical arm
  length and recovers groove radius before seeking. Release WASM pinned a test
  gap to `0.421..0.429` and mapped its midpoint into the matching PCM interval.
- The canvas is the only bundled scratch surface. The permanently hidden legacy
  platter and its separate single-turn, event-rate-dependent differentiator
  were removed, so every bundled gesture uses the canonical tracker.
- `npm run build`: both browser WASM packages build in release mode.
- The current 2026-07-20 Chrome 150 integration run passed. It reached empty-deck
  motor phase, automatic needle drop, all direct preset controls and every
  radial click value through API, mouse, touch and keyboard paths. It verified
  per-preset click memory and hid the control for non-click techniques. Chrome
  reported zero underruns over `9.02 s`.
- `npm run bench:worklet`: three release-WASM runs measured p95 at `1.47%`
  normal and `8.10–8.14%` for alternating `±8×` crab/8. Fresh six-second
  stereo PCM window p95 was `2.90–3.08%` against a `2.667 ms` quantum.
- The rate-adaptive sinc keeps eight output-domain frames of bounded support
  instead of collapsing a fixed 24-source-tap kernel at high speed. Spectral
  sweeps at `±2×`, `±4×` and `±8×` keep passband RMS within `0.012` and
  stopband RMS below `0.003`, with exact forward/reverse symmetry. A recurrence
  matches the direct sinc calculation within `1e-11` while avoiding per-tap
  trigonometry. Release WASM measured `1.52%` normal, `6.60%` reversing Crab/8
  and `3.28%` fresh-window p95 against the audio quantum.
- Signed Rust mechanics traces apply identical limits in both directions. The
  motor reaches `0.55..0.70×` after 300 ms and more than `0.94×` after one
  second. A firm 50 ms grab reduces rate below 20% of steady speed. The motor
  catches above `0.75×` within 100 ms of release. A free throw retains more
  than `0.55×` after 200 ms, while a powered brake falls below `0.05×` after
  400 ms. Forward and reverse results agree within `1e-10`.
- Two current high-contention benchmark attempts exceeded the fresh-window
  threshold. The threshold was not relaxed. A later rerun passed with normal
  p95 `0.0573 ms`, reversing Crab/8 p95 `0.2599 ms`, and fresh-window maximum
  `0.3707 ms` against the `2.667 ms` quantum after gate v5.
- The post-calibration release run measured p95 `1.49%` normal, `8.70%` for
  alternating `±8×` crab/8 and `2.90%` for a fresh six-second window. Replay
  hashes remained unchanged, confirming that presentation calibration does not
  enter the audio renderer.
- The release-WASM harness requires identical output SHA-256 and gate traces
  when 2,176 live frames separate two runs of the same take. It verifies
  preset, click and manual-fader events at four exact sub-quantum offsets. A
  variable-grip take hashes to `0d44547b...`; its full-grip control hashes to
  `d3bf7be0...` and must differ. The harness also proves that a version-4 replay
  selects the historical Rust gate behavior and restores live version 5.
- The release-WASM harness also proves live scratch release policy in the next
  128-frame quantum. Explicit `resumePlayback: false` and a grab that began
  while paused produce exact silence and stop programme DSP immediately. A grab
  that began during playback resumes, while a lifted powered platter keeps
  rotating silently. Chrome captured all 12 post-release packets as silence,
  spanning 5,760 contiguous frames.
- The release-WASM progressive fixture stops at contiguous decoded coverage,
  requests the next bank, and holds the exact rendered source frame through four
  waiting quanta. New availability activates the requested bank without a
  position reset. Rust fades program audio out and back in over 6 ms. The first
  resumed-sample difference is less than `0.02`. The fixture does not expose
  zero-filled PCM or return at full level. The current release run measured
  `1.47%` normal, `6.40%` reversing Crab/8 and `2.94%` fresh-window p95 against
  the audio quantum.
- The release-WASM bounded-window fixture configures a five-minute stereo
  source. The worklet and Rust retain 6.59 MiB of PCM instead of a 109.86 MiB
  full Float32 record. Transport initialization and distant bank swaps do not
  grow Rust memory beyond the first 2.25 MiB window allocation. The worklet
  rejects a window that exceeds its configured frame budget before Rust can
  allocate it. The generated Rust API has no legacy full-record set or append
  methods. Rendered forward and reverse rates above `4×` each request and
  activate a bank 48,000 source frames in the direction of travel.
- `npm run test:browser`: Chrome 150 loaded real release WASM at 48 kHz.
  It exercised all eight gates in both directions and captured rendered audio.
  It recorded, replayed and restored an engine-version-5 take with more than
  400 events. The take retained begin/motion grip values `0.25`, `0.35` and
  `0.85`; public grip returned to zero on release.
  Scratch begin also retained `-1.4×` reverse intent and a `0.37` grab impulse
  after the Rust-core round trip instead of replacing them with zeros.
  Pointer input timestamps now project onto one bounded integer output frame
  carried through the worklet message, Rust command protocol and capture. A
  deliberately delayed browser marker must record within 512 frames of its
  independently calculated intent offset while applied-frame telemetry remains
  separate.
  Pointer cancellation follows the same safe physical release, remains distinct
  in the normalized take, and immediately frees canvas record ownership.
  It kept a trusted record touch active while a second touch moved XFADE. The
  API, trusted mouse, trusted touch and trusted keyboard paths each selected
  all eight presets and all eight click counts while XFADE remained at `0.37`.
  Transform restored its stored click count. Chirp disabled and hid CLICKS.
  The embed exposed both canvas selectors and its collapsed advanced dropdown
  in the initial viewport.
- The current browser run measured less than `5.44 ms` maximum pointer-to-apply
  latency in this synthetic trace. It reported `5.33 ms` base latency and `32 ms`
  output latency. These are headless host results, not hardware population data.
- The post-frontier-fix Chrome run reported zero underruns over `9.019 s`. Its
  sampled Web Audio callback p95 was `5.02%`, and its maximum was `6.12%` of the
  callback budget.
- Three independent browser runs sampled Chrome's Web Audio render capacity by
  phase. The worst maximum was `13.33%`, and the worst run-level p95 was
  `10.65%`. A later build-and-test invocation produced a `62.66%` callback
  sample, still below the full callback deadline. The harness enforces p95
  below `50%` and every sampled callback below `100%`. Capture timestamps stayed
  monotonic, cumulative timeline error stayed within `3.18 ms` over about
  `12.7 s`, and steady playback had no silent packet.
- The public state normalizes current `AudioContext.playbackStats` and legacy
  `playoutStats`. Three current Chrome runs observed `12.05–13.05 s` with zero
  underrun events and zero underrun duration. Target hardware must repeat this
  result.
- `npm run validation:template` creates the versioned DJ-study data shape.
  `npm run validation:analyze -- <file>` applies the pinned preflight, ABX and
  live-control acceptance rules. Unit tests cover passing and failing studies.
- `/dj-validation.html` embeds the real player and collects build-bound hardware,
  loopback, audio-block, participant, trial, routine and artifact evidence. It
  refuses release measurements from a dirty or mismatched build. Its bounded
  pointer probe records emitted pointer types, cadence, coalesced samples,
  pressure range, contact geometry, multi-touch and cancellation evidence. The
  schema-four session cannot start a block until that target-device profile
  passes; touch is explicitly full-contact grip and requires two pointers.
- `/dj-abx.html` runs participant-bound blind packages without condition labels.
  The coordinator tool verifies matched WAV captures, gives every A/B/X file a
  unique opaque identity, binds the package to the clean candidate build and
  keeps decoding in a private post-freeze step. Audio-data hashes prevent reuse
  across participants even when WAV metadata changes.
- The public acoustic-loopback probe emits a bounded diagnostic sweep and uses
  AudioWorklet frame tags for input correlation. Chrome passed a three-probe
  software loopback. Target hardware must supply the physical result.
- Physical output-device xrun measurements, physical controller runs and the
  human protocol in `DJ_VALIDATION_PROTOCOL.md` remain outstanding.

Grip now crosses the complete Phase 1 protocol. Explicit `0..1` API grip and
pen pressure reach Rust slipmat coupling, public state and deterministic
capture/replay. Missing historical values, mouse input and finger touch default
to full grip. iPhone Haptic Touch is not treated as force input; any
intent-derived touch grip needs target-device measurements before tuning.
Initial hand rate and grab impulse now cross the same ordered Rust protocol.
Historical begin events default to `0×`, the canonical `0.22` grab and full
grip, so the asynchronous core response cannot overwrite a newer live intent.

## Objective and proof standard

The player must use one continuous, audio-authoritative vinyl transport for normal
playback, braking, hand grabs, reverse motion, cue holds, releases, lead-in and
run-out. Scratch assistance must infer the user's motion rather than replaying a
wall-clock animation, and it must never prevent a DJ from operating the physical
record and manual crossfader with separate pointers.

"A DJ cannot tell" is a human perceptual claim, so automated checks alone cannot
prove it. This implementation will provide deterministic mechanical, spectral,
latency and browser evidence plus a repeatable blind comparison protocol. The
claim remains unproven until that protocol is run with experienced DJs.

## Audit baseline

Before implementation, the following checks pass:

- `cargo test --workspace`: 19 tests (14 player, 5 decoder facade).
- `node --test test/*.test.mjs`: 1 test.
- `npm run build`: both browser WASM packages build.

The passing suite does not cover production gesture handling, audio-thread
windowing, seam continuity, presets, replay clocks, multiple pointers or browser
latency.

## Architectural decisions

1. The Rust `ScratchAcousticDsp` remains the only programme-audio renderer and
   owns the final per-sample packet/mixer output gain. The browser `GainNode`
   remains unity and exists only for routing and capture. There
   will be no second scratch buffer-source path.
   More generally, per-sample and state-critical engine behavior belongs in Rust
   by default. JavaScript retains browser scheduling, pointer input and bounded
   buffer coordination only where moving the boundary would add measured work or
   latency; performance measurements, not language preference, decide exceptions.
2. Source positions use source frames. Performance timing uses AudioContext/output
   frames. Names and schemas must say which clock they use.
3. Pointer geometry and coalesced event collection live in the UI layer. Physical
   rate mapping, platter mechanics, de-clicking and assisted gate generation live
   in the audio engine.
4. Crossfader ownership follows the selected mode:

   - `baby`: `deck gain = channel gain × manual crossfader curve`.
   - automatic presets: `deck gain = channel gain × technique gate`.

   The Rust gate is the real audible XFADE in every automatic preset. Direct
   manual XFADE input selects `baby` and transfers ownership to the manual
   curve. Public state retains both the stored manual value and effective gate.
5. Preset phase advances from rendered groove travel in the confirmed stroke
   direction. It freezes while held still and resets after a hysteretic reversal.
   Residual outgoing motion must not advance the new stroke. Phase must not depend
   on `requestAnimationFrame`, pointer frequency or main-thread load.
6. Worklet memory is bounded by double-buffered PCM windows. Decoding and Int16 to
   Float32 conversion stay off the audio thread. A non-shared fallback may copy a
   bounded window, but may not restore full-record worklet allocations.
7. Published records enter a physical run-out/deadwax by default. Conventional
   authoring-preview audio may opt into a clean end explicitly.
8. The sharp `0.08` fader cut is shared by the Rust mixer and replay DSP, named in
   schema metadata and regression-tested. A public curve-tuning API is deferred
   until blind DJ evidence justifies the extra variability; the reference app's
   `0.22` width remains a documented different tuning, not silent parity.

## Phase 1 — clocks, ownership and state transitions

- Add an explicit source sample-rate update to the Rust player engine when a source
  is initialised; remove the fixed-48 kHz assumption from begin/end conversions.
- Track pointer target position and audio-rendered position separately in the host.
  Never overwrite one with the other.
- Make scratch begin/move/end/cancel an ordered protocol with explicit:
  `sourcePositionFrames`, `outputFrame`, `handContact`, `resumePlayback`, needle
  state, grip strength and release/handoff policy.
- Preserve a lifted needle during a hand grab. The record may turn visually, but
  the cartridge remains muted and the groove readhead does not falsely advance.
- Make `resumePlayback: false` stop programme playback in both core and worklet.
- Carry contact impulses through `HostCommand` translation instead of dropping
  them, and use rendered position for release/capture.
- Update the motor target whenever RPM changes while the motor is on, including
  run-out and non-programme states.

Acceptance evidence:

- Transition-table tests cover motor on/off × playing/paused × needle up/down ×
  scratch begin/end/cancel.
- 44.1, 48 and 96 kHz sources release to the same source frame (within one frame).
- `resumePlayback: false` produces silence and a stopped state in the next quantum.

## Phase 2 — bounded, continuous PCM delivery

- Reconnect `pcm-window-worker.js` and the two shared 6-second banks.
- Keep full Int16 programme storage outside the AudioWorklet; keep only the active
  Float32 window in Rust.
- Route progressive segments through the worker, merge written ranges, and expose
  contiguous availability to the worklet.
- Repair every contiguous chunk seam in the authoritative worker/source copy before
  a window becomes visible to Rust. Do not repair an unused JavaScript mirror.
- Consume `ScratchAcousticDsp.takeWindowRequest()` and project refills in both
  directions at high speed.
- Clamp seeking/scratching to decoded coverage, enter an explicit bounded buffering
  state at the frontier, and resume without resetting motor or spring state.
- Remove allocation/conversion loops from the render quantum.

Acceptance evidence:

- A five-minute stereo source keeps worklet + Rust PCM below the configured window
  budget rather than allocating full-record Float32 copies.
- A deliberately discontinuous chunk fixture crosses the repaired seam with a
  bounded first difference in forward and reverse playback.
- Slow progressive decode pauses at the availability frontier and resumes from the
  same rendered frame without a zero-filled excursion.
- High-speed forward/reverse traces request and activate the correct projected bank.

## Phase 3 — canonical high-resolution gesture input

- Replace both raw DOM/canvas differentiators with one pure gesture tracker.
- Consume `getCoalescedEvents()` sequentially and use incremental unwrapped angles,
  a 4 ms differentiation floor, multi-turn accumulation and a time-aware adaptive
  velocity filter (fast reversal response, approximately 35 ms steady smoothing).
- Apply a Schmitt direction state so sub-deadzone jitter cannot flip preset intent.
- Emit contact impulse only for a real grab, reversal or high acceleration; remove
  the current event-rate-dependent `abs(rate)/3` impulse on every move.
- Treat stylus-up motion and pointer cancellation explicitly.
- Replace the canvas's one global gesture slot with a pointer map, allowing one hand
  on the record while another moves XFADE/CH/PITCH.
- Wire programme-gap stylus calibration into the default canvas tonearm mapping.

Acceptance evidence:

- Equivalent gestures sampled at 30/60/120/240 Hz and with coalesced samples produce
  equivalent position/rate traces within declared tolerances.
- Tests cover ±π wrap, multiple turns, duplicate timestamps, reversal, dwell,
  cancellation, near-spindle movement and lifted-needle movement.
- A browser test holds the record with pointer A and moves/releases the fader with
  pointer B without losing either gesture.

## Phase 4 — audio-rate intent-aware scratch presets

- Add versioned presets: Baby, Stab, Chirp, Transform, Flare, Crab, Orbit and Drum.
- Preserve the reference starting constants (5.2/9.5/6.4/18/7.2/12 Hz, transform
  duty 0.48, crab duty 0.38, click range 1–8), but interpret them as initial groove-
  travel spans rather than an unconditional wall-clock oscillator.
- Implement the gate as a Rust audio-rate state machine using filtered target rate,
  rendered rate, velocity confidence, acceleration, dwell, direction hysteresis,
  distance since reversal and a learned stroke span.
- Gate algorithm 4 applies click count only to Transform, Flare, Crab and Orbit.
  Each preset keeps its own setting. Versioned replay retains the version 1–3
  global phase multiplier for historical takes.
- Gate algorithm 5 buffers candidate-direction rendered travel during onset and
  reversal confirmation. It commits that distance only after intent is confirmed,
  so fast patterns do not land late and false reversals cannot spend a stroke.
  Tests cover 8× travel, rejected jitter and 44.1/48/96 kHz invariance.
- Technique behavior:
  - Baby: assisted gate open; manual fader only.
  - Stab: forward stroke audible, return and hold cut.
  - Chirp: open at forward onset, close by speed-adjusted travel, reopen on return.
  - Transform: travel-locked repeated open/closed chops.
  - Flare: open phrase with click-count closed notches.
  - Crab: velocity-scaled rapid open pulses.
  - Orbit: symmetric notches reset for each direction.
  - Drum: acceleration/onset transient with travel/time close and retrigger guard.
- Filter Drum intent on the audio clock before differentiation. This prevents a
  small held-target step from becoming a sample-rate-dependent false attack.
- Require Drum onset and reversal attacks to exceed the minimum motion rate.
- Apply velocity-dependent 0.35–4 ms attack/release envelopes to prevent digital
  discontinuities while retaining a sharp professional cut.
- Return the technique gate to open on hand release so motor playback cannot remain
  accidentally muted.
- Expose effective gate, direction and rate telemetry without putting UI work on the
  render thread.

Acceptance evidence:

- Truth-table and trace tests cover rest/forward/reverse/reversal for every preset.
- Click values 1/4/8 create the expected pulses/notches over a learned stroke.
- Doubling travel velocity doubles temporal chop frequency while identical travel
  produces the same pattern; holding still freezes phase.
- A confirmed intent reversal resets phase before the rendered spring crosses
  zero. The new phase waits for rendered motion in the confirmed direction.
- Gate timing is invariant at 44.1/48/96 kHz and across 128-frame boundaries.
- Closed/open RMS and maximum adjacent-sample discontinuity are bounded on sine and
  transient fixtures.

## Phase 5 — platter and stylus fidelity beyond reference parity

- Apply the reference deadzone and Gaussian ±1× lock in the actual DSP command path,
  not in unused code.
- Shorten/tune hand ownership for deliberate DJ grabs and cover it with quantitative
  response tests; retain motor spin-up/brake constants unless evidence contradicts
  them.
- Model an unpowered hand throw separately from an explicit motor brake, preserving
  signed platter momentum with bearing-friction decay.
- Configure revolution-locked wow from native RPM (1.8 s at 33⅓, 1.333… s at 45).
- Add a rate-adaptive band-limited interpolation/anti-alias path for high-speed
  forward and reverse motion, with a smooth transition from the low-latency path.
- Preserve the speed- and source-curvature cartridge model as the adjustable
  `stylusTracingLimit` (default `0.72`). This is the physical stylus-tracing model,
  not a programme mastering limiter, and must never be mislabeled as one.
- Add a separate stereo-linked `highFrequencyAccelerationLimit` (conservative
  default `0.35`, exact bypass at `0`) for mastering protection. Split programme
  into a complementary base plus approximately 5.2 kHz upper residual, detect
  discrete upper-band acceleration and rapid direction changes, and apply a soft
  knee with very fast attack and transparent release to the upper residual only.
  Do not attenuate lows/mids, foley, needle-surface audio or surface-only renders.
- Drive canvas rotation from worklet effective rate during spin-up, pitch slew,
  braking and release rather than from an instantaneous nominal RPM flag.
- Keep lead-in surface rendering isolated from programme sampling and complete its
  timed transition on the audio clock. Restore record deadwax/locked-groove behavior;
  make clean preview ending a load option.
- Resolve one immutable needle-cue landing `50–140 ms` before the visual aim
  during active programme playback. Use it for both the immediate worklet update
  and the queued core seek. Keep paused and lifted-needle seeks exact.

Acceptance evidence:

- Quantitative traces cover grab latency, motor catch, powered brake and unpowered
  throw in both directions.
- 33⅓/45 wow completes one phase per physical revolution.
- Spectral tests show bounded alias energy for representative 2×/4×/8× sweeps and
  forward/reverse symmetry.
- Stylus-tracing tests retain curvature × squared-velocity behavior, while separate
  programme-limiter tests prove exact bypass, low-frequency preservation, benign
  high-frequency brightness, harsh/sibilant acceleration reduction without
  full-band collapse, bounded stereo linking and transparent release.
- Visual rotation stays within a declared angular tolerance of integrated rendered
  effective rate through start and stop.
- Lead-in cannot advance/leak programme PCM; published records enter run-out while
  preview sources can request a clean end.
- Chrome observes no transient exact-target seek before an active-playback cue
  reaches its physical landing.

## Phase 6 — deterministic capture and replay

- Introduce performance schema v2 with separate `sourceSampleRate` and
  `outputSampleRate`, preset/gate algorithm version, preset/click state, manual
  fader state, motor/needle state and canonical source-frame motion events.
- Record preset, click and manual fader changes as output-frame-timestamped events.
- Scale schema-v1 timestamps during import/replay and default them to Baby/manual.
- Apply replay events inside the worklet at exact sub-quantum frame offsets.
- Snapshot and restore motor, playing, needle, effects, preset/gate and final
  output-gain state in Rust on completion and cancellation; keep host/core
  public state untouched throughout replay.
- Store a stable replay seed and initial platter angle. Initialize replay-only
  mechanics, wow/flutter, noise, filters, limiter and gate state in Rust so a
  take never inherits mutable live DSP phase.
- Validate/migrate IndexedDB records instead of blindly accepting arbitrary shapes.

Acceptance evidence:

- Replaying the same performance twice yields the same gate trace and output hash.
- Mid-gesture preset/click/fader events apply on their exact requested frame.
- Schema-v1 import remains playable with documented defaults.
- Completion and cancellation restore all pre-replay state.

## Phase 7 — public controls, UI, documentation and measurement

- Add public preset/click APIs, fixed fader-curve metadata, distinct stylus-tracing
  and programme acceleration-limit controls, state subscription fields and
  postMessage bridge messages. Public fader-curve tuning remains deliberately
  deferred pending human validation.
- Add real preset and click controls to the canvas plus accessible fallback HTML;
  reflect (but do not drive) audio-owned gate/direction telemetry.
- Publish AudioContext `baseLatency`/`outputLatency` and measured pointer-command-
  apply latency so device-specific problems are observable.
- Correct stale acoustic/windowing documentation and record each intentional
  reference divergence.
- Add a blind DJ validation protocol using matched source material, calibrated
  output level, hidden real/digital conditions, baby/stab/chirp/flare/transform
  tasks, latency ratings and ABX-style identification results.

Acceptance evidence:

- Keyboard, pointer, touch and programmatic control paths select all presets and
  click counts without overwriting the stored manual-fader value. Automatic
  presets drive the effective XFADE from the Rust gate.
- API and README examples match runtime state and schema.
- Headless browser smoke tests report no page/worklet errors and exercise synthetic
  audio loading, normal playback, window replacement, multi-pointer control,
  every preset, capture and replay. The test samples Web Audio callback use by
  phase and checks capture timeline continuity in a dedicated worker.
- The final audit maps every item above to a passing command, trace, browser result
  or explicitly outstanding human validation result.

## Completion boundary

The checkpoint preserves the pre-refactor work. Implementation was committed in
reviewable clock, mechanics, limiter, replay and browser slices. Automated
acceptance is complete for the native, JavaScript, release-WASM and Chrome
harnesses listed above. The remaining deployment claim requires device-xrun and
acoustic latency evidence on target hardware plus the pre-registered DJ
protocol. Those results must be attached here. They cannot be inferred from
automated tests.
