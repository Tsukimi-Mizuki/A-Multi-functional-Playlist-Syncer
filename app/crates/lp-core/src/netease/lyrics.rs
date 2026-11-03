//! 歌词接口。对应 C# `NeteaseApiClient.Lyrics.cs`。
//!
//! 三条路径依次兜底，任何一条返回即止：
//!
//! 1. 公开接口 `GET /api/song/lyric`：匿名可用，且 `yv=1` 时能拿到干净的 YRC 逐字歌词（实测）；
//! 2. `POST /weapi/song/lyric`（v0，YRC 为纯文本行）；
//! 3. `POST /weapi/song/lyric/v1`（新版 JSON 风格，仅兜底，LRC / YRC 可能是 JSON 行）。
//!
//! 前两条失败（任何 [`NeteaseError`]，含网络错误）都静默落到下一条；三条全败时抛最后一条的错误，
//! 保留原 `code` / 错误类别，所以最终的 301 仍然能被调用方识别为“未登录”。

use serde_json::json;

use super::client::{MUSIC_BASE_URL, NeteaseClient};
use super::error::NeteaseError;
use super::mappers;
use crate::lyrics::RawLyrics;

const OPERATION: &str = "获取歌词";

impl NeteaseClient {
    /// 获取一首歌的原始歌词（LRC / 译文 / 罗马音 / YRC）。纯音乐或未收录时返回空的 [`RawLyrics`]。
    pub async fn get_lyrics(&self, song_id: i64) -> Result<RawLyrics, NeteaseError> {
        if song_id <= 0 {
            return Err(NeteaseError::protocol("歌曲 ID 必须大于 0。"));
        }

        // 1) 公开接口。
        let url =
            format!("{MUSIC_BASE_URL}/api/song/lyric?id={song_id}&lv=-1&kv=-1&tv=-1&rv=-1&yv=1");
        if let Ok(document) = self.get_json(&url, OPERATION).await {
            return Ok(mappers::map_lyrics(&document));
        }

        // 2) weapi v0。
        let payload = json!({ "id": song_id, "lv": -1, "kv": 0, "tv": -1, "rv": -1, "yv": 1 });
        if let Ok(document) = self
            .post_weapi("/weapi/song/lyric", &payload, OPERATION, false)
            .await
        {
            return Ok(mappers::map_lyrics(&document));
        }

        // 3) weapi v1（兜底）。
        let payload = json!({
            "id": song_id, "cp": false, "tv": 0, "lv": 0, "rv": 0, "kv": 0, "yv": 0, "ytv": 0, "yrv": 0
        });
        match self
            .post_weapi("/weapi/song/lyric/v1", &payload, OPERATION, false)
            .await
        {
            Ok(document) => Ok(mappers::map_lyrics(&document)),
            Err(error) => Err(NeteaseError::new(
                format!("获取歌曲 {song_id} 歌词失败：{}", error.message()),
                error.code(),
                error.kind(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::netease::NeteaseErrorKind;
    use crate::test_util::{FIXED_SECRET, FakeTransport, decrypt_business_json, test_client};

    const LYRIC_JSON: &str = r#"{
      "code": 200,
      "lrc": { "version": 10, "lyric": "[00:00.000]作词 : 测试\n[00:12.570]第一句\n" },
      "tlyric": { "version": 0, "lyric": "[00:12.570]First line\n" },
      "romalrc": { "version": 0, "lyric": "[00:12.570]romaji\n" },
      "yrc": { "version": 3, "lyric": "[12580,3470](12580,250,0)第(12830,300,0)一(13130,370,0)句\n" }
    }"#;

    #[tokio::test]
    async fn maps_all_lyric_sections_from_the_public_endpoint() {
        let transport = FakeTransport::new();
        transport.enqueue_json(LYRIC_JSON);
        let client = test_client(&transport);

        let lyrics = client.get_lyrics(33894312).await.unwrap();

        assert!(lyrics.lrc.as_deref().unwrap().contains("[00:12.570]第一句"));
        assert!(
            lyrics
                .translated_lrc
                .as_deref()
                .unwrap()
                .contains("First line")
        );
        assert!(lyrics.romanized_lrc.as_deref().unwrap().contains("romaji"));
        assert!(lyrics.yrc.as_deref().unwrap().contains("(12580,250,0)"));
        assert_eq!(lyrics.qrc, None);

        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method.as_str(), "GET");
        assert!(
            request
                .url
                .starts_with("https://music.163.com/api/song/lyric?")
        );
        for expected in ["id=33894312", "yv=1", "lv=-1", "tv=-1", "kv=-1", "rv=-1"] {
            assert!(
                request.url.contains(expected),
                "缺少 {expected}：{}",
                request.url
            );
        }
        assert_eq!(
            request.header("Cookie"),
            Some("os=pc; appver=8.9.70"),
            "匿名请求使用固定的基础 Cookie"
        );
    }

    #[tokio::test]
    async fn falls_back_to_weapi_when_the_public_endpoint_fails() {
        let transport = FakeTransport::new();
        transport.enqueue_status(500).enqueue_json(LYRIC_JSON);
        let client = test_client(&transport);

        let lyrics = client.get_lyrics(33894312).await.unwrap();

        assert!(!lyrics.lrc.unwrap().is_empty());
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].method.as_str(), "POST");
        assert!(requests[1].url.ends_with("/weapi/song/lyric"));

        let payload: Value =
            serde_json::from_str(&decrypt_business_json(&requests[1], FIXED_SECRET)).unwrap();
        assert_eq!(payload["id"], 33894312);
        assert_eq!(payload["yv"], 1);
        assert_eq!(payload["tv"], -1);
        assert_eq!(payload["kv"], 0);
        assert_eq!(payload["csrf_token"], "");
    }

    #[tokio::test]
    async fn falls_back_to_weapi_v1_when_previous_sources_fail() {
        let transport = FakeTransport::new();
        transport
            .enqueue_status(500)
            .enqueue_json(r#"{"code":500,"message":"接口异常"}"#)
            .enqueue_json(LYRIC_JSON);
        let client = test_client(&transport);

        let lyrics = client.get_lyrics(33894312).await.unwrap();

        assert!(!lyrics.yrc.unwrap().is_empty());
        let requests = transport.requests();
        assert_eq!(requests.len(), 3);
        assert!(requests[2].url.ends_with("/weapi/song/lyric/v1"));
        let payload: Value =
            serde_json::from_str(&decrypt_business_json(&requests[2], FIXED_SECRET)).unwrap();
        assert_eq!(payload["cp"], false);
        assert_eq!(payload["ytv"], 0);
        assert_eq!(payload["yrv"], 0);
    }

    #[tokio::test]
    async fn returns_empty_raw_lyrics_for_nolyric() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":200,"nolyric":true}"#);
        let client = test_client(&transport);

        let lyrics = client.get_lyrics(186016).await.unwrap();

        assert_eq!(lyrics, RawLyrics::default());
        assert_eq!(transport.requests().len(), 1);
    }

    #[tokio::test]
    async fn fails_with_a_lyric_message_when_all_sources_fail() {
        let transport = FakeTransport::new();
        transport
            .enqueue_status(500)
            .enqueue_status(500)
            .enqueue_status(500);
        let client = test_client(&transport);

        let error = client.get_lyrics(186016).await.unwrap_err();

        assert!(error.message().contains("歌词"));
        assert!(error.message().contains("186016"));
        assert_eq!(error.kind(), NeteaseErrorKind::HttpStatus);
        assert_eq!(transport.requests().len(), 3);
    }

    #[tokio::test]
    async fn the_last_error_keeps_its_business_code_so_301_is_still_recognised() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json(r#"{"code":301}"#)
            .enqueue_json(r#"{"code":301}"#)
            .enqueue_json(r#"{"code":301}"#);
        let client = test_client(&transport);

        let error = client.get_lyrics(186016).await.unwrap_err();

        assert!(error.is_not_logged_in());
        assert!(error.message().starts_with("获取歌曲 186016 歌词失败"));
    }

    #[tokio::test]
    async fn rejects_non_positive_song_ids_without_a_request() {
        let transport = FakeTransport::new();
        let client = test_client(&transport);

        assert!(client.get_lyrics(0).await.is_err());
        assert!(client.get_lyrics(-7).await.is_err());
        assert!(transport.requests().is_empty());
    }

    #[test]
    fn map_lyrics_treats_uncollected_and_blank_sections_as_missing() {
        let uncollected = mappers::map_lyrics(&json!({"code": 200, "uncollected": true}));
        assert_eq!(uncollected, RawLyrics::default());

        let blank = mappers::map_lyrics(&json!({
            "lrc": {"lyric": "  \n "},
            "tlyric": {"lyric": ""},
            "romalrc": "不是对象",
            "yrc": {"lyric": "[1,2](1,2,0)字"}
        }));
        assert_eq!(blank.lrc, None);
        assert_eq!(blank.translated_lrc, None);
        assert_eq!(blank.romanized_lrc, None);
        assert_eq!(blank.yrc.as_deref(), Some("[1,2](1,2,0)字"));
    }

    #[test]
    fn map_lyrics_accepts_string_booleans_for_the_empty_flags() {
        assert_eq!(
            mappers::map_lyrics(&json!({"nolyric": "true", "lrc": {"lyric": "[0:1]x"}})),
            RawLyrics::default()
        );
        assert!(
            mappers::map_lyrics(&json!({"nolyric": false, "lrc": {"lyric": "[0:1]x"}}))
                .lrc
                .is_some()
        );
    }
}
