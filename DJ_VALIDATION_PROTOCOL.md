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
  measured pointer-command-apply latency distribution.
- Stop the transport and run `player.measureAcousticLoopbackLatency()` through
  the selected physical output and input path. Keep all probe results in
  `environment.acousticLoopback`. Reject fewer than three correlated probes.
- Require pointer-command p95 latency at or below `20 ms`. Require physical
  loopback p95 at or below `30 ms` and jitter at or below `3 ms`. Require
  minimum probe correlation of at least `0.15`.
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

Two analysts must code each free-text cue before condition labels become
available. They must use one frozen, lower-case cue codebook. Resolve coding
differences while the conditions are still hidden. Store the codebook hash with
the other artifact hashes.

Publish per-participant and pooled correct counts with exact binomial confidence
intervals. The auditory indistinguishability criterion is pre-registered as:

- the pooled two-sided test does not reject chance identification at `p < 0.05`;
- the upper 95% confidence bound on identification accuracy is below 60%;
- no gesture family has a repeatable cue identified by more than 25% of
  participants; and
- the median rendered-condition realism score is at least 6/7.

"Failure to reject chance" alone is not evidence of equivalence, hence the upper
confidence bound and realism requirements.

The analysis uses an exact two-sided binomial test with chance probability
`0.5`. It uses a two-sided 95% Clopper-Pearson interval for the confidence
limits.

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

## Executable analysis

Use the dedicated console for a hardware session. Start from a committed, clean
candidate, then build and run the player:

```sh
npm run build
npm run dev
```

Open `http://localhost:5193/dj-validation.html`. The console embeds the tested
player and reads `player-build-info.json`. It refuses physical-loopback
measurements and audio blocks if the build is dirty, the draft commit does not
match the running build, or the `0.35` HF, `0.72` stylus and `0.08` fader settings
are not pinned.

Use the console to record the hardware chain, participants, browser audio blocks,
physical loopback, ABX trials, live routines, preflight results, study controls,
exclusions and artifact metadata. It keeps a local draft. Export the results
after each session. A live block also produces a separate movement-trace JSON
file and records its browser-computed SHA-256 digest.

The console does not perform the randomization or reveal hidden condition labels.
Follow the blinding procedure above. Enter decoded A/B/X conditions only after
responses and exclusions are frozen.

The command-line template remains available for an offline workflow. Generate a
version-2 JSON collection template:

```sh
npm run validation:template > dj-validation-results.json
```

Complete the environment, artifact, preflight, audio-block, trial and routine
fields. Use anonymized participant identifiers. Do not put participant names in
the file.

Use a new `excerptId` for each scored trial. Record all exclusions before
unblinding. Complete the blinding and cue-coding declarations only after their
conditions are true.

The command-line template contains one example participant, trial and routine.
Duplicate the records to meet the registered counts. Replace all placeholder and
`null` values. An exclusion record requires `id`, `reason` and
`decidedBeforeUnblinding: true`.

Analyze the frozen file:

```sh
npm run validation:analyze -- dj-validation-results.json
```

Add `--json` after the file name to produce a machine-readable report. The
report contains the input SHA-256 digest and all acceptance decisions.
The analyzer also streams each artifact path and verifies its SHA-256 digest.
Relative artifact paths start from the results-file directory.

Exit status `0` means that all pre-registered criteria passed. Exit status `2`
means that the data was valid but one or more criteria failed. Exit status `1`
means that the data was invalid.
