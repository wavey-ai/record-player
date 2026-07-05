"use strict";

const state = {
  sampleRate: 48000,
  totalFrames: 0,
  channelCount: 0,
  chunkFrames: 48000,
  channels: [],
  banks: [],
  windowFrames: 0,
  nextBank: 0,
  generation: 0,
  availableStart: 0,
  availableEnd: 0,
};

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function initialise(message) {
  state.sampleRate = Math.max(1, Number(message.sampleRate) || 48000);
  state.totalFrames = Math.max(1, Math.floor(Number(message.totalFrames) || 1));
  state.channelCount = Math.max(1, Math.floor(Number(message.channelCount) || 2));
  state.chunkFrames = Math.max(1024, Math.floor(Number(message.chunkFrames) || state.sampleRate));
  state.windowFrames = Math.max(4096, Math.floor(Number(message.windowFrames) || state.sampleRate * 12));
  state.banks = (message.bankBuffers || []).map(bankBuffers =>
    bankBuffers.map(buffer => new Float32Array(buffer))
  );
  state.channels = Array.from({ length: state.channelCount }, () => new Int16Array(state.totalFrames));
  state.availableStart = state.totalFrames;
  state.availableEnd = 0;
}

function appendSegment(segment) {
  const start = clamp(Math.floor(Number(segment.startFrame) || 0), 0, state.totalFrames);
  const end = clamp(Math.floor(Number(segment.endFrame) || start), start, state.totalFrames);
  const frameCount = end - start;
  if (!frameCount) return;
  const buffers = Array.isArray(segment.channelBuffers) ? segment.channelBuffers : [];
  for (let channel = 0; channel < state.channelCount; channel += 1) {
    const sourceBuffer = buffers[Math.min(channel, Math.max(0, buffers.length - 1))];
    if (!sourceBuffer) continue;
    const source = new Int16Array(sourceBuffer, 0, Math.min(frameCount, sourceBuffer.byteLength / Int16Array.BYTES_PER_ELEMENT));
    state.channels[channel].set(source, start);
  }
  state.availableStart = Math.min(state.availableStart, start);
  state.availableEnd = Math.max(state.availableEnd, end);
}

function fillWindow(position, resetPosition, requestId) {
  if (!state.banks.length || !state.totalFrames || state.availableEnd <= state.availableStart) return;
  const bankIndex = state.nextBank;
  state.nextBank = (state.nextBank + 1) % state.banks.length;
  const half = Math.floor(state.windowFrames / 2);
  const maxStart = Math.max(0, state.totalFrames - state.windowFrames);
  const requestedStart = clamp(Math.round(position) - half, 0, maxStart);
  const start = Math.min(requestedStart, Math.max(0, state.availableEnd - 1));
  const length = Math.min(state.windowFrames, state.totalFrames - start);
  const bank = state.banks[bankIndex];

  for (let channel = 0; channel < state.channelCount; channel += 1) {
    const target = bank[channel];
    target.fill(0);
    const copyStart = Math.max(start, state.availableStart);
    const copyEnd = Math.min(start + length, state.availableEnd);
    if (copyEnd > copyStart) {
      const source = state.channels[channel].subarray(copyStart, copyEnd);
      const targetOffset = copyStart - start;
      for (let frame = 0; frame < source.length; frame += 1) {
        target[targetOffset + frame] = source[frame] / 32768;
      }
    }
  }

  state.generation += 1;
  postMessage({
    type: "window-ready",
    bankIndex,
    start,
    length,
    availableStart: state.availableStart,
    availableEnd: state.availableEnd,
    totalFrames: state.totalFrames,
    sampleRate: state.sampleRate,
    generation: state.generation,
    resetPosition: Boolean(resetPosition),
    position: clamp(Number(position) || 0, 0, Math.max(0, state.totalFrames - 1)),
    requestId,
  });
}

self.onmessage = event => {
  const message = event.data || {};
  if (message.type === "init-progressive") {
    initialise(message);
    postMessage({ type: "initialised", totalFrames: state.totalFrames, sampleRate: state.sampleRate });
    return;
  }
  if (message.type === "append-segments") {
    const segments = Array.isArray(message.segments) ? message.segments : [];
    for (const segment of segments) appendSegment(segment);
    if (segments.length) fillWindow(Number(message.position) || 0, Boolean(message.resetPosition), message.requestId);
    return;
  }
  if (message.type === "init") {
    initialise(message);
    const buffers = Array.isArray(message.channelBuffers) ? message.channelBuffers : [];
    appendSegment({ startFrame: 0, endFrame: state.totalFrames, channelBuffers: buffers });
    postMessage({ type: "initialised", totalFrames: state.totalFrames, sampleRate: state.sampleRate });
    fillWindow(0, true, message.requestId);
    return;
  }
  if (message.type === "request-window") {
    fillWindow(message.position, message.resetPosition, message.requestId);
  }
};
