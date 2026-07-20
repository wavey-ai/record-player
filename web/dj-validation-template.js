export const DJ_VALIDATION_SCHEMA_VERSION = 3;

export const DJ_GESTURE_FAMILIES = Object.freeze([
  "baby-drag-cue",
  "stab-transform",
  "chirp-flare",
  "crab-orbit",
  "fast-release",
  "motor-runout",
]);

export const DJ_SCRATCH_PRESETS = Object.freeze([
  "baby",
  "stab",
  "chirp",
  "transform",
  "flare",
  "crab",
  "orbit",
  "drum",
]);

export const DJ_CONDITIONS = Object.freeze(["physical", "player"]);
export const DJ_BLOCK_KINDS = Object.freeze(["abx", "live"]);
export const DJ_EXPERIENCE_BANDS = Object.freeze([
  "under-2-years",
  "2-5-years",
  "6-10-years",
  "over-10-years",
]);
export const DJ_REQUIRED_ARTIFACT_ROLES = Object.freeze([
  "candidate-build-info",
  "source-master",
  "physical-capture",
  "player-capture",
  "movement-trace",
  "randomization-manifest",
  "cue-codebook",
]);
export const DJ_REQUIRED_PREFLIGHT_CHECKS = Object.freeze([
  "level-and-delay-calibration",
  "mechanics",
  "transport-rates",
  "presets",
  "clocks-and-windows",
  "multi-pointer",
  "limiter-cells",
]);
export const DJ_REQUIRED_SETTINGS = Object.freeze({
  highFrequencyAccelerationLimit: 0.35,
  stylusTracingLimit: 0.72,
  faderCurve: 0.08,
});

function emptyPlaybackStats() {
  return {
    supported: null,
    api: "",
    underrunEvents: null,
    underrunDurationMs: null,
    totalDurationMs: null,
    averageLatencyMs: null,
    minimumLatencyMs: null,
    maximumLatencyMs: null,
  };
}

function exampleParticipant(participantId) {
  return {
    id: participantId,
    currentlyActiveDj: false,
    regularlyScratches: false,
    experienceBand: DJ_EXPERIENCE_BANDS[0],
    trainingCompleted: false,
    trials: [{
      id: `${participantId}-trial-1`,
      excerptId: `${participantId}-excerpt-1`,
      gestureFamily: DJ_GESTURE_FAMILIES[0],
      aCondition: "physical",
      bCondition: "player",
      xCondition: "physical",
      responseCondition: "physical",
      captureSha256: {
        physical: "",
        player: "",
      },
      confidence: null,
      realism: null,
      transientSharpness: null,
      timingNaturalness: null,
      audibleCue: "",
      cueCode: null,
    }],
    routines: [{
      id: `${participantId}-routine-1`,
      assistancePreset: "baby",
      durationSeconds: null,
      instructedAttempts: null,
      successfulAttempts: null,
      missedGrabs: null,
      unintendedCuts: null,
      pointerLosses: null,
      stuckScratchIncidents: null,
      postReleaseMutes: null,
      timingCorrections: null,
      ownershipRating: null,
      timingRating: null,
      assistanceFollowedIntent: null,
      useInRecordedSet: null,
      useInLiveSet: null,
      firstChange: "",
    }],
  };
}

export function createDjValidationTemplate({ includeExample = true } = {}) {
  const participantId = "replace-with-participant-id";
  return {
    schemaVersion: DJ_VALIDATION_SCHEMA_VERSION,
    candidate: {
      commit: "replace-with-tested-commit",
      worktreeDirty: null,
      settings: {
        highFrequencyAccelerationLimit: DJ_REQUIRED_SETTINGS.highFrequencyAccelerationLimit,
        stylusTracingLimit: DJ_REQUIRED_SETTINGS.stylusTracingLimit,
        faderCurve: DJ_REQUIRED_SETTINGS.faderCurve,
        acousticEffects: true,
        surfaceEffects: true,
        nativeRpmValues: [100 / 3, 45],
        endPolicies: ["runout", "clean"],
      },
    },
    environment: {
      browser: "",
      os: "",
      inputDevice: "",
      audioInterface: "",
      listeningTransducers: ["headphones", "monitors"],
      quietRoom: false,
      displaySampleRateHz: null,
      audioContextSampleRateHz: null,
      baseLatencyMs: null,
      outputLatencyMs: null,
      interfaceBufferFrames: null,
      pointerCommandLatencyMs: { p50: null, p95: null, maximum: null },
      acousticLoopback: {
        samples: null,
        sampleRate: null,
        repetitionsRequested: null,
        medianMs: null,
        p95Ms: null,
        maximumMs: null,
        minimumMs: null,
        jitterMs: null,
        minimumCorrelation: null,
        maximumLatencyMs: 500,
        amplitude: 0.08,
        inputDeviceLabel: "",
        inputDeviceSettings: {
          echoCancellation: false,
          noiseSuppression: false,
          autoGainControl: false,
        },
        outputDeviceId: null,
      },
    },
    artifacts: DJ_REQUIRED_ARTIFACT_ROLES.map(role => ({ role, path: "", sha256: "" })),
    preflight: {
      checks: Object.fromEntries(DJ_REQUIRED_PREFLIGHT_CHECKS.map(check => [check, false])),
      unexpectedClips: 0,
      undecodedZeroExcursions: 0,
      discontinuitiesAboveBound: 0,
      levelMismatchDb: null,
      fixedPathDelayMs: null,
      declickBound: null,
      maximumAdjacentDiscontinuity: null,
    },
    blinding: {
      participantConditionLabelsHidden: false,
      operatorConditionLabelsHidden: false,
      assistancePresetHidden: false,
      randomizationGeneratedBeforeSession: false,
      decodedAfterResultsFrozen: false,
    },
    cueCoding: {
      coderCount: 0,
      conditionLabelsHidden: false,
      differencesResolvedBeforeUnblinding: false,
    },
    exclusions: [],
    blocks: includeExample ? DJ_BLOCK_KINDS.map(kind => ({
      id: `${participantId}-${kind}`,
      participantId,
      kind,
      before: emptyPlaybackStats(),
      after: emptyPlaybackStats(),
    })) : [],
    participants: includeExample ? [exampleParticipant(participantId)] : [],
  };
}
