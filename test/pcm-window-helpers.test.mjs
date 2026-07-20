import assert from "node:assert/strict";
import test from "node:test";

import {
  contiguousPcmEnd,
  copyPcmWindow,
  mergePcmWrittenRange,
  planPcmWindow,
  repairPcmSeam,
} from "../web/pcm-window-helpers.js";

test("tracks only contiguous progressive PCM availability", () => {
  const ranges = [];
  mergePcmWrittenRange(ranges, 100, 200);
  assert.equal(contiguousPcmEnd(ranges), 0);
  mergePcmWrittenRange(ranges, 0, 80);
  assert.equal(contiguousPcmEnd(ranges), 80);
  mergePcmWrittenRange(ranges, 70, 120);
  assert.deepEqual(ranges, [[0, 200]]);
  assert.equal(contiguousPcmEnd(ranges), 200);
});

test("plans bounded windows that never expose undecoded zero-fill", () => {
  assert.equal(planPcmWindow({ position: 800, totalFrames: 4000, availableEnd: 800, windowFrames: 1000 }), null);
  assert.deepEqual(
    planPcmWindow({ position: 799, totalFrames: 4000, availableEnd: 800, windowFrames: 1000 }),
    { start: 0, end: 800, length: 800, position: 799 },
  );
  assert.deepEqual(
    planPcmWindow({ position: 1800, totalFrames: 4000, availableEnd: 2000, windowFrames: 1000 }),
    { start: 1000, end: 2000, length: 1000, position: 1800 },
  );
});

function discontinuousChannel(length = 160, boundary = 80) {
  const channel = new Int16Array(length);
  for (let index = 0; index < boundary; index += 1) channel[index] = -12000 + index * 35;
  for (let index = boundary; index < length; index += 1) channel[index] = 14500 + index * 17;
  return channel;
}

function maxAdjacentJump(samples) {
  let max = 0;
  for (let index = 1; index < samples.length; index += 1) {
    max = Math.max(max, Math.abs(samples[index] - samples[index - 1]));
  }
  return max;
}

test("authoritative seam repair smooths forward traversal before window copy", () => {
  const boundary = 80;
  const channel = discontinuousChannel(160, boundary);
  const originalJump = Math.abs(channel[boundary] - channel[boundary - 1]) / 32768;
  const repair = repairPcmSeam([channel], boundary);
  assert.equal(repair.channels, 1);
  assert.equal(repair.samples, 24);
  const [window] = copyPcmWindow([channel], { start: boundary - 16, length: 32 });
  assert.ok(maxAdjacentJump(window) < originalJump * 0.2);
});

test("reverse traversal reads the same repaired seam without a directional discontinuity", () => {
  const boundary = 80;
  const channel = discontinuousChannel(160, boundary);
  const originalJump = Math.abs(channel[boundary] - channel[boundary - 1]) / 32768;
  repairPcmSeam([channel], boundary);
  const [window] = copyPcmWindow([channel], { start: boundary - 16, length: 32 });
  const reverse = Float32Array.from(window).reverse();
  assert.ok(maxAdjacentJump(reverse) < originalJump * 0.2);
  assert.deepEqual(Array.from(reverse).reverse(), Array.from(window));
});
