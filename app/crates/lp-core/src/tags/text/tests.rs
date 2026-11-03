use super::*;
use crate::ncm::client_comment::build_comment;

#[test]
fn null_if_blank_trims_and_drops_blank() {
    assert_eq!(null_if_blank(Some("  a b  ")).as_deref(), Some("a b"));
    assert_eq!(null_if_blank(Some(" \t\r\n")), None);
    assert_eq!(null_if_blank(Some("")), None);
    assert_eq!(null_if_blank(None), None);
}

#[test]
fn join_values_trims_skips_blank_and_uses_slash() {
    assert_eq!(join_values([" A ", "", "  ", "B"]).as_deref(), Some("A/B"));
    assert_eq!(join_values(["x"]).as_deref(), Some("x"));
    assert_eq!(join_values([" ", ""]), None);
    assert_eq!(join_values([]), None);
}

#[test]
fn split_values_handles_every_separator() {
    assert_eq!(
        split_values(Some(" A/B;C；D\r\nE\nF ")),
        ["A", "B", "C", "D", "E", "F"]
    );
    assert_eq!(split_values(Some("//;; ")), Vec::<String>::new());
    assert_eq!(split_values(Some("   ")), Vec::<String>::new());
    assert_eq!(split_values(None), Vec::<String>::new());
    // 中文全角斜杠不是分隔符。
    assert_eq!(split_values(Some("A／B")), ["A／B"]);
}

#[test]
fn clean_values_drops_blank_and_trims() {
    let values = vec![
        " A ".to_owned(),
        String::new(),
        "  ".to_owned(),
        "B".to_owned(),
    ];
    assert_eq!(clean_values(&values), ["A", "B"]);
}

#[test]
fn clean_comment_strips_leftover_separators() {
    assert_eq!(clean_comment("  ; , | 备注 ;\n"), "备注");
    assert_eq!(clean_comment(";;;"), "");
    assert_eq!(clean_comment("a; b"), "a; b");
    assert_eq!(clean_comment("\0 x \0"), "x");
}

#[test]
fn strip_netease_marker_removes_marker_and_bare_word() {
    assert_eq!(
        strip_netease_marker("原始备注; LunaticPrincess:186016"),
        "原始备注; "
    );
    assert_eq!(strip_netease_marker("lunaticprincess : 42"), "");
    assert_eq!(strip_netease_marker("LunaticPrincess"), "");
    assert_eq!(strip_netease_marker("没有标记"), "没有标记");
}

#[test]
fn clean_display_comment_hides_the_marker() {
    assert_eq!(
        clean_display_comment(Some("原始备注; LunaticPrincess:186016")).as_deref(),
        Some("原始备注")
    );
    assert_eq!(clean_display_comment(Some("LunaticPrincess:186016")), None);
    assert_eq!(clean_display_comment(Some("  ")), None);
    assert_eq!(clean_display_comment(None), None);
    // 客户端的 163 key 不属于本程序的标记，原样展示。
    let client = build_comment(r#"{"musicId":1}"#);
    assert_eq!(
        clean_display_comment(Some(&client)).as_deref(),
        Some(client.as_str())
    );
}

#[test]
fn parse_netease_id_prefers_the_marker() {
    assert_eq!(
        parse_netease_id(Some("LunaticPrincess:186016")),
        Some(186016)
    );
    assert_eq!(
        parse_netease_id(Some("备注; lunaticprincess : 42 ")),
        Some(42)
    );
    assert_eq!(parse_netease_id(Some("LunaticPrincess:0042")), Some(42));
    assert_eq!(parse_netease_id(Some("LunaticPrincess")), None);
    assert_eq!(parse_netease_id(Some("   ")), None);
    assert_eq!(parse_netease_id(None), None);
}

#[test]
fn parse_netease_id_falls_back_to_the_client_comment() {
    let client = build_comment(r#"{"musicId":33894312}"#);
    assert_eq!(parse_netease_id(Some(&client)), Some(33894312));
    // 两者都有时本程序的标记优先。
    let both = format!("{client}; LunaticPrincess:7");
    assert_eq!(parse_netease_id(Some(&both)), Some(7));
}

#[test]
fn parse_netease_id_overflow_means_no_id_and_skips_the_fallback() {
    // 与 C# 一致：标记存在但数字放不进 long（最多 19 位，仍可能溢出）时返回 null，不回落到 163 key。
    let client = build_comment(r#"{"musicId":5}"#);
    let comment = format!("LunaticPrincess:9999999999999999999 {client}");
    assert_eq!(parse_netease_id(Some(&comment)), None);
}

#[test]
fn detect_image_mime_recognises_common_formats() {
    assert_eq!(
        detect_image_mime(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0]),
        "image/png"
    );
    assert_eq!(detect_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), "image/jpeg");
    assert_eq!(detect_image_mime(b"GIF89a"), "image/gif");
    assert_eq!(detect_image_mime(b"BM\x00\x00"), "image/bmp");
    assert_eq!(
        detect_image_mime(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
        "image/webp"
    );
    assert_eq!(detect_image_mime(b"RIFF\x00\x00\x00\x00WAVE"), "image/jpeg");
    assert_eq!(detect_image_mime(&[]), "image/jpeg");
    assert_eq!(detect_image_mime(b"??"), "image/jpeg");
}
