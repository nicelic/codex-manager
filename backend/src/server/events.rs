use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use chrono::Utc;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;
use crate::state::AppState;

pub async fn events_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut rx = state.event_sender.subscribe();

    loop {
        tokio::select! {
            // 定时广播周期状态快照
            _ = sleep(Duration::from_secs(1)) => {
                let now = Utc::now().to_rfc3339();
                let proxy_running = *state.proxy_running.read().await;
                let cfg = state.config.read().await;

                let event = json!({
                    "type": "status",
                    "timestamp": now,
                    "proxy": {
                        "running": proxy_running,
                        "state": if proxy_running { "running" } else { "stopped" },
                        "process_id": std::process::id(),
                        "listen_address": cfg.listen_address,
                        "message": if proxy_running { "HTTP 代理转发已启动。" } else { "代理转发已停止，请在网页中点击“启动代理”。" },
                        "connections": {
                            "local_http1": 0,
                            "local_ws": 1,
                            "upstream_h2": 0,
                            "upstream_h2_ws": false,
                            "upstream_h3": 0,
                            "upstream_h3_ws": false,
                            "upstream_ws": 0,
                            "upstream_streams": 0
                        }
                    },
                    "logs": {
                        "showing": *state.log_showing.read().await
                    },
                    "llmtrim_logs": {
                        "showing": *state.llmtrim_log_showing.read().await
                    }
                });

                if let Ok(text) = serde_json::to_string(&event) {
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
            }
            // 接收即时广播事件
            Ok(event_data) = rx.recv() => {
                if let Ok(text) = serde_json::to_string(&event_data) {
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
            }
            // 接收来自客户端的消息（心跳或 ping）
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
