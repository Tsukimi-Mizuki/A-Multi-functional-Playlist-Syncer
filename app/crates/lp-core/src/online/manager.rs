//! 歌词与标签编辑器背后的在线命令编排。对应 C# `LyricsEditorViewModel` 里与界面无关的部分：
//! 搜索 + 按本地文件打分排序、取详情（标签 / 封面 / 歌词）、备用词源检索、匹配度阈值。
//!
//! 命令本身是无状态的；唯一的状态是“最近搜索结果”的小缓存，让 `online_load_result` 只传来源与 ID 也能找回
//! QQ 音乐取歌词所需的完整摘要（标题、歌手、专辑、时长、数值 ID、封面地址）。

use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::pin::pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::Poll;

use serde::Deserialize;

use super::model::{
    LyricsQuery, LyricsSource, OnlineError, OnlineMusicSource, OnlineSongResult, music_source_of,
};
use super::service::{DEFAULT_SEARCH_LIMIT, OnlineMusicSearch};
use super::view::{
    CoverSizeView, FallbackLyricsView, LoadedResultView, LoadedTagsView, MatchScoreView,
    MatchThresholdView, OnlineCommandError, OnlineSearchItem, OnlineSearchResponse, OnlineSongView,
    TagCandidateView, TagCandidatesResponse, thumbnail_url,
};
use crate::library::text::is_blank;
use crate::lyrics::batch::{SettingsProvider, title_from_file_name};
use crate::lyrics::scorer;
use crate::models::LyricsSourceKind;
use crate::utilities::CancelToken;

/// 最近搜索结果缓存的容量。
const CACHE_CAPACITY: usize = 200;

/// 在线标签候选：每个来源最多取这么多条（两个来源合起来一屏多一点，够挑）。
pub const TAG_CANDIDATES_PER_SOURCE: usize = 10;

/// 本地文件的信息，用来给搜索结果打分（编辑器打开了文件才有）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContext {
    /// 本地标题（标签里的；可为空，此时用文件名推）。
    #[serde(default)]
    pub title: Option<String>,
    /// 本地歌手。
    #[serde(default)]
    pub artist: Option<String>,
    /// 本地专辑。
    #[serde(default)]
    pub album: Option<String>,
    /// 本地时长（毫秒），未知为 0。
    #[serde(default)]
    pub duration_ms: i64,
    /// 文件路径（标题为空时用文件名推标题 / 默认关键词）。
    #[serde(default)]
    pub file_path: Option<String>,
}

impl FileContext {
    /// 用来打分的本地标题：标签标题 → 文件名推出的标题。
    fn scoring_title(&self) -> Option<String> {
        if let Some(title) = self.title.as_deref().filter(|title| !is_blank(title)) {
            return Some(title.to_owned());
        }
        self.keyword_from_file_name()
    }

    /// 从文件名推出的搜索关键词（去掉音轨前缀、下划线换空格、空白折叠）。
    fn keyword_from_file_name(&self) -> Option<String> {
        let path = self.file_path.as_deref().filter(|path| !is_blank(path))?;
        let stem = std::path::Path::new(path)
            .file_stem()?
            .to_string_lossy()
            .into_owned();
        let name = title_from_file_name(stem.trim());
        // 与 C# 一致：折叠连续空白。
        let collapsed = name.split_whitespace().collect::<Vec<_>>().join(" ");
        (!collapsed.is_empty()).then_some(collapsed)
    }
}

/// `online_search` 的请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    /// 搜索来源（只能是主词源 `Netease` / `QQMusic`）。
    pub source: LyricsSourceKind,
    /// 关键词；留空且带了文件上下文时用文件名推出。
    #[serde(default)]
    pub query: String,
    /// 本地文件信息；带了就给结果打分并按总分排序。
    #[serde(default)]
    pub file_context: Option<FileContext>,
}

/// `online_tag_candidates` 的请求。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCandidatesRequest {
    /// 关键词；留空且带了文件上下文时用文件名推出。
    #[serde(default)]
    pub query: String,
    /// 本地文件信息；带了就给候选打分并按总分排序。
    #[serde(default)]
    pub file_context: Option<FileContext>,
}

fn default_true() -> bool {
    true
}

/// `online_load_tags` 的请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagLoadRequest {
    /// 来源（主词源）。
    pub source: LyricsSourceKind,
    /// 候选里的 `sourceId`。
    pub id: String,
    /// 候选的完整摘要；不传就从最近搜索结果的缓存里按来源与 ID 找。
    #[serde(default)]
    pub song: Option<OnlineSongResult>,
    /// 是否下载封面（默认下载）。
    #[serde(default = "default_true")]
    pub cover: bool,
}

/// `online_load_result` 的请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadRequest {
    /// 来源（主词源）。
    pub source: LyricsSourceKind,
    /// 搜索结果里的 `sourceId`。
    pub id: String,
    /// 优先逐字歌词。
    #[serde(default)]
    pub word_lyrics: bool,
    /// 搜索结果的完整摘要；不传就从最近搜索结果的缓存里按来源与 ID 找。
    #[serde(default)]
    pub song: Option<OnlineSongResult>,
}

/// `lyrics_source_fetch` 的请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FallbackRequest {
    /// 备用词源（`Kugou` / `Lrclib`）。
    pub source: LyricsSourceKind,
    /// 本地标题。
    pub title: String,
    /// 本地歌手。
    #[serde(default)]
    pub artist: String,
    /// 本地专辑（可选，打分时用）。
    #[serde(default)]
    pub album: Option<String>,
    /// 本地时长（毫秒）。
    #[serde(default)]
    pub duration_ms: i64,
    /// 优先逐字歌词。
    #[serde(default)]
    pub word_lyrics: bool,
}

/// 在线命令编排。
pub struct OnlineManager {
    search: Arc<dyn OnlineMusicSearch>,
    fallback_sources: Vec<Arc<dyn LyricsSource>>,
    settings: SettingsProvider,
    cache: Mutex<VecDeque<OnlineSongResult>>,
}

fn failed(error: OnlineError) -> OnlineCommandError {
    match error {
        OnlineError::Cancelled => OnlineCommandError::new("cancelled", "操作已取消"),
        OnlineError::Failed(message) => OnlineCommandError::new("failed", message),
    }
}

/// 同时推进两个 future，都完成后一起返回（两个来源并行搜索，不必一个等另一个）。
async fn join_both<A: Future, B: Future>(first: A, second: B) -> (A::Output, B::Output) {
    let mut first = pin!(first);
    let mut second = pin!(second);
    let mut first_done = None;
    let mut second_done = None;
    poll_fn(|cx| {
        if first_done.is_none()
            && let Poll::Ready(value) = first.as_mut().poll(cx)
        {
            first_done = Some(value);
        }
        if second_done.is_none()
            && let Poll::Ready(value) = second.as_mut().poll(cx)
        {
            second_done = Some(value);
        }
        match (first_done.take(), second_done.take()) {
            (Some(first), Some(second)) => Poll::Ready((first, second)),
            (first, second) => {
                first_done = first;
                second_done = second;
                Poll::Pending
            }
        }
    })
    .await
}

/// 实际要搜的关键词：请求里的 → 文件名推出的；都没有就报「请输入」。
fn resolve_query(query: &str, context: Option<&FileContext>) -> Result<String, OnlineCommandError> {
    let mut query = query.trim().to_owned();
    if query.is_empty() {
        query = context
            .and_then(FileContext::keyword_from_file_name)
            .unwrap_or_default();
    }
    if is_blank(&query) {
        return Err(OnlineCommandError::new(
            "emptyQuery",
            "请输入要搜索的歌曲、歌手或专辑。",
        ));
    }
    Ok(query)
}

/// 给一条搜索结果按本地文件打分；没有文件上下文就不打分。
fn score_item(
    song: &OnlineSongResult,
    context: Option<&FileContext>,
    query: &str,
    threshold: f64,
) -> OnlineSearchItem {
    let Some(context) = context else {
        return OnlineSearchItem {
            song: OnlineSongView::from(song),
            score: None,
            below_threshold: false,
        };
    };
    let title = context.scoring_title().unwrap_or_else(|| query.to_owned());
    let score = scorer::score(
        Some(&title),
        context.artist.as_deref(),
        context.album.as_deref(),
        context.duration_ms,
        Some(&song.title),
        Some(&song.artist),
        Some(&song.album),
        song.duration_ms,
    );
    OnlineSearchItem {
        song: OnlineSongView::from(song),
        below_threshold: score.total < threshold,
        score: Some(MatchScoreView::from(&score)),
    }
}

/// 排序用的总分（没打分记 0）。
fn item_total(item: &OnlineSearchItem) -> f64 {
    item.score.as_ref().map_or(0.0, |score| score.total)
}

impl OnlineManager {
    /// 创建编排器；`settings` 用来读取当前的匹配度阈值。
    pub fn new(
        search: Arc<dyn OnlineMusicSearch>,
        fallback_sources: Vec<Arc<dyn LyricsSource>>,
        settings: SettingsProvider,
    ) -> Self {
        Self {
            search,
            fallback_sources,
            settings,
            cache: Mutex::new(VecDeque::new()),
        }
    }

    fn cache(&self) -> MutexGuard<'_, VecDeque<OnlineSongResult>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn remember(&self, songs: &[OnlineSongResult]) {
        let mut cache = self.cache();
        for song in songs {
            cache.retain(|known| {
                !(known.source == song.source && known.source_id == song.source_id)
            });
            cache.push_back(song.clone());
        }
        while cache.len() > CACHE_CAPACITY {
            cache.pop_front();
        }
    }

    fn recall(&self, source: OnlineMusicSource, id: &str) -> Option<OnlineSongResult> {
        self.cache()
            .iter()
            .rev()
            .find(|song| song.source == source && song.source_id == id)
            .cloned()
    }

    /// 当前设置里的匹配度阈值。
    pub fn threshold(&self) -> f64 {
        (self.settings)()
            .match_threshold
            .clamp(scorer::MIN_THRESHOLD, scorer::MAX_THRESHOLD)
    }

    /// `get_match_threshold`。
    pub fn threshold_view(&self) -> MatchThresholdView {
        MatchThresholdView::new(self.threshold())
    }

    /// 搜索：主词源按关键词搜；带了文件上下文就给每条结果打分并按总分从高到低排序（同分保持原顺序）。
    pub async fn search(
        &self,
        request: SearchRequest,
        cancel: &CancelToken,
    ) -> Result<OnlineSearchResponse, OnlineCommandError> {
        let Some(source) = music_source_of(request.source) else {
            return Err(OnlineCommandError::new(
                "fallbackSource",
                format!(
                    "{}只按当前音频的标题、歌手和时长检索歌词，请改用「备用词源」入口。",
                    request.source.display_label()
                ),
            ));
        };

        let context = request.file_context.as_ref();
        let query = resolve_query(&request.query, context)?;

        let results = self
            .search
            .search(source, &query, DEFAULT_SEARCH_LIMIT, cancel)
            .await
            .map_err(failed)?;
        self.remember(&results);

        let threshold = self.threshold();
        let mut items: Vec<OnlineSearchItem> = results
            .iter()
            .map(|song| score_item(song, context, &query, threshold))
            .collect();
        if context.is_some() {
            // `sort_by` 是稳定排序，同分保持搜索接口给出的顺序（与 C# 的 OrderByDescending 一致）。
            items.sort_by(|left, right| item_total(right).total_cmp(&item_total(left)));
        }

        Ok(OnlineSearchResponse {
            source: request.source,
            query,
            scored: context.is_some(),
            threshold,
            threshold_percent: scorer::format_threshold_percent(threshold),
            items,
        })
    }

    /// 在线标签候选：同一个关键词同时搜网易云与 QQ 音乐（每个来源最多 [`TAG_CANDIDATES_PER_SOURCE`] 条），
    /// 合在一起按匹配度从高到低排（同分网易云在前，它的封面是原图）。只搜索，不取歌词也不下载封面。
    ///
    /// 一个来源失败时照常返回另一个来源的结果，失败原因放进 `failures`；两个都失败才报错。
    pub async fn tag_candidates(
        &self,
        request: TagCandidatesRequest,
        cancel: &CancelToken,
    ) -> Result<TagCandidatesResponse, OnlineCommandError> {
        let context = request.file_context.as_ref();
        let query = resolve_query(&request.query, context)?;

        let sources = [OnlineMusicSource::Netease, OnlineMusicSource::QqMusic];
        let (netease, qq) = join_both(
            self.search
                .search(sources[0], &query, TAG_CANDIDATES_PER_SOURCE, cancel),
            self.search
                .search(sources[1], &query, TAG_CANDIDATES_PER_SOURCE, cancel),
        )
        .await;

        let threshold = self.threshold();
        let mut items: Vec<TagCandidateView> = Vec::new();
        let mut failures: Vec<String> = Vec::new();
        for (source, outcome) in sources.into_iter().zip([netease, qq]) {
            match outcome {
                Err(OnlineError::Cancelled) => return Err(failed(OnlineError::Cancelled)),
                Err(OnlineError::Failed(message)) => {
                    failures.push(format!("{}：{message}", source.label()));
                }
                Ok(mut songs) => {
                    // 来源不一定理会条数上限，这里再截一次。
                    songs.truncate(TAG_CANDIDATES_PER_SOURCE);
                    self.remember(&songs);
                    items.extend(songs.iter().map(|song| TagCandidateView {
                        item: score_item(song, context, &query, threshold),
                        thumbnail_url: thumbnail_url(song),
                    }));
                }
            }
        }
        if failures.len() == sources.len() {
            return Err(OnlineCommandError::new("failed", failures.join("；")));
        }
        if context.is_some() {
            items.sort_by(|left, right| item_total(&right.item).total_cmp(&item_total(&left.item)));
        }

        Ok(TagCandidatesResponse {
            query,
            scored: context.is_some(),
            threshold,
            threshold_percent: scorer::format_threshold_percent(threshold),
            items,
            failures,
        })
    }

    /// 一条候选的封面下载下来是多少像素（只读图片开头，不整张下载）；认不出来返回 `None`。
    pub async fn cover_size(
        &self,
        song: &OnlineSongResult,
        cancel: &CancelToken,
    ) -> Result<Option<CoverSizeView>, OnlineCommandError> {
        let size = self.search.cover_size(song, cancel).await.map_err(failed)?;
        Ok(size.map(|(width, height)| CoverSizeView { width, height }))
    }

    /// 取一条候选的标签字段与封面，不取歌词。
    pub async fn load_tags(
        &self,
        request: TagLoadRequest,
        cancel: &CancelToken,
    ) -> Result<LoadedTagsView, OnlineCommandError> {
        let summary = self.resolve_summary(request.source, &request.id, request.song)?;
        let details = self
            .search
            .get_tag_details(&summary, request.cover, cancel)
            .await
            .map_err(failed)?;
        Ok(LoadedTagsView::new(&summary, details.as_ref()))
    }

    /// 找回一首歌的搜索摘要：请求里带的 → 最近搜索的缓存 → 网易云只凭 ID 也行。
    fn resolve_summary(
        &self,
        kind: LyricsSourceKind,
        id: &str,
        song: Option<OnlineSongResult>,
    ) -> Result<OnlineSongResult, OnlineCommandError> {
        let Some(source) = music_source_of(kind) else {
            return Err(OnlineCommandError::new(
                "primarySource",
                "备用词源只提供歌词，请使用备用词源检索。",
            ));
        };

        song.filter(|song| song.source == source)
            .or_else(|| self.recall(source, id))
            .or_else(|| match source {
                // 网易云的详情接口只需要 ID，没有缓存也能取。
                OnlineMusicSource::Netease => Some(OnlineSongResult {
                    source,
                    source_id: id.to_owned(),
                    title: String::new(),
                    artist: String::new(),
                    artists: Vec::new(),
                    album: String::new(),
                    duration_ms: 0,
                    year: None,
                    track_number: None,
                    disc_number: None,
                    genre: None,
                    composer: None,
                    cover_url: None,
                    numeric_source_id: None,
                }),
                OnlineMusicSource::QqMusic => None,
            })
            .ok_or_else(|| {
                OnlineCommandError::new(
                    "unknownSong",
                    "找不到这首歌的搜索结果，请重新搜索后再选择。",
                )
            })
    }

    /// 取一首歌的标签字段、歌词与封面。
    pub async fn load_result(
        &self,
        request: LoadRequest,
        cancel: &CancelToken,
    ) -> Result<LoadedResultView, OnlineCommandError> {
        let summary = self.resolve_summary(request.source, &request.id, request.song)?;
        let details = self
            .search
            .get_details(&summary, cancel)
            .await
            .map_err(failed)?;
        Ok(LoadedResultView::new(
            &summary,
            details.as_ref(),
            request.word_lyrics,
        ))
    }

    /// 备用词源（酷狗 / LRCLIB）按本地标题、歌手与时长检索一份歌词；没有匹配返回 `None`。
    pub async fn fetch_fallback(
        &self,
        request: FallbackRequest,
        cancel: &CancelToken,
    ) -> Result<Option<FallbackLyricsView>, OnlineCommandError> {
        if !request.source.is_fallback() {
            return Err(OnlineCommandError::new(
                "primarySource",
                "网易云音乐 / QQ 音乐请使用关键词搜索。",
            ));
        }
        if is_blank(&request.title) {
            return Err(OnlineCommandError::new(
                "noFile",
                "备用词源按当前音频的标题、歌手和时长检索，请先打开要编辑的音频文件。",
            ));
        }

        let label = match request.source {
            LyricsSourceKind::Kugou => super::KugouLyricsSource::LABEL,
            _ => super::LrclibLyricsSource::LABEL,
        };
        let Some(source) = self
            .fallback_sources
            .iter()
            .find(|source| source.label() == label)
        else {
            return Err(OnlineCommandError::new(
                "failed",
                format!("{}不可用。", request.source.display_label()),
            ));
        };

        let queries = scorer::build_queries(Some(&request.title), Some(&request.artist));
        let query = LyricsQuery {
            queries: &queries,
            title: &request.title,
            artist: &request.artist,
            album: request.album.as_deref(),
            duration_ms: request.duration_ms,
            min_score: self.threshold(),
        };
        let candidate = source.fetch(&query, cancel).await.map_err(failed)?;
        Ok(candidate.map(|candidate| {
            let score = scorer::score(
                Some(&request.title),
                Some(&request.artist),
                request.album.as_deref(),
                request.duration_ms,
                Some(&candidate.title),
                Some(&candidate.artist),
                Some(&candidate.album),
                candidate.duration_ms,
            );
            FallbackLyricsView::new(&candidate, &score, request.word_lyrics)
        }))
    }
}
