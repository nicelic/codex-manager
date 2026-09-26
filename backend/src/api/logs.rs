use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use crate::common::config::runtime_config_dir;
use crate::common::process::{is_pid_alive, kill_process_by_pid, wait_for_process_exit};
use crate::common::windows::open_log_viewer;
use crate::state::AppState;

pub fn get_log_file_path() -> PathBuf {
    runtime_config_dir().join("code-Manager-rust.log")
}

pub fn get_log_status_data(showing: bool) -> serde_json::Value {
    let path = get_log_file_path();
    json!({
        "showing": showing,
        "path": path.to_string_lossy().to_string(),
        "message": if showing { "日志窗口已打开" } else { "日志窗口已隐藏" }
    })
}

pub async fn get_logs(State(state): State<AppState>) -> impl IntoResponse {
    let mut pid_guard = state.log_viewer_pid.write().await;
    if let Some(pid) = *pid_guard {
        if !is_pid_alive(pid) {
            *pid_guard = None;
        }
    }
    let showing = pid_guard.is_some();
    Json(get_log_status_data(showing))
}

pub async fn show_logs(State(state): State<AppState>) -> impl IntoResponse {
    let log_path = get_log_file_path();
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if !log_path.exists() {
        let _ = fs::write(&log_path, "=== code-Manager-rust 实时运行日志 ===\r\n服务已正常就绪。\r\n");
    }

    let mut pid_guard = state.log_viewer_pid.write().await;
    if let Some(old_pid) = *pid_guard {
        kill_process_by_pid(old_pid);
    }

    match open_log_viewer("code-Manager-rust 实时日志", &log_path) {
        Ok(pid) => {
            *pid_guard = Some(pid);
            drop(pid_guard);

            // 启动后台异步任务，监听进程自然退出（如用户在控制台点击 X 关闭），自动同步状态
            let state_clone = state.clone();
            tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    wait_for_process_exit(pid);
                })
                .await;
                let mut guard = state_clone.log_viewer_pid.write().await;
                if *guard == Some(pid) {
                    *guard = None;
                }
            });

            (StatusCode::OK, Json(json!({ "showing": true, "message": "已在独立控制台启动日志监视器" })))
        }
        Err(e) => {
            *pid_guard = None;
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "showing": false, "message": format!("启动日志窗口失败: {}", e) })))
        }
    }
}

pub async fn hide_logs(State(state): State<AppState>) -> impl IntoResponse {
    let mut pid_guard = state.log_viewer_pid.write().await;
    if let Some(pid) = *pid_guard {
        kill_process_by_pid(pid);
    }
    *pid_guard = None;
    (StatusCode::OK, Json(json!({ "showing": false, "message": "已关闭日志窗口" })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/logs", get(get_logs))
        .route("/api/logs/show", post(show_logs))
        .route("/api/logs/hide", post(hide_logs))
}
