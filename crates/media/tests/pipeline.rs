//! End-to-end tests for the FFmpeg adapters, using a generated fixture.

use std::path::Path;
use std::process::Command;

use sonolytica_core::Limits;
use sonolytica_media::{
    DecodeOptions, MediaTools, decode_mono, probe, sha256_file, validate_input,
};

fn make_fixture(path: &Path) {
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1:sample_rate=44100",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(path)
        .status()
        .expect("ffmpeg must be installed for these tests");
    assert!(status.success(), "fixture generation failed");
}

#[tokio::test]
async fn probes_and_decodes_a_real_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    make_fixture(&path);

    let tools = MediaTools::detect();
    let limits = Limits::default();

    let info = probe(&tools, &path, &limits).await.unwrap();
    assert_eq!(info.streams.len(), 1);
    assert!(info.has_audio());
    assert!(!info.has_video());
    let audio_stream = info.primary_audio().unwrap();
    assert_eq!(audio_stream.sample_rate, Some(16_000));
    assert_eq!(audio_stream.channels, Some(1));
    let duration = info.duration.expect("duration");
    assert!(
        (duration.as_secs_f64() - 1.0).abs() < 0.1,
        "duration was {}",
        duration.as_secs_f64()
    );

    let audio = decode_mono(
        &tools,
        &path,
        &DecodeOptions {
            sample_rate: 16_000,
            ..DecodeOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(audio.sample_rate, 16_000);
    assert!(
        (audio.len() as i64 - 16_000).abs() <= 160,
        "expected ~16000 samples, got {}",
        audio.len()
    );
    assert!(audio.peak() > 0.1, "the tone should not be silent");
}

#[tokio::test]
async fn rejects_inputs_without_audio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("silent_video.mkv");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=64x64:d=0.2",
        ])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());

    let tools = MediaTools::detect();
    let result = decode_mono(&tools, &path, &DecodeOptions::default()).await;
    assert!(
        matches!(result, Err(sonolytica_media::MediaError::NoAudioStream)),
        "expected NoAudioStream, got {result:?}"
    );
}

#[tokio::test]
async fn hashing_and_validation_agree() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    make_fixture(&path);

    let validated = validate_input(&path, &Limits::default()).unwrap();
    assert_eq!(
        validated.size_bytes,
        std::fs::metadata(&path).unwrap().len()
    );
    let hash = sha256_file(&validated.path).unwrap();
    assert_eq!(hash.len(), 64);
    assert_eq!(hash, sha256_file(&path).unwrap());
}
