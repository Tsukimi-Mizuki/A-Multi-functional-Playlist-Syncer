//! HTTP 抽象。
//!
//! `NeteaseClient` 只依赖 [`HttpTransport`]：生产环境用 [`ReqwestTransport`]（Schannel / 系统证书 / 系统代理），
//! 单测用 `test_util::FakeTransport`（按序返回预设响应并记录请求），测试因此完全不联网。

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// HTTP 方法（目前只需要这两种）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// GET。
    Get,
    /// POST（表单体）。
    Post,
}

impl HttpMethod {
    /// 方法名，用于错误文案。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

/// 一次 HTTP 请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// 方法。
    pub method: HttpMethod,
    /// 完整 URL（含查询串）。
    pub url: String,
    /// 请求头，按添加顺序。
    pub headers: Vec<(String, String)>,
    /// `application/x-www-form-urlencoded` 表单字段（由传输层负责编码）；GET 为 `None`。
    pub form: Option<Vec<(String, String)>>,
    /// 原始请求体（JSON 等，调用方自己在 `headers` 里写 `Content-Type`）；与 `form` 互斥，没有则为 `None`。
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    /// 取指定请求头（名字不区分大小写）。
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// 取指定表单字段。
    pub fn form_value(&self, name: &str) -> Option<&str> {
        self.form
            .as_ref()?
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// 一次 HTTP 响应（响应体已完整读入内存）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// 状态码。
    pub status: u16,
    /// 所有 `Set-Cookie` 头原文（可能有多条）。
    pub set_cookies: Vec<String>,
    /// 响应体。
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// 状态码是否为 2xx。
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// 响应体按 UTF-8 解码（非法字节替换为 `U+FFFD`）。
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// 传输层失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// 传输层自己的超时。
    Timeout,
    /// 连接、TLS、读取等失败；内容是已整理好的细节。
    Network(String),
}

/// `dyn` 兼容的异步返回值。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 流式响应的响应体：一块一块地取，取完返回 `None`。丢弃它即放弃剩余内容。
pub trait BodyStream: Send {
    /// 取下一块数据；响应体结束返回 `Ok(None)`。取消安全：future 被丢弃后可以继续调用。
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, TransportError>>;
}

/// 响应头已到、响应体还在路上的 HTTP 响应（大文件下载用，不把整个文件读进内存）。
pub struct HttpStreamResponse {
    /// 状态码。
    pub status: u16,
    /// 响应头里声明的内容长度（未知为 `None`）。
    pub content_length: Option<u64>,
    /// 响应体。
    pub body: Box<dyn BodyStream>,
}

impl HttpStreamResponse {
    /// 状态码是否为 2xx。
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// 把一份已经在内存里的响应体包装成只有一块的流（默认实现与测试用）。
struct BufferedBody(Option<Vec<u8>>);

impl BodyStream for BufferedBody {
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, TransportError>> {
        let chunk = self.0.take().filter(|bytes| !bytes.is_empty());
        Box::pin(async move { Ok(chunk) })
    }
}

/// 发送一次 HTTP 请求。
pub trait HttpTransport: Send + Sync {
    /// 发送请求并读完整个响应。取消通过丢弃返回的 future 实现。
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>>;

    /// 发送请求，响应头到达就返回，响应体按块流式读取（音频下载用）。
    ///
    /// 默认实现退化为 [`send`](Self::send)：整份响应体读完后当作只有一块的流，所以只实现 `send` 的
    /// 测试传输也能用；生产实现 [`ReqwestTransport`] 覆盖它做真正的流式读取。
    fn send_stream(
        &self,
        request: HttpRequest,
    ) -> BoxFuture<'_, Result<HttpStreamResponse, TransportError>> {
        Box::pin(async move {
            let response = self.send(request).await?;
            Ok(HttpStreamResponse {
                status: response.status,
                content_length: Some(response.body.len() as u64),
                body: Box::new(BufferedBody(Some(response.body))),
            })
        })
    }
}

/// 生产环境的传输实现：`reqwest` + `native-tls`（Windows 上即 Schannel，使用系统证书与系统代理）。
///
/// 不启用 cookie 容器：Cookie 完全由 `NeteaseClient` 管理，避免退出登录后旧 `MUSIC_U` 继续被带出去。
#[derive(Debug, Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    /// 创建传输层。`timeout` 是单次请求的总超时上限（连接超时取 10 秒与它中的较小者）。
    pub fn new(timeout: Duration) -> Result<Self, TransportError> {
        let client = reqwest::Client::builder()
            .connect_timeout(timeout.min(Duration::from_secs(10)))
            .timeout(timeout)
            .build()
            .map_err(|error| TransportError::Network(describe_error(&error)))?;
        Ok(Self { client })
    }

    /// 创建音频下载用的传输层：整份下载最长 `total_timeout`（C# 版为 30 分钟），另外限制连接时间与“相邻两次
    /// 读到数据的间隔”`read_timeout`——网络断了很快就能发现，而不是等满总超时。
    pub fn for_downloads(
        connect_timeout: Duration,
        read_timeout: Duration,
        total_timeout: Duration,
    ) -> Result<Self, TransportError> {
        let client = reqwest::Client::builder()
            .connect_timeout(connect_timeout)
            .read_timeout(read_timeout)
            .timeout(total_timeout)
            .build()
            .map_err(|error| TransportError::Network(describe_error(&error)))?;
        Ok(Self { client })
    }
}

impl HttpTransport for ReqwestTransport {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        Box::pin(async move {
            let mut builder = match request.method {
                HttpMethod::Get => self.client.get(&request.url),
                HttpMethod::Post => self.client.post(&request.url),
            };
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }
            if let Some(form) = &request.form {
                builder = builder.form(form);
            }
            if let Some(body) = request.body.clone() {
                builder = builder.body(body);
            }

            let response = builder.send().await.map_err(map_error)?;
            let status = response.status().as_u16();
            let set_cookies = response
                .headers()
                .get_all(reqwest::header::SET_COOKIE)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .map(str::to_owned)
                .collect();
            let body = response.bytes().await.map_err(map_error)?.to_vec();
            Ok(HttpResponse {
                status,
                set_cookies,
                body,
            })
        })
    }

    fn send_stream(
        &self,
        request: HttpRequest,
    ) -> BoxFuture<'_, Result<HttpStreamResponse, TransportError>> {
        Box::pin(async move {
            let mut builder = match request.method {
                HttpMethod::Get => self.client.get(&request.url),
                HttpMethod::Post => self.client.post(&request.url),
            };
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }
            if let Some(form) = &request.form {
                builder = builder.form(form);
            }
            if let Some(body) = request.body.clone() {
                builder = builder.body(body);
            }

            let response = builder.send().await.map_err(map_error)?;
            Ok(HttpStreamResponse {
                status: response.status().as_u16(),
                content_length: response.content_length(),
                body: Box::new(ReqwestBody { response }),
            })
        })
    }
}

/// `reqwest` 响应体的逐块读取。
struct ReqwestBody {
    response: reqwest::Response,
}

impl BodyStream for ReqwestBody {
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, TransportError>> {
        Box::pin(async move {
            let chunk = self.response.chunk().await.map_err(map_error)?;
            Ok(chunk.map(|bytes| bytes.to_vec()))
        })
    }
}

fn map_error(error: reqwest::Error) -> TransportError {
    if error.is_timeout() {
        TransportError::Timeout
    } else {
        TransportError::Network(describe_error(&error))
    }
}

/// 把 `reqwest` 的错误链拼成一行：`error sending request: connection closed: …`。
fn describe_error(error: &reqwest::Error) -> String {
    use std::error::Error;

    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(inner) = source {
        let inner_text = inner.to_string();
        if !text.contains(&inner_text) {
            text.push_str(": ");
            text.push_str(&inner_text);
        }
        source = inner.source();
    }
    text
}
