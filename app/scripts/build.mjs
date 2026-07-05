import { access, cp, mkdir, rm } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const appDir = resolve(scriptDir, "..");
const rootDir = resolve(appDir, "..");
const distDir = resolve(appDir, "dist");
const wasmDir = resolve(distDir, "wasm");
const sharedDir = resolve(process.env.BITNEEDLE_SHARED_DIR || resolve(rootDir, "../bitneedle-platform/apps/shared"));
const onnxRuntimeDir = resolve(process.env.BITNEEDLE_ONNX_RUNTIME_DIR || resolve(rootDir, "../bitneedle-platform/vendor/wasm/onnxruntime-web"));
const encodecBundlesDir = resolve(process.env.BITNEEDLE_ENCODEC_BUNDLES_DIR || resolve(rootDir, "../bitneedle-platform/vendor/wasm/encodec-rs/bundles"));

async function requirePath(path, label) {
  try {
    await access(path);
  } catch {
    throw new Error(`${label} was not found at ${path}`);
  }
}

function buildWasm(crateDir, outDir, outName, extraArgs = []) {
  const result = spawnSync("wasm-pack", [
    "build",
    crateDir,
    "--target",
    "web",
    "--release",
    "--out-dir",
    outDir,
    "--out-name",
    outName,
    ...extraArgs,
  ], { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

await rm(distDir, { recursive: true, force: true });
await mkdir(wasmDir, { recursive: true });
await cp(resolve(appDir, "src"), distDir, { recursive: true });

buildWasm(rootDir, resolve(wasmDir, "record-player"), "record_player", ["--features", "wasm"]);
buildWasm(resolve(rootDir, "player-wasm"), resolve(wasmDir, "player-wasm"), "player_wasm");

const sharedWorkerFiles = [
  "browser-formatting.js",
  "encodec-bundle-names.js",
  "onnx-runtime-session.js",
  "onnx-worker-tensors.js",
  "ecdc-pcm-layout.js",
];

for (const file of sharedWorkerFiles) {
  const source = resolve(sharedDir, file);
  await requirePath(source, `Shared decoder helper ${file}`);
  await cp(source, resolve(distDir, file));
}

await requirePath(onnxRuntimeDir, "ONNX Runtime Web assets");
await requirePath(encodecBundlesDir, "EnCodec decoder bundles");
await mkdir(resolve(wasmDir, "onnxruntime-web"), { recursive: true });
await mkdir(resolve(wasmDir, "encodec-rs", "onnx-bundles"), { recursive: true });
await cp(onnxRuntimeDir, resolve(wasmDir, "onnxruntime-web"), { recursive: true });
await cp(encodecBundlesDir, resolve(wasmDir, "encodec-rs", "onnx-bundles"), { recursive: true });
