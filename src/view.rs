use crate::{DeckId, LoadStatus, PlayerState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeckViewState {
    pub deck: DeckId,
    pub loaded: bool,
    pub load_status: LoadStatus,
    pub transport_on: bool,
    pub needle_lifted: bool,
    pub playing: bool,
    pub scratch_active: bool,
    pub playback_seconds: f64,
    pub duration_seconds: f64,
    pub source_sample_rate: f64,
    pub playback_rate: f32,
    pub channel_gain: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerViewState {
    pub revision: u64,
    pub active_deck: DeckId,
    pub decks: [DeckViewState; 2],
    pub crossfader: f32,
    pub lead_in_active: bool,
    pub deadwax_active: bool,
    pub clip_loop_active: bool,
}

impl PlayerViewState {
    pub fn from_state(state: &PlayerState, revision: u64) -> Self {
        let deck_view = |deck: DeckId| {
            let value = &state.decks[deck.index()];
            DeckViewState {
                deck,
                loaded: value.loaded,
                load_status: value.playback.load_status,
                transport_on: value.transport.motor_on,
                needle_lifted: value.needle.lifted,
                playing: value.playback.playing,
                scratch_active: value.scratch.active,
                playback_seconds: value.playback.current_seconds,
                duration_seconds: value.playback.duration_seconds,
                source_sample_rate: value.playback.source_sample_rate,
                playback_rate: value.playback.playback_rate,
                channel_gain: value.mixer.channel_gain,
            }
        };
        Self {
            revision,
            active_deck: state.active_deck,
            decks: [deck_view(DeckId::A), deck_view(DeckId::B)],
            crossfader: state.decks[0].mixer.crossfader,
            lead_in_active: state.lead_in.active,
            deadwax_active: state.deadwax.active,
            clip_loop_active: state.clip_loop.active,
        }
    }
}
