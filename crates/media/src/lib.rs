//! Media ingestion.
//!
//! FFmpeg is used as a *subprocess*, not linked: it keeps the build free of C
//! linkage and licensing entanglement, and it is the decoder that handles the
//! chaos of real-world containers. Every invocation is argv-only (never a
//! shell string), runs with a protocol whitelist restricted to local files,
//! a timeout, and a size/duration budget.

#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]

pub mod decode;
pub mod error;
pub mod frames;
pub mod hash;
pub mod probe;
pub mod tools;

pub use decode::{DecodeOptions, decode_mono};
pub use error::MediaError;
pub use frames::{FrameOptions, FrameSignature, Shot, detect_shots, sample_video};
pub use hash::sha256_file;
pub use probe::probe;
pub use tools::{MediaTools, ValidatedInput, validate_input};
