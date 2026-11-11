//! SONOLYTICA domain model.
//!
//! This crate is deliberately free of I/O, async runtimes and model runtimes.
//! It defines the vocabulary every other crate speaks: time, media metadata,
//! analysis configuration, provenance and the document (timeline) model.
//!
//! Invariants that matter for data integrity:
//!
//! * time is integer microseconds on the media timeline, never floating point;
//! * ranges are half-open `[start, end)`;
//! * every segment points at a [`RunRecord`], so nothing is unattributable;
//! * absence of evidence is represented by `None`/an empty vec, never by a
//!   synthetic zero, empty string or fabricated timestamp.

#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]

pub mod analysis;
pub mod audio;
pub mod config;
pub mod document;
pub mod error;
pub mod media;
pub mod provenance;
pub mod time;

pub use analysis::{
    AcousticSummary, AnalysisBlock, Bpm, Chord, ChordQuality, KeyEstimate, Loudness, Mode,
    MusicSummary, PitchClass, SpeechSummary,
};
pub use audio::Audio;
pub use config::{AnalysisConfig, Capability, DiarizationConfig, DspConfig, Limits, VadConfig};
pub use document::{
    Artifact, BBox, Confidence, DialogueTurn, Document, DocumentBuilder, Relation, RelationKind,
    Segment, SegmentId, SegmentKind, SpeakerId, Track, TrackId, TrackKind, Violation, Word,
    now_unix_ms,
};
pub use error::{CoreError, Result};
pub use media::{MediaInfo, StreamInfo, StreamKind};
pub use provenance::{ModelRef, RunId, RunRecord, RunStatus, Warning};
pub use time::{MediaTime, TimeRange};

/// Version of the serialized document schema. Bump on any breaking change.
pub const SCHEMA_VERSION: &str = "1.3.0";

/// Name of this implementation, embedded in every document.
pub const GENERATOR_NAME: &str = "sonolytica";

/// Version of this implementation.
pub const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");
