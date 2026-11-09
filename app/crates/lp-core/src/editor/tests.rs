use std::fs;

use super::*;
use crate::tags::fixtures::PNG_COVER;
use crate::test_util::TempDir;

fn touch(dir: &TempDir, name: &str) -> String {
    let path = dir.path().join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, b"x").unwrap();
    path.to_string_lossy().into_owned()
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

// ───────────────────────── 记住来源与歌词格式 ─────────────────────────

#[test]
fn remembering_the_options_changes_only_those_two_fields() {
    let mut settings = AppSettings::new(r"D:\Music");
    settings.max_concurrency = 6;
    settings.match_threshold = 0.8;
    settings.ncm_import_folders = vec![r"D:\in".to_owned()];

    let changed = remember_editor_options(&mut settings, LyricsSourceKind::Lrclib, true);

    assert!(changed);
    assert_eq!(
        settings.lyrics_editor_search_source,
        LyricsSourceKind::Lrclib
    );
    assert!(settings.lyrics_editor_prefer_word_lyrics);
    assert_eq!(settings.max_concurrency, 6);
    assert_eq!(settings.match_threshold, 0.8);
    assert_eq!(settings.ncm_import_folders, [r"D:\in"]);
}

#[test]
fn remembering_the_same_options_reports_no_change() {
    let mut settings = AppSettings::new(r"D:\Music");
    settings.lyrics_editor_search_source = LyricsSourceKind::QqMusic;
    settings.lyrics_editor_prefer_word_lyrics = true;

    assert!(!remember_editor_options(
        &mut settings,
        LyricsSourceKind::QqMusic,
        true
    ));
    assert!(remember_editor_options(
        &mut settings,
        LyricsSourceKind::QqMusic,
        false
    ));
}

#[test]
fn every_source_priority_has_its_csharp_label() {
    assert_eq!(
        priority_label(OnlineSourcePriority::NeteaseFirst),
        "网易云音乐优先"
    );
    assert_eq!(
        priority_label(OnlineSourcePriority::QqMusicFirst),
        "QQ 音乐优先"
    );
    assert_eq!(
        priority_label(OnlineSourcePriority::NeteaseOnly),
        "仅网易云音乐"
    );
    assert_eq!(
        priority_label(OnlineSourcePriority::QqMusicOnly),
        "仅 QQ 音乐"
    );
}

#[test]
fn editor_state_serializes_to_camel_case_with_the_remembered_options() {
    let mut settings = AppSettings::new(r"D:\Music");
    settings.lyrics_editor_search_source = LyricsSourceKind::QqMusic;
    settings.lyrics_editor_prefer_word_lyrics = true;
    settings.match_threshold = 0.82;
    settings.tag_source_priority = OnlineSourcePriority::QqMusicFirst;
    settings.lyrics_source_priority = OnlineSourcePriority::NeteaseOnly;

    let json = serde_json::to_value(EditorStateView::new(&settings, true)).unwrap();

    assert_eq!(json["searchSource"], "QQMusic");
    assert_eq!(json["preferWordLyrics"], true);
    assert_eq!(json["threshold"]["percent"], "82");
    assert_eq!(json["tagPriorityText"], "QQ 音乐优先");
    assert_eq!(json["lyricsPriorityText"], "仅网易云音乐");
    assert_eq!(json["isElevated"], true);
    assert!(
        json["dragDropBlockedHint"]
            .as_str()
            .unwrap()
            .contains("管理员")
    );
    let extensions = json["extensions"].as_array().unwrap();
    assert!(extensions.iter().any(|e| e == "mp3"));
    assert!(extensions.iter().any(|e| e == "wma"));
    assert!(!extensions.iter().any(|e| e == "ncm"));
}

// ───────────────────────── 拖入解析 ─────────────────────────

#[test]
fn a_dropped_audio_file_is_returned_as_is() {
    let dir = TempDir::new();
    let file = touch(&dir, "晴天.mp3");

    let result = resolve_drop(std::slice::from_ref(&file));

    assert_eq!(result.kind, DropKind::Audio);
    assert_eq!(result.path.as_deref(), Some(file.as_str()));
    assert_eq!(result.file_name.as_deref(), Some("晴天.mp3"));
    assert_eq!(result.resolved_count, 1);
    assert_eq!(result.total, 1);
    assert!(result.message.starts_with("拖入文件："));
}

#[test]
fn a_dropped_folder_with_several_songs_goes_to_batch_including_subfolders() {
    let dir = TempDir::new();
    touch(&dir, "readme.txt");
    let b = touch(&dir, "b.flac");
    let a = touch(&dir, "A.mp3");
    let nested = touch(&dir, r"CD2\c.ogg");

    let result = resolve_drop(&[dir.path().to_string_lossy().into_owned()]);

    assert_eq!(result.kind, DropKind::Batch);
    assert!(result.path.is_none());
    let paths: Vec<&str> = result.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [a.as_str(), b.as_str(), nested.as_str()],
        "按路径排序"
    );
    assert_eq!(result.files[2].name, "c.ogg");
    assert_eq!(result.resolved_count, 3);
    assert_eq!(result.total, 1);
    assert_eq!(
        result.message,
        "拖入 3 首音频，交给批量获取标签、封面与歌词"
    );
}

#[test]
fn a_folder_with_a_single_song_still_opens_it_in_the_editor() {
    let dir = TempDir::new();
    touch(&dir, "cover.jpg");
    let only = touch(&dir, "only.mp3");

    let result = resolve_drop(&[dir.path().to_string_lossy().into_owned()]);

    assert_eq!(result.kind, DropKind::Audio);
    assert_eq!(result.path.as_deref(), Some(only.as_str()));
    assert!(result.files.is_empty());
}

#[test]
fn several_songs_go_to_batch_in_drop_order_and_skip_unusable_targets() {
    let dir = TempDir::new();
    let bad = touch(&dir, "notes.txt");
    let one = touch(&dir, "one.mp3");
    let two = touch(&dir, "two.flac");
    fs::write(&two, [0_u8; 7]).unwrap();

    let result = resolve_drop(&[two.clone(), bad, one.clone()]);

    assert_eq!(result.kind, DropKind::Batch);
    let paths: Vec<&str> = result.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, [two.as_str(), one.as_str()], "保持拖入顺序");
    assert_eq!(result.files[0].size, 7);
    assert_eq!(result.resolved_count, 2);
    assert_eq!(result.total, 3);
    assert_eq!(
        result.message,
        "拖入 2 首音频，交给批量获取标签、封面与歌词（另有 1 个目标无法处理，已跳过）"
    );
}

#[test]
fn a_song_dropped_together_with_its_folder_counts_once() {
    let dir = TempDir::new();
    let one = touch(&dir, "one.mp3");
    touch(&dir, "two.mp3");

    let result = resolve_drop(&[
        one.to_uppercase(),
        dir.path().to_string_lossy().into_owned(),
    ]);

    assert_eq!(result.kind, DropKind::Batch);
    assert_eq!(result.files.len(), 2);
}

#[test]
fn one_song_among_unusable_targets_opens_it_in_the_editor() {
    let dir = TempDir::new();
    let bad = touch(&dir, "notes.txt");
    let one = touch(&dir, "one.mp3");

    let result = resolve_drop(&[bad, one.clone()]);

    assert_eq!(result.kind, DropKind::Audio);
    assert_eq!(result.path.as_deref(), Some(one.as_str()));
    assert_eq!(result.message, "拖入 2 个目标，打开其中的「one.mp3」");
}

#[test]
fn a_dropped_ncm_file_points_to_the_import_dialog() {
    let dir = TempDir::new();
    let ncm = touch(&dir, "歌曲.NCM");

    let result = resolve_drop(&[ncm]);

    assert_eq!(result.kind, DropKind::Ncm);
    assert!(result.path.is_none());
    assert_eq!(result.message, NCM_DROP_MESSAGE);
}

#[test]
fn an_audio_file_wins_over_an_ncm_file_in_the_same_drop() {
    let dir = TempDir::new();
    let ncm = touch(&dir, "a.ncm");
    let mp3 = touch(&dir, "b.mp3");

    let result = resolve_drop(&[ncm, mp3.clone()]);

    assert_eq!(result.kind, DropKind::Audio);
    assert_eq!(result.path.as_deref(), Some(mp3.as_str()));
}

#[test]
fn unsupported_empty_and_missing_targets_explain_themselves() {
    let dir = TempDir::new();
    let text = touch(&dir, "notes.txt");
    let empty_folder = dir.path().join("空文件夹");
    fs::create_dir_all(&empty_folder).unwrap();

    let unsupported = resolve_drop(&[text]);
    assert_eq!(unsupported.kind, DropKind::Error);
    assert!(unsupported.message.contains("暂不支持「.txt」格式"));

    let folder = resolve_drop(&[empty_folder.to_string_lossy().into_owned()]);
    assert_eq!(folder.kind, DropKind::Error);
    assert_eq!(folder.message, "该文件夹里没有可编辑的音频文件。");

    let missing = resolve_drop(&[r"D:\不存在\x.mp3".to_owned()]);
    assert_eq!(missing.kind, DropKind::Error);
    assert!(missing.message.starts_with("找不到"));

    let nothing = resolve_drop(&[]);
    assert_eq!(nothing.kind, DropKind::Error);
    assert_eq!(nothing.message, "没有识别到拖入的文件路径。");
}

#[test]
fn drop_resolution_serializes_to_camel_case() {
    let dir = TempDir::new();
    let file = touch(&dir, "a.mp3");

    let json = serde_json::to_value(resolve_drop(&[file])).unwrap();

    assert_eq!(json["kind"], "audio");
    assert_eq!(json["resolvedCount"], 1);
    assert_eq!(json["fileName"], "a.mp3");
    let ncm = serde_json::to_value(resolve_drop(&["x.ncm".to_owned()])).unwrap();
    assert_eq!(ncm["kind"], "ncm");
    let second = touch(&dir, "b.flac");
    let batch = serde_json::to_value(resolve_drop(&[touch(&dir, "c.mp3"), second])).unwrap();
    assert_eq!(batch["kind"], "batch");
    assert_eq!(batch["files"][1]["name"], "b.flac");
    assert!(batch["files"][1]["size"].is_u64());
}

// ───────────────────────── 启动参数 ─────────────────────────

#[test]
fn edit_and_lyrics_flags_open_an_existing_file() {
    let dir = TempDir::new();
    let file = touch(&dir, "x.mp3");

    for flag in ["--edit", "--lyrics"] {
        let request = parse_startup_args(["app.exe", flag, file.as_str()]);
        assert_eq!(
            request
                .file
                .as_deref()
                .map(|p| p.to_string_lossy().into_owned()),
            Some(file.clone())
        );
        assert!(request.missing.is_none());
    }
}

#[test]
fn a_relative_startup_path_becomes_a_full_path() {
    let dir = TempDir::new();
    touch(&dir, "rel.mp3");
    let relative = dir.path().join("rel.mp3");
    let request = parse_startup_args(args(&["app.exe", "--edit", &relative.to_string_lossy()]));
    assert!(request.file.unwrap().is_absolute());
}

#[test]
fn a_missing_startup_file_is_ignored_but_remembered_for_the_log() {
    let request = parse_startup_args(["app.exe", "--edit", r"D:\不存在\x.mp3"]);
    assert!(request.file.is_none());
    assert_eq!(request.missing.as_deref(), Some(r"D:\不存在\x.mp3"));
}

#[test]
fn startup_flags_without_a_value_or_unknown_flags_do_nothing() {
    assert_eq!(parse_startup_args(["app.exe"]), StartupRequest::default());
    assert_eq!(
        parse_startup_args(["app.exe", "--edit"]),
        StartupRequest::default()
    );
    assert_eq!(
        parse_startup_args(["app.exe", "--edit", "  "]),
        StartupRequest::default()
    );
    let dir = TempDir::new();
    let file = touch(&dir, "x.mp3");
    assert_eq!(
        parse_startup_args(["app.exe", "--open", file.as_str()]),
        StartupRequest::default()
    );
}

#[test]
fn the_program_name_is_never_read_as_a_flag() {
    let dir = TempDir::new();
    let file = touch(&dir, "x.mp3");
    // 与 C# 一样从第二项开始找：第一项是程序自己。
    let request = parse_startup_args(["--edit", file.as_str()]);
    assert!(request.file.is_none());
}

#[test]
fn the_first_existing_file_wins_when_several_flags_are_given() {
    let dir = TempDir::new();
    let file = touch(&dir, "ok.mp3");
    let request = parse_startup_args(args(&["app.exe", "--edit", "nope.mp3", "--lyrics", &file]));
    assert!(request.file.is_some());
    assert!(request.missing.is_none());
}

// ───────────────────────── 封面图片文件 ─────────────────────────

#[test]
fn a_png_cover_file_is_read_as_base64_with_its_mime() {
    let dir = TempDir::new();
    let path = dir.path().join("cover.bin");
    fs::write(&path, PNG_COVER).unwrap();

    let cover = read_cover_file(&path).unwrap();

    assert_eq!(cover.mime_type, "image/png");
    assert_eq!(cover.size, PNG_COVER.len());
    assert_eq!(STANDARD.decode(&cover.base64).unwrap(), PNG_COVER);
    assert_eq!(cover.file_name, "cover.bin");
}

#[test]
fn non_image_empty_missing_and_oversized_cover_files_are_rejected_in_chinese() {
    let dir = TempDir::new();

    let text = dir.path().join("a.png");
    fs::write(&text, b"not an image at all").unwrap();
    assert!(
        read_cover_file(&text)
            .unwrap_err()
            .contains("不是支持的图片格式")
    );

    let empty = dir.path().join("empty.png");
    fs::write(&empty, b"").unwrap();
    assert!(read_cover_file(&empty).unwrap_err().contains("是空的"));

    assert!(
        read_cover_file(&dir.path().join("nope.png"))
            .unwrap_err()
            .starts_with("找不到图片文件")
    );
    assert!(
        read_cover_file(dir.path())
            .unwrap_err()
            .contains("不是图片文件")
    );

    let big = dir.path().join("big.jpg");
    let file = fs::File::create(&big).unwrap();
    file.set_len(MAX_COVER_FILE_BYTES + 1).unwrap();
    assert!(read_cover_file(&big).unwrap_err().contains("超过 10 MB"));
}

#[test]
fn jpeg_webp_gif_and_bmp_headers_are_recognised() {
    assert!(is_known_image(&[0xFF, 0xD8, 0xFF, 0xE0]));
    assert!(is_known_image(b"GIF89a"));
    assert!(is_known_image(b"BM\x00\x00"));
    assert!(is_known_image(b"RIFF\x00\x00\x00\x00WEBPVP8 "));
    assert!(!is_known_image(b"RIFF\x00\x00\x00\x00WAVEfmt "));
    assert!(!is_known_image(b""));
}

#[test]
fn elevation_detection_does_not_panic() {
    // 测试进程通常不是管理员，但 CI 可能是：只验证调用安全。
    let _ = is_elevated();
}
