use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use crate::common::windows::open_log_viewer;
use crate::state::AppState;
use std::fs;
use std::path::PathBuf;

fn get_log_file_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join("code-Manager.log");
        }
    }
    std::env::temp_dir().join("code-Manager.log")
}

pub async fn get_logs(State(state): State<AppState>) -> impl IntoResponse {
    let showing = *state.log_showing.read().await;
    let path = get_log_file_path();
    Json(json!({
        "showing": showing,
        "path": path.to_string_lossy().to_string(),
        "message": if showing { "日志窗口已打开" } else { "日志窗口已隐藏" }
    }))
}

pub async fn show_logs(State(state): State<AppState>) -> impl IntoResponse {
    let mut showing = state.log_showing.write().await;
    *showing = true;

    let log_path = get_log_file_path();
    if !log_path.exists() {
        let _ = fs::write(&log_path, "=== code-Manager 实时运行日志 ===\r\n服务已正常就绪。\r\n");
    }

    let _ = open_log_viewer("code-Manager 实时日志", &log_path);

    (StatusCode::OK, Json(json!({ "showing": true, "message": "已在独立控制台启动日志监视器" })))
}

pub async fn hide_logs(State(state): State<AppState>) -> impl IntoResponse {
    let mut showing = state.log_showing.write().await;
    *showing = false;
    (StatusCode::OK, Json(json!({ "showing": false, "message": "已隐藏日志" })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/logs", get(get_logs))
        .route("/api/logs/show", post(show_logs))
        .route("/api/logs/hide", post(hide_logs))
}
