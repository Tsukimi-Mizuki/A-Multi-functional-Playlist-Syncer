//! 批量标签 / 封面 / 歌词匹配。对应 C# `BatchLyricsService` / `IBatchLyricsService`。
//!
//! 扫描本地音频，用标签 / 文件名搜索网易云或 QQ 音乐，按「音樂標籤」的思路做多变体搜索与四路打分
//! （标题 / 歌手 / 专辑 / 时长）重排，用匹配度阈值筛选后，把标题、歌手、专辑、年份、音轨 / 碟号、风格、作曲家
//! 和封面写回标签。网易云 / QQ 都没有可用歌词时，再按顺序尝试备用词源（酷狗 KRC、LRCLIB），
//! 让冷门曲目与同人音乐也能补上逐行 / 逐字歌词。
//!
//! 模块 7（NCM 导入）与模块 9（下载 / 同步）留下的 [`OnlineEnricher`] 挂钩由 [`BatchEnricher`] 实现：
//! 固定走 [`BatchLyricsOptions::auto_enrich`]（标签与封面网易云优先、歌词 QQ 逐字优先，覆盖已有标签与歌词，
//! 封面只换成更清楚的），匹配度阈值、“是否取歌词”与“是否写封面”每次调用时从**当前设置**读取。
//!
//! ## 与 C# 的差异
//!
//! - 扫描结果按小写完整路径排序（C# 用当前区域的不区分大小写比较，中文区域下的拼音顺序无法逐位复现）；
//!   不跟随目录符号链接；读不了的子目录跳过。
//! - 取消用 [`CancelToken`]：在每次网络请求处立刻生效，标签写入（阻塞线程里）开始前检查一次。
//! - 标签读写的 IO 错误以 [`BatchError::Failed`] 返回（中文原因），调用方转成各自的“处理失败 / 在线匹配失败”文案。

mod task;

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use regex::Regex;
use serde::Serialize;

use super::scorer;
use crate::download::ncm_import::{EnrichError, OnlineEnricher, OnlineMatchOutcome};
use crate::library::text::is_blank;
use crate::models::{LyricsWriteMode, OnlineSourcePriority};
use crate::netease::http::BoxFuture;
use crate::online::{
    LyricsQuery, LyricsSource, LyricsSourceCandidate, OnlineError, OnlineMusicSearch,
    OnlineMusicSource, OnlineSongDetails, OnlineSongResult,
};
use crate::settings::AppSettings;
use crate::tags::text::image_dimensions;
use crate::tags::{AudioTagData, AudioTagInfo, AudioTagService};
use crate::utilities::{CancelToken, Cancelled, cancellable};

pub use task::{
    BatchFileView, BatchJobError, BatchManager, BatchProgressView, BatchRequest, BatchResultView,
    BatchScanView, BatchSummaryView,
};

/// 批量处理支持的扩展名。
pub const AUDIO_EXTENSIONS: [&str; 8] = [
    ".mp3", ".flac", ".m4a", ".ogg", ".wav", ".aac", ".wma", ".opus",
];

/// 每个音源最多尝试几组搜索关键词（多变体搜索的成本控制）。
pub const MAX_QUERIES_PER_SOURCE: usize = 3;

/// 每次搜索请求的候选数量。
pub const SEARCH_LIMIT: usize = 10;

/// 批量处理的并发数。
pub const BATCH_CONCURRENCY: usize = 3;

/// 批量匹配的选项。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatchLyricsOptions {
    /// 标签 / 封面的来源次序。
    pub tag_priority: OnlineSourcePriority,
    /// 歌词的来源次序。
    pub lyrics_priority: OnlineSourcePriority,
    /// 覆盖已有的标签、封面与歌词。
    pub overwrite_existing: bool,
    /// 是否获取歌词。
    pub fetch_lyrics: bool,
    /// 优先逐字歌词。
    pub prefer_word_lyrics: bool,
    /// 匹配度阈值（0.30–1.00，超出范围会被夹回）。
    pub min_score: f64,
    /// 是否写入封面。
    pub write_cover: bool,
    /// 覆盖已有封面时只换成像素更多的（避免把清楚的封面换成模糊的）；文件里没有封面时照常写入。
    pub keep_larger_cover: bool,
}

impl Default for BatchLyricsOptions {
    fn default() -> Self {
        Self {
            tag_priority: OnlineSourcePriority::NeteaseFirst,
            lyrics_priority: OnlineSourcePriority::NeteaseFirst,
            overwrite_existing: false,
            fetch_lyrics: true,
            prefer_word_lyrics: false,
            min_score: scorer::DEFAULT_MIN_SCORE,
            write_cover: true,
            keep_larger_cover: false,
        }
    }
}

impl BatchLyricsOptions {
    /// 下载、同步与 NCM 解锁后自动补全用：标签与封面网易云音乐优先（原图更清楚），其次 QQ 音乐；
    /// 歌词 QQ 音乐逐字优先，其次网易云音乐。覆盖已有标签与歌词，封面只换成像素更多的。
    pub fn auto_enrich() -> Self {
        Self {
            tag_priority: OnlineSourcePriority::NeteaseFirst,
            lyrics_priority: OnlineSourcePriority::QqMusicFirst,
            overwrite_existing: true,
            fetch_lyrics: true,
            prefer_word_lyrics: true,
            min_score: scorer::DEFAULT_MIN_SCORE,
            write_cover: true,
            keep_larger_cover: true,
        }
    }

    /// 自动补全选项 + 当前设置里的匹配度阈值、“是否取歌词”（歌词写入方式为「不写」时只补标签与封面）
    /// 与“是否写封面”（关掉「写入封面」时不写封面）。
    pub fn auto_enrich_with(settings: &AppSettings) -> Self {
        Self {
            min_score: settings.match_threshold,
            fetch_lyrics: settings.lyrics_mode != LyricsWriteMode::None,
            write_cover: settings.write_cover,
            ..Self::auto_enrich()
        }
    }
}

/// 一个文件的处理结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchLyricsResult {
    /// 文件完整路径。
    pub file_path: String,
    /// 是否写入成功。
    pub success: bool,
    /// 是否因为已经完整而跳过。
    pub skipped: bool,
    /// 给用户看的说明（中文）。
    pub message: String,
    /// 匹配到的歌名。
    pub matched_title: Option<String>,
    /// 匹配到的歌手。
    pub matched_artist: Option<String>,
    /// 来源名（「网易云音乐」「QQ 音乐」「酷狗音乐」「LRCLIB」）。
    pub source_label: Option<String>,
    /// 写入的歌词格式（如「双语逐字歌词」）；没有歌词时为空串。
    pub lyrics_format: Option<String>,
}

impl BatchLyricsResult {
    fn fail(path: &str, message: impl Into<String>) -> Self {
        Self {
            file_path: path.to_owned(),
            success: false,
            skipped: false,
            message: message.into(),
            matched_title: None,
            matched_artist: None,
            source_label: None,
            lyrics_format: None,
        }
    }
}

/// 处理失败（不同于“没找到匹配”这种正常结果）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchError {
    /// 调用方取消了。
    Cancelled,
    /// 标签读写等失败，附中文原因。
    Failed(String),
}

impl std::fmt::Display for BatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("操作已取消"),
            Self::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for BatchError {}

impl From<Cancelled> for BatchError {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

impl From<OnlineError> for BatchError {
    fn from(error: OnlineError) -> Self {
        match error {
            OnlineError::Cancelled => Self::Cancelled,
            OnlineError::Failed(message) => Self::Failed(message),
        }
    }
}

/// 运行期过滤备用词源：返回 `false` 时跳过该源（对应设置里的「备用词源」开关）。
pub type FallbackFilter = Arc<dyn Fn(&dyn LyricsSource) -> bool + Send + Sync>;

/// 批量标签与封面服务。
pub struct BatchLyricsService {
    tags: Arc<dyn AudioTagService>,
    online: Arc<dyn OnlineMusicSearch>,
    fallback_sources: Vec<Arc<dyn LyricsSource>>,
    filter: RwLock<Option<FallbackFilter>>,
}

/// 只凭歌曲信息（不读文件）检索歌词时的输入，例如歌单里的歌曲做歌词预览。
#[derive(Debug, Clone, Copy)]
pub struct SongLookup<'a> {
    /// 歌名。
    pub title: &'a str,
    /// 歌手（多个以 `/` 连接）。
    pub artist: &'a str,
    /// 专辑。
    pub album: Option<&'a str>,
    /// 时长（毫秒），未知为 0。
    pub duration_ms: i64,
}

/// 本地歌曲的检索信息。
struct LocalSong<'a> {
    title: &'a str,
    artist: &'a str,
    album: Option<&'a str>,
    duration_ms: i64,
    queries: &'a [String],
}

impl<'a> LocalSong<'a> {
    fn from_lookup(song: &SongLookup<'a>, queries: &'a [String]) -> Self {
        Self {
            title: song.title,
            artist: song.artist,
            album: song.album,
            duration_ms: song.duration_ms,
            queries,
        }
    }
}

impl BatchLyricsService {
    /// 创建服务；`fallback_sources` 是备用词源（酷狗、LRCLIB…），按顺序询问。
    pub fn new(
        tags: Arc<dyn AudioTagService>,
        online: Arc<dyn OnlineMusicSearch>,
        fallback_sources: Vec<Arc<dyn LyricsSource>>,
    ) -> Self {
        Self {
            tags,
            online,
            fallback_sources,
            filter: RwLock::new(None),
        }
    }

    /// 设置备用词源过滤器；`None` 表示全部启用。随时可改，下一次需要备用词源时生效。
    pub fn set_fallback_filter(&self, filter: Option<FallbackFilter>) {
        *self
            .filter
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = filter;
    }

    /// 备用词源（用于编辑器按名字查找）。
    pub fn fallback_sources(&self) -> &[Arc<dyn LyricsSource>] {
        &self.fallback_sources
    }

    fn fallback_allowed(&self, source: &dyn LyricsSource) -> bool {
        let filter = self
            .filter
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        filter.is_none_or(|filter| filter(source))
    }

    /// 扫描目录里的音频文件（同步，调用方在阻塞线程里调用）；目录为空白或不存在时返回空列表。
    pub fn scan_audio_files(directory: &str, recursive: bool) -> Vec<PathBuf> {
        if is_blank(directory) || !Path::new(directory.trim()).is_dir() {
            return Vec::new();
        }

        let mut files: Vec<PathBuf> = Vec::new();
        let mut pending = vec![PathBuf::from(directory.trim())];
        while let Some(current) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&current) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                let path = entry.path();
                if file_type.is_dir() {
                    if recursive {
                        pending.push(path);
                    }
                } else if has_audio_extension(&path) {
                    files.push(path);
                }
            }
        }
        files.sort_by_cached_key(|path| path.to_string_lossy().to_lowercase());
        files
    }

    async fn read_tags(&self, path: &Path) -> Result<Option<AudioTagInfo>, BatchError> {
        let tags = Arc::clone(&self.tags);
        let target = path.to_path_buf();
        match tokio::task::spawn_blocking(move || tags.read(&target)).await {
            Ok(Ok(info)) => Ok(info),
            Ok(Err(error)) => Err(BatchError::Failed(error.to_string())),
            Err(_) => Err(BatchError::Failed(
                "读取标签时程序内部出错，请重试。".to_owned(),
            )),
        }
    }

    async fn write_tags(&self, path: &Path, data: AudioTagData) -> Result<(), BatchError> {
        let tags = Arc::clone(&self.tags);
        let target = path.to_path_buf();
        match tokio::task::spawn_blocking(move || tags.write(&target, &data)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(BatchError::Failed(error.to_string())),
            Err(_) => Err(BatchError::Failed(
                "写入标签时程序内部出错，请重试。".to_owned(),
            )),
        }
    }

    /// 为一个文件匹配在线标签 / 封面 / 歌词并写回。
    ///
    /// “没找到 / 已完整 / 文件不能读”都是正常结果（`Ok`，`success == false`）；
    /// 取消与标签读写失败是 `Err`。
    pub async fn fetch_for_file(
        &self,
        file_path: &Path,
        options: &BatchLyricsOptions,
        cancel: &CancelToken,
    ) -> Result<BatchLyricsResult, BatchError> {
        let path_text = file_path.to_string_lossy().into_owned();
        if !file_path.is_file() {
            return Ok(BatchLyricsResult::fail(&path_text, "文件不存在"));
        }
        if cancel.is_cancelled() {
            return Err(BatchError::Cancelled);
        }

        let Some(info) = self.read_tags(file_path).await? else {
            return Ok(BatchLyricsResult::fail(&path_text, "无法读取音频标签"));
        };

        let (local_title, local_artist) = resolve_local_info(file_path, &info);
        if is_blank(&local_title) {
            return Ok(BatchLyricsResult::fail(
                &path_text,
                "无法从标签或文件名识别歌曲名",
            ));
        }

        let queries = scorer::build_queries(Some(&local_title), Some(&local_artist));
        let local_album = info.album.as_deref().map(str::trim);
        let min_score = options
            .min_score
            .clamp(scorer::MIN_THRESHOLD, scorer::MAX_THRESHOLD);
        let local = LocalSong {
            title: &local_title,
            artist: &local_artist,
            album: local_album,
            duration_ms: info.duration_ms,
            queries: &queries,
        };

        let tag_details = self
            .find_best_details(
                &local,
                &order_sources(options.tag_priority),
                false,
                false,
                min_score,
                cancel,
            )
            .await?;

        let replace = options.overwrite_existing;
        let should_fetch_lyrics =
            options.fetch_lyrics && (replace || info.lyrics.as_deref().is_none_or(is_blank));
        let lyrics_details = if should_fetch_lyrics {
            self.find_best_details(
                &local,
                &order_sources(options.lyrics_priority),
                true,
                options.prefer_word_lyrics,
                min_score,
                cancel,
            )
            .await?
        } else {
            None
        };

        // 网易云 / QQ 都没有可用歌词时，再按顺序问备用词源（酷狗、LRCLIB）。
        let online_lyrics = lyrics_details
            .as_ref()
            .and_then(|details| details.get_lyrics(options.prefer_word_lyrics, true))
            .filter(|text| !is_blank(text))
            .map(str::to_owned);
        let fallback_lyrics = if should_fetch_lyrics
            && online_lyrics.is_none()
            && !self.fallback_sources.is_empty()
        {
            self.find_fallback_lyrics(&local, options.prefer_word_lyrics, min_score, cancel)
                .await?
        } else {
            None
        };

        let metadata_source = tag_details.as_ref().or(lyrics_details.as_ref());
        if metadata_source.is_none() && fallback_lyrics.is_none() {
            return Ok(BatchLyricsResult::fail(
                &path_text,
                "未找到匹配度足够的在线歌曲",
            ));
        }

        let tag_song: Option<&OnlineSongResult> = metadata_source.map(|details| &details.song);
        let source_label = tag_song.map_or_else(
            || {
                fallback_lyrics
                    .as_ref()
                    .map(|candidate| candidate.source_label.clone())
            },
            |song| Some(song.source_label().to_owned()),
        );
        let cover = tag_details
            .as_ref()
            .and_then(|details| details.cover_data.clone())
            .or_else(|| {
                lyrics_details
                    .as_ref()
                    .and_then(|details| details.cover_data.clone())
            });

        let mut lyrics: Option<String> = None;
        let mut lyrics_format = String::new();
        let mut writes_lyrics = false;
        let mut use_word = false;
        if should_fetch_lyrics {
            if let (Some(online), Some(details)) = (&online_lyrics, &lyrics_details) {
                lyrics = Some(online.clone());
                use_word = options.prefer_word_lyrics && details.has_word_lyrics();
                writes_lyrics = true;
                lyrics_format = format_label(use_word, details.has_translation());
            } else if let Some(candidate) = &fallback_lyrics {
                let text = candidate.get_lyrics(options.prefer_word_lyrics, true);
                if let Some(text) = text.filter(|text| !is_blank(text)) {
                    lyrics = Some(text.to_owned());
                    use_word = options.prefer_word_lyrics && candidate.has_word_lyrics();
                    writes_lyrics = true;
                    lyrics_format = format_label(use_word, candidate.has_translation());
                }
            }
        }

        let tag_title = tag_song
            .map(|song| song.title.clone())
            .or_else(|| fallback_lyrics.as_ref().map(|c| c.title.clone()));
        let tag_artist = tag_song
            .map(|song| song.artist.clone())
            .or_else(|| fallback_lyrics.as_ref().map(|c| c.artist.clone()));
        let tag_album = tag_song
            .map(|song| song.album.clone())
            .or_else(|| fallback_lyrics.as_ref().map(|c| c.album.clone()));

        let tag_data = AudioTagData {
            title: (replace || info.title.as_deref().is_none_or(is_blank))
                .then(|| null_if_blank(tag_title.as_deref()))
                .flatten(),
            artists: if replace || info.artist.as_deref().is_none_or(is_blank) {
                // 来源给了歌手清单就原样用（名字里带 `/` 的歌手不会被拆开）；没有才按分隔符拆。
                tag_song
                    .and_then(OnlineSongResult::artist_values)
                    .unwrap_or_else(|| split_artists(tag_artist.as_deref()))
            } else {
                Vec::new()
            },
            album: (replace || info.album.as_deref().is_none_or(is_blank))
                .then(|| null_if_blank(tag_album.as_deref()))
                .flatten(),
            year: (replace || info.year.is_none())
                .then(|| tag_song.and_then(|song| song.year))
                .flatten(),
            track_number: (replace || info.track_number.is_none())
                .then(|| tag_song.and_then(|song| song.track_number))
                .flatten(),
            disc_number: (replace || info.disc_number.is_none())
                .then(|| tag_song.and_then(|song| song.disc_number))
                .flatten(),
            genre: (replace || info.genre.as_deref().is_none_or(is_blank))
                .then(|| null_if_blank(tag_song.and_then(|song| song.genre.as_deref())))
                .flatten(),
            composer: (replace || info.composer.as_deref().is_none_or(is_blank))
                .then(|| null_if_blank(tag_song.and_then(|song| song.composer.as_deref())))
                .flatten(),
            cover_data: cover.filter(|cover| {
                options.write_cover
                    && (!info.has_cover
                        || (replace
                            && (!options.keep_larger_cover
                                || is_larger_cover(cover, info.cover_data.as_deref()))))
            }),
            plain_lyrics: if writes_lyrics && !use_word {
                lyrics.clone()
            } else {
                None
            },
            word_lyrics: if writes_lyrics && use_word {
                lyrics.clone()
            } else {
                None
            },
            ..AudioTagData::default()
        };

        let writes_tags = tag_data.title.is_some()
            || !tag_data.artists.is_empty()
            || tag_data.album.is_some()
            || tag_data.year.is_some()
            || tag_data.track_number.is_some()
            || tag_data.disc_number.is_some()
            || tag_data.genre.is_some()
            || tag_data.composer.is_some();
        let writes_cover = tag_data
            .cover_data
            .as_ref()
            .is_some_and(|data| !data.is_empty());

        if !writes_tags && !writes_cover && !writes_lyrics {
            return Ok(BatchLyricsResult {
                file_path: path_text,
                success: false,
                skipped: true,
                message: "标签、封面与歌词已完整，跳过".to_owned(),
                matched_title: tag_title,
                matched_artist: tag_artist,
                source_label,
                lyrics_format: Some(lyrics_format),
            });
        }

        if cancel.is_cancelled() {
            return Err(BatchError::Cancelled);
        }
        self.write_tags(file_path, tag_data).await?;

        let mut parts: Vec<&str> = Vec::new();
        if writes_tags {
            parts.push("标签");
        }
        if writes_cover {
            parts.push("封面");
        }
        if writes_lyrics {
            parts.push(&lyrics_format);
        }

        let mut message = format!("已写入{}", parts.join("、"));
        // 备用词源的歌词总写明来源；网易云 / QQ 的歌词与标签来源不同时也写明（来源名默认指标签的来源）。
        let lyrics_label = match (&online_lyrics, &lyrics_details, &fallback_lyrics) {
            (Some(_), Some(details), _) => Some(details.song.source_label())
                .filter(|label| source_label.as_deref() != Some(*label)),
            (None, _, Some(candidate)) => Some(candidate.source_label.as_str()),
            _ => None,
        };
        if writes_lyrics && let Some(label) = lyrics_label {
            message.push_str(&format!("（歌词来源：{label}）"));
        }
        if should_fetch_lyrics && !writes_lyrics {
            message.push_str("（未获取到歌词）");
        }

        Ok(BatchLyricsResult {
            file_path: path_text,
            success: true,
            skipped: false,
            message,
            matched_title: tag_title,
            matched_artist: tag_artist,
            source_label,
            lyrics_format: Some(lyrics_format),
        })
    }

    /// 凭歌曲信息在一个主词源里找带歌词的匹配：多变体搜索 + 打分，规则与批量处理相同，不读写任何文件。
    /// 没有达到匹配度阈值、或匹配到的歌曲没有歌词时返回 `None`。
    pub async fn find_source_lyrics(
        &self,
        song: &SongLookup<'_>,
        source: OnlineMusicSource,
        prefer_word: bool,
        min_score: f64,
        cancel: &CancelToken,
    ) -> Result<Option<OnlineSongDetails>, BatchError> {
        let queries = scorer::build_queries(Some(song.title), Some(song.artist));
        let local = LocalSong::from_lookup(song, &queries);
        let min_score = min_score.clamp(scorer::MIN_THRESHOLD, scorer::MAX_THRESHOLD);
        self.find_best_details(&local, &[source], true, prefer_word, min_score, cancel)
            .await
    }

    /// 凭歌曲信息依次询问备用词源（酷狗、LRCLIB…），同样受备用词源开关约束。
    pub async fn find_fallback_song_lyrics(
        &self,
        song: &SongLookup<'_>,
        prefer_word: bool,
        min_score: f64,
        cancel: &CancelToken,
    ) -> Result<Option<LyricsSourceCandidate>, BatchError> {
        let queries = scorer::build_queries(Some(song.title), Some(song.artist));
        let local = LocalSong::from_lookup(song, &queries);
        let min_score = min_score.clamp(scorer::MIN_THRESHOLD, scorer::MAX_THRESHOLD);
        self.find_fallback_lyrics(&local, prefer_word, min_score, cancel)
            .await
    }

    /// 依次询问备用词源：某个源出错或没有可用歌词时继续下一个，全部失败返回 `None`。
    /// 备用源只补歌词，不改写标签来源，因此不影响原有的标签 / 封面匹配逻辑。
    async fn find_fallback_lyrics(
        &self,
        local: &LocalSong<'_>,
        prefer_word: bool,
        min_score: f64,
        cancel: &CancelToken,
    ) -> Result<Option<LyricsSourceCandidate>, BatchError> {
        for source in &self.fallback_sources {
            if !self.fallback_allowed(source.as_ref()) {
                continue;
            }

            let query = LyricsQuery {
                queries: local.queries,
                title: local.title,
                artist: local.artist,
                album: local.album,
                duration_ms: local.duration_ms,
                min_score,
            };
            let candidate = match source.fetch(&query, cancel).await {
                Ok(candidate) => candidate,
                Err(OnlineError::Cancelled) => return Err(BatchError::Cancelled),
                Err(OnlineError::Failed(_)) => continue,
            };

            let Some(candidate) = candidate else {
                continue;
            };
            if candidate.get_lyrics(prefer_word, true).is_none_or(is_blank) {
                continue;
            }
            return Ok(Some(candidate));
        }
        Ok(None)
    }

    async fn find_best_details(
        &self,
        local: &LocalSong<'_>,
        sources: &[OnlineMusicSource],
        require_lyrics: bool,
        prefer_word: bool,
        min_score: f64,
        cancel: &CancelToken,
    ) -> Result<Option<OnlineSongDetails>, BatchError> {
        for source in sources {
            let Some(best) = self
                .find_best_candidate(*source, local, min_score, cancel)
                .await?
            else {
                continue;
            };

            let details = match self.online.get_details(&best, cancel).await {
                Ok(details) => details,
                Err(OnlineError::Cancelled) => return Err(BatchError::Cancelled),
                Err(OnlineError::Failed(_)) => continue,
            };
            let Some(details) = details else {
                continue;
            };

            if require_lyrics && details.get_lyrics(prefer_word, true).is_none_or(is_blank) {
                continue;
            }
            return Ok(Some(details));
        }
        Ok(None)
    }

    /// 多变体搜索 + 打分重排（「音樂標籤」思路）：按关键词顺序搜索同一音源，
    /// 标题、歌手、专辑、时长四路打分取最高分候选；达到可信匹配就不再换关键词，
    /// 全部关键词都低于匹配度阈值时返回 `None`。
    async fn find_best_candidate(
        &self,
        source: OnlineMusicSource,
        local: &LocalSong<'_>,
        min_score: f64,
        cancel: &CancelToken,
    ) -> Result<Option<OnlineSongResult>, BatchError> {
        let mut best: Option<OnlineSongResult> = None;
        let mut best_score = 0.0_f64;
        for (attempts, query) in local.queries.iter().enumerate() {
            if attempts >= MAX_QUERIES_PER_SOURCE {
                break;
            }

            let candidates = match cancellable(
                cancel,
                self.online.search(source, query, SEARCH_LIMIT, cancel),
            )
            .await?
            {
                Ok(candidates) => candidates,
                Err(OnlineError::Cancelled) => return Err(BatchError::Cancelled),
                Err(OnlineError::Failed(_)) => continue,
            };

            for candidate in candidates {
                let score = scorer::score(
                    Some(local.title),
                    Some(local.artist),
                    local.album,
                    local.duration_ms,
                    Some(&candidate.title),
                    Some(&candidate.artist),
                    Some(&candidate.album),
                    candidate.duration_ms,
                );
                if score.rejected {
                    continue;
                }
                if score.total > best_score {
                    best_score = score.total;
                    best = Some(candidate);
                }
            }

            // 标题与歌手都可信时没必要继续换关键词，省掉多余请求。
            if let Some(current) = &best
                && best_score >= min_score
                && scorer::title_score(Some(local.title), Some(&current.title))
                    >= scorer::GOOD_TITLE_SCORE
                && scorer::artist_score(Some(local.artist), Some(&current.artist))
                    >= scorer::RELIABLE_ARTIST_SCORE
            {
                break;
            }
        }

        Ok(if best_score >= min_score { best } else { None })
    }
}

/// 按来源优先级排出要尝试的主词源。
pub fn order_sources(priority: OnlineSourcePriority) -> Vec<OnlineMusicSource> {
    match priority {
        OnlineSourcePriority::QqMusicFirst => {
            vec![OnlineMusicSource::QqMusic, OnlineMusicSource::Netease]
        }
        OnlineSourcePriority::NeteaseOnly => vec![OnlineMusicSource::Netease],
        OnlineSourcePriority::QqMusicOnly => vec![OnlineMusicSource::QqMusic],
        OnlineSourcePriority::NeteaseFirst => {
            vec![OnlineMusicSource::Netease, OnlineMusicSource::QqMusic]
        }
    }
}

/// 歌词格式的中文名（写进结果消息与编辑器提示）。
pub fn format_label(use_word: bool, has_translation: bool) -> String {
    match (use_word, has_translation) {
        (true, true) => "双语逐字歌词",
        (true, false) => "逐字歌词",
        (false, true) => "双语逐行歌词",
        (false, false) => "逐行歌词",
    }
    .to_owned()
}

fn has_audio_extension(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let extension = format!(".{}", extension.to_string_lossy());
        AUDIO_EXTENSIONS
            .iter()
            .any(|supported| supported.eq_ignore_ascii_case(&extension))
    })
}

fn null_if_blank(value: Option<&str>) -> Option<String> {
    value.filter(|text| !is_blank(text)).map(|text| {
        text.trim_matches(crate::library::text::is_dotnet_whitespace)
            .to_owned()
    })
}

/// 把 `a/b；c、d` 之类的歌手串拆成列表（C# 这里只拆 `/ ; ； 、`）。
fn split_artists(value: Option<&str>) -> Vec<String> {
    let Some(value) = value.filter(|text| !is_blank(text)) else {
        return Vec::new();
    };
    value
        .split(['/', ';', '；', '、'])
        .map(|part| part.trim_matches(crate::library::text::is_dotnet_whitespace))
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 新封面的像素是否比已有封面多；认不出尺寸的新封面不算，认不出尺寸的旧封面按 0 算。
fn is_larger_cover(new: &[u8], existing: Option<&[u8]>) -> bool {
    let area = |data: &[u8]| image_dimensions(data).map_or(0, |(w, h)| u64::from(w) * u64::from(h));
    let new_area = area(new);
    new_area > 0 && new_area > existing.map_or(0, area)
}

/// 本地歌曲的标题与歌手：标签优先；没有标题时用文件名（去掉音轨前缀、下划线换空格）。
///
/// 文件名没有标签时无法可靠判断“歌手 - 标题”的顺序，直接用整个文件名作为标题，
/// 由搜索和相似度匹配处理，避免把标题和歌手拆反。
fn resolve_local_info(path: &Path, info: &AudioTagInfo) -> (String, String) {
    let trim = |text: &str| {
        text.trim_matches(crate::library::text::is_dotnet_whitespace)
            .to_owned()
    };
    let title = info.title.as_deref().map(trim).unwrap_or_default();
    let artist = info.artist.as_deref().map(trim).unwrap_or_default();
    if !is_blank(&title) {
        return (title, artist);
    }

    let name = path
        .file_stem()
        .map(|stem| trim(&stem.to_string_lossy()))
        .unwrap_or_default();
    (title_from_file_name(&name), artist)
}

/// 去掉常见的 `01 - ` / `01.` 音轨前缀并把下划线替换为空格。
pub fn title_from_file_name(name: &str) -> String {
    static TRACK_PREFIX: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"^\s*\d{1,3}[\s._-]+").expect("音轨前缀正则"));
    let without_prefix = TRACK_PREFIX.replace(name, "");
    without_prefix
        .replace('_', " ")
        .trim_matches(crate::library::text::is_dotnet_whitespace)
        .to_owned()
}

// ───────────────────────────── 自动补全挂钩 ─────────────────────────────

/// 读取当前设置的函数（每次补全时调用，所以设置改动立即生效）。
pub type SettingsProvider = Arc<dyn Fn() -> AppSettings + Send + Sync>;

/// 把批量服务接成模块 7 / 9 的在线补全挂钩。
pub struct BatchEnricher {
    service: Arc<BatchLyricsService>,
    settings: SettingsProvider,
}

impl BatchEnricher {
    /// 创建补全器。
    pub fn new(service: Arc<BatchLyricsService>, settings: SettingsProvider) -> Self {
        Self { service, settings }
    }
}

impl OnlineEnricher for BatchEnricher {
    fn enrich<'a>(
        &'a self,
        audio_path: &'a Path,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<OnlineMatchOutcome, EnrichError>> {
        Box::pin(async move {
            let options = BatchLyricsOptions::auto_enrich_with(&(self.settings)());
            match self
                .service
                .fetch_for_file(audio_path, &options, cancel)
                .await
            {
                Ok(result) => Ok(OnlineMatchOutcome {
                    success: result.success,
                    skipped: result.skipped,
                    message: result.message,
                    matched_title: result.matched_title,
                    matched_artist: result.matched_artist,
                    source_label: result.source_label,
                }),
                Err(BatchError::Cancelled) => Err(EnrichError::Cancelled),
                Err(BatchError::Failed(reason)) => Err(EnrichError::Failed(reason)),
            }
        })
    }
}

#[cfg(test)]
mod tests;
