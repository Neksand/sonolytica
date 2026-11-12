//! Real end-to-end speech test: two synthetic voices, real models.
//!
//! Run with:
//!   SONOLYTICA_E2E=1 cargo test -p sonolytica-engine --features full --test e2e_speech -- --nocapture
//!
//! It is skipped by default because it needs the pinned models in the local
//! model store and macOS `say` for the fixtures. The assertions are functional
//! (plumbing, timestamps, speaker separation), not accuracy claims.

#![cfg(feature = "full")]

use std::path::Path;
use std::process::Command;

use sonolytica_core::{AnalysisBlock, AnalysisConfig, Capability, RunStatus, SegmentKind};
use sonolytica_engine::Engine;
use sonolytica_providers::ModelStore;

#[cfg(not(target_os = "macos"))]
#[tokio::test]
async fn two_speaker_conversation_end_to_end() {
    eprintln!("skipped: the fixture generator uses macOS `say`");
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn two_speaker_conversation_end_to_end() {
    if std::env::var("SONOLYTICA_E2E").is_err() {
        eprintln!("skipped: set SONOLYTICA_E2E=1 to run the model-backed test");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.wav");
    let second = dir.path().join("second.wav");
    let conversation = dir.path().join("conversation.wav");

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
        .arg(&conversation)
        .status()
        .expect("ffmpeg is required");
    assert!(status.success());

    let engine = Engine::discover(ModelStore::default());
    let registry = engine.registry();
    eprintln!("providers: {:?}", registry.provider_infos());
    if registry.asr_for(Some("whisper-base.en")).is_none() || registry.diarizer().is_none() {
        eprintln!(
            "skipped: run `sonolytica models fetch whisper-base.en pyannote-segmentation-3.0 wespeaker-campplus` first"
        );
        return;
    }

    let config = AnalysisConfig {
        modules: vec![
            Capability::Vad,
            Capability::Transcribe,
            Capability::Diarize,
            Capability::Music,
            Capability::Acoustic,
        ],
        language: Some("en".into()),
        asr_model: Some("whisper-base.en".into()),
        ..AnalysisConfig::default()
    };

    let document = engine.analyze(&conversation, &config).await.unwrap();
    assert!(document.validate().is_empty());
    eprintln!(
        "runs:\n{}",
        document
            .runs
            .iter()
            .map(|r| format!(
                "  {:?} {} {}ms {:?}",
                r.capability, r.provider, r.duration_ms, r.status
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
    eprintln!("dialogue:");
    for turn in document.dialogue() {
        eprintln!("  {} {} {}", turn.range, turn.speaker, turn.text);
    }

    assert!(
        document
            .runs
            .iter()
            .all(|r| !matches!(r.status, RunStatus::Failed { .. })),
        "no stage may fail: {:?}",
        document.warnings
    );

    let speech = document
        .analyses
        .iter()
        .find_map(|a| match a {
            AnalysisBlock::Speech(s) => Some(s),
            _ => None,
        })
        .expect("speech summary");
    assert!(speech.word_count >= 15, "got {} words", speech.word_count);
    assert_eq!(
        speech.language.as_deref(),
        Some("en"),
        "an English-only model must report en, not a stray detected code"
    );
    assert!(
        speech.speaker_count >= 2,
        "two clearly different voices should be separated, got {}",
        speech.speaker_count
    );

    let all_text = document
        .segments
        .iter()
        .filter_map(|s| match &s.kind {
            SegmentKind::Speech { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    assert!(
        all_text.contains("quick brown fox"),
        "expected a recognisable phrase, got: {all_text}"
    );

    // Every word must carry a timestamp inside the media duration.
    let duration = document.duration();
    for segment in &document.segments {
        if let SegmentKind::Speech { words, .. } = &segment.kind {
            for word in words {
                assert!(
                    word.range.start <= word.range.end && word.range.end <= duration,
                    "word `{}` has an impossible range {}",
                    word.text,
                    word.range
                );
            }
        }
    }

    // At least one speaker turn must carry transcribed text.
    assert!(document.dialogue().iter().any(|turn| !turn.text.is_empty()));
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
