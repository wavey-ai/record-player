import { RecordDecoderClient } from "./record-decoder-client.js";
import { readPcmCache, recordCacheKey, writePcmCache } from "./pcm-cache.js";

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
  rpm45: document.querySelector("#rpm-45")
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
  queuedSeekSeconds: null
};

function setStatus(message) {
  elements.status.value = message;
  elements.status.textContent = message;
}

function resetProgressiveStream() {
  state.streamInitialised = false;
  state.streamReady = false;
  state.streamDecodedFrames = 0;
  state.streamReadyPromise = null;
}

function makeProgressivePlaybackReady() {
  if (state.streamReady || state.streamReadyPromise) return;
  state.streamReadyPromise = (async () => {
    await dispatch({
      type: "set_load_state",
      deck: "a",
      status: "ready",
      loaded: true,
      duration_seconds: state.duration
    });
    await dispatch({
      type: "set_needle",
      deck: "a",
      lifted: false,
      observed_playback_seconds: 0
    });
    state.node.port.postMessage({ type: "needle", lifted: false });
    state.streamReady = true;
    elements.play.disabled = false;
    elements.needle.disabled = false;
    elements.seek.disabled = false;
    setStatus(`Ready to play · ${(state.streamDecodedFrames / state.sampleRate).toFixed(1)}s buffered · decoding continues`);
  })().catch(error => {
    state.streamReadyPromise = null;
    setStatus(error.message);
  });
}

function handleDecodedSegments(progress, inspected) {
  const segments = Array.isArray(progress?.decodedPcmSegments)
    ? progress.decodedPcmSegments
    : [];
  if (!segments.length) return;

  for (const segment of segments) {
    const sampleRate = Math.max(1, Number(segment.sampleRate) || 48000);
    const audioLength = Math.max(1, Number(segment.audioLength) || 1);
    const channels = Math.max(1, Number(segment.channels) || segment.channelBuffers?.length || 2);

    if (!state.streamInitialised) {
      state.sampleRate = sampleRate;
      state.duration = audioLength / sampleRate;
      state.baseRpm = profileRpm(inspected.recordProfile);
      state.rpm = state.baseRpm;
      updateRpmButtons();
      state.positionFrames = 0;
      state.lastReportedPosition = 0;
      state.node.port.postMessage({
        type: "stream-init",
        channels,
        sampleRate,
        audioLength
      });
      state.streamInitialised = true;
    }

    const channelBuffers = Array.isArray(segment.channelBuffers)
      ? segment.channelBuffers
      : [];
    state.node.port.postMessage({
      type: "append-pcm",
      startFrame: segment.startFrame,
      endFrame: segment.endFrame,
      channelBuffers
    }, channelBuffers);

    const startFrame = Math.max(0, Number(segment.startFrame) || 0);
    const endFrame = Math.max(startFrame, Number(segment.endFrame) || startFrame);
    if (startFrame <= state.streamDecodedFrames) {
      state.streamDecodedFrames = Math.max(state.streamDecodedFrames, endFrame);
    }
  }


}

function profileRpm(recordProfile) {
  return String(recordProfile || "").toLowerCase().includes("single45") ? 45 : 33.3333333333;
}

function updateRpmButtons() {
  elements.rpm33?.classList.toggle("selected", Math.abs(state.rpm - 33.3333333333) < 0.01);
  elements.rpm45?.classList.toggle("selected", Math.abs(state.rpm - 45) < 0.01);
}

async function setRpm(rpm) {
  state.rpm = rpm;
  updateRpmButtons();
  const rate = rpm / state.baseRpm;
  await dispatch({ type: "set_playback_rate", deck: "a", rate });
}

function loadCachedPcm(cached) {
  const sampleRate = Math.max(1, Number(cached.sampleRate) || 48000);
  const audioLength = Math.max(1, Number(cached.audioLength) || 0);
  const sourceBuffers = Array.isArray(cached.s16ChannelBuffers) ? cached.s16ChannelBuffers : [];
  if (!sourceBuffers.length) throw new Error("Cached record contains no PCM channels");
  const channels = [];
  const transfers = [];
  for (let index = 0; index < 2; index += 1) {
    const source = new Int16Array(sourceBuffers[Math.min(index, sourceBuffers.length - 1)]);
    const channel = new Float32Array(audioLength);
    const limit = Math.min(audioLength, source.length);
    for (let frame = 0; frame < limit; frame += 1) channel[frame] = source[frame] / 32768;
    channels.push(channel.buffer);
    transfers.push(channel.buffer);
  }
  state.sampleRate = sampleRate;
  state.duration = audioLength / sampleRate;
  state.positionFrames = 0;
  state.lastReportedPosition = 0;
  state.node.port.postMessage({ type: "load", channels, sampleRate }, transfers);
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
  }
}

function seekWorklet(position) {
  const generation = ++state.pendingSeekGeneration;
  state.positionFrames = position;
  state.node.port.postMessage({ type: "seek", position, generation });
}

async function initialiseAudio() {
  if (state.context) return;
  state.context = new AudioContext({ latencyHint: "interactive" });
  const simulationWasmResponse = await fetch("./wasm/record-player-simulation/record_player_simulation_wasm_bg.wasm");
  if (!simulationWasmResponse.ok) throw new Error(`Failed to load scratch simulation WASM: ${simulationWasmResponse.status}`);
  const simulationWasmModule = await WebAssembly.compileStreaming(simulationWasmResponse);
  await state.context.audioWorklet.addModule("./player-worklet.js");
  state.node = new AudioWorkletNode(state.context, "bitneedle-player", {
    numberOfInputs: 0,
    numberOfOutputs: 1,
    outputChannelCount: [2],
    processorOptions: { wasmModule: simulationWasmModule }
  });
  state.node.connect(state.context.destination);
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
  } else if (message.type === "seeked") {
    state.acknowledgedSeekGeneration = Math.max(state.acknowledgedSeekGeneration, message.generation ?? 0);
    state.positionFrames = message.position;
  } else if (message.type === "ended") {
    state.positionFrames = message.position;
    void dispatch({ type: "playback_ended", deck: "a" });
  }
}

async function loadFile(file) {
  await initialiseAudio();
  await state.context.resume();
  state.decoder ??= new RecordDecoderClient();
  await state.decoder.initialise();
  resetProgressiveStream();
  elements.play.disabled = true;
  elements.needle.disabled = true;
  elements.seek.disabled = true;
  if (state.recordObjectUrl) URL.revokeObjectURL(state.recordObjectUrl);
  state.recordObjectUrl = URL.createObjectURL(file);
  elements.recordImage.src = state.recordObjectUrl;
  setStatus(`Inspecting ${file.name}…`);
  const sourceBytes = await file.arrayBuffer();
  const cacheKey = await recordCacheKey(sourceBytes);
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
    loadCachedPcm(cached);
    await markLoadedReady();
    setStatus(`${file.name} · ${state.duration.toFixed(1)}s · PCM cache hit`);
    return;
  }
  setStatus(`Decoding ${file.name}…`);
  const decoded = await state.decoder.decode(sourceBytes, inspected.recordProfile || "", progress => {
    handleDecodedSegments(progress, inspected);
    const message = progress.msg || progress.message || progress.status;
    if (message && !state.streamReady) setStatus(String(message));
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

  if (state.streamInitialised) {
    state.node.port.postMessage({ type: "stream-complete" });
    if (!state.streamReady) {
      await markLoadedReady();
      state.streamReady = true;
    }
    setStatus(`${file.name} · ${state.duration.toFixed(1)}s · fully decoded`);
    return;
  }

  const sampleRate = Math.max(1, Number(decoded.sampleRate) || 48000);
  const audioLength = Math.max(1, Number(decoded.audioLength) || 0);
  const s16Buffers = Array.isArray(decoded.s16ChannelBuffers) ? decoded.s16ChannelBuffers : [];
  if (!s16Buffers.length) throw new Error("Record decoder returned no PCM channels");
  const channels = [];
  const transfers = [];
  for (let index = 0; index < Math.max(2, Math.min(2, s16Buffers.length)); index += 1) {
    const source = new Int16Array(s16Buffers[Math.min(index, s16Buffers.length - 1)]);
    const channel = new Float32Array(audioLength);
    const limit = Math.min(audioLength, source.length);
    for (let frame = 0; frame < limit; frame += 1) channel[frame] = source[frame] / 32768;
    channels.push(channel.buffer);
    transfers.push(channel.buffer);
  }
  state.sampleRate = sampleRate;
  state.baseRpm = profileRpm(inspected.recordProfile);
  state.rpm = state.baseRpm;
  updateRpmButtons();
  state.duration = audioLength / sampleRate;
  state.positionFrames = 0;
  state.lastReportedPosition = 0;
  state.node.port.postMessage({ type: "load", channels, sampleRate }, transfers);
  await markLoadedReady();
  setStatus(`${file.name} · ${state.duration.toFixed(1)}s · ${sampleRate} Hz · ${inspected.payloadContainer || "record"}`);
}

function render() {
  const view = deckView();
  if (!view) return;
  elements.play.textContent = view.playing ? "STOP" : "START";
  elements.needle.textContent = view.needle_lifted ? "NEEDLE DOWN" : "NEEDLE UP";
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
  state.node.port.postMessage({ type: "scratch", active: true, position, rate, impulse: Math.min(1, Math.abs(rate) / 3) });
  void dispatch({
    type: "move_scratch",
    deck: "a",
    position_frames: position,
    rendered_position_frames: position,
    rate,
    rotation_degrees: state.rotation,
    impulse: Math.min(1, Math.abs(rate) / 3)
  });
  state.scratchLastAngle = angle;
  state.scratchLastTime = event.timeStamp;
}

async function endScratch(event) {
  if (!state.scratching || event.pointerId !== state.scratchPointerId) return;
  state.scratching = false;
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
  const result = await coreRequest("init", { moduleUrl: "./wasm/bitneedle_record_player_core.js" });
  state.view = result.view;
  render();
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
elements.platter.addEventListener("pointerdown", event => void beginScratch(event));
elements.platter.addEventListener("pointermove", moveScratch);
elements.platter.addEventListener("pointerup", event => void endScratch(event));
elements.platter.addEventListener("pointercancel", event => void endScratch(event));

initialise().catch(error => setStatus(error.message));
