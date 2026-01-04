use crate::config::Config;
use crate::disk_manager::{DiskManager, StorageInfo};
use crate::recorder::{Recorder, RecordingStats, StreamStats, MultiStreamStats};
use axum::{
    Router,
    extract::{State, Path},
    http::StatusCode,
    response::Json,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{trace::TraceLayer, services::ServeDir};
use tracing::{error, info, trace};

#[derive(Clone)]
pub struct AppState {
    pub recorder: Arc<dyn Recorder + Send + Sync>,
    pub disk_manager: Arc<DiskManager>,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub uptime: u64,
    pub timestamp: String,
    pub recording: bool,
    pub storage: StorageInfo,
}

#[derive(Serialize)]
pub struct MetricsResponse {
    pub recording_stats: RecordingStats,
    pub storage_info: StorageInfo,
    pub files_deleted: u64,
    pub last_cleanup: Option<String>,
}

#[derive(Serialize)]
pub struct MultiStreamMetricsResponse {
    pub multi_stream_stats: MultiStreamStats,
    pub storage_info: StorageInfo,
    pub files_deleted: u64,
    pub last_cleanup: Option<String>,
}

#[derive(Serialize)]
pub struct StreamListResponse {
    pub streams: Vec<String>,
}

#[derive(Deserialize)]
pub struct StartRequest {
    //pub force: Option<bool>,
}

pub struct Server {
    config: Config,
    recorder: Arc<dyn Recorder + Send + Sync>,
    disk_manager: Arc<DiskManager>,
}

impl Server {
    pub fn new(
        config: Config,
        recorder: Arc<dyn Recorder + Send + Sync>,
        disk_manager: Arc<DiskManager>,
    ) -> Self {
        Self {
            config,
            recorder,
            disk_manager,
        }
    }

    pub async fn start(self) -> Result<(), anyhow::Error> {
        let state = AppState {
            recorder: self.recorder,
            disk_manager: self.disk_manager,
        };

        // Get the frontend dist path from build script
        let frontend_dist = if let Ok(dist_path) = std::env::var("FRONTEND_DIST_PATH") {
            std::path::PathBuf::from(dist_path)
        } else {
            // Fallback to local development path
            std::path::PathBuf::from("frontend/dist")
        };

        info!("Serving frontend from: {:?}", frontend_dist);

        let app = Router::new()
            // Legacy API routes (for backward compatibility)
            .route("/health", get(health_check))
            .route("/metrics", get(get_metrics))
            .route("/start", post(start_recording))
            .route("/stop", post(stop_recording))
            // New multi-stream API routes
            .route("/streams", get(list_streams))
            .route("/streams/metrics", get(get_multi_stream_metrics))
            .route("/streams/:stream_id/start", post(start_stream_recording))
            .route("/streams/:stream_id/stop", post(stop_stream_recording))
            .route("/streams/:stream_id/stats", get(get_stream_stats))
            .route("/streams/:stream_id/status", get(get_stream_status))
            // Static file serving (must be last)
            .fallback_service(ServeDir::new(frontend_dist))
            .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()))
            .with_state(state);

        let addr = format!("{}:{}", self.config.server_host, self.config.server_port);
        info!("Starting server on {}", addr);

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}

async fn health_check(State(state): State<AppState>) -> Result<Json<HealthResponse>, StatusCode> {
    let storage = state
        .disk_manager
        .get_storage_info()
        .await
        .map_err(|e| {
            error!("Failed to get storage info for health check: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let is_recording = state.recorder.is_recording().await;

    let response = HealthResponse {
        status: "healthy".to_string(),
        uptime: 0, // You'd implement actual uptime tracking
        timestamp: chrono::Utc::now().to_rfc3339(),
        recording: is_recording,
        storage: storage.clone(),
    };

    trace!(
        "Health check - Recording: {}, Storage: {:.1}% used ({} GB available)",
        is_recording,
        storage.usage_percent,
        storage.available_space / 1_000_000_000
    );

    Ok(Json(response))
}

async fn get_metrics(State(state): State<AppState>) -> Result<Json<MetricsResponse>, StatusCode> {
    info!("Metrics requested");

    let recording_stats = state.recorder.get_stats().await;
    let storage_info = state
        .disk_manager
        .get_storage_info()
        .await
        .map_err(|e| {
            error!("Failed to get storage info for metrics: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let files_deleted = state.disk_manager.get_files_deleted();
    let last_cleanup = state
        .disk_manager
        .get_last_cleanup()
        .await
        .map(|dt| dt.to_rfc3339());

    let response = MetricsResponse {
        recording_stats: recording_stats.clone(),
        storage_info: storage_info.clone(),
        files_deleted,
        last_cleanup: last_cleanup.clone(),
    };

    info!(
        "Metrics - Files recorded: {}, Total duration: {}s, Files deleted: {}, Storage: {:.1}%",
        recording_stats.files_recorded,
        recording_stats.total_duration,
        files_deleted,
        storage_info.usage_percent
    );

    Ok(Json(response))
}

async fn start_recording(State(state): State<AppState>) -> Result<String, StatusCode> {
    info!("Received request to start recording");

    match state.recorder.start().await {
        Ok(_) => {
            info!("Recording started successfully via API");
            Ok("Recording started".to_string())
        }
        Err(e) => {
            error!("Failed to start recording via API: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn stop_recording(State(state): State<AppState>) -> Result<String, StatusCode> {
    info!("Received request to stop recording");

    match state.recorder.stop().await {
        Ok(_) => {
            info!("Recording stopped successfully via API");
            Ok("Recording stopped".to_string())
        }
        Err(e) => {
            error!("Failed to stop recording via API: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// New multi-stream API endpoints

async fn list_streams(State(state): State<AppState>) -> Result<Json<StreamListResponse>, StatusCode> {
    info!("Received request to list streams");

    let streams = state.recorder.list_streams().await;
    Ok(Json(StreamListResponse { streams }))
}

async fn get_multi_stream_metrics(State(state): State<AppState>) -> Result<Json<MultiStreamMetricsResponse>, StatusCode> {
    info!("Multi-stream metrics requested");

    let multi_stream_stats = state.recorder.get_multi_stream_stats().await;
    let storage_info = state
        .disk_manager
        .get_storage_info()
        .await
        .map_err(|e| {
            error!("Failed to get storage info for multi-stream metrics: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let files_deleted = state.disk_manager.get_files_deleted();
    let last_cleanup = state
        .disk_manager
        .get_last_cleanup()
        .await
        .map(|dt| dt.to_rfc3339());

    let response = MultiStreamMetricsResponse {
        multi_stream_stats,
        storage_info,
        files_deleted,
        last_cleanup,
    };

    Ok(Json(response))
}

async fn start_stream_recording(
    Path(stream_id): Path<String>,
    State(state): State<AppState>,
) -> Result<String, StatusCode> {
    info!("Received request to start recording for stream: {}", stream_id);

    match state.recorder.start_stream(&stream_id).await {
        Ok(_) => {
            info!("Recording started successfully for stream: {}", stream_id);
            Ok(format!("Recording started for stream: {}", stream_id))
        }
        Err(e) => {
            error!("Failed to start recording for stream {}: {}", stream_id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn stop_stream_recording(
    Path(stream_id): Path<String>,
    State(state): State<AppState>,
) -> Result<String, StatusCode> {
    info!("Received request to stop recording for stream: {}", stream_id);

    match state.recorder.stop_stream(&stream_id).await {
        Ok(_) => {
            info!("Recording stopped successfully for stream: {}", stream_id);
            Ok(format!("Recording stopped for stream: {}", stream_id))
        }
        Err(e) => {
            error!("Failed to stop recording for stream {}: {}", stream_id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_stream_stats(
    Path(stream_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Option<StreamStats>>, StatusCode> {
    info!("Received request for stats for stream: {}", stream_id);

    let stats = state.recorder.get_stream_stats(&stream_id).await;
    Ok(Json(stats))
}

async fn get_stream_status(
    Path(stream_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Received request for status of stream: {}", stream_id);

    let is_recording = state.recorder.is_stream_recording(&stream_id).await;
    let response = serde_json::json!({
        "stream_id": stream_id,
        "is_recording": is_recording
    });

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::disk_manager::DiskManager;
    use crate::recorder::{Recorder, RecordingStats, StreamStats, MultiStreamStats};
    use crate::error::RecorderError;
    use std::sync::Arc;

    /// Mock recorder for testing that implements the Recorder trait
    struct MockRecorder {
        is_recording_value: bool,
        stats: RecordingStats,
        streams: Vec<String>,
        should_fail_start: bool,
        should_fail_stop: bool,
    }

    impl MockRecorder {
        fn new() -> Self {
            Self {
                is_recording_value: false,
                stats: RecordingStats {
                    started_at: None,
                    files_recorded: 0,
                    total_duration: 0,
                    last_file: None,
                },
                streams: vec!["test_stream".to_string()],
                should_fail_start: false,
                should_fail_stop: false,
            }
        }

        fn with_recording(mut self) -> Self {
            self.is_recording_value = true;
            self
        }

        fn with_stats(mut self, files: u64, duration: u64) -> Self {
            self.stats.files_recorded = files;
            self.stats.total_duration = duration;
            self
        }

        fn with_streams(mut self, streams: Vec<String>) -> Self {
            self.streams = streams;
            self
        }

        fn failing_start(mut self) -> Self {
            self.should_fail_start = true;
            self
        }

        fn failing_stop(mut self) -> Self {
            self.should_fail_stop = true;
            self
        }
    }

    #[async_trait::async_trait]
    impl Recorder for MockRecorder {
        async fn start(&self) -> Result<(), RecorderError> {
            if self.should_fail_start {
                Err(RecorderError::StreamConnection("Mock start failure".to_string()))
            } else {
                Ok(())
            }
        }

        async fn stop(&self) -> Result<(), RecorderError> {
            if self.should_fail_stop {
                Err(RecorderError::NotRecording)
            } else {
                Ok(())
            }
        }

        async fn is_recording(&self) -> bool {
            self.is_recording_value
        }

        async fn get_stats(&self) -> RecordingStats {
            self.stats.clone()
        }

        async fn start_stream(&self, _stream_id: &str) -> Result<(), RecorderError> {
            if self.should_fail_start {
                Err(RecorderError::StreamConnection("Mock stream start failure".to_string()))
            } else {
                Ok(())
            }
        }

        async fn stop_stream(&self, _stream_id: &str) -> Result<(), RecorderError> {
            if self.should_fail_stop {
                Err(RecorderError::NotRecording)
            } else {
                Ok(())
            }
        }

        async fn is_stream_recording(&self, _stream_id: &str) -> bool {
            self.is_recording_value
        }

        async fn get_stream_stats(&self, stream_id: &str) -> Option<StreamStats> {
            if self.streams.contains(&stream_id.to_string()) {
                Some(StreamStats {
                    stream_id: stream_id.to_string(),
                    stream_url: format!("rtsp://example.com/{}", stream_id),
                    is_recording: self.is_recording_value,
                    stats: self.stats.clone(),
                })
            } else {
                None
            }
        }

        async fn get_multi_stream_stats(&self) -> MultiStreamStats {
            let streams = self.streams.iter().map(|id| StreamStats {
                stream_id: id.clone(),
                stream_url: format!("rtsp://example.com/{}", id),
                is_recording: self.is_recording_value,
                stats: self.stats.clone(),
            }).collect();

            let active_count = if self.is_recording_value { self.streams.len() as u64 } else { 0 };

            MultiStreamStats {
                streams,
                total_files_recorded: self.stats.files_recorded * self.streams.len() as u64,
                active_streams: active_count,
            }
        }

        async fn list_streams(&self) -> Vec<String> {
            self.streams.clone()
        }
    }

    fn create_test_state(recorder: MockRecorder) -> AppState {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.into_path();

        AppState {
            recorder: Arc::new(recorder),
            disk_manager: Arc::new(DiskManager::new(config)),
        }
    }

    /// Tests HealthResponse serialization
    #[test]
    fn test_health_response_serialization() {
        let response = HealthResponse {
            status: "healthy".to_string(),
            uptime: 100,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            recording: true,
            storage: StorageInfo {
                total_space: 1000000,
                available_space: 500000,
                used_space: 500000,
                usage_percent: 50.0,
            },
        };

        let json = serde_json::to_string(&response).unwrap();

        assert!(json.contains("\"status\":\"healthy\""));
        assert!(json.contains("\"uptime\":100"));
        assert!(json.contains("\"recording\":true"));
    }

    /// Tests MetricsResponse serialization
    #[test]
    fn test_metrics_response_serialization() {
        let response = MetricsResponse {
            recording_stats: RecordingStats {
                started_at: None,
                files_recorded: 10,
                total_duration: 100,
                last_file: None,
            },
            storage_info: StorageInfo {
                total_space: 1000000,
                available_space: 500000,
                used_space: 500000,
                usage_percent: 50.0,
            },
            files_deleted: 5,
            last_cleanup: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_string(&response).unwrap();

        assert!(json.contains("\"files_recorded\":10"));
        assert!(json.contains("\"files_deleted\":5"));
        assert!(json.contains("\"last_cleanup\":"));
    }

    /// Tests MultiStreamMetricsResponse serialization
    #[test]
    fn test_multi_stream_metrics_response_serialization() {
        let response = MultiStreamMetricsResponse {
            multi_stream_stats: MultiStreamStats {
                streams: vec![],
                total_files_recorded: 25,
                active_streams: 2,
            },
            storage_info: StorageInfo {
                total_space: 1000000,
                available_space: 500000,
                used_space: 500000,
                usage_percent: 50.0,
            },
            files_deleted: 3,
            last_cleanup: None,
        };

        let json = serde_json::to_string(&response).unwrap();

        assert!(json.contains("\"total_files_recorded\":25"));
        assert!(json.contains("\"active_streams\":2"));
        assert!(json.contains("\"files_deleted\":3"));
    }

    /// Tests StreamListResponse serialization
    #[test]
    fn test_stream_list_response_serialization() {
        let response = StreamListResponse {
            streams: vec!["cam1".to_string(), "cam2".to_string()],
        };

        let json = serde_json::to_string(&response).unwrap();

        assert!(json.contains("\"streams\":[\"cam1\",\"cam2\"]"));
    }

    /// Tests StartRequest deserialization (empty object is valid)
    #[test]
    fn test_start_request_deserialization() {
        let json = "{}";
        let result: Result<StartRequest, _> = serde_json::from_str(json);

        assert!(result.is_ok());
    }

    /// Tests Server::new creates server correctly
    #[test]
    fn test_server_new() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.output_dir = temp_dir.into_path();

        let disk_manager = Arc::new(DiskManager::new(config.clone()));
        let recorder: Arc<dyn Recorder + Send + Sync> = Arc::new(MockRecorder::new());

        let server = Server::new(config.clone(), recorder, disk_manager);

        assert_eq!(server.config.server_port, config.server_port);
    }

    /// Tests AppState is Clone
    #[test]
    fn test_app_state_clone() {
        let state = create_test_state(MockRecorder::new());

        let cloned = state.clone();

        // Verify both states work
        assert!(Arc::strong_count(&cloned.disk_manager) >= 2);
    }

    /// FR-10: Tests health check returns status through mock recorder
    #[tokio::test]
    async fn test_mock_recorder_is_recording() {
        let recorder = MockRecorder::new();
        assert!(!recorder.is_recording().await);

        let recording_recorder = MockRecorder::new().with_recording();
        assert!(recording_recorder.is_recording().await);
    }

    /// FR-13: Tests mock recorder get_stats
    #[tokio::test]
    async fn test_mock_recorder_get_stats() {
        let recorder = MockRecorder::new().with_stats(15, 150);
        let stats = recorder.get_stats().await;

        assert_eq!(stats.files_recorded, 15);
        assert_eq!(stats.total_duration, 150);
    }

    /// FR-12: Tests mock recorder start success
    #[tokio::test]
    async fn test_mock_recorder_start_success() {
        let recorder = MockRecorder::new();
        let result = recorder.start().await;

        assert!(result.is_ok());
    }

    /// FR-12: Tests mock recorder start failure
    #[tokio::test]
    async fn test_mock_recorder_start_failure() {
        let recorder = MockRecorder::new().failing_start();
        let result = recorder.start().await;

        assert!(result.is_err());
    }

    /// FR-12: Tests mock recorder stop success
    #[tokio::test]
    async fn test_mock_recorder_stop_success() {
        let recorder = MockRecorder::new();
        let result = recorder.stop().await;

        assert!(result.is_ok());
    }

    /// FR-12: Tests mock recorder stop failure
    #[tokio::test]
    async fn test_mock_recorder_stop_failure() {
        let recorder = MockRecorder::new().failing_stop();
        let result = recorder.stop().await;

        assert!(result.is_err());
    }

    /// FR-11: Tests mock recorder list streams
    #[tokio::test]
    async fn test_mock_recorder_list_streams() {
        let recorder = MockRecorder::new()
            .with_streams(vec!["cam1".to_string(), "cam2".to_string()]);

        let streams = recorder.list_streams().await;

        assert_eq!(streams.len(), 2);
        assert!(streams.contains(&"cam1".to_string()));
        assert!(streams.contains(&"cam2".to_string()));
    }

    /// FR-13: Tests mock recorder get_multi_stream_stats
    #[tokio::test]
    async fn test_mock_recorder_get_multi_stream_stats() {
        let recorder = MockRecorder::new()
            .with_streams(vec!["cam1".to_string(), "cam2".to_string()])
            .with_stats(10, 100);

        let stats = recorder.get_multi_stream_stats().await;

        assert_eq!(stats.streams.len(), 2);
        assert_eq!(stats.total_files_recorded, 20); // 10 * 2 streams
    }

    /// FR-12: Tests mock recorder start_stream
    #[tokio::test]
    async fn test_mock_recorder_start_stream() {
        let recorder = MockRecorder::new();
        let result = recorder.start_stream("cam1").await;

        assert!(result.is_ok());
    }

    /// FR-12: Tests mock recorder stop_stream
    #[tokio::test]
    async fn test_mock_recorder_stop_stream() {
        let recorder = MockRecorder::new();
        let result = recorder.stop_stream("cam1").await;

        assert!(result.is_ok());
    }

    /// FR-13: Tests mock recorder get_stream_stats existing stream
    #[tokio::test]
    async fn test_mock_recorder_get_stream_stats_exists() {
        let recorder = MockRecorder::new()
            .with_streams(vec!["cam1".to_string()])
            .with_stats(5, 50);

        let stats = recorder.get_stream_stats("cam1").await;

        assert!(stats.is_some());
        let stats = stats.unwrap();
        assert_eq!(stats.stream_id, "cam1");
        assert_eq!(stats.stats.files_recorded, 5);
    }

    /// FR-13: Tests mock recorder get_stream_stats unknown stream
    #[tokio::test]
    async fn test_mock_recorder_get_stream_stats_unknown() {
        let recorder = MockRecorder::new().with_streams(vec![]);
        let stats = recorder.get_stream_stats("unknown").await;

        assert!(stats.is_none());
    }

    /// Tests mock recorder is_stream_recording
    #[tokio::test]
    async fn test_mock_recorder_is_stream_recording() {
        let recorder = MockRecorder::new();
        assert!(!recorder.is_stream_recording("cam1").await);

        let recording_recorder = MockRecorder::new().with_recording();
        assert!(recording_recorder.is_stream_recording("cam1").await);
    }

    /// Tests active_streams count in multi_stream_stats
    #[tokio::test]
    async fn test_mock_recorder_active_streams_count() {
        let not_recording = MockRecorder::new()
            .with_streams(vec!["cam1".to_string(), "cam2".to_string()]);
        let stats = not_recording.get_multi_stream_stats().await;
        assert_eq!(stats.active_streams, 0);

        let recording = MockRecorder::new()
            .with_streams(vec!["cam1".to_string(), "cam2".to_string()])
            .with_recording();
        let stats = recording.get_multi_stream_stats().await;
        assert_eq!(stats.active_streams, 2);
    }

    // Integration tests for API handlers using axum-test

    use axum_test::TestServer;

    fn create_test_app(recorder: MockRecorder) -> axum::Router {
        let state = create_test_state(recorder);

        axum::Router::new()
            .route("/health", axum::routing::get(super::health_check))
            .route("/metrics", axum::routing::get(super::get_metrics))
            .route("/start", axum::routing::post(super::start_recording))
            .route("/stop", axum::routing::post(super::stop_recording))
            .route("/streams", axum::routing::get(super::list_streams))
            .route("/streams/metrics", axum::routing::get(super::get_multi_stream_metrics))
            .route("/streams/:stream_id/start", axum::routing::post(super::start_stream_recording))
            .route("/streams/:stream_id/stop", axum::routing::post(super::stop_stream_recording))
            .route("/streams/:stream_id/stats", axum::routing::get(super::get_stream_stats))
            .route("/streams/:stream_id/status", axum::routing::get(super::get_stream_status))
            .with_state(state)
    }

    /// FR-10: Tests health endpoint returns healthy status
    #[tokio::test]
    async fn test_health_endpoint_returns_healthy() {
        let app = create_test_app(MockRecorder::new());
        let server = TestServer::new(app).unwrap();

        let response = server.get("/health").await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert_eq!(body["status"], "healthy");
        assert!(body["timestamp"].is_string());
    }

    /// FR-10: Tests health endpoint shows recording status
    #[tokio::test]
    async fn test_health_endpoint_shows_recording_status() {
        let app = create_test_app(MockRecorder::new().with_recording());
        let server = TestServer::new(app).unwrap();

        let response = server.get("/health").await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert_eq!(body["recording"], true);
    }

    /// FR-13: Tests metrics endpoint returns recording stats
    #[tokio::test]
    async fn test_metrics_endpoint_returns_stats() {
        let app = create_test_app(MockRecorder::new().with_stats(10, 100));
        let server = TestServer::new(app).unwrap();

        let response = server.get("/metrics").await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert_eq!(body["recording_stats"]["files_recorded"], 10);
        assert_eq!(body["recording_stats"]["total_duration"], 100);
    }

    /// FR-12: Tests start endpoint starts recording
    #[tokio::test]
    async fn test_start_endpoint_starts_recording() {
        let app = create_test_app(MockRecorder::new());
        let server = TestServer::new(app).unwrap();

        let response = server.post("/start").await;

        response.assert_status_ok();
        let body = response.text();
        assert!(body.contains("Recording started"));
    }

    /// FR-12: Tests start endpoint returns error on failure
    #[tokio::test]
    async fn test_start_endpoint_handles_failure() {
        let app = create_test_app(MockRecorder::new().failing_start());
        let server = TestServer::new(app).unwrap();

        let response = server.post("/start").await;

        response.assert_status(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// FR-12: Tests stop endpoint stops recording
    #[tokio::test]
    async fn test_stop_endpoint_stops_recording() {
        let app = create_test_app(MockRecorder::new());
        let server = TestServer::new(app).unwrap();

        let response = server.post("/stop").await;

        response.assert_status_ok();
        let body = response.text();
        assert!(body.contains("Recording stopped"));
    }

    /// FR-12: Tests stop endpoint returns error on failure
    #[tokio::test]
    async fn test_stop_endpoint_handles_failure() {
        let app = create_test_app(MockRecorder::new().failing_stop());
        let server = TestServer::new(app).unwrap();

        let response = server.post("/stop").await;

        response.assert_status(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// FR-11: Tests list streams endpoint returns stream list
    #[tokio::test]
    async fn test_list_streams_endpoint() {
        let app = create_test_app(MockRecorder::new()
            .with_streams(vec!["cam1".to_string(), "cam2".to_string()]));
        let server = TestServer::new(app).unwrap();

        let response = server.get("/streams").await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        let streams = body["streams"].as_array().unwrap();
        assert_eq!(streams.len(), 2);
    }

    /// FR-13: Tests multi-stream metrics endpoint
    #[tokio::test]
    async fn test_multi_stream_metrics_endpoint() {
        let app = create_test_app(MockRecorder::new()
            .with_streams(vec!["cam1".to_string(), "cam2".to_string()])
            .with_stats(5, 50));
        let server = TestServer::new(app).unwrap();

        let response = server.get("/streams/metrics").await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        let stats = &body["multi_stream_stats"];
        assert_eq!(stats["streams"].as_array().unwrap().len(), 2);
        assert_eq!(stats["total_files_recorded"], 10); // 5 * 2 streams
    }

    /// FR-12: Tests start stream endpoint
    #[tokio::test]
    async fn test_start_stream_endpoint() {
        let app = create_test_app(MockRecorder::new()
            .with_streams(vec!["cam1".to_string()]));
        let server = TestServer::new(app).unwrap();

        let response = server.post("/streams/cam1/start").await;

        response.assert_status_ok();
        let body = response.text();
        assert!(body.contains("cam1"));
    }

    /// FR-12: Tests start stream endpoint failure
    #[tokio::test]
    async fn test_start_stream_endpoint_failure() {
        let app = create_test_app(MockRecorder::new().failing_start());
        let server = TestServer::new(app).unwrap();

        let response = server.post("/streams/cam1/start").await;

        response.assert_status(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// FR-12: Tests stop stream endpoint
    #[tokio::test]
    async fn test_stop_stream_endpoint() {
        let app = create_test_app(MockRecorder::new()
            .with_streams(vec!["cam1".to_string()]));
        let server = TestServer::new(app).unwrap();

        let response = server.post("/streams/cam1/stop").await;

        response.assert_status_ok();
        let body = response.text();
        assert!(body.contains("cam1"));
    }

    /// FR-12: Tests stop stream endpoint failure
    #[tokio::test]
    async fn test_stop_stream_endpoint_failure() {
        let app = create_test_app(MockRecorder::new().failing_stop());
        let server = TestServer::new(app).unwrap();

        let response = server.post("/streams/cam1/stop").await;

        response.assert_status(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// FR-13: Tests get stream stats endpoint for existing stream
    #[tokio::test]
    async fn test_get_stream_stats_endpoint_exists() {
        let app = create_test_app(MockRecorder::new()
            .with_streams(vec!["cam1".to_string()])
            .with_stats(15, 150));
        let server = TestServer::new(app).unwrap();

        let response = server.get("/streams/cam1/stats").await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert_eq!(body["stream_id"], "cam1");
        assert_eq!(body["stats"]["files_recorded"], 15);
    }

    /// FR-13: Tests get stream stats endpoint for non-existent stream
    #[tokio::test]
    async fn test_get_stream_stats_endpoint_not_found() {
        let app = create_test_app(MockRecorder::new().with_streams(vec![]));
        let server = TestServer::new(app).unwrap();

        let response = server.get("/streams/unknown/stats").await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert!(body.is_null());
    }

    /// Tests get stream status endpoint
    #[tokio::test]
    async fn test_get_stream_status_endpoint() {
        let app = create_test_app(MockRecorder::new()
            .with_streams(vec!["cam1".to_string()])
            .with_recording());
        let server = TestServer::new(app).unwrap();

        let response = server.get("/streams/cam1/status").await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert_eq!(body["stream_id"], "cam1");
        assert_eq!(body["is_recording"], true);
    }

    /// Tests get stream status endpoint when not recording
    #[tokio::test]
    async fn test_get_stream_status_endpoint_not_recording() {
        let app = create_test_app(MockRecorder::new()
            .with_streams(vec!["cam1".to_string()]));
        let server = TestServer::new(app).unwrap();

        let response = server.get("/streams/cam1/status").await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert_eq!(body["stream_id"], "cam1");
        assert_eq!(body["is_recording"], false);
    }

    /// Tests health endpoint includes storage info
    #[tokio::test]
    async fn test_health_endpoint_includes_storage() {
        let app = create_test_app(MockRecorder::new());
        let server = TestServer::new(app).unwrap();

        let response = server.get("/health").await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert!(body["storage"]["total_space"].is_number());
        assert!(body["storage"]["available_space"].is_number());
        assert!(body["storage"]["usage_percent"].is_number());
    }

    /// Tests metrics endpoint includes storage info
    #[tokio::test]
    async fn test_metrics_endpoint_includes_storage() {
        let app = create_test_app(MockRecorder::new());
        let server = TestServer::new(app).unwrap();

        let response = server.get("/metrics").await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert!(body["storage_info"]["total_space"].is_number());
        assert!(body["files_deleted"].is_number());
    }

    /// Tests multi-stream metrics endpoint includes storage info
    #[tokio::test]
    async fn test_multi_stream_metrics_includes_storage() {
        let app = create_test_app(MockRecorder::new()
            .with_streams(vec!["cam1".to_string()]));
        let server = TestServer::new(app).unwrap();

        let response = server.get("/streams/metrics").await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert!(body["storage_info"]["total_space"].is_number());
        assert!(body["files_deleted"].is_number());
    }
}
