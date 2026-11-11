//! End-to-end tests for the video frame sampler and shot detection.

use std::path::Path;
use std::process::Command;

use sonolytica_media::{FrameOptions, MediaError, MediaTools, detect_shots, sample_video};

fn ffmpeg(args: &[&str]) {
    let status = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(args)
        .status()
        .expect("ffmpeg must be installed for these tests");
    assert!(status.success(), "ffmpeg failed for {args:?}");
}

fn three_colour_video(path: &Path) {
    let out = path.to_str().unwrap();
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "color=c=red:s=320x180:r=25:d=1",
        "-f",
        "lavfi",
        "-i",
        "color=c=blue:s=320x180:r=25:d=1",
        "-f",
        "lavfi",
        "-i",
        "color=c=white:s=320x180:r=25:d=1",
        "-filter_complex",
        "[0:v][1:v][2:v]concat=n=3:v=1:a=0[v]",
        "-map",
        "[v]",
        "-pix_fmt",
        "yuv420p",
        out,
    ]);
}

#[tokio::test]
async fn finds_cuts_in_a_generated_video() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cuts.mp4");
    three_colour_video(&path);

    let tools = MediaTools::detect();
    let frames = sample_video(&tools, &path, &FrameOptions::default())
        .await
        .unwrap();
    assert!(
        (5..=7).contains(&frames.len()),
        "3 s at 2 fps should give 6 frames, got {}",
        frames.len()
    );

    let shots = detect_shots(&frames);
    assert_eq!(shots.len(), 2, "two colour changes, two cuts: {shots:?}");
    for (shot, expected) in shots.iter().zip([1.0_f32, 2.0]) {
        assert!(
            (shot.start_s - expected).abs() <= 0.5,
            "cut at {} s, expected ≈{expected} s",
            shot.start_s
        );
    }
}

#[tokio::test]
async fn a_static_video_has_no_cuts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("static.mp4");
    let out = path.to_str().unwrap();
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "color=c=gray:s=320x180:r=25:d=2",
        "-pix_fmt",
        "yuv420p",
        out,
    ]);

    let tools = MediaTools::detect();
    let frames = sample_video(&tools, &path, &FrameOptions::default())
        .await
        .unwrap();
    assert!(!frames.is_empty());
    assert!(detect_shots(&frames).is_empty());
}

#[tokio::test]
async fn an_audio_file_reports_no_video_stream() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=1:sample_rate=16000",
        "-c:a",
        "pcm_s16le",
        path.to_str().unwrap(),
    ]);

    let tools = MediaTools::detect();
    let error = sample_video(&tools, &path, &FrameOptions::default())
        .await
        .unwrap_err();
    assert!(
        matches!(error, MediaError::NoVideoStream),
        "expected NoVideoStream, got {error}"
    );
}
