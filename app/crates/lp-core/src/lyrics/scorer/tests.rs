//! `LyricsMatchScorer` 的单测：C# `LyricsMatchScorerTests` 全部移植，另加归一化、多值切段、版本标记、阈值文本的边界。

use super::*;

fn approx(left: f64, right: f64) {
    assert!((left - right).abs() < 1e-3, "{left} 与 {right} 相差过大");
}

type Side<'a> = (&'a str, &'a str, Option<&'a str>, i64);

fn s(local: Side, result: Side) -> LyricsMatchScore {
    score(
        Some(local.0),
        Some(local.1),
        local.2,
        local.3,
        Some(result.0),
        Some(result.1),
        result.2,
        result.3,
    )
}

// ---- 与 C# 测试对应 ----

#[test]
fn build_queries_prefers_title_artist_and_adds_variants() {
    let queries = build_queries(Some("Lemon (Live)"), Some("米津玄師"));

    assert_eq!(queries[0], "Lemon (Live) 米津玄師");
    assert_eq!(queries[1], "Lemon (Live)");
    assert!(queries.contains(&"米津玄師 Lemon (Live)".to_owned()));
    assert!(queries.contains(&"Lemon 米津玄師".to_owned()));
    let mut distinct: Vec<String> = queries.iter().map(|q| q.to_lowercase()).collect();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), queries.len());
}

#[test]
fn build_queries_without_artist_still_returns_title_variants() {
    let queries = build_queries(Some("晴天（钢琴版）"), None);

    assert_eq!(queries[0], "晴天（钢琴版）");
    assert!(queries.contains(&"晴天".to_owned()));
}

#[test]
fn exact_match_scores_one_and_is_confident() {
    let result = s(
        ("晴天", "周杰伦", Some("叶惠美"), 269_000),
        ("晴天", "周杰伦", Some("叶惠美"), 269_000),
    );

    assert!(!result.rejected);
    assert!(result.total > 0.95, "{}", result.total);
    assert!(result.is_confident());
}

#[test]
fn live_version_is_penalized_but_still_matched() {
    let exact = s(
        ("Lemon", "米津玄師", None, 255_000),
        ("Lemon", "米津玄師", None, 255_000),
    );
    let live = s(
        ("Lemon", "米津玄師", None, 255_000),
        ("Lemon (Live)", "米津玄師", None, 255_000),
    );

    assert!(!live.rejected);
    assert!(live.total < exact.total, "{} < {}", live.total, exact.total);
    assert!(live.title > POOR_TITLE_SCORE, "{}", live.title);
}

#[test]
fn same_title_but_different_artist_falls_below_default_threshold() {
    let result = s(
        (
            "レッド・パージ！！！",
            "+α、あるふぁきゅん。",
            None,
            220_000,
        ),
        ("レッド・パージ！！！", "Reol", None, 220_000),
    );

    assert!(result.title > 0.95);
    assert!(result.artist < 0.10);
    assert!(result.total < DEFAULT_MIN_SCORE, "{}", result.total);
}

#[test]
fn instrumental_candidate_is_rejected() {
    let result = s(
        ("晴天", "周杰伦", None, 269_000),
        ("晴天 (伴奏)", "周杰伦", None, 269_000),
    );

    assert!(result.rejected);
    assert!(
        result
            .reject_reason
            .as_deref()
            .is_some_and(|reason| !reason.trim().is_empty())
    );
    assert!(!result.is_confident());
}

#[test]
fn instrumental_candidate_is_allowed_when_the_local_song_is_instrumental_too() {
    let result = s(
        ("晴天 (伴奏)", "周杰伦", None, 269_000),
        ("晴天 (伴奏)", "周杰伦", None, 269_000),
    );
    assert!(!result.rejected);
    assert_eq!(result.reject_reason, None);
}

#[test]
fn multi_artist_local_matches_single_artist_candidate() {
    let result = s(
        ("晴天", "周杰伦/杨瑞代", None, 269_000),
        ("晴天", "周杰伦", None, 269_500),
    );

    assert!(!result.rejected);
    assert!(result.artist >= 0.9, "{}", result.artist);
    assert!(result.total >= DEFAULT_MIN_SCORE, "{}", result.total);
}

#[test]
fn unrelated_song_is_below_default_threshold() {
    let result = s(
        ("完全无关的歌曲", "陌生歌手", None, 200_000),
        ("晴天", "周杰伦", None, 269_500),
    );

    assert!(!result.rejected);
    assert!(result.total < DEFAULT_MIN_SCORE, "{}", result.total);
}

#[test]
fn duration_score_uses_tolerance_buckets() {
    for (delta, expected) in [
        (1_000, 1.0),
        (4_000, 0.90),
        (8_000, 0.75),
        (15_000, 0.45),
        (35_000, 0.15),
        (120_000, 0.0),
    ] {
        approx(duration_score(200_000, 200_000 + delta), expected);
        approx(duration_score(200_000 + delta, 200_000), expected);
    }
}

#[test]
fn duration_score_bucket_edges_are_inclusive() {
    assert_eq!(duration_score(100_000, 102_000), 1.0);
    assert_eq!(duration_score(100_000, 102_001), 0.90);
    assert_eq!(duration_score(100_000, 105_000), 0.90);
    assert_eq!(duration_score(100_000, 105_001), 0.75);
    assert_eq!(duration_score(100_000, 110_000), 0.75);
    assert_eq!(duration_score(100_000, 110_001), 0.45);
    assert_eq!(duration_score(100_000, 120_000), 0.45);
    assert_eq!(duration_score(100_000, 120_001), 0.15);
    assert_eq!(duration_score(100_000, 140_000), 0.15);
    assert_eq!(duration_score(100_000, 140_001), 0.0);
}

#[test]
fn duration_score_unknown_duration_returns_neutral_half() {
    approx(duration_score(0, 200_000), 0.5);
    approx(duration_score(200_000, 0), 0.5);
    approx(duration_score(-5, -5), 0.5);
}

#[test]
fn normalize_text_ignores_width_case_and_punctuation() {
    assert_eq!(
        normalize_text(Some("Ｌｅｍｏｎ   -   Lemon")),
        normalize_text(Some("lemon lemon"))
    );
}

#[test]
fn try_parse_threshold_accepts_percent_and_decimal() {
    for (text, expected) in [
        ("60", 0.60),
        ("60%", 0.60),
        (" 65 % ", 0.65),
        ("0.6", 0.60),
        ("0.75", 0.75),
        ("100", 1.00),
        ("30", 0.30),
        ("46", 0.46),
    ] {
        let parsed = try_parse_threshold(Some(text)).unwrap_or_else(|| panic!("{text} 应能解析"));
        approx(parsed, expected);
    }
}

#[test]
fn try_parse_threshold_rejects_out_of_range_or_garbage() {
    for text in ["", "   ", "abc", "29", "0.2", "101", "150", "1.5", "%"] {
        assert_eq!(try_parse_threshold(Some(text)), None, "{text:?} 应被拒绝");
    }
    assert_eq!(try_parse_threshold(None), None);
}

#[test]
fn format_threshold_percent_clamps_and_rounds() {
    for (threshold, expected) in [
        (0.60, "60"),
        (0.45, "45"),
        (0.3, "30"),
        (1.0, "100"),
        (0.05, "30"),
    ] {
        assert_eq!(format_threshold_percent(threshold), expected);
    }
}

#[test]
fn threshold_text_round_trips_through_settings_format() {
    let text = format_threshold_percent(0.72);
    let parsed = try_parse_threshold(Some(&text)).unwrap();
    approx(parsed, 0.72);
}

// ---- 边界与细节 ----

#[test]
fn threshold_parsing_handles_fullwidth_percent_signs_and_exponents() {
    // 全角数字 .NET 与 Rust 都不当数字。
    assert_eq!(try_parse_threshold(Some("６０％")), None);
    approx(try_parse_threshold(Some("60％")).unwrap(), 0.60);
    approx(try_parse_threshold(Some("6e1")).unwrap(), 0.60);
    approx(try_parse_threshold(Some("+0.5")).unwrap(), 0.50);
    assert_eq!(try_parse_threshold(Some("NaN")), None);
    assert_eq!(try_parse_threshold(Some("Infinity")), None);
    assert_eq!(try_parse_threshold(Some("-0.5")), None);
}

#[test]
fn threshold_rounding_is_bankers_rounding_on_two_digits() {
    // 0.625 能被二进制精确表示，.NET 的 Math.Round(0.625, 2) = 0.62（向偶数舍入）。
    assert_eq!(try_parse_threshold(Some("0.625")), Some(0.62));
    assert_eq!(try_parse_threshold(Some("0.875")), Some(0.88));
}

#[test]
fn normalize_text_keeps_letters_and_digits_only() {
    assert_eq!(
        normalize_text(Some("  Hello,   World!! 2024 ")),
        "hello world 2024"
    );
    assert_eq!(normalize_text(Some("")), "");
    assert_eq!(normalize_text(None), "");
    assert_eq!(normalize_text(Some("!!!")), "");
    // 全角字母数字经 NFKC 折成半角。
    assert_eq!(normalize_text(Some("ＡＢＣ１２３")), "abc123");
}

#[test]
fn normalize_text_treats_supplementary_characters_as_separators() {
    // .NET 按码元判定：代理码元不是字母或数字，所以补充平面字符（含 emoji 与 CJK 扩展 B 汉字）等同分隔符。
    assert_eq!(normalize_text(Some("a🎵b")), "a b");
    assert_eq!(normalize_text(Some("a𠀀b")), "a b");
}

#[test]
fn normalize_text_drops_lone_combining_marks_like_dotnet() {
    // 能合成的组合字符经 NFKC 合成后保留；孤立的组合附加符号（Me / Mn）不是字母，当作分隔符。
    assert_eq!(normalize_text(Some("e\u{0301}")), "é");
    assert_eq!(normalize_text(Some("a\u{20DD}b")), "a b");
}

#[test]
fn compact_text_removes_spaces() {
    assert_eq!(compact_text(Some("Hello, World")), "helloworld");
}

#[test]
fn strip_brackets_handles_nesting_and_unbalanced_brackets() {
    assert_eq!(strip_brackets(Some("晴天（钢琴版）")), "晴天");
    assert_eq!(strip_brackets(Some("a(b(c)d)e")), "ae");
    assert_eq!(strip_brackets(Some("a)b")), "ab");
    assert_eq!(strip_brackets(Some("a(b")), "a");
    assert_eq!(strip_brackets(Some("【TV】Title「x」")), "Title");
    assert_eq!(strip_brackets(Some("  ")), "");
    assert_eq!(strip_brackets(None), "");
}

#[test]
fn core_title_normalizes_after_stripping() {
    assert_eq!(core_title(Some("Lemon (Live) [2019]")), "lemon");
}

#[test]
fn instrumental_detection_covers_chinese_and_english_markers() {
    for title in [
        "Song (Instrumental)",
        "歌曲 伴奏",
        "纯音乐",
        "Off Vocal ver.",
        "OffVocal",
        "Karaoke Mix",
        "伴唱版",
    ] {
        assert!(is_instrumental(Some(title)), "{title}");
    }
    assert!(!is_instrumental(Some("晴天")));
    assert!(!is_instrumental(None));
    assert!(!is_instrumental(Some("")));
}

#[test]
fn queries_are_empty_without_a_title() {
    assert!(build_queries(None, Some("歌手")).is_empty());
    assert!(build_queries(Some("   "), Some("歌手")).is_empty());
}

#[test]
fn queries_collapse_whitespace_and_dedupe_ignoring_case() {
    let queries = build_queries(Some("  Hello \t  World "), Some(" ARTIST  "));
    assert_eq!(
        queries,
        vec!["Hello World ARTIST", "Hello World", "ARTIST Hello World"]
    );

    // 变体之间仅大小写不同的不重复。
    let queries = build_queries(Some("LEMON"), Some("lemon"));
    assert_eq!(queries, vec!["LEMON lemon", "LEMON"]);
}

#[test]
fn queries_strip_featuring_tails_and_fall_back_to_the_first_segment() {
    let queries = build_queries(Some("Song feat. Someone"), Some("Artist"));
    assert!(queries.contains(&"Song Artist".to_owned()));

    let queries = build_queries(Some("春夏 / 秋冬"), Some("歌手"));
    assert_eq!(queries.last().map(String::as_str), Some("春夏 歌手"));

    let queries = build_queries(Some("春夏 / 秋冬"), None);
    assert_eq!(queries.last().map(String::as_str), Some("春夏"));
}

#[test]
fn featuring_keyword_at_the_start_is_not_removed() {
    let queries = build_queries(Some("With You"), None);
    assert_eq!(queries, vec!["With You"]);
    let queries = build_queries(Some("Dance with me"), None);
    assert!(queries.contains(&"Dance".to_owned()));
}

#[test]
fn queries_never_exceed_six_variants() {
    // 标题+歌手、标题、歌手+标题、主体+歌手、去 feat+歌手、并列第一段+歌手。
    let queries = build_queries(Some("A (x) feat. B / C"), Some("Z"));
    assert!(queries.len() <= 6, "{queries:?}");
    assert_eq!(queries[0], "A (x) feat. B / C Z");
}

#[test]
fn duration_matches_is_lenient_for_unknown_durations() {
    assert!(duration_matches(0, 200_000, DEFAULT_DURATION_TOLERANCE_MS));
    assert!(duration_matches(200_000, 0, DEFAULT_DURATION_TOLERANCE_MS));
    assert!(duration_matches(
        200_000,
        208_000,
        DEFAULT_DURATION_TOLERANCE_MS
    ));
    assert!(!duration_matches(
        200_000,
        208_001,
        DEFAULT_DURATION_TOLERANCE_MS
    ));
    assert!(duration_matches(200_000, 215_000, 20_000));
}

#[test]
fn split_multi_value_handles_separators_keywords_and_blanks() {
    let parts = split_multi_value(Some("A feat. B / C"), &ARTIST_SEPARATORS, &ARTIST_KEYWORDS);
    assert_eq!(parts, vec!["A", "B", "C"]);

    let parts = split_multi_value(Some("A FEATURING b"), &ARTIST_SEPARATORS, &ARTIST_KEYWORDS);
    assert_eq!(parts, vec!["A", "b"]);

    // 没有分隔符与连接词时返回空列表。
    assert!(split_multi_value(Some("Solo"), &ARTIST_SEPARATORS, &ARTIST_KEYWORDS).is_empty());
    assert!(split_multi_value(Some("   "), &ARTIST_SEPARATORS, &ARTIST_KEYWORDS).is_empty());
    assert!(split_multi_value(None, &ARTIST_SEPARATORS, &ARTIST_KEYWORDS).is_empty());

    // 空片段被丢弃，片段两端空白被修剪。
    let parts = split_multi_value(Some(" /A// B /"), &ARTIST_SEPARATORS, &[]);
    assert_eq!(parts, vec!["A", "B"]);
}

#[test]
fn artist_score_matches_any_pair_of_split_artists() {
    approx(artist_score(Some("周杰伦"), Some("周杰伦")), 1.0);
    // 任意一对相同 → 0.98。
    approx(artist_score(Some("A & B"), Some("C / B")), 0.98);
    approx(artist_score(Some("Jay feat. Zed"), Some("Zed / Q")), 0.98);
    // 与 C# 一致：没有分隔符的一侧不切段，只能靠整体包含关系（0.88 + 0.09 * 3/12）。
    approx(artist_score(Some("Jay feat. Zed"), Some("Zed")), 0.9025);
    // 任一方为空 → 中性 0.5。
    approx(artist_score(None, Some("A")), 0.5);
    approx(artist_score(Some("A"), Some("")), 0.5);
}

#[test]
fn album_score_is_neutral_when_missing_and_exact_when_equal() {
    approx(album_score(None, Some("a")), 0.5);
    approx(album_score(Some("Abc"), Some("abc")), 1.0);
    // 包含关系：0.88 + 0.09 * 3/9。
    approx(
        album_score(Some("abc"), Some("abcdefghi")),
        0.88 + 0.09 / 3.0,
    );
}

#[test]
fn title_score_handles_brackets_segments_and_reversed_tokens() {
    // 标题主体相同 → 满分。
    approx(title_score(Some("晴天"), Some("晴天 (Live)")), 1.0);
    // 并列标题：命中某一段时轻微降权。
    approx(title_score(Some("春夏 / 秋冬"), Some("春夏")), 0.97);
    // 词序颠倒救援。
    approx(title_score(Some("love you"), Some("you love")), 0.95);
    assert_eq!(title_score(Some(""), Some("晴天")), 0.0);
    assert_eq!(title_score(Some("晴天"), None), 0.0);
}

#[test]
fn version_tags_match_whole_english_words_only() {
    // "discover" 不应触发 cover，"delivery" 不应触发 live：与自身比较仍是满分。
    let discover = s(
        ("Discover", "A", None, 200_000),
        ("Discover", "A", None, 200_000),
    );
    let delivery = s(
        ("Delivery", "A", None, 200_000),
        ("Delivery", "A", None, 200_000),
    );
    approx(discover.total, 1.0);
    approx(delivery.total, 1.0);

    // 候选多出 Live 标记：乘 0.86；标题主体（去括号）与本地一致，所以标题分仍是 1。
    let live = s(
        ("Delivery", "A", None, 200_000),
        ("Delivery (Live)", "A", None, 200_000),
    );
    assert!((live.total - 0.86).abs() < 1e-12, "{}", live.total);
}

#[test]
fn local_version_tag_missing_in_candidate_costs_less_than_an_extra_candidate_tag() {
    let local_live = s(
        ("Song (Live)", "A", None, 200_000),
        ("Song", "A", None, 200_000),
    );
    let candidate_live = s(
        ("Song", "A", None, 200_000),
        ("Song (Live)", "A", None, 200_000),
    );
    // 候选缺少标记乘 0.94，候选多出标记乘 0.86。
    assert!((local_live.total - 0.94).abs() < 1e-12);
    assert!((candidate_live.total - 0.86).abs() < 1e-12);
}

#[test]
fn artist_mismatch_multiplies_total_by_three_quarters_when_both_artists_exist() {
    let result = s(
        ("Song", "AAA", None, 200_000),
        ("Song", "ZZZZZZ", None, 200_000),
    );
    assert!(result.artist < 0.30);
    let expected = (1.0 * 0.55 + result.artist * 0.25 + 1.0 * 0.20) * 0.75;
    assert!((result.total - expected).abs() < 1e-12);
}

#[test]
fn total_without_artist_uses_title_duration_album_weights() {
    let result = score(
        Some("Song"),
        None,
        Some("Album"),
        200_000,
        Some("Song"),
        Some("Artist"),
        Some("Album"),
        200_000,
    );
    let expected = 1.0 * 0.72 + 1.0 * 0.20 + 1.0 * 0.08;
    assert!((result.total - expected).abs() < 1e-12);
}

#[test]
fn album_mismatch_costs_three_percent_only_when_both_albums_exist() {
    let with_albums = score(
        Some("Song"),
        Some("A"),
        Some("Alpha"),
        200_000,
        Some("Song"),
        Some("A"),
        Some("Zzzzz"),
        200_000,
    );
    let without = score(
        Some("Song"),
        Some("A"),
        None,
        200_000,
        Some("Song"),
        Some("A"),
        Some("Zzzzz"),
        200_000,
    );
    assert!(with_albums.album < RELIABLE_ALBUM_SCORE);
    assert!((without.total * 0.97 - with_albums.total).abs() < 1e-12);
}

#[test]
fn score_serializes_as_camel_case() {
    let result = s(("晴天", "周杰伦", None, 1), ("晴天", "周杰伦", None, 1));
    let value = serde_json::to_value(&result).unwrap();
    assert!(value.get("rejectReason").is_some());
    assert!(value.get("title").is_some());
    assert_eq!(value["rejected"], false);
}
