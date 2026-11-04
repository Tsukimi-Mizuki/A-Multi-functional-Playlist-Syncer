//! 歌曲搜索。对应 C# `NeteaseApiClient.Search.cs`。
//!
//! 公开的网页搜索接口 `GET /api/search/get/web`：匿名可用，但连续多次请求会触发 `code=406` 限流
//! （匿名与登录态都可能），所以调用方要克制地发请求。

use serde_json::Value;

use super::client::{MUSIC_BASE_URL, NeteaseClient, escape_data_string};
use super::error::NeteaseError;
use super::json;
use super::mappers;
use super::models::NeteaseSong;
use crate::library::text::is_blank;

/// 搜索请求数量的合法范围（与 C# 的 `Math.Clamp(limit, 1, 50)` 一致）。
const MIN_LIMIT: usize = 1;
const MAX_LIMIT: usize = 50;

impl NeteaseClient {
    /// 按关键词搜索歌曲。关键词为空白时直接返回空列表（不发请求）；
    /// 缺少 `result.songs` 时返回空列表；id 非法或歌名为空白的项被丢弃。
    pub async fn search_songs(
        &self,
        keyword: &str,
        limit: usize,
    ) -> Result<Vec<NeteaseSong>, NeteaseError> {
        if is_blank(keyword) {
            return Ok(Vec::new());
        }

        let limit = limit.clamp(MIN_LIMIT, MAX_LIMIT);
        let url = format!(
            "{MUSIC_BASE_URL}/api/search/get/web?s={}&type=1&limit={limit}&offset=0",
            escape_data_string(keyword.trim())
        );
        let document = self.get_json(&url, "搜索网易云歌曲").await?;

        // 实网（2026-11 探针）：`/api/search/get/web` 现在返回一段十六进制密文字符串而不是 JSON 对象，
        // 解析不了。给出明确的错误，而不是悄悄当成“没有结果”。
        if matches!(json::property(&document, "result"), Some(Value::String(_))) {
            return Err(NeteaseError::protocol(
                "搜索网易云歌曲失败：接口返回了加密数据，暂时无法使用，请改用 QQ 音乐",
            ));
        }

        let songs = json::property(&document, "result").and_then(|result| {
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
}

#[cfg(test)]
mod tests {
    use crate::test_util::{FakeTransport, test_client};

    const SEARCH_JSON: &str = r#"{
      "code": 200,
      "result": { "songCount": 3, "songs": [
        { "id": 186016, "name": "晴天", "duration": 269000,
          "artists": [ { "id": 6452, "name": "周杰伦" } ],
          "album": { "id": 18877, "name": "叶惠美", "publishTime": 1058284800000 },
          "alias": [], "fee": 8 },
        { "id": 0, "name": "无效 ID" },
        { "id": 7, "name": "  " }
      ] }
    }"#;

    #[tokio::test]
    async fn maps_search_results_and_drops_invalid_entries() {
        let transport = FakeTransport::new();
        transport.enqueue_json(SEARCH_JSON);
        let client = test_client(&transport);

        let songs = client.search_songs("  晴天 周杰伦 ", 10).await.unwrap();

        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].id, 186016);
        assert_eq!(songs[0].artist_text(), "周杰伦");
        assert_eq!(songs[0].album.name, "叶惠美");
        assert_eq!(songs[0].duration_ms, 269_000);
        assert_eq!(songs[0].release_date.as_deref(), Some("2003-07-16"));

        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].url,
            "https://music.163.com/api/search/get/web?s=%E6%99%B4%E5%A4%A9%20%E5%91%A8%E6%9D%B0%E4%BC%A6&type=1&limit=10&offset=0"
        );
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

        let urls: Vec<String> = transport.requests().into_iter().map(|r| r.url).collect();
        assert!(urls[0].contains("&limit=1&"));
        assert!(urls[1].contains("&limit=50&"));
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
    }

    #[tokio::test]
    async fn an_encrypted_result_payload_is_reported_instead_of_looking_like_no_results() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"result":"35b1748964af8a7c1d883e3a6f3c","code":200}"#);
        let client = test_client(&transport);

        let error = client.search_songs("Lemon", 5).await.unwrap_err();

        assert!(error.message().contains("加密数据"), "{}", error.message());
    }

    #[tokio::test]
    async fn rate_limit_code_406_is_reported_as_an_api_error() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":406,"message":"too fast"}"#);
        let client = test_client(&transport);

        let error = client.search_songs("a", 5).await.unwrap_err();

        assert_eq!(error.code(), Some(406));
        assert!(error.message().contains("搜索网易云歌曲"));
    }
}
