export const PCM_SEAM_HALF_SAMPLES = 12;
export const PCM_SEAM_TOTAL_SAMPLES = PCM_SEAM_HALF_SAMPLES * 2;

export function clampPcmWindowValue(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

export function mergePcmWrittenRange(ranges, start, end) {
  const nextStart = Math.max(0, Math.floor(Number(start) || 0));
  const nextEnd = Math.max(nextStart, Math.floor(Number(end) || 0));
  if (nextEnd <= nextStart) return ranges;

  const merged = [];
  let low = nextStart;
  let high = nextEnd;
  let inserted = false;
  for (const range of ranges) {
    if (range[1] < low) {
      merged.push([range[0], range[1]]);
    } else if (high < range[0]) {
      if (!inserted) {
        merged.push([low, high]);
        inserted = true;
      }
      merged.push([range[0], range[1]]);
    } else {
      low = Math.min(low, range[0]);
      high = Math.max(high, range[1]);
    }
  }
  if (!inserted) merged.push([low, high]);
  ranges.splice(0, ranges.length, ...merged);
  return ranges;
}

export function contiguousPcmEnd(ranges) {
  return ranges.length > 0 && ranges[0][0] === 0 ? ranges[0][1] : 0;
}

export function pcmRangeIsWritten(ranges, start, end) {
  return ranges.some(range => range[0] <= start && range[1] >= end);
}

function cubicHermite(y0, y1, m0, m1, t, span) {
  const t2 = t * t;
  const t3 = t2 * t;
  const h00 = (2 * t3) - (3 * t2) + 1;
  const h10 = t3 - (2 * t2) + t;
  const h01 = (-2 * t3) + (3 * t2);
  const h11 = t3 - t2;
  return (h00 * y0) + (h10 * span * m0) + (h01 * y1) + (h11 * span * m1);
}

// Repairs the canonical Int16 source, before any shared or transferred window
// can reach the AudioWorklet. Both forward and reverse playback therefore read
// the exact same replacement samples.
export function repairPcmSeam(channels, boundaryFrame) {
  const boundary = Math.floor(Number(boundaryFrame));
  if (!(boundary > 0) || !Array.isArray(channels) || !channels.length) return null;
  const leftAnchor = boundary - PCM_SEAM_HALF_SAMPLES - 1;
  const rightAnchor = boundary + PCM_SEAM_HALF_SAMPLES;
  if (leftAnchor < 1) return null;
  const span = rightAnchor - leftAnchor;
  let repairedChannels = 0;
  let maxBeforeJump = 0;
  let maxAfterJump = 0;

  for (const channel of channels) {
    if (!(channel instanceof Int16Array) || rightAnchor + 1 >= channel.length) continue;
    const beforeJump = Math.abs(channel[boundary] - channel[boundary - 1]);
    const y0 = channel[leftAnchor];
    const y1 = channel[rightAnchor];
    const m0 = y0 - channel[leftAnchor - 1];
    const m1 = channel[rightAnchor + 1] - y1;
    for (let index = leftAnchor + 1; index < rightAnchor; index += 1) {
      const t = (index - leftAnchor) / span;
      channel[index] = Math.round(clampPcmWindowValue(
        cubicHermite(y0, y1, m0, m1, t, span),
        -32768,
        32767,
      ));
    }
    maxBeforeJump = Math.max(maxBeforeJump, beforeJump);
    maxAfterJump = Math.max(maxAfterJump, Math.abs(channel[boundary] - channel[boundary - 1]));
    repairedChannels += 1;
  }

  return repairedChannels > 0 ? {
    boundaryFrame: boundary,
    channels: repairedChannels,
    samples: PCM_SEAM_TOTAL_SAMPLES,
    maxBeforeJump,
    maxAfterJump,
  } : null;
}

export function planPcmWindow({ position, totalFrames, availableEnd, windowFrames }) {
  const total = Math.max(1, Math.floor(Number(totalFrames) || 1));
  const available = clampPcmWindowValue(Math.floor(Number(availableEnd) || 0), 0, total);
  const capacity = Math.max(1, Math.min(total, Math.floor(Number(windowFrames) || total)));
  const requestedPosition = clampPcmWindowValue(Number(position) || 0, 0, total - 1);
  if (available <= 0 || requestedPosition >= available) return null;
  const half = Math.floor(capacity / 2);
  const maxStart = Math.max(0, available - capacity);
  const start = clampPcmWindowValue(Math.round(requestedPosition) - half, 0, maxStart);
  const end = Math.min(total, available, start + capacity);
  return {
    start,
    end,
    length: Math.max(0, end - start),
    position: requestedPosition,
  };
}

export function copyPcmWindow(channels, plan, targets = null) {
  if (!plan || plan.length <= 0) return [];
  return channels.map((channel, channelIndex) => {
    const target = targets?.[channelIndex] || new Float32Array(plan.length);
    if (!(target instanceof Float32Array) || target.length < plan.length) {
      throw new RangeError(`PCM window target ${channelIndex} is shorter than ${plan.length} frames`);
    }
    target.fill(0);
    for (let frame = 0; frame < plan.length; frame += 1) {
      target[frame] = channel[plan.start + frame] / 32768;
    }
    return target;
  });
}
