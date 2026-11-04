//! LRCLIB 词源：`?q=` 多变体 → `track_name` 兜底、429 / 503 退避重试、候选筛选。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::netease::http::{HttpResponse, TransportError};
use crate::online::lrclib::LrclibLyricsSource;
use crate::online::model::{LyricsQuery, LyricsSource, LyricsSourceCandidate, OnlineError};
use crate::online::test_support::{json_ok, status};
use crate::test_util::FakeTransport;
use crate::utilities::CancelToken;

const GOOD: &str = r#"[
  { "trackName": "晴天", "artistName": "周杰伦", "albumName": "叶惠美", "duration": 269.4,
    "instrumental": false, "syncedLyrics": "[00:01.00]第一句\n[00:05.00]第二句\n" }
]"#;

fn source(transport: &Arc<FakeTransport>) -> LrclibLyricsSource {
    LrclibLyricsSource::with_timing(transport.clone(), Duration::from_secs(10), Duration::ZERO)
}

fn queries() -> Vec<String> {
    vec![
        "晴天 周杰伦".to_owned(),
        "晴天".to_owned(),
        "周杰伦 晴天".to_owned(),
        "第四组".to_owned(),
    ]
}

async fn fetch(
    source: &LrclibLyricsSource,
    queries: &[String],
    title: &str,
    artist: &str,
    duration_ms: i64,
) -> Result<Option<LyricsSourceCandidate>, OnlineError> {
    let query = LyricsQuery {
        queries,
        title,
        artist,
        album: Some("叶惠美"),
        duration_ms,
        min_score: 0.6,
    };
    source.fetch(&query, &CancelToken::new()).await
}

#[tokio::test]
async fn fetches_synced_lyrics_from_the_first_query() {
    let transport = FakeTransport::new();
    transport.enqueue_json(GOOD);

    let candidate = fetch(&source(&transport), &queries(), "晴天", "周杰伦", 269_000)
        .await
        .unwrap()
        .expect("应命中");

    assert_eq!(candidate.source_label, "LRCLIB");
    assert_eq!(candidate.title, "晴天");
    assert_eq!(candidate.artist, "周杰伦");
    assert_eq!(candidate.album, "叶惠美");
    assert_eq!(candidate.duration_ms, 269_400);
    assert_eq!(
        candidate.line_lyrics.as_deref(),
        Some("[00:01.000]第一句\n[00:05.000]第二句")
    );
    assert!(!candidate.has_word_lyrics());
    assert!(!candidate.has_translation());

    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].url,
        "https://lrclib.net/api/search?q=%E6%99%B4%E5%A4%A9%20%E5%91%A8%E6%9D%B0%E4%BC%A6"
    );
    assert!(
        requests[0]
            .header("User-Agent")
            .unwrap()
            .starts_with("LunaticPrincess")
    );
}

#[tokio::test]
async fn tries_at_most_three_query_variants_then_the_track_name_search() {
    let transport = FakeTransport::new();
    transport
        .enqueue_json("[]")
        .enqueue_json("[]")
        .enqueue_json("[]")
        .enqueue_json(GOOD);

    let candidate = fetch(&source(&transport), &queries(), "晴天", "周杰伦", 269_000)
        .await
        .unwrap();

    assert!(candidate.is_some());
    let urls: Vec<String> = transport.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(urls.len(), 4, "第 4 组关键词不会被使用：{urls:?}");
    assert!(urls[..3].iter().all(|url| url.contains("?q=")));
    assert_eq!(
        urls[3],
        "https://lrclib.net/api/search?track_name=%E6%99%B4%E5%A4%A9&artist_name=%E5%91%A8%E6%9D%B0%E4%BC%A6"
    );
}

#[tokio::test]
async fn the_track_name_search_omits_a_blank_artist() {
    let transport = FakeTransport::new();
    transport.enqueue_json("[]").enqueue_json("[]");
    let one = vec!["晴天".to_owned()];

    let found = fetch(&source(&transport), &one, " 晴天 ", "  ", 269_000)
        .await
        .unwrap();

    assert!(found.is_none());
    assert_eq!(
        transport.requests()[1].url,
        "https://lrclib.net/api/search?track_name=%E6%99%B4%E5%A4%A9"
    );
}

#[tokio::test]
async fn a_blank_title_is_not_searched() {
    let transport = FakeTransport::new();

    let found = fetch(&source(&transport), &queries(), "  ", "周杰伦", 269_000)
        .await
        .unwrap();

    assert!(found.is_none());
    assert!(transport.requests().is_empty());
}

#[tokio::test]
async fn skips_instrumentals_unsynced_entries_and_wrong_durations() {
    let transport = FakeTransport::new();
    transport.enqueue_json(
        r#"[
          { "trackName": "晴天", "artistName": "周杰伦", "duration": 269, "instrumental": true,
            "syncedLyrics": "[00:01.00]x" },
          { "trackName": "晴天", "artistName": "周杰伦", "duration": 269, "plainLyrics": "只有纯文本" },
          { "trackName": "晴天", "artistName": "周杰伦", "duration": 300, "syncedLyrics": "[00:01.00]时长不符" },
          { "trackName": "晴天", "artistName": "周杰伦", "duration": 269, "syncedLyrics": "没有时间轴" },
          "不是对象",
          { "trackName": "晴天", "artistName": "周杰伦", "duration": "269.5", "syncedLyrics": "[00:01.00]合格" }
        ]"#,
    );

    let candidate = fetch(&source(&transport), &queries(), "晴天", "周杰伦", 269_000)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(candidate.line_lyrics.as_deref(), Some("[00:01.000]合格"));
    // 269.5 秒按银行家舍入：269500 毫秒。
    assert_eq!(candidate.duration_ms, 269_500);
}

#[tokio::test]
async fn picks_the_highest_scoring_candidate_and_keeps_the_first_on_ties() {
    let transport = FakeTransport::new();
    transport.enqueue_json(
        r#"[
          { "trackName": "晴天 (Live)", "artistName": "周杰伦", "duration": 269, "syncedLyrics": "[00:01.00]现场版" },
          { "trackName": "晴天", "artistName": "周杰伦", "duration": 269, "syncedLyrics": "[00:01.00]第一份" },
          { "trackName": "晴天", "artistName": "周杰伦", "duration": 269, "syncedLyrics": "[00:01.00]第二份" }
        ]"#,
    );

    let candidate = fetch(&source(&transport), &queries(), "晴天", "周杰伦", 269_000)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(candidate.line_lyrics.as_deref(), Some("[00:01.000]第一份"));
}

#[tokio::test]
async fn rejects_candidates_for_other_artists() {
    let transport = FakeTransport::new();
    transport.set_fallback(|_| {
        json_ok(
            r#"[{ "trackName": "晴天", "artistName": "陌生人", "duration": 269,
                  "syncedLyrics": "[00:01.00]别人的歌" }]"#,
        )
    });

    let found = fetch(&source(&transport), &queries(), "晴天", "周杰伦", 269_000)
        .await
        .unwrap();

    assert!(found.is_none());
}

#[tokio::test]
async fn missing_names_default_to_the_local_song() {
    let transport = FakeTransport::new();
    transport.enqueue_json(r#"[{ "duration": 269, "syncedLyrics": "[00:01.00]x" }]"#);

    let candidate = fetch(&source(&transport), &queries(), "晴天", "周杰伦", 269_000)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(candidate.title, "晴天");
    assert_eq!(candidate.artist, "周杰伦");
    assert_eq!(candidate.album, "");
}

#[tokio::test]
async fn retries_once_after_429_or_503() {
    for transient in [429, 503] {
        let transport = FakeTransport::new();
        transport.enqueue_status(transient).enqueue_json(GOOD);

        let candidate = fetch(&source(&transport), &queries(), "晴天", "周杰伦", 269_000)
            .await
            .unwrap();

        assert!(candidate.is_some(), "{transient}");
        let urls: Vec<String> = transport.requests().into_iter().map(|r| r.url).collect();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], urls[1], "重试的是同一个地址");
    }
}

#[tokio::test]
async fn gives_up_after_the_single_retry_and_never_retries_other_failures() {
    // 429 → 重试后仍 429 → 放弃，转下一个关键词。
    let transport = FakeTransport::new();
    transport.set_fallback(|_| status(429));
    let one = vec!["晴天".to_owned()];
    fetch(&source(&transport), &one, "晴天", "周杰伦", 269_000)
        .await
        .unwrap();
    // q 查询 2 次 + track_name 查询 2 次。
    assert_eq!(transport.requests().len(), 4);

    for permanent in [404, 500] {
        let transport = FakeTransport::new();
        transport.set_fallback(move |_| status(permanent));
        fetch(&source(&transport), &one, "晴天", "周杰伦", 269_000)
            .await
            .unwrap();
        assert_eq!(transport.requests().len(), 2, "{permanent} 不重试");
    }
}

#[tokio::test]
async fn transport_errors_and_bad_bodies_degrade_to_no_result() {
    let transport = FakeTransport::new();
    transport
        .enqueue_error(TransportError::Network("connection reset".to_owned()))
        .enqueue_raw(200, "not json")
        .enqueue_raw(200, "   ")
        .enqueue_json(r#"{"error":"not an array"}"#);
    let one = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];

    let found = fetch(&source(&transport), &one, "晴天", "周杰伦", 269_000)
        .await
        .unwrap();

    assert!(found.is_none());
    assert_eq!(transport.requests().len(), 4);
}

#[tokio::test]
async fn an_unusable_document_is_skipped_for_the_next_candidate_and_query() {
    let transport = FakeTransport::new();
    transport.enqueue_json(
        r#"[{ "trackName": "晴天", "artistName": "周杰伦", "duration": 269, "syncedLyrics": "没有时间轴" }]"#,
    );
    transport.enqueue_json(GOOD);

    let candidate = fetch(&source(&transport), &queries(), "晴天", "周杰伦", 269_000)
        .await
        .unwrap();

    assert!(candidate.is_some());
    assert_eq!(transport.requests().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn the_retry_delay_counts_against_the_overall_timeout() {
    let transport = FakeTransport::new();
    transport.set_fallback(|_| status(429));
    let slow = LrclibLyricsSource::with_timing(
        transport.clone(),
        Duration::from_secs(2),
        Duration::from_secs(5),
    );
    let one = vec!["晴天".to_owned()];

    let found = fetch(&slow, &one, "晴天", "周杰伦", 269_000).await.unwrap();

    assert!(found.is_none());
    // 每个地址只发出第一次请求，退避等待超过 2 秒的上限，就地放弃。
    assert_eq!(transport.requests().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_hung_request_times_out_into_no_result() {
    let transport = FakeTransport::new();
    transport.enqueue_gated("[]", Arc::new(tokio::sync::Notify::new()));
    transport.set_fallback(|_| HttpResponse {
        status: 500,
        set_cookies: vec![],
        body: vec![],
    });
    let source =
        LrclibLyricsSource::with_timing(transport.clone(), Duration::from_secs(2), Duration::ZERO);

    let found = fetch(&source, &queries(), "晴天", "周杰伦", 269_000)
        .await
        .unwrap();

    assert!(found.is_none());
}

#[tokio::test]
async fn cancellation_is_the_only_error() {
    let transport = FakeTransport::new();
    transport.enqueue_json(GOOD);
    let cancel = CancelToken::new();
    cancel.cancel();
    let list = queries();
    let query = LyricsQuery {
        queries: &list,
        title: "晴天",
        artist: "周杰伦",
        album: None,
        duration_ms: 269_000,
        min_score: 0.6,
    };

    let error = source(&transport).fetch(&query, &cancel).await.unwrap_err();

    assert_eq!(error, OnlineError::Cancelled);
    assert!(transport.requests().is_empty());
}

#[tokio::test]
async fn blank_queries_are_not_sent_and_the_count_applies_to_non_blank_ones() {
    let transport = FakeTransport::new();
    let count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&count);
    transport.set_fallback(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        json_ok("[]")
    });
    let list = vec![
        " ".to_owned(),
        "a".to_owned(),
        String::new(),
        "b".to_owned(),
        "c".to_owned(),
        "d".to_owned(),
    ];

    fetch(&source(&transport), &list, "晴天", "周杰伦", 269_000)
        .await
        .unwrap();

    // 3 个非空白关键词 + 1 次 track_name。
    assert_eq!(count.load(Ordering::SeqCst), 4);
}

#[test]
fn the_label_matches_the_settings_switch() {
    assert_eq!(LrclibLyricsSource::LABEL, "LRCLIB");
}
