//! 在线搜索与词源：网易云 / QQ 音乐为主，酷狗 KRC 与 LRCLIB 作为备用词源。
//!
//! 对应 C# 的 `Core/Online/`（`OnlineMusicSearchService`、`KugouLyricsSource`、`LrclibLyricsSource`、`LyricsSourceKind`）：
//!
//! - [`model`]：搜索结果 / 详情 / 备用词源候选的模型与 [`LyricsSource`] trait；
//! - [`service`]：[`OnlineMusicSearch`] trait 与真实实现 [`OnlineMusicSearchService`]
//!   （网易云走 `NeteaseClient`，QQ 走公开搜索 / 歌词接口，域名回退、PlayLyricInfo 逐字歌词）；
//! - [`kugou`] / [`lrclib`]：备用词源，只补歌词，任何失败都回落成“没有结果”；
//! - [`view`] / [`manager`]：给 Tauri 命令用的 DTO 与编排（搜索打分、取详情、备用词源、阈值）。
//!
//! 所有 HTTP 都经 [`HttpTransport`](crate::netease::http::HttpTransport) 抽象，单测用假传输，不联网。

pub mod kugou;
pub mod lrclib;
pub mod manager;
pub mod model;
pub(crate) mod net;
mod qq;
pub mod service;
pub mod view;

#[cfg(test)]
pub(crate) mod test_support;

pub use kugou::KugouLyricsSource;
pub use lrclib::LrclibLyricsSource;
pub use manager::{FallbackRequest, FileContext, LoadRequest, OnlineManager, SearchRequest};
pub use model::{
    LyricsQuery, LyricsSource, LyricsSourceCandidate, OnlineError, OnlineMusicSource,
    OnlineSongDetails, OnlineSongResult, SourceFuture, music_source_of,
};
pub use service::{OnlineMusicSearch, OnlineMusicSearchService};

use crate::settings::AppSettings;

/// 备用词源开关（对应 C# `AppServices.ApplyFallbackLyricsSettings`）：按词源名返回设置里是否启用，
/// 未知词源一律启用。批量服务在每次需要备用词源时调用它，所以设置改动**立即生效**。
pub fn fallback_source_enabled(settings: &AppSettings, label: &str) -> bool {
    match label {
        KugouLyricsSource::LABEL => settings.fallback_kugou_lyrics,
        LrclibLyricsSource::LABEL => settings.fallback_lrclib_lyrics,
        _ => true,
    }
}

#[cfg(test)]
mod tests;
