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
    pub outbound_proxy: String,
    pub local_api_key: String,
    pub llmtrim_path: String,
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
        outbound_proxy: cfg.outbound_proxy.clone(),
        local_api_key: cfg.local_api_key.clone(),
        llmtrim_path: cfg.llmtrim_path.clone(),
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
    let mut val = payload.value.trim().to_string();
    let message: String;

    match setting.as_str() {
        "listen_address" => {
            let socket_addr = match crate::common::config::validate_listen_address(&val) {
                Ok(addr) => addr,
                Err(err_msg) => {
                    return (StatusCode::BAD_REQUEST, Json(json!({ "saved": false, "message": err_msg }))).into_response();
                }
            };
            val = socket_addr.to_string();
            cfg.listen_address = val.clone();
            message = "监听地址已保存到 config.yaml，将在下次重启或重新启动代理时生效".to_string();
        }
        "upstream_base_url" => {
            if val.is_empty() {
                return (StatusCode::BAD_REQUEST, Json(json!({ "saved": false, "message": "上游 Base URL 不能为空" }))).into_response();
            }
            if let Err(e) = url::Url::parse(&val) {
                return (StatusCode::BAD_REQUEST, Json(json!({ "saved": false, "message": format!("无效的 URL 格式: {}", e) }))).into_response();
            }
            cfg.upstream_base_url = val.clone();
            let _ = crate::tools::llmtrim::service::LlmtrimService::sync_extra_hosts(&val);
            message = "已保存到 config.yaml，并接管同步 llmtrim extra_hosts；停止后再次启动 llmtrim 才会使用新的主机和 CA。".to_string();
        }
        "upstream_api_key" => {
            cfg.upstream_api_key = val.clone();
            message = "上游 API Key 已更新".to_string();
        }
        "outbound_proxy" => {
            cfg.outbound_proxy = val.clone();
            message = "出站代理已更新".to_string();
        }
        "local_api_key" => {
            cfg.local_api_key = val.clone();
            message = "本地 API Key 已更新".to_string();
        }
        "llmtrim_path" => {
            cfg.llmtrim_path = val.clone();
            message = "llmtrim 路径已更新".to_string();
        }
        "upstream_websocket_enabled" => {
            let enabled = val.parse::<bool>().unwrap_or(true);
            cfg.upstream_websocket_enabled = enabled;
            message = if enabled { "上游 WebSocket 已启用" } else { "上游 WebSocket 已禁用" }.to_string();
        }
        "startup_enabled" => {
            let enabled = val.parse::<bool>().unwrap_or(false);
            if !enabled {
                cfg.background_start = false;
            }
            cfg.startup_enabled = enabled;
            let _ = crate::common::windows::sync_startup_registry(cfg.startup_enabled, cfg.background_start);
            message = if enabled { "开机启动已开启" } else { "开机启动已关闭" }.to_string();
        }
        "background_start" => {
            let enabled = val.parse::<bool>().unwrap_or(false);
            if enabled && !cfg.startup_enabled {
                return (StatusCode::BAD_REQUEST, Json(json!({ "saved": false, "message": "后台运行必须先开启开机启动" }))).into_response();
            }
            cfg.background_start = enabled;
            if cfg.startup_enabled {
                let _ = crate::common::windows::sync_startup_registry(cfg.startup_enabled, cfg.background_start);
            }
            message = if enabled { "后台运行已开启，开机启动时不自动打开网页" } else { "后台运行已关闭" }.to_string();
        }
        "retry_enabled" => {
            let enabled = val.parse::<bool>().unwrap_or(false);
            cfg.retry_enabled = enabled;
            message = if enabled { "自动重试已开启" } else { "自动重试已关闭" }.to_string();
        }
        "retry_count" => {
            let parsed: usize = val.parse().unwrap_or(5);
            let clamped = parsed.min(999);
            val = clamped.to_string();
            cfg.retry_count = val.clone();
            message = "重试次数已更新".to_string();
        }
        "retry_interval_seconds" => {
            let parsed: u64 = val.parse().unwrap_or(1);
            let clamped = parsed.max(1).min(3600);
            val = clamped.to_string();
            cfg.retry_interval_seconds = val.clone();
            message = "重试间隔已更新".to_string();
        }
        "retry_status_codes" => {
            let (normalized, _) = crate::common::config::normalize_retry_status_codes(&val);
            if !val.is_empty() && normalized.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "saved": false, "message": "无效的状态码范围（允许 100-599 区间或数字，多个以逗号分隔）" })),
                ).into_response();
            }
            cfg.retry_status_codes = normalized.clone();
            val = normalized;
            message = "重试状态码范围已更新并已规范化".to_string();
        }
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "saved": false, "message": format!("未知设置项: {}", setting) })),
            ).into_response();
        }
    }

    if let Err(e) = cfg.save(&state.config_path) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "saved": false, "message": format!("保存配置文件失败: {}", e) })),
        ).into_response();
    }

    (
        StatusCode::OK,
        Json(json!({ "saved": true, "message": message, "value": val })),
    ).into_response()
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/settings", get(get_settings).post(get_settings))
        .route("/api/settings/{setting}", put(update_setting).post(update_setting))
}
