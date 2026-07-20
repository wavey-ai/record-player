#![forbid(unsafe_code)]

pub mod acoustic;
mod command;
mod engine;
mod event;
mod mixer;
mod resampler;
pub mod scratch_gate;
mod state;
mod view;

pub use acoustic::{
    AcousticConfig, AcousticStatus, CalibrationAnchor, ScratchAcousticDsp, StylusCalibration,
};
pub use command::*;
pub use engine::*;
pub use event::*;
pub use mixer::*;
pub use scratch_gate::{
    ScratchGate, ScratchPreset, MAX_SCRATCH_CLICKS, MIN_SCRATCH_CLICKS,
    SCRATCH_GATE_ALGORITHM_VERSION,
};
pub use state::*;
pub use view::*;

#[cfg(feature = "wasm")]
mod wasm;
