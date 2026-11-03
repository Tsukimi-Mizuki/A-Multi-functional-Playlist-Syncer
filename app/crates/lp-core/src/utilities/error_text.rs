//! 把底层 IO 错误翻译成用户能直接读懂的中文提示。
//!
//! 对应 C# `ErrorText`。Windows 的“拒绝访问”原始信息既没有中文也不告诉用户该怎么办，
//! 这里统一换成「哪个路径 + 出了什么问题 + 怎么解决」的表述；无法识别的错误回退到原始信息，保证不吞掉内容。
//!
//! 与 C# 的差异：.NET 异常的 `Message` 里带着路径（`Access to the path 'D:\x' is denied.`），所以 C# 靠正则从
//! 消息里“猜”路径；Rust 的 `io::Error` 通常不带路径，调用方知道目标目录时应传给 [`describe_write`]。
//! [`try_get_path`] 仍然保留，用来处理第三方库（或自定义错误）把路径写进消息的情况。

use std::io::{Error, ErrorKind};
use std::sync::LazyLock;

use regex::Regex;

/// `ERROR_ACCESS_DENIED`。
const ACCESS_DENIED: i32 = 5;
/// `ERROR_SHARING_VIOLATION`。
const SHARING_VIOLATION: i32 = 32;
/// `ERROR_LOCK_VIOLATION`（文件区域被另一进程锁定，对用户来说同样是“被占用”）。
const LOCK_VIOLATION: i32 = 33;
/// `ERROR_DISK_FULL`。
const DISK_FULL: i32 = 112;
/// `ERROR_FILENAME_EXCED_RANGE`。
const PATH_TOO_LONG: i32 = 206;

const WRITE_PERMISSION_HINT: &str = "请检查该文件夹的权限，或以管理员身份运行本程序后重试。";
const READ_PERMISSION_HINT: &str = "请检查该文件或文件夹的权限，或以管理员身份运行本程序后重试。";

/// 从错误信息里提取被拒绝访问的路径（支持英文与简繁中文的系统提示）。
static QUOTED_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:'|\u{201C}|\u{300C})(?P<path>[A-Za-z]:[^'\u{201D}\u{300D}\r\n]+)(?:'|\u{201D}|\u{300D})",
    )
    .expect("路径提取的正则是常量")
});

/// 是否为权限类错误（无写入权限、ACL 拒绝等）。
pub fn is_permission_error(error: &Error) -> bool {
    error.kind() == ErrorKind::PermissionDenied
        || error.raw_os_error() == Some(ACCESS_DENIED)
        || has_any_text(
            &error.to_string(),
            &[
                "is denied",
                "access is denied",
                "拒绝访问",
                "拒絕存取",
                "存取遭拒",
                "访问被拒绝",
                "存取被拒",
            ],
        )
}

/// 是否为磁盘空间不足。
pub fn is_disk_full(error: &Error) -> bool {
    error.kind() == ErrorKind::StorageFull
        || error.raw_os_error() == Some(DISK_FULL)
        || has_any_text(
            &error.to_string(),
            &[
                "not enough space",
                "disk full",
                "磁盘空间不足",
                "磁碟空間不足",
                "空間不足",
            ],
        )
}

/// 是否为文件被其他程序占用。
pub fn is_file_in_use(error: &Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(SHARING_VIOLATION | LOCK_VIOLATION)
    ) || has_any_text(
        &error.to_string(),
        &[
            "being used by another process",
            "另一个程序正在使用",
            "正在使用此文件",
            "被另一个进程使用",
            "另一個程序正在使用",
        ],
    )
}

/// 是否为路径过长。
pub fn is_path_too_long(error: &Error) -> bool {
    error.raw_os_error() == Some(PATH_TOO_LONG)
}

/// 是否属于本模块能给出明确中文说明的 IO 错误。
pub fn is_known_io_error(error: &Error) -> bool {
    is_permission_error(error)
        || is_disk_full(error)
        || is_file_in_use(error)
        || is_path_too_long(error)
}

/// 写入场景的提示：优先使用调用方已知的目标目录（比从错误信息里猜路径更准确）。
pub fn describe_write(error: &Error, target_directory: Option<&str>) -> String {
    if !is_permission_error(error) {
        return describe(error);
    }
    let path = non_blank(target_directory)
        .map(str::to_owned)
        .or_else(|| try_get_path(Some(&error.to_string())));
    match path {
        None => format!("无法写入目标文件夹：没有写入权限，{WRITE_PERMISSION_HINT}"),
        Some(path) => format!("无法写入「{path}」：没有写入权限，{WRITE_PERMISSION_HINT}"),
    }
}

/// 把错误转成中文提示；无法识别时回退到原始信息。
pub fn describe(error: &Error) -> String {
    let path = try_get_path(Some(&error.to_string()));
    if is_permission_error(error) {
        return match path {
            None => format!("权限不足：无法访问目标文件，{READ_PERMISSION_HINT}"),
            Some(path) => format!("无法访问「{path}」：权限不足，{READ_PERMISSION_HINT}"),
        };
    }
    if is_disk_full(error) {
        return match path {
            None => "磁盘空间不足，请清理磁盘后重试。".to_owned(),
            Some(path) => format!("磁盘空间不足，无法写入「{path}」，请清理该磁盘后重试。"),
        };
    }
    if is_file_in_use(error) {
        return match path {
            None => "文件正被其他程序占用，请关闭占用它的程序后重试。".to_owned(),
            Some(path) => format!("文件「{path}」正被其他程序占用，请关闭占用它的程序后重试。"),
        };
    }
    if is_path_too_long(error) {
        return "文件路径过长，请把文件放到更短的目录，或缩短文件名后重试。".to_owned();
    }
    let message = error.to_string();
    if message.trim().is_empty() {
        format!("{:?}", error.kind())
    } else {
        message
    }
}

/// 尽量从错误信息里取出路径；取不到返回 `None`。
pub fn try_get_path(message: Option<&str>) -> Option<String> {
    let message = message.filter(|m| !m.trim().is_empty())?;
    if let Some(captures) = QUOTED_PATH.captures(message) {
        return Some(captures["path"].trim().to_owned());
    }

    // 有些本地化提示把路径放在引号之外或用双引号；再退一步找带盘符的片段。
    let chars: Vec<char> = message.chars().collect();
    let index = chars.windows(2).position(|pair| pair == [':', '\\'])?;
    if index == 0 {
        return None;
    }
    let mut start = index - 1;
    while start > 0 && chars[start - 1].is_alphabetic() {
        start -= 1;
    }
    let mut end = start;
    while end < chars.len()
        && !chars[end].is_whitespace()
        && !matches!(chars[end], '\'' | '\u{201D}' | '\u{300D}' | '"')
    {
        end += 1;
    }
    if end - start > 3 {
        let path: String = chars[start..end].iter().collect();
        return Some(path.trim().to_owned());
    }
    None
}

fn has_any_text(message: &str, needles: &[&str]) -> bool {
    if message.trim().is_empty() {
        return false;
    }
    let haystack = message.to_lowercase();
    needles
        .iter()
        .any(|needle| haystack.contains(&needle.to_lowercase()))
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests;
