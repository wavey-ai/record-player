"use strict";

import "./worklet-text-codec-polyfill.js";
import { createLogger, setPlayerLoggingEnabled } from "./player-message-logger.js";

const log = createLogger("audio-worklet");
import { initSync, ScratchAcousticDsp } from "./wasm/record-player/record_player.js";

let wasm = null;

const SEAM_REPAIR_TOTAL_SAMPLES = 24;
const SEAM_REPAIR_HALF_SAMPLES = SEAM_REPAIR_TOTAL_SAMPLES / 2;

function cubicHermite(y0, y1, m0, m1, t, span) {
  const t2 = t * t;
  const t3 = t2 * t;
  const h00 = (2 * t3) - (3 * t2) + 1;
  const h10 = t3 - (2 * t2) + t;
  const h01 = (-2 * t3) + (3 * t2);
  const h11 = t3 - t2;
  return (h00 * y0) + (h10 * span * m0) + (h01 * y1) + (h11 * span * m1);
}

function ensureDspWasm(module) {
  if (!wasm) {
    wasm = initSync({ module });
  }
  return wasm;
}

class BitneedlePlayerProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    setPlayerLoggingEnabled(options?.processorOptions?.loggingEnabled !== false);
    log.action("constructor", { sampleRate, loggingEnabled: options?.processorOptions?.loggingEnabled !== false });
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
    this.streamChannels = [];
    this.streamLength = 0;
    this.decodedLength = 0;
    this.streamComplete = false;
    this.waitingForData = false;
    this.replay = null;
    this.effects = { acoustic: true, surface: true };
    this.port.onmessage = event => { const message = event.data || {}; log.receive(message.type, message); this.handleMessage(message); };
  }

  send(message) { log.send(message?.type || "message", message); this.port.postMessage(message); }

  handleMessage(message) {
    if (message.type === "set-logging") { setPlayerLoggingEnabled(message.enabled); log.action("logging-changed", { enabled: message.enabled }); return; }
    switch (message.type) {
      case "load": {
        const channels = Array.isArray(message.channels)
          ? message.channels.map(buffer => new Float32Array(buffer))
          : [];
        if (!channels.length || !channels[0]?.length) break;
        this.length = channels[0].length;
        this.sourceSampleRate = Number.isFinite(message.sampleRate) && message.sampleRate > 0
          ? message.sampleRate
          : 48000;
        this.dsp.setWindow(channels, this.sourceSampleRate, 0, this.length, 0);
        this.dsp.setNeedleLifted(this.needleLifted);
        this.lastPosition = 0;
        this.send({ type: "loaded", length: this.length });
        break;
      }
      case "stream-init": {
        const channelCount = Math.max(1, Math.floor(Number(message.channels) || 2));
        this.streamLength = Math.max(1, Math.floor(Number(message.audioLength) || 1));
        this.sourceSampleRate = Number.isFinite(message.sampleRate) && message.sampleRate > 0
          ? message.sampleRate
          : 48000;
        this.streamChannels = Array.from({ length: channelCount }, () => new Float32Array(this.streamLength));
        this.length = 0;
        this.decodedLength = 0;
        this.streamComplete = false;
        this.waitingForData = false;
        this.lastPosition = 0;
        this.send({ type: "stream-initialised", length: this.streamLength });
        break;
      }
      case "append-pcm": {
        if (!this.streamChannels.length) break;
        const startFrame = Math.max(0, Math.floor(Number(message.startFrame) || 0));
        const endFrame = Math.min(this.streamLength, Math.max(startFrame, Math.floor(Number(message.endFrame) || startFrame)));
        const frameCount = endFrame - startFrame;
        const buffers = Array.isArray(message.channelBuffers) ? message.channelBuffers : [];
        for (let channelIndex = 0; channelIndex < this.streamChannels.length; channelIndex += 1) {
          const sourceBuffer = buffers[Math.min(channelIndex, buffers.length - 1)];
          if (!sourceBuffer) continue;
          const source = new Int16Array(sourceBuffer);
          const target = this.streamChannels[channelIndex];
          const limit = Math.min(frameCount, source.length);
          for (let frame = 0; frame < limit; frame += 1) {
            target[startFrame + frame] = source[frame] / 32768;
          }
        }
        const repairedChannels = this.repairChunkSeam(startFrame, endFrame);
        if (startFrame <= this.decodedLength) {
          this.decodedLength = Math.max(this.decodedLength, endFrame);
        }
        this.length = Math.max(1, this.decodedLength);
        try {
          this.dsp.setWindow(this.streamChannels, this.sourceSampleRate, 0, this.length, undefined);
          this.dsp.setNeedleLifted(this.needleLifted);
        } catch (error) {
          this.send({
            type: "worklet-error",
            stage: "append-pcm",
            message: error instanceof Error ? error.message : String(error)
          });
          break;
        }
        if (this.waitingForData && this.decodedLength > this.lastPosition + 1024) {
          this.waitingForData = false;
          this.active = this.playing;
          if (this.active) {
            this.dsp.start();
            this.dsp.setPosition(this.lastPosition, 0);
            this.applyTransport();
          }
        }
        this.send({
          type: "buffered",
          decodedLength: this.decodedLength,
          totalLength: this.streamLength,
          seamRepair: repairedChannels > 0
            ? { boundaryFrame: startFrame, channels: repairedChannels, samples: SEAM_REPAIR_TOTAL_SAMPLES }
            : null
        });
        break;
      }
      case "stream-complete":
        this.streamComplete = true;
        break;
      case "transport":
        this.motorRunning = Boolean(message.running);
        this.applyTransport();
        break;
      case "scratch-transport": {
        const handContact = Boolean(message.handContact);
        const motorRate = this.motorRunning
          ? (Number.isFinite(message.motorRate) ? message.motorRate : this.playbackRate)
          : 0;
        this.scratching = handContact;
        this.dsp.setTransport(handContact, motorRate, 0);
        break;
      }
      case "play": {
        const handoff = Boolean(message.handoff);
        const wasActive = this.active;
        this.playbackRate = Number.isFinite(message.rate) ? message.rate : 1;
        this.playing = true;
        this.active = true;
        if (!handoff || !wasActive) {
          this.dsp.start();
          this.dsp.setPosition(this.clampPosition(message.position), 0);
        }
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
        this.send({ type: "seeked", position, generation: message.generation });
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
      case "surface-asset": {
        const channels = Array.isArray(message.channels)
          ? message.channels.map(buffer => new Float32Array(buffer))
          : [];
        if (!channels.length || !channels[0]?.length) break;
        try {
          this.dsp.setSurfaceAsset(channels, Number(message.sampleRate) || sampleRate);
          log.action("surface-asset-loaded", { channels: channels.length, frames: channels[0].length });
        } catch (error) {
          this.send({ type: "worklet-error", stage: "surface-asset", message: error instanceof Error ? error.message : String(error) });
        }
        break;
      }
      case "surface-gain":
        this.dsp.setSurfaceGainMultiplier(Number(message.multiplier) || 1);
        break;
      case "surface-region": {
        const region = message.region === "deadwax" ? 1 : 0;
        if (message.action === "start") {
          this.dsp.startSurfaceRegion(region, Number(message.durationSeconds) || 0);
          log.state(message.region === "deadwax" ? "deadwax" : "lead-in", { durationSeconds: message.durationSeconds });
        } else {
          this.dsp.stopSurfaceRegion();
          log.state("surface-region-stopped", { region: message.region });
        }
        break;
      }
      case "needle-drop":
        this.dsp.triggerNeedleDrop();
        log.state("needle-drop", {});
        break;
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

  repairChunkSeam(boundaryFrame, endFrame) {
    if (!(boundaryFrame > 0) || boundaryFrame !== this.decodedLength) return 0;
    const leftAnchor = boundaryFrame - SEAM_REPAIR_HALF_SAMPLES - 1;
    const rightAnchor = boundaryFrame + SEAM_REPAIR_HALF_SAMPLES;
    if (leftAnchor < 1 || rightAnchor + 1 >= endFrame) return 0;
    const span = rightAnchor - leftAnchor;
    let repairedChannels = 0;
    let maxBeforeJump = 0;
    let maxAfterJump = 0;
    for (const channel of this.streamChannels) {
      if (!channel || rightAnchor + 1 >= channel.length) continue;
      const beforeJump = Math.abs(channel[boundaryFrame] - channel[boundaryFrame - 1]);
      const y0 = channel[leftAnchor];
      const y1 = channel[rightAnchor];
      const m0 = y0 - channel[leftAnchor - 1];
      const m1 = channel[rightAnchor + 1] - y1;
      for (let index = leftAnchor + 1; index < rightAnchor; index += 1) {
        const t = (index - leftAnchor) / span;
        const value = cubicHermite(y0, y1, m0, m1, t, span);
        channel[index] = Math.max(-1, Math.min(1, value));
      }
      maxBeforeJump = Math.max(maxBeforeJump, beforeJump);
      maxAfterJump = Math.max(
        maxAfterJump,
        Math.abs(channel[boundaryFrame] - channel[boundaryFrame - 1])
      );
      repairedChannels += 1;
    }
    if (repairedChannels) {
      log.action("pcm-seam-repaired", {
        boundaryFrame,
        channels: repairedChannels,
        samples: SEAM_REPAIR_TOTAL_SAMPLES,
        maxBeforeJump,
        maxAfterJump
      });
    }
    return repairedChannels;
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
    this.send({ type: "scratch-replay-ended", id, cancelled, position: this.dsp.position });
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
      this.send({
        type: "position",
        position: this.lastPosition,
        effectiveRate: this.dsp.effectiveRate,
        scratching: this.scratching
      });
    }
    if (this.dsp.takeEnded()) {
      this.lastPosition = this.dsp.position;
      if (!this.streamComplete && this.decodedLength < this.streamLength) {
        this.waitingForData = true;
        this.active = false;
        this.dsp.stop();
        this.send({ type: "buffering", position: this.lastPosition });
      } else {
        this.playing = false;
        this.active = false;
        this.send({ type: "ended", position: this.dsp.position });
      }
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
