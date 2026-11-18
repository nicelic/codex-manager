use std::collections::HashSet;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use axum::extract::ws::{CloseFrame, Message as AxumWsMessage, WebSocket as AxumWebSocket};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Response, StatusCode};
use bytes::Bytes;
use futures_util::{SinkExt, Stream, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, watch};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame as TungsteniteCloseFrame;
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;

use super::upstream_transport::{
    UpstreamHttpRequest, UpstreamHttpResponse, UpstreamTransport, UpstreamWsConnection,
    UpstreamWsOutcome,
};

const MAX_SSE_BUFFER: usize = 8 * 1024 * 1024;
const MAX_NON_STREAM_BODY: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub enabled: bool,
    pub max_retries: usize,
    pub interval: Duration,
    pub status_codes: HashSet<u16>,
    pub status_ranges: Vec<(u16, u16)>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            max_retries: 0,
            interval: Duration::ZERO,
            status_codes: HashSet::new(),
            status_ranges: Vec::new(),
        }
    }
}

impl RetryPolicy {
    pub fn should_retry_status(&self, status: u16) -> bool {
        self.enabled
            && (self.status_codes.contains(&status)
                || self
                    .status_ranges
                    .iter()
                    .any(|&(start, end)| status >= start && status <= end))
    }

    pub fn attempts(&self) -> usize {
        if self.enabled {
            1 + self.max_retries
        } else {
            1
        }
    }
}

pub struct ActiveRequestGuard(Arc<AtomicI64>);

impl ActiveRequestGuard {
    pub fn new(counter: Arc<AtomicI64>) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}

impl Drop for ActiveRequestGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

pub struct ActiveWsSessionGuard(Arc<AtomicUsize>);

impl ActiveWsSessionGuard {
    pub fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}

impl Drop for ActiveWsSessionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

pub enum PreparedLocalWs {
    Tunnel(UpstreamWsConnection),
    BridgeHttp,
    HttpError {
        status: StatusCode,
        headers: HeaderMap,
        body: Bytes,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
    pub retry: Option<u64>,
}

/// Incremental UTF-8 Server-Sent Events parser preserving event/id/retry/multi-line data.
pub struct SseDecoder {
    buffer: Vec<u8>,
    event: Option<String>,
    data_lines: Vec<String>,
    id: Option<String>,
    retry: Option<u64>,
}

impl Default for SseDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl SseDecoder {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            event: None,
            data_lines: Vec::new(),
            id: None,
            retry: None,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, String> {
        if self.buffer.len() + chunk.len() > MAX_SSE_BUFFER {
            return Err("SSE stream buffer exceeded 8MB limit".to_string());
        }
        self.buffer.extend_from_slice(chunk);

        let mut events = Vec::new();
        while let Some(newline_pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = self.buffer.drain(..=newline_pos).collect();
            let trimmed = strip_line_ending(&line_bytes);
            let line = String::from_utf8_lossy(trimmed);
            if let Some(ev) = self.process_line(&line) {
                events.push(ev);
            }
        }
        Ok(events)
    }

    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        if !self.buffer.is_empty() {
            let line_bytes = std::mem::take(&mut self.buffer);
            let trimmed = strip_line_ending(&line_bytes);
            if !trimmed.is_empty() {
                let line = String::from_utf8_lossy(trimmed);
                if let Some(ev) = self.process_line(&line) {
                    events.push(ev);
                }
            }
        }
        if let Some(ev) = self.flush_event() {
            events.push(ev);
        }
        events
    }

    fn process_line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return self.flush_event();
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, raw_val) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "data" => self.data_lines.push(raw_val.to_string()),
            "event" => self.event = Some(raw_val.to_string()),
            "id" => self.id = Some(raw_val.to_string()),
            "retry" => {
                if let Ok(ms) = raw_val.parse::<u64>() {
                    self.retry = Some(ms);
                }
            }
            _ => {}
        }
        None
    }

    fn flush_event(&mut self) -> Option<SseEvent> {
        if self.data_lines.is_empty() && self.event.is_none() {
            return None;
        }
        let data = self.data_lines.join("\n");
        self.data_lines.clear();
        let event = self.event.take();
        let id = self.id.clone();
        let retry = self.retry;
        Some(SseEvent {
            event,
            data,
            id,
            retry,
        })
    }
}

fn strip_line_ending(bytes: &[u8]) -> &[u8] {
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    bytes.strip_suffix(b"\r").unwrap_or(bytes)
}

struct MpscBodyStream(mpsc::Receiver<Result<Bytes, io::Error>>);

impl Stream for MpscBodyStream {
    type Item = Result<Bytes, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_recv(cx)
    }
}

/// Probe upstream WebSocket capability when `upstream_websocket_enabled` is true.
/// When false or rejected due to protocol/endpoint capabilities, smoothly selects `BridgeHttp`.
pub async fn prepare_local_ws_session(
    transport: &UpstreamTransport,
    upstream_url: &str,
    prepared_headers: &[(HeaderName, HeaderValue)],
    upstream_websocket_enabled: bool,
    outbound_proxy: &str,
) -> PreparedLocalWs {
    if !upstream_websocket_enabled {
        return PreparedLocalWs::BridgeHttp;
    }

    let Ok(parsed_url) = reqwest::Url::parse(upstream_url) else {
        return PreparedLocalWs::BridgeHttp;
    };

    let ws_headers = filter_ws_upgrade_headers(prepared_headers);
    match transport
        .connect_websocket(&parsed_url, &ws_headers, outbound_proxy)
        .await
    {
        UpstreamWsOutcome::Connected(conn) => PreparedLocalWs::Tunnel(conn),
        UpstreamWsOutcome::Rejected {
            status,
            headers,
            body,
        } => {
            if is_ws_capability_rejection(status) {
                PreparedLocalWs::BridgeHttp
            } else {
                PreparedLocalWs::HttpError {
                    status,
                    headers,
                    body,
                }
            }
        }
        UpstreamWsOutcome::Unavailable(_) => PreparedLocalWs::BridgeHttp,
    }
}

/// Lossless bidirectional relay between local HTTP/1.1 WebSocket and upstream WebSocket
/// (either HTTP/2 Extended CONNECT or HTTP/1.1 TLS/plain WebSocket).
pub async fn relay_ws_to_ws(
    local_ws: AxumWebSocket,
    mut upstream_conn: UpstreamWsConnection,
    active_ws: Arc<AtomicUsize>,
    mut route_changed: watch::Receiver<u64>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    let _ws_guard = ActiveWsSessionGuard::new(active_ws);
    let (mut local_tx, mut local_rx) = local_ws.split();

    loop {
        tokio::select! {
            _ = route_changed.changed() => {
                let _ = local_tx
                    .send(AxumWsMessage::Close(Some(CloseFrame {
                        code: 1001,
                        reason: "upstream route changed".into(),
                    })))
                    .await;
                let _ = upstream_conn
                    .ws
                    .close(Some(TungsteniteCloseFrame {
                        code: CloseCode::Away,
                        reason: "upstream route changed".into(),
                    }))
                    .await;
                break;
            }
            _ = shutdown_rx.recv() => {
                let _ = local_tx
                    .send(AxumWsMessage::Close(Some(CloseFrame {
                        code: 1001,
                        reason: "proxy stopped".into(),
                    })))
                    .await;
                let _ = upstream_conn
                    .ws
                    .close(Some(TungsteniteCloseFrame {
                        code: CloseCode::Away,
                        reason: "proxy stopped".into(),
                    }))
                    .await;
                break;
            }
            inbound = local_rx.next() => {
                match inbound {
                    Some(Ok(msg)) => {
                        let mapped = match msg {
                            AxumWsMessage::Text(t) => Some(TungsteniteMessage::Text(t.as_str().into())),
                            AxumWsMessage::Binary(b) => Some(TungsteniteMessage::Binary(b)),
                            AxumWsMessage::Ping(p) => Some(TungsteniteMessage::Ping(p)),
                            AxumWsMessage::Pong(p) => Some(TungsteniteMessage::Pong(p)),
                            AxumWsMessage::Close(cf) => {
                                let t_cf = cf.map(|c| TungsteniteCloseFrame {
                                    code: CloseCode::from(c.code),
                                    reason: c.reason.as_str().into(),
                                });
                                let _ = upstream_conn.ws.send(TungsteniteMessage::Close(t_cf)).await;
                                break;
                            }
                        };
                        if let Some(m) = mapped {
                            if upstream_conn.ws.send(m).await.is_err() {
                                break;
                            }
                        }
                    }
                    _ => {
                        let _ = upstream_conn.ws.close(None).await;
                        break;
                    }
                }
            }
            outbound = upstream_conn.ws.next() => {
                match outbound {
                    Some(Ok(msg)) => {
                        let mapped = match msg {
                            TungsteniteMessage::Text(t) => Some(AxumWsMessage::Text(t.as_str().into())),
                            TungsteniteMessage::Binary(b) => Some(AxumWsMessage::Binary(b)),
                            TungsteniteMessage::Ping(p) => Some(AxumWsMessage::Ping(p)),
                            TungsteniteMessage::Pong(p) => Some(AxumWsMessage::Pong(p)),
                            TungsteniteMessage::Close(cf) => {
                                let a_cf = cf.map(|c| CloseFrame {
                                    code: c.code.into(),
                                    reason: c.reason.as_str().into(),
                                });
                                let _ = local_tx.send(AxumWsMessage::Close(a_cf)).await;
                                break;
                            }
                            TungsteniteMessage::Frame(_) => None,
                        };
                        if let Some(m) = mapped {
                            if local_tx.send(m).await.is_err() {
                                break;
                            }
                        }
                    }
                    _ => {
                        let _ = local_tx.close().await;
                        break;
                    }
                }
            }
        }
    }
}

/// Bridge a local WebSocket session into upstream HTTP/3 / HTTP/2 SSE (or fallback non-streaming HTTPS).
pub async fn handle_ws_to_http_bridge(
    local_ws: AxumWebSocket,
    transport: Arc<UpstreamTransport>,
    upstream_url: String,
    prepared_headers: Vec<(HeaderName, HeaderValue)>,
    outbound_proxy: String,
    retry_policy: RetryPolicy,
    active_requests: Arc<AtomicI64>,
    active_ws: Arc<AtomicUsize>,
    mut route_changed: watch::Receiver<u64>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    let _ws_guard = ActiveWsSessionGuard::new(active_ws);
    let (mut local_tx, mut local_rx) = local_ws.split();

    loop {
        let msg = tokio::select! {
            _ = route_changed.changed() => {
                let _ = local_tx
                    .send(AxumWsMessage::Close(Some(CloseFrame {
                        code: 1001,
                        reason: "upstream route changed".into(),
                    })))
                    .await;
                break;
            }
            _ = shutdown_rx.recv() => {
                let _ = local_tx
                    .send(AxumWsMessage::Close(Some(CloseFrame {
                        code: 1001,
                        reason: "proxy stopped".into(),
                    })))
                    .await;
                break;
            }
            next = local_rx.next() => match next {
                Some(Ok(m)) => m,
                _ => break,
            }
        };

        match msg {
            AxumWsMessage::Ping(p) => {
                if local_tx.send(AxumWsMessage::Pong(p)).await.is_err() {
                    break;
                }
            }
            AxumWsMessage::Pong(_) => {}
            AxumWsMessage::Close(cf) => {
                let _ = local_tx.send(AxumWsMessage::Close(cf)).await;
                break;
            }
            AxumWsMessage::Binary(bin) => {
                if let Ok(text) = std::str::from_utf8(&bin) {
                    if !forward_ws_turn_via_http(
                        text,
                        &mut local_tx,
                        &transport,
                        &upstream_url,
                        &prepared_headers,
                        &outbound_proxy,
                        &retry_policy,
                        &active_requests,
                        &mut route_changed,
                        &mut shutdown_rx,
                    )
                    .await
                    {
                        break;
                    }
                }
            }
            AxumWsMessage::Text(text) => {
                if !forward_ws_turn_via_http(
                    text.as_str(),
                    &mut local_tx,
                    &transport,
                    &upstream_url,
                    &prepared_headers,
                    &outbound_proxy,
                    &retry_policy,
                    &active_requests,
                    &mut route_changed,
                    &mut shutdown_rx,
                )
                .await
                {
                    break;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn forward_ws_turn_via_http<S>(
    raw_text: &str,
    local_tx: &mut S,
    transport: &UpstreamTransport,
    upstream_url: &str,
    prepared_headers: &[(HeaderName, HeaderValue)],
    outbound_proxy: &str,
    retry_policy: &RetryPolicy,
    active_requests: &Arc<AtomicI64>,
    route_changed: &mut watch::Receiver<u64>,
    shutdown_rx: &mut broadcast::Receiver<()>,
) -> bool
where
    S: futures_util::Sink<AxumWsMessage> + Unpin,
{
    let _req_guard = ActiveRequestGuard::new(Arc::clone(active_requests));
    let Ok(parsed_url) = reqwest::Url::parse(upstream_url) else {
        let _ = send_ws_error(local_tx, 502, "invalid upstream url").await;
        return true;
    };

    let (body_bytes, tried_sse) = prepare_bridge_http_body(raw_text, true);
    let mut base_headers = build_bridged_http_headers(prepared_headers, tried_sse);

    let result = send_http_with_retry_and_sse_fallback(
        transport,
        Method::POST,
        parsed_url,
        &mut base_headers,
        body_bytes,
        tried_sse,
        true,
        outbound_proxy,
        retry_policy,
    )
    .await;

    let mut resp = match result {
        Ok(r) => r,
        Err(err) => {
            let _ = send_ws_error(local_tx, 502, &err).await;
            return true;
        }
    };

    if has_unsupported_content_encoding(&resp.headers) {
        let enc = header_str(&resp.headers, "content-encoding").unwrap_or("unknown");
        let _ = send_ws_error(
            local_tx,
            502,
            &format!("upstream sent unexpected compressed content-encoding: {enc}"),
        )
        .await;
        return true;
    }

    let status = resp.status;
    let is_sse = header_str(&resp.headers, "content-type")
        .map(|ct| ct.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false);

    if !status.is_success() {
        let err_body = resp
            .collect_body(64 * 1024)
            .await
            .unwrap_or_else(|_| Bytes::new());
        let err_text = String::from_utf8_lossy(&err_body);
        if let Ok(v) = serde_json::from_slice::<Value>(&err_body) {
            if v.get("error").is_some() {
                if local_tx.send(AxumWsMessage::Text(v.to_string().into())).await.is_err() {
                    return false;
                }
                return true;
            }
        }
        let _ = send_ws_error(
            local_tx,
            status.as_u16(),
            if err_text.trim().is_empty() {
                status.canonical_reason().unwrap_or("upstream error")
            } else {
                err_text.trim()
            },
        )
        .await;
        return true;
    }

    if !is_sse {
        let body = match resp.collect_body(MAX_NON_STREAM_BODY).await {
            Ok(b) => b,
            Err(e) => {
                let _ = send_ws_error(local_tx, 502, &e.to_string()).await;
                return true;
            }
        };
        let text = String::from_utf8_lossy(&body);
        if text.trim().is_empty() {
            return true;
        }
        return local_tx
            .send(AxumWsMessage::Text(text.into_owned().into()))
            .await
            .is_ok();
    }

    let mut decoder = SseDecoder::new();
    loop {
        tokio::select! {
            _ = route_changed.changed() => {
                let _ = local_tx
                    .send(AxumWsMessage::Close(Some(CloseFrame {
                        code: 1001,
                        reason: "upstream route changed".into(),
                    })))
                    .await;
                return false;
            }
            _ = shutdown_rx.recv() => {
                let _ = local_tx
                    .send(AxumWsMessage::Close(Some(CloseFrame {
                        code: 1001,
                        reason: "proxy stopped".into(),
                    })))
                    .await;
                return false;
            }
            chunk_res = resp.next_chunk() => {
                match chunk_res {
                    Ok(Some(chunk)) => {
                        match decoder.push(&chunk) {
                            Ok(events) => {
                                for ev in events {
                                    if !forward_sse_event_to_ws(local_tx, ev).await {
                                        return false;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = send_ws_error(local_tx, 502, &e).await;
                                return true;
                            }
                        }
                    }
                    Ok(None) => {
                        for ev in decoder.finish() {
                            if !forward_sse_event_to_ws(local_tx, ev).await {
                                return false;
                            }
                        }
                        return true;
                    }
                    Err(e) => {
                        let _ = send_ws_error(local_tx, 502, &e.to_string()).await;
                        return true;
                    }
                }
            }
        }
    }
}

async fn forward_sse_event_to_ws<S>(local_tx: &mut S, ev: SseEvent) -> bool
where
    S: futures_util::Sink<AxumWsMessage> + Unpin,
{
    let trimmed = ev.data.trim();
    if trimmed.is_empty() {
        return true;
    }
    // Preserve [DONE] and raw SSE payloads losslessly without mutating JSON payloads.
    local_tx
        .send(AxumWsMessage::Text(ev.data.into()))
        .await
        .is_ok()
}

async fn send_ws_error<S>(local_tx: &mut S, status: u16, message: &str) -> Result<(), S::Error>
where
    S: futures_util::Sink<AxumWsMessage> + Unpin,
{
    let payload = json!({
        "type": "error",
        "error": {
            "message": message,
            "status": status,
        }
    })
    .to_string();
    local_tx.send(AxumWsMessage::Text(payload.into())).await
}

/// Bridge a local HTTP/SSE request onto an upstream WebSocket (`ws://`/`wss://` or 426 fallback).
#[allow(clippy::too_many_arguments)]
pub async fn handle_http_to_upstream_ws(
    transport: Arc<UpstreamTransport>,
    upstream_url: String,
    prepared_headers: Vec<(HeaderName, HeaderValue)>,
    body_bytes: Bytes,
    outbound_proxy: String,
    active_requests: Arc<AtomicI64>,
    mut route_changed: watch::Receiver<u64>,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Response<Body> {
    let req_guard = ActiveRequestGuard::new(active_requests);
    let Ok(parsed_url) = reqwest::Url::parse(&upstream_url) else {
        return json_error_response(StatusCode::BAD_GATEWAY, "invalid upstream ws url");
    };

    let (wants_sse, outbound_frame) = prepare_http_to_ws_payload(&parsed_url, &prepared_headers, &body_bytes);
    let ws_headers = filter_ws_upgrade_headers(&prepared_headers);

    let mut upstream_conn = match transport
        .connect_websocket(&parsed_url, &ws_headers, &outbound_proxy)
        .await
    {
        UpstreamWsOutcome::Connected(c) => c,
        UpstreamWsOutcome::Rejected {
            status,
            headers,
            body,
        } => {
            return build_axum_response(status, &headers, Body::from(body));
        }
        UpstreamWsOutcome::Unavailable(e) => {
            return json_error_response(
                StatusCode::BAD_GATEWAY,
                &format!("upstream websocket unavailable: {e}"),
            );
        }
    };

    let msg = match std::str::from_utf8(&outbound_frame) {
        Ok(txt) => TungsteniteMessage::Text(txt.into()),
        Err(_) => TungsteniteMessage::Binary(outbound_frame),
    };
    if let Err(e) = upstream_conn.ws.send(msg).await {
        return json_error_response(
            StatusCode::BAD_GATEWAY,
            &format!("upstream websocket write failed: {e}"),
        );
    }

    if wants_sse {
        let (tx, rx) = mpsc::channel::<Result<Bytes, io::Error>>(32);
        tokio::spawn(async move {
            let _req_guard = req_guard;
            let _ws_guard = upstream_conn._guard;
            let mut saw_done = false;
            loop {
                tokio::select! {
                    _ = route_changed.changed() => {
                        let _ = upstream_conn.ws.close(None).await;
                        break;
                    }
                    _ = shutdown_rx.recv() => {
                        let _ = upstream_conn.ws.close(None).await;
                        break;
                    }
                    next = upstream_conn.ws.next() => {
                        match next {
                            Some(Ok(TungsteniteMessage::Text(txt))) => {
                                let trimmed = txt.trim();
                                if trimmed == "[DONE]" {
                                    let _ = tx.send(Ok(Bytes::from_static(b"data: [DONE]\n\n"))).await;
                                    saw_done = true;
                                    break;
                                }
                                let formatted = format_ws_json_as_sse(trimmed);
                                if tx.send(Ok(Bytes::from(formatted))).await.is_err() {
                                    break;
                                }
                                if is_terminal_responses_event(trimmed) {
                                    saw_done = true;
                                    break;
                                }
                            }
                            Some(Ok(TungsteniteMessage::Binary(bin))) => {
                                if let Ok(txt) = std::str::from_utf8(&bin) {
                                    let trimmed = txt.trim();
                                    let formatted = format_ws_json_as_sse(trimmed);
                                    if tx.send(Ok(Bytes::from(formatted))).await.is_err() {
                                        break;
                                    }
                                    if is_terminal_responses_event(trimmed) {
                                        saw_done = true;
                                        break;
                                    }
                                }
                            }
                            Some(Ok(TungsteniteMessage::Ping(p))) => {
                                let _ = upstream_conn.ws.send(TungsteniteMessage::Pong(p)).await;
                            }
                            Some(Ok(TungsteniteMessage::Close(_))) | None => break,
                            Some(Ok(_)) => {}
                            Some(Err(e)) => {
                                let _ = tx.send(Err(io::Error::other(e.to_string()))).await;
                                break;
                            }
                        }
                    }
                }
            }
            if !saw_done {
                let _ = upstream_conn.ws.close(None).await;
            }
        });

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream; charset=utf-8")
            .header("cache-control", "no-cache")
            .body(Body::from_stream(MpscBodyStream(rx)))
            .unwrap_or_else(|_| Response::new(Body::empty()))
    } else {
        // Non-streaming JSON mode over upstream WebSocket.
        let mut final_json: Option<String> = None;
        loop {
            tokio::select! {
                _ = route_changed.changed() => {
                    return json_error_response(StatusCode::SERVICE_UNAVAILABLE, "upstream route changed");
                }
                _ = shutdown_rx.recv() => {
                    return json_error_response(StatusCode::SERVICE_UNAVAILABLE, "proxy stopped");
                }
                next = upstream_conn.ws.next() => {
                    match next {
                        Some(Ok(TungsteniteMessage::Text(txt))) => {
                            let trimmed = txt.trim();
                            if trimmed == "[DONE]" {
                                break;
                            }
                            if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
                                if let Some(ty) = v.get("type").and_then(|t| t.as_str()) {
                                    if ty == "error" {
                                        return Response::builder()
                                            .status(StatusCode::BAD_GATEWAY)
                                            .header("content-type", "application/json")
                                            .body(Body::from(txt.to_string()))
                                            .unwrap_or_else(|_| Response::new(Body::empty()));
                                    }
                                    if matches!(ty, "response.completed" | "response.failed" | "response.incomplete") {
                                        if let Some(resp_obj) = v.get("response") {
                                            final_json = Some(resp_obj.to_string());
                                        } else {
                                            final_json = Some(txt.to_string());
                                        }
                                        break;
                                    }
                                    final_json = Some(txt.to_string());
                                    continue;
                                }
                            }
                            final_json = Some(txt.to_string());
                            break;
                        }
                        Some(Ok(TungsteniteMessage::Ping(p))) => {
                            let _ = upstream_conn.ws.send(TungsteniteMessage::Pong(p)).await;
                        }
                        Some(Ok(TungsteniteMessage::Close(_))) | None => break,
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            return json_error_response(
                                StatusCode::BAD_GATEWAY,
                                &format!("upstream ws read error: {e}"),
                            );
                        }
                    }
                }
            }
        }
        let body = final_json.unwrap_or_else(|| "{}".to_string());
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap_or_else(|_| Response::new(Body::empty()))
    }
}

/// Forward local HTTP/SSE over the default concurrent H3/H2 upstream transport with
/// configured retries, SSE-to-HTTPS automatic fallback, and 426 WS fallback.
#[allow(clippy::too_many_arguments)]
pub async fn handle_http_direct(
    transport: Arc<UpstreamTransport>,
    method: Method,
    upstream_url: String,
    prepared_headers: Vec<(HeaderName, HeaderValue)>,
    body_bytes: Bytes,
    outbound_proxy: String,
    upstream_websocket_enabled: bool,
    retry_policy: RetryPolicy,
    active_requests: Arc<AtomicI64>,
    mut route_changed: watch::Receiver<u64>,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Response<Body> {
    if upstream_url.starts_with("ws://") || upstream_url.starts_with("wss://") {
        return handle_http_to_upstream_ws(
            transport,
            upstream_url,
            prepared_headers,
            body_bytes,
            outbound_proxy,
            active_requests,
            route_changed,
            shutdown_rx,
        )
        .await;
    }

    let req_guard = ActiveRequestGuard::new(Arc::clone(&active_requests));
    let Ok(parsed_url) = reqwest::Url::parse(&upstream_url) else {
        return json_error_response(StatusCode::BAD_GATEWAY, "invalid upstream url");
    };

    let mut clean_headers = sanitize_direct_http_headers(&prepared_headers);
    let wants_sse = request_wants_sse(&clean_headers, &body_bytes);
    let allow_sse_fallback = wants_sse && accepts_json_fallback(&clean_headers);

    let result = send_http_with_retry_and_sse_fallback(
        &transport,
        method,
        parsed_url,
        &mut clean_headers,
        body_bytes.clone(),
        wants_sse,
        allow_sse_fallback,
        &outbound_proxy,
        &retry_policy,
    )
    .await;

    let mut upstream_resp = match result {
        Ok(r) => r,
        Err(msg) => return json_error_response(StatusCode::BAD_GATEWAY, &msg),
    };

    if upstream_resp.status == reqwest::StatusCode::UPGRADE_REQUIRED && upstream_websocket_enabled {
        drop(upstream_resp);
        drop(req_guard);
        return handle_http_to_upstream_ws(
            transport,
            upstream_url,
            prepared_headers,
            body_bytes,
            outbound_proxy,
            active_requests,
            route_changed,
            shutdown_rx,
        )
        .await;
    }

    let status = StatusCode::from_u16(upstream_resp.status.as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let resp_headers = upstream_resp.headers.clone();

    let (tx, rx) = mpsc::channel::<Result<Bytes, io::Error>>(32);
    tokio::spawn(async move {
        let _req_guard = req_guard;
        loop {
            tokio::select! {
                _ = route_changed.changed() => break,
                _ = shutdown_rx.recv() => break,
                chunk = upstream_resp.next_chunk() => {
                    match chunk {
                        Ok(Some(bytes)) => {
                            if !bytes.is_empty() && tx.send(Ok(bytes)).await.is_err() {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            let _ = tx.send(Err(io::Error::other(e.to_string()))).await;
                            break;
                        }
                    }
                }
            }
        }
    });

    build_axum_response(status, &resp_headers, Body::from_stream(MpscBodyStream(rx)))
}

#[allow(clippy::too_many_arguments)]
async fn send_http_with_retry_and_sse_fallback(
    transport: &UpstreamTransport,
    method: Method,
    url: reqwest::Url,
    headers: &mut Vec<(HeaderName, HeaderValue)>,
    body: Bytes,
    tried_sse: bool,
    allow_sse_fallback: bool,
    outbound_proxy: &str,
    retry_policy: &RetryPolicy,
) -> Result<UpstreamHttpResponse, String> {
    let mut current_body = body;
    let total_attempts = retry_policy.attempts();

    for attempt in 0..total_attempts {
        let req = UpstreamHttpRequest {
            method: method.clone(),
            url: url.clone(),
            headers: headers.clone(),
            body: current_body.clone(),
            outbound_proxy: outbound_proxy.to_string(),
        };

        match transport.send_http(req).await {
            Ok(resp) => {
                // Automatic fallback to ordinary HTTPS JSON if upstream explicitly rejects SSE.
                if tried_sse
                    && allow_sse_fallback
                    && is_sse_rejection_status(resp.status)
                {
                    let status = resp.status;
                    let resp_headers = resp.headers.clone();
                    let err_body = resp
                        .collect_body(64 * 1024)
                        .await
                        .unwrap_or_else(|_| Bytes::new());
                    if is_sse_rejection(status, &resp_headers, &err_body) {
                        if let Some(non_stream_body) = force_non_streaming_body(&current_body) {
                            current_body = non_stream_body;
                            set_accept_json(headers);

                            let fallback_req = UpstreamHttpRequest {
                                method: method.clone(),
                                url: url.clone(),
                                headers: headers.clone(),
                                body: current_body,
                                outbound_proxy: outbound_proxy.to_string(),
                            };
                            return transport
                                .send_http(fallback_req)
                                .await
                                .map_err(|e| e.to_string());
                        }
                    }
                    // Not actually an SSE error (or non-JSON body); rebuild response for caller.
                    if retry_policy.should_retry_status(status.as_u16()) && attempt + 1 < total_attempts
                    {
                        if !retry_policy.interval.is_zero() {
                            tokio::time::sleep(retry_policy.interval).await;
                        }
                        continue;
                    }
                    return Ok(rebuild_buffered_response(
                        status,
                        resp_headers,
                        err_body,
                    ));
                }

                if retry_policy.should_retry_status(resp.status.as_u16())
                    && attempt + 1 < total_attempts
                {
                    drop(resp);
                    if !retry_policy.interval.is_zero() {
                        tokio::time::sleep(retry_policy.interval).await;
                    }
                    continue;
                }
                return Ok(resp);
            }
            Err(err) => {
                if attempt + 1 < total_attempts && retry_policy.enabled {
                    if !retry_policy.interval.is_zero() {
                        tokio::time::sleep(retry_policy.interval).await;
                    }
                    continue;
                }
                return Err(err.to_string());
            }
        }
    }

    Err("exhausted upstream retry attempts".to_string())
}

fn rebuild_buffered_response(
    status: reqwest::StatusCode,
    headers: HeaderMap,
    body: Bytes,
) -> UpstreamHttpResponse {
    UpstreamHttpResponse::from_buffered(
        super::upstream_transport::UpstreamProtocol::Http2,
        status,
        headers,
        body,
    )
}

pub fn is_ws_capability_rejection(status: reqwest::StatusCode) -> bool {
    matches!(
        status.as_u16(),
        400 | 404 | 405 | 426 | 501 | 505
    )
}

pub fn prepare_bridge_http_body(raw: &str, default_sse: bool) -> (Bytes, bool) {
    if let Ok(mut v) = serde_json::from_str::<Value>(raw) {
        if let Some(obj) = v.as_object_mut() {
            let mut modified = false;
            if obj.get("type").and_then(|t| t.as_str()) == Some("response.create") {
                obj.remove("type");
                modified = true;
            }
            let wants_sse = match obj.get("stream").and_then(|s| s.as_bool()) {
                Some(b) => b,
                None => {
                    if default_sse {
                        obj.insert("stream".to_string(), Value::Bool(true));
                        modified = true;
                        true
                    } else {
                        false
                    }
                }
            };
            if !modified {
                return (Bytes::copy_from_slice(raw.as_bytes()), wants_sse);
            }
            if let Ok(vec) = serde_json::to_vec(&v) {
                return (Bytes::from(vec), wants_sse);
            }
        }
    }
    (Bytes::copy_from_slice(raw.as_bytes()), default_sse)
}

pub fn force_non_streaming_body(body: &[u8]) -> Option<Bytes> {
    let mut v = serde_json::from_slice::<Value>(body).ok()?;
    let obj = v.as_object_mut()?;
    obj.insert("stream".to_string(), Value::Bool(false));
    serde_json::to_vec(&v).ok().map(Bytes::from)
}

pub fn prepare_http_to_ws_payload(
    url: &reqwest::Url,
    headers: &[(HeaderName, HeaderValue)],
    body: &[u8],
) -> (bool, Bytes) {
    let is_responses_path = url.path().trim_end_matches('/').ends_with("/responses");
    if let Ok(mut v) = serde_json::from_slice::<Value>(body) {
        if let Some(obj) = v.as_object_mut() {
            let wants_sse = obj
                .get("stream")
                .and_then(|s| s.as_bool())
                .unwrap_or_else(|| request_wants_sse(headers, body));
            let mut modified = false;
            if is_responses_path && !obj.contains_key("type") {
                obj.insert(
                    "type".to_string(),
                    Value::String("response.create".to_string()),
                );
                modified = true;
            }
            if modified {
                if let Ok(vec) = serde_json::to_vec(&v) {
                    return (wants_sse, Bytes::from(vec));
                }
            }
            return (wants_sse, Bytes::copy_from_slice(body));
        }
    }
    (request_wants_sse(headers, body), Bytes::copy_from_slice(body))
}

fn format_ws_json_as_sse(trimmed: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Some(ty) = v.get("type").and_then(|t| t.as_str()) {
            return format!("event: {ty}\ndata: {trimmed}\n\n");
        }
    }
    format!("data: {trimmed}\n\n")
}

fn is_terminal_responses_event(trimmed: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Some(ty) = v.get("type").and_then(|t| t.as_str()) {
            return matches!(
                ty,
                "response.completed" | "response.failed" | "response.incomplete" | "error"
            );
        }
    }
    false
}

fn is_sse_rejection_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 400 | 406 | 422 | 501)
}

pub fn is_sse_rejection(
    status: reqwest::StatusCode,
    headers: &HeaderMap,
    body: &[u8],
) -> bool {
    if status == reqwest::StatusCode::NOT_ACCEPTABLE {
        return true;
    }
    if !matches!(status.as_u16(), 400 | 422 | 501) {
        return false;
    }
    let is_sse_resp = header_str(headers, "content-type")
        .map(|ct| ct.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false);
    if is_sse_resp {
        return false;
    }
    let text = String::from_utf8_lossy(body).to_ascii_lowercase();
    (text.contains("stream") || text.contains("sse") || text.contains("text/event-stream"))
        && (text.contains("unsupported")
            || text.contains("not support")
            || text.contains("invalid")
            || text.contains("false")
            || text.contains("not accept")
            || text.contains("disabled"))
}

fn filter_ws_upgrade_headers(headers: &[(HeaderName, HeaderValue)]) -> Vec<(HeaderName, HeaderValue)> {
    headers
        .iter()
        .filter(|(k, _)| {
            let lower = k.as_str();
            !matches!(
                lower,
                "host"
                    | "connection"
                    | "upgrade"
                    | "sec-websocket-key"
                    | "sec-websocket-version"
                    | "sec-websocket-extensions"
                    | "content-length"
                    | "transfer-encoding"
            )
        })
        .cloned()
        .collect()
}

fn sanitize_direct_http_headers(headers: &[(HeaderName, HeaderValue)]) -> Vec<(HeaderName, HeaderValue)> {
    let mut connection_tokens: HashSet<String> = HashSet::new();
    for (k, v) in headers {
        if k.as_str() == "connection" {
            if let Ok(s) = v.to_str() {
                for tok in s.split(',') {
                    let t = tok.trim().to_ascii_lowercase();
                    if !t.is_empty() {
                        connection_tokens.insert(t);
                    }
                }
            }
        }
    }

    headers
        .iter()
        .filter(|(k, _)| {
            let lower = k.as_str();
            !matches!(
                lower,
                "connection"
                    | "keep-alive"
                    | "proxy-connection"
                    | "transfer-encoding"
                    | "upgrade"
                    | "host"
                    | "content-length"
                    | "sec-websocket-key"
                    | "sec-websocket-version"
                    | "sec-websocket-extensions"
                    | "sec-websocket-protocol"
                    | "sec-websocket-accept"
            ) && !connection_tokens.contains(lower)
        })
        .cloned()
        .collect()
}

fn build_bridged_http_headers(
    headers: &[(HeaderName, HeaderValue)],
    wants_sse: bool,
) -> Vec<(HeaderName, HeaderValue)> {
    let mut out: Vec<(HeaderName, HeaderValue)> = sanitize_direct_http_headers(headers)
        .into_iter()
        .filter(|(k, _)| {
            let lower = k.as_str();
            lower != "accept-encoding" && lower != "accept"
        })
        .collect();

    out.push((
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("application/json"),
    ));
    out.push((
        HeaderName::from_static("accept-encoding"),
        HeaderValue::from_static("identity"),
    ));
    set_accept_header(&mut out, wants_sse);
    out
}

fn set_accept_json(headers: &mut Vec<(HeaderName, HeaderValue)>) {
    headers.retain(|(k, _)| k.as_str() != "accept");
    headers.push((
        HeaderName::from_static("accept"),
        HeaderValue::from_static("application/json"),
    ));
}

fn set_accept_header(headers: &mut Vec<(HeaderName, HeaderValue)>, wants_sse: bool) {
    headers.retain(|(k, _)| k.as_str() != "accept");
    let val = if wants_sse {
        HeaderValue::from_static("text/event-stream, application/json")
    } else {
        HeaderValue::from_static("application/json")
    };
    headers.push((HeaderName::from_static("accept"), val));
}

fn request_wants_sse(headers: &[(HeaderName, HeaderValue)], body: &[u8]) -> bool {
    if let Ok(Value::Object(obj)) = serde_json::from_slice::<Value>(body) {
        if let Some(s) = obj.get("stream").and_then(|v| v.as_bool()) {
            return s;
        }
    }
    headers.iter().any(|(k, v)| {
        k.as_str() == "accept"
            && v.to_str()
                .map(|s| s.to_ascii_lowercase().contains("text/event-stream"))
                .unwrap_or(false)
    })
}

fn accepts_json_fallback(headers: &[(HeaderName, HeaderValue)]) -> bool {
    let mut found_accept = false;
    for (k, v) in headers {
        if k.as_str() == "accept" {
            found_accept = true;
            if let Ok(s) = v.to_str() {
                let lower = s.to_ascii_lowercase();
                if lower.contains("application/json") || lower.contains("*/*") {
                    return true;
                }
            }
        }
    }
    !found_accept
}

fn has_unsupported_content_encoding(headers: &HeaderMap) -> bool {
    let Some(enc) = header_str(headers, "content-encoding") else {
        return false;
    };
    let lower = enc.trim().to_ascii_lowercase();
    !lower.is_empty() && lower != "identity"
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn build_axum_response(status: StatusCode, headers: &HeaderMap, body: Body) -> Response<Body> {
    let mut builder = Response::builder().status(status);
    for (name, val) in headers.iter() {
        let lower = name.as_str();
        if matches!(
            lower,
            "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade"
        ) {
            continue;
        }
        builder = builder.header(name.clone(), val.clone());
    }
    builder
        .body(body)
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn json_error_response(status: StatusCode, message: &str) -> Response<Body> {
    let payload = json!({
        "error": {
            "message": message,
            "status": status.as_u16(),
        }
    })
    .to_string();
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(payload))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_decoder_preserves_event_id_retry_and_multiline_data() {
        let mut dec = SseDecoder::new();
        let evs1 = dec
            .push(b"id: 42\nevent: response.delta\nretry: 1500\ndata: {\"a\":1,\n")
            .unwrap();
        assert!(evs1.is_empty());
        let evs2 = dec
            .push(b"data: \"b\":2}\r\n\r\ndata: [DONE]\n\n")
            .unwrap();
        assert_eq!(evs2.len(), 2);
        assert_eq!(evs2[0].event.as_deref(), Some("response.delta"));
        assert_eq!(evs2[0].id.as_deref(), Some("42"));
        assert_eq!(evs2[0].retry, Some(1500));
        assert_eq!(evs2[0].data, "{\"a\":1,\n\"b\":2}");
        assert_eq!(evs2[1].data, "[DONE]");
    }

    #[test]
    fn prepare_bridge_body_preserves_explicit_json_and_strips_response_create() {
        let raw_keep = r#"{"model":"gpt-5","stream":false,"extra":"  preserve  "}"#;
        let (bytes, wants_sse) = prepare_bridge_http_body(raw_keep, true);
        assert!(!wants_sse);
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), raw_keep);

        let raw_ws = r#"{"type":"response.create","model":"gpt-5","input":"hi"}"#;
        let (bytes2, wants_sse2) = prepare_bridge_http_body(raw_ws, true);
        assert!(wants_sse2);
        let parsed: Value = serde_json::from_slice(&bytes2).unwrap();
        assert!(parsed.get("type").is_none());
        assert_eq!(parsed.get("stream").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(parsed.get("model").and_then(|v| v.as_str()), Some("gpt-5"));
    }

    #[test]
    fn sanitize_direct_http_headers_strips_ws_negotiation_headers() {
        let headers = vec![
            (
                HeaderName::from_static("authorization"),
                HeaderValue::from_static("Bearer secret"),
            ),
            (
                HeaderName::from_static("upgrade"),
                HeaderValue::from_static("websocket"),
            ),
            (
                HeaderName::from_static("connection"),
                HeaderValue::from_static("Upgrade"),
            ),
            (
                HeaderName::from_static("sec-websocket-key"),
                HeaderValue::from_static("abc"),
            ),
            (
                HeaderName::from_static("sec-websocket-version"),
                HeaderValue::from_static("13"),
            ),
        ];
        let cleaned = sanitize_direct_http_headers(&headers);
        assert_eq!(cleaned.len(), 1);
        assert_eq!(cleaned[0].0.as_str(), "authorization");
    }
}
