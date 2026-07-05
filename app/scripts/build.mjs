import { access, cp, mkdir, rm } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const appDir = resolve(scriptDir, "..");
const rootDir = resolve(appDir, "..");
const distDir = resolve(appDir, "dist");
const wasmDir = resolve(distDir, "wasm");
const simulationDir = resolve(rootDir, "simulation");
const simulationWasmDir = resolve(wasmDir, "record-player-simulation");
const sharedDir = resolve(process.env.BITNEEDLE_SHARED_DIR || resolve(rootDir, "../bitneedle-platform/apps/shared"));
const playerWasmDir = resolve(process.env.BITNEEDLE_PLAYER_WASM_DIR || resolve(rootDir, "../bitneedle/player-wasm/pkg"));
const recordRenderWasmDir = resolve(process.env.BITNEEDLE_RECORD_RENDER_WASM_DIR || resolve(rootDir, "../bitneedle-platform/record-wasm/pkg"));
const onnxRuntimeDir = resolve(process.env.BITNEEDLE_ONNX_RUNTIME_DIR || resolve(rootDir, "../bitneedle-platform/vendor/wasm/onnxruntime-web"));
const encodecBundlesDir = resolve(process.env.BITNEEDLE_ENCODEC_BUNDLES_DIR || resolve(rootDir, "../bitneedle-platform/vendor/wasm/encodec-rs/bundles"));

async function requirePath(path, label) {
  try {
    await access(path);
  } catch {
    throw new Error(`${label} was not found at ${path}`);
  }
}

await rm(distDir, { recursive: true, force: true });
await mkdir(wasmDir, { recursive: true });
await mkdir(simulationWasmDir, { recursive: true });
await cp(resolve(appDir, "src"), distDir, { recursive: true });

const result = spawnSync("wasm-pack", [
  "build",
  rootDir,
  "--target",
  "web",
  "--out-dir",
  wasmDir,
  "--out-name",
  "bitneedle_record_player_core",
  "--features",
  "wasm"
], { stdio: "inherit" });

if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);

const simulationResult = spawnSync("wasm-pack", [
  "build",
  simulationDir,
  "--target",
  "web",
  "--out-dir",
  simulationWasmDir,
  "--out-name",
  "record_player_simulation_wasm"
], { stdio: "inherit" });

if (simulationResult.error) throw simulationResult.error;
if (simulationResult.status !== 0) process.exit(simulationResult.status ?? 1);

const sharedWorkerFiles = [
  "browser-formatting.js",
  "encodec-bundle-names.js",
  "onnx-runtime-session.js",
  "onnx-worker-tensors.js",
  "ecdc-pcm-layout.js"
];

for (const file of sharedWorkerFiles) {
  const source = resolve(sharedDir, file);
  await requirePath(source, `Shared decoder helper ${file}`);
  await cp(source, resolve(distDir, file));
}

await requirePath(playerWasmDir, "Bitneedle player WASM package");
await requirePath(recordRenderWasmDir, "Record-render WASM package");
await requirePath(onnxRuntimeDir, "ONNX Runtime Web assets");
await requirePath(encodecBundlesDir, "EnCodec decoder bundles");

await mkdir(resolve(wasmDir, "bitneedle-player"), { recursive: true });
await mkdir(resolve(wasmDir, "record-render"), { recursive: true });
await mkdir(resolve(wasmDir, "onnxruntime-web"), { recursive: true });
await mkdir(resolve(wasmDir, "encodec-rs", "onnx-bundles"), { recursive: true });

await cp(playerWasmDir, resolve(wasmDir, "bitneedle-player"), { recursive: true });
await cp(recordRenderWasmDir, resolve(wasmDir, "record-render"), { recursive: true });
await cp(onnxRuntimeDir, resolve(wasmDir, "onnxruntime-web"), { recursive: true });
await cp(encodecBundlesDir, resolve(wasmDir, "encodec-rs", "onnx-bundles"), { recursive: true });
