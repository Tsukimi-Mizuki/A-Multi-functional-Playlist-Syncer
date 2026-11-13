//! 联网搜索服务：网易云走现有的 `NeteaseClient`，QQ 音乐走公开搜索 / 歌词接口。
//! 对应 C# `IOnlineMusicSearchService` / `OnlineMusicSearchService`。

use std::sync::Arc;
use std::time::Duration;

use super::model::{OnlineError, OnlineMusicSource, OnlineSongDetails, OnlineSongResult};
use super::net::{CoverFetch, OnlineHttp};
use super::qq::{QqClient, parse_year};
use crate::library::text::is_blank;
use crate::lyrics::parser;
use crate::netease::cover::cover_download_url;
use crate::netease::http::{BoxFuture, HttpTransport};
use crate::netease::models::NeteaseSong;
use crate::netease::{NeteaseClient, NeteaseError};
use crate::utilities::{CancelToken, cancellable};

/// 单次请求的独立超时（与 C# 的 `requestTimeout` 默认值一致）。
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// 默认的搜索条数（C# `SearchAsync` 的默认 `limit = 20`）。
pub const DEFAULT_SEARCH_LIMIT: usize = 20;

/// 网易云 / QQ 音乐联网搜索与歌词读取。
pub trait OnlineMusicSearch: Send + Sync {
    /// 搜索歌曲。关键词为空白时返回空列表。
    fn search<'a>(
        &'a self,
        source: OnlineMusicSource,
        keyword: &'a str,
        limit: usize,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Vec<OnlineSongResult>, OnlineError>>;

    /// 取一首歌的标签字段、歌词与封面。拿不到可用信息（ID 非法等）返回 `Ok(None)`。
    fn get_details<'a>(
        &'a self,
        song: &'a OnlineSongResult,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Option<OnlineSongDetails>, OnlineError>>;

    /// 只取标签字段与封面，**不请求歌词**；`with_cover` 为假时连封面也不下载。
    ///
    /// 默认实现取完整详情再去掉歌词；真实实现覆盖它，从一开始就不发歌词请求。
    fn get_tag_details<'a>(
        &'a self,
        song: &'a OnlineSongResult,
        with_cover: bool,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Option<OnlineSongDetails>, OnlineError>> {
        Box::pin(async move {
            let details = self.get_details(song, cancel).await?;
            Ok(details.map(|details| OnlineSongDetails {
                cover_data: details.cover_data.filter(|_| with_cover),
                ..OnlineSongDetails::without_extras(details.song)
            }))
        })
    }

    /// 这首歌的封面下载下来会是多少像素（宽，高）；没有封面或认不出来返回 `Ok(None)`。
    fn cover_size<'a>(
        &'a self,
        _song: &'a OnlineSongResult,
        _cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Option<(u32, u32)>, OnlineError>> {
        Box::pin(async { Ok(None) })
    }
}

/// 真实实现。
pub struct OnlineMusicSearchService {
    netease: Arc<NeteaseClient>,
    qq: QqClient,
    http: OnlineHttp,
}

impl OnlineMusicSearchService {
    /// 创建服务；`transport` 供 QQ 音乐与封面下载使用，网易云请求走 `netease` 自己的传输。
    pub fn new(netease: Arc<NeteaseClient>, transport: Arc<dyn HttpTransport>) -> Self {
        Self::with_timeout(netease, transport, REQUEST_TIMEOUT)
    }

    /// 同 [`new`](Self::new)，可指定单次请求的独立超时。
    pub fn with_timeout(
        netease: Arc<NeteaseClient>,
        transport: Arc<dyn HttpTransport>,
        timeout: Duration,
    ) -> Self {
        let http = OnlineHttp::new(transport, timeout);
        Self {
            netease,
            qq: QqClient::new(http.clone()),
            http,
        }
    }

    async fn search_netease(
        &self,
        keyword: &str,
        limit: usize,
        cancel: &CancelToken,
    ) -> Result<Vec<OnlineSongResult>, OnlineError> {
        let songs = cancellable(cancel, self.netease.search_songs(keyword, limit))
            .await?
            .map_err(describe_netease_error)?;
        Ok(songs.iter().map(from_netease).collect())
    }

    /// 网易云的详情；`with_lyrics` / `with_cover` 为假时不发对应的请求。
    async fn netease_details(
        &self,
        summary: &OnlineSongResult,
        with_lyrics: bool,
        with_cover: bool,
        cancel: &CancelToken,
    ) -> Result<Option<OnlineSongDetails>, OnlineError> {
        let Some(song_id) = summary
            .source_id
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|id| *id > 0)
        else {
            return Ok(None);
        };

        // 详情接口失败时，搜索摘要仍可用于填充标签；歌词单独尝试。
        let detail = match cancellable(cancel, self.netease.get_song_details(&[song_id])).await? {
            Ok(details) => details.into_iter().next(),
            Err(_) => None,
        };

        let song = match detail {
            None => summary.clone(),
            Some(detail) => merge_netease_detail(summary, &detail),
        };

        let mut details = OnlineSongDetails::without_extras(song);
        // 没有歌词权限 / 数据时仍可应用标签和封面。
        if with_lyrics
            && let Ok(raw) = cancellable(cancel, self.netease.get_lyrics(song_id)).await?
        {
            let parsed = parser::parse(&raw);
            let has_word = parsed.has_word_level();
            details.line_lyrics = Some(parser::to_plain_lrc(&parsed, false));
            details.word_lyrics = has_word.then(|| parser::to_enhanced_lrc(&parsed, false));
            if parsed.translation.is_some() {
                details.bilingual_line_lyrics = Some(parser::to_plain_lrc(&parsed, true));
                details.bilingual_word_lyrics =
                    has_word.then(|| parser::to_enhanced_lrc(&parsed, true));
            }
        }

        if with_cover {
            details.cover_data = self
                .http
                .try_download_cover(
                    details
                        .song
                        .cover_url
                        .as_deref()
                        .map(cover_download_url)
                        .as_deref(),
                    cancel,
                )
                .await?;
        }
        Ok(Some(details))
    }

    /// 网易云封面的像素尺寸：探测的是下载时用的那个地址（最长边已经限到上限）。
    async fn netease_cover_size(
        &self,
        song: &OnlineSongResult,
        cancel: &CancelToken,
    ) -> Result<Option<(u32, u32)>, OnlineError> {
        let Some(url) = song.cover_url.as_deref().filter(|url| !is_blank(url)) else {
            return Ok(None);
        };
        let probe = self
            .http
            .probe_cover_size(&cover_download_url(url), cancel)
            .await?;
        Ok(match probe {
            CoverFetch::Done(size) => Some(size),
            CoverFetch::Rejected | CoverFetch::Failed => None,
        })
    }
}

impl OnlineMusicSearch for OnlineMusicSearchService {
    fn search<'a>(
        &'a self,
        source: OnlineMusicSource,
        keyword: &'a str,
        limit: usize,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Vec<OnlineSongResult>, OnlineError>> {
        Box::pin(async move {
            if is_blank(keyword) {
                return Ok(Vec::new());
            }
            let keyword = keyword.trim();
            match source {
                OnlineMusicSource::Netease => self.search_netease(keyword, limit, cancel).await,
                OnlineMusicSource::QqMusic => self.qq.search(keyword, limit, cancel).await,
            }
        })
    }

    fn get_details<'a>(
        &'a self,
        song: &'a OnlineSongResult,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Option<OnlineSongDetails>, OnlineError>> {
        Box::pin(async move {
            match song.source {
                OnlineMusicSource::Netease => self.netease_details(song, true, true, cancel).await,
                OnlineMusicSource::QqMusic => self.qq.details(song, cancel).await.map(Some),
            }
        })
    }

    fn get_tag_details<'a>(
        &'a self,
        song: &'a OnlineSongResult,
        with_cover: bool,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Option<OnlineSongDetails>, OnlineError>> {
        Box::pin(async move {
            match song.source {
                OnlineMusicSource::Netease => {
                    self.netease_details(song, false, with_cover, cancel).await
                }
                OnlineMusicSource::QqMusic => self
                    .qq
                    .tag_details(song, with_cover, cancel)
                    .await
                    .map(Some),
            }
        })
    }

    fn cover_size<'a>(
        &'a self,
        song: &'a OnlineSongResult,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Option<(u32, u32)>, OnlineError>> {
        Box::pin(async move {
            match song.source {
                OnlineMusicSource::Netease => self.netease_cover_size(song, cancel).await,
                OnlineMusicSource::QqMusic => Ok(self
                    .qq
                    .cover_size(song.cover_url.as_deref(), cancel)
                    .await?),
            }
        })
    }
}

/// 把网易云错误转成给用户看的中文；`code=406` 是搜索限流，单独给出建议。
fn describe_netease_error(error: NeteaseError) -> OnlineError {
    if error.code() == Some(406) && !error.is_http_status() {
        return OnlineError::failed(
            "网易云音乐搜索请求过于频繁（code=406），请稍候再试，或改用 QQ 音乐",
        );
    }
    OnlineError::failed(error.user_message())
}

fn positive(value: i32) -> Option<u32> {
    u32::try_from(value).ok().filter(|value| *value > 0)
}

fn from_netease(song: &NeteaseSong) -> OnlineSongResult {
    OnlineSongResult {
        source: OnlineMusicSource::Netease,
        source_id: song.id.to_string(),
        title: song.name.clone(),
        artist: song.artist_text(),
        artists: song.artist_names(),
        album: song.album.name.clone(),
        duration_ms: i64::from(song.duration_ms),
        year: parse_year(song.release_date.as_deref()),
        track_number: positive(song.track_number),
        disc_number: positive(song.disc_number),
        genre: None,
        composer: None,
        cover_url: song.cover_url().map(str::to_owned),
        numeric_source_id: None,
    }
}

/// 详情接口返回的字段优先，缺失的字段沿用搜索摘要。
fn merge_netease_detail(summary: &OnlineSongResult, detail: &NeteaseSong) -> OnlineSongResult {
    let pick = |detail: String, fallback: &str| {
        if is_blank(&detail) {
            fallback.to_owned()
        } else {
            detail
        }
    };
    OnlineSongResult {
        source: OnlineMusicSource::Netease,
        source_id: summary.source_id.clone(),
        title: pick(detail.name.clone(), &summary.title),
        artist: pick(detail.artist_text(), &summary.artist),
        // 歌手清单跟着歌手文字走：详情里有歌手就用详情的，否则沿用摘要的。
        artists: if is_blank(&detail.artist_text()) {
            summary.artists.clone()
        } else {
            detail.artist_names()
        },
        album: pick(detail.album.name.clone(), &summary.album),
        duration_ms: if detail.duration_ms > 0 {
            i64::from(detail.duration_ms)
        } else {
            summary.duration_ms
        },
        year: parse_year(detail.release_date.as_deref()).or(summary.year),
        track_number: positive(detail.track_number).or(summary.track_number),
        disc_number: positive(detail.disc_number).or(summary.disc_number),
        genre: summary.genre.clone(),
        composer: summary.composer.clone(),
        cover_url: detail
            .cover_url()
            .filter(|url| !is_blank(url))
            .map(str::to_owned)
            .or_else(|| summary.cover_url.clone()),
        numeric_source_id: summary.numeric_source_id.clone(),
    }
}
