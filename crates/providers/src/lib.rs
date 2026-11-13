//! Model providers for SONOLYTICA.
//!
//! The engine knows only the traits in [`traits`]; every model runtime lives
//! behind a feature flag so the core pipeline can be built and tested where
//! ONNX Runtime or whisper.cpp cannot be compiled. Providers are replaceable
//! by design, including by out-of-tree crates implementing the same traits.

#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]

pub mod align;
pub mod face;
pub mod florence;
pub mod models;
pub mod objects;
pub mod ocr;
pub mod pose;
pub mod registry;
pub mod traits;

#[cfg(any(
    feature = "silero",
    feature = "diarize",
    feature = "ocr",
    feature = "vision",
    feature = "align"
))]
#[cfg_attr(not(any(feature = "silero", feature = "diarize")), allow(dead_code))]
mod onnx;

#[cfg(feature = "silero")]
mod silero;

#[cfg(feature = "whisper")]
mod whisper;

#[cfg(feature = "diarize")]
mod diarize;

#[cfg(feature = "diarize")]
mod sortformer;

use std::path::PathBuf;

/// The user's home directory: `$HOME` on Unix, `%USERPROFILE%` on Windows.
///
/// `HOME` stays first so Unix-style shells, CI and tests keep working; native
/// Windows shells normally set only `USERPROFILE`.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub use align::{LazyWav2Vec2Aligner, Wav2Vec2Aligner};
pub use models::{MODELS, ModelSpec, ModelStore, default_model_dir, find_model};
pub use registry::{Registry, Unavailable};
pub use traits::{
    AsrConfig, AsrOutput, AsrProvider, AsrSegment, DiarTurn, DiarizationOutput,
    DiarizationProvider, ProviderError, ProviderInfo, ProviderResult, SpeechRegion, VadProvider,
};
