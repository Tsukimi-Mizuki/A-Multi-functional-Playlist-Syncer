//! `OnlineMusicSearchService`：网易云搜索 / 详情（经 `NeteaseClient`）与到 QQ 的分发。

use std::sync::Arc;

use crate::lyrics::{RawLyrics, parser};
use crate::netease::http::HttpResponse;
use crate::online::model::{OnlineError, OnlineMusicSource, OnlineSongResult};
use crate::online::service::{OnlineMusicSearch, OnlineMusicSearchService};
use crate::online::test_support::status;
use crate::test_util::{FakeTransport, test_client};
use crate::utilities::CancelToken;

const SEARCH_JSON: &str = r#"{
  "code": 200,
  "result": { "songs": [
    { "id": 186016, "name": "晴天", "duration": 269000,
      "artists": [ { "id": 6452, "name": "周杰伦" } ],
      "album": { "id": 18877, "name": "叶惠美", "publishTime": 1058284800000 } }
  ] }
}"#;

const DETAIL_JSON: &str = r#"{
  "code": 200,
  "songs": [{
    "id": 186016, "name": "晴天（详情）",
    "ar": [ { "id": 6452, "name": "周杰伦" } ],
    "al": { "id": 13628, "name": "叶惠美", "picUrl": "https://p1.music.126.net/cover.jpg", "publishTime": 1058284800000 },
    "dt": 269000, "no": 3, "cd": "2", "publishTime": 1058284800000
  }],
  "privileges": []
}"#;

const LYRIC_JSON: &str = r#"{
  "code": 200,
  "lrc": { "lyric": "[00:01.000]第一句\n[00:05.000]第二句\n" },
  "tlyric": { "lyric": "[00:01.000]Line one\n[00:05.000]Line two\n" },
  "yrc": { "lyric": "[1000,2000](1000,1000,0)第(2000,1000,0)一句\n" }
}"#;

struct Rig {
    netease: Arc<FakeTransport>,
    other: Arc<FakeTransport>,
    service: OnlineMusicSearchService,
}

fn rig() -> Rig {
    let netease = FakeTransport::new();
    let other = FakeTransport::new();
    let service = OnlineMusicSearchService::new(Arc::new(test_client(&netease)), other.clone());
    Rig {
        netease,
        other,
        service,
    }
}

fn summary() -> OnlineSongResult {
    OnlineSongResult {
        source: OnlineMusicSource::Netease,
        source_id: "186016".to_owned(),
        title: "晴天".to_owned(),
        artist: "周杰伦".to_owned(),
        album: "叶惠美".to_owned(),
        duration_ms: 269_000,
        year: Some(2003),
        track_number: None,
        disc_number: None,
        genre: Some("流行".to_owned()),
        composer: None,
        cover_url: None,
        numeric_source_id: None,
    }
}

#[tokio::test]
async fn netease_search_maps_results_and_ignores_blank_keywords() {
    let rig = rig();
    rig.netease.enqueue_json(SEARCH_JSON);

    let songs = rig
        .service
        .search(
            OnlineMusicSource::Netease,
            "  晴天  ",
            10,
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].source, OnlineMusicSource::Netease);
    assert_eq!(songs[0].source_id, "186016");
    assert_eq!(songs[0].title, "晴天");
    assert_eq!(songs[0].artist, "周杰伦");
    assert_eq!(songs[0].album, "叶惠美");
    assert_eq!(songs[0].duration_ms, 269_000);
    assert_eq!(songs[0].year, Some(2003));
    assert_eq!(songs[0].track_number, None);
    assert!(
        rig.netease.requests()[0]
            .url
            .contains("s=%E6%99%B4%E5%A4%A9&type=1&limit=10")
    );

    let blank = rig
        .service
        .search(OnlineMusicSource::Netease, "   ", 10, &CancelToken::new())
        .await
        .unwrap();
    assert!(blank.is_empty());
    assert_eq!(rig.netease.requests().len(), 1, "空白关键词不发请求");
}

#[tokio::test]
async fn netease_rate_limits_get_a_specific_message() {
    let rig = rig();
    rig.netease
        .enqueue_json(r#"{"code":406,"message":"too fast"}"#);

    let error = rig
        .service
        .search(OnlineMusicSource::Netease, "晴天", 10, &CancelToken::new())
        .await
        .unwrap_err();

    assert!(
        matches!(&error, OnlineError::Failed(message) if message.contains("code=406") && message.contains("QQ 音乐")),
        "{error}"
    );

    let rig = self::rig();
    rig.netease.enqueue_status(500);
    let error = rig
        .service
        .search(OnlineMusicSource::Netease, "晴天", 10, &CancelToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, OnlineError::Failed(message) if message.contains("HTTP 500")));
}

#[tokio::test]
async fn netease_details_merge_the_detail_over_the_summary_and_parse_lyrics() {
    let rig = rig();
    rig.netease
        .enqueue_json(DETAIL_JSON)
        .enqueue_json(LYRIC_JSON);
    rig.other.set_fallback(|request| {
        assert!(request.url.contains("cover.jpg"));
        HttpResponse {
            status: 200,
            set_cookies: vec![],
            body: vec![1, 2, 3, 4],
        }
    });

    let details = rig
        .service
        .get_details(&summary(), &CancelToken::new())
        .await
        .unwrap()
        .unwrap();

    let song = &details.song;
    assert_eq!(song.title, "晴天（详情）");
    assert_eq!(song.artist, "周杰伦");
    assert_eq!(song.duration_ms, 269_000);
    assert_eq!(song.year, Some(2003));
    assert_eq!(song.track_number, Some(3));
    assert_eq!(song.disc_number, Some(2));
    assert_eq!(song.genre.as_deref(), Some("流行"), "摘要里的流行保留");
    assert_eq!(
        song.cover_url.as_deref(),
        Some("https://p1.music.126.net/cover.jpg")
    );
    assert_eq!(details.cover_data.as_deref(), Some(&[1u8, 2, 3, 4][..]));

    let parsed = parser::parse(&RawLyrics {
        lrc: Some("[00:01.000]第一句\n[00:05.000]第二句\n".to_owned()),
        translated_lrc: Some("[00:01.000]Line one\n[00:05.000]Line two\n".to_owned()),
        yrc: Some("[1000,2000](1000,1000,0)第(2000,1000,0)一句\n".to_owned()),
        ..RawLyrics::default()
    });
    assert!(parsed.has_word_level());
    assert!(details.has_word_lyrics());
    assert!(details.has_translation());
    assert_eq!(
        details.line_lyrics.as_deref(),
        Some(parser::to_plain_lrc(&parsed, false).as_str())
    );
    assert_eq!(
        details.word_lyrics.as_deref(),
        Some(parser::to_enhanced_lrc(&parsed, false).as_str())
    );
    assert_eq!(
        details.bilingual_word_lyrics.as_deref(),
        Some(parser::to_enhanced_lrc(&parsed, true).as_str())
    );
    // 偏好逐字 + 双语 → 双语逐字；偏好逐行 → 双语逐行。
    assert_eq!(
        details.get_lyrics(true, true),
        details.bilingual_word_lyrics.as_deref()
    );
    assert_eq!(
        details.get_lyrics(false, true),
        details.bilingual_line_lyrics.as_deref()
    );
    assert_eq!(
        details.get_lyrics(true, false),
        details.word_lyrics.as_deref()
    );
}

#[tokio::test]
async fn netease_details_fall_back_to_the_summary_when_the_detail_call_fails() {
    let rig = rig();
    rig.netease
        .enqueue_status(500)
        .enqueue_json(r#"{"code":200,"lrc":{"lyric":"[00:01.000]只有逐行\n"}}"#);

    let details = rig
        .service
        .get_details(&summary(), &CancelToken::new())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(details.song, summary());
    assert_eq!(details.cover_data, None);
    assert_eq!(details.line_lyrics.as_deref(), Some("[00:01.000]只有逐行"));
    assert!(!details.has_word_lyrics());
    assert!(!details.has_translation());
    assert_eq!(details.bilingual_line_lyrics, None);
}

#[tokio::test]
async fn netease_details_keep_the_tags_when_lyrics_are_unavailable() {
    let rig = rig();
    rig.netease
        .enqueue_json(DETAIL_JSON)
        .enqueue_status(500)
        .enqueue_status(500)
        .enqueue_status(500);
    rig.other.set_fallback(|_| status(404));

    let details = rig
        .service
        .get_details(&summary(), &CancelToken::new())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(details.song.title, "晴天（详情）");
    assert_eq!(details.line_lyrics, None);
    assert_eq!(details.get_lyrics(true, true), None);
}

#[tokio::test]
async fn netease_details_reject_invalid_ids_without_any_request() {
    let rig = rig();
    for id in ["", "abc", "0", "-5"] {
        let mut song = summary();
        song.source_id = id.to_owned();
        let details = rig
            .service
            .get_details(&song, &CancelToken::new())
            .await
            .unwrap();
        assert!(details.is_none(), "{id:?}");
    }
    assert!(rig.netease.requests().is_empty());
}

#[tokio::test]
async fn qq_songs_are_dispatched_to_the_qq_client() {
    let rig = rig();
    rig.other
        .enqueue_json(r#"{"code":0,"data":{"song":{"list":[{"songname":"Lemon","songmid":"m"}]}}}"#)
        .enqueue_json(r#"{"retcode":0,"lyric":"[00:01.00]x"}"#);

    let songs = rig
        .service
        .search(OnlineMusicSource::QqMusic, "lemon", 5, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(songs[0].source_id, "m");
    assert!(rig.netease.requests().is_empty());

    let details = rig
        .service
        .get_details(&songs[0], &CancelToken::new())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(details.line_lyrics.as_deref(), Some("[00:01.00]x"));
    assert!(rig.netease.requests().is_empty());
}

#[tokio::test]
async fn cancellation_stops_netease_requests() {
    let rig = rig();
    let cancel = CancelToken::new();
    cancel.cancel();

    let error = rig
        .service
        .search(OnlineMusicSource::Netease, "晴天", 10, &cancel)
        .await
        .unwrap_err();

    assert_eq!(error, OnlineError::Cancelled);
    assert!(rig.netease.requests().is_empty());
}
