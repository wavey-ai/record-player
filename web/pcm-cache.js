const DB_NAME = "bitneedle-player-test-cache";
const DB_VERSION = 1;
const STORE_NAME = "decoded-pcm";

function openDatabase() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) db.createObjectStore(STORE_NAME);
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error || new Error("Failed to open PCM cache"));
  });
}

export async function recordCacheKey(bytes) {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), value => value.toString(16).padStart(2, "0")).join("");
}

export async function readPcmCache(key) {
  const db = await openDatabase();
  try {
    return await new Promise((resolve, reject) => {
      const request = db.transaction(STORE_NAME, "readonly").objectStore(STORE_NAME).get(key);
      request.onsuccess = () => resolve(request.result || null);
      request.onerror = () => reject(request.error || new Error("Failed to read PCM cache"));
    });
  } finally {
    db.close();
  }
}

export async function writePcmCache(key, value) {
  const db = await openDatabase();
  try {
    await new Promise((resolve, reject) => {
      const request = db.transaction(STORE_NAME, "readwrite").objectStore(STORE_NAME).put(value, key);
      request.onsuccess = () => resolve();
      request.onerror = () => reject(request.error || new Error("Failed to write PCM cache"));
    });
  } finally {
    db.close();
  }
}

export function createPcmChunkCacheHandler() {
  return Object.freeze({
    async get(key) {
      const value = await readPcmCache(key);
      const channelBuffers = Array.isArray(value?.channelBuffers)
        ? value.channelBuffers.map(buffer => buffer instanceof ArrayBuffer
          ? buffer.slice(0)
          : buffer?.buffer?.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength))
        : [];
      if (!channelBuffers.length) return null;
      return {
        chunkIndex: Number(value.chunkIndex) || 0,
        startFrame: Number(value.startFrame) || 0,
        endFrame: Number(value.endFrame) || 0,
        sampleRate: Number(value.sampleRate) || 48000,
        channels: channelBuffers.length,
        channelBuffers,
      };
    },
    async put(key, pcm) {
      const channelBuffers = Array.isArray(pcm?.channelBuffers)
        ? pcm.channelBuffers.map(buffer => buffer instanceof ArrayBuffer
          ? buffer.slice(0)
          : buffer?.buffer?.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength))
        : [];
      if (!channelBuffers.length) return;
      await writePcmCache(key, {
        chunkIndex: Number(pcm.chunkIndex) || 0,
        startFrame: Number(pcm.startFrame) || 0,
        endFrame: Number(pcm.endFrame) || 0,
        sampleRate: Number(pcm.sampleRate) || 48000,
        channels: channelBuffers.length,
        channelBuffers,
      });
    },
  });
}
