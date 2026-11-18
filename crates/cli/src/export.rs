//! Export a document to human-editing formats: SRT, WebVTT and Markdown.
//!
//! These formats are views over the document, not new analysis. Every cue
//! keeps the document range; no text is rewritten or invented.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};
use sonolytica_core::{Document, SegmentKind};

use crate::query::load;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// SubRip subtitles (`HH:MM:SS,mmm`)
    Srt,
    /// WebVTT subtitles (`HH:MM:SS.mmm`)
    Vtt,
    /// Human-readable Markdown transcript
    Markdown,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::Vtt => "vtt",
            Self::Markdown => "md",
        }
    }
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Analysed document (.json)
    pub document: PathBuf,
    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Srt)]
    pub format: Format,
    /// Output path (default: document path with the format extension)
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    /// Prefix each cue with the speaker whose turn covers its start
    #[arg(long)]
    pub speakers: bool,
    /// Include on-screen text in Markdown exports
    #[arg(long)]
    pub ocr: bool,
}

#[derive(Debug, Clone)]
struct Cue {
    start: i64,
    end: i64,
    text: String,
    speaker: Option<String>,
}

pub fn run(args: &ExportArgs) -> Result<PathBuf> {
    let document = load(&args.document)?;
    let out = args
        .out
        .clone()
        .unwrap_or_else(|| args.document.with_extension(args.format.extension()));
    let content = match args.format {
        Format::Srt => render_srt(&document, args.speakers),
        Format::Vtt => render_vtt(&document, args.speakers),
        Format::Markdown => render_markdown(&document, args.speakers, args.ocr),
    };
    std::fs::write(&out, content).with_context(|| format!("could not write {}", out.display()))?;
    Ok(out)
}

fn cues(document: &Document, speakers: bool) -> Vec<Cue> {
    let turns: Vec<(i64, i64, String)> = if speakers {
        document
            .segments_of_kind("speaker_turn")
            .filter_map(|segment| match &segment.kind {
                SegmentKind::SpeakerTurn { speaker, .. } => Some((
                    segment.range.start.micros(),
                    segment.range.end.micros(),
                    speaker.as_str().to_string(),
                )),
                _ => None,
            })
            .collect()
    } else {
        Vec::new()
    };

    let mut cues: Vec<Cue> = document
        .segments_of_kind("speech")
        .filter_map(|segment| match &segment.kind {
            SegmentKind::Speech { text, .. } => {
                let text = text.trim();
                if text.is_empty() {
                    return None;
                }
                Some(Cue {
                    start: segment.range.start.micros(),
                    end: segment.range.end.micros(),
                    text: text.to_string(),
                    speaker: if speakers {
                        turns
                            .iter()
                            .filter_map(|(turn_start, turn_end, speaker)| {
                                let overlap = segment.range.end.micros().min(*turn_end)
                                    - segment.range.start.micros().max(*turn_start);
                                (overlap > 0).then_some((overlap, speaker))
                            })
                            .max_by_key(|(overlap, _)| *overlap)
                            .map(|(_, speaker)| speaker.clone())
                    } else {
                        None
                    },
                })
            }
            _ => None,
        })
        .collect();

    if cues.is_empty() {
        cues = document
            .dialogue()
            .into_iter()
            .filter_map(|turn| {
                let text = turn.text.trim();
                if text.is_empty() {
                    return None;
                }
                Some(Cue {
                    start: turn.range.start.micros(),
                    end: turn.range.end.micros(),
                    text: text.to_string(),
                    speaker: speakers.then(|| turn.speaker.as_str().to_string()),
                })
            })
            .collect();
    }
    cues.sort_by_key(|cue| (cue.start, cue.end));
    cues
}

fn render_srt(document: &Document, speakers: bool) -> String {
    let mut out = String::new();
    for (index, cue) in cues(document, speakers).iter().enumerate() {
        let _ = writeln!(out, "{}", index + 1);
        let _ = writeln!(out, "{} --> {}", srt_time(cue.start), srt_time(cue.end));
        let _ = writeln!(out, "{}", cue_text(cue));
        out.push('\n');
    }
    out
}

fn render_vtt(document: &Document, speakers: bool) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for cue in cues(document, speakers) {
        let _ = writeln!(out, "{} --> {}", vtt_time(cue.start), vtt_time(cue.end));
        let _ = writeln!(out, "{}", cue_text(&cue));
        out.push('\n');
    }
    out
}

fn render_markdown(document: &Document, speakers: bool, ocr: bool) -> String {
    let title = Path::new(&document.media.path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "media".into());
    let mut out = format!(
        "# {title}\n\n- duration: {}\n- media: `{}`\n",
        document.duration(),
        document.media.path
    );
    if let Some(language) = document.analyses.iter().find_map(|block| match block {
        sonolytica_core::AnalysisBlock::Speech(speech) => speech.language.as_deref(),
        _ => None,
    }) {
        let _ = writeln!(out, "- language: {language}");
    }
    out.push_str("\n## Transcript\n\n");
    let cues = cues(document, speakers);
    if cues.is_empty() {
        out.push_str("_No speech segments in this document._\n");
    }
    for cue in &cues {
        let head = match (&cue.speaker, speakers) {
            (Some(speaker), true) => format!(
                "**{speaker}** `{}–{}`",
                display(cue.start),
                display(cue.end)
            ),
            _ => format!("`{}–{}`", display(cue.start), display(cue.end)),
        };
        let _ = writeln!(out, "- {head}  \n  {}\n", cue.text);
    }
    if ocr {
        let overlays: Vec<&sonolytica_core::Segment> =
            document.segments_of_kind("text_overlay").collect();
        if !overlays.is_empty() {
            out.push_str("\n## On-screen text\n\n");
            for segment in overlays {
                if let SegmentKind::TextOverlay { text, .. } = &segment.kind {
                    let _ = writeln!(
                        out,
                        "- `{}`  {}",
                        display(segment.range.start.micros()),
                        text
                    );
                }
            }
        }
    }
    if !document.warnings.is_empty() {
        out.push_str("\n## Warnings\n\n");
        for warning in &document.warnings {
            let _ = writeln!(out, "- `{}` {}", warning.code, warning.message);
        }
    }
    out
}

fn cue_text(cue: &Cue) -> String {
    match &cue.speaker {
        Some(speaker) => format!("[{speaker}] {}", cue.text.replace('\n', " ")),
        None => cue.text.replace('\n', " "),
    }
}

fn display(micros: i64) -> String {
    let micros = micros.max(0);
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        micros / 3_600_000_000,
        (micros / 60_000_000) % 60,
        (micros / 1_000_000) % 60,
        (micros / 1_000) % 1_000
    )
}

fn srt_time(micros: i64) -> String {
    display(micros).replace('.', ",")
}

fn vtt_time(micros: i64) -> String {
    display(micros)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_millisecond_exact() {
        assert_eq!(srt_time(3_723_456_000), "01:02:03,456");
        assert_eq!(vtt_time(950_000), "00:00:00.950");
        assert_eq!(display(0), "00:00:00.000");
    }

    #[test]
    fn cue_text_keeps_speaker_and_flattens_newlines() {
        let cue = Cue {
            start: 0,
            end: 1_000_000,
            text: "one\ntwo".into(),
            speaker: Some("S2".into()),
        };
        assert_eq!(cue_text(&cue), "[S2] one two");
    }
}
