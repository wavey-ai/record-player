import assert from "node:assert/strict";
import test from "node:test";

import {
  createPointerInputProfiler,
  pointerInputProfileRequirements,
  validatePointerInputProfile,
} from "../web/pointer-input-profile.js";

function pointer(pointerId, pointerType, timeStamp, pressure, extra = {}) {
  return { pointerId, pointerType, timeStamp, pressure, width: 20, height: 18, ...extra };
}

test("profiles coalesced two-touch input without treating pressure as grip", () => {
  let now = 0;
  const profiler = createPointerInputProfiler({ now: () => now });
  profiler.observe("pointerdown", pointer(1, "touch", 0, 0.5));
  profiler.observe("pointerdown", pointer(2, "touch", 1, 0.5));
  for (let index = 1; index <= 8; index += 1) {
    const time = index * 8;
    profiler.observe("pointermove", pointer(1, "touch", time, 0.5, {
      getCoalescedEvents: () => [
        pointer(1, "touch", time - 4, 0.5),
        pointer(1, "touch", time, 0.5),
      ],
    }));
  }
  profiler.observe("pointerup", pointer(1, "touch", 70, 0));
  profiler.observe("lostpointercapture", pointer(1, "touch", 70, 0));
  profiler.observe("pointerup", pointer(2, "touch", 71, 0));
  now = 80;

  const profile = profiler.snapshot();
  assert.equal(profile.maximumConcurrentPointers, 2);
  assert.equal(profile.contactSamples, 18);
  assert.equal(profile.types.touch.gripPolicy, "full-contact");
  assert.equal(profile.types.touch.pressure.variable, false);
  assert.equal(profile.types.touch.coalescedSamples, 16);
  assert.equal(profile.types.touch.medianSampleRateHz, 250);
  assert.equal(profile.lostPointerCaptures, 0);
  assert.equal(profile.requirements.pass, true);
  assert.deepEqual(validatePointerInputProfile(profile), profile);
});

test("profiles variable pen pressure and rejects incomplete or cancelled probes", () => {
  let now = 0;
  const profiler = createPointerInputProfiler({ now: () => now });
  profiler.observe("pointerdown", pointer(9, "pen", 0, 0.1));
  for (let index = 1; index <= 15; index += 1) {
    profiler.observe("pointermove", pointer(9, "pen", index * 10, index / 20));
  }
  profiler.observe("pointercancel", pointer(9, "pen", 170, 0));
  now = 200;

  const profile = profiler.snapshot();
  assert.equal(profile.types.pen.gripPolicy, "pointer-pressure");
  assert.equal(profile.types.pen.pressure.variable, true);
  assert.equal(profile.pointerCancels, 1);
  assert.equal(profile.requirements.pass, false);
  assert.match(profile.requirements.reasons.join(" "), /cancellation/);

  const incomplete = structuredClone(profile);
  incomplete.pointerCancels = 0;
  incomplete.contactSamples = 2;
  assert.equal(pointerInputProfileRequirements(incomplete).pass, false);
});

test("rejects self-reported grip policies and malformed cadence evidence", () => {
  const profiler = createPointerInputProfiler({ now: () => 100 });
  profiler.observe("pointerdown", pointer(1, "mouse", 0, 0.5));
  for (let index = 1; index <= 15; index += 1) {
    profiler.observe("pointermove", pointer(1, "mouse", index * 8, 0.5));
  }
  const profile = profiler.snapshot();

  const falsePressureGrip = structuredClone(profile);
  falsePressureGrip.types.mouse.gripPolicy = "pointer-pressure";
  assert.throws(() => validatePointerInputProfile(falsePressureGrip), /full-contact/);

  const reversedCadence = structuredClone(profile);
  reversedCadence.types.mouse.sampleIntervalMs.p50 = 20;
  assert.throws(() => validatePointerInputProfile(reversedCadence), /monotonic/);

  const inventedRate = structuredClone(profile);
  inventedRate.types.mouse.medianSampleRateHz = 1_000;
  assert.throws(() => validatePointerInputProfile(inventedRate), /measured interval/);
});
