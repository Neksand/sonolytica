//! Human-facing HTML report over a document.
//!
//! One file, no network, no JavaScript: chapters, speakers, quotes with ranges,
//! keyframes and OCR. Keyframe images are copied next to the report into
//! `<report>.artifacts/frames/` so the folder can be moved as a unit.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use sonolytica_core::{AnalysisBlock, Document, MediaTime, SegmentKind};

use crate::query::{chapter_views, load, speaker_views};

/// Same 1.2 s that `query chapters` uses by default.
const CHAPTER_GAP: MediaTime = MediaTime::from_millis(1_200);

#[derive(Debug, Default)]
pub struct ReportOptions {
    /// Where to write the HTML; defaults to the document path with `.html`.
    pub out: Option<PathBuf>,
}

/// Write the report and return its path.
pub fn run(document_path: &Path, options: &ReportOptions) -> Result<PathBuf> {
    let document = load(document_path)?;
    let out = options
        .out
        .clone()
        .unwrap_or_else(|| document_path.with_extension("html"));
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    let out_name = out
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "report.html".into());
    let assets = format!("{out_name}.artifacts");
    let frames = copy_keyframes(&document, document_path, out.parent(), &assets)?;
    let html = render(&document, &frames);
    std::fs::write(&out, html).with_context(|| format!("could not write {}", out.display()))?;
    Ok(out)
}

/// Copy keyframes next to the report and return `(artifact index, relative
/// source)` pairs in document order.
fn copy_keyframes(
    document: &Document,
    document_path: &Path,
    out_dir: Option<&Path>,
    assets: &str,
) -> Result<Vec<(usize, String)>> {
    let base = document_path.parent().unwrap_or_else(|| Path::new("."));
    let out_dir = out_dir.unwrap_or_else(|| Path::new("."));
    let dir = out_dir.join(assets).join("frames");
    let mut copied = Vec::new();
    for (index, artifact) in document.artifacts.iter().enumerate() {
        if artifact.kind != "keyframe" {
            continue;
        }
        let Some(relative) = safe_relative(&artifact.path) else {
            continue;
        };
        let source = base.join(relative);
        let Some(name) = source.file_name() else {
            continue;
        };
        // Two scenes can carry frames with the same basename; the index keeps
        // them apart and makes the copy order reproducible.
        let unique = format!("{index:05}-{}", name.to_string_lossy());
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
        let target = dir.join(&unique);
        match std::fs::copy(&source, &target) {
            Ok(_) => copied.push((index, format!("{assets}/frames/{unique}"))),
            Err(error) => eprintln!("warning: keyframe {} not copied: {error}", source.display()),
        }
    }
    Ok(copied)
}

/// Artifact paths are relative to the document. Refuse absolute paths and
/// `..` so a crafted document cannot copy files from outside its directory.
fn safe_relative(path: &str) -> Option<&Path> {
    let path = Path::new(path);
    if path.is_absolute() {
        return None;
    }
    let ok = path
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    ok.then_some(path)
}

fn render(document: &Document, frames: &[(usize, String)]) -> String {
    let mut out = String::with_capacity(64 * 1024);
    let title = Path::new(&document.media.path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "media".into());
    let duration = document.duration();
    let complete = document.is_complete();

    let _ = write!(
        out,
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — SONOLYTICA</title>
<style>{css}</style>
</head><body>
<header>
  <p class="eyebrow">SONOLYTICA report</p>
  <h1>{title}</h1>
  <p class="meta">{duration} · {kind} · <span class="{status_class}">{status}</span></p>
  <p class="meta mono">sha256 {sha}</p>
</header>
<main>
"#,
        css = CSS,
        title = esc(&title),
        duration = duration,
        kind = if document.media.has_video() {
            "video"
        } else {
            "audio"
        },
        status_class = if complete { "ok" } else { "warn" },
        status = if complete { "complete" } else { "not complete" },
        sha = esc(document.media.sha256.as_deref().unwrap_or("—")),
    );

    if !document.warnings.is_empty() {
        out.push_str("<section class=\"warnings\"><h2>Warnings</h2><ul>\n");
        for warning in &document.warnings {
            let _ = writeln!(
                out,
                "<li><code>{}</code> {}</li>",
                esc(&warning.code),
                esc(&warning.message)
            );
        }
        out.push_str("</ul></section>\n");
    }

    summary(&mut out, document);
    speakers(&mut out, document);
    chapters(&mut out, document);
    frames_section(&mut out, document, frames);
    transcript(&mut out, document);
    music_and_acoustic(&mut out, document);
    runs(&mut out, document);

    let _ = write!(
        out,
        "</main>\n<footer>Generated by SONOLYTICA {} · every claim above is a document fact with a range.</footer>\n</body></html>\n",
        env!("CARGO_PKG_VERSION")
    );
    out
}

fn summary(out: &mut String, document: &Document) {
    let turns = document.dialogue();
    let speech = document.analyses.iter().find_map(|block| match block {
        AnalysisBlock::Speech(summary) => Some(summary),
        _ => None,
    });
    out.push_str("<section><h2>At a glance</h2><div class=\"grid\">\n");
    let _ = writeln!(
        out,
        "<div><span>duration</span><strong>{}</strong></div>",
        document.duration()
    );
    let _ = writeln!(
        out,
        "<div><span>speakers</span><strong>{}</strong></div>",
        document.speakers().len()
    );
    let _ = writeln!(
        out,
        "<div><span>turns</span><strong>{}</strong></div>",
        turns.len()
    );
    let words = speech.map_or(0, |speech| speech.word_count);
    let _ = writeln!(out, "<div><span>words</span><strong>{words}</strong></div>");
    let _ = writeln!(
        out,
        "<div><span>language</span><strong>{}</strong></div>",
        esc(speech
            .and_then(|speech| speech.language.as_deref())
            .unwrap_or("—"))
    );
    let _ = writeln!(
        out,
        "<div><span>speech</span><strong>{}</strong></div>",
        speech.map_or_else(
            || "—".to_string(),
            |speech| format!("{:.0}%", speech.speech_ratio * 100.0)
        )
    );
    out.push_str("</div></section>\n");
}

fn speakers(out: &mut String, document: &Document) {
    let views = speaker_views(document);
    if views.is_empty() {
        return;
    }
    out.push_str("<section><h2>Speakers</h2><table><thead><tr><th>speaker</th><th>speech</th><th>turns</th><th>from</th><th>to</th></tr></thead><tbody>\n");
    for view in views {
        let _ = writeln!(
            out,
            "<tr><td><strong>{}</strong></td><td>{}</td><td>{}</td><td class=\"mono\">{}</td><td class=\"mono\">{}</td></tr>",
            esc(view.id),
            view.speech_display,
            view.turn_count,
            MediaTime::from_micros(view.first_start_us),
            MediaTime::from_micros(view.last_end_us),
        );
    }
    out.push_str("</tbody></table><p class=\"note\">Labels are local to this document, not people.</p></section>\n");
}

fn chapters(out: &mut String, document: &Document) {
    let views = chapter_views(document, CHAPTER_GAP);
    if views.is_empty() {
        return;
    }
    out.push_str("<section><h2>Chapters</h2><p class=\"note\">Structural boundaries (silence, speaker change, cut), not topics.</p><ol class=\"chapters\">\n");
    for view in views {
        let _ = writeln!(
            out,
            "<li><span class=\"mono\">{}–{}</span> <strong>{}</strong> {}</li>",
            esc(&view.start_display),
            esc(&view.end_display),
            esc(view.title_hint.as_deref().unwrap_or("")),
            if view.speakers.is_empty() {
                String::new()
            } else {
                format!(
                    "<span class=\"tag\">{}</span>",
                    esc(&view.speakers.join(", "))
                )
            }
        );
    }
    out.push_str("</ol></section>\n");
}

fn frames_section(out: &mut String, document: &Document, frames: &[(usize, String)]) {
    let overlays: Vec<(i64, String)> = document
        .segments_of_kind("text_overlay")
        .filter_map(|segment| match &segment.kind {
            SegmentKind::TextOverlay { text, .. } => {
                Some((segment.range.start.micros(), text.clone()))
            }
            _ => None,
        })
        .collect();
    let faces: Vec<i64> = document
        .segments_of_kind("object")
        .filter(
            |segment| matches!(&segment.kind, SegmentKind::Object { class, .. } if class == "face"),
        )
        .map(|segment| segment.range.start.micros())
        .collect();
    let captions: Vec<(i64, String)> = document
        .segments_of_kind("caption")
        .filter_map(|segment| match &segment.kind {
            SegmentKind::Caption { text } => Some((segment.range.start.micros(), text.clone())),
            _ => None,
        })
        .collect();
    let poses: Vec<i64> = document
        .segments_of_kind("pose")
        .map(|segment| segment.range.start.micros())
        .collect();
    let objects: Vec<(i64, String)> = document
        .segments_of_kind("object")
        .filter_map(|segment| match &segment.kind {
            SegmentKind::Object { class, .. } if class != "face" => {
                Some((segment.range.start.micros(), class.clone()))
            }
            _ => None,
        })
        .collect();
    if frames.is_empty() && overlays.is_empty() {
        return;
    }
    out.push_str("<section><h2>Frames and on-screen text</h2><div class=\"frames\">\n");
    for (index, source) in frames {
        let Some(artifact) = document.artifacts.get(*index) else {
            continue;
        };
        let at = artifact.range.start.micros();
        let _ = write!(
            out,
            "<figure><img src=\"{}\" alt=\"keyframe at {}\" loading=\"lazy\"><figcaption class=\"mono\">{}</figcaption>",
            esc(source),
            MediaTime::from_micros(at),
            MediaTime::from_micros(at)
        );
        for (overlay_at, text) in &overlays {
            if *overlay_at / 1000 == at / 1000 {
                let _ = write!(out, "<p class=\"ocr\">{}</p>", esc(text));
            }
        }
        let mut classes: Vec<&str> = objects
            .iter()
            .filter(|(object_at, _)| *object_at / 1000 == at / 1000)
            .map(|(_, class)| class.as_str())
            .collect();
        classes.sort_unstable();
        classes.dedup();
        if !classes.is_empty() {
            let _ = write!(
                out,
                "<p class=\"note\">objects: {}</p>",
                esc(&classes.join(", "))
            );
        }
        let face_count = faces
            .iter()
            .filter(|face_at| **face_at / 1000 == at / 1000)
            .count();
        if face_count > 0 {
            let _ = write!(
                out,
                "<p class=\"note\">{} detected</p>",
                if face_count == 1 {
                    "1 face".to_string()
                } else {
                    format!("{face_count} faces")
                }
            );
        }
        for (caption_at, text) in &captions {
            if *caption_at / 1000 == at / 1000 {
                let _ = write!(out, "<p class=\"caption\">{}</p>", esc(text));
            }
        }
        let pose_count = poses
            .iter()
            .filter(|pose_at| **pose_at / 1000 == at / 1000)
            .count();
        if pose_count > 0 {
            let _ = write!(
                out,
                "<p class=\"note\">{} with pose</p>",
                if pose_count == 1 {
                    "1 person".to_string()
                } else {
                    format!("{pose_count} people")
                }
            );
        }
        out.push_str("</figure>\n");
    }
    out.push_str("</div></section>\n");
}

fn transcript(out: &mut String, document: &Document) {
    let turns = document.dialogue();
    if turns.is_empty() {
        return;
    }
    out.push_str("<section><h2>Transcript</h2><p class=\"note\">Quotes are the ASR text; ranges are document facts.</p>\n");
    for turn in turns {
        let overlap = if turn.overlap {
            " <span class=\"tag\">overlap</span>"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "<div class=\"turn\"><div class=\"turn-head mono\"><span>{}</span>–<span>{}</span> <strong>{}</strong>{overlap}</div><p>{}</p></div>",
            turn.range.start,
            turn.range.end,
            esc(turn.speaker.as_str()),
            esc(&turn.text)
        );
    }
    out.push_str("</section>\n");
}

fn music_and_acoustic(out: &mut String, document: &Document) {
    let music = document.analyses.iter().find_map(|block| match block {
        AnalysisBlock::Music(summary) => Some(summary),
        _ => None,
    });
    let acoustic = document.analyses.iter().find_map(|block| match block {
        AnalysisBlock::Acoustic(summary) => Some(summary),
        _ => None,
    });
    if music.is_none() && acoustic.is_none() {
        return;
    }
    out.push_str("<section><h2>Music and acoustics</h2><div class=\"grid\">\n");
    if let Some(music) = music {
        let _ = writeln!(
            out,
            "<div><span>tempo</span><strong>{}</strong></div>",
            music
                .bpm
                .as_ref()
                .map_or_else(|| "—".to_string(), |bpm| format!("{:.1} BPM", bpm.value))
        );
        let _ = writeln!(
            out,
            "<div><span>sections</span><strong>{}</strong></div>",
            music.sections.len()
        );
        let _ = writeln!(
            out,
            "<div><span>chords</span><strong>{}</strong></div>",
            music.chords.len()
        );
    }
    if let Some(acoustic) = acoustic {
        let _ = writeln!(
            out,
            "<div><span>silence regions</span><strong>{}</strong></div>",
            acoustic.silence.len()
        );
        let _ = writeln!(
            out,
            "<div><span>peak</span><strong>{}</strong></div>",
            acoustic
                .peak_dbfs
                .map_or_else(|| "—".to_string(), |peak| format!("{peak:.1} dBFS"))
        );
    }
    out.push_str("</div></section>\n");
}

fn runs(out: &mut String, document: &Document) {
    if document.runs.is_empty() {
        return;
    }
    out.push_str("<section><h2>Runs</h2><table><thead><tr><th>capability</th><th>provider</th><th>status</th><th>ms</th></tr></thead><tbody>\n");
    for run in &document.runs {
        let (status, reason) = match &run.status {
            sonolytica_core::RunStatus::Complete => ("complete".to_string(), None),
            sonolytica_core::RunStatus::Partial { reason } => {
                ("partial".to_string(), Some(reason.clone()))
            }
            sonolytica_core::RunStatus::Failed { reason } => {
                ("failed".to_string(), Some(reason.clone()))
            }
        };
        let _ = writeln!(
            out,
            "<tr><td>{}</td><td>{}</td><td>{}{}</td><td>{}</td></tr>",
            esc(run.capability.as_str()),
            esc(&run.provider),
            esc(&status),
            reason.map_or_else(String::new, |reason| format!(" — {}", esc(&reason))),
            run.duration_ms
        );
    }
    out.push_str("</tbody></table></section>\n");
}

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

const CSS: &str = r#"
:root{--bg:#0f1115;--panel:#171a21;--text:#e7e9ee;--muted:#98a2b3;--line:#262b36;--accent:#7dd3fc;--warn:#fbbf24;--ok:#86efac}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--text);font:15px/1.55 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif}
header,main,footer{max-width:960px;margin:0 auto;padding:0 24px}
header{padding-top:40px}
.eyebrow{letter-spacing:.14em;text-transform:uppercase;color:var(--accent);font-size:12px;margin:0 0 8px}
h1{font-size:32px;line-height:1.2;margin:0 0 10px;overflow-wrap:anywhere}
h2{font-size:18px;margin:0 0 12px}
.meta{color:var(--muted);margin:4px 0}
.mono{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:13px}
.ok{color:var(--ok)}.warn{color:var(--warn)}
section{background:var(--panel);border:1px solid var(--line);border-radius:14px;padding:20px;margin:20px 0}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:12px}
.grid div{background:#11141a;border:1px solid var(--line);border-radius:10px;padding:12px}
.grid span{display:block;color:var(--muted);font-size:12px;text-transform:uppercase;letter-spacing:.08em}
.grid strong{font-size:20px}
table{width:100%;border-collapse:collapse;font-size:14px}
th,td{text-align:left;padding:8px 6px;border-bottom:1px solid var(--line)}
th{color:var(--muted);font-weight:600;font-size:12px;text-transform:uppercase;letter-spacing:.06em}
.note{color:var(--muted);font-size:13px;margin:10px 0 0}
.chapters{padding-left:20px}
.chapters li{margin:6px 0}
.frames{display:grid;grid-template-columns:repeat(auto-fill,minmax(240px,1fr));gap:14px}
figure{margin:0;background:#11141a;border:1px solid var(--line);border-radius:10px;overflow:hidden}
figure img{display:block;width:100%;height:auto}
figcaption{padding:8px 10px 0;color:var(--muted)}
.ocr{margin:6px 10px 10px;font-size:13px}
.caption{margin:6px 10px 10px;font-size:13px;color:var(--muted);font-style:italic}
.turn{padding:10px 0;border-bottom:1px solid var(--line)}
.turn:last-child{border-bottom:0}
.turn p{margin:4px 0 0}
.turn-head{color:var(--muted)}
.tag{background:#3b2f0b;color:var(--warn);border-radius:6px;padding:0 6px;font-size:11px}
.warnings{border-color:#4a3a12}
footer{color:var(--muted);font-size:13px;padding:10px 24px 48px}
"#;
