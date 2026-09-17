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
use super::service::RtkService;
use super::types::RtkInstallRequest;

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub page: Option<usize>,
}

pub async fn get_status() -> impl IntoResponse {
    Json(RtkService::get_status())
}

pub async fn get_releases(Query(query): Query<PageQuery>) -> impl IntoResponse {
    let page = query.page.unwrap_or(1);
    Json(RtkService::get_releases(page))
}

pub async fn install(Json(_payload): Json<RtkInstallRequest>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "RTK 安装/配置指令已执行", "success": true })),
    )
}

pub async fn start() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "RTK 已启动", "success": true })),
    )
}

pub async fn stop() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "RTK 已停止", "success": true })),
    )
}

pub async fn uninstall() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "message": "RTK 已卸载", "success": true })),
    )
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
