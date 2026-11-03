//! 批量导入本地 `.ncm`：解密为 mp3 / flac，并把 NCM 内嵌元数据（标题 / 歌手 / 专辑）、封面与网易云 ID
//! 写回音频标签；可选在解密成功后自动在线匹配标签、封面与歌词。单个文件失败不会中断整批。
//!
//! 对应 C# `NcmImportService` / `INcmImportService`、`MediaFileHelper.CreateNcmTagData`，以及
//! `NcmImportViewModel` 里与界面无关的部分（扫描 `.ncm`、输出目录校验、输入文件夹 / 输出目录的记忆）。
//!
//! ## 与 C# 的差异
//!
//! - **取消时保留已完成的结果**：C# 取消会抛 `OperationCanceledException`，已处理的结果全部丢失；
//!   这里返回 [`NcmImportOutcome`]（`cancelled == true`），界面能保留前面已经成功的条目。
//! - 写标签直接交给 [`AudioTagService`]：它自己就是“写临时副本 → 原子替换”，不需要 C# 那层额外的复制。
//! - **在线补全是挂钩**：真正的在线匹配（`BatchLyricsService`）属于模块 10。这里定义 [`OnlineEnricher`]，
//!   默认没有实现，勾选了“解密后自动在线匹配”时在结果里给出“将在后续版本提供”的提示并跳过。
//! - 按 C# 依次处理（不并发）：解密是磁盘顺序读写，并发只会让机械硬盘更慢。

mod job;
mod scan;
mod view;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use crate::ncm::{NcmDecoder, NcmDecryptResult, NcmError, NcmMetadata};
use crate::netease::http::BoxFuture;
use crate::settings::AppSettings;
use crate::tags::{AudioTagData, AudioTagService, TagError, detect_image_mime};
use crate::utilities::{CancelToken, error_text};

pub use job::{NcmJob, NcmJobSlot};
pub use scan::{NcmFileEntry, NcmScanResult, scan_ncm_files};
pub use view::{NcmDialogState, NcmFileResultView, NcmProgressView, NcmSummaryView};

/// 写进备注里的程序标记（与下载管线一致）。
const IMPORT_COMMENT: &str = "LunaticPrincess";

/// 勾选了在线匹配但没有可用的在线服务时给用户看的说明。
pub const ONLINE_MATCH_UNAVAILABLE_MESSAGE: &str = "在线匹配将在后续版本提供，已跳过。";

/// 导入选项。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NcmImportOptions {
    /// 目标文件已存在时是否覆盖。
    pub overwrite: bool,
    /// 解密成功后是否自动在线匹配标签、封面与歌词。
    pub auto_match_online: bool,
}

/// 单个文件的处理阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportStage {
    /// 正在解密并写回内嵌标签。
    Decrypting,
    /// 正在在线匹配。
    Matching,
    /// 这个文件处理完了（`result` 有值）。
    Finished,
}

/// 导入进度；每个文件至少回报“开始”和“完成”两次。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NcmImportProgress {
    /// 已处理完的文件数。
    pub completed: usize,
    /// 文件总数（去重后）。
    pub total: usize,
    /// 当前文件在列表里的序号（从 0 起）。
    pub index: usize,
    /// 当前文件的完整路径。
    pub path: PathBuf,
    /// 当前文件名。
    pub file_name: String,
    /// 处理阶段。
    pub stage: ImportStage,
    /// 给用户看的一句话说明。
    pub message: String,
    /// `Finished` 时的最终结果。
    pub result: Option<NcmDecryptResult>,
}

/// 一次导入的整体结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NcmImportOutcome {
    /// 已处理完的文件的结果，顺序与输入一致；取消时只包含取消之前完成的部分。
    pub results: Vec<NcmDecryptResult>,
    /// 去重后的文件总数。
    pub total: usize,
    /// 是否中途被取消。
    pub cancelled: bool,
}

/// 导入失败（整批无法开始）。单个文件的失败体现在结果里，不走这里。
#[derive(Debug, thiserror::Error)]
pub enum NcmImportError {
    /// 参数不合法。
    #[error("{0}")]
    InvalidArgument(&'static str),
    /// 输出目录无法创建。
    #[error("输出目录不可用：{0}")]
    OutputDirectory(String),
    /// 处理文件的后台任务异常退出。
    #[error("处理 NCM 文件时程序内部出错，请重试。")]
    Internal,
    /// 已经有一个导入任务在运行。
    #[error("已有一个 NCM 导入正在进行，请等待它完成或先取消。")]
    Busy,
}

/// 在线匹配的结果（对应 C# `BatchLyricsResult` 里导入用到的部分）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OnlineMatchOutcome {
    /// 是否匹配并写入成功。
    pub success: bool,
    /// 是否因为不需要而跳过（例如本地已经完整）。
    pub skipped: bool,
    /// 给用户看的说明。
    pub message: String,
    /// 匹配到的歌名。
    pub matched_title: Option<String>,
    /// 匹配到的歌手。
    pub matched_artist: Option<String>,
    /// 来源名称（如“QQ 音乐”）。
    pub source_label: Option<String>,
}

/// 在线补全失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnrichError {
    /// 被取消。
    Cancelled,
    /// 失败，附面向用户的中文原因。
    Failed(String),
}

/// 在线补全：对一个已解密的音频文件匹配在线标签、封面与歌词并写回。
///
/// 模块 10 的批量匹配服务会实现它（来源优先级、匹配度阈值、是否取歌词等都由实现自己读取当前设置）；
/// 本模块只定义挂钩，所以默认没有实现。
pub trait OnlineEnricher: Send + Sync {
    /// 为 `audio_path` 补全标签、封面与歌词。
    fn enrich<'a>(
        &'a self,
        audio_path: &'a Path,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<OnlineMatchOutcome, EnrichError>>;
}

/// NCM 批量导入服务。
pub struct NcmImportService {
    decoder: Arc<dyn NcmDecoder>,
    tags: Arc<dyn AudioTagService>,
    enricher: Option<Arc<dyn OnlineEnricher>>,
}

impl NcmImportService {
    /// 创建服务；`enricher` 为 `None` 表示没有在线匹配能力。
    pub fn new(
        decoder: Arc<dyn NcmDecoder>,
        tags: Arc<dyn AudioTagService>,
        enricher: Option<Arc<dyn OnlineEnricher>>,
    ) -> Self {
        Self {
            decoder,
            tags,
            enricher,
        }
    }

    /// 是否有可用的在线匹配。
    pub fn online_match_available(&self) -> bool {
        self.enricher.is_some()
    }

    /// 依次解密并导入 `paths`，输出到 `output_directory`。
    ///
    /// 单个文件的失败只体现在对应的结果里；取消时返回已完成的部分（`cancelled == true`）。
    /// 进度通过 `progress` 回报，回调在调用者的任务里同步执行，不要在里面做耗时操作。
    pub async fn import(
        &self,
        paths: &[PathBuf],
        output_directory: &Path,
        options: NcmImportOptions,
        progress: &(dyn Fn(NcmImportProgress) + Send + Sync),
        cancel: &CancelToken,
    ) -> Result<NcmImportOutcome, NcmImportError> {
        if output_directory
            .as_os_str()
            .to_string_lossy()
            .trim()
            .is_empty()
        {
            return Err(NcmImportError::InvalidArgument("输出目录不能为空。"));
        }
        let target_directory = std::path::absolute(output_directory)
            .map_err(|_| NcmImportError::InvalidArgument("输出目录路径无效。"))?;
        std::fs::create_dir_all(&target_directory)
            .map_err(|error| NcmImportError::OutputDirectory(error_text::describe(&error)))?;

        let paths = dedupe_paths(paths);
        let total = paths.len();
        let mut results = Vec::with_capacity(total);
        let mut cancelled = false;

        for (index, path) in paths.into_iter().enumerate() {
            if cancel.is_cancelled() {
                cancelled = true;
                break;
            }
            let file_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let report = |stage: ImportStage, message: String, result: Option<NcmDecryptResult>| {
                progress(NcmImportProgress {
                    completed: index + usize::from(result.is_some()),
                    total,
                    index,
                    path: path.clone(),
                    file_name: file_name.clone(),
                    stage,
                    message,
                    result,
                });
            };

            report(ImportStage::Decrypting, "正在解密 NCM…".to_owned(), None);

            let decoder = Arc::clone(&self.decoder);
            let tags = Arc::clone(&self.tags);
            let job_path = path.clone();
            let job_directory = target_directory.clone();
            let job_cancel = cancel.clone();
            let imported = tokio::task::spawn_blocking(move || {
                import_one(
                    decoder.as_ref(),
                    tags.as_ref(),
                    &job_path,
                    &job_directory,
                    options.overwrite,
                    &job_cancel,
                )
            })
            .await
            .map_err(|_| NcmImportError::Internal)?;
            let Some(mut result) = imported else {
                cancelled = true;
                break;
            };

            if result.success && options.auto_match_online {
                match self.enrich(&mut result, &report, cancel).await {
                    Flow::Continue => {}
                    Flow::Cancelled => {
                        cancelled = true;
                        break;
                    }
                }
            }

            let message = if result.success {
                match &result.enrichment_message {
                    None => "完成".to_owned(),
                    Some(note) => format!("完成 · {note}"),
                }
            } else {
                result
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "失败".to_owned())
            };
            report(ImportStage::Finished, message, Some(result.clone()));
            results.push(result);
        }

        Ok(NcmImportOutcome {
            results,
            total,
            cancelled,
        })
    }

    /// 在线补全一个已解密的文件，把说明写进 `result.enrichment_message`。
    async fn enrich(
        &self,
        result: &mut NcmDecryptResult,
        report: &(dyn Fn(ImportStage, String, Option<NcmDecryptResult>) + Sync),
        cancel: &CancelToken,
    ) -> Flow {
        let Some(enricher) = &self.enricher else {
            result.enrichment_message = Some(ONLINE_MATCH_UNAVAILABLE_MESSAGE.to_owned());
            return Flow::Continue;
        };
        let Some(audio) = result.output_path.clone().filter(|path| path.is_file()) else {
            return Flow::Continue;
        };

        report(
            ImportStage::Matching,
            "正在匹配在线标签、封面与歌词…".to_owned(),
            None,
        );
        match enricher.enrich(&audio, cancel).await {
            Ok(outcome) => result.enrichment_message = Some(describe_match(&outcome)),
            Err(EnrichError::Cancelled) => return Flow::Cancelled,
            Err(EnrichError::Failed(reason)) => {
                result.enrichment_message = Some(format!("在线匹配失败：{reason}"));
            }
        }
        Flow::Continue
    }
}

enum Flow {
    Continue,
    Cancelled,
}

/// 去重但保持输入顺序：先转成绝对路径，再按不区分大小写比较（Windows 文件系统不区分大小写）。
/// 无法转换的路径原样保留。
fn dedupe_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut unique = Vec::with_capacity(paths.len());
    for path in paths {
        if path.as_os_str().to_string_lossy().trim().is_empty() {
            continue;
        }
        match std::path::absolute(path) {
            Ok(full) => {
                if seen.insert(full.to_string_lossy().to_lowercase()) {
                    unique.push(full);
                }
            }
            Err(_) => unique.push(path.clone()),
        }
    }
    unique
}

/// 处理一个文件；返回 `None` 表示被取消。其余失败都体现在结果的 `success == false` 里。
fn import_one(
    decoder: &dyn NcmDecoder,
    tags: &dyn AudioTagService,
    ncm_path: &Path,
    target_directory: &Path,
    overwrite: bool,
    cancel: &CancelToken,
) -> Option<NcmDecryptResult> {
    if !ncm_path.is_file() {
        return Some(NcmDecryptResult::failure(ncm_path, "文件不存在。"));
    }
    if !decoder.is_ncm_file(ncm_path) {
        return Some(NcmDecryptResult::failure(ncm_path, "不是有效的 NCM 文件。"));
    }

    let mut decrypted = match decoder.decrypt(ncm_path, target_directory, overwrite, cancel) {
        Ok(decrypted) => decrypted,
        Err(NcmError::Cancelled) => return None,
        Err(error) => {
            let reason = match &error {
                NcmError::Io(io) => {
                    error_text::describe_write(io, Some(&target_directory.to_string_lossy()))
                }
                other => other.to_string(),
            };
            return Some(NcmDecryptResult::failure(
                ncm_path,
                format!("导入失败：{reason}"),
            ));
        }
    };
    if !decrypted.success {
        return Some(decrypted);
    }

    let Some(output) = decrypted.output_path.clone().filter(|p| p.is_file()) else {
        decrypted.success = false;
        decrypted.error_message = Some("解密输出文件不存在。".to_owned());
        return Some(decrypted);
    };

    let empty = NcmMetadata::empty();
    let data = create_ncm_tag_data(
        decrypted.metadata.as_ref().unwrap_or(&empty),
        decrypted.cover_data.as_deref(),
    );
    if let Err(error) = tags.write(&output, &data) {
        // 音频已解密成功但标签失败：保留输出文件并明确报告。
        let reason = match &error {
            TagError::Io(crate::error::CoreError::Io(io)) => {
                error_text::describe_write(io, Some(&target_directory.to_string_lossy()))
            }
            other => other.to_string(),
        };
        decrypted.success = false;
        decrypted.error_message = Some(format!("标签写入失败（音频已解密保留）：{reason}"));
    }
    Some(decrypted)
}

/// 构造 NCM 导入用的标签数据（NCM 内嵌元数据 + 封面）。
pub fn create_ncm_tag_data(metadata: &NcmMetadata, cover: Option<&[u8]>) -> AudioTagData {
    let artists = split_artists(metadata.artists.as_deref());
    let cover = cover.filter(|c| !c.is_empty());
    AudioTagData {
        title: null_if_empty(metadata.title.as_deref()),
        album_artist: artists.first().cloned(),
        artists,
        album: null_if_empty(metadata.album.as_deref()),
        cover_data: cover.map(<[u8]>::to_vec),
        cover_mime_type: cover.map(|c| detect_image_mime(c).to_owned()),
        netease_id: metadata.music_id.filter(|&id| id > 0),
        comment: Some(IMPORT_COMMENT.to_owned()),
        ..AudioTagData::default()
    }
}

/// 把 `a/b&c、d;e；f,g，h` 之类的歌手串拆成列表（C# `SplitArtists`）。
pub fn split_artists(artists: Option<&str>) -> Vec<String> {
    let Some(artists) = artists.filter(|a| !a.trim().is_empty()) else {
        return Vec::new();
    };
    artists
        .split(['/', '&', '、', ';', '；', ',', '，'])
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

fn null_if_empty(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// 把在线匹配结果整理成一句话（C# `DescribeMatch`）。
pub fn describe_match(outcome: &OnlineMatchOutcome) -> String {
    let source = outcome
        .source_label
        .as_deref()
        .filter(|label| !label.trim().is_empty())
        .map(|label| format!("（{label}）"))
        .unwrap_or_default();
    let song = match outcome
        .matched_title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
    {
        None => String::new(),
        Some(title) => {
            let artist = outcome
                .matched_artist
                .as_deref()
                .filter(|artist| !artist.trim().is_empty())
                .map(|artist| format!(" - {artist}"))
                .unwrap_or_default();
            format!("匹配「{title}{artist}」：")
        }
    };
    if outcome.success || outcome.skipped {
        format!("{song}{}{source}", outcome.message)
    } else {
        format!("{song}在线匹配失败：{}{source}", outcome.message)
    }
}

/// 校验并创建输出目录：必须是完整路径（`D:\x` 或 UNC），创建失败给出中文原因。
pub fn prepare_output_directory(text: &str) -> Result<PathBuf, String> {
    let trimmed = text.trim();
    let path = Path::new(trimmed);
    if trimmed.is_empty() || !path.is_absolute() {
        return Err("请先选择有效的输出目录（需要是完整路径，例如 D:\\音乐\\解密）。".to_owned());
    }
    std::fs::create_dir_all(path)
        .map_err(|error| format!("输出目录不可用：{}", error_text::describe(&error)))?;
    Ok(path.to_path_buf())
}

/// 窗口打开时恢复的状态：记住的输入文件夹，以及输出目录（记住的目录为空则回退到下载目录）。
pub fn restore_state(settings: &AppSettings) -> (Vec<String>, String) {
    let mut folders: Vec<String> = Vec::new();
    for folder in &settings.ncm_import_folders {
        let trimmed = folder.trim();
        if !trimmed.is_empty() && !folders.iter().any(|f| same_text(f, trimmed)) {
            folders.push(trimmed.to_owned());
        }
    }
    let output = if settings.ncm_import_output_directory.trim().is_empty() {
        settings.download_directory.clone()
    } else {
        settings.ncm_import_output_directory.clone()
    };
    (folders, output)
}

/// 把输入文件夹与输出目录写回设置对象（调用方负责保存）；**只改这两个字段**，其余设置原样保留。
pub fn remember_state(settings: &mut AppSettings, folders: &[String], output_directory: &str) {
    let mut remembered: Vec<String> = Vec::new();
    for folder in folders {
        let trimmed = folder.trim();
        if !trimmed.is_empty() && !remembered.iter().any(|f| same_text(f, trimmed)) {
            remembered.push(trimmed.to_owned());
        }
    }
    settings.ncm_import_folders = remembered;
    settings.ncm_import_output_directory = output_directory.trim().to_owned();
}

/// 不区分大小写的文本比较（路径在 Windows 上不区分大小写）。
fn same_text(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// 给界面的提示：当前设置的匹配度阈值（百分数）。
pub fn match_threshold_percent(settings: &AppSettings) -> u32 {
    (settings.match_threshold.clamp(0.30, 1.00) * 100.0).round() as u32
}

/// 在线匹配的来源说明（与 C# 的 `AutoMatchHintText` 一致）。
pub fn online_match_hint(threshold_percent: u32) -> String {
    format!(
        "在线匹配来源：QQ 音乐逐字歌词优先，其次网易云音乐 · 匹配度 {threshold_percent}%（可在「设置」的「在线匹配度」中调整）"
    )
}

#[cfg(test)]
mod tests;
