//! Decoding to mono `f32` PCM through an FFmpeg pipe.

use std::path::Path;
use std::time::Duration;

use sonolytica_core::Audio;
use tokio::io::AsyncReadExt;

use crate::error::MediaError;
use crate::tools::MediaTools;

#[derive(Debug, Clone, Copy)]
pub struct DecodeOptions {
    pub sample_rate: u32,
    /// Hard cap on decoded duration; also enforced by `-t`.
    pub max_duration_secs: u64,
    pub timeout_secs: u64,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            max_duration_secs: 7_200,
            timeout_secs: 600,
        }
    }
}

/// Decode the first audio stream to mono `f32` at `opts.sample_rate`.
///
/// Downmixing and resampling are FFmpeg's job here: it is better tested than
/// anything this project would hand-roll, and doing it in one pass keeps peak
/// memory at roughly `samples * 4` bytes.
pub async fn decode_mono(
    tools: &MediaTools,
    path: &Path,
    opts: &DecodeOptions,
) -> Result<Audio, MediaError> {
    if opts.sample_rate == 0 {
        return Err(MediaError::InvalidInput("sample rate must be > 0".into()));
    }

    let timeout = Duration::from_secs(opts.timeout_secs.clamp(5, 3_600));
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
            // The trailing `?` makes a missing audio stream a non-fatal map;
            // decode then produces zero bytes and we report NoAudioStream.
            "0:a:0?",
            "-vn",
            "-sn",
            "-dn",
            "-ac",
            "1",
            "-ar",
            &opts.sample_rate.to_string(),
            "-t",
            &opts.max_duration_secs.to_string(),
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
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

    if !status.success() {
        let message = stderr_text
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("ffmpeg exited with an error")
            .to_string();
        // FFmpeg refuses to open an output with no streams and reports the
        // failure in its own words; map that to the domain error.
        if stderr_text.contains("matches no streams")
            || stderr_text.contains("does not contain any stream")
            || stderr_text.contains("Output file does not contain any stream")
        {
            return Err(MediaError::NoAudioStream);
        }
        return Err(MediaError::DecodeFailed(message));
    }

    if bytes.is_empty() {
        return Err(MediaError::NoAudioStream);
    }
    if bytes.len() % 4 != 0 {
        return Err(MediaError::DecodeFailed(
            "decoder produced a truncated sample".into(),
        ));
    }

    let max_samples = (opts.max_duration_secs as usize)
        .saturating_mul(opts.sample_rate as usize)
        .saturating_add(opts.sample_rate as usize);
    if bytes.len() / 4 > max_samples {
        return Err(MediaError::DecodeFailed(format!(
            "decoded {} samples exceeds the {} sample budget",
            bytes.len() / 4,
            max_samples
        )));
    }

    let mut samples = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let sample = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        if !sample.is_finite() {
            return Err(MediaError::BadAudio);
        }
        samples.push(sample);
    }

    let audio = Audio::new(opts.sample_rate, samples);
    audio.validate().map_err(|_| MediaError::BadAudio)?;
    Ok(audio)
}
