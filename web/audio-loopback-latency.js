const CAPTURE_PROCESSOR_NAME = "vinyl-acoustic-loopback-capture";
const CAPTURE_WORKLET_URL = new URL("./audio-loopback-capture-worklet.js", import.meta.url);
const workletRegistrations = new WeakMap();

function finite(value, fallback) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function positive(value, fallback) {
  const number = finite(value, fallback);
  return number > 0 ? number : fallback;
}

function clamp(value, minimum, maximum) {
  return Math.min(maximum, Math.max(minimum, value));
}

function percentile(sorted, fraction) {
  if (sorted.length === 0) return null;
  if (sorted.length === 1) return sorted[0];
  const position = clamp(fraction, 0, 1) * (sorted.length - 1);
  const lower = Math.floor(position);
  const upper = Math.ceil(position);
  const blend = position - lower;
  return sorted[lower] * (1 - blend) + sorted[upper] * blend;
}

function sleep(milliseconds) {
  return new Promise(resolve => globalThis.setTimeout(resolve, milliseconds));
}

async function registerCaptureWorklet(context) {
  let registration = workletRegistrations.get(context);
  if (!registration) {
    registration = context.audioWorklet.addModule(CAPTURE_WORKLET_URL);
    workletRegistrations.set(context, registration);
  }
  try {
    await registration;
  } catch (error) {
    workletRegistrations.delete(context);
    throw error;
  }
}

/**
 * Creates a deterministic, band-limited logarithmic sweep for acoustic
 * loopback correlation. This signal is diagnostic audio, not player DSP.
 */
export function createAcousticLoopbackProbe(sampleRate, { durationMs = 32 } = {}) {
  const rate = positive(sampleRate, 48_000);
  const durationSeconds = clamp(positive(durationMs, 32), 12, 100) / 1_000;
  const length = Math.max(32, Math.round(rate * durationSeconds));
  const startFrequency = Math.min(700, rate * 0.04);
  const endFrequency = Math.min(9_000, rate * 0.35);
  const sweepRate = Math.log(endFrequency / startFrequency) / durationSeconds;
  const probe = new Float32Array(length);
  for (let index = 0; index < length; index += 1) {
    const time = index / rate;
    const phase = 2 * Math.PI * startFrequency * Math.expm1(sweepRate * time) / sweepRate;
    const window = Math.sin(Math.PI * index / Math.max(1, length - 1)) ** 2;
    probe[index] = Math.sin(phase) * window;
  }
  return probe;
}

function correlationAt(captured, probe, offset, stride) {
  let dot = 0;
  let captureEnergy = 0;
  let probeEnergy = 0;
  for (let index = 0; index < probe.length; index += stride) {
    const capture = captured[offset + index];
    const reference = probe[index];
    dot += capture * reference;
    captureEnergy += capture * capture;
    probeEnergy += reference * reference;
  }
  const denominator = Math.sqrt(captureEnergy * probeEnergy);
  return denominator > 1e-12 ? clamp(dot / denominator, -1, 1) : 0;
}

/**
 * Finds one probe in captured input. Frame numbers use the AudioContext clock.
 */
export function detectAcousticLoopbackProbe({
  captured,
  captureStartFrame,
  probe,
  expectedOutputFrame,
  sampleRate,
  minimumLatencyMs = 0,
  maximumLatencyMs = 500,
  searchStride = 4,
}) {
  if (!(captured instanceof Float32Array) || !(probe instanceof Float32Array)) {
    throw new TypeError("captured and probe must be Float32Array values");
  }
  const rate = positive(sampleRate, 48_000);
  const stride = Math.max(1, Math.floor(positive(searchStride, 4)));
  const minimumFrames = Math.max(0, Math.floor(finite(minimumLatencyMs, 0) * rate / 1_000));
  const maximumFrames = Math.max(minimumFrames, Math.ceil(positive(maximumLatencyMs, 500) * rate / 1_000));
  const expectedOffset = Math.round(finite(expectedOutputFrame, 0) - finite(captureStartFrame, 0));
  const firstOffset = Math.max(0, expectedOffset + minimumFrames);
  const finalOffset = Math.min(captured.length - probe.length, expectedOffset + maximumFrames);
  if (finalOffset < firstOffset) return null;

  let bestOffset = firstOffset;
  let bestCorrelation = -Infinity;
  for (let offset = firstOffset; offset <= finalOffset; offset += stride) {
    const correlation = correlationAt(captured, probe, offset, stride);
    if (correlation > bestCorrelation) {
      bestCorrelation = correlation;
      bestOffset = offset;
    }
  }

  const refineStart = Math.max(firstOffset, bestOffset - stride);
  const refineEnd = Math.min(finalOffset, bestOffset + stride);
  bestCorrelation = -Infinity;
  for (let offset = refineStart; offset <= refineEnd; offset += 1) {
    const correlation = correlationAt(captured, probe, offset, 1);
    if (correlation > bestCorrelation) {
      bestCorrelation = correlation;
      bestOffset = offset;
    }
  }

  const detectedInputFrame = Math.round(captureStartFrame) + bestOffset;
  const latencyFrames = detectedInputFrame - Math.round(expectedOutputFrame);
  return Object.freeze({
    expectedOutputFrame: Math.round(expectedOutputFrame),
    detectedInputFrame,
    latencyFrames,
    latencyMs: latencyFrames * 1_000 / rate,
    correlation: bestCorrelation,
  });
}

export function summarizeAcousticLoopbackDetections(
  detections,
  { minimumCorrelation = 0.15, minimumDetections = 3 } = {},
) {
  if (!Array.isArray(detections)) throw new TypeError("detections must be an array");
  const accepted = detections.filter(detection => (
    Number.isFinite(detection?.latencyMs)
    && detection.latencyMs >= 0
    && Number.isFinite(detection?.correlation)
    && detection.correlation >= minimumCorrelation
  ));
  if (accepted.length < Math.max(1, Math.floor(minimumDetections))) {
    throw new Error(`Only ${accepted.length} acoustic probes passed the correlation threshold`);
  }
  const latenciesMs = accepted.map(detection => detection.latencyMs).sort((left, right) => left - right);
  const correlations = accepted.map(detection => detection.correlation);
  return Object.freeze({
    samples: accepted.length,
    medianMs: percentile(latenciesMs, 0.5),
    p95Ms: percentile(latenciesMs, 0.95),
    maximumMs: latenciesMs.at(-1),
    minimumMs: latenciesMs[0],
    jitterMs: latenciesMs.at(-1) - latenciesMs[0],
    minimumCorrelation: Math.min(...correlations),
    latenciesMs: Object.freeze(latenciesMs),
  });
}

function assembleCapture(packets) {
  if (packets.length === 0) throw new Error("The microphone did not produce input samples");
  const firstFrame = Math.min(...packets.map(packet => packet.startFrame));
  const finalFrame = Math.max(...packets.map(packet => packet.startFrame + packet.samples.length));
  const captured = new Float32Array(finalFrame - firstFrame);
  for (const packet of packets) captured.set(packet.samples, packet.startFrame - firstFrame);
  return { captured, captureStartFrame: firstFrame };
}

/**
 * Measures output-to-input latency through the selected physical audio path.
 * Keep the microphone path muted. The caller must stop programme playback.
 */
export async function measureAcousticLoopbackLatency(context, {
  inputStream = null,
  mediaDevices = globalThis.navigator?.mediaDevices,
  inputDeviceId = null,
  outputDestination = context?.destination,
  amplitude = 0.08,
  repetitions = 5,
  maximumLatencyMs = 500,
  minimumCorrelation = 0.15,
  leadInMs = 500,
} = {}) {
  if (!context?.audioWorklet || typeof context.createBufferSource !== "function") {
    throw new TypeError("A Web Audio AudioContext is required");
  }
  const sampleRate = positive(context.sampleRate, 48_000);
  const repeatCount = clamp(Math.floor(positive(repetitions, 5)), 3, 12);
  const peak = clamp(positive(amplitude, 0.08), 0.005, 0.25);
  const maximumDelay = clamp(positive(maximumLatencyMs, 500), 50, 2_000);
  const probe = createAcousticLoopbackProbe(sampleRate);
  const intervalFrames = Math.ceil((maximumDelay + 150) * sampleRate / 1_000) + probe.length;
  const totalFrames = intervalFrames * (repeatCount - 1) + probe.length;
  const packets = [];
  let ownedStream = null;
  let mediaSource = null;
  let captureNode = null;
  let muteNode = null;
  let probeSource = null;
  let probeGain = null;

  try {
    if (!inputStream) {
      if (typeof mediaDevices?.getUserMedia !== "function") {
        throw new Error("Microphone capture is not available in this browser");
      }
      ownedStream = await mediaDevices.getUserMedia({
        audio: {
          echoCancellation: false,
          noiseSuppression: false,
          autoGainControl: false,
          ...(inputDeviceId ? { deviceId: { exact: String(inputDeviceId) } } : {}),
        },
      });
      inputStream = ownedStream;
    }
    if (!inputStream?.getAudioTracks?.().length) throw new Error("The input stream has no audio track");

    await context.resume();
    await registerCaptureWorklet(context);
    mediaSource = context.createMediaStreamSource(inputStream);
    captureNode = new AudioWorkletNode(context, CAPTURE_PROCESSOR_NAME, {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      outputChannelCount: [1],
    });
    muteNode = context.createGain();
    muteNode.gain.value = 0;
    mediaSource.connect(captureNode).connect(muteNode).connect(context.destination);
    captureNode.port.onmessage = event => {
      if (event.data?.type !== "capture" || !(event.data.samples instanceof Float32Array)) return;
      packets.push({
        startFrame: Math.round(event.data.startFrame),
        samples: event.data.samples,
      });
    };

    const buffer = context.createBuffer(1, totalFrames, sampleRate);
    const output = buffer.getChannelData(0);
    const expectedFrames = [];
    const startTime = context.currentTime + clamp(positive(leadInMs, 500), 250, 2_000) / 1_000;
    const startFrame = Math.round(startTime * sampleRate);
    for (let index = 0; index < repeatCount; index += 1) {
      const offset = index * intervalFrames;
      output.set(probe, offset);
      expectedFrames.push(startFrame + offset);
    }

    probeSource = context.createBufferSource();
    probeSource.buffer = buffer;
    probeGain = context.createGain();
    probeGain.gain.value = peak;
    probeSource.connect(probeGain).connect(outputDestination);
    const ended = new Promise(resolve => { probeSource.onended = resolve; });
    probeSource.start(startTime);
    await ended;
    await sleep(maximumDelay + 150);
    captureNode.port.postMessage({ type: "stop" });
    await sleep(50);

    const { captured, captureStartFrame } = assembleCapture(packets);
    const detections = expectedFrames.map(expectedOutputFrame => detectAcousticLoopbackProbe({
      captured,
      captureStartFrame,
      probe,
      expectedOutputFrame,
      sampleRate,
      maximumLatencyMs: maximumDelay,
    })).filter(Boolean);
    const summary = summarizeAcousticLoopbackDetections(detections, {
      minimumCorrelation,
      minimumDetections: Math.min(3, repeatCount),
    });
    return Object.freeze({
      ...summary,
      sampleRate,
      repetitionsRequested: repeatCount,
      maximumLatencyMs: maximumDelay,
      amplitude: peak,
      inputDeviceLabel: inputStream.getAudioTracks()[0]?.label || "",
      inputDeviceSettings: Object.freeze({ ...(inputStream.getAudioTracks()[0]?.getSettings?.() || {}) }),
      outputDeviceId: typeof context.sinkId === "string" ? context.sinkId : null,
      detections: Object.freeze(detections),
    });
  } finally {
    if (probeSource) {
      try { probeSource.stop(); } catch {}
    }
    probeSource?.disconnect();
    probeGain?.disconnect();
    captureNode?.port?.postMessage({ type: "stop" });
    captureNode?.disconnect();
    mediaSource?.disconnect();
    muteNode?.disconnect();
    for (const track of ownedStream?.getTracks?.() || []) track.stop();
  }
}
