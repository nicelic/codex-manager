use std::collections::{HashMap, HashSet};
use std::future::poll_fn;
use std::io;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock as PoolLock};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bytes::{Buf, Bytes};
use h2::ext::Protocol as H2Protocol;
use h3::client::SendRequest as H3SendRequest;
use http::{HeaderMap, HeaderName, HeaderValue, Method, Request, Uri};
use quinn::crypto::rustls::QuicClientConfig;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls_platform_verifier::BuilderVerifierExt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, Notify, OwnedSemaphorePermit, RwLock, Semaphore};
use tokio_rustls::TlsConnector;
use tokio_socks::tcp::Socks5Stream;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::WebSocketStream;

const MAX_RESPONSE_ERROR_BODY: usize = 64 * 1024;
const MAX_SESSION_STREAMS: usize = 500;

#[derive(Debug, Clone)]
pub struct UpstreamTransportConfig {
    pub h3_grace_period: Duration,
    pub h3_handshake_timeout: Duration,
    pub h3_keep_alive_interval: Duration,
    pub h3_max_idle_timeout: Duration,
    pub h3_max_retries: usize,
    pub h3_cooldown: Duration,
    pub h2_handshake_timeout: Duration,
    pub h2_ping_interval: Duration,
    pub h2_extended_connect_ping_timeout: Duration,
    pub session_stream_limit: usize,
    pub session_lifetime: Duration,
    pub maintenance_interval: Duration,
    pub prewarm_timeout: Duration,
    pub h2_extended_connect_handshake_timeout: Duration,
    pub enable_h3: bool,
    pub extra_root_certs: Vec<CertificateDer<'static>>,
}

impl Default for UpstreamTransportConfig {
    fn default() -> Self {
        Self {
            h3_grace_period: Duration::from_millis(600),
            h3_handshake_timeout: Duration::from_secs(5),
            h3_keep_alive_interval: Duration::from_secs(2),
            h3_max_idle_timeout: Duration::from_secs(5),
            h3_max_retries: 3,
            h3_cooldown: Duration::from_secs(300),
            h2_handshake_timeout: Duration::from_secs(8),
            h2_ping_interval: Duration::from_secs(20),
            h2_extended_connect_ping_timeout: Duration::from_secs(2),
            session_stream_limit: MAX_SESSION_STREAMS,
            session_lifetime: Duration::from_secs(600),
            maintenance_interval: Duration::from_secs(20),
            prewarm_timeout: Duration::from_secs(5),
            h2_extended_connect_handshake_timeout: Duration::from_secs(10),
            enable_h3: true,
            extra_root_certs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamProtocol {
    Http3,
    Http2,
    Http1,
}

impl UpstreamProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Http3 => "http/3",
            Self::Http2 => "http/2",
            Self::Http1 => "http/1.1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamWsKind {
    H2ExtendedConnect,
    H3ExtendedConnect,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpstreamSessionSnapshot {
    pub id: u64,
    pub protocol: String,
    pub occupied_stream_slots: usize,
}

#[derive(Debug, Clone)]
pub struct UpstreamTransportSnapshot {
    pub upstream_h2: usize,
    pub upstream_h2_ws: usize,
    pub upstream_h3: usize,
    pub upstream_h3_ws: usize,
    pub upstream_ws: usize,
    pub upstream_streams: usize,
    pub upstream_stream_limit: usize,
    pub upstream_sessions: Vec<UpstreamSessionSnapshot>,
}

#[derive(Debug, Clone)]
pub struct UpstreamHttpRequest {
    pub method: Method,
    pub url: reqwest::Url,
    pub headers: Vec<(HeaderName, HeaderValue)>,
    pub body: Bytes,
    pub outbound_proxy: String,
}

#[derive(Debug)]
pub enum UpstreamTransportError {
    InvalidRequest(String),
    DialFailed(String),
    PreResponseFailure {
        protocol: UpstreamProtocol,
        detail: String,
    },
    StreamFailure(String),
}

impl std::fmt::Display for UpstreamTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(msg) => write!(f, "invalid upstream request: {msg}"),
            Self::DialFailed(msg) => write!(f, "upstream dial failed: {msg}"),
            Self::PreResponseFailure { protocol, detail } => {
                write!(f, "{} pre-response failure: {detail}", protocol.as_str())
            }
            Self::StreamFailure(msg) => write!(f, "upstream stream error: {msg}"),
        }
    }
}

impl std::error::Error for UpstreamTransportError {}

pub struct SessionStreamGuard {
    active_streams: Arc<AtomicUsize>,
    extra_counter: Option<Arc<AtomicUsize>>,
    _permit: Option<OwnedSemaphorePermit>,
}

impl SessionStreamGuard {
    fn new(
        active_streams: Arc<AtomicUsize>,
        extra_counter: Option<Arc<AtomicUsize>>,
        permit: Option<OwnedSemaphorePermit>,
    ) -> Self {
        active_streams.fetch_add(1, Ordering::Relaxed);
        if let Some(ref c) = extra_counter {
            c.fetch_add(1, Ordering::Relaxed);
        }
        Self { active_streams, extra_counter, _permit: permit }
    }
}

impl Drop for SessionStreamGuard {
    fn drop(&mut self) {
        self.active_streams.fetch_sub(1, Ordering::Relaxed);
        if let Some(ref c) = self.extra_counter {
            c.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

pub enum UpstreamHttpBody {
    H3 {
        _send_req: H3SendRequest<h3_quinn::OpenStreams, Bytes>,
        stream: h3::client::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
        _guard: SessionStreamGuard,
    },
    H2 {
        recv: h2::RecvStream,
        _guard: SessionStreamGuard,
    },
    Http1 {
        resp: reqwest::Response,
        _guard: SessionStreamGuard,
    },
    Buffered(Option<Bytes>),
}

pub struct UpstreamHttpResponse {
    pub protocol: UpstreamProtocol,
    pub status: reqwest::StatusCode,
    pub headers: HeaderMap,
    pub body: UpstreamHttpBody,
}

impl UpstreamHttpResponse {
    pub fn from_buffered(
        protocol: UpstreamProtocol,
        status: reqwest::StatusCode,
        headers: HeaderMap,
        body: Bytes,
    ) -> Self {
        Self {
            protocol,
            status,
            headers,
            body: UpstreamHttpBody::Buffered(if body.is_empty() { None } else { Some(body) }),
        }
    }

    pub async fn next_chunk(&mut self) -> Result<Option<Bytes>, UpstreamTransportError> {
        let result = match &mut self.body {
            UpstreamHttpBody::H3 { stream, .. } => match stream.recv_data().await {
                Ok(Some(mut buf)) => {
                    let bytes = buf.copy_to_bytes(buf.remaining());
                    Ok(Some(bytes))
                }
                Ok(None) => Ok(None),
                Err(e) => Err(UpstreamTransportError::StreamFailure(e.to_string())),
            },
            UpstreamHttpBody::H2 { recv, .. } => match recv.data().await {
                Some(Ok(chunk)) => {
                    let _ = recv.flow_control().release_capacity(chunk.len());
                    Ok(Some(chunk))
                }
                Some(Err(e)) => Err(UpstreamTransportError::StreamFailure(e.to_string())),
                None => Ok(None),
            },
            UpstreamHttpBody::Http1 { resp, .. } => match resp.chunk().await {
                Ok(chunk) => Ok(chunk),
                Err(e) => Err(UpstreamTransportError::StreamFailure(e.to_string())),
            },
            UpstreamHttpBody::Buffered(opt) => Ok(opt.take()),
        };
        if !matches!(result, Ok(Some(_))) {
            self.body = UpstreamHttpBody::Buffered(None);
        }
        result
    }

    pub async fn collect_body(
        mut self,
        max_bytes: usize,
    ) -> Result<Bytes, UpstreamTransportError> {
        let mut out = Vec::new();
        while let Some(chunk) = self.next_chunk().await? {
            let take = chunk.len().min(max_bytes.saturating_sub(out.len()));
            out.extend_from_slice(&chunk[..take]);
            if out.len() >= max_bytes {
                break;
            }
        }
        Ok(Bytes::from(out))
    }
}

pub enum ProxyStream {
    Direct(TcpStream),
    Prefixed {
        inner: TcpStream,
        prefix: Bytes,
    },
}

impl AsyncRead for ProxyStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::Direct(s) => Pin::new(s).poll_read(cx, buf),
            Self::Prefixed { inner, prefix } => {
                if !prefix.is_empty() {
                    let n = prefix.len().min(buf.remaining());
                    buf.put_slice(&prefix[..n]);
                    prefix.advance(n);
                    return Poll::Ready(Ok(()));
                }
                Pin::new(inner).poll_read(cx, buf)
            }
        }
    }
}

impl AsyncWrite for ProxyStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut *self {
            Self::Direct(s) => Pin::new(s).poll_write(cx, data),
            Self::Prefixed { inner, .. } => Pin::new(inner).poll_write(cx, data),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::Direct(s) => Pin::new(s).poll_flush(cx),
            Self::Prefixed { inner, .. } => Pin::new(inner).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::Direct(s) => Pin::new(s).poll_shutdown(cx),
            Self::Prefixed { inner, .. } => Pin::new(inner).poll_shutdown(cx),
        }
    }
}

/// AsyncRead + AsyncWrite adapter over an RFC 8441 HTTP/2 Extended CONNECT stream.
pub struct H2ConnectIo {
    send: h2::SendStream<Bytes>,
    recv: h2::RecvStream,
    read_pending: Bytes,
    shutdown_sent: bool,
    _guard: Option<SessionStreamGuard>,
}

impl H2ConnectIo {
    pub fn new(send: h2::SendStream<Bytes>, recv: h2::RecvStream) -> Self {
        Self {
            send,
            recv,
            read_pending: Bytes::new(),
            shutdown_sent: false,
            _guard: None,
        }
    }
}

impl Drop for H2ConnectIo {
    fn drop(&mut self) {
        // Send END_STREAM before SendStream drops so h2 closes cleanly rather than
        // emitting RST_STREAM(CANCEL) while the peer may still be draining buffered DATA frames.
        if !self.shutdown_sent {
            self.shutdown_sent = true;
            let _ = self.send.send_data(Bytes::new(), true);
        }
    }
}

impl AsyncRead for H2ConnectIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if !self.read_pending.is_empty() {
            let n = self.read_pending.len().min(buf.remaining());
            buf.put_slice(&self.read_pending[..n]);
            self.read_pending.advance(n);
            return Poll::Ready(Ok(()));
        }

        loop {
            match Pin::new(&mut self.recv).poll_data(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(io::Error::other(e))),
                Poll::Ready(Some(Ok(chunk))) => {
                    if chunk.is_empty() {
                        continue;
                    }
                    let _ = self.recv.flow_control().release_capacity(chunk.len());
                    let n = chunk.len().min(buf.remaining());
                    buf.put_slice(&chunk[..n]);
                    if n < chunk.len() {
                        self.read_pending = chunk.slice(n..);
                    }
                    return Poll::Ready(Ok(()));
                }
            }
        }
    }
}

impl AsyncWrite for H2ConnectIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        if data.is_empty() {
            return Poll::Ready(Ok(0));
        }

        if self.send.capacity() == 0 {
            self.send.reserve_capacity(data.len());
            match self.send.poll_capacity(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "h2 extended connect send stream closed",
                    )));
                }
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(io::Error::other(e))),
                Poll::Ready(Some(Ok(0))) => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Poll::Ready(Some(Ok(_))) => {}
            }
        }

        let cap = self.send.capacity();
        if cap == 0 {
            return Poll::Pending;
        }
        let n = data.len().min(cap);
        self.send
            .send_data(Bytes::copy_from_slice(&data[..n]), false)
            .map_err(io::Error::other)?;
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.shutdown_sent {
            self.shutdown_sent = true;
            let _ = self.send.send_data(Bytes::new(), true);
        }
        Poll::Ready(Ok(()))
    }
}

pub enum UpstreamWsIo {
    H2(H2ConnectIo),
    H3(tokio::io::DuplexStream),
}

impl AsyncRead for UpstreamWsIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::H2(io) => Pin::new(io).poll_read(cx, buf),
            Self::H3(io) => Pin::new(io).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for UpstreamWsIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut *self {
            Self::H2(io) => Pin::new(io).poll_write(cx, data),
            Self::H3(io) => Pin::new(io).poll_write(cx, data),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::H2(io) => Pin::new(io).poll_flush(cx),
            Self::H3(io) => Pin::new(io).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::H2(io) => Pin::new(io).poll_shutdown(cx),
            Self::H3(io) => Pin::new(io).poll_shutdown(cx),
        }
    }
}

fn bridge_h3_websocket_stream(
    stream: h3::client::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
    guard: SessionStreamGuard,
) -> tokio::io::DuplexStream {
    let (ws_io, bridge_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        // Keep the slot until the task drops both halves of the actual QUIC stream.
        let _guard = guard;
        let (mut send, mut recv) = stream.split();
        let (mut bridge_read, mut bridge_write) = tokio::io::split(bridge_io);
        let receive = async {
            loop {
                match recv.recv_data().await {
                    Ok(Some(mut chunk)) => {
                        let size = chunk.remaining();
                        bridge_write
                            .write_all(&chunk.copy_to_bytes(size))
                            .await?;
                    }
                    Ok(None) => break,
                    Err(error) => return Err(io::Error::other(error)),
                }
            }
            bridge_write.shutdown().await
        };
        let transmit = async {
            let mut buffer = vec![0; 16 * 1024];
            loop {
                let size = bridge_read.read(&mut buffer).await?;
                if size == 0 {
                    send.finish().await.map_err(io::Error::other)?;
                    break;
                }
                send.send_data(Bytes::copy_from_slice(&buffer[..size]))
                    .await
                    .map_err(io::Error::other)?;
            }
            Ok::<(), io::Error>(())
        };
        tokio::select! {
            _ = receive => {}
            _ = transmit => {}
        }
    });
    ws_io
}

pub struct UpstreamWsConnection {
    pub kind: UpstreamWsKind,
    pub negotiated_protocol: Option<String>,
    pub ws: WebSocketStream<UpstreamWsIo>,
}

pub enum UpstreamWsOutcome {
    Connected(UpstreamWsConnection),
    Rejected {
        status: reqwest::StatusCode,
        headers: HeaderMap,
        body: Bytes,
    },
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SessionKey {
    host: String,
    port: u16,
    proxy: String,
}

impl SessionKey {
    fn new(host: &str, port: u16, proxy: &str) -> Self {
        Self {
            host: host.to_ascii_lowercase(),
            port,
            proxy: proxy.trim().to_string(),
        }
    }

    fn h3_key(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    fn authority(&self) -> String {
        let host = if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        if self.port == 443 {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }
}

struct LiveSessionMark {
    active: AtomicBool,
    counter: Arc<AtomicUsize>,
}

impl LiveSessionMark {
    fn new(counter: Arc<AtomicUsize>) -> Arc<Self> {
        counter.fetch_add(1, Ordering::Relaxed);
        Arc::new(Self {
            active: AtomicBool::new(true),
            counter,
        })
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn deactivate(&self) {
        if self.active.swap(false, Ordering::AcqRel) {
            self.counter.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

impl Drop for LiveSessionMark {
    fn drop(&mut self) {
        self.deactivate();
    }
}

#[derive(Clone)]
struct H3SessionEntry {
    id: u64,
    created_at: Instant,
    conn: quinn::Connection,
    send_req: H3SendRequest<h3_quinn::OpenStreams, Bytes>,
    mark: Arc<LiveSessionMark>,
    stream_slots: Arc<Semaphore>,
}

impl H3SessionEntry {
    fn is_usable(&self, lifetime: Duration) -> bool {
        self.mark.is_active() && self.conn.close_reason().is_none() && self.created_at.elapsed() < lifetime
    }
}

#[derive(Clone)]
struct H2SessionEntry {
    id: u64,
    created_at: Instant,
    send_req: h2::client::SendRequest<Bytes>,
    ping_pong: Arc<Mutex<Option<h2::PingPong>>>,
    mark: Arc<LiveSessionMark>,
    stream_slots: Arc<Semaphore>,
    ws_enabled: bool,
}

impl H2SessionEntry {
    fn is_usable(&self, lifetime: Duration) -> bool {
        self.mark.is_active() && self.created_at.elapsed() < lifetime
    }
}

enum SelectedSession {
    H3(H3SessionEntry, OwnedSemaphorePermit),
    H2(H2SessionEntry, OwnedSemaphorePermit),
}

struct QuicEndpoints {
    v4: Option<quinn::Endpoint>,
    v6: Option<quinn::Endpoint>,
}

struct TransportInner {
    config: UpstreamTransportConfig,
    next_id: AtomicU64,
    h3_sessions: PoolLock<HashMap<SessionKey, Vec<H3SessionEntry>>>,
    h2_sessions: PoolLock<HashMap<SessionKey, Vec<H2SessionEntry>>>,
    h3_sessions_active: Arc<AtomicUsize>,
    h2_sessions_active: Arc<AtomicUsize>,
    h3_cooldowns: RwLock<HashMap<String, Instant>>,
    active_streams: Arc<AtomicUsize>,
    h2_ws_active: Arc<AtomicUsize>,
    h3_ws_active: Arc<AtomicUsize>,
    dial_locks: Mutex<HashMap<SessionKey, Arc<Mutex<()>>>>,
    quic_endpoints: Mutex<Option<QuicEndpoints>>,
    shutdown_notify: Notify,
    stopped: AtomicBool,
}

pub struct UpstreamTransport {
    inner: Arc<TransportInner>,
}

impl Default for UpstreamTransport {
    fn default() -> Self {
        Self::new(UpstreamTransportConfig::default())
    }
}

impl Drop for UpstreamTransport {
    fn drop(&mut self) {
        self.inner.stopped.store(true, Ordering::Relaxed);
        self.inner.shutdown_notify.notify_waiters();
    }
}

impl UpstreamTransport {
    pub fn new(mut config: UpstreamTransportConfig) -> Self {
        config.session_stream_limit = config.session_stream_limit.clamp(1, MAX_SESSION_STREAMS);
        let inner = Arc::new(TransportInner {
            config,
            next_id: AtomicU64::new(1),
            h3_sessions: PoolLock::new(HashMap::new()),
            h2_sessions: PoolLock::new(HashMap::new()),
            h3_sessions_active: Arc::new(AtomicUsize::new(0)),
            h2_sessions_active: Arc::new(AtomicUsize::new(0)),
            h3_cooldowns: RwLock::new(HashMap::new()),
            active_streams: Arc::new(AtomicUsize::new(0)),
            h2_ws_active: Arc::new(AtomicUsize::new(0)),
            h3_ws_active: Arc::new(AtomicUsize::new(0)),
            dial_locks: Mutex::new(HashMap::new()),
            quic_endpoints: Mutex::new(None),
            shutdown_notify: Notify::new(),
            stopped: AtomicBool::new(false),
        });

        let weak = Arc::downgrade(&inner);
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                while let Some(inner) = weak.upgrade() {
                    if inner.stopped.load(Ordering::Relaxed) {
                        break;
                    }
                    let interval = inner.config.maintenance_interval;
                    tokio::select! {
                        _ = tokio::time::sleep(interval) => {}
                        _ = inner.shutdown_notify.notified() => {
                            if inner.stopped.load(Ordering::Relaxed) {
                                break;
                            }
                            continue;
                        }
                    }
                    if inner.stopped.load(Ordering::Relaxed) {
                        break;
                    }
                    inner.run_maintenance().await;
                }
            });
        }

        Self { inner }
    }

    pub fn config(&self) -> &UpstreamTransportConfig {
        &self.inner.config
    }

    pub fn snapshot(&self) -> UpstreamTransportSnapshot {
        let h2_ws = self.inner.h2_ws_active.load(Ordering::Relaxed);
        let h3_ws = self.inner.h3_ws_active.load(Ordering::Relaxed);

        let stream_limit = self.inner.config.session_stream_limit;
        let mut upstream_sessions = Vec::new();
        {
            let pool = self.inner.h2_sessions.read().unwrap();
            upstream_sessions.extend(pool.values().flatten().filter(|s| s.mark.is_active()).map(|s| {
                UpstreamSessionSnapshot {
                    id: s.id,
                    protocol: UpstreamProtocol::Http2.as_str().to_owned(),
                    occupied_stream_slots: stream_limit - s.stream_slots.available_permits(),
                }
            }));
        }
        {
            let pool = self.inner.h3_sessions.read().unwrap();
            upstream_sessions.extend(pool.values().flatten().filter(|s| s.mark.is_active()).map(|s| {
                UpstreamSessionSnapshot {
                    id: s.id,
                    protocol: UpstreamProtocol::Http3.as_str().to_owned(),
                    occupied_stream_slots: stream_limit - s.stream_slots.available_permits(),
                }
            }));
        }
        upstream_sessions.sort_unstable_by_key(|s| s.id);

        UpstreamTransportSnapshot {
            upstream_h2: self.inner.h2_sessions_active.load(Ordering::Relaxed),
            upstream_h2_ws: h2_ws,
            upstream_h3: self.inner.h3_sessions_active.load(Ordering::Relaxed),
            upstream_h3_ws: h3_ws,
            upstream_ws: h2_ws + h3_ws,
            upstream_streams: self.inner.active_streams.load(Ordering::Relaxed),
            upstream_stream_limit: stream_limit,
            upstream_sessions,
        }
    }

    pub async fn close_all(&self) {
        {
            let mut h3_map = self.inner.h3_sessions.write().unwrap();
            for (_, sessions) in h3_map.drain() {
                for session in sessions {
                    session.mark.deactivate();
                    session.conn.close(0u32.into(), b"transport closed");
                }
            }
        }
        {
            let mut h2_map = self.inner.h2_sessions.write().unwrap();
            for (_, sessions) in h2_map.drain() {
                for session in sessions {
                    session.mark.deactivate();
                }
            }
        }
        self.inner.h3_cooldowns.write().await.clear();
    }

    pub async fn is_h3_cooled_down(&self, host: &str, port: u16) -> bool {
        let key = format!("{}:{port}", host.to_ascii_lowercase());
        self.inner.is_h3_cooled_down(&key).await
    }

    /// Prewarm upstream sessions concurrently (HTTPS) or perform plain HTTP probe when `http://`.
    pub async fn prewarm(
        &self,
        probe_url: &str,
        outbound_proxy: &str,
    ) -> Result<UpstreamProtocol, UpstreamTransportError> {
        let parsed = reqwest::Url::parse(probe_url)
            .map_err(|e| UpstreamTransportError::InvalidRequest(e.to_string()))?;
        match parsed.scheme() {
            "https" | "wss" => {
                let host = parsed
                    .host_str()
                    .ok_or_else(|| UpstreamTransportError::InvalidRequest("missing host".into()))?;
                let port = parsed.port_or_known_default().unwrap_or(443);
                let key = SessionKey::new(host, port, outbound_proxy);
                match tokio::time::timeout(
                    self.inner.config.prewarm_timeout,
                    self.inner.acquire_or_race(&key),
                )
                .await
                {
                    Ok(Ok(SelectedSession::H3(_, _))) => Ok(UpstreamProtocol::Http3),
                    Ok(Ok(SelectedSession::H2(_, _))) => Ok(UpstreamProtocol::Http2),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err(UpstreamTransportError::DialFailed(format!(
                        "prewarm timed out after {:?}",
                        self.inner.config.prewarm_timeout
                    ))),
                }
            }
            "http" | "ws" => {
                let http_url = if parsed.scheme() == "ws" {
                    let mut u = parsed.clone();
                    let _ = u.set_scheme("http");
                    u
                } else {
                    parsed
                };
                let client = build_plain_reqwest_client(
                    outbound_proxy,
                    self.inner.config.prewarm_timeout,
                    &self.inner.config.extra_root_certs,
                )?;
                client
                    .get(http_url)
                    .send()
                    .await
                    .map_err(|e| UpstreamTransportError::DialFailed(e.to_string()))?;
                Ok(UpstreamProtocol::Http1)
            }
            other => Err(UpstreamTransportError::InvalidRequest(format!(
                "unsupported prewarm scheme: {other}"
            ))),
        }
    }

    /// Execute an HTTP/SSE request over H3 (preferred) / H2 with 600ms H3 grace period,
    /// QUIC keepalive vs dead-link 5s detection, and up to 3 H3 retries before H2 fallback.
    pub async fn send_http(
        &self,
        req: UpstreamHttpRequest,
    ) -> Result<UpstreamHttpResponse, UpstreamTransportError> {
        if req.url.scheme() == "http" {
            return self.send_plain_http(req).await;
        }
        if req.url.scheme() != "https" {
            return Err(UpstreamTransportError::InvalidRequest(format!(
                "expected https/http url, got {}",
                req.url.scheme()
            )));
        }

        let host = req
            .url
            .host_str()
            .ok_or_else(|| UpstreamTransportError::InvalidRequest("missing host".into()))?;
        let port = req.url.port_or_known_default().unwrap_or(443);
        let key = SessionKey::new(host, port, &req.outbound_proxy);

        let selected = self.inner.acquire_or_race(&key).await?;
        match selected {
            SelectedSession::H3(initial_h3, permit) => {
                match self
                    .inner
                    .send_h3_with_retry_then_h2(&key, initial_h3, permit, &req)
                    .await
                {
                    Ok(resp) => Ok(resp),
                    Err(e) => Err(e),
                }
            }
            SelectedSession::H2(initial_h2, permit) => self.inner.send_h2_with_reconnect(&key, initial_h2, permit, &req).await,
        }
    }

    async fn send_plain_http(
        &self,
        req: UpstreamHttpRequest,
    ) -> Result<UpstreamHttpResponse, UpstreamTransportError> {
        let client = build_plain_reqwest_client(
            &req.outbound_proxy,
            Duration::from_secs(30),
            &self.inner.config.extra_root_certs,
        )?;
        let reqwest_method = reqwest::Method::from_bytes(req.method.as_str().as_bytes())
            .map_err(|e| UpstreamTransportError::InvalidRequest(e.to_string()))?;
        let mut builder = client.request(reqwest_method, req.url);
        for (k, v) in &req.headers {
            let lower = k.as_str();
            if is_hop_by_hop_header(lower) || lower == "host" || lower == "content-length" {
                continue;
            }
            builder = builder.header(k.as_str(), v.as_bytes());
        }
        if !req.body.is_empty() {
            builder = builder.body(req.body);
        }
        let resp = builder
            .send()
            .await
            .map_err(|e| UpstreamTransportError::PreResponseFailure {
                protocol: UpstreamProtocol::Http1,
                detail: e.to_string(),
            })?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let guard = SessionStreamGuard::new(Arc::clone(&self.inner.active_streams), None, None);
        Ok(UpstreamHttpResponse {
            protocol: UpstreamProtocol::Http1,
            status,
            headers,
            body: UpstreamHttpBody::Http1 {
                resp,
                _guard: guard,
            },
        })
    }

    /// Connect using RFC 8441 (H2) or RFC 9220 (H3) Extended CONNECT over TLS.
    /// The normal H3/H2 race selects the transport; if H3 cannot establish WS,
    /// retry the Extended CONNECT on H2 before reporting it unavailable.
    pub async fn connect_websocket(
        &self,
        target_url: &reqwest::Url,
        extra_headers: &[(HeaderName, HeaderValue)],
        outbound_proxy: &str,
    ) -> UpstreamWsOutcome {
        if target_url.scheme() != "https" {
            return UpstreamWsOutcome::Unavailable(
                "upstream WebSocket requires an HTTPS URL and H2/H3 Extended CONNECT".into(),
            );
        }
        let Some(host) = target_url.host_str() else {
            return UpstreamWsOutcome::Unavailable("missing target host".into());
        };
        let port = target_url.port_or_known_default().unwrap_or(443);
        let key = SessionKey::new(host, port, outbound_proxy);
        let selected = match self.inner.acquire_or_race(&key).await {
            Ok(session) => session,
            Err(error) => return UpstreamWsOutcome::Unavailable(error.to_string()),
        };

        match selected {
            SelectedSession::H2(session, permit) => {
                self.inner
                    .connect_h2_extended_connect(&key, &session, permit, target_url, extra_headers)
                    .await
            }
            SelectedSession::H3(session, permit) => {
                match self.inner.connect_h3_extended_connect(&key, &session, permit, target_url, extra_headers).await {
                    connected @ UpstreamWsOutcome::Connected(_) => connected,
                    h3_outcome => {
                        let (h2, h2_permit) = match self.inner.acquire_h2_session(&key).await {
                            Ok(session) => session,
                            Err(error) => {
                                return match h3_outcome {
                                    UpstreamWsOutcome::Rejected { status, headers, body }
                                        if !matches!(status, reqwest::StatusCode::BAD_REQUEST | reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED | reqwest::StatusCode::UPGRADE_REQUIRED | reqwest::StatusCode::NOT_IMPLEMENTED | reqwest::StatusCode::HTTP_VERSION_NOT_SUPPORTED) => {
                                            UpstreamWsOutcome::Rejected { status, headers, body }
                                        }
                                    _ => UpstreamWsOutcome::Unavailable(format!(
                                        "H3 WebSocket failed; H2 fallback failed: {error}"
                                    )),
                                };
                            }
                        };
                        match self.inner.connect_h2_extended_connect(&key, &h2, h2_permit, target_url, extra_headers).await {
                            connected @ UpstreamWsOutcome::Connected(_) => connected,
                            h2_outcome => match h3_outcome {
                                rejected @ UpstreamWsOutcome::Rejected { status, .. }
                                    if !matches!(status, reqwest::StatusCode::BAD_REQUEST | reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED | reqwest::StatusCode::UPGRADE_REQUIRED | reqwest::StatusCode::NOT_IMPLEMENTED | reqwest::StatusCode::HTTP_VERSION_NOT_SUPPORTED) => rejected,
                                _ => h2_outcome,
                            },
                        }
                    }
                }
            }
        }
    }
}

impl TransportInner {
    async fn run_maintenance(&self) {
        let lifetime = self.config.session_lifetime;
        let stream_limit = self.config.session_stream_limit;
        let now = Instant::now();
        let h3_cooldowns = self.h3_cooldowns.read().await;
        let mut preferred_h3 = HashSet::new();

        {
            let mut h3_map = self.h3_sessions.write().unwrap();
            for (key, sessions) in h3_map.iter_mut() {
                let mut kept_primary = false;
                sessions.retain(|session| {
                    let idle = session.stream_slots.available_permits() == stream_limit;
                    let remove = !session.mark.is_active()
                        || session.conn.close_reason().is_some()
                        || (idle && (kept_primary || session.created_at.elapsed() >= lifetime));
                    kept_primary |= !remove;

                    if remove {
                        session.mark.deactivate();
                        session.conn.close(0u32.into(), b"session expired");
                    }
                    !remove
                });
                if !h3_cooldowns.get(&key.h3_key()).is_some_and(|until| *until > now)
                    && sessions.iter().any(|session| session.is_usable(lifetime))
                {
                    preferred_h3.insert(key.clone());
                }
            }
            h3_map.retain(|_, sessions| !sessions.is_empty());
        }
        drop(h3_cooldowns);

        let idle_pings: Vec<(SessionKey, H2SessionEntry)> = {
            let mut h2_map = self.h2_sessions.write().unwrap();
            let mut to_ping = Vec::new();
            for (key, sessions) in h2_map.iter_mut() {
                let mut kept_primary = preferred_h3.contains(key);
                sessions.retain(|session| {
                    let idle = session.stream_slots.available_permits() == stream_limit;
                    let remove = !session.mark.is_active()
                        || (idle && (kept_primary || session.created_at.elapsed() >= lifetime));
                    kept_primary |= !remove;

                    if remove {
                        session.mark.deactivate();
                    } else {
                        to_ping.push((key.clone(), session.clone()));
                    }
                    !remove
                });
            }
            h2_map.retain(|_, sessions| !sessions.is_empty());
            to_ping
        };

        let ping_timeout = self.config.h2_ping_interval.min(Duration::from_secs(5));
        for (key, session) in idle_pings {
            let mut guard = session.ping_pong.lock().await;
            if let Some(pp) = guard.as_mut() {
                let res = tokio::time::timeout(ping_timeout, pp.ping(h2::Ping::opaque())).await;
                if !matches!(res, Ok(Ok(_))) {
                    session.mark.deactivate();
                    drop(guard);
                    self.remove_h2_session(&key, session.id).await;
                }
            }
        }

        let now = Instant::now();
        self.h3_cooldowns.write().await.retain(|_, until| *until > now);
    }

    async fn is_h3_cooled_down(&self, h3_key: &str) -> bool {
        let now = Instant::now();
        let read = self.h3_cooldowns.read().await;
        match read.get(h3_key) {
            Some(until) => *until > now,
            None => false,
        }
    }

    async fn set_h3_cooldown(&self, h3_key: &str) {
        if self.config.h3_cooldown.is_zero() {
            return;
        }
        let until = Instant::now() + self.config.h3_cooldown;
        self.h3_cooldowns
            .write()
            .await
            .insert(h3_key.to_string(), until);
    }

    async fn store_h3_session(&self, key: SessionKey, entry: H3SessionEntry) {
        self.h3_sessions.write().unwrap().entry(key).or_default().push(entry);
    }

    async fn store_h2_session(&self, key: SessionKey, entry: H2SessionEntry) {
        self.h2_sessions.write().unwrap().entry(key).or_default().push(entry);
    }

    async fn remove_h3_session(&self, key: &SessionKey, expected_id: u64) {
        let mut guard = self.h3_sessions.write().unwrap();
        if let Some(sessions) = guard.get_mut(key) {
            if let Some(index) = sessions.iter().position(|session| session.id == expected_id) {
                let session = sessions.remove(index);
                session.mark.deactivate();
                session.conn.close(0u32.into(), b"reset");
            }
            if sessions.is_empty() { guard.remove(key); }
        }
    }

    async fn remove_h2_session(&self, key: &SessionKey, expected_id: u64) {
        let mut guard = self.h2_sessions.write().unwrap();
        if let Some(sessions) = guard.get_mut(key) {
            if let Some(index) = sessions.iter().position(|session| session.id == expected_id) {
                sessions.remove(index).mark.deactivate();
            }
            if sessions.is_empty() { guard.remove(key); }
        }
    }

    async fn reserve_h3_session(&self, key: &SessionKey) -> Option<(H3SessionEntry, OwnedSemaphorePermit)> {
        let pool = self.h3_sessions.read().unwrap();
        pool.get(key)?.iter().find_map(|session| {
            if !session.is_usable(self.config.session_lifetime) {
                return None;
            }
            let permit = Arc::clone(&session.stream_slots).try_acquire_owned().ok()?;
            Some((session.clone(), permit))
        })
    }

    async fn reserve_h2_session(&self, key: &SessionKey) -> Option<(H2SessionEntry, OwnedSemaphorePermit)> {
        let pool = self.h2_sessions.read().unwrap();
        pool.get(key)?.iter().find_map(|session| {
            if !session.is_usable(self.config.session_lifetime) {
                return None;
            }
            let permit = Arc::clone(&session.stream_slots).try_acquire_owned().ok()?;
            Some((session.clone(), permit))
        })
    }

    async fn acquire_dial_lock(&self, key: &SessionKey) -> Arc<Mutex<()>> {
        let mut locks = self.dial_locks.lock().await;
        locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn h3_allowed(&self, key: &SessionKey) -> bool {
        // HTTP/3 uses UDP; skip H3 whenever an explicit TCP HTTP/SOCKS outbound proxy is set.
        self.config.enable_h3 && key.proxy.is_empty()
    }

    async fn acquire_or_race(&self, key: &SessionKey) -> Result<SelectedSession, UpstreamTransportError> {
        let h3_allowed = self.h3_allowed(key) && !self.is_h3_cooled_down(&key.h3_key()).await;
        if h3_allowed {
            if let Some((session, permit)) = self.reserve_h3_session(key).await { return Ok(SelectedSession::H3(session, permit)); }
        }
        if let Some((session, permit)) = self.reserve_h2_session(key).await {
            return Ok(SelectedSession::H2(session, permit));
        }

        let lock = self.acquire_dial_lock(key).await;
        let _guard = lock.lock().await;
        let h3_allowed = self.h3_allowed(key) && !self.is_h3_cooled_down(&key.h3_key()).await;
        if h3_allowed {
            if let Some((session, permit)) = self.reserve_h3_session(key).await { return Ok(SelectedSession::H3(session, permit)); }
        }
        if let Some((session, permit)) = self.reserve_h2_session(key).await {
            return Ok(SelectedSession::H2(session, permit));
        }

        if !h3_allowed {
            let h2 = self.dial_h2(key).await?;
            return Ok(self.select_new_h2(key, h2).await);
        }

        // Give H3 a head start. If it is still dialing when the window expires,
        // H2 starts while H3 continues. An early H3 failure starts H2 immediately.
        let mut h3_dial = Box::pin(self.dial_h3(key));
        let h3_result = match tokio::time::timeout(self.config.h3_grace_period, &mut h3_dial).await {
            Ok(Ok(h3)) => return Ok(self.select_new_h3(key, h3).await),
            Ok(Err(error)) => Some(error),
            Err(_) => None,
        };

        let mut h2_dial = Box::pin(self.dial_h2(key));
        let dial_error = |h3, h2| {
            UpstreamTransportError::DialFailed(format!("H3 failed ({h3}); H2 failed ({h2})"))
        };
        if let Some(h3_error) = h3_result {
            return match h2_dial.await {
                Ok(h2) => Ok(self.select_new_h2(key, h2).await),
                Err(h2_error) => Err(dial_error(h3_error, h2_error)),
            };
        }

        tokio::select! {
            biased;
            result = &mut h3_dial => match result {
                Ok(h3) => Ok(self.select_new_h3(key, h3).await),
                Err(h3_error) => match h2_dial.await {
                    Ok(h2) => Ok(self.select_new_h2(key, h2).await),
                    Err(h2_error) => Err(dial_error(h3_error, h2_error)),
                },
            },
            result = &mut h2_dial => match result {
                Ok(h2) => Ok(self.select_new_h2(key, h2).await),
                Err(h2_error) => match h3_dial.await {
                    Ok(h3) => Ok(self.select_new_h3(key, h3).await),
                    Err(h3_error) => Err(dial_error(h3_error, h2_error)),
                },
            },
        }
    }

    async fn select_new_h3(&self, key: &SessionKey, session: H3SessionEntry) -> SelectedSession {
        let permit = Arc::clone(&session.stream_slots)
            .try_acquire_owned()
            .expect("new H3 session has capacity");
        self.store_h3_session(key.clone(), session.clone()).await;
        SelectedSession::H3(session, permit)
    }

    async fn select_new_h2(&self, key: &SessionKey, session: H2SessionEntry) -> SelectedSession {
        let permit = Arc::clone(&session.stream_slots)
            .try_acquire_owned()
            .expect("new H2 session has capacity");
        self.store_h2_session(key.clone(), session.clone()).await;
        SelectedSession::H2(session, permit)
    }

    async fn acquire_h2_session(&self, key: &SessionKey) -> Result<(H2SessionEntry, OwnedSemaphorePermit), UpstreamTransportError> {
        if let Some(reserved) = self.reserve_h2_session(key).await { return Ok(reserved); }
        let lock = self.acquire_dial_lock(key).await;
        let _guard = lock.lock().await;
        if let Some(reserved) = self.reserve_h2_session(key).await { return Ok(reserved); }
        let h2 = self.dial_h2(key).await?;
        let permit = Arc::clone(&h2.stream_slots).try_acquire_owned().expect("new H2 session has capacity");
        self.store_h2_session(key.clone(), h2.clone()).await;
        Ok((h2, permit))
    }

    async fn send_h3_with_retry_then_h2(
        &self, key: &SessionKey, initial_h3: H3SessionEntry, initial_permit: OwnedSemaphorePermit,
        req: &UpstreamHttpRequest,
    ) -> Result<UpstreamHttpResponse, UpstreamTransportError> {
        let mut current = Some((initial_h3, initial_permit));
        let mut last_err = None;
        for attempt in 0..=self.config.h3_max_retries {
            let (session, permit) = match current.take() {
                Some(pair) => pair,
                None => {
                    if let Some(pair) = self.reserve_h3_session(key).await { pair } else {
                        let lock = self.acquire_dial_lock(key).await;
                        let _guard = lock.lock().await;
                        if let Some(pair) = self.reserve_h3_session(key).await { pair } else {
                            match self.dial_h3(key).await {
                                Ok(s) => {
                                    let permit = Arc::clone(&s.stream_slots).try_acquire_owned().expect("new H3 session has capacity");
                                    self.store_h3_session(key.clone(), s.clone()).await;
                                    (s, permit)
                                }
                                Err(error) => { last_err = Some(error); continue; }
                            }
                        }
                    }
                }
            };
            match self.send_on_h3_session(key, &session, permit, req).await {
                Ok(response) => return Ok(response),
                Err(error) => {
                    self.remove_h3_session(key, session.id).await;
                    last_err = Some(error);
                    if attempt == self.config.h3_max_retries { break; }
                }
            }
        }
        self.set_h3_cooldown(&key.h3_key()).await;
        match self.acquire_h2_session(key).await {
            Ok((h2, permit)) => self.send_h2_with_reconnect(key, h2, permit, req).await,
            Err(error) => Err(last_err.unwrap_or(error)),
        }
    }

    async fn send_on_h3_session(
        &self,
        key: &SessionKey,
        session: &H3SessionEntry,
        permit: OwnedSemaphorePermit,
        req: &UpstreamHttpRequest,
    ) -> Result<UpstreamHttpResponse, UpstreamTransportError> {
        let uri = build_target_uri(&req.url, &key.authority())?;
        let mut builder = Request::builder().method(req.method.clone()).uri(uri);
        for (name, val) in sanitize_h2_h3_headers(&req.headers) {
            builder = builder.header(name, val);
        }
        let h3_req = builder
            .body(())
            .map_err(|e| UpstreamTransportError::InvalidRequest(e.to_string()))?;

        let guard = SessionStreamGuard::new(
            Arc::clone(&self.active_streams),
            None,
            Some(permit),
        );

        let mut send_req = session.send_req.clone();
        // Bound opening & sending headers/body; do NOT apply a short HTTP response timeout
        // on `recv_response()` so slow-thinking AI models survive via QUIC 2s keepalive PINGs
        // while dead links fail within `h3_max_idle_timeout` (5s).
        let open_timeout = self.config.h3_handshake_timeout;
        let mut stream = tokio::time::timeout(open_timeout, send_req.send_request(h3_req))
            .await
            .map_err(|_| UpstreamTransportError::PreResponseFailure {
                protocol: UpstreamProtocol::Http3,
                detail: "H3 stream open timed out".into(),
            })?
            .map_err(|e| UpstreamTransportError::PreResponseFailure {
                protocol: UpstreamProtocol::Http3,
                detail: e.to_string(),
            })?;

        if !req.body.is_empty() {
            tokio::time::timeout(open_timeout, stream.send_data(req.body.clone()))
                .await
                .map_err(|_| UpstreamTransportError::PreResponseFailure {
                    protocol: UpstreamProtocol::Http3,
                    detail: "H3 send_data timed out".into(),
                })?
                .map_err(|e| UpstreamTransportError::PreResponseFailure {
                    protocol: UpstreamProtocol::Http3,
                    detail: e.to_string(),
                })?;
        }

        tokio::time::timeout(open_timeout, stream.finish())
            .await
            .map_err(|_| UpstreamTransportError::PreResponseFailure {
                protocol: UpstreamProtocol::Http3,
                detail: "H3 finish timed out".into(),
            })?
            .map_err(|e| UpstreamTransportError::PreResponseFailure {
                protocol: UpstreamProtocol::Http3,
                detail: e.to_string(),
            })?;

        let resp = stream.recv_response().await.map_err(|e| {
            UpstreamTransportError::PreResponseFailure {
                protocol: UpstreamProtocol::Http3,
                detail: e.to_string(),
            }
        })?;

        let status = reqwest::StatusCode::from_u16(resp.status().as_u16())
            .unwrap_or(reqwest::StatusCode::INTERNAL_SERVER_ERROR);
        let headers = resp.headers().clone();

        Ok(UpstreamHttpResponse {
            protocol: UpstreamProtocol::Http3,
            status,
            headers,
            body: UpstreamHttpBody::H3 {
                _send_req: send_req,
                stream,
                _guard: guard,
            },
        })
    }

    async fn send_h2_with_reconnect(
        &self, key: &SessionKey, initial_h2: H2SessionEntry, initial_permit: OwnedSemaphorePermit,
        req: &UpstreamHttpRequest,
    ) -> Result<UpstreamHttpResponse, UpstreamTransportError> {
        match self.send_on_h2_session(key, &initial_h2, initial_permit, req).await {
            Ok(resp) => Ok(resp),
            Err(first_err) => {
                self.remove_h2_session(key, initial_h2.id).await;
                if matches!(first_err, UpstreamTransportError::InvalidRequest(_)) { return Err(first_err); }
                let (fresh, permit) = self.acquire_h2_session(key).await?;
                match self.send_on_h2_session(key, &fresh, permit, req).await {
                    Ok(resp) => Ok(resp),
                    Err(err) => { self.remove_h2_session(key, fresh.id).await; Err(err) }
                }
            }
        }
    }

    async fn send_on_h2_session(
        &self,
        key: &SessionKey,
        session: &H2SessionEntry,
        permit: OwnedSemaphorePermit,
        req: &UpstreamHttpRequest,
    ) -> Result<UpstreamHttpResponse, UpstreamTransportError> {
        let uri = build_target_uri(&req.url, &key.authority())?;
        let mut builder = Request::builder()
            .method(req.method.clone())
            .uri(uri)
            .version(http::Version::HTTP_2);
        for (name, val) in sanitize_h2_h3_headers(&req.headers) {
            builder = builder.header(name, val);
        }
        let h2_req = builder
            .body(())
            .map_err(|e| UpstreamTransportError::InvalidRequest(e.to_string()))?;

        let guard = SessionStreamGuard::new(
            Arc::clone(&self.active_streams),
            None,
            Some(permit),
        );

        let send_req = session.send_req.clone();
        let mut ready = send_req.ready().await.map_err(|e| {
            UpstreamTransportError::PreResponseFailure {
                protocol: UpstreamProtocol::Http2,
                detail: e.to_string(),
            }
        })?;

        let end_of_stream = req.body.is_empty();
        let (resp_fut, mut send_stream) = ready
            .send_request(h2_req, end_of_stream)
            .map_err(|e| UpstreamTransportError::PreResponseFailure {
                protocol: UpstreamProtocol::Http2,
                detail: e.to_string(),
            })?;

        if !end_of_stream {
            send_h2_body(&mut send_stream, req.body.clone()).await?;
        }

        let resp = resp_fut.await.map_err(|e| {
            UpstreamTransportError::PreResponseFailure {
                protocol: UpstreamProtocol::Http2,
                detail: e.to_string(),
            }
        })?;

        let status = reqwest::StatusCode::from_u16(resp.status().as_u16())
            .unwrap_or(reqwest::StatusCode::INTERNAL_SERVER_ERROR);
        let headers = resp.headers().clone();
        let recv = resp.into_body();

        Ok(UpstreamHttpResponse {
            protocol: UpstreamProtocol::Http2,
            status,
            headers,
            body: UpstreamHttpBody::H2 {
                recv,
                _guard: guard,
            },
        })
    }

    async fn connect_h2_extended_connect(
        &self,
        key: &SessionKey,
        session: &H2SessionEntry,
        permit: OwnedSemaphorePermit,
        target_url: &reqwest::Url,
        extra_headers: &[(HeaderName, HeaderValue)],
    ) -> UpstreamWsOutcome {
        if !session.ws_enabled {
            return UpstreamWsOutcome::Unavailable(
                "upstream H2 did not advertise SETTINGS_ENABLE_CONNECT_PROTOCOL".into(),
            );
        }
        let Ok(uri) = build_ws_extended_connect_uri(target_url, &key.authority()) else {
            return UpstreamWsOutcome::Unavailable("invalid H2 extended connect URI".into());
        };

        let mut builder = Request::builder()
            .method(Method::CONNECT)
            .uri(uri)
            .version(http::Version::HTTP_2);

        let mut has_ws_version = false;
        for (name, val) in extra_headers {
            let lower = name.as_str();
            if is_ws_h1_hop_header(lower) || is_hop_by_hop_header(lower) || lower == "host" {
                continue;
            }
            if lower == "sec-websocket-version" {
                has_ws_version = true;
            }
            builder = builder.header(name.clone(), val.clone());
        }
        if !has_ws_version {
            builder = builder.header("sec-websocket-version", "13");
        }

        let Ok(mut req) = builder.body(()) else {
            return UpstreamWsOutcome::Unavailable("failed to build H2 CONNECT request".into());
        };
        req.extensions_mut().insert(H2Protocol::from_static("websocket"));

        let send_req = session.send_req.clone();
        let mut ready = match send_req.ready().await {
            Ok(r) => r,
            Err(e) => {
                self.remove_h2_session(key, session.id).await;
                return UpstreamWsOutcome::Unavailable(e.to_string());
            }
        };

        let (resp_fut, send_stream) = match ready.send_request(req, false) {
            Ok(pair) => pair,
            Err(e) => {
                self.remove_h2_session(key, session.id).await;
                return UpstreamWsOutcome::Unavailable(e.to_string());
            }
        };

        let resp = match tokio::time::timeout(self.config.h2_extended_connect_handshake_timeout, resp_fut).await
        {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return UpstreamWsOutcome::Unavailable(e.to_string()),
            Err(_) => return UpstreamWsOutcome::Unavailable("H2 extended CONNECT timed out".into()),
        };

        let status = reqwest::StatusCode::from_u16(resp.status().as_u16())
            .unwrap_or(reqwest::StatusCode::BAD_GATEWAY);
        let headers = resp.headers().clone();

        if status != reqwest::StatusCode::OK {
            let mut recv = resp.into_body();
            let mut body_buf = Vec::new();
            while let Some(chunk) = recv.data().await {
                if let Ok(chunk) = chunk {
                    let _ = recv.flow_control().release_capacity(chunk.len());
                    let take = chunk
                        .len()
                        .min(MAX_RESPONSE_ERROR_BODY.saturating_sub(body_buf.len()));
                    body_buf.extend_from_slice(&chunk[..take]);
                    if body_buf.len() >= MAX_RESPONSE_ERROR_BODY {
                        break;
                    }
                } else {
                    break;
                }
            }
            return UpstreamWsOutcome::Rejected {
                status,
                headers,
                body: Bytes::from(body_buf),
            };
        }

        let negotiated_protocol = headers
            .get("sec-websocket-protocol")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let recv = resp.into_body();
        let mut io = H2ConnectIo::new(send_stream, recv);
        io._guard = Some(SessionStreamGuard::new(
            Arc::clone(&self.active_streams),
            Some(Arc::clone(&self.h2_ws_active)),
            Some(permit),
        ));

        UpstreamWsOutcome::Connected(UpstreamWsConnection {
            kind: UpstreamWsKind::H2ExtendedConnect,
            negotiated_protocol,
            ws: WebSocketStream::from_raw_socket(UpstreamWsIo::H2(io), Role::Client, None).await,
        })
    }

    async fn connect_h3_extended_connect(
        &self,
        key: &SessionKey,
        session: &H3SessionEntry,
        permit: OwnedSemaphorePermit,
        target_url: &reqwest::Url,
        extra_headers: &[(HeaderName, HeaderValue)],
    ) -> UpstreamWsOutcome {
        let Ok(uri) = build_ws_extended_connect_uri(target_url, &key.authority()) else {
            return UpstreamWsOutcome::Unavailable("invalid H3 extended CONNECT URI".into());
        };

        let mut builder = Request::builder()
            .method(Method::CONNECT)
            .uri(uri)
            .version(http::Version::HTTP_3);
        let mut has_ws_version = false;
        for (name, value) in extra_headers {
            let lower = name.as_str();
            if is_ws_h1_hop_header(lower) || is_hop_by_hop_header(lower) || lower == "host" {
                continue;
            }
            if lower == "sec-websocket-version" {
                has_ws_version = true;
            }
            builder = builder.header(name.clone(), value.clone());
        }
        if !has_ws_version {
            builder = builder.header("sec-websocket-version", "13");
        }

        let Ok(mut request) = builder.body(()) else {
            return UpstreamWsOutcome::Unavailable("failed to build H3 CONNECT request".into());
        };
        request.extensions_mut().insert(h3::ext::Protocol::WEBSOCKET);

        let mut send_req = session.send_req.clone();
        let mut stream = match tokio::time::timeout(
            self.config.h3_handshake_timeout,
            send_req.send_request(request),
        )
        .await
        {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => return UpstreamWsOutcome::Unavailable(error.to_string()),
            Err(_) => return UpstreamWsOutcome::Unavailable("H3 CONNECT timed out".into()),
        };
        let response = match tokio::time::timeout(
            self.config.h3_handshake_timeout,
            stream.recv_response(),
        )
        .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return UpstreamWsOutcome::Unavailable(error.to_string()),
            Err(_) => return UpstreamWsOutcome::Unavailable("H3 WS response timed out".into()),
        };

        let status = reqwest::StatusCode::from_u16(response.status().as_u16())
            .unwrap_or(reqwest::StatusCode::BAD_GATEWAY);
        let headers = response.headers().clone();
        if status != reqwest::StatusCode::OK {
            let mut body = Vec::new();
            loop {
                match stream.recv_data().await {
                    Ok(Some(mut chunk)) => {
                        let remaining = MAX_RESPONSE_ERROR_BODY.saturating_sub(body.len());
                        if remaining == 0 {
                            break;
                        }
                        let take = chunk.remaining().min(remaining);
                        body.extend_from_slice(&chunk.copy_to_bytes(take));
                        if body.len() >= MAX_RESPONSE_ERROR_BODY {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            return UpstreamWsOutcome::Rejected {
                status,
                headers,
                body: Bytes::from(body),
            };
        }

        let negotiated_protocol = headers
            .get("sec-websocket-protocol")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let guard = SessionStreamGuard::new(
            Arc::clone(&self.active_streams),
            Some(Arc::clone(&self.h3_ws_active)),
            Some(permit),
        );
        let io = bridge_h3_websocket_stream(stream, guard);
        UpstreamWsOutcome::Connected(UpstreamWsConnection {
            kind: UpstreamWsKind::H3ExtendedConnect,
            negotiated_protocol,
            ws: WebSocketStream::from_raw_socket(UpstreamWsIo::H3(io), Role::Client, None).await,
        })
    }

    async fn get_quic_endpoint_for(
        &self,
        addr: SocketAddr,
    ) -> Result<quinn::Endpoint, UpstreamTransportError> {
        let mut guard = self.quic_endpoints.lock().await;
        if guard.is_none() {
            let v4 = quinn::Endpoint::client(SocketAddr::from(([0, 0, 0, 0], 0))).ok();
            let v6 = quinn::Endpoint::client(SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0))
                .ok();
            *guard = Some(QuicEndpoints { v4, v6 });
        }
        let endpoints = guard.as_ref().expect("quic endpoints initialized");
        let ep = match addr {
            SocketAddr::V4(_) => endpoints.v4.as_ref(),
            SocketAddr::V6(_) => endpoints.v6.as_ref().or(endpoints.v4.as_ref()),
        };
        ep.cloned()
            .ok_or_else(|| UpstreamTransportError::DialFailed("no matching UDP QUIC endpoint".into()))
    }

    async fn dial_h3(&self, key: &SessionKey) -> Result<H3SessionEntry, UpstreamTransportError> {
        let addrs = resolve_addrs(&key.host, key.port).await?;
        let mut tls_cfg = build_rustls_client_config(
            vec![b"h3".to_vec()],
            &self.config.extra_root_certs,
        )
        .map_err(UpstreamTransportError::DialFailed)?;
        tls_cfg.enable_early_data = false;

        let quic_crypto = QuicClientConfig::try_from(tls_cfg)
            .map_err(|e| UpstreamTransportError::DialFailed(e.to_string()))?;
        let mut transport = quinn::TransportConfig::default();
        if !self.config.h3_keep_alive_interval.is_zero() {
            transport.keep_alive_interval(Some(self.config.h3_keep_alive_interval));
        }
        if let Ok(idle) = quinn::IdleTimeout::try_from(self.config.h3_max_idle_timeout) {
            transport.max_idle_timeout(Some(idle));
        }
        transport.max_concurrent_bidi_streams(1024u32.into());
        transport.max_concurrent_uni_streams(64u32.into());

        let mut quinn_cfg = quinn::ClientConfig::new(Arc::new(quic_crypto));
        quinn_cfg.transport_config(Arc::new(transport));

        let handshake_timeout = self.config.h3_handshake_timeout;
        let host = key.host.clone();
        let mut prepared = Vec::with_capacity(addrs.len());
        let mut last_err = String::from("no resolved addresses");
        for addr in addrs {
            match self.get_quic_endpoint_for(addr).await {
                Ok(ep) => prepared.push((addr, ep)),
                Err(e) => last_err = e.to_string(),
            }
        }
        if prepared.is_empty() {
            return Err(UpstreamTransportError::DialFailed(last_err));
        }

        // Dual-stack Happy Eyeballs (staggered 120ms): avoids wasting the 600ms grace period
        // on a blackholed IPv6/AAAA address when IPv4 QUIC is healthy.
        let mut join_set = tokio::task::JoinSet::new();
        let mut next_idx = 0usize;
        while next_idx < prepared.len() {
            let (addr, endpoint) = prepared[next_idx].clone();
            next_idx += 1;
            let cfg = quinn_cfg.clone();
            let sni = host.clone();
            join_set.spawn(async move { dial_h3_single(endpoint, cfg, addr, sni, handshake_timeout).await });

            if next_idx < prepared.len() {
                tokio::select! {
                    Some(res) = join_set.join_next() => {
                        match res {
                            Ok(Ok((conn, driver, send_req))) => {
                                return Ok(self.finish_h3_session(conn, driver, send_req));
                            }
                            Ok(Err(e)) => last_err = e,
                            Err(e) => last_err = e.to_string(),
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(120)) => {}
                }
            }
        }

        while let Some(res) = join_set.join_next().await {
            match res {
                Ok(Ok((conn, driver, send_req))) => {
                    return Ok(self.finish_h3_session(conn, driver, send_req));
                }
                Ok(Err(e)) => last_err = e,
                Err(e) => last_err = e.to_string(),
            }
        }

        Err(UpstreamTransportError::DialFailed(last_err))
    }

    fn finish_h3_session(
        &self,
        conn: quinn::Connection,
        mut driver: h3::client::Connection<h3_quinn::Connection, Bytes>,
        send_req: H3SendRequest<h3_quinn::OpenStreams, Bytes>,
    ) -> H3SessionEntry {
        let mark = LiveSessionMark::new(Arc::clone(&self.h3_sessions_active));
        let mark_driver = Arc::clone(&mark);
        tokio::spawn(async move {
            let _ = poll_fn(|cx| driver.poll_close(cx)).await;
            mark_driver.deactivate();
        });

        H3SessionEntry {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            created_at: Instant::now(),
            conn,
            send_req,
            mark,
            stream_slots: Arc::new(Semaphore::new(self.config.session_stream_limit)),
        }
    }

    async fn dial_h2(&self, key: &SessionKey) -> Result<H2SessionEntry, UpstreamTransportError> {
        let tcp = tokio::time::timeout(
            self.config.h2_handshake_timeout,
            connect_tcp_maybe_proxy(&key.host, key.port, &key.proxy),
        )
        .await
        .map_err(|_| UpstreamTransportError::DialFailed("TCP connect timed out".into()))?
        .map_err(UpstreamTransportError::DialFailed)?;

        let tls_cfg = build_rustls_client_config(
            vec![b"h2".to_vec()],
            &self.config.extra_root_certs,
        )
        .map_err(UpstreamTransportError::DialFailed)?;
        let server_name = ServerName::try_from(key.host.clone())
            .map_err(|e| UpstreamTransportError::DialFailed(e.to_string()))?;
        let connector = TlsConnector::from(Arc::new(tls_cfg));

        let tls_stream = tokio::time::timeout(
            self.config.h2_handshake_timeout,
            connector.connect(server_name, tcp),
        )
        .await
        .map_err(|_| UpstreamTransportError::DialFailed("H2 TLS handshake timed out".into()))?
        .map_err(|e| UpstreamTransportError::DialFailed(e.to_string()))?;

        let negotiated_h2 = tls_stream
            .get_ref()
            .1
            .alpn_protocol()
            .map(|p| p == b"h2")
            .unwrap_or(false);
        if !negotiated_h2 {
            return Err(UpstreamTransportError::DialFailed(
                "upstream ALPN did not negotiate h2".into(),
            ));
        }

        let mut builder = h2::client::Builder::new();
        builder
            .initial_window_size(1_048_576)
            .initial_connection_window_size(2_097_152)
            .max_concurrent_streams(self.config.session_stream_limit as u32)
            .max_frame_size(16_384)
            .enable_push(false);

        let (send_req, mut conn) = tokio::time::timeout(
            self.config.h2_handshake_timeout,
            builder.handshake(tls_stream),
        )
        .await
        .map_err(|_| UpstreamTransportError::DialFailed("H2 handshake timed out".into()))?
        .map_err(|e| UpstreamTransportError::DialFailed(e.to_string()))?;

        let mut ping_pong = conn.ping_pong();
        let mark = LiveSessionMark::new(Arc::clone(&self.h2_sessions_active));
        let mark_driver = Arc::clone(&mark);
        tokio::spawn(async move {
            let _ = conn.await;
            mark_driver.deactivate();
        });

        // Exchange an initial PING so the server's initial SETTINGS frame (including
        // SETTINGS_ENABLE_CONNECT_PROTOCOL for RFC 8441) is guaranteed processed.
        if let Some(pp) = ping_pong.as_mut() {
            let _ = tokio::time::timeout(
                self.config.h2_extended_connect_ping_timeout,
                pp.ping(h2::Ping::opaque()),
            )
            .await;
        }
        let ws_enabled = send_req.is_extended_connect_protocol_enabled();

        Ok(H2SessionEntry {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            created_at: Instant::now(),
            send_req,
            ping_pong: Arc::new(Mutex::new(ping_pong)),
            mark,
            stream_slots: Arc::new(Semaphore::new(self.config.session_stream_limit)),
            ws_enabled,
        })
    }
}

static RUSTLS_PROVIDER_INIT: OnceLock<()> = OnceLock::new();

fn rustls_provider() -> Arc<rustls::crypto::CryptoProvider> {
    RUSTLS_PROVIDER_INIT.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    Arc::new(rustls::crypto::ring::default_provider())
}

pub fn build_rustls_client_config(
    alpn_protocols: Vec<Vec<u8>>,
    extra_root_certs: &[CertificateDer<'static>],
) -> Result<rustls::ClientConfig, String> {
    let provider = rustls_provider();
    let builder = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?;

    let mut cfg = if extra_root_certs.is_empty() {
        builder
            .with_platform_verifier()
            .map_err(|e| e.to_string())?
            .with_no_client_auth()
    } else {
        let mut store = rustls::RootCertStore::empty();
        for cert in extra_root_certs {
            store.add(cert.clone()).map_err(|e| e.to_string())?;
        }
        builder.with_root_certificates(store).with_no_client_auth()
    };

    cfg.alpn_protocols = alpn_protocols;
    Ok(cfg)
}

fn build_plain_reqwest_client(
    outbound_proxy: &str,
    timeout: Duration,
    extra_root_certs: &[CertificateDer<'static>],
) -> Result<reqwest::Client, UpstreamTransportError> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(timeout)
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(50);

    for cert_der in extra_root_certs {
        if let Ok(cert) = reqwest::Certificate::from_der(cert_der.as_ref()) {
            builder = builder.add_root_certificate(cert);
        }
    }

    let trimmed = outbound_proxy.trim();
    if trimmed.is_empty() {
        builder = builder.no_proxy();
    } else {
        let proxy = reqwest::Proxy::all(trimmed)
            .map_err(|e| UpstreamTransportError::InvalidRequest(e.to_string()))?;
        builder = builder.proxy(proxy);
    }

    builder
        .build()
        .map_err(|e| UpstreamTransportError::DialFailed(e.to_string()))
}

async fn send_h2_body(
    send_stream: &mut h2::SendStream<Bytes>,
    body: Bytes,
) -> Result<(), UpstreamTransportError> {
    let mut offset = 0;
    while offset < body.len() {
        let remaining = body.len() - offset;
        send_stream.reserve_capacity(remaining);
        match poll_fn(|cx| send_stream.poll_capacity(cx)).await {
            Some(Ok(cap)) if cap > 0 => {
                let n = remaining.min(cap);
                let is_last = offset + n == body.len();
                send_stream
                    .send_data(body.slice(offset..offset + n), is_last)
                    .map_err(|e| UpstreamTransportError::PreResponseFailure {
                        protocol: UpstreamProtocol::Http2,
                        detail: e.to_string(),
                    })?;
                offset += n;
            }
            Some(Ok(_)) => continue,
            Some(Err(e)) => {
                return Err(UpstreamTransportError::PreResponseFailure {
                    protocol: UpstreamProtocol::Http2,
                    detail: e.to_string(),
                });
            }
            None => {
                return Err(UpstreamTransportError::PreResponseFailure {
                    protocol: UpstreamProtocol::Http2,
                    detail: "h2 request stream closed while sending body".into(),
                });
            }
        }
    }
    Ok(())
}

async fn dial_h3_single(
    endpoint: quinn::Endpoint,
    quinn_cfg: quinn::ClientConfig,
    addr: SocketAddr,
    sni_host: String,
    handshake_timeout: Duration,
) -> Result<
    (
        quinn::Connection,
        h3::client::Connection<h3_quinn::Connection, Bytes>,
        H3SendRequest<h3_quinn::OpenStreams, Bytes>,
    ),
    String,
> {
    let connecting = endpoint
        .connect_with(quinn_cfg, addr, &sni_host)
        .map_err(|e| e.to_string())?;

    let conn = tokio::time::timeout(handshake_timeout, connecting)
        .await
        .map_err(|_| format!("QUIC handshake to {addr} timed out"))?
        .map_err(|e| e.to_string())?;

    let h3_conn = h3_quinn::Connection::new(conn.clone());
    match tokio::time::timeout(
        handshake_timeout,
        h3::client::builder()
            .enable_extended_connect(true)
            .build::<_, _, Bytes>(h3_conn),
    )
    .await
    {
        Ok(Ok((driver, send_req))) => Ok((conn, driver, send_req)),
        Ok(Err(e)) => {
            conn.close(0u32.into(), b"h3 build failed");
            Err(e.to_string())
        }
        Err(_) => {
            conn.close(0u32.into(), b"h3 build timed out");
            Err("H3 connection initialization timed out".to_string())
        }
    }
}

async fn resolve_addrs(host: &str, port: u16) -> Result<Vec<SocketAddr>, UpstreamTransportError> {
    if let Ok(ip) = host.trim_matches(|c| c == '[' || c == ']').parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| UpstreamTransportError::DialFailed(format!("DNS lookup failed: {e}")))?
        .collect();
    if addrs.is_empty() {
        return Err(UpstreamTransportError::DialFailed(format!(
            "DNS resolved no addresses for {host}:{port}"
        )));
    }
    Ok(addrs)
}

async fn connect_tcp_addrs(host: &str, port: u16) -> Result<TcpStream, String> {
    let addrs = resolve_addrs(host, port)
        .await
        .map_err(|e| e.to_string())?;
    let mut join_set = tokio::task::JoinSet::new();
    let mut last_err = String::from("no resolved addresses");
    let mut next_idx = 0usize;

    while next_idx < addrs.len() {
        let addr = addrs[next_idx];
        next_idx += 1;
        join_set.spawn(async move {
            let s = TcpStream::connect(addr)
                .await
                .map_err(|e| format!("{addr}: {e}"))?;
            let _ = s.set_nodelay(true);
            Ok::<TcpStream, String>(s)
        });

        if next_idx < addrs.len() {
            tokio::select! {
                Some(res) = join_set.join_next() => {
                    match res {
                        Ok(Ok(stream)) => return Ok(stream),
                        Ok(Err(e)) => last_err = e,
                        Err(e) => last_err = e.to_string(),
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(120)) => {}
            }
        }
    }

    while let Some(res) = join_set.join_next().await {
        match res {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(e)) => last_err = e,
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(last_err)
}

async fn connect_tcp_maybe_proxy(
    target_host: &str,
    target_port: u16,
    outbound_proxy: &str,
) -> Result<ProxyStream, String> {
    let trimmed = outbound_proxy.trim();
    if trimmed.is_empty() {
        let tcp = connect_tcp_addrs(target_host, target_port).await?;
        return Ok(ProxyStream::Direct(tcp));
    }

    let proxy_url =
        reqwest::Url::parse(trimmed).map_err(|e| format!("invalid outbound proxy url: {e}"))?;
    let proxy_host = proxy_url
        .host_str()
        .ok_or_else(|| "outbound proxy missing host".to_string())?;
    let proxy_port = proxy_url
        .port_or_known_default()
        .ok_or_else(|| "outbound proxy missing port".to_string())?;

    match proxy_url.scheme() {
        "http" => {
            let mut tcp = connect_tcp_addrs(proxy_host, proxy_port).await?;
            let target_auth = if target_host.contains(':') && !target_host.starts_with('[') {
                format!("[{target_host}]:{target_port}")
            } else {
                format!("{target_host}:{target_port}")
            };
            let mut req_str =
                format!("CONNECT {target_auth} HTTP/1.1\r\nHost: {target_auth}\r\n");
            if !proxy_url.username().is_empty() || proxy_url.password().is_some() {
                let user = proxy_url.username();
                let pass = proxy_url.password().unwrap_or("");
                let creds = encode_base64(format!("{user}:{pass}").as_bytes());
                req_str.push_str(&format!("Proxy-Authorization: Basic {creds}\r\n"));
            }
            req_str.push_str("\r\n");
            tcp.write_all(req_str.as_bytes())
                .await
                .map_err(|e| format!("proxy CONNECT write failed: {e}"))?;

            let mut buf = Vec::with_capacity(1024);
            let mut tmp = [0u8; 512];
            let header_end = loop {
                let n = tcp
                    .read(&mut tmp)
                    .await
                    .map_err(|e| format!("proxy CONNECT read failed: {e}"))?;
                if n == 0 {
                    return Err("proxy closed during CONNECT".into());
                }
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = find_double_crlf(&buf) {
                    break pos;
                }
                if buf.len() > 8192 {
                    return Err("proxy CONNECT response headers too large".into());
                }
            };

            let status_line = std::str::from_utf8(&buf[..header_end])
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("");
            let code: u16 = status_line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if !(200..300).contains(&code) {
                return Err(format!("proxy CONNECT rejected with status {status_line}"));
            }

            let leftover = buf.split_off(header_end + 4);
            if leftover.is_empty() {
                Ok(ProxyStream::Direct(tcp))
            } else {
                Ok(ProxyStream::Prefixed {
                    inner: tcp,
                    prefix: Bytes::from(leftover),
                })
            }
        }
        "socks5" | "socks5h" => {
            let proxy_addr = resolve_addrs(proxy_host, proxy_port)
                .await
                .map_err(|e| e.to_string())?
                .into_iter()
                .next()
                .ok_or_else(|| "no SOCKS5 proxy address".to_string())?;
            let socks = if !proxy_url.username().is_empty() || proxy_url.password().is_some() {
                Socks5Stream::connect_with_password(
                    proxy_addr,
                    (target_host, target_port),
                    proxy_url.username(),
                    proxy_url.password().unwrap_or(""),
                )
                .await
                .map_err(|e| format!("SOCKS5 connect failed: {e}"))?
            } else {
                Socks5Stream::connect(proxy_addr, (target_host, target_port))
                    .await
                    .map_err(|e| format!("SOCKS5 connect failed: {e}"))?
            };
            let inner = socks.into_inner();
            let _ = inner.set_nodelay(true);
            Ok(ProxyStream::Direct(inner))
        }
        other => Err(format!("unsupported outbound proxy scheme: {other}")),
    }
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((triple >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn build_target_uri(
    url: &reqwest::Url,
    authority: &str,
) -> Result<Uri, UpstreamTransportError> {
    let mut path_and_query = url.path().to_string();
    if path_and_query.is_empty() {
        path_and_query.push('/');
    }
    if let Some(q) = url.query() {
        path_and_query.push('?');
        path_and_query.push_str(q);
    }
    format!("https://{authority}{path_and_query}")
        .parse::<Uri>()
        .map_err(|e| UpstreamTransportError::InvalidRequest(e.to_string()))
}

fn build_ws_extended_connect_uri(
    url: &reqwest::Url,
    authority: &str,
) -> Result<Uri, UpstreamTransportError> {
    let mut path_and_query = url.path().to_string();
    if path_and_query.is_empty() {
        path_and_query.push('/');
    }
    if let Some(q) = url.query() {
        path_and_query.push('?');
        path_and_query.push_str(q);
    }
    format!("https://{authority}{path_and_query}")
        .parse::<Uri>()
        .map_err(|e| UpstreamTransportError::InvalidRequest(e.to_string()))
}

fn is_hop_by_hop_header(lower_name: &str) -> bool {
    matches!(
        lower_name,
        "connection"
            | "keep-alive"
            | "proxy-connection"
            | "transfer-encoding"
            | "upgrade"
            | "te"
            | "trailer"
    )
}

fn is_ws_h1_hop_header(lower_name: &str) -> bool {
    matches!(
        lower_name,
        "sec-websocket-key" | "sec-websocket-extensions" | "upgrade" | "connection"
    )
}

fn sanitize_h2_h3_headers(headers: &[(HeaderName, HeaderValue)]) -> Vec<(HeaderName, HeaderValue)> {
    let mut connection_tokens: HashSet<String> = HashSet::new();
    for (name, value) in headers {
        if name.as_str() == "connection" {
            if let Ok(text) = value.to_str() {
                for token in text.split(',') {
                    let trimmed = token.trim().to_ascii_lowercase();
                    if !trimmed.is_empty() {
                        connection_tokens.insert(trimmed);
                    }
                }
            }
        }
    }

    headers
        .iter()
        .filter(|(name, _)| {
            let lower = name.as_str();
            !is_hop_by_hop_header(lower)
                && lower != "host"
                && !connection_tokens.contains(lower)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use futures_util::{SinkExt, StreamExt};
    use http::Response;
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    fn generate_localhost_cert() -> (
        Vec<CertificateDer<'static>>,
        rustls::pki_types::PrivateKeyDer<'static>,
        CertificateDer<'static>,
    ) {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let der = cert.cert.der().clone();
        let key_der = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()),
        );
        (vec![der.clone()], key_der, der)
    }

    fn test_server_rustls(
        alpn: Vec<Vec<u8>>,
        certs: Vec<CertificateDer<'static>>,
        key: rustls::pki_types::PrivateKeyDer<'static>,
    ) -> rustls::ServerConfig {
        let provider = rustls_provider();
        let mut cfg = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .unwrap();
        cfg.alpn_protocols = alpn;
        cfg
    }

    async fn spawn_h3_server<F, Fut>(
        certs: Vec<CertificateDer<'static>>,
        key: rustls::pki_types::PrivateKeyDer<'static>,
        handler: F,
    ) -> (u16, tokio::task::JoinHandle<()>)
    where
        F: Fn(
                http::Request<()>,
                h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
            ) -> Fut
            + Send
            + Sync
            + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let tls_cfg = test_server_rustls(vec![b"h3".to_vec()], certs, key);
        let quic_srv = quinn::crypto::rustls::QuicServerConfig::try_from(tls_cfg).unwrap();
        let mut srv_cfg = quinn::ServerConfig::with_crypto(Arc::new(quic_srv));
        let mut quic_transport = quinn::TransportConfig::default();
        quic_transport.max_concurrent_bidi_streams((MAX_SESSION_STREAMS as u32 + 1).into());
        srv_cfg.transport_config(Arc::new(quic_transport));
        let endpoint =
            quinn::Endpoint::server(srv_cfg, SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let port = endpoint.local_addr().unwrap().port();
        let handler = Arc::new(handler);

        let join = tokio::spawn(async move {
            while let Some(incoming) = endpoint.accept().await {
                let handler = Arc::clone(&handler);
                tokio::spawn(async move {
                    let Ok(conn) = incoming.await else {
                        return;
                    };
                    let Ok(mut h3_conn) = h3::server::builder()
                        .enable_extended_connect(true)
                        .build(h3_quinn::Connection::new(conn))
                        .await
                    else {
                        return;
                    };
                    while let Ok(Some(resolver)) = h3_conn.accept().await {
                        let handler = Arc::clone(&handler);
                        tokio::spawn(async move {
                            if let Ok((req, stream)) = resolver.resolve_request().await {
                                handler(req, stream).await;
                            }
                        });
                    }
                });
            }
        });
        (port, join)
    }

    async fn spawn_h2_server<F, Fut>(
        port: u16,
        certs: Vec<CertificateDer<'static>>,
        key: rustls::pki_types::PrivateKeyDer<'static>,
        delay_before_accept: Duration,
        enable_extended_connect: bool,
        handler: F,
    ) -> (u16, tokio::task::JoinHandle<()>)
    where
        F: Fn(http::Request<h2::RecvStream>, h2::server::SendResponse<Bytes>) -> Fut
            + Send
            + Sync
            + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let tls_cfg = test_server_rustls(vec![b"h2".to_vec()], certs, key);
        let acceptor = TlsAcceptor::from(Arc::new(tls_cfg));
        let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        let actual_port = listener.local_addr().unwrap().port();
        let handler = Arc::new(handler);

        let join = tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                let handler = Arc::clone(&handler);
                tokio::spawn(async move {
                    if !delay_before_accept.is_zero() {
                        tokio::time::sleep(delay_before_accept).await;
                    }
                    let Ok(tls) = acceptor.accept(tcp).await else {
                        return;
                    };
                    let mut builder = h2::server::Builder::new();
                    if enable_extended_connect {
                        builder.enable_connect_protocol();
                    }
                    let Ok(mut h2_conn) = builder.handshake::<_, Bytes>(tls).await else {
                        return;
                    };
                    while let Some(Ok((req, respond))) = h2_conn.accept().await {
                        let handler = Arc::clone(&handler);
                        tokio::spawn(async move {
                            handler(req, respond).await;
                        });
                    }
                });
            }
        });

        (actual_port, join)
    }

    fn capacity_test_request(port: u16) -> UpstreamHttpRequest {
        UpstreamHttpRequest {
            method: Method::GET,
            url: format!("https://localhost:{port}/events").parse().unwrap(),
            headers: vec![],
            body: Bytes::new(),
            outbound_proxy: String::new(),
        }
    }

    async fn check_stream_pool_capacity(
        transport: &UpstreamTransport,
        port: u16,
        protocol: UpstreamProtocol,
        finish_first: &Notify,
    ) {
        let request = capacity_test_request(port);
        let mut first_request = request.clone();
        first_request.url.set_path("/first");
        let (first, remaining) = tokio::join!(
            transport.send_http(first_request),
            futures_util::future::try_join_all(
                (1..MAX_SESSION_STREAMS).map(|_| transport.send_http(request.clone())),
            ),
        );
        let mut first = first.unwrap();
        let remaining = remaining.unwrap();
        assert_eq!(first.protocol, protocol);
        assert!(remaining.iter().all(|response| response.protocol == protocol));

        let full = transport.snapshot().upstream_sessions;
        assert_eq!(full.len(), 1);
        let primary = full[0].id;
        assert_eq!(full[0].occupied_stream_slots, 500);

        let overflow = transport.send_http(request.clone()).await.unwrap();
        assert_eq!(overflow.protocol, protocol);
        let full = transport.snapshot().upstream_sessions;
        assert_eq!(full.len(), 2);
        assert!(full.iter().all(|s| s.protocol == protocol.as_str()));
        assert_eq!(full[0].occupied_stream_slots, 500);
        assert_eq!(full[1].occupied_stream_slots, 1);

        finish_first.notify_one();
        while let Some(chunk) = first.next_chunk().await.unwrap() {
            // H2 can deliver an empty DATA frame carrying END_STREAM.
            assert!(chunk.is_empty());
        }
        let replacement = transport.send_http(request).await.unwrap();
        assert_eq!(replacement.protocol, protocol);
        let reused = transport.snapshot().upstream_sessions;
        assert_eq!(reused.len(), 2);
        assert_eq!(reused[0].id, primary);
        assert_eq!(reused[0].occupied_stream_slots, 500);
        assert_eq!(reused[1].occupied_stream_slots, 1);

        drop((remaining, overflow, replacement));
        transport.inner.run_maintenance().await;
        let compacted = transport.snapshot().upstream_sessions;
        assert_eq!(compacted.len(), 1);
        assert_eq!(compacted[0].id, primary);
        assert_eq!(compacted[0].occupied_stream_slots, 0);
    }

    #[tokio::test]
    async fn h2_pool_enforces_concurrent_capacity_and_reuses_released_streams() {
        let (certs, key, root) = generate_localhost_cert();
        let finish_first = Arc::new(Notify::new());
        let finish_others = Arc::new(Notify::new());
        let finish = Arc::clone(&finish_first);
        let others = Arc::clone(&finish_others);
        let (port, server) = spawn_h2_server(
            0, certs, key, Duration::ZERO, false,
            move |request, mut respond| {
                let finish = Arc::clone(if request.uri().path() == "/first" { &finish } else { &others });
                async move {
                    let mut send = respond.send_response(Response::new(()), false).unwrap();
                    finish.notified().await;
                    let _ = send.send_data(Bytes::new(), true);
                }
            },
        ).await;
        let transport = UpstreamTransport::new(UpstreamTransportConfig {
            enable_h3: false,
            extra_root_certs: vec![root],
            ..Default::default()
        });
        tokio::time::timeout(
            Duration::from_secs(20),
            check_stream_pool_capacity(&transport, port, UpstreamProtocol::Http2, &finish_first),
        ).await.unwrap();
        finish_others.notify_waiters();
        server.abort();
    }

    #[tokio::test]
    async fn h3_pool_enforces_concurrent_capacity_and_reuses_released_streams() {
        let (certs, key, root) = generate_localhost_cert();
        let finish_first = Arc::new(Notify::new());
        let finish_others = Arc::new(Notify::new());
        let finish = Arc::clone(&finish_first);
        let others = Arc::clone(&finish_others);
        let (port, server) = spawn_h3_server(certs, key, move |request, mut stream| {
            let finish = Arc::clone(if request.uri().path() == "/first" { &finish } else { &others });
            async move {
                stream.send_response(Response::new(())).await.unwrap();
                finish.notified().await;
                let _ = stream.finish().await;
            }
        }).await;
        let transport = UpstreamTransport::new(UpstreamTransportConfig {
            extra_root_certs: vec![root],
            ..Default::default()
        });
        tokio::time::timeout(
            Duration::from_secs(20),
            check_stream_pool_capacity(&transport, port, UpstreamProtocol::Http3, &finish_first),
        ).await.unwrap();
        finish_others.notify_waiters();
        server.abort();
    }

    #[tokio::test]
    async fn h3_websocket_shares_http_capacity_and_releases_it_on_drop() {
        let (certs, key, root) = generate_localhost_cert();
        let (port, server) = spawn_h3_server(certs, key, |request, mut stream| async move {
            let response = Response::new(());
            if request.method() == Method::CONNECT {
                assert_eq!(
                    request.extensions().get::<h3::ext::Protocol>(),
                    Some(&h3::ext::Protocol::WEBSOCKET)
                );
                stream.send_response(response).await.unwrap();
                // Leave the tunnel open until the client tears it down.
                while let Ok(Some(_)) = stream.recv_data().await {}
            } else {
                stream.send_response(response).await.unwrap();
            }
            let _ = stream.finish().await;
        }).await;
        let transport = UpstreamTransport::new(UpstreamTransportConfig {
            session_stream_limit: 1,
            extra_root_certs: vec![root],
            ..Default::default()
        });
        tokio::time::timeout(Duration::from_secs(10), async {
            let request = capacity_test_request(port);
            let ws = match transport.connect_websocket(&request.url, &[], "").await {
                UpstreamWsOutcome::Connected(ws) => ws,
                UpstreamWsOutcome::Rejected { status, .. } => panic!("H3 WS rejected: {status}"),
                UpstreamWsOutcome::Unavailable(error) => panic!("H3 WS failed: {error}"),
            };
            assert_eq!(ws.kind, UpstreamWsKind::H3ExtendedConnect);
            let before = transport.snapshot();
            assert_eq!(before.upstream_h3_ws, 1);
            let primary = before.upstream_sessions[0].id;
            assert_eq!(before.upstream_sessions[0].occupied_stream_slots, 1);

            let http = transport.send_http(request.clone()).await.unwrap();
            let full = transport.snapshot().upstream_sessions;
            assert_eq!(full.len(), 2);
            assert!(full.iter().all(|s| s.occupied_stream_slots == 1));

            drop(ws);
            while transport.snapshot().upstream_sessions[0].occupied_stream_slots != 0 {
                tokio::task::yield_now().await;
            }
            let reused = transport.send_http(request).await.unwrap();
            let full_again = transport.snapshot().upstream_sessions;
            assert_eq!(full_again.len(), 2);
            assert_eq!(full_again[0].id, primary);
            assert!(full_again.iter().all(|s| s.occupied_stream_slots == 1));
            drop((http, reused));
        }).await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn race_prefers_h3_during_its_head_start() {
        let (certs, key, root_cert) = generate_localhost_cert();

        // Bind a UDP socket temporarily to obtain a free UDP port, Wait: QUIC and TCP ports are
        // independent on OS, so we spawn H3 first and bind TCP on the SAME port (if available).
        // Retry if TCP port collides.
        let mut h3_handle = None;
        let mut h2_handle = None;
        let mut shared_port = 0u16;
        for _ in 0..8 {
            let (p, h3j) = spawn_h3_server(certs.clone(), key.clone_key(), |_req, mut stream| async move {
                let _ = stream.recv_data().await;
                let resp = Response::builder().status(200).body(()).unwrap();
                stream.send_response(resp).await.unwrap();
                stream.send_data(Bytes::from_static(b"from-h3")).await.unwrap();
                stream.finish().await.unwrap();
            })
            .await;
            if let Ok(listener) = TcpListener::bind(("127.0.0.1", p)).await {
                drop(listener);
                let (h2p, h2j) = spawn_h2_server(
                    p,
                    certs.clone(),
                    key.clone_key(),
                    Duration::ZERO,
                    false,
                    |_req, mut respond| async move {
                        let resp = Response::builder().status(200).body(()).unwrap();
                        let mut send = respond.send_response(resp, false).unwrap();
                        send.send_data(Bytes::from_static(b"from-h2"), true).unwrap();
                    },
                )
                .await;
                shared_port = h2p;
                h3_handle = Some(h3j);
                h2_handle = Some(h2j);
                break;
            } else {
                h3j.abort();
            }
        }
        assert_ne!(shared_port, 0);

        let transport = UpstreamTransport::new(UpstreamTransportConfig {
            h3_grace_period: Duration::from_millis(600),
            extra_root_certs: vec![root_cert],
            ..Default::default()
        });

        let resp = transport
            .send_http(UpstreamHttpRequest {
                method: Method::POST,
                url: format!("https://localhost:{shared_port}/v1/chat/completions")
                    .parse()
                    .unwrap(),
                headers: vec![],
                body: Bytes::from_static(b"{}"),
                outbound_proxy: String::new(),
            })
            .await
            .unwrap();

        assert_eq!(resp.protocol, UpstreamProtocol::Http3);
        let body = resp.collect_body(1024).await.unwrap();
        assert_eq!(body.as_ref(), b"from-h3");

        let key = SessionKey::new("localhost", shared_port, "");
        let (_overflow, permit) = transport.inner.acquire_h2_session(&key).await.unwrap();
        transport.inner.run_maintenance().await;
        assert_eq!(transport.snapshot().upstream_sessions.len(), 2);
        drop(permit);
        drop(_overflow);
        transport.inner.run_maintenance().await;
        let compacted = transport.snapshot().upstream_sessions;
        assert_eq!(compacted.len(), 1);
        assert_eq!(compacted[0].protocol, "http/3");

        if let Some(j) = h3_handle {
            j.abort();
        }
        if let Some(j) = h2_handle {
            j.abort();
        }
    }

    #[tokio::test]
    async fn h3_ai_thinking_survives_longer_than_idle_timeout_via_quic_keepalive() {
        let (certs, key, root_cert) = generate_localhost_cert();

        // H3 server sleeps 350ms before sending response headers ("AI thinking"), which is
        // longer than h3_max_idle_timeout (150ms), yet survives because QUIC PING/ACK runs every 40ms.
        let (port, h3j) = spawn_h3_server(certs, key, |_req, mut stream| async move {
            let _ = stream.recv_data().await;
            tokio::time::sleep(Duration::from_millis(350)).await;
            let resp = Response::builder().status(200).body(()).unwrap();
            stream.send_response(resp).await.unwrap();
            stream.send_data(Bytes::from_static(b"thought-complete")).await.unwrap();
            stream.finish().await.unwrap();
        })
        .await;

        let transport = UpstreamTransport::new(UpstreamTransportConfig {
            h3_grace_period: Duration::from_millis(200),
            h3_handshake_timeout: Duration::from_millis(500),
            h3_keep_alive_interval: Duration::from_millis(40),
            h3_max_idle_timeout: Duration::from_millis(150),
            h3_max_retries: 3,
            extra_root_certs: vec![root_cert],
            ..Default::default()
        });

        let resp = transport
            .send_http(UpstreamHttpRequest {
                method: Method::POST,
                url: format!("https://localhost:{port}/v1/responses")
                    .parse()
                    .unwrap(),
                headers: vec![],
                body: Bytes::from_static(b"{}"),
                outbound_proxy: String::new(),
            })
            .await
            .unwrap();

        assert_eq!(resp.protocol, UpstreamProtocol::Http3);
        let body = resp.collect_body(1024).await.unwrap();
        assert_eq!(body.as_ref(), b"thought-complete");
        h3j.abort();
    }

    #[tokio::test]
    async fn h3_dead_link_retries_three_times_then_falls_back_to_h2() {
        let (certs, key, root_cert) = generate_localhost_cert();
        let h3_attempts = Arc::new(AtomicUsize::new(0));
        let h2_hits = Arc::new(AtomicUsize::new(0));

        // QUIC server accepts handshake but immediately drops connection without responding,
        // forcing H3 stream failure on initial attempt + 3 retries, then H2 succeeds.
        let (port, h3j) = {
            let mut found = None;
            for _ in 0..8 {
                let tls_cfg = test_server_rustls(vec![b"h3".to_vec()], certs.clone(), key.clone_key());
                let quic_srv = quinn::crypto::rustls::QuicServerConfig::try_from(tls_cfg).unwrap();
                let srv_cfg = quinn::ServerConfig::with_crypto(Arc::new(quic_srv));
                let endpoint =
                    quinn::Endpoint::server(srv_cfg, SocketAddr::from(([127, 0, 0, 1], 0)))
                        .unwrap();
                let p = endpoint.local_addr().unwrap().port();
                if let Ok(listener) = TcpListener::bind(("127.0.0.1", p)).await {
                    drop(listener);
                    let h3_attempts_srv = Arc::clone(&h3_attempts);
                    let h3_join = tokio::spawn(async move {
                        while let Some(incoming) = endpoint.accept().await {
                            let h3_attempts_srv = Arc::clone(&h3_attempts_srv);
                            tokio::spawn(async move {
                                let Ok(conn) = incoming.await else {
                                    return;
                                };
                                let Ok(mut h3_conn) =
                                    h3::server::Connection::<_, Bytes>::new(h3_quinn::Connection::new(conn.clone()))
                                        .await
                                else {
                                    return;
                                };
                                if let Ok(Some(resolver)) = h3_conn.accept().await {
                                    if resolver.resolve_request().await.is_ok() {
                                        h3_attempts_srv.fetch_add(1, Ordering::Relaxed);
                                        conn.close(0x100u32.into(), b"simulated h3 failure");
                                    }
                                }
                            });
                        }
                    });
                    found = Some((p, h3_join));
                    break;
                }
            }
            found.unwrap()
        };

        let h2_hits_srv = Arc::clone(&h2_hits);
        let (_p2, h2j) = spawn_h2_server(
            port,
            certs,
            key,
            Duration::from_millis(20),
            false,
            move |_req, mut respond| {
                let h2_hits_srv = Arc::clone(&h2_hits_srv);
                async move {
                    h2_hits_srv.fetch_add(1, Ordering::Relaxed);
                    let resp = Response::builder().status(200).body(()).unwrap();
                    let mut send = respond.send_response(resp, false).unwrap();
                    send.send_data(Bytes::from_static(b"h2-rescue"), true)
                        .unwrap();
                }
            },
        )
        .await;

        let transport = UpstreamTransport::new(UpstreamTransportConfig {
            h3_grace_period: Duration::from_millis(400),
            h3_handshake_timeout: Duration::from_millis(300),
            h3_keep_alive_interval: Duration::from_millis(40),
            h3_max_idle_timeout: Duration::from_millis(120),
            h3_max_retries: 3,
            h3_cooldown: Duration::from_secs(60),
            extra_root_certs: vec![root_cert],
            ..Default::default()
        });

        let resp = transport
            .send_http(UpstreamHttpRequest {
                method: Method::POST,
                url: format!("https://localhost:{port}/v1/chat/completions")
                    .parse()
                    .unwrap(),
                headers: vec![],
                body: Bytes::from_static(b"{\"stream\":false}"),
                outbound_proxy: String::new(),
            })
            .await
            .unwrap();

        assert_eq!(resp.protocol, UpstreamProtocol::Http2);
        let body = resp.collect_body(1024).await.unwrap();
        assert_eq!(body.as_ref(), b"h2-rescue");
        // Initial H3 attempt + 3 retries = 4 total H3 attempts before H2 rescue.
        assert_eq!(h3_attempts.load(Ordering::Relaxed), 4);
        assert_eq!(h2_hits.load(Ordering::Relaxed), 1);
        assert!(transport.is_h3_cooled_down("localhost", port).await);

        h3j.abort();
        h2j.abort();
    }

    #[tokio::test]
    async fn h2_extended_connect_websocket_roundtrip_and_snapshot() {
        let (certs, key, root_cert) = generate_localhost_cert();

        let (port, h2j) = spawn_h2_server(
            0,
            certs,
            key,
            Duration::ZERO,
            true,
            |req, mut respond| async move {
                if req.method() == Method::GET {
                    respond.send_response(Response::new(()), true).unwrap();
                    return;
                }
                assert_eq!(req.method(), Method::CONNECT);
                let proto = req.extensions().get::<H2Protocol>().map(|p| p.as_str());
                assert_eq!(proto, Some("websocket"));
                let subproto = req
                    .headers()
                    .get("sec-websocket-protocol")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);

                let mut resp_builder = Response::builder().status(200);
                if let Some(sp) = subproto {
                    resp_builder = resp_builder.header("sec-websocket-protocol", sp);
                }
                let resp = resp_builder.body(()).unwrap();
                let send = respond.send_response(resp, false).unwrap();
                let recv = req.into_body();
                let io = H2ConnectIo::new(send, recv);
                let mut ws = WebSocketStream::from_raw_socket(io, Role::Server, None).await;
                while let Some(Ok(msg)) = ws.next().await {
                    match msg {
                        WsMessage::Text(txt) => {
                            let mut out = BytesMut::from("echo:");
                            out.extend_from_slice(txt.as_bytes());
                            let s = String::from_utf8(out.to_vec()).unwrap();
                            let _ = ws.send(WsMessage::Text(s.into())).await;
                        }
                        WsMessage::Binary(bin) => {
                            let _ = ws.send(WsMessage::Binary(bin)).await;
                        }
                        WsMessage::Close(_) => {
                            let _ = ws.close(None).await;
                            break;
                        }
                        _ => {}
                    }
                }
            },
        )
        .await;

        let transport = UpstreamTransport::new(UpstreamTransportConfig {
            enable_h3: false,
            session_stream_limit: 1,
            extra_root_certs: vec![root_cert],
            ..Default::default()
        });

        let url: reqwest::Url = format!("https://localhost:{port}/v1/responses").parse().unwrap();
        let headers = vec![(
            HeaderName::from_static("sec-websocket-protocol"),
            HeaderValue::from_static("openai-beta.realtime-v1"),
        )];
        let outcome = transport.connect_websocket(&url, &headers, "").await;
        let mut conn = match outcome {
            UpstreamWsOutcome::Connected(c) => c,
            UpstreamWsOutcome::Rejected { status, .. } => {
                panic!("unexpected rejection: {status}")
            }
            UpstreamWsOutcome::Unavailable(e) => panic!("unexpected unavailable: {e}"),
        };

        assert_eq!(conn.kind, UpstreamWsKind::H2ExtendedConnect);
        assert_eq!(
            conn.negotiated_protocol.as_deref(),
            Some("openai-beta.realtime-v1")
        );

        let snap = transport.snapshot();
        assert_eq!(snap.upstream_h2, 1);
        assert_eq!(snap.upstream_h2_ws, 1);
        assert_eq!(snap.upstream_h3_ws, 0);
        assert_eq!(snap.upstream_ws, 1);
        assert!(snap.upstream_streams >= 1);

        conn.ws.send(WsMessage::Text("hello".into())).await.unwrap();
        let reply = conn.ws.next().await.unwrap().unwrap();
        assert_eq!(reply, WsMessage::Text("echo:hello".into()));

        conn.ws
            .send(WsMessage::Binary(Bytes::from_static(&[1, 2, 3, 4])))
            .await
            .unwrap();
        let bin_reply = conn.ws.next().await.unwrap().unwrap();
        assert_eq!(
            bin_reply,
            WsMessage::Binary(Bytes::from_static(&[1, 2, 3, 4]))
        );

        let ws_session = snap.upstream_sessions[0].id;
        assert_eq!(snap.upstream_sessions[0].occupied_stream_slots, 1);

        let request = capacity_test_request(port);
        let http = transport.send_http(request.clone()).await.unwrap();
        let saturated = transport.snapshot().upstream_sessions;
        assert_eq!(saturated.len(), 2);
        assert!(saturated.iter().all(|s| s.occupied_stream_slots == 1));

        drop(conn);
        let reused = transport.send_http(request).await.unwrap();
        let both = transport.snapshot().upstream_sessions;
        assert_eq!(both.len(), 2);
        assert_eq!(both[0].id, ws_session);
        assert!(both.iter().all(|s| s.occupied_stream_slots == 1));
        drop((http, reused));
        let snap_after = transport.snapshot();
        assert_eq!(snap_after.upstream_h2_ws, 0);
        assert_eq!(snap_after.upstream_ws, 0);
        assert_eq!(snap_after.upstream_streams, 0);

        h2j.abort();
    }
}
