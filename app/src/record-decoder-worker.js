if (typeof importScripts === "function") {
  if (!globalThis.VinylPlayerMessageLogger) importScripts("./player-message-logger-global.js");
  if (!globalThis.BitneedleBrowserFormatting) {
    importScripts("./browser-formatting.js");
  }
  if (!globalThis.BitneedleEncodecBundleNames) {
    importScripts("./encodec-bundle-names.js");
  }
  if (!globalThis.BitneedleOnnxRuntimeSession) {
    importScripts("./onnx-runtime-session.js");
  }
  if (!globalThis.BitneedleOnnxWorkerTensors) {
    importScripts("./onnx-worker-tensors.js");
  }
  if (!globalThis.BitneedleEcdcPcmLayout) {
    importScripts("./ecdc-pcm-layout.js");
  }
}


const playerMessageLog = globalThis.VinylPlayerMessageLogger.createLogger("decoder-worker");
globalThis.VinylPlayerMessageLogger.setEnabled(new URL(self.location.href).searchParams.get("player_log") !== "0");
const rawPostMessage = self.postMessage.bind(self);
self.postMessage = (message, transfer) => { playerMessageLog.send(message?.progress ? "progress" : message?.ok ? "response" : message?.type || "message", message, { transferCount: Array.isArray(transfer) ? transfer.length : 0 }); return transfer === undefined ? rawPostMessage(message) : rawPostMessage(message, transfer); };
const WORKER_GENERATION_CACHE_VERSION =
  new URL(self.location.href).searchParams.get("v") || "dev";
const WORKER_PERF_LOG_ENABLED =
  new URL(self.location.href).searchParams.get("perf") === "1";
const WORKER_DECODE_DEBUG_ENABLED =
  new URL(self.location.href).searchParams.get("decode_debug") === "1";
// MOSS Nano decode is disabled unless the app config opts in (see
// PLAYER_MOSSNANO_DECODE_ENABLED in player-playback-config.js); its ONNX
// weights are ~42MB and current records do not use it.
const MOSS_NANO_DECODE_ENABLED =
  new URL(self.location.href).searchParams.get("mossnano") === "1";

const ONNX_RUNTIME_BASE_URL = "wasm/onnxruntime-web/";
const ENCODEC_BUNDLE_BASE_URL = "wasm/encodec-rs/bundles";
const MOSSNANO_WASM_BASE_URL = "wasm/mossnano-rs";
const MOSS_NANO_CODEC = "moss-audio-tokenizer-nano-rvq16";
const MOSS_NANO_MODEL_ROOT = `${MOSSNANO_WASM_BASE_URL}/weights/MOSS-Audio-Tokenizer-Nano-ONNX`;
const MOSS_NANO_RECORD_PROFILE_CHUNK_SECONDS = Object.freeze({
  single45: 1.333,
  lp: 1.8,
});

let onnxRuntimeModulePromise = null;
let playerWasmModulePromise = null;
let playerAppWasmModule = null;
let mossNanoWasmModulePromise = null;
let mossNanoDecodeSessionPromise = null;
let cacheRequestId = 0;
const pendingCacheRequests = new Map();

function versionedWorkerAssetUrl(path) {
  const url = new URL(path, self.location.href);
  url.searchParams.set("v", WORKER_GENERATION_CACHE_VERSION);
  return url.toString();
}

function versionedWorkerManifestPartUrl(part, manifestUrl) {
  const url = new URL(part, manifestUrl);
  url.searchParams.set("v", WORKER_GENERATION_CACHE_VERSION);
  return url.toString();
}

const {
  formatDecimal,
  workerErrorMessage,
} = globalThis.BitneedleBrowserFormatting;
const {
  expectedFramePcmSamples,
  floatToS16Sample,
  frameStartSample,
  triangleWeight,
} = globalThis.BitneedleEcdcPcmLayout || {};
if (
  typeof expectedFramePcmSamples !== "function" ||
  typeof floatToS16Sample !== "function" ||
  typeof frameStartSample !== "function" ||
  typeof triangleWeight !== "function"
) {
  throw new Error("Bitneedle ECDC PCM layout helpers were not loaded before client-playback-worker.js.");
}

function postDecodeProgress(id, payload, transfer = []) {
  self.postMessage({
    id,
    progress: {
      status: "decoding",
      ...payload,
    },
  }, transfer);
}

function workerPerfStart(label, details = {}) {
  if (!WORKER_PERF_LOG_ENABLED) {
    return null;
  }
  return { label, details, startedAt: performance.now() };
}

function workerPerfEnd(id, mark, details = {}) {
  if (!mark) {
    return;
  }
  self.postMessage({
    id,
    perf: {
      label: mark.label,
      ...mark.details,
      ...details,
      ms: Math.round((performance.now() - mark.startedAt) * 10) / 10,
    },
  });
}

function createWorkerEcdcCacheProofContext(ecdc) {
  const context = JSON.parse(
    requirePlayerAppWasmFunction("createPlayerEcdcCacheProofContextJson")(
      ecdc instanceof Uint8Array ? ecdc : new Uint8Array(ecdc || 0),
    ),
  );
  return context && typeof context === "object" ? context : null;
}

// Bound how long a host-side cache read/write can take. A network-backed
// encrypted cache read that never resolves must not hang record decoding
// forever; a timed-out read degrades to the ordinary cache-miss path.
const CACHE_REQUEST_TIMEOUT_MS = 8000;

function requestCache(type, parentRequestId, payload = {}, transfer = []) {
  return new Promise((resolve, reject) => {
    const requestId = ++cacheRequestId;
    let settled = false;
    const timeoutId = setTimeout(() => {
      if (settled) {
        return;
      }
      settled = true;
      pendingCacheRequests.delete(requestId);
      reject(new Error(`Cache request "${type}" timed out after ${CACHE_REQUEST_TIMEOUT_MS}ms`));
    }, CACHE_REQUEST_TIMEOUT_MS);
    pendingCacheRequests.set(requestId, {
      type,
      resolve: (value) => {
        if (settled) {
          return;
        }
        settled = true;
        clearTimeout(timeoutId);
        resolve(value);
      },
      reject: (error) => {
        if (settled) {
          return;
        }
        settled = true;
        clearTimeout(timeoutId);
        reject(error);
      },
    });
    self.postMessage({
      id: parentRequestId,
      type,
      cacheRequestId: requestId,
      ...payload,
    }, transfer);
  });
}

async function requestCacheGet(parentRequestId, key, meta) {
  return requestCache("cache-get", parentRequestId, { key, meta });
}

async function requestCacheGetMany(parentRequestId, entries) {
  return requestCache("cache-get-many", parentRequestId, { entries });
}

async function requestCachePut(parentRequestId, key, pcm) {
  const channelBuffers = Array.isArray(pcm?.channelData)
    ? pcm.channelData.map(channel => {
      const buffer = channel.buffer.slice(channel.byteOffset, channel.byteOffset + channel.byteLength);
      return buffer;
    })
    : [];
  return requestCache("cache-put", parentRequestId, {
    key,
    pcm: {
      chunkIndex: pcm?.chunkIndex,
      startFrame: pcm?.startFrame,
      endFrame: pcm?.endFrame,
      channels: channelBuffers.length,
      sampleRate: pcm?.sampleRate,
      channelBuffers,
    },
  }, channelBuffers);
}

function handleCacheResponseMessage(message) {
  const request = pendingCacheRequests.get(message.cacheRequestId);
  if (!request) {
    return false;
  }
  pendingCacheRequests.delete(message.cacheRequestId);
  if (message.ok) {
    request.resolve(message.result ?? null);
  } else {
    request.reject(new Error(message.error || `${request.type} failed`));
  }
  return true;
}

function cachedChannelBuffersToRawSegment(segment, chunkIndex, expectedChannels = 0) {
  const channelBuffers = Array.isArray(segment?.channelBuffers) ? segment.channelBuffers : [];
  if (!channelBuffers.length) {
    return null;
  }
  if (expectedChannels > 0 && channelBuffers.length !== expectedChannels) {
    return null;
  }
  if (!channelBuffers.every((buffer) => (buffer?.byteLength || 0) % Int16Array.BYTES_PER_ELEMENT === 0)) {
    return null;
  }
  const frameCount = new Int16Array(channelBuffers[0]).length;
  if (!channelBuffers.every((buffer) => new Int16Array(buffer).length === frameCount)) {
    return null;
  }
  const startFrame = Math.max(0, Math.floor(Number(segment.startFrame) || 0));
  const endFrame = Math.floor(Number(segment.endFrame));
  if (!Number.isFinite(endFrame) || endFrame - startFrame !== frameCount) {
    return null;
  }
  return {
    chunkIndex,
    startFrame,
    endFrame,
    channelData: channelBuffers.map(buffer => new Int16Array(buffer)),
  };
}

async function tryRequestCacheGet(parentRequestId, key, meta) {
  try {
    return await requestCacheGet(parentRequestId, key, meta);
  } catch (error) {
    workerDecodeWarn("[play:client-playback-worker.js] cache get failed", {
      key,
      chunkIndex: meta?.chunkIndex,
      error: workerErrorMessage(error),
    });
    return null;
  }
}

async function tryRequestCacheGetMany(parentRequestId, entries) {
  try {
    const result = await requestCacheGetMany(parentRequestId, entries);
    return Array.isArray(result) ? result : [];
  } catch (error) {
    workerDecodeWarn("[play:client-playback-worker.js] cache get many failed", {
      entryCount: Array.isArray(entries) ? entries.length : 0,
      error: workerErrorMessage(error),
    });
    return [];
  }
}

async function tryRequestCachePut(parentRequestId, key, pcm) {
  if (!key) {
    return;
  }
  try {
    await requestCachePut(parentRequestId, key, pcm);
  } catch (error) {
    workerDecodeWarn("[play:client-playback-worker.js] cache put failed", {
      key,
      chunkIndex: pcm?.chunkIndex,
      error: workerErrorMessage(error),
    });
  }
}

function tryEcdcChunkCacheKey(chunkPayload, chunkIndex, recordBindingHex) {
  try {
    return requirePlayerAppWasmFunction("ecdcChunkCacheKey")(chunkPayload, recordBindingHex || "");
  } catch (error) {
    workerDecodeWarn("[play:client-playback-worker.js] cache key skipped", {
      chunkIndex,
      error: workerErrorMessage(error),
    });
    return "";
  }
}

function reportSkippedDecodePacket(id, label, error) {
  const message = `${label}: ${workerErrorMessage(error)}`;
  console.error(`[bitneedle-player] ${message}`, error);
  postDecodeProgress(id, {
    status: "warning",
    msg: message,
    warning: message,
    error: workerErrorMessage(error),
  });
}

function workerDecodeDebug(...args) {
  if (!WORKER_DECODE_DEBUG_ENABLED) {
    return;
  }
  console.log(...args);
}

function workerDecodeInfo(...args) {
  if (!WORKER_DECODE_DEBUG_ENABLED) {
    return;
  }
  console.info(...args);
}

function workerDecodeWarn(...args) {
  if (!WORKER_DECODE_DEBUG_ENABLED) {
    return;
  }
  console.warn(...args);
}

async function ensureOnnxRuntimeModule() {
  if (!onnxRuntimeModulePromise) {
    onnxRuntimeModulePromise = (async () => {
      const runtimeBaseUrl = ONNX_RUNTIME_BASE_URL.replace(/\/?$/, "/");
      const ort = await import(versionedWorkerAssetUrl(`${runtimeBaseUrl}ort.wasm.min.mjs`));
      ort.env.wasm.wasmPaths = new URL(runtimeBaseUrl, self.location.href).href;
      // Single-threaded everywhere: multi-threaded ORT requires a shared
      // WebAssembly.Memory whose full maximum is reserved up front, which
      // fails with RangeError out-of-memory on memory-constrained devices.
      ort.env.wasm.numThreads = 1;
      return ort;
    })();
  }
  return onnxRuntimeModulePromise;
}

async function ensurePlayerWasmModule() {
  if (!playerWasmModulePromise) {
    playerWasmModulePromise = (async () => {
      const module = await import(versionedWorkerAssetUrl("./player-wasm/player_wasm.js"));
      await module.default({
        module_or_path: versionedWorkerAssetUrl("./player-wasm/player_wasm_bg.wasm"),
      });
      module.initPanicHook?.();
      playerAppWasmModule = module;
      return module;
    })().catch((error) => {
      playerWasmModulePromise = null;
      playerAppWasmModule = null;
      throw error;
    });
  }
  return playerWasmModulePromise;
}

async function ensureRecordRenderWasmModule() {
  return ensurePlayerWasmModule();
}

async function ensurePlayerAppWasmModule() {
  return ensurePlayerWasmModule();
}

function requirePlayerAppWasmFunction(name) {
  const fn = playerAppWasmModule?.[name];
  if (typeof fn !== "function") {
    throw new Error(`Bitneedle player app WASM is missing ${name}.`);
  }
  return fn;
}

function normalizeWorkerRecordProfileName(recordProfile) {
  return requirePlayerAppWasmFunction("normalizeRecordProfileName")(String(recordProfile ?? ""));
}

async function ensureEncodecWasmModule() {
  return ensurePlayerAppWasmModule();
}

function assertMossNanoDecodeEnabled() {
  if (!MOSS_NANO_DECODE_ENABLED) {
    throw new Error(
      "MOSS Nano decode is disabled (PLAYER_MOSSNANO_DECODE_ENABLED is false in player-playback-config.js).",
    );
  }
}

async function ensureMossNanoWasmModule() {
  assertMossNanoDecodeEnabled();
  if (!mossNanoWasmModulePromise) {
    mossNanoWasmModulePromise = (async () => {
      const module = await import(versionedWorkerAssetUrl(`${MOSSNANO_WASM_BASE_URL}/pkg/mossnano_rs.js`));
      await module.default({
        module_or_path: versionedWorkerAssetUrl(`${MOSSNANO_WASM_BASE_URL}/pkg/mossnano_rs_bg.wasm`),
      });
      module.initPanicHook?.();
      return module;
    })().catch((error) => {
      mossNanoWasmModulePromise = null;
      throw error;
    });
  }
  return mossNanoWasmModulePromise;
}

async function ensureMossNanoDecodeSession(runtime) {
  assertMossNanoDecodeEnabled();
  if (!mossNanoDecodeSessionPromise) {
    mossNanoDecodeSessionPromise = (async () => {
      const ort = await ensureOnnxRuntimeModule();
      const model = await fetchMaybeSplitArrayBuffer(`${MOSS_NANO_MODEL_ROOT}/moss_audio_tokenizer_decode_full.onnx`);
      const external = new Uint8Array(await fetchMaybeSplitArrayBuffer(
        `${MOSS_NANO_MODEL_ROOT}/moss_audio_tokenizer_decode_shared.data`,
      ));
      const executionProviders =
        Array.isArray(runtime?.executionProviders) && runtime.executionProviders.length
          ? runtime.executionProviders
          : ["wasm"];
      const session = await ort.InferenceSession.create(model, {
        executionProviders: [...executionProviders],
        graphOptimizationLevel: "all",
        externalData: [
          {
            path: "moss_audio_tokenizer_decode_shared.data",
            data: external,
          },
        ],
      });
      return { ort, session };
    })().catch((error) => {
      mossNanoDecodeSessionPromise = null;
      throw error;
    });
  }
  return mossNanoDecodeSessionPromise;
}

async function fetchArrayBuffer(url) {
  const response = await fetch(url, { cache: "force-cache" });
  if (!response.ok) {
    throw new Error(`Failed to fetch ${url}: ${response.status}`);
  }
  return response.arrayBuffer();
}

function bundleAssetMetadata(meta, assetName) {
  const assets = meta?.bitneedle_player_assets || meta?.bitneedlePlayerAssets || {};
  return assets?.[assetName] || null;
}

async function fetchMaybeSplitArrayBuffer(assetPath, assetMetadata = null) {
  if (assetMetadata?.split === true) {
    return fetchSplitArrayBuffer(assetPath);
  }
  return fetchArrayBuffer(versionedWorkerAssetUrl(assetPath));
}

async function fetchSplitArrayBuffer(assetPath) {
  const partsManifestUrl = versionedWorkerAssetUrl(`${assetPath}.parts.json`);
  const manifestResponse = await fetch(partsManifestUrl, { cache: "force-cache" });
  if (!manifestResponse.ok) {
    throw new Error(`Failed to fetch ${partsManifestUrl}: ${manifestResponse.status}`);
  }

  const manifest = await manifestResponse.json();
  if (!Array.isArray(manifest.parts) || !Number.isInteger(manifest.byteLength)) {
    throw new Error(`Invalid asset parts manifest: ${partsManifestUrl}`);
  }

  const chunks = await Promise.all(
    manifest.parts.map(async (part) => new Uint8Array(await fetchArrayBuffer(versionedWorkerManifestPartUrl(part, partsManifestUrl)))),
  );
  const bytes = concatenateUint8Chunks(chunks);
  if (bytes.byteLength !== manifest.byteLength) {
    throw new Error(`Asset parts for ${assetPath} produced ${bytes.byteLength} bytes, expected ${manifest.byteLength}`);
  }
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
}

function concatenateUint8Chunks(chunks) {
  const total = chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
  const bytes = new Uint8Array(total);
  let offset = 0;
  chunks.forEach((chunk) => {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  });
  return bytes;
}

async function loadModelForOrt(modelPath, assetMetadata = null) {
  if (assetMetadata?.split !== true) {
    return versionedWorkerAssetUrl(modelPath);
  }
  const partsManifestUrl = versionedWorkerAssetUrl(`${modelPath}.parts.json`);
  const manifestResponse = await fetch(partsManifestUrl, { cache: "force-cache" });
  if (!manifestResponse.ok) {
    throw new Error(`Failed to fetch ${partsManifestUrl}: ${manifestResponse.status}`);
  }

  const manifest = await manifestResponse.json();
  if (!Array.isArray(manifest.parts) || !Number.isInteger(manifest.byteLength)) {
    throw new Error(`Invalid model parts manifest: ${partsManifestUrl}`);
  }

  const modelFilename = modelPath.split("/").pop();
  if (manifest.parts.length === 1 && manifest.parts[0] === modelFilename) {
    return versionedWorkerAssetUrl(modelPath);
  }

  const chunks = await Promise.all(
    manifest.parts.map(async (part) => new Uint8Array(await fetchArrayBuffer(versionedWorkerManifestPartUrl(part, partsManifestUrl)))),
  );
  const model = concatenateUint8Chunks(chunks);
  if (model.byteLength !== manifest.byteLength) {
    throw new Error(`Model parts for ${modelPath} produced ${model.byteLength} bytes, expected ${manifest.byteLength}`);
  }
  return model;
}

async function loadQuantizedLmWeights(bundleRoot, meta) {
  if (!meta?.lm_quant_weight_model) {
    throw new Error("Client playback ECDC requires q8 LM weights.");
  }
  const weightsUrl = `${bundleRoot}/${meta.lm_quant_weight_model}`;
  return new Uint8Array(await fetchMaybeSplitArrayBuffer(
    weightsUrl,
    bundleAssetMetadata(meta, meta.lm_quant_weight_model),
  ));
}

const bitneedleEncodecBundleNames = globalThis.BitneedleEncodecBundleNames;
if (!bitneedleEncodecBundleNames) {
  throw new Error("Bitneedle EnCodec bundle helpers were not loaded before client-playback-worker.js.");
}
const {
  encodeBundleNameForRecord: bitneedleEncodeBundleNameForRecord,
  encodeBundleNameFromEcdcMetadata: bitneedleEncodeBundleNameFromEcdcMetadata,
  getRecordMeta,
  recordPayloadByteLength,
  recordVisibleTurns,
} = bitneedleEncodecBundleNames;

function playerEncodeBundleNameForRecord(record) {
  return bitneedleEncodeBundleNameForRecord(record);
}

function playerEncodeBundleNameFromEcdcMetadata(metadata, recordContext) {
  return bitneedleEncodeBundleNameFromEcdcMetadata(metadata, recordContext);
}

function parseWorkerJson(value, fallback = null) {
  if (!value) {
    return fallback;
  }
  try {
    return typeof value === "string" ? JSON.parse(value) : value;
  } catch (_error) {
    return fallback;
  }
}

function normalizePayloadContainer(value, fallback = "") {
  const normalized = String(value || fallback || "").trim().toUpperCase();
  return normalized || String(fallback || "").trim().toUpperCase();
}

function payloadContainerIsEcdc(value) {
  return normalizePayloadContainer(value) === "ECDC";
}

function payloadContainerIsMossNano(value) {
  return normalizePayloadContainer(value) === "MOSSNANO";
}

function metadataFromRecordHeaderJson(header = {}) {
  const metadata = JSON.parse(
    requirePlayerAppWasmFunction("recordPlaybackMetadataFromHeaderJson")(
      typeof header === "string" ? header : JSON.stringify(header || {}),
    ),
  );
  if (!metadata || typeof metadata !== "object") {
    throw new Error("record header metadata parser returned an invalid value.");
  }
  return metadata;
}

function resolvePlaybackPayloadMetadata({
  headerMetadata = {},
  payloadMetadata = {},
  lengthPrefixedEntries = false,
} = {}) {
  const metadata = JSON.parse(
    requirePlayerAppWasmFunction("resolvePlaybackPayloadMetadataJson")(
      JSON.stringify(headerMetadata || {}),
      JSON.stringify(payloadMetadata || {}),
      Boolean(lengthPrefixedEntries),
    ),
  );
  if (!metadata || typeof metadata !== "object") {
    throw new Error("playback payload metadata resolver returned an invalid value.");
  }
  return metadata;
}

function metadataFromRecordHeader(module, pngBytes) {
  if (typeof module?.decodeRecordMetadataJson !== "function") {
    throw new Error("Bitneedle record renderer WASM is missing decodeRecordMetadataJson.");
  }
  return metadataFromRecordHeaderJson(module.decodeRecordMetadataJson(pngBytes));
}

// Pre-decode programme map (exact musical/GAP sample boundaries + groove
// anchors). Optional: older records or older WASM lack the export, in which case
// the player falls back to its uncalibrated stylus mapping.
function programmeMapFromRecordHeader(module, pngBytes) {
  if (typeof module?.decodeRecordProgrammeMapJson !== "function") {
    return null;
  }
  try {
    return parseWorkerJson(module.decodeRecordProgrammeMapJson(pngBytes), null);
  } catch (_error) {
    return null;
  }
}

function readPayloadDecodeResult(result) {
  const payloadBytes = result?.payloadBytes?.() || result?.payload_bytes?.() || result?.payloadBytes || result?.payload || null;
  const chunkStreamBytes = result?.chunkStreamBytes?.() || result?.chunk_stream_bytes?.() || result?.chunkStreamBytes || result?.chunkStream || null;
  const metadataJson = result?.metadataJson?.() || result?.metadata_json?.() || result?.metadataJson || "{}";
  const silenceMapJson = result?.silenceMapJson?.() || result?.silence_map_json?.() || result?.silenceMapJson || "[]";
  const payload = payloadBytes instanceof Uint8Array ? payloadBytes : new Uint8Array(payloadBytes || 0);
  const chunkStream = chunkStreamBytes instanceof Uint8Array ? chunkStreamBytes : new Uint8Array(chunkStreamBytes || 0);
  const metadata = parseWorkerJson(metadataJson, {}) || {};
  const silenceMap = parseWorkerJson(silenceMapJson, []) || [];
  result?.free?.();
  return { payload, chunkStream, metadata, silenceMap };
}

function decodeRecordPayloadWithModule(module, {
  bytes,
  profile,
  explicitProfile,
  byteLength,
  visibleTurns,
}) {
  if (!explicitProfile && !byteLength && !visibleTurns && typeof module.decodeRecordPngToPayload === "function") {
    return readPayloadDecodeResult(module.decodeRecordPngToPayload(bytes));
  }
  if (visibleTurns && byteLength && typeof module.decodeRecordPngToPayloadForProfileWithTurnsAndLength === "function") {
    return readPayloadDecodeResult(module.decodeRecordPngToPayloadForProfileWithTurnsAndLength(bytes, profile, visibleTurns, byteLength));
  }
  if (visibleTurns && typeof module.decodeRecordPngToPayloadForProfileWithTurns === "function") {
    return readPayloadDecodeResult(module.decodeRecordPngToPayloadForProfileWithTurns(bytes, profile, visibleTurns));
  }
  if (byteLength && typeof module.decodeRecordPngToPayloadForProfileWithLength === "function") {
    return readPayloadDecodeResult(module.decodeRecordPngToPayloadForProfileWithLength(bytes, profile, byteLength));
  }
  if (typeof module.decodeRecordPngToPayloadForProfile === "function") {
    return readPayloadDecodeResult(module.decodeRecordPngToPayloadForProfile(bytes, profile));
  }
  if (byteLength && typeof module.decodeRecordPngToPayloadWithLength === "function") {
    return readPayloadDecodeResult(module.decodeRecordPngToPayloadWithLength(bytes, byteLength));
  }
  if (typeof module.decodeRecordPngToPayload === "function") {
    return readPayloadDecodeResult(module.decodeRecordPngToPayload(bytes));
  }
  return null;
}

async function fetchEncodeBundle(bundleName) {
  const bundleRoot = `${ENCODEC_BUNDLE_BASE_URL.replace(/\/+$/, "")}/${bundleName}`;
  const response = await fetch(versionedWorkerAssetUrl(`${bundleRoot}/bundle.json`), { cache: "force-cache" });
  if (!response.ok) {
    throw new Error(`Failed to load EnCodec bundle ${bundleName}: ${response.status}`);
  }
  const bundleJson = await response.text();
  return {
    bundleName,
    bundleRoot,
    bundleJson,
    meta: JSON.parse(bundleJson),
  };
}

async function extractPayloadFromRecordPng({ id, pngBytes, record = {}, recordProfile }) {
  workerDecodeDebug("[play:client-playback-worker.js] extractPayloadFromRecordPng", { id, pngBytes: pngBytes?.byteLength || 0, recordProfile });
  const perfMark = workerPerfStart("extract record PNG", {
    pngBytes: pngBytes?.byteLength || 0,
  });
  postDecodeProgress(id, {
    msg: "Extracting spiral payload...",
    progressPercent: 0,
  });
  try {
    const module = await ensureRecordRenderWasmModule();
    await ensurePlayerAppWasmModule();
    const bytes = pngBytes instanceof Uint8Array ? pngBytes : new Uint8Array(pngBytes || 0);
    // The spiral descriptor carries the record release id (ULID) — the same
    // key press uses for the locally cached master-quality source audio.
    let releaseId = "";
    if (typeof module.decodeRecordMetadataJson === "function") {
      try {
        const recordMetadata = JSON.parse(module.decodeRecordMetadataJson(bytes));
        releaseId = String(recordMetadata?.descriptor?.releaseId || "").trim();
      } catch (_error) {
        // Records without a spiral descriptor have no master release id.
      }
    }
    const explicitProfile =
      record?.recordProfile ||
      getRecordMeta(record).recordProfile ||
      "";
    let inferredProfile = "";
    if (!explicitProfile && typeof module.inferRecordProfileFromPng === "function") {
      inferredProfile = module.inferRecordProfileFromPng(bytes);
    }
    const profile = normalizeWorkerRecordProfileName(explicitProfile || inferredProfile || recordProfile);
    const byteLength = recordPayloadByteLength(record);
    const visibleTurns = recordVisibleTurns(record);
    let decoded = decodeRecordPayloadWithModule(module, {
      bytes,
      profile,
      explicitProfile,
      byteLength,
      visibleTurns,
    });
    if (!decoded?.payload?.byteLength) {
      throw new Error("Bitneedle Player WASM does not expose PNG audio extraction.");
    }
    const payloadBytes = decoded.payload instanceof Uint8Array ? decoded.payload : new Uint8Array(decoded.payload);
    const headerMetadata = metadataFromRecordHeader(module, bytes);
    const programmeMap = programmeMapFromRecordHeader(module, bytes);
    const lengthPrefixedEntries = false;
    const playbackMetadata = resolvePlaybackPayloadMetadata({
      headerMetadata,
      payloadMetadata: decoded.metadata || {},
      lengthPrefixedEntries,
    });
    const {
      payloadContainer,
      payloadCodec,
      entryContainer,
      trackListing,
      dummySpiralRegions,
    } = playbackMetadata;
    if (!payloadContainer) {
      throw new Error("record payload metadata must declare payloadContainer.");
    }
    let bundleName = "";
    let recordHeaderProof = null;
    if (payloadContainerIsEcdc(payloadContainer)) {
      const ecdcForMetadata = lengthPrefixedEntries
        ? parseLengthPrefixedPayloadEntries(payloadBytes)[0]
        : payloadBytes;
      const metadata = (await ensureEncodecWasmModule()).ecdcMetadata(ecdcForMetadata);
      bundleName = playerEncodeBundleNameFromEcdcMetadata(metadata, record);
      recordHeaderProof = lengthPrefixedEntries
        ? null
        : createWorkerEcdcCacheProofContext(payloadBytes);
      workerDecodeDebug("[bitneedle-cache-dbg] worker recordHeaderProof", {
        lengthPrefixedEntries,
        hasProof: Boolean(recordHeaderProof),
        proofFormat: recordHeaderProof?.format || null,
        proofChunks: recordHeaderProof?.chunks?.length ?? null,
      });
    } else if (payloadContainerIsMossNano(payloadContainer)) {
      bundleName = payloadCodec || MOSS_NANO_CODEC;
    }
    workerPerfEnd(id, perfMark, {
      payloadBytes: payloadBytes.byteLength,
      payloadContainer,
      profile,
      bundleName,
    });
    return {
      payload: payloadBytes,
      chunkStream: decoded.chunkStream instanceof Uint8Array ? decoded.chunkStream : new Uint8Array(decoded.chunkStream || 0),
      ecdc: payloadContainerIsEcdc(payloadContainer) ? payloadBytes : null,
      payloadContainer,
      payloadCodec,
      entryContainer,
      payloadMetadata: decoded.metadata,
      trackListing,
      dummySpiralRegions,
      silenceMap: Array.isArray(decoded.silenceMap) ? decoded.silenceMap : [],
      programmeMap,
      bundleName,
      recordProfile: profile,
      recordHeaderProof,
      releaseId,
    };
  } catch (error) {
    workerPerfEnd(id, perfMark, {
      error: workerErrorMessage(error),
    });
    throw error;
  }
}

const { createOnnxRuntimeSession } = globalThis.BitneedleOnnxRuntimeSession || {};
if (typeof createOnnxRuntimeSession !== "function") {
  throw new Error("Bitneedle ONNX runtime session helpers were not loaded before client-playback-worker.js.");
}
const {
  buildDecodeInputs,
  disposeOrtTensorMap,
  findDecodeOutput,
} = globalThis.BitneedleOnnxWorkerTensors || {};
if (
  typeof buildDecodeInputs !== "function" ||
  typeof disposeOrtTensorMap !== "function" ||
  typeof findDecodeOutput !== "function"
) {
  throw new Error("Bitneedle ONNX worker tensor helpers were not loaded before client-playback-worker.js.");
}

async function createDecodeSession(modelPath, runtime, assetMetadata = null) {
  return createOnnxRuntimeSession({
    modelPath,
    runtime,
    assetMetadata,
    ensureOnnxRuntimeModule,
    loadModelForOrt,
    // The CPU memory arena and memory pattern planner trade higher retained
    // WASM heap for faster repeat allocations; with single-frame decode
    // batches the savings dominate, so keep the heap small instead.
    enableCpuMemArena: false,
    enableMemPattern: false,
  });
}

async function decodeLmChunk(encodecModule, lmWeights, bundleJson, chunk, meta) {
  workerDecodeDebug("[play:client-playback-worker.js] decodeLmChunk", { offset: chunk?.offset, samples: chunk?.samples, frameLength: chunk?.frameLength, numCodebooks: meta?.num_codebooks });
  const payload = chunk.payload instanceof Uint8Array ? chunk.payload : Uint8Array.from(chunk.payload || []);
  const decoder = new encodecModule.QuantizedLmChunkDecoder(bundleJson, lmWeights, payload);
  const encodedFrameLength = Math.max(0, Number(chunk.frameLength) || 0);
  const frameLength = Math.max(1, encodedFrameLength || Number(meta.frame_length) || 1);
  const codes = new Uint16Array(meta.num_codebooks * frameLength);

  try {
    for (let step = 0; step < Math.min(encodedFrameLength || frameLength, frameLength); step += 1) {
      const symbols = decoder.pull();
      for (let codebook = 0; codebook < meta.num_codebooks; codebook += 1) {
        codes[codebook * frameLength + step] = symbols[codebook];
      }
    }
    return {
      offset: chunk.offset,
      samples: chunk.samples,
      frameLength,
      scale: decoder.scale(),
      codes,
    };
  } finally {
    decoder.free?.();
  }
}

function normalizeEncodedFrame(frame) {
  return {
    offset: frame.offset,
    samples: frame.samples,
    frameLength: frame.frameLength,
    scale: Number(frame.scale ?? 1),
    codes: frame.codes instanceof Uint16Array ? frame.codes : new Uint16Array(frame.codes || []),
    silentPcm: frame.silentPcm === true,
    decodeFailed: frame.decodeFailed === true,
  };
}

function createSilentEncodedFrame(chunk, meta) {
  const frameLength = Math.max(1, Number(chunk?.frameLength) || Number(meta?.frame_length) || 1);
  const codebooks = Math.max(1, Number(meta?.num_codebooks) || 1);
  return {
    offset: chunk?.offset,
    samples: chunk?.samples,
    frameLength,
    scale: 1,
    codes: new Uint16Array(codebooks * frameLength),
    silentPcm: true,
    decodeFailed: true,
  };
}

async function prepareEcdcChunkDecode({ ecdcBuffer, bundleJson, bundleRoot, bundleName = "", meta }) {
  workerDecodeDebug("[play:client-playback-worker.js] prepareEcdcChunkDecode", { ecdcBytes: ecdcBuffer?.byteLength || 0, bundleName });
  const encodecModule = await ensureEncodecWasmModule();
  const ecdc = ecdcBuffer instanceof Uint8Array ? ecdcBuffer : new Uint8Array(ecdcBuffer);
  workerDecodeDebug("[play:client-playback-worker.js] prepareEcdcChunkDecode wasm ready", { ecdcBytes: ecdc.byteLength, firstBytes: Array.from(ecdc.subarray(0, 16)) });
  let metadata;
  try {
    metadata = encodecModule.ecdcMetadata(ecdc);
  } catch (error) {
    console.error("[play:client-playback-worker.js] prepareEcdcChunkDecode ecdcMetadata threw", { ecdcBytes: ecdc.byteLength, error: workerErrorMessage(error) });
    throw error;
  }
  const bitstreamVersion = Number(metadata.acv ?? metadata.bitstream_version ?? 0);
  workerDecodeDebug("[play:client-playback-worker.js] prepareEcdcChunkDecode metadata", { bitstreamVersion, metadata });
  if (bitstreamVersion !== 2) {
    throw new Error(`Client playback ECDC requires q8 LM acv=2, got acv=${bitstreamVersion}`);
  }

  let parsed;
  try {
    parsed = encodecModule.lmEcdcDecodeChunks(bundleJson, ecdc);
  } catch (error) {
    console.error("[play:client-playback-worker.js] prepareEcdcChunkDecode lmEcdcDecodeChunks threw", { ecdcBytes: ecdc.byteLength, error: workerErrorMessage(error) });
    throw error;
  }
  const chunks = parsed.chunks || [];
  workerDecodeDebug("[play:client-playback-worker.js] prepareEcdcChunkDecode chunks decoded", { chunkCount: chunks.length });
  const expectedLmHash = metadata.lmh ?? metadata.lm_hash;
  workerDecodeDebug("[play:client-playback-worker.js] prepareEcdcChunkDecode loading LM weights", { lmQuantWeightModel: meta?.lm_quant_weight_model || "", bundleRoot });
  const lmWeights = await loadQuantizedLmWeights(bundleRoot, meta);
  workerDecodeDebug("[play:client-playback-worker.js] prepareEcdcChunkDecode LM weights loaded", { lmWeightBytes: lmWeights.byteLength });
  const actualLmHash = encodecModule.stableHashHex(lmWeights);
  if (expectedLmHash && expectedLmHash !== actualLmHash) {
    console.error("[bitneedle-player-worker] ECDC LM hash mismatch", {
      expectedLmHash,
      actualLmHash,
      requiredBrowserLmHash: expectedLmHash,
      bundleName,
    });
    throw new Error(`Client playback ECDC requires LM hash ${expectedLmHash}, but loaded ${actualLmHash}`);
  }
  // `parsed.metadata` is the aggregate across every revolution in the
  // concatenated payload (lmEcdcDecodeChunks sums audio_length), whereas the
  // standalone `metadata` from ecdcMetadata only describes the first revolution.
  // Use the aggregate so playback duration and PCM overlap-add span the whole
  // record, not just the first ~1.3s/1.8s entry.
  const aggregateMetadata = parsed.metadata || metadata;
  const audioLength = Number(aggregateMetadata.al ?? aggregateMetadata.audio_length) || 0;
  workerDecodeDebug("[play:client-playback-worker.js] prepareEcdcChunkDecode parsed", { bundleName, chunkCount: chunks.length, audioLength, bitstreamVersion });
  return {
    encodecModule,
    chunks,
    lmWeights,
    metadata: aggregateMetadata,
    audioLength,
  };
}

function frameDescriptorFromEcdcChunk(chunk, meta) {
  return {
    offset: chunk?.offset,
    samples: chunk?.samples,
    frameLength: Math.max(1, Number(chunk?.frameLength) || Number(meta?.frame_length) || 1),
  };
}

function createS16OverlapAddWriter({ meta, frames, audioLength, layout }) {
  const channels = Math.max(1, Number(meta.channels) || 1);
  const safeAudioLength = Math.max(0, Math.floor(Number(audioLength) || 0));
  const weight = triangleWeight(layout.samples);
  // Overlap-add only ever needs the un-emitted tail of the signal (the
  // current frame window plus overlap), so accumulate into a ring buffer
  // instead of full-length Float32Arrays. Full-length accumulators cost
  // ~12 bytes per sample per channel and were the largest single allocation
  // in the decode pipeline on long records.
  let ringCapacity = nextRingPow2(Math.max(1, layout.samples * 2));
  let ringMask = ringCapacity - 1;
  let sums = new Float32Array(channels * ringCapacity);
  let weights = new Float32Array(ringCapacity);
  const channelData = Array.from({ length: channels }, () => new Int16Array(safeAudioLength));
  let emittedSample = 0;
  let maxWrittenSample = 0;
  let emittedSegmentIndex = 0;
  // Ranges filled directly from the remote segment cache: their PCM is final,
  // so emit must not overwrite them from the overlap-add ring. Pushed in
  // ascending order (the decode loop walks chunks sequentially).
  const cachedRanges = [];
  let cachedRangePointer = 0;

  function nextRingPow2(value) {
    let capacity = 1;
    while (capacity < value) {
      capacity *= 2;
    }
    return capacity;
  }

  function growRing(requiredSpan) {
    const newCapacity = nextRingPow2(requiredSpan);
    const newMask = newCapacity - 1;
    const newSums = new Float32Array(channels * newCapacity);
    const newWeights = new Float32Array(newCapacity);
    for (let globalSample = emittedSample; globalSample < maxWrittenSample; globalSample += 1) {
      const oldIndex = globalSample & ringMask;
      const newIndex = globalSample & newMask;
      newWeights[newIndex] = weights[oldIndex];
      for (let channel = 0; channel < channels; channel += 1) {
        newSums[(channel * newCapacity) + newIndex] = sums[(channel * ringCapacity) + oldIndex];
      }
    }
    sums = newSums;
    weights = newWeights;
    ringCapacity = newCapacity;
    ringMask = newMask;
  }

  function ensureRingSpan(endSample) {
    if (endSample - emittedSample > ringCapacity) {
      growRing(endSample - emittedSample);
    }
  }

  function addDecodedBatch(start, end, decodedFrameChunk) {
    const batchSize = end - start;
    if (!(batchSize > 0)) {
      return;
    }
    const decodedSamples = Math.floor(decodedFrameChunk.length / Math.max(1, batchSize * channels));
    if (!(decodedSamples > 0)) {
      throw new Error("Decoded PCM batch was empty.");
    }

    for (let frameIndex = start; frameIndex < end; frameIndex += 1) {
      const frame = frames[frameIndex];
      const localIndex = frameIndex - start;
      const frameStart = frameStartSample(frame, frameIndex, layout);
      const frameSamples = Math.max(
        0,
        Math.min(
          Number(frame?.samples) || layout.samples,
          layout.samples,
          safeAudioLength - frameStart,
        ),
      );
      if (decodedSamples < frameSamples) {
        throw new Error(`Decoded frame ${frameIndex + 1} has ${decodedSamples} samples, expected at least ${frameSamples}.`);
      }
      const sourceBase = localIndex * channels * decodedSamples;
      ensureRingSpan(Math.min(safeAudioLength, frameStart + frameSamples));
      for (let sample = 0; sample < frameSamples; sample += 1) {
        const globalSample = frameStart + sample;
        if (globalSample < emittedSample || globalSample >= safeAudioLength) {
          continue;
        }
        const w = weight[sample] || 0;
        const ringIndex = globalSample & ringMask;
        weights[ringIndex] += w;
        for (let channel = 0; channel < channels; channel += 1) {
          sums[(channel * ringCapacity) + ringIndex] +=
            decodedFrameChunk[sourceBase + (channel * decodedSamples) + sample] * w;
        }
      }
      maxWrittenSample = Math.max(
        maxWrittenSample,
        Math.min(safeAudioLength, frameStart + frameSamples),
      );
    }
  }

  function addCachedRange(startFrame, endFrame, interleavedInt16) {
    const start = Math.max(0, Math.min(safeAudioLength, Math.floor(Number(startFrame) || 0)));
    const end = Math.max(start, Math.min(safeAudioLength, Math.floor(Number(endFrame) || 0)));
    if (!(end > start) || !(interleavedInt16?.length >= (end - start) * channels)) {
      return false;
    }
    for (let globalSample = start; globalSample < end; globalSample += 1) {
      const localBase = (globalSample - start) * channels;
      for (let channel = 0; channel < channels; channel += 1) {
        channelData[channel][globalSample] = interleavedInt16[localBase + channel];
      }
    }
    cachedRanges.push([start, end]);
    maxWrittenSample = Math.max(maxWrittenSample, end);
    return true;
  }

  function addSilentFrame(frameIndex) {
    const frame = frames[frameIndex];
    const frameStart = frameStartSample(frame, frameIndex, layout);
    const frameSamples = Math.max(
      0,
      Math.min(
        Number(frame?.samples) || layout.samples,
        layout.samples,
        safeAudioLength - frameStart,
      ),
    );
    ensureRingSpan(Math.min(safeAudioLength, frameStart + frameSamples));
    for (let sample = 0; sample < frameSamples; sample += 1) {
      const globalSample = frameStart + sample;
      if (globalSample >= emittedSample && globalSample < safeAudioLength) {
        weights[globalSample & ringMask] += weight[sample] || 0;
      }
    }
    maxWrittenSample = Math.max(
      maxWrittenSample,
      Math.min(safeAudioLength, frameStart + frameSamples),
    );
  }

  function addSilentBatch(start, end) {
    for (let frameIndex = start; frameIndex < end; frameIndex += 1) {
      addSilentFrame(frameIndex);
    }
  }

  function emitUntil(sampleEnd) {
    const safeEnd = Math.max(emittedSample, Math.min(safeAudioLength, Math.floor(Number(sampleEnd) || 0)));
    for (let globalSample = emittedSample; globalSample < safeEnd; globalSample += 1) {
      const ringIndex = globalSample & ringMask;
      while (cachedRangePointer < cachedRanges.length && globalSample >= cachedRanges[cachedRangePointer][1]) {
        cachedRangePointer += 1;
      }
      const isCachedSample = cachedRangePointer < cachedRanges.length
        && globalSample >= cachedRanges[cachedRangePointer][0];
      const denom = weights[ringIndex];
      for (let channel = 0; channel < channels; channel += 1) {
        const sourceIndex = (channel * ringCapacity) + ringIndex;
        if (!isCachedSample) {
          channelData[channel][globalSample] = floatToS16Sample(denom > 0 ? sums[sourceIndex] / denom : 0);
        }
        sums[sourceIndex] = 0;
      }
      weights[ringIndex] = 0;
    }
    emittedSample = safeEnd;
    maxWrittenSample = Math.max(maxWrittenSample, emittedSample);
  }

  function safeEmitEnd(nextFrameIndex) {
    if (nextFrameIndex >= frames.length) {
      return safeAudioLength;
    }
    return frameStartSample(frames[nextFrameIndex], nextFrameIndex, layout);
  }

  function collectEmittedSegments() {
    const segments = [];
    while (emittedSegmentIndex < frames.length) {
      const segmentStart = Math.max(
        0,
        Math.min(
          safeAudioLength,
          frameStartSample(frames[emittedSegmentIndex], emittedSegmentIndex, layout),
        ),
      );
      const segmentEnd = Math.max(
        segmentStart,
        Math.min(safeAudioLength, safeEmitEnd(emittedSegmentIndex + 1)),
      );
      if (segmentEnd > emittedSample) {
        break;
      }
      if (segmentEnd > segmentStart) {
        segments.push({
          chunkIndex: emittedSegmentIndex,
          startFrame: segmentStart,
          endFrame: segmentEnd,
          channelData: channelData.map((channel) => channel.slice(segmentStart, segmentEnd)),
        });
      }
      emittedSegmentIndex += 1;
    }
    return segments;
  }


  return {
    addCachedRange,
    addDecodedBatch,
    addSilentBatch,
    addSilentFrame,
    emitAfterBatch(nextFrameIndex) {
      emitUntil(safeEmitEnd(nextFrameIndex));
      return collectEmittedSegments();
    },
    flush() {
      emitUntil(safeAudioLength);
      return collectEmittedSegments();
    },
    result() {
      this.flush();
      return { channelData };
    },
  };
}

function normalizedProgressSegments(segments, { sampleRate, channels, audioLength }, transfer) {
  return segments.map((segment) => {
    // Always send an independent copy of each channel's bytes. The raw-segment
    // progress post (postRawDecodedPcmSegments) hands over `rawSegment` while the
    // overlap-add assembler still reads it on the next chunk, so transferring the
    // segment's *live* buffer would detach it mid-decode — corrupting the
    // assembler and shipping a detached ArrayBuffer that throws "Cannot perform
    // Construct on a detached ArrayBuffer" when the main thread rebuilds the view.
    // We transfer the freshly-sliced copies (cheap, and they are never reused
    // here), so the worker keeps its originals intact.
    const channelBuffers = segment.channelData.map((channel) => {
      const buffer = channel.buffer.slice(channel.byteOffset, channel.byteOffset + channel.byteLength);
      transfer.push(buffer);
      return buffer;
    });
    return {
      chunkIndex: segment.chunkIndex,
      startFrame: segment.startFrame,
      endFrame: segment.endFrame,
      sampleRate,
      channels,
      audioLength,
      channelBuffers,
    };
  });
}

function postDecodedPcmSegments(id, segments, { sampleRate, channels, audioLength }) {
  if (!Array.isArray(segments) || !segments.length) {
    return;
  }
  const transfer = [];
  const decodedPcmSegments = normalizedProgressSegments(segments, { sampleRate, channels, audioLength }, transfer);
  playerMessageLog.action("pcm-segments-ready", {
    requestId: id,
    segmentCount: decodedPcmSegments.length,
    sampleRate,
    channels,
    audioLength,
    segments: decodedPcmSegments.map(segment => ({
      chunkIndex: segment.chunkIndex,
      startFrame: segment.startFrame,
      endFrame: segment.endFrame,
      frameCount: Math.max(0, segment.endFrame - segment.startFrame),
      byteLength: segment.channelBuffers.reduce((sum, buffer) => sum + (buffer?.byteLength || 0), 0),
    })),
  });
  postDecodeProgress(id, { decodedPcmSegments }, transfer);
}

function postRawDecodedPcmSegments(id, segments, { sampleRate, channels, audioLength }) {
  if (!Array.isArray(segments) || !segments.length) {
    return;
  }
  const transfer = [];
  const rawDecodedPcmSegments = normalizedProgressSegments(segments, { sampleRate, channels, audioLength }, transfer);
  postDecodeProgress(id, { rawDecodedPcmSegments }, transfer);
}

function seamRepairProfileFromMeta(meta) {
  const repair = String(meta?.seam_repair || meta?.seamRepair || "").trim();
  if (!repair) {
    return null;
  }
  if (repair !== "cubic-hermite-v1") {
    throw new Error(`Unsupported ECDC seam repair profile: ${repair}`);
  }
  const ownedSamples = Math.max(0, Math.floor(
    Number(meta?.owned_samples ?? meta?.ownedSamples) || 0,
  ));
  const repairSamples = Math.max(0, Math.floor(
    Number(meta?.seam_repair_samples ?? meta?.seamRepairSamples) || 0,
  ));
  if (!(ownedSamples > 0) || !(repairSamples > 0) || (repairSamples % 2) !== 0) {
    throw new Error(`Invalid cubic-hermite-v1 profile: ownedSamples=${ownedSamples} repairSamples=${repairSamples}`);
  }
  return {
    repair,
    ownedSamples,
    repairSamples,
  };
}

function clampS16(value) {
  const rounded = Math.round(Number(value) || 0);
  if (rounded < -32768) return -32768;
  if (rounded > 32767) return 32767;
  return rounded;
}

function applyHermiteSeamRepairToS16(channelData, audioLength, profile) {
  if (!Array.isArray(channelData) || !channelData.length) {
    return;
  }
  const ownedSamples = Math.max(1, profile.ownedSamples);
  const eachSide = Math.max(1, Math.floor(profile.repairSamples / 2));
  for (let seamFrame = ownedSamples; seamFrame < audioLength; seamFrame += ownedSamples) {
    const start = Math.max(1, seamFrame - eachSide);
    const end = Math.min(audioLength - 1, seamFrame + eachSide);
    const span = end - start;
    if (!(span > 0)) {
      continue;
    }
    for (const channel of channelData) {
      const y0 = channel[start - 1];
      const y1 = channel[end];
      const m0 = channel[start] - channel[start - 1];
      const m1 = channel[end] - channel[end - 1];
      for (let index = 0; index < span; index += 1) {
        const t = (index + 1) / (span + 1);
        const h00 = (2 * t ** 3) - (3 * t ** 2) + 1;
        const h10 = (t ** 3) - (2 * t ** 2) + t;
        const h01 = (-2 * t ** 3) + (3 * t ** 2);
        const h11 = (t ** 3) - (t ** 2);
        channel[start + index] = clampS16(
          (h00 * y0) + (h10 * span * m0) + (h01 * y1) + (h11 * span * m1),
        );
      }
    }
  }
}

function deinterleaveS16Segment(interleavedInt16, channels, frameCount) {
  const channelData = Array.from({ length: channels }, () => new Int16Array(frameCount));
  for (let frame = 0; frame < frameCount; frame += 1) {
    for (let channel = 0; channel < channels; channel += 1) {
      channelData[channel][frame] = interleavedInt16[(frame * channels) + channel] || 0;
    }
  }
  return channelData;
}

function interleaveS16Channels(channelData) {
  const channels = Array.isArray(channelData) ? channelData : [];
  const channelCount = channels.length;
  const frameCount = channelCount ? channels[0].length : 0;
  const interleaved = new Int16Array(frameCount * Math.max(1, channelCount));
  for (let frame = 0; frame < frameCount; frame += 1) {
    for (let channel = 0; channel < channelCount; channel += 1) {
      interleaved[(frame * channelCount) + channel] = channels[channel]?.[frame] || 0;
    }
  }
  return interleaved;
}

function cropDecodedOwnedSegmentToS16(encodecModule, bundleJson, decodedFloat32, meta, chunkIndex, audioLength) {
  const cropped = encodecModule.ecdcCropDecodedOwnedAudio(bundleJson, decodedFloat32);
  const planar = cropped instanceof Float32Array ? cropped : new Float32Array(cropped);
  const channels = Math.max(1, Number(meta?.channels) || 2);
  const ownedSamples = Math.max(1, Number(meta?.owned_samples ?? meta?.ownedSamples) || Math.floor(planar.length / channels));
  const startFrame = chunkIndex * ownedSamples;
  const endFrame = Math.max(startFrame, Math.min(audioLength, startFrame + ownedSamples));
  const frameCount = endFrame - startFrame;
  const channelData = Array.from({ length: channels }, () => new Int16Array(frameCount));
  for (let channel = 0; channel < channels; channel += 1) {
    const sourceOffset = channel * ownedSamples;
    const target = channelData[channel];
    for (let frame = 0; frame < frameCount; frame += 1) {
      target[frame] = floatToS16Sample(planar[sourceOffset + frame] || 0);
    }
  }
  return {
    chunkIndex,
    startFrame,
    endFrame,
    channelData,
  };
}

function applyHermiteBetweenChunkSegments(leftSegment, rightSegment, repairSamples) {
  const eachSide = Math.max(1, Math.floor(repairSamples / 2));
  const channels = Math.min(leftSegment.channelData.length, rightSegment.channelData.length);
  for (let channel = 0; channel < channels; channel += 1) {
    const left = leftSegment.channelData[channel];
    const right = rightSegment.channelData[channel];
    const leftFrames = left.length;
    const rightFrames = right.length;
    if (leftFrames < 2 || rightFrames < 2) {
      continue;
    }
    const leftReplace = Math.min(eachSide, leftFrames - 1);
    const rightReplace = Math.min(eachSide, rightFrames - 1);
    const span = leftReplace + rightReplace;
    if (!(span > 0)) {
      continue;
    }
    const leftStart = leftFrames - leftReplace;
    const y0 = left[leftStart - 1];
    const y1 = right[rightReplace];
    const m0 = left[leftStart] - left[leftStart - 1];
    const m1 = right[rightReplace] - right[rightReplace - 1];
    for (let index = 0; index < span; index += 1) {
      const t = (index + 1) / (span + 1);
      const h00 = (2 * t ** 3) - (3 * t ** 2) + 1;
      const h10 = (t ** 3) - (2 * t ** 2) + t;
      const h01 = (-2 * t ** 3) + (3 * t ** 2);
      const h11 = (t ** 3) - (t ** 2);
      const value = clampS16(
        (h00 * y0) + (h10 * span * m0) + (h01 * y1) + (h11 * span * m1),
      );
      if (index < leftReplace) {
        left[leftStart + index] = value;
      } else {
        right[index - leftReplace] = value;
      }
    }
  }
}

function createHermiteChunkAssembler({ channels, audioLength, ownedSamples, repairSamples }) {
  const finalChannelData = Array.from({ length: channels }, () => new Int16Array(audioLength));
  let pendingSegment = null;

  function copyToOutput(segment) {
    const start = Math.max(0, Math.floor(Number(segment.startFrame) || 0));
    const end = Math.max(start, Math.min(audioLength, Math.floor(Number(segment.endFrame) || start)));
    const frameCount = end - start;
    for (let channel = 0; channel < channels; channel += 1) {
      const source = segment.channelData[Math.min(channel, segment.channelData.length - 1)] || segment.channelData[0];
      if (!source) continue;
      finalChannelData[channel].set(source.subarray(0, frameCount), start);
    }
  }

  return {
    addRawSegment(segment) {
      const emitted = [];
      if (pendingSegment) {
        applyHermiteBetweenChunkSegments(pendingSegment, segment, repairSamples);
        copyToOutput(pendingSegment);
        emitted.push(pendingSegment);
      }
      pendingSegment = segment;
      return emitted;
    },
    flush() {
      if (!pendingSegment) {
        return [];
      }
      copyToOutput(pendingSegment);
      const final = pendingSegment;
      pendingSegment = null;
      return [final];
    },
    result() {
      const flushed = this.flush();
      return { flushed, channelData: finalChannelData };
    },
  };
}

function asInt32Array(values) {
  return values instanceof Int32Array ? values : Int32Array.from(values || []);
}

function firstMossFloatOutput(outputs) {
  for (const value of Object.values(outputs || {})) {
    if (value?.type === "float32") {
      return value;
    }
  }
  throw new Error(`MOSS Nano decoder returned no float32 output: ${Object.keys(outputs || {}).join(", ")}`);
}

function mossOutputTensor(outputs, name) {
  const tensor = outputs?.[name];
  if (!tensor) {
    throw new Error(`MOSS Nano decoder missing output tensor ${name}.`);
  }
  return tensor;
}

function trimMossPlanar(data, channels, frames, originalSamples) {
  const outFrames = Math.min(frames, Math.max(0, Math.floor(Number(originalSamples) || 0)));
  const out = new Float32Array(channels * outFrames);
  for (let channel = 0; channel < channels; channel += 1) {
    const srcOffset = channel * frames;
    const dstOffset = channel * outFrames;
    out.set(data.subarray(srcOffset, srcOffset + outFrames), dstOffset);
  }
  return out;
}

function mossPlanarAudio(tensor, metadata) {
  const dims = Array.from(tensor?.dims || []).map(Number);
  const channels = Math.max(1, Number(metadata?.channels) || 2);
  const originalSamples = Math.max(0, Math.floor(Number(metadata?.originalSamples) || 0));
  if (dims.length === 3 && dims[0] === 1 && dims[1] === channels) {
    return {
      channels,
      frames: Math.min(dims[2], originalSamples),
      planar: trimMossPlanar(tensor.data, channels, dims[2], originalSamples),
    };
  }
  if (dims.length === 2 && dims[0] === channels) {
    return {
      channels,
      frames: Math.min(dims[1], originalSamples),
      planar: trimMossPlanar(tensor.data, channels, dims[1], originalSamples),
    };
  }
  throw new Error(`unsupported MOSS Nano audio output shape ${JSON.stringify(dims)}`);
}

function copyMossPlanarChunkToS16(channelData, planar, channels, startFrame, frames, audioLength) {
  const safeFrames = Math.max(0, Math.min(
    Math.floor(Number(frames) || 0),
    Math.max(0, Math.floor(Number(audioLength) || 0) - Math.max(0, startFrame)),
  ));
  if (!(safeFrames > 0)) {
    return null;
  }
  for (let channel = 0; channel < channels; channel += 1) {
    const sourceOffset = channel * frames;
    const target = channelData[channel];
    for (let sample = 0; sample < safeFrames; sample += 1) {
      target[startFrame + sample] = floatToS16Sample(planar[sourceOffset + sample] || 0);
    }
  }
  return {
    chunkIndex: 0,
    startFrame,
    endFrame: startFrame + safeFrames,
    channelData: channelData.map((channel) => channel.slice(startFrame, startFrame + safeFrames)),
  };
}

async function decodeMossNanoPlayback({
  id,
  payloadBuffer,
  recordProfile,
  runtime,
  cacheDecodedSegments = false,
}) {
  assertMossNanoDecodeEnabled();
  const perfMark = workerPerfStart("decode MOSS Nano playback", {
    payloadBytes: payloadBuffer?.byteLength || 0,
  });
  let stream = null;
  try {
    const payloadBytes = payloadBuffer instanceof Uint8Array ? payloadBuffer : new Uint8Array(payloadBuffer || 0);
    const mossNanoModule = await ensureMossNanoWasmModule();
    const metadata = JSON.parse(mossNanoModule.mossnanoMetadataJson(payloadBytes));
    const sampleRate = Math.max(1, Number(metadata.sampleRate) || 48000);
    const channels = Math.max(1, Number(metadata.channels) || 2);
    const audioLength = Math.max(1, Math.floor(Number(metadata.originalSamples) || 0));
    const profile = normalizeWorkerRecordProfileName(recordProfile);
    const chunkSeconds = MOSS_NANO_RECORD_PROFILE_CHUNK_SECONDS[profile];
    if (!(chunkSeconds > 0)) {
      throw new Error(`MOSS Nano decode does not support record profile ${profile}.`);
    }
    const chunkFrames = Math.max(1, Number(mossNanoModule.mossnanoChunkFramesForSeconds(sampleRate, chunkSeconds)) || 1);
    const actualChunkSeconds = Number(mossNanoModule.mossnanoChunkSecondsForFrames(sampleRate, chunkFrames)) || chunkSeconds;
    stream = new mossNanoModule.MossNanoDecodeStream(payloadBytes, chunkFrames);
    const { ort, session } = await ensureMossNanoDecodeSession(runtime);
    const channelData = Array.from({ length: channels }, () => new Int16Array(audioLength));
    const chunkCount = Math.max(1, Number(stream.chunkCount()) || 1);
    let writeFrame = 0;
    let chunkIndex = 0;

    while (stream.hasNext()) {
      const tokenStart = Number(stream.nextStartFrame()) || 0;
      const tokenFrames = Math.max(1, Number(stream.nextTokenFrames()) || 1);
      postDecodeProgress(id, {
        msg: `Reading MOSS Nano audio ${chunkIndex + 1}/${chunkCount}`,
        progressPercent: Math.round((chunkIndex / chunkCount) * 100),
        processedSeconds: formatDecimal(Math.min(audioLength / sampleRate, writeFrame / sampleRate), 2),
        duration: formatDecimal(audioLength / sampleRate, 2),
      });
      const tensorCodes = stream.nextCodesTqI32();
      let feeds = null;
      let outputs = null;
      try {
        const audioCodeInput = session.inputNames.includes("audio_codes") ? "audio_codes" : session.inputNames[0];
        const audioLengthInput = session.inputNames.includes("audio_code_lengths")
          ? "audio_code_lengths"
          : session.inputNames[1];
        feeds = {
          [audioCodeInput]: new ort.Tensor("int32", asInt32Array(tensorCodes), [1, tokenFrames, Number(metadata.quantizers) || 16]),
          [audioLengthInput]: new ort.Tensor("int32", Int32Array.of(tokenFrames), [1]),
        };
        outputs = await session.run(feeds);
        const audioTensor = firstMossFloatOutput(outputs);
        const audioLengths = outputs.audio_lengths ? mossOutputTensor(outputs, "audio_lengths") : null;
        const requestedFrames = audioLengths ? Number(audioLengths.data[0]) : tokenFrames * 3840;
        const chunk = mossPlanarAudio(audioTensor, {
          ...metadata,
          channels,
          originalSamples: Math.min(requestedFrames, audioLength - writeFrame),
        });
        const segment = copyMossPlanarChunkToS16(
          channelData,
          chunk.planar,
          channels,
          writeFrame,
          chunk.frames,
          audioLength,
        );
        stream.pushDecodedPlanar(chunk.planar, channels, chunk.frames);
        if (segment) {
          segment.chunkIndex = chunkIndex;
          postDecodedPcmSegments(id, [segment], {
            sampleRate,
            channels,
            audioLength,
          });
          writeFrame = segment.endFrame;
        }
      } catch (error) {
        reportSkippedDecodePacket(
          id,
          `Skipped failed MOSS Nano packet ${chunkIndex + 1}/${chunkCount}; inserted silence`,
          error,
        );
        writeFrame = Math.min(audioLength, writeFrame + Math.max(1, Math.round(tokenFrames * 3840)));
      } finally {
        disposeOrtTensorMap(feeds);
        disposeOrtTensorMap(outputs);
      }
      chunkIndex += 1;
      workerPerfEnd(id, null, { tokenStart, tokenFrames, actualChunkSeconds });
    }

    postDecodeProgress(id, {
      msg: "Preparing playback",
      progressPercent: 100,
    });
    workerPerfEnd(id, perfMark, {
      chunks: chunkCount,
      duration: audioLength / sampleRate,
      payloadCodec: MOSS_NANO_CODEC,
    });
    return {
      s16ChannelData: channelData,
      channels,
      sampleRate,
      audioLength,
      bundleName: MOSS_NANO_CODEC,
      audioFormat: "record_png_mossnano",
      payloadContainer: "MOSSNANO",
      payloadCodec: MOSS_NANO_CODEC,
    };
  } catch (error) {
    workerPerfEnd(id, perfMark, {
      error: workerErrorMessage(error),
    });
    throw error;
  } finally {
    stream?.free?.();
  }
}

// Inter-track silence (GAP entries, see PAYLOAD_CONTAINER_GAP in record-core)
// never reaches the EnCodec decoder — record-wasm strips those entries from
// the decodable payload and reports where they belonged via `silenceMap`
// (`{afterEntryIndex, sampleCount}`, ordered, one entry per excluded run).
// Splice zero-filled PCM back in at those positions now that decode is done,
// converting "after which chunk" into a sample offset via `samplesPerChunk`.
function spliceSilenceIntoChannelData(channelData, audioLength, samplesPerChunk, silenceMap) {
  const spans = (Array.isArray(silenceMap) ? silenceMap : [])
    .map((span) => ({
      afterEntryIndex: Math.max(0, Math.floor(Number(span?.afterEntryIndex) || 0)),
      sampleCount: Math.max(0, Math.floor(Number(span?.sampleCount) || 0)),
    }))
    .filter((span) => span.sampleCount > 0);
  if (!spans.length) {
    return { channelData, audioLength };
  }

  const totalSilenceSamples = spans.reduce((sum, span) => sum + span.sampleCount, 0);
  const splicedAudioLength = audioLength + totalSilenceSamples;
  const splicedChannelData = channelData.map((channel) => {
    const spliced = new Int16Array(splicedAudioLength); // zero-filled by default
    let sourceOffset = 0;
    let destOffset = 0;
    for (const span of spans) {
      const sampleOffset = Math.min(audioLength, span.afterEntryIndex * samplesPerChunk);
      const copyLength = Math.max(0, sampleOffset - sourceOffset);
      if (copyLength > 0) {
        spliced.set(channel.subarray(sourceOffset, sourceOffset + copyLength), destOffset);
        sourceOffset += copyLength;
        destOffset += copyLength;
      }
      destOffset += span.sampleCount;
    }
    if (sourceOffset < channel.length) {
      spliced.set(channel.subarray(sourceOffset), destOffset);
    }
    return spliced;
  });
  return { channelData: splicedChannelData, audioLength: splicedAudioLength };
}

async function decodePlayback({ id, frames, ecdcBuffer, bundleJson, bundleRoot, bundleName = "", meta, runtime, audioLength, record = null, cacheDecodedSegments = false, cachedPcmSegments = [], silenceMap = [], cache = null, recordBindingHex = "" }) {
  workerDecodeDebug("[play:client-playback-worker.js] decodePlayback", { id, ecdcBytes: ecdcBuffer?.byteLength || 0, bundleName, cacheDecodedSegments, silenceSpans: Array.isArray(silenceMap) ? silenceMap.length : 0 });
  if (!ecdcBuffer) {
    throw new Error("Client playback worker requires the ECDC round-trip payload.");
  }
  const perfMark = workerPerfStart("decode playback", {
    ecdcBytes: ecdcBuffer?.byteLength || 0,
    bundleName,
  });
  const ecdcBytes = ecdcBuffer instanceof Uint8Array ? ecdcBuffer : new Uint8Array(ecdcBuffer);
  let selectedBundleName = bundleName;
  let selectedBundleRoot = bundleRoot;
  let selectedBundleJson = bundleJson;
  let selectedMeta = meta;
  let session = null;
  let ort = null;

  try {
    if (!selectedBundleJson || !selectedBundleRoot || !selectedMeta) {
      const bundleMark = workerPerfStart("select encodec bundle");
      const metadata = (await ensureEncodecWasmModule()).ecdcMetadata(ecdcBytes);
      selectedBundleName = selectedBundleName || playerEncodeBundleNameFromEcdcMetadata(metadata, record);
      const bundle = await fetchEncodeBundle(selectedBundleName);
      selectedBundleRoot = bundle.bundleRoot;
      selectedBundleJson = bundle.bundleJson;
      selectedMeta = bundle.meta;
      workerPerfEnd(id, bundleMark, { bundleName: selectedBundleName });
    }
    const entropyMark = workerPerfStart("prepare interleaved ECDC decode", {
      bundleName: selectedBundleName,
    });
    const ecdcDecode = await prepareEcdcChunkDecode({
      ecdcBuffer: ecdcBytes,
      bundleJson: selectedBundleJson,
      bundleRoot: selectedBundleRoot,
      bundleName: selectedBundleName,
      meta: selectedMeta,
    });
    workerPerfEnd(id, entropyMark, {
      chunks: ecdcDecode.chunks?.length || 0,
    });
    const chunks = ecdcDecode.chunks || [];
    const encodedFrames = chunks.map((chunk) => frameDescriptorFromEcdcChunk(chunk, selectedMeta));
    if (!encodedFrames.length) {
      throw new Error("Client playback ECDC did not contain EnCodec frames.");
    }
    const safeAudioLength = Math.floor(Number(ecdcDecode.audioLength) || 0);
    if (!(safeAudioLength > 0)) {
      throw new Error("Client playback ECDC is missing PCM audio length.");
    }
    const sampleRate = Number(selectedMeta.sample_rate) || 48000;
    const seamRepairProfile = seamRepairProfileFromMeta(selectedMeta);
    const durationSeconds = safeAudioLength / sampleRate;
    const decodeModelPath = `${selectedBundleRoot}/${selectedMeta.decode_model}`;
    // The ONNX runtime + decode_frame.onnx weights (tens of MB) are only
    // fetched lazily, the first time a chunk actually misses the cache. A
    // fully cached record should never pay for that download at all — the
    // cache lookup below has to happen before this, not after it.
    let onnxRuntimeReady = false;
    const ensureDecodeSessionReady = async () => {
      if (onnxRuntimeReady) {
        return;
      }
      ort = await ensureOnnxRuntimeModule();
      const sessionMark = workerPerfStart("create ONNX decode session", {
        bundleName: selectedBundleName,
      });
      ({ session } = await createDecodeSession(
        decodeModelPath,
        runtime,
        bundleAssetMetadata(selectedMeta, selectedMeta.decode_model),
      ));
      workerPerfEnd(id, sessionMark);
      onnxRuntimeReady = true;
    };
    const layout = ecdcDecode.encodecModule.ecdcChunkLayoutFromMetadata(
      selectedBundleJson,
      ecdcBytes,
      encodedFrames.length,
    );
    const cacheEnabled = Boolean(cache?.enabled);
    const chunkCacheKeys = cacheEnabled
      ? chunks.map((chunk, index) => tryEcdcChunkCacheKey(chunk?.payload, index, recordBindingHex))
      : [];
    const cacheLookupStride = layout.stride;
    const cacheLookupFrameSamples = layout.samples;
    const cacheLookupEntries = chunkCacheKeys
      .map((key, index) => {
        if (!key) {
          return null;
        }
        const frameStart = index * cacheLookupStride;
        const frameSamples = Math.max(
          0,
          Math.min(
            cacheLookupFrameSamples,
            safeAudioLength - frameStart,
          ),
        );
        return {
          key,
          chunkIndex: index,
          meta: {
            chunkIndex: index,
            sampleRate,
            channels: Number(selectedMeta.channels) || 2,
            startFrame: frameStart,
            endFrame: frameStart + frameSamples,
          },
        };
      })
      .filter(Boolean);
    const prefetchedRemoteCacheResults = cacheEnabled && cacheLookupEntries.length
      ? await tryRequestCacheGetMany(id, cacheLookupEntries.map(({ key, meta }) => ({ key, meta })))
      : [];
    const prefetchedRemoteSegmentsByIndex = new Map();
    for (let index = 0; index < prefetchedRemoteCacheResults.length; index += 1) {
      const chunkIndex = Math.max(0, Math.floor(Number(cacheLookupEntries[index]?.chunkIndex) || 0));
      const rawSegment = cachedChannelBuffersToRawSegment(
        prefetchedRemoteCacheResults[index],
        chunkIndex,
        Number(selectedMeta.channels) || 2,
      );
      if (rawSegment) {
        prefetchedRemoteSegmentsByIndex.set(chunkIndex, rawSegment);
      }
    }
    if (seamRepairProfile) {
      const channels = Number(selectedMeta.channels) || 2;
      const ownedSamples = Math.max(1, seamRepairProfile.ownedSamples);
      const assembler = createHermiteChunkAssembler({
        channels,
        audioLength: safeAudioLength,
        ownedSamples,
        repairSamples: seamRepairProfile.repairSamples,
      });
      const cachedSegmentsByIndex = new Map();
      for (const segment of Array.isArray(cachedPcmSegments) ? cachedPcmSegments : []) {
        const chunkIndex = Math.max(0, Math.floor(Number(segment?.chunkIndex) || 0));
        if (segment?.pcmBuffer?.byteLength) {
          cachedSegmentsByIndex.set(chunkIndex, segment);
        }
      }
      const pcmMark = workerPerfStart("run guarded ECDC PCM decode", {
        frames: encodedFrames.length,
        bundleName: selectedBundleName,
        cachedChunks: cachedSegmentsByIndex.size,
      });
      for (let index = 0; index < chunks.length; index += 1) {
        const end = index + 1;
        const processedSeconds = Math.min(durationSeconds, durationSeconds * (end / Math.max(1, chunks.length)));
        postDecodeProgress(id, {
          msg: `Reading groove audio ${end}/${chunks.length}`,
          progressPercent: Math.round((end / chunks.length) * 100),
          processedSeconds: formatDecimal(processedSeconds, 2),
          duration: formatDecimal(durationSeconds, 2),
        });
        const cachedSegment = cachedSegmentsByIndex.get(index);
        let rawSegment = null;
        if (cachedSegment) {
          const startFrame = Math.max(0, Math.floor(Number(cachedSegment.startFrame) || (index * ownedSamples)));
          const endFrame = Math.max(startFrame, Math.min(
            safeAudioLength,
            Math.floor(Number(cachedSegment.endFrame) || (startFrame + ownedSamples)),
          ));
          const frameCount = endFrame - startFrame;
          rawSegment = {
            chunkIndex: index,
            startFrame,
            endFrame,
            channelData: deinterleaveS16Segment(new Int16Array(cachedSegment.pcmBuffer), channels, frameCount),
          };
        } else if (cacheEnabled) {
          rawSegment = prefetchedRemoteSegmentsByIndex.get(index) || null;
        }
        if (!rawSegment) {
          let frame = null;
          try {
            frame = await decodeLmChunk(
              ecdcDecode.encodecModule,
              ecdcDecode.lmWeights,
              selectedBundleJson,
              chunks[index],
              selectedMeta,
            );
          } catch (error) {
            reportSkippedDecodePacket(
              id,
              `Skipped damaged ECDC packet ${end}/${chunks.length}; inserted ${expectedFramePcmSamples(chunks[index], selectedMeta)} silent samples`,
              error,
            );
            frame = createSilentEncodedFrame(chunks[index], selectedMeta);
          }
          encodedFrames[index] = normalizeEncodedFrame(frame);
          if (chunks[index] && typeof chunks[index] === "object") {
            chunks[index].payload = null;
          }

          let feeds = null;
          let outputs = null;
          try {
            if (encodedFrames[index]?.silentPcm) {
              const startFrame = index * ownedSamples;
              const endFrame = Math.max(startFrame, Math.min(safeAudioLength, startFrame + ownedSamples));
              rawSegment = {
                chunkIndex: index,
                startFrame,
                endFrame,
                channelData: Array.from({ length: channels }, () => new Int16Array(endFrame - startFrame)),
              };
            } else {
              await ensureDecodeSessionReady();
              const decoderInputs = buildDecodeInputs(encodedFrames, selectedMeta, index, end);
              feeds = {
                [session.inputNames[0]]: new ort.Tensor("int64", decoderInputs.codes, [
                  decoderInputs.batchSize,
                  selectedMeta.num_codebooks,
                  decoderInputs.frameLength,
                ]),
                [session.inputNames[1]]: new ort.Tensor("float32", decoderInputs.scales, [decoderInputs.batchSize, 1]),
              };
              outputs = await session.run(feeds);
              const decodedTensor = findDecodeOutput(outputs);
              const decodedFloat = decodedTensor.data instanceof Float32Array
                ? decodedTensor.data
                : new Float32Array(decodedTensor.data);
              const decodedSamples = Math.floor(decodedFloat.length / Math.max(1, channels));
              if (decodedSamples < Math.max(1, Number(selectedMeta.segment_samples) || 0)) {
                throw new Error(`Guarded decoder returned ${decodedSamples} samples, expected at least ${selectedMeta.segment_samples}`);
              }
              rawSegment = cropDecodedOwnedSegmentToS16(
                ecdcDecode.encodecModule,
                selectedBundleJson,
                decodedFloat,
                selectedMeta,
                index,
                safeAudioLength,
              );
            }
            if (cacheDecodedSegments && rawSegment) {
              postRawDecodedPcmSegments(id, [rawSegment], {
                sampleRate,
                channels,
                audioLength: safeAudioLength,
              });
            }
          } catch (error) {
            reportSkippedDecodePacket(
              id,
              `Skipped failed EnCodec PCM packet ${end}/${encodedFrames.length}; inserted silence`,
              error,
            );
            const startFrame = index * ownedSamples;
            const endFrame = Math.max(startFrame, Math.min(safeAudioLength, startFrame + ownedSamples));
            rawSegment = {
              chunkIndex: index,
              startFrame,
              endFrame,
              channelData: Array.from({ length: channels }, () => new Int16Array(endFrame - startFrame)),
            };
          } finally {
            disposeOrtTensorMap(feeds);
            disposeOrtTensorMap(outputs);
            if (encodedFrames[index]) {
              encodedFrames[index].codes = null;
            }
          }
          if (cacheEnabled && rawSegment) {
            await tryRequestCachePut(id, chunkCacheKeys[index], {
              ...rawSegment,
              sampleRate,
            });
          }
        }
        const emittedSegments = assembler.addRawSegment(rawSegment);
        if (emittedSegments.length) {
          postDecodedPcmSegments(id, emittedSegments, {
            sampleRate,
            channels,
            audioLength: safeAudioLength,
          });
        }
      }
      workerPerfEnd(id, pcmMark, {
        decodedSamples: safeAudioLength,
      });
      postDecodeProgress(id, {
        msg: "Preparing playback",
        progressPercent: 100,
      });
      const s16Result = assembler.result();
      if (s16Result.flushed.length) {
        postDecodedPcmSegments(id, s16Result.flushed, {
          sampleRate,
          channels,
          audioLength: safeAudioLength,
        });
      }
      workerPerfEnd(id, perfMark, {
        bundleName: selectedBundleName,
        duration: durationSeconds,
      });
      return {
        s16ChannelData: s16Result.channelData,
        channels,
        sampleRate,
        audioLength: safeAudioLength,
        bundleName: selectedBundleName,
      };
    }
    const s16Writer = createS16OverlapAddWriter({
      meta: selectedMeta,
      frames: encodedFrames,
      audioLength: safeAudioLength,
      layout,
    });
    // Remote cache hits arrive as final PCM: splice them into the output and
    // run the LM/ONNX decode only for the chunks that missed.
    const cachedSegmentsByIndex = new Map();
    for (const segment of Array.isArray(cachedPcmSegments) ? cachedPcmSegments : []) {
      const chunkIndex = Math.max(0, Math.floor(Number(segment?.chunkIndex) || 0));
      if (segment?.pcmBuffer?.byteLength) {
        cachedSegmentsByIndex.set(chunkIndex, segment);
      }
    }
    if (cachedSegmentsByIndex.size) {
      workerDecodeInfo("[play:client-playback-worker.js] decodePlayback cached segments", {
        cachedChunks: cachedSegmentsByIndex.size,
        totalChunks: chunks.length,
        decodingChunks: Math.max(0, chunks.length - cachedSegmentsByIndex.size),
      });
    }
    const pcmMark = workerPerfStart("run interleaved ECDC PCM decode", {
      frames: encodedFrames.length,
      bundleName: selectedBundleName,
      cachedChunks: cachedSegmentsByIndex.size,
    });
    for (let index = 0; index < chunks.length; index += 1) {
      const end = index + 1;
      const cachedSegment = cachedSegmentsByIndex.get(index);
      const remoteCachedSegment = !cachedSegment && cacheEnabled
        ? (prefetchedRemoteSegmentsByIndex.get(index) || null)
        : null;
      const activeCachedSegment = cachedSegment || (remoteCachedSegment
        ? {
          chunkIndex: remoteCachedSegment.chunkIndex,
          startFrame: remoteCachedSegment.startFrame,
          endFrame: remoteCachedSegment.endFrame,
          pcmBuffer: interleaveS16Channels(remoteCachedSegment.channelData).buffer,
        }
        : null);
      if (activeCachedSegment) {
        const spliced = s16Writer.addCachedRange(
          activeCachedSegment.startFrame,
          activeCachedSegment.endFrame,
          new Int16Array(activeCachedSegment.pcmBuffer),
        );
        if (spliced) {
          if (chunks[index] && typeof chunks[index] === "object") {
            chunks[index].payload = null;
          }
          postDecodeProgress(id, {
            msg: `Using cached groove audio ${end}/${chunks.length}`,
            progressPercent: Math.round((end / chunks.length) * 100),
            processedSeconds: formatDecimal(Math.min(durationSeconds, durationSeconds * (end / Math.max(1, chunks.length))), 2),
            duration: formatDecimal(durationSeconds, 2),
          });
          const emittedSegments = s16Writer.emitAfterBatch(end);
          if (cacheDecodedSegments) {
            postRawDecodedPcmSegments(id, emittedSegments, {
              sampleRate,
              channels: Number(selectedMeta.channels) || 2,
              audioLength: safeAudioLength,
            });
          }
          postDecodedPcmSegments(id, emittedSegments, {
            sampleRate,
            channels: Number(selectedMeta.channels) || 2,
            audioLength: safeAudioLength,
          });
          if (encodedFrames[index]) {
            encodedFrames[index].codes = null;
          }
          continue;
        }
        workerDecodeWarn("[play:client-playback-worker.js] cached segment splice rejected; decoding chunk", { index });
      }
      workerDecodeDebug("[play:client-playback-worker.js] decodePlayback chunk", { index, totalChunks: chunks.length, offset: chunks[index]?.offset, frameLength: chunks[index]?.frameLength, bundleName: selectedBundleName });
      const processedSeconds = Math.min(durationSeconds, durationSeconds * (end / Math.max(1, chunks.length)));
      postDecodeProgress(id, {
        msg: `Reading groove audio ${end}/${chunks.length}`,
        progressPercent: Math.round((end / chunks.length) * 100),
        processedSeconds: formatDecimal(processedSeconds, 2),
        duration: formatDecimal(durationSeconds, 2),
      });

      let frame = null;
      try {
        frame = await decodeLmChunk(
          ecdcDecode.encodecModule,
          ecdcDecode.lmWeights,
          selectedBundleJson,
          chunks[index],
          selectedMeta,
        );
      } catch (error) {
        reportSkippedDecodePacket(
          id,
          `Skipped damaged ECDC packet ${end}/${chunks.length}; inserted ${expectedFramePcmSamples(chunks[index], selectedMeta)} silent samples`,
          error,
        );
        frame = createSilentEncodedFrame(chunks[index], selectedMeta);
      }
      encodedFrames[index] = normalizeEncodedFrame(frame);
      // The entropy-coded payload is consumed once expanded to codes; drop it
      // so the full record's packets are not retained for the whole decode.
      if (chunks[index] && typeof chunks[index] === "object") {
        chunks[index].payload = null;
      }

      let feeds = null;
      let outputs = null;
      try {
        if (encodedFrames[index]?.silentPcm) {
          s16Writer.addSilentFrame(index);
        } else {
          await ensureDecodeSessionReady();
          const decoderInputs = buildDecodeInputs(encodedFrames, selectedMeta, index, end);
          feeds = {
            [session.inputNames[0]]: new ort.Tensor("int64", decoderInputs.codes, [
              decoderInputs.batchSize,
              selectedMeta.num_codebooks,
              decoderInputs.frameLength,
            ]),
            [session.inputNames[1]]: new ort.Tensor("float32", decoderInputs.scales, [decoderInputs.batchSize, 1]),
          };
          outputs = await session.run(feeds);
          const decodedTensor = findDecodeOutput(outputs);
          s16Writer.addDecodedBatch(index, end, decodedTensor.data);
        }
        const emittedSegments = s16Writer.emitAfterBatch(end);
        if (cacheDecodedSegments) {
          postRawDecodedPcmSegments(id, emittedSegments, {
            sampleRate,
            channels: Number(selectedMeta.channels) || 2,
            audioLength: safeAudioLength,
          });
        }
        if (cacheEnabled) {
          for (const segment of emittedSegments) {
            await tryRequestCachePut(id, chunkCacheKeys[segment.chunkIndex], {
              ...segment,
              sampleRate,
            });
          }
        }
        postDecodedPcmSegments(id, emittedSegments, {
          sampleRate,
          channels: Number(selectedMeta.channels) || 2,
          audioLength: safeAudioLength,
        });
      } catch (error) {
        reportSkippedDecodePacket(
          id,
          `Skipped failed EnCodec PCM packet ${end}/${encodedFrames.length}; inserted silence`,
          error,
        );
        s16Writer.addSilentFrame(index);
        const emittedSegments = s16Writer.emitAfterBatch(end);
        if (cacheDecodedSegments) {
          postRawDecodedPcmSegments(id, emittedSegments, {
            sampleRate,
            channels: Number(selectedMeta.channels) || 2,
            audioLength: safeAudioLength,
          });
        }
        if (cacheEnabled) {
          for (const segment of emittedSegments) {
            await tryRequestCachePut(id, chunkCacheKeys[segment.chunkIndex], {
              ...segment,
              sampleRate,
            });
          }
        }
        postDecodedPcmSegments(id, emittedSegments, {
          sampleRate,
          channels: Number(selectedMeta.channels) || 2,
          audioLength: safeAudioLength,
        });
      } finally {
        disposeOrtTensorMap(feeds);
        disposeOrtTensorMap(outputs);
        // Codes were consumed by the decoder above; release them so peak
        // memory stays bounded by the batch instead of the whole record.
        if (encodedFrames[index]) {
          encodedFrames[index].codes = null;
        }
      }
    }
    workerPerfEnd(id, pcmMark, {
      decodedSamples: safeAudioLength,
    });

    postDecodeProgress(id, {
      msg: "Preparing playback",
      progressPercent: 100,
    });
    const flushedSegments = s16Writer.flush();
    if (cacheDecodedSegments) {
      postRawDecodedPcmSegments(id, flushedSegments, {
        sampleRate,
        channels: Number(selectedMeta.channels) || 2,
        audioLength: safeAudioLength,
      });
    }
    if (cacheEnabled) {
      for (const segment of flushedSegments) {
        await tryRequestCachePut(id, chunkCacheKeys[segment.chunkIndex], {
          ...segment,
          sampleRate,
        });
      }
    }
    postDecodedPcmSegments(id, flushedSegments, {
      sampleRate,
      channels: Number(selectedMeta.channels) || 2,
      audioLength: safeAudioLength,
    });
    const s16Result = s16Writer.result();
    const samplesPerChunk = Math.max(
      1,
      Math.floor(Number(selectedMeta?.owned_samples ?? selectedMeta?.ownedSamples) || 0)
      || Math.round(safeAudioLength / Math.max(1, encodedFrames.length)),
    );
    const { channelData: splicedChannelData, audioLength: splicedAudioLength } =
      spliceSilenceIntoChannelData(s16Result.channelData, safeAudioLength, samplesPerChunk, silenceMap);

    workerPerfEnd(id, perfMark, {
      bundleName: selectedBundleName,
      duration: durationSeconds,
    });
    return {
      s16ChannelData: splicedChannelData,
      channels: Number(selectedMeta.channels) || 2,
      sampleRate,
      audioLength: splicedAudioLength,
      bundleName: selectedBundleName,
    };
  } catch (error) {
    workerPerfEnd(id, perfMark, {
      error: workerErrorMessage(error),
    });
    throw error;
  } finally {
    await session?.release?.();
  }
}

async function decodeRecordPngPlayback({
  id,
  pngBytes,
  record = {},
  recordProfile,
  runtime,
  cacheDecodedSegments = false,
  cachedPcmSegments = [],
  silenceMap = [],
  cache = null,
  recordBindingHex = "",
}) {
  workerDecodeDebug("[play:client-playback-worker.js] decodeRecordPngPlayback", {
    id,
    pngBytes: pngBytes?.byteLength || 0,
    recordProfile,
    cacheDecodedSegments,
    cacheEnabled: Boolean(cache?.enabled),
    cachedPcmSegments: Array.isArray(cachedPcmSegments) ? cachedPcmSegments.length : 0,
  });
  const perfMark = workerPerfStart("decode record PNG end-to-end", {
    pngBytes: pngBytes?.byteLength || 0,
  });
  try {
    const extracted = await extractPayloadFromRecordPng({ id, pngBytes, record, recordProfile });
    const decoded = await decodePayloadPlayback({
      id,
      payloadBuffer: extracted.payload,
      payloadContainer: extracted.payloadContainer,
      payloadCodec: extracted.payloadCodec,
      entryContainer: extracted.entryContainer,
      bundleName: extracted.bundleName,
      record,
      recordProfile: extracted.recordProfile,
      runtime,
      silenceMap: Array.isArray(silenceMap) && silenceMap.length ? silenceMap : extracted.silenceMap,
      cacheDecodedSegments,
      cachedPcmSegments,
      cache,
      recordBindingHex,
    });
    workerPerfEnd(id, perfMark, {
      bundleName: decoded.bundleName,
      recordProfile: extracted.recordProfile,
    });
    return {
      ...decoded,
      recordProfile: extracted.recordProfile,
      recordHeaderProof: extracted.recordHeaderProof || null,
      releaseId: extracted.releaseId || "",
    };
  } catch (error) {
    workerPerfEnd(id, perfMark, {
      error: workerErrorMessage(error),
    });
    throw error;
  }
}

function decodeTrackMetadataFromRecord(record = {}) {
  const meta = record?.meta && typeof record.meta === "object" ? record.meta : {};
  const payloadMetadata = record?.payloadMetadata && typeof record.payloadMetadata === "object"
    ? record.payloadMetadata
    : {};
  return resolvePlaybackPayloadMetadata({
    headerMetadata: {
      payloadContainer: record?.payloadContainer || meta.payloadContainer || "",
      entryContainer: record?.entryContainer || meta.entryContainer || "",
      trackListing: Array.isArray(record?.trackListing)
        ? record.trackListing
        : (Array.isArray(meta.trackListing) ? meta.trackListing : []),
      dummySpiralRegions: Array.isArray(record?.dummySpiralRegions)
        ? record.dummySpiralRegions
        : (Array.isArray(meta.dummySpiralRegions) ? meta.dummySpiralRegions : []),
    },
    payloadMetadata,
  });
}

async function decodePayloadPlayback({ payloadBuffer, payloadContainer = "", payloadCodec = "", entryContainer = "", recordProfile, ...rest }) {
  workerDecodeDebug("[play:client-playback-worker.js] decodePayloadPlayback", { payloadBytes: payloadBuffer?.byteLength || 0, payloadContainer, payloadCodec, entryContainer, recordProfile });
  await ensurePlayerAppWasmModule();
  payloadContainer = normalizePayloadContainer(payloadContainer);
  if (!payloadContainer) {
    throw new Error("payloadContainer is required.");
  }
  const decorate = (result) => {
    const metadata = decodeTrackMetadataFromRecord({
      ...(rest.record || {}),
      payloadContainer,
      entryContainer,
      payloadMetadata: rest.payloadMetadata || rest.record?.payloadMetadata || {},
      trackListing: Array.isArray(rest.trackListing) ? rest.trackListing : rest.record?.trackListing,
      dummySpiralRegions: Array.isArray(rest.dummySpiralRegions)
        ? rest.dummySpiralRegions
        : rest.record?.dummySpiralRegions,
    });
    return {
      ...result,
      payloadContainer: result.payloadContainer || metadata.payloadContainer,
      entryContainer: result.entryContainer || metadata.entryContainer,
      trackListing: metadata.trackListing,
      dummySpiralRegions: metadata.dummySpiralRegions,
    };
  };
  if (payloadContainerIsMossNano(payloadContainer)) {
    return decorate(await decodeMossNanoPlayback({
      ...rest,
      payloadBuffer,
      recordProfile,
    }));
  }
  return decorate(await decodePlayback({
    ...rest,
    ecdcBuffer: payloadBuffer,
  }));
}

function postWorkerDecodeResult(id, result) {
  const s16ChannelBuffers = (result.s16ChannelData || []).map((channel) => channel.buffer);
  const transfer = s16ChannelBuffers;
  self.postMessage(
    {
      id,
      ok: true,
      result: {
        s16ChannelBuffers,
        channels: result.channels,
        sampleRate: result.sampleRate,
        audioLength: result.audioLength,
        bundleName: result.bundleName,
        audioFormat: result.audioFormat || "record_png_payload",
        payloadContainer: result.payloadContainer || "",
        payloadCodec: result.payloadCodec || "",
        entryContainer: result.entryContainer || "",
        recordProfile: result.recordProfile,
        recordHeaderProof: result.recordHeaderProof || null,
        releaseId: result.releaseId || "",
        trackListing: Array.isArray(result.trackListing) ? result.trackListing : [],
        dummySpiralRegions: Array.isArray(result.dummySpiralRegions) ? result.dummySpiralRegions : [],
      },
    },
    transfer,
  );
}

self.onmessage = async (event) => {
  if (event.data?.type === "set-logging") { globalThis.VinylPlayerMessageLogger.setEnabled(event.data.enabled); playerMessageLog.action("logging-changed", { enabled: event.data.enabled }); return; }
  if (
    event.data?.type === "cache-get-result" ||
    event.data?.type === "cache-get-many-result" ||
    event.data?.type === "cache-put-result"
  ) {
    handleCacheResponseMessage(event.data);
    return;
  }
  playerMessageLog.receive(event.data?.type || "message", event.data);
  const { id, type } = event.data || {};
  if (type === "init") {
    self.postMessage({ id, ok: true, result: { crossOriginIsolated: self.crossOriginIsolated === true } });
    return;
  }

  try {
    if (type === "inspect-record-png") {
      workerDecodeDebug("[play:client-playback-worker.js] onmessage inspect-record-png", { id, pngBytes: event.data?.pngBytes?.byteLength || 0, recordProfile: event.data?.recordProfile });
      const result = await extractPayloadFromRecordPng(event.data);
      const payloadBuffer = result.payload.buffer.slice(result.payload.byteOffset, result.payload.byteOffset + result.payload.byteLength);
      const chunkStreamBuffer = result.chunkStream?.byteLength
        ? result.chunkStream.buffer.slice(result.chunkStream.byteOffset, result.chunkStream.byteOffset + result.chunkStream.byteLength)
        : null;
      const ecdcBuffer = result.ecdc
        ? result.ecdc.buffer.slice(result.ecdc.byteOffset, result.ecdc.byteOffset + result.ecdc.byteLength)
        : null;
      const transfer = [payloadBuffer];
      if (chunkStreamBuffer) transfer.push(chunkStreamBuffer);
      if (ecdcBuffer) transfer.push(ecdcBuffer);
      self.postMessage({
        id,
        ok: true,
        result: {
          payloadBuffer,
          chunkStreamBuffer,
          payloadContainer: result.payloadContainer,
          payloadCodec: result.payloadCodec,
          entryContainer: result.entryContainer || "",
          payloadMetadataJson: JSON.stringify(result.payloadMetadata || {}),
          trackListingJson: JSON.stringify(result.trackListing || []),
          dummySpiralRegionsJson: JSON.stringify(result.dummySpiralRegions || []),
          silenceMapJson: JSON.stringify(result.silenceMap || []),
          programmeMapJson: result.programmeMap ? JSON.stringify(result.programmeMap) : "",
          ecdcBuffer,
          bundleName: result.bundleName,
          recordProfile: result.recordProfile,
          recordHeaderProof: result.recordHeaderProof || null,
          releaseId: result.releaseId || "",
        },
      }, transfer);
      return;
    }
    if (type === "decode-record-png") {
      workerDecodeDebug("[play:client-playback-worker.js] onmessage decode-record-png", { id, pngBytes: event.data?.pngBytes?.byteLength || 0, recordProfile: event.data?.recordProfile });
      const result = await decodeRecordPngPlayback(event.data);
      postWorkerDecodeResult(id, result);
      return;
    }
    if (type === "decode-payload") {
      workerDecodeDebug("[play:client-playback-worker.js] onmessage decode-payload", { id, payloadBytes: event.data?.payloadBuffer?.byteLength || 0, payloadContainer: event.data?.payloadContainer, entryContainer: event.data?.entryContainer });
      const result = await decodePayloadPlayback(event.data);
      postWorkerDecodeResult(id, result);
      return;
    }
    if (type === "decode" || type === "decode-ecdc") {
      workerDecodeDebug("[play:client-playback-worker.js] onmessage decode-ecdc", { id, ecdcBytes: event.data?.ecdcBuffer?.byteLength || 0, bundleName: event.data?.bundleName || "" });
      const result = await decodePlayback(event.data);
      postWorkerDecodeResult(id, result);
      return;
    }
    throw new Error(`Unknown playback worker action: ${type || "missing"}`);
  } catch (error) {
    self.postMessage({
      id,
      ok: false,
      error: error?.message || String(error),
    });
  }
};
