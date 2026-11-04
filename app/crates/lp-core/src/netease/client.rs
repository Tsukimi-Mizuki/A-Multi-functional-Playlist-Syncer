//! 网易云 web / weapi 客户端的通用部分：会话持有、请求发送、错误映射、Set-Cookie 合并。
//!
//! 对应 C# 的 `NeteaseApiClient.cs`。Cookie 由本客户端自行管理（传输层不带 cookie 容器）。
//! 各端点按文件拆分：登录在 [`login`](super::login)，后续模块会陆续加歌单 / 歌曲 / 歌词 / 专辑 / 搜索。

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use super::cookie_merger::{self, parse_cookie_string};
use super::crypto;
use super::error::{NeteaseError, NeteaseErrorKind};
use super::http::{HttpMethod, HttpRequest, HttpResponse, HttpTransport, TransportError};
use super::json;
use crate::session::NeteaseSession;

/// 业务 code：成功。
pub const WEAPI_CODE_SUCCESS: i32 = 200;

/// weapi / 公开 API 使用的浏览器 User-Agent。
pub const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

/// 新版桌面客户端 User-Agent（扫码登录 interface 域名使用）。
pub const DESKTOP_USER_AGENT: &str = "NeteaseMusicDesktop/3.0.18";

/// 匿名请求的固定基础 Cookie（社区惯例，非必需）。
pub const ANONYMOUS_COOKIE: &str = "os=pc; appver=8.9.70";

/// 播放 / 下载地址接口必须带上桌面客户端 Cookie（os / appver / deviceId），
/// 否则服务端会按网页端策略把 Hi-Res 及以上档位静默标成 lossless。
pub const PLAYBACK_COOKIE_SUFFIX: &str = "os=pc; appver=2.10.6; osver=Microsoft-Windows-10-Professional-build-19045-64bit; deviceId=LunaticPrincessPc; channel=netease";

/// 主站地址。
pub const MUSIC_BASE_URL: &str = "https://music.163.com";
/// 客户端接口域名（扫码登录出码用）。
pub const INTERFACE_BASE_URL: &str = "https://interface.music.163.com";

const REFERER: &str = "https://music.163.com/";
/// 单次请求超时。
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// 订阅会话变化后拿到的编号，用于取消订阅。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubscriptionId(u64);

type SessionListener = Arc<dyn Fn(&NeteaseSession) + Send + Sync>;

struct SessionState {
    session: Option<NeteaseSession>,
    /// 每次显式切换登录态（含退出登录）都加一；在途旧请求的 Set-Cookie 据此识别并丢弃。
    generation: u64,
}

/// 扫码登录 key 与服务端 base / type 的对应关系（轮询必须回到出码时的地址与 type）。
#[derive(Debug, Clone)]
pub(crate) struct QrLoginContext {
    pub base_url: &'static str,
    pub qr_type: u8,
    pub created_at: Instant,
}

/// 网易云音乐 web / weapi 客户端（匿名 + 扫码登录态）。
pub struct NeteaseClient {
    transport: Arc<dyn HttpTransport>,
    secret_factory: Box<dyn Fn() -> String + Send + Sync>,
    request_timeout: Duration,
    state: Mutex<SessionState>,
    listeners: Mutex<Vec<(u64, SessionListener)>>,
    next_listener_id: AtomicU64,
    pub(crate) qr_contexts: Mutex<HashMap<String, QrLoginContext>>,
}

impl std::fmt::Debug for NeteaseClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NeteaseClient")
            .field("logged_in", &self.is_logged_in())
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl NeteaseClient {
    /// 使用随机 secret 创建客户端。
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self::with_secret_factory(transport, crypto::create_secret)
    }

    /// 允许注入确定性的 weapi secret（测试用）。
    pub fn with_secret_factory(
        transport: Arc<dyn HttpTransport>,
        secret_factory: impl Fn() -> String + Send + Sync + 'static,
    ) -> Self {
        Self {
            transport,
            secret_factory: Box::new(secret_factory),
            request_timeout: REQUEST_TIMEOUT,
            state: Mutex::new(SessionState {
                session: None,
                generation: 0,
            }),
            listeners: Mutex::new(Vec::new()),
            next_listener_id: AtomicU64::new(1),
            qr_contexts: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn qr_contexts_lock(&self) -> MutexGuard<'_, HashMap<String, QrLoginContext>> {
        lock(&self.qr_contexts)
    }

    /// 当前登录态的副本。
    pub fn session(&self) -> Option<NeteaseSession> {
        lock(&self.state).session.clone()
    }

    /// 是否带有可用 Cookie。
    pub fn is_logged_in(&self) -> bool {
        lock(&self.state)
            .session
            .as_ref()
            .is_some_and(NeteaseSession::is_logged_in)
    }

    /// 切换登录态；传 `None` 表示退出登录。不会触发会话变化通知。
    pub fn set_session(&self, session: Option<NeteaseSession>) {
        let mut state = lock(&self.state);
        state.session = session;
        // 显式切换登录态后，仍在途的旧请求的 Set-Cookie 不得再合并进新会话。
        state.generation += 1;
    }

    /// 订阅“登录 Cookie 被服务端续期 / 轮换”。
    ///
    /// 显式 [`set_session`](Self::set_session) 与扫码登录不触发；回调在发出请求的任务里同步执行，不要阻塞。
    pub fn subscribe_session_changed(
        &self,
        listener: impl Fn(&NeteaseSession) + Send + Sync + 'static,
    ) -> SubscriptionId {
        let id = self.next_listener_id.fetch_add(1, Ordering::Relaxed);
        lock(&self.listeners).push((id, Arc::new(listener)));
        SubscriptionId(id)
    }

    /// 取消订阅。
    pub fn unsubscribe_session_changed(&self, id: SubscriptionId) {
        lock(&self.listeners).retain(|(existing, _)| *existing != id.0);
    }

    /// 同时取得当前登录态与其代数（用于判断请求期间登录态是否被显式切换）。
    fn snapshot_session(&self) -> (Option<NeteaseSession>, u64) {
        let state = lock(&self.state);
        (state.session.clone(), state.generation)
    }

    /// 把响应里的 Set-Cookie 合并进当前登录态；Cookie 真的变化时更新会话并通知订阅者。
    /// 匿名请求、登录态已被显式切换、或合并后会丢失 `MUSIC_U`（疑似误删）时不处理。
    fn absorb_set_cookies(&self, set_cookies: &[String], generation: u64) {
        if set_cookies.is_empty() {
            return;
        }

        let updated = {
            let mut state = lock(&self.state);
            let generation_matches = state.generation == generation;
            let Some(current) = state.session.as_ref().filter(|s| s.is_logged_in()) else {
                return;
            };
            if !generation_matches {
                return;
            }

            let merged = cookie_merger::merge(Some(&current.cookie), set_cookies);
            if !merged.changed {
                return;
            }
            if current.cookie_value("MUSIC_U").is_some() && !has_cookie(&merged.cookie, "MUSIC_U") {
                // 服务端偶发会对非登录相关请求下发清除指令；丢掉 MUSIC_U 会让登录彻底失效，宁可保留旧值。
                return;
            }

            let updated = NeteaseSession {
                cookie: merged.cookie,
                ..current.clone()
            };
            state.session = Some(updated.clone());
            updated
        };

        let listeners: Vec<SessionListener> = lock(&self.listeners)
            .iter()
            .map(|(_, listener)| Arc::clone(listener))
            .collect();
        for listener in listeners {
            // 订阅方（落盘）的异常不能影响正在进行的业务请求。
            let _ = catch_unwind(AssertUnwindSafe(|| listener(&updated)));
        }
    }

    /// 用接口返回的账号资料更新内存里的会话（不改 Cookie，不触发通知）。
    pub(crate) fn update_session_from_user(&self, user: &super::models::NeteaseUser) {
        let mut state = lock(&self.state);
        let Some(current) = state.session.as_ref().filter(|s| s.is_logged_in()) else {
            return;
        };
        let updated = NeteaseSession {
            cookie: current.cookie.clone(),
            nickname: Some(user.nickname.clone()),
            user_id: user.user_id,
            vip_type: user.vip_type,
            avatar_url: user
                .avatar_url
                .clone()
                .or_else(|| current.avatar_url.clone()),
            logged_in_at: current.logged_in_at.clone(),
        };
        state.session = Some(updated);
    }

    /// 把当前登录态拼成 Cookie 头；未登录时使用匿名基础 Cookie。
    pub fn build_cookie_header(session: Option<&NeteaseSession>) -> String {
        match session {
            Some(session) if !session.cookie.trim().is_empty() => session.cookie.clone(),
            _ => ANONYMOUS_COOKIE.to_owned(),
        }
    }

    /// 播放 / 下载接口使用的 Cookie 头：登录态 + 桌面客户端标识。
    pub fn build_playback_cookie_header(session: Option<&NeteaseSession>) -> String {
        // 登录态里已有的键（如 os / appver）以登录态原值为准，只追加缺失的客户端标识，避免重复键。
        let base = session
            .map(|s| s.cookie.trim().trim_end_matches(';').to_owned())
            .unwrap_or_default();
        let existing: Vec<String> = parse_cookie_string(&base)
            .into_iter()
            .map(|(name, _)| name.to_ascii_lowercase())
            .collect();
        let suffix = parse_cookie_string(PLAYBACK_COOKIE_SUFFIX)
            .into_iter()
            .filter(|(name, _)| !existing.contains(&name.to_ascii_lowercase()))
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");

        if base.is_empty() {
            suffix
        } else if suffix.is_empty() {
            base
        } else {
            format!("{base}; {suffix}")
        }
    }

    /// 构造 weapi 业务 JSON：原样 payload + 来自 Cookie 的 `csrf_token`。
    pub fn build_business_json(payload: &Value, csrf_token: &str) -> String {
        let mut body: Map<String, Value> = payload.as_object().cloned().unwrap_or_default();
        body.insert(
            "csrf_token".to_owned(),
            Value::String(csrf_token.to_owned()),
        );
        Value::Object(body).to_string()
    }

    /// 发送 weapi POST 请求并返回已校验 `code` 的 JSON。
    pub async fn post_weapi(
        &self,
        path: &str,
        payload: &Value,
        operation: &str,
        use_playback_cookie: bool,
    ) -> Result<Value, NeteaseError> {
        let (session, generation) = self.snapshot_session();
        let secret = (self.secret_factory)();
        let csrf = session
            .as_ref()
            .map(NeteaseSession::csrf_token)
            .unwrap_or_default();
        let business_json = Self::build_business_json(payload, &csrf);
        let (params, enc_sec_key) = crypto::encrypt(&business_json, &secret)
            .ok_or_else(|| NeteaseError::protocol("weapi secret 必须是 16 个字符"))?;

        let mut request = HttpRequest {
            method: HttpMethod::Post,
            url: format!("{MUSIC_BASE_URL}{path}"),
            headers: Vec::new(),
            form: Some(vec![
                ("params".to_owned(), params),
                ("encSecKey".to_owned(), enc_sec_key),
            ]),
            body: None,
        };
        apply_headers(
            &mut request,
            session.as_ref(),
            BROWSER_USER_AGENT,
            use_playback_cookie,
        );

        let response = self.send(request).await?;
        self.absorb_set_cookies(&response.set_cookies, generation);
        let document = parse_json_or_throw(&response, operation)?;
        ensure_success_code(&document, operation)?;
        Ok(document)
    }

    /// 以桌面客户端的身份发送**匿名** eapi POST 请求（目前只有搜索用），返回已校验 `code` 的 JSON。
    ///
    /// `base_url` 是主机（`https://interface.music.163.com`）；`api_path` 是网页风格的接口路径（`/api/cloudsearch/pc`），
    /// 摘要与密文基于它，实际 URL 是把 `/api/` 换成 `/eapi/` 的结果。请求体里会补一个 `header` 对象
    /// （客户端标识、`buildver`、`requestId`），同样的字段也以 Cookie 发出，与官方客户端一致。
    ///
    /// 不读取、不带、也不合并登录态：搜索不需要账号，匿名请求不会拿账号去撞风控或限流。
    pub(crate) async fn post_eapi(
        &self,
        base_url: &str,
        api_path: &str,
        payload: &Value,
        operation: &str,
    ) -> Result<Value, NeteaseError> {
        let request = build_eapi_request(
            base_url,
            api_path,
            payload,
            eapi_client_header(),
            BROWSER_USER_AGENT,
            true,
        )?;

        let response = self.send(request).await?;
        let document = parse_json_or_throw(&response, operation)?;
        ensure_success_code(&document, operation)?;
        Ok(document)
    }

    /// 以桌面客户端的身份、**带登录态**发送 eapi POST 请求（目前只有登录续期用），返回已校验 `code` 的 JSON。
    ///
    /// 格式与 [`post_eapi`](Self::post_eapi) 相同，区别在于 `header` 对象（同时作为 Cookie）里带上当前会话的
    /// `__csrf`、`MUSIC_U`、`NMTID`——服务端靠它们认出是谁在续期；UA 用桌面客户端的，并且不发 `Referer`
    /// （官方客户端的 eapi 请求没有）。响应里的 `Set-Cookie` 经 [`CookieMerger`](cookie_merger) 并进会话，
    /// 与 weapi 请求一样会通知订阅者落盘。没有登录态时直接返回协议错误，不发请求。
    pub(crate) async fn post_eapi_with_session(
        &self,
        base_url: &str,
        api_path: &str,
        payload: &Value,
        operation: &str,
    ) -> Result<Value, NeteaseError> {
        let (session, generation) = self.snapshot_session();
        let session = session
            .filter(NeteaseSession::is_logged_in)
            .ok_or_else(|| NeteaseError::protocol("当前没有登录态"))?;

        let mut header = eapi_client_header();
        header.insert("__csrf".to_owned(), Value::String(session.csrf_token()));
        for name in ["MUSIC_U", "NMTID"] {
            if let Some(value) = session.cookie_value(name).filter(|v| !v.is_empty()) {
                header.insert(name.to_owned(), Value::String(value));
            }
        }
        let request = build_eapi_request(
            base_url,
            api_path,
            payload,
            header,
            DESKTOP_USER_AGENT,
            false,
        )?;

        let response = self.send(request).await?;
        self.absorb_set_cookies(&response.set_cookies, generation);
        let document = parse_json_or_throw(&response, operation)?;
        ensure_success_code(&document, operation)?;
        Ok(document)
    }

    /// 发送 GET 请求并返回已校验 `code` 的 JSON。
    pub async fn get_json(&self, url: &str, operation: &str) -> Result<Value, NeteaseError> {
        let (session, generation) = self.snapshot_session();
        let mut request = HttpRequest {
            method: HttpMethod::Get,
            url: url.to_owned(),
            headers: Vec::new(),
            form: None,
            body: None,
        };
        apply_headers(&mut request, session.as_ref(), BROWSER_USER_AGENT, false);

        let response = self.send(request).await?;
        self.absorb_set_cookies(&response.set_cookies, generation);
        let document = parse_json_or_throw(&response, operation)?;
        ensure_success_code(&document, operation)?;
        Ok(document)
    }

    /// 发送裸表单 POST（扫码登录接口不走 weapi 加密）。
    pub(crate) async fn post_plain_form(
        &self,
        url: &str,
        form: Vec<(String, String)>,
        use_desktop_user_agent: bool,
    ) -> Result<HttpResponse, NeteaseError> {
        let session = self.session();
        let mut request = HttpRequest {
            method: HttpMethod::Post,
            url: url.to_owned(),
            headers: Vec::new(),
            form: Some(form),
            body: None,
        };
        let user_agent = if use_desktop_user_agent {
            DESKTOP_USER_AGENT
        } else {
            BROWSER_USER_AGENT
        };
        apply_headers(&mut request, session.as_ref(), user_agent, false);
        self.send(request).await
    }

    /// 发送请求，套上独立超时，并把传输层失败映射成带中文说明的 [`NeteaseError`]。
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, NeteaseError> {
        let description = format!("{} {}", request.method.as_str(), request.url);
        let timeout_error = || {
            NeteaseError::new(
                format!(
                    "请求超时（超过 {} 秒）：{description}",
                    self.request_timeout.as_secs()
                ),
                None,
                NeteaseErrorKind::Timeout,
            )
        };

        match tokio::time::timeout(self.request_timeout, self.transport.send(request)).await {
            Err(_) => Err(timeout_error()),
            Ok(Err(TransportError::Timeout)) => Err(timeout_error()),
            Ok(Err(TransportError::Network(detail))) => Err(NeteaseError::new(
                format!("网络请求失败：{detail}"),
                None,
                NeteaseErrorKind::Network,
            )),
            Ok(Ok(response)) => Ok(response),
        }
    }
}

fn has_cookie(cookie: &str, name: &str) -> bool {
    parse_cookie_string(cookie)
        .iter()
        .any(|(existing, value)| existing.eq_ignore_ascii_case(name) && !value.is_empty())
}

fn apply_headers(
    request: &mut HttpRequest,
    session: Option<&NeteaseSession>,
    user_agent: &str,
    use_playback_cookie: bool,
) {
    let cookie = if use_playback_cookie {
        NeteaseClient::build_playback_cookie_header(session)
    } else {
        NeteaseClient::build_cookie_header(session)
    };
    push_common_headers(request, user_agent, cookie, true);
}

fn push_common_headers(
    request: &mut HttpRequest,
    user_agent: &str,
    cookie: String,
    with_referer: bool,
) {
    request
        .headers
        .push(("User-Agent".to_owned(), user_agent.to_owned()));
    if with_referer {
        request
            .headers
            .push(("Referer".to_owned(), REFERER.to_owned()));
    }
    request.headers.extend([
        ("Accept".to_owned(), "*/*".to_owned()),
        (
            "Accept-Language".to_owned(),
            "zh-CN,zh;q=0.9,en;q=0.8".to_owned(),
        ),
        ("Cookie".to_owned(), cookie),
    ]);
}

/// 组装 eapi 请求：`header` 对象写进业务 JSON（参与加密）并原样作为 Cookie 发出。
///
/// `api_path` 是网页风格的 `/api/...` 路径，实际 URL 把它换成 `/eapi/...`。
fn build_eapi_request(
    base_url: &str,
    api_path: &str,
    payload: &Value,
    header: Map<String, Value>,
    user_agent: &str,
    with_referer: bool,
) -> Result<HttpRequest, NeteaseError> {
    let route = api_path
        .strip_prefix("/api/")
        .ok_or_else(|| NeteaseError::protocol("eapi 接口路径必须以 /api/ 开头"))?;
    let cookie = header
        .iter()
        .map(|(name, value)| format!("{name}={}", value.as_str().unwrap_or_default()))
        .collect::<Vec<_>>()
        .join("; ");

    let mut body: Map<String, Value> = payload.as_object().cloned().unwrap_or_default();
    body.insert("header".to_owned(), Value::Object(header));
    let params = crypto::eapi_encrypt(api_path, &Value::Object(body).to_string());

    let mut request = HttpRequest {
        method: HttpMethod::Post,
        url: format!("{base_url}/eapi/{route}"),
        headers: Vec::new(),
        form: Some(vec![("params".to_owned(), params)]),
        body: None,
    };
    push_common_headers(&mut request, user_agent, cookie, with_referer);
    Ok(request)
}

/// eapi 请求里的客户端标识（`header` 对象，同时作为 Cookie）：桌面端身份取自 [`PLAYBACK_COOKIE_SUFFIX`]，
/// 另有每次都变的 `buildver`（Unix 秒）与 `requestId`（`<Unix 毫秒>_<4 位随机数>`）。
fn eapi_client_header() -> Map<String, Value> {
    use rand::RngExt;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let mut header = Map::new();
    header.insert("__csrf".to_owned(), Value::String(String::new()));
    for (name, value) in parse_cookie_string(PLAYBACK_COOKIE_SUFFIX) {
        header.insert(name, Value::String(value));
    }
    header.insert("versioncode".to_owned(), Value::String("140".to_owned()));
    header.insert("mobilename".to_owned(), Value::String(String::new()));
    header.insert(
        "buildver".to_owned(),
        Value::String(now.as_secs().to_string()),
    );
    header.insert(
        "resolution".to_owned(),
        Value::String("1920x1080".to_owned()),
    );
    header.insert(
        "requestId".to_owned(),
        Value::String(format!(
            "{}_{:04}",
            now.as_millis(),
            rand::rng().random_range(0..1000u32)
        )),
    );
    header
}

/// 非 2xx 抛 HTTP 状态错误；响应体不是合法 JSON 抛协议错误。
pub(crate) fn parse_json_or_throw(
    response: &HttpResponse,
    operation: &str,
) -> Result<Value, NeteaseError> {
    if !response.is_success() {
        return Err(NeteaseError::http_status(
            format!("{operation}失败：HTTP {}", response.status),
            response.status,
        ));
    }
    serde_json::from_slice(&response.body)
        .map_err(|_| NeteaseError::protocol(format!("{operation}失败：服务端返回的不是合法 JSON")))
}

/// 校验业务 `code`。部分公开接口不返回 `code`，按成功处理。
fn ensure_success_code(root: &Value, operation: &str) -> Result<(), NeteaseError> {
    let Some(code_value) = json::property(root, "code") else {
        return Ok(());
    };
    let code = json::as_i64(code_value).map_or(WEAPI_CODE_SUCCESS, |n| n as i32);
    if code == WEAPI_CODE_SUCCESS {
        Ok(())
    } else {
        Err(NeteaseError::api(
            format!("{operation}失败：接口返回 code={code}"),
            code,
        ))
    }
}

/// 与 .NET `Uri.EscapeDataString` 一致：保留 `A-Za-z0-9-_.~`，其余按 UTF-8 字节百分号编码。
pub(crate) fn escape_data_string(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            escaped.push(char::from(byte));
        } else {
            escaped.push_str(&format!("%{byte:02X}"));
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::test_util::{FIXED_SECRET, FakeTransport, decrypt_business_json, test_client};

    const ACCOUNT_JSON: &str = r#"{
        "code": 200,
        "account": { "id": 42, "userName": "test@example.com" },
        "profile": { "userId": 42, "nickname": "测试用户", "avatarUrl": "https://p1/avatar.jpg", "vipType": 1 }
    }"#;

    fn logged_in_client(transport: &Arc<FakeTransport>) -> NeteaseClient {
        let client = test_client(transport);
        client.set_session(Some(NeteaseSession {
            cookie: "MUSIC_U=music-u; __csrf=csrf-token".to_owned(),
            ..Default::default()
        }));
        client
    }

    #[tokio::test]
    async fn get_current_user_returns_none_for_anonymous_account() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":200,"account":null,"profile":null}"#);
        let client = test_client(&transport);

        let user = client.get_current_user().await.unwrap();

        assert!(user.is_none());
        assert!(!client.is_logged_in());
        assert!(client.session().is_none());
    }

    #[tokio::test]
    async fn get_current_user_maps_profile_and_updates_current_session() {
        let transport = FakeTransport::new();
        transport.enqueue_json(ACCOUNT_JSON);
        let client = logged_in_client(&transport);

        let user = client.get_current_user().await.unwrap().unwrap();

        assert_eq!(user.user_id, 42);
        assert_eq!(user.nickname, "测试用户");
        assert_eq!(user.vip_type, 1);
        assert_eq!(user.avatar_url.as_deref(), Some("https://p1/avatar.jpg"));

        let session = client.session().unwrap();
        assert_eq!(session.cookie, "MUSIC_U=music-u; __csrf=csrf-token");
        assert_eq!(session.user_id, 42);
        assert_eq!(session.nickname.as_deref(), Some("测试用户"));
        assert_eq!(session.vip_type, 1);
        assert!(session.is_logged_in());

        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].url.ends_with("/weapi/nuser/account/get"));
        assert!(
            requests[0]
                .header("Cookie")
                .unwrap()
                .contains("MUSIC_U=music-u")
        );
        let payload: Value =
            serde_json::from_str(&decrypt_business_json(&requests[0], FIXED_SECRET)).unwrap();
        assert_eq!(payload["csrf_token"], "csrf-token");
    }

    #[tokio::test]
    async fn weapi_request_uses_two_layer_encryption_and_deterministic_enc_sec_key() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":200,"account":null}"#);
        let client = test_client(&transport);

        client.get_current_user().await.unwrap();

        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.url, "https://music.163.com/weapi/nuser/account/get");
        let params = request.form_value("params").expect("缺少 params");
        let enc_sec_key = request.form_value("encSecKey").expect("缺少 encSecKey");
        assert_eq!(enc_sec_key, crypto::rsa_encrypt_hex(FIXED_SECRET));
        assert_eq!(enc_sec_key.len(), 256);
        assert_eq!(
            crypto::decrypt_payload(params, FIXED_SECRET),
            r#"{"csrf_token":""}"#
        );
    }

    #[tokio::test]
    async fn anonymous_request_sends_anonymous_base_cookie() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":200,"account":null}"#);
        let client = test_client(&transport);

        client.get_current_user().await.unwrap();

        let requests = transport.requests();
        assert_eq!(requests[0].header("Cookie"), Some(ANONYMOUS_COOKIE));
        assert!(
            requests[0]
                .header("User-Agent")
                .unwrap()
                .contains("Chrome/")
        );
        assert_eq!(
            requests[0].header("Referer"),
            Some("https://music.163.com/")
        );
    }

    #[test]
    fn set_session_controls_login_state() {
        let client = test_client(&FakeTransport::new());
        assert!(!client.is_logged_in());

        client.set_session(Some(NeteaseSession {
            cookie: "MUSIC_U=abc".to_owned(),
            ..Default::default()
        }));
        assert!(client.is_logged_in());
        assert!(client.session().is_some());

        client.set_session(None);
        assert!(!client.is_logged_in());
        assert!(client.session().is_none());
    }

    #[tokio::test]
    async fn http_error_status_becomes_error_with_status_code() {
        let transport = FakeTransport::new();
        transport.enqueue_status(502);
        let client = test_client(&transport);

        let error = client.get_current_user().await.unwrap_err();

        assert_eq!(error.code(), Some(502));
        assert!(error.is_http_status());
        assert!(error.message().contains("HTTP 502"));
    }

    #[tokio::test]
    async fn business_error_code_becomes_error_with_code() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":-460,"message":"Cheating"}"#);
        let client = test_client(&transport);

        let error = client.get_current_user().await.unwrap_err();

        assert_eq!(error.code(), Some(-460));
        assert!(error.message().contains("-460"));
    }

    #[tokio::test]
    async fn invalid_json_response_becomes_error() {
        let transport = FakeTransport::new();
        transport.enqueue_raw(200, "<html>blocked</html>");
        let client = test_client(&transport);

        let error = client.get_current_user().await.unwrap_err();

        assert_eq!(error.kind(), NeteaseErrorKind::Protocol);
    }

    #[tokio::test]
    async fn transport_failures_are_mapped_to_chinese_errors() {
        let transport = FakeTransport::new();
        transport.enqueue_error(TransportError::Network("connection reset".to_owned()));
        transport.enqueue_error(TransportError::Timeout);
        let client = test_client(&transport);

        let network = client.get_current_user().await.unwrap_err();
        assert_eq!(network.kind(), NeteaseErrorKind::Network);
        assert!(network.message().starts_with("网络请求失败："));
        assert!(network.is_transient_network());

        let timeout = client.get_current_user().await.unwrap_err();
        assert_eq!(timeout.kind(), NeteaseErrorKind::Timeout);
        assert!(timeout.message().starts_with("请求超时（超过 20 秒）"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_hanging_transport_hits_the_independent_timeout() {
        let transport = FakeTransport::new();
        // 响应被挡住且永远不放行：只有客户端自己的超时能让请求结束。
        transport.enqueue_gated(r#"{"code":200}"#, Arc::new(tokio::sync::Notify::new()));
        let client = test_client(&transport);

        let error = client.get_current_user().await.unwrap_err();

        assert_eq!(error.kind(), NeteaseErrorKind::Timeout);
    }

    #[test]
    fn session_survives_concurrent_reads_and_writes() {
        let client = Arc::new(test_client(&FakeTransport::new()));
        let handles: Vec<_> = (0..8)
            .map(|index| {
                let client = Arc::clone(&client);
                std::thread::spawn(move || {
                    for round in 0..50 {
                        client.set_session(Some(NeteaseSession {
                            cookie: format!("MUSIC_U=user{index}-{round}"),
                            user_id: index,
                            ..Default::default()
                        }));
                        let session = client.session();
                        assert!(
                            session.is_none_or(|s| s.cookie.starts_with("MUSIC_U=user")),
                            "读到了被撕裂的会话"
                        );
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert!(client.session().is_some());
    }

    #[test]
    fn business_json_keeps_payload_and_adds_csrf_token() {
        assert_eq!(
            NeteaseClient::build_business_json(&json!({}), ""),
            r#"{"csrf_token":""}"#
        );
        let value: Value = serde_json::from_str(&NeteaseClient::build_business_json(
            &json!({"id": 42, "csrf_token": "stale"}),
            "fresh",
        ))
        .unwrap();
        assert_eq!(value, json!({"id": 42, "csrf_token": "fresh"}));
    }

    #[test]
    fn escape_data_string_matches_dotnet_rules() {
        assert_eq!(escape_data_string("abcDEF123-_.~"), "abcDEF123-_.~");
        assert_eq!(escape_data_string("a b/c+中"), "a%20b%2Fc%2B%E4%B8%AD");
    }
}
