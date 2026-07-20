import { DjValidationSession } from "./dj-validation-session.js";
import {
  createDjValidationTemplate,
  DJ_REQUIRED_ARTIFACT_ROLES,
} from "./dj-validation-template.js";
import { createPointerInputProfiler } from "./pointer-input-profile.js";

const STORAGE_KEY = "vinyl-dj-validation-v4";
const element = id => document.getElementById(id);
const statusOutput = element("console-status");
const playerFrame = element("validation-player");
const participantSelects = [
  element("block-participant"),
  document.querySelector("#trial-form [name=participantId]"),
  document.querySelector("#routine-form [name=participantId]"),
];
let session = new DjValidationSession();
let player = null;
let playerUnsubscribe = null;
let lastPlayerSnapshot = null;
let currentBuildInfo = null;
let movementTraces = [];
let activeScratchRecording = false;
let renderQueued = false;
let pointerInputProfiler = createPointerInputProfiler();
let pointerProbeStarted = false;
const activeProbePointers = new Set();

function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function ensureReleaseCandidate() {
  if (currentBuildInfo?.schemaVersion !== 1
    || !currentBuildInfo.commit
    || typeof currentBuildInfo.worktreeDirty !== "boolean") {
    throw new Error("Build metadata is not available. Rebuild before collecting release evidence.");
  }
  if (currentBuildInfo.worktreeDirty) {
    throw new Error("This build came from a dirty worktree. Commit and rebuild before collecting release evidence.");
  }
  const candidate = session.snapshot().candidate;
  if (candidate.commit !== currentBuildInfo.commit || candidate.worktreeDirty !== false) {
    throw new Error("The open draft does not match this clean player build.");
  }
  if (canonicalJson(candidate.settings) !== canonicalJson(currentBuildInfo.settings)) {
    throw new Error("The open draft settings do not match this player build.");
  }
  if (lastPlayerSnapshot) {
    const hfMatches = Math.abs(lastPlayerSnapshot.highFrequencyAccelerationLimit - 0.35) < 1e-9;
    const stylusMatches = Math.abs(lastPlayerSnapshot.stylusTracingLimit - 0.72) < 1e-9;
    if (!hfMatches
      || !stylusMatches
      || lastPlayerSnapshot.acousticEffects !== true
      || lastPlayerSnapshot.surfaceEffects !== true) {
      throw new Error("Restore the shipped acoustic and limiter settings before the block.");
    }
  }
}

function setStatus(message, error = false) {
  statusOutput.textContent = message;
  statusOutput.classList.toggle("error", error);
}

function formBoolean(form, name) {
  return Boolean(form.elements.namedItem(name)?.checked);
}

function formNumber(form, name) {
  const input = form.elements.namedItem(name);
  return input?.value === "" ? null : Number(input.value);
}

function persist() {
  localStorage.setItem(STORAGE_KEY, JSON.stringify({
    results: session.snapshot(),
    movementTraces,
  }));
}

function refreshParticipantSelects() {
  const participants = session.snapshot().participants;
  for (const select of participantSelects) {
    const selected = select.value;
    select.replaceChildren(...participants.map(participant => {
      const option = document.createElement("option");
      option.value = participant.id;
      option.textContent = participant.id;
      return option;
    }));
    if (participants.some(participant => participant.id === selected)) select.value = selected;
  }
}

function setFormValue(form, name, value) {
  const input = form.elements.namedItem(name);
  if (!input) return;
  if (input.type === "checkbox") input.checked = Boolean(value);
  else input.value = value ?? "";
}

function formatPointerProfile(profile) {
  if (!profile) return "No pointer input profile.";
  const typeSummaries = profile.pointerTypes.map(pointerType => {
    const type = profile.types[pointerType];
    const cadence = Number.isFinite(type.medianSampleRateHz)
      ? `${type.medianSampleRateHz.toFixed(1)} Hz median`
      : "cadence pending";
    const pressure = type.pressure.variable
      ? `variable pressure ${type.pressure.minimum.toFixed(2)}–${type.pressure.maximum.toFixed(2)}`
      : "no variable pressure";
    return `${pointerType}: ${type.gripPolicy}, ${cadence}, ${pressure}`;
  });
  const readiness = profile.requirements.pass
    ? "ready"
    : profile.requirements.reasons.join("; ");
  return [
    `${profile.contactSamples} samples`,
    `${profile.maximumConcurrentPointers} simultaneous pointers`,
    ...typeSummaries,
    readiness,
  ].join(" · ");
}

function renderPointerProfile(profile = session.snapshot().environment.pointerInputProfile) {
  const output = element("pointer-profile-result");
  output.textContent = formatPointerProfile(profile);
  output.classList.toggle("ready", Boolean(profile?.requirements?.pass));
}

function populateForms() {
  const data = session.snapshot();
  const environmentForm = element("environment-form");
  for (const name of ["inputDevice", "audioInterface", "displaySampleRateHz", "interfaceBufferFrames", "quietRoom"]) {
    setFormValue(environmentForm, name, data.environment[name]);
  }
  setFormValue(environmentForm, "headphones", data.environment.listeningTransducers.includes("headphones"));
  setFormValue(environmentForm, "monitors", data.environment.listeningTransducers.includes("monitors"));

  const preflightForm = element("preflight-form");
  for (const [name, value] of Object.entries(data.preflight.checks)) setFormValue(preflightForm, name, value);
  for (const name of [
    "levelMismatchDb", "fixedPathDelayMs", "declickBound", "maximumAdjacentDiscontinuity",
    "unexpectedClips", "undecodedZeroExcursions", "discontinuitiesAboveBound",
  ]) setFormValue(preflightForm, name, data.preflight[name]);

  const protocolForm = element("protocol-form");
  for (const [name, value] of Object.entries(data.blinding)) setFormValue(protocolForm, name, value);
  for (const [name, value] of Object.entries(data.cueCoding)) setFormValue(protocolForm, name, value);

  const artifactForm = element("artifact-form");
  for (const artifact of data.artifacts) {
    setFormValue(artifactForm, `${artifact.role}-path`, artifact.path);
    setFormValue(artifactForm, `${artifact.role}-sha256`, artifact.sha256);
  }
  const loopback = data.environment.acousticLoopback;
  renderPointerProfile(data.environment.pointerInputProfile);
  element("loopback-result").textContent = Number.isFinite(loopback?.p95Ms)
    ? `p95 ${loopback.p95Ms.toFixed(2)} ms · jitter ${loopback.jitterMs.toFixed(2)} ms · correlation ${loopback.minimumCorrelation.toFixed(3)}`
    : "No physical loopback result.";
}

function render() {
  renderQueued = false;
  const summary = session.summary();
  element("collection-summary").textContent = [
    `${summary.participants} DJs`,
    `${summary.trials} trials`,
    `${summary.routines} routines`,
    `${summary.blocks} blocks`,
  ].join(" · ");
  if (lastPlayerSnapshot) {
    element("player-state").textContent = [
      lastPlayerSnapshot.ready ? "record ready" : "no record",
      lastPlayerSnapshot.motorRunning ? "motor on" : "motor off",
      lastPlayerSnapshot.audioPlaybackStats?.underrunEvents === 0 ? "zero underruns" : "check underruns",
    ].join(" · ");
  }
  const active = summary.activeBlock;
  element("start-block").disabled = Boolean(active) || !player || summary.participants === 0;
  element("end-block").disabled = !active;
  element("cancel-block").disabled = !active;
  element("block-result").textContent = active
    ? `${active.participantId} ${active.kind.toUpperCase()} block is active.`
    : "No audio block is active.";
}

function queueRender() {
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(render);
}

function saveAndRender(message) {
  persist();
  refreshParticipantSelects();
  render();
  setStatus(message);
}

function replaceSession(data, traces = []) {
  session = new DjValidationSession({ data });
  movementTraces = Array.isArray(traces) ? structuredClone(traces) : [];
  pointerInputProfiler = createPointerInputProfiler();
  pointerProbeStarted = false;
  activeProbePointers.clear();
  refreshParticipantSelects();
  populateForms();
  render();
}

function download(name, content, type = "application/json") {
  const url = URL.createObjectURL(new Blob([content], { type }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 1_000);
}

async function sha256Hex(text) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map(value => value.toString(16).padStart(2, "0")).join("");
}

async function loadBuildInfo() {
  try {
    const response = await fetch("./player-build-info.json", { cache: "no-store" });
    if (!response.ok) throw new Error(`build metadata returned ${response.status}`);
    currentBuildInfo = await response.json();
    const label = currentBuildInfo.commit
      ? `${currentBuildInfo.commit.slice(0, 12)}${currentBuildInfo.worktreeDirty ? " · dirty" : " · clean"}`
      : "unknown build";
    element("build-state").textContent = label;
    const candidate = session.snapshot().candidate;
    if (candidate.commit === "replace-with-tested-commit" && currentBuildInfo.commit) {
      session.setBuildInfo(currentBuildInfo);
      persist();
    } else if (currentBuildInfo.commit && candidate.commit !== currentBuildInfo.commit) {
      setStatus("The open draft belongs to a different build.", true);
    }
  } catch (error) {
    element("build-state").textContent = "metadata unavailable";
    setStatus(error.message || String(error), true);
  }
}

function attachPlayer(nextPlayer) {
  playerUnsubscribe?.();
  player = nextPlayer;
  playerUnsubscribe = player.subscribe(snapshot => {
    lastPlayerSnapshot = snapshot;
    session.observePlayerState(snapshot);
    queueRender();
  });
  setStatus("Player ready. Load the tested record and complete the setup.");
  render();
}

function findPlayer() {
  const frameWindow = playerFrame.contentWindow;
  const available = frameWindow?.vin?.yl?.player;
  if (available) {
    attachPlayer(available);
    return;
  }
  frameWindow?.addEventListener("vin.yl.player.ready", event => attachPlayer(event.detail), { once: true });
}

playerFrame.addEventListener("load", findPlayer);
findPlayer();

element("environment-form").addEventListener("submit", event => {
  event.preventDefault();
  try {
    const form = event.currentTarget;
    session.setEnvironment({
      browser: navigator.userAgent,
      os: navigator.userAgentData?.platform || navigator.platform || "Unknown",
      inputDevice: form.elements.inputDevice.value.trim(),
      audioInterface: form.elements.audioInterface.value.trim(),
      listeningTransducers: [
        ...(formBoolean(form, "headphones") ? ["headphones"] : []),
        ...(formBoolean(form, "monitors") ? ["monitors"] : []),
      ],
      quietRoom: formBoolean(form, "quietRoom"),
      displaySampleRateHz: formNumber(form, "displaySampleRateHz"),
      interfaceBufferFrames: formNumber(form, "interfaceBufferFrames"),
    });
    saveAndRender("Hardware and room details saved.");
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

function resetPointerProbe() {
  pointerInputProfiler = createPointerInputProfiler();
  pointerProbeStarted = false;
  activeProbePointers.clear();
  session.clearPointerInputProfile();
  renderPointerProfile(null);
  persist();
}

function observePointerProbe(event) {
  event.preventDefault();
  if (event.type === "pointerdown") {
    if (!pointerProbeStarted) {
      pointerInputProfiler = createPointerInputProfiler();
      pointerProbeStarted = true;
    }
    activeProbePointers.add(event.pointerId);
    try {
      event.currentTarget.setPointerCapture(event.pointerId);
    } catch {
      // The profiler still records devices that do not expose pointer capture.
    }
  }
  const profile = pointerInputProfiler.observe(event.type, event);
  if (["pointerup", "pointercancel", "lostpointercapture"].includes(event.type)) {
    activeProbePointers.delete(event.pointerId);
  }
  session.setPointerInputProfile(profile);
  renderPointerProfile(profile);
  if (event.type !== "pointermove") persist();
}

const pointerProbePad = element("pointer-probe-pad");
for (const type of ["pointerdown", "pointermove", "pointerup", "pointercancel", "lostpointercapture"]) {
  pointerProbePad.addEventListener(type, observePointerProbe);
}
element("reset-pointer-probe").addEventListener("click", resetPointerProbe);

async function refreshInputs() {
  try {
    const devices = await navigator.mediaDevices.enumerateDevices();
    const select = element("loopback-input");
    const selected = select.value;
    const inputs = devices.filter(device => device.kind === "audioinput");
    select.replaceChildren(new Option("Default input", ""), ...inputs.map((device, index) => (
      new Option(device.label || `Audio input ${index + 1}`, device.deviceId)
    )));
    select.value = inputs.some(device => device.deviceId === selected) ? selected : "";
    setStatus(`${inputs.length} audio inputs available.`);
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
}

element("refresh-inputs").addEventListener("click", () => { void refreshInputs(); });
element("measure-loopback").addEventListener("click", async event => {
  if (!player) return setStatus("Wait for the player to start.", true);
  const button = event.currentTarget;
  button.disabled = true;
  try {
    ensureReleaseCandidate();
    setStatus("Measuring the physical loopback. Keep the room quiet.");
    const result = await player.measureAcousticLoopbackLatency({
      inputDeviceId: element("loopback-input").value || null,
      repetitions: 5,
      amplitude: 0.08,
      maximumLatencyMs: 500,
    });
    session.setAcousticLoopback(result);
    element("loopback-result").textContent = [
      `p95 ${result.p95Ms.toFixed(2)} ms`,
      `jitter ${result.jitterMs.toFixed(2)} ms`,
      `correlation ${result.minimumCorrelation.toFixed(3)}`,
      result.inputDeviceLabel,
    ].join(" · ");
    persist();
    await refreshInputs();
    setStatus("Physical loopback result saved.");
  } catch (error) {
    setStatus(error.message || String(error), true);
  } finally {
    button.disabled = false;
  }
});

element("participant-form").addEventListener("submit", event => {
  event.preventDefault();
  try {
    const form = event.currentTarget;
    session.addParticipant({
      id: form.elements.id.value,
      currentlyActiveDj: formBoolean(form, "currentlyActiveDj"),
      regularlyScratches: formBoolean(form, "regularlyScratches"),
      experienceBand: form.elements.experienceBand.value,
      trainingCompleted: formBoolean(form, "trainingCompleted"),
    });
    form.elements.id.value = "";
    saveAndRender("Participant added.");
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

element("start-block").addEventListener("click", () => {
  try {
    ensureReleaseCandidate();
    const participantId = element("block-participant").value;
    const kind = element("block-kind").value;
    session.startBlock(participantId, kind, player.getState());
    if (kind === "live") {
      player.startScratchRecording({ name: `${participantId} live validation` });
      activeScratchRecording = true;
    }
    persist();
    render();
    setStatus(`${kind.toUpperCase()} audio block started.`);
  } catch (error) {
    session.cancelBlock();
    setStatus(error.message || String(error), true);
  }
});

element("end-block").addEventListener("click", async () => {
  try {
    ensureReleaseCandidate();
    const active = session.summary().activeBlock;
    const block = session.endBlock(player.getState());
    if (activeScratchRecording) {
      const trace = await player.stopScratchRecording({ save: false });
      if (trace) movementTraces.push({ participantId: active.participantId, blockId: block.id, trace });
      activeScratchRecording = false;
    }
    saveAndRender(`${block.kind.toUpperCase()} audio block saved.`);
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

element("cancel-block").addEventListener("click", async () => {
  if (activeScratchRecording) {
    await player.stopScratchRecording({ save: false });
    activeScratchRecording = false;
  }
  session.cancelBlock();
  saveAndRender("Audio block canceled.");
});

element("trial-form").addEventListener("submit", event => {
  event.preventDefault();
  try {
    const form = event.currentTarget;
    session.addTrial(form.elements.participantId.value, {
      excerptId: form.elements.excerptId.value,
      gestureFamily: form.elements.gestureFamily.value,
      aCondition: form.elements.aCondition.value,
      bCondition: form.elements.bCondition.value,
      xCondition: form.elements.xCondition.value,
      responseCondition: form.elements.responseCondition.value,
      captureSha256: {
        physical: form.elements.physicalCaptureSha256.value,
        player: form.elements.playerCaptureSha256.value,
      },
      confidence: formNumber(form, "confidence"),
      realism: formNumber(form, "realism"),
      transientSharpness: formNumber(form, "transientSharpness"),
      timingNaturalness: formNumber(form, "timingNaturalness"),
      audibleCue: form.elements.audibleCue.value,
      cueCode: form.elements.cueCode.value || null,
    });
    form.elements.excerptId.value = "";
    form.elements.audibleCue.value = "";
    form.elements.cueCode.value = "";
    saveAndRender("ABX trial added.");
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

element("routine-form").addEventListener("submit", event => {
  event.preventDefault();
  try {
    const form = event.currentTarget;
    const fields = [
      "durationSeconds", "instructedAttempts", "successfulAttempts", "missedGrabs",
      "unintendedCuts", "pointerLosses", "stuckScratchIncidents", "postReleaseMutes",
      "timingCorrections", "ownershipRating", "timingRating",
    ];
    const values = Object.fromEntries(fields.map(name => [name, formNumber(form, name)]));
    session.addRoutine(form.elements.participantId.value, {
      ...values,
      assistancePreset: form.elements.assistancePreset.value,
      assistanceFollowedIntent: formBoolean(form, "assistanceFollowedIntent"),
      useInRecordedSet: formBoolean(form, "useInRecordedSet"),
      useInLiveSet: formBoolean(form, "useInLiveSet"),
      firstChange: form.elements.firstChange.value,
    });
    saveAndRender("Live-control routine added.");
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

element("preflight-form").addEventListener("submit", event => {
  event.preventDefault();
  try {
    const form = event.currentTarget;
    const checkNames = [
      "level-and-delay-calibration", "mechanics", "transport-rates", "presets",
      "clocks-and-windows", "multi-pointer", "limiter-cells",
    ];
    const numberNames = [
      "levelMismatchDb", "fixedPathDelayMs", "declickBound",
      "maximumAdjacentDiscontinuity", "unexpectedClips", "undecodedZeroExcursions",
      "discontinuitiesAboveBound",
    ];
    session.setPreflight({
      checks: Object.fromEntries(checkNames.map(name => [name, formBoolean(form, name)])),
      ...Object.fromEntries(numberNames.map(name => [name, formNumber(form, name)])),
    });
    saveAndRender("Preflight results saved.");
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

element("protocol-form").addEventListener("submit", event => {
  event.preventDefault();
  try {
    const form = event.currentTarget;
    session.setBlinding({
      participantConditionLabelsHidden: formBoolean(form, "participantConditionLabelsHidden"),
      operatorConditionLabelsHidden: formBoolean(form, "operatorConditionLabelsHidden"),
      assistancePresetHidden: formBoolean(form, "assistancePresetHidden"),
      randomizationGeneratedBeforeSession: formBoolean(form, "randomizationGeneratedBeforeSession"),
      decodedAfterResultsFrozen: formBoolean(form, "decodedAfterResultsFrozen"),
    });
    session.setCueCoding({
      coderCount: formNumber(form, "coderCount"),
      conditionLabelsHidden: formBoolean(form, "conditionLabelsHidden"),
      differencesResolvedBeforeUnblinding: formBoolean(form, "differencesResolvedBeforeUnblinding"),
    });
    saveAndRender("Study controls saved.");
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

element("exclusion-form").addEventListener("submit", event => {
  event.preventDefault();
  try {
    const form = event.currentTarget;
    session.addExclusion({
      id: form.elements.id.value,
      reason: form.elements.reason.value,
      decidedBeforeUnblinding: formBoolean(form, "decidedBeforeUnblinding"),
    });
    form.reset();
    saveAndRender("Exclusion added.");
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

function buildArtifactForm() {
  const form = element("artifact-form");
  form.replaceChildren(...DJ_REQUIRED_ARTIFACT_ROLES.map(role => {
    const row = document.createElement("div");
    row.className = "artifact-row";
    const title = document.createElement("strong");
    title.textContent = role;
    const pathLabel = document.createElement("label");
    pathLabel.textContent = "Relative path";
    const path = document.createElement("input");
    path.name = `${role}-path`;
    path.autocomplete = "off";
    pathLabel.append(path);
    const hashLabel = document.createElement("label");
    hashLabel.textContent = "SHA-256";
    const hash = document.createElement("input");
    hash.name = `${role}-sha256`;
    hash.pattern = "[0-9a-fA-F]{64}";
    hash.autocomplete = "off";
    hashLabel.append(hash);
    row.append(title, pathLabel, hashLabel);
    return row;
  }));
}

element("save-artifacts").addEventListener("click", () => {
  try {
    const form = element("artifact-form");
    for (const role of DJ_REQUIRED_ARTIFACT_ROLES) {
      session.setArtifact(role, {
        path: form.elements.namedItem(`${role}-path`).value,
        sha256: form.elements.namedItem(`${role}-sha256`).value,
      });
    }
    saveAndRender("Artifact paths saved.");
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

element("import-results").addEventListener("change", async event => {
  const file = event.target.files?.[0];
  if (!file) return;
  try {
    replaceSession(JSON.parse(await file.text()));
    persist();
    setStatus("Draft opened.");
  } catch (error) {
    setStatus(error.message || String(error), true);
  } finally {
    event.target.value = "";
  }
});

element("restore-draft").addEventListener("click", () => {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) || "null");
    if (!saved?.results) throw new Error("No local draft is available");
    replaceSession(saved.results, saved.movementTraces);
    setStatus("Local draft restored.");
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

element("clear-draft").addEventListener("click", () => {
  if (!confirm("Clear the local validation draft? Export it first if you need to keep it.")) return;
  localStorage.removeItem(STORAGE_KEY);
  replaceSession(createDjValidationTemplate({ includeExample: false }));
  movementTraces = [];
  if (currentBuildInfo?.commit) session.setBuildInfo(currentBuildInfo);
  setStatus("Local draft cleared.");
});

element("export-results").addEventListener("click", async () => {
  try {
    const results = session.exportResults();
    const shortCommit = /^[0-9a-f]{7,40}$/i.test(results.candidate.commit)
      ? results.candidate.commit.slice(0, 12)
      : "uncommitted";
    if (movementTraces.length > 0) {
      const traceName = `movement-trace-${shortCommit}.json`;
      const traceJson = `${JSON.stringify({ schemaVersion: 1, traces: movementTraces }, null, 2)}\n`;
      session.setArtifact("movement-trace", {
        path: traceName,
        sha256: await sha256Hex(traceJson),
      });
      download(traceName, traceJson);
    }
    const finalResults = session.exportResults();
    download(`dj-validation-${shortCommit}.json`, `${JSON.stringify(finalResults, null, 2)}\n`);
    persist();
    setStatus("Results exported. Run the Node analyzer before accepting the build.");
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

buildArtifactForm();
refreshParticipantSelects();
populateForms();
render();
void loadBuildInfo();
void refreshInputs();

globalThis.__VINYL_DJ_VALIDATION__ = Object.freeze({
  getSession: () => session,
  getPlayer: () => player,
  getBuildInfo: () => currentBuildInfo,
  getPointerInputProfile: () => session.snapshot().environment.pointerInputProfile,
});
