# Vinyl Scratch Perceptual Validation Protocol

## Claim being tested

The defensible claim is auditory and operational: under the documented device and
latency conditions, experienced DJs cannot reliably identify the rendered player
against a calibrated physical-vinyl reference from its sound, and they rate its
timing/control response inside the agreed professional-use bounds.

A browser touchscreen cannot reproduce the haptics of a slipmat, record inertia or
crossfader hardware. This protocol therefore does not claim tactile identity. No
"indistinguishable" wording should be published until the blind test below has
been completed and its raw results retained.

## Reference and test chain

- Use a quartz-locked direct-drive deck with documented wow/flutter, a fresh DJ
  stylus, a low-capacitance phono path, and a replaceable-cut record made from the
  exact digital master loaded into the player.
- Capture the physical reference and browser output through the same interface,
  clock, channel count and sample rate. Bypass loudness normalization, limiters,
  enhancement and operating-system spatial audio everywhere outside the player.
  Do not add an external HF limiter to make either condition resemble the other;
  document the physical cutting/mastering chain separately from the player's
  internal programme limiter.
- Calibrate 1 kHz programme RMS to within 0.1 dB and align fixed path delay before
  randomization. Do not normalize individual scratch excerpts after capture.
- Record browser, OS, input device, display sampling rate, AudioContext sample
  rate, `baseLatency`, `outputLatency` (where exposed), interface buffer and the
  measured pointer-to-output latency distribution.
- Record `audioPlaybackStats` before and after each block. Require supported
  device statistics, zero `underrunEvents` and zero `underrunDurationMs`. Keep
  total duration and latency fields with the raw session data.
- Keep an unprocessed master, the physical capture, the player capture, movement
  traces and randomized trial manifest under content hashes.
- Pin every candidate manifest to the shipped Rust settings: programme HF
  acceleration limit `0.35`, stylus-tracing limit `0.72`, sharp fader curve
  `0.08`, selected acoustic/surface flags, native RPM and end policy. Silent
  setting changes invalidate the block.

The primary blind condition uses the shipped `0.35`/`0.72` pair. Run a separate
diagnostic block from the same source and gestures with these four cells:

| HF acceleration | Stylus tracing | Purpose |
| ---: | ---: | --- |
| `0.35` | `0.72` | Shipped candidate and primary acceptance condition. |
| `0` | `0.72` | Exact programme-limiter bypass; isolates its contribution. |
| `0.35` | `0` | Added stylus-tracing-limit bypass; isolates that model. |
| `0` | `0` | Joint diagnostic control. |

Include clean sibilants, cymbal attacks and deliberately harsh upper-band
transients as well as benign bright material. The diagnostic cells explain cues;
they do not replace the pre-registered primary condition or permit choosing the
best-sounding settings after results are known.

## Mechanical and signal preflight

Before involving listeners, the build must pass the repository test suite and a
measurement capture covering:

1. motor start, powered brake, unpowered throw and hand grab/release in both
   directions;
2. cue hold, slow drag, ±1× lock, 2×/4×/8× sweeps and reversal;
3. baby, stab, chirp, transform, flare, crab, orbit and drum presets at click
   counts 1, 4 and 8 where applicable;
4. 33⅓ and 45 RPM, 44.1/48/96 kHz source clocks, seam crossings and progressive
   window boundaries;
5. manual-fader operation while a separate pointer owns the record.
6. the four pinned HF/tracing cells above, including exact-bypass checks and
   confirmation that programme limiting does not duck lows/mids or surface foley.

Reject the candidate before listening if it clips unexpectedly, reads undecoded
zeros, reports an audio underrun, loses a pointer, changes timing with pointer
event rate, leaves the gate closed after release, or produces a discontinuity
above the declared de-click bound.

## Double-blind listening test

Recruit at least 12 currently active DJs, including at least six who regularly
scratch. Use headphones and monitors in a quiet room; neither the participant nor
the test operator may see condition labels. Randomization is generated before the
session and decoded only after results are frozen.

Each participant completes training with non-scored examples, then at least 24
scored ABX trials balanced across these gesture families:

- baby/drag and cue hold;
- stab and transform;
- chirp and flare;
- crab/orbit;
- fast forward/reverse release;
- motor start/brake and run-out.

For every trial, A and B are the physical and rendered conditions in randomized
order; X repeats one. The participant identifies X and reports confidence (1–5),
realism (1–7), transient sharpness (1–7), timing naturalness (1–7), and any audible
cue in free text. Use fresh excerpts across trials so memory of surface-noise
events cannot reveal a condition.

Publish per-participant and pooled correct counts with exact binomial confidence
intervals. The auditory indistinguishability criterion is pre-registered as:

- the pooled two-sided test does not reject chance identification at `p < 0.05`;
- the upper 95% confidence bound on identification accuracy is below 60%;
- no gesture family has a repeatable cue identified by more than 25% of
  participants; and
- the median rendered-condition realism score is at least 6/7.

"Failure to reject chance" alone is not evidence of equivalence, hence the upper
confidence bound and realism requirements.

## Live-control test

The same DJs then perform five 60-second routines on the player without being told
which assistance preset is active. Capture missed grabs, unintended cuts, pointer
loss, timing corrections, and task completion. After each routine collect:

- perceived record ownership and reversal response (1–7);
- perceived fader timing and cut sharpness (1–7);
- whether assistance followed or fought intent;
- whether they would use the build in a recorded set and in a live set;
- a short description of the first behavior they would change.

Pre-register operational acceptance as zero pointer-loss/stuck-scratch incidents,
no unintended post-release mute, at least 90% successful instructed techniques,
and median ownership/timing scores of at least 6/7. Report device-specific latency
alongside results; a pass on one hardware/browser chain does not generalize to all
chains.

## Reporting

Retain the version/commit, configuration, anonymized experience bands, exclusions,
all trials, raw ratings, analysis script and failures. Report negative and positive
results together. Any DSP, gate, latency or gesture-filter change after a passing
run creates a new candidate and requires at least the affected preflight and blind
trial blocks to be rerun.
