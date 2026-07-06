import { RecordDecoderClient } from "./record-decoder-client.js";

const DEFAULT_REMOTE_API_BASE_URL = "https://yl.vin/api/play/tape";
const DEFAULT_REMOTE_CACHE_FORMATS = Object.freeze([
  "opus",
  "opus_packet",
  "opus_packets",
  "soundkit_opus_packet",
  "soundkit_opus_packets",
]);

let sharedModulesPromise = null;
let playerWasmModulePromise = null;
let playerWasmOpusModulePromise = null;

function versionedAssetUrl(path) {
  const url = new URL(path, globalThis.location?.href || import.meta.url);
  const version = new URL(globalThis.location?.href || import.meta.url).searchParams.get("v");
  if (version) {
    url.searchParams.set("v", version);
  }
  return url.toString();
}

async function loadScriptOnce(path, globalName) {
  if (globalThis[globalName]) {
    return globalThis[globalName];
  }
  await import(/* @vite-ignore */ versionedAssetUrl(path));
  if (!globalThis[globalName]) {
    throw new Error(`Expected ${globalName} after loading ${path}.`);
  }
  return globalThis[globalName];
}

async function ensureSharedModules() {
  if (!sharedModulesPromise) {
    sharedModulesPromise = (async () => {
      await loadScriptOnce("./player-cache-config.js", "BitneedlePlayerCacheConfig");
      await loadScriptOnce("./player-cache.js", "BitneedlePlayerCache");
      await loadScriptOnce("./player-pcm-helpers.js", "BitneedlePlayerPcmHelpers");
      return {
        cache: globalThis.BitneedlePlayerCache,
        cacheConfig: globalThis.BitneedlePlayerCacheConfig,
        pcmHelpers: globalThis.BitneedlePlayerPcmHelpers,
      };
    })();
  }
  return sharedModulesPromise;
}

async function ensurePlayerWasmModule() {
  if (!playerWasmModulePromise) {
    playerWasmModulePromise = import("./wasm/player-wasm/player_wasm.js").then(async (module) => {
      if (typeof module.default === "function") {
        await module.default();
      }
      return module;
    });
  }
  return playerWasmModulePromise;
}

async function ensurePlayerWasmOpusModule() {
  if (!playerWasmOpusModulePromise) {
    playerWasmOpusModulePromise = ensurePlayerWasmModule().then((module) => {
      if (typeof module?.OpusEncoder !== "function") {
        throw new Error("player-wasm OpusEncoder export is missing.");
      }
      if (typeof module?.OpusDecoder !== "function") {
        throw new Error("player-wasm OpusDecoder export is missing.");
      }
      return {
        Encoder: module.OpusEncoder,
        Decoder: module.OpusDecoder,
      };
    });
  }
  return playerWasmOpusModulePromise;
}

function uint8View(value) {
  if (value instanceof Uint8Array) return value;
  if (ArrayBuffer.isView(value)) return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  return new Uint8Array(value || 0);
}

function concatenateUint8Chunks(chunks, totalLength = null) {
  const list = (Array.isArray(chunks) ? chunks : []).map(uint8View).filter(chunk => chunk.byteLength > 0);
  const length = totalLength == null ? list.reduce((sum, chunk) => sum + chunk.byteLength, 0) : totalLength;
  const output = new Uint8Array(Math.max(0, length));
  let offset = 0;
  for (const chunk of list) {
    output.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return output;
}

function encodeUint8ArrayBase64(bytes) {
  const view = uint8View(bytes);
  let binary = "";
  for (let index = 0; index < view.length; index += 1) {
    binary += String.fromCharCode(view[index]);
  }
  return btoa(binary);
}

function encodeUint8ArrayBase64Url(bytes) {
  return encodeUint8ArrayBase64(bytes).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
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

async function sha256Hex(bytes) {
  const digest = await crypto.subtle.digest("SHA-256", uint8View(bytes));
  return Array.from(new Uint8Array(digest), value => value.toString(16).padStart(2, "0")).join("");
}

function cloneRecordHeaderProof(proof) {
  return proof && typeof proof === "object" ? JSON.parse(JSON.stringify(proof)) : null;
}

function cloneArrayBuffer(buffer) {
  if (buffer instanceof ArrayBuffer) {
    return buffer.slice(0);
  }
  if (ArrayBuffer.isView(buffer)) {
    return buffer.buffer.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength);
  }
  return null;
}

function normalizeSegmentMeta(meta = {}) {
  const channels = Math.max(1, Math.floor(Number(meta.channels) || 2));
  const startFrame = Math.max(0, Math.floor(Number(meta.startFrame) || 0));
  const endFrame = Math.max(startFrame, Math.floor(Number(meta.endFrame) || startFrame));
  return {
    chunkIndex: Math.max(0, Math.floor(Number(meta.chunkIndex) || 0)),
    startFrame,
    endFrame,
    channels,
    sampleRate: Math.max(1, Math.floor(Number(meta.sampleRate) || 48000)),
    bytesPerFrame: channels * 2,
    bitsPerSample: 16,
    audioFormat: "soundkit_opus_packets",
    audioLength: endFrame,
  };
}

function createSoundkitPacketHelpers(playerWasm) {
  if (typeof playerWasm?.buildSoundkitFrameHeader !== "function") {
    throw new Error("player-wasm buildSoundkitFrameHeader export is missing.");
  }
  if (typeof playerWasm?.decodeSoundkitFrameHeader !== "function") {
    throw new Error("player-wasm decodeSoundkitFrameHeader export is missing.");
  }
  return {
    buildSoundkitFrameHeader(options = {}) {
      return uint8View(playerWasm.buildSoundkitFrameHeader(options));
    },
    soundkitOpusPacketItemsFromPackets(packets) {
      const items = [];
      for (const packetSource of Array.isArray(packets) ? packets : []) {
        const source = uint8View(packetSource);
        let offset = 0;
        while (offset < source.byteLength) {
          const remaining = source.subarray(offset);
          const header = playerWasm.decodeSoundkitFrameHeader(remaining);
          if (Number(header?.encoding) !== 2) {
            throw new Error("SoundKit packet is not Opus.");
          }
          const headerSize = Math.max(0, Math.floor(Number(header?.headerSize) || 0));
          const payloadSize = Math.max(0, Math.floor(Number(header?.payloadSize) || 0));
          const packetSize = headerSize + payloadSize;
          if (!(headerSize > 0) || !(payloadSize > 0)) {
            throw new Error("SoundKit Opus packet has invalid dimensions.");
          }
          if (packetSize > remaining.byteLength) {
            throw new Error(
              `Truncated SoundKit Opus packet at byte ${offset}: need ${packetSize}, have ${remaining.byteLength}.`,
            );
          }
          items.push({
            header,
            payload: source.slice(offset + headerSize, offset + packetSize),
          });
          offset += packetSize;
        }
      }
      return items;
    },
  };
}

function createRecordContextStore(initial = {}) {
  let context = {
    descriptorJson: String(initial.descriptorJson || "").trim(),
    recordHeaderProof: cloneRecordHeaderProof(initial.recordHeaderProof),
    recordProfile: String(initial.recordProfile || "").trim(),
  };
  return {
    get() {
      return {
        descriptorJson: context.descriptorJson,
        recordHeaderProof: cloneRecordHeaderProof(context.recordHeaderProof),
        recordProfile: context.recordProfile,
      };
    },
    set(next = {}) {
      context = {
        descriptorJson: String(next.descriptorJson || context.descriptorJson || "").trim(),
        recordHeaderProof: cloneRecordHeaderProof(next.recordHeaderProof ?? context.recordHeaderProof),
        recordProfile: String(next.recordProfile || context.recordProfile || "").trim(),
      };
      return this.get();
    },
  };
}

async function createOpusRuntime(config = {}) {
  const { cache, cacheConfig, pcmHelpers } = await ensureSharedModules();
  const playerWasm = await ensurePlayerWasmModule();
  const soundkitPacketHelpers = createSoundkitPacketHelpers(playerWasm);
  const runtimeContext = createRecordContextStore(config.recordContext || {});

  const cacheClient = cache.createPlayerCache({
    generationCacheVersion: String(config.generationCacheVersion || "vin-yl-player"),
    decodeSegmentCacheStoreName: String(config.storeName || "opus-chunks"),
    waveformCacheStoreName: "waveform-peaks",
    decodeSegmentRemoteCacheFormats: new Set(config.acceptedFormats || DEFAULT_REMOTE_CACHE_FORMATS),
    remoteCacheApiBaseUrl: String(config.apiBaseUrl || DEFAULT_REMOTE_API_BASE_URL),
    remoteCacheStreamContentType: String(
      config.remoteCacheStreamContentType
      || cacheConfig.PLAYER_REMOTE_CACHE_STREAM_CONTENT_TYPE
      || "application/vnd.bitneedle.player-cache-stream+binary",
    ),
    remoteCacheBatchFormat: String(
      config.remoteCacheBatchFormat
      || cacheConfig.PLAYER_REMOTE_CACHE_BATCH_FORMAT
      || "bitneedle-player-cache-batch-v1",
    ),
    remoteCacheBatchSize: Math.max(1, Math.floor(Number(
      config.remoteCacheBatchSize
      || cacheConfig.PLAYER_REMOTE_CACHE_BATCH_SIZE
      || 512,
    ))),
    remoteCacheBatchWriteSize: Math.max(1, Math.floor(Number(
      config.remoteCacheBatchWriteSize
      || cacheConfig.PLAYER_REMOTE_CACHE_BATCH_WRITE_SIZE
      || 32,
    ))),
    remoteCacheBatchRetryDelaysMs: Array.isArray(config.remoteCacheBatchRetryDelaysMs)
      ? config.remoteCacheBatchRetryDelaysMs
      : cacheConfig.PLAYER_REMOTE_CACHE_BATCH_RETRY_DELAYS_MS,
    disableAllCaching: false,
    localCacheDbName: String(config.localCacheDbName || "bitneedle-player-cache"),
    playerFetch: (url, options) => fetch(url, options),
    delay: (ms) => new Promise(resolve => setTimeout(resolve, Math.max(0, ms))),
    decodeBase64ToUint8Array,
    encodeUint8ArrayBase64,
    encodeUint8ArrayBase64Url,
    uint8View,
    setStatus: () => {},
    getPlayerAppWasmModule: () => playerWasm,
  });

  const helpers = pcmHelpers.createPlayerPcmHelpers({
    clamp: (value, min, max) => Math.max(min, Math.min(max, value)),
    getScratchAudioContext: () => null,
    ensureLibopusModule: ensurePlayerWasmOpusModule,
    buildSoundkitFrameHeader: soundkitPacketHelpers.buildSoundkitFrameHeader,
    uint8View,
    yieldToMainThread: () => Promise.resolve(),
    assertPlayerRuntimeActive: () => {},
    concatenateUint8Chunks,
    soundkitOpusPacketItemsFromPackets: soundkitPacketHelpers.soundkitOpusPacketItemsFromPackets,
    decodeBase64ToUint8Array,
  });

  return {
    cacheClient,
    helpers,
    playerWasm,
    runtimeContext,
    apiBaseUrl: String(config.apiBaseUrl || DEFAULT_REMOTE_API_BASE_URL),
    storeName: String(config.storeName || "opus-chunks"),
    maxEntries: Math.max(1, Math.floor(Number(config.maxEntries) || 512)),
  };
}

export async function decodeRecordDescriptorJson(pngBytes, recordProfile = "") {
  const playerWasm = await ensurePlayerWasmModule();
  return String(
    playerWasm.decodeRecordDescriptorHeaderJson(
      uint8View(pngBytes),
      recordProfile ? String(recordProfile) : undefined,
    ) || "",
  ).trim();
}

function remoteCacheEncryptionContext(key, normalized) {
  return {
    protocolVersion: 1,
    cacheFormatVersion: 1,
    cacheStoreName: "opus-chunks",
    cacheKey: String(key || ""),
    chunkIndex: normalized.chunkIndex,
    packetOffset: normalized.startFrame,
    plaintextLength: Math.max(0, (normalized.endFrame - normalized.startFrame) * normalized.bytesPerFrame),
    codecIdentifier: normalized.audioFormat,
  };
}

function payloadToPcmResult(runtime, payload, normalized) {
  const packetSources = runtime.cacheClient.playerDecodedSegmentPacketSources(payload);
  const packets = packetSources.length
    ? packetSources
    : [runtime.cacheClient.playerDecodedSegmentPacketBytes(payload)].filter(
      (packetBytes) => packetBytes?.byteLength > 0,
    );
  return runtime.helpers.decodeSoundkitOpusPacketsToPcmBytes({
    sampleRate: normalized.sampleRate,
    channels: normalized.channels,
    bitsPerSample: normalized.bitsPerSample,
    bytesPerFrame: normalized.bytesPerFrame,
    startFrame: normalized.startFrame,
    endFrame: normalized.endFrame,
  }, packets).then(pcmBytes => {
    const declaredFrameCount = Math.max(0, normalized.endFrame - normalized.startFrame);
    if (pcmBytes.byteLength % normalized.bytesPerFrame !== 0) {
      throw new Error(
        `Cached Opus chunk ${normalized.chunkIndex} produced ${pcmBytes.byteLength} PCM bytes, not divisible by bytesPerFrame ${normalized.bytesPerFrame}.`,
      );
    }
    const decodedFrameCount = pcmBytes.byteLength / normalized.bytesPerFrame;
    if (decodedFrameCount !== declaredFrameCount) {
      throw new Error(
        `Cached Opus chunk ${normalized.chunkIndex} decoded ${decodedFrameCount} frames but declared ${declaredFrameCount}.`,
      );
    }
    const provider = runtime.helpers.createS16PcmWindowProviderFromPcmBytes({
      pcmBytes,
      frameCount: decodedFrameCount,
      sampleRate: normalized.sampleRate,
      channels: normalized.channels,
      bitsPerSample: normalized.bitsPerSample,
      audioFormat: normalized.audioFormat,
    });
    const channelBuffers = provider.channelData.map(channel => channel.buffer.slice(channel.byteOffset, channel.byteOffset + channel.byteLength));
    if (!channelBuffers.every((buffer) => buffer.byteLength / 2 === decodedFrameCount)) {
      throw new Error(`Cached Opus chunk ${normalized.chunkIndex} produced mismatched channel lengths.`);
    }
    return {
      chunkIndex: normalized.chunkIndex,
      startFrame: normalized.startFrame,
      endFrame: normalized.startFrame + decodedFrameCount,
      sampleRate: normalized.sampleRate,
      channels: normalized.channels,
      channelBuffers,
    };
  });
}

async function readLocalCachedSegment(runtime, recordContext, key, normalized) {
  const payload = await runtime.cacheClient.readPlayerStageCache(runtime.storeName, key, {
    cacheKey: key,
    recordDescriptorJson: recordContext.descriptorJson,
    recordHeaderProof: recordContext.recordHeaderProof
      ? {
        ...recordContext.recordHeaderProof,
        chunkIndex: normalized.chunkIndex,
      }
      : null,
    chunkIndex: normalized.chunkIndex,
    packetOffset: normalized.startFrame,
    audioFormat: normalized.audioFormat,
    skipRemote: true,
  });
  if (!runtime.cacheClient.isPlayerDecodedSegmentOpusCachePayload(payload)) {
    return null;
  }
  return payloadToPcmResult(runtime, payload, normalized);
}

async function readRemoteCachedSegment(runtime, recordContext, key, normalized) {
  const payload = await runtime.cacheClient.readPlayerStageCache(runtime.storeName, key, {
    cacheKey: key,
    recordDescriptorJson: recordContext.descriptorJson,
    recordHeaderProof: recordContext.recordHeaderProof
      ? {
        ...recordContext.recordHeaderProof,
        chunkIndex: normalized.chunkIndex,
      }
      : null,
    chunkIndex: normalized.chunkIndex,
    packetOffset: normalized.startFrame,
    audioFormat: normalized.audioFormat,
  });
  if (!runtime.cacheClient.isPlayerDecodedSegmentOpusCachePayload(payload)) {
    return null;
  }
  return payloadToPcmResult(runtime, payload, normalized);
}

async function batchLookupRemoteSegments(runtime, recordContext, entries) {
  const requestEntries = entries.map(entry => ({
    key: String(entry?.key || ""),
    normalized: normalizeSegmentMeta(entry?.meta || {}),
  })).filter(entry => entry.key);
  if (!requestEntries.length) {
    return [];
  }
  const batchUrl = `${runtime.apiBaseUrl.replace(/\/+$/, "")}/batch`;
  const response = await fetch(batchUrl, {
    method: "POST",
    mode: "cors",
    credentials: "omit",
    cache: "no-store",
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      format: "bitneedle-player-cache-batch-v1",
      keys: requestEntries.map(entry => entry.key),
    }),
  });
  if (!response.ok) {
    console.warn("[vin.yl.player] tape fetch batch:failed", {
      url: batchUrl,
      status: response.status,
      entries: requestEntries.length,
    });
    throw new Error(`Remote cache batch lookup failed: ${response.status}`);
  }
  const payload = await response.json();
  const keyResults = Array.isArray(payload?.results) ? payload.results : [];
  const lookupByKey = new Map(keyResults.map(result => [String(result?.key || ""), result]));
  return Promise.all(requestEntries.map(async entry => {
    try {
      return await lookupRemoteSegment(entry);
    } catch (error) {
      console.warn("[vin.yl.player] tape fetch hit:malformed", {
        key: entry.key,
        chunkIndex: entry.normalized.chunkIndex,
        error: error instanceof Error ? error.message : String(error),
      });
      return null;
    }
  }));

  async function lookupRemoteSegment(entry) {
    const hit = lookupByKey.get(entry.key);
    if (!hit?.hit || !recordContext.descriptorJson) {
      console.info("[vin.yl.player] tape fetch miss", {
        key: entry.key,
        chunkIndex: entry.normalized.chunkIndex,
        startFrame: entry.normalized.startFrame,
      });
      return null;
    }
    if (!hit?.directGetUrl) {
      console.info("[vin.yl.player] tape fetch hit:worker-payload", {
        key: entry.key,
        chunkIndex: entry.normalized.chunkIndex,
        startFrame: entry.normalized.startFrame,
      });
      return readRemoteCachedSegment(runtime, recordContext, entry.key, entry.normalized);
    }
    console.info("[vin.yl.player] tape fetch hit:direct-get:start", {
      key: entry.key,
      chunkIndex: entry.normalized.chunkIndex,
      startFrame: entry.normalized.startFrame,
      url: hit.directGetUrl,
    });
    const objectResponse = await fetch(hit.directGetUrl, {
      method: "GET",
      mode: "cors",
      credentials: "omit",
      cache: "no-store",
    });
    if (!objectResponse.ok) {
      console.warn("[vin.yl.player] tape fetch hit:direct-get:failed", {
        key: entry.key,
        status: objectResponse.status,
        url: hit.directGetUrl,
      });
      return readRemoteCachedSegment(runtime, recordContext, entry.key, entry.normalized);
    }
    const encryptedBytes = new Uint8Array(await objectResponse.arrayBuffer());
    console.info("[vin.yl.player] tape fetch hit:direct-get:ok", {
      key: entry.key,
      status: objectResponse.status,
      encryptedBytes: encryptedBytes.byteLength,
      url: hit.directGetUrl,
    });
    const plaintext = runtime.playerWasm.decryptCacheEntry(
      recordContext.descriptorJson,
      JSON.stringify(remoteCacheEncryptionContext(entry.key, entry.normalized)),
      encryptedBytes,
    );
    const packetBytes = uint8View(plaintext);
    if (!packetBytes.byteLength) {
      return null;
    }
    const decodedPayload = {
      key: entry.key,
      audioFormat: entry.normalized.audioFormat,
      chunkIndex: entry.normalized.chunkIndex,
      startFrame: entry.normalized.startFrame,
      endFrame: entry.normalized.endFrame,
      audioLength: entry.normalized.audioLength,
      sampleRate: entry.normalized.sampleRate,
      channels: entry.normalized.channels,
      bitsPerSample: entry.normalized.bitsPerSample,
      bytesPerFrame: entry.normalized.bytesPerFrame,
      packetBytes,
    };
    return payloadToPcmResult(runtime, decodedPayload, entry.normalized);
  }
}

export function createRemoteOpusChunkCacheHandler(config = {}) {
  let runtimePromise = null;
  const ensureRuntime = () => {
    if (!runtimePromise) {
      runtimePromise = createOpusRuntime(config);
    }
    return runtimePromise;
  };

  return Object.freeze({
    async setRecordContext(nextContext = {}) {
      const runtime = await ensureRuntime();
      console.info("[vin.yl.player] tape prep:set-record-context", {
        hasDescriptorJson: Boolean(nextContext?.descriptorJson),
        descriptorJsonLength: String(nextContext?.descriptorJson || "").length,
        hasRecordHeaderProof: Boolean(nextContext?.recordHeaderProof),
      });
      return runtime.runtimeContext.set(nextContext);
    },
    async getRecordContext() {
      const runtime = await ensureRuntime();
      return runtime.runtimeContext.get();
    },
    async getMany(entries = []) {
      const runtime = await ensureRuntime();
      const recordContext = runtime.runtimeContext.get();
      if (!recordContext.descriptorJson) {
        console.warn("[vin.yl.player] remote opus cache getMany skipped: missing descriptor JSON", {
          entries: Array.isArray(entries) ? entries.length : 0,
        });
        return Array.isArray(entries) ? entries.map(() => null) : [];
      }
      const localResults = await Promise.all((Array.isArray(entries) ? entries : []).map(async entry => {
        const key = String(entry?.key || "");
        const normalized = normalizeSegmentMeta(entry?.meta || {});
        let result = null;
        if (key) {
          try {
            result = await readLocalCachedSegment(runtime, recordContext, key, normalized);
          } catch (error) {
            console.warn("[vin.yl.player] tape fetch local:malformed", {
              key,
              chunkIndex: normalized.chunkIndex,
              error: error instanceof Error ? error.message : String(error),
            });
            result = null;
          }
        }
        return { key, normalized, result };
      }));
      const misses = localResults.filter(entry => !entry.result).map(entry => ({
        key: entry.key,
        meta: entry.normalized,
      }));
      console.info("[vin.yl.player] tape fetch plan", {
        requested: localResults.length,
        localHits: localResults.filter((entry) => Boolean(entry.result)).length,
        remoteMisses: misses.length,
      });
      const remoteResults = await batchLookupRemoteSegments(runtime, recordContext, misses);
      const remoteByKey = new Map(misses.map((entry, index) => [entry.key, remoteResults[index] || null]));
      console.info("[vin.yl.player] tape fetch complete", {
        requested: localResults.length,
        localHits: localResults.filter((entry) => Boolean(entry.result)).length,
        remoteHits: remoteResults.filter((entry) => Boolean(entry)).length,
      });
      return localResults.map(entry => entry.result || remoteByKey.get(entry.key) || null);
    },
    async get(key, meta = {}) {
      const [result] = await this.getMany([{ key, meta }]);
      return result || null;
    },
    async put(key, pcm = {}) {
      const runtime = await ensureRuntime();
      const recordContext = runtime.runtimeContext.get();
      if (!recordContext.descriptorJson) {
        console.warn("[vin.yl.player] remote opus cache put skipped: missing descriptor JSON", { key });
        return false;
      }
      const normalized = normalizeSegmentMeta(pcm);
      const channelData = (Array.isArray(pcm.channelBuffers) ? pcm.channelBuffers : [])
        .map(buffer => new Int16Array(cloneArrayBuffer(buffer) || new ArrayBuffer(0)))
        .filter(channel => channel.length > 0);
      if (!channelData.length) {
        console.warn("[vin.yl.player] tape put skipped: empty PCM", { key });
        return false;
      }
      console.info("[vin.yl.player] tape put:start", {
        key,
        chunkIndex: normalized.chunkIndex,
        startFrame: normalized.startFrame,
        endFrame: normalized.endFrame,
        channels: normalized.channels,
        sampleRate: normalized.sampleRate,
      });
      const provider = runtime.helpers.createS16PcmWindowProvider({
        channelData,
        sampleRate: normalized.sampleRate,
        length: Math.max(0, normalized.endFrame - normalized.startFrame),
        numberOfChannels: normalized.channels,
        audioFormat: normalized.audioFormat,
      });
      const packets = await runtime.helpers.encodeS16PcmWindowProviderToSoundkitOpusPackets(
        provider,
        {
          channels: normalized.channels,
          sample_rate: normalized.sampleRate,
        },
        normalized.startFrame,
      );
      const packetBytes = concatenateUint8Chunks(
        packets.map(packet => uint8View(packet)),
        packets.reduce((sum, packet) => sum + uint8View(packet).byteLength, 0),
      );
      const payload = {
        key: String(key || ""),
        sourceKey: String(key || ""),
        audioFormat: normalized.audioFormat,
        chunkIndex: normalized.chunkIndex,
        startFrame: normalized.startFrame,
        endFrame: normalized.endFrame,
        audioLength: normalized.audioLength,
        sampleRate: normalized.sampleRate,
        channels: normalized.channels,
        bitsPerSample: normalized.bitsPerSample,
        bytesPerFrame: normalized.bytesPerFrame,
        packetBytes,
      };
      const stored = await runtime.cacheClient.writePlayerStageCache(
        runtime.storeName,
        payload,
        runtime.maxEntries,
        {
          cacheKey: payload.key,
          chunkIndex: payload.chunkIndex,
          packetOffset: payload.startFrame,
          audioFormat: payload.audioFormat,
          recordDescriptorJson: recordContext.descriptorJson,
          // chunkOffset/chunkByteLength here just need to be positive — the
          // tape worker's proof check (validate_opus_proof) only requires
          // chunkByteLength > 0, it doesn't cross-verify byte ranges against
          // the original ECDC stream. The opus payload's own byte length is
          // a real, always-available positive value for this purpose.
          recordHeaderProof: recordContext.recordHeaderProof
            ? runtime.cacheClient.playerEcdcCacheProofForChunk(
              recordContext.recordHeaderProof,
              payload.chunkIndex,
              0,
              packetBytes.byteLength,
            )
            : null,
          force: true,
          warn: true,
        },
      );
      return stored;
    },
  });
}

export function createRemoteOpusPrecache(config = {}) {
  const handler = config.cacheHandler || createRemoteOpusChunkCacheHandler(config);
  return Object.freeze({
    cacheHandler: handler,
    async precacheRecord(pngBytes, { recordProfile = "", decoderUrl = "./record-decoder-worker.js", onProgress = null } = {}) {
      const bytes = uint8View(pngBytes);
      const descriptorJson = await decodeRecordDescriptorJson(bytes, recordProfile);
      const decoder = new RecordDecoderClient(decoderUrl, {
        cache: handler,
      });
      try {
        const inspected = await decoder.inspect(bytes.slice(0), recordProfile);
        const recordBindingHex = await sha256Hex(bytes);
        if (typeof handler.setRecordContext === "function") {
          await handler.setRecordContext({
            descriptorJson,
            recordHeaderProof: inspected.recordHeaderProof || null,
            recordProfile: inspected.recordProfile || recordProfile,
          });
        }
        await decoder.decode(bytes.slice(0), inspected.recordProfile || recordProfile, {
          recordBindingHex,
        }, onProgress);
        return {
          descriptorJson,
          recordHeaderProof: inspected.recordHeaderProof || null,
          recordProfile: inspected.recordProfile || recordProfile,
          payloadContainer: inspected.payloadContainer || "",
          releaseId: inspected.releaseId || "",
          recordBindingHex,
        };
      } finally {
        decoder.close();
      }
    },
  });
}
