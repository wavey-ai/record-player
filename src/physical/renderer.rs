//! Renders the physical player at a supported host sample rate.
//!
//! The player always advances at 192 kHz. The renderer calculates the exact
//! internal-frame demand for each host block. It uses one bounded scratch buffer
//! that is allocated during construction.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    GrooveAsset, GrooveContentIdentity, GrooveGenerationId, PagedGrooveCache,
    PagedGroovePrefetchPlan, PhysicalGrooveSource, PhysicalGrooveSourceIdentity, PhysicalProfile,
    PhysicalRecordPlayer, PhysicalRecordPlayerError, PhysicalRecordPlayerSnapshot,
    PhysicalRenderTelemetry, PlayerControlIngressReport, RealtimePagedGrooveCache,
    StereoOutputResampler, StereoOutputResamplerError, StereoOutputResamplerSnapshot,
};
use crate::spsc::TimedPlayerControlConsumer;
use crate::timed_control::TimedPlayerControl;

pub const PHYSICAL_HOST_RENDERER_SNAPSHOT_VERSION: u32 = 3;
pub const MAXIMUM_HOST_VOLTS_PER_FULL_SCALE: f64 = 1_000.0;

/// Defines the explicit boundary between phono volts and host full scale.
///
/// The host renderer divides each resampled phono voltage by
/// `volts_per_full_scale`. It then clips the result to `[-1.0, 1.0]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalHostOutputConfig {
    pub volts_per_full_scale: f64,
}

impl PhysicalHostOutputConfig {
    pub fn validate(self) -> Result<Self, PhysicalHostOutputConfigError> {
        if !self.volts_per_full_scale.is_finite()
            || !(f64::MIN_POSITIVE..=MAXIMUM_HOST_VOLTS_PER_FULL_SCALE)
                .contains(&self.volts_per_full_scale)
        {
            return Err(PhysicalHostOutputConfigError::InvalidVoltsPerFullScale);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum PhysicalHostOutputConfigError {
    #[error("host volts per full scale must be finite and positive")]
    InvalidVoltsPerFullScale,
}

/// Reports the voltage-to-host conversion for one successful render block.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalHostOutputTelemetry {
    pub volts_per_full_scale: f64,
    pub peak_unclipped_abs_output_v: [f64; 2],
    pub clipped_samples: [u64; 2],
    pub total_clipped_samples: [u64; 2],
}

impl PhysicalHostOutputTelemetry {
    fn empty(config: PhysicalHostOutputConfig, total_clipped_samples: [u64; 2]) -> Self {
        Self {
            volts_per_full_scale: config.volts_per_full_scale,
            peak_unclipped_abs_output_v: [0.0; 2],
            clipped_samples: [0; 2],
            total_clipped_samples,
        }
    }
}

/// Reports the exact work completed for one host block.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalHostRenderReport {
    pub output_sample_rate_hz: u32,
    pub rendered_host_frames: usize,
    pub rendered_internal_frames: usize,
    pub host_output: PhysicalHostOutputTelemetry,
    pub player: PhysicalRenderTelemetry,
}

/// Stores all renderer state that can change future output.
///
/// Restore requires the same groove content and source identity.
/// Paged caches can contain different resident pages when metadata and generation match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalHostRendererSnapshot {
    pub version: u32,
    pub output_sample_rate_hz: u32,
    pub host_output_config: PhysicalHostOutputConfig,
    pub host_output_telemetry: PhysicalHostOutputTelemetry,
    pub loaded_groove_identity: Option<GrooveContentIdentity>,
    pub loaded_source_identity: Option<PhysicalGrooveSourceIdentity>,
    pub player: PhysicalRecordPlayerSnapshot,
    pub resampler: StereoOutputResamplerSnapshot,
}

/// Converts consecutive physical-player frames to a supported host rate.
///
/// `render_interleaved` does not allocate. Host blocks can exceed the internal
/// scratch capacity because the renderer divides them into bounded chunks.
pub struct PhysicalHostRenderer {
    player: PhysicalRecordPlayer,
    resampler: StereoOutputResampler,
    host_output_config: PhysicalHostOutputConfig,
    host_output_telemetry: PhysicalHostOutputTelemetry,
    internal_scratch: Box<[f32]>,
    host_scratch: Box<[f32]>,
}

impl PhysicalHostRenderer {
    pub fn new(
        profile: PhysicalProfile,
        output_sample_rate_hz: u32,
        host_output_config: PhysicalHostOutputConfig,
    ) -> Result<Self, PhysicalHostRendererError> {
        let host_output_config = host_output_config.validate()?;
        let maximum_render_frames = profile.config.solver.maximum_render_frames;
        let resampler = StereoOutputResampler::new(output_sample_rate_hz)?;
        let player = PhysicalRecordPlayer::new(profile)?;
        let sample_capacity = maximum_render_frames
            .checked_mul(2)
            .ok_or(PhysicalHostRendererError::ScratchCapacityOverflow)?;
        Ok(Self {
            player,
            resampler,
            host_output_config,
            host_output_telemetry: PhysicalHostOutputTelemetry::empty(host_output_config, [0; 2]),
            internal_scratch: vec![0.0; sample_capacity].into_boxed_slice(),
            host_scratch: vec![0.0; sample_capacity].into_boxed_slice(),
        })
    }

    pub fn profile(&self) -> &PhysicalProfile {
        self.player.profile()
    }

    pub fn player(&self) -> &PhysicalRecordPlayer {
        &self.player
    }

    pub fn output_sample_rate_hz(&self) -> u32 {
        self.resampler.output_sample_rate_hz()
    }

    pub const fn host_output_config(&self) -> PhysicalHostOutputConfig {
        self.host_output_config
    }

    pub const fn host_output_telemetry(&self) -> PhysicalHostOutputTelemetry {
        self.host_output_telemetry
    }

    pub fn latency_internal_frames(&self) -> usize {
        self.resampler.latency_input_frames()
    }

    pub fn latency_seconds(&self) -> f64 {
        self.resampler.latency_seconds()
    }

    pub fn scratch_capacity_frames(&self) -> usize {
        self.internal_scratch.len() / 2
    }

    pub fn maximum_host_render_frames(&self) -> usize {
        self.host_scratch.len() / 2
    }

    pub fn current_internal_frame(&self) -> u64 {
        self.player.current_internal_frame()
    }

    pub fn rendered_host_frames(&self) -> u64 {
        self.resampler.output_frames_produced()
    }

    pub fn loaded_groove_identity(&self) -> Option<GrooveContentIdentity> {
        self.player.loaded_groove_identity()
    }

    pub fn loaded_source_identity(&self) -> Option<PhysicalGrooveSourceIdentity> {
        self.player.loaded_source_identity()
    }

    pub fn telemetry(&self) -> PhysicalRenderTelemetry {
        self.player.telemetry()
    }

    pub fn paged_prefetch_plan(
        &self,
        render_frame_horizon: u32,
        adjacent_turn_offsets: &[i64],
    ) -> Result<Option<PagedGroovePrefetchPlan>, PhysicalRecordPlayerError> {
        self.player
            .paged_prefetch_plan(render_frame_horizon, adjacent_turn_offsets)
    }

    pub fn internal_frames_required(
        &self,
        host_frames: usize,
    ) -> Result<usize, PhysicalHostRendererError> {
        self.validate_clock_alignment()?;
        Ok(self.resampler.input_frames_required(host_frames)?)
    }

    pub fn load_groove(
        &mut self,
        groove: Arc<GrooveAsset>,
    ) -> Result<Option<Arc<GrooveAsset>>, PhysicalRecordPlayerError> {
        self.player.load_groove(groove)
    }

    pub fn load_paged_groove(
        &mut self,
        cache: Arc<PagedGrooveCache>,
    ) -> Result<Option<PhysicalGrooveSource>, PhysicalRecordPlayerError> {
        self.player.load_paged_groove(cache)
    }

    /// Loads an explicit requested generation for stale-publication handling.
    pub fn load_paged_groove_generation(
        &mut self,
        cache: Arc<PagedGrooveCache>,
        requested_generation: GrooveGenerationId,
    ) -> Result<Option<PhysicalGrooveSource>, PhysicalRecordPlayerError> {
        self.player
            .load_paged_groove_generation(cache, requested_generation)
    }

    pub fn load_realtime_paged_groove(
        &mut self,
        cache: RealtimePagedGrooveCache,
    ) -> Result<Option<PhysicalGrooveSource>, PhysicalRecordPlayerError> {
        self.player.load_realtime_paged_groove(cache)
    }

    /// Loads an explicit requested generation for stale-generation handling.
    pub fn load_realtime_paged_groove_generation(
        &mut self,
        cache: RealtimePagedGrooveCache,
        requested_generation: GrooveGenerationId,
    ) -> Result<Option<PhysicalGrooveSource>, PhysicalRecordPlayerError> {
        self.player
            .load_realtime_paged_groove_generation(cache, requested_generation)
    }

    pub fn load_source(
        &mut self,
        source: PhysicalGrooveSource,
    ) -> Result<Option<PhysicalGrooveSource>, PhysicalRecordPlayerError> {
        self.player.load_source(source)
    }

    pub fn replace_loaded_paged_cache_snapshot(
        &mut self,
        new_cache: Arc<PagedGrooveCache>,
    ) -> Result<Arc<PagedGrooveCache>, PhysicalRecordPlayerError> {
        self.player.replace_loaded_paged_cache_snapshot(new_cache)
    }

    pub fn unload_groove(&mut self) -> Option<Arc<GrooveAsset>> {
        self.player.unload_groove()
    }

    pub fn unload_source(&mut self) -> Option<PhysicalGrooveSource> {
        self.player.unload_source()
    }

    /// Borrows the fixed cache between render calls.
    pub fn realtime_paged_cache(&self) -> Option<&RealtimePagedGrooveCache> {
        self.player.realtime_paged_cache()
    }

    /// Mutably borrows the fixed cache between render calls.
    pub fn realtime_paged_cache_mut(&mut self) -> Option<&mut RealtimePagedGrooveCache> {
        self.player.realtime_paged_cache_mut()
    }

    pub fn set_groove_frame_position(
        &mut self,
        position: f64,
    ) -> Result<(), PhysicalRecordPlayerError> {
        self.player.set_groove_frame_position(position)
    }

    pub fn reset_transport(
        &mut self,
        platter_rate: f64,
        record_rate: f64,
        platter_angle_turns: f64,
        record_angle_turns: f64,
    ) -> Result<(), PhysicalRecordPlayerError> {
        self.player.reset_transport(
            platter_rate,
            record_rate,
            platter_angle_turns,
            record_angle_turns,
        )
    }

    pub fn enqueue_control(
        &mut self,
        event: TimedPlayerControl,
    ) -> Result<(), PhysicalRecordPlayerError> {
        self.player.enqueue_control(event)
    }

    /// Moves available mailbox controls into the player's sample timeline.
    pub fn drain_control_ingress(
        &mut self,
        consumer: &mut TimedPlayerControlConsumer,
    ) -> PlayerControlIngressReport {
        self.player.drain_control_ingress(consumer)
    }

    pub fn snapshot(&self) -> PhysicalHostRendererSnapshot {
        debug_assert_eq!(
            self.player.current_internal_frame(),
            self.resampler.input_frames_consumed()
        );
        PhysicalHostRendererSnapshot {
            version: PHYSICAL_HOST_RENDERER_SNAPSHOT_VERSION,
            output_sample_rate_hz: self.output_sample_rate_hz(),
            host_output_config: self.host_output_config,
            host_output_telemetry: self.host_output_telemetry,
            loaded_groove_identity: self.loaded_groove_identity(),
            loaded_source_identity: self.loaded_source_identity(),
            player: self.player.snapshot(),
            resampler: self.resampler.snapshot(),
        }
    }

    /// Restores a validated snapshot without changing the scratch allocation.
    pub fn restore(
        &mut self,
        snapshot: &PhysicalHostRendererSnapshot,
    ) -> Result<(), PhysicalHostRendererError> {
        if snapshot.version != PHYSICAL_HOST_RENDERER_SNAPSHOT_VERSION {
            return Err(PhysicalHostRendererError::UnsupportedSnapshotVersion {
                version: snapshot.version,
            });
        }
        if snapshot.output_sample_rate_hz != self.output_sample_rate_hz() {
            return Err(PhysicalHostRendererError::SnapshotOutputRateMismatch);
        }
        snapshot.host_output_config.validate()?;
        if snapshot.host_output_config != self.host_output_config {
            return Err(PhysicalHostRendererError::SnapshotHostOutputConfigMismatch);
        }
        validate_host_output_telemetry(
            snapshot.host_output_telemetry,
            snapshot.host_output_config,
        )?;
        if snapshot.loaded_groove_identity != snapshot.player.loaded_groove_identity() {
            return Err(PhysicalHostRendererError::InvalidSnapshotIdentity);
        }
        if snapshot
            .loaded_source_identity
            .map(|identity| identity.canonical_content_identity)
            != snapshot.loaded_groove_identity
        {
            return Err(PhysicalHostRendererError::InvalidSnapshotIdentity);
        }
        if snapshot.loaded_groove_identity != self.loaded_groove_identity() {
            return Err(PhysicalHostRendererError::SnapshotGrooveMismatch);
        }
        if snapshot.loaded_source_identity != self.loaded_source_identity() {
            return Err(PhysicalHostRendererError::SnapshotSourceMismatch);
        }
        if snapshot.player.current_internal_frame() != snapshot.resampler.input_frames_consumed {
            return Err(PhysicalHostRendererError::InvalidSnapshotClock);
        }

        // Validate the converter before the player changes. Both restore methods
        // validate their complete input before they update live state.
        let restored_resampler = StereoOutputResampler::from_snapshot(&snapshot.resampler)?;
        self.player
            .restore(&snapshot.player)
            .map_err(|source| PhysicalHostRendererError::PlayerRestore { source })?;
        self.resampler = restored_resampler;
        self.host_output_telemetry = snapshot.host_output_telemetry;
        self.validate_clock_alignment()?;
        Ok(())
    }

    /// Renders one complete interleaved stereo host block.
    ///
    /// The complete block commits only after all bounded chunks succeed.
    pub fn render_interleaved(
        &mut self,
        output: &mut [f32],
    ) -> Result<PhysicalHostRenderReport, PhysicalHostRendererError> {
        if !output.len().is_multiple_of(2) {
            return Err(PhysicalHostRendererError::OutputMustBeStereo);
        }
        self.validate_clock_alignment()?;
        let host_frames = output.len() / 2;
        if host_frames > self.maximum_host_render_frames() {
            return Err(PhysicalHostRendererError::HostBlockTooLarge {
                maximum: self.maximum_host_render_frames(),
            });
        }
        let internal_frames = self.resampler.input_frames_required(host_frames)?;
        let expected_host_frames = self.resampler.expected_output_frames(internal_frames)?;
        if expected_host_frames != host_frames {
            return Err(PhysicalHostRendererError::FrameAccountingMismatch {
                expected_host_frames: host_frames,
                calculated_host_frames: expected_host_frames,
            });
        }

        let player_checkpoint = self.player.render_checkpoint();
        let resampler_checkpoint = self.resampler.snapshot();
        let render_result = (|| {
            let mut completed_internal_frames = 0;
            let mut completed_host_frames = 0;
            while completed_internal_frames < internal_frames {
                let chunk_frames = (internal_frames - completed_internal_frames)
                    .min(self.scratch_capacity_frames());
                let chunk_samples = chunk_frames * 2;
                let scratch = &mut self.internal_scratch[..chunk_samples];
                let player_telemetry =
                    self.player
                        .render_internal_interleaved(scratch)
                        .map_err(|source| PhysicalHostRendererError::PlayerRender {
                            completed_internal_frames,
                            completed_host_frames,
                            source,
                        })?;
                debug_assert_eq!(player_telemetry.rendered_internal_frames, chunk_frames);

                let chunk_host_frames = self.resampler.expected_output_frames(chunk_frames)?;
                let output_start = completed_host_frames * 2;
                let output_end = output_start + chunk_host_frames * 2;
                let process = self
                    .resampler
                    .process_interleaved(scratch, &mut self.host_scratch[output_start..output_end])
                    .map_err(|source| PhysicalHostRendererError::ResamplerRender {
                        completed_internal_frames,
                        completed_host_frames,
                        source,
                    })?;
                if process.input_frames != chunk_frames
                    || process.output_frames != chunk_host_frames
                {
                    return Err(PhysicalHostRendererError::FrameAccountingMismatch {
                        expected_host_frames: chunk_host_frames,
                        calculated_host_frames: process.output_frames,
                    });
                }
                completed_internal_frames += chunk_frames;
                completed_host_frames += chunk_host_frames;
            }
            if completed_host_frames != host_frames {
                return Err(PhysicalHostRendererError::FrameAccountingMismatch {
                    expected_host_frames: host_frames,
                    calculated_host_frames: completed_host_frames,
                });
            }
            self.validate_clock_alignment()?;
            Ok((completed_internal_frames, completed_host_frames))
        })();
        let (completed_internal_frames, completed_host_frames) = match render_result {
            Ok(completed) => completed,
            Err(error) => {
                self.player
                    .restore_render_checkpoint(player_checkpoint)
                    .map_err(|source| PhysicalHostRendererError::PlayerRestore { source })?;
                self.resampler.restore(&resampler_checkpoint)?;
                return Err(error);
            }
        };
        let host_output = match convert_host_output_in_place(
            self.host_output_config,
            &mut self.host_scratch[..output.len()],
            self.host_output_telemetry.total_clipped_samples,
        ) {
            Ok(telemetry) => telemetry,
            Err(error) => {
                self.player
                    .restore_render_checkpoint(player_checkpoint)
                    .map_err(|source| PhysicalHostRendererError::PlayerRestore { source })?;
                self.resampler.restore(&resampler_checkpoint)?;
                return Err(error);
            }
        };
        output.copy_from_slice(&self.host_scratch[..output.len()]);
        if host_frames != 0 {
            self.host_output_telemetry = host_output;
        }

        Ok(PhysicalHostRenderReport {
            output_sample_rate_hz: self.output_sample_rate_hz(),
            rendered_host_frames: completed_host_frames,
            rendered_internal_frames: completed_internal_frames,
            host_output,
            player: self.player.telemetry(),
        })
    }

    fn validate_clock_alignment(&self) -> Result<(), PhysicalHostRendererError> {
        let player_internal_frame = self.player.current_internal_frame();
        let resampler_input_frames = self.resampler.input_frames_consumed();
        if player_internal_frame != resampler_input_frames {
            return Err(PhysicalHostRendererError::ClockMismatch {
                player_internal_frame,
                resampler_input_frames,
            });
        }
        Ok(())
    }
}

fn convert_host_output_in_place(
    config: PhysicalHostOutputConfig,
    output_v: &mut [f32],
    previous_total_clipped_samples: [u64; 2],
) -> Result<PhysicalHostOutputTelemetry, PhysicalHostRendererError> {
    debug_assert!(output_v.len().is_multiple_of(2));
    let mut telemetry = PhysicalHostOutputTelemetry::empty(config, previous_total_clipped_samples);
    for frame in output_v.chunks_exact(2) {
        for channel in 0..2 {
            let volts = f64::from(frame[channel]);
            if !volts.is_finite() {
                return Err(PhysicalHostRendererError::NonFiniteHostOutputVoltage { channel });
            }
            telemetry.peak_unclipped_abs_output_v[channel] =
                telemetry.peak_unclipped_abs_output_v[channel].max(volts.abs());
            if volts.abs() > config.volts_per_full_scale {
                telemetry.clipped_samples[channel] = telemetry.clipped_samples[channel]
                    .checked_add(1)
                    .ok_or(PhysicalHostRendererError::HostClipCounterOverflow)?;
            }
        }
    }
    for channel in 0..2 {
        telemetry.total_clipped_samples[channel] = previous_total_clipped_samples[channel]
            .checked_add(telemetry.clipped_samples[channel])
            .ok_or(PhysicalHostRendererError::HostClipCounterOverflow)?;
    }
    for sample in output_v {
        let host_sample = f64::from(*sample) / config.volts_per_full_scale;
        *sample = host_sample.clamp(-1.0, 1.0) as f32;
    }
    Ok(telemetry)
}

fn validate_host_output_telemetry(
    telemetry: PhysicalHostOutputTelemetry,
    config: PhysicalHostOutputConfig,
) -> Result<(), PhysicalHostRendererError> {
    if telemetry.volts_per_full_scale != config.volts_per_full_scale
        || telemetry
            .peak_unclipped_abs_output_v
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        || (0..2).any(|channel| {
            telemetry.clipped_samples[channel] > telemetry.total_clipped_samples[channel]
        })
    {
        return Err(PhysicalHostRendererError::InvalidSnapshotHostOutputTelemetry);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum PhysicalHostRendererError {
    #[error(transparent)]
    HostOutputConfig(#[from] PhysicalHostOutputConfigError),
    #[error(transparent)]
    Player(#[from] PhysicalRecordPlayerError),
    #[error(transparent)]
    Resampler(#[from] StereoOutputResamplerError),
    #[error("renderer scratch capacity exceeds the supported size")]
    ScratchCapacityOverflow,
    #[error("host output must contain complete interleaved stereo frames")]
    OutputMustBeStereo,
    #[error("host render block exceeds {maximum} frames")]
    HostBlockTooLarge { maximum: usize },
    #[error("host output voltage for channel {channel} is not finite")]
    NonFiniteHostOutputVoltage { channel: usize },
    #[error(
        "player frame {player_internal_frame} does not match resampler frame {resampler_input_frames}"
    )]
    ClockMismatch {
        player_internal_frame: u64,
        resampler_input_frames: u64,
    },
    #[error(
        "frame accounting expected {expected_host_frames} host frames but calculated {calculated_host_frames}"
    )]
    FrameAccountingMismatch {
        expected_host_frames: usize,
        calculated_host_frames: usize,
    },
    #[error(
        "player rendering stopped after {completed_internal_frames} internal frames and {completed_host_frames} host frames"
    )]
    PlayerRender {
        completed_internal_frames: usize,
        completed_host_frames: usize,
        #[source]
        source: PhysicalRecordPlayerError,
    },
    #[error(
        "output conversion stopped after {completed_internal_frames} internal frames and {completed_host_frames} host frames"
    )]
    ResamplerRender {
        completed_internal_frames: usize,
        completed_host_frames: usize,
        #[source]
        source: StereoOutputResamplerError,
    },
    #[error("snapshot version {version} is unsupported")]
    UnsupportedSnapshotVersion { version: u32 },
    #[error("snapshot output rate does not match the renderer")]
    SnapshotOutputRateMismatch,
    #[error("snapshot host output configuration does not match the renderer")]
    SnapshotHostOutputConfigMismatch,
    #[error("snapshot host output telemetry is invalid")]
    InvalidSnapshotHostOutputTelemetry,
    #[error("snapshot groove identity does not match its player state")]
    InvalidSnapshotIdentity,
    #[error("snapshot groove identity does not match the loaded groove")]
    SnapshotGrooveMismatch,
    #[error("snapshot source identity does not match the loaded groove representation")]
    SnapshotSourceMismatch,
    #[error("snapshot player and resampler clocks do not match")]
    InvalidSnapshotClock,
    #[error("player snapshot restore failed")]
    PlayerRestore {
        #[source]
        source: PhysicalRecordPlayerError,
    },
    #[error("host clipping telemetry counter overflowed")]
    HostClipCounterOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physical::{GrooveAsset, PhysicalProfile, SUPPORTED_PHYSICAL_OUTPUT_RATES_HZ};
    use crate::timed_control::PlayerControl;

    const TEST_HOST_OUTPUT: PhysicalHostOutputConfig = PhysicalHostOutputConfig {
        volts_per_full_scale: 10.0,
    };

    fn renderer(rate: u32) -> PhysicalHostRenderer {
        PhysicalHostRenderer::new(
            PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed(),
            rate,
            TEST_HOST_OUTPUT,
        )
        .unwrap()
    }

    fn render_blocks(renderer: &mut PhysicalHostRenderer, blocks: &[usize]) -> Vec<f32> {
        let mut result = Vec::new();
        for &frames in blocks {
            let mut block = vec![f32::NAN; frames * 2];
            let report = renderer.render_interleaved(&mut block).unwrap();
            assert_eq!(report.rendered_host_frames, frames);
            assert!(block.iter().all(|sample| sample.is_finite()));
            result.extend(block);
        }
        result
    }

    fn test_groove(profile: &PhysicalProfile, velocity_m_s: f32) -> Arc<GrooveAsset> {
        let left = [
            velocity_m_s,
            -velocity_m_s,
            velocity_m_s,
            -velocity_m_s,
            velocity_m_s,
            -velocity_m_s,
            velocity_m_s,
            -velocity_m_s,
        ];
        Arc::new(
            GrooveAsset::from_stereo_wall_velocity_m_s(&left, &left, profile.config.groove)
                .unwrap(),
        )
    }

    fn signal_groove(profile: &PhysicalProfile) -> Arc<GrooveAsset> {
        let frame_count = 8_192;
        let sample_rate_hz = profile.config.groove.groove_sample_rate_hz;
        let left = (0..frame_count)
            .map(|frame| {
                (1.0e-3 * (std::f64::consts::TAU * 997.0 * frame as f64 / sample_rate_hz).sin())
                    as f32
            })
            .collect::<Vec<_>>();
        let right = (0..frame_count)
            .map(|frame| {
                (7.0e-4 * (std::f64::consts::TAU * 1_501.0 * frame as f64 / sample_rate_hz).cos())
                    as f32
            })
            .collect::<Vec<_>>();
        Arc::new(
            GrooveAsset::from_stereo_wall_velocity_m_s(&left, &right, profile.config.groove)
                .unwrap(),
        )
    }

    fn prepare_moving_player(renderer: &mut PhysicalHostRenderer, groove: Arc<GrooveAsset>) {
        renderer.load_groove(groove).unwrap();
        renderer.reset_transport(1.0, 1.0, 0.0, 0.0).unwrap();
        renderer
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                PlayerControl::new(Default::default(), true),
            ))
            .unwrap();
    }

    #[test]
    fn all_supported_rates_have_exact_frame_accounting() {
        for rate in SUPPORTED_PHYSICAL_OUTPUT_RATES_HZ {
            let mut renderer = renderer(rate);
            let host_frames = rate as usize / 100 + 13;
            let required = renderer.internal_frames_required(host_frames).unwrap();
            let mut output = vec![f32::NAN; host_frames * 2];
            let report = renderer.render_interleaved(&mut output).unwrap();
            assert_eq!(report.output_sample_rate_hz, rate);
            assert_eq!(report.rendered_host_frames, host_frames);
            assert_eq!(report.rendered_internal_frames, required);
            assert_eq!(renderer.current_internal_frame(), required as u64);
            assert_eq!(renderer.rendered_host_frames(), host_frames as u64);
        }
    }

    #[test]
    fn arbitrary_host_partitions_produce_identical_output_and_clock() {
        let rate = 44_100;
        let total = 1_001;
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = signal_groove(&profile);
        let mut whole = PhysicalHostRenderer::new(profile.clone(), rate, TEST_HOST_OUTPUT).unwrap();
        let mut split = PhysicalHostRenderer::new(profile, rate, TEST_HOST_OUTPUT).unwrap();
        prepare_moving_player(&mut whole, Arc::clone(&groove));
        prepare_moving_player(&mut split, groove);
        let whole_output = render_blocks(&mut whole, &[total]);
        let split_output = render_blocks(&mut split, &[1, 127, 3, 256, 111, 300, 203]);
        assert_eq!(split_output.len(), total * 2);
        assert_eq!(whole_output, split_output);
        assert!(whole_output.iter().any(|sample| sample.abs() > 1.0e-8));
        assert_eq!(
            whole.current_internal_frame(),
            split.current_internal_frame()
        );
        assert_eq!(whole.rendered_host_frames(), split.rendered_host_frames());
        assert_eq!(whole.resampler.snapshot(), split.resampler.snapshot());

        let mut whole_telemetry = whole.telemetry();
        let mut split_telemetry = split.telemetry();
        whole_telemetry.rendered_internal_frames = 0;
        split_telemetry.rendered_internal_frames = 0;
        assert_eq!(whole_telemetry, split_telemetry);
    }

    #[test]
    fn renderer_chunks_blocks_without_reallocating_scratch() {
        let mut renderer = renderer(44_100);
        let capacity = renderer.scratch_capacity_frames();
        let scratch_address = renderer.internal_scratch.as_ptr();
        let host_scratch_address = renderer.host_scratch.as_ptr();
        let host_frames = capacity;
        let required = renderer.internal_frames_required(host_frames).unwrap();
        assert!(required > capacity);
        let mut output = vec![0.0; host_frames * 2];
        let report = renderer.render_interleaved(&mut output).unwrap();
        assert_eq!(report.rendered_internal_frames, required);
        assert_eq!(renderer.internal_scratch.as_ptr(), scratch_address);
        assert_eq!(renderer.host_scratch.as_ptr(), host_scratch_address);
        assert_eq!(renderer.scratch_capacity_frames(), capacity);
    }

    #[test]
    fn later_chunk_failure_rolls_back_the_complete_host_block() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let mut renderer =
            PhysicalHostRenderer::new(profile.clone(), 44_100, TEST_HOST_OUTPUT).unwrap();
        let first_chunk = renderer.scratch_capacity_frames() as u64;
        renderer
            .enqueue_control(TimedPlayerControl::new(
                0,
                1,
                PlayerControl::new(Default::default(), true),
            ))
            .unwrap();
        renderer
            .player
            .inject_render_failure_at_completed_step(first_chunk + 4);
        let before = renderer.snapshot();
        let host_frames = renderer.maximum_host_render_frames();
        assert!(renderer.internal_frames_required(host_frames).unwrap() > first_chunk as usize);
        let mut output = vec![7.0_f32; host_frames * 2];

        assert!(matches!(
            renderer.render_interleaved(&mut output),
            Err(PhysicalHostRendererError::PlayerRender {
                completed_internal_frames,
                ..
            }) if completed_internal_frames >= first_chunk as usize
        ));
        assert!(output.iter().all(|sample| *sample == 7.0));
        assert_eq!(renderer.snapshot(), before);
    }

    #[test]
    fn oversized_host_block_is_rejected_without_mutation() {
        let mut renderer = renderer(48_000);
        let before = renderer.snapshot();
        let mut output = vec![9.0_f32; (renderer.maximum_host_render_frames() + 1) * 2];
        assert!(matches!(
            renderer.render_interleaved(&mut output),
            Err(PhysicalHostRendererError::HostBlockTooLarge { .. })
        ));
        assert!(output.iter().all(|sample| *sample == 9.0));
        assert_eq!(renderer.snapshot(), before);
    }

    #[test]
    fn snapshot_restore_continues_bit_exactly() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let groove = signal_groove(&profile);
        let mut original =
            PhysicalHostRenderer::new(profile.clone(), 88_200, TEST_HOST_OUTPUT).unwrap();
        prepare_moving_player(&mut original, Arc::clone(&groove));
        render_blocks(&mut original, &[337]);
        let snapshot = original.snapshot();
        let expected = render_blocks(&mut original, &[29, 701, 3]);

        let mut restored = PhysicalHostRenderer::new(profile, 88_200, TEST_HOST_OUTPUT).unwrap();
        restored.load_groove(groove).unwrap();
        restored.restore(&snapshot).unwrap();
        let actual = render_blocks(&mut restored, &[29, 701, 3]);
        assert_eq!(actual, expected);
        assert_eq!(restored.snapshot(), original.snapshot());

        let json = serde_json::to_string(&snapshot).unwrap();
        let decoded: PhysicalHostRendererSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn restore_requires_the_same_loaded_groove_content() {
        let profile = PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed();
        let first_groove = test_groove(&profile, 1.0e-6);
        let second_groove = test_groove(&profile, 2.0e-6);
        let mut source =
            PhysicalHostRenderer::new(profile.clone(), 48_000, TEST_HOST_OUTPUT).unwrap();
        source.load_groove(Arc::clone(&first_groove)).unwrap();
        let snapshot = source.snapshot();

        let mut invalid_identity = snapshot.clone();
        invalid_identity.loaded_groove_identity = None;
        assert!(matches!(
            source.restore(&invalid_identity),
            Err(PhysicalHostRendererError::InvalidSnapshotIdentity)
        ));
        assert_eq!(source.snapshot(), snapshot);

        let mut destination = PhysicalHostRenderer::new(profile, 48_000, TEST_HOST_OUTPUT).unwrap();
        assert!(matches!(
            destination.restore(&snapshot),
            Err(PhysicalHostRendererError::SnapshotGrooveMismatch)
        ));
        destination.load_groove(second_groove).unwrap();
        assert!(matches!(
            destination.restore(&snapshot),
            Err(PhysicalHostRendererError::SnapshotGrooveMismatch)
        ));
        destination.unload_groove();
        destination.load_groove(first_groove).unwrap();
        destination.restore(&snapshot).unwrap();
        assert_eq!(
            destination.loaded_groove_identity(),
            snapshot.loaded_groove_identity
        );
    }

    #[test]
    fn restore_rejects_wrong_version_rate_and_clock_without_mutation() {
        let mut renderer = renderer(48_000);
        render_blocks(&mut renderer, &[101]);
        let before = renderer.snapshot();

        let mut wrong_version = before.clone();
        wrong_version.version += 1;
        assert!(matches!(
            renderer.restore(&wrong_version),
            Err(PhysicalHostRendererError::UnsupportedSnapshotVersion { .. })
        ));
        assert_eq!(renderer.snapshot(), before);

        let mut wrong_rate = before.clone();
        wrong_rate.output_sample_rate_hz = 96_000;
        assert!(matches!(
            renderer.restore(&wrong_rate),
            Err(PhysicalHostRendererError::SnapshotOutputRateMismatch)
        ));
        assert_eq!(renderer.snapshot(), before);

        let mut wrong_clock = before.clone();
        wrong_clock.resampler.input_frames_consumed += 1;
        assert!(matches!(
            renderer.restore(&wrong_clock),
            Err(PhysicalHostRendererError::InvalidSnapshotClock)
        ));
        assert_eq!(renderer.snapshot(), before);
    }

    #[test]
    fn invalid_rate_and_stereo_shape_are_rejected() {
        assert!(matches!(
            PhysicalHostRenderer::new(
                PhysicalProfile::sl_1200mk7_concorde_mkii_scratch_seed(),
                32_000,
                TEST_HOST_OUTPUT,
            ),
            Err(PhysicalHostRendererError::Resampler(
                StereoOutputResamplerError::UnsupportedOutputRate { .. }
            ))
        ));

        let mut renderer = renderer(48_000);
        let before = renderer.snapshot();
        let report = renderer.render_interleaved(&mut []).unwrap();
        assert_eq!(report.rendered_host_frames, 0);
        assert_eq!(report.rendered_internal_frames, 0);
        assert_eq!(renderer.snapshot(), before);
        assert!(matches!(
            renderer.render_interleaved(&mut [0.0; 3]),
            Err(PhysicalHostRendererError::OutputMustBeStereo)
        ));
        assert_eq!(renderer.snapshot(), before);
    }

    #[test]
    fn explicit_voltage_boundary_maps_one_and_ten_volts_exactly() {
        let config = PhysicalHostOutputConfig {
            volts_per_full_scale: 10.0,
        };
        let mut output = [1.0_f32, -1.0, 10.0, -10.0];
        let telemetry = convert_host_output_in_place(config, &mut output, [0; 2]).unwrap();

        assert_eq!(output, [0.1, -0.1, 1.0, -1.0]);
        assert_eq!(telemetry.volts_per_full_scale, 10.0);
        assert_eq!(telemetry.peak_unclipped_abs_output_v, [10.0, 10.0]);
        assert_eq!(telemetry.clipped_samples, [0; 2]);
        assert_eq!(telemetry.total_clipped_samples, [0; 2]);
    }

    #[test]
    fn host_boundary_clips_after_retaining_unclipped_voltage_telemetry() {
        let config = PhysicalHostOutputConfig {
            volts_per_full_scale: 1.0,
        };
        let mut output = [1.0_f32, -1.0, 10.0, -10.0, 0.5, -2.0];
        let telemetry = convert_host_output_in_place(config, &mut output, [7, 11]).unwrap();

        assert_eq!(output, [1.0, -1.0, 1.0, -1.0, 0.5, -1.0]);
        assert_eq!(telemetry.peak_unclipped_abs_output_v, [10.0, 10.0]);
        assert_eq!(telemetry.clipped_samples, [1, 2]);
        assert_eq!(telemetry.total_clipped_samples, [8, 13]);
    }

    #[test]
    fn host_boundary_rejects_non_finite_voltage_without_changing_samples() {
        let config = PhysicalHostOutputConfig {
            volts_per_full_scale: 10.0,
        };
        let mut output = [1.0_f32, -1.0, 2.0, f32::NAN];
        let before = output.map(f32::to_bits);

        assert!(matches!(
            convert_host_output_in_place(config, &mut output, [0; 2]),
            Err(PhysicalHostRendererError::NonFiniteHostOutputVoltage { channel: 1 })
        ));
        assert_eq!(output.map(f32::to_bits), before);
    }

    #[test]
    fn host_output_configuration_is_required_and_bounded() {
        for volts_per_full_scale in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            assert_eq!(
                PhysicalHostOutputConfig {
                    volts_per_full_scale,
                }
                .validate(),
                Err(PhysicalHostOutputConfigError::InvalidVoltsPerFullScale)
            );
        }
        assert!(PhysicalHostOutputConfig {
            volts_per_full_scale: MAXIMUM_HOST_VOLTS_PER_FULL_SCALE,
        }
        .validate()
        .is_ok());
        assert!(PhysicalHostOutputConfig {
            volts_per_full_scale: MAXIMUM_HOST_VOLTS_PER_FULL_SCALE * 2.0,
        }
        .validate()
        .is_err());
    }
}
