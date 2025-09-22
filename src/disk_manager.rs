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
            "Starting cleanup of old files in {:?}",
            self.config.output_dir
        );

        let mut entries = tokio::fs::read_dir(&self.config.output_dir).await?;
        let mut files = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            if let Ok(metadata) = entry.metadata().await {
                if metadata.is_file() {
                    if let Some(path_str) = entry.path().to_str() {
                        if path_str.ends_with(".mp4") {
                            if let Ok(created) = metadata.created() {
                                files.push((entry.path(), created, metadata.len()));
                            }
                        }
                    }
                }
            }
        }

        // Sort by creation time (oldest first)
        files.sort_by_key(|(_, created, _)| *created);

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

    pub async fn start_monitoring(&self) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(
            self.config.cleanup_check_interval,
        ));

        loop {
            interval.tick().await;

            match self.has_space().await {
                Ok(has_space) => {
                    if !has_space {
                        warn!("Disk usage exceeded threshold, starting cleanup");
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
}
