//! 批量下载：取播放地址 → 流式下载（带进度）→ NCM 魔数识别并解密 → 拉取歌词 / 封面 →
//! 通过标签服务写入标签 → 按文件名模板原子落盘。
//!
//! 对应 C# `DownloadService` / `IDownloadService`。单曲失败不会中断整批；支持并发上限与取消。
//!
//! ## 与 C# 的差异
//!
//! - **取消时保留已完成的结果**：C# 取消会抛 `OperationCanceledException`，整批结果丢失；这里返回
//!   [`DownloadOutcome`]（`cancelled == true`），其中是取消之前已经处理完的歌曲，在途歌曲的临时文件被清理。
//! - **在线补全是挂钩**：真正的在线匹配属于模块 10，沿用模块 7 的 [`OnlineEnricher`]；没有接入实现时
//!   跳过，并在 [`DownloadOutcome::online_match_skipped`] 里让界面提示一次（而不是在每首歌的结果里重复）。
//! - 下载下来的内容长度与响应头声明的不符时按“下载中断”失败（C# 依赖 `HttpClient` 自己发现）。
//! - 冲突时的唯一文件名先 `create_new` 占位再改名，并发下载同名歌曲不会互相覆盖。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Serialize;

use super::fetch::{FetchError, MAX_COVER_BYTES, MediaFetcher};
use super::media::{
    OutputPlan, TempFiles, build_base_name, create_tag_data, find_existing_audio_file,
    has_ncm_magic, null_if_empty, parse_year, resolve_container_extension, resolve_extension,
    unique_token, write_tags_atomically,
};
use super::ncm_import::{EnrichError, OnlineEnricher, OnlineMatchOutcome};
use super::pool::{Slot, Worker, run_pool};
use crate::lyrics::{RawLyrics, parser};
use crate::models::{AudioQuality, LyricsWriteMode};
use crate::ncm::{NcmDecoder, NcmError};
use crate::netease::http::BoxFuture;
use crate::netease::{NeteaseClient, NeteaseError, NeteaseSong, SongPlaybackInfo};
use crate::settings::AppSettings;
use crate::tags::AudioTagService;
use crate::utilities::{CancelToken, Cancelled, cancellable, error_text};

/// 一首歌的下载选项（对应 C# `DownloadOptions`）。
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadOptions {
    /// 目标目录（不存在会创建）。
    pub target_directory: PathBuf,
    /// 请求的音质。
    pub quality: AudioQuality,
    /// 歌词写入方式。
    pub lyrics_mode: LyricsWriteMode,
    /// 是否写入封面。
    pub write_cover: bool,
    /// 是否覆盖同名文件。
    pub overwrite_existing: bool,
    /// 目录里已有同名音频时是否直接跳过（不发起任何网络请求）。
    pub skip_existing: bool,
    /// 文件名模板。
    pub file_name_template: String,
    /// 并发上限（至少 1）。
    pub max_concurrency: usize,
    /// 写完基础标签后是否再在线补全标签 / 封面 / 歌词。
    pub auto_match_online: bool,
}

impl DownloadOptions {
    /// 全部取 C# 的默认值。
    pub fn new(target_directory: impl Into<PathBuf>) -> Self {
        Self {
            target_directory: target_directory.into(),
            quality: AudioQuality::ExHigh,
            lyrics_mode: LyricsWriteMode::Line,
            write_cover: true,
            overwrite_existing: false,
            skip_existing: true,
            file_name_template: "{artist} - {title}".to_owned(),
            max_concurrency: 3,
            auto_match_online: true,
        }
    }

    /// 从设置构造（对应 `MainWindowViewModel.BuildDownloadOptions`）。
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            target_directory: PathBuf::from(&settings.download_directory),
            quality: settings.quality,
            lyrics_mode: settings.lyrics_mode,
            write_cover: settings.write_cover,
            overwrite_existing: settings.overwrite_existing,
            skip_existing: settings.skip_existing,
            file_name_template: settings.file_name_template.clone(),
            max_concurrency: usize::try_from(settings.max_concurrency)
                .unwrap_or(1)
                .clamp(1, 8),
            auto_match_online: true,
        }
    }
}

/// 一首歌当前处于哪个阶段（界面据此画进度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadPhase {
    /// 获取播放地址 / 检查是否已存在 / 获取歌曲信息。
    Resolving,
    /// 正在下载音频。
    Downloading,
    /// 正在解密 NCM。
    Decrypting,
    /// 获取封面、歌词，写入标签。
    Tagging,
    /// 在线补全标签、封面与歌词。
    Matching,
}

/// 一首歌最终是怎么处理的（同步结果里的来源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemSource {
    /// 从网络下载。
    Downloaded,
    /// 本地的 NCM 被解密。
    DecryptedNcm,
    /// 已存在，跳过。
    Skipped,
    /// 失败。
    Failed,
}

/// 一首歌处理完的结果，随最后一条进度事件一起交给界面。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemFinish {
    /// 是否成功（跳过也算成功）。
    pub success: bool,
    /// 是否因为已存在而跳过。
    pub skipped: bool,
    /// 来源。
    pub source: ItemSource,
    /// 成品文件的完整路径。
    pub output_path: Option<String>,
    /// 失败原因，或成功时的补充说明（标签写入失败、在线补全结果等）。
    pub message: Option<String>,
    /// 实际音质与请求不一致时的说明：`实际音质：…（… 不可用，已自动降级）`。
    pub quality_note: Option<String>,
}

/// 下载 / 同步进度（对应 C# `DownloadProgress` / `SyncProgress`）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    /// 已处理完的歌曲数。
    pub completed: usize,
    /// 歌曲总数。
    pub total: usize,
    /// 这首歌在本次任务里的序号（从 0 起）。
    pub index: usize,
    /// 歌曲 ID。
    pub song_id: i64,
    /// 歌曲显示名。
    pub song_name: String,
    /// 阶段名（与 C# 相同的中文，例如 `下载中`、`写入标签`）。
    pub stage: String,
    /// 阶段类别。
    pub phase: DownloadPhase,
    /// 补充说明（实际音质提示、失败原因等）。
    pub message: Option<String>,
    /// 当前阶段的百分比（0–100），未知为 `None`。
    pub percent: Option<f64>,
    /// 已下载字节数。
    pub bytes_done: Option<u64>,
    /// 总字节数。
    pub bytes_total: Option<u64>,
    /// 这首歌处理完时才有：最终结果。
    pub finish: Option<ItemFinish>,
}

/// 同步用的进度类型（与下载共用）。
pub type SyncProgress = DownloadProgress;

/// 进度的接收方；会从工作任务里调用，不要在里面做耗时操作。
pub type ProgressSink = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

/// 一首歌在任务里的上下文：序号、总数、已完成计数与进度出口。
#[derive(Clone)]
pub struct ItemContext {
    index: usize,
    total: usize,
    completed: Arc<AtomicUsize>,
    sink: ProgressSink,
}

impl ItemContext {
    /// 创建上下文；同一任务里的所有歌曲共享 `completed` 计数。
    pub fn new(
        index: usize,
        total: usize,
        completed: Arc<AtomicUsize>,
        sink: ProgressSink,
    ) -> Self {
        Self {
            index,
            total,
            completed,
            sink,
        }
    }

    /// 报告一次阶段进度。
    pub fn report(
        &self,
        song: &NeteaseSong,
        phase: DownloadPhase,
        stage: &str,
        message: Option<String>,
        percent: Option<f64>,
    ) {
        self.emit(song, phase, stage, message, percent, None, None, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &self,
        song: &NeteaseSong,
        phase: DownloadPhase,
        stage: &str,
        message: Option<String>,
        percent: Option<f64>,
        bytes: Option<(u64, Option<u64>)>,
        finish: Option<ItemFinish>,
        completed: Option<usize>,
    ) {
        (self.sink)(DownloadProgress {
            completed: completed.unwrap_or_else(|| self.completed.load(Ordering::SeqCst)),
            total: self.total,
            index: self.index,
            song_id: song.id,
            song_name: display_name(song),
            stage: stage.to_owned(),
            phase,
            message,
            percent,
            bytes_done: bytes.map(|(done, _)| done),
            bytes_total: bytes.and_then(|(_, total)| total),
            finish,
        });
    }

    fn report_bytes(&self, song: &NeteaseSong, done: u64, total: Option<u64>) {
        let percent = total.map(|total| (done as f64 * 100.0 / total as f64).clamp(0.0, 100.0));
        self.emit(
            song,
            DownloadPhase::Downloading,
            "下载中",
            Some("正在下载音频…".to_owned()),
            percent,
            Some((done, total)),
            None,
            None,
        );
    }

    /// 这首歌处理完了：计数加一，并发出带最终结果的进度。
    pub fn finish(&self, song: &NeteaseSong, stage: &str, finish: ItemFinish) {
        let completed = self.completed.fetch_add(1, Ordering::SeqCst) + 1;
        let message = finish.message.clone();
        let percent = finish.success.then_some(100.0);
        self.emit(
            song,
            DownloadPhase::Tagging,
            stage,
            message,
            percent,
            None,
            Some(finish),
            Some(completed),
        );
    }
}

/// 单曲下载结果（对应 C# `DownloadItemResult`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadItemResult {
    /// 歌曲 ID。
    pub song_id: i64,
    /// 歌曲显示名。
    pub song_name: String,
    /// 是否成功（跳过也算成功）。
    pub success: bool,
    /// 成品文件路径（失败为 `None`）。
    pub output_path: Option<PathBuf>,
    /// 失败原因；成功时是补充说明（标签写入失败、在线补全结果），与 C# 的 `ErrorMessage` 一致。
    pub error_message: Option<String>,
    /// 下载下来的其实是 NCM，已解密。
    pub decrypted_from_ncm: bool,
    /// 因为已存在而跳过。
    pub skipped: bool,
    /// 服务端降档时的说明（实际音质 / 请求音质）。
    pub quality_note: Option<String>,
    /// 请求了在线补全但没有可用的在线服务，已跳过。
    pub online_match_skipped: bool,
}

impl DownloadItemResult {
    fn failure(
        song: &NeteaseSong,
        message: impl Into<String>,
        quality_note: Option<String>,
    ) -> Self {
        Self {
            song_id: song.id,
            song_name: display_name(song),
            success: false,
            output_path: None,
            error_message: Some(message.into()),
            decrypted_from_ncm: false,
            skipped: false,
            quality_note,
            online_match_skipped: false,
        }
    }

    /// 给进度事件用的最终结果。
    pub fn to_finish(&self) -> ItemFinish {
        ItemFinish {
            success: self.success,
            skipped: self.skipped,
            source: if self.skipped {
                ItemSource::Skipped
            } else if !self.success {
                ItemSource::Failed
            } else {
                ItemSource::Downloaded
            },
            output_path: self
                .output_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            message: self.error_message.clone(),
            quality_note: self.quality_note.clone(),
        }
    }

    /// 最终阶段名（C# `StageOf`）。
    pub fn stage(&self) -> &'static str {
        if self.skipped {
            "已存在，跳过"
        } else if !self.success {
            "失败"
        } else if self.decrypted_from_ncm {
            "NCM 解密完成"
        } else {
            "完成"
        }
    }
}

/// 一批下载的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome {
    /// 已处理完的歌曲的结果，顺序与输入一致；取消时只包含取消之前完成的部分。
    pub results: Vec<DownloadItemResult>,
    /// 歌曲总数。
    pub total: usize,
    /// 是否中途被取消。
    pub cancelled: bool,
    /// 请求了在线补全但没有可用的在线服务，已跳过。
    pub online_match_skipped: bool,
}

impl DownloadOutcome {
    /// 成功下载（不含跳过）的数量。
    pub fn success_count(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.success && !r.skipped)
            .count()
    }

    /// 跳过的数量。
    pub fn skipped_count(&self) -> usize {
        self.results.iter().filter(|r| r.skipped).count()
    }

    /// 失败的数量。
    pub fn failed_count(&self) -> usize {
        self.results
            .iter()
            .filter(|r| !r.success && !r.skipped)
            .count()
    }
}

/// 整批无法开始。单曲失败不走这里。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DownloadError {
    /// 下载目录为空。
    #[error("下载目录不能为空。")]
    EmptyDirectory,
    /// 下载目录建不出来。
    #[error("下载目录不可用：{0}")]
    Directory(String),
}

/// 下载用到的网易云接口（`NeteaseClient` 实现它；测试用手写假实现）。
pub trait DownloadApi: Send + Sync {
    /// 取播放地址（含逐级降档）。
    fn playback_info<'a>(
        &'a self,
        song_id: i64,
        quality: AudioQuality,
    ) -> BoxFuture<'a, Result<SongPlaybackInfo, NeteaseError>>;

    /// 取歌词。
    fn lyrics<'a>(&'a self, song_id: i64) -> BoxFuture<'a, Result<RawLyrics, NeteaseError>>;

    /// 取一首歌的最新详情；查不到返回 `None`。
    fn song_detail<'a>(
        &'a self,
        song_id: i64,
    ) -> BoxFuture<'a, Result<Option<NeteaseSong>, NeteaseError>>;
}

impl DownloadApi for NeteaseClient {
    fn playback_info<'a>(
        &'a self,
        song_id: i64,
        quality: AudioQuality,
    ) -> BoxFuture<'a, Result<SongPlaybackInfo, NeteaseError>> {
        Box::pin(self.get_song_playback_info(song_id, quality))
    }

    fn lyrics<'a>(&'a self, song_id: i64) -> BoxFuture<'a, Result<RawLyrics, NeteaseError>> {
        Box::pin(self.get_lyrics(song_id))
    }

    fn song_detail<'a>(
        &'a self,
        song_id: i64,
    ) -> BoxFuture<'a, Result<Option<NeteaseSong>, NeteaseError>> {
        Box::pin(async move {
            let songs = self.get_song_details(&[song_id]).await?;
            Ok(songs
                .iter()
                .find(|song| song.id == song_id)
                .or_else(|| songs.first())
                .cloned())
        })
    }
}

/// 下载单曲的能力（同步服务通过它走“缺失 → 下载”分支，测试里可以换成假实现）。
pub trait Downloader: Send + Sync {
    /// 下载一首歌。取消时返回 `Err(Cancelled)`；其他失败都在结果里（`success == false`）。
    ///
    /// 不会发出“处理完”的最终进度（`ItemContext::finish`），那由调用方负责。
    fn download_one<'a>(
        &'a self,
        song: &'a NeteaseSong,
        options: &'a DownloadOptions,
        ctx: &'a ItemContext,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<DownloadItemResult, Cancelled>>;
}

/// 批量下载服务。
pub struct DownloadService {
    api: Arc<dyn DownloadApi>,
    tags: Arc<dyn AudioTagService>,
    decoder: Arc<dyn NcmDecoder>,
    fetcher: MediaFetcher,
    enricher: Option<Arc<dyn OnlineEnricher>>,
}

/// 单曲处理过程中的中断：取消，或带着面向用户的原因失败。
enum Abort {
    Cancelled,
    Failed(String),
}

impl From<Cancelled> for Abort {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

impl DownloadService {
    /// 创建服务；`enricher` 为 `None` 表示没有在线补全能力。
    pub fn new(
        api: Arc<dyn DownloadApi>,
        tags: Arc<dyn AudioTagService>,
        decoder: Arc<dyn NcmDecoder>,
        fetcher: MediaFetcher,
        enricher: Option<Arc<dyn OnlineEnricher>>,
    ) -> Self {
        Self {
            api,
            tags,
            decoder,
            fetcher,
            enricher,
        }
    }

    /// 是否有可用的在线补全。
    pub fn online_match_available(&self) -> bool {
        self.enricher.is_some()
    }

    /// 批量下载 `songs`：最多 `options.max_concurrency` 首同时进行，结果按输入顺序返回。
    pub async fn download(
        self: &Arc<Self>,
        songs: Vec<NeteaseSong>,
        options: DownloadOptions,
        sink: ProgressSink,
        cancel: &CancelToken,
    ) -> Result<DownloadOutcome, DownloadError> {
        let directory = prepare_directory(&options.target_directory)?;
        let mut options = options;
        options.target_directory = directory;

        let total = songs.len();
        if total == 0 {
            return Ok(DownloadOutcome {
                results: Vec::new(),
                total: 0,
                cancelled: false,
                online_match_skipped: false,
            });
        }

        let completed = Arc::new(AtomicUsize::new(0));
        let concurrency = options.max_concurrency;
        let shared_options = Arc::new(options);
        let shared_songs = Arc::new(songs);
        let indices: Vec<usize> = (0..total).collect();

        let service = Arc::clone(self);
        let worker_options = Arc::clone(&shared_options);
        let worker_songs = Arc::clone(&shared_songs);
        let worker_cancel = cancel.clone();
        let worker: Arc<Worker<usize, DownloadItemResult>> = Arc::new(move |index, _| {
            let service = Arc::clone(&service);
            let options = Arc::clone(&worker_options);
            let songs = Arc::clone(&worker_songs);
            let cancel = worker_cancel.clone();
            let ctx = ItemContext::new(index, total, Arc::clone(&completed), Arc::clone(&sink));
            Box::pin(async move {
                let song = &songs[index];
                let result = service
                    .download_one(song, &options, &ctx, &cancel)
                    .await
                    .ok()?;
                ctx.finish(song, result.stage(), result.to_finish());
                Some(result)
            })
        });

        let slots = run_pool(indices, concurrency, cancel, worker).await;

        let mut results = Vec::new();
        let mut cancelled = cancel.is_cancelled();
        for (index, slot) in slots.into_iter().enumerate() {
            match slot {
                Slot::Done(result) => results.push(result),
                Slot::Panicked => results.push(DownloadItemResult::failure(
                    &shared_songs[index],
                    "处理这首歌时程序内部出错，请重试。",
                    None,
                )),
                Slot::NotRun => cancelled = true,
            }
        }
        let online_match_skipped = results.iter().any(|result| result.online_match_skipped);
        Ok(DownloadOutcome {
            results,
            total,
            cancelled,
            online_match_skipped,
        })
    }

    async fn download_item(
        &self,
        song: &NeteaseSong,
        options: &DownloadOptions,
        ctx: &ItemContext,
        cancel: &CancelToken,
    ) -> Result<DownloadItemResult, Cancelled> {
        let target = match prepare_directory(&options.target_directory) {
            Ok(target) => target,
            Err(error) => return Ok(DownloadItemResult::failure(song, error.to_string(), None)),
        };
        let mut note = None;
        match self
            .run_item(song, options, &target, ctx, cancel, &mut note)
            .await
        {
            Ok(result) => Ok(result),
            Err(Abort::Cancelled) => Err(Cancelled),
            Err(Abort::Failed(message)) => Ok(DownloadItemResult::failure(song, message, note)),
        }
    }

    async fn run_item(
        &self,
        song: &NeteaseSong,
        options: &DownloadOptions,
        target: &Path,
        ctx: &ItemContext,
        cancel: &CancelToken,
        quality_note: &mut Option<String>,
    ) -> Result<DownloadItemResult, Abort> {
        let song_name = display_name(song);
        let base_name = build_base_name(
            &options.file_name_template,
            Some(&song.name),
            Some(&song.artist_text()),
            Some(&song.album.name),
            song.track_number,
            parse_year(song.release_date.as_deref()),
            song.id,
        );

        // SkipExisting 优先于下载：不发起任何网络请求。
        if options.skip_existing
            && !options.overwrite_existing
            && let Some(existing) = find_existing_audio_file(target, &base_name)
        {
            ctx.report(
                song,
                DownloadPhase::Resolving,
                "已存在，跳过",
                Some(existing.to_string_lossy().into_owned()),
                Some(100.0),
            );
            return Ok(DownloadItemResult {
                song_id: song.id,
                song_name,
                success: true,
                output_path: Some(existing),
                error_message: None,
                decrypted_from_ncm: false,
                skipped: true,
                quality_note: None,
                online_match_skipped: false,
            });
        }

        ctx.report(
            song,
            DownloadPhase::Resolving,
            "获取播放地址",
            Some("正在获取下载地址…".to_owned()),
            Some(0.0),
        );
        let playback = cancellable(cancel, self.api.playback_info(song.id, options.quality))
            .await?
            .map_err(|error| {
                let reason = describe_api_error(&error);
                ctx.report(
                    song,
                    DownloadPhase::Resolving,
                    "获取播放地址失败",
                    Some(reason.clone()),
                    None,
                );
                Abort::Failed(reason)
            })?;

        let url = playback
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty());
        let (true, Some(url)) = (playback.is_success(), url) else {
            let reason = null_if_empty(playback.error_message.as_deref())
                .unwrap_or_else(|| "无法获取播放地址（可能受版权限制或需要登录/VIP）。".to_owned());
            ctx.report(
                song,
                DownloadPhase::Resolving,
                "获取播放地址失败",
                Some(reason.clone()),
                None,
            );
            return Err(Abort::Failed(reason));
        };

        // 试听片段绝不作为完整下载保存。
        if playback.is_trial {
            let message = "该歌曲仅返回试听片段（需要 VIP 会员），已跳过下载。".to_owned();
            ctx.report(
                song,
                DownloadPhase::Resolving,
                "需要会员",
                Some(message.clone()),
                None,
            );
            return Err(Abort::Failed(message));
        }

        // 高档位音质需要对应会员：服务端不支持时会静默降档，这里如实提示实际拿到的音质。
        if let Some(actual) = playback
            .level
            .as_deref()
            .and_then(AudioQuality::from_api_level)
            && actual != options.quality
        {
            let notice = format!(
                "实际音质：{}（{} 不可用，已自动降级）",
                actual.display_name(),
                options.quality.display_name()
            );
            *quality_note = Some(notice.clone());
            ctx.report(
                song,
                DownloadPhase::Resolving,
                "获取播放地址",
                Some(notice),
                Some(0.0),
            );
        }

        let mut extension = resolve_extension(playback.format.as_deref(), Some(url));
        let token = unique_token();
        let download_temp = target.join(format!(".lp-{token}.part"));
        let ncm_temp = target.join(format!(".lp-{token}.ncm"));
        let mut temps = TempFiles::default();
        temps.add_file(download_temp.clone());
        temps.add_file(ncm_temp.clone());

        ctx.report(
            song,
            DownloadPhase::Downloading,
            "下载中",
            Some("正在下载音频…".to_owned()),
            Some(0.0),
        );
        let mut last_percent: i64 = -1;
        let downloaded = self
            .fetcher
            .download_to_file(
                url,
                &download_temp,
                &mut |done, total| {
                    let percent = total.map_or(-1, |total| (done * 100 / total.max(1)) as i64);
                    if total.is_none() || percent != last_percent {
                        last_percent = percent;
                        ctx.report_bytes(song, done, total);
                    }
                },
                cancel,
            )
            .await
            .map_err(|error| match error {
                FetchError::Cancelled => Abort::Cancelled,
                other => {
                    let reason = other.to_string();
                    ctx.report(
                        song,
                        DownloadPhase::Downloading,
                        "下载失败",
                        Some(reason.clone()),
                        None,
                    );
                    Abort::Failed(reason)
                }
            })?;

        if downloaded == 0 {
            let message = "下载内容为空，已放弃该歌曲。".to_owned();
            ctx.report(
                song,
                DownloadPhase::Downloading,
                "下载失败",
                Some(message.clone()),
                None,
            );
            return Err(Abort::Failed(message));
        }
        ctx.report(
            song,
            DownloadPhase::Downloading,
            "下载完成",
            None,
            Some(100.0),
        );
        check(cancel)?;

        let audio_path;
        let mut decrypted_from_ncm = false;
        let mut ncm_cover: Option<Vec<u8>> = None;
        if has_ncm_magic(&download_temp) {
            ctx.report(
                song,
                DownloadPhase::Decrypting,
                "解密 NCM",
                Some("检测到 NCM 加密音频，正在解密…".to_owned()),
                None,
            );
            fs::rename(&download_temp, &ncm_temp).map_err(|e| io_abort(&e, target))?;
            let decoder = Arc::clone(&self.decoder);
            let (source, directory, token) =
                (ncm_temp.clone(), target.to_path_buf(), cancel.clone());
            let decrypted = tokio::task::spawn_blocking(move || {
                decoder.decrypt(&source, &directory, true, &token)
            })
            .await
            .map_err(|_| Abort::Failed("解密时程序内部出错，请重试。".to_owned()))?;
            let decrypted = match decrypted {
                Ok(result) => result,
                Err(NcmError::Cancelled) => return Err(Abort::Cancelled),
                Err(NcmError::Io(error)) => {
                    let reason = error_text::describe_write(&error, target.to_str());
                    return Err(fail_stage(
                        ctx,
                        song,
                        DownloadPhase::Decrypting,
                        "解密失败",
                        reason,
                    ));
                }
                Err(other) => {
                    return Err(fail_stage(
                        ctx,
                        song,
                        DownloadPhase::Decrypting,
                        "解密失败",
                        other.to_string(),
                    ));
                }
            };
            let output = decrypted
                .output_path
                .clone()
                .filter(|path| decrypted.success && path.is_file());
            let Some(output) = output else {
                let reason = null_if_empty(decrypted.error_message.as_deref())
                    .unwrap_or_else(|| "NCM 解密失败。".to_owned());
                return Err(fail_stage(
                    ctx,
                    song,
                    DownloadPhase::Decrypting,
                    "解密失败",
                    reason,
                ));
            };
            temps.add_file(output.clone());
            // 解密输出才是真实音频格式，优先于播放接口给出的格式。
            extension = resolve_container_extension(output.to_str()).unwrap_or(extension);
            ncm_cover = decrypted.cover_data.filter(|cover| !cover.is_empty());
            decrypted_from_ncm = true;
            audio_path = output;
        } else {
            audio_path = download_temp.clone();
        }

        let (plain, word) =
            build_lyrics(self.api.as_ref(), song.id, options.lyrics_mode, cancel).await?;

        let mut cover: Option<Vec<u8>> = None;
        if options.write_cover {
            ctx.report(song, DownloadPhase::Tagging, "获取封面", None, None);
            cover = match ncm_cover {
                Some(cover) => Some(cover),
                None => {
                    self.fetcher
                        .try_download_bytes(song.cover_url(), MAX_COVER_BYTES, cancel)
                        .await?
                }
            };
        }
        check(cancel)?;

        let tags = create_tag_data(song, cover.as_deref(), plain.as_deref(), word.as_deref());
        ctx.report(
            song,
            DownloadPhase::Tagging,
            "写入标签",
            Some("正在写入元数据/封面/歌词…".to_owned()),
            None,
        );
        let (tagged, output_path) = {
            let tag_service = Arc::clone(&self.tags);
            let (source, directory, base, ext) = (
                audio_path.clone(),
                target.to_path_buf(),
                base_name.clone(),
                extension.clone(),
            );
            let (song_id, overwrite) = (song.id, options.overwrite_existing);
            tokio::task::spawn_blocking(move || {
                write_tags_atomically(
                    tag_service.as_ref(),
                    &source,
                    &OutputPlan {
                        directory: &directory,
                        base_name: &base,
                        extension: &ext,
                        song_id: Some(song_id),
                        overwrite,
                    },
                    &tags,
                )
            })
            .await
            .map_err(|_| Abort::Failed("写入文件时程序内部出错，请重试。".to_owned()))?
            .map_err(|error| io_abort(&error, target))?
        };
        // 音频已经改名成成品，不再属于临时文件。
        temps.forget_file(&audio_path);

        let mut warning = (!tagged).then(|| "标签写入失败，已保留音频文件。".to_owned());
        let enrichment = self
            .try_enrich(&output_path, options, song, ctx, cancel)
            .await?;
        let mut online_match_skipped = false;
        match enrichment {
            Enrichment::Note(note) => {
                warning = Some(match warning {
                    Some(existing) => format!("{existing}；{note}"),
                    None => note,
                });
            }
            Enrichment::Unavailable => online_match_skipped = true,
            Enrichment::Nothing => {}
        }

        ctx.report(
            song,
            DownloadPhase::Tagging,
            "完成",
            warning.clone(),
            Some(100.0),
        );
        Ok(DownloadItemResult {
            song_id: song.id,
            song_name,
            success: true,
            output_path: Some(output_path),
            error_message: warning,
            decrypted_from_ncm,
            skipped: false,
            quality_note: quality_note.clone(),
            online_match_skipped,
        })
    }

    /// 标签写完后在线补全标签、封面与歌词。补全失败只作为提示返回，不影响本次下载成功。
    async fn try_enrich(
        &self,
        path: &Path,
        options: &DownloadOptions,
        song: &NeteaseSong,
        ctx: &ItemContext,
        cancel: &CancelToken,
    ) -> Result<Enrichment, Cancelled> {
        enrich_file(
            self.enricher.as_deref(),
            path,
            options.auto_match_online,
            cancel,
            |stage| {
                ctx.report(
                    song,
                    DownloadPhase::Matching,
                    "匹配在线歌词",
                    Some(stage.to_owned()),
                    None,
                )
            },
        )
        .await
    }
}

impl Downloader for DownloadService {
    fn download_one<'a>(
        &'a self,
        song: &'a NeteaseSong,
        options: &'a DownloadOptions,
        ctx: &'a ItemContext,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<DownloadItemResult, Cancelled>> {
        Box::pin(self.download_item(song, options, ctx, cancel))
    }
}

/// 在线补全的结果。
pub(crate) enum Enrichment {
    /// 没有请求补全，或补全判定为“无需处理”。
    Nothing,
    /// 请求了补全，但没有可用的在线服务。
    Unavailable,
    /// 补全的说明（成功时带来源，失败时是“在线匹配未完成：…”）。
    Note(String),
}

/// 对一个已写好标签的文件做在线补全（下载与同步共用）。
pub(crate) async fn enrich_file(
    enricher: Option<&dyn OnlineEnricher>,
    path: &Path,
    auto_match_online: bool,
    cancel: &CancelToken,
    announce: impl FnOnce(&str),
) -> Result<Enrichment, Cancelled> {
    if !auto_match_online || !path.is_file() {
        return Ok(Enrichment::Nothing);
    }
    let Some(enricher) = enricher else {
        return Ok(Enrichment::Unavailable);
    };
    announce("正在按 QQ 优先次序补全标签、封面与歌词…");
    match enricher.enrich(path, cancel).await {
        Ok(outcome) if outcome.success => {
            Ok(describe_enrichment(&outcome).map_or(Enrichment::Nothing, Enrichment::Note))
        }
        Ok(outcome) if outcome.skipped => Ok(Enrichment::Nothing),
        Ok(outcome) => Ok(Enrichment::Note(format!(
            "在线匹配未完成：{}",
            outcome.message
        ))),
        Err(EnrichError::Cancelled) => Err(Cancelled),
        Err(EnrichError::Failed(reason)) => {
            Ok(Enrichment::Note(format!("在线匹配未完成：{reason}")))
        }
    }
}

/// 补全结果文案附带来源（如“（QQ 音乐）”），方便确认歌词来源（C# `DescribeMatch`）。
fn describe_enrichment(outcome: &OnlineMatchOutcome) -> Option<String> {
    let message = null_if_empty(Some(&outcome.message));
    let label = outcome
        .source_label
        .as_deref()
        .filter(|label| !label.trim().is_empty());
    match (label, message) {
        (None, message) => message,
        (Some(label), None) => Some(format!("已在线补全（{label}）")),
        (Some(label), Some(message)) => Some(format!("{message}（{label}）")),
    }
}

/// 取歌词并按模式序列化：逐行写普通 LRC，逐字模式另写增强 LRC。歌词是可选增强，失败不影响主体。
pub(crate) async fn build_lyrics(
    api: &dyn DownloadApi,
    song_id: i64,
    mode: LyricsWriteMode,
    cancel: &CancelToken,
) -> Result<(Option<String>, Option<String>), Cancelled> {
    if mode == LyricsWriteMode::None {
        return Ok((None, None));
    }
    let Ok(raw) = cancellable(cancel, api.lyrics(song_id)).await? else {
        return Ok((None, None));
    };
    let document = parser::parse(&raw);
    if document.is_empty() {
        return Ok((None, None));
    }
    let plain = null_if_empty(Some(&parser::to_plain_lrc(&document, true)));
    let word = (mode == LyricsWriteMode::Word)
        .then(|| null_if_empty(Some(&parser::to_enhanced_lrc(&document, true))))
        .flatten();
    Ok((plain, word))
}

/// 歌曲显示名；别名为空时就是歌名。
pub(crate) fn display_name(song: &NeteaseSong) -> String {
    let name = song.display_name();
    if name.trim().is_empty() {
        song.name.clone()
    } else {
        name
    }
}

/// 把接口错误变成给用户看的原因：网络类用友好文案，其余保留接口给出的中文说明。
pub(crate) fn describe_api_error(error: &NeteaseError) -> String {
    if error.is_transient_network() {
        error.user_message()
    } else {
        error.message().to_owned()
    }
}

/// 创建（必要时）并返回目标目录的绝对路径。
pub(crate) fn prepare_directory(directory: &Path) -> Result<PathBuf, DownloadError> {
    if directory.as_os_str().to_string_lossy().trim().is_empty() {
        return Err(DownloadError::EmptyDirectory);
    }
    let full = std::path::absolute(directory)
        .map_err(|_| DownloadError::Directory("路径无效。".to_owned()))?;
    fs::create_dir_all(&full).map_err(|error| {
        DownloadError::Directory(error_text::describe_write(&error, full.to_str()))
    })?;
    Ok(full)
}

fn check(cancel: &CancelToken) -> Result<(), Abort> {
    if cancel.is_cancelled() {
        Err(Abort::Cancelled)
    } else {
        Ok(())
    }
}

fn io_abort(error: &std::io::Error, directory: &Path) -> Abort {
    Abort::Failed(error_text::describe_write(error, directory.to_str()))
}

fn fail_stage(
    ctx: &ItemContext,
    song: &NeteaseSong,
    phase: DownloadPhase,
    stage: &str,
    reason: String,
) -> Abort {
    ctx.report(song, phase, stage, Some(reason.clone()), None);
    Abort::Failed(reason)
}

#[cfg(test)]
mod tests;
