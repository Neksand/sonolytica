//! The document: tracks, a segment hierarchy and typed relations.
//!
//! Design decision (see `docs/ARCHITECTURE.md`): a document is *not* a flat
//! event list, and not a free-form graph. It is a set of modality tracks whose
//! segments form a parent/child hierarchy (utterance -> speaker turn -> word),
//! plus typed edges between segments for cross-modal facts (overlap,
//! interruption, accompaniment). Segments live in one arena so edges can
//! reference them by a compact id, and every segment is attributable to a run.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::analysis::AnalysisBlock;
use crate::config::AnalysisConfig;
use crate::media::MediaInfo;
use crate::provenance::{RunId, RunRecord, RunStatus, Warning};
use crate::time::{MediaTime, TimeRange};
use crate::{GENERATOR_NAME, GENERATOR_VERSION, SCHEMA_VERSION};

pub type TrackId = u32;
pub type SegmentId = u32;

/// A probability in `[0, 1]`. Construction rejects anything else so a
/// deserialized document cannot smuggle in `NaN` or `1.7` as confidence.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Confidence(f32);

impl Confidence {
    pub fn new(value: f32) -> Option<Self> {
        (value.is_finite() && (0.0..=1.0).contains(&value)).then_some(Self(value))
    }

    pub fn get(self) -> f32 {
        self.0
    }

    /// Clamp a heuristic score into a valid confidence. Non-finite input
    /// becomes 0 rather than producing an invalid document.
    pub fn clamped(value: f32) -> Self {
        Self(if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        })
    }

    pub fn is_valid(self) -> bool {
        self.0.is_finite() && (0.0..=1.0).contains(&self.0)
    }
}

impl PartialEq for Confidence {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.3}", self.0)
    }
}

/// Opaque, stable speaker label within one document.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SpeakerId(String);

impl SpeakerId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SpeakerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub range: TimeRange,
    pub confidence: Option<Confidence>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Speech,
    SpeakerTurn,
    Music,
    Acoustic,
    Scene,
    Object,
    Face,
    /// Text read from the picture (OCR), one segment per line.
    Text,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub kind: TrackKind,
    pub label: String,
    pub run: RunId,
}

/// What a segment asserts. Tagged, so new modalities do not change the shape
/// of segments produced by existing ones.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SegmentKind {
    /// A recognised utterance as produced by an ASR model.
    Speech {
        text: String,
        words: Vec<Word>,
        language: Option<String>,
        no_speech_prob: Option<f32>,
    },
    /// A derived speaker turn. `text` may be empty when no ASR output exists,
    /// but it is never invented.
    SpeakerTurn {
        speaker: SpeakerId,
        text: String,
        overlap: bool,
    },
    MusicLabel {
        label: String,
    },
    AcousticEvent {
        class: String,
    },
    Scene {
        index: u32,
        cut: bool,
    },
    Object {
        class: String,
        bbox: Option<BBox>,
    },
    /// Text visible in the picture at one instant, as produced by OCR. The
    /// range is the keyframe's instant, not a claim about how long the text
    /// was on screen.
    TextOverlay {
        text: String,
        bbox: BBox,
    },
}

impl SegmentKind {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Speech { .. } => "speech",
            Self::SpeakerTurn { .. } => "speaker_turn",
            Self::MusicLabel { .. } => "music_label",
            Self::AcousticEvent { .. } => "acoustic_event",
            Self::Scene { .. } => "scene",
            Self::Object { .. } => "object",
            Self::TextOverlay { .. } => "text_overlay",
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Speech { text, .. }
            | Self::SpeakerTurn { text, .. }
            | Self::TextOverlay { text, .. } => Some(text),
            _ => None,
        }
    }

    pub fn speaker(&self) -> Option<&SpeakerId> {
        match self {
            Self::SpeakerTurn { speaker, .. } => Some(speaker),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    /// Equals the segment's index in [`Document::segments`].
    pub id: SegmentId,
    pub track: TrackId,
    pub range: TimeRange,
    /// Parent segment, when this segment is a refinement of another (a
    /// speaker turn inside an utterance, a word inside a turn, ...).
    pub parent: Option<SegmentId>,
    pub kind: SegmentKind,
    pub confidence: Option<Confidence>,
    pub run: RunId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    /// Two segments partially share time.
    Overlaps,
    /// `from` starts while `to` is still speaking and continues past it.
    Interrupts,
    /// Temporal containment that is not a parent/child refinement.
    Contains,
    /// Produced independently, aligned on the timeline (e.g. scene cut and
    /// acoustic event).
    AlignsWith,
    OccursDuring,
    /// Two turns attributed to the same speaker (transitive identity).
    SameSpeakerAs,
    Accompanies,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub from: SegmentId,
    pub to: SegmentId,
    pub kind: RelationKind,
    pub confidence: Option<Confidence>,
}

/// One speaker-attributed stretch of dialogue, ordered by time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueTurn {
    pub speaker: SpeakerId,
    pub range: TimeRange,
    pub text: String,
    pub overlap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Generator {
    pub name: String,
    pub version: String,
}

impl Default for Generator {
    fn default() -> Self {
        Self {
            name: GENERATOR_NAME.to_string(),
            version: GENERATOR_VERSION.to_string(),
        }
    }
}

/// A file produced alongside the document, such as a keyframe image.
///
/// Paths are relative to the document file so a document and its artifacts can
/// be moved together; an absolute path is accepted when the artifact lives
/// elsewhere.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// `keyframe`, `contact_sheet`, ...
    pub kind: String,
    pub path: String,
    /// Where on the timeline the artifact belongs. A still image uses a point
    /// range (`start == end`).
    pub range: TimeRange,
    pub sha256: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// The complete, versioned analysis result for one media file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: String,
    pub generator: Generator,
    pub created_unix_ms: u64,
    pub media: MediaInfo,
    pub config: AnalysisConfig,
    pub runs: Vec<RunRecord>,
    pub tracks: Vec<Track>,
    pub segments: Vec<Segment>,
    pub relations: Vec<Relation>,
    #[serde(default)]
    pub analyses: Vec<AnalysisBlock>,
    #[serde(default)]
    pub warnings: Vec<Warning>,
    /// Files that belong to this analysis (keyframes, contact sheets). They
    /// are not part of the timeline; `range` ties each one to an instant.
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
}

/// A violated document invariant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Violation {
    pub code: String,
    pub message: String,
}

impl Violation {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

impl Document {
    pub fn new(media: MediaInfo, config: AnalysisConfig) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            generator: Generator::default(),
            created_unix_ms: now_unix_ms(),
            media,
            config,
            runs: Vec::new(),
            tracks: Vec::new(),
            segments: Vec::new(),
            relations: Vec::new(),
            analyses: Vec::new(),
            warnings: Vec::new(),
            artifacts: Vec::new(),
        }
    }

    pub fn duration(&self) -> MediaTime {
        self.media
            .audio_duration()
            .or(self.media.duration)
            .unwrap_or(MediaTime::ZERO)
    }

    pub fn run(&self, id: RunId) -> Option<&RunRecord> {
        self.runs.get(id as usize)
    }

    /// True when every run that was asked to produce output succeeded.
    pub fn is_complete(&self) -> bool {
        self.runs
            .iter()
            .all(|r| matches!(r.status, RunStatus::Complete))
    }

    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.get(id as usize)
    }

    pub fn segment(&self, id: SegmentId) -> Option<&Segment> {
        self.segments.get(id as usize)
    }

    /// Segments intersecting `range`, ordered by start time.
    ///
    /// A linear scan is intentional at this size (thousands of segments); an
    /// interval index is only worth its complexity once profiling says so.
    pub fn segments_in(&self, range: TimeRange) -> Vec<&Segment> {
        let mut found: Vec<&Segment> = self
            .segments
            .iter()
            .filter(|s| s.range.intersects(&range))
            .collect();
        found.sort_by_key(|s| (s.range.start, s.range.end, s.id));
        found
    }

    /// Segments active at an instant.
    pub fn at(&self, t: MediaTime) -> Vec<&Segment> {
        self.segments
            .iter()
            .filter(|s| s.range.contains(t))
            .collect()
    }

    pub fn tracks_of(&self, kind: TrackKind) -> impl Iterator<Item = &Track> {
        self.tracks.iter().filter(move |t| t.kind == kind)
    }

    pub fn segments_of_kind<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Segment> {
        self.segments
            .iter()
            .filter(move |s| s.kind.kind_name() == kind)
    }

    /// Ordered speaker-attributed turns.
    pub fn dialogue(&self) -> Vec<DialogueTurn> {
        let mut turns: Vec<DialogueTurn> = self
            .segments_of_kind("speaker_turn")
            .filter_map(|s| match &s.kind {
                SegmentKind::SpeakerTurn {
                    speaker,
                    text,
                    overlap,
                } => Some(DialogueTurn {
                    speaker: speaker.clone(),
                    range: s.range,
                    text: text.clone(),
                    overlap: *overlap,
                }),
                _ => None,
            })
            .collect();
        turns.sort_by_key(|t| (t.range.start, t.range.end));
        turns
    }

    pub fn speakers(&self) -> Vec<SpeakerId> {
        let set: BTreeSet<SpeakerId> = self
            .segments
            .iter()
            .filter_map(|s| s.kind.speaker().cloned())
            .collect();
        set.into_iter().collect()
    }

    /// Case-insensitive substring search over speech and speaker-turn text.
    pub fn search(&self, needle: &str) -> Vec<&Segment> {
        let needle = needle.to_lowercase();
        let mut found: Vec<&Segment> = self
            .segments
            .iter()
            .filter(|s| {
                s.kind
                    .text()
                    .is_some_and(|t| t.to_lowercase().contains(&needle))
            })
            .collect();
        found.sort_by_key(|s| s.range.start);
        found
    }

    pub fn relations_of(&self, id: SegmentId) -> impl Iterator<Item = &Relation> {
        self.relations
            .iter()
            .filter(move |r| r.from == id || r.to == id)
    }

    /// Full invariant check. Serialized documents must pass this before they
    /// are considered usable output.
    pub fn validate(&self) -> Vec<Violation> {
        let mut out = Vec::new();

        let supported_major = SCHEMA_VERSION.split('.').next().unwrap_or("1");
        if self.schema_version.split('.').next() != Some(supported_major) {
            out.push(Violation::new(
                "schema_version",
                format!(
                    "document schema {} is not compatible with {}",
                    self.schema_version, SCHEMA_VERSION
                ),
            ));
        }

        if self.media.path.is_empty() {
            out.push(Violation::new("media_path", "media path is empty"));
        }

        // The container is the timeline; an audio stream can end before the
        // picture (or be missing entirely), and scene/text segments still belong
        // to the media. Audio duration is only a fallback.
        let media_end = self.media.duration.or_else(|| self.media.audio_duration());

        for (idx, segment) in self.segments.iter().enumerate() {
            if segment.id as usize != idx {
                out.push(Violation::new(
                    "segment_id",
                    format!("segment at position {idx} has id {}", segment.id),
                ));
            }
            if segment.range.end < segment.range.start {
                out.push(Violation::new(
                    "range_order",
                    format!("segment {} has end before start", segment.id),
                ));
            }
            if self.track(segment.track).is_none() {
                out.push(Violation::new(
                    "track_ref",
                    format!("segment {} references track {}", segment.id, segment.track),
                ));
            }
            if self.run(segment.run).is_none() {
                out.push(Violation::new(
                    "run_ref",
                    format!("segment {} references run {}", segment.id, segment.run),
                ));
            }
            if let Some(parent) = segment.parent {
                match self.segment(parent) {
                    None => out.push(Violation::new(
                        "parent_ref",
                        format!("segment {} references parent {parent}", segment.id),
                    )),
                    Some(p) if p.id == segment.id => out.push(Violation::new(
                        "parent_cycle",
                        format!("segment {} is its own parent", segment.id),
                    )),
                    Some(p) => {
                        if !p.range.contains_range(&segment.range) {
                            out.push(Violation::new(
                                "parent_contains_child",
                                format!(
                                    "segment {} {} is not contained by parent {} {}",
                                    segment.id, segment.range, p.id, p.range
                                ),
                            ));
                        }
                        if p.track == segment.track {
                            out.push(Violation::new(
                                "parent_same_track",
                                format!(
                                    "segment {} and parent {} share a track; hierarchy \
                                     must cross tracks",
                                    segment.id, p.id
                                ),
                            ));
                        }
                    }
                }
            }
            if let Some(end) = media_end {
                if segment.range.end > end {
                    out.push(Violation::new(
                        "segment_bounds",
                        format!(
                            "segment {} ends at {} beyond the media duration {}",
                            segment.id, segment.range.end, end
                        ),
                    ));
                }
            }
            if let SegmentKind::Speech { words, .. } = &segment.kind {
                for word in words {
                    if let Some(end) = media_end {
                        if word.range.end > end {
                            out.push(Violation::new(
                                "word_bounds",
                                format!(
                                    "word `{}` ends at {} beyond the media duration {}",
                                    word.text, word.range.end, end
                                ),
                            ));
                        }
                    }
                    if word.range.start > word.range.end {
                        out.push(Violation::new(
                            "word_order",
                            format!("word `{}` ends before it starts", word.text),
                        ));
                    }
                }
            }
            if let Some(c) = segment.confidence {
                if !c.is_valid() {
                    out.push(Violation::new(
                        "confidence_range",
                        format!("segment {} has invalid confidence {}", segment.id, c.0),
                    ));
                }
            }
        }

        for (idx, track) in self.tracks.iter().enumerate() {
            if track.id as usize != idx {
                out.push(Violation::new(
                    "track_id",
                    format!("track at position {idx} has id {}", track.id),
                ));
            }
            if self.run(track.run).is_none() {
                out.push(Violation::new(
                    "track_run_ref",
                    format!("track {} references run {}", track.id, track.run),
                ));
            }
        }

        for (idx, run) in self.runs.iter().enumerate() {
            if run.id as usize != idx {
                out.push(Violation::new(
                    "run_id",
                    format!("run at position {idx} has id {}", run.id),
                ));
            }
            if run.params_sha256.len() != 64 {
                out.push(Violation::new(
                    "params_hash",
                    format!("run {} has a malformed params hash", run.id),
                ));
            }
        }

        for relation in &self.relations {
            if relation.from as usize >= self.segments.len()
                || relation.to as usize >= self.segments.len()
            {
                out.push(Violation::new(
                    "relation_ref",
                    format!(
                        "relation {} -> {} references a missing segment",
                        relation.from, relation.to
                    ),
                ));
            }
            if relation.from == relation.to {
                out.push(Violation::new(
                    "relation_self",
                    format!("relation on segment {} points at itself", relation.from),
                ));
            }
        }

        for artifact in &self.artifacts {
            if artifact.path.is_empty() {
                out.push(Violation::new("artifact_path", "artifact path is empty"));
            }
            if artifact.range.end < artifact.range.start {
                out.push(Violation::new(
                    "artifact_range",
                    format!("artifact {} ends before it starts", artifact.path),
                ));
            }
            if let Some(end) = media_end {
                if artifact.range.end > end {
                    out.push(Violation::new(
                        "artifact_bounds",
                        format!(
                            "artifact {} at {} is past the media duration {end}",
                            artifact.path, artifact.range.end
                        ),
                    ));
                }
            }
        }

        out
    }

    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parse a document, refusing schemas this build cannot interpret.
    pub fn from_json(json: &str) -> crate::Result<Self> {
        let doc: Self = serde_json::from_str(json)?;
        let supported_major = SCHEMA_VERSION.split('.').next().unwrap_or("1");
        if doc.schema_version.split('.').next() != Some(supported_major) {
            return Err(crate::CoreError::SchemaVersion {
                found: doc.schema_version,
                supported: SCHEMA_VERSION.to_string(),
            });
        }
        Ok(doc)
    }
}

/// Ergonomic construction with invariant enforcement on `finish`.
#[derive(Debug)]
pub struct DocumentBuilder {
    doc: Document,
}

impl DocumentBuilder {
    pub fn new(media: MediaInfo, config: AnalysisConfig) -> Self {
        Self {
            doc: Document::new(media, config),
        }
    }

    pub fn run_count(&self) -> RunId {
        self.doc.runs.len() as RunId
    }

    pub fn add_run(&mut self, mut record: RunRecord) -> RunId {
        record.id = self.run_count();
        let id = record.id;
        self.doc.runs.push(record);
        id
    }

    pub fn add_track(&mut self, kind: TrackKind, label: impl Into<String>, run: RunId) -> TrackId {
        let id = self.doc.tracks.len() as TrackId;
        self.doc.tracks.push(Track {
            id,
            kind,
            label: label.into(),
            run,
        });
        id
    }

    pub fn add_segment(
        &mut self,
        track: TrackId,
        range: TimeRange,
        kind: SegmentKind,
        confidence: Option<Confidence>,
        run: RunId,
    ) -> SegmentId {
        let id = self.doc.segments.len() as SegmentId;
        self.doc.segments.push(Segment {
            id,
            track,
            range,
            parent: None,
            kind,
            confidence,
            run,
        });
        id
    }

    pub fn set_parent(&mut self, child: SegmentId, parent: Option<SegmentId>) {
        if let Some(segment) = self.doc.segments.get_mut(child as usize) {
            segment.parent = parent;
        }
    }

    pub fn add_relation(
        &mut self,
        from: SegmentId,
        to: SegmentId,
        kind: RelationKind,
        confidence: Option<Confidence>,
    ) {
        self.doc.relations.push(Relation {
            from,
            to,
            kind,
            confidence,
        });
    }

    pub fn add_analysis(&mut self, block: AnalysisBlock) {
        self.doc.analyses.push(block);
    }

    pub fn add_artifact(&mut self, artifact: Artifact) {
        self.doc.artifacts.push(artifact);
    }

    pub fn warn(&mut self, warning: Warning) {
        self.doc.warnings.push(warning);
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Finish, returning the document only if it satisfies every invariant.
    pub fn finish(self) -> Result<Document, Vec<Violation>> {
        let violations = self.doc.validate();
        if violations.is_empty() {
            Ok(self.doc)
        } else {
            Err(violations)
        }
    }
}

pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AnalysisConfig;
    use crate::media::MediaInfo;
    use crate::provenance::{ModelRef, RunRecord, RunStatus};

    fn run() -> RunRecord {
        RunRecord {
            id: 0,
            capability: crate::Capability::Vad,
            provider: "test".into(),
            model: Some(ModelRef::new("test-model")),
            params_sha256: "0".repeat(64),
            device: "cpu".into(),
            started_at_unix_ms: 0,
            duration_ms: 1,
            status: RunStatus::Complete,
        }
    }

    fn media() -> MediaInfo {
        MediaInfo {
            path: "/tmp/fixture.wav".into(),
            ..MediaInfo::default()
        }
    }

    fn fixture() -> Document {
        let mut b = DocumentBuilder::new(media(), AnalysisConfig::default());
        let r = b.add_run(run());
        let speech = b.add_track(TrackKind::Speech, "asr", r);
        let turns = b.add_track(TrackKind::SpeakerTurn, "diarization+asr", r);
        let utt = b.add_segment(
            speech,
            TimeRange::from_secs(1.0, 3.0),
            SegmentKind::Speech {
                text: "hello world".into(),
                words: vec![Word {
                    text: "hello".into(),
                    range: TimeRange::from_secs(1.0, 1.4),
                    confidence: Confidence::new(0.9),
                }],
                language: Some("en".into()),
                no_speech_prob: None,
            },
            None,
            r,
        );
        let turn = b.add_segment(
            turns,
            TimeRange::from_secs(1.0, 3.0),
            SegmentKind::SpeakerTurn {
                speaker: SpeakerId::new("S1"),
                text: "hello world".into(),
                overlap: false,
            },
            Confidence::new(0.8),
            r,
        );
        b.set_parent(turn, Some(utt));
        b.finish().expect("fixture is valid")
    }

    #[test]
    fn valid_document_passes_and_queries_work() {
        let doc = fixture();
        assert!(doc.validate().is_empty());
        assert_eq!(doc.dialogue().len(), 1);
        assert_eq!(doc.speakers(), vec![SpeakerId::new("S1")]);
        assert_eq!(doc.segments_in(TimeRange::from_secs(2.8, 5.0)).len(), 2);
        assert_eq!(doc.search("HELLO").len(), 2);
        assert_eq!(doc.at(MediaTime::from_secs_f64(1.0)).len(), 2);
    }

    #[test]
    fn child_outside_parent_is_rejected() {
        let mut b = DocumentBuilder::new(media(), AnalysisConfig::default());
        let r = b.add_run(run());
        let t1 = b.add_track(TrackKind::Speech, "asr", r);
        let t2 = b.add_track(TrackKind::SpeakerTurn, "diarization", r);
        let parent = b.add_segment(
            t1,
            TimeRange::from_secs(0.0, 1.0),
            SegmentKind::Speech {
                text: "a".into(),
                words: vec![],
                language: None,
                no_speech_prob: None,
            },
            None,
            r,
        );
        let child = b.add_segment(
            t2,
            TimeRange::from_secs(2.0, 3.0),
            SegmentKind::SpeakerTurn {
                speaker: SpeakerId::new("S1"),
                text: String::new(),
                overlap: false,
            },
            None,
            r,
        );
        b.set_parent(child, Some(parent));
        let violations = b.finish().expect_err("must fail");
        assert!(violations.iter().any(|v| v.code == "parent_contains_child"));
    }

    #[test]
    fn roundtrip_preserves_shape_and_rejects_future_schema() {
        let doc = fixture();
        let json = doc.to_json_pretty().unwrap();
        let back = Document::from_json(&json).unwrap();
        assert_eq!(back.segments.len(), 2);
        assert_eq!(back.dialogue()[0].text, "hello world");

        let future = json.replace(SCHEMA_VERSION, "2.0.0");
        assert!(Document::from_json(&future).is_err());
    }

    #[test]
    fn artifacts_must_have_a_path_and_fit_the_media() {
        let mut document = fixture();
        document.artifacts.push(Artifact {
            kind: "keyframe".into(),
            path: String::new(),
            range: TimeRange::from_secs(0.0, 0.0),
            sha256: None,
            width: Some(1280),
            height: None,
        });
        let violations = document.validate();
        assert!(
            violations.iter().any(|v| v.code == "artifact_path"),
            "empty path must be rejected: {violations:?}"
        );

        // The fixture has no media duration, so add one and place the frame
        // past it.
        let mut document = fixture();
        document.media.duration = Some(MediaTime::from_secs_f64(6.0));
        document.artifacts.push(Artifact {
            kind: "keyframe".into(),
            path: "doc.artifacts/frames/frame_0000.jpg".into(),
            range: TimeRange::from_secs(9.0, 9.0),
            sha256: None,
            width: Some(1280),
            height: None,
        });
        let violations = document.validate();
        assert!(
            violations.iter().any(|v| v.code == "artifact_bounds"),
            "a frame past the media must be rejected: {violations:?}"
        );
    }

    #[test]
    fn artifacts_may_outlast_a_short_audio_stream() {
        // Codex review finding: a video can outlast its audio; keyframes after
        // the audio end are still inside the container and must be valid.
        let mut document = fixture();
        document.media.duration = Some(MediaTime::from_secs_f64(10.0));
        document.media.streams.push(crate::media::StreamInfo {
            index: 0,
            kind: crate::media::StreamKind::Audio,
            codec: None,
            sample_rate: None,
            channels: None,
            width: None,
            height: None,
            frame_rate: None,
            language: None,
            duration: Some(MediaTime::from_secs_f64(5.0)),
            bit_rate: None,
        });
        document.artifacts.push(Artifact {
            kind: "keyframe".into(),
            path: "doc.artifacts/frames/frame_0001.jpg".into(),
            range: TimeRange::from_secs(9.0, 9.0),
            sha256: None,
            width: Some(1280),
            height: None,
        });
        let violations = document.validate();
        assert!(
            violations.is_empty(),
            "a frame after the audio end is inside the container: {violations:?}"
        );
    }

    #[test]
    fn text_overlay_carries_a_box_and_is_searchable() {
        let mut document = fixture();
        document.media.duration = Some(MediaTime::from_secs_f64(10.0));
        let run = 0;
        let track = document.tracks.len() as TrackId;
        document.tracks.push(Track {
            id: track,
            kind: TrackKind::Text,
            label: "ocr".into(),
            run,
        });
        let instant = MediaTime::from_secs_f64(2.0);
        document.segments.push(Segment {
            id: document.segments.len() as SegmentId,
            track,
            range: TimeRange::empty(instant),
            parent: None,
            kind: SegmentKind::TextOverlay {
                text: "Quarterly revenue".into(),
                bbox: BBox {
                    x: 12.0,
                    y: 34.0,
                    width: 300.0,
                    height: 40.0,
                },
            },
            confidence: Confidence::new(0.95),
            run,
        });

        assert!(document.validate().is_empty(), "{:?}", document.validate());
        let overlay = document.segments.last().unwrap();
        assert_eq!(overlay.kind.kind_name(), "text_overlay");
        assert_eq!(overlay.kind.text(), Some("Quarterly revenue"));
        assert_eq!(document.search("revenue").len(), 1);
        assert_eq!(document.tracks_of(TrackKind::Text).count(), 1);

        let json = document.to_json_pretty().unwrap();
        assert!(json.contains("\"type\": \"text_overlay\""));
        assert!(json.contains("\"height\": 40.0"));
    }

    #[test]
    fn video_segments_are_bounded_by_the_container() {
        // The same rule as artifacts: a picture can outlast its audio, and a
        // text overlay after the audio end is still inside the media.
        let mut document = fixture();
        document.media.duration = Some(MediaTime::from_secs_f64(10.0));
        document.media.streams.push(crate::media::StreamInfo {
            index: 0,
            kind: crate::media::StreamKind::Audio,
            codec: None,
            sample_rate: None,
            channels: None,
            width: None,
            height: None,
            frame_rate: None,
            language: None,
            duration: Some(MediaTime::from_secs_f64(5.0)),
            bit_rate: None,
        });
        document.tracks.push(Track {
            id: 2,
            kind: TrackKind::Text,
            label: "ocr".into(),
            run: 0,
        });
        document.segments.push(Segment {
            id: 2,
            track: 2,
            parent: None,
            range: TimeRange::from_secs(9.0, 9.0),
            kind: SegmentKind::TextOverlay {
                text: "hello".into(),
                bbox: BBox {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
            },
            confidence: None,
            run: 0,
        });
        let violations = document.validate();
        assert!(
            violations.is_empty(),
            "a text overlay after the audio end is inside the container: {violations:?}"
        );
    }

    #[test]
    fn confidence_rejects_out_of_range_values() {
        assert!(Confidence::new(0.0).is_some());
        assert!(Confidence::new(1.0).is_some());
        assert!(Confidence::new(1.01).is_none());
        assert!(Confidence::new(f32::NAN).is_none());
    }
}
