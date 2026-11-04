//! 扫码登录、读取当前账号、登录续期。对应 C# 的 `NeteaseApiClient.Login.cs`。

use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::client::{
    INTERFACE_BASE_URL, MUSIC_BASE_URL, NeteaseClient, QrLoginContext, WEAPI_CODE_SUCCESS,
    escape_data_string, parse_json_or_throw,
};
use super::error::NeteaseError;
use super::http::HttpResponse;
use super::json;
use super::models::{NeteaseUser, QrLoginStart, QrLoginStatus};
use crate::session::NeteaseSession;

const QR_KEY_TYPE_DESKTOP: u8 = 3;
const QR_KEY_TYPE_WEB: u8 = 1;
/// 二维码有效期约 180 秒。
pub const QR_LIFETIME: Duration = Duration::from_secs(180);
/// 出码上下文保留时长，超过后不再需要。
const QR_CONTEXT_LIFETIME: Duration = Duration::from_secs(10 * 60);
/// 登录完成后读取账号资料失败时的重试间隔。
const PROFILE_RETRY_DELAY: Duration = Duration::from_millis(250);

impl NeteaseClient {
    /// 申请登录二维码：优先桌面端接口（`type=3`），不可用时降级到 web 端点（`type=1`）。
    pub async fn create_qr_login(&self) -> Result<QrLoginStart, NeteaseError> {
        self.prune_expired_qr_contexts();

        // 桌面端出码接口不可用（网络错误、非 2xx、没有 unikey）时都降级到 web 端点。
        let desktop = self
            .try_create_qr(INTERFACE_BASE_URL, QR_KEY_TYPE_DESKTOP, true)
            .await
            .ok()
            .flatten();
        let created = match desktop {
            Some(created) => Some(created),
            None => {
                self.try_create_qr(MUSIC_BASE_URL, QR_KEY_TYPE_WEB, false)
                    .await?
            }
        };

        let (key, base_url, qr_type) = created
            .ok_or_else(|| NeteaseError::protocol("获取登录二维码失败：接口未返回 unikey"))?;
        self.qr_contexts_lock().insert(
            key.clone(),
            QrLoginContext {
                base_url,
                qr_type,
                created_at: Instant::now(),
            },
        );
        Ok(QrLoginStart {
            qr_content: format!(
                "https://music.163.com/login?codekey={}",
                escape_data_string(&key)
            ),
            key,
            lifetime: QR_LIFETIME,
        })
    }

    /// 轮询扫码状态；803 时返回 [`QrLoginStatus::Confirmed`] 并携带登录态（已设置到本客户端）。
    pub async fn poll_qr_login(&self, key: &str) -> Result<QrLoginStatus, NeteaseError> {
        if key.trim().is_empty() {
            return Err(NeteaseError::protocol("二维码 key 不能为空。"));
        }

        // 进程重启后丢失出码上下文时按 web 端点 type=1 保守轮询。
        let (base_url, qr_type) = self
            .qr_contexts_lock()
            .get(key)
            .map(|context| (context.base_url, context.qr_type))
            .unwrap_or((MUSIC_BASE_URL, QR_KEY_TYPE_WEB));

        let url = format!(
            "{base_url}/api/login/qrcode/client/login?key={}&type={qr_type}",
            escape_data_string(key)
        );
        let response = self
            .post_plain_form(&url, Vec::new(), qr_type == QR_KEY_TYPE_DESKTOP)
            .await?;

        let root = parse_json_or_throw(&response, "轮询扫码状态")?;
        let code = json::i32_or(&root, "code", -1);
        let api_message =
            json::string_or_none(&root, "message").or_else(|| json::string_or_none(&root, "msg"));

        match code {
            800 => {
                self.forget_qr_context(key);
                Ok(QrLoginStatus::Expired)
            }
            801 => Ok(QrLoginStatus::Waiting),
            802 => Ok(QrLoginStatus::Scanned),
            803 => {
                self.forget_qr_context(key);
                Ok(self.complete_qr_login(&root, &response).await)
            }
            _ => {
                self.forget_qr_context(key);
                let detail = api_message
                    .filter(|m| !m.trim().is_empty())
                    .map(|m| format!("：{m}"))
                    .unwrap_or_default();
                Ok(QrLoginStatus::Failed {
                    message: format!("扫码登录失败（code={code}{detail}）"),
                })
            }
        }
    }

    /// 读取当前账号。
    ///
    /// 接口 HTTP 200 且 `code=200` 但 account / profile 为空时返回 `Ok(None)`（明确未登录）；
    /// 其余失败（HTTP 错误、限流、风控、网络）一律返回 `Err`，调用方不应据此判定登录失效。
    pub async fn get_current_user(&self) -> Result<Option<NeteaseUser>, NeteaseError> {
        let root = self
            .post_weapi(
                "/weapi/nuser/account/get",
                &json!({}),
                "获取当前用户",
                false,
            )
            .await?;

        let account = match json::property(&root, "account") {
            None | Some(Value::Null) => return Ok(None),
            Some(account) => account,
        };

        let profile = match json::property(&root, "profile") {
            Some(profile) if profile.is_object() => profile,
            _ => {
                // 极少数响应只有 account 没有 profile：仍然视为已登录。
                let account_id = json::i64_or(account, "id", 0);
                if account_id <= 0 {
                    return Ok(None);
                }
                let user = NeteaseUser {
                    user_id: account_id,
                    nickname: json::string_or_none(account, "userName").unwrap_or_default(),
                    avatar_url: None,
                    vip_type: 0,
                };
                self.update_session_from_user(&user);
                return Ok(Some(user));
            }
        };

        let user_id = json::i64_or(profile, "userId", json::i64_or(account, "id", 0));
        if user_id <= 0 {
            return Ok(None);
        }
        let user = NeteaseUser {
            user_id,
            nickname: json::string_or_none(profile, "nickname")
                .or_else(|| json::string_or_none(account, "userName"))
                .unwrap_or_default(),
            avatar_url: json::string_or_none(profile, "avatarUrl"),
            vip_type: json::i32_or(profile, "vipType", 0),
        };
        self.update_session_from_user(&user);
        Ok(Some(user))
    }

    /// 请求服务端续期登录态（eapi `login/token/refresh`），Set-Cookie 由请求层统一合并并通知落盘。
    ///
    /// 失败（未登录、网络、接口拒绝）只返回 `false`，不返回错误；需要失败原因时用
    /// [`try_refresh_login`](Self::try_refresh_login)。
    pub async fn refresh_login(&self) -> bool {
        self.try_refresh_login().await.is_ok()
    }

    /// 同 [`refresh_login`](Self::refresh_login)，但把失败原因带出来，供日志说明“为什么续期没成功”。
    ///
    /// 必须走 **eapi**（`https://interface.music.163.com/eapi/login/token/refresh`，签名用 `/api/login/token/refresh`）：
    /// 桌面端扫码得到的会话在 weapi 同名端点上要么返回 `code=400`（会话 Cookie 里没有桌面客户端标识），
    /// 要么返回 `code=200` 却不换发任何令牌（`bizCode=201`）；eapi 才会真正下发新的 `MUSIC_U`（约 180 天）、
    /// `__csrf` 与 `MUSIC_R_U`（2026-11 联网实测，旧令牌在换发后仍有效，可以连续续期）。
    /// `e_r:false` 让服务端返回明文 JSON。
    pub async fn try_refresh_login(&self) -> Result<(), NeteaseError> {
        self.post_eapi_with_session(
            INTERFACE_BASE_URL,
            "/api/login/token/refresh",
            &json!({ "e_r": false }),
            "续期登录状态",
        )
        .await
        .map(|_| ())
    }

    async fn complete_qr_login(&self, root: &Value, response: &HttpResponse) -> QrLoginStatus {
        let json_cookie = json::string_or_none(root, "cookie");
        let cookies = build_cookie_collection(&response.set_cookies, json_cookie.as_deref());
        if cookies.is_empty() {
            return QrLoginStatus::Failed {
                message: "扫码登录成功，但服务端未返回 Cookie".to_owned(),
            };
        }

        let pending = NeteaseSession {
            cookie: super::cookie_merger::join_cookie_pairs(&cookies),
            ..Default::default()
        };
        self.set_session(Some(pending.clone()));

        let mut user = None;
        let mut last_error: Option<NeteaseError> = None;
        for attempt in 0..2 {
            if attempt > 0 {
                tokio::time::sleep(PROFILE_RETRY_DELAY).await;
            }
            match self.get_current_user().await {
                Ok(Some(found)) => {
                    user = Some(found);
                    break;
                }
                Ok(None) => {}
                Err(error) => last_error = Some(error),
            }
        }

        // 803 已经拿到可用 Cookie：即使读取资料失败也要把 Cookie 登录态交给上层
        // （user_id 为 0，稍后可再调用 get_current_user 补齐）。
        let session = self.session().unwrap_or(pending);
        let message = match (&user, &last_error) {
            (Some(_), _) => "登录成功".to_owned(),
            (None, None) => "扫码登录成功，正在获取账号信息".to_owned(),
            (None, Some(error)) => {
                format!("扫码登录成功，但读取账号信息失败（可稍后重试）：{error}")
            }
        };
        QrLoginStatus::Confirmed { session, message }
    }

    async fn try_create_qr(
        &self,
        base_url: &'static str,
        qr_type: u8,
        use_desktop_user_agent: bool,
    ) -> Result<Option<(String, &'static str, u8)>, NeteaseError> {
        let url = format!("{base_url}/api/login/qrcode/unikey");
        let form = vec![("type".to_owned(), qr_type.to_string())];
        let response = self
            .post_plain_form(&url, form, use_desktop_user_agent)
            .await?;
        if !response.is_success() {
            return Ok(None);
        }
        let Ok(root) = serde_json::from_slice::<Value>(&response.body) else {
            return Ok(None);
        };
        if json::i32_or(&root, "code", 0) != WEAPI_CODE_SUCCESS {
            return Ok(None);
        }
        Ok(json::string_or_none(&root, "unikey")
            .filter(|key| !key.trim().is_empty())
            .map(|key| (key, base_url, qr_type)))
    }

    fn forget_qr_context(&self, key: &str) {
        self.qr_contexts_lock().remove(key);
    }

    fn prune_expired_qr_contexts(&self) {
        let mut contexts = self.qr_contexts_lock();
        contexts.retain(|_, context| context.created_at.elapsed() <= QR_CONTEXT_LIFETIME);
    }
}

/// 合并 Set-Cookie 头与 803 响应体里的 cookie 串。Set-Cookie 头优先于响应体；
/// 同一来源内的重复键以后到的为准（后出现的 Set-Cookie 才是服务端最终要保留的值）。
pub(crate) fn build_cookie_collection(
    set_cookie_headers: &[String],
    json_cookie: Option<&str>,
) -> Vec<(String, String)> {
    let mut cookies: Vec<(String, String)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut from_headers: HashSet<String> = HashSet::new();

    let mut add = |name: Option<&str>, value: Option<&str>| {
        let (Some(name), Some(value)) = (name, value) else {
            return;
        };
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty() || value.is_empty() {
            return;
        }
        if seen.insert(name.to_ascii_lowercase()) {
            cookies.push((name.to_owned(), value.to_owned()));
        } else if let Some(existing) = cookies
            .iter_mut()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(name))
        {
            existing.1 = value.to_owned();
        }
    };

    for header in set_cookie_headers {
        let (name, value) = parse_cookie_pair(header);
        add(name, value);
        if let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) {
            from_headers.insert(name.to_ascii_lowercase());
        }
    }

    if let Some(json_cookie) = json_cookie.filter(|c| !c.trim().is_empty()) {
        for part in json_cookie
            .split(';')
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let (name, value) = parse_cookie_pair(part);
            // 响应体只补齐头里没有的键；响应体内部重复则后者为准。
            if name
                .map(|n| from_headers.contains(&n.trim().to_ascii_lowercase()))
                .unwrap_or(false)
            {
                continue;
            }
            add(name, value);
        }
    }

    // 部分登录响应只下发 __csrf_token；weapi 写接口要求 __csrf。
    let csrf_token = cookies
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("__csrf_token"))
        .map(|(_, value)| value.clone());
    if let Some(value) = csrf_token
        && !seen.contains("__csrf")
    {
        cookies.push(("__csrf".to_owned(), value));
    }

    cookies
}

/// `Set-Cookie: MUSIC_U=xxx; Path=/; HttpOnly` —— 只取第一段 `name=value`。
fn parse_cookie_pair(raw: &str) -> (Option<&str>, Option<&str>) {
    if raw.trim().is_empty() {
        return (None, None);
    }
    let segment = raw.split(';').next().unwrap_or_default().trim();
    match segment.find('=').filter(|&index| index > 0) {
        Some(index) => (
            Some(segment[..index].trim()),
            Some(segment[index + 1..].trim()),
        ),
        None => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::netease::error::NeteaseErrorKind;
    use crate::test_util::{FakeTransport, test_client};

    const ACCOUNT_JSON: &str = r#"{
        "code": 200,
        "account": { "id": 42, "userName": "test@example.com" },
        "profile": {
            "userId": 42,
            "nickname": "测试用户",
            "avatarUrl": "https://p1.music.126.net/avatar.jpg",
            "vipType": 1
        }
    }"#;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn url_has(request: &crate::netease::http::HttpRequest, fragment: &str) -> bool {
        request.url.contains(fragment)
    }

    #[tokio::test]
    async fn create_qr_login_uses_desktop_type3_and_maps_qr_content() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":200,"unikey":"abcDEF123456"}"#);
        transport.enqueue_json(r#"{"code":801}"#);
        let client = test_client(&transport);

        let start = client.create_qr_login().await.unwrap();

        assert_eq!(start.key, "abcDEF123456");
        assert_eq!(
            start.qr_content,
            "https://music.163.com/login?codekey=abcDEF123456"
        );
        assert_eq!(start.lifetime, Duration::from_secs(180));

        let create = transport.requests();
        assert_eq!(create.len(), 1);
        assert_eq!(
            create[0].url,
            "https://interface.music.163.com/api/login/qrcode/unikey"
        );
        assert_eq!(create[0].form_value("type"), Some("3"));
        assert!(
            create[0]
                .header("User-Agent")
                .unwrap()
                .contains("NeteaseMusicDesktop")
        );
        assert_eq!(create[0].header("Referer"), Some("https://music.163.com/"));

        // 轮询必须回到出码时的 host 和 type。
        let status = client.poll_qr_login(&start.key).await.unwrap();

        assert_eq!(status, QrLoginStatus::Waiting);
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[1]
                .url
                .starts_with("https://interface.music.163.com/api/login/qrcode/client/login?")
        );
        assert!(url_has(&requests[1], "key=abcDEF123456"));
        assert!(url_has(&requests[1], "type=3"));
    }

    #[tokio::test]
    async fn create_qr_login_falls_back_to_web_type1_when_desktop_endpoint_fails() {
        let transport = FakeTransport::new();
        transport.enqueue_status(500);
        transport.enqueue_json(r#"{"code":200,"unikey":"webKey"}"#);
        transport.enqueue_json(r#"{"code":801}"#);
        let client = test_client(&transport);

        let start = client.create_qr_login().await.unwrap();
        let status = client.poll_qr_login(&start.key).await.unwrap();

        assert_eq!(start.key, "webKey");
        assert_eq!(status, QrLoginStatus::Waiting);
        let requests = transport.requests();
        assert_eq!(requests.len(), 3);
        assert!(
            requests[0]
                .url
                .starts_with("https://interface.music.163.com/")
        );
        assert!(requests[1].url.starts_with("https://music.163.com/"));
        assert_eq!(requests[1].form_value("type"), Some("1"));
        assert!(requests[2].url.starts_with("https://music.163.com/"));
        assert!(url_has(&requests[2], "type=1"));
    }

    #[tokio::test]
    async fn create_qr_login_falls_back_when_desktop_endpoint_has_a_network_error() {
        let transport = FakeTransport::new();
        transport.enqueue_error(crate::netease::http::TransportError::Network(
            "x".to_owned(),
        ));
        transport.enqueue_json(r#"{"code":200,"unikey":"webKey"}"#);
        let client = test_client(&transport);

        assert_eq!(client.create_qr_login().await.unwrap().key, "webKey");
    }

    #[tokio::test]
    async fn create_qr_login_fails_when_both_endpoints_are_unavailable() {
        let transport = FakeTransport::new();
        transport.enqueue_status(500);
        transport.enqueue_json(r#"{"code":500}"#);
        let client = test_client(&transport);

        let error = client.create_qr_login().await.unwrap_err();

        assert!(error.message().contains("未返回 unikey"));
    }

    #[tokio::test]
    async fn poll_qr_login_maps_server_codes() {
        for (code, expected) in [
            (800, "expired"),
            (801, "waiting"),
            (802, "scanned"),
            (999, "failed"),
        ] {
            let transport = FakeTransport::new();
            transport.enqueue_json(&format!(r#"{{"code":{code},"message":"x"}}"#));
            let client = test_client(&transport);

            let status = client.poll_qr_login("some-key").await.unwrap();

            let actual = match &status {
                QrLoginStatus::Expired => "expired",
                QrLoginStatus::Waiting => "waiting",
                QrLoginStatus::Scanned => "scanned",
                QrLoginStatus::Failed { .. } => "failed",
                QrLoginStatus::Confirmed { .. } => "confirmed",
            };
            assert_eq!(actual, expected, "code {code}");
            assert!(!status.message().trim().is_empty());
        }
    }

    #[tokio::test]
    async fn failed_status_carries_code_and_server_message() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":999,"message":"风控"}"#);
        let client = test_client(&transport);

        let status = client.poll_qr_login("k").await.unwrap();

        assert_eq!(status.message(), "扫码登录失败（code=999：风控）");
    }

    #[tokio::test]
    async fn status_is_terminal_only_for_final_states() {
        for (code, terminal) in [(800, true), (801, false), (802, false), (803, true)] {
            let transport = FakeTransport::new();
            transport.enqueue_json(&format!(
                r#"{{"code":{code},"cookie":"MUSIC_U=u; __csrf=c"}}"#
            ));
            transport.enqueue_json(ACCOUNT_JSON);
            let client = test_client(&transport);

            let status = client.poll_qr_login("key-1").await.unwrap();

            assert_eq!(status.is_terminal(), terminal, "code {code}");
        }
    }

    #[tokio::test]
    async fn confirmed_merges_set_cookie_and_json_cookie_and_fills_profile() {
        let transport = FakeTransport::new();
        transport.enqueue_json_with_cookies(
            r#"{"code":803,"cookie":"__csrf_token=json-csrf; music_a=from-json"}"#,
            &[
                "MUSIC_U=music-u-value; Path=/; HttpOnly",
                "__csrf=csrf-cookie-value; Path=/",
            ],
        );
        transport.enqueue_json(ACCOUNT_JSON);
        let client = test_client(&transport);

        let status = client.poll_qr_login("qr-key").await.unwrap();

        let QrLoginStatus::Confirmed { session, message } = status else {
            panic!("应为 Confirmed");
        };
        assert_eq!(message, "登录成功");
        assert!(session.cookie.contains("MUSIC_U=music-u-value"));
        assert_eq!(
            session.cookie_value("__csrf").as_deref(),
            Some("csrf-cookie-value")
        );
        assert_eq!(
            session.cookie_value("__csrf_token").as_deref(),
            Some("json-csrf")
        );
        assert_eq!(
            session.cookie_value("music_a").as_deref(),
            Some("from-json")
        );
        assert_eq!(session.user_id, 42);
        assert_eq!(session.nickname.as_deref(), Some("测试用户"));
        assert_eq!(session.vip_type, 1);
        assert_eq!(
            session.avatar_url.as_deref(),
            Some("https://p1.music.126.net/avatar.jpg")
        );
        assert!(session.is_logged_in());
        assert!(client.is_logged_in());

        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].url.ends_with("/weapi/nuser/account/get"));
        assert!(
            requests[1]
                .header("Cookie")
                .unwrap()
                .contains("MUSIC_U=music-u-value")
        );
    }

    #[tokio::test]
    async fn confirmed_builds_cookie_from_json_field_and_aliases_csrf_token() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json(r#"{"code":803,"cookie":"MUSIC_U=json-u; __csrf_token=token-value"}"#);
        transport.enqueue_json(ACCOUNT_JSON);
        let client = test_client(&transport);

        let status = client.poll_qr_login("qr-key").await.unwrap();

        let QrLoginStatus::Confirmed { session, .. } = status else {
            panic!("应为 Confirmed");
        };
        assert_eq!(session.cookie_value("MUSIC_U").as_deref(), Some("json-u"));
        assert_eq!(
            session.cookie_value("__csrf").as_deref(),
            Some("token-value")
        );
        assert_eq!(session.user_id, 42);
    }

    #[tokio::test(start_paused = true)]
    async fn confirmed_keeps_cookie_session_when_profile_fetch_fails() {
        let transport = FakeTransport::new();
        transport.enqueue_json_with_cookies(r#"{"code":803}"#, &["MUSIC_U=only-cookie; Path=/"]);
        transport.enqueue_status(500);
        transport.enqueue_status(500);
        let client = test_client(&transport);

        let status = client.poll_qr_login("qr-key").await.unwrap();

        let QrLoginStatus::Confirmed { session, message } = status else {
            panic!("应为 Confirmed");
        };
        assert_eq!(
            session.cookie_value("MUSIC_U").as_deref(),
            Some("only-cookie")
        );
        assert_eq!(session.user_id, 0);
        assert!(session.is_logged_in()); // 约定：Cookie 非空即登录态
        assert!(message.contains("读取账号信息失败"));
        assert_eq!(transport.requests().len(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn confirmed_without_profile_asks_the_caller_to_wait() {
        let transport = FakeTransport::new();
        transport.enqueue_json_with_cookies(r#"{"code":803}"#, &["MUSIC_U=only-cookie"]);
        transport.enqueue_json(r#"{"code":200,"account":null}"#);
        transport.enqueue_json(r#"{"code":200,"account":null}"#);
        let client = test_client(&transport);

        let status = client.poll_qr_login("qr-key").await.unwrap();

        assert_eq!(status.message(), "扫码登录成功，正在获取账号信息");
    }

    #[tokio::test]
    async fn confirmed_without_any_cookie_fails() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":803}"#);
        let client = test_client(&transport);

        let status = client.poll_qr_login("qr-key").await.unwrap();

        assert_eq!(
            status,
            QrLoginStatus::Failed {
                message: "扫码登录成功，但服务端未返回 Cookie".to_owned()
            }
        );
        assert!(!client.is_logged_in());
    }

    #[tokio::test]
    async fn poll_qr_login_returns_error_on_http_error() {
        let transport = FakeTransport::new();
        transport.enqueue_status(502);
        let client = test_client(&transport);

        let error = client.poll_qr_login("key").await.unwrap_err();

        assert_eq!(error.code(), Some(502));
    }

    #[tokio::test]
    async fn poll_qr_login_rejects_blank_key() {
        let client = test_client(&FakeTransport::new());
        assert!(client.poll_qr_login("  ").await.is_err());
    }

    #[test]
    fn build_cookie_collection_prefers_set_cookie_and_aliases_csrf_token() {
        let cookies = build_cookie_collection(
            &strings(&[
                "MUSIC_U=set-cookie-u; Path=/; HttpOnly",
                "__csrf_token=token-value; Path=/",
            ]),
            Some("MUSIC_U=json-u; extra=1"),
        );

        let get = |name: &str| {
            cookies
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("MUSIC_U"), Some("set-cookie-u"));
        assert_eq!(get("__csrf_token"), Some("token-value"));
        assert_eq!(get("__csrf"), Some("token-value"));
        assert_eq!(get("extra"), Some("1"));
        assert_eq!(cookies.len(), 4);
    }

    #[test]
    fn build_cookie_collection_duplicate_keys_last_wins_headers_still_beat_body() {
        let cookies = build_cookie_collection(
            &strings(&["MUSIC_U=first; Path=/", "MUSIC_U=second; Path=/"]),
            Some("MUSIC_U=body; extra=1; extra=2"),
        );

        let get = |name: &str| {
            cookies
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("MUSIC_U"), Some("second"));
        assert_eq!(get("extra"), Some("2"));
    }

    // ---- 登录续期、Set-Cookie 合并、「登录失效」判定（对应 C# NeteaseApiClientSessionTests） ----

    fn logged_in_client(transport: &Arc<FakeTransport>) -> NeteaseClient {
        let client = test_client(transport);
        client.set_session(Some(NeteaseSession {
            cookie: "MUSIC_U=old; __csrf=csrf1".to_owned(),
            user_id: 42,
            nickname: Some("旧昵称".to_owned()),
            ..Default::default()
        }));
        client
    }

    fn capture_changes(client: &NeteaseClient) -> Arc<std::sync::Mutex<Vec<NeteaseSession>>> {
        let raised = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&raised);
        client.subscribe_session_changed(move |session| sink.lock().unwrap().push(session.clone()));
        raised
    }

    const SIMPLE_ACCOUNT_JSON: &str = r#"{
        "code": 200,
        "account": { "id": 42, "userName": "u" },
        "profile": { "userId": 42, "nickname": "测试用户", "vipType": 1 }
    }"#;

    #[tokio::test]
    async fn weapi_response_set_cookie_is_merged_and_raises_session_changed() {
        let transport = FakeTransport::new();
        transport.enqueue_json_with_cookies(
            SIMPLE_ACCOUNT_JSON,
            &["MUSIC_U=rotated; Max-Age=1296000; Path=/; HttpOnly"],
        );
        let client = logged_in_client(&transport);
        let raised = capture_changes(&client);

        client.get_current_user().await.unwrap();

        let session = client.session().unwrap();
        assert_eq!(session.cookie_value("MUSIC_U").as_deref(), Some("rotated"));
        assert_eq!(session.csrf_token(), "csrf1");
        assert_eq!(session.user_id, 42);
        let raised = raised.lock().unwrap();
        assert_eq!(raised.len(), 1);
        assert_eq!(
            raised[0].cookie_value("MUSIC_U").as_deref(),
            Some("rotated")
        );
    }

    #[tokio::test]
    async fn get_json_response_set_cookie_is_also_merged() {
        let transport = FakeTransport::new();
        transport.enqueue_json_with_cookies(
            r#"{"code":200,"lrc":{"lyric":"[00:01.00]x"}}"#,
            &["MUSIC_U=rotated; Path=/"],
        );
        let client = logged_in_client(&transport);
        let raised = capture_changes(&client);

        client
            .get_json("https://music.163.com/api/song/lyric?id=1", "获取歌词")
            .await
            .unwrap();

        assert_eq!(
            client.session().unwrap().cookie_value("MUSIC_U").as_deref(),
            Some("rotated")
        );
        assert_eq!(raised.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn set_cookie_with_same_values_does_not_raise_session_changed() {
        let transport = FakeTransport::new();
        transport.enqueue_json_with_cookies(SIMPLE_ACCOUNT_JSON, &["MUSIC_U=old; Path=/"]);
        let client = logged_in_client(&transport);
        let raised = capture_changes(&client);

        client.get_current_user().await.unwrap();

        assert!(raised.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn anonymous_response_set_cookie_is_ignored() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json_with_cookies(r#"{"code":200,"account":null}"#, &["MUSIC_A=anon; Path=/"]);
        let client = test_client(&transport);
        let raised = capture_changes(&client);

        client.get_current_user().await.unwrap();

        assert!(client.session().is_none());
        assert!(raised.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn next_request_uses_rotated_cookie() {
        let transport = FakeTransport::new();
        transport.enqueue_json_with_cookies(SIMPLE_ACCOUNT_JSON, &["MUSIC_U=rotated; Path=/"]);
        transport.enqueue_json(SIMPLE_ACCOUNT_JSON);
        let client = logged_in_client(&transport);

        client.get_current_user().await.unwrap();
        client.get_current_user().await.unwrap();

        let requests = transport.requests();
        assert!(
            requests[0]
                .header("Cookie")
                .unwrap()
                .contains("MUSIC_U=old")
        );
        assert!(
            requests[1]
                .header("Cookie")
                .unwrap()
                .contains("MUSIC_U=rotated")
        );
    }

    #[tokio::test]
    async fn set_cookie_that_would_drop_music_u_is_ignored() {
        let transport = FakeTransport::new();
        transport.enqueue_json_with_cookies(
            SIMPLE_ACCOUNT_JSON,
            &["MUSIC_U=; Max-Age=0; Path=/", "os=pc; Path=/"],
        );
        let client = logged_in_client(&transport);
        let raised = capture_changes(&client);

        client.get_current_user().await.unwrap();

        assert_eq!(
            client.session().unwrap().cookie_value("MUSIC_U").as_deref(),
            Some("old")
        );
        assert!(raised.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn set_cookie_from_outdated_request_does_not_leak_into_new_session() {
        let transport = FakeTransport::new();
        let gate = Arc::new(tokio::sync::Notify::new());
        transport.enqueue_gated_with_cookies(
            SIMPLE_ACCOUNT_JSON,
            &["MUSIC_U=stale; Path=/"],
            Arc::clone(&gate),
        );
        let client = Arc::new(logged_in_client(&transport));

        let pending = {
            let client = Arc::clone(&client);
            tokio::spawn(async move { client.get_current_user().await })
        };
        while transport.requests().is_empty() {
            tokio::task::yield_now().await;
        }

        client.set_session(Some(NeteaseSession {
            cookie: "MUSIC_U=other-account".to_owned(),
            ..Default::default()
        }));
        gate.notify_one();
        pending.await.unwrap().unwrap();

        assert_eq!(
            client.session().unwrap().cookie_value("MUSIC_U").as_deref(),
            Some("other-account")
        );
    }

    #[tokio::test]
    async fn unsubscribed_listener_is_not_called() {
        let transport = FakeTransport::new();
        transport.enqueue_json_with_cookies(SIMPLE_ACCOUNT_JSON, &["MUSIC_U=rotated; Path=/"]);
        let client = logged_in_client(&transport);
        let raised = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&raised);
        let id = client.subscribe_session_changed(move |_| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        client.unsubscribe_session_changed(id);

        client.get_current_user().await.unwrap();

        assert_eq!(raised.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn panicking_listener_does_not_break_the_request() {
        let transport = FakeTransport::new();
        transport.enqueue_json_with_cookies(SIMPLE_ACCOUNT_JSON, &["MUSIC_U=rotated; Path=/"]);
        let client = logged_in_client(&transport);
        client.subscribe_session_changed(|_| panic!("订阅方出错"));

        assert!(client.get_current_user().await.unwrap().is_some());
    }

    // ---- 登录续期（eapi `login/token/refresh`）----
    //
    // 响应形状取自 2026-11 对桌面端扫码会话的联网实测（Cookie 值全部换成了假值）：除了真正的新令牌
    // （`MUSIC_U`、`__csrf`、`MUSIC_R_U`），还有二十多条只作用于 `/api/feedback`、`/neapi/clientlog` 等子路径的
    // `MUSIC_A_T` / `MUSIC_R_T`，以及一条会话里根本没有的 `MUSIC_SNS` 删除指令。

    /// 带完整令牌的会话（桌面端扫码得到的 Cookie 名）。
    fn desktop_session_client(transport: &Arc<FakeTransport>) -> NeteaseClient {
        let client = test_client(transport);
        client.set_session(Some(NeteaseSession {
            cookie:
                "MUSIC_A_T=a1; MUSIC_R_T=r1; __csrf=csrf1; NMTID=nmtid1; MUSIC_R_U=ru1; MUSIC_U=old"
                    .to_owned(),
            user_id: 42,
            nickname: Some("旧昵称".to_owned()),
            ..Default::default()
        }));
        client
    }

    const REFRESH_OK_JSON: &str = r#"{"code":200,"bizCode":200}"#;

    /// 续期成功时服务端下发的 Set-Cookie（顺序故意打乱，与实测一样没有规律）。
    fn refresh_set_cookies(music_u: &str, csrf: &str, music_r_u: &str) -> Vec<String> {
        let far = "Max-Age=2147483647; Expires=Mon, 22 Nov 2094 12:46:05 GMT";
        vec![
            format!("MUSIC_A_T=a1; {far}; Path=/weapi/clientlog; Domain=.music.163.com"),
            format!(
                "MUSIC_R_U={music_r_u}; Max-Age=15552000; Expires=Mon, 03 May 2027 09:31:58 GMT; Path=/eapi/login/token/refresh; Domain=.music.163.com"
            ),
            format!("MUSIC_R_T=r1; {far}; Path=/neapi/clientlog; Domain=.music.163.com"),
            format!(
                "MUSIC_U={music_u}; Max-Age=15552000; Expires=Mon, 03 May 2027 09:31:58 GMT; Path=/; Domain=.music.163.com"
            ),
            format!("MUSIC_A_T=a1; {far}; Path=/api/feedback; Domain=.music.163.com"),
            "MUSIC_SNS=; Max-Age=0; Expires=Wed, 04 Nov 2026 09:31:58 GMT; Path=/".to_owned(),
            format!(
                "MUSIC_R_U={music_r_u}; Max-Age=15552000; Expires=Mon, 03 May 2027 09:31:58 GMT; Path=/api/login/token/refresh; Domain=.music.163.com"
            ),
            format!("MUSIC_R_T=r1; {far}; Path=/eapi/feedback; Domain=.music.163.com"),
            format!(
                "__csrf={csrf}; Max-Age=1296010; Expires=Thu, 19 Nov 2026 09:32:08 GMT; Path=/; Domain=.music.163.com"
            ),
        ]
    }

    fn enqueue_refresh_ok(transport: &FakeTransport, set_cookies: &[String]) {
        let refs: Vec<&str> = set_cookies.iter().map(String::as_str).collect();
        transport.enqueue_json_with_cookies(REFRESH_OK_JSON, &refs);
    }

    /// 解开续期请求：返回 `(签名路径, 业务 JSON, 摘要)` 与其中的 `header` 对象。
    fn open_refresh_request(
        request: &crate::netease::http::HttpRequest,
    ) -> (String, Value, serde_json::Map<String, Value>) {
        let params = request.form_value("params").expect("续期请求缺少 params");
        let (path, json, digest) = crate::netease::crypto::eapi_decrypt(params);
        // 摘要必须与路径和业务 JSON 对得上，否则服务端会拒绝。
        let expected = {
            use md5::{Digest, Md5};
            Md5::digest(format!("nobody{path}use{json}md5forencrypt").as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        assert_eq!(digest, expected);
        let body: Value = serde_json::from_str(&json).expect("业务 JSON 合法");
        let header = body["header"]
            .as_object()
            .expect("body 里要有 header")
            .clone();
        (path, body, header)
    }

    #[tokio::test]
    async fn refresh_login_posts_an_eapi_request_with_the_desktop_identity() {
        let transport = FakeTransport::new();
        transport.enqueue_json(REFRESH_OK_JSON);
        let client = desktop_session_client(&transport);

        assert!(client.refresh_login().await);

        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(
            request.url,
            "https://interface.music.163.com/eapi/login/token/refresh"
        );
        assert_eq!(
            request.header("User-Agent"),
            Some(crate::netease::client::DESKTOP_USER_AGENT)
        );
        assert_eq!(
            request.header("Referer"),
            None,
            "官方客户端的 eapi 请求不带 Referer"
        );
        // 表单里只有 params，没有 weapi 的 encSecKey。
        let form = request.form.as_ref().unwrap();
        assert_eq!(form.len(), 1);
        assert_eq!(form[0].0, "params");
        assert_eq!(request.form_value("encSecKey"), None);

        let (path, body, header) = open_refresh_request(request);
        assert_eq!(path, "/api/login/token/refresh");
        assert_eq!(body["e_r"], Value::Bool(false));
        assert_eq!(header["os"], "pc");
        assert!(!header["appver"].as_str().unwrap().is_empty());
        assert!(!header["osver"].as_str().unwrap().is_empty());
        assert!(!header["deviceId"].as_str().unwrap().is_empty());
        assert_eq!(header["channel"], "netease");
        assert_eq!(header["__csrf"], "csrf1");
        assert_eq!(header["MUSIC_U"], "old");
        assert_eq!(header["NMTID"], "nmtid1");
        // buildver 是 Unix 秒（10 位），requestId 是 `<毫秒>_<4 位>`。
        let buildver = header["buildver"].as_str().unwrap();
        assert!(buildver.len() == 10 && buildver.bytes().all(|b| b.is_ascii_digit()));
        let request_id = header["requestId"].as_str().unwrap();
        let (millis, suffix) = request_id.split_once('_').expect("requestId 带下划线");
        assert!(millis.len() == 13 && millis.bytes().all(|b| b.is_ascii_digit()));
        assert!(suffix.len() == 4 && suffix.bytes().all(|b| b.is_ascii_digit()));

        // Cookie 头就是 header 对象里的键值对：服务端靠它们认出会话，并且只带这些
        // （MUSIC_R_U / MUSIC_A_T / MUSIC_R_T 不发，联网实测不需要）。
        let cookie = request.header("Cookie").unwrap();
        for (name, value) in &header {
            assert!(
                cookie.contains(&format!("{name}={}", value.as_str().unwrap())),
                "Cookie 头缺少 {name}"
            );
        }
        for absent in ["MUSIC_R_U", "MUSIC_A_T", "MUSIC_R_T"] {
            assert!(!cookie.contains(absent), "不应带上 {absent}");
        }
    }

    #[tokio::test]
    async fn refresh_login_rotates_music_u_csrf_and_refresh_token_and_keeps_the_rest() {
        let transport = FakeTransport::new();
        enqueue_refresh_ok(&transport, &refresh_set_cookies("new-u", "csrf2", "ru2"));
        let client = desktop_session_client(&transport);
        let raised = capture_changes(&client);

        client.try_refresh_login().await.unwrap();

        let session = client.session().unwrap();
        assert_eq!(session.cookie_value("MUSIC_U").as_deref(), Some("new-u"));
        assert_eq!(session.cookie_value("__csrf").as_deref(), Some("csrf2"));
        assert_eq!(session.cookie_value("MUSIC_R_U").as_deref(), Some("ru2"));
        // 其余令牌不变，没有的 MUSIC_SNS 不会冒出来；账号资料也不受影响。
        assert_eq!(session.cookie_value("MUSIC_A_T").as_deref(), Some("a1"));
        assert_eq!(session.cookie_value("MUSIC_R_T").as_deref(), Some("r1"));
        assert_eq!(session.cookie_value("NMTID").as_deref(), Some("nmtid1"));
        assert_eq!(session.cookie_value("MUSIC_SNS"), None);
        assert_eq!(session.user_id, 42);
        assert_eq!(session.nickname.as_deref(), Some("旧昵称"));
        // 恰好通知一次落盘，内容就是合并后的会话。
        let raised = raised.lock().unwrap();
        assert_eq!(raised.len(), 1);
        assert_eq!(raised[0].cookie, session.cookie);
    }

    #[tokio::test]
    async fn refresh_login_can_be_chained_on_the_rotated_cookie() {
        let transport = FakeTransport::new();
        enqueue_refresh_ok(&transport, &refresh_set_cookies("new-u", "csrf2", "ru2"));
        enqueue_refresh_ok(&transport, &refresh_set_cookies("newer-u", "csrf3", "ru3"));
        let client = desktop_session_client(&transport);

        client.try_refresh_login().await.unwrap();
        client.try_refresh_login().await.unwrap();

        let requests = transport.requests();
        let (_, _, second) = open_refresh_request(&requests[1]);
        assert_eq!(second["MUSIC_U"], "new-u", "第二次要带上第一次换发的令牌");
        assert_eq!(second["__csrf"], "csrf2");
        assert_eq!(
            client.session().unwrap().cookie_value("MUSIC_U").as_deref(),
            Some("newer-u")
        );
    }

    #[tokio::test]
    async fn refresh_response_that_would_drop_music_u_keeps_the_old_login() {
        let transport = FakeTransport::new();
        transport.enqueue_json_with_cookies(
            REFRESH_OK_JSON,
            &["MUSIC_U=; Max-Age=0; Path=/", "__csrf=csrf2; Path=/"],
        );
        let client = desktop_session_client(&transport);
        let raised = capture_changes(&client);

        client.try_refresh_login().await.unwrap();

        let session = client.session().unwrap();
        assert_eq!(session.cookie_value("MUSIC_U").as_deref(), Some("old"));
        assert_eq!(session.cookie_value("__csrf").as_deref(), Some("csrf1"));
        assert!(raised.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn refresh_code_400_is_a_soft_failure_with_the_reason() {
        // 旧的 weapi 端点就是这样回应的：HTTP 200，只有 `{"code":400}`，没有 message。
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":400}"#);
        let client = desktop_session_client(&transport);
        let raised = capture_changes(&client);

        let error = client.try_refresh_login().await.unwrap_err();

        assert_eq!(error.code(), Some(400));
        assert!(!error.is_not_logged_in());
        assert_eq!(error.to_string(), "续期登录状态失败：接口返回 code=400");
        assert!(client.is_logged_in());
        assert!(raised.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn refresh_code_301_is_reported_as_not_logged_in() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":301,"message":"需要登录"}"#);
        let client = desktop_session_client(&transport);

        let error = client.try_refresh_login().await.unwrap_err();

        assert!(error.is_not_logged_in());
        // 续期本身从不清除会话，清除是 LoginManager 复核之后的事。
        assert!(client.is_logged_in());
    }

    #[tokio::test]
    async fn refresh_login_failure_returns_false_without_error() {
        for (status, body) in [
            (200, r#"{"code":301,"message":"需要登录"}"#),
            (200, r#"{"code":400}"#),
            (200, r#"{"code":-460}"#),
            (502, ""),
        ] {
            let transport = FakeTransport::new();
            transport.enqueue_raw(status, body);
            let client = logged_in_client(&transport);

            assert!(!client.refresh_login().await);
            assert_eq!(
                client.session().unwrap().cookie_value("MUSIC_U").as_deref(),
                Some("old")
            );
        }
    }

    #[tokio::test]
    async fn refresh_login_network_failure_returns_false() {
        let transport = FakeTransport::new();
        transport.enqueue_error(crate::netease::http::TransportError::Timeout);
        let client = logged_in_client(&transport);

        assert!(!client.refresh_login().await);
    }

    #[tokio::test]
    async fn refresh_login_without_session_returns_false_and_sends_nothing() {
        let transport = FakeTransport::new();
        let client = test_client(&transport);

        assert!(!client.refresh_login().await);
        assert!(transport.requests().is_empty());
    }

    #[tokio::test]
    async fn get_current_user_account_null_returns_none() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":200,"account":null,"profile":null}"#);
        let client = logged_in_client(&transport);

        assert!(client.get_current_user().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn get_current_user_account_without_profile_still_counts_as_logged_in() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":200,"account":{"id":7,"userName":"only-account"}}"#);
        let client = logged_in_client(&transport);

        let user = client.get_current_user().await.unwrap().unwrap();

        assert_eq!(user.user_id, 7);
        assert_eq!(user.nickname, "only-account");
        assert_eq!(client.session().unwrap().user_id, 7);
    }

    #[tokio::test]
    async fn get_current_user_http_status_errors_are_not_login_expiry() {
        for status in [302u16, 401, 500] {
            let transport = FakeTransport::new();
            transport.enqueue_status(status);
            let client = logged_in_client(&transport);

            let error = client.get_current_user().await.unwrap_err();

            assert_eq!(error.code(), Some(i32::from(status)));
            assert!(error.is_http_status());
            assert!(!error.is_not_logged_in());
        }
    }

    #[tokio::test]
    async fn get_current_user_other_business_codes_are_not_login_expiry() {
        for code in [-460, 406, -2] {
            let transport = FakeTransport::new();
            transport.enqueue_json(&format!(r#"{{"code":{code}}}"#));
            let client = logged_in_client(&transport);

            let error = client.get_current_user().await.unwrap_err();

            assert_eq!(error.code(), Some(code));
            assert!(!error.is_http_status());
            assert!(!error.is_not_logged_in());
            assert_eq!(error.kind(), NeteaseErrorKind::Api);
        }
    }

    #[tokio::test]
    async fn get_current_user_code_301_is_explicit_not_logged_in() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":301,"message":"需要登录"}"#);
        let client = logged_in_client(&transport);

        let error = client.get_current_user().await.unwrap_err();

        assert_eq!(error.code(), Some(301));
        assert!(!error.is_http_status());
        assert!(error.is_not_logged_in());
    }

    #[test]
    fn playback_cookie_header_does_not_duplicate_keys_already_in_session() {
        let session = NeteaseSession {
            cookie: "MUSIC_U=u; os=android; appver=9.9.9".to_owned(),
            ..Default::default()
        };

        let header = NeteaseClient::build_playback_cookie_header(Some(&session));

        let mut names: Vec<String> = header
            .split(';')
            .map(|part| part.trim().split('=').next().unwrap().to_ascii_lowercase())
            .collect();
        let total = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), total);
        assert!(header.contains("os=android"));
        assert!(header.contains("appver=9.9.9"));
        assert!(!header.contains("os=pc"));
        assert!(header.contains("deviceId=LunaticPrincessPc"));
        assert!(header.contains("channel=netease"));
    }

    #[test]
    fn playback_cookie_header_anonymous_uses_client_flags_once() {
        let header = NeteaseClient::build_playback_cookie_header(None);

        let mut names: Vec<String> = header
            .split(';')
            .map(|part| part.trim().split('=').next().unwrap().to_ascii_lowercase())
            .collect();
        let total = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), total);
        assert!(header.contains("appver=2.10.6"));
    }

    #[tokio::test]
    async fn playback_requests_carry_the_desktop_client_cookie() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":200}"#);
        let client = logged_in_client(&transport);

        client
            .post_weapi("/api/song/enhance/player/url/v1", &json!({}), "取链", true)
            .await
            .unwrap();

        let cookie = transport.requests()[0].header("Cookie").unwrap().to_owned();
        assert!(cookie.starts_with("MUSIC_U=old; __csrf=csrf1; "));
        assert!(cookie.contains("deviceId=LunaticPrincessPc"));
    }
}
