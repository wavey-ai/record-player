import assert from "node:assert/strict";
import test from "node:test";

import {
  SCRATCH_GATE_ALGORITHM_VERSION,
  SCRATCH_PRESET_DEFAULT_CLICKS,
  normalizeScratchClicks,
  normalizeScratchPerformance,
  normalizeScratchPreset,
} from "../web/scratch-performance-schema.js";

test("schema v1 migrates source positions and timestamps onto separate clocks", () => {
  const migrated = normalizeScratchPerformance({
    id: "legacy",
    schemaVersion: 1,
    sampleRate: 44_100,
    durationFrames: 44_100,
    initialState: { positionFrames: 22_050, crossfader: 0.25 },
    events: [
      { type: "scratch-start", frameOffset: 0, positionFrames: 22_050, rate: 0, impulse: 0.22 },
      { type: "scratch-motion", frameOffset: 22_050, positionFrames: 44_100, rate: 1, impulse: 0 },
      { type: "scratch-end", frameOffset: 44_100, positionFrames: 66_150, resumePlayback: true },
    ],
  }, { sourceSampleRate: 96_000, outputSampleRate: 48_000 });

  assert.equal(migrated.schemaVersion, 2);
  assert.equal(migrated.sourceSampleRate, 96_000);
  assert.equal(migrated.outputSampleRate, 48_000);
  assert.equal(migrated.durationFrames, 48_000);
  assert.ok(Math.abs(migrated.initialState.positionFrames - 48_000) < 1e-9);
  assert.equal(migrated.initialState.manualCrossfader, 0.25);
  assert.equal(migrated.initialState.preset, "baby");
  assert.equal(migrated.initialState.highFrequencyAccelerationLimit, 0);
  assert.equal(migrated.initialState.stylusTracingLimit, 0);
  assert.equal(migrated.events[1].frameOffset, 24_000);
  assert.ok(Math.abs(migrated.events[1].positionFrames - 96_000) < 1e-9);
});

test("schema v2 preserves equal-frame ordering and normalizes controls", () => {
  const normalized = normalizeScratchPerformance({
    id: "v2",
    schemaVersion: 2,
    sourceSampleRate: 48_000,
    outputSampleRate: 48_000,
    initialState: {
      preset: "FLARE",
      clicks: 99,
      highFrequencyAccelerationLimit: 2,
      stylusTracingLimit: -1,
    },
    events: [
      { type: "scratch-clicks", frameOffset: 100, clicks: 0 },
      { type: "scratch-preset", frameOffset: 100, preset: "CRAB" },
      { type: "manual-crossfader", frameOffset: 20, value: 2 },
    ],
  });

  assert.deepEqual(normalized.events.map(event => event.type), [
    "manual-crossfader",
    "scratch-clicks",
    "scratch-preset",
  ]);
  assert.equal(normalized.events[0].value, 1);
  assert.equal(normalized.events[1].clicks, 1);
  assert.equal(normalized.events[2].preset, "crab");
  assert.equal(normalized.initialState.clicks, 8);
  assert.equal(normalized.initialState.highFrequencyAccelerationLimit, 1);
  assert.equal(normalized.initialState.stylusTracingLimit, 0);
  assert.equal(SCRATCH_GATE_ALGORITHM_VERSION, 2);
  assert.equal(normalized.engine.gateAlgorithmVersion, 2);
});

test("preset and click normalization is bounded", () => {
  assert.equal(normalizeScratchPreset("Orbit"), "orbit");
  assert.equal(normalizeScratchPreset("unknown"), "baby");
  assert.equal(SCRATCH_PRESET_DEFAULT_CLICKS.crab, 4);
  assert.equal(normalizeScratchClicks(-4), 1);
  assert.equal(normalizeScratchClicks(12), 8);
});

test("invalid schemas and event shapes are rejected", () => {
  assert.throws(() => normalizeScratchPerformance({ schemaVersion: 7, events: [] }), /Unsupported/);
  assert.throws(() => normalizeScratchPerformance({ schemaVersion: 2, sourceSampleRate: 48_000, outputSampleRate: 48_000, events: [{ type: "eval", frameOffset: 0 }] }), /Invalid/);
});

test("rejects replay event density that could monopolize one audio quantum", () => {
  const events = Array.from({ length: 129 }, (_, index) => ({
    type: "manual-crossfader",
    frameOffset: index % 2,
    value: index % 2,
  }));
  assert.throws(
    () => normalizeScratchPerformance({
      schemaVersion: 2,
      sourceSampleRate: 48_000,
      outputSampleRate: 48_000,
      events,
    }),
    /exceeds 128 events in a 128-frame render window/,
  );
});

test("missing scratch motion rates normalize to rest, never maximum reverse", () => {
  const normalized = normalizeScratchPerformance({
    schemaVersion: 2,
    sourceSampleRate: 48_000,
    outputSampleRate: 48_000,
    events: [{ type: "scratch-motion", frameOffset: 0, positionFrames: 120 }],
  });
  assert.equal(normalized.events[0].rate, 0);
});
