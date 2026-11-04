//! QQ 音乐：搜索解析、域名回退、PlayLyricInfo 逐字歌词、旧歌词接口回退、封面。

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::lyrics::{RawLyrics, parser};
use crate::netease::http::{HttpMethod, HttpResponse, TransportError};
use crate::online::model::{OnlineError, OnlineMusicSource, OnlineSongResult};
use crate::online::net::OnlineHttp;
use crate::online::qq::{QqClient, parse_year, parse_year_from_unix};
use crate::online::test_support::{base64_of, qrc_hex, status};
use crate::test_util::FakeTransport;
use crate::utilities::CancelToken;

fn client(transport: &Arc<FakeTransport>) -> QqClient {
    client_with_timeout(transport, Duration::from_secs(20))
}

fn client_with_timeout(transport: &Arc<FakeTransport>, timeout: Duration) -> QqClient {
    QqClient::new(OnlineHttp::new(transport.clone(), timeout))
}

const SEARCH_JSON: &str = r#"{
  "code": 0,
  "data": { "song": { "list": [
    { "songname": "晴天", "songmid": "0039MnYb0qxYhV", "songid": 97773,
      "albumname": "叶惠美", "albummid": "000MkMni19ClKG", "interval": 269,
      "pubtime": 1058284800, "index_album": 3, "index_cd": 0,
      "singer": [ { "name": "周杰伦" }, { "name": "杨瑞代" }, { "name": " " } ] },
    { "songname": "", "songmid": "skip-no-title" },
    { "songname": "没有 mid" },
    { "songname": "双碟", "songmid": "mid2", "index_cd": 1, "interval": 0, "singer": [] }
  ] } }
}"#;

#[tokio::test]
async fn search_parses_new_json_items_and_builds_the_request() {
    let transport = FakeTransport::new();
    transport.enqueue_json(SEARCH_JSON);

    let songs = client(&transport)
        .search("晴天 周杰伦", 20, &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(songs.len(), 2, "没有标题或 mid 的项被丢弃");
    let first = &songs[0];
    assert_eq!(first.source, OnlineMusicSource::QqMusic);
    assert_eq!(first.source_id, "0039MnYb0qxYhV");
    assert_eq!(first.title, "晴天");
    assert_eq!(first.artist, "周杰伦/杨瑞代", "空白歌手名被丢弃，用 / 连接");
    assert_eq!(first.album, "叶惠美");
    assert_eq!(first.duration_ms, 269_000);
    assert_eq!(first.year, Some(2003), "pubtime 折算 UTC 年份");
    assert_eq!(first.track_number, Some(3));
    assert_eq!(first.disc_number, Some(1), "index_cd = 0 → 第 1 碟");
    assert_eq!(first.numeric_source_id.as_deref(), Some("97773"));
    assert_eq!(
        first.cover_url.as_deref(),
        Some("https://y.gtimg.cn/music/photo_new/T002R300x300M000000MkMni19ClKG.jpg")
    );

    let second = &songs[1];
    assert_eq!(second.disc_number, Some(2), "index_cd = 1 → 第 2 碟");
    assert_eq!(second.duration_ms, 0);
    assert_eq!(second.artist, "");
    assert_eq!(second.cover_url, None);
    assert_eq!(second.numeric_source_id, None);
    assert_eq!(second.track_number, None);

    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].url,
        "https://c.y.qq.com/soso/fcgi-bin/search_cp?w=%E6%99%B4%E5%A4%A9%20%E5%91%A8%E6%9D%B0%E4%BC%A6&format=json&p=1&n=20&cr=1&new_json=1&g_tk=5381"
    );
    assert_eq!(requests[0].header("Referer"), Some("https://y.qq.com/"));
    assert!(requests[0].header("User-Agent").unwrap().contains("Chrome"));
}

#[tokio::test]
async fn search_also_understands_the_legacy_item_shape() {
    let transport = FakeTransport::new();
    transport.enqueue_json(
        r#"{"code":0,"data":{"song":{"list":[
          { "title": "Lemon", "mid": "m1", "id": 5,
            "album": { "name": "Lemon", "mid": "am1", "time_public": "2018-03-14" },
            "singer": [ { "name": "米津玄師" } ], "interval": 255 }
        ]}}}"#,
    );

    let songs = client(&transport)
        .search("lemon", 5, &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(songs[0].title, "Lemon");
    assert_eq!(songs[0].source_id, "m1");
    assert_eq!(songs[0].album, "Lemon");
    assert_eq!(songs[0].year, Some(2018));
    assert_eq!(songs[0].numeric_source_id.as_deref(), Some("5"));
    assert!(songs[0].cover_url.as_deref().unwrap().ends_with("am1.jpg"));
}

#[tokio::test]
async fn search_limit_is_clamped_to_1_through_30() {
    let transport = FakeTransport::new();
    transport
        .enqueue_json(r#"{"code":0}"#)
        .enqueue_json(r#"{"code":0}"#);
    let qq = client(&transport);

    qq.search("a", 0, &CancelToken::new()).await.unwrap();
    qq.search("a", 999, &CancelToken::new()).await.unwrap();

    let urls: Vec<String> = transport.requests().into_iter().map(|r| r.url).collect();
    assert!(urls[0].contains("&n=1&"));
    assert!(urls[1].contains("&n=30&"));
}

#[tokio::test]
async fn search_switches_to_the_c6_mirror_when_the_main_host_fails() {
    let transport = FakeTransport::new();
    transport.enqueue_status(500).enqueue_json(SEARCH_JSON);

    let songs = client(&transport)
        .search("晴天", 20, &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(songs.len(), 2);
    let urls: Vec<String> = transport.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(urls.len(), 2);
    assert!(urls[0].starts_with("https://c.y.qq.com/soso/fcgi-bin/search_cp?"));
    assert!(urls[1].starts_with("https://c6.y.qq.com/soso/fcgi-bin/search_cp?"));
}

#[tokio::test]
async fn search_switches_mirror_on_connection_errors_too() {
    let transport = FakeTransport::new();
    transport
        .enqueue_error(TransportError::Network("tls handshake eof".to_owned()))
        .enqueue_json(SEARCH_JSON);

    let songs = client(&transport)
        .search("晴天", 20, &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(songs.len(), 2);
    assert_eq!(transport.requests().len(), 2);
}

#[tokio::test]
async fn search_reports_a_locatable_reason_when_every_mirror_fails() {
    let transport = FakeTransport::new();
    transport.enqueue_status(500).enqueue_status(503);

    let error = client(&transport)
        .search("晴天", 20, &CancelToken::new())
        .await
        .unwrap_err();

    assert_eq!(
        error,
        OnlineError::Failed(
            "QQ 音乐搜索接口不可用（HTTP 503），请稍后重试或改用网易云音乐".to_owned()
        )
    );

    let network = FakeTransport::new();
    network
        .enqueue_error(TransportError::Network("a".to_owned()))
        .enqueue_error(TransportError::Network("dns failed".to_owned()));
    let error = client(&network)
        .search("晴天", 20, &CancelToken::new())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        OnlineError::Failed(
            "QQ 音乐搜索接口不可用（网络请求失败：dns failed），请稍后重试或改用网易云音乐"
                .to_owned()
        )
    );
}

#[tokio::test]
async fn search_reports_business_codes_and_bad_json() {
    let transport = FakeTransport::new();
    transport
        .enqueue_json(r#"{"code":-1}"#)
        .enqueue_raw(200, "<html>nope</html>")
        .enqueue_json(r#"{"code":0,"data":{"song":{"list":"oops"}}}"#)
        .enqueue_json(r#"{"code":0,"data":{}}"#);
    let qq = client(&transport);

    assert_eq!(
        qq.search("a", 5, &CancelToken::new()).await.unwrap_err(),
        OnlineError::Failed("QQ 音乐搜索失败：code=-1".to_owned())
    );
    assert!(matches!(
        qq.search("a", 5, &CancelToken::new()).await.unwrap_err(),
        OnlineError::Failed(message) if message.contains("不是合法 JSON")
    ));
    assert!(
        qq.search("a", 5, &CancelToken::new())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        qq.search("a", 5, &CancelToken::new())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(start_paused = true)]
async fn search_timeouts_do_not_switch_mirrors() {
    let transport = FakeTransport::new();
    // 第一个请求永远不返回；超时后直接报错，不再试第二个域名。
    transport.enqueue_gated(r#"{"code":0}"#, Arc::new(tokio::sync::Notify::new()));
    let qq = client_with_timeout(&transport, Duration::from_secs(5));

    let error = qq.search("a", 5, &CancelToken::new()).await.unwrap_err();

    assert!(
        matches!(&error, OnlineError::Failed(message) if message.contains("超时")),
        "{error}"
    );
    assert_eq!(transport.requests().len(), 1);
}

#[tokio::test]
async fn search_can_be_cancelled() {
    let transport = FakeTransport::new();
    let cancel = CancelToken::new();
    cancel.cancel();

    let error = client(&transport)
        .search("a", 5, &cancel)
        .await
        .unwrap_err();

    assert_eq!(error, OnlineError::Cancelled);
    assert!(transport.requests().is_empty());
}

#[test]
fn years_parse_like_csharp() {
    assert_eq!(parse_year(Some("2018-03-14")), Some(2018));
    assert_eq!(parse_year(Some("  2003 ")), Some(2003));
    assert_eq!(parse_year(Some("1999")), Some(1999));
    assert_eq!(parse_year(Some("199")), None);
    assert_eq!(parse_year(Some("abcd")), None);
    assert_eq!(parse_year(Some("0000-01-01")), None, "年份必须大于 0");
    assert_eq!(parse_year(Some("3000-01-01")), None, "年份必须小于 3000");
    assert_eq!(parse_year(Some("")), None);
    assert_eq!(parse_year(None), None);
    // `uint.TryParse` 允许前导加号。
    assert_eq!(parse_year(Some("+201")), Some(201));

    assert_eq!(parse_year_from_unix(1_058_284_800), Some(2003));
    assert_eq!(parse_year_from_unix(0), None);
    assert_eq!(parse_year_from_unix(-5), None);
    assert_eq!(parse_year_from_unix(253_402_300_800), None);
    // 年底 / 年初的边界按 UTC 折算。
    assert_eq!(parse_year_from_unix(1_104_537_599), Some(2004));
    assert_eq!(parse_year_from_unix(1_104_537_600), Some(2005));
}

// ───────────────────────────── 详情 ─────────────────────────────

fn qq_song() -> OnlineSongResult {
    OnlineSongResult {
        source: OnlineMusicSource::QqMusic,
        source_id: "0039MnYb0qxYhV".to_owned(),
        title: "晴天".to_owned(),
        artist: "周杰伦".to_owned(),
        album: "叶惠美".to_owned(),
        duration_ms: 269_000,
        year: Some(2003),
        track_number: Some(3),
        disc_number: Some(1),
        genre: None,
        composer: None,
        cover_url: None,
        numeric_source_id: Some("97773".to_owned()),
    }
}

const QRC_PLAIN: &str = "[1000,1000]Hello(1000,500) world(1500,500)";

#[tokio::test]
async fn details_decrypt_the_qrc_word_lyrics_and_build_the_request() {
    let transport = FakeTransport::new();
    let translation = base64_of("[00:01.00]你好 世界\n");
    transport.enqueue_raw(
        200,
        &format!(
            r#"{{"code":0,"request":{{"code":0,"data":{{"lyric":"{}","trans":"{translation}","roma":""}}}}}}"#,
            qrc_hex(QRC_PLAIN)
        ),
    );

    let details = client(&transport)
        .details(&qq_song(), &CancelToken::new())
        .await
        .unwrap();

    let parsed = parser::parse(&RawLyrics {
        qrc: Some(QRC_PLAIN.to_owned()),
        translated_lrc: Some("[00:01.00]你好 世界\n".to_owned()),
        ..RawLyrics::default()
    });
    assert!(details.has_word_lyrics());
    assert!(details.has_translation());
    assert_eq!(
        details.word_lyrics.as_deref(),
        Some(parser::to_enhanced_lrc(&parsed, false).as_str())
    );
    assert_eq!(
        details.bilingual_word_lyrics.as_deref(),
        Some(parser::to_enhanced_lrc(&parsed, true).as_str())
    );
    assert_eq!(
        details.line_lyrics.as_deref(),
        Some(parser::to_plain_lrc(&parsed, false).as_str())
    );
    assert_eq!(
        details.bilingual_line_lyrics.as_deref(),
        Some(parser::to_plain_lrc(&parsed, true).as_str())
    );
    assert!(
        details
            .bilingual_line_lyrics
            .as_deref()
            .unwrap()
            .contains("你好 世界")
    );

    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method, HttpMethod::Post);
    assert_eq!(request.url, "https://u.y.qq.com/cgi-bin/musicu.fcg");
    assert_eq!(request.header("User-Agent"), Some("okhttp/3.14.9"));
    assert_eq!(request.header("Referer"), Some("https://y.qq.com/"));
    assert_eq!(
        request.header("Content-Type"),
        Some("application/json; charset=utf-8")
    );
    let body: Value = serde_json::from_slice(request.body.as_ref().unwrap()).unwrap();
    let param = &body["request"]["param"];
    assert_eq!(
        body["request"]["module"],
        "music.musichallSong.PlayLyricInfo"
    );
    assert_eq!(body["request"]["method"], "GetPlayLyricInfo");
    assert_eq!(body["comm"]["tmeAppID"], "qqmusiclight");
    assert_eq!(param["songID"], 97773);
    assert_eq!(param["interval"], 269);
    assert_eq!(param["qrc"], 1);
    assert_eq!(param["crypt"], 1);
    assert_eq!(param["roma"], 1);
    assert_eq!(param["trans"], 1);
    assert_eq!(param["songName"], base64_of("晴天"));
    assert_eq!(param["singerName"], base64_of("周杰伦"));
    assert_eq!(param["albumName"], base64_of("叶惠美"));
}

#[tokio::test]
async fn details_without_qrc_use_the_base64_line_lyrics() {
    let transport = FakeTransport::new();
    transport.enqueue_raw(
        200,
        &format!(
            r#"{{"code":0,"request":{{"data":{{"lyric":"{}"}}}}}}"#,
            base64_of("[00:01.00]第一句\n[00:05.00]第二句\n")
        ),
    );

    let details = client(&transport)
        .details(&qq_song(), &CancelToken::new())
        .await
        .unwrap();

    assert!(!details.has_word_lyrics());
    assert_eq!(details.word_lyrics, None);
    assert_eq!(details.bilingual_word_lyrics, None);
    let line = details.line_lyrics.as_deref().unwrap();
    assert!(line.contains("[00:01.000]第一句"));
    // 与 C# 一致：没有译文时双语版本等同原文，所以 HasTranslation 总是为真。
    assert_eq!(details.bilingual_line_lyrics.as_deref(), Some(line));
    assert!(details.has_translation());
}

#[tokio::test]
async fn encrypted_translations_are_converted_to_line_lrc() {
    let transport = FakeTransport::new();
    let encrypted_translation = qrc_hex("[1000,1000]你好(1000,500)世界(1500,500)");
    transport.enqueue_raw(
        200,
        &format!(
            r#"{{"code":0,"request":{{"data":{{"lyric":"{}","trans":"{encrypted_translation}"}}}}}}"#,
            qrc_hex(QRC_PLAIN)
        ),
    );

    let details = client(&transport)
        .details(&qq_song(), &CancelToken::new())
        .await
        .unwrap();

    assert!(
        details
            .bilingual_line_lyrics
            .as_deref()
            .unwrap()
            .contains("你好世界")
    );
}

#[tokio::test]
async fn details_fall_back_to_the_legacy_interface_when_play_lyric_fails() {
    let transport = FakeTransport::new();
    transport.enqueue_status(500).enqueue_json(
        r#"{"retcode":0,"lyric":"[00:01.00]Hello &amp; bye","trans":"[00:01.00]你好 &amp; 再见"}"#,
    );

    let details = client(&transport)
        .details(&qq_song(), &CancelToken::new())
        .await
        .unwrap();

    let requests = transport.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, HttpMethod::Post);
    assert_eq!(
        requests[1].url,
        "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg?songmid=0039MnYb0qxYhV&format=json&nobase64=1&g_tk=5381&trans=1&trans_t=0"
    );
    assert!(
        details
            .line_lyrics
            .as_deref()
            .unwrap()
            .contains("Hello & bye")
    );
    assert!(
        details
            .bilingual_line_lyrics
            .as_deref()
            .unwrap()
            .contains("你好 & 再见")
    );
    assert_eq!(details.word_lyrics, None);
}

#[tokio::test]
async fn legacy_lyrics_without_translation_are_returned_as_is() {
    let transport = FakeTransport::new();
    transport
        .enqueue_json(r#"{"code":1}"#)
        .enqueue_json(r#"{"retcode":0,"lyric":"[00:01.00]&#26202;&#23433;"}"#);

    let details = client(&transport)
        .details(&qq_song(), &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(details.line_lyrics.as_deref(), Some("[00:01.00]晚安"));
    assert_eq!(details.bilingual_line_lyrics, None);
}

#[tokio::test]
async fn songs_without_a_numeric_id_go_straight_to_the_legacy_interface() {
    let transport = FakeTransport::new();
    transport.enqueue_json(r#"{"retcode":0,"lyric":"[00:01.00]x"}"#);
    let mut song = qq_song();
    song.numeric_source_id = None;

    let details = client(&transport)
        .details(&song, &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(transport.requests().len(), 1);
    assert_eq!(transport.requests()[0].method, HttpMethod::Get);
    assert_eq!(details.line_lyrics.as_deref(), Some("[00:01.00]x"));
}

#[tokio::test]
async fn details_survive_total_lyric_failure_and_still_return_the_cover() {
    let transport = FakeTransport::new();
    transport.set_fallback(|request| {
        if request.url.contains("cover.jpg") {
            HttpResponse {
                status: 200,
                set_cookies: vec![],
                body: vec![0xFF, 0xD8, 0xFF, 1, 2, 3],
            }
        } else {
            status(500)
        }
    });
    let mut song = qq_song();
    song.cover_url = Some("https://y.gtimg.cn/cover.jpg".to_owned());

    let details = client(&transport)
        .details(&song, &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(details.line_lyrics, None);
    assert_eq!(
        details.cover_data.as_deref(),
        Some(&[0xFF, 0xD8, 0xFF, 1, 2, 3][..])
    );
    assert!(!details.has_word_lyrics());
    let cover_request = &transport.requests()[0];
    assert_eq!(cover_request.header("Referer"), Some("https://y.qq.com/"));
}

#[tokio::test]
async fn bad_cover_responses_are_ignored() {
    for response in [
        status(404),
        HttpResponse {
            status: 200,
            set_cookies: vec![],
            body: Vec::new(),
        },
    ] {
        let transport = FakeTransport::new();
        let reply = response.clone();
        transport.set_fallback(move |request| {
            if request.url.contains("cover.jpg") {
                reply.clone()
            } else {
                status(500)
            }
        });
        let mut song = qq_song();
        song.cover_url = Some("https://y.gtimg.cn/cover.jpg".to_owned());

        let details = client(&transport)
            .details(&song, &CancelToken::new())
            .await
            .unwrap();

        assert_eq!(details.cover_data, None);
    }
}

#[tokio::test]
async fn covers_over_ten_megabytes_are_dropped() {
    let transport = FakeTransport::new();
    transport.set_fallback(|request| {
        if request.url.contains("big.jpg") {
            HttpResponse {
                status: 200,
                set_cookies: vec![],
                body: vec![7u8; 10 * 1024 * 1024 + 1],
            }
        } else {
            status(500)
        }
    });
    let mut song = qq_song();
    song.cover_url = Some("https://y.gtimg.cn/big.jpg".to_owned());

    let details = client(&transport)
        .details(&song, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(details.cover_data, None);

    // 恰好 10 MB 仍然收下。
    let transport = FakeTransport::new();
    transport.set_fallback(|request| {
        if request.url.contains("big.jpg") {
            HttpResponse {
                status: 200,
                set_cookies: vec![],
                body: vec![7u8; 10 * 1024 * 1024],
            }
        } else {
            status(500)
        }
    });
    let details = client(&transport)
        .details(&song, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(
        details.cover_data.map(|data| data.len()),
        Some(10 * 1024 * 1024)
    );
}

#[tokio::test(start_paused = true)]
async fn a_play_lyric_timeout_fails_the_details_instead_of_silently_retrying() {
    let transport = FakeTransport::new();
    transport.enqueue_gated(r#"{}"#, Arc::new(tokio::sync::Notify::new()));
    let qq = client_with_timeout(&transport, Duration::from_secs(5));

    let error = qq
        .details(&qq_song(), &CancelToken::new())
        .await
        .unwrap_err();

    assert!(
        matches!(&error, OnlineError::Failed(message) if message.contains("超时")),
        "{error}"
    );
    assert_eq!(transport.requests().len(), 1);
}

#[tokio::test]
async fn malformed_play_lyric_json_falls_back_to_the_legacy_interface() {
    let transport = FakeTransport::new();
    transport
        .enqueue_raw(200, "not json")
        .enqueue_json(r#"{"retcode":0,"lyric":"[00:01.00]ok"}"#);

    let details = client(&transport)
        .details(&qq_song(), &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(details.line_lyrics.as_deref(), Some("[00:01.00]ok"));
    assert_eq!(transport.requests().len(), 2);
}

#[tokio::test]
async fn a_blank_play_lyric_payload_falls_back_to_the_legacy_interface() {
    let transport = FakeTransport::new();
    transport
        .enqueue_json(r#"{"code":0,"request":{"data":{"lyric":""}}}"#)
        .enqueue_json(r#"{"retcode":0,"lyric":"[00:01.00]legacy"}"#);

    let details = client(&transport)
        .details(&qq_song(), &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(details.line_lyrics.as_deref(), Some("[00:01.00]legacy"));
}
