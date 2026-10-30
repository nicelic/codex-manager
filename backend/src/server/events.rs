use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use chrono::Utc;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;
use crate::api::logs::get_log_status_data;
use crate::common::process::is_pid_alive;
use crate::server::proxy::get_proxy_status_data;
use crate::state::AppState;
use crate::tools::gortex::service::GortexService;
use crate::tools::llmtrim::service::LlmtrimService;
use crate::tools::rtk::service::RtkService;
use crate::tools::snip::service::SnipService;

pub async fn events_handler(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, &'static str)> {
    // 校验 Origin 标头，保障本地与内网合法来源，防范恶意跨站 WebSocket 劫持 (CSWSH)
    if let Some(origin_val) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        let origin = origin_val.trim().to_lowercase();
        let is_allowed = origin.starts_with("http://127.0.0.1")
            || origin.starts_with("https://127.0.0.1")
            || origin.starts_with("http://localhost")
            || origin.starts_with("https://localhost")
            || origin.starts_with("http://[::1]")
            || origin.starts_with("http://0.0.0.0")
            || origin.starts_with("http://192.168.")
            || origin.starts_with("http://10.")
            || origin.starts_with("http://172.")
            || origin == "null"
            || origin.is_empty();

        if !is_allowed {
            tracing::warn!("拒绝来自不受信任 Origin 的 WebSocket 事件订阅请求: {}", origin_val);
            return Err((StatusCode::FORBIDDEN, "Forbidden: Invalid WebSocket Origin"));
        }
    }

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state)))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut rx = state.event_sender.subscribe();
    let mut last_snapshot_hash: Option<u64>;
    let mut heartbeat_counter: u32 = 0;

    // 客户端连接建立时，先立即下发一次全量初始状态快照
    {
        let now = Utc::now().to_rfc3339();
        let proxy_running = *state.proxy_running.read().await;
        let cfg = state.config.read().await.clone();
        let log_showing = {
            let mut guard = state.log_viewer_pid.write().await;
            if let Some(pid) = *guard {
                if !is_pid_alive(pid) {
                    *guard = None;
                }
            }
            guard.is_some()
        };
        let llmtrim_log_showing = {
            let mut guard = state.llmtrim_log_viewer_pid.write().await;
            if let Some(pid) = *guard {
                if !is_pid_alive(pid) {
                    *guard = None;
                }
            }
            guard.is_some()
        };

        let proxy_data = get_proxy_status_data(&state, proxy_running, &cfg.listen_address);
        let log_data = get_log_status_data(log_showing);
        let llmtrim_log_data = json!({
            "showing": llmtrim_log_showing,
            "path": "llmtrim.log"
        });
        let llmtrim_data = LlmtrimService::get_status(&cfg.llmtrim_path);
        let rtk_data = RtkService::get_status();
        let snip_data = SnipService::get_status();
        let gortex_data = GortexService::get_status();

        let payload_for_hash = json!({
            "proxy": &proxy_data,
            "logs": &log_data,
            "llmtrim_logs": &llmtrim_log_data,
            "llmtrim": &llmtrim_data,
            "rtk": &rtk_data,
            "snip": &snip_data,
            "gortex": &gortex_data,
        });

        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        payload_for_hash.to_string().hash(&mut hasher);
        last_snapshot_hash = Some(hasher.finish());

        let event = json!({
            "type": "status",
            "timestamp": now,
            "proxy": proxy_data,
            "logs": log_data,
            "llmtrim_logs": llmtrim_log_data,
            "llmtrim": llmtrim_data,
            "rtk": rtk_data,
            "snip": snip_data,
            "gortex": gortex_data,
        });

        if let Ok(text) = serde_json::to_string(&event) {
            if socket.send(Message::Text(text.into())).await.is_err() {
                return;
            }
        }
    }

    loop {
        tokio::select! {
            // 定时广播全量周期状态快照（放宽至 3 秒，且只在状态变动或长周期保活时发送）
            _ = sleep(Duration::from_secs(3)) => {
                let proxy_running = *state.proxy_running.read().await;
                let cfg = state.config.read().await.clone();
                let log_showing = {
                    let mut guard = state.log_viewer_pid.write().await;
                    if let Some(pid) = *guard {
                        if !is_pid_alive(pid) {
                            *guard = None;
                        }
                    }
                    guard.is_some()
                };
                let llmtrim_log_showing = {
                    let mut guard = state.llmtrim_log_viewer_pid.write().await;
                    if let Some(pid) = *guard {
                        if !is_pid_alive(pid) {
                            *guard = None;
                        }
                    }
                    guard.is_some()
                };

                let proxy_data = get_proxy_status_data(&state, proxy_running, &cfg.listen_address);
                let log_data = get_log_status_data(log_showing);
                let llmtrim_log_data = json!({
                    "showing": llmtrim_log_showing,
                    "path": "llmtrim.log"
                });
                let llmtrim_data = LlmtrimService::get_status(&cfg.llmtrim_path);
                let rtk_data = RtkService::get_status();
                let snip_data = SnipService::get_status();
                let gortex_data = GortexService::get_status();

                let payload_for_hash = json!({
                    "proxy": &proxy_data,
                    "logs": &log_data,
                    "llmtrim_logs": &llmtrim_log_data,
                    "llmtrim": &llmtrim_data,
                    "rtk": &rtk_data,
                    "snip": &snip_data,
                    "gortex": &gortex_data,
                });

                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                payload_for_hash.to_string().hash(&mut hasher);
                let current_hash = hasher.finish();

                heartbeat_counter += 1;
                let changed = last_snapshot_hash != Some(current_hash);

                // 仅当状态发生改变，或者连续 10 次周期（30 秒）无变动时发送心跳快照
                if changed || heartbeat_counter >= 10 {
                    last_snapshot_hash = Some(current_hash);
                    heartbeat_counter = 0;
                    let now = Utc::now().to_rfc3339();
                    let event = json!({
                        "type": "status",
                        "timestamp": now,
                        "proxy": proxy_data,
                        "logs": log_data,
                        "llmtrim_logs": llmtrim_log_data,
                        "llmtrim": llmtrim_data,
                        "rtk": rtk_data,
                        "snip": snip_data,
                        "gortex": gortex_data,
                    });

                    if let Ok(text) = serde_json::to_string(&event) {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
            // 接收即时广播事件
            Ok(event_data) = rx.recv() => {
                last_snapshot_hash = None; // 即时事件发生后，使快照比对失效，确保下轮巡检立即同步
                heartbeat_counter = 0;
                if let Ok(text) = serde_json::to_string(&event_data) {
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
            }
            // 接收来自客户端的消息（心跳或 ping/pong/close）
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(p))) => {
                        if socket.send(Message::Pong(p)).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/events", get(events_handler))
}
