use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    pub id: String,
    pub url: String,
    pub enabled: bool,
    #[serde(default)]
    pub output_subdir: Option<String>, // Optional subdirectory for this stream
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // Legacy single stream support (for backward compatibility)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_url: Option<String>,

    // New multi-stream support
    #[serde(default)]
    pub streams: Vec<StreamConfig>,

    pub output_dir: PathBuf,
    pub segment_duration: u32, // seconds
    pub max_disk_usage_percent: f64,
    pub cleanup_check_interval: u64, // seconds
    pub server_port: u16,
    pub server_host: String,
    pub auto_start: bool,
    pub max_retries: u32,
    pub retry_delay: u64, // seconds

    // FFmpeg configuration options
    pub ffmpeg_analyzeduration: u64, // microseconds
    pub ffmpeg_probesize: u64, // bytes
    pub rtsp_transport: String, // "tcp" or "udp"
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // No default streams - user must configure
            stream_url: None,
            streams: vec![],
            output_dir: PathBuf::from("./recordings"),
            segment_duration: 10,
            max_disk_usage_percent: 90.0,
            cleanup_check_interval: 60,
            server_port: 8080,
            server_host: "0.0.0.0".to_string(),
            auto_start: true,
            max_retries: 0, // 0 = infinite retries
            retry_delay: 5,

            // FFmpeg defaults optimized for HEVC RTSP streams
            ffmpeg_analyzeduration: 10_000_000, // 10 seconds in microseconds
            ffmpeg_probesize: 10_000_000, // 10MB in bytes
            rtsp_transport: "tcp".to_string(), // TCP is more reliable for RTSP
        }
    }
}

impl Config {
    pub fn load(config_path: Option<&str>) -> Result<Self> {
        let mut config = if let Some(path) = config_path {
            let content = std::fs::read_to_string(path)?;
            serde_json::from_str(&content)?
        } else {
            Self::default()
        };

        // Override with environment variables (12-factor principle)
        config.load_from_env();

        // Handle backward compatibility: if stream_url is set but streams is empty,
        // create a default stream from the legacy URL
        config.migrate_legacy_config();

        // Ensure output directory exists
        std::fs::create_dir_all(&config.output_dir)?;

        // Create stream-specific output directories
        for stream in &config.streams {
            if stream.enabled {
                let stream_dir = config.get_stream_output_dir(&stream.id);
                std::fs::create_dir_all(&stream_dir)?;
            }
        }

        Ok(config)
    }

    fn migrate_legacy_config(&mut self) {
        // If we have a legacy stream_url but no streams configured, create a default stream
        if let Some(ref url) = self.stream_url {
            if self.streams.is_empty() {
                self.streams.push(StreamConfig {
                    id: "default".to_string(),
                    url: url.clone(),
                    enabled: true,
                    output_subdir: None,
                });
            }
        }

        // If we have streams but no legacy URL, use the first enabled stream as legacy
        if self.stream_url.is_none() && !self.streams.is_empty() {
            if let Some(first_stream) = self.streams.iter().find(|s| s.enabled) {
                self.stream_url = Some(first_stream.url.clone());
            }
        }
    }

    pub fn get_stream_output_dir(&self, stream_id: &str) -> PathBuf {
        if let Some(stream) = self.streams.iter().find(|s| s.id == stream_id) {
            if let Some(ref subdir) = stream.output_subdir {
                return self.output_dir.join(subdir);
            }
        }
        // Default: use stream ID as subdirectory
        self.output_dir.join(stream_id)
    }

    pub fn get_enabled_streams(&self) -> Vec<&StreamConfig> {
        self.streams.iter().filter(|s| s.enabled).collect()
    }

    fn load_from_env(&mut self) {
        // Legacy single stream support
        if let Ok(url) = env::var("STREAM_URL") {
            self.stream_url = Some(url);
        }

        // Multi-stream environment variable support
        self.load_streams_from_env();

        // Other configuration
        if let Ok(dir) = env::var("OUTPUT_DIR") {
            self.output_dir = PathBuf::from(dir);
        }
        if let Ok(duration) = env::var("SEGMENT_DURATION") {
            if let Ok(val) = duration.parse() {
                self.segment_duration = val;
            }
        }
        if let Ok(usage) = env::var("MAX_DISK_USAGE_PERCENT") {
            if let Ok(val) = usage.parse() {
                self.max_disk_usage_percent = val;
            }
        }
        if let Ok(interval) = env::var("CLEANUP_CHECK_INTERVAL") {
            if let Ok(val) = interval.parse() {
                self.cleanup_check_interval = val;
            }
        }
        if let Ok(port) = env::var("PORT") {
            if let Ok(val) = port.parse() {
                self.server_port = val;
            }
        }
        if let Ok(host) = env::var("HOST") {
            self.server_host = host;
        }
        if let Ok(auto_start) = env::var("AUTO_START") {
            self.auto_start = auto_start.to_lowercase() == "true";
        }

        // FFmpeg configuration options
        if let Ok(analyzeduration) = env::var("FFMPEG_ANALYZEDURATION") {
            if let Ok(val) = analyzeduration.parse() {
                self.ffmpeg_analyzeduration = val;
            }
        }
        if let Ok(probesize) = env::var("FFMPEG_PROBESIZE") {
            if let Ok(val) = probesize.parse() {
                self.ffmpeg_probesize = val;
            }
        }
        if let Ok(transport) = env::var("RTSP_TRANSPORT") {
            if transport == "tcp" || transport == "udp" {
                self.rtsp_transport = transport;
            }
        }
    }

    fn load_streams_from_env(&mut self) {
        // Support multiple streams via environment variables
        // Format: STREAM_IDS="cam1,cam2,cam3" and STREAM_URL_CAM1="url1", etc.

        if let Ok(stream_ids) = env::var("STREAM_IDS") {
            let mut new_streams = Vec::new();

            for stream_id in stream_ids.split(',').map(|s| s.trim()) {
                if stream_id.is_empty() {
                    continue;
                }

                let url_var = format!("STREAM_URL_{}", stream_id.to_uppercase());
                let enabled_var = format!("STREAM_ENABLED_{}", stream_id.to_uppercase());
                let subdir_var = format!("STREAM_SUBDIR_{}", stream_id.to_uppercase());

                if let Ok(url) = env::var(&url_var) {
                    let enabled = env::var(&enabled_var)
                        .map(|v| v.to_lowercase() == "true")
                        .unwrap_or(true); // Default to enabled

                    let output_subdir = env::var(&subdir_var).ok();

                    new_streams.push(StreamConfig {
                        id: stream_id.to_string(),
                        url,
                        enabled,
                        output_subdir,
                    });
                }
            }

            if !new_streams.is_empty() {
                self.streams = new_streams;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let config = Config::default();

        assert_eq!(config.segment_duration, 10);
        assert_eq!(config.max_disk_usage_percent, 90.0);
        assert_eq!(config.cleanup_check_interval, 60);
        assert_eq!(config.server_port, 8080);
        assert_eq!(config.server_host, "0.0.0.0");
        assert!(config.auto_start);
        assert_eq!(config.max_retries, 0);
        assert_eq!(config.retry_delay, 5);
        assert_eq!(config.ffmpeg_analyzeduration, 10_000_000);
        assert_eq!(config.ffmpeg_probesize, 10_000_000);
        assert_eq!(config.rtsp_transport, "tcp");
        assert!(config.streams.is_empty());
        assert!(config.stream_url.is_none());
    }

    #[test]
    fn test_migrate_legacy_config_creates_stream_from_url() {
        let mut config = Config::default();
        config.stream_url = Some("rtsp://example.com/stream".to_string());
        config.streams = vec![];

        config.migrate_legacy_config();

        assert_eq!(config.streams.len(), 1);
        assert_eq!(config.streams[0].id, "default");
        assert_eq!(config.streams[0].url, "rtsp://example.com/stream");
        assert!(config.streams[0].enabled);
        assert!(config.streams[0].output_subdir.is_none());
    }

    #[test]
    fn test_migrate_legacy_config_does_not_overwrite_existing_streams() {
        let mut config = Config::default();
        config.stream_url = Some("rtsp://example.com/stream".to_string());
        config.streams = vec![StreamConfig {
            id: "existing".to_string(),
            url: "rtsp://other.com/stream".to_string(),
            enabled: true,
            output_subdir: None,
        }];

        config.migrate_legacy_config();

        assert_eq!(config.streams.len(), 1);
        assert_eq!(config.streams[0].id, "existing");
    }

    #[test]
    fn test_migrate_legacy_config_sets_stream_url_from_first_enabled() {
        let mut config = Config::default();
        config.stream_url = None;
        config.streams = vec![
            StreamConfig {
                id: "disabled".to_string(),
                url: "rtsp://disabled.com/stream".to_string(),
                enabled: false,
                output_subdir: None,
            },
            StreamConfig {
                id: "enabled".to_string(),
                url: "rtsp://enabled.com/stream".to_string(),
                enabled: true,
                output_subdir: None,
            },
        ];

        config.migrate_legacy_config();

        assert_eq!(config.stream_url, Some("rtsp://enabled.com/stream".to_string()));
    }

    #[test]
    fn test_get_stream_output_dir_with_custom_subdir() {
        let mut config = Config::default();
        config.output_dir = PathBuf::from("/recordings");
        config.streams = vec![StreamConfig {
            id: "cam1".to_string(),
            url: "rtsp://example.com/cam1".to_string(),
            enabled: true,
            output_subdir: Some("custom_subdir".to_string()),
        }];

        let output_dir = config.get_stream_output_dir("cam1");

        assert_eq!(output_dir, PathBuf::from("/recordings/custom_subdir"));
    }

    #[test]
    fn test_get_stream_output_dir_uses_stream_id_as_default() {
        let mut config = Config::default();
        config.output_dir = PathBuf::from("/recordings");
        config.streams = vec![StreamConfig {
            id: "cam2".to_string(),
            url: "rtsp://example.com/cam2".to_string(),
            enabled: true,
            output_subdir: None,
        }];

        let output_dir = config.get_stream_output_dir("cam2");

        assert_eq!(output_dir, PathBuf::from("/recordings/cam2"));
    }

    #[test]
    fn test_get_stream_output_dir_for_unknown_stream() {
        let mut config = Config::default();
        config.output_dir = PathBuf::from("/recordings");
        config.streams = vec![];

        let output_dir = config.get_stream_output_dir("unknown");

        assert_eq!(output_dir, PathBuf::from("/recordings/unknown"));
    }

    #[test]
    fn test_get_enabled_streams_filters_disabled() {
        let config = Config {
            streams: vec![
                StreamConfig {
                    id: "enabled1".to_string(),
                    url: "rtsp://example.com/1".to_string(),
                    enabled: true,
                    output_subdir: None,
                },
                StreamConfig {
                    id: "disabled".to_string(),
                    url: "rtsp://example.com/2".to_string(),
                    enabled: false,
                    output_subdir: None,
                },
                StreamConfig {
                    id: "enabled2".to_string(),
                    url: "rtsp://example.com/3".to_string(),
                    enabled: true,
                    output_subdir: None,
                },
            ],
            ..Config::default()
        };

        let enabled = config.get_enabled_streams();

        assert_eq!(enabled.len(), 2);
        assert_eq!(enabled[0].id, "enabled1");
        assert_eq!(enabled[1].id, "enabled2");
    }

    #[test]
    fn test_get_enabled_streams_returns_empty_when_all_disabled() {
        let config = Config {
            streams: vec![StreamConfig {
                id: "disabled".to_string(),
                url: "rtsp://example.com/1".to_string(),
                enabled: false,
                output_subdir: None,
            }],
            ..Config::default()
        };

        let enabled = config.get_enabled_streams();

        assert!(enabled.is_empty());
    }

    #[test]
    fn test_stream_config_serialization() {
        let stream = StreamConfig {
            id: "test".to_string(),
            url: "rtsp://example.com/stream".to_string(),
            enabled: true,
            output_subdir: Some("custom".to_string()),
        };

        let json = serde_json::to_string(&stream).unwrap();
        let deserialized: StreamConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, stream.id);
        assert_eq!(deserialized.url, stream.url);
        assert_eq!(deserialized.enabled, stream.enabled);
        assert_eq!(deserialized.output_subdir, stream.output_subdir);
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = Config {
            stream_url: Some("rtsp://test.com/stream".to_string()),
            streams: vec![StreamConfig {
                id: "cam1".to_string(),
                url: "rtsp://test.com/cam1".to_string(),
                enabled: true,
                output_subdir: None,
            }],
            output_dir: PathBuf::from("/test/recordings"),
            segment_duration: 30,
            max_disk_usage_percent: 85.0,
            cleanup_check_interval: 120,
            server_port: 9090,
            server_host: "127.0.0.1".to_string(),
            auto_start: false,
            max_retries: 5,
            retry_delay: 10,
            ffmpeg_analyzeduration: 20_000_000,
            ffmpeg_probesize: 20_000_000,
            rtsp_transport: "udp".to_string(),
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.segment_duration, config.segment_duration);
        assert_eq!(deserialized.max_disk_usage_percent, config.max_disk_usage_percent);
        assert_eq!(deserialized.server_port, config.server_port);
        assert_eq!(deserialized.rtsp_transport, config.rtsp_transport);
    }
}
