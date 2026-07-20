use serde::{Deserialize, Serialize};

use crate::mixer::DEFAULT_SHARP_CROSSFADER_WIDTH;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeckId {
    A,
    B,
}

impl DeckId {
    pub const ALL: [Self; 2] = [Self::A, Self::B];
    pub const fn index(self) -> usize {
        match self {
            Self::A => 0,
            Self::B => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadStatus {
    Empty,
    Loading,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceRegion {
    Programme,
    LeadIn,
    Deadwax,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackState {
    pub load_status: LoadStatus,
    pub duration_seconds: f64,
    pub current_seconds: f64,
    /// The programme PCM clock. Scratch positions are expressed in source
    /// frames, so this must not be confused with the AudioContext output rate.
    pub source_sample_rate: f64,
    pub playing: bool,
    pub pending_play_when_ready: bool,
    pub suspended_at_seconds: Option<f64>,
    pub playback_rate: f32,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            load_status: LoadStatus::Empty,
            duration_seconds: 0.0,
            current_seconds: 0.0,
            source_sample_rate: 48_000.0,
            playing: false,
            pending_play_when_ready: false,
            suspended_at_seconds: None,
            playback_rate: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransportState {
    pub motor_on: bool,
}
impl Default for TransportState {
    fn default() -> Self {
        Self { motor_on: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NeedleState {
    pub lifted: bool,
}
impl Default for NeedleState {
    fn default() -> Self {
        Self { lifted: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixerState {
    pub channel_gain: f32,
    pub crossfader: f32,
}
impl Default for MixerState {
    fn default() -> Self {
        Self {
            channel_gain: 1.0,
            crossfader: 0.5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScratchState {
    pub active: bool,
    pub pointer_id: Option<i32>,
    pub was_playing: bool,
    pub started_at_seconds: f64,
    pub target_position_frames: f64,
    pub rendered_position_frames: f64,
    pub target_rate: f32,
    pub grip: f32,
    pub base_rotation_degrees: f64,
}
impl Default for ScratchState {
    fn default() -> Self {
        Self {
            active: false,
            pointer_id: None,
            was_playing: false,
            started_at_seconds: 0.0,
            target_position_frames: 0.0,
            rendered_position_frames: 0.0,
            target_rate: 0.0,
            grip: 0.0,
            base_rotation_degrees: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeckState {
    pub loaded: bool,
    pub transport: TransportState,
    pub needle: NeedleState,
    pub playback: PlaybackState,
    pub mixer: MixerState,
    pub scratch: ScratchState,
}
impl Default for DeckState {
    fn default() -> Self {
        Self {
            loaded: false,
            transport: TransportState::default(),
            needle: NeedleState::default(),
            playback: PlaybackState::default(),
            mixer: MixerState::default(),
            scratch: ScratchState::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimedRegionState {
    pub active: bool,
    pub completed: bool,
    pub started_at_ms: f64,
    pub duration_ms: f64,
}
impl Default for TimedRegionState {
    fn default() -> Self {
        Self {
            active: false,
            completed: false,
            started_at_ms: 0.0,
            duration_ms: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClipLoopState {
    pub active: bool,
    pub clip_id: Option<String>,
}
impl Default for ClipLoopState {
    fn default() -> Self {
        Self {
            active: false,
            clip_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerConfig {
    pub sample_rate: f64,
    pub sharp_crossfader_width: f32,
}
impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            sharp_crossfader_width: DEFAULT_SHARP_CROSSFADER_WIDTH,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    pub decks: [DeckState; 2],
    pub active_deck: DeckId,
    pub lead_in: TimedRegionState,
    pub deadwax: TimedRegionState,
    pub clip_loop: ClipLoopState,
}
impl Default for PlayerState {
    fn default() -> Self {
        Self {
            decks: [DeckState::default(), DeckState::default()],
            active_deck: DeckId::A,
            lead_in: TimedRegionState::default(),
            deadwax: TimedRegionState::default(),
            clip_loop: ClipLoopState::default(),
        }
    }
}
