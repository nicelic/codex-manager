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
        Ok(res) => (StatusCode::OK, Json(json!(res))),
        Err(err) => {
            let status = if err.contains("正在运行") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_GATEWAY
            };
            (status, Json(json!({ "message": format!("安装 RTK 失败: {}", err) })))
        }
    }
}

pub async fn start() -> impl IntoResponse {
    match RtkService::start() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({
                "running": true,
                "desired_running": true,
                "activation_state": "running",
                "message": "RTK 已激活。PATH 已配置；仅检测到且可安全写入的平台会尝试接入，请查看各平台状态。"
            })),
        ),
        Err(err) => {
            let status = if err.contains("正在运行") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_GATEWAY
            };
            (status, Json(json!({ "message": format!("启动 RTK 失败: {}", err) })))
        }
    }
}

pub async fn stop() -> impl IntoResponse {
    match RtkService::stop() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({
                "running": false,
                "desired_running": false,
                "activation_state": "installed_stopped",
                "message": "RTK 已停止，当前受管 PATH、提示词和 Hook 已清理；未受管内容会保留。"
            })),
        ),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "message": format!("停止 RTK 失败: {}", err) })),
        ),
    }
}

pub async fn uninstall() -> impl IntoResponse {
    match RtkService::uninstall() {
        Ok(res) => (StatusCode::OK, Json(json!(res))),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "message": format!("删除 RTK 失败: {}", err) })),
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
