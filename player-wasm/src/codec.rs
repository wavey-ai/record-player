use base64::{engine::general_purpose, Engine as _};
use encodec_rs::arithmetic::ArithmeticDecoder;
use encodec_rs::binary::{read_chunk_payload, read_ecdc_header, read_exactly};
use encodec_rs::format::{
    ecdc_chunk_layout_from_metadata, ecdc_frame_ranges, ecdc_lm_frame_length, validate_metadata,
    EcdcChunkLayout, EcdcMetadata, ARITHMETIC_TOTAL_RANGE_BITS, DEFAULT_FP_SCALE,
    DEFAULT_MIN_RANGE, QUANTIZED_LM_BITSTREAM_VERSION,
};
use encodec_rs::metadata::OnnxFrameBundleMetadata;
use encodec_rs::quantized_lm::{QuantizedLm, QuantizedLmState, QuantizedLmWeights};
use encodec_rs::stable_hash::stable_hash_hex;
use libopus_rs::{Application as OpusApplication, Decoder as RustOpusDecoder, Encoder as RustOpusEncoder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Cursor;
use wasm_bindgen::prelude::*;

const OPUS_CHUNK_CACHE_KEY_FORMAT: &str = "bitneedle-opus-chunk-cache-keys-v1";
const OPUS_CHUNK_CACHE_STORE_NAME: &str = "opus-chunks";
const OPUS_CHUNK_CACHE_VERSION: &str = "bitneedle-opus-chunk-cache-v2";
const OPUS_CHUNK_CACHE_OUTPUT_CODEC: &str = "soundkit_opus_packets";
const OPUS_CHUNK_CACHE_BITRATE: u32 = 64_000;
const OPUS_CHUNK_CACHE_KEY_DOMAIN: &str = "bitneedle.opus-chunk-cache-key.v1";
// Visible R2/cache-key prefix for this object type. The only output codec
// this player currently ships; a future codec (e.g. FLAC) would get its own
// prefix rather than reusing this one.
const ECDC_OPUS_CACHE_KEY_PREFIX: &str = "ecdc-opus";

const SOUNDKIT_V1_SAMPLE_RATES: [u32; 4] = [16000, 44100, 48000, 96000];
const SOUNDKIT_V2_SAMPLE_RATES: [u32; 11] = [
    8000, 12000, 16000, 24000, 32000, 44100, 48000, 88200, 96000, 176400, 192000,
];
const SOUNDKIT_V2_BITS_PER_SAMPLE: [u16; 6] = [0, 8, 16, 24, 32, 64];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SoundkitFrameHeader {
    encoding: u8,
    endianness: u8,
    sample_rate: u32,
    channels: u8,
    bits_per_sample: u16,
    sample_size: u32,
    payload_size: u32,
    id: Option<u64>,
    pts: Option<u64>,
    packet_crc32: Option<u32>,
    header_size: usize,
    packet_header_version: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SoundkitFrameHeaderOptions {
    #[serde(default = "default_soundkit_encoding")]
    encoding: u8,
    #[serde(default = "default_soundkit_sample_rate")]
    sample_rate: u32,
    #[serde(default = "default_soundkit_channels")]
    channels: u8,
    #[serde(default = "default_soundkit_bits_per_sample")]
    bits_per_sample: u16,
    #[serde(default)]
    sample_size: u32,
    #[serde(default)]
    payload_size: u32,
    #[serde(default)]
    pts: Option<u64>,
    #[serde(default)]
    packet_header_version: Option<u8>,
}

fn default_soundkit_encoding() -> u8 { 2 }
fn default_soundkit_sample_rate() -> u32 { 48_000 }
fn default_soundkit_channels() -> u8 { 2 }
fn default_soundkit_bits_per_sample() -> u16 { 16 }

#[wasm_bindgen]
pub struct OpusEncoder {
    inner: RustOpusEncoder,
    channels: usize,
    frame_size: usize,
}

#[wasm_bindgen]
pub struct OpusEncodeResult {
    encoded: Vec<u8>,
}

#[wasm_bindgen]
pub struct OpusDecoder {
    inner: RustOpusDecoder,
    output: Vec<i16>,
    decoded_size: usize,
}

#[wasm_bindgen]
pub struct OpusDecodeResult {
    output: Vec<i16>,
    decoded_size: usize,
}

#[wasm_bindgen(js_name = playerWasmBuildInfoJson)]
pub fn record_decoder_build_info_json() -> String {
    json!({
        "crate": "player-wasm",
        "api": "bitneedle-record-wasm",
        "version": env!("CARGO_PKG_VERSION"),
        "builtFrom": "vin.yl.player/record-wasm",
        "recordProfiles": record_core::known_record_profile_names(),
        "capabilities": {
            "pngDecode": true,
            "recordVerification": true,
            "sidecars": true,
            "ecdcDecode": true,
            "lmDecode": true,
            "playbackMetadata": true
        }
    }).to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayerLmEcdcChunk {
    offset: usize,
    samples: usize,
    frame_length: usize,
    payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayerLmEcdcChunks {
    metadata: EcdcMetadata,
    chunks: Vec<PlayerLmEcdcChunk>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Bcs2OpusChunkCacheKeys {
    format: &'static str,
    store_name: &'static str,
    cache_version: &'static str,
    output_codec: &'static str,
    bitrate: u32,
    keys: Vec<String>,
}

#[wasm_bindgen(js_name = stableHashHex)]
pub fn wasm_stable_hash_hex(bytes: &[u8]) -> String {
    stable_hash_hex(bytes)
}

#[wasm_bindgen(js_name = bcs2OpusChunkCacheKeysJson)]
pub fn wasm_bcs2_opus_chunk_cache_keys_json(bcs2: &[u8]) -> Result<String, JsValue> {
    let stream = record_core::parse_chunk_stream(bcs2).map_err(to_js_error)?;
    let keys = stream.chunks.iter().map(|chunk| opus_chunk_cache_key_u64_hex(&chunk.payload)).collect();
    serde_json::to_string(&Bcs2OpusChunkCacheKeys {
        format: OPUS_CHUNK_CACHE_KEY_FORMAT,
        store_name: OPUS_CHUNK_CACHE_STORE_NAME,
        cache_version: OPUS_CHUNK_CACHE_VERSION,
        output_codec: OPUS_CHUNK_CACHE_OUTPUT_CODEC,
        bitrate: OPUS_CHUNK_CACHE_BITRATE,
        keys,
    }).map_err(to_js_error)
}

#[wasm_bindgen(js_name = ecdcMetadata)]
pub fn wasm_ecdc_metadata(payload: &[u8]) -> Result<JsValue, JsValue> {
    let metadata: EcdcMetadata = read_ecdc_header(&mut Cursor::new(payload)).map_err(to_js_error)?;
    to_js_value(&metadata)
}

#[wasm_bindgen(js_name = ecdcFrameRanges)]
pub fn wasm_ecdc_frame_ranges(payload: &[u8]) -> Result<JsValue, JsValue> {
    let mut reader = Cursor::new(payload);
    let _: EcdcMetadata = read_ecdc_header(&mut reader).map_err(to_js_error)?;
    let ranges = ecdc_frame_ranges(payload, reader.position() as usize).map_err(to_js_error)?;
    let mapped: Vec<Value> = ranges.into_iter().map(|range| json!({
        "start": range.start,
        "end": range.end,
        "byteLength": range.end - range.start,
    })).collect();
    to_js_value(&mapped)
}

#[wasm_bindgen(js_name = ecdcChunkLayoutFromMetadata)]
pub fn wasm_ecdc_chunk_layout_from_metadata(bundle_json: &str, payload: &[u8], chunk_count: usize) -> Result<JsValue, JsValue> {
    let meta = parse_encodec_bundle_metadata(bundle_json)?;
    let metadata: EcdcMetadata = read_ecdc_header(&mut Cursor::new(payload)).map_err(to_js_error)?;
    let _ = chunk_count;
    let layout: EcdcChunkLayout = ecdc_chunk_layout_from_metadata(&meta, &metadata).map_err(to_js_error)?;
    to_js_value(&json!({ "samples": layout.samples, "stride": layout.stride }))
}

#[wasm_bindgen(js_name = ecdcCropDecodedOwnedAudio)]
pub fn wasm_ecdc_crop_decoded_owned_audio(bundle_json: &str, decoded_audio: &[f32]) -> Result<Vec<f32>, JsValue> {
    let meta = parse_encodec_bundle_metadata(bundle_json)?;
    let channels = meta.channels;
    if channels == 0 || decoded_audio.len() % channels != 0 {
        return Err(to_js_error("invalid decoded audio channel geometry"));
    }
    let decoded_samples = decoded_audio.len() / channels;
    let left_guard = record_core::ecdc::ECDC_OUTPUT_OFFSET_SAMPLES as usize;
    let owned_samples = record_core::ecdc::ECDC_OUTPUT_SAMPLES as usize;
    let required = record_core::ecdc::ECDC_BLOCK_SAMPLES as usize;
    if decoded_samples < required {
        return Err(to_js_error(format!("decoded audio has {decoded_samples} samples per channel but requires {required}")));
    }
    let mut cropped = vec![0.0_f32; channels * owned_samples];
    for channel in 0..channels {
        let src_base = channel * decoded_samples + left_guard;
        let dst_base = channel * owned_samples;
        cropped[dst_base..dst_base + owned_samples].copy_from_slice(&decoded_audio[src_base..src_base + owned_samples]);
    }
    Ok(cropped)
}

#[wasm_bindgen(js_name = buildSoundkitFrameHeader)]
pub fn wasm_build_soundkit_frame_header(options: JsValue) -> Result<Vec<u8>, JsValue> {
    let options: SoundkitFrameHeaderOptions = serde_wasm_bindgen::from_value(options).map_err(to_js_error)?;
    build_soundkit_frame_header(&options)
}

#[wasm_bindgen(js_name = decodeSoundkitFrameHeader)]
pub fn wasm_decode_soundkit_frame_header(packet: &[u8]) -> Result<JsValue, JsValue> {
    let header = decode_soundkit_frame_header(packet)?;
    to_js_value(&header)
}

#[wasm_bindgen]
impl OpusEncoder {
    #[wasm_bindgen(constructor)]
    pub fn new(
        channels: usize,
        sample_rate: i32,
        bitrate: i32,
        frame_size: usize,
    ) -> Result<OpusEncoder, JsValue> {
        if sample_rate != 48_000 {
            return Err(to_js_error("player-wasm Opus currently supports 48 kHz CELT-only Opus"));
        }
        let mut inner =
            RustOpusEncoder::new(sample_rate, channels, OpusApplication::RestrictedLowDelay)
                .map_err(to_js_error)?;
        inner.set_bitrate(bitrate).map_err(to_js_error)?;
        inner.set_vbr(false).map_err(to_js_error)?;
        Ok(Self {
            inner,
            channels,
            frame_size,
        })
    }

    #[wasm_bindgen(js_name = enc_frame)]
    pub fn enc_frame(&mut self, input: &[i16]) -> Result<OpusEncodeResult, JsValue> {
        let required = self.frame_size * self.channels;
        if input.len() < required {
            return Err(to_js_error(format!(
                "Opus encode input too short: got {}, need {required}",
                input.len()
            )));
        }
        let encoded = self
            .inner
            .encode_i16(&input[..required], self.frame_size)
            .map_err(to_js_error)?;
        Ok(OpusEncodeResult { encoded })
    }

    #[wasm_bindgen(js_name = set_vbr)]
    pub fn set_vbr(&mut self, enabled: bool) -> Result<(), JsValue> {
        self.inner.set_vbr(enabled).map_err(to_js_error)
    }

    pub fn destroy(self) {}
}

#[wasm_bindgen]
impl OpusEncodeResult {
    #[wasm_bindgen(getter)]
    pub fn ok(&self) -> bool { true }

    #[wasm_bindgen(getter, js_name = encodedData)]
    pub fn encoded_data(&self) -> Vec<u8> { self.encoded.clone() }
}

#[wasm_bindgen]
impl OpusDecoder {
    #[wasm_bindgen(constructor)]
    pub fn new(channels: usize, sample_rate: i32, _frame_size: usize) -> Result<OpusDecoder, JsValue> {
        if sample_rate != 48_000 {
            return Err(to_js_error("player-wasm Opus currently supports 48 kHz CELT-only Opus"));
        }
        let inner = RustOpusDecoder::new(sample_rate, channels).map_err(to_js_error)?;
        Ok(Self {
            inner,
            output: Vec::new(),
            decoded_size: 0,
        })
    }

    #[wasm_bindgen(js_name = dec_frame)]
    pub fn dec_frame(&mut self, packet: &[u8]) -> Result<OpusDecodeResult, JsValue> {
        self.decode_reuse(packet)?;
        Ok(OpusDecodeResult {
            output: self.output.clone(),
            decoded_size: self.decoded_size,
        })
    }

    #[wasm_bindgen(js_name = dec_frame_reuse)]
    pub fn dec_frame_reuse(&mut self, packet: &[u8]) -> Result<usize, JsValue> {
        self.decode_reuse(packet)
    }

    #[wasm_bindgen(getter, js_name = decodedSize)]
    pub fn decoded_size(&self) -> usize { self.decoded_size }

    #[wasm_bindgen(getter, js_name = outputPtr)]
    pub fn output_ptr(&self) -> usize { self.output.as_ptr() as usize }

    #[wasm_bindgen(getter, js_name = outputLen)]
    pub fn output_len(&self) -> usize { self.output.len() }

    pub fn destroy(self) {}
}

impl OpusDecoder {
    fn decode_reuse(&mut self, packet: &[u8]) -> Result<usize, JsValue> {
        self.decoded_size = self
            .inner
            .decode_i16_into(packet, false, &mut self.output)
            .map_err(to_js_error)?;
        Ok(self.decoded_size)
    }
}

#[wasm_bindgen]
impl OpusDecodeResult {
    #[wasm_bindgen(getter, js_name = decodedSize)]
    pub fn decoded_size(&self) -> usize { self.decoded_size }

    #[wasm_bindgen(getter)]
    pub fn output(&self) -> Vec<i16> { self.output.clone() }
}

fn next_is_ecdc_magic(payload: &[u8], reader: &Cursor<&[u8]>) -> bool {
    let pos = reader.position() as usize;
    payload.get(pos..pos + 4) == Some(b"ECDC")
}

#[wasm_bindgen(js_name = lmEcdcDecodeChunks)]
pub fn wasm_lm_ecdc_decode_chunks(bundle_json: &str, payload: &[u8]) -> Result<JsValue, JsValue> {
    let meta = parse_encodec_bundle_metadata(bundle_json)?;
    let mut reader = Cursor::new(payload);
    let mut chunks = Vec::new();
    let mut global_offset = 0usize;
    let mut aggregate_metadata: Option<EcdcMetadata> = None;
    while (reader.position() as usize) < payload.len() {
        let metadata: EcdcMetadata = read_ecdc_header(&mut reader).map_err(to_js_error)?;
        validate_metadata(&meta, &metadata).map_err(to_js_error)?;
        if !metadata.use_lm { return Err(to_js_error("ECDC payload does not use LM coding")); }
        let layout = ecdc_chunk_layout_from_metadata(&meta, &metadata).map_err(to_js_error)?;
        let mut local_offset = 0usize;
        while (reader.position() as usize) < payload.len() && !next_is_ecdc_magic(payload, &reader) {
            let chunk_payload = read_chunk_payload(&mut reader, true).map_err(to_js_error)?;
            let samples = metadata.audio_length.saturating_sub(local_offset).min(layout.samples);
            let frame_length = ecdc_lm_frame_length(&metadata, samples, meta.segment_samples, meta.frame_length);
            chunks.push(PlayerLmEcdcChunk { offset: global_offset + local_offset, samples, frame_length, payload: chunk_payload });
            local_offset += layout.stride;
        }
        global_offset += metadata.audio_length;
        match aggregate_metadata.as_mut() {
            Some(existing) => existing.audio_length = global_offset,
            None => { let mut first = metadata; first.audio_length = global_offset; aggregate_metadata = Some(first); }
        }
    }
    let metadata = aggregate_metadata.ok_or_else(|| to_js_error("ECDC payload contained no revolutions"))?;
    to_js_value(&PlayerLmEcdcChunks { metadata, chunks })
}

#[wasm_bindgen]
pub struct QuantizedLmChunkDecoder {
    meta: OnnxFrameBundleMetadata,
    lm: QuantizedLm,
    state: QuantizedLmState,
    lm_window_frame_length: usize,
    input_symbols: Vec<usize>,
    decoder: ArithmeticDecoder,
    scale: f32,
    pulled_steps: usize,
}

#[wasm_bindgen]
impl QuantizedLmChunkDecoder {
    #[wasm_bindgen(constructor)]
    pub fn new(bundle_json: &str, weights: &[u8], payload: &[u8]) -> Result<QuantizedLmChunkDecoder, JsValue> {
        let meta = parse_encodec_bundle_metadata(bundle_json)?;
        validate_encodec_lm_metadata(&meta).map_err(to_js_error)?;
        let weights = QuantizedLmWeights::from_bytes(weights).map_err(to_js_error)?;
        weights.validate_for_codebooks(meta.num_codebooks).map_err(to_js_error)?;
        let lm_window_frame_length = weights.frame_length.max(1);
        let lm = QuantizedLm::new(weights);
        let state = lm.initial_state();
        let mut cursor = Cursor::new(payload);
        let scale = if meta.normalize {
            let bytes = read_exactly(&mut cursor, 4).map_err(to_js_error)?;
            f32::from_be_bytes(bytes.try_into().expect("slice length"))
        } else { 1.0 };
        let remaining = payload.len().saturating_sub(cursor.position() as usize);
        let encoded = read_exactly(&mut cursor, remaining).map_err(to_js_error)?;
        Ok(Self {
            input_symbols: vec![0; meta.num_codebooks], meta, lm, state, lm_window_frame_length,
            decoder: ArithmeticDecoder::new(encoded, ARITHMETIC_TOTAL_RANGE_BITS).map_err(to_js_error)?,
            scale, pulled_steps: 0,
        })
    }
    pub fn bitstream_version(&self) -> u8 { QUANTIZED_LM_BITSTREAM_VERSION }

    #[wasm_bindgen(js_name = lmWindowFrameLength)]
    pub fn lm_window_frame_length(&self) -> usize { self.lm_window_frame_length }
    pub fn scale(&self) -> f32 { self.scale }
    pub fn pull(&mut self) -> Result<Vec<u16>, JsValue> {
        if self.pulled_steps > 0 && self.pulled_steps % self.lm_window_frame_length == 0 {
            self.state = self.lm.initial_state(); self.input_symbols.fill(0);
        }
        let logits = self.lm.forward_step(&mut self.state, &self.input_symbols).map_err(to_js_error)?;
        let pdf = encodec_probability_columns_from_logits(&logits, &self.meta, 1.0).map_err(to_js_error)?;
        let symbols = self.decoder.pull_symbols(&pdf, self.meta.lm_cardinality(), self.meta.num_codebooks, DEFAULT_FP_SCALE, DEFAULT_MIN_RANGE).map_err(to_js_error)?;
        for (dst, symbol) in self.input_symbols.iter_mut().zip(symbols.iter().copied()) { *dst = symbol + 1; }
        self.pulled_steps += 1;
        symbols.into_iter().map(|symbol| u16::try_from(symbol).map_err(|_| to_js_error(format!("LM symbol {symbol} does not fit u16")))).collect()
    }
}

#[wasm_bindgen(js_name = normalizeRecordProfileName)]
pub fn wasm_normalize_record_profile_name(record_profile: &str) -> Result<String, JsValue> {
    record_core::normalize_record_profile_name(record_profile).map_err(to_js_error)
}

#[wasm_bindgen(js_name = recordPlaybackMetadataFromHeaderJson)]
pub fn wasm_record_playback_metadata_from_header_json(header_json: &str) -> String {
    let header = serde_json::from_str::<Value>(header_json).unwrap_or(Value::Null);
    let descriptor = header.get("descriptor").filter(|v| v.is_object()).unwrap_or(&header);
    let arbitrary = descriptor.get("arbitraryMetadata").and_then(Value::as_str)
        .and_then(|s| serde_json::from_str::<Value>(s).ok()).unwrap_or(Value::Null);
    let payload_container = arbitrary.get("payloadContainer").or_else(|| descriptor.get("payloadContainer")).and_then(Value::as_str).unwrap_or("");
    let track_listing = arbitrary.get("trackListing").or_else(|| descriptor.get("trackListing")).cloned().unwrap_or_else(|| Value::Array(vec![]));
    let dummy = arbitrary.get("dummySpiralRegions").or_else(|| descriptor.get("dummySpiralRegions")).cloned().unwrap_or_else(|| Value::Array(vec![]));
    json!({ "payloadContainer": payload_container, "entryContainer": "", "trackListing": track_listing, "dummySpiralRegions": dummy }).to_string()
}

#[wasm_bindgen(js_name = resolvePlaybackPayloadMetadataJson)]
pub fn wasm_resolve_playback_payload_metadata_json(header_metadata_json: &str, payload_metadata_json: &str, _length_prefixed_entries: bool) -> String {
    let header = serde_json::from_str::<Value>(header_metadata_json).unwrap_or(Value::Null);
    let payload = serde_json::from_str::<Value>(payload_metadata_json).unwrap_or(Value::Null);
    let payload_container = header.get("payloadContainer").and_then(Value::as_str).filter(|s| !s.is_empty())
        .or_else(|| payload.get("payloadDescriptors").and_then(Value::as_array).and_then(|a| a.first()).and_then(|v| v.get("container")).and_then(Value::as_str))
        .or_else(|| payload.get("payloadContainer").and_then(Value::as_str)).unwrap_or("").to_uppercase();
    let tracks = header.get("trackListing").and_then(Value::as_array).filter(|a| !a.is_empty()).cloned()
        .or_else(|| payload.get("trackListing").and_then(Value::as_array).cloned()).unwrap_or_default();
    let dummy = header.get("dummySpiralRegions").and_then(Value::as_array).filter(|a| !a.is_empty()).cloned()
        .or_else(|| payload.get("dummySpiralRegions").and_then(Value::as_array).cloned()).unwrap_or_default();
    json!({
        "payloadContainer": payload_container,
        "payloadCodec": payload.get("payloadCodec").and_then(Value::as_str).unwrap_or(""),
        "entryContainer": "single",
        "trackListing": tracks,
        "dummySpiralRegions": dummy,
    }).to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EcdcCacheChunk { chunk_index: usize, chunk_offset: usize, chunk_byte_length: usize }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EcdcCacheProofContext { format: String, header_base64url: String, header_byte_length: usize, chunks: Vec<EcdcCacheChunk> }

#[wasm_bindgen(js_name = createPlayerEcdcCacheProofContextJson)]
pub fn wasm_create_player_ecdc_cache_proof_context_json(ecdc: &[u8]) -> String {
    create_player_ecdc_cache_proof_context(ecdc).and_then(|c| serde_json::to_string(&c).ok()).unwrap_or_else(|| "null".to_string())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EcdcCacheChunkProof { record_header: String, chunk_index: usize, chunk_offset: usize, chunk_byte_length: usize }

// Builds the per-chunk proof (record header + this chunk's byte range) a
// batch write needs. The record header comes from the whole-stream context
// produced by createPlayerEcdcCacheProofContextJson (its header/format
// fields are always correct); chunk_offset/chunk_byte_length are supplied
// by the caller directly rather than looked up from the context's own
// `chunks` array, because that array is derived from a naive [len][payload]
// byte walk that only matches this file's synthetic unit-test fixture — real
// production ECDC streams are an interleaved/arithmetic-coded bitstream with
// a different chunk layout, and the caller already has the real per-chunk
// byte range from the actual EnCodec chunk decoder (e.g. encodec-rs's
// lmEcdcDecodeChunks).
#[wasm_bindgen(js_name = playerEcdcCacheProofForChunkJson)]
pub fn wasm_player_ecdc_cache_proof_for_chunk_json(
    context_json: &str,
    chunk_index: usize,
    chunk_offset: usize,
    chunk_byte_length: usize,
) -> String {
    (|| -> Option<String> {
        let context: EcdcCacheProofContext = serde_json::from_str(context_json).ok()?;
        let proof = EcdcCacheChunkProof {
            record_header: format!("{}:{}", context.format, context.header_base64url),
            chunk_index,
            chunk_offset,
            chunk_byte_length,
        };
        serde_json::to_string(&proof).ok()
    })()
    .unwrap_or_else(|| "null".to_string())
}

// Takes the chunk's own entropy-coded payload bytes directly (as sliced by
// the real EnCodec chunk decoder, e.g. encodec-rs's lmEcdcDecodeChunks) —
// NOT the whole ECDC buffer plus an index. The whole-buffer + byte-walking
// approach previously used here (create_player_ecdc_cache_proof_context's
// chunk enumeration) only matches the hand-rolled [len][payload] framing of
// this file's own unit-test fixture; real production ECDC streams are an
// interleaved/arithmetic-coded bitstream with a completely different chunk
// boundary structure, so that walk silently stopped after chunk 0 on real
// records. The caller already knows the correct per-chunk byte range from
// the real decoder, so we just hash whatever payload it hands us.
#[wasm_bindgen(js_name = ecdcChunkCacheKey)]
pub fn wasm_ecdc_chunk_cache_key(chunk_payload: &[u8], record_binding_hex: &str) -> String {
    ecdc_chunk_cache_key_hex(chunk_payload, record_binding_hex)
}

fn create_player_ecdc_cache_proof_context(ecdc: &[u8]) -> Option<EcdcCacheProofContext> {
    if ecdc.len() < 8 || ecdc.get(0..4)? != b"ECDC" { return None; }
    let metadata_start = ecdc.iter().position(|byte| *byte == b'{')?;
    let header_end = json_object_end(ecdc, metadata_start)?;
    serde_json::from_slice::<Value>(&ecdc[metadata_start..header_end]).ok()?;
    let mut chunks = Vec::new();
    let mut offset = header_end;
    while offset + 8 <= ecdc.len() {
        let payload_len = u32::from_be_bytes(ecdc.get(offset..offset + 4)?.try_into().ok()?) as usize;
        let end = offset.checked_add(8 + payload_len)?;
        if end > ecdc.len() { break; }
        chunks.push(EcdcCacheChunk { chunk_index: chunks.len(), chunk_offset: offset, chunk_byte_length: 8 + payload_len });
        offset = end;
    }
    if chunks.is_empty() { return None; }
    Some(EcdcCacheProofContext {
        format: "ecdc-v2".into(), header_base64url: general_purpose::URL_SAFE_NO_PAD.encode(&ecdc[..header_end]), header_byte_length: header_end, chunks
    })
}

fn json_object_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start).copied()? != b'{' { return None; }
    let (mut depth, mut in_string, mut escaped) = (0i32, false, false);
    for (index, byte) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped { escaped = false; } else if *byte == b'\\' { escaped = true; } else if *byte == b'"' { in_string = false; }
            continue;
        }
        match *byte { b'"' => in_string = true, b'{' => depth += 1, b'}' => { depth -= 1; if depth == 0 { return Some(index + 1); } }, _ => {} }
    }
    None
}

fn opus_chunk_cache_key_u64_hex(source_payload: &[u8]) -> String {
    let hash = ecdc_chunk_cache_hash_hex(source_payload, "");
    format!("{ECDC_OPUS_CACHE_KEY_PREFIX}/{}", &hash[0..16])
}

fn ecdc_chunk_cache_key_hex(source_payload: &[u8], record_binding_hex: &str) -> String {
    let hash = ecdc_chunk_cache_hash_hex(source_payload, record_binding_hex);
    format!("{ECDC_OPUS_CACHE_KEY_PREFIX}/{hash}")
}

// Raw sha256-derived hash, with no `{prefix}/` segment. Every cache-key
// variant (full 64-hex, truncated 16-hex) prefixes this the same way so the
// object type (currently always `ecdc-opus`, the only codec this player
// ships) is visible in the R2 key rather than only baked into the hash —
// that makes prefix `list()`/lifecycle rules and manual debugging possible.
fn ecdc_chunk_cache_hash_hex(source_payload: &[u8], record_binding_hex: &str) -> String {
    let source_payload_hash = stable_hash_hex(source_payload);
    let mut preimage = format!("{OPUS_CHUNK_CACHE_KEY_DOMAIN}\nsource_payload_sha256={source_payload_hash}\n");
    let binding = record_binding_hex.trim().to_ascii_lowercase();
    if !binding.is_empty() {
        preimage.push_str(&format!("record_binding_sha256={binding}\n"));
    }
    preimage.push_str(&format!(
        "output_codec={OPUS_CHUNK_CACHE_OUTPUT_CODEC}\nbitrate={OPUS_CHUNK_CACHE_BITRATE}\ncache_version={OPUS_CHUNK_CACHE_VERSION}\n"
    ));
    stable_hash_hex(preimage.as_bytes())
}

fn decode_soundkit_frame_header(data: &[u8]) -> Result<SoundkitFrameHeader, JsValue> {
    if data.len() < 4 {
        return Err(to_js_error("Invalid SoundKit frame header."));
    }
    let header = u32::from_be_bytes(data[0..4].try_into().expect("slice length"));
    let magic_word = header >> 26;
    match magic_word {
        0x2b => decode_soundkit_frame_header_v2(data, header),
        0x2a => decode_soundkit_frame_header_v1(data, header),
        _ => Err(to_js_error("Invalid SoundKit frame magic.")),
    }
}

fn decode_soundkit_frame_header_v1(data: &[u8], header: u32) -> Result<SoundkitFrameHeader, JsValue> {
    let sample_rate = SOUNDKIT_V1_SAMPLE_RATES
        .get(((header >> 24) & 0x3) as usize)
        .copied()
        .ok_or_else(|| to_js_error("Unsupported SoundKit frame format."))?;
    let bits_per_sample = match (header >> 22) & 0x3 {
        0 => 16,
        1 => 24,
        2 => 32,
        _ => return Err(to_js_error("Unsupported SoundKit frame format.")),
    };
    let has_pts = ((header >> 21) & 0x1) == 1;
    let has_id = ((header >> 20) & 0x1) == 1;
    let encoding = ((header >> 17) & 0x7) as u8;
    let endianness = ((header >> 16) & 0x1) as u8;
    let channels = (((header >> 12) & 0xf) + 1) as u8;
    let sample_size = (header & 0xfff) as u32;
    let mut offset = 4usize;
    let id = if has_id {
        if data.len() < offset + 8 {
            return Err(to_js_error("Truncated SoundKit frame id."));
        }
        let value = u64::from_be_bytes(data[offset..offset + 8].try_into().expect("slice length"));
        offset += 8;
        Some(value)
    } else {
        None
    };
    let pts = if has_pts {
        if data.len() < offset + 8 {
            return Err(to_js_error("Truncated SoundKit frame pts."));
        }
        let value = u64::from_be_bytes(data[offset..offset + 8].try_into().expect("slice length"));
        offset += 8;
        Some(value)
    } else {
        None
    };
    Ok(SoundkitFrameHeader {
        encoding,
        endianness,
        sample_rate,
        channels,
        bits_per_sample,
        sample_size,
        payload_size: data.len().saturating_sub(offset) as u32,
        id,
        pts,
        packet_crc32: None,
        header_size: offset,
        packet_header_version: 1,
    })
}

fn decode_soundkit_frame_header_v2(data: &[u8], header: u32) -> Result<SoundkitFrameHeader, JsValue> {
    if data.len() < 8 {
        return Err(to_js_error("Truncated SoundKit v2 frame header."));
    }
    let version = ((header >> 24) & 0x3) as u8;
    if version != 2 {
        return Err(to_js_error("Invalid SoundKit v2 frame version."));
    }
    let flags = ((header >> 16) & 0xff) as u8;
    let encoding = ((header >> 12) & 0x0f) as u8;
    let sample_rate = SOUNDKIT_V2_SAMPLE_RATES
        .get(((header >> 8) & 0x0f) as usize)
        .copied()
        .ok_or_else(|| to_js_error("Unsupported SoundKit v2 frame format."))?;
    let channels = (((header >> 3) & 0x1f) + 1) as u8;
    let bits_per_sample = SOUNDKIT_V2_BITS_PER_SAMPLE
        .get((header & 0x7) as usize)
        .copied()
        .ok_or_else(|| to_js_error("Unsupported SoundKit v2 frame format."))?;
    let has_id = (flags & 0x01) != 0;
    let id_is_u64 = (flags & 0x02) != 0;
    let has_pts = (flags & 0x04) != 0;
    let has_crc = (flags & 0x08) != 0;
    let big_endian = (flags & 0x10) != 0;
    let extended_sizes = (flags & 0x20) != 0;
    if id_is_u64 && !has_id {
        return Err(to_js_error("Invalid SoundKit v2 frame id flags."));
    }
    let size_word = u32::from_be_bytes(data[4..8].try_into().expect("slice length"));
    let (payload_size, sample_size, mut offset) = if extended_sizes {
        if size_word != 0xffff_ffff || data.len() < 16 {
            return Err(to_js_error("Invalid SoundKit v2 extended size header."));
        }
        (
            u32::from_be_bytes(data[8..12].try_into().expect("slice length")),
            u32::from_be_bytes(data[12..16].try_into().expect("slice length")),
            16usize,
        )
    } else {
        let payload_size = (size_word >> 16) & 0xffff;
        let sample_size = size_word & 0xffff;
        if payload_size == 0xffff || sample_size == 0xffff {
            return Err(to_js_error("Invalid SoundKit v2 short size sentinel."));
        }
        (payload_size, sample_size, 8usize)
    };
    let id = if has_id {
        if id_is_u64 {
            if data.len() < offset + 8 {
                return Err(to_js_error("Truncated SoundKit v2 frame id."));
            }
            let value = u64::from_be_bytes(data[offset..offset + 8].try_into().expect("slice length"));
            offset += 8;
            Some(value)
        } else {
            if data.len() < offset + 4 {
                return Err(to_js_error("Truncated SoundKit v2 frame id."));
            }
            let value = u32::from_be_bytes(data[offset..offset + 4].try_into().expect("slice length")) as u64;
            offset += 4;
            Some(value)
        }
    } else {
        None
    };
    let pts = if has_pts {
        if data.len() < offset + 8 {
            return Err(to_js_error("Truncated SoundKit v2 frame pts."));
        }
        let value = u64::from_be_bytes(data[offset..offset + 8].try_into().expect("slice length"));
        offset += 8;
        Some(value)
    } else {
        None
    };
    let packet_crc32 = if has_crc {
        if data.len() < offset + 4 {
            return Err(to_js_error("Truncated SoundKit v2 frame crc."));
        }
        let value = u32::from_be_bytes(data[offset..offset + 4].try_into().expect("slice length"));
        offset += 4;
        Some(value)
    } else {
        None
    };
    if payload_size == 0 || sample_size == 0 || data.len() < offset + payload_size as usize {
        return Err(to_js_error("Invalid SoundKit v2 Opus payload."));
    }
    Ok(SoundkitFrameHeader {
        encoding,
        endianness: if big_endian { 1 } else { 0 },
        sample_rate,
        channels,
        bits_per_sample,
        sample_size,
        payload_size,
        id,
        pts,
        packet_crc32,
        header_size: offset,
        packet_header_version: 2,
    })
}

fn build_soundkit_frame_header(options: &SoundkitFrameHeaderOptions) -> Result<Vec<u8>, JsValue> {
    let encoding = options.encoding;
    let sample_rate = options.sample_rate;
    let channels = options.channels.clamp(1, 32);
    let bits_per_sample = options.bits_per_sample;
    let sample_size = options.sample_size;
    let payload_size = options.payload_size;
    let pts = options.pts;
    let packet_header_version = options.packet_header_version.unwrap_or(2);

    if packet_header_version == 2 || payload_size > 0xffff || sample_size > 0x0fff {
        if payload_size == 0 || sample_size == 0 {
            return Err(to_js_error("SoundKit v2 frame header requires payloadSize and sampleSize."));
        }
        let use_extended_sizes = payload_size > 0xfffe || sample_size > 0xfffe;
        let has_pts = pts.is_some();
        let flags: u8 = (if has_pts { 0x04 } else { 0 }) | (if use_extended_sizes { 0x20 } else { 0 });
        let header = ((0x2b_u32) << 26)
            | ((2_u32) << 24)
            | ((flags as u32) << 16)
            | (((encoding & 0x0f) as u32) << 12)
            | ((soundkit_sample_rate_index_v2(sample_rate)? as u32) << 8)
            | (((channels as u32 - 1) & 0x1f) << 3)
            | (soundkit_bits_per_sample_index_v2(bits_per_sample)? as u32);
        let header_size = if use_extended_sizes { 16 } else { 8 } + if has_pts { 8 } else { 0 };
        let mut output = vec![0u8; header_size];
        output[0..4].copy_from_slice(&header.to_be_bytes());
        if use_extended_sizes {
            output[4..8].copy_from_slice(&0xffff_ffff_u32.to_be_bytes());
            output[8..12].copy_from_slice(&payload_size.to_be_bytes());
            output[12..16].copy_from_slice(&sample_size.to_be_bytes());
        } else {
            let size_word = ((payload_size & 0xffff) << 16) | (sample_size & 0xffff);
            output[4..8].copy_from_slice(&size_word.to_be_bytes());
        }
        if let Some(pts) = pts {
            let offset = if use_extended_sizes { 16 } else { 8 };
            output[offset..offset + 8].copy_from_slice(&pts.to_be_bytes());
        }
        return Ok(output);
    }

    let bits_index: u32 = match bits_per_sample {
        16 => 0,
        24 => 1,
        32 => 2,
        _ => return Err(to_js_error("Unsupported SoundKit frame format.")),
    };
    let has_pts = pts.is_some();
    let header = ((0x2a_u32) << 26)
        | ((soundkit_sample_rate_index_v1(sample_rate)? as u32) << 24)
        | (bits_index << 22)
        | ((if has_pts { 1_u32 } else { 0 }) << 21)
        | (((encoding & 0x7) as u32) << 17)
        | ((((channels.min(16) as u32).saturating_sub(1)) & 0x0f) << 12)
        | (sample_size.min(4095) & 0x0fff);
    let mut output = vec![0u8; 4 + if has_pts { 8 } else { 0 }];
    output[0..4].copy_from_slice(&header.to_be_bytes());
    if let Some(pts) = pts {
        output[4..12].copy_from_slice(&pts.to_be_bytes());
    }
    Ok(output)
}

fn soundkit_sample_rate_index_v1(sample_rate: u32) -> Result<usize, JsValue> {
    SOUNDKIT_V1_SAMPLE_RATES
        .iter()
        .position(|value| *value == sample_rate)
        .ok_or_else(|| to_js_error("Unsupported SoundKit frame format."))
}

fn soundkit_sample_rate_index_v2(sample_rate: u32) -> Result<usize, JsValue> {
    SOUNDKIT_V2_SAMPLE_RATES
        .iter()
        .position(|value| *value == sample_rate)
        .ok_or_else(|| to_js_error("Unsupported SoundKit v2 frame format."))
}

fn soundkit_bits_per_sample_index_v2(bits_per_sample: u16) -> Result<usize, JsValue> {
    SOUNDKIT_V2_BITS_PER_SAMPLE
        .iter()
        .position(|value| *value == bits_per_sample)
        .ok_or_else(|| to_js_error("Unsupported SoundKit v2 frame format."))
}

fn parse_encodec_bundle_metadata(bundle_json: &str) -> Result<OnnxFrameBundleMetadata, JsValue> { serde_json::from_str(bundle_json).map_err(to_js_error) }
fn validate_encodec_lm_metadata(meta: &OnnxFrameBundleMetadata) -> Result<(), String> {
    meta.lm_dim().map_err(|e| e.to_string())?; meta.lm_num_layers().map_err(|e| e.to_string())?; meta.lm_past_context().map_err(|e| e.to_string())?;
    if meta.lm_cardinality() == 0 { return Err("LM cardinality must be non-zero".into()); } Ok(())
}
fn encodec_probability_columns_from_logits(logits: &[f32], meta: &OnnxFrameBundleMetadata, lm_tau: f64) -> Result<Vec<f64>, String> {
    let card = meta.lm_cardinality(); let codebooks = meta.num_codebooks;
    if logits.len() != card * codebooks { return Err("LM logits shape mismatch".into()); }
    let mut pdf = vec![0.0; card * codebooks]; let mut quantized = vec![0.0; card]; let mut probs = vec![0.0; card];
    let uniform = 1.0 / card as f64; let threshold = 0.25 / DEFAULT_FP_SCALE as f64; let step = meta.lm_entropy_logit_step();
    for codebook in 0..codebooks {
        let (mut max_value, mut min_value) = (f64::NEG_INFINITY, f64::INFINITY);
        for bin in 0..card { let q = ((logits[bin * codebooks + codebook] as f64 / lm_tau) / step + 0.5 - 2_f64.powi(-40)).floor() * step; quantized[bin] = q; max_value = max_value.max(q); min_value = min_value.min(q); }
        let mut denom = 0.0; for bin in 0..card { probs[bin] = (quantized[bin] - max_value).exp(); denom += probs[bin]; }
        if !denom.is_finite() || denom <= 0.0 { for bin in 0..card { pdf[bin * codebooks + codebook] = uniform; } continue; }
        let (mut max_pdf, mut min_pdf) = (0.0_f64, f64::INFINITY); for p in &mut probs { *p /= denom; max_pdf = max_pdf.max(*p); min_pdf = min_pdf.min(*p); }
        let near_uniform = max_value - min_value <= 2.0 * step || max_pdf - min_pdf <= threshold;
        for bin in 0..card { pdf[bin * codebooks + codebook] = if near_uniform { uniform } else { probs[bin] }; }
    }
    Ok(pdf)
}
fn to_js_value<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsValue> {
    value.serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true)).map_err(to_js_error)
}
fn to_js_error(error: impl std::fmt::Display) -> JsValue { JsValue::from_str(&error.to_string()) }

#[cfg(test)]
mod tests {
    use super::{
        build_soundkit_frame_header, decode_soundkit_frame_header, ecdc_chunk_cache_key_hex,
        wasm_ecdc_chunk_cache_key, SoundkitFrameHeaderOptions,
    };

    const CHUNK_PAYLOAD_ONE: &[u8] = b"abc";
    const CHUNK_PAYLOAD_TWO: &[u8] = b"def";

    #[test]
    fn ecdc_chunk_cache_key_changes_with_binding_and_chunk() {
        let first = wasm_ecdc_chunk_cache_key(CHUNK_PAYLOAD_ONE, "");
        let second = wasm_ecdc_chunk_cache_key(CHUNK_PAYLOAD_TWO, "");
        let scoped = wasm_ecdc_chunk_cache_key(CHUNK_PAYLOAD_ONE, "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
        assert_ne!(first, second);
        assert_ne!(first, scoped);
        assert!(first.starts_with("ecdc-opus/"));
        assert_eq!(first.len(), "ecdc-opus/".len() + 64);
        assert_eq!(scoped, ecdc_chunk_cache_key_hex(CHUNK_PAYLOAD_ONE, "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"));
    }

    #[test]
    fn soundkit_v2_header_round_trips() {
        let header = build_soundkit_frame_header(&SoundkitFrameHeaderOptions {
            encoding: 2,
            sample_rate: 48_000,
            channels: 2,
            bits_per_sample: 16,
            sample_size: 960,
            payload_size: 127,
            pts: Some(12345),
            packet_header_version: Some(2),
        })
        .unwrap();
        let mut packet = header.clone();
        packet.extend_from_slice(&vec![0u8; 127]);
        let decoded = decode_soundkit_frame_header(&packet).unwrap();
        assert_eq!(decoded.packet_header_version, 2);
        assert_eq!(decoded.sample_rate, 48_000);
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.sample_size, 960);
        assert_eq!(decoded.payload_size, 127);
        assert_eq!(decoded.pts, Some(12345));
    }
}
