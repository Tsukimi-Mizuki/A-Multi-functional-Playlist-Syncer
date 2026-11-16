//! 歌词转逐字：把已有的逐行歌词换成带逐字时间轴的版本。
//!
//! 每个字唱在第几毫秒只有平台知道，从逐行歌词里算不出来。所以这里做的不是在原歌词上加工，
//! 而是重新找到同一首歌，把平台的逐字歌词取回来替换：
//!
//! 1. 读现有歌词——音频文件里内嵌的，和旁边同名的 `.lrc`。两处各自判断：已经是逐字的、没有歌词的不动。
//! 2. 用标题 / 歌手 / 时长搜索，匹配规则与批量在线获取相同；依次问 QQ 音乐、网易云音乐、备用词源（酷狗）。
//! 3. **核对是不是同一个版本**（[`convert`]）：取回的逐字歌词逐行去现有歌词里找文字相同、时间相差不超过
//!    [`TIME_WINDOW_MS`] 的行，对上的不到 [`MIN_MATCH_RATIO`] 就不换。现场版、重录版的歌词时间对不上，
//!    换上去唱的时候是错位的；一个来源对不上就换下一个来源再试。
//! 4. 现有歌词带译文的，把译文按对上的行搬到逐字歌词上（译文原样保留，不换成平台的译文）；没有译文的不加。
//! 5. 写回原处：内嵌的写进音频标签（只写歌词，别的标签不动），`.lrc` 里的写回 `.lrc`（先备份）。
//!
//! 分两步走：[`BatchManager::word_preview`] 只联网、不写文件，把每首歌能不能换告诉界面；用户确认后
//! [`BatchManager::word_apply`] 才写，用的是预览时已经算好的结果，不再联网。写之前会再读一遍文件，
//! 歌词在预览之后被改过就不写。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};

use icu_normalizer::ComposingNormalizerBorrowed;
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::task::{BatchJobError, BatchManager, file_name_of};
use super::{BATCH_CONCURRENCY, BatchError, BatchLyricsService, LocalSong, resolve_local_info};
use crate::download::pool::{Slot, Worker, run_pool};
use crate::library::text::is_blank;
use crate::lyrics::{LyricDocument, RawLyrics, notice, parser, scorer};
use crate::online::{OnlineError, OnlineMusicSource};
use crate::settings::AppSettings;
use crate::tags::AudioTagData;
use crate::tags::sidecar::{
    SIDECAR_BACKUP_DIRECTORY, read_sidecar_lyrics, sidecar_lyrics_path, write_sidecar_lyrics,
};
use crate::utilities::{CancelToken, error_text};

/// 逐字歌词的一行与现有歌词的一行，起始时间最多能差多少还算同一行（毫秒）。
/// 同一个录音在不同平台上的时间轴一般只差几百毫秒；差到一秒半以上，换上去唱的时候就明显错位了。
pub const TIME_WINDOW_MS: i32 = 1500;

/// 逐字歌词里至少要有多大比例的行能在现有歌词里对上，才认为是同一个版本。
/// 留出余量给各平台写法不同的行（作词作曲那几行、标点、个别字的差异）。
pub const MIN_MATCH_RATIO: f64 = 0.6;

/// 两行歌词的相似度（最长公共子序列 ÷ 较长一方的长度）达到多少算同一句。
pub const MIN_LINE_SIMILARITY: f64 = 0.7;

/// 断行不同时，被拆出来的一截至少要有几个字才拿去比（太短的随处都能找到）。
const MIN_PIECE_CHARS: usize = 2;

/// `[ti:…]`、`[ar:…]` 这类说明标签行（不是时间标签）。
static INFO_TAG_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[([A-Za-z]+):.*\]\s*$").expect("说明标签正则合法"));

/// `[mm:ss.xx]` 行时间标签与 `<mm:ss.xx>` 词时间标签（出现在一行歌词中间或末尾的）。
static TIME_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\[[0-9]{1,3}:[0-9]{1,2}(?:[.:][0-9]{1,3})?\]|<[0-9]{1,3}:[0-9]{1,2}(?:[.:][0-9]{1,3})?>",
    )
    .expect("时间标签正则合法")
});

/// 歌词里有多大比例的行带着「夹在文字中间的时间标签」，就认为它已经是逐字歌词（方括号写法）。
const BRACKET_WORD_RATIO: f64 = 0.3;

/// 歌词放在哪里。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WordTarget {
    /// 音频文件里内嵌的歌词。
    Embedded,
    /// 旁边同名的 `.lrc`。
    Sidecar,
}

impl WordTarget {
    fn label(self) -> &'static str {
        match self {
            Self::Embedded => "内嵌歌词",
            Self::Sidecar => "歌词文件",
        }
    }
}

/// 一首歌的结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WordStatus {
    /// 预览：可以转换。
    Ready,
    /// 已经换成逐字歌词。
    Converted,
    /// 本来就是逐字歌词。
    AlreadyWord,
    /// 没有歌词，或歌词没有时间轴。
    NoLyrics,
    /// 没找到匹配度足够的在线歌曲。
    NoMatch,
    /// 找到了歌，但在线来源没有它的逐字歌词。
    NoWordLyrics,
    /// 找到的逐字歌词与现有歌词对不上（多半是另一个版本）。
    Mismatch,
    /// 读写失败。
    Failed,
}

/// 现有的一份歌词是什么样的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Existing {
    /// 没有，或全是空白。
    Missing,
    /// 有文字但没有时间标签，没法核对版本。
    Untimed,
    /// 已经带逐字时间轴。
    Word,
    /// 逐行歌词：要转换的就是它。
    Line,
}

pub(super) fn classify(text: Option<&str>) -> Existing {
    let Some(text) = text.filter(|text| !is_blank(text)) else {
        return Existing::Missing;
    };
    let document = parser::parse(&RawLyrics::from_lrc(text));
    if document.is_empty() {
        Existing::Untimed
    } else if document.has_word_level() || is_bracket_word_level(&document) {
        Existing::Word
    } else {
        Existing::Line
    }
}

/// 方括号写法的逐字歌词：`[00:01.000]第[00:01.300]一[00:01.600]句[00:02.000]`——每个字前面是它的起始时间，
/// 用的是和行时间一样的方括号（LDDC 等工具的「逐字 LRC」）。本程序的解析器只认尖括号的词标签，
/// 会把这种行当成「文字里夹着方括号」的逐行歌词，所以这里单独认：一行正文里有时间标签，而且不只是行尾那一个。
///
/// 只在行尾带一个时间标签的（`[开始]歌词[结束]`）是标了结束时间的**逐行**歌词，不算逐字；这种文件里的译文行也是这个样子。
fn is_bracket_word_level(document: &LyricDocument) -> bool {
    let lines: Vec<&str> = document
        .lines
        .iter()
        .map(|line| line.text.trim())
        .filter(|text| !text.is_empty())
        .collect();
    if lines.is_empty() {
        return false;
    }
    let inner = lines
        .iter()
        .filter(|text| {
            let mut tags = TIME_TAG.find_iter(text);
            match (tags.next(), tags.next()) {
                (Some(_), Some(_)) => true,
                (Some(only), None) => only.end() != text.len(),
                _ => false,
            }
        })
        .count();
    inner as f64 >= BRACKET_WORD_RATIO * lines.len() as f64
}

/// 转换成功的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Converted {
    /// 新的歌词全文（增强 LRC）。
    pub text: String,
    /// 逐字歌词里对上了的行数。
    pub matched: usize,
    /// 逐字歌词里参与核对的行数（没有文字的行不算）。
    pub total: usize,
    /// 现有歌词的译文搬过来了。
    pub keeps_translation: bool,
}

/// 逐字歌词与现有歌词对不上：对上的行数 / 参与核对的行数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mismatch {
    /// 对上了的行数。
    pub matched: usize,
    /// 参与核对的行数。
    pub total: usize,
}

/// 现有歌词里的一行原文，连同紧跟着它、时间相同的译文行。
struct ExistingLine {
    start_ms: i32,
    key: String,
    translation: Option<String>,
}

/// 用来比较两行文字是否相同的键：去掉夹在文字里的时间标签（行尾的结束时间等）、NFKC、小写，
/// 只留字母与数字（空白、标点、括号都不算）。
fn match_key(text: &str) -> String {
    ComposingNormalizerBorrowed::new_nfkc()
        .normalize(&TIME_TAG.replace_all(text, ""))
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// 两个键有多像：最长公共子序列的长度 ÷ 较长一方的长度（0–1）。
fn similarity(left: &str, right: &str) -> f64 {
    let (left, right): (Vec<char>, Vec<char>) = (left.chars().collect(), right.chars().collect());
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let mut previous = vec![0usize; right.len() + 1];
    for a in &left {
        let mut current = vec![0usize; right.len() + 1];
        for (index, b) in right.iter().enumerate() {
            current[index + 1] = if a == b {
                previous[index] + 1
            } else {
                previous[index + 1].max(current[index])
            };
        }
        previous = current;
    }
    previous[right.len()] as f64 / left.len().max(right.len()) as f64
}

/// 两行算不算同一句：键相同；或者够长的一方包含另一方（一边多了括号里的和声、少了语气词）；
/// 或者够长而且相似度不低于 [`MIN_LINE_SIMILARITY`]（各平台同一句歌词常差一两个字：异体字、符号、送假名）。
/// 不到 4 个字的短句只认完全相同，免得随便两个短句就对上。
fn same_line(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let shorter = left.chars().count().min(right.chars().count());
    shorter >= 4
        && (left.contains(right)
            || right.contains(left)
            || similarity(left, right) >= MIN_LINE_SIMILARITY)
}

/// `piece` 是不是 `whole` 的一截（两边断行不同：一边写成一行，另一边拆成两行）。
/// 必须真的比整句短：一模一样的两句是「同一行」，要按起始时间去对，不能当成一截混过时间这一关。
fn is_piece_of(piece: &str, whole: &str) -> bool {
    piece.chars().count() >= MIN_PIECE_CHARS && piece.len() < whole.len() && whole.contains(piece)
}

/// 把现有歌词整理成「原文 + 译文」：本程序与多数工具写双语歌词时，译文行紧跟在原文行后面、时间标签相同。
fn existing_lines(document: &LyricDocument) -> Vec<ExistingLine> {
    let mut result: Vec<ExistingLine> = Vec::new();
    for line in &document.lines {
        let text = line.text.trim();
        if text.is_empty() {
            continue;
        }
        if let Some(previous) = result.last_mut()
            && previous.start_ms == line.start_ms
        {
            // 平台的声明、表示「这行没有译文」的 `//` 不算译文。
            if !notice::is_notice(text) && !text.chars().all(|ch| ch == '/') {
                match &mut previous.translation {
                    Some(translation) => {
                        translation.push(' ');
                        translation.push_str(text);
                    }
                    None => previous.translation = Some(text.to_owned()),
                }
            }
            continue;
        }
        result.push(ExistingLine {
            start_ms: line.start_ms,
            key: match_key(text),
            translation: None,
        });
    }
    result
}

/// 现有歌词开头的说明标签（`[ti:]`、`[ar:]` …）原样保留；`[offset:]` 不留——现有歌词的时间在核对时已经把它算进去了。
fn info_tag_lines(existing: &str) -> Vec<&str> {
    existing
        .lines()
        .map(str::trim)
        .filter(|line| {
            INFO_TAG_LINE
                .captures(line)
                .is_some_and(|captures| !captures[1].eq_ignore_ascii_case("offset"))
        })
        .collect()
}

/// 核对取回的逐字歌词与现有的逐行歌词是不是同一个版本；是的话给出新的歌词全文。
///
/// `word_lrc` 是在线来源给的逐字歌词（增强 LRC，只有原文）。现有歌词带译文时，译文按对上的行搬过去。
pub fn convert(existing: &str, word_lrc: &str) -> Result<Converted, Mismatch> {
    let lines = existing_lines(&parser::parse(&RawLyrics::from_lrc(existing)));
    let mut document = parser::parse(&RawLyrics::from_lrc(word_lrc));

    // 每一行逐字歌词到下一行开始之前的这段时间归它；现有歌词同理。
    let word_ends: Vec<i32> = (0..document.lines.len())
        .map(|index| {
            document
                .lines
                .get(index + 1)
                .map_or(i32::MAX, |next| next.start_ms)
        })
        .collect();
    let existing_end = |index: usize| lines.get(index + 1).map_or(i32::MAX, |next| next.start_ms);
    let gap = |from: i32, to: i32| i64::from(to) - i64::from(from);
    let window = i64::from(TIME_WINDOW_MS);

    let mut taken = vec![false; lines.len()];
    let (mut total, mut matched) = (0usize, 0usize);
    let mut keeps_translation = false;
    for (index, line) in document.lines.iter_mut().enumerate() {
        let key = match_key(&line.text);
        if key.is_empty() {
            continue;
        }
        total += 1;
        let (start, end) = (line.start_ms, word_ends[index]);

        // ① 同一行：起始时间差不多、文字相同或相近。
        let direct = lines
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                gap(start, candidate.start_ms).abs() <= window && same_line(&key, &candidate.key)
            })
            .min_by_key(|(_, candidate)| gap(start, candidate.start_ms).abs())
            .map(|(found, _)| found);
        // ② 断行不同：现有歌词的一行在这里被拆成了几行，这一行是它后面的一截。
        let inside = lines
            .iter()
            .enumerate()
            .find(|(found, candidate)| {
                candidate.start_ms < start
                    && start < existing_end(*found)
                    && is_piece_of(&key, &candidate.key)
            })
            .map(|(found, _)| found);
        // ③ 反过来：这一行在现有歌词里被拆成了几行，它们都落在这一行的时间里。
        let pieces: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                gap(start, candidate.start_ms) >= -window
                    && candidate.start_ms < end
                    && is_piece_of(&candidate.key, &key)
            })
            .map(|(found, _)| found)
            .collect();
        if direct.is_none() && inside.is_none() && pieces.is_empty() {
            continue;
        }
        matched += 1;

        // 译文跟着对上的行走；现有歌词的每一句译文只用一次（一行被拆成几行时给第一截）。
        let mut translations: Vec<&str> = Vec::new();
        for found in direct.or(inside).into_iter().chain(pieces) {
            if !taken[found]
                && let Some(translation) = lines[found].translation.as_deref()
            {
                taken[found] = true;
                translations.push(translation);
            }
        }
        if !translations.is_empty() {
            line.translation = Some(translations.join(" "));
            keeps_translation = true;
        }
    }

    if total == 0 || !document.has_word_level() || (matched as f64) < MIN_MATCH_RATIO * total as f64
    {
        return Err(Mismatch { matched, total });
    }

    // 没有译文要搬时原样用来源给的文本，不重新生成一遍。
    let body = if keeps_translation {
        parser::to_enhanced_lrc(&document, true)
    } else {
        word_lrc.trim().to_owned()
    };
    let mut text = String::new();
    for tag in info_tag_lines(existing) {
        text.push_str(tag);
        text.push('\n');
    }
    text.push_str(&body);
    Ok(Converted {
        text,
        matched,
        total,
        keeps_translation,
    })
}

/// 一处要写的歌词。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlannedTarget {
    target: WordTarget,
    /// 预览时读到的原文：写之前再读一遍，不一样就不写。
    original: String,
    /// 新的歌词全文。
    text: String,
}

/// 预览阶段为一首歌算好的结果，等用户确认后写入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WordPlan {
    targets: Vec<PlannedTarget>,
}

/// 一首歌的结论（服务层）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WordOutcome {
    status: WordStatus,
    message: String,
    source_label: Option<String>,
    targets: Vec<WordTarget>,
    keeps_translation: bool,
    plan: Option<WordPlan>,
}

impl WordOutcome {
    fn plain(status: WordStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            source_label: None,
            targets: Vec::new(),
            keeps_translation: false,
            plan: None,
        }
    }
}

fn target_names(targets: &[WordTarget]) -> String {
    targets
        .iter()
        .map(|target| target.label())
        .collect::<Vec<_>>()
        .join("、")
}

/// 拿一份逐字歌词去核对每一处逐行歌词；至少对上一处就得到可以写的结果。
fn plan_with(
    line_targets: &[(WordTarget, String)],
    word_lrc: &str,
    source_label: &str,
) -> Result<WordOutcome, Mismatch> {
    let mut planned = Vec::new();
    let mut best: Option<Converted> = None;
    let mut worst = Mismatch {
        matched: 0,
        total: 0,
    };
    for (target, original) in line_targets {
        match convert(original, word_lrc) {
            Ok(converted) => {
                planned.push(PlannedTarget {
                    target: *target,
                    original: original.clone(),
                    text: converted.text.clone(),
                });
                best.get_or_insert(converted);
            }
            Err(mismatch) => worst = mismatch,
        }
    }
    let Some(best) = best else {
        return Err(worst);
    };

    let targets: Vec<WordTarget> = planned.iter().map(|item| item.target).collect();
    let keeps_translation = best.keeps_translation;
    let mut message = format!(
        "可以转换：{source_label}的逐字歌词与现有歌词对上 {}/{} 行（{}）",
        best.matched,
        best.total,
        target_names(&targets)
    );
    if keeps_translation {
        message.push_str("，保留现有译文");
    }
    Ok(WordOutcome {
        status: WordStatus::Ready,
        message,
        source_label: Some(source_label.to_owned()),
        targets,
        keeps_translation,
        plan: Some(WordPlan { targets: planned }),
    })
}

impl BatchLyricsService {
    async fn read_sidecar_text(&self, path: &Path) -> Option<String> {
        let target = path.to_path_buf();
        tokio::task::spawn_blocking(move || read_sidecar_lyrics(&target))
            .await
            .ok()
            .flatten()
            .map(|lyrics| lyrics.text)
    }

    /// 预览：这首歌的歌词能不能换成逐字的。只联网、不写文件。
    pub(super) async fn plan_word_lyrics(
        &self,
        file_path: &Path,
        min_score: f64,
        cancel: &CancelToken,
    ) -> Result<WordOutcome, BatchError> {
        if !file_path.is_file() {
            return Ok(WordOutcome::plain(WordStatus::Failed, "文件不存在"));
        }
        if cancel.is_cancelled() {
            return Err(BatchError::Cancelled);
        }
        let Some(info) = self.read_tags(file_path).await? else {
            return Ok(WordOutcome::plain(WordStatus::Failed, "无法读取音频标签"));
        };

        let embedded = info.lyrics.clone().filter(|text| !is_blank(text));
        let sidecar = self.read_sidecar_text(file_path).await;
        let kinds = [
            (
                WordTarget::Embedded,
                classify(embedded.as_deref()),
                embedded,
            ),
            (WordTarget::Sidecar, classify(sidecar.as_deref()), sidecar),
        ];
        let line_targets: Vec<(WordTarget, String)> = kinds
            .iter()
            .filter(|(_, kind, _)| *kind == Existing::Line)
            .filter_map(|(target, _, text)| text.clone().map(|text| (*target, text)))
            .collect();
        if line_targets.is_empty() {
            let has = |wanted: Existing| kinds.iter().any(|(_, kind, _)| *kind == wanted);
            return Ok(if has(Existing::Word) {
                WordOutcome::plain(WordStatus::AlreadyWord, "已经是逐字歌词")
            } else if has(Existing::Untimed) {
                WordOutcome::plain(
                    WordStatus::NoLyrics,
                    "歌词没有时间轴，无法核对是不是同一个版本",
                )
            } else {
                WordOutcome::plain(WordStatus::NoLyrics, "没有歌词")
            });
        }

        let (local_title, local_artist) = resolve_local_info(file_path, &info);
        if is_blank(&local_title) {
            return Ok(WordOutcome::plain(
                WordStatus::NoMatch,
                "无法从标签或文件名识别歌曲名",
            ));
        }
        let queries = scorer::build_queries(Some(&local_title), Some(&local_artist));
        let min_score = min_score.clamp(scorer::MIN_THRESHOLD, scorer::MAX_THRESHOLD);
        let local = LocalSong {
            title: &local_title,
            artist: &local_artist,
            album: info.album.as_deref().map(str::trim),
            duration_ms: info.duration_ms,
            queries: &queries,
        };

        let mut found_song = false;
        let mut found_word = false;
        let mut mismatch: Option<Mismatch> = None;
        let mut consider = |word: Option<String>, label: &str| -> Option<WordOutcome> {
            let word = word.filter(|text| !is_blank(text))?;
            found_word = true;
            match plan_with(&line_targets, &word, label) {
                Ok(outcome) => Some(outcome),
                Err(current) => {
                    // 留下对得最多的那一次，告诉用户差多少。
                    if mismatch.is_none_or(|kept| current.matched > kept.matched) {
                        mismatch = Some(current);
                    }
                    None
                }
            }
        };

        // QQ 音乐的逐字歌词最全，先问它；一个来源的版本对不上就换下一个。
        for source in [OnlineMusicSource::QqMusic, OnlineMusicSource::Netease] {
            let Some(best) = self
                .find_best_candidate(source, &local, min_score, cancel)
                .await?
            else {
                continue;
            };
            found_song = true;
            let details = match self.online.get_lyrics_details(&best, cancel).await {
                Ok(details) => details,
                Err(OnlineError::Cancelled) => return Err(BatchError::Cancelled),
                Err(OnlineError::Failed(_)) => continue,
            };
            if let Some(outcome) = consider(
                details.and_then(|details| details.word_lyrics),
                source.label(),
            ) {
                return Ok(outcome);
            }
        }
        if let Some(candidate) = self
            .find_fallback_lyrics(&local, true, min_score, cancel)
            .await?
        {
            found_song = true;
            let label = candidate.source_label.clone();
            if let Some(outcome) = consider(candidate.word_lyrics, &label) {
                return Ok(outcome);
            }
        }

        Ok(match (found_song, found_word, mismatch) {
            (false, _, _) => WordOutcome::plain(WordStatus::NoMatch, "未找到匹配度足够的在线歌曲"),
            (true, false, _) | (true, true, None) => {
                WordOutcome::plain(WordStatus::NoWordLyrics, "在线来源没有这首歌的逐字歌词")
            }
            (true, true, Some(mismatch)) => WordOutcome::plain(
                WordStatus::Mismatch,
                format!(
                    "找到的逐字歌词与现有歌词对不上（只对上 {}/{} 行，可能是另一个版本），没有替换",
                    mismatch.matched, mismatch.total
                ),
            ),
        })
    }

    /// 把预览时算好的逐字歌词写进文件。歌词在预览之后被改过的那一处不写，并当作失败。
    pub(super) async fn apply_word_plan(
        &self,
        file_path: &Path,
        plan: &WordPlan,
    ) -> Result<WordOutcome, BatchError> {
        let changed = || BatchError::Failed("歌词在预览之后有改动，请重新预览".to_owned());
        let mut written = Vec::new();
        let mut backed_up = false;
        for item in &plan.targets {
            match item.target {
                WordTarget::Embedded => {
                    let current = self
                        .read_tags(file_path)
                        .await?
                        .and_then(|info| info.lyrics);
                    if current.as_deref() != Some(item.original.as_str()) {
                        return Err(changed());
                    }
                    self.write_tags(
                        file_path,
                        AudioTagData {
                            word_lyrics: Some(item.text.clone()),
                            ..AudioTagData::default()
                        },
                    )
                    .await?;
                }
                WordTarget::Sidecar => {
                    if self.read_sidecar_text(file_path).await.as_deref()
                        != Some(item.original.as_str())
                    {
                        return Err(changed());
                    }
                    let lrc_path = sidecar_lyrics_path(file_path);
                    let text = item.text.clone();
                    let result =
                        tokio::task::spawn_blocking(move || write_sidecar_lyrics(&lrc_path, &text))
                            .await;
                    match result {
                        Ok(Ok(())) => backed_up = true,
                        Ok(Err(error)) => {
                            return Err(BatchError::Failed(format!(
                                "写入歌词文件失败：{}",
                                error_text::describe(&error)
                            )));
                        }
                        Err(_) => {
                            return Err(BatchError::Failed(
                                "写入歌词文件时程序内部出错，请重试。".to_owned(),
                            ));
                        }
                    }
                }
            }
            written.push(item.target);
        }

        let mut message = format!("已换成逐字歌词（{}）", target_names(&written));
        if backed_up {
            message.push_str(&format!(
                "，原歌词文件备份在 {SIDECAR_BACKUP_DIRECTORY} 文件夹"
            ));
        }
        Ok(WordOutcome {
            targets: written,
            ..WordOutcome::plain(WordStatus::Converted, message)
        })
    }
}

// ===== 任务编排 =====

/// `word_lyrics_preview` / `word_lyrics_apply` 的请求（camelCase）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordLyricsRequest {
    /// 要处理的音频文件。
    pub files: Vec<String>,
}

/// 一首歌的结果（给界面）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordLyricsResultView {
    /// 完整路径。
    pub file_path: String,
    /// 文件名。
    pub file_name: String,
    /// 结论。
    pub status: WordStatus,
    /// 中文说明。
    pub message: String,
    /// 逐字歌词的来源名。
    pub source_label: Option<String>,
    /// 要写（预览）或已经写了（转换）的地方。
    pub targets: Vec<WordTarget>,
    /// 现有歌词的译文会保留。
    pub keeps_translation: bool,
}

impl WordLyricsResultView {
    fn new(path: &str, outcome: &WordOutcome) -> Self {
        Self {
            file_path: path.to_owned(),
            file_name: file_name_of(path),
            status: outcome.status,
            message: outcome.message.clone(),
            source_label: outcome.source_label.clone(),
            targets: outcome.targets.clone(),
            keeps_translation: outcome.keeps_translation,
        }
    }
}

/// `wordlyrics:progress` 事件的载荷。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordLyricsProgressView {
    /// 已处理完的文件数。
    pub completed: usize,
    /// 总数。
    pub total: usize,
    /// 这个文件在请求里的下标。
    pub index: usize,
    /// 完整路径。
    pub path: String,
    /// 文件名。
    pub file_name: String,
    /// `started` / `finished`（`result` 有值）。
    pub stage: &'static str,
    /// 结果（`finished` 才有）。
    pub result: Option<WordLyricsResultView>,
}

/// 预览或转换的汇总。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordLyricsSummaryView {
    /// 请求的文件总数（去重后）。
    pub total: usize,
    /// 实际处理完的文件数（取消时小于 `total`）。
    pub processed: usize,
    /// 可以转换的（预览）。
    pub ready: usize,
    /// 已经转换的。
    pub converted: usize,
    /// 不需要或没法转换的（已是逐字、没有歌词、没找到、没有逐字版本、版本对不上）。
    pub skipped: usize,
    /// 读写失败的。
    pub failed: usize,
    /// 是否被取消。
    pub cancelled: bool,
    /// 已处理文件的结果，按请求顺序。
    pub results: Vec<WordLyricsResultView>,
}

/// 预览阶段算好、等确认后写入的结果，按小写路径存。
pub(super) type WordPlans = Arc<std::sync::Mutex<HashMap<String, WordPlan>>>;

type WordProgressSink = Arc<dyn Fn(WordLyricsProgressView) + Send + Sync>;

fn plans_of(plans: &WordPlans) -> std::sync::MutexGuard<'_, HashMap<String, WordPlan>> {
    plans
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn unique_files(request: &WordLyricsRequest) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    request
        .files
        .iter()
        .filter(|path| !path.trim().is_empty())
        .filter(|path| seen.insert(path.to_lowercase()))
        .map(PathBuf::from)
        .collect()
}

/// 一个文件要做的事：预览（联网核对）或转换（写入预览的结果）。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    Preview { min_score: f64 },
    Apply,
}

impl BatchManager {
    /// 预览：逐个文件联网核对歌词能不能换成逐字的，不写任何文件。结果留在内存里等 [`word_apply`](Self::word_apply)。
    pub async fn word_preview(
        &self,
        request: WordLyricsRequest,
        settings: &AppSettings,
        progress: WordProgressSink,
    ) -> Result<WordLyricsSummaryView, BatchJobError> {
        self.run_word_step(
            request,
            Step::Preview {
                min_score: settings.match_threshold,
            },
            progress,
        )
        .await
    }

    /// 转换：把预览时算好的逐字歌词写进文件（不再联网）。没有预览过、或预览结论不是「可以转换」的文件算失败。
    pub async fn word_apply(
        &self,
        request: WordLyricsRequest,
        progress: WordProgressSink,
    ) -> Result<WordLyricsSummaryView, BatchJobError> {
        self.run_word_step(request, Step::Apply, progress).await
    }

    async fn run_word_step(
        &self,
        request: WordLyricsRequest,
        step: Step,
        progress: WordProgressSink,
    ) -> Result<WordLyricsSummaryView, BatchJobError> {
        let files = unique_files(&request);
        if files.is_empty() {
            return Err(BatchJobError {
                kind: "noFiles",
                message: "没有可处理的音频文件。".to_owned(),
            });
        }
        let Some(job) = self.slot.try_begin() else {
            return Err(BatchJobError {
                kind: "busy",
                message: "已有一个批量任务正在进行，请等待它完成或先取消。".to_owned(),
            });
        };

        let total = files.len();
        let preview = matches!(step, Step::Preview { .. });
        if preview {
            // 重新预览的文件丢掉上一次的结果。
            let mut plans = plans_of(&self.word_plans);
            for file in &files {
                plans.remove(&file.to_string_lossy().to_lowercase());
            }
            self.logs
                .info(format!("开始检查歌词能否换成逐字：{total} 个文件"));
        } else {
            self.logs
                .info(format!("开始把歌词换成逐字：{total} 个文件"));
        }

        let completed = Arc::new(AtomicUsize::new(0));
        let worker: Arc<Worker<PathBuf, WordOutcome>> = {
            let service = Arc::clone(&self.service);
            let plans = Arc::clone(&self.word_plans);
            let cancel = job.cancel.clone();
            let completed = Arc::clone(&completed);
            Arc::new(move |index, path: PathBuf| {
                let service = Arc::clone(&service);
                let plans = Arc::clone(&plans);
                let cancel = cancel.clone();
                let completed = Arc::clone(&completed);
                let progress = Arc::clone(&progress);
                Box::pin(async move {
                    let path_text = path.to_string_lossy().into_owned();
                    let key = path_text.to_lowercase();
                    let file_name = file_name_of(&path_text);
                    progress(WordLyricsProgressView {
                        completed: completed.load(Ordering::SeqCst),
                        total,
                        index,
                        path: path_text.clone(),
                        file_name: file_name.clone(),
                        stage: "started",
                        result: None,
                    });

                    let attempt = match step {
                        Step::Preview { min_score } => {
                            service.plan_word_lyrics(&path, min_score, &cancel).await
                        }
                        Step::Apply => {
                            let plan = plans_of(&plans).remove(&key);
                            match plan {
                                Some(plan) if cancel.is_cancelled() => {
                                    // 取消了：把结果放回去，之后还能接着转换。
                                    plans_of(&plans).insert(key.clone(), plan);
                                    Err(BatchError::Cancelled)
                                }
                                Some(plan) => service.apply_word_plan(&path, &plan).await,
                                None => Err(BatchError::Failed(
                                    "还没有预览过这首歌，请先预览".to_owned(),
                                )),
                            }
                        }
                    };
                    let mut outcome = match attempt {
                        Ok(outcome) => outcome,
                        // 取消时无需逐项记录。
                        Err(BatchError::Cancelled) => return None,
                        Err(BatchError::Failed(reason)) => {
                            WordOutcome::plain(WordStatus::Failed, format!("处理失败：{reason}"))
                        }
                    };
                    if let Some(plan) = outcome.plan.take() {
                        plans_of(&plans).insert(key, plan);
                    }

                    let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                    progress(WordLyricsProgressView {
                        completed: done,
                        total,
                        index,
                        path: path_text.clone(),
                        file_name,
                        stage: "finished",
                        result: Some(WordLyricsResultView::new(&path_text, &outcome)),
                    });
                    Some(outcome)
                })
            })
        };

        let slots = run_pool(files.clone(), BATCH_CONCURRENCY, &job.cancel, worker).await;

        let mut results = Vec::new();
        for (path, slot) in files.iter().zip(slots) {
            let path_text = path.to_string_lossy();
            match slot {
                Slot::Done(outcome) => {
                    results.push(WordLyricsResultView::new(&path_text, &outcome));
                }
                Slot::Panicked => results.push(WordLyricsResultView::new(
                    &path_text,
                    &WordOutcome::plain(WordStatus::Failed, "处理失败：程序内部出错，请重试。"),
                )),
                Slot::NotRun => {}
            }
        }

        let count = |status: WordStatus| results.iter().filter(|r| r.status == status).count();
        let ready = count(WordStatus::Ready);
        let converted = count(WordStatus::Converted);
        let failed = count(WordStatus::Failed);
        let skipped = results.len() - ready - converted - failed;
        let cancelled = job.cancel.is_cancelled();
        let text = if preview {
            format!("可以转换 {ready} 首，不需要或没法转换 {skipped} 首，失败 {failed} 首")
        } else {
            format!("已转换 {converted} 首，失败 {failed} 首")
        };
        let name = if preview {
            "检查歌词能否换成逐字"
        } else {
            "歌词转逐字"
        };
        if cancelled {
            self.logs.warning(format!(
                "{name}已取消：已处理 {}/{total}，{text}",
                results.len()
            ));
        } else if failed == 0 {
            self.logs.success(format!("{name}完成：{text}"));
        } else {
            self.logs.warning(format!("{name}完成：{text}"));
        }

        Ok(WordLyricsSummaryView {
            total,
            processed: results.len(),
            ready,
            converted,
            skipped,
            failed,
            cancelled,
            results,
        })
    }
}

#[cfg(test)]
mod tests;
