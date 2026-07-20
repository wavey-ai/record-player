import {
  CANVAS_COMPONENTS,
  CANVAS_THEME,
  buildCanvasGeometry,
  clamp,
  localPointer
} from "./player-canvas-geometry.js";
import { controlAt, drawRadialControls, updateArcControl } from "./player-canvas-controls.js";
import { drawStrobe, strobeLampGeometry } from "./player-canvas-strobe.js";
import { drawStylus, resolveStylusGeometry } from "./player-canvas-stylus.js";
import { createScratchGestureTracker } from "./scratch-gesture.js";

const UI_IDLE_DELAY_MS = 10000;
const UI_FADE_IN_MS = 140;
const UI_FADE_OUT_MS = 1100;

function clamp01(value) {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(1, value));
}

function easeOutCubic(value) {
  const t = clamp01(value);
  return 1 - Math.pow(1 - t, 3);
}

function easeIncandescentFade(value) {
  const t = clamp01(value);
  return 1 - Math.pow(1 - t, 4);
}

export function createVinylPlayerCanvas(player, canvas, options = {}) {
  if (!(canvas instanceof HTMLCanvasElement)) throw new TypeError("A canvas element is required");
  let components = { ...CANVAS_COMPONENTS, ...(options.components || {}) };
  let theme = { ...CANVAS_THEME, ...(options.theme || {}) };
  let interaction = {
    loadOnEmptyRecordTap: Boolean(options.loadOnEmptyRecordTap),
    recordFill: options.recordFill === true,
  };
  let snapshot = player.getState();
  let image = null;
  let imageUrl = "";
  let frame = 0;
  let destroyed = false;
  const activeGestures = new Map();
  let hitRegions = [];
  let visualRotation = Number(snapshot.rotationDegrees) || 0;
  let rotationClockAtMs = performance.now();
  let strobeLightOn = true;
  let latestGeometry = null;
  let latestStylus = null;
  let lastActivityAt = performance.now();
  let uiOpacity = 1;
  let uiFadeFrom = 1;
  let uiFadeTo = 1;
  let uiFadeStartedAt = lastActivityAt;
  let idleTimer = 0;
  const ctx = canvas.getContext("2d", { alpha: true });
  const activityTarget = canvas.ownerDocument || document;
  const root = activityTarget.documentElement || document.documentElement;

  function canAutoFadeUi(nextSnapshot = snapshot) {
    return Boolean(nextSnapshot?.ready && nextSnapshot?.playing);
  }

  function shouldAnimate() {
    return Boolean(
      activeGestures.size > 0 ||
      snapshot.playing ||
      snapshot.motorRunning ||
      snapshot.scratching ||
      Math.abs(Number(snapshot.effectiveRate) || 0) > 0.0001 ||
      snapshot.loading ||
      snapshot.decoding ||
      Math.abs(uiOpacity - uiFadeTo) > 0.001
    );
  }

  function scheduleRender() {
    if (destroyed || frame) return;
    frame = requestAnimationFrame(render);
  }

  function clearIdleTimer() {
    if (!idleTimer) return;
    clearTimeout(idleTimer);
    idleTimer = 0;
  }

  function scheduleIdleTimer() {
    clearIdleTimer();
    if (destroyed) return;
    if (!canAutoFadeUi()) {
      setUiFadeTarget(1);
      return;
    }
    const delay = Math.max(0, UI_IDLE_DELAY_MS - (performance.now() - lastActivityAt));
    idleTimer = setTimeout(() => {
      idleTimer = 0;
      if (canAutoFadeUi()) setUiFadeTarget(0);
    }, delay);
  }

  function setUiFadeTarget(target, now = performance.now()) {
    const next = clamp01(target);
    if (Math.abs(uiFadeTo - next) < 0.001 && Math.abs(uiOpacity - next) < 0.001) return;
    uiFadeFrom = uiOpacity;
    uiFadeTo = next;
    uiFadeStartedAt = now;
    root.classList.toggle("vinyl-ui-idle", next === 0);
    scheduleRender();
  }

  function updateUiFade(timestamp) {
    if (Math.abs(uiOpacity - uiFadeTo) <= 0.001) {
      uiOpacity = uiFadeTo;
      root.classList.toggle("vinyl-ui-idle", uiOpacity < 0.01);
      return;
    }
    const durationMs = uiFadeTo < uiFadeFrom ? UI_FADE_OUT_MS : UI_FADE_IN_MS;
    const progress = durationMs <= 0 ? 1 : (timestamp - uiFadeStartedAt) / durationMs;
    const eased = uiFadeTo < uiFadeFrom ? easeIncandescentFade(progress) : easeOutCubic(progress);
    uiOpacity = uiFadeFrom + (uiFadeTo - uiFadeFrom) * eased;
    if (progress >= 1) {
      uiOpacity = uiFadeTo;
      root.classList.toggle("vinyl-ui-idle", uiOpacity < 0.01);
    }
  }

  function noteActivity() {
    lastActivityAt = performance.now();
    setUiFadeTarget(1, lastActivityAt);
    scheduleIdleTimer();
  }

  function withUiOpacity(draw) {
    if (uiOpacity <= 0.001) return;
    ctx.save();
    ctx.globalAlpha *= uiOpacity;
    draw();
    ctx.restore();
  }

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
      if (next.src === imageUrl) {
        image = next;
        scheduleRender();
      }
    };
    next.src = imageUrl;
  }

  function drawMinimalTurntable(geometry) {
    ctx.save();
    ctx.translate(geometry.cx, geometry.cy);
    ctx.rotate(visualRotation * Math.PI / 180);
    ctx.fillStyle = theme.recordFallback;
    if (theme.recordFallback !== "transparent") {
      ctx.beginPath();
      ctx.arc(0, 0, geometry.recordRadius, 0, Math.PI * 2);
      ctx.fill();
    }
    const spacing = Math.max(7, 10 * geometry.scale);
    const inner = Math.max(spacing, geometry.recordRadius * 0.08);
    const segmentCount = 96;
    const segmentSpan = (Math.PI * 2) / segmentCount;
    let index = 0;
    for (let radius = inner; radius <= geometry.recordRadius; radius += spacing) {
      const majorRing = index % 4 === 0;
      const lineWidth = majorRing ? Math.max(0.8, geometry.scale) : Math.max(0.5, geometry.scale * 0.7);
      for (let segmentIndex = 0; segmentIndex < segmentCount; segmentIndex += 1) {
        const t = segmentIndex / segmentCount;
        const wave = 0.5 + 0.5 * Math.sin((t * Math.PI * 2) + (index * 0.31));
        ctx.beginPath();
        ctx.arc(
          0,
          0,
          radius,
          segmentIndex * segmentSpan,
          (segmentIndex + 1) * segmentSpan + 0.002
        );
        ctx.lineWidth = lineWidth;
        if (majorRing) {
          ctx.strokeStyle = theme.turntableRingStrong;
          ctx.globalAlpha = 0.18 + wave * 0.12;
        } else {
          ctx.strokeStyle = theme.turntableRing;
          ctx.globalAlpha = 0.14 + wave * 0.08;
        }
        ctx.stroke();
      }
      index += 1;
    }
    ctx.globalAlpha = 1;
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

  function advanceAudioOwnedRotation(timestamp) {
    if (hasRecordGesture()) return;
    const dt = Math.min(0.1, Math.max(0, (timestamp - rotationClockAtMs) / 1000));
    rotationClockAtMs = timestamp;
    const effectiveRate = Number(snapshot.effectiveRate) || 0;
    const nativeRpm = Math.max(0, Number(snapshot.nativeRpm) || Number(snapshot.rpm) || 0);
    visualRotation = (visualRotation + effectiveRate * nativeRpm * 6 * dt) % 360;
  }

  function render(timestamp) {
    frame = 0;
    if (destroyed) return;
    const { width, height } = resize();
    updateUiFade(timestamp);
    // Rust publishes the rate that actually rendered. Extrapolate from the
    // latest audio-owned phase so spin-up, braking, reverse motion, pitch slew
    // and wow remain smooth between worklet telemetry messages.
    advanceAudioOwnedRotation(timestamp);
    loadImage(snapshot.recordImageUrl);
    ctx.clearRect(0, 0, width, height);
    if (theme.background !== "transparent") {
      ctx.fillStyle = theme.background;
      ctx.fillRect(0, 0, width, height);
    }
    hitRegions = [];
    const geometry = buildCanvasGeometry(width, height, interaction);
    latestGeometry = geometry;
    // syncRings kept as a legacy master switch; the strobe subsystem is now
    // gated per-part: syncDots (base ring dots), strobe (lit sampling +
    // beam), strobeLamp (the lamp fixture / light).
    if (components.syncRings && (components.syncDots || components.strobe || components.strobeLamp)) {
      withUiOpacity(() => {
        const lamp = drawStrobe(ctx, geometry, snapshot, theme, strobeLightOn, timestamp, {
          showDots: components.syncDots,
          showStrobe: components.strobe,
          showLamp: components.strobeLamp,
        });
        if (components.strobeLamp) {
          hitRegions.push({ key: "strobeLamp", kind: "lamp", x: lamp.x, y: lamp.y, radius: lamp.radius * 1.4 });
        }
      });
    }
    drawRecord(geometry);
    withUiOpacity(() => drawSpindle(geometry));
    hitRegions.push({ key: "record", kind: "record", x: geometry.cx, y: geometry.cy, radius: geometry.recordRadius });
    withUiOpacity(() => {
      latestStylus = drawStylus(ctx, geometry, snapshot, theme, components, timestamp, {
        calibrateProgress: options.calibrateStylusProgress
      });
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
    withUiOpacity(() => drawRadialControls(ctx, geometry, player, snapshot, components, theme, hitRegions));
    if (shouldAnimate()) scheduleRender();
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
    const angle = Math.atan2(point.y - resolved.anchorY, point.x - resolved.anchorX);
    const projectedTipX = resolved.anchorX + Math.cos(angle) * resolved.armLength;
    const projectedTipY = resolved.anchorY + Math.sin(angle) * resolved.armLength;
    const grooveRadius = Math.hypot(
      projectedTipX - latestGeometry.cx,
      projectedTipY - latestGeometry.cy,
    );
    const denominator = resolved.innerGroove - resolved.outerGroove;
    const visualRatio = Math.abs(denominator) > 0.0001
      ? clamp((grooveRadius - resolved.outerGroove) / denominator, 0, 1)
      : 0;
    player.seekRatio(typeof options.inverseStylusProgress === "function" ? options.inverseStylusProgress(visualRatio) : visualRatio);
  }

  function coalescedSamples(event) {
    if (typeof event.getCoalescedEvents !== "function") return [event];
    try {
      const samples = Array.from(event.getCoalescedEvents() || []);
      return samples.length ? samples : [event];
    } catch {
      return [event];
    }
  }

  function hasRecordGesture() {
    return Array.from(activeGestures.values()).some(gesture => gesture.kind === "record");
  }

  function pointerEventTimeMs(event) {
    const timestamp = Number(event?.timeStamp);
    return Number.isFinite(timestamp) ? timestamp : performance.now();
  }

  function scratchPointerSample(event, point, needleLifted) {
    const sample = {
      pointerId: event.pointerId,
      angleRadians: Math.atan2(point.y - latestGeometry.cy, point.x - latestGeometry.cx),
      timeMs: pointerEventTimeMs(event),
      radius: Math.hypot(point.x - latestGeometry.cx, point.y - latestGeometry.cy),
      pressure: event.pressure,
      pointerType: event.pointerType,
      handContact: true,
    };
    if (typeof needleLifted === "boolean") sample.needleLifted = needleLifted;
    return sample;
  }

  function captureGesture(event, gesture) {
    activeGestures.set(event.pointerId, gesture);
    canvas.setPointerCapture(event.pointerId);
  }

  function pointerDown(event) {
    noteActivity();
    if (!latestGeometry || activeGestures.has(event.pointerId)) return;
    const point = localPointer(canvas, event);
    const region = regionAt(point);
    if (!region) return;
    if (region.kind === "lamp") {
      strobeLightOn = !strobeLightOn;
      scheduleRender();
      return;
    }
    if (region.kind === "sector-button" || region.kind === "round-button") {
      captureGesture(event, { kind: "button", region, pointerId: event.pointerId });
      return;
    }
    if (region.kind === "arc-slider") {
      captureGesture(event, { kind: "arc-slider", region, pointerId: event.pointerId });
      updateArcControl(region, latestGeometry, point);
      return;
    }
    if (region.kind === "needle-point" || region.kind === "needle-arc") {
      captureGesture(event, { kind: "needle", pointerId: event.pointerId });
      seekFromStylusPoint(point);
      return;
    }
    if (!snapshot.ready) {
      if (interaction.loadOnEmptyRecordTap) {
        player.openRecordPicker?.();
      }
      return;
    }
    if (snapshot.scratchReplayActive) return;
    if (hasRecordGesture()) return;
    const sampleRate = Math.max(1, Number(snapshot.sampleRate) || 48000);
    const nativeRpm = Math.max(1, Number(snapshot.nativeRpm) || 33.3333333333);
    const durationFrames = (Number(snapshot.durationSeconds) || 0) * sampleRate;
    const positionFrames = Number.isFinite(Number(snapshot.positionFrames))
      ? Number(snapshot.positionFrames)
      : Math.round((Number(snapshot.positionSeconds) || 0) * sampleRate);
    const tracker = createScratchGestureTracker({
      sampleRate,
      secondsPerTurn: 60 / nativeRpm,
      minPositionFrames: 0,
      maxPositionFrames: durationFrames > 0 ? durationFrames : Number.MAX_SAFE_INTEGER,
      minimumRadius: Math.max(8, latestGeometry.recordRadius * 0.06),
    });
    const motion = tracker.begin({
      ...scratchPointerSample(event, point, Boolean(snapshot.needleLifted)),
      positionFrames,
      rotationDegrees: visualRotation,
    });
    captureGesture(event, {
      kind: "record",
      pointerId: event.pointerId,
      tracker,
    });
    player.beginScratch({
      pointerId: event.pointerId,
      rotationDegrees: motion.rotationDegrees,
      positionFrames: motion.positionFrames,
      rate: motion.rate,
      impulse: motion.impulse,
      pressure: motion.pressure,
      grip: motion.grip,
      handContact: motion.handContact,
      needleLifted: motion.needleLifted,
      inputTimeMs: pointerEventTimeMs(event),
    });
  }

  function pointerMove(event) {
    noteActivity();
    const gesture = activeGestures.get(event.pointerId);
    if (!gesture || !latestGeometry) return;
    const samples = coalescedSamples(event);
    if (gesture.kind === "arc-slider") {
      for (const sample of samples) {
        updateArcControl(gesture.region, latestGeometry, localPointer(canvas, sample));
      }
      return;
    }
    if (gesture.kind === "needle") {
      for (const sample of samples) seekFromStylusPoint(localPointer(canvas, sample));
      return;
    }
    if (gesture.kind !== "record") return;
    for (const sample of samples) {
      const point = localPointer(canvas, sample);
      const motion = gesture.tracker.update(scratchPointerSample(sample, point));
      visualRotation = motion.rotationDegrees;
      player.updateScratch({
        positionFrames: motion.positionFrames,
        rate: motion.rate,
        rotationDegrees: motion.rotationDegrees,
        impulse: motion.impulse,
        direction: motion.direction,
        reversal: motion.reversal,
        acceleration: motion.acceleration,
        pressure: motion.pressure,
        grip: motion.grip,
        handContact: motion.handContact,
        needleLifted: motion.needleLifted,
        visualOnly: motion.visualOnly,
        inputTimeMs: pointerEventTimeMs(sample),
      });
    }
    scheduleRender();
  }

  function finishPointer(event, cancelled = false) {
    noteActivity();
    const gesture = activeGestures.get(event.pointerId);
    if (!gesture) return;
    activeGestures.delete(event.pointerId);
    if (gesture.kind === "button" && !cancelled) gesture.region.onActivate();
    if (gesture.kind === "record") {
      const motion = cancelled
        ? gesture.tracker.cancel({
          pointerId: event.pointerId,
          pressure: event.pressure,
          pointerType: event.pointerType,
        })
        : gesture.tracker.finish({
          pointerId: event.pointerId,
          pressure: event.pressure,
          pointerType: event.pointerType,
        });
      visualRotation = motion.rotationDegrees;
      rotationClockAtMs = performance.now();
      player.endScratch({
        rotationDegrees: motion.rotationDegrees,
        resumePlayback: true,
        cancelled: motion.cancelled,
        pressure: motion.pressure,
        grip: motion.grip,
        handContact: motion.handContact,
        inputTimeMs: pointerEventTimeMs(event),
      });
    }
    scheduleRender();
  }

  function pointerUp(event) {
    finishPointer(event, false);
  }

  function pointerCancel(event) {
    finishPointer(event, true);
  }

  const unsubscribe = player.subscribe(next => {
    const couldAutoFade = canAutoFadeUi(snapshot);
    const rotationNow = performance.now();
    advanceAudioOwnedRotation(rotationNow);
    snapshot = next;
    if (!hasRecordGesture()) {
      if (!next.scratchReplayActive && Number.isFinite(next.rotationDegrees)) {
        visualRotation = next.rotationDegrees;
      }
      rotationClockAtMs = rotationNow;
    }
    if (!canAutoFadeUi(next)) {
      clearIdleTimer();
      setUiFadeTarget(1);
    } else if (!couldAutoFade) {
      lastActivityAt = performance.now();
      setUiFadeTarget(1, lastActivityAt);
      scheduleIdleTimer();
    }
    scheduleRender();
  });

  const activityEvents = ["pointermove", "mousemove", "touchstart", "touchmove", "wheel", "keydown", "focusin"];
  for (const type of activityEvents) {
    activityTarget.addEventListener(type, noteActivity, { passive: true });
  }
  canvas.addEventListener("pointerdown", pointerDown);
  canvas.addEventListener("pointermove", pointerMove);
  canvas.addEventListener("pointerup", pointerUp);
  canvas.addEventListener("pointercancel", pointerCancel);
  canvas.addEventListener("lostpointercapture", pointerCancel);
  scheduleIdleTimer();
  scheduleRender();

  return Object.freeze({
    configure(next = {}) {
      if (next.components) components = { ...components, ...next.components };
      if (next.theme) theme = { ...theme, ...next.theme };
      if (Object.prototype.hasOwnProperty.call(next, "loadOnEmptyRecordTap")) {
        interaction = {
          ...interaction,
          loadOnEmptyRecordTap: Boolean(next.loadOnEmptyRecordTap)
        };
      }
      if (Object.prototype.hasOwnProperty.call(next, "recordFill")) {
        interaction = { ...interaction, recordFill: next.recordFill === true };
      }
      if (typeof next.strobeLightOn === "boolean") strobeLightOn = next.strobeLightOn;
      scheduleRender();
      return this.getConfig();
    },
    setComponentVisible(name, visible) {
      if (!(name in CANVAS_COMPONENTS)) throw new RangeError(`Unknown canvas component: ${name}`);
      components = { ...components, [name]: Boolean(visible) };
      scheduleRender();
      return { ...components };
    },
    setTheme(next = {}) {
      theme = { ...theme, ...next };
      scheduleRender();
      return { ...theme };
    },
    setStrobeLight(enabled) {
      strobeLightOn = Boolean(enabled);
      scheduleRender();
      return strobeLightOn;
    },
    getConfig() {
      return Object.freeze({
        components: { ...components },
        theme: { ...theme },
        interaction: { ...interaction },
        strobeLightOn
      });
    },
    destroy() {
      destroyed = true;
      cancelAnimationFrame(frame);
      clearIdleTimer();
      root.classList.remove("vinyl-ui-idle");
      for (const [pointerId, gesture] of activeGestures) {
        if (gesture.kind !== "record") continue;
        const motion = gesture.tracker.cancel({ pointerId });
        player.endScratch({
          rotationDegrees: motion.rotationDegrees,
          resumePlayback: true,
          cancelled: true,
          pressure: 0,
          grip: 0,
          handContact: false,
        });
      }
      activeGestures.clear();
      unsubscribe();
      for (const type of activityEvents) {
        activityTarget.removeEventListener(type, noteActivity, { passive: true });
      }
      canvas.removeEventListener("pointerdown", pointerDown);
      canvas.removeEventListener("pointermove", pointerMove);
      canvas.removeEventListener("pointerup", pointerUp);
      canvas.removeEventListener("pointercancel", pointerCancel);
      canvas.removeEventListener("lostpointercapture", pointerCancel);
    }
  });
}

export const VIN_YL_PLAYER_CANVAS_COMPONENTS = CANVAS_COMPONENTS;
export const VIN_YL_PLAYER_CANVAS_THEME = CANVAS_THEME;
