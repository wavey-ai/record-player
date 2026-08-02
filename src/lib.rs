#![forbid(unsafe_code)]

#[cfg(all(test, debug_assertions))]
#[global_allocator]
static TEST_ALLOCATION_GUARD: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

pub mod acoustic;
mod command;
mod engine;
mod event;
pub mod gesture;
pub mod mechanics;
mod mixer;
pub mod physical;
mod resampler;
pub mod scratch_gate;
pub mod spsc;
mod state;
pub mod timed_control;
mod view;

pub use acoustic::{
    AcousticConfig, AcousticStatus, CalibrationAnchor, DeckRecoveryDiagnostic,
    DeckRecoveryOperation, ScratchAcousticDsp, StylusCalibration,
};
pub use command::*;
pub use engine::*;
pub use event::*;
pub use gesture::*;
pub use mechanics::{
    ContactMode, DeckMechanicalControl, DeckMechanicalError, DeckMechanicalSnapshot,
    DeckMechanicalState, DeckMechanicalTelemetry, MotorMode, NormalizedDeckControl,
    PhysicalDeckConfig, PhysicalDeckConfigError,
};
pub use mixer::*;
pub use scratch_gate::{
    ScratchCrossfaderOwner, ScratchGate, ScratchGateSnapshot, ScratchPerformance,
    ScratchPerformanceError, ScratchPerformanceInput, ScratchPerformanceOutput,
    ScratchPerformanceSnapshot, ScratchPreset, ScratchPresetDescriptor,
    MAXIMUM_SCRATCH_RECORD_RATE, MAX_SCRATCH_CLICKS, MIN_SCRATCH_CLICKS,
    SCRATCH_GATE_ALGORITHM_VERSION, SCRATCH_GATE_SNAPSHOT_VERSION,
    SCRATCH_PERFORMANCE_SNAPSHOT_VERSION, SCRATCH_PRESET_CATALOG,
};
pub use state::*;
pub use view::*;

#[cfg(feature = "wasm")]
mod wasm;
