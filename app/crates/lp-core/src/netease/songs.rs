//! 歌曲详情接口。对应 C# `NeteaseApiClient.Songs.cs` 中的 `GetSongDetailsAsync` 与批量请求。
//!
//! 播放地址（取链）在 [`super::player`]。

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use super::client::NeteaseClient;
use super::error::NeteaseError;
use super::json;
use super::mappers;
use super::models::NeteaseSong;

/// `song/detail` 单次请求的最大歌曲数。
pub const SONG_DETAIL_BATCH_SIZE: usize = 500;

impl NeteaseClient {
    /// 批量获取歌曲详情，按调用方传入的顺序返回（接口返回顺序不可依赖；查不到的 id 跳过）。
    ///
    /// 非法 id（≤ 0）被忽略，重复 id 只请求一次，每 500 个一批。任何一批失败都会直接返回错误。
    pub async fn get_song_details(
        &self,
        song_ids: &[i64],
    ) -> Result<Vec<NeteaseSong>, NeteaseError> {
        let mut seen = HashSet::new();
        let unique: Vec<i64> = song_ids
            .iter()
            .copied()
            .filter(|id| *id > 0 && seen.insert(*id))
            .collect();
        if unique.is_empty() {
            return Ok(Vec::new());
        }

        let mut songs_by_id: HashMap<i64, NeteaseSong> = HashMap::with_capacity(unique.len());
        for batch in unique.chunks(SONG_DETAIL_BATCH_SIZE) {
            for song in self.fetch_song_detail_batch(batch).await? {
                songs_by_id.insert(song.id, song);
            }
        }

        Ok(song_ids
            .iter()
            .filter(|id| **id > 0)
            .filter_map(|id| songs_by_id.get(id).cloned())
            .collect())
    }

    /// 调用 `/weapi/v3/song/detail` 获取一批歌曲（最多 500 个 id）。
    pub(crate) async fn fetch_song_detail_batch(
        &self,
        song_ids: &[i64],
    ) -> Result<Vec<NeteaseSong>, NeteaseError> {
        let ids_json = format!(
            "[{}]",
            song_ids
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        let c_json = format!(
            "[{}]",
            song_ids
                .iter()
                .map(|id| format!("{{\"id\":{id}}}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let document = self
            .post_weapi(
                "/weapi/v3/song/detail",
                &json!({ "c": c_json, "ids": ids_json }),
                "获取歌曲详情",
                false,
            )
            .await?;

        let Some(Value::Array(items)) = json::property(&document, "songs") else {
            return Ok(Vec::new());
        };
        let privileges = mappers::map_privileges(&document);
        Ok(mappers::map_songs(items, &privileges))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::NeteaseSession;
    use crate::test_util::{
        FakeTransport, parse_song_ids, song_detail_response_for_ids, test_client,
    };

    const FULL_SONG_JSON: &str = r#"{
      "code": 200,
      "songs": [{
        "id": 186016, "name": "晴天",
        "ar": [ { "id": 6452, "name": "周杰伦" } ],
        "al": { "id": 13628, "name": "叶惠美", "picUrl": "https://p1.music.126.net/cover.jpg", "publishTime": 1058284800000 },
        "dt": 269000, "no": 1, "cd": "1", "alias": [ "Sunny Day" ],
        "publishTime": 1058284800000, "fee": 1
      }],
      "privileges": [ { "id": 186016, "maxbr": 999000, "fee": 1 } ]
    }"#;

    fn ids(songs: &[NeteaseSong]) -> Vec<i64> {
        songs.iter().map(|song| song.id).collect()
    }

    #[tokio::test]
    async fn maps_all_song_fields_and_posts_the_detail_endpoint() {
        let transport = FakeTransport::new();
        transport.enqueue_json(FULL_SONG_JSON);
        let client = test_client(&transport);

        let songs = client.get_song_details(&[186016]).await.unwrap();

        assert_eq!(songs.len(), 1);
        let song = &songs[0];
        assert_eq!(song.id, 186016);
        assert_eq!(song.name, "晴天");
        assert_eq!(song.artists.len(), 1);
        assert_eq!(song.artists[0].id, 6452);
        assert_eq!(song.artist_text(), "周杰伦");
        assert_eq!(song.album.id, 13628);
        assert_eq!(song.album.name, "叶惠美");
        assert_eq!(song.cover_url(), Some("https://p1.music.126.net/cover.jpg"));
        assert_eq!(song.alias.as_deref(), Some("Sunny Day"));
        assert_eq!(song.display_name(), "晴天 (Sunny Day)");
        assert_eq!(song.duration_ms, 269_000);
        assert_eq!(song.track_number, 1);
        assert_eq!(song.disc_number, 1);
        assert_eq!(song.fee, 1);
        assert_eq!(song.max_bitrate, 999_000);
        assert_eq!(song.release_date.as_deref(), Some("2003-07-16"));

        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].url.ends_with("/weapi/v3/song/detail"));
        assert_eq!(parse_song_ids(&requests[0]), vec![186016]);
    }

    #[tokio::test]
    async fn sends_both_c_and_ids_fields() {
        let transport = FakeTransport::new();
        transport.set_fallback(|request| song_detail_response_for_ids(&parse_song_ids(request)));
        let client = test_client(&transport);

        client.get_song_details(&[5, 6]).await.unwrap();

        let business = crate::test_util::decrypt_business_json(
            &transport.requests()[0],
            crate::test_util::FIXED_SECRET,
        );
        let payload: Value = serde_json::from_str(&business).unwrap();
        assert_eq!(payload["ids"], "[5,6]");
        assert_eq!(payload["c"], r#"[{"id":5},{"id":6}]"#);
        assert_eq!(payload["csrf_token"], "");
    }

    #[tokio::test]
    async fn preserves_requested_order_despite_response_order() {
        let transport = FakeTransport::new();
        transport.set_fallback(|request| song_detail_response_for_ids(&parse_song_ids(request)));
        let client = test_client(&transport);

        let songs = client.get_song_details(&[3, 1, 2]).await.unwrap();

        assert_eq!(ids(&songs), vec![3, 1, 2]);
        assert_eq!(transport.requests().len(), 1);
    }

    #[tokio::test]
    async fn batches_more_than_500_ids() {
        let transport = FakeTransport::new();
        transport.set_fallback(|request| song_detail_response_for_ids(&parse_song_ids(request)));
        let client = test_client(&transport);
        let wanted: Vec<i64> = (1..=600).collect();

        let songs = client.get_song_details(&wanted).await.unwrap();

        assert_eq!(songs.len(), 600);
        assert_eq!(ids(&songs), wanted);
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(parse_song_ids(&requests[0]).len(), 500);
        assert_eq!(parse_song_ids(&requests[1]).len(), 100);
    }

    #[tokio::test]
    async fn empty_or_invalid_input_sends_nothing() {
        let transport = FakeTransport::new();
        let client = test_client(&transport);

        assert!(client.get_song_details(&[]).await.unwrap().is_empty());
        assert!(client.get_song_details(&[0, -3]).await.unwrap().is_empty());
        assert!(transport.requests().is_empty());
    }

    #[tokio::test]
    async fn duplicate_ids_are_requested_once_and_missing_ids_are_skipped() {
        let transport = FakeTransport::new();
        transport.enqueue_json(
            r#"{"code":200,"songs":[{"id":1,"name":"a"},{"id":3,"name":"c"}],"privileges":[]}"#,
        );
        let client = test_client(&transport);

        let songs = client.get_song_details(&[1, 2, 1, 3]).await.unwrap();

        assert_eq!(parse_song_ids(&transport.requests()[0]), vec![1, 2, 3]);
        // 与 C# 一致：按调用方传入的顺序（含重复）返回，查不到的 2 被跳过。
        assert_eq!(ids(&songs), vec![1, 1, 3]);
    }

    #[tokio::test]
    async fn per_song_privilege_wins_over_top_level_privileges() {
        let transport = FakeTransport::new();
        transport.enqueue_json(
            r#"{"code":200,"songs":[{"id":186016,"name":"晴天","fee":1,"privilege":{"maxbr":320000,"fee":1}}],
                "privileges":[{"id":186016,"maxbr":999000,"fee":0}]}"#,
        );
        let client = test_client(&transport);

        let songs = client.get_song_details(&[186016]).await.unwrap();

        assert_eq!(songs[0].max_bitrate, 320_000);
    }

    #[tokio::test]
    async fn song_detail_does_not_send_the_playback_client_cookie() {
        let transport = FakeTransport::new();
        transport.enqueue_json(FULL_SONG_JSON);
        let client = test_client(&transport);
        client.set_session(Some(NeteaseSession {
            cookie: "MUSIC_U=session123".to_owned(),
            ..Default::default()
        }));

        client.get_song_details(&[186016]).await.unwrap();

        let cookie = transport.requests()[0]
            .header("Cookie")
            .expect("请求带 Cookie")
            .to_owned();
        assert!(cookie.contains("MUSIC_U=session123"));
        assert!(!cookie.contains("deviceId="));
        assert!(!cookie.contains("appver=2.10.6"));
    }

    #[tokio::test]
    async fn business_errors_propagate() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":301}"#);
        let client = test_client(&transport);

        let error = client.get_song_details(&[1]).await.unwrap_err();

        assert!(error.is_not_logged_in());
    }
}
