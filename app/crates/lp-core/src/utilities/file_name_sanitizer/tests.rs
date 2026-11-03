use super::*;

#[test]
fn sanitize_replaces_invalid_windows_characters() {
    let value = sanitize(Some("a/b:c*?\"<>|"), DEFAULT_MAX_LENGTH);

    for c in ['/', ':', '*', '?', '"', '<', '>', '|'] {
        assert!(!value.contains(c), "{value}");
    }
    assert_eq!(value, "a_b_c______");
}

#[test]
fn sanitize_handles_blank_control_and_trailing_dots() {
    assert_eq!(sanitize(None, DEFAULT_MAX_LENGTH), "未知");
    assert_eq!(sanitize(Some("   "), DEFAULT_MAX_LENGTH), "未知");
    assert_eq!(sanitize(Some("..."), DEFAULT_MAX_LENGTH), "未知");
    assert_eq!(sanitize(Some("a\tb\u{0}c"), DEFAULT_MAX_LENGTH), "a_b_c");
    assert_eq!(sanitize(Some(" 歌名. "), DEFAULT_MAX_LENGTH), "歌名");
}

#[test]
fn sanitize_truncates_by_utf16_units_and_trims() {
    let long = format!("{} {}", "a".repeat(9), "b".repeat(200));

    let value = sanitize(Some(&long), 10);

    assert_eq!(value, "a".repeat(9));
    assert_eq!(sanitize(Some(&"歌".repeat(300)), 120).chars().count(), 120);
    // 代理对不会被劈成两半。
    let emoji = "😀".repeat(5);
    assert_eq!(sanitize(Some(&emoji), 5), "😀😀");
}

#[test]
fn build_file_name_applies_template_and_extension() {
    let name = build_file_name(
        "{artist} - {title} ({year}) [{track}]",
        NameFields {
            title: Some("晴天"),
            artist: Some("周杰伦"),
            album: Some("叶惠美"),
            track_number: 3,
            year: 2003,
            song_id: 186016,
        },
        ".mp3",
    );

    assert_eq!(name, "周杰伦 - 晴天 (2003) [03].mp3");
}

#[test]
fn build_file_name_handles_empty_values_and_missing_dot() {
    let name = build_file_name(
        "{title}",
        NameFields {
            song_id: 12,
            ..NameFields::default()
        },
        "flac",
    );

    assert_eq!(name, "未知歌曲.flac");
    assert!(!name[..name.len() - 5].trim().is_empty());
}

#[test]
fn build_file_name_is_case_insensitive_and_does_not_double_the_extension() {
    let name = build_file_name(
        "{ARTIST}_{Album}_{ID}.MP3",
        NameFields {
            title: Some("t"),
            artist: Some("a"),
            album: Some("b"),
            song_id: 7,
            ..NameFields::default()
        },
        "mp3",
    );

    assert_eq!(name, "a_b_7.MP3");
}

#[test]
fn build_file_name_falls_back_to_default_template_and_sanitizes_values() {
    let fields = NameFields {
        title: Some("a/b"),
        artist: Some("c:d"),
        track_number: 100,
        song_id: 1,
        ..NameFields::default()
    };

    assert_eq!(build_file_name("  ", fields, "mp3"), "c_d - a_b.mp3");
    assert_eq!(build_file_name("{track}", fields, "mp3"), "100.mp3");
}
