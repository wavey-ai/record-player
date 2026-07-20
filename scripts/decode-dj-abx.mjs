#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

import {
  decodeDjBlindAbxResponses,
  reconstructDjAbxBlindManifest,
} from "../web/dj-abx.js";

function usage() {
  return [
    "Usage:",
    "  node scripts/decode-dj-abx.mjs <private-codebook.json> <response.json> [options]",
    "",
    "Options:",
    "  --cue-codes <file>   Frozen blind cue-code assignments.",
    "  --results <file>     Merge decoded trials into an existing validation draft.",
    "  --out <file>         Write to a new file instead of stdout.",
    "",
    "Run this only after responses, exclusions and cue codes are frozen.",
  ].join("\n");
}

function parseArguments(args) {
  const options = { cueCodes: null, results: null, out: null };
  const positionals = [];
  const names = new Map([
    ["--cue-codes", "cueCodes"],
    ["--results", "results"],
    ["--out", "out"],
  ]);
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (names.has(argument)) {
      const value = args[index + 1];
      if (!value || value.startsWith("--")) throw new Error(`${argument} requires a path`);
      options[names.get(argument)] = value;
      index += 1;
    } else if (argument.startsWith("--")) {
      throw new Error(`Unknown option: ${argument}`);
    } else {
      positionals.push(argument);
    }
  }
  return { options, positionals };
}

async function readJson(path, label) {
  try {
    return JSON.parse(await readFile(resolve(path), "utf8"));
  } catch (error) {
    throw new Error(`${label} could not be read: ${error instanceof Error ? error.message : String(error)}`);
  }
}

async function readJsonWithSha256(path, label) {
  try {
    const bytes = await readFile(resolve(path));
    return {
      value: JSON.parse(bytes.toString("utf8")),
      sha256: createHash("sha256").update(bytes).digest("hex"),
    };
  } catch (error) {
    throw new Error(`${label} could not be read: ${error instanceof Error ? error.message : String(error)}`);
  }
}

function mergeDecodedTrials(resultsInput, decoded) {
  if (!resultsInput || resultsInput.schemaVersion !== 2 || !Array.isArray(resultsInput.participants)) {
    throw new TypeError("The validation draft must use schema version 2");
  }
  const results = structuredClone(resultsInput);
  const existingExcerptIds = new Set(results.participants.flatMap(participant => (
    Array.isArray(participant.trials) ? participant.trials.map(trial => trial.excerptId) : []
  )));
  for (const decodedParticipant of decoded.participants) {
    const participant = results.participants.find(value => value.id === decodedParticipant.id);
    if (!participant) throw new Error(`Validation draft has no participant ${decodedParticipant.id}`);
    if (!Array.isArray(participant.trials)) throw new TypeError(`${participant.id}.trials must be an array`);
    const existingIds = new Set(participant.trials.map(trial => trial.id));
    for (const trial of decodedParticipant.trials) {
      if (existingIds.has(trial.id)) throw new Error(`Validation draft already contains trial ${trial.id}`);
      if (existingExcerptIds.has(trial.excerptId)) {
        throw new Error(`Validation draft already contains excerpt ${trial.excerptId}`);
      }
      participant.trials.push(structuredClone(trial));
      existingIds.add(trial.id);
      existingExcerptIds.add(trial.excerptId);
    }
  }
  return results;
}

const args = process.argv.slice(2);
if (args.includes("--help") || args.includes("-h")) {
  console.log(usage());
  process.exit(0);
}

try {
  const { options, positionals } = parseArguments(args);
  if (positionals.length < 2) throw new Error(usage());
  const [codebookPath, ...responsePaths] = positionals;
  const [codebookFile, responseInputs, cueCodes, resultsInput] = await Promise.all([
    readJsonWithSha256(codebookPath, "Private codebook"),
    Promise.all(responsePaths.map((path, index) => readJson(path, `Response ${index + 1}`))),
    options.cueCodes ? readJson(options.cueCodes, "Cue codes") : null,
    options.results ? readJson(options.results, "Validation draft") : null,
  ]);
  const reconstructedManifest = reconstructDjAbxBlindManifest(
    codebookFile.value,
    codebookFile.sha256,
  );
  const manifestSha256 = createHash("sha256")
    .update(`${JSON.stringify(reconstructedManifest, null, 2)}\n`)
    .digest("hex");
  const decoded = decodeDjBlindAbxResponses(codebookFile.value, responseInputs, {
    codebookSha256: codebookFile.sha256,
    manifestSha256,
    cueCodes,
  });
  const output = resultsInput ? mergeDecodedTrials(resultsInput, decoded) : decoded;
  const json = `${JSON.stringify(output, null, 2)}\n`;
  if (options.out) {
    await writeFile(resolve(options.out), json, { flag: "wx" });
    console.log(`Decoded ABX output: ${resolve(options.out)}`);
  } else {
    process.stdout.write(json);
  }
} catch (error) {
  console.error(`ABX decode error: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
}
