//! SONOLYTICA evaluation harness.

#![allow(clippy::needless_range_loop)]

mod metrics;

use std::path::PathBuf;
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
        #[arg(long)]
        reference: PathBuf,
        /// Plain text, or a SONOLYTICA document (JSON)
        #[arg(long)]
        hypothesis: PathBuf,
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
        #[arg(long)]
        document: PathBuf,
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
            hypothesis,
            json,
        } => {
            let reference_text = std::fs::read_to_string(&reference)
                .with_context(|| format!("could not read {}", reference.display()))?;
            let hypothesis_text = if hypothesis.extension().is_some_and(|e| e == "json") {
                transcript_of(&load_document(&hypothesis)?)
            } else {
                std::fs::read_to_string(&hypothesis)
                    .with_context(|| format!("could not read {}", hypothesis.display()))?
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
        Command::Report { document } => {
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
