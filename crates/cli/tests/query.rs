//! End-to-end tests for `sonolytica query`: a real document, the real binary,
//! and assertions on both JSON and the human table.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use sonolytica_core::{
    AcousticSummary, AnalysisBlock, AnalysisConfig, BBox, Bpm, Capability, Chord, ChordQuality,
    Confidence, Document, DocumentBuilder, KeyEstimate, Keypoint, Loudness, MediaInfo, MediaTime,
    Mode, ModelRef, MusicSummary, PitchClass, RunRecord, RunStatus, SegmentKind, SpeakerId,
    TimeRange, TrackKind, Warning,
};

fn run_record(capability: Capability, model: &str, status: RunStatus) -> RunRecord {
    RunRecord {
        id: 0,
        capability,
        provider: match capability {
            Capability::Transcribe => "whisper-cpp",
            Capability::Diarize => "pyannote",
            _ => "silero-vad",
        }
        .into(),
        model: Some(ModelRef::new(model)),
        params_sha256: "0".repeat(64),
        device: "cpu".into(),
        started_at_unix_ms: 0,
        duration_ms: 42,
        status,
    }
}

fn fixture() -> Document {
    let media = MediaInfo {
        path: "/tmp/query-fixture.wav".into(),
        duration: Some(MediaTime::from_millis(15_000)),
        ..MediaInfo::default()
    };
    let mut builder = DocumentBuilder::new(media, AnalysisConfig::default());

    let vad = builder.add_run(run_record(
        Capability::Vad,
        "silero-vad-v5",
        RunStatus::Complete,
    ));
    let _ = vad;
    let transcribe = builder.add_run(run_record(
        Capability::Transcribe,
        "whisper-base.en",
        RunStatus::Partial {
            reason: "no speech recognised in region 2".into(),
        },
    ));
    let diarize = builder.add_run(run_record(
        Capability::Diarize,
        "wespeaker-campplus",
        RunStatus::Complete,
    ));

    let speech_track = builder.add_track(TrackKind::Speech, "asr", transcribe);
    let turn_track = builder.add_track(TrackKind::SpeakerTurn, "speaker_turns", diarize);

    let first = builder.add_segment(
        speech_track,
        TimeRange::from_secs(0.0, 6.0),
        SegmentKind::Speech {
            text: "Hello pricing team".into(),
            words: vec![
                sonolytica_core::Word {
                    text: "Hello".into(),
                    range: TimeRange::from_secs(0.0, 0.4),
                    confidence: Confidence::new(0.9),
                },
                sonolytica_core::Word {
                    text: "pricing".into(),
                    range: TimeRange::from_secs(1.0, 1.6),
                    confidence: Confidence::new(0.8),
                },
                sonolytica_core::Word {
                    text: "team".into(),
                    range: TimeRange::from_secs(1.7, 2.1),
                    confidence: None,
                },
            ],
            language: Some("en".into()),
            no_speech_prob: Some(0.01),
        },
        Confidence::new(0.95),
        transcribe,
    );
    let first_turn = builder.add_segment(
        turn_track,
        TimeRange::from_secs(0.0, 6.0),
        SegmentKind::SpeakerTurn {
            speaker: SpeakerId::new("S1"),
            text: "Hello pricing team".into(),
            overlap: false,
        },
        Confidence::new(0.97),
        diarize,
    );
    builder.set_parent(first_turn, Some(first));

    let second = builder.add_segment(
        speech_track,
        TimeRange::from_secs(6.0, 9.0),
        SegmentKind::Speech {
            text: "second part".into(),
            words: vec![sonolytica_core::Word {
                text: "second".into(),
                range: TimeRange::from_secs(6.1, 6.6),
                confidence: Confidence::new(0.7),
            }],
            language: Some("en".into()),
            no_speech_prob: None,
        },
        Confidence::new(0.8),
        transcribe,
    );
    let second_turn = builder.add_segment(
        turn_track,
        TimeRange::from_secs(6.0, 9.0),
        SegmentKind::SpeakerTurn {
            speaker: SpeakerId::new("S2"),
            text: "second part".into(),
            overlap: false,
        },
        Confidence::new(0.9),
        diarize,
    );
    builder.set_parent(second_turn, Some(second));

    let third = builder.add_segment(
        speech_track,
        TimeRange::from_secs(9.0, 15.0),
        SegmentKind::Speech {
            text: "closing remarks about pricing".into(),
            words: vec![sonolytica_core::Word {
                text: "pricing".into(),
                range: TimeRange::from_secs(11.0, 11.5),
                confidence: Confidence::new(0.85),
            }],
            language: Some("en".into()),
            no_speech_prob: None,
        },
        Confidence::new(0.9),
        transcribe,
    );
    let third_turn = builder.add_segment(
        turn_track,
        TimeRange::from_secs(9.0, 15.0),
        SegmentKind::SpeakerTurn {
            speaker: SpeakerId::new("S1"),
            text: "closing remarks about pricing".into(),
            overlap: false,
        },
        Confidence::new(0.93),
        diarize,
    );
    builder.set_parent(third_turn, Some(third));

    builder.add_analysis(AnalysisBlock::Music(MusicSummary {
        bpm: Some(Bpm {
            value: 98.0,
            confidence: Confidence::clamped(0.31),
        }),
        beats: vec![MediaTime::ZERO, MediaTime::from_millis(612)],
        key: Some(KeyEstimate {
            tonic: PitchClass::FSharp,
            mode: Mode::Minor,
            confidence: Confidence::clamped(0.12),
        }),
        chords: vec![Chord {
            range: TimeRange::from_secs(0.0, 2.0),
            root: PitchClass::FSharp,
            quality: ChordQuality::Minor,
            confidence: Confidence::new(0.7).unwrap(),
        }],
        sections: vec![
            sonolytica_core::Section {
                range: TimeRange::from_secs(0.0, 6.0),
                novelty: 0.0,
            },
            sonolytica_core::Section {
                range: TimeRange::from_secs(6.0, 15.0),
                novelty: 0.42,
            },
        ],
        loudness: Some(Loudness {
            integrated_lufs: -17.6,
            loudness_range_lu: Some(13.9),
            true_peak_dbtp: Some(-1.0),
        }),
    }));
    builder.add_analysis(AnalysisBlock::Acoustic(AcousticSummary {
        silence: vec![TimeRange::from_secs(0.0, 0.5)],
        dynamic_range_lu: Some(13.9),
        mean_rms_dbfs: Some(-24.1),
        peak_dbfs: Some(-1.0),
        events: Vec::new(),
    }));
    builder.warn(
        Warning::new(
            "provider_unavailable",
            "pyannote-3.0 (diarize) is unavailable: model missing",
        )
        .with_run(diarize),
    );

    builder
        .finish()
        .unwrap_or_else(|violations| panic!("fixture violates invariants: {violations:?}"))
}

/// The document is built here rather than stored as JSON so the fixture cannot
/// drift from the schema: `finish()` validates every invariant.
fn fixture_path() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let document = fixture();
    assert!(document.validate().is_empty(), "fixture must be valid");
    let path = std::env::temp_dir().join(format!(
        "sonolytica-query-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, document.to_json_pretty().unwrap()).expect("write fixture");
    path
}

fn query(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonolytica"))
        .args(args)
        .output()
        .expect("run sonolytica")
}

fn query_json(args: &[&str]) -> serde_json::Value {
    let output = query(args);
    assert!(
        output.status.success(),
        "query {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid JSON on stdout")
}

#[test]
fn timeline_covers_the_document_in_order() {
    let path = fixture_path();
    let json = query_json(&["query", "timeline", path.to_str().unwrap(), "--json"]);
    let segments = json["segments"].as_array().expect("segments array");
    assert_eq!(json["count"], 6);
    assert_eq!(segments.len(), 6);
    let starts: Vec<i64> = segments
        .iter()
        .map(|segment| segment["start"].as_i64().unwrap())
        .collect();
    let mut sorted = starts.clone();
    sorted.sort_unstable();
    assert_eq!(starts, sorted, "timeline must be ordered by start");
    assert_eq!(segments[0]["kind"], "speech");
    assert_eq!(segments[0]["start_display"], "00:00:00.000");
    assert!(
        segments.iter().any(|segment| segment["speaker"] == "S1"),
        "a speaker turn must carry its speaker: {segments:?}"
    );
}

#[test]
fn timeline_window_and_type_filter() {
    let path = fixture_path();
    let json = query_json(&[
        "query",
        "timeline",
        path.to_str().unwrap(),
        "--from",
        "00:00:06",
        "--to",
        "9s",
        "--type",
        "speaker_turn",
        "--json",
    ]);
    let segments = json["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 1, "half-open window holds exactly one turn");
    assert_eq!(segments[0]["speaker"], "S2");
    assert_eq!(segments[0]["text"], "second part");
}

#[test]
fn at_reports_segments_active_at_an_instant() {
    let path = fixture_path();
    let json = query_json(&["query", "at", path.to_str().unwrap(), "7s", "--json"]);
    assert_eq!(json["count"], 2, "one utterance and its turn");
    let kinds: Vec<&str> = json["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|segment| segment["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"speech"));
    assert!(kinds.contains(&"speaker_turn"));
}

#[test]
fn search_returns_word_ranges_and_the_speaker() {
    let path = fixture_path();
    let json = query_json(&[
        "query",
        "search",
        path.to_str().unwrap(),
        "PRICING",
        "--json",
    ]);
    assert_eq!(json["count"], 2, "case-insensitive, one row per utterance");
    let first = &json["matches"][0];
    assert_eq!(first["speaker"], "S1");
    let words = first["words"].as_array().unwrap();
    assert_eq!(words.len(), 1);
    assert_eq!(words[0]["text"], "pricing");
    assert_eq!(words[0]["start"], 1_000_000);
    assert_eq!(words[0]["end"], 1_600_000);
}

#[test]
fn poses_are_returned_with_keypoints() {
    let mut document = fixture();
    let track = document.tracks.len() as u32;
    document.tracks.push(sonolytica_core::Track {
        id: track,
        kind: TrackKind::Pose,
        label: "poses".into(),
        run: 0,
    });
    document.segments.push(sonolytica_core::Segment {
        id: document.segments.len() as u32,
        track,
        range: TimeRange::empty(MediaTime::from_secs_f64(1.0)),
        parent: None,
        kind: SegmentKind::Pose {
            person: 0,
            bbox: None,
            keypoints: vec![Keypoint {
                name: "nose".into(),
                x: 0.5,
                y: 0.2,
                score: 0.9,
            }],
        },
        confidence: Confidence::new(0.8),
        run: 0,
    });
    assert!(document.validate().is_empty());
    let path = std::env::temp_dir().join(format!("sonolytica-poses-{}.json", std::process::id()));
    std::fs::write(&path, document.to_json_pretty().unwrap()).unwrap();
    let json = query_json(&["query", "poses", path.to_str().unwrap(), "--json"]);
    assert_eq!(json["count"], 1);
    assert_eq!(json["poses"][0]["person"], 0);
    assert_eq!(json["poses"][0]["keypoints"][0]["name"], "nose");
    let _ = std::fs::remove_file(path);
}

#[test]
fn ask_ranks_evidence_and_reports_absence() {
    let path = fixture_path();
    let json = query_json(&[
        "query",
        "ask",
        path.to_str().unwrap(),
        "who mentioned pricing",
        "--json",
    ]);
    assert_eq!(json["terms"], serde_json::json!(["mentioned", "pricing"]));
    let evidence = json["evidence"].as_array().unwrap();
    assert!(!evidence.is_empty(), "pricing must produce evidence");
    assert!(evidence.iter().any(|item| {
        item["text"]
            .as_str()
            .is_some_and(|text| text.contains("pricing"))
    }));
    let absent: Vec<&str> = json["absence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["term"].as_str().unwrap())
        .collect();
    assert_eq!(
        absent,
        vec!["mentioned"],
        "the absent term must be reported"
    );

    let missing = query_json(&["query", "ask", path.to_str().unwrap(), "deadline", "--json"]);
    assert!(missing["evidence"].as_array().unwrap().is_empty());
    assert_eq!(missing["absence"][0]["term"], "deadline");
    assert_eq!(missing["absence"][0]["occurrences"], 0);
}

#[test]
fn ask_drops_stopwords_instead_of_matching_everything() {
    let path = fixture_path();
    let json = query_json(&[
        "query",
        "ask",
        path.to_str().unwrap(),
        "what did they say about it",
        "--json",
    ]);
    assert_eq!(json["terms"], serde_json::json!([]));
    assert!(json["evidence"].as_array().unwrap().is_empty());
}

#[test]
fn speakers_rank_by_speaking_time() {
    let path = fixture_path();
    let json = query_json(&["query", "speakers", path.to_str().unwrap(), "--json"]);
    assert_eq!(json["speaker_count"], 2);
    let speakers = json["speakers"].as_array().unwrap();
    assert_eq!(speakers[0]["id"], "S1");
    assert_eq!(speakers[0]["speech_duration_us"], 12_000_000);
    assert_eq!(speakers[0]["turn_count"], 2);
    assert_eq!(speakers[1]["id"], "S2");
    assert_eq!(speakers[1]["speech_duration_us"], 3_000_000);
    assert_eq!(speakers[1]["first_start_us"], 6_000_000);
}

#[test]
fn music_reports_measured_values() {
    let path = fixture_path();
    let json = query_json(&["query", "music", path.to_str().unwrap(), "--json"]);
    assert_eq!(json["available"], true);
    assert_eq!(json["bpm"]["value"], 98.0);
    assert_eq!(json["key"]["tonic"], "f_sharp");
    assert_eq!(json["key"]["mode"], "minor");
    assert_eq!(json["chords"].as_array().unwrap().len(), 1);
    assert_eq!(json["sections"].as_array().unwrap().len(), 2);
    assert_eq!(json["sections"][1]["novelty"], 0.42);
    assert_eq!(json["beat_count"], 2);
    assert_eq!(json["loudness"]["integrated_lufs"], -17.6);
}

#[test]
fn acoustic_reports_silence_without_events() {
    let path = fixture_path();
    let json = query_json(&["query", "acoustic", path.to_str().unwrap(), "--json"]);
    assert_eq!(json["available"], true);
    assert_eq!(json["silence"].as_array().unwrap().len(), 1);
    assert_eq!(json["event_count"], 0);
    assert_eq!(json["peak_dbfs"], -1.0);
}

#[test]
fn runs_expose_partial_status_and_warnings() {
    let path = fixture_path();
    let json = query_json(&["query", "runs", path.to_str().unwrap(), "--json"]);
    assert_eq!(json["complete"], false);
    let runs = json["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 3);
    let partial = runs
        .iter()
        .find(|run| run["status"] == "partial")
        .expect("a partial run");
    assert_eq!(partial["capability"], "transcribe");
    assert_eq!(partial["model_id"], "whisper-base.en");
    assert_eq!(partial["reason"], "no speech recognised in region 2");
    assert_eq!(json["warnings"].as_array().unwrap().len(), 1);
}

#[test]
fn summary_reports_counts_and_media() {
    let path = fixture_path();
    let json = query_json(&["query", "summary", path.to_str().unwrap(), "--json"]);
    assert_eq!(json["counts"]["segments"], 6);
    assert_eq!(json["counts"]["tracks"], 2);
    assert_eq!(json["media"]["duration"], 15_000_000);
    assert_eq!(json["analyses"].as_array().unwrap().len(), 2);
}

#[test]
fn chapters_split_on_speaker_change() {
    let path = fixture_path();
    let json = query_json(&["query", "chapters", path.to_str().unwrap(), "--json"]);
    assert_eq!(json["count"], 2);
    let chapters = json["chapters"].as_array().unwrap();
    assert_eq!(chapters[0]["start_display"], "00:00:00.000");
    assert_eq!(chapters[0]["title_hint"], "Hello pricing team");
    assert_eq!(chapters[0]["speakers"], serde_json::json!(["S1"]));
    assert_eq!(chapters[1]["start"], 6_000_000);
    let second_speakers = chapters[1]["speakers"].as_array().unwrap();
    assert!(second_speakers.iter().any(|speaker| speaker == "S2"));
    assert!(second_speakers.iter().any(|speaker| speaker == "S1"));
}

#[test]
fn transcript_is_untruncated_and_carries_the_speaker() {
    let path = fixture_path();
    let json = query_json(&["query", "transcript", path.to_str().unwrap(), "--json"]);
    assert_eq!(json["count"], 3);
    let segments = json["segments"].as_array().unwrap();
    assert_eq!(segments[0]["speaker"], "S1");
    assert_eq!(segments[0]["text"], "Hello pricing team");
    assert_eq!(segments[1]["speaker"], "S2");
    // Text is never truncated in JSON or in the transcript table.
    assert_eq!(segments[2]["text"], "closing remarks about pricing");

    let windowed = query_json(&[
        "query",
        "transcript",
        path.to_str().unwrap(),
        "--from",
        "6s",
        "--to",
        "9s",
        "--json",
    ]);
    assert_eq!(windowed["count"], 1);
    assert_eq!(windowed["segments"][0]["text"], "second part");
}

#[test]
fn words_are_available_per_segment_and_turns_point_at_their_parent() {
    let path = fixture_path();
    let doc = path.to_str().unwrap();

    let json = query_json(&["query", "words", doc, "0", "--json"]);
    assert_eq!(json["segment"], 0);
    assert_eq!(json["speaker"], "S1");
    let words = json["words"].as_array().unwrap();
    assert_eq!(words[1]["text"], "pricing");
    assert_eq!(words[1]["start"], 1_000_000);
    assert_eq!(words[1]["end"], 1_600_000);

    // Segment 1 is a speaker turn: the words live in parent segment 0.
    let output = query(&["query", "words", doc, "1"]);
    assert_eq!(output.status.code(), Some(1));
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("parent segment 0"), "got: {message}");
}

#[test]
fn human_output_is_plain_and_readable() {
    let path = fixture_path();
    let output = query(&["query", "timeline", path.to_str().unwrap()]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("S1"), "speaker column: {text}");
    assert!(text.contains("Hello pricing team"), "text column: {text}");
    assert!(text.contains("00:00:00.000"), "display range: {text}");
}

#[test]
fn verify_accepts_a_quote_that_also_appears_elsewhere() {
    // The same sentence at 0-1 s and 10-11 s: a claim on the second copy must
    // not be downgraded by the first, non-overlapping match.
    let media = MediaInfo {
        path: "/tmp/query-duplicate.wav".into(),
        duration: Some(MediaTime::from_millis(12_000)),
        ..MediaInfo::default()
    };
    let mut builder = DocumentBuilder::new(media, AnalysisConfig::default());
    let run = builder.add_run(run_record(
        Capability::Transcribe,
        "whisper-base.en",
        RunStatus::Complete,
    ));
    let track = builder.add_track(TrackKind::Speech, "asr", run);
    for (start, end) in [(0.0_f64, 1.0_f64), (10.0, 11.0)] {
        builder.add_segment(
            track,
            TimeRange::from_secs(start, end),
            SegmentKind::Speech {
                text: "same words".into(),
                words: Vec::new(),
                language: Some("en".into()),
                no_speech_prob: None,
            },
            Confidence::new(0.9),
            run,
        );
    }
    let dir = std::env::temp_dir().join(format!("sonolytica-verify-dup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let doc = dir.join("doc.json");
    let document = builder.finish().expect("duplicate fixture must be valid");
    std::fs::write(&doc, document.to_json_pretty().unwrap()).unwrap();
    let claims = dir.join("claims.json");
    std::fs::write(
        &claims,
        r#"{"claims":[{"quote":"same words","start_us":10000000,"end_us":11000000}]}"#,
    )
    .unwrap();

    let output = query(&[
        "query",
        "verify",
        doc.to_str().unwrap(),
        "--claims",
        claims.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        output.status.success(),
        "second occurrence must verify: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["claims"][0]["status"], "ok");
    assert_eq!(json["claims"][0]["matches"].as_array().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_checks_quotes_and_ranges() {
    let path = fixture_path();
    let claims =
        std::env::temp_dir().join(format!("sonolytica-claims-{}.json", std::process::id()));
    std::fs::write(
        &claims,
        r#"{"claims":[
            {"quote":"Hello pricing team","start_us":0,"end_us":6000000},
            {"quote":"Hello pricing team","start_us":10000000,"end_us":11000000},
            {"quote":"the budget was cut","start_us":0,"end_us":1000000}
        ]}"#,
    )
    .unwrap();

    let output = query(&[
        "query",
        "verify",
        path.to_str().unwrap(),
        "--claims",
        claims.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a failed claim must exit 1: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("verify emits JSON");
    assert_eq!(json["ok"], false);
    assert_eq!(json["failed"], 2);
    assert_eq!(json["claims"][0]["status"], "ok");
    assert_eq!(json["claims"][1]["status"], "range_mismatch");
    assert_eq!(json["claims"][2]["status"], "quote_not_found");
    assert_eq!(
        json["claims"][1]["matches"][0]["start_us"], 0,
        "the report shows where the quote actually is"
    );

    std::fs::write(
        &claims,
        r#"{"claims":[{"quote":"Hello pricing team","start_us":0,"end_us":6000000}]}"#,
    )
    .unwrap();
    let output = query(&[
        "query",
        "verify",
        path.to_str().unwrap(),
        "--claims",
        claims.to_str().unwrap(),
    ]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 claims, 0 failed"));
}

#[test]
fn bad_input_exits_one() {
    let path = fixture_path();
    let doc = path.to_str().unwrap();

    for args in [
        vec!["query", "timeline", doc, "--from", "not-a-time"],
        vec!["query", "timeline", doc, "--from", "10s", "--to", "5s"],
        vec!["query", "timeline", doc, "--type", "nonsense"],
        // `-3s` would be read as a flag, so pass it as a value explicitly.
        vec!["query", "at", doc, "--", "-3s"],
        vec!["query", "summary", "/nonexistent/document.json"],
    ] {
        let output = query(&args);
        assert_eq!(
            output.status.code(),
            Some(1),
            "query {args:?} should exit 1, stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn empty_fixture_path() -> PathBuf {
    let media = MediaInfo {
        path: "/tmp/query-empty.wav".into(),
        duration: Some(MediaTime::from_secs_f64(5.0)),
        ..MediaInfo::default()
    };
    let document = DocumentBuilder::new(media, AnalysisConfig::default())
        .finish()
        .expect("empty document must be valid");
    let path = std::env::temp_dir().join(format!(
        "sonolytica-query-empty-{}.json",
        std::process::id()
    ));
    std::fs::write(&path, document.to_json_pretty().unwrap()).expect("write fixture");
    path
}

#[test]
fn brief_json_carries_every_section_and_stays_compact() {
    let path = fixture_path();
    let json = query_json(&["query", "brief", path.to_str().unwrap(), "--json"]);

    assert_eq!(json["media"]["duration_us"], 15_000_000);
    assert_eq!(json["media"]["duration_display"], "00:00:15.000");
    assert_eq!(json["media"]["has_video"], false);
    assert_eq!(json["complete"], false);

    // No `speech` analysis block in the fixture: the counts are derived from
    // the segments, and the ratio stays null instead of being guessed.
    assert_eq!(json["speech"]["word_count"], 5);
    assert_eq!(json["speech"]["speaker_count"], 2);
    assert_eq!(json["speech"]["language"], "en");
    assert!(json["speech"]["speech_ratio"].is_null());

    assert_eq!(json["speaker_count"], 2);
    assert_eq!(json["speakers"][0]["id"], "S1");
    assert_eq!(json["speakers"][0]["speech_duration_us"], 12_000_000);
    assert_eq!(json["speakers"][1]["id"], "S2");

    assert!(json["chapter_count"].as_u64().unwrap() >= 1);
    assert_eq!(
        json["chapters"].as_array().unwrap().len(),
        json["chapter_count"].as_u64().unwrap().min(5) as usize
    );

    assert_eq!(json["video"]["scenes"], 0);
    assert_eq!(json["video"]["keyframes"], 0);
    assert_eq!(json["music"]["bpm"], 98.0);
    assert_eq!(json["music"]["key"], "F# minor");
    assert_eq!(json["music"]["section_count"], 2);
    assert_eq!(json["acoustic"]["silence_count"], 1);
    assert_eq!(json["acoustic"]["peak_dbfs"], -1.0);

    let bytes = serde_json::to_string(&json).unwrap();
    assert!(
        bytes.len() <= 4096,
        "brief JSON must stay under 4 KB, got {} bytes",
        bytes.len()
    );
}

#[test]
fn brief_text_is_one_compact_screen() {
    let path = fixture_path();
    let output = query(&["query", "brief", path.to_str().unwrap()]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    for needle in [
        "file",
        "speech",
        "S1",
        "chapters",
        "music",
        "98.0 BPM",
        "warnings",
        "provider_unavailable",
    ] {
        assert!(text.contains(needle), "missing `{needle}` in:\n{text}");
    }
    assert!(
        text.lines().count() <= 40,
        "brief must fit one screen, got {} lines",
        text.lines().count()
    );
}

#[test]
fn brief_survives_a_document_without_speech_or_video() {
    let path = empty_fixture_path();
    let doc = path.to_str().unwrap();
    let json = query_json(&["query", "brief", doc, "--json"]);
    assert!(json["speech"].is_null());
    assert_eq!(json["speaker_count"], 0);
    assert_eq!(json["speakers"].as_array().unwrap().len(), 0);
    assert_eq!(json["video"]["scenes"], 0);
    assert!(json["chapters"].is_array());

    let output = query(&["query", "brief", doc]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("speech     (none)"), "{text}");
}

/// A document with more scenes and OCR lines than `brief` shows, and two OCR
/// lines at every keyframe instant.
fn video_fixture_path() -> PathBuf {
    let media = MediaInfo {
        path: "/tmp/query-video.mp4".into(),
        duration: Some(MediaTime::from_millis(120_000)),
        ..MediaInfo::default()
    };
    let mut builder = DocumentBuilder::new(media, AnalysisConfig::default());

    let scene_run = builder.add_run(run_record(
        Capability::Scene,
        "ffmpeg-histogram",
        RunStatus::Complete,
    ));
    let scene_track = builder.add_track(TrackKind::Scene, "scenes", scene_run);
    for index in 0..55_u32 {
        builder.add_segment(
            scene_track,
            TimeRange::from_secs(f64::from(index) * 2.0, f64::from(index) * 2.0 + 2.0),
            SegmentKind::Scene {
                index,
                cut: index > 0,
            },
            Confidence::new(0.9),
            scene_run,
        );
    }

    let object_run = builder.add_run(run_record(
        Capability::Vision,
        "yolos-tiny",
        RunStatus::Complete,
    ));
    let object_track = builder.add_track(TrackKind::Object, "objects", object_run);
    builder.add_segment(
        object_track,
        TimeRange::empty(MediaTime::from_secs_f64(2.0)),
        SegmentKind::Object {
            class: "laptop".into(),
            bbox: Some(BBox {
                x: 1.0,
                y: 2.0,
                width: 30.0,
                height: 20.0,
            }),
        },
        Confidence::new(0.9),
        object_run,
    );

    let text_run = builder.add_run(run_record(
        Capability::Vision,
        "pp-ocrv5",
        RunStatus::Complete,
    ));
    let text_track = builder.add_track(TrackKind::Text, "ocr", text_run);
    for index in 0..12_u32 {
        let instant = MediaTime::from_secs_f64(2.0 + f64::from(index) * 5.0);
        for text in ["FIRST LINE", "SECOND LINE"] {
            builder.add_segment(
                text_track,
                TimeRange::empty(instant),
                SegmentKind::TextOverlay {
                    text: text.into(),
                    bbox: BBox {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 20.0,
                    },
                },
                Confidence::new(0.9),
                text_run,
            );
        }
    }

    let path = std::env::temp_dir().join(format!(
        "sonolytica-query-video-{}.json",
        std::process::id()
    ));
    let document = builder.finish().expect("video fixture must be valid");
    std::fs::write(&path, document.to_json_pretty().unwrap()).expect("write fixture");
    path
}

#[test]
fn brief_maps_overlap_spans() {
    let media = MediaInfo {
        path: "/tmp/query-overlap.wav".into(),
        duration: Some(MediaTime::from_millis(10_000)),
        ..MediaInfo::default()
    };
    let mut builder = DocumentBuilder::new(media, AnalysisConfig::default());
    let run = builder.add_run(run_record(
        Capability::Diarize,
        "sortformer-4spk-v2.1",
        RunStatus::Complete,
    ));
    let track = builder.add_track(TrackKind::SpeakerTurn, "speaker_turns", run);
    for (start, end, id, overlap) in [
        (1.0, 4.0, "S1", true),
        (2.5, 5.0, "S2", true),
        (6.0, 7.0, "S1", false),
    ] {
        builder.add_segment(
            track,
            TimeRange::from_secs(start, end),
            SegmentKind::SpeakerTurn {
                speaker: SpeakerId::new(id),
                text: "x".into(),
                overlap,
            },
            Confidence::new(0.9),
            run,
        );
    }
    let path = std::env::temp_dir().join(format!(
        "sonolytica-query-overlap-{}.json",
        std::process::id()
    ));
    let document = builder.finish().expect("overlap fixture must be valid");
    std::fs::write(&path, document.to_json_pretty().unwrap()).expect("write fixture");

    let json = query_json(&["query", "brief", path.to_str().unwrap(), "--json"]);
    let overlaps = json["speech"]["overlaps"].as_array().unwrap();
    assert_eq!(overlaps.len(), 1, "touching turns merge into one span");
    assert_eq!(overlaps[0]["start_us"], 1_000_000);
    assert_eq!(overlaps[0]["end_us"], 5_000_000);
    assert_eq!(overlaps[0]["speakers"], serde_json::json!(["S1", "S2"]));

    let text = String::from_utf8_lossy(&query(&["query", "brief", path.to_str().unwrap()]).stdout)
        .into_owned();
    assert!(text.contains("overlaps   1 total"), "{text}");
    assert!(text.contains("00:00:01.000–00:00:05.000  S1,S2"), "{text}");
}

#[test]
fn brief_maps_scene_cuts_and_one_ocr_line_per_keyframe() {
    let path = video_fixture_path();
    let json = query_json(&["query", "brief", path.to_str().unwrap(), "--json"]);

    assert_eq!(json["video"]["scenes"], 55);
    let cuts = json["video"]["scene_cuts"].as_array().unwrap();
    assert_eq!(cuts.len(), 50, "cuts are capped at 50");
    assert_eq!(cuts[0], 2_000_000, "first cut is scene 1 at 2 s");

    let ocr = json["video"]["ocr"].as_array().unwrap();
    assert_eq!(ocr.len(), 10, "OCR is capped at ten keyframes");
    assert_eq!(ocr[0]["start_us"], 2_000_000);
    assert_eq!(ocr[0]["text"], "FIRST LINE", "one line per instant");

    let bytes = serde_json::to_string(&json).unwrap();
    assert!(
        bytes.len() <= 4096,
        "brief JSON grew to {} bytes",
        bytes.len()
    );

    let text = String::from_utf8_lossy(&query(&["query", "brief", path.to_str().unwrap()]).stdout)
        .into_owned();
    assert!(text.contains("cuts       00:00:02.000"), "{text}");
    assert!(
        text.contains("ocr        00:00:02.000  FIRST LINE"),
        "{text}"
    );
}
