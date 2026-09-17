use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use crate::state::AppState;

pub async fn get_logs(State(state): State<AppState>) -> impl IntoResponse {
    let showing = *state.log_showing.read().await;
    Json(json!({
        "showing": showing,
        "path": "code-Manager.log",
        "message": if showing { "日志窗口已打开" } else { "日志窗口已隐藏" }
    }))
}

pub async fn show_logs(State(state): State<AppState>) -> impl IntoResponse {
    let mut showing = state.log_showing.write().await;
    *showing = true;
    (StatusCode::OK, Json(json!({ "showing": true, "message": "已显示日志" })))
}

pub async fn hide_logs(State(state): State<AppState>) -> impl IntoResponse {
    let mut showing = state.log_showing.write().await;
    *showing = false;
    (StatusCode::OK, Json(json!({ "showing": false, "message": "已隐藏日志" })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/logs", get(get_logs))
        .route("/api/logs/show", post(show_logs))
        .route("/api/logs/hide", post(hide_logs))
}
