use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use crate::state::AppState;
use super::service::LlmtrimService;
use super::types::LlmtrimInstallRequest;

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
    let proxy = {
        let cfg = state.config.read().await;
        if cfg.outbound_proxy.is_empty() {
            None
        } else {
            Some(cfg.outbound_proxy.clone())
        }
    };

    match LlmtrimService::install(payload.tag_name, proxy.as_deref()).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "llmtrim 安装任务已完成", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn start() -> impl IntoResponse {
    match LlmtrimService::start() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "llmtrim 守护进程已启动", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn stop() -> impl IntoResponse {
    match LlmtrimService::stop() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "llmtrim 已停止", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn uninstall() -> impl IntoResponse {
    match LlmtrimService::uninstall() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "llmtrim 已卸载", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn get_logs(State(state): State<AppState>) -> impl IntoResponse {
    let showing = *state.llmtrim_log_showing.read().await;
    Json(json!({
        "showing": showing,
        "path": "llmtrim.log"
    }))
}

pub async fn show_logs(State(state): State<AppState>) -> impl IntoResponse {
    let mut showing = state.llmtrim_log_showing.write().await;
    *showing = true;
    let _ = LlmtrimService::show_logs();
    (StatusCode::OK, Json(json!({ "showing": true })))
}

pub async fn hide_logs(State(state): State<AppState>) -> impl IntoResponse {
    let mut showing = state.llmtrim_log_showing.write().await;
    *showing = false;
    (StatusCode::OK, Json(json!({ "showing": false })))
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
