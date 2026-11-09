//! 一键同步：已存在且无需更新 → 跳过；本地是 NCM → 解密并重新写标签；不存在 → 联网下载。
//!
//! 对应 C# `MusicSyncService` / `ISyncService`。每个请求恰好产出一个结果，顺序与输入一致；支持取消与聚合进度。
//!
//! ## 与 C# 的差异
//!
//! - **按设置的并发数同时处理多首**：C# 逐首顺序处理（同步里每首歌各调一次下载服务，下载服务自己的并发上限
//!   形同虚设）。上千首的歌单逐首下载太慢，这里与下载共用同一个任务池；结果顺序、跳过 / 解密 / 下载三分支的
//!   行为与 C# 一致。文件名冲突靠原子占位解决，并发处理同名歌曲不会互相覆盖。
//! - **NCM 解密到临时子目录**：C# 先把 `.ncm` 解密到下载目录里、文件名取自 `.ncm` 的主干，下载目录里恰好有同名
//!   音频时会被覆盖；这里解密到下载目录下的临时子目录，写完标签再改名成最终文件名，不碰任何已有文件。
//! - 取消时保留已完成的结果（同下载服务），不抛异常。
//! - 在线补全沿用 [`OnlineEnricher`] 挂钩，没有接入时在 [`SyncOutcome::online_match_skipped`] 里提示一次。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use crate::download::fetch::{MAX_COVER_BYTES, MediaFetcher};
use crate::download::media::{
    OutputPlan, TempFiles, build_base_name, create_tag_data, first_non_empty, parse_year,
    resolve_container_extension, unique_token, write_tags_atomically,
};
use crate::download::ncm_import::{
    NcmImportError, NcmImportOptions, NcmImportProgress, NcmImportService, OnlineEnricher,
};
use crate::download::pool::{Slot, Worker, run_pool};
use crate::download::service::{
    DownloadApi, DownloadError, DownloadItemResult, DownloadOptions, DownloadPhase, Downloader,
    Enrichment, ItemContext, ItemFinish, ProgressSink, build_lyrics, display_name, enrich_file,
    prepare_directory,
};
pub use crate::download::service::{ItemSource as SyncSourceKind, SyncProgress};
use crate::library::LocalTrack;
use crate::ncm::NcmDecoder;
use crate::netease::NeteaseSong;
use crate::tags::AudioTagService;
use crate::utilities::{CancelToken, Cancelled, cancellable, error_text};

/// 一首歌的同步请求：歌单里的歌 + 它在本地曲库里匹配到的文件（没有匹配为 `None`）。
#[derive(Debug, Clone)]
pub struct SyncRequest {
    /// 歌单里的歌曲。
    pub song: NeteaseSong,
    /// 本地匹配到的文件。
    pub local: Option<LocalTrack>,
}

/// 同步选项。
#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// 下载选项（目录、音质、歌词、封面、文件名模板、并发数……）。
    pub download: DownloadOptions,
    /// 本地已有普通音频时是否仍然重新下载（对应设置里的“覆盖已存在文件”）。
    pub redownload_existing: bool,
}

/// 单曲同步结果（对应 C# `SyncItemResult`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncItemResult {
    /// 歌曲 ID。
    pub song_id: i64,
    /// 歌曲显示名。
    pub song_name: String,
    /// 是否成功（跳过也算成功）。
    pub success: bool,
    /// 这首歌是怎么处理的。
    pub source: SyncSourceKind,
    /// 成品文件路径。
    pub output_path: Option<PathBuf>,
    /// 失败原因，或成功时的补充说明。
    pub message: Option<String>,
    /// 服务端降档时的说明。
    pub quality_note: Option<String>,
    /// 请求了在线补全但没有可用的在线服务，已跳过。
    pub online_match_skipped: bool,
}

impl SyncItemResult {
    fn failed(song: &NeteaseSong, message: impl Into<String>) -> Self {
        Self {
            song_id: song.id,
            song_name: display_name(song),
            success: false,
            source: SyncSourceKind::Failed,
            output_path: None,
            message: Some(message.into()),
            quality_note: None,
            online_match_skipped: false,
        }
    }

    /// 最终阶段名（C# `StageOf`）。
    pub fn stage(&self) -> &'static str {
        if !self.success {
            return "失败";
        }
        match self.source {
            SyncSourceKind::Downloaded => "下载完成",
            SyncSourceKind::DecryptedNcm => "NCM 解密完成",
            SyncSourceKind::Skipped => "已跳过",
            SyncSourceKind::Failed => "完成",
        }
    }

    /// 给进度事件用的最终结果。
    pub fn to_finish(&self) -> ItemFinish {
        ItemFinish {
            success: self.success,
            skipped: self.source == SyncSourceKind::Skipped,
            source: self.source,
            output_path: self
                .output_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            message: self.message.clone(),
            quality_note: self.quality_note.clone(),
        }
    }
}

/// 一次同步的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    /// 已处理完的歌曲的结果，顺序与输入一致；取消时只包含取消之前完成的部分。
    pub results: Vec<SyncItemResult>,
    /// 请求总数。
    pub total: usize,
    /// 是否中途被取消。
    pub cancelled: bool,
    /// 请求了在线补全但没有可用的在线服务，已跳过。
    pub online_match_skipped: bool,
}

impl SyncOutcome {
    /// 成功的数量（含跳过）。
    pub fn success_count(&self) -> usize {
        self.results.iter().filter(|r| r.success).count()
    }

    /// 失败的数量。
    pub fn failed_count(&self) -> usize {
        self.results.iter().filter(|r| !r.success).count()
    }

    /// 联网下载的数量。
    pub fn downloaded_count(&self) -> usize {
        self.count_source(SyncSourceKind::Downloaded)
    }

    /// 本地 NCM 解密的数量。
    pub fn decrypted_count(&self) -> usize {
        self.count_source(SyncSourceKind::DecryptedNcm)
    }

    /// 跳过的数量。
    pub fn skipped_count(&self) -> usize {
        self.count_source(SyncSourceKind::Skipped)
    }

    fn count_source(&self, source: SyncSourceKind) -> usize {
        self.results.iter().filter(|r| r.source == source).count()
    }
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

/// 一键同步服务。
pub struct SyncService {
    api: Arc<dyn DownloadApi>,
    decoder: Arc<dyn NcmDecoder>,
    tags: Arc<dyn AudioTagService>,
    importer: Arc<NcmImportService>,
    downloader: Arc<dyn Downloader>,
    fetcher: MediaFetcher,
    enricher: Option<Arc<dyn OnlineEnricher>>,
}

impl SyncService {
    /// 创建服务。`importer` 负责本地 NCM 的解密与基础标签；`downloader` 负责联网下载分支。
    pub fn new(
        api: Arc<dyn DownloadApi>,
        decoder: Arc<dyn NcmDecoder>,
        tags: Arc<dyn AudioTagService>,
        importer: Arc<NcmImportService>,
        downloader: Arc<dyn Downloader>,
        fetcher: MediaFetcher,
        enricher: Option<Arc<dyn OnlineEnricher>>,
    ) -> Self {
        Self {
            api,
            decoder,
            tags,
            importer,
            downloader,
            fetcher,
            enricher,
        }
    }

    /// 同步 `requests`：最多 `options.download.max_concurrency` 首同时进行，结果按输入顺序返回。
    pub async fn sync(
        self: &Arc<Self>,
        requests: Vec<SyncRequest>,
        options: SyncOptions,
        sink: ProgressSink,
        cancel: &CancelToken,
    ) -> Result<SyncOutcome, DownloadError> {
        let mut options = options;
        options.download.target_directory = prepare_directory(&options.download.target_directory)?;

        let total = requests.len();
        if total == 0 {
            return Ok(SyncOutcome {
                results: Vec::new(),
                total: 0,
                cancelled: false,
                online_match_skipped: false,
            });
        }

        let completed = Arc::new(AtomicUsize::new(0));
        let concurrency = options.download.max_concurrency;
        let options = Arc::new(options);
        let requests = Arc::new(requests);
        let indices: Vec<usize> = (0..total).collect();

        let service = Arc::clone(self);
        let (worker_options, worker_requests, worker_cancel) =
            (Arc::clone(&options), Arc::clone(&requests), cancel.clone());
        let worker: Arc<Worker<usize, SyncItemResult>> = Arc::new(move |index, _| {
            let service = Arc::clone(&service);
            let options = Arc::clone(&worker_options);
            let requests = Arc::clone(&worker_requests);
            let cancel = worker_cancel.clone();
            let ctx = ItemContext::new(index, total, Arc::clone(&completed), Arc::clone(&sink));
            Box::pin(async move {
                let request = &requests[index];
                let song = &request.song;
                ctx.report(
                    song,
                    DownloadPhase::Resolving,
                    "处理中",
                    Some("正在同步…".to_owned()),
                    None,
                );
                let result = match service.sync_item(request, &options, &ctx, &cancel).await {
                    Ok(result) => result,
                    Err(Abort::Cancelled) => return None,
                    Err(Abort::Failed(message)) => SyncItemResult::failed(song, message),
                };
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
                Slot::Panicked => results.push(SyncItemResult::failed(
                    &requests[index].song,
                    "处理这首歌时程序内部出错，请重试。",
                )),
                Slot::NotRun => cancelled = true,
            }
        }
        let online_match_skipped = results.iter().any(|result| result.online_match_skipped);
        Ok(SyncOutcome {
            results,
            total,
            cancelled,
            online_match_skipped,
        })
    }

    async fn sync_item(
        &self,
        request: &SyncRequest,
        options: &SyncOptions,
        ctx: &ItemContext,
        cancel: &CancelToken,
    ) -> Result<SyncItemResult, Abort> {
        let song = &request.song;
        match &request.local {
            Some(local) if local.is_ncm => {
                self.sync_from_ncm(song, local, options, ctx, cancel).await
            }
            Some(local) if !options.redownload_existing => Ok(SyncItemResult {
                song_id: song.id,
                song_name: display_name(song),
                success: true,
                source: SyncSourceKind::Skipped,
                output_path: Some(local.path.clone()),
                message: Some("本地已存在，跳过。".to_owned()),
                quality_note: None,
                online_match_skipped: false,
            }),
            _ => self.sync_via_downloader(song, options, ctx, cancel).await,
        }
    }

    async fn sync_via_downloader(
        &self,
        song: &NeteaseSong,
        options: &SyncOptions,
        ctx: &ItemContext,
        cancel: &CancelToken,
    ) -> Result<SyncItemResult, Abort> {
        let item: DownloadItemResult = self
            .downloader
            .download_one(song, &options.download, ctx, cancel)
            .await?;
        let song_name = display_name(song);
        if item.skipped {
            return Ok(SyncItemResult {
                song_id: song.id,
                song_name,
                success: true,
                source: SyncSourceKind::Skipped,
                output_path: item.output_path,
                message: Some(
                    item.error_message
                        .unwrap_or_else(|| "已存在，跳过。".to_owned()),
                ),
                quality_note: item.quality_note,
                online_match_skipped: false,
            });
        }
        if !item.success {
            return Ok(SyncItemResult {
                quality_note: item.quality_note,
                ..SyncItemResult::failed(
                    song,
                    item.error_message
                        .unwrap_or_else(|| "下载失败。".to_owned()),
                )
            });
        }
        Ok(SyncItemResult {
            song_id: song.id,
            song_name,
            success: true,
            source: SyncSourceKind::Downloaded,
            output_path: item.output_path,
            message: item.error_message,
            quality_note: item.quality_note,
            online_match_skipped: item.online_match_skipped,
        })
    }

    async fn sync_from_ncm(
        &self,
        song: &NeteaseSong,
        local: &LocalTrack,
        options: &SyncOptions,
        ctx: &ItemContext,
        cancel: &CancelToken,
    ) -> Result<SyncItemResult, Abort> {
        let song_name = display_name(song);
        let ncm_path = local.path.clone();
        if ncm_path.as_os_str().to_string_lossy().trim().is_empty() || !ncm_path.is_file() {
            return Err(Abort::Failed(format!(
                "本地 NCM 文件不存在：{}",
                ncm_path.display()
            )));
        }
        let target = prepare_directory(&options.download.target_directory)
            .map_err(|error| Abort::Failed(error.to_string()))?;
        ctx.report(
            song,
            DownloadPhase::Decrypting,
            "解密 NCM",
            Some("正在解密本地 NCM…".to_owned()),
            None,
        );

        // 头部元数据只作为在线数据缺失时的回退，读取失败不影响解密。
        let metadata = {
            let decoder = Arc::clone(&self.decoder);
            let path = ncm_path.clone();
            tokio::task::spawn_blocking(move || decoder.read_info(&path))
                .await
                .ok()
                .and_then(Result::ok)
                .and_then(|info| info.metadata)
        };
        check(cancel)?;

        // 解密到下载目录下的临时子目录：成品文件名由模板决定，不会碰下载目录里已有的文件。
        let scratch = target.join(format!(".lp-sync-{}", unique_token()));
        let mut temps = TempFiles::default();
        temps.add_directory(scratch.clone());
        std::fs::create_dir_all(&scratch).map_err(|error| io_abort(&error, &target))?;

        let progress = |progress: NcmImportProgress| {
            ctx.report(
                song,
                DownloadPhase::Decrypting,
                "解密 NCM",
                Some(progress.message),
                None,
            );
        };
        let outcome = self
            .importer
            .import(
                std::slice::from_ref(&ncm_path),
                &scratch,
                NcmImportOptions {
                    overwrite: true,
                    auto_match_online: false,
                },
                &progress,
                cancel,
            )
            .await
            .map_err(|error| match error {
                NcmImportError::Busy => Abort::Failed("NCM 解密失败：导入器正忙。".to_owned()),
                other => Abort::Failed(format!("NCM 解密失败：{other}")),
            })?;
        if outcome.cancelled {
            return Err(Abort::Cancelled);
        }
        let Some(imported) = outcome.results.into_iter().next() else {
            return Err(Abort::Failed("NCM 解密失败：导入器未返回结果。".to_owned()));
        };
        if !imported.success {
            return Err(Abort::Failed(
                imported
                    .error_message
                    .unwrap_or_else(|| "NCM 解密失败。".to_owned()),
            ));
        }
        let Some(decrypted_path) = imported.output_path.clone().filter(|path| path.is_file())
        else {
            return Err(Abort::Failed("NCM 解密输出文件不存在。".to_owned()));
        };

        ctx.report(
            song,
            DownloadPhase::Resolving,
            "获取歌曲信息",
            Some("正在获取在线元数据/歌词/封面…".to_owned()),
            None,
        );
        let enriched = match cancellable(cancel, self.api.song_detail(song.id)).await? {
            Ok(Some(detail)) => detail,
            _ => song.clone(),
        };

        let (plain, word) = build_lyrics(
            self.api.as_ref(),
            song.id,
            options.download.lyrics_mode,
            cancel,
        )
        .await?;

        let mut cover: Option<Vec<u8>> = None;
        if options.download.write_cover {
            cover = match imported
                .cover_data
                .clone()
                .filter(|cover| !cover.is_empty())
            {
                Some(cover) => Some(cover),
                None => {
                    self.fetcher
                        .try_download_bytes(
                            enriched
                                .cover_download_url()
                                .or_else(|| song.cover_download_url())
                                .as_deref(),
                            MAX_COVER_BYTES,
                            cancel,
                        )
                        .await?
                }
            };
        }
        check(cancel)?;

        let tags = create_tag_data(
            &enriched,
            cover.as_deref(),
            plain.as_deref(),
            word.as_deref(),
        );

        // 在线字段缺失时回退到 NCM 内嵌元数据 / 本地扫描结果，保证文件名可用。
        let header = metadata.as_ref();
        let title = first_non_empty(&[
            Some(&enriched.name),
            header.and_then(|m| m.title.as_deref()),
            Some(&local.title),
        ])
        .or_else(|| first_non_empty(&[Some(&song.name)]))
        .unwrap_or_else(|| "未知歌曲".to_owned());
        let artist = first_non_empty(&[
            Some(&enriched.artist_text()),
            header.and_then(|m| m.artists.as_deref()),
            local.artist.as_deref(),
        ])
        .or_else(|| first_non_empty(&[Some(&song.artist_text())]));
        let album = first_non_empty(&[
            Some(&enriched.album.name),
            header.and_then(|m| m.album.as_deref()),
        ]);
        let extension = resolve_container_extension(decrypted_path.to_str())
            .unwrap_or_else(|| ".mp3".to_owned());
        let base_name = build_base_name(
            &options.download.file_name_template,
            Some(&title),
            artist.as_deref(),
            album.as_deref(),
            enriched.track_number.max(0),
            parse_year(enriched.release_date.as_deref()),
            song.id,
        );

        ctx.report(
            song,
            DownloadPhase::Tagging,
            "写入标签",
            Some("正在写入元数据/封面/歌词…".to_owned()),
            None,
        );
        let (tagged, output_path) = {
            let tag_service = Arc::clone(&self.tags);
            let (source, directory, base, ext) =
                (decrypted_path.clone(), target.clone(), base_name, extension);
            let (song_id, overwrite) = (song.id, options.download.overwrite_existing);
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
            .map_err(|error| io_abort(&error, &target))?
        };

        let mut warning = (!tagged).then(|| "标签写入失败，已保留解密音频。".to_owned());
        let mut online_match_skipped = false;
        match self
            .try_enrich(&output_path, options, song, ctx, cancel)
            .await?
        {
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
        Ok(SyncItemResult {
            song_id: song.id,
            song_name,
            success: true,
            source: SyncSourceKind::DecryptedNcm,
            output_path: Some(output_path),
            message: warning,
            quality_note: None,
            online_match_skipped,
        })
    }

    async fn try_enrich(
        &self,
        path: &Path,
        options: &SyncOptions,
        song: &NeteaseSong,
        ctx: &ItemContext,
        cancel: &CancelToken,
    ) -> Result<Enrichment, Cancelled> {
        enrich_file(
            self.enricher.as_deref(),
            path,
            options.download.auto_match_online,
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

#[cfg(test)]
mod tests;
