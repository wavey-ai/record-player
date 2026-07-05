"use strict";

import "./worklet-text-codec-polyfill.js";
import { initSync, ScratchAcousticDsp } from "./wasm/record-player-simulation/record_player_simulation_wasm.js";

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
    this.streamChannels = [];
    this.streamLength = 0;
    this.decodedLength = 0;
    this.streamComplete = false;
    this.waitingForData = false;
    this.port.onmessage = event => this.handleMessage(event.data || {});
  }

  handleMessage(message) {
    switch (message.type) {
      case "load": {
        const channels = Array.isArray(message.channels)
          ? message.channels.map(buffer => new Float32Array(buffer))
          : [];
        if (!channels.length || !channels[0]?.length) {
          return;
        }
        this.length = channels[0].length;
        this.sourceSampleRate = Number.isFinite(message.sampleRate) && message.sampleRate > 0
          ? message.sampleRate
          : 48000;
        this.dsp.setWindow(channels, this.sourceSampleRate, 0, this.length, 0);
        this.dsp.setNeedleLifted(this.needleLifted);
        this.lastPosition = 0;
        this.port.postMessage({ type: "loaded", length: this.length });
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
        this.port.postMessage({ type: "stream-initialised", length: this.streamLength });
        break;
      }
      case "append-pcm": {
        if (!this.streamChannels.length) {
          return;
        }
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
        if (startFrame <= this.decodedLength) {
          this.decodedLength = Math.max(this.decodedLength, endFrame);
        }
        this.length = Math.max(1, this.decodedLength);
        this.dsp.setWindow(this.streamChannels, this.sourceSampleRate, 0, this.length, 0);
        this.dsp.setNeedleLifted(this.needleLifted);
        if (this.waitingForData && this.decodedLength > this.lastPosition + 1024) {
          this.waitingForData = false;
          this.active = this.playing;
          if (this.active) {
            this.dsp.start();
            this.dsp.setPosition(this.lastPosition, 0);
            this.applyTransport();
          }
        }
        this.port.postMessage({ type: "buffered", decodedLength: this.decodedLength, totalLength: this.streamLength });
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

  copyOutput(outputs, frameCount) {
    const output = outputs[0];
    const channelCount = output.length;
    const interleaved = new Float32Array(wasm.memory.buffer, this.dsp.outputPtr, this.dsp.outputLen);
    for (let frame = 0; frame < frameCount; frame += 1) {
      for (let channel = 0; channel < channelCount; channel += 1) {
        output[channel][frame] = interleaved[frame * channelCount + channel] || 0;
      }
    }
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
    if (this.dsp.takeEnded()) {
      this.lastPosition = this.dsp.position;
      if (!this.streamComplete && this.decodedLength < this.streamLength) {
        this.waitingForData = true;
        this.active = false;
        this.dsp.stop();
        this.port.postMessage({ type: "buffering", position: this.lastPosition });
      } else {
        this.playing = false;
        this.active = false;
        this.port.postMessage({ type: "ended", position: this.dsp.position });
      }
    }
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output?.length) {
      return true;
    }
    const frameCount = output[0]?.length || 0;
    if (!this.active && !this.scratching) {
      this.dsp.stop();
    }
    this.dsp.render(frameCount, output.length);
    this.copyOutput(outputs, frameCount);
    this.publishState(frameCount);
    return true;
  }
}

registerProcessor("bitneedle-player", BitneedlePlayerProcessor);
