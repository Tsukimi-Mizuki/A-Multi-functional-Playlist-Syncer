//! 音频标签、封面与歌词的读写（MP3 / FLAC / M4A / OGG 等容器）。
//!
//! 对应 C# 的 `Core/Tags/`（`IAudioTagService`、`AudioTagService`），底层从 TagLibSharp 换成 Rust 的 `lofty`。
//!
//! - [`model`]：[`AudioTagData`]（要写入的数据）、[`AudioTagInfo`]（读取结果）、[`TagError`]；
//! - [`service`]：[`AudioTagService`] trait 与 lofty 实现 [`LoftyTagService`]；
//! - [`text`]：网易云 ID 标记、备注清理、多值拆分等纯函数；
//! - [`view`]：给界面用的视图（封面 data URL）、写入请求与错误 DTO。

pub mod model;
pub mod service;
pub mod text;
pub mod view;

pub use model::{AudioTagData, AudioTagInfo, TagError};
pub use service::{AudioTagService, LoftyTagService};
pub use text::{detect_image_mime, parse_netease_id};
pub use view::{MAX_COVER_PREVIEW_BYTES, TagCommandError, TagView, WriteTagsRequest};

#[cfg(test)]
mod fixtures;
