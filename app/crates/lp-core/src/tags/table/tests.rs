use std::fs;
use std::path::PathBuf;

use super::*;
use crate::tags::fixtures::{self, PNG_COVER, png_cover_2};
use crate::tags::service::LoftyTagService;
use crate::test_util::TempDir;

fn service() -> LoftyTagService {
    LoftyTagService
}

fn create(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, bytes).unwrap();
    path
}

/// 写好一份“各字段都有值”的标签，返回文件路径。
fn tagged(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = create(dir, name, bytes);
    service()
        .write(
            &path,
            &AudioTagData {
                title: Some("晴天".into()),
                artists: vec!["周杰伦".into()],
                album: Some("叶惠美".into()),
                album_artist: Some("周杰伦".into()),
                track_number: Some(3),
                track_total: Some(11),
                disc_number: Some(1),
                year: Some(2003),
                genre: Some("Pop".into()),
                composer: Some("周杰伦".into()),
                lyricist: Some("方文山".into()),
                comment: Some("原始备注".into()),
                plain_lyrics: Some("\n  \n[00:01.00]第一行\n[00:02.00]第二行".into()),
                netease_id: Some(186016),
                cover_data: Some(PNG_COVER.to_vec()),
                ..Default::default()
            },
        )
        .unwrap();
    path
}

fn patch(json: serde_json::Value) -> AudioTagData {
    serde_json::from_value::<TagPatch>(json)
        .unwrap()
        .into_data()
        .unwrap()
}

fn read(path: &std::path::Path) -> AudioTagInfo {
    service().read(path).unwrap().unwrap()
}

// ───────────────────────── 列表行 ─────────────────────────

#[test]
fn rows_carry_the_tags_without_cover_bytes_and_keep_the_order() {
    let dir = TempDir::new();
    let bare = create(&dir, "b 无标签.mp3", &fixtures::mp3_default());
    let full = tagged(&dir, "a 晴天.flac", &fixtures::flac());

    let rows = read_rows(&service(), &[bare.clone(), full.clone()]);

    assert_eq!(rows.len(), 2);
    let row = &rows[1];
    assert_eq!(row.path, full.to_string_lossy());
    assert_eq!(row.file_name, "a 晴天.flac");
    assert_eq!(row.directory, dir.path().to_string_lossy());
    assert_eq!(row.file_format.as_deref(), Some("FLAC"));
    assert!(row.readable && row.error.is_none());
    assert_eq!(row.title.as_deref(), Some("晴天"));
    assert_eq!(row.artist.as_deref(), Some("周杰伦"));
    assert_eq!(row.album.as_deref(), Some("叶惠美"));
    assert_eq!(row.album_artist.as_deref(), Some("周杰伦"));
    assert_eq!(
        (row.year, row.track_number, row.disc_number),
        (Some(2003), Some(3), Some(1))
    );
    assert_eq!(row.genre.as_deref(), Some("Pop"));
    assert_eq!(row.composer.as_deref(), Some("周杰伦"));
    assert_eq!(row.lyricist.as_deref(), Some("方文山"));
    assert_eq!(row.comment.as_deref(), Some("原始备注"));
    assert_eq!(row.lyrics_preview.as_deref(), Some("[00:01.00]第一行"));
    assert!(row.has_lyrics && row.has_cover);
    assert_eq!(row.lyrics_file, None);

    let row = &rows[0];
    assert_eq!(row.file_name, "b 无标签.mp3");
    assert!(row.readable);
    assert_eq!(row.title, None);
    assert_eq!(row.lyrics_preview, None);
    assert!(!row.has_lyrics && !row.has_cover);
    assert_eq!(row.lyrics_file, None);
    assert!(row.duration_ms > 0);
}

#[test]
fn rows_without_embedded_lyrics_preview_the_lrc_file_next_to_them() {
    let dir = TempDir::new();
    let bare = create(&dir, "歌手 - 歌名.flac", &fixtures::flac());
    create(
        &dir,
        "歌手 - 歌名.lrc",
        "\u{feff}[ti:歌名]\n[00:01.00]文件里的第一句\n".as_bytes(),
    );
    // 内嵌歌词优先：旁边的 .lrc 不看。
    let embedded = tagged(&dir, "晴天.flac", &fixtures::flac());
    create(&dir, "晴天.lrc", "[00:01.00]旁边文件里的".as_bytes());
    // 读不出来的音频：不去读它的 .lrc。
    let broken = create(&dir, "坏的.mp3", b"not audio");
    create(&dir, "坏的.lrc", "[00:01.00]x".as_bytes());

    let rows = read_rows(&service(), &[bare.clone(), embedded, broken]);

    assert_eq!(rows[0].lyrics_preview.as_deref(), Some("[ti:歌名]"));
    assert!(rows[0].has_lyrics);
    assert_eq!(rows[0].lyrics_file.as_deref(), Some("歌手 - 歌名.lrc"));
    // 标签本身没有被改动：文件里仍然没有内嵌歌词。
    assert_eq!(read(&bare).lyrics, None);

    assert_eq!(rows[1].lyrics_preview.as_deref(), Some("[00:01.00]第一行"));
    assert_eq!(rows[1].lyrics_file, None);

    assert!(!rows[2].readable && !rows[2].has_lyrics);
    assert_eq!(rows[2].lyrics_file, None);
}

#[test]
fn unreadable_files_still_get_a_row_with_the_reason() {
    let dir = TempDir::new();
    let broken = create(&dir, "坏的.mp3", b"not audio at all");
    let missing = dir.path().join("不存在.flac");

    let rows = read_rows(&service(), &[broken, missing]);

    for (row, name, format) in [
        (&rows[0], "坏的.mp3", "MP3"),
        (&rows[1], "不存在.flac", "FLAC"),
    ] {
        assert_eq!(row.file_name, name);
        assert_eq!(row.file_format.as_deref(), Some(format));
        assert!(!row.readable);
        assert!(row.error.as_deref().unwrap().contains("无法读取标签"));
        assert_eq!(row.title, None);
    }
    assert!(read_rows(&service(), &[]).is_empty());
}

#[test]
fn lyrics_preview_is_cut_by_characters_and_rows_serialize_to_camel_case() {
    let long = "歌".repeat(300);
    assert_eq!(lyrics_preview(&long).unwrap().chars().count(), 120);
    assert_eq!(lyrics_preview(" \n\t\n"), None);

    let json = serde_json::to_value(TagRowView::unreadable(
        std::path::Path::new("D:/音乐/a.mp3"),
        "原因".into(),
    ))
    .unwrap();
    for key in [
        "fileName",
        "fileFormat",
        "albumArtist",
        "trackNumber",
        "discNumber",
        "lyricsPreview",
        "hasLyrics",
        "hasCover",
        "durationMs",
    ] {
        assert!(json.get(key).is_some(), "缺少 {key}");
    }
    assert_eq!(json["readable"], false);
    assert_eq!(json["error"], "原因");
}

// ───────────────────────── 改动的解析 ─────────────────────────

#[test]
fn patch_tells_absent_fields_from_cleared_ones() {
    let data = patch(serde_json::json!({
        "title": "  新标题 ", "album": null, "albumArtist": "   ", "year": 2024, "trackNumber": null,
        "discNumber": 0, "artists": [" A ", "", "B"], "genre": "Rock"
    }));

    assert_eq!(data.title.as_deref(), Some("新标题"));
    assert_eq!(data.artists, ["A", "B"]);
    assert_eq!(data.year, Some(2024));
    assert_eq!(data.genre.as_deref(), Some("Rock"));
    assert_eq!(
        (
            &data.album,
            &data.album_artist,
            data.track_number,
            data.disc_number
        ),
        (&None, &None, None, None)
    );
    assert_eq!(
        data.clear,
        [
            TagField::Album,
            TagField::AlbumArtist,
            TagField::TrackNumber,
            TagField::DiscNumber
        ]
    );
    // 没提到的字段既没有值，也不在清除列表里。
    assert_eq!(
        (&data.composer, &data.lyricist, &data.comment),
        (&None, &None, &None)
    );
    assert!(!data.replace_all && !data.remove_cover && data.cover_data.is_none());

    let data = patch(serde_json::json!({ "artists": [" "], "comment": "", "lyricist": "方文山" }));
    assert!(data.artists.is_empty());
    assert_eq!(data.clear, [TagField::Comment, TagField::Artist]);
    assert_eq!(data.lyricist.as_deref(), Some("方文山"));
}

#[test]
fn patch_handles_covers_and_rejects_empty_or_broken_requests() {
    let data = patch(serde_json::json!({
        "coverBase64": "iVBORw0KGgo=", "coverMimeType": "image/png", "removeCover": true
    }));
    assert_eq!(data.cover_data.as_deref().map(<[u8]>::len), Some(8));
    assert_eq!(data.cover_mime_type.as_deref(), Some("image/png"));
    assert!(!data.remove_cover, "给了新封面就不是移除");

    let data = patch(serde_json::json!({ "removeCover": true, "coverMimeType": "image/png" }));
    assert!(data.remove_cover);
    assert_eq!((data.cover_data, data.cover_mime_type), (None, None));

    for json in [
        serde_json::json!({}),
        serde_json::json!({ "coverBase64": "  " }),
        serde_json::json!({ "coverMimeType": "image/png" }),
    ] {
        let parsed: TagPatch = serde_json::from_value(json).unwrap();
        assert!(matches!(
            parsed.into_data(),
            Err(TagError::InvalidArgument("没有要修改的内容。"))
        ));
    }
    let parsed: TagPatch =
        serde_json::from_value(serde_json::json!({ "title": "T", "coverBase64": "***" })).unwrap();
    assert!(matches!(
        parsed.into_data(),
        Err(TagError::InvalidArgument("封面数据不是有效的 Base64。"))
    ));
    // 类型不对的请求在反序列化时就被拒绝。
    assert!(serde_json::from_value::<TagPatch>(serde_json::json!({ "year": "二零二四" })).is_err());
}

// ───────────────────────── 套用改动 ─────────────────────────

#[test]
fn applying_a_patch_only_touches_the_mentioned_fields_in_every_container() {
    let dir = TempDir::new();
    let files: Vec<PathBuf> = fixtures::all_formats()
        .into_iter()
        .map(|(ext, bytes)| tagged(&dir, &format!("song.{ext}"), &bytes))
        .collect();
    let data = patch(serde_json::json!({
        "album": "新专辑", "albumArtist": null, "year": null, "trackNumber": null, "genre": null,
        "lyricist": "新作词"
    }));

    let results = apply_patch(&service(), &files, &data);

    assert_eq!(results.len(), files.len());
    for (result, path) in results.iter().zip(&files) {
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(result.success, "{name}: {:?}", result.message);
        assert_eq!(result.path, path.to_string_lossy());
        assert_eq!(result.row.album.as_deref(), Some("新专辑"), "{name}");

        let info = read(path);
        assert_eq!(info.album.as_deref(), Some("新专辑"), "{name}");
        assert_eq!(info.lyricist.as_deref(), Some("新作词"), "{name}");
        assert_eq!(info.album_artist, None, "{name}");
        assert_eq!(info.year, None, "{name}");
        assert_eq!(
            (info.track_number, info.track_total),
            (None, None),
            "{name}"
        );
        assert_eq!(info.genre, None, "{name}");
        // 没提到的字段原样保留。
        assert_eq!(info.title.as_deref(), Some("晴天"), "{name}");
        assert_eq!(info.artist.as_deref(), Some("周杰伦"), "{name}");
        assert_eq!(info.disc_number, Some(1), "{name}");
        assert_eq!(info.composer.as_deref(), Some("周杰伦"), "{name}");
        assert_eq!(info.comment.as_deref(), Some("原始备注"), "{name}");
        assert_eq!(info.netease_id, Some(186016), "{name}");
        assert!(info.lyrics.unwrap().contains("第二行"), "{name}");
        assert_eq!(info.cover_data.as_deref(), Some(&PNG_COVER[..]), "{name}");
    }
}

#[test]
fn changing_or_clearing_the_comment_keeps_the_netease_id() {
    let dir = TempDir::new();
    let changed = tagged(&dir, "changed.mp3", &fixtures::mp3_default());
    let cleared = tagged(&dir, "cleared.flac", &fixtures::flac());
    let bare = create(&dir, "bare.mp3", &fixtures::mp3_default());

    let results = apply_patch(
        &service(),
        &[changed.clone(), bare.clone()],
        &patch(serde_json::json!({ "comment": "新备注" })),
    );
    assert!(results.iter().all(|result| result.success));
    let info = read(&changed);
    assert_eq!(info.comment.as_deref(), Some("新备注"));
    assert_eq!(info.netease_id, Some(186016));
    // 本来没有 ID 的文件不会凭空多出标记。
    let info = read(&bare);
    assert_eq!(
        (info.comment.as_deref(), info.netease_id),
        (Some("新备注"), None)
    );

    let results = apply_patch(
        &service(),
        std::slice::from_ref(&cleared),
        &patch(serde_json::json!({ "comment": null })),
    );
    assert!(results[0].success);
    assert_eq!(results[0].row.comment, None);
    let info = read(&cleared);
    assert_eq!((info.comment, info.netease_id), (None, Some(186016)));
    assert_eq!(info.title.as_deref(), Some("晴天"));
}

#[test]
fn covers_are_replaced_or_removed_for_all_files() {
    let dir = TempDir::new();
    let first = tagged(&dir, "1.mp3", &fixtures::mp3_default());
    let second = create(&dir, "2.flac", &fixtures::flac());
    let files = [first.clone(), second.clone()];
    let cover = png_cover_2();
    let encoded = STANDARD.encode(&cover);

    let results = apply_patch(
        &service(),
        &files,
        &patch(serde_json::json!({ "coverBase64": encoded })),
    );
    assert!(
        results
            .iter()
            .all(|result| result.success && result.row.has_cover)
    );
    for path in &files {
        assert_eq!(read(path).cover_data.as_deref(), Some(&cover[..]));
    }
    assert_eq!(read(&first).title.as_deref(), Some("晴天"));

    let results = apply_patch(
        &service(),
        &files,
        &patch(serde_json::json!({ "removeCover": true })),
    );
    assert!(
        results
            .iter()
            .all(|result| result.success && !result.row.has_cover)
    );
    assert!(!read(&second).has_cover);
}

#[test]
fn a_failing_file_does_not_stop_the_others_and_duplicates_run_once() {
    let dir = TempDir::new();
    let good = tagged(&dir, "good.mp3", &fixtures::mp3_default());
    let broken = create(&dir, "broken.mp3", b"not audio at all");
    let missing = dir.path().join("missing.mp3");
    let upper = PathBuf::from(good.to_string_lossy().to_uppercase());

    let results = apply_patch(
        &service(),
        &[good.clone(), broken, missing, upper],
        &patch(serde_json::json!({ "title": "改过" })),
    );

    assert_eq!(results.len(), 3, "大小写不同的同一路径只处理一次");
    assert!(results[0].success && results[0].message.is_none());
    assert_eq!(results[0].row.title.as_deref(), Some("改过"));
    assert!(!results[1].success);
    assert!(
        results[1]
            .message
            .as_deref()
            .unwrap()
            .contains("broken.mp3")
    );
    assert!(!results[1].row.readable);
    assert!(!results[2].success);
    assert!(results[2].message.as_deref().unwrap().contains("找不到"));
    assert_eq!(read(&good).title.as_deref(), Some("改过"));
}

#[test]
fn patch_results_serialize_to_camel_case() {
    let json = serde_json::to_value(TagPatchResult {
        path: "a.mp3".into(),
        success: false,
        message: Some("原因".into()),
        row: TagRowView::default(),
    })
    .unwrap();
    assert_eq!(json["success"], false);
    assert_eq!(json["message"], "原因");
    assert!(json["row"].get("fileName").is_some());
}
