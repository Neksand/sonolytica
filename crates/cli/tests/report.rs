//! `sonolytica report` writes a human HTML report without network or JS.

use std::path::{Path, PathBuf};
use std::process::Command;

use sonolytica_core::{
    AnalysisConfig, Artifact, BBox, Capability, Confidence, DocumentBuilder, MediaInfo, MediaTime,
    RunRecord, RunStatus, SegmentKind, SpeakerId, TimeRange, TrackKind,
};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_sonolytica")
}

fn run_record(capability: Capability, model: &str) -> RunRecord {
    RunRecord {
        id: 0,
        capability,
        provider: "test-provider".into(),
        model: Some(sonolytica_core::ModelRef::new(model)),
        params_sha256: "0".repeat(64),
        device: "cpu".into(),
        started_at_unix_ms: 0,
        duration_ms: 5,
        status: RunStatus::Complete,
    }
}

fn fixture_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sonolytica-report-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("doc.json.artifacts/frames")).unwrap();
    std::fs::write(
        dir.join("doc.json.artifacts/frames/frame_0001.jpg"),
        b"JPEG",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("other")).unwrap();
    std::fs::write(dir.join("other/frame_0001.jpg"), b"SECOND").unwrap();
    std::fs::write(dir.join("secret.jpg"), b"SECRET").unwrap();
    dir
}

fn write_document(dir: &Path) -> PathBuf {
    let media = MediaInfo {
        path: "/tmp/report-fixture.mp4".into(),
        duration: Some(MediaTime::from_secs_f64(30.0)),
        ..MediaInfo::default()
    };
    let mut builder = DocumentBuilder::new(media, AnalysisConfig::default());
    let diarize = builder.add_run(run_record(Capability::Diarize, "sortformer-4spk-v2"));
    let turn_track = builder.add_track(TrackKind::SpeakerTurn, "speaker_turns", diarize);
    builder.add_segment(
        turn_track,
        TimeRange::from_secs(1.0, 4.0),
        SegmentKind::SpeakerTurn {
            speaker: SpeakerId::new("S1"),
            text: "Hello <script>alert('x')</script> world".into(),
            overlap: false,
        },
        Confidence::new(0.9),
        diarize,
    );

    let text = builder.add_run(run_record(Capability::Vision, "pp-ocrv5"));
    let text_track = builder.add_track(TrackKind::Text, "ocr", text);
    builder.add_segment(
        text_track,
        TimeRange::empty(MediaTime::from_secs_f64(2.0)),
        SegmentKind::TextOverlay {
            text: "QUARTERLY REVENUE".into(),
            bbox: BBox {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 5.0,
            },
        },
        Confidence::new(0.9),
        text,
    );
    let object_run = builder.add_run(run_record(Capability::Vision, "yunet"));
    let person_track = builder.add_track(TrackKind::Object, "objects", object_run);
    builder.add_segment(
        person_track,
        TimeRange::empty(MediaTime::from_secs_f64(2.0)),
        SegmentKind::Object {
            class: "person".into(),
            bbox: Some(BBox {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            }),
        },
        Confidence::new(0.9),
        object_run,
    );
    let object_track = builder.add_track(TrackKind::Object, "faces", object_run);
    builder.add_segment(
        object_track,
        TimeRange::empty(MediaTime::from_secs_f64(2.0)),
        SegmentKind::Object {
            class: "face".into(),
            bbox: Some(BBox {
                x: 10.0,
                y: 10.0,
                width: 50.0,
                height: 50.0,
            }),
        },
        Confidence::new(0.9),
        object_run,
    );

    for (path, at) in [
        ("doc.json.artifacts/frames/frame_0001.jpg", 2.0),
        ("other/frame_0001.jpg", 4.0),
        ("../secret.jpg", 6.0),
    ] {
        builder.add_artifact(Artifact {
            kind: "keyframe".into(),
            path: path.into(),
            range: TimeRange::empty(MediaTime::from_secs_f64(at)),
            sha256: None,
            width: Some(640),
            height: Some(360),
        });
    }

    let document = builder.finish().expect("report fixture must be valid");
    let path = dir.join("doc.json");
    std::fs::write(&path, document.to_json_pretty().unwrap()).unwrap();
    path
}

#[test]
fn report_is_self_contained_and_escapes_text() {
    let dir = fixture_dir();
    let doc = write_document(&dir);
    let out = dir.join("report.html");
    let output = Command::new(binary())
        .args([
            "report",
            doc.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("run sonolytica report");
    assert!(
        output.status.success(),
        "report failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let html = std::fs::read_to_string(&out).unwrap();
    for needle in [
        "<h2>Transcript</h2>",
        "<h2>Frames and on-screen text</h2>",
        "S1",
        "QUARTERLY REVENUE",
        "1 face",
        "objects: person",
        "&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;",
        "report.html.artifacts/frames/00000-frame_0001.jpg",
    ] {
        assert!(html.contains(needle), "missing `{needle}` in report");
    }
    assert!(
        !html.contains("<script>"),
        "report must not emit raw script tags"
    );
    assert!(
        !html.contains("http://") && !html.contains("https://"),
        "a report must not depend on the network"
    );
    let frames_dir = dir.join("report.html.artifacts/frames");
    assert!(
        frames_dir.join("00000-frame_0001.jpg").exists()
            && frames_dir.join("00001-frame_0001.jpg").exists(),
        "same basenames must not overwrite each other: {:?}",
        std::fs::read_dir(&frames_dir)
            .map(|entries| entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name())
                .collect::<Vec<_>>())
            .unwrap_or_default()
    );
    assert!(
        !html.contains("secret.jpg"),
        "a document must not copy files from outside its directory"
    );
    assert!(
        !frames_dir.join("secret.jpg").exists(),
        "path traversal must be refused"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
