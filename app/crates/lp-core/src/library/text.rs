//! 匹配用的文本归一化与相似度，逐项对应 C# `SongMatcher` 里的静态方法。
//!
//! .NET 的字符串是 UTF-16，`char.IsPunctuation` 等判定按**码元**做：补充平面字符（emoji 等）会被拆成两个代理码元，
//! 它们的类别是 `Surrogate`，既不是标点也不是符号，所以 **C# 会保留 emoji**。为了得到与 C# 完全相同的分数，
//! 这里同样用 `Vec<u16>` 表示归一化后的文本，长度、包含关系、字符二元组都按码元计算。
//!
//! 需要与 .NET 对齐的三处平台能力：
//!
//! - `string.Normalize(NormalizationForm.FormKC)` → ICU4X 的 NFKC；
//! - `char.IsPunctuation` / `IsSymbol` / `IsControl` / `IsWhiteSpace` → Unicode 通用类别（ICU4X 内置数据）；
//! - `char.ToLowerInvariant` → 单字符小写映射（一对多的映射 .NET 保持原字符不动）。
//!
//! 两边的 Unicode 版本可能不同（.NET 8 是 Unicode 15.0，这里更新），只会影响新版本才分配的字符。

use icu_normalizer::ComposingNormalizerBorrowed;
use icu_properties::CodePointMapData;
use icu_properties::props::GeneralCategory;

const OPEN_BRACKETS: [u16; 10] = [
    '(' as u16, '[' as u16, '{' as u16, 0x3010, 0x300C, 0x300E, 0x300A, 0x3008, 0x3014, 0xFF08,
];
const CLOSE_BRACKETS: [u16; 10] = [
    ')' as u16, ']' as u16, '}' as u16, 0x3011, 0x300D, 0x300F, 0x300B, 0x3009, 0x3015, 0xFF09,
];

/// 歌手名的分隔符：`/ & 、 ; ； , ，`。
const ARTIST_SEPARATORS: [char; 7] = ['/', '&', '、', ';', '；', ',', '，'];

fn category(c: char) -> GeneralCategory {
    CodePointMapData::<GeneralCategory>::new().get(c)
}

/// 与 `char.IsWhiteSpace` 一致：Latin-1 范围按固定集合，其余按 Zs / Zl / Zp。
pub fn is_dotnet_whitespace(c: char) -> bool {
    let code = c as u32;
    if code < 0x100 {
        return matches!(code, 0x09..=0x0D | 0x20 | 0x85 | 0xA0);
    }
    matches!(
        category(c),
        GeneralCategory::SpaceSeparator
            | GeneralCategory::LineSeparator
            | GeneralCategory::ParagraphSeparator
    )
}

/// 与 `char.IsLetterOrDigit` 一致：字母（Lu Ll Lt Lm Lo）或十进制数字（Nd）。
pub fn is_dotnet_letter_or_digit(c: char) -> bool {
    matches!(
        category(c),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
            | GeneralCategory::DecimalNumber
    )
}

/// 与 `char.IsControl` 一致（Cc）。
pub fn is_dotnet_control(c: char) -> bool {
    category(c) == GeneralCategory::Control
}

/// 与 `char.IsPunctuation` 一致（Pc Pd Ps Pe Pi Pf Po）。
pub fn is_dotnet_punctuation(c: char) -> bool {
    matches!(
        category(c),
        GeneralCategory::ConnectorPunctuation
            | GeneralCategory::DashPunctuation
            | GeneralCategory::OpenPunctuation
            | GeneralCategory::ClosePunctuation
            | GeneralCategory::InitialPunctuation
            | GeneralCategory::FinalPunctuation
            | GeneralCategory::OtherPunctuation
    )
}

/// 与 `char.IsSymbol` 一致（Sm Sc Sk So）。
pub fn is_dotnet_symbol(c: char) -> bool {
    matches!(
        category(c),
        GeneralCategory::MathSymbol
            | GeneralCategory::CurrencySymbol
            | GeneralCategory::ModifierSymbol
            | GeneralCategory::OtherSymbol
    )
}

/// 与 `char.ToLowerInvariant` 一致：只取一对一的小写映射，一对多（如 `İ`）保持原字符。
pub fn to_lower_invariant(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(single), None) => single,
        _ => c,
    }
}

/// `string.IsNullOrWhiteSpace`。
pub fn is_blank(value: &str) -> bool {
    value.chars().all(is_dotnet_whitespace)
}

/// 标题 / 歌手比较用的归一化：NFKC（全角 → 半角）、去括号内容、去标点符号与空白、小写。
/// 返回 UTF-16 码元序列（见模块文档）。
pub fn normalize_for_match(value: Option<&str>) -> Vec<u16> {
    let Some(value) = value.filter(|v| !is_blank(v)) else {
        return Vec::new();
    };

    let normalized = ComposingNormalizerBorrowed::new_nfkc().normalize(value);
    let mut builder: Vec<u16> = Vec::with_capacity(normalized.len());
    let mut bracket_depth = 0usize;

    for c in normalized.chars() {
        if (c as u32) > 0xFFFF {
            // 补充平面字符在 .NET 里是两个代理码元：不是括号、空白、标点或符号，也没有小写映射。
            if bracket_depth == 0 {
                let mut buffer = [0u16; 2];
                builder.extend_from_slice(c.encode_utf16(&mut buffer));
            }
            continue;
        }

        let unit = c as u16;
        if OPEN_BRACKETS.contains(&unit) {
            bracket_depth += 1;
            continue;
        }
        if CLOSE_BRACKETS.contains(&unit) {
            bracket_depth = bracket_depth.saturating_sub(1);
            continue;
        }
        if bracket_depth > 0 {
            continue;
        }
        if is_dotnet_whitespace(c)
            || is_dotnet_control(c)
            || is_dotnet_punctuation(c)
            || is_dotnet_symbol(c)
        {
            continue;
        }
        builder.push(to_lower_invariant(c) as u16);
    }

    builder
}

/// 归一化后的文本，附带预先算好的字符二元组（排序后的多重集合），热路径里不再重复分配。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MatchText {
    pub units: Vec<u16>,
    bigrams: Vec<u32>,
}

impl MatchText {
    pub fn from_units(units: Vec<u16>) -> Self {
        let mut bigrams: Vec<u32> = units
            .windows(2)
            .map(|pair| (u32::from(pair[0]) << 16) | u32::from(pair[1]))
            .collect();
        bigrams.sort_unstable();
        Self { units, bigrams }
    }

    pub fn len(&self) -> usize {
        self.units.len()
    }

    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }
}

/// 字符串相似度：相等为 1；包含关系按长度比 0.85~1；其余用字符二元组 Dice 系数。
pub fn similarity(left: &MatchText, right: &MatchText) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    if left.units == right.units {
        return 1.0;
    }

    let (shorter, longer) = if left.len() <= right.len() {
        (left, right)
    } else {
        (right, left)
    };
    if contains(&longer.units, &shorter.units) {
        let ratio = shorter.len() as f64 / longer.len() as f64;
        return 0.85 + (0.15 * ratio);
    }

    dice_coefficient(left, right)
}

/// 码元序列的子序列（连续）判定，对应 `string.Contains(.., StringComparison.Ordinal)`。
fn contains(haystack: &[u16], needle: &[u16]) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn dice_coefficient(left: &MatchText, right: &MatchText) -> f64 {
    if left.len() < 2 || right.len() < 2 {
        return 0.0;
    }

    // 两个排序后的多重集合的交集大小，等价于 C# 里“字典计数 + 逐个扣减”。
    let (mut i, mut j, mut intersection) = (0usize, 0usize, 0usize);
    while i < left.bigrams.len() && j < right.bigrams.len() {
        match left.bigrams[i].cmp(&right.bigrams[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                intersection += 1;
                i += 1;
                j += 1;
            }
        }
    }

    2.0 * intersection as f64 / ((left.len() - 1) + (right.len() - 1)) as f64
}

/// 按 `/ & 、 ; ； , ，` 拆分歌手串并逐个归一化，返回去重后的集合（保持首次出现的顺序，顺序不影响结果）。
pub fn split_artist_tokens(artists: Option<&str>) -> Vec<MatchText> {
    let mut tokens: Vec<MatchText> = Vec::new();
    let Some(artists) = artists.filter(|a| !is_blank(a)) else {
        return tokens;
    };

    for part in artists.split(ARTIST_SEPARATORS) {
        let normalized = normalize_for_match(Some(part));
        if normalized.is_empty() {
            continue;
        }
        let text = MatchText::from_units(normalized);
        if !tokens.contains(&text) {
            tokens.push(text);
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> MatchText {
        MatchText::from_units(normalize_for_match(Some(value)))
    }

    fn string(units: &[u16]) -> String {
        String::from_utf16_lossy(units)
    }

    #[test]
    fn normalization_folds_width_case_punctuation_and_brackets() {
        assert_eq!(
            string(&normalize_for_match(Some("ＳＵＮＮＹ（现场版）"))),
            "sunny"
        );
        assert_eq!(
            string(&normalize_for_match(Some("Hello, World!"))),
            "helloworld"
        );
        assert_eq!(
            string(&normalize_for_match(Some("晴天 【Live】 [x] {y}"))),
            "晴天"
        );
        assert_eq!(string(&normalize_for_match(Some("a(b(c)d)e"))), "ae");
    }

    #[test]
    fn unbalanced_closing_brackets_are_ignored_and_open_ones_swallow_the_rest() {
        assert_eq!(string(&normalize_for_match(Some("a)b"))), "ab");
        assert_eq!(string(&normalize_for_match(Some("a(b"))), "a");
    }

    #[test]
    fn blank_or_missing_input_normalizes_to_nothing() {
        assert!(normalize_for_match(None).is_empty());
        assert!(normalize_for_match(Some("")).is_empty());
        assert!(normalize_for_match(Some(" \t\u{3000}")).is_empty());
    }

    #[test]
    fn supplementary_plane_characters_survive_like_surrogate_pairs_in_dotnet() {
        // .NET 按码元判定：emoji 的代理码元既不是符号也不是标点，会被保留。
        assert_eq!(string(&normalize_for_match(Some("歌🎵"))), "歌🎵");
        assert_eq!(normalize_for_match(Some("🎵")).len(), 2);
        // 括号里的 emoji 照样被吞掉。
        assert_eq!(string(&normalize_for_match(Some("歌(🎵)"))), "歌");
    }

    #[test]
    fn nfkc_expands_compatibility_characters() {
        assert_eq!(string(&normalize_for_match(Some("ﬁne"))), "fine");
        // ㈱ 展开成「(株)」，括号里的内容被吞掉。
        assert_eq!(string(&normalize_for_match(Some("㈱ABC"))), "abc");
        assert_eq!(string(&normalize_for_match(Some("ｶﾞｷﾞ"))), "ガギ");
    }

    #[test]
    fn identical_text_scores_one_and_empty_scores_zero() {
        assert_eq!(similarity(&text("晴天"), &text("晴天")), 1.0);
        assert_eq!(similarity(&text(""), &text("晴天")), 0.0);
        assert_eq!(similarity(&text("晴天"), &text("")), 0.0);
    }

    #[test]
    fn containment_scores_between_085_and_one_by_length_ratio() {
        let score = similarity(&text("abcd"), &text("abcdefgh"));
        assert!((score - (0.85 + 0.15 * 0.5)).abs() < 1e-12);
        // 对称。
        assert_eq!(score, similarity(&text("abcdefgh"), &text("abcd")));
    }

    #[test]
    fn dice_counts_repeated_bigrams_as_a_multiset() {
        // "aaa" 有两个 aa，"aa" 只有一个：交集 1，2*1/(2+1)。
        let score = similarity(&text("aaab"), &text("aaxx"));
        // "aaab": aa aa ab；"aaxx": aa ax xx → 交集 1 → 2/(3+3)。
        assert!((score - (2.0 / 6.0)).abs() < 1e-12);
        assert_eq!(similarity(&text("a"), &text("b")), 0.0);
    }

    #[test]
    fn similarity_works_on_utf16_code_units_like_dotnet() {
        // 一个 emoji 是 2 个码元，所以包含比例按码元长度算。
        let score = similarity(&text("🎵"), &text("🎵歌"));
        assert!((score - (0.85 + 0.15 * (2.0 / 3.0))).abs() < 1e-12);
    }

    #[test]
    fn artist_tokens_are_split_normalized_and_deduplicated() {
        let tokens = split_artist_tokens(Some("Jason Mraz & Colbie Caillat; jason mraz、 "));
        let texts: Vec<String> = tokens.iter().map(|t| string(&t.units)).collect();
        assert_eq!(texts, ["jasonmraz", "colbiecaillat"]);
        assert!(split_artist_tokens(None).is_empty());
        assert!(split_artist_tokens(Some("  ")).is_empty());
        assert!(split_artist_tokens(Some("/ ; ,")).is_empty());
    }

    #[test]
    fn dotnet_character_classes_match_the_documented_definitions() {
        assert!(is_dotnet_whitespace('\u{85}'));
        assert!(is_dotnet_whitespace('\u{A0}'));
        assert!(is_dotnet_whitespace('\u{3000}'));
        assert!(!is_dotnet_whitespace('a'));
        assert!(is_dotnet_control('\u{7F}'));
        assert!(is_dotnet_punctuation('、'));
        assert!(is_dotnet_punctuation('_'));
        assert!(is_dotnet_symbol('+'));
        assert!(is_dotnet_symbol('$'));
        assert!(is_dotnet_symbol('^'));
        assert!(!is_dotnet_symbol('a'));
        assert_eq!(to_lower_invariant('Ä'), 'ä');
    }
}
