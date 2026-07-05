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
  const profile = String(state.recordProfile || "").trim().toLowerCase();
  const canonicalOuterRadius = 287;
  const payloadOuterRadius = 280;
  const payloadInnerRadius = profile === "single45" ? 169 : 109;
  const outerGroove = geometry.recordRadius * (payloadOuterRadius / canonicalOuterRadius);
  const innerGroove = geometry.recordRadius * (payloadInnerRadius / canonicalOuterRadius);
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
      ctx.setLineDash([Math.max(0.8, geometry.scale), 5 * geometry.scale]);
      ctx.strokeStyle = theme.tonearmGuide;
      ctx.lineWidth = Math.max(0.7, 0.8 * geometry.scale);
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
    const pulse = active ? 0.78 + Math.sin(timestamp / 62) * 0.12 : 0.46;
    const glowRadius = Math.max(4, 7 * geometry.scale);
    const headRadius = Math.max(1.2, 1.6 * geometry.scale);
    ctx.save();
    ctx.globalCompositeOperation = "hard-light";
    if (active) {
      const gradient = ctx.createRadialGradient(tipX, tipY, 0, tipX, tipY, glowRadius);
      gradient.addColorStop(0, theme.stylus);
      gradient.addColorStop(0.22, theme.stylusGlow);
      gradient.addColorStop(0.62, `rgba(255,255,255,${0.14 + pulse * 0.12})`);
      gradient.addColorStop(1, "rgba(255,255,255,0)");
      ctx.fillStyle = gradient;
      ctx.beginPath();
      ctx.arc(tipX, tipY, glowRadius, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.globalAlpha = active ? 0.78 + pulse * 0.12 : 0.64;
    ctx.fillStyle = active ? theme.stylus : theme.tonearm;
    ctx.beginPath();
    ctx.arc(tipX, tipY, headRadius, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  const guideAngle0 = Math.atan2(resolved.outerTip.y - resolved.anchorY, resolved.outerTip.x - resolved.anchorX);
  const guideAngle1 = Math.atan2(resolved.innerTip.y - resolved.anchorY, resolved.innerTip.x - resolved.anchorX);
  return { ...resolved, tipX, tipY, liftedOffset, guideAngle0, guideAngle1 };
}
