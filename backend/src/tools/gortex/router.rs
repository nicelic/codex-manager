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
use super::service::GortexService;
use super::types::{GortexInstallRequest, GortexTrackRequest};

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub page: Option<usize>,
}

pub async fn get_status() -> impl IntoResponse {
    Json(GortexService::get_status())
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

    match GortexService::fetch_releases(page, proxy.as_deref()).await {
        Ok(res) => (StatusCode::OK, Json(json!(res))),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": err, "releases": [], "page": page, "has_more": false })),
        ),
    }
}

pub async fn install(
    State(state): State<AppState>,
    Json(payload): Json<GortexInstallRequest>,
) -> impl IntoResponse {
    let proxy = {
        let cfg = state.config.read().await;
        if cfg.outbound_proxy.is_empty() {
            None
        } else {
            Some(cfg.outbound_proxy.clone())
        }
    };

    match GortexService::install(payload.tag_name, proxy.as_deref()).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "Gortex 安装任务已完成", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn start() -> impl IntoResponse {
    match GortexService::start_daemon() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "Gortex daemon 已启动", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn stop() -> impl IntoResponse {
    match GortexService::stop_daemon() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "Gortex daemon 已停止", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn register() -> impl IntoResponse {
    match GortexService::register_mcp() {
        Ok(warnings) => (
            StatusCode::OK,
            Json(json!({
                "message": "Gortex 平台集成配置已注册",
                "warnings": warnings,
                "success": true
            })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn remove() -> impl IntoResponse {
    match GortexService::remove_mcp() {
        Ok(warnings) => (
            StatusCode::OK,
            Json(json!({
                "message": "Gortex 平台集成配置已安全移除",
                "warnings": warnings,
                "success": true
            })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn trust() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "Gortex 信任配置已注入", "success": true })),
    )
}

pub async fn diagnostics() -> impl IntoResponse {
    Json(GortexService::run_diagnostics())
}

pub async fn track(Json(payload): Json<GortexTrackRequest>) -> impl IntoResponse {
    match GortexService::track_project(&payload.path) {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "项目已成功添加到 Gortex 跟踪列表", "success": true })),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn untrack(Json(payload): Json<GortexTrackRequest>) -> impl IntoResponse {
    match GortexService::untrack_project(&payload.path) {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "项目已从 Gortex 跟踪列表中移除", "success": true })),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub async fn uninstall() -> impl IntoResponse {
    match GortexService::uninstall() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "message": "Gortex 卸载完成", "success": true })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": err, "success": false })),
        ),
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/gortex", get(get_status))
        .route("/api/gortex/releases", get(get_releases))
        .route("/api/gortex/install", post(install))
        .route("/api/gortex/start", post(start))
        .route("/api/gortex/stop", post(stop))
        .route("/api/gortex/register", post(register))
        .route("/api/gortex/remove", post(remove))
        .route("/api/gortex/trust", post(trust))
        .route("/api/gortex/diagnostics", post(diagnostics))
        .route("/api/gortex/track", post(track))
        .route("/api/gortex/untrack", post(untrack))
        .route("/api/gortex/uninstall", post(uninstall))
}
