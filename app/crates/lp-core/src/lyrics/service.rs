//! 歌词预览编排：取网易云歌词 → 解析 → 按“逐行 / 逐字优先”序列化成可直接显示的 LRC 文本。
//!
//! 与 C# 下载流程里的 `TryBuildLyricsAsync` 同一套规则：逐行取 `to_plain_lrc`，逐字取 `to_enhanced_lrc`，
//! 译文默认写入（紧跟在原文行之后，时间戳相同）。这里只是只读预览，不写入任何文件。
//!
//! 歌词接口匿名可用，所以**不要求登录**；只有接口业务 `code == 301` 且当前确实带着登录态时，
//! 才走 [`LoginManager::handle_runtime_auth_error`]（先续期再复核，仍未登录才清除）。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::parser;
use crate::auth::{LoginManager, RuntimeAuthOutcome};
use crate::logs::LogHub;
use crate::netease::NeteaseError;

/// 歌词来源名称（模块 10 接入 QQ / 酷狗 / LRCLIB 后会有别的值）。
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

/// 歌词预览服务。
pub struct LyricsService {
    login: Arc<LoginManager>,
    logs: Arc<LogHub>,
}

impl LyricsService {
    /// 创建服务；请求共用 `login` 持有的客户端与登录态。
    pub fn new(login: Arc<LoginManager>, logs: Arc<LogHub>) -> Self {
        Self { login, logs }
    }

    /// 取一首歌的歌词并按 `mode` 序列化（默认带翻译）。
    pub async fn get(&self, song_id: i64, mode: LyricsMode) -> Result<LyricsPreview, LyricsError> {
        if song_id <= 0 {
            return Err(LyricsError {
                kind: LyricsErrorKind::Failed,
                message: format!("{OPERATION}：歌曲 ID 无效"),
            });
        }

        let raw = match self.login.client().get_lyrics(song_id).await {
            Ok(raw) => raw,
            Err(error) => return Err(self.fail(song_id, error).await),
        };

        let preview = build_preview(song_id, mode, &raw);
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
