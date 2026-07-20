function finiteNonNegative(value, scale = 1) {
  const number = Number(value);
  return Number.isFinite(number) && number >= 0 ? number * scale : null;
}

function emptyStats(api = null, supported = false) {
  return Object.freeze({
    supported,
    api,
    underrunEvents: null,
    underrunDurationMs: null,
    totalDurationMs: null,
    averageLatencyMs: null,
    minimumLatencyMs: null,
    maximumLatencyMs: null,
  });
}

function statsObject(value) {
  if (!value) return null;
  return typeof value.toJSON === "function" ? value.toJSON() : value;
}

/**
 * Normalizes Chromium's current AudioPlaybackStats and its pre-M148
 * AudioPlayoutStats predecessor to one millisecond-based public shape.
 */
export function readAudioPlaybackStats(context) {
  if (!context) return emptyStats();

  let modern = null;
  try {
    modern = context.playbackStats;
  } catch {
    // Some browser/device combinations expose the property but reject reads.
  }
  if (modern) {
    try {
      const stats = statsObject(modern);
      return Object.freeze({
        supported: true,
        api: "playbackStats",
        underrunEvents: finiteNonNegative(stats?.underrunEvents),
        underrunDurationMs: finiteNonNegative(stats?.underrunDuration, 1_000),
        totalDurationMs: finiteNonNegative(stats?.totalDuration, 1_000),
        averageLatencyMs: finiteNonNegative(stats?.averageLatency, 1_000),
        minimumLatencyMs: finiteNonNegative(stats?.minimumLatency, 1_000),
        maximumLatencyMs: finiteNonNegative(stats?.maximumLatency, 1_000),
      });
    } catch {
      return emptyStats("playbackStats", true);
    }
  }

  let legacy = null;
  try {
    legacy = context.playoutStats;
  } catch {
    // The legacy implementation can also reject reads after context teardown.
  }
  if (legacy) {
    try {
      const stats = statsObject(legacy);
      return Object.freeze({
        supported: true,
        api: "playoutStats",
        underrunEvents: finiteNonNegative(stats?.fallbackFramesEvents),
        underrunDurationMs: finiteNonNegative(stats?.fallbackFramesDuration),
        totalDurationMs: finiteNonNegative(stats?.totalFramesDuration),
        averageLatencyMs: finiteNonNegative(stats?.averageLatency),
        minimumLatencyMs: finiteNonNegative(stats?.minimumLatency),
        maximumLatencyMs: finiteNonNegative(stats?.maximumLatency),
      });
    } catch {
      return emptyStats("playoutStats", true);
    }
  }

  return emptyStats();
}
