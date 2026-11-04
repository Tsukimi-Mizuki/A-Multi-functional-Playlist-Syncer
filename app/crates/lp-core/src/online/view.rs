//! 给界面（歌词与标签编辑器、批量窗口）用的在线搜索 DTO。全部 camelCase。

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

use super::model::{LyricsSourceCandidate, OnlineSongDetails, OnlineSongResult};
use crate::lyrics::scorer::{self, LyricsMatchScore};
use crate::models::LyricsSourceKind;
use crate::tags::detect_image_mime;

/// 一首在线歌曲（搜索结果的一行）：模型字段 + 界面常用的派生文本。
///
/// 前端把它原样传回 `online_load_result` 的 `song` 字段即可（多余的派生字段会被忽略）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineSongView {
    /// 模型字段（`source` / `sourceId` / `title` / `artist` / `album` / `durationMs` / `year` …）。
    #[serde(flatten)]
    pub song: OnlineSongResult,
    /// 来源名（「网易云音乐」「QQ 音乐」）。
    pub source_label: String,
    /// 列表里显示的歌名。
    pub display_title: String,
    /// 「歌手 · 专辑」。
    pub subtitle: String,
    /// `m:ss`。
    pub duration_text: String,
}

impl From<&OnlineSongResult> for OnlineSongView {
    fn from(song: &OnlineSongResult) -> Self {
        Self {
            source_label: song.source_label().to_owned(),
            display_title: song.display_title().to_owned(),
            subtitle: song.subtitle(),
            duration_text: song.duration_text(),
            song: song.clone(),
        }
    }
}

/// 打分结果（四路分值 + 总分）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchScoreView {
    /// 标题相似度（0-1）。
    pub title: f64,
    /// 歌手相似度（0-1）。
    pub artist: f64,
    /// 专辑相似度（0-1）。
    pub album: f64,
    /// 时长相似度（0-1）。
    pub duration: f64,
    /// 加权总分（0-1）。
    pub total: f64,
    /// 总分的百分数文本（如「87」）。
    pub percent: String,
    /// 被直接排除（伴奏 / 纯音乐）。
    pub rejected: bool,
    /// 排除原因。
    pub reject_reason: Option<String>,
    /// 标题与歌手都可信。
    pub confident: bool,
}

impl From<&LyricsMatchScore> for MatchScoreView {
    fn from(score: &LyricsMatchScore) -> Self {
        Self {
            title: score.title,
            artist: score.artist,
            album: score.album,
            duration: score.duration,
            total: score.total,
            percent: format!("{}", (score.total * 100.0).round() as i64),
            rejected: score.rejected,
            reject_reason: score.reject_reason.clone(),
            confident: score.is_confident(),
        }
    }
}

/// 搜索结果里的一项。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineSearchItem {
    /// 歌曲。
    pub song: OnlineSongView,
    /// 打分；没有本地文件上下文时为 `None`。
    pub score: Option<MatchScoreView>,
    /// 总分低于匹配度阈值（界面标注“匹配度较低”）；没打分时为 `false`。
    pub below_threshold: bool,
}

/// `online_search` 的返回值。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineSearchResponse {
    /// 搜索的来源。
    pub source: LyricsSourceKind,
    /// 实际使用的关键词（请求里留空时由文件名推出）。
    pub query: String,
    /// 是否按本地文件打了分（打分时结果按总分从高到低排序）。
    pub scored: bool,
    /// 当前设置里的匹配度阈值。
    pub threshold: f64,
    /// 阈值的百分数文本（「60」）。
    pub threshold_percent: String,
    /// 结果。
    pub items: Vec<OnlineSearchItem>,
}

/// 封面（Base64，不带 `data:` 前缀；可直接填进 `write_tags` 的 `coverBase64`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverView {
    /// MIME 类型。
    pub mime_type: String,
    /// 图片字节的 Base64。
    pub base64: String,
    /// 字节数。
    pub size: usize,
}

impl CoverView {
    pub(crate) fn new(data: &[u8]) -> Self {
        Self {
            mime_type: detect_image_mime(data).to_owned(),
            base64: STANDARD.encode(data),
            size: data.len(),
        }
    }
}

/// 歌词格式的说明（与 C# 编辑器状态栏同文案）：逐字 / 逐行 + 是否有翻译。
pub(crate) fn lyrics_label(prefer_word: bool, has_word: bool, has_translation: bool) -> String {
    let base = if prefer_word {
        if has_word {
            "逐字歌词"
        } else {
            "逐行歌词（该来源无逐字）"
        }
    } else {
        "逐行歌词"
    };
    if has_translation {
        format!("双语{base}")
    } else {
        format!("{base}（该来源无翻译）")
    }
}

/// `online_load_result` 的返回值：标签字段、歌词与封面。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedResultView {
    /// 详情接口返回的歌曲信息（字段比搜索摘要更全）。
    pub song: OnlineSongView,
    /// 按请求的偏好选好的歌词（双语优先；逐字不可用时回退逐行）；没有歌词为 `None`。
    pub lyrics: Option<String>,
    /// 歌词格式说明（如「双语逐字歌词」「逐行歌词（该来源无逐字）」）；没有歌词为 `None`。
    pub lyrics_label: Option<String>,
    /// 请求了逐字但只有逐行。
    pub word_fallback: bool,
    /// 是否有逐字歌词。
    pub has_word_lyrics: bool,
    /// 是否有翻译。
    pub has_translation: bool,
    /// 逐行 LRC（原文）。
    pub line_lyrics: Option<String>,
    /// 逐字（增强 LRC，原文）。
    pub word_lyrics: Option<String>,
    /// 原文 + 译文的逐行 LRC。
    pub bilingual_line_lyrics: Option<String>,
    /// 原文 + 译文的逐字 LRC。
    pub bilingual_word_lyrics: Option<String>,
    /// 封面。
    pub cover: Option<CoverView>,
    /// 与 C# 一致的状态栏文案：「已载入QQ 音乐的标签双语逐字歌词、封面，请确认后保存。」。
    pub status_text: String,
}

impl LoadedResultView {
    pub(crate) fn new(
        summary: &OnlineSongResult,
        details: Option<&OnlineSongDetails>,
        prefer_word: bool,
    ) -> Self {
        let song = details.map_or(summary, |details| &details.song);
        let lyrics = details
            .and_then(|details| details.get_lyrics(prefer_word, true))
            .filter(|text| !crate::library::text::is_blank(text))
            .map(str::to_owned);
        let has_word = details.is_some_and(OnlineSongDetails::has_word_lyrics);
        let has_translation = details.is_some_and(OnlineSongDetails::has_translation);
        let word_fallback = prefer_word && lyrics.is_some() && !has_word;
        let cover = details
            .and_then(|details| details.cover_data.as_deref())
            .filter(|data| !data.is_empty())
            .map(CoverView::new);

        let label = lyrics
            .as_ref()
            .map(|_| lyrics_label(prefer_word, has_word, has_translation));
        let mut parts: Vec<String> = Vec::new();
        parts.push(label.clone().unwrap_or_else(|| "未获取到歌词".to_owned()));
        if cover.is_some() {
            parts.push("封面".to_owned());
        }
        let status_text = format!(
            "已载入{}的标签{}，请确认后保存。",
            summary.source_label(),
            parts.join("、")
        );

        Self {
            song: OnlineSongView::from(song),
            lyrics,
            lyrics_label: label,
            word_fallback,
            has_word_lyrics: has_word,
            has_translation,
            line_lyrics: details.and_then(|d| d.line_lyrics.clone()),
            word_lyrics: details.and_then(|d| d.word_lyrics.clone()),
            bilingual_line_lyrics: details.and_then(|d| d.bilingual_line_lyrics.clone()),
            bilingual_word_lyrics: details.and_then(|d| d.bilingual_word_lyrics.clone()),
            cover,
            status_text,
        }
    }
}

/// 备用词源命中的歌词（`lyrics_source_fetch` 的返回值）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FallbackLyricsView {
    /// 来源名（「酷狗音乐」「LRCLIB」）。
    pub source_label: String,
    /// 候选的歌名。
    pub title: String,
    /// 候选的歌手。
    pub artist: String,
    /// 候选的专辑（常为空）。
    pub album: String,
    /// 候选的时长（毫秒）。
    pub duration_ms: i64,
    /// 候选与本地歌曲的匹配度。
    pub score: MatchScoreView,
    /// 按偏好选好的歌词（双语优先；逐字不可用时回退逐行）；候选没有可用歌词为 `None`。
    pub lyrics: Option<String>,
    /// 歌词格式说明。
    pub lyrics_label: Option<String>,
    /// 是否有逐字歌词。
    pub has_word_lyrics: bool,
    /// 是否有翻译。
    pub has_translation: bool,
    /// 逐行 LRC。
    pub line_lyrics: Option<String>,
    /// 逐字（增强 LRC）。
    pub word_lyrics: Option<String>,
    /// 原文 + 译文的逐行 LRC。
    pub bilingual_line_lyrics: Option<String>,
    /// 原文 + 译文的逐字 LRC。
    pub bilingual_word_lyrics: Option<String>,
    /// 与 C# 一致的状态栏文案。
    pub status_text: String,
}

impl FallbackLyricsView {
    pub(crate) fn new(
        candidate: &LyricsSourceCandidate,
        score: &LyricsMatchScore,
        prefer_word: bool,
    ) -> Self {
        let lyrics = candidate
            .get_lyrics(prefer_word, true)
            .filter(|text| !crate::library::text::is_blank(text))
            .map(str::to_owned);
        let label = lyrics.as_ref().map(|_| {
            lyrics_label(
                prefer_word,
                candidate.has_word_lyrics(),
                candidate.has_translation(),
            )
        });
        let status_text = match &label {
            Some(label) => format!(
                "已从{}载入{label}（仅歌词，标签与封面保持当前内容），请确认后保存。",
                candidate.source_label
            ),
            None => format!("{}没有返回可用歌词。", candidate.source_label),
        };
        Self {
            source_label: candidate.source_label.clone(),
            title: candidate.title.clone(),
            artist: candidate.artist.clone(),
            album: candidate.album.clone(),
            duration_ms: candidate.duration_ms,
            score: MatchScoreView::from(score),
            lyrics,
            lyrics_label: label,
            has_word_lyrics: candidate.has_word_lyrics(),
            has_translation: candidate.has_translation(),
            line_lyrics: candidate.line_lyrics.clone(),
            word_lyrics: candidate.word_lyrics.clone(),
            bilingual_line_lyrics: candidate.bilingual_line_lyrics.clone(),
            bilingual_word_lyrics: candidate.bilingual_word_lyrics.clone(),
            status_text,
        }
    }
}

/// `get_match_threshold` 的返回值。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchThresholdView {
    /// 当前阈值（0.30–1.00）。
    pub threshold: f64,
    /// 百分数文本（「60」），设置界面直接显示。
    pub percent: String,
    /// 合法下限。
    pub min: f64,
    /// 合法上限。
    pub max: f64,
    /// 默认值。
    pub default: f64,
}

impl MatchThresholdView {
    pub(crate) fn new(threshold: f64) -> Self {
        let clamped = threshold.clamp(scorer::MIN_THRESHOLD, scorer::MAX_THRESHOLD);
        Self {
            threshold: clamped,
            percent: scorer::format_threshold_percent(clamped),
            min: scorer::MIN_THRESHOLD,
            max: scorer::MAX_THRESHOLD,
            default: scorer::DEFAULT_MIN_SCORE,
        }
    }
}

/// 在线命令的失败：`kind` 供界面分支，`message` 可直接展示。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineCommandError {
    /// `emptyQuery` / `fallbackSource` / `primarySource` / `noFile` / `unknownSong` / `cancelled` / `failed`。
    pub kind: String,
    /// 中文说明。
    pub message: String,
}

impl OnlineCommandError {
    pub(crate) fn new(kind: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.to_owned(),
            message: message.into(),
        }
    }
}
