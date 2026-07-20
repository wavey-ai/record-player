import assert from "node:assert/strict";
import test from "node:test";

import { createVinylPlayerCanvas } from "../web/player-canvas.js";
import { buildCanvasGeometry, minuteToDegrees } from "../web/player-canvas-geometry.js";

function noopContext({ rotations = null } = {}) {
  const values = {
    globalAlpha: 1,
    measureText: text => ({ width: String(text).length * 6 }),
    createRadialGradient: () => ({ addColorStop() {} }),
    rotate: value => rotations?.push(value),
  };
  return new Proxy(values, {
    get(target, key) {
      if (key in target) return target[key];
      return () => {};
    },
    set(target, key, value) {
      target[key] = value;
      return true;
    },
  });
}

class FakeCanvas {
  constructor(ownerDocument, context = noopContext()) {
    this.ownerDocument = ownerDocument;
    this.width = 800;
    this.height = 840;
    this.listeners = new Map();
    this.capturedPointers = new Set();
    this.context = context;
  }

  getContext() {
    return this.context;
  }

  getBoundingClientRect() {
    return { left: 0, top: 0, width: 800, height: 840 };
  }

  addEventListener(type, listener) {
    this.listeners.set(type, listener);
  }

  removeEventListener(type, listener) {
    if (this.listeners.get(type) === listener) this.listeners.delete(type);
  }

  setPointerCapture(pointerId) {
    this.capturedPointers.add(pointerId);
  }

  emit(type, event) {
    this.listeners.get(type)?.(event);
  }
}

function pointer(pointerId, x, y, timeStamp, extra = {}) {
  return {
    pointerId,
    clientX: x,
    clientY: y,
    timeStamp,
    pressure: 0.5,
    pointerType: "touch",
    ...extra,
  };
}

test("record and crossfader pointers retain independent canvas gesture ownership", t => {
  const animationFrames = new Map();
  let nextAnimationFrame = 1;
  const classList = { toggle() {}, remove() {} };
  const ownerDocument = {
    documentElement: { classList },
    addEventListener() {},
    removeEventListener() {},
    querySelector() { return null; },
  };
  const globals = {
    HTMLCanvasElement: globalThis.HTMLCanvasElement,
    document: globalThis.document,
    window: globalThis.window,
    requestAnimationFrame: globalThis.requestAnimationFrame,
    cancelAnimationFrame: globalThis.cancelAnimationFrame,
  };
  globalThis.HTMLCanvasElement = FakeCanvas;
  globalThis.document = ownerDocument;
  globalThis.window = { devicePixelRatio: 1 };
  globalThis.requestAnimationFrame = callback => {
    const id = nextAnimationFrame;
    nextAnimationFrame += 1;
    animationFrames.set(id, callback);
    return id;
  };
  globalThis.cancelAnimationFrame = id => animationFrames.delete(id);
  t.after(() => {
    for (const [name, value] of Object.entries(globals)) {
      if (value === undefined) delete globalThis[name];
      else globalThis[name] = value;
    }
  });

  const calls = { begin: [], update: [], end: [], crossfader: [] };
  const state = {
    ready: true,
    playing: false,
    motorRunning: false,
    scratching: false,
    loading: false,
    decoding: false,
    rotationDegrees: 0,
    rpm: 33.3333333333,
    nativeRpm: 33.3333333333,
    positionSeconds: 10,
    positionFrames: 480_000,
    durationSeconds: 60,
    sampleRate: 48_000,
    needleLifted: false,
    volume: 1,
    crossfader: 0.5,
    recordImageUrl: "",
  };
  const player = {
    getState: () => ({ ...state }),
    subscribe: () => () => {},
    beginScratch: value => calls.begin.push(value),
    updateScratch: value => calls.update.push(value),
    endScratch: value => calls.end.push(value),
    setCrossfader: value => calls.crossfader.push(value),
    setRpm() {},
    setVolume() {},
    setNeedleLifted() {},
    toggleTransport() {},
    seekRatio() {},
  };
  const canvas = new FakeCanvas(ownerDocument);
  const mounted = createVinylPlayerCanvas(player, canvas, {
    components: {
      syncRings: false,
      stylus: false,
      needlePoint: false,
      tonearmGuide: false,
      startStop: false,
      needle: false,
      loadRecord: false,
      rpm: false,
      volume: false,
    },
  });
  const firstFrame = animationFrames.entries().next().value;
  animationFrames.delete(firstFrame[0]);
  firstFrame[1](0);

  const geometry = buildCanvasGeometry(800, 840, { recordFill: false });
  const recordRadius = geometry.recordRadius * 0.7;
  const recordPoint = angle => ({
    x: geometry.cx + Math.cos(angle) * recordRadius,
    y: geometry.cy + Math.sin(angle) * recordRadius,
  });
  const recordStart = recordPoint(0);
  canvas.emit("pointerdown", pointer(1, recordStart.x, recordStart.y, 0));
  assert.equal(calls.begin.length, 1);

  const coalesced = [0.08, 0.16].map((angle, index) => {
    const point = recordPoint(angle);
    return pointer(1, point.x, point.y, (index + 1) * 10);
  });
  const recordMove = coalesced.at(-1);
  canvas.emit("pointermove", {
    ...recordMove,
    getCoalescedEvents: () => coalesced,
  });
  assert.equal(calls.update.length, 2);

  const faderAngle = minuteToDegrees(26) * Math.PI / 180;
  const faderRadius = (geometry.controlBandInner + geometry.controlBandOuter) / 2;
  const fader = {
    x: geometry.cx + Math.cos(faderAngle) * faderRadius,
    y: geometry.cy + Math.sin(faderAngle) * faderRadius,
  };
  canvas.emit("pointerdown", pointer(2, fader.x, fader.y, 25));
  assert.equal(calls.crossfader.length, 1);
  canvas.emit("pointermove", pointer(2, fader.x + 8, fader.y - 4, 30));
  canvas.emit("pointerup", pointer(2, fader.x + 8, fader.y - 4, 35));
  assert.ok(calls.crossfader.length >= 2);
  assert.equal(calls.end.length, 0, "releasing the fader must not release the record");

  const finalPoint = recordPoint(0.24);
  canvas.emit("pointermove", pointer(1, finalPoint.x, finalPoint.y, 40));
  assert.equal(calls.update.length, 3, "record pointer must remain active after fader release");
  canvas.emit("pointerup", pointer(1, finalPoint.x, finalPoint.y, 45));
  assert.equal(calls.end.length, 1);
  assert.equal(calls.end[0].cancelled, false);
  mounted.destroy();
});

test("canvas rotation follows audio-owned phase and effective rate", t => {
  const animationFrames = new Map();
  let nextAnimationFrame = 1;
  const classList = { toggle() {}, remove() {} };
  const ownerDocument = {
    documentElement: { classList },
    addEventListener() {},
    removeEventListener() {},
    querySelector() { return null; },
  };
  const globals = {
    HTMLCanvasElement: globalThis.HTMLCanvasElement,
    document: globalThis.document,
    window: globalThis.window,
    requestAnimationFrame: globalThis.requestAnimationFrame,
    cancelAnimationFrame: globalThis.cancelAnimationFrame,
  };
  globalThis.HTMLCanvasElement = FakeCanvas;
  globalThis.document = ownerDocument;
  globalThis.window = { devicePixelRatio: 1 };
  globalThis.requestAnimationFrame = callback => {
    const id = nextAnimationFrame;
    nextAnimationFrame += 1;
    animationFrames.set(id, callback);
    return id;
  };
  globalThis.cancelAnimationFrame = id => animationFrames.delete(id);
  t.after(() => {
    for (const [name, value] of Object.entries(globals)) {
      if (value === undefined) delete globalThis[name];
      else globalThis[name] = value;
    }
  });

  const state = {
    ready: true,
    playing: true,
    motorRunning: true,
    scratching: false,
    scratchReplayActive: false,
    loading: false,
    decoding: false,
    rotationDegrees: 12,
    effectiveRate: 0,
    rpm: 33.3333333333,
    nativeRpm: 33.3333333333,
    positionSeconds: 10,
    positionFrames: 480_000,
    durationSeconds: 60,
    sampleRate: 48_000,
    needleLifted: false,
    volume: 1,
    crossfader: 0.5,
    recordImageUrl: "",
  };
  let publishSnapshot = null;
  const player = {
    getState: () => ({ ...state }),
    subscribe: listener => {
      publishSnapshot = listener;
      return () => {};
    },
    setCrossfader() {},
    setRpm() {},
    setVolume() {},
    setNeedleLifted() {},
    toggleTransport() {},
    seekRatio() {},
  };
  const rotations = [];
  const canvas = new FakeCanvas(ownerDocument, noopContext({ rotations }));
  const mounted = createVinylPlayerCanvas(player, canvas, {
    components: {
      syncRings: false,
      spindle: false,
      stylus: false,
      needlePoint: false,
      tonearmGuide: false,
      startStop: false,
      needle: false,
      loadRecord: false,
      rpm: false,
      volume: false,
      crossfader: false,
      seek: false,
      labels: false,
    },
  });

  function renderNext(timestamp) {
    const next = animationFrames.entries().next().value;
    assert(next, "The canvas did not schedule a render");
    animationFrames.delete(next[0]);
    rotations.length = 0;
    next[1](timestamp);
    assert(rotations.length > 0, "The record did not expose its drawn rotation");
    return rotations[0] * 180 / Math.PI;
  }

  renderNext(performance.now());
  state.rotationDegrees = 30;
  state.effectiveRate = 0.5;
  publishSnapshot({ ...state });
  const forwardStart = performance.now();
  const forwardDegrees = renderNext(forwardStart + 50);
  assert.ok(
    Math.abs(forwardDegrees - 35) < 1,
    `Half-speed audio motion drew ${forwardDegrees} degrees instead of about 35`,
  );

  state.rotationDegrees = -40;
  state.effectiveRate = -1;
  publishSnapshot({ ...state });
  const reverseStart = performance.now();
  const reverseDegrees = renderNext(reverseStart + 50);
  assert.ok(
    Math.abs(reverseDegrees - -50) < 1,
    `Reverse audio motion drew ${reverseDegrees} degrees instead of about -50`,
  );

  state.rotationDegrees = 55;
  state.effectiveRate = 0;
  state.motorRunning = false;
  state.playing = false;
  publishSnapshot({ ...state });
  const stoppedDegrees = renderNext(performance.now() + 50);
  assert.ok(
    Math.abs(stoppedDegrees - 55) < 0.01,
    `A stopped audio platter drifted to ${stoppedDegrees} degrees`,
  );
  mounted.destroy();
});
