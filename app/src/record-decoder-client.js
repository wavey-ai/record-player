export class RecordDecoderClient {
  constructor(url = "./record-decoder-worker.js") {
    this.worker = new Worker(url);
    this.requestId = 0;
    this.pending = new Map();
    this.worker.onmessage = event => this.handleMessage(event.data);
    this.worker.onerror = event => {
      const error = new Error(event.message || "Record decoder worker failed");
      for (const request of this.pending.values()) request.reject(error);
      this.pending.clear();
    };
  }

  handleMessage(message) {
    const request = this.pending.get(message.id);
    if (!request) return;
    if (message.progress) {
      request.onProgress?.(message.progress);
      return;
    }
    this.pending.delete(message.id);
    if (message.ok) request.resolve(message.result);
    else request.reject(new Error(message.error || "Record decoding failed"));
  }

  request(type, payload = {}, transfer = [], onProgress = null) {
    return new Promise((resolve, reject) => {
      const id = ++this.requestId;
      this.pending.set(id, { resolve, reject, onProgress });
      this.worker.postMessage({ id, type, ...payload }, transfer);
    });
  }

  async initialise() {
    return this.request("init");
  }

  async inspect(pngBytes, recordProfile = "") {
    const buffer = pngBytes instanceof ArrayBuffer ? pngBytes : pngBytes.buffer.slice(pngBytes.byteOffset, pngBytes.byteOffset + pngBytes.byteLength);
    return this.request("inspect-record-png", { pngBytes: buffer, recordProfile }, [buffer]);
  }

  async decode(pngBytes, recordProfile = "", onProgress = null) {
    const buffer = pngBytes instanceof ArrayBuffer ? pngBytes : pngBytes.buffer.slice(pngBytes.byteOffset, pngBytes.byteOffset + pngBytes.byteLength);
    return this.request("decode-record-png", {
      pngBytes: buffer,
      recordProfile,
      record: { id: "local-record", recordProfile },
      runtime: { id: "wasm", label: "WASM CPU", executionProviders: ["wasm"] },
      cacheDecodedSegments: true
    }, [buffer], onProgress);
  }

  close() {
    this.worker.terminate();
    this.pending.clear();
  }
}
