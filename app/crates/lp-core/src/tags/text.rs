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

/// 读图片的像素宽高（PNG / JPEG / GIF / BMP / WebP），只看文件头，不解码；认不出或数据不完整时返回 `None`。
///
/// 自动补全用它判断新封面是不是比文件里已有的更清楚。
pub fn image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let u16_be = |at: usize| {
        Some(u32::from(u16::from_be_bytes(
            data.get(at..at + 2)?.try_into().ok()?,
        )))
    };
    let u16_le = |at: usize| {
        Some(u32::from(u16::from_le_bytes(
            data.get(at..at + 2)?.try_into().ok()?,
        )))
    };
    let u32_be = |at: usize| Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?));
    let u24_le = |at: usize| {
        let bytes = data.get(at..at + 3)?;
        Some(u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16))
    };
    let size = match detect_image_mime(data) {
        "image/png" if data.get(12..16) == Some(b"IHDR") => (u32_be(16)?, u32_be(20)?),
        "image/gif" => (u16_le(6)?, u16_le(8)?),
        "image/bmp" => {
            let width = i32::from_le_bytes(data.get(18..22)?.try_into().ok()?);
            let height = i32::from_le_bytes(data.get(22..26)?.try_into().ok()?);
            (width.unsigned_abs(), height.unsigned_abs())
        }
        "image/webp" => match data.get(12..16)? {
            b"VP8 " => (u16_le(26)? & 0x3FFF, u16_le(28)? & 0x3FFF),
            b"VP8L" => {
                let bits = u32::from_le_bytes(data.get(21..25)?.try_into().ok()?);
                ((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1)
            }
            b"VP8X" => (u24_le(24)? + 1, u24_le(27)? + 1),
            _ => return None,
        },
        _ if data.starts_with(&[0xFF, 0xD8]) => {
            // 逐段跳过，直到帧头 SOFn（C0–CF，除了 C4 DHT、C8 保留、CC DAC）。
            let mut at = 2;
            loop {
                while data.get(at) == Some(&0xFF) && data.get(at + 1) == Some(&0xFF) {
                    at += 1;
                }
                if *data.get(at)? != 0xFF {
                    return None;
                }
                let marker = *data.get(at + 1)?;
                if matches!(marker, 0x01 | 0xD0..=0xD8) {
                    at += 2;
                    continue;
                }
                if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                    break (u16_be(at + 7)?, u16_be(at + 5)?);
                }
                at += 2 + usize::try_from(u16_be(at + 2)?).ok()?;
            }
        }
        _ => return None,
    };
    (size.0 > 0 && size.1 > 0).then_some(size)
}

#[cfg(test)]
mod tests;
