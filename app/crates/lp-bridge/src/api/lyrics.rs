//! 歌词：只读预览，以及「歌词转逐字」的两步任务（先联网核对、确认后再写入）。
//!
//! 与 Tauri 外壳的分工相同：预览结果与任务结论都装进 DTO；进度用流推给界面（只保留最后一个订阅者）；
//! 需要区分失败类别的命令用「二选一载荷」，因为 frb 的 `Result` 错误位只能放一段可直接展示的文字。

use std::sync::{Arc, Mutex, OnceLock};

use lp_core::lyrics::batch::{
    BatchJobError, WordLyricsProgressView, WordLyricsRequest, WordLyricsResultView,
    WordLyricsSummaryView, WordStatus, WordTarget,
};
use lp_core::lyrics::{
    LyricsError, LyricsErrorKind, LyricsLookup, LyricsMode, LyricsPreview, LyricsPreviewOptions,
};

use crate::frb_generated::StreamSink;
use crate::state::{bridge, run};

/// 可以直接显示的歌词预览。
#[derive(Clone)]
pub struct LyricsPreviewDto {
    /// 歌曲 ID。
    pub song_id: i64,
    /// 请求的形式：`line`（逐行）/ `word`（逐字优先）。
    pub mode: String,
    /// 歌词文本（LRC / 增强 LRC）。
    pub text: String,
    /// `text` 是否逐字格式（请求逐字但没有逐字数据时为 false，已回退逐行）。
    pub is_word_level: bool,
    /// 这首歌是否有逐字数据。
    pub has_word_data: bool,
    /// `text` 是否带翻译行。
    pub has_translation: bool,
    /// 歌词行数（不含翻译行）。
    pub line_count: i64,
    /// 没有歌词（纯音乐、未收录或解析后为空）。
    pub is_empty: bool,
    /// 来源名称。
    pub source: String,
}

impl From<LyricsPreview> for LyricsPreviewDto {
    fn from(preview: LyricsPreview) -> Self {
        Self {
            song_id: preview.song_id,
            mode: mode_name(preview.mode).to_owned(),
            text: preview.text,
            is_word_level: preview.is_word_level,
            has_word_data: preview.has_word_data,
            has_translation: preview.has_translation,
            line_count: preview.line_count as i64,
            is_empty: preview.is_empty,
            source: preview.source,
        }
    }
}

/// 取词失败的原因。
#[derive(Clone)]
pub struct LyricsErrorDto {
    /// `sessionExpired` / `failed`。
    pub kind: String,
    /// 中文说明，可以直接显示。
    pub message: String,
}

impl From<LyricsError> for LyricsErrorDto {
    fn from(error: LyricsError) -> Self {
        let kind = match error.kind {
            LyricsErrorKind::SessionExpired => "sessionExpired",
            LyricsErrorKind::Failed => "failed",
        };
        Self {
            kind: kind.to_owned(),
            message: error.message,
        }
    }
}

/// 预览的「二选一」载荷：成功时有 `data`，失败时有 `error`。
#[derive(Clone)]
pub struct LyricsPreviewResultDto {
    /// 成功时的预览。
    pub data: Option<LyricsPreviewDto>,
    /// 失败时的原因。
    pub error: Option<LyricsErrorDto>,
}

/// 一首歌「歌词转逐字」的结论。
#[derive(Clone)]
pub struct WordLyricsResultDto {
    /// 完整路径。
    pub file_path: String,
    /// 文件名。
    pub file_name: String,
    /// `ready` / `converted` / `alreadyWord` / `noLyrics` / `noMatch` / `noWordLyrics` / `mismatch` / `failed`。
    pub status: String,
    /// 中文说明。
    pub message: String,
    /// 逐字歌词的来源名。
    pub source_label: Option<String>,
    /// 要写（预览）或已经写了（转换）的地方：`embedded`（内嵌）/ `sidecar`（同名 .lrc）。
    pub targets: Vec<String>,
    /// 现有歌词的译文会保留。
    pub keeps_translation: bool,
}

impl From<WordLyricsResultView> for WordLyricsResultDto {
    fn from(view: WordLyricsResultView) -> Self {
        Self {
            file_path: view.file_path,
            file_name: view.file_name,
            status: status_name(view.status).to_owned(),
            message: view.message,
            source_label: view.source_label,
            targets: view.targets.into_iter().map(target_name).collect(),
            keeps_translation: view.keeps_translation,
        }
    }
}

/// 任务进度（每处理一个文件推两条：开始与结束）。
#[derive(Clone)]
pub struct WordLyricsProgressDto {
    /// 已处理完的文件数。
    pub completed: i64,
    /// 总数。
    pub total: i64,
    /// 这个文件在请求里的下标。
    pub index: i64,
    /// 完整路径。
    pub path: String,
    /// 文件名。
    pub file_name: String,
    /// `started` / `finished`（`result` 有值）。
    pub stage: String,
    /// 结论（`finished` 才有）。
    pub result: Option<WordLyricsResultDto>,
}

impl From<WordLyricsProgressView> for WordLyricsProgressDto {
    fn from(progress: WordLyricsProgressView) -> Self {
        Self {
            completed: progress.completed as i64,
            total: progress.total as i64,
            index: progress.index as i64,
            path: progress.path,
            file_name: progress.file_name,
            stage: progress.stage.to_owned(),
            result: progress.result.map(WordLyricsResultDto::from),
        }
    }
}

/// 预览或转换的汇总。
#[derive(Clone)]
pub struct WordLyricsSummaryDto {
    /// 请求的文件总数（去重后）。
    pub total: i64,
    /// 实际处理完的文件数（取消时小于 `total`）。
    pub processed: i64,
    /// 可以转换的（预览）。
    pub ready: i64,
    /// 已经转换的。
    pub converted: i64,
    /// 不需要或没法转换的。
    pub skipped: i64,
    /// 读写失败的。
    pub failed: i64,
    /// 是否被取消。
    pub cancelled: bool,
    /// 已处理文件的结果，按请求顺序。
    pub results: Vec<WordLyricsResultDto>,
}

impl From<WordLyricsSummaryView> for WordLyricsSummaryDto {
    fn from(view: WordLyricsSummaryView) -> Self {
        Self {
            total: view.total as i64,
            processed: view.processed as i64,
            ready: view.ready as i64,
            converted: view.converted as i64,
            skipped: view.skipped as i64,
            failed: view.failed as i64,
            cancelled: view.cancelled,
            results: view
                .results
                .into_iter()
                .map(WordLyricsResultDto::from)
                .collect(),
        }
    }
}

/// 批量任务失败的原因。
#[derive(Clone)]
pub struct BatchJobErrorDto {
    /// `busy` / `noFiles` / `failed`。
    pub kind: String,
    /// 中文说明，可以直接显示。
    pub message: String,
}

impl From<BatchJobError> for BatchJobErrorDto {
    fn from(error: BatchJobError) -> Self {
        Self {
            kind: error.kind.to_owned(),
            message: error.message,
        }
    }
}

/// 「歌词转逐字」的「二选一」载荷：成功时有 `summary`，失败时有 `error`。
#[derive(Clone)]
pub struct WordLyricsOutcomeDto {
    /// 成功时的汇总。
    pub summary: Option<WordLyricsSummaryDto>,
    /// 失败时的原因。
    pub error: Option<BatchJobErrorDto>,
}

fn mode_name(mode: LyricsMode) -> &'static str {
    match mode {
        LyricsMode::Line => "line",
        LyricsMode::Word => "word",
    }
}

fn parse_mode(mode: &str) -> Result<LyricsMode, String> {
    match mode {
        "line" => Ok(LyricsMode::Line),
        "word" => Ok(LyricsMode::Word),
        other => Err(format!("无法识别的歌词形式：{other}")),
    }
}

fn status_name(status: WordStatus) -> &'static str {
    match status {
        WordStatus::Ready => "ready",
        WordStatus::Converted => "converted",
        WordStatus::AlreadyWord => "alreadyWord",
        WordStatus::NoLyrics => "noLyrics",
        WordStatus::NoMatch => "noMatch",
        WordStatus::NoWordLyrics => "noWordLyrics",
        WordStatus::Mismatch => "mismatch",
        WordStatus::Failed => "failed",
    }
}

fn target_name(target: WordTarget) -> String {
    match target {
        WordTarget::Embedded => "embedded".to_owned(),
        WordTarget::Sidecar => "sidecar".to_owned(),
    }
}

/// 取一首歌的歌词预览。
///
/// `mode`：`line`（逐行）/ `word`（逐字优先）。歌名、歌手、专辑与时长只在当前来源不是网易云时
/// 用来检索其他来源；网易云按 `id` 精确取词。
pub async fn get_song_lyrics(
    id: i64,
    mode: String,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration_ms: Option<i64>,
) -> Result<LyricsPreviewResultDto, String> {
    let parsed = parse_mode(&mode)?;
    let bridge = bridge()?;
    let lyrics = Arc::clone(&bridge.lyrics);
    let options = LyricsPreviewOptions::from_settings(&bridge.settings());
    let lookup = LyricsLookup {
        song_id: id,
        title: title.unwrap_or_default(),
        artist: artist.unwrap_or_default(),
        album: album.unwrap_or_default(),
        duration_ms: duration_ms.unwrap_or(0),
    };
    run(async move {
        match lyrics.preview(&lookup, parsed, options).await {
            Ok(preview) => LyricsPreviewResultDto {
                data: Some(preview.into()),
                error: None,
            },
            Err(error) => LyricsPreviewResultDto {
                data: None,
                error: Some(error.into()),
            },
        }
    })
    .await
}

/// 「歌词转逐字」任务的进度（只保留最后一个订阅者）。
static WORD_PROGRESS_SINK: OnceLock<Mutex<Option<StreamSink<WordLyricsProgressDto>>>> =
    OnceLock::new();

fn publish_word_progress(progress: &WordLyricsProgressView) {
    let Some(sink) = WORD_PROGRESS_SINK.get() else {
        return;
    };
    let sink = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(sink) = sink.as_ref() {
        // Dart 那边的订阅已经关掉时发送会失败，忽略即可。
        let _ = sink.add(WordLyricsProgressDto::from(progress.clone()));
    }
}

/// 订阅「歌词转逐字」任务的进度（只保留最后一个订阅者）。
pub fn watch_word_lyrics_progress(sink: StreamSink<WordLyricsProgressDto>) -> Result<(), String> {
    let cell = WORD_PROGRESS_SINK.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sink);
    Ok(())
}

/// 交给 `lp-core` 的进度回调。
fn word_progress_sink() -> Arc<dyn Fn(WordLyricsProgressView) + Send + Sync> {
    Arc::new(|progress: WordLyricsProgressView| publish_word_progress(&progress))
}

/// 检查这些文件的歌词能不能换成逐字：只联网核对，不写任何文件；结果留在内存里等 `word_lyrics_apply`。
pub async fn word_lyrics_preview(files: Vec<String>) -> Result<WordLyricsOutcomeDto, String> {
    let bridge = bridge()?;
    let batch = Arc::clone(&bridge.batch);
    let settings = bridge.settings();
    let progress = word_progress_sink();
    run(async move {
        let request = WordLyricsRequest { files };
        match batch.word_preview(request, &settings, progress).await {
            Ok(summary) => WordLyricsOutcomeDto {
                summary: Some(summary.into()),
                error: None,
            },
            Err(error) => WordLyricsOutcomeDto {
                summary: None,
                error: Some(error.into()),
            },
        }
    })
    .await
}

/// 把检查时算好的逐字歌词写进文件（不再联网）。
pub async fn word_lyrics_apply(files: Vec<String>) -> Result<WordLyricsOutcomeDto, String> {
    let bridge = bridge()?;
    let batch = Arc::clone(&bridge.batch);
    let progress = word_progress_sink();
    run(async move {
        let request = WordLyricsRequest { files };
        match batch.word_apply(request, progress).await {
            Ok(summary) => WordLyricsOutcomeDto {
                summary: Some(summary.into()),
                error: None,
            },
            Err(error) => WordLyricsOutcomeDto {
                summary: None,
                error: Some(error.into()),
            },
        }
    })
    .await
}

/// 取消正在进行的批量任务（「歌词转逐字」与批量在线获取共用这一个任务槽）；没有任务时返回 `false`。
pub fn word_lyrics_cancel() -> Result<bool, String> {
    Ok(bridge()?.batch.cancel())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lyrics_mode_names_round_trip() {
        assert_eq!(mode_name(LyricsMode::Line), "line");
        assert_eq!(mode_name(LyricsMode::Word), "word");
        assert_eq!(parse_mode("line"), Ok(LyricsMode::Line));
        assert_eq!(parse_mode("word"), Ok(LyricsMode::Word));
        assert!(parse_mode("").is_err());
        assert!(parse_mode("Word").is_err());
    }

    #[test]
    fn preview_keeps_its_flags() {
        let preview = LyricsPreview {
            song_id: 42,
            mode: LyricsMode::Word,
            text: "[00:01.00]第一句".to_owned(),
            is_word_level: false,
            has_word_data: false,
            has_translation: true,
            line_count: 3,
            is_empty: false,
            source: "网易云音乐".to_owned(),
        };
        let dto = LyricsPreviewDto::from(preview);
        assert_eq!(dto.song_id, 42);
        assert_eq!(dto.mode, "word");
        assert_eq!(dto.line_count, 3);
        assert!(dto.has_translation);
        assert!(!dto.is_word_level);
    }

    #[test]
    fn error_kinds_are_stable() {
        let expired = LyricsErrorDto::from(LyricsError {
            kind: LyricsErrorKind::SessionExpired,
            message: "登录已失效".to_owned(),
        });
        assert_eq!(expired.kind, "sessionExpired");
        assert_eq!(expired.message, "登录已失效");
        let failed = LyricsErrorDto::from(LyricsError {
            kind: LyricsErrorKind::Failed,
            message: "网络错误".to_owned(),
        });
        assert_eq!(failed.kind, "failed");
    }

    #[test]
    fn word_status_and_target_names_are_stable() {
        let names: Vec<&str> = [
            WordStatus::Ready,
            WordStatus::Converted,
            WordStatus::AlreadyWord,
            WordStatus::NoLyrics,
            WordStatus::NoMatch,
            WordStatus::NoWordLyrics,
            WordStatus::Mismatch,
            WordStatus::Failed,
        ]
        .into_iter()
        .map(status_name)
        .collect();
        assert_eq!(
            names,
            [
                "ready",
                "converted",
                "alreadyWord",
                "noLyrics",
                "noMatch",
                "noWordLyrics",
                "mismatch",
                "failed"
            ]
        );
        assert_eq!(target_name(WordTarget::Embedded), "embedded");
        assert_eq!(target_name(WordTarget::Sidecar), "sidecar");
    }

    #[test]
    fn batch_error_kind_is_kept_for_the_ui() {
        let dto = BatchJobErrorDto::from(BatchJobError {
            kind: "busy",
            message: "已有一个批量任务正在进行".to_owned(),
        });
        assert_eq!(dto.kind, "busy");
        assert_eq!(dto.message, "已有一个批量任务正在进行");
    }
}
