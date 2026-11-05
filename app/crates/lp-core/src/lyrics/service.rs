//! 歌词预览编排：按设置里的「歌词来源」取歌词 → 解析 → 按“逐行 / 逐字优先”序列化成可直接显示的 LRC 文本。
//!
//! 网易云按歌曲 ID 精确取词，规则与 C# 下载流程里的 `TryBuildLyricsAsync` 相同：逐行取 `to_plain_lrc`，
//! 逐字取 `to_enhanced_lrc`，译文默认写入（紧跟在原文行之后，时间戳相同）。QQ 音乐按歌名 / 歌手 / 专辑 / 时长
//! 检索并打分（与批量获取同一套规则），主来源都没有歌词时再按备用词源开关问酷狗、LRCLIB。
//! 这里只是只读预览，不写入任何文件。
//!
//! 歌词接口匿名可用，所以**不要求登录**；只有接口业务 `code == 301` 且当前确实带着登录态时，
//! 才走 [`LoginManager::handle_runtime_auth_error`]（先续期再复核，仍未登录才清除）。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::batch::{BatchLyricsService, SongLookup, order_sources};
use super::parser;
use crate::auth::{LoginManager, RuntimeAuthOutcome};
use crate::library::text::is_blank;
use crate::logs::LogHub;
use crate::models::OnlineSourcePriority;
use crate::netease::NeteaseError;
use crate::online::{LyricsSourceCandidate, OnlineMusicSource, OnlineSongDetails};
use crate::settings::AppSettings;
use crate::utilities::CancelToken;

/// 网易云音乐的来源名称。
pub const LYRICS_SOURCE_LABEL: &str = "网易云音乐";

/// 预览的歌词形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LyricsMode {
    /// 逐行 LRC。
    Line,
    /// 逐字优先：有逐字歌词时输出增强 LRC，没有则回退逐行。
    Word,
}

/// 可直接显示的歌词预览。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsPreview {
    /// 歌曲 ID。
    pub song_id: i64,
    /// 请求的形式。
    pub mode: LyricsMode,
    /// 歌词文本（LRC / 增强 LRC）；没有歌词时为空串。
    pub text: String,
    /// `text` 是否是逐字（增强 LRC）格式。请求逐字但歌曲没有逐字数据时为 `false`（已回退逐行）。
    pub is_word_level: bool,
    /// 这首歌是否有逐字数据（不论请求的形式）。
    pub has_word_data: bool,
    /// `text` 是否带翻译行。
    pub has_translation: bool,
    /// 歌词行数（不含翻译行）。
    pub line_count: usize,
    /// 没有歌词（纯音乐、未收录或解析后为空）。
    pub is_empty: bool,
    /// 来源名称。
    pub source: String,
}

/// 获取失败的类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LyricsErrorKind {
    /// 接口说未登录，复核后确认已失效，登录态已清除。
    SessionExpired,
    /// 其他失败（网络、接口错误……），登录态保留。
    Failed,
}

/// 获取歌词失败，`message` 可以直接展示给用户。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsError {
    /// 失败类别。
    pub kind: LyricsErrorKind,
    /// 中文说明。
    pub message: String,
}

impl std::fmt::Display for LyricsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LyricsError {}

const OPERATION: &str = "获取歌词失败";

/// 预览哪一首歌：网易云按 `song_id` 精确取词，其他来源用歌名、歌手、专辑与时长检索。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LyricsLookup {
    /// 网易云歌曲 ID。
    pub song_id: i64,
    /// 歌名。
    pub title: String,
    /// 歌手（多个以 `/` 连接）。
    pub artist: String,
    /// 专辑。
    pub album: String,
    /// 时长（毫秒），未知为 0。
    pub duration_ms: i64,
}

/// 预览时跟随设置的取词规则。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LyricsPreviewOptions {
    /// 设置里的「歌词来源」。
    pub priority: OnlineSourcePriority,
    /// 在线匹配度阈值（检索其他来源时用）。
    pub min_score: f64,
}

impl LyricsPreviewOptions {
    /// 从当前设置读取（每次预览时读，保存设置后下一次预览就生效）。
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            priority: settings.lyrics_source_priority,
            min_score: settings.match_threshold,
        }
    }
}

/// 歌词预览服务。
pub struct LyricsService {
    login: Arc<LoginManager>,
    logs: Arc<LogHub>,
    /// QQ 音乐与备用词源的检索；没有接上时只能取网易云歌词。
    online: Option<Arc<BatchLyricsService>>,
}

impl LyricsService {
    /// 创建服务；请求共用 `login` 持有的客户端与登录态。
    pub fn new(login: Arc<LoginManager>, logs: Arc<LogHub>) -> Self {
        Self {
            login,
            logs,
            online: None,
        }
    }

    /// 接上 QQ 音乐与备用词源的检索，预览才能按设置里的「歌词来源」换源。
    pub fn with_online(mut self, online: Arc<BatchLyricsService>) -> Self {
        self.online = Some(online);
        self
    }

    /// 按设置里的「歌词来源」取一首歌的歌词并按 `mode` 序列化（默认带翻译）。
    ///
    /// 按来源次序逐个尝试，某个来源出错或没有歌词就问下一个，主来源都没有时再问备用词源。
    /// 网易云确认登录已失效时立即返回；所有来源都没取到歌词时，途中出过错就返回第一个错误（可重试），
    /// 否则返回空预览（`source` 是尝试过的主来源）。
    pub async fn preview(
        &self,
        lookup: &LyricsLookup,
        mode: LyricsMode,
        options: LyricsPreviewOptions,
    ) -> Result<LyricsPreview, LyricsError> {
        validate_song_id(lookup.song_id)?;

        let song = SongLookup {
            title: lookup.title.trim(),
            artist: lookup.artist.trim(),
            album: Some(lookup.album.trim()).filter(|album| !is_blank(album)),
            duration_ms: lookup.duration_ms,
        };
        // 没有歌名就无从检索其他来源，只能按 ID 问网易云。
        let online = self.online.as_ref().filter(|_| !is_blank(song.title));
        let prefer_word = mode == LyricsMode::Word;
        let cancel = CancelToken::new();
        let mut first_error: Option<LyricsError> = None;
        let mut tried: Vec<&str> = Vec::new();

        for source in order_sources(options.priority) {
            tried.push(source.label());
            match source {
                OnlineMusicSource::Netease => {
                    match self.fetch_netease(lookup.song_id, mode).await {
                        Ok(preview) if !preview.is_empty => return Ok(self.found(lookup, preview)),
                        Ok(_) => {}
                        Err(error) if error.kind == LyricsErrorKind::SessionExpired => {
                            return Err(error);
                        }
                        Err(error) => {
                            first_error.get_or_insert(error);
                        }
                    }
                }
                OnlineMusicSource::QqMusic => {
                    let Some(online) = online else { continue };
                    match online
                        .find_source_lyrics(&song, source, prefer_word, options.min_score, &cancel)
                        .await
                    {
                        Ok(Some(details)) => {
                            let preview = online_preview(
                                lookup.song_id,
                                mode,
                                source.label(),
                                &LyricsTexts::of_details(&details),
                            );
                            return Ok(self.found(lookup, preview));
                        }
                        Ok(None) => {}
                        Err(error) => self.logs.warning(format!(
                            "从{}获取「{}」的歌词失败：{error}",
                            source.label(),
                            song_name(lookup)
                        )),
                    }
                }
            }
        }

        if let Some(online) = online {
            match online
                .find_fallback_song_lyrics(&song, prefer_word, options.min_score, &cancel)
                .await
            {
                Ok(Some(candidate)) => {
                    let preview = online_preview(
                        lookup.song_id,
                        mode,
                        &candidate.source_label,
                        &LyricsTexts::of_candidate(&candidate),
                    );
                    return Ok(self.found(lookup, preview));
                }
                Ok(None) => {}
                Err(error) => self.logs.warning(format!(
                    "从备用词源获取「{}」的歌词失败：{error}",
                    song_name(lookup)
                )),
            }
        }

        if let Some(error) = first_error {
            return Err(error);
        }
        self.logs.info(format!(
            "「{}」在{}都没有可用的歌词（纯音乐或未收录）",
            song_name(lookup),
            tried.join("、")
        ));
        Ok(LyricsPreview {
            song_id: lookup.song_id,
            mode,
            text: String::new(),
            is_word_level: false,
            has_word_data: false,
            has_translation: false,
            line_count: 0,
            is_empty: true,
            source: tried.join("、"),
        })
    }

    /// 取到歌词后记一条日志。
    fn found(&self, lookup: &LyricsLookup, preview: LyricsPreview) -> LyricsPreview {
        self.logs.success(format!(
            "已从{}获取「{}」的歌词（{}{}）",
            preview.source,
            song_name(lookup),
            if preview.is_word_level {
                "逐字"
            } else {
                "逐行"
            },
            if preview.has_translation {
                "，含翻译"
            } else {
                ""
            }
        ));
        preview
    }

    /// 按歌曲 ID 取网易云歌词并序列化（不记成功日志）。
    async fn fetch_netease(
        &self,
        song_id: i64,
        mode: LyricsMode,
    ) -> Result<LyricsPreview, LyricsError> {
        match self.login.client().get_lyrics(song_id).await {
            Ok(raw) => Ok(build_preview(song_id, mode, &raw)),
            Err(error) => Err(self.fail(song_id, error).await),
        }
    }

    /// 只取网易云歌词（按歌曲 ID），不看设置里的来源。
    pub async fn get(&self, song_id: i64, mode: LyricsMode) -> Result<LyricsPreview, LyricsError> {
        validate_song_id(song_id)?;
        let preview = self.fetch_netease(song_id, mode).await?;
        if preview.is_empty {
            self.logs
                .info(format!("歌曲 {song_id} 没有可用的歌词（纯音乐或未收录）"));
        } else {
            self.logs.success(format!(
                "已获取歌曲 {song_id} 的歌词（{}{}）",
                if preview.is_word_level {
                    "逐字"
                } else {
                    "逐行"
                },
                if preview.has_translation {
                    "，含翻译"
                } else {
                    ""
                }
            ));
        }
        Ok(preview)
    }

    /// 记录错误日志并生成面向用户的错误；301 且带着登录态时先走登录复核。
    async fn fail(&self, song_id: i64, error: NeteaseError) -> LyricsError {
        let detail = error.user_message();
        self.logs
            .error(format!("{OPERATION}（歌曲 {song_id}）：{detail}"));

        let mut kept_after_recheck = false;
        if error.is_not_logged_in() && self.login.account().logged_in {
            match self.login.handle_runtime_auth_error().await {
                RuntimeAuthOutcome::LoggedOut => {
                    return LyricsError {
                        kind: LyricsErrorKind::SessionExpired,
                        message: "登录状态已失效，请重新扫码登录".to_owned(),
                    };
                }
                RuntimeAuthOutcome::Kept | RuntimeAuthOutcome::Skipped => kept_after_recheck = true,
            }
        }

        LyricsError {
            kind: LyricsErrorKind::Failed,
            message: if kept_after_recheck {
                format!("{OPERATION}：{detail}（已保留登录，可稍后重试）")
            } else {
                format!("{OPERATION}：{detail}")
            },
        }
    }
}

fn validate_song_id(song_id: i64) -> Result<(), LyricsError> {
    if song_id <= 0 {
        return Err(LyricsError {
            kind: LyricsErrorKind::Failed,
            message: format!("{OPERATION}：歌曲 ID 无效"),
        });
    }
    Ok(())
}

/// 日志里称呼这首歌：有歌名用歌名，否则用 ID。
fn song_name(lookup: &LyricsLookup) -> String {
    if is_blank(&lookup.title) {
        format!("歌曲 {}", lookup.song_id)
    } else {
        lookup.title.trim().to_owned()
    }
}

/// 在线来源的几种歌词写法（QQ 音乐详情与备用词源候选共用）。
#[derive(Debug, Clone, Copy, Default)]
pub struct LyricsTexts<'a> {
    /// 逐行 LRC。
    pub line: Option<&'a str>,
    /// 逐字（增强 LRC）。
    pub word: Option<&'a str>,
    /// 原文 + 译文的逐行 LRC。
    pub bilingual_line: Option<&'a str>,
    /// 原文 + 译文的逐字 LRC。
    pub bilingual_word: Option<&'a str>,
}

impl<'a> LyricsTexts<'a> {
    fn of_details(details: &'a OnlineSongDetails) -> Self {
        Self {
            line: details.line_lyrics.as_deref(),
            word: details.word_lyrics.as_deref(),
            bilingual_line: details.bilingual_line_lyrics.as_deref(),
            bilingual_word: details.bilingual_word_lyrics.as_deref(),
        }
    }

    fn of_candidate(candidate: &'a LyricsSourceCandidate) -> Self {
        Self {
            line: candidate.line_lyrics.as_deref(),
            word: candidate.word_lyrics.as_deref(),
            bilingual_line: candidate.bilingual_line_lyrics.as_deref(),
            bilingual_word: candidate.bilingual_word_lyrics.as_deref(),
        }
    }
}

/// 把在线来源已经生成好的歌词文本整理成预览（纯函数，不联网）。
///
/// 逐字优先且有逐字歌词时用逐字写法，否则用逐行；有双语版本时显示双语。QQ 音乐没有译文时
/// 「双语」版本与原文完全相同，这种情况不算有翻译。行数只数原文里带时间戳的歌词行。
pub fn online_preview(
    song_id: i64,
    mode: LyricsMode,
    source: &str,
    texts: &LyricsTexts<'_>,
) -> LyricsPreview {
    let filled = |text: Option<&str>| text.filter(|text| !is_blank(text)).map(str::to_owned);
    let has_word_data = filled(texts.word).is_some();
    let is_word_level = mode == LyricsMode::Word && has_word_data;
    let (original, bilingual) = if is_word_level {
        (filled(texts.word), filled(texts.bilingual_word))
    } else {
        (filled(texts.line), filled(texts.bilingual_line))
    };
    let translated = bilingual.filter(|bilingual| {
        original
            .as_deref()
            .is_none_or(|original| bilingual.trim() != original.trim())
    });
    let has_translation = translated.is_some() && original.is_some();
    let line_count = original
        .as_deref()
        .or(translated.as_deref())
        .map_or(0, count_timed_lines);
    let text = translated.or(original).unwrap_or_default();

    LyricsPreview {
        song_id,
        mode,
        is_empty: text.trim().is_empty(),
        is_word_level,
        has_word_data,
        has_translation,
        line_count,
        text,
        source: source.to_owned(),
    }
}

/// 数 LRC 文本里带时间戳、且有内容的行（`[ti:]` 之类的标签行不算）。
fn count_timed_lines(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            let mut rest = line.trim();
            let mut timed = false;
            while let Some(tail) = rest.strip_prefix('[') {
                let Some(end) = tail.find(']') else { break };
                if !tail.starts_with(|c: char| c.is_ascii_digit()) {
                    break;
                }
                timed = true;
                rest = tail[end + 1..].trim_start();
            }
            timed && !rest.is_empty()
        })
        .count()
}

/// 把原始歌词解析并序列化成预览（纯函数，不联网）。
pub fn build_preview(song_id: i64, mode: LyricsMode, raw: &super::RawLyrics) -> LyricsPreview {
    let document = parser::parse(raw);
    let has_word_data = document.has_word_level();
    let is_word_level = mode == LyricsMode::Word && has_word_data;
    let text = if is_word_level {
        parser::to_enhanced_lrc(&document, true)
    } else {
        parser::to_plain_lrc(&document, true)
    };

    LyricsPreview {
        song_id,
        mode,
        is_empty: document.is_empty() || text.trim().is_empty(),
        is_word_level,
        has_word_data,
        has_translation: document.has_translation(),
        line_count: document.lines.len(),
        text,
        source: LYRICS_SOURCE_LABEL.to_owned(),
    }
}

#[cfg(test)]
mod tests;
