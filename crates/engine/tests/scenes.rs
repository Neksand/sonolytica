//! Video scene detection end to end: no models, only FFmpeg and the document.

use std::path::{Path, PathBuf};
use std::process::Command;

use sonolytica_core::{AnalysisConfig, Capability, MediaTime, RunStatus, SegmentKind};
use sonolytica_engine::Engine;
use sonolytica_providers::ModelStore;

fn ffmpeg(args: &[&str]) {
    let status = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(args)
        .status()
        .expect("ffmpeg must be installed for these tests");
    assert!(status.success(), "ffmpeg failed for {args:?}");
}

/// Three one-second colour shots, no audio: this is also the video-only path.
fn make_video(path: &Path) {
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
        path.to_str().unwrap(),
    ]);
}

#[tokio::test]
async fn detects_scenes_in_a_video_only_file() {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("cuts.mp4");
    make_video(&path);

    let engine = Engine::discover(ModelStore::default());
    let config = AnalysisConfig {
        modules: vec![Capability::Scene],
        ..AnalysisConfig::default()
    };
    let document = engine.analyze(&path, &config).await.unwrap();

    assert!(document.validate().is_empty());
    assert_eq!(document.runs.len(), 1, "one scene run: {:?}", document.runs);
    assert!(matches!(document.runs[0].status, RunStatus::Complete));
    assert_eq!(document.runs[0].capability, Capability::Scene);

    let scenes: Vec<_> = document
        .segments
        .iter()
        .filter(|segment| matches!(segment.kind, SegmentKind::Scene { .. }))
        .collect();
    assert_eq!(scenes.len(), 3, "three shots: {scenes:?}");

    assert_eq!(scenes[0].range.start, MediaTime::ZERO);
    assert!(
        !matches!(scenes[0].kind, SegmentKind::Scene { cut: true, .. }),
        "the first scene does not start with a cut"
    );
    for pair in scenes.windows(2) {
        assert_eq!(
            pair[0].range.end, pair[1].range.start,
            "scenes must be contiguous"
        );
    }
    for (segment, expected) in scenes.iter().skip(1).zip([1.0_f64, 2.0]) {
        let start = segment.range.start.as_secs_f64();
        assert!(
            (start - expected).abs() <= 0.6,
            "scene at {start} s, expected ≈{expected} s"
        );
        assert!(
            matches!(segment.kind, SegmentKind::Scene { cut: true, .. }),
            "scenes after the first start with a cut"
        );
    }
    assert_eq!(
        scenes.last().unwrap().range.end,
        document.duration(),
        "the last scene ends with the media"
    );
}
