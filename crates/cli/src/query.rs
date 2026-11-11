//! Read-only queries over an analysed document.
//!
//! These commands exist so an agent can answer questions about a document
//! without `jq`: every result is either a stable plain-text table or a typed
//! JSON object. Nothing here mutates the document or touches a model.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use serde::Serialize;
use sonolytica_core::{
    AcousticSummary, AnalysisBlock, AnalysisConfig, Bpm, Capability, Chord, Document, KeyEstimate,
    Loudness, MediaInfo, MediaTime, MusicSummary, RunRecord, RunStatus, Section, Segment,
    SegmentKind, SpeakerId, SpeechSummary, TimeRange, Warning, Word,
};

/// Segment kinds a document can contain. Kept explicit so `--type` rejects a
/// typo instead of silently returning nothing.
const SEGMENT_KINDS: [&str; 7] = [
    "speech",
    "speaker_turn",
    "music_label",
    "acoustic_event",
    "scene",
    "object",
    "text_overlay",
];

/// Text is truncated by characters, not bytes: transcripts are UTF-8 and a
/// byte slice would panic on the first non-ASCII character.
const TEXT_WIDTH: usize = 80;

#[derive(Debug, Args)]
pub struct QueryArgs {
    #[command(subcommand)]
    pub command: QueryCommand,
}

#[derive(Debug, Subcommand)]
pub enum QueryCommand {
    /// Segments in a time window, ordered by start
    Timeline {
        /// Document produced by `sonolytica analyze`
        doc: PathBuf,
        /// Window start: `HH:MM:SS.mmm`, `MM:SS`, `12.5s`, `950ms` or µs
        /// (default 0)
        #[arg(long)]
        from: Option<String>,
        /// Window end: `HH:MM:SS.mmm`, `MM:SS`, `12.5s`, `950ms` or µs
        /// (default media duration)
        #[arg(long)]
        to: Option<String>,
        /// Restrict to one segment type (speech, speaker_turn, music_label,
        /// acoustic_event, scene, object)
        #[arg(long = "type")]
        kind: Option<String>,
        /// Emit typed JSON on stdout instead of a table
        #[arg(long)]
        json: bool,
    },
    /// What is active at one instant
    At {
        /// Document produced by `sonolytica analyze`
        doc: PathBuf,
        /// Instant: `HH:MM:SS.mmm`, `MM:SS`, `12.5s`, `950ms` or µs
        time: String,
        /// Emit typed JSON on stdout instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Case-insensitive search over transcript text
    Search {
        /// Document produced by `sonolytica analyze`
        doc: PathBuf,
        /// Substring to find in text and word timings
        needle: String,
        /// Emit typed JSON on stdout instead of a table
        #[arg(long)]
        json: bool,
    },
    /// The whole transcript, untruncated, one line per utterance
    Transcript {
        /// Document produced by `sonolytica analyze`
        doc: PathBuf,
        /// Only utterances intersecting this window start
        #[arg(long)]
        from: Option<String>,
        /// Only utterances intersecting this window end
        #[arg(long)]
        to: Option<String>,
        /// Emit typed JSON on stdout instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Every word of one speech segment, with its own range
    Words {
        /// Document produced by `sonolytica analyze`
        doc: PathBuf,
        /// Segment id from `query timeline`
        segment: u32,
        /// Emit typed JSON on stdout instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Speaking time per speaker, longest first
    Speakers {
        /// Document produced by `sonolytica analyze`
        doc: PathBuf,
        /// Emit typed JSON on stdout instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Tempo, key, chords and loudness
    Music {
        /// Document produced by `sonolytica analyze`
        doc: PathBuf,
        /// Emit typed JSON on stdout instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Silence and level summary
    Acoustic {
        /// Document produced by `sonolytica analyze`
        doc: PathBuf,
        /// Emit typed JSON on stdout instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Stage status and warnings
    Runs {
        /// Document produced by `sonolytica analyze`
        doc: PathBuf,
        /// Emit typed JSON on stdout instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Chapter boundaries from silences, speaker changes and scene cuts
    Chapters {
        /// Document produced by `sonolytica analyze`
        doc: PathBuf,
        /// Minimum silence that starts a new chapter
        #[arg(long, default_value = "1.2s")]
        min_gap: String,
        /// Emit typed JSON on stdout instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Keyframe images registered in the document
    Frames {
        /// Document produced by `sonolytica analyze`
        doc: PathBuf,
        /// Only frames at or after this instant
        #[arg(long)]
        from: Option<String>,
        /// Only frames before this instant
        #[arg(long)]
        to: Option<String>,
        /// Return at most this many frames
        #[arg(long)]
        max: Option<usize>,
        /// Emit typed JSON on stdout instead of a table
        #[arg(long)]
        json: bool,
    },
    /// One compact overview: media, runs, speech, speakers, chapters, counts
    Brief {
        /// Document produced by `sonolytica analyze`
        doc: PathBuf,
        /// Emit typed JSON on stdout instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Media, counts, analyses, runs and warnings
    Summary {
        /// Document produced by `sonolytica analyze`
        doc: PathBuf,
        /// Emit typed JSON on stdout instead of a table
        #[arg(long)]
        json: bool,
    },
}

pub fn run(command: QueryCommand) -> Result<()> {
    match command {
        QueryCommand::Timeline {
            doc,
            from,
            to,
            kind,
            json,
        } => timeline(
            &load(&doc)?,
            from.as_deref(),
            to.as_deref(),
            kind.as_deref(),
            json,
        ),
        QueryCommand::At { doc, time, json } => at(&load(&doc)?, &time, json),
        QueryCommand::Search { doc, needle, json } => search(&load(&doc)?, &needle, json),
        QueryCommand::Transcript {
            doc,
            from,
            to,
            json,
        } => transcript(&load(&doc)?, from.as_deref(), to.as_deref(), json),
        QueryCommand::Words { doc, segment, json } => words(&load(&doc)?, segment, json),
        QueryCommand::Speakers { doc, json } => speakers(&load(&doc)?, json),
        QueryCommand::Music { doc, json } => music(&load(&doc)?, json),
        QueryCommand::Acoustic { doc, json } => acoustic(&load(&doc)?, json),
        QueryCommand::Runs { doc, json } => runs(&load(&doc)?, json),
        QueryCommand::Chapters { doc, min_gap, json } => chapters(&load(&doc)?, &min_gap, json),
        QueryCommand::Frames {
            doc,
            from,
            to,
            max,
            json,
        } => frames(
            &load(&doc)?,
            &doc,
            from.as_deref(),
            to.as_deref(),
            max,
            json,
        ),
        QueryCommand::Brief { doc, json } => brief(&load(&doc)?, json),
        QueryCommand::Summary { doc, json } => summary(&load(&doc)?, json),
    }
}

fn load(path: &Path) -> Result<Document> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;
    Document::from_json(&text).with_context(|| format!("could not parse {}", path.display()))
}

// ---------------------------------------------------------------- time input

/// Parse a point on the media timeline.
///
/// Accepted forms: `HH:MM:SS.mmm`, `HH:MM:SS`, `MM:SS[.mmm]`, `12.5s`, `950ms`,
/// `12.5` (seconds) and a bare integer (microseconds — the document's unit).
fn parse_time(input: &str) -> Result<MediaTime> {
    let text = input.trim();
    let invalid = || {
        anyhow!(
            "invalid time `{input}`: expected HH:MM:SS.mmm, MM:SS, 12.5s, 950ms or microseconds"
        )
    };
    if text.is_empty() {
        bail!("empty time value");
    }
    if text.starts_with('-') {
        bail!("time `{input}` is negative");
    }
    if text.contains('e') || text.contains('E') {
        return Err(invalid());
    }

    let parts: Vec<&str> = text.split(':').collect();
    let micros = match parts.as_slice() {
        [hours, minutes, seconds] => {
            parse_whole(hours, input)? as f64 * 3_600_000_000.0
                + parse_whole(minutes, input)? as f64 * 60_000_000.0
                + parse_number(seconds, input)? * 1.0e6
        }
        [minutes, seconds] => {
            parse_whole(minutes, input)? as f64 * 60_000_000.0
                + parse_number(seconds, input)? * 1.0e6
        }
        [bare] => {
            if let Some(value) = bare.strip_suffix("ms") {
                parse_number(value, input)? * 1_000.0
            } else if let Some(value) = bare.strip_suffix("us") {
                parse_number(value, input)?
            } else if let Some(value) = bare.strip_suffix('s') {
                parse_number(value, input)? * 1.0e6
            } else if bare.contains('.') {
                parse_number(bare, input)? * 1.0e6
            } else {
                parse_whole(bare, input)? as f64
            }
        }
        _ => return Err(invalid()),
    };

    let micros = micros.round();
    if !micros.is_finite() || micros < 0.0 || micros > i64::MAX as f64 {
        bail!("time `{input}` is out of range");
    }
    Ok(MediaTime::from_micros(micros as i64))
}

fn parse_whole(value: &str, whole: &str) -> Result<u64> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|_| anyhow!("invalid time `{whole}`"))
}

fn parse_number(value: &str, whole: &str) -> Result<f64> {
    let value: f64 = value
        .trim()
        .parse()
        .map_err(|_| anyhow!("invalid time `{whole}`"))?;
    if !value.is_finite() || value < 0.0 {
        bail!("invalid time `{whole}`");
    }
    Ok(value)
}

// ------------------------------------------------------------------ timeline

fn timeline(
    doc: &Document,
    from: Option<&str>,
    to: Option<&str>,
    kind: Option<&str>,
    json: bool,
) -> Result<()> {
    let start = from.map(parse_time).transpose()?.unwrap_or(MediaTime::ZERO);
    let end = match to {
        Some(value) => parse_time(value)?,
        None => {
            let duration = doc.duration();
            if duration == MediaTime::ZERO {
                MediaTime::MAX
            } else {
                duration
            }
        }
    };
    if end < start {
        bail!("`--to` ({}) is before `--from` ({})", end, start);
    }
    let kind = kind.map(validate_kind).transpose()?;

    let segments: Vec<&Segment> = doc
        .segments_in(TimeRange::new(start, end))
        .into_iter()
        .filter(|segment| kind.is_none_or(|kind| segment.kind.kind_name() == kind))
        .collect();

    if json {
        emit(&SegmentsOutput {
            count: segments.len(),
            segments: segments.iter().map(|s| SegmentView::new(s)).collect(),
        })
    } else {
        print_segments(&segments);
        Ok(())
    }
}

fn validate_kind(kind: &str) -> Result<&str> {
    SEGMENT_KINDS
        .iter()
        .copied()
        .find(|known| *known == kind)
        .ok_or_else(|| {
            anyhow!(
                "unknown segment type `{kind}`; expected one of: {}",
                SEGMENT_KINDS.join(", ")
            )
        })
}

fn at(doc: &Document, time: &str, json: bool) -> Result<()> {
    let instant = parse_time(time)?;
    let mut segments = doc.at(instant);
    segments.sort_by_key(|segment| (segment.range.start, segment.range.end, segment.id));

    if json {
        emit(&SegmentsOutput {
            count: segments.len(),
            segments: segments.iter().map(|s| SegmentView::new(s)).collect(),
        })
    } else {
        print_segments(&segments);
        Ok(())
    }
}

// -------------------------------------------------------------------- search

fn search(doc: &Document, needle: &str, json: bool) -> Result<()> {
    let needle_lower = needle.to_lowercase();
    let mut matches: Vec<MatchView<'_>> = Vec::new();

    for segment in doc.segments_of_kind("speech") {
        let SegmentKind::Speech { text, words, .. } = &segment.kind else {
            continue;
        };
        let text_hit = text.to_lowercase().contains(&needle_lower);
        let word_hits: Vec<&Word> = words
            .iter()
            .filter(|word| word.text.to_lowercase().contains(&needle_lower))
            .collect();
        if !text_hit && word_hits.is_empty() {
            continue;
        }

        // A turn is a refinement of its utterance (`parent`), so the speaker
        // for an ASR segment comes from the turn that points at it. Reporting
        // both rows would duplicate every match.
        let speaker = speaker_of(doc, segment);

        matches.push(MatchView {
            segment: SegmentView::with_speaker(segment, speaker),
            words: word_hits.iter().map(|word| WordView::new(word)).collect(),
        });
    }

    if json {
        emit(&SearchOutput {
            needle,
            count: matches.len(),
            matches,
        })
    } else {
        if matches.is_empty() {
            println!("(none)");
            return Ok(());
        }
        for hit in &matches {
            let mut line = view_line(&hit.segment);
            for word in &hit.words {
                let _ = write!(line, "  | {} {}", word.text, word.range_display);
            }
            println!("{line}");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- transcript

fn transcript(doc: &Document, from: Option<&str>, to: Option<&str>, json: bool) -> Result<()> {
    let start = from.map(parse_time).transpose()?.unwrap_or(MediaTime::ZERO);
    let end = match to {
        Some(value) => parse_time(value)?,
        None => {
            let duration = doc.duration();
            if duration == MediaTime::ZERO {
                MediaTime::MAX
            } else {
                duration
            }
        }
    };
    if end < start {
        bail!("`--to` ({}) is before `--from` ({})", end, start);
    }

    let mut rows: Vec<SegmentView<'_>> = doc
        .segments_of_kind("speech")
        .filter(|segment| segment.range.intersects(&TimeRange::new(start, end)))
        .map(|segment| SegmentView::with_speaker(segment, speaker_of(doc, segment)))
        .collect();
    rows.sort_by_key(|row| (row.start, row.id));

    if json {
        emit(&TranscriptOutput {
            count: rows.len(),
            segments: rows,
        })
    } else {
        if rows.is_empty() {
            println!("(no speech)");
            return Ok(());
        }
        for row in &rows {
            println!(
                "{}–{}  {:<4}  {}",
                row.start_display,
                row.end_display,
                row.speaker.unwrap_or("--"),
                row.text.unwrap_or("")
            );
        }
        Ok(())
    }
}

/// The speaker of an ASR segment, taken from the turn that refines it.
fn speaker_of<'a>(doc: &'a Document, segment: &'a Segment) -> Option<&'a str> {
    doc.segments
        .iter()
        .find(|candidate| {
            candidate.parent == Some(segment.id)
                && matches!(candidate.kind, SegmentKind::SpeakerTurn { .. })
        })
        .and_then(|turn| turn.kind.speaker())
        .map(SpeakerId::as_str)
}

// --------------------------------------------------------------------- words
fn words(doc: &Document, segment: u32, json: bool) -> Result<()> {
    let Some(segment) = doc.segment(segment) else {
        bail!("document has no segment {segment}");
    };
    let SegmentKind::Speech { text, words, .. } = &segment.kind else {
        if let Some(parent) = segment.parent {
            bail!(
                "segment {} is a {}; its words are in parent segment {parent}",
                segment.id,
                segment.kind.kind_name()
            );
        }
        bail!(
            "segment {} is a {}; it has no words",
            segment.id,
            segment.kind.kind_name()
        );
    };
    let speaker = speaker_of(doc, segment);

    if json {
        emit(&WordsOutput {
            segment: segment.id,
            speaker,
            text,
            start: segment.range.start.micros(),
            end: segment.range.end.micros(),
            start_display: segment.range.start.to_string(),
            end_display: segment.range.end.to_string(),
            words: words.iter().map(WordView::new).collect(),
        })
    } else {
        if words.is_empty() {
            println!("(no words)");
            return Ok(());
        }
        for word in words.iter().map(WordView::new) {
            let confidence = word
                .confidence
                .map_or_else(String::new, |value| format!("  [{value:.3}]"));
            println!("{}  {}{confidence}", word.range_display, word.text);
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ speakers
#[derive(Default)]
struct SpeakerAcc {
    duration: MediaTime,
    turns: u32,
    first: Option<MediaTime>,
    last: Option<MediaTime>,
}

/// Speaking time per speaker, longest first. Shared by `speakers` and `brief`.
fn speaker_views(doc: &Document) -> Vec<SpeakerView<'_>> {
    let mut acc: BTreeMap<&SpeakerId, SpeakerAcc> = BTreeMap::new();
    for segment in doc.segments_of_kind("speaker_turn") {
        let SegmentKind::SpeakerTurn { speaker, .. } = &segment.kind else {
            continue;
        };
        let entry = acc.entry(speaker).or_default();
        entry.duration = entry.duration + segment.range.duration();
        entry.turns += 1;
        entry.first = Some(
            entry
                .first
                .map_or(segment.range.start, |t| t.min(segment.range.start)),
        );
        entry.last = Some(
            entry
                .last
                .map_or(segment.range.end, |t| t.max(segment.range.end)),
        );
    }

    let mut speakers: Vec<SpeakerView<'_>> = acc
        .into_iter()
        .map(|(speaker, acc)| SpeakerView {
            id: speaker.as_str(),
            speech_duration_us: acc.duration.micros(),
            speech_display: acc.duration.to_string(),
            turn_count: acc.turns,
            first_start_us: acc.first.unwrap_or(MediaTime::ZERO).micros(),
            last_end_us: acc.last.unwrap_or(MediaTime::ZERO).micros(),
        })
        .collect();
    speakers.sort_by(|a, b| {
        b.speech_duration_us
            .cmp(&a.speech_duration_us)
            .then_with(|| a.id.cmp(b.id))
    });
    speakers
}

fn speakers(doc: &Document, json: bool) -> Result<()> {
    let speakers = speaker_views(doc);

    if json {
        emit(&SpeakersOutput {
            speaker_count: speakers.len(),
            speakers,
        })
    } else {
        if speakers.is_empty() {
            println!("(none)");
            return Ok(());
        }
        for speaker in &speakers {
            let turns = if speaker.turn_count == 1 {
                "turn"
            } else {
                "turns"
            };
            println!(
                "{:<6} {:>9.3} s  {:>2} {turns}  {}–{}",
                speaker.id,
                speaker.speech_duration_us as f64 / 1e6,
                speaker.turn_count,
                MediaTime::from_micros(speaker.first_start_us),
                MediaTime::from_micros(speaker.last_end_us)
            );
        }
        Ok(())
    }
}

// ------------------------------------------------------------------- chapters

/// Boundaries closer than this are the same moment (a speaker change inside a
/// pause, a scene cut at a chapter start): keep the earliest.
const CHAPTER_MERGE_GAP: MediaTime = MediaTime::from_millis(5_000);
/// Chapters shorter than this are noise; they merge into the previous one.
const MIN_CHAPTER: MediaTime = MediaTime::from_millis(2_000);

fn chapters(doc: &Document, min_gap: &str, json: bool) -> Result<()> {
    let chapters = chapter_views(doc, parse_time(min_gap)?);

    if json {
        emit(&ChaptersOutput {
            count: chapters.len(),
            chapters,
        })
    } else {
        if chapters.is_empty() {
            println!("(none)");
            return Ok(());
        }
        for chapter in &chapters {
            println!(
                "{}–{}  {:<12}  {}  [{} seg]",
                chapter.start_display,
                chapter.end_display,
                if chapter.speakers.is_empty() {
                    "-".to_string()
                } else {
                    chapter.speakers.join(", ")
                },
                chapter.title_hint.as_deref().unwrap_or(""),
                chapter.segment_count
            );
        }
        Ok(())
    }
}

/// Chapters from structure: long silence, speaker change, scene cut. Shared by
/// `chapters` and `brief`.
fn chapter_views(doc: &Document, min_gap: MediaTime) -> Vec<ChapterView<'_>> {
    let end = doc.duration();
    let end = if end == MediaTime::ZERO {
        MediaTime::MAX
    } else {
        end
    };

    // A chapter is a stretch between meaningful changes. Three cheap signals,
    // no language model involved: long silence, a change of speaker, a cut.
    let mut boundaries: Vec<MediaTime> = Vec::new();
    for block in &doc.analyses {
        if let AnalysisBlock::Acoustic(acoustic) = block {
            boundaries.extend(
                acoustic
                    .silence
                    .iter()
                    .filter(|silence| silence.duration() >= min_gap)
                    .map(|silence| silence.start),
            );
        }
    }
    let turns = doc.dialogue();
    for pair in turns.windows(2) {
        if pair[0].speaker != pair[1].speaker {
            boundaries.push(pair[1].range.start);
        }
    }
    for segment in doc.segments_of_kind("scene") {
        if matches!(segment.kind, SegmentKind::Scene { cut: true, .. }) {
            boundaries.push(segment.range.start);
        }
    }

    boundaries.retain(|instant| *instant > MediaTime::ZERO && *instant < end);
    boundaries.sort_unstable();
    boundaries.dedup_by(|later, earlier| (*later).saturating_sub(*earlier) < CHAPTER_MERGE_GAP);

    let mut ranges: Vec<TimeRange> = Vec::new();
    let mut start = MediaTime::ZERO;
    for boundary in boundaries {
        if boundary > start {
            ranges.push(TimeRange::new(start, boundary));
            start = boundary;
        }
    }
    ranges.push(TimeRange::new(start, end));

    let mut merged: Vec<TimeRange> = Vec::new();
    for range in ranges {
        match merged.last_mut() {
            Some(previous) if range.duration() < MIN_CHAPTER || range.is_empty() => {
                *previous = TimeRange::new(previous.start, range.end);
            }
            _ => merged.push(range),
        }
    }

    merged
        .iter()
        .map(|range| {
            let segments = doc.segments_in(*range);
            let speakers: BTreeSet<&str> = segments
                .iter()
                .filter_map(|segment| segment.kind.speaker())
                .map(SpeakerId::as_str)
                .collect();
            ChapterView {
                start: range.start.micros(),
                end: range.end.micros(),
                start_display: range.start.to_string(),
                end_display: range.end.to_string(),
                title_hint: title_hint(&segments),
                speakers: speakers.into_iter().collect(),
                segment_count: segments.len(),
            }
        })
        .collect()
}

/// First words of the first utterance in a range. A hint for the reader, never
/// presented as a generated title.
fn title_hint(segments: &[&Segment]) -> Option<String> {
    segments.iter().find_map(|segment| {
        let text = segment.kind.text()?.trim();
        if text.is_empty() {
            return None;
        }
        Some(
            text.split_whitespace()
                .take(8)
                .collect::<Vec<_>>()
                .join(" "),
        )
    })
}

// -------------------------------------------------------------------- frames

fn frames(
    doc: &Document,
    doc_path: &Path,
    from: Option<&str>,
    to: Option<&str>,
    max: Option<usize>,
    json: bool,
) -> Result<()> {
    let start = from.map(parse_time).transpose()?.unwrap_or(MediaTime::ZERO);
    let end = match to {
        Some(value) => parse_time(value)?,
        None => {
            let duration = doc.duration();
            if duration == MediaTime::ZERO {
                MediaTime::MAX
            } else {
                duration
            }
        }
    };
    if end < start {
        bail!("`--to` ({}) is before `--from` ({})", end, start);
    }

    let base = doc_path.parent().unwrap_or_else(|| Path::new(""));
    let mut items: Vec<FrameView> = doc
        .artifacts
        .iter()
        .filter(|artifact| artifact.kind == "keyframe")
        .filter(|artifact| {
            // A still image is a point range; half-open intersection is empty
            // for it, so test containment explicitly.
            if artifact.range.start == artifact.range.end {
                artifact.range.start >= start && artifact.range.start < end
            } else {
                artifact.range.intersects(&TimeRange::new(start, end))
            }
        })
        .map(|artifact| FrameView {
            start: artifact.range.start.micros(),
            start_display: artifact.range.start.to_string(),
            path: base.join(&artifact.path).display().to_string(),
            sha256: artifact.sha256.clone(),
            width: artifact.width,
            height: artifact.height,
        })
        .collect();
    items.sort_by(|a, b| (a.start, &a.path).cmp(&(b.start, &b.path)));
    if let Some(max) = max {
        items.truncate(max);
    }

    if json {
        emit(&FramesOutput {
            count: items.len(),
            frames: items,
        })
    } else {
        if items.is_empty() {
            println!("(none)");
            return Ok(());
        }
        for frame in &items {
            let hash = frame.sha256.as_deref().map_or_else(String::new, |hash| {
                format!("  {}", &hash[..12.min(hash.len())])
            });
            println!("{}  {}{hash}", frame.start_display, frame.path);
        }
        Ok(())
    }
}

// --------------------------------------------------------------------- music
fn music(doc: &Document, json: bool) -> Result<()> {
    let found = analysis_music(doc);

    match found {
        Some(summary) if json => emit(&MusicView::new(summary)),
        Some(summary) => {
            print_music(summary);
            Ok(())
        }
        None if json => emit(&MusicView::empty()),
        None => {
            println!("music     not analysed (no music block in this document)");
            Ok(())
        }
    }
}

fn print_music(summary: &MusicSummary) {
    match &summary.bpm {
        Some(bpm) => println!(
            "tempo     {:.1} BPM (score {:.3})",
            bpm.value, bpm.confidence
        ),
        None => println!("tempo     not estimated"),
    }
    match &summary.key {
        Some(key) => println!(
            "key       {} {} (score {:.3})",
            key.tonic.name(),
            if key.mode == sonolytica_core::Mode::Major {
                "major"
            } else {
                "minor"
            },
            key.confidence
        ),
        None => println!("key       not estimated"),
    }
    match &summary.loudness {
        Some(loudness) => {
            print!("loudness  {:.1} LUFS", loudness.integrated_lufs);
            if let Some(range) = loudness.loudness_range_lu {
                print!(" · LRA {range:.1} LU");
            }
            if let Some(peak) = loudness.true_peak_dbtp {
                print!(" · true peak {peak:.1} dBTP");
            }
            println!();
        }
        None => println!("loudness  not measured"),
    }
    println!(
        "chords    {} · beats {}",
        summary.chords.len(),
        summary.beats.len()
    );
    if summary.sections.is_empty() {
        println!("sections  not estimated");
    } else {
        println!("sections  {}", summary.sections.len());
        let changes: Vec<String> = summary
            .sections
            .iter()
            .skip(1)
            .map(|section| section.range.start.to_string())
            .collect();
        if !changes.is_empty() {
            println!("changes   {}", changes.join(", "));
        }
    }
}

// ------------------------------------------------------------------ acoustic

fn acoustic(doc: &Document, json: bool) -> Result<()> {
    let found = analysis_acoustic(doc);

    match found {
        Some(summary) if json => emit(&AcousticView::new(summary)),
        Some(summary) => {
            let total: MediaTime = summary
                .silence
                .iter()
                .fold(MediaTime::ZERO, |sum, range| sum + range.duration());
            println!(
                "silence   {} region(s), {:.3} s total",
                summary.silence.len(),
                total.as_secs_f64()
            );
            print!("levels    ");
            if let Some(mean) = summary.mean_rms_dbfs {
                print!("mean {mean:.1} dBFS · ");
            }
            if let Some(peak) = summary.peak_dbfs {
                print!("peak {peak:.1} dBFS · ");
            }
            match summary.dynamic_range_lu {
                Some(range) => println!("LRA {range:.1} LU"),
                None => println!("LRA not measured"),
            }
            Ok(())
        }
        None if json => emit(&AcousticView::empty()),
        None => {
            println!("acoustic  not analysed (no acoustic block in this document)");
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------- runs

fn runs(doc: &Document, json: bool) -> Result<()> {
    if json {
        return emit(&RunsOutput {
            complete: doc.is_complete(),
            runs: doc.runs.iter().map(RunView::new).collect(),
            warnings: &doc.warnings,
        });
    }

    if doc.runs.is_empty() {
        println!("(no runs recorded)");
    }
    for run in &doc.runs {
        let view = RunView::new(run);
        let mut line = format!(
            "{:<12} {:<9} {:>6} ms  {:<18} {}",
            run.capability.as_str(),
            view.status,
            view.duration_ms,
            view.provider,
            view.device
        );
        if let Some(reason) = view.reason {
            let _ = write!(line, "  {reason}");
        }
        println!("{line}");
    }
    for warning in &doc.warnings {
        let mut line = format!("warning      [{}] {}", warning.code, warning.message);
        if let Some(run) = warning.run {
            let _ = write!(line, " (run {run})");
        }
        println!("{line}");
    }
    Ok(())
}

// --------------------------------------------------------------------- brief

/// Chapters shown by `brief`; the document can hold many more.
const BRIEF_CHAPTERS: usize = 5;
/// Speakers shown by `brief` (Sortformer caps at 4, pyannote at 12).
const BRIEF_SPEAKERS: usize = 8;
/// Same 1.2 s that `query chapters` uses by default.
const BRIEF_CHAPTER_GAP: MediaTime = MediaTime::from_millis(1_200);
/// Scene cuts mapped by `brief`; a long video can hold hundreds.
const BRIEF_SCENE_CUTS: usize = 50;
/// One OCR line per keyframe, capped: the full list is `timeline --type text_overlay`.
const BRIEF_OCR_LINES: usize = 10;
/// Overlap spans mapped by `brief`; crowded audio can hold many.
const BRIEF_OVERLAPS: usize = 20;
/// Turns that touch within this gap belong to one overlap span.
const OVERLAP_MERGE_GAP: MediaTime = MediaTime::from_millis(50);

fn analysis_speech(doc: &Document) -> Option<&SpeechSummary> {
    doc.analyses.iter().find_map(|block| match block {
        AnalysisBlock::Speech(summary) => Some(summary),
        _ => None,
    })
}

fn analysis_music(doc: &Document) -> Option<&MusicSummary> {
    doc.analyses.iter().find_map(|block| match block {
        AnalysisBlock::Music(summary) => Some(summary),
        _ => None,
    })
}

fn analysis_acoustic(doc: &Document) -> Option<&AcousticSummary> {
    doc.analyses.iter().find_map(|block| match block {
        AnalysisBlock::Acoustic(summary) => Some(summary),
        _ => None,
    })
}

/// One OCR line per keyframe instant, in time order. Several overlays can share
/// one instant (several lines on a slide); `brief` shows the first. It is a map,
/// not a quotation source: the full text is `timeline --type text_overlay`.
fn brief_ocr(doc: &Document) -> Vec<BriefOcr> {
    let mut by_instant: BTreeMap<i64, String> = BTreeMap::new();
    for segment in doc.segments_of_kind("text_overlay") {
        let SegmentKind::TextOverlay { text, .. } = &segment.kind else {
            continue;
        };
        by_instant
            .entry(segment.range.start.micros())
            .or_insert_with(|| truncate(text));
    }
    by_instant
        .into_iter()
        .take(BRIEF_OCR_LINES)
        .map(|(start_us, text)| BriefOcr { start_us, text })
        .collect()
}

/// Overlap spans: speaker turns flagged `overlap: true`, merged when they touch.
/// `brief` is a map, so it lists the spans (longest first, capped) with the
/// speakers involved; the individual turns stay in `timeline --type speaker_turn`.
fn brief_overlaps(doc: &Document) -> Vec<BriefOverlap> {
    let mut turns: Vec<(MediaTime, MediaTime, &str)> = doc
        .segments_of_kind("speaker_turn")
        .filter_map(|segment| match &segment.kind {
            SegmentKind::SpeakerTurn {
                speaker,
                overlap: true,
                ..
            } => Some((segment.range.start, segment.range.end, speaker.as_str())),
            _ => None,
        })
        .collect();
    turns.sort_unstable_by_key(|(start, end, _)| (*start, *end));

    let mut spans: Vec<(MediaTime, MediaTime, Vec<String>)> = Vec::new();
    for (start, end, speaker) in turns {
        match spans.last_mut() {
            Some((_, span_end, speakers))
                if start <= span_end.saturating_add(OVERLAP_MERGE_GAP) =>
            {
                *span_end = (*span_end).max(end);
                if !speakers.iter().any(|known| known == speaker) {
                    speakers.push(speaker.to_string());
                }
            }
            _ => spans.push((start, end, vec![speaker.to_string()])),
        }
    }

    if spans.len() > BRIEF_OVERLAPS {
        spans.sort_unstable_by_key(|(start, end, _)| end.saturating_sub(*start));
        spans.truncate(BRIEF_OVERLAPS);
    }
    spans.sort_unstable_by_key(|(start, _, _)| *start);
    spans
        .into_iter()
        .map(|(start, end, speakers)| BriefOverlap {
            start_us: start.micros(),
            end_us: end.micros(),
            speakers,
        })
        .collect()
}

fn brief(doc: &Document, json: bool) -> Result<()> {
    let summary = analysis_speech(doc);
    let speech_segments: Vec<&Segment> = doc.segments_of_kind("speech").collect();
    let turns = doc.dialogue();
    let has_speech = summary.is_some() || !speech_segments.is_empty() || !turns.is_empty();

    let word_count = summary.map_or_else(
        || {
            speech_segments
                .iter()
                .map(|segment| match &segment.kind {
                    SegmentKind::Speech { words, .. } => words.len() as u64,
                    _ => 0,
                })
                .sum()
        },
        |speech| speech.word_count,
    );
    let language = summary
        .and_then(|speech| speech.language.clone())
        .or_else(|| {
            speech_segments
                .iter()
                .find_map(|segment| match &segment.kind {
                    SegmentKind::Speech { language, .. } => language.clone(),
                    _ => None,
                })
        });
    let speaker_count = summary.map_or_else(
        || u32::try_from(doc.speakers().len()).unwrap_or(0),
        |speech| speech.speaker_count,
    );
    let speech = has_speech.then(|| BriefSpeech {
        word_count,
        language,
        speaker_count,
        speech_ratio: summary.map(|speech| speech.speech_ratio),
        speech_duration_us: summary.map(|speech| speech.speech_duration.micros()),
        overlaps: brief_overlaps(doc),
    });

    let all_chapters = chapter_views(doc, BRIEF_CHAPTER_GAP);
    let chapter_count = all_chapters.len();
    let all_speakers = speaker_views(doc);
    let speaker_total = all_speakers.len();
    let scene_segments: Vec<&Segment> = doc.segments_of_kind("scene").collect();
    let scene_cuts: Vec<i64> = scene_segments
        .iter()
        .filter(|segment| matches!(segment.kind, SegmentKind::Scene { cut: true, .. }))
        .map(|segment| segment.range.start.micros())
        .take(BRIEF_SCENE_CUTS)
        .collect();
    let video = BriefVideo {
        scenes: scene_segments.len(),
        keyframes: doc
            .artifacts
            .iter()
            .filter(|artifact| artifact.kind == "keyframe")
            .count(),
        text_overlays: doc.segments_of_kind("text_overlay").count(),
        scene_cuts,
        ocr: brief_ocr(doc),
    };
    let music = analysis_music(doc).map(|music| BriefMusic {
        bpm: music.bpm.as_ref().map(|bpm| bpm.value),
        key: music.key.as_ref().map(|key| {
            format!(
                "{} {}",
                key.tonic.name(),
                if key.mode == sonolytica_core::Mode::Major {
                    "major"
                } else {
                    "minor"
                }
            )
        }),
        section_count: music.sections.len(),
    });
    let acoustic = analysis_acoustic(doc).map(|acoustic| BriefAcoustic {
        peak_dbfs: acoustic.peak_dbfs,
        lra_lu: acoustic.dynamic_range_lu,
        silence_count: acoustic.silence.len(),
    });
    let runs: Vec<BriefRun<'_>> = doc
        .runs
        .iter()
        .map(|run| BriefRun {
            capability: run.capability,
            provider: &run.provider,
            status: status_of(run).0,
            duration_ms: run.duration_ms,
        })
        .collect();

    if json {
        return emit(&BriefOutput {
            media: BriefMedia {
                path: &doc.media.path,
                duration_us: doc.duration().micros(),
                duration_display: doc.duration().to_string(),
                sha256: doc.media.sha256.as_deref(),
                has_video: doc.media.has_video(),
            },
            config: BriefConfig {
                modules: &doc.config.modules,
                language: doc.config.language.as_deref(),
                asr_model: doc.config.asr_model.as_deref(),
                align_words: doc.config.align_words,
            },
            complete: doc.is_complete(),
            runs,
            warnings: &doc.warnings,
            speech,
            speaker_count: speaker_total,
            speakers: all_speakers.into_iter().take(BRIEF_SPEAKERS).collect(),
            chapter_count,
            chapters: all_chapters.into_iter().take(BRIEF_CHAPTERS).collect(),
            video,
            music,
            acoustic,
        });
    }

    println!(
        "file       {}  {}  {}",
        doc.media.path,
        doc.duration(),
        if doc.media.has_video() {
            "video"
        } else {
            "audio"
        }
    );
    println!("sha256     {}", doc.media.sha256.as_deref().unwrap_or("-"));
    let modules = doc
        .config
        .modules
        .iter()
        .map(|capability| capability.as_str())
        .collect::<Vec<_>>()
        .join(",");
    print!("modules    {modules}");
    if let Some(language) = &doc.config.language {
        print!(" · language {language}");
    }
    if let Some(model) = &doc.config.asr_model {
        print!(" · asr {model}");
    }
    if doc.config.align_words {
        print!(" · align");
    }
    println!();
    println!(
        "status     {}",
        if doc.is_complete() {
            "complete"
        } else {
            "not complete (see warnings/runs)"
        }
    );
    if runs.is_empty() {
        println!("runs       (none)");
    } else {
        let line = doc
            .runs
            .iter()
            .map(|run| {
                let (status, reason) = status_of(run);
                let mut text = format!(
                    "{} {} {status} {} ms",
                    run.capability, run.provider, run.duration_ms
                );
                if let Some(reason) = reason {
                    let _ = write!(text, " ({reason})");
                }
                text
            })
            .collect::<Vec<_>>()
            .join(" · ");
        println!("runs       {line}");
    }

    match &speech {
        Some(speech) => {
            let ratio = speech.speech_ratio.map_or_else(String::new, |ratio| {
                format!(" · {:.0}% of media", ratio * 100.0)
            });
            println!(
                "speech     {} words · {} · {} · {}{ratio}",
                speech.word_count,
                count_label(speech.speaker_count as usize, "speaker", "speakers"),
                speech.language.as_deref().unwrap_or("language ?"),
                speech.speech_duration_us.map_or_else(
                    || "duration ?".to_string(),
                    |micros| MediaTime::from_micros(micros).to_string()
                )
            );
        }
        None => println!("speech     (none)"),
    }

    if all_speakers.is_empty() {
        println!("speakers   (none)");
    } else {
        for (index, speaker) in all_speakers.iter().take(BRIEF_SPEAKERS).enumerate() {
            let label = if index == 0 {
                "speakers  "
            } else {
                "          "
            };
            println!(
                "{label} {:<4} {:>9.3} s  {:>2} {}  {}–{}",
                speaker.id,
                speaker.speech_duration_us as f64 / 1e6,
                speaker.turn_count,
                if speaker.turn_count == 1 {
                    "turn "
                } else {
                    "turns"
                },
                MediaTime::from_micros(speaker.first_start_us),
                MediaTime::from_micros(speaker.last_end_us)
            );
        }
        if speaker_total > BRIEF_SPEAKERS {
            println!("           … {} more", speaker_total - BRIEF_SPEAKERS);
        }
    }

    if let Some(speech) = &speech {
        if !speech.overlaps.is_empty() {
            println!("overlaps   {} total", speech.overlaps.len());
            for span in speech.overlaps.iter().take(5) {
                println!(
                    "           {}–{}  {}",
                    MediaTime::from_micros(span.start_us),
                    MediaTime::from_micros(span.end_us),
                    span.speakers.join(",")
                );
            }
            if speech.overlaps.len() > 5 {
                println!("           … {} more", speech.overlaps.len() - 5);
            }
        }
    }

    if all_chapters.is_empty() {
        println!("chapters   (none)");
    } else {
        println!("chapters   {} total", chapter_count);
        for chapter in all_chapters.iter().take(BRIEF_CHAPTERS) {
            println!(
                "           {}–{}  {:<8}  {}",
                chapter.start_display,
                chapter.end_display,
                if chapter.speakers.is_empty() {
                    "-".to_string()
                } else {
                    chapter.speakers.join(",")
                },
                chapter.title_hint.as_deref().unwrap_or("")
            );
        }
        if chapter_count > BRIEF_CHAPTERS {
            println!("           … {} more", chapter_count - BRIEF_CHAPTERS);
        }
    }

    if doc.media.has_video() || video.scenes > 0 {
        println!(
            "video      {} · {} · {}",
            count_label(video.scenes, "scene", "scenes"),
            count_label(video.keyframes, "keyframe", "keyframes"),
            count_label(video.text_overlays, "text overlay", "text overlays")
        );
        if !video.scene_cuts.is_empty() {
            let cuts = video
                .scene_cuts
                .iter()
                .map(|micros| MediaTime::from_micros(*micros).to_string())
                .collect::<Vec<_>>()
                .join(" ");
            println!("cuts       {cuts}");
        }
        for line in &video.ocr {
            println!(
                "ocr        {}  {}",
                MediaTime::from_micros(line.start_us),
                line.text
            );
        }
    }
    if let Some(music) = &music {
        println!(
            "music      {} · {} · {}",
            music
                .bpm
                .map_or_else(|| "tempo ?".to_string(), |bpm| format!("{bpm:.1} BPM")),
            music.key.as_deref().unwrap_or("key ?"),
            count_label(music.section_count, "section", "sections")
        );
    }
    if let Some(acoustic) = &acoustic {
        println!(
            "acoustic   peak {} · LRA {} · {}",
            acoustic
                .peak_dbfs
                .map_or_else(|| "?".to_string(), |peak| format!("{peak:.1} dBFS")),
            acoustic
                .lra_lu
                .map_or_else(|| "?".to_string(), |lra| format!("{lra:.1} LU")),
            count_label(acoustic.silence_count, "silence region", "silence regions")
        );
    }
    if doc.warnings.is_empty() {
        println!("warnings   none");
    } else {
        println!("warnings   {} total", doc.warnings.len());
        for warning in &doc.warnings {
            println!("           [{}] {}", warning.code, warning.message);
        }
    }
    Ok(())
}

// ------------------------------------------------------------------- summary

fn summary(doc: &Document, json: bool) -> Result<()> {
    if json {
        return emit(&SummaryOutput {
            media: &doc.media,
            config: &doc.config,
            counts: Counts {
                tracks: doc.tracks.len(),
                segments: doc.segments.len(),
                relations: doc.relations.len(),
            },
            analyses: &doc.analyses,
            runs: doc.runs.iter().map(RunView::new).collect(),
            warnings: &doc.warnings,
        });
    }
    print!("{}", crate::summarize(doc));
    Ok(())
}

fn emit<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

// ------------------------------------------------------------- text rendering

fn print_segments(segments: &[&Segment]) {
    if segments.is_empty() {
        println!("(none)");
        return;
    }
    for segment in segments {
        println!("{}", view_line(&SegmentView::new(segment)));
    }
}

fn view_line(view: &SegmentView<'_>) -> String {
    let mut line = format!(
        "{}–{}  {:<14}",
        view.start_display, view.end_display, view.kind
    );
    match view.speaker {
        Some(speaker) => {
            let _ = write!(line, " {speaker:<4}");
        }
        None => line.push_str("     "),
    }
    let body = match (view.text, &view.label) {
        (Some(text), _) => truncate(text),
        (None, Some(label)) => truncate(label),
        (None, None) => String::new(),
    };
    line.push_str(&body);
    if let Some(confidence) = view.confidence {
        let _ = write!(line, "  [{confidence:.3}]");
    }
    line
}

fn truncate(text: &str) -> String {
    let mut out: String = text.chars().take(TEXT_WIDTH + 1).collect();
    if let Some((cut, _)) = out.char_indices().nth(TEXT_WIDTH) {
        out.truncate(cut);
        out.push('…');
    }
    out
}

fn count_label(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

// ------------------------------------------------------------------------ DTOs

#[derive(Debug, Serialize)]
struct SegmentsOutput<'a> {
    count: usize,
    segments: Vec<SegmentView<'a>>,
}

#[derive(Debug, Serialize)]
struct SegmentView<'a> {
    id: u32,
    track: u32,
    kind: &'static str,
    /// Microseconds, the document's unit.
    start: i64,
    end: i64,
    /// The same instant as `HH:MM:SS.mmm`, so a citation needs no arithmetic.
    start_display: String,
    end_display: String,
    speaker: Option<&'a str>,
    text: Option<&'a str>,
    /// Label/class for non-text segments (music, acoustic, scene, object).
    label: Option<String>,
    confidence: Option<f32>,
}

impl<'a> SegmentView<'a> {
    fn new(segment: &'a Segment) -> Self {
        Self::with_speaker(segment, segment.kind.speaker().map(SpeakerId::as_str))
    }

    fn with_speaker(segment: &'a Segment, speaker: Option<&'a str>) -> Self {
        Self {
            id: segment.id,
            track: segment.track,
            kind: segment.kind.kind_name(),
            start: segment.range.start.micros(),
            end: segment.range.end.micros(),
            start_display: segment.range.start.to_string(),
            end_display: segment.range.end.to_string(),
            speaker,
            text: segment.kind.text(),
            label: label_of(&segment.kind),
            confidence: segment.confidence.map(|c| c.get()),
        }
    }
}

fn label_of(kind: &SegmentKind) -> Option<String> {
    match kind {
        SegmentKind::MusicLabel { label } => Some(label.clone()),
        SegmentKind::AcousticEvent { class } => Some(class.clone()),
        SegmentKind::Scene { index, cut } => {
            Some(format!("scene {index}{}", if *cut { " (cut)" } else { "" }))
        }
        SegmentKind::Object { class, .. } => Some(class.clone()),
        SegmentKind::TextOverlay { bbox, .. } => Some(format!(
            "{:.0}x{:.0} at {:.0},{:.0}",
            bbox.width, bbox.height, bbox.x, bbox.y
        )),
        SegmentKind::Speech { .. } | SegmentKind::SpeakerTurn { .. } => None,
    }
}

#[derive(Debug, Serialize)]
struct SearchOutput<'a> {
    needle: &'a str,
    count: usize,
    matches: Vec<MatchView<'a>>,
}

#[derive(Debug, Serialize)]
struct MatchView<'a> {
    #[serde(flatten)]
    segment: SegmentView<'a>,
    /// Only the words that matched, with their own ranges.
    words: Vec<WordView<'a>>,
}

#[derive(Debug, Serialize)]
struct WordView<'a> {
    text: &'a str,
    start: i64,
    end: i64,
    range_display: String,
    confidence: Option<f32>,
}

impl<'a> WordView<'a> {
    fn new(word: &'a Word) -> Self {
        Self {
            text: &word.text,
            start: word.range.start.micros(),
            end: word.range.end.micros(),
            range_display: format!("{}–{}", word.range.start, word.range.end),
            confidence: word.confidence.map(|c| c.get()),
        }
    }
}

#[derive(Debug, Serialize)]
struct TranscriptOutput<'a> {
    count: usize,
    segments: Vec<SegmentView<'a>>,
}

#[derive(Debug, Serialize)]
struct WordsOutput<'a> {
    segment: u32,
    speaker: Option<&'a str>,
    text: &'a str,
    start: i64,
    end: i64,
    start_display: String,
    end_display: String,
    words: Vec<WordView<'a>>,
}

#[derive(Debug, Serialize)]
struct SpeakersOutput<'a> {
    speaker_count: usize,
    speakers: Vec<SpeakerView<'a>>,
}

#[derive(Debug, Serialize)]
struct SpeakerView<'a> {
    id: &'a str,
    speech_duration_us: i64,
    speech_display: String,
    turn_count: u32,
    first_start_us: i64,
    last_end_us: i64,
}

#[derive(Debug, Serialize)]
struct MusicView<'a> {
    available: bool,
    bpm: Option<&'a Bpm>,
    key: Option<&'a KeyEstimate>,
    chords: &'a [Chord],
    sections: &'a [Section],
    loudness: Option<&'a Loudness>,
    beat_count: usize,
}

impl<'a> MusicView<'a> {
    fn new(summary: &'a MusicSummary) -> Self {
        Self {
            available: true,
            bpm: summary.bpm.as_ref(),
            key: summary.key.as_ref(),
            chords: &summary.chords,
            sections: &summary.sections,
            loudness: summary.loudness.as_ref(),
            beat_count: summary.beats.len(),
        }
    }

    fn empty() -> Self {
        Self {
            available: false,
            bpm: None,
            key: None,
            chords: &[],
            sections: &[],
            loudness: None,
            beat_count: 0,
        }
    }
}

#[derive(Debug, Serialize)]
struct AcousticView<'a> {
    available: bool,
    silence: &'a [TimeRange],
    dynamic_range_lu: Option<f32>,
    mean_rms_dbfs: Option<f32>,
    peak_dbfs: Option<f32>,
    event_count: usize,
}

impl<'a> AcousticView<'a> {
    fn new(summary: &'a AcousticSummary) -> Self {
        Self {
            available: true,
            silence: &summary.silence,
            dynamic_range_lu: summary.dynamic_range_lu,
            mean_rms_dbfs: summary.mean_rms_dbfs,
            peak_dbfs: summary.peak_dbfs,
            event_count: summary.events.len(),
        }
    }

    fn empty() -> Self {
        Self {
            available: false,
            silence: &[],
            dynamic_range_lu: None,
            mean_rms_dbfs: None,
            peak_dbfs: None,
            event_count: 0,
        }
    }
}

#[derive(Debug, Serialize)]
struct ChaptersOutput<'a> {
    count: usize,
    chapters: Vec<ChapterView<'a>>,
}

#[derive(Debug, Serialize)]
struct ChapterView<'a> {
    start: i64,
    end: i64,
    start_display: String,
    end_display: String,
    /// First words of the first utterance. A hint, not a generated title.
    title_hint: Option<String>,
    speakers: Vec<&'a str>,
    segment_count: usize,
}

#[derive(Debug, Serialize)]
struct FramesOutput {
    count: usize,
    frames: Vec<FrameView>,
}

#[derive(Debug, Serialize)]
struct FrameView {
    start: i64,
    start_display: String,
    /// Absolute path on this machine: the artifact path resolved relative to
    /// the document.
    path: String,
    sha256: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Debug, Serialize)]
struct RunsOutput<'a> {
    complete: bool,
    runs: Vec<RunView<'a>>,
    warnings: &'a [Warning],
}

#[derive(Debug, Serialize)]
struct RunView<'a> {
    id: u32,
    capability: Capability,
    provider: &'a str,
    model_id: Option<&'a str>,
    device: &'a str,
    duration_ms: u64,
    status: &'static str,
    reason: Option<&'a str>,
}

fn status_of(run: &RunRecord) -> (&'static str, Option<&str>) {
    match &run.status {
        RunStatus::Complete => ("complete", None),
        RunStatus::Partial { reason } => ("partial", Some(reason.as_str())),
        RunStatus::Failed { reason } => ("failed", Some(reason.as_str())),
    }
}

impl<'a> RunView<'a> {
    fn new(run: &'a RunRecord) -> Self {
        let (status, reason) = status_of(run);
        Self {
            id: run.id,
            capability: run.capability,
            provider: &run.provider,
            model_id: run.model.as_ref().map(|model| model.id.as_str()),
            device: &run.device,
            duration_ms: run.duration_ms,
            status,
            reason,
        }
    }
}

#[derive(Debug, Serialize)]
struct BriefOutput<'a> {
    media: BriefMedia<'a>,
    config: BriefConfig<'a>,
    complete: bool,
    runs: Vec<BriefRun<'a>>,
    warnings: &'a [Warning],
    speech: Option<BriefSpeech>,
    speaker_count: usize,
    speakers: Vec<SpeakerView<'a>>,
    chapter_count: usize,
    chapters: Vec<ChapterView<'a>>,
    video: BriefVideo,
    music: Option<BriefMusic>,
    acoustic: Option<BriefAcoustic>,
}

#[derive(Debug, Serialize)]
struct BriefMedia<'a> {
    path: &'a str,
    duration_us: i64,
    duration_display: String,
    sha256: Option<&'a str>,
    has_video: bool,
}

#[derive(Debug, Serialize)]
struct BriefConfig<'a> {
    modules: &'a [Capability],
    language: Option<&'a str>,
    asr_model: Option<&'a str>,
    align_words: bool,
}

#[derive(Debug, Serialize)]
struct BriefRun<'a> {
    capability: Capability,
    provider: &'a str,
    status: &'static str,
    duration_ms: u64,
}

#[derive(Debug, Serialize)]
struct BriefSpeech {
    word_count: u64,
    language: Option<String>,
    speaker_count: u32,
    speech_ratio: Option<f32>,
    speech_duration_us: Option<i64>,
    /// Spans where two or more turns talk at once, in time order (capped).
    overlaps: Vec<BriefOverlap>,
}

#[derive(Debug, Serialize)]
struct BriefOverlap {
    start_us: i64,
    end_us: i64,
    speakers: Vec<String>,
}

#[derive(Debug, Serialize)]
struct BriefVideo {
    scenes: usize,
    keyframes: usize,
    text_overlays: usize,
    /// Start of each detected cut, microseconds; empty when nothing was cut.
    scene_cuts: Vec<i64>,
    /// First OCR line per keyframe instant, at most `BRIEF_OCR_LINES`.
    ocr: Vec<BriefOcr>,
}

#[derive(Debug, Serialize)]
struct BriefOcr {
    start_us: i64,
    text: String,
}

#[derive(Debug, Serialize)]
struct BriefMusic {
    bpm: Option<f32>,
    key: Option<String>,
    section_count: usize,
}

#[derive(Debug, Serialize)]
struct BriefAcoustic {
    peak_dbfs: Option<f32>,
    lra_lu: Option<f32>,
    silence_count: usize,
}

#[derive(Debug, Serialize)]
struct SummaryOutput<'a> {
    media: &'a MediaInfo,
    config: &'a AnalysisConfig,
    counts: Counts,
    analyses: &'a [AnalysisBlock],
    runs: Vec<RunView<'a>>,
    warnings: &'a [Warning],
}

#[derive(Debug, Serialize)]
struct Counts {
    tracks: usize,
    segments: usize,
    relations: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_documented_form() {
        let cases = [
            ("00:00:12.450", 12_450_000),
            ("01:02:03", 3_723_000_000),
            ("02:03.500", 123_500_000),
            ("12.5s", 12_500_000),
            ("950ms", 950_000),
            ("1500us", 1_500),
            ("12.5", 12_500_000),
            ("12450000", 12_450_000),
            ("0", 0),
        ];
        for (input, expected) in cases {
            assert_eq!(
                parse_time(input).unwrap().micros(),
                expected,
                "parsing `{input}`"
            );
        }
    }

    #[test]
    fn rejects_garbage() {
        for input in ["", " ", "abc", "-1s", "1:2:3:4", "1e3", "12..5", "1:2:x"] {
            assert!(parse_time(input).is_err(), "`{input}` must not parse");
        }
    }

    #[test]
    fn truncation_is_character_safe() {
        let cyrillic = "я".repeat(TEXT_WIDTH + 5);
        let out = truncate(&cyrillic);
        assert_eq!(out.chars().count(), TEXT_WIDTH + 1);
        assert!(out.ends_with('…'));
        assert_eq!(truncate("short"), "short");
    }
}
