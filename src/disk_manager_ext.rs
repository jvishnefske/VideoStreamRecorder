use crate::disk_manager::DiskManager;
use anyhow::Result;

#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamStorageInfo {
    pub stream_id: String,
    pub directory: std::path::PathBuf,
    pub total_files: u64,
    pub total_size: u64,
    pub estimated_duration: u64, // in seconds
}

impl DiskManager {
    /// Get storage info for a specific stream directory
    pub async fn get_stream_storage_info(&self, stream_id: &str) -> Result<Option<StreamStorageInfo>> {
        let stream_dir = self.get_config().get_stream_output_dir(stream_id);

        if !stream_dir.exists() {
            return Ok(None);
        }

        let mut total_files = 0;
        let mut total_size = 0u64;
        let mut total_duration = 0u64;

        let mut entries = tokio::fs::read_dir(&stream_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if let Ok(metadata) = entry.metadata().await {
                if metadata.is_file() {
                    if let Some(path_str) = entry.path().to_str() {
                        if path_str.ends_with(".mp4") {
                            total_files += 1;
                            total_size += metadata.len();
                            // Estimate duration based on segment duration from config
                            total_duration += self.get_config().segment_duration as u64;
                        }
                    }
                }
            }
        }

        Ok(Some(StreamStorageInfo {
            stream_id: stream_id.to_string(),
            directory: stream_dir,
            total_files,
            total_size,
            estimated_duration: total_duration,
        }))
    }

    /// Get storage info for all streams
    pub async fn get_all_streams_storage_info(&self) -> Result<Vec<StreamStorageInfo>> {
        let mut results = Vec::new();

        for stream in &self.get_config().streams {
            if stream.enabled {
                if let Some(info) = self.get_stream_storage_info(&stream.id).await? {
                    results.push(info);
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_storage_info_serialization() {
        let info = StreamStorageInfo {
            stream_id: "cam1".to_string(),
            directory: std::path::PathBuf::from("/recordings/cam1"),
            total_files: 100,
            total_size: 5_000_000_000,
            estimated_duration: 1000,
        };

        let json = serde_json::to_string(&info).unwrap();

        assert!(json.contains("\"stream_id\":\"cam1\""));
        assert!(json.contains("\"total_files\":100"));
        assert!(json.contains("\"total_size\":5000000000"));
        assert!(json.contains("\"estimated_duration\":1000"));
    }

    #[test]
    fn test_stream_storage_info_clone() {
        let info = StreamStorageInfo {
            stream_id: "cam2".to_string(),
            directory: std::path::PathBuf::from("/recordings/cam2"),
            total_files: 50,
            total_size: 2_500_000_000,
            estimated_duration: 500,
        };

        let cloned = info.clone();

        assert_eq!(cloned.stream_id, info.stream_id);
        assert_eq!(cloned.directory, info.directory);
        assert_eq!(cloned.total_files, info.total_files);
        assert_eq!(cloned.total_size, info.total_size);
        assert_eq!(cloned.estimated_duration, info.estimated_duration);
    }

    #[test]
    fn test_stream_storage_info_debug() {
        let info = StreamStorageInfo {
            stream_id: "test".to_string(),
            directory: std::path::PathBuf::from("/test"),
            total_files: 10,
            total_size: 1000,
            estimated_duration: 100,
        };

        let debug_str = format!("{:?}", info);

        assert!(debug_str.contains("StreamStorageInfo"));
        assert!(debug_str.contains("test"));
        assert!(debug_str.contains("10"));
    }

    /// Tests get_stream_storage_info returns None for non-existent directory
    #[tokio::test]
    async fn test_get_stream_storage_info_nonexistent() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.streams = vec![crate::config::StreamConfig {
            id: "cam1".to_string(),
            url: "rtsp://example.com/cam1".to_string(),
            enabled: true,
            output_subdir: None,
        }];

        let manager = DiskManager::new(config);

        // Stream directory doesn't exist
        let result = manager.get_stream_storage_info("cam1").await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    /// Tests get_stream_storage_info returns valid info for existing directory
    #[tokio::test]
    async fn test_get_stream_storage_info_with_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.segment_duration = 10;
        config.streams = vec![crate::config::StreamConfig {
            id: "cam1".to_string(),
            url: "rtsp://example.com/cam1".to_string(),
            enabled: true,
            output_subdir: None,
        }];

        // Create stream directory with mp4 files
        let stream_dir = temp_dir.path().join("cam1");
        std::fs::create_dir(&stream_dir).unwrap();
        std::fs::write(stream_dir.join("segment_001.mp4"), b"fake video 1").unwrap();
        std::fs::write(stream_dir.join("segment_002.mp4"), b"fake video 2 longer").unwrap();

        let manager = DiskManager::new(config);

        let result = manager.get_stream_storage_info("cam1").await;
        assert!(result.is_ok());

        let info = result.unwrap().unwrap();
        assert_eq!(info.stream_id, "cam1");
        assert_eq!(info.total_files, 2);
        assert!(info.total_size > 0);
        assert_eq!(info.estimated_duration, 20); // 2 files * 10 seconds segment duration
    }

    /// Tests get_stream_storage_info ignores non-mp4 files
    #[tokio::test]
    async fn test_get_stream_storage_info_ignores_non_mp4() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.segment_duration = 10;
        config.streams = vec![crate::config::StreamConfig {
            id: "cam1".to_string(),
            url: "rtsp://example.com/cam1".to_string(),
            enabled: true,
            output_subdir: None,
        }];

        // Create stream directory with mixed files
        let stream_dir = temp_dir.path().join("cam1");
        std::fs::create_dir(&stream_dir).unwrap();
        std::fs::write(stream_dir.join("segment_001.mp4"), b"video").unwrap();
        std::fs::write(stream_dir.join("readme.txt"), b"text file").unwrap();
        std::fs::write(stream_dir.join("video.avi"), b"avi file").unwrap();

        let manager = DiskManager::new(config);

        let result = manager.get_stream_storage_info("cam1").await;
        assert!(result.is_ok());

        let info = result.unwrap().unwrap();
        assert_eq!(info.total_files, 1); // Only the mp4 file
    }

    /// Tests get_stream_storage_info with empty directory
    #[tokio::test]
    async fn test_get_stream_storage_info_empty_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.streams = vec![crate::config::StreamConfig {
            id: "cam1".to_string(),
            url: "rtsp://example.com/cam1".to_string(),
            enabled: true,
            output_subdir: None,
        }];

        // Create empty stream directory
        let stream_dir = temp_dir.path().join("cam1");
        std::fs::create_dir(&stream_dir).unwrap();

        let manager = DiskManager::new(config);

        let result = manager.get_stream_storage_info("cam1").await;
        assert!(result.is_ok());

        let info = result.unwrap().unwrap();
        assert_eq!(info.total_files, 0);
        assert_eq!(info.total_size, 0);
        assert_eq!(info.estimated_duration, 0);
    }

    /// Tests get_all_streams_storage_info with multiple streams
    #[tokio::test]
    async fn test_get_all_streams_storage_info() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.segment_duration = 10;
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
            crate::config::StreamConfig {
                id: "cam3".to_string(),
                url: "rtsp://example.com/cam3".to_string(),
                enabled: false, // Disabled stream
                output_subdir: None,
            },
        ];

        // Create directories for cam1 and cam2 only
        let cam1_dir = temp_dir.path().join("cam1");
        let cam2_dir = temp_dir.path().join("cam2");
        std::fs::create_dir(&cam1_dir).unwrap();
        std::fs::create_dir(&cam2_dir).unwrap();
        std::fs::write(cam1_dir.join("video.mp4"), b"cam1 video").unwrap();
        std::fs::write(cam2_dir.join("video.mp4"), b"cam2 video").unwrap();

        let manager = DiskManager::new(config);

        let result = manager.get_all_streams_storage_info().await;
        assert!(result.is_ok());

        let infos = result.unwrap();
        // Should only include enabled streams with existing directories
        assert_eq!(infos.len(), 2);
    }

    /// Tests get_all_streams_storage_info with no streams configured
    #[tokio::test]
    async fn test_get_all_streams_storage_info_no_streams() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.streams = vec![];

        let manager = DiskManager::new(config);

        let result = manager.get_all_streams_storage_info().await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    /// Tests get_all_streams_storage_info skips disabled streams
    #[tokio::test]
    async fn test_get_all_streams_storage_info_skips_disabled() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.streams = vec![
            crate::config::StreamConfig {
                id: "disabled".to_string(),
                url: "rtsp://example.com/disabled".to_string(),
                enabled: false,
                output_subdir: None,
            },
        ];

        // Create directory even though disabled
        let stream_dir = temp_dir.path().join("disabled");
        std::fs::create_dir(&stream_dir).unwrap();
        std::fs::write(stream_dir.join("video.mp4"), b"video").unwrap();

        let manager = DiskManager::new(config);

        let result = manager.get_all_streams_storage_info().await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    /// Tests get_all_streams_storage_info handles missing directories
    #[tokio::test]
    async fn test_get_all_streams_storage_info_missing_dirs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.streams = vec![
            crate::config::StreamConfig {
                id: "missing1".to_string(),
                url: "rtsp://example.com/missing1".to_string(),
                enabled: true,
                output_subdir: None,
            },
            crate::config::StreamConfig {
                id: "existing".to_string(),
                url: "rtsp://example.com/existing".to_string(),
                enabled: true,
                output_subdir: None,
            },
        ];

        // Only create one directory
        let existing_dir = temp_dir.path().join("existing");
        std::fs::create_dir(&existing_dir).unwrap();
        std::fs::write(existing_dir.join("video.mp4"), b"video").unwrap();

        let manager = DiskManager::new(config);

        let result = manager.get_all_streams_storage_info().await;
        assert!(result.is_ok());

        let infos = result.unwrap();
        // Only the existing directory should be in results
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].stream_id, "existing");
    }

    /// Tests StreamStorageInfo directory field is correct
    #[tokio::test]
    async fn test_stream_storage_info_directory_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.output_dir = temp_dir.path().to_path_buf();
        config.streams = vec![crate::config::StreamConfig {
            id: "cam1".to_string(),
            url: "rtsp://example.com/cam1".to_string(),
            enabled: true,
            output_subdir: Some("custom_path".to_string()),
        }];

        // Create custom path directory
        let stream_dir = temp_dir.path().join("custom_path");
        std::fs::create_dir(&stream_dir).unwrap();
        std::fs::write(stream_dir.join("video.mp4"), b"video").unwrap();

        let manager = DiskManager::new(config);

        let result = manager.get_stream_storage_info("cam1").await;
        assert!(result.is_ok());

        let info = result.unwrap().unwrap();
        assert_eq!(info.directory, temp_dir.path().join("custom_path"));
    }
}