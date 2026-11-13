//! Model-free pipeline tests. These must pass on a machine with FFmpeg and no
//! models at all: that is the point of the capability/module split.

use std::path::Path;
use std::process::Command;

use sonolytica_core::{AnalysisBlock, AnalysisConfig, Capability, Document, RunStatus};
use sonolytica_engine::Engine;
use sonolytica_providers::{ModelStore, Registry};

fn tone_fixture(path: &Path, seconds: f32) {
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={seconds}:sample_rate=32000"),
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(path)
        .status()
        .expect("ffmpeg is required for these tests");
    assert!(status.success());
}

fn video_fixture(path: &Path) {
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=64x64:d=0.5",
        ])
        .arg(path)
        .status()
        .expect("ffmpeg is required for these tests");
    assert!(status.success());
}

#[tokio::test]
async fn dsp_only_analysis_produces_a_valid_document() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    tone_fixture(&path, 3.0);

    // Empty registry: no models, no providers. The DSP modules must still run.
    let engine = Engine::discover(ModelStore::new(dir.path().join("models")));
    let config = AnalysisConfig {
        modules: vec![Capability::Music, Capability::Acoustic],
        ..AnalysisConfig::default()
    };

    let document = engine.analyze(&path, &config).await.unwrap();
    assert!(document.validate().is_empty());
    assert!(document.media.sha256.is_some(), "hashing is on by default");
    assert_eq!(document.runs.len(), 2, "one run per DSP capability");
    assert!(
        document
            .runs
            .iter()
            .all(|r| matches!(r.status, RunStatus::Complete))
    );

    let music = document
        .analyses
        .iter()
        .find_map(|a| match a {
            AnalysisBlock::Music(m) => Some(m),
            _ => None,
        })
        .expect("music analysis");
    let loudness = music.loudness.as_ref().expect("measurable loudness");
    // A steady tone must measure as a finite, negative loudness; the exact
    // value depends on the generator's amplitude.
    assert!(
        (-60.0..0.0).contains(&loudness.integrated_lufs),
        "integrated loudness {} is implausible",
        loudness.integrated_lufs
    );
    assert!(
        !music.is_present(),
        "a single steady tone is not enough musical evidence"
    );
    assert!(music.key.is_none(), "gated music must not publish a key");

    let acoustic = document
        .analyses
        .iter()
        .find_map(|a| match a {
            AnalysisBlock::Acoustic(a) => Some(a),
            _ => None,
        })
        .expect("acoustic analysis");
    // The fixture has no silence, so silence must not be invented.
    assert!(acoustic.silence.is_empty());
    assert!(acoustic.peak_dbfs.is_some());

    let json = document.to_json_pretty().unwrap();
    let parsed = Document::from_json(&json).unwrap();
    assert_eq!(parsed.schema_version, document.schema_version);
}

#[tokio::test]
async fn missing_providers_degrade_to_warnings_not_failures() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    tone_fixture(&path, 1.0);

    let engine = Engine::discover(ModelStore::new(dir.path().join("models")));
    let config = AnalysisConfig {
        modules: vec![
            Capability::Vad,
            Capability::Transcribe,
            Capability::Diarize,
            Capability::Acoustic,
        ],
        ..AnalysisConfig::default()
    };
    let document = engine.analyze(&path, &config).await.unwrap();

    assert!(document.validate().is_empty());
    assert!(
        document
            .warnings
            .iter()
            .any(|w| w.code == "provider_unavailable"),
        "warnings: {:?}",
        document.warnings
    );
    // The acoustic capability still ran.
    assert!(
        document
            .analyses
            .iter()
            .any(|a| matches!(a, AnalysisBlock::Acoustic(_)))
    );
    // Unavailable stages are not recorded as failed runs; they did not run.
    assert!(
        !document
            .runs
            .iter()
            .any(|r| r.capability == Capability::Transcribe)
    );
}

#[tokio::test]
async fn video_without_audio_is_rejected_for_audio_analysis() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("silent.mkv");
    video_fixture(&path);

    let engine = Engine::discover(ModelStore::new(dir.path().join("models")));

    // Metadata-only inspection succeeds on a silent video.
    let inspect = AnalysisConfig {
        modules: vec![Capability::Inspect],
        ..AnalysisConfig::default()
    };
    let document = engine.analyze(&path, &inspect).await.unwrap();
    assert!(document.media.has_video());
    assert!(!document.media.has_audio());

    // Audio analysis fails with a clear error rather than an empty document.
    let audio = AnalysisConfig {
        modules: vec![Capability::Acoustic],
        ..AnalysisConfig::default()
    };
    let error = engine.analyze(&path, &audio).await.unwrap_err();
    assert!(
        error.to_string().contains("no audio"),
        "unexpected error: {error}"
    );
}

#[test]
fn empty_registry_reports_what_is_missing() {
    let registry = Registry::empty();
    assert!(registry.vad().is_none());
    assert!(registry.asr_for(None).is_none());
    assert!(registry.diarizer().is_none());
}
