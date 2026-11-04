//! 在线词源共用的 HTTP 小工具：独立超时、取消、状态码检查与封面下载。

use std::sync::Arc;
use std::time::Duration;

use crate::download::fetch::MAX_COVER_BYTES;
use crate::netease::http::{HttpMethod, HttpRequest, HttpResponse, HttpTransport, TransportError};
use crate::utilities::{CancelToken, Cancelled, cancellable};

/// 浏览器 User-Agent（与 C# `QqUserAgent` 相同）。
pub(crate) use crate::netease::client::BROWSER_USER_AGENT as BROWSER_UA;

/// 一次请求的失败原因（传输层 / 状态码 / 取消）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HttpFailure {
    /// 调用方取消了。
    Cancelled,
    /// 超过独立超时。
    Timeout(Duration),
    /// 连接 / TLS / 读取失败。
    Network(String),
    /// 服务端返回了非 2xx。
    Status(u16),
}

impl HttpFailure {
    /// 一行中文说明。
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Cancelled => "操作已取消".to_owned(),
            Self::Timeout(limit) => format!("请求超时（超过 {} 秒）", limit.as_secs().max(1)),
            Self::Network(detail) => format!("网络请求失败：{detail}"),
            Self::Status(status) => format!("HTTP {status}"),
        }
    }
}

impl From<Cancelled> for HttpFailure {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

/// 带独立超时的 HTTP 访问（QQ / 酷狗 / LRCLIB / 封面共用）。
#[derive(Clone)]
pub(crate) struct OnlineHttp {
    transport: Arc<dyn HttpTransport>,
    timeout: Duration,
}

impl OnlineHttp {
    pub(crate) fn new(transport: Arc<dyn HttpTransport>, timeout: Duration) -> Self {
        Self { transport, timeout }
    }

    pub(crate) fn timeout(&self) -> Duration {
        self.timeout
    }

    /// 不套独立超时、不处理取消地发送一次请求（调用方自己在外层统一套超时与取消）。
    pub(crate) async fn send_raw(&self, request: HttpRequest) -> Result<HttpResponse, HttpFailure> {
        match self.transport.send(request).await {
            Err(TransportError::Timeout) => Err(HttpFailure::Timeout(self.timeout)),
            Err(TransportError::Network(detail)) => Err(HttpFailure::Network(detail)),
            Ok(response) => Ok(response),
        }
    }

    /// 发送请求并读完响应体；不检查状态码。超时按 [`HttpFailure::Timeout`]，取消立即生效。
    pub(crate) async fn send(
        &self,
        request: HttpRequest,
        cancel: &CancelToken,
    ) -> Result<HttpResponse, HttpFailure> {
        let call = tokio::time::timeout(self.timeout, self.transport.send(request));
        match cancellable(cancel, call).await? {
            Err(_elapsed) => Err(HttpFailure::Timeout(self.timeout)),
            Ok(Err(TransportError::Timeout)) => Err(HttpFailure::Timeout(self.timeout)),
            Ok(Err(TransportError::Network(detail))) => Err(HttpFailure::Network(detail)),
            Ok(Ok(response)) => Ok(response),
        }
    }

    /// GET 并要求 2xx，返回响应体文本。
    pub(crate) async fn get_text(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
        cancel: &CancelToken,
    ) -> Result<String, HttpFailure> {
        let request = HttpRequest {
            method: HttpMethod::Get,
            url: url.to_owned(),
            headers,
            form: None,
            body: None,
        };
        let response = self.send(request, cancel).await?;
        if !response.is_success() {
            return Err(HttpFailure::Status(response.status));
        }
        Ok(response.text())
    }

    /// 下载封面：任何失败（含超时、非 2xx、超过 10 MB）返回 `None`，只有取消时返回 `Err`。
    ///
    /// 声明了过大的长度就不读响应体；没声明（或声明得不准）就边读边数。
    pub(crate) async fn try_download_cover(
        &self,
        url: Option<&str>,
        cancel: &CancelToken,
    ) -> Result<Option<Vec<u8>>, Cancelled> {
        let Some(url) = url.map(str::trim).filter(|url| !url.is_empty()) else {
            return Ok(None);
        };
        let request = HttpRequest {
            method: HttpMethod::Get,
            url: url.to_owned(),
            headers: vec![
                ("User-Agent".to_owned(), BROWSER_UA.to_owned()),
                ("Referer".to_owned(), "https://y.qq.com/".to_owned()),
            ],
            form: None,
            body: None,
        };

        let download = async {
            let mut response = self.transport.send_stream(request).await.ok()?;
            if !response.is_success()
                || response
                    .content_length
                    .is_some_and(|length| length > MAX_COVER_BYTES)
            {
                return None;
            }
            let mut bytes: Vec<u8> = Vec::new();
            while let Some(chunk) = response.body.next_chunk().await.ok()? {
                if bytes.len() as u64 + chunk.len() as u64 > MAX_COVER_BYTES {
                    return None;
                }
                bytes.extend_from_slice(&chunk);
            }
            (!bytes.is_empty()).then_some(bytes)
        };
        match cancellable(cancel, tokio::time::timeout(self.timeout, download)).await? {
            Ok(bytes) => Ok(bytes),
            Err(_elapsed) => Ok(None),
        }
    }
}
