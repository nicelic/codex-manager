use axum::{
    body::Body,
    extract::{
        ws::{Message as AxumWsMessage, WebSocket as AxumWebSocket, WebSocketUpgrade},
        FromRequestParts, Request, State,
    },
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
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
use crate::server::llmtrim_transport::LlmtrimTransport;
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

    // 预热必须使用与正式请求相同的上游路由。
    let probe_url = cfg.upstream_base_url.trim().to_string();
    let (llmtrim_running, mut route_changed) = refresh_llmtrim_route(&state, &cfg.llmtrim_path);
    let prewarm_result = if llmtrim_running {
        if !llmtrim_route_available(&state) {
            Err("llmtrim 未就绪，已禁止回退到默认路由".to_string())
        } else {
            match select_llmtrim_http1_client(&state).await {
                Ok(client) => {
                    let empty_headers = HeaderMap::new();
                    tokio::select! {
                    biased;
                    _ = route_changed.changed() => Err("代理路由已切换，请重新启动代理".to_string()),
                    result = tokio::time::timeout(
                        Duration::from_secs(5),
                        client.request(&Method::GET, &probe_url, &empty_headers, Bytes::new()),
                    ) => {
                        match result {
                            Ok(Ok(response)) => {
                                info!("上游预热经 LLMTrim 成功: {} (状态: {})", probe_url, response.status);
                                Ok(())
                            }
                            Ok(Err(err)) => Err(err),
                            Err(_) => Err("经 LLMTrim 的上游预热超时".to_string()),
                        }
                    }
                    }
                }
                Err(err) => Err(err),
            }
        }
    } else {
        match select_upstream_client(&state, false, &cfg.outbound_proxy).await {
            Ok(client) => tokio::select! {
                biased;
                _ = route_changed.changed() => Err("代理路由已切换，请重新启动代理".to_string()),
                result = client.get(&probe_url).timeout(Duration::from_secs(5)).send() => {
                    result.map(|resp| {
                        info!("上游预热握手成功: {} (状态: {})", probe_url, resp.status());
                    }).map_err(|e| e.to_string())
                }
            },
            Err(err) => Err(err),
        }
    };

    if let Err(reason) = prewarm_result {
        warn!("上游预热连接失败: {} ({})", probe_url, reason);
        let err_msg = format!("上游不可用: {}", reason);
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

    start_llmtrim_route_monitor(&state);
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

struct SessionGuard(std::sync::Arc<std::sync::atomic::AtomicUsize>);
impl SessionGuard {
    fn new(counter: &std::sync::Arc<std::sync::atomic::AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(counter.clone())
    }
}
impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

const LLMTRIM_PROXY_URL: &str = "http://127.0.0.1:43117";

fn llmtrim_ca_path() -> std::path::PathBuf {
    LlmtrimService::get_runtime_state_dir().join("ca.pem")
}

fn read_llmtrim_ca(path: &std::path::Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("读取 llmtrim CA 证书 {} 失败: {}", path.display(), e))
}

fn build_llmtrim_proxy_client_configured(
    proxy_url: &str,
    ca_path: &std::path::Path,
    http1_only: bool,
) -> Result<reqwest::Client, String> {
    let pem = read_llmtrim_ca(ca_path)?;
    let cert = reqwest::Certificate::from_pem(&pem)
        .map_err(|e| format!("解析 llmtrim CA 证书 {} 失败: {}", ca_path.display(), e))?;
    let proxy = reqwest::Proxy::all(proxy_url)
        .map_err(|e| format!("创建 llmtrim 代理失败: {}", e))?;
    let mut builder = reqwest::Client::builder()
        .proxy(proxy)
        .add_root_certificate(cert)
        .pool_idle_timeout(Duration::from_secs(300))
        .pool_max_idle_per_host(16);
    if http1_only {
        builder = builder.http1_only();
    }
    builder.build().map_err(|e| format!("创建 llmtrim HTTP 客户端失败: {}", e))
}

fn build_llmtrim_proxy_client_with(proxy_url: &str, ca_path: &std::path::Path) -> Result<reqwest::Client, String> {
    build_llmtrim_proxy_client_configured(proxy_url, ca_path, false)
}

async fn select_llmtrim_http1_client(state: &AppState) -> Result<LlmtrimTransport, String> {
    let mut cached = state.llmtrim_http1_client.write().await;
    let generation = *state.proxy_route_changed.borrow();
    if cached.as_ref().map(|(cached_generation, _)| *cached_generation) != Some(generation) {
        *cached = None;
        *cached = Some((generation, LlmtrimTransport::plain()));
    }
    Ok(cached.as_ref().expect("llmtrim HTTP/1.1 client initialized").1.clone())
}

async fn select_upstream_client(state: &AppState, llmtrim_running: bool, outbound_proxy: &str) -> Result<reqwest::Client, String> {
    select_upstream_client_with_ca(state, llmtrim_running, outbound_proxy, &llmtrim_ca_path()).await
}

async fn select_upstream_client_with_ca(
    state: &AppState,
    llmtrim_running: bool,
    outbound_proxy: &str,
    ca_path: &std::path::Path,
) -> Result<reqwest::Client, String> {
    if llmtrim_running {
        let mut cached = state.llmtrim_client.write().await;
        let generation = *state.proxy_route_changed.borrow();
        if cached.as_ref().map(|(cached_generation, _)| *cached_generation) != Some(generation) {
            *cached = None;
            *cached = Some((generation, build_llmtrim_proxy_client_with(LLMTRIM_PROXY_URL, ca_path)?));
        }
        return Ok(cached.as_ref().expect("llmtrim client initialized").1.clone());
    }

    *state.llmtrim_client.write().await = None;
    if !outbound_proxy.trim().is_empty() {
        if let Ok(proxy) = reqwest::Proxy::all(outbound_proxy.trim()) {
            return Ok(reqwest::Client::builder()
                .proxy(proxy)
                .build()
                .unwrap_or_else(|_| state.http_client.clone()));
        }
    }
    Ok(state.http_client.clone())
}

fn observe_llmtrim_route_with<F, P>(
    state: &AppState,
    is_running: F,
    persist_intent: P,
) -> (bool, tokio::sync::watch::Receiver<u64>)
where
    F: FnOnce() -> bool,
    P: FnOnce() -> Result<(), String>,
{
    let mut route = state.llmtrim_route.lock().unwrap_or_else(|e| e.into_inner());
    let running = is_running();
    let selected = route.selected || running;
    let mut persisted = route.persisted;
    if selected && running && !persisted && !route.transitioning {
        match persist_intent() {
            Ok(()) => persisted = true,
            Err(err) => warn!("无法持久化 llmtrim 路由，保持锁定: {}", err),
        }
    }
    let next = crate::state::LlmtrimRouteState {
        selected,
        available: selected && running && persisted && !route.transitioning,
        persisted,
        transitioning: route.transitioning,
    };
    if *route != next {
        *route = next;
        state.proxy_route_changed.send_modify(|generation| *generation = generation.wrapping_add(1));
    }
    (selected, state.proxy_route_changed.subscribe())
}

#[cfg(test)]
fn observe_llmtrim_route<F>(state: &AppState, is_running: F) -> (bool, tokio::sync::watch::Receiver<u64>)
where
    F: FnOnce() -> bool,
{
    observe_llmtrim_route_with(state, is_running, || Ok(()))
}

pub(crate) fn activate_llmtrim_route(state: &AppState) {
    let mut route = state.llmtrim_route.lock().unwrap_or_else(|e| e.into_inner());
    let next = crate::state::LlmtrimRouteState {
        selected: true,
        available: false,
        persisted: route.persisted,
        transitioning: true,
    };
    if *route != next {
        *route = next;
        state.proxy_route_changed.send_modify(|generation| *generation = generation.wrapping_add(1));
    }
}

pub(crate) fn finish_llmtrim_route_transition(state: &AppState) {
    let mut route = state.llmtrim_route.lock().unwrap_or_else(|e| e.into_inner());
    if route.transitioning {
        route.transitioning = false;
        route.available = false;
        state.proxy_route_changed.send_modify(|generation| *generation = generation.wrapping_add(1));
    }
}

pub(crate) fn deactivate_llmtrim_route(state: &AppState) {
    let mut route = state.llmtrim_route.lock().unwrap_or_else(|e| e.into_inner());
    let next = crate::state::LlmtrimRouteState::default();
    if *route != next {
        *route = next;
        state.proxy_route_changed.send_modify(|generation| *generation = generation.wrapping_add(1));
    }
}

fn llmtrim_route_available(state: &AppState) -> bool {
    state.llmtrim_route.lock().unwrap_or_else(|e| e.into_inner()).available
}

pub(crate) async fn clear_llmtrim_clients(state: &AppState) {
    *state.llmtrim_client.write().await = None;
    *state.llmtrim_http1_client.write().await = None;
}

pub(crate) fn refresh_llmtrim_route(state: &AppState, path: &str) -> (bool, tokio::sync::watch::Receiver<u64>) {
    observe_llmtrim_route_with(state, || LlmtrimService::owns_listener(path), || {
        let executable = LlmtrimService::get_executable_path(path);
        let directory = executable.parent().ok_or_else(|| "无法确定 llmtrim 安装目录".to_string())?;
        crate::tools::llmtrim::ownership::set_managed_tool_desired_running(directory, true)
    })
}

fn start_llmtrim_route_monitor(state: &AppState) {
    if state.proxy_route_monitor_started.swap(true, Ordering::SeqCst) {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        let mut shutdown = state.shutdown_sender.subscribe();
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = shutdown.recv() => break,
                _ = interval.tick() => {
                    if *state.proxy_running.read().await {
                        let path = state.config.read().await.llmtrim_path.clone();
                        refresh_llmtrim_route(&state, &path);
                    }
                }
            }
        }
        state.proxy_route_monitor_started.store(false, Ordering::SeqCst);
    });
}

fn build_llmtrim_ws_connector(path: &std::path::Path) -> Result<tokio_tungstenite::Connector, String> {
    let pem = read_llmtrim_ca(path)?;
    let cert = native_tls::Certificate::from_pem(&pem)
        .map_err(|e| format!("解析 llmtrim CA 证书 {} 失败: {}", path.display(), e))?;
    let connector = native_tls::TlsConnector::builder()
        .add_root_certificate(cert)
        .build()
        .map_err(|e| format!("创建 llmtrim WebSocket TLS 客户端失败: {}", e))?;
    Ok(tokio_tungstenite::Connector::NativeTls(connector))
}

/// 处理 WebSocket 双向原始帧代理（支持直连及 HTTP CONNECT 代理隧道）
async fn handle_v1_websocket_tunnel(
    mut client_ws: AxumWebSocket,
    state: AppState,
    target_url: String,
    upstream_api_key: String,
    outbound_proxy: Option<String>,
    llmtrim_running: bool,
    client_headers: HeaderMap,
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
            let _ = client_ws.send(AxumWsMessage::Close(Some(axum::extract::ws::CloseFrame {
                code: 1011,
                reason: format!("构造上游 WebSocket 请求失败: {}", e).into(),
            }))).await;
            return;
        }
    };

    // 复制客户端自定义头部（过滤 hop-by-hop 标头与 WS 内部标头）
    for (k, v) in client_headers.iter() {
        let name = k.as_str().to_lowercase();
        if name == "host"
            || name == "connection"
            || name == "upgrade"
            || name == "sec-websocket-key"
            || name == "sec-websocket-version"
            || name == "sec-websocket-extensions"
            || name == "sec-websocket-protocol"
        {
            continue;
        }
        if let Ok(hname) = reqwest::header::HeaderName::from_bytes(k.as_ref()) {
            if let Ok(hval) = reqwest::header::HeaderValue::from_bytes(v.as_bytes()) {
                req.headers_mut().insert(hname, hval);
            }
        }
    }

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
            let _ = client_ws.send(AxumWsMessage::Close(Some(axum::extract::ws::CloseFrame {
                code: 1011,
                reason: format!("解析 WebSocket 目标 URL 失败: {}", e).into(),
            }))).await;
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

                    let mut resp_buf = Vec::new();
                    let mut temp_buf = [0u8; 1024];
                    let mut connect_ok = false;
                    while resp_buf.len() < 8192 {
                        let n = tcp.read(&mut temp_buf).await
                            .map_err(|e| format!("读取出站代理 CONNECT 响应失败: {}", e))?;
                        if n == 0 { break; }
                        resp_buf.extend_from_slice(&temp_buf[..n]);
                        if let Some(pos) = resp_buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let header_str = String::from_utf8_lossy(&resp_buf[..pos]);
                            if header_str.starts_with("HTTP/1.1 200") || header_str.starts_with("HTTP/1.0 200") {
                                connect_ok = true;
                            }
                            break;
                        }
                    }

                    if !connect_ok {
                        let resp_str = String::from_utf8_lossy(&resp_buf);
                        return Err(format!("出站代理 CONNECT 握手被拒绝: {}", resp_str.lines().next().unwrap_or("")));
                    }

                    if is_secure {
                        let connector = if llmtrim_running {
                            Some(build_llmtrim_ws_connector(&llmtrim_ca_path())?)
                        } else {
                            None
                        };
                        tokio_tungstenite::client_async_tls_with_config(req, tcp, None, connector)
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
            Err(e) => Err(format!("解析出站代理 URL ({}) 失败: {}", proxy, e)),
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
            let _ = client_ws.send(AxumWsMessage::Close(Some(axum::extract::ws::CloseFrame {
                code: 1011,
                reason: format!("连接上游失败: {}", err_msg).into(),
            }))).await;
            return;
        }
    };

    let _session_guard = SessionGuard::new(&state.active_ws_sessions);
    let _carrier_guard = SessionGuard::new(&state.upstream_ws_carriers);

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

    info!("WebSocket 代理隧道已断开");
}

const MAX_BRIDGE_BUFFER: usize = 8 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct SseMessage {
    event: Option<String>,
    data: String,
}

#[derive(Default)]
struct SseDecoder {
    buffer: Vec<u8>,
    data: Vec<String>,
    event: Option<String>,
    data_bytes: usize,
    seen_line: bool,
}

impl SseDecoder {
    fn dispatch(&mut self, messages: &mut Vec<SseMessage>) {
        if !self.data.is_empty() {
            messages.push(SseMessage {
                event: self.event.take(),
                data: self.data.join("\n"),
            });
        }
        self.data.clear();
        self.event = None;
        self.data_bytes = 0;
    }

    fn process_line(&mut self, bytes: &[u8], messages: &mut Vec<SseMessage>) {
        let decoded = String::from_utf8_lossy(bytes);
        let line = if self.seen_line {
            decoded.as_ref()
        } else {
            self.seen_line = true;
            decoded.strip_prefix('\u{feff}').unwrap_or(decoded.as_ref())
        };
        if line.is_empty() {
            self.dispatch(messages);
        } else if !line.starts_with(':') {
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "data" => {
                    self.data_bytes = self.data_bytes.saturating_add(value.len());
                    self.data.push(value.to_string());
                }
                "event" => self.event = Some(value.to_string()),
                _ => {}
            }
        }
    }

    fn feed(&mut self, chunk: &[u8], finished: bool) -> Result<Vec<SseMessage>, String> {
        if self.buffer.len().saturating_add(self.data_bytes).saturating_add(chunk.len()) > MAX_BRIDGE_BUFFER {
            return Err("上游 SSE 事件超过大小限制".to_string());
        }
        self.buffer.extend_from_slice(chunk);
        let mut messages = Vec::new();
        loop {
            let end = match self.buffer.iter().position(|byte| *byte == b'\n' || *byte == b'\r') {
                Some(end) if self.buffer[end] == b'\r' && end + 1 == self.buffer.len() && !finished => break,
                Some(end) => end,
                None if finished && !self.buffer.is_empty() => self.buffer.len(),
                None => break,
            };
            let mut consumed = end;
            if consumed < self.buffer.len() {
                consumed += 1;
                if self.buffer[end] == b'\r' && self.buffer.get(consumed) == Some(&b'\n') {
                    consumed += 1;
                }
            }
            let mut line: Vec<u8> = self.buffer.drain(..consumed).collect();
            line.truncate(end);
            self.process_line(&line, &mut messages);
        }
        if finished {
            self.dispatch(&mut messages);
        }
        Ok(messages)
    }
}

fn ws_bridge_error(kind: &str, message: impl std::fmt::Display) -> AxumWsMessage {
    AxumWsMessage::Text(json!({
        "type": "error",
        "error": { "type": kind, "message": message.to_string() }
    }).to_string().into())
}

fn normalize_ws_bridge_request(text: &str, responses_protocol: bool) -> Result<(Vec<u8>, bool), String> {
    let mut payload: serde_json::Value = serde_json::from_str(text)
        .map_err(|_| "请求体格式无效，期待 JSON 对象".to_string())?;
    let object = payload.as_object_mut().ok_or_else(|| "请求体必须是 JSON 对象".to_string())?;

    if responses_protocol {
        if let Some(kind) = object.remove("type") {
            if kind.as_str() != Some("response.create") {
                return Err("当前仅支持 response.create 请求事件".to_string());
            }
        }
        if object.contains_key("stream_id") {
            return Err("HTTP 桥接暂不支持 stream_id 并发通道".to_string());
        }
        if let Some(generate) = object.remove("generate") {
            if generate != serde_json::Value::Bool(true) {
                return Err("HTTP 桥接暂不支持 generate=false".to_string());
            }
        }
    }

    let wants_stream = match object.get("stream") {
        Some(serde_json::Value::Bool(value)) => *value,
        Some(_) => return Err("stream 必须是布尔值".to_string()),
        None => {
            object.insert("stream".to_string(), serde_json::Value::Bool(true));
            true
        }
    };
    let body = serde_json::to_vec(&payload).map_err(|e| format!("编码请求体失败: {}", e))?;
    Ok((body, wants_stream))
}

fn is_ws_bridge_cancel(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text).ok()
        .and_then(|value| value.get("type").and_then(|kind| kind.as_str()).map(str::to_string))
        .as_deref() == Some("response.cancel")
}

async fn send_sse_messages(
    socket: &mut AxumWebSocket,
    messages: Vec<SseMessage>,
    responses_protocol: bool,
) -> bool {
    for message in messages {
        let data = if message.data.trim() == "[DONE]" {
            if responses_protocol {
                continue;
            }
            json!({ "choices": [], "done": true }).to_string()
        } else if responses_protocol {
            match (message.event, serde_json::from_str::<serde_json::Value>(&message.data)) {
                (Some(event), Ok(mut value)) if event != "message" && value.is_object() => {
                    if value.get("type").is_none() {
                        value["type"] = serde_json::Value::String(event);
                    }
                    value.to_string()
                }
                _ => message.data,
            }
        } else {
            message.data
        };
        if socket.send(AxumWsMessage::Text(data.into())).await.is_err() {
            return false;
        }
    }
    true
}

/// Bridges supported JSON WebSocket requests to HTTP and relays SSE events.
async fn handle_v1_ws_to_sse_bridge(
    mut client_ws: AxumWebSocket,
    state: AppState,
    target_url: String,
    upstream_api_key: String,
    client: LlmtrimTransport,
    client_headers: HeaderMap,
) {
    let _session_guard = SessionGuard::new(&state.active_ws_sessions);
    let responses_protocol = Url::parse(&target_url).ok()
        .map(|url| url.path().trim_end_matches('/').ends_with("/responses"))
        .unwrap_or(false);
    info!("已开启 WebSocket -> HTTP/SSE 桥接: {}", target_url);

    while let Some(message) = client_ws.recv().await {
        let text = match message {
            Ok(AxumWsMessage::Text(text)) => text,
            Ok(AxumWsMessage::Ping(payload)) => {
                if client_ws.send(AxumWsMessage::Pong(payload)).await.is_err() { break; }
                continue;
            }
            Ok(AxumWsMessage::Pong(_)) => continue,
            Ok(AxumWsMessage::Close(frame)) => {
                let _ = client_ws.send(AxumWsMessage::Close(frame)).await;
                break;
            }
            Ok(AxumWsMessage::Binary(_)) => {
                if client_ws.send(ws_bridge_error("unsupported_message", "HTTP 桥接不支持二进制 WebSocket 消息")).await.is_err() { break; }
                continue;
            }
            Err(err) => {
                warn!("读取客户端 WebSocket 帧出错: {}", err);
                break;
            }
        };
        let text = text.trim();
        if text.is_empty() { continue; }

        let (body, wants_stream) = match normalize_ws_bridge_request(text, responses_protocol) {
            Ok(request) => request,
            Err(err) => {
                if client_ws.send(ws_bridge_error("invalid_request_error", err)).await.is_err() { break; }
                continue;
            }
        };
        let mut request_headers = HeaderMap::new();
        request_headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
        request_headers.insert(header::ACCEPT, HeaderValue::from_static("text/event-stream"));
        for (key, value) in client_headers.iter() {
            let name = key.as_str().to_ascii_lowercase();
            if matches!(name.as_str(), "host" | "connection" | "upgrade" | "content-length"
                | "content-type" | "accept" | "keep-alive" | "transfer-encoding" | "proxy-connection")
                || name.starts_with("sec-websocket-")
                || (name == "authorization" && !is_upstream_placeholder(&upstream_api_key))
            {
                continue;
            }
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(key.as_ref()),
                HeaderValue::from_bytes(value.as_bytes()),
            ) {
                request_headers.append(name, value);
            }
        }
        if !is_upstream_placeholder(&upstream_api_key) {
            if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", upstream_api_key.trim())) {
                request_headers.insert(header::AUTHORIZATION, value);
            }
        }

        let pending = client.request(
            &Method::POST,
            &target_url,
            &request_headers,
            Bytes::from(body),
        );
        tokio::pin!(pending);
        let response = loop {
            tokio::select! {
                result = &mut pending => break Some(result),
                frame = client_ws.recv() => match frame {
                    Some(Ok(AxumWsMessage::Ping(payload))) => {
                        if client_ws.send(AxumWsMessage::Pong(payload)).await.is_err() { return; }
                    }
                    Some(Ok(AxumWsMessage::Pong(_))) => {}
                    Some(Ok(AxumWsMessage::Close(frame))) => {
                        let _ = client_ws.send(AxumWsMessage::Close(frame)).await;
                        return;
                    }
                    Some(Ok(AxumWsMessage::Text(text))) if is_ws_bridge_cancel(&text) => {
                        let _ = client_ws.send(ws_bridge_error("request_cancelled", "请求已取消")).await;
                        break None;
                    }
                    Some(Ok(AxumWsMessage::Text(_))) | Some(Ok(AxumWsMessage::Binary(_))) => {
                        if client_ws.send(ws_bridge_error("request_in_progress", "请等待当前请求完成后再发送新消息")).await.is_err() { return; }
                    }
                    Some(Err(_)) | None => return,
                }
            }
        };
        let Some(response) = response else { continue; };
        let response = match response {
            Ok(response) => response,
            Err(err) => {
                if client_ws.send(ws_bridge_error("gateway_error", format!("调用上游 HTTP 接口失败: {}", err))).await.is_err() { break; }
                continue;
            }
        };
        if !response.status.is_success() {
            let status = response.status;
            let mut detail_bytes = Vec::new();
            let mut error_body = response.body;
            while let Some(chunk) = error_body.next().await {
                match chunk {
                    Ok(chunk) => detail_bytes.extend_from_slice(&chunk),
                    Err(_) => break,
                }
                if detail_bytes.len() >= MAX_BRIDGE_BUFFER {
                    break;
                }
            }
            let detail = String::from_utf8_lossy(&detail_bytes).into_owned();
            if client_ws.send(ws_bridge_error("upstream_error", format!("上游返回错误状态 {}: {}", status, detail))).await.is_err() { break; }
            continue;
        }

        let content_type = response.headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok());
        let is_sse = content_type.map(|value| value.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("text/event-stream"))
            .unwrap_or(wants_stream);
        let mut stream = response.body;
        let mut decoder = SseDecoder::default();
        let mut plain_body = Vec::new();
        loop {
            tokio::select! {
                chunk = stream.next() => match chunk {
                    Some(Ok(chunk)) if is_sse => match decoder.feed(&chunk, false) {
                        Ok(messages) => {
                            if !send_sse_messages(&mut client_ws, messages, responses_protocol).await { return; }
                        }
                        Err(err) => {
                            let _ = client_ws.send(ws_bridge_error("upstream_error", err)).await;
                            break;
                        }
                    },
                    Some(Ok(chunk)) => {
                        if plain_body.len().saturating_add(chunk.len()) > MAX_BRIDGE_BUFFER {
                            let _ = client_ws.send(ws_bridge_error("upstream_error", "上游响应超过大小限制")).await;
                            break;
                        }
                        plain_body.extend_from_slice(&chunk);
                    }
                    Some(Err(err)) => {
                        let _ = client_ws.send(ws_bridge_error("upstream_error", format!("读取上游响应失败: {}", err))).await;
                        break;
                    }
                    None => {
                        if is_sse {
                            match decoder.feed(&[], true) {
                                Ok(messages) => {
                                    if !send_sse_messages(&mut client_ws, messages, responses_protocol).await { return; }
                                }
                                Err(err) => { let _ = client_ws.send(ws_bridge_error("upstream_error", err)).await; }
                            }
                        } else if !plain_body.is_empty() {
                            let text = String::from_utf8_lossy(&plain_body).into_owned();
                            if client_ws.send(AxumWsMessage::Text(text.into())).await.is_err() { return; }
                        }
                        break;
                    }
                },
                frame = client_ws.recv() => match frame {
                    Some(Ok(AxumWsMessage::Ping(payload))) => {
                        if client_ws.send(AxumWsMessage::Pong(payload)).await.is_err() { return; }
                    }
                    Some(Ok(AxumWsMessage::Pong(_))) => {}
                    Some(Ok(AxumWsMessage::Close(frame))) => {
                        let _ = client_ws.send(AxumWsMessage::Close(frame)).await;
                        return;
                    }
                    Some(Ok(AxumWsMessage::Text(text))) if is_ws_bridge_cancel(&text) => {
                        let _ = client_ws.send(ws_bridge_error("request_cancelled", "请求已取消")).await;
                        break;
                    }
                    Some(Ok(AxumWsMessage::Text(_))) | Some(Ok(AxumWsMessage::Binary(_))) => {
                        if client_ws.send(ws_bridge_error("request_in_progress", "请等待当前请求完成后再发送新消息")).await.is_err() { return; }
                    }
                    Some(Err(_)) | None => return,
                }
            }
        }
    }
    info!("WebSocket -> HTTP/SSE 桥接会话已结束");
}

/// 核心反向代理处理函数：流式转发 /v1/* 请求到配置的 upstream_base_url
pub async fn forward_handler(
    State(state): State<AppState>,
    req: Request,
) -> Response {
    let path = state.config.read().await.llmtrim_path.clone();
    let (llmtrim_running, mut route_changed) = refresh_llmtrim_route(&state, &path);
    if llmtrim_running && !llmtrim_route_available(&state) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": { "type": "llmtrim_unavailable", "message": "llmtrim 未就绪，禁止回退到默认路由" } })),
        ).into_response();
    }
    let response_route_changed = route_changed.clone();
    tokio::select! {
        biased;
        _ = route_changed.changed() => (
            StatusCode::SERVICE_UNAVAILABLE,
            "代理路由已切换，请重新连接",
        ).into_response(),
        response = forward_handler_selected(state, req, llmtrim_running, response_route_changed) => response,
    }
}

async fn forward_via_llmtrim(
    state: AppState,
    method: Method,
    path: String,
    target_url: String,
    fwd_headers: HeaderMap,
    body_bytes: Bytes,
    mut route_changed: tokio::sync::watch::Receiver<u64>,
    retry_enabled: bool,
    retry_count: usize,
    retry_interval: u64,
    retry_ranges: Vec<(u16, u16)>,
) -> Response {
    let transport = match select_llmtrim_http1_client(&state).await {
        Ok(client) => client,
        Err(err) => return (StatusCode::BAD_GATEWAY, err).into_response(),
    };

    state.active_requests.fetch_add(1, Ordering::SeqCst);
    let _guard = RequestGuard(state.active_requests.clone());
    let start_time = Instant::now();
    let mut attempts = 0usize;

    loop {
        attempts += 1;
        let response = transport
            .request(&method, &target_url, &fwd_headers, body_bytes.clone())
            .await;

        match response {
            Ok(response) => {
                let status_u16 = response.status.as_u16();
                if retry_enabled
                    && attempts <= retry_count
                    && retry_ranges
                        .iter()
                        .any(|&(start, end)| status_u16 >= start && status_u16 <= end)
                {
                    let retry_after_secs = response
                        .headers
                        .get(header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.trim().parse::<u64>().ok())
                        .map(|value| value.clamp(1, 60));
                    let base_delay = retry_after_secs.unwrap_or_else(|| retry_interval.max(1));
                    let jitter_ms = (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| u64::from(duration.subsec_millis() % 100 + 50))
                        .unwrap_or(50)) as u64;
                    let total_delay = Duration::from_millis(base_delay * 1000 + jitter_ms);
                    warn!(
                        "LLMTrim 上游返回状态码 {}，命中重试规则，等待 {:.2}s 后进行第 {} 次重试...",
                        status_u16,
                        total_delay.as_secs_f64(),
                        attempts
                    );
                    drop(response);
                    tokio::time::sleep(total_delay).await;
                    continue;
                }

                info!(
                    "{} {} via llmtrim completed in {:?}",
                    method,
                    path,
                    start_time.elapsed()
                );
                let mut response_builder = Response::builder().status(response.status);
                for (name, value) in response.headers.iter() {
                    let lower = name.as_str().to_ascii_lowercase();
                    if lower == "connection" || lower == "transfer-encoding" {
                        continue;
                    }
                    response_builder = response_builder.header(name, value);
                }
                let stream = response.body.take_until(async move {
                    let _ = route_changed.changed().await;
                });
                return response_builder
                    .body(Body::from_stream(stream))
                    .unwrap_or_else(|_| {
                        (StatusCode::INTERNAL_SERVER_ERROR, "构建响应体失败").into_response()
                    });
            }
            Err(err) => {
                if retry_enabled && attempts <= retry_count {
                    let jitter_ms = (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| u64::from(duration.subsec_millis() % 100 + 50))
                        .unwrap_or(50)) as u64;
                    let total_delay = Duration::from_millis(retry_interval.max(1) * 1000 + jitter_ms);
                    warn!(
                        "请求 LLMTrim/上游发生网络错误: {}，等待 {:.2}s 进行第 {} 次重试...",
                        err,
                        total_delay.as_secs_f64(),
                        attempts
                    );
                    tokio::time::sleep(total_delay).await;
                    continue;
                }
                error!(
                    "{} {} 经 LLMTrim 请求最终失败: {} (耗时: {:?})",
                    method,
                    path,
                    err,
                    start_time.elapsed()
                );
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({
                        "error": {
                            "message": format!("LLMTrim/上游连接失败: {}", err),
                            "type": "bad_gateway"
                        }
                    })),
                )
                    .into_response();
            }
        }
    }
}

async fn forward_handler_selected(
    state: AppState,
    req: Request,
    llmtrim_running: bool,
    route_changed: tokio::sync::watch::Receiver<u64>,
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

    let effective_ws_proxy = if llmtrim_running {
        Some(LLMTRIM_PROXY_URL.to_string())
    } else if !cfg.outbound_proxy.trim().is_empty() {
        Some(cfg.outbound_proxy.trim().to_string())
    } else {
        None
    };

    if is_ws {
        if cfg.upstream_websocket_enabled && !llmtrim_running {
            if let Ok(ws) = WebSocketUpgrade::from_request_parts(&mut parts, &state).await {
                let ws_target = target_url.clone();
                let key = cfg.upstream_api_key.clone();
                let ws_state = state.clone();
                let ws_proxy = effective_ws_proxy.clone();
                let client_headers = parts.headers.clone();
                let mut ws_route_changed = route_changed.clone();
                return ws
                    .on_upgrade(move |socket| async move {
                        tokio::select! {
                            biased;
                            _ = ws_route_changed.changed() => {},
                            _ = handle_v1_websocket_tunnel(socket, ws_state, ws_target, key, ws_proxy, llmtrim_running, client_headers) => {},
                        }
                    })
                    .into_response();
            }
        } else {
            // 当上游 WebSocket 开关关闭时，支持无缝降级为 SSE 承载：
            // 将客户端的 WS 会话无缝桥接到上游默认的 SSE 流式接口
            if let Ok(ws) = WebSocketUpgrade::from_request_parts(&mut parts, &state).await {
                let ws_target = target_url.clone();
                let key = cfg.upstream_api_key.clone();
                let ws_state = state.clone();
                let client_result = if llmtrim_running {
                    select_llmtrim_http1_client(&state).await
                } else {
                    select_upstream_client(&state, false, &cfg.outbound_proxy)
                        .await
                        .map(LlmtrimTransport::from_reqwest)
                };
                let client = match client_result {
                    Ok(client) => client,
                    Err(err) => return (StatusCode::BAD_GATEWAY, err).into_response(),
                };
                let client_headers = parts.headers.clone();
                let mut ws_route_changed = route_changed.clone();
                return ws
                    .on_upgrade(move |socket| async move {
                        tokio::select! {
                            biased;
                            _ = ws_route_changed.changed() => {},
                            _ = handle_v1_ws_to_sse_bridge(socket, ws_state, ws_target, key, client, client_headers) => {},
                        }
                    })
                    .into_response();
            }
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

    // When LLMTrim is selected, keep the whole outbound request on the local
    // plain HTTP/1.1 proxy hop. The absolute target URI is sent to LLMTrim;
    // LLMTrim then owns the external upstream connection and response stream.
    if llmtrim_running {
        let (_, parsed_retry_ranges) = crate::common::config::normalize_retry_status_codes(&cfg.retry_status_codes);
        return forward_via_llmtrim(
            state,
            parts.method,
            path,
            target_url,
            fwd_headers,
            body_bytes,
            route_changed,
            cfg.retry_enabled,
            cfg.retry_count.parse().unwrap_or(5),
            cfg.retry_interval_seconds.parse().unwrap_or(1),
            parsed_retry_ranges,
        )
        .await;
    }

    // llmtrim 运行时只能使用其专属客户端，初始化失败不得回退。
    let client = match select_upstream_client(&state, false, &cfg.outbound_proxy).await {
        Ok(client) => client,
        Err(err) => return (StatusCode::BAD_GATEWAY, err).into_response(),
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
                let mut response_route_changed = route_changed;
                let stream = upstream_resp.bytes_stream().take_until(async move {
                    let _ = response_route_changed.changed().await;
                });
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn test_state(upstream: String, outbound_proxy: String) -> AppState {
        let config = AppConfig {
            upstream_base_url: upstream,
            outbound_proxy,
            llmtrim_path: std::env::temp_dir().join(format!("llmtrim-route-test-{}", std::process::id()))
                .join("llmtrim.exe").to_string_lossy().to_string(),
            ..AppConfig::default()
        };
        let mut state = AppState::new(config, std::env::temp_dir().join("llmtrim-route-test.yaml"));
        state.http_client = reqwest::Client::builder().no_proxy().build().unwrap();
        state
    }

    async fn serve_mock_http_proxy(
        listener: TcpListener,
        count: usize,
        content_type: &str,
        body: &str,
    ) -> Vec<String> {
        let mut requests = Vec::new();
        for _ in 0..count {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 2048];
            let header_end = loop {
                let size = socket.read(&mut buf).await.unwrap();
                assert!(size > 0, "request ended before headers");
                request.extend_from_slice(&buf[..size]);
                if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok()).flatten()
            }).unwrap_or(0);
            while request.len() < header_end + content_length {
                let size = socket.read(&mut buf).await.unwrap();
                assert!(size > 0, "request ended before its body");
                request.extend_from_slice(&buf[..size]);
            }
            requests.push(String::from_utf8(request).unwrap());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                content_type, body.len(), body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
        requests
    }

    fn mock_proxy_client(address: std::net::SocketAddr) -> reqwest::Client {
        reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(format!("http://{}", address)).unwrap())
            .build()
            .unwrap()
    }

    async fn set_mock_llmtrim_client(state: &AppState, address: std::net::SocketAddr) {
        let generation = *state.proxy_route_changed.borrow();
        *state.llmtrim_client.write().await = Some((generation, mock_proxy_client(address)));
        let http1_client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(format!("http://{}", address)).unwrap())
            .http1_only()
            .build().unwrap();
        *state.llmtrim_http1_client.write().await = Some((
            generation,
            LlmtrimTransport::from_reqwest(http1_client),
        ));
    }

    #[tokio::test]
    async fn llmtrim_has_priority_for_all_http_methods_and_errors_do_not_fall_back() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_address = listener.local_addr().unwrap();
        let normal_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let normal_address = normal_listener.local_addr().unwrap();
        let state = test_state(
            format!("http://{}/v1", normal_address),
            format!("http://{}", normal_address),
        );
        *state.proxy_running.write().await = true;
        observe_llmtrim_route(&state, || true);
        set_mock_llmtrim_client(&state, proxy_address).await;
        let server = tokio::spawn(async move {
            serve_mock_http_proxy(listener, 5, "application/json", "{\"via\":\"llmtrim\"}").await
        });

        for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
            let (_, route_changed) = observe_llmtrim_route(&state, || true);
            let request = Request::builder()
                .method(method)
                .uri("/v1/chat/completions")
                .body(Body::from("payload"))
                .unwrap();
            let response = forward_handler_selected(state.clone(), request, true, route_changed).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(axum::body::to_bytes(response.into_body(), 1024).await.unwrap(), "{\"via\":\"llmtrim\"}");
        }
        let requests = tokio::time::timeout(Duration::from_secs(3), server).await.unwrap().unwrap();
        for (request, method) in requests.iter().zip(["GET", "POST", "PUT", "PATCH", "DELETE"]) {
            assert!(request.starts_with(&format!("{} http://{}/v1/chat/completions ", method, normal_address)), "{}", request);
            assert!(request.ends_with("payload"));
        }

        let closed_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed_address = closed_listener.local_addr().unwrap();
        drop(closed_listener);
        set_mock_llmtrim_client(&state, closed_address).await;
        let (_, route_changed) = observe_llmtrim_route(&state, || true);
        let request = Request::builder().uri("/v1/models").body(Body::empty()).unwrap();
        let response = forward_handler_selected(state, request, true, route_changed).await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert!(tokio::time::timeout(Duration::from_millis(200), normal_listener.accept()).await.is_err());
    }

    #[test]
    fn direct_gateway_client_ignores_inherited_proxy_environment() {
        const CHILD_FLAG: &str = "CODE_MANAGER_PROXY_ENV_TEST_CHILD";
        if std::env::var_os(CHILD_FLAG).is_some() {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async {
                let state = AppState::new(AppConfig::default(), std::env::temp_dir().join("unused-config.yaml"));
                let result = state.http_client.get("http://127.0.0.1:1/")
                    .timeout(Duration::from_millis(500)).send().await;
                assert!(result.is_err());
            });
            return;
        }

        let proxy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        proxy.set_nonblocking(true).unwrap();
        let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args(["--exact", "server::proxy::tests::direct_gateway_client_ignores_inherited_proxy_environment"])
            .env(CHILD_FLAG, "1")
            .env("HTTP_PROXY", &proxy_url)
            .env("http_proxy", &proxy_url)
            .env_remove("NO_PROXY")
            .env_remove("no_proxy");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            child.creation_flags(0x08000000);
        }
        let result = child.output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        assert!(matches!(proxy.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock));
    }

    #[tokio::test]
    async fn stopped_llmtrim_uses_the_configured_normal_proxy() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let state = test_state("http://unreachable.invalid/v1".into(), format!("http://{}", address));
        *state.proxy_running.write().await = true;
        set_mock_llmtrim_client(&state, address).await;
        let server = tokio::spawn(async move {
            serve_mock_http_proxy(listener, 1, "text/plain", "normal").await
        });
        let (_, route_changed) = observe_llmtrim_route(&state, || false);
        let request = Request::builder().uri("/v1/models").body(Body::empty()).unwrap();
        let response = forward_handler_selected(state.clone(), request, false, route_changed).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(axum::body::to_bytes(response.into_body(), 1024).await.unwrap(), "normal");
        assert!(state.llmtrim_client.read().await.is_none());
        let requests = tokio::time::timeout(Duration::from_secs(3), server).await.unwrap().unwrap();
        assert!(requests[0].starts_with("GET http://unreachable.invalid/v1/models "));
    }

    #[tokio::test]
    async fn missing_or_invalid_llmtrim_ca_never_uses_the_normal_client() {
        let state = test_state("http://unreachable.invalid/v1".into(), String::new());
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("llmtrim-route-ca-{}-{}.pem", std::process::id(), nonce));
        let error = select_upstream_client_with_ca(&state, true, "", &path).await.unwrap_err();
        assert!(error.contains("读取 llmtrim CA"));
        assert!(build_llmtrim_ws_connector(&path).is_err());
        std::fs::write(&path, b"invalid certificate").unwrap();
        let error = select_upstream_client_with_ca(&state, true, "", &path).await.unwrap_err();
        assert!(error.contains("解析 llmtrim CA"));
        assert!(build_llmtrim_ws_connector(&path).is_err());
        assert!(state.llmtrim_client.read().await.is_none());
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn llmtrim_restart_invalidates_cached_certificate_and_connections() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let state = test_state("http://unreachable.invalid/v1".into(), String::new());
        observe_llmtrim_route(&state, || true);
        set_mock_llmtrim_client(&state, listener.local_addr().unwrap()).await;
        observe_llmtrim_route(&state, || false);
        observe_llmtrim_route(&state, || true);
        let missing_ca = std::env::temp_dir().join(format!("missing-ca-for-llmtrim-test-{}", std::process::id()));
        let result = select_upstream_client_with_ca(&state, true, "", &missing_ca).await;
        assert!(result.is_err());
        assert!(state.llmtrim_client.read().await.is_none());
    }

    #[tokio::test]
    async fn forward_handler_cancels_an_inflight_request_when_the_route_changes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted, request_seen) = tokio::sync::oneshot::channel();
        let upstream = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            socket.read(&mut request).await.unwrap();
            let _ = accepted.send(());
            let mut byte = [0; 1];
            let _ = socket.read(&mut byte).await;
        });
        let state = test_state(format!("http://{}/v1", address), String::new());
        *state.proxy_running.write().await = true;
        let request = Request::builder().uri("/v1/models").body(Body::empty()).unwrap();
        let task_state = state.clone();
        let request_task = tokio::spawn(async move {
            forward_handler(State(task_state), request).await
        });
        tokio::time::timeout(Duration::from_secs(2), request_seen).await.unwrap().unwrap();
        observe_llmtrim_route(&state, || true);
        let response = tokio::time::timeout(Duration::from_secs(2), request_task).await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        tokio::time::timeout(Duration::from_secs(2), upstream).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn changing_routes_stops_an_existing_http_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            socket.read(&mut request).await.unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n").await.unwrap();
            let mut byte = [0; 1];
            let _ = socket.read(&mut byte).await;
        });
        let state = test_state(format!("http://{}/v1", address), String::new());
        *state.proxy_running.write().await = true;
        let (_, route_changed) = observe_llmtrim_route(&state, || false);
        let request = Request::builder().uri("/v1/models").body(Body::empty()).unwrap();
        let response = forward_handler_selected(state.clone(), request, false, route_changed).await;
        let mut stream = response.into_body().into_data_stream();
        assert_eq!(stream.next().await.unwrap().unwrap(), "hello");
        observe_llmtrim_route(&state, || true);
        assert!(tokio::time::timeout(Duration::from_secs(2), stream.next()).await.unwrap().is_none());
        drop(stream);
        tokio::time::timeout(Duration::from_secs(2), server).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn websocket_tunnel_uses_the_selected_proxy() {
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_address = proxy_listener.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            let (mut socket, _) = proxy_listener.accept().await.unwrap();
            let mut connect_request = Vec::new();
            let mut byte = [0; 1];
            while !connect_request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.unwrap();
                connect_request.push(byte[0]);
            }
            socket.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await.unwrap();
            let mut upstream = tokio_tungstenite::accept_async(socket).await.unwrap();
            let message = upstream.next().await.unwrap().unwrap();
            upstream.send(message).await.unwrap();
            String::from_utf8(connect_request).unwrap()
        });
        let state = test_state("http://unused.invalid/v1".into(), String::new());
        let app = Router::new().route("/ws", get({
            let state = state.clone();
            move |ws: WebSocketUpgrade| {
                let state = state.clone();
                async move {
                    ws.on_upgrade(move |socket| handle_v1_websocket_tunnel(
                        socket,
                        state,
                        "http://example.invalid:9000/v1".into(),
                        String::new(),
                        Some(format!("http://{}", proxy_address)),
                        true,
                        HeaderMap::new(),
                    ))
                }
            }
        }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let gateway = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (mut client, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", address)).await.unwrap();
        client.send(TungsteniteMessage::Text("hello".into())).await.unwrap();
        assert_eq!(client.next().await.unwrap().unwrap().into_text().unwrap(), "hello");
        let request = tokio::time::timeout(Duration::from_secs(2), proxy_task).await.unwrap().unwrap();
        assert!(request.starts_with("CONNECT example.invalid:9000 HTTP/1.1\r\n"));
        drop(client);
        gateway.abort();
    }

    #[tokio::test]
    async fn websocket_sse_bridge_uses_llmtrim_and_closes_when_route_changes() {
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_address = proxy_listener.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            serve_mock_http_proxy(proxy_listener, 1, "text/event-stream", "data: {\"ok\":true}\n\n").await
        });
        let state = test_state("http://unreachable.invalid/v1".into(), "http://127.0.0.1:1".into());
        state.config.write().await.upstream_websocket_enabled = false;
        *state.proxy_running.write().await = true;
        observe_llmtrim_route(&state, || true);
        set_mock_llmtrim_client(&state, proxy_address).await;
        let app = Router::new()
            .route("/v1/{*path}", axum::routing::any(|State(state): State<AppState>, req: Request| async move {
                let (_, route_changed) = observe_llmtrim_route(&state, || true);
                forward_handler_selected(state, req, true, route_changed).await
            }))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let gateway = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (mut client, _) = tokio_tungstenite::connect_async(format!("ws://{}/v1/chat/completions", address)).await.unwrap();
        client.send(TungsteniteMessage::Text("{\"messages\":[]}".into())).await.unwrap();
        let message = tokio::time::timeout(Duration::from_secs(2), client.next()).await.unwrap().unwrap().unwrap();
        assert_eq!(message.into_text().unwrap(), "{\"ok\":true}");
        let requests = tokio::time::timeout(Duration::from_secs(2), proxy_task).await.unwrap().unwrap();
        assert!(requests[0].starts_with("POST http://unreachable.invalid/v1/chat/completions "));
        assert!(requests[0].contains("\"stream\":true"));
        observe_llmtrim_route(&state, || false);
        let closed = tokio::time::timeout(Duration::from_secs(2), client.next()).await.unwrap();
        assert!(closed.is_none() || closed.unwrap().is_err());
        tokio::time::timeout(Duration::from_secs(2), async {
            while state.active_ws_sessions.load(Ordering::SeqCst) != 0 {
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
        gateway.abort();
    }

    #[test]
    fn llmtrim_route_stays_unavailable_during_management_transitions() {
        let state = test_state("http://unreachable.invalid/v1".into(), String::new());
        activate_llmtrim_route(&state);
        let (selected, _) = observe_llmtrim_route(&state, || true);
        assert!(selected);
        assert!(!llmtrim_route_available(&state));
        finish_llmtrim_route_transition(&state);
        observe_llmtrim_route(&state, || true);
        assert!(llmtrim_route_available(&state));
    }

    #[test]
    fn llmtrim_route_blocks_until_its_intent_can_be_persisted() {
        let state = test_state("http://unreachable.invalid/v1".into(), String::new());
        let (selected, _) = observe_llmtrim_route_with(&state, || true, || Err("ledger unavailable".into()));
        assert!(selected);
        assert!(!llmtrim_route_available(&state));
        let (selected, _) = observe_llmtrim_route_with(&state, || true, || Ok(()));
        assert!(selected);
        assert!(llmtrim_route_available(&state));
    }

    #[test]
    fn llmtrim_route_stays_selected_after_listener_loss() {
        let state = test_state("http://unreachable.invalid/v1".into(), String::new());
        assert!(!observe_llmtrim_route(&state, || false).0);
        let (selected, changed) = observe_llmtrim_route(&state, || true);
        assert!(selected);
        assert!(llmtrim_route_available(&state));

        let (selected, _) = observe_llmtrim_route(&state, || false);
        assert!(selected);
        assert!(!llmtrim_route_available(&state));
        assert!(changed.has_changed().unwrap());

        deactivate_llmtrim_route(&state);
        assert!(!observe_llmtrim_route(&state, || false).0);
    }

    #[tokio::test]
    async fn unavailable_llmtrim_never_sends_requests_to_the_default_upstream() {
        let normal = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = normal.local_addr().unwrap();
        let state = test_state(format!("http://{}/v1", address), String::new());
        *state.proxy_running.write().await = true;
        observe_llmtrim_route(&state, || true);
        observe_llmtrim_route(&state, || false);

        let request = Request::builder().uri("/v1/models").body(Body::empty()).unwrap();
        let response = forward_handler(State(state.clone()), request).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(tokio::time::timeout(Duration::from_millis(200), normal.accept()).await.is_err());

        deactivate_llmtrim_route(&state);
        let server = tokio::spawn(async move {
            serve_mock_http_proxy(normal, 1, "text/plain", "normal").await
        });
        let request = Request::builder().uri("/v1/models").body(Body::empty()).unwrap();
        let response = forward_handler(State(state.clone()), request).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(axum::body::to_bytes(response.into_body(), 1024).await.unwrap(), "normal");
        tokio::time::timeout(Duration::from_secs(2), server).await.unwrap().unwrap();
    }

    #[test]
    fn desired_llmtrim_route_is_restored_from_the_state_ledger() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("llmtrim-route-ledger-{}-{}", std::process::id(), nonce));
        std::fs::create_dir_all(&directory).unwrap();
        crate::tools::llmtrim::ownership::set_managed_tool_desired_running(&directory, true).unwrap();
        let config = AppConfig {
            llmtrim_path: directory.join("llmtrim.exe").to_string_lossy().to_string(),
            ..AppConfig::default()
        };
        let state = AppState::new(config, directory.join("config.yaml"));
        assert!(observe_llmtrim_route(&state, || false).0);
        assert!(!llmtrim_route_available(&state));
        std::fs::remove_file(directory.join(crate::tools::llmtrim::ownership::TOOL_STATE_FILE)).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn running_llmtrim_is_adopted_and_route_survives_app_restart() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("llmtrim-adopt-{}-{}", std::process::id(), nonce));
        std::fs::create_dir_all(&directory).unwrap();
        let config = AppConfig {
            llmtrim_path: directory.join("llmtrim.exe").to_string_lossy().to_string(),
            ..AppConfig::default()
        };
        let state = AppState::new(config.clone(), directory.join("config.yaml"));
        assert!(!state.llmtrim_route.lock().unwrap().selected);
        let (selected, _) = observe_llmtrim_route_with(&state, || true, || {
            crate::tools::llmtrim::ownership::set_managed_tool_desired_running(&directory, true)
        });
        assert!(selected);
        assert!(llmtrim_route_available(&state));

        let restarted = AppState::new(config, directory.join("config.yaml"));
        assert!(restarted.llmtrim_route.lock().unwrap().selected);
        assert!(!llmtrim_route_available(&restarted));
        observe_llmtrim_route(&restarted, || true);
        assert!(llmtrim_route_available(&restarted));
        std::fs::remove_file(directory.join(crate::tools::llmtrim::ownership::TOOL_STATE_FILE)).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn sse_decoder_preserves_split_utf8_and_multiline_events() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.feed(b"event: response.output_text.delta\r\ndata: \xe4", false).unwrap().is_empty());
        assert!(decoder.feed(b"\xbd\xa0\r", false).unwrap().is_empty());
        let messages = decoder.feed(b"\ndata: second\r\n\r\n", false).unwrap();
        assert_eq!(messages, vec![SseMessage {
            event: Some("response.output_text.delta".into()),
            data: "你\nsecond".into(),
        }]);
        let mut decoder = SseDecoder::default();
        assert!(decoder.feed(b"\xef\xbb", false).unwrap().is_empty());
        let messages = decoder.feed(b"\xbfdata: tail", true).unwrap();
        assert_eq!(messages, vec![SseMessage { event: None, data: "tail".into() }]);
    }

    #[test]
    fn response_create_is_converted_to_an_http_request_body() {
        let (body, streaming) = normalize_ws_bridge_request(
            r#"{"type":"response.create","model":"example","input":"hello"}"#, true,
        ).unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(streaming);
        assert_eq!(payload["stream"], true);
        assert!(payload.get("type").is_none());
        assert!(normalize_ws_bridge_request(r#"{"type":"response.create","stream_id":"lane"}"#, true).is_err());
        assert!(normalize_ws_bridge_request(r#"{"type":"response.create","generate":false}"#, true).is_err());
        assert!(normalize_ws_bridge_request(r#"{"type":"response.steer"}"#, true).is_err());
    }

    #[tokio::test]
    async fn llmtrim_websocket_bridge_uses_http1_for_both_ws_settings() {
        for websocket_enabled in [true, false] {
            let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let proxy_address = proxy_listener.local_addr().unwrap();
            let proxy_task = tokio::spawn(async move {
                serve_mock_http_proxy(
                    proxy_listener, 1, "text/event-stream",
                    "event: response.completed\ndata: {\"response\":{\"id\":\"resp_1\"}}\n\n",
                ).await
            });
            let state = test_state("http://unreachable.invalid/v1".into(), "http://127.0.0.1:1".into());
            state.config.write().await.upstream_websocket_enabled = websocket_enabled;
            *state.proxy_running.write().await = true;
            observe_llmtrim_route(&state, || true);
            set_mock_llmtrim_client(&state, proxy_address).await;
            let app = Router::new()
                .route("/v1/{*path}", axum::routing::any(|State(state): State<AppState>, req: Request| async move {
                    let (_, route_changed) = observe_llmtrim_route(&state, || true);
                    forward_handler_selected(state, req, true, route_changed).await
                }))
                .with_state(state);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let gateway = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let (mut client, _) = tokio_tungstenite::connect_async(format!("ws://{}/v1/responses", address)).await.unwrap();
            client.send(TungsteniteMessage::Text(r#"{"type":"response.create","model":"example","input":"hi"}"#.into())).await.unwrap();
            let message = tokio::time::timeout(Duration::from_secs(2), client.next()).await.unwrap().unwrap().unwrap();
            let event: serde_json::Value = serde_json::from_str(message.into_text().unwrap().as_str()).unwrap();
            assert_eq!(event["type"], "response.completed");

            let requests = tokio::time::timeout(Duration::from_secs(2), proxy_task).await.unwrap().unwrap();
            assert!(requests[0].starts_with("POST http://unreachable.invalid/v1/responses HTTP/1.1\r\n"));
            assert!(!requests[0].to_ascii_lowercase().contains("\r\nupgrade:"));
            let body = requests[0].split_once("\r\n\r\n").unwrap().1;
            let payload: serde_json::Value = serde_json::from_str(body).unwrap();
            assert_eq!(payload["stream"], true);
            assert!(payload.get("type").is_none());

            client.send(TungsteniteMessage::Binary(vec![1, 2, 3].into())).await.unwrap();
            let message = tokio::time::timeout(Duration::from_secs(2), client.next()).await.unwrap().unwrap().unwrap();
            let error: serde_json::Value = serde_json::from_str(message.into_text().unwrap().as_str()).unwrap();
            assert_eq!(error["error"]["type"], "unsupported_message");
            drop(client);
            gateway.abort();
        }
    }

    #[tokio::test]
    async fn websocket_bridge_forwards_a_non_streaming_http_response() {
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_address = proxy_listener.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            serve_mock_http_proxy(proxy_listener, 1, "application/json", "{\"id\":\"resp_2\"}").await
        });
        let state = test_state("http://unreachable.invalid/v1".into(), String::new());
        *state.proxy_running.write().await = true;
        observe_llmtrim_route(&state, || true);
        set_mock_llmtrim_client(&state, proxy_address).await;
        let app = Router::new()
            .route("/v1/{*path}", axum::routing::any(|State(state): State<AppState>, req: Request| async move {
                let (_, route_changed) = observe_llmtrim_route(&state, || true);
                forward_handler_selected(state, req, true, route_changed).await
            }))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let gateway = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (mut client, _) = tokio_tungstenite::connect_async(format!("ws://{}/v1/responses", address)).await.unwrap();
        client.send(TungsteniteMessage::Text(r#"{"type":"response.create","stream":false}"#.into())).await.unwrap();
        let message = tokio::time::timeout(Duration::from_secs(2), client.next()).await.unwrap().unwrap().unwrap();
        assert_eq!(message.into_text().unwrap(), "{\"id\":\"resp_2\"}");
        let requests = tokio::time::timeout(Duration::from_secs(2), proxy_task).await.unwrap().unwrap();
        assert!(requests[0].contains("\"stream\":false"));
        drop(client);
        gateway.abort();
    }

    #[tokio::test]
    async fn upstream_prewarm_uses_the_normal_proxy_when_llmtrim_is_stopped() {
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_address = proxy_listener.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            serve_mock_http_proxy(proxy_listener, 1, "text/plain", "ok").await
        });
        let state = test_state("http://unreachable.invalid/v1".into(), format!("http://{}", proxy_address));
        *state.web_port.write().await = 7788;
        let response = start_proxy(State(state.clone())).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let requests = tokio::time::timeout(Duration::from_secs(2), proxy_task).await.unwrap().unwrap();
        assert!(requests[0].starts_with("GET http://unreachable.invalid/v1 "));
        let _ = state.shutdown_sender.send(());
    }

    #[tokio::test]
    #[ignore = "requires an isolated llmtrim executable supplied through LLMTRIM_TEST_EXECUTABLE"]
    async fn real_llmtrim_proxy_accepts_local_gateway_requests() {
        struct IsolatedDaemon {
            child: std::process::Child,
            directory: std::path::PathBuf,
        }
        impl Drop for IsolatedDaemon {
            fn drop(&mut self) {
                let _ = self.child.kill();
                let _ = self.child.wait();
                let _ = std::fs::remove_dir_all(&self.directory);
            }
        }

        let executable = std::env::var_os("LLMTRIM_TEST_EXECUTABLE").expect("isolated llmtrim executable");
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("llmtrim-integration-{}-{}", std::process::id(), nonce));
        let daemon_home = directory.join(".llmtrim");
        std::fs::create_dir_all(&daemon_home).unwrap();
        let config_path = directory.join("config.toml");
        std::fs::write(&config_path, "").unwrap();
        let log = std::fs::File::create(directory.join("serve.log")).unwrap();
        let port_reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = port_reservation.local_addr().unwrap().port();
        drop(port_reservation);

        let mut command = std::process::Command::new(executable);
        command.args(["serve", "--port", &port.to_string()])
            .env("LLMTRIM_HOME", &daemon_home)
            .env("LLMTRIM_CONFIG", &config_path)
            .env("HOME", &directory)
            .env("USERPROFILE", &directory)
            .env("APPDATA", &directory)
            .env("LOCALAPPDATA", &directory)
            .env("XDG_CONFIG_HOME", &directory)
            .env("XDG_DATA_HOME", &directory)
            .env("TEMP", &directory)
            .env("TMP", &directory)
            .env_remove("HTTP_PROXY")
            .env_remove("HTTPS_PROXY")
            .env_remove("ALL_PROXY")
            .env_remove("http_proxy")
            .env_remove("https_proxy")
            .env_remove("all_proxy")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::from(log.try_clone().unwrap()))
            .stderr(std::process::Stdio::from(log));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let child = command.spawn().unwrap();
        let mut daemon = IsolatedDaemon { child, directory };
        let ca_path = daemon_home.join("ca.pem");
        let proxy_address = format!("127.0.0.1:{}", port);
        let ready = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if daemon.child.try_wait().unwrap().is_some() {
                    break false;
                }
                if ca_path.is_file() && tokio::net::TcpStream::connect(&proxy_address).await.is_ok() {
                    break true;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }).await.unwrap_or(false);
        assert!(ready, "isolated llmtrim failed to start: {}", std::fs::read_to_string(daemon.directory.join("serve.log")).unwrap_or_default());

        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let server = tokio::spawn(async move {
            serve_mock_http_proxy(upstream, 1, "text/plain", "through real llmtrim").await
        });
        let client = LlmtrimTransport::plain_at(proxy_address.parse().unwrap());
        let response = client
            .request(
                &Method::GET,
                &format!("http://{}/v1/models", upstream_address),
                &HeaderMap::new(),
                Bytes::new(),
            )
            .await
            .unwrap();
        assert_eq!(response.status, StatusCode::OK);
        let mut response_body = Vec::new();
        let mut body = response.body;
        while let Some(chunk) = body.next().await {
            response_body.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(response_body, b"through real llmtrim");
        let requests = tokio::time::timeout(Duration::from_secs(2), server).await.unwrap().unwrap();
        assert!(requests[0].starts_with("GET /v1/models "));
    }

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

    #[test]
    fn test_ws_target_url_scheme_mapping() {
        let base_https = "https://api.openai.com/v1";
        let target_https = join_upstream_url(base_https, "/v1/chat/completions", None).unwrap();
        let ws_target_https = if target_https.starts_with("https://") {
            target_https.replacen("https://", "wss://", 1)
        } else if target_https.starts_with("http://") {
            target_https.replacen("http://", "ws://", 1)
        } else {
            target_https.clone()
        };
        assert_eq!(ws_target_https, "wss://api.openai.com/v1/chat/completions");

        let base_http = "http://127.0.0.1:8000/v1";
        let target_http = join_upstream_url(base_http, "/v1/models", None).unwrap();
        let ws_target_http = if target_http.starts_with("https://") {
            target_http.replacen("https://", "wss://", 1)
        } else if target_http.starts_with("http://") {
            target_http.replacen("http://", "ws://", 1)
        } else {
            target_http.clone()
        };
        assert_eq!(ws_target_http, "ws://127.0.0.1:8000/v1/models");
    }
}
