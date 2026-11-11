//! Provider contracts.
//!
//! Providers are synchronous on purpose: inference is blocking CPU/GPU work,
//! and the engine already owns the async boundary by running each provider in
//! `spawn_blocking`. That keeps implementations free of `async_trait` and
//! makes it impossible to accidentally `.await` inside a model run.

use std::fmt;

use sonolytica_core::{
    Audio, Capability, Confidence, DiarizationConfig, ModelRef, SpeakerId, TimeRange, VadConfig,
    Word,
};

#[derive(Debug)]
pub enum ProviderError {
    /// The model file is missing; the message says how to obtain it.
    ModelMissing {
        id: String,
        hint: String,
    },
    /// Inference failed, or the model graph did not look as expected.
    Inference(String),
    /// The input cannot be handled by this provider (wrong sample rate, ...).
    Unsupported(String),
    Io(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModelMissing { id, hint } => write!(f, "model `{id}` is not available: {hint}"),
            Self::Inference(m) => write!(f, "inference error: {m}"),
            Self::Unsupported(m) => write!(f, "unsupported input: {m}"),
            Self::Io(m) => write!(f, "i/o error: {m}"),
        }
    }
}

impl std::error::Error for ProviderError {}

pub type ProviderResult<T> = Result<T, ProviderError>;

/// What a provider is, what it needs, and what it claims.
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    pub name: &'static str,
    pub capability: Capability,
    pub device: &'static str,
    pub models: Vec<ModelRef>,
    /// Honest limits, surfaced in `sonolytica models list` and document warnings.
    pub notes: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeechRegion {
    pub range: TimeRange,
    pub probability: Option<f32>,
}

pub trait VadProvider: Send + Sync {
    fn info(&self) -> ProviderInfo;
    fn detect(&self, audio: &Audio, cfg: &VadConfig) -> ProviderResult<Vec<SpeechRegion>>;
}

#[derive(Debug, Clone)]
pub struct AsrConfig {
    pub language: Option<String>,
    pub translate_to_english: bool,
    pub threads: usize,
    /// Speech regions to transcribe one by one, on the media timeline.
    ///
    /// Empty means "transcribe the whole buffer". Region-driven decoding is
    /// both faster (no silence) and more robust: long-form file decoding can
    /// drop everything after a short utterance when the model emits an early
    /// end-of-text, which is a documented Whisper failure mode.
    pub regions: Vec<TimeRange>,
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            language: None,
            translate_to_english: false,
            threads: std::thread::available_parallelism()
                .map_or(4, std::num::NonZeroUsize::get)
                .min(8),
            regions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AsrSegment {
    pub range: TimeRange,
    pub text: String,
    pub words: Vec<Word>,
    pub confidence: Option<Confidence>,
    pub no_speech_prob: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct AsrOutput {
    pub language: Option<String>,
    pub segments: Vec<AsrSegment>,
}

pub trait AsrProvider: Send + Sync {
    fn info(&self) -> ProviderInfo;
    fn transcribe(&self, audio: &Audio, cfg: &AsrConfig) -> ProviderResult<AsrOutput>;
}

#[derive(Debug, Clone)]
pub struct DiarTurn {
    pub range: TimeRange,
    pub speaker: SpeakerId,
    pub confidence: Option<Confidence>,
}

#[derive(Debug, Clone, Default)]
pub struct DiarizationOutput {
    pub turns: Vec<DiarTurn>,
    /// True when regions of overlapping speech were observed. Overlap is
    /// reported, not guessed: a false negative is better than a fabricated one.
    pub overlap_detected: bool,
}

pub trait DiarizationProvider: Send + Sync {
    fn info(&self) -> ProviderInfo;
    fn diarize(&self, audio: &Audio, cfg: &DiarizationConfig) -> ProviderResult<DiarizationOutput>;
}

/// Cosine similarity, both inputs assumed non-zero.
#[cfg_attr(not(feature = "diarize"), allow(dead_code))]
pub(crate) fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0_f64;
    let mut na = 0.0_f64;
    let mut nb = 0.0_f64;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += f64::from(*x) * f64::from(*y);
        na += f64::from(*x) * f64::from(*x);
        nb += f64::from(*y) * f64::from(*y);
    }
    let denom = (na * nb).sqrt();
    if denom <= f64::EPSILON {
        0.0
    } else {
        (dot / denom) as f32
    }
}

#[cfg_attr(not(feature = "diarize"), allow(dead_code))]
pub(crate) fn l2_normalize(values: &mut [f32]) {
    let norm = values
        .iter()
        .map(|v| f64::from(*v) * f64::from(*v))
        .sum::<f64>()
        .sqrt();
    if norm > f64::EPSILON {
        for v in values {
            *v = (f64::from(*v) / norm) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_is_scale_invariant() {
        let a = [1.0, 2.0, 3.0];
        let b = [2.0, 4.0, 6.0];
        assert!((cosine(&a, &b) - 1.0).abs() < 1e-6);
        assert!(cosine(&a, &[0.0, 0.0, 0.0]).abs() < 1e-6);
    }

    #[test]
    fn normalization_yields_unit_length() {
        let mut v = vec![3.0_f32, 4.0];
        l2_normalize(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-5);
        assert!((v[1] - 0.8).abs() < 1e-5);
    }
}
