import {
  CANVAS_COMPONENTS,
  CANVAS_THEME,
  buildCanvasGeometry,
  clamp,
  localPointer,
  pointerPolar
} from "./player-canvas-geometry.js";
import { controlAt, drawRadialControls, updateArcControl } from "./player-canvas-controls.js";
import { drawStrobe, strobeLampGeometry } from "./player-canvas-strobe.js";
import { drawStylus, resolveStylusGeometry } from "./player-canvas-stylus.js";

function unwrapRadians(delta) {
  if (delta > Math.PI) return delta - Math.PI * 2;
  if (delta < -Math.PI) return delta + Math.PI * 2;
  return delta;
}

export function createVinylPlayerCanvas(player, canvas, options = {}) {
  if (!(canvas instanceof HTMLCanvasElement)) throw new TypeError("A canvas element is required");
  let components = { ...CANVAS_COMPONENTS, ...(options.components || {}) };
  let theme = { ...CANVAS_THEME, ...(options.theme || {}) };
  let snapshot = player.getState();
  let image = null;
  let imageUrl = "";
  let frame = 0;
  let destroyed = false;
  let activeGesture = null;
  let hitRegions = [];
  let lastTimestamp = performance.now();
  let visualRotation = Number(snapshot.rotationDegrees) || 0;
  let strobeLightOn = true;
  let latestGeometry = null;
  let latestStylus = null;
  const ctx = canvas.getContext("2d", { alpha: true });

  function resize() {
    const rect = canvas.getBoundingClientRect();
    const dpr = Math.max(1, window.devicePixelRatio || 1);
    const width = Math.max(1, Math.round(rect.width * dpr));
    const height = Math.max(1, Math.round(rect.height * dpr));
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    return { width: rect.width, height: rect.height };
  }

  function loadImage(url) {
    if (url === imageUrl) return;
    imageUrl = url || "";
    image = null;
    if (!imageUrl) return;
    const next = new Image();
    next.decoding = "async";
    next.onload = () => {
      if (next.src === imageUrl) image = next;
    };
    next.src = imageUrl;
  }

  function drawMinimalTurntable(geometry) {
    ctx.save();
    ctx.fillStyle = theme.recordFallback;
    if (theme.recordFallback !== "transparent") {
      ctx.beginPath();
      ctx.arc(geometry.cx, geometry.cy, geometry.recordRadius, 0, Math.PI * 2);
      ctx.fill();
    }
    const spacing = Math.max(7, 10 * geometry.scale);
    const inner = Math.max(spacing, geometry.recordRadius * 0.08);
    let index = 0;
    for (let radius = inner; radius <= geometry.recordRadius; radius += spacing) {
      ctx.beginPath();
      ctx.arc(geometry.cx, geometry.cy, radius, 0, Math.PI * 2);
      ctx.strokeStyle = index % 4 === 0 ? theme.turntableRingStrong : theme.turntableRing;
      ctx.lineWidth = index % 4 === 0 ? Math.max(0.8, geometry.scale) : Math.max(0.5, geometry.scale * 0.7);
      ctx.stroke();
      index += 1;
    }
    ctx.restore();
  }

  function drawRecord(geometry) {
    if (!components.record) return;
    if (!image) {
      drawMinimalTurntable(geometry);
      return;
    }
    ctx.save();
    ctx.translate(geometry.cx, geometry.cy);
    ctx.rotate(visualRotation * Math.PI / 180);
    ctx.beginPath();
    ctx.arc(0, 0, geometry.recordRadius, 0, Math.PI * 2);
    ctx.clip();
    ctx.drawImage(image, -geometry.recordRadius, -geometry.recordRadius, geometry.recordRadius * 2, geometry.recordRadius * 2);
    ctx.restore();
    ctx.strokeStyle = theme.line;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.arc(geometry.cx, geometry.cy, geometry.recordRadius, 0, Math.PI * 2);
    ctx.stroke();
  }

  function drawSpindle(geometry) {
    if (!components.spindle) return;
    ctx.fillStyle = theme.line;
    ctx.beginPath();
    ctx.arc(geometry.cx, geometry.cy, Math.max(2, geometry.recordRadius * 0.014), 0, Math.PI * 2);
    ctx.fill();
  }

  function render(timestamp) {
    if (destroyed) return;
    const { width, height } = resize();
    const dt = Math.min(0.1, Math.max(0, (timestamp - lastTimestamp) / 1000));
    lastTimestamp = timestamp;
    if (snapshot.motorRunning && !snapshot.scratching) {
      visualRotation = (visualRotation + (Number(snapshot.rpm) || 0) * 6 * dt) % 360;
    }
    loadImage(snapshot.recordImageUrl);
    ctx.clearRect(0, 0, width, height);
    if (theme.background !== "transparent") {
      ctx.fillStyle = theme.background;
      ctx.fillRect(0, 0, width, height);
    }
    hitRegions = [];
    const geometry = buildCanvasGeometry(width, height);
    latestGeometry = geometry;
    if (components.syncRings) {
      const lamp = drawStrobe(ctx, geometry, snapshot, theme, strobeLightOn, timestamp);
      if (components.strobeLamp) {
        hitRegions.push({ key: "strobeLamp", kind: "lamp", x: lamp.x, y: lamp.y, radius: lamp.radius * 1.4 });
      }
    }
    drawRecord(geometry);
    drawSpindle(geometry);
    hitRegions.push({ key: "record", kind: "record", x: geometry.cx, y: geometry.cy, radius: geometry.recordRadius });
    latestStylus = drawStylus(ctx, geometry, snapshot, theme, components, timestamp, {
      calibrateProgress: options.calibrateStylusProgress
    });
    if (latestStylus && components.needlePoint) {
      hitRegions.push({
        key: "needlePoint",
        kind: "needle-point",
        x: latestStylus.tipX,
        y: latestStylus.tipY,
        radius: Math.max(12, 18 * geometry.scale)
      });
    }
    if (latestStylus && components.seek) {
      hitRegions.push({
        key: "needleArc",
        kind: "needle-arc",
        x: latestStylus.anchorX,
        y: latestStylus.anchorY,
        radius: latestStylus.armLength,
        angle0: latestStylus.guideAngle0,
        angle1: latestStylus.guideAngle1,
        tolerance: Math.max(16, 26 * geometry.scale)
      });
    }
    drawRadialControls(ctx, geometry, player, snapshot, components, theme, hitRegions);
    frame = requestAnimationFrame(render);
  }

  function regionAt(point) {
    const direct = [...hitRegions].reverse().find(region => {
      if (region.kind === "record" || region.kind === "lamp" || region.kind === "needle-point") {
        return Math.hypot(point.x - region.x, point.y - region.y) <= region.radius;
      }
      if (region.kind === "needle-arc") {
        const distance = Math.hypot(point.x - region.x, point.y - region.y);
        if (Math.abs(distance - region.radius) > region.tolerance) return false;
        const angle = Math.atan2(point.y - region.y, point.x - region.x);
        let end = region.angle1;
        while (end - region.angle0 > Math.PI) end -= Math.PI * 2;
        while (end - region.angle0 < -Math.PI) end += Math.PI * 2;
        let current = angle;
        while (current - region.angle0 > Math.PI) current -= Math.PI * 2;
        while (current - region.angle0 < -Math.PI) current += Math.PI * 2;
        const low = Math.min(region.angle0, end) - 0.12;
        const high = Math.max(region.angle0, end) + 0.12;
        return current >= low && current <= high;
      }
      return false;
    });
    return direct || controlAt(hitRegions, latestGeometry, point);
  }

  function seekFromStylusPoint(point) {
    if (!latestGeometry) return;
    const resolved = resolveStylusGeometry(latestGeometry, snapshot, {
      calibrateProgress: options.calibrateStylusProgress
    });
    const angle0 = Math.atan2(resolved.outerTip.y - resolved.anchorY, resolved.outerTip.x - resolved.anchorX);
    let angle1 = Math.atan2(resolved.innerTip.y - resolved.anchorY, resolved.innerTip.x - resolved.anchorX);
    while (angle1 - angle0 > Math.PI) angle1 -= Math.PI * 2;
    while (angle1 - angle0 < -Math.PI) angle1 += Math.PI * 2;
    let angle = Math.atan2(point.y - resolved.anchorY, point.x - resolved.anchorX);
    while (angle - angle0 > Math.PI) angle -= Math.PI * 2;
    while (angle - angle0 < -Math.PI) angle += Math.PI * 2;
    const denominator = angle1 - angle0;
    const visualRatio = Math.abs(denominator) > 0.0001 ? clamp((angle - angle0) / denominator, 0, 1) : 0;
    player.seekRatio(typeof options.inverseStylusProgress === "function" ? options.inverseStylusProgress(visualRatio) : visualRatio);
  }

  function pointerDown(event) {
    if (!latestGeometry) return;
    const point = localPointer(canvas, event);
    const region = regionAt(point);
    if (!region) return;
    canvas.setPointerCapture(event.pointerId);
    if (region.kind === "lamp") {
      strobeLightOn = !strobeLightOn;
      return;
    }
    if (region.kind === "sector-button" || region.kind === "round-button") {
      activeGesture = { kind: "button", region, pointerId: event.pointerId };
      return;
    }
    if (region.kind === "arc-slider") {
      activeGesture = { kind: "arc-slider", region, pointerId: event.pointerId };
      updateArcControl(region, latestGeometry, point);
      return;
    }
    if (region.kind === "needle-point" || region.kind === "needle-arc") {
      activeGesture = { kind: "needle", pointerId: event.pointerId };
      seekFromStylusPoint(point);
      return;
    }
    if (!snapshot.ready) {
      document.querySelector("#file")?.click();
      return;
    }
    const angle = Math.atan2(point.y - latestGeometry.cy, point.x - latestGeometry.cx);
    activeGesture = {
      kind: "record",
      pointerId: event.pointerId,
      lastAngle: angle,
      lastTime: event.timeStamp,
      positionFrames: Math.round((Number(snapshot.positionSeconds) || 0) * (Number(snapshot.sampleRate) || 48000))
    };
    player.beginScratch({
      pointerId: event.pointerId,
      rotationDegrees: visualRotation,
      positionFrames: activeGesture.positionFrames
    });
  }

  function pointerMove(event) {
    if (!activeGesture || activeGesture.pointerId !== event.pointerId || !latestGeometry) return;
    const point = localPointer(canvas, event);
    if (activeGesture.kind === "arc-slider") {
      updateArcControl(activeGesture.region, latestGeometry, point);
      return;
    }
    if (activeGesture.kind === "needle") {
      seekFromStylusPoint(point);
      return;
    }
    if (activeGesture.kind !== "record") return;
    const angle = Math.atan2(point.y - latestGeometry.cy, point.x - latestGeometry.cx);
    const delta = unwrapRadians(angle - activeGesture.lastAngle);
    const dt = Math.max(0.001, (event.timeStamp - activeGesture.lastTime) / 1000);
    const sampleRate = Number(snapshot.sampleRate) || 48000;
    const framesPerTurn = sampleRate * 60 / Math.max(1, Number(snapshot.nativeRpm) || 33.3333333333);
    const frameDelta = delta / (Math.PI * 2) * framesPerTurn;
    activeGesture.positionFrames = Math.max(0, activeGesture.positionFrames + frameDelta);
    const rate = frameDelta / (sampleRate * dt);
    visualRotation = (visualRotation + delta * 180 / Math.PI) % 360;
    player.updateScratch({
      positionFrames: activeGesture.positionFrames,
      rate,
      rotationDegrees: visualRotation,
      impulse: Math.min(1, Math.abs(rate) / 3)
    });
    activeGesture.lastAngle = angle;
    activeGesture.lastTime = event.timeStamp;
  }

  function pointerUp(event) {
    if (!activeGesture || activeGesture.pointerId !== event.pointerId) return;
    const gesture = activeGesture;
    activeGesture = null;
    if (gesture.kind === "button") gesture.region.onActivate();
    if (gesture.kind === "record") {
      player.endScratch({ rotationDegrees: visualRotation, resumePlayback: true });
    }
  }

  const unsubscribe = player.subscribe(next => {
    const wasScratching = snapshot.scratching;
    snapshot = next;
    if ((next.scratching || wasScratching) && Number.isFinite(next.rotationDegrees)) {
      visualRotation = next.rotationDegrees;
    }
  });

  canvas.addEventListener("pointerdown", pointerDown);
  canvas.addEventListener("pointermove", pointerMove);
  canvas.addEventListener("pointerup", pointerUp);
  canvas.addEventListener("pointercancel", pointerUp);
  frame = requestAnimationFrame(render);

  return Object.freeze({
    configure(next = {}) {
      if (next.components) components = { ...components, ...next.components };
      if (next.theme) theme = { ...theme, ...next.theme };
      if (typeof next.strobeLightOn === "boolean") strobeLightOn = next.strobeLightOn;
      return this.getConfig();
    },
    setComponentVisible(name, visible) {
      if (!(name in CANVAS_COMPONENTS)) throw new RangeError(`Unknown canvas component: ${name}`);
      components = { ...components, [name]: Boolean(visible) };
      return { ...components };
    },
    setTheme(next = {}) {
      theme = { ...theme, ...next };
      return { ...theme };
    },
    setStrobeLight(enabled) {
      strobeLightOn = Boolean(enabled);
      return strobeLightOn;
    },
    getConfig() {
      return Object.freeze({ components: { ...components }, theme: { ...theme }, strobeLightOn });
    },
    destroy() {
      destroyed = true;
      cancelAnimationFrame(frame);
      unsubscribe();
      canvas.removeEventListener("pointerdown", pointerDown);
      canvas.removeEventListener("pointermove", pointerMove);
      canvas.removeEventListener("pointerup", pointerUp);
      canvas.removeEventListener("pointercancel", pointerUp);
    }
  });
}

export const VIN_YL_PLAYER_CANVAS_COMPONENTS = CANVAS_COMPONENTS;
export const VIN_YL_PLAYER_CANVAS_THEME = CANVAS_THEME;
