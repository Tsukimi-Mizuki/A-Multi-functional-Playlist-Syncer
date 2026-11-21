//! 批量获取标签、封面与歌词：把 `lp-core` 的批量任务与进度流暴露给 Flutter。
//!
//! 命令返回「二选一」载荷而不是 `Result` 的错误位：frb 的 `Result` 错误位只能放字符串，
//! 而界面要按 `kind`（`busy` / `noFiles` / `failed`）区分文案与按钮状态。

use std::sync::{Arc, Mutex, OnceLock};

use lp_core::lyrics::batch::{
    BatchJobError, BatchProgressView, BatchRequest, BatchResultView, BatchSummaryView,
};

use crate::frb_generated::StreamSink;
use crate::state::{bridge, run};

/// 批量任务的请求。来源次序不传，用设置里的值。
pub struct BatchRunRequestDto {
    pub files: Vec<String>,
    pub overwrite_existing: bool,
    pub fetch_lyrics: bool,
    pub prefer_word_lyrics: bool,
}

impl BatchRunRequestDto {
    fn into_request(self) -> BatchRequest {
        BatchRequest {
            files: self.files,
            tag_priority: None,
            lyrics_priority: None,
            overwrite_existing: self.overwrite_existing,
            fetch_lyrics: self.fetch_lyrics,
            prefer_word_lyrics: self.prefer_word_lyrics,
        }
    }
}

/// 一个文件的结果。
pub struct BatchResultDto {
    pub file_path: String,
    pub file_name: String,
    /// `success` / `skipped` / `failed`。
    pub status: String,
    pub success: bool,
    pub skipped: bool,
    pub message: String,
    pub matched_title: Option<String>,
    pub matched_artist: Option<String>,
    pub source_label: Option<String>,
    pub lyrics_format: Option<String>,
}

impl From<BatchResultView> for BatchResultDto {
    fn from(view: BatchResultView) -> Self {
        Self {
            file_path: view.file_path,
            file_name: view.file_name,
            status: view.status.to_owned(),
            success: view.success,
            skipped: view.skipped,
            message: view.message,
            matched_title: view.matched_title,
            matched_artist: view.matched_artist,
            source_label: view.source_label,
            lyrics_format: view.lyrics_format,
        }
    }
}

/// 逐文件的进度。
pub struct BatchProgressDto {
    pub completed: i64,
    pub total: i64,
    pub index: i64,
    pub path: String,
    pub file_name: String,
    /// `started`（开始处理）/ `finished`（处理完，`result` 有值）。
    pub stage: String,
    pub result: Option<BatchResultDto>,
}

impl From<BatchProgressView> for BatchProgressDto {
    fn from(view: BatchProgressView) -> Self {
        Self {
            completed: view.completed as i64,
            total: view.total as i64,
            index: view.index as i64,
            path: view.path,
            file_name: view.file_name,
            stage: view.stage.to_owned(),
            result: view.result.map(BatchResultDto::from),
        }
    }
}

/// 批量任务的汇总（取消时是已处理的部分）。
pub struct BatchSummaryDto {
    pub total: i64,
    pub processed: i64,
    pub success: i64,
    pub skipped: i64,
    pub failed: i64,
    pub cancelled: bool,
    pub results: Vec<BatchResultDto>,
}

impl From<BatchSummaryView> for BatchSummaryDto {
    fn from(view: BatchSummaryView) -> Self {
        Self {
            total: view.total as i64,
            processed: view.processed as i64,
            success: view.success as i64,
            skipped: view.skipped as i64,
            failed: view.failed as i64,
            cancelled: view.cancelled,
            results: view.results.into_iter().map(BatchResultDto::from).collect(),
        }
    }
}

/// 批量任务失败：`kind` 是 `busy` / `noFiles` / `failed`。
pub struct BatchTaskErrorDto {
    pub kind: String,
    pub message: String,
}

impl From<BatchJobError> for BatchTaskErrorDto {
    fn from(error: BatchJobError) -> Self {
        Self {
            kind: error.kind.to_owned(),
            message: error.message,
        }
    }
}

/// `batch_run` 的结果：成功给汇总，失败给类别与中文原因。
pub struct BatchRunOutcomeDto {
    pub summary: Option<BatchSummaryDto>,
    pub error: Option<BatchTaskErrorDto>,
}

/// 「批量在线获取」的进度（只保留最后一个订阅者）。
static BATCH_PROGRESS_SINK: OnceLock<Mutex<Option<StreamSink<BatchProgressDto>>>> = OnceLock::new();

fn publish_batch_progress(progress: &BatchProgressView) {
    let Some(sink) = BATCH_PROGRESS_SINK.get() else {
        return;
    };
    let sink = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(sink) = sink.as_ref() {
        // Dart 那边的订阅已经关掉时发送会失败，忽略即可。
        let _ = sink.add(BatchProgressDto::from(progress.clone()));
    }
}

/// 订阅批量任务的进度（只保留最后一个订阅者）。
pub fn watch_batch_progress(sink: StreamSink<BatchProgressDto>) -> Result<(), String> {
    let cell = BATCH_PROGRESS_SINK.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sink);
    Ok(())
}

/// 交给 `lp-core` 的进度回调。
fn progress_sink() -> Arc<dyn Fn(BatchProgressView) + Send + Sync> {
    Arc::new(|progress: BatchProgressView| publish_batch_progress(&progress))
}

/// 批量获取标签、封面与歌词：最多 3 个文件并发，逐文件推进度，结束后给汇总。
///
/// 匹配度阈值与来源次序取自设置；与「歌词转逐字」共用一个任务槽。
pub async fn batch_run(request: BatchRunRequestDto) -> Result<BatchRunOutcomeDto, String> {
    let bridge = bridge()?;
    let batch = Arc::clone(&bridge.batch);
    let settings = bridge.settings();
    let progress = progress_sink();
    run(async move {
        match batch.run(request.into_request(), &settings, progress).await {
            Ok(summary) => BatchRunOutcomeDto {
                summary: Some(summary.into()),
                error: None,
            },
            Err(error) => BatchRunOutcomeDto {
                summary: None,
                error: Some(error.into()),
            },
        }
    })
    .await
}

/// 取消正在进行的批量任务（含「歌词转逐字」）；没有任务时返回 `false`。
pub fn batch_cancel() -> Result<bool, String> {
    Ok(bridge()?.batch.cancel())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> BatchResultView {
        BatchResultView {
            file_path: "C:/Music/曲.mp3".to_owned(),
            file_name: "曲.mp3".to_owned(),
            status: "skipped",
            success: false,
            skipped: true,
            message: "标签、封面与歌词已完整，跳过".to_owned(),
            matched_title: Some("曲".to_owned()),
            matched_artist: None,
            source_label: Some("网易云音乐".to_owned()),
            lyrics_format: None,
        }
    }

    #[test]
    fn result_status_and_optional_fields_survive() {
        let dto = BatchResultDto::from(sample_result());
        assert_eq!(dto.status, "skipped");
        assert!(dto.skipped);
        assert!(!dto.success);
        assert_eq!(dto.matched_title.as_deref(), Some("曲"));
        assert!(dto.matched_artist.is_none());
        assert!(dto.lyrics_format.is_none());
    }

    #[test]
    fn summary_counts_become_i64() {
        let dto = BatchSummaryDto::from(BatchSummaryView {
            total: 5,
            processed: 4,
            success: 2,
            skipped: 1,
            failed: 1,
            cancelled: true,
            results: vec![sample_result()],
        });
        assert_eq!(dto.total, 5);
        assert_eq!(dto.processed, 4);
        assert_eq!(dto.success, 2);
        assert_eq!(dto.skipped, 1);
        assert_eq!(dto.failed, 1);
        assert!(dto.cancelled);
        assert_eq!(dto.results.len(), 1);
    }

    #[test]
    fn progress_keeps_the_stage_and_the_result() {
        let dto = BatchProgressDto::from(BatchProgressView {
            completed: 2,
            total: 5,
            index: 1,
            path: "C:/Music/曲.mp3".to_owned(),
            file_name: "曲.mp3".to_owned(),
            stage: "finished",
            result: Some(sample_result()),
        });
        assert_eq!(dto.completed, 2);
        assert_eq!(dto.total, 5);
        assert_eq!(dto.index, 1);
        assert_eq!(dto.stage, "finished");
        assert!(dto.result.is_some());
    }

    #[test]
    fn error_kind_is_kept_for_the_ui() {
        let dto = BatchTaskErrorDto::from(BatchJobError {
            kind: "busy",
            message: "已有一个批量任务正在进行，请等待它完成或先取消。".to_owned(),
        });
        assert_eq!(dto.kind, "busy");
        assert!(dto.message.contains("批量任务"));
    }
}
