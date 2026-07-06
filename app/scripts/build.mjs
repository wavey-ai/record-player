import { access, cp, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const appDir = resolve(scriptDir, "..");
const rootDir = resolve(appDir, "..");
const distDir = resolve(appDir, "dist");
const wasmDir = resolve(distDir, "wasm");
const testdataDir = resolve(rootDir, "testdata");
const onnxRuntimeDir = resolve(process.env.BITNEEDLE_ONNX_RUNTIME_DIR || resolve(rootDir, "vendor/wasm/onnxruntime-web"));
const encodecBundlesDir = resolve(process.env.BITNEEDLE_ENCODEC_BUNDLES_DIR || resolve(rootDir, "vendor/wasm/encodec-rs/bundles"));

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

await requirePath(onnxRuntimeDir, "ONNX Runtime Web assets");
await requirePath(encodecBundlesDir, "EnCodec decoder bundles");
await mkdir(resolve(wasmDir, "onnxruntime-web"), { recursive: true });
await cp(onnxRuntimeDir, resolve(wasmDir, "onnxruntime-web"), { recursive: true });

// The decode_frame.onnx / lm_weights_q8.bin model files (31-36MB each) exceed
// Cloudflare Workers Assets' 25MB per-file limit, so they are not bundled as
// static assets here. They're uploaded separately to the ONNX_BUNDLES R2
// bucket (see `make sync-onnx-assets`) and streamed through the worker at
// the same URL the client already expects. Only the small bundle.json
// manifests are copied into dist.
const onnxBundlesOutDir = resolve(wasmDir, "encodec-rs", "onnx-bundles");
const bundleNames = (await readdir(encodecBundlesDir, { withFileTypes: true }))
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name);

for (const bundleName of bundleNames) {
  const sourceBundleDir = resolve(encodecBundlesDir, bundleName);
  const targetBundleDir = resolve(onnxBundlesOutDir, bundleName);
  await mkdir(targetBundleDir, { recursive: true });

  const bundle = JSON.parse(await readFile(resolve(sourceBundleDir, "bundle.json"), "utf8"));
  bundle.encode_model = "__not_shipped_in_bitneedle_player__";
  await writeFile(resolve(targetBundleDir, "bundle.json"), `${JSON.stringify(bundle, null, 2)}\n`);
}

const bundledTestRecord = resolve(testdataDir, "test.png");
try {
  await access(bundledTestRecord);
  await cp(bundledTestRecord, resolve(distDir, "test.png"));
} catch {
  // Optional local test asset.
}
