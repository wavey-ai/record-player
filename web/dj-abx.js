import { DJ_GESTURE_FAMILIES } from "./dj-validation-template.js";

export const DJ_ABX_PACKAGE_SCHEMA_VERSION = 1;
export const DJ_ABX_RESPONSE_SCHEMA_VERSION = 1;
export const DJ_ABX_CODEBOOK_SCHEMA_VERSION = 1;

const gestureFamilies = new Set(DJ_GESTURE_FAMILIES);
const roles = new Set(["a", "b", "x"]);
const conditions = new Set(["physical", "player"]);
const studyIdPattern = /^[a-z0-9][a-z0-9._-]{2,63}$/;
const digestPattern = /^[0-9a-f]{64}$/i;
const audioPathPattern = /^audio\/[0-9a-f]{32}\.wav$/;

function clone(value) {
  return structuredClone(value);
}

function object(value, name) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${name} must be an object`);
  }
  return value;
}

function array(value, name, { minimum = 0 } = {}) {
  if (!Array.isArray(value) || value.length < minimum) {
    throw new TypeError(`${name} must contain at least ${minimum} items`);
  }
  return value;
}

function string(value, name, { allowEmpty = false } = {}) {
  if (typeof value !== "string" || (!allowEmpty && value.trim().length === 0)) {
    throw new TypeError(`${name} must ${allowEmpty ? "be a string" : "not be empty"}`);
  }
  return value.trim();
}

function integer(value, name, minimum, maximum) {
  const number = Number(value);
  if (!Number.isInteger(number) || number < minimum || number > maximum) {
    throw new TypeError(`${name} must be an integer from ${minimum} to ${maximum}`);
  }
  return number;
}

function exactKeys(value, keys, name) {
  const expected = [...keys].sort();
  const actual = Object.keys(value).sort();
  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index])) {
    throw new TypeError(`${name} contains unexpected or missing fields`);
  }
}

function unique(values, name) {
  const seen = new Set();
  for (const value of values) {
    if (seen.has(value)) throw new Error(`${name} reuses ${value}`);
    seen.add(value);
  }
}

function studyId(value, name = "studyId") {
  const normalized = string(value, name).toLowerCase();
  if (!studyIdPattern.test(normalized)) {
    throw new TypeError(`${name} must use 3–64 lower-case letters, numbers, dots, underscores or hyphens`);
  }
  return normalized;
}

function digest(value, name) {
  const normalized = string(value, name).toLowerCase();
  if (!digestPattern.test(normalized)) throw new TypeError(`${name} must be a SHA-256 digest`);
  return normalized;
}

function gestureFamily(value, name) {
  const normalized = string(value, name);
  if (!gestureFamilies.has(normalized)) throw new TypeError(`${name} is not registered`);
  return normalized;
}

function condition(value, name) {
  const normalized = string(value, name);
  if (!conditions.has(normalized)) throw new TypeError(`${name} is not physical or player`);
  return normalized;
}

function audioPath(value, name) {
  const normalized = string(value, name);
  if (!audioPathPattern.test(normalized)) {
    throw new TypeError(`${name} must be an opaque 128-bit WAV path under audio/`);
  }
  return normalized;
}

function isoTimestamp(value, name) {
  const normalized = string(value, name);
  if (!Number.isFinite(Date.parse(normalized))) throw new TypeError(`${name} must be an ISO timestamp`);
  return normalized;
}

function validatePreparationTrial(value, index) {
  const path = `trials[${index}]`;
  object(value, path);
  exactKeys(value, ["id", "excerptId", "gestureFamily", "physicalPath", "playerPath"], path);
  const physicalPath = string(value.physicalPath, `${path}.physicalPath`);
  const playerPath = string(value.playerPath, `${path}.playerPath`);
  if (physicalPath === playerPath) throw new Error(`${path} must use distinct physical and player captures`);
  return {
    id: string(value.id, `${path}.id`),
    excerptId: string(value.excerptId, `${path}.excerptId`),
    gestureFamily: gestureFamily(value.gestureFamily, `${path}.gestureFamily`),
    physicalPath,
    playerPath,
  };
}

export function validateDjAbxPreparationSpec(input) {
  const value = object(input, "ABX preparation spec");
  exactKeys(value, ["schemaVersion", "studyId", "participantId", "trials"], "ABX preparation spec");
  if (value.schemaVersion !== DJ_ABX_PACKAGE_SCHEMA_VERSION) {
    throw new TypeError(`ABX preparation spec schemaVersion must be ${DJ_ABX_PACKAGE_SCHEMA_VERSION}`);
  }
  const trials = array(value.trials, "trials", { minimum: 1 }).map(validatePreparationTrial);
  unique(trials.map(trial => trial.id), "trials.id");
  unique(trials.map(trial => trial.excerptId), "trials.excerptId");
  unique(trials.flatMap(trial => [trial.physicalPath, trial.playerPath]), "trials capture paths");
  return {
    schemaVersion: DJ_ABX_PACKAGE_SCHEMA_VERSION,
    studyId: studyId(value.studyId),
    participantId: string(value.participantId, "participantId"),
    trials,
  };
}

function shuffled(values, randomInteger) {
  const output = [...values];
  for (let index = output.length - 1; index > 0; index -= 1) {
    const swapIndex = randomInteger(index + 1);
    if (!Number.isInteger(swapIndex) || swapIndex < 0 || swapIndex > index) {
      throw new TypeError("randomInteger returned an out-of-range value");
    }
    [output[index], output[swapIndex]] = [output[swapIndex], output[index]];
  }
  return output;
}

function balancedConditions(count, draw) {
  const values = [];
  const pairs = Math.floor(count / 2);
  for (let index = 0; index < pairs; index += 1) values.push("physical", "player");
  if (count % 2 === 1) values.push(draw(2) === 0 ? "physical" : "player");
  return shuffled(values, draw);
}

export function createDjAbxBlindPlan(input, {
  randomInteger,
  opaqueAudioPath,
  generatedAt = new Date().toISOString(),
} = {}) {
  if (typeof randomInteger !== "function") throw new TypeError("randomInteger is required");
  if (typeof opaqueAudioPath !== "function") throw new TypeError("opaqueAudioPath is required");
  const spec = validateDjAbxPreparationSpec(input);
  const draw = maximum => {
    const value = randomInteger(maximum);
    if (!Number.isInteger(value) || value < 0 || value >= maximum) {
      throw new TypeError("randomInteger returned an out-of-range value");
    }
    return value;
  };
  const assignments = new Map();
  for (const family of DJ_GESTURE_FAMILIES) {
    const familyTrials = spec.trials.filter(trial => trial.gestureFamily === family);
    const aConditions = balancedConditions(familyTrials.length, draw);
    const xConditions = balancedConditions(familyTrials.length, draw);
    familyTrials.forEach((trial, index) => assignments.set(trial.id, {
      aCondition: aConditions[index],
      xCondition: xConditions[index],
    }));
  }
  const planned = spec.trials.map((trial, index) => {
    const { aCondition, xCondition } = assignments.get(trial.id);
    const bCondition = aCondition === "physical" ? "player" : "physical";
    const publicAudio = Object.fromEntries([...roles].map(role => [role, {
      path: audioPath(opaqueAudioPath({ trial, role, index }), `opaqueAudioPath(${trial.id}, ${role})`),
      sha256: null,
    }]));
    unique(Object.values(publicAudio).map(value => value.path), `${trial.id} audio paths`);
    const sourcePath = conditionValue => (
      conditionValue === "physical" ? trial.physicalPath : trial.playerPath
    );
    return {
      manifestTrial: {
        id: trial.id,
        excerptId: trial.excerptId,
        gestureFamily: trial.gestureFamily,
        audio: publicAudio,
      },
      codebookTrial: {
        id: trial.id,
        excerptId: trial.excerptId,
        gestureFamily: trial.gestureFamily,
        aCondition,
        bCondition,
        xCondition,
        audio: publicAudio,
        sources: {
          physical: { path: trial.physicalPath, sha256: null, wav: null },
          player: { path: trial.playerPath, sha256: null, wav: null },
        },
      },
      copies: [
        { sourcePath: sourcePath(aCondition), outputPath: publicAudio.a.path },
        { sourcePath: sourcePath(bCondition), outputPath: publicAudio.b.path },
        { sourcePath: sourcePath(xCondition), outputPath: publicAudio.x.path },
      ],
    };
  });
  const ordered = shuffled(planned, draw);
  unique(
    ordered.flatMap(value => Object.values(value.manifestTrial.audio).map(audio => audio.path)),
    "blind package audio paths",
  );
  const timestamp = isoTimestamp(generatedAt, "generatedAt");
  return {
    manifest: {
      schemaVersion: DJ_ABX_PACKAGE_SCHEMA_VERSION,
      studyId: spec.studyId,
      participantId: spec.participantId,
      generatedAt: timestamp,
      codebookSha256: null,
      trials: ordered.map(value => value.manifestTrial),
    },
    codebook: {
      schemaVersion: DJ_ABX_CODEBOOK_SCHEMA_VERSION,
      studyId: spec.studyId,
      participantId: spec.participantId,
      generatedAt: timestamp,
      trials: ordered.map(value => value.codebookTrial),
    },
    copies: ordered.flatMap(value => value.copies),
  };
}

function validateAudioDescriptors(value, path) {
  const audio = object(value, path);
  exactKeys(audio, roles, path);
  const normalizedAudio = Object.fromEntries([...roles].map(role => {
    const descriptor = object(audio[role], `${path}.${role}`);
    exactKeys(descriptor, ["path", "sha256"], `${path}.${role}`);
    return [role, {
      path: audioPath(descriptor.path, `${path}.${role}.path`),
      sha256: digest(descriptor.sha256, `${path}.${role}.sha256`),
    }];
  }));
  unique(Object.values(normalizedAudio).map(item => item.path), `${path} paths`);
  unique(Object.values(normalizedAudio).map(item => item.sha256), `${path} hashes`);
  return normalizedAudio;
}

function validateManifestTrial(value, index) {
  const path = `manifest.trials[${index}]`;
  object(value, path);
  exactKeys(value, ["id", "excerptId", "gestureFamily", "audio"], path);
  return {
    id: string(value.id, `${path}.id`),
    excerptId: string(value.excerptId, `${path}.excerptId`),
    gestureFamily: gestureFamily(value.gestureFamily, `${path}.gestureFamily`),
    audio: validateAudioDescriptors(value.audio, `${path}.audio`),
  };
}

export function validateDjAbxBlindManifest(input) {
  const value = object(input, "blind manifest");
  exactKeys(value, ["schemaVersion", "studyId", "participantId", "generatedAt", "codebookSha256", "trials"], "blind manifest");
  if (value.schemaVersion !== DJ_ABX_PACKAGE_SCHEMA_VERSION) {
    throw new TypeError(`blind manifest schemaVersion must be ${DJ_ABX_PACKAGE_SCHEMA_VERSION}`);
  }
  const trials = array(value.trials, "manifest.trials", { minimum: 1 }).map(validateManifestTrial);
  unique(trials.map(trial => trial.id), "manifest.trials.id");
  unique(trials.map(trial => trial.excerptId), "manifest.trials.excerptId");
  unique(trials.flatMap(trial => Object.values(trial.audio).map(value => value.path)), "manifest audio paths");
  unique(trials.flatMap(trial => Object.values(trial.audio).map(value => value.sha256)), "manifest audio hashes");
  return {
    schemaVersion: DJ_ABX_PACKAGE_SCHEMA_VERSION,
    studyId: studyId(value.studyId, "manifest.studyId"),
    participantId: string(value.participantId, "manifest.participantId"),
    generatedAt: isoTimestamp(value.generatedAt, "manifest.generatedAt"),
    codebookSha256: digest(value.codebookSha256, "manifest.codebookSha256"),
    trials,
  };
}

function validateBlindResponseValue(values, name) {
  const value = object(values, name);
  exactKeys(value, [
    "trialId", "excerptId", "gestureFamily", "responseLabel", "confidence", "realism",
    "transientSharpness", "timingNaturalness", "audibleCue",
  ], name);
  const responseLabel = string(value.responseLabel, `${name}.responseLabel`).toLowerCase();
  if (responseLabel !== "a" && responseLabel !== "b") {
    throw new TypeError(`${name}.responseLabel must be a or b`);
  }
  return {
    trialId: string(value.trialId, `${name}.trialId`),
    excerptId: string(value.excerptId, `${name}.excerptId`),
    gestureFamily: gestureFamily(value.gestureFamily, `${name}.gestureFamily`),
    responseLabel,
    confidence: integer(value.confidence, `${name}.confidence`, 1, 5),
    realism: integer(value.realism, `${name}.realism`, 1, 7),
    transientSharpness: integer(value.transientSharpness, `${name}.transientSharpness`, 1, 7),
    timingNaturalness: integer(value.timingNaturalness, `${name}.timingNaturalness`, 1, 7),
    audibleCue: string(value.audibleCue, `${name}.audibleCue`, { allowEmpty: true }),
  };
}

export class DjBlindAbxSession {
  constructor({
    manifest,
    manifestSha256,
    participantId,
    responses = [],
    completedAt = () => new Date().toISOString(),
  }) {
    this.manifest = validateDjAbxBlindManifest(manifest);
    this.manifestSha256 = digest(manifestSha256, "manifestSha256");
    this.codebookSha256 = this.manifest.codebookSha256;
    this.participantId = string(participantId, "participantId");
    if (this.participantId !== this.manifest.participantId) {
      throw new Error("participantId does not match this blind package");
    }
    this.completedAt = completedAt;
    this.responses = array(responses, "restored responses").map((response, index) => {
      const trial = this.manifest.trials[index];
      if (!trial) throw new Error("Restored responses exceed the registered trial count");
      const normalized = validateBlindResponseValue(response, `restored responses[${index}]`);
      if (normalized.trialId !== trial.id
        || normalized.excerptId !== trial.excerptId
        || normalized.gestureFamily !== trial.gestureFamily) {
        throw new Error(`Restored response does not match trial ${trial.id}`);
      }
      return normalized;
    });
    this.listened = new Set();
  }

  get currentTrial() {
    return this.responses.length < this.manifest.trials.length
      ? clone(this.manifest.trials[this.responses.length])
      : null;
  }

  get progress() {
    return Object.freeze({
      completed: this.responses.length,
      total: this.manifest.trials.length,
      finished: this.responses.length === this.manifest.trials.length,
    });
  }

  markListened(role) {
    const normalized = string(role, "audio role").toLowerCase();
    if (!roles.has(normalized)) throw new TypeError("audio role must be a, b or x");
    if (!this.currentTrial) throw new Error("The blind session is complete");
    this.listened.add(normalized);
  }

  recordResponse(values) {
    const trial = this.currentTrial;
    if (!trial) throw new Error("The blind session is complete");
    if ([...roles].some(role => !this.listened.has(role))) {
      throw new Error("Listen to A, B and X before you save the response");
    }
    const response = validateBlindResponseValue({
      ...values,
      trialId: trial.id,
      excerptId: trial.excerptId,
      gestureFamily: trial.gestureFamily,
    }, "response");
    this.responses.push(response);
    this.listened.clear();
    return clone(response);
  }

  exportDraft() {
    return {
      schemaVersion: DJ_ABX_RESPONSE_SCHEMA_VERSION,
      studyId: this.manifest.studyId,
      manifestSha256: this.manifestSha256,
      codebookSha256: this.codebookSha256,
      participantId: this.participantId,
      responses: clone(this.responses),
    };
  }

  exportResponses() {
    if (!this.progress.finished) throw new Error("Complete every blind trial before export");
    return {
      schemaVersion: DJ_ABX_RESPONSE_SCHEMA_VERSION,
      studyId: this.manifest.studyId,
      manifestSha256: this.manifestSha256,
      codebookSha256: this.codebookSha256,
      participantId: this.participantId,
      completedAt: isoTimestamp(this.completedAt(), "completedAt"),
      responses: clone(this.responses),
    };
  }
}

function validateCodebook(input) {
  const value = object(input, "private codebook");
  exactKeys(value, ["schemaVersion", "studyId", "participantId", "generatedAt", "trials"], "private codebook");
  if (value.schemaVersion !== DJ_ABX_CODEBOOK_SCHEMA_VERSION) {
    throw new TypeError(`private codebook schemaVersion must be ${DJ_ABX_CODEBOOK_SCHEMA_VERSION}`);
  }
  const trials = array(value.trials, "codebook.trials", { minimum: 1 }).map((trial, index) => {
    const path = `codebook.trials[${index}]`;
    object(trial, path);
    exactKeys(trial, ["id", "excerptId", "gestureFamily", "aCondition", "bCondition", "xCondition", "audio", "sources"], path);
    const aCondition = condition(trial.aCondition, `${path}.aCondition`);
    const bCondition = condition(trial.bCondition, `${path}.bCondition`);
    if (aCondition === bCondition) throw new Error(`${path} A and B conditions must differ`);
    const xCondition = condition(trial.xCondition, `${path}.xCondition`);
    if (xCondition !== aCondition && xCondition !== bCondition) {
      throw new Error(`${path}.xCondition must repeat A or B`);
    }
    const sources = object(trial.sources, `${path}.sources`);
    exactKeys(sources, conditions, `${path}.sources`);
    const normalizedSources = Object.fromEntries([...conditions].map(conditionName => {
      const source = object(sources[conditionName], `${path}.sources.${conditionName}`);
      exactKeys(source, ["path", "sha256", "wav"], `${path}.sources.${conditionName}`);
      const wav = object(source.wav, `${path}.sources.${conditionName}.wav`);
      exactKeys(wav, ["audioFormat", "channels", "sampleRateHz", "bitsPerSample", "frames"], `${path}.sources.${conditionName}.wav`);
      const audioFormat = integer(wav.audioFormat, `${path}.sources.${conditionName}.wav.audioFormat`, 1, 3);
      const bitsPerSample = integer(wav.bitsPerSample, `${path}.sources.${conditionName}.wav.bitsPerSample`, 16, 32);
      if (![1, 3].includes(audioFormat)) {
        throw new TypeError(`${path}.sources.${conditionName}.wav.audioFormat must be PCM or IEEE float`);
      }
      if (![16, 24, 32].includes(bitsPerSample)) {
        throw new TypeError(`${path}.sources.${conditionName}.wav.bitsPerSample is unsupported`);
      }
      return [conditionName, {
        path: string(source.path, `${path}.sources.${conditionName}.path`),
        sha256: digest(source.sha256, `${path}.sources.${conditionName}.sha256`),
        wav: {
          audioFormat,
          channels: integer(wav.channels, `${path}.sources.${conditionName}.wav.channels`, 1, 2),
          sampleRateHz: integer(wav.sampleRateHz, `${path}.sources.${conditionName}.wav.sampleRateHz`, 44_100, 384_000),
          bitsPerSample,
          frames: integer(wav.frames, `${path}.sources.${conditionName}.wav.frames`, 1, Number.MAX_SAFE_INTEGER),
        },
      }];
    }));
    if (JSON.stringify(normalizedSources.physical.wav) !== JSON.stringify(normalizedSources.player.wav)) {
      throw new Error(`${path} physical and player WAV formats must match exactly`);
    }
    return {
      id: string(trial.id, `${path}.id`),
      excerptId: string(trial.excerptId, `${path}.excerptId`),
      gestureFamily: gestureFamily(trial.gestureFamily, `${path}.gestureFamily`),
      aCondition,
      bCondition,
      xCondition,
      audio: validateAudioDescriptors(trial.audio, `${path}.audio`),
      sources: normalizedSources,
    };
  });
  unique(trials.map(trial => trial.id), "codebook.trials.id");
  unique(trials.map(trial => trial.excerptId), "codebook.trials.excerptId");
  unique(trials.flatMap(trial => Object.values(trial.audio).map(value => value.path)), "codebook audio paths");
  unique(trials.flatMap(trial => Object.values(trial.audio).map(value => value.sha256)), "codebook audio hashes");
  unique(trials.flatMap(trial => Object.values(trial.sources).map(value => value.path)), "codebook source paths");
  unique(trials.flatMap(trial => Object.values(trial.sources).map(value => value.sha256)), "codebook source hashes");
  return {
    schemaVersion: DJ_ABX_CODEBOOK_SCHEMA_VERSION,
    studyId: studyId(value.studyId, "codebook.studyId"),
    participantId: string(value.participantId, "codebook.participantId"),
    generatedAt: isoTimestamp(value.generatedAt, "codebook.generatedAt"),
    trials,
  };
}

function normalizeResponseBundle(input) {
  const value = object(input, "blind response bundle");
  exactKeys(value, ["schemaVersion", "studyId", "manifestSha256", "codebookSha256", "participantId", "completedAt", "responses"], "blind response bundle");
  if (value.schemaVersion !== DJ_ABX_RESPONSE_SCHEMA_VERSION) {
    throw new TypeError(`blind response schemaVersion must be ${DJ_ABX_RESPONSE_SCHEMA_VERSION}`);
  }
  const normalized = {
    schemaVersion: DJ_ABX_RESPONSE_SCHEMA_VERSION,
    studyId: studyId(value.studyId, "response.studyId"),
    manifestSha256: digest(value.manifestSha256, "response.manifestSha256"),
    codebookSha256: digest(value.codebookSha256, "response.codebookSha256"),
    participantId: string(value.participantId, "response.participantId"),
    completedAt: isoTimestamp(value.completedAt, "response.completedAt"),
    responses: array(value.responses, "response.responses", { minimum: 1 })
      .map((response, index) => validateBlindResponseValue(response, `response.responses[${index}]`)),
  };
  unique(normalized.responses.map(response => response.trialId), "response trial IDs");
  return normalized;
}

function validateResponseBundle(input, codebook, manifestSha256, codebookSha256) {
  const normalized = normalizeResponseBundle(input);
  if (normalized.studyId !== codebook.studyId
    || normalized.manifestSha256 !== manifestSha256
    || normalized.codebookSha256 !== codebookSha256) {
    throw new Error("Blind response does not match the private codebook");
  }
  if (normalized.participantId !== codebook.participantId) {
    throw new Error("Blind response participant does not match the private codebook");
  }
  if (normalized.responses.length !== codebook.trials.length) {
    throw new Error("Blind response does not contain every registered trial");
  }
  return normalized;
}

export function createDjAbxCueCodeTemplate(responseInput) {
  const response = normalizeResponseBundle(responseInput);
  return {
    schemaVersion: 1,
    studyId: response.studyId,
    manifestSha256: response.manifestSha256,
    codebookSha256: response.codebookSha256,
    entries: response.responses
      .filter(value => value.audibleCue.length > 0)
      .map(value => ({
        participantId: response.participantId,
        trialId: value.trialId,
        audibleCue: value.audibleCue,
        cueCode: "",
      })),
  };
}

export function reconstructDjAbxBlindManifest(codebookInput, codebookSha256) {
  const codebook = validateCodebook(codebookInput);
  return validateDjAbxBlindManifest({
    schemaVersion: DJ_ABX_PACKAGE_SCHEMA_VERSION,
    studyId: codebook.studyId,
    participantId: codebook.participantId,
    generatedAt: codebook.generatedAt,
    codebookSha256: digest(codebookSha256, "codebookSha256"),
    trials: codebook.trials.map(trial => ({
      id: trial.id,
      excerptId: trial.excerptId,
      gestureFamily: trial.gestureFamily,
      audio: trial.audio,
    })),
  });
}

function cueCodeMap(input, codebook, manifestSha256, codebookSha256) {
  if (input === null || input === undefined) return new Map();
  const value = object(input, "cue codes");
  exactKeys(value, ["schemaVersion", "studyId", "manifestSha256", "codebookSha256", "entries"], "cue codes");
  if (value.schemaVersion !== 1) throw new TypeError("cue codes schemaVersion must be 1");
  if (studyId(value.studyId, "cueCodes.studyId") !== codebook.studyId
    || digest(value.manifestSha256, "cueCodes.manifestSha256") !== manifestSha256
    || digest(value.codebookSha256, "cueCodes.codebookSha256") !== codebookSha256) {
    throw new Error("Cue codes do not match the private codebook");
  }
  const entries = array(value.entries, "cueCodes.entries").map((entry, index) => {
    const path = `cueCodes.entries[${index}]`;
    object(entry, path);
    exactKeys(entry, ["participantId", "trialId", "audibleCue", "cueCode"], path);
    const code = string(entry.cueCode, `${path}.cueCode`);
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(code) || code === "none" || code === "no-cue") {
      throw new TypeError(`${path}.cueCode must identify the cue in lower-case kebab-case`);
    }
    return [
      `${string(entry.participantId, `${path}.participantId`)}\u0000${string(entry.trialId, `${path}.trialId`)}`,
      { audibleCue: string(entry.audibleCue, `${path}.audibleCue`), cueCode: code },
    ];
  });
  unique(entries.map(([key]) => key), "cue code entries");
  return new Map(entries);
}

export function decodeDjBlindAbxResponses(codebookInput, responseInputs, {
  codebookSha256,
  manifestSha256,
  cueCodes = null,
} = {}) {
  const codebook = validateCodebook(codebookInput);
  const expectedCodebookSha256 = digest(codebookSha256, "codebookSha256");
  const expectedManifestSha256 = digest(manifestSha256, "manifestSha256");
  const responses = array(responseInputs, "response bundles", { minimum: 1 })
    .map(input => validateResponseBundle(
      input,
      codebook,
      expectedManifestSha256,
      expectedCodebookSha256,
    ));
  if (responses.length !== 1) throw new Error("Use one participant-bound response bundle per private codebook");
  unique(responses.map(response => response.participantId), "response participant IDs");
  const codes = cueCodeMap(
    cueCodes,
    codebook,
    expectedManifestSha256,
    expectedCodebookSha256,
  );
  const consumedCueKeys = new Set();
  const trialsById = new Map(codebook.trials.map(trial => [trial.id, trial]));
  const participants = responses.map(bundle => ({
    id: bundle.participantId,
    trials: bundle.responses.map(response => {
      const trial = trialsById.get(response.trialId);
      if (!trial
        || trial.excerptId !== response.excerptId
        || trial.gestureFamily !== response.gestureFamily) {
        throw new Error(`Response trial does not match the codebook: ${response.trialId}`);
      }
      const cueKey = `${bundle.participantId}\u0000${trial.id}`;
      const codedCue = response.audibleCue.length > 0 ? codes.get(cueKey) : null;
      if (response.audibleCue.length > 0 && !codedCue) {
        throw new Error(`A frozen cue code is required for ${bundle.participantId}/${trial.id}`);
      }
      if (codedCue && codedCue.audibleCue !== response.audibleCue) {
        throw new Error(`Frozen cue text does not match ${bundle.participantId}/${trial.id}`);
      }
      if (response.audibleCue.length === 0 && codes.has(cueKey)) {
        throw new Error(`Cue code exists without a reported cue for ${bundle.participantId}/${trial.id}`);
      }
      if (codedCue) consumedCueKeys.add(cueKey);
      return {
        id: trial.id,
        excerptId: trial.excerptId,
        gestureFamily: trial.gestureFamily,
        aCondition: trial.aCondition,
        bCondition: trial.bCondition,
        xCondition: trial.xCondition,
        responseCondition: response.responseLabel === "a" ? trial.aCondition : trial.bCondition,
        confidence: response.confidence,
        realism: response.realism,
        transientSharpness: response.transientSharpness,
        timingNaturalness: response.timingNaturalness,
        audibleCue: response.audibleCue,
        cueCode: codedCue?.cueCode ?? null,
      };
    }),
  }));
  if (consumedCueKeys.size !== codes.size) {
    throw new Error("Cue codes contain an entry that does not match a reported cue");
  }
  return {
    schemaVersion: 1,
    studyId: codebook.studyId,
    manifestSha256: expectedManifestSha256,
    codebookSha256: expectedCodebookSha256,
    participants,
  };
}
