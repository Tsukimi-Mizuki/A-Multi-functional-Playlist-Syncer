//! 给界面用的曲库视图：扫描进度、对比结果与错误 DTO（camelCase）。

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::matcher::{LocalAvailability, MatchMethod, SongMatch};
use super::track::{LibraryScanProgress, LocalTrack};
use crate::netease::NeteaseSong;

/// `match_playlist` 遇到还没有扫描过的曲库时怎么办。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanMode {
    /// 没有扫描结果就不对比（对应 C# 关闭「启动后自动对比本地曲库」时的行为）。
    Never,
    /// 没有扫描结果（或曲库目录变了）就先扫描。
    IfNeeded,
}

/// 扫描进度事件的载荷。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgressView {
    /// 已处理的文件数。
    pub scanned: usize,
    /// 文件总数。
    pub total: usize,
    /// 当前文件的文件名。
    pub file_name: String,
}

impl From<&LibraryScanProgress> for ScanProgressView {
    fn from(progress: &LibraryScanProgress) -> Self {
        Self {
            scanned: progress.scanned_files,
            total: progress.total_files,
            file_name: file_name_of(&progress.current_file),
        }
    }
}

/// 一次扫描的摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySummaryView {
    /// 被扫描的曲库目录。
    pub directory: String,
    /// 文件总数。
    pub total_files: usize,
    /// 其中未解密的 `.ncm` 数。
    pub ncm_files: usize,
    /// 因权限不足等被跳过的文件夹数。
    pub skipped_directories: usize,
}

/// 一首歌单歌曲的本地匹配结果。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalMatchView {
    /// 歌曲 ID。
    pub song_id: i64,
    /// `available`（本地已存在）/ `ncm`（NCM 待解密）/ `missing`（需下载）。
    pub availability: &'static str,
    /// `none` / `neteaseId`（标签 ID 匹配）/ `metadata`（元数据匹配）/ `fileName`（文件名匹配）。
    pub method: &'static str,
    /// 置信度 0–1。
    pub confidence: f64,
    /// 匹配到的本地文件完整路径。
    pub path: Option<String>,
    /// 文件名。
    pub file_name: Option<String>,
    /// 本地文件的格式（`mp3` / `flac` …）。
    pub format: Option<String>,
    /// 本地文件的标题（悬停提示里帮用户判断匹配对不对）。
    pub local_title: Option<String>,
    /// 本地文件的歌手。
    pub local_artist: Option<String>,
}

/// 对比后的统计（按歌单里的行数算，重复的歌曲各算一行，与 C# 一致）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchStats {
    /// 歌单歌曲数。
    pub total: usize,
    /// 本地已存在。
    pub available: usize,
    /// NCM 待解密。
    pub ncm: usize,
    /// 需下载。
    pub missing: usize,
}

/// `match_playlist` 的返回值。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchOutcomeView {
    /// 歌单 ID。
    pub playlist_id: i64,
    /// 曲库是否已经扫描过；`false` 时 `matches` 为空，界面应显示「尚未对比」。
    pub scanned: bool,
    /// 曲库目录。
    pub library_directory: String,
    /// 本地文件总数（未扫描为 0）。
    pub total_files: usize,
    /// 其中 NCM 数。
    pub ncm_files: usize,
    /// 与歌单歌曲一一对应（按歌单顺序）。
    pub matches: Vec<LocalMatchView>,
    /// 统计。
    pub stats: MatchStats,
}

/// 命令失败：`kind` 供界面区分处理，`message` 是可以直接展示的中文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct LibraryCommandError {
    /// `directoryMissing` / `cancelled` / `playlistNotLoaded` / `failed`。
    pub kind: &'static str,
    /// 中文说明。
    pub message: String,
}

impl LibraryCommandError {
    pub(crate) fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn availability_name(availability: LocalAvailability) -> &'static str {
    match availability {
        LocalAvailability::Available => "available",
        LocalAvailability::EncryptedNcm => "ncm",
        LocalAvailability::Missing => "missing",
    }
}

fn method_name(method: MatchMethod) -> &'static str {
    match method {
        MatchMethod::None => "none",
        MatchMethod::NeteaseId => "neteaseId",
        MatchMethod::Metadata => "metadata",
        MatchMethod::FileName => "fileName",
    }
}

fn view_of(matched: &SongMatch, song: &NeteaseSong, tracks: &[LocalTrack]) -> LocalMatchView {
    let track = matched.local_index.map(|index| &tracks[index]);
    // 标题是从文件名推断出来的（标签里没有标题）时，把「元数据匹配」标成「文件名匹配」。
    // 这只影响显示，评分与结果不变。
    let method = match (matched.method, track) {
        (MatchMethod::Metadata, Some(track)) if track.title_from_file_name => MatchMethod::FileName,
        (method, _) => method,
    };
    LocalMatchView {
        song_id: song.id,
        availability: availability_name(matched.availability),
        method: method_name(method),
        confidence: matched.confidence,
        path: track.map(|t| t.path.to_string_lossy().into_owned()),
        file_name: track.map(|t| file_name_of(&t.path)),
        format: track.and_then(|t| t.format.clone()),
        local_title: track.map(|t| t.title.clone()),
        local_artist: track.and_then(|t| t.artist.clone()),
    }
}

/// 把匹配器的结果整理成界面用的视图。
///
/// 歌单里同一首歌出现多次时，每一行都沿用**第一次出现**那一行的匹配结果（与 C# 的 `TryAdd` 一致）：
/// 本地有这首歌，重复的行也应显示「已存在」，而不是因为一对一约束变成「需下载」。
pub fn build_match_view(
    playlist_id: i64,
    library_directory: &str,
    songs: &[NeteaseSong],
    tracks: &[LocalTrack],
    matches: &[SongMatch],
) -> MatchOutcomeView {
    let mut first_by_id: HashMap<i64, usize> = HashMap::new();
    for (index, song) in songs.iter().enumerate() {
        first_by_id.entry(song.id).or_insert(index);
    }

    let views: Vec<LocalMatchView> = songs
        .iter()
        .map(|song| {
            let first = first_by_id[&song.id];
            view_of(&matches[first], song, tracks)
        })
        .collect();

    let mut stats = MatchStats {
        total: views.len(),
        ..MatchStats::default()
    };
    for view in &views {
        match view.availability {
            "available" => stats.available += 1,
            "ncm" => stats.ncm += 1,
            _ => stats.missing += 1,
        }
    }

    MatchOutcomeView {
        playlist_id,
        scanned: true,
        library_directory: library_directory.to_owned(),
        total_files: tracks.len(),
        ncm_files: tracks.iter().filter(|t| t.is_ncm).count(),
        matches: views,
        stats,
    }
}

/// 还没有扫描过曲库时的结果：不对比，全部留空。
pub fn unscanned_view(
    playlist_id: i64,
    library_directory: &str,
    song_count: usize,
) -> MatchOutcomeView {
    MatchOutcomeView {
        playlist_id,
        scanned: false,
        library_directory: library_directory.to_owned(),
        total_files: 0,
        ncm_files: 0,
        matches: Vec::new(),
        stats: MatchStats {
            total: song_count,
            ..MatchStats::default()
        },
    }
}
