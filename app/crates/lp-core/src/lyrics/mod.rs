//! 歌词解析与匹配：LRC / 增强 LRC / YRC / QRC（3DES 变体）/ KRC，匹配打分与批量匹配。
//!
//! 对应 C# 的 `Core/Lyrics/`（`LyricParser`、`QrcDecryptor`、`QqTripleDes`、`KrcLyricsParser`、
//! `LyricsMatchScorer`、`BatchLyricsService`）。模块 5 只做解析层：
//!
//! - [`parser`]：LRC / 增强 LRC / YRC / QRC 文本的解析、译文合并，以及 `to_plain_lrc` / `to_enhanced_lrc` 序列化；
//! - [`qrc`]：QQ 音乐 QRC 的非标准 3DES 变体 + zlib 解密；
//! - [`krc`]：酷狗 KRC 的 XOR + zlib 解密与解析；
//! - [`service`]：把网易云歌词接口、解析与序列化串起来，给界面预览用。
//!
//! 在线搜索与取词（QQ / 酷狗 / LRCLIB）、`LyricsMatchScorer`、`BatchLyricsService` 属于模块 10。

pub mod html;
pub mod krc;
pub mod models;
pub mod parser;
pub mod qrc;
pub mod service;
mod zlib;

pub use models::{LyricDocument, LyricLine, LyricWord, RawLyrics};
pub use service::{
    LYRICS_SOURCE_LABEL, LyricsError, LyricsErrorKind, LyricsMode, LyricsPreview, LyricsService,
};
