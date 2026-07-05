(function (root, factory) {
  const api = factory(root);
  root.VinylPlayerMessageLogger = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis, function (root) {
  "use strict";

  const DEFAULT_TELEMETRY_INTERVAL_MS = 1000;
  const TELEMETRY_TYPES = new Set([
    "position",
    "worklet:position",
    "playback_position_observed",
    "dispatch:playback_position_observed",
    "core:dispatch:playback_position_observed",
    "dispatch:response:playback_position_observed"
  ]);

  const state = root.__VIN_YL_PLAYER_LOGGER_STATE__ || {
    enabled: root.__VIN_YL_PLAYER_LOGGING__ !== false,
    level: normalizeLevel(root.__VIN_YL_PLAYER_LOG_LEVEL__ || "normal"),
    sequence: 0,
    startedAt: typeof performance !== "undefined" ? performance.now() : Date.now(),
    telemetryIntervalMs: DEFAULT_TELEMETRY_INTERVAL_MS,
    lastTelemetryAt: new Map(),
    suppressedTelemetry: new Map()
  };
  root.__VIN_YL_PLAYER_LOGGER_STATE__ = state;

  function normalizeLevel(level) {
    return ["quiet", "normal", "verbose"].includes(String(level)) ? String(level) : "normal";
  }

  function setEnabled(enabled) {
    state.enabled = Boolean(enabled);
    root.__VIN_YL_PLAYER_LOGGING__ = state.enabled;
    return state.enabled;
  }

  function isEnabled() {
    return state.enabled;
  }

  function setLevel(level) {
    state.level = normalizeLevel(level);
    root.__VIN_YL_PLAYER_LOG_LEVEL__ = state.level;
    return state.level;
  }

  function getLevel() {
    return state.level;
  }

  function setTelemetryInterval(ms) {
    const next = Number(ms);
    state.telemetryIntervalMs = Number.isFinite(next) && next >= 0 ? next : DEFAULT_TELEMETRY_INTERVAL_MS;
    return state.telemetryIntervalMs;
  }

  function byteLength(value) {
    if (value instanceof ArrayBuffer || (typeof SharedArrayBuffer !== "undefined" && value instanceof SharedArrayBuffer)) return value.byteLength;
    if (ArrayBuffer.isView(value)) return value.byteLength;
    return null;
  }

  function summarize(value, depth = 0, seen = new WeakSet()) {
    if (value == null || typeof value === "string" || typeof value === "number" || typeof value === "boolean") return value;
    const bytes = byteLength(value);
    if (bytes != null) return { kind: value.constructor?.name || "Buffer", byteLength: bytes };
    if (typeof value === "function") return `[function ${value.name || "anonymous"}]`;
    if (typeof value !== "object") return String(value);
    if (seen.has(value)) return "[circular]";
    if (depth >= 3) return Array.isArray(value) ? `[array:${value.length}]` : "[object]";
    seen.add(value);
    if (Array.isArray(value)) {
      const output = value.slice(0, 8).map(item => summarize(item, depth + 1, seen));
      if (value.length > 8) output.push(`[+${value.length - 8} more]`);
      return output;
    }
    const output = {};
    for (const [key, item] of Object.entries(value).slice(0, 24)) output[key] = summarize(item, depth + 1, seen);
    return output;
  }

  function nestedEventType(payload) {
    return payload?.event?.type || payload?.payload?.event?.type || payload?.payload?.payload?.event?.type || "";
  }

  function isTelemetry(type, payload, meta) {
    if (meta?.telemetry === true) return true;
    if (meta?.telemetry === false) return false;
    const resolved = String(type || payload?.type || "");
    const eventType = String(nestedEventType(payload));
    return TELEMETRY_TYPES.has(resolved) || TELEMETRY_TYPES.has(eventType) || resolved.endsWith(":position");
  }

  function shouldEmit(subsystem, direction, type, payload, meta, now) {
    if (!state.enabled) return false;
    if (direction === "error" || direction === "warn") return true;
    if (state.level === "quiet") return false;
    if (state.level === "verbose" || !isTelemetry(type, payload, meta)) return true;
    const key = `${subsystem}:${direction}:${String(type || payload?.type || "message")}:${nestedEventType(payload)}`;
    const last = state.lastTelemetryAt.get(key) ?? -Infinity;
    if (now - last < state.telemetryIntervalMs) {
      state.suppressedTelemetry.set(key, (state.suppressedTelemetry.get(key) || 0) + 1);
      return false;
    }
    state.lastTelemetryAt.set(key, now);
    return true;
  }

  function emit(subsystem, direction, type, payload, meta = {}) {
    const now = typeof performance !== "undefined" ? performance.now() : Date.now();
    if (!shouldEmit(subsystem, direction, type, payload, meta, now)) return null;
    const key = `${subsystem}:${direction}:${String(type || payload?.type || "message")}:${nestedEventType(payload)}`;
    const suppressed = state.suppressedTelemetry.get(key) || 0;
    state.suppressedTelemetry.delete(key);
    const entry = {
      seq: ++state.sequence,
      tMs: Math.round((now - state.startedAt) * 1000) / 1000,
      subsystem,
      direction,
      type: String(type || payload?.type || "message"),
      payload: summarize(payload),
      ...(suppressed ? { suppressedSinceLast: suppressed } : {}),
      ...meta
    };
    const method = direction === "error" ? "error" : direction === "warn" ? "warn" : "log";
    console[method](`[vin.yl.player][${entry.seq}][${subsystem}][${direction}] ${entry.type}`, entry);
    return entry;
  }

  function createLogger(subsystem) {
    return {
      setEnabled,
      isEnabled,
      setLevel,
      getLevel,
      setTelemetryInterval,
      send(type, payload, meta) { return emit(subsystem, "send", type, payload, meta); },
      receive(type, payload, meta) { return emit(subsystem, "receive", type, payload, meta); },
      action(type, payload, meta) { return emit(subsystem, "action", type, payload, meta); },
      state(type, payload, meta) { return emit(subsystem, "state", type, payload, meta); },
      warn(type, payload, meta) { return emit(subsystem, "warn", type, payload, meta); },
      error(type, payload, meta) { return emit(subsystem, "error", type, payload, meta); }
    };
  }

  return { createLogger, emit, summarize, setEnabled, isEnabled, setLevel, getLevel, setTelemetryInterval };
});
