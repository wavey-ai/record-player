import assert from "node:assert/strict";
import test from "node:test";

import { projectPointerOutputFrame } from "../web/player-input-timing.js";

test("projects a DOM pointer timestamp onto the output audio clock", () => {
  assert.equal(projectPointerOutputFrame({
    sampleRate: 48_000,
    currentAudioTime: 10,
    inputTimeMs: 988,
    nowMs: 1_000,
  }), 480_000 - 576);
});

test("normalizes epoch pointer timestamps before audio-frame projection", () => {
  assert.equal(projectPointerOutputFrame({
    sampleRate: 48_000,
    currentAudioTime: 2,
    inputTimeMs: 1_700_000_000_975,
    nowMs: 1_000,
    timeOriginMs: 1_700_000_000_000,
  }), 96_000 - 1_200);
});

test("bounds explicit or stale timestamps to the live audio history", () => {
  assert.equal(projectPointerOutputFrame({
    sampleRate: 48_000,
    currentAudioTime: 1,
    requestedOutputFrame: 99_999,
  }), 48_000);
  assert.equal(projectPointerOutputFrame({
    sampleRate: 48_000,
    currentAudioTime: 2,
    inputTimeMs: -50_000,
    nowMs: 1_000,
  }), 48_000);
});
