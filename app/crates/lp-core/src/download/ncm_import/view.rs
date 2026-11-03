//! 给界面用的 NCM 导入视图：路径转字符串、结果压平、camelCase。
//!
//! Tauri 外壳只做命令转发，这些转换放在 lp-core 里才能单测。

use serde::Serialize;

use super::{ImportStage, NcmImportOutcome, NcmImportProgress, online_match_hint};
use crate::ncm::NcmDecryptResult;

/// 一个文件的最终结果（前端表格的一行状态）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NcmFileResultView {
    /// 源 `.ncm` 的完整路径（前端用它对应列表里的行）。
    pub path: String,
    /// 是否成功。
    pub success: bool,
    /// 输出音频的完整路径。
    pub output_path: Option<String>,
    /// 输出音频的文件名。
    pub output_file_name: Option<String>,
    /// 输出格式（小写 `mp3` / `flac`）。
    pub format: Option<String>,
    /// 失败原因（中文）。
    pub error_message: Option<String>,
    /// 在线匹配的说明。
    pub enrichment_message: Option<String>,
    /// NCM 内嵌的歌名。
    pub title: Option<String>,
    /// NCM 内嵌的歌手。
    pub artists: Option<String>,
    /// NCM 内嵌的专辑。
    pub album: Option<String>,
}

impl From<&NcmDecryptResult> for NcmFileResultView {
    fn from(result: &NcmDecryptResult) -> Self {
        let metadata = result.metadata.as_ref();
        Self {
            path: result.source_path.to_string_lossy().into_owned(),
            success: result.success,
            output_path: result
                .output_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            output_file_name: result
                .output_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned()),
            format: result.format.clone(),
            error_message: result.error_message.clone(),
            enrichment_message: result.enrichment_message.clone(),
            title: metadata.and_then(|m| m.title.clone()),
            artists: metadata.and_then(|m| m.artists.clone()),
            album: metadata.and_then(|m| m.album.clone()),
        }
    }
}

/// `ncm:progress` 事件的载荷。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NcmProgressView {
    /// 已处理完的文件数。
    pub completed: usize,
    /// 文件总数。
    pub total: usize,
    /// 当前文件序号（从 0 起）。
    pub index: usize,
    /// 当前文件的完整路径。
    pub path: String,
    /// 当前文件名。
    pub file_name: String,
    /// 处理阶段。
    pub stage: ImportStage,
    /// 一句话说明。
    pub message: String,
    /// `finished` 时的结果。
    pub result: Option<NcmFileResultView>,
}

impl From<&NcmImportProgress> for NcmProgressView {
    fn from(progress: &NcmImportProgress) -> Self {
        Self {
            completed: progress.completed,
            total: progress.total,
            index: progress.index,
            path: progress.path.to_string_lossy().into_owned(),
            file_name: progress.file_name.clone(),
            stage: progress.stage,
            message: progress.message.clone(),
            result: progress.result.as_ref().map(NcmFileResultView::from),
        }
    }
}

/// `ncm_import` 命令的返回值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NcmSummaryView {
    /// 已处理文件的结果。
    pub results: Vec<NcmFileResultView>,
    /// 文件总数。
    pub total: usize,
    /// 成功数。
    pub succeeded: usize,
    /// 失败数。
    pub failed: usize,
    /// 是否被取消。
    pub cancelled: bool,
    /// 实际使用的输出目录。
    pub output_directory: String,
    /// 其中带有在线匹配成功说明的数量（C# 的“在线匹配 N 个”）。
    pub matched_online: usize,
}

impl NcmSummaryView {
    /// 由导入结果构造。
    pub fn new(
        outcome: &NcmImportOutcome,
        output_directory: &str,
        auto_match_online: bool,
    ) -> Self {
        let succeeded = outcome.results.iter().filter(|r| r.success).count();
        let matched_online = if auto_match_online {
            outcome
                .results
                .iter()
                .filter(|r| r.success)
                .filter(|r| {
                    r.enrichment_message
                        .as_deref()
                        .is_some_and(|m| m.contains("已写入") || m.contains("已完整"))
                })
                .count()
        } else {
            0
        };
        Self {
            results: outcome
                .results
                .iter()
                .map(NcmFileResultView::from)
                .collect(),
            total: outcome.total,
            succeeded,
            failed: outcome.results.len() - succeeded,
            cancelled: outcome.cancelled,
            output_directory: output_directory.to_owned(),
            matched_online,
        }
    }
}

/// `ncm_get_state` 的返回值：对话框打开时需要恢复的内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NcmDialogState {
    /// 记住的输入文件夹。
    pub folders: Vec<String>,
    /// 输出目录（记住的目录为空则是下载目录）。
    pub output_directory: String,
    /// 下载目录（给界面作占位 / 重置用）。
    pub download_directory: String,
    /// 是否有可用的在线匹配。
    pub online_match_available: bool,
    /// 在线匹配的来源说明；没有在线匹配时是“后续版本提供”的提示。
    pub online_match_hint: String,
}

impl NcmDialogState {
    /// 由设置和在线匹配可用性构造。
    pub fn new(settings: &crate::settings::AppSettings, online_match_available: bool) -> Self {
        let (folders, output_directory) = super::restore_state(settings);
        let online_match_hint = if online_match_available {
            online_match_hint(super::match_threshold_percent(settings))
        } else {
            "在线匹配将在后续版本提供：目前勾选只会记住你的选择，解密后不会联网。".to_owned()
        };
        Self {
            folders,
            output_directory,
            download_directory: settings.download_directory.clone(),
            online_match_available,
            online_match_hint,
        }
    }
}
