//! 本地曲库扫描，以及与歌单的差异匹配（网易云 ID 精确匹配 → 标题 / 歌手 / 时长评分；扫描时用文件名回退）。
//!
//! 对应 C# 的 `Core/Library/`：
//!
//! - [`track`]：`LocalTrack`、扫描进度与错误（`ILocalLibraryService.cs`）；
//! - [`scanner`]：`LocalLibraryService`（并行读标签、结果顺序稳定）与文件名推断；
//! - [`matcher`]：`SongMatcher`（评分与优先级与 C# 逐项一致）；
//! - [`text`]：归一化与相似度（用 UTF-16 码元复现 .NET 的字符串语义）；
//! - [`manager`]：原来放在 `MainWindowViewModel` 里的扫描缓存与对比编排；
//! - [`view`]：给界面用的 camelCase DTO。

pub mod manager;
pub mod matcher;
pub mod scanner;
pub mod text;
pub mod track;
pub mod view;

pub use manager::{LibraryManager, PlaylistMatches, ScanProgressSink};
pub use matcher::{
    DEFAULT_THRESHOLD, DURATION_TOLERANCE_MS, LocalAvailability, MatchMethod, SongMatch,
    SongMatcher, duration_similarity,
};
pub use scanner::{
    FileNameGuess, LocalLibraryService, ProgressCallback, SUPPORTED_EXTENSIONS, parse_file_name,
};
pub use track::{LibraryError, LibraryScanProgress, LocalTrack, ScanOutcome};
pub use view::{
    LibraryCommandError, LibrarySummaryView, LocalMatchView, MatchOutcomeView, MatchStats,
    ScanMode, ScanProgressView, build_match_view, unscanned_view,
};

#[cfg(test)]
mod tests;
