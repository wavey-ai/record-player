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

  globalScope.BitneedleEncodecBundleNames = Object.freeze({
    ...(globalScope.BitneedleEncodecBundleNames || {}),
    encodeBundleNameForRecord,
    encodeBundleNameFromEcdcMetadata,
    getRecordMeta,
    normalizeRecordProfileName,
    recordPayloadByteLength,
    recordVisibleTurns,
  });
})(globalThis);
