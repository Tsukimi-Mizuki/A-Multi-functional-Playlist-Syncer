//! 批量修改标签面板的表单规则：多首歌的共同值、「保持」、改动清单，以及由它生成的 [`TagPatch`]。
//!
//! 面板左边显示选中歌曲的标签：值都一样就显示这个值，不一样就显示「保持」。用户只要动过某个字段，
//! 保存时才会把它写进所有选中的文件；没动过的字段原样保留。这些规则原来在前端 `useBatchTags.ts` 里，
//! 搬到这里让不同界面共用一份。

use std::collections::HashMap;

use serde::Serialize;

use super::{CoverChange, FormError, parse_number, split_artists, trimmed_or_none};
use crate::tags::model::TagField;
use crate::tags::table::{TagPatch, TagRowView};

/// 面板里的字段，按显示顺序。
pub const BATCH_FIELDS: [TagField; 11] = [
    TagField::Title,
    TagField::Artist,
    TagField::Album,
    TagField::Year,
    TagField::TrackNumber,
    TagField::DiscNumber,
    TagField::Genre,
    TagField::AlbumArtist,
    TagField::Composer,
    TagField::Lyricist,
    TagField::Comment,
];

/// 用户在面板里改过的字段（字段 -> 输入框里的文字）。没出现的字段表示没动过。
pub type TagEdits = HashMap<TagField, String>;

/// 一个字段在所有选中歌曲里的共同值（camelCase）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommonValue {
    /// 字段。
    pub field: TagField,
    /// 共同值；各首不一样时为空串。
    pub value: String,
    /// 各首的值是否不一样（界面显示「保持」）。
    pub mixed: bool,
}

/// 面板里一个字段此刻该显示的内容（camelCase）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldView {
    /// 字段。
    pub field: TagField,
    /// 输入框里的文字。
    pub value: String,
    /// 选中的歌曲在这个字段上是否各不相同。
    pub mixed: bool,
    /// 用户是否改过它（保存时才会写）。
    pub edited: bool,
}

/// 路径比较用的键：Windows 路径不分大小写、`/` 与 `\` 等价。
pub fn path_key(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}

/// 一行里某个字段的文字；没有值是空串，数字按十进制。
pub fn row_value(row: &TagRowView, field: TagField) -> String {
    let text = |value: &Option<String>| value.clone().unwrap_or_default();
    let number = |value: Option<u32>| value.map(|n| n.to_string()).unwrap_or_default();
    match field {
        TagField::Title => text(&row.title),
        TagField::Artist => text(&row.artist),
        TagField::Album => text(&row.album),
        TagField::AlbumArtist => text(&row.album_artist),
        TagField::Year => number(row.year),
        TagField::TrackNumber => number(row.track_number),
        TagField::DiscNumber => number(row.disc_number),
        TagField::Genre => text(&row.genre),
        TagField::Composer => text(&row.composer),
        TagField::Lyricist => text(&row.lyricist),
        TagField::Comment => text(&row.comment),
    }
}

/// 选中歌曲的共同值，顺序同 [`BATCH_FIELDS`]；调用方只传读得出标签的选中行。
///
/// 以第一行为准，只要有一行不一样就是「保持」（值为空串）；没有行时每项都是空串、不算「保持」。
pub fn common_values(rows: &[TagRowView]) -> Vec<CommonValue> {
    BATCH_FIELDS
        .iter()
        .map(|&field| {
            let first = rows
                .first()
                .map(|row| row_value(row, field))
                .unwrap_or_default();
            let mixed = rows.iter().any(|row| row_value(row, field) != first);
            CommonValue {
                field,
                value: if mixed { String::new() } else { first },
                mixed,
            }
        })
        .collect()
}

/// 用户改了某个字段。
///
/// 值与共同值相同（且各首本来一样）说明改回去了，不算改动，把记录删掉；
/// 「保持」的字段改成什么都算改动，包括空串（表示把所有选中歌曲的这一项清掉）。
pub fn set_edit(edits: &mut TagEdits, common: &[CommonValue], field: TagField, value: String) {
    // 找不到这个字段时当作「没有值、也不是保持」，改成空串仍算没改。
    let unchanged = common
        .iter()
        .find(|item| item.field == field)
        .map_or(value.is_empty(), |item| !item.mixed && item.value == value);
    if unchanged {
        edits.remove(&field);
    } else {
        edits.insert(field, value);
    }
}

/// 面板上每个字段此刻的显示：没改过显示共同值，改过显示用户输入的文字。顺序同 `common`。
pub fn field_views(common: &[CommonValue], edits: &TagEdits) -> Vec<FieldView> {
    common
        .iter()
        .map(|item| match edits.get(&item.field) {
            Some(text) => FieldView {
                field: item.field,
                value: text.clone(),
                mixed: item.mixed,
                edited: true,
            },
            None => FieldView {
                field: item.field,
                value: item.value.clone(),
                mixed: item.mixed,
                edited: false,
            },
        })
        .collect()
}

/// 由改动清单生成要交给后端的 [`TagPatch`]：只带改过的字段，空白表示清除。
///
/// 数字按年份、音轨号、碟号的顺序检查，同时有几项填错时总是先报前面的，与 `HashMap` 的遍历顺序无关。
/// 什么都没改时返回空的补丁，「没有要修改的内容」由后端的 `TagPatch::into_data` 报告。
pub fn build_patch(edits: &TagEdits, cover: &CoverChange) -> Result<TagPatch, FormError> {
    let text = |field: TagField| edits.get(&field).map(|text| trimmed_or_none(text));
    let number = |field: TagField| -> Result<Option<Option<u32>>, FormError> {
        edits
            .get(&field)
            .map(|text| parse_number(field, text))
            .transpose()
    };
    let mut patch = TagPatch {
        title: text(TagField::Title),
        album: text(TagField::Album),
        album_artist: text(TagField::AlbumArtist),
        genre: text(TagField::Genre),
        composer: text(TagField::Composer),
        lyricist: text(TagField::Lyricist),
        comment: text(TagField::Comment),
        artists: edits.get(&TagField::Artist).map(|text| split_artists(text)),
        ..TagPatch::default()
    };
    patch.year = number(TagField::Year)?;
    patch.track_number = number(TagField::TrackNumber)?;
    patch.disc_number = number(TagField::DiscNumber)?;

    match cover {
        CoverChange::Keep => {}
        CoverChange::Replace { base64, mime_type } => {
            patch.cover_base64 = Some(base64.clone());
            patch.cover_mime_type = Some(mime_type.clone());
        }
        CoverChange::Remove => patch.remove_cover = true,
    }
    Ok(patch)
}

#[cfg(test)]
mod tests;
