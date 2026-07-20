const TAU = Math.PI * 2;

export const SCRATCH_GESTURE_DEFAULTS = Object.freeze({
  sampleRate: 48000,
  secondsPerTurn: 1.8,
  minPositionFrames: 0,
  maxPositionFrames: Number.MAX_SAFE_INTEGER,
  minimumRadius: 0,
  minimumDeltaSeconds: 0.004,
  steadyFilterSeconds: 0.035,
  reversalFilterSeconds: 0.008,
  directionEnterRate: 0.035,
  directionExitRate: 0.018,
  reversalRateThreshold: 0.08,
  accelerationThreshold: 10,
  accelerationReleaseThreshold: 4,
  accelerationImpulseGain: 0.004,
  accelerationImpulseMinimum: 0.02,
  accelerationImpulseMaximum: 0.16,
  reversalImpulse: 0.18,
  grabImpulse: 0.22,
  impulseScale: 0.35,
  impulseCooldownSeconds: 0.024,
  defaultGrip: 1,
});

function finite(value, fallback) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function positive(value, fallback) {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? number : fallback;
}

function nonNegative(value, fallback) {
  const number = Number(value);
  return Number.isFinite(number) && number >= 0 ? number : fallback;
}

function clamp(value, low, high) {
  return Math.max(low, Math.min(high, value));
}

function clamp01(value) {
  return clamp(value, 0, 1);
}

function assertFinite(value, name) {
  const number = Number(value);
  if (!Number.isFinite(number)) throw new TypeError(`${name} must be finite`);
  return number;
}

/**
 * Resolve one incremental angular step without losing accumulated turns.
 * Samples still need to be frequent enough that one physical step is under
 * half a revolution; coalesced samples should therefore be fed sequentially.
 */
export function unwrapScratchAngle(deltaRadians) {
  let delta = assertFinite(deltaRadians, "deltaRadians") % TAU;
  if (delta > Math.PI) delta -= TAU;
  if (delta < -Math.PI) delta += TAU;
  return delta;
}

function trackerConfig(options) {
  const defaults = SCRATCH_GESTURE_DEFAULTS;
  const minPositionFrames = finite(options.minPositionFrames, defaults.minPositionFrames);
  const requestedMaximum = finite(options.maxPositionFrames, defaults.maxPositionFrames);
  const directionEnterRate = positive(options.directionEnterRate, defaults.directionEnterRate);
  const directionExitRate = Math.min(
    directionEnterRate,
    nonNegative(options.directionExitRate, defaults.directionExitRate),
  );
  const accelerationThreshold = positive(
    options.accelerationThreshold,
    defaults.accelerationThreshold,
  );
  const accelerationImpulseMinimum = nonNegative(
    options.accelerationImpulseMinimum,
    defaults.accelerationImpulseMinimum,
  );
  const accelerationImpulseMaximum = Math.max(
    accelerationImpulseMinimum,
    nonNegative(
      options.accelerationImpulseMaximum,
      defaults.accelerationImpulseMaximum,
    ),
  );

  return Object.freeze({
    sampleRate: positive(options.sampleRate, defaults.sampleRate),
    secondsPerTurn: positive(options.secondsPerTurn, defaults.secondsPerTurn),
    minPositionFrames,
    maxPositionFrames: Math.max(minPositionFrames, requestedMaximum),
    minimumRadius: nonNegative(options.minimumRadius, defaults.minimumRadius),
    minimumDeltaSeconds: positive(
      options.minimumDeltaSeconds,
      defaults.minimumDeltaSeconds,
    ),
    steadyFilterSeconds: positive(
      options.steadyFilterSeconds,
      defaults.steadyFilterSeconds,
    ),
    reversalFilterSeconds: positive(
      options.reversalFilterSeconds,
      defaults.reversalFilterSeconds,
    ),
    directionEnterRate,
    directionExitRate,
    reversalRateThreshold: positive(
      options.reversalRateThreshold,
      defaults.reversalRateThreshold,
    ),
    accelerationThreshold,
    accelerationReleaseThreshold: Math.min(
      accelerationThreshold,
      nonNegative(
        options.accelerationReleaseThreshold,
        defaults.accelerationReleaseThreshold,
      ),
    ),
    accelerationImpulseGain: nonNegative(
      options.accelerationImpulseGain,
      defaults.accelerationImpulseGain,
    ),
    accelerationImpulseMinimum,
    accelerationImpulseMaximum,
    reversalImpulse: clamp01(nonNegative(options.reversalImpulse, defaults.reversalImpulse)),
    grabImpulse: clamp01(nonNegative(options.grabImpulse, defaults.grabImpulse)),
    impulseScale: clamp01(nonNegative(options.impulseScale, defaults.impulseScale)),
    impulseCooldownSeconds: nonNegative(
      options.impulseCooldownSeconds,
      defaults.impulseCooldownSeconds,
    ),
    defaultGrip: clamp01(nonNegative(options.defaultGrip, defaults.defaultGrip)),
  });
}

function pointerTelemetry(sample, defaultGrip, active = true) {
  const handContact = active && sample.handContact !== false;
  const hasPressure = Number.isFinite(Number(sample.pressure));
  const pressure = hasPressure ? clamp01(Number(sample.pressure)) : 0;
  const pointerType = typeof sample.pointerType === "string" ? sample.pointerType : "";
  const explicitGrip = Number(sample.grip);
  const grip = handContact
    ? clamp01(
      Number.isFinite(explicitGrip)
        ? explicitGrip
        : pointerType === "pen" && hasPressure
          ? pressure
          : defaultGrip,
    )
    : 0;
  return {
    pressure,
    pressureAvailable: hasPressure,
    grip,
    handContact,
    pointerType,
  };
}

function isReliableRadius(sample, minimumRadius) {
  if (!(minimumRadius > 0)) return true;
  const radius = Number(sample.radius);
  return !Number.isFinite(radius) || radius >= minimumRadius;
}

/**
 * A deterministic, DOM-free gesture tracker. One instance owns one pointer.
 * Call update once for every hardware/coalesced sample, in timestamp order.
 */
export class ScratchGestureTracker {
  constructor(options = {}) {
    this.config = trackerConfig(options);
    this.active = false;
    this.pointerId = null;
    this.positionFrames = this.config.minPositionFrames;
    this.rotationDegrees = 0;
    this.totalAngleRadians = 0;
    this.lastAngleRadians = 0;
    this.lastTimeMs = 0;
    this.filteredRate = 0;
    this.direction = 0;
    this.lastStrongDirection = 0;
    this.accelerationArmed = true;
    this.lastImpulseTimeMs = -Infinity;
    this.angleReliable = true;
    this.needleLifted = false;
    this.lastTelemetry = pointerTelemetry({}, this.config.defaultGrip, false);
  }

  begin(sample = {}) {
    const angleRadians = assertFinite(sample.angleRadians, "angleRadians");
    const timeMs = assertFinite(sample.timeMs, "timeMs");
    const requestedPosition = finite(sample.positionFrames, this.config.minPositionFrames);
    const positionFrames = clamp(
      requestedPosition,
      this.config.minPositionFrames,
      this.config.maxPositionFrames,
    );

    this.active = true;
    this.pointerId = sample.pointerId ?? 0;
    this.positionFrames = positionFrames;
    this.rotationDegrees = finite(sample.rotationDegrees, 0);
    this.totalAngleRadians = 0;
    this.lastAngleRadians = angleRadians;
    this.lastTimeMs = timeMs;
    this.filteredRate = 0;
    this.direction = 0;
    this.lastStrongDirection = 0;
    this.accelerationArmed = true;
    this.lastImpulseTimeMs = timeMs;
    this.angleReliable = isReliableRadius(sample, this.config.minimumRadius);
    this.needleLifted = Boolean(sample.needleLifted);
    this.lastTelemetry = pointerTelemetry(sample, this.config.defaultGrip);

    return this.#motion({
      phase: "begin",
      elapsedSeconds: 0,
      deltaAngleRadians: 0,
      rawRate: 0,
      acceleration: 0,
      reversal: false,
      impulse: this.config.grabImpulse,
      positionClamped: requestedPosition !== positionFrames,
      ignored: !this.angleReliable,
      ignoreReason: this.angleReliable ? "" : "near-spindle",
      visualOnly: this.needleLifted,
    });
  }

  update(sample = {}) {
    if (!this.active) throw new Error("scratch gesture is not active");
    if (sample.pointerId != null && sample.pointerId !== this.pointerId) {
      throw new Error("scratch gesture pointer does not match");
    }

    const angleRadians = assertFinite(sample.angleRadians, "angleRadians");
    const suppliedTimeMs = assertFinite(sample.timeMs, "timeMs");
    const elapsedSeconds = Math.max(
      this.config.minimumDeltaSeconds,
      Math.max(0, suppliedTimeMs - this.lastTimeMs) / 1000,
    );
    const timeMs = Math.max(this.lastTimeMs, suppliedTimeMs);
    const telemetry = pointerTelemetry(sample, this.config.defaultGrip);
    const radiusReliable = isReliableRadius(sample, this.config.minimumRadius);
    this.needleLifted = Object.prototype.hasOwnProperty.call(sample, "needleLifted")
      ? Boolean(sample.needleLifted)
      : this.needleLifted;
    this.lastTelemetry = telemetry;

    if (!radiusReliable) {
      this.lastTimeMs = timeMs;
      this.angleReliable = false;
      this.#resetKinematics(true);
      return this.#motion({
        phase: "move",
        elapsedSeconds,
        deltaAngleRadians: 0,
        rawRate: 0,
        acceleration: 0,
        reversal: false,
        impulse: 0,
        positionClamped: false,
        ignored: true,
        ignoreReason: "near-spindle",
        visualOnly: this.needleLifted,
      });
    }

    if (!this.angleReliable) {
      this.lastAngleRadians = angleRadians;
      this.lastTimeMs = timeMs;
      this.angleReliable = true;
      this.#resetKinematics(true);
      return this.#motion({
        phase: "move",
        elapsedSeconds,
        deltaAngleRadians: 0,
        rawRate: 0,
        acceleration: 0,
        reversal: false,
        impulse: 0,
        positionClamped: false,
        ignored: true,
        ignoreReason: "angle-reacquired",
        visualOnly: this.needleLifted,
      });
    }

    const deltaAngleRadians = unwrapScratchAngle(angleRadians - this.lastAngleRadians);
    this.lastAngleRadians = angleRadians;
    this.lastTimeMs = timeMs;
    this.totalAngleRadians += deltaAngleRadians;
    this.rotationDegrees += deltaAngleRadians * 180 / Math.PI;

    if (this.needleLifted) {
      this.#resetKinematics(true);
      return this.#motion({
        phase: "move",
        elapsedSeconds,
        deltaAngleRadians,
        rawRate: 0,
        acceleration: 0,
        reversal: false,
        impulse: 0,
        positionClamped: false,
        ignored: false,
        ignoreReason: "",
        visualOnly: true,
      });
    }

    const deltaSeconds = deltaAngleRadians / TAU * this.config.secondsPerTurn;
    const rawRate = deltaSeconds / elapsedSeconds;
    const previousRate = this.filteredRate;
    const inputIsReversing = (
      Math.abs(previousRate) >= this.config.directionEnterRate
      && Math.abs(rawRate) >= this.config.directionEnterRate
      && Math.sign(previousRate) !== Math.sign(rawRate)
    );
    const filterSeconds = inputIsReversing
      ? Math.min(this.config.steadyFilterSeconds, this.config.reversalFilterSeconds)
      : this.config.steadyFilterSeconds;
    const alpha = 1 - Math.exp(-elapsedSeconds / filterSeconds);
    this.filteredRate = previousRate + (rawRate - previousRate) * alpha;
    const acceleration = (this.filteredRate - previousRate) / elapsedSeconds;
    const requestedPosition = this.positionFrames + deltaSeconds * this.config.sampleRate;
    this.positionFrames = clamp(
      requestedPosition,
      this.config.minPositionFrames,
      this.config.maxPositionFrames,
    );

    const previousDirection = this.direction;
    this.direction = this.#nextDirection(this.filteredRate);
    const strongDirection = Math.abs(this.filteredRate) >= this.config.reversalRateThreshold
      ? Math.sign(this.filteredRate)
      : 0;
    const reversal = Boolean(
      strongDirection
      && this.lastStrongDirection
      && strongDirection !== this.lastStrongDirection
    );
    if (strongDirection) this.lastStrongDirection = strongDirection;

    const accelerationMagnitude = Math.abs(acceleration);
    if (accelerationMagnitude <= this.config.accelerationReleaseThreshold) {
      this.accelerationArmed = true;
    }
    let impulse = 0;
    if (reversal) {
      impulse = this.config.reversalImpulse * this.config.impulseScale;
      this.accelerationArmed = false;
      this.lastImpulseTimeMs = timeMs;
    } else if (
      this.accelerationArmed
      && accelerationMagnitude >= this.config.accelerationThreshold
      && Math.abs(this.filteredRate) >= this.config.directionEnterRate
      && (timeMs - this.lastImpulseTimeMs) / 1000 >= this.config.impulseCooldownSeconds
    ) {
      impulse = clamp(
        accelerationMagnitude * this.config.accelerationImpulseGain,
        this.config.accelerationImpulseMinimum,
        this.config.accelerationImpulseMaximum,
      ) * this.config.impulseScale;
      this.accelerationArmed = false;
      this.lastImpulseTimeMs = timeMs;
    }

    return this.#motion({
      phase: "move",
      elapsedSeconds,
      deltaAngleRadians,
      rawRate,
      acceleration,
      reversal,
      impulse,
      positionClamped: requestedPosition !== this.positionFrames,
      ignored: false,
      ignoreReason: "",
      visualOnly: false,
      directionChanged: previousDirection !== this.direction,
    });
  }

  updateMany(samples) {
    return Array.from(samples || [], sample => this.update(sample));
  }

  finish(sample = {}) {
    if (!this.active) return null;
    if (sample.pointerId != null && sample.pointerId !== this.pointerId) {
      throw new Error("scratch gesture pointer does not match");
    }
    const pointerId = this.pointerId;
    const telemetry = pointerTelemetry(
      { ...sample, handContact: false, grip: 0 },
      this.config.defaultGrip,
      false,
    );
    this.lastTelemetry = telemetry;
    this.active = false;
    this.pointerId = null;
    this.#resetKinematics(false);
    return Object.freeze({
      phase: "end",
      pointerId,
      positionFrames: this.positionFrames,
      rate: 0,
      rawRate: 0,
      filteredRate: 0,
      acceleration: 0,
      direction: 0,
      directionChanged: false,
      reversal: false,
      rotationDegrees: this.rotationDegrees,
      deltaAngleRadians: 0,
      totalAngleRadians: this.totalAngleRadians,
      elapsedSeconds: 0,
      impulse: 0,
      pressure: telemetry.pressure,
      pressureAvailable: telemetry.pressureAvailable,
      grip: 0,
      handContact: false,
      pointerType: telemetry.pointerType,
      needleLifted: this.needleLifted,
      visualOnly: this.needleLifted,
      positionClamped: false,
      ignored: false,
      ignoreReason: "",
      cancelled: Boolean(sample.cancelled),
    });
  }

  cancel(sample = {}) {
    return this.finish({ ...sample, cancelled: true });
  }

  #nextDirection(rate) {
    if (this.direction > 0) {
      if (rate <= -this.config.directionEnterRate) return -1;
      if (rate <= this.config.directionExitRate) return 0;
      return 1;
    }
    if (this.direction < 0) {
      if (rate >= this.config.directionEnterRate) return 1;
      if (rate >= -this.config.directionExitRate) return 0;
      return -1;
    }
    if (rate >= this.config.directionEnterRate) return 1;
    if (rate <= -this.config.directionEnterRate) return -1;
    return 0;
  }

  #resetKinematics(resetStrongDirection) {
    this.filteredRate = 0;
    this.direction = 0;
    if (resetStrongDirection) this.lastStrongDirection = 0;
    this.accelerationArmed = true;
  }

  #motion(values) {
    const telemetry = this.lastTelemetry;
    return Object.freeze({
      phase: values.phase,
      pointerId: this.pointerId,
      positionFrames: this.positionFrames,
      rate: this.filteredRate,
      rawRate: values.rawRate,
      filteredRate: this.filteredRate,
      acceleration: values.acceleration,
      direction: this.direction,
      directionChanged: Boolean(values.directionChanged),
      reversal: values.reversal,
      rotationDegrees: this.rotationDegrees,
      deltaAngleRadians: values.deltaAngleRadians,
      totalAngleRadians: this.totalAngleRadians,
      elapsedSeconds: values.elapsedSeconds,
      impulse: values.impulse,
      pressure: telemetry.pressure,
      pressureAvailable: telemetry.pressureAvailable,
      grip: telemetry.grip,
      handContact: telemetry.handContact,
      pointerType: telemetry.pointerType,
      needleLifted: this.needleLifted,
      visualOnly: values.visualOnly,
      positionClamped: values.positionClamped,
      ignored: values.ignored,
      ignoreReason: values.ignoreReason,
      cancelled: false,
    });
  }
}

export function createScratchGestureTracker(options = {}) {
  return new ScratchGestureTracker(options);
}
