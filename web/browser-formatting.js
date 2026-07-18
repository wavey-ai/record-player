(function attachBitneedleBrowserFormatting(globalScope) {
  function formatBytes(bytes) {
    return `${Number(bytes || 0).toLocaleString()} bytes`;
  }

  function sanitizeFilenamePart(value, fallback) {
    const text = String(value || "").trim().toLowerCase()
      .replace(/['"]/g, "")
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "");
    return text || fallback;
  }

  function formatDecimal(value, digits = 2) {
    const numeric = Number(value);
    if (!Number.isFinite(numeric)) {
      return "0";
    }
    return numeric.toFixed(digits);
  }

  function workerErrorMessage(error, fallback = "decode failed") {
    return error?.message || String(error || fallback);
  }

  globalScope.BitneedleBrowserFormatting = Object.freeze({
    ...(globalScope.BitneedleBrowserFormatting || {}),
    formatDecimal,
    formatBytes,
    sanitizeFilenamePart,
    workerErrorMessage,
  });
})(globalThis);
