//! NCM 导入：扫描 `.ncm`、恢复 / 记住导入位置、批量解密并推送进度。
//!
//! 与 Tauri 外壳的 `ncm_get_state` / `ncm_scan` / `ncm_save_state` / `ncm_import` / `ncm_cancel`
//! 一一对应；进度用流推送（Tauri 那边是 `ncm:progress` 事件）。

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use lp_core::download::ncm_import::{
    ImportStage, NcmDialogState, NcmFileEntry, NcmFileResultView, NcmImportOptions,
    NcmImportProgress, NcmProgressView, NcmScanResult, NcmSummaryView, prepare_output_directory,
    scan_ncm_files,
};

use crate::frb_generated::StreamSink;
use crate::state::{bridge, run};

/// 扫描到的一个 `.ncm` 文件。
#[derive(Clone)]
pub struct NcmFileDto {
    /// 完整路径。
    pub path: String,
    /// 文件名。
    pub name: String,
    /// 大小（字节）。
    pub size: i64,
}

impl From<&NcmFileEntry> for NcmFileDto {
    fn from(entry: &NcmFileEntry) -> Self {
        Self {
            path: entry.path.clone(),
            name: entry.name.clone(),
            size: i64::try_from(entry.size).unwrap_or(i64::MAX),
        }
    }
}

/// 一次扫描的结果。
#[derive(Clone)]
pub struct NcmScanDto {
    /// 找到的文件。
    pub files: Vec<NcmFileDto>,
    /// 不存在（或不是文件夹）的输入文件夹。
    pub missing_folders: Vec<String>,
    /// 因权限不足等被跳过的子文件夹数。
    pub skipped_directories: i64,
}

impl From<NcmScanResult> for NcmScanDto {
    fn from(result: NcmScanResult) -> Self {
        Self {
            files: result.files.iter().map(NcmFileDto::from).collect(),
            missing_folders: result.missing_folders,
            skipped_directories: i64::try_from(result.skipped_directories).unwrap_or(i64::MAX),
        }
    }
}

/// 打开导入窗口时需要恢复的内容。
#[derive(Clone)]
pub struct NcmDialogStateDto {
    /// 记住的输入文件夹。
    pub folders: Vec<String>,
    /// 输出目录（记住的目录为空则是下载目录）。
    pub output_directory: String,
    /// 下载目录。
    pub download_directory: String,
    /// 是否有可用的在线匹配。
    pub online_match_available: bool,
    /// 在线匹配的来源与阈值说明。
    pub online_match_hint: String,
}

impl From<NcmDialogState> for NcmDialogStateDto {
    fn from(state: NcmDialogState) -> Self {
        Self {
            folders: state.folders,
            output_directory: state.output_directory,
            download_directory: state.download_directory,
            online_match_available: state.online_match_available,
            online_match_hint: state.online_match_hint,
        }
    }
}

/// 一个文件的最终结果（表格里一行）。
#[derive(Clone)]
pub struct NcmFileResultDto {
    /// 源 `.ncm` 的完整路径。
    pub path: String,
    /// 是否成功。
    pub success: bool,
    /// 输出音频的完整路径。
    pub output_path: Option<String>,
    /// 输出音频的文件名。
    pub output_file_name: Option<String>,
    /// 输出格式（`mp3` / `flac`）。
    pub format: Option<String>,
    /// 失败原因。
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

impl From<&NcmFileResultView> for NcmFileResultDto {
    fn from(view: &NcmFileResultView) -> Self {
        Self {
            path: view.path.clone(),
            success: view.success,
            output_path: view.output_path.clone(),
            output_file_name: view.output_file_name.clone(),
            format: view.format.clone(),
            error_message: view.error_message.clone(),
            enrichment_message: view.enrichment_message.clone(),
            title: view.title.clone(),
            artists: view.artists.clone(),
            album: view.album.clone(),
        }
    }
}

/// 处理阶段的字符串形式（`decrypting` / `matching` / `finished`）。
fn stage_name(stage: ImportStage) -> &'static str {
    match stage {
        ImportStage::Decrypting => "decrypting",
        ImportStage::Matching => "matching",
        ImportStage::Finished => "finished",
    }
}

/// 一个文件的处理进度。
#[derive(Clone)]
pub struct NcmProgressDto {
    /// 已处理完的文件数。
    pub completed: i64,
    /// 文件总数。
    pub total: i64,
    /// 当前文件序号（从 0 起）。
    pub index: i64,
    /// 当前文件的完整路径。
    pub path: String,
    /// 当前文件名。
    pub file_name: String,
    /// `decrypting` / `matching` / `finished`。
    pub stage: String,
    /// 一句话说明。
    pub message: String,
    /// `finished` 时的结果。
    pub result: Option<NcmFileResultDto>,
}

impl From<&NcmProgressView> for NcmProgressDto {
    fn from(view: &NcmProgressView) -> Self {
        Self {
            completed: view.completed as i64,
            total: view.total as i64,
            index: view.index as i64,
            path: view.path.clone(),
            file_name: view.file_name.clone(),
            stage: stage_name(view.stage).to_owned(),
            message: view.message.clone(),
            result: view.result.as_ref().map(NcmFileResultDto::from),
        }
    }
}

/// 一次导入的汇总。
#[derive(Clone)]
pub struct NcmSummaryDto {
    /// 已处理文件的结果。
    pub results: Vec<NcmFileResultDto>,
    /// 文件总数。
    pub total: i64,
    /// 成功数。
    pub succeeded: i64,
    /// 失败数。
    pub failed: i64,
    /// 是否被取消。
    pub cancelled: bool,
    /// 实际使用的输出目录。
    pub output_directory: String,
    /// 其中带有在线匹配成功说明的数量。
    pub matched_online: i64,
}

impl From<NcmSummaryView> for NcmSummaryDto {
    fn from(view: NcmSummaryView) -> Self {
        Self {
            results: view.results.iter().map(NcmFileResultDto::from).collect(),
            total: view.total as i64,
            succeeded: view.succeeded as i64,
            failed: view.failed as i64,
            cancelled: view.cancelled,
            output_directory: view.output_directory,
            matched_online: view.matched_online as i64,
        }
    }
}

/// `ncm_import` 的参数。
#[derive(Clone)]
pub struct NcmImportRequestDto {
    /// 要导入的 `.ncm` 文件（界面从扫描结果里取）。
    pub paths: Vec<String>,
    /// 当前的输入文件夹列表，开始前记住。
    pub folders: Vec<String>,
    /// 输出目录（必须是完整路径，不存在会创建）。
    pub output_directory: String,
    /// 目标文件已存在时是否覆盖。
    pub overwrite: bool,
    /// 解密后是否自动在线匹配。
    pub auto_match_online: bool,
}

/// 当前订阅导入进度的界面（只保留最后一个订阅者）。
static PROGRESS_SINK: OnceLock<Mutex<Option<StreamSink<NcmProgressDto>>>> = OnceLock::new();

fn publish_progress(progress: &NcmProgressView) {
    let Some(sink) = PROGRESS_SINK.get() else {
        return;
    };
    let sink = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(sink) = sink.as_ref() {
        // Dart 那边的订阅已经关掉时发送会失败，忽略即可。
        let _ = sink.add(NcmProgressDto::from(progress));
    }
}

/// 交给 `lp-core` 的进度回调。
fn progress_sink() -> Arc<dyn Fn(NcmImportProgress) + Send + Sync> {
    Arc::new(|progress: NcmImportProgress| publish_progress(&NcmProgressView::from(&progress)))
}

/// 订阅导入进度（只保留最后一个订阅者）。
pub fn watch_ncm_progress(sink: StreamSink<NcmProgressDto>) -> Result<(), String> {
    let cell = PROGRESS_SINK.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sink);
    Ok(())
}

/// 打开导入窗口时需要恢复的内容。
pub fn ncm_get_state() -> Result<NcmDialogStateDto, String> {
    let bridge = bridge()?;
    let state = NcmDialogState::new(&bridge.settings(), bridge.ncm.online_match_available());
    Ok(state.into())
}

/// 递归查找这些文件夹里的 `.ncm` 文件（不存在的文件夹会列在 `missing_folders` 里）。
pub async fn ncm_scan(folders: Vec<String>) -> Result<NcmScanDto, String> {
    run(async move {
        tokio::task::spawn_blocking(move || NcmScanDto::from(scan_ncm_files(&folders)))
            .await
            .map_err(|_| "扫描 .ncm 文件时程序内部出错，请重试。".to_owned())
    })
    .await?
}

/// 记住导入窗口的输入文件夹与输出目录（添加文件夹、关闭窗口时调用）。
pub fn ncm_save_state(folders: Vec<String>, output_directory: String) -> Result<bool, String> {
    bridge()?.remember_ncm_state(&folders, &output_directory);
    Ok(true)
}

/// 批量解密并导入 `.ncm`：逐文件推送进度，结束后返回汇总。
///
/// 被取消时返回已完成的部分（`cancelled: true`）；同一时间只允许一个任务。
pub async fn ncm_import(request: NcmImportRequestDto) -> Result<NcmSummaryDto, String> {
    let output = prepare_output_directory(&request.output_directory)?;
    if request.paths.is_empty() {
        return Err("没有可导入的 .ncm 文件，请先添加包含 .ncm 的文件夹。".to_owned());
    }
    let bridge = bridge()?;
    let job = bridge
        .ncm_job
        .try_begin()
        .map_err(|error| error.to_string())?;
    // 与 Tauri 外壳一致：开始导入前记住本次实际使用的位置。
    bridge.remember_ncm_state(&request.folders, &request.output_directory);

    let output_text = output.to_string_lossy().into_owned();
    let paths: Vec<PathBuf> = request.paths.iter().map(PathBuf::from).collect();
    let options = NcmImportOptions {
        overwrite: request.overwrite,
        auto_match_online: request.auto_match_online,
    };
    let auto_match_online = request.auto_match_online;
    let service = Arc::clone(&bridge.ncm);
    let logs = Arc::clone(&bridge.logs);
    let progress = progress_sink();
    run(async move {
        logs.info(format!(
            "开始导入 {} 个 NCM 文件，输出到 {output_text}",
            paths.len()
        ));
        let outcome = match service
            .import(&paths, &output, options, progress.as_ref(), &job.cancel)
            .await
        {
            Ok(outcome) => outcome,
            Err(error) => {
                let message = error.to_string();
                logs.warning(format!("NCM 导入失败：{message}"));
                return Err(message);
            }
        };
        let summary = NcmSummaryView::new(&outcome, &output_text, auto_match_online);
        if summary.cancelled {
            logs.warning(format!(
                "NCM 导入已取消：已处理 {}/{}，成功 {}，失败 {}",
                outcome.results.len(),
                summary.total,
                summary.succeeded,
                summary.failed
            ));
        } else if summary.failed == 0 {
            logs.success(format!(
                "NCM 导入完成：成功 {} 个（共 {} 个）",
                summary.succeeded, summary.total
            ));
        } else {
            logs.warning(format!(
                "NCM 导入完成：成功 {} 个，失败 {} 个（共 {} 个）",
                summary.succeeded, summary.failed, summary.total
            ));
        }
        Ok(NcmSummaryDto::from(summary))
    })
    .await?
}

/// 取消正在进行的导入；没有任务时返回 `false`。
pub fn ncm_cancel() -> Result<bool, String> {
    Ok(bridge()?.ncm_job.cancel())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_file() -> NcmFileResultView {
        NcmFileResultView {
            path: "D:/in/歌.ncm".to_owned(),
            success: true,
            output_path: Some("D:/out/歌.flac".to_owned()),
            output_file_name: Some("歌.flac".to_owned()),
            format: Some("flac".to_owned()),
            error_message: None,
            enrichment_message: Some("已写入在线标签".to_owned()),
            title: Some("歌".to_owned()),
            artists: Some("歌手".to_owned()),
            album: None,
        }
    }

    #[test]
    fn stage_names_are_stable() {
        let progress = NcmProgressView {
            completed: 1,
            total: 2,
            index: 0,
            path: "D:/in/歌.ncm".to_owned(),
            file_name: "歌.ncm".to_owned(),
            stage: ImportStage::Matching,
            message: "正在匹配".to_owned(),
            result: None,
        };
        let dto = NcmProgressDto::from(&progress);
        assert_eq!(dto.stage, "matching");
        assert_eq!(dto.total, 2);
        assert!(dto.result.is_none());
        assert_eq!(stage_name(ImportStage::Decrypting), "decrypting");
        assert_eq!(stage_name(ImportStage::Finished), "finished");
    }

    #[test]
    fn file_result_keeps_the_optional_fields() {
        let dto = NcmFileResultDto::from(&sample_file());
        assert!(dto.success);
        assert_eq!(dto.format.as_deref(), Some("flac"));
        assert_eq!(dto.output_file_name.as_deref(), Some("歌.flac"));
        assert!(dto.album.is_none());
    }

    #[test]
    fn scan_dto_keeps_missing_folders() {
        let scan = NcmScanResult {
            files: vec![NcmFileEntry {
                path: "D:/in/歌.ncm".to_owned(),
                name: "歌.ncm".to_owned(),
                size: 1024,
            }],
            missing_folders: vec!["D:/nope".to_owned()],
            skipped_directories: 2,
        };
        let dto = NcmScanDto::from(scan);
        assert_eq!(dto.files.len(), 1);
        assert_eq!(dto.files[0].size, 1024);
        assert_eq!(dto.missing_folders, vec!["D:/nope".to_owned()]);
        assert_eq!(dto.skipped_directories, 2);
    }

    #[test]
    fn summary_counts_become_i64() {
        let summary = NcmSummaryView {
            results: vec![sample_file()],
            total: 3,
            succeeded: 2,
            failed: 1,
            cancelled: false,
            output_directory: "D:/out".to_owned(),
            matched_online: 1,
        };
        let dto = NcmSummaryDto::from(summary);
        assert_eq!(dto.total, 3);
        assert_eq!(dto.failed, 1);
        assert_eq!(dto.matched_online, 1);
        assert_eq!(dto.results.len(), 1);
    }
}
