export const CANVAS_COMPONENTS = Object.freeze({
  record: true,
  syncRings: true,
  syncDots: true,
  strobe: true,
  strobeLamp: true,
  spindle: true,
  stylus: true,
  needlePoint: true,
  tonearmGuide: true,
  startStop: true,
  needle: true,
  loadRecord: true,
  rpm: true,
  volume: true,
  crossfader: true,
  seek: true,
  labels: true
});

export const CANVAS_THEME = Object.freeze({
  background: "transparent",
  line: "#050505",
  mutedLine: "rgba(5,5,5,0.28)",
  controlFill: "rgba(255,255,255,0.08)",
  controlActive: "#050505",
  controlText: "#050505",
  controlActiveText: "#f00020",
  accent: "#00bfd3",
  accentSecondary: "#ef035c",
  accentTertiary: "#f3b511",
  recordFallback: "transparent",
  turntableRing: "rgba(5,5,5,0.18)",
  turntableRingStrong: "rgba(5,5,5,0.34)",
  label: "transparent",
  syncDot: "rgba(0,0,0,0.23)",
  syncLit: "#00bfd3",
  lamp: "#00bfd3",
  tonearm: "#050505",
  tonearmGuide: "#050505",
  stylus: "#00bfd3",
  stylusGlow: "rgba(0,191,211,0.65)"
});

export const STROBE_ROWS = Object.freeze([
  { pitchPct: -3.3, count: 150, dotScale: 0.65 },
  { pitchPct: 0, count: 132, dotScale: 1.3 },
  { pitchPct: 3.3, count: 138, dotScale: 0.65 },
  { pitchPct: 6, count: 120, dotScale: 0.65 }
]);

export const STROBE_LAMP_DEG = 135;
export const STROBE_BEAM_FULL_DEG = 4;
export const STROBE_BEAM_HALF_DEG = 11;
export const STROBE_DEG_PER_SEC = 320;
export const STROBE_CONT_DEG_PER_SEC = 150;
export const PITCH_RANGE_PCT = 8;
export const FONT_FAMILY = '"Archivo Black", "Arial Black", Helvetica, Arial, sans-serif';

export function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

export function degToRad(value) {
  return value * Math.PI / 180;
}

export function radToDeg(value) {
  return value * 180 / Math.PI;
}

export function normalizeDegrees(value) {
  return ((value % 360) + 360) % 360;
}

export function angularDeltaDegrees(a, b) {
  return Math.abs((((a - b + 540) % 360) - 180));
}

export function minuteToDegrees(minute) {
  return minute * 6 - 90;
}

export function pointOnCircle(geometry, angleDeg, radius) {
  const angle = degToRad(angleDeg);
  return {
    x: geometry.cx + Math.cos(angle) * radius,
    y: geometry.cy + Math.sin(angle) * radius
  };
}

export function buildCanvasGeometry(width, height, options = {}) {
  const recordFill = options.recordFill === true;
  const scale = recordFill
    ? Math.max(0.01, Math.min(width, height) / 576)
    : Math.max(0.55, Math.min(width / 800, height / 840));
  const localWidth = 800 * scale;
  const localHeight = 840 * scale;
  const left = (width - localWidth) / 2;
  const top = (height - localHeight) / 2;
  const recordSize = 576 * scale;
  const recordRadius = recordSize / 2;
  const cx = left + localWidth / 2;
  const cy = top + localHeight / 2;
  const buttonHeight = 30 * scale;
  const innerRadius = recordRadius + 3;
  const outerRadius = innerRadius + buttonHeight;
  return {
    width,
    height,
    scale,
    left,
    top,
    localWidth,
    localHeight,
    cx,
    cy,
    recordRadius,
    innerRadius,
    outerRadius,
    buttonHeight,
    controlBandInner: outerRadius + 6 * scale,
    controlBandOuter: outerRadius + 24 * scale
  };
}

export function localPointer(canvas, event) {
  const rect = canvas.getBoundingClientRect();
  return {
    x: event.clientX - rect.left,
    y: event.clientY - rect.top,
    rect
  };
}

export function pointerPolar(geometry, point) {
  const dx = point.x - geometry.cx;
  const dy = point.y - geometry.cy;
  return {
    radius: Math.hypot(dx, dy),
    angle: normalizeDegrees(radToDeg(Math.atan2(dy, dx)))
  };
}

export function unwrapAngleNear(angle, reference) {
  let value = angle;
  while (value - reference > 180) value -= 360;
  while (value - reference < -180) value += 360;
  return value;
}

export function arcValue(angle, startAngle, endAngle, reverse = false) {
  const start = startAngle;
  const end = unwrapAngleNear(endAngle, start);
  const current = unwrapAngleNear(angle, start);
  const low = Math.min(start, end);
  const high = Math.max(start, end);
  const projected = clamp(current, low, high);
  const ratio = Math.abs(end - start) > 0.001 ? (projected - start) / (end - start) : 0;
  return reverse ? 1 - clamp(ratio, 0, 1) : clamp(ratio, 0, 1);
}
