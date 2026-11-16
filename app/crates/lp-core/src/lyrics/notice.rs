//! 清除在线歌词里由音乐平台插入的「非歌词」声明行，例如：
//!
//! ```text
//! [00:00.000]QQ音乐享有本翻译作品的著作权[00:12.400]
//! [00:02.020]以下歌词翻译由文曲大模型提供
//! [00:12.500]【未经著作权人许可 不得翻唱 翻录或使用】
//! [02:12.240]【感谢某某提供原歌词】
//! ```
//!
//! 规则来自用户自己整理曲库时写的清理脚本（`lrc_strip_notice.py`，2026-11），判定原则照搬：
//! **只做整行判定，绝不按关键词做子串匹配**——
//!
//! 1. 整行正文规范化后与某条声明完全相等；或
//! 2. 整行正文规范化后匹配某条首尾锚定（`^…$`）的正则。
//!
//! 原因：「感谢」「提供」「声明」「翻译」「制作」这些词在真实歌词里到处都是，按关键词删会误删歌词。
//! 宁可漏掉一种没见过的声明，也不能删掉一句歌词。
//!
//! 「规范化」= 去掉所有 `[mm:ss.xx]` / `<mm:ss.xx>` 时间标签 → NFKC（全角转半角）→ 剥掉首尾成对的括号
//! （`【】`、`()`、`「」` 等）→ 去掉全部空白 → 转小写。所以全角 / 半角标点、括号包不包、字间有没有空格，
//! 都落到同一条规则上。简体与繁体写法各认一份（脚本只有简体，这里补了同一句话的繁体）。
//!
//! 默认**保留**的两类（脚本里也是要加开关才删）：「纯音乐，请欣赏」这类纯音乐提示，以及 `(2021年作品)` 这类年份标注。
//!
//! 只用于联网取回的歌词（[`parse_online`]）；文件里已有的歌词、同名 `.lrc` 与用户在编辑器里自己输入的内容不经过这里。

use std::sync::LazyLock;

use icu_normalizer::ComposingNormalizerBorrowed;
use regex::Regex;

use super::models::{LyricDocument, RawLyrics};
use super::parser;

/// `[mm:ss]` / `[mm:ss.xxx]` 行时间标签与 `<mm:ss.xxx>` 词时间标签（可以出现在行里任何位置，行尾的也算）。
static TIME_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\[[0-9]{1,3}:[0-9]{1,2}(?:[.:][0-9]{1,3})?\]|<[0-9]{1,3}:[0-9]{1,2}(?:[.:][0-9]{1,3})?>",
    )
    .expect("时间标签正则合法")
});

/// 成对的括号：整行被其中一对包住时剥掉（可以套多层）。全角圆括号在 NFKC 之后已经是半角。
const BRACKET_PAIRS: [(char, char); 8] = [
    ('【', '】'),
    ('[', ']'),
    ('〔', '〕'),
    ('(', ')'),
    ('「', '」'),
    ('『', '』'),
    ('《', '》'),
    ('<', '>'),
];

/// 整行相等的声明（写成人能读的样子，使用前按同一套规范化处理）。
const EXACT_NOTICES: [&str; 6] = [
    "QQ音乐享有本翻译作品的著作权",
    "TME享有本翻译作品的著作权",
    "未经许可,不得翻唱或使用",
    "翻译授权网易云音乐使用",
    "翻译水平有限 授权网易云音乐使用",
    "歌詞 / Eng Subs",
];

/// 首尾锚定的正则，作用在规范化后的整行正文上（已经没有空白、全是小写与半角标点）。
/// 字符组里并列的是同一个字的简体与繁体。
const NOTICE_PATTERNS: [&str; 6] = [
    r"^(?:qq音[乐樂]|tme|[网網]易[云雲]音[乐樂]|酷狗音[乐樂]|全民k歌|酷我音[乐樂])?享有本翻[译譯]作品的著作[权權]$",
    r"^以下歌[词詞]翻[译譯]由.{1,24}提供$",
    r"^未[经經](?:[许許]可|著作[权權]人[许許]可|授[权權]).{0,24}(?:不得|[请請]勿|禁止)(?:翻唱|翻[录錄]|[转轉][载載]|使用).{0,24}$",
    r"^翻[译譯](?:水平有限)?,?授[权權].{0,12}音[乐樂]使用$",
    r"^感[谢謝].{0,24}提供原歌[词詞]?$",
    r"^以下歌[词詞](?:由|[来來]自).{1,24}提供$",
];

static EXACT: LazyLock<Vec<String>> =
    LazyLock::new(|| EXACT_NOTICES.iter().map(|text| normalize(text)).collect());

static PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    NOTICE_PATTERNS
        .iter()
        .map(|pattern| Regex::new(pattern).expect("声明正则合法"))
        .collect()
});

/// 规范化一行正文：去时间标签 → NFKC → 剥首尾成对括号 → 去空白 → 小写。
fn normalize(text: &str) -> String {
    let without_tags = TIME_TAG.replace_all(text, "");
    let folded = ComposingNormalizerBorrowed::new_nfkc().normalize(&without_tags);

    let mut body = folded.trim();
    loop {
        let mut chars = body.chars();
        let (Some(first), Some(last)) = (chars.next(), chars.next_back()) else {
            break;
        };
        if !BRACKET_PAIRS.contains(&(first, last)) {
            break;
        }
        body = body[first.len_utf8()..body.len() - last.len_utf8()].trim();
    }

    body.chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// 这一行（可以带时间标签）是不是平台插入的声明。空行不是。
pub fn is_notice(line: &str) -> bool {
    let body = normalize(line);
    if body.is_empty() {
        return false;
    }
    EXACT.contains(&body) || PATTERNS.iter().any(|rule| rule.is_match(&body))
}

/// 从解析好的歌词里去掉声明：整行是声明的删掉这一行；歌词行还在、只是配上的译文是声明的，只去掉译文。
/// 文档里留着的原始译文 / 罗马音文本也一并清理（清完没有内容就当作没有）。返回去掉了多少处。
pub fn strip_document(document: &mut LyricDocument) -> usize {
    let before = document.lines.len();
    document.lines.retain(|line| !is_notice(&line.text));
    let mut removed = before - document.lines.len();

    for line in &mut document.lines {
        if line.translation.as_deref().is_some_and(is_notice) {
            line.translation = None;
            removed += 1;
        }
    }

    for raw in [&mut document.translation, &mut document.romanization] {
        if let Some(text) = raw.take() {
            let cleaned = strip_lrc_text(&text);
            *raw = (!cleaned.trim().is_empty()).then_some(cleaned);
        }
    }
    removed
}

/// 从 LRC 文本里逐行去掉声明行（没有时间标签的声明行也去掉），其余内容与换行原样保留。
/// 给没有经过解析、直接把来源文本交出去的地方用。
pub fn strip_lrc_text(text: &str) -> String {
    text.split_inclusive('\n')
        .filter(|line| !is_notice(line))
        .collect()
}

/// 解析联网取回的歌词，并去掉平台插入的声明行。所有在线来源（网易云、QQ 音乐、酷狗、LRCLIB）都走这里；
/// [`parser::parse`] 本身不做清理，保持与 C# 版逐字节一致。
pub fn parse_online(raw: &RawLyrics) -> LyricDocument {
    let mut document = parser::parse(raw);
    strip_document(&mut document);
    document
}

#[cfg(test)]
mod tests;
