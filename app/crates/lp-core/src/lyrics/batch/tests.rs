//! 批量标签 / 封面 / 歌词匹配的单测：C# `BatchLyricsServiceTests` 全部移植，另加来源次序、多变体搜索的成本控制、
//! 覆盖 / 补缺规则、取消、文件名推断、扫描、任务编排与在线补全挂钩。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::download::ncm_import::describe_match;
use crate::logs::LogHub;
use crate::online::OnlineMusicSource::{Netease, QqMusic};
use crate::online::test_support::{
    StubFallback, StubOnline, StubTags, line_candidate, song, tag_info,
};
use crate::test_util::TempDir;

fn file(temp: &TempDir, name: &str) -> PathBuf {
    let path = temp.path().join(name);
    std::fs::write(&path, [0u8]).unwrap();
    path
}

fn service(
    tags: &Arc<StubTags>,
    online: &Arc<StubOnline>,
    fallback: Vec<Arc<dyn LyricsSource>>,
) -> BatchLyricsService {
    BatchLyricsService::new(
        Arc::clone(tags) as Arc<dyn AudioTagService>,
        Arc::clone(online) as Arc<dyn OnlineMusicSearch>,
        fallback,
    )
}

fn overwrite() -> BatchLyricsOptions {
    BatchLyricsOptions {
        overwrite_existing: true,
        ..BatchLyricsOptions::default()
    }
}

fn sunny(duration: i64) -> OnlineSongResult {
    song(Netease, "186016", "晴天", "周杰伦", "叶惠美", duration)
}

fn settings() -> AppSettings {
    AppSettings::new("D:\\x")
}

// ---- 移植自 C# BatchLyricsServiceTests ----

#[tokio::test]
async fn matches_split_title_and_multiple_artists() {
    let temp = TempDir::new();
    let path = file(&temp, "split.mp3");
    let tags = StubTags::with(tag_info("晴天 / 叶惠美", "周杰伦/杨瑞代", 269_000));
    let online = StubOnline::with(vec![sunny(269_500)], Some("[00:01.00]晴天"));

    let result = service(&tags, &online, vec![])
        .fetch_for_file(&path, &overwrite(), &CancelToken::new())
        .await
        .unwrap();

    assert!(result.success, "{}", result.message);
    assert_eq!(result.matched_title.as_deref(), Some("晴天"));
    assert_eq!(result.matched_artist.as_deref(), Some("周杰伦"));
    assert!(result.message.contains("标签"));
    let written = tags.written().unwrap();
    assert_eq!(written.title.as_deref(), Some("晴天"));
    assert!(written.artists.contains(&"周杰伦".to_owned()));
}

#[tokio::test]
async fn rejects_an_unrelated_candidate() {
    let temp = TempDir::new();
    let path = file(&temp, "unrelated.mp3");
    let tags = StubTags::with(tag_info("完全无关的歌曲", "陌生歌手", 200_000));
    let online = StubOnline::with(vec![sunny(269_500)], Some("[00:01.00]晴天"));

    let result = service(&tags, &online, vec![])
        .fetch_for_file(&path, &BatchLyricsOptions::default(), &CancelToken::new())
        .await
        .unwrap();

    assert!(!result.success);
    assert!(!result.skipped);
    assert!(result.message.contains("未找到匹配度足够的在线歌曲"));
    assert!(tags.written().is_none());
}

#[tokio::test]
async fn writes_tags_and_lyrics_for_an_exact_match() {
    let temp = TempDir::new();
    let path = file(&temp, "exact.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(vec![sunny(269_000)], Some("[00:01.00]晴天"));

    let result = service(&tags, &online, vec![])
        .fetch_for_file(&path, &overwrite(), &CancelToken::new())
        .await
        .unwrap();

    assert!(result.success, "{}", result.message);
    assert!(result.message.contains("标签"));
    assert!(result.message.contains("歌词"));
    assert_eq!(result.message, "已写入标签、逐行歌词");
    let written = tags.written().unwrap();
    assert!(
        written
            .plain_lyrics
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty())
    );
    assert_eq!(written.word_lyrics, None);
}

#[tokio::test]
async fn uses_fallback_lyrics_when_online_has_no_lyrics() {
    let temp = TempDir::new();
    let path = file(&temp, "fallback-lyrics.mp3");
    let mut info = tag_info("Lemon", "米津玄師", 255_000);
    info.album = Some("Lemon".to_owned());
    info.has_cover = true;
    let tags = StubTags::with(info);
    let online = StubOnline::with(
        vec![song(
            Netease,
            "2051316826",
            "Lemon",
            "米津玄師",
            "Lemon",
            255_000,
        )],
        None,
    );
    let fallback = StubFallback::returning(
        "LRCLIB",
        Some(line_candidate(
            "LRCLIB",
            "Lemon",
            "米津玄師",
            "Lemon",
            255_000,
            "[00:01.00]夢ならば",
        )),
    );

    let result = service(&tags, &online, vec![fallback.clone()])
        .fetch_for_file(&path, &BatchLyricsOptions::default(), &CancelToken::new())
        .await
        .unwrap();

    assert!(result.success, "{}", result.message);
    assert!(result.message.contains("逐行歌词"));
    assert!(result.message.contains("歌词来源：LRCLIB"));
    assert_eq!(
        tags.written().unwrap().plain_lyrics.as_deref(),
        Some("[00:01.00]夢ならば")
    );
    assert!(!fallback.searched_queries().is_empty());
}

#[tokio::test]
async fn uses_fallback_source_when_online_search_misses() {
    let temp = TempDir::new();
    let path = file(&temp, "fallback-only.mp3");
    let tags = StubTags::with(tag_info("冷门曲子", "同人歌手", 200_000));
    let online = StubOnline::with(vec![sunny(269_500)], None);
    let broken = StubFallback::failing("故障源");
    let word = LyricsSourceCandidate {
        word_lyrics: Some("[00:01.00]<00:01.00>冷<00:02.00>门".to_owned()),
        line_lyrics: None,
        ..line_candidate("酷狗音乐", "冷门曲子", "同人歌手", "同人专辑", 200_500, "")
    };
    let fallback = StubFallback::returning("酷狗音乐", Some(word));

    let options = BatchLyricsOptions {
        prefer_word_lyrics: true,
        ..BatchLyricsOptions::default()
    };
    let result = service(&tags, &online, vec![broken, fallback])
        .fetch_for_file(&path, &options, &CancelToken::new())
        .await
        .unwrap();

    assert!(result.success, "{}", result.message);
    assert_eq!(result.source_label.as_deref(), Some("酷狗音乐"));
    assert_eq!(result.lyrics_format.as_deref(), Some("逐字歌词"));
    assert!(result.message.contains("逐字歌词"));
    let written = tags.written().unwrap();
    assert_eq!(written.title, None);
    assert_eq!(written.album.as_deref(), Some("同人专辑"));
    assert_eq!(
        written.word_lyrics.as_deref(),
        Some("[00:01.00]<00:01.00>冷<00:02.00>门")
    );
    assert_eq!(written.plain_lyrics, None);
}

#[tokio::test]
async fn does_not_consult_fallback_when_online_lyrics_exist() {
    let temp = TempDir::new();
    let path = file(&temp, "online-first.mp3");
    let mut info = tag_info("晴天", "周杰伦", 269_000);
    info.album = Some("叶惠美".to_owned());
    let tags = StubTags::with(info);
    let online = StubOnline::with(vec![sunny(269_500)], Some("[00:01.00]晴天"));
    let fallback = StubFallback::returning(
        "LRCLIB",
        Some(line_candidate(
            "LRCLIB",
            "晴天",
            "周杰伦",
            "叶惠美",
            269_000,
            "[00:01.00]别的歌词",
        )),
    );

    let result = service(&tags, &online, vec![fallback.clone()])
        .fetch_for_file(&path, &overwrite(), &CancelToken::new())
        .await
        .unwrap();

    assert!(result.success, "{}", result.message);
    assert!(fallback.searched_queries().is_empty());
    assert_eq!(
        tags.written().unwrap().plain_lyrics.as_deref(),
        Some("[00:01.00]晴天")
    );
}

type FanMade = (
    PathBuf,
    Arc<StubTags>,
    Arc<StubOnline>,
    Arc<StubFallback>,
    Arc<StubFallback>,
);

fn fan_made_setup(temp: &TempDir) -> FanMade {
    let path = file(temp, "filtered.mp3");
    let tags = StubTags::with(tag_info("冷门曲子", "同人歌手", 200_000));
    let online = StubOnline::with(
        vec![song(
            Netease,
            "1",
            "冷门曲子",
            "同人歌手",
            "同人专辑",
            200_000,
        )],
        None,
    );
    let kugou_candidate = LyricsSourceCandidate {
        word_lyrics: Some("[00:01.00]<00:01.00>冷".to_owned()),
        line_lyrics: None,
        ..line_candidate("酷狗音乐", "冷门曲子", "同人歌手", "同人专辑", 200_000, "")
    };
    let kugou = StubFallback::returning("酷狗音乐", Some(kugou_candidate));
    let lrclib = StubFallback::returning(
        "LRCLIB",
        Some(line_candidate(
            "LRCLIB",
            "冷门曲子",
            "同人歌手",
            "同人专辑",
            200_000,
            "[00:01.00]冷",
        )),
    );
    (path, tags, online, kugou, lrclib)
}

#[tokio::test]
async fn honours_the_fallback_source_filter() {
    let temp = TempDir::new();
    let (path, tags, online, kugou, lrclib) = fan_made_setup(&temp);
    let service = service(&tags, &online, vec![kugou.clone(), lrclib.clone()]);
    // 设置里关掉酷狗音乐后，兜底应直接跳到 LRCLIB。
    service.set_fallback_filter(Some(Arc::new(|source: &dyn LyricsSource| {
        source.label() != "酷狗音乐"
    })));

    let result = service
        .fetch_for_file(&path, &BatchLyricsOptions::default(), &CancelToken::new())
        .await
        .unwrap();

    assert!(result.success, "{}", result.message);
    assert!(result.message.contains("歌词来源：LRCLIB"));
    assert!(kugou.searched_queries().is_empty());
    assert!(!lrclib.searched_queries().is_empty());
    assert_eq!(
        tags.written().unwrap().plain_lyrics.as_deref(),
        Some("[00:01.00]冷")
    );
}

#[tokio::test]
async fn writes_no_lyrics_when_every_fallback_source_is_disabled() {
    let temp = TempDir::new();
    let (path, tags, online, kugou, lrclib) = fan_made_setup(&temp);
    let service = service(&tags, &online, vec![kugou.clone(), lrclib.clone()]);
    service.set_fallback_filter(Some(Arc::new(|_: &dyn LyricsSource| false)));

    let result = service
        .fetch_for_file(&path, &BatchLyricsOptions::default(), &CancelToken::new())
        .await
        .unwrap();

    assert!(result.success, "{}", result.message);
    assert!(result.message.contains("未获取到歌词"));
    assert!(kugou.searched_queries().is_empty());
    assert!(lrclib.searched_queries().is_empty());
    assert_eq!(tags.written().unwrap().plain_lyrics, None);
}

#[tokio::test]
async fn the_fallback_filter_can_change_while_the_service_is_alive() {
    let temp = TempDir::new();
    let (path, tags, online, kugou, lrclib) = fan_made_setup(&temp);
    let service = service(&tags, &online, vec![kugou.clone(), lrclib.clone()]);
    let kugou_enabled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&kugou_enabled);
    service.set_fallback_filter(Some(Arc::new(move |source: &dyn LyricsSource| {
        source.label() != "酷狗音乐" || flag.load(Ordering::SeqCst)
    })));
    let options = BatchLyricsOptions {
        prefer_word_lyrics: true,
        ..overwrite()
    };

    let first = service
        .fetch_for_file(&path, &options, &CancelToken::new())
        .await
        .unwrap();
    assert!(kugou.searched_queries().is_empty());
    assert_eq!(first.source_label.as_deref(), Some("网易云音乐"));
    assert_eq!(first.lyrics_format.as_deref(), Some("逐行歌词"));

    // 打开开关后，下一次调用立刻问酷狗，并优先取它的逐字歌词。
    kugou_enabled.store(true, Ordering::SeqCst);
    let second = service
        .fetch_for_file(&path, &options, &CancelToken::new())
        .await
        .unwrap();
    assert!(!kugou.searched_queries().is_empty());
    assert_eq!(second.lyrics_format.as_deref(), Some("逐字歌词"));
    assert!(
        second.message.contains("歌词来源：酷狗音乐"),
        "{}",
        second.message
    );
}

// ---- 新增 ----

#[tokio::test]
async fn missing_and_unreadable_files_report_normal_failures() {
    let temp = TempDir::new();
    let tags = StubTags::unreadable();
    let online = StubOnline::with(vec![], None);
    let svc = service(&tags, &online, vec![]);

    let missing = svc
        .fetch_for_file(
            &temp.path().join("nope.mp3"),
            &BatchLyricsOptions::default(),
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(missing.message, "文件不存在");
    assert!(!missing.success && !missing.skipped);

    let path = file(&temp, "broken.mp3");
    let unreadable = svc
        .fetch_for_file(&path, &BatchLyricsOptions::default(), &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(unreadable.message, "无法读取音频标签");
    assert!(online.searches().is_empty());
}

#[tokio::test]
async fn a_file_without_title_tag_searches_by_file_name_and_keeps_the_tag_artist() {
    let temp = TempDir::new();
    let path = file(&temp, "01 - 晴天_Live.mp3");
    let tags = StubTags::with(tag_info("", "周杰伦", 0));
    let online = StubOnline::with(vec![], None);

    let result = service(&tags, &online, vec![])
        .fetch_for_file(&path, &BatchLyricsOptions::default(), &CancelToken::new())
        .await
        .unwrap();

    assert!(!result.success);
    let searches = online.searches();
    assert_eq!(searches[0], (Netease, "晴天 Live 周杰伦".to_owned()));

    // 去掉音轨前缀后什么都不剩 → 无法识别。
    let nameless = file(&temp, "01 -.mp3");
    let tags = StubTags::with(tag_info("  ", "", 0));
    let result = service(&tags, &online, vec![])
        .fetch_for_file(
            &nameless,
            &BatchLyricsOptions::default(),
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.message, "无法从标签或文件名识别歌曲名");
}

#[test]
fn file_name_titles_drop_track_prefixes_and_underscores() {
    assert_eq!(title_from_file_name("01 - 晴天"), "晴天");
    assert_eq!(title_from_file_name("12. Lemon"), "Lemon");
    assert_eq!(title_from_file_name("003_夜曲"), "夜曲");
    assert_eq!(title_from_file_name("1984"), "1984");
    assert_eq!(title_from_file_name("1984 Song"), "1984 Song");
    assert_eq!(title_from_file_name("198 Song"), "Song");
    assert_eq!(title_from_file_name("a_b_c"), "a b c");
    assert_eq!(title_from_file_name("  Song  "), "Song");
}

#[tokio::test]
async fn searches_follow_source_priority_and_stop_at_a_confident_match() {
    let temp = TempDir::new();
    let path = file(&temp, "priority.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(vec![sunny(269_000)], Some("[00:01.00]晴天"));
    let svc = service(&tags, &online, vec![]);

    let options = BatchLyricsOptions {
        tag_priority: OnlineSourcePriority::QqMusicFirst,
        lyrics_priority: OnlineSourcePriority::NeteaseOnly,
        ..overwrite()
    };
    let result = svc
        .fetch_for_file(&path, &options, &CancelToken::new())
        .await
        .unwrap();
    assert!(result.success);

    // 标签：QQ 先搜，可信匹配（第一组关键词）后不再换关键词也不再问网易云；歌词：只问网易云。
    let searches = online.searches();
    assert_eq!(searches.len(), 2);
    assert_eq!(searches[0].0, QqMusic);
    assert_eq!(searches[1].0, Netease);
    assert_eq!(online.detail_calls().len(), 2);
}

#[test]
fn source_ordering_covers_all_priorities() {
    assert_eq!(
        order_sources(OnlineSourcePriority::NeteaseFirst),
        vec![Netease, QqMusic]
    );
    assert_eq!(
        order_sources(OnlineSourcePriority::QqMusicFirst),
        vec![QqMusic, Netease]
    );
    assert_eq!(
        order_sources(OnlineSourcePriority::NeteaseOnly),
        vec![Netease]
    );
    assert_eq!(
        order_sources(OnlineSourcePriority::QqMusicOnly),
        vec![QqMusic]
    );
}

#[tokio::test]
async fn at_most_three_query_variants_are_tried_per_source() {
    let temp = TempDir::new();
    let path = file(&temp, "variants.mp3");
    let tags = StubTags::with(tag_info("Song (Live) feat. X / Y", "Artist", 200_000));
    let online = StubOnline::with(vec![], None);
    let options = BatchLyricsOptions {
        fetch_lyrics: false,
        ..BatchLyricsOptions::default()
    };

    let result = service(&tags, &online, vec![])
        .fetch_for_file(&path, &options, &CancelToken::new())
        .await
        .unwrap();

    assert!(!result.success);
    let count = |wanted| {
        online
            .searches()
            .into_iter()
            .filter(|(source, _)| *source == wanted)
            .count()
    };
    assert_eq!(count(Netease), 3);
    assert_eq!(count(QqMusic), 3);
}

#[tokio::test]
async fn failing_searches_are_skipped_and_the_next_query_is_tried() {
    let temp = TempDir::new();
    let path = file(&temp, "flaky.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(vec![], Some("[00:01.00]晴天"));
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&attempts);
    online.on_search(move |source, _| {
        if source == Netease && counter.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(OnlineError::failed("限流 406"));
        }
        Ok(vec![sunny(269_000)])
    });

    let result = service(&tags, &online, vec![])
        .fetch_for_file(&path, &overwrite(), &CancelToken::new())
        .await
        .unwrap();

    assert!(result.success, "{}", result.message);
    assert_eq!(result.matched_title.as_deref(), Some("晴天"));
}

#[tokio::test]
async fn a_source_whose_details_fail_is_skipped_for_the_next_source() {
    let temp = TempDir::new();
    let path = file(&temp, "details.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(vec![sunny(269_000)], None);
    online.on_details(|candidate| match candidate.source {
        Netease => Err(OnlineError::failed("详情失败")),
        QqMusic => {
            let mut details = OnlineSongDetails::without_extras(candidate.clone());
            details.line_lyrics = Some("[00:01.00]来自 QQ".to_owned());
            Ok(Some(details))
        }
    });
    online.on_search(|source, _| Ok(vec![song(source, "1", "晴天", "周杰伦", "叶惠美", 269_000)]));

    let result = service(&tags, &online, vec![])
        .fetch_for_file(&path, &overwrite(), &CancelToken::new())
        .await
        .unwrap();

    assert!(result.success, "{}", result.message);
    assert_eq!(result.source_label.as_deref(), Some("QQ 音乐"));
    assert_eq!(
        tags.written().unwrap().plain_lyrics.as_deref(),
        Some("[00:01.00]来自 QQ")
    );
}

#[tokio::test]
async fn lyrics_are_required_only_for_the_lyrics_lookup_not_for_tags() {
    let temp = TempDir::new();
    let path = file(&temp, "tags-only.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(vec![sunny(269_000)], None);

    let options = BatchLyricsOptions {
        fetch_lyrics: false,
        ..overwrite()
    };
    let result = service(&tags, &online, vec![])
        .fetch_for_file(&path, &options, &CancelToken::new())
        .await
        .unwrap();

    assert!(result.success);
    assert_eq!(result.message, "已写入标签");
    assert_eq!(online.detail_calls().len(), 1);
    assert_eq!(tags.written().unwrap().plain_lyrics, None);

    // 要歌词但哪里都没有：标签照写，并提示没有歌词。
    let with_lyrics = service(&tags, &online, vec![])
        .fetch_for_file(&path, &overwrite(), &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(with_lyrics.message, "已写入标签（未获取到歌词）");
}

#[tokio::test]
async fn existing_tags_are_kept_without_overwrite_and_complete_files_are_skipped() {
    let temp = TempDir::new();
    let path = file(&temp, "complete.mp3");
    let mut info = tag_info("晴天", "周杰伦", 269_000);
    info.album = Some("叶惠美".to_owned());
    info.year = Some(2003);
    info.track_number = Some(3);
    info.disc_number = Some(1);
    info.genre = Some("流行".to_owned());
    info.composer = Some("周杰伦".to_owned());
    info.has_cover = true;
    info.lyrics = Some("[00:01.00]已有".to_owned());
    let tags = StubTags::with(info);
    let online = StubOnline::with(vec![sunny(269_000)], Some("[00:01.00]晴天"));

    let result = service(&tags, &online, vec![])
        .fetch_for_file(&path, &BatchLyricsOptions::default(), &CancelToken::new())
        .await
        .unwrap();

    assert!(!result.success);
    assert!(result.skipped);
    assert_eq!(result.message, "标签、封面与歌词已完整，跳过");
    assert_eq!(result.matched_title.as_deref(), Some("晴天"));
    assert_eq!(result.lyrics_format.as_deref(), Some(""));
    assert_eq!(tags.write_count(), 0);
}

#[tokio::test]
async fn only_missing_fields_are_filled_without_overwrite() {
    let temp = TempDir::new();
    let path = file(&temp, "partial.mp3");
    let mut info = tag_info("晴天", "周杰伦", 269_000);
    info.year = Some(1999);
    let tags = StubTags::with(info);
    let mut found = sunny(269_000);
    found.genre = Some("流行".to_owned());
    found.composer = Some("周杰伦".to_owned());
    found.cover_url = Some("https://cover".to_owned());
    let online = StubOnline::with(vec![found], None);
    online.on_details(|candidate| {
        let mut details = OnlineSongDetails::without_extras(candidate.clone());
        details.line_lyrics = Some("[00:01.00]晴天".to_owned());
        details.cover_data = Some(vec![1, 2, 3]);
        Ok(Some(details))
    });

    let result = service(&tags, &online, vec![])
        .fetch_for_file(&path, &BatchLyricsOptions::default(), &CancelToken::new())
        .await
        .unwrap();

    assert!(result.success);
    assert_eq!(result.message, "已写入标签、封面、逐行歌词");
    let written = tags.written().unwrap();
    // 已有的标题 / 歌手 / 年份不动。
    assert_eq!(written.title, None);
    assert!(written.artists.is_empty());
    assert_eq!(written.year, None);
    // 缺的补上。
    assert_eq!(written.album.as_deref(), Some("叶惠美"));
    assert_eq!(written.track_number, Some(3));
    assert_eq!(written.genre.as_deref(), Some("流行"));
    assert_eq!(written.composer.as_deref(), Some("周杰伦"));
    assert_eq!(written.cover_data.as_deref(), Some(&[1u8, 2, 3][..]));
    assert_eq!(written.plain_lyrics.as_deref(), Some("[00:01.00]晴天"));
}

#[tokio::test]
async fn overwrite_replaces_everything_and_splits_artists_on_slash_semicolon_and_enumeration_comma()
{
    let temp = TempDir::new();
    let path = file(&temp, "overwrite.mp3");
    let mut info = tag_info("晴天", "周杰伦", 269_000);
    info.year = Some(1999);
    info.lyrics = Some("[00:01.00]旧".to_owned());
    info.has_cover = true;
    let tags = StubTags::with(info);
    let found = song(
        Netease,
        "1",
        "晴天",
        "周杰伦 / 杨瑞代；陈 & 林、黄",
        "叶惠美",
        269_000,
    );
    let online = StubOnline::with(vec![found], Some("[00:01.00]晴天"));

    let result = service(&tags, &online, vec![])
        .fetch_for_file(&path, &overwrite(), &CancelToken::new())
        .await
        .unwrap();

    assert!(result.success, "{}", result.message);
    let written = tags.written().unwrap();
    assert_eq!(written.title.as_deref(), Some("晴天"));
    // C# 这里只拆 `/ ; ； 、`（不拆 `&` 和逗号）。
    assert_eq!(written.artists, vec!["周杰伦", "杨瑞代", "陈 & 林", "黄"]);
    assert_eq!(written.year, Some(2003));
    assert_eq!(written.plain_lyrics.as_deref(), Some("[00:01.00]晴天"));
}

fn bilingual_online() -> Arc<StubOnline> {
    let online = StubOnline::with(vec![sunny(269_000)], None);
    online.on_details(|candidate| {
        let mut details = OnlineSongDetails::without_extras(candidate.clone());
        details.line_lyrics = Some("[00:01.00]原文".to_owned());
        details.word_lyrics = Some("[00:01.00]<00:01.00>原<00:01.50>文".to_owned());
        details.bilingual_line_lyrics = Some("[00:01.00]原文\n[00:01.00]译文".to_owned());
        details.bilingual_word_lyrics =
            Some("[00:01.00]<00:01.00>原<00:01.50>文\n[00:01.00]译文".to_owned());
        Ok(Some(details))
    });
    online
}

#[tokio::test]
async fn word_lyrics_are_preferred_and_labelled_with_translation() {
    let temp = TempDir::new();
    let path = file(&temp, "word.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = bilingual_online();
    let svc = service(&tags, &online, vec![]);

    let word = BatchLyricsOptions {
        prefer_word_lyrics: true,
        ..overwrite()
    };
    let result = svc
        .fetch_for_file(&path, &word, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(result.lyrics_format.as_deref(), Some("双语逐字歌词"));
    let written = tags.written().unwrap();
    assert_eq!(
        written.word_lyrics.as_deref(),
        Some("[00:01.00]<00:01.00>原<00:01.50>文\n[00:01.00]译文")
    );
    assert_eq!(written.plain_lyrics, None);

    let line = svc
        .fetch_for_file(&path, &overwrite(), &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(line.lyrics_format.as_deref(), Some("双语逐行歌词"));
    assert_eq!(
        tags.written().unwrap().plain_lyrics.as_deref(),
        Some("[00:01.00]原文\n[00:01.00]译文")
    );
}

#[tokio::test]
async fn word_preference_falls_back_to_line_lyrics_when_the_source_has_no_word_data() {
    let temp = TempDir::new();
    let path = file(&temp, "fallback-to-line.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(vec![sunny(269_000)], Some("[00:01.00]晴天"));

    let options = BatchLyricsOptions {
        prefer_word_lyrics: true,
        ..overwrite()
    };
    let result = service(&tags, &online, vec![])
        .fetch_for_file(&path, &options, &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(result.lyrics_format.as_deref(), Some("逐行歌词"));
    assert!(tags.written().unwrap().plain_lyrics.is_some());
}

#[tokio::test]
async fn the_threshold_is_clamped_into_the_supported_range() {
    let temp = TempDir::new();
    let path = file(&temp, "threshold.mp3");
    // 标题相同、歌手完全不同 → 总分 0.5625：默认 0.6 拒绝，阈值被夹到下限 0.3 时通过。
    let tags = StubTags::with(tag_info("晴天", "AAA", 269_000));
    let online = StubOnline::with(vec![sunny(269_000)], Some("[00:01.00]晴天"));
    let svc = service(&tags, &online, vec![]);

    let strict = svc
        .fetch_for_file(&path, &overwrite(), &CancelToken::new())
        .await
        .unwrap();
    assert!(!strict.success);

    let lenient = BatchLyricsOptions {
        min_score: 0.01,
        ..overwrite()
    };
    let result = svc
        .fetch_for_file(&path, &lenient, &CancelToken::new())
        .await
        .unwrap();
    assert!(result.success, "{}", result.message);
}

#[tokio::test]
async fn instrumental_candidates_never_win() {
    let temp = TempDir::new();
    let path = file(&temp, "instrumental.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(
        vec![song(
            Netease,
            "9",
            "晴天 (伴奏)",
            "周杰伦",
            "叶惠美",
            269_000,
        )],
        Some("[00:01.00]x"),
    );

    let result = service(&tags, &online, vec![])
        .fetch_for_file(&path, &overwrite(), &CancelToken::new())
        .await
        .unwrap();

    assert!(!result.success);
    assert_eq!(tags.write_count(), 0);
}

#[tokio::test]
async fn cancellation_is_reported_and_nothing_is_written() {
    let temp = TempDir::new();
    let path = file(&temp, "cancel.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(vec![sunny(269_000)], Some("[00:01.00]晴天"));
    let cancel = CancelToken::new();
    cancel.cancel();

    let error = service(&tags, &online, vec![])
        .fetch_for_file(&path, &overwrite(), &cancel)
        .await
        .unwrap_err();

    assert_eq!(error, BatchError::Cancelled);
    assert_eq!(tags.write_count(), 0);
    assert!(online.searches().is_empty());
}

#[tokio::test]
async fn an_online_cancellation_error_propagates_instead_of_being_swallowed() {
    let temp = TempDir::new();
    let path = file(&temp, "cancel-online.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(vec![], None);
    online.on_search(|_, _| Err(OnlineError::Cancelled));

    let error = service(&tags, &online, vec![])
        .fetch_for_file(&path, &overwrite(), &CancelToken::new())
        .await
        .unwrap_err();

    assert_eq!(error, BatchError::Cancelled);
}

#[tokio::test]
async fn tag_write_failures_are_reported_as_errors_with_the_chinese_reason() {
    let temp = TempDir::new();
    let path = file(&temp, "write-fails.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    tags.fail_writes();
    let online = StubOnline::with(vec![sunny(269_000)], Some("[00:01.00]晴天"));

    let error = service(&tags, &online, vec![])
        .fetch_for_file(&path, &overwrite(), &CancelToken::new())
        .await
        .unwrap_err();

    match error {
        BatchError::Failed(reason) => assert!(reason.contains("模拟写入失败"), "{reason}"),
        other => panic!("意外的结果：{other:?}"),
    }
}

#[test]
fn lyrics_format_labels() {
    assert_eq!(format_label(true, true), "双语逐字歌词");
    assert_eq!(format_label(true, false), "逐字歌词");
    assert_eq!(format_label(false, true), "双语逐行歌词");
    assert_eq!(format_label(false, false), "逐行歌词");
}

// ---- 扫描 ----

#[test]
fn scanning_filters_extensions_recurses_on_request_and_sorts_ignoring_case() {
    let temp = TempDir::new();
    for name in [
        "b.MP3", "a.flac", "c.txt", "d.m4a", "e.ogg", "f.wav", "g.aac", "h.wma", "i.opus", "j.mp4",
        "k.ncm",
    ] {
        std::fs::write(temp.path().join(name), [0u8]).unwrap();
    }
    std::fs::create_dir_all(temp.path().join("sub")).unwrap();
    std::fs::write(temp.path().join("sub").join("A2.mp3"), [0u8]).unwrap();
    let root = temp.path().to_string_lossy().into_owned();

    let flat: Vec<String> = BatchLyricsService::scan_audio_files(&root, false)
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        flat,
        vec![
            "a.flac", "b.MP3", "d.m4a", "e.ogg", "f.wav", "g.aac", "h.wma", "i.opus"
        ]
    );

    let deep = BatchLyricsService::scan_audio_files(&root, true);
    assert_eq!(deep.len(), 9);
    assert!(deep.iter().any(|p| p.ends_with("A2.mp3")));

    assert!(BatchLyricsService::scan_audio_files("", true).is_empty());
    assert!(BatchLyricsService::scan_audio_files("   ", true).is_empty());
    assert!(BatchLyricsService::scan_audio_files(&format!("{root}/missing"), true).is_empty());
}

// ---- 选项与在线补全挂钩 ----

#[test]
fn auto_enrich_takes_tags_from_netease_first_and_word_lyrics_from_qq_first() {
    let options = BatchLyricsOptions::auto_enrich();
    assert_eq!(options.tag_priority, OnlineSourcePriority::NeteaseFirst);
    assert_eq!(options.lyrics_priority, OnlineSourcePriority::QqMusicFirst);
    assert!(options.overwrite_existing && options.fetch_lyrics && options.prefer_word_lyrics);
    assert!(options.write_cover && options.keep_larger_cover);
    assert_eq!(options.min_score, 0.60);

    let defaults = BatchLyricsOptions::default();
    assert_eq!(defaults.tag_priority, OnlineSourcePriority::NeteaseFirst);
    assert!(!defaults.overwrite_existing && defaults.fetch_lyrics && !defaults.prefer_word_lyrics);
    assert!(defaults.write_cover && !defaults.keep_larger_cover);
}

#[test]
fn auto_enrich_reads_threshold_and_lyrics_mode_from_settings() {
    let mut current = settings();
    current.match_threshold = 0.75;
    current.lyrics_mode = LyricsWriteMode::None;

    let options = BatchLyricsOptions::auto_enrich_with(&current);
    assert_eq!(options.min_score, 0.75);
    assert!(!options.fetch_lyrics);
    assert!(options.overwrite_existing && options.prefer_word_lyrics);

    current.lyrics_mode = LyricsWriteMode::Word;
    assert!(BatchLyricsOptions::auto_enrich_with(&current).fetch_lyrics);

    assert!(options.write_cover);
    current.write_cover = false;
    assert!(!BatchLyricsOptions::auto_enrich_with(&current).write_cover);
}

type SharedSettings = Arc<std::sync::Mutex<AppSettings>>;

fn enricher(
    tags: &Arc<StubTags>,
    online: &Arc<StubOnline>,
    initial: AppSettings,
) -> (BatchEnricher, SharedSettings) {
    let shared: SharedSettings = Arc::new(std::sync::Mutex::new(initial));
    let provider_source = Arc::clone(&shared);
    let provider: SettingsProvider = Arc::new(move || provider_source.lock().unwrap().clone());
    (
        BatchEnricher::new(Arc::new(service(tags, online, vec![])), provider),
        shared,
    )
}

#[tokio::test]
async fn the_enricher_produces_the_csharp_messages_for_ncm_import_and_downloads() {
    let temp = TempDir::new();
    let path = file(&temp, "enrich.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(
        vec![song(QqMusic, "003", "晴天", "周杰伦", "叶惠美", 269_000)],
        None,
    );
    online.on_details(|candidate| {
        let mut details = OnlineSongDetails::without_extras(candidate.clone());
        details.line_lyrics = Some("[00:01.00]晴天".to_owned());
        details.word_lyrics = Some("[00:01.00]<00:01.00>晴<00:02.00>天".to_owned());
        details.bilingual_line_lyrics = Some("[00:01.00]晴天\n[00:01.00]Sunny".to_owned());
        details.bilingual_word_lyrics =
            Some("[00:01.00]<00:01.00>晴<00:02.00>天\n[00:01.00]Sunny".to_owned());
        Ok(Some(details))
    });
    let (enricher, _) = enricher(&tags, &online, settings());

    let outcome = enricher.enrich(&path, &CancelToken::new()).await.unwrap();

    assert!(outcome.success && !outcome.skipped);
    assert_eq!(outcome.message, "已写入标签、双语逐字歌词");
    assert_eq!(outcome.source_label.as_deref(), Some("QQ 音乐"));
    // NCM 导入窗口里显示的说明（C# DescribeMatch）。
    assert_eq!(
        describe_match(&outcome),
        "匹配「晴天 - 周杰伦」：已写入标签、双语逐字歌词（QQ 音乐）"
    );
    // 自动补全的标签先搜网易云（假搜索对任何来源都返回这首 QQ 歌，所以来源名仍是 QQ 音乐）。
    assert_eq!(online.searches()[0].0, Netease);
    assert!(tags.written().unwrap().word_lyrics.is_some());
}

#[tokio::test]
async fn the_enricher_reads_the_current_settings_on_every_call() {
    let temp = TempDir::new();
    let path = file(&temp, "settings.mp3");
    let tags = StubTags::with(tag_info("晴天", "AAA", 269_000));
    let online = StubOnline::with(vec![sunny(269_000)], Some("[00:01.00]晴天"));
    let (enricher, shared) = enricher(&tags, &online, settings());

    // 默认阈值 0.6：歌手不符 → 没找到。
    let miss = enricher.enrich(&path, &CancelToken::new()).await.unwrap();
    assert!(!miss.success && !miss.skipped);
    assert_eq!(
        describe_match(&miss),
        "在线匹配失败：未找到匹配度足够的在线歌曲"
    );

    // 把阈值调到下限：同一个补全器立刻命中；歌词写入方式「不写」时只补标签与封面。
    {
        let mut current = shared.lock().unwrap();
        current.match_threshold = 0.30;
        current.lyrics_mode = LyricsWriteMode::None;
    }
    let hit = enricher.enrich(&path, &CancelToken::new()).await.unwrap();
    assert!(hit.success, "{}", hit.message);
    assert_eq!(hit.message, "已写入标签");
    assert_eq!(tags.written().unwrap().plain_lyrics, None);
}

#[tokio::test]
async fn the_enricher_maps_cancellation_and_write_failures() {
    let temp = TempDir::new();
    let path = file(&temp, "mapping.mp3");
    let online = StubOnline::with(vec![sunny(269_000)], Some("[00:01.00]晴天"));

    // 取消。
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let (cancelling, _) = enricher(&tags, &online, settings());
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        cancelling.enrich(&path, &cancel).await.unwrap_err(),
        EnrichError::Cancelled
    );

    // 标签写入失败 → Failed(中文原因)。
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    tags.fail_writes();
    let (failing, _) = enricher(&tags, &online, settings());
    match failing
        .enrich(&path, &CancelToken::new())
        .await
        .unwrap_err()
    {
        EnrichError::Failed(reason) => assert!(reason.contains("模拟写入失败")),
        other => panic!("意外的结果：{other:?}"),
    }
}

// ---- 任务编排 ----

fn manager(tags: &Arc<StubTags>, online: &Arc<StubOnline>) -> (BatchManager, Arc<LogHub>) {
    let logs = LogHub::new();
    (
        BatchManager::new(Arc::new(service(tags, online, vec![])), Arc::clone(&logs)),
        logs,
    )
}

type Events = Arc<std::sync::Mutex<Vec<BatchProgressView>>>;
type Sink = Arc<dyn Fn(BatchProgressView) + Send + Sync>;

fn recorder() -> (Events, Sink) {
    let events: Events = Arc::default();
    let sink = Arc::clone(&events);
    (
        events,
        Arc::new(move |event| sink.lock().unwrap().push(event)),
    )
}

fn request(files: &[PathBuf]) -> BatchRequest {
    BatchRequest {
        files: files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        tag_priority: None,
        lyrics_priority: None,
        overwrite_existing: true,
        fetch_lyrics: true,
        prefer_word_lyrics: false,
    }
}

#[tokio::test]
async fn running_a_batch_reports_each_file_and_counts_outcomes() {
    let temp = TempDir::new();
    let ok = file(&temp, "ok.mp3");
    let missing = temp.path().join("missing.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(vec![sunny(269_000)], Some("[00:01.00]晴天"));
    let (manager, logs) = manager(&tags, &online);
    let (events, sink) = recorder();

    let summary = manager
        .run(
            request(&[ok.clone(), missing.clone(), ok.clone()]),
            &settings(),
            sink,
        )
        .await
        .unwrap();

    // 重复路径只处理一次。
    assert_eq!(summary.total, 2);
    assert_eq!(summary.processed, 2);
    assert_eq!(
        (summary.success, summary.skipped, summary.failed),
        (1, 0, 1)
    );
    assert!(!summary.cancelled);
    assert_eq!(summary.results[0].status, "success");
    assert_eq!(summary.results[0].file_name, "ok.mp3");
    assert_eq!(summary.results[1].status, "failed");
    assert_eq!(summary.results[1].message, "文件不存在");

    let events = events.lock().unwrap();
    assert_eq!(events.iter().filter(|e| e.stage == "started").count(), 2);
    let finished: Vec<&BatchProgressView> =
        events.iter().filter(|e| e.stage == "finished").collect();
    assert_eq!(finished.len(), 2);
    assert!(finished.iter().all(|e| e.result.is_some() && e.total == 2));
    assert_eq!(finished.iter().map(|e| e.completed).max(), Some(2));
    assert!(logs.recent().iter().any(|e| {
        e.message
            .contains("批量获取标签完成：成功 1，跳过 0，失败 1")
    }));
}

#[tokio::test]
async fn running_without_files_or_while_busy_is_rejected() {
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(vec![sunny(269_000)], Some("[00:01.00]晴天"));
    let (manager, _) = manager(&tags, &online);
    let (_, sink) = recorder();

    let empty = manager
        .run(request(&[]), &settings(), Arc::clone(&sink))
        .await
        .unwrap_err();
    assert_eq!(empty.kind, "noFiles");

    // 占住任务槽后再启动 → busy。
    let held = manager.slot.try_begin().unwrap();
    assert!(manager.cancel());
    assert!(held.cancel.is_cancelled());
    let temp = TempDir::new();
    let busy = manager
        .run(request(&[file(&temp, "a.mp3")]), &settings(), sink)
        .await
        .unwrap_err();
    assert_eq!(busy.kind, "busy");
    drop(held);
    assert!(!manager.cancel());
}

#[tokio::test]
async fn cancelling_stops_new_files_and_keeps_finished_results() {
    let temp = TempDir::new();
    let files: Vec<PathBuf> = (0..8).map(|i| file(&temp, &format!("{i}.mp3"))).collect();
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(vec![sunny(269_000)], Some("[00:01.00]晴天"));
    online.set_delay(std::time::Duration::from_millis(10));
    let (manager, logs) = manager(&tags, &online);
    let manager = Arc::new(manager);

    // 第一首完成时请求取消。
    let canceller = Arc::clone(&manager);
    let finished = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&finished);
    let sink: Sink = Arc::new(move |event| {
        if event.stage == "finished" && counter.fetch_add(1, Ordering::SeqCst) == 0 {
            canceller.cancel();
        }
    });

    let summary = manager
        .run(request(&files), &settings(), sink)
        .await
        .unwrap();

    assert!(summary.cancelled);
    assert!(summary.processed < summary.total, "{summary:?}");
    assert!(summary.processed >= 1);
    assert!(
        logs.recent()
            .iter()
            .any(|e| e.message.starts_with("批量获取标签已取消"))
    );
}

#[tokio::test]
async fn the_batch_never_runs_more_than_three_files_at_once() {
    let temp = TempDir::new();
    let files: Vec<PathBuf> = (0..9).map(|i| file(&temp, &format!("{i}.mp3"))).collect();
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(vec![], None);
    online.set_delay(std::time::Duration::from_millis(20));
    let (manager, _) = manager(&tags, &online);
    let (_, sink) = recorder();

    manager
        .run(request(&files), &settings(), sink)
        .await
        .unwrap();

    assert_eq!(online.peak(), BATCH_CONCURRENCY);
}

#[tokio::test]
async fn request_priorities_default_to_the_settings_and_can_be_overridden() {
    let temp = TempDir::new();
    let path = file(&temp, "p.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = StubOnline::with(vec![sunny(269_000)], Some("[00:01.00]晴天"));
    let (manager, _) = manager(&tags, &online);
    let (_, sink) = recorder();
    let mut current = settings();
    current.tag_source_priority = OnlineSourcePriority::QqMusicOnly;
    current.lyrics_source_priority = OnlineSourcePriority::QqMusicOnly;

    manager
        .run(
            request(std::slice::from_ref(&path)),
            &current,
            Arc::clone(&sink),
        )
        .await
        .unwrap();
    assert!(
        online
            .searches()
            .iter()
            .all(|(source, _)| *source == QqMusic)
    );

    let mut overridden = request(&[path]);
    overridden.tag_priority = Some(OnlineSourcePriority::NeteaseOnly);
    overridden.lyrics_priority = Some(OnlineSourcePriority::NeteaseOnly);
    let before = online.searches().len();
    manager.run(overridden, &current, sink).await.unwrap();
    assert!(
        online.searches()[before..]
            .iter()
            .all(|(source, _)| *source == Netease)
    );
}

#[tokio::test]
async fn scanning_through_the_manager_reports_missing_directories() {
    let temp = TempDir::new();
    file(&temp, "a.mp3");
    let tags = StubTags::with(tag_info("a", "b", 1));
    let online = StubOnline::with(vec![], None);
    let (manager, _) = manager(&tags, &online);

    let found = manager
        .scan(temp.path().to_string_lossy().into_owned(), true)
        .await;
    assert!(!found.directory_missing);
    assert_eq!(found.files.len(), 1);
    assert_eq!(found.files[0].name, "a.mp3");
    assert_eq!(found.files[0].size, 1);

    let missing = manager
        .scan("D:\\definitely\\missing".to_owned(), false)
        .await;
    assert!(missing.directory_missing);
    assert!(missing.files.is_empty());
}

#[test]
fn request_and_views_use_camel_case_json() {
    let request: BatchRequest = serde_json::from_str(
        r#"{"files":["a.mp3"],"tagPriority":"QQMusicFirst","overwriteExisting":true,"preferWordLyrics":true}"#,
    )
    .unwrap();
    assert_eq!(
        request.tag_priority,
        Some(OnlineSourcePriority::QqMusicFirst)
    );
    assert_eq!(request.lyrics_priority, None);
    assert!(request.fetch_lyrics, "fetchLyrics 默认为真");
    assert!(request.overwrite_existing && request.prefer_word_lyrics);

    let view = BatchResultView::from(&BatchLyricsResult {
        file_path: "D:\\音乐\\a.mp3".to_owned(),
        success: false,
        skipped: true,
        message: "m".to_owned(),
        matched_title: None,
        matched_artist: None,
        source_label: None,
        lyrics_format: Some(String::new()),
    });
    let value = serde_json::to_value(&view).unwrap();
    assert_eq!(value["status"], "skipped");
    assert_eq!(value["fileName"], "a.mp3");
    assert_eq!(value["lyricsFormat"], serde_json::Value::Null);
}

// ---- 自动补全的封面 ----

/// 只有文件头的 PNG（够 `image_dimensions` 认出尺寸）。
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    data.extend_from_slice(b"IHDR");
    data.extend_from_slice(&width.to_be_bytes());
    data.extend_from_slice(&height.to_be_bytes());
    data
}

/// 网易云与 QQ 各有一首「晴天」：网易云只有逐行歌词、封面是 `netease_cover`；QQ 有逐字歌词、封面 300×300。
fn split_sources(netease_cover: Vec<u8>) -> Arc<StubOnline> {
    let online = StubOnline::with(vec![], None);
    online.on_search(|source, _| {
        Ok(vec![match source {
            Netease => song(Netease, "186016", "晴天", "周杰伦", "叶惠美", 269_000),
            _ => song(QqMusic, "003", "晴天", "周杰伦", "叶惠美", 269_000),
        }])
    });
    online.on_details(move |candidate| {
        let mut details = OnlineSongDetails::without_extras(candidate.clone());
        details.line_lyrics = Some("[00:01.00]晴天".to_owned());
        if candidate.source == QqMusic {
            details.word_lyrics = Some("[00:01.00]<00:01.00>晴<00:02.00>天".to_owned());
            details.cover_data = Some(png(300, 300));
        } else {
            details.cover_data = Some(netease_cover.clone());
        }
        Ok(Some(details))
    });
    online
}

#[tokio::test]
async fn auto_enrich_takes_tags_and_cover_from_netease_and_word_lyrics_from_qq() {
    let temp = TempDir::new();
    let path = file(&temp, "split.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = split_sources(png(3000, 3000));
    let (enricher, _) = enricher(&tags, &online, settings());

    let outcome = enricher.enrich(&path, &CancelToken::new()).await.unwrap();

    assert!(outcome.success, "{}", outcome.message);
    assert_eq!(
        outcome.message,
        "已写入标签、封面、逐字歌词（歌词来源：QQ 音乐）"
    );
    assert_eq!(outcome.source_label.as_deref(), Some("网易云音乐"));
    let sources: Vec<_> = online
        .searches()
        .iter()
        .map(|(source, _)| *source)
        .collect();
    assert_eq!(
        sources,
        vec![Netease, QqMusic],
        "标签先搜网易云，歌词先搜 QQ"
    );
    let written = tags.written().unwrap();
    assert_eq!(written.cover_data, Some(png(3000, 3000)));
    assert!(written.word_lyrics.is_some());
}

#[tokio::test]
async fn auto_enrich_only_replaces_an_existing_cover_with_a_larger_one() {
    let cases: [(Option<Vec<u8>>, Vec<u8>, bool); 6] = [
        // 文件里没有封面：再小也写。
        (None, png(300, 300), true),
        (Some(png(1200, 1200)), png(800, 800), false),
        (Some(png(1200, 1200)), png(1200, 1200), false),
        (Some(png(1200, 1200)), png(3000, 3000), true),
        // 认不出尺寸的旧封面按 0 算；认不出尺寸的新封面不换掉已有的。
        (Some(vec![1, 2, 3]), png(500, 500), true),
        (Some(png(500, 500)), vec![1, 2, 3], false),
    ];
    for (existing, offered, replaced) in cases {
        let temp = TempDir::new();
        let path = file(&temp, "cover.mp3");
        let mut info = tag_info("晴天", "周杰伦", 269_000);
        info.has_cover = existing.is_some();
        info.cover_data = existing.clone();
        let tags = StubTags::with(info);
        let online = split_sources(offered.clone());

        let result = service(&tags, &online, vec![])
            .fetch_for_file(
                &path,
                &BatchLyricsOptions::auto_enrich(),
                &CancelToken::new(),
            )
            .await
            .unwrap();

        assert!(result.success, "{}", result.message);
        let written = tags.written().unwrap().cover_data;
        assert_eq!(
            written,
            replaced.then(|| offered.clone()),
            "已有 {existing:?}，新封面 {offered:?}"
        );
        assert_eq!(
            result.message.contains("封面"),
            replaced,
            "{}",
            result.message
        );
    }

    // 批量窗口勾了「覆盖」时照旧直接替换。
    let temp = TempDir::new();
    let path = file(&temp, "window.mp3");
    let mut info = tag_info("晴天", "周杰伦", 269_000);
    info.has_cover = true;
    info.cover_data = Some(png(1200, 1200));
    let tags = StubTags::with(info);
    let result = service(&tags, &split_sources(png(800, 800)), vec![])
        .fetch_for_file(&path, &overwrite(), &CancelToken::new())
        .await
        .unwrap();
    assert!(result.success, "{}", result.message);
    assert_eq!(tags.written().unwrap().cover_data, Some(png(800, 800)));
}

#[tokio::test]
async fn auto_enrich_writes_no_cover_when_writing_covers_is_off() {
    let temp = TempDir::new();
    let path = file(&temp, "no-cover.mp3");
    let tags = StubTags::with(tag_info("晴天", "周杰伦", 269_000));
    let online = split_sources(png(3000, 3000));
    let mut current = settings();
    current.write_cover = false;
    let (enricher, _) = enricher(&tags, &online, current);

    let outcome = enricher.enrich(&path, &CancelToken::new()).await.unwrap();

    assert!(outcome.success, "{}", outcome.message);
    assert_eq!(outcome.message, "已写入标签、逐字歌词（歌词来源：QQ 音乐）");
    assert_eq!(tags.written().unwrap().cover_data, None);
}
