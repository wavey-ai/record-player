import assert from "node:assert/strict";
import test from "node:test";

import { readAudioPlaybackStats } from "../web/audio-playback-stats.js";

test("reports unavailable playback statistics without inventing zero underruns", () => {
  assert.deepEqual(readAudioPlaybackStats(null), {
    supported: false,
    api: null,
    underrunEvents: null,
    underrunDurationMs: null,
    totalDurationMs: null,
    averageLatencyMs: null,
    minimumLatencyMs: null,
    maximumLatencyMs: null,
  });
});

test("normalizes AudioPlaybackStats seconds to milliseconds", () => {
  const result = readAudioPlaybackStats({
    playbackStats: {
      toJSON: () => ({
        underrunEvents: 2,
        underrunDuration: 0.003,
        totalDuration: 12.5,
        averageLatency: 0.018,
        minimumLatency: 0.012,
        maximumLatency: 0.027,
      }),
    },
  });
  assert.deepEqual(result, {
    supported: true,
    api: "playbackStats",
    underrunEvents: 2,
    underrunDurationMs: 3,
    totalDurationMs: 12_500,
    averageLatencyMs: 18,
    minimumLatencyMs: 12,
    maximumLatencyMs: 27,
  });
  assert.ok(Object.isFrozen(result));
});

test("normalizes legacy AudioPlayoutStats names without rescaling milliseconds", () => {
  assert.deepEqual(readAudioPlaybackStats({
    playoutStats: {
      fallbackFramesEvents: 1,
      fallbackFramesDuration: 2.5,
      totalFramesDuration: 9_000,
      averageLatency: 21,
      minimumLatency: 15,
      maximumLatency: 36,
    },
  }), {
    supported: true,
    api: "playoutStats",
    underrunEvents: 1,
    underrunDurationMs: 2.5,
    totalDurationMs: 9_000,
    averageLatencyMs: 21,
    minimumLatencyMs: 15,
    maximumLatencyMs: 36,
  });
});

test("preserves supported status when a browser rejects a stats read", () => {
  assert.deepEqual(readAudioPlaybackStats({
    playbackStats: {
      toJSON() {
        throw new Error("context closed");
      },
    },
  }), {
    supported: true,
    api: "playbackStats",
    underrunEvents: null,
    underrunDurationMs: null,
    totalDurationMs: null,
    averageLatencyMs: null,
    minimumLatencyMs: null,
    maximumLatencyMs: null,
  });
});
