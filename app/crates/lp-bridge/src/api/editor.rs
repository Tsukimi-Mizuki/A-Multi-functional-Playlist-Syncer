//! 拖入文件的解析：把拖放的路径（文件或文件夹）交给 `lp-core::editor::resolve_drop` 判断，
//! 界面据此决定打开标签编辑器、批量修改标签，还是提示先解密 NCM。

use lp_core::editor::{DropKind, DropResolution, resolve_drop};

use crate::state::run;

/// 拖入结果里的一个音频文件（与批量面板的 BatchFileDto 同形）。
pub struct DropFileDto {
    pub path: String,
    pub name: String,
    pub size: i64,
}

/// 拖入解析结果，字段含义见 `lp-core` 的 DropResolution。
pub struct DropResolutionDto {
    pub kind: String,
    pub path: Option<String>,
    pub file_name: Option<String>,
    pub message: String,
    pub resolved_count: i64,
    pub total: i64,
    pub files: Vec<DropFileDto>,
}

fn kind_name(kind: DropKind) -> &'static str {
    match kind {
        DropKind::Audio => "audio",
        DropKind::Batch => "batch",
        DropKind::Ncm => "ncm",
        DropKind::Error => "error",
    }
}

fn count(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn bytes(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

impl From<DropResolution> for DropResolutionDto {
    fn from(value: DropResolution) -> Self {
        Self {
            kind: kind_name(value.kind).to_owned(),
            path: value.path,
            file_name: value.file_name,
            message: value.message,
            resolved_count: count(value.resolved_count),
            total: count(value.total),
            files: value
                .files
                .into_iter()
                .map(|file| DropFileDto {
                    path: file.path,
                    name: file.name,
                    size: bytes(file.size),
                })
                .collect(),
        }
    }
}

/// 解析拖入的路径。展开文件夹与去重都在 lp-core 里做，这里只负责把结果转成 Dart 类型。
pub async fn resolve_drop_paths(paths: Vec<String>) -> Result<DropResolutionDto, String> {
    run(async move {
        tokio::task::spawn_blocking(move || resolve_drop(&paths))
            .await
            .map(DropResolutionDto::from)
            .map_err(|error| format!("解析拖入的文件时程序内部出错，请重试：{error}"))
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_stable_name() {
        assert_eq!(kind_name(DropKind::Audio), "audio");
        assert_eq!(kind_name(DropKind::Batch), "batch");
        assert_eq!(kind_name(DropKind::Ncm), "ncm");
        assert_eq!(kind_name(DropKind::Error), "error");
    }

    #[test]
    fn empty_targets_become_an_error_with_the_reason() {
        let dto = DropResolutionDto::from(resolve_drop(&[]));
        assert_eq!(dto.kind, "error");
        assert_eq!(dto.total, 0);
        assert!(dto.path.is_none());
        assert!(!dto.message.is_empty());
    }

    #[test]
    fn files_and_counts_survive_the_conversion() {
        let dto = DropResolutionDto::from(DropResolution {
            kind: DropKind::Batch,
            path: None,
            file_name: None,
            message: "已打开 2 首".to_owned(),
            resolved_count: 2,
            total: 1,
            files: vec![lp_core::lyrics::batch::BatchFileView {
                path: "C:/Music/a.mp3".to_owned(),
                name: "a.mp3".to_owned(),
                size: 4096,
            }],
        });
        assert_eq!(dto.kind, "batch");
        assert_eq!(dto.resolved_count, 2);
        assert_eq!(dto.files.len(), 1);
        assert_eq!(dto.files[0].size, 4096);
    }
}
