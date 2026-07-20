import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  createDjAbxBlindPlan,
  createDjAbxCueCodeTemplate,
  decodeDjBlindAbxResponses,
  DjBlindAbxSession,
  validateDjAbxBlindManifest,
  validateDjAbxPreparationSpec,
} from "../web/dj-abx.js";
import { createDjValidationTemplate } from "../web/dj-validation-template.js";

const scriptsDirectory = fileURLToPath(new URL("../scripts/", import.meta.url));
const candidateSettings = Object.freeze({
  highFrequencyAccelerationLimit: 0.35,
  stylusTracingLimit: 0.72,
  faderCurve: 0.08,
  acousticEffects: true,
  surfaceEffects: true,
  nativeRpmValues: [100 / 3, 45],
  endPolicies: ["runout", "clean"],
});

function candidate() {
  return {
    commit: "d".repeat(40),
    worktreeDirty: false,
    buildInfoSha256: "a".repeat(64),
    settings: structuredClone(candidateSettings),
  };
}

function preparationSpec(count = 2) {
  return {
    schemaVersion: 2,
    studyId: "scratch-study-001",
    participantId: "dj-01",
    trials: Array.from({ length: count }, (_, index) => ({
      id: `trial-${index + 1}`,
      excerptId: `excerpt-${index + 1}`,
      gestureFamily: index % 2 === 0 ? "chirp-flare" : "crab-orbit",
      physicalPath: `physical-${index + 1}.wav`,
      playerPath: `player-${index + 1}.wav`,
    })),
  };
}

function blindPlan(spec = preparationSpec()) {
  let pathIndex = 0;
  const plan = createDjAbxBlindPlan(spec, {
    candidate: candidate(),
    randomInteger: () => 0,
    opaqueAudioPath: () => `audio/${(++pathIndex).toString(16).padStart(32, "0")}.wav`,
    generatedAt: "2026-07-20T12:00:00.000Z",
  });
  let hashIndex = 0;
  for (const trial of plan.manifest.trials) {
    for (const descriptor of Object.values(trial.audio)) {
      descriptor.sha256 = (++hashIndex).toString(16).padStart(64, "0");
    }
  }
  let sourceHashIndex = 100;
  for (const trial of plan.codebook.trials) {
    for (const condition of ["physical", "player"]) {
      trial.sources[condition] = {
        path: trial.sources[condition].path,
        fileSha256: (++sourceHashIndex).toString(16).padStart(64, "0"),
        audioSha256: (++sourceHashIndex).toString(16).padStart(64, "0"),
        wav: {
          audioFormat: 1,
          channels: 2,
          sampleRateHz: 48_000,
          bitsPerSample: 16,
          frames: 4_800,
        },
      };
    }
  }
  plan.manifest.codebookSha256 = createHash("sha256")
    .update(`${JSON.stringify(plan.codebook, null, 2)}\n`)
    .digest("hex");
  return plan;
}

function materializeCodebook(plan) {
  return structuredClone(plan.codebook);
}

function manifestSha256(plan) {
  return createHash("sha256")
    .update(`${JSON.stringify(plan.manifest, null, 2)}\n`)
    .digest("hex");
}

function decodeOptions(plan, values = {}) {
  return {
    codebookSha256: plan.manifest.codebookSha256,
    manifestSha256: manifestSha256(plan),
    ...values,
  };
}

function completeSession(plan, participantId = "dj-01", audibleCue = "") {
  const session = new DjBlindAbxSession({
    manifest: plan.manifest,
    manifestSha256: manifestSha256(plan),
    participantId,
    completedAt: () => "2026-07-20T13:00:00.000Z",
  });
  while (session.currentTrial) {
    session.markListened("a");
    session.markListened("b");
    session.markListened("x");
    session.recordResponse({
      responseLabel: "a",
      confidence: 4,
      realism: 6,
      transientSharpness: 6,
      timingNaturalness: 7,
      audibleCue,
    });
  }
  return session;
}

function wavBytes(sample, frames = 32) {
  const channels = 2;
  const bitsPerSample = 16;
  const sampleRate = 48_000;
  const blockAlign = channels * bitsPerSample / 8;
  const dataBytes = frames * blockAlign;
  const bytes = Buffer.alloc(44 + dataBytes);
  bytes.write("RIFF", 0, "ascii");
  bytes.writeUInt32LE(bytes.length - 8, 4);
  bytes.write("WAVE", 8, "ascii");
  bytes.write("fmt ", 12, "ascii");
  bytes.writeUInt32LE(16, 16);
  bytes.writeUInt16LE(1, 20);
  bytes.writeUInt16LE(channels, 22);
  bytes.writeUInt32LE(sampleRate, 24);
  bytes.writeUInt32LE(sampleRate * blockAlign, 28);
  bytes.writeUInt16LE(blockAlign, 32);
  bytes.writeUInt16LE(bitsPerSample, 34);
  bytes.write("data", 36, "ascii");
  bytes.writeUInt32LE(dataBytes, 40);
  for (let offset = 44; offset < bytes.length; offset += 2) bytes.writeInt16LE(sample, offset);
  return bytes;
}

function wavBytesWithJunk(sample, tag) {
  const source = wavBytes(sample);
  const bytes = Buffer.alloc(source.length + 12);
  source.copy(bytes);
  bytes.writeUInt32LE(bytes.length - 8, 4);
  bytes.write("JUNK", source.length, "ascii");
  bytes.writeUInt32LE(4, source.length + 4);
  bytes.writeUInt32LE(tag, source.length + 8);
  return bytes;
}

test("creates an opaque blind plan with distinct A, B and X files", () => {
  const plan = blindPlan();
  const serialized = JSON.stringify(plan.manifest);
  assert.doesNotMatch(serialized, /physical|player/);
  assert.equal(plan.manifest.trials.length, 2);
  assert.equal(new Set(plan.manifest.trials.flatMap(trial => Object.values(trial.audio).map(audio => audio.path))).size, 6);
  assert.equal(new Set(plan.manifest.trials.flatMap(trial => Object.values(trial.audio).map(audio => audio.sha256))).size, 6);
  assert.equal(plan.copies.length, 6);
  assert.doesNotThrow(() => validateDjAbxBlindManifest(plan.manifest));
});

test("rejects a dirty or differently configured candidate", () => {
  const dirty = candidate();
  dirty.worktreeDirty = true;
  assert.throws(() => createDjAbxBlindPlan(preparationSpec(1), {
    candidate: dirty,
    randomInteger: () => 0,
    opaqueAudioPath: () => "audio/11111111111111111111111111111111.wav",
  }), /worktreeDirty/);
  const changed = candidate();
  changed.settings.surfaceEffects = false;
  assert.throws(() => createDjAbxBlindPlan(preparationSpec(1), {
    candidate: changed,
    randomInteger: () => 0,
    opaqueAudioPath: () => "audio/11111111111111111111111111111111.wav",
  }), /shipped release settings/);
});

test("balances A and X conditions inside an even gesture-family block", () => {
  const spec = preparationSpec(4);
  spec.trials.forEach(trial => { trial.gestureFamily = "chirp-flare"; });
  const plan = blindPlan(spec);
  assert.equal(plan.codebook.trials.filter(trial => trial.aCondition === "physical").length, 2);
  assert.equal(plan.codebook.trials.filter(trial => trial.xCondition === "physical").length, 2);
});

test("rejects reused captures and condition-bearing manifest fields", () => {
  const spec = preparationSpec();
  spec.trials[1].physicalPath = spec.trials[0].physicalPath;
  assert.throws(() => validateDjAbxPreparationSpec(spec), /reuses/);
  const manifest = blindPlan().manifest;
  manifest.trials[0].condition = "physical";
  assert.throws(() => validateDjAbxBlindManifest(manifest), /unexpected/);
  const matchingHashes = blindPlan().manifest;
  matchingHashes.trials[0].audio.x.sha256 = matchingHashes.trials[0].audio.a.sha256;
  assert.throws(() => validateDjAbxBlindManifest(matchingHashes), /reuses/);
});

test("requires A, B and X listening and restores only condition-free responses", () => {
  const plan = blindPlan(preparationSpec(1));
  assert.throws(() => new DjBlindAbxSession({
    manifest: plan.manifest,
    manifestSha256: manifestSha256(plan),
    participantId: "dj-02",
  }), /does not match/);
  const session = new DjBlindAbxSession({
    manifest: plan.manifest,
    manifestSha256: manifestSha256(plan),
    participantId: "dj-01",
  });
  session.markListened("a");
  session.markListened("b");
  assert.throws(() => session.recordResponse({
    responseLabel: "a",
    confidence: 4,
    realism: 6,
    transientSharpness: 6,
    timingNaturalness: 6,
    audibleCue: "",
  }), /Listen to A, B and X/);
  session.markListened("x");
  session.recordResponse({
    responseLabel: "a",
    confidence: 4,
    realism: 6,
    transientSharpness: 6,
    timingNaturalness: 6,
    audibleCue: "",
  });
  const draft = session.exportDraft();
  assert.doesNotMatch(JSON.stringify(draft), /physical|player/);
  const restored = new DjBlindAbxSession({
    manifest: plan.manifest,
    manifestSha256: manifestSha256(plan),
    participantId: "dj-01",
    responses: draft.responses,
  });
  assert.equal(restored.progress.finished, true);
  assert.equal(restored.exportResponses().responses.length, 1);
});

test("decodes response labels only after the manifest-bound codebook is supplied", () => {
  const plan = blindPlan(preparationSpec(1));
  const codebook = materializeCodebook(plan);
  const bundle = completeSession(plan).exportResponses();
  const decoded = decodeDjBlindAbxResponses(codebook, [bundle], decodeOptions(plan));
  const expected = codebook.trials[0].aCondition;
  assert.equal(decoded.participants[0].trials[0].responseCondition, expected);
  assert.equal(decoded.participants[0].trials[0].xCondition, codebook.trials[0].xCondition);
  const mismatched = structuredClone(bundle);
  mismatched.manifestSha256 = "f".repeat(64);
  assert.throws(() => decodeDjBlindAbxResponses(codebook, [mismatched], decodeOptions(plan)), /does not match/);
});

test("requires frozen cue codes before it decodes a reported cue", () => {
  const plan = blindPlan(preparationSpec(1));
  const codebook = materializeCodebook(plan);
  const bundle = completeSession(plan, "dj-01", "The upper edge sounds bright.").exportResponses();
  const template = createDjAbxCueCodeTemplate(bundle);
  assert.deepEqual(template.entries, [{
    participantId: "dj-01",
    trialId: codebook.trials[0].id,
    audibleCue: "The upper edge sounds bright.",
    cueCode: "",
  }]);
  assert.throws(() => decodeDjBlindAbxResponses(codebook, [bundle], decodeOptions(plan)), /cue code is required/i);
  const decoded = decodeDjBlindAbxResponses(codebook, [bundle], decodeOptions(plan, {
    cueCodes: {
      schemaVersion: 1,
      studyId: codebook.studyId,
      manifestSha256: bundle.manifestSha256,
      codebookSha256: bundle.codebookSha256,
      entries: [{
        participantId: "dj-01",
        trialId: codebook.trials[0].id,
        audibleCue: "The upper edge sounds bright.",
        cueCode: "bright-edge",
      }],
    },
  }));
  assert.equal(decoded.participants[0].trials[0].cueCode, "bright-edge");
  assert.throws(() => decodeDjBlindAbxResponses(codebook, [bundle], decodeOptions(plan, {
    cueCodes: {
      schemaVersion: 1,
      studyId: codebook.studyId,
      manifestSha256: bundle.manifestSha256,
      codebookSha256: bundle.codebookSha256,
      entries: [
        {
          participantId: "dj-01",
          trialId: codebook.trials[0].id,
          audibleCue: "The upper edge sounds bright.",
          cueCode: "bright-edge",
        },
        {
          participantId: "dj-99",
          trialId: "unregistered-trial",
          audibleCue: "Unregistered cue.",
          cueCode: "extra-cue",
        },
      ],
    },
  })), /does not match a reported cue/);
});

test("CLI creates a non-overwriting matched-WAV package and merges frozen responses", async () => {
  const directory = await mkdtemp(join(tmpdir(), "vinyl-dj-abx-"));
  const specPath = join(directory, "spec.json");
  const packagePath = join(directory, "operator-package");
  const codebookPath = join(directory, "private", "codebook.json");
  const buildInfoPath = join(directory, "player-build-info.json");
  const responsePath = join(directory, "response.json");
  const resultsPath = join(directory, "results.json");
  const outputPath = join(directory, "merged.json");
  try {
    const spec = preparationSpec(1);
    const buildInfo = {
      schemaVersion: 1,
      commit: candidate().commit,
      worktreeDirty: false,
      settings: structuredClone(candidateSettings),
    };
    const buildInfoBytes = Buffer.from(`${JSON.stringify(buildInfo, null, 2)}\n`);
    await writeFile(buildInfoPath, buildInfoBytes);
    await writeFile(join(directory, spec.trials[0].physicalPath), wavBytes(1_000));
    await writeFile(join(directory, spec.trials[0].playerPath), wavBytesWithJunk(1_000, 7));
    await writeFile(specPath, `${JSON.stringify(spec)}\n`);
    const undersized = spawnSync(process.execPath, [
      join(scriptsDirectory, "prepare-dj-abx.mjs"),
      specPath,
      "--build-info", buildInfoPath,
      "--out", packagePath,
      "--codebook", codebookPath,
    ], { encoding: "utf8" });
    assert.equal(undersized.status, 1);
    assert.match(undersized.stderr, /at least 24 trials/);
    const reusedCapture = spawnSync(process.execPath, [
      join(scriptsDirectory, "prepare-dj-abx.mjs"),
      specPath,
      "--build-info", buildInfoPath,
      "--out", packagePath,
      "--codebook", codebookPath,
      "--pilot",
    ], { encoding: "utf8" });
    assert.equal(reusedCapture.status, 1);
    assert.match(reusedCapture.stderr, /reuses capture audio/);
    await writeFile(join(directory, spec.trials[0].playerPath), wavBytes(-1_000));
    const prepare = spawnSync(process.execPath, [
      join(scriptsDirectory, "prepare-dj-abx.mjs"),
      specPath,
      "--build-info", buildInfoPath,
      "--out", packagePath,
      "--codebook", codebookPath,
      "--pilot",
    ], { encoding: "utf8" });
    assert.equal(prepare.status, 0, prepare.stderr);
    const manifestBytes = await readFile(join(packagePath, "blind-manifest.json"));
    const manifest = JSON.parse(manifestBytes);
    const codebook = JSON.parse(await readFile(codebookPath, "utf8"));
    assert.doesNotMatch(JSON.stringify(manifest), /physical|player/);
    assert.equal(manifest.candidate.commit, buildInfo.commit);
    assert.equal(
      manifest.candidate.buildInfoSha256,
      createHash("sha256").update(buildInfoBytes).digest("hex"),
    );
    assert.equal((await readdir(join(packagePath, "audio"))).length, 3);
    assert.equal(new Set(Object.values(manifest.trials[0].audio).map(audio => audio.sha256)).size, 3);
    const codebookBytes = await readFile(codebookPath);
    assert.equal(manifest.codebookSha256, createHash("sha256").update(codebookBytes).digest("hex"));
    assert.deepEqual(codebook.trials[0].sources.physical.wav, codebook.trials[0].sources.player.wav);

    const response = completeSession({ manifest }, "dj-01").exportResponses();
    assert.equal(response.manifestSha256, createHash("sha256").update(manifestBytes).digest("hex"));
    await writeFile(responsePath, `${JSON.stringify(response)}\n`);
    const results = createDjValidationTemplate({ includeExample: false });
    results.candidate = {
      commit: buildInfo.commit,
      worktreeDirty: false,
      settings: structuredClone(candidateSettings),
    };
    const buildArtifact = results.artifacts.find(artifact => artifact.role === "candidate-build-info");
    buildArtifact.path = "player-build-info.json";
    buildArtifact.sha256 = createHash("sha256").update(buildInfoBytes).digest("hex");
    results.participants.push({
      id: "dj-01",
      currentlyActiveDj: true,
      regularlyScratches: true,
      experienceBand: "over-10-years",
      trainingCompleted: true,
      trials: [],
      routines: [],
    });
    await writeFile(resultsPath, `${JSON.stringify(results)}\n`);
    const decode = spawnSync(process.execPath, [
      join(scriptsDirectory, "decode-dj-abx.mjs"),
      codebookPath,
      responsePath,
      "--results", resultsPath,
      "--out", outputPath,
    ], { encoding: "utf8" });
    assert.equal(decode.status, 0, decode.stderr);
    const merged = JSON.parse(await readFile(outputPath, "utf8"));
    assert.equal(merged.participants[0].trials.length, 1);
    codebook.trials[0].aCondition = codebook.trials[0].aCondition === "physical" ? "player" : "physical";
    codebook.trials[0].bCondition = codebook.trials[0].aCondition === "physical" ? "player" : "physical";
    await writeFile(codebookPath, `${JSON.stringify(codebook, null, 2)}\n`);
    const changedMapping = spawnSync(process.execPath, [
      join(scriptsDirectory, "decode-dj-abx.mjs"),
      codebookPath,
      responsePath,
    ], { encoding: "utf8" });
    assert.equal(changedMapping.status, 1);
    assert.match(changedMapping.stderr, /does not match/);
    const repeat = spawnSync(process.execPath, [
      join(scriptsDirectory, "prepare-dj-abx.mjs"),
      specPath,
      "--build-info", buildInfoPath,
      "--out", packagePath,
      "--codebook", codebookPath,
      "--pilot",
    ], { encoding: "utf8" });
    assert.equal(repeat.status, 1);
    assert.match(repeat.stderr, /already exists/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
