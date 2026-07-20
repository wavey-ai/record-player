import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  analyzeDjValidation,
  clopperPearsonInterval,
  createDjValidationTemplate,
  DJ_GESTURE_FAMILIES,
  exactBinomialTwoSidedP,
} from "../scripts/dj-validation-analysis.js";

function playbackStats(totalDurationMs) {
  return {
    supported: true,
    api: "playbackStats",
    underrunEvents: 0,
    underrunDurationMs: 0,
    totalDurationMs,
    averageLatencyMs: 20,
    minimumLatencyMs: 10,
    maximumLatencyMs: 30,
  };
}

function participant(participantIndex) {
  const id = `dj-${String(participantIndex + 1).padStart(2, "0")}`;
  const trials = Array.from({ length: 24 }, (_, trialIndex) => {
    const xCondition = trialIndex % 2 === 0 ? "physical" : "player";
    const correct = trialIndex < 12;
    const captureIndex = participantIndex * 48 + trialIndex * 2;
    return {
      id: `${id}-trial-${trialIndex + 1}`,
      excerptId: `${id}-excerpt-${trialIndex + 1}`,
      gestureFamily: DJ_GESTURE_FAMILIES[trialIndex % DJ_GESTURE_FAMILIES.length],
      aCondition: trialIndex % 2 === 0 ? "physical" : "player",
      bCondition: trialIndex % 2 === 0 ? "player" : "physical",
      xCondition,
      responseCondition: correct ? xCondition : (xCondition === "physical" ? "player" : "physical"),
      captureSha256: {
        physical: (captureIndex + 1).toString(16).padStart(64, "0"),
        player: (captureIndex + 2).toString(16).padStart(64, "0"),
      },
      confidence: 3,
      realism: 6,
      transientSharpness: 6,
      timingNaturalness: 6,
      audibleCue: "",
      cueCode: null,
    };
  });
  const routines = Array.from({ length: 5 }, (_, routineIndex) => ({
    id: `${id}-routine-${routineIndex + 1}`,
    assistancePreset: ["baby", "stab", "chirp", "flare", "crab"][routineIndex],
    durationSeconds: 60,
    instructedAttempts: 10,
    successfulAttempts: 9,
    missedGrabs: 0,
    unintendedCuts: 0,
    pointerLosses: 0,
    stuckScratchIncidents: 0,
    postReleaseMutes: 0,
    timingCorrections: 0,
    ownershipRating: 6,
    timingRating: 6,
    assistanceFollowedIntent: true,
    useInRecordedSet: true,
    useInLiveSet: true,
    firstChange: "No change requested.",
  }));
  return {
    id,
    currentlyActiveDj: true,
    regularlyScratches: participantIndex < 6,
    experienceBand: participantIndex % 2 === 0 ? "6-10-years" : "over-10-years",
    trainingCompleted: true,
    trials,
    routines,
  };
}

function passingResults() {
  const participants = Array.from({ length: 12 }, (_, index) => participant(index));
  const blocks = participants.flatMap(({ id }) => ["abx", "live"].map((kind, index) => ({
    id: `${id}-${kind}`,
    participantId: id,
    kind,
    before: playbackStats(index * 10_000),
    after: playbackStats((index + 1) * 10_000),
  })));
  const artifactRoles = [
    "candidate-build-info",
    "source-master",
    "physical-capture",
    "player-capture",
    "movement-trace",
    "randomization-manifest",
    "cue-codebook",
  ];
  return {
    schemaVersion: 3,
    candidate: {
      commit: "2".repeat(40),
      worktreeDirty: false,
      settings: {
        highFrequencyAccelerationLimit: 0.35,
        stylusTracingLimit: 0.72,
        faderCurve: 0.08,
        acousticEffects: true,
        surfaceEffects: true,
        nativeRpmValues: [100 / 3, 45],
        endPolicies: ["runout", "clean"],
      },
    },
    environment: {
      browser: "Chrome 150",
      os: "macOS 26.5",
      inputDevice: "Test touch surface",
      audioInterface: "Test interface",
      listeningTransducers: ["headphones", "monitors"],
      quietRoom: true,
      displaySampleRateHz: 120,
      audioContextSampleRateHz: 48_000,
      baseLatencyMs: 5,
      outputLatencyMs: 20,
      interfaceBufferFrames: 128,
      pointerCommandLatencyMs: { p50: 6, p95: 12, maximum: 18 },
      acousticLoopback: {
        samples: 5,
        sampleRate: 48_000,
        repetitionsRequested: 5,
        medianMs: 24,
        p95Ms: 24.8,
        maximumMs: 25,
        minimumMs: 23,
        jitterMs: 2,
        minimumCorrelation: 0.82,
        maximumLatencyMs: 500,
        amplitude: 0.08,
        inputDeviceLabel: "Test loopback input",
        inputDeviceSettings: {
          echoCancellation: false,
          noiseSuppression: false,
          autoGainControl: false,
        },
        outputDeviceId: "test-output",
      },
    },
    artifacts: artifactRoles.map((role, index) => ({
      role,
      path: `${role}-${index}.bin`,
      sha256: String(index + 1).repeat(64),
    })),
    preflight: {
      checks: {
        "level-and-delay-calibration": true,
        mechanics: true,
        "transport-rates": true,
        presets: true,
        "clocks-and-windows": true,
        "multi-pointer": true,
        "limiter-cells": true,
      },
      unexpectedClips: 0,
      undecodedZeroExcursions: 0,
      discontinuitiesAboveBound: 0,
      levelMismatchDb: 0.05,
      fixedPathDelayMs: 12.5,
      declickBound: 0.1,
      maximumAdjacentDiscontinuity: 0.05,
    },
    blinding: {
      participantConditionLabelsHidden: true,
      operatorConditionLabelsHidden: true,
      assistancePresetHidden: true,
      randomizationGeneratedBeforeSession: true,
      decodedAfterResultsFrozen: true,
    },
    cueCoding: {
      coderCount: 2,
      conditionLabelsHidden: true,
      differencesResolvedBeforeUnblinding: true,
    },
    exclusions: [],
    blocks,
    participants,
  };
}

function analyzeFixture(results, options = {}) {
  return analyzeDjValidation(results, {
    verifiedArtifactRoles: results.artifacts.map(artifact => artifact.role),
    verifiedCandidateBuildInfo: {
      schemaVersion: 1,
      commit: results.candidate.commit,
      worktreeDirty: results.candidate.worktreeDirty,
      settings: structuredClone(results.candidate.settings),
    },
    ...options,
  });
}

test("computes exact two-sided binomial results and Clopper-Pearson intervals", () => {
  assert.equal(exactBinomialTwoSidedP(5, 10), 1);
  assert.ok(Math.abs(exactBinomialTwoSidedP(10, 10) - 0.001953125) < 1e-15);
  const interval = clopperPearsonInterval(5, 10);
  assert.ok(Math.abs(interval.lower - 0.1870860284) < 1e-9);
  assert.ok(Math.abs(interval.upper - 0.8129139716) < 1e-9);
});

test("accepts a complete pre-registered study at chance identification", () => {
  const result = analyzeFixture(passingResults(), { sourceSha256: "a".repeat(64) });
  assert.equal(result.accepted, true);
  assert.equal(result.sourceSha256, "a".repeat(64));
  assert.equal(result.pooledAbx.trials, 288);
  assert.equal(result.pooledAbx.correct, 144);
  assert.equal(result.pooledAbx.uniqueCaptureHashes, 576);
  assert.equal(result.pooledAbx.exactTwoSidedP, 1);
  assert.ok(result.pooledAbx.clopperPearson95.upper < 0.60);
  assert.equal(result.participants[0].correct, 12);
  assert.equal(result.participants[0].exactTwoSidedP, 1);
  assert.ok(result.participants[0].clopperPearson95.upper > 0.5);
  assert.equal(result.live.successRate, 0.9);
  assert.ok(Object.values(result.criteria).every(criterion => criterion.pass));
});

test("rejects a player that participants identify above chance", () => {
  const results = passingResults();
  for (const participantValue of results.participants) {
    for (const trial of participantValue.trials) trial.responseCondition = trial.xCondition;
  }
  const analysis = analyzeFixture(results);
  assert.equal(analysis.accepted, false);
  assert.equal(analysis.criteria.chanceIdentification.pass, false);
  assert.equal(analysis.criteria.identificationUpperBound.pass, false);
});

test("rejects a repeatable cue reported by more than one quarter of DJs", () => {
  const results = passingResults();
  for (const participantValue of results.participants.slice(0, 4)) {
    const trial = participantValue.trials.find(value => value.gestureFamily === "baby-drag-cue");
    trial.audibleCue = "The digital version has a bright edge.";
    trial.cueCode = "bright-edge";
  }
  const analysis = analyzeFixture(results);
  assert.equal(analysis.accepted, false);
  assert.equal(analysis.criteria.repeatableCue.pass, false);
  assert.equal(analysis.cues[0].participantCount, 4);
  assert.equal(analysis.cues[0].participantFraction, 1 / 3);
});

test("rejects any browser-reported audio underrun", () => {
  const results = passingResults();
  results.blocks[0].after.underrunEvents = 1;
  results.blocks[0].after.underrunDurationMs = 2.5;
  const analysis = analyzeFixture(results);
  assert.equal(analysis.accepted, false);
  assert.equal(analysis.criteria.deviceAudio.pass, false);
});

test("rejects broken blinding or level calibration", () => {
  const results = passingResults();
  results.blinding.operatorConditionLabelsHidden = false;
  results.preflight.levelMismatchDb = 0.11;
  const analysis = analyzeFixture(results);
  assert.equal(analysis.accepted, false);
  assert.equal(analysis.criteria.studyControls.pass, false);
  assert.equal(analysis.criteria.preflight.pass, false);
});

test("rejects evidence collected from a dirty candidate worktree", () => {
  const results = passingResults();
  results.candidate.worktreeDirty = true;
  const analysis = analyzeFixture(results);
  assert.equal(analysis.accepted, false);
  assert.equal(analysis.criteria.pinnedCandidate.pass, false);
});

test("rejects a physical audio path outside the registered latency bound", () => {
  const results = passingResults();
  results.environment.acousticLoopback.medianMs = 31;
  results.environment.acousticLoopback.p95Ms = 34;
  results.environment.acousticLoopback.maximumMs = 35;
  results.environment.acousticLoopback.minimumMs = 30;
  results.environment.acousticLoopback.jitterMs = 5;
  const analysis = analyzeFixture(results);
  assert.equal(analysis.accepted, false);
  assert.equal(analysis.criteria.acousticLatency.pass, false);
});

test("rejects reuse of a supposedly fresh ABX excerpt", () => {
  const results = passingResults();
  results.participants[1].trials[0].excerptId = results.participants[0].trials[0].excerptId;
  assert.throws(() => analyzeFixture(results), /reuses excerptId/);
});

test("rejects reuse of capture audio across participant packages", () => {
  const results = passingResults();
  results.participants[1].trials[0].captureSha256.physical =
    results.participants[0].trials[0].captureSha256.physical;
  assert.throws(() => analyzeFixture(results), /reuses capture SHA-256/);
});

test("requires the shipped acoustic and surface effects", () => {
  const results = passingResults();
  results.candidate.settings.surfaceEffects = false;
  const analysis = analyzeFixture(results);
  assert.equal(analysis.criteria.pinnedCandidate.pass, false);
});

test("rejects build metadata from a different candidate", () => {
  const results = passingResults();
  const analysis = analyzeFixture(results, {
    verifiedCandidateBuildInfo: {
      schemaVersion: 1,
      commit: "f".repeat(40),
      worktreeDirty: false,
      settings: structuredClone(results.candidate.settings),
    },
  });
  assert.equal(analysis.criteria.artifactHashes.pass, false);
});

test("requires frozen cue coding when a participant reports a cue", () => {
  const results = passingResults();
  results.participants[0].trials[0].audibleCue = "I heard a click.";
  assert.throws(() => analyzeFixture(results), /cueCode is required/);
});

test("rejects older schemas because they cannot bind capture evidence", () => {
  const results = passingResults();
  results.schemaVersion = 2;
  assert.throws(() => analyzeFixture(results), /schema version 2 lacks build-bound capture evidence/);
});

test("generates a schema-three collection template with pinned settings", () => {
  const template = createDjValidationTemplate();
  assert.equal(template.schemaVersion, 3);
  assert.equal(template.candidate.settings.highFrequencyAccelerationLimit, 0.35);
  assert.equal(template.candidate.settings.stylusTracingLimit, 0.72);
  assert.equal(template.candidate.settings.faderCurve, 0.08);
  assert.equal(template.candidate.worktreeDirty, null);
  assert.equal(template.participants.length, 1);
  assert.equal(template.participants[0].trials.length, 1);
  assert.deepEqual(template.participants[0].trials[0].captureSha256, { physical: "", player: "" });
  assert.equal(template.participants[0].routines.length, 1);
  assert.equal(template.blocks.length, 2);
  assert.equal(template.blocks[0].before.underrunEvents, null);
});

test("CLI hashes and accepts a frozen passing result file", async () => {
  const directory = await mkdtemp(join(tmpdir(), "vinyl-dj-validation-"));
  const inputPath = join(directory, "results.json");
  try {
    const results = passingResults();
    for (const artifact of results.artifacts) {
      const content = artifact.role === "candidate-build-info"
        ? `${JSON.stringify({
          schemaVersion: 1,
          commit: results.candidate.commit,
          worktreeDirty: false,
          settings: results.candidate.settings,
        }, null, 2)}\n`
        : `${artifact.role}\n`;
      await writeFile(join(directory, artifact.path), content, "utf8");
      artifact.sha256 = createHash("sha256").update(content).digest("hex");
    }
    await writeFile(inputPath, `${JSON.stringify(results)}\n`, "utf8");
    const command = fileURLToPath(new URL("../scripts/analyze-dj-validation.mjs", import.meta.url));
    const result = spawnSync(process.execPath, [command, inputPath, "--json"], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    const report = JSON.parse(result.stdout);
    assert.equal(report.accepted, true);
    assert.match(report.sourceSha256, /^[0-9a-f]{64}$/);
    assert.equal(report.participants.length, 12);

    for (const participantValue of results.participants) {
      for (const trial of participantValue.trials) trial.responseCondition = trial.xCondition;
    }
    await writeFile(inputPath, `${JSON.stringify(results)}\n`, "utf8");
    const identified = spawnSync(process.execPath, [command, inputPath, "--json"], { encoding: "utf8" });
    assert.equal(identified.status, 2, identified.stderr);
    assert.equal(JSON.parse(identified.stdout).accepted, false);

    await writeFile(join(directory, results.artifacts[0].path), "tampered\n", "utf8");
    const tampered = spawnSync(process.execPath, [command, inputPath, "--json"], { encoding: "utf8" });
    assert.equal(tampered.status, 1);
    assert.match(tampered.stderr, /SHA-256 mismatch/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
