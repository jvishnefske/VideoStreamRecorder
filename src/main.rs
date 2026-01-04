//! Video Stream Recorder Binary
//!
//! A 12-factor app for recording RTSP/HTTPS webcam streams with
//! configurable segmentation, disk management, and a web API.

use anyhow::Result;
use clap::Parser;
use tokio::signal;
use tracing::info;

use video_stream_recorder::{
    auto_start_if_configured, initialize_app, start_disk_monitoring, Recorder, Server,
};

/// Command-line arguments for the video stream recorder
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Configuration file path (JSON format)
    #[arg(short, long)]
    pub config: Option<String>,
}

/// Run the application with the given arguments
///
/// This function:
/// 1. Initializes tracing for logging
/// 2. Loads configuration
/// 3. Initializes FFmpeg
/// 4. Creates and starts all application components
/// 5. Waits for shutdown signal
/// 6. Gracefully shuts down
///
/// # Arguments
///
/// * `args` - The parsed command-line arguments
///
/// # Returns
///
/// Returns Ok(()) on successful shutdown, or an error if any component fails.
pub async fn run(args: Args) -> Result<()> {
    // Initialize FFmpeg
    ffmpeg_next::init().map_err(|e| anyhow::anyhow!("Failed to initialize FFmpeg: {}", e))?;

    // Initialize application components
    let components = initialize_app(args.config.as_deref()).await?;

    info!(
        "Starting video stream recorder with config: {:?}",
        components.config
    );

    // Start disk monitoring task
    let disk_task = start_disk_monitoring(components.disk_manager.clone());

    // Start web server
    let server = Server::new(
        components.config.clone(),
        components.recorder.clone(),
        components.disk_manager.clone(),
    );
    let server_task = tokio::spawn(async move { server.start().await });

    // Auto-start recording if configured
    auto_start_if_configured(&components.config, &components.recorder).await?;

    // Wait for shutdown signal
    info!("Application started. Press Ctrl+C to shutdown.");
    signal::ctrl_c().await?;
    info!("Shutdown signal received");

    // Graceful shutdown - ignore errors on stop since we're shutting down anyway
    let _ = components.recorder.stop().await;
    disk_task.abort();
    server_task.abort();

    info!("Application shut down gracefully");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    run(args).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests Args parsing with no arguments
    #[test]
    fn test_args_default() {
        let args = Args::parse_from(["test"]);
        assert!(args.config.is_none());
    }

    /// Tests Args parsing with config file
    #[test]
    fn test_args_with_config() {
        let args = Args::parse_from(["test", "-c", "/path/to/config.json"]);
        assert_eq!(args.config, Some("/path/to/config.json".to_string()));
    }

    /// Tests Args parsing with long form config
    #[test]
    fn test_args_with_long_config() {
        let args = Args::parse_from(["test", "--config", "/another/path.json"]);
        assert_eq!(args.config, Some("/another/path.json".to_string()));
    }

    /// Tests Args debug trait
    #[test]
    fn test_args_debug() {
        let args = Args {
            config: Some("test.json".to_string()),
        };
        let debug_str = format!("{:?}", args);
        assert!(debug_str.contains("Args"));
        assert!(debug_str.contains("test.json"));
    }
}
