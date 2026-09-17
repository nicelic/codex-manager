use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub local_http1: i64,
    pub local_ws: usize,
    pub upstream_h2: usize,
    pub upstream_h2_ws: bool,
    pub upstream_h3: usize,
    pub upstream_h3_ws: bool,
    pub upstream_ws: usize,
    pub upstream_streams: usize,
}

impl Default for ConnectionStatus {
    fn default() -> Self {
        Self {
            local_http1: 0,
            local_ws: 0,
            upstream_h2: 0,
            upstream_h2_ws: false,
            upstream_h3: 0,
            upstream_h3_ws: false,
            upstream_ws: 0,
            upstream_streams: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyStatusResponse {
    pub running: bool,
    pub state: String,
    pub process_id: u32,
    pub listen_address: String,
    pub message: String,
    pub connections: ConnectionStatus,
}

pub async fn get_proxy_status(State(state): State<AppState>) -> Json<ProxyStatusResponse> {
    let running = *state.proxy_running.read().await;
    let cfg = state.config.read().await;
    let process_id = std::process::id();

    let (proxy_state, message) = if running {
        ("running".to_string(), "HTTP 代理转发已启动。".to_string())
    } else {
        ("stopped".to_string(), "代理转发已停止，请在网页中点击“启动代理”。".to_string())
    };

    Json(ProxyStatusResponse {
        running,
        state: proxy_state,
        process_id,
        listen_address: cfg.listen_address.clone(),
        message,
        connections: ConnectionStatus::default(),
    })
}

pub async fn start_proxy(State(state): State<AppState>) -> impl IntoResponse {
    let mut running = state.proxy_running.write().await;
    *running = true;
    (StatusCode::OK, Json(json!({ "message": "代理已启动", "success": true })))
}

pub async fn stop_proxy(State(state): State<AppState>) -> impl IntoResponse {
    let mut running = state.proxy_running.write().await;
    *running = false;
    (StatusCode::OK, Json(json!({ "message": "代理已停止", "success": true })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/proxy", get(get_proxy_status))
        .route("/api/proxy/start", post(start_proxy))
        .route("/api/proxy/stop", post(stop_proxy))
}
