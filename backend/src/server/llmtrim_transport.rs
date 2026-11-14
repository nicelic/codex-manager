use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use bytes::Bytes;
use futures_util::{stream::BoxStream, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::{body::Incoming, Request as HyperRequest, Uri};
use hyper_util::{
    client::legacy::{
        connect::{Connected, Connection},
        Client,
    },
    rt::{TokioExecutor, TokioIo, TokioTimer},
};
use std::{
    future::Future,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::net::TcpStream;
use tower_service::Service;

const LLMTRIM_PROXY_ADDR: &str = "127.0.0.1:43117";

pub type ByteStream = BoxStream<'static, Result<Bytes, std::io::Error>>;

pub struct TransportResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: ByteStream,
}

#[derive(Clone)]
pub struct LlmtrimConnector {
    address: SocketAddr,
}

pub struct LlmtrimStream {
    io: TokioIo<TcpStream>,
}

impl Service<Uri> for LlmtrimConnector {
    type Response = LlmtrimStream;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _destination: Uri) -> Self::Future {
        let address = self.address;
        Box::pin(async move {
            let stream = TcpStream::connect(address).await?;
            stream.set_nodelay(true)?;
            Ok(LlmtrimStream {
                io: TokioIo::new(stream),
            })
        })
    }
}

impl Connection for LlmtrimStream {
    fn connected(&self) -> Connected {
        self.io.connected().proxy(true)
    }
}

impl hyper::rt::Read for LlmtrimStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        hyper::rt::Read::poll_read(Pin::new(&mut self.io), cx, buf)
    }
}

impl hyper::rt::Write for LlmtrimStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        hyper::rt::Write::poll_write(Pin::new(&mut self.io), cx, data)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        hyper::rt::Write::poll_flush(Pin::new(&mut self.io), cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        hyper::rt::Write::poll_shutdown(Pin::new(&mut self.io), cx)
    }

    fn is_write_vectored(&self) -> bool {
        hyper::rt::Write::is_write_vectored(&self.io)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<Result<usize, std::io::Error>> {
        hyper::rt::Write::poll_write_vectored(Pin::new(&mut self.io), cx, bufs)
    }
}

pub type PlainClient = Client<LlmtrimConnector, Full<Bytes>>;

#[derive(Clone)]
pub enum LlmtrimTransport {
    Plain(PlainClient),
    /// Test-only and compatibility transport for a local HTTP proxy fixture.
    Reqwest(reqwest::Client),
}

impl LlmtrimTransport {
    pub fn plain() -> Self {
        Self::plain_at(
            LLMTRIM_PROXY_ADDR
                .parse()
                .expect("LLMTrim proxy address is valid"),
        )
    }

    pub fn plain_at(address: SocketAddr) -> Self {
        let mut builder = Client::builder(TokioExecutor::new());
        builder
            .pool_timer(TokioTimer::new())
            .pool_idle_timeout(Duration::from_secs(300))
            .pool_max_idle_per_host(16);
        Self::Plain(builder.build(LlmtrimConnector { address }))
    }

    pub fn from_reqwest(client: reqwest::Client) -> Self {
        Self::Reqwest(client)
    }

    pub async fn request(
        &self,
        method: &Method,
        target_url: &str,
        headers: &HeaderMap,
        body: Bytes,
    ) -> Result<TransportResponse, String> {
        match self {
            Self::Plain(client) => {
                self.request_plain(client, method, target_url, headers, body)
                    .await
            }
            Self::Reqwest(client) => {
                self.request_reqwest(client, method, target_url, headers, body)
                    .await
            }
        }
    }

    async fn request_plain(
        &self,
        client: &PlainClient,
        method: &Method,
        target_url: &str,
        headers: &HeaderMap,
        body: Bytes,
    ) -> Result<TransportResponse, String> {
        let uri = target_url
            .parse::<Uri>()
            .map_err(|err| format!("LLMTrim 目标 URI 无效: {err}"))?;
        if uri.scheme().is_none() || uri.authority().is_none() {
            return Err("LLMTrim 明文代理请求必须使用完整的绝对 URI".to_string());
        }
        let hyper_method = hyper::Method::from_bytes(method.as_str().as_bytes())
            .map_err(|err| format!("HTTP 方法无效: {err}"))?;
        let mut builder = HyperRequest::builder().method(hyper_method).uri(uri);
        for (name, value) in headers {
            if should_skip_request_header(name.as_str()) {
                continue;
            }
            builder = builder.header(name.as_str(), value.as_bytes());
        }
        if !body.is_empty() {
            builder = builder.header("content-length", body.len().to_string());
        }
        let request = builder
            .body(Full::new(body))
            .map_err(|err| format!("构造 LLMTrim HTTP/1.1 请求失败: {err}"))?;
        let response = client
            .request(request)
            .await
            .map_err(|err| format!("请求 LLMTrim 43117 失败: {err}"))?;
        Ok(convert_hyper_response(response))
    }

    async fn request_reqwest(
        &self,
        client: &reqwest::Client,
        method: &Method,
        target_url: &str,
        headers: &HeaderMap,
        body: Bytes,
    ) -> Result<TransportResponse, String> {
        let method = reqwest::Method::from_bytes(method.as_str().as_bytes())
            .map_err(|err| format!("HTTP 方法无效: {err}"))?;
        let mut request = client.request(method, target_url);
        for (name, value) in headers {
            if should_skip_request_header(name.as_str()) {
                continue;
            }
            request = request.header(name.as_str(), value.as_bytes());
        }
        let response = request
            .body(body)
            .send()
            .await
            .map_err(|err| format!("请求 LLMTrim 代理失败: {err}"))?;
        Ok(convert_reqwest_response(response))
    }
}

fn should_skip_request_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "connection"
            | "keep-alive"
            | "proxy-connection"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
    )
}

fn copy_headers<I, K, V>(headers: I) -> HeaderMap
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
{
    let mut output = HeaderMap::new();
    for (name, value) in headers {
        let Ok(name) = HeaderName::from_bytes(name.as_ref()) else {
            continue;
        };
        let Ok(value) = HeaderValue::from_bytes(value.as_ref()) else {
            continue;
        };
        output.append(name, value);
    }
    output
}

fn convert_hyper_response(response: hyper::Response<Incoming>) -> TransportResponse {
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let headers = copy_headers(
        response
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str().as_bytes().to_vec(), value.as_bytes().to_vec())),
    );
    let body = response
        .into_body()
        .into_data_stream()
        .map(|frame| frame.map_err(std::io::Error::other))
        .boxed();
    TransportResponse {
        status,
        headers,
        body,
    }
}

fn convert_reqwest_response(response: reqwest::Response) -> TransportResponse {
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let headers = copy_headers(
        response
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str().as_bytes().to_vec(), value.as_bytes().to_vec())),
    );
    let body = response
        .bytes_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other))
        .boxed();
    TransportResponse {
        status,
        headers,
        body,
    }
}
