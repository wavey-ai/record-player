import { spawn } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(fileURLToPath(new URL("..", import.meta.url)));
const chromePath = [
  process.env.CHROME_PATH,
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome",
  "/usr/bin/google-chrome-stable",
  "/usr/bin/chromium",
  "/usr/bin/chromium-browser",
].find(candidate => candidate && existsSync(candidate));

if (!chromePath) {
  throw new Error("Chrome or Chromium was not found. Set CHROME_PATH to its executable.");
}

function reservePort() {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : 0;
      server.close(error => error ? reject(error) : resolvePort(port));
    });
  });
}

async function pollJson(url, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError = null;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return response.json();
      lastError = new Error(`${url} returned ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise(resolveWait => setTimeout(resolveWait, 50));
  }
  throw lastError || new Error(`Timed out waiting for ${url}`);
}

async function pollResponse(url, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError = null;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return;
      lastError = new Error(`${url} returned ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise(resolveWait => setTimeout(resolveWait, 50));
  }
  throw lastError || new Error(`Timed out waiting for ${url}`);
}

function withTimeout(promise, timeoutMs, label) {
  let timeout = 0;
  return Promise.race([
    promise.finally(() => clearTimeout(timeout)),
    new Promise((_, reject) => {
      timeout = setTimeout(() => reject(new Error(`${label} timed out after ${timeoutMs} ms`)), timeoutMs);
    }),
  ]);
}

class CdpSession {
  constructor(url) {
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = new Map();
    this.socket = new WebSocket(url);
    this.ready = new Promise((resolveReady, reject) => {
      this.socket.addEventListener("open", resolveReady, { once: true });
      this.socket.addEventListener("error", reject, { once: true });
    });
    this.socket.addEventListener("message", event => {
      const message = JSON.parse(String(event.data || "{}"));
      if (message.id) {
        const request = this.pending.get(message.id);
        if (!request) return;
        this.pending.delete(message.id);
        clearTimeout(request.timeout);
        if (message.error) request.reject(new Error(message.error.message || "CDP command failed"));
        else request.resolve(message.result || {});
        return;
      }
      for (const listener of this.listeners.get(message.method) || []) {
        listener(message.params || {});
      }
    });
    this.socket.addEventListener("close", () => {
      for (const [id, request] of this.pending) {
        clearTimeout(request.timeout);
        request.reject(new Error(`CDP connection closed before command ${id} completed`));
      }
      this.pending.clear();
    });
  }

  on(method, listener) {
    const listeners = this.listeners.get(method) || [];
    listeners.push(listener);
    this.listeners.set(method, listeners);
  }

  async send(method, params = {}, timeoutMs = 15_000) {
    await withTimeout(this.ready, timeoutMs, "Chrome DevTools connection");
    const id = this.nextId++;
    const response = new Promise((resolveResponse, reject) => {
      const timeout = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`${method} timed out after ${timeoutMs} ms`));
      }, timeoutMs);
      this.pending.set(id, { resolve: resolveResponse, reject, timeout });
    });
    this.socket.send(JSON.stringify({ id, method, params }));
    return response;
  }

  async close() {
    if (this.socket.readyState === 3) return;
    let timeout = 0;
    const closed = new Promise(resolveClose => {
      this.socket.addEventListener("close", resolveClose, { once: true });
      timeout = setTimeout(resolveClose, 1_000);
    });
    this.socket.close();
    await closed;
    clearTimeout(timeout);
  }
}

async function runBrowserScenario() {
  const wait = milliseconds => new Promise(resolveWait => setTimeout(resolveWait, milliseconds));
  const assert = (condition, message) => {
    if (!condition) throw new Error(message);
  };
  const waitUntil = async (predicate, timeoutMs, message) => {
    const deadline = performance.now() + timeoutMs;
    while (!predicate() && performance.now() < deadline) await wait(20);
    assert(predicate(), message);
  };
  let phase = "startup";
  let continuityWorker = null;
  const setPhase = value => {
    phase = value;
    globalThis.__VINYL_BROWSER_PHASE__ = value;
    continuityWorker?.postMessage({ type: "phase", phase: value });
  };
  const readyDeadline = performance.now() + 10_000;
  while (!globalThis.vin?.yl?.player && performance.now() < readyDeadline) {
    await wait(25);
  }
  const player = globalThis.vin?.yl?.player;
  assert(player, "The public player API was not published");
  assert(
    typeof player.measureAcousticLoopbackLatency === "function",
    "The public acoustic-loopback diagnostic API was not published",
  );
  const { createProgrammeStylusCalibration } = await import("./player-stylus-calibration.js");
  const programmeGapMap = {
    totalSamples: 9_000_000,
    gaps: [{
      startSample: 4_000_000,
      endSample: 4_096_000,
      radialStartNormalized: 0.421,
      radialEndNormalized: 0.429,
    }],
  };
  const programmeGapCalibration = await createProgrammeStylusCalibration(programmeGapMap, {
    loadModule: async () => {
      const module = await import("./record-player/record_player.js");
      await module.default({ module_or_path: "./record-player/record_player_bg.wasm" });
      return module;
    },
  });
  const gapStartGroove = programmeGapCalibration.sampleRatioToGroove(4_000_000 / 9_000_000);
  const gapEndGroove = programmeGapCalibration.sampleRatioToGroove(4_096_000 / 9_000_000);
  const gapMidSampleRatio = programmeGapCalibration.grooveToSampleRatio(0.425);
  assert(Math.abs(gapStartGroove - 0.421) < 1e-9, "Rust stylus calibration missed the outer gap edge");
  assert(Math.abs(gapEndGroove - 0.429) < 1e-9, "Rust stylus calibration missed the inner gap edge");
  assert(
    gapMidSampleRatio >= 4_000_000 / 9_000_000
      && gapMidSampleRatio <= 4_096_000 / 9_000_000,
    "A needle drop in the visible gap did not map into gap PCM",
  );
  const programmeGapStylusCalibration = {
    gapStartGroove,
    gapEndGroove,
    gapMidSample: gapMidSampleRatio * 9_000_000,
  };
  programmeGapCalibration.destroy();
  const { measureAcousticLoopbackLatency } = await import("./audio-loopback-latency.js");
  const loopbackContext = new AudioContext({ sampleRate: 48_000 });
  const loopbackDestination = loopbackContext.createMediaStreamDestination();
  let softwareLoopback;
  try {
    softwareLoopback = await measureAcousticLoopbackLatency(loopbackContext, {
      inputStream: loopbackDestination.stream,
      outputDestination: loopbackDestination,
      repetitions: 3,
      maximumLatencyMs: 100,
      minimumCorrelation: 0.1,
      leadInMs: 250,
    });
    assert(softwareLoopback.samples === 3, "The browser software loopback missed a probe");
    assert(softwareLoopback.maximumMs < 100, "The browser software loopback exceeded its search range");
  } finally {
    for (const track of loopbackDestination.stream.getTracks()) track.stop();
    await loopbackContext.close();
  }

  function createWaveFile(seconds = 12, sampleRate = 48_000) {
    const frameCount = Math.round(seconds * sampleRate);
    const channelCount = 2;
    const bytesPerSample = 2;
    const dataBytes = frameCount * channelCount * bytesPerSample;
    const bytes = new ArrayBuffer(44 + dataBytes);
    const view = new DataView(bytes);
    const writeText = (offset, value) => {
      for (let index = 0; index < value.length; index += 1) {
        view.setUint8(offset + index, value.charCodeAt(index));
      }
    };
    writeText(0, "RIFF");
    view.setUint32(4, 36 + dataBytes, true);
    writeText(8, "WAVE");
    writeText(12, "fmt ");
    view.setUint32(16, 16, true);
    view.setUint16(20, 1, true);
    view.setUint16(22, channelCount, true);
    view.setUint32(24, sampleRate, true);
    view.setUint32(28, sampleRate * channelCount * bytesPerSample, true);
    view.setUint16(32, channelCount * bytesPerSample, true);
    view.setUint16(34, bytesPerSample * 8, true);
    writeText(36, "data");
    view.setUint32(40, dataBytes, true);
    for (let frame = 0; frame < frameCount; frame += 1) {
      const time = frame / sampleRate;
      const attack = Math.min(1, frame / 128);
      const transient = frame % 12_000 < 72 ? Math.exp(-(frame % 12_000) / 18) * 0.35 : 0;
      const left = attack * (
        Math.sin(Math.PI * 2 * 173 * time) * 0.32
        + Math.sin(Math.PI * 2 * 3_911 * time) * 0.08
        + transient
      );
      const right = attack * (
        Math.sin(Math.PI * 2 * 257 * time) * 0.29
        + Math.sin(Math.PI * 2 * 6_217 * time) * 0.07
        + transient * 0.8
      );
      const offset = 44 + frame * 4;
      view.setInt16(offset, Math.round(Math.max(-1, Math.min(1, left)) * 32_767), true);
      view.setInt16(offset + 2, Math.round(Math.max(-1, Math.min(1, right)) * 32_767), true);
    }
    return new File([bytes], "browser-smoke.wav", { type: "audio/wav" });
  }

  const initialAdvanced = document.querySelector(".advanced-controls");
  assert(initialAdvanced instanceof HTMLDetailsElement, "Advanced controls are missing");
  assert(!initialAdvanced.open, "Advanced controls must start collapsed");
  assert(initialAdvanced.contains(document.querySelector("#scratch-preset")), "Scratch preset is outside advanced controls");
  assert(initialAdvanced.contains(document.querySelector("#scratch-clicks")), "Scratch clicks are outside advanced controls");
  assert(document.querySelector("#hf-acceleration-limit")?.value === "0.35", "HF limiter UI default is not 0.35");

  await player.startTransport();
  const emptyMotor = player.getState();
  assert(!emptyMotor.ready, "The empty deck unexpectedly reported ready");
  assert(emptyMotor.motorRunning, "START did not run the empty platter motor");
  assert(emptyMotor.needleLifted, "The empty deck lowered its needle before media was ready");
  await waitUntil(
    () => Math.abs(player.getState().rotationDegrees - emptyMotor.rotationDegrees) > 0.01,
    2_000,
    "The Rust platter phase did not advance before media was ready",
  );
  const emptyMotorAdvanced = player.getState();
  assert(
    Math.abs(emptyMotorAdvanced.rotationDegrees - emptyMotor.rotationDegrees) > 0.01,
    "The Rust platter phase did not advance before media was ready",
  );

  await player.loadAudioFile(createWaveFile(), { cleanEnd: true, title: "Browser smoke" });
  const loaded = player.getState();
  assert(loaded.ready, "The synthetic browser source did not become ready");
  assert(loaded.motorRunning, "Media loading stopped the live platter motor");
  assert(!loaded.needleLifted, "The needle did not lower automatically when media became ready");
  assert(loaded.playing, "Automatic needle drop did not start programme playback");
  assert(loaded.highFrequencyAccelerationLimit === 0.35, "HF limiter engine default is not 0.35");
  assert(loaded.stylusTracingLimit === 0.72, "Stylus tracing engine default is not 0.72");
  assert(loaded.acousticEffects === true, "Acoustic effects are not enabled in the rendered path");
  assert(loaded.surfaceEffects === true, "Surface effects are not enabled in the rendered path");
  assert(globalThis.crossOriginIsolated, "The player page is not cross-origin isolated");

  const capture = await player.getCaptureStream();
  function continuityWorkerEntry() {
    let reader = null;
    let reading = false;
    let phase = "startup";
    const result = {
      supported: typeof ReadableStream === "function",
      packets: 0,
      frames: 0,
      positiveTimestampDeviationCount: 0,
      negativeTimestampDeviationCount: 0,
      nonMonotonicTimestampCount: 0,
      maximumAdjacentTimestampDeviationUs: 0,
      summedDurationUs: 0,
      firstTimestampUs: null,
      lastEndTimestampUs: null,
      phases: {},
    };

    async function readStream(readable) {
      if (!result.supported || !readable) throw new Error("A transferable audio stream is unavailable");
      reader = readable.getReader();
      reading = true;
      self.postMessage({ type: "ready" });
      let expectedTimestamp = null;
      let previousTimestamp = null;
      while (reading) {
        const { value, done } = await reader.read();
        if (done || !value) break;
        try {
          const duration = Number(value.duration)
            || Math.round(value.numberOfFrames / value.sampleRate * 1_000_000);
          const timestamp = Number(value.timestamp);
          const phaseState = result.phases[phase] ||= {
            packets: 0,
            frames: 0,
            silentPackets: 0,
            maximumSilentRunFrames: 0,
            currentSilentRunFrames: 0,
          };
          phaseState.packets += 1;
          phaseState.frames += value.numberOfFrames;
          result.packets += 1;
          result.frames += value.numberOfFrames;
          result.summedDurationUs += duration;
          if (result.firstTimestampUs == null && Number.isFinite(timestamp)) {
            result.firstTimestampUs = timestamp;
          }
          if (previousTimestamp != null && timestamp <= previousTimestamp) {
            result.nonMonotonicTimestampCount += 1;
          }
          if (expectedTimestamp != null && Number.isFinite(timestamp)) {
            const delta = timestamp - expectedTimestamp;
            const timestampToleranceUs = 1_000_000 / value.sampleRate + 2;
            if (delta > timestampToleranceUs) {
              result.positiveTimestampDeviationCount += 1;
            } else if (delta < -timestampToleranceUs) {
              result.negativeTimestampDeviationCount += 1;
            }
            result.maximumAdjacentTimestampDeviationUs = Math.max(
              result.maximumAdjacentTimestampDeviationUs,
              Math.abs(delta),
            );
          }
          if (Number.isFinite(timestamp)) {
            previousTimestamp = timestamp;
            expectedTimestamp = timestamp + duration;
            result.lastEndTimestampUs = expectedTimestamp;
          }

          const samples = new Float32Array(value.numberOfFrames);
          let energy = 0;
          for (let channel = 0; channel < value.numberOfChannels; channel += 1) {
            value.copyTo(samples, { planeIndex: channel, format: "f32-planar" });
            for (const sample of samples) energy += sample * sample;
          }
          const sampleCount = samples.length * value.numberOfChannels;
          if (energy / Math.max(1, sampleCount) < 1e-12) {
            phaseState.silentPackets += 1;
            phaseState.currentSilentRunFrames += value.numberOfFrames;
            phaseState.maximumSilentRunFrames = Math.max(
              phaseState.maximumSilentRunFrames,
              phaseState.currentSilentRunFrames,
            );
          } else {
            phaseState.currentSilentRunFrames = 0;
          }
        } finally {
          value.close();
        }
      }
      for (const phaseState of Object.values(result.phases)) {
        delete phaseState.currentSilentRunFrames;
      }
      result.timelineSpanUs = result.firstTimestampUs == null || result.lastEndTimestampUs == null
        ? 0
        : result.lastEndTimestampUs - result.firstTimestampUs;
      result.timelineDurationErrorUs = result.timelineSpanUs - result.summedDurationUs;
      self.postMessage({ type: "result", result });
    }

    self.onmessage = event => {
      const message = event.data || {};
      if (message.type === "phase") {
        phase = String(message.phase || "unknown");
      } else if (message.type === "start") {
        phase = String(message.phase || phase);
        void readStream(message.readable).catch(error => {
          self.postMessage({ type: "error", message: error?.message || String(error) });
        });
      } else if (message.type === "stop") {
        reading = false;
        void reader?.cancel().catch(() => {});
      }
    };
  }

  const continuityWorkerUrl = URL.createObjectURL(new Blob([
    `(${continuityWorkerEntry.toString()})()`,
  ], { type: "text/javascript" }));
  continuityWorker = new Worker(continuityWorkerUrl);
  const continuityReady = new Promise((resolveReady, rejectReady) => {
    continuityWorker.addEventListener("message", event => {
      if (event.data?.type === "ready") resolveReady();
      if (event.data?.type === "error") rejectReady(new Error(event.data.message));
    });
    continuityWorker.addEventListener("error", event => rejectReady(event.error || new Error(event.message)));
  });
  const continuityResult = new Promise((resolveResult, rejectResult) => {
    continuityWorker.addEventListener("message", event => {
      if (event.data?.type === "result") resolveResult(event.data.result);
      if (event.data?.type === "error") rejectResult(new Error(event.data.message));
    });
    continuityWorker.addEventListener("error", event => rejectResult(event.error || new Error(event.message)));
  });
  assert(typeof MediaStreamTrackProcessor === "function", "MediaStreamTrackProcessor is unavailable");
  const continuityTrack = capture.getAudioTracks()[0].clone();
  const continuityReadable = new MediaStreamTrackProcessor({
    track: continuityTrack,
  }).readable;
  continuityWorker.postMessage(
    { type: "start", readable: continuityReadable, phase },
    [continuityReadable],
  );
  await continuityReady;
  const recorder = new MediaRecorder(capture);
  const chunks = [];
  recorder.addEventListener("dataavailable", event => {
    if (event.data?.size) chunks.push(event.data);
  });
  const recorderStopped = new Promise(resolveStopped => recorder.addEventListener("stop", resolveStopped, { once: true }));
  recorder.start(100);

  setPhase("lead-in");
  await player.play();
  await waitUntil(
    () => player.getState().playing && !player.getState().leadInActive,
    6_000,
    "The real browser lead-in did not hand off to programme playback",
  );
  setPhase("steady-playback");
  const steadyStartPosition = player.getState().positionFrames;
  await wait(800);
  assert(
    player.getState().positionFrames > steadyStartPosition + loaded.sampleRate * 0.4,
    "Steady browser playback did not advance on the Rust audio clock",
  );
  setPhase("window-swap");
  const observedSeekPositions = [];
  const originalMessagePortPost = MessagePort.prototype.postMessage;
  const originalRandom = Math.random;
  MessagePort.prototype.postMessage = function postMessage(message, ...transfer) {
    if (message?.type === "seek" && Number.isFinite(message.position)) {
      observedSeekPositions.push(message.position);
    }
    return originalMessagePortPost.call(this, message, ...transfer);
  };
  Math.random = () => 0.5;
  try {
    await player.seekSeconds(9.25);
    await wait(500);
  } finally {
    Math.random = originalRandom;
    MessagePort.prototype.postMessage = originalMessagePortPost;
  }
  const expectedCuePosition = (9.25 - 0.095) * loaded.sampleRate;
  assert(observedSeekPositions.length > 0, "The browser seek did not reach the worklet");
  assert(
    observedSeekPositions.every(position => Math.abs(position - expectedCuePosition) <= 1),
    `The browser seek exposed pre-landing positions: ${observedSeekPositions.join(", ")}`,
  );
  assert(player.getState().positionSeconds > 8.5, "The browser window-swap seek did not apply");

  const presetNames = ["baby", "stab", "chirp", "transform", "flare", "crab", "orbit", "drum"];
  const traces = Object.fromEntries(presetNames.map(name => [name, {
    minimumGate: 1,
    maximumGate: 0,
    directions: new Set(),
    samples: 0,
  }]));
  let activePreset = "baby";
  let maximumLatencyMs = 0;
  let collectPointerLatency = true;
  let lastCollectedPointerCommandId = null;
  const unsubscribe = player.subscribe(snapshot => {
    if (!snapshot.scratching) return;
    const trace = traces[activePreset];
    trace.minimumGate = Math.min(trace.minimumGate, Number(snapshot.scratchGate));
    trace.maximumGate = Math.max(trace.maximumGate, Number(snapshot.scratchGate));
    trace.directions.add(Number(snapshot.scratchDirection));
    trace.samples += 1;
    if (
      collectPointerLatency
      && Number.isInteger(snapshot.pointerAppliedCommandId)
      && snapshot.pointerAppliedCommandId !== lastCollectedPointerCommandId
      && Number.isFinite(snapshot.pointerToAudioLatencyMs)
    ) {
      lastCollectedPointerCommandId = snapshot.pointerAppliedCommandId;
      maximumLatencyMs = Math.max(maximumLatencyMs, snapshot.pointerToAudioLatencyMs);
    }
  });

  setPhase("scratch-presets");
  let positionFrames = player.getState().positionFrames;
  const recordingStartedAtMs = performance.now();
  player.startScratchRecording({ name: "Chrome smoke" });
  const began = await player.beginScratch({
    pointerId: 41,
    positionFrames,
    rotationDegrees: 0,
    rate: -1.4,
    impulse: 0.37,
    grip: 0.25,
    inputTimeMs: performance.now(),
  });
  assert(began !== false, "The browser scratch gesture did not start");
  assert(Math.abs(player.getState().scratchGrip - 0.25) < 1e-9, "The browser did not expose begin grip");
  await wait(80);
  const beginDirectionAfterCore = player.getState().scratchDirection;
  assert(beginDirectionAfterCore === -1, "The Rust begin command overwrote initial reverse intent");

  const delayedInputTimeMs = performance.now() - 40;
  const delayedInputExpectedOffsetFrames = Math.max(
    0,
    Math.round((delayedInputTimeMs - recordingStartedAtMs) * loaded.outputSampleRate / 1_000),
  );
  const delayedInputPriorCommandId = player.getState().pointerAppliedCommandId;
  positionFrames += 0.333 * loaded.sampleRate * 0.008;
  collectPointerLatency = false;
  await player.updateScratch({
    positionFrames,
    rate: 0.333,
    rotationDegrees: 1,
    impulse: 0.019,
    grip: 0.55,
    inputTimeMs: delayedInputTimeMs,
  });
  await waitUntil(
    () => player.getState().pointerAppliedCommandId !== delayedInputPriorCommandId,
    1_000,
    "The worklet did not acknowledge the delayed pointer marker",
  );
  lastCollectedPointerCommandId = player.getState().pointerAppliedCommandId;
  collectPointerLatency = true;

  for (const preset of presetNames) {
    activePreset = preset;
    player.setScratchPreset(preset);
    const browserClicks = { transform: 2, flare: 1, crab: 8, orbit: 2 }[preset];
    if (browserClicks) player.setScratchClicks(browserClicks);
    for (let step = 0; step < 48; step += 1) {
      const direction = step < 24 ? 1 : -1;
      const magnitude = step % 5 === 0 ? 2.2 : step % 3 === 0 ? 1.35 : 0.72;
      const rate = direction * magnitude;
      positionFrames += rate * loaded.sampleRate * 0.008;
      await player.updateScratch({
        positionFrames,
        rate,
        rotationDegrees: direction * step * 3,
        impulse: step === 24 ? 0.063 : 0,
        grip: direction > 0 ? 0.35 : 0.85,
        inputTimeMs: performance.now(),
      });
      if (preset === "baby" && step === 8) {
        await player.setCrossfader(0.35);
        assert(player.getState().scratching, "A simultaneous fader move cancelled the record gesture");
      }
      if (preset === "baby" && step === 10) await player.setCrossfader(0.5);
      await wait(8);
    }
  }

  await player.endScratch({
    rotationDegrees: 0,
    resumePlayback: false,
    cancelled: true,
    inputTimeMs: performance.now(),
  });
  assert(player.getState().scratchGrip === 0, "The browser did not release scratch grip");
  await wait(120);
  unsubscribe();
  const recordedTake = await player.stopScratchRecording({ save: false });
  assert(recordedTake?.events?.length > 10, "The browser scratch take did not record engine events");
  assert(recordedTake.schemaVersion === 2, "The browser scratch take did not use schema version 2");
  assert(recordedTake.engine?.version === 5, "The browser scratch take did not identify projected-input capture engine version 5");
  assert(recordedTake.engine?.gateAlgorithmVersion === 3, "The browser scratch take did not identify gate algorithm version 3");
  assert(Number.isInteger(recordedTake.replaySeed) && recordedTake.replaySeed > 0, "The browser scratch take did not store a replay seed");
  assert(Number.isFinite(recordedTake.initialState?.rotationDegrees), "The browser scratch take did not store its platter angle");
  const recordedGrip = recordedTake.events
    .filter(event => event.type === "scratch-start" || event.type === "scratch-motion")
    .map(event => event.grip);
  assert(recordedGrip.includes(0.25), "The browser take did not record begin grip");
  assert(recordedGrip.includes(0.35) && recordedGrip.includes(0.85), "The browser take did not record motion grip");
  const recordedStart = recordedTake.events.find(event => event.type === "scratch-start");
  const recordedEnd = recordedTake.events.find(event => event.type === "scratch-end");
  assert(recordedStart?.rate === -1.4, "The browser take did not record begin rate");
  assert(recordedStart?.impulse === 0.37, "The browser take did not record grab impulse");
  assert(recordedEnd?.cancelled === true, "The browser take did not retain pointer cancellation");
  const delayedInputEvent = recordedTake.events.find(event => (
    event.type === "scratch-motion"
    && event.rate === 0.333
    && event.impulse === 0.019
  ));
  assert(delayedInputEvent, "The browser take did not retain the projected-input marker");
  assert(
    Math.abs(delayedInputEvent.frameOffset - delayedInputExpectedOffsetFrames) <= 512,
    `Projected pointer frame ${delayedInputEvent.frameOffset} missed ${delayedInputExpectedOffsetFrames}`,
  );
  let replayObserved = false;
  const unsubscribeReplay = player.subscribe(snapshot => {
    replayObserved ||= Boolean(snapshot.scratchReplayActive);
  });
  setPhase("replay");
  await player.replayScratch(recordedTake, { effects: "original" });
  unsubscribeReplay();
  assert(replayObserved, "The browser did not expose the active replay transaction");
  assert(!player.getState().scratchReplayActive, "The browser replay transaction did not restore state");
  recorder.stop();
  await recorderStopped;
  setPhase("complete");
  continuityWorker.postMessage({ type: "stop" });
  const continuity = await continuityResult;
  continuityTrack.stop();
  continuityWorker.terminate();
  continuityWorker = null;
  URL.revokeObjectURL(continuityWorkerUrl);
  const captureBytes = chunks.reduce((total, chunk) => total + chunk.size, 0);

  for (const preset of presetNames) {
    const trace = traces[preset];
    assert(trace.samples > 0, `${preset} did not publish browser telemetry`);
    assert(Number.isFinite(trace.minimumGate) && Number.isFinite(trace.maximumGate), `${preset} gate telemetry was invalid`);
    assert(trace.directions.has(1) && trace.directions.has(-1), `${preset} did not recognize both directions`);
  }
  assert(traces.baby.minimumGate > 0.98, "Baby unexpectedly closed the assisted gate");
  for (const preset of presetNames.filter(name => name !== "baby")) {
    assert(traces[preset].minimumGate < 0.75, `${preset} never produced a closed gate state`);
    assert(traces[preset].maximumGate > 0.25, `${preset} never produced an open gate state`);
  }
  assert(captureBytes > 1_024, "The real browser capture stream did not contain rendered audio");
  assert(maximumLatencyMs > 0 && maximumLatencyMs < 250, `Pointer-to-audio latency telemetry was ${maximumLatencyMs} ms`);
  assert(continuity.supported, "MediaStreamTrackProcessor is unavailable for continuity checks");
  assert(continuity.packets > 100, "The browser capture did not expose enough audio packets");
  assert(
    continuity.nonMonotonicTimestampCount === 0,
    `The browser capture had ${continuity.nonMonotonicTimestampCount} non-monotonic timestamps`,
  );
  assert(
    Math.abs(continuity.timelineDurationErrorUs) < 5_000,
    `The browser capture timeline differed from its audio duration by ${continuity.timelineDurationErrorUs} us`,
  );
  assert(
    continuity.maximumAdjacentTimestampDeviationUs < 20_000,
    `The browser capture had a ${continuity.maximumAdjacentTimestampDeviationUs} us delivery deviation`,
  );
  const steadyContinuity = continuity.phases["steady-playback"];
  assert(steadyContinuity?.packets > 5, "Steady playback did not expose continuity packets");
  assert(
    steadyContinuity.maximumSilentRunFrames <= loaded.outputSampleRate * 0.02,
    `Steady playback had ${steadyContinuity.maximumSilentRunFrames} consecutive silent frames`,
  );
  const audioPlaybackStats = player.getState().audioPlaybackStats;
  assert(audioPlaybackStats?.supported, "Chrome did not expose AudioContext playback statistics");
  assert(
    audioPlaybackStats.totalDurationMs > 1_000,
    `Chrome reported only ${audioPlaybackStats.totalDurationMs} ms of observed audio`,
  );
  assert(
    audioPlaybackStats.underrunEvents === 0,
    `Chrome reported ${audioPlaybackStats.underrunEvents} audio underrun events`,
  );
  assert(
    audioPlaybackStats.underrunDurationMs === 0,
    `Chrome reported ${audioPlaybackStats.underrunDurationMs} ms of audio underruns`,
  );
  let liveMeasurementError = null;
  try {
    await player.measureAcousticLoopbackLatency();
  } catch (error) {
    liveMeasurementError = error;
  }
  assert(
    /Stop the transport/.test(String(liveMeasurementError?.message || "")),
    "The public acoustic-loopback API did not reject an active transport",
  );
  const pointerAppliedCommandId = player.getState().pointerAppliedCommandId;
  assert(
    Number.isInteger(pointerAppliedCommandId) && pointerAppliedCommandId > 0,
    "The public player state did not identify the applied pointer command",
  );
  const pointerInputOutputFrame = player.getState().pointerInputOutputFrame;
  const pointerAppliedOutputFrame = player.getState().pointerAppliedOutputFrame;
  assert(
    Number.isInteger(pointerInputOutputFrame) && Number.isInteger(pointerAppliedOutputFrame),
    "The public player state did not expose pointer input/applied output frames",
  );
  assert(
    pointerAppliedOutputFrame >= pointerInputOutputFrame,
    "The pointer command was reported applied before its projected input frame",
  );
  assert(
    Math.abs(
      (pointerAppliedOutputFrame - pointerInputOutputFrame) / loaded.outputSampleRate * 1_000
      - player.getState().pointerToAudioLatencyMs
    ) < 0.1,
    "Pointer frame telemetry disagreed with pointer latency telemetry",
  );

  return {
    chrome: navigator.userAgent,
    crossOriginIsolated: globalThis.crossOriginIsolated,
    sampleRate: loaded.sampleRate,
    outputSampleRate: loaded.outputSampleRate,
    audioBaseLatencyMs: player.getState().audioBaseLatencyMs,
    audioOutputLatencyMs: player.getState().audioOutputLatencyMs,
    audioPlaybackStats,
    softwareLoopback,
    needleCue: {
      aimedPositionFrames: 9.25 * loaded.sampleRate,
      expectedLandingFrames: expectedCuePosition,
      workletSeekPositions: observedSeekPositions,
    },
    programmeGapStylusCalibration,
    maximumPointerToAudioLatencyMs: maximumLatencyMs,
    pointerAppliedCommandId,
    pointerInputOutputFrame,
    pointerAppliedOutputFrame,
    delayedInputCapture: {
      expectedFrameOffset: delayedInputExpectedOffsetFrames,
      recordedFrameOffset: delayedInputEvent.frameOffset,
    },
    captureBytes,
    continuity,
    replay: {
      eventCount: recordedTake.events.length,
      durationFrames: recordedTake.durationFrames,
      engineVersion: recordedTake.engine.version,
      cancellationStored: recordedEnd.cancelled,
      replaySeedStored: Number.isInteger(recordedTake.replaySeed),
      rotationStored: Number.isFinite(recordedTake.initialState.rotationDegrees),
      completedAndRestored: true,
    },
    beginIntent: {
      rate: recordedStart.rate,
      impulse: recordedStart.impulse,
      directionAfterCore: beginDirectionAfterCore,
    },
    traces: Object.fromEntries(Object.entries(traces).map(([name, trace]) => [name, {
      minimumGate: trace.minimumGate,
      maximumGate: trace.maximumGate,
      directions: [...trace.directions].sort(),
      samples: trace.samples,
    }])),
  };
}

const serverPort = await reservePort();
const debugPort = await reservePort();
const profilePath = mkdtempSync(join(tmpdir(), "vin-yl-player-chrome-"));
const server = spawn(process.execPath, ["scripts/dev-server.mjs", String(serverPort)], {
  cwd: repoRoot,
  env: { ...process.env, PORT: String(serverPort) },
  stdio: ["ignore", "pipe", "pipe"],
});
const chrome = spawn(chromePath, [
  "--headless=new",
  "--autoplay-policy=no-user-gesture-required",
  "--disable-background-timer-throttling",
  "--disable-renderer-backgrounding",
  "--no-default-browser-check",
  "--no-first-run",
  `--remote-debugging-port=${debugPort}`,
  `--user-data-dir=${profilePath}`,
  "about:blank",
], { stdio: ["ignore", "pipe", "pipe"] });

let session = null;
const pause = milliseconds => new Promise(resolveWait => setTimeout(resolveWait, milliseconds));
try {
  await pollResponse(`http://127.0.0.1:${serverPort}/`);
  await pollJson(`http://127.0.0.1:${debugPort}/json/version`);
  const targetResponse = await fetch(
    `http://127.0.0.1:${debugPort}/json/new?${encodeURIComponent(`http://127.0.0.1:${serverPort}/?player_log=0`)}`,
    { method: "PUT" },
  );
  if (!targetResponse.ok) throw new Error(`Chrome target creation returned ${targetResponse.status}`);
  const target = await targetResponse.json();
  session = new CdpSession(target.webSocketDebuggerUrl);
  const pageErrors = [];
  const audioContexts = new Map();
  const realtimeSamples = [];
  session.on("Runtime.exceptionThrown", event => {
    pageErrors.push(event.exceptionDetails?.exception?.description || event.exceptionDetails?.text || "Page exception");
  });
  session.on("Log.entryAdded", event => {
    if (event.entry?.level === "error") {
      const url = String(event.entry.url || "");
      if (!url.endsWith("/favicon.ico")) {
        pageErrors.push(`${event.entry.text || "Browser log error"}${url ? ` (${url})` : ""}`);
      }
    }
  });
  session.on("Runtime.consoleAPICalled", event => {
    if (event.type !== "error") return;
    pageErrors.push((event.args || []).map(argument => argument.value || argument.description || "").join(" "));
  });
  session.on("WebAudio.contextCreated", event => {
    if (event.context?.contextId) audioContexts.set(event.context.contextId, event.context);
  });
  session.on("WebAudio.contextChanged", event => {
    if (event.context?.contextId) audioContexts.set(event.context.contextId, event.context);
  });
  session.on("WebAudio.contextWillBeDestroyed", event => {
    if (event.contextId) audioContexts.delete(event.contextId);
  });
  await session.send("Runtime.enable");
  await session.send("Log.enable");
  await session.send("Page.enable");
  await session.send("WebAudio.enable");
  await session.send("Page.navigate", { url: `http://127.0.0.1:${serverPort}/?player_log=0` });
  await new Promise(resolveLoad => {
    const timeout = setTimeout(resolveLoad, 15_000);
    session.on("Page.loadEventFired", () => {
      clearTimeout(timeout);
      resolveLoad();
    });
  });
  const scenarioPromise = withTimeout(
    session.send("Runtime.evaluate", {
      expression: `(${runBrowserScenario.toString()})()`,
      awaitPromise: true,
      returnByValue: true,
      userGesture: true,
    }, 35_000),
    30_000,
    "Chrome AudioWorklet scenario",
  );
  let scenarioSettled = false;
  scenarioPromise.then(
    () => { scenarioSettled = true; },
    () => { scenarioSettled = true; },
  );
  while (!scenarioSettled) {
    const phaseResult = await session.send("Runtime.evaluate", {
      expression: `String(globalThis.__VINYL_BROWSER_PHASE__ || "startup")`,
      returnByValue: true,
    }, 5_000);
    const phase = String(phaseResult.result?.value || "startup");
    for (const [contextId, context] of audioContexts) {
      if (context.contextType !== "realtime" || context.contextState === "closed") continue;
      try {
        const sample = await session.send("WebAudio.getRealtimeData", { contextId }, 5_000);
        const realtimeData = sample.realtimeData || {};
        if (Number.isFinite(realtimeData.renderCapacity)) {
          realtimeSamples.push({ contextId, phase, ...realtimeData });
        }
      } catch {
        // A context can close between its lifecycle event and this sample.
      }
    }
    await pause(40);
  }
  const result = await scenarioPromise;
  if (result.exceptionDetails) {
    throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text || "Browser scenario failed");
  }
  const pointResult = await session.send("Runtime.evaluate", {
    expression: `(async () => {
      const { buildCanvasGeometry, minuteToDegrees } = await import("./player-canvas-geometry.js");
      const { scratchClicksControlGeometry, scratchPresetControlGeometry } = await import("./player-canvas-controls.js");
      const canvas = document.querySelector("#player-canvas");
      const rect = canvas.getBoundingClientRect();
      const geometry = buildCanvasGeometry(rect.width, rect.height);
      const point = (angle, radius) => ({
        x: rect.left + geometry.cx + Math.cos(angle) * radius,
        y: rect.top + geometry.cy + Math.sin(angle) * radius,
      });
      const recordRadius = geometry.recordRadius * 0.7;
      const faderRadius = (geometry.controlBandInner + geometry.controlBandOuter) / 2;
      const scratchPresets = Object.fromEntries(
        ["baby", "stab", "chirp", "transform", "flare", "crab", "orbit", "drum"].map(preset => {
          const control = scratchPresetControlGeometry(geometry, preset);
          return [preset, { x: rect.left + control.x, y: rect.top + control.y }];
        }),
      );
      const scratchClicks = Object.fromEntries(
        [1, 2, 3, 4, 5, 6, 7, 8].map(clicks => {
          const control = scratchClicksControlGeometry(geometry, clicks);
          return [clicks, { x: rect.left + control.x, y: rect.top + control.y }];
        }),
      );
      return {
        recordStart: point(0, recordRadius),
        recordMoveA: point(0.10, recordRadius),
        recordMoveB: point(0.18, recordRadius),
        faderStart: point(minuteToDegrees(26) * Math.PI / 180, faderRadius),
        faderMove: point(minuteToDegrees(24.3) * Math.PI / 180, faderRadius),
        scratchPresets,
        scratchClicks,
        crossfaderBefore: globalThis.vin.yl.player.getState().crossfader,
      };
    })()`,
    awaitPromise: true,
    returnByValue: true,
  });
  if (pointResult.exceptionDetails) {
    throw new Error(pointResult.exceptionDetails.exception?.description || "Could not resolve browser touch points");
  }
  const points = pointResult.result.value;
  const touch = (id, point) => ({
    id,
    x: point.x,
    y: point.y,
    radiusX: 6,
    radiusY: 6,
    force: 0.7,
  });
  await session.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [touch(1, points.recordStart)],
  });
  await pause(80);
  await session.send("Input.dispatchTouchEvent", {
    type: "touchMove",
    touchPoints: [touch(1, points.recordMoveA)],
  });
  await session.send("Input.dispatchTouchEvent", {
    type: "touchMove",
    touchPoints: [touch(1, points.recordMoveA), touch(2, points.faderStart)],
  });
  await pause(30);
  await session.send("Input.dispatchTouchEvent", {
    type: "touchMove",
    touchPoints: [touch(1, points.recordMoveA), touch(2, points.faderMove)],
  });
  await pause(60);
  await session.send("Input.dispatchTouchEvent", {
    type: "touchMove",
    touchPoints: [touch(1, points.recordMoveA)],
  });
  await pause(30);
  const afterFaderRelease = await session.send("Runtime.evaluate", {
    expression: `(() => {
      const state = globalThis.vin.yl.player.getState();
      return { scratching: state.scratching, crossfader: state.crossfader };
    })()`,
    returnByValue: true,
  });
  await session.send("Input.dispatchTouchEvent", {
    type: "touchMove",
    touchPoints: [touch(1, points.recordMoveB)],
  });
  await pause(30);
  await session.send("Input.dispatchTouchEvent", {
    type: "touchEnd",
    touchPoints: [],
  });
  await pause(80);
  const afterRecordRelease = await session.send("Runtime.evaluate", {
    expression: `(() => ({ scratching: globalThis.vin.yl.player.getState().scratching }))()`,
    returnByValue: true,
  });
  const multiPointer = {
    crossfaderBefore: points.crossfaderBefore,
    crossfaderAfter: afterFaderRelease.result?.value?.crossfader,
    recordHeldAfterFaderRelease: afterFaderRelease.result?.value?.scratching,
    recordReleased: !afterRecordRelease.result?.value?.scratching,
  };
  if (!multiPointer.recordHeldAfterFaderRelease) {
    throw new Error("Releasing the browser fader pointer released the record pointer");
  }
  if (!multiPointer.recordReleased) throw new Error("The browser record pointer did not release");
  if (Math.abs(multiPointer.crossfaderAfter - multiPointer.crossfaderBefore) < 0.05) {
    throw new Error("The second browser pointer did not move XFADE");
  }

  const allPresets = ["baby", "stab", "chirp", "transform", "flare", "crab", "orbit", "drum"];
  const allClicks = [1, 2, 3, 4, 5, 6, 7, 8];
  const cycledPresets = [...allPresets.slice(1), allPresets[0]];
  const cycledClicks = [...allClicks.slice(1), allClicks[0]];
  const resetTechniqueControls = async () => {
    const reset = await session.send("Runtime.evaluate", {
      expression: `(async () => {
        const player = globalThis.vin.yl.player;
        await player.setCrossfader(0.37);
        player.setScratchPreset("baby");
        player.setScratchClicks(1);
        const state = player.getState();
        return {
          preset: state.scratchPreset,
          clicks: state.scratchClicks,
          crossfader: state.crossfader,
          presetControl: document.querySelector("#scratch-preset")?.value,
          clicksControl: Number(document.querySelector("#scratch-clicks")?.value),
          activeControl: document.activeElement?.id || document.activeElement?.tagName,
        };
      })()`,
      awaitPromise: true,
      returnByValue: true,
    });
    if (reset.exceptionDetails) {
      throw new Error(reset.exceptionDetails.exception?.description || "Could not reset technique controls");
    }
    await pause(25);
    return reset.result?.value;
  };
  const readTechniqueControls = async () => {
    const current = await session.send("Runtime.evaluate", {
      expression: `(() => {
        const state = globalThis.vin.yl.player.getState();
        return {
          preset: state.scratchPreset,
          clicks: state.scratchClicks,
          crossfader: state.crossfader,
          presetControl: document.querySelector("#scratch-preset")?.value,
          clicksControl: Number(document.querySelector("#scratch-clicks")?.value),
          activeControl: document.activeElement?.id || document.activeElement?.tagName,
        };
      })()`,
      returnByValue: true,
    });
    return current.result?.value;
  };
  const verifyTechniqueTrace = (name, trace, expectedPresets, expectedClicks) => {
    const presets = trace.presets.map(entry => entry.preset);
    const clicks = trace.clicks.map(entry => entry.clicks);
    if (JSON.stringify(presets) !== JSON.stringify(expectedPresets)) {
      throw new Error(
        `${name} selected presets ${presets.join(", ")} instead of ${expectedPresets.join(", ")}`
          + `; trace=${JSON.stringify(trace.presets)}`,
      );
    }
    if (JSON.stringify(clicks) !== JSON.stringify(expectedClicks)) {
      throw new Error(`${name} selected click counts ${clicks.join(", ")} instead of ${expectedClicks.join(", ")}`);
    }
    for (const entry of [...trace.presets, ...trace.clicks]) {
      if (Math.abs(entry.crossfader - 0.37) > 0.000001) {
        throw new Error(`${name} moved the manual crossfader to ${entry.crossfader}`);
      }
    }
  };

  await resetTechniqueControls();
  const programmaticResult = await session.send("Runtime.evaluate", {
    expression: `(() => {
      const player = globalThis.vin.yl.player;
      const presets = [];
      const clicks = [];
      for (const preset of ${JSON.stringify(allPresets)}) {
        player.setScratchPreset(preset);
        const state = player.getState();
        presets.push({ preset: state.scratchPreset, crossfader: state.crossfader });
      }
      for (const clickCount of ${JSON.stringify(allClicks)}) {
        player.setScratchClicks(clickCount);
        const state = player.getState();
        clicks.push({ clicks: state.scratchClicks, crossfader: state.crossfader });
      }
      const combined = player.setScratchTechnique({ preset: "crab", clicks: 7 });
      return { presets, clicks, combined };
    })()`,
    returnByValue: true,
  });
  const programmatic = programmaticResult.result?.value;
  verifyTechniqueTrace("Programmatic controls", programmatic, allPresets, allClicks);
  if (programmatic?.combined?.preset !== "crab" || programmatic?.combined?.clicks !== 7) {
    throw new Error(`Combined scratch API returned ${JSON.stringify(programmatic?.combined)}`);
  }
  const manualOwnershipResult = await session.send("Runtime.evaluate", {
    expression: `(async () => {
      const player = globalThis.vin.yl.player;
      player.setScratchPreset("stab");
      const automatic = player.getState();
      await player.setCrossfader(0.63);
      return { automatic, manual: player.getState() };
    })()`,
    awaitPromise: true,
    returnByValue: true,
  });
  const ownership = manualOwnershipResult.result?.value;
  if (ownership?.automatic?.crossfaderOwner !== "scratch-preset") {
    throw new Error(`Automatic preset did not own XFADE: ${JSON.stringify(ownership?.automatic)}`);
  }
  if (
    ownership?.manual?.scratchPreset !== "baby"
    || ownership?.manual?.crossfaderOwner !== "manual"
    || Math.abs(ownership?.manual?.crossfader - 0.63) > 0.000001
  ) {
    throw new Error(`Direct XFADE input did not transfer ownership to baby: ${JSON.stringify(ownership?.manual)}`);
  }

  await resetTechniqueControls();
  const pointer = { presets: [], clicks: [] };
  const mouseTap = async point => {
    await session.send("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: point.x,
      y: point.y,
      buttons: 0,
    });
    await pause(5);
    await session.send("Input.dispatchMouseEvent", {
      type: "mousePressed",
      x: point.x,
      y: point.y,
      button: "left",
      buttons: 1,
      clickCount: 1,
    });
    await pause(12);
    await session.send("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x: point.x,
      y: point.y,
      button: "left",
      buttons: 0,
      clickCount: 1,
    });
    await pause(25);
  };
  for (const preset of allPresets) {
    await mouseTap(points.scratchPresets[preset]);
    pointer.presets.push(await readTechniqueControls());
  }
  for (const clicks of allClicks) {
    await mouseTap(points.scratchClicks[clicks]);
    pointer.clicks.push(await readTechniqueControls());
  }
  verifyTechniqueTrace("Pointer canvas controls", pointer, allPresets, allClicks);

  await resetTechniqueControls();
  const touchControls = { presets: [], clicks: [] };
  let techniqueTouchId = 20;
  const touchTap = async point => {
    techniqueTouchId += 1;
    await session.send("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [touch(techniqueTouchId, point)],
    });
    await pause(12);
    await session.send("Input.dispatchTouchEvent", {
      type: "touchEnd",
      touchPoints: [],
    });
    await pause(25);
  };
  for (const preset of allPresets) {
    await touchTap(points.scratchPresets[preset]);
    touchControls.presets.push(await readTechniqueControls());
  }
  for (const clicks of allClicks) {
    await touchTap(points.scratchClicks[clicks]);
    touchControls.clicks.push(await readTechniqueControls());
  }
  verifyTechniqueTrace("Touch canvas controls", touchControls, allPresets, allClicks);

  await resetTechniqueControls();
  const focusSummary = await session.send("Runtime.evaluate", {
    expression: `(() => {
      const advanced = document.querySelector(".advanced-controls");
      advanced.open = false;
      const summary = advanced.querySelector("summary");
      summary.focus();
      return document.activeElement === summary;
    })()`,
    returnByValue: true,
  });
  if (!focusSummary.result?.value) throw new Error("Advanced-controls summary did not receive keyboard focus");
  const dispatchKey = async (key, code, keyCode) => {
    const macNativeKeyCodes = { Enter: 36, Space: 49, Home: 115, ArrowDown: 125, ArrowRight: 124 };
    const nativeVirtualKeyCode = process.platform === "darwin" ? macNativeKeyCodes[code] : keyCode;
    const text = key === "Enter" ? "\r" : key === " " ? " " : "";
    await session.send("Input.dispatchKeyEvent", {
      type: text ? "keyDown" : "rawKeyDown",
      key,
      code,
      windowsVirtualKeyCode: keyCode,
      nativeVirtualKeyCode,
      ...(text ? { text, unmodifiedText: text } : {}),
    });
    await session.send("Input.dispatchKeyEvent", {
      type: "keyUp",
      key,
      code,
      windowsVirtualKeyCode: keyCode,
      nativeVirtualKeyCode,
    });
    await pause(25);
  };
  await dispatchKey("Enter", "Enter", 13);
  const advancedOpened = await session.send("Runtime.evaluate", {
    expression: `document.querySelector(".advanced-controls").open`,
    returnByValue: true,
  });
  if (!advancedOpened.result?.value) throw new Error("Keyboard did not open advanced controls");
  const keyboard = { presets: [], clicks: [] };
  const presetButtonFocused = await session.send("Runtime.evaluate", {
    expression: `(() => {
      const button = document.querySelector("#scratch-preset-next");
      button?.focus();
      return document.activeElement === button;
    })()`,
    returnByValue: true,
  });
  if (!presetButtonFocused.result?.value) throw new Error("Next-preset button did not receive keyboard focus");
  for (let index = 0; index < 8; index += 1) {
    await dispatchKey("Enter", "Enter", 13);
    keyboard.presets.push(await readTechniqueControls());
  }
  const clicksButtonFocused = await session.send("Runtime.evaluate", {
    expression: `(() => {
      const button = document.querySelector("#scratch-clicks-next");
      button?.focus();
      return document.activeElement === button;
    })()`,
    returnByValue: true,
  });
  if (!clicksButtonFocused.result?.value) throw new Error("Next-click-count button did not receive keyboard focus");
  for (let index = 0; index < 8; index += 1) {
    await dispatchKey("Enter", "Enter", 13);
    keyboard.clicks.push(await readTechniqueControls());
  }
  verifyTechniqueTrace("Keyboard advanced controls", keyboard, cycledPresets, cycledClicks);
  const summarizeTechniqueTrace = trace => ({
    presets: trace.presets.map(entry => entry.preset),
    clicks: trace.clicks.map(entry => entry.clicks),
  });
  const techniqueControls = {
    programmatic: summarizeTechniqueTrace(programmatic),
    pointer: summarizeTechniqueTrace(pointer),
    touch: summarizeTechniqueTrace(touchControls),
    keyboard: summarizeTechniqueTrace(keyboard),
    manualCrossfaderHeldAt: 0.37,
  };

  if (realtimeSamples.length < 20) {
    throw new Error(`Chrome exposed only ${realtimeSamples.length} Web Audio realtime samples`);
  }
  const percentile = (values, ratio) => {
    const sorted = [...values].sort((left, right) => left - right);
    return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * ratio))];
  };
  const capacities = realtimeSamples.map(sample => sample.renderCapacity);
  // Chromium supplies a normalized ratio here: render duration / callback
  // interval. The DevTools Protocol prose still describes a percentage.
  const maximumRenderCapacitySample = realtimeSamples.reduce((maximum, sample) => (
    sample.renderCapacity > maximum.renderCapacity ? sample : maximum
  ));
  const maximumRenderCapacityRatio = maximumRenderCapacitySample.renderCapacity;
  const p95RenderCapacityRatio = percentile(capacities, 0.95);
  const maximumRenderCapacityRatioAllowed = 1;
  const p95RenderCapacityRatioAllowed = 0.5;
  if (!(maximumRenderCapacityRatio < maximumRenderCapacityRatioAllowed)) {
    throw new Error(
      `Chrome Web Audio callback deadline reached ${(maximumRenderCapacityRatio * 100).toFixed(2)}%`
        + ` during ${maximumRenderCapacitySample.phase}`,
    );
  }
  if (!(p95RenderCapacityRatio < p95RenderCapacityRatioAllowed)) {
    throw new Error(
      `Chrome Web Audio p95 render capacity reached ${(p95RenderCapacityRatio * 100).toFixed(2)}%`,
    );
  }
  const phaseCapacity = {};
  for (const sample of realtimeSamples) {
    const values = phaseCapacity[sample.phase] ||= [];
    values.push(sample.renderCapacity);
  }
  const webAudioRealtime = {
    samples: realtimeSamples.length,
    maximumRenderCapacityPercent: maximumRenderCapacityRatio * 100,
    maximumRenderCapacityPhase: maximumRenderCapacitySample.phase,
    p95RenderCapacityPercent: p95RenderCapacityRatio * 100,
    meanRenderCapacityPercent:
      capacities.reduce((sum, value) => sum + value, 0) / capacities.length * 100,
    maximumRenderCapacityPercentAllowed: maximumRenderCapacityRatioAllowed * 100,
    p95RenderCapacityPercentAllowed: p95RenderCapacityRatioAllowed * 100,
    maximumCallbackIntervalMeanMs: Math.max(
      ...realtimeSamples.map(sample => Number(sample.callbackIntervalMean) || 0),
    ) * 1_000,
    maximumCallbackIntervalVarianceMsSquared: Math.max(
      ...realtimeSamples.map(sample => Number(sample.callbackIntervalVariance) || 0),
    ) * 1_000_000,
    phaseCapacity: Object.fromEntries(Object.entries(phaseCapacity).map(([phase, values]) => [phase, {
      samples: values.length,
      maximumPercent: Math.max(...values) * 100,
      p95Percent: percentile(values, 0.95) * 100,
      meanPercent: values.reduce((sum, value) => sum + value, 0) / values.length * 100,
    }])),
  };
  await session.send("Page.navigate", {
    url: `http://127.0.0.1:${serverPort}/embed.html?player_log=0`,
  });
  await pause(250);
  const embedAdvancedResult = await session.send("Runtime.evaluate", {
    expression: `(async () => {
      const deadline = performance.now() + 10000;
      while ((!globalThis.vin?.yl?.player || location.pathname !== "/embed.html") && performance.now() < deadline) {
        await new Promise(resolve => setTimeout(resolve, 25));
      }
      const player = globalThis.vin?.yl?.player;
      if (!player) throw new Error("Embed player did not start");
      const advanced = document.querySelector(".advanced-controls");
      const preset = document.querySelector("#scratch-preset");
      const clicks = document.querySelector("#scratch-clicks");
      const rect = advanced.getBoundingClientRect();
      const style = getComputedStyle(advanced);
      const components = player.canvas.getConfig()?.components || {};
      return {
        collapsed: !advanced.open,
        containsPreset: advanced.contains(preset),
        containsClicks: advanced.contains(clicks),
        visible: style.display !== "none" && style.visibility !== "hidden" && Number(style.opacity) > 0,
        inInitialViewport: rect.top >= 0 && rect.bottom <= innerHeight,
        scratchPresetCanvasVisible: components.scratchPreset === true,
        scratchClicksCanvasVisible: components.scratchClicks === true,
      };
    })()`,
    awaitPromise: true,
    returnByValue: true,
  });
  if (embedAdvancedResult.exceptionDetails) {
    throw new Error(
      embedAdvancedResult.exceptionDetails.exception?.description
        || embedAdvancedResult.exceptionDetails.text
        || "Embed advanced controls failed",
    );
  }
  const embedAdvancedControls = embedAdvancedResult.result?.value;
  if (!Object.values(embedAdvancedControls || {}).every(Boolean)) {
    throw new Error(`Embed technique controls were inaccessible: ${JSON.stringify(embedAdvancedControls)}`);
  }
  await session.send("Page.navigate", {
    url: `http://127.0.0.1:${serverPort}/dj-validation.html`,
  });
  await pause(500);
  const pointerProbeRectResult = await session.send("Runtime.evaluate", {
    expression: `(async () => {
      const deadline = performance.now() + 10000;
      while (!globalThis.__VINYL_DJ_VALIDATION__?.getPlayer() && performance.now() < deadline) {
        await new Promise(resolve => setTimeout(resolve, 25));
      }
      const pad = document.querySelector("#pointer-probe-pad");
      pad?.scrollIntoView({ block: "center" });
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const rect = pad?.getBoundingClientRect();
      if (!rect?.width || !rect?.height) throw new Error("Pointer input probe is not visible");
      return { left: rect.left, top: rect.top, width: rect.width, height: rect.height };
    })()`,
    awaitPromise: true,
    returnByValue: true,
  });
  if (pointerProbeRectResult.exceptionDetails) {
    throw new Error(pointerProbeRectResult.exceptionDetails.exception?.description || "Pointer probe failed");
  }
  const probeRect = pointerProbeRectResult.result.value;
  const probePoint = (fractionX, fractionY) => ({
    x: probeRect.left + probeRect.width * fractionX,
    y: probeRect.top + probeRect.height * fractionY,
  });
  await session.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [touch(41, probePoint(0.2, 0.45))],
  });
  for (let index = 0; index < 18; index += 1) {
    const progress = index / 17;
    await session.send("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [
        touch(41, probePoint(0.2 + progress * 0.55, 0.45)),
        touch(42, probePoint(0.22 + progress * 0.5, 0.65)),
      ],
    });
    await pause(8);
  }
  await session.send("Input.dispatchTouchEvent", {
    type: "touchMove",
    touchPoints: [touch(41, probePoint(0.78, 0.45))],
  });
  await session.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await pause(50);
  const validationConsoleResult = await session.send("Runtime.evaluate", {
    expression: `(async () => {
      const deadline = performance.now() + 10000;
      while (
        (!globalThis.__VINYL_DJ_VALIDATION__?.getPlayer()
          || !globalThis.__VINYL_DJ_VALIDATION__?.getBuildInfo()?.commit)
        && performance.now() < deadline
      ) await new Promise(resolve => setTimeout(resolve, 25));
      const consoleApi = globalThis.__VINYL_DJ_VALIDATION__;
      if (!consoleApi?.getPlayer()) throw new Error("Validation console player did not start");
      const form = document.querySelector("#participant-form");
      form.elements.id.value = "browser-dj-01";
      form.elements.currentlyActiveDj.checked = true;
      form.elements.regularlyScratches.checked = true;
      form.elements.trainingCompleted.checked = true;
      form.elements.experienceBand.value = "over-10-years";
      form.requestSubmit();
      await new Promise(resolve => setTimeout(resolve, 50));
      return {
        schemaVersion: consoleApi.getSession().snapshot().schemaVersion,
        summary: consoleApi.getSession().summary(),
        buildInfo: consoleApi.getBuildInfo(),
        playerApiPublished: typeof consoleApi.getPlayer().measureAcousticLoopbackLatency === "function",
        pointerInputProfile: consoleApi.getPointerInputProfile(),
        status: document.querySelector("#console-status").textContent,
      };
    })()`,
    awaitPromise: true,
    returnByValue: true,
  });
  if (validationConsoleResult.exceptionDetails) {
    throw new Error(
      validationConsoleResult.exceptionDetails.exception?.description
        || validationConsoleResult.exceptionDetails.text
        || "Validation console failed",
    );
  }
  const validationConsole = validationConsoleResult.result?.value;
  if (validationConsole?.schemaVersion !== 4) throw new Error("Validation console did not use schema version 4");
  if (validationConsole?.summary?.participants !== 1) throw new Error("Validation console did not record a participant");
  if (!validationConsole?.pointerInputProfile?.requirements?.pass
    || validationConsole.pointerInputProfile.maximumConcurrentPointers < 2
    || validationConsole.pointerInputProfile.types?.touch?.gripPolicy !== "full-contact") {
    throw new Error(`Validation console did not capture touch input evidence: ${JSON.stringify(validationConsole?.pointerInputProfile)}`);
  }
  if (!validationConsole?.playerApiPublished) throw new Error("Validation console did not expose the real player API");
  if (typeof validationConsole?.buildInfo?.worktreeDirty !== "boolean") {
    throw new Error("Validation console did not load Git build metadata");
  }
  await session.send("Page.navigate", {
    url: `http://127.0.0.1:${serverPort}/dj-abx.html`,
  });
  await pause(250);
  const blindAbxResult = await session.send("Runtime.evaluate", {
    expression: `(async () => {
      const deadline = performance.now() + 10000;
      while (!globalThis.__VINYL_DJ_ABX__ && performance.now() < deadline) {
        await new Promise(resolve => setTimeout(resolve, 25));
      }
      const runner = globalThis.__VINYL_DJ_ABX__;
      if (!runner) throw new Error("Blind ABX runner did not start");
      const paths = {
        a: "audio/11111111111111111111111111111111.wav",
        b: "audio/22222222222222222222222222222222.wav",
        x: "audio/33333333333333333333333333333333.wav",
      };
      const packageFile = (content, name, path, type) => {
        const file = new File([content], name, { type });
        Object.defineProperty(file, "webkitRelativePath", { value: "operator-package/" + path });
        return file;
      };
      const wav = (sample, tag) => {
        const frames = 2400;
        const channels = 2;
        const blockAlign = channels * 2;
        const dataBytes = frames * blockAlign;
        const bytes = new ArrayBuffer(44 + dataBytes + 12);
        const view = new DataView(bytes);
        const text = (offset, value) => [...value].forEach((character, index) => view.setUint8(offset + index, character.charCodeAt(0)));
        text(0, "RIFF");
        view.setUint32(4, bytes.byteLength - 8, true);
        text(8, "WAVE");
        text(12, "fmt ");
        view.setUint32(16, 16, true);
        view.setUint16(20, 1, true);
        view.setUint16(22, channels, true);
        view.setUint32(24, 48000, true);
        view.setUint32(28, 48000 * blockAlign, true);
        view.setUint16(32, blockAlign, true);
        view.setUint16(34, 16, true);
        text(36, "data");
        view.setUint32(40, dataBytes, true);
        for (let offset = 44; offset < 44 + dataBytes; offset += 2) view.setInt16(offset, sample, true);
        text(44 + dataBytes, "JUNK");
        view.setUint32(48 + dataBytes, 4, true);
        view.setUint32(52 + dataBytes, tag, true);
        return bytes;
      };
      const audioBytes = { a: wav(700, 1), b: wav(-700, 2), x: wav(700, 3) };
      const hexDigest = async bytes => {
        const digest = await crypto.subtle.digest("SHA-256", bytes);
        return [...new Uint8Array(digest)].map(value => value.toString(16).padStart(2, "0")).join("");
      };
      const audio = Object.fromEntries(await Promise.all(["a", "b", "x"].map(async role => [role, {
        path: paths[role],
        sha256: await hexDigest(audioBytes[role]),
      }])));
      const manifest = {
        schemaVersion: 2,
        studyId: "browser-abx-001",
        participantId: "browser-dj-01",
        candidate: {
          commit: "dddddddddddddddddddddddddddddddddddddddd",
          worktreeDirty: false,
          buildInfoSha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
          settings: {
            highFrequencyAccelerationLimit: 0.35,
            stylusTracingLimit: 0.72,
            faderCurve: 0.08,
            acousticEffects: true,
            surfaceEffects: true,
            nativeRpmValues: [100 / 3, 45],
            endPolicies: ["runout", "clean"],
          },
        },
        generatedAt: "2026-07-20T12:00:00.000Z",
        codebookSha256: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        trials: [{
          id: "trial-1",
          excerptId: "browser-excerpt-1",
          gestureFamily: "chirp-flare",
          audio,
        }],
      };
      const manifestText = JSON.stringify(manifest);
      await runner.loadPackageFiles([
        packageFile(manifestText, "blind-manifest.json", "blind-manifest.json", "application/json"),
        ...["a", "b", "x"].map(role => packageFile(
          audioBytes[role],
          paths[role].split("/").pop(),
          paths[role],
          "audio/wav",
        )),
      ]);
      document.querySelector("#start-session").click();
      for (const role of ["a", "b", "x"]) {
        const button = document.querySelector("[data-role=" + role + "]");
        button.click();
        const playDeadline = performance.now() + 3000;
        while (!button.classList.contains("heard") && performance.now() < playDeadline) {
          await new Promise(resolve => setTimeout(resolve, 10));
        }
        if (!button.classList.contains("heard")) throw new Error("Blind runner did not play " + role.toUpperCase());
      }
      const form = document.querySelector("#response-form");
      form.elements.responseLabel.value = "a";
      form.elements.confidence.value = "4";
      form.elements.realism.value = "6";
      form.elements.transientSharpness.value = "6";
      form.elements.timingNaturalness.value = "7";
      form.requestSubmit();
      await new Promise(resolve => setTimeout(resolve, 25));
      const responses = runner.getSession().exportResponses();
      return {
        studyId: runner.getManifest().studyId,
        manifestSha256: runner.getManifestSha256(),
        codebookSha256: runner.getManifest().codebookSha256,
        progress: runner.getSession().progress,
        responses,
        manifestConditionFree: !/physical|player/.test(manifestText),
        completionVisible: !document.querySelector("#complete-panel").hidden,
      };
    })()`,
    awaitPromise: true,
    returnByValue: true,
    userGesture: true,
  });
  if (blindAbxResult.exceptionDetails) {
    throw new Error(
      blindAbxResult.exceptionDetails.exception?.description
        || blindAbxResult.exceptionDetails.text
        || "Blind ABX runner failed",
    );
  }
  const blindAbx = blindAbxResult.result?.value;
  if (!blindAbx?.manifestConditionFree || !blindAbx?.completionVisible) {
    throw new Error("Blind ABX runner exposed a condition or did not complete the trial");
  }
  if (blindAbx?.progress?.completed !== 1 || blindAbx?.responses?.responses?.length !== 1) {
    throw new Error("Blind ABX runner did not freeze the browser response");
  }
  if (!/^[0-9a-f]{64}$/.test(blindAbx?.manifestSha256 || "")) {
    throw new Error("Blind ABX runner did not bind responses to the manifest hash");
  }
  if (blindAbx?.responses?.codebookSha256 !== blindAbx?.codebookSha256) {
    throw new Error("Blind ABX runner did not retain the private-codebook commitment");
  }
  if (/physical|player/.test(JSON.stringify(blindAbx.responses))) {
    throw new Error("Blind ABX response leaked a condition label");
  }
  if (pageErrors.length) throw new Error(`Browser page errors:\n${pageErrors.join("\n")}`);
  process.stdout.write(`Real Chrome AudioWorklet smoke passed:\n${JSON.stringify({
    ...result.result?.value,
    multiPointer,
    techniqueControls,
    webAudioRealtime,
    embedAdvancedControls,
    validationConsole,
    blindAbx,
  }, null, 2)}\n`);
} finally {
  await session?.close();
  chrome.kill("SIGTERM");
  server.kill("SIGTERM");
  await Promise.race([
    once(chrome, "exit").catch(() => {}),
    new Promise(resolveWait => setTimeout(resolveWait, 2_000)),
  ]);
  rmSync(profilePath, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}
