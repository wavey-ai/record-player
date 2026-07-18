(function attachBitneedleEncodecBundleNames(globalScope) {
  function getRecordMeta(record) {
    return record?.meta && typeof record.meta === "object" ? record.meta : {};
  }

  function normalizeRecordProfileName(recordProfile = "") {
    const normalized = String(recordProfile ?? "").trim();
    if (normalized === "single45") {
      return "single45";
    }
    if (normalized === "lp") {
      return "lp";
    }
    throw new Error(`Unknown record profile: ${recordProfile}`);
  }

  function recordPayloadByteLength(record) {
    const meta = getRecordMeta(record);
    const value =
      meta.payloadByteLength ||
      meta["X-Payload-Byte-Length"] ||
      record?.payloadByteLength;
    const numeric = Number(value);
    return Number.isFinite(numeric) && numeric > 0 ? Math.floor(numeric) : 0;
  }

  function recordVisibleTurns(record) {
    const meta = getRecordMeta(record);
    const value =
      meta.recordRevolutions ||
      meta["X-Record-Revolutions"] ||
      record?.recordRevolutions;
    const numeric = Number(value);
    return Number.isFinite(numeric) && numeric > 0 ? numeric : 0;
  }

  function encodeBundleNameForRecord(record) {
    const meta = getRecordMeta(record);
    const quality = String(meta.encodecQuality || meta["X-Encodec-Quality"] || "").toLowerCase();
    const bandwidth = Number(meta.encodecBandwidth || meta["X-Encodec-Bandwidth"]);
    if (quality === "standard" || (Number.isFinite(bandwidth) && bandwidth <= 6.5)) {
      return "encodec_48khz_6kbps";
    }
    return "encodec_48khz_12kbps";
  }

  function encodeBundleNameFromEcdcMetadata(metadata, recordContext) {
    const numCodebooks = Number(metadata?.nc ?? metadata?.num_codebooks ?? metadata?.numCodebooks);
    const chunkSamples = Number(metadata?.cs ?? metadata?.chunk_samples ?? metadata?.chunkSamples);
    const frameLength = Number(metadata?.fl ?? metadata?.frame_length ?? metadata?.frameLength);
    const chunkSuffix = encodeBundleChunkSuffix(chunkSamples, frameLength);
    if (numCodebooks === 4) {
      return `encodec_48khz_6kbps${chunkSuffix}`;
    }
    if (numCodebooks === 8) {
      return `encodec_48khz_12kbps${chunkSuffix}`;
    }
    return encodeBundleNameForRecord(recordContext);
  }

  function encodeBundleChunkSuffix(chunkSamples, frameLength) {
    // Guarded production bundles. chunkSamples is the full model window
    // (owned + 2 * 480-sample guard): 64,960 = 1333ms, 87,360 = 1800ms.
    if ((Number.isFinite(chunkSamples) && Math.abs(chunkSamples - 64960) <= 4) || frameLength === 203) {
      return "_1333ms";
    }
    if ((Number.isFinite(chunkSamples) && Math.abs(chunkSamples - 87360) <= 4) || frameLength === 273) {
      return "_1800ms";
    }
    return "";
  }

  function readUint32Be(bytes, offset) {
    return (
      (bytes[offset] * 0x1000000)
      + (bytes[offset + 1] << 16)
      + (bytes[offset + 2] << 8)
      + bytes[offset + 3]
    ) >>> 0;
  }

  // record-wasm reconstructs one standalone ECDC object per musical
  // revolution. Walk the explicit header and CRC-wrapped packet lengths rather
  // than searching entropy-coded bytes for magic strings.
  function splitStandaloneEcdcObjects(payload, recordContext = null) {
    const bytes = payload instanceof Uint8Array ? payload : new Uint8Array(payload || 0);
    const objects = [];
    let offset = 0;
    while (offset < bytes.byteLength) {
      if (
        offset + 9 > bytes.byteLength
        || bytes[offset] !== 0x45
        || bytes[offset + 1] !== 0x43
        || bytes[offset + 2] !== 0x44
        || bytes[offset + 3] !== 0x43
      ) {
        throw new Error(`Invalid standalone ECDC object at byte ${offset}.`);
      }
      if (bytes[offset + 4] !== 0) {
        throw new Error(`Unsupported ECDC version ${bytes[offset + 4]} at byte ${offset}.`);
      }
      const metadataLength = readUint32Be(bytes, offset + 5);
      const metadataStart = offset + 9;
      const metadataEnd = metadataStart + metadataLength;
      if (!metadataLength || metadataEnd > bytes.byteLength) {
        throw new Error(`Truncated ECDC metadata at byte ${offset}.`);
      }
      let metadata;
      try {
        metadata = JSON.parse(new TextDecoder().decode(bytes.subarray(metadataStart, metadataEnd)));
      } catch (error) {
        throw new Error(`Invalid ECDC metadata at byte ${offset}: ${error?.message || error}`);
      }
      let cursor = metadataEnd;
      let packetCount = 0;
      while (cursor < bytes.byteLength) {
        if (
          packetCount > 0
          && cursor + 4 <= bytes.byteLength
          && bytes[cursor] === 0x45
          && bytes[cursor + 1] === 0x43
          && bytes[cursor + 2] === 0x44
          && bytes[cursor + 3] === 0x43
        ) {
          break;
        }
        if (cursor + 8 > bytes.byteLength) {
          throw new Error(`Truncated ECDC packet header at byte ${cursor}.`);
        }
        const packetLength = readUint32Be(bytes, cursor);
        const packetEnd = cursor + 8 + packetLength;
        if (!packetLength || packetEnd > bytes.byteLength) {
          throw new Error(`Invalid ECDC packet length at byte ${cursor}.`);
        }
        cursor = packetEnd;
        packetCount += 1;
      }
      if (!packetCount) throw new Error(`Standalone ECDC object at byte ${offset} has no packets.`);
      objects.push({
        index: objects.length,
        bytes: bytes.subarray(offset, cursor),
        metadata,
        audioLength: Math.max(0, Number(metadata?.al ?? metadata?.audio_length) || 0),
        bundleName: encodeBundleNameFromEcdcMetadata(metadata, recordContext),
        packetCount,
      });
      offset = cursor;
    }
    return objects;
  }

  globalScope.BitneedleEncodecBundleNames = Object.freeze({
    ...(globalScope.BitneedleEncodecBundleNames || {}),
    encodeBundleNameForRecord,
    encodeBundleNameFromEcdcMetadata,
    splitStandaloneEcdcObjects,
    getRecordMeta,
    normalizeRecordProfileName,
    recordPayloadByteLength,
    recordVisibleTurns,
  });
})(globalThis);
