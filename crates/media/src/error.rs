use thiserror::Error;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error(
        "ffmpeg/ffprobe not found; install FFmpeg and set SONOLYTICA_FFMPEG / SONOLYTICA_FFPROBE if it is not on PATH"
    )]
    ToolsNotFound,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("input is not a regular file: {0}")]
    NotAFile(String),
    #[error("input is too large: {size} bytes exceeds the {limit} byte limit")]
    TooLarge { size: u64, limit: u64 },
    #[error("ffprobe failed: {0}")]
    ProbeFailed(String),
    #[error("ffmpeg failed: {0}")]
    DecodeFailed(String),
    #[error("external process timed out after {0:?}")]
    Timeout(std::time::Duration),
    #[error("no audio stream in input")]
    NoAudioStream,
    #[error("no video stream in input")]
    NoVideoStream,
    #[error("decoded audio is empty or non-finite")]
    BadAudio,
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse media metadata: {0}")]
    Parse(String),
}
