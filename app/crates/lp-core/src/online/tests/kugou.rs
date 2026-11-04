//! 酷狗词源：三段链路、候选打分、KRC / LRC 回落、任何网络故障都回落成“没有结果”。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::lyrics::{krc, parser};
use crate::netease::http::{HttpRequest, HttpResponse, TransportError};
use crate::online::kugou::KugouLyricsSource;
use crate::online::model::{LyricsQuery, LyricsSource, OnlineError};
use crate::online::test_support::{base64_of, json_ok, krc_base64, status};
use crate::test_util::FakeTransport;
use crate::utilities::CancelToken;

const KRC_TEXT: &str = "[ti:x]\n[1000,2000]<0,500,0>Hello <500,500,0>world<1000,1000,0>!\n[4000,1500]<0,750,0>第二<750,750,0>行";

fn queries() -> Vec<String> {
    vec![
        "晴天 周杰伦".to_owned(),
        "晴天".to_owned(),
        "周杰伦 晴天".to_owned(),
    ]
}

async fn fetch_with(
    source: &KugouLyricsSource,
    queries: &[String],
    duration_ms: i64,
    min_score: f64,
) -> Result<Option<crate::online::LyricsSourceCandidate>, OnlineError> {
    let query = LyricsQuery {
        queries,
        title: "晴天",
        artist: "周杰伦",
        album: None,
        duration_ms,
        min_score,
    };
    source.fetch(&query, &CancelToken::new()).await
}

fn candidates_json(items: &[(&str, &str, &str, &str, i64)]) -> String {
    let rows: Vec<String> = items
        .iter()
        .map(|(id, key, song, singer, duration)| {
            format!(
                r#"{{"id":"{id}","accesskey":"{key}","song":"{song}","singer":"{singer}","duration":{duration}}}"#
            )
        })
        .collect();
    format!(r#"{{"candidates":[{}]}}"#, rows.join(","))
}

/// 按 URL 路由的假酷狗服务。
struct Router {
    hash: Box<dyn Fn() -> HttpResponse + Send + Sync>,
    search: Box<dyn Fn(&HttpRequest) -> HttpResponse + Send + Sync>,
    krc: Box<dyn Fn() -> HttpResponse + Send + Sync>,
    lrc: Box<dyn Fn() -> HttpResponse + Send + Sync>,
}

fn install(transport: &Arc<FakeTransport>, router: Router) {
    transport.set_fallback(move |request| {
        let url = &request.url;
        if url.contains("mobilecdn.kugou.com") {
            (router.hash)()
        } else if url.contains("krcs.kugou.com/search") {
            (router.search)(request)
        } else if url.contains("fmt=krc") {
            (router.krc)()
        } else if url.contains("fmt=lrc") {
            (router.lrc)()
        } else {
            panic!("未预期的请求：{url}");
        }
    });
}

fn happy_router() -> Router {
    Router {
        hash: Box::new(|| json_ok(r#"{"data":{"info":[{"hash":""},{"hash":"HASH1"}]}}"#)),
        search: Box::new(|_| json_ok(&candidates_json(&[("77", "KEY", "晴天", "周杰伦", 269)]))),
        krc: Box::new(|| {
            json_ok(&format!(
                r#"{{"status":200,"content":"{}"}}"#,
                krc_base64(KRC_TEXT)
            ))
        }),
        lrc: Box::new(|| status(404)),
    }
}

#[tokio::test]
async fn fetches_krc_word_lyrics_with_translation_through_the_three_step_chain() {
    let transport = FakeTransport::new();
    let with_translation = format!(
        "[language:{}]\n{KRC_TEXT}",
        base64_of(r#"{"content":[{"type":1,"lyricContent":[["你好 世界！"],["第二行译"]]}]}"#)
    );
    install(
        &transport,
        Router {
            krc: Box::new(move || {
                json_ok(&format!(
                    r#"{{"status":200,"content":"{}"}}"#,
                    krc_base64(&with_translation)
                ))
            }),
            ..happy_router()
        },
    );
    let source = KugouLyricsSource::new(transport.clone());

    let candidate = fetch_with(&source, &queries(), 269_000, 0.6)
        .await
        .unwrap()
        .expect("应命中");

    assert_eq!(candidate.source_label, "酷狗音乐");
    assert_eq!(candidate.title, "晴天");
    assert_eq!(candidate.artist, "周杰伦");
    assert_eq!(candidate.album, "");
    assert_eq!(candidate.duration_ms, 269_000);
    assert!(candidate.has_word_lyrics());
    assert!(candidate.has_translation());
    assert!(candidate.word_lyrics.as_deref().unwrap().contains('<'));
    assert!(
        candidate
            .bilingual_line_lyrics
            .as_deref()
            .unwrap()
            .contains("你好 世界！")
    );
    let document = krc::parse(Some(&format!(
        "[language:{}]\n{KRC_TEXT}",
        base64_of(r#"{"content":[{"type":1,"lyricContent":[["你好 世界！"],["第二行译"]]}]}"#)
    )));
    assert_eq!(
        candidate.line_lyrics.as_deref(),
        Some(parser::to_plain_lrc(&document, false).as_str())
    );
    assert_eq!(
        candidate.bilingual_word_lyrics.as_deref(),
        Some(parser::to_enhanced_lrc(&document, true).as_str())
    );

    // 三段链路：hash 搜索 → 歌词候选（带 hash 与时长）→ KRC 下载（带 id 与 accesskey）。
    let urls: Vec<String> = transport.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(urls.len(), 3);
    assert_eq!(
        urls[0],
        "https://mobilecdn.kugou.com/api/v3/search/song?format=json&page=1&pagesize=3&showtype=1&keyword=%E6%99%B4%E5%A4%A9%20%E5%91%A8%E6%9D%B0%E4%BC%A6"
    );
    assert_eq!(
        urls[1],
        "https://krcs.kugou.com/search?ver=1&man=yes&client=mobi&keyword=%E6%99%B4%E5%A4%A9%20%E5%91%A8%E6%9D%B0%E4%BC%A6&duration=269&hash=HASH1"
    );
    assert_eq!(
        urls[2],
        "https://lyrics.kugou.com/download?ver=1&client=pc&fmt=krc&charset=utf8&id=77&accesskey=KEY"
    );
}

#[tokio::test]
async fn a_krc_without_translation_has_no_bilingual_variants() {
    let transport = FakeTransport::new();
    install(&transport, happy_router());
    let source = KugouLyricsSource::new(transport.clone());

    let candidate = fetch_with(&source, &queries(), 269_000, 0.6)
        .await
        .unwrap()
        .unwrap();

    assert!(candidate.has_word_lyrics());
    assert!(!candidate.has_translation());
    assert_eq!(candidate.bilingual_line_lyrics, None);
    assert_eq!(candidate.bilingual_word_lyrics, None);
}

#[tokio::test]
async fn searches_without_a_hash_when_the_hash_lookup_finds_nothing() {
    let transport = FakeTransport::new();
    install(
        &transport,
        Router {
            hash: Box::new(|| json_ok(r#"{"data":{"info":[]}}"#)),
            search: Box::new(|request| {
                assert!(!request.url.contains("&hash="), "{}", request.url);
                json_ok(&candidates_json(&[("77", "KEY", "晴天", "周杰伦", 269)]))
            }),
            ..happy_router()
        },
    );
    let source = KugouLyricsSource::new(transport.clone());

    assert!(
        fetch_with(&source, &queries(), 269_000, 0.6)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn rounds_the_search_duration_to_even_seconds_and_never_below_one() {
    for (duration_ms, expected) in [(269_500, 270), (270_500, 270), (0, 1), (400, 1)] {
        let transport = FakeTransport::new();
        let seen = Arc::new(std::sync::Mutex::new(String::new()));
        let sink = Arc::clone(&seen);
        install(
            &transport,
            Router {
                hash: Box::new(|| status(500)),
                search: Box::new(move |request| {
                    *sink.lock().unwrap() = request.url.clone();
                    json_ok(r#"{"candidates":[]}"#)
                }),
                ..happy_router()
            },
        );
        let source = KugouLyricsSource::new(transport);
        let one = vec!["晴天".to_owned()];
        fetch_with(&source, &one, duration_ms, 0.6).await.unwrap();
        assert!(
            seen.lock()
                .unwrap()
                .ends_with(&format!("&duration={expected}")),
            "{duration_ms} → {}",
            seen.lock().unwrap()
        );
    }
}

#[tokio::test]
async fn candidates_with_the_wrong_duration_or_artist_are_filtered() {
    let transport = FakeTransport::new();
    install(
        &transport,
        Router {
            search: Box::new(|_| {
                json_ok(&candidates_json(&[
                    // 时长相差 20 秒：硬性过滤。
                    ("1", "K1", "晴天", "周杰伦", 289),
                    // 别人的同名歌：总分低于阈值。
                    ("2", "K2", "晴天", "陌生人", 269),
                    // 伴奏：直接排除。
                    ("3", "K3", "晴天 (伴奏)", "周杰伦", 269),
                    // 缺 accesskey / 缺歌名：跳过。
                    ("4", "", "晴天", "周杰伦", 269),
                    ("5", "K5", "", "周杰伦", 269),
                    // 合格。
                    ("6", "K6", "晴天", "周杰伦", 270),
                ]))
            }),
            ..happy_router()
        },
    );
    let source = KugouLyricsSource::new(transport.clone());

    let candidate = fetch_with(&source, &queries(), 269_000, 0.6)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(candidate.duration_ms, 270_000);
    assert!(
        transport
            .requests()
            .iter()
            .any(|r| r.url.contains("id=6&accesskey=K6"))
    );
}

#[tokio::test]
async fn only_the_first_eight_candidates_are_inspected() {
    let transport = FakeTransport::new();
    let mut rows: Vec<(String, String, String, String, i64)> = (1..=8)
        .map(|n| {
            (
                n.to_string(),
                format!("K{n}"),
                "完全无关".to_owned(),
                "路人".to_owned(),
                269,
            )
        })
        .collect();
    rows.push(("9".into(), "K9".into(), "晴天".into(), "周杰伦".into(), 269));
    let json = {
        let refs: Vec<(&str, &str, &str, &str, i64)> = rows
            .iter()
            .map(|(a, b, c, d, e)| (a.as_str(), b.as_str(), c.as_str(), d.as_str(), *e))
            .collect();
        candidates_json(&refs)
    };
    install(
        &transport,
        Router {
            search: Box::new(move |_| json_ok(&json)),
            ..happy_router()
        },
    );
    let source = KugouLyricsSource::new(transport);

    let found = fetch_with(&source, &queries(), 269_000, 0.6).await.unwrap();

    assert!(found.is_none(), "第 9 个候选不检查");
}

#[tokio::test]
async fn falls_back_to_lrc_when_the_krc_cannot_be_decoded() {
    let transport = FakeTransport::new();
    install(
        &transport,
        Router {
            krc: Box::new(|| json_ok(r#"{"status":200,"content":"%%%not-base64%%%"}"#)),
            lrc: Box::new(|| {
                json_ok(&format!(
                    r#"{{"status":200,"content":"{}"}}"#,
                    base64_of("[00:01.00]第一句\n[00:05.00]第二句\n")
                ))
            }),
            ..happy_router()
        },
    );
    let source = KugouLyricsSource::new(transport.clone());

    let candidate = fetch_with(&source, &queries(), 269_000, 0.6)
        .await
        .unwrap()
        .unwrap();

    assert!(!candidate.has_word_lyrics());
    assert!(
        candidate
            .line_lyrics
            .as_deref()
            .unwrap()
            .contains("[00:01.000]第一句")
    );
    assert!(transport.requests().last().unwrap().url.contains("fmt=lrc"));
}

#[tokio::test]
async fn an_lrc_without_timestamps_is_rejected() {
    let transport = FakeTransport::new();
    install(
        &transport,
        Router {
            krc: Box::new(|| status(404)),
            lrc: Box::new(|| {
                json_ok(&format!(
                    r#"{{"status":200,"content":"{}"}}"#,
                    base64_of("没有任何时间戳的纯文本")
                ))
            }),
            ..happy_router()
        },
    );
    let source = KugouLyricsSource::new(transport);

    assert!(
        fetch_with(&source, &queries(), 269_000, 0.6)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn tries_at_most_two_queries_and_skips_blank_ones() {
    let transport = FakeTransport::new();
    let searches = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&searches);
    install(
        &transport,
        Router {
            hash: Box::new(|| status(500)),
            search: Box::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                json_ok(r#"{"candidates":[]}"#)
            }),
            ..happy_router()
        },
    );
    let source = KugouLyricsSource::new(transport);
    let many = vec![
        "  ".to_owned(),
        "a".to_owned(),
        String::new(),
        "b".to_owned(),
        "c".to_owned(),
    ];

    assert!(
        fetch_with(&source, &many, 1_000, 0.6)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(searches.load(Ordering::SeqCst), 2);

    assert!(
        fetch_with(&source, &[], 1_000, 0.6)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(searches.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn the_second_query_is_tried_when_the_first_one_has_no_usable_candidate() {
    let transport = FakeTransport::new();
    let call = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call);
    install(
        &transport,
        Router {
            search: Box::new(move |_| {
                if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                    json_ok(r#"{"candidates":[]}"#)
                } else {
                    json_ok(&candidates_json(&[("77", "KEY", "晴天", "周杰伦", 269)]))
                }
            }),
            ..happy_router()
        },
    );
    let source = KugouLyricsSource::new(transport);

    assert!(
        fetch_with(&source, &queries(), 269_000, 0.6)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(call.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn every_kind_of_failure_degrades_to_no_result_instead_of_an_error() {
    // 证书域名不匹配等传输层故障。
    let transport = FakeTransport::new();
    transport.set_fallback(|_| status(500));
    let source = KugouLyricsSource::new(transport.clone());
    assert!(
        fetch_with(&source, &queries(), 269_000, 0.6)
            .await
            .unwrap()
            .is_none()
    );

    let broken = FakeTransport::new();
    for _ in 0..6 {
        broken.enqueue_error(TransportError::Network(
            "certificate name mismatch".to_owned(),
        ));
    }
    let source = KugouLyricsSource::new(broken.clone());
    assert!(
        fetch_with(&source, &queries(), 269_000, 0.6)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!broken.requests().is_empty());

    // 返回的不是 JSON / 业务状态不是 200。
    let html = FakeTransport::new();
    html.set_fallback(|_| HttpResponse {
        status: 200,
        set_cookies: vec![],
        body: b"<html>blocked</html>".to_vec(),
    });
    let source = KugouLyricsSource::new(html);
    assert!(
        fetch_with(&source, &queries(), 269_000, 0.6)
            .await
            .unwrap()
            .is_none()
    );

    let bad_status = FakeTransport::new();
    install(
        &bad_status,
        Router {
            krc: Box::new(|| json_ok(r#"{"status":404,"content":"x"}"#)),
            lrc: Box::new(|| json_ok(r#"{"status":404}"#)),
            ..happy_router()
        },
    );
    let source = KugouLyricsSource::new(bad_status);
    assert!(
        fetch_with(&source, &queries(), 269_000, 0.6)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test(start_paused = true)]
async fn a_hung_request_times_out_into_no_result() {
    let transport = FakeTransport::new();
    transport.enqueue_gated("{}", Arc::new(tokio::sync::Notify::new()));
    transport.set_fallback(|_| status(500));
    let source = KugouLyricsSource::with_timeout(transport.clone(), Duration::from_secs(2));

    let found = fetch_with(&source, &queries(), 269_000, 0.6).await.unwrap();

    assert!(found.is_none());
}

#[tokio::test]
async fn cancellation_is_the_only_error() {
    let transport = FakeTransport::new();
    install(&transport, happy_router());
    let source = KugouLyricsSource::new(transport.clone());
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

    let error = source.fetch(&query, &cancel).await.unwrap_err();

    assert_eq!(error, OnlineError::Cancelled);
    assert!(transport.requests().is_empty());
}

#[tokio::test]
async fn the_threshold_is_clamped_to_the_supported_range() {
    // 歌手不同的同名歌曲总分约 0.56：阈值 0.0 被夹到 0.30 时接受，阈值 5.0 被夹到 1.0 时拒绝。
    let make = || {
        let transport = FakeTransport::new();
        install(
            &transport,
            Router {
                search: Box::new(|_| json_ok(&candidates_json(&[("1", "K", "晴天", "AAA", 269)]))),
                ..happy_router()
            },
        );
        KugouLyricsSource::new(transport)
    };

    assert!(
        fetch_with(&make(), &queries(), 269_000, 0.0)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        fetch_with(&make(), &queries(), 269_000, 5.0)
            .await
            .unwrap()
            .is_none()
    );
}

#[test]
fn the_label_matches_the_settings_switch() {
    assert_eq!(KugouLyricsSource::LABEL, "酷狗音乐");
}

#[tokio::test]
async fn candidate_durations_may_be_milliseconds_like_the_live_api_returns() {
    // 实网探针：krcs 的 duration 是毫秒（255582）；旧接口 / C# 夹具是秒（269）。两种都要能用。
    for (duration, expected_ms) in [(269_582_i64, 269_582_i64), (269, 269_000)] {
        let transport = FakeTransport::new();
        install(
            &transport,
            Router {
                search: Box::new(move |_| {
                    json_ok(&candidates_json(&[(
                        "77",
                        "KEY",
                        "晴天",
                        "周杰伦",
                        duration,
                    )]))
                }),
                ..happy_router()
            },
        );
        let source = KugouLyricsSource::new(transport);

        let candidate = fetch_with(&source, &queries(), 269_000, 0.6)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("duration={duration} 应命中"));

        assert_eq!(candidate.duration_ms, expected_ms);
    }
}
