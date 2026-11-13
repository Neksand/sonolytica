//! SONOLYTICA evaluation harness.

#![allow(clippy::needless_range_loop)]

mod metrics;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use sonolytica_core::{Document, SegmentKind};

use crate::metrics::{diarization_error_rate, parse_rttm, word_error_rate};

#[derive(Debug, Parser)]
#[command(
    name = "sonolytica-eval",
    version,
    about = "Evaluation harness for SONOLYTICA"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Word/character error rate against a reference transcript
    Wer {
        /// Path to a UTF-8 reference transcript; use - for stdin
        #[arg(long, value_name = "PATH", conflicts_with = "reference_text")]
        reference: Option<PathBuf>,
        /// Raw reference transcript, instead of --reference
        #[arg(long, value_name = "TEXT", conflicts_with = "reference")]
        reference_text: Option<String>,
        /// Path to plain text or a SONOLYTICA document (JSON); use - for stdin
        #[arg(long, value_name = "PATH", conflicts_with = "hypothesis_text")]
        hypothesis: Option<PathBuf>,
        /// Raw hypothesis transcript, instead of --hypothesis
        #[arg(long, value_name = "TEXT", conflicts_with = "hypothesis")]
        hypothesis_text: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Diarization error rate against an RTTM reference
    Der {
        #[arg(long)]
        rttm: PathBuf,
        #[arg(long)]
        document: PathBuf,
        /// Only score this file id (RTTM files often hold several recordings)
        #[arg(long)]
        file_id: Option<String>,
        /// Forgiveness window around reference boundaries, seconds
        #[arg(long, default_value_t = 0.25)]
        collar: f64,
        #[arg(long, default_value_t = 0.01)]
        frame: f64,
        #[arg(long)]
        json: bool,
    },
    /// Per-run timings and status from a document
    Report {
        /// Document path
        #[arg(value_name = "DOCUMENT", required_unless_present = "document_flag")]
        document: Option<PathBuf>,
        /// Deprecated alias for the positional DOCUMENT
        #[arg(
            long = "document",
            value_name = "DOCUMENT",
            hide = true,
            conflicts_with = "document"
        )]
        document_flag: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn read_text(path: &Path) -> Result<String> {
    if path == Path::new("-") {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .context("could not read stdin")?;
        return Ok(text);
    }
    std::fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))
}

fn source_text(
    path: Option<&Path>,
    raw: Option<String>,
    path_flag: &str,
    text_flag: &str,
) -> Result<String> {
    match (path, raw) {
        (Some(path), None) => read_text(path),
        (None, Some(text)) => Ok(text),
        (Some(_), Some(_)) => bail!("use either {path_flag} or {text_flag}, not both"),
        (None, None) => bail!("provide {path_flag} or {text_flag}"),
    }
}

fn load_document(path: &PathBuf) -> Result<Document> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;
    Document::from_json(&text)
        .with_context(|| format!("{} is not a SONOLYTICA document", path.display()))
}

/// Hypothesis text from a document: speech segments in time order, joined.
fn transcript_of(document: &Document) -> String {
    let mut segments: Vec<(sonolytica_core::MediaTime, &str)> = document
        .segments
        .iter()
        .filter_map(|segment| match &segment.kind {
            SegmentKind::Speech { text, .. } => Some((segment.range.start, text.as_str())),
            _ => None,
        })
        .collect();
    segments.sort_by_key(|(start, _)| *start);
    segments
        .into_iter()
        .map(|(_, text)| text)
        .collect::<Vec<_>>()
        .join(" ")
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Wer {
            reference,
            reference_text,
            hypothesis,
            hypothesis_text,
            json,
        } => {
            let reference_text = source_text(
                reference.as_deref(),
                reference_text,
                "--reference <PATH>",
                "--reference-text <TEXT>",
            )?;
            let hypothesis_text = match (hypothesis, hypothesis_text) {
                (Some(path), None) if path.extension().is_some_and(|e| e == "json") => {
                    transcript_of(&load_document(&path)?)
                }
                (Some(path), None) => read_text(&path)?,
                (None, Some(text)) => text,
                (Some(_), Some(_)) => {
                    bail!("use either --hypothesis or --hypothesis-text, not both")
                }
                (None, None) => {
                    bail!("provide --hypothesis <PATH> or --hypothesis-text <TEXT>")
                }
            };
            let score = word_error_rate(&reference_text, &hypothesis_text);
            if json {
                println!("{}", serde_json::to_string_pretty(&score)?);
            } else {
                println!(
                    "WER {:.2}%  (S {} / D {} / I {} over {} words)",
                    score.wer * 100.0,
                    score.substitutions,
                    score.deletions,
                    score.insertions,
                    score.reference_words
                );
                println!(
                    "CER {:.2}%  ({} reference characters)",
                    score.cer * 100.0,
                    score.reference_chars
                );
            }
        }
        Command::Der {
            rttm,
            document,
            file_id,
            collar,
            frame,
            json,
        } => {
            let reference_text = std::fs::read_to_string(&rttm)
                .with_context(|| format!("could not read {}", rttm.display()))?;
            let mut reference = parse_rttm(&reference_text);
            if let Some(id) = &file_id {
                reference.retain(|s| &s.file == id);
            } else if reference
                .iter()
                .map(|s| s.file.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                > 1
            {
                bail!("RTTM contains several recordings; pass --file-id");
            }
            if reference.is_empty() {
                bail!("no SPEAKER segments in {}", rttm.display());
            }

            let document = load_document(&document)?;
            let hypothesis: Vec<(f64, f64, String)> = document
                .segments
                .iter()
                .filter_map(|segment| match &segment.kind {
                    SegmentKind::SpeakerTurn { speaker, .. } => Some((
                        segment.range.start.as_secs_f64(),
                        segment.range.end.as_secs_f64(),
                        speaker.as_str().to_string(),
                    )),
                    _ => None,
                })
                .collect();
            if hypothesis.is_empty() {
                bail!("document has no speaker turns (was diarization enabled?)");
            }

            let Some(score) = diarization_error_rate(&reference, &hypothesis, collar, frame) else {
                bail!("could not score diarization");
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&score)?);
            } else {
                println!(
                    "DER {:.2}%  JER {:.2}%  missed {:.2}%  false alarm {:.2}%  confusion {:.2}%",
                    score.der * 100.0,
                    score.jer * 100.0,
                    score.missed_speech * 100.0,
                    score.false_alarm * 100.0,
                    score.confusion * 100.0
                );
                println!(
                    "reference speakers {} / hypothesis speakers {}",
                    score.reference_speakers, score.hypothesis_speakers
                );
                for (reference_speaker, hypothesis_speaker) in &score.mapping {
                    println!("  {reference_speaker} <- {hypothesis_speaker}");
                }
            }
        }
        Command::Report {
            document,
            document_flag,
        } => {
            let document = document
                .or(document_flag)
                .context("provide the document path (or the deprecated --document)")?;
            let document = load_document(&document)?;
            println!("CAPABILITY   PROVIDER               MS  STATUS");
            let mut total = 0_u64;
            for run in &document.runs {
                total += run.duration_ms;
                let status = match &run.status {
                    sonolytica_core::RunStatus::Complete => "complete".to_string(),
                    sonolytica_core::RunStatus::Partial { reason } => format!("partial: {reason}"),
                    sonolytica_core::RunStatus::Failed { reason } => format!("FAILED: {reason}"),
                };
                println!(
                    "{:<12} {:<22} {:>10}  {}",
                    run.capability.to_string(),
                    run.provider,
                    run.duration_ms,
                    status
                );
            }
            println!("total analysis time: {total} ms");
            let audio_ms = document.duration().as_millis().max(1) as f64;
            println!("speed: {:.2}x realtime", audio_ms / (total.max(1) as f64));
            for warning in &document.warnings {
                println!("warning [{}] {}", warning.code, warning.message);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_accepts_a_positional_document() {
        let cli = Cli::try_parse_from(["sonolytica-eval", "report", "doc.json"]).unwrap();
        match cli.command {
            Command::Report {
                document,
                document_flag,
            } => {
                assert_eq!(document.as_deref(), Some(Path::new("doc.json")));
                assert!(document_flag.is_none());
            }
            _ => panic!("expected report"),
        }
    }

    #[test]
    fn report_keeps_the_document_flag_as_an_alias() {
        let cli =
            Cli::try_parse_from(["sonolytica-eval", "report", "--document", "doc.json"]).unwrap();
        match cli.command {
            Command::Report {
                document,
                document_flag,
            } => {
                assert!(document.is_none());
                assert_eq!(document_flag.as_deref(), Some(Path::new("doc.json")));
            }
            _ => panic!("expected report"),
        }
    }

    #[test]
    fn wer_accepts_raw_text_and_files() {
        let cli = Cli::try_parse_from([
            "sonolytica-eval",
            "wer",
            "--reference-text",
            "one two",
            "--hypothesis",
            "hyp.json",
        ])
        .unwrap();
        match cli.command {
            Command::Wer {
                reference,
                reference_text,
                hypothesis,
                hypothesis_text,
                ..
            } => {
                assert!(reference.is_none());
                assert_eq!(reference_text.as_deref(), Some("one two"));
                assert_eq!(hypothesis.as_deref(), Some(Path::new("hyp.json")));
                assert!(hypothesis_text.is_none());
            }
            _ => panic!("expected wer"),
        }
    }

    #[test]
    fn wer_rejects_two_sources_for_one_side() {
        assert!(
            Cli::try_parse_from([
                "sonolytica-eval",
                "wer",
                "--reference",
                "ref.txt",
                "--reference-text",
                "one two",
                "--hypothesis-text",
                "one two",
            ])
            .is_err()
        );
    }
}
