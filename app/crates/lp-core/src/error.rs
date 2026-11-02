//! 统一错误类型。
//!
//! 对应 C# 的 `Core/Utilities/ErrorText.cs`：核心层只产出带语义的错误，
//! 面向用户的中文文案（“是什么 + 怎么办”）在这里集中转换，不把英文异常抛给界面。

use thiserror::Error;

/// 业务层的基础错误。后续模块会按需增加变体。
#[derive(Debug, Error)]
pub enum CoreError {
    /// 文件读写失败。
    #[error("{}", describe_io(.0))]
    Io(#[from] std::io::Error),
    /// JSON 序列化 / 解析失败。
    #[error("数据格式无法解析：{0}")]
    Json(#[from] serde_json::Error),
    /// Windows 数据保护接口（DPAPI）加解密失败。
    #[error("系统加密失败：{0}")]
    Dpapi(String),
    /// 目录解析或准备失败（所有候选位置都不可用）。
    #[error("无法准备数据目录：{0}")]
    Path(String),
    /// 登录文件读取失败；内容已经是面向用户的中文原因，原样显示。
    #[error("{0}")]
    Session(String),
    /// 暂未实现的功能（重写期间的占位）。
    #[error("功能尚未实现：{0}")]
    NotImplemented(&'static str),
}

/// 业务层的统一返回类型。
pub type CoreResult<T> = Result<T, CoreError>;

/// 把 IO 错误翻译成“是什么 + 怎么办”的中文说明。
///
/// 对应 C# `ErrorText` 对权限不足 / 空间不足 / 文件占用 / 路径过长的识别。
fn describe_io(error: &std::io::Error) -> String {
    use std::io::ErrorKind;

    // Windows 错误码：32 共享冲突、33 锁定冲突、112 磁盘已满、206 路径过长。
    let hint = match (error.kind(), error.raw_os_error()) {
        (ErrorKind::PermissionDenied, _) => "没有权限访问该位置，请检查文件夹权限，或换一个目录",
        (_, Some(32 | 33)) => "文件正被其他程序占用，请关闭占用它的程序后重试",
        (_, Some(112)) => "磁盘空间不足，请释放空间后重试",
        (_, Some(206)) => "路径过长，请缩短目录层级或文件名",
        (ErrorKind::NotFound, _) => "找不到指定的文件或目录",
        _ => "",
    };
    if hint.is_empty() {
        format!("文件操作失败：{error}")
    } else {
        format!("文件操作失败：{hint}（{error}）")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_chinese() {
        let err = CoreError::NotImplemented("登录");
        assert_eq!(err.to_string(), "功能尚未实现：登录");
    }

    #[test]
    fn io_error_converts() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err: CoreError = io.into();
        assert!(err.to_string().starts_with("文件操作失败"));
    }

    #[test]
    fn io_permission_and_space_hints_are_actionable() {
        let denied: CoreError = std::io::Error::from(std::io::ErrorKind::PermissionDenied).into();
        assert!(denied.to_string().contains("没有权限"));
        let full: CoreError = std::io::Error::from_raw_os_error(112).into();
        assert!(full.to_string().contains("磁盘空间不足"));
        let busy: CoreError = std::io::Error::from_raw_os_error(32).into();
        assert!(busy.to_string().contains("占用"));
    }

    #[test]
    fn json_error_is_chinese() {
        let parse = serde_json::from_str::<serde_json::Value>("{oops").unwrap_err();
        let err: CoreError = parse.into();
        assert!(err.to_string().starts_with("数据格式无法解析"));
    }

    #[test]
    fn session_error_is_shown_verbatim() {
        let err = CoreError::Session("登录文件格式不正确或已损坏".to_owned());
        assert_eq!(err.to_string(), "登录文件格式不正确或已损坏");
    }
}
