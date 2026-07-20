"use strict";

import {
  contiguousPcmEnd,
  copyPcmWindow,
  mergePcmWrittenRange,
  pcmRangeIsWritten,
  planPcmWindow,
  PCM_SEAM_HALF_SAMPLES,
  repairPcmSeam,
} from "./pcm-window-helpers.js";

const state = {
  sampleRate: 48000,
  totalFrames: 0,
  channelCount: 0,
  channels: [],
  banks: [],
  windowFrames: 0,
  nextBank: 0,
  generation: 0,
  availableEnd: 0,
  writtenRanges: [],
  seamBoundaries: new Set(),
};

function postWorkerError(stage, message, requestId) {
  postMessage({ type: "worker-error", stage, message, requestId });
}

function initialise(message) {
  state.sampleRate = Math.max(1, Number(message.sampleRate) || 48000);
  state.totalFrames = Math.max(1, Math.floor(Number(message.totalFrames) || 1));
  state.channelCount = Math.max(1, Math.min(2, Math.floor(Number(message.channelCount) || 2)));
  state.windowFrames = Math.max(4096, Math.min(
    state.totalFrames,
    Math.floor(Number(message.windowFrames) || state.sampleRate * 12),
  ));
  const banks = (message.bankBuffers || []).map(bankBuffers =>
    bankBuffers.map(buffer => new Float32Array(buffer))
  );
  state.banks = banks.length > 0 && banks.every(bank => (
    bank.length === state.channelCount
    && bank.every(channel => channel.length >= state.windowFrames)
  )) ? banks : [];
  state.channels = Array.from({ length: state.channelCount }, () => new Int16Array(state.totalFrames));
  state.nextBank = 0;
  state.generation = 0;
  state.availableEnd = 0;
  state.writtenRanges = [];
  state.seamBoundaries = new Set();
}

function repairAvailableSeams(updatedStart, updatedEnd) {
  const repairs = [];
  const radius = PCM_SEAM_HALF_SAMPLES + 2;
  for (const boundary of state.seamBoundaries) {
    if (boundary + radius < updatedStart || boundary - radius > updatedEnd) continue;
    if (!pcmRangeIsWritten(state.writtenRanges, boundary - radius, boundary + radius)) continue;
    const repair = repairPcmSeam(state.channels, boundary);
    if (repair) {
      repairs.push(repair);
      state.seamBoundaries.delete(boundary);
    }
  }
  return repairs;
}

function appendSegment(segment) {
  const start = Math.floor(Number(segment.startFrame));
  const end = Math.floor(Number(segment.endFrame));
  const buffers = Array.isArray(segment.channelBuffers) ? segment.channelBuffers : [];
  const frameCount = end - start;
  const segmentValid = (
    Number.isInteger(start) &&
    Number.isInteger(end) &&
    start >= 0 &&
    end > start &&
    end <= state.totalFrames &&
    buffers.length === state.channelCount &&
    buffers.every(buffer => buffer && new Int16Array(buffer).length === frameCount)
  );
  if (!segmentValid) {
    throw new Error(
      `Rejected malformed PCM segment startFrame=${segment?.startFrame} endFrame=${segment?.endFrame} buffers=${buffers.length} expectedChannels=${state.channelCount}`,
    );
  }
  for (let channel = 0; channel < state.channelCount; channel += 1) {
    state.channels[channel].set(new Int16Array(buffers[channel]), start);
  }
  if (start > 0 && segment.workletSeamRepair !== false) {
    state.seamBoundaries.add(start);
  } else if (segment.workletSeamRepair === false) {
    state.seamBoundaries.delete(start);
  }
  mergePcmWrittenRange(state.writtenRanges, start, end);
  state.availableEnd = contiguousPcmEnd(state.writtenRanges);
  return repairAvailableSeams(start, end);
}

function fillWindow(position, resetPosition, requestId, workletRequestId = 0) {
  const plan = planPcmWindow({
    position,
    totalFrames: state.totalFrames,
    availableEnd: state.availableEnd,
    windowFrames: state.windowFrames,
  });
  if (!plan) {
    postMessage({
      type: "window-unavailable",
      requestId,
      position: Math.max(0, Number(position) || 0),
      availableStart: 0,
      availableEnd: state.availableEnd,
      totalFrames: state.totalFrames,
      workletRequestId: Math.max(0, Math.floor(Number(workletRequestId) || 0)),
    });
    return;
  }

  let bankIndex = -1;
  let channelBuffers;
  let transfer = [];
  if (state.banks.length) {
    bankIndex = state.nextBank;
    state.nextBank = (state.nextBank + 1) % state.banks.length;
    const bank = state.banks[bankIndex];
    copyPcmWindow(state.channels, plan, bank);
    channelBuffers = [];
  } else {
    const channels = copyPcmWindow(state.channels, plan);
    channelBuffers = channels.map(channel => channel.buffer);
    transfer = channelBuffers;
  }

  state.generation += 1;
  postMessage({
    type: "window-ready",
    bankIndex,
    channelBuffers,
    start: plan.start,
    length: plan.length,
    availableStart: 0,
    availableEnd: state.availableEnd,
    totalFrames: state.totalFrames,
    sampleRate: state.sampleRate,
    generation: state.generation,
    resetPosition: Boolean(resetPosition),
    position: plan.position,
    requestId,
    workletRequestId: Math.max(0, Math.floor(Number(workletRequestId) || 0)),
  }, transfer);
}

self.onmessage = event => {
  const message = event.data || {};
  try {
    if (message.type === "init-progressive") {
      initialise(message);
      postMessage({
        type: "initialised",
        totalFrames: state.totalFrames,
        sampleRate: state.sampleRate,
        channelCount: state.channelCount,
        windowFrames: state.windowFrames,
        shared: state.banks.length > 0,
      });
      return;
    }
    if (message.type === "append-segments") {
      const segments = Array.isArray(message.segments) ? message.segments : [];
      const repairs = [];
      for (const segment of segments) repairs.push(...appendSegment(segment));
      postMessage({
        type: "availability",
        availableStart: 0,
        availableEnd: state.availableEnd,
        totalFrames: state.totalFrames,
        seamRepairs: repairs,
      });
      return;
    }
    if (message.type === "request-window") {
      fillWindow(
        message.position,
        message.resetPosition,
        message.requestId,
        message.workletRequestId,
      );
      return;
    }
    if (message.type === "reset") {
      initialise({ sampleRate: 48000, totalFrames: 1, channelCount: 1, windowFrames: 4096 });
    }
  } catch (error) {
    postWorkerError(message.type || "message", error instanceof Error ? error.message : String(error), message.requestId);
  }
};
