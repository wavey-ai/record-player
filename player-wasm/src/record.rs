// Copyright © Wavey, Inc.
// Licensed under the Apache License, Version 2.0.
//
// Player-only WebAssembly record reader. Only PNG extraction and programme-map
// functions required by the standalone player are exported to JavaScript.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::convert::TryInto;
use wasm_bindgen::prelude::*;

const FIXED_CONTEXT_SAMPLES: usize = 480;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixedContextSegmentPlanOptions {
    total_samples: usize,
    segment_samples: usize,
    segment_stride: usize,
    frame_length: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FixedContextSegmentPlan {
    revolution_samples: usize,
    owned_samples: usize,
    context_samples: usize,
    model_samples: usize,
    starts: Vec<usize>,
    frame_lengths: Vec<usize>,
    count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MultiTrackSegmentBudgetOptions {
    revolution_samples: usize,
    sample_rate: usize,
    track_audio_lengths: Vec<usize>,
    gap_after_seconds: Vec<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MultiTrackSegmentBudget {
    track_segment_counts: Vec<usize>,
    gap_segment_counts: Vec<usize>,
    gap_frame_lengths: Vec<usize>,
    total_segments: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EcdcProgrammeBuildOptions {
    ecdc_descriptor: serde_json::Value,
    track_titles: Vec<String>,
    track_entry_counts: Vec<usize>,
    gap_entry_counts: Vec<usize>,
}

fn record_reader_build_info_json() -> String {
    json!({
        "crate": "player-wasm",
        "api": "bitneedle-player-record-reader",
        "version": env!("CARGO_PKG_VERSION"),
        "builtFrom": "player-wasm",
        "descriptorMagic": record_descriptor_magic(),
        "chunkStreamMagic": String::from_utf8_lossy(record_core::RECORD_STREAM_MAGIC)
    })
    .to_string()
}

#[wasm_bindgen(js_name = decodeRecordMetadataJson)]
pub fn wasm_decode_record_metadata_json(png_bytes: &[u8]) -> Result<String, JsValue> {
    decode_record_metadata_json(png_bytes).map_err(to_js_error)
}

pub fn wasm_build_fixed_context_segment_plan_json(options_json: &str) -> Result<String, JsValue> {
    build_fixed_context_segment_plan_json(options_json).map_err(to_js_error)
}

pub fn wasm_build_multi_track_segment_budget_json(options_json: &str) -> Result<String, JsValue> {
    build_multi_track_segment_budget_json(options_json).map_err(to_js_error)
}

pub fn wasm_build_ecdc_programme_json(options_json: &str) -> Result<String, JsValue> {
    build_ecdc_programme_json(options_json).map_err(to_js_error)
}

/// Pre-decode programme map: exact musical/GAP sample boundaries and total
/// duration, recoverable directly from the BRS1 metadata + payload bytes without
/// any neural/PCM decode (plan §8.4, §11.1). `grooveStart`/`grooveEnd` for GAP
/// bands are populated by the renderer's groove-anchor map and are omitted here
/// until that path is wired.
#[wasm_bindgen(js_name = decodeRecordProgrammeMapJson)]
pub fn wasm_decode_record_programme_map_json(png_bytes: &[u8]) -> Result<String, JsValue> {
    decode_record_programme_map_json(png_bytes).map_err(to_js_error)
}

pub fn wasm_validate_record_header_json(
    png_bytes: &[u8],
    record_profile: Option<String>,
) -> Result<String, JsValue> {
    decode_record_header_json(png_bytes, record_profile.as_deref()).map_err(to_js_error)
}

#[wasm_bindgen(js_name = decodeRecordDescriptorHeaderJson)]
pub fn wasm_decode_record_descriptor_header_json(
    png_bytes: &[u8],
    record_profile: Option<String>,
) -> Result<String, JsValue> {
    decode_record_descriptor_header_json(png_bytes, record_profile.as_deref()).map_err(to_js_error)
}

#[wasm_bindgen(js_name = encryptCacheEntry)]
pub fn wasm_encrypt_cache_entry(
    descriptor_json: &str,
    context_json: &str,
    plaintext: &[u8],
) -> Result<Vec<u8>, JsValue> {
    encrypt_cache_entry(descriptor_json, context_json, plaintext).map_err(to_js_error)
}

#[wasm_bindgen(js_name = decryptCacheEntry)]
pub fn wasm_decrypt_cache_entry(
    descriptor_json: &str,
    context_json: &str,
    envelope: &[u8],
) -> Result<Vec<u8>, JsValue> {
    decrypt_cache_entry(descriptor_json, context_json, envelope).map_err(to_js_error)
}

#[wasm_bindgen(js_name = cacheEncryptionRecordBindingHashHex)]
pub fn wasm_cache_encryption_record_binding_hash_hex(
    descriptor_json: &str,
) -> Result<String, JsValue> {
    let descriptor = record_descriptor_from_json(descriptor_json).map_err(to_js_error)?;
    record_descriptor::cache_encryption_record_binding_hash_hex(&descriptor).map_err(to_js_error)
}

#[wasm_bindgen(js_name = inferRecordProfileFromPng)]
pub fn wasm_infer_record_profile_from_png(png_bytes: &[u8]) -> Result<String, JsValue> {
    decode_record_descriptor_resolving_profile(png_bytes, None)
        .map(|(profile, _)| profile)
        .map_err(to_js_error)
}

pub fn wasm_validate_sidecar_container(bts1: &[u8]) -> Result<String, JsValue> {
    let validation = record_sidecar::validate_sidecar_container(bts1).map_err(to_js_error)?;
    serde_json::to_string(&validation).map_err(to_js_error)
}

pub fn wasm_decode_sidecar_container_items_json(bts1: &[u8]) -> Result<String, JsValue> {
    let decoded = record_sidecar::decode_sidecar_container_items(bts1).map_err(to_js_error)?;
    serde_json::to_string(&decoded).map_err(to_js_error)
}

pub fn wasm_decode_record_png_sidecar(
    png_bytes: &[u8],
    record_profile: Option<String>,
) -> Result<Vec<u8>, JsValue> {
    decode_record_png_sidecar_to_bytes(png_bytes, record_profile.as_deref()).map_err(to_js_error)
}

pub fn wasm_decode_record_png_sidecar_json(
    png_bytes: &[u8],
    record_profile: Option<String>,
) -> Result<String, JsValue> {
    let result =
        decode_record_png_sidecar(png_bytes, record_profile.as_deref()).map_err(to_js_error)?;
    serde_json::to_string(&result).map_err(to_js_error)
}

pub fn wasm_decode_record_png_sidecar_items_json(
    png_bytes: &[u8],
    record_profile: Option<String>,
) -> Result<String, JsValue> {
    let bytes = decode_record_png_sidecar_to_bytes(png_bytes, record_profile.as_deref())
        .map_err(to_js_error)?;
    let items = record_sidecar::decode_sidecar_container_items(&bytes).map_err(to_js_error)?;
    serde_json::to_string(&items).map_err(to_js_error)
}

pub fn wasm_extract_label_thumbnail(
    png_bytes: &[u8],
    record_profile: Option<String>,
) -> Result<WasmLabelThumbnail, JsValue> {
    let (bytes, mime) =
        extract_label_thumbnail_image(png_bytes, record_profile.as_deref()).map_err(to_js_error)?;
    Ok(WasmLabelThumbnail { bytes, mime })
}

pub struct WasmLabelThumbnail {
    bytes: Vec<u8>,
    mime: String,
}

impl WasmLabelThumbnail {
    pub fn bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    pub fn mime(&self) -> String {
        self.mime.clone()
    }
}

pub fn wasm_record_png_to_rgb_color_block_png(
    png_bytes: &[u8],
    record_profile: Option<String>,
) -> Result<Vec<u8>, JsValue> {
    record_png_to_rgb_color_block_png(png_bytes, record_profile.as_deref()).map_err(to_js_error)
}

#[wasm_bindgen]
pub struct WasmPayloadDecodeResult {
    payload_bytes: Vec<u8>,
    chunk_stream_bytes: Vec<u8>,
    metadata_json: String,
    silence_map_json: String,
}

#[wasm_bindgen]
impl WasmPayloadDecodeResult {
    #[wasm_bindgen(js_name = payloadBytes)]
    pub fn payload_bytes(&self) -> Vec<u8> {
        self.payload_bytes.clone()
    }

    #[wasm_bindgen(js_name = chunkStreamBytes)]
    pub fn chunk_stream_bytes(&self) -> Vec<u8> {
        self.chunk_stream_bytes.clone()
    }

    #[wasm_bindgen(js_name = metadataJson)]
    pub fn metadata_json(&self) -> String {
        self.metadata_json.clone()
    }

    /// JSON array of `{afterByteOffset, sampleCount}` spans describing where,
    /// in `payloadBytes`, GAP-container silence belongs. `payloadBytes` itself
    /// excludes GAP entries entirely (they carry no real codec data — see
    /// `PAYLOAD_CONTAINER_GAP`), so the EnCodec decoder never sees them; the
    /// player splices `sampleCount` zero-filled PCM samples after decoding
    /// the byte at `afterByteOffset` instead. Empty array (`"[]"`) when the
    /// record has no GAP entries (the common case).
    #[wasm_bindgen(js_name = silenceMapJson)]
    pub fn silence_map_json(&self) -> String {
        self.silence_map_json.clone()
    }
}

#[wasm_bindgen(js_name = decodeRecordPngToPayload)]
pub fn wasm_decode_record_png_to_payload(
    png_bytes: &[u8],
) -> Result<WasmPayloadDecodeResult, JsValue> {
    decode_record_png_to_payload(png_bytes).map_err(to_js_error)
}

#[wasm_bindgen(js_name = decodeRecordPngToPayloadWithLength)]
pub fn wasm_decode_record_png_to_payload_with_length(
    png_bytes: &[u8],
    byte_length: usize,
) -> Result<WasmPayloadDecodeResult, JsValue> {
    decode_record_png_to_payload_with_length(png_bytes, byte_length).map_err(to_js_error)
}

#[wasm_bindgen(js_name = decodeRecordPngToPayloadForProfile)]
pub fn wasm_decode_record_png_to_payload_for_profile(
    png_bytes: &[u8],
    record_profile: &str,
) -> Result<WasmPayloadDecodeResult, JsValue> {
    decode_record_png_to_payload_for_profile(png_bytes, record_profile).map_err(to_js_error)
}

#[wasm_bindgen(js_name = decodeRecordPngToPayloadForProfileWithLength)]
pub fn wasm_decode_record_png_to_payload_for_profile_with_length(
    png_bytes: &[u8],
    record_profile: &str,
    byte_length: usize,
) -> Result<WasmPayloadDecodeResult, JsValue> {
    decode_record_png_to_payload_for_profile_with_length(png_bytes, record_profile, byte_length)
        .map_err(to_js_error)
}

#[wasm_bindgen(js_name = decodeRecordPngToPayloadForProfileWithTurns)]
pub fn wasm_decode_record_png_to_payload_for_profile_with_turns(
    png_bytes: &[u8],
    record_profile: &str,
    _visible_turns: f64,
) -> Result<WasmPayloadDecodeResult, JsValue> {
    decode_record_png_to_payload_for_profile(png_bytes, record_profile).map_err(to_js_error)
}

#[wasm_bindgen(js_name = decodeRecordPngToPayloadForProfileWithTurnsAndLength)]
pub fn wasm_decode_record_png_to_payload_for_profile_with_turns_and_length(
    png_bytes: &[u8],
    record_profile: &str,
    _visible_turns: f64,
    byte_length: usize,
) -> Result<WasmPayloadDecodeResult, JsValue> {
    decode_record_png_to_payload_for_profile_with_length(png_bytes, record_profile, byte_length)
        .map_err(to_js_error)
}

fn decode_record_programme_map_json(png_bytes: &[u8]) -> Result<String> {
    let decoded = decode_record_png_resolving_profile(png_bytes, None)
        .context("failed to decode Bitneedle record PNG")?;
    let stream = record_core::parse_chunk_stream(&decoded.chunk_stream.bytes)
        .context("failed to parse decoded chunk stream")?;
    let map = record_core::build_programme_map(&stream, Some(&decoded.record_profile))
        .context("failed to build pre-decode programme map")?;
    Ok(programme_map_to_json(&map).to_string())
}

fn build_fixed_context_segment_plan_json(options_json: &str) -> Result<String> {
    let options: FixedContextSegmentPlanOptions =
        serde_json::from_str(options_json).context("segment plan options JSON is invalid")?;
    let owned_samples = options.segment_stride.max(1);
    let context_samples =
        fixed_context_samples_for_bundle(options.segment_samples, options.segment_stride)?;
    let model_samples = options.segment_samples.max(1);
    let frame_length = options.frame_length.max(1);
    let starts = client_segment_starts(options.total_samples, owned_samples);
    let plan = FixedContextSegmentPlan {
        revolution_samples: owned_samples,
        owned_samples,
        context_samples,
        model_samples,
        frame_lengths: vec![frame_length; starts.len()],
        count: starts.len(),
        starts,
    };
    serde_json::to_string(&plan).context("failed to serialize fixed-context segment plan")
}

fn build_multi_track_segment_budget_json(options_json: &str) -> Result<String> {
    let options: MultiTrackSegmentBudgetOptions = serde_json::from_str(options_json)
        .context("multi-track segment budget options JSON is invalid")?;
    let revolution_samples = options.revolution_samples.max(1);
    let sample_rate = options.sample_rate.max(1);
    let track_count = options.track_audio_lengths.len();
    let mut track_segment_counts = Vec::with_capacity(track_count);
    let mut gap_segment_counts = Vec::with_capacity(track_count);
    let mut gap_frame_lengths = Vec::with_capacity(track_count);
    let mut total_segments = 0usize;

    for index in 0..track_count {
        let track_samples = options.track_audio_lengths[index].max(1);
        let track_segments =
            ((track_samples.saturating_sub(1)) / revolution_samples).saturating_add(1);
        track_segment_counts.push(track_segments);
        total_segments = total_segments.saturating_add(track_segments);

        let gap_seconds = options.gap_after_seconds.get(index).copied().unwrap_or(0.0);
        let gap_frames = if gap_seconds.is_finite() && gap_seconds > 0.0 {
            ((gap_seconds * sample_rate as f64).round().max(1.0)) as usize
        } else {
            0
        };
        let gap_segments = if gap_frames > 0 {
            ((gap_frames.saturating_sub(1)) / revolution_samples).saturating_add(1)
        } else {
            0
        };
        gap_frame_lengths.push(gap_frames);
        gap_segment_counts.push(gap_segments);
        total_segments = total_segments.saturating_add(gap_segments);
    }

    serde_json::to_string(&MultiTrackSegmentBudget {
        track_segment_counts,
        gap_segment_counts,
        gap_frame_lengths,
        total_segments,
    })
    .context("failed to serialize multi-track segment budget")
}

fn build_ecdc_programme_json(options_json: &str) -> Result<String> {
    let options: EcdcProgrammeBuildOptions =
        serde_json::from_str(options_json).context("ECDC programme options JSON is invalid")?;
    if options.track_titles.len() != options.track_entry_counts.len() {
        bail!("track_titles and track_entry_counts lengths must match");
    }

    let mut payload_cursor = 0usize;
    let mut tracks = Vec::with_capacity(options.track_titles.len());
    let mut track_gaps = Vec::new();

    for index in 0..options.track_titles.len() {
        let track_entry_count = options.track_entry_counts[index];
        let first_track_index = payload_cursor;
        let payload_indexes = (first_track_index
            ..first_track_index.saturating_add(track_entry_count))
            .collect::<Vec<_>>();
        payload_cursor = payload_cursor.saturating_add(track_entry_count);
        tracks.push(serde_json::json!({
            "title": options.track_titles[index],
            "payloadIndexes": payload_indexes,
        }));

        let gap_entry_count = options.gap_entry_counts.get(index).copied().unwrap_or(0);
        if gap_entry_count > 0 {
            track_gaps.push(serde_json::json!({
                "firstRevolutionIndex": payload_cursor,
                "revolutionCount": gap_entry_count,
                "afterTrackIndex": index,
            }));
            payload_cursor = payload_cursor.saturating_add(gap_entry_count);
        }
    }

    serde_json::to_string(&serde_json::json!({
        "ecdcDescriptor": options.ecdc_descriptor,
        "tracks": tracks,
        "trackGaps": track_gaps,
    }))
    .context("failed to serialize ECDC programme")
}

fn fixed_context_samples_for_bundle(
    segment_samples: usize,
    segment_stride: usize,
) -> Result<usize> {
    if (segment_samples == 64_960 && segment_stride == 64_000)
        || (segment_samples == 87_360 && segment_stride == 86_400)
    {
        return Ok(FIXED_CONTEXT_SAMPLES);
    }
    bail!(
        "Unsupported fixed ECDC geometry: segment_samples={}, segment_stride={}. Expected 64960/64000 or 87360/86400.",
        segment_samples,
        segment_stride
    );
}

fn client_segment_starts(total_samples: usize, stride: usize) -> Vec<usize> {
    let safe_stride = stride.max(1);
    let mut starts = Vec::new();
    let mut offset = 0usize;
    while offset < total_samples {
        starts.push(offset);
        offset = offset.saturating_add(safe_stride);
    }
    starts
}

fn programme_map_to_json(map: &record_core::ProgrammeMap) -> serde_json::Value {
    let sample_rate = f64::from(map.sample_rate).max(1.0);
    let ms = |samples: u64| (samples as f64) * 1000.0 / sample_rate;

    let mut tracks = Vec::new();
    let mut gaps = Vec::new();
    for region in &map.regions {
        match &region.kind {
            record_core::ProgrammeRegionKind::Track { number, title } => {
                tracks.push(json!({
                    "number": number,
                    "title": title,
                    "startSample": region.start_sample,
                    "endSample": region.end_sample,
                }));
            }
            record_core::ProgrammeRegionKind::Gap { after_track_number } => {
                gaps.push(json!({
                    "afterTrackNumber": after_track_number,
                    "startSample": region.start_sample,
                    "endSample": region.end_sample,
                    "sampleCount": region.sample_count,
                    "durationMs": ms(region.sample_count),
                    "radialStartNormalized": region.radial_start_normalized,
                    "radialEndNormalized": region.radial_end_normalized,
                }));
            }
        }
    }

    json!({
        "sampleRate": map.sample_rate,
        "channels": map.channels,
        "totalSamples": map.total_samples,
        "durationMs": ms(map.total_samples),
        "tracks": tracks,
        "gaps": gaps,
    })
}

fn decode_record_metadata_json(png_bytes: &[u8]) -> Result<String> {
    let decoded = decode_record_png_resolving_profile(png_bytes, None)
        .context("failed to decode Bitneedle record PNG")?;

    let stream = record_core::parse_chunk_stream(&decoded.chunk_stream.bytes)
        .context("failed to parse decoded chunk stream")?;

    let ranges = record_core::chunk_all_ranges(&decoded.chunk_stream.bytes)
        .context("failed to derive chunk ranges")?;

    let payload = record_core::chunk_stream_payload_bytes(&stream);

    let chunks = stream
        .chunks
        .iter()
        .zip(ranges.iter())
        .enumerate()
        .map(|(index, (chunk, range))| {
            let payload_bytes = decoded
                .chunk_stream
                .bytes
                .get(range.payload_start..range.payload_end)
                .unwrap_or(&[]);

            json!({
                "index": index,
                "byteOffset": range.chunk_start,
                "byteLength": range.chunk_end - range.chunk_start,
                "byteEnd": range.chunk_end,
                "payloadOffset": range.payload_start,
                "payloadByteLength": range.payload_end - range.payload_start,
                "payloadEnd": range.payload_end,
                "payloadSha256": sha256_base64url(payload_bytes),
                "crc32": chunk.crc32,
                "crc32Hex": format!("0x{:08x}", chunk.crc32),
            })
        })
        .collect::<Vec<_>>();

    let descriptor = &decoded.descriptor;
    let signed_release_reference = descriptor
        .signed_release_reference
        .as_ref()
        .map(|reference| {
            json!({
                "version": reference.version,
                "releaseCommitmentSha256Base64url": base64_url_encode(&reference.release_commitment_sha256),
                "keyIdBase64url": base64_url_encode(&reference.key_id),
                "signatureBase64url": base64_url_encode(&reference.signature),
            })
        });

    let result = json!({
        "record": {
            "recordProfile": decoded.record_profile,
            "pngByteLength": png_bytes.len(),
            "pngSha256": sha256_base64url(png_bytes),
            "chunkStreamPixelCount": decoded.chunk_stream.pixel_count,
        },
        "descriptor": {
            "version": descriptor.version,
            "checksumProtected": descriptor.checksum_protected,
            "bValue": descriptor.b_value(),
            "recordProfile": descriptor.record_profile,
            "streamByteLength": descriptor.stream_byte_length,
            "payloadEncoding": descriptor.payload_encoding,
            "title": descriptor.title,
            "artist": descriptor.artist,
            "releaseId": descriptor.release_id.map(record_descriptor::release_id_to_text),
            "catalogNumber": descriptor.catalog_number,
            "label": descriptor.label,
            "copyrightYear": descriptor.copyright_year,
            "copyrightHolder": descriptor.copyright_holder,
            "artworkCredit": descriptor.artwork_credit,
            "canonicalUrl": descriptor.canonical_url,
            "createdAt": descriptor.created_at,
            "signedReleaseReference": signed_release_reference,
            "bscPointerBase64url": descriptor
                .bsc_pointer
                .as_ref()
                .map(|bytes| base64_url_encode(bytes)),
            "toneSpans": descriptor.tone_spans,
            "signed": descriptor.signed_release_reference.is_some(),
        },
        "chunkStream": {
            "magic": String::from_utf8_lossy(record_core::RECORD_STREAM_MAGIC).to_string(),
            "byteLength": decoded.chunk_stream.bytes.len(),
            "sha256": sha256_base64url(&decoded.chunk_stream.bytes),
            "metadataByteLength": stream.metadata_bytes.len(),
            "metadata": stream.metadata,
            "chunkCount": stream.chunks.len(),
            "payloadByteLength": payload.len(),
            "payloadSha256": sha256_base64url(&payload),
            "chunks": chunks,
        }
    });

    serde_json::to_string(&result).context("failed to serialize decoded metadata JSON")
}

fn decode_record_header_json(png_bytes: &[u8], record_profile: Option<&str>) -> Result<String> {
    let decoded = decode_record_png_resolving_profile(png_bytes, record_profile)
        .context("failed to decode Bitneedle record PNG")?;

    let stream = record_core::parse_chunk_stream(&decoded.chunk_stream.bytes)
        .context("failed to parse decoded chunk stream")?;
    let payload = record_core::chunk_stream_payload_bytes(&stream);
    let stream_metadata = stream.metadata.clone();
    let descriptor = &decoded.descriptor;

    let mut value =
        serde_json::to_value(descriptor).context("failed to serialize decoded descriptor")?;
    if let Some(object) = value.as_object_mut() {
        object.insert("version".to_string(), json!(descriptor.version));
        object.insert(
            "checksumProtected".to_string(),
            json!(descriptor.checksum_protected),
        );
        object.insert("recordProfile".to_string(), json!(decoded.record_profile));
        object.insert(
            "streamByteLength".to_string(),
            json!(decoded.chunk_stream.bytes.len()),
        );
        object.insert("payloadByteLength".to_string(), json!(payload.len()));
        object.insert(
            "payloadEncoding".to_string(),
            json!(descriptor.payload_encoding),
        );
        object.insert(
            "descriptorMagic".to_string(),
            json!(record_descriptor_magic()),
        );
        object.insert(
            "chunkStreamMagic".to_string(),
            json!(String::from_utf8_lossy(record_core::RECORD_STREAM_MAGIC).to_string()),
        );
        object.insert(
            "chunkStream".to_string(),
            json!({
                "byteLength": decoded.chunk_stream.bytes.len(),
                "metadataByteLength": stream.metadata_bytes.len(),
                "metadata": stream_metadata,
                "chunkCount": stream.chunks.len(),
                "payloadByteLength": payload.len(),
                "payloadSha256": sha256_base64url(&payload),
            }),
        );
    }

    serde_json::to_string(&value).context("failed to serialize decoded header JSON")
}

fn decode_record_descriptor_header_json(
    png_bytes: &[u8],
    record_profile: Option<&str>,
) -> Result<String> {
    let (resolved_profile, descriptor) =
        decode_record_descriptor_resolving_profile(png_bytes, record_profile)
            .context("failed to decode Bitneedle record descriptor")?;

    let mut value =
        serde_json::to_value(&descriptor).context("failed to serialize decoded descriptor")?;
    if let Some(object) = value.as_object_mut() {
        object.insert("version".to_string(), json!(descriptor.version));
        object.insert(
            "checksumProtected".to_string(),
            json!(descriptor.checksum_protected),
        );
        object.insert("recordProfile".to_string(), json!(resolved_profile));
        object.insert(
            "streamByteLength".to_string(),
            json!(descriptor.stream_byte_length),
        );
        object.insert(
            "payloadEncoding".to_string(),
            json!(descriptor.payload_encoding),
        );
        object.insert(
            "descriptorMagic".to_string(),
            json!(record_descriptor_magic()),
        );
        object.insert(
            "chunkStreamMagic".to_string(),
            json!(String::from_utf8_lossy(record_core::RECORD_STREAM_MAGIC).to_string()),
        );
    }

    serde_json::to_string(&value).context("failed to serialize decoded descriptor header JSON")
}

fn decode_record_png_payload_bytes(png_bytes: &[u8]) -> Result<Vec<u8>> {
    let decoded = decode_record_png_resolving_profile(png_bytes, None)
        .context("failed to decode Bitneedle record PNG")?;
    decoded_chunk_stream_payload_bytes(&decoded.chunk_stream.bytes)
}

fn decode_record_png_payload_bytes_for_profile(
    png_bytes: &[u8],
    record_profile: &str,
) -> Result<Vec<u8>> {
    let decoded =
        record_decode::decode_record_png_to_chunk_stream_for_profile(png_bytes, record_profile)
            .context("failed to decode Bitneedle record PNG for profile")?;
    decoded_chunk_stream_payload_bytes(&decoded.bytes)
}

fn decode_record_png_to_payload(png_bytes: &[u8]) -> Result<WasmPayloadDecodeResult> {
    let decoded = decode_record_png_resolving_profile(png_bytes, None)
        .context("failed to decode Bitneedle record PNG")?;
    decoded_payload_result_from_chunk_stream_bytes(&decoded.chunk_stream.bytes)
}

fn decode_record_png_to_payload_with_length(
    png_bytes: &[u8],
    byte_length: usize,
) -> Result<WasmPayloadDecodeResult> {
    let decode_with_length = || -> Result<WasmPayloadDecodeResult> {
        let (profile, _descriptor) = decode_record_descriptor_resolving_profile(png_bytes, None)
            .context("failed to decode Bitneedle record descriptor")?;
        let decoded = record_decode::decode_record_png_to_chunk_stream_for_profile_with_length(
            png_bytes,
            &profile,
            Some(byte_length),
        )
        .context("failed to decode Bitneedle record PNG with explicit byte length")?;
        decoded_payload_result_from_chunk_stream_bytes(&decoded.bytes)
    };

    decode_with_length().or_else(|first_error| {
        decode_record_png_to_payload(png_bytes).with_context(|| {
            format!(
                "explicit byte length decode failed ({first_error:#}); retry without explicit length also failed"
            )
        })
    })
}

fn decode_record_png_to_payload_for_profile(
    png_bytes: &[u8],
    record_profile: &str,
) -> Result<WasmPayloadDecodeResult> {
    let decoded =
        record_decode::decode_record_png_to_chunk_stream_for_profile(png_bytes, record_profile)
            .context("failed to decode Bitneedle record PNG for profile")?;
    decoded_payload_result_from_chunk_stream_bytes(&decoded.bytes)
}

fn decode_record_png_to_payload_for_profile_with_length(
    png_bytes: &[u8],
    record_profile: &str,
    byte_length: usize,
) -> Result<WasmPayloadDecodeResult> {
    let decode_with_length = || -> Result<WasmPayloadDecodeResult> {
        let decoded = record_decode::decode_record_png_to_chunk_stream_for_profile_with_length(
            png_bytes,
            record_profile,
            Some(byte_length),
        )
        .context("failed to decode Bitneedle record PNG for profile with explicit byte length")?;
        decoded_payload_result_from_chunk_stream_bytes(&decoded.bytes)
    };

    decode_with_length().or_else(|first_error| {
        decode_record_png_to_payload_for_profile(png_bytes, record_profile).with_context(|| {
            format!(
                "explicit byte length decode failed ({first_error:#}); retry without explicit length also failed"
            )
        })
    })
}

#[derive(Debug, Clone)]
struct RecordPngContext {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
    record_profile: String,
    descriptor: record_descriptor::RecordDescriptor,
}

fn optional_record_profile(record_profile: Option<&str>) -> Option<&str> {
    record_profile
        .map(str::trim)
        .filter(|profile| !profile.is_empty() && !profile.eq_ignore_ascii_case("auto"))
}

fn record_profile_candidates(record_profile: Option<&str>) -> Result<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();

    if let Some(profile) = optional_record_profile(record_profile) {
        let normalized = record_core::normalize_record_profile_name(profile)?;
        seen.insert(normalized.clone());
        candidates.push(normalized);
    }

    for &profile in record_core::known_record_profile_names() {
        let normalized = record_core::normalize_record_profile_name(profile)?;
        if seen.insert(normalized.clone()) {
            candidates.push(normalized);
        }
    }

    Ok(candidates)
}

fn decode_record_descriptor_resolving_profile(
    png_bytes: &[u8],
    record_profile: Option<&str>,
) -> Result<(String, record_descriptor::RecordDescriptor)> {
    let mut failures = Vec::new();

    for profile in record_profile_candidates(record_profile)? {
        match record_decode::decode_record_descriptor_from_png(png_bytes, Some(&profile)) {
            Ok((normalized_profile, descriptor)) => return Ok((normalized_profile, descriptor)),
            Err(error) => failures.push(format!("{profile}: {error:#}")),
        }
    }

    bail!(
        "failed to decode record descriptor for known profiles; tried {}",
        failures.join(" | ")
    )
}

fn decode_record_png_resolving_profile(
    png_bytes: &[u8],
    record_profile: Option<&str>,
) -> Result<record_decode::DecodedRecord> {
    let (normalized_profile, descriptor) =
        decode_record_descriptor_resolving_profile(png_bytes, record_profile)?;
    let chunk_stream = record_decode::decode_record_png_to_chunk_stream_for_profile_with_length(
        png_bytes,
        &normalized_profile,
        Some(descriptor.stream_byte_length),
    )
    .context("failed to decode Bitneedle record chunk stream")?;

    Ok(record_decode::DecodedRecord {
        record_profile: normalized_profile,
        descriptor,
        chunk_stream,
    })
}

fn load_png_rgba(png_bytes: &[u8]) -> Result<(usize, usize, Vec<u8>)> {
    let image = image::load_from_memory(png_bytes)
        .context("failed to decode record PNG")?
        .to_rgba8();
    let (width, height) = image.dimensions();
    Ok((width as usize, height as usize, image.into_raw()))
}

fn decode_record_png_context(
    png_bytes: &[u8],
    record_profile: Option<&str>,
) -> Result<RecordPngContext> {
    let (width, height, rgba) = load_png_rgba(png_bytes)?;
    let (record_profile, descriptor) =
        decode_record_descriptor_resolving_profile(png_bytes, record_profile)?;

    Ok(RecordPngContext {
        width,
        height,
        rgba,
        record_profile,
        descriptor,
    })
}

fn write_rgba_png(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>> {
    if rgba.len() != width.saturating_mul(height).saturating_mul(4) {
        bail!("RGBA buffer length does not match width * height * 4");
    }

    let mut out = Vec::new();
    let encoder =
        PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::NoFilter);
    encoder
        .write_image(rgba, width as u32, height as u32, ExtendedColorType::Rgba8)
        .context("failed to encode RGBA PNG")?;
    Ok(out)
}

fn smallest_even_square_side(pixel_count: usize) -> usize {
    let mut side = (pixel_count as f64).sqrt().ceil() as usize;
    side = side.max(2);
    if side % 2 == 1 {
        side += 1;
    }
    while side.saturating_mul(side) < pixel_count {
        side += 2;
    }
    while side > 2 {
        let previous = side - 2;
        if previous.saturating_mul(previous) < pixel_count {
            break;
        }
        side = previous;
    }
    side
}

fn bytes_to_rgb_block_png(bytes: &[u8]) -> Result<Vec<u8>> {
    let real_pixel_count = bytes.len().div_ceil(3);
    let size = smallest_even_square_side(real_pixel_count);
    let mut rgba = vec![0_u8; size * size * 4];

    for pixel_index in 0..real_pixel_count {
        let rgb_offset = pixel_index * 3;
        let rgba_offset = pixel_index * 4;
        rgba[rgba_offset] = bytes.get(rgb_offset).copied().unwrap_or(0);
        rgba[rgba_offset + 1] = bytes.get(rgb_offset + 1).copied().unwrap_or(0);
        rgba[rgba_offset + 2] = bytes.get(rgb_offset + 2).copied().unwrap_or(0);
        rgba[rgba_offset + 3] = 255;
    }

    write_rgba_png(size, size, &rgba)
}

fn record_png_to_rgb_color_block_png(
    png_bytes: &[u8],
    record_profile: Option<&str>,
) -> Result<Vec<u8>> {
    let profile = optional_record_profile(record_profile);
    let payload = if let Some(profile) = profile {
        decode_record_png_payload_bytes_for_profile(png_bytes, profile)?
    } else {
        decode_record_png_payload_bytes(png_bytes)?
    };
    bytes_to_rgb_block_png(&payload)
}

fn sidecar_label_inner_radius(geometry: &record_core::RecordProfileGeometry) -> i32 {
    record_core::label_sidecar_inner_radius_from_profile_geometry(geometry)
}

fn sidecar_label_outer_radius(geometry: &record_core::RecordProfileGeometry) -> i32 {
    record_core::label_sidecar_outer_radius_from_profile_geometry(geometry)
}

fn sidecar_lead_in_outer_radius(geometry: &record_core::RecordProfileGeometry) -> i32 {
    (geometry.outer_radius - record_core::HEADER_SPIRAL_OUTER_EDGE_INSET)
        .max(geometry.payload_outer_radius + 1)
}

fn build_sidecar_protected_metadata_pixels(
    width: usize,
    height: usize,
    record_profile: &str,
) -> Result<Vec<bool>> {
    let mut protected = vec![false; width * height];
    for pixel_index in
        record_core::build_header_spiral_indices(width, height, record_profile, None, None, None)?
    {
        protected[pixel_index] = true;
    }
    for pixel_index in
        record_core::build_trailer_spiral_indices(width, height, record_profile, None, None, None)?
    {
        protected[pixel_index] = true;
    }
    Ok(protected)
}

#[derive(Debug, Clone, Copy)]
struct SidecarCarrierRegions {
    label: bool,
    payload_intergroove: bool,
    lead_in_deadwax: bool,
}

fn sidecar_pixel_in_carrier_regions(
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    geometry: &record_core::RecordProfileGeometry,
    payload_intergroove_is_available: bool,
    regions: SidecarCarrierRegions,
) -> bool {
    let center_x = width as f64 / 2.0;
    let center_y = height as f64 / 2.0;
    let dx = x as f64 + 0.5 - center_x;
    let dy = y as f64 + 0.5 - center_y;
    let distance = (dx * dx + dy * dy).sqrt();
    let label_outer_radius = sidecar_label_outer_radius(geometry) as f64;
    let in_label = regions.label
        && distance > sidecar_label_inner_radius(geometry) as f64
        && distance < label_outer_radius;
    let in_intergroove = regions.payload_intergroove
        && distance > geometry.payload_inner_radius as f64
        && distance < geometry.payload_outer_radius as f64
        && payload_intergroove_is_available;
    let in_lead_in = regions.lead_in_deadwax
        && payload_intergroove_is_available
        && distance > geometry.payload_outer_radius as f64
        && distance < sidecar_lead_in_outer_radius(geometry) as f64;
    let in_deadwax = regions.lead_in_deadwax
        && payload_intergroove_is_available
        && distance > label_outer_radius
        && distance < geometry.payload_inner_radius as f64;
    in_label || in_intergroove || in_lead_in || in_deadwax
}

// Text-avoid geometry was formerly carried in arbitrary BRD1 JSON metadata.
// The compact descriptor no longer carries that blob, so sidecar ordering is
// now independent of label compositor hints.
#[derive(Clone)]
struct TextAvoidSpec;

fn text_avoid_spec_from_descriptor(
    _descriptor: &record_descriptor::RecordDescriptor,
) -> Option<TextAvoidSpec> {
    None
}

fn apply_text_avoid_spec(
    _protected: &mut [bool],
    _width: usize,
    _height: usize,
    _text_avoid: Option<&TextAvoidSpec>,
) {
}

fn build_sidecar_carrier_region_pairs(
    width: usize,
    height: usize,
    b_value: f64,
    record_profile: &str,
    regions: SidecarCarrierRegions,
    seed: u32,
    text_avoid: Option<&TextAvoidSpec>,
) -> Result<Vec<(usize, usize)>> {
    let geometry = record_core::describe_record_profile(record_profile)?;
    let mask = if regions.payload_intergroove || regions.lead_in_deadwax {
        Some(record_core::build_spiral_mask(
            width,
            height,
            b_value,
            &geometry.record_profile,
            None,
            None,
            None,
        )?)
    } else {
        None
    };
    let mut protected =
        build_sidecar_protected_metadata_pixels(width, height, &geometry.record_profile)?;
    apply_text_avoid_spec(&mut protected, width, height, text_avoid);
    let mut pairs = Vec::new();

    for y in 0..height {
        let mut x = 0usize;
        while x + 1 < width {
            let first = y * width + x;
            let second = first + 1;
            if protected[first] || protected[second] {
                x += 1;
                continue;
            }

            let first_payload_intergroove_available = mask
                .as_ref()
                .map(|mask| mask.kinds[first] == 0)
                .unwrap_or(false);
            let second_payload_intergroove_available = mask
                .as_ref()
                .map(|mask| mask.kinds[second] == 0)
                .unwrap_or(false);

            let first_ok = sidecar_pixel_in_carrier_regions(
                x,
                y,
                width,
                height,
                &geometry,
                first_payload_intergroove_available,
                regions,
            );
            let second_ok = sidecar_pixel_in_carrier_regions(
                x + 1,
                y,
                width,
                height,
                &geometry,
                second_payload_intergroove_available,
                regions,
            );

            if first_ok && second_ok {
                pairs.push((first, second));
                x += 2;
            } else {
                x += 1;
            }
        }
    }

    record_sidecar::shuffle_pairs_mulberry32(&mut pairs, seed);
    Ok(pairs)
}

fn build_sidecar_carrier_pairs(
    width: usize,
    height: usize,
    b_value: f64,
    record_profile: &str,
    carriers: &[record_sidecar::SidecarCarrier],
    seed: u32,
    text_avoid: Option<&TextAvoidSpec>,
) -> Result<Vec<(usize, usize)>> {
    build_sidecar_carrier_region_pairs(
        width,
        height,
        b_value,
        record_profile,
        SidecarCarrierRegions {
            label: carriers.contains(&record_sidecar::SidecarCarrier::Label),
            payload_intergroove: carriers.contains(&record_sidecar::SidecarCarrier::Intergroove),
            lead_in_deadwax: carriers.contains(&record_sidecar::SidecarCarrier::LeadInDeadwax),
        },
        seed,
        text_avoid,
    )
}

fn sidecar_pointer_from_descriptor(
    descriptor: &record_descriptor::RecordDescriptor,
) -> Result<Option<record_sidecar::SidecarHeaderPointer>> {
    descriptor
        .bsc_pointer
        .as_deref()
        .map(record_sidecar::decode_sidecar_header_pointer)
        .transpose()
}

fn decode_record_png_sidecar_to_bytes(
    png_bytes: &[u8],
    record_profile: Option<&str>,
) -> Result<Vec<u8>> {
    decode_record_png_sidecar_with_context(png_bytes, record_profile).map(|(bytes, _)| bytes)
}

fn decode_record_png_sidecar(
    png_bytes: &[u8],
    record_profile: Option<&str>,
) -> Result<record_sidecar::SidecarDecodeResult> {
    let (_bytes, result) = decode_record_png_sidecar_with_context(png_bytes, record_profile)?;
    Ok(result)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LabelThumbnailPatchManifest {
    version: u32,
    role: String,
    profile_name: Option<String>,
    width: usize,
    height: usize,
    base: Option<String>,
    patch_item_name: String,
    patch_mime: Option<String>,
}

fn find_label_thumbnail_manifest(
    decoded: &record_sidecar::SidecarDecodedItems,
) -> Result<LabelThumbnailPatchManifest> {
    let item = decoded
        .items
        .iter()
        .find(|item| item.name == "bitneedle-label-thumbnail-v1.json")
        .context("record has no embedded label thumbnail patch manifest")?;
    let json = item
        .json
        .as_ref()
        .context("label thumbnail patch manifest item is not JSON")?;
    let manifest: LabelThumbnailPatchManifest = serde_json::from_value(json.clone())
        .context("label thumbnail patch manifest is invalid")?;
    if manifest.version != 1 {
        bail!(
            "unsupported label thumbnail patch manifest version {}",
            manifest.version
        );
    }
    if manifest.role != "label-thumbnail-patch" {
        bail!("label thumbnail patch manifest role is invalid");
    }
    if manifest.width == 0 || manifest.height == 0 || manifest.width != manifest.height {
        bail!("label thumbnail patch manifest dimensions are invalid");
    }
    Ok(manifest)
}

fn find_label_thumbnail_patch_bytes(
    decoded: &record_sidecar::SidecarDecodedItems,
    manifest: &LabelThumbnailPatchManifest,
) -> Result<(Vec<u8>, String)> {
    let item = decoded
        .items
        .iter()
        .find(|item| item.name == manifest.patch_item_name)
        .with_context(|| {
            format!(
                "record has no embedded label thumbnail patch item {}",
                manifest.patch_item_name
            )
        })?;
    let mime = if !item.mime.is_empty() {
        item.mime.clone()
    } else {
        manifest
            .patch_mime
            .clone()
            .unwrap_or_else(|| "image/avif".to_string())
    };
    if !mime.eq_ignore_ascii_case("image/avif") {
        bail!("label thumbnail patch item MIME type must be image/avif");
    }
    let bytes = general_purpose::STANDARD
        .decode(&item.data_base64)
        .context("label thumbnail patch item data is not valid base64")?;
    Ok((bytes, mime))
}

/// Extracts the embedded label thumbnail and returns `(bytes, mime)`.
///
/// The thumbnail is stored as a self-contained AVIF (`full-label-thumbnail-avif`
/// base) — a downscaled render of the label. WASM cannot decode AVIF, so the
/// stored image is returned verbatim for the browser to decode and display.
fn extract_label_thumbnail_image(
    png_bytes: &[u8],
    record_profile: Option<&str>,
) -> Result<(Vec<u8>, String)> {
    let context = decode_record_png_context(png_bytes, record_profile)?;
    let (sidecar_bytes, _) = decode_record_png_sidecar_with_context(png_bytes, record_profile)?;
    let decoded = record_sidecar::decode_sidecar_container_items(&sidecar_bytes)?;
    let manifest = find_label_thumbnail_manifest(&decoded)?;
    if let Some(profile_name) = manifest.profile_name.as_deref() {
        let normalized = record_core::normalize_record_profile_name(profile_name)?;
        if normalized != context.record_profile {
            bail!(
                "label thumbnail patch profile {} does not match record profile {}",
                normalized,
                context.record_profile
            );
        }
    }
    if manifest.base.as_deref() != Some("full-label-thumbnail-avif") {
        bail!("label thumbnail base must be full-label-thumbnail-avif");
    }
    let (patch_bytes, patch_mime) = find_label_thumbnail_patch_bytes(&decoded, &manifest)?;
    if !patch_mime.eq_ignore_ascii_case("image/avif") {
        bail!("label thumbnail must be image/avif");
    }
    Ok((patch_bytes, patch_mime))
}

fn decode_record_png_sidecar_with_context(
    png_bytes: &[u8],
    record_profile: Option<&str>,
) -> Result<(Vec<u8>, record_sidecar::SidecarDecodeResult)> {
    let context = decode_record_png_context(png_bytes, record_profile)?;
    let pointer = sidecar_pointer_from_descriptor(&context.descriptor)?;
    let scheme = pointer
        .as_ref()
        .map(|pointer| pointer.scheme.clone())
        .unwrap_or_else(|| record_sidecar::SIDECAR_SCHEME_PAIRSIGN_SAFE_LUMA_V2.to_string());
    let carriers = pointer
        .as_ref()
        .map(|pointer| pointer.carriers.clone())
        .unwrap_or_else(record_sidecar::default_sidecar_carriers);
    let seed = pointer
        .as_ref()
        .map(|pointer| pointer.seed)
        .unwrap_or(record_sidecar::SIDECAR_DEFAULT_SEED);
    let text_avoid = text_avoid_spec_from_descriptor(&context.descriptor);
    let carrier_pairs = build_sidecar_carrier_pairs(
        context.width,
        context.height,
        context.descriptor.b_value(),
        &context.record_profile,
        &carriers,
        seed,
        text_avoid.as_ref(),
    )?;
    let capacity_bytes =
        record_sidecar::sidecar_capacity_bytes_for_scheme(&scheme, carrier_pairs.len())?;
    let length = if let Some(pointer) = pointer.as_ref() {
        pointer.length
    } else {
        let prefix = record_sidecar::decode_pairsign_sidecar_bytes_from_pairs(
            &context.rgba,
            &carrier_pairs,
            &scheme,
            12,
        )?;
        if prefix.len() < 12 || &prefix[..4] != record_sidecar::SIDECAR_MAGIC {
            bail!("record does not contain a typed sidecar stream");
        }
        u32::from_be_bytes(prefix[8..12].try_into().expect("slice length")) as usize
    };

    if length > capacity_bytes {
        bail!(
            "sidecar stream length {} exceeds carrier capacity {}",
            length,
            capacity_bytes
        );
    }

    let (bts1, result) =
        record_sidecar::decode_sidecar_from_pairs(&context.rgba, &carrier_pairs, &scheme, length)?;

    if let Some(pointer) = pointer.as_ref() {
        let actual: [u8; 32] = Sha256::digest(&bts1).into();
        if actual != pointer.sha256_bytes {
            bail!("record sidecar SHA-256 does not match header pointer");
        }
    }

    Ok((bts1, result))
}

fn decoded_chunk_stream_payload_bytes(chunk_stream_bytes: &[u8]) -> Result<Vec<u8>> {
    let stream = record_core::parse_chunk_stream(chunk_stream_bytes)
        .context("decoded groove is not a valid BCS2 chunk stream")?;
    // The spiral chunks are fragments of a single ECDC (header + per-spiral
    // frames); reassemble by plain concatenation back into that one ECDC. Track
    // titles/boundaries ride along in the BRS1 `trackListing` metadata.
    Ok(record_core::chunk_stream_payload_bytes(&stream))
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct SilenceSpan {
    /// Number of ECDC payload entries (revolutions) that precede this silence in
    /// the decodable, GAP-excluded payload. The decode worker positions the
    /// zero-PCM splice at `afterEntryIndex * samplesPerEntry`.
    after_entry_index: usize,
    sample_count: usize,
}

/// Reconstruct a record stream's decodable payload as a back-to-back stream of
/// standalone ECDC objects, excluding any GAP-container entries (see
/// `PAYLOAD_CONTAINER_GAP`) — those carry a canonical GAP1 payload, not codec
/// data, and must never reach the EnCodec decoder.
///
/// The programme format stores one canonical ECDC descriptor in BRS1 metadata
/// plus many *headerless* ECDC payload entries — one discrete EnCodec frame per
/// entry. The decoder (`player-wasm`'s `lmEcdcDecodeChunks`) walks the payload
/// object-by-object: it reads an `ECDC` header, consumes that object's frame(s)
/// up to the next `ECDC` magic, advances the global sample offset by the
/// header's `audio_length`, and repeats. So every entry must be wrapped in its
/// *own* standalone ECDC header (`al == block_samples`, the per-frame sample
/// count) and the resulting objects concatenated in programme order. Handing the
/// decoder a single header followed by raw frames collapses all but the first
/// frame to zero samples. Header reconstruction is delegated to
/// `record_core::ecdc::payload_to_standalone_ecdc`; no ECDC framing is rebuilt
/// here.
///
/// Returns the concatenated standalone-ECDC payload plus an ordered list of
/// silence spans describing where, in the decodable (GAP-excluded) ECDC entry
/// timeline, each excluded GAP run belongs and how many PCM samples it
/// represents. The sample count is read authoritatively from each GAP1 header,
/// never from descriptor geometry.
fn ecdc_only_payload_bytes_with_silence_map(
    stream: &record_core::RecordStream,
) -> Result<(Vec<u8>, Vec<SilenceSpan>)> {
    let resolved_entries = record_core::resolve_payload_entries(
        &stream.metadata.payload_entries,
        stream.metadata.payload_descriptors.len(),
    )
    .context("failed to resolve BRS1 payload entries")?;

    // A gap is an ECDC entry that no musical track covers. Build the set of
    // track-covered entry indexes so untracked entries can be excluded from the
    // decodable timeline and spliced as exact zero PCM (the default gap policy),
    // even though their stored bytes are valid, decodable ECDC ambience.
    let mut entry_is_tracked = vec![false; resolved_entries.len()];
    for track in &stream.metadata.tracks {
        let start = track.first_revolution_index;
        let end = start.saturating_add(track.revolution_count);
        for slot in entry_is_tracked
            .iter_mut()
            .take(end.min(resolved_entries.len()))
            .skip(start)
        {
            *slot = true;
        }
    }

    let record_payload = record_core::record_stream_payload_bytes(stream);

    // Concatenated standalone ECDC objects, one per ECDC payload entry, in
    // programme order.
    let mut payload = Vec::with_capacity(record_payload.len());
    let mut silence_spans = Vec::<SilenceSpan>::new();
    let mut ecdc_entry_index = 0usize;

    for (entry_index, entry) in resolved_entries.iter().enumerate() {
        let descriptor = stream
            .metadata
            .payload_descriptors
            .get(entry.payload_descriptor_index as usize)
            .with_context(|| {
                format!(
                    "payload entry {entry_index} references descriptor index {}",
                    entry.payload_descriptor_index
                )
            })?;

        let end = entry
            .byte_offset
            .checked_add(entry.byte_length)
            .context("payload entry byte range overflow")?;

        let entry_bytes = record_payload
            .get(entry.byte_offset..end)
            .with_context(|| {
                format!(
                    "payload entry {entry_index} byte range {}..{} exceeds reconstructed payload length {}",
                    entry.byte_offset,
                    end,
                    record_payload.len()
                )
            })?;

        if !descriptor
            .container
            .eq_ignore_ascii_case(record_core::PAYLOAD_CONTAINER_ECDC)
        {
            bail!(
                "payload entry {entry_index} uses unsupported decoder container {}",
                descriptor.container
            );
        }

        // Untracked ECDC entries are inter-track gaps: exclude their (valid,
        // decodable) ECDC ambience from the decoded timeline and record a
        // silence span so the player splices exact zero PCM of the entry's real
        // ECDC duration. Diagnostic playback that wants to audition the ambience
        // can decode the stored bytes directly instead.
        if !entry_is_tracked.get(entry_index).copied().unwrap_or(false) {
            let sample_count =
                record_core::ecdc::headerless_entry_sample_count(entry_bytes, descriptor)
                    .with_context(|| {
                        format!(
                            "failed to read ECDC sample count for gap payload entry {entry_index}"
                        )
                    })?;

            let sample_count =
                usize::try_from(sample_count).context("gap sample count exceeds usize")?;

            match silence_spans.last_mut() {
                Some(last) if last.after_entry_index == ecdc_entry_index => {
                    last.sample_count = last
                        .sample_count
                        .checked_add(sample_count)
                        .context("combined GAP sample count overflow")?;
                }
                _ => silence_spans.push(SilenceSpan {
                    after_entry_index: ecdc_entry_index,
                    sample_count,
                }),
            };

            continue;
        }

        // Wrap this discrete frame in its own standalone ECDC object (`al` =
        // the descriptor's per-frame `block_samples`) and append it. The decoder
        // splits the concatenated objects on the `ECDC` magic boundary and sums
        // each object's `audio_length` into a continuous global timeline.
        let object = record_core::ecdc::payload_to_standalone_ecdc(descriptor, entry_bytes)
            .with_context(|| {
                format!(
                    "failed to reconstruct standalone ECDC object for payload entry {entry_index}"
                )
            })?;
        payload.extend_from_slice(&object);

        ecdc_entry_index = ecdc_entry_index
            .checked_add(1)
            .context("ECDC entry index overflow")?;
    }

    Ok((payload, silence_spans))
}

fn decoded_payload_result_from_chunk_stream_bytes(
    chunk_stream_bytes: &[u8],
) -> Result<WasmPayloadDecodeResult> {
    let stream = record_core::parse_chunk_stream(chunk_stream_bytes)
        .context("decoded groove is not a valid BCS2 chunk stream")?;
    let (payload, silence_spans) = ecdc_only_payload_bytes_with_silence_map(&stream)?;
    let metadata_json = serde_json::to_string(&stream.metadata)
        .context("failed to serialize decoded BCS2 metadata JSON")?;
    let silence_map_json = serde_json::to_string(&silence_spans)
        .context("failed to serialize decoded silence map JSON")?;
    Ok(WasmPayloadDecodeResult {
        payload_bytes: payload,
        chunk_stream_bytes: chunk_stream_bytes.to_vec(),
        metadata_json,
        silence_map_json,
    })
}

fn cache_encryption_context_from_json(
    context_json: &str,
) -> Result<record_descriptor::CacheEncryptionContext> {
    let context: record_descriptor::CacheEncryptionContext =
        serde_json::from_str(context_json).context("cache encryption context JSON is invalid")?;
    Ok(context)
}

fn record_descriptor_from_json(
    descriptor_json: &str,
) -> Result<record_descriptor::RecordDescriptor> {
    let descriptor: record_descriptor::RecordDescriptor =
        serde_json::from_str(descriptor_json).context("record descriptor JSON is invalid")?;
    descriptor
        .validate_cache_encryption()
        .context("record descriptor cache encryption is invalid")?;
    Ok(descriptor)
}

fn encrypt_cache_entry(
    descriptor_json: &str,
    context_json: &str,
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let descriptor = record_descriptor_from_json(descriptor_json)?;
    let context = cache_encryption_context_from_json(context_json)?;
    record_descriptor::encrypt_cache_envelope(&descriptor, &context, plaintext)
}

fn decrypt_cache_entry(
    descriptor_json: &str,
    context_json: &str,
    envelope: &[u8],
) -> Result<Vec<u8>> {
    let descriptor = record_descriptor_from_json(descriptor_json)?;
    let context = cache_encryption_context_from_json(context_json)?;
    record_descriptor::decrypt_cache_envelope(&descriptor, &context, envelope)
}

fn sha256_base64url(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    base64_url_encode(&digest)
}

fn base64_url_encode(bytes: &[u8]) -> String {
    general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn record_descriptor_magic() -> String {
    String::from_utf8_lossy(record_descriptor::RECORD_DESCRIPTOR_MAGIC).to_string()
}

#[wasm_bindgen(js_name = initPanicHook)]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

fn to_js_error(error: impl std::fmt::Display + std::fmt::Debug) -> JsValue {
    JsValue::from_str(&format!("{error:#?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use record_cut::{
        encode_record_stream, PayloadDescriptorInput, PayloadEntryInput, RecordStreamInput,
        TrackInput,
    };

    // The compact two-track groove is the smallest fixture that exercises
    // payload entries, track listing, and the chunk-stream payload offsets
    // that `decode_record_metadata_json` (the function the WASM facade
    // exposes) and native `record_core::parse_chunk_stream` must agree on.
    // `record-render`/`record-cut` are dev-dependencies here purely to build
    // the test fixture PNG; this crate's own (non-test) code never renders.
    #[test]
    fn wasm_facade_decode_matches_native_decode() {
        let payload_one = vec![0xAAu8; 4_000];
        let payload_two = vec![0xBBu8; 5_500];

        let input = RecordStreamInput {
            payload_descriptors: vec![PayloadDescriptorInput::from_container("TEST")],
            tracks: vec![
                TrackInput {
                    title: "Side A".to_string(),
                    first_revolution_index: None,
                    revolution_count: None,
                },
                TrackInput {
                    title: "Side B".to_string(),
                    first_revolution_index: None,
                    revolution_count: None,
                },
            ],
            track_gaps: vec![],
        };
        let entries = vec![
            PayloadEntryInput::already_chunked(0, payload_one),
            PayloadEntryInput::already_chunked(0, payload_two),
        ];
        let stream = encode_record_stream(&input, &entries).unwrap();
        let render_options = serde_json::json!({
            "headerTitle": "Prepared",
            "headerArtist": "Fixture",
            "headerReleaseId": "rel_014D2PF2DBSQQH081G81860W40",
        })
        .to_string();
        let output = record_render::render_chunk_stream_to_png(
            &stream,
            "single45",
            208.5,
            Some(&render_options),
        )
        .unwrap();

        let facade_json: serde_json::Value =
            serde_json::from_str(&decode_record_metadata_json(&output.png_bytes).unwrap()).unwrap();

        let decoded = decode_record_png_resolving_profile(&output.png_bytes, None).unwrap();
        let native_stream = record_core::parse_chunk_stream(&decoded.chunk_stream.bytes).unwrap();

        assert_eq!(
            facade_json["chunkStream"]["byteLength"].as_u64().unwrap() as usize,
            decoded.chunk_stream.bytes.len()
        );
        assert_eq!(
            facade_json["record"]["recordProfile"].as_str().unwrap(),
            decoded.record_profile
        );
        assert_eq!(
            facade_json["chunkStream"]["chunks"]
                .as_array()
                .unwrap()
                .len(),
            native_stream.chunks.len()
        );
        assert_eq!(
            facade_json["chunkStream"]["metadata"]["tracks"][0]["title"]
                .as_str()
                .unwrap(),
            native_stream.metadata.tracks[0].title
        );
        assert_eq!(
            facade_json["chunkStream"]["metadata"]["tracks"][1]["title"]
                .as_str()
                .unwrap(),
            native_stream.metadata.tracks[1].title
        );
    }

    #[test]
    fn build_multi_track_segment_budget_json_counts_track_and_gap_segments() {
        let budget: serde_json::Value = serde_json::from_str(
            &build_multi_track_segment_budget_json(
                &serde_json::json!({
                    "revolutionSamples": 64_000,
                    "sampleRate": 48_000,
                    "trackAudioLengths": [64_000, 96_001],
                    "gapAfterSeconds": [1.0, 0.0],
                })
                .to_string(),
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            budget["trackSegmentCounts"].as_array().unwrap(),
            &vec![serde_json::json!(1), serde_json::json!(2)]
        );
        assert_eq!(
            budget["gapSegmentCounts"].as_array().unwrap(),
            &vec![serde_json::json!(1), serde_json::json!(0)]
        );
        assert_eq!(
            budget["gapFrameLengths"].as_array().unwrap(),
            &vec![serde_json::json!(48_000), serde_json::json!(0)]
        );
        assert_eq!(budget["totalSegments"].as_u64().unwrap(), 4);
    }

    #[test]
    fn build_ecdc_programme_json_builds_payload_indexes_and_gaps() {
        let programme: serde_json::Value = serde_json::from_str(
            &build_ecdc_programme_json(
                &serde_json::json!({
                    "ecdcDescriptor": { "codec": "ECDC" },
                    "trackTitles": ["A", "B"],
                    "trackEntryCounts": [2, 3],
                    "gapEntryCounts": [1, 0],
                })
                .to_string(),
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            programme["tracks"][0]["payloadIndexes"].as_array().unwrap(),
            &vec![serde_json::json!(0), serde_json::json!(1)]
        );
        assert_eq!(
            programme["tracks"][1]["payloadIndexes"].as_array().unwrap(),
            &vec![
                serde_json::json!(3),
                serde_json::json!(4),
                serde_json::json!(5)
            ]
        );
        assert_eq!(
            programme["trackGaps"][0]["firstRevolutionIndex"]
                .as_u64()
                .unwrap(),
            2
        );
        assert_eq!(
            programme["trackGaps"][0]["revolutionCount"]
                .as_u64()
                .unwrap(),
            1
        );
        assert_eq!(
            programme["trackGaps"][0]["afterTrackIndex"]
                .as_u64()
                .unwrap(),
            0
        );
    }
}
