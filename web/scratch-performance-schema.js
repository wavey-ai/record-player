export const SCRATCH_PERFORMANCE_SCHEMA_VERSION = 2;
export const SCRATCH_GATE_ALGORITHM_VERSION = 1;

export const SCRATCH_PRESETS = Object.freeze([
  "baby",
  "stab",
  "chirp",
  "transform",
  "flare",
  "crab",
  "orbit",
  "drum",
]);

export const SCRATCH_PRESET_DEFAULT_CLICKS = Object.freeze({
  baby: 1,
  stab: 1,
  chirp: 1,
  transform: 2,
  flare: 1,
  crab: 4,
  orbit: 2,
  drum: 1,
});

const EVENT_TYPES = new Set([
  "scratch-start",
  "scratch-motion",
  "scratch-end",
  "scratch-preset",
  "scratch-clicks",
  "manual-crossfader",
]);

const MAX_EVENTS = 65_536;
const MAX_EVENTS_PER_RENDER_QUANTUM = 128;
const RENDER_QUANTUM_FRAMES = 128;
const MAX_RATE = 16;

function finite(value, fallback = 0) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function positiveRate(value, fallback = 48_000) {
  const number = finite(value, fallback);
  if (number <= 0) throw new TypeError("Scratch performance sample rates must be positive");
  return number;
}

function clamp(value, minimum, maximum) {
  return Math.max(minimum, Math.min(maximum, finite(value, minimum)));
}

export function normalizeScratchPreset(value, fallback = "baby") {
  const preset = String(value || "").trim().toLowerCase();
  return SCRATCH_PRESETS.includes(preset) ? preset : fallback;
}

export function normalizeScratchClicks(value, fallback = 1) {
  const number = Number(value);
  return Number.isFinite(number)
    ? Math.max(1, Math.min(8, Math.round(number)))
    : fallback;
}

function normalizeInitialState(value, sourceScale, { legacy = false } = {}) {
  const initial = value && typeof value === "object" ? value : {};
  const crossfader = clamp(initial.manualCrossfader ?? initial.crossfader ?? 0.5, 0, 1);
  const preset = normalizeScratchPreset(initial.preset);
  return {
    positionFrames: Math.max(0, finite(initial.positionFrames) * sourceScale),
    rpm: Math.max(0, finite(initial.rpm, 33.3333333333)),
    nativeRpm: Math.max(1, finite(initial.nativeRpm, 33.3333333333)),
    playbackRate: clamp(initial.playbackRate ?? 1, -MAX_RATE, MAX_RATE),
    volume: clamp(initial.volume ?? 1, 0, 1),
    crossfader,
    manualCrossfader: crossfader,
    motorRunning: Boolean(initial.motorRunning),
    playing: Boolean(initial.playing),
    needleLifted: Boolean(initial.needleLifted),
    preset,
    clicks: normalizeScratchClicks(initial.clicks, SCRATCH_PRESET_DEFAULT_CLICKS[preset]),
    faderCurve: String(initial.faderCurve || "sharp-0.08"),
    highFrequencyAccelerationLimit: clamp(initial.highFrequencyAccelerationLimit ?? (legacy ? 0 : 0.35), 0, 1),
    stylusTracingLimit: clamp(initial.stylusTracingLimit ?? (legacy ? 0 : 0.72), 0, 1),
  };
}

function normalizeEvent(value, index, sourceScale, outputScale) {
  if (!value || typeof value !== "object" || !EVENT_TYPES.has(value.type)) {
    throw new TypeError(`Invalid scratch performance event at index ${index}`);
  }

  const event = {
    type: value.type,
    frameOffset: Math.max(0, Math.round(finite(value.frameOffset) * outputScale)),
  };

  if (value.type.startsWith("scratch-")) {
    if (value.type === "scratch-preset") {
      event.preset = normalizeScratchPreset(value.preset);
    } else if (value.type === "scratch-clicks") {
      event.clicks = normalizeScratchClicks(value.clicks);
    } else {
      event.positionFrames = Math.max(0, finite(value.positionFrames) * sourceScale);
      event.rate = clamp(finite(value.rate, 0), -MAX_RATE, MAX_RATE);
      event.impulse = clamp(value.impulse, 0, 1);
      if (value.type === "scratch-end") event.resumePlayback = value.resumePlayback !== false;
    }
  } else if (value.type === "manual-crossfader") {
    event.value = clamp(value.value, 0, 1);
  }

  return event;
}

/**
 * Validate a stored/imported performance and migrate it onto explicit source and
 * output clocks. Schema v1 used `sampleRate` for both source positions and event
 * timing, even when the AudioContext ran at a different rate.
 */
export function normalizeScratchPerformance(performance, target = {}) {
  if (!performance || typeof performance !== "object" || !Array.isArray(performance.events)) {
    throw new TypeError("A scratch performance with an events array is required");
  }
  if (performance.events.length > MAX_EVENTS) {
    throw new RangeError(`Scratch performance exceeds the ${MAX_EVENTS} event limit`);
  }

  const schemaVersion = Math.floor(finite(performance.schemaVersion, 1));
  if (schemaVersion !== 1 && schemaVersion !== SCRATCH_PERFORMANCE_SCHEMA_VERSION) {
    throw new RangeError(`Unsupported scratch performance schema version: ${schemaVersion}`);
  }

  const storedSourceRate = positiveRate(
    schemaVersion === 1 ? performance.sampleRate : performance.sourceSampleRate,
  );
  const storedOutputRate = positiveRate(
    schemaVersion === 1 ? performance.sampleRate : performance.outputSampleRate,
    storedSourceRate,
  );
  const sourceSampleRate = positiveRate(target.sourceSampleRate, storedSourceRate);
  const outputSampleRate = positiveRate(target.outputSampleRate, storedOutputRate);
  const sourceScale = sourceSampleRate / storedSourceRate;
  const outputScale = outputSampleRate / storedOutputRate;

  const events = performance.events
    .map((event, index) => ({
      event: normalizeEvent(event, index, sourceScale, outputScale),
      index,
    }))
    .sort((a, b) => a.event.frameOffset - b.event.frameOffset || a.index - b.index)
    .map(({ event }) => event);

  let quantumWindowStart = 0;
  for (let index = 0; index < events.length; index += 1) {
    while (
      events[index].frameOffset - events[quantumWindowStart].frameOffset
      >= RENDER_QUANTUM_FRAMES
    ) {
      quantumWindowStart += 1;
    }
    if (index - quantumWindowStart + 1 > MAX_EVENTS_PER_RENDER_QUANTUM) {
      throw new RangeError(
        `Scratch performance exceeds ${MAX_EVENTS_PER_RENDER_QUANTUM} events in a 128-frame render window`,
      );
    }
  }

  const derivedDuration = events.length ? events[events.length - 1].frameOffset : 0;
  const durationFrames = Math.max(
    derivedDuration,
    Math.round(finite(performance.durationFrames, derivedDuration) * outputScale),
  );
  const engine = performance.engine && typeof performance.engine === "object"
    ? performance.engine
    : {};

  return {
    id: String(performance.id || ""),
    schemaVersion: SCRATCH_PERFORMANCE_SCHEMA_VERSION,
    name: String(performance.name || ""),
    recordHash: String(performance.recordHash || ""),
    releaseId: String(performance.releaseId || ""),
    createdAt: String(performance.createdAt || ""),
    sourceSampleRate,
    outputSampleRate,
    durationFrames,
    durationMs: durationFrames / outputSampleRate * 1000,
    engine: {
      name: String(engine.name || "vin.yl.player.acoustic"),
      version: Math.max(1, Math.floor(finite(engine.version, 1))),
      gateAlgorithmVersion: Math.max(
        1,
        Math.floor(finite(engine.gateAlgorithmVersion, SCRATCH_GATE_ALGORITHM_VERSION)),
      ),
      recordProfile: String(engine.recordProfile || ""),
      nativeRpm: Math.max(1, finite(engine.nativeRpm, 33.3333333333)),
    },
    initialState: normalizeInitialState(performance.initialState, sourceScale, {
      legacy: schemaVersion === 1,
    }),
    events,
    effects: {
      acoustic: performance.effects?.acoustic !== false,
      surface: performance.effects?.surface !== false,
    },
  };
}
