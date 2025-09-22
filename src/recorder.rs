# src/recorder.rs
use crate::config::Config;
use crate::disk_manager::DiskManager;
use crate::error::RecorderError;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ffmpeg_next as ffmpeg;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct RecordingStats {
    pub started_at: Option<DateTime<Utc>>,
    pub files_recorded: u64,
    pub total_duration: u64, // seconds
    pub last_file: Option<PathBuf>,
}

#[async_trait]
pub trait Recorder {
    async fn start(&self) -> Result<(), RecorderError>;
    async fn stop(&self) -> Result<(), RecorderError>;
    async fn is_recording(&self) -> bool;
    async fn get_stats(&self) -> RecordingStats;
}

pub struct VideoRecorder {
    config: Config,
    disk_manager: Arc<DiskManager>,
    is_recording: AtomicBool,
    stats: Arc<RwLock<RecordingStats>>,
    files_recorded: AtomicU64,
}

impl VideoRecorder {
    pub fn new(config: Config, disk_manager: Arc<DiskManager>) -> Self {
        Self {
            config,
            disk_manager,
            is_recording: AtomicBool::new(false),
            stats: Arc::new(RwLock::new(RecordingStats {
                started_at: None,
                files_recorded: 0,
                total_duration: 0,
                last_file: None,
            })),
            files_recorded: AtomicU64::new(0),
        }
    }

    async fn record_stream(&self) -> Result<(), RecorderError> {
        let mut retry_count = 0;
        
        while self.is_recording.load(Ordering::Relaxed) && retry_count < self.config.max_retries {
            match self.record_stream_once().await {
                Ok(_) => {
                    info!("Stream recording completed normally");
                    break;
                }
                Err(e) => {
                    retry_count += 1;
                    error!("Recording failed (attempt {}/{}): {}", retry_count, self.config.max_retries, e);
                    
                    if retry_count < self.config.max_retries {
                        warn!("Retrying in {} seconds...", self.config.retry_delay);
                        tokio::time::sleep(tokio::time::Duration::from_secs(self.config.retry_delay)).await;
                    }
                }
            }
        }
        
        if retry_count >= self.config.max_retries {
            error!("Max retries exceeded, stopping recording");
        }
        
        Ok(())
    }

    async fn record_stream_once(&self) -> Result<(), RecorderError> {
        // Check disk space before starting
        if !self.disk_manager.has_space().await? {
            return Err(RecorderError::DiskFull);
        }

        info!("Connecting to stream: {}", self.config.stream_url);
        
        // Open input stream
        let mut input = ffmpeg::format::input(&self.config.stream_url)?;
        let video_stream_index = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or_else(|| RecorderError::StreamConnection("No video stream found".to_string()))?
            .index();

        let video_stream = input.stream(video_stream_index).unwrap();
        let video_codec_parameters = video_stream.parameters();
        
        info!("Video stream found - codec: {:?}, {}x{}", 
              video_codec_parameters.id(),
              video_codec_parameters.width(),
              video_codec_parameters.height());

        let mut segment_index = 0;
        let start_time = Utc::now();
        
        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.started_at = Some(start_time);
        }

        while self.is_recording.load(Ordering::Relaxed) {
            let segment_path = self.generate_segment_path(segment_index);
            
            info!("Recording segment: {:?}", segment_path);
            
            // Create output for this segment
            let mut output = ffmpeg::format::output(&segment_path)?;
            
            // Add video stream to output
            let mut out_stream = output.add_stream(ffmpeg::encoder::find(ffmpeg::codec::Id::H264))?;
            out_stream.set_parameters(&video_codec_parameters);
            
            // Write header
            output.write_header()?;
            
            let segment_start = std::time::Instant::now();
            let mut packet_count = 0;
            
            // Record for segment duration
            while self.is_recording.load(Ordering::Relaxed) 
                && segment_start.elapsed().as_secs() < self.config.segment_duration as u64 {
                
                for (stream, mut packet) in input.packets() {
                    if stream.index() == video_stream_index {
                        packet.set_stream(0);
                        packet.write_interleaved(&mut output)?;
                        packet_count += 1;
                        
                        // Check if segment duration reached
                        if segment_start.elapsed().as_secs() >= self.config.segment_duration as u64 {
                            break;
                        }
                    }
                }
                
                // Prevent tight loop if no packets
                if packet_count == 0 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
            
            // Write trailer for this segment
            output.write_trailer()?;
            
            info!("Completed segment {} with {} packets", segment_index, packet_count);
            
            // Update stats
            {
                let mut stats = self.stats.write().await;
                stats.files_recorded += 1;
                stats.total_duration += self.config.segment_duration as u64;
                stats.last_file = Some(segment_path);
            }
            
            self.files_recorded.fetch_add(1, Ordering::Relaxed);
            segment_index += 1;
            
            // Check disk space periodically
            if segment_index % 10 == 0 && !self.disk_manager.has_space().await? {
                warn!("Disk space low, attempting cleanup");
                self.disk_manager.cleanup_old_files().await?;
                
                if !self.disk_manager.has_space().await? {
                    return Err(RecorderError::DiskFull);
                }
            }
        }
        
        Ok(())
    }

    fn generate_segment_path(&self, index: u32) -> PathBuf {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let filename = format!("segment_{}_{:06}.mp4", timestamp, index);
        self.config.output_dir.join(filename)
    }
}

#[async_trait]
impl Recorder for VideoRecorder {
    async fn start(&self) -> Result<(), RecorderError> {
        if self.is_recording.compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed).is_err() {
            return Err(RecorderError::AlreadyRecording);
        }
        
        info!("Starting video recording from: {}", self.config.stream_url);
        
        let recorder = Arc::new(self.clone());
        tokio::spawn(async move {
            if let Err(e) = recorder.record_stream().await {
                error!("Recording failed: {}", e);
            }
            recorder.is_recording.store(false, Ordering::Relaxed);
        });
        
        Ok(())
    }

    async fn stop(&self) -> Result<(), RecorderError> {
        if !self.is_recording.load(Ordering::Relaxed) {
            return Err(RecorderError::NotRecording);
        }
        
        info!("Stopping video recording");
        self.is_recording.store(false, Ordering::Relaxed);
        
        // Wait a bit for graceful shutdown
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        
        Ok(())
    }

    async fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::Relaxed)
    }

    async fn get_stats(&self) -> RecordingStats {
        self.stats.read().await.clone()
    }
}

impl Clone for VideoRecorder {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            disk_manager: self.disk_manager.clone(),
            is_recording: AtomicBool::new(self.is_recording.load(Ordering::Relaxed)),
            stats: self.stats.clone(),
            files_recorded: AtomicU64::new(self.files_recorded.load(Ordering::Relaxed)),
        }
    }
}

