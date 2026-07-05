import { createLogger, isPlayerLoggingEnabled } from "./player-message-logger.js";

const log = createLogger("decoder-client");

export class RecordDecoderClient {
  constructor(url = "./record-decoder-worker.js", { loggingEnabled = isPlayerLoggingEnabled() } = {}) {
    const workerUrl = new URL(url, globalThis.location?.href || import.meta.url);
    workerUrl.searchParams.set("player_log", loggingEnabled ? "1" : "0");
    this.worker = new Worker(workerUrl);
    this.requestId = 0;
    this.pending = new Map();
    this.loggingEnabled = Boolean(loggingEnabled);
    this.worker.onmessage = event => {
      log.receive(`worker:${event.data?.progress ? "progress" : event.data?.ok ? "response" : "error"}`, event.data);
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
  handleMessage(message) {
    const request = this.pending.get(message.id);
    if (!request) { if (message.id !== 0) log.warn("unmatched-response", message); return; }
    if (message.progress) {
      const progress = message.progress;
      log.action("progress-shape", {
        requestId: message.id,
        keys: Object.keys(progress),
        status: progress.status,
        message: progress.msg || progress.message || "",
        decodedSegmentCount: Array.isArray(progress.decodedPcmSegments) ? progress.decodedPcmSegments.length : 0,
        rawSegmentCount: Array.isArray(progress.rawDecodedPcmSegments) ? progress.rawDecodedPcmSegments.length : 0,
        decodedBytes: Array.isArray(progress.decodedPcmSegments)
          ? progress.decodedPcmSegments.reduce((total, segment) => total + (segment.channelBuffers || []).reduce((sum, buffer) => sum + (buffer?.byteLength || 0), 0), 0)
          : 0,
      });
      request.onProgress?.(progress);
      return;
    }
    this.pending.delete(message.id);
    if (message.ok) request.resolve(message.result);
    else request.reject(new Error(message.error || "Record decoding failed"));
  }
  request(type, payload = {}, transfer = [], onProgress = null) {
    return new Promise((resolve, reject) => {
      const id = ++this.requestId;
      this.pending.set(id, { resolve, reject, onProgress, type, startedAt: performance.now() });
      const message = { id, type, ...payload };
      log.send(`worker:${type}`, message, { transferCount: transfer.length });
      this.worker.postMessage(message, transfer);
    });
  }
  async initialise() { return this.request("init"); }
  async inspect(pngBytes, recordProfile = "") {
    const buffer = pngBytes instanceof ArrayBuffer ? pngBytes : pngBytes.buffer.slice(pngBytes.byteOffset, pngBytes.byteOffset + pngBytes.byteLength);
    return this.request("inspect-record-png", { pngBytes: buffer, recordProfile }, [buffer]);
  }
  async decode(pngBytes, recordProfile = "", onProgress = null) {
    const buffer = pngBytes instanceof ArrayBuffer ? pngBytes : pngBytes.buffer.slice(pngBytes.byteOffset, pngBytes.byteOffset + pngBytes.byteLength);
    return this.request("decode-record-png", { pngBytes: buffer, recordProfile, record: { id: "local-record", recordProfile }, runtime: { id: "wasm", label: "WASM CPU", executionProviders: ["wasm"] }, cacheDecodedSegments: false }, [buffer], onProgress);
  }
  close() { log.action("close", { pending: this.pending.size }); this.worker.terminate(); this.pending.clear(); }
}
