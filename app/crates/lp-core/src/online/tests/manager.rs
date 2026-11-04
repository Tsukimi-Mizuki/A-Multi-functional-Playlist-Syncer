//! 编辑器背后的在线命令编排：搜索打分排序、取详情、备用词源、阈值。

use std::sync::Arc;

use crate::lyrics::batch::SettingsProvider;
use crate::models::LyricsSourceKind;
use crate::online::manager::{
    FallbackRequest, FileContext, LoadRequest, OnlineManager, SearchRequest,
};
use crate::online::model::{LyricsSource, OnlineMusicSource, OnlineSongDetails, OnlineSongResult};
use crate::online::test_support::{StubFallback, StubOnline, line_candidate, song};
use crate::settings::AppSettings;
use crate::utilities::CancelToken;

use OnlineMusicSource::{Netease, QqMusic};

fn provider(threshold: f64) -> SettingsProvider {
    let mut settings = AppSettings::new("D:\\x");
    settings.match_threshold = threshold;
    Arc::new(move || settings.clone())
}

fn manager(online: &Arc<StubOnline>, fallback: Vec<Arc<dyn LyricsSource>>) -> OnlineManager {
    OnlineManager::new(online.clone(), fallback, provider(0.6))
}

fn context() -> FileContext {
    FileContext {
        title: Some("晴天".to_owned()),
        artist: Some("周杰伦".to_owned()),
        album: Some("叶惠美".to_owned()),
        duration_ms: 269_000,
        file_path: Some("D:\\音乐\\01 - 晴天.mp3".to_owned()),
    }
}

fn search_request(
    source: LyricsSourceKind,
    query: &str,
    file: Option<FileContext>,
) -> SearchRequest {
    SearchRequest {
        source,
        query: query.to_owned(),
        file_context: file,
    }
}

fn candidates() -> Vec<OnlineSongResult> {
    vec![
        song(Netease, "3", "完全无关", "路人", "x", 100_000),
        song(Netease, "2", "晴天 (Live)", "周杰伦", "叶惠美", 269_000),
        song(Netease, "1", "晴天", "周杰伦", "叶惠美", 269_000),
    ]
}

#[tokio::test]
async fn scored_search_sorts_by_total_descending_and_marks_low_matches() {
    let online = StubOnline::with(candidates(), None);
    let manager = manager(&online, vec![]);

    let response = manager
        .search(
            search_request(LyricsSourceKind::Netease, "晴天", Some(context())),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert!(response.scored);
    assert_eq!(response.query, "晴天");
    assert_eq!(response.threshold, 0.6);
    assert_eq!(response.threshold_percent, "60");
    let ids: Vec<&str> = response
        .items
        .iter()
        .map(|item| item.song.song.source_id.as_str())
        .collect();
    assert_eq!(ids, vec!["1", "2", "3"]);
    let totals: Vec<f64> = response
        .items
        .iter()
        .map(|item| item.score.as_ref().unwrap().total)
        .collect();
    assert!(totals[0] > totals[1] && totals[1] > totals[2], "{totals:?}");
    assert!(!response.items[0].below_threshold);
    assert!(response.items[2].below_threshold);
    assert!(response.items[0].score.as_ref().unwrap().confident);
    assert_eq!(response.items[0].song.display_title, "晴天");
    assert_eq!(response.items[0].song.duration_text, "4:29");
    assert_eq!(response.items[0].song.source_label, "网易云音乐");
    assert_eq!(online.searches()[0], (Netease, "晴天".to_owned()));
}

#[tokio::test]
async fn equal_scores_keep_the_order_the_source_returned() {
    let online = StubOnline::with(
        vec![
            song(Netease, "a", "晴天", "周杰伦", "叶惠美", 269_000),
            song(Netease, "b", "晴天", "周杰伦", "叶惠美", 269_000),
            song(Netease, "c", "晴天", "周杰伦", "叶惠美", 269_000),
        ],
        None,
    );
    let response = manager(&online, vec![])
        .search(
            search_request(LyricsSourceKind::Netease, "晴天", Some(context())),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    let ids: Vec<&str> = response
        .items
        .iter()
        .map(|item| item.song.song.source_id.as_str())
        .collect();
    assert_eq!(ids, vec!["a", "b", "c"]);
}

#[tokio::test]
async fn search_without_a_file_context_is_unscored_and_keeps_source_order() {
    let online = StubOnline::with(candidates(), None);

    let response = manager(&online, vec![])
        .search(
            search_request(LyricsSourceKind::QqMusic, "  晴天  ", None),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert!(!response.scored);
    assert_eq!(response.items.len(), 3);
    assert!(
        response
            .items
            .iter()
            .all(|item| item.score.is_none() && !item.below_threshold)
    );
    assert_eq!(response.items[0].song.song.source_id, "3");
    assert_eq!(online.searches()[0], (QqMusic, "晴天".to_owned()));
}

#[tokio::test]
async fn search_rejects_fallback_sources_and_empty_queries() {
    let online = StubOnline::with(candidates(), None);
    let manager = manager(&online, vec![]);

    for kind in [LyricsSourceKind::Kugou, LyricsSourceKind::Lrclib] {
        let error = manager
            .search(search_request(kind, "晴天", None), &CancelToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.kind, "fallbackSource");
        assert!(error.message.contains("仅歌词"));
    }

    let empty = manager
        .search(
            search_request(LyricsSourceKind::Netease, "   ", None),
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(empty.kind, "emptyQuery");
    assert_eq!(empty.message, "请输入要搜索的歌曲、歌手或专辑。");
    assert!(online.searches().is_empty());
}

#[tokio::test]
async fn a_blank_query_uses_the_keyword_derived_from_the_file_name() {
    let online = StubOnline::with(vec![], None);
    let manager = manager(&online, vec![]);
    let mut file = context();
    file.file_path = Some("D:\\音乐\\01 - 晴天_Live  版.flac".to_owned());

    let response = manager
        .search(
            search_request(LyricsSourceKind::Netease, "", Some(file)),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(response.query, "晴天 Live 版");
    assert_eq!(online.searches()[0].1, "晴天 Live 版");

    // 没有文件名可推时仍然提示输入。
    let mut nameless = context();
    nameless.file_path = None;
    let error = manager
        .search(
            search_request(LyricsSourceKind::Netease, "", Some(nameless)),
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, "emptyQuery");
}

#[tokio::test]
async fn scoring_falls_back_to_the_file_name_and_then_the_query_when_the_tag_title_is_missing() {
    let online = StubOnline::with(
        vec![song(Netease, "1", "晴天", "周杰伦", "叶惠美", 269_000)],
        None,
    );
    let manager = manager(&online, vec![]);

    let mut by_file_name = context();
    by_file_name.title = None;
    by_file_name.file_path = Some("D:\\音乐\\07. 晴天.mp3".to_owned());
    let response = manager
        .search(
            search_request(LyricsSourceKind::Netease, "别的关键词", Some(by_file_name)),
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(response.items[0].score.as_ref().unwrap().title, 1.0);

    let mut by_query = context();
    by_query.title = Some("  ".to_owned());
    by_query.file_path = None;
    let response = manager
        .search(
            search_request(LyricsSourceKind::Netease, "晴天", Some(by_query)),
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(response.items[0].score.as_ref().unwrap().title, 1.0);
}

#[tokio::test]
async fn search_failures_are_reported_with_the_chinese_message() {
    let online = StubOnline::with(vec![], None);
    online.on_search(|_, _| {
        Err(crate::online::OnlineError::failed(
            "QQ 音乐搜索失败：code=-1",
        ))
    });

    let error = manager(&online, vec![])
        .search(
            search_request(LyricsSourceKind::QqMusic, "晴天", None),
            &CancelToken::new(),
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind, "failed");
    assert_eq!(error.message, "QQ 音乐搜索失败：code=-1");
}

fn details_with_everything(candidate: &OnlineSongResult) -> OnlineSongDetails {
    let mut details = OnlineSongDetails::without_extras(candidate.clone());
    details.song.title = "详情里的标题".to_owned();
    details.line_lyrics = Some("[00:01.00]原文".to_owned());
    details.word_lyrics = Some("[00:01.00]<00:01.00>原文".to_owned());
    details.bilingual_line_lyrics = Some("[00:01.00]原文\n[00:01.00]译文".to_owned());
    details.bilingual_word_lyrics = Some("[00:01.00]<00:01.00>原文\n[00:01.00]译文".to_owned());
    details.cover_data = Some(vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 1]);
    details
}

#[tokio::test]
async fn loading_a_result_uses_the_cached_search_summary() {
    let online = StubOnline::with(
        vec![song(QqMusic, "mid1", "晴天", "周杰伦", "叶惠美", 269_000)],
        None,
    );
    online.on_details(|candidate| {
        // QQ 取歌词需要搜索摘要里的数值 ID。
        assert_eq!(candidate.album, "叶惠美");
        Ok(Some(details_with_everything(candidate)))
    });
    let manager = manager(&online, vec![]);
    manager
        .search(
            search_request(LyricsSourceKind::QqMusic, "晴天", None),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    let loaded = manager
        .load_result(
            LoadRequest {
                source: LyricsSourceKind::QqMusic,
                id: "mid1".to_owned(),
                word_lyrics: true,
                song: None,
            },
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(loaded.song.song.title, "详情里的标题");
    assert_eq!(
        loaded.lyrics.as_deref(),
        Some("[00:01.00]<00:01.00>原文\n[00:01.00]译文")
    );
    assert_eq!(loaded.lyrics_label.as_deref(), Some("双语逐字歌词"));
    assert!(loaded.has_word_lyrics && loaded.has_translation && !loaded.word_fallback);
    assert_eq!(loaded.line_lyrics.as_deref(), Some("[00:01.00]原文"));
    let cover = loaded.cover.as_ref().unwrap();
    assert_eq!(cover.mime_type, "image/png");
    assert_eq!(cover.size, 9);
    assert_eq!(cover.base64, "iVBORw0KGgoB");
    assert_eq!(
        loaded.status_text,
        "已载入QQ 音乐的标签双语逐字歌词、封面，请确认后保存。"
    );
}

#[tokio::test]
async fn loading_prefers_an_explicit_song_and_works_for_netease_without_a_cache() {
    let online = StubOnline::with(vec![], None);
    online.on_details(|candidate| {
        let mut details = OnlineSongDetails::without_extras(candidate.clone());
        details.line_lyrics = Some("[00:01.00]只有逐行".to_owned());
        Ok(Some(details))
    });
    let manager = manager(&online, vec![]);

    // 网易云：没有缓存也能按 ID 取。
    let loaded = manager
        .load_result(
            LoadRequest {
                source: LyricsSourceKind::Netease,
                id: "186016".to_owned(),
                word_lyrics: true,
                song: None,
            },
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(online.detail_calls()[0], (Netease, "186016".to_owned()));
    // 要逐字但只有逐行：回退并说明；同时没有翻译。
    assert!(loaded.word_fallback);
    assert_eq!(
        loaded.lyrics_label.as_deref(),
        Some("逐行歌词（该来源无逐字）（该来源无翻译）")
    );
    assert_eq!(loaded.cover, None);
    assert_eq!(
        loaded.status_text,
        "已载入网易云音乐的标签逐行歌词（该来源无逐字）（该来源无翻译），请确认后保存。"
    );

    // 显式传入摘要（QQ，缓存里没有）。
    let explicit = song(QqMusic, "m", "晴天", "周杰伦", "叶惠美", 1);
    let loaded = manager
        .load_result(
            LoadRequest {
                source: LyricsSourceKind::QqMusic,
                id: "m".to_owned(),
                word_lyrics: false,
                song: Some(explicit),
            },
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(loaded.song.song.source_id, "m");
}

#[tokio::test]
async fn loading_an_unknown_qq_song_or_a_fallback_source_is_an_error() {
    let online = StubOnline::with(vec![], None);
    let manager = manager(&online, vec![]);

    let unknown = manager
        .load_result(
            LoadRequest {
                source: LyricsSourceKind::QqMusic,
                id: "nope".to_owned(),
                word_lyrics: false,
                song: None,
            },
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(unknown.kind, "unknownSong");

    let fallback = manager
        .load_result(
            LoadRequest {
                source: LyricsSourceKind::Kugou,
                id: "1".to_owned(),
                word_lyrics: false,
                song: None,
            },
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(fallback.kind, "primarySource");
    assert!(online.detail_calls().is_empty());
}

#[tokio::test]
async fn details_that_come_back_empty_still_produce_a_view_from_the_summary() {
    let online = StubOnline::with(
        vec![song(Netease, "5", "晴天", "周杰伦", "叶惠美", 1)],
        None,
    );
    online.on_details(|_| Ok(None));
    let manager = manager(&online, vec![]);
    manager
        .search(
            search_request(LyricsSourceKind::Netease, "晴天", None),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    let loaded = manager
        .load_result(
            LoadRequest {
                source: LyricsSourceKind::Netease,
                id: "5".to_owned(),
                word_lyrics: false,
                song: None,
            },
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(loaded.song.song.title, "晴天");
    assert_eq!(loaded.lyrics, None);
    assert_eq!(loaded.lyrics_label, None);
    assert_eq!(
        loaded.status_text,
        "已载入网易云音乐的标签未获取到歌词，请确认后保存。"
    );
}

#[tokio::test]
async fn fallback_lyrics_are_returned_with_a_match_score_and_the_csharp_status_text() {
    let online = StubOnline::with(vec![], None);
    let kugou = StubFallback::returning(
        "酷狗音乐",
        Some(crate::online::LyricsSourceCandidate {
            word_lyrics: Some("[00:01.00]<00:01.00>冷".to_owned()),
            bilingual_word_lyrics: Some("[00:01.00]<00:01.00>冷\n[00:01.00]cold".to_owned()),
            bilingual_line_lyrics: Some("[00:01.00]冷\n[00:01.00]cold".to_owned()),
            ..line_candidate(
                "酷狗音乐",
                "冷门曲子",
                "同人歌手",
                "",
                200_500,
                "[00:01.00]冷",
            )
        }),
    );
    let lrclib = StubFallback::returning("LRCLIB", None);
    let manager = manager(&online, vec![kugou.clone(), lrclib.clone()]);

    let found = manager
        .fetch_fallback(
            FallbackRequest {
                source: LyricsSourceKind::Kugou,
                title: "冷门曲子".to_owned(),
                artist: "同人歌手".to_owned(),
                album: None,
                duration_ms: 200_000,
                word_lyrics: true,
            },
            &CancelToken::new(),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(found.source_label, "酷狗音乐");
    assert_eq!(
        found.lyrics.as_deref(),
        Some("[00:01.00]<00:01.00>冷\n[00:01.00]cold")
    );
    assert_eq!(found.lyrics_label.as_deref(), Some("双语逐字歌词"));
    assert_eq!(
        found.status_text,
        "已从酷狗音乐载入双语逐字歌词（仅歌词，标签与封面保持当前内容），请确认后保存。"
    );
    assert!(found.score.total > 0.9);
    // 关键词与阈值取自本地歌曲与设置。
    assert!(!kugou.searched_queries().is_empty());
    assert_eq!(kugou.calls()[0], (0.6, 200_000));
    assert!(lrclib.searched_queries().is_empty());

    // 命中不了 → None。
    let none = manager
        .fetch_fallback(
            FallbackRequest {
                source: LyricsSourceKind::Lrclib,
                title: "冷门曲子".to_owned(),
                artist: String::new(),
                album: None,
                duration_ms: 0,
                word_lyrics: false,
            },
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert!(none.is_none());
}

#[tokio::test]
async fn fallback_requests_validate_their_inputs() {
    let online = StubOnline::with(vec![], None);
    let manager = manager(&online, vec![StubFallback::returning("酷狗音乐", None)]);
    let request = |source, title: &str| FallbackRequest {
        source,
        title: title.to_owned(),
        artist: String::new(),
        album: None,
        duration_ms: 0,
        word_lyrics: false,
    };

    let primary = manager
        .fetch_fallback(request(LyricsSourceKind::Netease, "x"), &CancelToken::new())
        .await
        .unwrap_err();
    assert_eq!(primary.kind, "primarySource");

    let no_file = manager
        .fetch_fallback(request(LyricsSourceKind::Kugou, "  "), &CancelToken::new())
        .await
        .unwrap_err();
    assert_eq!(no_file.kind, "noFile");

    // LRCLIB 没有注册 → 不可用。
    let missing = manager
        .fetch_fallback(request(LyricsSourceKind::Lrclib, "x"), &CancelToken::new())
        .await
        .unwrap_err();
    assert_eq!(missing.kind, "failed");
    assert_eq!(missing.message, "LRCLIB（仅歌词）不可用。");
}

#[tokio::test]
async fn a_fallback_candidate_without_usable_lyrics_says_so() {
    let online = StubOnline::with(vec![], None);
    let empty = StubFallback::returning(
        "LRCLIB",
        Some(line_candidate(
            "LRCLIB",
            "冷门曲子",
            "同人歌手",
            "",
            200_000,
            "  ",
        )),
    );
    let manager = manager(&online, vec![empty]);

    let found = manager
        .fetch_fallback(
            FallbackRequest {
                source: LyricsSourceKind::Lrclib,
                title: "冷门曲子".to_owned(),
                artist: "同人歌手".to_owned(),
                album: None,
                duration_ms: 200_000,
                word_lyrics: false,
            },
            &CancelToken::new(),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(found.lyrics, None);
    assert_eq!(found.status_text, "LRCLIB没有返回可用歌词。");
}

#[tokio::test]
async fn failing_fallback_sources_report_their_error() {
    let online = StubOnline::with(vec![], None);
    let manager = manager(&online, vec![StubFallback::failing("酷狗音乐")]);

    let error = manager
        .fetch_fallback(
            FallbackRequest {
                source: LyricsSourceKind::Kugou,
                title: "x".to_owned(),
                artist: String::new(),
                album: None,
                duration_ms: 0,
                word_lyrics: false,
            },
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, "failed");
    assert_eq!(error.message, "模拟词源故障");
}

#[test]
fn the_threshold_view_reads_the_current_settings_and_clamps() {
    let online = StubOnline::with(vec![], None);
    let shared = Arc::new(std::sync::Mutex::new(0.72_f64));
    let source = Arc::clone(&shared);
    let manager = OnlineManager::new(
        online,
        vec![],
        Arc::new(move || {
            let mut settings = AppSettings::new("D:\\x");
            settings.match_threshold = *source.lock().unwrap();
            settings
        }),
    );

    let view = manager.threshold_view();
    assert_eq!(view.threshold, 0.72);
    assert_eq!(view.percent, "72");
    assert_eq!((view.min, view.max, view.default), (0.30, 1.00, 0.60));

    *shared.lock().unwrap() = 0.05;
    assert_eq!(manager.threshold_view().percent, "30");
    *shared.lock().unwrap() = 7.0;
    assert_eq!(manager.threshold_view().percent, "100");

    let value = serde_json::to_value(manager.threshold_view()).unwrap();
    assert_eq!(value["threshold"], 1.0);
}

#[test]
fn requests_deserialize_from_camel_case_json() {
    let search: SearchRequest = serde_json::from_str(
        r#"{"source":"QQMusic","query":"晴天","fileContext":{"title":"晴天","durationMs":269000,"filePath":"D:\\a.mp3"}}"#,
    )
    .unwrap();
    assert_eq!(search.source, LyricsSourceKind::QqMusic);
    let file = search.file_context.unwrap();
    assert_eq!(file.duration_ms, 269_000);
    assert_eq!(file.artist, None);

    let load: LoadRequest =
        serde_json::from_str(r#"{"source":"Netease","id":"1","wordLyrics":true}"#).unwrap();
    assert!(load.word_lyrics && load.song.is_none());

    let fallback: FallbackRequest =
        serde_json::from_str(r#"{"source":"Lrclib","title":"x","durationMs":5}"#).unwrap();
    assert_eq!(fallback.artist, "");
    assert_eq!(fallback.duration_ms, 5);

    // 搜索结果的视图原样传回时，派生字段被忽略。
    let song_json = serde_json::to_value(crate::online::view::OnlineSongView::from(&song(
        QqMusic, "m", "t", "a", "b", 1000,
    )))
    .unwrap();
    assert_eq!(song_json["sourceId"], "m");
    assert_eq!(song_json["source"], "QQMusic");
    assert_eq!(song_json["sourceLabel"], "QQ 音乐");
    let back: OnlineSongResult = serde_json::from_value(song_json).unwrap();
    assert_eq!(back, song(QqMusic, "m", "t", "a", "b", 1000));
}
