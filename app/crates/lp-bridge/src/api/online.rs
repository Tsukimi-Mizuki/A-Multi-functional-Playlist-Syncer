//! 在线标签：同一个关键词同时搜网易云与 QQ 音乐，取一条候选的封面尺寸与标签 / 封面。
//!
//! 与 Tauri 外壳一样，编排与打分都在 `lp-core` 的 `OnlineManager` 里，桥接层只把 DTO 摊平成
//! frb 能传的类型：frb 的 `Result` 错误位只能放字符串，带 `kind` 的错误要放进返回值里。

use std::sync::Arc;

use lp_core::models::LyricsSourceKind;
use lp_core::online::manager::{FileContext, TagCandidatesRequest, TagLoadRequest};
use lp_core::online::model::{OnlineMusicSource, OnlineSongResult};
use lp_core::online::view::{
    CoverSizeView, LoadedTagsView, MatchScoreView, OnlineCommandError, TagCandidateView,
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
}
