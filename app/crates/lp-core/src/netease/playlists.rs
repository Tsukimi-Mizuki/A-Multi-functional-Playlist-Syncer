//! 用户歌单列表与歌单详情。对应 C# `NeteaseApiClient.Playlists.cs`。
//!
//! 大歌单的详情接口只内嵌前若干首歌曲：`trackIds` 是完整且有序的 id 来源，
//! 缺失的歌曲按 500 首 / 批调用 `/weapi/v3/song/detail` 补齐，最后按 `trackIds` 的顺序还原。

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use super::client::NeteaseClient;
use super::error::NeteaseError;
use super::json;
use super::mappers;
use super::models::{NeteaseSong, PlaylistDetail, PlaylistSummary};
use super::songs::SONG_DETAIL_BATCH_SIZE;

/// 用户歌单每页数量；1000 已能覆盖绝大多数账号。
const USER_PLAYLIST_PAGE_SIZE: usize = 1000;
/// 仍按 `more` / 页长防御性翻页，最多翻这么多页。
const USER_PLAYLIST_MAX_PAGES: usize = 10;

/// 歌单曲目加载进度回调：`(已取得详情的歌曲数, 歌单总曲目数)`。
pub type ProgressFn<'a> = dyn Fn(usize, usize) + Send + Sync + 'a;

impl NeteaseClient {
    /// 获取用户的全部歌单（创建的 + 收藏的），按接口顺序返回并按 id 去重。
    pub async fn get_user_playlists(
        &self,
        user_id: i64,
    ) -> Result<Vec<PlaylistSummary>, NeteaseError> {
        if user_id <= 0 {
            return Err(NeteaseError::protocol("用户 ID 必须大于 0"));
        }

        let mut playlists = Vec::new();
        let mut seen = HashSet::new();
        let mut offset = 0usize;

        for _ in 0..USER_PLAYLIST_MAX_PAGES {
            let document = self
                .post_weapi(
                    "/weapi/user/playlist",
                    &json!({
                        "uid": user_id,
                        "limit": USER_PLAYLIST_PAGE_SIZE,
                        "offset": offset,
                        "includeVideo": false,
                    }),
                    "获取用户歌单",
                    false,
                )
                .await?;

            let Some(Value::Array(items)) = json::property(&document, "playlist") else {
                break;
            };
            for item in items {
                let summary = mappers::map_playlist_summary(item);
                if summary.id > 0 && seen.insert(summary.id) {
                    playlists.push(summary);
                }
            }

            if items.len() < USER_PLAYLIST_PAGE_SIZE {
                break;
            }
            if json::bool_or_none(&document, "more") == Some(false) {
                break;
            }
            offset += USER_PLAYLIST_PAGE_SIZE;
        }

        Ok(playlists)
    }

    /// 获取歌单详情与完整曲目。
    ///
    /// `progress` 在“还有歌曲要补齐”时被调用：先报内嵌曲目数，之后每补完一批再报一次。
    ///
    /// 某一批歌曲详情补不回来（重试一次仍失败）时不会让整个歌单失败：
    /// 该批歌曲被略过，结果的 `is_partial` 为真。但“未登录”（业务 301）会立即返回错误，
    /// 因为登录态已失效，继续请求没有意义，调用方需要走登录复核流程。
    pub async fn get_playlist_detail(
        &self,
        playlist_id: i64,
        progress: Option<&ProgressFn<'_>>,
    ) -> Result<PlaylistDetail, NeteaseError> {
        if playlist_id <= 0 {
            return Err(NeteaseError::protocol("歌单 ID 必须大于 0"));
        }

        let document = self
            .post_weapi(
                "/weapi/v6/playlist/detail",
                &json!({ "id": playlist_id, "n": 1000, "s": 8 }),
                "获取歌单详情",
                false,
            )
            .await?;

        let playlist = match json::property(&document, "playlist") {
            Some(playlist) if playlist.is_object() => playlist,
            _ => {
                return Err(NeteaseError::api(
                    format!("歌单 {playlist_id} 不存在或无权访问"),
                    404,
                ));
            }
        };

        // 1) 先收下内嵌 tracks（v6 详情首次最多返回 n 首）。
        //    maxbr / fee 在响应顶层 privileges 数组里，按 id 关联。
        let mut songs_by_id: HashMap<i64, NeteaseSong> = HashMap::new();
        let mut embedded_ids: Vec<i64> = Vec::new();
        if let Some(Value::Array(tracks)) = json::property(playlist, "tracks") {
            let privileges = mappers::map_privileges(&document);
            for song in mappers::map_songs(tracks, &privileges) {
                let id = song.id;
                if songs_by_id.insert(id, song).is_none() {
                    embedded_ids.push(id);
                }
            }
        }

        // 2) trackIds 是全量顺序来源（去重、丢弃非法 id）。
        let mut track_ids: Vec<i64> = Vec::new();
        let mut track_id_set: HashSet<i64> = HashSet::new();
        if let Some(Value::Array(elements)) = json::property(playlist, "trackIds") {
            for element in elements {
                let id = if element.is_object() {
                    json::i64_or(element, "id", 0)
                } else {
                    json::as_i64(element).unwrap_or(0)
                };
                if id > 0 && track_id_set.insert(id) {
                    track_ids.push(id);
                }
            }
        }
        if track_ids.is_empty() {
            track_ids = embedded_ids;
        }

        // 3) 缺失曲目按 trackIds 顺序批量补齐（每批 <= 500）。
        let missing: Vec<i64> = track_ids
            .iter()
            .copied()
            .filter(|id| !songs_by_id.contains_key(id))
            .collect();
        let total = track_ids.len();
        let loaded_count = |songs: &HashMap<i64, NeteaseSong>| {
            track_ids.iter().filter(|id| songs.contains_key(id)).count()
        };
        if !missing.is_empty()
            && let Some(report) = progress
        {
            report(loaded_count(&songs_by_id), total);
        }
        for batch in missing.chunks(SONG_DETAIL_BATCH_SIZE) {
            if let Some(fetched) = self.fetch_batch_tolerant(batch).await? {
                for song in fetched {
                    songs_by_id.insert(song.id, song);
                }
            }
            if let Some(report) = progress {
                report(loaded_count(&songs_by_id), total);
            }
        }

        let songs: Vec<NeteaseSong> = track_ids
            .iter()
            .filter_map(|id| songs_by_id.remove(id))
            .collect();
        let track_count = json::property(playlist, "trackCount")
            .and_then(json::as_i64)
            .map_or(total as i32, |count| count as i32);
        Ok(PlaylistDetail {
            id: json::i64_or(playlist, "id", playlist_id),
            name: json::string_or_none(playlist, "name").unwrap_or_default(),
            cover_url: json::string_or_none(playlist, "coverImgUrl"),
            description: json::string_or_none(playlist, "description"),
            creator_nickname: mappers::creator_nickname(playlist),
            track_count,
            is_partial: songs.len() < total,
            songs,
            track_ids,
        })
    }

    /// 取一批歌曲详情：失败时重试一次；仍失败返回 `Ok(None)`（该批被略过），业务 301 直接返回错误。
    async fn fetch_batch_tolerant(
        &self,
        batch: &[i64],
    ) -> Result<Option<Vec<NeteaseSong>>, NeteaseError> {
        for _attempt in 0..2 {
            match self.fetch_song_detail_batch(batch).await {
                Ok(songs) => return Ok(Some(songs)),
                Err(error) if error.is_not_logged_in() => return Err(error),
                Err(_) => {}
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::netease::error::NeteaseErrorKind;
    use crate::netease::http::TransportError;
    use crate::test_util::{
        FIXED_SECRET, FakeTransport, decrypt_business_json, parse_song_ids,
        song_detail_response_for_ids, test_client,
    };

    const TRACK_JSON: &str = r#"{"id":22,"name":"歌曲22","ar":[{"id":1,"name":"歌手"}],"al":{"id":2,"name":"专辑","picUrl":"https://p1/cover.jpg"},"dt":2000,"fee":0}"#;

    fn song_ids(detail: &PlaylistDetail) -> Vec<i64> {
        detail.songs.iter().map(|song| song.id).collect()
    }

    fn with_detail_responder(transport: &Arc<FakeTransport>) {
        transport.set_fallback(|request| song_detail_response_for_ids(&parse_song_ids(request)));
    }

    fn build_playlist_page(count: usize, start_id: usize, more: bool) -> String {
        let mut text = format!(r#"{{"code":200,"more":{more},"playlist":["#);
        for index in 0..count {
            if index > 0 {
                text.push(',');
            }
            let id = start_id + index;
            write!(
                text,
                r#"{{"id":{id},"name":"歌单{id}","coverImgUrl":null,"trackCount":{index}}}"#
            )
            .unwrap();
        }
        text.push_str("]}");
        text
    }

    fn build_large_playlist(track_count: usize) -> String {
        let ids: Vec<String> = (1..=track_count)
            .map(|id| format!(r#"{{"id":{id}}}"#))
            .collect();
        format!(
            r#"{{"code":200,"playlist":{{"id":1,"name":"大歌单","trackCount":{track_count},"tracks":[],"trackIds":[{}]}}}}"#,
            ids.join(",")
        )
    }

    // ---- 用户歌单 ----

    #[tokio::test]
    async fn user_playlists_map_summary_fields_and_send_the_expected_payload() {
        let transport = FakeTransport::new();
        transport.enqueue_json(
            r#"{
              "code": 200, "more": false,
              "playlist": [
                { "id": 1001, "name": "我喜欢的音乐", "coverImgUrl": "https://p1.music.126.net/cover.jpg",
                  "trackCount": 25, "creator": { "nickname": "小明" }, "description": "我的收藏" },
                { "id": 1002, "name": "第二个歌单", "coverImgUrl": null, "trackCount": 3, "creator": null }
              ]
            }"#,
        );
        let client = test_client(&transport);

        let playlists = client.get_user_playlists(42).await.unwrap();

        assert_eq!(playlists.len(), 2);
        assert_eq!(playlists[0].id, 1001);
        assert_eq!(playlists[0].name, "我喜欢的音乐");
        assert_eq!(
            playlists[0].cover_url.as_deref(),
            Some("https://p1.music.126.net/cover.jpg")
        );
        assert_eq!(playlists[0].track_count, 25);
        assert_eq!(playlists[0].creator_nickname.as_deref(), Some("小明"));
        assert_eq!(playlists[0].description.as_deref(), Some("我的收藏"));
        assert_eq!(playlists[1].track_count, 3);
        assert_eq!(playlists[1].creator_nickname, None);
        assert_eq!(playlists[1].cover_url, None);

        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].url.ends_with("/weapi/user/playlist"));
        let payload: Value =
            serde_json::from_str(&decrypt_business_json(&requests[0], FIXED_SECRET)).unwrap();
        assert_eq!(payload["uid"], 42);
        assert_eq!(payload["limit"], 1000);
        assert_eq!(payload["offset"], 0);
        assert_eq!(payload["includeVideo"], false);
        assert_eq!(payload["csrf_token"], "");
    }

    #[tokio::test]
    async fn user_playlists_paginate_when_the_page_is_full() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json(&build_playlist_page(1000, 1, true))
            .enqueue_json(&build_playlist_page(1, 1001, false));
        let client = test_client(&transport);

        let playlists = client.get_user_playlists(1).await.unwrap();

        assert_eq!(playlists.len(), 1001);
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        let second: Value =
            serde_json::from_str(&decrypt_business_json(&requests[1], FIXED_SECRET)).unwrap();
        assert_eq!(second["offset"], 1000);
    }

    #[tokio::test]
    async fn user_playlists_stop_paging_when_more_is_false_even_with_a_full_page() {
        let transport = FakeTransport::new();
        transport.enqueue_json(&build_playlist_page(1000, 1, false));
        let client = test_client(&transport);

        assert_eq!(client.get_user_playlists(1).await.unwrap().len(), 1000);
        assert_eq!(transport.requests().len(), 1);
    }

    #[tokio::test]
    async fn user_playlists_cap_the_page_count() {
        let transport = FakeTransport::new();
        transport.set_fallback(|_| {
            // 永远是满页、永远 more=true；ID 每页相同，去重后只剩 1000 个。
            FakeTransport::json_response(&build_playlist_page(1000, 1, true))
        });
        let client = test_client(&transport);

        let playlists = client.get_user_playlists(1).await.unwrap();

        assert_eq!(playlists.len(), 1000);
        assert_eq!(transport.requests().len(), USER_PLAYLIST_MAX_PAGES);
    }

    #[tokio::test]
    async fn user_playlists_dedupe_by_id_and_skip_invalid_ids() {
        let transport = FakeTransport::new();
        transport.enqueue_json(
            r#"{"code":200,"playlist":[{"id":5,"name":"a"},{"id":5,"name":"dup"},{"id":0,"name":"bad"},{"name":"noid"},{"id":6,"name":"b"}]}"#,
        );
        let client = test_client(&transport);

        let playlists = client.get_user_playlists(1).await.unwrap();

        let names: Vec<&str> = playlists.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
    }

    #[tokio::test]
    async fn user_playlists_without_a_playlist_array_are_empty() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":200}"#);
        let client = test_client(&transport);

        assert!(client.get_user_playlists(1).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn user_playlists_reject_non_positive_user_ids_without_a_request() {
        let transport = FakeTransport::new();
        let client = test_client(&transport);

        assert!(client.get_user_playlists(0).await.is_err());
        assert!(transport.requests().is_empty());
    }

    #[tokio::test]
    async fn user_playlists_propagate_code_301() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":301,"msg":"需要登录"}"#);
        let client = test_client(&transport);

        let error = client.get_user_playlists(1).await.unwrap_err();

        assert!(error.is_not_logged_in());
        assert_eq!(error.code(), Some(301));
    }

    // ---- 歌单详情 ----

    #[tokio::test]
    async fn detail_fetches_missing_tracks_in_track_ids_order() {
        let detail_json = r#"{
          "code": 200,
          "playlist": {
            "id": 100, "name": "测试歌单", "coverImgUrl": "https://p1/cover.jpg", "description": "描述",
            "trackCount": 3, "creator": { "nickname": "创建者" },
            "tracks": [TRACK_JSON],
            "trackIds": [ { "id": 11 }, { "id": 22 }, { "id": 33 } ]
          },
          "privileges": [ { "id": 22, "maxbr": 999000, "fee": 1 } ]
        }"#
        .replace("TRACK_JSON", TRACK_JSON);
        let transport = FakeTransport::new();
        transport.enqueue_json(&detail_json);
        with_detail_responder(&transport);
        let client = test_client(&transport);

        let detail = client.get_playlist_detail(100, None).await.unwrap();

        assert_eq!(detail.id, 100);
        assert_eq!(detail.name, "测试歌单");
        assert_eq!(detail.cover_url.as_deref(), Some("https://p1/cover.jpg"));
        assert_eq!(detail.description.as_deref(), Some("描述"));
        assert_eq!(detail.creator_nickname.as_deref(), Some("创建者"));
        assert_eq!(detail.track_count, 3);
        assert_eq!(detail.track_ids, vec![11, 22, 33]);
        assert_eq!(song_ids(&detail), vec![11, 22, 33]);
        assert!(!detail.is_partial);
        // 内嵌曲目关联顶层 privileges 的 maxbr；批量补齐的曲目关联各自响应里的 privileges。
        assert_eq!(detail.songs[0].max_bitrate, 320_000);
        assert_eq!(detail.songs[1].max_bitrate, 999_000);
        assert_eq!(detail.songs[1].fee, 0); // 歌曲自己的 fee 字段优先。
        assert_eq!(detail.songs[2].max_bitrate, 320_000);

        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].url.ends_with("/weapi/v6/playlist/detail"));
        let payload: Value =
            serde_json::from_str(&decrypt_business_json(&requests[0], FIXED_SECRET)).unwrap();
        assert_eq!(payload["id"], 100);
        assert_eq!(payload["n"], 1000);
        assert_eq!(payload["s"], 8);
        assert!(requests[1].url.ends_with("/weapi/v3/song/detail"));
        assert_eq!(parse_song_ids(&requests[1]), vec![11, 33]);
    }

    #[tokio::test]
    async fn detail_batches_requests_at_500_and_preserves_order() {
        const TOTAL: usize = 1200;
        let transport = FakeTransport::new();
        transport.enqueue_json(&build_large_playlist(TOTAL));
        with_detail_responder(&transport);
        let client = test_client(&transport);

        let detail = client.get_playlist_detail(1, None).await.unwrap();

        assert_eq!(detail.songs.len(), TOTAL);
        assert_eq!(
            song_ids(&detail),
            (1..=TOTAL as i64).collect::<Vec<_>>(),
            "歌曲必须按 trackIds 顺序排列，不受每批响应顺序影响"
        );
        assert!(!detail.is_partial);

        let batches: Vec<usize> = transport
            .requests()
            .iter()
            .skip(1)
            .map(|request| parse_song_ids(request).len())
            .collect();
        assert_eq!(batches, vec![500, 500, 200]);
    }

    #[tokio::test]
    async fn detail_batches_contain_the_right_ids_when_some_tracks_are_embedded() {
        // 内嵌了 1..=3，剩下 4..=1003 分 500 + 500 + ... 批。
        let embedded: Vec<String> = (1..=3)
            .map(|id| format!(r#"{{"id":{id},"name":"内嵌{id}"}}"#))
            .collect();
        let track_ids: Vec<String> = (1..=1003).map(|id| format!(r#"{{"id":{id}}}"#)).collect();
        let transport = FakeTransport::new();
        transport.enqueue_json(&format!(
            r#"{{"code":200,"playlist":{{"id":1,"name":"x","tracks":[{}],"trackIds":[{}]}}}}"#,
            embedded.join(","),
            track_ids.join(",")
        ));
        with_detail_responder(&transport);
        let client = test_client(&transport);

        let detail = client.get_playlist_detail(1, None).await.unwrap();

        assert_eq!(detail.songs.len(), 1003);
        assert_eq!(detail.songs[0].name, "内嵌1");
        assert_eq!(detail.songs[3].name, "歌曲4");
        let requests = transport.requests();
        assert_eq!(parse_song_ids(&requests[1]).first(), Some(&4));
        assert_eq!(parse_song_ids(&requests[1]).len(), 500);
        assert_eq!(parse_song_ids(&requests[2]).len(), 500);
        assert_eq!(requests.len(), 3);
    }

    #[tokio::test]
    async fn detail_reports_progress_after_the_embedded_tracks_and_each_batch() {
        let transport = FakeTransport::new();
        transport.enqueue_json(&build_large_playlist(1200));
        with_detail_responder(&transport);
        let client = test_client(&transport);
        let reports = Mutex::new(Vec::new());

        let progress = |loaded: usize, total: usize| reports.lock().unwrap().push((loaded, total));
        client
            .get_playlist_detail(1, Some(&progress))
            .await
            .unwrap();

        assert_eq!(
            *reports.lock().unwrap(),
            vec![(0, 1200), (500, 1200), (1000, 1200), (1200, 1200)]
        );
    }

    #[tokio::test]
    async fn detail_does_not_report_progress_when_nothing_is_missing() {
        let transport = FakeTransport::new();
        transport.enqueue_json(
            r#"{"code":200,"playlist":{"id":1,"tracks":[{"id":1,"name":"a"}],"trackIds":[{"id":1}]}}"#,
        );
        let client = test_client(&transport);
        let reports = Mutex::new(Vec::new());

        let progress = |loaded: usize, total: usize| reports.lock().unwrap().push((loaded, total));
        client
            .get_playlist_detail(1, Some(&progress))
            .await
            .unwrap();

        assert!(reports.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn detail_is_partial_when_a_batch_keeps_failing_but_other_batches_survive() {
        let transport = FakeTransport::new();
        transport.enqueue_json(&build_large_playlist(700));
        // 第一批两次尝试都失败（500），第二批成功。
        transport
            .enqueue_json(r#"{"code":500,"message":"接口异常"}"#)
            .enqueue_json(r#"{"code":500,"message":"接口异常"}"#);
        with_detail_responder(&transport);
        let client = test_client(&transport);

        let detail = client.get_playlist_detail(1, None).await.unwrap();

        assert!(detail.is_partial);
        assert_eq!(detail.track_ids.len(), 700);
        assert_eq!(song_ids(&detail), (501..=700).collect::<Vec<_>>());
        assert_eq!(transport.requests().len(), 4);
    }

    #[tokio::test]
    async fn detail_retries_a_failed_batch_once_and_recovers() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json(&build_large_playlist(3))
            .enqueue_error(TransportError::Network("连接被重置".to_owned()));
        with_detail_responder(&transport);
        let client = test_client(&transport);

        let detail = client.get_playlist_detail(1, None).await.unwrap();

        assert!(!detail.is_partial);
        assert_eq!(song_ids(&detail), vec![1, 2, 3]);
        assert_eq!(transport.requests().len(), 3);
    }

    #[tokio::test]
    async fn detail_is_partial_when_missing_tracks_cannot_be_recovered() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json(
                r#"{"code":200,"playlist":{"id":7,"name":"部分歌单","trackCount":2,"tracks":[],"trackIds":[{"id":11},{"id":22}]}}"#,
            )
            .enqueue_json(r#"{"code":500,"message":"接口异常"}"#)
            .enqueue_json(r#"{"code":500,"message":"接口异常"}"#);
        let client = test_client(&transport);

        let detail = client.get_playlist_detail(7, None).await.unwrap();

        assert!(detail.is_partial);
        assert!(detail.songs.is_empty());
        assert_eq!(detail.track_ids, vec![11, 22]);
        assert_eq!(detail.track_count, 2);
    }

    #[tokio::test]
    async fn detail_is_partial_when_the_server_omits_some_songs() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json(&build_large_playlist(3))
            .enqueue_json(
                r#"{"code":200,"songs":[{"id":3,"name":"c"},{"id":1,"name":"a"}],"privileges":[]}"#,
            );
        let client = test_client(&transport);

        let detail = client.get_playlist_detail(1, None).await.unwrap();

        assert!(detail.is_partial);
        assert_eq!(song_ids(&detail), vec![1, 3]);
    }

    #[tokio::test]
    async fn detail_propagates_code_301_from_a_batch_without_retrying() {
        let transport = FakeTransport::new();
        transport
            .enqueue_json(&build_large_playlist(3))
            .enqueue_json(r#"{"code":301,"msg":"需要登录"}"#);
        let client = test_client(&transport);

        let error = client.get_playlist_detail(1, None).await.unwrap_err();

        assert!(error.is_not_logged_in());
        assert_eq!(transport.requests().len(), 2);
    }

    #[tokio::test]
    async fn detail_falls_back_to_embedded_ids_when_track_ids_are_missing() {
        let transport = FakeTransport::new();
        transport.enqueue_json(
            r#"{"code":200,"playlist":{"id":9,"name":"x","tracks":[{"id":5,"name":"a"},{"id":6,"name":"b"},{"id":5,"name":"dup"}]}}"#,
        );
        let client = test_client(&transport);

        let detail = client.get_playlist_detail(9, None).await.unwrap();

        assert_eq!(song_ids(&detail), vec![5, 6]);
        assert_eq!(detail.songs[0].name, "dup"); // 与 C# 一致：同 id 后者覆盖前者。
        assert_eq!(detail.track_ids, vec![5, 6]);
        assert!(!detail.is_partial);
        assert_eq!(transport.requests().len(), 1);
    }

    #[tokio::test]
    async fn detail_dedupes_track_ids_and_accepts_plain_numbers() {
        let transport = FakeTransport::new();
        transport.enqueue_json(
            r#"{"code":200,"playlist":{"id":9,"name":"x","tracks":[],"trackIds":[3,"4",{"id":3},{"id":0},-1,{"id":5}]}}"#,
        );
        with_detail_responder(&transport);
        let client = test_client(&transport);

        let detail = client.get_playlist_detail(9, None).await.unwrap();

        assert_eq!(detail.track_ids, vec![3, 4, 5]);
        assert_eq!(song_ids(&detail), vec![3, 4, 5]);
    }

    #[tokio::test]
    async fn detail_fails_with_404_when_the_playlist_payload_is_missing() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":200,"msg":"not found"}"#);
        let client = test_client(&transport);

        let error = client.get_playlist_detail(999, None).await.unwrap_err();

        assert_eq!(error.code(), Some(404));
        assert_eq!(error.kind(), NeteaseErrorKind::Api);
        assert!(error.message().contains("999"));
    }

    #[tokio::test]
    async fn detail_fails_when_the_business_code_is_not_200() {
        let transport = FakeTransport::new();
        transport.enqueue_json(r#"{"code":404,"msg":"歌单不存在"}"#);
        let client = test_client(&transport);

        let error = client.get_playlist_detail(999, None).await.unwrap_err();

        assert_eq!(error.code(), Some(404));
        assert!(!error.is_not_logged_in());
    }

    #[tokio::test]
    async fn detail_maps_http_and_network_failures_to_readable_errors() {
        let transport = FakeTransport::new();
        transport
            .enqueue_status(502)
            .enqueue_error(TransportError::Network("dns".to_owned()))
            .enqueue_error(TransportError::Timeout);
        let client = test_client(&transport);

        let http = client.get_playlist_detail(1, None).await.unwrap_err();
        assert!(http.is_http_status());
        assert_eq!(http.code(), Some(502));

        let network = client.get_playlist_detail(1, None).await.unwrap_err();
        assert!(network.is_transient_network());
        assert_eq!(network.user_message(), "网络连接失败，请检查网络后重试");

        let timeout = client.get_playlist_detail(1, None).await.unwrap_err();
        assert_eq!(timeout.user_message(), "请求超时，请稍后重试");
    }

    #[tokio::test]
    async fn detail_rejects_non_positive_playlist_ids_without_a_request() {
        let transport = FakeTransport::new();
        let client = test_client(&transport);

        assert!(client.get_playlist_detail(0, None).await.is_err());
        assert!(transport.requests().is_empty());
    }

    #[tokio::test]
    async fn detail_track_count_defaults_to_the_track_id_count() {
        let transport = FakeTransport::new();
        transport.enqueue_json(
            r#"{"code":200,"playlist":{"id":9,"tracks":[],"trackIds":[{"id":3},{"id":4}]}}"#,
        );
        with_detail_responder(&transport);
        let client = test_client(&transport);

        let detail = client.get_playlist_detail(9, None).await.unwrap();

        assert_eq!(detail.track_count, 2);
        assert_eq!(detail.name, "");
    }

    #[tokio::test]
    async fn detail_serializes_to_camel_case_without_the_track_id_list() {
        let transport = FakeTransport::new();
        transport.enqueue_json(
            r#"{"code":200,"playlist":{"id":9,"name":"x","tracks":[{"id":3,"name":"a","dt":1000}],"trackIds":[{"id":3}]}}"#,
        );
        let client = test_client(&transport);

        let detail = client.get_playlist_detail(9, None).await.unwrap();
        let value = serde_json::to_value(&detail).unwrap();

        assert_eq!(value["trackCount"], 1);
        assert_eq!(value["isPartial"], false);
        assert_eq!(value["songs"][0]["durationMs"], 1000);
        assert_eq!(value["songs"][0]["album"]["coverUrl"], Value::Null);
        assert!(value.get("trackIds").is_none());
    }
}
