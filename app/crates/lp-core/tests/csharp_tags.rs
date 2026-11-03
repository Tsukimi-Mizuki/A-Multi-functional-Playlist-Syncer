//! 标签读写与 C# 版（TagLibSharp 2.3.0）的交叉兼容测试。
//!
//! `tests/fixtures/tags/csharp-*` 是临时 C# 探针（已删除、不入库）调用现有 `AudioTagService` 写出的文件：
//! 基础样本是静音的 MP3 / FLAC / M4A / OGG / OPUS / WAV，`spec-full.json` 是写入的数据，
//! `csharp-expected.json` 是 C# 的 `AudioTagService.ReadAsync` 读回同一批文件的结果。
//! 这里用 Rust 读同一批文件，要求每个字段与 C# 读出的一致。反方向（Rust 写 → C# 读）无法在 Rust 的
//! 测试里跑，结论记录在 `docs/TAURI_REWRITE.md`。
//!
//! 夹具里没有任何真实歌曲：音频是静音，标签是手写的假数据，封面是 1×1 PNG。

use std::path::PathBuf;

use lp_core::tags::{AudioTagInfo, AudioTagService, LoftyTagService};
use serde_json::Value;

/// 1×1 透明 PNG（与探针里的 `png` 封面是同一份字节）。
const PNG_COVER: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("tags")
}

fn expected() -> Vec<Value> {
    let bytes = std::fs::read(fixture_dir().join("csharp-expected.json")).expect("读取期望值");
    match serde_json::from_slice(&bytes).expect("期望值是合法 JSON") {
        Value::Array(items) => items,
        other => panic!("期望值应是数组：{other}"),
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(str::to_owned)
}

fn number(value: &Value, key: &str) -> Option<u32> {
    value[key].as_u64().map(|n| n as u32)
}

fn read(name: &str) -> AudioTagInfo {
    LoftyTagService
        .read(&fixture_dir().join(name))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .unwrap_or_else(|| panic!("{name}: 无法读取"))
}

#[test]
fn rust_reads_every_csharp_written_file_like_csharp_does() {
    let items = expected();
    assert_eq!(items.len(), 19, "6 种容器 × 3 个场景 + 客户端备注样本");

    for item in items {
        let name = item["file"].as_str().unwrap();
        let expected = &item["info"];
        let actual = read(name);

        assert_eq!(actual.title, text(expected, "title"), "{name} title");
        assert_eq!(actual.artist, text(expected, "artist"), "{name} artist");
        assert_eq!(actual.album, text(expected, "album"), "{name} album");
        assert_eq!(
            actual.album_artist,
            text(expected, "albumArtist"),
            "{name} albumArtist"
        );
        assert_eq!(actual.year, number(expected, "year"), "{name} year");
        assert_eq!(
            actual.track_number,
            number(expected, "trackNumber"),
            "{name} track"
        );
        assert_eq!(
            actual.disc_number,
            number(expected, "discNumber"),
            "{name} disc"
        );
        assert_eq!(actual.genre, text(expected, "genre"), "{name} genre");
        assert_eq!(
            actual.composer,
            text(expected, "composer"),
            "{name} composer"
        );
        assert_eq!(actual.comment, text(expected, "comment"), "{name} comment");
        assert_eq!(actual.lyrics, text(expected, "lyrics"), "{name} lyrics");
        assert_eq!(
            actual.has_cover,
            expected["hasCover"].as_bool().unwrap(),
            "{name} hasCover"
        );
        assert_eq!(
            actual.cover_data.as_ref().map(Vec::len),
            expected["coverLen"].as_u64().map(|n| n as usize),
            "{name} coverLen"
        );
        if actual.has_cover {
            assert_eq!(
                actual.cover_data.as_deref(),
                Some(&PNG_COVER[..]),
                "{name} 封面字节"
            );
            assert_eq!(
                actual.cover_mime_type.as_deref(),
                Some("image/png"),
                "{name} 封面 MIME"
            );
        }
        assert_eq!(
            actual.file_size,
            expected["fileSize"].as_u64().unwrap(),
            "{name} size"
        );
        assert_eq!(
            text(expected, "fileFormat"),
            actual.file_format,
            "{name} format"
        );

        // 网易云 ID：C# 的 M4A 在“备注 + 标记”同时存在时会丢掉标记（TagLib 的 AppleTag 备注只取第一段），
        // Rust 能读到的不会比 C# 少；其余格式必须完全一致。
        let csharp_id = expected["neteaseId"].as_i64();
        if name.ends_with(".m4a") && csharp_id.is_none() {
            assert!(
                actual.netease_id.is_none() || actual.netease_id == Some(186016),
                "{name}"
            );
        } else {
            assert_eq!(actual.netease_id, csharp_id, "{name} neteaseId");
        }

        // 时长允许一点取整差异；码率只在两边都报告时比较（lofty 对 FLAC / Opus 报告更准确的值）。
        // M4A 样本的 AAC 帧是占位字节，lofty 按实际数据量算出约 2 kbps，TagLib 读 esds 里声明的 128 kbps，不比较。
        let csharp_ms = expected["durationMs"].as_i64().unwrap();
        assert!(
            (actual.duration_ms - csharp_ms).abs() <= 25,
            "{name} duration {} vs {csharp_ms}",
            actual.duration_ms
        );
        let csharp_bitrate = expected["bitrate"].as_i64().unwrap();
        if csharp_bitrate > 0 && actual.bitrate > 0 && !name.ends_with(".m4a") {
            assert!(
                (i64::from(actual.bitrate) - csharp_bitrate).abs() <= csharp_bitrate / 5 + 2,
                "{name} bitrate {} vs {csharp_bitrate}",
                actual.bitrate
            );
        }
    }
}

#[test]
fn csharp_client_comment_file_yields_the_music_id() {
    let info = read("csharp-client.mp3");
    assert_eq!(info.netease_id, Some(33894312));
    assert!(
        info.comment
            .as_deref()
            .is_some_and(|c| c.starts_with("163 key(Don't modify):")),
        "客户端注释原样展示"
    );
}

#[test]
fn csharp_written_flac_covers_are_native_picture_blocks() {
    // C# 的 FLAC 封面其实就是原生 PICTURE 块（TagLib 的 FlacMetadata），不是 METADATA_BLOCK_PICTURE。
    let bytes = std::fs::read(fixture_dir().join("csharp-full.flac")).unwrap();
    let mut pos = 4;
    let mut kinds = Vec::new();
    loop {
        let header = bytes[pos];
        let len = u32::from_be_bytes([0, bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as usize;
        kinds.push(header & 0x7F);
        pos += 4 + len;
        if header & 0x80 != 0 {
            break;
        }
    }
    assert!(kinds.contains(&6), "应有 PICTURE 块：{kinds:?}");
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("METADATA_BLOCK_PICTURE"));
}

#[test]
fn rust_can_rewrite_a_csharp_written_file_without_losing_anything() {
    use lp_core::tags::AudioTagData;

    let dir = std::env::temp_dir().join(format!("lp-csharp-tags-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for ext in ["mp3", "flac", "m4a", "ogg", "opus", "wav"] {
        let name = format!("csharp-full.{ext}");
        let copy = dir.join(&name);
        std::fs::copy(fixture_dir().join(&name), &copy).unwrap();
        let before = LoftyTagService.read(&copy).unwrap().unwrap();

        LoftyTagService
            .write(
                &copy,
                &AudioTagData {
                    title: Some("Rust 改的标题".into()),
                    ..Default::default()
                },
            )
            .unwrap();

        let after = LoftyTagService.read(&copy).unwrap().unwrap();
        assert_eq!(after.title.as_deref(), Some("Rust 改的标题"), "{ext}");
        assert_eq!(after.artist, before.artist, "{ext}");
        assert_eq!(after.album, before.album, "{ext}");
        assert_eq!(after.year, before.year, "{ext}");
        assert_eq!(after.genre, before.genre, "{ext}");
        assert_eq!(after.composer, before.composer, "{ext}");
        assert_eq!(after.comment, before.comment, "{ext}");
        assert_eq!(after.lyrics, before.lyrics, "{ext}");
        assert_eq!(after.netease_id, before.netease_id, "{ext}");
        assert_eq!(after.cover_data, before.cover_data, "{ext}");
        assert_eq!(after.track_number, before.track_number, "{ext}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
