//! 酷狗 KRC 歌词的解密与解析。对应 C# `KrcLyricsParser`。
//!
//! KRC 文件 = 明文魔数 `krc1` + XOR 混淆的 zlib 流，解压后是 UTF-8 文本；文本行形如
//! `[行起始ms,行时长ms]<词偏移,词时长,0>词<…>词`，逐字词的标签在词文本之前，词时间是相对行起始的偏移。
//! 译文放在 `[language:<base64 JSON>]` 头里（`type == 1` 为逐行译文）。
//!
//! 网络搜索与取词（`mobilecdn` → `krcs` → `lyrics.kugou.com`）属于模块 10，这里只做纯文本的解密与解析。

use std::sync::LazyLock;

use base64::Engine;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use regex::Regex;
use serde_json::Value;

use super::models::{LyricDocument, LyricLine, LyricWord};
use super::zlib::inflate_lenient;

/// 固定的 16 字节 XOR 密钥。
pub const XOR_KEY: [u8; 16] = [
    0x40, 0x47, 0x61, 0x77, 0x5E, 0x32, 0x74, 0x47, 0x51, 0x36, 0x31, 0x2D, 0xCE, 0xD2, 0x6E, 0x69,
];

const MAGIC: &[u8; 4] = b"krc1";

static LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[([0-9]+),([0-9]+)\](.*)$").expect("KRC 行正则合法"));

static WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<([0-9]+),([0-9]+),-?[0-9]+>").expect("KRC 词标签正则合法"));

static LANGUAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\[language:([A-Za-z0-9+/=]*)\]$").expect("KRC language 正则合法")
});

/// 与 .NET `Convert.FromBase64String` 一致：要求规范填充，但不拒绝末位残留的非零比特。
const BASE64_LENIENT: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::RequireCanonical)
        .with_decode_allow_trailing_bits(true),
);

/// 把酷狗下载接口返回的 base64 内容解码为 KRC 明文；格式不符或解压失败返回 `None`。
pub fn decrypt_base64(value: Option<&str>) -> Option<String> {
    let value = value.filter(|text| !text.trim().is_empty())?;
    let raw = decode_base64(value.trim())?;
    decrypt(&raw)
}

/// 把加密后的 KRC 字节流解成明文歌词文本。
pub fn decrypt(encrypted: &[u8]) -> Option<String> {
    if encrypted.is_empty() {
        return None;
    }

    // 魔数是明文，不参与 XOR 与解压；没有魔数的老格式从头开始。
    let start = if encrypted.starts_with(MAGIC) {
        MAGIC.len()
    } else {
        0
    };
    if start >= encrypted.len() {
        return None;
    }

    let data: Vec<u8> = encrypted[start..]
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ XOR_KEY[index % XOR_KEY.len()])
        .collect();

    match inflate_lenient(&data) {
        Some(inflated) => Some(decode_text_with_bom(&inflated)),
        None => {
            // 少数接口直接返回明文 UTF-8（没有 zlib 头），按明文兜底。
            let text = String::from_utf8_lossy(&data).into_owned();
            text.contains('[').then_some(text)
        }
    }
}

/// 解析 KRC 明文为歌词文档（含逐字时间轴与逐行译文）。
pub fn parse(text: Option<&str>) -> LyricDocument {
    let Some(text) = text.filter(|value| !value.trim().is_empty()) else {
        return LyricDocument::default();
    };

    let translations = read_translations(text);
    let mut lines: Vec<LyricLine> = Vec::new();

    for raw_line in text.split('\n') {
        let line = raw_line.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        let Some(captures) = LINE.captures(line) else {
            continue;
        };

        let line_start = parse_int(&captures[1]);
        let line_duration = parse_int(&captures[2]);
        let content = &captures[3];

        let word_matches: Vec<_> = WORD.captures_iter(content).collect();
        let mut words = Vec::new();
        for (index, word_match) in word_matches.iter().enumerate() {
            let whole = word_match.get(0).expect("整体匹配必然存在");
            let text_start = whole.end();
            let text_end = word_matches
                .get(index + 1)
                .and_then(|next| next.get(0))
                .map_or(content.len(), |next| next.start());
            if text_end <= text_start {
                continue;
            }
            words.push(LyricWord {
                start_ms: line_start.wrapping_add(parse_int(&word_match[1])),
                duration_ms: parse_int(&word_match[2]),
                text: content[text_start..text_end].to_owned(),
            });
        }

        let line_text = if words.is_empty() {
            content.trim().to_owned()
        } else {
            words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<String>()
                .trim()
                .to_owned()
        };
        if line_text.is_empty() && words.is_empty() {
            continue;
        }

        let translation = translations
            .as_ref()
            .and_then(|all| all.get(lines.len()))
            .cloned()
            .flatten();

        lines.push(LyricLine {
            start_ms: line_start,
            duration_ms: line_duration,
            text: line_text,
            words,
            translation,
        });
    }

    LyricDocument {
        lines,
        ..LyricDocument::default()
    }
}

/// 读取 `[language:…]` 里的逐行译文（`type == 1`）；没有时返回 `None`。
fn read_translations(text: &str) -> Option<Vec<Option<String>>> {
    for raw_line in text.split('\n') {
        let line = raw_line.trim_end_matches('\r').trim();
        let Some(captures) = LANGUAGE.captures(line) else {
            continue;
        };

        let bytes = decode_base64(&captures[1])?;
        let json = String::from_utf8_lossy(&bytes);
        let document: Value = serde_json::from_str(&json).ok()?;
        let Some(Value::Array(content)) = document.get("content") else {
            return None;
        };

        for track in content {
            // 与 C# 的异常语义一致：条目不是对象，或 type 是数字却不是 32 位整数（如 1.5），
            // 都会让 TryGetProperty / GetInt32 抛异常，整体判为没有译文；type 不是数字则只跳过这一项。
            let object = track.as_object()?;
            let Some(Value::Number(kind)) = object.get("type") else {
                continue;
            };
            match kind.as_i64().and_then(|n| i32::try_from(n).ok()) {
                Some(1) => {}
                Some(_) => continue,
                None => return None,
            }

            let Some(Value::Array(entries)) = object.get("lyricContent") else {
                return None;
            };
            return Some(
                entries
                    .iter()
                    .map(|entry| {
                        let value = flatten_text(entry);
                        (!value.trim().is_empty()).then(|| value.trim().to_owned())
                    })
                    .collect(),
            );
        }
    }
    None
}

fn flatten_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(flatten_text).collect(),
        _ => String::new(),
    }
}

fn decode_base64(text: &str) -> Option<Vec<u8>> {
    BASE64_LENIENT.decode(text).ok()
}

/// 解码解压后的文本：与 .NET `StreamReader(detectEncodingFromByteOrderMarks: true)` 一样识别 UTF-8 / UTF-16 BOM。
fn decode_text_with_bom(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |rest: &[u8], little_endian: bool| {
        let (pairs, _) = rest.as_chunks::<2>();
        let units: Vec<u16> = pairs
            .iter()
            .map(|pair| {
                if little_endian {
                    u16::from_le_bytes(*pair)
                } else {
                    u16::from_be_bytes(*pair)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn parse_int(text: &str) -> i32 {
    text.parse().unwrap_or(0)
}

#[cfg(test)]
mod tests;
