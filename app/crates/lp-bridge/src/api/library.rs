//! 本地曲库：扫描设定目录、与已加载的歌单对比、取消任务，以及扫描进度推送。

use std::sync::{Arc, Mutex, OnceLock};

use lp_core::library::{
    LibraryCommandError, LibrarySummaryView, LocalMatchView, MatchOutcomeView, MatchStats,
    ScanMode, ScanProgressSink, ScanProgressView,
};

use crate::frb_generated::StreamSink;
use crate::state::{bridge, run};

/// 扫描进度（`total` 为 0 表示还在统计文件总数）。
#[derive(Clone)]
pub struct ScanProgressDto {
    /// 已处理的文件数。
    pub scanned: i64,
    /// 文件总数。
    pub total: i64,
    /// 当前文件名。
    pub file_name: String,
}

impl From<&ScanProgressView> for ScanProgressDto {
    fn from(progress: &ScanProgressView) -> Self {
        Self {
            scanned: progress.scanned as i64,
            total: progress.total as i64,
            file_name: progress.file_name.clone(),
        }
    }
}

/// 一次扫描的摘要。
#[derive(Clone)]
pub struct LibrarySummaryDto {
    /// 被扫描的目录。
    pub directory: String,
    /// 文件总数。
    pub total_files: i64,
    /// 其中未解密的 `.ncm` 数。
    pub ncm_files: i64,
    /// 因权限不足等被跳过的文件夹数。
    pub skipped_directories: i64,
}

impl From<LibrarySummaryView> for LibrarySummaryDto {
    fn from(view: LibrarySummaryView) -> Self {
        Self {
            directory: view.directory,
            total_files: view.total_files as i64,
            ncm_files: view.ncm_files as i64,
            skipped_directories: view.skipped_directories as i64,
        }
    }
}

/// 一首歌单歌曲的本地匹配结果。
#[derive(Clone)]
pub struct LocalMatchDto {
    /// 歌曲 ID。
    pub song_id: i64,
    /// `available`（本地已存在）/ `ncm`（NCM 待解密）/ `missing`（需下载）。
    pub availability: String,
    /// `none` / `neteaseId` / `metadata` / `fileName`。
    pub method: String,
    /// 置信度 0–1。
    pub confidence: f64,
    /// 匹配到的本地文件路径。
    pub path: Option<String>,
    /// 文件名。
    pub file_name: Option<String>,
    /// 格式（`mp3` / `flac` …）。
    pub format: Option<String>,
    /// 本地文件的标题。
    pub local_title: Option<String>,
    /// 本地文件的歌手。
    pub local_artist: Option<String>,
}

impl From<LocalMatchView> for LocalMatchDto {
    fn from(view: LocalMatchView) -> Self {
        Self {
            song_id: view.song_id,
            availability: view.availability.to_owned(),
            method: view.method.to_owned(),
            confidence: view.confidence,
            path: view.path,
            file_name: view.file_name,
            format: view.format,
            local_title: view.local_title,
            local_artist: view.local_artist,
        }
    }
}

/// 对比统计。
#[derive(Clone)]
pub struct MatchStatsDto {
    /// 歌单歌曲数。
    pub total: i64,
    /// 本地已存在。
    pub available: i64,
    /// NCM 待解密。
    pub ncm: i64,
    /// 需下载。
    pub missing: i64,
}

impl From<MatchStats> for MatchStatsDto {
    fn from(stats: MatchStats) -> Self {
        Self {
            total: stats.total as i64,
            available: stats.available as i64,
            ncm: stats.ncm as i64,
            missing: stats.missing as i64,
        }
    }
}

/// 对比结果。
#[derive(Clone)]
pub struct MatchOutcomeDto {
    /// 歌单 ID。
    pub playlist_id: i64,
    /// 曲库是否已扫描；`false` 时 `matches` 为空。
    pub scanned: bool,
    /// 曲库目录。
    pub library_directory: String,
    /// 本地文件总数。
    pub total_files: i64,
    /// 其中 NCM 数。
    pub ncm_files: i64,
    /// 与歌单歌曲一一对应。
    pub matches: Vec<LocalMatchDto>,
    /// 统计。
    pub stats: MatchStatsDto,
}

impl From<MatchOutcomeView> for MatchOutcomeDto {
    fn from(view: MatchOutcomeView) -> Self {
        Self {
            playlist_id: view.playlist_id,
            scanned: view.scanned,
            library_directory: view.library_directory,
            total_files: view.total_files as i64,
            ncm_files: view.ncm_files as i64,
            matches: view.matches.into_iter().map(LocalMatchDto::from).collect(),
            stats: view.stats.into(),
        }
    }
}

/// 曲库命令失败：`kind` 供界面区分处理（取消、目录缺失……），`message` 可直接展示。
#[derive(Clone)]
pub struct LibraryErrorDto {
    /// `directoryMissing` / `cancelled` / `playlistNotLoaded` / `failed`。
    pub kind: String,
    /// 中文说明。
    pub message: String,
}

impl From<LibraryCommandError> for LibraryErrorDto {
    fn from(error: LibraryCommandError) -> Self {
        Self {
            kind: error.kind.to_owned(),
            message: error.message,
        }
    }
}

/// 扫描结果：成功给摘要，失败给可展示的错误。
///
/// 曲库命令的失败要带 `kind` 让界面分支处理（用户取消不该弹错误框），而 flutter_rust_bridge
/// 的 `Result` 错误位只放字符串，所以这里把结果做成「二选一」的载荷。
pub struct ScanResultDto {
    /// 成功时的摘要。
    pub summary: Option<LibrarySummaryDto>,
    /// 失败时的原因。
    pub error: Option<LibraryErrorDto>,
}

/// 对比结果：成功给结果，失败给可展示的错误（同 [`ScanResultDto`]）。
pub struct MatchResultDto {
    /// 成功时的对比结果。
    pub outcome: Option<MatchOutcomeDto>,
    /// 失败时的原因。
    pub error: Option<LibraryErrorDto>,
}

/// 当前订阅扫描进度的界面（只保留最后一个订阅者）。
static PROGRESS_SINK: OnceLock<Mutex<Option<StreamSink<ScanProgressDto>>>> = OnceLock::new();

fn publish_progress(progress: &ScanProgressView) {
    let Some(sink) = PROGRESS_SINK.get() else {
        return;
    };
    let sink = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(sink) = sink.as_ref() {
        // Dart 那边的订阅已经关掉时发送会失败，忽略即可。
        let _ = sink.add(ScanProgressDto::from(progress));
    }
}

/// 每次扫描交给 `lp-core` 的进度回调（`lp-core` 内部已经按 80 ms 节流）。
fn progress_sink() -> ScanProgressSink {
    Arc::new(|progress: ScanProgressView| publish_progress(&progress))
}

/// 订阅扫描进度（只保留最后一个订阅者）。
pub fn watch_library_progress(sink: StreamSink<ScanProgressDto>) -> Result<(), String> {
    let cell = PROGRESS_SINK.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sink);
    Ok(())
}

/// 上一次扫描的摘要（没有扫描过时为 `None`）。
pub fn library_summary() -> Result<Option<LibrarySummaryDto>, String> {
    Ok(bridge()?.library.summary().map(LibrarySummaryDto::from))
}

/// 是否正在扫描或对比。
pub fn library_is_scanning() -> Result<bool, String> {
    Ok(bridge()?.library.is_scanning())
}

/// 取消正在进行的扫描或对比；没有任务在跑时返回 `false`。
pub fn library_cancel() -> Result<bool, String> {
    Ok(bridge()?.library.cancel())
}

/// 扫描设置里配置的本地曲库目录。
pub async fn library_scan() -> Result<ScanResultDto, String> {
    let bridge = bridge()?;
    let directory = bridge.settings().library_directory;
    let library = Arc::clone(&bridge.library);
    let progress = progress_sink();
    run(async move {
        match library.scan(&directory, progress).await {
            Ok(summary) => ScanResultDto {
                summary: Some(summary.into()),
                error: None,
            },
            Err(error) => ScanResultDto {
                summary: None,
                error: Some(error.into()),
            },
        }
    })
    .await
}

/// 把已加载的歌单与本地曲库对比。
///
/// `rescan` 为 `true` 时丢弃旧的扫描结果重新扫描（界面的「重新扫描并对比」）。
pub async fn library_match(playlist_id: i64, rescan: bool) -> Result<MatchResultDto, String> {
    let bridge = bridge()?;
    let directory = bridge.settings().library_directory;
    let library = Arc::clone(&bridge.library);
    let progress = progress_sink();
    run(async move {
        if rescan {
            library.invalidate();
        }
        match library
            .match_playlist(playlist_id, ScanMode::IfNeeded, &directory, progress)
            .await
        {
            Ok(outcome) => MatchResultDto {
                outcome: Some(outcome.into()),
                error: None,
            },
            Err(error) => MatchResultDto {
                outcome: None,
                error: Some(error.into()),
            },
        }
    })
    .await
}
