#!/usr/bin/env node

import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

import { createDjAbxCueCodeTemplate } from "../web/dj-abx.js";

function usage() {
  return [
    "Usage:",
    "  node scripts/create-dj-cue-code-template.mjs <blind-response.json> --out <cue-codes.json>",
    "",
    "Two blinded coders must resolve every non-empty cueCode before decoding conditions.",
  ].join("\n");
}

const args = process.argv.slice(2);
if (args.includes("--help") || args.includes("-h")) {
  console.log(usage());
  process.exit(0);
}

const outputIndex = args.indexOf("--out");
const responsePath = args[0] && !args[0].startsWith("--") ? resolve(args[0]) : null;
const outputPath = outputIndex >= 0 && args[outputIndex + 1] ? resolve(args[outputIndex + 1]) : null;
if (!responsePath || !outputPath) {
  console.error(usage());
  process.exit(1);
}

try {
  const response = JSON.parse(await readFile(responsePath, "utf8"));
  const template = createDjAbxCueCodeTemplate(response);
  await writeFile(outputPath, `${JSON.stringify(template, null, 2)}\n`, { flag: "wx" });
  console.log(`Cue-code template: ${outputPath}`);
  console.log(`Reported cues: ${template.entries.length}`);
} catch (error) {
  console.error(`Cue-code template error: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
}
