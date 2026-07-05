use crate::{DeckId, SurfaceRegion};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostCommand {
    SetMotor { deck: DeckId, running: bool },
    SetPacketGain { deck: DeckId, gain: f32, ramp_ms: u32 },
    StartPacketPlayback { deck: DeckId, offset_seconds: f64, rate: f32, platter_handoff: bool },
    StopPacketPlayback { deck: DeckId, platter_handoff: bool },
    SeekPacketPlayback { deck: DeckId, offset_seconds: f64 },
    SetScratchTransport { deck: DeckId, hand_contact: bool, motor_rate: f32 },
    SetScratchTarget { deck: DeckId, position_frames: f64, rate: f32, impulse: f32 },
    SetScratchPosition { deck: DeckId, position_frames: f64, impulse: f32 },
    StartSurfaceRegion { region: SurfaceRegion, duration_seconds: f64 },
    StopSurfaceRegion { region: SurfaceRegion },
    StopClipLoop,
    SetMixerTrackGain { track: u8, gain: f32, ramp_ms: u32 },
    CaptureScratchStart { deck: DeckId, start_seconds: f64, rotation_degrees: f64 },
    CaptureScratchFinish { deck: DeckId, end_seconds: f64, rotation_degrees: f64, save_sample: bool },
    PublishTransportIntent,
    RefreshView,
}
