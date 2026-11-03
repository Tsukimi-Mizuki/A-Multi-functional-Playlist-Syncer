//! 递归查找文件夹里的 `.ncm` 文件（对应 `NcmImportViewModel.FindNcmFiles`）。

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// 扫描到的一个 `.ncm` 文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NcmFileEntry {
    /// 完整路径。
    pub path: String,
    /// 文件名。
    pub name: String,
    /// 大小（字节）。
    pub size: u64,
}

/// 一次扫描的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NcmScanResult {
    /// 找到的文件，按完整路径不区分大小写排序。
    pub files: Vec<NcmFileEntry>,
    /// 不存在（或不是文件夹）的输入文件夹。
    pub missing_folders: Vec<String>,
    /// 因权限不足或扫描时被移动 / 删除而跳过的子文件夹数。
    pub skipped_directories: usize,
}

/// 递归查找 `folders` 里所有 `.ncm` 文件：不存在的文件夹记入 `missing_folders`，
/// 无权限的子文件夹跳过但不影响其余部分；同一个文件（路径相同，不区分大小写）只出现一次。
/// 不跟随目录符号链接 / 联接点，避免环。
pub fn scan_ncm_files(folders: &[String]) -> NcmScanResult {
    let mut result = NcmScanResult::default();
    let mut seen = HashSet::new();

    for folder in folders {
        let root = Path::new(folder.trim());
        if folder.trim().is_empty() || !root.is_dir() {
            result.missing_folders.push(folder.clone());
            continue;
        }

        let mut pending: Vec<PathBuf> = vec![root.to_path_buf()];
        while let Some(directory) = pending.pop() {
            let Ok(entries) = fs::read_dir(&directory) else {
                result.skipped_directories += 1;
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if file_type.is_dir() {
                    pending.push(path);
                } else if is_ncm_name(&entry.file_name().to_string_lossy()) {
                    // 符号链接要再确认它指向的是文件。
                    let Ok(metadata) = fs::metadata(&path) else {
                        continue;
                    };
                    if !metadata.is_file() {
                        continue;
                    }
                    let text = path.to_string_lossy().into_owned();
                    if seen.insert(text.to_lowercase()) {
                        result.files.push(NcmFileEntry {
                            name: entry.file_name().to_string_lossy().into_owned(),
                            size: metadata.len(),
                            path: text,
                        });
                    }
                }
            }
        }
    }

    result
        .files
        .sort_by(|a, b| compare_ordinal_ignore_case(&a.path, &b.path));
    result
}

/// 文件名以 `.ncm` 结尾（不区分大小写；`.ncm` 本身也算，与 `*.ncm` 通配符一致）。
fn is_ncm_name(name: &str) -> bool {
    name.to_lowercase().ends_with(".ncm")
}

/// 与 .NET `StringComparer.OrdinalIgnoreCase` 一致：先转大写，再按 UTF-16 码元逐个比较。
fn compare_ordinal_ignore_case(a: &str, b: &str) -> Ordering {
    a.to_uppercase()
        .encode_utf16()
        .cmp(b.to_uppercase().encode_utf16())
}
