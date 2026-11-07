//! 下载 / 同步任务的编排：挑出要处理的歌曲、同一时间只跑一个任务、写运行日志、结束后重新扫描本地曲库。
//!
//! 对应 C# `MainWindowViewModel` 里的下载选中 / 下载缺失 / 一键同步命令（`SyncSelectedAsync`、`SelectMissing`
//! 与完成后的 `TryScanLibraryInternalAsync`）。Tauri 外壳只负责把参数交进来、把进度事件推出去。
//!
//! ## 挑选规则
//!
//! - **下载选中**：只下载用户勾选的歌曲（按勾选顺序，重复 ID 只算一次），不看本地状态——目录里已有同名音频时
//!   由 `skip_existing` 决定跳过。
//! - **下载缺失**：本地曲库里**没有**的歌（`Missing`）；NCM 待解密的不算缺失（用同步解密）。界面的确认框
//!   可以取消勾选部分歌曲，这时只下载传进来的 ID 里**仍然**缺失的（确认之后才变成已存在的不会再下载）。
//! - **一键同步**：整个歌单，每首按本地状态走跳过 / NCM 解密 / 下载三个分支。
//! - 后两者对歌单里重复出现的同一首歌只处理一次（并发处理同一首歌只会得到两份文件）。
//! - 曲库目录不存在时按“本地为空”处理并给出警告（同步 / 下载缺失的语义本来就是把缺的补齐）。

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;

use super::service::{DownloadOptions, DownloadOutcome, DownloadService, ItemSource, ProgressSink};
use crate::library::{
    LibraryCommandError, LibraryManager, PlaylistMatches, ScanMode, ScanProgressSink,
};
use crate::logs::LogHub;
use crate::netease::NeteaseSong;
use crate::settings::AppSettings;
use crate::sync::{SyncOptions, SyncOutcome, SyncRequest, SyncService};
use crate::utilities::CancelToken;

/// 每类日志最多逐条列出的条数（与 C# 一致，避免几百首失败把日志面板刷满）。
const MAX_LOGGED_ITEMS: usize = 15;

/// 任务类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferKind {
    /// 下载选中的歌曲。
    Selected,
    /// 下载本地缺失的歌曲。
    Missing,
    /// 一键同步整个歌单。
    Sync,
}

/// 任务开始时发给界面的计划：哪些歌曲会被处理。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferStarted {
    /// 任务类别。
    pub kind: TransferKind,
    /// 歌曲总数。
    pub total: usize,
    /// 将被处理的歌曲 ID（按处理顺序，已去重）。
    pub song_ids: Vec<i64>,
}

/// 一首歌的最终结果（给界面）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferItemView {
    /// 歌曲 ID。
    pub song_id: i64,
    /// 歌曲显示名。
    pub song_name: String,
    /// 是否成功（跳过也算成功）。
    pub success: bool,
    /// 是否因为已存在而跳过。
    pub skipped: bool,
    /// 来源。
    pub source: ItemSource,
    /// 成品文件路径。
    pub output_path: Option<String>,
    /// 失败原因，或成功时的补充说明。
    pub message: Option<String>,
    /// 实际音质与请求不一致时的说明。
    pub quality_note: Option<String>,
}

/// 任务汇总（给界面）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSummaryView {
    /// 任务类别。
    pub kind: TransferKind,
    /// 歌曲总数。
    pub total: usize,
    /// 成功（联网下载 + NCM 解密，不含跳过）。
    pub succeeded: usize,
    /// 联网下载的数量。
    pub downloaded: usize,
    /// 本地 NCM 解密的数量。
    pub decrypted: usize,
    /// 跳过的数量。
    pub skipped: usize,
    /// 失败的数量。
    pub failed: usize,
    /// 是否中途被取消。
    pub cancelled: bool,
    /// 请求了在线补全但没有可用的在线服务，已跳过。
    pub online_match_skipped: bool,
    /// 结束后是否已重新扫描本地曲库（界面据此刷新对比）。
    pub library_refreshed: bool,
    /// 已处理完的歌曲的结果。
    pub results: Vec<TransferItemView>,
}

/// 任务无法开始 / 无法完成。`kind` 供界面区分处理，`message` 是可以直接展示的中文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct TransferError {
    /// `busy` / `noSongs` / `playlistNotLoaded` / `cancelled` / `failed`。
    pub kind: &'static str,
    /// 中文说明。
    pub message: String,
}

impl TransferError {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// “同一时间只有一个下载 / 同步任务”的登记处。克隆后共享同一个登记处。
#[derive(Debug, Clone, Default)]
pub struct TaskSlot {
    current: Arc<Mutex<Option<CancelToken>>>,
}

/// 正在运行的任务句柄；丢弃时自动释放登记。
#[derive(Debug)]
pub struct TaskGuard {
    /// 本任务的取消令牌。
    pub cancel: CancelToken,
    slot: Arc<Mutex<Option<CancelToken>>>,
}

fn lock(slot: &Mutex<Option<CancelToken>>) -> MutexGuard<'_, Option<CancelToken>> {
    slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl TaskSlot {
    /// 登记一个新任务；已有任务在运行时返回 `busy`。
    pub fn try_begin(&self) -> Result<TaskGuard, TransferError> {
        let mut current = lock(&self.current);
        if current.is_some() {
            return Err(TransferError::new(
                "busy",
                "已有一个下载或同步任务正在进行，请等待它完成或先取消。",
            ));
        }
        let cancel = CancelToken::new();
        *current = Some(cancel.clone());
        Ok(TaskGuard {
            cancel,
            slot: Arc::clone(&self.current),
        })
    }

    /// 请求取消当前任务；返回是否确实有任务在运行。
    pub fn cancel(&self) -> bool {
        match lock(&self.current).as_ref() {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// 是否有任务在运行。
    pub fn is_running(&self) -> bool {
        lock(&self.current).is_some()
    }
}

impl Drop for TaskGuard {
    fn drop(&mut self) {
        *lock(&self.slot) = None;
    }
}

/// 任务向外汇报的出口。
pub struct TransferChannels<'a> {
    /// 任务开始时调用一次，告诉界面会处理哪些歌曲。
    pub started: &'a (dyn Fn(TransferStarted) + Send + Sync),
    /// 逐首歌的进度。
    pub sink: ProgressSink,
    /// 任务结束后重新扫描曲库的进度。
    pub scan_progress: ScanProgressSink,
}

/// 下载 / 同步任务管理器。
pub struct TransferManager {
    downloader: Arc<DownloadService>,
    sync: Arc<crate::sync::SyncService>,
    library: Arc<LibraryManager>,
    logs: Arc<LogHub>,
    slot: TaskSlot,
}

impl TransferManager {
    /// 创建管理器。
    pub fn new(
        downloader: Arc<DownloadService>,
        sync: Arc<SyncService>,
        library: Arc<LibraryManager>,
        logs: Arc<LogHub>,
    ) -> Self {
        Self {
            downloader,
            sync,
            library,
            logs,
            slot: TaskSlot::default(),
        }
    }

    /// 请求取消正在进行的任务；没有任务时返回 `false`。
    pub fn cancel(&self) -> bool {
        self.slot.cancel()
    }

    /// 是否有任务在运行。
    pub fn is_running(&self) -> bool {
        self.slot.is_running()
    }

    /// 下载用户勾选的歌曲。
    pub async fn download_tracks(
        &self,
        playlist_id: i64,
        track_ids: &[i64],
        settings: &AppSettings,
        channels: &TransferChannels<'_>,
    ) -> Result<TransferSummaryView, TransferError> {
        let guard = self.slot.try_begin()?;
        let all = self
            .library
            .playlist_songs(playlist_id)
            .ok_or_else(not_loaded)?;
        let mut seen = HashSet::new();
        let songs: Vec<NeteaseSong> = track_ids
            .iter()
            .filter(|id| seen.insert(**id))
            .filter_map(|id| all.iter().find(|song| song.id == *id).cloned())
            .collect();
        if songs.is_empty() {
            return Err(TransferError::new(
                "noSongs",
                "没有可下载的歌曲，请先勾选歌曲。",
            ));
        }
        self.run_download(TransferKind::Selected, songs, settings, channels, &guard)
            .await
    }

    /// 下载本地曲库里没有的歌曲；`only` 给出时只下载其中仍然缺失的（确认框里取消勾选的不下载）。
    pub async fn download_missing(
        &self,
        playlist_id: i64,
        only: Option<&[i64]>,
        settings: &AppSettings,
        channels: &TransferChannels<'_>,
    ) -> Result<TransferSummaryView, TransferError> {
        let guard = self.slot.try_begin()?;
        if only.is_some_and(<[i64]>::is_empty) {
            return Err(TransferError::new(
                "noSongs",
                "没有选择要下载的歌曲，请至少勾选一首。",
            ));
        }
        let only: Option<HashSet<i64>> = only.map(|ids| ids.iter().copied().collect());
        let matched = self
            .match_for_transfer(playlist_id, settings, channels.scan_progress.clone())
            .await?;
        let rows = unique_rows(&matched.songs);
        let songs: Vec<NeteaseSong> = rows
            .into_iter()
            .filter(|row| {
                only.as_ref()
                    .is_none_or(|ids| ids.contains(&matched.songs[*row].id))
            })
            .filter(|row| {
                // 目录不存在（没有对比结果）时按本地为空处理：全部算缺失。
                matched
                    .availability(*row)
                    .is_none_or(|a| a == crate::library::LocalAvailability::Missing)
            })
            .map(|row| matched.songs[row].clone())
            .collect();
        if songs.is_empty() {
            return Err(TransferError::new(
                "noSongs",
                "没有需要下载的歌曲：本地已存在，或是待解密的 NCM（可以用「一键同步」解密）。",
            ));
        }
        self.run_download(TransferKind::Missing, songs, settings, channels, &guard)
            .await
    }

    /// 一键同步整个歌单。
    pub async fn sync_playlist(
        &self,
        playlist_id: i64,
        settings: &AppSettings,
        channels: &TransferChannels<'_>,
    ) -> Result<TransferSummaryView, TransferError> {
        let guard = self.slot.try_begin()?;
        let matched = self
            .match_for_transfer(playlist_id, settings, channels.scan_progress.clone())
            .await?;
        let requests: Vec<SyncRequest> = unique_rows(&matched.songs)
            .into_iter()
            .map(|row| SyncRequest {
                song: matched.songs[row].clone(),
                local: matched.local_track(row).cloned(),
            })
            .collect();
        if requests.is_empty() {
            return Err(TransferError::new("noSongs", "歌单里没有可同步的歌曲。"));
        }

        let song_ids: Vec<i64> = requests.iter().map(|r| r.song.id).collect();
        let options = SyncOptions {
            download: DownloadOptions::from_settings(settings),
            // 与 C# 一致：设置里勾了“覆盖已存在文件”才重新下载本地已有的普通音频。
            redownload_existing: settings.overwrite_existing,
        };
        self.logs.info(format!(
            "开始同步 {} 首歌曲 → {}（音质：{}）",
            requests.len(),
            settings.download_directory,
            settings.quality.display_name()
        ));
        (channels.started)(TransferStarted {
            kind: TransferKind::Sync,
            total: requests.len(),
            song_ids,
        });

        let outcome = self
            .sync
            .sync(requests, options, Arc::clone(&channels.sink), &guard.cancel)
            .await
            .map_err(|error| {
                let message = error.to_string();
                self.logs.error(format!("同步失败：{message}"));
                TransferError::new("failed", message)
            })?;
        self.log_sync_outcome(&outcome);

        let refreshed = self
            .refresh_library(settings, channels.scan_progress.clone())
            .await;
        Ok(summarize_sync(&outcome, refreshed))
    }

    async fn run_download(
        &self,
        kind: TransferKind,
        songs: Vec<NeteaseSong>,
        settings: &AppSettings,
        channels: &TransferChannels<'_>,
        guard: &TaskGuard,
    ) -> Result<TransferSummaryView, TransferError> {
        let options = DownloadOptions::from_settings(settings);
        self.logs.info(format!(
            "开始下载 {} 首歌曲 → {}（音质：{}）",
            songs.len(),
            settings.download_directory,
            settings.quality.display_name()
        ));
        (channels.started)(TransferStarted {
            kind,
            total: songs.len(),
            song_ids: songs.iter().map(|song| song.id).collect(),
        });

        let outcome = self
            .downloader
            .download(songs, options, Arc::clone(&channels.sink), &guard.cancel)
            .await
            .map_err(|error| {
                let message = error.to_string();
                self.logs.error(format!("下载失败：{message}"));
                TransferError::new("failed", message)
            })?;
        self.log_download_outcome(&outcome);

        let refreshed = self
            .refresh_library(settings, channels.scan_progress.clone())
            .await;
        Ok(summarize_download(kind, &outcome, refreshed))
    }

    /// 对比歌单与曲库；曲库目录不存在时按“本地为空”处理（并警告）。
    async fn match_for_transfer(
        &self,
        playlist_id: i64,
        settings: &AppSettings,
        scan_progress: ScanProgressSink,
    ) -> Result<PlaylistMatches, TransferError> {
        let result = self
            .library
            .match_playlist_raw(
                playlist_id,
                ScanMode::IfNeeded,
                &settings.library_directory,
                scan_progress,
            )
            .await;
        match result {
            Ok(matched) => Ok(matched),
            Err(LibraryCommandError {
                kind: "directoryMissing",
                ..
            }) => {
                self.logs
                    .warning("曲库目录不存在，按本地曲库为空处理（请在「设置」中选择曲库目录）");
                let songs = self
                    .library
                    .playlist_songs(playlist_id)
                    .ok_or_else(not_loaded)?;
                Ok(PlaylistMatches {
                    playlist_id,
                    scanned: false,
                    songs,
                    tracks: Arc::new(Vec::new()),
                    matches: Vec::new(),
                })
            }
            Err(error) => Err(TransferError::new(
                match error.kind {
                    "playlistNotLoaded" => "playlistNotLoaded",
                    "cancelled" => "cancelled",
                    _ => "failed",
                },
                error.message,
            )),
        }
    }

    /// 任务结束后重新扫描曲库，让界面能刷新“本地已存在”状态。失败不报错（与 C# 一致）。
    async fn refresh_library(&self, settings: &AppSettings, progress: ScanProgressSink) -> bool {
        self.library
            .scan(&settings.library_directory, progress)
            .await
            .is_ok()
    }

    fn log_download_outcome(&self, outcome: &DownloadOutcome) {
        let (ok, skipped, failed) = (
            outcome.success_count(),
            outcome.skipped_count(),
            outcome.failed_count(),
        );
        if outcome.cancelled {
            self.logs.warning(format!(
                "下载已取消：已处理 {}/{}，成功 {ok} 首，跳过 {skipped} 首，失败 {failed} 首",
                outcome.results.len(),
                outcome.total
            ));
        } else if failed == 0 {
            self.logs
                .success(format!("下载完成：成功 {ok} 首，跳过 {skipped} 首"));
        } else {
            self.logs.warning(format!(
                "下载完成：成功 {ok} 首，跳过 {skipped} 首，失败 {failed} 首"
            ));
        }
        for result in outcome
            .results
            .iter()
            .filter(|r| r.quality_note.is_some())
            .take(MAX_LOGGED_ITEMS)
        {
            self.logs.warning(format!(
                "{} · {}",
                result.song_name,
                result.quality_note.as_deref().unwrap_or_default()
            ));
        }
        for result in outcome
            .results
            .iter()
            .filter(|r| r.success && !r.skipped && r.error_message.is_some())
            .take(MAX_LOGGED_ITEMS)
        {
            self.logs.info(format!(
                "{} · {}",
                result.song_name,
                result.error_message.as_deref().unwrap_or_default()
            ));
        }
        for result in outcome
            .results
            .iter()
            .filter(|r| !r.success)
            .take(MAX_LOGGED_ITEMS)
        {
            self.logs.error(format!(
                "下载失败：{} · {}",
                result.song_name,
                result.error_message.as_deref().unwrap_or("未知错误")
            ));
        }
        self.log_online_skip(outcome.online_match_skipped);
    }

    fn log_sync_outcome(&self, outcome: &SyncOutcome) {
        if outcome.cancelled {
            self.logs.warning(format!(
                "同步已取消：已处理 {}/{}，成功 {} 首，失败 {} 首",
                outcome.results.len(),
                outcome.total,
                outcome.success_count(),
                outcome.failed_count()
            ));
        } else {
            let message = format!(
                "同步完成：成功 {} 首（下载 {}、NCM 解密 {}、跳过 {}），失败 {} 首",
                outcome.success_count(),
                outcome.downloaded_count(),
                outcome.decrypted_count(),
                outcome.skipped_count(),
                outcome.failed_count()
            );
            if outcome.failed_count() == 0 {
                self.logs.success(message);
            } else {
                self.logs.warning(message);
            }
        }
        for result in outcome
            .results
            .iter()
            .filter(|r| r.quality_note.is_some())
            .take(MAX_LOGGED_ITEMS)
        {
            self.logs.warning(format!(
                "{} · {}",
                result.song_name,
                result.quality_note.as_deref().unwrap_or_default()
            ));
        }
        // 成功的曲目也把在线匹配结果（QQ 逐字歌词等）写进日志，方便确认歌词来源。
        for result in outcome
            .results
            .iter()
            .filter(|r| r.success && r.source != ItemSource::Skipped && r.message.is_some())
            .take(MAX_LOGGED_ITEMS)
        {
            self.logs.info(format!(
                "{} · {}",
                result.song_name,
                result.message.as_deref().unwrap_or_default()
            ));
        }
        for result in outcome
            .results
            .iter()
            .filter(|r| !r.success)
            .take(MAX_LOGGED_ITEMS)
        {
            self.logs.error(format!(
                "同步失败：{} · {}",
                result.song_name,
                result.message.as_deref().unwrap_or("未知错误")
            ));
        }
        self.log_online_skip(outcome.online_match_skipped);
    }

    fn log_online_skip(&self, skipped: bool) {
        if skipped {
            self.logs
                .info("在线补全标签、封面与歌词（QQ 音乐 / 网易云）将在后续版本提供，本次已跳过。");
        }
    }
}

fn not_loaded() -> TransferError {
    TransferError::new("playlistNotLoaded", "歌单还没有加载完成，请稍后再试。")
}

/// 歌单里每个不同歌曲 ID 第一次出现的行号（保持歌单顺序）。
fn unique_rows(songs: &[NeteaseSong]) -> Vec<usize> {
    let mut seen = HashSet::new();
    songs
        .iter()
        .enumerate()
        .filter(|(_, song)| seen.insert(song.id))
        .map(|(row, _)| row)
        .collect()
}

fn summarize_download(
    kind: TransferKind,
    outcome: &DownloadOutcome,
    library_refreshed: bool,
) -> TransferSummaryView {
    let results: Vec<TransferItemView> = outcome
        .results
        .iter()
        .map(|result| {
            let finish = result.to_finish();
            TransferItemView {
                song_id: result.song_id,
                song_name: result.song_name.clone(),
                success: finish.success,
                skipped: finish.skipped,
                source: finish.source,
                output_path: finish.output_path,
                message: finish.message,
                quality_note: finish.quality_note,
            }
        })
        .collect();
    summary(
        kind,
        outcome.total,
        results,
        outcome.cancelled,
        outcome.online_match_skipped,
        library_refreshed,
    )
}

fn summarize_sync(outcome: &SyncOutcome, library_refreshed: bool) -> TransferSummaryView {
    let results: Vec<TransferItemView> = outcome
        .results
        .iter()
        .map(|result| {
            let finish = result.to_finish();
            TransferItemView {
                song_id: result.song_id,
                song_name: result.song_name.clone(),
                success: finish.success,
                skipped: finish.skipped,
                source: finish.source,
                output_path: finish.output_path,
                message: finish.message,
                quality_note: finish.quality_note,
            }
        })
        .collect();
    summary(
        TransferKind::Sync,
        outcome.total,
        results,
        outcome.cancelled,
        outcome.online_match_skipped,
        library_refreshed,
    )
}

fn summary(
    kind: TransferKind,
    total: usize,
    results: Vec<TransferItemView>,
    cancelled: bool,
    online_match_skipped: bool,
    library_refreshed: bool,
) -> TransferSummaryView {
    let count = |source: ItemSource| results.iter().filter(|r| r.source == source).count();
    let downloaded = count(ItemSource::Downloaded);
    let decrypted = count(ItemSource::DecryptedNcm);
    TransferSummaryView {
        kind,
        total,
        succeeded: downloaded + decrypted,
        downloaded,
        decrypted,
        skipped: count(ItemSource::Skipped),
        failed: count(ItemSource::Failed),
        cancelled,
        online_match_skipped,
        library_refreshed,
        results,
    }
}

#[cfg(test)]
mod tests;
