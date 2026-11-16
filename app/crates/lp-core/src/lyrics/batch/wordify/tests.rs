//! 歌词转逐字的测试。样例里的「歌词」都是为测试编的句子。

use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::logs::LogHub;
use crate::online::OnlineMusicSource::{Netease, QqMusic};
use crate::online::test_support::{
    StubFallback, StubOnline, StubTags, line_candidate, song, tag_info,
};
use crate::online::{LyricsSource, OnlineMusicSearch, OnlineSongDetails, OnlineSongResult};
use crate::tags::{AudioTagInfo, AudioTagService};
use crate::test_util::TempDir;

/// 五句互不相同的句子，每句分成前后两个词。
const SENTENCES: [(&str, &str); 5] = [
    ("清晨的风", "吹过窗台"),
    ("我把昨天", "折成纸船"),
    ("让它顺着", "小河漂远"),
    ("直到看见", "海的那边"),
    ("再回头说", "一声再见"),
];

/// 第 `index` 句逐字歌词的一行（起始时间加上 `shift_ms`）。
fn word_line(index: usize, shift_ms: i32) -> String {
    let (head, tail) = SENTENCES[index];
    let start = 1000 + 4000 * index as i32 + shift_ms;
    format!(
        "[{0}]<{0}>{head}<{1}>{tail}<{2}>",
        stamp(start),
        stamp(start + 800),
        stamp(start + 1600)
    )
}

fn stamp(ms: i32) -> String {
    format!("{:02}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

/// 现有的逐行歌词：五句，从第 1 秒起每 4 秒一句。
fn line_lrc() -> String {
    SENTENCES
        .iter()
        .enumerate()
        .map(|(index, (head, tail))| format!("[{}]{head}{tail}", stamp(1000 + 4000 * index as i32)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 同样五句的逐字歌词（程序写出的增强 LRC 的样子）；`shift_ms` 让整首歌的时间整体偏移。
fn word_lrc(shift_ms: i32) -> String {
    (0..SENTENCES.len())
        .map(|index| word_line(index, shift_ms))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---- 判断现有歌词是哪一种 ----

#[test]
fn existing_lyrics_are_classified() {
    assert_eq!(classify(None), Existing::Missing);
    assert_eq!(classify(Some("  \n")), Existing::Missing);
    assert_eq!(classify(Some("只有文字\n没有时间")), Existing::Untimed);
    assert_eq!(classify(Some(&line_lrc())), Existing::Line);
    assert_eq!(classify(Some(&word_lrc(0))), Existing::Word);
}

#[test]
fn bracket_style_word_lyrics_count_as_word_level_but_end_stamps_do_not() {
    // 方括号写法的逐字歌词（LDDC 等工具）：每个字前面是它的起始时间；译文行只在行尾带结束时间。
    let bracket_word = "[00:01.000]清[00:01.300]晨[00:01.600]的[00:01.900]风[00:02.400]\n\
                        [00:01.000]译文一[00:02.400]\n\
                        [00:05.000]我[00:05.300]把[00:05.600]昨[00:05.900]天[00:06.400]\n\
                        [00:05.000]译文二[00:06.400]";
    assert_eq!(classify(Some(bracket_word)), Existing::Word);

    // 只在行尾标了结束时间的仍然是逐行歌词。
    let line_with_end = "[00:01.000]清晨的风吹过窗台[00:04.800]\n\
                         [00:01.000]译文一[00:04.800]\n\
                         [00:05.000]我把昨天折成纸船[00:08.800]\n\
                         [00:05.000]译文二[00:08.800]";
    assert_eq!(classify(Some(line_with_end)), Existing::Line);
}

// ---- 核对与转换 ----

#[test]
fn the_same_version_is_converted_and_uses_the_source_text_as_is() {
    let converted = convert(&line_lrc(), &word_lrc(0)).unwrap();
    assert_eq!(converted.text, word_lrc(0));
    assert_eq!((converted.matched, converted.total), (5, 5));
    assert!(!converted.keeps_translation);
}

#[test]
fn small_time_differences_and_writing_differences_still_match() {
    // 另一个平台的时间轴整体晚 600 毫秒；现有歌词多了空格、全角标点、括号和大小写不同的英文。
    let existing = "[00:01.00]清晨的风 吹过窗台！\n\
                    [00:05.00]我把昨天折成纸船（和声）\n\
                    [00:09.00]让它顺着小河漂远\n\
                    [00:13.00]HELLO World\n\
                    [00:17.00]再回头说一声再见";
    let word = [
        word_line(0, 600),
        word_line(1, 600),
        word_line(2, 600),
        "[00:13.600]<00:13.600>hello <00:14.400>world<00:15.200>".to_owned(),
        word_line(4, 600),
    ]
    .join("\n");
    let converted = convert(existing, &word).unwrap();
    assert_eq!((converted.matched, converted.total), (5, 5));
}

#[test]
fn a_line_that_differs_by_a_character_or_two_still_matches() {
    // 各平台同一句歌词常差一两个字（异体字、符号、送假名）：够长的句子按相似度算，
    // 不到 4 个字的短句只认完全相同。
    assert!(same_line("清晨的风吹过窗台", "清晨的風吹过窗台"));
    assert!(same_line("abcdefghij", "abcdefghxj"));
    assert!(!same_line("清晨的风吹过窗台", "我把昨天折成纸船"));
    assert!(same_line("啦啦啦", "啦啦啦"));
    assert!(!same_line("啦啦啦", "啦啦哦"));
    assert!((similarity("abcd", "abxd") - 0.75).abs() < 1e-9);
    assert_eq!(similarity("", "abc"), 0.0);

    let existing = line_lrc().replace("清晨的风", "清晨的風");
    let converted = convert(&existing, &word_lrc(0)).unwrap();
    assert_eq!((converted.matched, converted.total), (5, 5));
}

#[test]
fn lines_broken_differently_on_the_two_sides_still_match() {
    // 现有歌词把每句写成一行并带译文；逐字歌词把第一句拆成了两行（后半句晚 2 秒才开始）。
    let existing = "[00:01.00]清晨的风吹过窗台\n\
                    [00:01.00]译文一\n\
                    [00:05.00]我把昨天折成纸船\n\
                    [00:05.00]译文二";
    let word = "[00:01.000]<00:01.000>清晨<00:01.400>的风<00:01.800>\n\
                [00:03.000]<00:03.000>吹过<00:03.400>窗台<00:03.800>\n\
                [00:05.000]<00:05.000>我把昨天<00:05.800>折成纸船<00:06.600>";
    let converted = convert(existing, word).unwrap();
    assert_eq!((converted.matched, converted.total), (3, 3));
    // 被拆开的那句，译文只跟第一截。
    assert_eq!(
        converted.text,
        "[00:01.000]<00:01.000>清晨<00:01.400>的风<00:01.800>\n\
         [00:01.000]译文一\n\
         [00:03.000]<00:03.000>吹过<00:03.400>窗台<00:03.800>\n\
         [00:05.000]<00:05.000>我把昨天<00:05.800>折成纸船<00:06.600>\n\
         [00:05.000]译文二"
    );

    // 反过来：现有歌词拆成两行（各有译文），逐字歌词是一行——两句译文接在一起。
    let existing = "[00:01.00]清晨的风\n\
                    [00:01.00]译文前半\n\
                    [00:03.00]吹过窗台\n\
                    [00:03.00]译文后半\n\
                    [00:05.00]我把昨天折成纸船";
    let converted = convert(existing, &[word_line(0, 0), word_line(1, 0)].join("\n")).unwrap();
    assert_eq!((converted.matched, converted.total), (2, 2));
    assert_eq!(
        converted.text,
        format!(
            "{}\n[00:01.000]译文前半 译文后半\n{}",
            word_line(0, 0),
            word_line(1, 0)
        )
    );
}

#[test]
fn end_stamps_on_existing_lines_are_ignored_when_comparing() {
    // `[开始]歌词[结束]` 写法：行尾的结束时间不算歌词文字；译文原样搬过去（连同它的结束时间）。
    let existing = "[00:01.000]清晨的风吹过窗台[00:04.800]\n\
                    [00:01.000]译文一[00:04.800]\n\
                    [00:05.000]我把昨天折成纸船[00:08.800]\n\
                    [00:05.000]译文二[00:08.800]";
    let converted = convert(existing, &[word_line(0, 0), word_line(1, 0)].join("\n")).unwrap();
    assert_eq!((converted.matched, converted.total), (2, 2));
    assert_eq!(
        converted.text,
        format!(
            "{}\n[00:01.000]译文一[00:04.800]\n{}\n[00:05.000]译文二[00:08.800]",
            word_line(0, 0),
            word_line(1, 0)
        )
    );
}

#[test]
fn a_few_lines_that_differ_between_platforms_are_tolerated() {
    // 来源多了两行作词作曲，五句歌词都对得上：5/7 ≥ 0.6。
    let word = format!(
        "[00:00.000]<00:00.000>作词 : <00:00.300>某某<00:00.600>\n\
         [00:00.600]<00:00.600>作曲 : <00:00.800>某某<00:01.000>\n{}",
        word_lrc(0)
    );
    let converted = convert(&line_lrc(), &word).unwrap();
    assert_eq!((converted.matched, converted.total), (5, 7));
}

#[test]
fn another_version_is_refused() {
    // 时间整体差 5 秒（现场版、重录版）：文字一样也不算同一个版本。
    assert_eq!(
        convert(&line_lrc(), &word_lrc(5000)),
        Err(Mismatch {
            matched: 0,
            total: 5
        })
    );
    // 时间对得上但完全是另一首歌的词。
    let other = "[00:01.000]<00:01.000>别的<00:01.800>句子<00:02.600>\n\
                 [00:05.000]<00:05.000>完全<00:05.800>不同<00:06.600>";
    assert_eq!(
        convert(&line_lrc(), other),
        Err(Mismatch {
            matched: 0,
            total: 2
        })
    );
    // 「逐字歌词」其实没有逐字时间轴：不算。
    assert!(convert(&line_lrc(), &line_lrc()).is_err());
    assert!(convert(&line_lrc(), "").is_err());
}

#[test]
fn the_existing_translation_is_carried_over_line_by_line() {
    // 译文行紧跟在原文行后面、时间相同；平台声明与表示「没有译文」的 // 不算译文。
    let existing = "[00:01.00]清晨的风吹过窗台\n\
                    [00:01.00]the first line\n\
                    [00:05.00]我把昨天折成纸船\n\
                    [00:05.00]QQ音乐享有本翻译作品的著作权\n\
                    [00:09.00]让它顺着小河漂远\n\
                    [00:09.00]//\n\
                    [00:13.00]直到看见海的那边\n\
                    [00:13.00]the fourth line\n\
                    [00:17.00]再回头说一声再见";
    let converted = convert(existing, &word_lrc(0)).unwrap();

    assert!(converted.keeps_translation);
    assert_eq!((converted.matched, converted.total), (5, 5));
    let expected = [
        word_line(0, 0),
        "[00:01.000]the first line".to_owned(),
        word_line(1, 0),
        word_line(2, 0),
        word_line(3, 0),
        "[00:13.000]the fourth line".to_owned(),
        word_line(4, 0),
    ]
    .join("\n");
    assert_eq!(converted.text, expected);
}

#[test]
fn info_tags_are_kept_and_the_offset_is_applied_instead_of_copied() {
    // 现有歌词整体标早了 500 毫秒，用 [offset:500] 修正：核对时按修正后的时间算，新歌词不再带 offset。
    let existing = format!(
        "[ti:测试歌]\n[ar:某某]\n[offset:500]\n{}",
        SENTENCES
            .iter()
            .enumerate()
            .map(|(index, (head, tail))| format!(
                "[{}]{head}{tail}",
                stamp(500 + 4000 * index as i32)
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let converted = convert(&existing, &word_lrc(0)).unwrap();
    assert_eq!(
        converted.text,
        format!("[ti:测试歌]\n[ar:某某]\n{}", word_lrc(0))
    );
}

// ---- 服务：预览 ----

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

fn info_with(lyrics: Option<&str>) -> AudioTagInfo {
    AudioTagInfo {
        lyrics: lyrics.map(str::to_owned),
        ..tag_info("测试歌", "某某", 200_000)
    }
}

fn found(source: OnlineMusicSource) -> OnlineSongResult {
    song(source, "1", "测试歌", "某某", "专辑", 200_000)
}

/// 两个来源都搜得到这首歌；`qq` / `netease` 是各自的逐字歌词（`None` 表示只有逐行）。
fn online(qq: Option<String>, netease: Option<String>) -> Arc<StubOnline> {
    let stub = StubOnline::with(vec![], None);
    stub.on_search(|source, _| Ok(vec![found(source)]));
    stub.on_details(move |song| {
        let mut details = OnlineSongDetails::without_extras(song.clone());
        details.line_lyrics = Some(line_lrc());
        details.word_lyrics = match song.source {
            QqMusic => qq.clone(),
            Netease => netease.clone(),
        };
        Ok(Some(details))
    });
    stub
}

const SCORE: f64 = 0.6;

#[tokio::test]
async fn preview_finds_word_lyrics_for_embedded_line_lyrics_without_writing() {
    let temp = TempDir::new();
    let path = file(&temp, "a.mp3");
    let tags = StubTags::with(info_with(Some(&line_lrc())));
    let online = online(Some(word_lrc(200)), None);

    let outcome = service(&tags, &online, vec![])
        .plan_word_lyrics(&path, SCORE, &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(outcome.status, WordStatus::Ready);
    assert_eq!(outcome.source_label.as_deref(), Some("QQ 音乐"));
    assert_eq!(outcome.targets, vec![WordTarget::Embedded]);
    assert!(
        outcome.message.contains("对上 5/5 行"),
        "{}",
        outcome.message
    );
    assert!(outcome.plan.is_some());
    assert_eq!(tags.write_count(), 0, "预览不写文件");
    // 只问了 QQ 音乐就够了。
    assert_eq!(online.detail_calls(), vec![(QqMusic, "1".to_owned())]);
}

#[tokio::test]
async fn songs_that_need_no_conversion_are_not_looked_up() {
    let temp = TempDir::new();
    let path = file(&temp, "a.mp3");
    for (lyrics, status) in [
        (Some(word_lrc(0)), WordStatus::AlreadyWord),
        (None, WordStatus::NoLyrics),
        (Some("没有时间轴的歌词".to_owned()), WordStatus::NoLyrics),
    ] {
        let tags = StubTags::with(info_with(lyrics.as_deref()));
        let online = online(Some(word_lrc(0)), None);
        let outcome = service(&tags, &online, vec![])
            .plan_word_lyrics(&path, SCORE, &CancelToken::new())
            .await
            .unwrap();
        assert_eq!(outcome.status, status);
        assert!(outcome.plan.is_none());
        assert!(online.searches().is_empty(), "不需要转换的歌不联网");
    }
}

#[tokio::test]
async fn a_source_with_another_version_is_skipped_for_the_next_one() {
    let temp = TempDir::new();
    let path = file(&temp, "a.mp3");
    let tags = StubTags::with(info_with(Some(&line_lrc())));
    // QQ 音乐的是时间差 5 秒的另一个版本，网易云的对得上。
    let online = online(Some(word_lrc(5000)), Some(word_lrc(0)));

    let outcome = service(&tags, &online, vec![])
        .plan_word_lyrics(&path, SCORE, &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(outcome.status, WordStatus::Ready);
    assert_eq!(outcome.source_label.as_deref(), Some("网易云音乐"));
}

#[tokio::test]
async fn the_reasons_for_not_converting_are_told_apart() {
    let temp = TempDir::new();
    let path = file(&temp, "a.mp3");
    let tags = StubTags::with(info_with(Some(&line_lrc())));
    let plan = |online: Arc<StubOnline>| {
        let tags = Arc::clone(&tags);
        let path = path.clone();
        async move {
            service(&tags, &online, vec![])
                .plan_word_lyrics(&path, SCORE, &CancelToken::new())
                .await
                .unwrap()
        }
    };

    // 搜不到这首歌。
    let nothing = StubOnline::with(vec![], None);
    assert_eq!(plan(nothing).await.status, WordStatus::NoMatch);

    // 搜得到，但哪个来源都只有逐行歌词。
    assert_eq!(
        plan(online(None, None)).await.status,
        WordStatus::NoWordLyrics
    );

    // 有逐字歌词，但都是另一个版本。
    let outcome = plan(online(Some(word_lrc(5000)), Some(word_lrc(-6000)))).await;
    assert_eq!(outcome.status, WordStatus::Mismatch);
    assert!(outcome.message.contains("0/5"), "{}", outcome.message);
    assert!(outcome.plan.is_none());
}

#[tokio::test]
async fn fallback_sources_are_asked_last() {
    let temp = TempDir::new();
    let path = file(&temp, "a.mp3");
    let tags = StubTags::with(info_with(Some(&line_lrc())));
    let mut candidate = line_candidate("酷狗音乐", "测试歌", "某某", "", 200_000, &line_lrc());
    candidate.word_lyrics = Some(word_lrc(0));
    let fallback = StubFallback::returning("酷狗音乐", Some(candidate));

    let outcome = service(
        &tags,
        &online(None, None),
        vec![fallback as Arc<dyn LyricsSource>],
    )
    .plan_word_lyrics(&path, SCORE, &CancelToken::new())
    .await
    .unwrap();

    assert_eq!(outcome.status, WordStatus::Ready);
    assert_eq!(outcome.source_label.as_deref(), Some("酷狗音乐"));
}

// ---- 服务：写入 ----

#[tokio::test]
async fn applying_writes_only_the_lyrics_into_the_audio_tags() {
    let temp = TempDir::new();
    let path = file(&temp, "a.mp3");
    let tags = StubTags::with(info_with(Some(&line_lrc())));
    let service = service(&tags, &online(Some(word_lrc(0)), None), vec![]);
    let plan = service
        .plan_word_lyrics(&path, SCORE, &CancelToken::new())
        .await
        .unwrap()
        .plan
        .unwrap();

    let outcome = service.apply_word_plan(&path, &plan).await.unwrap();

    assert_eq!(outcome.status, WordStatus::Converted);
    assert_eq!(outcome.targets, vec![WordTarget::Embedded]);
    let written = tags.written().unwrap();
    assert_eq!(written.word_lyrics.as_deref(), Some(word_lrc(0).as_str()));
    assert_eq!(
        written,
        AudioTagData {
            word_lyrics: Some(word_lrc(0)),
            ..AudioTagData::default()
        },
        "别的标签一个都不写"
    );
}

#[tokio::test]
async fn lyrics_kept_in_an_lrc_file_are_written_back_to_it_with_a_backup() {
    let temp = TempDir::new();
    let path = file(&temp, "歌 - 名.flac");
    let lrc = temp.path().join("歌 - 名.lrc");
    // 带 BOM、CRLF 换行的歌词文件；音频文件里没有内嵌歌词。
    let original = format!("\u{feff}{}", line_lrc().replace('\n', "\r\n"));
    std::fs::write(&lrc, &original).unwrap();
    let tags = StubTags::with(info_with(None));
    let service = service(&tags, &online(Some(word_lrc(0)), None), vec![]);

    let outcome = service
        .plan_word_lyrics(&path, SCORE, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.status, WordStatus::Ready);
    assert_eq!(outcome.targets, vec![WordTarget::Sidecar]);
    assert_eq!(
        std::fs::read_to_string(&lrc).unwrap(),
        original,
        "预览不写文件"
    );

    let applied = service
        .apply_word_plan(&path, &outcome.plan.unwrap())
        .await
        .unwrap();

    assert_eq!(applied.status, WordStatus::Converted);
    assert!(applied.message.contains("_backup"), "{}", applied.message);
    assert_eq!(
        std::fs::read_to_string(&lrc).unwrap(),
        format!("\u{feff}{}", word_lrc(0).replace('\n', "\r\n")),
        "BOM 与换行风格照旧"
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("_backup").join("歌 - 名.lrc")).unwrap(),
        original
    );
    assert_eq!(tags.write_count(), 0, "歌词不在音频文件里，就不往里写");
}

#[tokio::test]
async fn both_places_are_converted_when_both_hold_line_lyrics() {
    let temp = TempDir::new();
    let path = file(&temp, "a.flac");
    std::fs::write(temp.path().join("a.lrc"), line_lrc()).unwrap();
    let tags = StubTags::with(info_with(Some(&line_lrc())));
    let service = service(&tags, &online(Some(word_lrc(0)), None), vec![]);

    let outcome = service
        .plan_word_lyrics(&path, SCORE, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(
        outcome.targets,
        vec![WordTarget::Embedded, WordTarget::Sidecar]
    );

    service
        .apply_word_plan(&path, &outcome.plan.unwrap())
        .await
        .unwrap();
    assert_eq!(tags.write_count(), 1);
    assert_eq!(
        std::fs::read_to_string(temp.path().join("a.lrc")).unwrap(),
        word_lrc(0)
    );
}

#[tokio::test]
async fn lyrics_changed_after_the_preview_are_not_overwritten() {
    let temp = TempDir::new();
    let path = file(&temp, "a.flac");
    let lrc = temp.path().join("a.lrc");
    std::fs::write(&lrc, line_lrc()).unwrap();
    let tags = StubTags::with(info_with(None));
    let service = service(&tags, &online(Some(word_lrc(0)), None), vec![]);
    let plan = service
        .plan_word_lyrics(&path, SCORE, &CancelToken::new())
        .await
        .unwrap()
        .plan
        .unwrap();

    std::fs::write(&lrc, "[00:01.00]用户自己改过的歌词").unwrap();
    let error = service.apply_word_plan(&path, &plan).await.unwrap_err();

    assert_eq!(
        error,
        BatchError::Failed("歌词在预览之后有改动，请重新预览".to_owned())
    );
    assert_eq!(
        std::fs::read_to_string(&lrc).unwrap(),
        "[00:01.00]用户自己改过的歌词"
    );
    assert!(!temp.path().join("_backup").exists());
}

// ---- 任务编排 ----

type Events = Arc<std::sync::Mutex<Vec<WordLyricsProgressView>>>;

fn recorder() -> (Events, WordProgressSink) {
    let events: Events = Arc::default();
    let sink = Arc::clone(&events);
    (
        events,
        Arc::new(move |event| sink.lock().unwrap().push(event)),
    )
}

fn request(files: &[PathBuf]) -> WordLyricsRequest {
    WordLyricsRequest {
        files: files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
    }
}

#[tokio::test]
async fn preview_then_apply_converts_the_ready_files_once() {
    let temp = TempDir::new();
    let files = [file(&temp, "a.mp3"), file(&temp, "b.mp3")];
    let tags = StubTags::with(info_with(Some(&line_lrc())));
    let online = online(Some(word_lrc(0)), None);
    let logs = LogHub::new();
    let manager = BatchManager::new(Arc::new(service(&tags, &online, vec![])), Arc::clone(&logs));
    let settings = AppSettings::new("D:\\x");

    let (events, sink) = recorder();
    let preview = manager
        .word_preview(request(&files), &settings, sink)
        .await
        .unwrap();
    assert_eq!((preview.total, preview.processed, preview.ready), (2, 2, 2));
    assert_eq!(
        (preview.converted, preview.skipped, preview.failed),
        (0, 0, 0)
    );
    assert!(!preview.cancelled);
    assert_eq!(tags.write_count(), 0);
    let finished: Vec<_> = events
        .lock()
        .unwrap()
        .iter()
        .filter(|event| event.stage == "finished")
        .map(|event| event.result.as_ref().unwrap().status)
        .collect();
    assert_eq!(finished, vec![WordStatus::Ready, WordStatus::Ready]);
    let searches_after_preview = online.searches().len();

    let (_, sink) = recorder();
    let applied = manager
        .word_apply(request(&files[..1]), sink)
        .await
        .unwrap();
    assert_eq!(
        (applied.total, applied.converted, applied.failed),
        (1, 1, 0)
    );
    assert_eq!(tags.write_count(), 1);
    assert_eq!(
        online.searches().len(),
        searches_after_preview,
        "转换不再联网"
    );

    // 同一个预览结果只能用一次；另一首还留着。
    let (_, sink) = recorder();
    let again = manager.word_apply(request(&files), sink).await.unwrap();
    assert_eq!((again.converted, again.failed), (1, 1));
    assert!(
        again.results[0].message.contains("请先预览"),
        "{}",
        again.results[0].message
    );
    assert_eq!(tags.write_count(), 2);
}

#[tokio::test]
async fn an_empty_request_is_rejected() {
    let tags = StubTags::with(info_with(None));
    let manager = BatchManager::new(
        Arc::new(service(&tags, &online(None, None), vec![])),
        LogHub::new(),
    );
    let (_, sink) = recorder();
    let error = manager
        .word_preview(request(&[]), &AppSettings::new("D:\\x"), sink)
        .await
        .unwrap_err();
    assert_eq!(error.kind, "noFiles");
}

#[test]
fn views_serialize_to_camel_case() {
    let view = WordLyricsResultView::new(
        r"D:\a\歌.flac",
        &WordOutcome {
            status: WordStatus::NoWordLyrics,
            message: "x".to_owned(),
            source_label: Some("QQ 音乐".to_owned()),
            targets: vec![WordTarget::Embedded, WordTarget::Sidecar],
            keeps_translation: true,
            plan: None,
        },
    );
    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["fileName"], "歌.flac");
    assert_eq!(json["status"], "noWordLyrics");
    assert_eq!(json["sourceLabel"], "QQ 音乐");
    assert_eq!(json["targets"], serde_json::json!(["embedded", "sidecar"]));
    assert_eq!(json["keepsTranslation"], true);

    let request: WordLyricsRequest = serde_json::from_str(r#"{"files":["a.mp3"]}"#).unwrap();
    assert_eq!(request.files, vec!["a.mp3".to_owned()]);
}
