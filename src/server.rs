use crate::config::Config;
use crate::disk_manager::{DiskManager, StorageInfo};
use crate::recorder::{Recorder, RecordingStats};
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing::info;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
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

#[derive(Deserialize)]
pub struct StartRequest {
    pub force: Option<bool>,
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
            config: self.config.clone(),
            recorder: self.recorder,
            disk_manager: self.disk_manager,
        };

        let app = Router::new()
            .route("/health", get(health_check))
            .route("/metrics", get(get_metrics))
            .route("/start", post(start_recording))
            .route("/stop", post(stop_recording))
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
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let response = HealthResponse {
        status: "healthy".to_string(),
        uptime: 0, // You'd implement actual uptime tracking
        timestamp: chrono::Utc::now().to_rfc3339(),
        recording: state.recorder.is_recording().await,
        storage,
    };

    Ok(Json(response))
}

async fn get_metrics(State(state): State<AppState>) -> Result<Json<MetricsResponse>, StatusCode> {
    let recording_stats = state.recorder.get_stats().await;
    let storage_info = state
        .disk_manager
        .get_storage_info()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let files_deleted = state.disk_manager.get_files_deleted();
    let last_cleanup = state
        .disk_manager
        .get_last_cleanup()
        .await
        .map(|dt| dt.to_rfc3339());

    let response = MetricsResponse {
        recording_stats,
        storage_info,
        files_deleted,
        last_cleanup,
    };

    Ok(Json(response))
}

async fn start_recording(State(state): State<AppState>) -> Result<String, StatusCode> {
    state
        .recorder
        .start()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok("Recording started".to_string())
}

async fn stop_recording(State(state): State<AppState>) -> Result<String, StatusCode> {
    state
        .recorder
        .stop()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok("Recording stopped".to_string())
}
