//! Analysis configuration: what was asked for, and under which parameters.
//!
//! The whole struct is serialized into every document and hashed into
//! [`crate::RunRecord::params_sha256`], which is what makes a result
//! reproducible after the fact.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Metadata only.
    Inspect,
    /// Voice activity detection.
    Vad,
    /// Speech recognition.
    Transcribe,
    /// Speaker diarization.
    Diarize,
    /// Music information retrieval.
    Music,
    /// Non-speech acoustic analysis.
    Acoustic,
    /// Shot/scene detection.
    Scene,
    /// Visual understanding.
    Vision,
}

impl Capability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Vad => "vad",
            Self::Transcribe => "transcribe",
            Self::Diarize => "diarize",
            Self::Music => "music",
            Self::Acoustic => "acoustic",
            Self::Scene => "scene",
            Self::Vision => "vision",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "inspect" => Self::Inspect,
            "vad" | "speech" => Self::Vad,
            "transcribe" | "asr" | "speech_to_text" => Self::Transcribe,
            "diarize" | "diarization" | "speakers" => Self::Diarize,
            "music" => Self::Music,
            "acoustic" | "sounds" => Self::Acoustic,
            "scene" | "video" => Self::Scene,
            "vision" => Self::Vision,
            _ => return None,
        })
    }

    /// Capabilities implemented by the audio pipeline in this release.
    pub const AUDIO: [Self; 5] = [
        Self::Vad,
        Self::Transcribe,
        Self::Diarize,
        Self::Music,
        Self::Acoustic,
    ];
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VadConfig {
    /// Speech probability threshold for the model-based detector.
    pub threshold: f32,
    pub min_speech_ms: u32,
    pub min_silence_ms: u32,
    /// Padding added around detected speech.
    pub pad_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.45,
            min_speech_ms: 250,
            min_silence_ms: 500,
            pad_ms: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DiarizationConfig {
    /// Average-linkage cosine similarity at which two speaker clusters merge.
    /// Calibrated for 1.5 s embedding chunks: same-speaker chunks sit around
    /// 0.5–0.8, different speakers below 0.3.
    pub threshold: f32,
    pub max_speakers: u32,
    /// Regions shorter than this are not used to create a speaker identity.
    pub min_region_ms: u32,
}

impl Default for DiarizationConfig {
    fn default() -> Self {
        Self {
            threshold: 0.45,
            max_speakers: 12,
            min_region_ms: 500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DspConfig {
    pub fft_size: usize,
    pub hop_size: usize,
    /// Frame level below this RMS is considered silence.
    pub silence_dbfs: f32,
    pub min_silence_ms: u32,
    pub detect_tempo: bool,
}

impl Default for DspConfig {
    fn default() -> Self {
        Self {
            fft_size: 2048,
            hop_size: 512,
            silence_dbfs: -45.0,
            min_silence_ms: 500,
            detect_tempo: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    /// Longest media this process will decode, in seconds.
    pub max_duration_secs: u64,
    /// Largest input file this process will accept, in bytes.
    pub max_file_bytes: u64,
    /// Per external process timeout (probe/decode).
    pub timeout_secs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_duration_secs: 2 * 60 * 60,
            max_file_bytes: 8 * 1024 * 1024 * 1024,
            timeout_secs: 600,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisConfig {
    pub modules: Vec<Capability>,
    /// Preferred language (ISO 639-1) or `None` for automatic detection.
    pub language: Option<String>,
    pub translate_to_english: bool,
    /// Explicit ASR model id, overriding the default choice.
    pub asr_model: Option<String>,
    pub vad: VadConfig,
    pub diarization: DiarizationConfig,
    pub dsp: DspConfig,
    pub limits: Limits,
    /// Hash the input file (SHA-256) for provenance and cache keys.
    pub hash_media: bool,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            modules: Capability::AUDIO.to_vec(),
            language: None,
            translate_to_english: false,
            asr_model: None,
            vad: VadConfig::default(),
            diarization: DiarizationConfig::default(),
            dsp: DspConfig::default(),
            limits: Limits::default(),
            hash_media: true,
        }
    }
}

impl AnalysisConfig {
    pub fn wants(&self, capability: Capability) -> bool {
        self.modules.contains(&capability)
    }

    /// Remove duplicates and unknown-to-this-release capabilities.
    pub fn normalize(&mut self) {
        self.modules.sort_unstable();
        self.modules.dedup();
        self.modules.retain(|c| {
            matches!(
                c,
                Capability::Inspect
                    | Capability::Vad
                    | Capability::Transcribe
                    | Capability::Diarize
                    | Capability::Music
                    | Capability::Acoustic
            )
        });
    }

    /// Stable hash of the effective parameters. Field order is fixed by the
    /// struct definition, and `serde_json` preserves it, so the digest is
    /// reproducible across runs and machines.
    pub fn params_sha256(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        let digest = Sha256::digest(&bytes);
        format!("{digest:x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_parsing_accepts_aliases() {
        assert_eq!(Capability::parse("asr"), Some(Capability::Transcribe));
        assert_eq!(Capability::parse("Speakers"), Some(Capability::Diarize));
        assert_eq!(Capability::parse("nope"), None);
    }

    #[test]
    fn params_hash_is_stable_and_sensitive() {
        let a = AnalysisConfig::default();
        let b = AnalysisConfig::default();
        assert_eq!(a.params_sha256(), b.params_sha256());

        let mut c = AnalysisConfig::default();
        c.vad.threshold = 0.7;
        assert_ne!(a.params_sha256(), c.params_sha256());
    }

    #[test]
    fn normalize_removes_duplicates() {
        let mut cfg = AnalysisConfig {
            modules: vec![Capability::Vad, Capability::Vad, Capability::Music],
            ..AnalysisConfig::default()
        };
        cfg.normalize();
        assert_eq!(cfg.modules, vec![Capability::Vad, Capability::Music]);
    }
}
