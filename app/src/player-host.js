import { createVinylPlayerCanvas } from "./player-canvas.js";
import { RecordDecoderClient } from "./record-decoder-client.js";
import { readPcmCache, recordCacheKey, writePcmCache } from "./pcm-cache.js";
import { clearScratchPerformances, deleteScratchPerformance, getScratchPerformance, listScratchPerformances, saveScratchPerformance } from "./scratch-performance-store.js";

const elements = {
  file: document.querySelector("#file"),
  load: document.querySelector("#load"),
  play: document.querySelector("#play"),
  needle: document.querySelector("#needle"),
  platter: document.querySelector("#platter"),
  seek: document.querySelector("#seek"),
  status: document.querySelector("#status"),
  recordImage: document.querySelector("#record-image"),
  metadata: document.querySelector("#record-metadata"),
  metaProfile: document.querySelector("#meta-profile"),
  metaContainer: document.querySelector("#meta-container"),
  metaRelease: document.querySelector("#meta-release"),
  rpm33: document.querySelector("#rpm-33"),
  rpm45: document.querySelector("#rpm-45"),
  rpm: document.querySelector("#rpm"),
  volume: document.querySelector("#volume"),
  xfade: document.querySelector("#xfade")
};

const state = {
  context: null,
  node: null,
  worker: null,
  requestId: 0,
  pending: new Map(),
  view: null,
  duration: 0,
  sampleRate: 48000,
  positionFrames: 0,
  pendingSeekGeneration: 0,
  acknowledgedSeekGeneration: 0,
  draggingSeek: false,
  scratching: false,
  scratchPointerId: null,
  scratchStartAngle: 0,
  scratchStartPosition: 0,
  scratchLastAngle: 0,
  scratchLastTime: 0,
  rotation: 0,
  rpm: 33.3333333333,
  lastReportedPosition: 0,
  decoder: null,
  recordObjectUrl: "",
  streamInitialised: false,
  streamReady: false,
  streamDecodedFrames: 0,
  streamReadyPromise: null,
  baseRpm: 33.3333333333,
  seekTimer: 0,
  seekInFlight: false,
  queuedSeekSeconds: null,
  pcmWindowWorker: null,
  pcmWindowRequestId: 0,
  pcmWindowPending: new Map(),
  sharedWindowBanks: [],
  sharedWindowFrames: 0,
  gainNode: null,
  packetGain: 1,
  mixerGain: 1,
  volume: 1,
  crossfader: 0.5,
  listeners: new Set(),
  loadedFile: null,
  recordHash: "",
  scratchRecorders: new Set(),
  scratchReplayRequests: new Map(),
  scratchReplayId: 0,
  activeScratchRecorder: null,
  canvasController: null
};

function setStatus(message) {
  elements.status.value = message;
  elements.status.textContent = message;
}

function profileRpm(recordProfile) {
  return String(recordProfile || "").toLowerCase().includes("single45") ? 45 : 33.3333333333;
}

function updateRpmButtons() {
  elements.rpm33?.classList.toggle("selected", Math.abs(state.rpm - 33.3333333333) < 0.01);
  elements.rpm45?.classList.toggle("selected", Math.abs(state.rpm - 45) < 0.01);
}

async function setRpm(rpm) {
  const nextRpm = Math.max(16, Math.min(90, Number(rpm) || state.baseRpm));
  state.rpm = nextRpm;
  updateRpmButtons();
  if (elements.rpm) elements.rpm.value = String(nextRpm);
  const rate = nextRpm / state.baseRpm;
  await dispatch({ type: "set_playback_rate", deck: "a", rate });
}

async function ensurePcmWindowWorker() {
  if (state.pcmWindowWorker) return;
  state.pcmWindowWorker = new Worker("./pcm-window-worker.js", { type: "module" });
  state.pcmWindowWorker.onmessage = event => {
    const message = event.data || {};
    if (message.type === "window-ready") {
      state.node.port.postMessage({ type: "shared-window-ready", ...message });
      const pending = state.pcmWindowPending.get(message.requestId);
      if (pending) {
        state.pcmWindowPending.delete(message.requestId);
        pending.resolve(message);
      }
    }
  };
}

function createSharedWindowBanks(channelCount, windowFrames) {
  return Array.from({ length: 2 }, () =>
    Array.from({ length: channelCount }, () =>
      new SharedArrayBuffer(windowFrames * Float32Array.BYTES_PER_ELEMENT)
    )
  );
}

function requestPcmWindow(position, resetPosition = false) {
  if (!state.pcmWindowWorker) return Promise.resolve(null);
  const requestId = ++state.pcmWindowRequestId;
  const promise = new Promise((resolve, reject) => {
    state.pcmWindowPending.set(requestId, { resolve, reject });
  });
  state.pcmWindowWorker.postMessage({ type: "request-window", position, resetPosition, requestId });
  return promise;
}

async function loadWindowedPcm({ sampleRate, audioLength, s16ChannelBuffers }) {
  await ensurePcmWindowWorker();
  const sourceBuffers = Array.isArray(s16ChannelBuffers) ? s16ChannelBuffers : [];
  if (!sourceBuffers.length) throw new Error("Record contains no PCM channels");
  const channelCount = Math.max(2, Math.min(2, sourceBuffers.length));
  state.sampleRate = Math.max(1, Number(sampleRate) || 48000);
  state.duration = Math.max(1, Number(audioLength) || 1) / state.sampleRate;
  state.positionFrames = 0;
  state.lastReportedPosition = 0;
  state.sharedWindowFrames = Math.max(16384, Math.round(state.sampleRate * 12));
  state.sharedWindowBanks = createSharedWindowBanks(channelCount, state.sharedWindowFrames);
  state.node.port.postMessage({
    type: "shared-window-init",
    sampleRate: state.sampleRate,
    totalFrames: Math.max(1, Number(audioLength) || 1),
    windowFrames: state.sharedWindowFrames,
    bankBuffers: state.sharedWindowBanks
  });
  const transfers = sourceBuffers.slice(0, channelCount);
  const requestId = ++state.pcmWindowRequestId;
  const ready = new Promise((resolve, reject) => {
    state.pcmWindowPending.set(requestId, { resolve, reject });
  });
  state.pcmWindowWorker.postMessage({
    type: "init",
    sampleRate: state.sampleRate,
    totalFrames: Math.max(1, Number(audioLength) || 1),
    channelCount,
    chunkFrames: state.sampleRate,
    windowFrames: state.sharedWindowFrames,
    bankBuffers: state.sharedWindowBanks,
    channelBuffers: transfers,
    requestId
  }, transfers);
  await ready;
}

async function loadCachedPcm(cached) {
  await loadWindowedPcm({
    sampleRate: cached.sampleRate,
    audioLength: cached.audioLength,
    s16ChannelBuffers: Array.isArray(cached.s16ChannelBuffers)
      ? cached.s16ChannelBuffers.map(buffer => buffer.slice(0))
      : []
  });
}

async function markLoadedReady() {
  await dispatch({ type: "set_load_state", deck: "a", status: "ready", loaded: true, duration_seconds: state.duration });
  await dispatch({ type: "set_needle", deck: "a", lifted: false, observed_playback_seconds: 0 });
  state.node.port.postMessage({ type: "needle", lifted: false });
  elements.play.disabled = false;
  elements.needle.disabled = false;
  elements.seek.disabled = false;
}

async function flushQueuedSeek() {
  if (state.seekInFlight || state.queuedSeekSeconds == null) return;
  const seconds = state.queuedSeekSeconds;
  state.queuedSeekSeconds = null;
  state.seekInFlight = true;
  try {
    await dispatch({ type: "seek", deck: "a", seconds });
  } finally {
    state.seekInFlight = false;
    if (state.queuedSeekSeconds != null) void flushQueuedSeek();
  }
}

function queueSeek(seconds) {
  state.positionFrames = secondsToFrames(seconds);
  seekWorklet(state.positionFrames);
  state.queuedSeekSeconds = seconds;
  clearTimeout(state.seekTimer);
  state.seekTimer = setTimeout(() => void flushQueuedSeek(), 35);
}

function coreRequest(type, payload = {}) {
  return new Promise((resolve, reject) => {
    const id = ++state.requestId;
    state.pending.set(id, { resolve, reject });
    state.worker.postMessage({ id, type, payload });
  });
}

async function dispatch(event) {
  const result = await coreRequest("dispatch", { event });
  state.view = result.view;
  for (const command of result.commands) executeCommand(command);
  render();
}

function deckView() {
  return state.view?.decks?.[0] ?? null;
}

function secondsToFrames(seconds) {
  return Math.round(seconds * state.sampleRate);
}

function framesToSeconds(frames) {
  return frames / state.sampleRate;
}

function executeCommand(command) {
  if (!state.node) return;
  if (command.type === "set_motor") {
    state.node.port.postMessage({ type: "transport", running: command.running });
  } else if (command.type === "start_packet_playback") {
    state.node.port.postMessage({ type: "play", position: secondsToFrames(command.offset_seconds), rate: command.rate, handoff: Boolean(command.platter_handoff) });
  } else if (command.type === "stop_packet_playback") {
    state.node.port.postMessage({ type: "stop", handoff: Boolean(command.platter_handoff) });
  } else if (command.type === "seek_packet_playback") {
    seekWorklet(secondsToFrames(command.offset_seconds));
  } else if (command.type === "set_scratch_transport") {
    state.node.port.postMessage({ type: "scratch-transport", handContact: Boolean(command.hand_contact), motorRate: Number(command.motor_rate) || 0 });
  } else if (command.type === "set_scratch_target") {
    state.node.port.postMessage({ type: "scratch", active: true, position: command.position_frames, rate: command.rate });
  } else if (command.type === "set_scratch_position") {
    seekWorklet(command.position_frames);
  } else if (command.type === "set_packet_gain") {
    state.packetGain = Math.max(0, Number(command.gain) || 0);
    updateOutputGain(command.ramp_ms);
  } else if (command.type === "set_mixer_track_gain" && Number(command.track) === 0) {
    state.mixerGain = Math.max(0, Number(command.gain) || 0);
    updateOutputGain(command.ramp_ms);
  }
}

function seekWorklet(position) {
  const generation = ++state.pendingSeekGeneration;
  state.positionFrames = position;
  state.node.port.postMessage({ type: "seek", position, generation });
  void requestPcmWindow(position, true);
}

async function initialiseAudio() {
  if (state.context) return;
  state.context = new AudioContext({ latencyHint: "interactive" });
  const recordPlayerWasmResponse = await fetch("./wasm/record-player/record_player_bg.wasm");
  if (!recordPlayerWasmResponse.ok) throw new Error(`Failed to load record-player WASM: ${recordPlayerWasmResponse.status}`);
  const recordPlayerWasmModule = await WebAssembly.compileStreaming(recordPlayerWasmResponse);
  await state.context.audioWorklet.addModule("./player-worklet.js");
  state.node = new AudioWorkletNode(state.context, "bitneedle-player", {
    numberOfInputs: 0,
    numberOfOutputs: 1,
    outputChannelCount: [2],
    processorOptions: { wasmModule: recordPlayerWasmModule }
  });
  state.gainNode = state.context.createGain();
  state.gainNode.gain.value = 1;
  state.node.connect(state.gainNode);
  state.gainNode.connect(state.context.destination);
  state.node.port.onmessage = handleWorkletMessage;
}

function handleWorkletMessage(event) {
  const message = event.data;
  if (message.type === "position") {
    if (state.pendingSeekGeneration !== state.acknowledgedSeekGeneration) return;
    const previousPosition = state.lastReportedPosition;
    state.positionFrames = message.position;
    state.lastReportedPosition = message.position;
    if (!state.draggingSeek) elements.seek.value = String(state.duration > 0 ? framesToSeconds(message.position) / state.duration : 0);
    if (!message.scratching && Number.isFinite(previousPosition)) {
      const framesPerTurn = state.sampleRate * 60 / state.baseRpm;
      state.rotation = (state.rotation + ((message.position - previousPosition) / framesPerTurn) * 360) % 360;
      elements.platter.style.setProperty("--rotation", `${state.rotation}deg`);
    }
    const view = deckView();
    if (view && !message.scratching) {
      void dispatch({ type: "playback_position_observed", deck: "a", seconds: framesToSeconds(message.position) });
    }
    publishState();
  } else if (message.type === "seeked") {
    state.acknowledgedSeekGeneration = Math.max(state.acknowledgedSeekGeneration, message.generation ?? 0);
    state.positionFrames = message.position;
  } else if (message.type === "window-request") {
    void requestPcmWindow(message.position, false);
  } else if (message.type === "ended") {
    state.positionFrames = message.position;
    void dispatch({ type: "playback_ended", deck: "a" });
  } else if (message.type === "scratch-replay-ended") {
    const request = state.scratchReplayRequests.get(message.id);
    if (request) {
      state.scratchReplayRequests.delete(message.id);
      request.resolve({ cancelled: Boolean(message.cancelled), positionFrames: Number(message.position) || 0 });
    }
  }
}

async function loadFile(file) {
  await initialiseAudio();
  await state.context.resume();
  state.decoder ??= new RecordDecoderClient();
  await state.decoder.initialise();
  elements.play.disabled = true;
  elements.needle.disabled = true;
  elements.seek.disabled = true;
  if (state.recordObjectUrl) URL.revokeObjectURL(state.recordObjectUrl);
  state.recordObjectUrl = URL.createObjectURL(file);
  elements.recordImage.src = state.recordObjectUrl;
  setStatus(`Inspecting ${file.name}…`);
  const sourceBytes = await file.arrayBuffer();
  const cacheKey = await recordCacheKey(sourceBytes);
  state.recordHash = cacheKey;
  const inspected = await state.decoder.inspect(sourceBytes.slice(0));
  elements.metadata.hidden = false;
  elements.metaProfile.textContent = inspected.recordProfile || "unknown";
  elements.metaContainer.textContent = inspected.payloadContainer || "unknown";
  elements.metaRelease.textContent = inspected.releaseId || "unsigned / unavailable";
  state.baseRpm = profileRpm(inspected.recordProfile);
  state.rpm = state.baseRpm;
  updateRpmButtons();
  const cached = await readPcmCache(cacheKey);
  if (cached) {
    setStatus(`Loading cached PCM for ${file.name}…`);
    await loadCachedPcm(cached);
    await markLoadedReady();
    setStatus(`${file.name} · ${state.duration.toFixed(1)}s · PCM cache hit`);
    return;
  }
  setStatus(`Decoding ${file.name}…`);
  const decoded = await state.decoder.decode(sourceBytes, inspected.recordProfile || "", progress => {
    const message = progress.msg || progress.message || progress.status;
    if (message) setStatus(String(message));
  });

  const cacheBuffers = Array.isArray(decoded.s16ChannelBuffers)
    ? decoded.s16ChannelBuffers.map(buffer => buffer.slice(0))
    : [];
  if (cacheBuffers.length) {
    await writePcmCache(cacheKey, {
      sampleRate: Number(decoded.sampleRate) || state.sampleRate,
      audioLength: Number(decoded.audioLength) || Math.round(state.duration * state.sampleRate),
      s16ChannelBuffers: cacheBuffers,
      recordProfile: inspected.recordProfile || ""
    });
  }

  const sampleRate = Math.max(1, Number(decoded.sampleRate) || 48000);
  const audioLength = Math.max(1, Number(decoded.audioLength) || 0);
  const s16Buffers = Array.isArray(decoded.s16ChannelBuffers) ? decoded.s16ChannelBuffers : [];
  if (!s16Buffers.length) throw new Error("Record decoder returned no PCM channels");
  state.baseRpm = profileRpm(inspected.recordProfile);
  state.rpm = state.baseRpm;
  updateRpmButtons();
  await loadWindowedPcm({ sampleRate, audioLength, s16ChannelBuffers: s16Buffers });

  await markLoadedReady();
  setStatus(`${file.name} · ${state.duration.toFixed(1)}s · ${sampleRate} Hz · ${inspected.payloadContainer || "record"}`);
}

function render() {
  const view = deckView();
  if (!view) return;
  elements.play.textContent = view.playing ? "STOP" : "START";
  elements.needle.textContent = view.needle_lifted ? "NEEDLE DOWN" : "NEEDLE UP";
  publishState();
}

function angleForPointer(event) {
  const rect = elements.platter.getBoundingClientRect();
  return Math.atan2(event.clientY - (rect.top + rect.height / 2), event.clientX - (rect.left + rect.width / 2));
}

function unwrapAngle(delta) {
  if (delta > Math.PI) return delta - Math.PI * 2;
  if (delta < -Math.PI) return delta + Math.PI * 2;
  return delta;
}

function audioFrameNow() {
  return Math.max(0, Math.round((state.context?.currentTime || 0) * state.sampleRate));
}

function scratchInitialState() {
  const view = deckView();
  return {
    positionFrames: state.positionFrames,
    rpm: state.rpm,
    nativeRpm: state.baseRpm,
    playbackRate: state.baseRpm > 0 ? state.rpm / state.baseRpm : 1,
    volume: state.volume,
    crossfader: state.crossfader,
    motorRunning: Boolean(view?.motor_running ?? view?.playing),
    playing: Boolean(view?.playing)
  };
}

function recordScratchEvent(event) {
  const frame = audioFrameNow();
  for (const recorder of state.scratchRecorders) recorder.capture(event, frame);
}

function createScratchRecorder({ name = "" } = {}) {
  let active = false;
  let startFrame = 0;
  let events = [];
  let initialState = null;
  return Object.freeze({
    start() {
      if (active) return;
      active = true;
      startFrame = audioFrameNow();
      events = [];
      initialState = scratchInitialState();
      state.scratchRecorders.add(this);
    },
    capture(event, frame) {
      if (!active) return;
      const next = { ...event, frameOffset: Math.max(0, frame - startFrame) };
      const previous = events[events.length - 1];
      if (previous && previous.type === next.type && previous.frameOffset === next.frameOffset && previous.positionFrames === next.positionFrames && previous.rate === next.rate) return;
      events.push(next);
    },
    stop() {
      if (!active) return null;
      active = false;
      state.scratchRecorders.delete(this);
      const durationFrames = events.length ? events[events.length - 1].frameOffset : Math.max(0, audioFrameNow() - startFrame);
      return Object.freeze({
        id: crypto.randomUUID(),
        schemaVersion: 1,
        name: String(name || ""),
        recordHash: state.recordHash,
        releaseId: elements.metaRelease?.textContent || "",
        createdAt: new Date().toISOString(),
        sampleRate: state.sampleRate,
        durationFrames,
        durationMs: durationFrames / state.sampleRate * 1000,
        engine: {
          name: "vin.yl.player.acoustic",
          version: 1,
          recordProfile: elements.metaProfile?.textContent || "",
          nativeRpm: state.baseRpm
        },
        initialState,
        events: events.map(event => ({ ...event })),
        effects: { acoustic: true, surface: true }
      });
    },
    get active() { return active; }
  });
}

async function replayScratch(performance, { effects = "original" } = {}) {
  if (!performance || !Array.isArray(performance.events)) throw new TypeError("A valid scratch performance is required");
  if (performance.recordHash && state.recordHash && performance.recordHash !== state.recordHash) throw new Error("Scratch performance belongs to a different record");
  await initialiseAudio();
  await state.context.resume();
  const initialPosition = Math.max(0, Number(performance.initialState?.positionFrames) || 0);
  await requestPcmWindow(initialPosition, true);
  const id = ++state.scratchReplayId;
  const completion = new Promise((resolve, reject) => state.scratchReplayRequests.set(id, { resolve, reject }));
  state.node.port.postMessage({ type: "replay-scratch", id, performance, effectsMode: effects });
  return completion;
}

function cancelScratchReplay() {
  state.node?.port.postMessage({ type: "cancel-scratch-replay" });
}

async function beginScratch(event) {
  if (!state.node || state.scratching) return;
  elements.platter.setPointerCapture(event.pointerId);
  const angle = angleForPointer(event);
  state.scratching = true;
  state.scratchPointerId = event.pointerId;
  state.scratchStartAngle = angle;
  state.scratchLastAngle = angle;
  state.scratchLastTime = event.timeStamp;
  state.scratchStartPosition = state.positionFrames;
  recordScratchEvent({ type: "scratch-start", positionFrames: state.positionFrames, rate: 0, impulse: 0.22 });
  await dispatch({
    type: "begin_scratch",
    deck: "a",
    pointer_id: event.pointerId,
    playback_seconds: framesToSeconds(state.positionFrames),
    rotation_degrees: state.rotation
  });
}

function moveScratch(event) {
  if (!state.scratching || event.pointerId !== state.scratchPointerId) return;
  const angle = angleForPointer(event);
  const totalDelta = unwrapAngle(angle - state.scratchStartAngle);
  const localDelta = unwrapAngle(angle - state.scratchLastAngle);
  const elapsedSeconds = Math.max(0.001, (event.timeStamp - state.scratchLastTime) / 1000);
  const framesPerTurn = state.sampleRate * 60 / state.baseRpm;
  const position = Math.max(0, Math.min(secondsToFrames(state.duration), state.scratchStartPosition + (totalDelta / (Math.PI * 2)) * framesPerTurn));
  const rate = (localDelta / (Math.PI * 2)) * framesPerTurn / state.sampleRate / elapsedSeconds;
  state.positionFrames = position;
  state.rotation += localDelta * 180 / Math.PI;
  elements.platter.style.setProperty("--rotation", `${state.rotation}deg`);
  const impulse = Math.min(1, Math.abs(rate) / 3);
  recordScratchEvent({ type: "scratch-motion", positionFrames: position, rate, impulse });
  state.node.port.postMessage({ type: "scratch", active: true, position, rate, impulse });
  void dispatch({
    type: "move_scratch",
    deck: "a",
    position_frames: position,
    rendered_position_frames: position,
    rate,
    rotation_degrees: state.rotation,
    impulse
  });
  state.scratchLastAngle = angle;
  state.scratchLastTime = event.timeStamp;
}

async function endScratch(event) {
  if (!state.scratching || event.pointerId !== state.scratchPointerId) return;
  state.scratching = false;
  recordScratchEvent({ type: "scratch-end", positionFrames: state.positionFrames, rate: 0, impulse: 0, resumePlayback: true });
  await dispatch({
    type: "end_scratch",
    deck: "a",
    rendered_position_frames: state.positionFrames,
    rotation_degrees: state.rotation,
    resume_playback: true,
    save_sample: false,
    can_platter_handoff: true
  });
  state.scratchPointerId = null;
}


function updateOutputGain(rampMs = 12) {
  if (!state.gainNode || !state.context) return;
  const gain = state.packetGain * state.mixerGain;
  const now = state.context.currentTime;
  const end = now + Math.max(0, Number(rampMs) || 0) / 1000;
  state.gainNode.gain.cancelScheduledValues(now);
  state.gainNode.gain.setValueAtTime(state.gainNode.gain.value, now);
  state.gainNode.gain.linearRampToValueAtTime(gain, end);
}

function publicState() {
  const view = deckView();
  return Object.freeze({
    ready: Boolean(view?.loaded),
    playing: Boolean(view?.playing),
    needleLifted: Boolean(view?.needle_lifted),
    scratching: state.scratching,
    positionSeconds: framesToSeconds(state.positionFrames),
    durationSeconds: state.duration,
    positionRatio: state.duration > 0 ? framesToSeconds(state.positionFrames) / state.duration : 0,
    rpm: state.rpm,
    nativeRpm: state.baseRpm,
    playbackRate: state.baseRpm > 0 ? state.rpm / state.baseRpm : 1,
    volume: state.volume,
    crossfader: state.crossfader,
    recordProfile: elements.metaProfile?.textContent || "",
    payloadContainer: elements.metaContainer?.textContent || "",
    releaseId: elements.metaRelease?.textContent || "",
    recordHash: state.recordHash,
    recordImageUrl: state.recordObjectUrl,
    rotationDegrees: state.rotation,
    sampleRate: state.sampleRate,
    positionFrames: state.positionFrames
  });
}

function publishState() {
  const snapshot = publicState();
  for (const listener of state.listeners) listener(snapshot);
}

async function setVolume(value) {
  state.volume = Math.max(0, Math.min(1, Number(value) || 0));
  if (elements.volume) elements.volume.value = String(state.volume);
  await dispatch({ type: "set_channel_gain", deck: "a", value: state.volume });
}

async function setCrossfader(value) {
  state.crossfader = Math.max(0, Math.min(1, Number(value) || 0));
  if (elements.xfade) elements.xfade.value = String(state.crossfader);
  await dispatch({ type: "set_crossfader", value: state.crossfader });
}

function startScratchRecording(options = {}) {
  if (state.activeScratchRecorder?.active) throw new Error("A scratch recording is already active");
  const recorder = createScratchRecorder(options);
  recorder.start();
  state.activeScratchRecorder = recorder;
  return recorder;
}

async function stopScratchRecording({ save = true } = {}) {
  const recorder = state.activeScratchRecorder;
  if (!recorder) return null;
  const performance = recorder.stop();
  state.activeScratchRecorder = null;
  if (performance && save) await saveScratchPerformance(performance);
  return performance;
}

const api = Object.freeze({
  loadRecord: loadFile,
  play: async () => {
    await initialiseAudio();
    await state.context.resume();
    const view = deckView();
    if (!view?.playing) await dispatch({ type: "toggle_playback", deck: "a" });
  },
  pause: async () => {
    const view = deckView();
    if (view?.playing) await dispatch({ type: "toggle_playback", deck: "a" });
  },
  togglePlayback: async () => {
    await initialiseAudio();
    await state.context.resume();
    await dispatch({ type: "toggle_playback", deck: "a" });
  },
  seekSeconds: seconds => queueSeek(Math.max(0, Math.min(state.duration, Number(seconds) || 0))),
  seekRatio: ratio => queueSeek(Math.max(0, Math.min(1, Number(ratio) || 0)) * state.duration),
  setRpm,
  setVolume,
  setCrossfader,
  setNeedleLifted: async lifted => {
    const next = Boolean(lifted);
    await dispatch({ type: "set_needle", deck: "a", lifted: next, observed_playback_seconds: framesToSeconds(state.positionFrames) });
    state.node?.port.postMessage({ type: "needle", lifted: next });
  },
  beginScratch: ({ pointerId = 0, rotationDegrees = state.rotation, positionFrames = state.positionFrames, rate = 0, impulse = 0.22 } = {}) => {
    const position = Number(positionFrames) || 0;
    recordScratchEvent({ type: "scratch-start", positionFrames: position, rate: Number(rate) || 0, impulse: Number(impulse) || 0 });
    state.node?.port.postMessage({ type: "scratch", active: true, position, rate: Number(rate) || 0, impulse: Number(impulse) || 0 });
    return dispatch({ type: "begin_scratch", deck: "a", pointer_id: pointerId, playback_seconds: framesToSeconds(position), rotation_degrees: rotationDegrees });
  },
  updateScratch: ({ positionFrames, rate = 0, rotationDegrees = state.rotation, impulse = 0 } = {}) => {
    const position = Number(positionFrames) || 0;
    const nextRate = Number(rate) || 0;
    const nextImpulse = Number(impulse) || 0;
    recordScratchEvent({ type: "scratch-motion", positionFrames: position, rate: nextRate, impulse: nextImpulse });
    state.node?.port.postMessage({ type: "motion", position, rate: nextRate, impulse: nextImpulse });
    return dispatch({ type: "move_scratch", deck: "a", position_frames: position, rendered_position_frames: position, rate: nextRate, rotation_degrees: Number(rotationDegrees) || 0, impulse: nextImpulse });
  },
  endScratch: ({ rotationDegrees = state.rotation, resumePlayback = true } = {}) => {
    recordScratchEvent({ type: "scratch-end", positionFrames: state.positionFrames, rate: 0, impulse: 0, resumePlayback: Boolean(resumePlayback) });
    state.node?.port.postMessage({ type: "scratch", active: false, position: state.positionFrames, rate: 0, impulse: 0 });
    return dispatch({ type: "end_scratch", deck: "a", rendered_position_frames: state.positionFrames, rotation_degrees: rotationDegrees, resume_playback: Boolean(resumePlayback), save_sample: false, can_platter_handoff: true });
  },
  createScratchRecorder,
  startScratchRecording,
  stopScratchRecording,
  replayScratch,
  cancelScratchReplay,
  scratches: Object.freeze({
    save: saveScratchPerformance,
    get: getScratchPerformance,
    list: query => listScratchPerformances({ recordHash: state.recordHash, ...(query || {}) }),
    delete: deleteScratchPerformance,
    clear: query => clearScratchPerformances({ recordHash: state.recordHash, ...(query || {}) }),
    export: performance => JSON.stringify(performance),
    import: value => typeof value === "string" ? JSON.parse(value) : structuredClone(value)
  }),
  getState: publicState,
  subscribe(listener) { state.listeners.add(listener); listener(publicState()); return () => state.listeners.delete(listener); },
  canvas: Object.freeze({
    mount(canvas, options = {}) {
      state.canvasController?.destroy();
      state.canvasController = createVinylPlayerCanvas(api, canvas, options);
      return state.canvasController;
    },
    configure(options = {}) {
      if (!state.canvasController) throw new Error("No player canvas is mounted");
      return state.canvasController.configure(options);
    },
    setComponentVisible(name, visible) {
      if (!state.canvasController) throw new Error("No player canvas is mounted");
      return state.canvasController.setComponentVisible(name, visible);
    },
    setTheme(theme) {
      if (!state.canvasController) throw new Error("No player canvas is mounted");
      return state.canvasController.setTheme(theme);
    },
    setStrobeLight(enabled) {
      if (!state.canvasController) throw new Error("No player canvas is mounted");
      return state.canvasController.setStrobeLight(enabled);
    },
    getConfig() {
      return state.canvasController?.getConfig() || null;
    },
    destroy() {
      state.canvasController?.destroy();
      state.canvasController = null;
    }
  })
});

globalThis.vin ??= {};
globalThis.vin.yl ??= {};
globalThis.vin.yl.player = api;

async function initialise() {
  state.worker = new Worker("./player-core-worker.js", { type: "module" });
  state.worker.onmessage = event => {
    const { id, ok, result, error } = event.data;
    const request = state.pending.get(id);
    if (!request) return;
    state.pending.delete(id);
    if (ok) request.resolve(result);
    else request.reject(new Error(error));
  };
  const result = await coreRequest("init", { moduleUrl: "./wasm/record-player/record_player.js" });
  state.view = result.view;
  render();
  const canvas = document.querySelector("#player-canvas");
  if (canvas) state.canvasController = createVinylPlayerCanvas(api, canvas);
  globalThis.dispatchEvent(new CustomEvent("vin.yl.player.ready", { detail: api }));
}

elements.load.addEventListener("click", () => elements.file.click());
elements.file.addEventListener("change", () => {
  const file = elements.file.files?.[0];
  if (file) void loadFile(file).catch(error => setStatus(error.message));
});
elements.play.addEventListener("click", async () => {
  await initialiseAudio();
  await state.context.resume();
  await dispatch({ type: "toggle_playback", deck: "a" });
});
elements.needle.addEventListener("click", () => {
  const view = deckView();
  if (!view) return;
  void dispatch({ type: "set_needle", deck: "a", lifted: !view.needle_lifted, observed_playback_seconds: framesToSeconds(state.positionFrames) });
  state.node.port.postMessage({ type: "needle", lifted: !view.needle_lifted });
});
elements.seek.addEventListener("pointerdown", () => { state.draggingSeek = true; });
elements.seek.addEventListener("input", () => {
  const seconds = Number(elements.seek.value) * state.duration;
  queueSeek(seconds);
});
elements.seek.addEventListener("change", () => {
  state.draggingSeek = false;
  const seconds = Number(elements.seek.value) * state.duration;
  state.queuedSeekSeconds = seconds;
  void flushQueuedSeek();
});
elements.seek.addEventListener("pointerup", () => { state.draggingSeek = false; });
elements.rpm33?.addEventListener("click", () => void setRpm(33.3333333333));
elements.rpm45?.addEventListener("click", () => void setRpm(45));
elements.rpm?.addEventListener("input", () => void setRpm(elements.rpm.value));
elements.volume?.addEventListener("input", () => void setVolume(elements.volume.value));
elements.xfade?.addEventListener("input", () => void setCrossfader(elements.xfade.value));
elements.platter.addEventListener("pointerdown", event => void beginScratch(event));
elements.platter.addEventListener("pointermove", moveScratch);
elements.platter.addEventListener("pointerup", event => void endScratch(event));
elements.platter.addEventListener("pointercancel", event => void endScratch(event));

initialise().catch(error => setStatus(error.message));
