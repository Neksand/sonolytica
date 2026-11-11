//! Model providers for SONOLYTICA.
//!
//! The engine knows only the traits in [`traits`]; every model runtime lives
//! behind a feature flag so the core pipeline can be built and tested where
//! ONNX Runtime or whisper.cpp cannot be compiled. Providers are replaceable
//! by design, including by out-of-tree crates implementing the same traits.

#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]

pub mod models;
pub mod ocr;
pub mod registry;
pub mod traits;

#[cfg(any(feature = "silero", feature = "diarize", feature = "ocr"))]
#[cfg_attr(not(any(feature = "silero", feature = "diarize")), allow(dead_code))]
mod onnx;

#[cfg(feature = "silero")]
mod silero;

#[cfg(feature = "whisper")]
mod whisper;

#[cfg(feature = "diarize")]
mod diarize;

pub use models::{MODELS, ModelSpec, ModelStore, default_model_dir, find_model};
pub use registry::{Registry, Unavailable};
pub use traits::{
    AsrConfig, AsrOutput, AsrProvider, AsrSegment, DiarTurn, DiarizationOutput,
    DiarizationProvider, ProviderError, ProviderInfo, ProviderResult, SpeechRegion, VadProvider,
};
