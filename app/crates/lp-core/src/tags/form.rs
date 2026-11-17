//! 标签编辑表单的纯逻辑：歌词与标签编辑器（[`editor`]）与批量修改标签（[`batch`]）共用的规则。
//!
//! 这些规则原来写在 Tauri 版前端（`useLyricsEditor.ts`、`useBatchTags.ts`）里。搬进 lp-core 之后，
//! 不论界面换成什么，「保存时写什么、哪些字段不动、数字怎么校验」都只有这一份实现，并且有单测。
//! 界面只负责把表单里的文字交进来、把结果显示出去。

use std::fmt;

use serde::{Deserialize, Serialize};

use super::model::TagField;

pub mod batch;
pub mod editor;

/// 封面的改动（编辑器与批量面板共用）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CoverChange {
    /// 不动文件里的封面。
    #[default]
    Keep,
    /// 换成这张图片（图片字节的 Base64，不带 `data:` 前缀）。
    Replace { base64: String, mime_type: String },
    /// 保存时删除文件里的全部封面。
    Remove,
}

/// 表单里填的内容不能保存的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormError {
    /// 年份 / 音轨号 / 碟号填的不是非负整数（或超出范围）。
    NotANumber(TagField),
}

impl fmt::Display for FormError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotANumber(field) => write!(f, "{}必须是数字。", number_label(*field)),
        }
    }
}

impl std::error::Error for FormError {}

/// 数字字段在提示里的叫法。
fn number_label(field: TagField) -> &'static str {
    match field {
        TagField::Year => "年份",
        TagField::TrackNumber => "音轨号",
        TagField::DiscNumber => "碟号",
        _ => "这一项",
    }
}

/// 艺术家等多值字段按 `/ ; ； 、` 拆成多个（去掉首尾空白与空项）。
///
/// 与 Tauri 版前端的 `splitArtists` 一致；名字里带 `/` 的人会被拆开，所以编辑器保存时只在用户改过这一行时才用它
/// （见 [`editor::multi_value_plan`]）。
pub fn split_artists(text: &str) -> Vec<String> {
    text.split(['/', ';', '；', '、'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 去掉首尾空白；剩下空串时为 `None`。
pub fn trimmed_or_none(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// 年份 / 音轨号 / 碟号：空白为 `Ok(None)`；只认 ASCII 数字（允许前导零），超出 `u32` 或含其他字符时报错。
pub fn parse_number(field: TagField, text: &str) -> Result<Option<u32>, FormError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if !trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FormError::NotANumber(field));
    }
    trimmed
        .parse::<u32>()
        .map(Some)
        .map_err(|_| FormError::NotANumber(field))
}

#[cfg(test)]
mod tests;
