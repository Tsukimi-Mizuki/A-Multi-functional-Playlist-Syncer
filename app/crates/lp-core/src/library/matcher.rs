//! 歌单歌曲与本地曲库的一对一匹配（对应 C# `SongMatcher` / `ISongMatcher`）。
//!
//! 先按网易云 ID 精确匹配，再在剩余歌曲上按「标题 0.55 + 歌手 0.30 + 时长 0.15」的相似度做贪心一对一匹配：
//! 所有候选放进优先队列，分数高的先占用本地文件，被占用的歌曲再找下一个最优候选。分数、阈值、
//! 并列时的取舍都与 C# 完全一致（有跨实现测试逐首比对）。
//!
//! 「文件名猜测」不在这里：扫描阶段标签里没有标题 / 歌手时，会用文件名推断（见 `scanner::parse_file_name`），
//! 推断结果作为普通元数据参与评分。C# 的 `MatchMethod.FileName` 从未被产生，这里同样只保留枚举值。

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use super::text::{MatchText, normalize_for_match, similarity, split_artist_tokens};
use super::track::LocalTrack;
use crate::netease::NeteaseSong;

/// 大于等于该分值才认为模糊匹配成立（固定值，与设置里的「在线匹配度阈值」无关）。
pub const DEFAULT_THRESHOLD: f64 = 0.72;

/// 时长容差（毫秒）；差值达到该值时时长得分归零。
pub const DURATION_TOLERANCE_MS: i64 = 12_000;

const TITLE_WEIGHT: f64 = 0.55;
const ARTIST_WEIGHT: f64 = 0.30;
const DURATION_WEIGHT: f64 = 0.15;

/// 匹配方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMethod {
    /// 没有匹配。
    None,
    /// 网易云歌曲 ID 精确匹配（置信度 1）。
    NeteaseId,
    /// 标题 / 歌手 / 时长评分。
    Metadata,
    /// 文件名匹配（保留与 C# 一致的枚举值，匹配器本身不会产生）。
    FileName,
}

/// 歌曲在本地的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalAvailability {
    /// 本地没有。
    Missing,
    /// 本地已有可用的音频文件。
    Available,
    /// 本地只有未解密的 `.ncm`。
    EncryptedNcm,
}

/// 一首歌单歌曲的匹配结果（对应 C# `SongMatch`）。用下标指回输入，避免复制整首歌与整条本地曲目。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SongMatch {
    /// 歌曲在输入歌单里的下标。
    pub song_index: usize,
    /// 匹配到的本地曲目在输入曲库里的下标。
    pub local_index: Option<usize>,
    /// 匹配方式。
    pub method: MatchMethod,
    /// 置信度：ID 匹配为 1，模糊匹配为综合分，未匹配为 0。
    pub confidence: f64,
    /// 本地状态。
    pub availability: LocalAvailability,
}

impl SongMatch {
    /// 本地是否有对应文件。
    pub fn is_present(&self) -> bool {
        self.local_index.is_some()
    }

    fn missing(song_index: usize) -> Self {
        Self {
            song_index,
            local_index: None,
            method: MatchMethod::None,
            confidence: 0.0,
            availability: LocalAvailability::Missing,
        }
    }

    fn found(
        song_index: usize,
        local_index: usize,
        local: &LocalTrack,
        method: MatchMethod,
        confidence: f64,
    ) -> Self {
        Self {
            song_index,
            local_index: Some(local_index),
            method,
            confidence,
            availability: if local.is_ncm {
                LocalAvailability::EncryptedNcm
            } else {
                LocalAvailability::Available
            },
        }
    }
}

/// 歌单歌曲与本地曲库的一对一匹配器。无状态，可随意共享。
#[derive(Debug, Default, Clone, Copy)]
pub struct SongMatcher;

struct SongProfile {
    titles: Vec<MatchText>,
    artists: Vec<MatchText>,
    duration_ms: i64,
}

struct LocalProfile {
    title: MatchText,
    artists: Vec<MatchText>,
    duration_ms: i64,
}

impl SongProfile {
    fn new(song: &NeteaseSong) -> Self {
        let mut titles: Vec<MatchText> = Vec::with_capacity(2);
        let name = normalize_for_match(Some(&song.name));
        if !name.is_empty() {
            titles.push(MatchText::from_units(name));
        }
        if let Some(alias) = song.alias.as_deref() {
            let alias = normalize_for_match(Some(alias));
            if !alias.is_empty() {
                let alias = MatchText::from_units(alias);
                if !titles.contains(&alias) {
                    titles.push(alias);
                }
            }
        }

        let mut artists: Vec<MatchText> = Vec::new();
        for artist in &song.artists {
            for token in split_artist_tokens(Some(&artist.name)) {
                if !artists.contains(&token) {
                    artists.push(token);
                }
            }
        }

        Self {
            titles,
            artists,
            duration_ms: i64::from(song.duration_ms),
        }
    }
}

impl LocalProfile {
    fn new(track: &LocalTrack) -> Self {
        Self {
            title: MatchText::from_units(normalize_for_match(Some(&track.title))),
            artists: split_artist_tokens(track.artist.as_deref()),
            duration_ms: track.duration_ms,
        }
    }
}

/// 候选：优先队列里「更大」的先出队——分数高的优先，分数相同时歌曲下标小的优先，再相同时本地下标小的优先。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Candidate {
    song_index: usize,
    local_index: usize,
    score: f64,
}

impl Eq for Candidate {}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.song_index.cmp(&self.song_index))
            .then_with(|| other.local_index.cmp(&self.local_index))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl SongMatcher {
    /// 把歌单歌曲与本地曲库做一对一匹配；每首本地曲最多匹配一首歌单歌曲。
    /// 返回值与 `songs` 等长、顺序一致。
    pub fn match_songs(&self, songs: &[NeteaseSong], locals: &[LocalTrack]) -> Vec<SongMatch> {
        let song_count = songs.len();
        let mut results: Vec<Option<SongMatch>> = vec![None; song_count];
        if song_count == 0 {
            return Vec::new();
        }

        let mut assigned = vec![false; song_count];
        let mut used_local = vec![false; locals.len()];

        // 第一阶段：网易云 ID 精确匹配。同一个 ID 出现在多个本地文件里时只认第一个，其余留给第二阶段。
        let mut local_by_netease_id: HashMap<i64, usize> = HashMap::new();
        for (index, track) in locals.iter().enumerate() {
            if let Some(id) = track.netease_id.filter(|id| *id > 0) {
                local_by_netease_id.entry(id).or_insert(index);
            }
        }
        if !local_by_netease_id.is_empty() {
            for (song_index, song) in songs.iter().enumerate() {
                let Some(&local_index) = local_by_netease_id.get(&song.id) else {
                    continue;
                };
                if used_local[local_index] {
                    continue;
                }
                used_local[local_index] = true;
                assigned[song_index] = true;
                results[song_index] = Some(SongMatch::found(
                    song_index,
                    local_index,
                    &locals[local_index],
                    MatchMethod::NeteaseId,
                    1.0,
                ));
            }
        }

        // 第二阶段：基于归一化元数据 + 时长的一对一贪心匹配。
        let local_profiles: Vec<LocalProfile> = locals.iter().map(LocalProfile::new).collect();
        let mut profiles: Vec<Option<SongProfile>> = (0..song_count).map(|_| None).collect();
        let mut queue: BinaryHeap<Candidate> = BinaryHeap::new();
        for song_index in 0..song_count {
            if assigned[song_index] {
                continue;
            }
            let profile = SongProfile::new(&songs[song_index]);
            if let Some(best) =
                find_best_candidate(song_index, &profile, &local_profiles, &used_local)
            {
                queue.push(best);
            }
            profiles[song_index] = Some(profile);
        }

        while let Some(candidate) = queue.pop() {
            if assigned[candidate.song_index] {
                continue;
            }

            if used_local[candidate.local_index] {
                // 该候选的本地曲已被更高分的歌曲占用，重新找当前最优候选。
                let profile = profiles[candidate.song_index]
                    .as_ref()
                    .expect("进入队列的歌曲一定建过画像");
                if let Some(next) =
                    find_best_candidate(candidate.song_index, profile, &local_profiles, &used_local)
                {
                    queue.push(next);
                }
                continue;
            }

            used_local[candidate.local_index] = true;
            assigned[candidate.song_index] = true;
            results[candidate.song_index] = Some(SongMatch::found(
                candidate.song_index,
                candidate.local_index,
                &locals[candidate.local_index],
                MatchMethod::Metadata,
                candidate.score,
            ));
        }

        results
            .into_iter()
            .enumerate()
            .map(|(song_index, found)| found.unwrap_or_else(|| SongMatch::missing(song_index)))
            .collect()
    }
}

/// 在尚未被占用的本地曲目里找分数最高且不低于阈值的一个；分数相同取下标小的。
fn find_best_candidate(
    song_index: usize,
    song: &SongProfile,
    locals: &[LocalProfile],
    used_local: &[bool],
) -> Option<Candidate> {
    let mut best: Option<Candidate> = None;
    for (local_index, local) in locals.iter().enumerate() {
        if used_local[local_index] {
            continue;
        }
        let Some(score) = score(song, local) else {
            continue;
        };
        if best.is_none_or(|current| score > current.score) {
            best = Some(Candidate {
                song_index,
                local_index,
                score,
            });
        }
    }
    best
}

/// 综合分：标题 0.55 + 歌手 0.30 + 时长 0.15；低于阈值返回 `None`。
///
/// 先算便宜的歌手与时长分：就算标题满分也达不到阈值时直接跳过标题比较（标题相似度不会超过 1，
/// 浮点的乘加按同一顺序求值又是单调的，所以这个剪枝不会改变任何结果）。
fn score(song: &SongProfile, local: &LocalProfile) -> Option<f64> {
    let artist_score = artist_similarity(&song.artists, &local.artists);
    let duration_score = duration_similarity(song.duration_ms, local.duration_ms);
    let upper_bound =
        (TITLE_WEIGHT * 1.0) + (ARTIST_WEIGHT * artist_score) + (DURATION_WEIGHT * duration_score);
    if upper_bound < DEFAULT_THRESHOLD {
        return None;
    }

    let title_score = title_similarity(song, local);
    let total = (TITLE_WEIGHT * title_score)
        + (ARTIST_WEIGHT * artist_score)
        + (DURATION_WEIGHT * duration_score);
    (total >= DEFAULT_THRESHOLD).then_some(total)
}

fn title_similarity(song: &SongProfile, local: &LocalProfile) -> f64 {
    if local.title.is_empty() {
        return 0.0;
    }
    song.titles
        .iter()
        .filter(|title| !title.is_empty())
        .map(|title| similarity(title, &local.title))
        .fold(0.0, f64::max)
}

fn artist_similarity(song_artists: &[MatchText], local_artists: &[MatchText]) -> f64 {
    if song_artists.is_empty() || local_artists.is_empty() {
        // 未知歌手信息时保持中性，避免因为缺标签否定整首歌曲。
        return 0.5;
    }

    let intersection = local_artists
        .iter()
        .filter(|artist| song_artists.contains(artist))
        .count();
    if intersection > 0 {
        if intersection == song_artists.len() && intersection == local_artists.len() {
            return 1.0;
        }
        let dice = 2.0 * intersection as f64 / (song_artists.len() + local_artists.len()) as f64;
        return 0.75 + (0.25 * dice);
    }

    // 没有完全相同项时给「近似歌手名」较弱的加分。
    let mut best = 0.0f64;
    for left in song_artists {
        for right in local_artists {
            best = best.max(similarity(left, right));
        }
    }
    best * 0.6
}

/// 时长相似度：完全一致为 1，差值达到容差为 0；任一方未知时返回中性 0.5。
pub fn duration_similarity(song_duration_ms: i64, local_duration_ms: i64) -> f64 {
    if song_duration_ms <= 0 || local_duration_ms <= 0 {
        return 0.5;
    }
    let difference = (song_duration_ms - local_duration_ms).abs();
    if difference >= DURATION_TOLERANCE_MS {
        return 0.0;
    }
    1.0 - (difference as f64 / DURATION_TOLERANCE_MS as f64)
}
