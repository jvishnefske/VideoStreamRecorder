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
    use std::sync::Mutex;

    // Mutex to serialize environment variable tests (env vars are global state)
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

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

    /// FR-16: JSON configuration file support - Tests loading from file
    #[test]
    fn test_config_load_from_json_file() {
        use tempfile::NamedTempFile;
        use std::io::Write;

        let config_json = r#"{
            "stream_url": "rtsp://file.com/stream",
            "streams": [],
            "output_dir": "/tmp/test_recordings",
            "segment_duration": 15,
            "max_disk_usage_percent": 80.0,
            "cleanup_check_interval": 30,
            "server_port": 9000,
            "server_host": "127.0.0.1",
            "auto_start": false,
            "max_retries": 3,
            "retry_delay": 10,
            "ffmpeg_analyzeduration": 5000000,
            "ffmpeg_probesize": 5000000,
            "rtsp_transport": "udp"
        }"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(config_json.as_bytes()).unwrap();

        let config = Config::load(Some(temp_file.path().to_str().unwrap())).unwrap();

        assert_eq!(config.segment_duration, 15);
        assert_eq!(config.max_disk_usage_percent, 80.0);
        assert_eq!(config.server_port, 9000);
        assert_eq!(config.rtsp_transport, "udp");
    }

    /// FR-16: Tests that Config::load works without a config file
    #[test]
    fn test_config_load_without_file() {
        // Clear relevant environment variables to ensure defaults are used
        std::env::remove_var("STREAM_URL");
        std::env::remove_var("OUTPUT_DIR");
        std::env::remove_var("SEGMENT_DURATION");
        std::env::remove_var("MAX_DISK_USAGE_PERCENT");
        std::env::remove_var("CLEANUP_CHECK_INTERVAL");
        std::env::remove_var("PORT");
        std::env::remove_var("HOST");
        std::env::remove_var("AUTO_START");
        std::env::remove_var("STREAM_IDS");
        std::env::remove_var("FFMPEG_ANALYZEDURATION");
        std::env::remove_var("FFMPEG_PROBESIZE");
        std::env::remove_var("RTSP_TRANSPORT");

        let config = Config::load(None).unwrap();

        // Should use defaults
        assert_eq!(config.segment_duration, 10);
        assert_eq!(config.max_disk_usage_percent, 90.0);
        assert_eq!(config.server_port, 8080);
        assert_eq!(config.rtsp_transport, "tcp");
    }

    /// FR-15: Tests environment variable loading for single stream
    #[test]
    fn test_load_from_env_stream_url() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("STREAM_URL", "rtsp://env.com/stream");
        config.load_from_env();
        std::env::remove_var("STREAM_URL");

        assert_eq!(config.stream_url, Some("rtsp://env.com/stream".to_string()));
    }

    /// FR-15: Tests environment variable loading for output directory
    #[test]
    fn test_load_from_env_output_dir() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("OUTPUT_DIR", "/custom/output");
        config.load_from_env();
        std::env::remove_var("OUTPUT_DIR");

        assert_eq!(config.output_dir, PathBuf::from("/custom/output"));
    }

    /// FR-15: Tests environment variable loading for segment duration
    #[test]
    fn test_load_from_env_segment_duration() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("SEGMENT_DURATION", "30");
        config.load_from_env();
        std::env::remove_var("SEGMENT_DURATION");

        assert_eq!(config.segment_duration, 30);
    }

    /// FR-15: Tests invalid segment duration is ignored
    #[test]
    fn test_load_from_env_invalid_segment_duration() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();
        let original = config.segment_duration;

        std::env::set_var("SEGMENT_DURATION", "not_a_number");
        config.load_from_env();
        std::env::remove_var("SEGMENT_DURATION");

        assert_eq!(config.segment_duration, original);
    }

    /// FR-15: Tests environment variable loading for max disk usage
    #[test]
    fn test_load_from_env_max_disk_usage() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("MAX_DISK_USAGE_PERCENT", "75.5");
        config.load_from_env();
        std::env::remove_var("MAX_DISK_USAGE_PERCENT");

        assert!((config.max_disk_usage_percent - 75.5).abs() < 0.001);
    }

    /// FR-15: Tests environment variable loading for cleanup interval
    #[test]
    fn test_load_from_env_cleanup_interval() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("CLEANUP_CHECK_INTERVAL", "120");
        config.load_from_env();
        std::env::remove_var("CLEANUP_CHECK_INTERVAL");

        assert_eq!(config.cleanup_check_interval, 120);
    }

    /// FR-15: Tests environment variable loading for port
    #[test]
    fn test_load_from_env_port() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("PORT", "3000");
        config.load_from_env();
        std::env::remove_var("PORT");

        assert_eq!(config.server_port, 3000);
    }

    /// FR-15: Tests environment variable loading for host
    #[test]
    fn test_load_from_env_host() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("HOST", "localhost");
        config.load_from_env();
        std::env::remove_var("HOST");

        assert_eq!(config.server_host, "localhost");
    }

    /// FR-15: Tests environment variable loading for auto_start true
    #[test]
    fn test_load_from_env_auto_start_true() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();
        config.auto_start = false;

        std::env::set_var("AUTO_START", "true");
        config.load_from_env();
        std::env::remove_var("AUTO_START");

        assert!(config.auto_start);
    }

    /// FR-15: Tests environment variable loading for auto_start false
    #[test]
    fn test_load_from_env_auto_start_false() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("AUTO_START", "false");
        config.load_from_env();
        std::env::remove_var("AUTO_START");

        assert!(!config.auto_start);
    }

    /// FR-15: Tests environment variable loading for FFmpeg analyzeduration
    #[test]
    fn test_load_from_env_ffmpeg_analyzeduration() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("FFMPEG_ANALYZEDURATION", "20000000");
        config.load_from_env();
        std::env::remove_var("FFMPEG_ANALYZEDURATION");

        assert_eq!(config.ffmpeg_analyzeduration, 20_000_000);
    }

    /// FR-15: Tests environment variable loading for FFmpeg probesize
    #[test]
    fn test_load_from_env_ffmpeg_probesize() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("FFMPEG_PROBESIZE", "15000000");
        config.load_from_env();
        std::env::remove_var("FFMPEG_PROBESIZE");

        assert_eq!(config.ffmpeg_probesize, 15_000_000);
    }

    /// FR-15: Tests environment variable loading for RTSP transport tcp
    #[test]
    fn test_load_from_env_rtsp_transport_tcp() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();
        config.rtsp_transport = "udp".to_string(); // Start with non-default

        std::env::set_var("RTSP_TRANSPORT", "tcp");
        config.load_from_env();
        std::env::remove_var("RTSP_TRANSPORT");

        assert_eq!(config.rtsp_transport, "tcp");
    }

    /// FR-15: Tests environment variable loading for RTSP transport udp
    #[test]
    fn test_load_from_env_rtsp_transport_udp() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("RTSP_TRANSPORT", "udp");
        config.load_from_env();
        std::env::remove_var("RTSP_TRANSPORT");

        assert_eq!(config.rtsp_transport, "udp");
    }

    /// FR-15: Tests invalid RTSP transport is ignored
    #[test]
    fn test_load_from_env_rtsp_transport_invalid() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();
        let original = config.rtsp_transport.clone();

        std::env::set_var("RTSP_TRANSPORT", "invalid");
        config.load_from_env();
        std::env::remove_var("RTSP_TRANSPORT");

        assert_eq!(config.rtsp_transport, original);
    }

    /// FR-15: Tests multi-stream environment variable loading
    #[test]
    fn test_load_streams_from_env() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("STREAM_IDS", "cam1,cam2");
        std::env::set_var("STREAM_URL_CAM1", "rtsp://example.com/cam1");
        std::env::set_var("STREAM_URL_CAM2", "rtsp://example.com/cam2");
        std::env::set_var("STREAM_ENABLED_CAM1", "true");
        std::env::set_var("STREAM_ENABLED_CAM2", "false");
        std::env::set_var("STREAM_SUBDIR_CAM1", "camera1");

        config.load_from_env();

        std::env::remove_var("STREAM_IDS");
        std::env::remove_var("STREAM_URL_CAM1");
        std::env::remove_var("STREAM_URL_CAM2");
        std::env::remove_var("STREAM_ENABLED_CAM1");
        std::env::remove_var("STREAM_ENABLED_CAM2");
        std::env::remove_var("STREAM_SUBDIR_CAM1");

        assert_eq!(config.streams.len(), 2);
        assert_eq!(config.streams[0].id, "cam1");
        assert_eq!(config.streams[1].id, "cam2");
        assert!(config.streams[0].enabled);
        assert!(!config.streams[1].enabled);
        assert_eq!(config.streams[0].output_subdir, Some("camera1".to_string()));
    }

    /// FR-15: Tests multi-stream with empty stream ID is skipped
    #[test]
    fn test_load_streams_from_env_empty_id_skipped() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("STREAM_IDS", "cam1,,cam2");
        std::env::set_var("STREAM_URL_CAM1", "rtsp://example.com/cam1");
        std::env::set_var("STREAM_URL_CAM2", "rtsp://example.com/cam2");

        config.load_from_env();

        std::env::remove_var("STREAM_IDS");
        std::env::remove_var("STREAM_URL_CAM1");
        std::env::remove_var("STREAM_URL_CAM2");

        // Empty stream ID should be skipped, so we get exactly 2 streams
        assert_eq!(config.streams.len(), 2);
    }

    /// FR-15: Tests multi-stream without URL is skipped
    #[test]
    fn test_load_streams_from_env_missing_url() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("STREAM_IDS", "cam1,cam2");
        std::env::set_var("STREAM_URL_CAM1", "rtsp://example.com/cam1");
        // Intentionally not setting STREAM_URL_CAM2

        config.load_from_env();

        std::env::remove_var("STREAM_IDS");
        std::env::remove_var("STREAM_URL_CAM1");

        // cam2 should be skipped due to missing URL
        assert_eq!(config.streams.len(), 1);
        assert_eq!(config.streams[0].id, "cam1");
    }

    /// FR-15: Tests stream defaults to enabled when STREAM_ENABLED not set
    #[test]
    fn test_load_streams_from_env_default_enabled() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();

        std::env::set_var("STREAM_IDS", "cam1");
        std::env::set_var("STREAM_URL_CAM1", "rtsp://example.com/cam1");

        config.load_from_env();

        std::env::remove_var("STREAM_IDS");
        std::env::remove_var("STREAM_URL_CAM1");

        assert_eq!(config.streams.len(), 1);
        assert!(config.streams[0].enabled); // Default is enabled
    }

    /// FR-15: Tests empty STREAM_IDS doesn't replace existing streams
    #[test]
    fn test_load_streams_from_env_empty_ids() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let mut config = Config::default();
        config.streams = vec![StreamConfig {
            id: "existing".to_string(),
            url: "rtsp://existing.com/stream".to_string(),
            enabled: true,
            output_subdir: None,
        }];

        std::env::set_var("STREAM_IDS", "");
        config.load_from_env();
        std::env::remove_var("STREAM_IDS");

        // Existing streams should remain
        assert_eq!(config.streams.len(), 1);
        assert_eq!(config.streams[0].id, "existing");
    }

    /// FR-17: Tests migrate_legacy_config when streams is empty but has no stream_url
    #[test]
    fn test_migrate_legacy_no_url_no_streams() {
        let mut config = Config::default();
        config.stream_url = None;
        config.streams = vec![];

        config.migrate_legacy_config();

        assert!(config.streams.is_empty());
        assert!(config.stream_url.is_none());
    }

    /// FR-17: Tests migrate_legacy_config with all disabled streams
    #[test]
    fn test_migrate_legacy_all_disabled_streams() {
        let mut config = Config::default();
        config.stream_url = None;
        config.streams = vec![
            StreamConfig {
                id: "disabled1".to_string(),
                url: "rtsp://disabled1.com/stream".to_string(),
                enabled: false,
                output_subdir: None,
            },
            StreamConfig {
                id: "disabled2".to_string(),
                url: "rtsp://disabled2.com/stream".to_string(),
                enabled: false,
                output_subdir: None,
            },
        ];

        config.migrate_legacy_config();

        // stream_url should remain None since no enabled streams
        assert!(config.stream_url.is_none());
    }

    /// Tests config load fails with invalid JSON
    #[test]
    fn test_config_load_invalid_json() {
        use tempfile::NamedTempFile;
        use std::io::Write;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"{ invalid json }").unwrap();

        let result = Config::load(Some(temp_file.path().to_str().unwrap()));

        assert!(result.is_err());
    }

    /// Tests config load fails with non-existent file
    #[test]
    fn test_config_load_nonexistent_file() {
        let result = Config::load(Some("/nonexistent/path/config.json"));

        assert!(result.is_err());
    }

    /// Tests StreamConfig output_subdir default
    #[test]
    fn test_stream_config_output_subdir_default() {
        let json = r#"{
            "id": "test",
            "url": "rtsp://test.com",
            "enabled": true
        }"#;

        let stream: StreamConfig = serde_json::from_str(json).unwrap();

        assert!(stream.output_subdir.is_none());
    }
}
