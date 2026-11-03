//! 曲库的数据模型：本地曲目、扫描进度与错误。对应 C# `ILocalLibraryService.cs`。

use std::path::PathBuf;

use thiserror::Error;

use crate::ncm::NcmMetadata;

/// 扫描到的一个本地音频文件（对应 C# `LocalTrack`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTrack {
    /// 完整路径。
    pub path: PathBuf,
    /// 标题：标签里的标题，没有就取文件名里推断的，都没有为「未知」。
    pub title: String,
    /// 歌手；多个用 `/` 连接。
    pub artist: Option<String>,
    /// 专辑。
    pub album: Option<String>,
    /// 时长（毫秒），未知为 0。
    pub duration_ms: i64,
    /// 格式：普通音频是扩展名（小写），NCM 优先取内嵌元数据里的 `mp3` / `flac`。
    pub format: Option<String>,
    /// 码率（普通音频为 kbps，NCM 为元数据里的 bps，与 C# 一致），未知为 `None`。
    pub bitrate: Option<i32>,
    /// 是否是未解密的 `.ncm`。
    pub is_ncm: bool,
    /// NCM 文件头里的元数据；读不出来为 `None`。
    pub ncm_metadata: Option<NcmMetadata>,
    /// 网易云歌曲 ID（本程序写入的标记、客户端 `163 key` 或 NCM 元数据里的 `musicId`）。
    pub netease_id: Option<i64>,
    /// 是否带封面。NCM 的封面在文件头之后，扫描阶段不解密，所以恒为 `false`。
    pub has_cover: bool,
    /// 标题是从文件名推断的（标签里没有标题）。只用于界面把这类匹配标成「文件名匹配」，不参与评分。
    pub title_from_file_name: bool,
}

/// 扫描进度（对应 C# `LibraryScanProgress`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryScanProgress {
    /// 已处理的文件数。
    pub scanned_files: usize,
    /// 要处理的文件总数。
    pub total_files: usize,
    /// 刚处理完（或即将开始）的文件。
    pub current_file: PathBuf,
}

/// 一次扫描的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanOutcome {
    /// 全部本地曲目，按完整路径不区分大小写排序（顺序稳定，与是否并行读标签无关）。
    pub tracks: Vec<LocalTrack>,
    /// 因权限不足或扫描时被移动 / 删除而跳过的文件夹数。
    pub skipped_directories: usize,
}

impl ScanOutcome {
    /// 其中未解密的 `.ncm` 文件数。
    pub fn ncm_count(&self) -> usize {
        self.tracks.iter().filter(|track| track.is_ncm).count()
    }
}

/// 扫描失败。单个文件 / 目录的问题只会被跳过，不会走到这里。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum LibraryError {
    /// 调用方取消了扫描。
    #[error("扫描本地曲库已取消")]
    Cancelled,
}
