import assert from "node:assert/strict";
import test from "node:test";

import {
  SCRATCH_GESTURE_DEFAULTS,
  createScratchGestureTracker,
  unwrapScratchAngle,
} from "../web/scratch-gesture.js";

const TAU = Math.PI * 2;

function close(actual, expected, tolerance = 1e-9, message = "") {
  assert.ok(
    Math.abs(actual - expected) <= tolerance,
    message || `${actual} is not within ${tolerance} of ${expected}`,
  );
}

function wrapAngle(angle) {
  return Math.atan2(Math.sin(angle), Math.cos(angle));
}

function beginTracker(options = {}, sample = {}) {
  const tracker = createScratchGestureTracker({
    sampleRate: 1000,
    secondsPerTurn: 2,
    maxPositionFrames: 1_000_000,
    ...options,
  });
  const start = tracker.begin({
    pointerId: 7,
    angleRadians: 0,
    timeMs: 0,
    positionFrames: 100_000,
    rotationDegrees: 0,
    radius: 100,
    ...sample,
  });
  return { tracker, start };
}

function steadyTrace(sampleHz, durationSeconds = 1) {
  const { tracker } = beginTracker({ accelerationThreshold: 1_000_000 });
  const count = Math.round(sampleHz * durationSeconds);
  let result;
  for (let index = 1; index <= count; index += 1) {
    const timeSeconds = index / sampleHz;
    result = tracker.update({
      pointerId: 7,
      angleRadians: wrapAngle(TAU * timeSeconds / 2),
      timeMs: timeSeconds * 1000,
      radius: 100,
    });
  }
  return result;
}

test("unwraps the ±π boundary incrementally and retains multiple turns", () => {
  close(unwrapScratchAngle((-170 - 170) * Math.PI / 180), 20 * Math.PI / 180);
  close(unwrapScratchAngle((170 - -170) * Math.PI / 180), -20 * Math.PI / 180);

  const { tracker } = beginTracker();
  const angles = [];
  for (let turn = 0; turn < 3; turn += 1) {
    angles.push(Math.PI / 2, Math.PI, -Math.PI / 2, 0);
  }
  let result;
  angles.forEach((angleRadians, index) => {
    result = tracker.update({
      pointerId: 7,
      angleRadians,
      timeMs: (index + 1) * 250,
      radius: 100,
    });
  });

  close(result.totalAngleRadians, TAU * 3);
  close(result.rotationDegrees, 1080);
  close(result.positionFrames, 106_000);
});

test("sequential coalesced samples and 30/60/120/240 Hz traces agree", () => {
  const traces = [30, 60, 120, 240].map(rate => steadyTrace(rate));
  for (const result of traces) {
    close(result.positionFrames, 101_000, 1e-6);
    close(result.rawRate, 1, 1e-9);
    close(result.filteredRate, 1, 1e-9);
    assert.equal(result.direction, 1);
  }

  const samples = Array.from({ length: 60 }, (_, index) => {
    const timeSeconds = (index + 1) / 120;
    return {
      pointerId: 7,
      angleRadians: wrapAngle(TAU * timeSeconds / 2),
      timeMs: timeSeconds * 1000,
      radius: 100,
    };
  });
  const one = beginTracker().tracker;
  const many = beginTracker().tracker;
  let sequential;
  for (const sample of samples) sequential = one.update(sample);
  const coalesced = many.updateMany(samples).at(-1);
  assert.deepEqual(coalesced, sequential);
});

test("uses a four millisecond differentiation floor for duplicate timestamps", () => {
  const { tracker } = beginTracker();
  const deltaForOneXAtFourMs = TAU * 0.004 / 2;
  const result = tracker.update({
    pointerId: 7,
    angleRadians: deltaForOneXAtFourMs,
    timeMs: 0,
    radius: 100,
  });
  close(result.elapsedSeconds, 0.004);
  close(result.rawRate, 1);
  assert.ok(Number.isFinite(result.filteredRate));
});

test("uses 35ms steady smoothing and a faster reversal path", () => {
  const { tracker } = beginTracker();
  const forward = tracker.update({
    pointerId: 7,
    angleRadians: TAU * 0.035 / 2,
    timeMs: 35,
    radius: 100,
  });
  close(forward.filteredRate, 1 - Math.exp(-1), 1e-12);
  assert.equal(forward.direction, 1);

  const reversedAngle = TAU * 0.035 / 2 - TAU * 0.008 / 2;
  const reverse = tracker.update({
    pointerId: 7,
    angleRadians: reversedAngle,
    timeMs: 43,
    radius: 100,
  });
  assert.ok(reverse.filteredRate < -0.35, "reversal should not retain the 35ms lag");
  assert.equal(reverse.direction, -1);
  assert.equal(reverse.reversal, true);
  assert.ok(reverse.impulse > 0);
});

test("Schmitt direction state ignores sub-threshold jitter", () => {
  const { tracker } = beginTracker({
    steadyFilterSeconds: 0.001,
    reversalFilterSeconds: 0.001,
    accelerationThreshold: 1_000_000,
  });
  let angle = 0;
  let timeMs = 0;
  const moveAtRate = rate => {
    timeMs += 10;
    angle += TAU * rate * 0.01 / 2;
    return tracker.update({ pointerId: 7, angleRadians: angle, timeMs, radius: 100 });
  };

  assert.equal(moveAtRate(0.02).direction, 0);
  assert.equal(moveAtRate(-0.02).direction, 0);
  assert.equal(moveAtRate(0.1).direction, 1);
  assert.equal(moveAtRate(0.01).direction, 0);
  assert.equal(moveAtRate(-0.02).direction, 0);
  assert.equal(moveAtRate(-0.1).direction, -1);
});

test("clamps source position at both record boundaries", () => {
  const { tracker } = beginTracker(
    { sampleRate: 100, secondsPerTurn: 1, maxPositionFrames: 100 },
    { positionFrames: 90 },
  );
  const high = tracker.update({
    pointerId: 7,
    angleRadians: Math.PI / 2,
    timeMs: 250,
    radius: 100,
  });
  assert.equal(high.positionFrames, 100);
  assert.equal(high.positionClamped, true);
  let low;
  [0, -Math.PI / 2, -Math.PI].forEach((angleRadians, index) => {
    low = tracker.update({
      pointerId: 7,
      angleRadians,
      timeMs: 500 + index * 250,
      radius: 100,
    });
  });
  assert.equal(low.positionFrames, 25);
  const zero = tracker.update({
    pointerId: 7,
    angleRadians: Math.PI / 2,
    timeMs: 1250,
    radius: 100,
  });
  assert.equal(zero.positionFrames, 0);
  const bottom = tracker.update({
    pointerId: 7,
    angleRadians: 0,
    timeMs: 1500,
    radius: 100,
  });
  assert.equal(bottom.positionFrames, 0);
  assert.equal(bottom.positionClamped, true);
});

test("lifted-needle movement is visual-only and retains pressure/grip telemetry", () => {
  const { tracker, start } = beginTracker({}, {
    positionFrames: 500,
    rotationDegrees: 10,
    needleLifted: true,
    pressure: 0.7,
    pointerType: "pen",
  });
  assert.equal(start.grip, 0.7);
  assert.equal(start.pressure, 0.7);

  const result = tracker.update({
    pointerId: 7,
    angleRadians: Math.PI / 2,
    timeMs: 100,
    radius: 100,
    pressure: 2,
    grip: 0.4,
    pointerType: "pen",
  });
  assert.equal(result.positionFrames, 500);
  assert.equal(result.rotationDegrees, 100);
  assert.equal(result.rate, 0);
  assert.equal(result.rawRate, 0);
  assert.equal(result.impulse, 0);
  assert.equal(result.visualOnly, true);
  assert.equal(result.pressure, 1);
  assert.equal(result.grip, 0.4);
  assert.equal(result.handContact, true);

  const end = tracker.cancel({ pointerId: 7, pressure: 0, pointerType: "pen" });
  assert.equal(end.cancelled, true);
  assert.equal(end.handContact, false);
  assert.equal(end.grip, 0);
});

test("only pen pressure changes implicit grip", () => {
  const mouse = beginTracker({}, { pressure: 0.5, pointerType: "mouse" }).start;
  const touch = beginTracker({}, { pressure: 0.35, pointerType: "touch" }).start;
  const pen = beginTracker({}, { pressure: 0.35, pointerType: "pen" }).start;
  const explicit = beginTracker({}, {
    pressure: 0.9,
    grip: 0.2,
    pointerType: "touch",
  }).start;

  assert.equal(mouse.grip, 1);
  assert.equal(touch.grip, 1);
  assert.equal(pen.grip, 0.35);
  assert.equal(explicit.grip, 0.2);
});

test("near-spindle samples cannot create angular or rate spikes", () => {
  const { tracker } = beginTracker({ minimumRadius: 10 });
  const ignored = tracker.update({
    pointerId: 7,
    angleRadians: Math.PI,
    timeMs: 10,
    radius: 5,
  });
  assert.equal(ignored.ignored, true);
  assert.equal(ignored.ignoreReason, "near-spindle");
  assert.equal(ignored.positionFrames, 100_000);
  assert.equal(ignored.rotationDegrees, 0);
  assert.equal(ignored.impulse, 0);

  const reacquired = tracker.update({
    pointerId: 7,
    angleRadians: -Math.PI / 2,
    timeMs: 20,
    radius: 100,
  });
  assert.equal(reacquired.ignoreReason, "angle-reacquired");
  assert.equal(reacquired.rotationDegrees, 0);

  const moved = tracker.update({
    pointerId: 7,
    angleRadians: -Math.PI / 2 + 0.01,
    timeMs: 30,
    radius: 100,
  });
  close(moved.deltaAngleRadians, 0.01);
  assert.ok(Math.abs(moved.rawRate) < 1);
});

test("motion impulses occur only on acceleration or a real reversal", () => {
  const { tracker, start } = beginTracker();
  assert.equal(start.impulse, SCRATCH_GESTURE_DEFAULTS.grabImpulse);
  const results = [];
  let angle = 0;
  for (let index = 1; index <= 30; index += 1) {
    angle += TAU * 0.01 / 2;
    results.push(tracker.update({
      pointerId: 7,
      angleRadians: wrapAngle(angle),
      timeMs: index * 10,
      radius: 100,
    }));
  }
  const steadyTail = results.slice(-8);
  assert.ok(steadyTail.every(result => result.impulse === 0));

  let reversal;
  for (let index = 31; index <= 36; index += 1) {
    angle -= TAU * 0.01 / 2;
    const result = tracker.update({
      pointerId: 7,
      angleRadians: wrapAngle(angle),
      timeMs: index * 10,
      radius: 100,
    });
    results.push(result);
    if (result.reversal) reversal = result;
  }
  assert.ok(reversal);
  assert.ok(reversal.impulse > 0);
  assert.ok(results.every(result => (
    result.impulse === 0
    || result.reversal
    || Math.abs(result.acceleration) >= SCRATCH_GESTURE_DEFAULTS.accelerationThreshold
  )));
});

test("rejects another pointer and reports explicit completion", () => {
  const { tracker } = beginTracker();
  assert.throws(() => tracker.update({
    pointerId: 8,
    angleRadians: 0,
    timeMs: 10,
  }), /pointer does not match/);
  const end = tracker.finish({ pointerId: 7 });
  assert.equal(end.phase, "end");
  assert.equal(end.cancelled, false);
  assert.equal(end.rate, 0);
  assert.equal(tracker.finish({ pointerId: 7 }), null);
});
