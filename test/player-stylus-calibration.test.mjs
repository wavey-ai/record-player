import assert from "node:assert/strict";
import test from "node:test";

import {
  createProgrammeStylusCalibration,
  programmeHasStylusGaps,
} from "../web/player-stylus-calibration.js";

const programmeWithGap = Object.freeze({
  totalSamples: 1_000,
  gaps: [Object.freeze({
    startSample: 400,
    endSample: 500,
    radialStartNormalized: 0.3,
    radialEndNormalized: 0.5,
  })],
});

test("no-gap programmes retain the linear path without loading another WASM instance", async () => {
  let loads = 0;
  const calibration = await createProgrammeStylusCalibration(
    { totalSamples: 1_000, gaps: [] },
    { loadModule: async () => { loads += 1; } },
  );
  assert.equal(calibration, null);
  assert.equal(loads, 0);
  assert.equal(programmeHasStylusGaps(programmeWithGap), true);
});

test("programme-gap ratios cross the Rust calibration boundary in both directions", async () => {
  const calls = { programme: null, samples: [], grooves: [], frees: 0 };
  const rustCalibration = {
    hasGaps: true,
    totalSamples: 1_000,
    sampleToGroove(sample) {
      calls.samples.push(sample);
      return sample / 2_000;
    },
    grooveToSample(groove) {
      calls.grooves.push(groove);
      return groove * 2_000;
    },
    free() {
      calls.frees += 1;
    },
  };
  const calibration = await createProgrammeStylusCalibration(programmeWithGap, {
    loadModule: async () => ({
      StylusCalibration: {
        fromProgrammeMap(programme) {
          calls.programme = programme;
          return rustCalibration;
        },
      },
    }),
  });

  assert.equal(calls.programme, programmeWithGap);
  assert.equal(calibration.sampleRatioToGroove(0.8), 0.4);
  assert.deepEqual(calls.samples, [800]);
  assert.equal(calibration.grooveToSampleRatio(0.4), 0.8);
  assert.deepEqual(calls.grooves, [0.4]);
  calibration.destroy();
  calibration.destroy();
  assert.equal(calls.frees, 1);
  assert.equal(calibration.sampleRatioToGroove(0.6), 0.6);
});

test("a gap map requires the Rust programme-map constructor", async () => {
  await assert.rejects(
    createProgrammeStylusCalibration(programmeWithGap, {
      loadModule: async () => ({ StylusCalibration: class {} }),
    }),
    /fromProgrammeMap/,
  );
});
