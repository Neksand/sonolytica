//! Metadata extraction through `ffprobe -print_format json`.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use sonolytica_core::{Limits, MediaInfo, MediaTime, StreamInfo, StreamKind};

use crate::error::MediaError;
use crate::tools::MediaTools;

/// Probe container and stream metadata.
pub async fn probe(
    tools: &MediaTools,
    path: &Path,
    limits: &Limits,
) -> Result<MediaInfo, MediaError> {
    let timeout = Duration::from_secs(limits.timeout_secs.clamp(5, 3_600));
    let output = tokio::time::timeout(
        timeout,
        tokio::process::Command::new(&tools.ffprobe)
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
                "-protocol_whitelist",
                "file,pipe",
                "-i",
            ])
            .arg(path)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| MediaError::Timeout(timeout))?
    .map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => MediaError::ToolsNotFound,
        _ => MediaError::Io(e),
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MediaError::ProbeFailed(
            stderr
                .trim()
                .lines()
                .next()
                .unwrap_or("unknown error")
                .to_string(),
        ));
    }

    let parsed: ProbeOutput = serde_json::from_slice(&output.stdout)
        .map_err(|e| MediaError::Parse(format!("ffprobe json: {e}")))?;
    Ok(map_probe(parsed, path))
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    format: Option<Format>,
    #[serde(default)]
    streams: Vec<Stream>,
}

#[derive(Debug, Deserialize)]
struct Format {
    format_name: Option<String>,
    duration: Option<String>,
    size: Option<String>,
    bit_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Stream {
    index: i64,
    codec_type: Option<String>,
    codec_name: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    duration: Option<String>,
    bit_rate: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

fn parse_f64(value: &Option<String>) -> Option<f64> {
    value
        .as_deref()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
}

fn parse_u64(value: &Option<String>) -> Option<u64> {
    value.as_deref().and_then(|v| v.trim().parse::<u64>().ok())
}

fn parse_u32(value: &Option<String>) -> Option<u32> {
    value.as_deref().and_then(|v| v.trim().parse::<u32>().ok())
}

fn parse_rate(stream: &Stream) -> Option<f64> {
    for candidate in [&stream.avg_frame_rate, &stream.r_frame_rate] {
        let Some(raw) = candidate.as_deref() else {
            continue;
        };
        if let Some((num, den)) = raw.split_once('/') {
            let (num, den) = (num.parse::<f64>().ok()?, den.parse::<f64>().ok()?);
            if den > 0.0 && num > 0.0 {
                return Some(num / den);
            }
        } else if let Ok(v) = raw.parse::<f64>() {
            if v > 0.0 {
                return Some(v);
            }
        }
    }
    None
}

fn kind_of(codec_type: Option<&str>) -> StreamKind {
    match codec_type {
        Some("audio") => StreamKind::Audio,
        Some("video") => StreamKind::Video,
        Some("subtitle") => StreamKind::Subtitle,
        Some("data") => StreamKind::Data,
        Some("attachment") => StreamKind::Attachment,
        _ => StreamKind::Unknown,
    }
}

fn map_probe(parsed: ProbeOutput, path: &Path) -> MediaInfo {
    let streams: Vec<StreamInfo> = parsed
        .streams
        .iter()
        .map(|s| StreamInfo {
            index: u32::try_from(s.index).unwrap_or(0),
            kind: kind_of(s.codec_type.as_deref()),
            codec: s.codec_name.clone(),
            sample_rate: parse_u32(&s.sample_rate),
            channels: s.channels.and_then(|c| u16::try_from(c).ok()),
            width: s.width,
            height: s.height,
            frame_rate: parse_rate(s),
            language: s.tags.get("language").cloned(),
            duration: parse_f64(&s.duration).map(MediaTime::from_secs_f64),
            bit_rate: parse_u64(&s.bit_rate),
        })
        .collect();

    let format_duration = parsed
        .format
        .as_ref()
        .and_then(|f| parse_f64(&f.duration))
        .map(MediaTime::from_secs_f64);
    let stream_duration = streams.iter().filter_map(|s| s.duration).max();
    let duration = format_duration.or(stream_duration);

    MediaInfo {
        path: path.display().to_string(),
        sha256: None,
        size_bytes: parsed.format.as_ref().and_then(|f| parse_u64(&f.size)),
        container: parsed.format.as_ref().and_then(|f| {
            f.format_name
                .as_deref()
                .and_then(|n| n.split(',').next())
                .map(str::to_string)
        }),
        duration,
        bit_rate: parsed.format.as_ref().and_then(|f| parse_u64(&f.bit_rate)),
        streams,
    }
}
