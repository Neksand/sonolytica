//! Document level analysis results that are not per-segment events:
//! music features, acoustic summaries, speech statistics.

use serde::{Deserialize, Serialize};

use crate::time::{MediaTime, TimeRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PitchClass {
    C,
    CSharp,
    D,
    DSharp,
    E,
    F,
    FSharp,
    G,
    GSharp,
    A,
    ASharp,
    B,
}

impl PitchClass {
    pub const ALL: [Self; 12] = [
        Self::C,
        Self::CSharp,
        Self::D,
        Self::DSharp,
        Self::E,
        Self::F,
        Self::FSharp,
        Self::G,
        Self::GSharp,
        Self::A,
        Self::ASharp,
        Self::B,
    ];

    pub const fn from_index(index: usize) -> Self {
        Self::ALL[index % 12]
    }

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::CSharp => "C#",
            Self::D => "D",
            Self::DSharp => "D#",
            Self::E => "E",
            Self::F => "F",
            Self::FSharp => "F#",
            Self::G => "G",
            Self::GSharp => "G#",
            Self::A => "A",
            Self::ASharp => "A#",
            Self::B => "B",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Major,
    Minor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChordQuality {
    Major,
    Minor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bpm {
    pub value: f32,
    pub confidence: crate::document::Confidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyEstimate {
    pub tonic: PitchClass,
    pub mode: Mode,
    pub confidence: crate::document::Confidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chord {
    pub range: TimeRange,
    pub root: PitchClass,
    pub quality: ChordQuality,
    pub confidence: crate::document::Confidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Loudness {
    /// EBU R128 integrated loudness in LUFS.
    pub integrated_lufs: f32,
    /// Loudness range in LU, when enough programme material exists.
    pub loudness_range_lu: Option<f32>,
    pub true_peak_dbtp: Option<f32>,
}

/// A technical section boundary from chroma self-similarity. No semantic label
/// ("verse", "chorus") is invented.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub range: TimeRange,
    /// Novelty at the boundary that starts this section; 0 for the first.
    pub novelty: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MusicSummary {
    pub bpm: Option<Bpm>,
    /// Beat positions on the media timeline.
    pub beats: Vec<MediaTime>,
    pub key: Option<KeyEstimate>,
    pub chords: Vec<Chord>,
    /// Repeating-material sections, technical (not verse/chorus).
    #[serde(default)]
    pub sections: Vec<Section>,
    pub loudness: Option<Loudness>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AcousticSummary {
    /// Detected silence, as measured on the analysis signal.
    pub silence: Vec<TimeRange>,
    pub dynamic_range_lu: Option<f32>,
    pub mean_rms_dbfs: Option<f32>,
    pub peak_dbfs: Option<f32>,
    /// Non-speech events, when a sound event model is available.
    pub events: Vec<AcousticEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcousticEvent {
    pub range: TimeRange,
    pub class: String,
    pub confidence: Option<crate::document::Confidence>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpeechSummary {
    /// Dominant detected language (ISO 639-1), when the ASR model reports one.
    pub language: Option<String>,
    pub speaker_count: u32,
    pub speech_duration: MediaTime,
    /// Fraction of the analysed media containing speech, 0.0..=1.0.
    pub speech_ratio: f32,
    pub word_count: u64,
}

/// A capability's aggregate output. Tagged so new analysis domains can be
/// added without changing the document shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnalysisBlock {
    Speech(SpeechSummary),
    Music(MusicSummary),
    Acoustic(AcousticSummary),
}
