import {
  STROBE_ROWS,
  STROBE_LAMP_DEG,
  STROBE_BEAM_FULL_DEG,
  STROBE_BEAM_HALF_DEG,
  STROBE_DEG_PER_SEC,
  STROBE_CONT_DEG_PER_SEC,
  angularDeltaDegrees,
  degToRad,
  pointOnCircle,
  FONT_FAMILY
} from "./player-canvas-geometry.js";

function drawLampFaceLabel(ctx, lamp, lightOn, theme, scale) {
  const text = "ON <-----> OFF";
  const chars = Array.from(text);
  const radius = Math.max(lamp.radius * 0.56, lamp.radius - 3.2 * scale);
  const fontSize = Math.max(3, lamp.radius * 0.18);
  const centerDeg = lightOn ? -90 : -30;
  const center = degToRad(centerDeg);

  ctx.save();
  ctx.font = `700 ${fontSize}px ${FONT_FAMILY}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillStyle = theme.line;
  const widths = chars.map(char => ctx.measureText(char).width);
  const totalWidth = widths.reduce((sum, width) => sum + width, 0);
  let offset = -totalWidth / 2;

  chars.forEach((char, index) => {
    const charOffset = offset + widths[index] / 2;
    const angle = center + charOffset / radius;
    const x = lamp.x + Math.cos(angle) * radius;
    const y = lamp.y + Math.sin(angle) * radius;
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(angle + Math.PI / 2);
    ctx.fillText(char, 0, 0);
    ctx.restore();
    offset += widths[index];
  });

  ctx.restore();
}

export function strobeLampGeometry(geometry) {
  const radius = Math.max(7, geometry.buttonHeight * 0.5);
  const distance = geometry.outerRadius + radius + geometry.buttonHeight * 0.6;
  const point = pointOnCircle(geometry, STROBE_LAMP_DEG, distance);
  return { x: point.x, y: point.y, radius, angle: STROBE_LAMP_DEG };
}

export function drawStrobe(ctx, geometry, state, theme, lightOn, timestamp, parts = {}) {
  const showDots = parts.showDots !== false;
  const showStrobe = parts.showStrobe !== false;
  const showLamp = parts.showLamp !== false;
  // The lamp fixture (and its beam-sampled lit dots) only make sense when
  // the lamp is actually shown; otherwise treat the strobe as unlit so the
  // base dots render evenly all the way round.
  const beamLit = lightOn && showLamp;
  const playing = Boolean(state.motorRunning || state.scratching);
  const rate = Number(state.playbackRate) || 0;
  const timeSec = timestamp / 1000;
  const baseDot = Math.max(1.15, geometry.scale * 1.45);
  const lamp = strobeLampGeometry(geometry);
  const physicalRotation = playing ? (rate * STROBE_CONT_DEG_PER_SEC * timeSec) % 360 : 0;

  ctx.save();
  const baseAlpha = ctx.globalAlpha;
  STROBE_ROWS.forEach((row, rowIndex) => {
    const t = (rowIndex + 0.5) / STROBE_ROWS.length;
    const ringRadius = geometry.outerRadius - (geometry.outerRadius - geometry.innerRadius) * t;
    const dotRadius = baseDot * row.dotScale;
    const calibratedRate = 1 + row.pitchPct / 100;
    const error = playing ? rate - calibratedRate : 0;
    const stepDeg = 360 / row.count;
    const strobeOffset = ((error * STROBE_DEG_PER_SEC * timeSec) % stepDeg + stepDeg) % stepDeg;

    for (let index = 0; index < row.count; index += 1) {
      const movingAngleDeg = index * stepDeg + physicalRotation;
      const movingDistance = angularDeltaDegrees(movingAngleDeg, lamp.angle);
      // A base dot renders wherever the beam isn't currently "eating" it —
      // i.e. everywhere when the beam is off, or outside the beam cone.
      if (showDots && (!beamLit || movingDistance > STROBE_BEAM_HALF_DEG)) {
        const angle = degToRad(movingAngleDeg);
        ctx.beginPath();
        ctx.arc(
          geometry.cx + Math.cos(angle) * ringRadius,
          geometry.cy + Math.sin(angle) * ringRadius,
          dotRadius,
          0,
          Math.PI * 2
        );
        ctx.fillStyle = theme.syncDot;
        ctx.fill();
      }

      if (!beamLit || !showStrobe) continue;
      const sampledAngleDeg = index * stepDeg + strobeOffset;
      const distance = angularDeltaDegrees(sampledAngleDeg, lamp.angle);
      if (distance > STROBE_BEAM_HALF_DEG) continue;
      const feather = distance <= STROBE_BEAM_FULL_DEG
        ? 1
        : 1 - ((distance - STROBE_BEAM_FULL_DEG) / (STROBE_BEAM_HALF_DEG - STROBE_BEAM_FULL_DEG));
      const pulse = 0.9 + Math.sin(timestamp / 45) * 0.1;
      const angle = degToRad(sampledAngleDeg);
      ctx.beginPath();
      ctx.arc(
        geometry.cx + Math.cos(angle) * ringRadius,
        geometry.cy + Math.sin(angle) * ringRadius,
        dotRadius * (1 + feather * 0.12),
        0,
        Math.PI * 2
      );
      ctx.fillStyle = theme.syncLit;
      ctx.globalAlpha = baseAlpha * Math.max(0, feather) * pulse;
      ctx.fill();
      ctx.globalAlpha = baseAlpha;
    }
  });

  if (beamLit) {
    const beamRadius = geometry.outerRadius + geometry.buttonHeight * 0.2;
    const beam = ctx.createRadialGradient(lamp.x, lamp.y, 0, lamp.x, lamp.y, geometry.buttonHeight * 2.2);
    beam.addColorStop(0, theme.lamp);
    beam.addColorStop(0.2, theme.lamp);
    beam.addColorStop(1, "rgba(0,191,211,0)");
    ctx.globalAlpha = baseAlpha * 0.16;
    ctx.fillStyle = beam;
    ctx.beginPath();
    ctx.arc(lamp.x, lamp.y, geometry.buttonHeight * 2.2, 0, Math.PI * 2);
    ctx.fill();
    ctx.globalAlpha = baseAlpha;
    const a0 = degToRad(STROBE_LAMP_DEG - STROBE_BEAM_HALF_DEG);
    const a1 = degToRad(STROBE_LAMP_DEG + STROBE_BEAM_HALF_DEG);
    ctx.beginPath();
    ctx.moveTo(lamp.x, lamp.y);
    ctx.arc(geometry.cx, geometry.cy, beamRadius, a0, a1);
    ctx.closePath();
    const gradient = ctx.createRadialGradient(lamp.x, lamp.y, 0, lamp.x, lamp.y, geometry.outerRadius);
    gradient.addColorStop(0, theme.lamp);
    gradient.addColorStop(1, "rgba(0,191,211,0)");
    ctx.fillStyle = gradient;
    ctx.globalAlpha = baseAlpha * 0.08;
    ctx.fill();
    ctx.globalAlpha = baseAlpha;
  }

  if (showLamp) {
    ctx.beginPath();
    ctx.arc(lamp.x, lamp.y, lamp.radius, 0, Math.PI * 2);
    ctx.fillStyle = theme.controlFill;
    ctx.fill();
    ctx.strokeStyle = theme.line;
    ctx.lineWidth = 1;
    ctx.stroke();

    drawLampFaceLabel(ctx, lamp, lightOn, theme, geometry.scale);

    ctx.beginPath();
    ctx.arc(lamp.x, lamp.y, Math.max(1.2, lamp.radius * 0.08), 0, Math.PI * 2);
    ctx.fillStyle = lightOn ? theme.lamp : theme.mutedLine;
    ctx.fill();
  }

  ctx.restore();
  return lamp;
}
