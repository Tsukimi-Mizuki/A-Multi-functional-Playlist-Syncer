//! 歌词与标签编辑器的表单规则：打开文件时表单怎么填、保存时写什么、套用在线结果时改哪些字段。
//!
//! 这些规则原来写在 Tauri 版前端的 `useLyricsEditor.ts` 里，搬到这里之后不论界面是什么，
//! 行为只有这一份，并有单测。界面只负责保存表单文字、记住「打开文件时的表单」与「最近套用的在线歌手清单」，
//! 再把它们交进来。

use serde::{Deserialize, Serialize};

use super::{CoverChange, FormError, parse_number, split_artists, trimmed_or_none};
use crate::online::{FileContext, OnlineSongResult, keyword_from_file_name};
use crate::tags::{TagField, TagView, WriteTagsRequest};

/// 界面上用 `/` 接成一行显示的多值字段（顺序即 `keep` 里的顺序）。
pub const MULTI_VALUE_FIELDS: [TagField; 5] = [
    TagField::Artist,
    TagField::AlbumArtist,
    TagField::Genre,
    TagField::Composer,
    TagField::Lyricist,
];

/// 编辑器表单里的全部文字（数字也是文字，保存时才校验）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorFields {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub year: String,
    pub track_number: String,
    pub disc_number: String,
    pub genre: String,
    pub composer: String,
    pub lyricist: String,
    pub comment: String,
    pub lyrics: String,
}

impl EditorFields {
    /// 由读出的标签填表单：没有的字段是空串，数字转十进制文字。
    ///
    /// 歌词优先用文件里内嵌的；没有内嵌歌词时显示旁边同名 `.lrc` 的内容（保存时随表单一起写进音频文件）。
    pub fn from_view(view: &TagView) -> Self {
        let text = |value: &Option<String>| value.clone().unwrap_or_default();
        let number = |value: Option<u32>| value.map(|n| n.to_string()).unwrap_or_default();
        Self {
            title: text(&view.title),
            artist: text(&view.artist),
            album: text(&view.album),
            album_artist: text(&view.album_artist),
            year: number(view.year),
            track_number: number(view.track_number),
            disc_number: number(view.disc_number),
            genre: text(&view.genre),
            composer: text(&view.composer),
            lyricist: text(&view.lyricist),
            comment: text(&view.comment),
            lyrics: view
                .lyrics
                .clone()
                .or_else(|| view.sidecar_lyrics.as_ref().map(|lrc| lrc.text.clone()))
                .unwrap_or_default(),
        }
    }

    /// 按字段名取值（歌词不是 [`TagField`]，不在其中）；用来逐字段比较「改过没有」。
    pub fn value(&self, field: TagField) -> &str {
        match field {
            TagField::Title => &self.title,
            TagField::Artist => &self.artist,
            TagField::Album => &self.album,
            TagField::AlbumArtist => &self.album_artist,
            TagField::Year => &self.year,
            TagField::TrackNumber => &self.track_number,
            TagField::DiscNumber => &self.disc_number,
            TagField::Genre => &self.genre,
            TagField::Composer => &self.composer,
            TagField::Lyricist => &self.lyricist,
            TagField::Comment => &self.comment,
        }
    }
}

/// 在线结果给的艺术家清单，连同它接成的那行文字（表单里填的就是这行字）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistList {
    /// 接成的那行文字，原样保存（不修整），用来判断表单里的艺术家有没有被用户再改过。
    pub text: String,
    /// 来源给的各位歌手，一人一项。
    pub values: Vec<String>,
}

impl ArtistList {
    /// 在线结果里的艺术家清单；来源没给清单、或那行文字是空白时为 `None`。
    pub fn from_online(song: &OnlineSongResult, text: &str) -> Option<Self> {
        let values = song.artist_values()?;
        (!text.trim().is_empty()).then(|| Self {
            text: text.to_owned(),
            values,
        })
    }
}

/// 保存时多值字段怎么写。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiValuePlan {
    /// 写进文件的艺术家，一人一项。
    pub artists: Vec<String>,
    /// 保持不动的字段。
    pub keep: Vec<TagField>,
}

/// 决定保存时多值字段怎么写。
///
/// 艺术家等在文件里是一个个独立的值，界面上却是用 `/` 接起来的一行字；如果保存时一律按 `/` 拆，
/// 名字里带 `/` 的人（`AC/DC`、`+α/あるふぁきゅん。`）就会被拆成两个。所以：
/// - 艺术家这一行还是在线结果接出来的那一串：直接用在线结果的清单（边界是来源给的，准确）；
/// - 某个多值字段和打开文件时一样（没改过）：列进 `keep`，后端对它一个字节都不碰；
/// - 用户改过的：只能按分隔符拆。
pub fn multi_value_plan(
    values: &EditorFields,
    loaded: Option<&EditorFields>,
    online: Option<&ArtistList>,
) -> MultiValuePlan {
    let from_online = online.is_some_and(|list| list.text == values.artist);
    let keep = MULTI_VALUE_FIELDS
        .into_iter()
        .filter(|field| {
            loaded.is_some_and(|loaded| values.value(*field) == loaded.value(*field))
                && !(*field == TagField::Artist && from_online)
        })
        .collect();
    let artists = match online {
        Some(list) if from_online => list.values.clone(),
        _ => split_artists(&values.artist),
    };
    MultiValuePlan { artists, keep }
}

/// 保存用的写入请求。
///
/// 先校验年份、音轨号、碟号（按这个顺序，第一个错误为准）。`replace_all` 为真：表单里展示的字段整体替换，
/// 没填的就清除；歌词不修整（保留用户输入的原样），空白则不写歌词；网易云 ID 沿用文件里读出的那个，
/// 只认纯数字。
pub fn build_write_request(
    view: &TagView,
    values: &EditorFields,
    loaded: Option<&EditorFields>,
    online: Option<&ArtistList>,
    cover: &CoverChange,
) -> Result<WriteTagsRequest, FormError> {
    let year = parse_number(TagField::Year, &values.year)?;
    let track_number = parse_number(TagField::TrackNumber, &values.track_number)?;
    let disc_number = parse_number(TagField::DiscNumber, &values.disc_number)?;

    let netease_id = view
        .netease_id
        .as_deref()
        .filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|id| id.parse::<i64>().ok());
    let plan = multi_value_plan(values, loaded, online);
    let (cover_base64, cover_mime_type, remove_cover) = match cover {
        CoverChange::Keep => (None, None, false),
        CoverChange::Replace { base64, mime_type } => {
            (Some(base64.clone()), Some(mime_type.clone()), false)
        }
        CoverChange::Remove => (None, None, true),
    };
    Ok(WriteTagsRequest {
        title: trimmed_or_none(&values.title),
        artists: plan.artists,
        album: trimmed_or_none(&values.album),
        album_artist: trimmed_or_none(&values.album_artist),
        track_number,
        disc_number,
        year,
        cover_base64,
        cover_mime_type,
        remove_cover,
        plain_lyrics: (!values.lyrics.trim().is_empty()).then(|| values.lyrics.clone()),
        netease_id,
        comment: trimmed_or_none(&values.comment),
        genre: trimmed_or_none(&values.genre),
        composer: trimmed_or_none(&values.composer),
        lyricist: trimmed_or_none(&values.lyricist),
        replace_all: true,
        keep: plan.keep,
        ..WriteTagsRequest::default()
    })
}

/// 表单有没有未保存的改动。
///
/// 套用过在线艺术家清单、且那行文字恰好和文件里的一样时也算改动：值的边界不同，保存会改写文件。
pub fn is_dirty(
    values: &EditorFields,
    baseline: &EditorFields,
    cover: &CoverChange,
    online: Option<&ArtistList>,
) -> bool {
    values != baseline
        || *cover != CoverChange::Keep
        || online.is_some_and(|list| list.text == values.artist)
}

/// 数字字段：在线结果有值就用它，否则保持原样。
fn number_or(value: Option<u32>, previous: &str) -> String {
    value.map_or_else(|| previous.to_owned(), |n| n.to_string())
}

/// 文字字段：在线结果有（非空白）值就用修整后的它，否则保持原样。
fn text_or(value: Option<&str>, previous: &str) -> String {
    value
        .and_then(trimmed_or_none)
        .unwrap_or_else(|| previous.to_owned())
}

/// 把一条搜索结果（主词源）填进表单：标题、艺术家、专辑**无条件**换成在线的（连空值也换，
/// 搜索结果是用户点选的整体），其余字段在线有值才换；歌词有（非空白）就换。
///
/// 返回新表单与新的艺术家清单；调用方用后者**替换**记住的清单，即使它是 `None`。
pub fn apply_search_result(
    previous: &EditorFields,
    song: &OnlineSongResult,
    lyrics: Option<&str>,
) -> (EditorFields, Option<ArtistList>) {
    let fields = EditorFields {
        title: song.title.clone(),
        artist: song.artist.clone(),
        album: song.album.clone(),
        year: number_or(song.year, &previous.year),
        track_number: number_or(song.track_number, &previous.track_number),
        disc_number: number_or(song.disc_number, &previous.disc_number),
        genre: text_or(song.genre.as_deref(), &previous.genre),
        composer: text_or(song.composer.as_deref(), &previous.composer),
        lyrics: lyrics
            .filter(|text| !text.trim().is_empty())
            .map_or_else(|| previous.lyrics.clone(), str::to_owned),
        ..previous.clone()
    };
    let list = ArtistList::from_online(song, &song.artist);
    (fields, list)
}

/// 把「在线标签」窗口选中的那一条的标签部分填进表单：在线结果里空着的字段保持表单原样，歌词不动。
///
/// 返回新表单与新的艺术家清单：在线艺术家非空时换成它的清单（可能为 `None`），否则沿用 `current`。
pub fn apply_online_tags(
    previous: &EditorFields,
    song: &OnlineSongResult,
    current: Option<&ArtistList>,
) -> (EditorFields, Option<ArtistList>) {
    let fields = EditorFields {
        title: text_or(Some(&song.title), &previous.title),
        artist: text_or(Some(&song.artist), &previous.artist),
        album: text_or(Some(&song.album), &previous.album),
        year: number_or(song.year, &previous.year),
        track_number: number_or(song.track_number, &previous.track_number),
        disc_number: number_or(song.disc_number, &previous.disc_number),
        genre: text_or(song.genre.as_deref(), &previous.genre),
        composer: text_or(song.composer.as_deref(), &previous.composer),
        ..previous.clone()
    };
    let list = match trimmed_or_none(&song.artist) {
        Some(text) => ArtistList::from_online(song, &text),
        None => current.cloned(),
    };
    (fields, list)
}

/// 「在线标签」窗口打开时的搜索条件：关键词是表单里现在的标题 + 艺术家（标题空着就用文件名推），
/// 打分用表单里现在的标题 / 艺术家 / 专辑与文件的时长。
pub fn online_tags_query(
    path: &str,
    duration_ms: i64,
    fields: &EditorFields,
) -> (String, FileContext) {
    let title = fields.title.trim();
    let artist = fields.artist.trim();
    let keyword = if title.is_empty() {
        keyword_from_file_name(path)
    } else {
        std::iter::once(title.to_owned())
            .chain(split_artists(artist))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let context = FileContext {
        title: trimmed_or_none(title),
        artist: trimmed_or_none(artist),
        album: trimmed_or_none(&fields.album),
        duration_ms,
        file_path: Some(path.to_owned()),
    };
    (keyword, context)
}

#[cfg(test)]
mod tests;
