//! 模型、视图文案与备用词源开关。

use crate::models::LyricsSourceKind;
use crate::online::model::{
    LyricsSourceCandidate, OnlineMusicSource, OnlineSongDetails, OnlineSongResult, music_source_of,
};
use crate::online::test_support::song;
use crate::online::view::{CoverView, lyrics_label};
use crate::online::{KugouLyricsSource, LrclibLyricsSource, fallback_source_enabled};
use crate::settings::AppSettings;

#[test]
fn artist_values_are_the_trimmed_list_or_nothing() {
    let mut result = song(
        OnlineMusicSource::Netease,
        "1",
        "歌",
        "AC/DC/周杰伦",
        "专辑",
        1000,
    );
    assert_eq!(result.artist_values(), None, "没有清单");

    result.artists = vec![" AC/DC ".to_owned(), " ".to_owned(), "周杰伦".to_owned()];
    assert_eq!(
        result.artist_values(),
        Some(vec!["AC/DC".to_owned(), "周杰伦".to_owned()])
    );

    // 前端把候选原样传回来：新字段带着清单，旧形状（没有 artists）也认。
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["artists"][0], " AC/DC ");
    let old: OnlineSongResult = serde_json::from_value(serde_json::json!({
        "source": "Netease", "sourceId": "1", "title": "歌", "artist": "a/b", "album": "", "durationMs": 0
    }))
    .unwrap();
    assert!(old.artists.is_empty());
}

#[test]
fn fallback_switches_follow_the_settings_and_unknown_sources_stay_on() {
    let mut settings = AppSettings::new("D:\\x");
    assert!(fallback_source_enabled(&settings, KugouLyricsSource::LABEL));
    assert!(fallback_source_enabled(
        &settings,
        LrclibLyricsSource::LABEL
    ));

    settings.fallback_kugou_lyrics = false;
    assert!(!fallback_source_enabled(&settings, "酷狗音乐"));
    assert!(fallback_source_enabled(&settings, "LRCLIB"));

    settings.fallback_lrclib_lyrics = false;
    assert!(!fallback_source_enabled(&settings, "LRCLIB"));
    assert!(fallback_source_enabled(&settings, "某个新词源"));
}

#[test]
fn only_primary_lyrics_sources_map_to_a_music_source() {
    assert_eq!(
        music_source_of(LyricsSourceKind::Netease),
        Some(OnlineMusicSource::Netease)
    );
    assert_eq!(
        music_source_of(LyricsSourceKind::QqMusic),
        Some(OnlineMusicSource::QqMusic)
    );
    assert_eq!(music_source_of(LyricsSourceKind::Kugou), None);
    assert_eq!(music_source_of(LyricsSourceKind::Lrclib), None);
    assert_eq!(OnlineMusicSource::Netease.label(), "网易云音乐");
    assert_eq!(OnlineMusicSource::QqMusic.label(), "QQ 音乐");
}

#[test]
fn song_display_helpers_match_the_desktop_list() {
    let mut item = song(
        OnlineMusicSource::QqMusic,
        "m",
        "晴天",
        "周杰伦",
        "叶惠美",
        269_000,
    );
    assert_eq!(item.display_title(), "晴天");
    assert_eq!(item.subtitle(), "周杰伦 · 叶惠美");
    assert_eq!(item.duration_text(), "4:29");
    assert_eq!(item.source_label(), "QQ 音乐");

    item.title = "  ".to_owned();
    item.artist = String::new();
    item.album = " ".to_owned();
    item.duration_ms = 0;
    assert_eq!(item.display_title(), "未知歌曲");
    assert_eq!(item.subtitle(), "未知歌手 · 未知专辑");
    assert_eq!(item.duration_text(), "--:--");

    item.duration_ms = 3_605_000;
    assert_eq!(item.duration_text(), "60:05", "与 C# 一致：分钟不折算小时");
    item.duration_ms = 59_999;
    assert_eq!(item.duration_text(), "0:59");
}

fn details(
    line: Option<&str>,
    word: Option<&str>,
    bilingual_line: Option<&str>,
    bilingual_word: Option<&str>,
) -> OnlineSongDetails {
    let mut details =
        OnlineSongDetails::without_extras(song(OnlineMusicSource::Netease, "1", "t", "a", "b", 1));
    details.line_lyrics = line.map(str::to_owned);
    details.word_lyrics = word.map(str::to_owned);
    details.bilingual_line_lyrics = bilingual_line.map(str::to_owned);
    details.bilingual_word_lyrics = bilingual_word.map(str::to_owned);
    details
}

#[test]
fn lyrics_selection_prefers_bilingual_then_word_and_falls_back_to_line() {
    let all = details(Some("L"), Some("W"), Some("BL"), Some("BW"));
    assert_eq!(all.get_lyrics(true, true), Some("BW"));
    assert_eq!(all.get_lyrics(false, true), Some("BL"));
    assert_eq!(all.get_lyrics(true, false), Some("W"));
    assert_eq!(all.get_lyrics(false, false), Some("L"));

    // 只有双语逐行：偏好逐字时也用它；没有任何双语时回到原文。
    let line_only_bilingual = details(Some("L"), Some("W"), Some("BL"), None);
    assert_eq!(line_only_bilingual.get_lyrics(true, true), Some("BL"));
    let plain = details(Some("L"), None, None, None);
    assert_eq!(plain.get_lyrics(true, true), Some("L"));
    assert_eq!(plain.get_lyrics(false, true), Some("L"));

    // 空白的双语版本视为没有。
    let blank_bilingual = details(Some("L"), Some("W"), Some("  "), Some(""));
    assert_eq!(blank_bilingual.get_lyrics(true, true), Some("W"));
    assert_eq!(blank_bilingual.get_lyrics(false, true), Some("L"));

    // 逐字为 None 时回退逐行；都没有则 None。
    assert_eq!(details(None, None, None, None).get_lyrics(true, true), None);
    assert!(all.has_word_lyrics() && all.has_translation());
    assert!(!plain.has_word_lyrics() && !plain.has_translation());
    assert!(!blank_bilingual.has_translation());
}

#[test]
fn candidates_select_lyrics_like_details() {
    let candidate = LyricsSourceCandidate {
        source_label: "酷狗音乐".to_owned(),
        title: "t".to_owned(),
        artist: "a".to_owned(),
        album: String::new(),
        duration_ms: 1,
        line_lyrics: Some("L".to_owned()),
        word_lyrics: Some("W".to_owned()),
        bilingual_line_lyrics: None,
        bilingual_word_lyrics: Some("BW".to_owned()),
    };
    assert_eq!(candidate.get_lyrics(true, true), Some("BW"));
    assert_eq!(candidate.get_lyrics(false, true), Some("L"));
    assert_eq!(candidate.get_lyrics(true, false), Some("W"));
    assert!(candidate.has_word_lyrics());
    assert!(candidate.has_translation());
}

#[test]
fn lyrics_labels_cover_every_combination() {
    assert_eq!(lyrics_label(true, true, true), "双语逐字歌词");
    assert_eq!(lyrics_label(true, true, false), "逐字歌词（该来源无翻译）");
    assert_eq!(
        lyrics_label(true, false, true),
        "双语逐行歌词（该来源无逐字）"
    );
    assert_eq!(
        lyrics_label(true, false, false),
        "逐行歌词（该来源无逐字）（该来源无翻译）"
    );
    assert_eq!(lyrics_label(false, true, true), "双语逐行歌词");
    assert_eq!(
        lyrics_label(false, false, false),
        "逐行歌词（该来源无翻译）"
    );
}

#[test]
fn covers_are_encoded_with_their_detected_mime_type() {
    let png = CoverView::new(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    assert_eq!(png.mime_type, "image/png");
    assert_eq!(png.size, 8);
    // 只有签名、没有 IHDR：认不出尺寸。
    assert_eq!((png.width, png.height), (None, None));
    let jpeg = CoverView::new(&[0xFF, 0xD8, 0xFF, 0xE0]);
    assert_eq!(jpeg.mime_type, "image/jpeg");
    assert_eq!(jpeg.base64, "/9j/4A==");

    let mut sized = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    sized.extend_from_slice(b"IHDR");
    sized.extend_from_slice(&1200_u32.to_be_bytes());
    sized.extend_from_slice(&1200_u32.to_be_bytes());
    let sized = CoverView::new(&sized);
    assert_eq!((sized.width, sized.height), (Some(1200), Some(1200)));
}

#[test]
fn song_results_round_trip_through_json_with_optional_fields_missing() {
    let parsed: OnlineSongResult = serde_json::from_str(
        r#"{"source":"Netease","sourceId":"1","title":"t","artist":"a","album":"b","durationMs":5}"#,
    )
    .unwrap();
    assert_eq!(parsed.year, None);
    assert_eq!(parsed.cover_url, None);
    assert_eq!(parsed.source, OnlineMusicSource::Netease);
}
