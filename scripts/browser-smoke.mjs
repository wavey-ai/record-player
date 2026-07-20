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
        if (message.error) request.reject(new Error(message.error.message || "CDP command failed"));
        else request.resolve(message.result || {});
        return;
      }
      for (const listener of this.listeners.get(message.method) || []) {
        listener(message.params || {});
      }
    });
  }

  on(method, listener) {
    const listeners = this.listeners.get(method) || [];
    listeners.push(listener);
    this.listeners.set(method, listeners);
  }

  async send(method, params = {}) {
    await this.ready;
    const id = this.nextId++;
    const response = new Promise((resolveResponse, reject) => {
      this.pending.set(id, { resolve: resolveResponse, reject });
    });
    this.socket.send(JSON.stringify({ id, method, params }));
    return response;
  }

  close() {
    this.socket.close();
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
  assert(document.querySelector("#hf-acceleration-limit")?.value === "0.35", "HF limiter UI default is not 0.35");

  await player.loadAudioFile(createWaveFile(), { cleanEnd: true, title: "Browser smoke" });
  await player.setNeedleLifted(false);
  const loaded = player.getState();
  assert(loaded.ready, "The synthetic browser source did not become ready");
  assert(loaded.highFrequencyAccelerationLimit === 0.35, "HF limiter engine default is not 0.35");
  assert(loaded.stylusTracingLimit === 0.72, "Stylus tracing engine default is not 0.72");
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
  await player.seekSeconds(9.25);
  await wait(500);
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
  const unsubscribe = player.subscribe(snapshot => {
    if (!snapshot.scratching) return;
    const trace = traces[activePreset];
    trace.minimumGate = Math.min(trace.minimumGate, Number(snapshot.scratchGate));
    trace.maximumGate = Math.max(trace.maximumGate, Number(snapshot.scratchGate));
    trace.directions.add(Number(snapshot.scratchDirection));
    trace.samples += 1;
    if (Number.isFinite(snapshot.pointerToAudioLatencyMs)) {
      maximumLatencyMs = Math.max(maximumLatencyMs, snapshot.pointerToAudioLatencyMs);
    }
  });

  setPhase("scratch-presets");
  let positionFrames = player.getState().positionFrames;
  player.startScratchRecording({ name: "Chrome smoke" });
  const began = await player.beginScratch({
    pointerId: 41,
    positionFrames,
    rotationDegrees: 0,
    inputTimeMs: performance.now(),
  });
  assert(began !== false, "The browser scratch gesture did not start");

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
        inputTimeMs: performance.now(),
      });
      if (step === 8) {
        await player.setCrossfader(0.35);
        assert(player.getState().scratching, "A simultaneous fader move cancelled the record gesture");
      }
      if (step === 10) await player.setCrossfader(0.5);
      await wait(8);
    }
  }

  await player.endScratch({ rotationDegrees: 0, resumePlayback: false });
  await wait(120);
  unsubscribe();
  const recordedTake = await player.stopScratchRecording({ save: false });
  assert(recordedTake?.events?.length > 10, "The browser scratch take did not record engine events");
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

  return {
    chrome: navigator.userAgent,
    crossOriginIsolated: globalThis.crossOriginIsolated,
    sampleRate: loaded.sampleRate,
    outputSampleRate: loaded.outputSampleRate,
    audioBaseLatencyMs: player.getState().audioBaseLatencyMs,
    audioOutputLatencyMs: player.getState().audioOutputLatencyMs,
    audioPlaybackStats,
    softwareLoopback,
    maximumPointerToAudioLatencyMs: maximumLatencyMs,
    pointerAppliedCommandId,
    captureBytes,
    continuity,
    replay: {
      eventCount: recordedTake.events.length,
      durationFrames: recordedTake.durationFrames,
      completedAndRestored: true,
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
    }),
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
    });
    const phase = String(phaseResult.result?.value || "startup");
    for (const [contextId, context] of audioContexts) {
      if (context.contextType !== "realtime" || context.contextState === "closed") continue;
      try {
        const sample = await session.send("WebAudio.getRealtimeData", { contextId });
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
      const canvas = document.querySelector("#player-canvas");
      const rect = canvas.getBoundingClientRect();
      const geometry = buildCanvasGeometry(rect.width, rect.height);
      const point = (angle, radius) => ({
        x: rect.left + geometry.cx + Math.cos(angle) * radius,
        y: rect.top + geometry.cy + Math.sin(angle) * radius,
      });
      const recordRadius = geometry.recordRadius * 0.7;
      const faderRadius = (geometry.controlBandInner + geometry.controlBandOuter) / 2;
      return {
        recordStart: point(0, recordRadius),
        recordMoveA: point(0.10, recordRadius),
        recordMoveB: point(0.18, recordRadius),
        faderStart: point(minuteToDegrees(26) * Math.PI / 180, faderRadius),
        faderMove: point(minuteToDegrees(24.3) * Math.PI / 180, faderRadius),
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
    url: `http://127.0.0.1:${serverPort}/dj-validation.html`,
  });
  await pause(500);
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
  if (validationConsole?.schemaVersion !== 2) throw new Error("Validation console did not use schema version 2");
  if (validationConsole?.summary?.participants !== 1) throw new Error("Validation console did not record a participant");
  if (!validationConsole?.playerApiPublished) throw new Error("Validation console did not expose the real player API");
  if (typeof validationConsole?.buildInfo?.worktreeDirty !== "boolean") {
    throw new Error("Validation console did not load Git build metadata");
  }
  if (pageErrors.length) throw new Error(`Browser page errors:\n${pageErrors.join("\n")}`);
  process.stdout.write(`Real Chrome AudioWorklet smoke passed:\n${JSON.stringify({
    ...result.result?.value,
    multiPointer,
    webAudioRealtime,
    validationConsole,
  }, null, 2)}\n`);
} finally {
  session?.close();
  chrome.kill("SIGTERM");
  server.kill("SIGTERM");
  await Promise.race([
    once(chrome, "exit").catch(() => {}),
    new Promise(resolveWait => setTimeout(resolveWait, 2_000)),
  ]);
  rmSync(profilePath, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}
