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

/// 发送一次 HTTP 请求。
pub trait HttpTransport: Send + Sync {
    /// 发送请求并读完整个响应。取消通过丢弃返回的 future 实现。
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>>;
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
