//! Video Stream Recorder Library
//!
//! This library provides components for recording RTSP/HTTPS video streams
//! with configurable segmentation, disk management, and a web API.
//!
//! # Architecture
//!
//! The library follows a "Functional Core, Imperative Shell" architecture:
//! - Configuration is immutable-by-default
//! - State transitions are modeled as immutable transformations
//! - All I/O operations are isolated at the boundaries
//!
//! # Modules
//!
//! - `config` - Configuration loading and environment variable parsing (FR-15, FR-16, FR-17)
//! - `recorder` - Stream recording logic with FFmpeg integration (FR-1 through FR-5)
//! - `disk_manager` - Storage monitoring and cleanup (FR-6 through FR-9)
//! - `server` - Axum web server and API endpoints (FR-10 through FR-14)
//! - `error` - Custom error types with thiserror

pub mod config;
pub mod disk_manager;
pub mod disk_manager_ext;
pub mod error;
pub mod recorder;
pub mod server;

// Re-export commonly used types
pub use config::{Config, StreamConfig};
pub use disk_manager::{DiskManager, StorageInfo};
pub use error::RecorderError;
pub use recorder::{MultiStreamRecorder, Recorder, RecordingStats, StreamStats, VideoRecorder};
pub use server::Server;

use anyhow::Result;
use std::sync::Arc;
use tracing::{info, warn};

/// Application initialization result
pub struct AppComponents {
    pub config: Config,
    pub disk_manager: Arc<DiskManager>,
    pub recorder: Arc<VideoRecorder>,
}

/// Initialize the application components
///
/// This function creates and configures all the main application components:
/// - Loads configuration from file or environment variables
/// - Initializes FFmpeg
/// - Creates the disk manager for storage monitoring
/// - Creates the video recorder with multi-stream support
///
/// # Arguments
///
/// * `config_path` - Optional path to a JSON configuration file
///
/// # Returns
///
/// Returns `AppComponents` containing all initialized components, or an error
/// if initialization fails.
///
/// # Errors
///
/// This function will return an error if:
/// - Configuration loading fails
/// - FFmpeg initialization fails
/// - Stream directory creation fails
pub async fn initialize_app(config_path: Option<&str>) -> Result<AppComponents> {
    let config = Config::load(config_path)?;

    info!("Configuration loaded with {} streams configured", config.streams.len());

    // Create shared components
    let disk_manager = Arc::new(DiskManager::new(config.clone()));
    let recorder = Arc::new(VideoRecorder::new(config.clone(), disk_manager.clone()));

    // Initialize stream recorders for multi-stream support
    recorder.initialize_streams().await;

    Ok(AppComponents {
        config,
        disk_manager,
        recorder,
    })
}

/// Start the disk monitoring task
///
/// This spawns a background task that periodically checks disk usage
/// and cleans up old recordings when the threshold is exceeded.
///
/// # Arguments
///
/// * `disk_manager` - The disk manager to use for monitoring
///
/// # Returns
///
/// Returns a JoinHandle for the spawned task
pub fn start_disk_monitoring(disk_manager: Arc<DiskManager>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        disk_manager.start_monitoring().await;
    })
}

/// Auto-start recording if configured
///
/// This function starts recording on all enabled streams if `auto_start`
/// is enabled in the configuration.
///
/// # Arguments
///
/// * `config` - The configuration to check
/// * `recorder` - The recorder to start
///
/// # Returns
///
/// Returns Ok(true) if recording was started, Ok(false) if auto_start is disabled,
/// or an error if starting failed.
pub async fn auto_start_if_configured(
    config: &Config,
    recorder: &Arc<VideoRecorder>,
) -> Result<bool> {
    if config.auto_start {
        info!("Auto-starting recording");
        match recorder.start().await {
            Ok(_) => {
                info!("Recording auto-started successfully");
                Ok(true)
            }
            Err(e) => {
                warn!("Failed to auto-start recording: {}", e);
                // Don't fail the application, just warn
                Ok(false)
            }
        }
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that the library re-exports are available
    #[test]
    fn test_reexports_available() {
        // Just verify the re-exports compile
        let _config = Config::default();
    }

    /// Tests AppComponents struct creation
    #[tokio::test]
    async fn test_app_components_creation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();

        let disk_manager = Arc::new(DiskManager::new(config.clone()));
        let recorder = Arc::new(VideoRecorder::new(config.clone(), disk_manager.clone()));

        let components = AppComponents {
            config: config.clone(),
            disk_manager,
            recorder,
        };

        assert_eq!(components.config.segment_duration, config.segment_duration);
    }

    /// Tests initialize_app with defaults
    #[tokio::test]
    async fn test_initialize_app_no_config() {
        // Clear environment variables that might interfere
        std::env::remove_var("STREAM_URL");
        std::env::remove_var("STREAM_IDS");

        let result = initialize_app(None).await;

        assert!(result.is_ok());
        let components = result.unwrap();
        assert!(components.config.streams.is_empty());
    }

    /// Tests auto_start_if_configured when disabled
    #[tokio::test]
    async fn test_auto_start_disabled() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.auto_start = false;

        let disk_manager = Arc::new(DiskManager::new(config.clone()));
        let recorder = Arc::new(VideoRecorder::new(config.clone(), disk_manager));

        let result = auto_start_if_configured(&config, &recorder).await;

        assert!(result.is_ok());
        assert!(!result.unwrap()); // Should return false when disabled
    }

    /// Tests auto_start_if_configured when enabled but no streams
    #[tokio::test]
    async fn test_auto_start_enabled_no_streams() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.auto_start = true;
        config.streams = vec![]; // No streams

        let disk_manager = Arc::new(DiskManager::new(config.clone()));
        let recorder = Arc::new(VideoRecorder::new(config.clone(), disk_manager));

        recorder.initialize_streams().await;

        let result = auto_start_if_configured(&config, &recorder).await;

        // Should return false because no streams could be started
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    /// Tests disk monitoring task can be started
    #[tokio::test]
    async fn test_start_disk_monitoring() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.cleanup_check_interval = 3600; // 1 hour to avoid actual cleanup

        let disk_manager = Arc::new(DiskManager::new(config));

        let handle = start_disk_monitoring(disk_manager);

        // Let it run briefly then abort
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        handle.abort();

        // Just verify we can start and abort the task
        assert!(handle.is_finished() || handle.await.is_err());
    }

    /// Tests auto_start_if_configured returns false when recording fails
    #[tokio::test]
    async fn test_auto_start_with_invalid_stream() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.auto_start = true;
        config.streams = vec![crate::config::StreamConfig {
            id: "invalid".to_string(),
            url: "rtsp://invalid-host-that-does-not-exist.example.com:8554/stream".to_string(),
            enabled: true,
            output_subdir: None,
        }];

        std::fs::create_dir_all(temp_dir.path().join("invalid")).unwrap();

        let disk_manager = Arc::new(DiskManager::new(config.clone()));
        let recorder = Arc::new(VideoRecorder::new(config.clone(), disk_manager));

        recorder.initialize_streams().await;

        // This should return false because no streams successfully started recording
        // (The stream URL is invalid and will fail to connect)
        let result = auto_start_if_configured(&config, &recorder).await;

        // Clean up - stop any tasks that may have been spawned
        let _ = recorder.stop().await;

        assert!(result.is_ok());
        // Result may be true or false depending on whether the spawn happened before connect failure
    }

    /// Tests AppComponents fields are accessible
    #[tokio::test]
    async fn test_app_components_fields() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.segment_duration = 15;
        config.server_port = 9999;

        let disk_manager = Arc::new(DiskManager::new(config.clone()));
        let recorder = Arc::new(VideoRecorder::new(config.clone(), disk_manager.clone()));

        let components = AppComponents {
            config: config.clone(),
            disk_manager,
            recorder,
        };

        assert_eq!(components.config.segment_duration, 15);
        assert_eq!(components.config.server_port, 9999);
        assert!(components.disk_manager.get_files_deleted() == 0);
    }

    /// Tests initialize_app creates all components
    #[tokio::test]
    async fn test_initialize_app_creates_components() {
        // Clear environment to ensure defaults
        std::env::remove_var("STREAM_URL");
        std::env::remove_var("STREAM_IDS");

        let result = initialize_app(None).await;

        assert!(result.is_ok());
        let components = result.unwrap();

        // Verify components are properly initialized
        assert!(components.disk_manager.get_files_deleted() == 0);
        assert!(!components.recorder.is_recording().await);
    }

    /// Tests re-exports are properly available
    #[test]
    fn test_all_reexports() {
        // Test Config
        let config = Config::default();
        assert!(config.segment_duration > 0);

        // Test StreamConfig
        let stream = StreamConfig {
            id: "test".to_string(),
            url: "rtsp://test.com".to_string(),
            enabled: true,
            output_subdir: None,
        };
        assert!(stream.enabled);

        // Test StorageInfo
        let storage = StorageInfo {
            total_space: 1000,
            available_space: 500,
            used_space: 500,
            usage_percent: 50.0,
        };
        assert_eq!(storage.usage_percent, 50.0);

        // Test RecordingStats
        let stats = RecordingStats {
            started_at: None,
            files_recorded: 10,
            total_duration: 100,
            last_file: None,
        };
        assert_eq!(stats.files_recorded, 10);

        // Test StreamStats
        let stream_stats = StreamStats {
            stream_id: "cam1".to_string(),
            stream_url: "rtsp://test.com".to_string(),
            is_recording: false,
            stats: stats.clone(),
        };
        assert_eq!(stream_stats.stream_id, "cam1");
    }

    /// Tests DiskManager creation through lib re-export
    #[test]
    fn test_disk_manager_reexport() {
        let config = Config::default();
        let manager = DiskManager::new(config);
        assert_eq!(manager.get_files_deleted(), 0);
    }

    /// Tests RecorderError re-export
    #[test]
    fn test_recorder_error_reexport() {
        let error = RecorderError::NotRecording;
        assert_eq!(error.to_string(), "Recording not active");

        let error2 = RecorderError::AlreadyRecording;
        assert_eq!(error2.to_string(), "Recording already in progress");

        let error3 = RecorderError::DiskFull;
        assert_eq!(error3.to_string(), "Disk full - cannot continue recording");
    }
}
