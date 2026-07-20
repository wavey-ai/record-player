import {
  createDjValidationTemplate,
  DJ_BLOCK_KINDS,
  DJ_CONDITIONS,
  DJ_EXPERIENCE_BANDS,
  DJ_GESTURE_FAMILIES,
  DJ_REQUIRED_ARTIFACT_ROLES,
  DJ_SCRATCH_PRESETS,
  DJ_VALIDATION_SCHEMA_VERSION,
} from "./dj-validation-template.js";
import {
  pointerInputProfileRequirements,
  validatePointerInputProfile,
} from "./pointer-input-profile.js";

const blockKinds = new Set(DJ_BLOCK_KINDS);
const conditions = new Set(DJ_CONDITIONS);
const experienceBands = new Set(DJ_EXPERIENCE_BANDS);
const gestureFamilies = new Set(DJ_GESTURE_FAMILIES);
const scratchPresets = new Set(DJ_SCRATCH_PRESETS);

function clone(value) {
  return structuredClone(value);
}

function requiredString(value, name) {
  if (typeof value !== "string" || value.trim().length === 0) {
    throw new TypeError(`${name} must not be empty`);
  }
  return value.trim();
}

function finiteNumber(value, name, { minimum = -Infinity, maximum = Infinity, integer = false } = {}) {
  const number = Number(value);
  if (!Number.isFinite(number) || number < minimum || number > maximum || (integer && !Number.isInteger(number))) {
    throw new TypeError(`${name} is outside its allowed range`);
  }
  return number;
}

function requiredBoolean(value, name) {
  if (typeof value !== "boolean") throw new TypeError(`${name} must be a boolean`);
  return value;
}

function requiredDigest(value, name) {
  const normalized = requiredString(value, name).toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(normalized)) throw new TypeError(`${name} must be a SHA-256 digest`);
  return normalized;
}

function hasPinnedPlayerSettings(snapshot) {
  return Math.abs(Number(snapshot?.highFrequencyAccelerationLimit) - 0.35) < 1e-9
    && Math.abs(Number(snapshot?.stylusTracingLimit) - 0.72) < 1e-9
    && snapshot?.acousticEffects === true
    && snapshot?.surfaceEffects === true;
}

function allowed(value, values, name) {
  const normalized = requiredString(value, name);
  if (!values.has(normalized)) throw new TypeError(`${name} has an unsupported value: ${normalized}`);
  return normalized;
}

function percentile(values, fraction) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((left, right) => left - right);
  if (sorted.length === 1) return sorted[0];
  const position = fraction * (sorted.length - 1);
  const lower = Math.floor(position);
  const upper = Math.ceil(position);
  const blend = position - lower;
  return sorted[lower] * (1 - blend) + sorted[upper] * blend;
}

function playbackStats(snapshot, name) {
  const stats = snapshot?.audioPlaybackStats;
  if (!stats?.supported) throw new Error(`${name} does not include supported audio playback statistics`);
  const output = {
    supported: true,
    api: requiredString(stats.api, `${name}.api`),
    underrunEvents: finiteNumber(stats.underrunEvents, `${name}.underrunEvents`, { minimum: 0, integer: true }),
    underrunDurationMs: finiteNumber(stats.underrunDurationMs, `${name}.underrunDurationMs`, { minimum: 0 }),
    totalDurationMs: finiteNumber(stats.totalDurationMs, `${name}.totalDurationMs`, { minimum: 0 }),
    averageLatencyMs: finiteNumber(stats.averageLatencyMs, `${name}.averageLatencyMs`, { minimum: 0 }),
    minimumLatencyMs: finiteNumber(stats.minimumLatencyMs, `${name}.minimumLatencyMs`, { minimum: 0 }),
    maximumLatencyMs: finiteNumber(stats.maximumLatencyMs, `${name}.maximumLatencyMs`, { minimum: 0 }),
  };
  if (output.minimumLatencyMs > output.averageLatencyMs || output.averageLatencyMs > output.maximumLatencyMs) {
    throw new Error(`${name} latency values are not monotonic`);
  }
  return output;
}

function participantById(data, participantId) {
  const participant = data.participants.find(value => value.id === participantId);
  if (!participant) throw new Error(`Unknown participant: ${participantId}`);
  return participant;
}

export class DjValidationSession {
  constructor({ data = createDjValidationTemplate({ includeExample: false }), now = () => Date.now() } = {}) {
    if (data?.schemaVersion !== DJ_VALIDATION_SCHEMA_VERSION || !Array.isArray(data?.participants) || !Array.isArray(data?.blocks)) {
      throw new TypeError(`A DJ validation schema version ${DJ_VALIDATION_SCHEMA_VERSION} collection is required`);
    }
    this.data = clone(data);
    if (this.data.environment?.pointerInputProfile !== null) {
      this.data.environment.pointerInputProfile = validatePointerInputProfile(
        this.data.environment?.pointerInputProfile,
      );
    }
    this.now = now;
    this.activeBlock = null;
  }

  snapshot() {
    return clone(this.data);
  }

  summary() {
    return Object.freeze({
      participants: this.data.participants.length,
      trials: this.data.participants.reduce((sum, participant) => sum + participant.trials.length, 0),
      routines: this.data.participants.reduce((sum, participant) => sum + participant.routines.length, 0),
      blocks: this.data.blocks.length,
      activeBlock: this.activeBlock ? Object.freeze({
        participantId: this.activeBlock.participantId,
        kind: this.activeBlock.kind,
        startedAtMs: this.activeBlock.startedAtMs,
      }) : null,
    });
  }

  setCandidateCommit(value) {
    const commit = requiredString(value, "candidate commit");
    if (!/^[0-9a-f]{7,40}$/i.test(commit)) throw new TypeError("candidate commit must be a Git commit hash");
    this.data.candidate.commit = commit;
    return commit;
  }

  setBuildInfo({ commit, worktreeDirty }) {
    this.setCandidateCommit(commit);
    this.data.candidate.worktreeDirty = requiredBoolean(worktreeDirty, "worktreeDirty");
    return clone(this.data.candidate);
  }

  setEnvironment(patch) {
    if (!patch || typeof patch !== "object" || Array.isArray(patch)) {
      throw new TypeError("environment patch must be an object");
    }
    if (Object.hasOwn(patch, "pointerInputProfile")) {
      throw new TypeError("Use setPointerInputProfile or clearPointerInputProfile for pointer evidence");
    }
    Object.assign(this.data.environment, clone(patch));
    return clone(this.data.environment);
  }

  setPointerInputProfile(profile) {
    if (this.activeBlock) throw new Error("Pointer input evidence cannot change during an audio block");
    const validated = validatePointerInputProfile(profile);
    this.data.environment.pointerInputProfile = validated;
    return clone(validated);
  }

  clearPointerInputProfile() {
    if (this.activeBlock) throw new Error("Pointer input evidence cannot change during an audio block");
    this.data.environment.pointerInputProfile = null;
  }

  setPreflight({ checks = null, ...measurements } = {}) {
    if (checks) Object.assign(this.data.preflight.checks, clone(checks));
    Object.assign(this.data.preflight, clone(measurements));
    return clone(this.data.preflight);
  }

  setBlinding(patch) {
    Object.assign(this.data.blinding, clone(patch));
    return clone(this.data.blinding);
  }

  setCueCoding(patch) {
    Object.assign(this.data.cueCoding, clone(patch));
    return clone(this.data.cueCoding);
  }

  setArtifact(role, { path, sha256 = "" }) {
    const normalizedRole = allowed(role, new Set(DJ_REQUIRED_ARTIFACT_ROLES), "artifact role");
    const artifact = this.data.artifacts.find(value => value.role === normalizedRole);
    artifact.path = requiredString(path, `${normalizedRole} path`);
    artifact.sha256 = typeof sha256 === "string" ? sha256.trim() : "";
    return clone(artifact);
  }

  addExclusion({ id, reason, decidedBeforeUnblinding }) {
    const exclusionId = requiredString(id, "exclusion id");
    if (this.data.exclusions.some(exclusion => exclusion.id === exclusionId)) {
      throw new Error(`Exclusion already exists: ${exclusionId}`);
    }
    const exclusion = {
      id: exclusionId,
      reason: requiredString(reason, "exclusion reason"),
      decidedBeforeUnblinding: requiredBoolean(
        decidedBeforeUnblinding,
        "decidedBeforeUnblinding",
      ),
    };
    this.data.exclusions.push(exclusion);
    return clone(exclusion);
  }

  addParticipant({
    id,
    currentlyActiveDj,
    regularlyScratches,
    experienceBand,
    trainingCompleted,
  }) {
    const participantId = requiredString(id, "participant id");
    if (this.data.participants.some(participant => participant.id === participantId)) {
      throw new Error(`Participant already exists: ${participantId}`);
    }
    const participant = {
      id: participantId,
      currentlyActiveDj: requiredBoolean(currentlyActiveDj, "currentlyActiveDj"),
      regularlyScratches: requiredBoolean(regularlyScratches, "regularlyScratches"),
      experienceBand: allowed(experienceBand, experienceBands, "experienceBand"),
      trainingCompleted: requiredBoolean(trainingCompleted, "trainingCompleted"),
      trials: [],
      routines: [],
    };
    this.data.participants.push(participant);
    return clone(participant);
  }

  addTrial(participantId, values) {
    const participant = participantById(this.data, participantId);
    const id = requiredString(values.id || `${participant.id}-trial-${participant.trials.length + 1}`, "trial id");
    if (participant.trials.some(trial => trial.id === id)) throw new Error(`Trial already exists: ${id}`);
    const excerptId = requiredString(values.excerptId, "excerptId");
    if (this.data.participants.some(entry => entry.trials.some(trial => trial.excerptId === excerptId))) {
      throw new Error(`Excerpt already exists: ${excerptId}`);
    }
    const aCondition = allowed(values.aCondition, conditions, "aCondition");
    const bCondition = allowed(values.bCondition, conditions, "bCondition");
    if (aCondition === bCondition) throw new Error("A and B conditions must differ");
    const audibleCue = typeof values.audibleCue === "string" ? values.audibleCue.trim() : "";
    const cueCode = values.cueCode === null || values.cueCode === undefined || values.cueCode === ""
      ? null
      : requiredString(values.cueCode, "cueCode");
    if ((audibleCue.length > 0) !== (cueCode !== null)) {
      throw new Error("audibleCue and cueCode must either both be present or both be absent");
    }
    const trial = {
      id,
      excerptId,
      gestureFamily: allowed(values.gestureFamily, gestureFamilies, "gestureFamily"),
      aCondition,
      bCondition,
      xCondition: allowed(values.xCondition, conditions, "xCondition"),
      responseCondition: allowed(values.responseCondition, conditions, "responseCondition"),
      captureSha256: {
        physical: requiredDigest(values.captureSha256?.physical, "captureSha256.physical"),
        player: requiredDigest(values.captureSha256?.player, "captureSha256.player"),
      },
      confidence: finiteNumber(values.confidence, "confidence", { minimum: 1, maximum: 5, integer: true }),
      realism: finiteNumber(values.realism, "realism", { minimum: 1, maximum: 7, integer: true }),
      transientSharpness: finiteNumber(values.transientSharpness, "transientSharpness", { minimum: 1, maximum: 7, integer: true }),
      timingNaturalness: finiteNumber(values.timingNaturalness, "timingNaturalness", { minimum: 1, maximum: 7, integer: true }),
      audibleCue,
      cueCode,
    };
    participant.trials.push(trial);
    return clone(trial);
  }

  addRoutine(participantId, values) {
    const participant = participantById(this.data, participantId);
    const id = requiredString(values.id || `${participant.id}-routine-${participant.routines.length + 1}`, "routine id");
    if (participant.routines.some(routine => routine.id === id)) throw new Error(`Routine already exists: ${id}`);
    const instructedAttempts = finiteNumber(values.instructedAttempts, "instructedAttempts", { minimum: 0, integer: true });
    const successfulAttempts = finiteNumber(values.successfulAttempts, "successfulAttempts", { minimum: 0, integer: true });
    if (successfulAttempts > instructedAttempts) throw new Error("successfulAttempts exceeds instructedAttempts");
    const routine = {
      id,
      assistancePreset: allowed(values.assistancePreset, scratchPresets, "assistancePreset"),
      durationSeconds: finiteNumber(values.durationSeconds, "durationSeconds", { minimum: 60 }),
      instructedAttempts,
      successfulAttempts,
      missedGrabs: finiteNumber(values.missedGrabs, "missedGrabs", { minimum: 0, integer: true }),
      unintendedCuts: finiteNumber(values.unintendedCuts, "unintendedCuts", { minimum: 0, integer: true }),
      pointerLosses: finiteNumber(values.pointerLosses, "pointerLosses", { minimum: 0, integer: true }),
      stuckScratchIncidents: finiteNumber(values.stuckScratchIncidents, "stuckScratchIncidents", { minimum: 0, integer: true }),
      postReleaseMutes: finiteNumber(values.postReleaseMutes, "postReleaseMutes", { minimum: 0, integer: true }),
      timingCorrections: finiteNumber(values.timingCorrections, "timingCorrections", { minimum: 0, integer: true }),
      ownershipRating: finiteNumber(values.ownershipRating, "ownershipRating", { minimum: 1, maximum: 7, integer: true }),
      timingRating: finiteNumber(values.timingRating, "timingRating", { minimum: 1, maximum: 7, integer: true }),
      assistanceFollowedIntent: requiredBoolean(values.assistanceFollowedIntent, "assistanceFollowedIntent"),
      useInRecordedSet: requiredBoolean(values.useInRecordedSet, "useInRecordedSet"),
      useInLiveSet: requiredBoolean(values.useInLiveSet, "useInLiveSet"),
      firstChange: requiredString(values.firstChange, "firstChange"),
    };
    participant.routines.push(routine);
    return clone(routine);
  }

  startBlock(participantId, kind, playerSnapshot) {
    if (this.activeBlock) throw new Error("Another audio block is already active");
    participantById(this.data, participantId);
    const blockKind = allowed(kind, blockKinds, "block kind");
    const inputRequirements = pointerInputProfileRequirements(this.data.environment.pointerInputProfile);
    if (!inputRequirements.pass) {
      throw new Error(`Complete the pointer input probe before collection: ${inputRequirements.reasons.join("; ")}`);
    }
    if (!hasPinnedPlayerSettings(playerSnapshot)) {
      throw new Error("The player does not have all shipped acoustic and limiter settings");
    }
    const before = playbackStats(playerSnapshot, "block start");
    this.activeBlock = {
      participantId,
      kind: blockKind,
      before,
      startedAtMs: this.now(),
      pointerCommandLatenciesMs: [],
      lastPointerAppliedCommandId: Number.isFinite(playerSnapshot?.pointerAppliedCommandId)
        ? playerSnapshot.pointerAppliedCommandId
        : null,
      candidateSettingsValid: true,
    };
    this.observePlayerState(playerSnapshot);
    return clone(this.activeBlock);
  }

  observePlayerState(snapshot) {
    if (!snapshot || typeof snapshot !== "object") return;
    if (
      this.activeBlock
      && Number.isFinite(snapshot.pointerAppliedCommandId)
      && snapshot.pointerAppliedCommandId !== this.activeBlock.lastPointerAppliedCommandId
      && Number.isFinite(snapshot.pointerToAudioLatencyMs)
      && snapshot.pointerToAudioLatencyMs >= 0
    ) {
      this.activeBlock.pointerCommandLatenciesMs.push(snapshot.pointerToAudioLatencyMs);
      this.activeBlock.lastPointerAppliedCommandId = snapshot.pointerAppliedCommandId;
    }
    if (this.activeBlock && !hasPinnedPlayerSettings(snapshot)) {
      this.activeBlock.candidateSettingsValid = false;
    }
    if (Number.isFinite(snapshot.outputSampleRate) && snapshot.outputSampleRate > 0) {
      this.data.environment.audioContextSampleRateHz = snapshot.outputSampleRate;
    }
    if (Number.isFinite(snapshot.audioBaseLatencyMs) && snapshot.audioBaseLatencyMs >= 0) {
      this.data.environment.baseLatencyMs = snapshot.audioBaseLatencyMs;
    }
    if (snapshot.audioOutputLatencyMs === null || (Number.isFinite(snapshot.audioOutputLatencyMs) && snapshot.audioOutputLatencyMs >= 0)) {
      this.data.environment.outputLatencyMs = snapshot.audioOutputLatencyMs;
    }
  }

  endBlock(playerSnapshot) {
    if (!this.activeBlock) throw new Error("No audio block is active");
    const after = playbackStats(playerSnapshot, "block end");
    if (after.totalDurationMs <= this.activeBlock.before.totalDurationMs) {
      throw new Error("Audio playback duration did not increase during the block");
    }
    this.observePlayerState(playerSnapshot);
    if (!this.activeBlock.candidateSettingsValid) {
      throw new Error("The candidate settings changed during the audio block");
    }
    const matchingBlocks = this.data.blocks.filter(block => (
      block.participantId === this.activeBlock.participantId && block.kind === this.activeBlock.kind
    ));
    const block = {
      id: `${this.activeBlock.participantId}-${this.activeBlock.kind}-${matchingBlocks.length + 1}`,
      participantId: this.activeBlock.participantId,
      kind: this.activeBlock.kind,
      before: this.activeBlock.before,
      after,
      pointerCommandLatenciesMs: [...this.activeBlock.pointerCommandLatenciesMs],
    };
    this.data.blocks.push(block);
    this.activeBlock = null;
    const pointerLatenciesMs = this.data.blocks.flatMap(value => (
      Array.isArray(value.pointerCommandLatenciesMs) ? value.pointerCommandLatenciesMs : []
    ));
    if (pointerLatenciesMs.length > 0) {
      this.data.environment.pointerCommandLatencyMs = {
        p50: percentile(pointerLatenciesMs, 0.5),
        p95: percentile(pointerLatenciesMs, 0.95),
        maximum: Math.max(...pointerLatenciesMs),
      };
    }
    return clone(block);
  }

  cancelBlock() {
    const cancelled = this.activeBlock ? clone(this.activeBlock) : null;
    this.activeBlock = null;
    return cancelled;
  }

  setAcousticLoopback(result) {
    if (!result || typeof result !== "object") throw new TypeError("loopback result must be an object");
    this.data.environment.acousticLoopback = clone(result);
    return clone(this.data.environment.acousticLoopback);
  }

  exportResults() {
    if (this.activeBlock) throw new Error("End or cancel the active audio block before export");
    return this.snapshot();
  }
}
