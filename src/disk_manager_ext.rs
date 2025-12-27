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
}