//! 移植自 C# `LyricParserTests` 与 `QrcLyricParserTests`，外加若干边界用例。

use super::*;

fn raw(lrc: &str) -> RawLyrics {
    RawLyrics::from_lrc(lrc)
}

fn with_yrc(yrc: &str) -> RawLyrics {
    RawLyrics {
        yrc: Some(yrc.to_owned()),
        ..RawLyrics::default()
    }
}

fn with_qrc(qrc: &str) -> RawLyrics {
    RawLyrics {
        qrc: Some(qrc.to_owned()),
        ..RawLyrics::default()
    }
}

fn plain(document: &LyricDocument) -> String {
    to_plain_lrc(document, true)
}

fn enhanced(document: &LyricDocument) -> String {
    to_enhanced_lrc(document, true)
}

fn starts(document: &LyricDocument) -> Vec<i32> {
    document.lines.iter().map(|line| line.start_ms).collect()
}

fn texts(document: &LyricDocument) -> Vec<&str> {
    document
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect()
}

// ===== LyricParserTests =====

#[test]
fn plain_lrc_applies_offset_and_multiple_timestamps() {
    let lrc = "[ti:测试歌曲]\n[ar:测试歌手]\n[offset:-100]\n[00:01.00]第一行\n[00:02.50][00:10.00]重复行\n[00:03.000]第三行";

    let document = parse(&raw(lrc));

    assert_eq!(document.lines.len(), 4);
    assert_eq!(starts(&document), [900, 2400, 2900, 9900]);
    assert_eq!(texts(&document), ["第一行", "重复行", "第三行", "重复行"]);
    assert_eq!(document.lines[0].duration_ms, 1500);
    assert_eq!(document.lines[1].duration_ms, 500);
    assert_eq!(document.lines[2].duration_ms, 7000);
    assert!(!document.has_word_level());

    assert_eq!(
        plain(&document),
        [
            "[00:00.900]第一行",
            "[00:02.400]重复行",
            "[00:02.900]第三行",
            "[00:09.900]重复行"
        ]
        .join("\n")
    );
}

#[test]
fn plain_lrc_ignores_metadata_tags_and_parses_fraction_variants() {
    let lrc = "[ti:标题]\n[ar:歌手]\n[al:专辑]\n[by:制作]\n[re:工具]\n[00:01.5]A\n[00:02.05]B\n[00:03.005]C";

    let document = parse(&raw(lrc));

    assert_eq!(document.lines.len(), 3);
    assert_eq!(starts(&document), [1500, 2050, 3005]);
    assert_eq!(texts(&document), ["A", "B", "C"]);
}

#[test]
fn translation_merges_by_nearest_timestamp() {
    let lrc = "[00:01.00]Hello\n[00:05.00]World\n[00:09.00]No translation line";
    let translated = "[00:05.30]世界\n[00:01.05]你好";

    let document = parse(&RawLyrics {
        lrc: Some(lrc.to_owned()),
        translated_lrc: Some(translated.to_owned()),
        ..RawLyrics::default()
    });

    assert_eq!(document.lines.len(), 3);
    assert_eq!(document.lines[0].translation.as_deref(), Some("你好"));
    assert_eq!(document.lines[1].translation.as_deref(), Some("世界"));
    assert_eq!(document.lines[2].translation, None);
    assert_eq!(document.translation.as_deref(), Some(translated));

    let plain = plain(&document);
    // 时间戳保留到毫秒，译文行与原文行使用同一时间戳，不再人为顺延。
    assert!(plain.contains("[00:01.000]Hello"));
    assert!(plain.contains("[00:01.000]你好"));
    assert!(plain.contains("[00:05.000]World"));
    assert!(plain.contains("[00:05.000]世界"));
    assert!(plain.contains("[00:09.000]No translation line"));
    assert!(!plain.contains("[00:09.000]世界"));
}

#[test]
fn enhanced_lrc_parses_word_tags() {
    let lrc = "[00:01.00]<00:01.00>Hello<00:02.00> world\n[00:05.00]plain line";

    let document = parse(&raw(lrc));

    assert!(document.has_word_level());
    assert_eq!(document.lines.len(), 2);
    let first = &document.lines[0];
    assert_eq!(first.start_ms, 1000);
    assert_eq!(first.duration_ms, 4000);
    assert_eq!(first.text, "Hello world");
    assert!(first.has_word_timing());
    assert_eq!(first.words.len(), 2);
    assert_eq!(first.words[0].start_ms, 1000);
    assert_eq!(first.words[0].duration_ms, 1000);
    assert_eq!(first.words[1].start_ms, 2000);
    assert_eq!(first.words[1].duration_ms, 3000);
    assert_eq!(first.words[1].text, " world");
    assert!(!document.lines[1].has_word_timing());

    // 与 LDDC 一致：结束标签总是写出，重复的起始标签省略，行尾补行结束标签。
    assert_eq!(
        enhanced(&document),
        [
            "[00:01.000]<00:01.000>Hello<00:02.000> world<00:05.000>",
            "[00:05.000]plain line"
        ]
        .join("\n")
    );
    assert_eq!(
        plain(&document),
        ["[00:01.000]Hello world", "[00:05.000]plain line"].join("\n")
    );
}

#[test]
fn yrc_v0_real_netease_sample() {
    let yrc = "[0,1000](0,1000,0) 作词 : 张国祥\n[12580,3470](12580,250,0)难(12830,300,0)以(13130,370,0)忘(13500,430,0)记(13930,570,0)初(14500,280,0)次(14780,300,0)见(15080,970,0)你\n[16960,3560](16960,240,0)一(17200,350,0)双(17550,470,0)迷(18020,490,0)人(18510,790,0)的(19300,270,0)眼(19570,950,0)睛";

    let document = parse(&with_yrc(yrc));

    assert_eq!(document.lines.len(), 3);
    assert!(document.has_word_level());

    let credit = &document.lines[0];
    assert_eq!(credit.start_ms, 0);
    assert_eq!(credit.duration_ms, 1000);
    assert_eq!(credit.text, " 作词 : 张国祥");
    assert_eq!(credit.words.len(), 1);

    let line = &document.lines[1];
    assert_eq!(line.start_ms, 12580);
    assert_eq!(line.duration_ms, 3470);
    assert_eq!(line.text, "难以忘记初次见你");
    assert_eq!(line.words.len(), 8);
    assert_eq!(line.words[0].start_ms, 12580);
    assert_eq!(line.words[0].duration_ms, 250);
    assert_eq!(line.words[0].text, "难");
    let last = line.words.last().unwrap();
    assert_eq!(last.text, "你");
    assert_eq!(last.start_ms, 15080);
    assert_eq!(last.duration_ms, 970);

    assert_eq!(document.lines[2].start_ms, 16960);
    assert_eq!(document.lines[2].text, "一双迷人的眼睛");

    let plain = plain(&document);
    assert!(plain.contains("[00:00.000]作词 : 张国祥"));
    assert!(plain.contains("[00:12.580]难以忘记初次见你"));
    assert!(plain.contains("[00:16.960]一双迷人的眼睛"));

    let enhanced = enhanced(&document);
    assert!(enhanced.contains("[00:12.580]<00:12.580>难<00:12.830>以<00:13.130>忘"));
    assert!(enhanced.contains("<00:15.080>你"));
    assert!(enhanced.contains("[00:00.000]<00:00.000> 作词 : 张国祥"));
}

#[test]
fn yrc_relative_word_timestamps_are_normalized() {
    let document = parse(&with_yrc("[10000,2000](0,500,0)甲(500,500,0)乙"));

    assert_eq!(document.lines.len(), 1);
    let line = &document.lines[0];
    assert_eq!(line.start_ms, 10000);
    assert_eq!(line.text, "甲乙");
    assert_eq!(line.words.len(), 2);
    assert_eq!(line.words[0].start_ms, 10000);
    assert_eq!(line.words[1].start_ms, 10500);
    assert_eq!(line.words[0].duration_ms, 500);
    assert_eq!(line.words[1].duration_ms, 500);
}

#[test]
fn json_flavor_yrc_supports_mixed_lines() {
    let yrc = "{\"t\":0,\"c\":[{\"tx\":\"作词: \"},{\"tx\":\"张国祥\"}]}\n{\"t\":1000,\"c\":[{\"tx\":\"作曲: 汤小康\"}]}\n[12580,3470](12580,250,0)难";

    let document = parse(&with_yrc(yrc));

    assert_eq!(document.lines.len(), 3);
    assert_eq!(document.lines[0].start_ms, 0);
    assert_eq!(document.lines[0].text, "作词: 张国祥");
    assert_eq!(document.lines[0].duration_ms, 1000);
    assert_eq!(document.lines[1].start_ms, 1000);
    assert_eq!(document.lines[1].text, "作曲: 汤小康");
    assert_eq!(document.lines[2].start_ms, 12580);
    assert_eq!(document.lines[2].words.len(), 1);

    assert_eq!(
        plain(&document),
        [
            "[00:00.000]作词: 张国祥",
            "[00:01.000]作曲: 汤小康",
            "[00:12.580]难"
        ]
        .join("\n")
    );
}

#[test]
fn yrc_without_word_tags_produces_line_level_lyrics() {
    let document = parse(&with_yrc("[1000,500]纯文本"));

    assert_eq!(document.lines.len(), 1);
    let line = &document.lines[0];
    assert_eq!(line.start_ms, 1000);
    assert_eq!(line.duration_ms, 500);
    assert_eq!(line.text, "纯文本");
    assert!(!line.has_word_timing());
    assert!(!document.has_word_level());
}

#[test]
fn yrc_takes_priority_over_lrc() {
    let document = parse(&RawLyrics {
        lrc: Some("[00:99.00]不应该出现".to_owned()),
        yrc: Some("[1000,500]逐字行".to_owned()),
        ..RawLyrics::default()
    });

    assert_eq!(document.lines.len(), 1);
    assert_eq!(document.lines[0].start_ms, 1000);
    assert_eq!(document.lines[0].text, "逐字行");
}

#[test]
fn yrc_with_translation_merges_and_serializes() {
    let yrc = "[12580,3470](12580,250,0)难(12830,300,0)以";

    let document = parse(&RawLyrics {
        translated_lrc: Some("[00:12.58]Hard to".to_owned()),
        yrc: Some(yrc.to_owned()),
        ..RawLyrics::default()
    });

    assert_eq!(document.lines.len(), 1);
    assert_eq!(document.lines[0].translation.as_deref(), Some("Hard to"));
    assert_eq!(
        enhanced(&document),
        [
            "[00:12.580]<00:12.580>难<00:12.830>以<00:13.130>",
            "[00:12.580]Hard to"
        ]
        .join("\n")
    );
}

#[test]
fn enhanced_falls_back_to_plain_when_no_word_level() {
    let document = parse(&raw("[00:01.00]只有行时间\n[00:02.00]第二行"));

    assert_eq!(plain(&document), enhanced(&document));
}

#[test]
fn plain_lrc_supports_bilingual_and_ignores_slash_placeholders() {
    let document = parse(&RawLyrics {
        lrc: Some("[00:01.00]line one\n[00:02.00]line two".to_owned()),
        translated_lrc: Some("[00:01.00]翻译一\n[00:02.00]//".to_owned()),
        ..RawLyrics::default()
    });

    let bilingual = to_plain_lrc(&document, true);
    let original = to_plain_lrc(&document, false);

    assert!(bilingual.contains("翻译一"));
    assert!(!original.contains("翻译一"));
    assert!(!bilingual.contains("//"));
    assert!(bilingual.contains("[00:02.000]line two"));
    assert!(!bilingual.contains("[00:02.000]//"));
}

#[test]
fn empty_input_returns_an_empty_document() {
    let document = parse(&RawLyrics::default());

    assert!(document.is_empty());
    assert!(!document.has_word_level());
    assert!(document.lines.is_empty());
    assert_eq!(document.translation, None);
    assert_eq!(document.romanization, None);
    assert_eq!(plain(&document), "");
    assert_eq!(enhanced(&document), "");
}

#[test]
fn romanization_is_exposed_on_the_document() {
    let romanized = "[00:01.00]roman line";

    let document = parse(&RawLyrics {
        lrc: Some("[00:01.00]原文".to_owned()),
        romanized_lrc: Some(romanized.to_owned()),
        ..RawLyrics::default()
    });

    assert_eq!(document.romanization.as_deref(), Some(romanized));
    assert_eq!(document.lines.len(), 1);
}

#[test]
fn lrc_with_json_flavor_credit_lines_parses_them() {
    let lrc =
        "{\"t\":0,\"c\":[{\"tx\":\"作词: \"},{\"tx\":\"张国祥\"}]}\n[00:12.57]难以忘记初次见你";

    let document = parse(&raw(lrc));

    assert_eq!(document.lines.len(), 2);
    assert_eq!(document.lines[0].text, "作词: 张国祥");
    assert_eq!(document.lines[0].start_ms, 0);
    assert_eq!(document.lines[1].start_ms, 12570);
    assert_eq!(document.lines[1].text, "难以忘记初次见你");
}

#[test]
fn word_tags_below_the_line_start_are_shifted() {
    // 增强 LRC 的词标签若小于行起始时间，按相对时间处理。
    let document = parse(&raw("[00:10.00]<00:00.50>甲<00:01.00>乙"));

    assert_eq!(document.lines.len(), 1);
    let line = &document.lines[0];
    assert_eq!(line.start_ms, 10000);
    assert_eq!(line.words[0].start_ms, 10500);
    assert_eq!(line.words[1].start_ms, 11000);
}

// ===== QrcLyricParserTests =====

const SAMPLE_QRC: &str = "[11720,3670]看(11720,370)着(12090,310)你(12400,290)\n[15400,2000]微笑(15400,500)的脸(15900,600)";

#[test]
fn qrc_words_use_absolute_timestamps() {
    let document = parse(&with_qrc(SAMPLE_QRC));

    assert!(document.has_word_level());
    assert_eq!(document.lines.len(), 2);
    let first = &document.lines[0];
    assert_eq!(first.start_ms, 11720);
    assert_eq!(first.duration_ms, 3670);
    assert_eq!(first.text, "看着你");
    assert!(first.has_word_timing());
    assert_eq!(
        first.words.iter().map(|w| w.start_ms).collect::<Vec<_>>(),
        [11720, 12090, 12400]
    );
    assert_eq!(
        first
            .words
            .iter()
            .map(|w| w.duration_ms)
            .collect::<Vec<_>>(),
        [370, 310, 290]
    );
    assert_eq!(first.words[0].text, "看");
    assert_eq!(first.words[2].text, "你");
}

#[test]
fn qrc_envelope_unwraps_and_decodes_entities() {
    let qrc = "<Lyric_1 LyricType=\"1\" LyricContent=\"[1000,500]Tom &amp; Jerry(1000,500)\"/>";

    let document = parse(&with_qrc(qrc));

    assert_eq!(document.lines.len(), 1);
    let line = &document.lines[0];
    assert_eq!(line.text, "Tom & Jerry");
    assert_eq!(line.words[0].start_ms, 1000);
    assert_eq!(line.words[0].duration_ms, 500);
}

#[test]
fn qrc_skips_optional_word_brackets() {
    let document = parse(&with_qrc("[2000,500][2000,200]嘿(2000,200)呀(2200,300)"));

    assert_eq!(document.lines.len(), 1);
    let line = &document.lines[0];
    assert_eq!(line.text, "嘿呀");
    assert_eq!(line.words.len(), 2);
    assert_eq!(line.words[0].start_ms, 2000);
    assert_eq!(line.words[1].start_ms, 2200);
}

#[test]
fn qrc_takes_priority_over_yrc_and_lrc() {
    let document = parse(&RawLyrics {
        lrc: Some("[00:99.00]普通LRC".to_owned()),
        yrc: Some("[1000,500]逐字YRC".to_owned()),
        qrc: Some("[2000,300]逐字QRC(2000,300)".to_owned()),
        ..RawLyrics::default()
    });

    assert_eq!(document.lines.len(), 1);
    let line = &document.lines[0];
    assert_eq!(line.start_ms, 2000);
    assert_eq!(line.text, "逐字QRC");
    assert!(line.has_word_timing());
}

#[test]
fn qrc_mixed_plain_lrc_lines_fall_back() {
    let document = parse(&with_qrc(
        "[ti:标题]\n[00:01.00]plain line\n[2000,500]逐字(2000,250)行(2250,250)",
    ));

    assert_eq!(document.lines.len(), 2);
    assert_eq!(document.lines[0].text, "plain line");
    assert!(!document.lines[0].has_word_timing());
    assert_eq!(document.lines[1].text, "逐字行");
    assert!(document.lines[1].has_word_timing());
}

#[test]
fn enhanced_lrc_from_qrc_emits_start_and_end_tags() {
    let document = parse(&with_qrc(SAMPLE_QRC));

    assert_eq!(
        to_enhanced_lrc(&document, false),
        [
            "[00:11.720]<00:11.720>看<00:12.090>着<00:12.400>你<00:12.690>",
            "[00:15.400]<00:15.400>微笑<00:15.900>的脸<00:16.500>"
        ]
        .join("\n")
    );
}

#[test]
fn qrc_with_translation_merges_the_translation() {
    let document = parse(&RawLyrics {
        translated_lrc: Some("[00:01.00]你好世界".to_owned()),
        qrc: Some("[1000,1000]Hello(1000,500) world(1500,500)".to_owned()),
        ..RawLyrics::default()
    });

    assert_eq!(document.lines.len(), 1);
    assert_eq!(document.lines[0].text, "Hello world");
    assert_eq!(document.lines[0].translation.as_deref(), Some("你好世界"));
}

// ===== 额外的边界用例 =====

#[test]
fn crlf_and_bare_cr_line_endings_are_equivalent_to_lf() {
    let lf = parse(&raw("[00:01.00]甲\n[00:02.00]乙\n[00:03.00]丙"));
    let crlf = parse(&raw("[00:01.00]甲\r\n[00:02.00]乙\r\n[00:03.00]丙"));
    let cr = parse(&raw("[00:01.00]甲\r[00:02.00]乙\r[00:03.00]丙"));

    assert_eq!(lf, crlf);
    assert_eq!(lf, cr);
}

#[test]
fn lines_are_sorted_stably_by_start_time() {
    let document = parse(&raw("[00:05.00]后\n[00:01.00]先一\n[00:01.00]先二"));

    assert_eq!(texts(&document), ["先一", "先二", "后"]);
}

#[test]
fn timestamp_forms_and_offsets_clamp_at_zero() {
    let document = parse(&raw("[offset:+500]\n[00:00.10]a\n[1:2:3]b\n[0001:02]c"));

    // [1:2:3] 的分隔符可以是 ':'，小数部分 "3" 按十分位 = 300ms；偏移 +500 对所有标签生效。
    assert_eq!(starts(&document), [600, 62_500, 62_800]);
    assert_eq!(texts(&document), ["a", "c", "b"]);

    let clamped = parse(&raw("[offset:-5000]\n[00:01.00]x"));
    assert_eq!(starts(&clamped), [0]);
}

#[test]
fn malformed_timestamps_are_not_lines() {
    let document = parse(&raw(
        "[00:1x.00]坏\n[abc]也坏\n[12345:00]分钟超过四位\n没有标签\n[00:01.00]好",
    ));

    assert_eq!(texts(&document), ["好"]);
}

#[test]
fn leading_whitespace_before_a_timestamp_is_allowed() {
    let document = parse(&raw("  \t[00:01.00] 前面有空白"));

    assert_eq!(starts(&document), [1000]);
    assert_eq!(texts(&document), ["前面有空白"]);
}

#[test]
fn blank_text_lines_keep_their_timestamp() {
    let document = parse(&raw("[00:01.00]甲\n[00:02.00]\n[00:03.00]乙"));

    assert_eq!(texts(&document), ["甲", "", "乙"]);
    assert_eq!(
        plain(&document),
        "[00:01.000]甲\n[00:02.000]\n[00:03.000]乙"
    );
}

#[test]
fn translation_outside_the_tolerance_is_ignored() {
    let document = parse(&RawLyrics {
        lrc: Some("[00:10.00]原文".to_owned()),
        translated_lrc: Some("[00:11.00]太远了\n[00:10.80]刚好".to_owned()),
        ..RawLyrics::default()
    });

    // 800ms 是闭区间；1000ms 不匹配。
    assert_eq!(document.lines[0].translation.as_deref(), Some("刚好"));

    let far = parse(&RawLyrics {
        lrc: Some("[00:10.00]原文".to_owned()),
        translated_lrc: Some("[00:11.00]太远了".to_owned()),
        ..RawLyrics::default()
    });
    assert_eq!(far.lines[0].translation, None);
    assert_eq!(far.translation.as_deref(), Some("[00:11.00]太远了"));
}

#[test]
fn each_translation_line_is_used_at_most_once() {
    let document = parse(&RawLyrics {
        lrc: Some("[00:10.00]一\n[00:10.20]二".to_owned()),
        translated_lrc: Some("[00:10.10]唯一的译文".to_owned()),
        ..RawLyrics::default()
    });

    let translated: Vec<_> = document
        .lines
        .iter()
        .filter(|line| line.translation.is_some())
        .collect();
    assert_eq!(translated.len(), 1);
}

#[test]
fn whitespace_only_translation_is_not_kept_on_the_document() {
    let document = parse(&RawLyrics {
        lrc: Some("[00:01.00]甲".to_owned()),
        translated_lrc: Some("  \n ".to_owned()),
        romanized_lrc: Some("".to_owned()),
        ..RawLyrics::default()
    });

    assert_eq!(document.translation, None);
    assert_eq!(document.romanization, None);
    assert_eq!(document.lines[0].translation, None);
}

#[test]
fn yrc_text_with_parentheses_is_split_at_the_next_open_parenthesis() {
    // 与 C# 一致：词文本在下一个 '(' 处截断，含括号的歌词会被拆开。
    let document = parse(&with_yrc("[0,1000](0,500,0)你(好)(500,500,0)吗"));

    assert_eq!(document.lines[0].words.len(), 2);
    assert_eq!(document.lines[0].words[0].text, "你");
    assert_eq!(document.lines[0].words[1].text, "吗");
}

#[test]
fn yrc_header_with_an_out_of_range_number_falls_back_to_lrc_parsing() {
    let document = parse(&with_yrc("[9999999999,10]溢出\n[00:01.00]正常"));

    assert_eq!(texts(&document), ["正常"]);
}

#[test]
fn json_line_time_accepts_strings_and_floats() {
    let yrc = "{\"t\":\"2000\",\"c\":[{\"tx\":\"字符串\"}]}\n{\"t\":3000.9,\"c\":[{\"tx\":\"浮点\"},{\"x\":1},{\"tx\":5}]}\n{\"c\":[]}\n{坏json";

    let document = parse(&with_yrc(yrc));

    assert_eq!(starts(&document), [2000, 3000]);
    assert_eq!(texts(&document), ["字符串", "浮点"]);
}

#[test]
fn qrc_line_without_word_tags_strips_stray_pairs() {
    let document = parse(&with_qrc("[1000,500]纯文本[3,4]带残留"));

    assert_eq!(document.lines.len(), 1);
    assert_eq!(document.lines[0].text, "纯文本带残留");
    assert!(!document.lines[0].has_word_timing());
}

#[test]
fn qrc_envelope_without_a_closing_tag_is_used_as_is() {
    let document = parse(&with_qrc("<Lyric_1 LyricContent=\"[1000,500]甲(1000,500)"));

    // 外壳不完整：整段当普通文本处理，行头不在行首，所以没有可解析的行。
    assert!(document.is_empty());
}

#[test]
fn enhanced_lrc_skips_empty_words_and_flattens_newlines_in_words() {
    let document = LyricDocument {
        lines: vec![LyricLine {
            start_ms: 1000,
            duration_ms: 2000,
            text: "ab".to_owned(),
            words: vec![
                LyricWord {
                    start_ms: 1000,
                    duration_ms: 500,
                    text: "a\nb".to_owned(),
                },
                LyricWord {
                    start_ms: 1500,
                    duration_ms: 500,
                    text: String::new(),
                },
                LyricWord {
                    start_ms: 2500,
                    duration_ms: -30,
                    text: "c".to_owned(),
                },
            ],
            translation: None,
        }],
        ..LyricDocument::default()
    };

    assert_eq!(
        enhanced(&document),
        "[00:01.000]<00:01.000>a b<00:01.500><00:02.500>c<00:02.500>"
    );
}

#[test]
fn multiline_text_is_split_into_separate_timestamped_lines() {
    let document = LyricDocument {
        lines: vec![LyricLine {
            start_ms: 61_005,
            duration_ms: 0,
            text: "第一行\n 第二行 ".to_owned(),
            words: Vec::new(),
            translation: Some("译一\r\n译二".to_owned()),
        }],
        ..LyricDocument::default()
    };

    assert_eq!(
        plain(&document),
        "[01:01.005]第一行\n[01:01.005]第二行\n[01:01.005]译一\n[01:01.005]译二"
    );
}

#[test]
fn format_time_clamps_negatives_and_keeps_long_minutes() {
    assert_eq!(format_time(-5), "00:00.000");
    assert_eq!(format_time(0), "00:00.000");
    assert_eq!(format_time(59_999), "00:59.999");
    assert_eq!(format_time(6_000_000), "100:00.000");
}

#[test]
fn extreme_numbers_never_panic() {
    let yrc = "[2147483647,2147483647](2147483647,2147483647,0)甲(0,2147483647,0)乙";
    let document = parse(&with_yrc(yrc));
    let _ = to_enhanced_lrc(&document, true);
    let _ = to_plain_lrc(&document, true);

    let lrc = "[offset:2147483647]\n[9999:99.999]x\n[00:00.000]<9999:99.999>y";
    let document = parse(&raw(lrc));
    let _ = to_enhanced_lrc(&document, true);
}

#[test]
fn json_time_beyond_the_int_range_follows_the_dotnet8_double_cast() {
    // .NET 8 / x64 把越界 double 转 int 得到 int.MinValue，再被钳到 0，所以这一行排在最前。
    let yrc =
        "{\"t\":1000,\"c\":[{\"tx\":\"正常\"}]}\n{\"t\":99999999999,\"c\":[{\"tx\":\"巨大\"}]}";

    let document = parse(&with_yrc(yrc));

    assert_eq!(texts(&document), ["巨大", "正常"]);
    assert_eq!(starts(&document), [0, 1000]);
    assert_eq!(double_to_i32_like_dotnet8(-1.9), -1);
    assert_eq!(double_to_i32_like_dotnet8(2_147_483_647.9), i32::MAX);
    assert_eq!(double_to_i32_like_dotnet8(2_147_483_648.0), i32::MIN);
    assert_eq!(double_to_i32_like_dotnet8(f64::NAN), i32::MIN);
}
