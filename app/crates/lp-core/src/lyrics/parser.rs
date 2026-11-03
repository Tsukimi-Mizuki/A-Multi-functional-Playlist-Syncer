//! 普通 LRC / 增强 LRC / 网易云 YRC / QQ 音乐 QRC 的解析与序列化。对应 C# `LyricParser`。
//!
//! 支持：一行多个时间标签、`[mm:ss]` / `[mm:ss.xx]` / `[mm:ss.xxx]`、`[offset:±ms]` 整体偏移、
//! 增强 LRC 的 `<mm:ss.xx>` 词标签、YRC v0 的 `[行起始,行时长](词起始,词时长,flag)词` 格式、
//! 新版接口返回的 JSON 风格 YRC 行（`{"t":0,"c":[{"tx":"..."}]}`），以及 QRC 的
//! `[行起始,行时长]词(词起始,词时长)`。
//!
//! 翻译（`tlyric`）按最近时间戳逐行匹配到 [`LyricLine::translation`]；无法匹配的翻译行被忽略。
//! 逐字时间若小于行起始时间则按“相对行起始”处理，否则按绝对毫秒处理。
//!
//! 输出（尤其是 [`to_plain_lrc`] / [`to_enhanced_lrc`]）必须与 C# 版逐字节一致：写入标签的歌词两边要互相兼容。
//! 毫秒运算沿用 C# `int` 的回绕语义（`wrapping_*`），不会因极端输入 panic。

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::html::html_decode;
use super::models::{LyricDocument, LyricLine, LyricWord, RawLyrics};

/// 翻译行与原文行时间差在 800ms 内即视为同一行。
const TRANSLATION_TOLERANCE_MS: i32 = 800;

static TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([0-9]{1,4}):([0-9]{1,2})(?:[.:]([0-9]{1,3}))?$").expect("时间戳正则合法")
});

static OFFSET_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\[offset:\s*([+-]?[0-9]+)\s*\]").expect("offset 正则合法"));

/// YRC 与 QRC 的行头相同：`[行起始,行时长]`。
static LINE_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[([0-9]{1,10}),([0-9]{1,10})\]").expect("行头正则合法"));

static QRC_ENVELOPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<Lyric_1\b[^>]*?LyricContent="(.*?)"\s*/>"#).expect("QRC 外壳正则合法")
});

/// 解析过程中的中间词（词时间尚未归一化为绝对时间）。
#[derive(Debug, Clone)]
struct RawWord {
    start_ms: i32,
    duration_ms: Option<i32>,
    text: String,
}

/// 解析过程中的中间行。
#[derive(Debug, Clone)]
struct RawLine {
    start_ms: i32,
    duration_ms: Option<i32>,
    text: String,
    words: Vec<RawWord>,
}

// ===== 对外入口 =====

/// 解析歌词。逐字优先级：QRC > YRC > 普通 / 增强 LRC；随后合并译文。
pub fn parse(raw: &RawLyrics) -> LyricDocument {
    let qrc_lines = parse_qrc(raw.qrc.as_deref());
    let yrc_lines = if qrc_lines.is_empty() {
        parse_yrc(raw.yrc.as_deref())
    } else {
        qrc_lines
    };
    let primary = if yrc_lines.is_empty() {
        parse_lrc(raw.lrc.as_deref())
    } else {
        yrc_lines
    };
    let mut lines = build_lines(primary);

    let translation = normalize_raw(raw.translated_lrc.as_deref());
    if let Some(translated) = &translation
        && !lines.is_empty()
    {
        lines = merge_translation(lines, translated);
    }

    LyricDocument {
        lines,
        translation,
        romanization: normalize_raw(raw.romanized_lrc.as_deref()),
    }
}

/// 生成普通 LRC（译文行紧跟在原文行之后）。
pub fn to_plain_lrc(document: &LyricDocument, include_translation: bool) -> String {
    if document.is_empty() {
        return String::new();
    }

    let mut builder = String::new();
    for line in &document.lines {
        let timestamp = format_timestamp(line.start_ms);
        for part in split_text(Some(&line.text)) {
            append_line(&mut builder, &format!("{timestamp}{}", part.trim()));
        }
        append_translation(&mut builder, &timestamp, line, include_translation);
    }
    builder
}

/// 生成增强 LRC（行时间 + 词标签），用于把逐字歌词写入标签；没有逐字信息时退化为普通 LRC。
pub fn to_enhanced_lrc(document: &LyricDocument, include_translation: bool) -> String {
    if document.is_empty() {
        return String::new();
    }
    if !document.has_word_level() {
        return to_plain_lrc(document, include_translation);
    }

    let mut builder = String::new();
    for line in &document.lines {
        let timestamp = format_timestamp(line.start_ms);

        if line.has_word_timing() {
            let mut line_builder = timestamp.clone();
            let mut last_end_text = format_time(line.start_ms);
            let mut wrote_word = false;
            for word in &line.words {
                let text = sanitize_word_text(&word.text);
                if text.is_empty() {
                    continue;
                }

                // 与 LDDC 一致：只有词起始时间与上一个词的结束时间不同时才写起始标签，
                // 结束标签总是写出，播放器据此把高亮精确到字。
                let word_start = line.start_ms.max(word.start_ms);
                let start_text = format_time(word_start);
                if !wrote_word || start_text != last_end_text {
                    line_builder.push('<');
                    line_builder.push_str(&start_text);
                    line_builder.push('>');
                }

                line_builder.push_str(&text);
                let word_end = word_start.max(word.end_ms());
                last_end_text = format_time(word_end);
                line_builder.push('<');
                line_builder.push_str(&last_end_text);
                line_builder.push('>');
                wrote_word = true;
            }
            append_line(&mut builder, &line_builder);
        } else {
            for part in split_text(Some(&line.text)) {
                append_line(&mut builder, &format!("{timestamp}{}", part.trim()));
            }
        }

        append_translation(&mut builder, &timestamp, line, include_translation);
    }
    builder
}

fn append_translation(
    builder: &mut String,
    timestamp: &str,
    line: &LyricLine,
    include_translation: bool,
) {
    let Some(translation) = line.translation.as_deref() else {
        return;
    };
    if include_translation && !translation.trim().is_empty() {
        for part in split_text(Some(translation)) {
            append_line(builder, &format!("{timestamp}{}", part.trim()));
        }
    }
}

// ===== 行构建 =====

fn build_lines(raw: Vec<RawLine>) -> Vec<LyricLine> {
    if raw.is_empty() {
        return Vec::new();
    }

    // 稳定排序：同一时间戳保持源顺序。
    let mut ordered = raw;
    ordered.sort_by_key(|line| line.start_ms);

    let mut result = Vec::with_capacity(ordered.len());
    for (index, current) in ordered.iter().enumerate() {
        let start = current.start_ms.max(0);
        let next_start = ordered.get(index + 1).map(|next| next.start_ms.max(0));

        let mut line_duration = match current.duration_ms {
            Some(duration) if duration > 0 => duration,
            _ => match next_start {
                Some(next) if next > 0 && next > start => next - start,
                _ => 0,
            },
        };

        let words = normalize_words(&current.words, start, line_duration, next_start);

        if line_duration <= 0
            && let Some(last) = words.last()
        {
            line_duration = last.end_ms().wrapping_sub(start).max(0);
        }

        let text = if current.text.is_empty() {
            words.iter().map(|word| word.text.as_str()).collect()
        } else {
            current.text.clone()
        };

        result.push(LyricLine {
            start_ms: start,
            duration_ms: line_duration,
            text,
            words,
            translation: None,
        });
    }
    result
}

fn normalize_words(
    raw_words: &[RawWord],
    line_start: i32,
    line_duration: i32,
    next_line_start: Option<i32>,
) -> Vec<LyricWord> {
    if raw_words.is_empty() {
        return Vec::new();
    }

    let starts: Vec<i32> = raw_words
        .iter()
        .map(|word| {
            let mut start = word.start_ms;
            if start < line_start {
                // 相对时间（少数实现按“相对行起始”写入词标签）。
                start = line_start.wrapping_add(start.max(0));
            }
            line_start.max(start)
        })
        .collect();

    raw_words
        .iter()
        .enumerate()
        .map(|(index, raw_word)| {
            let duration = if let Some(explicit) = raw_word.duration_ms {
                explicit.max(0)
            } else if index + 1 < starts.len() {
                starts[index + 1].wrapping_sub(starts[index]).max(0)
            } else if line_duration > 0 {
                line_start
                    .wrapping_add(line_duration)
                    .wrapping_sub(starts[index])
                    .max(0)
            } else if let Some(next) = next_line_start.filter(|next| *next > starts[index]) {
                next - starts[index]
            } else {
                0
            };

            LyricWord {
                start_ms: starts[index],
                duration_ms: duration,
                text: raw_word.text.clone(),
            }
        })
        .collect()
}

// ===== 翻译合并 =====

fn merge_translation(lines: Vec<LyricLine>, translated_lrc: &str) -> Vec<LyricLine> {
    let translated = build_lines(parse_lrc(Some(translated_lrc)));
    if translated.is_empty() {
        return lines;
    }

    let mapping = match_by_closest_time(&lines, &translated);
    lines
        .into_iter()
        .zip(mapping)
        .map(|(mut line, target)| {
            if let Some(target) = target {
                let text = translated[target].text.trim();
                // QQ / 网易云用 // 表示该行没有翻译或为纯音乐，不写入双语歌词。
                if !text.is_empty() && !text.chars().all(|ch| ch == '/') {
                    line.translation = Some(text.to_owned());
                }
            }
            line
        })
        .collect()
}

/// 按时间差升序做全局贪心配对（LDDC 的 `find_closest_match` 思路）：
/// 先把所有“原文行 × 翻译行”按时间差排序，再依次占用，避免逐行最近邻互相抢走对方的翻译。
fn match_by_closest_time(origin: &[LyricLine], other: &[LyricLine]) -> Vec<Option<usize>> {
    let mut mapping = vec![None; origin.len()];
    let mut used = vec![false; other.len()];

    // (时间差, 原文下标, 译文下标)：元组排序正好是 距离 → 原文下标 → 译文下标。
    let mut pairs: Vec<(i32, usize, usize)> = Vec::new();
    for (index, line) in origin.iter().enumerate() {
        for (candidate, other_line) in other.iter().enumerate() {
            let distance = other_line
                .start_ms
                .wrapping_sub(line.start_ms)
                .wrapping_abs();
            if distance <= TRANSLATION_TOLERANCE_MS {
                pairs.push((distance, index, candidate));
            }
        }
    }
    pairs.sort_unstable();

    for (_, index, candidate) in pairs {
        if mapping[index].is_some() || used[candidate] {
            continue;
        }
        mapping[index] = Some(candidate);
        used[candidate] = true;
    }
    mapping
}

// ===== QRC =====

/// 解析 QQ 音乐 QRC 逐字歌词：行 `[行起始,行时长]`，词为“文本在前、`(词起始,词时长)` 在后”。
fn parse_qrc(qrc: Option<&str>) -> Vec<RawLine> {
    let mut result = Vec::new();
    let Some(qrc) = qrc.filter(|value| !value.trim().is_empty()) else {
        return result;
    };

    let text = unwrap_qrc_envelope(qrc);
    for physical_line in split_physical_lines(&text) {
        if physical_line.trim().is_empty() {
            continue;
        }

        if let Some(parsed) = try_parse_qrc_line(physical_line.trim()) {
            result.push(parsed);
            continue;
        }

        // 容错：QRC 里混入了普通 LRC 行或 [ti:] / [ar:] 之类的元数据标签。
        result.extend(parse_lrc_physical_line(physical_line, 0));
    }
    result
}

/// 去掉 QRC 的 `<Lyric_1 LyricContent="..."/>` 外壳并还原 XML 实体。
fn unwrap_qrc_envelope(value: &str) -> String {
    let trimmed = value.trim();
    if !trimmed.contains("<Lyric_1") {
        return trimmed.to_owned();
    }
    match QRC_ENVELOPE.captures(trimmed) {
        Some(captures) => html_decode(&captures[1]),
        None => trimmed.to_owned(),
    }
}

fn try_parse_qrc_line(line: &str) -> Option<RawLine> {
    let captures = LINE_HEADER.captures(line)?;
    let start_ms = parse_int(&captures[1])?;
    let duration_ms = parse_int(&captures[2]).unwrap_or(0);

    let rest = &line[captures[0].len()..];
    let words = parse_qrc_words(rest);
    let text = if words.is_empty() {
        strip_qrc_word_tags(rest)
    } else {
        words.iter().map(|word| word.text.as_str()).collect()
    };

    Some(RawLine {
        start_ms,
        duration_ms: Some(duration_ms),
        text,
        words,
    })
}

fn parse_qrc_words(rest: &str) -> Vec<RawWord> {
    let mut words = Vec::new();
    let mut index = 0;
    while index < rest.len() {
        let Some(open) = find_from(rest, '(', index) else {
            break;
        };

        // 部分 QRC 会在每个词前再写一次 [词起始,词时长]，直接跳过。
        if rest.as_bytes()[index] == b'['
            && let Some(bracket_close) = find_from(rest, ']', index + 1)
            && bracket_close > index
            && try_parse_qrc_pair(&rest[index + 1..bracket_close]).is_some()
        {
            index = bracket_close + 1;
        }

        let Some(close) = find_from(rest, ')', open + 1) else {
            break;
        };

        let Some((word_start, word_duration)) = try_parse_qrc_pair(&rest[open + 1..close]) else {
            index = close + 1;
            continue;
        };

        words.push(RawWord {
            start_ms: word_start,
            duration_ms: Some(word_duration.max(0)),
            text: rest.get(index..open).unwrap_or_default().to_owned(),
        });
        index = close + 1;
    }
    words
}

/// 把 QRC 行内残留的 `(起始,时长)` 与 `[起始,时长]` 标签去掉，只保留文本。
fn strip_qrc_word_tags(rest: &str) -> String {
    let mut builder = String::with_capacity(rest.len());
    let mut index = 0;
    while index < rest.len() {
        let current = rest.as_bytes()[index];
        if current == b'(' || current == b'[' {
            let closing = if current == b'(' { ')' } else { ']' };
            if let Some(close) = find_from(rest, closing, index + 1)
                && close > 0
                && try_parse_qrc_pair(&rest[index + 1..close]).is_some()
            {
                index = close + 1;
                continue;
            }
        }

        let ch = rest[index..].chars().next().unwrap_or_default();
        builder.push(ch);
        index += ch.len_utf8();
    }
    builder
}

fn try_parse_qrc_pair(content: &str) -> Option<(i32, i32)> {
    let comma = content.find(',')?;
    if comma == 0 {
        return None;
    }
    let start = parse_int(content[..comma].trim())?;
    let duration = parse_int(content[comma + 1..].trim())?;
    Some((start, duration))
}

// ===== YRC =====

/// 解析网易云 YRC（v0 明文行或 v1 JSON 风格行，二者可混合出现）。
fn parse_yrc(yrc: Option<&str>) -> Vec<RawLine> {
    let mut result = Vec::new();
    let Some(yrc) = yrc.filter(|value| !value.trim().is_empty()) else {
        return result;
    };

    for physical_line in split_physical_lines(yrc) {
        if physical_line.trim().is_empty() {
            continue;
        }

        let line = physical_line.trim();
        if line.starts_with('{') {
            if let Some(parsed) = try_parse_json_line(line) {
                result.push(parsed);
            }
            continue;
        }

        if let Some(parsed) = try_parse_yrc_line(line) {
            result.push(parsed);
            continue;
        }

        // 容错：YRC 字段里混入了普通 LRC 行。
        result.extend(parse_lrc_physical_line(physical_line, 0));
    }
    result
}

fn try_parse_yrc_line(line: &str) -> Option<RawLine> {
    let captures = LINE_HEADER.captures(line)?;
    let start_ms = parse_int(&captures[1])?;
    let duration_ms = parse_int(&captures[2]).unwrap_or(0);

    let rest = &line[captures[0].len()..];
    let mut words = Vec::new();
    let mut index = 0;
    while index < rest.len() {
        if rest.as_bytes()[index] != b'(' {
            index += 1;
            continue;
        }

        let Some(close) = find_from(rest, ')', index + 1) else {
            break;
        };

        let parts: Vec<&str> = rest[index + 1..close].split(',').collect();
        if parts.len() >= 2
            && let Some(word_start) = parse_int(parts[0].trim())
            && let Some(word_duration) = parse_int(parts[1].trim())
        {
            let text_start = close + 1;
            let next_tag = find_from(rest, '(', text_start);
            let word_text = match next_tag {
                Some(next) => &rest[text_start..next],
                None => &rest[text_start..],
            };
            words.push(RawWord {
                start_ms: word_start,
                duration_ms: Some(word_duration.max(0)),
                text: word_text.to_owned(),
            });
            index = next_tag.unwrap_or(rest.len());
            continue;
        }

        index = close + 1;
    }

    let text = if words.is_empty() {
        rest.trim().to_owned()
    } else {
        words.iter().map(|word| word.text.as_str()).collect()
    };

    Some(RawLine {
        start_ms,
        duration_ms: Some(duration_ms),
        text,
        words,
    })
}

/// 解析新版接口的 JSON 风格歌词行：`{"t":123,"c":[{"tx":"文本"}]}`。
fn try_parse_json_line(line: &str) -> Option<RawLine> {
    let root: Value = serde_json::from_str(line).ok()?;
    let object = root.as_object()?;
    let time = object.get("t")?;

    let start_ms = match time {
        Value::Number(number) => match number.as_i64().and_then(|n| i32::try_from(n).ok()) {
            Some(value) => value,
            None => number.as_f64().map_or(0, double_to_i32_like_dotnet8),
        },
        Value::String(text) => parse_int(text).unwrap_or(0),
        _ => 0,
    };

    let mut text = String::new();
    if let Some(Value::Array(items)) = object.get("c") {
        for item in items {
            if let Some(Value::String(fragment)) = item.as_object().and_then(|o| o.get("tx")) {
                text.push_str(fragment);
            }
        }
    }

    Some(RawLine {
        start_ms,
        duration_ms: None,
        text,
        words: Vec::new(),
    })
}

// ===== LRC =====

/// 解析普通 LRC 文本（含 `[offset]` 与增强 LRC 词标签）。
fn parse_lrc(lrc: Option<&str>) -> Vec<RawLine> {
    let mut result = Vec::new();
    let Some(lrc) = lrc.filter(|value| !value.trim().is_empty()) else {
        return result;
    };

    let offset_ms = parse_offset(lrc);
    for physical_line in split_physical_lines(lrc) {
        result.extend(parse_lrc_physical_line(physical_line, offset_ms));
    }
    result
}

fn parse_lrc_physical_line(physical_line: &str, offset_ms: i32) -> Vec<RawLine> {
    let mut result = Vec::new();
    if physical_line.trim().is_empty() {
        return result;
    }

    let line = physical_line.trim_start();
    if line.starts_with('{') {
        if let Some(parsed) = try_parse_json_line(line.trim()) {
            result.push(parsed);
        }
        return result;
    }

    let mut position = 0;
    let mut timestamps = Vec::new();
    while let Some(milliseconds) = try_read_lrc_timestamp(physical_line, &mut position, offset_ms) {
        timestamps.push(milliseconds);
    }

    // 元数据标签（[ti:]、[ar:]、[offset:] 等）直接忽略。
    if timestamps.is_empty() {
        return result;
    }

    let rest = &physical_line[position..];
    let words = parse_enhanced_words(rest);
    let text: String = if words.is_empty() {
        rest.trim().to_owned()
    } else {
        words.iter().map(|word| word.text.as_str()).collect()
    };

    for timestamp in timestamps {
        result.push(RawLine {
            start_ms: timestamp,
            duration_ms: None,
            text: text.clone(),
            words: words.clone(),
        });
    }
    result
}

/// 从 `position` 起读一个 `[mm:ss.xxx]` 标签（允许前导空白）；成功时推进 `position`。
fn try_read_lrc_timestamp(line: &str, position: &mut usize, offset_ms: i32) -> Option<i32> {
    let mut index = *position;
    while let Some(ch) = line[index..].chars().next() {
        if !ch.is_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }

    if line.as_bytes().get(index) != Some(&b'[') {
        return None;
    }
    let close = find_from(line, ']', index + 1)?;
    let total = parse_timestamp_total(&line[index + 1..close])?;

    *position = close + 1;
    let shifted = (total + i64::from(offset_ms)).clamp(0, i64::from(i32::MAX));
    Some(shifted as i32)
}

fn parse_enhanced_words(rest: &str) -> Vec<RawWord> {
    let mut words = Vec::new();
    let mut index = 0;
    while let Some(open) = find_from(rest, '<', index) {
        let Some(close) = find_from(rest, '>', open + 1) else {
            break;
        };

        let Some(absolute_ms) = parse_word_timestamp(&rest[open + 1..close]) else {
            index = close + 1;
            continue;
        };

        let text_start = close + 1;
        let next_tag = find_from(rest, '<', text_start);
        let text = match next_tag {
            Some(next) => &rest[text_start..next],
            None => &rest[text_start..],
        };
        words.push(RawWord {
            start_ms: absolute_ms,
            duration_ms: None,
            text: text.to_owned(),
        });

        match next_tag {
            Some(next) => index = next,
            None => break,
        }
    }
    words
}

fn parse_word_timestamp(content: &str) -> Option<i32> {
    let total = parse_timestamp_total(content.trim())?;
    i32::try_from(total).ok()
}

/// 把 `mm:ss[.xxx]` 解析成毫秒；格式不符返回 `None`。
fn parse_timestamp_total(content: &str) -> Option<i64> {
    let captures = TIMESTAMP.captures(content)?;
    let minutes: i64 = captures[1].parse().ok()?;
    let seconds: i64 = captures[2].parse().ok()?;
    let fraction = captures
        .get(3)
        .map_or(0, |m| parse_fraction_milliseconds(m.as_str()));
    Some(minutes * 60_000 + seconds * 1000 + fraction)
}

fn parse_fraction_milliseconds(fraction: &str) -> i64 {
    let Ok(value) = fraction.parse::<i64>() else {
        return 0;
    };
    match fraction.len() {
        1 => value * 100,
        2 => value * 10,
        _ => value,
    }
}

fn parse_offset(lrc: &str) -> i32 {
    OFFSET_TAG
        .captures(lrc)
        .and_then(|captures| parse_int(&captures[1]))
        .unwrap_or(0)
}

// ===== 通用小工具 =====

/// C# 的 `(int)double`：截断取整；超出范围时 .NET 8 / x64 得到 `int.MinValue`（.NET 9 起改为饱和，
/// 但 C# 版发布在 .NET 8 上，保持一致）。
fn double_to_i32_like_dotnet8(value: f64) -> i32 {
    if value.is_nan() || value >= 2_147_483_648.0 || value <= -2_147_483_649.0 {
        i32::MIN
    } else {
        value as i32
    }
}

/// 与 .NET `string.IndexOf(char, startIndex)` 一致：从 `from` 起找，返回相对整串的字节下标。
fn find_from(text: &str, needle: char, from: usize) -> Option<usize> {
    text.get(from..)?.find(needle).map(|offset| offset + from)
}

/// 对齐 `int.TryParse(NumberStyles.Integer)`：允许首尾空白与正负号，溢出失败。
fn parse_int(text: &str) -> Option<i32> {
    text.trim_matches(|ch| matches!(ch, '\t'..='\r' | ' '))
        .parse()
        .ok()
}

fn split_physical_lines(text: &str) -> Vec<&str> {
    // 与 C# 的 Replace("\r\n","\n").Replace('\r','\n').Split('\n') 等价：\r\n 与单独的 \r 都算换行。
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                lines.push(&text[start..index]);
                index += 1;
                start = index;
            }
            b'\r' => {
                lines.push(&text[start..index]);
                index += if bytes.get(index + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = index;
            }
            _ => index += 1,
        }
    }
    lines.push(&text[start..]);
    lines
}

fn split_text(text: Option<&str>) -> Vec<&str> {
    match text {
        None | Some("") => vec![""],
        Some(text) => split_physical_lines(text),
    }
}

fn sanitize_word_text(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

fn append_line(builder: &mut String, line: &str) {
    if !builder.is_empty() {
        builder.push('\n');
    }
    builder.push_str(line);
}

fn normalize_raw(value: Option<&str>) -> Option<String> {
    value
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
}

fn format_timestamp(milliseconds: i32) -> String {
    format!("[{}]", format_time(milliseconds))
}

/// `mm:ss.fff`：毫秒精度（3 位），与 LDDC 的 `lrc_ms_digit_count=3` 一致；
/// 毫秒精度能让原文与译文使用同一时间戳而不必人为顺延。
fn format_time(milliseconds: i32) -> String {
    let total = milliseconds.max(0);
    let total_seconds = total / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}.{:03}", total % 1000)
}

#[cfg(test)]
mod tests;
