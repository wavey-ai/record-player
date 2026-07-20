#!/usr/bin/env node

import { createHash, randomBytes, randomInt } from "node:crypto";
import { createReadStream } from "node:fs";
import {
  copyFile,
  lstat,
  mkdir,
  mkdtemp,
  open,
  readFile,
  realpath,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import {
  dirname,
  extname,
  isAbsolute,
  relative,
  resolve,
  sep,
} from "node:path";

import { createDjAbxBlindPlan } from "../web/dj-abx.js";

function usage() {
  return [
    "Usage:",
    "  node scripts/prepare-dj-abx.mjs <spec.json> --out <blind-package-dir> --codebook <private-codebook.json> [--pilot]",
    "",
    "The package directory and private codebook must not already exist.",
    "Keep the private codebook outside the package and away from the test operator.",
    "Release packages require at least 24 balanced trials. --pilot permits a smaller test package.",
  ].join("\n");
}

function option(args, name) {
  const index = args.indexOf(name);
  if (index === -1 || !args[index + 1] || args[index + 1].startsWith("--")) return null;
  return args[index + 1];
}

async function exists(path) {
  try {
    await lstat(path);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

function isInside(parent, candidate) {
  const path = relative(parent, candidate);
  return path === "" || (!path.startsWith(`..${sep}`) && path !== ".." && !isAbsolute(path));
}

async function sha256File(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

async function copyBlindedWav(sourcePath, outputPath) {
  await copyFile(sourcePath, outputPath);
  const file = await open(outputPath, "r+");
  try {
    const { size } = await file.stat();
    const newSize = size + 40;
    if (newSize - 8 > 0xffff_ffff) throw new Error(`${sourcePath} exceeds the RIFF size limit`);
    const riffSize = Buffer.alloc(4);
    riffSize.writeUInt32LE(newSize - 8);
    await file.write(riffSize, 0, riffSize.length, 4);
    const junk = Buffer.alloc(40);
    junk.write("JUNK", 0, "ascii");
    junk.writeUInt32LE(32, 4);
    randomBytes(32).copy(junk, 8);
    await file.write(junk, 0, junk.length, size);
  } finally {
    await file.close();
  }
}

function readFourCc(bytes, offset) {
  return bytes.toString("ascii", offset, offset + 4);
}

async function inspectWav(path) {
  const file = await open(path, "r");
  let format = null;
  let dataBytes = null;
  try {
    const { size } = await file.stat();
    const riff = Buffer.alloc(12);
    if ((await file.read(riff, 0, riff.length, 0)).bytesRead !== riff.length
      || readFourCc(riff, 0) !== "RIFF"
      || readFourCc(riff, 8) !== "WAVE") {
      throw new Error(`${path} is not a RIFF/WAVE file`);
    }
    let offset = 12;
    while (offset + 8 <= size) {
      const header = Buffer.alloc(8);
      if ((await file.read(header, 0, header.length, offset)).bytesRead !== header.length) {
        throw new Error(`${path} contains a truncated WAV chunk header`);
      }
      const kind = readFourCc(header, 0);
      const length = header.readUInt32LE(4);
      const start = offset + 8;
      const end = start + length;
      if (end > size) throw new Error(`${path} contains a truncated ${kind} chunk`);
      if (kind === "fmt ") {
        if (length < 16) throw new Error(`${path} contains an invalid WAV format chunk`);
        const bytes = Buffer.alloc(16);
        if ((await file.read(bytes, 0, bytes.length, start)).bytesRead !== bytes.length) {
          throw new Error(`${path} contains a truncated WAV format chunk`);
        }
        format = {
          audioFormat: bytes.readUInt16LE(0),
          channels: bytes.readUInt16LE(2),
          sampleRateHz: bytes.readUInt32LE(4),
          blockAlign: bytes.readUInt16LE(12),
          bitsPerSample: bytes.readUInt16LE(14),
        };
      } else if (kind === "data") {
        dataBytes = length;
      }
      offset = end + (length % 2);
    }
  } finally {
    await file.close();
  }
  if (!format || dataBytes === null) throw new Error(`${path} lacks a WAV format or data chunk`);
  if (format.audioFormat !== 1 && format.audioFormat !== 3) {
    throw new Error(`${path} must use uncompressed PCM or IEEE float WAV audio`);
  }
  if (![1, 2].includes(format.channels)) throw new Error(`${path} must be mono or stereo`);
  if (format.sampleRateHz < 44_100) throw new Error(`${path} must use at least 44.1 kHz audio`);
  if (![16, 24, 32].includes(format.bitsPerSample)) {
    throw new Error(`${path} must use 16-, 24- or 32-bit WAV audio`);
  }
  const expectedBlockAlign = format.channels * format.bitsPerSample / 8;
  if (format.blockAlign !== expectedBlockAlign || dataBytes % format.blockAlign !== 0) {
    throw new Error(`${path} has inconsistent WAV frame alignment`);
  }
  return {
    audioFormat: format.audioFormat,
    channels: format.channels,
    sampleRateHz: format.sampleRateHz,
    bitsPerSample: format.bitsPerSample,
    frames: dataBytes / format.blockAlign,
  };
}

function sameWavFormat(left, right) {
  return Object.keys(left).every(key => left[key] === right[key]);
}

const args = process.argv.slice(2);
if (args.includes("--help") || args.includes("-h")) {
  console.log(usage());
  process.exit(0);
}

const specArgument = args.find(argument => !argument.startsWith("--")
  && argument !== option(args, "--out")
  && argument !== option(args, "--codebook"));
const outputArgument = option(args, "--out");
const codebookArgument = option(args, "--codebook");
if (!specArgument || !outputArgument || !codebookArgument) {
  console.error(usage());
  process.exit(1);
}

let temporaryDirectory = null;
let outputDirectory = null;
let packageCreated = false;
try {
  const specPath = resolve(specArgument);
  outputDirectory = resolve(outputArgument);
  const codebookPath = resolve(codebookArgument);
  if (isInside(outputDirectory, codebookPath)) {
    throw new Error("The private codebook must be outside the blind package directory");
  }
  if (await exists(outputDirectory)) throw new Error(`Blind package already exists: ${outputDirectory}`);
  if (await exists(codebookPath)) throw new Error(`Private codebook already exists: ${codebookPath}`);

  const specBytes = await readFile(specPath);
  const spec = JSON.parse(specBytes.toString("utf8"));
  if (!args.includes("--pilot")) {
    const familyCounts = new Map();
    for (const trial of spec.trials || []) {
      familyCounts.set(trial.gestureFamily, (familyCounts.get(trial.gestureFamily) || 0) + 1);
    }
    const counts = [
      "baby-drag-cue", "stab-transform", "chirp-flare", "crab-orbit", "fast-release", "motor-runout",
    ].map(family => familyCounts.get(family) || 0);
    if ((spec.trials?.length || 0) < 24 || Math.min(...counts) === 0 || Math.max(...counts) - Math.min(...counts) > 1) {
      throw new Error("A release ABX package requires at least 24 trials with balanced registered gesture families");
    }
  }
  const usedPaths = new Set();
  const plan = createDjAbxBlindPlan(spec, {
    randomInteger: maximum => randomInt(maximum),
    opaqueAudioPath: () => {
      let path;
      do path = `audio/${randomBytes(16).toString("hex")}.wav`;
      while (usedPaths.has(path));
      usedPaths.add(path);
      return path;
    },
  });

  const specDirectory = dirname(specPath);
  const sourceEvidence = new Map();
  const resolvedCapturePaths = new Set();
  const captureHashes = new Set();
  for (const trial of spec.trials) {
    const physicalPath = await realpath(resolve(specDirectory, trial.physicalPath));
    const playerPath = await realpath(resolve(specDirectory, trial.playerPath));
    for (const path of [physicalPath, playerPath]) {
      if (resolvedCapturePaths.has(path)) throw new Error(`Capture path is reused: ${path}`);
      resolvedCapturePaths.add(path);
    }
    if (extname(physicalPath).toLowerCase() !== ".wav" || extname(playerPath).toLowerCase() !== ".wav") {
      throw new Error(`${trial.id} must use WAV files for both conditions`);
    }
    const [physicalWav, playerWav, physicalSha256, playerSha256] = await Promise.all([
      inspectWav(physicalPath),
      inspectWav(playerPath),
      sha256File(physicalPath),
      sha256File(playerPath),
    ]);
    if (!sameWavFormat(physicalWav, playerWav)) {
      throw new Error(`${trial.id} physical and player WAV formats or frame counts differ`);
    }
    for (const hash of [physicalSha256, playerSha256]) {
      if (captureHashes.has(hash)) throw new Error(`${trial.id} reuses capture audio from another condition or trial`);
      captureHashes.add(hash);
    }
    sourceEvidence.set(trial.id, {
      physical: { path: trial.physicalPath, sha256: physicalSha256, wav: physicalWav },
      player: { path: trial.playerPath, sha256: playerSha256, wav: playerWav },
    });
  }
  for (const trial of plan.codebook.trials) trial.sources = sourceEvidence.get(trial.id);

  await mkdir(dirname(outputDirectory), { recursive: true });
  temporaryDirectory = await mkdtemp(`${outputDirectory}.tmp-`);
  await mkdir(resolve(temporaryDirectory, "audio"));
  const manifestAudioByPath = new Map(plan.manifest.trials.flatMap(trial => (
    Object.values(trial.audio).map(audio => [audio.path, audio])
  )));
  for (const copy of plan.copies) {
    const outputPath = resolve(temporaryDirectory, copy.outputPath);
    await copyBlindedWav(
      resolve(specDirectory, copy.sourcePath),
      outputPath,
    );
    manifestAudioByPath.get(copy.outputPath).sha256 = await sha256File(outputPath);
  }
  const outputHashes = [...manifestAudioByPath.values()].map(audio => audio.sha256);
  if (new Set(outputHashes).size !== outputHashes.length) {
    throw new Error("Blind WAV metadata did not produce unique package hashes");
  }
  const codebookJson = `${JSON.stringify(plan.codebook, null, 2)}\n`;
  const codebookSha256 = createHash("sha256").update(codebookJson).digest("hex");
  plan.manifest.codebookSha256 = codebookSha256;
  const manifestJson = `${JSON.stringify(plan.manifest, null, 2)}\n`;
  const manifestSha256 = createHash("sha256").update(manifestJson).digest("hex");
  await writeFile(resolve(temporaryDirectory, "blind-manifest.json"), manifestJson, { flag: "wx" });
  await rename(temporaryDirectory, outputDirectory);
  temporaryDirectory = null;
  packageCreated = true;
  await mkdir(dirname(codebookPath), { recursive: true });
  await writeFile(codebookPath, codebookJson, { flag: "wx" });

  console.log(`Blind package: ${outputDirectory}`);
  console.log(`Private codebook: ${codebookPath}`);
  console.log(`Codebook SHA-256: ${codebookSha256}`);
  console.log(`Manifest SHA-256: ${manifestSha256}`);
  console.log(`Trials: ${plan.manifest.trials.length}`);
} catch (error) {
  if (temporaryDirectory) await rm(temporaryDirectory, { recursive: true, force: true });
  if (packageCreated && outputDirectory) await rm(outputDirectory, { recursive: true, force: true });
  console.error(`ABX package error: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
}
