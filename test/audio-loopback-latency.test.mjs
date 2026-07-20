import assert from "node:assert/strict";
import test from "node:test";

import {
  createAcousticLoopbackProbe,
  detectAcousticLoopbackProbe,
  summarizeAcousticLoopbackDetections,
} from "../web/audio-loopback-latency.js";

function addSignal(destination, offset, source, gain) {
  for (let index = 0; index < source.length; index += 1) {
    destination[offset + index] += source[index] * gain;
  }
}

function deterministicNoise(length, amplitude = 0.002) {
  const output = new Float32Array(length);
  let state = 0x6d2b79f5;
  for (let index = 0; index < length; index += 1) {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    output[index] = ((state >>> 0) / 0xffff_ffff * 2 - 1) * amplitude;
  }
  return output;
}

test("creates a bounded deterministic acoustic probe", () => {
  const first = createAcousticLoopbackProbe(48_000);
  const second = createAcousticLoopbackProbe(48_000);
  assert.equal(first.length, 1_536);
  assert.deepEqual(first, second);
  assert.ok(Math.max(...first) <= 1);
  assert.ok(Math.min(...first) >= -1);
  assert.ok(Math.abs(first[0]) < 1e-12);
  assert.ok(Math.abs(first.at(-1)) < 1e-12);
});

test("detects output-to-input delay in noise and a later reflection", () => {
  const sampleRate = 48_000;
  const probe = createAcousticLoopbackProbe(sampleRate);
  const captureStartFrame = 10_000;
  const expectedOutputFrame = 20_000;
  const directLatencyFrames = 1_728;
  const expectedOffset = expectedOutputFrame - captureStartFrame;
  const captured = deterministicNoise(45_000);
  addSignal(captured, expectedOffset + directLatencyFrames, probe, 0.42);
  addSignal(captured, expectedOffset + directLatencyFrames + 240, probe, 0.14);

  const detection = detectAcousticLoopbackProbe({
    captured,
    captureStartFrame,
    probe,
    expectedOutputFrame,
    sampleRate,
  });
  assert.ok(detection);
  assert.ok(Math.abs(detection.latencyFrames - directLatencyFrames) <= 1);
  assert.ok(Math.abs(detection.latencyMs - 36) < 0.03);
  assert.ok(detection.correlation > 0.9);
});

test("summarizes repeated correlated probes and ignores a weak detection", () => {
  const summary = summarizeAcousticLoopbackDetections([
    { latencyMs: 20, correlation: 0.9 },
    { latencyMs: 21, correlation: 0.8 },
    { latencyMs: 19, correlation: 0.85 },
    { latencyMs: 20.5, correlation: 0.88 },
    { latencyMs: 200, correlation: 0.05 },
  ]);
  assert.equal(summary.samples, 4);
  assert.equal(summary.medianMs, 20.25);
  assert.equal(summary.minimumMs, 19);
  assert.equal(summary.maximumMs, 21);
  assert.equal(summary.jitterMs, 2);
  assert.equal(summary.minimumCorrelation, 0.8);
});

test("rejects a capture without three credible probes", () => {
  assert.throws(() => summarizeAcousticLoopbackDetections([
    { latencyMs: 20, correlation: 0.9 },
    { latencyMs: 21, correlation: 0.1 },
  ]), /Only 1 acoustic probes/);
});
