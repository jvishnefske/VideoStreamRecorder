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
