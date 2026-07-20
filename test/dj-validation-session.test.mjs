import assert from "node:assert/strict";
import test from "node:test";

import { DjValidationSession } from "../web/dj-validation-session.js";
import { createDjValidationTemplate } from "../web/dj-validation-template.js";

function playerSnapshot(totalDurationMs, pointerToAudioLatencyMs = null, pointerAppliedCommandId = null) {
  return {
    pointerToAudioLatencyMs,
    pointerAppliedCommandId,
    outputSampleRate: 48_000,
    audioBaseLatencyMs: 5.8,
    audioOutputLatencyMs: 12,
    audioPlaybackStats: {
      supported: true,
      api: "playbackStats",
      underrunEvents: 0,
      underrunDurationMs: 0,
      totalDurationMs,
      averageLatencyMs: 18,
      minimumLatencyMs: 12,
      maximumLatencyMs: 24,
    },
  };
}

function addParticipant(session, id = "dj-01") {
  return session.addParticipant({
    id,
    currentlyActiveDj: true,
    regularlyScratches: true,
    experienceBand: "over-10-years",
    trainingCompleted: true,
  });
}

test("creates an empty browser collection from the shared schema-two template", () => {
  const template = createDjValidationTemplate({ includeExample: false });
  const session = new DjValidationSession({ data: template });
  assert.equal(session.snapshot().schemaVersion, 2);
  assert.deepEqual(session.snapshot().participants, []);
  assert.deepEqual(session.snapshot().blocks, []);
  assert.equal(session.summary().participants, 0);
});

test("records participants and rejects duplicate anonymized ids", () => {
  const session = new DjValidationSession();
  session.setBuildInfo({ commit: "88329ae", worktreeDirty: false });
  const participant = addParticipant(session);
  assert.equal(session.snapshot().candidate.worktreeDirty, false);
  assert.equal(participant.id, "dj-01");
  assert.equal(participant.trials.length, 0);
  assert.throws(() => addParticipant(session), /already exists/);
});

test("records one complete ABX trial and protects fresh excerpt ids", () => {
  const session = new DjValidationSession();
  addParticipant(session);
  const values = {
    excerptId: "excerpt-001",
    gestureFamily: "chirp-flare",
    aCondition: "physical",
    bCondition: "player",
    xCondition: "player",
    responseCondition: "player",
    confidence: 4,
    realism: 6,
    transientSharpness: 6,
    timingNaturalness: 7,
    audibleCue: "",
    cueCode: null,
  };
  const trial = session.addTrial("dj-01", values);
  assert.equal(trial.id, "dj-01-trial-1");
  assert.equal(trial.responseCondition, "player");
  assert.throws(() => session.addTrial("dj-01", values), /Excerpt already exists/);
});

test("records a complete live-control routine", () => {
  const session = new DjValidationSession();
  addParticipant(session);
  const routine = session.addRoutine("dj-01", {
    assistancePreset: "crab",
    durationSeconds: 60,
    instructedAttempts: 10,
    successfulAttempts: 9,
    missedGrabs: 0,
    unintendedCuts: 1,
    pointerLosses: 0,
    stuckScratchIncidents: 0,
    postReleaseMutes: 0,
    timingCorrections: 1,
    ownershipRating: 6,
    timingRating: 6,
    assistanceFollowedIntent: true,
    useInRecordedSet: true,
    useInLiveSet: true,
    firstChange: "Shorten the second cut.",
  });
  assert.equal(routine.id, "dj-01-routine-1");
  assert.equal(session.summary().routines, 1);
  assert.throws(() => session.addRoutine("dj-01", {
    ...routine,
    id: "another",
    successfulAttempts: 11,
  }), /exceeds/);
});

test("captures an audio block and derives the pointer-command distribution", () => {
  let now = 1_000;
  const session = new DjValidationSession({ now: () => now });
  addParticipant(session);
  session.startBlock("dj-01", "live", playerSnapshot(1_000, 5, 1));
  now = 2_000;
  session.observePlayerState(playerSnapshot(1_500, 10, 2));
  session.observePlayerState(playerSnapshot(1_750, 10, 2));
  const block = session.endBlock(playerSnapshot(2_000, 15, 3));

  assert.equal(block.id, "dj-01-live-1");
  assert.equal(block.before.totalDurationMs, 1_000);
  assert.equal(block.after.totalDurationMs, 2_000);
  assert.deepEqual(block.pointerCommandLatenciesMs, [10, 15]);
  assert.deepEqual(session.snapshot().environment.pointerCommandLatencyMs, {
    p50: 12.5,
    p95: 14.75,
    maximum: 15,
  });
  assert.equal(session.snapshot().environment.audioContextSampleRateHz, 48_000);
  assert.equal(session.summary().activeBlock, null);
});

test("guards audio block ownership, duration and export", () => {
  const session = new DjValidationSession();
  addParticipant(session);
  session.startBlock("dj-01", "abx", playerSnapshot(1_000));
  assert.throws(() => session.startBlock("dj-01", "live", playerSnapshot(1_000)), /already active/);
  assert.throws(() => session.exportResults(), /active audio block/);
  assert.throws(() => session.endBlock(playerSnapshot(1_000)), /did not increase/);
  assert.ok(session.cancelBlock());
  assert.doesNotThrow(() => session.exportResults());
});

test("discards pointer-command latency samples from a canceled block", () => {
  const session = new DjValidationSession();
  addParticipant(session);
  session.startBlock("dj-01", "live", playerSnapshot(1_000, 5, 1));
  session.observePlayerState(playerSnapshot(1_500, 100, 2));
  session.cancelBlock();
  session.startBlock("dj-01", "live", playerSnapshot(2_000, 5, 3));
  session.observePlayerState(playerSnapshot(2_500, 8, 4));
  session.endBlock(playerSnapshot(3_000, 9, 5));
  assert.deepEqual(session.snapshot().environment.pointerCommandLatencyMs, {
    p50: 8.5,
    p95: 8.95,
    maximum: 9,
  });
});

test("stores physical-loopback and artifact metadata without sharing mutable input", () => {
  const session = new DjValidationSession();
  const loopback = {
    samples: 5,
    sampleRate: 48_000,
    medianMs: 20,
    p95Ms: 22,
    maximumMs: 23,
    minimumMs: 19,
    jitterMs: 4,
    minimumCorrelation: 0.8,
  };
  session.setAcousticLoopback(loopback);
  loopback.medianMs = 999;
  session.setArtifact("movement-trace", { path: "movement.json", sha256: "a".repeat(64) });
  session.addExclusion({
    id: "excluded-01",
    reason: "Did not complete training.",
    decidedBeforeUnblinding: true,
  });
  assert.equal(session.snapshot().environment.acousticLoopback.medianMs, 20);
  assert.equal(session.snapshot().artifacts.find(value => value.role === "movement-trace").path, "movement.json");
  assert.equal(session.snapshot().exclusions.length, 1);
  assert.throws(() => session.addExclusion({
    id: "excluded-01",
    reason: "Duplicate.",
    decidedBeforeUnblinding: true,
  }), /already exists/);
});
