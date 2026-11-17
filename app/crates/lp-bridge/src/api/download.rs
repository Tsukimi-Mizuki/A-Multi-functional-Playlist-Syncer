//! 下载与同步：下载选中、下载缺失、一键同步、取消，以及任务计划、逐首歌进度与结果。
//!
//! 与本地曲库那一组一样，任务的失败要带 `kind` 让界面分支处理（用户取消不该弹错误框），
//! 而 flutter_rust_bridge 的 `Result` 错误位只放字符串，所以命令把结果做成「二选一」的载荷。

use std::sync::{Arc, Mutex, OnceLock};

use lp_core::download::plan::{TransferPlan, plan_transfer};
use lp_core::download::service::{DownloadPhase, DownloadProgress, ItemSource, ProgressSink};
use lp_core::download::task::{
    TransferError, TransferItemView, TransferKind, TransferStarted, TransferSummaryView,
};
use lp_core::library::{ScanMode, ScanProgressSink, ScanProgressView};
use lp_core::settings::AppSettings;

use crate::frb_generated::StreamSink;
use crate::state::{bridge, run};

/// 任务类别（`selected` / `missing` / `sync`）。
fn kind_name(kind: TransferKind) -> &'static str {
    match kind {
        TransferKind::Selected => "selected",
        TransferKind::Missing => "missing",
        TransferKind::Sync => "sync",
    }
}

/// 一首歌最终是怎么处理的。
fn source_name(source: ItemSource) -> &'static str {
    match source {
        ItemSource::Downloaded => "downloaded",
        ItemSource::DecryptedNcm => "decryptedNcm",
        ItemSource::Skipped => "skipped",
        ItemSource::Failed => "failed",
    }
}

/// 当前阶段。
fn phase_name(phase: DownloadPhase) -> &'static str {
    match phase {
        DownloadPhase::Resolving => "resolving",
        DownloadPhase::Downloading => "downloading",
        DownloadPhase::Decrypting => "decrypting",
        DownloadPhase::Tagging => "tagging",
        DownloadPhase::Matching => "matching",
    }
}

/// 「下载缺失」「一键同步」确认框里的计划。
#[derive(Clone)]
pub struct TransferPlanDto {
    /// 去重后的歌曲数。
    pub total: i64,
    /// 本地已存在、会跳过。
    pub available: i64,
    /// 本地是 NCM、要解密。
    pub ncm: i64,
    /// 需要下载的歌曲在歌单里的行号（按歌单顺序）。
    pub missing_rows: Vec<i32>,
}

impl From<TransferPlan> for TransferPlanDto {
    fn from(plan: TransferPlan) -> Self {
        Self {
            total: plan.total as i64,
            available: plan.available as i64,
            ncm: plan.ncm as i64,
            missing_rows: plan
                .missing_rows
                .into_iter()
                .map(|row| i32::try_from(row).unwrap_or(i32::MAX))
                .collect(),
        }
    }
}

/// 计划结果：成功给计划，失败给可展示的错误。
pub struct TransferPlanResultDto {
    /// 成功时的计划。
    pub plan: Option<TransferPlanDto>,
    /// 曲库目录不存在等「按本地为空处理」的警告（此时仍然给计划）。
    pub warning: Option<String>,
    /// 失败时的原因。
    pub error: Option<TransferErrorDto>,
}

/// 任务开始：这次会处理哪些歌曲。
#[derive(Clone)]
pub struct TransferStartedDto {
    /// 任务类别。
    pub kind: String,
    /// 歌曲总数（已去重）。
    pub total: i64,
    /// 将按顺序处理的歌曲 ID。
    pub song_ids: Vec<i64>,
}

impl From<TransferStarted> for TransferStartedDto {
    fn from(started: TransferStarted) -> Self {
        Self {
            kind: kind_name(started.kind).to_owned(),
            total: started.total as i64,
            song_ids: started.song_ids,
        }
    }
}

/// 逐首歌的进度。
#[derive(Clone)]
pub struct TransferProgressDto {
    /// 已处理完的歌曲数。
    pub completed: i64,
    /// 歌曲总数。
    pub total: i64,
    /// 这首歌在本次任务里的序号（从 0 起）。
    pub index: i64,
    /// 歌曲 ID。
    pub song_id: i64,
    /// 歌曲显示名。
    pub song_name: String,
    /// 阶段名（可直接展示的中文）。
    pub stage: String,
    /// 阶段类别。
    pub phase: String,
    /// 补充说明。
    pub message: Option<String>,
    /// 当前阶段百分比（0–100）。
    pub percent: Option<f64>,
    /// 已下载字节数。
    pub bytes_done: Option<i64>,
    /// 总字节数。
    pub bytes_total: Option<i64>,
    /// 这首歌处理完时才有：是否成功。
    pub finish_success: Option<bool>,
    /// 是否因为已存在而跳过。
    pub finish_skipped: Option<bool>,
    /// 来源。
    pub finish_source: Option<String>,
    /// 成品文件路径。
    pub finish_path: Option<String>,
}

impl From<&DownloadProgress> for TransferProgressDto {
    fn from(progress: &DownloadProgress) -> Self {
        Self {
            completed: progress.completed as i64,
            total: progress.total as i64,
            index: progress.index as i64,
            song_id: progress.song_id,
            song_name: progress.song_name.clone(),
            stage: progress.stage.clone(),
            phase: phase_name(progress.phase).to_owned(),
            message: progress.message.clone(),
            percent: progress.percent,
            bytes_done: progress
                .bytes_done
                .map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
            bytes_total: progress
                .bytes_total
                .map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
            finish_success: progress.finish.as_ref().map(|finish| finish.success),
            finish_skipped: progress.finish.as_ref().map(|finish| finish.skipped),
            finish_source: progress
                .finish
                .as_ref()
                .map(|finish| source_name(finish.source).to_owned()),
            finish_path: progress
                .finish
                .as_ref()
                .and_then(|finish| finish.output_path.clone()),
        }
    }
}

/// 一首歌的最终结果。
#[derive(Clone)]
pub struct TransferItemDto {
    /// 歌曲 ID。
    pub song_id: i64,
    /// 歌曲显示名。
    pub song_name: String,
    /// 是否成功（跳过也算成功）。
    pub success: bool,
    /// 是否因为已存在而跳过。
    pub skipped: bool,
    /// 来源。
    pub source: String,
    /// 成品文件路径。
    pub output_path: Option<String>,
    /// 失败原因，或成功时的补充说明。
    pub message: Option<String>,
    /// 实际音质与请求不一致时的说明。
    pub quality_note: Option<String>,
}

impl From<TransferItemView> for TransferItemDto {
    fn from(view: TransferItemView) -> Self {
        Self {
            song_id: view.song_id,
            song_name: view.song_name,
            success: view.success,
            skipped: view.skipped,
            source: source_name(view.source).to_owned(),
            output_path: view.output_path,
            message: view.message,
            quality_note: view.quality_note,
        }
    }
}

/// 任务汇总。
#[derive(Clone)]
pub struct TransferSummaryDto {
    /// 任务类别。
    pub kind: String,
    /// 歌曲总数。
    pub total: i64,
    /// 成功（联网下载 + NCM 解密，不含跳过）。
    pub succeeded: i64,
    /// 联网下载的数量。
    pub downloaded: i64,
    /// 本地 NCM 解密的数量。
    pub decrypted: i64,
    /// 跳过的数量。
    pub skipped: i64,
    /// 失败的数量。
    pub failed: i64,
    /// 是否中途被取消。
    pub cancelled: bool,
    /// 请求了在线补全但没有可用的在线服务。
    pub online_match_skipped: bool,
    /// 结束后是否已重新扫描本地曲库（界面据此刷新对比）。
    pub library_refreshed: bool,
    /// 每首歌的结果。
    pub results: Vec<TransferItemDto>,
}

impl From<TransferSummaryView> for TransferSummaryDto {
    fn from(view: TransferSummaryView) -> Self {
        Self {
            kind: kind_name(view.kind).to_owned(),
            total: view.total as i64,
            succeeded: view.succeeded as i64,
            downloaded: view.downloaded as i64,
            decrypted: view.decrypted as i64,
            skipped: view.skipped as i64,
            failed: view.failed as i64,
            cancelled: view.cancelled,
            online_match_skipped: view.online_match_skipped,
            library_refreshed: view.library_refreshed,
            results: view
                .results
                .into_iter()
                .map(TransferItemDto::from)
                .collect(),
        }
    }
}

/// 任务失败：`kind` 供界面区分处理，`message` 可以直接展示。
#[derive(Clone)]
pub struct TransferErrorDto {
    /// `busy` / `noSongs` / `playlistNotLoaded` / `cancelled` / `failed`。
    pub kind: String,
    /// 中文说明。
    pub message: String,
}

impl From<TransferError> for TransferErrorDto {
    fn from(error: TransferError) -> Self {
        Self {
            kind: error.kind.to_owned(),
            message: error.message,
        }
    }
}

/// 任务结果：成功给汇总，失败给可展示的错误。
pub struct TransferResultDto {
    /// 成功时的汇总。
    pub summary: Option<TransferSummaryDto>,
    /// 失败时的原因。
    pub error: Option<TransferErrorDto>,
}

fn into_result(outcome: Result<TransferSummaryView, TransferError>) -> TransferResultDto {
    match outcome {
        Ok(summary) => TransferResultDto {
            summary: Some(summary.into()),
            error: None,
        },
        Err(error) => TransferResultDto {
            summary: None,
            error: Some(error.into()),
        },
    }
}

/// 当前订阅逐首歌进度的界面（只保留最后一个订阅者）。
static PROGRESS_SINK: OnceLock<Mutex<Option<StreamSink<TransferProgressDto>>>> = OnceLock::new();

/// 当前订阅任务开始的界面（只保留最后一个订阅者）。
static STARTED_SINK: OnceLock<Mutex<Option<StreamSink<TransferStartedDto>>>> = OnceLock::new();

fn publish_progress(progress: &DownloadProgress) {
    let Some(sink) = PROGRESS_SINK.get() else {
        return;
    };
    let sink = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(sink) = sink.as_ref() {
        // Dart 那边的订阅已经关掉时发送会失败，忽略即可。
        let _ = sink.add(TransferProgressDto::from(progress));
    }
}

fn publish_started(started: &TransferStarted) {
    let Some(sink) = STARTED_SINK.get() else {
        return;
    };
    let sink = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(sink) = sink.as_ref() {
        let _ = sink.add(TransferStartedDto::from(started.clone()));
    }
}

/// 每次任务交给 `lp-core` 的进度回调（`lp-core` 内部已经按节流发送）。
fn progress_sink() -> ProgressSink {
    Arc::new(|progress: DownloadProgress| publish_progress(&progress))
}

/// 订阅逐首歌进度（只保留最后一个订阅者）。
pub fn watch_transfer_progress(sink: StreamSink<TransferProgressDto>) -> Result<(), String> {
    let cell = PROGRESS_SINK.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sink);
    Ok(())
}

/// 订阅任务开始（只保留最后一个订阅者）。
pub fn watch_transfer_started(sink: StreamSink<TransferStartedDto>) -> Result<(), String> {
    let cell = STARTED_SINK.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sink);
    Ok(())
}

/// 计算「下载缺失」「一键同步」确认框里的计划。
///
/// 与真正的任务共用 `lp-core` 的去重与判定规则，框里写的数量才会和实际执行的一致。
pub async fn transfer_plan(playlist_id: i64) -> Result<TransferPlanResultDto, String> {
    let bridge = bridge()?;
    let directory = bridge.settings().library_directory;
    let library = Arc::clone(&bridge.library);
    run(async move {
        let Some(songs) = library.playlist_songs(playlist_id) else {
            return TransferPlanResultDto {
                plan: None,
                warning: None,
                error: Some(TransferErrorDto {
                    kind: "playlistNotLoaded".to_owned(),
                    message: "歌单还没有加载完成，请稍后再试。".to_owned(),
                }),
            };
        };
        let ids: Vec<i64> = songs.iter().map(|song| song.id).collect();
        let noop: ScanProgressSink = Arc::new(|_: ScanProgressView| {});
        let (matches, warning) = match library
            .match_playlist(playlist_id, ScanMode::IfNeeded, &directory, noop)
            .await
        {
            Ok(view) => (Some(view.matches), None),
            // 曲库目录不存在时按「本地为空」处理，计划里全部算缺失，并给一句警告。
            Err(error) if error.kind == "directoryMissing" => (None, Some(error.message)),
            Err(error) => {
                return TransferPlanResultDto {
                    plan: None,
                    warning: None,
                    error: Some(TransferErrorDto {
                        kind: error.kind.to_owned(),
                        message: error.message,
                    }),
                };
            }
        };
        let plan = plan_transfer(&ids, matches.as_deref());
        TransferPlanResultDto {
            plan: Some(plan.into()),
            warning,
            error: None,
        }
    })
    .await
}

/// 下载用户勾选的歌曲。
pub async fn download_tracks(
    playlist_id: i64,
    track_ids: Vec<i64>,
) -> Result<TransferResultDto, String> {
    let bridge = bridge()?;
    let settings: AppSettings = bridge.settings();
    let transfers = Arc::clone(&bridge.transfers);
    run(async move {
        let started = |item: TransferStarted| publish_started(&item);
        let channels = channels(&started);
        into_result(
            transfers
                .download_tracks(playlist_id, &track_ids, &settings, &channels)
                .await,
        )
    })
    .await
}

/// 下载本地缺失的歌曲；`track_ids` 为空时按整张歌单算。
pub async fn download_missing(
    playlist_id: i64,
    track_ids: Option<Vec<i64>>,
) -> Result<TransferResultDto, String> {
    let bridge = bridge()?;
    let settings: AppSettings = bridge.settings();
    let transfers = Arc::clone(&bridge.transfers);
    run(async move {
        let started = |item: TransferStarted| publish_started(&item);
        let channels = channels(&started);
        into_result(
            transfers
                .download_missing(playlist_id, track_ids.as_deref(), &settings, &channels)
                .await,
        )
    })
    .await
}

/// 一键同步整个歌单：本地已有跳过、NCM 解密、其余下载。
pub async fn sync_playlist(playlist_id: i64) -> Result<TransferResultDto, String> {
    let bridge = bridge()?;
    let settings: AppSettings = bridge.settings();
    let transfers = Arc::clone(&bridge.transfers);
    run(async move {
        let started = |item: TransferStarted| publish_started(&item);
        let channels = channels(&started);
        into_result(
            transfers
                .sync_playlist(playlist_id, &settings, &channels)
                .await,
        )
    })
    .await
}

/// 组装一次任务要用的出口：开始回调、逐首歌进度、结束后重新扫描曲库的进度。
fn channels<'a>(
    started: &'a (dyn Fn(TransferStarted) + Send + Sync),
) -> lp_core::download::task::TransferChannels<'a> {
    lp_core::download::task::TransferChannels {
        started,
        sink: progress_sink(),
        // 任务结束后会重新扫描曲库，进度复用曲库那条流，界面照常显示扫描进度。
        scan_progress: crate::api::library::progress_sink(),
    }
}

/// 取消正在进行的下载 / 同步；没有任务在跑时返回 `false`。
pub fn download_cancel() -> Result<bool, String> {
    Ok(bridge()?.transfers.cancel())
}

/// 是否有下载 / 同步任务在运行。
pub fn transfers_running() -> Result<bool, String> {
    Ok(bridge()?.transfers.is_running())
}
