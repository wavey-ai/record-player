import "./player-message-logger-global.js";

const logger = globalThis.VinylPlayerMessageLogger;

if (!logger) {
  throw new Error("VinylPlayerMessageLogger global implementation was not loaded.");
}

export const createLogger = logger.createLogger;
export const setPlayerLoggingEnabled = logger.setEnabled;
export const isPlayerLoggingEnabled = logger.isEnabled;

export const setPlayerLogLevel = logger.setLevel;
export const getPlayerLogLevel = logger.getLevel;
export const setPlayerTelemetryInterval = logger.setTelemetryInterval;
