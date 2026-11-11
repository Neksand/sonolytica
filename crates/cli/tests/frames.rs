//! Keyframes end to end: `analyze` writes images, `query frames` finds them.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn ffmpeg(args: &[&str]) {
    let status = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(args)
        .status()
        .expect("ffmpeg must be installed for these tests");
    assert!(status.success(), "ffmpeg failed for {args:?}");
}

fn sonolytica(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonolytica"))
        .args(args)
        .output()
        .expect("run sonolytica")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sono-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create workdir");
    dir
}

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

#[test]
fn analyze_writes_keyframes_and_query_returns_them() {
    let dir = workdir("frames");
    let video = dir.join("cuts.mp4");
    let doc = dir.join("doc.json");
    make_video(&video);

    let output = sonolytica(&[
        "analyze",
        video.to_str().unwrap(),
        "--out",
        doc.to_str().unwrap(),
        "--modules",
        "scene",
        "--no-hash",
    ]);
    assert!(
        output.status.success(),
        "analyze failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let document: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&doc).unwrap()).unwrap();
    let artifacts = document["artifacts"].as_array().expect("artifacts");
    assert_eq!(artifacts.len(), 3, "one keyframe per scene: {artifacts:?}");
    for artifact in artifacts {
        assert_eq!(artifact["kind"], "keyframe");
        let path = dir.join(artifact["path"].as_str().unwrap());
        assert!(path.exists(), "missing image {}", path.display());
        let bytes = std::fs::metadata(&path).unwrap().len();
        assert!(bytes > 1000, "suspiciously small image: {bytes} bytes");
    }

    let all = sonolytica(&["query", "frames", doc.to_str().unwrap(), "--json"]);
    assert!(all.status.success());
    let value: serde_json::Value = serde_json::from_slice(&all.stdout).unwrap();
    assert_eq!(value["count"], 3);
    for frame in value["frames"].as_array().unwrap() {
        assert!(Path::new(frame["path"].as_str().unwrap()).exists());
    }

    // Half-open window: the frame at exactly 1.5 s is inside, the one at 2.5 s
    // is not.
    let window = sonolytica(&[
        "query",
        "frames",
        doc.to_str().unwrap(),
        "--from",
        "1.5s",
        "--to",
        "2.5s",
        "--json",
    ]);
    let value: serde_json::Value = serde_json::from_slice(&window.stdout).unwrap();
    assert_eq!(value["count"], 1, "window {value:?}");

    let capped = sonolytica(&[
        "query",
        "frames",
        doc.to_str().unwrap(),
        "--max",
        "2",
        "--json",
    ]);
    let value: serde_json::Value = serde_json::from_slice(&capped.stdout).unwrap();
    assert_eq!(value["count"], 2);
}

#[test]
fn no_frames_flag_skips_extraction() {
    let dir = workdir("noframes");
    let video = dir.join("cuts.mp4");
    let doc = dir.join("doc.json");
    make_video(&video);

    let output = sonolytica(&[
        "analyze",
        video.to_str().unwrap(),
        "--out",
        doc.to_str().unwrap(),
        "--modules",
        "scene",
        "--no-hash",
        "--no-frames",
    ]);
    assert!(output.status.success());
    let document: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&doc).unwrap()).unwrap();
    assert!(document["artifacts"].as_array().unwrap().is_empty());
    assert!(!dir.join("doc.json.artifacts").exists());
}
