import { access, cp, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(scriptDir, "..");
const webDir = resolve(rootDir, "web");
const distDir = resolve(rootDir, "dist");
const testdataDir = resolve(rootDir, "testdata");
const vendorSoundkitDir = resolve(rootDir, "..", "vin.yl.vendor", "wasm", "soundkit-wasm");

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
await cp(webDir, distDir, { recursive: true });
await requirePath(vendorSoundkitDir, "SoundKit wasm package");
await cp(vendorSoundkitDir, resolve(distDir, "soundkit-wasm"), { recursive: true });


buildWasm(rootDir, resolve(distDir, "record-player"), "record_player", ["--features", "wasm"]);
buildWasm(resolve(rootDir, "player-wasm"), resolve(distDir, "player-wasm"), "player_wasm");
