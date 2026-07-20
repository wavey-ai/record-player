function clamp01(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return 0;
  return Math.max(0, Math.min(1, number));
}

export function programmeHasStylusGaps(programmeMap) {
  return Array.isArray(programmeMap?.gaps) && programmeMap.gaps.length > 0;
}

export async function createProgrammeStylusCalibration(programmeMap, { loadModule } = {}) {
  if (!programmeHasStylusGaps(programmeMap)) return null;
  if (typeof loadModule !== "function") {
    throw new TypeError("A record-player WASM module loader is required");
  }
  const module = await loadModule();
  const Calibration = module?.StylusCalibration;
  if (typeof Calibration?.fromProgrammeMap !== "function") {
    throw new Error("record-player WASM does not expose StylusCalibration.fromProgrammeMap");
  }
  const calibration = Calibration.fromProgrammeMap(programmeMap);
  const totalSamples = Number(calibration?.totalSamples);
  if (!calibration?.hasGaps || !Number.isFinite(totalSamples) || totalSamples <= 0) {
    calibration?.free?.();
    throw new Error("record-player WASM returned an invalid programme-gap calibration");
  }
  let active = true;
  return Object.freeze({
    hasGaps: true,
    totalSamples,
    sampleRatioToGroove(progress) {
      const ratio = clamp01(progress);
      return active ? clamp01(calibration.sampleToGroove(ratio * totalSamples)) : ratio;
    },
    grooveToSampleRatio(progress) {
      const ratio = clamp01(progress);
      return active ? clamp01(calibration.grooveToSample(ratio) / totalSamples) : ratio;
    },
    destroy() {
      if (!active) return;
      active = false;
      calibration.free?.();
    },
  });
}
