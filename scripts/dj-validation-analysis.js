export const DJ_VALIDATION_SCHEMA_VERSION = 2;

export const DJ_GESTURE_FAMILIES = Object.freeze([
  "baby-drag-cue",
  "stab-transform",
  "chirp-flare",
  "crab-orbit",
  "fast-release",
  "motor-runout",
]);

const SCRATCH_PRESETS = new Set([
  "baby",
  "stab",
  "chirp",
  "transform",
  "flare",
  "crab",
  "orbit",
  "drum",
]);
const CONDITIONS = new Set(["physical", "player"]);
const BLOCK_KINDS = new Set(["abx", "live"]);
const EXPERIENCE_BANDS = new Set([
  "under-2-years",
  "2-5-years",
  "6-10-years",
  "over-10-years",
]);
const REQUIRED_ARTIFACT_ROLES = Object.freeze([
  "source-master",
  "physical-capture",
  "player-capture",
  "movement-trace",
  "randomization-manifest",
  "cue-codebook",
]);
const REQUIRED_PREFLIGHT_CHECKS = Object.freeze([
  "level-and-delay-calibration",
  "mechanics",
  "transport-rates",
  "presets",
  "clocks-and-windows",
  "multi-pointer",
  "limiter-cells",
]);
const REQUIRED_SETTINGS = Object.freeze({
  highFrequencyAccelerationLimit: 0.35,
  stylusTracingLimit: 0.72,
  faderCurve: 0.08,
});
const EXACT_TEST_ALPHA = 0.05;
const MAX_IDENTIFICATION_UPPER_BOUND = 0.60;
const MAX_REPEATABLE_CUE_FRACTION = 0.25;
const MIN_RENDERED_REALISM = 6;
const MIN_LIVE_SUCCESS_RATE = 0.90;
const MIN_LIVE_RATING = 6;
const MIN_PARTICIPANTS = 12;
const MIN_REGULAR_SCRATCH_DJS = 6;
const MIN_TRIALS_PER_PARTICIPANT = 24;
const ROUTINES_PER_PARTICIPANT = 5;
const MAX_POINTER_COMMAND_P95_MS = 20;
const MAX_ACOUSTIC_LOOPBACK_P95_MS = 30;
const MAX_ACOUSTIC_LOOPBACK_JITTER_MS = 3;
const MIN_ACOUSTIC_LOOPBACK_CORRELATION = 0.15;

function assertion(condition, message) {
  if (!condition) throw new TypeError(message);
}

function object(value, path) {
  assertion(value && typeof value === "object" && !Array.isArray(value), `${path} must be an object`);
  return value;
}

function array(value, path) {
  assertion(Array.isArray(value), `${path} must be an array`);
  return value;
}

function string(value, path, { allowEmpty = false } = {}) {
  assertion(typeof value === "string", `${path} must be a string`);
  const normalized = value.trim();
  assertion(allowEmpty || normalized.length > 0, `${path} must not be empty`);
  return normalized;
}

function number(value, path, { minimum = -Infinity, maximum = Infinity, integer = false } = {}) {
  assertion(Number.isFinite(value), `${path} must be a finite number`);
  assertion(value >= minimum && value <= maximum, `${path} is outside its allowed range`);
  assertion(!integer || Number.isInteger(value), `${path} must be an integer`);
  return value;
}

function boolean(value, path) {
  assertion(typeof value === "boolean", `${path} must be a boolean`);
  return value;
}

function optionalNonNegative(value, path) {
  if (value === null) return null;
  return number(value, path, { minimum: 0 });
}

function enumValue(value, allowed, path) {
  const normalized = string(value, path);
  assertion(allowed.has(normalized), `${path} has an unsupported value: ${normalized}`);
  return normalized;
}

function uniqueIds(values, path) {
  const ids = new Set();
  for (let index = 0; index < values.length; index += 1) {
    const id = string(values[index].id, `${path}[${index}].id`);
    assertion(!ids.has(id), `${path} contains duplicate id: ${id}`);
    ids.add(id);
  }
  return ids;
}

function median(values) {
  if (values.length === 0) return null;
  const ordered = [...values].sort((left, right) => left - right);
  const middle = Math.floor(ordered.length / 2);
  if (ordered.length % 2 === 1) return ordered[middle];
  return (ordered[middle - 1] + ordered[middle]) / 2;
}

function logGamma(value) {
  const coefficients = [
    676.5203681218851,
    -1259.1392167224028,
    771.3234287776531,
    -176.6150291621406,
    12.507343278686905,
    -0.13857109526572012,
    9.984369578019572e-6,
    1.5056327351493116e-7,
  ];
  if (value < 0.5) {
    return Math.log(Math.PI) - Math.log(Math.sin(Math.PI * value)) - logGamma(1 - value);
  }
  const shifted = value - 1;
  let series = 0.9999999999998099;
  for (let index = 0; index < coefficients.length; index += 1) {
    series += coefficients[index] / (shifted + index + 1);
  }
  const t = shifted + coefficients.length - 0.5;
  return 0.5 * Math.log(2 * Math.PI) + (shifted + 0.5) * Math.log(t) - t + Math.log(series);
}

function logBinomialProbability(successes, trials, probability) {
  if (probability === 0) return successes === 0 ? 0 : -Infinity;
  if (probability === 1) return successes === trials ? 0 : -Infinity;
  return logGamma(trials + 1)
    - logGamma(successes + 1)
    - logGamma(trials - successes + 1)
    + successes * Math.log(probability)
    + (trials - successes) * Math.log1p(-probability);
}

function probabilitySum(logProbabilities) {
  const maximum = Math.max(...logProbabilities);
  if (maximum === -Infinity) return 0;
  let scaled = 0;
  for (const value of logProbabilities) scaled += Math.exp(value - maximum);
  const result = Math.exp(maximum) * scaled;
  if (Math.abs(1 - result) <= 1e-12) return 1;
  return Math.min(1, result);
}

function binomialCdf(successes, trials, probability) {
  if (successes < 0) return 0;
  if (successes >= trials) return 1;
  const terms = [];
  for (let value = 0; value <= successes; value += 1) {
    terms.push(logBinomialProbability(value, trials, probability));
  }
  return probabilitySum(terms);
}

function binomialSurvival(successes, trials, probability) {
  if (successes <= 0) return 1;
  if (successes > trials) return 0;
  const terms = [];
  for (let value = successes; value <= trials; value += 1) {
    terms.push(logBinomialProbability(value, trials, probability));
  }
  return probabilitySum(terms);
}

function bisectIncreasing(target, evaluate) {
  let lower = 0;
  let upper = 1;
  for (let iteration = 0; iteration < 80; iteration += 1) {
    const midpoint = (lower + upper) / 2;
    if (evaluate(midpoint) < target) lower = midpoint;
    else upper = midpoint;
  }
  return (lower + upper) / 2;
}

function bisectDecreasing(target, evaluate) {
  let lower = 0;
  let upper = 1;
  for (let iteration = 0; iteration < 80; iteration += 1) {
    const midpoint = (lower + upper) / 2;
    if (evaluate(midpoint) > target) lower = midpoint;
    else upper = midpoint;
  }
  return (lower + upper) / 2;
}

export function exactBinomialTwoSidedP(successes, trials, probability = 0.5) {
  number(successes, "successes", { minimum: 0, integer: true });
  number(trials, "trials", { minimum: 1, integer: true });
  assertion(successes <= trials, "successes must not exceed trials");
  number(probability, "probability", { minimum: 0, maximum: 1 });
  assertion(probability > 0 && probability < 1, "probability must be between zero and one");

  const observed = logBinomialProbability(successes, trials, probability);
  const included = [];
  for (let value = 0; value <= trials; value += 1) {
    const candidate = logBinomialProbability(value, trials, probability);
    if (candidate <= observed + 1e-12) included.push(candidate);
  }
  return probabilitySum(included);
}

export function clopperPearsonInterval(successes, trials, confidence = 0.95) {
  number(successes, "successes", { minimum: 0, integer: true });
  number(trials, "trials", { minimum: 1, integer: true });
  assertion(successes <= trials, "successes must not exceed trials");
  number(confidence, "confidence", { minimum: 0, maximum: 1 });
  assertion(confidence > 0 && confidence < 1, "confidence must be between zero and one");

  const tail = (1 - confidence) / 2;
  const lower = successes === 0
    ? 0
    : bisectIncreasing(tail, probability => binomialSurvival(successes, trials, probability));
  const upper = successes === trials
    ? 1
    : bisectDecreasing(tail, probability => binomialCdf(successes, trials, probability));
  return Object.freeze({ lower, upper });
}

function validatePlaybackStats(value, path) {
  const stats = object(value, path);
  assertion(stats.supported === true, `${path}.supported must be true`);
  string(stats.api, `${path}.api`);
  number(stats.underrunEvents, `${path}.underrunEvents`, { minimum: 0, integer: true });
  number(stats.underrunDurationMs, `${path}.underrunDurationMs`, { minimum: 0 });
  const totalDurationMs = number(stats.totalDurationMs, `${path}.totalDurationMs`, { minimum: 0 });
  const averageLatencyMs = number(stats.averageLatencyMs, `${path}.averageLatencyMs`, { minimum: 0 });
  const minimumLatencyMs = number(stats.minimumLatencyMs, `${path}.minimumLatencyMs`, { minimum: 0 });
  const maximumLatencyMs = number(stats.maximumLatencyMs, `${path}.maximumLatencyMs`, { minimum: 0 });
  assertion(minimumLatencyMs <= averageLatencyMs, `${path} minimum latency exceeds average latency`);
  assertion(averageLatencyMs <= maximumLatencyMs, `${path} average latency exceeds maximum latency`);
  return { ...stats, totalDurationMs };
}

function validateEnvironment(value) {
  const environment = object(value, "environment");
  const pointerLatency = object(environment.pointerCommandLatencyMs, "environment.pointerCommandLatencyMs");
  const p50 = number(pointerLatency.p50, "environment.pointerCommandLatencyMs.p50", { minimum: 0 });
  const p95 = number(pointerLatency.p95, "environment.pointerCommandLatencyMs.p95", { minimum: 0 });
  const maximum = number(pointerLatency.maximum, "environment.pointerCommandLatencyMs.maximum", { minimum: 0 });
  assertion(p50 <= p95 && p95 <= maximum, "pointer latency percentiles must be monotonic");
  const acoustic = object(environment.acousticLoopback, "environment.acousticLoopback");
  const acousticSamples = number(acoustic.samples, "environment.acousticLoopback.samples", { minimum: 3, integer: true });
  const acousticSampleRate = number(acoustic.sampleRate, "environment.acousticLoopback.sampleRate", { minimum: 1 });
  const acousticMinimum = number(acoustic.minimumMs, "environment.acousticLoopback.minimumMs", { minimum: 0 });
  const acousticMedian = number(acoustic.medianMs, "environment.acousticLoopback.medianMs", { minimum: 0 });
  const acousticP95 = number(acoustic.p95Ms, "environment.acousticLoopback.p95Ms", { minimum: 0 });
  const acousticMaximum = number(acoustic.maximumMs, "environment.acousticLoopback.maximumMs", { minimum: 0 });
  const acousticJitter = number(acoustic.jitterMs, "environment.acousticLoopback.jitterMs", { minimum: 0 });
  const acousticMinimumCorrelation = number(acoustic.minimumCorrelation, "environment.acousticLoopback.minimumCorrelation", { minimum: 0, maximum: 1 });
  const repetitionsRequested = number(acoustic.repetitionsRequested, "environment.acousticLoopback.repetitionsRequested", { minimum: 3, integer: true });
  const acousticSearchLimit = number(acoustic.maximumLatencyMs, "environment.acousticLoopback.maximumLatencyMs", { minimum: 1 });
  number(acoustic.amplitude, "environment.acousticLoopback.amplitude", { minimum: 0.005, maximum: 0.25 });
  assertion(repetitionsRequested >= acousticSamples, "acoustic loopback samples exceed requested repetitions");
  assertion(acousticSearchLimit >= acousticMaximum, "acoustic loopback result exceeds its search limit");
  assertion(
    acousticMinimum <= acousticMedian && acousticMedian <= acousticP95 && acousticP95 <= acousticMaximum,
    "acoustic loopback latency values must be monotonic",
  );
  assertion(
    Math.abs(acousticJitter - (acousticMaximum - acousticMinimum)) <= 1_000 / acousticSampleRate + 1e-9,
    "acoustic loopback jitter does not match the latency range",
  );
  string(acoustic.inputDeviceLabel, "environment.acousticLoopback.inputDeviceLabel");
  const inputDeviceSettings = object(acoustic.inputDeviceSettings, "environment.acousticLoopback.inputDeviceSettings");
  for (const field of ["echoCancellation", "noiseSuppression", "autoGainControl"]) {
    assertion(inputDeviceSettings[field] !== true, `environment.acousticLoopback.inputDeviceSettings.${field} must not be enabled`);
  }
  if (acoustic.outputDeviceId !== null) string(acoustic.outputDeviceId, "environment.acousticLoopback.outputDeviceId", { allowEmpty: true });
  string(environment.browser, "environment.browser");
  string(environment.os, "environment.os");
  string(environment.inputDevice, "environment.inputDevice");
  string(environment.audioInterface, "environment.audioInterface");
  const listeningTransducers = array(environment.listeningTransducers, "environment.listeningTransducers");
  listeningTransducers.forEach((value, index) => string(value, `environment.listeningTransducers[${index}]`));
  assertion(listeningTransducers.includes("headphones"), "environment.listeningTransducers must include headphones");
  assertion(listeningTransducers.includes("monitors"), "environment.listeningTransducers must include monitors");
  boolean(environment.quietRoom, "environment.quietRoom");
  number(environment.displaySampleRateHz, "environment.displaySampleRateHz", { minimum: 1 });
  const audioContextSampleRateHz = number(environment.audioContextSampleRateHz, "environment.audioContextSampleRateHz", { minimum: 1 });
  assertion(acousticSampleRate === audioContextSampleRateHz, "acoustic loopback and AudioContext sample rates must match");
  number(environment.baseLatencyMs, "environment.baseLatencyMs", { minimum: 0 });
  optionalNonNegative(environment.outputLatencyMs, "environment.outputLatencyMs");
  number(environment.interfaceBufferFrames, "environment.interfaceBufferFrames", { minimum: 1, integer: true });
  return {
    pointerP95Ms: p95,
    acousticP95Ms: acousticP95,
    acousticJitterMs: acousticJitter,
    acousticMinimumCorrelation,
  };
}

function validateTrial(trial, path) {
  object(trial, path);
  string(trial.id, `${path}.id`);
  string(trial.excerptId, `${path}.excerptId`);
  enumValue(trial.gestureFamily, new Set(DJ_GESTURE_FAMILIES), `${path}.gestureFamily`);
  const aCondition = enumValue(trial.aCondition, CONDITIONS, `${path}.aCondition`);
  const bCondition = enumValue(trial.bCondition, CONDITIONS, `${path}.bCondition`);
  assertion(aCondition !== bCondition, `${path}.aCondition and bCondition must differ`);
  enumValue(trial.xCondition, CONDITIONS, `${path}.xCondition`);
  enumValue(trial.responseCondition, CONDITIONS, `${path}.responseCondition`);
  number(trial.confidence, `${path}.confidence`, { minimum: 1, maximum: 5, integer: true });
  number(trial.realism, `${path}.realism`, { minimum: 1, maximum: 7, integer: true });
  number(trial.transientSharpness, `${path}.transientSharpness`, { minimum: 1, maximum: 7, integer: true });
  number(trial.timingNaturalness, `${path}.timingNaturalness`, { minimum: 1, maximum: 7, integer: true });
  const audibleCue = string(trial.audibleCue, `${path}.audibleCue`, { allowEmpty: true });
  const cueCode = trial.cueCode === null ? null : string(trial.cueCode, `${path}.cueCode`);
  if (audibleCue.length > 0) {
    assertion(cueCode !== null, `${path}.cueCode is required when audibleCue is not empty`);
    assertion(/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(cueCode), `${path}.cueCode must use lower-case kebab-case`);
    assertion(cueCode !== "none" && cueCode !== "no-cue", `${path}.cueCode must identify the reported cue`);
  } else {
    assertion(cueCode === null, `${path}.cueCode must be null when audibleCue is empty`);
  }
}

function validateRoutine(routine, path) {
  object(routine, path);
  string(routine.id, `${path}.id`);
  enumValue(routine.assistancePreset, SCRATCH_PRESETS, `${path}.assistancePreset`);
  number(routine.durationSeconds, `${path}.durationSeconds`, { minimum: 60 });
  const instructedAttempts = number(routine.instructedAttempts, `${path}.instructedAttempts`, { minimum: 0, integer: true });
  const successfulAttempts = number(routine.successfulAttempts, `${path}.successfulAttempts`, { minimum: 0, integer: true });
  assertion(successfulAttempts <= instructedAttempts, `${path}.successfulAttempts exceeds instructedAttempts`);
  for (const field of [
    "missedGrabs",
    "unintendedCuts",
    "pointerLosses",
    "stuckScratchIncidents",
    "postReleaseMutes",
    "timingCorrections",
  ]) {
    number(routine[field], `${path}.${field}`, { minimum: 0, integer: true });
  }
  number(routine.ownershipRating, `${path}.ownershipRating`, { minimum: 1, maximum: 7, integer: true });
  number(routine.timingRating, `${path}.timingRating`, { minimum: 1, maximum: 7, integer: true });
  boolean(routine.assistanceFollowedIntent, `${path}.assistanceFollowedIntent`);
  boolean(routine.useInRecordedSet, `${path}.useInRecordedSet`);
  boolean(routine.useInLiveSet, `${path}.useInLiveSet`);
  string(routine.firstChange, `${path}.firstChange`);
}

function criterion(pass, value, requirement) {
  return Object.freeze({ pass: Boolean(pass), value, requirement });
}

function closeTo(value, expected) {
  return Number.isFinite(value) && Math.abs(value - expected) <= 1e-12;
}

export function analyzeDjValidation(input, { sourceSha256 = null, verifiedArtifactRoles = [] } = {}) {
  const data = object(input, "results");
  if (data.schemaVersion === 1) {
    throw new RangeError(
      "DJ validation schema version 1 lacks physical-loopback evidence; generate a version 2 template",
    );
  }
  assertion(data.schemaVersion === DJ_VALIDATION_SCHEMA_VERSION, `Unsupported DJ validation schema version: ${data.schemaVersion}`);
  if (sourceSha256 !== null) {
    assertion(/^[0-9a-f]{64}$/i.test(sourceSha256), "sourceSha256 must be a SHA-256 digest");
  }

  const candidate = object(data.candidate, "candidate");
  const commit = string(candidate.commit, "candidate.commit");
  assertion(/^[0-9a-f]{7,40}$/i.test(commit), "candidate.commit must be a Git commit hash");
  const settings = object(candidate.settings, "candidate.settings");
  for (const [field] of Object.entries(REQUIRED_SETTINGS)) {
    number(settings[field], `candidate.settings.${field}`, { minimum: 0, maximum: 1 });
  }
  boolean(settings.acousticEffects, "candidate.settings.acousticEffects");
  boolean(settings.surfaceEffects, "candidate.settings.surfaceEffects");
  const nativeRpmValues = array(settings.nativeRpmValues, "candidate.settings.nativeRpmValues");
  nativeRpmValues.forEach((value, index) => number(value, `candidate.settings.nativeRpmValues[${index}]`, { minimum: 1 }));
  const endPolicies = array(settings.endPolicies, "candidate.settings.endPolicies");
  endPolicies.forEach((value, index) => string(value, `candidate.settings.endPolicies[${index}]`));
  const environmentMetrics = validateEnvironment(data.environment);

  const artifacts = array(data.artifacts, "artifacts");
  const artifactPaths = new Set();
  artifacts.forEach((artifact, index) => {
    object(artifact, `artifacts[${index}]`);
    string(artifact.role, `artifacts[${index}].role`);
    const path = string(artifact.path, `artifacts[${index}].path`);
    assertion(!artifactPaths.has(path), `artifacts contains duplicate path: ${path}`);
    artifactPaths.add(path);
    const sha256 = string(artifact.sha256, `artifacts[${index}].sha256`);
    assertion(/^[0-9a-f]{64}$/i.test(sha256), `artifacts[${index}].sha256 must be a SHA-256 digest`);
  });

  const preflight = object(data.preflight, "preflight");
  const preflightChecks = object(preflight.checks, "preflight.checks");
  for (const check of REQUIRED_PREFLIGHT_CHECKS) boolean(preflightChecks[check], `preflight.checks.${check}`);
  number(preflight.unexpectedClips, "preflight.unexpectedClips", { minimum: 0, integer: true });
  number(preflight.undecodedZeroExcursions, "preflight.undecodedZeroExcursions", { minimum: 0, integer: true });
  number(preflight.discontinuitiesAboveBound, "preflight.discontinuitiesAboveBound", { minimum: 0, integer: true });
  const levelMismatchDb = number(preflight.levelMismatchDb, "preflight.levelMismatchDb", { minimum: 0 });
  const fixedPathDelayMs = number(preflight.fixedPathDelayMs, "preflight.fixedPathDelayMs", { minimum: 0 });
  const declickBound = number(preflight.declickBound, "preflight.declickBound", { minimum: Number.EPSILON });
  const maximumAdjacentDiscontinuity = number(preflight.maximumAdjacentDiscontinuity, "preflight.maximumAdjacentDiscontinuity", { minimum: 0 });

  const blinding = object(data.blinding, "blinding");
  boolean(blinding.participantConditionLabelsHidden, "blinding.participantConditionLabelsHidden");
  boolean(blinding.operatorConditionLabelsHidden, "blinding.operatorConditionLabelsHidden");
  boolean(blinding.assistancePresetHidden, "blinding.assistancePresetHidden");
  boolean(blinding.randomizationGeneratedBeforeSession, "blinding.randomizationGeneratedBeforeSession");
  boolean(blinding.decodedAfterResultsFrozen, "blinding.decodedAfterResultsFrozen");
  const cueCoding = object(data.cueCoding, "cueCoding");
  const cueCoderCount = number(cueCoding.coderCount, "cueCoding.coderCount", { minimum: 0, integer: true });
  boolean(cueCoding.conditionLabelsHidden, "cueCoding.conditionLabelsHidden");
  boolean(cueCoding.differencesResolvedBeforeUnblinding, "cueCoding.differencesResolvedBeforeUnblinding");

  const exclusions = array(data.exclusions, "exclusions");
  uniqueIds(exclusions, "exclusions");
  exclusions.forEach((exclusion, index) => {
    object(exclusion, `exclusions[${index}]`);
    string(exclusion.reason, `exclusions[${index}].reason`);
    assertion(exclusion.decidedBeforeUnblinding === true, `exclusions[${index}].decidedBeforeUnblinding must be true`);
  });

  const participants = array(data.participants, "participants");
  const participantIds = uniqueIds(participants, "participants");
  const participantSummaries = [];
  const allTrials = [];
  const allRoutines = [];
  const cueParticipants = new Map();
  const excerptIds = new Set();
  let trialPlanPass = true;

  participants.forEach((participant, participantIndex) => {
    const path = `participants[${participantIndex}]`;
    object(participant, path);
    const id = string(participant.id, `${path}.id`);
    boolean(participant.currentlyActiveDj, `${path}.currentlyActiveDj`);
    boolean(participant.regularlyScratches, `${path}.regularlyScratches`);
    enumValue(participant.experienceBand, EXPERIENCE_BANDS, `${path}.experienceBand`);
    boolean(participant.trainingCompleted, `${path}.trainingCompleted`);
    const trials = array(participant.trials, `${path}.trials`);
    const routines = array(participant.routines, `${path}.routines`);
    uniqueIds(trials, `${path}.trials`);
    uniqueIds(routines, `${path}.routines`);
    trials.forEach((trial, index) => validateTrial(trial, `${path}.trials[${index}]`));
    routines.forEach((routine, index) => validateRoutine(routine, `${path}.routines[${index}]`));

    const familyCounts = Object.fromEntries(DJ_GESTURE_FAMILIES.map(family => [family, 0]));
    const conditionCounts = { physical: 0, player: 0 };
    let correct = 0;
    for (const trial of trials) {
      assertion(!excerptIds.has(trial.excerptId), `${path} reuses excerptId: ${trial.excerptId}`);
      excerptIds.add(trial.excerptId);
      familyCounts[trial.gestureFamily] += 1;
      conditionCounts[trial.xCondition] += 1;
      if (trial.xCondition === trial.responseCondition) correct += 1;
      if (trial.cueCode !== null) {
        const cueKey = `${trial.gestureFamily}\u0000${trial.cueCode}`;
        if (!cueParticipants.has(cueKey)) cueParticipants.set(cueKey, new Set());
        cueParticipants.get(cueKey).add(id);
      }
    }
    const familyValues = Object.values(familyCounts);
    const balancedFamilies = familyValues.length > 0
      && Math.min(...familyValues) > 0
      && Math.max(...familyValues) - Math.min(...familyValues) <= 1;
    const balancedConditions = Math.abs(conditionCounts.physical - conditionCounts.player) <= 1;
    const validTrialPlan = trials.length >= MIN_TRIALS_PER_PARTICIPANT && balancedFamilies && balancedConditions;
    trialPlanPass &&= validTrialPlan && routines.length === ROUTINES_PER_PARTICIPANT;
    const participantInterval = trials.length > 0
      ? clopperPearsonInterval(correct, trials.length)
      : { lower: null, upper: null };
    participantSummaries.push(Object.freeze({
      id,
      trials: trials.length,
      correct,
      accuracy: trials.length > 0 ? correct / trials.length : null,
      exactTwoSidedP: trials.length > 0 ? exactBinomialTwoSidedP(correct, trials.length) : null,
      clopperPearson95: Object.freeze(participantInterval),
      familyCounts: Object.freeze(familyCounts),
      conditionCounts: Object.freeze(conditionCounts),
      routines: routines.length,
      trialPlanPass: validTrialPlan,
    }));
    allTrials.push(...trials.map(trial => ({ ...trial, participantId: id })));
    allRoutines.push(...routines.map(routine => ({ ...routine, participantId: id })));
  });

  const blocks = array(data.blocks, "blocks");
  uniqueIds(blocks, "blocks");
  const blockCoverage = new Map([...participantIds].map(id => [id, new Set()]));
  let zeroUnderruns = true;
  blocks.forEach((block, index) => {
    const path = `blocks[${index}]`;
    object(block, path);
    const participantId = string(block.participantId, `${path}.participantId`);
    assertion(participantIds.has(participantId), `${path}.participantId does not identify a participant`);
    const kind = enumValue(block.kind, BLOCK_KINDS, `${path}.kind`);
    const before = validatePlaybackStats(block.before, `${path}.before`);
    const after = validatePlaybackStats(block.after, `${path}.after`);
    assertion(after.totalDurationMs > before.totalDurationMs, `${path} total duration did not increase`);
    zeroUnderruns &&= before.underrunEvents === 0
      && after.underrunEvents === 0
      && before.underrunDurationMs === 0
      && after.underrunDurationMs === 0;
    blockCoverage.get(participantId).add(kind);
  });
  const completeBlockCoverage = [...blockCoverage.values()].every(kinds => [...BLOCK_KINDS].every(kind => kinds.has(kind)));

  const pooledTrials = allTrials.length;
  const pooledCorrect = allTrials.filter(trial => trial.xCondition === trial.responseCondition).length;
  const pooledAccuracy = pooledTrials > 0 ? pooledCorrect / pooledTrials : null;
  const exactP = pooledTrials > 0 ? exactBinomialTwoSidedP(pooledCorrect, pooledTrials) : null;
  const interval = pooledTrials > 0 ? clopperPearsonInterval(pooledCorrect, pooledTrials) : { lower: null, upper: null };
  const renderedRealismValues = allTrials
    .filter(trial => trial.xCondition === "player")
    .map(trial => trial.realism);
  const renderedRealismMedian = median(renderedRealismValues);

  const cueSummary = [...cueParticipants.entries()].map(([key, ids]) => {
    const [gestureFamily, cueCode] = key.split("\u0000");
    return Object.freeze({
      gestureFamily,
      cueCode,
      participantCount: ids.size,
      participantFraction: participants.length > 0 ? ids.size / participants.length : null,
    });
  }).sort((left, right) => right.participantCount - left.participantCount
    || left.gestureFamily.localeCompare(right.gestureFamily)
    || left.cueCode.localeCompare(right.cueCode));
  const maximumCueFraction = cueSummary[0]?.participantFraction ?? 0;

  const totalInstructedAttempts = allRoutines.reduce((sum, routine) => sum + routine.instructedAttempts, 0);
  const totalSuccessfulAttempts = allRoutines.reduce((sum, routine) => sum + routine.successfulAttempts, 0);
  const liveSuccessRate = totalInstructedAttempts > 0 ? totalSuccessfulAttempts / totalInstructedAttempts : null;
  const ownershipMedian = median(allRoutines.map(routine => routine.ownershipRating));
  const timingMedian = median(allRoutines.map(routine => routine.timingRating));
  const missedGrabs = allRoutines.reduce((sum, routine) => sum + routine.missedGrabs, 0);
  const unintendedCuts = allRoutines.reduce((sum, routine) => sum + routine.unintendedCuts, 0);
  const timingCorrections = allRoutines.reduce((sum, routine) => sum + routine.timingCorrections, 0);
  const pointerLosses = allRoutines.reduce((sum, routine) => sum + routine.pointerLosses, 0);
  const stuckScratchIncidents = allRoutines.reduce((sum, routine) => sum + routine.stuckScratchIncidents, 0);
  const postReleaseMutes = allRoutines.reduce((sum, routine) => sum + routine.postReleaseMutes, 0);
  const assistanceFoughtIntent = allRoutines.filter(routine => !routine.assistanceFollowedIntent).length;
  const recordedSetYes = allRoutines.filter(routine => routine.useInRecordedSet).length;
  const liveSetYes = allRoutines.filter(routine => routine.useInLiveSet).length;

  const artifactRoles = new Set(artifacts.map(artifact => artifact.role));
  const verifiedRoles = new Set(verifiedArtifactRoles);
  const artifactHashesPass = REQUIRED_ARTIFACT_ROLES.every(role => artifactRoles.has(role) && verifiedRoles.has(role));
  const settingsPinned = Object.entries(REQUIRED_SETTINGS)
    .every(([field, expected]) => closeTo(settings[field], expected))
    && nativeRpmValues.some(value => closeTo(value, 100 / 3))
    && nativeRpmValues.some(value => closeTo(value, 45))
    && endPolicies.includes("runout")
    && endPolicies.includes("clean");
  const preflightPass = REQUIRED_PREFLIGHT_CHECKS.every(check => preflightChecks[check])
    && preflight.unexpectedClips === 0
    && preflight.undecodedZeroExcursions === 0
    && preflight.discontinuitiesAboveBound === 0
    && levelMismatchDb <= 0.1
    && maximumAdjacentDiscontinuity <= declickBound;
  const allCurrentlyActive = participants.every(participant => participant.currentlyActiveDj);
  const allTrained = participants.every(participant => participant.trainingCompleted);
  const regularScratchDjs = participants.filter(participant => participant.regularlyScratches).length;
  const studyControlsPass = data.environment.quietRoom
    && blinding.participantConditionLabelsHidden
    && blinding.operatorConditionLabelsHidden
    && blinding.assistancePresetHidden
    && blinding.randomizationGeneratedBeforeSession
    && blinding.decodedAfterResultsFrozen
    && allTrained
    && cueCoderCount >= 2
    && cueCoding.conditionLabelsHidden
    && cueCoding.differencesResolvedBeforeUnblinding;

  const criteria = Object.freeze({
    pinnedCandidate: criterion(settingsPinned, settings, "Use the shipped 0.35/0.72/0.08 settings, both RPM values, and both end policies."),
    artifactHashes: criterion(artifactHashesPass, { declared: [...artifactRoles], verified: [...verifiedRoles] }, "Verify each required source, capture, trace, manifest, and cue-codebook SHA-256 digest."),
    preflight: criterion(preflightPass, preflight, "Pass all mechanical and signal checks with no rejection event."),
    studyControls: criterion(studyControlsPass, { blinding, cueCoding, allTrained, fixedPathDelayMs }, "Use the registered double-blind, training, cue-coding, and room controls."),
    controlLatency: criterion(
      environmentMetrics.pointerP95Ms <= MAX_POINTER_COMMAND_P95_MS,
      environmentMetrics.pointerP95Ms,
      "Keep pointer-command p95 latency at or below 20 ms.",
    ),
    acousticLatency: criterion(
      environmentMetrics.acousticP95Ms <= MAX_ACOUSTIC_LOOPBACK_P95_MS
        && environmentMetrics.acousticJitterMs <= MAX_ACOUSTIC_LOOPBACK_JITTER_MS
        && environmentMetrics.acousticMinimumCorrelation >= MIN_ACOUSTIC_LOOPBACK_CORRELATION,
      environmentMetrics,
      "Keep physical-loopback p95 at or below 30 ms, jitter at or below 3 ms, and correlation at or above 0.15.",
    ),
    participants: criterion(participants.length >= MIN_PARTICIPANTS && allCurrentlyActive, participants.length, "Use at least 12 currently active DJs."),
    scratchExperience: criterion(regularScratchDjs >= MIN_REGULAR_SCRATCH_DJS, regularScratchDjs, "Include at least six DJs who regularly scratch."),
    trialPlan: criterion(trialPlanPass, participantSummaries.map(summary => ({ id: summary.id, pass: summary.trialPlanPass, routines: summary.routines })), "Give each DJ at least 24 balanced trials and exactly five routines."),
    deviceAudio: criterion(zeroUnderruns && completeBlockCoverage, { blocks: blocks.length, zeroUnderruns, completeBlockCoverage }, "Record ABX and live blocks for each DJ with zero underruns."),
    chanceIdentification: criterion(exactP !== null && exactP >= EXACT_TEST_ALPHA, exactP, "The pooled two-sided exact test must not reject chance at p < 0.05."),
    identificationUpperBound: criterion(interval.upper !== null && interval.upper < MAX_IDENTIFICATION_UPPER_BOUND, interval.upper, "The upper exact 95% confidence limit must be below 0.60."),
    repeatableCue: criterion(maximumCueFraction <= MAX_REPEATABLE_CUE_FRACTION, maximumCueFraction, "No cue can occur in more than 25% of participants for one gesture family."),
    renderedRealism: criterion(renderedRealismMedian !== null && renderedRealismMedian >= MIN_RENDERED_REALISM, renderedRealismMedian, "The median rendered-condition realism score must be at least 6/7."),
    liveIncidents: criterion(pointerLosses === 0 && stuckScratchIncidents === 0 && postReleaseMutes === 0, { pointerLosses, stuckScratchIncidents, postReleaseMutes }, "Record no pointer loss, stuck scratch, or post-release mute."),
    liveTechniqueSuccess: criterion(liveSuccessRate !== null && liveSuccessRate >= MIN_LIVE_SUCCESS_RATE, liveSuccessRate, "Complete at least 90% of instructed techniques."),
    liveOwnership: criterion(ownershipMedian !== null && ownershipMedian >= MIN_LIVE_RATING, ownershipMedian, "The median ownership score must be at least 6/7."),
    liveTiming: criterion(timingMedian !== null && timingMedian >= MIN_LIVE_RATING, timingMedian, "The median timing score must be at least 6/7."),
  });
  const accepted = Object.values(criteria).every(value => value.pass);

  return Object.freeze({
    schemaVersion: DJ_VALIDATION_SCHEMA_VERSION,
    sourceSha256,
    candidateCommit: commit,
    accepted,
    environment: Object.freeze({
      ...data.environment,
      listeningTransducers: Object.freeze([...data.environment.listeningTransducers]),
      pointerCommandLatencyMs: Object.freeze({ ...data.environment.pointerCommandLatencyMs }),
    }),
    exclusions: Object.freeze(exclusions.map(exclusion => Object.freeze({ ...exclusion }))),
    participants: Object.freeze(participantSummaries),
    pooledAbx: Object.freeze({
      trials: pooledTrials,
      correct: pooledCorrect,
      accuracy: pooledAccuracy,
      exactTwoSidedP: exactP,
      clopperPearson95: Object.freeze(interval),
      renderedRealismMedian,
      renderedRealismRatings: renderedRealismValues.length,
    }),
    cues: Object.freeze(cueSummary),
    live: Object.freeze({
      routines: allRoutines.length,
      instructedAttempts: totalInstructedAttempts,
      successfulAttempts: totalSuccessfulAttempts,
      successRate: liveSuccessRate,
      ownershipMedian,
      timingMedian,
      missedGrabs,
      unintendedCuts,
      timingCorrections,
      pointerLosses,
      stuckScratchIncidents,
      postReleaseMutes,
      assistanceFoughtIntent,
      recordedSetYes,
      liveSetYes,
    }),
    criteria,
  });
}

export function createDjValidationTemplate() {
  const emptyStats = {
    supported: null,
    api: "",
    underrunEvents: null,
    underrunDurationMs: null,
    totalDurationMs: null,
    averageLatencyMs: null,
    minimumLatencyMs: null,
    maximumLatencyMs: null,
  };
  const participantId = "replace-with-participant-id";
  return {
    schemaVersion: DJ_VALIDATION_SCHEMA_VERSION,
    candidate: {
      commit: "replace-with-tested-commit",
      settings: {
        highFrequencyAccelerationLimit: REQUIRED_SETTINGS.highFrequencyAccelerationLimit,
        stylusTracingLimit: REQUIRED_SETTINGS.stylusTracingLimit,
        faderCurve: REQUIRED_SETTINGS.faderCurve,
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
    artifacts: REQUIRED_ARTIFACT_ROLES.map(role => ({ role, path: "", sha256: "" })),
    preflight: {
      checks: Object.fromEntries(REQUIRED_PREFLIGHT_CHECKS.map(check => [check, false])),
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
    blocks: ["abx", "live"].map(kind => ({
      id: `${participantId}-${kind}`,
      participantId,
      kind,
      before: { ...emptyStats },
      after: { ...emptyStats },
    })),
    participants: [{
      id: participantId,
      currentlyActiveDj: false,
      regularlyScratches: false,
      experienceBand: "under-2-years",
      trainingCompleted: false,
      trials: [{
        id: `${participantId}-trial-1`,
        excerptId: `${participantId}-excerpt-1`,
        gestureFamily: DJ_GESTURE_FAMILIES[0],
        aCondition: "physical",
        bCondition: "player",
        xCondition: "physical",
        responseCondition: "physical",
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
    }],
  };
}
