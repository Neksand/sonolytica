use thiserror::Error;

#[derive(Debug, Error)]
pub enum DspError {
    #[error("invalid frame configuration: {0}")]
    InvalidConfig(String),
    #[error("audio buffer is empty")]
    EmptyAudio,
    #[error("sample rate must be greater than zero")]
    InvalidSampleRate,
}
