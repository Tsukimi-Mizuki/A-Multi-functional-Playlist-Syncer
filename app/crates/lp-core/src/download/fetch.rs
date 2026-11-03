//! 通过 [`HttpTransport`] 下载音频与封面。对应 C# `MediaFileHelper.DownloadToFileAsync` / `TryDownloadBytesAsync`。
//!
//! 音频流式写进磁盘（无损单曲可达数十 MB，不整份读进内存）；封面是小文件，有大小上限。
//! 取消通过 [`cancellable`] 立即中断正在等待的网络读取。

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use crate::netease::client::BROWSER_USER_AGENT;
use crate::netease::http::{
    HttpMethod, HttpRequest, HttpStreamResponse, HttpTransport, TransportError,
};
use crate::utilities::{CancelToken, Cancelled, cancellable, error_text};

/// 封面大小上限：超过就不要了（C# 默认 10 MB）。
pub const MAX_COVER_BYTES: u64 = 10 * 1024 * 1024;

/// 没有总长度时，每多下载这么多字节报告一次进度。
const UNKNOWN_LENGTH_REPORT_STEP: u64 = 256 * 1024;

/// 下载失败。`Display` 已经是面向用户的中文。
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// 调用方取消了下载。
    #[error("操作已取消")]
    Cancelled,
    /// 服务端返回了非 2xx。
    #[error("媒体下载失败：HTTP {0}")]
    Http(u16),
    /// 连接中断、超时、内容不完整等。
    #[error("{0}")]
    Network(String),
    /// 写临时文件失败；内容是 [`error_text`] 翻译过的说明。
    #[error("{0}")]
    Io(String),
}

impl From<Cancelled> for FetchError {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

/// 音频与封面的下载器。
#[derive(Clone)]
pub struct MediaFetcher {
    transport: Arc<dyn HttpTransport>,
}

impl MediaFetcher {
    /// 创建下载器；`transport` 应该是没有短总超时的下载专用传输（见 `ReqwestTransport::for_downloads`）。
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self { transport }
    }

    fn get(url: &str) -> HttpRequest {
        HttpRequest {
            method: HttpMethod::Get,
            url: url.to_owned(),
            headers: vec![("User-Agent".to_owned(), BROWSER_USER_AGENT.to_owned())],
            form: None,
        }
    }

    async fn open(
        &self,
        url: &str,
        cancel: &CancelToken,
    ) -> Result<HttpStreamResponse, FetchError> {
        cancellable(cancel, self.transport.send_stream(Self::get(url)))
            .await?
            .map_err(describe_transport_error)
    }

    /// 把 `url` 流式下载到 `destination`（覆盖已有文件），返回写入的字节数。
    ///
    /// `progress(已写入, 总长度)` 在每块数据之后调用（总长度未知时每 256 KB 一次）。失败或取消时
    /// 已写入的半截文件**不在这里删除**，由调用方统一清理临时文件。
    pub async fn download_to_file(
        &self,
        url: &str,
        destination: &Path,
        progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
        cancel: &CancelToken,
    ) -> Result<u64, FetchError> {
        let mut response = self.open(url, cancel).await?;
        if !response.is_success() {
            return Err(FetchError::Http(response.status));
        }

        let total = response.content_length.filter(|total| *total > 0);
        if total.is_some() {
            progress(0, total);
        }

        let io_error = |error: std::io::Error| {
            FetchError::Io(error_text::describe_write(
                &error,
                destination.parent().and_then(Path::to_str),
            ))
        };
        let mut file = File::create(destination).map_err(io_error)?;
        let mut written: u64 = 0;
        let mut last_reported: u64 = 0;
        loop {
            let chunk = cancellable(cancel, response.body.next_chunk())
                .await?
                .map_err(describe_transport_error)?;
            let Some(chunk) = chunk else {
                break;
            };
            file.write_all(&chunk).map_err(io_error)?;
            written += chunk.len() as u64;
            if total.is_some() || written - last_reported >= UNKNOWN_LENGTH_REPORT_STEP {
                last_reported = written;
                progress(written, total);
            }
        }
        file.flush().map_err(io_error)?;

        // 连接被对端提前关掉时，有的传输层会当作正常结束；长度对不上就是下载不完整。
        if let Some(total) = response.content_length
            && total > 0
            && written < total
        {
            return Err(FetchError::Network(format!(
                "下载中断：只收到 {written} / {total} 字节，请检查网络后重试。"
            )));
        }
        Ok(written)
    }

    /// 下载封面等小文件；任何失败（含超时、状态码非 2xx、超过 `max_bytes`）返回 `None`，不打断主流程。
    /// 只有调用方取消时返回 `Err`。
    pub async fn try_download_bytes(
        &self,
        url: Option<&str>,
        max_bytes: u64,
        cancel: &CancelToken,
    ) -> Result<Option<Vec<u8>>, Cancelled> {
        let Some(url) = url.map(str::trim).filter(|url| !url.is_empty()) else {
            return Ok(None);
        };
        let mut response = match self.open(url, cancel).await {
            Ok(response) => response,
            Err(FetchError::Cancelled) => return Err(Cancelled),
            Err(_) => return Ok(None),
        };
        if !response.is_success() {
            return Ok(None);
        }
        if response
            .content_length
            .is_some_and(|length| length > max_bytes)
        {
            return Ok(None);
        }

        let mut bytes: Vec<u8> = Vec::new();
        loop {
            match cancellable(cancel, response.body.next_chunk()).await? {
                Ok(Some(chunk)) => {
                    if bytes.len() as u64 + chunk.len() as u64 > max_bytes {
                        return Ok(None);
                    }
                    bytes.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(_) => return Ok(None),
            }
        }
        Ok((!bytes.is_empty()).then_some(bytes))
    }
}

fn describe_transport_error(error: TransportError) -> FetchError {
    match error {
        TransportError::Timeout => {
            FetchError::Network("下载超时：服务器长时间没有响应，请稍后重试。".to_owned())
        }
        TransportError::Network(detail) => {
            FetchError::Network(format!("下载失败：网络连接中断（{detail}）"))
        }
    }
}
