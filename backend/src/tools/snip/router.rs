use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

use super::service::SnipService;
use super::types::SnipInstallRequest;
use crate::state::AppState;

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
        Ok(res) => (StatusCode::OK, Json(json!(res))),
        Err(err) => {
            let status = if err.contains("正在运行") || err.contains("请先停止") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_GATEWAY
            };
            (status, Json(json!({ "message": format!("安装 snip 失败: {}", err), "success": false })))
        }
    }
}

pub async fn start() -> impl IntoResponse {
    match SnipService::start() {
        Ok(res) => (StatusCode::OK, Json(json!(res))),
        Err(err) => {
            let status = if err.contains("正在运行") || err.contains("请先停止") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_GATEWAY
            };
            (status, Json(json!({ "message": format!("启动 snip 失败: {}", err), "success": false })))
        }
    }
}

pub async fn stop() -> impl IntoResponse {
    match SnipService::stop() {
        Ok(res) => (StatusCode::OK, Json(json!(res))),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "message": format!("snip 操作失败: {}", err), "success": false })),
        ),
    }
}

pub async fn trust() -> impl IntoResponse {
    match SnipService::launch_trust() {
        Ok(res) => (StatusCode::OK, Json(json!(res))),
        Err(err) => {
            let status = if err.contains("尚未安装") || err.contains("未检测到") || err.contains("请先") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_GATEWAY
            };
            (status, Json(json!({ "message": format!("Codex 信任失败: {}", err), "success": false })))
        }
    }
}

pub async fn uninstall() -> impl IntoResponse {
    match SnipService::uninstall() {
        Ok(res) => (StatusCode::OK, Json(json!(res))),
        Err(err) => {
            let status = if err.contains("正在运行") || err.contains("请先停止") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_GATEWAY
            };
            (status, Json(json!({ "message": format!("删除 snip 失败: {}", err), "success": false })))
        }
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
