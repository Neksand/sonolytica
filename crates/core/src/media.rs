//! Media metadata, decoupled from any particular probing tool.

use serde::{Deserialize, Serialize};

use crate::time::MediaTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    Audio,
    Video,
    Subtitle,
    Data,
    Attachment,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub index: u32,
    pub kind: StreamKind,
    pub codec: Option<String>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Frames per second, including non-integer rates such as 29.97.
    pub frame_rate: Option<f64>,
    pub language: Option<String>,
    pub duration: Option<MediaTime>,
    pub bit_rate: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MediaInfo {
    /// Absolute path as resolved by the caller.
    pub path: String,
    /// SHA-256 of the file contents, when hashing was performed.
    pub sha256: Option<String>,
    pub size_bytes: Option<u64>,
    pub container: Option<String>,
    pub duration: Option<MediaTime>,
    pub bit_rate: Option<u64>,
    pub streams: Vec<StreamInfo>,
}

impl MediaInfo {
    pub fn primary_audio(&self) -> Option<&StreamInfo> {
        self.streams.iter().find(|s| s.kind == StreamKind::Audio)
    }

    pub fn primary_video(&self) -> Option<&StreamInfo> {
        self.streams.iter().find(|s| s.kind == StreamKind::Video)
    }

    pub fn has_audio(&self) -> bool {
        self.primary_audio().is_some()
    }

    pub fn has_video(&self) -> bool {
        self.primary_video().is_some()
    }

    pub fn audio_duration(&self) -> Option<MediaTime> {
        self.primary_audio()
            .and_then(|s| s.duration)
            .or(self.duration)
    }
}
