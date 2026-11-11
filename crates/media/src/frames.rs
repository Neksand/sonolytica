//! Video frame signatures and shot detection through the FFmpeg pipe.
//!
//! The point is to navigate a video cheaply, not to understand it. FFmpeg
//! decodes at a low frame rate into small grayscale frames; each frame becomes
//! a perceptual hash and a luma histogram; a cut is a spike in the difference
//! between consecutive signatures. No image-decoding dependency, no model.

use std::path::Path;
use std::time::Duration;

use tokio::io::AsyncReadExt;

use crate::error::MediaError;
use crate::tools::MediaTools;

/// Signature frame size. Small on purpose: 64x36 RGB is 6.9 KB per frame, so
/// two hours of video at 2 fps is ~100 MB of pipe traffic.
pub const FRAME_WIDTH: usize = 64;
pub const FRAME_HEIGHT: usize = 36;
const FRAME_BYTES: usize = FRAME_WIDTH * FRAME_HEIGHT * 3;
/// Chromaticity histogram: 4 levels per channel after normalising by the
/// brightest channel, 4^3 bins. Raw RGB would collapse every dark scene into
/// one bin; luma alone misses a cut between equal-brightness, different-hue
/// shots.
const HIST_BINS: usize = 64;

/// A cut must be at least this far from the previous one.
const MIN_SCENE_S: f32 = 0.5;
/// Below this combined score nothing is a cut, even in a perfectly static file.
const MIN_CUT_SCORE: f32 = 0.25;

#[derive(Debug, Clone, Copy)]
pub struct FrameOptions {
    /// Frames per second to sample. 2 is enough to find cuts; 1 misses fast
    /// editing, 5 costs 2.5x more decode for little gain.
    pub fps: f32,
    pub max_duration_secs: u64,
    pub timeout_secs: u64,
}

impl Default for FrameOptions {
    fn default() -> Self {
        Self {
            fps: 2.0,
            max_duration_secs: 7_200,
            timeout_secs: 900,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FrameSignature {
    /// Position on the sampled grid, not the source frame index.
    pub index: u64,
    pub time_s: f32,
    pub dhash: u64,
    pub hist: [f32; HIST_BINS],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    /// Start of the new shot on the media timeline.
    pub start_s: f32,
    pub score: f32,
}

/// Sample a video stream into frame signatures.
///
/// Returns [`MediaError::NoVideoStream`] when the input has no video stream;
/// callers that already probed the file should never see it.
pub async fn sample_video(
    tools: &MediaTools,
    path: &Path,
    opts: &FrameOptions,
) -> Result<Vec<FrameSignature>, MediaError> {
    if !(opts.fps.is_finite() && opts.fps > 0.0) {
        return Err(MediaError::InvalidInput(
            "frame rate must be a positive number".into(),
        ));
    }
    let timeout = Duration::from_secs(opts.timeout_secs.clamp(5, 3_600));
    let filter = format!(
        "fps={},scale={FRAME_WIDTH}:{FRAME_HEIGHT}:flags=area,format=rgb24",
        opts.fps
    );

    let mut child = tokio::process::Command::new(&tools.ffmpeg)
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(path)
        .args([
            "-map",
            // No `?`: a missing video stream must fail with a message we can
            // turn into `NoVideoStream`, not produce an unconfigured output.
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-vf",
            &filter,
            "-t",
            &opts.max_duration_secs.to_string(),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "pipe:1",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => MediaError::ToolsNotFound,
            _ => MediaError::Io(e),
        })?;

    let mut stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");

    let read_stdout = tokio::spawn(async move {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).await.map(|_| buf)
    });
    let read_stderr = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr.take(64 * 1024).read_to_end(&mut buf).await;
        String::from_utf8_lossy(&buf).into_owned()
    });

    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            let _ = child.kill().await;
            return Err(MediaError::Timeout(timeout));
        }
    };

    let bytes = read_stdout
        .await
        .map_err(|e| MediaError::DecodeFailed(e.to_string()))?
        .map_err(MediaError::Io)?;
    let stderr_text = read_stderr.await.unwrap_or_default();

    if !status.success() && bytes.is_empty() {
        if stderr_text.contains("matches no streams")
            || stderr_text.contains("does not contain any stream")
            || stderr_text.contains("no video")
        {
            return Err(MediaError::NoVideoStream);
        }
        let message = stderr_text
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("ffmpeg produced no video frames")
            .to_string();
        return Err(MediaError::DecodeFailed(message));
    }

    Ok(signatures(&bytes, opts.fps))
}

/// Split a raw RGB stream into per-frame signatures. A trailing partial frame
/// (FFmpeg can stop mid-frame) is ignored rather than treated as data.
fn signatures(bytes: &[u8], fps: f32) -> Vec<FrameSignature> {
    bytes
        .chunks_exact(FRAME_BYTES)
        .enumerate()
        .map(|(index, frame)| {
            let luma = |x: usize, y: usize| -> f32 {
                let offset = (y * FRAME_WIDTH + x) * 3;
                0.299 * f32::from(frame[offset])
                    + 0.587 * f32::from(frame[offset + 1])
                    + 0.114 * f32::from(frame[offset + 2])
            };
            let mut dhash = 0_u64;
            for row in 0..8_usize {
                let y = (row * FRAME_HEIGHT / 8).min(FRAME_HEIGHT - 1);
                let mut previous = luma(0, y);
                for column in 1..9_usize {
                    let x = (column * FRAME_WIDTH / 9).min(FRAME_WIDTH - 1);
                    let value = luma(x, y);
                    dhash <<= 1;
                    if value > previous {
                        dhash |= 1;
                    }
                    previous = value;
                }
            }
            // Chromaticity, not raw RGB: a 4-level bucket on raw values
            // collapses every dark scene (all channels < 64) into bin 0, so
            // two dark slides of different hue looked identical. Normalising
            // by the pixel's brightest channel keeps hue and ignores exposure.
            let mut hist = [0.0_f32; HIST_BINS];
            for pixel in frame.chunks_exact(3) {
                let max = pixel[0].max(pixel[1]).max(pixel[2]).max(1);
                let r = (usize::from(pixel[0]) * 255 / usize::from(max)) >> 6;
                let g = (usize::from(pixel[1]) * 255 / usize::from(max)) >> 6;
                let b = (usize::from(pixel[2]) * 255 / usize::from(max)) >> 6;
                hist[r * 16 + g * 4 + b] += 1.0;
            }
            let total = (FRAME_WIDTH * FRAME_HEIGHT) as f32;
            for bin in &mut hist {
                *bin /= total;
            }
            FrameSignature {
                index: index as u64,
                time_s: index as f32 / fps,
                dhash,
                hist,
            }
        })
        .collect()
}

/// One extracted still image.
#[derive(Debug, Clone)]
pub struct Keyframe {
    pub time_s: f32,
    pub path: std::path::PathBuf,
    pub sha256: String,
}

/// Extract one JPEG per requested instant into `out_dir`.
///
/// Each frame is a separate FFmpeg invocation with `-ss` before the input, so
/// seeking is fast; `-accurate_seek` (the default) still lands on the requested
/// time. A frame that fails to extract is skipped rather than failing the whole
/// batch; an empty result means nothing was decodable.
pub async fn extract_keyframes(
    tools: &MediaTools,
    media: &Path,
    times: &[f32],
    out_dir: &Path,
    width: u32,
) -> Result<Vec<Keyframe>, MediaError> {
    std::fs::create_dir_all(out_dir)?;
    let width = width.clamp(160, 3840);
    let mut frames = Vec::new();
    for (index, time) in times.iter().enumerate() {
        if !time.is_finite() || *time < 0.0 {
            continue;
        }
        let path = out_dir.join(format!("frame_{index:04}.jpg"));
        let filter = format!("scale={width}:-2");
        let mut child = tokio::process::Command::new(&tools.ffmpeg)
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-protocol_whitelist",
                "file,pipe",
                "-ss",
                &format!("{time:.3}"),
                "-i",
            ])
            .arg(media)
            .args(["-frames:v", "1", "-vf", &filter, "-q:v", "3", "-y"])
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => MediaError::ToolsNotFound,
                _ => MediaError::Io(e),
            })?;
        let status = match tokio::time::timeout(Duration::from_secs(30), child.wait()).await {
            Ok(status) => status?,
            Err(_) => {
                let _ = child.start_kill();
                continue;
            }
        };
        if !status.success() {
            continue;
        }
        let sha256 = crate::hash::sha256_file(&path)?;
        frames.push(Keyframe {
            time_s: *time,
            path,
            sha256,
        });
    }
    Ok(frames)
}

/// Detect cuts: a combined hash/histogram distance above `median + 3·MAD`,
/// never below [`MIN_CUT_SCORE`], never closer than [`MIN_SCENE_S`].
pub fn detect_shots(frames: &[FrameSignature]) -> Vec<Shot> {
    if frames.len() < 2 {
        return Vec::new();
    }
    let scores: Vec<f32> = frames
        .windows(2)
        .map(|pair| pair_score(&pair[0], &pair[1]))
        .collect();
    let threshold = (median(&scores) + 3.0 * median_absolute_deviation(&scores)).max(MIN_CUT_SCORE);

    let mut shots = Vec::new();
    let mut last = f32::NEG_INFINITY;
    for (index, score) in scores.iter().enumerate() {
        let start_s = frames[index + 1].time_s;
        if start_s - last <= MIN_SCENE_S {
            continue;
        }
        if *score >= threshold {
            shots.push(Shot {
                start_s,
                score: *score,
            });
            last = start_s;
        }
    }
    shots
}

fn pair_score(a: &FrameSignature, b: &FrameSignature) -> f32 {
    let hamming = (a.dhash ^ b.dhash).count_ones() as f32 / 64.0;
    let histogram = a
        .hist
        .iter()
        .zip(b.hist.iter())
        .map(|(left, right)| (left - right).abs())
        .sum::<f32>()
        / 2.0;
    0.6 * hamming + 0.4 * histogram
}

fn median(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[sorted.len() / 2]
}

fn median_absolute_deviation(values: &[f32]) -> f32 {
    let center = median(values);
    let deviations: Vec<f32> = values.iter().map(|value| (value - center).abs()).collect();
    median(&deviations)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signature(index: u64, fill: u8) -> FrameSignature {
        // A synthetic two-colour pixel that keeps the helper's "fill" meaning:
        // low fills are teal-ish, high fills are orange-ish, so chromaticity
        // bins differ even when both are dark.
        let (r, g, b) = if fill > 128 {
            (220_u8, 35_u8, 64_u8)
        } else {
            (30_u8, 225_u8, 64_u8)
        };
        let max = r.max(g).max(b).max(1);
        let level = |c: u8| (usize::from(c) * 255 / usize::from(max)) >> 6;
        let mut hist = [0.0_f32; HIST_BINS];
        hist[level(r) * 16 + level(g) * 4 + level(b)] = 1.0;
        FrameSignature {
            index,
            time_s: index as f32 / 2.0,
            dhash: if fill > 128 { u64::MAX } else { 0 },
            hist,
        }
    }

    #[test]
    fn a_static_sequence_has_no_cuts() {
        let frames: Vec<_> = (0..10).map(|index| signature(index, 30)).collect();
        assert!(detect_shots(&frames).is_empty());
    }

    #[test]
    fn a_hard_change_is_a_cut() {
        let mut frames: Vec<_> = (0..4).map(|index| signature(index, 30)).collect();
        frames.extend((4..8).map(|index| signature(index, 220)));
        let shots = detect_shots(&frames);
        assert_eq!(shots.len(), 1, "one colour change, one cut: {shots:?}");
        assert!((shots[0].start_s - 2.0).abs() < 0.01);
    }

    #[test]
    fn cuts_closer_than_the_minimum_are_merged() {
        let mut frames: Vec<_> = (0..4).map(|index| signature(index, 30)).collect();
        frames.extend((4..5).map(|index| signature(index, 220)));
        frames.extend((5..8).map(|index| signature(index, 30)));
        let shots = detect_shots(&frames);
        assert_eq!(shots.len(), 1, "0.5 s apart is one edit: {shots:?}");
    }

    #[test]
    fn a_partial_trailing_frame_is_ignored() {
        let mut bytes: Vec<u8> = (0..FRAME_BYTES).map(|_| 30).collect();
        bytes.extend(std::iter::repeat_n(200, 100));
        let frames = signatures(&bytes, 2.0);
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn hashes_differ_between_structure_and_its_inverse() {
        let mut frame: Vec<u8> = Vec::with_capacity(FRAME_BYTES);
        for _y in 0..FRAME_HEIGHT {
            for x in 0..FRAME_WIDTH {
                let value = (x * 255 / FRAME_WIDTH) as u8;
                frame.extend([value, value, value]);
            }
        }
        let inverse: Vec<u8> = frame.iter().map(|value| 255 - value).collect();
        let a = signatures(&frame, 2.0).remove(0);
        let b = signatures(&inverse, 2.0).remove(0);
        assert_eq!((a.dhash ^ b.dhash).count_ones(), 64);
    }
}
