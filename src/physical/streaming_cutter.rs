//! Sequential, bounded-memory cutting for progressively decoded PCM.
//!
//! This module is for loading and authoring paths. Do not call it from an
//! audio render thread.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::groove::{
    begin_groove_lateral_content_hash, begin_groove_vertical_content_hash, encode_45_45,
    finalize_groove_content_identity, CutterHighpass, GrooveClearanceFramePair,
    GrooveContentHasher, GrooveContentIdentity, GrooveCutReport, GrooveError, GrooveLayout,
    GrooveSourceProvenance, RecordCutConfig, GROOVE_ASSET_FORMAT_VERSION,
};
use super::paged_groove::{
    GrooveFrameRange, GrooveGenerationId, PagedGrooveError, PhysicalGrooveCutMetadata,
    PhysicalGrooveMetadata, PhysicalGroovePage, MAX_PAGED_GROOVE_TRACING_HALO_FRAMES,
    PAGED_GROOVE_SPATIAL_STORAGE_MARGIN_FRAMES, PHYSICAL_GROOVE_SAMPLE_RATE_HZ,
};
use super::riaa::{RiaaConfig, RiaaFilterState, RiaaRecordFilter};
use crate::resampler::catmull_rom_sample;

pub const STREAMING_GROOVE_CUTTER_SNAPSHOT_VERSION: u32 = 1;
pub const STREAMING_GROOVE_PAGE_CHUNK_FORMAT_VERSION: u32 = 1;
pub const MAX_STREAMING_GROOVE_PAGE_CORE_FRAMES: u32 = 1_048_576;
// Catmull-Rom interpolation needs frames center-1 through center+2. The
// one-page pull API can stop while the next output is already ready, so a
// snapshot can retain all four support frames.
pub const MAX_STREAMING_GROOVE_SOURCE_HISTORY_FRAMES: u64 = 4;

const MAX_EXACT_FRAME_COUNT: u64 = 1_u64 << 53;

/// Configures one sequential PCM cut.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingGrooveCutterConfig {
    pub source_sample_rate_hz: f64,
    pub source_channel_count: u8,
    pub total_source_frame_count: u64,
    pub layout: GrooveLayout,
    pub cut: RecordCutConfig,
    pub page_core_frame_count: u32,
    pub tracing_halo_frames: u32,
}

impl StreamingGrooveCutterConfig {
    pub fn validate(self) -> Result<Self, StreamingGrooveCutterError> {
        let layout = self.layout.validate()?;
        if layout.groove_sample_rate_hz != f64::from(PHYSICAL_GROOVE_SAMPLE_RATE_HZ) {
            return Err(StreamingGrooveCutterError::UnsupportedGrooveSampleRate);
        }
        self.cut.validate(layout.groove_sample_rate_hz)?;
        if !self.source_sample_rate_hz.is_finite()
            || self.source_sample_rate_hz <= 0.0
            || self.source_sample_rate_hz > f64::from(PHYSICAL_GROOVE_SAMPLE_RATE_HZ)
        {
            return Err(StreamingGrooveCutterError::UnsupportedSourceSampleRate);
        }
        if !matches!(self.source_channel_count, 1 | 2) {
            return Err(StreamingGrooveCutterError::InvalidSourceChannelCount);
        }
        if self.total_source_frame_count < 4
            || self.total_source_frame_count > MAX_EXACT_FRAME_COUNT
        {
            return Err(StreamingGrooveCutterError::InvalidTotalSourceFrameCount);
        }
        if self.page_core_frame_count == 0
            || self.page_core_frame_count > MAX_STREAMING_GROOVE_PAGE_CORE_FRAMES
        {
            return Err(StreamingGrooveCutterError::InvalidPageCoreFrameCount);
        }
        if self.tracing_halo_frames == 0
            || self.tracing_halo_frames > MAX_PAGED_GROOVE_TRACING_HALO_FRAMES
        {
            return Err(StreamingGrooveCutterError::InvalidTracingHaloFrameCount);
        }
        let output_frame_count = expected_output_frame_count(self)?;
        if !(4..=MAX_EXACT_FRAME_COUNT).contains(&output_frame_count) {
            return Err(StreamingGrooveCutterError::InvalidOutputFrameCount);
        }
        Ok(self)
    }

    pub fn expected_output_frame_count(self) -> Result<u64, StreamingGrooveCutterError> {
        expected_output_frame_count(self.validate()?)
    }

    pub fn storage_halo_frames(self) -> Result<u32, StreamingGrooveCutterError> {
        self.tracing_halo_frames
            .checked_add(PAGED_GROOVE_SPATIAL_STORAGE_MARGIN_FRAMES)
            .ok_or(StreamingGrooveCutterError::InvalidTracingHaloFrameCount)
    }

    /// Returns the source-frame history bound for one validated cut.
    pub fn maximum_source_history_frame_count(self) -> u64 {
        MAX_STREAMING_GROOVE_SOURCE_HISTORY_FRAMES
    }

    /// Returns the one-revolution clearance-history bound.
    pub fn maximum_clearance_history_frame_count(self) -> Result<u64, StreamingGrooveCutterError> {
        let config = self.validate()?;
        let output_frame_count = expected_output_frame_count(config)?;
        let frames_per_revolution =
            config.layout.groove_sample_rate_hz * 60.0 / config.layout.nominal_rpm;
        if frames_per_revolution > output_frame_count.saturating_sub(1) as f64 {
            Ok(0)
        } else {
            Ok(frames_per_revolution.ceil() as u64 + 1)
        }
    }

    /// Returns the pending page-history bound.
    pub fn maximum_pending_page_frame_count(self) -> Result<u64, StreamingGrooveCutterError> {
        let config = self.validate()?;
        u64::from(config.page_core_frame_count)
            .checked_add(u64::from(config.storage_halo_frames()?).saturating_mul(2))
            .ok_or(StreamingGrooveCutterError::InvalidPageCoreFrameCount)
    }
}

/// Stores raw page data before the final metadata identity exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingGroovePageChunk {
    format_version: u32,
    total_frame_count: u64,
    storage_halo_frames: u32,
    core_start_frame: u64,
    core_end_frame_exclusive: u64,
    stored_start_frame: u64,
    stored_end_frame_exclusive: u64,
    lateral_displacement_m: Box<[f32]>,
    vertical_displacement_m: Box<[f32]>,
}

impl StreamingGroovePageChunk {
    pub fn format_version(&self) -> u32 {
        self.format_version
    }

    pub fn total_frame_count(&self) -> u64 {
        self.total_frame_count
    }

    pub fn storage_halo_frames(&self) -> u32 {
        self.storage_halo_frames
    }

    pub fn core_start_frame(&self) -> u64 {
        self.core_start_frame
    }

    pub fn core_end_frame_exclusive(&self) -> u64 {
        self.core_end_frame_exclusive
    }

    pub fn stored_start_frame(&self) -> u64 {
        self.stored_start_frame
    }

    pub fn stored_end_frame_exclusive(&self) -> u64 {
        self.stored_end_frame_exclusive
    }

    pub fn lateral_displacement_m(&self) -> &[f32] {
        &self.lateral_displacement_m
    }

    pub fn vertical_displacement_m(&self) -> &[f32] {
        &self.vertical_displacement_m
    }

    /// Adds the final metadata identity and validates the page.
    pub fn into_physical_page(
        self,
        metadata: PhysicalGrooveMetadata,
    ) -> Result<PhysicalGroovePage, StreamingGrooveCutterError> {
        self.validate()?;
        if metadata.total_frame_count() != self.total_frame_count
            || metadata.required_storage_halo_frames() != self.storage_halo_frames
        {
            return Err(StreamingGrooveCutterError::RawPageMetadataMismatch);
        }
        let core_range =
            GrooveFrameRange::new(self.core_start_frame, self.core_end_frame_exclusive)?;
        let stored_range =
            GrooveFrameRange::new(self.stored_start_frame, self.stored_end_frame_exclusive)?;
        Ok(PhysicalGroovePage::new(
            metadata,
            core_range,
            stored_range,
            self.lateral_displacement_m.into_vec(),
            self.vertical_displacement_m.into_vec(),
        )?)
    }

    fn validate(&self) -> Result<(), StreamingGrooveCutterError> {
        let stored_length = self
            .stored_end_frame_exclusive
            .checked_sub(self.stored_start_frame)
            .ok_or(StreamingGrooveCutterError::InvalidRawPageChunk)?;
        if self.format_version != STREAMING_GROOVE_PAGE_CHUNK_FORMAT_VERSION
            || self.total_frame_count < 4
            || self.core_start_frame >= self.core_end_frame_exclusive
            || self.core_end_frame_exclusive > self.total_frame_count
            || self.stored_start_frame > self.core_start_frame
            || self.stored_end_frame_exclusive < self.core_end_frame_exclusive
            || self.stored_end_frame_exclusive > self.total_frame_count
            || stored_length != self.lateral_displacement_m.len() as u64
            || self.lateral_displacement_m.len() != self.vertical_displacement_m.len()
            || self
                .lateral_displacement_m
                .iter()
                .chain(self.vertical_displacement_m.iter())
                .any(|sample| !sample.is_finite())
        {
            return Err(StreamingGrooveCutterError::InvalidRawPageChunk);
        }
        let halo = u64::from(self.storage_halo_frames);
        if self.stored_start_frame != self.core_start_frame.saturating_sub(halo)
            || self.stored_end_frame_exclusive
                != self
                    .core_end_frame_exclusive
                    .saturating_add(halo)
                    .min(self.total_frame_count)
        {
            return Err(StreamingGrooveCutterError::InvalidRawPageChunk);
        }
        Ok(())
    }
}

/// Reports current work and bounded state sizes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingGrooveCutterProgress {
    accepted_source_frame_count: u64,
    total_source_frame_count: u64,
    produced_output_frame_count: u64,
    total_output_frame_count: u64,
    emitted_page_count: u64,
    partial_report: Option<GrooveCutReport>,
    lateral_prefix_identity: GrooveContentIdentity,
    vertical_prefix_identity: GrooveContentIdentity,
    source_history_frame_count: u64,
    clearance_history_frame_count: u64,
    pending_page_frame_count: u64,
    finished: bool,
}

impl StreamingGrooveCutterProgress {
    pub fn accepted_source_frame_count(self) -> u64 {
        self.accepted_source_frame_count
    }

    pub fn total_source_frame_count(self) -> u64 {
        self.total_source_frame_count
    }

    pub fn produced_output_frame_count(self) -> u64 {
        self.produced_output_frame_count
    }

    pub fn total_output_frame_count(self) -> u64 {
        self.total_output_frame_count
    }

    pub fn emitted_page_count(self) -> u64 {
        self.emitted_page_count
    }

    pub fn partial_report(self) -> Option<GrooveCutReport> {
        self.partial_report
    }

    pub fn lateral_prefix_identity(self) -> GrooveContentIdentity {
        self.lateral_prefix_identity
    }

    pub fn vertical_prefix_identity(self) -> GrooveContentIdentity {
        self.vertical_prefix_identity
    }

    pub fn source_history_frame_count(self) -> u64 {
        self.source_history_frame_count
    }

    pub fn clearance_history_frame_count(self) -> u64 {
        self.clearance_history_frame_count
    }

    pub fn pending_page_frame_count(self) -> u64 {
        self.pending_page_frame_count
    }

    pub fn is_finished(self) -> bool {
        self.finished
    }
}

/// Reports one bounded push that emits at most one page.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingGroovePushResult {
    consumed_source_frame_count: u64,
    progress: StreamingGrooveCutterProgress,
    emitted_page: Option<StreamingGroovePageChunk>,
}

impl StreamingGroovePushResult {
    pub fn consumed_source_frame_count(&self) -> u64 {
        self.consumed_source_frame_count
    }

    pub fn progress(&self) -> StreamingGrooveCutterProgress {
        self.progress
    }

    pub fn emitted_page(&self) -> Option<&StreamingGroovePageChunk> {
        self.emitted_page.as_ref()
    }

    pub fn into_emitted_page(self) -> Option<StreamingGroovePageChunk> {
        self.emitted_page
    }
}

/// Contains final metadata and the shared groove identity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingGrooveFinalization {
    config: StreamingGrooveCutterConfig,
    output_frame_count: u64,
    report: GrooveCutReport,
    content_identity: GrooveContentIdentity,
}

impl StreamingGrooveFinalization {
    pub fn config(self) -> StreamingGrooveCutterConfig {
        self.config
    }

    pub fn output_frame_count(self) -> u64 {
        self.output_frame_count
    }

    pub fn report(self) -> GrooveCutReport {
        self.report
    }

    pub fn content_identity(self) -> GrooveContentIdentity {
        self.content_identity
    }

    /// Creates metadata for all raw chunks from this cut.
    pub fn physical_metadata(
        self,
        generation: GrooveGenerationId,
    ) -> Result<PhysicalGrooveMetadata, PagedGrooveError> {
        PhysicalGrooveMetadata::new(
            generation,
            self.content_identity,
            PhysicalGrooveCutMetadata::new(self.config.layout, self.config.cut, self.report),
            self.output_frame_count,
            self.config.tracing_halo_frames,
        )
    }
}

/// Stores all state that changes subsequent output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingGrooveCutterSnapshot {
    version: u32,
    config: StreamingGrooveCutterConfig,
    expected_output_frame_count: u64,
    accepted_source_frame_count: u64,
    next_output_frame: u64,
    source_history_start_frame: u64,
    left_source_history: VecDeque<f32>,
    right_source_history: VecDeque<f32>,
    left_riaa_state: RiaaFilterState,
    right_riaa_state: RiaaFilterState,
    left_highpass: CutterHighpass,
    right_highpass: CutterHighpass,
    previous_lateral_velocity_m_s: Option<f64>,
    previous_vertical_velocity_m_s: Option<f64>,
    lateral_position_m: f64,
    vertical_position_m: f64,
    peak_left_velocity_m_s: f64,
    peak_right_velocity_m_s: f64,
    sum_left_velocity_squared: f64,
    sum_right_velocity_squared: f64,
    peak_lateral_displacement_m: f64,
    peak_vertical_displacement_m: f64,
    clearance_history_start_frame: u64,
    clearance_lateral_displacement_m: VecDeque<f32>,
    minimum_adjacent_turn_clearance_m: Option<f64>,
    first_failing_clearance_frame_pair: Option<GrooveClearanceFramePair>,
    pending_page_start_frame: u64,
    pending_lateral_displacement_m: VecDeque<f32>,
    pending_vertical_displacement_m: VecDeque<f32>,
    next_page_core_start_frame: u64,
    emitted_page_count: u64,
    lateral_content_hash: GrooveContentHasher,
    vertical_content_hash: GrooveContentHasher,
    finished: bool,
}

impl StreamingGrooveCutterSnapshot {
    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn config(&self) -> StreamingGrooveCutterConfig {
        self.config
    }
}

/// Cuts sequential PCM without retaining the complete record.
#[derive(Debug)]
pub struct StreamingGrooveCutter {
    config: StreamingGrooveCutterConfig,
    expected_output_frame_count: u64,
    source_step: f64,
    accepted_source_frame_count: u64,
    next_output_frame: u64,
    source_history_start_frame: u64,
    left_source_history: VecDeque<f32>,
    right_source_history: VecDeque<f32>,
    left_riaa: RiaaRecordFilter,
    right_riaa: RiaaRecordFilter,
    left_highpass: CutterHighpass,
    right_highpass: CutterHighpass,
    previous_lateral_velocity_m_s: Option<f64>,
    previous_vertical_velocity_m_s: Option<f64>,
    lateral_position_m: f64,
    vertical_position_m: f64,
    peak_left_velocity_m_s: f64,
    peak_right_velocity_m_s: f64,
    sum_left_velocity_squared: f64,
    sum_right_velocity_squared: f64,
    peak_lateral_displacement_m: f64,
    peak_vertical_displacement_m: f64,
    frames_per_revolution: f64,
    clearance_history_enabled: bool,
    clearance_delay_frames: u64,
    clearance_fraction: f64,
    clearance_history_start_frame: u64,
    clearance_lateral_displacement_m: VecDeque<f32>,
    minimum_adjacent_turn_clearance_m: Option<f64>,
    first_failing_clearance_frame_pair: Option<GrooveClearanceFramePair>,
    storage_halo_frames: u32,
    pending_page_start_frame: u64,
    pending_lateral_displacement_m: VecDeque<f32>,
    pending_vertical_displacement_m: VecDeque<f32>,
    next_page_core_start_frame: u64,
    emitted_page_count: u64,
    lateral_content_hash: GrooveContentHasher,
    vertical_content_hash: GrooveContentHasher,
    finished: bool,
}

impl StreamingGrooveCutter {
    pub fn new(config: StreamingGrooveCutterConfig) -> Result<Self, StreamingGrooveCutterError> {
        let config = config.validate()?;
        let expected_output_frame_count = expected_output_frame_count(config)?;
        let source_step = config.source_sample_rate_hz / config.layout.groove_sample_rate_hz;
        let riaa_config = RiaaConfig::new(
            config.layout.groove_sample_rate_hz,
            config.cut.cutter_bandwidth_hz,
        )
        .map_err(|_| StreamingGrooveCutterError::InvalidConfiguration)?;
        let highpass = CutterHighpass::new(
            config.cut.cutter_highpass_hz,
            config.layout.groove_sample_rate_hz,
        );
        let frames_per_revolution =
            config.layout.groove_sample_rate_hz * 60.0 / config.layout.nominal_rpm;
        let clearance_history_enabled =
            frames_per_revolution <= expected_output_frame_count.saturating_sub(1) as f64;
        let clearance_delay_frames = if clearance_history_enabled {
            frames_per_revolution.ceil() as u64
        } else {
            0
        };
        if clearance_history_enabled {
            usize::try_from(clearance_delay_frames)
                .map_err(|_| StreamingGrooveCutterError::ClearanceHistoryTooLarge)?;
        }
        let storage_halo_frames = config.storage_halo_frames()?;
        Ok(Self {
            config,
            expected_output_frame_count,
            source_step,
            accepted_source_frame_count: 0,
            next_output_frame: 0,
            source_history_start_frame: 0,
            left_source_history: VecDeque::with_capacity(4),
            right_source_history: VecDeque::with_capacity(4),
            left_riaa: RiaaRecordFilter::new(riaa_config),
            right_riaa: RiaaRecordFilter::new(riaa_config),
            left_highpass: highpass,
            right_highpass: highpass,
            previous_lateral_velocity_m_s: None,
            previous_vertical_velocity_m_s: None,
            lateral_position_m: 0.0,
            vertical_position_m: 0.0,
            peak_left_velocity_m_s: 0.0,
            peak_right_velocity_m_s: 0.0,
            sum_left_velocity_squared: 0.0,
            sum_right_velocity_squared: 0.0,
            peak_lateral_displacement_m: 0.0,
            peak_vertical_displacement_m: 0.0,
            frames_per_revolution,
            clearance_history_enabled,
            clearance_delay_frames,
            clearance_fraction: frames_per_revolution - frames_per_revolution.floor(),
            clearance_history_start_frame: 0,
            clearance_lateral_displacement_m: VecDeque::new(),
            minimum_adjacent_turn_clearance_m: None,
            first_failing_clearance_frame_pair: None,
            storage_halo_frames,
            pending_page_start_frame: 0,
            pending_lateral_displacement_m: VecDeque::new(),
            pending_vertical_displacement_m: VecDeque::new(),
            next_page_core_start_frame: 0,
            emitted_page_count: 0,
            lateral_content_hash: begin_groove_lateral_content_hash(expected_output_frame_count),
            vertical_content_hash: begin_groove_vertical_content_hash(expected_output_frame_count),
            finished: false,
        })
    }

    pub fn config(&self) -> StreamingGrooveCutterConfig {
        self.config
    }

    pub fn progress(&self) -> StreamingGrooveCutterProgress {
        StreamingGrooveCutterProgress {
            accepted_source_frame_count: self.accepted_source_frame_count,
            total_source_frame_count: self.config.total_source_frame_count,
            produced_output_frame_count: self.next_output_frame,
            total_output_frame_count: self.expected_output_frame_count,
            emitted_page_count: self.emitted_page_count,
            partial_report: self.current_report(),
            lateral_prefix_identity: self.lateral_content_hash.clone().finish(),
            vertical_prefix_identity: self.vertical_content_hash.clone().finish(),
            source_history_frame_count: self.left_source_history.len() as u64,
            clearance_history_frame_count: self.clearance_lateral_displacement_m.len() as u64,
            pending_page_frame_count: self.pending_lateral_displacement_m.len() as u64,
            finished: self.finished,
        }
    }

    /// Accepts one exact sequential source range.
    ///
    /// The callback receives one bounded page chunk at a time.
    pub fn push_chunk<F>(
        &mut self,
        absolute_source_frame: u64,
        channels: &[&[f32]],
        mut emit_page: F,
    ) -> Result<StreamingGrooveCutterProgress, StreamingGrooveCutterError>
    where
        F: FnMut(StreamingGroovePageChunk),
    {
        self.validate_chunk(absolute_source_frame, channels)?;
        for (frame, left) in channels[0].iter().copied().enumerate() {
            let right = if self.config.source_channel_count == 2 {
                channels[1][frame]
            } else {
                left
            };
            self.left_source_history.push_back(left);
            self.right_source_history.push_back(right);
            self.accepted_source_frame_count += 1;
            self.produce_available_output(&mut emit_page);
        }
        Ok(self.progress())
    }

    /// Accepts a sequential source prefix and emits at most one page.
    ///
    /// The result can consume zero source frames when previously accepted
    /// input can produce another page. Call this method again with the same
    /// unconsumed input after the caller stores the returned page.
    pub fn push_chunk_until_page(
        &mut self,
        absolute_source_frame: u64,
        channels: &[&[f32]],
    ) -> Result<StreamingGroovePushResult, StreamingGrooveCutterError> {
        self.validate_chunk(absolute_source_frame, channels)?;

        if let Some(emitted_page) = self.produce_available_output_until_page() {
            return Ok(StreamingGroovePushResult {
                consumed_source_frame_count: 0,
                progress: self.progress(),
                emitted_page: Some(emitted_page),
            });
        }

        let mut consumed_source_frame_count = 0_u64;
        for (frame, left) in channels[0].iter().copied().enumerate() {
            let right = if self.config.source_channel_count == 2 {
                channels[1][frame]
            } else {
                left
            };
            self.left_source_history.push_back(left);
            self.right_source_history.push_back(right);
            self.accepted_source_frame_count += 1;
            consumed_source_frame_count += 1;
            if let Some(emitted_page) = self.produce_available_output_until_page() {
                return Ok(StreamingGroovePushResult {
                    consumed_source_frame_count,
                    progress: self.progress(),
                    emitted_page: Some(emitted_page),
                });
            }
        }

        Ok(StreamingGroovePushResult {
            consumed_source_frame_count,
            progress: self.progress(),
            emitted_page: None,
        })
    }

    /// Finalizes a complete declared source stream.
    pub fn finish(&mut self) -> Result<StreamingGrooveFinalization, StreamingGrooveCutterError> {
        if self.finished {
            return Err(StreamingGrooveCutterError::AlreadyFinished);
        }
        if self.accepted_source_frame_count != self.config.total_source_frame_count {
            return Err(StreamingGrooveCutterError::IncompleteSource {
                expected_frame_count: self.config.total_source_frame_count,
                accepted_frame_count: self.accepted_source_frame_count,
            });
        }
        if self.next_output_frame != self.expected_output_frame_count
            || self.next_page_core_start_frame != self.expected_output_frame_count
        {
            return Err(StreamingGrooveCutterError::InvalidInternalState);
        }
        let report = self
            .current_report()
            .ok_or(StreamingGrooveCutterError::InvalidInternalState)?;
        let source = GrooveSourceProvenance::pcm_frames(
            self.config.source_sample_rate_hz,
            self.config.source_channel_count,
            self.config.total_source_frame_count,
        );
        let content_identity = finalize_groove_content_identity(
            GROOVE_ASSET_FORMAT_VERSION,
            self.config.layout,
            source,
            self.config.cut,
            report,
            self.lateral_content_hash.clone().finish(),
            self.vertical_content_hash.clone().finish(),
        );
        self.finished = true;
        Ok(StreamingGrooveFinalization {
            config: self.config,
            output_frame_count: self.expected_output_frame_count,
            report,
            content_identity,
        })
    }

    /// Captures the bounded continuation state.
    ///
    /// A restore can emit pages that the caller received after this snapshot.
    pub fn snapshot(&self) -> StreamingGrooveCutterSnapshot {
        StreamingGrooveCutterSnapshot {
            version: STREAMING_GROOVE_CUTTER_SNAPSHOT_VERSION,
            config: self.config,
            expected_output_frame_count: self.expected_output_frame_count,
            accepted_source_frame_count: self.accepted_source_frame_count,
            next_output_frame: self.next_output_frame,
            source_history_start_frame: self.source_history_start_frame,
            left_source_history: self.left_source_history.clone(),
            right_source_history: self.right_source_history.clone(),
            left_riaa_state: self.left_riaa.state(),
            right_riaa_state: self.right_riaa.state(),
            left_highpass: self.left_highpass,
            right_highpass: self.right_highpass,
            previous_lateral_velocity_m_s: self.previous_lateral_velocity_m_s,
            previous_vertical_velocity_m_s: self.previous_vertical_velocity_m_s,
            lateral_position_m: self.lateral_position_m,
            vertical_position_m: self.vertical_position_m,
            peak_left_velocity_m_s: self.peak_left_velocity_m_s,
            peak_right_velocity_m_s: self.peak_right_velocity_m_s,
            sum_left_velocity_squared: self.sum_left_velocity_squared,
            sum_right_velocity_squared: self.sum_right_velocity_squared,
            peak_lateral_displacement_m: self.peak_lateral_displacement_m,
            peak_vertical_displacement_m: self.peak_vertical_displacement_m,
            clearance_history_start_frame: self.clearance_history_start_frame,
            clearance_lateral_displacement_m: self.clearance_lateral_displacement_m.clone(),
            minimum_adjacent_turn_clearance_m: self.minimum_adjacent_turn_clearance_m,
            first_failing_clearance_frame_pair: self.first_failing_clearance_frame_pair,
            pending_page_start_frame: self.pending_page_start_frame,
            pending_lateral_displacement_m: self.pending_lateral_displacement_m.clone(),
            pending_vertical_displacement_m: self.pending_vertical_displacement_m.clone(),
            next_page_core_start_frame: self.next_page_core_start_frame,
            emitted_page_count: self.emitted_page_count,
            lateral_content_hash: self.lateral_content_hash.clone(),
            vertical_content_hash: self.vertical_content_hash.clone(),
            finished: self.finished,
        }
    }

    /// Restores one validated snapshot with the same configuration.
    pub fn restore(
        &mut self,
        snapshot: &StreamingGrooveCutterSnapshot,
    ) -> Result<(), StreamingGrooveCutterError> {
        if snapshot.config != self.config {
            return Err(StreamingGrooveCutterError::SnapshotConfigMismatch);
        }
        let restored = Self::from_snapshot(snapshot)?;
        *self = restored;
        Ok(())
    }

    /// Creates one cutter from a validated snapshot.
    pub fn from_snapshot(
        snapshot: &StreamingGrooveCutterSnapshot,
    ) -> Result<Self, StreamingGrooveCutterError> {
        validate_snapshot(snapshot)?;
        let mut cutter = Self::new(snapshot.config)?;
        cutter
            .left_riaa
            .restore_state(snapshot.left_riaa_state)
            .map_err(|_| StreamingGrooveCutterError::InvalidSnapshot)?;
        cutter
            .right_riaa
            .restore_state(snapshot.right_riaa_state)
            .map_err(|_| StreamingGrooveCutterError::InvalidSnapshot)?;
        cutter.accepted_source_frame_count = snapshot.accepted_source_frame_count;
        cutter.next_output_frame = snapshot.next_output_frame;
        cutter.source_history_start_frame = snapshot.source_history_start_frame;
        cutter.left_source_history = snapshot.left_source_history.clone();
        cutter.right_source_history = snapshot.right_source_history.clone();
        cutter.left_highpass = snapshot.left_highpass;
        cutter.right_highpass = snapshot.right_highpass;
        cutter.previous_lateral_velocity_m_s = snapshot.previous_lateral_velocity_m_s;
        cutter.previous_vertical_velocity_m_s = snapshot.previous_vertical_velocity_m_s;
        cutter.lateral_position_m = snapshot.lateral_position_m;
        cutter.vertical_position_m = snapshot.vertical_position_m;
        cutter.peak_left_velocity_m_s = snapshot.peak_left_velocity_m_s;
        cutter.peak_right_velocity_m_s = snapshot.peak_right_velocity_m_s;
        cutter.sum_left_velocity_squared = snapshot.sum_left_velocity_squared;
        cutter.sum_right_velocity_squared = snapshot.sum_right_velocity_squared;
        cutter.peak_lateral_displacement_m = snapshot.peak_lateral_displacement_m;
        cutter.peak_vertical_displacement_m = snapshot.peak_vertical_displacement_m;
        cutter.clearance_history_start_frame = snapshot.clearance_history_start_frame;
        cutter.clearance_lateral_displacement_m = snapshot.clearance_lateral_displacement_m.clone();
        cutter.minimum_adjacent_turn_clearance_m = snapshot.minimum_adjacent_turn_clearance_m;
        cutter.first_failing_clearance_frame_pair = snapshot.first_failing_clearance_frame_pair;
        cutter.pending_page_start_frame = snapshot.pending_page_start_frame;
        cutter.pending_lateral_displacement_m = snapshot.pending_lateral_displacement_m.clone();
        cutter.pending_vertical_displacement_m = snapshot.pending_vertical_displacement_m.clone();
        cutter.next_page_core_start_frame = snapshot.next_page_core_start_frame;
        cutter.emitted_page_count = snapshot.emitted_page_count;
        cutter.lateral_content_hash = snapshot.lateral_content_hash.clone();
        cutter.vertical_content_hash = snapshot.vertical_content_hash.clone();
        cutter.finished = snapshot.finished;
        Ok(cutter)
    }

    fn validate_chunk(
        &self,
        absolute_source_frame: u64,
        channels: &[&[f32]],
    ) -> Result<(), StreamingGrooveCutterError> {
        if self.finished {
            return Err(StreamingGrooveCutterError::AlreadyFinished);
        }
        if absolute_source_frame != self.accepted_source_frame_count {
            return Err(StreamingGrooveCutterError::UnexpectedSourceFrame {
                expected_frame: self.accepted_source_frame_count,
                received_frame: absolute_source_frame,
            });
        }
        if channels.len() != usize::from(self.config.source_channel_count) {
            return Err(StreamingGrooveCutterError::SourceChannelCountMismatch);
        }
        let frame_count = channels[0].len();
        if channels.iter().any(|channel| channel.len() != frame_count) {
            return Err(StreamingGrooveCutterError::SourceChannelLengthMismatch);
        }
        let end_frame = absolute_source_frame
            .checked_add(frame_count as u64)
            .ok_or(StreamingGrooveCutterError::SourceFrameRangeOverflow)?;
        if end_frame > self.config.total_source_frame_count {
            return Err(StreamingGrooveCutterError::SourceExceedsDeclaredLength);
        }
        if channels
            .iter()
            .flat_map(|channel| channel.iter())
            .any(|sample| !sample.is_finite())
        {
            return Err(StreamingGrooveCutterError::NonfiniteProgramme);
        }
        Ok(())
    }

    fn produce_available_output<F>(&mut self, emit_page: &mut F)
    where
        F: FnMut(StreamingGroovePageChunk),
    {
        while self.next_output_frame < self.expected_output_frame_count
            && self.next_output_is_ready()
        {
            let output_frame = self.next_output_frame;
            let position = output_frame as f64 * self.source_step;
            let left = self.resample_history(&self.left_source_history, position) as f32;
            let right = self.resample_history(&self.right_source_history, position) as f32;
            self.process_output_frame(output_frame, left, right);
            self.next_output_frame += 1;
            self.emit_ready_pages(emit_page);
            self.trim_source_history();
        }
    }

    fn produce_available_output_until_page(&mut self) -> Option<StreamingGroovePageChunk> {
        if let Some(page) = self.take_ready_page() {
            return Some(page);
        }
        while self.next_output_frame < self.expected_output_frame_count
            && self.next_output_is_ready()
        {
            let output_frame = self.next_output_frame;
            let position = output_frame as f64 * self.source_step;
            let left = self.resample_history(&self.left_source_history, position) as f32;
            let right = self.resample_history(&self.right_source_history, position) as f32;
            self.process_output_frame(output_frame, left, right);
            self.next_output_frame += 1;
            let emitted_page = self.take_ready_page();
            self.trim_source_history();
            if emitted_page.is_some() {
                return emitted_page;
            }
        }
        None
    }

    fn next_output_is_ready(&self) -> bool {
        let position = self.next_output_frame as f64 * self.source_step;
        let center = position.floor() as u64;
        let maximum_source_frame = center
            .saturating_add(2)
            .min(self.config.total_source_frame_count - 1);
        maximum_source_frame < self.accepted_source_frame_count
    }

    fn resample_history(&self, history: &VecDeque<f32>, position: f64) -> f64 {
        let center = position.floor() as i64;
        let fraction = position - center as f64;
        catmull_rom_sample(
            self.source_sample(history, center - 1),
            self.source_sample(history, center),
            self.source_sample(history, center + 1),
            self.source_sample(history, center + 2),
            fraction,
        )
    }

    fn source_sample(&self, history: &VecDeque<f32>, frame: i64) -> f64 {
        let maximum = self.config.total_source_frame_count - 1;
        let clamped = if frame <= 0 {
            0
        } else {
            (frame as u64).min(maximum)
        };
        let local = usize::try_from(clamped - self.source_history_start_frame)
            .expect("validated source history contains every interpolation frame");
        f64::from(history[local])
    }

    fn trim_source_history(&mut self) {
        if self.next_output_frame >= self.expected_output_frame_count {
            self.left_source_history.clear();
            self.right_source_history.clear();
            self.source_history_start_frame = self.accepted_source_frame_count;
            return;
        }
        let next_position = self.next_output_frame as f64 * self.source_step;
        let retain_from = (next_position.floor() as u64).saturating_sub(1);
        while self.source_history_start_frame < retain_from {
            self.left_source_history.pop_front();
            self.right_source_history.pop_front();
            self.source_history_start_frame += 1;
        }
    }

    fn process_output_frame(&mut self, output_frame: u64, left: f32, right: f32) {
        let velocity_scale =
            self.config.cut.full_scale_sine_velocity_rms_m_s * std::f64::consts::SQRT_2;
        let left_programme = self.left_highpass.process(f64::from(left));
        let right_programme = self.right_highpass.process(f64::from(right));
        let left_velocity =
            (self.left_riaa.process_sample_f64(left_programme) * velocity_scale) as f32;
        let right_velocity =
            (self.right_riaa.process_sample_f64(right_programme) * velocity_scale) as f32;
        let left_velocity_f64 = f64::from(left_velocity);
        let right_velocity_f64 = f64::from(right_velocity);
        let (lateral_velocity, vertical_velocity) =
            encode_45_45(left_velocity_f64, right_velocity_f64);

        if let (Some(previous_lateral), Some(previous_vertical)) = (
            self.previous_lateral_velocity_m_s,
            self.previous_vertical_velocity_m_s,
        ) {
            let dt = 1.0 / self.config.layout.groove_sample_rate_hz;
            self.lateral_position_m += 0.5 * (previous_lateral + lateral_velocity) * dt;
            self.vertical_position_m += 0.5 * (previous_vertical + vertical_velocity) * dt;
            self.peak_lateral_displacement_m = self
                .peak_lateral_displacement_m
                .max(self.lateral_position_m.abs());
            self.peak_vertical_displacement_m = self
                .peak_vertical_displacement_m
                .max(self.vertical_position_m.abs());
        }
        self.previous_lateral_velocity_m_s = Some(lateral_velocity);
        self.previous_vertical_velocity_m_s = Some(vertical_velocity);
        self.peak_left_velocity_m_s = self.peak_left_velocity_m_s.max(left_velocity_f64.abs());
        self.peak_right_velocity_m_s = self.peak_right_velocity_m_s.max(right_velocity_f64.abs());
        self.sum_left_velocity_squared += left_velocity_f64 * left_velocity_f64;
        self.sum_right_velocity_squared += right_velocity_f64 * right_velocity_f64;

        let lateral_displacement_m = self.lateral_position_m as f32;
        let vertical_displacement_m = self.vertical_position_m as f32;
        self.lateral_content_hash.f32(lateral_displacement_m);
        self.vertical_content_hash.f32(vertical_displacement_m);
        self.update_clearance(output_frame, lateral_displacement_m);
        debug_assert_eq!(
            self.pending_page_start_frame + self.pending_lateral_displacement_m.len() as u64,
            output_frame
        );
        self.pending_lateral_displacement_m
            .push_back(lateral_displacement_m);
        self.pending_vertical_displacement_m
            .push_back(vertical_displacement_m);
    }

    fn update_clearance(&mut self, output_frame: u64, lateral_displacement_m: f32) {
        if !self.clearance_history_enabled {
            return;
        }
        debug_assert_eq!(
            self.clearance_history_start_frame + self.clearance_lateral_displacement_m.len() as u64,
            output_frame
        );
        self.clearance_lateral_displacement_m
            .push_back(lateral_displacement_m);
        if output_frame >= self.clearance_delay_frames {
            let outer_frame = output_frame - self.clearance_delay_frames;
            let inner_lower_frame = outer_frame + self.frames_per_revolution.floor() as u64;
            let outer = self.clearance_sample(outer_frame);
            let inner_lower = self.clearance_sample(inner_lower_frame);
            let inner_upper = if self.clearance_fraction == 0.0 {
                inner_lower
            } else {
                self.clearance_sample(inner_lower_frame + 1)
            };
            let inner = inner_lower + (inner_upper - inner_lower) * self.clearance_fraction;
            let outer_centerline_m = self.config.layout.unclamped_radius_at_frame(
                outer_frame as f64,
                self.config.cut.groove_pitch_m_per_revolution,
            ) + outer;
            let inner_position = outer_frame as f64 + self.frames_per_revolution;
            let inner_centerline_m = self.config.layout.unclamped_radius_at_frame(
                inner_position,
                self.config.cut.groove_pitch_m_per_revolution,
            ) + inner;
            let clearance_m =
                outer_centerline_m - inner_centerline_m - self.config.cut.groove_top_width_m;
            self.minimum_adjacent_turn_clearance_m = Some(
                self.minimum_adjacent_turn_clearance_m
                    .map_or(clearance_m, |current| current.min(clearance_m)),
            );
            if self.first_failing_clearance_frame_pair.is_none()
                && clearance_m < self.config.cut.minimum_land_width_m
            {
                self.first_failing_clearance_frame_pair = Some(GrooveClearanceFramePair {
                    outer_frame,
                    inner_frame: inner_lower_frame,
                });
            }
        }
        let maximum_history = self.clearance_delay_frames.saturating_add(1);
        while self.clearance_lateral_displacement_m.len() as u64 > maximum_history {
            self.clearance_lateral_displacement_m.pop_front();
            self.clearance_history_start_frame += 1;
        }
    }

    fn clearance_sample(&self, frame: u64) -> f64 {
        let local = usize::try_from(frame - self.clearance_history_start_frame)
            .expect("validated clearance history contains one revolution");
        f64::from(self.clearance_lateral_displacement_m[local])
    }

    fn emit_ready_pages<F>(&mut self, emit_page: &mut F)
    where
        F: FnMut(StreamingGroovePageChunk),
    {
        while let Some(page) = self.take_ready_page() {
            emit_page(page);
        }
    }

    fn take_ready_page(&mut self) -> Option<StreamingGroovePageChunk> {
        let core_size = u64::from(self.config.page_core_frame_count);
        let halo = u64::from(self.storage_halo_frames);
        if self.next_page_core_start_frame >= self.expected_output_frame_count {
            return None;
        }
        let core_start = self.next_page_core_start_frame;
        let core_end = core_start
            .saturating_add(core_size)
            .min(self.expected_output_frame_count);
        let stored_start = core_start.saturating_sub(halo);
        let stored_end = core_end
            .saturating_add(halo)
            .min(self.expected_output_frame_count);
        if self.next_output_frame < stored_end {
            return None;
        }
        let local_start = usize::try_from(stored_start - self.pending_page_start_frame)
            .expect("validated page history contains the left halo");
        let stored_length = usize::try_from(stored_end - stored_start)
            .expect("validated page size fits this platform");
        let lateral_displacement_m = self
            .pending_lateral_displacement_m
            .iter()
            .skip(local_start)
            .take(stored_length)
            .copied()
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let vertical_displacement_m = self
            .pending_vertical_displacement_m
            .iter()
            .skip(local_start)
            .take(stored_length)
            .copied()
            .collect::<Vec<_>>()
            .into_boxed_slice();
        self.emitted_page_count += 1;
        self.next_page_core_start_frame = core_end;
        let retain_from = core_end.saturating_sub(halo);
        while self.pending_page_start_frame < retain_from {
            self.pending_lateral_displacement_m.pop_front();
            self.pending_vertical_displacement_m.pop_front();
            self.pending_page_start_frame += 1;
        }
        Some(StreamingGroovePageChunk {
            format_version: STREAMING_GROOVE_PAGE_CHUNK_FORMAT_VERSION,
            total_frame_count: self.expected_output_frame_count,
            storage_halo_frames: self.storage_halo_frames,
            core_start_frame: core_start,
            core_end_frame_exclusive: core_end,
            stored_start_frame: stored_start,
            stored_end_frame_exclusive: stored_end,
            lateral_displacement_m,
            vertical_displacement_m,
        })
    }

    fn current_report(&self) -> Option<GrooveCutReport> {
        (self.next_output_frame > 0).then(|| {
            let final_program_radius_m = self.config.layout.unclamped_radius_at_frame(
                (self.next_output_frame - 1) as f64,
                self.config.cut.groove_pitch_m_per_revolution,
            );
            GrooveCutReport {
                peak_left_velocity_m_s: self.peak_left_velocity_m_s,
                peak_right_velocity_m_s: self.peak_right_velocity_m_s,
                rms_left_velocity_m_s: (self.sum_left_velocity_squared
                    / self.next_output_frame as f64)
                    .sqrt(),
                rms_right_velocity_m_s: (self.sum_right_velocity_squared
                    / self.next_output_frame as f64)
                    .sqrt(),
                peak_lateral_displacement_m: self.peak_lateral_displacement_m,
                peak_vertical_displacement_m: self.peak_vertical_displacement_m,
                final_lateral_drift_m: self.lateral_position_m,
                final_vertical_drift_m: self.vertical_position_m,
                groove_pitch_m_per_revolution: self.config.cut.groove_pitch_m_per_revolution,
                final_program_radius_m,
                programme_exceeds_available_radius: final_program_radius_m
                    < self.config.layout.inner_program_radius_m,
                minimum_adjacent_turn_clearance_m: self.minimum_adjacent_turn_clearance_m,
                first_failing_clearance_frame_pair: self.first_failing_clearance_frame_pair,
                adjacent_turn_clearance_failed: self.first_failing_clearance_frame_pair.is_some(),
            }
        })
    }
}

fn expected_output_frame_count(
    config: StreamingGrooveCutterConfig,
) -> Result<u64, StreamingGrooveCutterError> {
    let source_step = config.source_sample_rate_hz / config.layout.groove_sample_rate_hz;
    let output_frame_count =
        (((config.total_source_frame_count - 1) as f64 / source_step).floor() as u64).max(1);
    (output_frame_count <= MAX_EXACT_FRAME_COUNT)
        .then_some(output_frame_count)
        .ok_or(StreamingGrooveCutterError::InvalidOutputFrameCount)
}

fn validate_snapshot(
    snapshot: &StreamingGrooveCutterSnapshot,
) -> Result<(), StreamingGrooveCutterError> {
    if snapshot.version != STREAMING_GROOVE_CUTTER_SNAPSHOT_VERSION {
        return Err(StreamingGrooveCutterError::UnsupportedSnapshotVersion {
            version: snapshot.version,
        });
    }
    let config = snapshot.config.validate()?;
    let expected_output = expected_output_frame_count(config)?;
    let storage_halo = u64::from(config.storage_halo_frames()?);
    let page_bound = u64::from(config.page_core_frame_count)
        .checked_add(storage_halo.saturating_mul(2))
        .ok_or(StreamingGrooveCutterError::InvalidSnapshot)?;
    let frames_per_revolution =
        config.layout.groove_sample_rate_hz * 60.0 / config.layout.nominal_rpm;
    let clearance_history_enabled =
        frames_per_revolution <= expected_output.saturating_sub(1) as f64;
    let clearance_delay = if clearance_history_enabled {
        frames_per_revolution.ceil() as u64
    } else {
        0
    };
    let clearance_bound = if clearance_history_enabled {
        clearance_delay + 1
    } else {
        0
    };
    let source_step = config.source_sample_rate_hz / config.layout.groove_sample_rate_hz;
    let output_is_ready = |output_frame: u64| {
        let position = output_frame as f64 * source_step;
        let center = position.floor() as u64;
        center
            .saturating_add(2)
            .min(config.total_source_frame_count - 1)
            < snapshot.accepted_source_frame_count
    };
    let previous_output_was_ready =
        snapshot.next_output_frame == 0 || output_is_ready(snapshot.next_output_frame - 1);
    let expected_source_history_start = if snapshot.next_output_frame == expected_output {
        snapshot.accepted_source_frame_count
    } else {
        (snapshot.next_output_frame as f64 * source_step)
            .floor()
            .max(0.0) as u64
    }
    .saturating_sub(u64::from(snapshot.next_output_frame != expected_output));
    let expected_clearance_length = snapshot.next_output_frame.min(clearance_bound);
    let expected_clearance_end = if clearance_history_enabled {
        snapshot.next_output_frame
    } else {
        0
    };
    let page_core = u64::from(config.page_core_frame_count);
    let maximum_next_page_core_start = if snapshot.next_output_frame == expected_output {
        expected_output
    } else {
        snapshot
            .next_output_frame
            .saturating_sub(storage_halo)
            .checked_div(page_core)
            .unwrap_or(0)
            .saturating_mul(page_core)
    };
    let expected_pending_page_start = snapshot
        .next_page_core_start_frame
        .saturating_sub(storage_halo);
    let expected_emitted_page_count = if snapshot.next_page_core_start_frame == expected_output {
        expected_output.saturating_sub(1) / page_core + 1
    } else {
        snapshot.next_page_core_start_frame / page_core
    };
    let expected_lateral_hash_bytes = begin_groove_lateral_content_hash(expected_output)
        .byte_len()
        .checked_add(snapshot.next_output_frame.saturating_mul(4));
    let expected_vertical_hash_bytes = begin_groove_vertical_content_hash(expected_output)
        .byte_len()
        .checked_add(snapshot.next_output_frame.saturating_mul(4));
    let numeric_values_are_finite = [
        snapshot.lateral_position_m,
        snapshot.vertical_position_m,
        snapshot.peak_left_velocity_m_s,
        snapshot.peak_right_velocity_m_s,
        snapshot.sum_left_velocity_squared,
        snapshot.sum_right_velocity_squared,
        snapshot.peak_lateral_displacement_m,
        snapshot.peak_vertical_displacement_m,
    ]
    .into_iter()
    .chain(snapshot.previous_lateral_velocity_m_s)
    .chain(snapshot.previous_vertical_velocity_m_s)
    .chain(snapshot.minimum_adjacent_turn_clearance_m)
    .all(f64::is_finite);
    let source_lengths_match =
        snapshot.left_source_history.len() == snapshot.right_source_history.len();
    let source_end = snapshot
        .source_history_start_frame
        .checked_add(snapshot.left_source_history.len() as u64);
    let clearance_end = snapshot
        .clearance_history_start_frame
        .checked_add(snapshot.clearance_lateral_displacement_m.len() as u64);
    let pending_lengths_match = snapshot.pending_lateral_displacement_m.len()
        == snapshot.pending_vertical_displacement_m.len();
    let pending_end = snapshot
        .pending_page_start_frame
        .checked_add(snapshot.pending_lateral_displacement_m.len() as u64);
    let samples_are_finite = snapshot
        .left_source_history
        .iter()
        .chain(snapshot.right_source_history.iter())
        .chain(snapshot.clearance_lateral_displacement_m.iter())
        .chain(snapshot.pending_lateral_displacement_m.iter())
        .chain(snapshot.pending_vertical_displacement_m.iter())
        .all(|sample| sample.is_finite());
    let velocities_match_output = if snapshot.next_output_frame == 0 {
        snapshot.previous_lateral_velocity_m_s.is_none()
            && snapshot.previous_vertical_velocity_m_s.is_none()
    } else {
        snapshot.previous_lateral_velocity_m_s.is_some()
            && snapshot.previous_vertical_velocity_m_s.is_some()
    };
    let page_core_is_aligned = snapshot.next_page_core_start_frame == expected_output
        || snapshot
            .next_page_core_start_frame
            .is_multiple_of(u64::from(config.page_core_frame_count));
    let failing_pair_is_valid = snapshot
        .first_failing_clearance_frame_pair
        .is_none_or(|pair| {
            pair.outer_frame < snapshot.next_output_frame
                && pair.inner_frame < snapshot.next_output_frame
                && pair.outer_frame <= pair.inner_frame
        });
    if snapshot.expected_output_frame_count != expected_output
        || snapshot.accepted_source_frame_count > config.total_source_frame_count
        || snapshot.next_output_frame > expected_output
        || !previous_output_was_ready
        || !source_lengths_match
        || source_end != Some(snapshot.accepted_source_frame_count)
        || snapshot.source_history_start_frame != expected_source_history_start
        || snapshot.left_source_history.len() as u64 > MAX_STREAMING_GROOVE_SOURCE_HISTORY_FRAMES
        || clearance_end != Some(expected_clearance_end)
        || snapshot.clearance_lateral_displacement_m.len() as u64 != expected_clearance_length
        || snapshot.clearance_history_start_frame
            != expected_clearance_end - expected_clearance_length
        || snapshot.minimum_adjacent_turn_clearance_m.is_some()
            != (clearance_history_enabled && snapshot.next_output_frame > clearance_delay)
        || !failing_pair_is_valid
        || !pending_lengths_match
        || pending_end != Some(snapshot.next_output_frame)
        || snapshot.pending_lateral_displacement_m.len() as u64 > page_bound
        || snapshot.next_page_core_start_frame > expected_output
        || snapshot.next_page_core_start_frame > maximum_next_page_core_start
        || snapshot.pending_page_start_frame != expected_pending_page_start
        || snapshot.emitted_page_count != expected_emitted_page_count
        || !page_core_is_aligned
        || !snapshot.left_highpass.is_valid()
        || !snapshot.right_highpass.is_valid()
        || !snapshot.left_highpass.configuration_matches(
            config.cut.cutter_highpass_hz,
            config.layout.groove_sample_rate_hz,
        )
        || !snapshot.right_highpass.configuration_matches(
            config.cut.cutter_highpass_hz,
            config.layout.groove_sample_rate_hz,
        )
        || !numeric_values_are_finite
        || snapshot.peak_left_velocity_m_s < 0.0
        || snapshot.peak_right_velocity_m_s < 0.0
        || snapshot.sum_left_velocity_squared < 0.0
        || snapshot.sum_right_velocity_squared < 0.0
        || snapshot.peak_lateral_displacement_m < 0.0
        || snapshot.peak_vertical_displacement_m < 0.0
        || !samples_are_finite
        || !velocities_match_output
        || !snapshot.lateral_content_hash.is_valid()
        || !snapshot.vertical_content_hash.is_valid()
        || Some(snapshot.lateral_content_hash.byte_len()) != expected_lateral_hash_bytes
        || Some(snapshot.vertical_content_hash.byte_len()) != expected_vertical_hash_bytes
        || snapshot.finished
            && (snapshot.accepted_source_frame_count != config.total_source_frame_count
                || snapshot.next_output_frame != expected_output
                || snapshot.next_page_core_start_frame != expected_output)
        || snapshot.next_output_frame == expected_output
            && snapshot.accepted_source_frame_count != config.total_source_frame_count
    {
        return Err(StreamingGrooveCutterError::InvalidSnapshot);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum StreamingGrooveCutterError {
    #[error("the streaming cutter configuration is invalid")]
    InvalidConfiguration,
    #[error("the streaming cutter requires a 192 kHz groove rate")]
    UnsupportedGrooveSampleRate,
    #[error("the source sample rate must be positive and no more than 192 kHz")]
    UnsupportedSourceSampleRate,
    #[error("the source must have one or two channels")]
    InvalidSourceChannelCount,
    #[error("the declared source frame count is invalid")]
    InvalidTotalSourceFrameCount,
    #[error("the calculated output frame count is invalid")]
    InvalidOutputFrameCount,
    #[error("the page core frame count is invalid")]
    InvalidPageCoreFrameCount,
    #[error("the tracing halo frame count is invalid")]
    InvalidTracingHaloFrameCount,
    #[error("one revolution needs more history than this platform can address")]
    ClearanceHistoryTooLarge,
    #[error("expected source frame {expected_frame}, received {received_frame}")]
    UnexpectedSourceFrame {
        expected_frame: u64,
        received_frame: u64,
    },
    #[error("the input channel count does not match the cutter configuration")]
    SourceChannelCountMismatch,
    #[error("the input channels have different frame counts")]
    SourceChannelLengthMismatch,
    #[error("the input source frame range overflowed")]
    SourceFrameRangeOverflow,
    #[error("the input extends past the declared source length")]
    SourceExceedsDeclaredLength,
    #[error("the input PCM contains a nonfinite sample")]
    NonfiniteProgramme,
    #[error(
        "the source is incomplete: expected {expected_frame_count} frames, accepted {accepted_frame_count}"
    )]
    IncompleteSource {
        expected_frame_count: u64,
        accepted_frame_count: u64,
    },
    #[error("the streaming cutter is already finished")]
    AlreadyFinished,
    #[error("the streaming cutter snapshot version {version} is not supported")]
    UnsupportedSnapshotVersion { version: u32 },
    #[error("the streaming cutter snapshot uses a different configuration")]
    SnapshotConfigMismatch,
    #[error("the streaming cutter snapshot is invalid")]
    InvalidSnapshot,
    #[error("the streaming cutter entered an invalid internal state")]
    InvalidInternalState,
    #[error("the raw streaming page chunk is invalid")]
    InvalidRawPageChunk,
    #[error("the raw page chunk does not match the final metadata")]
    RawPageMetadataMismatch,
    #[error(transparent)]
    Groove(#[from] GrooveError),
    #[error(transparent)]
    PagedGroove(#[from] PagedGrooveError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physical::{GrooveAsset, PagedGrooveAsset};

    fn test_programme(frame_count: usize, sample_rate_hz: f64) -> (Vec<f32>, Vec<f32>) {
        let left = (0..frame_count)
            .map(|frame| {
                let time = frame as f64 / sample_rate_hz;
                (0.61 * (std::f64::consts::TAU * 997.0 * time).sin()
                    + 0.17 * (std::f64::consts::TAU * 7_123.0 * time).cos()
                    + if frame % 113 == 0 { 0.09 } else { 0.0 }) as f32
            })
            .collect();
        let right = (0..frame_count)
            .map(|frame| {
                let time = frame as f64 / sample_rate_hz;
                (-0.43 * (std::f64::consts::TAU * 1_331.0 * time).cos()
                    + 0.21 * (std::f64::consts::TAU * 5_303.0 * time).sin()
                    - if frame % 197 == 0 { 0.07 } else { 0.0 }) as f32
            })
            .collect();
        (left, right)
    }

    fn config(
        source_sample_rate_hz: f64,
        source_channel_count: u8,
        total_source_frame_count: usize,
    ) -> StreamingGrooveCutterConfig {
        StreamingGrooveCutterConfig {
            source_sample_rate_hz,
            source_channel_count,
            total_source_frame_count: total_source_frame_count as u64,
            layout: GrooveLayout::default(),
            cut: RecordCutConfig::default(),
            page_core_frame_count: 257,
            tracing_halo_frames: 64,
        }
    }

    fn push_partitioned(
        cutter: &mut StreamingGrooveCutter,
        left: &[f32],
        right: Option<&[f32]>,
        partitions: &[usize],
    ) -> Vec<StreamingGroovePageChunk> {
        let mut pages = Vec::new();
        let mut start = 0;
        let mut partition_index = 0;
        while start < left.len() {
            let length = partitions[partition_index % partitions.len()].min(left.len() - start);
            let end = start + length;
            let progress = if let Some(right) = right {
                cutter
                    .push_chunk(
                        start as u64,
                        &[&left[start..end], &right[start..end]],
                        |page| pages.push(page),
                    )
                    .unwrap()
            } else {
                cutter
                    .push_chunk(start as u64, &[&left[start..end]], |page| pages.push(page))
                    .unwrap()
            };
            assert!(progress.source_history_frame_count() <= 4);
            start = end;
            partition_index += 1;
        }
        pages
    }

    fn push_stereo_from(
        cutter: &mut StreamingGrooveCutter,
        left: &[f32],
        right: &[f32],
        mut start: usize,
        partitions: &[usize],
    ) -> Vec<StreamingGroovePageChunk> {
        let mut pages = Vec::new();
        let mut partition_index = 0;
        while start < left.len() {
            let length = partitions[partition_index % partitions.len()].min(left.len() - start);
            let end = start + length;
            cutter
                .push_chunk(
                    start as u64,
                    &[&left[start..end], &right[start..end]],
                    |page| pages.push(page),
                )
                .unwrap();
            start = end;
            partition_index += 1;
        }
        pages
    }

    fn snapshot_bytes(cutter: &StreamingGrooveCutter) -> Vec<u8> {
        serde_json::to_vec(&cutter.snapshot()).unwrap()
    }

    fn reassemble_core(pages: &[StreamingGroovePageChunk]) -> (Vec<f32>, Vec<f32>) {
        let mut lateral = Vec::new();
        let mut vertical = Vec::new();
        let mut expected_core_start = 0;
        for page in pages {
            page.validate().unwrap();
            assert_eq!(page.core_start_frame(), expected_core_start);
            let local_core_start =
                usize::try_from(page.core_start_frame() - page.stored_start_frame()).unwrap();
            let core_length =
                usize::try_from(page.core_end_frame_exclusive() - page.core_start_frame()).unwrap();
            lateral.extend_from_slice(
                &page.lateral_displacement_m()[local_core_start..local_core_start + core_length],
            );
            vertical.extend_from_slice(
                &page.vertical_displacement_m()[local_core_start..local_core_start + core_length],
            );
            expected_core_start = page.core_end_frame_exclusive();
        }
        (lateral, vertical)
    }

    #[test]
    fn arbitrary_partitions_match_monolithic_cut_at_all_supported_rates() {
        for sample_rate_hz in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            let (left, right) = test_programme(777, sample_rate_hz);
            let expected = GrooveAsset::cut_from_pcm(
                &[&left, &right],
                sample_rate_hz,
                GrooveLayout::default(),
                RecordCutConfig::default(),
            )
            .unwrap();
            let config = config(sample_rate_hz, 2, left.len());
            let mut cutter = StreamingGrooveCutter::new(config).unwrap();
            let pages = push_partitioned(
                &mut cutter,
                &left,
                Some(&right),
                &[1, 31, 2, 127, 5, 64, 3, 211],
            );
            let finalization = cutter.finish().unwrap();
            let (lateral, vertical) = reassemble_core(&pages);

            assert_eq!(
                finalization.output_frame_count(),
                expected.frame_count() as u64
            );
            assert_eq!(lateral, expected.lateral_displacement_m());
            assert_eq!(vertical, expected.vertical_displacement_m());
            assert_eq!(finalization.report(), expected.report());
            assert_eq!(
                finalization.content_identity(),
                expected.provenance().content_identity()
            );

            let metadata = finalization
                .physical_metadata(GrooveGenerationId::new(7).unwrap())
                .unwrap();
            let physical_pages = pages
                .into_iter()
                .map(|page| page.into_physical_page(metadata).unwrap())
                .collect();
            let paged =
                PagedGrooveAsset::new(metadata, metadata.total_range(), physical_pages).unwrap();
            for frame in 0..expected.frame_count() {
                let sample = paged.sample_at(frame as u64).unwrap();
                assert_eq!(
                    sample.lateral_displacement_m,
                    expected.lateral_displacement_m()[frame]
                );
                assert_eq!(
                    sample.vertical_displacement_m,
                    expected.vertical_displacement_m()[frame]
                );
            }
        }
    }

    #[test]
    fn bounded_push_emits_one_page_and_can_pause_with_ready_output() {
        let sample_rate_hz = 8_000.0;
        let (left, right) = test_programme(19, sample_rate_hz);
        let mut cutter_config = config(sample_rate_hz, 2, left.len());
        cutter_config.page_core_frame_count = 1;
        cutter_config.tracing_halo_frames = 1;

        let mut callback_cutter = StreamingGrooveCutter::new(cutter_config).unwrap();
        let mut callback_pages = Vec::new();
        callback_cutter
            .push_chunk(0, &[&left, &right], |page| callback_pages.push(page))
            .unwrap();
        let callback_finalization = callback_cutter.finish().unwrap();

        let mut bounded_cutter = StreamingGrooveCutter::new(cutter_config).unwrap();
        let mut bounded_pages = Vec::new();
        let mut source_offset = 0_usize;
        let mut saw_zero_consumption = false;
        let mut restored_partial_snapshot = false;
        loop {
            let progress = bounded_cutter.progress();
            if source_offset == left.len()
                && progress.produced_output_frame_count() == progress.total_output_frame_count()
                && progress.emitted_page_count() == progress.total_output_frame_count()
            {
                break;
            }
            let result = bounded_cutter
                .push_chunk_until_page(
                    source_offset as u64,
                    &[&left[source_offset..], &right[source_offset..]],
                )
                .unwrap();
            let consumed = result.consumed_source_frame_count() as usize;
            let emitted_page = result.into_emitted_page();
            assert!(consumed > 0 || emitted_page.is_some());
            source_offset += consumed;
            if consumed == 0 {
                saw_zero_consumption = true;
            }
            if let Some(page) = emitted_page {
                bounded_pages.push(page);
                if consumed == 0 && !restored_partial_snapshot {
                    let encoded = serde_json::to_vec(&bounded_cutter.snapshot()).unwrap();
                    let snapshot: StreamingGrooveCutterSnapshot =
                        serde_json::from_slice(&encoded).unwrap();
                    let restored = StreamingGrooveCutter::from_snapshot(&snapshot).unwrap();
                    assert_eq!(restored.snapshot(), snapshot);
                    bounded_cutter = restored;
                    restored_partial_snapshot = true;
                }
            }
        }
        let bounded_finalization = bounded_cutter.finish().unwrap();

        assert!(saw_zero_consumption);
        assert!(restored_partial_snapshot);
        assert_eq!(bounded_pages, callback_pages);
        assert_eq!(bounded_finalization, callback_finalization);
    }

    #[test]
    fn mono_duplicates_the_source_and_matches_the_monolithic_cut() {
        let sample_rate_hz = 96_000.0;
        let (programme, _) = test_programme(521, sample_rate_hz);
        let expected = GrooveAsset::cut_from_pcm(
            &[&programme],
            sample_rate_hz,
            GrooveLayout::default(),
            RecordCutConfig::default(),
        )
        .unwrap();
        let mut cutter =
            StreamingGrooveCutter::new(config(sample_rate_hz, 1, programme.len())).unwrap();
        let pages = push_partitioned(&mut cutter, &programme, None, &[17, 1, 89, 4]);
        let finalization = cutter.finish().unwrap();
        let (lateral, vertical) = reassemble_core(&pages);
        assert_eq!(lateral, expected.lateral_displacement_m());
        assert_eq!(vertical, expected.vertical_displacement_m());
        assert_eq!(finalization.report(), expected.report());
        assert_eq!(
            finalization.content_identity(),
            expected.provenance().content_identity()
        );
    }

    #[test]
    fn the_declared_final_source_frame_releases_the_exact_clamped_tail() {
        for sample_rate_hz in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            let (left, right) = test_programme(41, sample_rate_hz);
            let expected = GrooveAsset::cut_from_pcm(
                &[&left, &right],
                sample_rate_hz,
                GrooveLayout::default(),
                RecordCutConfig::default(),
            )
            .unwrap();
            let mut cutter =
                StreamingGrooveCutter::new(config(sample_rate_hz, 2, left.len())).unwrap();
            let mut pages = Vec::new();
            let before_tail = cutter
                .push_chunk(
                    0,
                    &[&left[..left.len() - 1], &right[..right.len() - 1]],
                    |page| pages.push(page),
                )
                .unwrap();
            assert!(before_tail.produced_output_frame_count() < expected.frame_count() as u64);
            let complete = cutter
                .push_chunk(
                    (left.len() - 1) as u64,
                    &[&left[left.len() - 1..], &right[right.len() - 1..]],
                    |page| pages.push(page),
                )
                .unwrap();
            assert_eq!(
                complete.produced_output_frame_count(),
                expected.frame_count() as u64
            );
            let finalization = cutter.finish().unwrap();
            let (lateral, vertical) = reassemble_core(&pages);
            assert_eq!(lateral, expected.lateral_displacement_m());
            assert_eq!(vertical, expected.vertical_displacement_m());
            assert_eq!(finalization.report(), expected.report());
        }
    }

    #[test]
    fn snapshot_round_trip_resumes_with_identical_pages_report_and_identity() {
        let sample_rate_hz = 48_000.0;
        let (left, right) = test_programme(2_003, sample_rate_hz);
        let config = config(sample_rate_hz, 2, left.len());
        let mut original = StreamingGrooveCutter::new(config).unwrap();
        let mut pages_before_snapshot = Vec::new();
        let progress_at_snapshot = original
            .push_chunk(0, &[&left[..301], &right[..301]], |page| {
                pages_before_snapshot.push(page)
            })
            .unwrap();
        assert!(!pages_before_snapshot.is_empty());
        let encoded_snapshot = serde_json::to_vec(&original.snapshot()).unwrap();
        let decoded_snapshot: StreamingGrooveCutterSnapshot =
            serde_json::from_slice(&encoded_snapshot).unwrap();
        assert_eq!(
            decoded_snapshot.version(),
            STREAMING_GROOVE_CUTTER_SNAPSHOT_VERSION
        );
        assert_eq!(decoded_snapshot.config(), config);

        let original_pages =
            push_stereo_from(&mut original, &left, &right, 301, &[3, 97, 1, 256, 11]);
        let original_finalization = original.finish().unwrap();

        let mut restored = StreamingGrooveCutter::from_snapshot(&decoded_snapshot).unwrap();
        assert_eq!(restored.progress(), progress_at_snapshot);
        let restored_pages =
            push_stereo_from(&mut restored, &left, &right, 301, &[3, 97, 1, 256, 11]);
        let restored_finalization = restored.finish().unwrap();
        assert_eq!(restored_pages, original_pages);
        assert_eq!(restored_finalization, original_finalization);
        assert_eq!(restored.snapshot(), original.snapshot());

        let mut restored_in_place = StreamingGrooveCutter::new(config).unwrap();
        restored_in_place.restore(&decoded_snapshot).unwrap();
        assert_eq!(restored_in_place.snapshot(), decoded_snapshot);
    }

    #[test]
    fn rejected_chunks_and_finishes_leave_the_snapshot_bit_equal() {
        let sample_rate_hz = 48_000.0;
        let (left, right) = test_programme(64, sample_rate_hz);
        let mut cutter = StreamingGrooveCutter::new(config(sample_rate_hz, 2, left.len())).unwrap();
        cutter
            .push_chunk(0, &[&left[..10], &right[..10]], |_| {})
            .unwrap();

        let before = snapshot_bytes(&cutter);
        assert!(matches!(
            cutter.push_chunk(12, &[&left[10..11], &right[10..11]], |_| {}),
            Err(StreamingGrooveCutterError::UnexpectedSourceFrame {
                expected_frame: 10,
                received_frame: 12
            })
        ));
        assert_eq!(snapshot_bytes(&cutter), before);

        assert!(matches!(
            cutter.push_chunk(9, &[&left[9..10], &right[9..10]], |_| {}),
            Err(StreamingGrooveCutterError::UnexpectedSourceFrame {
                expected_frame: 10,
                received_frame: 9
            })
        ));
        assert_eq!(snapshot_bytes(&cutter), before);

        let nonfinite = [f32::NAN];
        assert!(matches!(
            cutter.push_chunk(10, &[&nonfinite, &right[10..11]], |_| {}),
            Err(StreamingGrooveCutterError::NonfiniteProgramme)
        ));
        assert_eq!(snapshot_bytes(&cutter), before);

        assert!(matches!(
            cutter.push_chunk(10, &[&left[10..11]], |_| {}),
            Err(StreamingGrooveCutterError::SourceChannelCountMismatch)
        ));
        assert_eq!(snapshot_bytes(&cutter), before);

        assert!(matches!(
            cutter.push_chunk(10, &[&left[10..12], &right[10..11]], |_| {}),
            Err(StreamingGrooveCutterError::SourceChannelLengthMismatch)
        ));
        assert_eq!(snapshot_bytes(&cutter), before);

        let too_long = vec![0.0_f32; left.len() - 10 + 1];
        assert!(matches!(
            cutter.push_chunk(10, &[&too_long, &too_long], |_| {}),
            Err(StreamingGrooveCutterError::SourceExceedsDeclaredLength)
        ));
        assert_eq!(snapshot_bytes(&cutter), before);

        assert!(matches!(
            cutter.finish(),
            Err(StreamingGrooveCutterError::IncompleteSource {
                expected_frame_count: 64,
                accepted_frame_count: 10
            })
        ));
        assert_eq!(snapshot_bytes(&cutter), before);

        cutter
            .push_chunk(10, &[&left[10..], &right[10..]], |_| {})
            .unwrap();
        cutter.finish().unwrap();
        let finished = snapshot_bytes(&cutter);
        assert!(matches!(
            cutter.push_chunk(64, &[&[], &[]], |_| {}),
            Err(StreamingGrooveCutterError::AlreadyFinished)
        ));
        assert_eq!(snapshot_bytes(&cutter), finished);
        assert!(matches!(
            cutter.finish(),
            Err(StreamingGrooveCutterError::AlreadyFinished)
        ));
        assert_eq!(snapshot_bytes(&cutter), finished);
    }

    #[test]
    fn invalid_snapshot_restore_is_transactional() {
        let sample_rate_hz = 96_000.0;
        let (left, right) = test_programme(333, sample_rate_hz);
        let config = config(sample_rate_hz, 2, left.len());
        let mut source = StreamingGrooveCutter::new(config).unwrap();
        source
            .push_chunk(0, &[&left[..111], &right[..111]], |_| {})
            .unwrap();
        let snapshot = source.snapshot();

        let mut target = StreamingGrooveCutter::new(config).unwrap();
        target
            .push_chunk(0, &[&left[..17], &right[..17]], |_| {})
            .unwrap();
        let target_before = snapshot_bytes(&target);

        let mut wrong_version = snapshot.clone();
        wrong_version.version += 1;
        assert!(matches!(
            target.restore(&wrong_version),
            Err(StreamingGrooveCutterError::UnsupportedSnapshotVersion { .. })
        ));
        assert_eq!(snapshot_bytes(&target), target_before);

        let mut invalid_state = snapshot.clone();
        invalid_state.pending_lateral_displacement_m.pop_back();
        assert!(matches!(
            target.restore(&invalid_state),
            Err(StreamingGrooveCutterError::InvalidSnapshot)
        ));
        assert_eq!(snapshot_bytes(&target), target_before);

        let mut wrong_config = snapshot;
        wrong_config.config.page_core_frame_count += 1;
        assert!(matches!(
            target.restore(&wrong_config),
            Err(StreamingGrooveCutterError::SnapshotConfigMismatch)
        ));
        assert_eq!(snapshot_bytes(&target), target_before);
    }

    #[test]
    fn long_stream_stays_within_all_reported_history_bounds() {
        let layout = GrooveLayout::default();
        let frames_per_revolution = layout.groove_sample_rate_hz * 60.0 / layout.nominal_rpm;
        let total_source_frames = frames_per_revolution.ceil() as usize + 2_048;
        let mut config = config(192_000.0, 2, total_source_frames);
        config.page_core_frame_count = 4_096;
        let source_bound = config.maximum_source_history_frame_count();
        let clearance_bound = config.maximum_clearance_history_frame_count().unwrap();
        let pending_page_bound = config.maximum_pending_page_frame_count().unwrap();
        let mut cutter = StreamingGrooveCutter::new(config).unwrap();
        let mut accepted = 0_usize;
        let mut reached_full_clearance_history = false;
        while accepted < total_source_frames {
            let end = (accepted + 2_047).min(total_source_frames);
            let left: Vec<f32> = (accepted..end)
                .map(|frame| ((frame as f64 * 0.013_731).sin() * 0.4) as f32)
                .collect();
            let right: Vec<f32> = (accepted..end)
                .map(|frame| ((frame as f64 * 0.019_337).cos() * 0.3) as f32)
                .collect();
            let progress = cutter
                .push_chunk(accepted as u64, &[&left, &right], |page| {
                    assert!(page.lateral_displacement_m().len() as u64 <= pending_page_bound);
                })
                .unwrap();
            assert!(progress.source_history_frame_count() <= source_bound);
            assert!(progress.clearance_history_frame_count() <= clearance_bound);
            assert!(progress.pending_page_frame_count() <= pending_page_bound);
            reached_full_clearance_history |=
                progress.clearance_history_frame_count() == clearance_bound;
            accepted = end;
        }
        assert!(reached_full_clearance_history);
        let finalization = cutter.finish().unwrap();
        assert_eq!(
            finalization.output_frame_count(),
            config.expected_output_frame_count().unwrap()
        );
        assert!(finalization
            .report()
            .minimum_adjacent_turn_clearance_m
            .is_some());
        assert!(cutter.progress().source_history_frame_count() <= source_bound);
        assert!(cutter.progress().clearance_history_frame_count() <= clearance_bound);
        assert!(cutter.progress().pending_page_frame_count() <= pending_page_bound);
    }

    #[test]
    fn programme_shorter_than_one_revolution_keeps_no_clearance_history() {
        let sample_rate_hz = 48_000.0;
        let (left, right) = test_programme(1_001, sample_rate_hz);
        let config = config(sample_rate_hz, 2, left.len());
        assert_eq!(config.maximum_clearance_history_frame_count().unwrap(), 0);
        let mut cutter = StreamingGrooveCutter::new(config).unwrap();
        cutter.push_chunk(0, &[&left, &right], |_| {}).unwrap();
        assert_eq!(cutter.progress().clearance_history_frame_count(), 0);
        assert!(cutter
            .finish()
            .unwrap()
            .report()
            .minimum_adjacent_turn_clearance_m
            .is_none());
    }

    #[test]
    fn fractional_one_revolution_clearance_matches_the_monolithic_report() {
        let sample_rate_hz = 48_000.0;
        let (left, right) = test_programme(2_101, sample_rate_hz);
        let layout = GrooveLayout {
            nominal_rpm: 3_599.0,
            ..GrooveLayout::default()
        };
        let expected = GrooveAsset::cut_from_pcm(
            &[&left, &right],
            sample_rate_hz,
            layout,
            RecordCutConfig::default(),
        )
        .unwrap();
        assert!(expected
            .report()
            .minimum_adjacent_turn_clearance_m
            .is_some());
        let mut config = config(sample_rate_hz, 2, left.len());
        config.layout = layout;
        let mut cutter = StreamingGrooveCutter::new(config).unwrap();
        let pages = push_partitioned(&mut cutter, &left, Some(&right), &[509, 1, 17, 4, 263, 2]);
        let finalization = cutter.finish().unwrap();
        let (lateral, vertical) = reassemble_core(&pages);
        assert_eq!(lateral, expected.lateral_displacement_m());
        assert_eq!(vertical, expected.vertical_displacement_m());
        assert_eq!(finalization.report(), expected.report());
        assert_eq!(
            finalization.content_identity(),
            expected.provenance().content_identity()
        );
    }

    #[test]
    fn raw_page_chunks_are_versioned_and_require_matching_final_metadata() {
        let sample_rate_hz = 192_000.0;
        let (left, right) = test_programme(1_009, sample_rate_hz);
        let mut cutter = StreamingGrooveCutter::new(config(sample_rate_hz, 2, left.len())).unwrap();
        let pages = push_partitioned(&mut cutter, &left, Some(&right), &[101, 3, 211]);
        let finalization = cutter.finish().unwrap();
        let encoded = serde_json::to_vec(&pages[0]).unwrap();
        let decoded: StreamingGroovePageChunk = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, pages[0]);
        assert_eq!(
            decoded.format_version(),
            STREAMING_GROOVE_PAGE_CHUNK_FORMAT_VERSION
        );

        let mut wrong_version = decoded.clone();
        wrong_version.format_version += 1;
        let metadata = finalization
            .physical_metadata(GrooveGenerationId::new(1).unwrap())
            .unwrap();
        assert!(matches!(
            wrong_version.into_physical_page(metadata),
            Err(StreamingGrooveCutterError::InvalidRawPageChunk)
        ));

        let wrong_halo_metadata = PhysicalGrooveMetadata::new(
            GrooveGenerationId::new(2).unwrap(),
            finalization.content_identity(),
            PhysicalGrooveCutMetadata::new(
                finalization.config().layout,
                finalization.config().cut,
                finalization.report(),
            ),
            finalization.output_frame_count(),
            finalization.config().tracing_halo_frames + 1,
        )
        .unwrap();
        assert!(matches!(
            decoded.into_physical_page(wrong_halo_metadata),
            Err(StreamingGrooveCutterError::RawPageMetadataMismatch)
        ));
    }

    #[test]
    fn one_page_pull_snapshot_retains_complete_interpolation_support() {
        let sample_rate_hz = 44_100.0;
        let (left, right) = test_programme(257, sample_rate_hz);
        let mut cutter_config = config(sample_rate_hz, 2, left.len());
        cutter_config.page_core_frame_count = 17;
        cutter_config.tracing_halo_frames = 3;
        let mut cutter = StreamingGrooveCutter::new(cutter_config).unwrap();
        for source_frame in 0..left.len() {
            let result = cutter
                .push_chunk_until_page(
                    source_frame as u64,
                    &[
                        &left[source_frame..=source_frame],
                        &right[source_frame..=source_frame],
                    ],
                )
                .unwrap();
            if result.emitted_page().is_some() {
                let snapshot = cutter.snapshot();
                assert_eq!(
                    cutter.progress().source_history_frame_count(),
                    MAX_STREAMING_GROOVE_SOURCE_HISTORY_FRAMES
                );
                let restored = StreamingGrooveCutter::from_snapshot(&snapshot).unwrap();
                assert_eq!(restored.snapshot(), snapshot);
                return;
            }
        }
        panic!("the cutter did not emit a page");
    }

    #[test]
    fn invalid_configuration_is_rejected_before_allocation() {
        let mut invalid = config(48_000.0, 2, 64);
        invalid.source_sample_rate_hz = 192_000.1;
        assert!(matches!(
            StreamingGrooveCutter::new(invalid),
            Err(StreamingGrooveCutterError::UnsupportedSourceSampleRate)
        ));

        invalid = config(48_000.0, 3, 64);
        assert!(matches!(
            StreamingGrooveCutter::new(invalid),
            Err(StreamingGrooveCutterError::InvalidSourceChannelCount)
        ));

        invalid = config(48_000.0, 2, 3);
        assert!(matches!(
            StreamingGrooveCutter::new(invalid),
            Err(StreamingGrooveCutterError::InvalidTotalSourceFrameCount)
        ));

        invalid = config(48_000.0, 2, 64);
        invalid.page_core_frame_count = 0;
        assert!(matches!(
            StreamingGrooveCutter::new(invalid),
            Err(StreamingGrooveCutterError::InvalidPageCoreFrameCount)
        ));

        invalid = config(48_000.0, 2, 64);
        invalid.tracing_halo_frames = 0;
        assert!(matches!(
            StreamingGrooveCutter::new(invalid),
            Err(StreamingGrooveCutterError::InvalidTracingHaloFrameCount)
        ));
    }
}
