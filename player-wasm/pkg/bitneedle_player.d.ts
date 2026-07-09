/* tslint:disable */
/* eslint-disable */

export class QuantizedLmChunkDecoder {
    free(): void;
    [Symbol.dispose](): void;
    bitstream_version(): number;
    lmWindowFrameLength(): number;
    constructor(bundle_json: string, weights: Uint8Array, payload: Uint8Array);
    pull(): Uint16Array;
    scale(): number;
}

/**
 * One encoded revolution at the BRS1 boundary: a shared codec/container
 * description plus only the bytes that vary per revolution (the headerless
 * codec payload body). Mirrors the `EncodedPayload` shape from the WASM
 * headerless-ECDC contract.
 */
export class WasmEncodedPayload {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    readonly descriptor: any;
    readonly payload: Uint8Array;
}

export function bcs2OpusChunkCacheKeysJson(bcs2: Uint8Array): string;

export function buildRightsLicenceCallDataJson(options_json: string): string;

export function bumpScratchRemoteControlRevision(value: any): number;

export function createPlayerEcdcCacheProofContextJson(ecdc: Uint8Array): string;

export function decodeEvmBool(result_hex: string): boolean;

export function decodeEvmUint256String(result_hex: string): string;

export function decodeRightsLicenceSaleJson(result_hex: string): string;

export function ecdcChunkLayoutFromMetadata(bundle_json: string, payload: Uint8Array, chunk_count: number): any;

/**
 * Crop a guarded decode down to its owned audio: drops the left/right guard
 * samples the encoder padded each revolution with, leaving exactly the owned
 * (audible) samples. Mirrors `encodec_rs::wasm::ecdc_crop_decoded_owned_audio`
 * so the player worker can call it on the bitneedle-player module.
 */
export function ecdcCropDecodedOwnedAudio(bundle_json: string, decoded_audio: Float32Array): Float32Array;

export function ecdcFrameRanges(payload: Uint8Array): any;

export function ecdcMetadata(payload: Uint8Array): any;

/**
 * Split a standalone ECDC byte stream (one fixed-profile revolution block) into
 * the shared `PayloadDescriptor` and the headerless codec payload bytes stored
 * in the BRS1 groove. The ECDC outer header is lifted into the descriptor.
 */
export function ecdcStandaloneToPayload(sample_rate: number, channels: number, ecdc_bytes: Uint8Array): WasmEncodedPayload;

export function encodeEvmQuantity(value: string): string;

export function ensureScratchRemoteControlRevisionJson(state_json: string): string;

export function getRecordRpm(record_profile: string): number;

export function initPanicHook(): void;

export function isValidScratchAnonUserId(value: string): boolean;

export function isValidScratchWalletAddress(value: string): boolean;

export function lmEcdcDecodeChunks(bundle_json: string, payload: Uint8Array): any;

export function normalizeRecordProfileName(record_profile: string): string;

export function normalizeRecordTextFieldText(value: string): string;

export function normalizeScratchDisplayName(value: string): string;

export function normalizeScratchRemoteControlRevision(value: any): number;

export function normalizeScratchSampleId(value: any): number;

export function parseEthToWeiString(value: string): string;

/**
 * Reconstruct a decodable standalone ECDC byte stream from a `PayloadDescriptor`
 * (JSON) and the headerless codec payload bytes. Inverse of
 * [`wasm_ecdc_standalone_to_payload`]; lets the player feed the existing decode
 * path without storing repeated ECDC headers in the groove.
 */
export function payloadToStandaloneEcdc(descriptor_json: string, payload: Uint8Array): Uint8Array;

export function playerAppBuildInfoJson(): string;

export function playerEcdcCacheProofForChunkJson(context_json: string, chunk_index: number): string;

export function recordDisplayMetadataJson(record_json: string, fallback_record_profile: string): string;

export function recordPlaybackMetadataFromHeaderJson(header_json: string): string;

export function recordProfileFromHeaderValidationJson(header_json: string): string;

export function recordProfileSpecJson(record_profile: string): string;

export function recordTextFromHeaderValidationJson(header_json: string): string;

export function recordVerificationMetaJson(verification_json: string): string;

export function resolveClipRevolutionsJson(start_time: number, end_time: number, duration_seconds: number, revolution_count: number): string;

export function resolveDeadwaxDurationSeconds(record_profile: string, rpm_candidate: any, playback_rate: any, default_rate: number, min_rate: number, max_rate: number): number;

export function resolveDeadwaxTurns(record_profile: string): number;

export function resolveLeadInDurationSeconds(record_profile: string, rpm_candidate: any, playback_rate: any, default_rate: number, min_rate: number, max_rate: number): number;

export function resolveLeadInTurns(record_profile: string): number;

export function resolvePhysicalRpm(rpm_candidate: any, record_profile: string, playback_rate: any, default_rate: number, min_rate: number, max_rate: number): number;

export function resolvePlaybackPayloadMetadataJson(header_metadata_json: string, payload_metadata_json: string, length_prefixed_entries: boolean): string;

export function resolvePlaybackRate(value: any, default_rate: number, min_rate: number, max_rate: number): number;

export function resolveRecordRightsContextJson(meta_json: string, fallback_json: string, total_revolutions: number): string;

export function resolveRecordRpm(rpm_candidate: any, record_profile: string): number;

export function resolveSecondsPerTurn(rpm_candidate: any, record_profile: string, playback_rate: any, default_rate: number, min_rate: number, max_rate: number): number;

export function scratchAnonUserIdFromRandom(random_part: string): string;

export function scratchClipIdForSampleId(value: any): string;

export function scratchClipSampleIdJson(clip_json: string): number;

export function scratchDisplayNameKey(name: string): string;

export function scratchSampleTokenFromBytes(random_bytes: Uint8Array, fallback: any): number;

export function scratchSampleTokenHex(value: any): string;

export function scratchVisitorWalletAddressFromBytes(random_bytes: Uint8Array): string;

export function secondsToRevolutions(seconds: any, record_profile: string, rpm_candidate: any): number;

export function shortScratchAddress(address: string, chars: any): string;

export function shouldApplyRemoteScratchControlsJson(state_json: string, command_json: string): string;

export function stableHashHex(bytes: Uint8Array): string;

export function stableLocalRecordIdFromMetaJson(meta_json: string, file_name: string): string;

/**
 * Verify that two `PayloadDescriptor`s (JSON) are identical across every field,
 * so a record builder can confirm all revolutions share one descriptor before
 * storing it once. Errors name the differing field.
 */
export function validateSharedPayloadDescriptor(expected_json: string, actual_json: string): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_quantizedlmchunkdecoder_free: (a: number, b: number) => void;
    readonly __wbg_wasmencodedpayload_free: (a: number, b: number) => void;
    readonly bcs2OpusChunkCacheKeysJson: (a: number, b: number) => [number, number, number, number];
    readonly buildRightsLicenceCallDataJson: (a: number, b: number) => [number, number, number, number];
    readonly bumpScratchRemoteControlRevision: (a: any) => number;
    readonly createPlayerEcdcCacheProofContextJson: (a: number, b: number) => [number, number];
    readonly decodeEvmBool: (a: number, b: number) => [number, number, number];
    readonly decodeEvmUint256String: (a: number, b: number) => [number, number, number, number];
    readonly decodeRightsLicenceSaleJson: (a: number, b: number) => [number, number, number, number];
    readonly ecdcChunkLayoutFromMetadata: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
    readonly ecdcCropDecodedOwnedAudio: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly ecdcFrameRanges: (a: number, b: number) => [number, number, number];
    readonly ecdcMetadata: (a: number, b: number) => [number, number, number];
    readonly ecdcStandaloneToPayload: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly encodeEvmQuantity: (a: number, b: number) => [number, number, number, number];
    readonly ensureScratchRemoteControlRevisionJson: (a: number, b: number) => [number, number];
    readonly getRecordRpm: (a: number, b: number) => [number, number, number];
    readonly isValidScratchAnonUserId: (a: number, b: number) => number;
    readonly isValidScratchWalletAddress: (a: number, b: number) => number;
    readonly lmEcdcDecodeChunks: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly normalizeRecordProfileName: (a: number, b: number) => [number, number, number, number];
    readonly normalizeRecordTextFieldText: (a: number, b: number) => [number, number];
    readonly normalizeScratchDisplayName: (a: number, b: number) => [number, number];
    readonly normalizeScratchRemoteControlRevision: (a: any) => number;
    readonly normalizeScratchSampleId: (a: any) => number;
    readonly parseEthToWeiString: (a: number, b: number) => [number, number, number, number];
    readonly payloadToStandaloneEcdc: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly playerAppBuildInfoJson: () => [number, number];
    readonly playerEcdcCacheProofForChunkJson: (a: number, b: number, c: number) => [number, number];
    readonly quantizedlmchunkdecoder_bitstream_version: (a: number) => number;
    readonly quantizedlmchunkdecoder_lmWindowFrameLength: (a: number) => number;
    readonly quantizedlmchunkdecoder_new: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number];
    readonly quantizedlmchunkdecoder_pull: (a: number) => [number, number, number, number];
    readonly quantizedlmchunkdecoder_scale: (a: number) => number;
    readonly recordDisplayMetadataJson: (a: number, b: number, c: number, d: number) => [number, number];
    readonly recordPlaybackMetadataFromHeaderJson: (a: number, b: number) => [number, number];
    readonly recordProfileFromHeaderValidationJson: (a: number, b: number) => [number, number];
    readonly recordProfileSpecJson: (a: number, b: number) => [number, number, number, number];
    readonly recordTextFromHeaderValidationJson: (a: number, b: number) => [number, number];
    readonly recordVerificationMetaJson: (a: number, b: number) => [number, number];
    readonly resolveClipRevolutionsJson: (a: number, b: number, c: number, d: number) => [number, number];
    readonly resolveDeadwaxDurationSeconds: (a: number, b: number, c: any, d: any, e: number, f: number, g: number) => [number, number, number];
    readonly resolveDeadwaxTurns: (a: number, b: number) => [number, number, number];
    readonly resolveLeadInDurationSeconds: (a: number, b: number, c: any, d: any, e: number, f: number, g: number) => [number, number, number];
    readonly resolveLeadInTurns: (a: number, b: number) => [number, number, number];
    readonly resolvePhysicalRpm: (a: any, b: number, c: number, d: any, e: number, f: number, g: number) => [number, number, number];
    readonly resolvePlaybackPayloadMetadataJson: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly resolvePlaybackRate: (a: any, b: number, c: number, d: number) => number;
    readonly resolveRecordRightsContextJson: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly resolveRecordRpm: (a: any, b: number, c: number) => [number, number, number];
    readonly resolveSecondsPerTurn: (a: any, b: number, c: number, d: any, e: number, f: number, g: number) => [number, number, number];
    readonly scratchAnonUserIdFromRandom: (a: number, b: number) => [number, number];
    readonly scratchClipIdForSampleId: (a: any) => [number, number];
    readonly scratchClipSampleIdJson: (a: number, b: number) => number;
    readonly scratchDisplayNameKey: (a: number, b: number) => [number, number];
    readonly scratchSampleTokenFromBytes: (a: number, b: number, c: any) => number;
    readonly scratchSampleTokenHex: (a: any) => [number, number];
    readonly scratchVisitorWalletAddressFromBytes: (a: number, b: number) => [number, number];
    readonly secondsToRevolutions: (a: any, b: number, c: number, d: any) => [number, number, number];
    readonly shortScratchAddress: (a: number, b: number, c: any) => [number, number];
    readonly shouldApplyRemoteScratchControlsJson: (a: number, b: number, c: number, d: number) => [number, number];
    readonly stableHashHex: (a: number, b: number) => [number, number];
    readonly stableLocalRecordIdFromMetaJson: (a: number, b: number, c: number, d: number) => [number, number];
    readonly validateSharedPayloadDescriptor: (a: number, b: number, c: number, d: number) => [number, number];
    readonly wasmencodedpayload_descriptor: (a: number) => any;
    readonly wasmencodedpayload_payload: (a: number) => [number, number];
    readonly initPanicHook: () => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
