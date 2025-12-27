use crate::config::{Config, StreamConfig};
use crate::disk_manager::DiskManager;
use crate::error::RecorderError;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::Dictionary;
use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::{RwLock, Mutex};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

#[derive(Debug, Clone, serde::Serialize)]
pub struct RecordingStats {
    pub started_at: Option<DateTime<Utc>>,
    pub files_recorded: u64,
    pub total_duration: u64, // seconds
    pub last_file: Option<PathBuf>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamStats {
    pub stream_id: String,
    pub stream_url: String,
    pub is_recording: bool,
    pub stats: RecordingStats,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MultiStreamStats {
    pub streams: Vec<StreamStats>,
    pub total_files_recorded: u64,
    pub active_streams: u64,
}

#[async_trait]
pub trait Recorder {
    // Legacy single-stream methods (for backward compatibility)
    async fn start(&self) -> Result<(), RecorderError>;
    async fn stop(&self) -> Result<(), RecorderError>;
    async fn is_recording(&self) -> bool;
    async fn get_stats(&self) -> RecordingStats;

    // New multi-stream methods
    async fn start_stream(&self, stream_id: &str) -> Result<(), RecorderError>;
    async fn stop_stream(&self, stream_id: &str) -> Result<(), RecorderError>;
    async fn is_stream_recording(&self, stream_id: &str) -> bool;
    async fn get_stream_stats(&self, stream_id: &str) -> Option<StreamStats>;
    async fn get_multi_stream_stats(&self) -> MultiStreamStats;
    async fn list_streams(&self) -> Vec<String>;
}

// Single stream recorder (kept for internal use)
struct SingleStreamRecorder {
    stream_id: String,
    stream_url: String,
    config: Config,
    disk_manager: Arc<DiskManager>,
    is_recording: AtomicBool,
    stats: Arc<RwLock<RecordingStats>>,
    files_recorded: AtomicU64,
    task_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

// Multi-stream recorder that manages multiple single stream recorders
pub struct MultiStreamRecorder {
    config: Config,
    disk_manager: Arc<DiskManager>,
    stream_recorders: Arc<RwLock<HashMap<String, Arc<SingleStreamRecorder>>>>,
}

// Legacy alias for backward compatibility
pub type VideoRecorder = MultiStreamRecorder;

impl Clone for SingleStreamRecorder {
    fn clone(&self) -> Self {
        Self {
            stream_id: self.stream_id.clone(),
            stream_url: self.stream_url.clone(),
            config: self.config.clone(),
            disk_manager: self.disk_manager.clone(),
            is_recording: AtomicBool::new(self.is_recording.load(Ordering::Relaxed)),
            stats: self.stats.clone(),
            files_recorded: AtomicU64::new(self.files_recorded.load(Ordering::Relaxed)),
            task_handle: Arc::new(Mutex::new(None)), // New task handle for cloned instance
        }
    }
}

impl SingleStreamRecorder {
    fn new(stream_config: StreamConfig, config: Config, disk_manager: Arc<DiskManager>) -> Self {
        Self {
            stream_id: stream_config.id.clone(),
            stream_url: stream_config.url.clone(),
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
            task_handle: Arc::new(Mutex::new(None)),
        }
    }

    fn create_input_options(&self) -> Dictionary {
        let mut options = Dictionary::new();

        // Set analyzeduration (in microseconds)
        options.set("analyzeduration", &self.config.ffmpeg_analyzeduration.to_string());

        // Set probesize (in bytes)
        options.set("probesize", &self.config.ffmpeg_probesize.to_string());

        // Set RTSP transport protocol for RTSP URLs
        if self.stream_url.starts_with("rtsp://") {
            options.set("rtsp_transport", &self.config.rtsp_transport);
        }

        info!(
            "Using FFmpeg options for stream {}: analyzeduration={}μs, probesize={} bytes, rtsp_transport={}",
            self.stream_id,
            self.config.ffmpeg_analyzeduration,
            self.config.ffmpeg_probesize,
            self.config.rtsp_transport
        );

        options
    }

    async fn start_recording(&self) -> Result<(), RecorderError> {
        if self
            .is_recording
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return Err(RecorderError::AlreadyRecording);
        }

        info!("Starting stream recording: {} ({})", self.stream_id, self.stream_url);

        let recorder = Arc::new(self.clone());
        let handle = tokio::spawn(async move {
            if let Err(e) = recorder.record_stream().await {
                error!("Recording failed for stream {}: {}", recorder.stream_id, e);
            }
            recorder.is_recording.store(false, Ordering::Relaxed);
        });

        *self.task_handle.lock().await = Some(handle);
        Ok(())
    }

    async fn stop_recording(&self) -> Result<(), RecorderError> {
        if !self.is_recording.load(Ordering::Relaxed) {
            return Err(RecorderError::NotRecording);
        }

        info!("Stopping stream recording: {}", self.stream_id);

        // Signal the recording task to stop gracefully
        self.is_recording.store(false, Ordering::Relaxed);

        // Wait for the recording task to complete gracefully
        if let Some(handle) = self.task_handle.lock().await.take() {
            // Use join instead of abort to allow graceful shutdown
            match tokio::time::timeout(tokio::time::Duration::from_secs(5), handle).await {
                Ok(_) => {
                    info!("Stream {} stopped gracefully", self.stream_id);
                }
                Err(_) => {
                    warn!("Stream {} did not stop within timeout, may have been forcefully terminated", self.stream_id);
                }
            }
        }

        Ok(())
    }

    async fn record_stream(&self) -> Result<(), RecorderError> {
        let mut retry_count = 0;

        while self.is_recording.load(Ordering::Relaxed) && (self.config.max_retries == 0 || retry_count < self.config.max_retries) {
            match self.record_stream_once().await {
                Ok(_) => {
                    info!("Stream recording completed normally for: {}", self.stream_id);
                    break;
                }
                Err(e) => {
                    retry_count += 1;

                    if self.config.max_retries == 0 {
                        error!(
                            "Recording failed for stream {} (attempt {}, infinite retries): {}",
                            self.stream_id, retry_count, e
                        );
                    } else {
                        error!(
                            "Recording failed for stream {} (attempt {}/{}): {}",
                            self.stream_id, retry_count, self.config.max_retries, e
                        );
                    }

                    // Continue to retry if we haven't exceeded max_retries (or infinite retries)
                    if self.config.max_retries == 0 || retry_count < self.config.max_retries {
                        // Calculate exponential backoff delay
                        let delay_seconds = if retry_count == 1 {
                            // First retry is immediate (0 seconds)
                            0
                        } else {
                            // Exponential backoff: 1, 2, 4, 8, ... up to retry_delay max
                            let exponential_delay = 1u64 << (retry_count - 2).min(6); // Cap at 2^6 = 64 seconds
                            exponential_delay.min(self.config.retry_delay)
                        };

                        if delay_seconds == 0 {
                            info!("Retrying stream {} immediately...", self.stream_id);
                        } else {
                            warn!("Retrying stream {} in {} seconds...", self.stream_id, delay_seconds);
                            tokio::time::sleep(tokio::time::Duration::from_secs(delay_seconds)).await;
                        }
                    }
                }
            }
        }

        if self.config.max_retries > 0 && retry_count >= self.config.max_retries {
            error!("Max retries ({}) exceeded for stream {}, stopping recording", self.config.max_retries, self.stream_id);
        }

        Ok(())
    }

    async fn record_stream_once(&self) -> Result<(), RecorderError> {
        // Check disk space before starting
        if !self.disk_manager.has_space().await? {
            error!("Insufficient disk space to start recording for stream: {}", self.stream_id);
            return Err(RecorderError::DiskFull);
        }

        info!(
            "Connecting to stream: {} ({}, segment duration: {}s)",
            self.stream_id, self.stream_url, self.config.segment_duration
        );

        // Extract video information and drop non-Send types before async operations
        let (video_stream_index, video_codec_parameters, video_width, video_height) = {
            let options = self.create_input_options();
            let input = ffmpeg::format::input_with_dictionary(&self.stream_url, options)
                .map_err(|e| {
                    error!(
                        "Failed to open stream {} with FFmpeg options: {}. \
                        Try adjusting FFMPEG_ANALYZEDURATION (currently {}μs) or FFMPEG_PROBESIZE (currently {} bytes)",
                        self.stream_url, e, self.config.ffmpeg_analyzeduration, self.config.ffmpeg_probesize
                    );
                    RecorderError::StreamConnection(format!(
                        "Cannot connect to stream {}: {}. This may be due to codec parameter detection issues with HEVC/H.265 streams. \
                        Consider increasing FFMPEG_ANALYZEDURATION (>10000000μs) or FFMPEG_PROBESIZE (>10000000 bytes) environment variables.",
                        self.stream_url, e
                    ))
                })?;

            let video_stream_index = input
                .streams()
                .best(ffmpeg::media::Type::Video)
                .ok_or_else(|| {
                    RecorderError::StreamConnection(format!(
                        "No video stream found in {}. The stream may not contain video data or may be using an unsupported format.",
                        self.stream_url
                    ))
                })?
                .index();

            let video_stream = input.stream(video_stream_index).unwrap();
            let video_codec_parameters = video_stream.parameters();

            // Create a decoder to get video dimensions
            let context = ffmpeg::codec::context::Context::from_parameters(video_codec_parameters.clone())
                .map_err(|e| {
                    error!(
                        "Failed to create codec context for stream {}: {}. \
                        This often happens with HEVC streams that need longer analysis time.",
                        self.stream_url, e
                    );
                    RecorderError::StreamConnection(format!(
                        "Codec parameter detection failed for stream {}: {}. \
                        For HEVC/H.265 streams, try: FFMPEG_ANALYZEDURATION=20000000 FFMPEG_PROBESIZE=20000000",
                        self.stream_url, e
                    ))
                })?;

            let decoder = context.decoder().video()
                .map_err(|e| {
                    error!("Failed to create video decoder for stream {}: {}", self.stream_url, e);
                    RecorderError::StreamConnection(format!(
                        "Video decoder creation failed for {}: {}. The codec may not be supported or the stream parameters are invalid.",
                        self.stream_url, e
                    ))
                })?;

            let width = decoder.width();
            let height = decoder.height();

            if width == 0 || height == 0 {
                return Err(RecorderError::StreamConnection(format!(
                    "Stream {} has unspecified dimensions ({}x{}). \
                    This is common with HEVC streams. Try: FFMPEG_ANALYZEDURATION=30000000 FFMPEG_PROBESIZE=30000000 RTSP_TRANSPORT=tcp",
                    self.stream_url, width, height
                )));
            }

            (video_stream_index, video_codec_parameters, width, height)
        };

        info!(
            "Video stream found for {} - codec: {:?}, {}x{}",
            self.stream_id,
            video_codec_parameters.id(),
            video_width,
            video_height
        );

        let mut segment_index = 0;
        let start_time = Utc::now();
        let recording_start = std::time::Instant::now();

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.started_at = Some(start_time);
        }

        while self.is_recording.load(Ordering::Relaxed) {
            let segment_path = self.generate_segment_path(segment_index);

            info!("Recording segment for stream {}: {:?}", self.stream_id, segment_path);

            // Re-open input stream for this segment
            let options = self.create_input_options();
            let mut input = ffmpeg::format::input_with_dictionary(&self.stream_url, options)?;

            // Create output for this segment with options to handle timestamp issues
            let mut output = ffmpeg::format::output(&segment_path)?;

            // Add video stream to output
            {
                let mut out_stream =
                    output.add_stream(ffmpeg::encoder::find(ffmpeg::codec::Id::H264))?;
                out_stream.set_parameters(video_codec_parameters.clone());
            }

            // Write header
            output.write_header()?;

            let segment_start = std::time::Instant::now();
            let mut packet_count = 0;

            // Record for segment duration with more frequent cancellation checks
            let mut last_packet_time = std::time::Instant::now();
            while self.is_recording.load(Ordering::Relaxed)
                && segment_start.elapsed().as_secs() < self.config.segment_duration as u64
            {
                let mut packets_processed_this_iteration = 0;
                let iteration_start = std::time::Instant::now();

                // Process packets with timeout to avoid blocking indefinitely
                for (stream, mut packet) in input.packets() {
                    // Check for cancellation frequently during packet processing
                    if !self.is_recording.load(Ordering::Relaxed) {
                        info!("Stream {} cancelled during packet processing", self.stream_id);
                        return Ok(());
                    }

                    if stream.index() == video_stream_index {
                        packet.set_stream(0);
                        packet.write_interleaved(&mut output)?;
                        packet_count += 1;
                        packets_processed_this_iteration += 1;
                        last_packet_time = std::time::Instant::now();

                        // Check if segment duration reached
                        if segment_start.elapsed().as_secs() >= self.config.segment_duration as u64
                        {
                            break;
                        }
                    }

                    // Break out of packet iterator if we've been processing for too long
                    // This ensures we check cancellation flag regularly
                    if iteration_start.elapsed().as_millis() > 500 {
                        break;
                    }
                }

                // If no packets for a while and we're trying to cancel, break out
                if packets_processed_this_iteration == 0 && last_packet_time.elapsed().as_millis() > 1000 {
                    if !self.is_recording.load(Ordering::Relaxed) {
                        info!("Stream {} no packets received and cancellation requested", self.stream_id);
                        return Ok(());
                    }
                    // Small sleep to prevent tight loop when no packets
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                }
            }

            // Check for cancellation before finalizing segment
            if !self.is_recording.load(Ordering::Relaxed) {
                info!("Stream {} cancelled before finalizing segment", self.stream_id);
                // Still write trailer to avoid corrupted file
                output.write_trailer()?;
                return Ok(());
            }

            // Write trailer for this segment
            output.write_trailer()?;

            let segment_duration_actual = segment_start.elapsed().as_secs();
            info!(
                "Completed segment {} for stream {} with {} packets (duration: {}s, size: {:?})",
                segment_index,
                self.stream_id,
                packet_count,
                segment_duration_actual,
                std::fs::metadata(&segment_path).map(|m| m.len()).unwrap_or(0)
            );

            // Drop FFmpeg contexts before async operations
            drop(output);
            drop(input);

            // Update stats
            {
                let mut stats = self.stats.write().await;
                stats.files_recorded += 1;
                stats.total_duration += self.config.segment_duration as u64;
                stats.last_file = Some(segment_path);
            }

            self.files_recorded.fetch_add(1, Ordering::Relaxed);
            segment_index += 1;

            // Check disk space and log status periodically
            if segment_index % 10 == 0 {
                let storage = self.disk_manager.get_storage_info().await?;
                let elapsed = recording_start.elapsed().as_secs();
                info!(
                    "Recording status for stream {} - Segment: {}, Runtime: {}s, Files: {}, Storage: {:.1}% used",
                    self.stream_id,
                    segment_index,
                    elapsed,
                    self.files_recorded.load(Ordering::Relaxed),
                    storage.usage_percent
                );

                if !self.disk_manager.has_space().await? {
                    warn!("Disk space low for stream {}, attempting cleanup", self.stream_id);
                    self.disk_manager.cleanup_old_files().await?;

                    if !self.disk_manager.has_space().await? {
                        error!("Insufficient disk space after cleanup, stopping stream: {}", self.stream_id);
                        return Err(RecorderError::DiskFull);
                    }
                }
            }
        }

        Ok(())
    }

    fn generate_segment_path(&self, index: u32) -> PathBuf {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let filename = format!("segment_{}_{}_{:06}.mp4", self.stream_id, timestamp, index);
        self.config.get_stream_output_dir(&self.stream_id).join(filename)
    }
}

impl MultiStreamRecorder {
    pub fn new(config: Config, disk_manager: Arc<DiskManager>) -> Self {
        Self {
            config,
            disk_manager,
            stream_recorders: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn initialize_streams(&self) {
        let mut map = self.stream_recorders.write().await;
        for stream in &self.config.streams {
            if stream.enabled {
                let stream_recorder = Arc::new(SingleStreamRecorder::new(
                    stream.clone(),
                    self.config.clone(),
                    self.disk_manager.clone(),
                ));
                map.insert(stream.id.clone(), stream_recorder);
            }
        }

        // After initializing streams, scan for existing recordings
        drop(map); // Release the write lock before calling scan_existing_recordings
        self.scan_existing_recordings().await;
    }

    /// Scan existing recording directories and update statistics
    pub async fn scan_existing_recordings(&self) {
        info!("Scanning for existing recording files...");

        let recorders = self.stream_recorders.read().await;
        let mut total_existing_files = 0;
        let mut total_existing_duration = 0;

        for (stream_id, recorder) in recorders.iter() {
            let stream_dir = self.config.get_stream_output_dir(stream_id);

            if stream_dir.exists() {
                match self.scan_stream_directory(&stream_dir, stream_id).await {
                    Ok((files_count, total_duration)) => {
                        // Update the stream's statistics
                        recorder.files_recorded.store(files_count as u64, Ordering::Relaxed);

                        // Update the stats with existing recordings info
                        {
                            let mut stats = recorder.stats.write().await;
                            stats.total_duration = total_duration;
                            // Don't set started_at for existing files
                        }

                        total_existing_files += files_count;
                        total_existing_duration += total_duration;

                        info!(
                            "Found {} existing recording files for stream {} (total duration: {}s)",
                            files_count, stream_id, total_duration
                        );
                    }
                    Err(e) => {
                        warn!("Failed to scan directory {:?} for stream {}: {}", stream_dir, stream_id, e);
                    }
                }
            } else {
                info!("No existing recording directory for stream {}", stream_id);
            }
        }

        if total_existing_files > 0 {
            info!(
                "Startup scan complete: Found {} existing recording files across all streams (total duration: {}s)",
                total_existing_files, total_existing_duration
            );
        } else {
            info!("Startup scan complete: No existing recordings found");
        }
    }

    /// Scan a specific stream directory for existing recording files
    async fn scan_stream_directory(&self, stream_dir: &std::path::Path, stream_id: &str) -> Result<(usize, u64), std::io::Error> {
        let mut files_count = 0;
        let mut total_duration = 0;

        let entries = std::fs::read_dir(stream_dir)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Check if it's a video file (mp4)
            if let Some(extension) = path.extension() {
                if extension == "mp4" {
                    files_count += 1;

                    // Try to extract duration from filename or file metadata
                    if let Some(duration) = self.extract_duration_from_filename(&path, stream_id) {
                        total_duration += duration;
                    } else {
                        // Default segment duration if we can't parse the filename
                        total_duration += self.config.segment_duration as u64;
                    }
                }
            }
        }

        Ok((files_count, total_duration))
    }

    /// Extract duration from segment filename if possible
    fn extract_duration_from_filename(&self, _path: &std::path::Path, _stream_id: &str) -> Option<u64> {
        // Our segments are named like: segment_STREAMID_20250924_161814_000000.mp4
        // For now, assume each segment is the configured segment duration
        // In the future, we could parse actual file metadata or use FFmpeg to get real duration
        Some(self.config.segment_duration as u64)
    }
}


#[async_trait]
impl Recorder for VideoRecorder {
    async fn start(&self) -> Result<(), RecorderError> {
        // For backward compatibility, start all enabled streams
        info!("Starting video recording for all enabled streams");

        let mut started_any = false;
        for stream in self.config.get_enabled_streams() {
            match self.start_stream(&stream.id).await {
                Ok(_) => {
                    info!("Started stream: {}", stream.id);
                    started_any = true;
                }
                Err(e) => {
                    error!("Failed to start stream {}: {}", stream.id, e);
                }
            }
        }

        if !started_any {
            return Err(RecorderError::StreamConnection("No streams could be started".to_string()));
        }

        Ok(())
    }

    async fn stop(&self) -> Result<(), RecorderError> {
        // For backward compatibility, stop all recording streams
        info!("Stopping video recording for all streams");

        let mut stopped_any = false;
        let recorders = self.stream_recorders.read().await;
        for (stream_id, recorder) in recorders.iter() {
            if recorder.is_recording.load(Ordering::Relaxed) {
                match recorder.stop_recording().await {
                    Ok(_) => {
                        info!("Stopped stream: {}", stream_id);
                        stopped_any = true;
                    }
                    Err(e) => {
                        error!("Failed to stop stream {}: {}", stream_id, e);
                    }
                }
            }
        }

        if !stopped_any {
            return Err(RecorderError::NotRecording);
        }

        Ok(())
    }

    async fn is_recording(&self) -> bool {
        // For backward compatibility, return true if any stream is recording
        let recorders = self.stream_recorders.read().await;
        for recorder in recorders.values() {
            if recorder.is_recording.load(Ordering::Relaxed) {
                return true;
            }
        }
        false
    }

    async fn get_stats(&self) -> RecordingStats {
        // For backward compatibility, return stats from first enabled stream
        if let Some(first_stream) = self.config.get_enabled_streams().first() {
            if let Some(stats) = self.get_stream_stats(&first_stream.id).await {
                return stats.stats;
            }
        }

        // Fallback to empty stats
        RecordingStats {
            started_at: None,
            files_recorded: 0,
            total_duration: 0,
            last_file: None,
        }
    }

    // New multi-stream methods
    async fn start_stream(&self, stream_id: &str) -> Result<(), RecorderError> {
        let recorders = self.stream_recorders.read().await;
        if let Some(recorder) = recorders.get(stream_id) {
            recorder.start_recording().await
        } else {
            Err(RecorderError::StreamConnection(format!("Stream not found: {}", stream_id)))
        }
    }

    async fn stop_stream(&self, stream_id: &str) -> Result<(), RecorderError> {
        let recorders = self.stream_recorders.read().await;
        if let Some(recorder) = recorders.get(stream_id) {
            recorder.stop_recording().await
        } else {
            Err(RecorderError::StreamConnection(format!("Stream not found: {}", stream_id)))
        }
    }

    async fn is_stream_recording(&self, stream_id: &str) -> bool {
        let recorders = self.stream_recorders.read().await;
        if let Some(recorder) = recorders.get(stream_id) {
            recorder.is_recording.load(Ordering::Relaxed)
        } else {
            false
        }
    }

    async fn get_stream_stats(&self, stream_id: &str) -> Option<StreamStats> {
        let recorders = self.stream_recorders.read().await;
        if let Some(recorder) = recorders.get(stream_id) {
            let stats = recorder.stats.read().await.clone();
            Some(StreamStats {
                stream_id: stream_id.to_string(),
                stream_url: recorder.stream_url.clone(),
                is_recording: recorder.is_recording.load(Ordering::Relaxed),
                stats,
            })
        } else {
            None
        }
    }

    async fn get_multi_stream_stats(&self) -> MultiStreamStats {
        let recorders = self.stream_recorders.read().await;
        let mut streams = Vec::new();
        let mut total_files_recorded = 0;
        let mut active_streams = 0;

        for (stream_id, recorder) in recorders.iter() {
            let stats = recorder.stats.read().await.clone();
            let is_recording = recorder.is_recording.load(Ordering::Relaxed);

            total_files_recorded += stats.files_recorded;
            if is_recording {
                active_streams += 1;
            }

            streams.push(StreamStats {
                stream_id: stream_id.clone(),
                stream_url: recorder.stream_url.clone(),
                is_recording,
                stats,
            });
        }

        MultiStreamStats {
            streams,
            total_files_recorded,
            active_streams,
        }
    }

    async fn list_streams(&self) -> Vec<String> {
        let recorders = self.stream_recorders.read().await;
        recorders.keys().cloned().collect()
    }
}

impl Clone for VideoRecorder {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            disk_manager: self.disk_manager.clone(),
            stream_recorders: self.stream_recorders.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_recording_stats_default_values() {
        let stats = RecordingStats {
            started_at: None,
            files_recorded: 0,
            total_duration: 0,
            last_file: None,
        };

        assert!(stats.started_at.is_none());
        assert_eq!(stats.files_recorded, 0);
        assert_eq!(stats.total_duration, 0);
        assert!(stats.last_file.is_none());
    }

    #[test]
    fn test_recording_stats_with_values() {
        let now = Utc::now();
        let stats = RecordingStats {
            started_at: Some(now),
            files_recorded: 42,
            total_duration: 3600,
            last_file: Some(PathBuf::from("/recordings/segment_001.mp4")),
        };

        assert_eq!(stats.started_at, Some(now));
        assert_eq!(stats.files_recorded, 42);
        assert_eq!(stats.total_duration, 3600);
        assert_eq!(stats.last_file, Some(PathBuf::from("/recordings/segment_001.mp4")));
    }

    #[test]
    fn test_recording_stats_clone() {
        let stats = RecordingStats {
            started_at: Some(Utc::now()),
            files_recorded: 10,
            total_duration: 100,
            last_file: Some(PathBuf::from("/test.mp4")),
        };

        let cloned = stats.clone();

        assert_eq!(cloned.started_at, stats.started_at);
        assert_eq!(cloned.files_recorded, stats.files_recorded);
        assert_eq!(cloned.total_duration, stats.total_duration);
        assert_eq!(cloned.last_file, stats.last_file);
    }

    #[test]
    fn test_recording_stats_serialization() {
        let stats = RecordingStats {
            started_at: None,
            files_recorded: 5,
            total_duration: 50,
            last_file: None,
        };

        let json = serde_json::to_string(&stats).unwrap();

        assert!(json.contains("\"files_recorded\":5"));
        assert!(json.contains("\"total_duration\":50"));
        assert!(json.contains("\"started_at\":null"));
        assert!(json.contains("\"last_file\":null"));
    }

    #[test]
    fn test_stream_stats_creation() {
        let stats = StreamStats {
            stream_id: "cam1".to_string(),
            stream_url: "rtsp://example.com/stream".to_string(),
            is_recording: true,
            stats: RecordingStats {
                started_at: None,
                files_recorded: 20,
                total_duration: 200,
                last_file: None,
            },
        };

        assert_eq!(stats.stream_id, "cam1");
        assert_eq!(stats.stream_url, "rtsp://example.com/stream");
        assert!(stats.is_recording);
        assert_eq!(stats.stats.files_recorded, 20);
    }

    #[test]
    fn test_stream_stats_clone() {
        let stats = StreamStats {
            stream_id: "test".to_string(),
            stream_url: "rtsp://test.com".to_string(),
            is_recording: false,
            stats: RecordingStats {
                started_at: None,
                files_recorded: 0,
                total_duration: 0,
                last_file: None,
            },
        };

        let cloned = stats.clone();

        assert_eq!(cloned.stream_id, stats.stream_id);
        assert_eq!(cloned.stream_url, stats.stream_url);
        assert_eq!(cloned.is_recording, stats.is_recording);
    }

    #[test]
    fn test_stream_stats_serialization() {
        let stats = StreamStats {
            stream_id: "cam2".to_string(),
            stream_url: "rtsp://example.com/cam2".to_string(),
            is_recording: true,
            stats: RecordingStats {
                started_at: None,
                files_recorded: 15,
                total_duration: 150,
                last_file: Some(PathBuf::from("/recordings/test.mp4")),
            },
        };

        let json = serde_json::to_string(&stats).unwrap();

        assert!(json.contains("\"stream_id\":\"cam2\""));
        assert!(json.contains("\"stream_url\":\"rtsp://example.com/cam2\""));
        assert!(json.contains("\"is_recording\":true"));
        assert!(json.contains("\"files_recorded\":15"));
    }

    #[test]
    fn test_multi_stream_stats_creation() {
        let stats = MultiStreamStats {
            streams: vec![
                StreamStats {
                    stream_id: "cam1".to_string(),
                    stream_url: "rtsp://example.com/1".to_string(),
                    is_recording: true,
                    stats: RecordingStats {
                        started_at: None,
                        files_recorded: 10,
                        total_duration: 100,
                        last_file: None,
                    },
                },
                StreamStats {
                    stream_id: "cam2".to_string(),
                    stream_url: "rtsp://example.com/2".to_string(),
                    is_recording: false,
                    stats: RecordingStats {
                        started_at: None,
                        files_recorded: 5,
                        total_duration: 50,
                        last_file: None,
                    },
                },
            ],
            total_files_recorded: 15,
            active_streams: 1,
        };

        assert_eq!(stats.streams.len(), 2);
        assert_eq!(stats.total_files_recorded, 15);
        assert_eq!(stats.active_streams, 1);
    }

    #[test]
    fn test_multi_stream_stats_clone() {
        let stats = MultiStreamStats {
            streams: vec![],
            total_files_recorded: 100,
            active_streams: 3,
        };

        let cloned = stats.clone();

        assert_eq!(cloned.streams.len(), stats.streams.len());
        assert_eq!(cloned.total_files_recorded, stats.total_files_recorded);
        assert_eq!(cloned.active_streams, stats.active_streams);
    }

    #[test]
    fn test_multi_stream_stats_serialization() {
        let stats = MultiStreamStats {
            streams: vec![],
            total_files_recorded: 50,
            active_streams: 2,
        };

        let json = serde_json::to_string(&stats).unwrap();

        assert!(json.contains("\"streams\":[]"));
        assert!(json.contains("\"total_files_recorded\":50"));
        assert!(json.contains("\"active_streams\":2"));
    }

    #[test]
    fn test_multi_stream_recorder_new() {
        let config = Config::default();
        let disk_manager = Arc::new(DiskManager::new(config.clone()));

        let recorder = MultiStreamRecorder::new(config.clone(), disk_manager);

        assert_eq!(recorder.config.segment_duration, config.segment_duration);
    }

    #[tokio::test]
    async fn test_multi_stream_recorder_list_streams_empty() {
        let config = Config::default();
        let disk_manager = Arc::new(DiskManager::new(config.clone()));
        let recorder = MultiStreamRecorder::new(config, disk_manager);

        let streams = recorder.list_streams().await;

        assert!(streams.is_empty());
    }

    #[tokio::test]
    async fn test_multi_stream_recorder_is_recording_when_empty() {
        let config = Config::default();
        let disk_manager = Arc::new(DiskManager::new(config.clone()));
        let recorder = MultiStreamRecorder::new(config, disk_manager);

        assert!(!recorder.is_recording().await);
    }

    #[tokio::test]
    async fn test_multi_stream_recorder_get_stats_when_empty() {
        let config = Config::default();
        let disk_manager = Arc::new(DiskManager::new(config.clone()));
        let recorder = MultiStreamRecorder::new(config, disk_manager);

        let stats = recorder.get_stats().await;

        assert!(stats.started_at.is_none());
        assert_eq!(stats.files_recorded, 0);
        assert_eq!(stats.total_duration, 0);
        assert!(stats.last_file.is_none());
    }

    #[tokio::test]
    async fn test_multi_stream_recorder_get_stream_stats_not_found() {
        let config = Config::default();
        let disk_manager = Arc::new(DiskManager::new(config.clone()));
        let recorder = MultiStreamRecorder::new(config, disk_manager);

        let stats = recorder.get_stream_stats("nonexistent").await;

        assert!(stats.is_none());
    }

    #[tokio::test]
    async fn test_multi_stream_recorder_is_stream_recording_not_found() {
        let config = Config::default();
        let disk_manager = Arc::new(DiskManager::new(config.clone()));
        let recorder = MultiStreamRecorder::new(config, disk_manager);

        assert!(!recorder.is_stream_recording("nonexistent").await);
    }

    #[tokio::test]
    async fn test_multi_stream_recorder_get_multi_stream_stats_empty() {
        let config = Config::default();
        let disk_manager = Arc::new(DiskManager::new(config.clone()));
        let recorder = MultiStreamRecorder::new(config, disk_manager);

        let stats = recorder.get_multi_stream_stats().await;

        assert!(stats.streams.is_empty());
        assert_eq!(stats.total_files_recorded, 0);
        assert_eq!(stats.active_streams, 0);
    }

    #[test]
    fn test_video_recorder_clone() {
        let config = Config::default();
        let disk_manager = Arc::new(DiskManager::new(config.clone()));
        let recorder = VideoRecorder::new(config.clone(), disk_manager);

        let cloned = recorder.clone();

        assert_eq!(cloned.config.segment_duration, recorder.config.segment_duration);
    }
}
