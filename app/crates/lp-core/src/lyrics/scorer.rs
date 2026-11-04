//! 在线搜索结果与本地歌曲的匹配度打分。对应 C# `LyricsMatchScorer`。
//!
//! 思路参考「音樂標籤」（Android）：先做全角 / 大小写 / 括号后缀归一化，标题、歌手、专辑三路打分并支持多值切段，
//! 再按“标题 > 歌手 > 时长”的权重合成总分（歌手不同、版本标记不一致、伴奏版本会降权或直接排除），
//! 调用方用匹配度阈值筛选。
//!
//! **浮点结果必须与 C# 逐位一致**（有交叉验证测试）：所有乘加按 C# 同样的顺序求值；
//! 文本按 .NET 的 UTF-16 语义处理（`char.IsLetterOrDigit` 按码元判定，补充平面字符在归一化后等同分隔符）。
//!
//! 与 .NET 的已知差异（只影响极端输入）：`StringComparison.OrdinalIgnoreCase` 这里只折叠 ASCII 大小写
//! （连接词 `feat.` 等都是 ASCII，非 ASCII 字符的大小写折叠不会影响 `feat.` 的命中）。

use icu_normalizer::ComposingNormalizerBorrowed;
use serde::Serialize;

use crate::library::text::{
    is_blank, is_dotnet_letter_or_digit, is_dotnet_whitespace, to_lower_invariant,
};

/// 默认匹配度阈值（等价于「音樂標籤」的 0.6 分界）。
pub const DEFAULT_MIN_SCORE: f64 = 0.60;
/// 匹配度阈值允许的最小值。
pub const MIN_THRESHOLD: f64 = 0.30;
/// 匹配度阈值允许的最大值。
pub const MAX_THRESHOLD: f64 = 1.00;
/// 标题达到该分数算“好标题”。
pub const GOOD_TITLE_SCORE: f64 = 0.80;
/// 标题低于该分数算“差标题”。
pub const POOR_TITLE_SCORE: f64 = 0.50;
/// 歌手达到该分数算可信。
pub const RELIABLE_ARTIST_SCORE: f64 = 0.80;
/// 专辑达到该分数算相符。
pub const RELIABLE_ALBUM_SCORE: f64 = 0.80;

/// 标题里的并列分隔符（MusicTag 的 `[\/&,]` 思路，另外补上全角与中文写法）。
pub const TITLE_SEPARATORS: [char; 9] = ['/', '&', ',', '，', '、', ';', '；', '·', '|'];
/// 歌手里常见的多值分隔符。
pub const ARTIST_SEPARATORS: [char; 12] = [
    '/', '&', ',', '，', '、', ';', '；', '·', '・', '×', '|', '+',
];
/// 歌手字段里的连接词，与分隔符等价。
pub const ARTIST_KEYWORDS: [&str; 6] = ["feat.", "ft.", "featuring", "cv:", "vs.", "with"];

/// 伴奏 / 纯音乐标记：本地不是伴奏而候选是伴奏时直接排除。
const INSTRUMENTAL_KEYWORDS: [&str; 7] = [
    "instrumental",
    "off vocal",
    "offvocal",
    "karaoke",
    "伴奏",
    "纯音乐",
    "伴唱",
];

/// 版本标记：Live / Remix / 翻唱 等版本不一致时降权，避免现场版顶替录音室版。
const VERSION_KEYWORDS: [&str; 19] = [
    "live",
    "remix",
    "acoustic",
    "demo",
    "karaoke",
    "cover",
    "edit",
    "unplugged",
    "instrumental",
    "tv size",
    "tv ver",
    "现场",
    "演唱会",
    "翻唱",
    "伴奏",
    "纯音乐",
    "伴唱",
    "片段",
    "剧场版",
];

/// 多值字段切分时用来替换连接词的内部标记（与 C# 的 `\u0001` 相同）。
const MULTI_VALUE_MARKER: char = '\u{1}';

/// 匹配度打分：标题、歌手、专辑、时长四路分数与加权总分。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsMatchScore {
    /// 标题相似度（0-1）。
    pub title: f64,
    /// 歌手相似度（0-1）。
    pub artist: f64,
    /// 专辑相似度（0-1）。
    pub album: f64,
    /// 时长相似度（0-1）。
    pub duration: f64,
    /// 加权总分（0-1），用于和匹配度阈值比较。
    pub total: f64,
    /// 是否直接排除（例如本地不是伴奏而候选是伴奏）。
    pub rejected: bool,
    /// 排除原因，便于日志与界面提示。
    pub reject_reason: Option<String>,
}

impl LyricsMatchScore {
    /// 标题与歌手都达到可信阈值，可以认为匹配非常确定。
    pub fn is_confident(&self) -> bool {
        !self.rejected && self.title >= GOOD_TITLE_SCORE && self.artist >= RELIABLE_ARTIST_SCORE
    }
}

// ───────────────────────────── 阈值文本 ─────────────────────────────

/// `Math.Round(value, 2)`：先放大 100 倍按“银行家舍入”取整再缩回（与 .NET 一致）。
fn round_two_digits(value: f64) -> f64 {
    (value * 100.0).round_ties_even() / 100.0
}

/// 把阈值格式化成设置界面里显示的百分数文本（0.60 →「60」）。
pub fn format_threshold_percent(threshold: f64) -> String {
    if threshold.is_nan() {
        return "NaN".to_owned();
    }
    let clamped = round_two_digits(threshold).clamp(MIN_THRESHOLD, MAX_THRESHOLD);
    // .NET 的 `ToString("0")` 对中点向远离零的方向舍入，与 `f64::round` 一致。
    format!("{}", (clamped * 100.0).round() as i64)
}

/// 解析设置界面输入的匹配度：接受「60」「60%」「0.6」等写法，换算成 0.30–1.00 的阈值；
/// 无法解析或超出范围时返回 `None`，由界面提示用户重填。
pub fn try_parse_threshold(text: Option<&str>) -> Option<f64> {
    let text = text.filter(|text| !is_blank(text))?;
    let trimmed = trim_dotnet(text).trim_end_matches(['%', '％']).to_owned();
    let trimmed = trim_dotnet(&trimmed);
    if trimmed.is_empty() {
        return None;
    }

    let value: f64 = trimmed.parse().ok()?;
    // 大于 1 按百分数理解（60 → 0.60），小于等于 1 按小数理解（0.6 → 0.60）。
    let normalized = if value > 1.0 { value / 100.0 } else { value };
    if normalized.is_nan() || !(MIN_THRESHOLD..=MAX_THRESHOLD).contains(&normalized) {
        return None;
    }
    Some(round_two_digits(normalized))
}

// ───────────────────────────── 文本工具 ─────────────────────────────

/// `string.Trim()`（按 `char.IsWhiteSpace`）。
fn trim_dotnet(value: &str) -> &str {
    value.trim_matches(is_dotnet_whitespace)
}

/// UTF-16 长度（.NET 的 `string.Length`）。
fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

/// 在 `haystack` 里从字节位置 `from` 起查找 ASCII `needle`（只折叠 ASCII 大小写），返回起始字节位置。
/// ASCII 针不会匹配到 UTF-8 多字节序列的中间，所以返回位置一定在字符边界上。
fn find_ascii_ignore_case(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    let hay = haystack.as_bytes();
    let pat = needle.as_bytes();
    if pat.is_empty() || hay.len() < pat.len() {
        return None;
    }
    (from..=hay.len() - pat.len())
        .find(|&start| hay[start..start + pat.len()].eq_ignore_ascii_case(pat))
}

/// `string.Replace(keyword, replacement, StringComparison.OrdinalIgnoreCase)`（ASCII 关键词）。
fn replace_ascii_ignore_case(value: &str, keyword: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut cursor = 0;
    while let Some(index) = find_ascii_ignore_case(value, keyword, cursor) {
        result.push_str(&value[cursor..index]);
        result.push_str(replacement);
        cursor = index + keyword.len();
    }
    result.push_str(&value[cursor..]);
    result
}

/// 把文本归一化：全角转半角、转小写、标点转空格、压缩空白。
///
/// 只保留字母与十进制数字（按 UTF-16 码元判定，补充平面字符一律当分隔符），词与词之间最多一个空格，
/// 开头不留空格。
pub fn normalize_text(value: Option<&str>) -> String {
    let Some(value) = value.filter(|value| !is_blank(value)) else {
        return String::new();
    };

    let form = ComposingNormalizerBorrowed::new_nfkc().normalize(value);
    let mut builder = String::with_capacity(form.len());
    let mut pending_space = false;
    for ch in form.chars() {
        // 补充平面字符在 .NET 里是两个代理码元，`char.IsLetterOrDigit` 对它们为 false。
        let lowered = if (ch as u32) > 0xFFFF {
            ch
        } else {
            to_lower_invariant(ch)
        };
        if (lowered as u32) <= 0xFFFF && is_dotnet_letter_or_digit(lowered) {
            if pending_space && !builder.is_empty() {
                builder.push(' ');
            }
            pending_space = false;
            builder.push(lowered);
        } else {
            pending_space = true;
        }
    }
    builder
}

/// 去掉所有空白与标点后的紧凑文本，用于“完全相同”这类判断。
pub fn compact_text(value: Option<&str>) -> String {
    normalize_text(value).replace(' ', "")
}

/// 去掉括号里的修饰（`(Live)`、`(Remastered)`、`【电视剧片尾曲】` 等），保留标题主体。
pub fn strip_brackets(value: Option<&str>) -> String {
    let Some(value) = value.filter(|value| !is_blank(value)) else {
        return String::new();
    };

    let mut builder = String::with_capacity(value.len());
    let mut depth = 0usize;
    for ch in value.chars() {
        if matches!(
            ch,
            '(' | '[' | '{' | '（' | '【' | '〈' | '《' | '「' | '『' | '〔'
        ) {
            depth += 1;
            continue;
        }
        if matches!(
            ch,
            ')' | ']' | '}' | '）' | '】' | '〉' | '》' | '」' | '』' | '〕'
        ) {
            depth = depth.saturating_sub(1);
            continue;
        }
        if depth == 0 {
            builder.push(ch);
        }
    }
    builder
}

/// 标题主体（去掉括号修饰后的归一化文本）。
pub fn core_title(value: Option<&str>) -> String {
    normalize_text(Some(&strip_brackets(value)))
}

/// 判断标题是否是伴奏 / 纯音乐版本。
pub fn is_instrumental(title: Option<&str>) -> bool {
    let text = normalize_text(title);
    if text.is_empty() {
        return false;
    }
    INSTRUMENTAL_KEYWORDS
        .iter()
        .any(|keyword| text.contains(keyword))
}

/// 把空白折叠成单个空格并去掉首尾空白（`Regex.Replace(value.Trim(), @"\s+", " ")`）。
fn collapse(value: Option<&str>) -> String {
    let Some(value) = value.filter(|value| !is_blank(value)) else {
        return String::new();
    };
    let mut result = String::with_capacity(value.len());
    let mut in_space = false;
    for ch in trim_dotnet(value).chars() {
        if is_dotnet_whitespace(ch) {
            if !in_space {
                result.push(' ');
            }
            in_space = true;
        } else {
            in_space = false;
            result.push(ch);
        }
    }
    result
}

/// `StringComparer.OrdinalIgnoreCase` 的相等（ASCII 折叠 + 单字符大写映射）。
fn equals_ordinal_ignore_case(left: &str, right: &str) -> bool {
    fn upper(ch: char) -> char {
        let mut mapped = ch.to_uppercase();
        match (mapped.next(), mapped.next()) {
            (Some(single), None) => single,
            _ => ch,
        }
    }
    left.chars().count() == right.chars().count()
        && left
            .chars()
            .zip(right.chars())
            .all(|(a, b)| a == b || upper(a) == upper(b))
}

/// 去掉标题里 `feat.` / `with` 等连接词及其后面的部分（连接词必须不在开头）。
fn remove_featuring(value: &str) -> &str {
    for keyword in ARTIST_KEYWORDS {
        if let Some(index) = find_ascii_ignore_case(value, keyword, 0)
            && index > 0
        {
            return &value[..index];
        }
    }
    value
}

/// 生成本地歌曲的多组搜索关键词（「音樂標籤」的多变体思路）：
/// “标题 歌手” → “标题” → “歌手 标题” → “标题主体 歌手” → 去掉 feat. 尾巴 → 并列标题的第一段。
/// 调用方按顺序尝试，命中可信匹配就不再继续换关键词。
pub fn build_queries(title: Option<&str>, artist: Option<&str>) -> Vec<String> {
    let mut queries: Vec<String> = Vec::new();
    let raw_title = collapse(title);
    let raw_artist = collapse(artist);
    if raw_title.is_empty() {
        return queries;
    }

    let core = collapse(Some(&strip_brackets(Some(&raw_title))));
    let no_feat = collapse(Some(remove_featuring(&raw_title)));

    let mut add = |query: &str| {
        let value = collapse(Some(query));
        if !value.is_empty()
            && !queries
                .iter()
                .any(|existing| equals_ordinal_ignore_case(existing, &value))
        {
            queries.push(value);
        }
    };

    if !raw_artist.is_empty() {
        add(&format!("{raw_title} {raw_artist}"));
    }

    add(&raw_title);

    if !raw_artist.is_empty() {
        add(&format!("{raw_artist} {raw_title}"));
    }

    if !core.is_empty() && core != raw_title {
        if raw_artist.is_empty() {
            add(&core);
        } else {
            add(&format!("{core} {raw_artist}"));
        }
    }

    if !no_feat.is_empty() && no_feat != raw_title {
        if raw_artist.is_empty() {
            add(&no_feat);
        } else {
            add(&format!("{no_feat} {raw_artist}"));
        }
    }

    // “A / B” 这类并列标题：整串往往搜不到，退化成第一段再试一次。
    let segments = split_multi_value(Some(&raw_title), &TITLE_SEPARATORS, &[]);
    if segments.len() > 1 {
        let first = trim_dotnet(&segments[0]);
        if !first.is_empty() {
            if raw_artist.is_empty() {
                add(first);
            } else {
                add(&format!("{first} {raw_artist}"));
            }
        }
    }

    queries
}

/// 时长是否相容：备用词源（酷狗 / LRCLIB）的硬性过滤，避免同名不同时长版本（现场版、剪辑版）把歌词写错。
/// 时长未知时放行。
pub fn duration_matches(local_ms: i64, result_ms: i64, tolerance_ms: i64) -> bool {
    if local_ms <= 0 || result_ms <= 0 {
        return true;
    }
    (local_ms - result_ms).abs() <= tolerance_ms
}

/// [`duration_matches`] 的默认容差（8 秒）。
pub const DEFAULT_DURATION_TOLERANCE_MS: i64 = 8000;

// ───────────────────────────── 打分 ─────────────────────────────

/// 计算本地歌曲与一条在线搜索结果的匹配度。
#[allow(clippy::too_many_arguments)]
pub fn score(
    local_title: Option<&str>,
    local_artist: Option<&str>,
    local_album: Option<&str>,
    local_duration_ms: i64,
    result_title: Option<&str>,
    result_artist: Option<&str>,
    result_album: Option<&str>,
    result_duration_ms: i64,
) -> LyricsMatchScore {
    let title = title_score(local_title, result_title);
    let artist = artist_score(local_artist, result_artist);
    let album = album_score(local_album, result_album);
    let duration = duration_score(local_duration_ms, result_duration_ms);

    let has_artist =
        local_artist.is_some_and(|a| !is_blank(a)) && result_artist.is_some_and(|a| !is_blank(a));
    let mut total = if has_artist {
        title * 0.55 + artist * 0.25 + duration * 0.20
    } else {
        title * 0.72 + duration * 0.20 + album * 0.08
    };

    // 专辑只是轻量信号：同专辑多版本、合辑很常见，不能因为专辑不符就直接判否。
    let has_local_album = local_album.is_some_and(|a| !is_blank(a));
    let has_result_album = result_album.is_some_and(|a| !is_blank(a));
    if has_local_album && has_result_album && album < RELIABLE_ALBUM_SCORE {
        total *= 0.97;
    }

    // 版本标记不一致时降权（本地没有 Live，候选是 Live；或反过来）。
    let local_versions = version_tags(local_title);
    let result_versions = version_tags(result_title);
    let extra = result_versions
        .iter()
        .filter(|tag| !local_versions.contains(tag))
        .count();
    let missing = local_versions
        .iter()
        .filter(|tag| !result_versions.contains(tag))
        .count();
    for _ in 0..extra {
        total *= 0.86;
    }
    for _ in 0..missing {
        total *= 0.94;
    }

    // 歌手完全不同：多半是别人的同名歌曲（翻唱、同名不同曲），显著降权，避免把标签写错。
    if has_artist && artist < 0.30 {
        total *= 0.75;
    }

    total = total.clamp(0.0, 1.0);

    let rejected = is_instrumental(result_title) && !is_instrumental(local_title);
    LyricsMatchScore {
        title,
        artist,
        album,
        duration,
        total,
        rejected,
        reject_reason: rejected.then(|| "候选是伴奏/纯音乐版本".to_owned()),
    }
}

/// 标题相似度：完整标题、标题主体、并列分段各算一次，取最佳。
pub fn title_score(local_title: Option<&str>, result_title: Option<&str>) -> f64 {
    let a = normalize_text(local_title);
    let b = normalize_text(result_title);
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let mut best = compare(&a, &b);

    let a_core = core_title(local_title);
    let b_core = core_title(result_title);
    if !a_core.is_empty() && !b_core.is_empty() {
        best = best.max(compare(&a_core, &b_core));
    }

    if utf16_len(&a_core) >= 2 && !b.is_empty() {
        best = best.max(compare(&a_core, &b) * 0.98);
    }

    if utf16_len(&b_core) >= 2 && !a.is_empty() {
        best = best.max(compare(&a, &b_core) * 0.98);
    }

    // “A / B” 这类并列标题：切段后逐段比较，命中某一段时轻微降权。
    for segment in split_multi_value(local_title, &TITLE_SEPARATORS, &[]) {
        let normalized = normalize_text(Some(&segment));
        if utf16_len(&normalized) < 2 {
            continue;
        }

        best = best.max(compare(&normalized, &b) * 0.97);
        if utf16_len(&b_core) >= 2 {
            best = best.max(compare(&normalized, &b_core) * 0.96);
        }
    }

    best.clamp(0.0, 1.0)
}

/// 歌手相似度：整体相似度 + 多值切段两两比较（“A/B”“A feat. B”“A CV:B”）。
pub fn artist_score(local_artist: Option<&str>, result_artist: Option<&str>) -> f64 {
    let a = normalize_text(local_artist);
    let b = normalize_text(result_artist);
    if a.is_empty() || b.is_empty() {
        return 0.5;
    }

    let mut best = compare(&a, &b);
    let right_parts: Vec<String> =
        split_multi_value(result_artist, &ARTIST_SEPARATORS, &ARTIST_KEYWORDS)
            .iter()
            .map(|part| normalize_text(Some(part)))
            .collect();
    for left in split_multi_value(local_artist, &ARTIST_SEPARATORS, &ARTIST_KEYWORDS) {
        let l = normalize_text(Some(&left));
        if l.is_empty() {
            continue;
        }

        for r in &right_parts {
            if r.is_empty() {
                continue;
            }

            let score = if l == *r { 0.98 } else { compare(&l, r) * 0.97 };
            if score > best {
                best = score;
            }
        }
    }

    best.clamp(0.0, 1.0)
}

/// 专辑相似度（为空时返回中性值 0.5）。
pub fn album_score(local_album: Option<&str>, result_album: Option<&str>) -> f64 {
    let a = normalize_text(local_album);
    let b = normalize_text(result_album);
    if a.is_empty() || b.is_empty() {
        return 0.5;
    }
    compare(&a, &b).clamp(0.0, 1.0)
}

/// 时长相似度：完全一致最好，相差越大越低（超过 40 秒视为不相关）。
pub fn duration_score(local_ms: i64, result_ms: i64) -> f64 {
    if local_ms <= 0 || result_ms <= 0 {
        return 0.5;
    }

    match (local_ms - result_ms).abs() {
        0..=2000 => 1.0,
        2001..=5000 => 0.90,
        5001..=10000 => 0.75,
        10001..=20000 => 0.45,
        20001..=40000 => 0.15,
        _ => 0.0,
    }
}

/// 把 “A/B”“A & B”“A feat. B” 之类的多值字段切成若干片段；没有分隔符时返回空列表。
pub fn split_multi_value(
    value: Option<&str>,
    separators: &[char],
    keywords: &[&str],
) -> Vec<String> {
    let Some(value) = value.filter(|value| !is_blank(value)) else {
        return Vec::new();
    };

    let has_separator = value.contains(|ch| separators.contains(&ch))
        || keywords
            .iter()
            .any(|keyword| find_ascii_ignore_case(value, keyword, 0).is_some());
    if !has_separator {
        return Vec::new();
    }

    let marker = MULTI_VALUE_MARKER.to_string();
    let mut working = value.to_owned();
    for keyword in keywords {
        working = replace_ascii_ignore_case(&working, keyword, &marker);
    }

    working
        .split(|ch| separators.contains(&ch) || ch == MULTI_VALUE_MARKER)
        .map(trim_dotnet)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 标题里出现的版本标记（按 [`VERSION_KEYWORDS`] 的顺序）。
fn version_tags(title: Option<&str>) -> Vec<&'static str> {
    let text = normalize_text(title);
    if text.is_empty() {
        return Vec::new();
    }

    VERSION_KEYWORDS
        .iter()
        .copied()
        .filter(|keyword| {
            // 英文标记按整词匹配，避免 "cover" 命中 "discover"、"live" 命中 "delivery"；
            // 中文标记没有词边界，直接用包含判断。
            if keyword.is_ascii() {
                contains_whole_word(&text, keyword)
            } else {
                text.contains(keyword)
            }
        })
        .collect()
}

/// 归一化文本里只有字母 / 数字与单个空格，所以“词边界”就是文本两端或空格。
fn contains_whole_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(start, _)| {
        let before_ok = text[..start].chars().next_back().is_none_or(|ch| ch == ' ');
        let after_ok = text[start + word.len()..]
            .chars()
            .next()
            .is_none_or(|ch| ch == ' ');
        before_ok && after_ok
    })
}

/// 两段归一化文本的相似度：编辑距离 + 包含关系 + 词序颠倒救援。
fn compare(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let mut best = similarity(a, b);

    let (a_len, b_len) = (utf16_len(a), utf16_len(b));
    if a_len >= 3 && b_len >= 3 && (a.contains(b) || b.contains(a)) {
        let ratio = a_len.min(b_len) as f64 / a_len.max(b_len) as f64;
        best = best.max(0.88 + 0.09 * ratio);
    }

    let a_tokens: Vec<&str> = a.split(' ').filter(|token| !token.is_empty()).collect();
    let b_tokens: Vec<&str> = b.split(' ').filter(|token| !token.is_empty()).collect();
    if a_tokens.len() > 1 && a_tokens.len() == b_tokens.len() {
        let reversed: Vec<&str> = a_tokens.iter().rev().copied().collect();
        if reversed.join(" ") == b_tokens.join(" ") {
            best = best.max(0.95);
        }
    }

    best.clamp(0.0, 1.0)
}

/// 1 - 编辑距离 / 较长长度（按 UTF-16 码元的 Levenshtein）。
fn similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }

    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    let max = a.len().max(b.len());
    if max == 0 {
        return 1.0;
    }

    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        current[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            current[j] = (current[j - 1] + 1)
                .min(previous[j] + 1)
                .min(previous[j - 1] + cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    1.0 - previous[b.len()] as f64 / max as f64
}

#[cfg(test)]
mod tests;
