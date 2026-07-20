function clamp(value, minimum, maximum) {
  return Math.max(minimum, Math.min(maximum, value));
}

export function projectPointerOutputFrame({
  sampleRate,
  currentAudioTime,
  inputTimeMs,
  nowMs,
  timeOriginMs,
  requestedOutputFrame,
  maximumInputAgeMs = 1_000,
}) {
  const rate = Number(sampleRate);
  if (!Number.isFinite(rate) || rate <= 0) {
    throw new RangeError("sampleRate must be positive");
  }
  const audioTime = Math.max(0, Number(currentAudioTime) || 0);
  const currentFrame = Math.max(0, Math.round(audioTime * rate));
  let outputFrame = Number(requestedOutputFrame);
  if (!Number.isFinite(outputFrame)) {
    let inputAudioTime = audioTime;
    if (Number.isFinite(Number(inputTimeMs)) && Number.isFinite(Number(nowMs))) {
      let timestamp = Number(inputTimeMs);
      if (timestamp > 1_000_000_000_000 && Number.isFinite(Number(timeOriginMs))) {
        timestamp -= Number(timeOriginMs);
      }
      const ageMs = clamp(
        Number(nowMs) - timestamp,
        0,
        Math.max(0, Number(maximumInputAgeMs) || 0),
      );
      inputAudioTime = Math.max(0, audioTime - ageMs / 1_000);
    }
    outputFrame = Math.round(inputAudioTime * rate);
  }
  return Math.max(0, Math.min(currentFrame, Math.round(outputFrame)));
}
