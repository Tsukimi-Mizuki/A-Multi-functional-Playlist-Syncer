//! 在线标签：同一个关键词同时搜网易云与 QQ 音乐，取一条候选的封面尺寸与标签 / 封面。
//!
//! 与 Tauri 外壳一样，编排与打分都在 `lp-core` 的 `OnlineManager` 里，桥接层只把 DTO 摊平成
//! frb 能传的类型：frb 的 `Result` 错误位只能放字符串，带 `kind` 的错误要放进返回值里。

use std::sync::Arc;

use lp_core::models::LyricsSourceKind;
use lp_core::online::manager::{
    FallbackRequest, FileContext, LoadRequest, SearchRequest, TagCandidatesRequest, TagLoadRequest,
};
use lp_core::online::model::{OnlineMusicSource, OnlineSongResult};
use lp_core::online::view::{
    CoverSizeView, FallbackLyricsView, LoadedResultView, LoadedTagsView, MatchScoreView,
    OnlineCommandError, OnlineSearchItem, OnlineSearchResponse, TagCandidateView,
    TagCandidatesResponse,
};
use lp_core::utilities::CancelToken;

use crate::state::{bridge, run};

/// 一首在线歌曲（候选行与详情）。来源用 `Netease` / `QQMusic` 两个固定字符串。
pub struct OnlineSongDto {
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub artist: String,
    pub artists: Vec<String>,
    pub album: String,
    pub duration_ms: i64,
    pub year: Option<i64>,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    pub genre: Option<String>,
    pub composer: Option<String>,
    pub cover_url: Option<String>,
    pub numeric_source_id: Option<String>,
    /// 来源名（「网易云音乐」「QQ 音乐」），界面直接显示。
    pub source_label: String,
    /// 列表里显示的歌名（空白时显示「未知歌曲」）。
    pub display_title: String,
    /// `m:ss`。
    pub duration_text: String,
}

fn source_name(source: OnlineMusicSource) -> String {
    match source {
        OnlineMusicSource::Netease => "Netease".to_owned(),
        OnlineMusicSource::QqMusic => "QQMusic".to_owned(),
    }
}

fn source_kind(source: OnlineMusicSource) -> LyricsSourceKind {
    match source {
        OnlineMusicSource::Netease => LyricsSourceKind::Netease,
        OnlineMusicSource::QqMusic => LyricsSourceKind::QqMusic,
    }
}

fn parse_source(name: &str) -> Result<OnlineMusicSource, String> {
    match name {
        "Netease" => Ok(OnlineMusicSource::Netease),
        "QQMusic" => Ok(OnlineMusicSource::QqMusic),
        other => Err(format!("无法识别的在线音乐来源：{other}")),
    }
}

fn number(value: Option<u32>) -> Option<i64> {
    value.map(i64::from)
}

fn small_number(value: Option<i64>) -> Option<u32> {
    value.and_then(|value| u32::try_from(value).ok())
}

impl From<&OnlineSongResult> for OnlineSongDto {
    fn from(song: &OnlineSongResult) -> Self {
        Self {
            source: source_name(song.source),
            source_id: song.source_id.clone(),
            title: song.title.clone(),
            artist: song.artist.clone(),
            artists: song.artists.clone(),
            album: song.album.clone(),
            duration_ms: song.duration_ms,
            year: number(song.year),
            track_number: number(song.track_number),
            disc_number: number(song.disc_number),
            genre: song.genre.clone(),
            composer: song.composer.clone(),
            cover_url: song.cover_url.clone(),
            numeric_source_id: song.numeric_source_id.clone(),
            source_label: song.source_label().to_owned(),
            display_title: song.display_title().to_owned(),
            duration_text: song.duration_text(),
        }
    }
}

impl OnlineSongDto {
    /// 还原成 `lp-core` 的搜索摘要；界面上带回来的派生字段（来源名、显示名、时长文本）会丢掉。
    fn to_song(&self) -> Result<OnlineSongResult, String> {
        Ok(OnlineSongResult {
            source: parse_source(&self.source)?,
            source_id: self.source_id.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            artists: self.artists.clone(),
            album: self.album.clone(),
            duration_ms: self.duration_ms,
            year: small_number(self.year),
            track_number: small_number(self.track_number),
            disc_number: small_number(self.disc_number),
            genre: self.genre.clone(),
            composer: self.composer.clone(),
            cover_url: self.cover_url.clone(),
            numeric_source_id: self.numeric_source_id.clone(),
        })
    }
}

/// 打分明细（四路分值 + 总分）。
pub struct MatchScoreDto {
    pub title: f64,
    pub artist: f64,
    pub album: f64,
    pub duration: f64,
    pub total: f64,
    pub percent: String,
    pub rejected: bool,
    pub reject_reason: Option<String>,
    pub confident: bool,
}

impl From<&MatchScoreView> for MatchScoreDto {
    fn from(score: &MatchScoreView) -> Self {
        Self {
            title: score.title,
            artist: score.artist,
            album: score.album,
            duration: score.duration,
            total: score.total,
            percent: score.percent.clone(),
            rejected: score.rejected,
            reject_reason: score.reject_reason.clone(),
            confident: score.confident,
        }
    }
}

/// 候选列表里的一行。
pub struct TagCandidateDto {
    pub song: OnlineSongDto,
    pub score: Option<MatchScoreDto>,
    /// 总分低于设置里的匹配度阈值。
    pub below_threshold: bool,
    /// 封面缩略图地址（小图）。
    pub thumbnail_url: Option<String>,
}

impl From<&TagCandidateView> for TagCandidateDto {
    fn from(candidate: &TagCandidateView) -> Self {
        Self {
            song: OnlineSongDto::from(&candidate.item.song.song),
            score: candidate.item.score.as_ref().map(MatchScoreDto::from),
            below_threshold: candidate.item.below_threshold,
            thumbnail_url: candidate.thumbnail_url.clone(),
        }
    }
}

/// `online_tag_candidates` 的结果。
pub struct TagCandidatesDto {
    pub query: String,
    pub scored: bool,
    pub threshold: f64,
    pub threshold_percent: String,
    pub items: Vec<TagCandidateDto>,
    /// 搜索失败的来源（「网易云音乐：……」）；另一个来源的结果照常返回。
    pub failures: Vec<String>,
}

impl From<TagCandidatesResponse> for TagCandidatesDto {
    fn from(response: TagCandidatesResponse) -> Self {
        Self {
            query: response.query,
            scored: response.scored,
            threshold: response.threshold,
            threshold_percent: response.threshold_percent,
            items: response.items.iter().map(TagCandidateDto::from).collect(),
            failures: response.failures,
        }
    }
}

/// 在线命令的失败：`kind` 供界面分支，`message` 可直接展示。
pub struct OnlineErrorDto {
    pub kind: String,
    pub message: String,
}

impl From<OnlineCommandError> for OnlineErrorDto {
    fn from(error: OnlineCommandError) -> Self {
        Self {
            kind: error.kind,
            message: error.message,
        }
    }
}

/// 候选搜索的结果：数据或错误。
pub struct TagCandidatesResultDto {
    pub data: Option<TagCandidatesDto>,
    pub error: Option<OnlineErrorDto>,
}

impl TagCandidatesResultDto {
    fn ok(data: TagCandidatesDto) -> Self {
        Self {
            data: Some(data),
            error: None,
        }
    }

    fn failed(error: impl Into<OnlineErrorDto>) -> Self {
        Self {
            data: None,
            error: Some(error.into()),
        }
    }
}

/// 封面的像素尺寸。
pub struct CoverSizeDto {
    pub width: i64,
    pub height: i64,
}

impl From<CoverSizeView> for CoverSizeDto {
    fn from(size: CoverSizeView) -> Self {
        Self {
            width: i64::from(size.width),
            height: i64::from(size.height),
        }
    }
}

/// 封面尺寸探测的结果：数据或错误；`size` 为 `None` 表示没有封面或认不出尺寸。
pub struct CoverSizeResultDto {
    pub size: Option<CoverSizeDto>,
    pub error: Option<OnlineErrorDto>,
}

impl CoverSizeResultDto {
    fn ok(size: Option<CoverSizeDto>) -> Self {
        Self { size, error: None }
    }
}

/// 标签详情里的封面（Base64，不带 `data:` 前缀）。
pub struct CoverInfoDto {
    pub mime_type: String,
    pub base64: String,
    pub size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

/// 一条候选的标签字段与封面（**没有歌词**）。
pub struct LoadedTagsDto {
    pub song: OnlineSongDto,
    pub cover: Option<CoverInfoDto>,
}

impl From<LoadedTagsView> for LoadedTagsDto {
    fn from(view: LoadedTagsView) -> Self {
        Self {
            song: OnlineSongDto::from(&view.song.song),
            cover: view.cover.map(|cover| CoverInfoDto {
                mime_type: cover.mime_type,
                base64: cover.base64,
                size: i64::try_from(cover.size).unwrap_or(i64::MAX),
                width: number(cover.width),
                height: number(cover.height),
            }),
        }
    }
}

/// `online_load_tags` 的结果：数据或错误。
pub struct LoadedTagsResultDto {
    pub data: Option<LoadedTagsDto>,
    pub error: Option<OnlineErrorDto>,
}

impl LoadedTagsResultDto {
    fn ok(data: LoadedTagsDto) -> Self {
        Self {
            data: Some(data),
            error: None,
        }
    }

    fn failed(error: OnlineErrorDto) -> Self {
        Self {
            data: None,
            error: Some(error),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::failed(OnlineErrorDto {
            kind: "invalid".to_owned(),
            message: message.into(),
        })
    }
}

/// 本地文件信息，用来给候选打分（编辑器打开了文件才有）。
pub struct FileContextDto {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: i64,
    pub file_path: Option<String>,
}

impl From<FileContextDto> for FileContext {
    fn from(context: FileContextDto) -> Self {
        Self {
            title: context.title,
            artist: context.artist,
            album: context.album,
            duration_ms: context.duration_ms,
            file_path: context.file_path,
        }
    }
}

/// 同时搜索网易云与 QQ 音乐，返回合并后按匹配度排序的候选（每个来源最多 10 条）。
pub async fn online_tag_candidates(
    query: String,
    file_context: Option<FileContextDto>,
) -> Result<TagCandidatesResultDto, String> {
    let bridge = bridge()?;
    let online = Arc::clone(&bridge.online);
    run(async move {
        let request = TagCandidatesRequest {
            query,
            file_context: file_context.map(FileContext::from),
        };
        match online.tag_candidates(request, &CancelToken::new()).await {
            Ok(response) => TagCandidatesResultDto::ok(TagCandidatesDto::from(response)),
            Err(error) => TagCandidatesResultDto::failed(error),
        }
    })
    .await
}

/// 一条候选的封面下载下来是多少像素（只读图片开头）；没有封面或认不出时为 `None`。
pub async fn online_cover_size(song: OnlineSongDto) -> Result<CoverSizeResultDto, String> {
    let bridge = bridge()?;
    let online = Arc::clone(&bridge.online);
    let song = match song.to_song() {
        Ok(song) => song,
        Err(message) => {
            return Ok(CoverSizeResultDto {
                size: None,
                error: Some(OnlineErrorDto {
                    kind: "invalid".to_owned(),
                    message,
                }),
            });
        }
    };
    run(async move {
        match online.cover_size(&song, &CancelToken::new()).await {
            Ok(size) => CoverSizeResultDto::ok(size.map(CoverSizeDto::from)),
            Err(error) => CoverSizeResultDto {
                size: None,
                error: Some(OnlineErrorDto::from(error)),
            },
        }
    })
    .await
}

/// 取一条候选的标签字段与封面，不取歌词；`cover` 为 `false` 时连封面也不下载。
pub async fn online_load_tags(
    song: OnlineSongDto,
    cover: bool,
) -> Result<LoadedTagsResultDto, String> {
    let bridge = bridge()?;
    let online = Arc::clone(&bridge.online);
    let summary = song.to_song();
    run(async move {
        let summary = match summary {
            Ok(summary) => summary,
            Err(message) => return LoadedTagsResultDto::invalid(message),
        };
        let request = TagLoadRequest {
            source: source_kind(summary.source),
            id: summary.source_id.clone(),
            song: Some(summary),
            cover,
        };
        match online.load_tags(request, &CancelToken::new()).await {
            Ok(view) => LoadedTagsResultDto::ok(LoadedTagsDto::from(view)),
            Err(error) => LoadedTagsResultDto::failed(OnlineErrorDto::from(error)),
        }
    })
    .await
}

/// 歌词来源的固定字符串（与设置文件、lp-core 的 serde 名一致）。
fn kind_name(kind: LyricsSourceKind) -> &'static str {
    match kind {
        LyricsSourceKind::Netease => "Netease",
        LyricsSourceKind::QqMusic => "QQMusic",
        LyricsSourceKind::Kugou => "Kugou",
        LyricsSourceKind::Lrclib => "Lrclib",
    }
}

fn parse_kind(name: &str) -> Result<LyricsSourceKind, String> {
    match name {
        "Netease" => Ok(LyricsSourceKind::Netease),
        "QQMusic" => Ok(LyricsSourceKind::QqMusic),
        "Kugou" => Ok(LyricsSourceKind::Kugou),
        "Lrclib" => Ok(LyricsSourceKind::Lrclib),
        other => Err(format!("无法识别的歌词来源：{other}")),
    }
}

/// 编辑器打开时恢复的在线搜索选项：上次的来源、歌词格式与当前匹配度阈值。
pub struct EditorOptionsDto {
    pub search_source: String,
    pub prefer_word_lyrics: bool,
    pub threshold: f64,
    pub threshold_percent: String,
}

/// 关键词搜索里的一行。
pub struct OnlineSearchItemDto {
    pub song: OnlineSongDto,
    pub score: Option<MatchScoreDto>,
    /// 总分低于设置里的匹配度阈值。
    pub below_threshold: bool,
}

impl From<&OnlineSearchItem> for OnlineSearchItemDto {
    fn from(item: &OnlineSearchItem) -> Self {
        Self {
            song: OnlineSongDto::from(&item.song.song),
            score: item.score.as_ref().map(MatchScoreDto::from),
            below_threshold: item.below_threshold,
        }
    }
}

/// online_search 的结果。
pub struct OnlineSearchResultDto {
    pub source: String,
    pub query: String,
    pub scored: bool,
    pub threshold: f64,
    pub threshold_percent: String,
    pub items: Vec<OnlineSearchItemDto>,
}

impl From<OnlineSearchResponse> for OnlineSearchResultDto {
    fn from(response: OnlineSearchResponse) -> Self {
        Self {
            source: kind_name(response.source).to_owned(),
            query: response.query,
            scored: response.scored,
            threshold: response.threshold,
            threshold_percent: response.threshold_percent,
            items: response
                .items
                .iter()
                .map(OnlineSearchItemDto::from)
                .collect(),
        }
    }
}

/// online_search 的结果：数据或错误。
pub struct OnlineSearchOutcomeDto {
    pub data: Option<OnlineSearchResultDto>,
    pub error: Option<OnlineErrorDto>,
}

impl OnlineSearchOutcomeDto {
    fn ok(data: OnlineSearchResultDto) -> Self {
        Self {
            data: Some(data),
            error: None,
        }
    }

    fn failed(error: impl Into<OnlineErrorDto>) -> Self {
        Self {
            data: None,
            error: Some(error.into()),
        }
    }
}

fn cover_info(cover: Option<&lp_core::online::view::CoverView>) -> Option<CoverInfoDto> {
    cover.map(|cover| CoverInfoDto {
        mime_type: cover.mime_type.clone(),
        base64: cover.base64.clone(),
        size: i64::try_from(cover.size).unwrap_or(i64::MAX),
        width: number(cover.width),
        height: number(cover.height),
    })
}

/// 一首歌的标签字段、歌词与封面（online_load_result）。
pub struct LoadedResultDto {
    pub song: OnlineSongDto,
    pub lyrics: Option<String>,
    pub lyrics_label: Option<String>,
    /// 请求了逐字但只有逐行。
    pub word_fallback: bool,
    pub has_word_lyrics: bool,
    pub has_translation: bool,
    pub line_lyrics: Option<String>,
    pub word_lyrics: Option<String>,
    pub bilingual_line_lyrics: Option<String>,
    pub bilingual_word_lyrics: Option<String>,
    pub cover: Option<CoverInfoDto>,
    /// 与 Tauri 版一致的状态栏文案。
    pub status_text: String,
}

impl From<LoadedResultView> for LoadedResultDto {
    fn from(view: LoadedResultView) -> Self {
        let cover = cover_info(view.cover.as_ref());
        Self {
            song: OnlineSongDto::from(&view.song.song),
            lyrics: view.lyrics,
            lyrics_label: view.lyrics_label,
            word_fallback: view.word_fallback,
            has_word_lyrics: view.has_word_lyrics,
            has_translation: view.has_translation,
            line_lyrics: view.line_lyrics,
            word_lyrics: view.word_lyrics,
            bilingual_line_lyrics: view.bilingual_line_lyrics,
            bilingual_word_lyrics: view.bilingual_word_lyrics,
            cover,
            status_text: view.status_text,
        }
    }
}

/// online_load_result 的结果：数据或错误。
pub struct LoadedResultOutcomeDto {
    pub data: Option<LoadedResultDto>,
    pub error: Option<OnlineErrorDto>,
}

impl LoadedResultOutcomeDto {
    fn ok(data: LoadedResultDto) -> Self {
        Self {
            data: Some(data),
            error: None,
        }
    }

    fn failed(error: impl Into<OnlineErrorDto>) -> Self {
        Self {
            data: None,
            error: Some(error.into()),
        }
    }
}

/// 备用词源（酷狗 / LRCLIB）找到的一份歌词。
pub struct FallbackLyricsDto {
    pub source_label: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: i64,
    pub score: MatchScoreDto,
    pub lyrics: Option<String>,
    pub lyrics_label: Option<String>,
    pub has_word_lyrics: bool,
    pub has_translation: bool,
    pub line_lyrics: Option<String>,
    pub word_lyrics: Option<String>,
    pub bilingual_line_lyrics: Option<String>,
    pub bilingual_word_lyrics: Option<String>,
    pub status_text: String,
}

impl From<FallbackLyricsView> for FallbackLyricsDto {
    fn from(view: FallbackLyricsView) -> Self {
        Self {
            source_label: view.source_label,
            title: view.title,
            artist: view.artist,
            album: view.album,
            duration_ms: view.duration_ms,
            score: MatchScoreDto::from(&view.score),
            lyrics: view.lyrics,
            lyrics_label: view.lyrics_label,
            has_word_lyrics: view.has_word_lyrics,
            has_translation: view.has_translation,
            line_lyrics: view.line_lyrics,
            word_lyrics: view.word_lyrics,
            bilingual_line_lyrics: view.bilingual_line_lyrics,
            bilingual_word_lyrics: view.bilingual_word_lyrics,
            status_text: view.status_text,
        }
    }
}

/// lyrics_source_fetch 的结果：数据（没有匹配时为 None）或错误。
pub struct FallbackLyricsOutcomeDto {
    pub data: Option<FallbackLyricsDto>,
    pub error: Option<OnlineErrorDto>,
}

impl FallbackLyricsOutcomeDto {
    fn ok(data: Option<FallbackLyricsDto>) -> Self {
        Self { data, error: None }
    }

    fn failed(error: impl Into<OnlineErrorDto>) -> Self {
        Self {
            data: None,
            error: Some(error.into()),
        }
    }
}

/// 编辑器打开时要恢复的在线搜索选项（来源、歌词格式、匹配度阈值）。
pub fn editor_options() -> Result<EditorOptionsDto, String> {
    let bridge = bridge()?;
    let settings = bridge.settings();
    let threshold = bridge.online.threshold_view();
    Ok(EditorOptionsDto {
        search_source: kind_name(settings.lyrics_editor_search_source).to_owned(),
        prefer_word_lyrics: settings.lyrics_editor_prefer_word_lyrics,
        threshold: threshold.threshold,
        threshold_percent: threshold.percent,
    })
}

/// 记住编辑器里选的来源与歌词格式（只改这两个设置；失败只写运行日志）。
pub fn editor_remember_options(source: String, prefer_word_lyrics: bool) -> Result<bool, String> {
    let kind = parse_kind(&source)?;
    bridge()?.remember_editor_options(kind, prefer_word_lyrics)
}

/// 主词源（网易云 / QQ 音乐）按关键词搜歌；带本地文件信息时按匹配度排序。
pub async fn online_search(
    source: String,
    query: String,
    file_context: Option<FileContextDto>,
) -> Result<OnlineSearchOutcomeDto, String> {
    let kind = parse_kind(&source)?;
    let bridge = bridge()?;
    let online = Arc::clone(&bridge.online);
    run(async move {
        let request = SearchRequest {
            source: kind,
            query,
            file_context: file_context.map(FileContext::from),
        };
        match online.search(request, &CancelToken::new()).await {
            Ok(response) => OnlineSearchOutcomeDto::ok(OnlineSearchResultDto::from(response)),
            Err(error) => OnlineSearchOutcomeDto::failed(OnlineErrorDto::from(error)),
        }
    })
    .await
}

/// 取一条搜索结果的标签字段、歌词与封面；song 不传时按来源与 ID 从最近搜索结果里找。
pub async fn online_load_result(
    source: String,
    id: String,
    word_lyrics: bool,
    song: Option<OnlineSongDto>,
) -> Result<LoadedResultOutcomeDto, String> {
    let kind = parse_kind(&source)?;
    let bridge = bridge()?;
    let online = Arc::clone(&bridge.online);
    let summary = match song {
        Some(song) => match song.to_song() {
            Ok(song) => Some(song),
            Err(message) => {
                return Ok(LoadedResultOutcomeDto::failed(OnlineErrorDto {
                    kind: "invalid".to_owned(),
                    message,
                }));
            }
        },
        None => None,
    };
    run(async move {
        let request = LoadRequest {
            source: kind,
            id,
            word_lyrics,
            song: summary,
        };
        match online.load_result(request, &CancelToken::new()).await {
            Ok(view) => LoadedResultOutcomeDto::ok(LoadedResultDto::from(view)),
            Err(error) => LoadedResultOutcomeDto::failed(OnlineErrorDto::from(error)),
        }
    })
    .await
}

/// 备用词源按当前音频的标题、歌手与时长检索一份歌词（只补歌词，不改标签与封面）。
pub async fn lyrics_source_fetch(
    source: String,
    title: String,
    artist: String,
    duration_ms: i64,
    album: Option<String>,
    word_lyrics: bool,
) -> Result<FallbackLyricsOutcomeDto, String> {
    let kind = parse_kind(&source)?;
    let bridge = bridge()?;
    let online = Arc::clone(&bridge.online);
    run(async move {
        let request = FallbackRequest {
            source: kind,
            title,
            artist,
            album,
            duration_ms,
            word_lyrics,
        };
        match online.fetch_fallback(request, &CancelToken::new()).await {
            Ok(Some(view)) => FallbackLyricsOutcomeDto::ok(Some(FallbackLyricsDto::from(view))),
            Ok(None) => FallbackLyricsOutcomeDto::ok(None),
            Err(error) => FallbackLyricsOutcomeDto::failed(OnlineErrorDto::from(error)),
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> OnlineSongResult {
        OnlineSongResult {
            source: OnlineMusicSource::QqMusic,
            source_id: "003abc".to_owned(),
            title: "夜に駆ける".to_owned(),
            artist: "YOASOBI".to_owned(),
            artists: vec!["YOASOBI".to_owned()],
            album: "THE BOOK".to_owned(),
            duration_ms: 261_000,
            year: Some(2019),
            track_number: Some(1),
            disc_number: Some(1),
            genre: Some("流行".to_owned()),
            composer: None,
            cover_url: Some("https://example.com/cover.jpg".to_owned()),
            numeric_source_id: Some("12345".to_owned()),
        }
    }

    #[test]
    fn song_survives_a_round_trip_through_dart() {
        let dto = OnlineSongDto::from(&sample());
        assert_eq!(dto.source, "QQMusic");
        assert_eq!(dto.source_label, "QQ 音乐");
        assert_eq!(dto.year, Some(2019));

        let restored = dto.to_song().expect("回来");
        assert_eq!(restored, sample());
    }

    #[test]
    fn unknown_source_is_rejected() {
        let mut dto = OnlineSongDto::from(&sample());
        dto.source = "Kugou".to_owned();
        let error = dto.to_song().expect_err("备用词源不是主来源");
        assert!(error.contains("Kugou"), "{error}");
    }

    #[test]
    fn oversized_numbers_are_dropped_instead_of_wrapping() {
        let mut dto = OnlineSongDto::from(&sample());
        dto.year = Some(i64::MAX);
        assert_eq!(dto.to_song().expect("回来").year, None);
    }

    #[test]
    fn error_kind_is_kept_for_the_ui() {
        let error = OnlineErrorDto::from(OnlineCommandError {
            kind: "emptyQuery".to_owned(),
            message: "请输入要搜索的歌曲、歌手或专辑。".to_owned(),
        });
        assert_eq!(error.kind, "emptyQuery");

        let result = LoadedTagsResultDto::invalid("无法识别的在线音乐来源：不知道");
        assert!(result.data.is_none());
        assert_eq!(result.error.as_ref().unwrap().kind, "invalid");
    }

    #[test]
    fn lyrics_sources_round_trip_through_dart() {
        for kind in [
            LyricsSourceKind::Netease,
            LyricsSourceKind::QqMusic,
            LyricsSourceKind::Kugou,
            LyricsSourceKind::Lrclib,
        ] {
            assert_eq!(parse_kind(kind_name(kind)).expect("回来"), kind);
        }
    }

    #[test]
    fn unknown_lyrics_source_is_rejected() {
        let error = parse_kind("Spotify").expect_err("不认识的来源");
        assert!(error.contains("Spotify"), "{error}");
    }

    #[test]
    fn search_item_keeps_the_score_and_the_flag() {
        let item = OnlineSearchItem {
            song: lp_core::online::view::OnlineSongView::from(&sample()),
            score: Some(MatchScoreView {
                title: 1.0,
                artist: 1.0,
                album: 0.5,
                duration: 1.0,
                total: 0.93,
                percent: "93".to_owned(),
                rejected: false,
                reject_reason: None,
                confident: true,
            }),
            below_threshold: false,
        };
        let dto = OnlineSearchItemDto::from(&item);
        assert_eq!(dto.song.source, "QQMusic");
        assert_eq!(dto.score.as_ref().unwrap().percent, "93");
        assert!(!dto.below_threshold);
    }
}
