//! 歌曲搜索。对应 C# `NeteaseApiClient.Search.cs`，但主端点已经换掉。
//!
//! **主路径：匿名 eapi `POST /eapi/cloudsearch/pc`**（桌面客户端用的搜索接口）。2026-11 的联网探针确认：它不需要登录，
//! 海外 IP 下也返回明文，结果带齐 id / 歌名 / 歌手 / 专辑 / 时长 / 封面（`ar` / `al` / `dt` 结构，与歌曲详情接口一致）。
//!
//! C# 用的网页接口 `GET /api/search/get/web` 在海外 IP 下返回十六进制密文（`{"result":"<hex>","abroad":true}`），
//! 密文用 eapi / weapi / linuxapi 的固定 key 都解不开；`/weapi/cloudsearch/get/web` 匿名只返回 `code=50000005`。
//! 所以网页接口只作**回退**：eapi 被明确拒绝或返回了无法识别的内容时再试一次（国内 IP 下它仍是明文）。
//!
//! 连续多次请求仍可能 `code=406` 限流，调用方要克制地发请求；限流 / 风控（`406` / `-460` / HTTP 429）不回退，
//! 回退只会多发一次同样会被拒绝的请求。

use serde_json::{Value, json};

use super::client::{INTERFACE_BASE_URL, MUSIC_BASE_URL, NeteaseClient, escape_data_string};
use super::error::{NeteaseError, NeteaseErrorKind};
use super::json;
use super::mappers;
use super::models::NeteaseSong;
use crate::library::text::is_blank;

/// 搜索请求数量的合法范围（与 C# 的 `Math.Clamp(limit, 1, 50)` 一致）。
const MIN_LIMIT: usize = 1;
const MAX_LIMIT: usize = 50;

/// eapi 搜索接口的网页风格路径（摘要与密文基于它；实际请求的是 `/eapi/cloudsearch/pc`）。
const EAPI_SEARCH_PATH: &str = "/api/cloudsearch/pc";
/// eapi 主机：先 `interface`，连接 / TLS 失败（实测偶发 `unexpected EOF during handshake`）再换主站，两者都实测可用。
const EAPI_HOSTS: [&str; 2] = [INTERFACE_BASE_URL, MUSIC_BASE_URL];
const OPERATION: &str = "搜索网易云歌曲";

impl NeteaseClient {
    /// 按关键词搜索歌曲（匿名，不读取登录态）。关键词为空白时直接返回空列表（不发请求）；
    /// 缺少 `result.songs` 时返回空列表；id 非法或歌名为空白的项被丢弃。
    pub async fn search_songs(
        &self,
        keyword: &str,
        limit: usize,
    ) -> Result<Vec<NeteaseSong>, NeteaseError> {
        if is_blank(keyword) {
            return Ok(Vec::new());
        }
        let keyword = keyword.trim();
        let limit = limit.clamp(MIN_LIMIT, MAX_LIMIT);

        let eapi_error = match self.search_via_eapi(keyword, limit).await {
            Ok(songs) => return Ok(songs),
            Err(error) => error,
        };
        if !should_fall_back(&eapi_error) {
            return Err(eapi_error);
        }
        // 网页接口也失败时报 eapi 的错误：它才是主路径，信息更有用。
        self.search_via_web(keyword, limit)
            .await
            .map_err(|_| eapi_error)
    }

    async fn search_via_eapi(
        &self,
        keyword: &str,
        limit: usize,
    ) -> Result<Vec<NeteaseSong>, NeteaseError> {
        let payload = json!({
            "s": keyword,
            "type": 1,
            "limit": limit,
            "offset": 0,
            "total": true,
        });

        let mut network_error = None;
        for host in EAPI_HOSTS {
            match self
                .post_eapi(host, EAPI_SEARCH_PATH, &payload, OPERATION)
                .await
            {
                Ok(document) => return parse_songs(&document),
                // 只有连接 / TLS 失败才换主机；超时（已经等了很久）与接口明确拒绝都不重试。
                Err(error) if error.kind() == NeteaseErrorKind::Network => {
                    network_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(network_error.unwrap_or_else(|| {
            NeteaseError::protocol(format!("{OPERATION}失败：没有可用的接口主机"))
        }))
    }

    async fn search_via_web(
        &self,
        keyword: &str,
        limit: usize,
    ) -> Result<Vec<NeteaseSong>, NeteaseError> {
        let url = format!(
            "{MUSIC_BASE_URL}/api/search/get/web?s={}&type=1&limit={limit}&offset=0",
            escape_data_string(keyword)
        );
        let document = self.get_json(&url, OPERATION).await?;
        parse_songs(&document)
    }
}

/// eapi 失败后要不要再试网页接口：网络层失败（连主机都连不上）与限流 / 风控不值得再发一次。
fn should_fall_back(error: &NeteaseError) -> bool {
    match error.kind() {
        NeteaseErrorKind::Timeout | NeteaseErrorKind::Network => false,
        NeteaseErrorKind::HttpStatus | NeteaseErrorKind::Api | NeteaseErrorKind::Protocol => {
            !matches!(error.code(), Some(406 | -460 | 429))
        }
    }
}

/// 解析 `result.songs`。`result` 是字符串（海外 IP 下网页接口返回的密文）时报明确的错误，
/// 而不是悄悄当成“没有结果”。
fn parse_songs(document: &Value) -> Result<Vec<NeteaseSong>, NeteaseError> {
    if matches!(json::property(document, "result"), Some(Value::String(_))) {
        return Err(NeteaseError::protocol(format!(
            "{OPERATION}失败：接口返回了无法识别的加密数据"
        )));
    }

    let songs = json::property(document, "result").and_then(|result| {
        match json::property(result, "songs") {
            Some(Value::Array(items)) => Some(items),
            _ => None,
        }
    });
    Ok(songs
        .map(|items| {
            items
                .iter()
                .map(|item| mappers::map_song(item, None))
                .filter(|song| song.id > 0 && !is_blank(&song.name))
                .collect()
        })
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use md5::{Digest, Md5};

    use super::*;
    use crate::netease::crypto;
    use crate::netease::http::{HttpMethod, HttpRequest, TransportError};
    use crate::session::NeteaseSession;
    use crate::test_util::{FakeTransport, test_client};

    /// 真实 eapi 搜索响应的精简版（字段结构取自 2026-11 的联网探针，内容是合成的）。
    const SEARCH_JSON: &str = r#"{
      "code": 200,
      "result": { "songCount": 300, "songs": [
        { "id": 536622304, "name": "Lemon", "dt": 256000, "no": 1, "cd": "1", "fee": 1,
          "ar": [ { "id": 159300, "name": "米津玄師", "tns": ["米津玄师"], "alia": ["ハチ"] },
                  { "id": 2, "name": "合作者" } ],
          "al": { "id": 37575103, "name": "Lemon",
                  "picUrl": "http://p1.music.126.net/jtPZRUFrSS-nRCjW_LYowQ==/109951166521931227.jpg" },
          "alia": ["日剧主题曲"], "publishTime": 1520956800000,
          "privilege": { "id": 536622304, "fee": 1, "maxbr": 999000 } },
        { "id": 0, "name": "无效 ID" },
        { "id": 7, "name": "  " }
      ] },
      "trp": { "rules": [] }
    }"#;

    /// 网页接口（回退用）的响应结构：`artists` / `album`。
    const WEB_SEARCH_JSON: &str = r#"{
      "code": 200,
      "result": { "songCount": 3, "songs": [
        { "id": 186016, "name": "晴天", "duration": 269000,
          "artists": [ { "id": 6452, "name": "周杰伦" } ],
          "album": { "id": 18877, "name": "叶惠美", "publishTime": 1058284800000 },
          "alias": [], "fee": 8 },
        { "id": 0, "name": "无效 ID" }
      ] }
    }"#;

    const ENCRYPTED_JSON: &str =
        r#"{"result":"35b1748964af8a7c1d883e3a6f3c","code":200,"abroad":true}"#;

    /// 解开 eapi 请求的 `params`，返回 `(路径, 业务 JSON, 摘要)`。
    fn decrypt_eapi(request: &HttpRequest) -> (String, Value, String) {
        let params = request.form_value("params").expect("缺少 params 表单字段");
        let (path, json_text, digest) = crypto::eapi_decrypt(params);
        let value = serde_json::from_str(&json_text).expect("业务 JSON 合法");
        // 摘要必须是 MD5("nobody<路径>use<业务 JSON>md5forencrypt")。
        let expected = Md5::digest(format!("nobody{path}use{json_text}md5forencrypt").as_bytes());
        let expected: String = expected.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(digest, expected, "摘要与明文不一致");
        (path, value, digest)
    }

    fn urls(transport: &FakeTransport) -> Vec<String> {
        transport.requests().into_iter().map(|r| r.url).collect()
    }

    #[tokio::test]
    async fn eapi_search_maps_results_and_drops_invalid_entries() {
        let transport = FakeTransport::new();
        transport.enqueue_json(SEARCH_JSON);
        let client = test_client(&transport);

        let songs = client.search_songs("  Lemon 米津玄師 ", 10).await.unwrap();

        assert_eq!(songs.len(), 1);
        let song = &songs[0];
        assert_eq!(song.id, 536_622_304);
        assert_eq!(song.name, "Lemon");
        assert_eq!(song.artist_text(), "米津玄師/合作者");
        assert_eq!(song.album.name, "Lemon");
        assert_eq!(
            song.cover_url(),
            Some("http://p1.music.126.net/jtPZRUFrSS-nRCjW_LYowQ==/109951166521931227.jpg")
        );
        assert_eq!(song.duration_ms, 256_000);
        assert_eq!(song.track_number, 1);
        assert_eq!(song.disc_number, 1);
        assert_eq!(song.release_date.as_deref(), Some("2018-03-14"));
        assert_eq!(song.alias.as_deref(), Some("日剧主题曲"));
        assert_eq!(song.max_bitrate, 999_000);
    }

    #[tokio::test]
    async fn eapi_request_is_an_encrypted_anonymous_desktop_client_post() {
        let transport = FakeTransport::new();
        transport.enqueue_json(SEARCH_JSON);
        let client = test_client(&transport);

        client.search_songs("  晴天 周杰伦 ", 7).await.unwrap();

        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(
            request.url,
            "https://interface.music.163.com/eapi/cloudsearch/pc"
        );
        assert_eq!(
            request.form.as_ref().map(Vec::len),
            Some(1),
            "只有 params 一个表单字段"
        );
        assert!(request.body.is_none());
        assert_eq!(request.header("Referer"), Some("https://music.163.com/"));
        assert!(request.header("User-Agent").unwrap().contains("Chrome/"));

        // 摘要与明文里的路径是网页风格的 /api/，URL 才是 /eapi/。
        let (path, payload, _) = decrypt_eapi(request);
        assert_eq!(path, "/api/cloudsearch/pc");
        assert_eq!(payload["s"], "晴天 周杰伦");
        assert_eq!(payload["type"], 1);
        assert_eq!(payload["limit"], 7);
        assert_eq!(payload["offset"], 0);
        assert_eq!(payload["total"], true);

        // 请求体里的 header 对象是桌面端身份，同样的字段也作为 Cookie 发出。
        let header = &payload["header"];
        assert_eq!(header["os"], "pc");
        assert_eq!(header["appver"], "2.10.6");
        assert_eq!(header["deviceId"], "LunaticPrincessPc");
        assert_eq!(header["channel"], "netease");
        assert_eq!(header["versioncode"], "140");
        assert_eq!(header["resolution"], "1920x1080");
        assert_eq!(header["__csrf"], "");
        let request_id = header["requestId"].as_str().unwrap();
        let (millis, random) = request_id
            .split_once('_')
            .expect("requestId 形如 <毫秒>_<随机>");
        assert!(
            millis.len() >= 13 && millis.bytes().all(|b| b.is_ascii_digit()),
            "{request_id}"
        );
        assert!(
            random.len() == 4 && random.bytes().all(|b| b.is_ascii_digit()),
            "{request_id}"
        );
        assert!(
            header["buildver"]
                .as_str()
                .unwrap()
                .bytes()
                .all(|b| b.is_ascii_digit())
        );

        let cookie = request.header("Cookie").unwrap();
        for expected in [
            "os=pc",
            "appver=2.10.6",
            "deviceId=LunaticPrincessPc",
            "channel=netease",
            "__csrf=",
        ] {
            assert!(
                cookie.contains(expected),
                "Cookie 缺少 {expected}：{cookie}"
            );
        }
        assert!(
            cookie.contains(&format!("requestId={request_id}")),
            "{cookie}"
        );
    }

    #[tokio::test]
    async fn search_is_anonymous_even_when_logged_in_and_never_touches_the_session() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json_with_cookies(SEARCH_JSON, &["NMTID=anon-device; Max-Age=3600; Path=/"]);
        let client = test_client(&transport);
        let session = NeteaseSession {
            cookie: "MUSIC_U=secret-token; __csrf=csrf-token".to_owned(),
            ..Default::default()
        };
        client.set_session(Some(session.clone()));
        let changes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = std::sync::Arc::clone(&changes);
        client.subscribe_session_changed(move |_| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });

        client.search_songs("Lemon", 5).await.unwrap();

        let request = &transport.requests()[0];
        let cookie = request.header("Cookie").unwrap();
        assert!(
            !cookie.contains("MUSIC_U") && !cookie.contains("secret-token"),
            "{cookie}"
        );
        assert!(!cookie.contains("csrf-token"), "{cookie}");
        let (_, payload, _) = decrypt_eapi(request);
        assert_eq!(payload["header"]["__csrf"], "");
        assert!(payload.get("csrf_token").is_none());
        // 匿名响应里的 Set-Cookie（NMTID）不能被合并进登录态。
        assert_eq!(client.session().unwrap().cookie, session.cookie);
        assert_eq!(changes.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn blank_keyword_sends_nothing() {
        let transport = FakeTransport::new();
        let client = test_client(&transport);

        assert!(client.search_songs("  ", 10).await.unwrap().is_empty());
        assert!(transport.requests().is_empty());
    }

    #[tokio::test]
    async fn limit_is_clamped_to_the_supported_range() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json(r#"{"code":200}"#)
            .enqueue_json(r#"{"code":200}"#);
        let client = test_client(&transport);

        client.search_songs("a", 0).await.unwrap();
        client.search_songs("a", 500).await.unwrap();

        let requests = transport.requests();
        assert_eq!(decrypt_eapi(&requests[0]).1["limit"], 1);
        assert_eq!(decrypt_eapi(&requests[1]).1["limit"], 50);
    }

    #[tokio::test]
    async fn missing_result_section_means_no_songs() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json(r#"{"code":200}"#)
            .enqueue_json(r#"{"code":200,"result":{"songCount":0}}"#)
            .enqueue_json(r#"{"code":200,"result":{"songs":"oops"}}"#);
        let client = test_client(&transport);

        assert!(client.search_songs("a", 5).await.unwrap().is_empty());
        assert!(client.search_songs("a", 5).await.unwrap().is_empty());
        assert!(client.search_songs("a", 5).await.unwrap().is_empty());
        // 都是正常的“没有结果”，不触发回退。
        assert_eq!(transport.requests().len(), 3);
    }

    #[tokio::test]
    async fn connection_failure_retries_once_on_the_main_site_host() {
        let transport = FakeTransport::new();
        transport
            .enqueue_error(TransportError::Network(
                "unexpected EOF during handshake".to_owned(),
            ))
            .enqueue_json(SEARCH_JSON);
        let client = test_client(&transport);

        let songs = client.search_songs("Lemon", 5).await.unwrap();

        assert_eq!(songs.len(), 1);
        assert_eq!(
            urls(&transport),
            [
                "https://interface.music.163.com/eapi/cloudsearch/pc",
                "https://music.163.com/eapi/cloudsearch/pc"
            ]
        );
        // 两次请求发的是同一个搜索，但每次都带新的 requestId。
        let requests = transport.requests();
        let first = decrypt_eapi(&requests[0]).1;
        let second = decrypt_eapi(&requests[1]).1;
        assert_eq!(first["s"], second["s"]);
    }

    #[tokio::test]
    async fn connection_failure_on_both_hosts_is_reported_without_the_web_fallback() {
        let transport = FakeTransport::new();
        transport
            .enqueue_error(TransportError::Network("connection reset".to_owned()))
            .enqueue_error(TransportError::Network("dns error".to_owned()));
        let client = test_client(&transport);

        let error = client.search_songs("Lemon", 5).await.unwrap_err();

        assert_eq!(error.kind(), NeteaseErrorKind::Network);
        assert!(error.message().contains("dns error"), "{}", error.message());
        assert_eq!(transport.requests().len(), 2);
    }

    #[tokio::test]
    async fn timeout_is_neither_retried_nor_falls_back() {
        let transport = FakeTransport::new();
        transport.enqueue_error(TransportError::Timeout);
        let client = test_client(&transport);

        let error = client.search_songs("Lemon", 5).await.unwrap_err();

        assert_eq!(error.kind(), NeteaseErrorKind::Timeout);
        assert_eq!(transport.requests().len(), 1);
    }

    #[tokio::test]
    async fn rate_limit_and_risk_control_codes_are_reported_without_falling_back() {
        for (body, code) in [
            (r#"{"code":406,"message":"too fast"}"#, 406),
            (r#"{"code":-460,"message":"Cheating"}"#, -460),
        ] {
            let transport = FakeTransport::new();
            transport.enqueue_json(body);
            let client = test_client(&transport);

            let error = client.search_songs("a", 5).await.unwrap_err();

            assert_eq!(error.code(), Some(code));
            assert!(error.message().contains("搜索网易云歌曲"));
            assert_eq!(
                transport.requests().len(),
                1,
                "code={code} 不应再发一次请求"
            );
        }

        let transport = FakeTransport::new();
        transport.enqueue_status(429);
        let client = test_client(&transport);
        let error = client.search_songs("a", 5).await.unwrap_err();
        assert!(error.is_http_status());
        assert_eq!(transport.requests().len(), 1);
    }

    #[tokio::test]
    async fn rejected_eapi_falls_back_to_the_web_endpoint() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json(r#"{"code":404,"message":"接口未找到！"}"#)
            .enqueue_json(WEB_SEARCH_JSON);
        let client = test_client(&transport);

        let songs = client.search_songs("  晴天 周杰伦 ", 10).await.unwrap();

        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].id, 186_016);
        assert_eq!(songs[0].artist_text(), "周杰伦");
        assert_eq!(songs[0].album.name, "叶惠美");
        assert_eq!(songs[0].duration_ms, 269_000);
        assert_eq!(songs[0].release_date.as_deref(), Some("2003-07-16"));

        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].method, HttpMethod::Get);
        assert_eq!(
            requests[1].url,
            "https://music.163.com/api/search/get/web?s=%E6%99%B4%E5%A4%A9%20%E5%91%A8%E6%9D%B0%E4%BC%A6&type=1&limit=10&offset=0"
        );
    }

    #[tokio::test]
    async fn eapi_server_errors_and_unrecognisable_bodies_also_fall_back() {
        for first in [
            ("status", ""),
            ("html", "<html>blocked</html>"),
            ("encrypted", ENCRYPTED_JSON),
        ] {
            let transport = FakeTransport::new();
            match first.0 {
                "status" => transport.enqueue_status(502),
                "html" => transport.enqueue_raw(200, first.1),
                _ => transport.enqueue_json(first.1),
            };
            transport.enqueue_json(WEB_SEARCH_JSON);
            let client = test_client(&transport);

            let songs = client.search_songs("晴天", 10).await.unwrap();

            assert_eq!(songs.len(), 1, "{}", first.0);
            assert_eq!(transport.requests().len(), 2, "{}", first.0);
        }
    }

    #[tokio::test]
    async fn when_the_web_fallback_fails_too_the_eapi_error_is_reported() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json(r#"{"code":404,"message":"接口未找到！"}"#)
            // 海外 IP 下网页接口返回密文。
            .enqueue_json(ENCRYPTED_JSON);
        let client = test_client(&transport);

        let error = client.search_songs("Lemon", 5).await.unwrap_err();

        assert_eq!(error.code(), Some(404));
        assert!(
            error.message().contains("搜索网易云歌曲失败"),
            "{}",
            error.message()
        );
        assert_eq!(transport.requests().len(), 2);
    }

    #[tokio::test]
    async fn an_encrypted_result_payload_is_reported_instead_of_looking_like_no_results() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json(ENCRYPTED_JSON)
            .enqueue_json(ENCRYPTED_JSON);
        let client = test_client(&transport);

        let error = client.search_songs("Lemon", 5).await.unwrap_err();

        assert!(error.message().contains("加密数据"), "{}", error.message());
        assert_eq!(error.kind(), NeteaseErrorKind::Protocol);
    }

    #[test]
    fn parse_songs_accepts_both_result_shapes() {
        let eapi: Value = serde_json::from_str(SEARCH_JSON).unwrap();
        let web: Value = serde_json::from_str(WEB_SEARCH_JSON).unwrap();
        assert_eq!(parse_songs(&eapi).unwrap().len(), 1);
        assert_eq!(parse_songs(&web).unwrap().len(), 1);
    }
}
