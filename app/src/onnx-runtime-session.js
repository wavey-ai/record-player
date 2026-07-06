(function attachBitneedleOnnxRuntimeSession(globalScope) {
  function requireFunction(value, name) {
    if (typeof value !== "function") {
      throw new Error(`Bitneedle ONNX runtime session requires ${name}.`);
    }
    return value;
  }

  function resolveOnnxRuntime(runtime) {
    const executionProviders = Array.isArray(runtime?.executionProviders) && runtime.executionProviders.length
      ? runtime.executionProviders.map((provider) => String(provider || "")).filter(Boolean)
      : ["wasm"];
    return {
      id: runtime?.id || "wasm",
      label: runtime?.label || "WASM CPU",
      executionProviders: executionProviders.length ? executionProviders : ["wasm"],
    };
  }

  async function createOnnxRuntimeSession({
    modelPath,
    runtime,
    assetMetadata = null,
    ensureOnnxRuntimeModule,
    loadModelForOrt,
    graphOptimizationLevel = "all",
    enableCpuMemArena = true,
    enableMemPattern = true,
  } = {}) {
    ensureOnnxRuntimeModule = requireFunction(ensureOnnxRuntimeModule, "ensureOnnxRuntimeModule");
    loadModelForOrt = requireFunction(loadModelForOrt, "loadModelForOrt");
    const ort = await ensureOnnxRuntimeModule();
    const model = await loadModelForOrt(modelPath, assetMetadata);
    const resolvedRuntime = resolveOnnxRuntime(runtime);
    return {
      session: await ort.InferenceSession.create(model, {
        executionProviders: [...resolvedRuntime.executionProviders],
        graphOptimizationLevel,
        enableCpuMemArena,
        enableMemPattern,
      }),
      runtime: resolvedRuntime,
    };
  }

  globalScope.BitneedleOnnxRuntimeSession = Object.freeze({
    ...(globalScope.BitneedleOnnxRuntimeSession || {}),
    createOnnxRuntimeSession,
    resolveOnnxRuntime,
  });
})(globalThis);
