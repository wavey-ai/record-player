"use strict";

import "./worklet-text-codec-polyfill.js";
import { initSync, ScratchAcousticDsp } from "./wasm/record-player/record_player.js";

let wasm = null;

function ensureDspWasm(module) {
  if (!wasm) {
    wasm = initSync({ module });
  }
  return wasm;
}

class BitneedlePlayerProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    ensureDspWasm(options?.processorOptions?.wasmModule);
    this.dsp = new ScratchAcousticDsp(sampleRate, options?.processorOptions?.acousticConfig ?? undefined);
    this.active = false;
    this.playing = false;
    this.motorRunning = false;
    this.needleLifted = true;
    this.scratching = false;
    this.playbackRate = 1;
    this.length = 0;
    this.sourceSampleRate = 48000;
    this.reportCounter = 0;
    this.lastPosition = 0;
    this.sharedBanks = [];
    this.sharedWindowFrames = 0;
    this.activeBankGeneration = 0;
    this.windowRequestPending = false;
    this.replay = null;
    this.effects = { acoustic: true, surface: true };
    this.port.onmessage = event => this.handleMessage(event.data || {});
  }

  handleMessage(message) {
    switch (message.type) {
      case "shared-window-init": {
        this.sourceSampleRate = Math.max(1, Number(message.sampleRate) || 48000);
        this.length = Math.max(1, Math.floor(Number(message.totalFrames) || 1));
        this.sharedWindowFrames = Math.max(1, Math.floor(Number(message.windowFrames) || 1));
        this.sharedBanks = (message.bankBuffers || []).map(bankBuffers =>
          bankBuffers.map(buffer => new Float32Array(buffer))
        );
        this.port.postMessage({ type: "shared-window-initialised" });
        break;
      }
      case "shared-window-ready": {
        const generation = Math.max(0, Math.floor(Number(message.generation) || 0));
        if (generation < this.activeBankGeneration) break;
        const bank = this.sharedBanks[Math.max(0, Math.floor(Number(message.bankIndex) || 0))];
        const length = Math.max(1, Math.min(this.sharedWindowFrames, Math.floor(Number(message.length) || 1)));
        if (!bank?.length) break;
        const channels = bank.map(channel => channel.subarray(0, length));
        this.dsp.setWindow(
          channels,
          this.sourceSampleRate,
          Math.max(0, Math.floor(Number(message.start) || 0)),
          this.length,
          message.resetPosition ? Number(message.position) || 0 : undefined
        );
        this.dsp.setNeedleLifted(this.needleLifted);
        this.activeBankGeneration = generation;
        this.windowRequestPending = false;
        this.port.postMessage({ type: "window-activated", generation });
        break;
      }
      case "transport":
        this.motorRunning = Boolean(message.running);
        this.applyTransport();
        break;
      case "scratch-transport": {
        const handContact = Boolean(message.handContact);
        const motorRate = Number.isFinite(message.motorRate) ? message.motorRate : 0;
        this.scratching = handContact;
        this.motorRunning = motorRate !== 0;
        this.dsp.setTransport(handContact, motorRate, 0);
        break;
      }
      case "play": {
        const handoff = Boolean(message.handoff);
        const wasActive = this.active;
        this.playbackRate = Number.isFinite(message.rate) ? message.rate : 1;
        this.playing = true;
        this.active = true;
        if (!handoff || !wasActive) this.dsp.start();
        this.dsp.setPosition(this.clampPosition(message.position), 0);
        this.dsp.setNeedleLifted(this.needleLifted);
        this.applyTransport();
        break;
      }
      case "stop": {
        const handoff = Boolean(message.handoff);
        this.playing = false;
        if (!handoff) {
          this.active = false;
          this.dsp.stop();
        }
        break;
      }
      case "seek": {
        const position = this.clampPosition(message.position);
        this.dsp.setPosition(position, Number(message.impulse) || 0);
        this.lastPosition = position;
        this.port.postMessage({ type: "seeked", position, generation: message.generation });
        break;
      }
      case "needle":
        this.needleLifted = Boolean(message.lifted);
        this.dsp.setNeedleLifted(this.needleLifted);
        break;
      case "scratch": {
        const active = Boolean(message.active);
        const position = this.clampPosition(message.position);
        const rate = Number.isFinite(message.rate) ? message.rate : 0;
        const impulse = Number.isFinite(message.impulse) ? message.impulse : 0;
        const wasScratching = this.scratching;
        this.scratching = active;
        if (active) {
          if (!this.active) {
            this.active = true;
            this.dsp.start();
            this.dsp.setPosition(position, 0);
            this.dsp.setNeedleLifted(this.needleLifted);
          }
          this.dsp.setTransport(true, this.motorRate(), rate);
          this.dsp.setMotion(position, rate, wasScratching ? impulse : Math.max(impulse, 0.22));
        } else {
          this.dsp.setTransport(false, this.motorRate(), 0);
          if (!this.playing) {
            this.active = false;
            this.dsp.stop();
          }
        }
        break;
      }
      case "motion": {
        const position = this.clampPosition(message.position);
        const rate = Number.isFinite(message.rate) ? message.rate : 0;
        const impulse = Number.isFinite(message.impulse) ? message.impulse : 0;
        this.scratching = true;
        this.active = true;
        this.dsp.setTransport(true, this.motorRate(), rate);
        this.dsp.setMotion(position, rate, impulse);
        break;
      }
      case "set-effects": {
        this.effects = {
          acoustic: message.acoustic !== false,
          surface: message.surface !== false
        };
        this.dsp.setEffects(this.effects.acoustic, this.effects.surface);
        break;
      }
      case "replay-scratch": {
        const performance = message.performance || {};
        const events = Array.isArray(performance.events)
          ? performance.events
              .map(event => ({ ...event, frameOffset: Math.max(0, Math.floor(Number(event.frameOffset) || 0)) }))
              .sort((a, b) => a.frameOffset - b.frameOffset)
          : [];
        const mode = message.effectsMode || "original";
        const requested = typeof mode === "object" && mode
          ? { acoustic: mode.acoustic !== false, surface: mode.surface !== false }
          : mode === "dry"
            ? { acoustic: false, surface: false }
            : { acoustic: true, surface: true };
        const initial = performance.initialState || {};
        this.replay = {
          id: message.id,
          events,
          index: 0,
          startFrame: currentFrame + 128,
          restoreEffects: { ...this.effects },
          requestedEffects: requested
        };
        this.effects = requested;
        this.dsp.setEffects(requested.acoustic, requested.surface);
        const position = this.clampPosition(Number(initial.positionFrames) || 0);
        this.playbackRate = Number.isFinite(initial.playbackRate) ? initial.playbackRate : this.playbackRate;
        this.motorRunning = initial.motorRunning !== false;
        this.playing = Boolean(initial.playing);
        this.active = true;
        this.dsp.start();
        this.dsp.setPosition(position, 0);
        this.dsp.setNeedleLifted(false);
        this.dsp.setTransport(false, this.motorRate(), 0);
        break;
      }
      case "cancel-scratch-replay":
        this.finishReplay(true);
        break;
      default:
        break;
    }
  }

  motorRate() {
    return this.motorRunning ? this.playbackRate : 0;
  }

  applyTransport() {
    this.dsp.setTransport(this.scratching, this.motorRate(), 0);
  }

  clampPosition(position) {
    if (!Number.isFinite(position) || this.length === 0) {
      return 0;
    }
    return Math.max(0, Math.min(this.length - 1, position));
  }

  copyOutput(outputs, outputOffset, frameCount) {
    const output = outputs[0];
    const channelCount = output.length;
    const interleaved = new Float32Array(wasm.memory.buffer, this.dsp.outputPtr, this.dsp.outputLen);
    for (let frame = 0; frame < frameCount; frame += 1) {
      for (let channel = 0; channel < channelCount; channel += 1) {
        output[channel][outputOffset + frame] = interleaved[frame * channelCount + channel] || 0;
      }
    }
  }

  applyReplayEvent(event) {
    const position = this.clampPosition(event.positionFrames);
    const rate = Number.isFinite(event.rate) ? event.rate : 0;
    const impulse = Number.isFinite(event.impulse) ? event.impulse : 0;
    if (event.type === "scratch-start") {
      this.scratching = true;
      this.active = true;
      this.dsp.setTransport(true, this.motorRate(), rate);
      this.dsp.setMotion(position, rate, impulse);
    } else if (event.type === "scratch-motion") {
      this.scratching = true;
      this.active = true;
      this.dsp.setTransport(true, this.motorRate(), rate);
      this.dsp.setMotion(position, rate, impulse);
    } else if (event.type === "scratch-end") {
      this.scratching = false;
      this.playing = event.resumePlayback !== false;
      this.dsp.setTransport(false, this.motorRate(), 0);
      if (!this.playing) {
        this.active = false;
        this.dsp.stop();
      }
    }
  }

  finishReplay(cancelled = false) {
    if (!this.replay) return;
    const { id, restoreEffects } = this.replay;
    this.replay = null;
    this.effects = restoreEffects;
    this.dsp.setEffects(restoreEffects.acoustic, restoreEffects.surface);
    this.port.postMessage({ type: "scratch-replay-ended", id, cancelled, position: this.dsp.position });
  }

  renderRange(outputs, outputOffset, frameCount) {
    if (frameCount <= 0) return;
    this.dsp.render(frameCount, outputs[0].length);
    this.copyOutput(outputs, outputOffset, frameCount);
  }

  publishState(frameCount) {
    this.reportCounter += frameCount;
    if (this.reportCounter >= 1024) {
      this.reportCounter = 0;
      this.lastPosition = this.dsp.position;
      this.port.postMessage({
        type: "position",
        position: this.lastPosition,
        effectiveRate: this.dsp.effectiveRate,
        scratching: this.scratching
      });
    }
    const requestedWindowPosition = this.dsp.takeWindowRequest();
    if (requestedWindowPosition >= 0 && !this.windowRequestPending) {
      this.windowRequestPending = true;
      this.port.postMessage({ type: "window-request", position: requestedWindowPosition });
    }
    if (this.dsp.takeEnded()) {
      this.lastPosition = this.dsp.position;
      this.playing = false;
      this.active = false;
      this.port.postMessage({ type: "ended", position: this.dsp.position });
    }
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output?.length) return true;
    const frameCount = output[0]?.length || 0;
    if (!this.active && !this.scratching) this.dsp.stop();

    let offset = 0;
    const quantumStart = currentFrame;
    const quantumEnd = quantumStart + frameCount;
    while (this.replay && this.replay.index < this.replay.events.length) {
      const event = this.replay.events[this.replay.index];
      const eventFrame = this.replay.startFrame + event.frameOffset;
      if (eventFrame >= quantumEnd) break;
      const eventOffset = Math.max(offset, Math.min(frameCount, eventFrame - quantumStart));
      this.renderRange(outputs, offset, eventOffset - offset);
      this.applyReplayEvent(event);
      this.replay.index += 1;
      offset = eventOffset;
    }
    this.renderRange(outputs, offset, frameCount - offset);
    if (this.replay && this.replay.index >= this.replay.events.length) {
      const last = this.replay.events[this.replay.events.length - 1];
      const finishFrame = this.replay.startFrame + (last?.frameOffset || 0);
      if (quantumEnd > finishFrame) this.finishReplay(false);
    }
    this.publishState(frameCount);
    return true;
  }
}

registerProcessor("bitneedle-player", BitneedlePlayerProcessor);
