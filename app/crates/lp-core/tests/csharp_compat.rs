//! 与 C# 版 `settings.json` / `session.dat` 的兼容性夹具测试。
//!
//! `tests/fixtures/` 里的文件是用 C# 的 `JsonSettingsStore` / `ProtectedSessionStore` 序列化逻辑生成的
//! **假数据**（Cookie 为 `MUSIC_U=fake; __csrf=abc`，昵称「测试」），不含任何真实账号信息。
//! DPAPI 加密的 `session.dat` 绑定 Windows 用户，无法作为夹具提交，所以这里用明文 JSON 与 mode 2 容器
//! 验证字段与格式；DPAPI 本身由 `session` 模块的往返测试覆盖，C# 与 Rust 互读 DPAPI 文件的实证记录在
//! `docs/TAURI_REWRITE.md`。

use std::path::PathBuf;

use lp_core::models::{
    AudioQuality, LyricsSourceKind, LyricsWriteMode, OnlineSourcePriority, RenderFrameRateMode,
    ThemeMode,
};
use lp_core::session::{NeteaseSession, SessionStore};
use lp_core::settings::{AppSettings, JsonSettingsStore};
use serde_json::Value;

const DEFAULT_DIR: &str = r"D:\Music\LunaticPrincess";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn read_json(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(fixture(name)).unwrap()).unwrap()
}

/// Rust 重写版新增、C# 版没有的设置键（C# 读取时忽略它们，所以文件仍然互通）。
const RUST_ONLY_KEYS: [&str; 3] = ["windowMaterial", "glassOpacity", "reduceMotion"];

/// 去掉 Rust 新增的键，只留 C# 也认识的部分用于逐字段比较。
fn csharp_view(settings: &AppSettings) -> Value {
    let mut value = serde_json::to_value(settings).unwrap();
    let object = value.as_object_mut().unwrap();
    for key in RUST_ONLY_KEYS {
        assert!(object.remove(key).is_some(), "Rust 设置应包含 {key}");
    }
    value
}

/// Rust 的默认设置，但编辑器搜索来源取 C# 的默认值（Rust 版默认改成了 QQ 音乐）。
fn csharp_defaults() -> AppSettings {
    let mut settings = AppSettings::new(DEFAULT_DIR);
    settings.lyrics_editor_search_source = LyricsSourceKind::Netease;
    settings
}

#[test]
fn csharp_default_settings_load_as_defaults() {
    let loaded = JsonSettingsStore::new(fixture("settings.csharp-default.json"), DEFAULT_DIR)
        .load()
        .unwrap();
    assert_eq!(loaded, csharp_defaults());
}

#[test]
fn rust_defaults_serialize_to_the_same_json_as_csharp() {
    // 值与键顺序都一致（serde_json 开启 preserve_order 之前用键列表比较顺序）。
    let csharp = read_json("settings.csharp-default.json");
    assert_eq!(csharp_view(&csharp_defaults()), csharp);

    let csharp_text = std::fs::read_to_string(fixture("settings.csharp-default.json")).unwrap();
    let keys_in_file: Vec<&str> = csharp_text
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix('"'))
        .filter_map(|rest| rest.split('"').next())
        .filter(|key| csharp.get(*key).is_some())
        .collect();
    let rust_text = serde_json::to_string_pretty(&csharp_defaults()).unwrap();
    let rust_keys: Vec<&str> = rust_text
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix('"'))
        .filter_map(|rest| rest.split('"').next())
        .filter(|key| csharp.get(*key).is_some())
        .collect();
    assert_eq!(rust_keys, keys_in_file, "键的书写顺序应与 C# 一致");
}

#[test]
fn csharp_custom_settings_round_trip_through_rust() {
    let loaded = JsonSettingsStore::new(fixture("settings.csharp-custom.json"), DEFAULT_DIR)
        .load()
        .unwrap();

    let mut expected = AppSettings::new(DEFAULT_DIR);
    expected.download_directory = r"E:\下载\music".into();
    expected.library_directory = r"E:\曲库".into();
    expected.quality = AudioQuality::DolbyAtmos;
    expected.lyrics_mode = LyricsWriteMode::Word;
    expected.write_cover = false;
    expected.overwrite_existing = true;
    expected.skip_existing = false;
    expected.file_name_template = "{title} - {artist}".into();
    expected.max_concurrency = 6;
    expected.auto_match_local = false;
    expected.tag_source_priority = OnlineSourcePriority::QqMusicFirst;
    expected.lyrics_source_priority = OnlineSourcePriority::NeteaseOnly;
    expected.ncm_import_folders = vec![r"D:\ncm-a".into(), r"D:\ncm-b".into()];
    expected.ncm_import_output_directory = r"D:\ncm-out".into();
    expected.lyrics_editor_search_source = LyricsSourceKind::QqMusic;
    expected.fallback_kugou_lyrics = false;
    expected.fallback_lrclib_lyrics = false;
    expected.lyrics_editor_prefer_word_lyrics = true;
    expected.match_threshold = 0.78;
    expected.theme = ThemeMode::System;
    expected.render_frame_rate = RenderFrameRateMode::Fps144;
    assert_eq!(loaded, expected);

    // 再序列化回去，与 C# 写出的内容逐字段一致（C# 会把非 ASCII 转义成 \uXXXX，解析后等价）。
    assert_eq!(
        csharp_view(&loaded),
        read_json("settings.csharp-custom.json")
    );
}

#[test]
fn csharp_session_json_parses_with_identical_field_names() {
    let session: NeteaseSession =
        serde_json::from_slice(&std::fs::read(fixture("session.csharp.json")).unwrap()).unwrap();
    assert_eq!(session.cookie, "MUSIC_U=fake; __csrf=abc");
    assert_eq!(session.nickname.as_deref(), Some("测试"));
    assert_eq!(session.user_id, 424_242);
    assert_eq!(session.vip_type, 11);
    assert_eq!(
        session.avatar_url.as_deref(),
        Some("https://example.invalid/avatar.jpg")
    );
    assert_eq!(session.logged_in_at, "2026-11-02T18:25:00.1234567+08:00");
    assert_eq!(session.csrf_token(), "abc");

    // 反向：Rust 序列化的 JSON 与 C# 的等价（C# 会把非 ASCII 转义成 \uXXXX，解析后相同）。
    assert_eq!(
        serde_json::to_value(&session).unwrap(),
        read_json("session.csharp.json")
    );
}

#[test]
fn csharp_mode2_session_container_is_readable_without_dpapi() {
    let store = SessionStore::new(fixture("session.csharp-mode2.dat"), None);
    let session = store.load().unwrap().expect("应读出登录态");
    assert_eq!(session.display_name(), "测试");
    assert_eq!(session.cookie_value("MUSIC_U").as_deref(), Some("fake"));
    assert!(store.last_load_failure().is_none());
}
