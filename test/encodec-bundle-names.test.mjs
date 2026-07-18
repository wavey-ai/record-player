import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const source = await readFile(new URL("../web/encodec-bundle-names.js", import.meta.url), "utf8");

function loadHelpers() {
  const context = vm.createContext({ TextDecoder, Uint8Array });
  vm.runInContext(source, context, { filename: "encodec-bundle-names.js" });
  return context.BitneedleEncodecBundleNames;
}

function object(metadata, payload) {
  const json = new TextEncoder().encode(JSON.stringify(metadata));
  const bytes = new Uint8Array(9 + json.length + 8 + payload.length);
  bytes.set([0x45, 0x43, 0x44, 0x43, 0], 0);
  new DataView(bytes.buffer).setUint32(5, json.length, false);
  bytes.set(json, 9);
  const packetOffset = 9 + json.length;
  new DataView(bytes.buffer).setUint32(packetOffset, payload.length, false);
  bytes.set([1, 2, 3, 4], packetOffset + 4);
  bytes.set(payload, packetOffset + 8);
  return bytes;
}

test("splits and identifies mixed 12/6 kbps standalone ECDC objects", () => {
  const helpers = loadHelpers();
  const twelve = object({ al: 64000, nc: 8, fl: 203 }, Uint8Array.from([9, 8, 7]));
  const six = object({ al: 64000, nc: 4, fl: 203 }, Uint8Array.from([6, 5]));
  const payload = new Uint8Array(twelve.length + six.length);
  payload.set(twelve, 0);
  payload.set(six, twelve.length);
  const objects = helpers.splitStandaloneEcdcObjects(payload);
  assert.equal(objects.length, 2);
  assert.equal(objects[0].bundleName, "encodec_48khz_12kbps_1333ms");
  assert.equal(objects[1].bundleName, "encodec_48khz_6kbps_1333ms");
  assert.equal(objects[0].audioLength, 64000);
  assert.deepEqual(Array.from(objects[1].bytes), Array.from(six));
});
