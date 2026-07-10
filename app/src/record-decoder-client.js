import { createLogger, isPlayerLoggingEnabled, isPlayerVerboseLoggingEnabled } from "./player-message-logger.js";

const log = createLogger("decoder-client");

export class RecordDecoderClient {
  constructor(url = "./record-decoder-worker.js", { loggingEnabled = isPlayerLoggingEnabled(), cache = null } = {}) {
    const workerUrl = new URL(url, globalThis.location?.href || import.meta.url);
    workerUrl.searchParams.set("player_log", loggingEnabled ? "1" : "0");
    this.worker = new Worker(workerUrl);
    this.requestId = 0;
    this.pending = new Map();
    this.cache = this.validateCacheHandler(cache);
    this.loggingEnabled = Boolean(loggingEnabled);
    this.worker.onmessage = event => {
      const type = event.data?.type;
      const classification = (type === "cache-get" || type === "cache-get-many" || type === "cache-put")
        ? "request"
        : event.data?.progress
          ? "progress"
          : event.data?.ok === true
            ? "response"
            : "error";
      if (isPlayerVerboseLoggingEnabled()) log.receive(`worker:${classification}`, event.data);
      this.handleMessage(event.data);
    };
    this.worker.onerror = event => {
      log.error("worker-error", { message: event.message, filename: event.filename, lineno: event.lineno });
      const error = new Error(event.message || "Record decoder worker failed");
      for (const request of this.pending.values()) request.reject(error);
      this.pending.clear();
    };
  }
  setLogging(enabled) {
    this.loggingEnabled = Boolean(enabled);
    log.action("logging-changed", { enabled: this.loggingEnabled });
    this.worker.postMessage({ id: 0, type: "set-logging", enabled: this.loggingEnabled });
  }
  validateCacheHandler(cache) {
    if (cache == null) return null;
    if (typeof cache !== "object") throw new Error("Cache handler must be an object.");
    if (typeof cache.get !== "function" || typeof cache.put !== "function") {
      throw new Error("Cache handler must provide async get(key, meta) and put(key, pcm) methods.");
    }
    return cache;
  }
  setCache(cache) {
    this.cache = this.validateCacheHandler(cache);
  }
  handleMessage(message) {
    if (message?.type === "cache-get" || message?.type === "cache-get-many" || message?.type === "cache-put") {
      void this.handleCacheMessage(message);
      return;
    }
    const request = this.pending.get(message.id);
    if (!request) { if (message.id !== 0) log.warn("unmatched-response", message); return; }
    if (message.progress) {
      const progress = message.progress;
      if (isPlayerVerboseLoggingEnabled()) {
        log.action("progress-shape", {
          requestId: message.id,
          keys: Object.keys(progress),
          status: progress.status,
          message: progress.msg || progress.message || "",
          chunksProcessed: progress.chunksProcessed,
          totalChunks: progress.totalChunks,
          chunksPending: progress.chunksPending,
          decodedSegmentCount: Array.isArray(progress.decodedPcmSegments) ? progress.decodedPcmSegments.length : 0,
          rawSegmentCount: Array.isArray(progress.rawDecodedPcmSegments) ? progress.rawDecodedPcmSegments.length : 0,
          decodedBytes: Array.isArray(progress.decodedPcmSegments)
            ? progress.decodedPcmSegments.reduce((total, segment) => total + (segment.channelBuffers || []).reduce((sum, buffer) => sum + (buffer?.byteLength || 0), 0), 0)
            : 0,
        }, { telemetry: true });
      }
      request.onProgress?.(progress);
      return;
    }
    this.pending.delete(message.id);
    if (message.ok) request.resolve(message.result);
    else {
      const errorMessage = message.error || "Record decoding failed";
      log.error("worker-request-failed", {
        requestId: message.id,
        type: request.type,
        error: errorMessage,
      });
      console.error("[vin.yl.player] record decoder worker request failed", {
        requestId: message.id,
        type: request.type,
        error: errorMessage,
      });
      request.reject(new Error(errorMessage));
    }
  }
  async handleCacheMessage(message) {
    const cache = this.cache;
    const response = {
      id: message.id,
      type: `${message.type}-result`,
      cacheRequestId: message.cacheRequestId,
      ok: true,
      result: null,
    };
    const transfer = [];
    log.action("cache-bridge:request", {
      type: message.type,
      cacheRequestId: message.cacheRequestId,
      key: message.key || "",
      entryCount: Array.isArray(message.entries) ? message.entries.length : 0,
      hasCache: Boolean(cache),
    });
    try {
      if (!cache) {
        response.result = null;
      } else if (message.type === "cache-get") {
        const result = await cache.get(message.key, message.meta || {});
        const channelBuffers = Array.isArray(result?.channelBuffers)
          ? result.channelBuffers.map(buffer => {
            const transferred = buffer instanceof ArrayBuffer
              ? buffer
              : buffer?.buffer?.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength);
            if (transferred) transfer.push(transferred);
            return transferred;
          }).filter(Boolean)
          : [];
        response.result = result && channelBuffers.length ? { ...result, channelBuffers } : null;
      } else if (message.type === "cache-get-many") {
        const entries = Array.isArray(message.entries) ? message.entries : [];
        const results = typeof cache.getMany === "function"
          ? await cache.getMany(entries)
          : await Promise.all(entries.map(entry => cache.get(entry?.key, entry?.meta || {})));
        response.result = Array.isArray(results) ? results.map(result => {
          const channelBuffers = Array.isArray(result?.channelBuffers)
            ? result.channelBuffers.map(buffer => {
              const transferred = buffer instanceof ArrayBuffer
                ? buffer
                : buffer?.buffer?.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength);
              if (transferred) transfer.push(transferred);
              return transferred;
            }).filter(Boolean)
            : [];
          return result && channelBuffers.length ? { ...result, channelBuffers } : null;
        }) : [];
      } else if (message.type === "cache-put") {
        await cache.put(message.key, message.pcm || {});
        response.result = { stored: true };
      }
      log.action("cache-bridge:response", {
        type: message.type,
        cacheRequestId: message.cacheRequestId,
        ok: true,
        hit: Boolean(response.result),
        entryCount: Array.isArray(response.result) ? response.result.filter(Boolean).length : 0,
        stored: response.result?.stored === true,
      });
    } catch (error) {
      response.ok = false;
      response.error = error?.message || String(error);
      log.warn("cache-bridge:failed", {
        type: message.type,
        cacheRequestId: message.cacheRequestId,
        error: response.error,
      });
    }
    this.worker.postMessage(response, transfer);
  }
  request(type, payload = {}, transfer = [], onProgress = null) {
    return new Promise((resolve, reject) => {
      const id = ++this.requestId;
      this.pending.set(id, { resolve, reject, onProgress, type, startedAt: performance.now() });
      const message = { id, type, ...payload };
      if (isPlayerVerboseLoggingEnabled()) log.send(`worker:${type}`, message, { transferCount: transfer.length });
      this.worker.postMessage(message, transfer);
    });
  }
  async initialise() { return this.request("init"); }
  async inspect(pngBytes, recordProfile = "") {
    const buffer = pngBytes instanceof ArrayBuffer ? pngBytes : pngBytes.buffer.slice(pngBytes.byteOffset, pngBytes.byteOffset + pngBytes.byteLength);
    return this.request("inspect-record-png", { pngBytes: buffer, recordProfile }, [buffer]);
  }
  async decode(pngBytes, recordProfile = "", options = {}, onProgress = null) {
    const buffer = pngBytes instanceof ArrayBuffer ? pngBytes : pngBytes.buffer.slice(pngBytes.byteOffset, pngBytes.byteOffset + pngBytes.byteLength);
    const recordBindingHex = String(options?.recordBindingHex || "").trim().toLowerCase();
    const cacheEnabled = Boolean(this.cache);
    log.action("decode-request", {
      hasCache: cacheEnabled,
      recordProfile,
      recordBindingHex: Boolean(recordBindingHex),
    });
    return this.request("decode-record-png", {
      pngBytes: buffer,
      recordProfile,
      record: { id: "local-record", recordProfile },
      runtime: { id: "wasm", label: "WASM CPU", executionProviders: ["wasm"] },
      cacheDecodedSegments: false,
      cache: { enabled: cacheEnabled },
      recordBindingHex,
    }, [buffer], onProgress);
  }
  close() {
    log.action("close", { pending: this.pending.size });
    const error = new Error("Record decoder client was closed");
    for (const request of this.pending.values()) request.reject(error);
    this.pending.clear();
    this.worker.terminate();
  }
}
