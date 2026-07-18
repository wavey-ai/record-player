(function attachBitneedleOnnxWorkerTensors(globalScope) {
  function summarizeOrtOutputs(outputs) {
    return Object.fromEntries(
      Object.entries(outputs || {}).map(([name, tensor]) => [
        name,
        {
          type: tensor.type,
          dims: tensor.dims,
          length: tensor.data.length,
        },
      ]),
    );
  }

  function findDecodeOutput(outputs) {
    const tensor = Object.values(outputs || {}).find(
      (candidate) => candidate.type === "float32" && candidate.dims.length === 3,
    );
    if (!tensor) {
      throw new Error(`Unexpected EnCodec decoder outputs: ${JSON.stringify(summarizeOrtOutputs(outputs))}`);
    }
    return tensor;
  }

  function disposeOrtTensor(tensor) {
    try {
      tensor?.dispose?.();
    } catch (_error) {
    }
  }

  function disposeOrtTensorMap(tensors) {
    Object.values(tensors || {}).forEach(disposeOrtTensor);
  }

  function buildDecodeInputs(frames, meta, start, end) {
    const batchSize = end - start;
    const frameLength = Math.max(1, Number(frames[start]?.frameLength) || Number(meta.frame_length) || 1);
    const valuesPerSegment = meta.num_codebooks * frameLength;
    const codes = new BigInt64Array(batchSize * valuesPerSegment);
    const scales = new Float32Array(batchSize);
    for (let frameIndex = start; frameIndex < end; frameIndex += 1) {
      const frame = frames[frameIndex];
      if ((Number(frame.frameLength) || frameLength) !== frameLength) {
        throw new Error("Mixed EnCodec frame lengths in one decoder batch.");
      }
      const localIndex = frameIndex - start;
      const base = localIndex * valuesPerSegment;
      const frameCodes = frame.codes instanceof Uint16Array ? frame.codes : new Uint16Array(frame.codes || []);
      for (let index = 0; index < valuesPerSegment; index += 1) {
        codes[base + index] = BigInt(frameCodes[index] || 0);
      }
      scales[localIndex] = Number(frame.scale ?? 1);
    }
    return { codes, scales, batchSize, frameLength };
  }

  globalScope.BitneedleOnnxWorkerTensors = Object.freeze({
    ...(globalScope.BitneedleOnnxWorkerTensors || {}),
    buildDecodeInputs,
    disposeOrtTensor,
    disposeOrtTensorMap,
    findDecodeOutput,
    summarizeOrtOutputs,
  });
})(globalThis);
