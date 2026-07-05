import {
  FONT_FAMILY,
  PITCH_RANGE_PCT,
  arcValue,
  clamp,
  degToRad,
  minuteToDegrees,
  normalizeDegrees
} from "./player-canvas-geometry.js";

function drawCurvedText(ctx, geometry, text, angleDeg, radius, color, size, flip = false) {
  const chars = Array.from(String(text || ""));
  if (!chars.length) return;
  ctx.save();
  ctx.font = `700 ${size}px ${FONT_FAMILY}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillStyle = color;
  const widths = chars.map(char => ctx.measureText(char).width);
  const total = widths.reduce((sum, width) => sum + width, 0);
  let offset = -total / 2;
  const centerAngle = degToRad(angleDeg);
  chars.forEach((char, index) => {
    const charOffset = offset + widths[index] / 2;
    const angle = centerAngle + (flip ? -charOffset : charOffset) / radius;
    const x = geometry.cx + Math.cos(angle) * radius;
    const y = geometry.cy + Math.sin(angle) * radius;
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(angle + (flip ? -Math.PI / 2 : Math.PI / 2));
    ctx.fillText(char, 0, 0);
    ctx.restore();
    offset += widths[index];
  });
  ctx.restore();
}

function strokeArcBand(ctx, geometry, startDeg, endDeg, inner, outer, color) {
  const anticlockwise = endDeg < startDeg;
  ctx.beginPath();
  ctx.arc(geometry.cx, geometry.cy, outer, degToRad(startDeg), degToRad(endDeg), anticlockwise);
  ctx.arc(geometry.cx, geometry.cy, inner, degToRad(endDeg), degToRad(startDeg), !anticlockwise);
  ctx.closePath();
  ctx.strokeStyle = color;
  ctx.lineWidth = 1;
  ctx.stroke();
}

function radialTick(ctx, geometry, angleDeg, r0, r1, color, width = 1) {
  const angle = degToRad(angleDeg);
  ctx.beginPath();
  ctx.moveTo(geometry.cx + Math.cos(angle) * r0, geometry.cy + Math.sin(angle) * r0);
  ctx.lineTo(geometry.cx + Math.cos(angle) * r1, geometry.cy + Math.sin(angle) * r1);
  ctx.strokeStyle = color;
  ctx.lineWidth = width;
  ctx.stroke();
}

function sliderAngle(segment, value) {
  const normalized = clamp(value, 0, 1);
  if (Number.isFinite(segment.neutralAngle) && Number.isFinite(segment.neutralValue)) {
    const neutralValue = clamp(segment.neutralValue, 0, 1);
    if (normalized >= neutralValue) {
      const ratio = neutralValue < 1 ? (normalized - neutralValue) / (1 - neutralValue) : 0;
      return segment.neutralAngle + (segment.endAngle - segment.neutralAngle) * ratio;
    }
    const ratio = neutralValue > 0 ? (neutralValue - normalized) / neutralValue : 0;
    return segment.neutralAngle + (segment.startAngle - segment.neutralAngle) * ratio;
  }
  return segment.startAngle + (segment.endAngle - segment.startAngle) * (segment.reverse ? 1 - normalized : normalized);
}

function drawEndLabels(ctx, geometry, segment, theme, inner, outer) {
  if (!segment.endLabels) return;
  const radius = outer + 9 * geometry.scale;
  const size = 7 * geometry.scale;
  drawCurvedText(ctx, geometry, segment.endLabels[0], segment.startAngle, radius, theme.controlText, size, segment.flip);
  drawCurvedText(ctx, geometry, segment.endLabels[1], segment.endAngle, radius, theme.controlText, size, segment.flip);
}

function drawSlider(ctx, geometry, segment, theme, labels, hitRegions) {
  const inner = geometry.controlBandInner;
  const outer = geometry.controlBandOuter;
  const mid = (inner + outer) / 2;
  strokeArcBand(ctx, geometry, segment.startAngle, segment.endAngle, inner, outer, theme.line);
  const angle = sliderAngle(segment, segment.value);
  radialTick(ctx, geometry, angle, inner - 1, outer + 1, theme.accent, Math.max(2, 2 * geometry.scale));
  if (Number.isFinite(segment.defaultValue)) {
    radialTick(ctx, geometry, sliderAngle(segment, segment.defaultValue), inner, outer, theme.mutedLine, 1);
  }
  if (segment.key === "rpm") {
    for (let pct = -PITCH_RANGE_PCT; pct <= PITCH_RANGE_PCT; pct += 1) {
      const ratio = 0.5 + pct / (PITCH_RANGE_PCT * 2);
      const tickAngle = sliderAngle(segment, ratio);
      const length = pct % 2 === 0 ? 5 : 3;
      radialTick(ctx, geometry, tickAngle, outer + geometry.scale, outer + length * geometry.scale, theme.mutedLine, 1);
    }
    const ledAngle = sliderAngle(segment, 0.5);
    const ledRadius = outer + 15 * geometry.scale;
    const ledX = geometry.cx + Math.cos(degToRad(ledAngle)) * ledRadius;
    const ledY = geometry.cy + Math.sin(degToRad(ledAngle)) * ledRadius;
    ctx.fillStyle = Math.abs(segment.value - 0.5) < 0.001 ? theme.accent : theme.controlFill;
    ctx.strokeStyle = theme.accent;
    ctx.lineWidth = 1;
    ctx.fillRect(ledX - 3 * geometry.scale, ledY - 3 * geometry.scale, 6 * geometry.scale, 6 * geometry.scale);
    ctx.strokeRect(ledX - 3 * geometry.scale, ledY - 3 * geometry.scale, 6 * geometry.scale, 6 * geometry.scale);
  }
  if (labels) {
    drawCurvedText(ctx, geometry, segment.label, segment.labelAngle, mid, theme.controlText, 10 * geometry.scale, segment.flip);
    if (segment.valueLabel) {
      drawCurvedText(ctx, geometry, segment.valueLabel, angle, mid, theme.accent, 8 * geometry.scale, segment.flip);
    }
    drawEndLabels(ctx, geometry, segment, theme, inner, outer);
  }
  hitRegions.push({
    key: segment.key,
    kind: "arc-slider",
    startAngle: segment.startAngle,
    endAngle: segment.endAngle,
    inner: inner - 12 * geometry.scale,
    outer: outer + 18 * geometry.scale,
    reverse: segment.reverse,
    onInput: segment.onInput
  });
}

function drawChevron(ctx, x, y, size, direction, color) {
  const halfW = size * 0.5;
  const halfH = size * 0.4;
  ctx.beginPath();
  if (direction === "up") {
    ctx.moveTo(x - halfW, y + halfH);
    ctx.lineTo(x, y - halfH);
    ctx.lineTo(x + halfW, y + halfH);
  } else {
    ctx.moveTo(x - halfW, y - halfH);
    ctx.lineTo(x, y + halfH);
    ctx.lineTo(x + halfW, y - halfH);
  }
  ctx.closePath();
  ctx.fillStyle = color;
  ctx.fill();
}

function drawNeedleChevrons(ctx, geometry, button, theme, inner, outer) {
  const angle = degToRad(button.angle);
  const mid = (inner + outer) / 2;
  const size = 10 * geometry.scale;
  const offset = size * 0.55;
  ctx.save();
  ctx.translate(geometry.cx + Math.cos(angle) * mid, geometry.cy + Math.sin(angle) * mid);
  ctx.rotate(angle);
  drawChevron(ctx, 0, -offset, size, "up", button.needleLifted ? theme.accent : theme.controlText);
  drawChevron(ctx, 0, offset, size, "down", button.needleLifted ? theme.controlText : theme.accent);
  ctx.restore();
}

function drawSectorButton(ctx, geometry, button, theme, labels, hitRegions) {
  const inner = geometry.controlBandInner;
  const outer = geometry.controlBandOuter;
  const start = degToRad(button.angle - button.span / 2);
  const end = degToRad(button.angle + button.span / 2);
  ctx.beginPath();
  ctx.arc(geometry.cx, geometry.cy, outer, start, end);
  ctx.arc(geometry.cx, geometry.cy, inner, end, start, true);
  ctx.closePath();
  ctx.fillStyle = button.active ? theme.controlActive : theme.controlFill;
  ctx.fill();
  ctx.strokeStyle = theme.line;
  ctx.lineWidth = 1;
  ctx.stroke();
  if (button.glyph === "needle-lift") {
    drawNeedleChevrons(ctx, geometry, button, theme, inner, outer);
  } else if (labels) {
    drawCurvedText(
      ctx,
      geometry,
      button.label,
      button.angle,
      (inner + outer) / 2,
      button.active ? theme.controlActiveText : theme.controlText,
      9 * geometry.scale,
      button.angle > 0 && button.angle < 180
    );
  }
  hitRegions.push({
    key: button.key,
    kind: "sector-button",
    angle: normalizeDegrees(button.angle),
    span: button.span,
    inner,
    outer,
    onActivate: button.onActivate
  });
}

export function createControlSegments(player, state, components) {
  const rpmCenter = minuteToDegrees(55);
  const rpmHalf = 22;
  const channelCenter = minuteToDegrees(45);
  const xfadeCenter = minuteToDegrees(30);
  const seekCenter = minuteToDegrees(15);
  const native = Number(state.nativeRpm) || 33.3333333333;
  const pitchRatio = clamp(((Number(state.rpm) || native) / native - 0.92) / 0.16, 0, 1);
  const segments = [];
  if (components.rpm) {
    segments.push({
      key: "rpm",
      label: "PITCH",
      labelAngle: rpmCenter - 12,
      startAngle: rpmCenter - rpmHalf,
      endAngle: rpmCenter + rpmHalf,
      neutralAngle: rpmCenter,
      neutralValue: 0.5,
      value: pitchRatio,
      defaultValue: 0.5,
      valueLabel: `${pitchRatio === 0.5 ? "0.0" : `${pitchRatio > 0.5 ? "+" : "-"}${Math.abs((pitchRatio - 0.5) * 16).toFixed(1)}`}`,
      endLabels: ["-8", "+8"],
      onInput: value => player.setRpm(native * (0.92 + value * 0.16))
    });
  }
  if (components.volume) {
    segments.push({
      key: "volume",
      label: "CH",
      labelAngle: channelCenter,
      startAngle: channelCenter - 18,
      endAngle: channelCenter + 18,
      value: Number(state.volume) || 0,
      defaultValue: 1,
      valueLabel: "",
      endLabels: ["MIN", "MAX"],
      onInput: value => player.setVolume(value)
    });
  }
  if (components.crossfader) {
    segments.push({
      key: "crossfader",
      label: "XFADE",
      labelAngle: xfadeCenter,
      startAngle: xfadeCenter + 15,
      endAngle: xfadeCenter - 15,
      reverse: false,
      value: Number(state.crossfader) || 0,
      defaultValue: 0.5,
      valueLabel: "",
      endLabels: ["CUT", "OPEN"],
      flip: true,
      onInput: value => player.setCrossfader(value)
    });
  }
  if (components.seek) {
    segments.push({
      key: "seek",
      label: "POSITION",
      labelAngle: seekCenter,
      startAngle: seekCenter - 27,
      endAngle: seekCenter + 27,
      value: Number(state.positionRatio) || 0,
      defaultValue: 0,
      valueLabel: "",
      onInput: value => player.seekRatio(value)
    });
  }
  return segments;
}

export function drawRadialControls(ctx, geometry, player, state, components, theme, hitRegions) {
  const segments = createControlSegments(player, state, components);
  segments.forEach(segment => drawSlider(ctx, geometry, segment, theme, components.labels, hitRegions));
  const buttons = [];
  if (components.startStop) {
    buttons.push({
      key: "startStop",
      label: "START - STOP",
      active: false,
      weight: 12,
      onActivate: () => player.togglePlayback()
    });
  }
  if (components.needle) {
    buttons.push({
      key: "needle",
      glyph: "needle-lift",
      active: !state.needleLifted,
      needleLifted: Boolean(state.needleLifted),
      weight: 3,
      onActivate: () => player.setNeedleLifted(!state.needleLifted)
    });
  }
  const start = minuteToDegrees(5);
  const end = minuteToDegrees(25);
  const gap = 3;
  const totalWeight = buttons.reduce((sum, button) => sum + button.weight, 0) || 1;
  const available = Math.abs(end - start) - gap * Math.max(0, buttons.length - 1);
  let cursor = start;
  buttons.forEach(button => {
    const span = available * button.weight / totalWeight;
    drawSectorButton(ctx, geometry, { ...button, angle: cursor + span / 2, span }, theme, components.labels, hitRegions);
    cursor += span + gap;
  });
  return segments;
}

export function controlAt(hitRegions, geometry, point) {
  const dx = point.x - geometry.cx;
  const dy = point.y - geometry.cy;
  const radius = Math.hypot(dx, dy);
  const angle = normalizeDegrees(Math.atan2(dy, dx) * 180 / Math.PI);
  return [...hitRegions].reverse().find(region => {
    if (radius < region.inner || radius > region.outer) return false;
    if (region.kind === "arc-slider") {
      const value = arcValue(angle, region.startAngle, region.endAngle, region.reverse);
      const projected = sliderAngle({ startAngle: region.startAngle, endAngle: region.endAngle, reverse: region.reverse }, value);
      const delta = Math.abs((((angle - projected + 540) % 360) - 180));
      return delta <= 16;
    }
    const delta = Math.abs((((angle - region.angle + 540) % 360) - 180));
    return delta <= region.span / 2 + 2;
  }) || null;
}

export function updateArcControl(region, geometry, point) {
  const angle = normalizeDegrees(Math.atan2(point.y - geometry.cy, point.x - geometry.cx) * 180 / Math.PI);
  region.onInput(arcValue(angle, region.startAngle, region.endAngle, region.reverse));
}
