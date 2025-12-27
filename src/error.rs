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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_already_recording_error_display() {
        let error = RecorderError::AlreadyRecording;
        assert_eq!(error.to_string(), "Recording already in progress");
    }

    #[test]
    fn test_not_recording_error_display() {
        let error = RecorderError::NotRecording;
        assert_eq!(error.to_string(), "Recording not active");
    }

    #[test]
    fn test_disk_full_error_display() {
        let error = RecorderError::DiskFull;
        assert_eq!(error.to_string(), "Disk full - cannot continue recording");
    }

    #[test]
    fn test_stream_connection_error_display() {
        let error = RecorderError::StreamConnection("Connection refused".to_string());
        assert_eq!(error.to_string(), "Stream connection failed: Connection refused");
    }

    #[test]
    fn test_io_error_conversion() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
        let recorder_error: RecorderError = io_error.into();
        assert!(matches!(recorder_error, RecorderError::Io(_)));
        assert!(recorder_error.to_string().contains("IO error"));
    }

    #[test]
    fn test_error_is_debug() {
        let error = RecorderError::DiskFull;
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("DiskFull"));
    }
}
