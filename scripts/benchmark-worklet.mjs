import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { access, readFile } from "node:fs/promises";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

const SAMPLE_RATE = 48_000;
const FRAME_COUNT = 128;
const CHANNEL_COUNT = 2;
const WINDOW_FRAMES = SAMPLE_RATE * 6;
const WARMUP_ITERATIONS = 1_000;
const MEASURED_ITERATIONS = 5_000;
const WINDOW_APPLY_WARMUP_ITERATIONS = 16;
const WINDOW_APPLY_MEASURED_ITERATIONS = 128;
const REGRESSION_BUDGET_FRACTION = 0.5;
const WINDOW_APPLY_P95_BUDGET_FRACTION = 0.25;
const WINDOW_APPLY_MAX_BUDGET_FRACTION = 0.5;
const QUANTUM_BUDGET_MS = FRAME_COUNT / SAMPLE_RATE * 1_000;
const WINDOW_CENTER = WINDOW_FRAMES / 2;

const workletUrl = new URL("../dist/player-worklet.js", import.meta.url);
const wasmUrl = new URL("../dist/record-player/record_player_bg.wasm", import.meta.url);

async function requireBuildArtifact(url) {
  try {
    await access(fileURLToPath(url));
  } catch (error) {
    throw new Error(
      `Missing ${fileURLToPath(url)}. Run \`npm run build\` before \`npm run bench:worklet\`.`,
      { cause: error },
    );
  }
}

await Promise.all([requireBuildArtifact(workletUrl), requireBuildArtifact(wasmUrl)]);

globalThis.sampleRate = SAMPLE_RATE;
globalThis.currentFrame = 0;
globalThis.currentTime = 0;

let PlayerProcessor = null;

class SilentMessagePort {
  constructor() {
    this.messages = [];
  }

  postMessage(message) {
    this.messages.push(message);
  }
}

globalThis.AudioWorkletProcessor = class MockAudioWorkletProcessor {
  constructor() {
    this.port = new SilentMessagePort();
  }
};
globalThis.registerProcessor = (name, Processor) => {
  assert.equal(name, "bitneedle-player");
  PlayerProcessor = Processor;
};

await import(workletUrl.href);
assert.equal(typeof PlayerProcessor, "function", "player worklet did not register its processor");

const wasmModule = await WebAssembly.compile(await readFile(wasmUrl));
const left = new Float32Array(WINDOW_FRAMES);
const right = new Float32Array(WINDOW_FRAMES);
for (let frame = 0; frame < WINDOW_FRAMES; frame += 1) {
  left[frame] = 0.38 * Math.sin(frame * 0.071) + 0.12 * Math.sin(frame * 0.319);
  right[frame] = 0.36 * Math.cos(frame * 0.067) + 0.11 * Math.sin(frame * 0.293);
}

function createProcessor() {
  const processor = new PlayerProcessor({
    processorOptions: { wasmModule, loggingEnabled: false },
  });
  processor.handleMessage({
    type: "window-transport-init",
    totalFrames: WINDOW_FRAMES,
    sampleRate: SAMPLE_RATE,
    channelCount: CHANNEL_COUNT,
    windowFrames: WINDOW_FRAMES,
    bankBuffers: [],
  });
  processor.handleMessage({
    type: "window-ready",
    generation: 1,
    requestId: 1,
    start: 0,
    totalFrames: WINDOW_FRAMES,
    length: WINDOW_FRAMES,
    availableEnd: WINDOW_FRAMES,
    sampleRate: SAMPLE_RATE,
    bankIndex: -1,
    channelBuffers: [left.buffer, right.buffer],
    position: WINDOW_CENTER,
    resetPosition: true,
  });
  processor.handleMessage({ type: "stream-complete" });
  processor.handleMessage({ type: "needle", lifted: false });
  processor.handleMessage({ type: "transport", running: true });
  processor.handleMessage({
    type: "play",
    position: WINDOW_CENTER,
    rate: 1,
    handoff: false,
  });
  return processor;
}

function createOutput() {
  return [[
    new Float32Array(FRAME_COUNT),
    new Float32Array(FRAME_COUNT),
  ]];
}

function renderQuantum(processor, output) {
  processor.process([], output);
  globalThis.currentFrame += FRAME_COUNT;
  globalThis.currentTime = currentFrame / SAMPLE_RATE;
}

function summarize(samples) {
  const sorted = [...samples].sort((a, b) => a - b);
  const totalMs = samples.reduce((sum, sample) => sum + sample, 0);
  const percentile = fraction => sorted[Math.ceil(sorted.length * fraction) - 1];
  const meanMs = totalMs / samples.length;
  const p95Ms = percentile(0.95);
  const maxMs = sorted.at(-1);
  return {
    meanMs,
    p95Ms,
    maxMs,
    meanBudgetPercent: meanMs / QUANTUM_BUDGET_MS * 100,
    p95BudgetPercent: p95Ms / QUANTUM_BUDGET_MS * 100,
    maxBudgetPercent: maxMs / QUANTUM_BUDGET_MS * 100,
  };
}

function createFreshWindowMessage(generation) {
  return {
    type: "window-ready",
    generation,
    requestId: generation,
    start: 0,
    totalFrames: WINDOW_FRAMES,
    length: WINDOW_FRAMES,
    availableEnd: WINDOW_FRAMES,
    sampleRate: SAMPLE_RATE,
    bankIndex: -1,
    channelBuffers: [left.slice().buffer, right.slice().buffer],
    position: WINDOW_CENTER,
    resetPosition: true,
  };
}

function benchmarkFreshWindowApplication() {
  const processor = new PlayerProcessor({
    processorOptions: { wasmModule, loggingEnabled: false },
  });
  processor.handleMessage({
    type: "window-transport-init",
    totalFrames: WINDOW_FRAMES,
    sampleRate: SAMPLE_RATE,
    channelCount: CHANNEL_COUNT,
    windowFrames: WINDOW_FRAMES,
    bankBuffers: [],
  });

  let generation = 0;
  for (let iteration = 0; iteration < WINDOW_APPLY_WARMUP_ITERATIONS; iteration += 1) {
    const message = createFreshWindowMessage(++generation);
    processor.handleMessage(message);
  }

  const samples = new Array(WINDOW_APPLY_MEASURED_ITERATIONS);
  for (let iteration = 0; iteration < WINDOW_APPLY_MEASURED_ITERATIONS; iteration += 1) {
    // Constructing the worker's newly transferred buffers is deliberately
    // outside the timer; this measures the AudioWorklet's apply/copy path.
    const message = createFreshWindowMessage(++generation);
    const start = performance.now();
    processor.handleMessage(message);
    samples[iteration] = performance.now() - start;
  }

  const result = summarize(samples);
  for (const [name, value] of Object.entries(result)) {
    assert.ok(Number.isFinite(value), `fresh PCM window application produced a non-finite ${name}`);
  }
  assert.ok(
    result.p95Ms < QUANTUM_BUDGET_MS * WINDOW_APPLY_P95_BUDGET_FRACTION,
    `fresh 6-second stereo PCM window p95 ${result.p95Ms.toFixed(4)} ms exceeded ${(
      WINDOW_APPLY_P95_BUDGET_FRACTION * 100
    ).toFixed(0)}% of the quantum budget`,
  );
  assert.ok(
    result.maxMs < QUANTUM_BUDGET_MS * WINDOW_APPLY_MAX_BUDGET_FRACTION,
    `fresh 6-second stereo PCM window max ${result.maxMs.toFixed(4)} ms exceeded ${(
      WINDOW_APPLY_MAX_BUDGET_FRACTION * 100
    ).toFixed(0)}% of the quantum budget`,
  );
  const applied = processor.port.messages.at(-1);
  assert.equal(applied?.type, "window-applied");
  assert.equal(applied?.applied, true);
  processor.handleMessage({ type: "stream-complete" });
  processor.handleMessage({ type: "set-effects", acoustic: false, surface: false });
  processor.handleMessage({ type: "needle", lifted: false });
  processor.handleMessage({ type: "transport", running: true });
  processor.handleMessage({
    type: "play",
    position: WINDOW_CENTER,
    rate: 1,
    handoff: true,
  });
  const copiedOutput = createOutput();
  for (let iteration = 0; iteration < 32; iteration += 1) renderQuantum(processor, copiedOutput);
  assert.ok(channelEnergy(copiedOutput[0][0], 0, FRAME_COUNT) > 0.1, "left PCM copy rendered silence");
  assert.ok(channelEnergy(copiedOutput[0][1], 0, FRAME_COUNT) > 0.1, "right PCM copy rendered silence");
  return result;
}

function assertFiniteOutput(output, label) {
  for (const channel of output[0]) {
    for (const sample of channel) {
      assert.ok(Number.isFinite(sample), `${label} emitted a non-finite sample`);
    }
  }
}

function channelEnergy(channel, start, end) {
  let energy = 0;
  for (let index = start; index < end; index += 1) energy += Math.abs(channel[index]);
  return energy;
}

function warmStablePlayback(processor, quantumCount = 512) {
  const output = createOutput();
  for (let iteration = 0; iteration < quantumCount; iteration += 1) renderQuantum(processor, output);
}

function armEofAtOffset(processor, expectedOffset) {
  const rate = Math.max(0.0001, Number(processor.dsp.effectiveRate) || 1);
  processor.dsp.setPosition(WINDOW_FRAMES - 3 - rate * expectedOffset, 0);
  processor.port.messages.length = 0;
}

function endedMessage(processor) {
  return processor.port.messages.find(message => message?.type === "ended");
}

function verifyEofHandoffs() {
  const expectedOffset = 37;

  globalThis.currentFrame = 0;
  globalThis.currentTime = 0;
  const deadwaxProcessor = createProcessor();
  deadwaxProcessor.handleMessage({ type: "set-effects", acoustic: false, surface: true });
  deadwaxProcessor.handleMessage({ type: "end-behavior", cleanEnd: false, deadwaxTurns: 2 });
  warmStablePlayback(deadwaxProcessor);
  let startCalls = 0;
  let programmeRenderCalls = 0;
  const originalStart = deadwaxProcessor.dsp.start.bind(deadwaxProcessor.dsp);
  const originalRender = deadwaxProcessor.dsp.render.bind(deadwaxProcessor.dsp);
  deadwaxProcessor.dsp.start = (...args) => {
    startCalls += 1;
    return originalStart(...args);
  };
  deadwaxProcessor.dsp.render = (...args) => {
    programmeRenderCalls += 1;
    return originalRender(...args);
  };
  armEofAtOffset(deadwaxProcessor, expectedOffset);
  const deadwaxOutput = createOutput();
  const deadwaxQuantumStart = currentFrame;
  const turnsBefore = deadwaxProcessor.dsp.platterRotationTurns;
  renderQuantum(deadwaxProcessor, deadwaxOutput);
  const deadwaxEnded = endedMessage(deadwaxProcessor);
  assert.ok(deadwaxEnded, "published playback did not report its exact programme end");
  const deadwaxOffset = deadwaxEnded.outputFrame - deadwaxQuantumStart;
  assert.equal(deadwaxOffset, expectedOffset, "programme/deadwax handoff moved off its expected frame");
  assert.equal(deadwaxEnded.deadwaxStarted, true);
  assert.equal(programmeRenderCalls, 1, "exact EOF used more than one Rust programme render call");
  assert.ok(channelEnergy(deadwaxOutput[0][0], 0, deadwaxOffset) > 0.01, "programme prefix was silent");
  assert.ok(
    channelEnergy(deadwaxOutput[0][0], deadwaxOffset, FRAME_COUNT) > 0.00001,
    "deadwax suffix was silent",
  );
  assert.equal(startCalls, 0, "programme/deadwax handoff reset the DSP");
  assert.ok(deadwaxProcessor.dsp.platterRotationTurns > turnsBefore, "platter rotation stopped at deadwax");

  const automaticStartFrame = deadwaxProcessor.surfaceRegion.startFrame;
  const automaticEndFrame = deadwaxProcessor.surfaceRegion.endFrame;
  deadwaxProcessor.handleMessage({ type: "stop", handoff: true, playbackEpoch: 2 });
  deadwaxProcessor.handleMessage({
    type: "surface-region",
    action: "start",
    region: "deadwax",
    regionId: 17,
    durationFrames: deadwaxEnded.deadwaxDurationFrames,
  });
  assert.equal(deadwaxProcessor.surfaceRegion.startFrame, automaticStartFrame);
  assert.equal(deadwaxProcessor.surfaceRegion.endFrame, automaticEndFrame);
  assert.equal(deadwaxProcessor.surfaceRegion.regionId, 17);
  assert.equal(startCalls, 0, "duplicate host deadwax start reset the DSP");
  const adoptedTurns = deadwaxProcessor.dsp.platterRotationTurns;
  renderQuantum(deadwaxProcessor, createOutput());
  assert.ok(deadwaxProcessor.dsp.platterRotationTurns > adoptedTurns, "adopted deadwax lost platter motion");

  globalThis.currentFrame = 0;
  globalThis.currentTime = 0;
  const cleanProcessor = createProcessor();
  cleanProcessor.handleMessage({ type: "set-effects", acoustic: false, surface: true });
  cleanProcessor.handleMessage({ type: "end-behavior", cleanEnd: true, deadwaxTurns: 2 });
  warmStablePlayback(cleanProcessor);
  let surfaceStarts = 0;
  const originalSurfaceStart = cleanProcessor.dsp.startSurfaceRegion.bind(cleanProcessor.dsp);
  cleanProcessor.dsp.startSurfaceRegion = (...args) => {
    surfaceStarts += 1;
    return originalSurfaceStart(...args);
  };
  armEofAtOffset(cleanProcessor, expectedOffset);
  const cleanOutput = createOutput();
  const cleanQuantumStart = currentFrame;
  renderQuantum(cleanProcessor, cleanOutput);
  const cleanEnded = endedMessage(cleanProcessor);
  assert.ok(cleanEnded, "clean preview did not report its exact programme end");
  const cleanOffset = cleanEnded.outputFrame - cleanQuantumStart;
  assert.equal(cleanOffset, expectedOffset, "clean preview end moved off its expected frame");
  assert.equal(cleanEnded.deadwaxStarted, false);
  assert.equal(channelEnergy(cleanOutput[0][0], cleanOffset, FRAME_COUNT), 0, "clean end suffix was not silent");
  assert.equal(surfaceStarts, 0, "clean end started a deadwax surface region");
  assert.equal(cleanProcessor.surfaceRegion, null);

  globalThis.currentFrame = 0;
  globalThis.currentTime = 0;
  const fortyFiveProcessor = createProcessor();
  fortyFiveProcessor.handleMessage({ type: "set-effects", acoustic: false, surface: true });
  fortyFiveProcessor.handleMessage({ type: "end-behavior", cleanEnd: false, deadwaxTurns: 2 });
  fortyFiveProcessor.handleMessage({
    type: "play",
    position: WINDOW_CENTER,
    rate: 45 / 33.3333333333,
    handoff: true,
    playbackEpoch: 3,
  });
  warmStablePlayback(fortyFiveProcessor);
  armEofAtOffset(fortyFiveProcessor, expectedOffset);
  const fortyFiveStart = currentFrame;
  renderQuantum(fortyFiveProcessor, createOutput());
  const fortyFiveEnded = endedMessage(fortyFiveProcessor);
  assert.ok(fortyFiveEnded, "45 RPM playback did not enter deadwax");
  assert.equal(fortyFiveEnded.outputFrame - fortyFiveStart, expectedOffset);
  assert.equal(
    fortyFiveEnded.deadwaxDurationFrames,
    Math.round(2 * 60 / 45 * SAMPLE_RATE),
    "deadwax duration did not follow selected RPM",
  );

  fortyFiveProcessor.handleMessage({
    type: "scratch",
    active: true,
    position: fortyFiveProcessor.dsp.position,
    rate: -1,
    impulse: 0.22,
  });
  assert.equal(fortyFiveProcessor.surfaceRegion, null, "scratch did not interrupt automatic deadwax");
  assert.equal(fortyFiveProcessor.scratching, true);

  globalThis.currentFrame = 0;
  globalThis.currentTime = 0;
  const rejectedReplayProcessor = createProcessor();
  rejectedReplayProcessor.handleMessage({ type: "end-behavior", cleanEnd: false, deadwaxTurns: 2 });
  warmStablePlayback(rejectedReplayProcessor, 64);
  armEofAtOffset(rejectedReplayProcessor, expectedOffset);
  renderQuantum(rejectedReplayProcessor, createOutput());
  rejectedReplayProcessor.port.messages.length = 0;
  rejectedReplayProcessor.handleMessage({
    type: "replay-scratch",
    id: 73,
    performance: { durationFrames: 12, initialState: {}, events: [] },
  });
  const rejectedReplay = rejectedReplayProcessor.port.messages.find(message => (
    message?.type === "scratch-replay-ended" && message.id === 73
  ));
  assert.equal(rejectedReplay?.cancelled, true, "replay was left pending behind automatic deadwax");
  assert.equal(rejectedReplayProcessor.replay, null);
  assert.equal(rejectedReplayProcessor.surfaceRegion?.automatic, true);
}

function verifyReplayDurationBoundary() {
  globalThis.currentFrame = 0;
  globalThis.currentTime = 0;
  const processor = createProcessor();
  processor.handleMessage({ type: "set-effects", acoustic: false, surface: true });
  warmStablePlayback(processor, 64);
  processor.port.messages.length = 0;
  const durationFrames = 37;
  processor.handleMessage({
    type: "replay-scratch",
    id: 91,
    performance: {
      durationFrames,
      initialState: {
        positionFrames: WINDOW_CENTER,
        playbackRate: 1,
        motorRunning: true,
        playing: true,
        needleLifted: false,
        manualCrossfader: 0,
        preset: "baby",
        clicks: 1,
      },
      events: [{ type: "manual-crossfader", frameOffset: 5, value: 0 }],
      effects: { acoustic: false, surface: true },
    },
  });
  const expectedFinishFrame = processor.replay.startFrame + durationFrames;
  const replayOutput = createOutput();
  renderQuantum(processor, replayOutput);
  const ended = processor.port.messages.find(message => (
    message?.type === "scratch-replay-ended" && message.id === 91
  ));
  assert.ok(ended, "tail-bearing replay did not finish");
  assert.equal(ended.outputFrame, expectedFinishFrame, "replay ignored its durationFrames boundary");
  assert.equal(
    channelEnergy(replayOutput[0][0], 0, durationFrames),
    0,
    "muted replay leaked before its duration boundary",
  );
  assert.ok(
    channelEnergy(replayOutput[0][0], durationFrames, FRAME_COUNT) > 0.01,
    "restored programme did not resume at the replay duration boundary",
  );
  assert.equal(processor.replay, null);
}

function verifyScratchGateVersionReplay() {
  globalThis.currentFrame = 0;
  globalThis.currentTime = 0;
  const processor = createProcessor();
  assert.equal(processor.dsp.scratchGateAlgorithmVersion, 4);

  processor.handleMessage({
    type: "replay-scratch",
    id: 97,
    performance: {
      durationFrames: FRAME_COUNT * 2,
      engine: { gateAlgorithmVersion: 3 },
      initialState: { positionFrames: WINDOW_CENTER, preset: "chirp", clicks: 8 },
      events: [],
    },
  });
  assert.equal(processor.dsp.scratchGateAlgorithmVersion, 3);
  processor.handleMessage({ type: "cancel-scratch-replay" });
  assert.equal(processor.dsp.scratchGateAlgorithmVersion, 4);
  assert.equal(processor.scratchGateAlgorithmVersion, 4);
}

function verifyReplayControlInterruption() {
  globalThis.currentFrame = 0;
  globalThis.currentTime = 0;
  const processor = createProcessor();
  warmStablePlayback(processor, 64);
  const originalPosition = processor.dsp.position;
  processor.port.messages.length = 0;

  processor.handleMessage({
    type: "replay-scratch",
    id: 92,
    performance: {
      durationFrames: FRAME_COUNT * 4,
      initialState: {
        positionFrames: WINDOW_CENTER - 2_000,
        playbackRate: 1,
        motorRunning: true,
        playing: true,
        needleLifted: false,
        preset: "crab",
        clicks: 8,
        highFrequencyAccelerationLimit: 0,
      },
      events: [],
    },
  });
  assert.ok(processor.replay, "control-interruption fixture did not start replay");
  processor.handleMessage({ type: "hf-acceleration-limit", strength: 0.83 });
  const interrupted = processor.port.messages.find(message => (
    message?.type === "scratch-replay-ended" && message.id === 92
  ));
  assert.equal(interrupted?.cancelled, true, "live limiter control did not interrupt replay");
  assert.equal(processor.replay, null);
  assert.ok(
    Math.abs(processor.dsp.position - originalPosition) < 0.001,
    "replay snapshot did not restore its authoritative position before the live control",
  );
  assert.ok(
    Math.abs(processor.dsp.highFrequencyAccelerationLimit - 0.83) < 0.000001,
    "live limiter value was overwritten by replay restoration",
  );
  assert.equal(processor.scratchPreset, "baby", "replay preset leaked into persistent worklet state");

  processor.handleMessage({
    type: "replay-scratch",
    id: 93,
    performance: {
      durationFrames: FRAME_COUNT * 4,
      initialState: { positionFrames: WINDOW_CENTER + 2_000, preset: "flare", clicks: 1 },
      events: [],
    },
  });
  const seekPosition = WINDOW_CENTER + 8_000;
  processor.handleMessage({ type: "seek", position: seekPosition, generation: 7, impulse: 0 });
  const seekInterrupted = processor.port.messages.find(message => (
    message?.type === "scratch-replay-ended" && message.id === 93
  ));
  assert.equal(seekInterrupted?.cancelled, true, "live seek did not interrupt replay");
  assert.ok(
    Math.abs(processor.dsp.position - seekPosition) < 0.001,
    "replay restoration overwrote the live seek",
  );
  assert.ok(
    Math.abs(processor.dsp.highFrequencyAccelerationLimit - 0.83) < 0.000001,
    "a second replay lost the post-cancellation limiter setting",
  );
}

function verifyFarWindowReplayCancellation() {
  globalThis.currentFrame = 0;
  globalThis.currentTime = 0;
  const processor = createProcessor();
  warmStablePlayback(processor, 64);
  const originalPosition = processor.dsp.position;
  const originalRate = processor.dsp.effectiveRate;
  const replayPosition = originalPosition + 5_000;

  // Model a bounded window around the original position while retaining the
  // already-loaded full fixture inside Rust. Replay must request its distant
  // reset-position window and then be cancelled before that response arrives.
  processor.windowStart = Math.floor(originalPosition) - 256;
  processor.windowEnd = Math.floor(originalPosition) + 256;
  processor.windowFrames = 512;
  processor.port.messages.length = 0;
  processor.handleMessage({
    type: "replay-scratch",
    id: 94,
    performance: {
      durationFrames: FRAME_COUNT * 8,
      initialState: {
        positionFrames: replayPosition,
        playbackRate: 1,
        motorRunning: true,
        playing: true,
        needleLifted: false,
      },
      events: [],
    },
  });
  assert.equal(processor.waitingForData, true);
  assert.equal(processor.windowRequestPending, true);
  const obsoleteRequestId = processor.pendingWindowRequest.workletRequestId;
  assert.ok(obsoleteRequestId > 0);

  processor.handleMessage({ type: "cancel-scratch-replay" });
  assert.equal(processor.replay, null);
  assert.ok(Math.abs(processor.dsp.position - originalPosition) < 0.001);
  assert.ok(Math.abs(processor.dsp.effectiveRate - originalRate) < 0.000001);

  const applyWindow = ({ start, position, generation, workletRequestId, resetPosition }) => {
    const length = 512;
    processor.handleMessage({
      type: "window-ready",
      generation,
      requestId: generation,
      workletRequestId,
      start,
      totalFrames: WINDOW_FRAMES,
      length,
      availableEnd: WINDOW_FRAMES,
      sampleRate: SAMPLE_RATE,
      bankIndex: -1,
      channelBuffers: [
        left.slice(start, start + length).buffer,
        right.slice(start, start + length).buffer,
      ],
      position,
      resetPosition,
    });
  };

  const replayWindowStart = Math.floor(replayPosition) - 128;
  applyWindow({
    start: replayWindowStart,
    position: replayPosition,
    generation: 2,
    workletRequestId: obsoleteRequestId,
    resetPosition: true,
  });
  assert.ok(
    Math.abs(processor.dsp.position - originalPosition) < 0.001,
    "late replay window reset the restored Rust position",
  );
  assert.ok(
    Math.abs(processor.dsp.effectiveRate - originalRate) < 0.000001,
    "late replay window reset restored platter dynamics",
  );
  assert.equal(processor.waitingForData, true);
  assert.equal(processor.pendingWindowRequest.position, originalPosition);
  assert.equal(processor.pendingWindowRequest.resetPosition, false);
  const restoreRequestId = processor.pendingWindowRequest.workletRequestId;
  assert.ok(restoreRequestId > obsoleteRequestId);

  const restoreWindowStart = Math.floor(originalPosition) - 128;
  applyWindow({
    start: restoreWindowStart,
    position: originalPosition,
    generation: 3,
    workletRequestId: restoreRequestId,
    resetPosition: false,
  });
  assert.equal(processor.waitingForData, false);
  assert.equal(processor.replayRestoreWindowPosition, null);
  assert.ok(Math.abs(processor.dsp.position - originalPosition) < 0.001);
  assert.ok(Math.abs(processor.dsp.effectiveRate - originalRate) < 0.000001);

  const unavailableProcessor = createProcessor();
  warmStablePlayback(unavailableProcessor, 64);
  const unavailableOriginalPosition = unavailableProcessor.dsp.position;
  const unavailableOriginalRate = unavailableProcessor.dsp.effectiveRate;
  unavailableProcessor.windowStart = Math.floor(unavailableOriginalPosition) - 256;
  unavailableProcessor.windowEnd = Math.floor(unavailableOriginalPosition) + 256;
  unavailableProcessor.windowFrames = 512;
  unavailableProcessor.handleMessage({
    type: "replay-scratch",
    id: 95,
    performance: {
      durationFrames: FRAME_COUNT * 8,
      initialState: { positionFrames: unavailableOriginalPosition + 5_000 },
      events: [],
    },
  });
  const unavailableRequestId = unavailableProcessor.pendingWindowRequest.workletRequestId;
  unavailableProcessor.handleMessage({ type: "cancel-scratch-replay" });
  unavailableProcessor.handleMessage({
    type: "window-unavailable",
    workletRequestId: unavailableRequestId,
    position: unavailableOriginalPosition + 5_000,
    availableEnd: WINDOW_FRAMES,
  });
  assert.equal(unavailableProcessor.windowRequestPending, false);
  assert.equal(unavailableProcessor.queuedWindowRequest, null);
  assert.equal(unavailableProcessor.replayRestoreWindowPosition, null);
  assert.ok(Math.abs(unavailableProcessor.dsp.position - unavailableOriginalPosition) < 0.001);
  assert.ok(Math.abs(unavailableProcessor.dsp.effectiveRate - unavailableOriginalRate) < 0.000001);
}

function hashReplayOutput(channelBlocks) {
  const hash = createHash("sha256");
  for (const channel of channelBlocks) {
    for (const block of channel) {
      hash.update(new Uint8Array(block.buffer, block.byteOffset, block.byteLength));
    }
  }
  return hash.digest("hex");
}

function captureReplayEvidence(processor, id, scratchPerformance) {
  const channelBlocks = Array.from({ length: CHANNEL_COUNT }, () => []);
  const gateTrace = [];
  const controlTrace = [];
  let renderedFrames = 0;
  const originalRender = processor.dsp.render.bind(processor.dsp);
  const originalSetPreset = processor.dsp.setScratchPreset.bind(processor.dsp);
  const originalSetClicks = processor.dsp.setScratchClicks.bind(processor.dsp);
  const originalSetManualCrossfader = processor.dsp.setManualCrossfader.bind(processor.dsp);
  processor.dsp.render = (frameCount, channelCount) => {
    const rendered = originalRender(frameCount, channelCount);
    renderedFrames += frameCount;
    gateTrace.push({
      frameOffset: renderedFrames,
      gate: processor.dsp.scratchGate,
      phase: processor.dsp.scratchGatePhase,
      direction: processor.dsp.scratchDirection,
    });
    return rendered;
  };
  processor.dsp.setScratchPreset = preset => {
    controlTrace.push({ type: "scratch-preset", frameOffset: renderedFrames, value: preset });
    return originalSetPreset(preset);
  };
  processor.dsp.setScratchClicks = clicks => {
    controlTrace.push({ type: "scratch-clicks", frameOffset: renderedFrames, value: clicks });
    return originalSetClicks(clicks);
  };
  processor.dsp.setManualCrossfader = value => {
    controlTrace.push({ type: "manual-crossfader", frameOffset: renderedFrames, value });
    return originalSetManualCrossfader(value);
  };

  try {
    processor.handleMessage({
      type: "replay-scratch",
      id,
      performance: scratchPerformance,
      effectsMode: "original",
    });
    assert.ok(processor.replay, `deterministic replay ${id} did not start`);
    // Initial-state controls are applied before frame zero. Keep only the
    // scheduled event trace below.
    controlTrace.length = 0;
    let quantumCount = 0;
    while (processor.replay && quantumCount < 32) {
      const output = createOutput();
      renderQuantum(processor, output);
      for (let channel = 0; channel < CHANNEL_COUNT; channel += 1) {
        channelBlocks[channel].push(output[0][channel].slice());
      }
      quantumCount += 1;
    }
    assert.equal(processor.replay, null, `deterministic replay ${id} did not finish`);
    assert.equal(renderedFrames, scratchPerformance.durationFrames);
  } finally {
    processor.dsp.render = originalRender;
    processor.dsp.setScratchPreset = originalSetPreset;
    processor.dsp.setScratchClicks = originalSetClicks;
    processor.dsp.setManualCrossfader = originalSetManualCrossfader;
  }

  return {
    outputHash: hashReplayOutput(channelBlocks),
    gateTrace,
    controlTrace,
    channelBlocks,
  };
}

function verifyDeterministicReplay() {
  globalThis.currentFrame = 0;
  globalThis.currentTime = 0;
  const processor = createProcessor();
  warmStablePlayback(processor, 64);
  const scratchPerformance = {
    durationFrames: FRAME_COUNT * 4,
    replaySeed: 0x4d2c6df3,
    effects: { acoustic: true, surface: true },
    initialState: {
      positionFrames: WINDOW_CENTER,
      playbackRate: 1,
      motorRunning: true,
      playing: true,
      needleLifted: false,
      manualCrossfader: 0,
      preset: "flare",
      clicks: 2,
      nativeRpm: 33.3333333333,
      highFrequencyAccelerationLimit: 0.35,
      stylusTracingLimit: 0.72,
    },
    events: [
      { type: "scratch-start", frameOffset: 0, positionFrames: WINDOW_CENTER, rate: 1.4, impulse: 0.4, grip: 0.65 },
      { type: "scratch-motion", frameOffset: 37, positionFrames: WINDOW_CENTER + 420, rate: 1.7, impulse: 0, grip: 0.75 },
      { type: "scratch-preset", frameOffset: 83, preset: "crab" },
      { type: "scratch-clicks", frameOffset: 91, clicks: 8 },
      { type: "manual-crossfader", frameOffset: 155, value: 1 },
      { type: "scratch-motion", frameOffset: 233, positionFrames: WINDOW_CENTER - 360, rate: -2.1, impulse: 0.25, grip: 0.91 },
      { type: "manual-crossfader", frameOffset: 301, value: 0 },
      { type: "scratch-end", frameOffset: 447, positionFrames: WINDOW_CENTER - 120, rate: 0, impulse: 0, resumePlayback: true },
    ],
  };

  const first = captureReplayEvidence(processor, 101, scratchPerformance);
  // A saved take must not inherit the live deck's later wow, noise or platter
  // phase. Advance ordinary playback before the second run to prove that the
  // recording, rather than invocation time, defines the rendered result.
  warmStablePlayback(processor, 17);
  const second = captureReplayEvidence(processor, 102, scratchPerformance);
  assert.match(first.outputHash, /^[0-9a-f]{64}$/);
  assert.equal(second.outputHash, first.outputHash, "identical replays produced different output hashes");
  assert.deepEqual(second.gateTrace, first.gateTrace, "identical replays produced different gate traces");
  const fullGripPerformance = {
    ...scratchPerformance,
    events: scratchPerformance.events.map(event => (
      event.type === "scratch-start" || event.type === "scratch-motion"
        ? { ...event, grip: 1 }
        : event
    )),
  };
  const fullGrip = captureReplayEvidence(processor, 103, fullGripPerformance);
  assert.notEqual(
    fullGrip.outputHash,
    first.outputHash,
    "variable grip did not change the deterministic Rust replay",
  );
  assert.deepEqual(first.controlTrace, [
    { type: "scratch-preset", frameOffset: 83, value: "crab" },
    { type: "scratch-clicks", frameOffset: 91, value: 8 },
    { type: "manual-crossfader", frameOffset: 155, value: 1 },
    { type: "manual-crossfader", frameOffset: 301, value: 0 },
  ], "replay controls did not apply on their requested sub-quantum frames");
  assert.ok(
    first.channelBlocks.some(channel => channel.some(block => channelEnergy(block, 0, block.length) > 0.01)),
    "deterministic replay fixture rendered only silence",
  );
  return {
    outputHash: first.outputHash,
    gateSegments: first.gateTrace.length,
    controlEvents: first.controlTrace.length,
    gripContrastHash: fullGrip.outputHash,
    interveningPlaybackFrames: 17 * FRAME_COUNT,
  };
}

function benchmark(label, processor, beforeQuantum, afterQuantum) {
  const output = createOutput();
  globalThis.currentFrame = 0;
  globalThis.currentTime = 0;

  for (let iteration = 0; iteration < WARMUP_ITERATIONS; iteration += 1) {
    beforeQuantum?.(iteration);
    renderQuantum(processor, output);
    afterQuantum?.(iteration);
  }

  const samples = new Array(MEASURED_ITERATIONS);
  for (let iteration = 0; iteration < MEASURED_ITERATIONS; iteration += 1) {
    beforeQuantum?.(iteration);
    const start = performance.now();
    processor.process([], output);
    samples[iteration] = performance.now() - start;
    globalThis.currentFrame += FRAME_COUNT;
    globalThis.currentTime = currentFrame / SAMPLE_RATE;
    afterQuantum?.(iteration);
  }

  assertFiniteOutput(output, label);
  const result = summarize(samples);
  for (const [name, value] of Object.entries(result)) {
    assert.ok(Number.isFinite(value), `${label} produced a non-finite ${name}`);
  }
  assert.ok(
    result.p95Ms < QUANTUM_BUDGET_MS * REGRESSION_BUDGET_FRACTION,
    `${label} p95 ${result.p95Ms.toFixed(4)} ms exceeded ${(
      REGRESSION_BUDGET_FRACTION * 100
    ).toFixed(0)}% of the ${QUANTUM_BUDGET_MS.toFixed(4)} ms quantum budget`,
  );
  return result;
}

verifyEofHandoffs();
verifyReplayDurationBoundary();
verifyScratchGateVersionReplay();
verifyReplayControlInterruption();
verifyFarWindowReplayCancellation();
const deterministicReplay = verifyDeterministicReplay();

const freshWindowApplication = benchmarkFreshWindowApplication();

const normalProcessor = createProcessor();
const normal = benchmark("normal playback", normalProcessor, iteration => {
  if (iteration % 512 === 0) normalProcessor.dsp.setPosition(WINDOW_CENTER, 0);
});

const scratchProcessor = createProcessor();
scratchProcessor.handleMessage({ type: "scratch-preset", preset: "crab" });
scratchProcessor.handleMessage({ type: "scratch-clicks", clicks: 8 });
let scratchDirection = 1;
let peakScratchEffectiveRate = 0;
const scratch = benchmark("reversing 8x scratch", scratchProcessor, iteration => {
  if (iteration % 64 === 0) {
    scratchDirection *= -1;
    scratchProcessor.dsp.setPosition(WINDOW_CENTER, 0);
  }
  scratchProcessor.handleMessage({
    type: "motion",
    position: WINDOW_CENTER,
    rate: scratchDirection * 8,
    impulse: iteration % 64 === 0 ? 1 : 0,
  });
}, () => {
  peakScratchEffectiveRate = Math.max(
    peakScratchEffectiveRate,
    Math.abs(scratchProcessor.dsp.effectiveRate),
  );
});
assert.ok(
  peakScratchEffectiveRate > 4,
  `scratch workload never reached a demanding effective rate (peak ${peakScratchEffectiveRate})`,
);

function formatResult(label, result) {
  return [
    label.padEnd(27),
    `mean ${result.meanMs.toFixed(4)} ms (${result.meanBudgetPercent.toFixed(2)}%)`,
    `p95 ${result.p95Ms.toFixed(4)} ms (${result.p95BudgetPercent.toFixed(2)}%)`,
    `max ${result.maxMs.toFixed(4)} ms (${result.maxBudgetPercent.toFixed(2)}%)`,
  ].join(" | ");
}

console.log(
  `Release-WASM AudioWorklet benchmark: ${CHANNEL_COUNT}ch x ${FRAME_COUNT} frames at ${SAMPLE_RATE} Hz; `
  + `${WARMUP_ITERATIONS} warmup + ${MEASURED_ITERATIONS} measured; `
  + `${QUANTUM_BUDGET_MS.toFixed(4)} ms budget`,
);
console.log(formatResult("Normal playback", normal));
console.log(formatResult("Reversing +/-8x crab/8", scratch));
console.log(
  `Deterministic replay        | SHA-256 ${deterministicReplay.outputHash}`
  + ` | ${deterministicReplay.gateSegments} gate segments`
  + ` | ${deterministicReplay.controlEvents} sub-quantum controls`
  + ` | variable-grip contrast ${deterministicReplay.gripContrastHash.slice(0, 12)}`
  + ` | ${deterministicReplay.interveningPlaybackFrames} live frames between runs`,
);
console.log(
  formatResult(
    `Fresh ${WINDOW_FRAMES / SAMPLE_RATE}s PCM window`,
    freshWindowApplication,
  ) + ` | ${WINDOW_APPLY_MEASURED_ITERATIONS} measured`,
);
