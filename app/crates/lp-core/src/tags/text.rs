//! 标签文本的纯函数小工具：修整、拆分、网易云 ID 标记与备注清理。
//!
//! 逐个对应 C# `AudioTagService` 里的私有静态方法，行为保持一致（见各函数说明）。

use std::sync::LazyLock;

use regex::Regex;

use crate::ncm::client_comment;

/// 写进备注里的网易云 ID 标记前缀。
pub const NETEASE_COMMENT_PREFIX: &str = "LunaticPrincess:";

static NETEASE_ID_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)LunaticPrincess\s*:\s*(\d{1,19})").expect("网易云 ID 标记的正则是常量")
});

static NETEASE_COMMENT_CLEANUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)LunaticPrincess(?:\s*:\s*\d{1,19})?").expect("标记清理的正则是常量")
});

/// 修整空白；全空白或 `None` 返回 `None`（C# `NullIfBlank`）。
pub(super) fn null_if_blank(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// 把多个值修整后用 `/` 连接；没有有效值返回 `None`（C# `JoinValues`）。
pub(super) fn join_values<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let joined = values
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("/");
    (!joined.trim().is_empty()).then_some(joined)
}

/// 按 `/`、`;`、`；`、换行拆成多个值，修整并丢弃空值（C# `SplitValues`）。
pub(super) fn split_values(value: Option<&str>) -> Vec<String> {
    let Some(value) = value.filter(|v| !v.trim().is_empty()) else {
        return Vec::new();
    };
    value
        .split(['/', ';', '；', '\r', '\n'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 丢弃空白项并修整（C# `CleanValues`）。
pub(super) fn clean_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_owned())
        .collect()
}

/// 备注的收尾清理：修整空白，再去掉两端残留的分隔符（C# `CleanComment`）。
pub(super) fn clean_comment(value: &str) -> String {
    value
        .trim()
        .trim_matches([';', ',', '|', '\r', '\n', '\t', ' ', '\0'])
        .trim()
        .to_owned()
}

/// 去掉备注里的 `LunaticPrincess` / `LunaticPrincess:<id>` 标记（含只剩单词本身的情况）。
pub(super) fn strip_netease_marker(value: &str) -> String {
    NETEASE_COMMENT_CLEANUP.replace_all(value, "").into_owned()
}

/// 备注用于展示的版本：去掉网易云 ID 标记并清理残留分隔符，空则 `None`（C# `CleanDisplayComment`）。
pub(super) fn clean_display_comment(comment: Option<&str>) -> Option<String> {
    let comment = comment.filter(|c| !c.trim().is_empty())?;
    null_if_blank(Some(&clean_comment(&strip_netease_marker(comment))))
}

/// `LunaticPrincess:<id>` 标记（`id` 位数过多或溢出时返回 `Err` 表示“有标记但无效”）。
fn parse_marker(comment: &str) -> Option<Option<i64>> {
    let captures = NETEASE_ID_PATTERN.captures(comment)?;
    Some(captures[1].parse::<i64>().ok())
}

/// 从一段备注里取网易云歌曲 ID：先认本程序的标记，没有标记再认客户端文件里的 `163 key`。
///
/// 与 C# 一致：有标记但数字溢出 `i64` 时直接视为没有 ID，不再回落到 `163 key`。
pub fn parse_netease_id(comment: Option<&str>) -> Option<i64> {
    let comment = comment.filter(|c| !c.trim().is_empty())?;
    match parse_marker(comment) {
        Some(parsed) => parsed,
        None => client_comment::try_read_music_id(Some(comment)),
    }
}

/// 按文件头判断图片 MIME；认不出时按 JPEG 处理（C# `DetectImageMime`）。
pub fn detect_image_mime(data: &[u8]) -> &'static str {
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        "image/png"
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if data.starts_with(&[0x47, 0x49, 0x46, 0x38]) {
        "image/gif"
    } else if data.starts_with(&[0x42, 0x4D]) {
        "image/bmp"
    } else if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

#[cfg(test)]
mod tests;
