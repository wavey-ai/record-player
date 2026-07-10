import "./player-message-logger-global.js";

const logger = globalThis.VinylPlayerMessageLogger;

if (!logger) {
  throw new Error("VinylPlayerMessageLogger global implementation was not loaded.");
}

export const createLogger = logger.createLogger;
export const setPlayerLoggingEnabled = logger.setEnabled;
export const isPlayerLoggingEnabled = logger.isEnabled;
export const isPlayerVerboseLoggingEnabled = logger.isVerboseEnabled;
export const isPayerVerboseLoggingEnabled = logger.isVerboseEnabled;
export const IsPlayerVerboseLoggingEnabled = logger.isVerboseEnabled;
export const IsPayerVerboseLoggingEnabled = logger.isVerboseEnabled;

export const setPlayerLogLevel = logger.setLevel;
export const getPlayerLogLevel = logger.getLevel;
export const setPlayerTelemetryInterval = logger.setTelemetryInterval;
