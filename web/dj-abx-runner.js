import {
  DjBlindAbxSession,
  validateDjAbxBlindManifest,
} from "./dj-abx.js";

const element = id => document.getElementById(id);
const packageInput = element("package-input");
const participantInput = element("participant-id");
const startButton = element("start-session");
const trialPanel = element("trial-panel");
const completePanel = element("complete-panel");
const responseForm = element("response-form");
const audio = element("blind-audio");
const roleButtons = [...document.querySelectorAll("[data-role]")];
let manifest = null;
let manifestSha256 = null;
let packageFiles = new Map();
let session = null;
let activeAudioUrl = null;
let activeRole = null;

function setStatus(message, error = false) {
  for (const output of [element("setup-status"), element("runner-status")]) {
    output.textContent = message;
    output.classList.toggle("error", error);
  }
}

function storageKey(participantId) {
  return `vinyl-dj-abx-v2:${manifestSha256}:${participantId}`;
}

async function sha256Hex(bytes) {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(digest)].map(value => value.toString(16).padStart(2, "0")).join("");
}

function relativePackagePath(file, root) {
  const path = file.webkitRelativePath || file.name;
  return root && path.startsWith(root) ? path.slice(root.length) : path;
}

async function loadPackageFiles(filesInput) {
  const files = [...filesInput];
  const manifestFile = files.find(file => file.name === "blind-manifest.json");
  if (!manifestFile) throw new Error("The selected directory has no blind-manifest.json");
  const manifestBytes = await manifestFile.arrayBuffer();
  const parsed = validateDjAbxBlindManifest(JSON.parse(new TextDecoder().decode(manifestBytes)));
  setStatus(`Verifying ${parsed.trials.length * 3} blind audio files…`);
  const manifestPath = manifestFile.webkitRelativePath || manifestFile.name;
  const root = manifestPath.slice(0, manifestPath.length - manifestFile.name.length);
  const mappedFiles = new Map(files.map(file => [relativePackagePath(file, root), file]));
  for (const trial of parsed.trials) {
    for (const descriptor of Object.values(trial.audio)) {
      const file = mappedFiles.get(descriptor.path);
      if (!file) throw new Error(`The blind package is missing an audio file for ${trial.id}`);
      const actualSha256 = await sha256Hex(await file.arrayBuffer());
      if (actualSha256 !== descriptor.sha256) {
        throw new Error(`Blind package audio integrity failed for ${trial.id}`);
      }
    }
  }
  manifest = parsed;
  manifestSha256 = await sha256Hex(manifestBytes);
  packageFiles = mappedFiles;
  session = null;
  participantInput.value = manifest.participantId;
  participantInput.readOnly = true;
  element("package-state").textContent = [
    manifest.studyId,
    `${manifest.trials.length} trials`,
    `build ${manifest.candidate.commit.slice(0, 12)}`,
    manifestSha256.slice(0, 12),
  ].join(" · ");
  element("progress-state").textContent = `0 / ${manifest.trials.length}`;
  startButton.disabled = false;
  trialPanel.hidden = true;
  completePanel.hidden = true;
  setStatus("Blind package loaded. Enter the participant ID.");
  return { manifest: structuredClone(manifest), manifestSha256 };
}

function stopAudio() {
  audio.pause();
  audio.removeAttribute("src");
  audio.load();
  if (activeAudioUrl) URL.revokeObjectURL(activeAudioUrl);
  activeAudioUrl = null;
  activeRole = null;
  roleButtons.forEach(button => button.classList.remove("playing"));
}

function renderTrial() {
  const progress = session.progress;
  element("progress-state").textContent = `${progress.completed} / ${progress.total}`;
  if (progress.finished) {
    stopAudio();
    trialPanel.hidden = true;
    completePanel.hidden = false;
    return;
  }
  completePanel.hidden = true;
  trialPanel.hidden = false;
  const trial = session.currentTrial;
  element("trial-title").textContent = `Trial ${progress.completed + 1} of ${progress.total}`;
  element("gesture-family").textContent = trial.gestureFamily.replaceAll("-", " ");
  roleButtons.forEach(button => button.classList.remove("heard", "playing"));
  responseForm.reset();
  setStatus("Listen to A, B and X in any order.");
}

function restoredResponses() {
  try {
    const draft = JSON.parse(localStorage.getItem(storageKey(participantInput.value.trim())) || "null");
    if (!draft
      || draft.studyId !== manifest.studyId
      || draft.manifestSha256 !== manifestSha256
      || draft.participantId !== participantInput.value.trim()
      || !Array.isArray(draft.responses)) return [];
    return draft.responses;
  } catch {
    return [];
  }
}

function startSession() {
  if (!manifest) throw new Error("Select the blind package first");
  const participantId = participantInput.value.trim();
  if (!participantId) throw new Error("Enter the anonymized participant ID");
  session = new DjBlindAbxSession({
    manifest,
    manifestSha256,
    participantId,
    responses: restoredResponses(),
  });
  participantInput.disabled = true;
  packageInput.disabled = true;
  startButton.disabled = true;
  renderTrial();
}

async function playRole(role) {
  if (!session?.currentTrial) return;
  stopAudio();
  const file = packageFiles.get(session.currentTrial.audio[role].path);
  if (!file) throw new Error("The requested blind audio file is unavailable");
  activeRole = role;
  activeAudioUrl = URL.createObjectURL(file);
  audio.src = activeAudioUrl;
  const button = roleButtons.find(value => value.dataset.role === role);
  button.classList.add("playing");
  setStatus(`Playing ${role.toUpperCase()}…`);
  await audio.play();
}

audio.addEventListener("ended", () => {
  if (!session || !activeRole) return;
  session.markListened(activeRole);
  const button = roleButtons.find(value => value.dataset.role === activeRole);
  button.classList.remove("playing");
  button.classList.add("heard");
  setStatus(`${activeRole.toUpperCase()} heard. Continue when ready.`);
  activeRole = null;
});

audio.addEventListener("error", () => setStatus("This browser could not play the blind WAV file.", true));

packageInput.addEventListener("change", async event => {
  try {
    await loadPackageFiles(event.target.files || []);
  } catch (error) {
    setStatus(error.message || String(error), true);
    startButton.disabled = true;
  }
});

startButton.addEventListener("click", () => {
  try {
    startSession();
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

for (const button of roleButtons) {
  button.addEventListener("click", async () => {
    try {
      await playRole(button.dataset.role);
    } catch (error) {
      setStatus(error.message || String(error), true);
    }
  });
}

responseForm.addEventListener("submit", event => {
  event.preventDefault();
  try {
    const form = event.currentTarget;
    session.recordResponse({
      responseLabel: form.elements.responseLabel.value,
      confidence: Number(form.elements.confidence.value),
      realism: Number(form.elements.realism.value),
      transientSharpness: Number(form.elements.transientSharpness.value),
      timingNaturalness: Number(form.elements.timingNaturalness.value),
      audibleCue: form.elements.audibleCue.value,
    });
    localStorage.setItem(storageKey(session.participantId), JSON.stringify(session.exportDraft()));
    stopAudio();
    renderTrial();
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

function download(name, content) {
  const url = URL.createObjectURL(new Blob([content], { type: "application/json" }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 1_000);
}

element("export-responses").addEventListener("click", () => {
  try {
    const output = session.exportResponses();
    download(`blind-abx-${manifest.studyId}-${session.participantId}.json`, `${JSON.stringify(output, null, 2)}\n`);
    setStatus("Blind responses exported.");
  } catch (error) {
    setStatus(error.message || String(error), true);
  }
});

globalThis.__VINYL_DJ_ABX__ = Object.freeze({
  loadPackageFiles,
  getManifest: () => manifest && structuredClone(manifest),
  getManifestSha256: () => manifestSha256,
  getSession: () => session,
});
