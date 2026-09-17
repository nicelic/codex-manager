use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use crate::state::AppState;

pub async fn identity() -> impl IntoResponse {
    Json(json!({
        "version": "1.0.0",
        "is_dev_mode": false,
        "name": "code-Manager"
    }))
}

pub async fn releases() -> impl IntoResponse {
    Json(json!({
        "releases": [
            {
                "tag_name": "v1.0.0",
                "name": "v1.0.0 正式发布",
                "published_at": "2026-09-18T00:00:00Z",
                "prerelease": false,
                "available": true,
                "asset_name": "code-Manager.exe"
            }
        ],
        "page": 1,
        "has_more": false,
        "has_update": false,
        "latest_version": "v1.0.0",
        "is_dev_mode": false
    }))
}

pub async fn update() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "message": "当前已是最新版本或处于受管状态",
            "success": true
        })),
    )
}

pub async fn exit_app(State(state): State<AppState>) -> impl IntoResponse {
    let sender = state.shutdown_sender.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        let _ = sender.send(());
    });

    (
        StatusCode::OK,
        Json(json!({
            "message": "正在停止所有服务并退出...",
            "success": true
        })),
    )
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/application/identity", get(identity))
        .route("/api/application/releases", get(releases))
        .route("/api/application/update", post(update))
        .route("/api/application/exit", post(exit_app))
}
