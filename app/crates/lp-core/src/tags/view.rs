//! 给界面用的标签视图与写入请求：封面转成 data URL（限制大小）、错误转成 `{ kind, message }`。
//!
//! Tauri 外壳只做命令转发，这里的转换放在 lp-core 里才能单测。

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

use super::model::{AudioTagData, AudioTagInfo, TagError};
use super::text::{detect_image_mime, image_dimensions};

/// 封面预览的大小上限：超过就不内联 data URL（只告诉界面“有封面，太大不预览”），避免一次 IPC 传几十 MB。
pub const MAX_COVER_PREVIEW_BYTES: usize = 8 * 1024 * 1024;

/// 前端查看标签用的数据（camelCase）。封面已编码成 data URL，网易云 ID 用字符串避免 JS 精度丢失。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagView {
    /// 文件完整路径。
    pub path: String,
    /// 文件名。
    pub file_name: String,
    /// 标题。
    pub title: Option<String>,
    /// 艺术家（多个用 `/` 连接）。
    pub artist: Option<String>,
    /// 专辑。
    pub album: Option<String>,
    /// 专辑艺术家。
    pub album_artist: Option<String>,
    /// 年份。
    pub year: Option<u32>,
    /// 音轨号。
    pub track_number: Option<u32>,
    /// 音轨总数。
    pub track_total: Option<u32>,
    /// 碟号。
    pub disc_number: Option<u32>,
    /// 碟片总数。
    pub disc_total: Option<u32>,
    /// 流派。
    pub genre: Option<String>,
    /// 作曲家。
    pub composer: Option<String>,
    /// 作词家。
    pub lyricist: Option<String>,
    /// 备注（已去掉网易云 ID 标记）。
    pub comment: Option<String>,
    /// 歌词原文。
    pub lyrics: Option<String>,
    /// 网易云歌曲 ID（字符串）。
    pub netease_id: Option<String>,
    /// 是否有封面。
    pub has_cover: bool,
    /// 封面的 data URL；没有封面或超过大小上限时为 `None`。
    pub cover_data_url: Option<String>,
    /// 封面字节数。
    pub cover_size: Option<usize>,
    /// 有封面但太大没有内联。
    pub cover_too_large: bool,
    /// 封面 MIME。
    pub cover_mime_type: Option<String>,
    /// 封面宽度（像素）；认不出图片尺寸时为 `None`。
    pub cover_width: Option<u32>,
    /// 封面高度（像素）。
    pub cover_height: Option<u32>,
    /// 封面的图片类型（中文名，如「正面封面」）。
    pub cover_picture_type: Option<String>,
    /// 时长（毫秒）。
    pub duration_ms: i64,
    /// 码率（kbps）。
    pub bitrate: u32,
    /// 文件大小（字节）。
    pub file_size: u64,
    /// 文件格式（大写扩展名）。
    pub file_format: Option<String>,
}

impl TagView {
    /// 由读取结果构造视图。
    pub fn new(path: &Path, info: AudioTagInfo) -> Self {
        let cover_size = info.cover_data.as_ref().map(Vec::len);
        let cover_dimensions = info.cover_data.as_deref().and_then(image_dimensions);
        let cover_too_large = cover_size.is_some_and(|size| size > MAX_COVER_PREVIEW_BYTES);
        let cover_data_url = match (&info.cover_data, cover_too_large) {
            (Some(data), false) if !data.is_empty() => {
                let mime = info
                    .cover_mime_type
                    .as_deref()
                    .filter(|m| m.starts_with("image/"))
                    .unwrap_or_else(|| detect_image_mime(data));
                Some(format!("data:{mime};base64,{}", STANDARD.encode(data)))
            }
            _ => None,
        };
        Self {
            path: path.to_string_lossy().into_owned(),
            file_name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            title: info.title,
            artist: info.artist,
            album: info.album,
            album_artist: info.album_artist,
            year: info.year,
            track_number: info.track_number,
            track_total: info.track_total,
            disc_number: info.disc_number,
            disc_total: info.disc_total,
            genre: info.genre,
            composer: info.composer,
            lyricist: info.lyricist,
            comment: info.comment,
            lyrics: info.lyrics,
            netease_id: info.netease_id.map(|id| id.to_string()),
            has_cover: info.has_cover,
            cover_data_url,
            cover_size,
            cover_too_large,
            cover_mime_type: info.cover_mime_type,
            cover_width: cover_dimensions.map(|(width, _)| width),
            cover_height: cover_dimensions.map(|(_, height)| height),
            cover_picture_type: info.cover_picture_type,
            duration_ms: info.duration_ms,
            bitrate: info.bitrate,
            file_size: info.file_size,
            file_format: info.file_format,
        }
    }
}

/// 前端写标签的请求（camelCase）；封面用 Base64 传。字段含义同 [`AudioTagData`]。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WriteTagsRequest {
    pub title: Option<String>,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_number: Option<u32>,
    pub track_total: Option<u32>,
    pub disc_number: Option<u32>,
    pub disc_total: Option<u32>,
    pub year: Option<u32>,
    /// 封面图片字节的 Base64（不带 `data:` 前缀）。
    pub cover_base64: Option<String>,
    pub cover_mime_type: Option<String>,
    /// 删除文件里的全部封面；同时提供了 `coverBase64` 时以新封面为准。
    pub remove_cover: bool,
    pub plain_lyrics: Option<String>,
    pub word_lyrics: Option<String>,
    pub netease_id: Option<i64>,
    pub comment: Option<String>,
    pub genre: Option<String>,
    pub composer: Option<String>,
    pub lyricist: Option<String>,
    pub replace_all: bool,
}

impl WriteTagsRequest {
    /// 转成服务层的写入数据；封面 Base64 不合法时报错。
    pub fn into_data(self) -> Result<AudioTagData, TagError> {
        let cover_data = match self.cover_base64.as_deref().map(str::trim) {
            Some(text) if !text.is_empty() => Some(
                STANDARD
                    .decode(text)
                    .map_err(|_| TagError::InvalidArgument("封面数据不是有效的 Base64。"))?,
            ),
            _ => None,
        };
        Ok(AudioTagData {
            title: self.title,
            artists: self.artists,
            album: self.album,
            album_artist: self.album_artist,
            track_number: self.track_number,
            track_total: self.track_total,
            disc_number: self.disc_number,
            disc_total: self.disc_total,
            year: self.year,
            cover_data,
            cover_mime_type: self.cover_mime_type,
            remove_cover: self.remove_cover,
            plain_lyrics: self.plain_lyrics,
            word_lyrics: self.word_lyrics,
            netease_id: self.netease_id,
            comment: self.comment,
            genre: self.genre,
            composer: self.composer,
            lyricist: self.lyricist,
            clear: Vec::new(),
            replace_all: self.replace_all,
        })
    }
}

/// 标签命令失败时前端收到的 `{ kind, message }`；`message` 已是中文说明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCommandError {
    /// 失败类别：`invalid`（参数）、`notFound`、`unsupported`、`failed`。
    pub kind: &'static str,
    /// 中文说明。
    pub message: String,
}

impl From<TagError> for TagCommandError {
    fn from(error: TagError) -> Self {
        let kind = match &error {
            TagError::InvalidArgument(_) => "invalid",
            TagError::NotFound(_) => "notFound",
            TagError::Unsupported(_) => "unsupported",
            TagError::SaveFailed { .. } | TagError::Io(_) => "failed",
        };
        Self {
            kind,
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests;
