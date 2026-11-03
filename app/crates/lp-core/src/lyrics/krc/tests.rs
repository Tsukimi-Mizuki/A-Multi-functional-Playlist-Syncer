//! 移植自 C# `KrcLyricsParserTests` 与 `KrcFixture`（夹具按 KRC 格式逆运算生成测试数据）。

use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use super::*;

/// 按酷狗 KRC 的格式逆算法构造测试数据：明文 `krc1` 魔数 + XOR 混淆的 zlib 流，再 base64。
fn encrypt_to_base64(text: &str, with_magic: bool) -> String {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(text.as_bytes()).unwrap();
    let compressed = encoder.finish().unwrap();

    let mut encrypted: Vec<u8> = if with_magic {
        MAGIC.to_vec()
    } else {
        Vec::new()
    };
    encrypted.extend(
        compressed
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ XOR_KEY[index % XOR_KEY.len()]),
    );
    base64::engine::general_purpose::STANDARD.encode(encrypted)
}

/// 生成 `[language:…]` 头（`type == 1` 的逐行译文）。
fn language_header(translations: &[&str]) -> String {
    let lines = translations
        .iter()
        .map(|text| format!("[\"{text}\"]"))
        .collect::<Vec<_>>()
        .join(",");
    let json = format!("{{\"content\":[{{\"type\":1,\"lyricContent\":[{lines}]}}]}}");
    format!(
        "[language:{}]",
        base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
    )
}

fn build_sample() -> String {
    [
        "[ti:テスト]",
        "[ar:歌手]",
        "[id:$00000000]",
        &language_header(&["你好 ", "第二行译"]),
        "[1000,2000]<0,500,0>Hello <500,500,0>world<1000,1000,0>!",
        "[3000,1500]<0,750,0>第二<750,750,0>行",
    ]
    .join("\n")
}

#[test]
fn decrypt_base64_round_trips_krc_text() {
    let text = build_sample();

    assert_eq!(
        decrypt_base64(Some(&encrypt_to_base64(&text, true))).as_deref(),
        Some(text.as_str())
    );
}

#[test]
fn decrypt_base64_handles_legacy_format_without_magic() {
    let text = build_sample();

    assert_eq!(
        decrypt_base64(Some(&encrypt_to_base64(&text, false))).as_deref(),
        Some(text.as_str())
    );
}

#[test]
fn decrypt_base64_returns_none_for_invalid_input() {
    assert_eq!(decrypt_base64(None), None);
    assert_eq!(decrypt_base64(Some("   ")), None);
    assert_eq!(decrypt_base64(Some("not-base64!!")), None);
    // 32 个零字节：XOR 后不是合法 zlib，也不含 '['，按明文兜底也失败。
    assert_eq!(
        decrypt_base64(Some(
            &base64::engine::general_purpose::STANDARD.encode([0_u8; 32])
        )),
        None
    );
}

#[test]
fn decrypt_handles_empty_and_magic_only_streams() {
    assert_eq!(decrypt(&[]), None);
    assert_eq!(decrypt(b"krc1"), None);
}

#[test]
fn decrypt_falls_back_to_the_xored_bytes_as_text_when_zlib_fails_but_a_bracket_appears() {
    // 与 C# 一致：兜底用的是已经 XOR 过的字节；构造一段 XOR 后恰好含 '[' 的数据。
    let wanted = b"[ti:x]\n[0,1]<0,1,0>a";
    let encrypted: Vec<u8> = wanted
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ XOR_KEY[index % XOR_KEY.len()])
        .collect();

    // 没有魔数、也不是合法 zlib，所以兜底按明文解释。
    assert_eq!(
        decrypt(&encrypted).as_deref(),
        Some("[ti:x]\n[0,1]<0,1,0>a")
    );
}

#[test]
fn decrypt_strips_a_utf8_bom() {
    let mut text = vec![0xEF, 0xBB, 0xBF];
    text.extend_from_slice("[0,1]<0,1,0>字".as_bytes());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&text).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut encrypted = MAGIC.to_vec();
    encrypted.extend(
        compressed
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ XOR_KEY[index % XOR_KEY.len()]),
    );

    assert_eq!(decrypt(&encrypted).as_deref(), Some("[0,1]<0,1,0>字"));
}

#[test]
fn parse_builds_word_timing_and_translations() {
    let document = parse(Some(&build_sample()));

    assert!(!document.is_empty());
    assert!(document.has_word_level());
    assert_eq!(document.lines.len(), 2);

    let first = &document.lines[0];
    assert_eq!(first.start_ms, 1000);
    assert_eq!(first.duration_ms, 2000);
    assert_eq!(first.text, "Hello world!");
    assert_eq!(first.translation.as_deref(), Some("你好"));
    assert_eq!(first.words.len(), 3);
    assert_eq!(first.words[0].start_ms, 1000);
    assert_eq!(first.words[0].duration_ms, 500);
    assert_eq!(first.words[0].text, "Hello ");
    assert_eq!(first.words[1].start_ms, 1500);
    assert_eq!(first.words[1].text, "world");
    assert_eq!(first.words[2].start_ms, 2000);
    assert_eq!(first.words[2].text, "!");

    let second = &document.lines[1];
    assert_eq!(second.start_ms, 3000);
    assert_eq!(second.text, "第二行");
    assert_eq!(second.translation.as_deref(), Some("第二行译"));
}

#[test]
fn parse_skips_metadata_and_blank_lines() {
    let text = [
        "[ti:标题]",
        "[ar:歌手]",
        "[id:$00000000]",
        "[offset:0]",
        "",
        "[0,1000]<0,400,0>只<400,600,0>有一行",
    ]
    .join("\n");

    let document = parse(Some(&text));

    assert_eq!(document.lines.len(), 1);
    assert_eq!(document.lines[0].text, "只有一行");
    assert_eq!(document.lines[0].translation, None);
}

#[test]
fn parse_returns_an_empty_document_for_blank_text() {
    assert!(parse(None).is_empty());
    assert!(parse(Some("   ")).is_empty());
    assert!(parse(Some("[ti:只有元数据]")).is_empty());
}

#[test]
fn parse_handles_crlf_and_lines_without_word_tags() {
    let text = "[0,1000]没有词标签的行\r\n[1000,1000]<0,500,0>有<500,500,0>词\r\n";

    let document = parse(Some(text));

    assert_eq!(document.lines.len(), 2);
    assert_eq!(document.lines[0].text, "没有词标签的行");
    assert!(!document.lines[0].has_word_timing());
    assert_eq!(document.lines[1].text, "有词");
    assert_eq!(document.lines[1].words[1].start_ms, 1500);
}

#[test]
fn translations_follow_the_accepted_line_index_and_missing_ones_are_none() {
    let header = language_header(&["译一", "", "译三"]);
    let text = [
        header.as_str(),
        "[ti:不算行]",
        "[0,1]<0,1,0>一",
        "[1,1]<0,1,0>二",
        "[2,1]<0,1,0>三",
        "[3,1]<0,1,0>四(没有译文)",
    ]
    .join("\n");

    let document = parse(Some(&text));

    let translations: Vec<_> = document
        .lines
        .iter()
        .map(|line| line.translation.as_deref())
        .collect();
    assert_eq!(translations, [Some("译一"), None, Some("译三"), None]);
}

#[test]
fn language_header_with_other_track_types_is_ignored() {
    // type 0 是逐字罗马音，C# 只读 type 1。
    let json = r#"{"content":[{"type":0,"lyricContent":[["a","b"]]}]}"#;
    let header = format!(
        "[language:{}]",
        base64::engine::general_purpose::STANDARD.encode(json)
    );
    let text = [header.as_str(), "[0,1]<0,1,0>字"].join("\n");

    assert_eq!(parse(Some(&text)).lines[0].translation, None);
}

#[test]
fn language_header_picks_the_first_type_1_track_and_flattens_nested_arrays() {
    let json = r#"{"content":[{"type":0,"lyricContent":[["x"]]},{"type":1,"lyricContent":[["你","好"],"  世界  ",[],null,7]}]}"#;
    let header = format!(
        "[language:{}]",
        base64::engine::general_purpose::STANDARD.encode(json)
    );
    let text = [
        header.as_str(),
        "[0,1]<0,1,0>a",
        "[1,1]<0,1,0>b",
        "[2,1]<0,1,0>c",
        "[3,1]<0,1,0>d",
        "[4,1]<0,1,0>e",
    ]
    .join("\n");

    let document = parse(Some(&text));

    let translations: Vec<_> = document
        .lines
        .iter()
        .map(|line| line.translation.as_deref())
        .collect();
    assert_eq!(translations, [Some("你好"), Some("世界"), None, None, None]);
}

#[test]
fn broken_language_headers_mean_no_translations() {
    for payload in [
        "!!!!".to_owned(),
        base64::engine::general_purpose::STANDARD.encode("不是 json"),
        base64::engine::general_purpose::STANDARD.encode(r#"{"nocontent":1}"#),
        base64::engine::general_purpose::STANDARD.encode(r#"{"content":[1]}"#),
        base64::engine::general_purpose::STANDARD.encode(r#"{"content":[{"type":1.5}]}"#),
        base64::engine::general_purpose::STANDARD.encode(r#"{"content":[{"type":1}]}"#),
    ] {
        let text = format!("[language:{payload}]\n[0,1]<0,1,0>字");
        let document = parse(Some(&text));
        assert_eq!(document.lines.len(), 1, "{payload}");
        assert_eq!(document.lines[0].translation, None, "{payload}");
    }
}

#[test]
fn base64_decoding_matches_dotnet_convert_from_base64_string() {
    // 规范填充必须齐全；末位残留的非零比特被接受（.NET 不拒绝）。
    assert!(decode_base64("YQ==").is_some());
    assert!(decode_base64("YQ").is_none());
    assert!(decode_base64("YR==").is_some());
    assert!(decode_base64("Y===").is_none());
}
