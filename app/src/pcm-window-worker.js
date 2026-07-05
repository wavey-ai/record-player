"use strict";

const state = {
  sampleRate: 48000,
  totalFrames: 0,
  channelCount: 0,
  chunkFrames: 48000,
  chunks: [],
  banks: [],
  windowFrames: 0,
  nextBank: 0,
  generation: 0
};

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function buildChunks(channelBuffers) {
  const sources = channelBuffers.map(buffer => new Int16Array(buffer));
  const chunkCount = Math.ceil(state.totalFrames / state.chunkFrames);
  state.chunks = Array.from({ length: state.channelCount }, () => new Array(chunkCount));
  for (let channel = 0; channel < state.channelCount; channel += 1) {
    const source = sources[Math.min(channel, sources.length - 1)];
    for (let chunkIndex = 0; chunkIndex < chunkCount; chunkIndex += 1) {
      const start = chunkIndex * state.chunkFrames;
      const end = Math.min(state.totalFrames, start + state.chunkFrames);
      state.chunks[channel][chunkIndex] = source.slice(start, end);
    }
  }
}

function sampleAt(channel, frame) {
  const chunkIndex = Math.floor(frame / state.chunkFrames);
  const chunkOffset = frame - chunkIndex * state.chunkFrames;
  return state.chunks[channel]?.[chunkIndex]?.[chunkOffset] ?? 0;
}

function fillWindow(position, resetPosition, requestId) {
  if (!state.banks.length || !state.totalFrames) return;
  const bankIndex = state.nextBank;
  state.nextBank = (state.nextBank + 1) % state.banks.length;
  const half = Math.floor(state.windowFrames / 2);
  const maxStart = Math.max(0, state.totalFrames - state.windowFrames);
  const start = clamp(Math.round(position) - half, 0, maxStart);
  const length = Math.min(state.windowFrames, state.totalFrames - start);
  const bank = state.banks[bankIndex];

  for (let channel = 0; channel < state.channelCount; channel += 1) {
    const target = bank[channel];
    for (let frame = 0; frame < length; frame += 1) {
      target[frame] = sampleAt(channel, start + frame) / 32768;
    }
    if (length < target.length) target.fill(0, length);
  }

  state.generation += 1;
  postMessage({
    type: "window-ready",
    bankIndex,
    start,
    length,
    totalFrames: state.totalFrames,
    sampleRate: state.sampleRate,
    generation: state.generation,
    resetPosition: Boolean(resetPosition),
    position: clamp(Number(position) || 0, 0, Math.max(0, state.totalFrames - 1)),
    requestId
  });
}

self.onmessage = event => {
  const message = event.data || {};
  if (message.type === "init") {
    state.sampleRate = Math.max(1, Number(message.sampleRate) || 48000);
    state.totalFrames = Math.max(1, Math.floor(Number(message.totalFrames) || 1));
    state.channelCount = Math.max(1, Math.floor(Number(message.channelCount) || 2));
    state.chunkFrames = Math.max(1024, Math.floor(Number(message.chunkFrames) || state.sampleRate));
    state.windowFrames = Math.max(4096, Math.floor(Number(message.windowFrames) || state.sampleRate * 12));
    state.banks = (message.bankBuffers || []).map(bankBuffers =>
      bankBuffers.map(buffer => new Float32Array(buffer))
    );
    buildChunks(Array.isArray(message.channelBuffers) ? message.channelBuffers : []);
    postMessage({ type: "initialised", totalFrames: state.totalFrames, sampleRate: state.sampleRate });
    fillWindow(0, true, message.requestId);
    return;
  }
  if (message.type === "request-window") {
    fillWindow(message.position, message.resetPosition, message.requestId);
  }
};
