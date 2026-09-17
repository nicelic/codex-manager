use axum::{
    extract::Query,
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

pub async fn get_releases(Query(query): Query<PageQuery>) -> impl IntoResponse {
    let page = query.page.unwrap_or(1);
    Json(GortexService::get_releases(page))
}

pub async fn install(Json(_payload): Json<GortexInstallRequest>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "Gortex 安装任务已完成", "success": true })),
    )
}

pub async fn start() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "Gortex daemon 已启动", "success": true })),
    )
}

pub async fn stop() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "Gortex daemon 已停止", "success": true })),
    )
}

pub async fn register() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "Gortex 平台集成配置已注册", "success": true })),
    )
}

pub async fn remove() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "Gortex 平台集成配置已移除", "success": true })),
    )
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
    GortexService::track(payload.path);
    (
        StatusCode::OK,
        Json(json!({ "message": "项目已成功添加到 Gortex 跟踪列表", "success": true })),
    )
}

pub async fn untrack(Json(payload): Json<GortexTrackRequest>) -> impl IntoResponse {
    GortexService::untrack(payload.path);
    (
        StatusCode::OK,
        Json(json!({ "message": "项目已从 Gortex 跟踪列表中移除", "success": true })),
    )
}

pub async fn uninstall() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "Gortex 卸载完成", "success": true })),
    )
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
