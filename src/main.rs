use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tokio::signal;
use tracing::{info, warn};

use crate::recorder::Recorder;

mod config;
mod disk_manager;
mod disk_manager_ext;
mod error;
mod recorder;
mod server;

use config::Config;
use disk_manager::DiskManager;
use recorder::VideoRecorder;
use server::Server;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Configuration file path
    #[arg(short, long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    let args = Args::parse();
    let config = Config::load(args.config.as_deref())?;

    info!("Starting video stream recorder with config: {:?}", config);

    // Initialize FFmpeg
    ffmpeg_next::init().map_err(|e| anyhow::anyhow!("Failed to initialize FFmpeg: {}", e))?;

    // Create shared components
    let disk_manager = Arc::new(DiskManager::new(config.clone()));
    let recorder = Arc::new(VideoRecorder::new(config.clone(), disk_manager.clone()));

    // Initialize stream recorders for multi-stream support
    recorder.initialize_streams().await;

    // Start disk monitoring task
    let disk_task = {
        let disk_manager = disk_manager.clone();
        tokio::spawn(async move {
            disk_manager.start_monitoring().await;
        })
    };

    // Start web server
    let server = Server::new(config.clone(), recorder.clone(), disk_manager.clone());
    let server_task = tokio::spawn(async move { server.start().await });

    // Auto-start recording if configured
    if config.auto_start {
        info!("Auto-starting recording");
        if let Err(e) = recorder.start().await {
            warn!("Failed to auto-start recording: {}", e);
        }
    }

    // Wait for shutdown signal
    info!("Application started. Press Ctrl+C to shutdown.");
    signal::ctrl_c().await?;
    info!("Shutdown signal received");

    // Graceful shutdown
    recorder.stop().await?;
    disk_task.abort();
    server_task.abort();

    info!("Application shut down gracefully");
    Ok(())
}
