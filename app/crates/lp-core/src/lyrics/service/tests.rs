use std::time::Duration;

use super::*;
use crate::auth::LoginOptions;
use crate::logs::LogLevel;
use crate::lyrics::RawLyrics;
use crate::netease::NeteaseClient;
use crate::netease::http::TransportError;
use crate::online::OnlineMusicSource::QqMusic;
use crate::online::test_support::{
    StubFallback, StubOnline, StubTags, line_candidate, song, tag_info,
};
use crate::online::{LyricsSource, OnlineMusicSearch};
use crate::session::NeteaseSession;
use crate::session::test_support::RecordingStore;
use crate::tags::AudioTagService;
use crate::test_util::{FakeTransport, test_client};

const NOT_LOGGED_IN: &str = r#"{"code":301,"message":"需要登录"}"#;
const ACCOUNT_OK: &str =
    r#"{"code":200,"account":{"id":42},"profile":{"userId":42,"nickname":"测试用户","vipType":0}}"#;
const ACCOUNT_NULL: &str = r#"{"code":200,"account":null,"profile":null}"#;
const REFRESH_OK: &str = r#"{"code":200}"#;

/// 带逐字 YRC、LRC 与译文的完整响应。
const FULL_LYRICS: &str = r#"{
  "code": 200,
  "lrc": { "lyric": "[00:12.580]第一句\n[00:16.000]第二句" },
  "tlyric": { "lyric": "[00:12.580]First line\n[00:16.000]Second line" },
  "yrc": { "lyric": "[12580,3420](12580,250,0)第(12830,300,0)一(13130,370,0)句\n[16000,2000](16000,500,0)第二句" }
}"#;

/// 只有逐行 LRC（没有 YRC、没有译文）。
const LINE_ONLY: &str = r#"{"code":200,"lrc":{"lyric":"[00:01.00]甲\n[00:02.00]乙"}}"#;

struct Fixture {
    transport: Arc<FakeTransport>,
    client: Arc<NeteaseClient>,
    store: Arc<RecordingStore>,
    logs: Arc<LogHub>,
    service: LyricsService,
}

fn fixture(logged_in: bool) -> Fixture {
    fixture_with(logged_in, None)
}

fn fixture_with(logged_in: bool, online: Option<Arc<BatchLyricsService>>) -> Fixture {
    let transport = FakeTransport::new();
    let client = Arc::new(test_client(&transport));
    let store = Arc::new(RecordingStore::default());
    let logs = LogHub::new();
    let login = LoginManager::new(
        Arc::clone(&client),
        store.clone(),
        Arc::clone(&logs),
        LoginOptions {
            keepalive_interval: Duration::from_secs(3600),
        },
    );
    if logged_in {
        *store.stored.lock().unwrap() = Some(NeteaseSession {
            cookie: "MUSIC_U=old; __csrf=c".to_owned(),
            nickname: Some("昵称".to_owned()),
            user_id: 42,
            ..Default::default()
        });
        login.restore();
    }
    let mut service = LyricsService::new(Arc::clone(&login), Arc::clone(&logs));
    if let Some(online) = online {
        service = service.with_online(online);
    }
    Fixture {
        transport,
        client,
        store,
        logs,
        service,
    }
}

fn has_log(logs: &LogHub, level: LogLevel, fragment: &str) -> bool {
    logs.recent()
        .iter()
        .any(|entry| entry.level == level && entry.message.contains(fragment))
}

#[tokio::test]
async fn line_mode_returns_plain_lrc_with_translation() {
    let f = fixture(false);
    f.transport.enqueue_json(FULL_LYRICS);

    let preview = f.service.get(33894312, LyricsMode::Line).await.unwrap();

    assert_eq!(
        preview.text,
        [
            "[00:12.580]第一句",
            "[00:12.580]First line",
            "[00:16.000]第二句",
            "[00:16.000]Second line"
        ]
        .join("\n")
    );
    assert_eq!(preview.song_id, 33894312);
    assert_eq!(preview.mode, LyricsMode::Line);
    assert!(!preview.is_word_level);
    assert!(preview.has_word_data);
    assert!(preview.has_translation);
    assert_eq!(preview.line_count, 2);
    assert!(!preview.is_empty);
    assert_eq!(preview.source, "网易云音乐");
    assert!(has_log(
        &f.logs,
        LogLevel::Success,
        "已获取歌曲 33894312 的歌词（逐行，含翻译）"
    ));
}

#[tokio::test]
async fn word_mode_prefers_yrc_and_emits_enhanced_lrc() {
    let f = fixture(false);
    f.transport.enqueue_json(FULL_LYRICS);

    let preview = f.service.get(1, LyricsMode::Word).await.unwrap();

    assert_eq!(
        preview.text,
        [
            "[00:12.580]<00:12.580>第<00:12.830>一<00:13.130>句<00:13.500>",
            "[00:12.580]First line",
            "[00:16.000]<00:16.000>第二句<00:16.500>",
            "[00:16.000]Second line"
        ]
        .join("\n")
    );
    assert!(preview.is_word_level);
    assert!(preview.has_word_data);
    assert!(preview.has_translation);
    assert!(has_log(&f.logs, LogLevel::Success, "（逐字，含翻译）"));
}

#[tokio::test]
async fn word_mode_falls_back_to_line_when_there_is_no_word_data() {
    let f = fixture(false);
    f.transport.enqueue_json(LINE_ONLY);

    let preview = f.service.get(2, LyricsMode::Word).await.unwrap();

    assert_eq!(preview.text, "[00:01.000]甲\n[00:02.000]乙");
    assert!(!preview.is_word_level);
    assert!(!preview.has_word_data);
    assert!(!preview.has_translation);
    assert_eq!(preview.mode, LyricsMode::Word);
    assert!(has_log(&f.logs, LogLevel::Success, "（逐行）"));
}

#[tokio::test]
async fn songs_without_lyrics_yield_an_empty_preview_not_an_error() {
    let f = fixture(false);
    f.transport.enqueue_json(r#"{"code":200,"nolyric":true}"#);

    let preview = f.service.get(186016, LyricsMode::Line).await.unwrap();

    assert!(preview.is_empty);
    assert_eq!(preview.text, "");
    assert_eq!(preview.line_count, 0);
    assert!(!preview.has_word_data);
    assert!(has_log(&f.logs, LogLevel::Info, "没有可用的歌词"));
}

#[tokio::test]
async fn lyrics_are_fetched_anonymously_without_any_login_request() {
    let f = fixture(false);
    f.transport.enqueue_json(LINE_ONLY);

    f.service.get(2, LyricsMode::Line).await.unwrap();

    let requests = f.transport.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].url.contains("/api/song/lyric?id=2"));
}

#[tokio::test]
async fn invalid_song_ids_fail_without_a_request() {
    let f = fixture(false);

    let error = f.service.get(0, LyricsMode::Line).await.unwrap_err();

    assert_eq!(error.kind, LyricsErrorKind::Failed);
    assert!(error.message.starts_with("获取歌词失败"));
    assert!(f.transport.requests().is_empty());
}

#[tokio::test]
async fn network_failures_are_reported_and_keep_the_login() {
    let f = fixture(true);
    for _ in 0..3 {
        f.transport
            .enqueue_error(TransportError::Network("连接被重置".to_owned()));
    }

    let error = f.service.get(7, LyricsMode::Line).await.unwrap_err();

    assert_eq!(error.kind, LyricsErrorKind::Failed);
    assert_eq!(
        error.message,
        "获取歌词失败：网络连接失败，请检查网络后重试"
    );
    assert!(f.client.is_logged_in());
    assert!(has_log(&f.logs, LogLevel::Error, "获取歌词失败（歌曲 7）"));
}

#[tokio::test]
async fn code_301_rechecks_the_login_and_clears_it_when_still_logged_out() {
    let f = fixture(true);
    f.transport
        .enqueue_json(NOT_LOGGED_IN) // 公开接口
        .enqueue_json(NOT_LOGGED_IN) // weapi v0
        .enqueue_json(NOT_LOGGED_IN) // weapi v1
        .enqueue_json(NOT_LOGGED_IN) // 续期
        .enqueue_json(ACCOUNT_NULL); // 复核账号

    let error = f.service.get(7, LyricsMode::Line).await.unwrap_err();

    assert_eq!(error.kind, LyricsErrorKind::SessionExpired);
    assert_eq!(error.message, "登录状态已失效，请重新扫码登录");
    assert!(!f.client.is_logged_in());
    assert_eq!(*f.store.cleared.lock().unwrap(), 1);
}

#[tokio::test]
async fn code_301_keeps_the_login_when_the_recheck_says_it_is_still_valid() {
    let f = fixture(true);
    f.transport
        .enqueue_json(NOT_LOGGED_IN)
        .enqueue_json(NOT_LOGGED_IN)
        .enqueue_json(NOT_LOGGED_IN)
        .enqueue_json(REFRESH_OK)
        .enqueue_json(ACCOUNT_OK);

    let error = f.service.get(7, LyricsMode::Line).await.unwrap_err();

    assert_eq!(error.kind, LyricsErrorKind::Failed);
    assert!(error.message.ends_with("（已保留登录，可稍后重试）"));
    assert!(f.client.is_logged_in());
    assert_eq!(*f.store.cleared.lock().unwrap(), 0);
}

#[tokio::test]
async fn code_301_without_a_login_does_not_trigger_a_recheck() {
    let f = fixture(false);
    for _ in 0..3 {
        f.transport.enqueue_json(NOT_LOGGED_IN);
    }

    let error = f.service.get(7, LyricsMode::Line).await.unwrap_err();

    assert_eq!(error.kind, LyricsErrorKind::Failed);
    assert!(!error.message.contains("已保留登录"));
    assert_eq!(f.transport.requests().len(), 3);
}

#[test]
fn build_preview_is_pure_and_serializes_to_camel_case() {
    let preview = build_preview(
        9,
        LyricsMode::Word,
        &RawLyrics {
            yrc: Some("[1000,500](1000,500,0)字".to_owned()),
            ..RawLyrics::default()
        },
    );

    let json = serde_json::to_value(&preview).unwrap();
    assert_eq!(json["songId"], 9);
    assert_eq!(json["mode"], "word");
    assert_eq!(json["isWordLevel"], true);
    assert_eq!(json["hasWordData"], true);
    assert_eq!(json["hasTranslation"], false);
    assert_eq!(json["lineCount"], 1);
    assert_eq!(json["isEmpty"], false);
    assert_eq!(json["source"], "网易云音乐");
    assert_eq!(json["text"], "[00:01.000]<00:01.000>字<00:01.500>");

    let mode: LyricsMode = serde_json::from_str("\"line\"").unwrap();
    assert_eq!(mode, LyricsMode::Line);
}

#[test]
fn errors_serialize_as_kind_and_message() {
    let json = serde_json::to_value(LyricsError {
        kind: LyricsErrorKind::SessionExpired,
        message: "x".to_owned(),
    })
    .unwrap();
    assert_eq!(json["kind"], "sessionExpired");
    assert_eq!(json["message"], "x");
}

// ---- 按设置里的「歌词来源」预览 ----

const QQ_LINE: &str = "[00:01.00]QQ 的第一句\n[00:02.00]QQ 的第二句";
const NO_LYRIC: &str = r#"{"code":200,"nolyric":true}"#;

fn sunny_lookup() -> LyricsLookup {
    LyricsLookup {
        song_id: 186016,
        title: "晴天".to_owned(),
        artist: "周杰伦".to_owned(),
        album: "叶惠美".to_owned(),
        duration_ms: 269_000,
    }
}

fn options(priority: OnlineSourcePriority) -> LyricsPreviewOptions {
    LyricsPreviewOptions {
        priority,
        min_score: 0.6,
    }
}

/// QQ 音乐能搜到《晴天》，详情带 `lyrics`；备用词源按给定顺序询问。
fn online_with(
    lyrics: Option<&str>,
    fallback: Vec<Arc<dyn LyricsSource>>,
) -> (Arc<StubOnline>, Arc<BatchLyricsService>) {
    let online = StubOnline::with(
        vec![song(
            QqMusic,
            "0039MnYb0qxYhV",
            "晴天",
            "周杰伦",
            "叶惠美",
            269_000,
        )],
        lyrics,
    );
    let service = Arc::new(BatchLyricsService::new(
        StubTags::with(tag_info("晴天", "周杰伦", 269_000)) as Arc<dyn AudioTagService>,
        Arc::clone(&online) as Arc<dyn OnlineMusicSearch>,
        fallback,
    ));
    (online, service)
}

async fn preview_with(
    f: &Fixture,
    priority: OnlineSourcePriority,
) -> Result<LyricsPreview, LyricsError> {
    f.service
        .preview(&sunny_lookup(), LyricsMode::Line, options(priority))
        .await
}

#[tokio::test]
async fn qq_first_previews_qq_lyrics_without_asking_netease() {
    let (online, batch) = online_with(Some(QQ_LINE), vec![]);
    let f = fixture_with(false, Some(batch));

    let preview = preview_with(&f, OnlineSourcePriority::QqMusicFirst)
        .await
        .unwrap();

    assert_eq!(preview.source, "QQ 音乐");
    assert_eq!(preview.text, QQ_LINE);
    assert_eq!(preview.line_count, 2);
    assert!(!preview.is_empty);
    assert!(f.transport.requests().is_empty());
    assert!(
        online
            .searches()
            .iter()
            .all(|(source, _)| *source == QqMusic)
    );
    assert!(has_log(
        &f.logs,
        LogLevel::Success,
        "已从QQ 音乐获取「晴天」的歌词（逐行）"
    ));
}

#[tokio::test]
async fn netease_first_uses_the_song_id_and_never_searches_when_netease_has_lyrics() {
    let (online, batch) = online_with(Some(QQ_LINE), vec![]);
    let f = fixture_with(false, Some(batch));
    f.transport.enqueue_json(LINE_ONLY);

    let preview = preview_with(&f, OnlineSourcePriority::NeteaseFirst)
        .await
        .unwrap();

    assert_eq!(preview.source, "网易云音乐");
    assert_eq!(preview.text, "[00:01.000]甲\n[00:02.000]乙");
    assert!(
        f.transport.requests()[0]
            .url
            .contains("/api/song/lyric?id=186016")
    );
    assert!(online.searches().is_empty());
}

#[tokio::test]
async fn netease_first_falls_back_to_qq_when_netease_has_no_lyrics() {
    let (_online, batch) = online_with(Some(QQ_LINE), vec![]);
    let f = fixture_with(false, Some(batch));
    f.transport.enqueue_json(NO_LYRIC);

    let preview = preview_with(&f, OnlineSourcePriority::NeteaseFirst)
        .await
        .unwrap();

    assert_eq!(preview.source, "QQ 音乐");
    assert_eq!(f.transport.requests().len(), 1);
}

#[tokio::test]
async fn qq_first_falls_back_to_netease_when_qq_finds_nothing() {
    let (online, batch) = online_with(None, vec![]);
    online.on_search(|_, _| Ok(Vec::new()));
    let f = fixture_with(false, Some(batch));
    f.transport.enqueue_json(LINE_ONLY);

    let preview = preview_with(&f, OnlineSourcePriority::QqMusicFirst)
        .await
        .unwrap();

    assert_eq!(preview.source, "网易云音乐");
    assert!(!online.searches().is_empty());
}

#[tokio::test]
async fn only_priorities_skip_the_other_main_source() {
    let (online, batch) = online_with(Some(QQ_LINE), vec![]);
    let f = fixture_with(false, Some(batch));
    f.transport.enqueue_json(NO_LYRIC);

    let netease_only = preview_with(&f, OnlineSourcePriority::NeteaseOnly)
        .await
        .unwrap();
    assert!(netease_only.is_empty);
    assert_eq!(netease_only.source, "网易云音乐");
    assert!(online.searches().is_empty());

    let qq_only = preview_with(&f, OnlineSourcePriority::QqMusicOnly)
        .await
        .unwrap();
    assert_eq!(qq_only.source, "QQ 音乐");
    assert_eq!(
        f.transport.requests().len(),
        1,
        "仅 QQ 音乐时不该再问网易云"
    );
}

#[tokio::test]
async fn fallback_sources_fill_in_when_no_main_source_has_lyrics() {
    let kugou = StubFallback::returning(
        "酷狗音乐",
        Some(line_candidate(
            "酷狗音乐",
            "晴天",
            "周杰伦",
            "",
            269_000,
            "[00:03.00]酷狗的歌词",
        )),
    );
    let (online, batch) = online_with(None, vec![kugou.clone() as Arc<dyn LyricsSource>]);
    online.on_search(|_, _| Ok(Vec::new()));
    let f = fixture_with(false, Some(batch));
    f.transport.enqueue_json(NO_LYRIC);

    let preview = preview_with(&f, OnlineSourcePriority::NeteaseFirst)
        .await
        .unwrap();

    assert_eq!(preview.source, "酷狗音乐");
    assert_eq!(preview.text, "[00:03.00]酷狗的歌词");
    assert_eq!(kugou.calls(), vec![(0.6, 269_000)]);
}

#[tokio::test]
async fn an_empty_preview_names_the_main_sources_that_were_tried() {
    let (online, batch) = online_with(None, vec![]);
    online.on_search(|_, _| Ok(Vec::new()));
    let f = fixture_with(false, Some(batch));
    f.transport.enqueue_json(NO_LYRIC);

    let preview = preview_with(&f, OnlineSourcePriority::QqMusicFirst)
        .await
        .unwrap();

    assert!(preview.is_empty);
    assert_eq!(preview.source, "QQ 音乐、网易云音乐");
    assert!(has_log(
        &f.logs,
        LogLevel::Info,
        "「晴天」在QQ 音乐、网易云音乐都没有可用的歌词"
    ));
}

#[tokio::test]
async fn a_netease_failure_does_not_stop_the_next_source() {
    let (_online, batch) = online_with(Some(QQ_LINE), vec![]);
    let f = fixture_with(false, Some(batch));
    for _ in 0..3 {
        f.transport
            .enqueue_error(TransportError::Network("连接被重置".to_owned()));
    }

    let preview = preview_with(&f, OnlineSourcePriority::NeteaseFirst)
        .await
        .unwrap();

    assert_eq!(preview.source, "QQ 音乐");
}

#[tokio::test]
async fn the_netease_error_is_returned_when_no_source_has_lyrics() {
    let (online, batch) = online_with(None, vec![]);
    online.on_search(|_, _| Ok(Vec::new()));
    let f = fixture_with(false, Some(batch));
    for _ in 0..3 {
        f.transport
            .enqueue_error(TransportError::Network("连接被重置".to_owned()));
    }

    let error = preview_with(&f, OnlineSourcePriority::NeteaseFirst)
        .await
        .unwrap_err();

    assert_eq!(error.kind, LyricsErrorKind::Failed);
    assert_eq!(
        error.message,
        "获取歌词失败：网络连接失败，请检查网络后重试"
    );
}

#[tokio::test]
async fn an_expired_login_stops_before_any_other_source() {
    let (online, batch) = online_with(Some(QQ_LINE), vec![]);
    let f = fixture_with(true, Some(batch));
    f.transport
        .enqueue_json(NOT_LOGGED_IN)
        .enqueue_json(NOT_LOGGED_IN)
        .enqueue_json(NOT_LOGGED_IN)
        .enqueue_json(NOT_LOGGED_IN)
        .enqueue_json(ACCOUNT_NULL);

    let error = preview_with(&f, OnlineSourcePriority::NeteaseFirst)
        .await
        .unwrap_err();

    assert_eq!(error.kind, LyricsErrorKind::SessionExpired);
    assert!(online.searches().is_empty());
}

#[tokio::test]
async fn without_a_title_only_netease_can_be_asked() {
    let (online, batch) = online_with(Some(QQ_LINE), vec![]);
    let f = fixture_with(false, Some(batch));
    f.transport.enqueue_json(NO_LYRIC);
    let lookup = LyricsLookup {
        song_id: 5,
        ..LyricsLookup::default()
    };

    let preview = f
        .service
        .preview(
            &lookup,
            LyricsMode::Line,
            options(OnlineSourcePriority::NeteaseFirst),
        )
        .await
        .unwrap();

    assert!(preview.is_empty);
    assert!(online.searches().is_empty());
    assert!(has_log(&f.logs, LogLevel::Info, "「歌曲 5」"));
}

#[test]
fn qq_word_lyrics_with_a_real_translation_are_shown_bilingual() {
    let preview = online_preview(
        1,
        LyricsMode::Word,
        "QQ 音乐",
        &LyricsTexts {
            line: Some("[00:01.00]原文"),
            word: Some("[00:01.00]<00:01.00>原<00:01.50>文"),
            bilingual_line: Some("[00:01.00]原文\n[00:01.00]译文"),
            bilingual_word: Some("[00:01.00]<00:01.00>原<00:01.50>文\n[00:01.00]译文"),
        },
    );

    assert!(preview.is_word_level);
    assert!(preview.has_word_data);
    assert!(preview.has_translation);
    assert_eq!(preview.line_count, 1);
    assert_eq!(
        preview.text,
        "[00:01.00]<00:01.00>原<00:01.50>文\n[00:01.00]译文"
    );
}

#[test]
fn a_bilingual_copy_identical_to_the_original_is_not_a_translation() {
    // QQ 音乐没有译文时，「双语」版本就是原文。
    let preview = online_preview(
        1,
        LyricsMode::Line,
        "QQ 音乐",
        &LyricsTexts {
            line: Some("[ti:晴天]\n[00:01.00]原文\n[00:02.00]\n[00:03.00]第二句"),
            bilingual_line: Some("[ti:晴天]\n[00:01.00]原文\n[00:02.00]\n[00:03.00]第二句\n"),
            ..LyricsTexts::default()
        },
    );

    assert!(!preview.has_translation);
    assert!(!preview.has_word_data);
    assert!(!preview.is_word_level);
    assert_eq!(preview.line_count, 2, "标签行与空白行不算歌词行");
    assert_eq!(
        preview.text,
        "[ti:晴天]\n[00:01.00]原文\n[00:02.00]\n[00:03.00]第二句"
    );
}

#[test]
fn word_mode_without_word_lyrics_uses_the_line_text() {
    let preview = online_preview(
        1,
        LyricsMode::Word,
        "LRCLIB",
        &LyricsTexts {
            line: Some("[00:01.00]只有逐行"),
            word: Some("   "),
            ..LyricsTexts::default()
        },
    );

    assert!(!preview.is_word_level);
    assert!(!preview.has_word_data);
    assert_eq!(preview.text, "[00:01.00]只有逐行");
    assert_eq!(preview.source, "LRCLIB");
}

#[test]
fn preview_options_follow_the_current_settings() {
    let mut settings = AppSettings::new("D:\\x");
    settings.lyrics_source_priority = OnlineSourcePriority::QqMusicOnly;
    settings.match_threshold = 0.75;

    assert_eq!(
        LyricsPreviewOptions::from_settings(&settings),
        LyricsPreviewOptions {
            priority: OnlineSourcePriority::QqMusicOnly,
            min_score: 0.75,
        }
    );
}
