(function attachBitneedleEcdcPcmLayout(globalScope) {
  // ECDC container parsing (metadata header + frame ranges) now lives in the
  // Rust record-* libraries and is exposed through the bitneedle-player WASM
  // module as ecdcMetadata / ecdcFrameRanges / ecdcChunkLayoutFromMetadata.
  // The helpers below are the trivial per-sample / per-frame math and the
  // graceful (non-bailing) chunk-layout solver that the streaming and
  // partial-payload preview paths rely on, so they stay in JS.

  function expectedFramePcmSamples(frame, meta) {
    const samples = Number(frame?.samples);
    const fallback = Number(meta?.segment_samples);
    return Math.max(0, Math.floor(
      Number.isFinite(samples) && samples > 0
        ? samples
        : Number.isFinite(fallback) && fallback > 0
          ? fallback
          : 0,
    ));
  }

  function segmentFrameLength(samples, segmentSamples, frameLength) {
    return Math.max(1, Math.ceil(
      (Math.max(1, Math.floor(samples)) * Math.max(1, Math.floor(frameLength)))
      / Math.max(1, Math.floor(segmentSamples)),
    ));
  }

  function segmentStarts(totalSamples, stride) {
    const starts = [];
    for (let offset = 0; offset < totalSamples; offset += Math.max(1, stride)) {
      starts.push(offset);
    }
    return starts;
  }

  // Non-bailing layout solver. Mirrors ecdc_chunk_layout_for_chunk_count in
  // encodec-rs, but falls back to the default layout when no candidate matches
  // the chunk count (the Rust solver bails). The cache-key and partial-payload
  // preview paths feed it chunk counts that legitimately do not reconcile with
  // the metadata stride, so the graceful fallback is required here.
  function ecdcChunkLayoutFromMetadata(meta, metadata, chunkCount) {
    const explicitSamples = Number(metadata?.cs ?? metadata?.chunk_samples ?? metadata?.chunkSamples);
    const explicitStride = Number(metadata?.cst ?? metadata?.chunk_stride ?? metadata?.chunkStride);
    const defaultLayout = {
      samples: Number.isFinite(explicitSamples) && explicitSamples > 0
        ? Math.floor(explicitSamples)
        : Math.max(1, Number(meta.segment_samples) || 1),
      stride: Number.isFinite(explicitStride) && explicitStride > 0
        ? Math.floor(explicitStride)
        : Math.max(1, Number(meta.segment_stride) || Number(meta.segment_samples) || 1),
    };
    if ((Number.isFinite(explicitSamples) && explicitSamples > 0) || (Number.isFinite(explicitStride) && explicitStride > 0)) {
      return defaultLayout;
    }

    const audioLength = Math.max(0, Number(metadata?.al ?? metadata?.audio_length ?? metadata?.audioLength) || 0);
    const candidates = [defaultLayout];
    if (Number(meta.sample_rate) === 48000) {
      candidates.push({ samples: 63998, stride: 63998 });
      candidates.push({ samples: 86400, stride: 86400 });
    }
    return candidates.find((layout) => segmentStarts(audioLength, layout.stride).length === chunkCount) || defaultLayout;
  }

  function triangleWeight(frameLength) {
    const length = Math.max(1, Math.floor(Number(frameLength) || 1));
    const weight = new Float32Array(length);
    for (let index = 0; index < length; index += 1) {
      const t = (index + 1) / (length + 1);
      weight[index] = 0.5 - Math.abs(t - 0.5);
    }
    return weight;
  }

  function frameStartSample(frame, frameIndex, layout) {
    const offset = Number(frame?.offset);
    return Number.isFinite(offset) && offset >= 0 ? Math.floor(offset) : frameIndex * layout.stride;
  }

  function floatToS16Sample(value) {
    const sample = Math.max(-1, Math.min(1, Number(value) || 0));
    return sample < 0 ? Math.round(sample * 32768) : Math.round(sample * 32767);
  }

  globalScope.BitneedleEcdcPcmLayout = Object.freeze({
    ...(globalScope.BitneedleEcdcPcmLayout || {}),
    ecdcChunkLayoutFromMetadata,
    expectedFramePcmSamples,
    floatToS16Sample,
    frameStartSample,
    segmentFrameLength,
    segmentStarts,
    triangleWeight,
  });
})(globalThis);
