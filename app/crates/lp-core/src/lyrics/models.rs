//! 歌词领域模型。对应 C# `LyricModels.cs` 与 `NeteaseModels.cs` 里的 `RawLyrics`。
//!
//! 时间一律是毫秒（`i32`，与 C# 的 `int` 保持一致，边界行为也一致）。

/// 逐字歌词中的一个词 / 字。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricWord {
    /// 词起始（绝对毫秒）。
    pub start_ms: i32,
    /// 词时长；负值按 0 计。
    pub duration_ms: i32,
    /// 词文本（可带前后空格）。
    pub text: String,
}

impl LyricWord {
    /// 词结束时间 = 起始 + max(0, 时长)。
    pub fn end_ms(&self) -> i32 {
        self.start_ms.wrapping_add(self.duration_ms.max(0))
    }
}

/// 一行歌词。`words` 为空表示只有逐行时间轴。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricLine {
    /// 行起始（绝对毫秒）。
    pub start_ms: i32,
    /// 行时长。
    pub duration_ms: i32,
    /// 行文本。
    pub text: String,
    /// 逐字信息。
    pub words: Vec<LyricWord>,
    /// 合并进来的译文。
    pub translation: Option<String>,
}

impl LyricLine {
    /// 是否带逐字时间轴。
    pub fn has_word_timing(&self) -> bool {
        !self.words.is_empty()
    }

    /// 行结束时间 = 起始 + max(0, 时长)。
    pub fn end_ms(&self) -> i32 {
        self.start_ms.wrapping_add(self.duration_ms.max(0))
    }
}

/// 解析后的歌词文档。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LyricDocument {
    /// 按起始时间排好序的歌词行。
    pub lines: Vec<LyricLine>,
    /// 原始译文 LRC（非空白时才有）。
    pub translation: Option<String>,
    /// 原始罗马音 LRC（非空白时才有）。
    pub romanization: Option<String>,
}

impl LyricDocument {
    /// 任意一行带逐字时间轴。
    pub fn has_word_level(&self) -> bool {
        self.lines.iter().any(LyricLine::has_word_timing)
    }

    /// 没有任何歌词行。
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// 任意一行带译文。
    pub fn has_translation(&self) -> bool {
        self.lines.iter().any(|line| line.translation.is_some())
    }
}

/// 各来源返回的未解析歌词文本。
///
/// 逐字优先级：`qrc`（QQ 音乐）> `yrc`（网易云）> `lrc`（普通 / 增强 LRC）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawLyrics {
    /// 普通 / 增强 LRC。
    pub lrc: Option<String>,
    /// 译文 LRC。
    pub translated_lrc: Option<String>,
    /// 罗马音 LRC。
    pub romanized_lrc: Option<String>,
    /// 网易云 YRC 逐字歌词。
    pub yrc: Option<String>,
    /// QQ 音乐 QRC 逐字歌词（解密后的明文）。
    pub qrc: Option<String>,
}

impl RawLyrics {
    /// 只有一份 LRC 的歌词。
    pub fn from_lrc(lrc: impl Into<String>) -> Self {
        Self {
            lrc: Some(lrc.into()),
            ..Self::default()
        }
    }
}
