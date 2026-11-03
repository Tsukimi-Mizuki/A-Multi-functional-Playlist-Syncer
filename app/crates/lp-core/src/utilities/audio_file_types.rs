//! 音频文件类型判定（歌词 / 标签编辑器的拖放载入、文件对话框过滤共用）。
//!
//! 对应 C# `AudioFileTypes`。与下载用的 `MediaFileHelper` 分开：这里只关心“能不能当作可编辑音频读入”。

use std::path::{Path, PathBuf};

/// 编辑器接受的扩展名（含点，小写；不含 `.ncm`：NCM 需先解密）。
pub const SUPPORTED_EXTENSIONS: [&str; 9] = [
    ".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".aac", ".wma",
];

/// 给用户看的一行格式说明。
pub const EXTENSION_HINT: &str = "MP3 / FLAC / M4A / MP4 / OGG / OPUS / WAV / AAC / WMA";

/// 目标为空时的提示。
pub const EMPTY_TARGET_MESSAGE: &str = "没有识别到拖入的文件路径。";

/// 文件夹里没有可编辑音频时的提示。
pub const NO_AUDIO_IN_FOLDER_MESSAGE: &str = "该文件夹里没有可编辑的音频文件。";

/// 和 .NET `Path.GetExtension` 一样的扩展名规则：取文件名里最后一个 `.` 起的部分，`.` 在末尾则为空。
fn extension_of(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(index) if index + 1 < name.len() => &name[index..],
        _ => "",
    }
}

/// 扩展名是否可编辑（只看扩展名，不读文件内容；不区分大小写）。
pub fn is_supported(path: Option<&str>) -> bool {
    let Some(path) = path.map(str::trim).filter(|p| !p.is_empty()) else {
        return false;
    };
    let extension = extension_of(path);
    !extension.is_empty()
        && SUPPORTED_EXTENSIONS
            .iter()
            .any(|supported| supported.eq_ignore_ascii_case(extension))
}

/// 把拖放目标解析成可载入的音频文件：文件直接返回；文件夹返回其中按名称排序的第一个受支持文件。
///
/// 失败时返回给用户看的中文提示。
pub fn try_resolve(target: Option<&str>) -> Result<PathBuf, String> {
    let Some(candidate) = target.map(str::trim).filter(|t| !t.is_empty()) else {
        return Err(EMPTY_TARGET_MESSAGE.to_owned());
    };

    let path = Path::new(candidate);
    if path.is_file() {
        if !is_supported(Some(candidate)) {
            let extension = extension_of(candidate);
            return Err(if extension.is_empty() {
                format!("暂不支持无扩展名的文件，请拖入 {EXTENSION_HINT} 格式的音频文件。")
            } else {
                format!("暂不支持「{extension}」格式，请拖入 {EXTENSION_HINT} 格式的音频文件。")
            });
        }
        return Ok(path.to_path_buf());
    }

    if path.is_dir() {
        return list_supported_files(Some(candidate))
            .into_iter()
            .next()
            .ok_or_else(|| NO_AUDIO_IN_FOLDER_MESSAGE.to_owned());
    }

    Err(format!("找不到「{candidate}」。"))
}

/// 列出目录中可编辑的音频文件（只看当前层级，按文件名排序，不区分大小写）。
pub fn list_supported_files(directory: Option<&str>) -> Vec<PathBuf> {
    let Some(directory) = directory.filter(|d| !d.trim().is_empty()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_supported(path.to_str()))
        .collect();
    files.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    files
}

#[cfg(test)]
mod tests;
