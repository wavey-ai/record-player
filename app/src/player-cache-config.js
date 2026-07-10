"use strict";

(() => {
  const OPUS_PACKET_AUDIO_FORMAT = "soundkit_opus_packets";
  const OPUS_CACHE_FRAME_DURATION_MS = 20;
  const OPUS_CACHE_FRAME_RATE = 1000 / OPUS_CACHE_FRAME_DURATION_MS;
  const OPUS_CACHE_BITRATE = 64000;
  const PLAYER_DECODE_SEGMENT_CACHE_STORE_NAME = "opus-chunks";
  const PLAYER_WAVEFORM_CACHE_STORE_NAME = "waveform-peaks";
  const PLAYER_DECODE_SEGMENT_REMOTE_CACHE_FORMAT = OPUS_PACKET_AUDIO_FORMAT;
  const PLAYER_DECODE_SEGMENT_REMOTE_CACHE_FORMATS = new Set([
    PLAYER_DECODE_SEGMENT_REMOTE_CACHE_FORMAT,
    "opus",
    "opus_packet",
    "opus_packets",
    "soundkit_opus_packet",
    "soundkit_opus_packets",
  ]);
  const PLAYER_DECODE_SEGMENT_CACHE_MAX_ENTRIES = 512;
  const PLAYER_WAVEFORM_CACHE_MAX_ENTRIES = 1024;
  const PLAYER_DECODED_SEGMENT_CACHE_KEY_VERSION = "bitneedle-player-decoded-ecdc-opus-v2";
  const PLAYER_DECODED_ECDC_SEGMENT_CACHE_KEY_VERSION = "bitneedle-opus-chunk-cache-v3";
  const PLAYER_DECODED_ECDC_SEGMENT_CACHE_WRITE_BATCH_SIZE = 4;
  const MOBILE_LP_PARSE_CACHE_CUTOFF_SECONDS = 60;
  const PLAYER_WAVEFORM_BUCKET_COUNT = 960;
  const PLAYER_WAVEFORM_BUCKET_COUNT_MOBILE = 480;
  const PLAYER_REMOTE_CACHE_ALLOWED_HOSTS = new Set([
    "wavey.ai",
    "www.wavey.ai",
    "yl.vin",
    "www.yl.vin",
    "local.bitneedle.com",
    "local.infidelity.io",
  ]);
  const PLAYER_REMOTE_CACHE_PUBLIC_API_BASE_URL = "https://yl.vin/api/bitneedle-player/cache";
  const PLAYER_REMOTE_CACHE_STREAM_CONTENT_TYPE = "application/vnd.bitneedle.player-cache-stream+binary";
  const PLAYER_REMOTE_CACHE_BATCH_FORMAT = "bitneedle-player-cache-batch-v1";
  const PLAYER_DISABLE_ALL_CACHING = false;
  // All chunk keys are known from the record header proof, so availability is
  // checked in a single request; 512 is the worker's per-batch key cap.
  const PLAYER_REMOTE_CACHE_BATCH_SIZE = 512;
  const PLAYER_REMOTE_CACHE_BATCH_WRITE_SIZE = 32;
  const PLAYER_REMOTE_CACHE_BATCH_RETRY_DELAYS_MS = Object.freeze([180, 600]);

  function resolveDefaultPlayerRemoteCacheApiBaseUrl({
    hostname = globalThis.location?.hostname || "",
    isLocalHost = false,
    isPrivateLanHost = false,
  } = {}) {
    const normalizedHostname = String(hostname || "").toLowerCase();
    return (PLAYER_REMOTE_CACHE_ALLOWED_HOSTS.has(normalizedHostname) || isLocalHost || isPrivateLanHost)
      ? PLAYER_REMOTE_CACHE_PUBLIC_API_BASE_URL
      : "";
  }

  function createPlayerDecodedEcdcOpusCacheSpec({
    sourceHash = "",
    bundleName = "",
    recordProfile = "",
    sampleRate = 48000,
    channels = 2,
    bitrate = OPUS_CACHE_BITRATE,
    frameSize = 960,
    frameDurationMs = OPUS_CACHE_FRAME_DURATION_MS,
    audioFormat = OPUS_PACKET_AUDIO_FORMAT,
  } = {}) {
    const normalizedRecordProfile = String(recordProfile ?? "").trim();
    if (!normalizedRecordProfile) {
      throw new Error("Decoded ECDC cache spec requires recordProfile.");
    }
    return {
      keyVersion: PLAYER_DECODED_SEGMENT_CACHE_KEY_VERSION,
      sourceHash: String(sourceHash || ""),
      bundleName: String(bundleName || ""),
      recordProfile: normalizedRecordProfile,
      audioFormat: String(audioFormat || OPUS_PACKET_AUDIO_FORMAT),
      codec: "soundkit-wasm",
      codecMode: "cbr",
      sampleRate: Math.max(1, Math.floor(Number(sampleRate) || 48000)),
      channels: Math.max(1, Math.floor(Number(channels) || 2)),
      bitrate: Math.max(1, Math.floor(Number(bitrate) || OPUS_CACHE_BITRATE)),
      frameSize: Math.max(1, Math.floor(Number(frameSize) || 960)),
      frameDurationMs: Math.max(1, Math.floor(Number(frameDurationMs) || OPUS_CACHE_FRAME_DURATION_MS)),
    };
  }

  function createPlayerDecodedEcdcOpusSegmentCacheSpec({
    sourceKey = "",
    bitrate = OPUS_CACHE_BITRATE,
    outputCodec = OPUS_PACKET_AUDIO_FORMAT,
  } = {}) {
    return {
      keyVersion: PLAYER_DECODED_ECDC_SEGMENT_CACHE_KEY_VERSION,
      sourceKey: String(sourceKey || ""),
      outputCodec: String(outputCodec || OPUS_PACKET_AUDIO_FORMAT),
      bitrate: Math.max(1, Math.floor(Number(bitrate) || OPUS_CACHE_BITRATE)),
    };
  }

  globalThis.BitneedlePlayerCacheConfig = Object.freeze({
    MOBILE_LP_PARSE_CACHE_CUTOFF_SECONDS,
    OPUS_CACHE_BITRATE,
    OPUS_CACHE_FRAME_DURATION_MS,
    OPUS_CACHE_FRAME_RATE,
    OPUS_PACKET_AUDIO_FORMAT,
    PLAYER_DECODE_SEGMENT_CACHE_MAX_ENTRIES,
    PLAYER_DECODE_SEGMENT_CACHE_STORE_NAME,
    PLAYER_DECODE_SEGMENT_REMOTE_CACHE_FORMATS,
    PLAYER_DISABLE_ALL_CACHING,
    PLAYER_DECODED_ECDC_SEGMENT_CACHE_KEY_VERSION,
    PLAYER_DECODED_ECDC_SEGMENT_CACHE_WRITE_BATCH_SIZE,
    PLAYER_DECODED_SEGMENT_CACHE_KEY_VERSION,
    PLAYER_REMOTE_CACHE_BATCH_FORMAT,
    PLAYER_REMOTE_CACHE_BATCH_RETRY_DELAYS_MS,
    PLAYER_REMOTE_CACHE_BATCH_SIZE,
    PLAYER_REMOTE_CACHE_BATCH_WRITE_SIZE,
    PLAYER_REMOTE_CACHE_STREAM_CONTENT_TYPE,
    PLAYER_WAVEFORM_BUCKET_COUNT,
    PLAYER_WAVEFORM_BUCKET_COUNT_MOBILE,
    PLAYER_WAVEFORM_CACHE_MAX_ENTRIES,
    PLAYER_WAVEFORM_CACHE_STORE_NAME,
    createPlayerDecodedEcdcOpusCacheSpec,
    createPlayerDecodedEcdcOpusSegmentCacheSpec,
    resolveDefaultPlayerRemoteCacheApiBaseUrl,
  });
})();
