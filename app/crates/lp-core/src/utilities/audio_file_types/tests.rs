use std::fs;

use super::*;
use crate::test_util::TempDir;

#[test]
fn is_supported_only_accepts_editable_audio_extensions() {
    let cases = [
        (Some("song.mp3"), true),
        (Some("song.FLAC"), true),
        (Some("song.m4a"), true),
        (Some("song.opus"), true),
        (Some("song.wma"), true),
        (Some("song.ncm"), false),
        (Some("cover.jpg"), false),
        (Some("song"), false),
        (Some("song."), false),
        (Some(""), false),
        (None, false),
        (Some("D:\\音乐\\a.b\\song"), false),
        (Some(" D:/音乐/歌.MP3 "), true),
    ];
    for (path, expected) in cases {
        assert_eq!(is_supported(path), expected, "{path:?}");
    }
}

#[test]
fn try_resolve_returns_audio_file_when_dropped_directly() {
    let dir = TempDir::new();
    let path = dir.path().join("song.flac");
    fs::write(&path, [0x66, 0x4C, 0x61, 0x43]).unwrap();

    assert_eq!(try_resolve(path.to_str()), Ok(path));
}

#[test]
fn try_resolve_picks_first_supported_file_from_folder() {
    let dir = TempDir::new();
    let folder = dir.path().join("album");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("cover.jpg"), "x").unwrap();
    fs::write(folder.join("b - 第二首.mp3"), "x").unwrap();
    fs::write(folder.join("a - 第一首.flac"), "x").unwrap();

    assert_eq!(
        try_resolve(folder.to_str()),
        Ok(folder.join("a - 第一首.flac"))
    );
}

#[test]
fn try_resolve_rejects_unsupported_extension_with_hint() {
    let dir = TempDir::new();
    let path = dir.path().join("song.ncm");
    fs::write(&path, [0x43, 0x54, 0x45, 0x4E]).unwrap();

    let error = try_resolve(path.to_str()).unwrap_err();
    assert!(error.contains(".ncm"), "{error}");
    assert!(error.contains("MP3"), "{error}");

    let bare = dir.path().join("noext");
    fs::write(&bare, "x").unwrap();
    assert!(try_resolve(bare.to_str()).unwrap_err().contains("无扩展名"));
}

#[test]
fn try_resolve_reports_empty_folder_and_missing_path() {
    let dir = TempDir::new();
    let empty = dir.path().join("empty");
    fs::create_dir(&empty).unwrap();
    assert_eq!(
        try_resolve(empty.to_str()),
        Err(NO_AUDIO_IN_FOLDER_MESSAGE.to_owned())
    );

    let missing = dir.path().join("missing.mp3");
    assert!(
        try_resolve(missing.to_str())
            .unwrap_err()
            .contains("找不到")
    );

    assert_eq!(
        try_resolve(Some("  ")),
        Err(EMPTY_TARGET_MESSAGE.to_owned())
    );
    assert_eq!(try_resolve(None), Err(EMPTY_TARGET_MESSAGE.to_owned()));
}

#[test]
fn list_supported_files_sorts_by_name_and_skips_other_formats() {
    let dir = TempDir::new();
    let folder = dir.path().join("mixed");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("z.mp3"), "x").unwrap();
    fs::write(folder.join("a.wav"), "x").unwrap();
    fs::write(folder.join("note.txt"), "x").unwrap();
    fs::create_dir(folder.join("sub.mp3")).unwrap(); // 目录不算文件

    let files = list_supported_files(folder.to_str());

    assert_eq!(files, [folder.join("a.wav"), folder.join("z.mp3")]);
    assert!(list_supported_files(None).is_empty());
    assert!(list_supported_files(folder.join("nope").to_str()).is_empty());
}
