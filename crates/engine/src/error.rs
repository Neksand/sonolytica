use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Media(#[from] sonolytica_media::MediaError),
    #[error("invalid analysis request: {0}")]
    InvalidRequest(String),
    #[error("analysis produced an invalid document: {0}")]
    InvalidDocument(String),
    #[error("task failed to join: {0}")]
    Join(String),
    #[error("no audio stream in input")]
    NoAudio,
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}
