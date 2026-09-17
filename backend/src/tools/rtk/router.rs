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
use super::service::RtkService;
use super::types::RtkInstallRequest;

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub page: Option<usize>,
}

pub async fn get_status() -> impl IntoResponse {
    Json(RtkService::get_status())
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

    match RtkService::fetch_releases(page, proxy.as_deref()).await {
        Ok(res) => (StatusCode::OK, Json(json!(res))),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": err, "releases": [], "page": page, "has_more": false })),
        ),
    }
}

pub async fn install(
    State(state): State<AppState>,
    Json(payload): Json<RtkInstallRequest>,
) -> impl IntoResponse {
    let proxy = {
        let cfg = state.config.read().await;
        if cfg.outbound_proxy.is_empty() {
            None
        } else {
            Some(cfg.outbound_proxy.clone())
        }
    };

    match RtkService::install(payload.tag_name, proxy.as_deref()).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "RTK 安装成功，PATH 与接入规则已配置", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn start() -> impl IntoResponse {
    match RtkService::start() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "RTK 已启动，环境配置已生效", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn stop() -> impl IntoResponse {
    match RtkService::stop() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "RTK 已停止，Hook 与 PATH 已停用", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn uninstall() -> impl IntoResponse {
    match RtkService::uninstall() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "RTK 已完全卸载", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/rtk", get(get_status))
        .route("/api/rtk/releases", get(get_releases))
        .route("/api/rtk/install", post(install))
        .route("/api/rtk/start", post(start))
        .route("/api/rtk/stop", post(stop))
        .route("/api/rtk/uninstall", post(uninstall))
}
