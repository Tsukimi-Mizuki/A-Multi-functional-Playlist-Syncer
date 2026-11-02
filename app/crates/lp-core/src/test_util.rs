//! 仅测试使用的小工具：不额外引入 `tempfile` 依赖。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 唯一的临时目录，离开作用域时递归删除。
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "lp-core-test-{}-{nanos}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)
            .unwrap_or_else(|e| panic!("创建测试临时目录失败 {}: {e}", path.display()));
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ---- 假 HTTP 传输 ----

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::netease::NeteaseClient;
use crate::netease::crypto;
use crate::netease::http::{BoxFuture, HttpRequest, HttpResponse, HttpTransport, TransportError};

/// 测试里统一使用的确定性 weapi secret。
pub const FIXED_SECRET: &str = "0123456789abcdef";

struct Reply {
    outcome: Result<HttpResponse, TransportError>,
    /// 设置后，响应会一直等到这个 `Notify` 被通知才返回（用于制造“请求在途”的窗口）。
    gate: Option<Arc<Notify>>,
}

/// 按序返回预设响应并记录请求的假传输（测试不访问真实网络）。
pub struct FakeTransport {
    replies: Mutex<VecDeque<Reply>>,
    requests: Mutex<Vec<HttpRequest>>,
}

impl FakeTransport {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(VecDeque::new()),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn push(&self, outcome: Result<HttpResponse, TransportError>, gate: Option<Arc<Notify>>) {
        self.replies
            .lock()
            .unwrap()
            .push_back(Reply { outcome, gate });
    }

    fn response(status: u16, body: &str, set_cookies: &[&str]) -> HttpResponse {
        HttpResponse {
            status,
            set_cookies: set_cookies.iter().map(|c| (*c).to_owned()).collect(),
            body: body.as_bytes().to_vec(),
        }
    }

    /// 200 + JSON 体。
    pub fn enqueue_json(&self, json: &str) -> &Self {
        self.enqueue_json_with_cookies(json, &[])
    }

    /// 200 + JSON 体 + 若干 `Set-Cookie`。
    pub fn enqueue_json_with_cookies(&self, json: &str, set_cookies: &[&str]) -> &Self {
        self.push(Ok(Self::response(200, json, set_cookies)), None);
        self
    }

    /// 指定状态码 + 空体。
    pub fn enqueue_status(&self, status: u16) -> &Self {
        self.enqueue_raw(status, "")
    }

    /// 指定状态码 + 原始体。
    pub fn enqueue_raw(&self, status: u16, body: &str) -> &Self {
        self.push(Ok(Self::response(status, body, &[])), None);
        self
    }

    /// 传输层失败。
    pub fn enqueue_error(&self, error: TransportError) -> &Self {
        self.push(Err(error), None);
        self
    }

    /// 200 + JSON 体，但要等 `gate` 被通知后才返回。
    pub fn enqueue_gated(&self, json: &str, gate: Arc<Notify>) -> &Self {
        self.enqueue_gated_with_cookies(json, &[], gate)
    }

    /// 同 [`enqueue_gated`](Self::enqueue_gated)，并带 `Set-Cookie`。
    pub fn enqueue_gated_with_cookies(
        &self,
        json: &str,
        set_cookies: &[&str],
        gate: Arc<Notify>,
    ) -> &Self {
        self.push(Ok(Self::response(200, json, set_cookies)), Some(gate));
        self
    }

    /// 目前为止收到的所有请求。
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// 还没被消费的预设响应数量。
    pub fn pending_replies(&self) -> usize {
        self.replies.lock().unwrap().len()
    }
}

impl HttpTransport for FakeTransport {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.clone());
            let reply = self.replies.lock().unwrap().pop_front().unwrap_or_else(|| {
                panic!(
                    "未预期的 HTTP 请求：{} {}",
                    request.method.as_str(),
                    request.url
                )
            });
            if let Some(gate) = reply.gate {
                gate.notified().await;
            }
            reply.outcome
        })
    }
}

/// 用确定性 secret 创建客户端。
pub fn test_client(transport: &Arc<FakeTransport>) -> NeteaseClient {
    let transport: Arc<dyn HttpTransport> = transport.clone();
    NeteaseClient::with_secret_factory(transport, || FIXED_SECRET.to_owned())
}

/// 用固定 secret 解开 weapi 请求里的业务 JSON。
pub fn decrypt_business_json(request: &HttpRequest, secret: &str) -> String {
    let params = request
        .form_value("params")
        .expect("请求缺少 params 表单字段");
    crypto::decrypt_payload(params, secret)
}
