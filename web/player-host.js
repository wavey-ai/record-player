import { createLogger, setPlayerLoggingEnabled, isPlayerLoggingEnabled, isPlayerVerboseLoggingEnabled, setPlayerLogLevel, getPlayerLogLevel, setPlayerTelemetryInterval } from "./player-message-logger.js";
import { readAudioPlaybackStats } from "./audio-playback-stats.js";
import { measureAcousticLoopbackLatency } from "./audio-loopback-latency.js";
import { createVinylPlayerCanvas } from "./player-canvas.js";
import { RecordDecoderClient } from "./record-decoder-client.js";
import { createPcmChunkCacheHandler, recordCacheKey } from "./pcm-cache.js";
import { createRemoteOpusChunkCacheHandler, createRemoteOpusPrecache, decodeRecordDescriptorJson } from "./opus-cache.js";
import { buildSoundkitFrameHeader, soundkitOpusPacketItemsFromPackets } from "./player-soundkit.js";
import { clearScratchPerformances, deleteScratchPerformance, getScratchPerformance, listScratchPerformances, saveScratchPerformance } from "./scratch-performance-store.js";
import {
  normalizeScratchClicks,
  normalizeScratchPerformance,
  normalizeScratchPreset,
  SCRATCH_GATE_ALGORITHM_VERSION,
  SCRATCH_PERFORMANCE_SCHEMA_VERSION,
  SCRATCH_PRESET_DEFAULT_CLICKS,
} from "./scratch-performance-schema.js";

const log = createLogger("host");
const HOST_CONFIG = globalThis.VIN_YL_PLAYER_HOST_CONFIG || {};
const playerRoot = HOST_CONFIG.root || document;
const playerAssetBaseUrl = new URL(HOST_CONFIG.assetBaseUrl || "./", import.meta.url);
const sharedWasmBaseUrl = String(HOST_CONFIG.sharedWasmBaseUrl || globalThis.VIN_YL_PLAYER_SHARED_WASM_BASE_URL || "").replace(/\/+$/, "");
const initialLoggingParam = new URLSearchParams(globalThis.location?.search || "").get("player_log");
setPlayerLoggingEnabled(initialLoggingParam === "1");
const DEFAULT_TAPE_API_URL = "https://yl.vin/api/play/tape";
const DEFAULT_TAPE_MASTER_API_URL = "https://yl.vin/api/bitneedle-source-audio";

let tapePcmHelpersPromise = null;
let tapePlayerWasmPromise = null;

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
  if (isPlayerLoggingEnabled()) {
    console.log("[vin.yl.player] startupRecordSrc", {
      href: globalThis.location?.href || "",
      src: value,
    });
  }
  return value || "";
}

function startupCacheUrl() {
  const params = queryParams();
  const value = String(params.get("tape_url") || "").trim();
  const resolved = value || DEFAULT_TAPE_API_URL;
  if (isPlayerLoggingEnabled()) {
    console.log("[vin.yl.player] startupCacheUrl", {
      href: globalThis.location?.href || "",
      tapeUrl: value,
      resolvedTapeUrl: resolved,
    });
  }
  if (!value && isPlayerLoggingEnabled()) {
    console.info("[vin.yl.player] tape defaulting to built-in endpoint", {
      tapeUrl: DEFAULT_TAPE_API_URL,
    });
  }
  return resolved;
}

function startupTapeMasterUrl() {
  const params = queryParams();
  const value = String(params.get("tape_master_url") || globalThis.BITNEEDLE_SOURCE_AUDIO_STORE_API_BASE_URL || "").trim();
  return value || DEFAULT_TAPE_MASTER_API_URL;
}

function versionedAssetUrl(path) {
  const url = new URL(path, playerAssetBaseUrl);
  const version = queryParams().get("v");
  if (version) {
    url.searchParams.set("v", version);
  }
  return url.toString();
}

function sharedWasmAssetUrl(path) {
  if (!sharedWasmBaseUrl) return versionedAssetUrl(path);
  return versionedAssetUrl(`${sharedWasmBaseUrl}/${String(path).replace(/^\/+/, "")}`);
}

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function uint8View(value) {
  if (value instanceof Uint8Array) return value;
  if (ArrayBuffer.isView(value)) return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  return new Uint8Array(value || 0);
}

function cloneArrayBuffer(buffer) {
  if (buffer instanceof ArrayBuffer) return buffer.slice(0);
  if (ArrayBuffer.isView(buffer)) {
    return buffer.buffer.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength);
  }
  return new ArrayBuffer(0);
}

function cloneChannelBuffers(channelBuffers = []) {
  return (Array.isArray(channelBuffers) ? channelBuffers : [])
    .map((buffer) => cloneArrayBuffer(buffer))
    .filter((buffer) => buffer.byteLength > 0);
}

function concatenateUint8Chunks(chunks, totalLength = null) {
  const list = (Array.isArray(chunks) ? chunks : []).map(uint8View).filter((chunk) => chunk.byteLength > 0);
  const length = totalLength == null ? list.reduce((sum, chunk) => sum + chunk.byteLength, 0) : totalLength;
  const output = new Uint8Array(Math.max(0, length));
  let offset = 0;
  for (const chunk of list) {
    output.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return output;
}

function decodeBase64ToUint8Array(value) {
  const normalized = String(value || "").replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(normalized);
  const output = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    output[index] = binary.charCodeAt(index);
  }
  return output;
}

async function ensureTapePlayerWasmModule() {
  if (!tapePlayerWasmPromise) {
    tapePlayerWasmPromise = import(versionedAssetUrl("./player-wasm/player_wasm.js")).then(async (module) => {
      if (typeof module.default === "function") {
        await module.default({
          module_or_path: versionedAssetUrl("./player-wasm/player_wasm_bg.wasm"),
        });
      }
      return module;
    });
  }
  return tapePlayerWasmPromise;
}

async function ensureTapePcmHelpers() {
  if (!tapePcmHelpersPromise) {
    tapePcmHelpersPromise = (async () => {
      await import(versionedAssetUrl("./player-pcm-helpers.js"));
      const helperFactory = globalThis.BitneedlePlayerRuntimePcmHelpers?.createPlayerPcmHelpers;
      if (typeof helperFactory !== "function") {
        throw new Error("BitneedlePlayerRuntimePcmHelpers is unavailable.");
      }
      const soundkitModule = await import(sharedWasmAssetUrl("soundkit-wasm/soundkit_wasm.js"));
      if (typeof soundkitModule.default === "function") {
        await soundkitModule.default({
          module_or_path: sharedWasmAssetUrl("soundkit-wasm/soundkit_wasm_bg.wasm"),
        });
      }
      return {
        playerWasm: await ensureTapePlayerWasmModule(),
        helpers: helperFactory({
          clamp,
          getScratchAudioContext: () => null,
          ensureSoundkitOpusModule: () => soundkitModule,
          buildSoundkitFrameHeader,
          uint8View,
          yieldToMainThread: () => Promise.resolve(),
          assertPlayerRuntimeActive: () => {},
          concatenateUint8Chunks,
          soundkitOpusPacketItemsFromPackets,
          decodeBase64ToUint8Array,
        }),
      };
    })();
  }
  return tapePcmHelpersPromise;
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
// Keep the physical lead-in, but end the programme cleanly. Presave previews
// and social clips should not add two revolutions of deadwax surface noise
// after the music finishes.
const LEAD_IN_TURNS = 2;
const DEADWAX_TURNS = 2;
const DEFAULT_HF_ACCELERATION_LIMIT = 0.35;
const DEFAULT_STYLUS_TRACING_LIMIT = 0.72;

const elements = {
  file: playerRoot.querySelector("#file"),
  load: playerRoot.querySelector("#load"),
  play: playerRoot.querySelector("#play"),
  needle: playerRoot.querySelector("#needle"),
  tape: playerRoot.querySelector("#tape"),
  platter: playerRoot.querySelector("#platter"),
  seek: playerRoot.querySelector("#seek"),
  status: playerRoot.querySelector("#status"),
  recordImage: playerRoot.querySelector("#record-image"),
  metadata: playerRoot.querySelector("#record-metadata"),
  metaProfile: playerRoot.querySelector("#meta-profile"),
  metaContainer: playerRoot.querySelector("#meta-container"),
  metaRelease: playerRoot.querySelector("#meta-release"),
  rpm33: playerRoot.querySelector("#rpm-33"),
  rpm45: playerRoot.querySelector("#rpm-45"),
  rpm: playerRoot.querySelector("#rpm"),
  volume: playerRoot.querySelector("#volume"),
  xfade: playerRoot.querySelector("#xfade"),
  scratchPreset: playerRoot.querySelector("#scratch-preset"),
  scratchClicks: playerRoot.querySelector("#scratch-clicks"),
  scratchClicksValue: playerRoot.querySelector("#scratch-clicks-value"),
  highFrequencyAccelerationLimit: playerRoot.querySelector("#hf-acceleration-limit"),
  highFrequencyAccelerationLimitValue: playerRoot.querySelector("#hf-acceleration-limit-value"),
  stylusTracingLimit: playerRoot.querySelector("#stylus-tracing-limit"),
  stylusTracingLimitValue: playerRoot.querySelector("#stylus-tracing-limit-value"),
  advancedControls: playerRoot.querySelector(".advanced-controls"),
};

const state = {
  context: null,
  audioInitialisePromise: null,
  node: null,
  worker: null,
  coreInitialisePromise: null,
  requestId: 0,
  pending: new Map(),
  view: null,
  duration: 0,
  metadataDuration: 0,
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
  loadSequence: 0,
  loadInFlightSequence: 0,
  failedLoadSequence: 0,
  loadFailureSequence: 0,
  loadFailurePromise: null,
  recordObjectUrl: "",
  streamInitialised: false,
  streamReady: false,
  streamReadyMarking: false,
  streamDecodedFrames: 0,
  buffering: false,
  streamReadyPromise: null,
  streamReadyResolve: null,
  streamReadyReject: null,
  baseRpm: 33.3333333333,
  seekTimer: 0,
  seekInFlight: false,
  seekDispatchSerial: 0,
  queuedSeekSeconds: null,
  gainNode: null,
  captureNode: null,
  packetGain: 1,
  mixerGain: 1,
  volume: 1,
  crossfader: 0.5,
  listeners: new Set(),
  loadedFile: null,
  recordHash: "",
  recordReleaseId: "",
  basePcmSource: null,
  scratchRecorders: new Set(),
  scratchReplayRequests: new Map(),
  scratchReplayId: 0,
  activeScratchRecorder: null,
  replayScratching: false,
  scratchPreset: "baby",
  scratchClicks: SCRATCH_PRESET_DEFAULT_CLICKS.baby,
  scratchGate: 1,
  scratchGateTarget: 1,
  scratchDirection: 0,
  scratchMoving: false,
  scratchGatePhase: 0,
  scratchStrokeProgress: 0,
  effectiveRate: 0,
  highFrequencyAccelerationLimit: DEFAULT_HF_ACCELERATION_LIMIT,
  stylusTracingLimit: DEFAULT_STYLUS_TRACING_LIMIT,
  pointerToAudioLatencyMs: null,
  pointerCommandId: 0,
  lastDspRotationTurns: null,
  cleanEnd: false,
  canvasController: null,
  streamAppendChain: Promise.resolve(),
  decodeProgressText: "",
  pcmWindowWorker: null,
  pcmWindowBanks: [],
  pcmWindowFrames: 0,
  pcmWindowTotalFrames: 0,
  pcmWindowShared: false,
  pcmWindowInitialised: false,
  pcmWindowReady: false,
  pcmWindowAppliedStart: 0,
  pcmWindowAppliedEnd: 0,
  pcmWindowAppliedAvailableEnd: 0,
  pcmWindowReadyPromise: null,
  pcmWindowReadyResolve: null,
  pcmWindowReadyReject: null,
  pcmWindowRequestId: 0,
  pcmWindowRequestInFlight: false,
  pcmWindowAwaitingApply: false,
  pcmWindowQueuedRequest: null,
  pcmWindowAvailabilityWaiters: new Set(),
  pcmStreamGeneration: 0,
  pcmStreamLoadSequence: 0,
  playbackEpoch: 0,
  endTransitionGeneration: 0,
  lastOutputFrame: 0,
  pendingAutomaticDeadwax: null,
  regionTimer: 0,
  surfaceRegion: null,
  surfaceRegionId: 0,
  needleAutoBehaviorEnabled: true,
  cacheHandler: null,
  postMessageBridge: null,
  programmeMap: null,
  recordHeaderProof: null,
  recordDescriptorJson: "",
  tape: {
    checkedKey: "",
    available: false,
    active: false,
    loading: false,
    source: null,
    releaseId: "",
    sourceLabel: "",
  },
};

function scratchReplayActive() {
  return state.scratchReplayRequests.size > 0;
}

function currentLoadFailedOrFailing() {
  return state.loadSequence > 0 && (
    state.failedLoadSequence === state.loadSequence
    || state.loadFailureSequence === state.loadSequence
  );
}

function grooveInteractionReady() {
  return Boolean(
    state.node
    && state.streamReady
    && !state.buffering
    && !state.loadInFlightSequence
    && !currentLoadFailedOrFailing()
  );
}

function seekInteractionReady() {
  return Boolean(
    state.node
    && state.streamReady
    && !state.loadInFlightSequence
    && !state.tape.loading
    && !currentLoadFailedOrFailing()
  );
}

function clearLiveScratchInteraction() {
  const pointerId = state.scratchPointerId;
  state.scratching = false;
  state.scratchPointerId = null;
  state.replayScratching = false;
  if (
    pointerId != null
    && elements.platter?.hasPointerCapture?.(pointerId)
  ) {
    elements.platter.releasePointerCapture(pointerId);
  }
}

async function interruptScratchReplay(action = "Live control") {
  if (!scratchReplayActive()) return;
  await cancelScratchReplays(new Error(`${action} interrupted scratch replay`));
}

function interruptScratchReplayNow(action = "Live control") {
  if (!scratchReplayActive()) return;
  const hasUncancelledRequest = Array.from(state.scratchReplayRequests.values())
    .some(request => !request.cancelled);
  if (!hasUncancelledRequest) return;
  void cancelScratchReplays(new Error(`${action} interrupted scratch replay`));
}

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
    setScratchPresetType: "bitneedle-set-scratch-preset",
    setScratchClicksType: "bitneedle-set-scratch-clicks",
    setHighFrequencyAccelerationLimitType: "bitneedle-set-hf-acceleration-limit",
    setStylusTracingLimitType: "bitneedle-set-stylus-tracing-limit",
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
    buffering: Boolean(snapshot.buffering),
    scratching: Boolean(snapshot.scratching),
    playbackRate: Number(snapshot.playbackRate) || 0,
    bufferedSeconds: Number(snapshot.bufferedSeconds) || 0,
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
      if (Boolean(message.playing)) {
        try {
          await api.play();
        } catch (error) {
          const errorMessage = error instanceof Error ? error.message : String(error);
          setStatus("Tap the player once to enable audio.");
          log.warn("iframe-audio-activation-required", {
            type: message.type,
            message: errorMessage,
          });
          return;
        }
      } else {
        await api.pause();
      }
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
    if (message.type === bridge.inbound.setScratchPresetType) {
      api.setScratchPreset(message.preset);
      return;
    }
    if (message.type === bridge.inbound.setScratchClicksType) {
      api.setScratchClicks(message.clicks);
      return;
    }
    if (message.type === bridge.inbound.setHighFrequencyAccelerationLimitType) {
      api.setHighFrequencyAccelerationLimit(message.strength);
      return;
    }
    if (message.type === bridge.inbound.setStylusTracingLimitType) {
      api.setStylusTracingLimit(message.strength);
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
  try {
    HOST_CONFIG.onStatus?.(String(message || ""));
  } catch (error) {
    log.warn("status-callback-failed", { message: error instanceof Error ? error.message : String(error) });
  }
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
  const prefix = state.streamReady
    ? "Ready · "
    : state.streamInitialised
      ? "Buffering · "
      : "";
  setStatus(`${prefix}${state.decodeProgressText}`);
}

function resetStreamReadyPromise() {
  state.streamReadyPromise = new Promise((resolve, reject) => {
    state.streamReadyResolve = resolve;
    state.streamReadyReject = reject;
  });
  state.streamReadyPromise.catch(() => {});
}

function cancelPendingStreamReady(error) {
  const reject = state.streamReadyReject;
  state.streamReadyPromise = null;
  state.streamReadyResolve = null;
  state.streamReadyReject = null;
  reject?.(error);
}

function profileRpm(recordProfile) {
  return String(recordProfile || "").toLowerCase().includes("single45") ? 45 : 33.3333333333;
}

function updateRpmButtons() {
  elements.rpm33?.classList.toggle("selected", Math.abs(state.rpm - 33.3333333333) < 0.01);
  elements.rpm45?.classList.toggle("selected", Math.abs(state.rpm - 45) < 0.01);
}

async function setRpm(rpm) {
  await interruptScratchReplay("RPM control");
  const nextRpm = Math.max(16, Math.min(90, Number(rpm) || state.baseRpm));
  state.rpm = nextRpm;
  updateRpmButtons();
  if (elements.rpm) elements.rpm.value = String(nextRpm);
  const rate = nextRpm / state.baseRpm;
  await dispatch({ type: "set_playback_rate", deck: "a", rate });
}

function updateScratchTechniqueControls() {
  if (elements.scratchPreset) elements.scratchPreset.value = state.scratchPreset;
  if (elements.scratchClicks) elements.scratchClicks.value = String(state.scratchClicks);
  if (elements.scratchClicksValue) elements.scratchClicksValue.value = String(state.scratchClicks);
}

function setScratchClicks(value, { record = true } = {}) {
  interruptScratchReplayNow("Scratch click control");
  state.scratchClicks = normalizeScratchClicks(value, state.scratchClicks);
  updateScratchTechniqueControls();
  state.node?.port.postMessage({ type: "scratch-clicks", clicks: state.scratchClicks });
  if (record) recordScratchEvent({ type: "scratch-clicks", clicks: state.scratchClicks });
  publishState();
  return state.scratchClicks;
}

function setScratchPreset(value, { record = true } = {}) {
  interruptScratchReplayNow("Scratch preset control");
  state.scratchPreset = normalizeScratchPreset(value, state.scratchPreset);
  state.scratchClicks = SCRATCH_PRESET_DEFAULT_CLICKS[state.scratchPreset];
  updateScratchTechniqueControls();
  state.node?.port.postMessage({ type: "scratch-preset", preset: state.scratchPreset });
  state.node?.port.postMessage({ type: "scratch-clicks", clicks: state.scratchClicks });
  if (record) {
    recordScratchEvent({ type: "scratch-preset", preset: state.scratchPreset });
    recordScratchEvent({ type: "scratch-clicks", clicks: state.scratchClicks });
  }
  publishState();
  return state.scratchPreset;
}

function normalizeLimitStrength(value, name) {
  const strength = Number(value);
  if (!Number.isFinite(strength)) throw new TypeError(`${name} must be a finite number`);
  return clamp(strength, 0, 1);
}

function setHighFrequencyAccelerationLimit(value) {
  interruptScratchReplayNow("High-frequency acceleration limiter");
  state.highFrequencyAccelerationLimit = normalizeLimitStrength(value, "High-frequency acceleration limit");
  if (elements.highFrequencyAccelerationLimit) {
    elements.highFrequencyAccelerationLimit.value = String(state.highFrequencyAccelerationLimit);
  }
  if (elements.highFrequencyAccelerationLimitValue) {
    elements.highFrequencyAccelerationLimitValue.value = `${Math.round(state.highFrequencyAccelerationLimit * 100)}%`;
  }
  state.node?.port.postMessage({
    type: "hf-acceleration-limit",
    strength: state.highFrequencyAccelerationLimit,
  });
  publishState();
  return state.highFrequencyAccelerationLimit;
}

function setStylusTracingLimit(value) {
  interruptScratchReplayNow("Stylus tracing control");
  state.stylusTracingLimit = normalizeLimitStrength(value, "Stylus tracing limit");
  if (elements.stylusTracingLimit) elements.stylusTracingLimit.value = String(state.stylusTracingLimit);
  if (elements.stylusTracingLimitValue) {
    elements.stylusTracingLimitValue.value = `${Math.round(state.stylusTracingLimit * 100)}%`;
  }
  state.node?.port.postMessage({
    type: "stylus-tracing-limit",
    strength: state.stylusTracingLimit,
  });
  publishState();
  return state.stylusTracingLimit;
}

const PCM_WINDOW_SECONDS = 6;
const PCM_WINDOW_BANK_COUNT = 2;

function resetPcmWindowReadyPromise() {
  state.pcmWindowReadyPromise = new Promise((resolve, reject) => {
    state.pcmWindowReadyResolve = resolve;
    state.pcmWindowReadyReject = reject;
  });
  // A record load can be superseded before its first window arrives. Keep the
  // cancellation rejection observable to awaiters without creating an
  // unhandled-rejection warning for a superseded progressive load.
  state.pcmWindowReadyPromise.catch(() => {});
}

function waitForPcmAvailability(targetFrame) {
  const target = Math.max(0, Math.floor(Number(targetFrame) || 0));
  if (state.streamDecodedFrames >= target) return Promise.resolve(state.streamDecodedFrames);
  const promise = new Promise((resolve, reject) => {
    state.pcmWindowAvailabilityWaiters.add({ target, resolve, reject });
  });
  promise.catch(() => {});
  return promise;
}

function resolvePcmAvailabilityWaiters() {
  for (const waiter of state.pcmWindowAvailabilityWaiters) {
    if (state.streamDecodedFrames < waiter.target) continue;
    state.pcmWindowAvailabilityWaiters.delete(waiter);
    waiter.resolve(state.streamDecodedFrames);
  }
}

function disposePcmWindowTransport({ resetWorklet = true } = {}) {
  const worker = state.pcmWindowWorker;
  state.pcmWindowWorker = null;
  worker?.terminate();
  const resetError = new Error("PCM window transport reset");
  state.pcmWindowReadyReject?.(resetError);
  for (const waiter of state.pcmWindowAvailabilityWaiters) waiter.reject(resetError);
  state.pcmWindowAvailabilityWaiters.clear();
  state.pcmWindowBanks = [];
  state.pcmWindowFrames = 0;
  state.pcmWindowTotalFrames = 0;
  state.pcmWindowShared = false;
  state.pcmWindowInitialised = false;
  state.pcmWindowReady = false;
  state.pcmWindowAppliedStart = 0;
  state.pcmWindowAppliedEnd = 0;
  state.pcmWindowAppliedAvailableEnd = 0;
  state.pcmWindowReadyPromise = null;
  state.pcmWindowReadyResolve = null;
  state.pcmWindowReadyReject = null;
  state.pcmWindowRequestInFlight = false;
  state.pcmWindowAwaitingApply = false;
  state.pcmWindowQueuedRequest = null;
  if (resetWorklet) state.node?.port.postMessage({ type: "window-transport-reset" });
}

function flushQueuedPcmWindowRequest() {
  const queued = state.pcmWindowQueuedRequest;
  if (!queued) return;
  state.pcmWindowQueuedRequest = null;
  requestPcmWindow(queued.position, queued);
}

function requestPcmWindow(position, { resetPosition = false, workletRequestId = 0 } = {}) {
  const request = {
    position: Math.max(0, Number(position) || 0),
    resetPosition: Boolean(resetPosition),
    workletRequestId: Math.max(0, Math.floor(Number(workletRequestId) || 0)),
  };
  if (!state.pcmWindowWorker || !state.pcmWindowInitialised || state.pcmWindowRequestInFlight || state.pcmWindowAwaitingApply) {
    state.pcmWindowQueuedRequest = request;
    return;
  }
  const requestId = ++state.pcmWindowRequestId;
  state.pcmWindowRequestInFlight = true;
  state.pcmWindowWorker.postMessage({ type: "request-window", requestId, ...request });
}

function handlePcmWindowFailure(error) {
  const loadSequence = state.pcmStreamLoadSequence;
  state.pcmWindowRequestInFlight = false;
  state.pcmWindowReadyReject?.(error);
  for (const waiter of state.pcmWindowAvailabilityWaiters) waiter.reject(error);
  state.pcmWindowAvailabilityWaiters.clear();
  state.streamReadyReject?.(error);
  void failCurrentLoad(loadSequence, error, "PCM window error");
}

function handlePcmWindowWorkerMessage(worker, message) {
  if (worker !== state.pcmWindowWorker) return;
  if (message.type === "initialised") {
    state.pcmWindowInitialised = true;
    flushQueuedPcmWindowRequest();
    return;
  }
  if (message.type === "availability") {
    state.streamDecodedFrames = Math.max(0, Math.floor(Number(message.availableEnd) || 0));
    resolvePcmAvailabilityWaiters();
    state.node?.port.postMessage({
      type: "stream-availability",
      decodedLength: state.streamDecodedFrames,
      totalLength: Math.max(1, Math.floor(Number(message.totalFrames) || 1)),
    });
    if (Array.isArray(message.seamRepairs) && message.seamRepairs.length && isPlayerVerboseLoggingEnabled()) {
      for (const repair of message.seamRepairs) log.action("pcm-seam-repaired", repair);
    }
    void handleWorkletBuffered({ decodedLength: state.streamDecodedFrames });
    if (
      state.pcmWindowQueuedRequest
      && !state.pcmWindowRequestInFlight
      && !state.pcmWindowAwaitingApply
    ) {
      flushQueuedPcmWindowRequest();
    } else if (
      !scratchReplayActive()
      && (!state.pcmWindowReady || !state.streamReady || state.buffering)
    ) {
      requestPcmWindow(state.positionFrames, { resetPosition: !state.pcmWindowReady });
    }
    return;
  }
  if (message.type === "window-ready") {
    state.pcmWindowRequestInFlight = false;
    state.pcmWindowAwaitingApply = true;
    const channelBuffers = Array.isArray(message.channelBuffers) ? message.channelBuffers : [];
    state.node?.port.postMessage({ type: "window-ready", ...message }, channelBuffers);
    return;
  }
  if (message.type === "window-unavailable") {
    state.pcmWindowRequestInFlight = false;
    state.node?.port.postMessage({ type: "window-unavailable", ...message });
    return;
  }
  if (message.type === "worker-error") {
    const error = new Error(message.message || "PCM window worker failed");
    handlePcmWindowFailure(error);
  }
}

function initialisePcmWindowTransport(
  { sampleRate, audioLength, channelCount },
  loadSequence = state.loadSequence,
) {
  disposePcmWindowTransport();
  const streamGeneration = ++state.pcmStreamGeneration;
  state.pcmStreamLoadSequence = loadSequence;
  resetPcmWindowReadyPromise();
  const windowFrames = Math.max(4096, Math.min(audioLength, Math.round(sampleRate * PCM_WINDOW_SECONDS)));
  const shared = typeof SharedArrayBuffer === "function" && globalThis.crossOriginIsolated === true;
  const bankBuffers = shared
    ? Array.from({ length: PCM_WINDOW_BANK_COUNT }, () => (
      Array.from({ length: channelCount }, () => new SharedArrayBuffer(windowFrames * Float32Array.BYTES_PER_ELEMENT))
    ))
    : [];
  const worker = new Worker(versionedAssetUrl("./pcm-window-worker.js"), { type: "module" });
  state.pcmWindowWorker = worker;
  state.pcmWindowBanks = bankBuffers;
  state.pcmWindowFrames = windowFrames;
  state.pcmWindowTotalFrames = audioLength;
  state.pcmWindowShared = shared;
  worker.onmessage = event => handlePcmWindowWorkerMessage(worker, event.data || {});
  worker.onerror = event => {
    if (worker !== state.pcmWindowWorker) return;
    const error = new Error(event.message || "PCM window worker failed");
    handlePcmWindowFailure(error);
  };
  state.node.port.postMessage({
    type: "window-transport-init",
    sampleRate,
    totalFrames: audioLength,
    channelCount,
    windowFrames,
    bankBuffers,
    shared,
    streamGeneration,
  });
  worker.postMessage({
    type: "init-progressive",
    sampleRate,
    totalFrames: audioLength,
    channelCount,
    windowFrames,
    bankBuffers,
  });
  log.action("pcm-window-transport-created", { sampleRate, audioLength, channelCount, windowFrames, shared });
}

async function initialiseProgressiveStream(
  { sampleRate, audioLength, channels },
  { force = false, position = 0, loadSequence = state.loadSequence } = {},
) {
  assertCurrentLoad(loadSequence);
  if (state.streamInitialised && !force) return;
  const channelCount = Math.max(1, Math.min(2, Number(channels) || 2));
  const resolvedSampleRate = Math.max(1, Number(sampleRate) || 48000);
  await dispatch({ type: "set_source_sample_rate", deck: "a", sample_rate: resolvedSampleRate });
  assertCurrentLoad(loadSequence);
  state.sampleRate = resolvedSampleRate;
  const totalFrames = Math.max(1, Math.floor(Number(audioLength) || 1));
  state.duration = state.metadataDuration > 0
    ? state.metadataDuration
    : totalFrames / state.sampleRate;
  state.positionFrames = Math.max(0, Math.min(totalFrames - 1, Number(position) || 0));
  state.lastReportedPosition = state.positionFrames;
  state.lastCoreObservedPositionFrames = -1;
  state.lastCoreObservedAtMs = 0;
  state.lastDspRotationTurns = null;
  state.pointerToAudioLatencyMs = null;
  state.streamDecodedFrames = 0;
  state.streamReadyMarking = false;
  initialisePcmWindowTransport(
    { sampleRate: state.sampleRate, audioLength: totalFrames, channelCount },
    loadSequence,
  );
  state.streamInitialised = true;
}

async function appendProgressiveSegments(segments, loadSequence = state.loadSequence) {
  assertCurrentLoad(loadSequence);
  if (!Array.isArray(segments) || !segments.length) return;
  const first = segments[0] || {};
  await initialiseProgressiveStream({
    sampleRate: first.sampleRate,
    audioLength: first.audioLength,
    channels: first.channels,
  }, { loadSequence });
  assertCurrentLoad(loadSequence);
  const normalized = [];
  const transfer = [];
  for (const segment of segments) {
    const channelBuffers = Array.isArray(segment.channelBuffers) ? segment.channelBuffers : [];
    if (!channelBuffers.length) continue;
    const startFrame = Math.max(0, Math.floor(Number(segment.startFrame ?? segment.offset ?? 0) || 0));
    const inferredFrames = new Int16Array(channelBuffers[0]).length;
    const endFrame = Math.max(startFrame, Math.floor(Number(segment.endFrame) || (startFrame + inferredFrames)));
    normalized.push({
      startFrame,
      endFrame,
      workletSeamRepair: segment.workletSeamRepair !== false,
      channelBuffers,
    });
    transfer.push(...channelBuffers);
    if (isPlayerVerboseLoggingEnabled()) log.send("pcm-window:append", { startFrame, endFrame, channels: channelBuffers.length, bytes: channelBuffers.reduce((sum, buffer) => sum + (buffer?.byteLength || 0), 0) });
  }
  assertCurrentLoad(loadSequence);
  if (normalized.length) {
    if (!state.pcmWindowWorker) throw new Error("PCM window worker is unavailable");
    state.pcmWindowWorker.postMessage({ type: "append-segments", segments: normalized }, transfer);
  }
}

// Keep a few seconds ahead of the playhead before handing control back to the
// caller. The remaining ECDC segments continue decoding in the background.
const PROGRESSIVE_READY_SECONDS = 3;

async function handleWorkletBuffered(message) {
  if (state.pcmStreamLoadSequence !== state.loadSequence) return;
  state.streamDecodedFrames = Math.max(
    state.streamDecodedFrames,
    Math.max(0, Math.floor(Number(message.decodedLength) || 0)),
  );
  publishState();
  if (state.streamReady || state.streamReadyMarking) return;
  const totalFrames = Math.max(
    1,
    state.pcmWindowTotalFrames || Math.round(state.duration * state.sampleRate),
  );
  const thresholdFrames = Math.max(1024, Math.round(state.sampleRate * PROGRESSIVE_READY_SECONDS));
  const readyFrames = Math.min(totalFrames, thresholdFrames);
  const contiguousReady = (
    state.pcmWindowReady
    && state.streamDecodedFrames >= readyFrames
    && state.pcmWindowAppliedStart === 0
    && state.pcmWindowAppliedEnd >= readyFrames
    && state.pcmWindowAppliedAvailableEnd >= readyFrames
  );
  if (contiguousReady) {
    const loadSequence = state.pcmStreamLoadSequence;
    try {
      await ensureStreamReady(loadSequence);
    } catch (error) {
      if (error?.name === "AbortError") return;
      state.streamReadyReject?.(error);
      state.streamReadyReject = null;
      await failCurrentLoad(loadSequence, error, "Playback setup failed");
    }
  }
}

async function loadDecodedPcm(
  { sampleRate, audioLength, s16ChannelBuffers },
  loadSequence = state.loadSequence,
) {
  assertCurrentLoad(loadSequence);
  const sourceBuffers = Array.isArray(s16ChannelBuffers) ? s16ChannelBuffers : [];
  if (!sourceBuffers.length) throw new Error("Record contains no PCM channels");
  await initialiseProgressiveStream({
    sampleRate,
    audioLength,
    channels: sourceBuffers.length
  }, { loadSequence });
  assertCurrentLoad(loadSequence);
  const endFrame = Math.max(1, Number(audioLength) || new Int16Array(sourceBuffers[0]).length);
  if (!state.pcmWindowWorker) throw new Error("PCM window worker is unavailable");
  state.pcmWindowWorker.postMessage({
    type: "append-segments",
    segments: [{ startFrame: 0, endFrame, channelBuffers: sourceBuffers }],
  }, sourceBuffers);
  requestPcmWindow(state.positionFrames, { resetPosition: true });
  await Promise.all([
    state.pcmWindowReadyPromise,
    waitForPcmAvailability(endFrame),
  ]);
  assertCurrentLoad(loadSequence);
}

function audioBufferChannelToS16Buffer(channel) {
  const source = channel instanceof Float32Array ? channel : new Float32Array(channel || 0);
  const output = new Int16Array(source.length);
  for (let index = 0; index < source.length; index += 1) {
    const sample = Math.max(-1, Math.min(1, Number(source[index]) || 0));
    output[index] = sample < 0 ? Math.round(sample * 32768) : Math.round(sample * 32767);
  }
  return output.buffer;
}

function loadSupersededError() {
  const error = new Error("Player load was superseded by a newer request");
  error.name = "AbortError";
  return error;
}

function assertCurrentLoad(loadSequence) {
  if (loadSequence !== state.loadSequence) throw loadSupersededError();
  if (loadSequence !== 0 && state.failedLoadSequence === loadSequence) {
    const error = new Error("Player load is no longer active after a fatal playback error");
    error.name = "AbortError";
    throw error;
  }
}

function invalidateEndTransition() {
  state.endTransitionGeneration += 1;
  state.pendingAutomaticDeadwax = null;
}

function clearPendingSeekTransaction() {
  clearTimeout(state.seekTimer);
  state.seekTimer = 0;
  state.queuedSeekSeconds = null;
  state.seekInFlight = false;
  state.seekDispatchSerial += 1;
  state.acknowledgedSeekGeneration = state.pendingSeekGeneration;
}

function failCurrentLoad(loadSequence, error, label = "Player load failed") {
  if (loadSequence !== state.loadSequence) return false;
  if (
    state.loadFailureSequence === loadSequence
    && state.loadFailurePromise
  ) {
    return state.loadFailurePromise;
  }
  if (state.failedLoadSequence === loadSequence) return true;

  state.loadFailureSequence = loadSequence;
  state.failedLoadSequence = loadSequence;
  // Close replay and seek admission before the first await. Decoder, worklet
  // and UI failure callbacks can otherwise re-enter replay/seek while teardown
  // is waiting for a sample-accurate replay acknowledgement.
  state.streamReady = false;
  state.streamReadyMarking = false;
  state.buffering = false;
  clearPendingSeekTransaction();
  clearLiveScratchInteraction();
  cancelPendingStreamReady(error);
  const failingDecoder = state.decoder;
  failingDecoder?.close();
  if (state.decoder === failingDecoder) state.decoder = null;

  const failurePromise = (async () => {
    // Settle replay while its worklet generation is still accepted. The reset
    // can then invalidate PCM without stranding a replay promise behind the
    // generation filter.
    await cancelScratchReplays(error);
    if (loadSequence !== state.loadSequence) return false;
    clearPendingSeekTransaction();
    clearLiveScratchInteraction();
    state.pcmStreamGeneration += 1;
    invalidateEndTransition();
    disposePcmWindowTransport();
    state.streamInitialised = false;
    state.streamDecodedFrames = 0;
    state.streamAppendChain = Promise.resolve();
    state.duration = 0;
    state.metadataDuration = 0;
    state.positionFrames = 0;
    state.lastReportedPosition = 0;
    state.basePcmSource = null;
    state.pendingAutomaticDeadwax = null;
    if (state.recordObjectUrl) URL.revokeObjectURL(state.recordObjectUrl);
    state.recordObjectUrl = "";
    if (elements.recordImage) elements.recordImage.removeAttribute("src");
    elements.play.disabled = true;
    elements.needle.disabled = true;
    elements.seek.disabled = true;
    await dispatch({
      type: "set_load_state",
      deck: "a",
      status: "failed",
      loaded: false,
      duration_seconds: 0,
    }).catch(() => {});
    if (loadSequence !== state.loadSequence) return false;
    setStatus(`${label}: ${error?.message || error}`);
    publishState();
    return true;
  })();
  const managedFailurePromise = failurePromise.finally(() => {
    if (state.loadFailureSequence !== loadSequence) return;
    state.loadFailureSequence = 0;
    state.loadFailurePromise = null;
  });
  state.loadFailurePromise = managedFailurePromise;
  return managedFailurePromise;
}

// Presave and authoring surfaces already have a conventional audio file before
// they have a published Bitneedle PNG. Feed that decoded PCM through the same
// Rust transport, AudioWorklet and acoustic scratch renderer used for records so
// previews never need a parallel scratch implementation.
async function loadAudioFile(file, options = {}) {
  if (!(file instanceof Blob)) throw new TypeError("An audio File or Blob is required");
  const loadSequence = ++state.loadSequence;
  state.loadInFlightSequence = loadSequence;
  invalidateEndTransition();
  clearPendingSeekTransaction();
  clearLiveScratchInteraction();
  cancelPendingStreamReady(loadSupersededError());
  try {
    await cancelScratchReplays(new Error("Player load cancelled scratch replay"));
    assertCurrentLoad(loadSequence);
    return await loadAudioFileForSequence(file, options, loadSequence);
  } catch (error) {
    if (loadSequence !== state.loadSequence) throw loadSupersededError();
    await failCurrentLoad(loadSequence, error, "Audio preview failed");
    throw error;
  } finally {
    if (state.loadInFlightSequence === loadSequence) state.loadInFlightSequence = 0;
  }
}

async function loadAudioFileForSequence(
  file,
  { artworkUrl = "", title = "", artist = "", cleanEnd = true } = {},
  loadSequence,
) {
  await initialiseAudio();
  assertCurrentLoad(loadSequence);
  await state.context.resume();
  assertCurrentLoad(loadSequence);
  const previousView = deckView();
  if (previousView?.transport_on || previousView?.playing || state.view?.lead_in_active || state.view?.deadwax_active) {
    await stopPlaybackTransport();
    assertCurrentLoad(loadSequence);
  }
  await dispatch({ type: "set_load_state", deck: "a", status: "loading", loaded: false, duration_seconds: 0 });
  assertCurrentLoad(loadSequence);
  const sourceBytes = await file.arrayBuffer();
  assertCurrentLoad(loadSequence);
  const audioBuffer = await state.context.decodeAudioData(sourceBytes.slice(0));
  assertCurrentLoad(loadSequence);
  if (!audioBuffer?.length || !audioBuffer.numberOfChannels) {
    throw new Error("The audio file decoded empty");
  }

  state.decoder?.close();
  state.decoder = null;
  disposePcmWindowTransport({ resetWorklet: false });
  state.streamInitialised = false;
  state.streamReady = false;
  state.streamReadyMarking = false;
  state.streamDecodedFrames = 0;
  state.streamAppendChain = Promise.resolve();
  state.decodeProgressText = "";
  state.metadataDuration = audioBuffer.duration;
  state.duration = audioBuffer.duration;
  state.positionFrames = 0;
  state.lastReportedPosition = 0;
  state.baseRpm = 33.3333333333;
  state.rpm = state.baseRpm;
  state.cleanEnd = Boolean(cleanEnd);
  state.node.port.postMessage({
    type: "end-behavior",
    cleanEnd: state.cleanEnd,
    deadwaxTurns: DEADWAX_TURNS,
  });
  state.recordHash = `audio:${file.name || "preview"}:${file.size || 0}:${file.lastModified || 0}`;
  state.recordReleaseId = "";
  state.recordHeaderProof = null;
  state.recordDescriptorJson = "";
  state.programmeMap = {
    sampleRate: audioBuffer.sampleRate,
    durationMs: Math.round(audioBuffer.duration * 1000),
    totalSamples: audioBuffer.length,
    tracks: [{ title: String(title || file.name || "Preview"), artist: String(artist || "") }],
  };
  resetTapeState();
  updateRpmButtons();
  state.node.port.postMessage({ type: "reset" });
  state.node.port.postMessage({ type: "native-rpm", rpm: state.baseRpm });

  const channelCount = Math.min(2, audioBuffer.numberOfChannels);
  const s16ChannelBuffers = Array.from({ length: channelCount }, (_, index) => (
    audioBufferChannelToS16Buffer(audioBuffer.getChannelData(index))
  ));
  storeBasePcmSource({
    sampleRate: audioBuffer.sampleRate,
    audioLength: audioBuffer.length,
    s16ChannelBuffers,
  });
  await loadDecodedPcm({
    sampleRate: audioBuffer.sampleRate,
    audioLength: audioBuffer.length,
    s16ChannelBuffers,
  }, loadSequence);
  assertCurrentLoad(loadSequence);
  state.node.port.postMessage({ type: "stream-complete" });
  await markLoadedReady(loadSequence);
  assertCurrentLoad(loadSequence);
  state.streamReady = true;
  state.streamReadyResolve?.();
  state.streamReadyResolve = null;

  state.recordObjectUrl = String(artworkUrl || "");
  if (elements.recordImage) {
    elements.recordImage.src = state.recordObjectUrl;
    elements.recordImage.alt = String(title || file.name || "Audio preview");
  }
  if (elements.metadata) elements.metadata.hidden = true;
  if (elements.metaProfile) elements.metaProfile.textContent = "audio-preview";
  if (elements.metaContainer) elements.metaContainer.textContent = file.type || "audio";
  if (elements.metaRelease) elements.metaRelease.textContent = String(title || file.name || "Preview");
  setStatus(`${file.name || "Audio preview"} · ${audioBuffer.duration.toFixed(1)}s · ${audioBuffer.sampleRate} Hz`);
  publishState();
  return publicState();
}

async function getCaptureStream() {
  await initialiseAudio();
  await state.context.resume();
  if (!state.captureNode) {
    state.captureNode = state.context.createMediaStreamDestination();
    state.gainNode.connect(state.captureNode);
  }
  return state.captureNode.stream;
}

async function measurePhysicalLoopbackLatency(options = {}) {
  await initialiseAudio();
  const snapshot = publicState();
  if (snapshot.playing || snapshot.motorRunning || snapshot.scratching) {
    throw new Error("Stop the transport before you measure acoustic loopback latency");
  }
  return measureAcousticLoopbackLatency(state.context, options);
}

async function markLoadedReady(loadSequence = state.loadSequence) {
  assertCurrentLoad(loadSequence);
  await dispatch({ type: "set_load_state", deck: "a", status: "ready", loaded: true, duration_seconds: state.duration });
  assertCurrentLoad(loadSequence);
  await dispatch({
    type: "playback_position_observed",
    deck: "a",
    seconds: framesToSeconds(state.positionFrames),
  });
  assertCurrentLoad(loadSequence);
  elements.play.disabled = false;
  elements.needle.disabled = false;
  elements.seek.disabled = false;
}

async function ensureStreamReady(loadSequence = state.loadSequence) {
  assertCurrentLoad(loadSequence);
  if (state.streamReady) return;
  if (state.streamReadyMarking) {
    if (!state.streamReadyPromise) throw new Error("Stream readiness transaction is unavailable");
    await state.streamReadyPromise;
    assertCurrentLoad(loadSequence);
    return;
  }
  state.streamReadyMarking = true;
  try {
    await markLoadedReady(loadSequence);
    assertCurrentLoad(loadSequence);
    state.streamReady = true;
    renderDecodeStatus();
    state.streamReadyResolve?.();
    state.streamReadyResolve = null;
  } finally {
    if (loadSequence === state.loadSequence) state.streamReadyMarking = false;
  }
}

function resetTapeState() {
  state.tape.checkedKey = "";
  state.tape.available = false;
  state.tape.active = false;
  state.tape.loading = false;
  state.tape.source = null;
  state.tape.releaseId = "";
  state.tape.sourceLabel = "";
}

function updateTapeButton() {
  if (!elements.tape) return;
  elements.tape.hidden = false;
  elements.tape.textContent = state.tape.loading ? "TAPE..." : "TAPE";
  elements.tape.disabled = !state.tape.available || state.tape.loading || !state.basePcmSource;
  elements.tape.classList.toggle("is-active", Boolean(state.tape.active));
  elements.tape.setAttribute("aria-pressed", state.tape.active ? "true" : "false");
  elements.tape.title = state.tape.active
    ? "Using HQ Opus tape master"
    : state.tape.available
      ? "Switch to HQ Opus tape master"
      : "HQ Opus tape master unavailable";
}

function tapeMasterObjectUrl(releaseId, action) {
  const base = String(startupTapeMasterUrl() || "").replace(/\/+$/, "");
  return `${base}/objects/${encodeURIComponent(releaseId)}/${action}`;
}

async function fetchTapeMasterRemoteManifest(releaseId) {
  try {
    const response = await fetch(tapeMasterObjectUrl(releaseId, "manifest"), {
      mode: "cors",
      credentials: "omit",
      cache: "no-store",
    });
    if (!response.ok) return null;
    const manifest = await response.json();
    if (manifest?.status !== "sealed") return null;
    if (!(Number(manifest?.durationFrames) > 0) || !(Number(manifest?.committedBytes) > 0)) return null;
    const codec = String(manifest?.codec || "").toLowerCase();
    const contentType = String(manifest?.contentType || "").toLowerCase();
    const encrypted = manifest?.encrypted !== false;
    if (encrypted) {
      if (codec !== "bce1" && contentType !== "application/vnd.bitneedle.bce1") return null;
    } else if (codec !== "opus" && !contentType.includes("opus")) {
      return null;
    }
    return manifest;
  } catch (error) {
    console.warn("[vin.yl.player] tape master manifest fetch failed", error);
    return null;
  }
}

async function fetchTapeMasterRemoteStream(releaseId) {
  const manifest = await fetchTapeMasterRemoteManifest(releaseId);
  if (!manifest) return null;
  const totalBytes = Math.max(0, Math.floor(Number(manifest.committedBytes) || 0));
  if (!(totalBytes > 0)) return null;
  const chunkSize = 4 * 1024 * 1024;
  const streamChunks = [];
  try {
    for (let offset = 0; offset < totalBytes; offset += chunkSize) {
      const end = Math.min(totalBytes, offset + chunkSize) - 1;
      const response = await fetch(tapeMasterObjectUrl(releaseId, "stream"), {
        mode: "cors",
        credentials: "omit",
        cache: "no-store",
        headers: { Range: `bytes=${offset}-${end}` },
      });
      if (!response.ok) return null;
      streamChunks.push(new Uint8Array(await response.arrayBuffer()));
    }
  } catch (error) {
    console.warn("[vin.yl.player] tape master stream fetch failed", error);
    return null;
  }
  return {
    meta: {
      sampleRate: Number(manifest.timescale) || 48000,
      channels: Number(manifest.channels) || 2,
      frameCount: Number(manifest.durationFrames) || 0,
      bitsPerSample: Number(manifest.bitsPerSample) || 16,
      encrypted: manifest.encrypted !== false,
      encryptedCache: manifest.encrypted !== false,
      audioFormat: "soundkit_v2_opus_stream",
      format: "soundkit_v2_opus_stream",
      streamCodec: "opus",
    },
    streamChunks,
  };
}

function tapeIdbRequest(request) {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error || new Error("IndexedDB request failed"));
  });
}

function openTapeMasterDb() {
  return new Promise((resolve) => {
    if (typeof indexedDB === "undefined") {
      resolve(null);
      return;
    }
    let request;
    try {
      request = indexedDB.open("bitneedle-source-audio-local");
    } catch (_error) {
      resolve(null);
      return;
    }
    request.onupgradeneeded = () => {
      try {
        request.transaction?.abort();
      } catch (_error) {}
      resolve(null);
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => resolve(null);
    request.onblocked = () => resolve(null);
  });
}

async function readTapeMasterMeta(releaseId) {
  const db = await openTapeMasterDb();
  if (!db) return null;
  try {
    if (!db.objectStoreNames.contains("source-stream-meta")) return null;
    const store = db.transaction("source-stream-meta", "readonly").objectStore("source-stream-meta");
    return (await tapeIdbRequest(store.get(String(releaseId)))) || null;
  } catch (error) {
    console.warn("[vin.yl.player] tape master meta read failed", error);
    return null;
  } finally {
    db.close();
  }
}

async function readTapeMasterStream(releaseId) {
  const db = await openTapeMasterDb();
  if (!db) return null;
  try {
    if (!db.objectStoreNames.contains("source-stream-meta") || !db.objectStoreNames.contains("source-stream-blobs")) {
      return null;
    }
    const transaction = db.transaction(["source-stream-meta", "source-stream-blobs"], "readonly");
    const meta = await tapeIdbRequest(transaction.objectStore("source-stream-meta").get(String(releaseId)));
    if (!meta) return null;
    const rows = await tapeIdbRequest(transaction.objectStore("source-stream-blobs").index("cacheKey").getAll(String(releaseId)));
    const streamChunks = (Array.isArray(rows) ? rows : [])
      .filter((row) => row?.kind === "stream" && row.chunk instanceof ArrayBuffer)
      .sort((a, b) => (Number(a.chunkIndex) || 0) - (Number(b.chunkIndex) || 0))
      .map((row) => new Uint8Array(row.chunk.slice(0)));
    if (!streamChunks.length) return null;
    return { meta, streamChunks };
  } catch (error) {
    console.warn("[vin.yl.player] tape master stream read failed", error);
    return null;
  } finally {
    db.close();
  }
}

function tapeMasterEnvelopeLength(bytes, offset = 0) {
  const headerLength = 84;
  if (!(bytes instanceof Uint8Array) || offset < 0 || offset + headerLength > bytes.length) return 0;
  if (bytes[offset] !== 0x42 || bytes[offset + 1] !== 0x43 || bytes[offset + 2] !== 0x45 || bytes[offset + 3] !== 0x31) {
    return 0;
  }
  const plaintextLength = (
    (bytes[offset + 56] << 24)
    | (bytes[offset + 57] << 16)
    | (bytes[offset + 58] << 8)
    | bytes[offset + 59]
  ) >>> 0;
  if (!(plaintextLength > 0)) return 0;
  return headerLength + plaintextLength + 16;
}

function tapeMasterReadBigUint64BE(bytes, offset) {
  if (!(bytes instanceof Uint8Array) || offset < 0 || offset + 8 > bytes.length) return 0;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (typeof view.getBigUint64 === "function") {
    try {
      return Number(view.getBigUint64(offset, false));
    } catch (_error) {}
  }
  let value = 0;
  for (let index = 0; index < 8; index += 1) {
    value = (value * 256) + bytes[offset + index];
  }
  return value;
}

function tapeMasterEncryptionContext({
  cacheKey = "",
  chunkIndex = 0,
  packetOffset = 0,
  plaintextLength = 0,
  codecIdentifier = "soundkit_v2_opus_stream",
} = {}) {
  return {
    protocolVersion: 1,
    cacheFormatVersion: 1,
    cacheStoreName: "bitneedle-source-audio-master",
    cacheKey: String(cacheKey || ""),
    chunkIndex: Math.max(0, Math.floor(Number(chunkIndex) || 0)),
    packetOffset: Math.max(0, Math.floor(Number(packetOffset) || 0)),
    plaintextLength: Math.max(0, Math.floor(Number(plaintextLength) || 0)),
    codecIdentifier: String(codecIdentifier || "soundkit_v2_opus_stream"),
  };
}

async function decryptTapeMasterStreamChunks(streamChunks, descriptorJson, cacheKey, codecIdentifier = "soundkit_v2_opus_stream") {
  const chunks = Array.isArray(streamChunks) ? streamChunks : [];
  if (!chunks.length) return null;
  const { playerWasm } = await ensureTapePcmHelpers();
  if (typeof playerWasm?.decryptCacheEntry !== "function") {
    throw new Error("player-wasm decryptCacheEntry is unavailable.");
  }
  const sourceBytes = concatenateUint8Chunks(chunks);
  const plaintextChunks = [];
  let offset = 0;
  while (offset < sourceBytes.length) {
    const envelopeLength = tapeMasterEnvelopeLength(sourceBytes, offset);
    if (!(envelopeLength > 0) || offset + envelopeLength > sourceBytes.length) {
      throw new Error("Invalid BCE1 tape master envelope.");
    }
    const envelope = sourceBytes.slice(offset, offset + envelopeLength);
    const chunkIndex = tapeMasterReadBigUint64BE(envelope, 40);
    const packetOffset = tapeMasterReadBigUint64BE(envelope, 48);
    const plaintextLength = (
      (envelope[56] << 24)
      | (envelope[57] << 16)
      | (envelope[58] << 8)
      | envelope[59]
    ) >>> 0;
    const plaintext = playerWasm.decryptCacheEntry(
      descriptorJson,
      JSON.stringify(tapeMasterEncryptionContext({
        cacheKey,
        chunkIndex,
        packetOffset,
        plaintextLength,
        codecIdentifier,
      })),
      envelope,
    );
    const packetBytes = uint8View(plaintext);
    if (!(packetBytes.byteLength > 0)) {
      throw new Error("Decrypted tape master chunk was empty.");
    }
    plaintextChunks.push(packetBytes);
    offset += envelopeLength;
  }
  return concatenateUint8Chunks(plaintextChunks);
}

function buildPcmSourceFromProvider(provider, sampleRate, frameCount) {
  const channelBuffers = provider.channelData
    .map((channel) => channel.buffer.slice(channel.byteOffset, channel.byteOffset + channel.byteLength))
    .filter((buffer) => buffer.byteLength > 0);
  return {
    sampleRate,
    audioLength: frameCount,
    channels: channelBuffers.length,
    channelBuffers,
  };
}

async function decodeTapeMasterSource(stored, releaseId) {
  const sampleRate = Math.max(1, Math.floor(Number(stored?.meta?.sampleRate) || state.sampleRate || 48000));
  const channels = Math.max(1, Math.floor(Number(stored?.meta?.channels) || 2));
  const frameCount = Math.max(0, Math.floor(Number(stored?.meta?.frameCount) || Number(stored?.meta?.audioLength) || 0));
  if (!(frameCount > 0)) return null;
  if (!state.recordDescriptorJson) return null;
  const codecIdentifier = String(stored?.meta?.audioFormat || stored?.meta?.format || "soundkit_v2_opus_stream");
  const encrypted = stored?.meta?.encrypted !== false && stored?.meta?.encryptedCache !== false;
  const streamBytes = encrypted
    ? await decryptTapeMasterStreamChunks(stored.streamChunks, state.recordDescriptorJson, releaseId, codecIdentifier)
    : concatenateUint8Chunks(stored.streamChunks);
  if (!(streamBytes?.byteLength > 0)) return null;
  const { helpers } = await ensureTapePcmHelpers();
  const pcmBytes = await helpers.decodeSoundkitOpusPacketsToPcmBytes(
    {
      sampleRate,
      channels,
      startFrame: 0,
      endFrame: frameCount,
    },
    [streamBytes],
  );
  if (!(pcmBytes?.byteLength > 0)) return null;
  const provider = helpers.createS16PcmWindowProviderFromPcmBytes({
    pcmBytes,
    frameCount,
    sampleRate,
    channels,
    bitsPerSample: Math.max(1, Math.floor(Number(stored?.meta?.bitsPerSample) || 16)),
    audioFormat: "tape_master_pcm",
  });
  return buildPcmSourceFromProvider(provider, sampleRate, frameCount);
}

async function ensureTapeMasterSource() {
  const loadSequence = state.loadSequence;
  const releaseId = String(state.recordReleaseId || "").trim();
  if (!releaseId) return null;
  if (state.tape.source && state.tape.releaseId === releaseId) return state.tape.source;
  const stored = (await readTapeMasterStream(releaseId)) || (await fetchTapeMasterRemoteStream(releaseId));
  assertCurrentLoad(loadSequence);
  if (!stored) return null;
  const source = await decodeTapeMasterSource(stored, releaseId);
  assertCurrentLoad(loadSequence);
  if (releaseId !== String(state.recordReleaseId || "").trim()) throw loadSupersededError();
  if (!source) return null;
  if (source.sampleRate !== state.sampleRate || source.audioLength !== Math.round(state.duration * state.sampleRate)) {
    throw new Error("Tape master geometry does not match the loaded record.");
  }
  state.tape.source = source;
  state.tape.releaseId = releaseId;
  return source;
}

function storeBasePcmSource({ sampleRate, audioLength, s16ChannelBuffers }) {
  const channelBuffers = cloneChannelBuffers(s16ChannelBuffers);
  if (!channelBuffers.length) {
    state.basePcmSource = null;
    return;
  }
  state.basePcmSource = {
    sampleRate: Math.max(1, Math.floor(Number(sampleRate) || 48000)),
    audioLength: Math.max(0, Math.floor(Number(audioLength) || new Int16Array(channelBuffers[0]).length)),
    channels: channelBuffers.length,
    channelBuffers,
  };
}

async function replaceActivePcmSource(source, loadSequence = state.loadSequence) {
  if (!state.node || !source) return;
  await cancelScratchReplays(new Error("PCM source replacement cancelled scratch replay"));
  assertCurrentLoad(loadSequence);
  const view = deckView();
  if (state.scratching || state.view?.lead_in_active || state.view?.deadwax_active) {
    throw new Error("TAPE switching is unavailable while scratching or cueing.");
  }
  clearTimeout(state.seekTimer);
  state.queuedSeekSeconds = null;
  const wasPlaying = Boolean(view?.playing);
  const motorRunning = Boolean(view?.transport_on);
  const needleLifted = Boolean(view?.needle_lifted);
  const position = clamp(Math.floor(Number(state.positionFrames) || 0), 0, Math.max(0, source.audioLength - 1));
  if (wasPlaying) {
    const playbackEpoch = ++state.playbackEpoch;
    state.node.port.postMessage({ type: "stop", handoff: false, playbackEpoch });
  }
  const channelBuffers = cloneChannelBuffers(source.channelBuffers);
  await initialiseProgressiveStream({
    sampleRate: source.sampleRate,
    audioLength: source.audioLength,
    channels: source.channels,
  }, { force: true, position, loadSequence });
  assertCurrentLoad(loadSequence);
  state.pcmWindowWorker.postMessage({
    type: "append-segments",
    segments: [{ startFrame: 0, endFrame: source.audioLength, channelBuffers }],
  }, channelBuffers);
  requestPcmWindow(position, { resetPosition: true });
  await Promise.all([
    state.pcmWindowReadyPromise,
    waitForPcmAvailability(source.audioLength),
  ]);
  assertCurrentLoad(loadSequence);
  state.node.port.postMessage({ type: "stream-complete" });
  state.positionFrames = position;
  state.lastReportedPosition = position;
  state.node.port.postMessage({ type: "transport", running: motorRunning });
  state.node.port.postMessage({ type: "needle", lifted: needleLifted });
  if (wasPlaying) {
    const playbackEpoch = ++state.playbackEpoch;
    state.node.port.postMessage({
      type: "play",
      position,
      rate: state.baseRpm > 0 ? state.rpm / state.baseRpm : 1,
      handoff: false,
      playbackEpoch,
    });
  }
  state.sampleRate = source.sampleRate;
  state.duration = source.sampleRate > 0 ? source.audioLength / source.sampleRate : state.duration;
  state.streamInitialised = true;
  state.streamReady = true;
  state.streamDecodedFrames = source.audioLength;
}

async function setTapeMonitor(active) {
  const loadSequence = state.loadSequence;
  invalidateEndTransition();
  const next = Boolean(active);
  if (next === state.tape.active) return;
  if (!state.basePcmSource) {
    setStatus("TAPE monitor is unavailable until the record finishes decoding.");
    return;
  }
  state.tape.loading = true;
  updateTapeButton();
  try {
    await cancelScratchReplays(new Error("Tape monitor switch cancelled scratch replay"));
    assertCurrentLoad(loadSequence);
    if (next) {
      setStatus("TAPE: loading HQ Opus master...");
      const source = await ensureTapeMasterSource();
      assertCurrentLoad(loadSequence);
      if (!source) {
        state.tape.available = false;
        setStatus("HQ Opus tape master is unavailable for this record.");
        return;
      }
      await replaceActivePcmSource(source, loadSequence);
      state.tape.active = true;
      state.tape.available = true;
      state.tape.sourceLabel = "hq-opus";
      setStatus("TAPE: HQ Opus");
    } else {
      await replaceActivePcmSource(state.basePcmSource, loadSequence);
      state.tape.active = false;
      setStatus("TAPE: record audio");
    }
  } finally {
    state.tape.loading = false;
    updateTapeButton();
    publishState();
  }
}

async function maybeRefreshTapeAvailability() {
  if (HOST_CONFIG.disableTapeRemote === true) {
    state.tape.checkedKey = `${state.recordHash}|local-only`;
    state.tape.available = false;
    updateTapeButton();
    return;
  }
  const releaseId = String(state.recordReleaseId || "").trim();
  const checkedKey = `${state.recordHash}|${releaseId}`;
  if (!releaseId || state.tape.checkedKey === checkedKey) {
    updateTapeButton();
    return;
  }
  state.tape.checkedKey = checkedKey;
  state.tape.available = false;
  state.tape.active = false;
  state.tape.source = null;
  state.tape.releaseId = "";
  state.tape.sourceLabel = "";
  updateTapeButton();
  const localMeta = await readTapeMasterMeta(releaseId);
  if (state.tape.checkedKey !== checkedKey) return;
  let available = Boolean(localMeta && (Number(localMeta.frameCount) > 0 || Number(localMeta.audioLength) > 0));
  if (!available) {
    const manifest = await fetchTapeMasterRemoteManifest(releaseId);
    if (state.tape.checkedKey !== checkedKey) return;
    available = Boolean(manifest);
  }
  state.tape.available = available;
  updateTapeButton();
}

async function flushQueuedSeek() {
  if (state.seekInFlight || state.queuedSeekSeconds == null) return;
  let seconds = state.queuedSeekSeconds;
  state.queuedSeekSeconds = null;
  state.seekInFlight = true;
  const loadSequence = state.loadSequence;
  const seekDispatchSerial = ++state.seekDispatchSerial;
  // Original seekPlaybackToRatio: cueing a spinning record by eye lands
  // 50–140 ms early (DJs aim ahead of the beat) and plays the needle-drop
  // foley while the stylus settles.
  const view = deckView();
  const cueingAudibleGroove = Boolean(view?.transport_on && !view?.needle_lifted && view?.loaded);
  if (cueingAudibleGroove) {
    if (view?.playing) seconds = Math.max(0, seconds - (0.05 + Math.random() * 0.09));
    state.node?.port.postMessage({ type: "needle-drop" });
  }
  try {
    await dispatch(
      { type: "seek", deck: "a", seconds },
      { loadSequence },
    );
  } finally {
    if (seekDispatchSerial !== state.seekDispatchSerial) return;
    state.seekInFlight = false;
    if (state.queuedSeekSeconds != null) void flushQueuedSeek();
  }
}

function queueSeek(seconds) {
  if (!seekInteractionReady()) return false;
  interruptScratchReplayNow("Seek control");
  invalidateEndTransition();
  state.positionFrames = secondsToFrames(seconds);
  seekWorklet(state.positionFrames);
  state.queuedSeekSeconds = seconds;
  clearTimeout(state.seekTimer);
  state.seekTimer = setTimeout(() => void flushQueuedSeek(), 35);
  return true;
}

function rejectPendingCoreRequests(error, worker = null) {
  for (const [id, request] of state.pending) {
    if (worker && request.worker !== worker) continue;
    state.pending.delete(id);
    clearTimeout(request.timeoutId);
    request.reject(error);
  }
}

function handleCoreWorkerFailure(worker, error) {
  if (state.worker !== worker) return;
  state.worker = null;
  worker.terminate();
  rejectPendingCoreRequests(error, worker);
  const loadSequence = state.loadSequence;
  if (loadSequence > 0 && !currentLoadFailedOrFailing()) {
    void failCurrentLoad(loadSequence, error, "Player core failed");
  } else {
    setStatus(`Player core failed: ${error.message || error}`);
  }
}

function attachCoreWorkerHandlers(worker) {
  worker.onmessage = event => {
    const request = state.pending.get(event.data?.id);
    log.receive(
      `core:${event.data?.type || (event.data?.ok ? "response" : "error")}`,
      event.data,
      request?.telemetry ? { telemetry: true } : undefined,
    );
    const { id, ok, result, error } = event.data;
    if (!request || request.worker !== worker) {
      if (id !== 0) log.warn("core-unmatched-response", event.data);
      return;
    }
    state.pending.delete(id);
    clearTimeout(request.timeoutId);
    if (ok) request.resolve(result);
    else request.reject(new Error(error));
  };
  worker.onerror = event => {
    event.preventDefault?.();
    handleCoreWorkerFailure(
      worker,
      new Error(event.message || "Player core worker failed"),
    );
  };
  worker.onmessageerror = () => {
    handleCoreWorkerFailure(worker, new Error("Player core worker response could not be decoded"));
  };
}

async function startCoreWorker() {
  const worker = new Worker(versionedAssetUrl("./player-core-worker.js"), { type: "module" });
  state.worker = worker;
  attachCoreWorkerHandlers(worker);
  log.action("core-worker-created", {});
  try {
    const result = await coreRequest(
      "init",
      { moduleUrl: versionedAssetUrl("./record-player/record_player.js") },
      { timeoutMs: 15_000 },
    );
    if (state.worker !== worker) throw new Error("Player core worker was replaced during initialisation");
    state.view = result.view;
    const controls = [];
    if (Math.abs(state.volume - 1) > 0.000001) {
      controls.push({ type: "set_channel_gain", deck: "a", value: state.volume });
    }
    if (Math.abs(state.crossfader - 0.5) > 0.000001) {
      controls.push({ type: "set_crossfader", value: state.crossfader });
    }
    for (const event of controls) {
      const controlResult = await coreRequest("dispatch", { event });
      state.view = controlResult.view;
      for (const command of controlResult.commands) executeCommand(command);
    }
    return result;
  } catch (error) {
    handleCoreWorkerFailure(worker, error);
    throw error;
  }
}

async function ensureCoreWorker() {
  if (state.coreInitialisePromise) {
    await state.coreInitialisePromise;
    return;
  }
  if (state.worker) return;
  const promise = startCoreWorker();
  state.coreInitialisePromise = promise;
  try {
    await promise;
  } finally {
    if (state.coreInitialisePromise === promise) state.coreInitialisePromise = null;
  }
}

function coreRequest(type, payload = {}, { timeoutMs = 5_000 } = {}) {
  return new Promise((resolve, reject) => {
    const worker = state.worker;
    if (!worker) {
      reject(new Error("Player core worker is unavailable"));
      return;
    }
    const id = ++state.requestId;
    const isPositionTelemetry = payload?.event?.type === "playback_position_observed";
    const timeoutId = setTimeout(() => {
      const request = state.pending.get(id);
      if (!request || request.worker !== worker) return;
      state.pending.delete(id);
      const error = new Error(`Player core ${type} request timed out`);
      request.reject(error);
      handleCoreWorkerFailure(worker, error);
    }, Math.max(1, Number(timeoutMs) || 5_000));
    state.pending.set(id, {
      resolve,
      reject,
      type,
      startedAt: performance.now(),
      telemetry: isPositionTelemetry,
      worker,
      timeoutId,
    });
    if (isPlayerVerboseLoggingEnabled()) log.send(`core:${type}`, { id, payload }, isPositionTelemetry ? { telemetry: true } : undefined);
    try {
      worker.postMessage({ id, type, payload });
    } catch (error) {
      state.pending.delete(id);
      clearTimeout(timeoutId);
      const failure = error instanceof Error ? error : new Error(String(error));
      reject(failure);
      handleCoreWorkerFailure(worker, failure);
    }
  });
}

async function dispatch(event, { loadSequence = null } = {}) {
  const isPositionTelemetry = event?.type === "playback_position_observed";
  if (isPlayerVerboseLoggingEnabled()) log.action(`dispatch:${event?.type || "unknown"}`, event, isPositionTelemetry ? { telemetry: true } : undefined);
  await ensureCoreWorker();
  const result = await coreRequest("dispatch", { event });
  if (
    loadSequence != null
    && (
      loadSequence !== state.loadSequence
      || state.failedLoadSequence === loadSequence
    )
  ) {
    return false;
  }
  state.view = result.view;
  for (const command of result.commands) executeCommand(command);
  render();
  await maybeAutoLowerNeedle();
  return true;
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

function timedRegionDurationSeconds(turns) {
  const rpm = Number(state.rpm) || Number(state.baseRpm) || 45;
  return rpm > 0 ? Math.max(0, Number(turns) || 0) * (60 / rpm) : 0;
}

async function startLeadInPlayback() {
  invalidateEndTransition();
  const view = deckView();
  if (!view?.loaded) {
    await dispatch({ type: "toggle_transport", deck: "a" });
    return;
  }
  const durationSeconds = timedRegionDurationSeconds(LEAD_IN_TURNS);
  if (durationSeconds <= 0) {
    await dispatch({ type: "toggle_playback", deck: "a" });
    return;
  }
  state.needleAutoBehaviorEnabled = false;
  state.node?.port.postMessage({ type: "needle", lifted: false });
  state.node?.port.postMessage({ type: "needle-drop" });
  await dispatch({ type: "start_timed_region", region: "lead_in", now_ms: performance.now(), duration_seconds: durationSeconds });
}

async function stopPlaybackTransport() {
  invalidateEndTransition();
  await dispatch({ type: "stop_timed_region", region: "lead_in", completed: false }).catch(() => {});
  await dispatch({ type: "stop_timed_region", region: "deadwax", completed: false }).catch(() => {});
  await dispatch({ type: "set_transport", deck: "a", running: false });
  await dispatch({
    type: "set_needle",
    deck: "a",
    lifted: true,
    observed_playback_seconds: framesToSeconds(state.positionFrames),
  });
  state.node?.port.postMessage({ type: "needle", lifted: true });
}

async function toggleStartStopPlayback() {
  const view = deckView();
  if (view?.transport_on || view?.playing || state.view?.lead_in_active || state.view?.deadwax_active) {
    await stopPlaybackTransport();
  } else {
    await startLeadInPlayback();
  }
}


function executeCommand(command) {
  if (isPlayerVerboseLoggingEnabled()) log.action(`command:${command?.type || "unknown"}`, command);
  if (command.type === "set_packet_gain") {
    state.packetGain = Math.max(0, Number(command.gain) || 0);
    updateOutputGain(command.ramp_ms);
    return;
  }
  if (command.type === "set_mixer_track_gain" && Number(command.track) === 0) {
    state.mixerGain = Math.max(0, Number(command.gain) || 0);
    updateOutputGain(command.ramp_ms);
    return;
  }
  if (!state.node) { log.warn("command-without-worklet", command); return; }
  if (command.type === "set_motor") {
    state.node.port.postMessage({ type: "transport", running: command.running });
  } else if (command.type === "start_packet_playback") {
    state.pendingAutomaticDeadwax = null;
    const playbackEpoch = ++state.playbackEpoch;
    state.node.port.postMessage({
      type: "play",
      position: secondsToFrames(command.offset_seconds),
      rate: command.rate,
      handoff: Boolean(command.platter_handoff),
      playbackEpoch,
    });
  } else if (command.type === "stop_packet_playback") {
    if (!command.platter_handoff) state.pendingAutomaticDeadwax = null;
    const playbackEpoch = ++state.playbackEpoch;
    state.node.port.postMessage({
      type: "stop",
      handoff: Boolean(command.platter_handoff),
      playbackEpoch,
    });
  } else if (command.type === "seek_packet_playback") {
    seekWorklet(secondsToFrames(command.offset_seconds));
  } else if (command.type === "set_scratch_transport") {
    state.node.port.postMessage({ type: "scratch-transport", handContact: Boolean(command.hand_contact), motorRate: Number(command.motor_rate) || 0 });
  } else if (command.type === "set_scratch_target") {
    state.node.port.postMessage({ type: "scratch", active: true, position: command.position_frames, rate: command.rate, impulse: command.impulse });
  } else if (command.type === "set_scratch_position") {
    seekWorklet(command.position_frames, command.impulse);
  } else if (command.type === "start_surface_region") {
    const durationSeconds = Math.max(0, Number(command.duration_seconds) || 0);
    const automaticDeadwax = command.region === "deadwax"
      ? state.pendingAutomaticDeadwax
      : null;
    state.pendingAutomaticDeadwax = null;
    const startOutputFrame = Number.isFinite(automaticDeadwax?.startOutputFrame)
      ? automaticDeadwax.startOutputFrame
      : audioFrameNow();
    const durationFrames = Number.isFinite(automaticDeadwax?.durationFrames)
      ? Math.max(0, automaticDeadwax.durationFrames)
      : Math.max(0, Math.round(durationSeconds * (state.context?.sampleRate || state.sampleRate)));
    const regionId = ++state.surfaceRegionId;
    state.surfaceRegion = {
      region: command.region,
      regionId,
      startedAtMs: performance.now(),
      durationSeconds,
      startOutputFrame,
      durationFrames,
    };
    state.node.port.postMessage({
      type: "surface-region",
      action: "start",
      region: command.region,
      regionId,
      durationSeconds,
    });
    publishState();
    clearTimeout(state.regionTimer);
    // Region completion is driven by AudioWorklet frames. A main-thread timer
    // can be throttled in a background tab and move the groove transition off
    // the audio clock.
    state.regionTimer = 0;
  } else if (command.type === "stop_surface_region") {
    clearTimeout(state.regionTimer);
    state.pendingAutomaticDeadwax = null;
    state.surfaceRegionId += 1;
    if (state.surfaceRegion?.region === command.region) state.surfaceRegion = null;
    state.node.port.postMessage({
      type: "surface-region",
      action: "stop",
      region: command.region,
      regionId: state.surfaceRegionId,
    });
    publishState();
  }
}

function seekWorklet(position, impulse = 0) {
  invalidateEndTransition();
  const generation = ++state.pendingSeekGeneration;
  state.positionFrames = position;
  requestPcmWindow(position, { resetPosition: true });
  state.node.port.postMessage({ type: "seek", position, generation, impulse: Number(impulse) || 0 });
}

async function initialiseAudio() {
  if (state.context && state.node) return;
  if (state.audioInitialisePromise) {
    await state.audioInitialisePromise;
    return initialiseAudio();
  }
  state.audioInitialisePromise = initialiseAudioOnce();
  try {
    await state.audioInitialisePromise;
  } catch (error) {
    state.node?.disconnect();
    state.gainNode?.disconnect();
    await state.context?.close().catch(() => {});
    state.node = null;
    state.gainNode = null;
    state.context = null;
    throw error;
  } finally {
    state.audioInitialisePromise = null;
  }
}

async function initialiseAudioOnce() {
  state.context = new AudioContext({ latencyHint: "interactive" });
  const recordPlayerWasmResponse = await fetch(versionedAssetUrl("./record-player/record_player_bg.wasm"));
  if (!recordPlayerWasmResponse.ok) throw new Error(`Failed to load record-player WASM: ${recordPlayerWasmResponse.status}`);
  const recordPlayerWasmModule = await WebAssembly.compileStreaming(recordPlayerWasmResponse);
  await state.context.audioWorklet.addModule(versionedAssetUrl("./player-worklet.js"));
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
    if (isPlayerVerboseLoggingEnabled()) log.receive(`worklet:${event.data?.type || "message"}`, event.data);
    handleWorkletMessage(event);
  };
  const audioNode = state.node;
  audioNode.onprocessorerror = () => {
    if (state.node !== audioNode) return;
    const error = new Error("AudioWorklet processor failed during playback");
    // A dead processor cannot publish the replay-ended acknowledgement. Settle
    // that half of the transaction locally before entering load teardown.
    for (const request of state.scratchReplayRequests.values()) {
      acknowledgeScratchReplayRequest(request);
    }
    const failedContext = state.context;
    const failedGainNode = state.gainNode;
    const failedCaptureNode = state.captureNode;
    state.node = null;
    state.gainNode = null;
    state.captureNode = null;
    state.context = null;
    audioNode.disconnect();
    failedGainNode?.disconnect();
    failedCaptureNode?.disconnect?.();
    void failedContext?.close().catch(() => {});
    const loadSequence = state.loadSequence;
    if (loadSequence > 0) {
      void failCurrentLoad(loadSequence, error, "Audio engine failed");
    } else {
      void cancelScratchReplays(error);
      setStatus(error.message);
      publishState();
    }
  };
  state.node.port.postMessage({ type: "scratch-preset", preset: state.scratchPreset });
  state.node.port.postMessage({ type: "scratch-clicks", clicks: state.scratchClicks });
  state.node.port.postMessage({ type: "native-rpm", rpm: state.baseRpm });
  state.node.port.postMessage({
    type: "end-behavior",
    cleanEnd: state.cleanEnd,
    deadwaxTurns: DEADWAX_TURNS,
  });
  state.node.port.postMessage({
    type: "hf-acceleration-limit",
    strength: state.highFrequencyAccelerationLimit,
  });
  state.node.port.postMessage({
    type: "stylus-tracing-limit",
    strength: state.stylusTracingLimit,
  });
  updateOutputGain(0);
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
    const response = await fetch(versionedAssetUrl("./assets/audio/needle-surface.opus"), { cache: "force-cache" });
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
  const streamGeneration = Number(message.streamGeneration);
  const matchingReplayRequest = message.type === "scratch-replay-ended"
    ? state.scratchReplayRequests.get(message.id)
    : null;
  if (
    Number.isFinite(streamGeneration)
    && streamGeneration !== state.pcmStreamGeneration
    && !matchingReplayRequest
  ) {
    if (isPlayerVerboseLoggingEnabled()) {
      log.action("worklet-message-stale", {
        type: message.type,
        received: streamGeneration,
        expected: state.pcmStreamGeneration,
      });
    }
    return;
  }
  if (message.type === "window-request") {
    requestPcmWindow(message.position, {
      resetPosition: Boolean(message.resetPosition),
      workletRequestId: message.workletRequestId,
    });
  } else if (message.type === "window-applied") {
    state.pcmWindowAwaitingApply = false;
    if (message.applied === false) {
      const error = new Error(message.error || "Audio worklet rejected a PCM window");
      setStatus(`PCM window error: ${error.message}`);
      handlePcmWindowFailure(error);
      return;
    }
    state.pcmWindowReady = true;
    state.pcmWindowAppliedStart = Math.max(0, Math.floor(Number(message.windowStart) || 0));
    state.pcmWindowAppliedEnd = Math.max(0, Math.floor(Number(message.windowEnd) || 0));
    state.pcmWindowAppliedAvailableEnd = Math.max(0, Math.floor(Number(message.availableEnd) || 0));
    if (message.resumed) state.buffering = false;
    state.streamDecodedFrames = Math.max(
      state.streamDecodedFrames,
      Math.floor(Number(message.availableEnd ?? message.decodedLength) || 0),
    );
    state.pcmWindowReadyResolve?.(message);
    state.pcmWindowReadyResolve = null;
    state.pcmWindowReadyReject = null;
    void handleWorkletBuffered({ decodedLength: state.streamDecodedFrames });
    flushQueuedPcmWindowRequest();
  } else if (message.type === "position") {
    if (Number.isFinite(message.outputFrame)) state.lastOutputFrame = message.outputFrame;
    const replayActive = scratchReplayActive();
    state.replayScratching = replayActive && Boolean(message.scratching);
    if (!replayActive) {
      if (typeof message.scratchPreset === "string") {
        state.scratchPreset = normalizeScratchPreset(message.scratchPreset, state.scratchPreset);
      }
      if (Number.isFinite(message.scratchClicks)) {
        state.scratchClicks = normalizeScratchClicks(message.scratchClicks, state.scratchClicks);
      }
      updateScratchTechniqueControls();
    }
    if (Number.isFinite(message.effectiveRate)) state.effectiveRate = message.effectiveRate;
    if (Number.isFinite(message.scratchGate)) state.scratchGate = message.scratchGate;
    if (Number.isFinite(message.scratchGateTarget)) state.scratchGateTarget = message.scratchGateTarget;
    if (Number.isFinite(message.scratchDirection)) state.scratchDirection = message.scratchDirection;
    if (typeof message.scratchMoving === "boolean") state.scratchMoving = message.scratchMoving;
    if (Number.isFinite(message.scratchGatePhase)) state.scratchGatePhase = message.scratchGatePhase;
    if (Number.isFinite(message.scratchStrokeProgress)) state.scratchStrokeProgress = message.scratchStrokeProgress;
    if (!replayActive) {
      if (Number.isFinite(message.highFrequencyAccelerationLimit)) {
        state.highFrequencyAccelerationLimit = message.highFrequencyAccelerationLimit;
      }
      if (Number.isFinite(message.stylusTracingLimit)) {
        state.stylusTracingLimit = message.stylusTracingLimit;
      }
    }
    if (Number.isFinite(message.inputLatencyMs)) state.pointerToAudioLatencyMs = message.inputLatencyMs;

    const rotationTurns = Number(message.platterRotationTurns);
    if (!replayActive && Number.isFinite(rotationTurns)) {
      if (state.lastDspRotationTurns != null && !state.scratching) {
        state.rotation = (state.rotation + (rotationTurns - state.lastDspRotationTurns) * 360) % 360;
        elements.platter.style.setProperty("--rotation", `${state.rotation}deg`);
      }
      state.lastDspRotationTurns = rotationTurns;
    }

    // Replay telemetry describes its temporary Rust transaction. Keep useful
    // gate/rate meters live, but do not move the public cursor or feed replay
    // positions/configuration back into the persistent Rust player core.
    if (replayActive) {
      state.buffering = Boolean(message.buffering);
      publishState();
      return;
    }

    if (state.pendingSeekGeneration !== state.acknowledgedSeekGeneration) {
      publishState();
      return;
    }
    state.positionFrames = message.position;
    state.buffering = Boolean(message.buffering);
    state.lastReportedPosition = message.position;
    const observedAtMs = performance.now();
    if (
      !message.scratching
      && !state.scratching
      && (
        observedAtMs - state.lastCoreObservedAtMs >= 100
        || Math.abs(message.position - state.lastCoreObservedPositionFrames) >= state.sampleRate * 0.25
      )
    ) {
      state.lastCoreObservedAtMs = observedAtMs;
      state.lastCoreObservedPositionFrames = message.position;
      void dispatch({
        type: "playback_position_observed",
        deck: "a",
        seconds: framesToSeconds(message.position),
      }).catch(error => log.warn("core-position-observation-failed", {
        message: error instanceof Error ? error.message : String(error),
      }));
    }
    if (!state.draggingSeek) elements.seek.value = String(state.duration > 0 ? framesToSeconds(message.position) / state.duration : 0);
    publishState();
  } else if (message.type === "seeked") {
    state.acknowledgedSeekGeneration = Math.max(state.acknowledgedSeekGeneration, message.generation ?? 0);
    state.positionFrames = message.position;
  } else if (message.type === "buffering") {
    if (!scratchReplayActive()) {
      state.positionFrames = Math.max(0, Number(message.position) || state.positionFrames);
      state.lastReportedPosition = state.positionFrames;
    }
    state.buffering = true;
    setStatus("Buffering decoded groove audio…");
    publishState();
  } else if (message.type === "buffered") {
    state.buffering = false;
    void handleWorkletBuffered(message);
  } else if (message.type === "worklet-error") {
    setStatus(`Audio engine error: ${message.message || message.stage || "unknown error"}`);
    console.error("[vin.yl.player] AudioWorklet error", message);
  } else if (message.type === "ended") {
    const playbackEpoch = Number(message.playbackEpoch);
    if (playbackEpoch !== state.playbackEpoch) return;
    state.positionFrames = message.position;
    if (Number.isFinite(message.outputFrame)) state.lastOutputFrame = message.outputFrame;
    const endTransitionGeneration = ++state.endTransitionGeneration;
    const automaticDeadwax = message.deadwaxStarted
      ? {
        endTransitionGeneration,
        startOutputFrame: Math.max(0, Number(message.outputFrame) || 0),
        durationFrames: Math.max(0, Number(message.deadwaxDurationFrames) || 0),
      }
      : null;
    state.pendingAutomaticDeadwax = automaticDeadwax;
    void (async () => {
      await dispatch({ type: "playback_ended", deck: "a" });
      if (
        playbackEpoch !== state.playbackEpoch
        || endTransitionGeneration !== state.endTransitionGeneration
      ) {
        if (state.pendingAutomaticDeadwax === automaticDeadwax) {
          state.pendingAutomaticDeadwax = null;
        }
        return;
      }
      const view = deckView();
      if (!state.cleanEnd && view?.transport_on && !view?.needle_lifted) {
        await dispatch({
          type: "start_timed_region",
          region: "deadwax",
          now_ms: performance.now(),
          duration_seconds: timedRegionDurationSeconds(DEADWAX_TURNS),
        });
      } else if (state.cleanEnd && view?.transport_on) {
        await stopPlaybackTransport();
      } else if (state.pendingAutomaticDeadwax === automaticDeadwax) {
        state.pendingAutomaticDeadwax = null;
      }
      if (playbackEpoch !== state.playbackEpoch && !state.surfaceRegion) return;
      publishState();
    })().catch(error => {
      if (state.pendingAutomaticDeadwax === automaticDeadwax) {
        state.pendingAutomaticDeadwax = null;
      }
      log.warn("programme-end-transition-failed", {
        message: error instanceof Error ? error.message : String(error),
      });
    });
  } else if (message.type === "surface-region-ended") {
    const region = message.region === "deadwax" ? "deadwax" : "lead_in";
    if (
      state.surfaceRegion?.region !== region
      || Number(message.regionId) !== state.surfaceRegion.regionId
    ) {
      return;
    }
    clearTimeout(state.regionTimer);
    state.regionTimer = 0;
    if (Number.isFinite(message.outputFrame)) state.lastOutputFrame = message.outputFrame;
    void dispatch({ type: "timed_region_elapsed", region });
  } else if (message.type === "scratch-replay-ended") {
    const request = matchingReplayRequest;
    acknowledgeScratchReplayRequest(request, message);
    if (request && !request.cancelled) {
      state.replayScratching = false;
      settleScratchReplayRequest(request, "resolve", {
        cancelled: Boolean(message.cancelled),
        positionFrames: Number(message.position) || 0,
      });
      if (state.scratchReplayRequests.get(request.id) === request) {
        state.scratchReplayRequests.delete(request.id);
      }
      if (!state.scratchReplayRequests.size) state.replayScratching = false;
      publishState();
    }
  }
}

async function loadFile(file, options = {}) {
  const loadSequence = ++state.loadSequence;
  state.loadInFlightSequence = loadSequence;
  invalidateEndTransition();
  clearPendingSeekTransaction();
  clearLiveScratchInteraction();
  cancelPendingStreamReady(loadSupersededError());
  try {
    await cancelScratchReplays(new Error("Player load cancelled scratch replay"));
    assertCurrentLoad(loadSequence);
    return await loadFileForSequence(file, options, loadSequence);
  } catch (error) {
    if (loadSequence !== state.loadSequence) throw loadSupersededError();
    await failCurrentLoad(loadSequence, error, "Record load failed");
    throw error;
  } finally {
    if (state.loadInFlightSequence === loadSequence) state.loadInFlightSequence = 0;
  }
}

async function loadFileForSequence(
  file,
  { cache, resumeAudio = true, cleanEnd = false } = {},
  loadSequence,
) {
  if (isPlayerLoggingEnabled()) {
    console.log("[vin.yl.player] loadFile:start", {
      name: file?.name || "",
      size: file?.size || 0,
      type: file?.type || "",
      resumeAudio,
      hasCacheArg: Boolean(cache),
      hasActiveCache: Boolean(state.cacheHandler),
    });
  }
  await initialiseAudio();
  assertCurrentLoad(loadSequence);
  if (resumeAudio) {
    await state.context.resume();
    assertCurrentLoad(loadSequence);
  }
  const previousView = deckView();
  if (previousView?.transport_on || previousView?.playing || state.view?.lead_in_active || state.view?.deadwax_active) {
    await stopPlaybackTransport();
    assertCurrentLoad(loadSequence);
  }
  await dispatch({ type: "set_load_state", deck: "a", status: "loading", loaded: false, duration_seconds: 0 });
  assertCurrentLoad(loadSequence);
  if (cache !== undefined) state.cacheHandler = normalizeCacheHandler(cache);
  if (state.decoder) {
    state.decoder.close();
  }
  disposePcmWindowTransport();
  const decoder = new RecordDecoderClient(versionedAssetUrl("./record-decoder-worker.js"), {
    loggingEnabled: isPlayerLoggingEnabled(),
    cache: state.cacheHandler,
  });
  state.decoder = decoder;
  decoder.setCache(state.cacheHandler);
  await decoder.initialise();
  assertCurrentLoad(loadSequence);
  state.streamInitialised = false;
  state.streamReady = false;
  state.streamReadyMarking = false;
  state.streamDecodedFrames = 0;
  state.buffering = false;
  state.duration = 0;
  state.metadataDuration = 0;
  state.cleanEnd = Boolean(cleanEnd);
  state.node.port.postMessage({
    type: "end-behavior",
    cleanEnd: state.cleanEnd,
    deadwaxTurns: DEADWAX_TURNS,
  });
  state.streamAppendChain = Promise.resolve();
  resetStreamReadyPromise();
  const streamReadyPromise = state.streamReadyPromise;
  state.decodeProgressText = "";
  state.programmeMap = null;
  state.recordReleaseId = "";
  state.basePcmSource = null;
  resetTapeState();
  elements.play.disabled = false;
  elements.needle.disabled = false;
  elements.seek.disabled = true;
  updateTapeButton();
  if (state.recordObjectUrl) URL.revokeObjectURL(state.recordObjectUrl);
  state.recordObjectUrl = URL.createObjectURL(file);
  elements.recordImage.src = state.recordObjectUrl;
  setStatus(`Inspecting ${file.name}…`);
  const sourceBytes = await file.arrayBuffer();
  assertCurrentLoad(loadSequence);
  if (isPlayerLoggingEnabled()) {
    console.log("[vin.yl.player] loadFile:bytes", {
      name: file?.name || "",
      bytes: sourceBytes.byteLength || 0,
    });
  }
  const cacheKey = await recordCacheKey(sourceBytes);
  assertCurrentLoad(loadSequence);
  state.recordHash = cacheKey;
  const inspected = await decoder.inspect(sourceBytes.slice(0));
  assertCurrentLoad(loadSequence);
  state.recordHeaderProof = inspected.recordHeaderProof || null;
  try {
    const descriptorJson = await decodeRecordDescriptorJson(sourceBytes, inspected.recordProfile || "");
    assertCurrentLoad(loadSequence);
    state.recordDescriptorJson = descriptorJson;
  } catch (error) {
    if (loadSequence !== state.loadSequence) throw loadSupersededError();
    state.recordDescriptorJson = "";
    console.warn("[vin.yl.player] record descriptor decode failed", error);
  }
  if (typeof state.cacheHandler?.setRecordContext === "function") {
    await state.cacheHandler.setRecordContext({
      descriptorJson: state.recordDescriptorJson,
      recordHeaderProof: state.recordHeaderProof,
      recordProfile: inspected.recordProfile || "",
    });
    assertCurrentLoad(loadSequence);
  }
  if (isPlayerLoggingEnabled()) {
    console.log("[vin.yl.player] loadFile:inspect", {
      recordProfile: inspected.recordProfile || "",
      payloadContainer: inspected.payloadContainer || "",
      releaseId: inspected.releaseId || "",
      hasProgrammeMap: Boolean(inspected.programmeMapJson),
      hasRecordDescriptorJson: Boolean(state.recordDescriptorJson),
    });
  }
  state.programmeMap = parseJsonObject(inspected.programmeMapJson, null);
  const programmeSampleRate = Math.max(1, Number(state.programmeMap?.sampleRate) || 48000);
  const programmeDuration = Number(state.programmeMap?.durationMs) > 0
    ? Number(state.programmeMap.durationMs) / 1000
    : Number(state.programmeMap?.totalSamples) > 0
      ? Number(state.programmeMap.totalSamples) / programmeSampleRate
      : 0;
  if (programmeDuration > 0) {
    state.sampleRate = programmeSampleRate;
    state.duration = programmeDuration;
    state.metadataDuration = programmeDuration;
  }
  state.recordReleaseId = String(inspected.releaseId || "").trim();
  elements.metadata.hidden = isEmbedMode();
  elements.metaProfile.textContent = inspected.recordProfile || "unknown";
  elements.metaContainer.textContent = inspected.payloadContainer || "unknown";
  elements.metaRelease.textContent = inspected.releaseId || "unsigned / unavailable";
  publishState();
  void maybeRefreshTapeAvailability();
  state.baseRpm = profileRpm(inspected.recordProfile);
  state.rpm = state.baseRpm;
  updateRpmButtons();
  state.node.port.postMessage({ type: "native-rpm", rpm: state.baseRpm });
  setStatus(`Decoding ${file.name}…`);
  let lastLoggedDecodeChunk = -1;
  const decodePromise = decoder.decode(sourceBytes, inspected.recordProfile || "", {
      recordBindingHex: state.recordHash,
    }, progress => {
      if (
        loadSequence !== state.loadSequence
        || state.failedLoadSequence === loadSequence
      ) return;
      const chunksProcessed = Math.max(0, Math.floor(Number(progress?.chunksProcessed) || 0));
      const totalChunks = Math.max(0, Math.floor(Number(progress?.totalChunks) || 0));
      if (totalChunks > 0 && chunksProcessed !== lastLoggedDecodeChunk) {
        lastLoggedDecodeChunk = chunksProcessed;
        console.log(`decoded ${chunksProcessed}/${totalChunks}`);
      }
      state.decodeProgressText = formatDecodeProgress(progress);
      renderDecodeStatus();
      if (Array.isArray(progress.decodedPcmSegments) && progress.decodedPcmSegments.length) {
        const segments = progress.decodedPcmSegments;
        const appendPromise = state.streamAppendChain
          .then(() => (
            loadSequence === state.loadSequence
              ? appendProgressiveSegments(segments, loadSequence)
              : undefined
          ))
          .catch(async error => {
            if (loadSequence === state.loadSequence) {
              setStatus(`Progressive playback failed: ${error.message || error}`);
              await failCurrentLoad(loadSequence, error, "Progressive playback failed");
            }
            throw error;
          });
        // Full decode consumes this chain later, but readiness must fail as soon
        // as an append does. Keep the retained rejection observed meanwhile.
        appendPromise.catch(() => {});
        state.streamAppendChain = appendPromise;
      }
    });
  decodePromise.then(async decoded => {
    if (
      loadSequence !== state.loadSequence
      || state.failedLoadSequence === loadSequence
    ) return;
    await state.streamAppendChain;
    if (loadSequence !== state.loadSequence) return;

    const sampleRate = Math.max(1, Number(decoded.sampleRate) || 48000);
    const audioLength = Math.max(1, Number(decoded.audioLength) || 0);
    const s16Buffers = Array.isArray(decoded.s16ChannelBuffers) ? decoded.s16ChannelBuffers : [];
    if (!s16Buffers.length) throw new Error("Record decoder returned no PCM channels");
    state.baseRpm = profileRpm(inspected.recordProfile);
    state.rpm = state.baseRpm;
    updateRpmButtons();
    state.node.port.postMessage({ type: "native-rpm", rpm: state.baseRpm });
    storeBasePcmSource({ sampleRate, audioLength, s16ChannelBuffers: s16Buffers });
    updateTapeButton();
    if (!state.streamInitialised) {
      await loadDecodedPcm({ sampleRate, audioLength, s16ChannelBuffers: s16Buffers }, loadSequence);
      if (loadSequence !== state.loadSequence) return;
    } else if (state.streamDecodedFrames < audioLength) {
      assertCurrentLoad(loadSequence);
      if (!state.pcmWindowWorker) throw new Error("PCM window worker is unavailable");
      state.pcmWindowWorker.postMessage({
        type: "append-segments",
        segments: [{ startFrame: 0, endFrame: audioLength, channelBuffers: s16Buffers }],
      }, s16Buffers);
    }
    await Promise.all([
      state.pcmWindowReadyPromise,
      waitForPcmAvailability(audioLength),
    ]);
    assertCurrentLoad(loadSequence);
    state.node.port.postMessage({ type: "stream-complete" });
    if (!state.streamReady) {
      await ensureStreamReady(loadSequence);
    }
    publishState();
    if (isPlayerLoggingEnabled()) {
      console.log("[vin.yl.player] loadFile:complete", {
        name: file?.name || "",
        durationSeconds: state.duration,
        sampleRate,
        payloadContainer: inspected.payloadContainer || "",
      });
    }
    setStatus(`${file.name} · ${state.duration.toFixed(1)}s · ${sampleRate} Hz · ${inspected.payloadContainer || "record"}`);
  }).catch(error => {
    if (loadSequence !== state.loadSequence) return;
    state.streamReadyReject?.(error);
    state.streamReadyReject = null;
    void failCurrentLoad(loadSequence, error, "Playback decode failed");
  });

  // Start as soon as the worklet has a small contiguous lead-in; the rest
  // continues decoding and appending in the background.
  await streamReadyPromise;
  if (loadSequence !== state.loadSequence) return null;
}

async function loadRecordFromUrl(url, options = {}) {
  const requestSequence = ++state.loadSequence;
  state.loadInFlightSequence = requestSequence;
  invalidateEndTransition();
  clearPendingSeekTransaction();
  clearLiveScratchInteraction();
  cancelPendingStreamReady(loadSupersededError());
  try {
    await cancelScratchReplays(new Error("Player load cancelled scratch replay"));
    assertCurrentLoad(requestSequence);
    const resolved = new URL(String(url || ""), globalThis.location?.href || import.meta.url);
    if (isPlayerLoggingEnabled()) {
      console.log("[vin.yl.player] loadRecordFromUrl:start", {
        input: String(url || ""),
        resolved: resolved.toString(),
        options,
      });
    }
    const response = await fetch(resolved.toString(), { cache: "force-cache" });
    assertCurrentLoad(requestSequence);
    if (!response.ok) {
      console.error("[vin.yl.player] loadRecordFromUrl:fetch-failed", {
        resolved: resolved.toString(),
        status: response.status,
      });
      throw new Error(`Failed to load record from ${resolved}: ${response.status}`);
    }
    const blob = await response.blob();
    assertCurrentLoad(requestSequence);
    if (isPlayerLoggingEnabled()) {
      console.log("[vin.yl.player] loadRecordFromUrl:fetched", {
        resolved: resolved.toString(),
        size: blob.size || 0,
        type: blob.type || "",
      });
    }
    const pathname = resolved.pathname.split("/").pop() || "record.png";
    const file = new File([blob], pathname, { type: blob.type || "image/png" });
    return await loadFileForSequence(
      file,
      { resumeAudio: false, ...(options || {}) },
      requestSequence,
    );
  } catch (error) {
    if (requestSequence !== state.loadSequence) throw loadSupersededError();
    await failCurrentLoad(requestSequence, error, "Record download failed");
    throw error;
  } finally {
    if (state.loadInFlightSequence === requestSequence) state.loadInFlightSequence = 0;
  }
}

async function configureStartupCache() {
  const cacheUrl = startupCacheUrl();
  if (!cacheUrl) {
    if (isPlayerLoggingEnabled()) {
      console.warn("[vin.yl.player] configureStartupCache skipped: no tape_url configured");
    }
    return null;
  }
  if (isPlayerLoggingEnabled()) {
    console.info("[vin.yl.player] configureStartupCache:start", { cacheUrl });
  }
  const cache = createRemoteOpusChunkCacheHandler({
    apiBaseUrl: cacheUrl,
    disableRemoteCache: HOST_CONFIG.disableRemoteCache === true,
  });
  state.cacheHandler = normalizeCacheHandler(cache);
  if (typeof state.cacheHandler.setRecordContext === "function" && state.recordDescriptorJson) {
    await state.cacheHandler.setRecordContext({
      descriptorJson: state.recordDescriptorJson,
      recordHeaderProof: state.recordHeaderProof,
      recordProfile: "",
    });
  }
  state.decoder?.setCache(state.cacheHandler);
  if (isPlayerLoggingEnabled()) {
    console.info("[vin.yl.player] configureStartupCache:ready", { cacheUrl });
  }
  return state.cacheHandler;
}

function render() {
  const view = deckView();
  if (!view) return;
  elements.play.textContent = (view.transport_on || view.playing || state.view?.lead_in_active || state.view?.deadwax_active) ? "STOP" : "START";
  elements.needle.textContent = view.needle_lifted ? "NEEDLE DOWN" : "NEEDLE UP";
  updateTapeButton();
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
  const outputSampleRate = state.context?.sampleRate || state.sampleRate;
  return Math.max(0, Math.round((state.context?.currentTime || 0) * outputSampleRate));
}

function pointerAudioTiming(inputTimeMs) {
  if (!state.context || !Number.isFinite(Number(inputTimeMs))) return {};
  let timestamp = Number(inputTimeMs);
  if (timestamp > 1_000_000_000_000 && Number.isFinite(performance.timeOrigin)) {
    timestamp -= performance.timeOrigin;
  }
  const ageMs = clamp(performance.now() - timestamp, 0, 1000);
  return {
    commandId: ++state.pointerCommandId,
    inputAudioTime: Math.max(0, state.context.currentTime - ageMs / 1000),
  };
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
    manualCrossfader: state.crossfader,
    motorRunning: Boolean(view?.transport_on ?? view?.playing),
    playing: Boolean(view?.playing),
    needleLifted: Boolean(view?.needle_lifted),
    preset: state.scratchPreset,
    clicks: state.scratchClicks,
    faderCurve: "sharp-0.08",
    highFrequencyAccelerationLimit: state.highFrequencyAccelerationLimit,
    stylusTracingLimit: state.stylusTracingLimit,
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
      if (previous && JSON.stringify(previous) === JSON.stringify(next)) return;
      events.push(next);
    },
    stop() {
      if (!active) return null;
      active = false;
      state.scratchRecorders.delete(this);
      const durationFrames = Math.max(
        events.length ? events[events.length - 1].frameOffset : 0,
        Math.max(0, audioFrameNow() - startFrame),
      );
      const outputSampleRate = state.context?.sampleRate || state.sampleRate;
      return Object.freeze(normalizeScratchPerformance({
        id: crypto.randomUUID(),
        schemaVersion: SCRATCH_PERFORMANCE_SCHEMA_VERSION,
        name: String(name || ""),
        recordHash: state.recordHash,
        releaseId: elements.metaRelease?.textContent || "",
        createdAt: new Date().toISOString(),
        sourceSampleRate: state.sampleRate,
        outputSampleRate,
        durationFrames,
        durationMs: durationFrames / outputSampleRate * 1000,
        engine: {
          name: "vin.yl.player.acoustic",
          version: 2,
          gateAlgorithmVersion: SCRATCH_GATE_ALGORITHM_VERSION,
          recordProfile: elements.metaProfile?.textContent || "",
          nativeRpm: state.baseRpm
        },
        initialState,
        events: events.map(event => ({ ...event })),
        effects: { acoustic: true, surface: true }
      }));
    },
    get active() { return active; }
  });
}

async function replayScratch(performance, { effects = "original" } = {}) {
  if (state.scratchReplayRequests.size > 0) throw new Error("A scratch replay is already active");
  if (state.loadInFlightSequence) throw new Error("Scratch replay is unavailable while a record is loading");
  if (
    state.failedLoadSequence === state.loadSequence
    || state.loadFailureSequence === state.loadSequence
  ) {
    throw new Error("Scratch replay is unavailable after a record load failure");
  }
  if (state.tape.loading) throw new Error("Scratch replay is unavailable while the TAPE source is changing");
  if (state.buffering || !state.streamReady) throw new Error("Scratch replay requires a ready, buffered record");
  if (
    state.seekInFlight
    || state.queuedSeekSeconds != null
    || state.pendingSeekGeneration !== state.acknowledgedSeekGeneration
  ) {
    throw new Error("Scratch replay is unavailable while a seek is pending");
  }
  if (state.scratching) throw new Error("Scratch replay is unavailable during live scratching");
  if (state.surfaceRegion || state.view?.lead_in_active || state.view?.deadwax_active) {
    throw new Error("Scratch replay is unavailable during lead-in or deadwax playback");
  }
  const id = ++state.scratchReplayId;
  let resolveCompletion;
  let rejectCompletion;
  let resolveWorkletAck;
  const completion = new Promise((resolve, reject) => {
    resolveCompletion = resolve;
    rejectCompletion = reject;
  });
  completion.catch(() => {});
  const workletAckPromise = new Promise(resolve => { resolveWorkletAck = resolve; });
  const request = {
    id,
    phase: "setup",
    cancelled: false,
    settled: false,
    workletStarted: false,
    workletAcknowledged: false,
    workletAckPromise,
    resolveWorkletAck,
    resolve: resolveCompletion,
    reject: rejectCompletion,
  };
  state.scratchReplayRequests.set(id, request);
  publishState();
  try {
    await initialiseAudio();
    assertScratchReplayRequestActive(request);
    await state.context.resume();
    assertScratchReplayRequestActive(request);
    const normalized = normalizeScratchPerformance(performance, {
      sourceSampleRate: state.sampleRate,
      outputSampleRate: state.context.sampleRate,
    });
    if (normalized.recordHash && state.recordHash && normalized.recordHash !== state.recordHash) {
      throw new Error("Scratch performance belongs to a different record");
    }
    if (state.surfaceRegion || state.view?.lead_in_active || state.view?.deadwax_active) {
      throw new Error("Scratch replay is unavailable during lead-in or deadwax playback");
    }
    request.phase = "active";
    state.node.port.postMessage({ type: "replay-scratch", id, performance: normalized, effectsMode: effects });
    request.workletStarted = true;
    return completion;
  } catch (error) {
    request.cancelled = true;
    if (!request.workletStarted) acknowledgeScratchReplayRequest(request);
    settleScratchReplayRequest(request, "reject", error);
    if (state.scratchReplayRequests.get(id) === request) state.scratchReplayRequests.delete(id);
    if (!state.scratchReplayRequests.size) state.replayScratching = false;
    publishState();
    throw error;
  }
}

function assertScratchReplayRequestActive(request) {
  if (
    request.cancelled
    || state.scratchReplayRequests.get(request.id) !== request
  ) {
    throw request.cancelError || new Error("Scratch replay was cancelled during setup");
  }
}

function settleScratchReplayRequest(request, action, value) {
  if (request.settled) return;
  request.settled = true;
  if (action === "resolve") request.resolve(value);
  else request.reject(value);
}

function acknowledgeScratchReplayRequest(request, message = null) {
  if (!request || request.workletAcknowledged) return;
  request.workletAcknowledged = true;
  request.resolveWorkletAck(message);
}

async function waitForScratchReplayAcknowledgement(request, timeoutMs = 1_000) {
  if (request.workletAcknowledged) return;
  let timeoutId = 0;
  await Promise.race([
    request.workletAckPromise,
    new Promise(resolve => {
      timeoutId = setTimeout(resolve, timeoutMs);
    }),
  ]);
  clearTimeout(timeoutId);
  if (!request.workletAcknowledged) {
    log.warn("scratch-replay-cancel-ack-timeout", { id: request.id, timeoutMs });
    acknowledgeScratchReplayRequest(request);
  }
}

async function cancelScratchReplays(error = new Error("Scratch replay cancelled")) {
  const requests = Array.from(state.scratchReplayRequests.values());
  if (!requests.length) return;
  state.replayScratching = false;
  for (const request of requests) {
    request.cancelled = true;
    request.cancelError = error;
    request.phase = "restoring";
    if (!request.workletStarted || !state.node?.port) acknowledgeScratchReplayRequest(request);
  }
  if (requests.some(request => request.workletStarted && !request.workletAcknowledged)) {
    try {
      state.node.port.postMessage({ type: "cancel-scratch-replay" });
    } catch (postError) {
      for (const request of requests) acknowledgeScratchReplayRequest(request);
      log.warn("scratch-replay-cancel-post-failed", {
        message: postError instanceof Error ? postError.message : String(postError),
      });
    }
  }
  await Promise.all(requests.map(async request => {
    await waitForScratchReplayAcknowledgement(request);
    settleScratchReplayRequest(request, "reject", error);
    if (state.scratchReplayRequests.get(request.id) === request) {
      state.scratchReplayRequests.delete(request.id);
    }
  }));
  if (!state.scratchReplayRequests.size) state.replayScratching = false;
  publishState();
}

function cancelScratchReplay() {
  return cancelScratchReplays();
}

async function beginScratch(event) {
  if (!grooveInteractionReady() || state.scratching || state.scratchReplayRequests.size) return;
  const loadSequence = state.loadSequence;
  invalidateEndTransition();
  elements.platter.setPointerCapture(event.pointerId);
  const angle = angleForPointer(event);
  state.scratching = true;
  state.scratchPointerId = event.pointerId;
  state.scratchStartAngle = angle;
  state.scratchLastAngle = angle;
  state.scratchLastTime = event.timeStamp;
  state.scratchStartPosition = state.positionFrames;
  recordScratchEvent({ type: "scratch-start", positionFrames: state.positionFrames, rate: 0, impulse: 0.22 });
  await dispatch(
    {
      type: "begin_scratch",
      deck: "a",
      pointer_id: event.pointerId,
      playback_seconds: framesToSeconds(state.positionFrames),
      rotation_degrees: state.rotation
    },
    { loadSequence },
  );
}

function moveScratch(event) {
  if (state.scratchReplayRequests.size || !state.scratching || event.pointerId !== state.scratchPointerId) return;
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
  publishState();
  state.scratchLastAngle = angle;
  state.scratchLastTime = event.timeStamp;
}

async function endScratch(event) {
  if (state.scratchReplayRequests.size || !state.scratching || event.pointerId !== state.scratchPointerId) return;
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
  const gain = Math.max(0, Math.min(4, state.packetGain * state.mixerGain));
  state.node?.port.postMessage({
    type: "output-gain",
    gain,
    rampMs: Math.max(0, Math.min(60_000, Number(rampMs) || 0)),
  });
}

function currentDeadwaxProgress() {
  const region = state.surfaceRegion;
  if (!region || region.region !== "deadwax") return 0;
  if (
    Number.isFinite(region.startOutputFrame)
    && Number.isFinite(region.durationFrames)
    && region.durationFrames > 0
  ) {
    return Math.max(0, Math.min(
      1,
      (state.lastOutputFrame - region.startOutputFrame) / region.durationFrames,
    ));
  }
  const durationMs = Math.max(1, Number(region.durationSeconds) * 1000 || 1);
  return Math.max(0, Math.min(1, (performance.now() - Number(region.startedAtMs || 0)) / durationMs));
}

function publicState() {
  const view = deckView();
  const audioPlaybackStats = readAudioPlaybackStats(state.context);
  return Object.freeze({
    ready: Boolean(view?.loaded),
    playing: Boolean(view?.playing),
    leadInActive: Boolean(state.view?.lead_in_active),
    deadwaxActive: Boolean(state.view?.deadwax_active),
    deadwaxProgress: currentDeadwaxProgress(),
    motorRunning: Boolean(view?.transport_on),
    needleLifted: Boolean(view?.needle_lifted),
    scratching: state.scratching || state.replayScratching,
    scratchReplayActive: state.scratchReplayRequests.size > 0,
    buffering: state.buffering,
    positionSeconds: framesToSeconds(state.positionFrames),
    durationSeconds: state.duration,
    bufferedSeconds: Math.min(
      state.duration,
      framesToSeconds(state.streamDecodedFrames),
    ),
    positionRatio: state.duration > 0 ? framesToSeconds(state.positionFrames) / state.duration : 0,
    rpm: state.rpm,
    nativeRpm: state.baseRpm,
    playbackRate: state.baseRpm > 0 ? state.rpm / state.baseRpm : 1,
    volume: state.volume,
    crossfader: state.crossfader,
    scratchPreset: state.scratchPreset,
    scratchClicks: state.scratchClicks,
    scratchGate: state.scratchGate,
    scratchGateTarget: state.scratchGateTarget,
    scratchDirection: state.scratchDirection,
    scratchMoving: state.scratchMoving,
    scratchGatePhase: state.scratchGatePhase,
    scratchStrokeProgress: state.scratchStrokeProgress,
    effectiveRate: state.effectiveRate,
    highFrequencyAccelerationLimit: state.highFrequencyAccelerationLimit,
    stylusTracingLimit: state.stylusTracingLimit,
    pointerToAudioLatencyMs: state.pointerToAudioLatencyMs,
    audioBaseLatencyMs: Number.isFinite(state.context?.baseLatency)
      ? state.context.baseLatency * 1000
      : null,
    audioOutputLatencyMs: Number.isFinite(state.context?.outputLatency)
      ? state.context.outputLatency * 1000
      : null,
    audioPlaybackStats,
    recordProfile: elements.metaProfile?.textContent || "",
    payloadContainer: elements.metaContainer?.textContent || "",
    releaseId: elements.metaRelease?.textContent || "",
    recordHash: state.recordHash,
    recordImageUrl: state.recordObjectUrl,
    rotationDegrees: state.rotation,
    sampleRate: state.sampleRate,
    outputSampleRate: state.context?.sampleRate || null,
    cleanEnd: state.cleanEnd,
    positionFrames: state.positionFrames,
    currentTrackIndex: currentTrackIndex(),
    currentTrackTitle: programmeTracks()[currentTrackIndex()]?.title || "",
    trackCount: programmeTracks().length,
    tapeAvailable: Boolean(state.tape.available),
    tapeActive: Boolean(state.tape.active),
  });
}

function publishState() {
  const snapshot = publicState();
  for (const listener of state.listeners) {
    try {
      listener(snapshot);
    } catch (error) {
      log.warn("state-listener-failed", {
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }
  try {
    reportBridgePlayback(snapshot);
    reportBridgeRecord(snapshot);
  } catch (error) {
    log.warn("state-bridge-report-failed", {
      message: error instanceof Error ? error.message : String(error),
    });
  }
}

async function setVolume(value) {
  await interruptScratchReplay("Volume control");
  state.volume = Math.max(0, Math.min(1, Number(value) || 0));
  if (elements.volume) elements.volume.value = String(state.volume);
  await dispatch({ type: "set_channel_gain", deck: "a", value: state.volume });
}

async function setCrossfader(value, { record = true } = {}) {
  await interruptScratchReplay("Crossfader control");
  state.crossfader = Math.max(0, Math.min(1, Number(value) || 0));
  if (elements.xfade) elements.xfade.value = String(state.crossfader);
  if (record) recordScratchEvent({ type: "manual-crossfader", value: state.crossfader });
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
  loadAudioFile,
  getCaptureStream,
  measureAcousticLoopbackLatency: measurePhysicalLoopbackLatency,
  activateAudio: async () => {
    await initialiseAudio();
    await state.context.resume();
  },
  startTransport: async () => {
    await interruptScratchReplay("Transport start");
    invalidateEndTransition();
    await initialiseAudio();
    await state.context.resume();
    const view = deckView();
    if (!view?.transport_on) await dispatch({ type: "set_transport", deck: "a", running: true });
  },
  stopTransport: async () => {
    await interruptScratchReplay("Transport stop");
    invalidateEndTransition();
    const view = deckView();
    if (view?.transport_on) await dispatch({ type: "set_transport", deck: "a", running: false });
  },
  toggleTransport: async () => {
    await interruptScratchReplay("Transport control");
    invalidateEndTransition();
    await initialiseAudio();
    await state.context.resume();
    await dispatch({ type: "toggle_transport", deck: "a" });
  },
  play: async () => {
    await interruptScratchReplay("Playback start");
    await initialiseAudio();
    await state.context.resume();
    const view = deckView();
    if (!(view?.transport_on || view?.playing || state.view?.lead_in_active || state.view?.deadwax_active)) await startLeadInPlayback();
  },
  pause: async () => {
    await interruptScratchReplay("Playback stop");
    const view = deckView();
    if (view?.transport_on || view?.playing || state.view?.lead_in_active || state.view?.deadwax_active) await stopPlaybackTransport();
  },
  reset: async () => {
    const resetSequence = ++state.loadSequence;
    state.loadInFlightSequence = 0;
    state.streamReady = false;
    state.streamReadyMarking = false;
    state.buffering = false;
    invalidateEndTransition();
    clearPendingSeekTransaction();
    clearLiveScratchInteraction();
    await cancelScratchReplays(new Error("Player reset during scratch replay"));
    if (resetSequence !== state.loadSequence) return false;
    clearPendingSeekTransaction();
    clearLiveScratchInteraction();
    await state.audioInitialisePromise?.catch(() => {});
    if (resetSequence !== state.loadSequence) return false;

    // From here through resource detachment there are no awaits, so a newer
    // load cannot be partially cleared by this reset. Once detached, a newer
    // request may safely create its own context while the old one closes.
    state.pcmStreamGeneration += 1;
    state.playbackEpoch += 1;
    state.surfaceRegionId += 1;
    clearTimeout(state.regionTimer);
    state.regionTimer = 0;
    state.surfaceRegion = null;
    state.pendingAutomaticDeadwax = null;
    state.lastOutputFrame = 0;
    state.decoder?.close();
    state.decoder = null;
    cancelPendingStreamReady(new Error("Player reset"));
    state.streamInitialised = false;
    state.streamReady = false;
    state.streamReadyMarking = false;
    state.streamDecodedFrames = 0;
    state.buffering = false;
    state.positionFrames = 0;
    state.duration = 0;
    state.metadataDuration = 0;
    state.lastReportedPosition = 0;
    state.lastDspRotationTurns = null;
    state.replayScratching = false;
    state.pointerToAudioLatencyMs = null;
    state.effectiveRate = 0;
    state.packetGain = 1;
    state.mixerGain = 1;
    state.scratchGate = 1;
    state.scratchGateTarget = 1;
    state.scratchDirection = 0;
    state.scratchMoving = false;
    state.streamAppendChain = Promise.resolve();
    state.decodeProgressText = "";
    state.basePcmSource = null;
    state.recordHash = "";
    state.recordReleaseId = "";
    state.recordDescriptorJson = "";
    state.programmeMap = null;
    state.cleanEnd = false;
    state.failedLoadSequence = 0;
    if (state.recordObjectUrl) URL.revokeObjectURL(state.recordObjectUrl);
    state.recordObjectUrl = "";
    disposePcmWindowTransport({ resetWorklet: false });
    const audioNode = state.node;
    const gainNode = state.gainNode;
    const captureNode = state.captureNode;
    const audioContext = state.context;
    audioNode?.port.postMessage({ type: "reset" });
    audioNode?.disconnect();
    gainNode?.disconnect();
    captureNode?.disconnect?.();
    state.node = null;
    state.gainNode = null;
    state.captureNode = null;
    state.context = null;
    state.audioInitialisePromise = null;
    await audioContext?.close().catch?.(() => {});
    if (resetSequence !== state.loadSequence) return false;
    if (state.worker) {
      await dispatch({
        type: "set_load_state",
        deck: "a",
        status: "empty",
        loaded: false,
        duration_seconds: 0,
      }).catch(() => {});
      if (resetSequence !== state.loadSequence) return false;
    }
    const canvas = playerRoot.querySelector("#player-canvas");
    const context = canvas?.getContext("2d");
    if (context) context.clearRect(0, 0, canvas.width, canvas.height);
    publishState();
    return true;
  },
  togglePlayback: async () => {
    await interruptScratchReplay("Playback control");
    await initialiseAudio();
    await state.context.resume();
    await toggleStartStopPlayback();
  },
  stepTrack,
  seekSeconds: seconds => queueSeek(Math.max(0, Math.min(state.duration, Number(seconds) || 0))),
  seekRatio: ratio => queueSeek(Math.max(0, Math.min(1, Number(ratio) || 0)) * state.duration),
  setRpm,
  setVolume,
  setCrossfader,
  setScratchPreset,
  setScratchClicks,
  setHighFrequencyAccelerationLimit,
  setStylusTracingLimit,
  setNeedleLifted: async lifted => {
    await interruptScratchReplay("Needle control");
    invalidateEndTransition();
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
  beginScratch: ({ pointerId = 0, rotationDegrees = state.rotation, positionFrames = state.positionFrames, rate = 0, impulse = 0.22, inputTimeMs } = {}) => {
    if (!grooveInteractionReady() || state.scratching || state.scratchReplayRequests.size) {
      return Promise.resolve(false);
    }
    const loadSequence = state.loadSequence;
    invalidateEndTransition();
    const position = Number(positionFrames) || 0;
    // The hand owns the record from the first touch: publish the scratching
    // state immediately so the canvas stops advancing the motor's visual
    // rotation — otherwise the drawn record fights the hand.
    state.scratching = true;
    if (Number.isFinite(Number(rotationDegrees))) state.rotation = Number(rotationDegrees);
    publishState();
    recordScratchEvent({ type: "scratch-start", positionFrames: position, rate: Number(rate) || 0, impulse: Number(impulse) || 0 });
    state.node?.port.postMessage({
      type: "scratch",
      active: true,
      position,
      rate: Number(rate) || 0,
      impulse: Number(impulse) || 0,
      ...pointerAudioTiming(inputTimeMs),
    });
    return dispatch(
      { type: "begin_scratch", deck: "a", pointer_id: pointerId, playback_seconds: framesToSeconds(position), rotation_degrees: rotationDegrees },
      { loadSequence },
    );
  },
  updateScratch: ({ positionFrames, rate = 0, rotationDegrees = state.rotation, impulse = 0, inputTimeMs } = {}) => {
    if (!grooveInteractionReady() || !state.scratching || state.scratchReplayRequests.size) {
      return Promise.resolve(false);
    }
    const position = Number(positionFrames) || 0;
    const nextRate = Number(rate) || 0;
    const nextImpulse = Number(impulse) || 0;
    state.scratching = true;
    if (Number.isFinite(Number(rotationDegrees))) state.rotation = Number(rotationDegrees);
    state.positionFrames = position;
    recordScratchEvent({ type: "scratch-motion", positionFrames: position, rate: nextRate, impulse: nextImpulse });
    state.node?.port.postMessage({
      type: "motion",
      position,
      rate: nextRate,
      impulse: nextImpulse,
      ...pointerAudioTiming(inputTimeMs),
    });
    publishState();
    return Promise.resolve();
  },
  endScratch: ({ rotationDegrees = state.rotation, resumePlayback = true } = {}) => {
    if (!state.scratching || state.scratchReplayRequests.size) return Promise.resolve(false);
    state.scratching = false;
    if (Number.isFinite(Number(rotationDegrees))) state.rotation = Number(rotationDegrees);
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
    get: id => getScratchPerformance(id, {
      sourceSampleRate: state.sampleRate,
      outputSampleRate: state.context?.sampleRate || state.sampleRate,
    }),
    list: query => listScratchPerformances({
      recordHash: state.recordHash,
      ...(query || {}),
      target: {
        sourceSampleRate: state.sampleRate,
        outputSampleRate: state.context?.sampleRate || state.sampleRate,
        ...(query?.target || {}),
      },
    }),
    delete: deleteScratchPerformance,
    clear: query => clearScratchPerformances({ recordHash: state.recordHash, ...(query || {}) }),
    export: performance => JSON.stringify(performance),
    import: value => normalizeScratchPerformance(
      typeof value === "string" ? JSON.parse(value) : structuredClone(value),
      {
        sourceSampleRate: state.sampleRate,
        outputSampleRate: state.context?.sampleRate || state.sampleRate,
      },
    )
  }),
  getState: publicState,
  subscribe(listener) {
    state.listeners.add(listener);
    try {
      listener(publicState());
    } catch (error) {
      log.warn("state-listener-initial-notification-failed", {
        message: error instanceof Error ? error.message : String(error),
      });
    }
    return () => state.listeners.delete(listener);
  },
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
  await ensureCoreWorker();
  elements.play.disabled = false;
  elements.needle.disabled = false;
  render();
  const canvas = playerRoot.querySelector("#player-canvas");
  if (canvas) {
    state.canvasController = createVinylPlayerCanvas(api, canvas);
    const embedOptions = embedCanvasOptions();
    if (embedOptions) state.canvasController.configure(embedOptions);
    if (HOST_CONFIG.canvasOptions) state.canvasController.configure(HOST_CONFIG.canvasOptions);
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
  if (HOST_CONFIG.postMessageBridge) {
    api.configurePostMessageBridge(HOST_CONFIG.postMessageBridge);
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
  await api.togglePlayback();
});
elements.needle.addEventListener("click", () => {
  const view = deckView();
  if (!view) return;
  void api.setNeedleLifted(!view.needle_lifted).catch(error => setStatus(error?.message || String(error)));
});
elements.tape?.addEventListener("click", () => {
  void setTapeMonitor(!state.tape.active).catch((error) => {
    setStatus(error?.message || String(error));
    updateTapeButton();
  });
});
elements.seek.addEventListener("pointerdown", () => { state.draggingSeek = true; });
elements.seek.addEventListener("input", () => {
  const seconds = Number(elements.seek.value) * state.duration;
  queueSeek(seconds);
});
elements.seek.addEventListener("change", () => {
  state.draggingSeek = false;
  if (!seekInteractionReady()) {
    clearPendingSeekTransaction();
    return;
  }
  const seconds = Number(elements.seek.value) * state.duration;
  state.queuedSeekSeconds = seconds;
  clearTimeout(state.seekTimer);
  state.seekTimer = 0;
  void flushQueuedSeek();
});
elements.seek.addEventListener("pointerup", () => { state.draggingSeek = false; });
elements.rpm33?.addEventListener("click", () => void setRpm(33.3333333333));
elements.rpm45?.addEventListener("click", () => void setRpm(45));
elements.rpm?.addEventListener("input", () => void setRpm(elements.rpm.value));
elements.volume?.addEventListener("input", () => void setVolume(elements.volume.value));
elements.xfade?.addEventListener("input", () => void setCrossfader(elements.xfade.value));
elements.scratchPreset?.addEventListener("change", () => setScratchPreset(elements.scratchPreset.value));
elements.scratchClicks?.addEventListener("input", () => setScratchClicks(elements.scratchClicks.value));
elements.highFrequencyAccelerationLimit?.addEventListener("input", () => {
  setHighFrequencyAccelerationLimit(elements.highFrequencyAccelerationLimit.value);
});
elements.stylusTracingLimit?.addEventListener("input", () => {
  setStylusTracingLimit(elements.stylusTracingLimit.value);
});
elements.platter.addEventListener("pointerdown", event => void beginScratch(event));
elements.platter.addEventListener("pointermove", moveScratch);
elements.platter.addEventListener("pointerup", event => void endScratch(event));
elements.platter.addEventListener("pointercancel", event => void endScratch(event));

updateTapeButton();
updateScratchTechniqueControls();
if (elements.highFrequencyAccelerationLimit) {
  elements.highFrequencyAccelerationLimit.value = String(state.highFrequencyAccelerationLimit);
}
if (elements.highFrequencyAccelerationLimitValue) {
  elements.highFrequencyAccelerationLimitValue.value = `${Math.round(state.highFrequencyAccelerationLimit * 100)}%`;
}
if (elements.stylusTracingLimit) elements.stylusTracingLimit.value = String(state.stylusTracingLimit);
if (elements.stylusTracingLimitValue) {
  elements.stylusTracingLimitValue.value = `${Math.round(state.stylusTracingLimit * 100)}%`;
}
initialise().catch(error => setStatus(error.message));
