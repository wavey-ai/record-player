#!/usr/bin/env node

import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, isAbsolute, resolve } from "node:path";

import {
  analyzeDjValidation,
  createDjValidationTemplate,
} from "./dj-validation-analysis.js";

function usage() {
  return [
    "Usage:",
    "  node scripts/analyze-dj-validation.mjs <results.json> [--json]",
    "  node scripts/analyze-dj-validation.mjs --template",
  ].join("\n");
}

function percent(value) {
  return value === null ? "n/a" : `${(value * 100).toFixed(2)}%`;
}

function formatReport(result) {
  const lines = [
    `DJ validation: ${result.accepted ? "PASS" : "FAIL"}`,
    `Candidate: ${result.candidateCommit}`,
    `Input SHA-256: ${result.sourceSha256}`,
    `Environment: ${result.environment.browser}; ${result.environment.os}; ${result.environment.inputDevice}`,
    `Audio path: ${result.environment.audioInterface}; ${result.environment.audioContextSampleRateHz} Hz; ${result.environment.interfaceBufferFrames} frames`,
    `Pointer-command p95: ${result.environment.pointerCommandLatencyMs.p95.toFixed(3)} ms`,
    `Acoustic-loopback p95: ${result.environment.acousticLoopback.p95Ms.toFixed(3)} ms (${result.environment.acousticLoopback.samples} probes)`,
    `Participants: ${result.participants.length}`,
    `Exclusions: ${result.exclusions.length}`,
    `ABX: ${result.pooledAbx.correct}/${result.pooledAbx.trials} correct (${percent(result.pooledAbx.accuracy)})`,
    `Exact two-sided p: ${result.pooledAbx.exactTwoSidedP?.toFixed(8) ?? "n/a"}`,
    `Exact 95% interval: ${percent(result.pooledAbx.clopperPearson95.lower)}–${percent(result.pooledAbx.clopperPearson95.upper)}`,
    `Rendered realism median: ${result.pooledAbx.renderedRealismMedian ?? "n/a"}/7`,
    `Live technique success: ${percent(result.live.successRate)}`,
    `Live ownership/timing medians: ${result.live.ownershipMedian ?? "n/a"}/7, ${result.live.timingMedian ?? "n/a"}/7`,
    `Live incidents: ${result.live.pointerLosses} pointer losses, ${result.live.stuckScratchIncidents} stuck scratches, ${result.live.postReleaseMutes} post-release mutes`,
    "Per-participant ABX:",
  ];
  for (const participant of result.participants) {
    lines.push(
      `  ${participant.id}: ${participant.correct}/${participant.trials} (${percent(participant.accuracy)}), exact 95% ${percent(participant.clopperPearson95.lower)}–${percent(participant.clopperPearson95.upper)}`,
    );
  }
  lines.push("Repeated cue groups:");
  if (result.cues.length === 0) lines.push("  none");
  else {
    for (const cue of result.cues) {
      lines.push(`  ${cue.gestureFamily}/${cue.cueCode}: ${cue.participantCount} DJs (${percent(cue.participantFraction)})`);
    }
  }
  lines.push("Criteria:");
  for (const [name, criterion] of Object.entries(result.criteria)) {
    lines.push(`  ${criterion.pass ? "PASS" : "FAIL"} ${name}: ${criterion.requirement}`);
  }
  return lines.join("\n");
}

async function sha256File(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

async function verifyArtifacts(input, inputPath) {
  if (!Array.isArray(input?.artifacts)) throw new TypeError("artifacts must be an array");
  const verifiedRoles = [];
  for (let index = 0; index < input.artifacts.length; index += 1) {
    const artifact = input.artifacts[index];
    if (!artifact || typeof artifact.path !== "string" || typeof artifact.sha256 !== "string") {
      throw new TypeError(`artifacts[${index}] must contain path and sha256 strings`);
    }
    if (artifact.path.trim().length === 0) throw new TypeError(`artifacts[${index}].path must not be empty`);
    if (!/^[0-9a-f]{64}$/i.test(artifact.sha256)) {
      throw new TypeError(`artifacts[${index}].sha256 must be a SHA-256 digest`);
    }
    const artifactPath = isAbsolute(artifact.path)
      ? artifact.path
      : resolve(dirname(inputPath), artifact.path);
    const actual = await sha256File(artifactPath);
    if (actual.toLowerCase() !== artifact.sha256.toLowerCase()) {
      throw new Error(`artifacts[${index}] SHA-256 mismatch: ${artifact.path}`);
    }
    verifiedRoles.push(artifact.role);
  }
  return verifiedRoles;
}

const args = process.argv.slice(2);
if (args.includes("--help") || args.includes("-h")) {
  console.log(usage());
  process.exit(0);
}
if (args.includes("--template")) {
  console.log(`${JSON.stringify(createDjValidationTemplate(), null, 2)}\n`);
  process.exit(0);
}

const inputPath = args.find(argument => !argument.startsWith("--"));
if (!inputPath) {
  console.error(usage());
  process.exit(1);
}

try {
  const resolvedInputPath = resolve(inputPath);
  const bytes = await readFile(resolvedInputPath);
  const sourceSha256 = createHash("sha256").update(bytes).digest("hex");
  const input = JSON.parse(bytes.toString("utf8"));
  const verifiedArtifactRoles = await verifyArtifacts(input, resolvedInputPath);
  const result = analyzeDjValidation(input, { sourceSha256, verifiedArtifactRoles });
  if (args.includes("--json")) console.log(`${JSON.stringify(result, null, 2)}\n`);
  else console.log(formatReport(result));
  process.exitCode = result.accepted ? 0 : 2;
} catch (error) {
  console.error(`DJ validation input error: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
}
