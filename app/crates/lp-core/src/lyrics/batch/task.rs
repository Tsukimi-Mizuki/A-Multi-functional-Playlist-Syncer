//! 批量获取标签 / 封面 / 歌词窗口背后的任务编排。对应 C# `BatchLyricsViewModel` 里与界面无关的部分：
//! 扫描目录、最多 3 个文件并发处理、逐文件汇报、统计成功 / 跳过 / 失败、取消。
//!
//! 同一时间只有一个批量任务（`busy`）；取消后不再启动新文件，已经在处理的文件在当前网络请求处立即停下。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use super::{
    BATCH_CONCURRENCY, BatchError, BatchLyricsOptions, BatchLyricsResult, BatchLyricsService,
};
use crate::download::pool::{Slot, Worker, run_pool};
use crate::logs::LogHub;
use crate::models::OnlineSourcePriority;
use crate::settings::AppSettings;
use crate::utilities::CancelToken;

/// 批量任务失败（`kind`：`busy` / `noFiles` / `failed`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchJobError {
    /// 机器可读的类别。
    pub kind: &'static str,
    /// 中文说明。
    pub message: String,
}

fn default_true() -> bool {
    true
}

/// `batch_run` 的请求（camelCase）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRequest {
    /// 要处理的文件（前端从 `batch_scan` 的结果里取）。
    pub files: Vec<String>,
    /// 标签 / 封面来源次序；不传用设置里的值。
    #[serde(default)]
    pub tag_priority: Option<OnlineSourcePriority>,
    /// 歌词来源次序；不传用设置里的值。
    #[serde(default)]
    pub lyrics_priority: Option<OnlineSourcePriority>,
    /// 覆盖已有的标签、封面与歌词。
    #[serde(default)]
    pub overwrite_existing: bool,
    /// 是否获取歌词。
    #[serde(default = "default_true")]
    pub fetch_lyrics: bool,
    /// 优先逐字歌词。
    #[serde(default)]
    pub prefer_word_lyrics: bool,
}

/// 一个文件的结果（给界面）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResultView {
    /// 完整路径。
    pub file_path: String,
    /// 文件名。
    pub file_name: String,
    /// `success` / `skipped` / `failed`。
    pub status: &'static str,
    /// 是否写入成功。
    pub success: bool,
    /// 是否因已完整而跳过。
    pub skipped: bool,
    /// 中文说明。
    pub message: String,
    /// 匹配到的歌名。
    pub matched_title: Option<String>,
    /// 匹配到的歌手。
    pub matched_artist: Option<String>,
    /// 来源名。
    pub source_label: Option<String>,
    /// 写入的歌词格式。
    pub lyrics_format: Option<String>,
}

fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

impl From<&BatchLyricsResult> for BatchResultView {
    fn from(result: &BatchLyricsResult) -> Self {
        Self {
            file_path: result.file_path.clone(),
            file_name: file_name_of(&result.file_path),
            status: if result.success {
                "success"
            } else if result.skipped {
                "skipped"
            } else {
                "failed"
            },
            success: result.success,
            skipped: result.skipped,
            message: result.message.clone(),
            matched_title: result.matched_title.clone(),
            matched_artist: result.matched_artist.clone(),
            source_label: result.source_label.clone(),
            lyrics_format: result.lyrics_format.clone().filter(|text| !text.is_empty()),
        }
    }
}

/// `batch:progress` 事件的载荷。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchProgressView {
    /// 已处理完的文件数（`finished` 事件才会增加）。
    pub completed: usize,
    /// 总数。
    pub total: usize,
    /// 这个文件在请求里的下标。
    pub index: usize,
    /// 完整路径。
    pub path: String,
    /// 文件名。
    pub file_name: String,
    /// `started`（开始处理）/ `finished`（处理完，`result` 有值）。
    pub stage: &'static str,
    /// 结果（`finished` 才有）。
    pub result: Option<BatchResultView>,
}

/// `batch_run` 的汇总。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSummaryView {
    /// 请求的文件总数（去重后）。
    pub total: usize,
    /// 实际处理完的文件数（取消时小于 `total`）。
    pub processed: usize,
    /// 成功数。
    pub success: usize,
    /// 跳过数。
    pub skipped: usize,
    /// 失败数。
    pub failed: usize,
    /// 是否被取消。
    pub cancelled: bool,
    /// 已处理文件的结果，按请求顺序。
    pub results: Vec<BatchResultView>,
}

/// 扫描到的一个音频文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchFileView {
    /// 完整路径。
    pub path: String,
    /// 文件名。
    pub name: String,
    /// 大小（字节）。
    pub size: u64,
}

/// `batch_scan` 的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchScanView {
    /// 扫描的目录。
    pub folder: String,
    /// 是否递归。
    pub recursive: bool,
    /// 目录为空白或不存在。
    pub directory_missing: bool,
    /// 找到的文件，按路径排序。
    pub files: Vec<BatchFileView>,
}

type ProgressSink = Arc<dyn Fn(BatchProgressView) + Send + Sync>;

/// 批量任务的“同一时间只有一个”登记处。
#[derive(Debug, Clone, Default)]
pub(super) struct JobSlot {
    current: Arc<Mutex<Option<CancelToken>>>,
}

pub(super) struct Job {
    pub(super) cancel: CancelToken,
    slot: Arc<Mutex<Option<CancelToken>>>,
}

fn lock(slot: &Mutex<Option<CancelToken>>) -> MutexGuard<'_, Option<CancelToken>> {
    slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl JobSlot {
    pub(super) fn try_begin(&self) -> Option<Job> {
        let mut current = lock(&self.current);
        if current.is_some() {
            return None;
        }
        let cancel = CancelToken::new();
        *current = Some(cancel.clone());
        Some(Job {
            cancel,
            slot: Arc::clone(&self.current),
        })
    }

    fn cancel(&self) -> bool {
        match lock(&self.current).as_ref() {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        *lock(&self.slot) = None;
    }
}

/// 批量任务管理器。
pub struct BatchManager {
    service: Arc<BatchLyricsService>,
    logs: Arc<LogHub>,
    pub(super) slot: JobSlot,
}

impl BatchManager {
    /// 创建管理器。
    pub fn new(service: Arc<BatchLyricsService>, logs: Arc<LogHub>) -> Self {
        Self {
            service,
            logs,
            slot: JobSlot::default(),
        }
    }

    /// 扫描目录里的音频文件。
    pub async fn scan(&self, folder: String, recursive: bool) -> BatchScanView {
        let scanned_folder = folder.clone();
        let files = tokio::task::spawn_blocking(move || {
            BatchLyricsService::scan_audio_files(&scanned_folder, recursive)
                .into_iter()
                .map(|path| BatchFileView {
                    name: file_name_of(&path.to_string_lossy()),
                    size: std::fs::metadata(&path).map_or(0, |meta| meta.len()),
                    path: path.to_string_lossy().into_owned(),
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();

        BatchScanView {
            directory_missing: crate::library::text::is_blank(&folder)
                || !Path::new(folder.trim()).is_dir(),
            folder,
            recursive,
            files,
        }
    }

    /// 取消正在运行的批量任务；没有任务时返回 `false`。
    pub fn cancel(&self) -> bool {
        self.slot.cancel()
    }

    /// 运行批量任务：最多 [`BATCH_CONCURRENCY`] 个文件并发，逐文件通过 `progress` 汇报。
    pub async fn run(
        &self,
        request: BatchRequest,
        settings: &AppSettings,
        progress: ProgressSink,
    ) -> Result<BatchSummaryView, BatchJobError> {
        let mut seen = HashSet::new();
        let files: Vec<PathBuf> = request
            .files
            .iter()
            .filter(|path| !path.trim().is_empty())
            .filter(|path| seen.insert(path.to_lowercase()))
            .map(PathBuf::from)
            .collect();
        if files.is_empty() {
            return Err(BatchJobError {
                kind: "noFiles",
                message: "没有可处理的音频文件，请先选择包含音频的文件夹。".to_owned(),
            });
        }
        let Some(job) = self.slot.try_begin() else {
            return Err(BatchJobError {
                kind: "busy",
                message: "已有一个批量任务正在进行，请等待它完成或先取消。".to_owned(),
            });
        };

        let total = files.len();
        let options = BatchLyricsOptions {
            tag_priority: request.tag_priority.unwrap_or(settings.tag_source_priority),
            lyrics_priority: request
                .lyrics_priority
                .unwrap_or(settings.lyrics_source_priority),
            overwrite_existing: request.overwrite_existing,
            fetch_lyrics: request.fetch_lyrics,
            prefer_word_lyrics: request.prefer_word_lyrics,
            min_score: settings.match_threshold,
        };
        self.logs
            .info(format!("开始批量获取标签、封面与歌词：{total} 个文件"));

        let completed = Arc::new(AtomicUsize::new(0));
        let worker: Arc<Worker<PathBuf, BatchLyricsResult>> = {
            let service = Arc::clone(&self.service);
            let cancel = job.cancel.clone();
            let completed = Arc::clone(&completed);
            Arc::new(move |index, path: PathBuf| {
                let service = Arc::clone(&service);
                let cancel = cancel.clone();
                let completed = Arc::clone(&completed);
                let progress = Arc::clone(&progress);
                Box::pin(async move {
                    let path_text = path.to_string_lossy().into_owned();
                    let file_name = file_name_of(&path_text);
                    progress(BatchProgressView {
                        completed: completed.load(Ordering::SeqCst),
                        total,
                        index,
                        path: path_text.clone(),
                        file_name: file_name.clone(),
                        stage: "started",
                        result: None,
                    });

                    let result = match service.fetch_for_file(&path, &options, &cancel).await {
                        Ok(result) => result,
                        // 取消时无需逐项记录。
                        Err(BatchError::Cancelled) => return None,
                        Err(BatchError::Failed(reason)) => BatchLyricsResult {
                            file_path: path_text.clone(),
                            success: false,
                            skipped: false,
                            message: format!("处理失败：{reason}"),
                            matched_title: None,
                            matched_artist: None,
                            source_label: None,
                            lyrics_format: None,
                        },
                    };

                    let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                    progress(BatchProgressView {
                        completed: done,
                        total,
                        index,
                        path: path_text,
                        file_name,
                        stage: "finished",
                        result: Some(BatchResultView::from(&result)),
                    });
                    Some(result)
                })
            })
        };

        let slots = run_pool(files.clone(), BATCH_CONCURRENCY, &job.cancel, worker).await;

        let mut results: Vec<BatchResultView> = Vec::new();
        for (path, slot) in files.iter().zip(slots) {
            match slot {
                Slot::Done(result) => results.push(BatchResultView::from(&result)),
                Slot::Panicked => results.push(BatchResultView::from(&BatchLyricsResult {
                    file_path: path.to_string_lossy().into_owned(),
                    success: false,
                    skipped: false,
                    message: "处理失败：程序内部出错，请重试。".to_owned(),
                    matched_title: None,
                    matched_artist: None,
                    source_label: None,
                    lyrics_format: None,
                })),
                Slot::NotRun => {}
            }
        }

        let success = results.iter().filter(|r| r.status == "success").count();
        let skipped = results.iter().filter(|r| r.status == "skipped").count();
        let failed = results.iter().filter(|r| r.status == "failed").count();
        let cancelled = job.cancel.is_cancelled();
        if cancelled {
            self.logs.warning(format!(
                "批量获取标签已取消：已处理 {}/{total}，成功 {success}，跳过 {skipped}，失败 {failed}",
                results.len()
            ));
        } else if failed == 0 {
            self.logs.success(format!(
                "批量获取标签完成：成功 {success}，跳过 {skipped}，失败 {failed}"
            ));
        } else {
            self.logs.warning(format!(
                "批量获取标签完成：成功 {success}，跳过 {skipped}，失败 {failed}"
            ));
        }

        Ok(BatchSummaryView {
            total,
            processed: results.len(),
            success,
            skipped,
            failed,
            cancelled,
            results,
        })
    }
}
