use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsResponse {
    pub listen_address: String,
    pub upstream_base_url: String,
    pub upstream_api_key: String,
    pub upstream_websocket_enabled: bool,
    pub startup_enabled: bool,
    pub background_start: bool,
    pub retry_enabled: bool,
    pub retry_count: String,
    pub retry_interval_seconds: String,
    pub retry_status_codes: String,
}

#[derive(Debug, Deserialize)]
pub struct SettingUpdateRequest {
    pub value: String,
}

pub async fn get_settings(State(state): State<AppState>) -> Json<SettingsResponse> {
    let cfg = state.config.read().await;
    Json(SettingsResponse {
        listen_address: cfg.listen_address.clone(),
        upstream_base_url: cfg.upstream_base_url.clone(),
        upstream_api_key: cfg.upstream_api_key.clone(),
        upstream_websocket_enabled: cfg.upstream_websocket_enabled,
        startup_enabled: cfg.startup_enabled,
        background_start: cfg.background_start,
        retry_enabled: cfg.retry_enabled,
        retry_count: cfg.retry_count.clone(),
        retry_interval_seconds: cfg.retry_interval_seconds.clone(),
        retry_status_codes: cfg.retry_status_codes.clone(),
    })
}

pub async fn update_setting(
    Path(setting): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<SettingUpdateRequest>,
) -> impl IntoResponse {
    let mut cfg = state.config.write().await;
    let val = payload.value.trim().to_string();

    match setting.as_str() {
        "listen_address" => cfg.listen_address = val,
        "upstream_base_url" => cfg.upstream_base_url = val,
        "upstream_api_key" => cfg.upstream_api_key = val,
        "upstream_websocket_enabled" => {
            cfg.upstream_websocket_enabled = val.parse::<bool>().unwrap_or(true);
        }
        "startup_enabled" => {
            cfg.startup_enabled = val.parse::<bool>().unwrap_or(false);
            let _ = crate::common::windows::sync_startup_registry(cfg.startup_enabled, cfg.background_start);
        }
        "background_start" => {
            cfg.background_start = val.parse::<bool>().unwrap_or(false);
            if cfg.startup_enabled {
                let _ = crate::common::windows::sync_startup_registry(cfg.startup_enabled, cfg.background_start);
            }
        }
        "retry_enabled" => {
            cfg.retry_enabled = val.parse::<bool>().unwrap_or(false);
        }
        "retry_count" => cfg.retry_count = val,
        "retry_interval_seconds" => cfg.retry_interval_seconds = val,
        "retry_status_codes" => cfg.retry_status_codes = val,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "message": format!("未知设置项: {}", setting) })),
            );
        }
    }

    let _ = cfg.save(&state.config_path);

    (
        StatusCode::OK,
        Json(json!({ "message": "配置更新成功", "success": true })),
    )
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/settings", get(get_settings))
        .route("/api/settings/{setting}", put(update_setting).post(update_setting))
}
