import { createLogger, setPlayerLoggingEnabled, isPlayerLoggingEnabled, setPlayerLogLevel, getPlayerLogLevel, setPlayerTelemetryInterval } from "./player-message-logger.js";
import { createVinylPlayerCanvas } from "./player-canvas.js";
import { RecordDecoderClient } from "./record-decoder-client.js";
import { createPcmChunkCacheHandler, recordCacheKey } from "./pcm-cache.js";
import { createRemoteOpusChunkCacheHandler, createRemoteOpusPrecache, decodeRecordDescriptorJson } from "./opus-cache.js";
import { clearScratchPerformances, deleteScratchPerformance, getScratchPerformance, listScratchPerformances, saveScratchPerformance } from "./scratch-performance-store.js";

const log = createLogger("host");
const initialLoggingParam = new URLSearchParams(globalThis.location?.search || "").get("player_log");
if (initialLoggingParam === "0") setPlayerLoggingEnabled(false);
if (initialLoggingParam === "1") setPlayerLoggingEnabled(true);

function queryParams() {
  return new URLSearchParams(globalThis.location?.search || "");
}

function isEmbedMode() {
  return document.documentElement.classList.contains("embed-mode");
}

// Embed query params (bg/tone/turntable/controls/status/light/strobe/dots/
// arm/arc/load) are read once at startup by embedCanvasOptions() below and
// by embed.html's own inline script (CSS vars + hide-controls/hide-status
// classes). liveEmbedOverrides lets a same-page or iframe host update those
// same params afterwards via postMessage, without a full iframe reload —
// effectiveParam() prefers a live override over the original URL value.
const liveEmbedOverrides = {};

function effectiveParam(name) {
  if (Object.prototype.hasOwnProperty.call(liveEmbedOverrides, name)) return liveEmbedOverrides[name];
  return queryParams().get(name);
}

function startupRecordSrc() {
  const value = String(queryParams().get("src") || "").trim();
  console.log("[vin.yl.player] startupRecordSrc", {
    href: globalThis.location?.href || "",
    src: value,
  });
  return value || "";
}

function startupCacheUrl() {
  const params = queryParams();
  const value = String(params.get("tape_url") || "").trim();
  console.log("[vin.yl.player] startupCacheUrl", {
    href: globalThis.location?.href || "",
    tapeUrl: value,
  });
  if (!value) {
    console.warn("[vin.yl.player] tape disabled: missing tape_url query param");
  }
  return value || "";
}

function embedParamColor(name, fallback = "") {
  const value = String(effectiveParam(name) || "").trim();
  if (!value) return fallback;
  if (value === "black") return "#000000";
  if (!/^#?[0-9a-fA-F]{3,8}$/.test(value)) return fallback;
  return value.startsWith("#") ? value : `#${value}`;
}

function embedCanvasOptions() {
  if (!document.documentElement.classList.contains("embed-mode")) {
    return null;
  }
  const tone = embedParamColor("tone", "#050505");
  const turntable = embedParamColor("turntable", tone);
  const controlsHidden = effectiveParam("controls") === "0";
  return {
    theme: {
      line: tone,
      mutedLine: `${tone}47`,
      controlFill: `${tone}1a`,
      controlActive: tone,
      controlText: tone,
      controlActiveText: embedParamColor("bg", "#f00020"),
      turntableRing: `${turntable}33`,
      turntableRingStrong: `${turntable}66`,
      tonearm: tone,
      tonearmGuide: tone,
      stylus: tone,
      stylusGlow: `${tone}b3`,
      syncDot: `${tone}3b`,
    },
    loadOnEmptyRecordTap: effectiveParam("load") === "1",
    // controls=0 strips the turntable down to just the record disc + spindle.
    // Individual overlay elements can then be opted back in one at a time via
    // their own params (each defaults off when controls are hidden):
    //   light=1   the strobe lamp/light fixture + beam
    //   strobe=1  the lit strobe sampling dots
    //   dots=1    the base sync dots ring
    //   arm=1     the tonearm + needle point
    //   arc=1     the dotted travel-guide arc
    // Always spelled out explicitly (not `{}` when controls are shown) so a
    // live bitneedle-set-embed-options update can flip controls back on
    // after they were hidden, not just off — canvas.configure() merges
    // partial patches on top of the current components, so an empty object
    // here would leave a previously-hidden overlay stuck hidden.
    components: controlsHidden ? {
      startStop: false,
      needle: false,
      loadRecord: false,
      rpm: false,
      volume: false,
      crossfader: false,
      seek: false,
      labels: false,
      strobeLamp: effectiveParam("light") === "1",
      strobe: effectiveParam("strobe") === "1",
      syncDots: effectiveParam("dots") === "1",
      stylus: effectiveParam("arm") === "1",
      needlePoint: effectiveParam("arm") === "1",
      tonearmGuide: effectiveParam("arc") === "1",
    } : {
      startStop: true,
      needle: true,
      loadRecord: true,
      rpm: true,
      volume: true,
      crossfader: true,
      seek: true,
      labels: true,
      strobeLamp: true,
      strobe: true,
      syncDots: true,
      stylus: true,
      needlePoint: true,
      tonearmGuide: true,
    },
  };
}

// Mirrors embed.html's own inline <head> script, which sets these CSS vars
// and classes once at first paint (before this module has loaded) to avoid
// a flash of the wrong theme. This copy re-applies the same rules whenever
// a live bitneedle-set-embed-options message changes bg/tone/turntable/
// controls/status, since that script only ever runs once.
function normalizeEmbedHex(value) {
  if (!value) return null;
  if (value === "black") return "#000000";
  if (!/^#?[0-9a-fA-F]{3,8}$/.test(value)) return null;
  return value.charAt(0) === "#" ? value : `#${value}`;
}

function applyEmbedChrome() {
  if (!document.documentElement.classList.contains("embed-mode")) return;
  const root = document.documentElement;
  const bg = effectiveParam("bg");
  if (bg === "transparent") {
    root.classList.add("embed-transparent-bg");
  } else {
    root.classList.remove("embed-transparent-bg");
    const bgColor = normalizeEmbedHex(bg);
    if (bgColor) root.style.setProperty("--embed-bg", bgColor);
  }
  const tone = normalizeEmbedHex(effectiveParam("tone"));
  if (tone) root.style.setProperty("--embed-tone", tone);
  const turntable = normalizeEmbedHex(effectiveParam("turntable"));
  if (turntable) root.style.setProperty("--embed-turntable", turntable);
  root.classList.toggle("embed-hide-controls", effectiveParam("controls") === "0");
  root.classList.toggle("embed-hide-status", effectiveParam("status") === "0");
}

// Live counterpart to the URL-param-driven embed setup: merges a patch of
// the same param names (bg/tone/turntable/controls/status/light/strobe/
// dots/arm/arc/load) into liveEmbedOverrides, then re-applies chrome (CSS
// vars/classes), canvas theme and component visibility, and status text
// visibility — all without touching the iframe's src, so a hard reload
// (and the record-loss it causes) is never required for these changes.
function applyEmbedOptions(patch = {}) {
  Object.assign(liveEmbedOverrides, patch);
  applyEmbedChrome();
  const options = embedCanvasOptions();
  if (options && state.canvasController) state.canvasController.configure(options);
}

// Original player-environment-config.js mobile detection, reproduced exactly:
// UA sniff plus iPadOS-style MacIntel-with-touch WebKit.
const IS_IOS_WEBKIT =
  /iP(ad|hone|od)/i.test(navigator.userAgent || "") ||
  ((navigator.platform || "") === "MacIntel" && (navigator.maxTouchPoints || 0) > 1);
const IS_MOBILE_DEVICE = /Mobi|Android|iPhone|iPad|iPod|Mobile/i.test(navigator.userAgent || "") || IS_IOS_WEBKIT;
// Original resolveNeedleSurfaceGain: surface foley is 2.25× louder on mobile speakers.
const MOBILE_SURFACE_GAIN_MULTIPLIER = 2.25;
// Original profile_turns(): lead-in and deadwax both traverse 2 revolutions.
const DEADWAX_TURNS = 2;

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
  lastCoreObservedPositionFrames: -1,
  lastCoreObservedAtMs: 0,
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
  canvasController: null,
  streamInitialised: false,
  streamReady: false,
  streamDecodedFrames: 0,
  streamAppendChain: Promise.resolve(),
  decodeProgressText: "",
  regionTimer: 0,
  needleAutoBehaviorEnabled: true,
  cacheHandler: null,
  postMessageBridge: null,
  programmeMap: null,
  recordHeaderProof: null,
  recordDescriptorJson: "",
};

const DEFAULT_POST_MESSAGE_BRIDGE = Object.freeze({
  enabled: false,
  targetOrigin: "*",
  targetWindow: () => globalThis.parent !== globalThis ? globalThis.parent : null,
  outbound: Object.freeze({
    playbackType: "bitneedle-embed-playback",
    recordType: "bitneedle-embed-record",
  }),
  inbound: Object.freeze({
    setPlayingType: "bitneedle-set-playing",
    setVolumeType: "bitneedle-set-volume",
    seekType: "bitneedle-seek",
    trackStepType: "bitneedle-track-step",
    setRpmType: "bitneedle-set-rpm",
    setCrossfaderType: "bitneedle-set-crossfader",
    setNeedleLiftedType: "bitneedle-set-needle-lifted",
    // Live counterpart to the ?bg/tone/turntable/controls/status/light/
    // strobe/dots/arm/arc/load embed query params — lets an embedder change
    // any of them after the iframe is already loaded, without reloading it
    // (a reload drops whatever record was loaded via loadRecordBytesType).
    setEmbedOptionsType: "bitneedle-set-embed-options",
    // Lets a same-page host (e.g. an editor with an unpublished record
    // only available as a local blob: URL, which a cross-origin iframe
    // can't fetch) hand over the record's raw bytes directly instead of
    // going through loadRecordFromUrl's fetch(). bytes is an ArrayBuffer
    // (structured-clonable, no base64 needed).
    loadRecordBytesType: "bitneedle-load-record-bytes",
  }),
  formatPlayback: snapshot => ({
    isPlaying: Boolean(snapshot.playing),
    currentTime: Number(snapshot.positionSeconds) || 0,
    duration: Number(snapshot.durationSeconds) || 0,
    volume: Math.max(0, Math.min(1, Number(snapshot.volume) || 0)),
  }),
  formatRecord: snapshot => snapshot.ready ? {
    title: snapshot.releaseId || "",
    releaseId: snapshot.releaseId || "",
    recordProfile: snapshot.recordProfile || "",
    payloadContainer: snapshot.payloadContainer || "",
    recordHash: snapshot.recordHash || "",
    trackIndex: Number.isFinite(snapshot.currentTrackIndex) ? snapshot.currentTrackIndex : -1,
    trackCount: Number(snapshot.trackCount) || 0,
    trackTitle: snapshot.currentTrackTitle || "",
  } : null,
});

let lastBridgePlaybackKey = "";
let lastBridgeRecordKey = "";
let lastBridgePlaybackAt = 0;

function currentPostMessageBridge() {
  return state.postMessageBridge || DEFAULT_POST_MESSAGE_BRIDGE;
}

function normalizePostMessageBridge(config = {}) {
  if (config == null || config === false) return { ...DEFAULT_POST_MESSAGE_BRIDGE, enabled: false };
  const next = {
    ...DEFAULT_POST_MESSAGE_BRIDGE,
    ...config,
    enabled: config.enabled !== false,
    outbound: { ...DEFAULT_POST_MESSAGE_BRIDGE.outbound, ...(config.outbound || {}) },
    inbound: { ...DEFAULT_POST_MESSAGE_BRIDGE.inbound, ...(config.inbound || {}) },
  };
  if (typeof next.targetWindow !== "function") {
    const target = next.targetWindow || null;
    next.targetWindow = () => target;
  }
  if (typeof next.formatPlayback !== "function") next.formatPlayback = DEFAULT_POST_MESSAGE_BRIDGE.formatPlayback;
  if (typeof next.formatRecord !== "function") next.formatRecord = DEFAULT_POST_MESSAGE_BRIDGE.formatRecord;
  return next;
}

function bridgePost(type, payload) {
  const bridge = currentPostMessageBridge();
  if (!bridge.enabled) return false;
  const target = bridge.targetWindow?.();
  if (!target || typeof target.postMessage !== "function") return false;
  target.postMessage({ type, ...payload }, bridge.targetOrigin || "*");
  return true;
}

function reportBridgePlayback(snapshot = publicState()) {
  const bridge = currentPostMessageBridge();
  if (!bridge.enabled) return;
  const payload = bridge.formatPlayback(snapshot);
  if (!payload || typeof payload !== "object") return;
  const now = Date.now();
  const key = `${Boolean(payload.isPlaying)}|${Number(payload.volume) || 0}|${Number(payload.duration) || 0}|${Math.round((Number(payload.currentTime) || 0) * 2)}`;
  if (key === lastBridgePlaybackKey && now - lastBridgePlaybackAt < 500) return;
  lastBridgePlaybackKey = key;
  lastBridgePlaybackAt = now;
  bridgePost(bridge.outbound.playbackType, payload);
}

function reportBridgeRecord(snapshot = publicState()) {
  const bridge = currentPostMessageBridge();
  if (!bridge.enabled) return;
  const payload = bridge.formatRecord(snapshot);
  const key = payload ? JSON.stringify(payload) : "";
  if (key === lastBridgeRecordKey) return;
  lastBridgeRecordKey = key;
  bridgePost(bridge.outbound.recordType, { record: payload });
}

async function handleBridgeMessage(event) {
  const bridge = currentPostMessageBridge();
  if (!bridge.enabled) return;
  const message = event.data || {};
  try {
    if (message.type === bridge.inbound.setPlayingType) {
      if (Boolean(message.playing)) await api.play();
      else await api.pause();
      return;
    }
    if (message.type === bridge.inbound.setVolumeType) {
      await api.setVolume(message.volume);
      return;
    }
    if (message.type === bridge.inbound.seekType) {
      api.seekRatio(message.ratio);
      return;
    }
    if (message.type === bridge.inbound.trackStepType) {
      api.stepTrack(Number(message.direction) >= 0 ? 1 : -1);
      return;
    }
    if (message.type === bridge.inbound.setRpmType) {
      await api.setRpm(message.rpm);
      return;
    }
    if (message.type === bridge.inbound.setCrossfaderType) {
      await api.setCrossfader(message.crossfader);
      return;
    }
    if (message.type === bridge.inbound.setNeedleLiftedType) {
      await api.setNeedleLifted(Boolean(message.lifted));
      return;
    }
    if (message.type === bridge.inbound.setEmbedOptionsType) {
      applyEmbedOptions(message.options || {});
      return;
    }
    if (message.type === bridge.inbound.loadRecordBytesType) {
      const bytes = message.bytes;
      if (!(bytes instanceof ArrayBuffer)) throw new Error("loadRecordBytesType message.bytes must be an ArrayBuffer");
      const file = new File([bytes], String(message.name || "record.png"), { type: String(message.mime || "image/png") });
      await loadFile(file, { resumeAudio: false });
    }
  } catch (error) {
    log.warn("post-message-bridge-error", { type: message.type, message: error instanceof Error ? error.message : String(error) });
  }
}

function parseJsonObject(json, fallback = null) {
  if (!json) return fallback;
  try {
    const parsed = JSON.parse(json);
    return parsed && typeof parsed === "object" ? parsed : fallback;
  } catch {
    return fallback;
  }
}

function programmeTracks() {
  const tracks = state.programmeMap?.tracks;
  return Array.isArray(tracks) ? tracks : [];
}

function trackSeconds(track) {
  const startSamples = Number(track?.startSamples ?? track?.start ?? track?.sampleStart);
  const endSamples = Number(track?.endSamples ?? track?.end ?? track?.sampleEnd);
  const startSeconds = Number(track?.startSeconds);
  const endSeconds = Number(track?.endSeconds);
  const sampleRate = Math.max(1, Number(state.sampleRate) || 48000);
  return {
    start: Number.isFinite(startSeconds) ? startSeconds : (Number.isFinite(startSamples) ? Math.max(0, startSamples / sampleRate) : 0),
    end: Number.isFinite(endSeconds) ? endSeconds : (Number.isFinite(endSamples) ? Math.max(0, endSamples / sampleRate) : 0),
  };
}

function currentTrackIndex() {
  const tracks = programmeTracks();
  if (!tracks.length) return -1;
  const current = framesToSeconds(state.positionFrames);
  for (let index = 0; index < tracks.length; index += 1) {
    const span = trackSeconds(tracks[index]);
    if (current >= span.start && current < span.end) return index;
  }
  return -1;
}

function stepTrack(direction = 1) {
  const tracks = programmeTracks();
  if (!tracks.length) return false;
  const delta = direction >= 0 ? 1 : -1;
  const index = currentTrackIndex();
  const fallback = delta > 0 ? -1 : tracks.length;
  const targetIndex = (index === -1 ? fallback : index) + delta;
  const target = tracks[targetIndex];
  if (!target) return false;
  const span = trackSeconds(target);
  api.seekSeconds(span.start);
  return true;
}

function normalizeCacheHandler(handler) {
  if (handler == null) return null;
  if (typeof handler !== "object") throw new Error("Cache handler must be an object.");
  if (typeof handler.get !== "function" || typeof handler.put !== "function") {
    throw new Error("Cache handler must provide get(key, meta) and put(key, pcm).");
  }
  return handler;
}

function setStatus(message) {
  log.state("status", { message });
  elements.status.value = message;
  elements.status.textContent = message;
}


function formatDecodeProgress(progress) {
  const message = String(progress?.msg || progress?.message || progress?.status || "Decoding groove audio").trim();
  const percent = Number(progress?.progressPercent);
  const processed = Number(progress?.processedSeconds);
  const duration = Number(progress?.duration);
  const details = [];
  if (Number.isFinite(percent)) details.push(`${Math.max(0, Math.min(100, Math.round(percent)))}%`);
  if (Number.isFinite(processed) && Number.isFinite(duration) && duration > 0) {
    details.push(`${processed.toFixed(1)}/${duration.toFixed(1)}s`);
  }
  return details.length ? `${message} · ${details.join(" · ")}` : message;
}

function renderDecodeStatus() {
  if (!state.decodeProgressText) return;
  setStatus(state.streamReady ? `Ready · ${state.decodeProgressText}` : state.decodeProgressText);
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

async function initialiseProgressiveStream({ sampleRate, audioLength, channels }) {
  if (state.streamInitialised) return;
  const channelCount = Math.max(1, Math.min(2, Number(channels) || 2));
  state.sampleRate = Math.max(1, Number(sampleRate) || 48000);
  state.duration = Math.max(1, Number(audioLength) || 1) / state.sampleRate;
  state.positionFrames = 0;
  state.lastReportedPosition = 0;
  state.streamDecodedFrames = 0;
  log.send("worklet:stream-init", { sampleRate: state.sampleRate, audioLength: Math.max(1, Number(audioLength) || 1), channels: channelCount });
  state.node.port.postMessage({
    type: "stream-init",
    sampleRate: state.sampleRate,
    audioLength: Math.max(1, Number(audioLength) || 1),
    channels: channelCount
  });
  state.streamInitialised = true;
}

async function appendProgressiveSegments(segments) {
  if (!Array.isArray(segments) || !segments.length) return;
  const first = segments[0] || {};
  await initialiseProgressiveStream({
    sampleRate: first.sampleRate,
    audioLength: first.audioLength,
    channels: first.channels
  });
  for (const segment of segments) {
    const channelBuffers = Array.isArray(segment.channelBuffers) ? segment.channelBuffers : [];
    if (!channelBuffers.length) continue;
    const startFrame = Math.max(0, Math.floor(Number(segment.startFrame ?? segment.offset ?? 0) || 0));
    const inferredFrames = new Int16Array(channelBuffers[0]).length;
    const endFrame = Math.max(startFrame, Math.floor(Number(segment.endFrame) || (startFrame + inferredFrames)));
    log.send("worklet:append-pcm", { startFrame, endFrame, channels: channelBuffers.length, bytes: channelBuffers.reduce((sum, buffer) => sum + (buffer?.byteLength || 0), 0) });
    state.node.port.postMessage({
      type: "append-pcm",
      startFrame,
      endFrame,
      channelBuffers
    }, channelBuffers);
  }
}

// Matches the worklet's own resume-from-underrun threshold (player-worklet.js
// `waitingForData && decodedLength > lastPosition + 1024`) — the smallest
// buffer depth the engine already treats as safe to start from.
const PROGRESSIVE_READY_THRESHOLD_FRAMES = 1024;

async function handleWorkletBuffered(message) {
  state.streamDecodedFrames = Math.max(0, Math.floor(Number(message.decodedLength) || 0));
  if (state.streamReady) return;
  const totalFrames = Math.max(1, Math.round(state.duration * state.sampleRate));
  const contiguousReady = state.streamDecodedFrames >= Math.min(totalFrames, PROGRESSIVE_READY_THRESHOLD_FRAMES);
  if (contiguousReady) {
    state.streamReady = true;
    await markLoadedReady();
    renderDecodeStatus();
  }
}

async function loadDecodedPcm({ sampleRate, audioLength, s16ChannelBuffers }) {
  const sourceBuffers = Array.isArray(s16ChannelBuffers) ? s16ChannelBuffers : [];
  if (!sourceBuffers.length) throw new Error("Record contains no PCM channels");
  await initialiseProgressiveStream({
    sampleRate,
    audioLength,
    channels: sourceBuffers.length
  });
  const endFrame = Math.max(1, Number(audioLength) || new Int16Array(sourceBuffers[0]).length);
  state.node.port.postMessage({
    type: "append-pcm",
    startFrame: 0,
    endFrame,
    channelBuffers: sourceBuffers
  }, sourceBuffers);
  state.streamDecodedFrames = endFrame;
}

async function markLoadedReady() {
  await dispatch({ type: "set_load_state", deck: "a", status: "ready", loaded: true, duration_seconds: state.duration });
  elements.play.disabled = false;
  elements.needle.disabled = false;
  elements.seek.disabled = false;
}

async function flushQueuedSeek() {
  if (state.seekInFlight || state.queuedSeekSeconds == null) return;
  let seconds = state.queuedSeekSeconds;
  state.queuedSeekSeconds = null;
  state.seekInFlight = true;
  // Original seekPlaybackToRatio: cueing a spinning record by eye lands
  // 50–140 ms early (DJs aim ahead of the beat) and plays the needle-drop
  // foley while the stylus settles.
  if (deckView()?.playing) {
    seconds = Math.max(0, seconds - (0.05 + Math.random() * 0.09));
    state.node?.port.postMessage({ type: "needle-drop" });
  }
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
    state.pending.set(id, { resolve, reject, type, startedAt: performance.now() });
    log.send(`core:${type}`, { id, payload });
    state.worker.postMessage({ id, type, payload });
  });
}

async function dispatch(event) {
  log.action(`dispatch:${event?.type || "unknown"}`, event);
  const result = await coreRequest("dispatch", { event });
  state.view = result.view;
  for (const command of result.commands) executeCommand(command);
  render();
  await maybeAutoLowerNeedle();
}

async function maybeAutoLowerNeedle() {
  if (!state.needleAutoBehaviorEnabled) return;
  const view = deckView();
  if (!view || !view.transport_on || !view.loaded || !view.needle_lifted) return;
  await dispatch({ type: "set_needle", deck: "a", lifted: false, observed_playback_seconds: framesToSeconds(state.positionFrames) });
  state.node?.port.postMessage({ type: "needle", lifted: false });
  state.node?.port.postMessage({ type: "needle-drop" });
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
  log.action(`command:${command?.type || "unknown"}`, command);
  if (!state.node) { log.warn("command-without-worklet", command); return; }
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
  } else if (command.type === "start_surface_region") {
    const durationSeconds = Math.max(0, Number(command.duration_seconds) || 0);
    state.node.port.postMessage({ type: "surface-region", action: "start", region: command.region, durationSeconds });
    clearTimeout(state.regionTimer);
    state.regionTimer = setTimeout(() => {
      void dispatch({ type: "timed_region_elapsed", region: command.region });
    }, durationSeconds * 1000);
  } else if (command.type === "stop_surface_region") {
    clearTimeout(state.regionTimer);
    state.node.port.postMessage({ type: "surface-region", action: "stop", region: command.region });
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
  const recordPlayerWasmResponse = await fetch("./wasm/record-player/record_player_bg.wasm");
  if (!recordPlayerWasmResponse.ok) throw new Error(`Failed to load record-player WASM: ${recordPlayerWasmResponse.status}`);
  const recordPlayerWasmModule = await WebAssembly.compileStreaming(recordPlayerWasmResponse);
  await state.context.audioWorklet.addModule("./player-worklet.js");
  state.node = new AudioWorkletNode(state.context, "bitneedle-player", {
    numberOfInputs: 0,
    numberOfOutputs: 1,
    outputChannelCount: [2],
    processorOptions: { wasmModule: recordPlayerWasmModule, loggingEnabled: isPlayerLoggingEnabled() }
  });
  state.gainNode = state.context.createGain();
  state.gainNode.gain.value = 1;
  state.node.connect(state.gainNode);
  state.gainNode.connect(state.context.destination);
  state.node.port.onmessage = event => {
    log.receive(`worklet:${event.data?.type || "message"}`, event.data);
    handleWorkletMessage(event);
  };
  if (IS_MOBILE_DEVICE) {
    state.node.port.postMessage({ type: "surface-gain", multiplier: MOBILE_SURFACE_GAIN_MULTIPLIER });
  }
  void loadNeedleSurfaceAsset();
}

// Decode the original needle-surface recording off the real-time thread and hand
// its PCM to the Rust DSP. On failure the DSP's synthetic groove noise remains the
// fallback, mirroring the original's warning path.
async function loadNeedleSurfaceAsset() {
  try {
    const response = await fetch("./assets/audio/needle-surface.opus", { cache: "force-cache" });
    if (!response.ok) throw new Error(`Needle surface audio asset failed: ${response.status} ${response.statusText}`);
    const audioBuffer = await state.context.decodeAudioData(await response.arrayBuffer());
    if (!(audioBuffer?.duration > 0)) throw new Error("Needle surface audio asset decoded empty.");
    const channels = [];
    for (let index = 0; index < Math.min(2, audioBuffer.numberOfChannels); index += 1) {
      channels.push(audioBuffer.getChannelData(index).slice().buffer);
    }
    state.node.port.postMessage({ type: "surface-asset", sampleRate: audioBuffer.sampleRate, channels }, channels);
    log.action("needle-surface-asset-loaded", { sampleRate: audioBuffer.sampleRate, frames: audioBuffer.length });
  } catch (error) {
    log.warn("needle-surface-asset-unavailable", { message: error instanceof Error ? error.message : String(error) });
    console.warn("[vin.yl.player] needle surface asset unavailable; synthesizing groove noise", error);
  }
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
      const now = performance.now();
      const deltaFrames = Math.abs(message.position - state.lastCoreObservedPositionFrames);
      const minimumDeltaFrames = Math.max(1, Math.round(state.sampleRate * 0.05));
      if (now - state.lastCoreObservedAtMs >= 250 && deltaFrames >= minimumDeltaFrames) {
        state.lastCoreObservedAtMs = now;
        state.lastCoreObservedPositionFrames = message.position;
        void dispatch({ type: "playback_position_observed", deck: "a", seconds: framesToSeconds(message.position) });
      }
    }
    publishState();
  } else if (message.type === "seeked") {
    state.acknowledgedSeekGeneration = Math.max(state.acknowledgedSeekGeneration, message.generation ?? 0);
    state.positionFrames = message.position;
  } else if (message.type === "buffering") {
    setStatus("Buffering decoded groove audio…");
  } else if (message.type === "buffered") {
    void handleWorkletBuffered(message);
  } else if (message.type === "worklet-error") {
    setStatus(`Audio engine error: ${message.message || message.stage || "unknown error"}`);
    console.error("[vin.yl.player] AudioWorklet error", message);
  } else if (message.type === "ended") {
    state.positionFrames = message.position;
    void (async () => {
      await dispatch({ type: "playback_ended", deck: "a" });
      // Original: programme end runs the stylus into the deadwax for
      // 2 revolutions of surface bed before playback is considered over
      // (player.js 11267–11273). The engine rejects it when the needle is
      // lifted, a scratch is active, or a clip loop runs — same guards as
      // the original startDeadwaxPlayback.
      const durationSeconds = state.rpm > 0 ? DEADWAX_TURNS * (60 / state.rpm) : 0;
      if (durationSeconds > 0) {
        try {
          await dispatch({ type: "start_timed_region", region: "deadwax", now_ms: performance.now(), duration_seconds: durationSeconds });
        } catch {
          // Needle lifted / not ready: no deadwax traversal, as in the original.
        }
      }
    })();
  } else if (message.type === "scratch-replay-ended") {
    const request = state.scratchReplayRequests.get(message.id);
    if (request) {
      state.scratchReplayRequests.delete(message.id);
      request.resolve({ cancelled: Boolean(message.cancelled), positionFrames: Number(message.position) || 0 });
    }
  }
}

async function loadFile(file, { cache, resumeAudio = true } = {}) {
  console.log("[vin.yl.player] loadFile:start", {
    name: file?.name || "",
    size: file?.size || 0,
    type: file?.type || "",
    resumeAudio,
    hasCacheArg: Boolean(cache),
    hasActiveCache: Boolean(state.cacheHandler),
  });
  await initialiseAudio();
  if (resumeAudio) {
    await state.context.resume();
  }
  if (cache !== undefined) state.cacheHandler = normalizeCacheHandler(cache);
  state.decoder ??= new RecordDecoderClient("./record-decoder-worker.js", {
    loggingEnabled: isPlayerLoggingEnabled(),
    cache: state.cacheHandler,
  });
  state.decoder.setCache(state.cacheHandler);
  await state.decoder.initialise();
  state.streamInitialised = false;
  state.streamReady = false;
  state.streamDecodedFrames = 0;
  state.streamAppendChain = Promise.resolve();
  state.decodeProgressText = "";
  state.programmeMap = null;
  elements.play.disabled = false;
  elements.needle.disabled = false;
  elements.seek.disabled = true;
  if (state.recordObjectUrl) URL.revokeObjectURL(state.recordObjectUrl);
  state.recordObjectUrl = URL.createObjectURL(file);
  elements.recordImage.src = state.recordObjectUrl;
  setStatus(`Inspecting ${file.name}…`);
  const sourceBytes = await file.arrayBuffer();
  console.log("[vin.yl.player] loadFile:bytes", {
    name: file?.name || "",
    bytes: sourceBytes.byteLength || 0,
  });
  const cacheKey = await recordCacheKey(sourceBytes);
  state.recordHash = cacheKey;
  const inspected = await state.decoder.inspect(sourceBytes.slice(0));
  state.recordHeaderProof = inspected.recordHeaderProof || null;
  try {
    state.recordDescriptorJson = await decodeRecordDescriptorJson(sourceBytes, inspected.recordProfile || "");
  } catch (error) {
    state.recordDescriptorJson = "";
    console.warn("[vin.yl.player] record descriptor decode failed", error);
  }
  if (typeof state.cacheHandler?.setRecordContext === "function") {
    await state.cacheHandler.setRecordContext({
      descriptorJson: state.recordDescriptorJson,
      recordHeaderProof: state.recordHeaderProof,
      recordProfile: inspected.recordProfile || "",
    });
  }
  console.log("[vin.yl.player] loadFile:inspect", {
    recordProfile: inspected.recordProfile || "",
    payloadContainer: inspected.payloadContainer || "",
    releaseId: inspected.releaseId || "",
    hasProgrammeMap: Boolean(inspected.programmeMapJson),
    hasRecordDescriptorJson: Boolean(state.recordDescriptorJson),
  });
  state.programmeMap = parseJsonObject(inspected.programmeMapJson, null);
  elements.metadata.hidden = isEmbedMode();
  elements.metaProfile.textContent = inspected.recordProfile || "unknown";
  elements.metaContainer.textContent = inspected.payloadContainer || "unknown";
  elements.metaRelease.textContent = inspected.releaseId || "unsigned / unavailable";
  publishState();
  state.baseRpm = profileRpm(inspected.recordProfile);
  state.rpm = state.baseRpm;
  updateRpmButtons();
  setStatus(`Decoding ${file.name}…`);
  const decoded = await state.decoder.decode(sourceBytes, inspected.recordProfile || "", {
    recordBindingHex: state.recordHash,
  }, progress => {
    log.action("decoder-progress", {
      keys: Object.keys(progress || {}),
      status: progress?.status,
      message: progress?.msg || progress?.message || "",
      decodedSegmentCount: Array.isArray(progress?.decodedPcmSegments) ? progress.decodedPcmSegments.length : 0,
      decodedBytes: Array.isArray(progress?.decodedPcmSegments)
        ? progress.decodedPcmSegments.reduce((total, segment) => total + (segment.channelBuffers || []).reduce((sum, buffer) => sum + (buffer?.byteLength || 0), 0), 0)
        : 0,
    });
    state.decodeProgressText = formatDecodeProgress(progress);
    renderDecodeStatus();
    if (Array.isArray(progress.decodedPcmSegments) && progress.decodedPcmSegments.length) {
      const segments = progress.decodedPcmSegments;
      state.streamAppendChain = state.streamAppendChain
        .then(() => appendProgressiveSegments(segments))
        .catch(error => {
          setStatus(`Progressive playback failed: ${error.message || error}`);
        });
    }
  });

  await state.streamAppendChain;

  const sampleRate = Math.max(1, Number(decoded.sampleRate) || 48000);
  const audioLength = Math.max(1, Number(decoded.audioLength) || 0);
  const s16Buffers = Array.isArray(decoded.s16ChannelBuffers) ? decoded.s16ChannelBuffers : [];
  if (!s16Buffers.length) throw new Error("Record decoder returned no PCM channels");
  state.baseRpm = profileRpm(inspected.recordProfile);
  state.rpm = state.baseRpm;
  updateRpmButtons();
  if (!state.streamInitialised) {
    await loadDecodedPcm({ sampleRate, audioLength, s16ChannelBuffers: s16Buffers });
  }
  state.node.port.postMessage({ type: "stream-complete" });
  if (!state.streamReady) {
    await markLoadedReady();
    state.streamReady = true;
  }
  publishState();
  console.log("[vin.yl.player] loadFile:complete", {
    name: file?.name || "",
    durationSeconds: state.duration,
    sampleRate,
    payloadContainer: inspected.payloadContainer || "",
  });
  setStatus(`${file.name} · ${state.duration.toFixed(1)}s · ${sampleRate} Hz · ${inspected.payloadContainer || "record"}`);
}

async function loadRecordFromUrl(url, options = {}) {
  const resolved = new URL(String(url || ""), globalThis.location?.href || import.meta.url);
  console.log("[vin.yl.player] loadRecordFromUrl:start", {
    input: String(url || ""),
    resolved: resolved.toString(),
    options,
  });
  const response = await fetch(resolved.toString(), { cache: "force-cache" });
  if (!response.ok) {
    console.error("[vin.yl.player] loadRecordFromUrl:fetch-failed", {
      resolved: resolved.toString(),
      status: response.status,
    });
    throw new Error(`Failed to load record from ${resolved}: ${response.status}`);
  }
  const blob = await response.blob();
  console.log("[vin.yl.player] loadRecordFromUrl:fetched", {
    resolved: resolved.toString(),
    size: blob.size || 0,
    type: blob.type || "",
  });
  const pathname = resolved.pathname.split("/").pop() || "record.png";
  const file = new File([blob], pathname, { type: blob.type || "image/png" });
  return loadFile(file, { resumeAudio: false, ...(options || {}) });
}

async function configureStartupCache() {
  const cacheUrl = startupCacheUrl();
  if (!cacheUrl) {
    console.warn("[vin.yl.player] configureStartupCache skipped: no tape_url configured");
    return null;
  }
  console.info("[vin.yl.player] configureStartupCache:start", { cacheUrl });
  const cache = createRemoteOpusChunkCacheHandler({ apiBaseUrl: cacheUrl });
  state.cacheHandler = normalizeCacheHandler(cache);
  if (typeof state.cacheHandler.setRecordContext === "function" && state.recordDescriptorJson) {
    await state.cacheHandler.setRecordContext({
      descriptorJson: state.recordDescriptorJson,
      recordHeaderProof: state.recordHeaderProof,
      recordProfile: "",
    });
  }
  state.decoder?.setCache(state.cacheHandler);
  console.info("[vin.yl.player] configureStartupCache:ready", { cacheUrl });
  return state.cacheHandler;
}

function render() {
  const view = deckView();
  if (!view) return;
  elements.play.textContent = view.transport_on ? "STOP" : "START";
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
    motorRunning: Boolean(view?.transport_on),
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
    positionFrames: state.positionFrames,
    currentTrackIndex: currentTrackIndex(),
    currentTrackTitle: programmeTracks()[currentTrackIndex()]?.title || "",
    trackCount: programmeTracks().length,
  });
}

function publishState() {
  const snapshot = publicState();
  for (const listener of state.listeners) listener(snapshot);
  reportBridgePlayback(snapshot);
  reportBridgeRecord(snapshot);
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
  setLogging(enabled) {
    const next = setPlayerLoggingEnabled(enabled);
    log.action("logging-changed", { enabled: next });
    state.decoder?.setLogging?.(next);
    state.worker?.postMessage({ id: 0, type: "set-logging", payload: { enabled: next } });
    state.node?.port.postMessage({ type: "set-logging", enabled: next });
    return next;
  },
  setLogLevel(level) {
    const next = setPlayerLogLevel(level);
    log.action("log-level-changed", { level: next });
    return next;
  },
  setTelemetryLogInterval(milliseconds) {
    const next = setPlayerTelemetryInterval(milliseconds);
    log.action("telemetry-log-interval-changed", { milliseconds: next });
    return next;
  },
  get loggingEnabled() { return isPlayerLoggingEnabled(); },
  get logLevel() { return getPlayerLogLevel(); },
  configurePostMessageBridge(config = {}) {
    state.postMessageBridge = normalizePostMessageBridge(config);
    lastBridgePlaybackKey = "";
    lastBridgeRecordKey = "";
    lastBridgePlaybackAt = 0;
    reportBridgeRecord();
    reportBridgePlayback();
    return state.postMessageBridge;
  },
  postMessageBridgeSend(type, payload = {}) {
    return bridgePost(type, payload);
  },
  configureCache(handler) {
    state.cacheHandler = normalizeCacheHandler(handler);
    state.decoder?.setCache(state.cacheHandler);
    return state.cacheHandler;
  },
  createPcmChunkCacheHandler,
  createRemoteOpusChunkCacheHandler,
  createRemoteOpusPrecache,
  loadRecord: loadFile,
  loadRecordFromUrl,
  startTransport: async () => {
    await initialiseAudio();
    await state.context.resume();
    const view = deckView();
    if (!view?.transport_on) await dispatch({ type: "set_transport", deck: "a", running: true });
  },
  stopTransport: async () => {
    const view = deckView();
    if (view?.transport_on) await dispatch({ type: "set_transport", deck: "a", running: false });
  },
  toggleTransport: async () => {
    await initialiseAudio();
    await state.context.resume();
    await dispatch({ type: "toggle_transport", deck: "a" });
  },
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
  stepTrack,
  seekSeconds: seconds => queueSeek(Math.max(0, Math.min(state.duration, Number(seconds) || 0))),
  seekRatio: ratio => queueSeek(Math.max(0, Math.min(1, Number(ratio) || 0)) * state.duration),
  setRpm,
  setVolume,
  setCrossfader,
  setNeedleLifted: async lifted => {
    state.needleAutoBehaviorEnabled = false;
    const next = Boolean(lifted);
    const view = deckView();
    const lowering = !next && Boolean(view?.needle_lifted);
    await dispatch({ type: "set_needle", deck: "a", lifted: next, observed_playback_seconds: framesToSeconds(state.positionFrames) });
    state.node?.port.postMessage({ type: "needle", lifted: next });
    // Original resumeDeckFromNeedleDrop: lowering onto a live transport is a
    // stylus re-placement with needle-drop foley, never a brake/restart.
    if (lowering && view?.transport_on) state.node?.port.postMessage({ type: "needle-drop" });
  },
  beginScratch: ({ pointerId = 0, rotationDegrees = state.rotation, positionFrames = state.positionFrames, rate = 0, impulse = 0.22 } = {}) => {
    const position = Number(positionFrames) || 0;
    // The hand owns the record from the first touch: publish the scratching
    // state immediately so the canvas stops advancing the motor's visual
    // rotation — otherwise the drawn record fights the hand.
    state.scratching = true;
    state.rotation = Number(rotationDegrees) || state.rotation;
    publishState();
    recordScratchEvent({ type: "scratch-start", positionFrames: position, rate: Number(rate) || 0, impulse: Number(impulse) || 0 });
    state.node?.port.postMessage({ type: "scratch", active: true, position, rate: Number(rate) || 0, impulse: Number(impulse) || 0 });
    return dispatch({ type: "begin_scratch", deck: "a", pointer_id: pointerId, playback_seconds: framesToSeconds(position), rotation_degrees: rotationDegrees });
  },
  updateScratch: ({ positionFrames, rate = 0, rotationDegrees = state.rotation, impulse = 0 } = {}) => {
    const position = Number(positionFrames) || 0;
    const nextRate = Number(rate) || 0;
    const nextImpulse = Number(impulse) || 0;
    state.scratching = true;
    state.rotation = Number(rotationDegrees) || state.rotation;
    state.positionFrames = position;
    recordScratchEvent({ type: "scratch-motion", positionFrames: position, rate: nextRate, impulse: nextImpulse });
    state.node?.port.postMessage({ type: "motion", position, rate: nextRate, impulse: nextImpulse });
    return dispatch({ type: "move_scratch", deck: "a", position_frames: position, rendered_position_frames: position, rate: nextRate, rotation_degrees: Number(rotationDegrees) || 0, impulse: nextImpulse });
  },
  endScratch: ({ rotationDegrees = state.rotation, resumePlayback = true } = {}) => {
    state.scratching = false;
    state.rotation = Number(rotationDegrees) || state.rotation;
    publishState();
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
  log.action("core-worker-created", {});
  state.worker.onmessage = event => {
    log.receive(`core:${event.data?.type || (event.data?.ok ? "response" : "error")}`, event.data);
    const { id, ok, result, error } = event.data;
    const request = state.pending.get(id);
    if (!request) { if (id !== 0) log.warn("core-unmatched-response", event.data); return; }
    state.pending.delete(id);
    if (ok) request.resolve(result);
    else request.reject(new Error(error));
  };
  const result = await coreRequest("init", { moduleUrl: "./wasm/record-player/record_player.js" });
  state.view = result.view;
  elements.play.disabled = false;
  elements.needle.disabled = false;
  render();
  const canvas = document.querySelector("#player-canvas");
  if (canvas) {
    state.canvasController = createVinylPlayerCanvas(api, canvas);
    const embedOptions = embedCanvasOptions();
    if (embedOptions) state.canvasController.configure(embedOptions);
  }
  globalThis.addEventListener("message", event => { void handleBridgeMessage(event); });
  if (document.documentElement.classList.contains("embed-mode")) {
    api.configurePostMessageBridge({ enabled: true });
    // The iframe's "load" event fires long before this point (WASM init is
    // async), so a host that only re-sends record bytes on load would race
    // ahead of the inbound listener above and be dropped. Announce that the
    // bridge is live now so the host can (re)send whatever record it holds.
    const parentWindow = globalThis.parent !== globalThis ? globalThis.parent : null;
    if (parentWindow) parentWindow.postMessage({ type: "bitneedle-embed-ready" }, "*");
  }
  await configureStartupCache();
  const initialSrc = startupRecordSrc();
  if (initialSrc) {
    console.log("[vin.yl.player] startup autoload", { src: initialSrc });
    void loadRecordFromUrl(initialSrc).catch(error => {
      console.error("[vin.yl.player] startup autoload failed", error);
      setStatus(error.message || String(error));
    });
  }
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
  await dispatch({ type: "toggle_transport", deck: "a" });
});
elements.needle.addEventListener("click", () => {
  state.needleAutoBehaviorEnabled = false;
  const view = deckView();
  if (!view) return;
  const lifted = !view.needle_lifted;
  void dispatch({ type: "set_needle", deck: "a", lifted, observed_playback_seconds: framesToSeconds(state.positionFrames) });
  state.node?.port.postMessage({ type: "needle", lifted });
  if (!lifted && view.transport_on) state.node?.port.postMessage({ type: "needle-drop" });
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
