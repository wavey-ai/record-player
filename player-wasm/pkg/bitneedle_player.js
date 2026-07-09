/* @ts-self-types="./bitneedle_player.d.ts" */

export class QuantizedLmChunkDecoder {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        QuantizedLmChunkDecoderFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_quantizedlmchunkdecoder_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    bitstream_version() {
        const ret = wasm.quantizedlmchunkdecoder_bitstream_version(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    lmWindowFrameLength() {
        const ret = wasm.quantizedlmchunkdecoder_lmWindowFrameLength(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {string} bundle_json
     * @param {Uint8Array} weights
     * @param {Uint8Array} payload
     */
    constructor(bundle_json, weights, payload) {
        const ptr0 = passStringToWasm0(bundle_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passArray8ToWasm0(weights, wasm.__wbindgen_malloc);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passArray8ToWasm0(payload, wasm.__wbindgen_malloc);
        const len2 = WASM_VECTOR_LEN;
        const ret = wasm.quantizedlmchunkdecoder_new(ptr0, len0, ptr1, len1, ptr2, len2);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        QuantizedLmChunkDecoderFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {Uint16Array}
     */
    pull() {
        const ret = wasm.quantizedlmchunkdecoder_pull(this.__wbg_ptr);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * @returns {number}
     */
    scale() {
        const ret = wasm.quantizedlmchunkdecoder_scale(this.__wbg_ptr);
        return ret;
    }
}
if (Symbol.dispose) QuantizedLmChunkDecoder.prototype[Symbol.dispose] = QuantizedLmChunkDecoder.prototype.free;

/**
 * One encoded revolution at the BRS1 boundary: a shared codec/container
 * description plus only the bytes that vary per revolution (the headerless
 * codec payload body). Mirrors the `EncodedPayload` shape from the WASM
 * headerless-ECDC contract.
 */
export class WasmEncodedPayload {
    static __wrap(ptr) {
        const obj = Object.create(WasmEncodedPayload.prototype);
        obj.__wbg_ptr = ptr;
        WasmEncodedPayloadFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WasmEncodedPayloadFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_wasmencodedpayload_free(ptr, 0);
    }
    /**
     * @returns {any}
     */
    get descriptor() {
        const ret = wasm.wasmencodedpayload_descriptor(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {Uint8Array}
     */
    get payload() {
        const ret = wasm.wasmencodedpayload_payload(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
}
if (Symbol.dispose) WasmEncodedPayload.prototype[Symbol.dispose] = WasmEncodedPayload.prototype.free;

/**
 * @param {Uint8Array} bcs2
 * @returns {string}
 */
export function bcs2OpusChunkCacheKeysJson(bcs2) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passArray8ToWasm0(bcs2, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bcs2OpusChunkCacheKeysJson(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * @param {string} options_json
 * @returns {string}
 */
export function buildRightsLicenceCallDataJson(options_json) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(options_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.buildRightsLicenceCallDataJson(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * @param {any} value
 * @returns {number}
 */
export function bumpScratchRemoteControlRevision(value) {
    const ret = wasm.bumpScratchRemoteControlRevision(value);
    return ret >>> 0;
}

/**
 * @param {Uint8Array} ecdc
 * @returns {string}
 */
export function createPlayerEcdcCacheProofContextJson(ecdc) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passArray8ToWasm0(ecdc, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.createPlayerEcdcCacheProofContextJson(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {string} result_hex
 * @returns {boolean}
 */
export function decodeEvmBool(result_hex) {
    const ptr0 = passStringToWasm0(result_hex, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decodeEvmBool(ptr0, len0);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0] !== 0;
}

/**
 * @param {string} result_hex
 * @returns {string}
 */
export function decodeEvmUint256String(result_hex) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(result_hex, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.decodeEvmUint256String(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * @param {string} result_hex
 * @returns {string}
 */
export function decodeRightsLicenceSaleJson(result_hex) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(result_hex, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.decodeRightsLicenceSaleJson(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * @param {string} bundle_json
 * @param {Uint8Array} payload
 * @param {number} chunk_count
 * @returns {any}
 */
export function ecdcChunkLayoutFromMetadata(bundle_json, payload, chunk_count) {
    const ptr0 = passStringToWasm0(bundle_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passArray8ToWasm0(payload, wasm.__wbindgen_malloc);
    const len1 = WASM_VECTOR_LEN;
    const ret = wasm.ecdcChunkLayoutFromMetadata(ptr0, len0, ptr1, len1, chunk_count);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * Crop a guarded decode down to its owned audio: drops the left/right guard
 * samples the encoder padded each revolution with, leaving exactly the owned
 * (audible) samples. Mirrors `encodec_rs::wasm::ecdc_crop_decoded_owned_audio`
 * so the player worker can call it on the bitneedle-player module.
 * @param {string} bundle_json
 * @param {Float32Array} decoded_audio
 * @returns {Float32Array}
 */
export function ecdcCropDecodedOwnedAudio(bundle_json, decoded_audio) {
    const ptr0 = passStringToWasm0(bundle_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passArrayF32ToWasm0(decoded_audio, wasm.__wbindgen_malloc);
    const len1 = WASM_VECTOR_LEN;
    const ret = wasm.ecdcCropDecodedOwnedAudio(ptr0, len0, ptr1, len1);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    var v3 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v3;
}

/**
 * @param {Uint8Array} payload
 * @returns {any}
 */
export function ecdcFrameRanges(payload) {
    const ptr0 = passArray8ToWasm0(payload, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.ecdcFrameRanges(ptr0, len0);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * @param {Uint8Array} payload
 * @returns {any}
 */
export function ecdcMetadata(payload) {
    const ptr0 = passArray8ToWasm0(payload, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.ecdcMetadata(ptr0, len0);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * Split a standalone ECDC byte stream (one fixed-profile revolution block) into
 * the shared `PayloadDescriptor` and the headerless codec payload bytes stored
 * in the BRS1 groove. The ECDC outer header is lifted into the descriptor.
 * @param {number} sample_rate
 * @param {number} channels
 * @param {Uint8Array} ecdc_bytes
 * @returns {WasmEncodedPayload}
 */
export function ecdcStandaloneToPayload(sample_rate, channels, ecdc_bytes) {
    const ptr0 = passArray8ToWasm0(ecdc_bytes, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.ecdcStandaloneToPayload(sample_rate, channels, ptr0, len0);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return WasmEncodedPayload.__wrap(ret[0]);
}

/**
 * @param {string} value
 * @returns {string}
 */
export function encodeEvmQuantity(value) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(value, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.encodeEvmQuantity(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * @param {string} state_json
 * @returns {string}
 */
export function ensureScratchRemoteControlRevisionJson(state_json) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(state_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.ensureScratchRemoteControlRevisionJson(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {string} record_profile
 * @returns {number}
 */
export function getRecordRpm(record_profile) {
    const ptr0 = passStringToWasm0(record_profile, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.getRecordRpm(ptr0, len0);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0];
}

export function initPanicHook() {
    wasm.initPanicHook();
}

/**
 * @param {string} value
 * @returns {boolean}
 */
export function isValidScratchAnonUserId(value) {
    const ptr0 = passStringToWasm0(value, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.isValidScratchAnonUserId(ptr0, len0);
    return ret !== 0;
}

/**
 * @param {string} value
 * @returns {boolean}
 */
export function isValidScratchWalletAddress(value) {
    const ptr0 = passStringToWasm0(value, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.isValidScratchWalletAddress(ptr0, len0);
    return ret !== 0;
}

/**
 * @param {string} bundle_json
 * @param {Uint8Array} payload
 * @returns {any}
 */
export function lmEcdcDecodeChunks(bundle_json, payload) {
    const ptr0 = passStringToWasm0(bundle_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passArray8ToWasm0(payload, wasm.__wbindgen_malloc);
    const len1 = WASM_VECTOR_LEN;
    const ret = wasm.lmEcdcDecodeChunks(ptr0, len0, ptr1, len1);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * @param {string} record_profile
 * @returns {string}
 */
export function normalizeRecordProfileName(record_profile) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(record_profile, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.normalizeRecordProfileName(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * @param {string} value
 * @returns {string}
 */
export function normalizeRecordTextFieldText(value) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(value, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.normalizeRecordTextFieldText(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {string} value
 * @returns {string}
 */
export function normalizeScratchDisplayName(value) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(value, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.normalizeScratchDisplayName(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {any} value
 * @returns {number}
 */
export function normalizeScratchRemoteControlRevision(value) {
    const ret = wasm.normalizeScratchRemoteControlRevision(value);
    return ret >>> 0;
}

/**
 * @param {any} value
 * @returns {number}
 */
export function normalizeScratchSampleId(value) {
    const ret = wasm.normalizeScratchSampleId(value);
    return ret;
}

/**
 * @param {string} value
 * @returns {string}
 */
export function parseEthToWeiString(value) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(value, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.parseEthToWeiString(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * Reconstruct a decodable standalone ECDC byte stream from a `PayloadDescriptor`
 * (JSON) and the headerless codec payload bytes. Inverse of
 * [`wasm_ecdc_standalone_to_payload`]; lets the player feed the existing decode
 * path without storing repeated ECDC headers in the groove.
 * @param {string} descriptor_json
 * @param {Uint8Array} payload
 * @returns {Uint8Array}
 */
export function payloadToStandaloneEcdc(descriptor_json, payload) {
    const ptr0 = passStringToWasm0(descriptor_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passArray8ToWasm0(payload, wasm.__wbindgen_malloc);
    const len1 = WASM_VECTOR_LEN;
    const ret = wasm.payloadToStandaloneEcdc(ptr0, len0, ptr1, len1);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    var v3 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    return v3;
}

/**
 * @returns {string}
 */
export function playerAppBuildInfoJson() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.playerAppBuildInfoJson();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * @param {string} context_json
 * @param {number} chunk_index
 * @returns {string}
 */
export function playerEcdcCacheProofForChunkJson(context_json, chunk_index) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(context_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.playerEcdcCacheProofForChunkJson(ptr0, len0, chunk_index);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {string} record_json
 * @param {string} fallback_record_profile
 * @returns {string}
 */
export function recordDisplayMetadataJson(record_json, fallback_record_profile) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(record_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(fallback_record_profile, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.recordDisplayMetadataJson(ptr0, len0, ptr1, len1);
        deferred3_0 = ret[0];
        deferred3_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * @param {string} header_json
 * @returns {string}
 */
export function recordPlaybackMetadataFromHeaderJson(header_json) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(header_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.recordPlaybackMetadataFromHeaderJson(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {string} header_json
 * @returns {string}
 */
export function recordProfileFromHeaderValidationJson(header_json) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(header_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.recordProfileFromHeaderValidationJson(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {string} record_profile
 * @returns {string}
 */
export function recordProfileSpecJson(record_profile) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(record_profile, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.recordProfileSpecJson(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * @param {string} header_json
 * @returns {string}
 */
export function recordTextFromHeaderValidationJson(header_json) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(header_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.recordTextFromHeaderValidationJson(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {string} verification_json
 * @returns {string}
 */
export function recordVerificationMetaJson(verification_json) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(verification_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.recordVerificationMetaJson(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {number} start_time
 * @param {number} end_time
 * @param {number} duration_seconds
 * @param {number} revolution_count
 * @returns {string}
 */
export function resolveClipRevolutionsJson(start_time, end_time, duration_seconds, revolution_count) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.resolveClipRevolutionsJson(start_time, end_time, duration_seconds, revolution_count);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * @param {string} record_profile
 * @param {any} rpm_candidate
 * @param {any} playback_rate
 * @param {number} default_rate
 * @param {number} min_rate
 * @param {number} max_rate
 * @returns {number}
 */
export function resolveDeadwaxDurationSeconds(record_profile, rpm_candidate, playback_rate, default_rate, min_rate, max_rate) {
    const ptr0 = passStringToWasm0(record_profile, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.resolveDeadwaxDurationSeconds(ptr0, len0, rpm_candidate, playback_rate, default_rate, min_rate, max_rate);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0];
}

/**
 * @param {string} record_profile
 * @returns {number}
 */
export function resolveDeadwaxTurns(record_profile) {
    const ptr0 = passStringToWasm0(record_profile, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.resolveDeadwaxTurns(ptr0, len0);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0];
}

/**
 * @param {string} record_profile
 * @param {any} rpm_candidate
 * @param {any} playback_rate
 * @param {number} default_rate
 * @param {number} min_rate
 * @param {number} max_rate
 * @returns {number}
 */
export function resolveLeadInDurationSeconds(record_profile, rpm_candidate, playback_rate, default_rate, min_rate, max_rate) {
    const ptr0 = passStringToWasm0(record_profile, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.resolveLeadInDurationSeconds(ptr0, len0, rpm_candidate, playback_rate, default_rate, min_rate, max_rate);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0];
}

/**
 * @param {string} record_profile
 * @returns {number}
 */
export function resolveLeadInTurns(record_profile) {
    const ptr0 = passStringToWasm0(record_profile, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.resolveLeadInTurns(ptr0, len0);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0];
}

/**
 * @param {any} rpm_candidate
 * @param {string} record_profile
 * @param {any} playback_rate
 * @param {number} default_rate
 * @param {number} min_rate
 * @param {number} max_rate
 * @returns {number}
 */
export function resolvePhysicalRpm(rpm_candidate, record_profile, playback_rate, default_rate, min_rate, max_rate) {
    const ptr0 = passStringToWasm0(record_profile, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.resolvePhysicalRpm(rpm_candidate, ptr0, len0, playback_rate, default_rate, min_rate, max_rate);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0];
}

/**
 * @param {string} header_metadata_json
 * @param {string} payload_metadata_json
 * @param {boolean} length_prefixed_entries
 * @returns {string}
 */
export function resolvePlaybackPayloadMetadataJson(header_metadata_json, payload_metadata_json, length_prefixed_entries) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(header_metadata_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(payload_metadata_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.resolvePlaybackPayloadMetadataJson(ptr0, len0, ptr1, len1, length_prefixed_entries);
        deferred3_0 = ret[0];
        deferred3_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * @param {any} value
 * @param {number} default_rate
 * @param {number} min_rate
 * @param {number} max_rate
 * @returns {number}
 */
export function resolvePlaybackRate(value, default_rate, min_rate, max_rate) {
    const ret = wasm.resolvePlaybackRate(value, default_rate, min_rate, max_rate);
    return ret;
}

/**
 * @param {string} meta_json
 * @param {string} fallback_json
 * @param {number} total_revolutions
 * @returns {string}
 */
export function resolveRecordRightsContextJson(meta_json, fallback_json, total_revolutions) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(meta_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(fallback_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.resolveRecordRightsContextJson(ptr0, len0, ptr1, len1, total_revolutions);
        deferred3_0 = ret[0];
        deferred3_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * @param {any} rpm_candidate
 * @param {string} record_profile
 * @returns {number}
 */
export function resolveRecordRpm(rpm_candidate, record_profile) {
    const ptr0 = passStringToWasm0(record_profile, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.resolveRecordRpm(rpm_candidate, ptr0, len0);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0];
}

/**
 * @param {any} rpm_candidate
 * @param {string} record_profile
 * @param {any} playback_rate
 * @param {number} default_rate
 * @param {number} min_rate
 * @param {number} max_rate
 * @returns {number}
 */
export function resolveSecondsPerTurn(rpm_candidate, record_profile, playback_rate, default_rate, min_rate, max_rate) {
    const ptr0 = passStringToWasm0(record_profile, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.resolveSecondsPerTurn(rpm_candidate, ptr0, len0, playback_rate, default_rate, min_rate, max_rate);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0];
}

/**
 * @param {string} random_part
 * @returns {string}
 */
export function scratchAnonUserIdFromRandom(random_part) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(random_part, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.scratchAnonUserIdFromRandom(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {any} value
 * @returns {string}
 */
export function scratchClipIdForSampleId(value) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.scratchClipIdForSampleId(value);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * @param {string} clip_json
 * @returns {number}
 */
export function scratchClipSampleIdJson(clip_json) {
    const ptr0 = passStringToWasm0(clip_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.scratchClipSampleIdJson(ptr0, len0);
    return ret;
}

/**
 * @param {string} name
 * @returns {string}
 */
export function scratchDisplayNameKey(name) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.scratchDisplayNameKey(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {Uint8Array} random_bytes
 * @param {any} fallback
 * @returns {number}
 */
export function scratchSampleTokenFromBytes(random_bytes, fallback) {
    const ptr0 = passArray8ToWasm0(random_bytes, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.scratchSampleTokenFromBytes(ptr0, len0, fallback);
    return ret;
}

/**
 * @param {any} value
 * @returns {string}
 */
export function scratchSampleTokenHex(value) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.scratchSampleTokenHex(value);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * @param {Uint8Array} random_bytes
 * @returns {string}
 */
export function scratchVisitorWalletAddressFromBytes(random_bytes) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passArray8ToWasm0(random_bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.scratchVisitorWalletAddressFromBytes(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {any} seconds
 * @param {string} record_profile
 * @param {any} rpm_candidate
 * @returns {number}
 */
export function secondsToRevolutions(seconds, record_profile, rpm_candidate) {
    const ptr0 = passStringToWasm0(record_profile, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.secondsToRevolutions(seconds, ptr0, len0, rpm_candidate);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0];
}

/**
 * @param {string} address
 * @param {any} chars
 * @returns {string}
 */
export function shortScratchAddress(address, chars) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(address, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.shortScratchAddress(ptr0, len0, chars);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {string} state_json
 * @param {string} command_json
 * @returns {string}
 */
export function shouldApplyRemoteScratchControlsJson(state_json, command_json) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(state_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(command_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.shouldApplyRemoteScratchControlsJson(ptr0, len0, ptr1, len1);
        deferred3_0 = ret[0];
        deferred3_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * @param {Uint8Array} bytes
 * @returns {string}
 */
export function stableHashHex(bytes) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.stableHashHex(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * @param {string} meta_json
 * @param {string} file_name
 * @returns {string}
 */
export function stableLocalRecordIdFromMetaJson(meta_json, file_name) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(meta_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(file_name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.stableLocalRecordIdFromMetaJson(ptr0, len0, ptr1, len1);
        deferred3_0 = ret[0];
        deferred3_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * Verify that two `PayloadDescriptor`s (JSON) are identical across every field,
 * so a record builder can confirm all revolutions share one descriptor before
 * storing it once. Errors name the differing field.
 * @param {string} expected_json
 * @param {string} actual_json
 */
export function validateSharedPayloadDescriptor(expected_json, actual_json) {
    const ptr0 = passStringToWasm0(expected_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(actual_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ret = wasm.validateSharedPayloadDescriptor(ptr0, len0, ptr1, len1);
    if (ret[1]) {
        throw takeFromExternrefTable0(ret[0]);
    }
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg_Error_fdd633d4bb5dd76a: function(arg0, arg1) {
            const ret = Error(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_String_8564e559799eccda: function(arg0, arg1) {
            const ret = String(arg1);
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_is_string_1fca8072260dd261: function(arg0) {
            const ret = typeof(arg0) === 'string';
            return ret;
        },
        __wbg___wbindgen_number_get_1cc01dd708740256: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64(arg0 + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_71bb4348194e31f0: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_ea4887a5f8f9a9db: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_error_a6fa202b58aa1cd3: function(arg0, arg1) {
            let deferred0_0;
            let deferred0_1;
            try {
                deferred0_0 = arg0;
                deferred0_1 = arg1;
                console.error(getStringFromWasm0(arg0, arg1));
            } finally {
                wasm.__wbindgen_free(deferred0_0, deferred0_1, 1);
            }
        },
        __wbg_new_227d7c05414eb861: function() {
            const ret = new Error();
            return ret;
        },
        __wbg_new_2e117a478906f062: function() {
            const ret = new Object();
            return ret;
        },
        __wbg_new_3444eb7412549f0b: function() {
            const ret = new Map();
            return ret;
        },
        __wbg_new_36e147a8ced3c6e0: function() {
            const ret = new Array();
            return ret;
        },
        __wbg_set_6be42768c690e380: function(arg0, arg1, arg2) {
            arg0[arg1] = arg2;
        },
        __wbg_set_9a1d61e17de7054c: function(arg0, arg1, arg2) {
            const ret = arg0.set(arg1, arg2);
            return ret;
        },
        __wbg_set_dc601f4a69da0bc2: function(arg0, arg1, arg2) {
            arg0[arg1 >>> 0] = arg2;
        },
        __wbg_stack_3b0d974bbf31e44f: function(arg0, arg1) {
            const ret = arg1.stack;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbindgen_cast_0000000000000001: function(arg0) {
            // Cast intrinsic for `F64 -> Externref`.
            const ret = arg0;
            return ret;
        },
        __wbindgen_cast_0000000000000002: function(arg0) {
            // Cast intrinsic for `I64 -> Externref`.
            const ret = arg0;
            return ret;
        },
        __wbindgen_cast_0000000000000003: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_cast_0000000000000004: function(arg0) {
            // Cast intrinsic for `U64 -> Externref`.
            const ret = BigInt.asUintN(64, arg0);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./bitneedle_player_bg.js": import0,
    };
}

const QuantizedLmChunkDecoderFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_quantizedlmchunkdecoder_free(ptr, 1));
const WasmEncodedPayloadFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_wasmencodedpayload_free(ptr, 1));

function getArrayF32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayU16FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint16ArrayMemory0().subarray(ptr / 2, ptr / 2 + len);
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

let cachedFloat32ArrayMemory0 = null;
function getFloat32ArrayMemory0() {
    if (cachedFloat32ArrayMemory0 === null || cachedFloat32ArrayMemory0.byteLength === 0) {
        cachedFloat32ArrayMemory0 = new Float32Array(wasm.memory.buffer);
    }
    return cachedFloat32ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint16ArrayMemory0 = null;
function getUint16ArrayMemory0() {
    if (cachedUint16ArrayMemory0 === null || cachedUint16ArrayMemory0.byteLength === 0) {
        cachedUint16ArrayMemory0 = new Uint16Array(wasm.memory.buffer);
    }
    return cachedUint16ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArrayF32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getFloat32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedFloat32ArrayMemory0 = null;
    cachedUint16ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('bitneedle_player_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
