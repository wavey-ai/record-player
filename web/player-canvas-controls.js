import {
  FONT_FAMILY,
  PITCH_RANGE_PCT,
  arcValue,
  clamp,
  degToRad,
  minuteToDegrees,
  normalizeDegrees
} from "./player-canvas-geometry.js";
import { SCRATCH_PRESETS } from "./scratch-performance-schema.js";

const RPM_CENTER_MINUTE = 15;
const RPM_RESET_PITCH_PCT = -6;
const CHANNEL_CENTER_MINUTE = 45;
const XFADE_CENTER_MINUTE = 26;
const START_STOP_CENTER_MINUTE = 33;
const NEEDLE_CENTER_MINUTE = 8;
const LOAD_RECORD_MINUTE = 21;
const SCRATCH_PRESET_CENTER_MINUTE = 52;
const SCRATCH_CLICKS_CENTER_MINUTE = 58;

function drawCurvedTextSegments(ctx, geometry, segments, angleDeg, radius, size, flip = false) {
  const parts = [];

  segments.forEach(segment => {
    Array.from(String(segment.text || "")).forEach(char => {
      parts.push({
        char,
        color: segment.color
      });
    });
  });

  if (!parts.length) return;

  ctx.save();
  ctx.font = `700 ${size}px ${FONT_FAMILY}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";

  const widths = parts.map(part => ctx.measureText(part.char).width);
  const total = widths.reduce((sum, width) => sum + width, 0);
  let offset = -total / 2;
  const centerAngle = degToRad(angleDeg);

  parts.forEach((part, index) => {
    const charOffset = offset + widths[index] / 2;
    const angle = centerAngle + (flip ? -charOffset : charOffset) / radius;
    const x = geometry.cx + Math.cos(angle) * radius;
    const y = geometry.cy + Math.sin(angle) * radius;

    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(angle + (flip ? -Math.PI / 2 : Math.PI / 2));
    ctx.fillStyle = part.color;
    ctx.fillText(part.char, 0, 0);
    ctx.restore();

    offset += widths[index];
  });

  ctx.restore();
}

function drawCurvedText(ctx, geometry, text, angleDeg, radius, color, size, flip = false) {
  drawCurvedTextSegments(
    ctx,
    geometry,
    [{ text, color }],
    angleDeg,
    radius,
    size,
    flip
  );
}

function strokeArcBand(ctx, geometry, startDeg, endDeg, inner, outer, color) {
  const anticlockwise = endDeg < startDeg;

  ctx.beginPath();
  ctx.arc(
    geometry.cx,
    geometry.cy,
    outer,
    degToRad(startDeg),
    degToRad(endDeg),
    anticlockwise
  );
  ctx.arc(
    geometry.cx,
    geometry.cy,
    inner,
    degToRad(endDeg),
    degToRad(startDeg),
    !anticlockwise
  );
  ctx.closePath();
  ctx.strokeStyle = color;
  ctx.lineWidth = 1;
  ctx.stroke();
}

function radialTick(ctx, geometry, angleDeg, r0, r1, color, width = 1) {
  const angle = degToRad(angleDeg);

  ctx.beginPath();
  ctx.moveTo(
    geometry.cx + Math.cos(angle) * r0,
    geometry.cy + Math.sin(angle) * r0
  );
  ctx.lineTo(
    geometry.cx + Math.cos(angle) * r1,
    geometry.cy + Math.sin(angle) * r1
  );
  ctx.strokeStyle = color;
  ctx.lineWidth = width;
  ctx.stroke();
}

function sliderAngle(segment, value) {
  const normalized = segment.reverse
    ? 1 - clamp(value, 0, 1)
    : clamp(value, 0, 1);

  if (
    Number.isFinite(segment.neutralAngle) &&
    Number.isFinite(segment.neutralValue)
  ) {
    const neutralValue = clamp(segment.neutralValue, 0, 1);

    if (normalized >= neutralValue) {
      const ratio =
        neutralValue < 1
          ? (normalized - neutralValue) / (1 - neutralValue)
          : 0;

      return (
        segment.neutralAngle +
        (segment.endAngle - segment.neutralAngle) * ratio
      );
    }

    const ratio =
      neutralValue > 0
        ? (neutralValue - normalized) / neutralValue
        : 0;

    return (
      segment.neutralAngle +
      (segment.startAngle - segment.neutralAngle) * ratio
    );
  }

  return (
    segment.startAngle +
    (segment.endAngle - segment.startAngle) * normalized
  );
}

function drawEndLabels(ctx, geometry, segment, theme, inner, outer) {
  if (!segment.endLabels) return;

  const radius = outer + 9 * geometry.scale;
  const size = 7 * geometry.scale;

  drawCurvedText(
    ctx,
    geometry,
    segment.endLabels[0],
    segment.startAngle,
    radius,
    theme.controlText,
    size,
    segment.flip
  );

  drawCurvedText(
    ctx,
    geometry,
    segment.endLabels[1],
    segment.endAngle,
    radius,
    theme.controlText,
    size,
    segment.flip
  );
}

function drawSlider(ctx, geometry, segment, theme, labels, hitRegions) {
  const inner = geometry.controlBandInner;
  const outer = geometry.controlBandOuter;
  const mid = (inner + outer) / 2;

  strokeArcBand(
    ctx,
    geometry,
    segment.startAngle,
    segment.endAngle,
    inner,
    outer,
    theme.line
  );

  const angle = sliderAngle(segment, segment.value);

  radialTick(
    ctx,
    geometry,
    angle,
    inner - 1,
    outer + 1,
    theme.accent,
    Math.max(2, 2 * geometry.scale)
  );

  if (Number.isFinite(segment.defaultValue)) {
    radialTick(
      ctx,
      geometry,
      sliderAngle(segment, segment.defaultValue),
      inner,
      outer,
      theme.mutedLine,
      1
    );
  }

  if (segment.key === "rpm") {
    for (let pct = -PITCH_RANGE_PCT; pct <= PITCH_RANGE_PCT; pct += 1) {
      const ratio = 0.5 + pct / (PITCH_RANGE_PCT * 2);
      const tickAngle = sliderAngle(segment, ratio);
      const length = pct % 2 === 0 ? 5 : 3;

      radialTick(
        ctx,
        geometry,
        tickAngle,
        outer + geometry.scale,
        outer + length * geometry.scale,
        theme.mutedLine,
        1
      );
    }

    const ledAngle = sliderAngle(segment, 0.5);
    const ledRadius = outer + 15 * geometry.scale;
    const ledX = geometry.cx + Math.cos(degToRad(ledAngle)) * ledRadius;
    const ledY = geometry.cy + Math.sin(degToRad(ledAngle)) * ledRadius;

    ctx.fillStyle =
      Math.abs(segment.value - 0.5) < 0.001
        ? theme.accent
        : theme.controlFill;
    ctx.strokeStyle = theme.accent;
    ctx.lineWidth = 1;

    ctx.fillRect(
      ledX - 3 * geometry.scale,
      ledY - 3 * geometry.scale,
      6 * geometry.scale,
      6 * geometry.scale
    );

    ctx.strokeRect(
      ledX - 3 * geometry.scale,
      ledY - 3 * geometry.scale,
      6 * geometry.scale,
      6 * geometry.scale
    );
  }

  if (labels) {
    if (segment.valueLabel) {
      drawCurvedText(
        ctx,
        geometry,
        segment.valueLabel,
        angle - 4,
        mid,
        theme.controlText,
        8 * geometry.scale,
        segment.flip
      );
    }

    drawEndLabels(
      ctx,
      geometry,
      segment,
      theme,
      inner,
      outer
    );
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

function withAlpha(hex, alpha) {
  const clean = String(hex || "").replace("#", "");
  const full =
    clean.length === 3
      ? clean
        .split("")
        .map(channel => channel + channel)
        .join("")
      : clean;

  const value = parseInt(full, 16);
  const r = (value >> 16) & 255;
  const g = (value >> 8) & 255;
  const b = value & 255;
  const mix = channel =>
    Math.round(channel * alpha + 255 * (1 - alpha));

  return `rgb(${mix(r)}, ${mix(g)}, ${mix(b)})`;
}

function drawNeedleLabel(ctx, geometry, button, theme, inner, outer, labels) {
  if (!labels) return;

  const dim = withAlpha(theme.controlText, 0.7);

  drawCurvedTextSegments(
    ctx,
    geometry,
    [
      {
        text: "DOWN",
        color: button.needleLifted ? dim : theme.controlText
      },
      {
        text: " — ",
        color: dim
      },
      {
        text: "UP",
        color: button.needleLifted ? theme.controlText : dim
      }
    ],
    button.angle,
    (inner + outer) / 2,
    8 * geometry.scale,
    button.angle > 0 && button.angle < 180
  );
}

function drawSectorButton(ctx, geometry, button, theme, labels, hitRegions) {
  const bandThickness =
    geometry.controlBandOuter -
    geometry.controlBandInner;

  const inner =
    geometry.controlBandInner -
    bandThickness / 6;

  const outer =
    geometry.controlBandOuter +
    bandThickness / 6;

  const start = degToRad(button.angle - button.span / 2);
  const end = degToRad(button.angle + button.span / 2);

  ctx.beginPath();
  ctx.arc(geometry.cx, geometry.cy, outer, start, end);
  ctx.arc(geometry.cx, geometry.cy, inner, end, start, true);
  ctx.closePath();

  ctx.fillStyle = button.active
    ? theme.controlActive
    : theme.controlFill;

  ctx.fill();
  ctx.strokeStyle = theme.line;
  ctx.lineWidth = 1;
  ctx.stroke();

  if (button.glyph === "needle-lift") {
    drawNeedleLabel(
      ctx,
      geometry,
      button,
      theme,
      inner,
      outer,
      labels
    );
  } else if (button.key === "startStop") {
    if (labels) {
      const dim = withAlpha(theme.controlText, 0.7);

      drawCurvedTextSegments(
        ctx,
        geometry,
        [
          {
            text: "START",
            color: button.playing ? theme.controlText : dim
          },
          {
            text: " — ",
            color: dim
          },
          {
            text: "STOP",
            color: button.playing ? dim : theme.controlText
          }
        ],
        button.angle,
        (inner + outer) / 2,
        9 * geometry.scale,
        button.angle > 0 && button.angle < 180
      );
    }
  } else if (labels) {
    drawCurvedText(
      ctx,
      geometry,
      button.label,
      button.angle,
      (inner + outer) / 2,
      button.active
        ? theme.controlActiveText
        : theme.controlText,
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
  const rpmCenter = minuteToDegrees(RPM_CENTER_MINUTE);
  const rpmHalf = 22;
  const channelCenter = minuteToDegrees(CHANNEL_CENTER_MINUTE);
  const xfadeCenter = minuteToDegrees(XFADE_CENTER_MINUTE);
  const native = Number(state.nativeRpm) || 33.3333333333;
  const pitchRatio = clamp(
    ((Number(state.rpm) || native) / native - 0.92) / 0.16,
    0,
    1
  );

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
      reverse: true,
      valueLabel:
        pitchRatio === 0.5
          ? "0.0"
          : `${pitchRatio > 0.5 ? "+" : "-"}${Math.abs(
            (pitchRatio - 0.5) * 16
          ).toFixed(1)}`,
      endLabels: ["+8", "-8"],
      onInput: value =>
        player.setRpm(
          native *
          (0.92 + value * 0.16)
        )
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
      onInput: value =>
        player.setVolume(value)
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
      onInput: value =>
        player.setCrossfader(value)
    });
  }

  return segments;
}

function drawPitchResetButton(
  ctx,
  geometry,
  player,
  state,
  theme,
  hitRegions
) {
  const native = Number(state.nativeRpm) || 33.3333333333;
  const rpmCenter = minuteToDegrees(RPM_CENTER_MINUTE);
  const rpmHalf = 22;
  const resetRatio =
    0.5 +
    RPM_RESET_PITCH_PCT /
    (PITCH_RANGE_PCT * 2);

  const angle = sliderAngle(
    {
      startAngle: rpmCenter - rpmHalf,
      endAngle: rpmCenter + rpmHalf,
      neutralAngle: rpmCenter,
      neutralValue: 0.5,
      reverse: true
    },
    resetRatio
  );

  const radius =
    geometry.controlBandOuter +
    24 * geometry.scale;

  const x =
    geometry.cx +
    Math.cos(degToRad(angle)) * radius;

  const y =
    geometry.cy +
    Math.sin(degToRad(angle)) * radius;

  const buttonRadius = Math.max(
    10,
    geometry.buttonHeight * 0.42
  );

  ctx.save();
  ctx.beginPath();
  ctx.arc(x, y, buttonRadius, 0, Math.PI * 2);
  ctx.strokeStyle = theme.line;
  ctx.lineWidth = 1;
  ctx.stroke();

  ctx.translate(x, y);
  ctx.rotate(degToRad(angle));
  ctx.fillStyle = theme.controlText;
  ctx.font = `700 ${Math.max(
    5,
    5.5 * geometry.scale
  )}px ${FONT_FAMILY}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(
    "RESET",
    0,
    0.5 * geometry.scale
  );
  ctx.restore();

  hitRegions.push({
    key: "pitchReset",
    kind: "round-button",
    x,
    y,
    radius: buttonRadius * 1.25,
    onActivate: () =>
      player.setRpm(native)
  });
}

export function drawRadialControls(
  ctx,
  geometry,
  player,
  state,
  components,
  theme,
  hitRegions
) {
  const segments = createControlSegments(
    player,
    state,
    components
  );

  segments.forEach(segment =>
    drawSlider(
      ctx,
      geometry,
      segment,
      theme,
      components.labels,
      hitRegions
    )
  );

  if (components.rpm) {
    drawPitchResetButton(
      ctx,
      geometry,
      player,
      state,
      theme,
      hitRegions
    );
  }

  const mid =
    (geometry.controlBandInner +
      geometry.controlBandOuter) /
    2;

  const textSize = 9 * geometry.scale;
  const padding = 18 * geometry.scale;

  const measureSpan = text => {
    ctx.save();
    ctx.font = `700 ${textSize}px ${FONT_FAMILY}`;
    const textWidth = ctx.measureText(text).width;
    ctx.restore();

    return (
      ((textWidth + padding) / mid) *
      (180 / Math.PI)
    );
  };

  const startStopSpan = measureSpan("START — STOP");

  if (components.startStop) {
    drawSectorButton(
      ctx,
      geometry,
      {
        key: "startStop",
        label: "START — STOP",
        active: false,
        playing: Boolean(state.motorRunning),
        angle: minuteToDegrees(START_STOP_CENTER_MINUTE),
        span: startStopSpan,
        onActivate: () =>
          player.toggleTransport()
      },
      theme,
      components.labels,
      hitRegions
    );
  }

  if (components.needle) {
    drawSectorButton(
      ctx,
      geometry,
      {
        key: "needle",
        glyph: "needle-lift",
        active: false,
        needleLifted: Boolean(state.needleLifted),
        angle: minuteToDegrees(NEEDLE_CENTER_MINUTE),
        span: startStopSpan,
        onActivate: () =>
          player.setNeedleLifted(
            !state.needleLifted
          )
      },
      theme,
      components.labels,
      hitRegions
    );
  }

  if (components.loadRecord) {
    const loadRecordSpan = measureSpan("LOAD RECORD");

    drawSectorButton(
      ctx,
      geometry,
      {
        key: "loadRecord",
        label: "LOAD RECORD",
        active: false,
        angle: minuteToDegrees(LOAD_RECORD_MINUTE),
        span: loadRecordSpan,
        onActivate: () =>
          document
            .querySelector("#file")
            ?.click()
      },
      theme,
      components.labels,
      hitRegions
    );
  }

  if (components.scratchPreset) {
    const preset = String(state.scratchPreset || "baby").toLowerCase();
    const presetIndex = Math.max(0, SCRATCH_PRESETS.indexOf(preset));
    const label = `SCRATCH ${SCRATCH_PRESETS[presetIndex].toUpperCase()}`;

    drawSectorButton(
      ctx,
      geometry,
      {
        key: "scratchPreset",
        label,
        active: false,
        angle: minuteToDegrees(SCRATCH_PRESET_CENTER_MINUTE),
        span: measureSpan(label),
        onActivate: () => player.setScratchPreset(
          SCRATCH_PRESETS[(presetIndex + 1) % SCRATCH_PRESETS.length]
        )
      },
      theme,
      components.labels,
      hitRegions
    );
  }

  if (components.scratchClicks) {
    const clicks = clamp(Math.round(Number(state.scratchClicks) || 1), 1, 8);
    const label = `CLICKS ${clicks}`;

    drawSectorButton(
      ctx,
      geometry,
      {
        key: "scratchClicks",
        label,
        active: false,
        angle: minuteToDegrees(SCRATCH_CLICKS_CENTER_MINUTE),
        span: measureSpan(label),
        onActivate: () => player.setScratchClicks(clicks === 8 ? 1 : clicks + 1)
      },
      theme,
      components.labels,
      hitRegions
    );
  }

  return segments;
}

export function controlAt(
  hitRegions,
  geometry,
  point
) {
  const dx = point.x - geometry.cx;
  const dy = point.y - geometry.cy;
  const radius = Math.hypot(dx, dy);

  const angle = normalizeDegrees(
    Math.atan2(dy, dx) *
    180 /
    Math.PI
  );

  return (
    [...hitRegions]
      .reverse()
      .find(region => {
        if (region.kind === "round-button") {
          return (
            Math.hypot(
              point.x - region.x,
              point.y - region.y
            ) <= region.radius
          );
        }

        if (
          radius < region.inner ||
          radius > region.outer
        ) {
          return false;
        }

        if (region.kind === "arc-slider") {
          const value = arcValue(
            angle,
            region.startAngle,
            region.endAngle,
            region.reverse
          );

          const projected = sliderAngle(
            {
              startAngle: region.startAngle,
              endAngle: region.endAngle,
              reverse: region.reverse
            },
            value
          );

          const delta = Math.abs(
            ((angle - projected + 540) % 360) - 180
          );

          return delta <= 16;
        }

        const delta = Math.abs(
          ((angle - region.angle + 540) % 360) - 180
        );

        return (
          delta <= region.span / 2 + 2
        );
      }) || null
  );
}

export function updateArcControl(
  region,
  geometry,
  point
) {
  const angle = normalizeDegrees(
    Math.atan2(
      point.y - geometry.cy,
      point.x - geometry.cx
    ) *
    180 /
    Math.PI
  );

  region.onInput(
    arcValue(
      angle,
      region.startAngle,
      region.endAngle,
      region.reverse
    )
  );
}
