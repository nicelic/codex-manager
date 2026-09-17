use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tracing::{error, warn};
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
    let local_http1 = state.active_requests.load(Ordering::Relaxed);

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
        connections: ConnectionStatus {
            local_http1,
            ..Default::default()
        },
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

/// 解析状态码范围，例如 "500-503,525-599" 或 "502"
pub fn should_retry_status(code: u16, rule_str: &str) -> bool {
    for part in rule_str.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            if let (Ok(s), Ok(e)) = (start.trim().parse::<u16>(), end.trim().parse::<u16>()) {
                if code >= s && code <= e {
                    return true;
                }
            }
        } else if let Ok(exact) = part.parse::<u16>() {
            if code == exact {
                return true;
            }
        }
    }
    false
}

struct RequestGuard(std::sync::Arc<std::sync::atomic::AtomicI64>);
impl Drop for RequestGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// 核心反向代理处理函数：流式转发 /v1/* 请求到配置的 upstream_base_url
pub async fn forward_handler(
    State(state): State<AppState>,
    req: Request,
) -> Response {
    let running = *state.proxy_running.read().await;
    let cfg = state.config.read().await.clone();

    // 1. 代理门禁检查
    if !running {
        let local_http1 = state.active_requests.load(Ordering::Relaxed);
        let resp = ProxyStatusResponse {
            running: false,
            state: "stopped".to_string(),
            process_id: std::process::id(),
            listen_address: cfg.listen_address.clone(),
            message: "代理转发已停止，请在网页中点击“启动代理”。".to_string(),
            connections: ConnectionStatus {
                local_http1,
                ..Default::default()
            },
        };
        return (StatusCode::SERVICE_UNAVAILABLE, Json(resp)).into_response();
    }

    // 2. 本地 API Key 鉴权校验（如果配置了）
    let (parts, body) = req.into_parts();
    if !cfg.local_api_key.is_empty() {
        let auth_header = parts.headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok());
        let expected = format!("Bearer {}", cfg.local_api_key);
        if auth_header != Some(&expected) {
            return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
        }
    }

    // 3. 构造上游目标 URL
    let upstream_base = cfg.upstream_base_url.trim_end_matches('/');
    let path_and_query = parts.uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("");
    let target_url = format!("{}{}", upstream_base, path_and_query);

    // 4. 读取客户端 Request Body（便于失败重试时重放）
    let body_bytes = match axum::body::to_bytes(body, 100 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("读取请求体失败: {}", e)).into_response();
        }
    };

    // 5. 准备转发请求头
    let mut fwd_headers = HeaderMap::new();
    for (k, v) in parts.headers.iter() {
        // 过滤 Hop-by-hop 请求头
        let name = k.as_str().to_lowercase();
        if name == "host"
            || name == "connection"
            || name == "keep-alive"
            || name == "transfer-encoding"
            || name == "upgrade"
        {
            continue;
        }
        fwd_headers.insert(k.clone(), v.clone());
    }

    // 注入上游 API Key
    if !cfg.upstream_api_key.is_empty() {
        if let Ok(auth_val) = HeaderValue::from_str(&format!("Bearer {}", cfg.upstream_api_key)) {
            fwd_headers.insert(header::AUTHORIZATION, auth_val);
        }
    }

    // 6. 原子计数与退出守卫
    state.active_requests.fetch_add(1, Ordering::SeqCst);
    let _guard = RequestGuard(state.active_requests.clone());

    // 7. 重试策略准备
    let retry_enabled = cfg.retry_enabled;
    let retry_count: usize = cfg.retry_count.parse().unwrap_or(3);
    let retry_interval: u64 = cfg.retry_interval_seconds.parse().unwrap_or(1);
    let retry_status_codes = cfg.retry_status_codes.clone();

    let method = parts.method;
    let mut attempts = 0;

    loop {
        attempts += 1;

        let mut req_builder = state.http_client.request(
            reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap_or(reqwest::Method::GET),
            &target_url,
        );

        for (k, v) in fwd_headers.iter() {
            if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_ref()) {
                if let Ok(val) = reqwest::header::HeaderValue::from_bytes(v.as_bytes()) {
                    req_builder = req_builder.header(name, val);
                }
            }
        }

        if !body_bytes.is_empty() {
            req_builder = req_builder.body(body_bytes.clone());
        }

        match req_builder.send().await {
            Ok(upstream_resp) => {
                let status = upstream_resp.status();
                let status_u16 = status.as_u16();

                // 检查是否需要触发重试
                if retry_enabled
                    && attempts <= retry_count
                    && should_retry_status(status_u16, &retry_status_codes)
                {
                    warn!(
                        "上游返回状态码 {}，命中重试规则，准备第 {} 次重试...",
                        status_u16, attempts
                    );
                    tokio::time::sleep(Duration::from_secs(retry_interval)).await;
                    continue;
                }

                // 成功或无需重试，构建响应
                let mut response_builder = Response::builder().status(status_u16);

                for (k, v) in upstream_resp.headers().iter() {
                    let name = k.as_str().to_lowercase();
                    if name == "connection" || name == "transfer-encoding" {
                        continue;
                    }
                    if let Ok(axum_k) = HeaderName::from_bytes(k.as_ref()) {
                        if let Ok(axum_v) = HeaderValue::from_bytes(v.as_bytes()) {
                            response_builder = response_builder.header(axum_k, axum_v);
                        }
                    }
                }

                // 流式透传上游响应（完全支持 SSE / 打字机输出）
                let stream = upstream_resp.bytes_stream();
                let body = Body::from_stream(stream);
                return response_builder.body(body).unwrap_or_else(|_| {
                    (StatusCode::INTERNAL_SERVER_ERROR, "构建响应体失败").into_response()
                });
            }
            Err(err) => {
                if retry_enabled && attempts <= retry_count {
                    warn!("请求上游发生网络错误: {}，准备第 {} 次重试...", err, attempts);
                    tokio::time::sleep(Duration::from_secs(retry_interval)).await;
                    continue;
                }

                error!("请求上游最终失败: {}", err);
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({
                        "error": true,
                        "message": format!("上游网关连接失败: {}", err)
                    })),
                )
                    .into_response();
            }
        }
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/proxy", get(get_proxy_status))
        .route("/api/proxy/start", post(start_proxy))
        .route("/api/proxy/stop", post(stop_proxy))
        .route("/v1", axum::routing::any(forward_handler))
        .route("/v1/{*path}", axum::routing::any(forward_handler))
}
