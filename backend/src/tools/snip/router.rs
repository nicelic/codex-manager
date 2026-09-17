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
use super::service::SnipService;
use super::types::SnipInstallRequest;

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub page: Option<usize>,
}

pub async fn get_status() -> impl IntoResponse {
    Json(SnipService::get_status())
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

    match SnipService::fetch_releases(page, proxy.as_deref()).await {
        Ok(res) => (StatusCode::OK, Json(json!(res))),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": err, "releases": [], "page": page, "has_more": false })),
        ),
    }
}

pub async fn install(
    State(state): State<AppState>,
    Json(payload): Json<SnipInstallRequest>,
) -> impl IntoResponse {
    let proxy = {
        let cfg = state.config.read().await;
        if cfg.outbound_proxy.is_empty() {
            None
        } else {
            Some(cfg.outbound_proxy.clone())
        }
    };

    match SnipService::install(payload.tag_name, proxy.as_deref()).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "snip 安装成功，Hook 与环境已就绪", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn start() -> impl IntoResponse {
    match SnipService::start() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "snip 已启动", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn stop() -> impl IntoResponse {
    match SnipService::stop() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "snip 已停止", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn trust() -> impl IntoResponse {
    match SnipService::launch_trust() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "已在独立控制台启动 Codex 信任审核引导", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn uninstall() -> impl IntoResponse {
    match SnipService::uninstall() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "snip 已卸载", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/snip", get(get_status))
        .route("/api/snip/releases", get(get_releases))
        .route("/api/snip/install", post(install))
        .route("/api/snip/start", post(start))
        .route("/api/snip/stop", post(stop))
        .route("/api/snip/trust", post(trust))
        .route("/api/snip/uninstall", post(uninstall))
}
