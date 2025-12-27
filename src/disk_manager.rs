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
}
