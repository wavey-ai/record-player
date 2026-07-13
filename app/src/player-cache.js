"use strict";

(() => {
  function requirePlayerCacheFunction(value, name) {
    if (typeof value !== "function") {
      throw new Error(`Bitneedle player cache requires ${name}.`);
    }
    return value;
  }

  function createPlayerCache({
    generationCacheVersion = "dev",
    decodeSegmentCacheStoreName = "opus-chunks",
    waveformCacheStoreName = "waveform-peaks",
    decodeSegmentRemoteCacheFormats = new Set(),
    remoteCacheApiBaseUrl = "",
    remoteCacheStreamContentType = "application/vnd.bitneedle.player-cache-stream+binary",
    remoteCacheBatchFormat = "bitneedle-player-cache-batch-v1",
    remoteCacheBatchSize = 4,
    remoteCacheBatchWriteSize = 16,
    remoteCacheBatchRetryDelaysMs = Object.freeze([180, 600]),
    disableAllCaching = false,
    localCacheDbName = "bitneedle-player-cache",
    playerFetch,
    delay,
    decodeBase64ToUint8Array,
    encodeUint8ArrayBase64,
    encodeUint8ArrayBase64Url,
    uint8View,
    setStatus,
    getPlayerAppWasmModule = null,
  } = {}) {
    playerFetch = requirePlayerCacheFunction(playerFetch, "playerFetch");
    delay = requirePlayerCacheFunction(delay, "delay");
    decodeBase64ToUint8Array = requirePlayerCacheFunction(decodeBase64ToUint8Array, "decodeBase64ToUint8Array");
    encodeUint8ArrayBase64 = requirePlayerCacheFunction(encodeUint8ArrayBase64, "encodeUint8ArrayBase64");
    encodeUint8ArrayBase64Url = requirePlayerCacheFunction(encodeUint8ArrayBase64Url, "encodeUint8ArrayBase64Url");
    uint8View = requirePlayerCacheFunction(uint8View, "uint8View");
    setStatus = requirePlayerCacheFunction(setStatus, "setStatus");
    const readPlayerAppWasmModule = typeof getPlayerAppWasmModule === "function"
      ? getPlayerAppWasmModule
      : () => null;

    const acceptedDecodeSegmentRemoteCacheFormats =
      decodeSegmentRemoteCacheFormats && typeof decodeSegmentRemoteCacheFormats.has === "function"
        ? decodeSegmentRemoteCacheFormats
        : new Set(decodeSegmentRemoteCacheFormats || []);
    const textDecoder = new TextDecoder();
    const safeDecode = (view) => {
      if (!view) return textDecoder.decode(view);

      // 1. Extract the underlying buffer
      const buffer = view.buffer || view;

      // 2. Check for SharedArrayBuffer across worker/realm boundaries
      const isShared = buffer && (
        Object.prototype.toString.call(buffer) === '[object SharedArrayBuffer]' ||
        (buffer.constructor && buffer.constructor.name === 'SharedArrayBuffer') ||
        (typeof globalThis.SharedArrayBuffer !== 'undefined' && buffer instanceof globalThis.SharedArrayBuffer)
      );

      if (isShared) {
        // 3. Extract correct lengths regardless of TypedArray vs DataView
        const byteLength = view.byteLength ?? view.length ?? 0;
        const byteOffset = view.byteOffset ?? 0;

        // 4. Create a clean, standard, non-shared ArrayBuffer copy
        const cleanArray = new Uint8Array(byteLength);

        if (view.buffer) {
          // Create a fresh view slice and copy it over
          cleanArray.set(new Uint8Array(view.buffer, byteOffset, byteLength));
        } else {
          cleanArray.set(new Uint8Array(view));
        }

        return textDecoder.decode(cleanArray);
      }

      // Fallback for normal non-shared arrays
      return textDecoder.decode(view);
    };
    let remoteCacheUnavailable = false;
    let localCacheDbPromise = null;

    function canUsePlayerLocalCache() {
      return (
        !disableAllCaching &&
        typeof globalThis.indexedDB !== "undefined" &&
        typeof globalThis.indexedDB.open === "function"
      );
    }

    function requestToPromise(request) {
      return new Promise((resolve, reject) => {
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error || new Error("IndexedDB request failed"));
      });
    }

    function transactionToPromise(transaction) {
      return new Promise((resolve, reject) => {
        transaction.oncomplete = () => resolve();
        transaction.onerror = () => reject(transaction.error || new Error("IndexedDB transaction failed"));
        transaction.onabort = () => reject(transaction.error || new Error("IndexedDB transaction aborted"));
      });
    }

    function localCacheStoreNames() {
      return Array.from(new Set([
        decodeSegmentCacheStoreName,
        waveformCacheStoreName,
      ].filter(Boolean)));
    }

    function openPlayerLocalCacheDb() {
      if (!canUsePlayerLocalCache()) {
        return Promise.resolve(null);
      }
      if (localCacheDbPromise) {
        return localCacheDbPromise;
      }
      localCacheDbPromise = new Promise((resolve, reject) => {
        const request = indexedDB.open(localCacheDbName, 1);
        request.onupgradeneeded = () => {
          const db = request.result;
          for (const storeName of localCacheStoreNames()) {
            if (!db.objectStoreNames.contains(storeName)) {
              const store = db.createObjectStore(storeName, { keyPath: "key" });
              store.createIndex("accessedAt", "accessedAt", { unique: false });
            }
          }
        };
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error || new Error("Failed to open player local cache"));
      }).catch((error) => {
        console.warn("[bitneedle-player] local cache unavailable", error);
        localCacheDbPromise = null;
        return null;
      });
      return localCacheDbPromise;
    }

    function localCacheMaxEntries(storeName, fallback) {
      const numeric = Math.max(0, Math.floor(Number(fallback) || 0));
      if (numeric > 0) {
        return numeric;
      }
      if (storeName === decodeSegmentCacheStoreName) {
        return 512;
      }
      if (storeName === waveformCacheStoreName) {
        return 1024;
      }
      return 0;
    }

    async function readPlayerLocalCache(storeName, cacheKey) {
      if (!storeName || !cacheKey) {
        return null;
      }
      const db = await openPlayerLocalCacheDb();
      if (!db || !db.objectStoreNames.contains(storeName)) {
        return null;
      }
      try {
        const tx = db.transaction(storeName, "readwrite");
        const store = tx.objectStore(storeName);
        const record = await requestToPromise(store.get(cacheKey));
        if (record) {
          record.accessedAt = Date.now();
          store.put(record);
        }
        await transactionToPromise(tx);
        return record?.payload || null;
      } catch (error) {
        console.warn("[bitneedle-player] local cache read failed", { storeName, cacheKey, error });
        return null;
      }
    }

    async function prunePlayerLocalCache(db, storeName, maxEntries) {
      const limit = localCacheMaxEntries(storeName, maxEntries);
      if (!(limit > 0)) {
        return;
      }
      const tx = db.transaction(storeName, "readwrite");
      const store = tx.objectStore(storeName);
      const keys = await requestToPromise(store.getAllKeys());
      if (keys.length <= limit) {
        await transactionToPromise(tx);
        return;
      }
      const index = store.index("accessedAt");
      let removeCount = keys.length - limit;
      await new Promise((resolve, reject) => {
        const cursorRequest = index.openCursor();
        cursorRequest.onerror = () => reject(cursorRequest.error || new Error("IndexedDB cursor failed"));
        cursorRequest.onsuccess = () => {
          const cursor = cursorRequest.result;
          if (!cursor || removeCount <= 0) {
            resolve();
            return;
          }
          cursor.delete();
          removeCount -= 1;
          cursor.continue();
        };
      });
      await transactionToPromise(tx);
    }

    async function writePlayerLocalCache(storeName, payload, maxEntries) {
      if (!payload?.key || !shouldWritePlayerRemoteCachePayload(storeName, payload)) {
        return false;
      }
      const db = await openPlayerLocalCacheDb();
      if (!db || !db.objectStoreNames.contains(storeName)) {
        return false;
      }
      try {
        const tx = db.transaction(storeName, "readwrite");
        tx.objectStore(storeName).put({
          key: payload.key,
          payload,
          buildId: generationCacheVersion,
          createdAt: Date.now(),
          accessedAt: Date.now(),
        });
        await transactionToPromise(tx);
        await prunePlayerLocalCache(db, storeName, maxEntries);
        return true;
      } catch (error) {
        console.warn("[bitneedle-player] local cache write failed", { storeName, key: payload.key, error });
        return false;
      }
    }

    function playerJsonObjectEnd(bytes, start) {
      if (bytes[start] !== 123) {
        return 0;
      }
      let depth = 0;
      let inString = false;
      let escaped = false;
      for (let index = start; index < bytes.length; index += 1) {
        const byte = bytes[index];
        if (inString) {
          if (escaped) {
            escaped = false;
          } else if (byte === 92) {
            escaped = true;
          } else if (byte === 34) {
            inString = false;
          }
          continue;
        }
        if (byte === 34) {
          inString = true;
        } else if (byte === 123) {
          depth += 1;
        } else if (byte === 125) {
          depth -= 1;
          if (depth === 0) {
            return index + 1;
          }
          if (depth < 0) {
            return 0;
          }
        }
      }
      return 0;
    }

    function createPlayerEcdcCacheProofContext(ecdc) {
      const appWasm = readPlayerAppWasmModule();
      if (typeof appWasm?.createPlayerEcdcCacheProofContextJson !== "function") {
        return null;
      }
      try {
        return JSON.parse(appWasm.createPlayerEcdcCacheProofContextJson(uint8View(ecdc)));
      } catch (_error) {
        return null;
      }
    }

    function playerEcdcCacheProofForChunk(context, chunkIndex = 0, chunkOffset = 0, chunkByteLength = 0) {
      const appWasm = readPlayerAppWasmModule();
      if (typeof appWasm?.playerEcdcCacheProofForChunkJson !== "function") {
        return null;
      }
      try {
        const proof = JSON.parse(appWasm.playerEcdcCacheProofForChunkJson(
          JSON.stringify(context || null),
          Math.max(0, Math.floor(Number(chunkIndex) || 0)),
          Math.max(0, Math.floor(Number(chunkOffset) || 0)),
          Math.max(0, Math.floor(Number(chunkByteLength) || 0)),
        ));
        return proof || null;
      } catch (_error) {
        return null;
      }
    }

    function canUsePlayerRemoteCache({ ignoreUnavailable = false } = {}) {
      return (
        (ignoreUnavailable || !remoteCacheUnavailable) &&
        !disableAllCaching &&
        Boolean(remoteCacheApiBaseUrl) &&
        typeof globalThis.fetch === "function"
      );
    }

    function canUsePlayerRemoteCacheStore(storeName) {
      return storeName === decodeSegmentCacheStoreName;
    }

    function playerRemoteCacheBatchUrl() {
      return `${remoteCacheApiBaseUrl.replace(/\/+$/, "")}/batch`;
    }

    function playerRemoteCacheBatchKey(storeName, cacheKey) {
      return `${storeName}\u0000${cacheKey}`;
    }

    async function fetchPlayerRemoteCacheBatch(options) {
      let lastNetworkError = null;
      for (let attempt = 0; attempt <= remoteCacheBatchRetryDelaysMs.length; attempt += 1) {
        try {
          const response = await playerFetch(playerRemoteCacheBatchUrl(), options);
          if (response.ok || response.status < 500 || attempt >= remoteCacheBatchRetryDelaysMs.length) {
            return response;
          }
          await response.body?.cancel?.().catch(() => { });
        } catch (error) {
          if (!(error instanceof TypeError) || attempt >= remoteCacheBatchRetryDelaysMs.length) {
            throw error;
          }
          lastNetworkError = error;
        }
        await delay(remoteCacheBatchRetryDelaysMs[attempt]);
      }
      if (lastNetworkError) {
        throw lastNetworkError;
      }
      throw new Error("Remote cache batch retry failed");
    }

    function playerRemoteCacheBatchState(batch, storeName, cacheKey) {
      if (!batch?.checked || !cacheKey) {
        return "unknown";
      }
      const key = playerRemoteCacheBatchKey(storeName, cacheKey);
      if (!batch.checked.has(key)) {
        return "unknown";
      }
      return batch.hits?.has(key) ? "hit" : "miss";
    }

    function playerRemoteCacheBatchPayload(batch, storeName, cacheKey) {
      if (!batch?.payloads || !cacheKey) {
        return null;
      }
      return batch.payloads.get(playerRemoteCacheBatchKey(storeName, cacheKey)) || null;
    }

    function normalizePlayerCacheFormat(value) {
      return String(value || "")
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "_")
        .replace(/^_+|_+$/g, "");
    }

    function playerDecodedSegmentCacheFormat(payload) {
      return normalizePlayerCacheFormat(
        payload?.audioFormat ?? "",
      );
    }

    function playerDecodedSegmentPacketSources(payload) {
      if (Array.isArray(payload?.packets)) {
        return payload.packets;
      }
      if (Array.isArray(payload?.soundkitPackets)) {
        return payload.soundkitPackets;
      }
      if (Array.isArray(payload?.opusPackets)) {
        return payload.opusPackets;
      }
      if (Array.isArray(payload?.packetsBase64)) {
        return payload.packetsBase64.map(decodeBase64ToUint8Array);
      }
      return [];
    }

    function playerDecodedSegmentPacketBytes(payload) {
      return playerRemoteCacheBufferView(payload?.packetBytes || payload?.soundkitPacketBytes || null);
    }

    function playerRemoteCacheDescriptorJson(payload, options = {}) {
      return String(
        options.recordDescriptorJson
        || payload?.recordDescriptorJson
        || payload?.descriptorJson
        || "",
      ).trim();
    }

    function playerRemoteCacheEncryptionContext(storeName, payload, options = {}) {
      const packetBytes = playerDecodedSegmentPacketBytes(payload);
      const plaintextLength = Math.max(
        0,
        Math.floor(
          Number(payload?.packetByteLength)
          || Number(options.packetByteLength)
          || packetBytes?.byteLength
          || 0,
        ),
      );
      return {
        protocolVersion: 1,
        cacheFormatVersion: 1,
        cacheStoreName: String(storeName || ""),
        cacheKey: String(payload?.key || options.cacheKey || ""),
        chunkIndex: Math.max(0, Math.floor(Number(payload?.chunkIndex) || Number(options.chunkIndex) || 0)),
        packetOffset: Math.max(0, Math.floor(Number(payload?.startFrame) || Number(options.packetOffset) || 0)),
        plaintextLength,
        codecIdentifier: normalizePlayerCacheFormat(
          payload?.audioFormat
          || payload?.codecSource
          || options.codecIdentifier
          || "soundkit_opus_packets",
        ),
      };
    }

    function playerRemoteCachePayloadPreparedForStorage(storeName, payload, options = {}) {
      if (storeName !== decodeSegmentCacheStoreName || !payload?.key) {
        return payload;
      }
      if (payload?.encryptedCache === true) {
        return payload;
      }
      const descriptorJson = playerRemoteCacheDescriptorJson(payload, options);
      if (!descriptorJson) {
        console.warn("[vin.yl.player] tape prep skipped: no record descriptor JSON", { key: payload.key });
        return null;
      }
      const plaintext = playerDecodedSegmentPacketBytes(payload);
      if (!(plaintext?.byteLength > 0)) {
        console.warn("[vin.yl.player] tape prep skipped: empty plaintext", { key: payload.key });
        return null;
      }
      const context = playerRemoteCacheEncryptionContext(storeName, payload, options);
      if (!(context.plaintextLength > 0)) {
        console.warn("[vin.yl.player] tape prep skipped: zero plaintextLength", { key: payload.key, context });
        return null;
      }
      const appWasm = readPlayerAppWasmModule();
      if (typeof appWasm?.encryptCacheEntry !== "function") {
        console.warn("[vin.yl.player] tape prep skipped: encryptCacheEntry not available on wasm module", { key: payload.key });
        return null;
      }
      // encryptCacheEntry(descriptorJson, contextJson, plaintext) — 3 args,
      // nonce derived internally. A stale wasm bundle still on the older
      // 4-arg (..., nonce, plaintext) signature must fail loudly here rather
      // than silently mismarshal plaintext into the nonce slot.
      if (appWasm.encryptCacheEntry.length !== 3) {
        throw new Error("record-render wasm encryptCacheEntry has an unexpected arity; wasm/JS build mismatch.");
      }
      console.info("[vin.yl.player] tape prep:encrypt", {
        key: payload.key,
        descriptorJsonLength: descriptorJson.length,
        plaintextBytes: plaintext.byteLength,
        context,
      });
      // The nonce is derived deterministically inside the wasm module from
      // the record's own key material and the plaintext itself, so the same
      // (record, plaintext, context) always yields byte-identical ciphertext
      // — required for the remote store's content-addressed storage key.
      let encryptedBytes;
      try {
        encryptedBytes = appWasm.encryptCacheEntry(
          descriptorJson,
          JSON.stringify(context),
          plaintext,
        );
      } catch (error) {
        console.warn("[vin.yl.player] tape prep:encrypt failed", { key: payload.key, error: error?.message || error });
        return null;
      }
      console.info("[vin.yl.player] tape prep:encrypt done", { key: payload.key, encryptedBytes: encryptedBytes?.byteLength || encryptedBytes?.length || 0 });
      return {
        ...payload,
        packetBytes: encryptedBytes instanceof Uint8Array ? encryptedBytes : new Uint8Array(encryptedBytes || 0),
        encryptedCache: true,
        cacheEncryptionVersion: 1,
        cacheEncryptionAlgorithm: "xchacha20-poly1305",
      };
    }

    function playerRemoteCachePayloadDecryptedFromBytes(storeName, cacheKey, bytes, options = {}, payloadContext = null) {
      if (storeName !== decodeSegmentCacheStoreName) {
        return null;
      }
      const envelopeBytes = playerRemoteCacheBufferView(bytes);
      if (!(envelopeBytes?.byteLength > 0)) {
        return null;
      }
      const descriptorJson = playerRemoteCacheDescriptorJson(payloadContext || {}, options);
      if (!descriptorJson) {
        return null;
      }
      const appWasm = readPlayerAppWasmModule();
      if (typeof appWasm?.decryptCacheEntry !== "function") {
        return null;
      }
      const context = playerRemoteCacheEncryptionContext(
        storeName,
        {
          ...(payloadContext || {}),
          key: cacheKey,
        },
        options,
      );
      const plaintext = appWasm.decryptCacheEntry(
        descriptorJson,
        JSON.stringify(context),
        envelopeBytes,
      );
      const packetBytes = playerRemoteCacheBufferView(plaintext);
      if (!(packetBytes?.byteLength > 0)) {
        return null;
      }
      return {
        ...(payloadContext || {}),
        key: cacheKey,
        audioFormat: payloadContext?.audioFormat || options.audioFormat || "soundkit_opus_packets",
        packetBytes,
        packetByteLength: packetBytes.byteLength,
        encryptedCache: false,
      };
    }

    function isPlayerDecodedSegmentOpusCachePayload(payload) {
      return (
        Boolean(payload) &&
        acceptedDecodeSegmentRemoteCacheFormats.has(playerDecodedSegmentCacheFormat(payload)) &&
        (
          playerDecodedSegmentPacketBytes(payload)?.byteLength > 0 ||
          playerDecodedSegmentPacketSources(payload).length > 0
        )
      );
    }

    function shouldAcceptPlayerRemoteCachePayload(storeName, payload) {
      if (storeName === decodeSegmentCacheStoreName) {
        return isPlayerDecodedSegmentOpusCachePayload(payload);
      }
      return true;
    }

    function shouldWritePlayerRemoteCachePayload(storeName, payload) {
      if (storeName === decodeSegmentCacheStoreName) {
        return (
          isPlayerDecodedSegmentOpusCachePayload(payload) ||
          (
            payload?.encryptedCache === true &&
            playerRemoteCacheBufferView(payload?.packetBytes)?.byteLength > 0
          )
        );
      }
      return true;
    }

    function playerRemoteCacheBufferView(value) {
      if (value instanceof ArrayBuffer) {
        return new Uint8Array(value);
      }
      if (ArrayBuffer.isView(value)) {
        return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
      }
      return null;
    }

    function concatenatePlayerRemoteCacheBuffers(buffers, totalByteLength) {
      const output = new Uint8Array(Math.max(0, Math.floor(Number(totalByteLength) || 0)));
      let offset = 0;
      for (const buffer of buffers) {
        const view = playerRemoteCacheBufferView(buffer);
        if (!view?.byteLength) {
          continue;
        }
        output.set(view, offset);
        offset += view.byteLength;
      }
      return output;
    }

    function playerRemoteCachePayloadBytesForRemote(storeName, payload) {
      if (storeName !== decodeSegmentCacheStoreName || !payload) {
        return null;
      }
      const packetBytes = playerDecodedSegmentPacketBytes(payload);
      if (packetBytes?.byteLength > 0) {
        return packetBytes;
      }
      const packets = playerDecodedSegmentPacketSources(payload)
        .map(playerRemoteCacheBufferView)
        .filter((packet) => packet?.byteLength > 0);
      if (!packets.length) {
        return null;
      }
      return concatenatePlayerRemoteCacheBuffers(
        packets,
        packets.reduce((sum, packet) => sum + packet.byteLength, 0),
      );
    }

    function playerRemoteCachePayloadFromBytes(storeName, cacheKey, bytes) {
      if (storeName !== decodeSegmentCacheStoreName) {
        return null;
      }
      const packetBytes = playerRemoteCacheBufferView(bytes);
      if (!(packetBytes?.byteLength > 0)) {
        return null;
      }
      return {
        key: cacheKey,
        audioFormat: "soundkit_opus_packets",
        packetBytes,
      };
    }

    function playerRemoteCacheProofPayload(options = {}, body = null) {
      const proof = options.recordHeaderProof || null;
      const bodyByteLength = Number(body?.byteLength) || 0;
      if (!proof?.recordHeader || !(proof.chunkByteLength > 0) || !(bodyByteLength > 0)) {
        return null;
      }
      return {
        recordHeader: proof.recordHeader,
        chunkIndex: Math.max(0, Math.floor(Number(proof.chunkIndex) || 0)),
        chunkOffset: Math.max(0, Math.floor(Number(proof.chunkOffset) || 0)),
        chunkByteLength: Math.max(0, Math.floor(Number(proof.chunkByteLength) || 0)),
        bodyByteLength,
      };
    }

    async function readPlayerRemoteCacheBatchAvailability(entries, options = {}) {
      const force = Boolean(options.force);
      if (!canUsePlayerRemoteCache({ ignoreUnavailable: force }) || !Array.isArray(entries) || !entries.length) {
        return null;
      }

      const uniqueEntries = [];
      const seen = new Set();
      for (const entry of entries) {
        const storeName = String(entry?.storeName || "");
        const cacheKey = String(entry?.cacheKey || "");
        if (!storeName || !cacheKey || !canUsePlayerRemoteCacheStore(storeName)) {
          continue;
        }
        const key = playerRemoteCacheBatchKey(storeName, cacheKey);
        if (!seen.has(key)) {
          seen.add(key);
          uniqueEntries.push({ ...entry, storeName, cacheKey });
        }
      }
      if (!uniqueEntries.length) {
        return null;
      }

      const checked = new Set();
      const hits = new Set();
      const payloads = new Map();
      try {
        for (let start = 0; start < uniqueEntries.length; start += remoteCacheBatchSize) {
          const chunk = uniqueEntries.slice(start, start + remoteCacheBatchSize);
          const keys = chunk.map((entry) => entry.cacheKey);

          console.info("[vin.yl.player] tape batch request", {
            url: playerRemoteCacheBatchUrl(),
            keys,
          });
          const response = await fetchPlayerRemoteCacheBatch({
            method: "POST",
            mode: "cors",
            credentials: "omit",
            cache: "no-store",
            headers: {
              Accept: remoteCacheStreamContentType,
              "Content-Type": "application/json",
            },
            body: JSON.stringify({
              format: remoteCacheBatchFormat,
              keys,
            }),
          });

          if (response.status === 404 || response.status === 405) {
            return null;
          }
          if (!response.ok) {
            throw new Error(`Remote cache batch ${response.status}`);
          }
          remoteCacheUnavailable = false;

          for (const entry of chunk) {
            checked.add(playerRemoteCacheBatchKey(entry.storeName, entry.cacheKey));
          }
          if ((response.headers.get("Content-Type") || "").includes(remoteCacheStreamContentType)) {
            const storesByCacheKey = new Map();
            for (const entry of chunk) {
              if (!storesByCacheKey.has(entry.cacheKey)) {
                storesByCacheKey.set(entry.cacheKey, []);
              }
              storesByCacheKey.get(entry.cacheKey).push(entry);
            }
            const streamedPayloads = [];
            await readPlayerRemoteCachePayloadStream(response, {
              onPayload: (cacheKey, serializedPayload) => {
                streamedPayloads.push({ key: cacheKey, bytes: serializedPayload?.byteLength || 0 });
                for (const entry of storesByCacheKey.get(cacheKey) || []) {
                  const payload = playerRemoteCachePayloadDecryptedFromBytes(
                    entry.storeName,
                    cacheKey,
                    serializedPayload,
                    { ...options },
                    entry,
                  );
                  if (!payload || payload.key !== cacheKey || !shouldAcceptPlayerRemoteCachePayload(entry.storeName, payload)) {
                    continue;
                  }
                  const key = playerRemoteCacheBatchKey(entry.storeName, cacheKey);
                  hits.add(key);
                  payloads.set(key, payload);
                }
              },
            });
            console.info("[vin.yl.player] tape batch response", {
              url: playerRemoteCacheBatchUrl(),
              requestedKeys: chunk.length,
              streamedPayloads: streamedPayloads.length,
              acceptedHits: hits.size,
              totalBytes: streamedPayloads.reduce((sum, item) => sum + item.bytes, 0),
              payloads: streamedPayloads,
            });
          } else {
            const payload = await response.json();
            const jsonKeys = Array.isArray(payload?.results) ? payload.results : [];
            console.info("[vin.yl.player] tape batch response", {
              url: playerRemoteCacheBatchUrl(),
              requestedKeys: chunk.length,
              jsonResults: jsonKeys.length,
              hits: jsonKeys.filter((item) => item?.hit).length,
              directGets: jsonKeys.filter((item) => item?.hit && item?.directGetUrl).length,
            });
            const entriesByCacheKey = new Map(chunk.map((entry) => [entry.cacheKey, entry]));
            for (const item of jsonKeys) {
              const cacheKey = String(item?.key || "");
              const entry = entriesByCacheKey.get(cacheKey);
              if (!entry) {
                continue;
              }
              const key = playerRemoteCacheBatchKey(entry.storeName, cacheKey);
              checked.add(key);
              if (item?.hit) {
                hits.add(key);
              }
            }
          }
        }
        return { checked, hits, payloads };
      } catch (error) {
        if (error instanceof TypeError) {
          remoteCacheUnavailable = true;
        }
        console.warn("[vin.yl.player] tape batch check failed", error);
        return null;
      }
    }

    async function readPlayerRemoteCachePayloadStream(response, { onPayload } = {}) {
      const reader = response.body?.getReader?.();
      if (!reader) {
        return;
      }
      const frameHeaderBytes = 6;
      const legacyFramePrefixBytes = 4 + 64;
      const cacheKeyPattern = /^(?:[0-9a-f]{16}|[0-9a-f]{64})$/i;
      const chunks = [];
      let headIndex = 0;
      let headOffset = 0;
      let queuedBytes = 0;
      const appendPending = (chunk) => {
        if (chunk?.byteLength) {
          chunks.push(chunk);
          queuedBytes += chunk.byteLength;
        }
      };
      const compactChunks = () => {
        if (headIndex >= chunks.length) {
          chunks.length = 0;
          headIndex = 0;
          headOffset = 0;
          return;
        }
        if (headIndex > 32 && headIndex * 2 > chunks.length) {
          chunks.splice(0, headIndex);
          headIndex = 0;
        }
      };
      const consumeBytes = (byteLength) => {
        const length = Math.max(0, Math.floor(Number(byteLength) || 0));
        if (!length) {
          return new Uint8Array(0);
        }
        const first = chunks[headIndex];
        const firstAvailable = first ? first.byteLength - headOffset : 0;
        if (length <= firstAvailable) {
          const output = first.subarray(headOffset, headOffset + length);
          headOffset += length;
          queuedBytes -= length;
          if (headOffset >= first.byteLength) {
            headIndex += 1;
            headOffset = 0;
          }
          compactChunks();
          return output;
        }
        const output = new Uint8Array(length);
        let written = 0;
        while (written < length && headIndex < chunks.length) {
          const chunk = chunks[headIndex];
          const available = chunk.byteLength - headOffset;
          const toCopy = Math.min(length - written, available);
          output.set(chunk.subarray(headOffset, headOffset + toCopy), written);
          written += toCopy;
          headOffset += toCopy;
          queuedBytes -= toCopy;
          if (headOffset >= chunk.byteLength) {
            headIndex += 1;
            headOffset = 0;
          }
        }
        compactChunks();
        return output;
      };
      const peekBytes = (byteLength) => {
        const length = Math.max(0, Math.floor(Number(byteLength) || 0));
        if (!length) {
          return new Uint8Array(0);
        }
        const first = chunks[headIndex];
        const firstAvailable = first ? first.byteLength - headOffset : 0;
        if (length <= firstAvailable) {
          return first.subarray(headOffset, headOffset + length);
        }
        const output = new Uint8Array(length);
        let readIndex = headIndex;
        let readOffset = headOffset;
        let written = 0;
        while (written < length && readIndex < chunks.length) {
          const chunk = chunks[readIndex];
          const available = chunk.byteLength - readOffset;
          const toCopy = Math.min(length - written, available);
          output.set(chunk.subarray(readOffset, readOffset + toCopy), written);
          written += toCopy;
          readIndex += 1;
          readOffset = 0;
        }
        return output;
      };
      const processPending = () => {
        while (queuedBytes >= frameHeaderBytes) {
          const header = peekBytes(frameHeaderBytes);
          const view = new DataView(header.buffer, header.byteOffset, header.byteLength);
          const payloadLength = view.getUint32(0, false);
          const keyLength = view.getUint16(4, false);
          let framePrefixBytes = frameHeaderBytes + keyLength;
          let keyStart = frameHeaderBytes;
          if (keyLength < 1 || keyLength > 1024) {
            if (queuedBytes < legacyFramePrefixBytes) {
              return;
            }
            const legacyPrefix = peekBytes(legacyFramePrefixBytes);
            const legacyKey = safeDecode(legacyPrefix.subarray(4, legacyFramePrefixBytes));
            if (!cacheKeyPattern.test(legacyKey)) {
              consumeBytes(1);
              continue;
            }
            framePrefixBytes = legacyFramePrefixBytes;
            keyStart = 4;
          }
          if (queuedBytes < framePrefixBytes) {
            return;
          }
          const prefix = peekBytes(framePrefixBytes);
          const frameLength = framePrefixBytes + payloadLength;
          if (frameLength > queuedBytes) {
            return;
          }
          const cacheKey = safeDecode(prefix.subarray(keyStart, framePrefixBytes));
          consumeBytes(framePrefixBytes);
          const payload = consumeBytes(payloadLength);
          if (cacheKeyPattern.test(cacheKey)) {
            onPayload?.(cacheKey, payload);
          }
        }
      };
      while (true) {
        const { value, done } = await reader.read();
        if (value?.byteLength) {
          appendPending(value);
          processPending();
        }
        if (done) {
          processPending();
          break;
        }
      }
    }

    async function readPlayerRemoteCachePayload(storeName, cacheKey, options = {}) {
      if (!canUsePlayerRemoteCache({ ignoreUnavailable: Boolean(options.force) }) || !canUsePlayerRemoteCacheStore(storeName) || !cacheKey) {
        return null;
      }
      const remoteBatch = await readPlayerRemoteCacheBatchAvailability(
        [{ storeName, cacheKey }],
        { force: Boolean(options.force), quiet: options.quiet !== false },
      );
      const payload = playerRemoteCacheBatchPayload(remoteBatch, storeName, cacheKey);
      if (!payload || payload.key !== cacheKey || !shouldAcceptPlayerRemoteCachePayload(storeName, payload)) {
        return null;
      }
      return payload;
    }

    async function writePlayerRemoteCacheBatchPayloads(entries, options = {}) {
      const force = Boolean(options.force);
      let localWrites = 0;
      if (!disableAllCaching && Array.isArray(entries) && entries.length) {
        for (const entry of entries) {
          const storeName = String(entry?.storeName || "");
          const payload = entry?.payload || null;
          const preparedPayload = playerRemoteCachePayloadPreparedForStorage(storeName, payload, {
            ...(options || {}),
            ...(entry.options || {}),
          });
          if (storeName && preparedPayload?.key && await writePlayerLocalCache(storeName, preparedPayload, entry?.maxEntries || options.maxEntries)) {
            localWrites += 1;
          }
        }
      }
      if (!canUsePlayerRemoteCache({ ignoreUnavailable: force }) || !Array.isArray(entries) || !entries.length) {
        if (options.warn === true && !localWrites) {
          console.warn("[bitneedle-player] remote cache batch write skipped", {
            entries: Array.isArray(entries) ? entries.length : 0,
            hasApiBaseUrl: Boolean(remoteCacheApiBaseUrl),
            remoteCacheUnavailable,
            force,
            hasFetch: typeof globalThis.fetch === "function",
          });
        }
        return localWrites > 0;
      }
      const writes = [];
      const seen = new Set();
      let skippedInvalidPayloads = 0;
      let skippedMissingProofs = 0;
      console.info("[vin.yl.player] tape put batch:prepare", {
        url: playerRemoteCacheBatchUrl(),
        entries: entries.length,
        force: Boolean(options.force),
      });
      for (const entry of entries) {
        const storeName = String(entry?.storeName || "");
        const payload = entry?.payload || null;
        const storeOk = canUsePlayerRemoteCacheStore(storeName);
        const preparedPayload = playerRemoteCachePayloadPreparedForStorage(storeName, payload, {
          ...(options || {}),
          ...(entry.options || {}),
        });
        if (!storeOk || !preparedPayload?.key || !shouldWritePlayerRemoteCachePayload(storeName, preparedPayload)) {
          console.warn("[vin.yl.player] tape put batch:skip invalid payload", {
            storeName,
            storeOk,
            hasKey: Boolean(payload?.key),
          });
          skippedInvalidPayloads += 1;
          continue;
        }
        const key = playerRemoteCacheBatchKey(storeName, preparedPayload.key);
        if (seen.has(key)) {
          continue;
        }
        const body = playerRemoteCachePayloadBytesForRemote(storeName, preparedPayload);
        if (!(body?.byteLength > 0)) {
          skippedInvalidPayloads += 1;
          continue;
        }
        const proof = playerRemoteCacheProofPayload({ ...options, ...(entry.options || {}) }, body);
        if (!proof) {
          console.warn("[vin.yl.player] tape put batch:skip missing proof", {
            storeName,
            key: preparedPayload.key,
            hasEntryOptions: Boolean(entry.options),
            hasRecordHeaderProof: Boolean(entry.options?.recordHeaderProof),
          });
          skippedMissingProofs += 1;
          continue;
        }
        seen.add(key);
        writes.push({
          key: preparedPayload.key,
          payloadBase64: encodeUint8ArrayBase64(body),
          proof,
        });
      }
      if (!writes.length) {
        if (options.warn === true) {
          console.warn("[bitneedle-player] remote cache batch write had no valid writes", {
            entries: entries.length,
            skippedInvalidPayloads,
            skippedMissingProofs,
          });
        }
        return false;
      }

      try {
        let accepted = 0;
        for (let start = 0; start < writes.length; start += remoteCacheBatchWriteSize) {
          const chunk = writes.slice(start, start + remoteCacheBatchWriteSize);
          if (options.quiet === false) {
            setStatus(`Updating Cloudflare audio decode cache ${Math.min(start + chunk.length, writes.length)}/${writes.length}...`);
          }
          const response = await fetchPlayerRemoteCacheBatch({
            method: "POST",
            mode: "cors",
            credentials: "omit",
            cache: "no-store",
            headers: {
              Accept: "application/json",
              "Content-Type": "application/json",
            },
            body: JSON.stringify({
              format: remoteCacheBatchFormat,
              writes: chunk,
            }),
          });
          if (response.status === 404 || response.status === 405) {
            return false;
          }
          if (!response.ok) {
            const detail = await response.text().catch(() => "");
            const suffix = detail.trim() ? `: ${detail.trim().slice(0, 240)}` : "";
            throw new Error(`Remote cache batch write ${response.status}${suffix}`);
          }
          remoteCacheUnavailable = false;
          const result = await response.json().catch(() => null);
          const results = Array.isArray(result?.results) ? result.results : [];
          const acceptedChunk = results.length
            ? results.filter((item) => item?.stored !== false).length
            : chunk.length;
          accepted += acceptedChunk;
        }
        return accepted > 0;
      } catch (error) {
        if (error instanceof TypeError) {
          remoteCacheUnavailable = true;
        }
        console.warn("[vin.yl.player] tape put batch:failed", error);
        return false;
      }
    }

    async function writePlayerRemoteCachePayload(storeName, payload, options = {}) {
      return writePlayerRemoteCacheBatchPayloads([{ storeName, payload, options }], options);
    }

    async function readPlayerStageCache(storeName, cacheKey, options = {}) {
      if (disableAllCaching) {
        return null;
      }
      const localPayload = await readPlayerLocalCache(storeName, cacheKey);
      if (localPayload) {
        const decryptedLocalPayload = localPayload?.encryptedCache
          ? playerRemoteCachePayloadDecryptedFromBytes(
            storeName,
            cacheKey,
            localPayload.packetBytes || localPayload.soundkitPacketBytes || null,
            options,
            localPayload,
          )
          : localPayload;
        if (decryptedLocalPayload && shouldAcceptPlayerRemoteCachePayload(storeName, decryptedLocalPayload)) {
          return decryptedLocalPayload;
        }
      }
      if (options.skipRemote || !remoteCacheApiBaseUrl) {
        return null;
      }
      const batchPayload = playerRemoteCacheBatchPayload(options.remoteCacheBatch, storeName, cacheKey);
      if (batchPayload) {
        const preparedBatchPayload = playerRemoteCachePayloadPreparedForStorage(storeName, batchPayload, options);
        if (preparedBatchPayload) {
          void writePlayerLocalCache(storeName, preparedBatchPayload, options.maxEntries).catch(() => { });
        }
        if (shouldAcceptPlayerRemoteCachePayload(storeName, batchPayload)) {
          return batchPayload;
        }
      }
      if (playerRemoteCacheBatchState(options.remoteCacheBatch, storeName, cacheKey) === "miss") {
        return null;
      }
      const remotePayload = await readPlayerRemoteCachePayload(storeName, cacheKey, options);
      if (remotePayload && !shouldAcceptPlayerRemoteCachePayload(storeName, remotePayload)) {
        return null;
      }
      if (remotePayload) {
        const preparedRemotePayload = playerRemoteCachePayloadPreparedForStorage(storeName, remotePayload, options);
        if (preparedRemotePayload) {
          void writePlayerLocalCache(storeName, preparedRemotePayload, options.maxEntries).catch(() => { });
        }
      }
      return remotePayload;
    }

    async function writePlayerStageCache(storeName, payload, maxEntries, options = {}) {
      if (disableAllCaching) {
        return false;
      }
      if (!payload?.key) {
        return false;
      }
      const preparedPayload = playerRemoteCachePayloadPreparedForStorage(storeName, payload, options);
      if (!preparedPayload?.key) {
        return false;
      }
      const cachePayload = {
        ...preparedPayload,
        buildId: generationCacheVersion,
        accessedAt: Date.now(),
      };
      const localOk = await writePlayerLocalCache(storeName, cachePayload, maxEntries);
      if (!remoteCacheApiBaseUrl) {
        return localOk;
      }
      if (shouldWritePlayerRemoteCachePayload(storeName, cachePayload)) {
        const remoteOptions = {
          ...options,
          force: Boolean(options.force) || storeName === decodeSegmentCacheStoreName,
        };
        writePlayerRemoteCachePayload(storeName, cachePayload, remoteOptions).catch((error) => {
          console.warn("[vin.yl.player] tape put:failed", { key: cachePayload.key, error: error?.message || error });
        });
        return true;
      }
      return localOk;
    }

    return {
      createPlayerEcdcCacheProofContext,
      isPlayerDecodedSegmentOpusCachePayload,
      playerDecodedSegmentPacketBytes,
      playerDecodedSegmentPacketSources,
      playerEcdcCacheProofForChunk,
      playerRemoteCacheBatchPayload,
      readPlayerRemoteCacheBatchAvailability,
      readPlayerStageCache,
      writePlayerLocalCache,
      writePlayerRemoteCacheBatchPayloads,
      writePlayerStageCache,
    };
  }

  globalThis.BitneedlePlayerRuntimeCache = {
    createPlayerCache,
  };
})();
