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

/// 只有文件头的 PNG：签名 + IHDR。
fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    data.extend_from_slice(b"IHDR");
    data.extend_from_slice(&width.to_be_bytes());
    data.extend_from_slice(&height.to_be_bytes());
    data.extend_from_slice(&[8, 6, 0, 0, 0]);
    data
}

/// 只有文件头的 JPEG：APP0、DHT 之后才是帧头，`sof` 是帧头标记（C0 基线 / C2 渐进）。
fn jpeg_header(sof: u8, width: u16, height: u16) -> Vec<u8> {
    let mut data = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    data.extend_from_slice(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
    data.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x04, 0x00, 0x00]);
    data.extend_from_slice(&[0xFF, sof, 0x00, 0x11, 0x08]);
    data.extend_from_slice(&height.to_be_bytes());
    data.extend_from_slice(&width.to_be_bytes());
    data.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    data
}

#[test]
fn image_dimensions_reads_png_and_jpeg_headers() {
    assert_eq!(
        image_dimensions(&png_header(4000, 4000)),
        Some((4000, 4000))
    );
    assert_eq!(
        image_dimensions(&png_header(1663, 1483)),
        Some((1663, 1483))
    );
    // DHT 段（C4）不是帧头，要跳过；渐进式 JPEG 的帧头是 C2。
    assert_eq!(
        image_dimensions(&jpeg_header(0xC0, 1200, 1200)),
        Some((1200, 1200))
    );
    assert_eq!(
        image_dimensions(&jpeg_header(0xC2, 3000, 2000)),
        Some((3000, 2000))
    );
}

#[test]
fn image_dimensions_reads_gif_bmp_and_webp_headers() {
    let mut gif = b"GIF89a".to_vec();
    gif.extend_from_slice(&[0x20, 0x03, 0x58, 0x02]);
    assert_eq!(image_dimensions(&gif), Some((800, 600)));

    // BMP 高度为负表示自上而下存储。
    let mut bmp = b"BM".to_vec();
    bmp.extend_from_slice(&[0; 16]);
    bmp.extend_from_slice(&500_i32.to_le_bytes());
    bmp.extend_from_slice(&(-400_i32).to_le_bytes());
    assert_eq!(image_dimensions(&bmp), Some((500, 400)));

    let mut webp = b"RIFF\0\0\0\0WEBPVP8X\x0A\0\0\0\0\0\0\0".to_vec();
    webp.extend_from_slice(&[0xCF, 0x07, 0x00, 0x2B, 0x02, 0x00]);
    assert_eq!(image_dimensions(&webp), Some((2000, 556)));
}

#[test]
fn image_dimensions_rejects_unknown_or_truncated_data() {
    assert_eq!(image_dimensions(&[]), None);
    assert_eq!(image_dimensions(b"not an image"), None);
    assert_eq!(image_dimensions(&png_header(0, 10)), None);
    let jpeg = jpeg_header(0xC0, 1200, 1200);
    // 帧头被截断。
    assert_eq!(image_dimensions(&jpeg[..jpeg.len() - 12]), None);
    // 段之间不是 FF 开头（数据损坏）。
    let mut broken = jpeg.clone();
    broken[20] = 0x00;
    assert_eq!(image_dimensions(&broken), None);
}
