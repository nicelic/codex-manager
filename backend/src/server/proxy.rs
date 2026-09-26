use axum::{
    body::Body,
    extract::{
        ws::{Message as AxumWsMessage, WebSocket as AxumWebSocket, WebSocketUpgrade},
        FromRequestParts, Request, State,
    },
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;
use tracing::{error, info, warn};
use url::Url;

use crate::common::config::is_upstream_placeholder;
use crate::common::windows::get_user_profile_dir;
use crate::state::AppState;
use crate::tools::llmtrim::service::LlmtrimService;

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

pub fn get_proxy_status_data(state: &AppState, running: bool, listen_address: &str) -> ProxyStatusResponse {
    let local_http1 = state.active_requests.load(Ordering::Relaxed);
    let local_ws = state.active_ws_sessions.load(Ordering::Relaxed);
    let upstream_h2 = if running { state.upstream_h2_active.load(Ordering::Relaxed) } else { 0 };
    let upstream_ws = state.upstream_ws_carriers.load(Ordering::Relaxed);
    let upstream_streams = (if local_http1 > 0 { local_http1 as usize } else { 0 }) + upstream_ws;

    let proxy_state = if let Ok(s) = state.proxy_state.try_read() {
        s.clone()
    } else if running {
        "running".to_string()
    } else {
        "stopped".to_string()
    };

    let message = if let Ok(m) = state.proxy_message.try_read() {
        m.clone()
    } else if running {
        "HTTP/WS 网关已启动，上游 H2/H3 扩展 CONNECT 已就绪。".to_string()
    } else {
        "代理转发已停止，请在网页中点击“启动代理”。".to_string()
    };

    ProxyStatusResponse {
        running,
        state: proxy_state,
        process_id: std::process::id(),
        listen_address: listen_address.to_string(),
        message,
        connections: ConnectionStatus {
            local_http1,
            local_ws,
            upstream_h2,
            upstream_h2_ws: upstream_ws > 0,
            upstream_h3: 0,
            upstream_h3_ws: false,
            upstream_ws,
            upstream_streams,
        },
    }
}

pub async fn get_proxy_status(State(state): State<AppState>) -> Json<ProxyStatusResponse> {
    let running = *state.proxy_running.read().await;
    let listen_addr = state.proxy_address.read().await.clone();
    Json(get_proxy_status_data(&state, running, &listen_addr))
}

pub async fn start_proxy(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.config.read().await.clone();
    let listen_addr = cfg.listen_address.trim().to_string();
    let web_port = *state.web_port.read().await;

    // 校验监听地址格式与端口合法性
    let socket_addr = match crate::common::config::validate_listen_address(&listen_addr) {
        Ok(addr) => addr,
        Err(err_msg) => {
            let msg = format!("启动代理失败: {}", err_msg);
            {
                let mut running = state.proxy_running.write().await;
                *running = false;
                let mut st = state.proxy_state.write().await;
                *st = "stopped".to_string();
                let mut pmsg = state.proxy_message.write().await;
                *pmsg = msg;
                state.upstream_h2_active.store(0, Ordering::SeqCst);
            }
            let data = get_proxy_status_data(&state, false, &listen_addr);
            return (StatusCode::BAD_REQUEST, Json(data));
        }
    };

    {
        let mut st = state.proxy_state.write().await;
        *st = "connecting".to_string();
        let mut msg = state.proxy_message.write().await;
        *msg = "正在建立上游 H2/H3 TLS 连接，并准备扩展 CONNECT。".to_string();
    }

    // 预热上游探测（对齐 Go 版本的 prewarmUpstreamWithRetry）
    let probe_url = cfg.upstream_base_url.trim().to_string();
    let client = state.http_client.clone();
    let prewarm_res = client
        .get(&probe_url)
        .timeout(Duration::from_secs(5))
        .send()
        .await;

    let prewarm_ok = match prewarm_res {
        Ok(resp) => {
            info!("上游预热握手成功: {} (状态: {})", probe_url, resp.status());
            true
        }
        Err(e) => {
            // 如果是 HTTP 状态码（如 401/403/404），说明网络与 TLS 握手已完成
            if e.is_status() {
                info!("上游预热握手连通: {} (状态: {:?})", probe_url, e.status());
                true
            } else {
                warn!("上游预热连接失败: {} ({})", probe_url, e);
                false
            }
        }
    };

    if !prewarm_ok {
        let err_msg = "上游不可用: 无法连接到上游 Base URL，请检查网络或配置".to_string();
        {
            let mut running = state.proxy_running.write().await;
            *running = false;
            let mut st = state.proxy_state.write().await;
            *st = "unavailable".to_string();
            let mut msg = state.proxy_message.write().await;
            *msg = err_msg.clone();
            state.upstream_h2_active.store(0, Ordering::SeqCst);
        }
        let data = get_proxy_status_data(&state, false, &listen_addr);
        return (StatusCode::BAD_GATEWAY, Json(data));
    }

    // 如果代理端口与当前网页主服务端口不同，启动专有代理监听器
    if socket_addr.port() != web_port {
        let mut cancel_guard = state.proxy_dedicated_cancel.write().await;
        if let Some(old_tx) = cancel_guard.take() {
            let _ = old_tx.send(());
        }

        match TcpListener::bind(socket_addr).await {
            Ok(listener) => {
                let (tx, rx) = oneshot::channel();
                *cancel_guard = Some(tx);
                let proxy_state_clone = state.clone();
                tokio::spawn(async move {
                    let app = Router::new()
                        .route("/v1", axum::routing::any(forward_handler))
                        .route("/v1/{*path}", axum::routing::any(forward_handler))
                        .route("/healthz", get(crate::router::health))
                        .with_state(proxy_state_clone);

                    let _ = axum::serve(listener, app)
                        .with_graceful_shutdown(async move {
                            let _ = rx.await;
                        })
                        .await;
                });
                info!("已成功在专有端口 {} 启动 /v1 代理监听器", listen_addr);
            }
            Err(e) => {
                let err_msg = format!("启动代理失败: 监听地址 {} 绑定失败（端口已被占用或无权限: {}）", listen_addr, e);
                error!("{}", err_msg);
                {
                    let mut running = state.proxy_running.write().await;
                    *running = false;
                    let mut st = state.proxy_state.write().await;
                    *st = "stopped".to_string();
                    let mut msg = state.proxy_message.write().await;
                    *msg = err_msg.clone();
                    state.upstream_h2_active.store(0, Ordering::SeqCst);
                }
                let data = get_proxy_status_data(&state, false, &listen_addr);
                return (StatusCode::CONFLICT, Json(data));
            }
        }
    } else {
        // 与网页服务端口一致，复用主服务，清理已有专有监听器
        let mut cancel_guard = state.proxy_dedicated_cancel.write().await;
        if let Some(old_tx) = cancel_guard.take() {
            let _ = old_tx.send(());
        }
        info!("代理端口与网页端口一致 ({})，直接复用网页主监听器承载 /v1 反代", web_port);
    }

    // 成功标记运行中
    {
        let mut running = state.proxy_running.write().await;
        *running = true;
        let mut st = state.proxy_state.write().await;
        *st = "running".to_string();
        let mut msg = state.proxy_message.write().await;
        *msg = if socket_addr.port() != web_port {
            format!("HTTP/WS 网关已在专有端口 {} 启动，上游 H2/H3 扩展 CONNECT 已就绪。", listen_addr)
        } else {
            "HTTP/WS 网关已启动，上游 H2/H3 扩展 CONNECT 已就绪。".to_string()
        };
        let mut paddr = state.proxy_address.write().await;
        *paddr = listen_addr.clone();
        state.upstream_h2_active.store(1, Ordering::SeqCst);
    }

    info!("HTTP/WS 反向代理已启动并就绪: {}", listen_addr);
    let data = get_proxy_status_data(&state, true, &listen_addr);
    (StatusCode::OK, Json(data))
}

pub async fn stop_proxy(State(state): State<AppState>) -> impl IntoResponse {
    // 关闭专有代理监听器（如果存在）
    {
        let mut cancel_guard = state.proxy_dedicated_cancel.write().await;
        if let Some(tx) = cancel_guard.take() {
            let _ = tx.send(());
        }
    }

    let listen_addr = state.proxy_address.read().await.clone();
    {
        let mut running = state.proxy_running.write().await;
        *running = false;
        let mut st = state.proxy_state.write().await;
        *st = "stopped".to_string();
        let mut msg = state.proxy_message.write().await;
        *msg = "代理已彻底停止；code-Manager 管理页面仍保持运行。".to_string();
        state.upstream_h2_active.store(0, Ordering::SeqCst);
    }

    info!("HTTP/WS 反向代理已停止");
    let data = get_proxy_status_data(&state, false, &listen_addr);
    (StatusCode::OK, Json(data))
}

/// 对齐 Go 的 joinUpstreamURL：保留 upstream_base_url 中用户填写的完整路径，
/// 严格剥离本地客户端传入的 /v1/ 或 /v1 前缀，将剩余子路径追加到上游路径后，并安全合并 query。
pub fn join_upstream_url(base: &str, path: &str, raw_query: Option<&str>) -> Result<String, String> {
    let mut u = Url::parse(base).map_err(|e| format!("无法解析上游 Base URL: {}", e))?;

    let suffix = if let Some(stripped) = path.strip_prefix("/v1/") {
        stripped
    } else if path == "/v1" {
        ""
    } else if let Some(stripped) = path.strip_prefix("/v1") {
        stripped.trim_start_matches('/')
    } else {
        path.trim_start_matches('/')
    };

    if !suffix.is_empty() {
        let mut current_path = u.path().trim_end_matches('/').to_string();
        if current_path.is_empty() {
            current_path = format!("/{}", suffix);
        } else {
            current_path = format!("{}/{}", current_path, suffix);
        }
        u.set_path(&current_path);
    }

    if let Some(rq) = raw_query {
        if !rq.is_empty() {
            if let Some(existing) = u.query() {
                if !existing.is_empty() {
                    u.set_query(Some(&format!("{}&{}", existing, rq)));
                } else {
                    u.set_query(Some(rq));
                }
            } else {
                u.set_query(Some(rq));
            }
        }
    }

    Ok(u.to_string())
}

/// 解析状态码范围，例如 "500-503,525-599" 或 "502"（对齐规范化规则）
pub fn should_retry_status(code: u16, rule_str: &str) -> bool {
    let (_, ranges) = crate::common::config::normalize_retry_status_codes(rule_str);
    ranges.iter().any(|&(s, e)| code >= s && code <= e)
}

struct RequestGuard(std::sync::Arc<std::sync::atomic::AtomicI64>);
impl Drop for RequestGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// 构建 LLMTrim 专属代理客户端（加载 %USERPROFILE%\.llmtrim\ca.pem 根证书）
fn build_llmtrim_proxy_client() -> Option<reqwest::Client> {
    let ca_path = get_user_profile_dir().join(".llmtrim").join("ca.pem");
    if !ca_path.exists() {
        warn!("LLMTrim CA 证书不存在于 {:?}，使用通用代理配置", ca_path);
    }

    let mut builder = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all("http://127.0.0.1:43117").ok()?)
        .pool_idle_timeout(Duration::from_secs(300))
        .pool_max_idle_per_host(16)
        .danger_accept_invalid_certs(false);

    if let Ok(pem_data) = std::fs::read(&ca_path) {
        if let Ok(cert) = reqwest::Certificate::from_pem(&pem_data) {
            builder = builder.add_root_certificate(cert);
            info!("已成功加载 LLMTrim 根证书并建立 43117 代理隧道");
        }
    }

    builder.build().ok()
}

/// 处理 WebSocket 双向原始帧代理（支持直连及 HTTP CONNECT 代理隧道）
async fn handle_v1_websocket_tunnel(
    client_ws: AxumWebSocket,
    state: AppState,
    target_url: String,
    upstream_api_key: String,
    outbound_proxy: Option<String>,
) {
    let ws_target = if target_url.starts_with("https://") {
        target_url.replacen("https://", "wss://", 1)
    } else if target_url.starts_with("http://") {
        target_url.replacen("http://", "ws://", 1)
    } else {
        target_url
    };

    let mut req = match ws_target.clone().into_client_request() {
        Ok(r) => r,
        Err(e) => {
            error!("构造上游 WebSocket 请求失败: {}", e);
            return;
        }
    };

    if !is_upstream_placeholder(&upstream_api_key) {
        if let Ok(auth_val) = HeaderValue::from_str(&format!("Bearer {}", upstream_api_key.trim())) {
            req.headers_mut().insert(header::AUTHORIZATION, auth_val);
        }
    }

    info!("正在与上游建立 WebSocket 隧道: {} (出站代理: {:?})", ws_target, outbound_proxy);

    let parsed_target = match Url::parse(&ws_target) {
        Ok(u) => u,
        Err(e) => {
            error!("解析 WebSocket 目标 URL 失败: {}", e);
            return;
        }
    };

    let is_secure = parsed_target.scheme() == "wss";
    let target_host = parsed_target.host_str().unwrap_or("localhost").to_string();
    let target_port = parsed_target.port_or_known_default().unwrap_or(if is_secure { 443 } else { 80 });

    let upstream_conn_res = if let Some(proxy) = outbound_proxy.filter(|p| !p.trim().is_empty()) {
        match Url::parse(&proxy) {
            Ok(proxy_parsed) => {
                let proxy_host = proxy_parsed.host_str().unwrap_or("127.0.0.1").to_string();
                let proxy_port = proxy_parsed.port_or_known_default().unwrap_or(8080);
                let proxy_addr = format!("{}:{}", proxy_host, proxy_port);

                async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut tcp = tokio::net::TcpStream::connect(&proxy_addr)
                        .await
                        .map_err(|e| format!("连接出站代理 {} 失败: {}", proxy_addr, e))?;

                    let connect_req = format!(
                        "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\nProxy-Connection: Keep-Alive\r\n\r\n",
                        target_host, target_port, target_host, target_port
                    );
                    tcp.write_all(connect_req.as_bytes())
                        .await
                        .map_err(|e| format!("向出站代理发送 CONNECT 指令失败: {}", e))?;

                    let mut resp_buf = [0u8; 1024];
                    let n = tcp
                        .read(&mut resp_buf)
                        .await
                        .map_err(|e| format!("读取出站代理 CONNECT 响应失败: {}", e))?;
                    let resp_str = String::from_utf8_lossy(&resp_buf[..n]);
                    if !resp_str.starts_with("HTTP/1.1 200") && !resp_str.starts_with("HTTP/1.0 200") {
                        return Err(format!("出站代理 CONNECT 握手被拒绝: {}", resp_str.lines().next().unwrap_or("")));
                    }

                    if is_secure {
                        tokio_tungstenite::client_async_tls(req, tcp)
                            .await
                            .map_err(|e| format!("经代理 TLS WebSocket 握手失败: {}", e))
                    } else {
                        tokio_tungstenite::client_async(req, tokio_tungstenite::MaybeTlsStream::Plain(tcp))
                            .await
                            .map_err(|e| format!("经代理 WebSocket 握手失败: {}", e))
                    }
                }
                .await
            }
            Err(e) => {
                warn!("解析出站代理 URL ({}) 失败: {}，回退到直连", proxy, e);
                tokio_tungstenite::connect_async(req)
                    .await
                    .map_err(|e| format!("直连上游 WebSocket 失败: {}", e))
            }
        }
    } else {
        tokio_tungstenite::connect_async(req)
            .await
            .map_err(|e| format!("直连上游 WebSocket 失败: {}", e))
    };

    let (upstream_ws, _) = match upstream_conn_res {
        Ok(res) => res,
        Err(err_msg) => {
            error!("{}", err_msg);
            return;
        }
    };

    state.active_ws_sessions.fetch_add(1, Ordering::SeqCst);
    state.upstream_ws_carriers.fetch_add(1, Ordering::SeqCst);

    let (mut client_sink, mut client_stream) = client_ws.split();
    let (mut upstream_sink, mut upstream_stream) = upstream_ws.split();

    // 双向全双工数据泵送
    let client_to_upstream = async {
        while let Some(Ok(msg)) = client_stream.next().await {
            let tung_msg = match msg {
                AxumWsMessage::Text(t) => TungsteniteMessage::Text(t.as_str().into()),
                AxumWsMessage::Binary(b) => TungsteniteMessage::Binary(b.into()),
                AxumWsMessage::Ping(p) => TungsteniteMessage::Ping(p.into()),
                AxumWsMessage::Pong(p) => TungsteniteMessage::Pong(p.into()),
                AxumWsMessage::Close(c) => {
                    let frame = c.map(|f| tokio_tungstenite::tungstenite::protocol::CloseFrame {
                        code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from(f.code),
                        reason: f.reason.as_str().into(),
                    });
                    let _ = upstream_sink.send(TungsteniteMessage::Close(frame)).await;
                    break;
                }
            };
            if upstream_sink.send(tung_msg).await.is_err() {
                break;
            }
        }
    };

    let upstream_to_client = async {
        while let Some(Ok(msg)) = upstream_stream.next().await {
            let axum_msg = match msg {
                TungsteniteMessage::Text(t) => AxumWsMessage::Text(t.as_str().into()),
                TungsteniteMessage::Binary(b) => AxumWsMessage::Binary(b.into()),
                TungsteniteMessage::Ping(p) => AxumWsMessage::Ping(p.into()),
                TungsteniteMessage::Pong(p) => AxumWsMessage::Pong(p.into()),
                TungsteniteMessage::Close(c) => {
                    let frame = c.map(|f| axum::extract::ws::CloseFrame {
                        code: f.code.into(),
                        reason: f.reason.as_str().into(),
                    });
                    let _ = client_sink.send(AxumWsMessage::Close(frame)).await;
                    break;
                }
                TungsteniteMessage::Frame(_) => continue,
            };
            if client_sink.send(axum_msg).await.is_err() {
                break;
            }
        }
    };

    tokio::select! {
        _ = client_to_upstream => {},
        _ = upstream_to_client => {},
    }

    state.active_ws_sessions.fetch_sub(1, Ordering::SeqCst);
    state.upstream_ws_carriers.fetch_sub(1, Ordering::SeqCst);
    info!("WebSocket 代理隧道已断开");
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
        let resp = get_proxy_status_data(&state, false, &cfg.listen_address);
        return (StatusCode::SERVICE_UNAVAILABLE, Json(resp)).into_response();
    }

    let (mut parts, body) = req.into_parts();

    // 2. 本地 API Key 鉴权校验（如果配置了 local_api_key）
    if !cfg.local_api_key.trim().is_empty() {
        let auth_bearer = parts.headers.get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer ").or_else(|| h.strip_prefix("bearer ")))
            .map(|s| s.trim());
        let auth_custom = parts.headers.get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim());

        let matched = match (auth_bearer, auth_custom) {
            (Some(b), _) if b == cfg.local_api_key.trim() => true,
            (_, Some(c)) if c == cfg.local_api_key.trim() => true,
            _ => false,
        };

        if !matched {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": { "message": "Incorrect or missing local API key", "type": "authentication_error" } })),
            ).into_response();
        }
    }

    // 3. 构造上游目标 URL（剥离 /v1 前缀，杜绝 /v1/v1 404）
    let path = parts.uri.path().to_string();
    let query = parts.uri.query().map(|s| s.to_string());
    let target_url = match join_upstream_url(&cfg.upstream_base_url, &path, query.as_deref()) {
        Ok(u) => u,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!("构建上游目标 URL 失败: {}", e)).into_response();
        }
    };

    // 4. WebSocket Upgrade 拦截与处理
    let is_ws = parts
        .headers
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);

    let llmtrim_running = LlmtrimService::is_port_open();
    let effective_ws_proxy = if llmtrim_running {
        Some("http://127.0.0.1:43117".to_string())
    } else if !cfg.outbound_proxy.trim().is_empty() {
        Some(cfg.outbound_proxy.trim().to_string())
    } else {
        None
    };

    if is_ws && cfg.upstream_websocket_enabled {
        if let Ok(ws) = WebSocketUpgrade::from_request_parts(&mut parts, &state).await {
            let ws_target = target_url.clone();
            let key = cfg.upstream_api_key.clone();
            let ws_state = state.clone();
            let ws_proxy = effective_ws_proxy.clone();
            return ws
                .on_upgrade(move |socket| handle_v1_websocket_tunnel(socket, ws_state, ws_target, key, ws_proxy))
                .into_response();
        }
    }

    // 5. 读取客户端 Request Body（最大 64MB 用于重试缓冲）
    let body_bytes = match axum::body::to_bytes(body, 64 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("读取请求体失败（单请求上限 64MB）: {}", e)).into_response();
        }
    };

    // 6. 准备转发请求头
    let mut fwd_headers = HeaderMap::new();
    for (k, v) in parts.headers.iter() {
        let name = k.as_str().to_lowercase();
        if name == "host"
            || name == "connection"
            || name == "keep-alive"
            || name == "transfer-encoding"
            || name == "upgrade"
            || name == "proxy-connection"
        {
            continue;
        }
        fwd_headers.insert(k.clone(), v.clone());
    }

    // 注入或透传 API Key
    if !is_upstream_placeholder(&cfg.upstream_api_key) {
        if let Ok(auth_val) = HeaderValue::from_str(&format!("Bearer {}", cfg.upstream_api_key.trim())) {
            fwd_headers.insert(header::AUTHORIZATION, auth_val);
        }
    }

    // 7. 确定 HTTP 转发客户端（优先 LLMTrim 43117 代理隧道）
    let client = if llmtrim_running {
        let mut cached = state.llmtrim_client.write().await;
        if cached.is_none() {
            *cached = build_llmtrim_proxy_client();
        }
        cached.clone().unwrap_or_else(|| state.http_client.clone())
    } else {
        let mut cached = state.llmtrim_client.write().await;
        *cached = None;
        if !cfg.outbound_proxy.trim().is_empty() {
            if let Ok(proxy) = reqwest::Proxy::all(cfg.outbound_proxy.trim()) {
                reqwest::Client::builder()
                    .proxy(proxy)
                    .build()
                    .unwrap_or_else(|_| state.http_client.clone())
            } else {
                state.http_client.clone()
            }
        } else {
            state.http_client.clone()
        }
    };

    // 8. 原子计数与退出守卫
    state.active_requests.fetch_add(1, Ordering::SeqCst);
    let _guard = RequestGuard(state.active_requests.clone());

    // 9. 重试策略准备与请求发送
    let retry_enabled = cfg.retry_enabled;
    let retry_count: usize = cfg.retry_count.parse().unwrap_or(5);
    let retry_interval: u64 = cfg.retry_interval_seconds.parse().unwrap_or(1);
    let retry_status_codes = cfg.retry_status_codes.clone();
    let (_, parsed_retry_ranges) = crate::common::config::normalize_retry_status_codes(&retry_status_codes);

    let method = parts.method;
    let mut attempts = 0;
    let start_time = Instant::now();

    loop {
        attempts += 1;

        let mut req_builder = client.request(
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
                    && parsed_retry_ranges.iter().any(|&(s, e)| status_u16 >= s && status_u16 <= e)
                {
                    let retry_after_secs = upstream_resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .map(|s| s.clamp(1, 60));

                    let base_delay = retry_after_secs.unwrap_or_else(|| retry_interval.max(1));
                    let jitter_ms = (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_millis() % 100 + 50)
                        .unwrap_or(50)) as u64;

                    let total_delay = Duration::from_millis(base_delay * 1000 + jitter_ms);
                    warn!(
                        "上游返回状态码 {}，命中重试规则，等待 {:.2}s 后进行第 {} 次重试...",
                        status_u16,
                        total_delay.as_secs_f64(),
                        attempts
                    );
                    tokio::time::sleep(total_delay).await;
                    continue;
                }

                // 记录完成耗时（对齐 Go requestLogger）
                info!("{} {} completed in {:?}", method, path, start_time.elapsed());

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
                    let base_delay = retry_interval.max(1);
                    let jitter_ms = (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_millis() % 100 + 50)
                        .unwrap_or(50)) as u64;
                    let total_delay = Duration::from_millis(base_delay * 1000 + jitter_ms);

                    warn!(
                        "请求上游发生网络错误: {}，等待 {:.2}s 进行第 {} 次重试...",
                        err,
                        total_delay.as_secs_f64(),
                        attempts
                    );
                    tokio::time::sleep(total_delay).await;
                    continue;
                }

                error!("{} {} 请求上游最终失败: {} (耗时: {:?})", method, path, err, start_time.elapsed());
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({
                        "error": {
                            "message": format!("上游网关连接失败: {}", err),
                            "type": "bad_gateway"
                        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::AppConfig;

    #[test]
    fn test_join_upstream_url_variations() {
        let base = "https://api.example.com/v1";
        assert_eq!(
            join_upstream_url(base, "/v1/chat/completions", None).unwrap(),
            "https://api.example.com/v1/chat/completions"
        );
        assert_eq!(
            join_upstream_url(base, "/v1/models", None).unwrap(),
            "https://api.example.com/v1/models"
        );
        assert_eq!(
            join_upstream_url(base, "/v1", None).unwrap(),
            "https://api.example.com/v1"
        );
        assert_eq!(
            join_upstream_url("https://api.example.com/v1/", "/v1/chat/completions", Some("stream=true")).unwrap(),
            "https://api.example.com/v1/chat/completions?stream=true"
        );
        assert_eq!(
            join_upstream_url("https://api.example.com/v1?api-key=test", "/v1/models", Some("foo=bar")).unwrap(),
            "https://api.example.com/v1/models?api-key=test&foo=bar"
        );
    }

    #[test]
    fn test_retry_status_matcher() {
        let rule = "100-199,300-399,401-407,409-499,500-503,505-523,525-599";
        assert!(should_retry_status(500, rule));
        assert!(should_retry_status(502, rule));
        assert!(should_retry_status(503, rule));
        assert!(!should_retry_status(504, rule));
        assert!(should_retry_status(525, rule));
        assert!(should_retry_status(429, rule));
        assert!(!should_retry_status(200, rule));
        assert!(!should_retry_status(400, rule));
        assert!(!should_retry_status(408, rule));
    }

    #[test]
    fn test_connection_status_snapshot() {
        let config = AppConfig::default();
        let state = AppState::new(config, std::path::PathBuf::from("config.json"));

        // 初始停止状态
        let status = get_proxy_status_data(&state, false, "127.0.0.1:7788");
        assert!(!status.running);
        assert_eq!(status.connections.local_http1, 0);
        assert_eq!(status.connections.local_ws, 0);
        assert_eq!(status.connections.upstream_h2, 0);
        assert!(!status.connections.upstream_h2_ws);
        assert_eq!(status.connections.upstream_streams, 0);

        // 模拟运行及活动连接计数
        state.active_requests.store(4, Ordering::SeqCst);
        state.active_ws_sessions.store(2, Ordering::SeqCst);
        state.upstream_h2_active.store(1, Ordering::SeqCst);
        state.upstream_ws_carriers.store(1, Ordering::SeqCst);

        let running_status = get_proxy_status_data(&state, true, "127.0.0.1:7788");
        assert!(running_status.running);
        assert_eq!(running_status.connections.local_http1, 4);
        assert_eq!(running_status.connections.local_ws, 2);
        assert_eq!(running_status.connections.upstream_h2, 1);
        assert!(running_status.connections.upstream_h2_ws);
        assert_eq!(running_status.connections.upstream_ws, 1);
        assert_eq!(running_status.connections.upstream_streams, 5);
    }
}
