//! 播放地址（取链）接口。对应 C# `NeteaseApiClient.Songs.cs` 的 `GetSongPlaybackInfoAsync`
//! 与 `NeteaseMappers.MapPlaybackInfo`。
//!
//! 请求一个音质档位时，服务端可能因为账号没有对应会员权益而**静默降档**（返回更低的 `level`），
//! 也可能直接没有地址；本模块按 C# 的顺序逐级降档重试，并把响应里的 `level` 原样交给调用方，
//! 由下载服务提示「实际音质」。试听片段（`freeTrialInfo`）只作标记，是否拒绝保存由调用方决定。

use serde::Serialize;
use serde_json::{Value, json};

use super::client::NeteaseClient;
use super::error::{NeteaseError, NeteaseErrorKind};
use super::json;
use crate::models::AudioQuality;

/// 取链结果。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongPlaybackInfo {
    /// 歌曲 ID。
    pub song_id: i64,
    /// 播放 / 下载地址；没有时为 `None`。
    pub url: Option<String>,
    /// 码率（bps）。
    pub bitrate: i32,
    /// 文件大小（字节）。
    pub size: i64,
    /// 容器格式（`mp3` / `flac` …）。
    pub format: Option<String>,
    /// 服务端实际给出的音质（可能比请求的低）。
    pub level: Option<String>,
    /// 付费类型。
    pub fee: i32,
    /// 是否只是试听片段。
    pub is_trial: bool,
    /// 试听起点（秒）。
    pub trial_start_seconds: Option<i32>,
    /// 试听终点（秒）。
    pub trial_end_seconds: Option<i32>,
    /// 地址有效期（秒）。
    pub expires_in_seconds: i32,
    /// 没有地址时的中文原因。
    pub error_message: Option<String>,
}

impl SongPlaybackInfo {
    /// 是否拿到了地址。
    pub fn is_success(&self) -> bool {
        self.url
            .as_deref()
            .is_some_and(|url| !url.trim().is_empty())
    }
}

/// 映射取链接口的响应（只看 `data` 数组的第一项）。
pub fn map_playback_info(root: &Value, song_id: i64) -> SongPlaybackInfo {
    let failure = |message: &str| SongPlaybackInfo {
        song_id,
        error_message: Some(message.to_owned()),
        ..SongPlaybackInfo::default()
    };
    let Some(Value::Array(data)) = json::property(root, "data") else {
        return failure("接口未返回播放数据（data 缺失）");
    };
    match data.first() {
        Some(item) => map_playback_item(item, song_id),
        None => failure("接口未返回播放数据（data 为空）"),
    }
}

fn map_playback_item(item: &Value, song_id: i64) -> SongPlaybackInfo {
    if !item.is_object() {
        return SongPlaybackInfo {
            song_id,
            error_message: Some("接口未返回播放数据（data 项非法）".to_owned()),
            ..SongPlaybackInfo::default()
        };
    }

    let url = json::string_or_none(item, "url").filter(|url| !url.trim().is_empty());
    let code = json::i32_or(item, "code", if url.is_none() { 0 } else { 200 });
    let fee = json::i32_or(item, "fee", 0);
    let trial = json::property(item, "freeTrialInfo").filter(|trial| trial.is_object());
    let trial_bound = |name: &str| {
        trial.and_then(|trial| json::property(trial, name).map(|_| json::i32_or(trial, name, 0)))
    };
    let trial_start = trial_bound("start");
    let trial_end = trial_bound("end");

    SongPlaybackInfo {
        song_id: json::i64_or(item, "id", song_id),
        error_message: url.is_none().then(|| describe_playback_failure(code, fee)),
        url,
        bitrate: json::i32_or(item, "br", 0),
        size: json::i64_or(item, "size", 0),
        format: json::string_or_none(item, "type"),
        level: json::string_or_none(item, "level"),
        fee,
        is_trial: trial.is_some(),
        trial_start_seconds: trial_start,
        trial_end_seconds: trial_end,
        expires_in_seconds: json::i32_or(item, "expi", 0),
    }
}

fn describe_playback_failure(code: i32, fee: i32) -> String {
    if fee == 1 && code == 404 {
        return "歌曲需要 VIP 会员或版权受限，没有可用的播放地址（code=404）".to_owned();
    }
    match code {
        0 => "接口未返回播放地址".to_owned(),
        404 => "歌曲版权受限或已下架，没有可用的播放地址（code=404）".to_owned(),
        403 => "无权访问该歌曲（code=403）".to_owned(),
        other => format!("获取播放地址失败（code={other}）"),
    }
}

impl NeteaseClient {
    /// 取一首歌的播放 / 下载地址。
    ///
    /// 从请求的音质开始逐级降档（杜比全景声 → … → 无损 → 极高 → 标准）：每个档位先试新接口
    /// `/weapi/song/enhance/player/url/v1`（同一档位有多种 `level` 写法时依次尝试），再试经典接口；
    /// 新接口一旦抛错（HTTP 失败、业务 code 非 200），本首歌后面的档位不再试它。
    /// 所有档位都拿不到地址时返回最后一个失败原因。
    pub async fn get_song_playback_info(
        &self,
        song_id: i64,
        quality: AudioQuality,
    ) -> Result<SongPlaybackInfo, NeteaseError> {
        if song_id <= 0 {
            return Err(NeteaseError::protocol("歌曲 ID 必须大于 0。"));
        }

        let mut v1_available = true;
        let mut last_error: Option<NeteaseError> = None;
        let mut last_message: Option<String> = None;

        for attempt in quality.fallback_chain() {
            if v1_available {
                match self.request_playback_url_v1(song_id, attempt).await {
                    Ok(info) if info.is_success() => return Ok(info),
                    Ok(info) => {
                        last_message = info.error_message.or(last_message);
                    }
                    Err(error) => {
                        // v1 端点在某些网络 / 匿名场景不可用：本首歌曲剩余音质不再尝试 v1，直接走经典接口。
                        v1_available = false;
                        last_error = Some(error);
                    }
                }
            }

            match self.request_playback_url_legacy(song_id, attempt).await {
                Ok(info) if info.is_success() => return Ok(info),
                Ok(info) => {
                    last_message = info.error_message.or(last_message);
                }
                Err(error) => last_error = Some(error),
            }
        }

        let message = last_message
            .or_else(|| last_error.as_ref().map(|error| error.message().to_owned()))
            .unwrap_or_else(|| format!("歌曲 {song_id} 没有可用的播放地址"));
        Err(NeteaseError::new(
            message,
            last_error.as_ref().and_then(NeteaseError::code),
            last_error
                .as_ref()
                .map_or(NeteaseErrorKind::Api, NeteaseError::kind),
        ))
    }

    /// 新接口：同一档位有多种 `level` 写法时依次尝试。
    async fn request_playback_url_v1(
        &self,
        song_id: i64,
        quality: AudioQuality,
    ) -> Result<SongPlaybackInfo, NeteaseError> {
        let mut last_info: Option<SongPlaybackInfo> = None;
        for level in quality.api_levels() {
            let payload = json!({
                "ids": format!("[{song_id}]"),
                "level": level,
                "encodeType": "flac",
            });
            let document = self
                .post_weapi(
                    "/weapi/song/enhance/player/url/v1",
                    &payload,
                    &format!("获取播放地址（{}）", quality.display_name()),
                    true,
                )
                .await?;
            let info = map_playback_info(&document, song_id);
            if info.is_success() {
                return Ok(info);
            }
            last_info = Some(info);
        }
        Ok(last_info.unwrap_or_else(|| SongPlaybackInfo {
            song_id,
            error_message: Some("接口未返回播放地址".to_owned()),
            ..SongPlaybackInfo::default()
        }))
    }

    /// 经典接口 `/weapi/song/enhance/player/url`。
    async fn request_playback_url_legacy(
        &self,
        song_id: i64,
        quality: AudioQuality,
    ) -> Result<SongPlaybackInfo, NeteaseError> {
        let payload = json!({
            "ids": format!("[{song_id}]"),
            "br": quality.bitrate(),
        });
        let document = self
            .post_weapi(
                "/weapi/song/enhance/player/url",
                &payload,
                &format!("获取播放地址（{}）", quality.display_name()),
                true,
            )
            .await?;
        Ok(map_playback_info(&document, song_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::NeteaseSession;
    use crate::test_util::{FIXED_SECRET, FakeTransport, decrypt_business_json, test_client};

    fn payload_of(request: &crate::netease::http::HttpRequest) -> Value {
        serde_json::from_str(&decrypt_business_json(request, FIXED_SECRET)).unwrap()
    }

    #[tokio::test]
    async fn maps_url_metadata_and_trial_info() {
        let transport = FakeTransport::new();
        transport.enqueue_json(
            r#"{
              "code": 200,
              "data": [{
                "id": 33894312, "url": "https://m701.music.126.net/song.mp3",
                "br": 320000, "size": 12345678, "type": "mp3", "level": "exhigh", "fee": 1,
                "freeTrialInfo": { "start": 0, "end": 45 }, "expi": 1200, "code": 200
              }]
            }"#,
        );
        let client = test_client(&transport);

        let info = client
            .get_song_playback_info(33_894_312, AudioQuality::ExHigh)
            .await
            .unwrap();

        assert!(info.is_success());
        assert_eq!(info.song_id, 33_894_312);
        assert_eq!(
            info.url.as_deref(),
            Some("https://m701.music.126.net/song.mp3")
        );
        assert_eq!(info.bitrate, 320_000);
        assert_eq!(info.size, 12_345_678);
        assert_eq!(info.format.as_deref(), Some("mp3"));
        assert_eq!(info.level.as_deref(), Some("exhigh"));
        assert_eq!(info.fee, 1);
        assert!(info.is_trial);
        assert_eq!(info.trial_start_seconds, Some(0));
        assert_eq!(info.trial_end_seconds, Some(45));
        assert_eq!(info.expires_in_seconds, 1200);
        assert_eq!(info.error_message, None);

        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0]
                .url
                .ends_with("/weapi/song/enhance/player/url/v1")
        );
        let payload = payload_of(&requests[0]);
        assert_eq!(payload["ids"], "[33894312]");
        assert_eq!(payload["level"], "exhigh");
        assert_eq!(payload["encodeType"], "flac");

        // 播放接口必须带桌面客户端 Cookie，否则服务端会把高音质档位静默降成 lossless。
        let cookie = requests[0].header("Cookie").expect("请求带 Cookie");
        assert!(cookie.contains("os=pc"));
        assert!(cookie.contains("appver=2.10.6"));
        assert!(cookie.contains("deviceId=LunaticPrincessPc"));
    }

    #[tokio::test]
    async fn keeps_the_session_cookie_and_adds_client_flags() {
        let transport = FakeTransport::new();
        transport.enqueue_json(
            r#"{"code":200,"data":[{"id":33894312,"url":"https://m701.music.126.net/song.flac","br":1785265,"type":"flac","level":"hires","code":200}]}"#,
        );
        let client = test_client(&transport);
        client.set_session(Some(NeteaseSession {
            cookie: "MUSIC_U=session123; __csrf=csrfabc".to_owned(),
            ..Default::default()
        }));

        let info = client
            .get_song_playback_info(33_894_312, AudioQuality::HiRes)
            .await
            .unwrap();

        assert!(info.is_success());
        let requests = transport.requests();
        let cookie = requests[0].header("Cookie").unwrap();
        assert!(cookie.contains("MUSIC_U=session123"));
        assert!(cookie.contains("__csrf=csrfabc"));
        assert!(cookie.contains("os=pc"));
        assert!(cookie.contains("deviceId=LunaticPrincessPc"));
    }

    #[tokio::test]
    async fn falls_back_to_the_legacy_endpoint_when_v1_fails() {
        let transport = FakeTransport::new();
        transport.enqueue_status(500).enqueue_json(
            r#"{"code":200,"data":[{"id":33894312,"url":"https://m701.music.126.net/song.mp3","br":320000,"type":"mp3","level":"exhigh","code":200}]}"#,
        );
        let client = test_client(&transport);

        let info = client
            .get_song_playback_info(33_894_312, AudioQuality::ExHigh)
            .await
            .unwrap();

        assert!(info.is_success());
        assert_eq!(info.bitrate, 320_000);
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[0]
                .url
                .ends_with("/weapi/song/enhance/player/url/v1")
        );
        assert!(requests[1].url.ends_with("/weapi/song/enhance/player/url"));
        assert!(
            requests[1]
                .header("Cookie")
                .unwrap()
                .contains("deviceId=LunaticPrincessPc")
        );
        assert_eq!(payload_of(&requests[1])["br"], 320_000);
    }

    #[tokio::test]
    async fn errors_when_no_url_is_returned_and_keeps_the_last_reason() {
        let transport = FakeTransport::new();
        let none = r#"{"code":200,"data":[{"id":186016,"url":null,"code":404,"fee":0}]}"#;
        transport.enqueue_json(none).enqueue_json(none);
        let client = test_client(&transport);

        let error = client
            .get_song_playback_info(186_016, AudioQuality::Standard)
            .await
            .unwrap_err();

        assert!(error.message().contains("404"));
        assert_eq!(transport.requests().len(), 2);
    }

    #[tokio::test]
    async fn retries_lower_qualities_and_reports_the_actual_level() {
        let transport = FakeTransport::new();
        // v1 对新音质整体不可用（HTTP 404）→ 本歌曲剩余音质直接走经典接口。
        transport.enqueue_status(404);
        // 经典接口 lossless：无版权，url 为空。
        transport
            .enqueue_json(r#"{"code":200,"data":[{"id":186016,"url":null,"code":404,"fee":1}]}"#);
        // 降档到 exhigh：返回 320k。
        transport.enqueue_json(
            r#"{"code":200,"data":[{"id":186016,"url":"https://m701.music.126.net/song.mp3","br":320000,"type":"mp3","level":"exhigh","code":200}]}"#,
        );
        let client = test_client(&transport);

        let info = client
            .get_song_playback_info(186_016, AudioQuality::Lossless)
            .await
            .unwrap();

        assert!(info.is_success());
        assert_eq!(info.level.as_deref(), Some("exhigh"));
        assert_eq!(info.bitrate, 320_000);
        let requests = transport.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(payload_of(&requests[1])["br"], 999_000);
        assert_eq!(payload_of(&requests[2])["br"], 320_000);
    }

    #[tokio::test]
    async fn returns_the_actual_lower_level_without_retrying() {
        let transport = FakeTransport::new();
        transport.enqueue_json(
            r#"{"code":200,"data":[{"id":186016,"url":"https://m701.music.126.net/song.flac","br":320000,"type":"flac","level":"exhigh","code":200}]}"#,
        );
        let client = test_client(&transport);

        let info = client
            .get_song_playback_info(186_016, AudioQuality::Lossless)
            .await
            .unwrap();

        assert!(info.is_success());
        assert_eq!(info.level.as_deref(), Some("exhigh"));
        assert_eq!(info.bitrate, 320_000);
        assert_eq!(transport.requests().len(), 1);
    }

    #[tokio::test]
    async fn audio_vivid_tries_the_alias_spelling_before_dropping_a_level() {
        let transport = FakeTransport::new();
        let none = r#"{"code":200,"data":[{"id":5,"url":null,"code":404,"fee":0}]}"#;
        transport.enqueue_json(none);
        transport.enqueue_json(
            r#"{"code":200,"data":[{"id":5,"url":"https://m.example/5.flac","br":999000,"type":"flac","level":"audio_vivid","code":200}]}"#,
        );
        let client = test_client(&transport);

        let info = client
            .get_song_playback_info(5, AudioQuality::AudioVivid)
            .await
            .unwrap();

        assert!(info.is_success());
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(payload_of(&requests[0])["level"], "audio_vivid");
        assert_eq!(payload_of(&requests[1])["level"], "vivid");
    }

    #[tokio::test]
    async fn rejects_invalid_song_ids_without_a_request() {
        let transport = FakeTransport::new();
        let client = test_client(&transport);

        let error = client
            .get_song_playback_info(0, AudioQuality::ExHigh)
            .await
            .unwrap_err();

        assert!(error.message().contains("歌曲 ID"));
        assert!(transport.requests().is_empty());
    }

    #[tokio::test]
    async fn business_not_logged_in_is_kept_recognisable() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":301}"#);
        transport.enqueue_json(r#"{"code":301}"#);
        let client = test_client(&transport);

        let error = client
            .get_song_playback_info(1, AudioQuality::Standard)
            .await
            .unwrap_err();

        assert!(error.is_not_logged_in());
    }

    #[test]
    fn failure_messages_distinguish_vip_copyright_and_access() {
        let describe = |code: i32, fee: i32| {
            map_playback_info(
                &json!({"code":200,"data":[{"id":1,"url":null,"code":code,"fee":fee}]}),
                1,
            )
            .error_message
            .unwrap()
        };
        assert!(describe(404, 1).contains("VIP"));
        assert!(describe(404, 0).contains("版权受限或已下架"));
        assert!(describe(403, 0).contains("code=403"));
        assert!(describe(-100, 0).contains("code=-100"));
        assert_eq!(describe(0, 0), "接口未返回播放地址");
    }

    #[test]
    fn malformed_payloads_become_descriptive_failures() {
        let missing = map_playback_info(&json!({"code":200}), 9);
        assert!(!missing.is_success());
        assert_eq!(missing.song_id, 9);
        assert!(missing.error_message.unwrap().contains("data 缺失"));

        let empty = map_playback_info(&json!({"data":[]}), 9);
        assert!(empty.error_message.unwrap().contains("data 为空"));

        let bad_item = map_playback_info(&json!({"data":[3]}), 9);
        assert!(bad_item.error_message.unwrap().contains("data 项非法"));

        // url 是空白串等同于没有。
        let blank = map_playback_info(&json!({"data":[{"id":9,"url":"  ","code":404}]}), 9);
        assert!(!blank.is_success());
    }

    #[test]
    fn a_trial_without_bounds_still_counts_as_a_trial() {
        let info = map_playback_info(
            &json!({"data":[{"id":9,"url":"https://x/9.mp3","freeTrialInfo":{}}]}),
            9,
        );
        assert!(info.is_trial);
        assert_eq!(info.trial_start_seconds, None);
        assert_eq!(info.trial_end_seconds, None);

        let none = map_playback_info(
            &json!({"data":[{"id":9,"url":"https://x/9.mp3","freeTrialInfo":null}]}),
            9,
        );
        assert!(!none.is_trial);
    }
}
