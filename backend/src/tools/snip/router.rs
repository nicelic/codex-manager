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
use super::service::SnipService;
use super::types::SnipInstallRequest;

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub page: Option<usize>,
}

pub async fn get_status() -> impl IntoResponse {
    Json(SnipService::get_status())
}

pub async fn get_releases(Query(query): Query<PageQuery>) -> impl IntoResponse {
    let page = query.page.unwrap_or(1);
    Json(SnipService::get_releases(page))
}

pub async fn install(Json(_payload): Json<SnipInstallRequest>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "snip 安装成功", "success": true })),
    )
}

pub async fn start() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "snip 已启动", "success": true })),
    )
}

pub async fn stop() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "snip 已停止", "success": true })),
    )
}

pub async fn trust() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "snip 信任设置已生效", "success": true })),
    )
}

pub async fn uninstall() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "snip 已卸载", "success": true })),
    )
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
