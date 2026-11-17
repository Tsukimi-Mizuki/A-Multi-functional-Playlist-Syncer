//! 设置：读取当前设置、保存界面改过的键。
//!
//! Flutter 端只迁移「真的有效果」的设置项。Tauri 版的液态玻璃、渲染帧率、背景雾、渐变预设、
//! 背景图这些外观键在 Flutter 里没有对应物，因此既不在 DTO 里，也不由保存逻辑提交；
//! 它们继续原样留在 `settings.json` 里，Tauri 版仍能读到。

use lp_core::models::AudioQuality;
use lp_core::settings::AppSettings;
use serde_json::Value;

use crate::state::bridge;

/// 音质档位选项（下拉框的每一项）。
pub struct QualityOptionDto {
    /// 档位名，与 [`AppSettingsDto::quality`] 的取值一致。
    pub value: String,
    /// 中文显示名。
    pub label: String,
}

/// Flutter 端读写的设置。
///
/// 枚举类字段（主题、窗口材质、音质、歌词写法）一律按 `lp-core` 自己的 serde 名给出，
/// 这样桥接层不必再维护第二份「枚举名 ↔ 字符串」映射表。
pub struct AppSettingsDto {
    /// 主题：`system` / `light` / `dark`。
    pub theme: String,
    /// 窗口材质：`mica` / `acrylic` / `none`。
    pub window_material: String,
    /// 下载目录。
    pub download_directory: String,
    /// 本地曲库目录（扫描与对比用）。
    pub library_directory: String,
    /// 音质档位。
    pub quality: String,
    /// 歌词写法：`line` / `word` 等。
    pub lyrics_mode: String,
    /// 写入封面。
    pub write_cover: bool,
    /// 覆盖已有文件。
    pub overwrite_existing: bool,
    /// 跳过已有文件。
    pub skip_existing: bool,
    /// 文件名模板（如 `{artist} - {title}`）。
    pub file_name_template: String,
    /// 并发下载数（1–8）。
    pub max_concurrency: i64,
    /// 打开歌单后自动对比本地曲库。
    pub auto_match_local: bool,
    /// 本地匹配的置信度阈值（0.30–1.00）。
    pub match_threshold: f64,
    /// 减少动画。
    pub reduce_motion: bool,
    /// 启动时检查更新。
    pub auto_check_updates: bool,
}

/// 设置窗口要用的全部数据。
pub struct SettingsDto {
    /// 当前设置。
    pub settings: AppSettingsDto,
    /// 数据目录。
    pub data_dir: String,
    /// 设置文件路径。
    pub settings_file: String,
    /// 是否便携模式。
    pub is_portable: bool,
    /// 默认下载目录（「恢复默认」用）。
    pub default_download_dir: String,
    /// 设置文件读取失败时的中文原因（保存成功后消失）。
    pub warning: Option<String>,
    /// 音质档位选项。
    pub quality_options: Vec<QualityOptionDto>,
}

impl AppSettingsDto {
    /// 把设置转成界面用的 DTO。
    fn build(settings: &AppSettings) -> Result<Self, String> {
        let names =
            serde_json::to_value(settings).map_err(|error| format!("读取设置失败：{error}"))?;
        let name = |key: &str| {
            names
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_default()
        };
        Ok(Self {
            theme: name("theme"),
            window_material: name("windowMaterial"),
            download_directory: settings.download_directory.clone(),
            library_directory: settings.library_directory.clone(),
            quality: name("quality"),
            lyrics_mode: name("lyricsMode"),
            write_cover: settings.write_cover,
            overwrite_existing: settings.overwrite_existing,
            skip_existing: settings.skip_existing,
            file_name_template: settings.file_name_template.clone(),
            max_concurrency: settings.max_concurrency,
            auto_match_local: settings.auto_match_local,
            match_threshold: settings.match_threshold,
            reduce_motion: settings.reduce_motion,
            auto_check_updates: settings.auto_check_updates,
        })
    }
}

fn describe(bridge: &crate::state::Bridge) -> Result<SettingsDto, String> {
    Ok(SettingsDto {
        settings: AppSettingsDto::build(&bridge.settings())?,
        data_dir: bridge.paths.data_dir.display().to_string(),
        settings_file: bridge.settings_store.file_path().display().to_string(),
        is_portable: bridge.paths.is_portable,
        default_download_dir: bridge.paths.default_download_dir.display().to_string(),
        warning: bridge.settings_warning(),
        quality_options: quality_options()?,
    })
}

/// 音质下拉框的选项。
fn quality_options() -> Result<Vec<QualityOptionDto>, String> {
    let mut options = Vec::with_capacity(AudioQuality::ALL.len());
    for quality in AudioQuality::ALL {
        options.push(QualityOptionDto {
            value: enum_value_name(*quality)?,
            label: quality.display_name().to_owned(),
        });
    }
    Ok(options)
}

/// 取一个枚举值的 serde 名（音质档位等）。
fn enum_value_name<T: serde::Serialize>(value: T) -> Result<String, String> {
    match serde_json::to_value(value).map_err(|error| format!("读取设置失败：{error}"))? {
        Value::String(name) => Ok(name),
        _ => Err("读取设置失败：音质档位名不是字符串".to_owned()),
    }
}

/// 读取当前设置。
pub fn load_settings() -> Result<SettingsDto, String> {
    describe(bridge()?)
}

/// 保存设置窗口提交的修改。
///
/// `patch` 是 JSON 对象文本（Dart 端 `jsonEncode`），可以只包含改动过的键：
/// 未出现的键保持原值，旧外观键也不会被顺手写回或抹掉。
pub fn save_settings(patch: String) -> Result<SettingsDto, String> {
    let patch: Value =
        serde_json::from_str(&patch).map_err(|error| format!("保存设置失败：{error}"))?;
    let bridge = bridge()?;
    bridge.save_settings_patch(&patch)?;
    describe(bridge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn sample() -> AppSettings {
        AppSettings::new("C:/Music")
    }

    #[test]
    fn builds_enum_fields_from_lp_core_serde_names() {
        let dto = AppSettingsDto::build(&sample()).expect("构建设置 DTO");
        // 这三个值必须和 lp-core 的 serde 名一致，否则界面上的下拉框选不中。
        assert_eq!(dto.theme, "Dark");
        assert_eq!(dto.window_material, "Acrylic");
        assert_eq!(dto.quality, "ExHigh");
    }

    #[test]
    fn copies_plain_fields_verbatim() {
        let mut settings = sample();
        settings.library_directory = "C:/Library".to_owned();
        settings.auto_match_local = true;
        settings.match_threshold = 0.75;
        settings.max_concurrency = 5;
        let dto = AppSettingsDto::build(&settings).expect("构建设置 DTO");
        assert_eq!(dto.library_directory, "C:/Library");
        assert_eq!(dto.download_directory, "C:/Music");
        assert!(dto.auto_match_local);
        assert!((dto.match_threshold - 0.75).abs() < f64::EPSILON);
        assert_eq!(dto.max_concurrency, 5);
    }

    #[test]
    fn quality_options_cover_every_level_once() {
        let options = quality_options().expect("读取音质选项");
        assert_eq!(options.len(), AudioQuality::ALL.len());
        let values: HashSet<&str> = options.iter().map(|o| o.value.as_str()).collect();
        assert_eq!(values.len(), options.len(), "音质取值不能重复");
        for option in &options {
            assert!(!option.label.is_empty(), "音质选项缺少显示名");
            assert!(!option.value.is_empty(), "音质选项缺少取值");
        }
    }
}
