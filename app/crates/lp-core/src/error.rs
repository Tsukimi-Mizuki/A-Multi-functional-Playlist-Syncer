//! 统一错误类型。
//!
//! 对应 C# 的 `Core/Utilities/ErrorText.cs`：核心层只产出带语义的错误，
//! 面向用户的中文文案（“是什么 + 怎么办”）在这里集中转换，不把英文异常抛给界面。

use thiserror::Error;

/// 业务层的基础错误。后续模块会按需增加变体。
#[derive(Debug, Error)]
pub enum CoreError {
    /// 文件读写失败。
    #[error("文件操作失败：{0}")]
    Io(#[from] std::io::Error),
    /// 暂未实现的功能（重写期间的占位）。
    #[error("功能尚未实现：{0}")]
    NotImplemented(&'static str),
}

/// 业务层的统一返回类型。
pub type CoreResult<T> = Result<T, CoreError>;

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
}
