export const POINTER_INPUT_PROFILE_SCHEMA_VERSION = 1;

const MAX_INTERVALS = 8_192;
const MAX_DISTINCT_PRESSURES = 64;
const MIN_CONTACT_SAMPLES = 16;
const MIN_SAMPLE_RATE_HZ = 30;

function finite(value) {
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function clamp(value, minimum, maximum) {
  return Math.max(minimum, Math.min(maximum, value));
}

function percentile(values, fraction) {
  if (!values.length) return null;
  const sorted = [...values].sort((left, right) => left - right);
  if (sorted.length === 1) return sorted[0];
  const position = fraction * (sorted.length - 1);
  const lower = Math.floor(position);
  const upper = Math.ceil(position);
  const blend = position - lower;
  return sorted[lower] * (1 - blend) + sorted[upper] * blend;
}

function createRange() {
  return { samples: 0, minimum: Infinity, maximum: -Infinity };
}

function observeRange(range, value) {
  const number = finite(value);
  if (number === null) return;
  range.samples += 1;
  range.minimum = Math.min(range.minimum, number);
  range.maximum = Math.max(range.maximum, number);
}

function rangeSnapshot(range) {
  return {
    samples: range.samples,
    minimum: range.samples ? range.minimum : null,
    maximum: range.samples ? range.maximum : null,
  };
}

function createTypeProfile(pointerType) {
  return {
    pointerType,
    events: 0,
    contactSamples: 0,
    moveSamples: 0,
    coalescedEvents: 0,
    coalescedSamples: 0,
    pressure: createRange(),
    pressureValues: new Set(),
    pressureValuesTruncated: false,
    width: createRange(),
    height: createRange(),
    intervals: [],
  };
}

function normalizedPointerType(value) {
  const pointerType = String(value || "unknown").trim().toLowerCase();
  return pointerType || "unknown";
}

function nonNegativeInteger(value, name) {
  if (!Number.isInteger(value) || value < 0) {
    throw new TypeError(`${name} must be a non-negative integer`);
  }
  return value;
}

function nonNegativeNumberOrNull(value, name) {
  if (value === null) return null;
  if (!Number.isFinite(value) || value < 0) {
    throw new TypeError(`${name} must be null or a non-negative number`);
  }
  return value;
}

function validateRange(value, name, { minimum = -Infinity, maximum = Infinity } = {}) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${name} must be an object`);
  }
  const samples = nonNegativeInteger(value.samples, `${name}.samples`);
  const lower = value.minimum;
  const upper = value.maximum;
  if (samples === 0) {
    if (lower !== null || upper !== null) throw new TypeError(`${name} empty range must use null bounds`);
  } else if (
    !Number.isFinite(lower)
    || !Number.isFinite(upper)
    || lower < minimum
    || upper > maximum
    || lower > upper
  ) {
    throw new TypeError(`${name} bounds are invalid`);
  }
}

function eventSamples(event, eventType) {
  if (eventType !== "pointermove" || typeof event?.getCoalescedEvents !== "function") {
    return [event];
  }
  try {
    const samples = Array.from(event.getCoalescedEvents() || []);
    return samples.length ? samples : [event];
  } catch {
    return [event];
  }
}

function typeSnapshot(profile) {
  const intervals = profile.intervals;
  const medianIntervalMs = percentile(intervals, 0.5);
  const pressure = rangeSnapshot(profile.pressure);
  const pressureValues = [...profile.pressureValues].sort((left, right) => left - right);
  return {
    pointerType: profile.pointerType,
    gripPolicy: profile.pointerType === "pen" ? "pointer-pressure" : "full-contact",
    events: profile.events,
    contactSamples: profile.contactSamples,
    moveSamples: profile.moveSamples,
    coalescedEvents: profile.coalescedEvents,
    coalescedSamples: profile.coalescedSamples,
    pressure: {
      ...pressure,
      distinctValues: pressureValues,
      distinctValuesTruncated: profile.pressureValuesTruncated,
      variable: pressure.samples > 1
        && pressure.maximum - pressure.minimum >= 0.01
        && pressureValues.length > 1,
    },
    contactWidthPx: rangeSnapshot(profile.width),
    contactHeightPx: rangeSnapshot(profile.height),
    sampleIntervalMs: {
      samples: intervals.length,
      minimum: intervals.length ? Math.min(...intervals) : null,
      p50: medianIntervalMs,
      p95: percentile(intervals, 0.95),
      maximum: intervals.length ? Math.max(...intervals) : null,
    },
    medianSampleRateHz: medianIntervalMs > 0 ? 1_000 / medianIntervalMs : null,
  };
}

export function pointerInputProfileRequirements(profile) {
  const reasons = [];
  if (!profile || profile.schemaVersion !== POINTER_INPUT_PROFILE_SCHEMA_VERSION) {
    reasons.push("profile schema is missing or unsupported");
    return Object.freeze({ pass: false, reasons: Object.freeze(reasons) });
  }
  const types = profile.types && typeof profile.types === "object"
    ? Object.values(profile.types)
    : [];
  if (profile.contactSamples < MIN_CONTACT_SAMPLES) {
    reasons.push(`record at least ${MIN_CONTACT_SAMPLES} contact samples`);
  }
  if (!types.length) reasons.push("record at least one pointer type");
  for (const type of types) {
    if (type.moveSamples > 0 && !(type.medianSampleRateHz >= MIN_SAMPLE_RATE_HZ)) {
      reasons.push(`${type.pointerType} median sample rate is below ${MIN_SAMPLE_RATE_HZ} Hz`);
    }
  }
  if (profile.pointerTypes?.includes("touch") && profile.maximumConcurrentPointers < 2) {
    reasons.push("touch validation requires two simultaneous pointers");
  }
  if (profile.pointerCancels > 0 || profile.lostPointerCaptures > 0) {
    reasons.push("the input probe observed pointer cancellation or lost capture");
  }
  return Object.freeze({ pass: reasons.length === 0, reasons: Object.freeze(reasons) });
}

export function validatePointerInputProfile(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError("pointer input profile must be an object");
  }
  if (value.schemaVersion !== POINTER_INPUT_PROFILE_SCHEMA_VERSION) {
    throw new TypeError(`pointer input profile schema must be ${POINTER_INPUT_PROFILE_SCHEMA_VERSION}`);
  }
  if (!Number.isFinite(value.durationMs) || value.durationMs < 0) {
    throw new TypeError("pointer input profile durationMs must be a non-negative number");
  }
  for (const field of [
    "events",
    "contactSamples",
    "maximumConcurrentPointers",
    "pointerCancels",
    "lostPointerCaptures",
  ]) {
    nonNegativeInteger(value[field], `pointer input profile ${field}`);
  }
  if (!Array.isArray(value.pointerTypes) || !value.pointerTypes.length) {
    throw new TypeError("pointer input profile pointerTypes must not be empty");
  }
  const pointerTypes = value.pointerTypes.map((pointerType, index) => {
    const normalized = normalizedPointerType(pointerType);
    if (normalized !== pointerType) {
      throw new TypeError(`pointer input profile pointerTypes[${index}] is not normalized`);
    }
    return normalized;
  });
  if (new Set(pointerTypes).size !== pointerTypes.length) {
    throw new TypeError("pointer input profile pointerTypes must be unique");
  }
  if (!value.types || typeof value.types !== "object" || Array.isArray(value.types)) {
    throw new TypeError("pointer input profile types must be an object");
  }
  const typeNames = Object.keys(value.types).sort();
  if (typeNames.join("\0") !== [...pointerTypes].sort().join("\0")) {
    throw new TypeError("pointer input profile pointerTypes and types do not match");
  }
  for (const pointerType of pointerTypes) {
    const type = value.types[pointerType];
    const prefix = `pointer input profile types.${pointerType}`;
    if (!type || typeof type !== "object" || Array.isArray(type) || type.pointerType !== pointerType) {
      throw new TypeError(`${prefix} is invalid`);
    }
    const expectedGripPolicy = pointerType === "pen" ? "pointer-pressure" : "full-contact";
    if (type.gripPolicy !== expectedGripPolicy) {
      throw new TypeError(`${prefix}.gripPolicy must be ${expectedGripPolicy}`);
    }
    for (const field of ["events", "contactSamples", "moveSamples", "coalescedEvents", "coalescedSamples"]) {
      nonNegativeInteger(type[field], `${prefix}.${field}`);
    }
    validateRange(type.pressure, `${prefix}.pressure`, { minimum: 0, maximum: 1 });
    if (!Array.isArray(type.pressure.distinctValues)
      || type.pressure.distinctValues.some(pressure => !Number.isFinite(pressure) || pressure < 0 || pressure > 1)) {
      throw new TypeError(`${prefix}.pressure.distinctValues is invalid`);
    }
    if (typeof type.pressure.distinctValuesTruncated !== "boolean"
      || typeof type.pressure.variable !== "boolean") {
      throw new TypeError(`${prefix}.pressure flags must be boolean`);
    }
    const expectedVariablePressure = type.pressure.samples > 1
      && type.pressure.maximum - type.pressure.minimum >= 0.01
      && new Set(type.pressure.distinctValues).size > 1;
    if (type.pressure.variable !== expectedVariablePressure) {
      throw new TypeError(`${prefix}.pressure.variable does not match its measured range`);
    }
    validateRange(type.contactWidthPx, `${prefix}.contactWidthPx`, { minimum: 0 });
    validateRange(type.contactHeightPx, `${prefix}.contactHeightPx`, { minimum: 0 });
    const intervals = type.sampleIntervalMs;
    validateRange(intervals, `${prefix}.sampleIntervalMs`, { minimum: 0 });
    for (const field of ["p50", "p95"]) {
      nonNegativeNumberOrNull(intervals[field], `${prefix}.sampleIntervalMs.${field}`);
    }
    if (intervals.samples > 0
      && !(intervals.minimum <= intervals.p50
        && intervals.p50 <= intervals.p95
        && intervals.p95 <= intervals.maximum)) {
      throw new TypeError(`${prefix}.sampleIntervalMs values must be monotonic`);
    }
    nonNegativeNumberOrNull(type.medianSampleRateHz, `${prefix}.medianSampleRateHz`);
    const expectedSampleRateHz = intervals.p50 > 0 ? 1_000 / intervals.p50 : null;
    if ((expectedSampleRateHz === null) !== (type.medianSampleRateHz === null)
      || (expectedSampleRateHz !== null
        && Math.abs(expectedSampleRateHz - type.medianSampleRateHz) > 1e-6)) {
      throw new TypeError(`${prefix}.medianSampleRateHz does not match its measured interval`);
    }
  }
  const totalEvents = pointerTypes.reduce((sum, pointerType) => sum + value.types[pointerType].events, 0);
  const totalContactSamples = pointerTypes.reduce(
    (sum, pointerType) => sum + value.types[pointerType].contactSamples,
    0,
  );
  if (totalEvents !== value.events || totalContactSamples !== value.contactSamples) {
    throw new TypeError("pointer input profile totals do not match its type profiles");
  }
  const output = structuredClone(value);
  output.requirements = pointerInputProfileRequirements(output);
  return output;
}

export function createPointerInputProfiler({ now = () => performance.now() } = {}) {
  const startedAtMs = finite(now()) ?? 0;
  const activePointers = new Set();
  const lastSampleTimes = new Map();
  const profiles = new Map();
  let events = 0;
  let contactSamples = 0;
  let maximumConcurrentPointers = 0;
  let pointerCancels = 0;
  let lostPointerCaptures = 0;

  function profileFor(pointerType) {
    if (!profiles.has(pointerType)) profiles.set(pointerType, createTypeProfile(pointerType));
    return profiles.get(pointerType);
  }

  function observe(eventType, event = {}) {
    const type = String(eventType || event.type || "").toLowerCase();
    if (!["pointerdown", "pointermove", "pointerup", "pointercancel", "lostpointercapture"].includes(type)) {
      throw new RangeError(`Unsupported pointer profile event: ${type}`);
    }
    const pointerId = Number.isFinite(Number(event.pointerId)) ? Number(event.pointerId) : 0;
    const pointerType = normalizedPointerType(event.pointerType);
    const profile = profileFor(pointerType);
    events += 1;
    profile.events += 1;

    if (type === "pointerdown") {
      activePointers.add(pointerId);
      maximumConcurrentPointers = Math.max(maximumConcurrentPointers, activePointers.size);
    } else if (type === "pointerup") {
      activePointers.delete(pointerId);
      lastSampleTimes.delete(pointerId);
    } else if (type === "pointercancel") {
      if (activePointers.has(pointerId)) pointerCancels += 1;
      activePointers.delete(pointerId);
      lastSampleTimes.delete(pointerId);
    } else if (type === "lostpointercapture") {
      if (activePointers.has(pointerId)) lostPointerCaptures += 1;
      activePointers.delete(pointerId);
      lastSampleTimes.delete(pointerId);
    }

    if (type !== "pointerdown" && type !== "pointermove") return snapshot();
    const samples = eventSamples(event, type);
    if (type === "pointermove" && typeof event.getCoalescedEvents === "function") {
      profile.coalescedEvents += 1;
      profile.coalescedSamples += samples.length;
    }
    for (const sample of samples) {
      contactSamples += 1;
      profile.contactSamples += 1;
      if (type === "pointermove") profile.moveSamples += 1;
      const pressure = finite(sample?.pressure);
      if (pressure !== null) {
        const bounded = clamp(pressure, 0, 1);
        const rounded = Math.round(bounded * 10_000) / 10_000;
        observeRange(profile.pressure, bounded);
        if (profile.pressureValues.size < MAX_DISTINCT_PRESSURES) {
          profile.pressureValues.add(rounded);
        } else if (!profile.pressureValues.has(rounded)) {
          profile.pressureValuesTruncated = true;
        }
      }
      observeRange(profile.width, sample?.width);
      observeRange(profile.height, sample?.height);
      const timestamp = finite(sample?.timeStamp);
      const previous = lastSampleTimes.get(pointerId);
      if (timestamp !== null && previous !== undefined) {
        const interval = timestamp - previous;
        if (interval > 0 && interval <= 1_000 && profile.intervals.length < MAX_INTERVALS) {
          profile.intervals.push(interval);
        }
      }
      if (timestamp !== null) lastSampleTimes.set(pointerId, timestamp);
    }
    return snapshot();
  }

  function snapshot() {
    const types = Object.fromEntries(
      [...profiles.entries()]
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([pointerType, profile]) => [pointerType, typeSnapshot(profile)]),
    );
    const output = {
      schemaVersion: POINTER_INPUT_PROFILE_SCHEMA_VERSION,
      durationMs: Math.max(0, (finite(now()) ?? startedAtMs) - startedAtMs),
      events,
      contactSamples,
      maximumConcurrentPointers,
      pointerCancels,
      lostPointerCaptures,
      pointerTypes: Object.keys(types),
      types,
    };
    output.requirements = pointerInputProfileRequirements(output);
    return structuredClone(output);
  }

  return Object.freeze({ observe, snapshot });
}
