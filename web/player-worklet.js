"use strict";

import "./worklet-text-codec-polyfill.js";
import { createLogger, isPlayerVerboseLoggingEnabled, setPlayerLoggingEnabled } from "./player-message-logger.js";

const rawLog = createLogger("audio-worklet");
const log = {
  send(type, payload, meta) { return isPlayerVerboseLoggingEnabled() ? rawLog.send(type, payload, meta) : null; },
  receive(type, payload, meta) { return isPlayerVerboseLoggingEnabled() ? rawLog.receive(type, payload, meta) : null; },
  action(type, payload, meta) { return isPlayerVerboseLoggingEnabled() ? rawLog.action(type, payload, meta) : null; },
  state(type, payload, meta) { return isPlayerVerboseLoggingEnabled() ? rawLog.state(type, payload, meta) : null; },
  warn(type, payload, meta) { return rawLog.warn(type, payload, meta); },
  error(type, payload, meta) { return rawLog.error(type, payload, meta); }
};
import { initSync, ScratchAcousticDsp } from "./record-player/record_player.js";

let wasm = null;
const SCRATCH_PRESETS = new Set(["baby", "stab", "chirp", "transform", "flare", "crab", "orbit", "drum"]);
const REPLAY_LOCKED_MESSAGE_TYPES = new Set([
  "transport",
  "scratch-preset",
  "scratch-clicks",
  "native-rpm",
  "end-behavior",
  "hf-acceleration-limit",
  "stylus-tracing-limit",
  "output-gain",
  "scratch-transport",
  "play",
  "stop",
  "seek",
  "needle",
  "scratch",
  "motion",
  "surface-region",
  "needle-drop",
  "set-effects",
  "window-transport-init",
]);
const REPLAY_POSITION_REPLACING_MESSAGE_TYPES = new Set([
  "play",
  "stop",
  "seek",
  "scratch",
  "motion",
]);

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
    this.scratchPreset = this.dsp.scratchPreset;
    this.scratchClicks = this.dsp.scratchClicks;
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
    this.windowBanks = [];
    this.windowChannelCount = 0;
    this.windowFrames = 0;
    this.windowStart = 0;
    this.windowEnd = 0;
    this.activeWindowGeneration = 0;
    this.windowRequestSerial = 0;
    this.ignoreResetThroughWindowRequestId = 0;
    this.replayRestoreWindowPosition = null;
    this.windowRequestPending = false;
    this.pendingWindowRequest = null;
    this.queuedWindowRequest = null;
    this.desiredResetPosition = null;
    this.waitingWindowPosition = null;
    this.streamLength = 0;
    this.decodedLength = 0;
    this.streamComplete = false;
    this.streamGeneration = 0;
    this.playbackEpoch = 0;
    this.cleanEnd = true;
    this.deadwaxTurns = 2;
    this.waitingForData = false;
    this.replay = null;
    this.effects = { acoustic: true, surface: true };
    this.outputGain = 1;
    this.surfaceRegionActive = false;
    this.surfaceRegion = null;
    this.lastInputTiming = null;
    this.port.onmessage = event => { const message = event.data || {}; log.receive(message.type, message); this.handleMessage(message); };
  }

  send(message) {
    const payload = { streamGeneration: this.streamGeneration, ...message };
    log.send(payload?.type || "message", payload);
    this.port.postMessage(payload);
  }

  handleMessage(message) {
    if (message.type === "set-logging") { setPlayerLoggingEnabled(message.enabled); log.action("logging-changed", { enabled: message.enabled }); return; }
    // Replay is a sample-accurate transaction backed by a Rust DSP snapshot.
    // Restore that snapshot before applying a live mutation so the new command
    // wins, including for delayed or third-party messages sent to the port.
    if (this.replay && REPLAY_LOCKED_MESSAGE_TYPES.has(message.type)) {
      log.state("replay-interrupted-by-control", { type: message.type, replayId: this.replay.id });
      this.finishReplay(true, currentFrame, {
        requestWindow: message.type !== "window-transport-init",
      });
      if (REPLAY_POSITION_REPLACING_MESSAGE_TYPES.has(message.type)) {
        this.replayRestoreWindowPosition = null;
      }
    }
    switch (message.type) {
      case "window-transport-init": {
        this.streamGeneration = Math.max(0, Math.floor(Number(message.streamGeneration) || 0));
        this.active = false;
        this.playing = false;
        this.scratching = false;
        this.waitingForData = false;
        this.windowRequestPending = false;
        this.pendingWindowRequest = null;
        this.queuedWindowRequest = null;
        this.desiredResetPosition = null;
        this.waitingWindowPosition = null;
        this.streamLength = Math.max(1, Math.floor(Number(message.totalFrames) || 1));
        this.length = this.streamLength;
        this.decodedLength = 0;
        this.streamComplete = false;
        this.sourceSampleRate = Number.isFinite(message.sampleRate) && message.sampleRate > 0
          ? message.sampleRate
          : 48000;
        this.windowChannelCount = Math.max(1, Math.min(2, Math.floor(Number(message.channelCount) || 2)));
        this.windowFrames = Math.max(1, Math.floor(Number(message.windowFrames) || 1));
        this.windowBanks = (message.bankBuffers || []).map(bankBuffers =>
          bankBuffers.map(buffer => new Float32Array(buffer))
        );
        this.windowStart = 0;
        this.windowEnd = 0;
        this.activeWindowGeneration = 0;
        this.windowRequestSerial = 0;
        this.ignoreResetThroughWindowRequestId = 0;
        this.replayRestoreWindowPosition = null;
        this.lastPosition = 0;
        this.dsp.stop();
        this.dsp.clearWindow();
        this.send({ type: "window-transport-initialised", length: this.streamLength, shared: this.windowBanks.length > 0 });
        break;
      }
      case "window-ready":
        this.applyPcmWindow(message);
        break;
      case "window-unavailable":
        this.windowRequestPending = false;
        this.pendingWindowRequest = null;
        this.decodedLength = Math.max(this.decodedLength, Math.floor(Number(message.availableEnd) || 0));
        if (
          this.replayRestoreWindowPosition != null
          && this.windowContainsPosition(this.replayRestoreWindowPosition)
        ) {
          this.replayRestoreWindowPosition = null;
          this.queuedWindowRequest = null;
        } else if (this.replayRestoreWindowPosition != null) {
          this.flushQueuedWindowRequest();
        }
        break;
      case "stream-availability":
        this.decodedLength = Math.max(this.decodedLength, Math.floor(Number(message.decodedLength) || 0));
        this.streamLength = Math.max(this.streamLength, Math.floor(Number(message.totalLength) || 0));
        this.length = this.streamLength;
        if (this.waitingForData && this.waitingWindowPosition <= this.decodedLength - 2) {
          this.queuedWindowRequest = null;
          this.requestWindow(this.waitingWindowPosition, this.desiredResetPosition != null);
        } else if (!this.windowRequestPending) {
          this.flushQueuedWindowRequest();
        }
        break;
      case "stream-complete":
        this.streamComplete = true;
        break;
      case "window-transport-reset":
      case "reset":
        this.resetPcmWindowTransport();
        break;
      case "transport":
        this.motorRunning = Boolean(message.running);
        if (!this.motorRunning) this.clearAutomaticSurfaceRegion("motor-off");
        this.applyTransport();
        break;
      case "scratch-preset":
        this.applyDspControl(message.type, () => {
          this.dsp.setScratchPreset(String(message.preset || ""));
          this.scratchPreset = this.dsp.scratchPreset;
          this.scratchClicks = this.dsp.scratchClicks;
        });
        break;
      case "scratch-clicks":
        this.applyDspControl(message.type, () => {
          this.scratchClicks = this.normalizeScratchClicks(message.clicks, this.scratchClicks);
          this.dsp.setScratchClicks(this.scratchClicks);
        });
        break;
      case "native-rpm":
        this.applyDspControl(message.type, () => {
          this.dsp.setNativeRpm(Number(message.rpm ?? message.nativeRpm));
        });
        break;
      case "end-behavior":
        this.cleanEnd = message.cleanEnd !== false;
        this.deadwaxTurns = Number.isFinite(message.deadwaxTurns) && message.deadwaxTurns > 0
          ? message.deadwaxTurns
          : 2;
        break;
      case "hf-acceleration-limit":
        this.applyDspControl(message.type, () => {
          this.dsp.setHighFrequencyAccelerationLimit(Number(message.strength ?? message.value));
        });
        break;
      case "stylus-tracing-limit":
        this.applyDspControl(message.type, () => {
          this.dsp.setStylusTracingLimit(Number(message.strength ?? message.value));
        });
        break;
      case "output-gain": {
        const gain = Number(message.gain);
        if (this.applyDspControl(message.type, () => {
          this.dsp.setOutputGain(
            gain,
            Math.max(0, Number(message.rampMs) || 0),
          );
        })) this.outputGain = gain;
        break;
      }
      case "scratch-transport": {
        const handContact = Boolean(message.handContact);
        if (handContact) this.clearAutomaticSurfaceRegion("scratch-transport");
        const motorRate = this.motorRunning
          ? (Number.isFinite(message.motorRate) ? message.motorRate : this.playbackRate)
          : 0;
        this.scratching = handContact;
        this.dsp.setTransport(handContact, motorRate, 0);
        break;
      }
      case "play": {
        const interruptedAutomaticSurface = this.clearAutomaticSurfaceRegion("play");
        this.playbackEpoch = Math.max(0, Math.floor(Number(message.playbackEpoch) || 0));
        const handoff = Boolean(message.handoff);
        const wasActive = this.active && !interruptedAutomaticSurface;
        const position = this.clampPosition(message.position);
        const resetPosition = !handoff || !wasActive;
        this.playbackRate = Number.isFinite(message.rate) ? message.rate : 1;
        this.playing = true;
        this.active = true;
        if (resetPosition) {
          this.dsp.start();
          this.dsp.setPosition(position, 0);
          this.lastPosition = position;
        }
        this.dsp.setNeedleLifted(this.needleLifted);
        this.applyTransport();
        this.ensureWindowForPosition(position, { resetPosition });
        break;
      }
      case "stop": {
        this.playbackEpoch = Math.max(0, Math.floor(Number(message.playbackEpoch) || 0));
        const handoff = Boolean(message.handoff);
        this.playing = false;
        if (!handoff) {
          this.clearAutomaticSurfaceRegion("stop");
          this.active = false;
          this.waitingForData = false;
          this.waitingWindowPosition = null;
          this.desiredResetPosition = null;
          this.dsp.stop();
        }
        break;
      }
      case "seek": {
        this.clearAutomaticSurfaceRegion("seek");
        const position = this.clampPosition(message.position);
        this.dsp.setPosition(position, Number(message.impulse) || 0);
        this.lastPosition = position;
        this.ensureWindowForPosition(position, { resetPosition: true });
        this.send({ type: "seeked", position, generation: message.generation });
        break;
      }
      case "needle":
        this.needleLifted = Boolean(message.lifted);
        this.dsp.setNeedleLifted(this.needleLifted);
        if (this.needleLifted && this.surfaceRegionActive) this.stopSurfaceRegion();
        break;
      case "scratch": {
        this.captureInputTiming(message);
        const active = Boolean(message.active);
        if (active) this.clearAutomaticSurfaceRegion("scratch");
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
          this.ensureWindowForPosition(position);
        } else {
          this.dsp.setTransport(false, this.motorRate(), 0);
          if (!this.playing && !this.motorRunning) {
            this.active = false;
            this.dsp.stop();
          } else if (this.motorRunning) {
            this.active = true;
          }
        }
        break;
      }
      case "motion": {
        this.captureInputTiming(message);
        this.clearAutomaticSurfaceRegion("motion");
        const position = this.clampPosition(message.position);
        const rate = Number.isFinite(message.rate) ? message.rate : 0;
        const impulse = Number.isFinite(message.impulse) ? message.impulse : 0;
        this.scratching = true;
        this.active = true;
        this.dsp.setTransport(true, this.motorRate(), rate);
        this.dsp.setMotion(position, rate, impulse);
        this.ensureWindowForPosition(position);
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
        if (message.action === "start") {
          this.startSurfaceRegion(message);
        } else {
          this.stopSurfaceRegion(message.region);
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
        if (this.surfaceRegionActive && this.surfaceRegion?.automatic) {
          this.send({
            type: "scratch-replay-ended",
            id: message.id,
            cancelled: true,
            position: this.dsp.position,
            outputFrame: Math.max(0, Number(currentFrame) || 0),
            currentFrame: Math.max(0, Number(currentFrame) || 0),
          });
          break;
        }
        if (this.replay) this.finishReplay(true);
        const performance = message.performance || {};
        // The host has already validated, normalized and stably sorted this
        // array. Re-sorting and cloning up to a million events on the realtime
        // AudioWorklet thread can miss many render deadlines.
        const events = Array.isArray(performance.events) ? performance.events : [];
        const mode = message.effectsMode || "original";
        const recordedEffects = performance.effects && typeof performance.effects === "object"
          ? {
            acoustic: performance.effects.acoustic !== false,
            surface: performance.effects.surface !== false,
          }
          : { acoustic: true, surface: true };
        const requested = typeof mode === "object" && mode
          ? { acoustic: mode.acoustic !== false, surface: mode.surface !== false }
          : mode === "dry"
            ? { acoustic: false, surface: false }
            : recordedEffects;
        const initial = performance.initialState || {};
        const restoreState = this.snapshotReplayState();
        this.dsp.captureReplayState();
        this.replay = {
          id: message.id,
          events,
          index: 0,
          durationFrames: Math.max(
            events.length ? events[events.length - 1].frameOffset : 0,
            Math.max(0, Math.floor(Number(performance.durationFrames) || 0)),
          ),
          startFrame: currentFrame,
          pausedAtFrame: null,
          restoreState,
          requestedEffects: requested
        };
        this.applyDspControl("replay-manual-crossfader", () => {
          this.dsp.setManualCrossfader(
            initial.manualCrossfader ?? initial.crossfader ?? 0.5,
          );
        });
        this.applyDspControl("replay-output-gain", () => {
          this.dsp.setOutputGain(Number(initial.volume ?? 1), 0);
        });
        this.effects = requested;
        this.dsp.setEffects(requested.acoustic, requested.surface);
        this.setScratchPresetAndClicks(initial.preset, initial.clicks);
        if (Number.isFinite(Number(initial.nativeRpm)) && Number(initial.nativeRpm) > 0) {
          this.applyDspControl("replay-native-rpm", () => this.dsp.setNativeRpm(Number(initial.nativeRpm)));
        }
        if (Number.isFinite(Number(initial.highFrequencyAccelerationLimit))) {
          this.applyDspControl("replay-hf-acceleration-limit", () => (
            this.dsp.setHighFrequencyAccelerationLimit(Number(initial.highFrequencyAccelerationLimit))
          ));
        }
        if (Number.isFinite(Number(initial.stylusTracingLimit))) {
          this.applyDspControl("replay-stylus-tracing-limit", () => (
            this.dsp.setStylusTracingLimit(Number(initial.stylusTracingLimit))
          ));
        }
        const position = this.clampPosition(Number(initial.positionFrames) || 0);
        this.playbackRate = Number.isFinite(initial.playbackRate) ? initial.playbackRate : this.playbackRate;
        this.motorRunning = Boolean(initial.motorRunning);
        this.playing = Boolean(initial.playing);
        this.needleLifted = Boolean(initial.needleLifted);
        this.active = true;
        this.dsp.start();
        this.dsp.setPosition(position, 0);
        this.dsp.setNeedleLifted(this.needleLifted);
        this.dsp.setTransport(false, this.motorRate(), 0);
        this.ensureWindowForPosition(position, { resetPosition: true, pauseFrame: currentFrame });
        break;
      }
      case "cancel-scratch-replay":
        this.finishReplay(true);
        break;
      default:
        break;
    }
  }

  applyDspControl(stage, apply) {
    try {
      apply();
      return true;
    } catch (error) {
      this.send({
        type: "worklet-error",
        stage,
        message: error instanceof Error ? error.message : String(error),
      });
      return false;
    }
  }

  normalizeScratchClicks(value, fallback = 1) {
    const number = Number(value);
    return Number.isFinite(number)
      ? Math.max(1, Math.min(8, Math.trunc(number)))
      : Math.max(1, Math.min(8, Math.trunc(Number(fallback) || 1)));
  }

  setScratchPresetAndClicks(preset, clicks) {
    const requested = String(preset || "baby").trim().toLowerCase();
    const normalized = SCRATCH_PRESETS.has(requested) ? requested : "baby";
    this.applyDspControl("replay-scratch-preset", () => {
      this.dsp.setScratchPreset(normalized);
      this.scratchPreset = this.dsp.scratchPreset;
      this.scratchClicks = this.dsp.scratchClicks;
    });
    if (clicks !== undefined && clicks !== null) {
      this.scratchClicks = this.normalizeScratchClicks(clicks, this.scratchClicks);
      this.dsp.setScratchClicks(this.scratchClicks);
    }
  }

  captureInputTiming(message) {
    const inputAudioTime = Number(message.inputAudioTime);
    const hasInputTime = Number.isFinite(inputAudioTime);
    const hasCommandId = message.commandId !== undefined && message.commandId !== null;
    if (!hasInputTime && !hasCommandId) return;
    const appliedFrame = Math.max(0, Number(currentFrame) || 0);
    const appliedAudioTime = Number.isFinite(Number(currentTime))
      ? Number(currentTime)
      : appliedFrame / sampleRate;
    this.lastInputTiming = {
      commandId: hasCommandId ? message.commandId : null,
      inputAudioTime: hasInputTime ? inputAudioTime : null,
      inputAppliedAudioTime: appliedAudioTime,
      inputLatencyMs: hasInputTime ? Math.max(0, (appliedAudioTime - inputAudioTime) * 1000) : null,
    };
  }

  snapshotReplayState() {
    return {
      effects: { ...this.effects },
      scratchPreset: this.scratchPreset,
      scratchClicks: this.scratchClicks,
      needleLifted: this.needleLifted,
      motorRunning: this.motorRunning,
      playbackRate: this.playbackRate,
      playing: this.playing,
      active: this.active,
      scratching: this.scratching,
      position: this.clampPosition(this.dsp.position),
      nativeRpm: this.dsp.nativeRpm,
      highFrequencyAccelerationLimit: this.dsp.highFrequencyAccelerationLimit,
      stylusTracingLimit: this.dsp.stylusTracingLimit,
      outputGain: this.outputGain,
    };
  }

  startSurfaceRegion(message, { preservePlatter = false } = {}) {
    if (this.replay) this.finishReplay(true);
    const region = message.region === "deadwax" ? "deadwax" : "lead_in";
    const regionId = Math.max(0, Math.floor(Number(message.regionId) || 0));
    const adoptingAutomaticDeadwax = (
      region === "deadwax"
      && this.surfaceRegionActive
      && this.surfaceRegion?.region === "deadwax"
      && this.surfaceRegion.automatic
      && !message.automatic
    );
    if (adoptingAutomaticDeadwax) {
      this.surfaceRegion.regionId = regionId;
      this.surfaceRegion.automatic = false;
      this.active = true;
      this.playing = false;
      this.scratching = false;
      this.dsp.setNeedleLifted(this.needleLifted);
      this.dsp.setTransport(false, this.motorRate(), 0);
      if (this.surfaceRegion.completed) {
        this.sendSurfaceRegionEnded(this.surfaceRegion, this.surfaceRegion.endFrame);
      }
      log.state("deadwax-adopted", {
        regionId,
        startFrame: this.surfaceRegion.startFrame,
        endFrame: this.surfaceRegion.endFrame,
      });
      return;
    }
    if (this.surfaceRegionActive) this.dsp.stopSurfaceRegion();
    const explicitDurationFrames = Number(message.durationFrames);
    const durationSeconds = Math.max(0, Number(message.durationSeconds) || 0);
    const durationFrames = Number.isFinite(explicitDurationFrames) && explicitDurationFrames >= 0
      ? Math.floor(explicitDurationFrames)
      : Math.max(0, Math.round(durationSeconds * sampleRate));
    const resolvedDurationSeconds = durationFrames / sampleRate;
    const explicitStartFrame = Number(message.startFrame);
    const startFrame = Number.isFinite(explicitStartFrame)
      ? Math.max(0, Math.floor(explicitStartFrame))
      : Math.max(0, Number(currentFrame) || 0);
    this.surfaceRegion = {
      region,
      regionId,
      startFrame,
      durationFrames,
      endFrame: startFrame + durationFrames,
      completed: false,
      automatic: Boolean(message.automatic),
    };
    this.surfaceRegionActive = true;
    this.waitingForData = false;
    this.waitingWindowPosition = null;
    this.desiredResetPosition = null;
    this.active = true;
    this.playing = false;
    this.scratching = false;
    if (!preservePlatter) this.dsp.start();
    this.dsp.setNeedleLifted(this.needleLifted);
    this.dsp.startSurfaceRegion(region === "deadwax" ? 1 : 0, resolvedDurationSeconds);
    this.dsp.setTransport(false, this.motorRate(), 0);
    log.state(region === "deadwax" ? "deadwax" : "lead-in", {
      durationSeconds: resolvedDurationSeconds,
      durationFrames,
      startFrame,
    });
    if (durationFrames === 0) this.completeSurfaceRegion(startFrame);
  }

  clearAutomaticSurfaceRegion(reason = "interrupted") {
    if (!this.surfaceRegionActive || !this.surfaceRegion?.automatic) return false;
    const region = this.surfaceRegion.region;
    this.surfaceRegionActive = false;
    this.surfaceRegion = null;
    this.dsp.stopSurfaceRegion();
    log.state("automatic-surface-interrupted", { region, reason });
    return true;
  }

  stopSurfaceRegion(region = this.surfaceRegion?.region) {
    const normalized = region === "deadwax" ? "deadwax" : "lead_in";
    this.surfaceRegionActive = false;
    this.surfaceRegion = null;
    this.dsp.stopSurfaceRegion();
    if (!this.playing && !this.scratching) {
      this.active = false;
      this.dsp.stop();
    }
    log.state("surface-region-stopped", { region: normalized });
  }

  completeSurfaceRegion(outputFrame) {
    const surface = this.surfaceRegion;
    if (!surface || surface.completed) return;
    surface.completed = true;
    const completionFrame = Math.max(surface.startFrame, Math.floor(Number(outputFrame) || surface.endFrame));
    this.sendSurfaceRegionEnded(surface, completionFrame);
    if (surface.region === "lead_in") {
      this.surfaceRegionActive = false;
      this.surfaceRegion = null;
      this.dsp.stopSurfaceRegion();
      this.playing = true;
      this.active = true;
      this.dsp.setTransport(false, this.motorRate(), 0);
    }
  }

  sendSurfaceRegionEnded(surface, completionFrame) {
    this.send({
      type: "surface-region-ended",
      region: surface.region,
      regionId: surface.regionId,
      durationFrames: surface.durationFrames,
      outputFrame: completionFrame,
      currentFrame: completionFrame,
    });
  }

  resetPcmWindowTransport() {
    if (this.replay) this.finishReplay(true, currentFrame, { requestWindow: false });
    this.active = false;
    this.playing = false;
    this.scratching = false;
    this.length = 0;
    this.streamLength = 0;
    this.decodedLength = 0;
    this.streamComplete = false;
    this.waitingForData = false;
    this.waitingWindowPosition = null;
    this.desiredResetPosition = null;
    this.windowBanks = [];
    this.windowChannelCount = 0;
    this.windowFrames = 0;
    this.windowStart = 0;
    this.windowEnd = 0;
    this.activeWindowGeneration = 0;
    this.windowRequestSerial = 0;
    this.ignoreResetThroughWindowRequestId = 0;
    this.replayRestoreWindowPosition = null;
    this.windowRequestPending = false;
    this.pendingWindowRequest = null;
    this.queuedWindowRequest = null;
    this.lastPosition = 0;
    this.surfaceRegionActive = false;
    this.surfaceRegion = null;
    this.lastInputTiming = null;
    this.applyDspControl("reset-manual-fader", () => this.dsp.setManualFaderGain(1));
    this.dsp.stopSurfaceRegion();
    this.dsp.stop();
    this.dsp.clearWindow();
  }

  windowContainsPosition(position, start = this.windowStart, end = this.windowEnd) {
    const value = Number(position);
    return Number.isFinite(value) && end - start >= 2 && value >= start && value <= end - 2;
  }

  requestWindow(position, resetPosition = false) {
    if (this.streamLength <= 0) return;
    const request = {
      position: this.clampPosition(position),
      resetPosition: Boolean(resetPosition),
    };
    if (this.windowRequestPending) {
      const pending = this.pendingWindowRequest;
      if (
        pending
        && Math.abs(pending.position - request.position) < 1
        && (!request.resetPosition || pending.resetPosition)
      ) {
        return;
      }
      this.queuedWindowRequest = request;
      return;
    }
    request.workletRequestId = ++this.windowRequestSerial;
    this.windowRequestPending = true;
    this.pendingWindowRequest = request;
    this.send({ type: "window-request", ...request });
  }

  flushQueuedWindowRequest() {
    if (this.windowRequestPending || !this.queuedWindowRequest) return;
    const request = this.queuedWindowRequest;
    this.queuedWindowRequest = null;
    this.requestWindow(request.position, request.resetPosition);
  }

  waitForWindow(
    position,
    { resetPosition = false, holdPosition = this.dsp.position, pauseFrame = currentFrame } = {},
  ) {
    const requestPosition = this.clampPosition(position);
    const wasWaiting = this.waitingForData;
    this.waitingForData = true;
    this.waitingWindowPosition = requestPosition;
    if (resetPosition) this.desiredResetPosition = requestPosition;
    if (Number.isFinite(holdPosition)) {
      this.lastPosition = this.clampPosition(holdPosition);
      if (Math.abs(this.dsp.position - this.lastPosition) > 0.5) {
        this.dsp.setPosition(this.lastPosition, 0);
      }
    }
    if (this.replay && this.replay.pausedAtFrame == null) {
      this.replay.pausedAtFrame = Math.max(0, Math.floor(Number(pauseFrame) || 0));
    }
    this.requestWindow(requestPosition, resetPosition);
    if (!wasWaiting && (this.active || this.playing || this.scratching || this.replay)) {
      this.send({ type: "buffering", position: this.lastPosition });
    }
  }

  ensureWindowForPosition(position, { resetPosition = false, pauseFrame = currentFrame } = {}) {
    const target = this.clampPosition(position);
    if (this.windowContainsPosition(target)) return true;
    if (!resetPosition && this.windowContainsPosition(this.dsp.position)) {
      this.requestWindow(target, false);
      return true;
    }
    this.waitForWindow(target, {
      resetPosition,
      holdPosition: resetPosition ? target : this.dsp.position,
      pauseFrame,
    });
    return false;
  }

  applyPcmWindow(message) {
    const generation = Math.max(0, Math.floor(Number(message.generation) || 0));
    const start = Math.max(0, Math.floor(Number(message.start) || 0));
    const totalFrames = Math.max(1, Math.floor(Number(message.totalFrames) || this.streamLength || 1));
    const length = Math.max(0, Math.floor(Number(message.length) || 0));
    const availableEnd = Math.max(0, Math.min(totalFrames, Math.floor(Number(message.availableEnd) || 0)));
    let applied = false;
    let resumed = false;
    let errorMessage = "";

    try {
      if (generation < this.activeWindowGeneration) throw new Error("stale PCM window generation");
      if (length <= 0 || start + length > totalFrames) throw new Error("invalid PCM window range");

      let channels;
      const bankIndex = Math.floor(Number(message.bankIndex));
      if (bankIndex >= 0) {
        const bank = this.windowBanks[bankIndex];
        if (!bank || bank.length !== this.windowChannelCount) throw new Error("missing shared PCM window bank");
        if (bank.some(channel => channel.length < length)) throw new Error("shared PCM window is shorter than advertised");
        channels = bank.map(channel => channel.subarray(0, length));
      } else {
        const buffers = Array.isArray(message.channelBuffers) ? message.channelBuffers : [];
        if (buffers.length !== this.windowChannelCount) throw new Error("invalid fallback PCM window channel count");
        channels = buffers.map(buffer => {
          const channel = new Float32Array(buffer);
          if (channel.length < length) throw new Error("fallback PCM window is shorter than advertised");
          return channel.subarray(0, length);
        });
      }

      const desiredReset = this.desiredResetPosition;
      const desiredResetIsReady = desiredReset != null
        && this.windowContainsPosition(desiredReset, start, start + length);
      const messagePosition = this.clampPosition(message.position);
      const workletRequestId = Math.max(0, Math.floor(Number(message.workletRequestId) || 0));
      const obsoleteReplayReset = (
        workletRequestId > 0
        && workletRequestId <= this.ignoreResetThroughWindowRequestId
      );
      const resetPosition = obsoleteReplayReset
        ? undefined
        : desiredResetIsReady
          ? desiredReset
          : desiredReset == null && message.resetPosition
            ? messagePosition
            : undefined;
      const sourceRate = Number.isFinite(message.sampleRate) && message.sampleRate > 0
        ? message.sampleRate
        : this.sourceSampleRate;

      this.dsp.prepareWindow(channels.length, length);
      for (let channelIndex = 0; channelIndex < channels.length; channelIndex += 1) {
        const pointer = this.dsp.windowChannelPtr(channelIndex);
        if (!pointer) throw new Error(`Rust PCM window channel ${channelIndex} has no storage`);
        new Float32Array(wasm.memory.buffer, pointer, length).set(channels[channelIndex]);
      }
      this.dsp.commitWindow(sourceRate, start, totalFrames, resetPosition);
      this.dsp.setNeedleLifted(this.needleLifted);
      this.sourceSampleRate = sourceRate;
      this.streamLength = totalFrames;
      this.length = totalFrames;
      this.decodedLength = Math.max(this.decodedLength, availableEnd);
      this.windowStart = start;
      this.windowEnd = start + length;
      this.activeWindowGeneration = generation;
      if (resetPosition !== undefined) {
        this.lastPosition = this.clampPosition(resetPosition);
        if (desiredResetIsReady) this.desiredResetPosition = null;
      }

      const replayRestorePosition = this.replayRestoreWindowPosition;
      if (replayRestorePosition != null) {
        if (this.windowContainsPosition(replayRestorePosition)) {
          this.replayRestoreWindowPosition = null;
        } else {
          // An obsolete replay request may still replace the PCM window, but it
          // cannot reset Rust. Hold the restored DSP state and make the original
          // position the next non-resetting request.
          this.waitingForData = true;
          this.waitingWindowPosition = replayRestorePosition;
          this.queuedWindowRequest = {
            position: replayRestorePosition,
            resetPosition: false,
          };
        }
      }

      const waitingPositionReady = this.waitingWindowPosition == null
        || this.windowContainsPosition(this.waitingWindowPosition);
      let resumePosition = this.clampPosition(this.dsp.position);
      if (
        this.waitingForData
        && waitingPositionReady
        && !this.windowContainsPosition(resumePosition)
        && this.windowContainsPosition(this.waitingWindowPosition)
      ) {
        resumePosition = this.waitingWindowPosition;
        this.dsp.setPosition(resumePosition, 0);
        this.lastPosition = resumePosition;
      }
      if (this.waitingForData && waitingPositionReady && this.windowContainsPosition(resumePosition)) {
        this.waitingForData = false;
        this.waitingWindowPosition = null;
        this.applyTransport();
        if (this.replay?.pausedAtFrame != null) {
          this.replay.startFrame += Math.max(0, currentFrame - this.replay.pausedAtFrame);
          this.replay.pausedAtFrame = null;
        }
        resumed = true;
        this.send({
          type: "buffered",
          decodedLength: this.decodedLength,
          totalLength: this.streamLength,
          position: this.lastPosition,
        });
      }
      applied = true;
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error);
      this.send({ type: "worklet-error", stage: "window-ready", message: errorMessage });
    }

    this.windowRequestPending = false;
    this.pendingWindowRequest = null;
    this.send({
      type: "window-applied",
      applied,
      resumed,
      generation,
      requestId: message.requestId,
      decodedLength: this.decodedLength,
      availableEnd,
      windowStart: applied ? this.windowStart : 0,
      windowEnd: applied ? this.windowEnd : 0,
      error: errorMessage,
    });
    this.flushQueuedWindowRequest();
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
    return Math.max(0, Math.min(Math.max(0, this.length - 2), position));
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

  applyReplayEvent(event, outputFrame = currentFrame) {
    if (event.type === "scratch-preset") {
      this.setScratchPresetAndClicks(event.preset, undefined);
      return;
    }
    if (event.type === "scratch-clicks") {
      this.scratchClicks = this.normalizeScratchClicks(event.clicks, this.scratchClicks);
      this.dsp.setScratchClicks(this.scratchClicks);
      return;
    }
    if (event.type === "manual-crossfader") {
      this.applyDspControl("replay-manual-crossfader", () => {
        this.dsp.setManualCrossfader(event.value);
      });
      return;
    }
    const position = this.clampPosition(event.positionFrames);
    const rate = Number.isFinite(event.rate) ? event.rate : 0;
    const impulse = Number.isFinite(event.impulse) ? event.impulse : 0;
    if (event.type === "scratch-start") {
      this.scratching = true;
      this.active = true;
      this.dsp.setTransport(true, this.motorRate(), rate);
      this.dsp.setMotion(position, rate, impulse);
      this.ensureWindowForPosition(position, { pauseFrame: outputFrame });
    } else if (event.type === "scratch-motion") {
      this.scratching = true;
      this.active = true;
      this.dsp.setTransport(true, this.motorRate(), rate);
      this.dsp.setMotion(position, rate, impulse);
      this.ensureWindowForPosition(position, { pauseFrame: outputFrame });
    } else if (event.type === "scratch-end") {
      this.scratching = false;
      this.playing = event.resumePlayback !== false;
      this.dsp.setTransport(false, this.motorRate(), 0);
      if (!this.playing && !this.motorRunning) {
        this.active = false;
        this.dsp.stop();
      } else if (this.motorRunning) {
        this.active = true;
      }
    }
  }

  finishReplay(cancelled = false, completionFrame = currentFrame, { requestWindow = true } = {}) {
    if (!this.replay) return;
    const { id, restoreState } = this.replay;
    const replayPosition = this.clampPosition(this.dsp.position);
    this.replay = null;
    this.effects = { ...restoreState.effects };
    this.scratchPreset = restoreState.scratchPreset;
    this.scratchClicks = restoreState.scratchClicks;
    this.needleLifted = restoreState.needleLifted;
    this.motorRunning = restoreState.motorRunning;
    this.playbackRate = restoreState.playbackRate;
    this.playing = restoreState.playing;
    this.scratching = restoreState.scratching;
    this.active = restoreState.active;
    this.lastPosition = restoreState.position;
    this.restoreCapturedReplayDsp(restoreState);
    this.waitingForData = false;
    this.waitingWindowPosition = null;
    this.desiredResetPosition = null;
    this.queuedWindowRequest = null;
    if (requestWindow) {
      this.ignoreResetThroughWindowRequestId = Math.max(
        this.ignoreResetThroughWindowRequestId,
        this.windowRequestSerial,
      );
      this.replayRestoreWindowPosition = restoreState.position;
      this.ensureWindowForPosition(restoreState.position, {
        // Rust has already restored position, inertia, limiter envelopes and gate
        // continuity. A PCM-window swap must not reset that dynamic state.
        resetPosition: false,
        pauseFrame: completionFrame,
      });
      if (this.windowRequestPending) {
        this.queuedWindowRequest = {
          position: restoreState.position,
          resetPosition: false,
        };
      } else if (this.windowContainsPosition(restoreState.position)) {
        this.replayRestoreWindowPosition = null;
      }
    } else {
      this.replayRestoreWindowPosition = null;
    }
    this.send({
      type: "scratch-replay-ended",
      id,
      cancelled,
      position: restoreState.position,
      replayPosition,
      outputFrame: Math.max(0, Math.floor(Number(completionFrame) || 0)),
      currentFrame: Math.max(0, Math.floor(Number(completionFrame) || 0)),
    });
  }

  restoreCapturedReplayDsp(restoreState) {
    if (this.dsp.restoreReplayState()) return;
    this.send({
      type: "worklet-error",
      stage: "restore-replay-state",
      message: "Rust replay snapshot was unavailable",
    });
    this.dsp.setManualFaderGain(1);
    this.dsp.setOutputGain(restoreState.outputGain, 0);
    this.dsp.setEffects(this.effects.acoustic, this.effects.surface);
    this.setScratchPresetAndClicks(restoreState.scratchPreset, restoreState.scratchClicks);
    if (Number.isFinite(restoreState.nativeRpm) && restoreState.nativeRpm > 0) {
      this.dsp.setNativeRpm(restoreState.nativeRpm);
    }
    if (this.active) this.dsp.start();
    this.dsp.setPosition(restoreState.position, 0);
    this.dsp.setNeedleLifted(this.needleLifted);
    this.dsp.setTransport(this.scratching, this.motorRate(), 0);
    if (!this.active) this.dsp.stop();
  }

  prepareWindowForRender(frameCount, outputFrame = currentFrame) {
    if (this.waitingForData || !this.active || this.surfaceRegionActive) return !this.waitingForData;
    const position = this.clampPosition(this.dsp.position);
    if (!this.windowContainsPosition(position)) {
      this.waitForWindow(position, { holdPosition: position, pauseFrame: outputFrame });
      return false;
    }

    let rate = Number(this.dsp.effectiveRate) || 0;
    if (Math.abs(rate) < 0.0001 && !this.scratching) rate = this.motorRate();
    if (Math.abs(rate) < 0.0001) return true;
    const sourcePerOutputFrame = this.sourceSampleRate / sampleRate;
    const travel = Math.max(4, Math.ceil(Math.abs(rate) * sourcePerOutputFrame * frameCount) + 4);

    if (rate > 0) {
      const readableEnd = Math.min(this.windowEnd, this.decodedLength);
      if (position + travel <= readableEnd - 2) return true;
      if (this.decodedLength >= this.streamLength && readableEnd >= this.streamLength) return true;
      const windowLimited = this.windowEnd + 2 < this.decodedLength;
      const requestPosition = windowLimited
        ? Math.min(this.decodedLength - 2, position + Math.max(travel, this.windowFrames / 4))
        : Math.min(this.streamLength - 1, this.decodedLength);
      const holdPosition = Math.min(position, Math.max(0, this.decodedLength - 2));
      this.waitForWindow(requestPosition, { holdPosition, pauseFrame: outputFrame });
      return false;
    }

    if (this.windowStart <= 0 || position - travel >= this.windowStart) return true;
    const requestPosition = Math.max(0, position - Math.max(travel, this.windowFrames / 4));
    this.waitForWindow(requestPosition, { holdPosition: position, pauseFrame: outputFrame });
    return false;
  }

  shouldRenderPreciseProgrammeEnd(frameCount) {
    return (
      frameCount > 0
      && this.playing
      && !this.scratching
      && !this.replay
      && !this.surfaceRegionActive
      && !this.waitingForData
    );
  }

  zeroOutputRange(outputs, outputOffset, frameCount) {
    if (frameCount <= 0) return;
    for (const channel of outputs[0]) channel.fill(0, outputOffset, outputOffset + frameCount);
  }

  deadwaxDurationFrames() {
    const nativeRpm = Number(this.dsp.nativeRpm) || 33.3333333333;
    const selectedRpm = Math.abs(nativeRpm * this.motorRate());
    const rpm = selectedRpm > 0.0001 ? selectedRpm : nativeRpm;
    return Math.max(1, Math.round(this.deadwaxTurns * 60 / rpm * sampleRate));
  }

  finishProgrammeWithinQuantum(outputs, outputOffset, frameCount, handoffFrame) {
    this.lastPosition = this.dsp.position;
    this.playing = false;
    const shouldStartDeadwax = !this.cleanEnd && this.motorRunning && !this.needleLifted;
    let deadwaxDurationFrames = 0;
    if (shouldStartDeadwax) {
      deadwaxDurationFrames = this.deadwaxDurationFrames();
      this.startSurfaceRegion({
        region: "deadwax",
        regionId: 0,
        durationFrames: deadwaxDurationFrames,
        startFrame: handoffFrame,
        automatic: true,
      }, { preservePlatter: true });
      this.renderRange(outputs, outputOffset, frameCount);
    } else {
      this.active = false;
      this.zeroOutputRange(outputs, outputOffset, frameCount);
    }
    this.send({
      type: "ended",
      position: this.lastPosition,
      playbackEpoch: this.playbackEpoch,
      outputFrame: handoffFrame,
      currentFrame: handoffFrame,
      deadwaxStarted: shouldStartDeadwax,
      deadwaxDurationFrames,
    });
  }

  renderPreciseProgrammeEnd(outputs, outputOffset, frameCount) {
    const outputFrame = Math.max(0, Number(currentFrame) || 0) + outputOffset;
    const windowReady = this.prepareWindowForRender(frameCount, outputFrame);
    if (!windowReady) {
      this.dsp.renderWindowMissing(frameCount, outputs[0].length);
      this.copyOutput(outputs, outputOffset, frameCount);
      return;
    }
    const renderedFrames = this.dsp.render(frameCount, outputs[0].length);
    const requestedWindowPosition = this.dsp.takeWindowRequest();
    if (requestedWindowPosition >= 0) this.requestWindow(requestedWindowPosition, false);
    if (!this.dsp.takeEnded()) {
      this.copyOutput(outputs, outputOffset, frameCount);
      if (!this.waitingForData) this.prepareWindowForRender(1, outputFrame + frameCount);
      return;
    }
    const programmeFrames = Math.max(0, Math.min(frameCount, renderedFrames));
    this.copyOutput(outputs, outputOffset, programmeFrames);
    const suffixOffset = outputOffset + programmeFrames;
    const suffixFrames = frameCount - programmeFrames;
    const handoffFrame = Math.max(0, Number(currentFrame) || 0) + suffixOffset;
    this.finishProgrammeWithinQuantum(outputs, suffixOffset, suffixFrames, handoffFrame);
  }

  renderRange(outputs, outputOffset, frameCount) {
    if (frameCount <= 0) return;
    if (this.surfaceRegionActive) {
      this.dsp.renderSurface(frameCount, outputs[0].length);
      this.copyOutput(outputs, outputOffset, frameCount);
      return;
    }
    if (this.shouldRenderPreciseProgrammeEnd(frameCount)) {
      this.programmeEndCheckedThisQuantum = true;
      this.renderPreciseProgrammeEnd(outputs, outputOffset, frameCount);
      return;
    }
    const outputFrame = Math.max(0, Number(currentFrame) || 0) + outputOffset;
    const windowReady = this.prepareWindowForRender(frameCount, outputFrame);
    if (windowReady) {
      this.dsp.render(frameCount, outputs[0].length);
      const requestedWindowPosition = this.dsp.takeWindowRequest();
      if (requestedWindowPosition >= 0) this.requestWindow(requestedWindowPosition, false);
      if (!this.waitingForData && !this.surfaceRegionActive) {
        this.prepareWindowForRender(1, outputFrame + frameCount);
      }
    } else {
      this.dsp.renderWindowMissing(frameCount, outputs[0].length);
    }
    this.copyOutput(outputs, outputOffset, frameCount);
  }

  processSurfaceRegion(outputs, frameCount, quantumStart, quantumEnd) {
    const surface = this.surfaceRegion;
    if (!this.surfaceRegionActive || !surface) return false;
    if (surface.region === "deadwax") {
      this.renderRange(outputs, 0, frameCount);
      if (!surface.completed && quantumEnd >= surface.endFrame) {
        this.completeSurfaceRegion(surface.endFrame);
      }
      return true;
    }

    const surfaceFrames = Math.max(0, Math.min(frameCount, surface.endFrame - quantumStart));
    if (surfaceFrames > 0) this.renderRange(outputs, 0, surfaceFrames);
    if (quantumEnd >= surface.endFrame) this.completeSurfaceRegion(surface.endFrame);
    if (surfaceFrames < frameCount) {
      this.renderRange(outputs, surfaceFrames, frameCount - surfaceFrames);
    }
    return true;
  }

  publishState(frameCount, outputFrame = currentFrame + frameCount) {
    this.reportCounter += frameCount;
    if (this.reportCounter >= 1024) {
      this.reportCounter = 0;
      if (!this.waitingForData) this.lastPosition = this.dsp.position;
      const renderedOutputFrame = Math.max(0, Math.floor(Number(outputFrame) || 0));
      this.send({
        type: "position",
        position: this.lastPosition,
        effectiveRate: this.dsp.effectiveRate,
        platterRotationTurns: this.dsp.platterRotationTurns,
        scratchPreset: this.scratchPreset,
        scratchClicks: this.scratchClicks,
        scratchGate: this.dsp.scratchGate,
        scratchGateTarget: this.dsp.scratchGateTarget,
        scratchDirection: this.dsp.scratchDirection,
        scratchMoving: this.dsp.scratchMoving,
        scratchGatePhase: this.dsp.scratchGatePhase,
        scratchStrokeProgress: this.dsp.scratchStrokeProgress,
        highFrequencyAccelerationLimit: this.dsp.highFrequencyAccelerationLimit,
        stylusTracingLimit: this.dsp.stylusTracingLimit,
        outputFrame: renderedOutputFrame,
        currentFrame: renderedOutputFrame,
        scratching: this.scratching,
        buffering: this.waitingForData,
        ...(this.lastInputTiming || {}),
      });
    }
    if (!this.programmeEndCheckedThisQuantum && this.dsp.takeEnded()) {
      this.lastPosition = this.dsp.position;
      if (this.surfaceRegionActive) {
        return;
      }
      if (this.replay) return;
      if (this.waitingForData || this.decodedLength < this.streamLength) {
        const holdPosition = Math.min(this.lastPosition, Math.max(0, this.decodedLength - 2));
        this.waitForWindow(Math.min(this.streamLength - 1, this.decodedLength), {
          holdPosition,
          pauseFrame: outputFrame,
        });
      } else {
        this.playing = false;
        this.active = false;
        this.send({
          type: "ended",
          position: this.dsp.position,
          playbackEpoch: this.playbackEpoch,
        });
      }
    }
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output?.length) return true;
    const frameCount = output[0]?.length || 0;
    this.programmeEndCheckedThisQuantum = false;
    if (!this.active && !this.scratching) this.dsp.stop();

    let offset = 0;
    const quantumStart = currentFrame;
    const quantumEnd = quantumStart + frameCount;
    if (this.processSurfaceRegion(outputs, frameCount, quantumStart, quantumEnd)) {
      this.publishState(frameCount, quantumEnd);
      return true;
    }
    while (this.replay && !this.waitingForData) {
      const replay = this.replay;
      const event = replay.events[replay.index];
      const eventFrame = event ? replay.startFrame + event.frameOffset : Number.POSITIVE_INFINITY;
      const finishFrame = replay.startFrame + replay.durationFrames;
      const boundaryFrame = Math.min(eventFrame, finishFrame);
      if (boundaryFrame > quantumEnd) break;
      const boundaryOffset = Math.max(
        offset,
        Math.min(frameCount, Math.floor(boundaryFrame - quantumStart)),
      );
      this.renderRange(outputs, offset, boundaryOffset - offset);
      offset = boundaryOffset;
      if (event && eventFrame <= finishFrame) {
        this.applyReplayEvent(event, boundaryFrame);
        replay.index += 1;
        continue;
      }
      this.finishReplay(false, boundaryFrame);
      break;
    }
    this.renderRange(outputs, offset, frameCount - offset);
    this.publishState(frameCount, quantumEnd);
    return true;
  }
}

registerProcessor("bitneedle-player", BitneedlePlayerProcessor);
