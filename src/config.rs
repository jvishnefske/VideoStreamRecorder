
# src/config.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub stream_url: String,
    pub output_dir: PathBuf,
    pub segment_duration: u32, // seconds
    pub max_disk_usage_percent: f64,
    pub cleanup_check_interval: u64, // seconds
    pub server_port: u16,
    pub server_host: String,
    pub auto_start: bool,
    pub max_retries: u32,
    pub retry_delay: u64, // seconds
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stream_url: "https://example.com/stream".to_string(),
            output_dir: PathBuf::from("./recordings"),
            segment_duration: 10,
            max_disk_usage_percent: 90.0,
            cleanup_check_interval: 60,
            server_port: 8080,
            server_host: "0.0.0.0".to_string(),
            auto_start: false,
            max_retries: 3,
            retry_delay: 5,
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
        
        // Ensure output directory exists
        std::fs::create_dir_all(&config.output_dir)?;
        
        Ok(config)
    }

    fn load_from_env(&mut self) {
        if let Ok(url) = env::var("STREAM_URL") {
            self.stream_url = url;
        }
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
    }
}

