//! 标签服务的数据模型与错误类型。
//!
//! 对应 C# `IAudioTagService.cs` 里的 `AudioTagData` / `AudioTagInfo`。

use thiserror::Error;

use crate::error::CoreError;

/// 要写入的标签数据（对应 C# `AudioTagData`）。
///
/// 默认（`replace_all == false`）只更新“有值”的字段：`None` / 空数组表示不动，所以下载管线可以只写
/// 标题、歌手、封面而不碰用户已有的其他标签。`replace_all == true` 是编辑器保存的语义：
/// 编辑器里展示的字段**整体替换**，没填的就清除。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioTagData {
    /// 标题。
    pub title: Option<String>,
    /// 艺术家（多值）。
    pub artists: Vec<String>,
    /// 专辑。
    pub album: Option<String>,
    /// 专辑艺术家。
    pub album_artist: Option<String>,
    /// 音轨号。
    pub track_number: Option<u32>,
    /// 音轨总数（C# 版没有这个字段；`None` 表示保留文件里已有的总数）。
    pub track_total: Option<u32>,
    /// 碟号。
    pub disc_number: Option<u32>,
    /// 碟片总数（语义同 `track_total`）。
    pub disc_total: Option<u32>,
    /// 年份。
    pub year: Option<u32>,
    /// 封面图片原始字节；提供时**替换**文件里的全部图片。
    pub cover_data: Option<Vec<u8>>,
    /// 封面 MIME 类型；为空时按文件头自动判断。
    pub cover_mime_type: Option<String>,
    /// 删除文件里的全部封面（编辑器的“移除封面”）。同时提供了 `cover_data` 时以新封面为准。
    pub remove_cover: bool,
    /// 普通 LRC 歌词。
    pub plain_lyrics: Option<String>,
    /// 逐字（增强 LRC）歌词；非空白时优先于 `plain_lyrics` 写入。
    pub word_lyrics: Option<String>,
    /// 额外写入网易云歌曲 ID（以 `LunaticPrincess:<id>` 标记写进备注），便于下次扫描精确匹配。
    pub netease_id: Option<i64>,
    /// 备注。
    pub comment: Option<String>,
    /// 流派；多个值用 `/`、`;`、`；` 或换行分隔。
    pub genre: Option<String>,
    /// 作曲家；多个值的分隔符同 `genre`。
    pub composer: Option<String>,
    /// 编辑器保存时替换全部字段（包括清空未填写的标签），而不是只更新非空字段。
    pub replace_all: bool,
}

/// 读取到的标签与文件信息（对应 C# `AudioTagInfo`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioTagInfo {
    /// 标题。
    pub title: Option<String>,
    /// 艺术家；多个值用 `/` 连接。
    pub artist: Option<String>,
    /// 专辑。
    pub album: Option<String>,
    /// 专辑艺术家；多个值用 `/` 连接。
    pub album_artist: Option<String>,
    /// 时长（毫秒）。
    pub duration_ms: i64,
    /// 音频码率（kbps）。
    pub bitrate: u32,
    /// 歌词（LRC / 增强 LRC 原文）。
    pub lyrics: Option<String>,
    /// 是否带封面。
    pub has_cover: bool,
    /// 网易云歌曲 ID：本程序写入的 `LunaticPrincess:<id>` 标记，或网易云客户端文件里 `163 key` 解出的 `musicId`。
    pub netease_id: Option<i64>,
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
    /// 流派；多个值用 `/` 连接。
    pub genre: Option<String>,
    /// 作曲家；多个值用 `/` 连接。
    pub composer: Option<String>,
    /// 备注（已去掉 `LunaticPrincess:<id>` 标记）。
    pub comment: Option<String>,
    /// 封面原始字节（优先正面封面，否则第一张图片）。
    pub cover_data: Option<Vec<u8>>,
    /// 封面 MIME 类型（文件里没写时按文件头判断）。
    pub cover_mime_type: Option<String>,
    /// 文件大小（字节）。
    pub file_size: u64,
    /// 文件格式：扩展名大写，如 `MP3`、`FLAC`。
    pub file_format: Option<String>,
}

/// 标签读写错误；文案已经是面向用户的中文。
#[derive(Debug, Error)]
pub enum TagError {
    /// 参数不合法（例如路径为空）。
    #[error("{0}")]
    InvalidArgument(&'static str),
    /// 要写入的文件不存在。
    #[error("找不到要写入标签的音频文件：{0}")]
    NotFound(String),
    /// 文件格式不支持，或已损坏，无法写入。
    #[error("无法写入标签：不支持或已损坏的音频文件「{0}」。")]
    Unsupported(String),
    /// 写入过程中标签库报错（格式不支持该标签、数据过大等）。
    #[error("保存标签失败：{reason}「{file}」。")]
    SaveFailed {
        /// 文件名。
        file: String,
        /// 面向用户的原因。
        reason: String,
    },
    /// 文件读写失败（权限、占用、磁盘已满、路径过长等），文案由 [`CoreError`] 统一翻译。
    #[error(transparent)]
    Io(#[from] CoreError),
}

impl From<std::io::Error> for TagError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(CoreError::Io(error))
    }
}
