//! 在线搜索的领域模型。对应 C# `OnlineMusicSearch.cs`（`OnlineSongResult`、`OnlineSongDetails`）、
//! `ILyricsSource.cs`（`LyricsSourceCandidate`、`ILyricsSource`）与 `LyricsSourceKind` 的扩展方法。

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::library::text::is_blank;
use crate::models::LyricsSourceKind;
use crate::netease::http::BoxFuture;
use crate::utilities::{CancelToken, Cancelled};

/// 主词源：可带回标签 / 封面 / 歌词的在线音乐库。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnlineMusicSource {
    /// 网易云音乐。
    Netease,
    /// QQ 音乐。
    #[serde(rename = "QQMusic")]
    QqMusic,
}

impl OnlineMusicSource {
    /// 界面与日志里显示的来源名。
    pub fn label(self) -> &'static str {
        match self {
            Self::Netease => "网易云音乐",
            Self::QqMusic => "QQ 音乐",
        }
    }
}

/// 主词源映射到在线搜索接口的枚举；备用词源返回 `None`（对应 C# `LyricsSourceKind.ToMusicSource`）。
pub fn music_source_of(kind: LyricsSourceKind) -> Option<OnlineMusicSource> {
    match kind {
        LyricsSourceKind::Netease => Some(OnlineMusicSource::Netease),
        LyricsSourceKind::QqMusic => Some(OnlineMusicSource::QqMusic),
        LyricsSourceKind::Kugou | LyricsSourceKind::Lrclib => None,
    }
}

/// 联网搜索返回的歌曲摘要（同时包含可直接写入标签的常用字段）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineSongResult {
    /// 来源。
    pub source: OnlineMusicSource,
    /// 来源里的歌曲 ID（网易云是数字 ID，QQ 是 `songmid`）。
    pub source_id: String,
    /// 歌名。
    pub title: String,
    /// 歌手（多个以 `/` 连接）。
    pub artist: String,
    /// 专辑。
    pub album: String,
    /// 时长（毫秒），未知为 0。
    pub duration_ms: i64,
    /// 年份。
    #[serde(default)]
    pub year: Option<u32>,
    /// 音轨号。
    #[serde(default)]
    pub track_number: Option<u32>,
    /// 碟号。
    #[serde(default)]
    pub disc_number: Option<u32>,
    /// 流派。
    #[serde(default)]
    pub genre: Option<String>,
    /// 作曲家。
    #[serde(default)]
    pub composer: Option<String>,
    /// 封面地址。
    #[serde(default)]
    pub cover_url: Option<String>,
    /// QQ 音乐的数值 song id；逐行翻译接口需要。
    #[serde(default)]
    pub numeric_source_id: Option<String>,
}

impl OnlineSongResult {
    /// 来源名。
    pub fn source_label(&self) -> &'static str {
        self.source.label()
    }

    /// 列表里显示的歌名（空白时显示「未知歌曲」）。
    pub fn display_title(&self) -> &str {
        if is_blank(&self.title) {
            "未知歌曲"
        } else {
            &self.title
        }
    }

    /// 「歌手 · 专辑」副标题。
    pub fn subtitle(&self) -> String {
        let artist = if is_blank(&self.artist) {
            "未知歌手"
        } else {
            &self.artist
        };
        let album = if is_blank(&self.album) {
            "未知专辑"
        } else {
            &self.album
        };
        format!("{artist} · {album}")
    }

    /// `m:ss` 形式的时长；未知显示 `--:--`。
    pub fn duration_text(&self) -> String {
        if self.duration_ms <= 0 {
            return "--:--".to_owned();
        }
        let total_seconds = self.duration_ms / 1000;
        format!("{}:{:02}", total_seconds / 60, total_seconds % 60)
    }
}

/// 同一首歌的几种歌词写法，主词源详情与备用词源候选共用选择逻辑。
fn pick_lyrics<'a>(
    line: Option<&'a str>,
    word: Option<&'a str>,
    bilingual_line: Option<&'a str>,
    bilingual_word: Option<&'a str>,
    prefer_word: bool,
    bilingual: bool,
) -> Option<&'a str> {
    if bilingual {
        let translated = if prefer_word {
            bilingual_word.or(bilingual_line)
        } else {
            bilingual_line
        };
        if let Some(text) = translated.filter(|text| !is_blank(text)) {
            return Some(text);
        }
    }
    if prefer_word { word.or(line) } else { line }
}

fn not_blank(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|text| !is_blank(text))
}

/// 一首在线歌曲的完整信息：标签字段、歌词（逐行 / 逐字 / 双语）与封面。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnlineSongDetails {
    /// 歌曲信息（详情接口返回的字段优先）。
    pub song: OnlineSongResult,
    /// 逐行 LRC（可能是空串）。
    pub line_lyrics: Option<String>,
    /// 逐字（增强 LRC）。
    pub word_lyrics: Option<String>,
    /// 封面图片字节。
    pub cover_data: Option<Vec<u8>>,
    /// 原文 + 译文的逐行 LRC。
    pub bilingual_line_lyrics: Option<String>,
    /// 原文 + 译文的逐字 LRC。
    pub bilingual_word_lyrics: Option<String>,
}

impl OnlineSongDetails {
    /// 没有歌词与封面的详情。
    pub fn without_extras(song: OnlineSongResult) -> Self {
        Self {
            song,
            line_lyrics: None,
            word_lyrics: None,
            cover_data: None,
            bilingual_line_lyrics: None,
            bilingual_word_lyrics: None,
        }
    }

    /// 按偏好选择逐字（增强 LRC）或逐行歌词；逐字不可用时自动回退逐行。
    /// `bilingual` 为真时优先返回原文 + 翻译的双语版本。
    pub fn get_lyrics(&self, prefer_word: bool, bilingual: bool) -> Option<&str> {
        pick_lyrics(
            self.line_lyrics.as_deref(),
            self.word_lyrics.as_deref(),
            self.bilingual_line_lyrics.as_deref(),
            self.bilingual_word_lyrics.as_deref(),
            prefer_word,
            bilingual,
        )
    }

    /// 是否有逐字歌词。
    pub fn has_word_lyrics(&self) -> bool {
        not_blank(&self.word_lyrics)
    }

    /// 是否有翻译。
    pub fn has_translation(&self) -> bool {
        not_blank(&self.bilingual_line_lyrics) || not_blank(&self.bilingual_word_lyrics)
    }
}

/// 备用词源的候选结果：已按项目统一格式生成可直接写入标签的歌词文本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricsSourceCandidate {
    /// 来源名（如「酷狗音乐」）。
    pub source_label: String,
    /// 候选的歌名。
    pub title: String,
    /// 候选的歌手。
    pub artist: String,
    /// 候选的专辑（常为空）。
    pub album: String,
    /// 候选的时长（毫秒）。
    pub duration_ms: i64,
    /// 逐行 LRC。
    pub line_lyrics: Option<String>,
    /// 逐字（增强 LRC）。
    pub word_lyrics: Option<String>,
    /// 原文 + 译文的逐行 LRC。
    pub bilingual_line_lyrics: Option<String>,
    /// 原文 + 译文的逐字 LRC。
    pub bilingual_word_lyrics: Option<String>,
}

impl LyricsSourceCandidate {
    /// 语义与 [`OnlineSongDetails::get_lyrics`] 完全一致。
    pub fn get_lyrics(&self, prefer_word: bool, bilingual: bool) -> Option<&str> {
        pick_lyrics(
            self.line_lyrics.as_deref(),
            self.word_lyrics.as_deref(),
            self.bilingual_line_lyrics.as_deref(),
            self.bilingual_word_lyrics.as_deref(),
            prefer_word,
            bilingual,
        )
    }

    /// 是否有逐字歌词。
    pub fn has_word_lyrics(&self) -> bool {
        not_blank(&self.word_lyrics)
    }

    /// 是否有翻译。
    pub fn has_translation(&self) -> bool {
        not_blank(&self.bilingual_line_lyrics) || not_blank(&self.bilingual_word_lyrics)
    }
}

/// 在线请求失败。`Display` 是面向用户的中文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnlineError {
    /// 调用方取消了操作。
    Cancelled,
    /// 失败，附中文原因。
    Failed(String),
}

impl OnlineError {
    /// 构造失败。
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }
}

impl fmt::Display for OnlineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("操作已取消"),
            Self::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for OnlineError {}

impl From<Cancelled> for OnlineError {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

/// 备用词源的检索请求（对应 C# `ILyricsSource.FetchAsync` 的参数）。
#[derive(Debug, Clone, Copy)]
pub struct LyricsQuery<'a> {
    /// 多组搜索关键词（`lyrics::scorer::build_queries` 的结果）。
    pub queries: &'a [String],
    /// 本地歌名。
    pub title: &'a str,
    /// 本地歌手。
    pub artist: &'a str,
    /// 本地专辑。
    pub album: Option<&'a str>,
    /// 本地时长（毫秒）。
    pub duration_ms: i64,
    /// 匹配度阈值。
    pub min_score: f64,
}

/// `dyn` 兼容的异步返回值（与 `HttpTransport` 同一个类型）。
pub type SourceFuture<'a, T> = BoxFuture<'a, T>;

/// 备用词源：网易云 / QQ 音乐之外的补充来源（酷狗、LRCLIB 等）。
///
/// 搜索失败、无匹配或候选低于匹配度阈值时返回 `Ok(None)`，不抛错，避免拖垮整条业务链；
/// 只有调用方取消时返回 `Err(OnlineError::Cancelled)`。
pub trait LyricsSource: Send + Sync {
    /// 写入日志与结果里的来源名，例如「酷狗音乐」「LRCLIB」。
    fn label(&self) -> &str;

    /// 按本地歌曲信息检索一份歌词。
    fn fetch<'a>(
        &'a self,
        query: &'a LyricsQuery<'a>,
        cancel: &'a CancelToken,
    ) -> SourceFuture<'a, Result<Option<LyricsSourceCandidate>, OnlineError>>;
}
