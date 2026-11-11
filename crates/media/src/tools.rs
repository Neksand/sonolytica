//! External tool discovery and input validation.

use std::env;
use std::path::{Path, PathBuf};

use sonolytica_core::Limits;

use crate::error::MediaError;

/// Paths to the FFmpeg binaries used for probing and decoding.
#[derive(Debug, Clone)]
pub struct MediaTools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl Default for MediaTools {
    fn default() -> Self {
        Self::detect()
    }
}

impl MediaTools {
    /// Resolve from `SONOLYTICA_FFMPEG`/`SONOLYTICA_FFPROBE`, falling back to
    /// `PATH` lookup by name.
    pub fn detect() -> Self {
        Self {
            ffmpeg: env::var_os("SONOLYTICA_FFMPEG")
                .map_or_else(|| PathBuf::from("ffmpeg"), PathBuf::from),
            ffprobe: env::var_os("SONOLYTICA_FFPROBE")
                .map_or_else(|| PathBuf::from("ffprobe"), PathBuf::from),
        }
    }

    /// First line of `ffmpeg -version`, used by `sonolytica doctor`.
    pub async fn ffmpeg_version(&self) -> Result<String, MediaError> {
        let output = tokio::process::Command::new(&self.ffmpeg)
            .arg("-version")
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => MediaError::ToolsNotFound,
                _ => MediaError::Io(e),
            })?;
        let first = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        if first.is_empty() {
            return Err(MediaError::ToolsNotFound);
        }
        Ok(first)
    }
}

/// A path that passed the trust-boundary checks.
#[derive(Debug, Clone)]
pub struct ValidatedInput {
    pub path: PathBuf,
    pub size_bytes: u64,
}

/// Validate an untrusted path before handing it to FFmpeg.
///
/// Checks: resolvable, a regular file after symlink resolution, and within the
/// configured byte budget. The canonical path is what gets passed to FFmpeg,
/// so a path cannot be swapped for a symlink between check and use.
pub fn validate_input(path: &Path, limits: &Limits) -> Result<ValidatedInput, MediaError> {
    let canonical = path.canonicalize().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => {
            MediaError::InvalidInput(format!("{} does not exist", path.display()))
        }
        _ => MediaError::Io(e),
    })?;

    let meta = std::fs::metadata(&canonical)?;
    if !meta.is_file() {
        return Err(MediaError::NotAFile(canonical.display().to_string()));
    }
    if meta.len() > limits.max_file_bytes {
        return Err(MediaError::TooLarge {
            size: meta.len(),
            limit: limits.max_file_bytes,
        });
    }
    if meta.len() == 0 {
        return Err(MediaError::InvalidInput("input file is empty".into()));
    }

    Ok(ValidatedInput {
        path: canonical,
        size_bytes: meta.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rejects_missing_and_directory_inputs() {
        let limits = Limits::default();
        let missing = Path::new("/nonexistent/sonolytica-test.wav");
        assert!(validate_input(missing, &limits).is_err());
        assert!(validate_input(Path::new("/tmp"), &limits).is_err());
    }

    #[test]
    fn enforces_size_budget() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"0123456789").unwrap();
        let limits = Limits {
            max_file_bytes: 4,
            ..Limits::default()
        };
        assert!(matches!(
            validate_input(file.path(), &limits),
            Err(MediaError::TooLarge { .. })
        ));
    }
}
