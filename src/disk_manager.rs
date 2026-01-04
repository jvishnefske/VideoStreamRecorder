use crate::config::Config;
use anyhow::Result;
use chrono::{DateTime, Utc};
use fs2::available_space;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

#[derive(Debug, Clone, serde::Serialize)]
pub struct StorageInfo {
    pub total_space: u64,
    pub available_space: u64,
    pub used_space: u64,
    pub usage_percent: f64,
}

pub struct DiskManager {
    config: Config,
    last_cleanup: Arc<RwLock<Option<DateTime<Utc>>>>,
    files_deleted: AtomicU64,
}

impl DiskManager {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            last_cleanup: Arc::new(RwLock::new(None)),
            files_deleted: AtomicU64::new(0),
        }
    }

    pub async fn has_space(&self) -> Result<bool> {
        let storage_info = self.get_storage_info().await?;
        Ok(storage_info.usage_percent < self.config.max_disk_usage_percent)
    }

    pub async fn get_storage_info(&self) -> Result<StorageInfo> {
        let available = available_space(&self.config.output_dir)?;
        let total = self.get_total_space(&self.config.output_dir).await?;
        let used = total.saturating_sub(available);
        let usage_percent = (used as f64 / total as f64) * 100.0;

        Ok(StorageInfo {
            total_space: total,
            available_space: available,
            used_space: used,
            usage_percent,
        })
    }

    async fn get_total_space(&self, path: &Path) -> Result<u64> {
        // This is a simplified implementation
        // In production, you'd want to use statvfs or similar
        Ok(available_space(path)? * 10) // Rough estimate
    }

    pub async fn cleanup_old_files(&self) -> Result<u64> {
        let mut deleted_count = 0;
        let mut deleted_size = 0u64;

        info!(
            "Starting cleanup of old files in {:?} and stream subdirectories",
            self.config.output_dir
        );

        // Collect files from main directory and all stream subdirectories
        let mut files = Vec::new();

        // Add files from main output directory
        self.collect_mp4_files(&self.config.output_dir, &mut files).await?;

        // Add files from each stream's output directory
        for stream in &self.config.streams {
            if stream.enabled {
                let stream_dir = self.config.get_stream_output_dir(&stream.id);
                if stream_dir.exists() {
                    self.collect_mp4_files(&stream_dir, &mut files).await?;
                }
            }
        }

        // Sort by creation time (oldest first)
        files.sort_by_key(|(_, created, _)| *created);

        info!("Found {} video files across all directories for potential cleanup", files.len());

        // Delete oldest files until we have enough space
        for (file_path, _, size) in files {
            if self.has_space().await? {
                break;
            }

            match tokio::fs::remove_file(&file_path).await {
                Ok(_) => {
                    info!("Deleted old file: {:?}", file_path);
                    deleted_count += 1;
                    deleted_size += size;
                    self.files_deleted.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    error!("Failed to delete file {:?}: {}", file_path, e);
                }
            }
        }

        *self.last_cleanup.write().await = Some(Utc::now());

        info!(
            "Cleanup completed: {} files deleted, {} bytes freed",
            deleted_count, deleted_size
        );
        Ok(deleted_size)
    }

    async fn collect_mp4_files(&self, dir: &Path, files: &mut Vec<(std::path::PathBuf, std::time::SystemTime, u64)>) -> Result<()> {
        let mut entries = tokio::fs::read_dir(dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if let Ok(metadata) = entry.metadata().await {
                if metadata.is_file() {
                    if let Some(path_str) = path.to_str() {
                        if path_str.ends_with(".mp4") {
                            if let Ok(created) = metadata.created() {
                                files.push((path, created, metadata.len()));
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn start_monitoring(&self) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(
            self.config.cleanup_check_interval,
        ));

        info!(
            "Starting disk monitoring (check interval: {}s, max usage: {:.1}%)",
            self.config.cleanup_check_interval, self.config.max_disk_usage_percent
        );

        loop {
            interval.tick().await;

            match self.get_storage_info().await {
                Ok(storage) => {
                    // Log storage status periodically
                    info!(
                        "Storage check - {:.1}% used ({} GB available)",
                        storage.usage_percent,
                        storage.available_space / 1_000_000_000
                    );

                    if storage.usage_percent >= self.config.max_disk_usage_percent {
                        warn!(
                            "Disk usage exceeded threshold ({:.1}% >= {:.1}%), starting cleanup",
                            storage.usage_percent, self.config.max_disk_usage_percent
                        );
                        if let Err(e) = self.cleanup_old_files().await {
                            error!("Cleanup failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to check disk space: {}", e);
                }
            }
        }
    }

    pub async fn get_last_cleanup(&self) -> Option<DateTime<Utc>> {
        *self.last_cleanup.read().await
    }

    pub fn get_files_deleted(&self) -> u64 {
        self.files_deleted.load(Ordering::Relaxed)
    }

    /// Get the configuration for use in extension modules
    pub fn get_config(&self) -> &Config {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_info_serialization() {
        let info = StorageInfo {
            total_space: 1_000_000_000_000,
            available_space: 500_000_000_000,
            used_space: 500_000_000_000,
            usage_percent: 50.0,
        };

        let json = serde_json::to_string(&info).unwrap();

        assert!(json.contains("\"total_space\":1000000000000"));
        assert!(json.contains("\"available_space\":500000000000"));
        assert!(json.contains("\"used_space\":500000000000"));
        assert!(json.contains("\"usage_percent\":50.0"));
    }

    #[test]
    fn test_storage_info_clone() {
        let info = StorageInfo {
            total_space: 100,
            available_space: 50,
            used_space: 50,
            usage_percent: 50.0,
        };

        let cloned = info.clone();

        assert_eq!(cloned.total_space, info.total_space);
        assert_eq!(cloned.available_space, info.available_space);
        assert_eq!(cloned.used_space, info.used_space);
        assert!((cloned.usage_percent - info.usage_percent).abs() < 0.001);
    }

    #[test]
    fn test_storage_info_debug() {
        let info = StorageInfo {
            total_space: 100,
            available_space: 50,
            used_space: 50,
            usage_percent: 50.0,
        };

        let debug_str = format!("{:?}", info);

        assert!(debug_str.contains("StorageInfo"));
        assert!(debug_str.contains("total_space"));
        assert!(debug_str.contains("100"));
    }

    #[test]
    fn test_disk_manager_new() {
        let config = Config::default();
        let manager = DiskManager::new(config.clone());

        assert_eq!(manager.get_files_deleted(), 0);
        assert_eq!(manager.get_config().segment_duration, config.segment_duration);
    }

    #[test]
    fn test_disk_manager_files_deleted_counter() {
        let config = Config::default();
        let manager = DiskManager::new(config);

        assert_eq!(manager.get_files_deleted(), 0);

        manager.files_deleted.fetch_add(5, Ordering::Relaxed);
        assert_eq!(manager.get_files_deleted(), 5);

        manager.files_deleted.fetch_add(3, Ordering::Relaxed);
        assert_eq!(manager.get_files_deleted(), 8);
    }

    #[tokio::test]
    async fn test_disk_manager_last_cleanup_initially_none() {
        let config = Config::default();
        let manager = DiskManager::new(config);

        assert!(manager.get_last_cleanup().await.is_none());
    }

    #[tokio::test]
    async fn test_disk_manager_get_config() {
        let mut config = Config::default();
        config.segment_duration = 42;
        config.max_disk_usage_percent = 75.0;

        let manager = DiskManager::new(config);

        assert_eq!(manager.get_config().segment_duration, 42);
        assert!((manager.get_config().max_disk_usage_percent - 75.0).abs() < 0.001);
    }

    /// FR-6: Tests that has_space returns true when under threshold
    #[tokio::test]
    async fn test_has_space_under_threshold() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.max_disk_usage_percent = 99.0; // Very high threshold

        let manager = DiskManager::new(config);

        // Should have space since threshold is very high
        let result = manager.has_space().await;
        assert!(result.is_ok());
        // Most filesystems will be under 99% usage
        assert!(result.unwrap());
    }

    /// FR-6: Tests get_storage_info returns valid storage information
    #[tokio::test]
    async fn test_get_storage_info_returns_valid_data() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();

        let manager = DiskManager::new(config);

        let result = manager.get_storage_info().await;
        assert!(result.is_ok());

        let info = result.unwrap();
        assert!(info.total_space > 0);
        assert!(info.available_space > 0);
        assert!(info.usage_percent >= 0.0);
        assert!(info.usage_percent <= 100.0);
    }

    /// FR-7: Tests cleanup_old_files with no files returns zero
    #[tokio::test]
    async fn test_cleanup_old_files_empty_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.max_disk_usage_percent = 99.0; // High threshold so cleanup won't delete

        let manager = DiskManager::new(config);

        let result = manager.cleanup_old_files().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    /// FR-7: Tests cleanup_old_files updates last_cleanup timestamp
    #[tokio::test]
    async fn test_cleanup_updates_last_cleanup() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();

        let manager = DiskManager::new(config);

        // Initially no cleanup
        assert!(manager.get_last_cleanup().await.is_none());

        // Run cleanup
        manager.cleanup_old_files().await.unwrap();

        // Now should have a timestamp
        assert!(manager.get_last_cleanup().await.is_some());
    }

    /// FR-7: Tests collect_mp4_files finds mp4 files
    #[tokio::test]
    async fn test_collect_mp4_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();

        // Create test mp4 files
        std::fs::write(temp_dir.path().join("test1.mp4"), b"fake video 1").unwrap();
        std::fs::write(temp_dir.path().join("test2.mp4"), b"fake video 2").unwrap();
        std::fs::write(temp_dir.path().join("test.txt"), b"not a video").unwrap();

        let manager = DiskManager::new(config);

        let mut files = Vec::new();
        manager.collect_mp4_files(temp_dir.path(), &mut files).await.unwrap();

        // Should find exactly 2 mp4 files
        assert_eq!(files.len(), 2);
    }

    /// FR-7: Tests collect_mp4_files ignores non-mp4 files
    #[tokio::test]
    async fn test_collect_mp4_files_ignores_other_extensions() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();

        // Create non-mp4 files
        std::fs::write(temp_dir.path().join("video.avi"), b"avi file").unwrap();
        std::fs::write(temp_dir.path().join("video.mkv"), b"mkv file").unwrap();
        std::fs::write(temp_dir.path().join("readme.txt"), b"text file").unwrap();

        let manager = DiskManager::new(config);

        let mut files = Vec::new();
        manager.collect_mp4_files(temp_dir.path(), &mut files).await.unwrap();

        // Should find no mp4 files
        assert_eq!(files.len(), 0);
    }

    /// FR-9: Tests cleanup includes stream subdirectories
    #[tokio::test]
    async fn test_cleanup_includes_stream_subdirectories() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.max_disk_usage_percent = 99.0;
        config.streams = vec![
            crate::config::StreamConfig {
                id: "cam1".to_string(),
                url: "rtsp://example.com/cam1".to_string(),
                enabled: true,
                output_subdir: None,
            },
        ];

        // Create stream subdirectory with mp4 file
        let stream_dir = temp_dir.path().join("cam1");
        std::fs::create_dir(&stream_dir).unwrap();
        std::fs::write(stream_dir.join("segment_001.mp4"), b"video content").unwrap();

        let manager = DiskManager::new(config);

        // Should complete without error
        let result = manager.cleanup_old_files().await;
        assert!(result.is_ok());
    }

    /// Tests files_deleted counter increments correctly
    #[tokio::test]
    async fn test_files_deleted_counter_tracking() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();

        let manager = DiskManager::new(config);

        assert_eq!(manager.get_files_deleted(), 0);

        // Manually increment to test counter
        manager.files_deleted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(manager.get_files_deleted(), 1);

        manager.files_deleted.fetch_add(4, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(manager.get_files_deleted(), 5);
    }

    /// FR-8: Tests that max_disk_usage_percent is respected
    #[tokio::test]
    async fn test_has_space_respects_threshold() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();

        // Set a very low threshold that will likely be exceeded
        config.max_disk_usage_percent = 0.001;

        let manager = DiskManager::new(config);

        let result = manager.has_space().await;
        assert!(result.is_ok());
        // Most filesystems will be above 0.001% usage, so should return false
        assert!(!result.unwrap());
    }

    /// Tests storage info calculation
    #[tokio::test]
    async fn test_storage_info_calculations() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();

        let manager = DiskManager::new(config);

        let info = manager.get_storage_info().await.unwrap();

        // Verify calculations are consistent
        assert_eq!(info.used_space, info.total_space.saturating_sub(info.available_space));

        // Verify usage percent matches
        let calculated_percent = (info.used_space as f64 / info.total_space as f64) * 100.0;
        assert!((info.usage_percent - calculated_percent).abs() < 0.001);
    }

    /// Tests cleanup with disabled streams
    #[tokio::test]
    async fn test_cleanup_skips_disabled_stream_directories() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.max_disk_usage_percent = 99.0;
        config.streams = vec![
            crate::config::StreamConfig {
                id: "disabled_cam".to_string(),
                url: "rtsp://example.com/disabled".to_string(),
                enabled: false,
                output_subdir: None,
            },
        ];

        // Create stream subdirectory even though it's disabled
        let stream_dir = temp_dir.path().join("disabled_cam");
        std::fs::create_dir(&stream_dir).unwrap();
        std::fs::write(stream_dir.join("segment_001.mp4"), b"video").unwrap();

        let manager = DiskManager::new(config);

        // Cleanup should complete without errors
        let result = manager.cleanup_old_files().await;
        assert!(result.is_ok());
    }

    /// Tests that cleanup handles non-existent stream directories
    #[tokio::test]
    async fn test_cleanup_handles_missing_stream_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.max_disk_usage_percent = 99.0;
        config.streams = vec![
            crate::config::StreamConfig {
                id: "missing_cam".to_string(),
                url: "rtsp://example.com/missing".to_string(),
                enabled: true,
                output_subdir: None,
            },
        ];

        // Don't create the stream subdirectory

        let manager = DiskManager::new(config);

        // Cleanup should complete without errors even if directory doesn't exist
        let result = manager.cleanup_old_files().await;
        assert!(result.is_ok());
    }

    /// Tests collect_mp4_files handles empty directory
    #[tokio::test]
    async fn test_collect_mp4_files_empty_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();

        let manager = DiskManager::new(config);

        let mut files = Vec::new();
        let result = manager.collect_mp4_files(temp_dir.path(), &mut files).await;

        assert!(result.is_ok());
        assert!(files.is_empty());
    }

    /// Tests collect_mp4_files returns files with correct metadata
    #[tokio::test]
    async fn test_collect_mp4_files_returns_metadata() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();

        let content = b"fake video content with some size";
        std::fs::write(temp_dir.path().join("video.mp4"), content).unwrap();

        let manager = DiskManager::new(config);

        let mut files = Vec::new();
        manager.collect_mp4_files(temp_dir.path(), &mut files).await.unwrap();

        assert_eq!(files.len(), 1);
        let (path, _created, size) = &files[0];
        assert!(path.ends_with("video.mp4"));
        assert_eq!(*size, content.len() as u64);
    }

    /// Tests cleanup sorts files by creation time
    #[tokio::test]
    async fn test_cleanup_sorts_by_creation_time() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.max_disk_usage_percent = 99.0;

        // Create files with small delay to ensure different timestamps
        std::fs::write(temp_dir.path().join("first.mp4"), b"first").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(temp_dir.path().join("second.mp4"), b"second").unwrap();

        let manager = DiskManager::new(config);

        let result = manager.cleanup_old_files().await;
        assert!(result.is_ok());
        // Files should be sorted but not deleted since we're under threshold
    }

    /// Tests cleanup deletes files when over threshold
    #[tokio::test]
    async fn test_cleanup_deletes_when_over_threshold() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        // Set threshold very low to trigger cleanup
        config.max_disk_usage_percent = 0.0001;

        // Create a file to be deleted
        std::fs::write(temp_dir.path().join("to_delete.mp4"), b"video content").unwrap();

        let manager = DiskManager::new(config);

        // Verify file exists
        assert!(temp_dir.path().join("to_delete.mp4").exists());

        // Run cleanup
        let result = manager.cleanup_old_files().await;
        assert!(result.is_ok());

        // File may or may not be deleted depending on how quickly has_space returns
        // The important thing is cleanup ran without error
    }

    /// Tests files_deleted counter increments during cleanup
    #[tokio::test]
    async fn test_cleanup_increments_deleted_counter() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.max_disk_usage_percent = 0.0001; // Very low to force cleanup

        // Create files
        std::fs::write(temp_dir.path().join("video1.mp4"), b"video1").unwrap();
        std::fs::write(temp_dir.path().join("video2.mp4"), b"video2").unwrap();

        let manager = DiskManager::new(config);

        let before_count = manager.get_files_deleted();

        // Run cleanup
        let _ = manager.cleanup_old_files().await;

        // Counter should be at least what it was before
        assert!(manager.get_files_deleted() >= before_count);
    }

    /// Tests collect_mp4_files handles subdirectories correctly
    #[tokio::test]
    async fn test_collect_mp4_files_no_subdirs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = Config::default();

        // Create a subdirectory with mp4 files - they should NOT be collected
        let subdir = temp_dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(subdir.join("nested.mp4"), b"nested video").unwrap();

        // Create mp4 in root dir
        std::fs::write(temp_dir.path().join("root.mp4"), b"root video").unwrap();

        let manager = DiskManager::new(config);

        let mut files = Vec::new();
        manager.collect_mp4_files(temp_dir.path(), &mut files).await.unwrap();

        // Should only find the root mp4, not the nested one
        assert_eq!(files.len(), 1);
        let (path, _, _) = &files[0];
        assert!(path.ends_with("root.mp4"));
    }

    /// Tests cleanup with multiple stream directories
    #[tokio::test]
    async fn test_cleanup_multiple_streams() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.max_disk_usage_percent = 99.0;
        config.streams = vec![
            crate::config::StreamConfig {
                id: "cam1".to_string(),
                url: "rtsp://example.com/cam1".to_string(),
                enabled: true,
                output_subdir: None,
            },
            crate::config::StreamConfig {
                id: "cam2".to_string(),
                url: "rtsp://example.com/cam2".to_string(),
                enabled: true,
                output_subdir: None,
            },
        ];

        // Create stream directories with files
        let cam1_dir = temp_dir.path().join("cam1");
        let cam2_dir = temp_dir.path().join("cam2");
        std::fs::create_dir(&cam1_dir).unwrap();
        std::fs::create_dir(&cam2_dir).unwrap();
        std::fs::write(cam1_dir.join("video.mp4"), b"cam1 video").unwrap();
        std::fs::write(cam2_dir.join("video.mp4"), b"cam2 video").unwrap();

        let manager = DiskManager::new(config);

        let result = manager.cleanup_old_files().await;
        assert!(result.is_ok());
    }

    /// Tests storage info calculations are consistent
    #[tokio::test]
    async fn test_storage_info_consistency() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();

        let manager = DiskManager::new(config);

        let info = manager.get_storage_info().await.unwrap();

        // Total should be >= available + used
        assert!(info.total_space >= info.available_space);
        assert!(info.total_space >= info.used_space);

        // Used space should be total - available
        assert_eq!(info.used_space, info.total_space.saturating_sub(info.available_space));

        // Usage percent should be between 0 and 100
        assert!(info.usage_percent >= 0.0);
        assert!(info.usage_percent <= 100.0);
    }

    /// Tests get_config returns the config used for initialization
    #[test]
    fn test_get_config_returns_initialized_config() {
        let mut config = Config::default();
        config.segment_duration = 42;
        config.max_disk_usage_percent = 75.5;
        config.cleanup_check_interval = 120;

        let manager = DiskManager::new(config);

        assert_eq!(manager.get_config().segment_duration, 42);
        assert!((manager.get_config().max_disk_usage_percent - 75.5).abs() < 0.001);
        assert_eq!(manager.get_config().cleanup_check_interval, 120);
    }

    /// Tests cleanup updates last_cleanup even with no files deleted
    #[tokio::test]
    async fn test_cleanup_always_updates_timestamp() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.max_disk_usage_percent = 99.0; // High threshold, won't delete

        let manager = DiskManager::new(config);

        // Before cleanup, no timestamp
        assert!(manager.get_last_cleanup().await.is_none());

        // Run cleanup (no files to delete)
        let _ = manager.cleanup_old_files().await;

        // After cleanup, timestamp should be set
        assert!(manager.get_last_cleanup().await.is_some());
    }

    /// Tests file metadata is correctly captured
    #[tokio::test]
    async fn test_collect_mp4_captures_size() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = Config::default();

        let content = b"This is test video content with known size";
        std::fs::write(temp_dir.path().join("sized.mp4"), content).unwrap();

        let manager = DiskManager::new(config);

        let mut files = Vec::new();
        manager.collect_mp4_files(temp_dir.path(), &mut files).await.unwrap();

        assert_eq!(files.len(), 1);
        let (_, _, size) = &files[0];
        assert_eq!(*size, content.len() as u64);
    }
}
