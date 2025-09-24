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
            // API routes
            .route("/health", get(health_check))
            .route("/metrics", get(get_metrics))
            .route("/start", post(start_recording))
            .route("/stop", post(stop_recording))
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
