import { createLogger, setPlayerLoggingEnabled, isPlayerLoggingEnabled, isPlayerVerboseLoggingEnabled, setPlayerLogLevel, getPlayerLogLevel, setPlayerTelemetryInterval } from "./player-message-logger.js";
import { createVinylPlayerCanvas } from "./player-canvas.js";
import { RecordDecoderClient } from "./record-decoder-client.js";
import { createPcmChunkCacheHandler, recordCacheKey } from "./pcm-cache.js";
import { createRemoteOpusChunkCacheHandler, createRemoteOpusPrecache, decodeRecordDescriptorJson } from "./opus-cache.js";
import { buildSoundkitFrameHeader, soundkitOpusPacketItemsFromPackets } from "./player-soundkit.js";
import { clearScratchPerformances, deleteScratchPerformance, getScratchPerformance, listScratchPerformances, saveScratchPerformance } from "./scratch-performance-store.js";

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
// Original profile_turns(): lead-in and deadwax both traverse 2 revolutions.
const LEAD_IN_TURNS = 2;
const DEADWAX_TURNS = 2;

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
  xfade: playerRoot.querySelector("#xfade")
};

const state = {
  context: null,
  node: null,
  worker: null,
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
  recordObjectUrl: "",
  streamInitialised: false,
  streamReady: false,
  streamDecodedFrames: 0,
  buffering: false,
  streamReadyPromise: null,
  streamReadyResolve: null,
  streamReadyReject: null,
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
  recordReleaseId: "",
  basePcmSource: null,
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
  surfaceRegion: null,
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

async function initialiseProgressiveStream({ sampleRate, audioLength, channels, workletSeamRepair = true }) {
  if (state.streamInitialised) return;
  const channelCount = Math.max(1, Math.min(2, Number(channels) || 2));
  state.sampleRate = Math.max(1, Number(sampleRate) || 48000);
  state.duration = state.metadataDuration > 0
    ? state.metadataDuration
    : Math.max(1, Number(audioLength) || 1) / state.sampleRate;
  state.positionFrames = 0;
  state.lastReportedPosition = 0;
  state.streamDecodedFrames = 0;
  if (isPlayerVerboseLoggingEnabled()) log.send("worklet:stream-init", { sampleRate: state.sampleRate, audioLength: Math.max(1, Number(audioLength) || 1), channels: channelCount, workletSeamRepair });
  state.node.port.postMessage({
    type: "stream-init",
    sampleRate: state.sampleRate,
    audioLength: Math.max(1, Number(audioLength) || 1),
    channels: channelCount,
    workletSeamRepair
  });
  state.streamInitialised = true;
}

async function appendProgressiveSegments(segments) {
  if (!Array.isArray(segments) || !segments.length) return;
  const first = segments[0] || {};
  await initialiseProgressiveStream({
    sampleRate: first.sampleRate,
    audioLength: first.audioLength,
    channels: first.channels,
    workletSeamRepair: first.workletSeamRepair !== false
  });
  for (const segment of segments) {
    const channelBuffers = Array.isArray(segment.channelBuffers) ? segment.channelBuffers : [];
    if (!channelBuffers.length) continue;
    const startFrame = Math.max(0, Math.floor(Number(segment.startFrame ?? segment.offset ?? 0) || 0));
    const inferredFrames = new Int16Array(channelBuffers[0]).length;
    const endFrame = Math.max(startFrame, Math.floor(Number(segment.endFrame) || (startFrame + inferredFrames)));
    if (isPlayerVerboseLoggingEnabled()) log.send("worklet:append-pcm", { startFrame, endFrame, channels: channelBuffers.length, bytes: channelBuffers.reduce((sum, buffer) => sum + (buffer?.byteLength || 0), 0) });
    state.node.port.postMessage({
      type: "append-pcm",
      startFrame,
      endFrame,
      channelBuffers
    }, channelBuffers);
  }
}

// Keep a few seconds ahead of the playhead before handing control back to the
// caller. The remaining ECDC segments continue decoding in the background.
const PROGRESSIVE_READY_SECONDS = 3;

async function handleWorkletBuffered(message) {
  state.streamDecodedFrames = Math.max(0, Math.floor(Number(message.decodedLength) || 0));
  publishState();
  if (state.streamReady) return;
  const totalFrames = Math.max(1, Math.round(state.duration * state.sampleRate));
  const thresholdFrames = Math.max(1024, Math.round(state.sampleRate * PROGRESSIVE_READY_SECONDS));
  const contiguousReady = state.streamDecodedFrames >= Math.min(totalFrames, thresholdFrames);
  if (contiguousReady) {
    state.streamReady = true;
    await markLoadedReady();
    renderDecodeStatus();
    state.streamReadyResolve?.();
    state.streamReadyResolve = null;
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
  const releaseId = String(state.recordReleaseId || "").trim();
  if (!releaseId) return null;
  if (state.tape.source && state.tape.releaseId === releaseId) return state.tape.source;
  const stored = (await readTapeMasterStream(releaseId)) || (await fetchTapeMasterRemoteStream(releaseId));
  if (!stored) return null;
  const source = await decodeTapeMasterSource(stored, releaseId);
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

async function replaceActivePcmSource(source) {
  if (!state.node || !source) return;
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
    state.node.port.postMessage({ type: "stop", handoff: false });
  }
  const channelBuffers = cloneChannelBuffers(source.channelBuffers);
  state.node.port.postMessage({
    type: "stream-init",
    sampleRate: source.sampleRate,
    audioLength: source.audioLength,
    channels: source.channels,
  });
  state.node.port.postMessage({
    type: "append-pcm",
    startFrame: 0,
    endFrame: source.audioLength,
    channelBuffers,
  }, channelBuffers);
  state.node.port.postMessage({ type: "stream-complete" });
  seekWorklet(position);
  state.node.port.postMessage({ type: "transport", running: motorRunning });
  state.node.port.postMessage({ type: "needle", lifted: needleLifted });
  if (wasPlaying) {
    state.node.port.postMessage({
      type: "play",
      position,
      rate: state.baseRpm > 0 ? state.rpm / state.baseRpm : 1,
      handoff: false,
    });
  }
  state.sampleRate = source.sampleRate;
  state.duration = source.sampleRate > 0 ? source.audioLength / source.sampleRate : state.duration;
  state.streamInitialised = true;
  state.streamReady = true;
  state.streamDecodedFrames = source.audioLength;
}

async function setTapeMonitor(active) {
  const next = Boolean(active);
  if (next === state.tape.active) return;
  if (!state.basePcmSource) {
    setStatus("TAPE monitor is unavailable until the record finishes decoding.");
    return;
  }
  state.tape.loading = true;
  updateTapeButton();
  try {
    if (next) {
      setStatus("TAPE: loading HQ Opus master...");
      const source = await ensureTapeMasterSource();
      if (!source) {
        state.tape.available = false;
        setStatus("HQ Opus tape master is unavailable for this record.");
        return;
      }
      await replaceActivePcmSource(source);
      state.tape.active = true;
      state.tape.available = true;
      state.tape.sourceLabel = "hq-opus";
      setStatus("TAPE: HQ Opus");
    } else {
      await replaceActivePcmSource(state.basePcmSource);
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
    const isPositionTelemetry = payload?.event?.type === "playback_position_observed";
    state.pending.set(id, { resolve, reject, type, startedAt: performance.now(), telemetry: isPositionTelemetry });
    if (isPlayerVerboseLoggingEnabled()) log.send(`core:${type}`, { id, payload }, isPositionTelemetry ? { telemetry: true } : undefined);
    state.worker.postMessage({ id, type, payload });
  });
}

async function dispatch(event) {
  const isPositionTelemetry = event?.type === "playback_position_observed";
  if (isPlayerVerboseLoggingEnabled()) log.action(`dispatch:${event?.type || "unknown"}`, event, isPositionTelemetry ? { telemetry: true } : undefined);
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

function timedRegionDurationSeconds(turns) {
  const rpm = Number(state.rpm) || Number(state.baseRpm) || 45;
  return rpm > 0 ? Math.max(0, Number(turns) || 0) * (60 / rpm) : 0;
}

async function startLeadInPlayback() {
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
  await dispatch({ type: "stop_timed_region", region: "lead_in", completed: false }).catch(() => {});
  await dispatch({ type: "stop_timed_region", region: "deadwax", completed: false }).catch(() => {});
  await dispatch({ type: "set_transport", deck: "a", running: false });
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
    state.surfaceRegion = { region: command.region, startedAtMs: performance.now(), durationSeconds };
    state.node.port.postMessage({ type: "surface-region", action: "start", region: command.region, durationSeconds });
    publishState();
    clearTimeout(state.regionTimer);
    if (command.region === "lead_in") {
      state.regionTimer = setTimeout(() => {
        void dispatch({ type: "timed_region_elapsed", region: command.region });
      }, durationSeconds * 1000);
    } else {
      state.regionTimer = 0;
    }
  } else if (command.type === "stop_surface_region") {
    clearTimeout(state.regionTimer);
    if (state.surfaceRegion?.region === command.region) state.surfaceRegion = null;
    state.node.port.postMessage({ type: "surface-region", action: "stop", region: command.region });
    publishState();
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
  if (message.type === "position") {
    if (state.pendingSeekGeneration !== state.acknowledgedSeekGeneration) return;
    const previousPosition = state.lastReportedPosition;
    state.positionFrames = message.position;
    state.buffering = false;
    state.lastReportedPosition = message.position;
    if (!state.draggingSeek) elements.seek.value = String(state.duration > 0 ? framesToSeconds(message.position) / state.duration : 0);
    if (!message.scratching && Number.isFinite(previousPosition)) {
      const framesPerTurn = state.sampleRate * 60 / state.baseRpm;
      state.rotation = (state.rotation + ((message.position - previousPosition) / framesPerTurn) * 360) % 360;
      elements.platter.style.setProperty("--rotation", `${state.rotation}deg`);
    }
    publishState();
  } else if (message.type === "seeked") {
    state.acknowledgedSeekGeneration = Math.max(state.acknowledgedSeekGeneration, message.generation ?? 0);
    state.positionFrames = message.position;
  } else if (message.type === "buffering") {
    state.positionFrames = Math.max(0, Number(message.position) || state.positionFrames);
    state.lastReportedPosition = state.positionFrames;
    state.buffering = true;
    setStatus("Buffering decoded groove audio…");
    publishState();
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
      const durationSeconds = timedRegionDurationSeconds(DEADWAX_TURNS);
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
  const loadSequence = state.loadSequence + 1;
  state.loadSequence = loadSequence;
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
  if (resumeAudio) {
    await state.context.resume();
  }
  if (cache !== undefined) state.cacheHandler = normalizeCacheHandler(cache);
  if (state.decoder) {
    state.decoder.close();
  }
  state.decoder = new RecordDecoderClient(versionedAssetUrl("./record-decoder-worker.js"), {
    loggingEnabled: isPlayerLoggingEnabled(),
    cache: state.cacheHandler,
  });
  state.decoder.setCache(state.cacheHandler);
  await state.decoder.initialise();
  state.streamInitialised = false;
  state.streamReady = false;
  state.streamDecodedFrames = 0;
  state.buffering = false;
  state.duration = 0;
  state.metadataDuration = 0;
  state.streamAppendChain = Promise.resolve();
  resetStreamReadyPromise();
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
  if (isPlayerLoggingEnabled()) {
    console.log("[vin.yl.player] loadFile:bytes", {
      name: file?.name || "",
      bytes: sourceBytes.byteLength || 0,
    });
  }
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
  setStatus(`Decoding ${file.name}…`);
  let lastLoggedDecodeChunk = -1;
  const decodePromise = state.decoder.decode(sourceBytes, inspected.recordProfile || "", {
      recordBindingHex: state.recordHash,
    }, progress => {
      if (loadSequence !== state.loadSequence) return;
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
        state.streamAppendChain = state.streamAppendChain
          .then(() => appendProgressiveSegments(segments))
          .catch(error => {
            setStatus(`Progressive playback failed: ${error.message || error}`);
          });
      }
    });
  decodePromise.then(async decoded => {
    if (loadSequence !== state.loadSequence) return;
    await state.streamAppendChain;
    if (loadSequence !== state.loadSequence) return;

    const sampleRate = Math.max(1, Number(decoded.sampleRate) || 48000);
    const audioLength = Math.max(1, Number(decoded.audioLength) || 0);
    const s16Buffers = Array.isArray(decoded.s16ChannelBuffers) ? decoded.s16ChannelBuffers : [];
    if (!s16Buffers.length) throw new Error("Record decoder returned no PCM channels");
    state.baseRpm = profileRpm(inspected.recordProfile);
    state.rpm = state.baseRpm;
    updateRpmButtons();
    storeBasePcmSource({ sampleRate, audioLength, s16ChannelBuffers: s16Buffers });
    updateTapeButton();
    if (!state.streamInitialised) {
      await loadDecodedPcm({ sampleRate, audioLength, s16ChannelBuffers: s16Buffers });
    }
    state.node.port.postMessage({ type: "stream-complete" });
    if (!state.streamReady) {
      await markLoadedReady();
      state.streamReady = true;
      state.streamReadyResolve?.();
      state.streamReadyResolve = null;
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
    setStatus(`Playback decode failed: ${error.message || error}`);
  });

  // Start as soon as the worklet has a small contiguous lead-in; the rest
  // continues decoding and appending in the background.
  await state.streamReadyPromise;
  if (loadSequence !== state.loadSequence) return null;
}

async function loadRecordFromUrl(url, options = {}) {
  const resolved = new URL(String(url || ""), globalThis.location?.href || import.meta.url);
  if (isPlayerLoggingEnabled()) {
    console.log("[vin.yl.player] loadRecordFromUrl:start", {
      input: String(url || ""),
      resolved: resolved.toString(),
      options,
    });
  }
  const response = await fetch(resolved.toString(), { cache: "force-cache" });
  if (!response.ok) {
    console.error("[vin.yl.player] loadRecordFromUrl:fetch-failed", {
      resolved: resolved.toString(),
      status: response.status,
    });
    throw new Error(`Failed to load record from ${resolved}: ${response.status}`);
  }
  const blob = await response.blob();
  if (isPlayerLoggingEnabled()) {
    console.log("[vin.yl.player] loadRecordFromUrl:fetched", {
      resolved: resolved.toString(),
      size: blob.size || 0,
      type: blob.type || "",
    });
  }
  const pathname = resolved.pathname.split("/").pop() || "record.png";
  const file = new File([blob], pathname, { type: blob.type || "image/png" });
  return loadFile(file, { resumeAudio: false, ...(options || {}) });
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
  publishState();
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

function currentDeadwaxProgress() {
  const region = state.surfaceRegion;
  if (!region || region.region !== "deadwax") return 0;
  const durationMs = Math.max(1, Number(region.durationSeconds) * 1000 || 1);
  return Math.max(0, Math.min(1, (performance.now() - Number(region.startedAtMs || 0)) / durationMs));
}

function publicState() {
  const view = deckView();
  return Object.freeze({
    ready: Boolean(view?.loaded),
    playing: Boolean(view?.playing),
    leadInActive: Boolean(state.view?.lead_in_active),
    deadwaxActive: Boolean(state.view?.deadwax_active),
    deadwaxProgress: currentDeadwaxProgress(),
    motorRunning: Boolean(view?.transport_on),
    needleLifted: Boolean(view?.needle_lifted),
    scratching: state.scratching,
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
    tapeAvailable: Boolean(state.tape.available),
    tapeActive: Boolean(state.tape.active),
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
  activateAudio: async () => {
    await initialiseAudio();
    await state.context.resume();
  },
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
    if (!(view?.transport_on || view?.playing || state.view?.lead_in_active || state.view?.deadwax_active)) await startLeadInPlayback();
  },
  pause: async () => {
    const view = deckView();
    if (view?.transport_on || view?.playing || state.view?.lead_in_active || state.view?.deadwax_active) await stopPlaybackTransport();
  },
  reset: async () => {
    state.loadSequence += 1;
    clearTimeout(state.regionTimer);
    state.regionTimer = 0;
    state.surfaceRegion = null;
    if (state.node) await stopPlaybackTransport().catch(() => {});
    state.decoder?.close();
    state.decoder = null;
    state.streamReadyReject?.(new Error("Player reset"));
    state.streamReadyResolve = null;
    state.streamReadyReject = null;
    state.streamReadyPromise = null;
    state.streamInitialised = false;
    state.streamReady = false;
    state.streamDecodedFrames = 0;
    state.streamAppendChain = Promise.resolve();
    state.decodeProgressText = "";
    state.basePcmSource = null;
    state.node?.port.postMessage({ type: "reset" });
    state.node?.disconnect();
    state.gainNode?.disconnect();
    await state.context?.close().catch?.(() => {});
    state.node = null;
    state.gainNode = null;
    state.context = null;
    const canvas = playerRoot.querySelector("#player-canvas");
    const context = canvas?.getContext("2d");
    if (context) context.clearRect(0, 0, canvas.width, canvas.height);
    publishState();
  },
  togglePlayback: async () => {
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
    publishState();
    return Promise.resolve();
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
  state.worker = new Worker(versionedAssetUrl("./player-core-worker.js"), { type: "module" });
  log.action("core-worker-created", {});
  state.worker.onmessage = event => {
    const request = state.pending.get(event.data?.id);
    log.receive(
      `core:${event.data?.type || (event.data?.ok ? "response" : "error")}`,
      event.data,
      request?.telemetry ? { telemetry: true } : undefined,
    );
    const { id, ok, result, error } = event.data;
    if (!request) { if (id !== 0) log.warn("core-unmatched-response", event.data); return; }
    state.pending.delete(id);
    if (ok) request.resolve(result);
    else request.reject(new Error(error));
  };
  const result = await coreRequest("init", { moduleUrl: versionedAssetUrl("./record-player/record_player.js") });
  state.view = result.view;
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
  await initialiseAudio();
  await state.context.resume();
  await toggleStartStopPlayback();
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

updateTapeButton();
initialise().catch(error => setStatus(error.message));
