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
    use super::{ecdc_chunk_cache_key_hex, wasm_ecdc_chunk_cache_key};

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

}
