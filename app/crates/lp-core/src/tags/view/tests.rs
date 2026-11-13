use std::path::Path;

use super::*;
use crate::tags::fixtures::PNG_COVER;
use crate::test_util::TempDir;

fn info_with_cover(data: Vec<u8>, mime: Option<&str>) -> AudioTagInfo {
    AudioTagInfo {
        title: Some("晴天".into()),
        netease_id: Some(9_007_199_254_740_993),
        has_cover: true,
        cover_data: Some(data),
        cover_mime_type: mime.map(str::to_owned),
        file_size: 1234,
        ..Default::default()
    }
}

#[test]
fn view_inlines_the_cover_as_a_data_url_and_keeps_big_ids_exact() {
    let view = TagView::new(
        Path::new("D:/音乐/晴天.mp3"),
        info_with_cover(PNG_COVER.to_vec(), Some("image/png")),
    );

    assert_eq!(view.file_name, "晴天.mp3");
    assert_eq!(view.netease_id.as_deref(), Some("9007199254740993"));
    let url = view.cover_data_url.unwrap();
    assert!(
        url.starts_with("data:image/png;base64,iVBORw0KGgo"),
        "{url}"
    );
    assert_eq!(view.cover_size, Some(PNG_COVER.len()));
    assert_eq!((view.cover_width, view.cover_height), (Some(1), Some(1)));
    assert!(!view.cover_too_large);
    assert!(view.has_cover);
}

#[test]
fn view_reports_the_cover_type_and_leaves_unknown_sizes_empty() {
    let mut info = info_with_cover(vec![1, 2, 3], Some("image/jpeg"));
    info.cover_picture_type = Some("正面封面".into());

    let view = TagView::new(Path::new("a.mp3"), info);

    assert_eq!(view.cover_picture_type.as_deref(), Some("正面封面"));
    assert_eq!((view.cover_width, view.cover_height), (None, None));
}

#[test]
fn view_detects_the_mime_when_the_file_does_not_say() {
    for mime in [None, Some(""), Some("application/octet-stream")] {
        let view = TagView::new(
            Path::new("a.mp3"),
            info_with_cover(PNG_COVER.to_vec(), mime),
        );
        assert!(
            view.cover_data_url
                .unwrap()
                .starts_with("data:image/png;base64,"),
            "{mime:?}"
        );
    }
}

#[test]
fn view_skips_oversized_covers_but_reports_them() {
    let view = TagView::new(
        Path::new("a.flac"),
        info_with_cover(vec![0xFF; MAX_COVER_PREVIEW_BYTES + 1], Some("image/jpeg")),
    );

    assert!(view.has_cover);
    assert!(view.cover_too_large);
    assert_eq!(view.cover_data_url, None);
    assert_eq!(view.cover_size, Some(MAX_COVER_PREVIEW_BYTES + 1));
}

#[test]
fn view_without_cover_has_no_data_url() {
    let view = TagView::new(Path::new("a.mp3"), AudioTagInfo::default());
    assert!(!view.has_cover);
    assert_eq!(view.cover_data_url, None);
    assert_eq!(view.cover_size, None);
    assert!(!view.cover_too_large);
}

#[test]
fn view_takes_the_lrc_file_next_to_the_audio_only_when_nothing_is_embedded() {
    let dir = TempDir::new();
    let audio = dir.path().join("歌手 - 歌名.flac");
    std::fs::write(
        dir.path().join("歌手 - 歌名.lrc"),
        "[00:01.00]旁边文件里的歌词",
    )
    .unwrap();

    // 构造视图本身不读磁盘。
    let view = TagView::new(&audio, AudioTagInfo::default());
    assert_eq!(view.sidecar_lyrics, None);

    let view = view.with_sidecar_lyrics();
    assert_eq!(view.lyrics, None, "内嵌歌词仍然是空的");
    let sidecar = view.sidecar_lyrics.as_ref().unwrap();
    assert_eq!(sidecar.file_name, "歌手 - 歌名.lrc");
    assert_eq!(sidecar.text, "[00:01.00]旁边文件里的歌词");
    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["sidecarLyrics"]["fileName"], "歌手 - 歌名.lrc");
    assert!(json["lyrics"].is_null());

    let embedded = AudioTagInfo {
        lyrics: Some("[00:01.00]内嵌的歌词".into()),
        ..Default::default()
    };
    let view = TagView::new(&audio, embedded).with_sidecar_lyrics();
    assert_eq!(view.lyrics.as_deref(), Some("[00:01.00]内嵌的歌词"));
    assert_eq!(view.sidecar_lyrics, None);

    // 没有同名的 .lrc。
    let view =
        TagView::new(&dir.path().join("别的.flac"), AudioTagInfo::default()).with_sidecar_lyrics();
    assert_eq!(view.sidecar_lyrics, None);
}

#[test]
fn view_serializes_to_camel_case() {
    let json = serde_json::to_value(TagView::new(
        Path::new("a.mp3"),
        info_with_cover(PNG_COVER.to_vec(), None),
    ))
    .unwrap();
    for key in [
        "fileName",
        "albumArtist",
        "trackNumber",
        "trackTotal",
        "discNumber",
        "discTotal",
        "neteaseId",
        "hasCover",
        "coverDataUrl",
        "coverTooLarge",
        "durationMs",
        "fileSize",
        "fileFormat",
        "sidecarLyrics",
    ] {
        assert!(json.get(key).is_some(), "缺少 {key}");
    }
}

#[test]
fn write_request_converts_to_tag_data() {
    let json = serde_json::json!({
        "title": "T", "artists": ["A", "B"], "year": 2024, "trackNumber": 2, "trackTotal": 9,
        "replaceAll": true, "neteaseId": 42, "coverBase64": "iVBORw0KGgo=", "coverMimeType": "image/png",
        "plainLyrics": "[00:01.00]x"
    });
    let request: WriteTagsRequest = serde_json::from_value(json).unwrap();

    let data = request.into_data().unwrap();

    assert_eq!(data.title.as_deref(), Some("T"));
    assert_eq!(data.artists, ["A", "B"]);
    assert_eq!(
        (data.year, data.track_number, data.track_total),
        (Some(2024), Some(2), Some(9))
    );
    assert!(data.replace_all);
    assert_eq!(data.netease_id, Some(42));
    assert_eq!(
        data.cover_data.as_deref(),
        Some(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..])
    );
    assert_eq!(data.plain_lyrics.as_deref(), Some("[00:01.00]x"));
    assert!(!data.remove_cover);
}

#[test]
fn write_request_can_ask_to_remove_the_cover() {
    let request: WriteTagsRequest =
        serde_json::from_value(serde_json::json!({ "removeCover": true, "replaceAll": true }))
            .unwrap();
    let data = request.into_data().unwrap();
    assert!(data.remove_cover && data.replace_all);
    assert!(data.cover_data.is_none());
}

#[test]
fn write_request_defaults_to_a_partial_update_and_rejects_bad_base64() {
    let request: WriteTagsRequest = serde_json::from_value(serde_json::json!({})).unwrap();
    let data = request.into_data().unwrap();
    assert_eq!(data, AudioTagData::default());

    let request: WriteTagsRequest =
        serde_json::from_value(serde_json::json!({ "coverBase64": "***" })).unwrap();
    assert!(matches!(
        request.into_data(),
        Err(TagError::InvalidArgument(_))
    ));

    // 空白的封面字段等于没有封面。
    let request: WriteTagsRequest =
        serde_json::from_value(serde_json::json!({ "coverBase64": "  " })).unwrap();
    assert_eq!(request.into_data().unwrap().cover_data, None);
}

#[test]
fn errors_reach_the_frontend_as_kind_and_message() {
    let cases = [
        (
            TagError::InvalidArgument("音频文件路径不能为空。"),
            "invalid",
        ),
        (TagError::NotFound("x.mp3".into()), "notFound"),
        (TagError::Unsupported("x.txt".into()), "unsupported"),
        (
            TagError::SaveFailed {
                file: "x.mp3".into(),
                reason: "原因".into(),
            },
            "failed",
        ),
        (
            TagError::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            "failed",
        ),
    ];
    for (error, kind) in cases {
        let message = error.to_string();
        let dto = TagCommandError::from(error);
        assert_eq!(dto.kind, kind);
        assert_eq!(dto.message, message);
        assert!(!dto.message.is_empty());
    }
    let json =
        serde_json::to_value(TagCommandError::from(TagError::Unsupported("x.txt".into()))).unwrap();
    assert_eq!(json["kind"], "unsupported");
    assert!(json["message"].as_str().unwrap().contains("x.txt"));
}
