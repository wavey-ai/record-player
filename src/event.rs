use crate::{DeckId, LoadStatus, SurfaceRegion};
use serde::{Deserialize, Serialize};

fn default_scratch_grip() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlayerEvent {
    HydrateDeck {
        deck: DeckId,
        loaded: bool,
        status: LoadStatus,
        duration_seconds: f64,
        current_seconds: f64,
        playing: bool,
        transport_on: bool,
        needle_lifted: bool,
        playback_rate: f32,
        channel_gain: f32,
    },
    SetActiveDeck {
        deck: DeckId,
    },
    SetLoadState {
        deck: DeckId,
        status: LoadStatus,
        loaded: bool,
        duration_seconds: f64,
    },
    SetSourceSampleRate {
        deck: DeckId,
        sample_rate: f64,
    },
    PlaybackPositionObserved {
        deck: DeckId,
        seconds: f64,
    },
    PlaybackEnded {
        deck: DeckId,
    },
    ToggleTransport {
        deck: DeckId,
    },
    SetTransport {
        deck: DeckId,
        running: bool,
    },
    TogglePlayback {
        deck: DeckId,
    },
    SetNeedle {
        deck: DeckId,
        lifted: bool,
        observed_playback_seconds: f64,
    },
    Seek {
        deck: DeckId,
        seconds: f64,
    },
    SetPlaybackRate {
        deck: DeckId,
        rate: f32,
    },
    StartTimedRegion {
        region: SurfaceRegion,
        now_ms: f64,
        duration_seconds: f64,
    },
    StopTimedRegion {
        region: SurfaceRegion,
        completed: bool,
    },
    TimedRegionElapsed {
        region: SurfaceRegion,
    },
    StartClipLoop {
        clip_id: String,
    },
    StopClipLoop,
    BeginScratch {
        deck: DeckId,
        pointer_id: i32,
        playback_seconds: f64,
        rotation_degrees: f64,
        #[serde(default = "default_scratch_grip")]
        grip: f32,
    },
    MoveScratch {
        deck: DeckId,
        position_frames: f64,
        rendered_position_frames: f64,
        rate: f32,
        rotation_degrees: f64,
        impulse: f32,
        #[serde(default = "default_scratch_grip")]
        grip: f32,
    },
    ScratchRenderedPosition {
        deck: DeckId,
        rendered_position_frames: f64,
    },
    EndScratch {
        deck: DeckId,
        rendered_position_frames: f64,
        rotation_degrees: f64,
        resume_playback: bool,
        save_sample: bool,
        can_platter_handoff: bool,
    },
    SetCrossfader {
        value: f32,
    },
    SetChannelGain {
        deck: DeckId,
        value: f32,
    },
    SetTwoDeckMode {
        enabled: bool,
    },
    Tick {
        now_ms: f64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_scratch_events_default_to_full_grip() {
        let begin: PlayerEvent = serde_json::from_value(serde_json::json!({
            "type": "begin_scratch",
            "deck": "a",
            "pointer_id": 1,
            "playback_seconds": 2.0,
            "rotation_degrees": 0.0
        }))
        .unwrap();
        let movement: PlayerEvent = serde_json::from_value(serde_json::json!({
            "type": "move_scratch",
            "deck": "a",
            "position_frames": 96_000.0,
            "rendered_position_frames": 96_000.0,
            "rate": -1.0,
            "rotation_degrees": -20.0,
            "impulse": 0.0
        }))
        .unwrap();

        assert!(matches!(begin, PlayerEvent::BeginScratch { grip, .. } if grip == 1.0));
        assert!(matches!(movement, PlayerEvent::MoveScratch { grip, .. } if grip == 1.0));
    }
}
