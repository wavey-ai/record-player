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
  if (!components.stylus && !components.needlePoint && !components.tonearmGuide) return null;
  const resolved = resolveStylusGeometry(geometry, state, options);
  const armDirection = Math.atan2(resolved.tip.y - resolved.anchorY, resolved.tip.x - resolved.anchorX);
  const tailLength = Math.max(16, 22 * geometry.scale);
  const liftedOffset = state.needleLifted ? Math.max(5, 8 * geometry.scale) : 0;
  const tipX = resolved.tip.x;
  const tipY = resolved.tip.y - liftedOffset;

  // The dotted travel guide (the "arc") is independent of the arm now, so it
  // can be shown on its own via the tonearmGuide component.
  if (components.tonearmGuide) {
    const angle0 = Math.atan2(resolved.outerTip.y - resolved.anchorY, resolved.outerTip.x - resolved.anchorX);
    const angle1 = Math.atan2(resolved.innerTip.y - resolved.anchorY, resolved.innerTip.x - resolved.anchorX);
    const low = Math.min(angle0, angle1);
    const high = Math.max(angle0, angle1);
    // Nudged slightly below the needle point so the dotted travel guide
    // reads as a distinct reference arc rather than tracing directly
    // through (and visually competing with) the needle tip itself.
    const guideYOffset = Math.max(2, 3 * geometry.scale);
    ctx.save();
    ctx.globalAlpha = 0.55;
    ctx.strokeStyle = theme.tonearmGuide || theme.tonearm;
    ctx.lineWidth = Math.max(1.4, 1.7 * geometry.scale);
    ctx.lineCap = "round";
    ctx.setLineDash([0.01, Math.max(5, 6 * geometry.scale)]);
    ctx.beginPath();
    ctx.arc(resolved.anchorX, resolved.anchorY + guideYOffset, resolved.armLength, low, high);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
    ctx.restore();
  }

  if (components.stylus) {
    ctx.save();
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
    const hub = Math.max(2, 2.2 * geometry.scale);
    ctx.globalAlpha = 0.4;
    ctx.fillStyle = theme.tonearm;
    ctx.beginPath();
    ctx.arc(resolved.anchorX, resolved.anchorY, hub, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  if (components.needlePoint) {
    const active = !state.needleLifted && Boolean(state.playing || state.scratching);
    const shimmer = active ? 0.5 + Math.sin(timestamp / 53) * 0.5 : 0.18;
    const pulse = active ? 0.72 + shimmer * 0.2 : 0.46;
    const glowRadius = Math.max(4.5, 7.5 * geometry.scale);
    const headRadius = Math.max(1.2, 1.6 * geometry.scale);
    ctx.save();
    ctx.globalCompositeOperation = active ? "screen" : "source-over";
    if (active) {
      const gradient = ctx.createRadialGradient(tipX, tipY, 0, tipX, tipY, glowRadius);
      gradient.addColorStop(0, `rgba(255,255,255,${0.58 + shimmer * 0.22})`);
      gradient.addColorStop(0.18, theme.stylus);
      gradient.addColorStop(0.44, theme.stylusGlow);
      gradient.addColorStop(0.72, `rgba(255,255,255,${0.08 + pulse * 0.08})`);
      gradient.addColorStop(1, "rgba(255,255,255,0)");
      ctx.fillStyle = gradient;
      ctx.beginPath();
      ctx.arc(tipX, tipY, glowRadius, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalCompositeOperation = "color-dodge";
      ctx.globalAlpha = 0.42 + shimmer * 0.26;
      ctx.strokeStyle = theme.stylus;
      ctx.lineWidth = Math.max(0.7, 0.9 * geometry.scale);
      ctx.beginPath();
      ctx.moveTo(tipX - 3.4 * geometry.scale, tipY);
      ctx.lineTo(tipX + 3.4 * geometry.scale, tipY);
      ctx.moveTo(tipX, tipY - 3.4 * geometry.scale);
      ctx.lineTo(tipX, tipY + 3.4 * geometry.scale);
      ctx.stroke();
    }
    ctx.globalCompositeOperation = active ? "screen" : "source-over";
    ctx.globalAlpha = active ? 0.78 + pulse * 0.12 : 0.64;
    ctx.fillStyle = active ? theme.stylus : theme.tonearm;
    ctx.beginPath();
    ctx.arc(tipX, tipY, headRadius, 0, Math.PI * 2);
    ctx.fill();
    if (active) {
      ctx.globalAlpha = 0.86;
      ctx.fillStyle = "rgba(255,255,255,0.72)";
      ctx.beginPath();
      ctx.arc(tipX - 0.45 * geometry.scale, tipY - 0.45 * geometry.scale, Math.max(0.45, 0.62 * geometry.scale), 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.restore();
  }

  const guideAngle0 = Math.atan2(resolved.outerTip.y - resolved.anchorY, resolved.outerTip.x - resolved.anchorX);
  const guideAngle1 = Math.atan2(resolved.innerTip.y - resolved.anchorY, resolved.innerTip.x - resolved.anchorX);
  return { ...resolved, tipX, tipY, liftedOffset, guideAngle0, guideAngle1 };
}
