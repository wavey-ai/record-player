import { clamp, degToRad } from "./player-canvas-geometry.js";

const ARM_PIVOT_ANGLE = Math.PI / 2;
const ARM_TIP_ANGLE = -Math.PI / 4;
const ARM_LENGTH_FACTOR = 1.26;

function tonearmTipForGroove(cx, cy, anchorX, anchorY, pivotDistance, armLength, grooveRadius) {
  const dx = anchorX - cx;
  const dy = anchorY - cy;
  const distance = pivotDistance || 1;
  const a = ((grooveRadius * grooveRadius) - (armLength * armLength) + (distance * distance)) / (2 * distance);
  const h = Math.sqrt(Math.max(0, grooveRadius * grooveRadius - a * a));
  const bx = cx + a * dx / distance;
  const by = cy + a * dy / distance;
  const first = { x: bx + h * -dy / distance, y: by + h * dx / distance };
  const second = { x: bx - h * -dy / distance, y: by - h * dx / distance };
  const score = point => Math.cos(Math.atan2(-(point.y - cy), point.x - cx) - ARM_TIP_ANGLE);
  return score(first) >= score(second) ? first : second;
}

export function resolveStylusGeometry(geometry, state, options = {}) {
  const scale = geometry.scale;
  const anchorX = geometry.cx + (geometry.recordRadius + 24 * scale) * Math.cos(ARM_PIVOT_ANGLE);
  const anchorY = geometry.cy - (geometry.recordRadius + 24 * scale) * Math.sin(ARM_PIVOT_ANGLE) - 50 * scale;
  const pivotDistance = Math.hypot(anchorX - geometry.cx, anchorY - geometry.cy) || 1;
  const armLength = pivotDistance * ARM_LENGTH_FACTOR;
  const progress = clamp(Number(state.positionRatio) || 0, 0, 1);
  const grooveProgress = typeof options.calibrateProgress === "function" ? clamp(options.calibrateProgress(progress), 0, 1) : progress;
  const outerGroove = geometry.recordRadius * 0.94;
  const innerGroove = geometry.recordRadius * 0.38;
  const grooveRadius = outerGroove + (innerGroove - outerGroove) * grooveProgress;
  const tip = tonearmTipForGroove(geometry.cx, geometry.cy, anchorX, anchorY, pivotDistance, armLength, grooveRadius);
  const outerTip = tonearmTipForGroove(geometry.cx, geometry.cy, anchorX, anchorY, pivotDistance, armLength, outerGroove);
  const innerTip = tonearmTipForGroove(geometry.cx, geometry.cy, anchorX, anchorY, pivotDistance, armLength, innerGroove);
  return { anchorX, anchorY, armLength, tip, outerTip, innerTip, grooveRadius, outerGroove, innerGroove };
}

export function drawStylus(ctx, geometry, state, theme, components, timestamp, options = {}) {
  if (!components.stylus && !components.needlePoint) return null;
  const resolved = resolveStylusGeometry(geometry, state, options);
  const armDirection = Math.atan2(resolved.tip.y - resolved.anchorY, resolved.tip.x - resolved.anchorX);
  const tailLength = Math.max(16, 22 * geometry.scale);
  const liftedOffset = state.needleLifted ? Math.max(5, 8 * geometry.scale) : 0;
  const tipX = resolved.tip.x;
  const tipY = resolved.tip.y - liftedOffset;

  if (components.stylus) {
    ctx.save();
    if (components.tonearmGuide) {
      const angle0 = Math.atan2(resolved.outerTip.y - resolved.anchorY, resolved.outerTip.x - resolved.anchorX);
      const angle1 = Math.atan2(resolved.innerTip.y - resolved.anchorY, resolved.innerTip.x - resolved.anchorX);
      ctx.setLineDash([4 * geometry.scale, 5 * geometry.scale]);
      ctx.strokeStyle = theme.tonearmGuide;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.arc(resolved.anchorX, resolved.anchorY, resolved.armLength, Math.min(angle0, angle1), Math.max(angle0, angle1));
      ctx.stroke();
      ctx.setLineDash([]);
    }
    ctx.strokeStyle = theme.tonearm;
    ctx.lineWidth = 1;
    ctx.lineCap = "round";
    ctx.beginPath();
    ctx.moveTo(
      resolved.anchorX - Math.cos(armDirection) * tailLength,
      resolved.anchorY - Math.sin(armDirection) * tailLength
    );
    ctx.lineTo(tipX, tipY);
    ctx.stroke();
    const hub = Math.max(5, 6 * geometry.scale);
    ctx.fillStyle = theme.tonearm;
    ctx.beginPath();
    ctx.arc(resolved.anchorX, resolved.anchorY, hub, 0, Math.PI * 2);
    ctx.fill();
    ctx.strokeStyle = theme.tonearm;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.arc(resolved.anchorX, resolved.anchorY, hub + 3 * geometry.scale, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
  }

  if (components.needlePoint) {
    const active = !state.needleLifted && Boolean(state.playing || state.scratching);
    const pulse = active ? 0.75 + Math.sin(timestamp / 62) * 0.2 : 0.4;
    ctx.save();
    ctx.shadowColor = theme.stylusGlow;
    ctx.shadowBlur = active ? 16 * geometry.scale : 5 * geometry.scale;
    ctx.globalAlpha = pulse;
    ctx.fillStyle = theme.stylus;
    ctx.beginPath();
    ctx.arc(tipX, tipY, Math.max(2.5, 3.5 * geometry.scale), 0, Math.PI * 2);
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.shadowBlur = 0;
    ctx.restore();
  }

  return { ...resolved, tipX, tipY, liftedOffset };
}
