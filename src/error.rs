use thiserror::Error;

#[derive(Error, Debug)]
pub enum RecorderError {
    #[error("FFmpeg error: {0}")]
    FFmpeg(#[from] ffmpeg_next::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Recording already in progress")]
    AlreadyRecording,

    #[error("Recording not active")]
    NotRecording,

    #[error("Disk full - cannot continue recording")]
    DiskFull,

    #[error("Stream connection failed: {0}")]
    StreamConnection(String),

    #[error("Anyhow error: {0}")]
    Anyhow(#[from] anyhow::Error),
}
