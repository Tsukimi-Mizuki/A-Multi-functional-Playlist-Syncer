//! 应用设置（`settings.json`）的模型、校验与读写。
//!
//! 对应 C# 的 `Core/Settings/`（`AppSettings`、`JsonSettingsStore`），文件格式与 C# 版互相兼容：
//! camelCase 键名（读取时不区分大小写）、枚举按成员名、缺字段取默认值、缩进输出。
//!
//! 读取策略比 C# 更宽容：C# 只要有一个字段类型不对（例如枚举写成了未知名字）就会整份重置为默认值，
//! 这里只让出问题的那个字段回落到默认，其余设置保持原样；文件整体无法解析时仍然使用默认设置。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::error::{CoreError, CoreResult};
use crate::models::{
    AudioQuality, LyricsSourceKind, LyricsWriteMode, OnlineSourcePriority, RenderFrameRateMode,
    ThemeMode, WindowMaterial,
};

/// 默认文件名模板。
pub const DEFAULT_FILE_NAME_TEMPLATE: &str = "{artist} - {title}";
/// 并发下载数的合法范围。
pub const MIN_CONCURRENCY: i64 = 1;
pub const MAX_CONCURRENCY: i64 = 8;
/// 在线匹配度阈值：默认值与合法范围（对应 `LyricsMatchScorer`）。
pub const DEFAULT_MATCH_THRESHOLD: f64 = 0.60;
pub const MIN_MATCH_THRESHOLD: f64 = 0.30;
pub const MAX_MATCH_THRESHOLD: f64 = 1.00;
/// 界面玻璃底色的浓度（0 = 完全通透，1 = 不透明）：默认值与合法范围。
pub const DEFAULT_GLASS_OPACITY: f64 = 0.45;
pub const MIN_GLASS_OPACITY: f64 = 0.10;
pub const MAX_GLASS_OPACITY: f64 = 0.95;

/// 应用设置。字段顺序与 C# 的 `AppSettings` 一致，序列化后的 JSON 键顺序也就一致。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// 下载目录。
    pub download_directory: String,
    /// 本地曲库目录。
    pub library_directory: String,
    /// 下载音质。
    pub quality: AudioQuality,
    /// 歌词写入方式。
    pub lyrics_mode: LyricsWriteMode,
    /// 是否写入封面。
    pub write_cover: bool,
    /// 是否覆盖已存在的文件。
    pub overwrite_existing: bool,
    /// 是否跳过已存在的文件。
    pub skip_existing: bool,
    /// 文件名模板。
    pub file_name_template: String,
    /// 下载并发数（1–8）。
    pub max_concurrency: i64,
    /// 启动后是否自动对比本地曲库。
    pub auto_match_local: bool,
    /// 在线标签来源优先级。
    pub tag_source_priority: OnlineSourcePriority,
    /// 在线歌词来源优先级。
    pub lyrics_source_priority: OnlineSourcePriority,
    /// NCM 导入窗口记住的输入文件夹列表（按添加顺序，自动去重）。
    pub ncm_import_folders: Vec<String>,
    /// NCM 导入窗口记住的输出目录；为空时回退到下载目录。
    pub ncm_import_output_directory: String,
    /// 歌词编辑窗口记住的在线搜索来源（主词源或备用词源）。
    pub lyrics_editor_search_source: LyricsSourceKind,
    /// 批量获取歌词时是否启用酷狗音乐备用词源（逐字 + 译文）。
    pub fallback_kugou_lyrics: bool,
    /// 批量获取歌词时是否启用 LRCLIB 备用词源（逐行）。
    pub fallback_lrclib_lyrics: bool,
    /// 歌词编辑窗口记住的在线歌词格式：true 表示优先逐字歌词。
    pub lyrics_editor_prefer_word_lyrics: bool,
    /// 在线匹配度阈值（0.30–1.00）。
    pub match_threshold: f64,
    /// 界面主题。
    pub theme: ThemeMode,
    /// 渲染帧率上限。
    pub render_frame_rate: RenderFrameRateMode,
    /// 窗口背景材质（Rust 重写版新增；C# 版读取时会忽略这个键）。
    pub window_material: WindowMaterial,
    /// 界面玻璃底色浓度（0.10–0.95）：越低越能看清窗口下面的内容。
    pub glass_opacity: f64,
    /// 减少动效：关掉弹出、滑动等过渡动画（系统开启“减少动画”时界面也会自动减弱）。
    pub reduce_motion: bool,
}

impl AppSettings {
    /// 全部取默认值；`default_download_dir` 对应 C# 的 `AppPaths.DefaultDownloadDirectory`。
    pub fn new(default_download_dir: &str) -> Self {
        Self {
            download_directory: default_download_dir.to_owned(),
            library_directory: default_download_dir.to_owned(),
            quality: AudioQuality::ExHigh,
            lyrics_mode: LyricsWriteMode::Line,
            write_cover: true,
            overwrite_existing: false,
            skip_existing: true,
            file_name_template: DEFAULT_FILE_NAME_TEMPLATE.to_owned(),
            max_concurrency: 3,
            auto_match_local: true,
            tag_source_priority: OnlineSourcePriority::NeteaseFirst,
            lyrics_source_priority: OnlineSourcePriority::NeteaseFirst,
            ncm_import_folders: Vec::new(),
            ncm_import_output_directory: String::new(),
            // 网易云匿名搜索目前不可用（见 docs/TAURI_REWRITE.md「待修问题」），默认改用 QQ 音乐；C# 版默认网易云。
            lyrics_editor_search_source: LyricsSourceKind::QqMusic,
            fallback_kugou_lyrics: true,
            fallback_lrclib_lyrics: true,
            lyrics_editor_prefer_word_lyrics: false,
            match_threshold: DEFAULT_MATCH_THRESHOLD,
            theme: ThemeMode::Dark,
            render_frame_rate: RenderFrameRateMode::VSync,
            window_material: WindowMaterial::Acrylic,
            glass_opacity: DEFAULT_GLASS_OPACITY,
            reduce_motion: false,
        }
    }

    /// 校验并归位非法值（对应 C# 的 `Normalize()`）。枚举在类型层面已经保证合法，
    /// 所以这里处理的是路径、模板、并发数、阈值与 NCM 文件夹列表。
    pub fn normalize(&mut self, default_download_dir: &str) {
        let download = self.download_directory.trim();
        self.download_directory = if download.is_empty() {
            default_download_dir.to_owned()
        } else {
            download.to_owned()
        };

        let library = self.library_directory.trim();
        self.library_directory = if library.is_empty() {
            self.download_directory.clone()
        } else {
            library.to_owned()
        };

        if self.file_name_template.trim().is_empty() {
            self.file_name_template = DEFAULT_FILE_NAME_TEMPLATE.to_owned();
        }

        self.max_concurrency = self.max_concurrency.clamp(MIN_CONCURRENCY, MAX_CONCURRENCY);

        if !self.match_threshold.is_finite() || self.match_threshold <= 0.0 {
            self.match_threshold = DEFAULT_MATCH_THRESHOLD;
        }
        // C# 的 Math.Round(x, 2) 是“银行家舍入”，这里保持一致。
        let rounded = (self.match_threshold * 100.0).round_ties_even() / 100.0;
        self.match_threshold = rounded.clamp(MIN_MATCH_THRESHOLD, MAX_MATCH_THRESHOLD);

        if !self.glass_opacity.is_finite() {
            self.glass_opacity = DEFAULT_GLASS_OPACITY;
        }
        let glass = (self.glass_opacity * 100.0).round() / 100.0;
        self.glass_opacity = glass.clamp(MIN_GLASS_OPACITY, MAX_GLASS_OPACITY);

        self.ncm_import_output_directory = self.ncm_import_output_directory.trim().to_owned();
        let mut seen = HashSet::new();
        let mut remembered = Vec::with_capacity(self.ncm_import_folders.len());
        for folder in &self.ncm_import_folders {
            let trimmed = folder.trim();
            if !trimmed.is_empty() && seen.insert(trimmed.to_uppercase()) {
                remembered.push(trimmed.to_owned());
            }
        }
        self.ncm_import_folders = remembered;
    }

    /// 从已解析的 JSON 值构造设置：缺字段或类型不对的字段取默认值，最后做一次 [`normalize`](Self::normalize)。
    ///
    /// 键名不区分大小写（与 `System.Text.Json` 的 Web 默认一致）。`quality` 额外兼容旧值
    /// （`MigrateLegacyValues`：`Higher` 并入 `ExHigh`，无法识别的值丢弃后用默认音质）。
    pub fn from_json_value(root: &Value, default_download_dir: &str) -> Self {
        let mut settings = Self::new(default_download_dir);
        let Some(object) = root.as_object() else {
            settings.normalize(default_download_dir);
            return settings;
        };
        let fields: HashMap<String, &Value> = object
            .iter()
            .map(|(key, value)| (key.to_ascii_lowercase(), value))
            .collect();
        let get = |name: &str| fields.get(&name.to_ascii_lowercase()).copied();

        if let Some(v) = get("downloadDirectory").and_then(as_string) {
            settings.download_directory = v;
        }
        if let Some(v) = get("libraryDirectory").and_then(as_string) {
            settings.library_directory = v;
        }
        if let Some(value) = get("quality") {
            settings.quality = match value {
                Value::String(text) => AudioQuality::parse_setting_value(text),
                other => AudioQuality::from_json(other),
            }
            .unwrap_or_default();
        }
        if let Some(v) = get("lyricsMode").and_then(LyricsWriteMode::from_json) {
            settings.lyrics_mode = v;
        }
        if let Some(v) = get("writeCover").and_then(Value::as_bool) {
            settings.write_cover = v;
        }
        if let Some(v) = get("overwriteExisting").and_then(Value::as_bool) {
            settings.overwrite_existing = v;
        }
        if let Some(v) = get("skipExisting").and_then(Value::as_bool) {
            settings.skip_existing = v;
        }
        if let Some(v) = get("fileNameTemplate").and_then(as_string) {
            settings.file_name_template = v;
        }
        if let Some(v) = get("maxConcurrency").and_then(as_integer) {
            settings.max_concurrency = v;
        }
        if let Some(v) = get("autoMatchLocal").and_then(Value::as_bool) {
            settings.auto_match_local = v;
        }
        if let Some(v) = get("tagSourcePriority").and_then(OnlineSourcePriority::from_json) {
            settings.tag_source_priority = v;
        }
        if let Some(v) = get("lyricsSourcePriority").and_then(OnlineSourcePriority::from_json) {
            settings.lyrics_source_priority = v;
        }
        if let Some(Value::Array(items)) = get("ncmImportFolders") {
            settings.ncm_import_folders = items.iter().filter_map(as_string).collect();
        }
        if let Some(v) = get("ncmImportOutputDirectory").and_then(as_string) {
            settings.ncm_import_output_directory = v;
        }
        if let Some(v) = get("lyricsEditorSearchSource").and_then(LyricsSourceKind::from_json) {
            settings.lyrics_editor_search_source = v;
        }
        if let Some(v) = get("fallbackKugouLyrics").and_then(Value::as_bool) {
            settings.fallback_kugou_lyrics = v;
        }
        if let Some(v) = get("fallbackLrclibLyrics").and_then(Value::as_bool) {
            settings.fallback_lrclib_lyrics = v;
        }
        if let Some(v) = get("lyricsEditorPreferWordLyrics").and_then(Value::as_bool) {
            settings.lyrics_editor_prefer_word_lyrics = v;
        }
        if let Some(v) = get("matchThreshold").and_then(as_float) {
            settings.match_threshold = v;
        }
        if let Some(v) = get("theme").and_then(ThemeMode::from_json) {
            settings.theme = v;
        }
        if let Some(v) = get("renderFrameRate").and_then(RenderFrameRateMode::from_json) {
            settings.render_frame_rate = v;
        }
        if let Some(v) = get("windowMaterial").and_then(WindowMaterial::from_json) {
            settings.window_material = v;
        }
        if let Some(v) = get("glassOpacity").and_then(as_float) {
            settings.glass_opacity = v;
        }
        if let Some(v) = get("reduceMotion").and_then(Value::as_bool) {
            settings.reduce_motion = v;
        }

        settings.normalize(default_download_dir);
        settings
    }

    /// 应用设置窗口提交的修改：`patch` 只含改动过的键（camelCase），其余字段原样保留。
    ///
    /// 与读取文件时的宽容策略不同，这里是用户刚在界面上做的选择，所以逐项严格校验：
    /// 未知键、类型不对、枚举名无法识别、数值越界都返回中文说明，整份修改不生效。
    pub fn apply_patch(
        &self,
        patch: &Value,
        default_download_dir: &str,
    ) -> Result<AppSettings, String> {
        let Some(changes) = patch.as_object() else {
            return Err("设置修改必须是键值对象".to_owned());
        };
        let mut root = serde_json::to_value(self).map_err(|error| error.to_string())?;
        let Some(object) = root.as_object_mut() else {
            return Err("当前设置无法序列化".to_owned());
        };
        for (key, value) in changes {
            if !object.contains_key(key) {
                return Err(format!("未知的设置项：{key}"));
            }
            validate_patch_value(key, value)?;
            object.insert(key.clone(), value.clone());
        }
        Ok(AppSettings::from_json_value(&root, default_download_dir))
    }
}

/// 校验设置窗口提交的单个字段。
fn validate_patch_value(key: &str, value: &Value) -> Result<(), String> {
    let invalid = || format!("设置项 {key} 的取值无效：{value}");
    let name = || value.as_str().ok_or_else(invalid);
    match key {
        "downloadDirectory"
        | "libraryDirectory"
        | "fileNameTemplate"
        | "ncmImportOutputDirectory" => name().map(drop),
        "writeCover"
        | "overwriteExisting"
        | "skipExisting"
        | "autoMatchLocal"
        | "fallbackKugouLyrics"
        | "fallbackLrclibLyrics"
        | "lyricsEditorPreferWordLyrics"
        | "reduceMotion" => value.as_bool().map(drop).ok_or_else(invalid),
        "quality" => AudioQuality::from_name(name()?)
            .map(drop)
            .ok_or_else(invalid),
        "lyricsMode" => LyricsWriteMode::from_name(name()?)
            .map(drop)
            .ok_or_else(invalid),
        "tagSourcePriority" | "lyricsSourcePriority" => OnlineSourcePriority::from_name(name()?)
            .map(drop)
            .ok_or_else(invalid),
        "lyricsEditorSearchSource" => LyricsSourceKind::from_name(name()?)
            .map(drop)
            .ok_or_else(invalid),
        "theme" => ThemeMode::from_name(name()?).map(drop).ok_or_else(invalid),
        "renderFrameRate" => RenderFrameRateMode::from_name(name()?)
            .map(drop)
            .ok_or_else(invalid),
        "windowMaterial" => WindowMaterial::from_name(name()?)
            .map(drop)
            .ok_or_else(invalid),
        "maxConcurrency" => match value.as_i64() {
            Some(count) if (MIN_CONCURRENCY..=MAX_CONCURRENCY).contains(&count) => Ok(()),
            _ => Err(format!(
                "下载并发数必须是 {MIN_CONCURRENCY}–{MAX_CONCURRENCY} 之间的整数"
            )),
        },
        "matchThreshold" => match value.as_f64() {
            Some(t) if (MIN_MATCH_THRESHOLD..=MAX_MATCH_THRESHOLD).contains(&t) => Ok(()),
            _ => Err("在线匹配度必须在 30%–100% 之间".to_owned()),
        },
        "glassOpacity" => match value.as_f64() {
            Some(o) if (MIN_GLASS_OPACITY..=MAX_GLASS_OPACITY).contains(&o) => Ok(()),
            _ => Err("玻璃浓度必须在 10%–95% 之间".to_owned()),
        },
        "ncmImportFolders" => match value.as_array() {
            Some(items) if items.iter().all(Value::is_string) => Ok(()),
            _ => Err(invalid()),
        },
        _ => Err(invalid()),
    }
}

fn as_string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// 整数：接受 JSON 整数、小数部分为 0 的浮点数，以及带引号的整数（`NumberHandling.AllowReadingFromString`）。
fn as_integer(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_f64()
                .filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15)
                .map(|f| f as i64)
        }),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn as_float(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// `settings.json` 的读写。
///
/// 对应 C# 的 `JsonSettingsStore`。调用方（Tauri 外壳）应把它放在同一把锁后面使用，
/// 这里不再重复 C# 里的 `SemaphoreSlim`。
#[derive(Debug, Clone)]
pub struct JsonSettingsStore {
    file_path: PathBuf,
    default_download_dir: String,
}

impl JsonSettingsStore {
    /// `default_download_dir` 对应 `AppPaths.DefaultDownloadDirectory`，用于缺字段 / 空路径的回落。
    pub fn new(file_path: impl Into<PathBuf>, default_download_dir: impl Into<String>) -> Self {
        Self {
            file_path: file_path.into(),
            default_download_dir: default_download_dir.into(),
        }
    }

    /// 设置文件路径。
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// 全部取默认值（已 normalize）。
    pub fn defaults(&self) -> AppSettings {
        let mut settings = AppSettings::new(&self.default_download_dir);
        settings.normalize(&self.default_download_dir);
        settings
    }

    /// 读取设置。
    ///
    /// - 文件不存在、不是 JSON 对象、内容损坏：返回默认设置（与 C# 一致）；
    /// - 其余读取错误（权限不足、被占用等）：返回 `Err`，由调用方决定提示并改用默认值，
    ///   避免静默地把用户的设置“读成默认”后又被下一次保存覆盖。
    pub fn load(&self) -> CoreResult<AppSettings> {
        let bytes = match fs::read(&self.file_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(self.defaults()),
            Err(error) => return Err(error.into()),
        };
        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        match serde_json::from_str::<Value>(text) {
            Ok(root) => Ok(AppSettings::from_json_value(
                &root,
                &self.default_download_dir,
            )),
            Err(_) => Ok(self.defaults()),
        }
    }

    /// 保存设置：先 normalize，再写临时文件并原子替换。返回实际保存的（已 normalize 的）设置。
    pub fn save(&self, settings: &AppSettings) -> CoreResult<AppSettings> {
        let mut normalized = settings.clone();
        normalized.normalize(&self.default_download_dir);
        let json = serde_json::to_vec_pretty(&normalized).map_err(CoreError::from)?;
        write_atomic(&self.file_path, &json)?;
        Ok(normalized)
    }
}

/// 写 `<文件>.tmp` 再 rename 覆盖目标，避免中途崩溃留下半截文件。
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> CoreResult<()> {
    if let Some(directory) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(directory)?;
    }
    let mut temp_name = path.as_os_str().to_owned();
    temp_name.push(".tmp");
    let temp_path = PathBuf::from(temp_name);
    {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
    }
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;
    use serde_json::json;

    const DEFAULT_DIR: &str = r"D:\default-music";

    fn store_in(dir: &TempDir) -> JsonSettingsStore {
        JsonSettingsStore::new(dir.path().join("settings.json"), DEFAULT_DIR)
    }

    fn write_settings(dir: &TempDir, text: &str) {
        fs::write(dir.path().join("settings.json"), text).unwrap();
    }

    fn custom() -> AppSettings {
        let mut settings = AppSettings::new(DEFAULT_DIR);
        settings.download_directory = r"D:\music".to_owned();
        settings
    }

    #[test]
    fn missing_file_returns_normalized_defaults() {
        let dir = TempDir::new();
        let settings = store_in(&dir).load().unwrap();

        assert_eq!(settings.download_directory, DEFAULT_DIR);
        assert_eq!(settings.library_directory, DEFAULT_DIR);
        assert_eq!(settings.file_name_template, "{artist} - {title}");
        assert_eq!(settings.max_concurrency, 3);
        assert!(settings.ncm_import_folders.is_empty());
        assert_eq!(settings.ncm_import_output_directory, "");
        assert_eq!(settings.quality, AudioQuality::ExHigh);
        assert_eq!(settings.lyrics_mode, LyricsWriteMode::Line);
        assert!(settings.write_cover && settings.skip_existing && settings.auto_match_local);
        assert!(!settings.overwrite_existing);
        assert_eq!(settings.match_threshold, 0.60);
        assert_eq!(settings.theme, ThemeMode::Dark);
        assert_eq!(settings.render_frame_rate, RenderFrameRateMode::VSync);
        assert!(settings.fallback_kugou_lyrics && settings.fallback_lrclib_lyrics);
        assert!(!settings.lyrics_editor_prefer_word_lyrics);
        assert_eq!(
            settings.lyrics_editor_search_source,
            LyricsSourceKind::QqMusic
        );
    }

    #[test]
    fn save_then_load_round_trips_every_field() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        let settings = AppSettings {
            download_directory: r"D:\music".to_owned(),
            library_directory: r"D:\library".to_owned(),
            quality: AudioQuality::DolbyAtmos,
            lyrics_mode: LyricsWriteMode::Word,
            write_cover: false,
            overwrite_existing: true,
            skip_existing: false,
            file_name_template: "{track} {title}".to_owned(),
            max_concurrency: 5,
            auto_match_local: false,
            tag_source_priority: OnlineSourcePriority::QqMusicFirst,
            lyrics_source_priority: OnlineSourcePriority::NeteaseOnly,
            ncm_import_folders: vec![r"D:\ncm-a".to_owned(), r"D:\ncm-b".to_owned()],
            ncm_import_output_directory: r"D:\ncm-out".to_owned(),
            lyrics_editor_search_source: LyricsSourceKind::Lrclib,
            fallback_kugou_lyrics: false,
            fallback_lrclib_lyrics: false,
            lyrics_editor_prefer_word_lyrics: true,
            match_threshold: 0.78,
            theme: ThemeMode::System,
            render_frame_rate: RenderFrameRateMode::Fps144,
            window_material: WindowMaterial::Mica,
            glass_opacity: 0.62,
            reduce_motion: true,
        };

        let saved = store.save(&settings).unwrap();
        assert_eq!(saved, settings);
        assert_eq!(store.load().unwrap(), settings);
    }

    #[test]
    fn saved_json_uses_camel_case_keys_and_enum_names() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        let mut settings = custom();
        settings.quality = AudioQuality::Lossless;
        settings.tag_source_priority = OnlineSourcePriority::QqMusicOnly;
        settings.lyrics_editor_search_source = LyricsSourceKind::QqMusic;
        settings.render_frame_rate = RenderFrameRateMode::Fps60;
        store.save(&settings).unwrap();

        let root: Value = serde_json::from_slice(&fs::read(store.file_path()).unwrap()).unwrap();
        let object = root.as_object().unwrap();
        for key in [
            "downloadDirectory",
            "libraryDirectory",
            "quality",
            "lyricsMode",
            "writeCover",
            "overwriteExisting",
            "skipExisting",
            "fileNameTemplate",
            "maxConcurrency",
            "autoMatchLocal",
            "tagSourcePriority",
            "lyricsSourcePriority",
            "ncmImportFolders",
            "ncmImportOutputDirectory",
            "lyricsEditorSearchSource",
            "fallbackKugouLyrics",
            "fallbackLrclibLyrics",
            "lyricsEditorPreferWordLyrics",
            "matchThreshold",
            "theme",
            "renderFrameRate",
            "windowMaterial",
            "glassOpacity",
            "reduceMotion",
        ] {
            assert!(object.contains_key(key), "缺少键 {key}");
        }
        assert_eq!(object.len(), 24);
        assert_eq!(root["quality"], "Lossless");
        assert_eq!(root["tagSourcePriority"], "QQMusicOnly");
        assert_eq!(root["lyricsEditorSearchSource"], "QQMusic");
        assert_eq!(root["renderFrameRate"], "Fps60");
        assert_eq!(root["theme"], "Dark");
        // 缩进输出且不留临时文件。
        assert!(
            String::from_utf8(fs::read(store.file_path()).unwrap())
                .unwrap()
                .contains("\n  \"quality\"")
        );
        assert!(!dir.path().join("settings.json.tmp").exists());
    }

    #[test]
    fn missing_fields_take_defaults_and_keep_the_rest() {
        let dir = TempDir::new();
        write_settings(
            &dir,
            r#"{"downloadDirectory":"D:\\music","maxConcurrency":5}"#,
        );

        let settings = store_in(&dir).load().unwrap();

        assert_eq!(settings.download_directory, r"D:\music");
        assert_eq!(settings.max_concurrency, 5);
        // 缺 libraryDirectory 时回落到默认下载目录（与 C# 的属性初始值一致），而不是 downloadDirectory。
        assert_eq!(settings.library_directory, DEFAULT_DIR);
        assert_eq!(settings.quality, AudioQuality::ExHigh);
        assert!(settings.write_cover);
    }

    #[test]
    fn empty_object_and_non_objects_give_defaults() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        for text in ["{}", "[]", "42", "null", "\"x\""] {
            write_settings(&dir, text);
            assert_eq!(store.load().unwrap(), store.defaults(), "输入 {text}");
        }
    }

    #[test]
    fn corrupt_or_empty_file_returns_defaults() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        for text in ["{oops", "", "{\"theme\":"] {
            write_settings(&dir, text);
            assert_eq!(store.load().unwrap(), store.defaults(), "输入 {text}");
        }
        fs::write(store.file_path(), [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(store.load().unwrap(), store.defaults());
    }

    #[test]
    fn utf8_bom_is_tolerated() {
        let dir = TempDir::new();
        write_settings(&dir, "\u{feff}{\"theme\":\"Light\"}");
        assert_eq!(store_in(&dir).load().unwrap().theme, ThemeMode::Light);
    }

    #[test]
    fn io_errors_other_than_not_found_are_reported() {
        let dir = TempDir::new();
        // 把设置路径建成目录：读取会失败，但不是“不存在”。
        fs::create_dir_all(dir.path().join("settings.json")).unwrap();
        let error = store_in(&dir).load().unwrap_err();
        assert!(error.to_string().starts_with("文件操作失败"));
    }

    #[test]
    fn keys_are_case_insensitive() {
        let dir = TempDir::new();
        write_settings(&dir, r#"{"Theme":"Light","MAXCONCURRENCY":6}"#);
        let settings = store_in(&dir).load().unwrap();
        assert_eq!(settings.theme, ThemeMode::Light);
        assert_eq!(settings.max_concurrency, 6);
    }

    #[test]
    fn unknown_theme_falls_back_to_dark() {
        let dir = TempDir::new();
        write_settings(&dir, r#"{"downloadDirectory":"D:\\music","theme":99}"#);
        let settings = store_in(&dir).load().unwrap();
        assert_eq!(settings.theme, ThemeMode::Dark);
        assert_eq!(settings.download_directory, r"D:\music");
    }

    #[test]
    fn unknown_enum_names_only_reset_their_own_field() {
        let dir = TempDir::new();
        write_settings(
            &dir,
            r#"{"theme":"Neon","lyricsMode":"Everything","renderFrameRate":7,
                "tagSourcePriority":"Nowhere","lyricsSourcePriority":"QQMusicOnly",
                "lyricsEditorSearchSource":42,"maxConcurrency":4}"#,
        );
        let settings = store_in(&dir).load().unwrap();
        assert_eq!(settings.theme, ThemeMode::Dark);
        assert_eq!(settings.lyrics_mode, LyricsWriteMode::Line);
        assert_eq!(settings.render_frame_rate, RenderFrameRateMode::VSync);
        assert_eq!(
            settings.tag_source_priority,
            OnlineSourcePriority::NeteaseFirst
        );
        assert_eq!(
            settings.lyrics_editor_search_source,
            LyricsSourceKind::QqMusic
        );
        assert_eq!(
            settings.lyrics_source_priority,
            OnlineSourcePriority::QqMusicOnly
        );
        assert_eq!(settings.max_concurrency, 4);
    }

    #[test]
    fn legacy_lyrics_editor_source_name_still_parses() {
        let dir = TempDir::new();
        write_settings(
            &dir,
            r#"{"downloadDirectory":"D:\\music","lyricsEditorSearchSource":"QQMusic"}"#,
        );
        let settings = store_in(&dir).load().unwrap();
        assert_eq!(
            settings.lyrics_editor_search_source,
            LyricsSourceKind::QqMusic
        );
        assert!(settings.fallback_kugou_lyrics);
        assert!(settings.fallback_lrclib_lyrics);
    }

    #[test]
    fn legacy_higher_quality_migrates_to_ex_high_and_keeps_other_settings() {
        let dir = TempDir::new();
        write_settings(
            &dir,
            r#"{"downloadDirectory":"D:\\music","quality":"Higher","maxConcurrency":5}"#,
        );
        let settings = store_in(&dir).load().unwrap();
        assert_eq!(settings.quality, AudioQuality::ExHigh);
        assert_eq!(settings.download_directory, r"D:\music");
        assert_eq!(settings.max_concurrency, 5);
    }

    #[test]
    fn unknown_quality_falls_back_to_default_but_keeps_other_settings() {
        let dir = TempDir::new();
        write_settings(
            &dir,
            r#"{"downloadDirectory":"D:\\music","quality":"Ultra","maxConcurrency":2}"#,
        );
        let settings = store_in(&dir).load().unwrap();
        assert_eq!(settings.quality, AudioQuality::ExHigh);
        assert_eq!(settings.download_directory, r"D:\music");
        assert_eq!(settings.max_concurrency, 2);
    }

    #[test]
    fn quality_accepts_case_insensitive_names_and_defined_numbers() {
        for (raw, expected) in [
            (r#""lossless""#, AudioQuality::Lossless),
            (r#""  HiRes ""#, AudioQuality::HiRes),
            ("8", AudioQuality::DolbyAtmos),
            ("0", AudioQuality::Standard),
            ("99", AudioQuality::ExHigh),
            ("null", AudioQuality::ExHigh),
            (r#""""#, AudioQuality::ExHigh),
        ] {
            let root: Value = serde_json::from_str(&format!("{{\"quality\":{raw}}}")).unwrap();
            assert_eq!(
                AppSettings::from_json_value(&root, DEFAULT_DIR).quality,
                expected,
                "输入 {raw}"
            );
        }
    }

    #[test]
    fn high_quality_tier_round_trips() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        let mut settings = custom();
        settings.quality = AudioQuality::DolbyAtmos;
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap().quality, AudioQuality::DolbyAtmos);
    }

    #[test]
    fn max_concurrency_is_clamped_on_save_and_load() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        let mut settings = custom();
        settings.max_concurrency = 99;
        assert_eq!(store.save(&settings).unwrap().max_concurrency, 8);
        assert_eq!(store.load().unwrap().max_concurrency, 8);

        settings.max_concurrency = -4;
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap().max_concurrency, 1);

        write_settings(&dir, r#"{"maxConcurrency":0}"#);
        assert_eq!(store.load().unwrap().max_concurrency, 1);
        write_settings(&dir, r#"{"maxConcurrency":"6"}"#);
        assert_eq!(store.load().unwrap().max_concurrency, 6);
        write_settings(&dir, r#"{"maxConcurrency":"many"}"#);
        assert_eq!(store.load().unwrap().max_concurrency, 3);
        write_settings(&dir, r#"{"maxConcurrency":2.0}"#);
        assert_eq!(store.load().unwrap().max_concurrency, 2);
    }

    #[test]
    fn match_threshold_is_rounded_clamped_and_defaults_when_invalid() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        let mut settings = custom();

        settings.match_threshold = 0.78;
        assert_eq!(store.save(&settings).unwrap().match_threshold, 0.78);
        assert_eq!(store.load().unwrap().match_threshold, 0.78);

        settings.match_threshold = 5.0;
        assert_eq!(
            store.save(&settings).unwrap().match_threshold,
            MAX_MATCH_THRESHOLD
        );

        settings.match_threshold = 0.1;
        assert_eq!(
            store.save(&settings).unwrap().match_threshold,
            MIN_MATCH_THRESHOLD
        );

        settings.match_threshold = f64::NAN;
        assert_eq!(
            store.save(&settings).unwrap().match_threshold,
            DEFAULT_MATCH_THRESHOLD
        );
        settings.match_threshold = f64::INFINITY;
        assert_eq!(
            store.save(&settings).unwrap().match_threshold,
            DEFAULT_MATCH_THRESHOLD
        );
        settings.match_threshold = -1.0;
        assert_eq!(
            store.save(&settings).unwrap().match_threshold,
            DEFAULT_MATCH_THRESHOLD
        );
        settings.match_threshold = 0.0;
        assert_eq!(
            store.save(&settings).unwrap().match_threshold,
            DEFAULT_MATCH_THRESHOLD
        );

        settings.match_threshold = 0.7849;
        assert_eq!(store.save(&settings).unwrap().match_threshold, 0.78);

        write_settings(&dir, r#"{"matchThreshold":"0.9"}"#);
        assert_eq!(store.load().unwrap().match_threshold, 0.9);
        write_settings(&dir, r#"{"matchThreshold":"abc"}"#);
        assert_eq!(
            store.load().unwrap().match_threshold,
            DEFAULT_MATCH_THRESHOLD
        );
    }

    #[test]
    fn ncm_import_memory_is_trimmed_and_deduplicated() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        let mut settings = custom();
        settings.ncm_import_folders = vec![
            r"  D:\ncm-a  ".to_owned(),
            r"D:\ncm-b".to_owned(),
            r"d:\NCM-A".to_owned(),
            String::new(),
            "   ".to_owned(),
        ];
        settings.ncm_import_output_directory = r" D:\ncm-out ".to_owned();

        store.save(&settings).unwrap();
        let loaded = store.load().unwrap();

        assert_eq!(loaded.ncm_import_folders, vec![r"D:\ncm-a", r"D:\ncm-b"]);
        assert_eq!(loaded.ncm_import_output_directory, r"D:\ncm-out");
    }

    #[test]
    fn non_string_entries_in_folder_list_are_skipped() {
        let root = json!({"ncmImportFolders": ["D:\\a", null, 3, "D:\\b", "D:\\A"], "ncmImportOutputDirectory": null});
        let settings = AppSettings::from_json_value(&root, DEFAULT_DIR);
        assert_eq!(settings.ncm_import_folders, vec![r"D:\a", r"D:\b"]);
        assert_eq!(settings.ncm_import_output_directory, "");
    }

    #[test]
    fn blank_paths_and_template_fall_back() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        let mut settings = custom();
        settings.download_directory = "   ".to_owned();
        settings.library_directory = String::new();
        settings.file_name_template = "  ".to_owned();

        let saved = store.save(&settings).unwrap();

        assert_eq!(saved.download_directory, DEFAULT_DIR);
        // 曲库目录为空时跟随（已归位的）下载目录。
        assert_eq!(saved.library_directory, DEFAULT_DIR);
        assert_eq!(saved.file_name_template, "{artist} - {title}");
    }

    #[test]
    fn library_directory_follows_download_directory_when_blank() {
        let mut settings = custom();
        settings.library_directory = " ".to_owned();
        settings.normalize(DEFAULT_DIR);
        assert_eq!(settings.library_directory, r"D:\music");
    }

    #[test]
    fn wrongly_typed_fields_fall_back_individually() {
        let root = json!({
            "writeCover": "yes", "skipExisting": 0, "autoMatchLocal": null,
            "downloadDirectory": 5, "fileNameTemplate": [], "theme": "Light"
        });
        let settings = AppSettings::from_json_value(&root, DEFAULT_DIR);
        assert!(settings.write_cover);
        assert!(settings.skip_existing);
        assert!(settings.auto_match_local);
        assert_eq!(settings.download_directory, DEFAULT_DIR);
        assert_eq!(settings.file_name_template, "{artist} - {title}");
        assert_eq!(settings.theme, ThemeMode::Light);
    }

    #[test]
    fn save_replaces_existing_file_atomically() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        let mut settings = custom();
        store.save(&settings).unwrap();
        settings.theme = ThemeMode::Light;
        store.save(&settings).unwrap();

        assert_eq!(store.load().unwrap().theme, ThemeMode::Light);
        let entries: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(entries.len(), 1, "不应遗留 .tmp 文件");
    }

    #[test]
    fn save_creates_missing_directories() {
        let dir = TempDir::new();
        let store = JsonSettingsStore::new(
            dir.path()
                .join("nested")
                .join("deeper")
                .join("settings.json"),
            DEFAULT_DIR,
        );
        store.save(&custom()).unwrap();
        assert!(store.file_path().is_file());
    }

    #[test]
    fn appearance_defaults_and_glass_opacity_is_clamped() {
        let defaults = AppSettings::new(r"D:\Music");
        assert_eq!(defaults.window_material, WindowMaterial::Acrylic);
        assert_eq!(defaults.glass_opacity, DEFAULT_GLASS_OPACITY);
        assert!(!defaults.reduce_motion);

        let read = |json: &str| {
            AppSettings::from_json_value(&serde_json::from_str(json).unwrap(), r"D:\Music")
        };
        assert_eq!(
            read(r#"{"glassOpacity":0.0}"#).glass_opacity,
            MIN_GLASS_OPACITY
        );
        assert_eq!(
            read(r#"{"glassOpacity":3}"#).glass_opacity,
            MAX_GLASS_OPACITY
        );
        assert_eq!(read(r#"{"glassOpacity":0.333}"#).glass_opacity, 0.33);
        let custom = read(r#"{"windowMaterial":"mica","reduceMotion":true}"#);
        assert_eq!(custom.window_material, WindowMaterial::Mica);
        assert!(custom.reduce_motion);
        assert_eq!(
            read(r#"{"windowMaterial":"Glass"}"#).window_material,
            WindowMaterial::Acrylic
        );
    }

    #[test]
    fn apply_patch_changes_only_the_given_keys() {
        let mut current = AppSettings::new(r"D:\Music");
        current.ncm_import_folders = vec![
            r"D:
cm"
            .to_owned(),
        ];
        let patch = serde_json::json!({
            "theme": "Light",
            "windowMaterial": "None",
            "glassOpacity": 0.7,
            "maxConcurrency": 6,
            "matchThreshold": 0.75,
            "downloadDirectory": r"E:\下载",
            "fallbackKugouLyrics": false,
        });
        let next = current.apply_patch(&patch, r"D:\Music").unwrap();
        assert_eq!(next.theme, ThemeMode::Light);
        assert_eq!(next.window_material, WindowMaterial::None);
        assert_eq!(next.glass_opacity, 0.7);
        assert_eq!(next.max_concurrency, 6);
        assert_eq!(next.match_threshold, 0.75);
        assert_eq!(next.download_directory, r"E:\下载");
        assert!(!next.fallback_kugou_lyrics);
        // 没提交的字段原样保留（包括界面上不显示的 NCM 导入记忆）。
        assert_eq!(next.ncm_import_folders, current.ncm_import_folders);
        assert_eq!(next.quality, current.quality);
        assert_eq!(next.library_directory, current.library_directory);
    }

    #[test]
    fn apply_patch_rejects_invalid_input_without_changing_anything() {
        let current = AppSettings::new(r"D:\Music");
        let rejected = [
            serde_json::json!({ "nope": 1 }),
            serde_json::json!({ "theme": "Neon" }),
            serde_json::json!({ "theme": 1 }),
            serde_json::json!({ "quality": "Ultra" }),
            serde_json::json!({ "maxConcurrency": 9 }),
            serde_json::json!({ "maxConcurrency": 2.5 }),
            serde_json::json!({ "matchThreshold": 0.2 }),
            serde_json::json!({ "glassOpacity": 1.0 }),
            serde_json::json!({ "writeCover": "yes" }),
            serde_json::json!({ "ncmImportFolders": [1] }),
            serde_json::json!([1, 2]),
        ];
        for patch in rejected {
            assert!(
                current.apply_patch(&patch, r"D:\Music").is_err(),
                "应拒绝 {patch}"
            );
        }
        // 空路径与空模板由 normalize 归位，而不是报错。
        let next = current
            .apply_patch(
                &serde_json::json!({ "downloadDirectory": "  ", "fileNameTemplate": "" }),
                r"D:\Music",
            )
            .unwrap();
        assert_eq!(next.download_directory, r"D:\Music");
        assert_eq!(next.file_name_template, DEFAULT_FILE_NAME_TEMPLATE);
    }
}
