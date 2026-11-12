//! Real end-to-end forced-alignment test: Whisper transcript, wav2vec2 word
//! boundaries, two synthetic voices.
//!
//! Run with:
//!   SONOLYTICA_E2E=1 cargo test -p sonolytica-engine --features full --test e2e_alignment -- --nocapture
//!
//! It is skipped by default because it needs the pinned models in the local
//! model store and macOS `say` for the fixture. Without the `align` feature, or
//! without the wav2vec2 model, the test skips with a message rather than
//! failing: alignment is an optional refinement of ASR output.

#![cfg(feature = "full")]

use std::path::Path;
use std::process::Command;

use sonolytica_core::{AnalysisBlock, AnalysisConfig, Capability, SegmentKind, Word};
use sonolytica_engine::Engine;
use sonolytica_providers::ModelStore;

#[cfg(not(target_os = "macos"))]
#[tokio::test]
async fn forced_alignment_keeps_words_close_to_whisper() {
    eprintln!("skipped: the fixture generator uses macOS `say`");
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn forced_alignment_keeps_words_close_to_whisper() {
    if std::env::var("SONOLYTICA_E2E").is_err() {
        eprintln!("skipped: set SONOLYTICA_E2E=1 to run the model-backed test");
        return;
    }

    let store = ModelStore::default();
    if !sonolytica_providers::align::available(&store) {
        eprintln!(
            "skipped: the `align` feature or the wav2vec2-base-960h model is missing \
             (`sonolytica models fetch wav2vec2-base-960h`)"
        );
        return;
    }
    if !store.is_present(
        sonolytica_providers::find_model("whisper-small").expect("whisper-small is registered"),
    ) {
        eprintln!("skipped: run `sonolytica models fetch whisper-small` first");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let conversation = dir.path().join("conversation.wav");
    build_fixture(&conversation);

    let engine = Engine::discover(store);
    let base = AnalysisConfig {
        modules: vec![Capability::Vad, Capability::Transcribe],
        language: Some("en".into()),
        asr_model: Some("whisper-small".into()),
        align_words: false,
        ..AnalysisConfig::default()
    };
    let aligned = AnalysisConfig {
        align_words: true,
        ..base.clone()
    };

    let whisper = engine.analyze(&conversation, &base).await.unwrap();
    let forced = engine.analyze(&conversation, &aligned).await.unwrap();
    assert!(whisper.validate().is_empty());
    assert!(forced.validate().is_empty());

    let align_runs: Vec<_> = forced
        .runs
        .iter()
        .filter(|run| run.capability == Capability::Align)
        .collect();
    eprintln!(
        "align runs: {} · warnings: {:?}",
        align_runs.len(),
        forced
            .warnings
            .iter()
            .map(|w| w.code.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        !align_runs.is_empty(),
        "the run must record alignment provenance: {:?}",
        forced.warnings
    );
    assert!(
        align_runs
            .iter()
            .all(|run| run.provider == "wav2vec2-align" && run.device == "cpu"),
        "align runs must be attributed to the CPU wav2vec2 provider"
    );

    // Aligned words must be ordered and inside the segment they describe.
    let mut aligned_words: Vec<&Word> = Vec::new();
    for segment in &forced.segments {
        if let SegmentKind::Speech { words, .. } = &segment.kind {
            let mut previous_end = segment.range.start;
            for word in words {
                assert!(
                    !word.range.is_empty()
                        && word.range.start >= previous_end
                        && word.range.end <= segment.range.end,
                    "word `{}` {} is outside or before segment {}",
                    word.text,
                    word.range,
                    segment.range
                );
                previous_end = word.range.end;
                aligned_words.push(word);
            }
        }
    }
    assert!(!aligned_words.is_empty(), "alignment produced no words");

    let whisper_words: Vec<&Word> = whisper
        .segments
        .iter()
        .filter_map(|segment| match &segment.kind {
            SegmentKind::Speech { words, .. } => Some(words.iter()),
            _ => None,
        })
        .flatten()
        .collect();

    let pairs = pair_by_text(&whisper_words, &aligned_words);
    assert!(
        pairs.len() >= 8,
        "expected at least 8 comparable words, got {} (whisper {}, aligned {})",
        pairs.len(),
        whisper_words.len(),
        aligned_words.len()
    );

    let mut within_200 = 0_usize;
    let mut within_600 = 0_usize;
    let mut drifts = Vec::with_capacity(pairs.len());
    for (whisper, aligned) in &pairs {
        let start_drift = (aligned.range.start - whisper.range.start)
            .as_millis()
            .abs();
        let end_drift = (aligned.range.end - whisper.range.end).as_millis().abs();
        if start_drift <= 200 {
            within_200 += 1;
        }
        if start_drift <= 600 {
            within_600 += 1;
        }
        drifts.push((whisper.text.clone(), start_drift, end_drift));
    }

    eprintln!("word drift (start/end ms) vs Whisper:");
    for (text, start, end) in &drifts {
        eprintln!("  {text:<24} {start:>5} / {end:>5}");
    }
    let mut sorted: Vec<i64> = drifts.iter().map(|(_, start, _)| *start).collect();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    let ratio_200 = within_200 as f64 / pairs.len() as f64;
    let ratio_600 = within_600 as f64 / pairs.len() as f64;
    eprintln!(
        "drift: median {median} ms · within 200 ms {within_200}/{} ({:.0}%) · \
         within 600 ms {within_600}/{} ({:.0}%)",
        pairs.len(),
        ratio_200 * 100.0,
        pairs.len(),
        ratio_600 * 100.0
    );

    // Whisper's token timestamps are systematically early here — the aligned
    // words match the model's greedy CTC boundaries to the frame. The 200 ms
    // ratio is printed for comparison code, while the assertion is a
    // regression bound that still catches a wrong frame rate or offset.
    assert!(
        ratio_600 >= 0.80,
        "forced alignment moved {} of {} words more than 600 ms",
        pairs.len() - within_600,
        pairs.len()
    );
    assert!(
        median <= 300,
        "median word drift is {median} ms; the frame mapping is probably wrong"
    );

    // A word list is only useful if the transcript itself survived.
    let speech = forced
        .analyses
        .iter()
        .find_map(|analysis| match analysis {
            AnalysisBlock::Speech(speech) => Some(speech),
            _ => None,
        })
        .expect("speech summary");
    assert!(speech.word_count >= 15, "got {} words", speech.word_count);
}

#[cfg(target_os = "macos")]
fn build_fixture(out: &Path) {
    let dir = out.parent().unwrap();
    let first = dir.join("first.wav");
    let second = dir.join("second.wav");
    speak(
        "Alice",
        "The quick brown fox jumps over the lazy dog. \
         This is a test of the speech recognition pipeline.",
        &first,
    );
    speak(
        "Albert",
        "Hello, I am the second speaker. \
         We are checking that speaker diarization separates two voices.",
        &second,
    );

    let status = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(&first)
        .arg("-i")
        .arg(&second)
        .args([
            "-filter_complex",
            "[0:a][1:a]concat=n=2:v=0:a=1[out]",
            "-map",
            "[out]",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(out)
        .status()
        .expect("ffmpeg is required");
    assert!(status.success());
}

#[cfg(target_os = "macos")]
fn speak(voice: &str, text: &str, out: &Path) {
    let aiff = out.with_extension("aiff");
    let status = Command::new("say")
        .args(["-v", voice, "-o"])
        .arg(&aiff)
        .arg(text)
        .status()
        .expect("say is required to generate fixtures");
    assert!(status.success(), "`say -v {voice}` failed");
    let status = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(&aiff)
        .args(["-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
        .arg(out)
        .status()
        .expect("ffmpeg is required");
    assert!(status.success());
}

/// Pair Whisper and aligned words by normalised text, in order. Punctuation
/// the aligner's vocabulary cannot represent is ignored, and a word the
/// aligner dropped simply has no pair.
#[cfg(target_os = "macos")]
fn pair_by_text<'a>(whisper: &[&'a Word], aligned: &[&'a Word]) -> Vec<(&'a Word, &'a Word)> {
    let mut pairs = Vec::new();
    let mut next = 0_usize;
    for word in whisper {
        let target = normalize(&word.text);
        let Some(offset) = aligned[next..]
            .iter()
            .take(4)
            .position(|candidate| normalize(&candidate.text) == target)
        else {
            continue;
        };
        let candidate = aligned[next + offset];
        pairs.push((*word, candidate));
        next += offset + 1;
    }
    pairs
}

#[cfg(target_os = "macos")]
fn normalize(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '\'')
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}
