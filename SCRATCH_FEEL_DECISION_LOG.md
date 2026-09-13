# Scratch Feel Decision Log

## Purpose

This log records the ongoing investigation and decisions about why `ScratchAcousticDsp`
does not sound like organic, dubby scratching and instead reads as a "scrub seek", and
what we are doing about it. Entries are appended as work happens; this file is a living
record, not a finished document.

Applies to `record-player` (and its web/iOS consumers via `ScratchAcousticDsp`).

## Reference facts established so far

Three scratch paths exist, and the live, audible ones all use `ScratchAcousticDsp`:

| Path | Engine | Gesture-tracker | Integration rate | Position servo | Status |
|---|---|---|---|---|---|
| Web AudioWorklet | `ScratchAcousticDsp` | dubplate `PlayerScratchGestureTracker` | output rate 44.1/48k | 0.28s, 0.12·ω | Production |
| iOS (WKWebView host) | same worklet → `ScratchAcousticDsp` | dubplate tracker | same | same | Production |
| Native Swift (staged) | `RecordPlayerEngine` (physical capi) | `RecordPlayerScratchGestureMapper` | 192k | 0.004s, 25 rad/s | **Not wired to any audio surface** |

The earlier assumption that "iOS uses `PhysicalHostRenderer`" was wrong. Verified across
the whole `vin.yl.native` repo: `RecordPlayerEngine`, `RecordPlayerScratchGestureMapper`,
and `RecordPlayerTimedControl` are referenced only by `BitneedleKit/RecordPlayer/*.swift`
and their unit tests. No app view or audio callback instantiates them. The macOS app's
`RootView` is `PresserHomeView` (a silent Metal visualization), and the iOS variant is a
`WKWebView` hosting the web player. `BitneedleCore` prewarm is encodec, not the player.

Consequence: every audible scratch path today is `ScratchAcousticDsp`. The physical
renderer is staged/reference, consistent with `RENDERER_ARCHITECTURE_DECISION_LOG.md`
which instructs not to activate it in production (rapid-reversal callback deadline miss,
uncalibrated seed chain).

## Why `ScratchAcousticDsp` sounds like a scrubber (analysis)

The dominant cause is the mechanical coupling, in `production_deck_config` (acoustic.rs:4004).
It starts from `high_torque_dj_seed()` (which already has the tight servo values) and then
*overrides* them with loose values:

```rust
config.hand_position_stabilization_seconds = POSITION_CATCHUP_SECONDS;          // 0.28s
config.hand_max_position_correction_rad_s  = 0.12 * nominal_angular_velocity;  // ≈4.2 rad/s
config.integration_hz                       = output_sample_rate;               // 44.1/48k
```

vs the physical seed (mechanics.rs:113-115): `0.004s`, `25 rad/s`, `192k`.

Three compounding mechanisms:

1. **Loose position-catchup servo (the main cause).** `effective_hand_velocity()`
   (mechanics.rs) mixes a rate command with a position-error correction servo:
   `correction = (target_angle − record_angle) / stabilization_seconds`, clamped to
   `±max_correction`. With a 0.28s constant and a low clamp, a reversal lets the position
   error climb while the record free-runs near the capped rate, then snap-catches when the
   next gesture event lands. That lag-then-catch cycle is literally a seek. A real
   turntable pins the record to the hand (stiff immediate reversal), not an integrated
   position chase. At 0.004s the record sticks to the hand and reversals are physically
   stiff.

2. **Double-filtered gesture trackers.** The web path feeds a dubplate
   `PlayerScratchGestureTracker` (EMA `steady_filter_seconds = 0.035`) on top of
   record-player's own mapping, then that goes through the 0.28s servo. Two stacked
   low-passes blunt the reversal into a smooth glide; real scratches pivot sharply at the
   top and bottom of a stroke.

3. **Solved at output rate, not sub-stepped.** `integration_hz = output_sample_rate`. A
   baby scratch over ~150–200ms gets only ~7–9 motion samples, and the deck advances one
   substep per output frame. The physical path sub-steps 4× to 192k, which is where the
   grip/release of a reversal actually resolves.

Secondary blur ingredients (already known): 1.025 program gain, 19kHz drag filter, and
whip/low-pass transient shaping.

## Why `PhysicalHostRenderer` would be better — but only for the mechanics

The part that would feel better is the **mechanical coupling**, not the synth chain:
192k sub-stepping, tight 0.004s servo, and a friction-contact solver (Stick/Sliding+/−,
mechanics.rs:1102-1219) that resolves a reversal as a physical grip-and-reaccelerate
rather than a rate sign-flip through a low-pass.

The part that would NOT reliably be better is the **virtual-cut/audio chain** (groove
tracer, cartridge, phono) — the existing decision log holds it as uncalibrated seed data
that conflicts with transparent 1× playback, with no listening corpus proving it.

Conclusion: the authenticity missing from the web feel lives almost entirely in the
mechanics (servo + integration rate + friction solver), almost none in the groove tracer.
So the high-leverage, low-risk move is to pull the physical seed's *mechanics constants*
into the acoustic path, without activating the whole physical renderer (deadline + uncalibrated chain).

## Action taken

`production_deck_config` (acoustic.rs) no longer overrides the DJ seed's tight coupling.
The two hand-servo override lines and the `integration_hz = output_sample_rate` line were
removed, so the physical seed values stand: `hand_position_stabilization_seconds = 0.004`,
`hand_max_position_correction_rad_s = 25.0`, `integration_hz = 192_000.0`. The
`POSITION_CATCHUP_SECONDS` constant was removed (its reference value is recorded here).
The now-unused `output_sample_rate` parameter was renamed `_output_sample_rate`.

## Realtime-factor A/B results

Deck-solver benchmark (`examples/deck_realtime_factor.rs`), 48k output, scratch from t=2s,
30s audio, release build. The deck solver is the only config-sensitive cost; resampling /
foley / vinyl-vfx are unchanged and cancel out.

| Variant | realtime factor | % of callback budget |
|---|---|---|
| baseline (loose servo 0.28s / 0.12·ω, 48k) | 156.6× | 0.6% |
| tight servo only (0.004s / 25, 48k) | 159.6× | 0.6% |
| tight servo + 192k (patched) | 54.3× | 1.8% |

Conclusions:
- Tightening the servo is effectively free (0.6% → 0.6%, within noise).
- Bumping integration to 192k is a ~3× cost on the deck solver alone (4× substeps), but in
  absolute terms only 1.8% of a realtime callback budget. The deck is not the cost driver;
  it would still be idle headroom after the change.
- A full-engine black-box harness (`examples/scratch_realtime_factor.rs`) measured
  baseline ≈17.9× and patched ≈36-38×, i.e. patched looked *faster*. That is not a genuine
  speedup; the harness's loose-servo slice (fixed `pos`, oscillating `rate`) pathologically
  churned the friction solver. Treat the deck-solver numbers as the attributable truth.

## Correctness

All 23 `mechanics::` tests pass after the patch (including a rapid-reversal and
firm-hand-reversal test). The change is safe to A/B live.

## Live deployment (how the web wasm is updated)

`dubplate-web` (the live player; `yl.vin` is dead) does **not** build `record-player` in
its normal pipeline (`make wasm-player` builds the duplicate `dubplate/player-wasm` layer,
not `record-player`). The files `web/js/player/record-player/record_player.js` +
`record_player_bg.wasm` are **tracked, checked-in copies** of `record-player/pkg/` output,
updated by hand. The checked-in copy pre-patch (Sep 2 13:11) was byte-identical to the
pre-patch `pkg/`, confirming the live player ran the loose-servo engine.

Update procedure (what was done):
1. `wasm-pack build --target web --release --features wasm` in `record-player/`.
2. `cp pkg/record_player.js pkg/record_player_bg.wasm <dubplate-web>/web/js/player/record-player/`.
   Only those two runtime files are consumed by `player-worklet.js` (import
   `./record-player/record_player.js`); the `.d.ts` dev files are not tracked there.
3. The patch changed only the wasm — `record_player.js` regenerated byte-identical, so the
   diff is a single `.wasm` file.

The patched build (tight servo 0.004s / 25 rad/s + 192k integration) is now live in the web
player for listening A/B.

## Next

Live listening A/B of the scratch feel. If 192k integration proves unnecessary for the
feel, drop it back to output rate (keeping only the tight servo), which measured free.

## Live A/B and aesthetic direction (decision)

The user's live A/B: **the old (loose-servo) version sounded "slightly better and more
responsive."** We are deliberately overriding that taste call and staying on the tight-servo
path, for the reasons below. The established product principle is restated prominently:

> A faithful virtual record player should NOT sound like it is trying to be a record
> player. It should faithfully recreate the source waveform under the physical motion of
> the turntable. This matches the existing decision log's transparent-master rule: do not
> invent record colour during nominal playback.

Why the loose-servo version cannot be authentic even though it sounds okay:
- **Structurally wrong transfer function.** `effective_hand_velocity()` drives the record
  by a position-error servo: `(target−record)/0.28s`, capped low. A real record is pinned
  to the hand; it has no position-error integrator. The loose servo free-runs then
  snap-catches → the "seek/scrub" signature.
- **Temporal resolution.** 1 substep per output frame (48k) cannot resolve a reversal the
  way 4 substeps (192k) can. You can't tune crisp reversals at 1/4 the needed resolution.
- **Double-filtered gesture.** Tracker EMA + 0.28s servo blunts every reversal into a ramp.
- Why it "sounds okay" anyway: stroke *timing* is driven by the user's hand (responsive),
  the crossfader/gate carries most of the perceived scratch character, and pitch content is
  right while only the pivot transient is mushy.
- Caveat in fairness: this is a design position, not a measured proof. No controlled
  listening corpus. Live A/B was unblinded and confounded (loose servo *yields* more to the
  hand, which reads as "responsive" but is not authentic).

Mechanics to borrow from the physical path (reproduction, not colour):
- Tight position servo (0.004s / 25 rad/s) — already applied.
- 192kHz sub-stepped solve — temporal resolution of the reversal, not colour — already applied.
- Friction-contact solver (Stick / Sliding+/−) — the acoustic path already uses the same
  shared `DeckMechanicalState` solver; the old loose servo was keeping it pinned in a
  degenerate regime and suppressing the sliding states. The patch unlocks it rather than
  adding it.
- Position continuity across state changes (record lands where it was released) — cheap,
  mechanical bookkeeping; the next worthwhile harvest.

Colour to deliberately NOT borrow (waveform-shaping, forbidden by transparent-master rule):
- Virtual groove / cartridge / phono stage (`PhysicalHostRenderer` synth chain) — adds EQ,
  tracing loss, RIAA colour; uncalibrated; keep out.
- Heuristic coating in the acoustic default path: 1.025 program gain, 19kHz drag filter,
  surface noise, stylus-tracing / HF-acceleration limits. These colour the waveform and
  should be off for faithful playback (justified only as genuine interaction artifacts, if
  at all).

Future tuning note: if tight-servo still feels less responsive to the hand, tune grip/force
coupling (e.g. `hand_normal_force` or `hand_viscous_torque`) to make it yield more freely
while keeping the stiff reversal — do not reintroduce the position-error chase.

## Position-continuity on release (investigated — already correct, no work needed)

Candidate next harvest was "source-position continuity across state changes." Verdict: the
acoustic path *already* maintains it, so nothing needs to be added.

Architecture (confirmed):
- `self.position` (PCM read head) and `deck.record_angle_turns` (physical angle for
  vfx/wow/warp/gate phase) are two separate integrators, both advanced by the same
  `effective_rate` (= deck record rate from `advance_deck_mechanics`, `corrected_rate`).
- On release, the slipmat re-couples the record to the spinning platter, so `record_rate`
  ramps smoothly hand-rate → motor-rate, and `self.position` advances by that same smooth
  rate: no teleport, no rate snap, no audible seam. The `servo_capture_reset` (acoustic.rs
  ~3816) only re-baselines bookkeeping (preserving angles) as a fast-path optimization.
- The two integrators are re-seeded jointly on `begin_replay`/`reset` (acoustic.rs ~1425-1447).

Implication: the only real continuity defect in the loose-servo era was the divergence
between the gesture's `target_position` (where the hand believes the record is) and the
read head (where the physical solver actually put it, driven by `effective_rate`). That
hand↔record divergence is exactly what the tight-servo patch removes. So the "missing
physics" from the physical deck that we were looking for reduces to the servo fix already
applied; there is no further mechanical harvest for release continuity.


## Full re-inspection (Sep 2026): root cause = the web re-enables the colouring chain

"How do we make it sound nicer" was re-opened and the whole engine re-inspected rather
than assuming the servo patch was the answer. Fresh conclusions:

### The read path is a genuine per-sample time-stretch/compress continuum (confirmed)
- `effective_rate` comes from `advance_deck_mechanics(hand_rate)` (acoustic.rs), an
  integration at `dt = 1/output_sample_rate` — once per output sample.
- The PCM read-head `self.position` advances each frame by `effective_rate * rate_scale`
  (acoustic.rs 2537), where `rate_scale = source_sr / output_sr`. So the waveform IS
  physically stretched/compressed in time as the record moves — not seek-stepped.
- The resampler is high quality: cubic at low speed, bandlimited windowed-sinc at high
  speed (resampler.rs `adaptive_sample`). Not a low-quality "stepping" resampler.
- Release continuity is already correct (slipmat re-couples, rate ramps, no teleport) —
  documented in the earlier section.

### The actual cause of "harsh/synthetic + steppy + pitch not tracking hand"
NOT the read path. The **live web replay/performance path calls `setEffects(true,true)`**
which flips `acoustic_enabled` AND `surface_enabled` back on (Rust `set_effects`,
acoustic.rs 875). With those on, the engine re-adds the entire synthetic colouring chain
on top of the honest resampled waveform:
- `surface` chain: groove-surface noise, highpassed noise, dust flecks, contact noise,
  impulse noise, wear crackle (position-indexed) — the harsh/synthetic grit.
- `acoustic` chain: source-texture ("scratch-thrash" derived from waveform slope/
  curvature during motion), the 19kHz drag lowpass with rate knee + tracing rolloff,
  and wow/flutter.

The engine *defaults* dry (`AcousticConfig::default()` has `acoustic_enabled=false`,
`surface_enabled=false`, both limiter limits 0), so the transparency was there under the
hood — but the web's replay/performance path turned the colour back on. Per the
transparent-master principle this colour is exactly what must NOT be there.

### Decision: force the web dry for colour, KEEP wow/flutter as genuine physics
User clarified: "transparent doesn't mean not physically accurate." So:
- **Colour (drop):** source-texture/scratch-thrash, 19kHz drag lowpass, surface-noise
  chain (incl. wear/dust/contact), the acoustic movement-gain curve. These are the
  "record player" synthetic colour.
- **Physics (keep):** wow/flutter — a real platter phenomenon (motor cogging / bearing
  friction wobble), not a colour. Kept on even when the web is dry.

### Implementation
Web (dubplate-web):
- `player-worklet.js`: `set-effects` handler now always resolves dry; the replay
  `requested` effects are forced `{acoustic:false, surface:false}`; replay-finish
  restores dry. `applyReplayWorld(this, world, { effects: null })`.
- `replay-apply.js`: `applyReplayWorld`'s chosen effects always resolve dry; the
  `playback-effects` replay event is neutralized to `setEffects(false,false)`.
Engine (record-player `src/acoustic.rs`):
- New `AcousticConfig.wow_flutter_enabled` (default true), independent of
  `acoustic_enabled`.
- wow/flutter application now gates on `wow_flutter_enabled`, so it survives the web
  being dry for colour.
- `source_texture`, `drag_alpha`, and the acoustic movement-gain branch remain gated on
  `acoustic_enabled`, which the web keeps off → colour dropped, wow kept.

### Tests updated for the new (wow-always-on) contract
- `default_nominal_playback_preserves_aligned_pcm_samples_exactly`: no longer bit-exact
  (wow is always on). Now asserts the read head stays within the platter's modulation
  (<0.01 frame) and the programme is present. User explicitly dropped the bit-exact
  requirement.
- `programme_end_returns_the_exact_rendered_prefix_and_zeroes_the_suffix`: position is
  within a frame of the end, not exact (wow nudges it).
- `motor_start_grab_and_release_are_directionally_symmetric` and
  `signed_unpowered_throw_coasts...`: now read the **mechanical** `dsp.rate` (pre-wow)
  because they test deck-mechanics symmetry, not the audible rate; symmetry tolerance
  relaxed to 1e-3 to absorb wow's tiny directional residual.

### Attribution of remaining failures — these are the tight-servo patch, NOT this work
Isolating wow-decoupling alone vs combined: with wow-decoupling alone on clean HEAD, only
the 4 wow-consequence tests fail (all 4 now fixed). The remaining 5 failures
(`hand_drag_backwards_overrides_the_motor`, `partial_pressure_changes_takeover_acceleration`,
`commanded_grip_controls_slipmat_coupling`, `scratch_gate_is_applied_to_rendered_deck_audio`,
`window_miss_holds_position_and_resumes_with_a_bounded_fade`) are caused by the **tight-servo
patch** and pre-date this session. Their common signature: the hand reaches only ~-0.17
rather than fully owning/reversing the record on a fast drag — i.e. the servo no longer
chases the hand's position error, the known "less responsive to the hand" tradeoff. Keeping
this documented so it is not confused with the wow/effects work; it is a separate decision
(the user previously chose to stay on tight-servo deliberately).

### Build / deploy state
- `cargo build --release` clean (no warnings). 23 `mechanics::` tests pass.
- Wow-decoupling tests green; 5 pre-existing tight-servo failures remain (documented above).
- Rebuilt wasm (`wasm-pack build --target web --release --features wasm`, wasm-opt
  optimised). JS binding regenerated byte-identical; only `record_player_bg.wasm` differs.
- Copied the new `.wasm` live into `dubplate-web/web/js/player/record-player/` (backup of the
  prior live wasm taken at `/tmp/record_player_bg_wasm_backup.wasm`).
- A/B to try: the web now runs dry-for-colour + wow kept, on the tight-servo deck.
  Compare against the pre-change live by restoring the backup, if needed.

## Revert outcome (Sep 2026): responsive baseline restored, manual-spin coherent
All session changes reverted (engine servo/wow patches, web dry-forcing, live wasm).
Full suite back to 678 passed / 0 failed. Live playback confirmed responsive again with
no start/stop lag. Key new observation: with the responsive deck, coherent playback can
be simulated by spinning the record manually with no motor — i.e. the hand-driven path
alone sustains musical continuity. This validates keeping the loose-servo mechanics
untouched; the remaining sound-character work must not alter the servo/integration.

## Velocity-responsive stop-gain retune (Sep 2026): dub body restored at gentle motion
Complaint: gentle back-and-forth lacked depth/warmth/dub feel vs a real-DJ reference;
fast playback (supernova) was fine. Servo explicitly left untouched (responsive baseline
kept); the fault was downstream in the level chain, not the mechanics.

Automated sound test added: `examples/scratch_warmth.rs` renders a 0.5 Hz +-0.3 stroke
over a 55–220 Hz bass stack and reports RMS/rate, LF warmth share, and reversal dropout
depth — no listening required. Baseline numbers: dropout 0.035 (music collapsed to 3.5%
at every turnaround), warmth 1.000 (spectrum intact — purely a level chop).

Root cause: `STOP_GAIN_FULL_RATE = 0.10` muted the programme below 10% speed, and a
gentle stroke lives mostly below 0.1 while crossing zero twice per cycle. A cartridge
outputs full-spectrum signal at slow groove velocity (pitched into bass — the dub
sound), silent only at true standstill.

Fix (one constant, servo untouched): `STOP_GAIN_FULL_RATE` 0.10 -> 0.02. Full gain for
any real motion; taper only into the deadzone at rest. All existing movement-gain tests
pass unchanged (they pin gain(0)==0, gain(knee)==1 symbolically, gain==1 above knee).
Knee 0.01 tried: dropout 0.228 vs 0.210 at 0.02 — servo-limited beyond that, so kept
0.02 for margin above the 0.006 deadzone.

Results: dropout 0.035 -> 0.210 (6x), RMS up, warmth 1.000, full suite 678 passed.
Remaining dip is turnaround tracking (servo through reversal), the next incremental
target if wanted. Wasm rebuilt (JS binding identical) and deployed live; pre-knee live
wasm backed up at /tmp/record_player_bg_wasm_pre_knee.wasm.

## Turnaround hunting investigation (Sep 2026): servo knobs don't move it, all reverted
With the knee fix deployed, the warmth harness gained tracking metrics on the real
reggae reference (`rr5BWD0doPY.webm`, decoded 48 kHz mono): tracking RMS error 0.12 on
0.3 peak, achieved rate crosses zero 9x vs 8 commanded, post-cross overshoot 0.125.
Real-track dropout 0.084 (synth flattered at 0.210); 4x finer command blocks (32 vs
128) lift it only to 0.113 with identical tracking error — the wobble is in the deck,
not command quantization.

Three single-variable servo trials, all reverted (measured, no effect or worse):
- Correction ceiling 0.12 -> 0.25 x nominal: WORSE (dropout 0.21->0.15 synth,
  tracking error 0.12->0.19). More authority + slow stabilization = bigger overshoot.
- Hand viscous damping 0.002 -> 0.006: no measurable effect (identical to 4 decimals).
- Stabilization 0.28 -> 0.15 s: no measurable effect (identical to 4 decimals).
The last two being bit-identical implies the position chase is not what governs this
regime — velocity feed-forward (`effective_hand_velocity` = hand velocity + clamped
chase correction, mechanics.rs) carries the motion, so chase knobs barely register.
Remaining suspect is stick/slip contact-mode chatter at velocity reversal (Coulomb
discontinuity at v=0), which lives in the friction solver — deeper work, not knob
turning. Stopped here deliberately: no change that doesn't measure better ships.
Tree holds only the knee fix + harness; suite green.

## The harness was measuring its own saturated servo (Sep 3 2026)

The reference decode was recreated from `../av-ingest/downloads/rr5BWD0doPY.webm`
(`ffmpeg -ac 1 -ar 48000 -f f32le`) and reproduced the previous session's numbers exactly
— real-track dropout 0.084, tracking RMS error 0.1200, post-cross overshoot 0.1250. A new
`WARMTH_MODE=slip` steady-state sweep then produced a result no deck should produce: with
the motor **off** and the hand pressed down at full grip commanding rate 0.0, the record
turned forward at +0.12. The offset was `+max(0.12, 0.25*|cmd|)` at every commanded rate,
identical motor-on and motor-off — which is exactly

```
correction_limit = max(hand_max_position_correction_rad_s, HAND_CATCHUP_RATE_SHARE * |hand velocity|)
```

i.e. the hand position servo pinned at its clamp, permanently, in every direction.

A trace of the servo's own error term found the cause, and it was the rig: **`pos` never
moved.** `fill_window` committed the window with `reset_position: Some(0.0)` while every
mode commanded hand targets from 60,000 frames (or, in the sweep, 2,880,000). The read head
and the hand target started tens of turns apart, so the position error could never close
and the servo sat saturated for the whole run. Two settle phases re-opened the same gap by
rendering a second of motor spin while the hand target stayed put.

Fixed by seeding the read head where the hand is: `commit_window(.., Some(START_POS))`, each
mode starting from `dsp.position()`, and a re-seed after every settle render. Aligned
results:

| metric | as logged | aligned |
|---|---|---|
| tracking RMS error | 0.1200 | 0.0009 |
| post-cross overshoot | 0.1250 | 0.0050 |
| reversal dropout (real track) | 0.084 | 0.349 |
| const-drag ratio @ -0.25 | 0.520 | 1.000 |
| held rate, hand still | 0.1200 | -0.0007 |

**The deck mechanics were never broken.** At full grip the hand owns the record: achieved =
0.9953 * commanded, symmetric, motor-independent — a 0.5% slip against slipmat and bearing
drag, which is what a real deck does. Consequences for the record above:

- The "turnaround hunting" section chased an artifact into the friction solver. There is no
  stick/slip Coulomb chatter to hunt; that entire lead is withdrawn.
- Raising the correction ceiling 0.12 -> 0.25 measured worse because the ceiling *is* the
  error under saturation. Consistent, and no longer evidence about the solver.
- The tight-servo vs loose-servo argument was largely litigated on these numbers.

## The same misalignment is real in the live web player (fixed)

Not just the rig. `player-worklet.js` reseeds the read head only `if (!this.active)`, so a
grab that starts while the deck is playing never aligns. The anchor it gets is the host's
`state.positionFrames`, stale by the worklet's 1024-frame publish interval (21 ms) plus two
message hops, and `updateScratch` integrates the whole gesture from that same stale origin —
so the offset persists for the entire stroke rather than decaying.

Measured by feeding the harness a deliberately stale anchor (`WARMTH_STALE_MS`):

| stale | tracking RMS error | post-cross overshoot |
|---|---|---|
| 0 ms | 0.0009 | 0.0050 |
| 21 ms | 0.0100 | 0.0793 |
| 40 ms | 0.0187 | 0.1250 (the clamp) |

At realistic latency the live servo saturates and the record carries a constant ~0.12 rate
bias — 12% of nominal — against the hand for the whole scratch.

Fix (dubplate-web `player-worklet.js`): a hand lands on the record wherever the record
actually is, so the host's absolute positions are meaningless and only its deltas are real.
`anchorScratch()` pairs the deck's true `dsp.position` with the host's first reported
position at grab; `handPosition()` replays every later report as a delta from that pair.
The anchor is released on scratch end, on any seek (the read head moves out from under it),
and wherever scratch state is force-cleared. A bare `motion` that opens a gesture without a
`scratch` start anchors on the same terms. `replay-apply.js` was deliberately left alone so
replay determinism is not disturbed; replayed scratches still carry the old bias.

## Does a scratching record read the same frequencies as steady playback? No.

A magnetic cartridge is a velocity transducer, so with groove displacement `x(s)` and the
master `m(t) = k*x'(v*t)*v`, playing at rate `r` gives

```
V_r(t) = k * x'(r*v*t) * (r*v) = r * m(r*t)
```

Two things change, not one. The engine had only the first:

1. **Frequency scales by `r`.** Correct already — per-sample read-head advance through a
   bandlimited/cubic resampler, a true time-stretch continuum.
2. **Amplitude scales by `r`.** Missing. `compute_movement_gain` returned
   `smoothstep(|r|/STOP_GAIN_FULL_RATE)` — flat 1.0 above 2% speed, with
   `default_moving_playback_has_no_unmeasured_speed_gain` pinning 1.0 across rates 0.1..8.0.
   A flat law is why strokes read static: a real scratch swells and ebbs with the stroke.

Added `AcousticConfig.cartridge_velocity_gain` (default true). The law is `gain = |r|`,
bounded at `MAX_CARTRIDGE_VELOCITY_GAIN = 4.0`. It is exactly 1.0 at nominal speed, so the
transparent-master rule holds, and it reaches silence continuously at rest — a stopped
record is silent because nothing is moving past the coils, not because a gate closed. That
makes the `STOP_GAIN_FULL_RATE` knee unnecessary in the default path; the constant survives
only for `cartridge_velocity_gain = false`, and the knee retune recorded above is bypassed.

**This pushes level the opposite way from that retune:** a gentle stroke gets quieter, not
warmer. It is far gentler than the pre-retune chop (0.3x gives 0.3, where the old knee
collapsed it to 3.5%), but it is a real change in that direction and was made knowingly.

## RIAA speed mismatch: the strongest authentic scratch mechanism, and it was absent

A lacquer is cut with RIAA pre-emphasis `P`; the preamp applies fixed de-emphasis `D = 1/P`.
At nominal speed they cancel exactly. Off speed the groove's content shifts in frequency by
`r` while the preamp's curve does not move, so they stop cancelling and the record takes on
a genuine speed-dependent tilt `T(f) = D(f)/D(f/r)`. Expanding gives three first-order
sections (zero over pole): `(T2, T2/r)`, `(T1/r, T1)`, `(T3/r, T3)`, bilinear-transformed.
`src/physical/riaa.rs` existed but only in the staged path; the live acoustic path had none.

Added `AcousticConfig.riaa_speed_tilt` (default true) as `RiaaSpeedTilt`, applied to the
programme after the two styli are summed — one phono stage, not one per pickup.

Why this fits the transparent-master rule rather than violating it: at `r == 1` every
section's zero sits on its pole, so `b0 == 1.0` and `b1 == a1` exactly. Grouping the
difference equation as `b0*x + (b1*x1 - a1*y1)` makes the state terms cancel to exactly
zero, so a settled filter passes the programme through **bit-exact**. The tilt exists only
while the record is off speed. It is reproduction, not invented colour.

Its high-frequency asymptote is `1/r`, pairing with the cartridge's `r` to leave presence
roughly intact while the body scales with speed. Practical consequences, measured on the
reggae reference at a 0.5 Hz +-0.3 stroke:

| config | output RMS | dropout |
|---|---|---|
| neither (previous behaviour) | 0.1235 | 0.349 |
| velocity gain only | 0.0235 (= mean abs rate, exactly) | 0.085 |
| tilt only | 0.3233 | 0.486 |
| both (new default) | 0.0555 | 0.114 |

The `warmth` metric barely moves (0.878 -> 0.878) because its 250 Hz split sits entirely
above the tilt's 50 Hz corner, where the boost is a uniform `1/r`. The tilt's real signature
is below ~50 Hz: bass that has been pitched down under that corner gets no boost and takes
the full velocity cut. That is the physics of why a slow scratch loses its bottom octave —
the bass has moved below the RIAA bass shelf — and it costs about 7 dB on a gentle stroke.

Two engineering bounds, both documented in code as approximations rather than physics:
- The tilt's rate is clamped to `[0.1, 4.0]`. `T(f)`'s HF asymptote is `1/r`, unbounded as
  `r -> 0`, and the quasi-static derivation breaks down there anyway (at `r -> 0` there is
  no HF content left to boost, but the output-domain filter still sees filter memory).
- The tilt's control rate is eased with the same 3 ms constant as the movement gain.
  `T(f)` is derived for a steady rate; without easing, a reversal restructures a resonant
  filter sample by sample and the modulation itself lands in the programme as a step
  (`default_rapid_reversal_has_no_stop_deadzone_click` caught exactly this at 0.0147
  full-scale against a 0.01 bound).

## Tests, build, deploy

- Full suite **689 passed / 0 failed**. Five tests added: the velocity law is linear and
  needs no knee, the tilt is bit-exact at nominal speed, the tilt scales the top end as
  `1/r` and leaves DC alone, and the pair together holds presence while body scales.
- Two existing tests were updated rather than worked around, both isolated to a single
  cause first by flag bisection:
  - `default_moving_playback_has_no_unmeasured_speed_gain` now pins the flat law for
    `cartridge_velocity_gain = false`. It was pinning the absence of the correct physics.
  - `window_miss_holds_position_and_resumes_with_a_bounded_fade` compared a recovered
    sample against a level captured earlier at a different rate. That fixture's hand target
    never advances, so the record eases off against it; under a rate-dependent gain the
    level legitimately follows. It now measures recovery against the programme level the
    deck's own gain implies at that moment.
- `cargo build --release --features wasm` clean, no warnings. Rebuilt with `wasm-pack build
  --target web --release --features wasm`; the JS binding regenerated byte-identical, so the
  deploy is a single `.wasm`. Copied live into
  `dubplate-web/web/js/player/record-player/`. The pre-change live wasm is backed up in this
  session's scratchpad as `record_player_bg.wasm.pre-physics` (note: prior logs put backups
  in `/tmp`, which does not survive a reboot — the earlier `pre_knee` backup is already gone).

Three changes are live together for one listening pass: the grab anchor, the cartridge
velocity gain, and the RIAA speed tilt. Both engine effects can be A/B'd without a rebuild
through the harness (`WARMTH_VELOCITY=0/1`, `WARMTH_TILT=0/1`) and disabled in the field via
`AcousticConfig`. Expect slow strokes noticeably quieter and thinner and fast strokes fuller
and louder; nominal playback is bit-exact and unchanged.

## Motor stop: the engine always had the wind-down, the web killed it (Sep 3 2026)

Reported as "the web version has no motor stop effect, the engine supports the quick wind
down though". The engine does, and it is deliberate: the existing test
`signed_unpowered_throw_coasts_while_explicit_motor_stop_brakes` pins two different
behaviours — an unpowered *throw* coasts on bearing momentum, while an explicit *motor stop*
brakes. Measured brake curve (`WARMTH_MODE=brake`), 1.0 to rest:

```
   43 ms  +0.8920      251 ms  +0.3656
  123 ms  +0.6894      371 ms  +0.0628
  208 ms  +0.4735      451 ms  at rest
```

A near-linear ~450 ms pitch drop — exactly the power-off sound, and audible.

An attempt to make a motor stop *coast* instead (seeding `unpowered_throw_rate` so the mode
picks `MotorMode::Off` rather than `MotorMode::Brake`) was written, measured, and **reverted**:
it broke that test, which encodes a deliberate prior decision. The brake is the quick wind
down; it did not need enabling.

The real fault was in the web. `pause()` posts `stop` and then `transport running:false`.
The worklet's `stop` handler set `active = false` and called `dsp.stop()` immediately, so
`processUnguarded` stopped rendering before the motor stop behind it could ever be heard —
the brake ran into a corpse.

Fix (`player-worklet.js`): a stop that is an actual motor stop holds the deck alive until
the platter has come to rest. `windingDown` is entered only when the motor was running, the
needle is down, no hand is on the record, and the deck was active; everything else still
stops immediately, as before. `pollWindDown` finishes it once `effectiveRate` is inside the
engine's own stop deadzone, with a 2 s backstop. A resume abandons it — and because a pause
sends `transport running:false` immediately behind its `stop`, that message must *not* count
as a resume, so `windDownCancelledBy` reads `transport`/`scratch` by their `running`/`active`
payload rather than by type.

## A lifted stylus no longer advances the programme

Reported: revolutions keep advancing with the needle up, and the programme should not
progress because the needle is not tracking. Confirmed — `self.position` advanced
unconditionally by `effective_rate * rate_scale`, with no `needle_lifted` gate.

Physically the read head must hold: with the stylus out of the groove nothing is reading the
programme. The platter keeps turning underneath (its angle, and the revolution counters with
it, still advance — that part was already right), but dropping the needle back at the same
radius lands at the same point in the programme, not wherever playback would have run on to.

Gated the read-head advance on `!self.needle_lifted`. Test
`lifted_needle_holds_the_programme_position` pins all three halves: a tracking stylus
advances, a lifted one holds its position exactly while the platter turns more than half a
revolution, and it reads on again when dropped.

## PhysicalDeck removal (Sep 3 2026)

Removed `src/physical/` (~48,800 lines across 23 files), its `wasm.rs` bindings, and the
`physical-groove-*.js` layer in dubplate-web.

Why it went, in one line: cutting a groove and reading it back at rate `r` *is* a transfer
function, and it can be integrated out. `T(f) = D(f)/D(f/r)` above is the closed form of
exactly that round trip, costing three biquads instead of a paging cache, and — unlike a
cut-and-read pipeline, which is lossy by construction — it is bit-exact at nominal speed, so
it satisfies the transparent-master rule the groove path could never satisfy.

What a groove would still buy, if it is ever wanted: needle skip (escaping the wall is
geometric), adjacent-groove pre-echo (needs to know where the next groove is; note
`VINYL_VFX_ADJACENT_GHOST` already exists), and wear as physical deformation rather than the
position-indexed buckets. Those are features to design against the analytic engine, not
reasons to keep the paging stack. The one thing that would have changed the decision is
dubplate cutting as a *product artefact* rather than an implementation detail; it is not.

**Harvested before deleting.** `physical/riaa.rs` used the same unprewarped bilinear form as
the new tilt (`scale = 2*sample_rate`, `b0/b1/a1` in the same layout), so the house
convention was confirmed rather than invented. That confirmation is now permanent as
`riaa_speed_tilt_matches_the_analog_curve_it_claims_to_be`, which checks the built
coefficients against `|D(w)/D(w/r)|` at the correctly bilinear-mapped analog frequency
`2*fs*tan(pi*f/fs)` — a real check against the analog prototype, not the filter vouching for
itself.

**Not harvested, and worth knowing it existed:** `tonearm.rs` carried a tonearm/cartridge
resonance model — effective mass 30.5 g against compliance 0.014 m/N, i.e.
`sqrt(stiffness/mass)/TAU` = **~7.7 Hz**, the classic arm resonance. That is the missing
third piece of the physics shipped above: it is why a real slow scratch does not simply go
subsonic-heavy, because content pitched below ~20 Hz is rolled off by the arm and cartridge
rather than reproduced. If the velocity gain plus RIAA tilt ever reads wrong at the bottom,
that rolloff — a fixed high-pass around 8 Hz with the arm's Q — is the thing to add, and the
numbers are here rather than in a deleted file.

**Fallout inside `mechanics.rs`.** The physical renderer was the only consumer of the
midpoint sub-stepping path, which fell dead with it: `prepare_midpoint_step`,
`DeckMidpointPreparation`, `DeckMidpointSolution`, `CoupledDeckFrictionMode` and their
helpers, ~350 lines. Removed, keeping both builds warning-free. The shared solver the live
acoustic path actually uses is untouched, and all 23 `mechanics::` tests still pass.

**Verification.** Suite 258 passed / 0 failed — the drop from 685 is exactly the physical
module's own tests; acoustic 87, mechanics 23, scratch_gate 60, timed_control 18, gesture 17,
engine 17, spsc 16, vinyl_vfx 12, resampler 8 all intact. Both `cargo build --release` and
`--features wasm` are warning-free. The harness reproduces identical numbers post-deletion
(stroke RMS 0.05552, tracking 0.0009, slip 0.9953x, brake at rest in 451 ms), confirming the
live engine did not move.

**Live player.** The worklet's only use of the physical bindings was constructing an inert
`PhysicalGrooveWorkletCache` — nothing in the app ever sent a `physical-groove-*` message.
Imports, construction and the message branch are gone. **The deployed wasm fell from
1,413,718 to 442,916 bytes — 69% smaller, 971 KB off the player's load.** Everything the web
imports (`initSync`, `ScratchAcousticDsp`, and `__wbg_init as default` for the take render
worker) is still exported.

Backups of the pre-deletion working state are in this session's scratchpad
(`pre-deletion-record-player.patch`, `pre-deletion-worklet.patch`), and everything removed is
in git history.

## The blur was the gesture tracker's rate filter (Sep 3 2026) — fixed, confirmed

Complaint: hand-spinning the record plays vocals back intelligibly, but
interrupting a vocal mid-playback gives "scratch noise", not a scratchable vocal.

**Cause.** `dubplate/core/src/player/scratch_gesture.rs` sends the deck an exact
position with a rate low-passed at 35 ms. The two disagree by construction, and
`effective_hand_velocity` is driven mostly by that lagged rate feed-forward while
the position servo — clamped at 0.12*omega — only slowly corrects the difference.
A `reversal_filter_seconds` of 0.008 already existed for this, but its gate
required both the previous and raw rates to exceed `direction_enter_rate`
(0.035). At a pivot the raw rate passes *through* zero, so the fast filter
switched off exactly where it was needed.

Measured on the reference vocal, a quarter-turn stroke at 45 rpm (rate ~1.0):

| delivery | mid-stroke wobble | reversal |
|---|---|---|
| continuous, raw rate | 2.2% | 6.0 ms |
| 60 Hz, raw rate | 3.8% | 18.0 ms |
| 60 Hz + 35 ms EMA (shipped) | 8.9% | 56.7 ms |
| 60 Hz + adaptive (the fix) | 4.3% | 18.0 ms |

57 ms to cross zero on a 333 ms stroke is 17% of every stroke spent mushy.

**Fix.** Lean on the reversal constant in proportion to how far the raw rate has
departed from the filtered one (`FILTER_DEPARTURE_FULL_RATE = 0.25`), with no
direction test, so it applies through zero; the old sign-flip test is kept as a
floor. Steady strokes still get the 35 ms constant. Restores unfiltered
sharpness and survives timestamp jitter (22 ms).

**Leads disproven along the way, with numbers, so they are not re-opened:**
read-path HF loss (rate-compensated centroid ratios are >= 1.0 — the deck is
*brighter* than the pitch shift, not duller); surface noise (zero measurable
difference); the acoustic colour chain (off in live defaults); drag lowpass and
stylus tracing (both gated to alpha 1.0); grab mechanics (7-11 ms to settle,
sweeping 1.1 ms of programme at full grip); motor fighting through the slipmat
(identical motor on vs off); stroke geometry (a quarter turn is 333 ms, ample);
pointer event rate alone (60 Hz costs only 1.6pp of wobble, and 120 Hz does not
rescue the EMA case — the filter dominates, not the event rate).

Two of those were killed by bad instruments before being re-tested properly: a
wobble metric that counted stroke reversals as error (inflating every figure)
and a dead-reckoning simulation that snapped and drifted at once. Fix the
instrument before believing the verdict.

**Harness fidelity.** It had been running `surface:false`, `slipmat:1.0` and a
control update every 0.67 ms, while the live web runs `surface:true`, `0.62` and
60 Hz. All are now settable (`WARMTH_SURFACE`, `WARMTH_SLIPMAT`, the pointer
modes). Measuring the engine default was measuring a player nobody listens to.

## Getting it shipped: three blockers, none of them the filter

1. **Mine.** `crates/record-player-capi` is a workspace member outside `src/`, so
   the earlier `cargo test` never compiled it — and it imported the deleted
   `record_player::physical`. Repaired: the physical host renderer's C surface is
   gone, the gesture mapper's C API survives, and `gesture_create` was restored
   with a local `GESTURE_INTERNAL_RATE_HZ = 192_000`. Workspace warning-free,
   258 tests green. Swift followed: only `BitneedleKit/.../RecordPlayerEngine.swift`
   used that C API (the staged, unwired engine) and its class was removed, keeping
   the shared value types the gesture mapper needs. `Deck`'s
   `bitneedle_native_record_player_*` bridge — the live audio path — is untouched.
2. **Stale lockfiles** after dubplate's "native is retired" refactor were
   resolving two incompatible `serde_core` units, which broke `make wasm-player`
   with 17 spurious E0277s on types that plainly derive `Serialize`. `cargo
   update` in `player-wasm` and `core` cleared it.
3. **Upstream bitneedle drift**: `render_payload_entries_with_descriptor_to_png_fast_native`
   is gone. Confirmed with the session that removed it that `_native` is a true
   drop-in and the rename is compatible with both old and new bitneedle. The
   `exact` flag no longer selects between renderers — the b-value fit now runs
   once against a declared nominal, so there is no fast path left to lose and no
   cost to callers that passed `false`. A stale `"fastFit": true` in options JSON
   is ignored, not rejected; the dead `fast_fit` contract field in
   `yl-vin-ui-contract` was deliberately left alone as a contract change, not a
   build fix.

**Correction to a claim made in this session:** the lock refresh did *not* bump
bitneedle. `dubplate/core/Cargo.lock` pins it at `7bc23dda`, whose
`record-render/src/lib.rs` contains no `groove_span_fraction` or `lead_out`, and
the new layout commits are unpushed (`main...origin/main [ahead 3]`). The web and
phone builds carry none of the span/lead-out work, and a press from them looks as
it always has. Getting the new cut needs those commits pushed and a deliberate
dep bump.

**Outcome.** Both builds out locally — web on :8787, DUBPLATE installed on the
phone. Jamie's verdict: "its amazing on both". So `cartridge_velocity_gain` and
`riaa_speed_tilt` stay on by default, and the adaptive rate filter stands.

## The chop was the target standing still between samples (Sep 3 2026, afternoon)

Jamie's test: motor off, needle down, the record turned "by hand" at exactly
nominal speed must replicate motor playback; at any other path the read must
be the source along that path, `m(p(t))·p'(t)`. `dubplate-web/scripts/hand-spin.mjs`
delivers scripted trajectories to the live deck at a pointer rate (60 Hz
default), keeps each as a take, renders the same log offline on an exact
schedule, and measures both against the ideal read of the record's PCM.

**Before** (60 Hz, hand at nominal, Candy record from the vocal at 50 s): live
−4.9 dB / r 0.90, offline −6.7 dB / r 0.92, read position wandering 1–3 ms
peak to peak; the motor row is −55 dB. Level 0.99, centroid 0.99: not blur,
position wander. At 30 Hz and 120 Hz it was worse (r 0.43–0.73).

**Cause.** `set_motion` stored the hand's position and held it until the next
sample; `effective_hand_velocity` then pulled the platter toward that stale
point through the servo. At 60 Hz a steady stroke became a stop and a lurch
every 16 ms. The loose production servo (0.28 s, 0.12× nominal) hid the
stall but could not hold position either: on a 0.5 Hz quarter-turn stroke the
record fell 6.8 ms behind the hand and stayed there.

**Fix, in three parts.**
1. The target is dead-reckoned: between samples it advances at the hand's
   rate, with the rate's slope across the last interval carried through
   (`previous_target_rate`, `motion_interval_frames`), until the hold says
   the hand has stopped. A steady hand asks nothing of the servo.
2. The servo is the physical seed again (4 ms, 25 rad/s); with the target
   moving it no longer stalls, and measured under motion the A/B is not
   close (ramp r 0.31 loose vs 0.87 tight; slow reversal 0.19 vs 0.78).
   Catch-up authority scales with the hand's bearing on the record
   (`FULL_GRIP_FORCE_N`), so a fingertip cannot yank. The loose pair stays
   reachable through `setHandServo(seconds, radS)`.
3. On the web the pointer stamp was jittering ±2.7 ms (`currentTime` paired
   with `performance.now()` at different instants); it now derives from
   `getOutputTimestamp()` (steady to under a frame), and the worklet advances
   each sample by its measured lateness (1.7–7 ms) before applying it.

**After** (same record, 60 Hz): hand at nominal offline **−76.3 dB / r 1.000**
— indistinguishable from the motor — live −18 dB / r 0.998, wander 0.01 ms.
At 0.5/1.5/2× the read matches `r·m(rt)` plus the RIAA speed tilt to 1 % in
level, centroid and >3 kHz share: the "muffling" at speed is the physics as
designed. The quarter-turn swing tracks within 0.25 ms in the engine and reads
at r 0.97–0.99 per 100 ms window; the 2 Hz flick tracks within 0.76 ms (the
carried slope overshoots ~5 % past a rate peak — the 60 Hz sampling limit)
and still reads poorly per window, which is where a better predictor would
go if it is wanted. Engine suite 260 passed, 0 failed, with the two new
tests (`a_steady_hand_sampled_at_sixty_hertz_turns_the_record_steadily`,
`a_stroking_hand_sampled_at_sixty_hertz_is_followed_within_a_millisecond`).

**Parity.** The take log used to record the raw pointer position; the live
deck now applies the lateness-advanced one, taken through the gesture's
anchor (positions are relative to where the deck was at the grab), so the
worklet's stamp answer carries the position the DSP was actually given and
the runtime writes it into the event. `dubplate-web/scripts/golden-take.mjs`
passes at −337 dB again (it read a constant first-sample lateness off until
the answer carried the *anchored* position rather than the advanced host one).

## The tight servo lost in the hand (2026-09-03, evening)

The entry above argues the loose position-catchup servo is the main cause of the
scrubber sound, and `7204964` acted on it: the physical seed's pair (0.004 s,
25 rad/s) in place of the shipped one (0.28 s, 0.12·ω), with catch-up authority
scaled by the hand's bearing on the record so a fingertip could not yank.

Felt on the phone, it was worse — clearly worse, immediately. The servo half is
reverted; `production_deck_config` ships the loose pair again and `set_hand_servo`
still reaches the seed's.

The measurements that chose it were not wrong, and they are worth keeping in view:
under motion the tight servo tracked a ramp at r 0.87 against the loose pair's
0.31, a slow reversal 0.78 against 0.19, and it holds a two-hertz flick inside a
millisecond where the loose pair falls 3.1 ms behind. Every one of those favours
the seed. The hand disagreed, so the hand wins, and the analysis above should be
read as an open question rather than a settled cause.

What stays from `7204964` is the dead-reckoned target, which was the real defect
and is independent of servo stiffness: `set_motion` froze the hand's position
between sixty-hertz pointer samples, so the servo chased where the hand *was* and
a steady stroke stopped and lurched every sixteen milliseconds — the record
sitting seven milliseconds behind the hand. Offline hand-spin went from
-5 dB / r 0.90 to -76 dB / r 1.000 on that change alone.

So the next thing to try is not the servo pair. It is the double-filtered gesture
trackers below, which sit upstream of both.

## Opt-in vinyl voicing: the RIAA pair cannot colour, so expose the mismatch (2026-09-12)

A request to "add the inverse RIAA to make a more authentic vinyl sound" was
resolved by first pinning what the RIAA curve can and cannot do. Channel D's
Pure Vinyl applies an inverse-RIAA (playback de-emphasis) to a *flat* transfer
to cancel the cut and reach an accurate master. A matched pre-emphasis/playback
pair is the identity operator, so applying "the inverse" to already-mastered PCM
cannot add warmth — it either pre-emphasises the file (thin and bright) or, if
de-emphasis is applied alone, introduces a deliberate curve error. The only way
an RIAA-shaped mechanism colours anything is as a *mismatch*.

The engine already had the one physical mismatch: `RiaaSpeedTilt`, which exists
only off nominal speed. Two opt-in stages now expose the fixed mismatches a
listener associates with vinyl. Both are off/neutral by default, so the
transparent-master rule still holds:

- `AcousticConfig.riaa_voicing_rate` (WASM `setRiaaVoicing` / `riaaVoicing`),
  default `1.0`. A second `RiaaSpeedTilt` held at a constant, caller-chosen
  rate instead of following the record. `1.0` is the standard curve and passes
  the programme bit-exactly; values above `1.0` trade top end for body. The
  rate is clamped to the tilt's existing `[0.1, 4.0]`, and changes ease through
  `follow_rate` so the filter never restructures in one sample.
- `AcousticConfig.vinyl_voicing` (WASM `setVinylVoicing` / `vinylVoicing`),
  default `0.0`, amount `[0, 1]`. Blends a fixed seed curve over the master:
  a 2nd-order cartridge/arm top-end loss (20 kHz, Q 0.707) into a small preamp
  low shelf (120 Hz, +1.5 dB) and high shelf (6 kHz, -1.5 dB). `0` is bypassed
  bit-exactly. The blend is eased over the same 3 ms constant as the movement
  gain, so toggling it ramps rather than steps, and the biquads are skipped
  entirely while the blend is zero.

Both stages sit in the existing per-channel phono section, after the live speed
tilt (`src/acoustic.rs`), and are carried in the replay snapshot so deterministic
replay is unchanged.

The seed curve is a seed, not a calibrated hardware profile — the same caveat
that already attaches to the motor, slipmat, and cartridge seed values. The
bounded-response test keeps any future change honest (±3.5 dB) but does not
prove authenticity; that needs listening evidence and preferably a measured
preamp/cartridge sweep.

Tests added: the default config is neutral; the fixed-rate stage holds DC flat
and scales the top as `1/rate`; the voicing curve lifts the low end and softens
the top within its bound; both stages are bit-exact at their neutral settings;
and the setters round-trip. The C ABI and `WASM_API.md` are untouched — those
describe `PhysicalHostRenderer`, not `ScratchAcousticDsp`; the acoustic surface
is consumed through the generated WASM bindings.


### Follow-up: the voicing seed was too small, measured on real material (2026-09-12)

The first seed curve was a 1.5 dB low shelf at 120 Hz, a 1.5 dB high shelf at
6 kHz, and a 20 kHz cartridge pole. Played through the browser rack it read as
no effect, so it was measured on the actual programme rather than argued about.

Fixture: `gcp-decoded.wav` — the 48 kHz stereo decode of the `pray-for-me`
12 kbps `.ecdc` from `lori-asha-btc/pray-for-me-confirmation`. The offline
filters and the compiled `ScratchAcousticDsp` (both amount `1.0`) agreed:

| curve | overall RMS | HF (first difference) | LF < 200 Hz |
| --- | ---: | ---: | ---: |
| first seed (1.5 / 1.5 / 20k) | +1.0 dB | −0.8 dB | +0.8 dB |
| current seed (4.0 / 4.5 / 16k) | +2.6 dB | −2.8 dB | +3.2 dB |

The first seed moved the track by under 1 dB in every metric, which is why it
was inaudible. The constants are now a 150 Hz / +4.0 dB low shelf, a 4.5 kHz /
−4.5 dB high shelf, and a 16 kHz / Q 0.6 cartridge pole: a clearly audible
warmth rather than a shelf the ear cannot resolve.

The material is itself dark — its own spectrum is about −23 dB at 8 kHz and
−33 dB at 14 kHz relative to 1 kHz — so the top-end half of any voicing is
inherently muted on this EP and the effect reads mostly as body. `RIAA VOICE`
at `1.5` is the more obvious control there (−2.2 dB overall, −2.8 dB top). A
brighter, full-band master is the right fixture for judging the top-end half.

The seed-curve test bounds moved with the curve: body is now pinned to
`+2.5…+6 dB` at 60 Hz and the top to `−10…−3.5 dB` at 15 kHz, with the
midrange required to stay inside ±1 dB. The rendered-programme test's bound
moved from `1.2` to `1.7` for the larger low shelf.

### Follow-up: the voicing became a table of curves (2026-09-12)

One fixed shape with an amount turned out to be the wrong surface for the
browser's `VOICE` row: three named characters that differ only in how much of
the same filter is applied are not three characters. `AcousticConfig` now
carries `vinyl_voicing_curve`, an index into `VINYL_VOICING_CURVES`:

| index | name | shape |
| --- | --- | --- |
| 0 | COIL LOAD | broad cartridge/arm top-end loss with body under it |
| 1 | TIP MASS | tip mass dulling the top, body nearly left alone |
| 2 | CURVE DRIFT | low-mid lift and a broad presence dip, no cartridge pole |

`VinylVoicingFilter::set_curve` rebuilds the three biquad coefficients in
place and carries the running delay across (`VoicingBiquad::retuned`), so a
character change does not zero the filter and click. The render loop only
calls it when the configured index differs from the active one, so steady
playback does no work. Out-of-range indices are rejected by the constructor
and the WASM setter. `amount == 0` is still bit-exact for every curve.

Tests: all curves tint body up and top down within usable bounds; the curves
are distinct shapes (tip mass dulls more and lifts less than coil load, curve
drift dulls least); the setter round-trips; zero stays bit-exact across all
curves. The pre-existing single-curve tests now run on curve 0.
