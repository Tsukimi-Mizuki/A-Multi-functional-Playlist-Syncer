//! 小工具集合：对应 C# 的 `Core/Utilities/`。
//!
//! - [`audio_file_types`]：`AudioFileTypes`；
//! - [`error_text`]：`ErrorText`，把 IO 错误翻译成「是什么 + 怎么办」的中文；
//! - [`file_name_sanitizer`]：`FileNameSanitizer`，Windows 安全文件名与文件名模板；
//! - [`cancel`]：同步 IO 用的取消令牌（对应 `CancellationToken`）。

pub mod audio_file_types;
pub mod cancel;
pub mod error_text;
pub mod file_name_sanitizer;

pub use cancel::CancelToken;
