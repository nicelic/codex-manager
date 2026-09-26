use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

use super::service::LlmtrimService;
use super::types::{
    LlmtrimInstallRequest, LlmtrimInstallResponse, LlmtrimUninstallResponse,
};
use crate::common::process::{is_pid_alive, kill_process_by_pid, wait_for_process_exit};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub page: Option<usize>,
}

pub async fn get_status(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.config.read().await;
    Json(LlmtrimService::get_status(&cfg.llmtrim_path))
}

pub async fn get_releases(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> impl IntoResponse {
    let page = query.page.unwrap_or(1);
    let proxy = {
        let cfg = state.config.read().await;
        if cfg.outbound_proxy.is_empty() {
            None
        } else {
            Some(cfg.outbound_proxy.clone())
        }
    };

    match LlmtrimService::fetch_releases(page, proxy.as_deref()).await {
        Ok(res) => (StatusCode::OK, Json(json!(res))),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": err, "releases": [], "page": page, "has_more": false })),
        ),
    }
}

pub async fn install(
    State(state): State<AppState>,
    Json(payload): Json<LlmtrimInstallRequest>,
) -> impl IntoResponse {
    let (upstream_url, proxy) = {
        let cfg = state.config.read().await;
        let p = if cfg.outbound_proxy.is_empty() {
            None
        } else {
            Some(cfg.outbound_proxy.clone())
        };
        (cfg.upstream_base_url.clone(), p)
    };

    // 重置已缓存的 llmtrim HTTP 客户端连接池
    {
        let mut cached = state.llmtrim_client.write().await;
        *cached = None;
    }

    match LlmtrimService::install(payload.tag_name, &upstream_url, proxy.as_deref()).await {
        Ok(()) => {
            let cfg = state.config.read().await;
            let status = LlmtrimService::get_status(&cfg.llmtrim_path);
            (
                StatusCode::OK,
                Json(json!(LlmtrimInstallResponse {
                    path: status.path,
                    version: status.version,
                    running: status.running,
                    configured: status.configured,
                    process_id: status.process_id,
                    port: status.port,
                    message: "llmtrim 已安装并配置，守护进程已启动。".to_string(),
                })),
            )
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn start(State(state): State<AppState>) -> impl IntoResponse {
    let (path, upstream_url) = {
        let cfg = state.config.read().await;
        (cfg.llmtrim_path.clone(), cfg.upstream_base_url.clone())
    };

    // 重置已缓存的 llmtrim HTTP 客户端连接池
    {
        let mut cached = state.llmtrim_client.write().await;
        *cached = None;
    }

    match LlmtrimService::start(&path, &upstream_url) {
        Ok(()) => {
            let status = LlmtrimService::get_status(&path);
            (
                StatusCode::OK,
                Json(json!({
                    "message": "llmtrim 守护进程已启动并确认正在运行。",
                    "success": true,
                    "running": true,
                    "status": status,
                })),
            )
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn stop(State(state): State<AppState>) -> impl IntoResponse {
    let path = {
        let cfg = state.config.read().await;
        cfg.llmtrim_path.clone()
    };

    // 丢弃旧连接池
    {
        let mut cached = state.llmtrim_client.write().await;
        *cached = None;
    }

    match LlmtrimService::stop(&path) {
        Ok(()) => {
            let status = LlmtrimService::get_status(&path);
            (
                StatusCode::OK,
                Json(json!({
                    "message": "llmtrim 和 llmtrim-tray 已停止，相关自启动和用户环境变量已清理。",
                    "success": true,
                    "running": false,
                    "status": status,
                })),
            )
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn uninstall(State(state): State<AppState>) -> impl IntoResponse {
    let path = {
        let cfg = state.config.read().await;
        cfg.llmtrim_path.clone()
    };

    // 丢弃旧连接池
    {
        let mut cached = state.llmtrim_client.write().await;
        *cached = None;
    }

    match LlmtrimService::uninstall(&path) {
        Ok(warnings) => {
            // 清空 config.yaml 中的 llmtrim_path 并保存
            {
                let mut cfg = state.config.write().await;
                cfg.llmtrim_path = String::new();
                let _ = cfg.save(&state.config_path);
            }

            (
                StatusCode::OK,
                Json(json!(LlmtrimUninstallResponse {
                    path: String::new(),
                    running: false,
                    configured: false,
                    directory_exists: false,
                    state_dir_exists: false,
                    tray_running: false,
                    residual: false,
                    process_id: 0,
                    tray_process_id: 0,
                    port: "127.0.0.1:43117".to_string(),
                    message: "llmtrim 和 llmtrim-tray 已停止，受管安装目录、统计数据库、用户 CA 与环境配置已彻底清理。".to_string(),
                    warnings,
                })),
            )
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn get_logs(State(state): State<AppState>) -> impl IntoResponse {
    let mut pid_guard = state.llmtrim_log_viewer_pid.write().await;
    if let Some(pid) = *pid_guard {
        if !is_pid_alive(pid) {
            *pid_guard = None;
        }
    }
    let showing = pid_guard.is_some();
    Json(json!({
        "showing": showing,
        "path": "llmtrim.log"
    }))
}

pub async fn show_logs(State(state): State<AppState>) -> impl IntoResponse {
    let mut pid_guard = state.llmtrim_log_viewer_pid.write().await;
    if let Some(old_pid) = *pid_guard {
        kill_process_by_pid(old_pid);
    }
    match LlmtrimService::show_logs() {
        Ok(pid) => {
            *pid_guard = Some(pid);
            drop(pid_guard);

            let state_clone = state.clone();
            tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    wait_for_process_exit(pid);
                })
                .await;
                let mut guard = state_clone.llmtrim_log_viewer_pid.write().await;
                if *guard == Some(pid) {
                    *guard = None;
                }
            });

            (
                StatusCode::OK,
                Json(json!({ "showing": true, "message": "已在独立控制台启动 LLMTrim 日志监视器" })),
            )
        }
        Err(e) => {
            *pid_guard = None;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "showing": false, "message": format!("启动日志窗口失败: {}", e) })),
            )
        }
    }
}

pub async fn hide_logs(State(state): State<AppState>) -> impl IntoResponse {
    let mut pid_guard = state.llmtrim_log_viewer_pid.write().await;
    if let Some(pid) = *pid_guard {
        kill_process_by_pid(pid);
    }
    *pid_guard = None;
    (
        StatusCode::OK,
        Json(json!({ "showing": false, "message": "已隐藏 LLMTrim 日志" })),
    )
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/llmtrim", get(get_status))
        .route("/api/llmtrim/releases", get(get_releases))
        .route("/api/llmtrim/install", post(install))
        .route("/api/llmtrim/start", post(start))
        .route("/api/llmtrim/stop", post(stop))
        .route("/api/llmtrim/uninstall", post(uninstall))
        .route("/api/llmtrim/logs", get(get_logs))
        .route("/api/llmtrim/logs/show", post(show_logs))
        .route("/api/llmtrim/logs/hide", post(hide_logs))
}
