//! LRCLIB 词源：开放歌词库，只有逐行同步歌词（无逐字、无译文），但对冷门曲目、同人音乐覆盖不错，
//! 用作网易云 / QQ / 酷狗之外的第三层兜底。对应 C# `LrclibLyricsSource`。
//!
//! 先按「标题 + 歌手」全文检索（`?q=` 比 `track_name` / `artist_name` 宽松得多，最多 3 个关键词变体），
//! 全部落空后再退回精确的 `track_name` + `artist_name` 查询。429 / 503 属瞬时失败，退避后重试一次。

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use super::model::{LyricsQuery, LyricsSource, LyricsSourceCandidate, OnlineError, SourceFuture};
use super::net::OnlineHttp;
use crate::library::text::is_blank;
use crate::lyrics::scorer::{self, DEFAULT_DURATION_TOLERANCE_MS, MAX_THRESHOLD, MIN_THRESHOLD};
use crate::lyrics::{RawLyrics, parser};
use crate::netease::client::escape_data_string;
use crate::netease::http::{HttpMethod, HttpRequest, HttpTransport};
use crate::utilities::{CancelToken, Cancelled, cancellable};

/// 最多使用几个关键词变体。
const MAX_QUERIES: usize = 3;
const SEARCH_URL: &str = "https://lrclib.net/api/search";

/// 单次检索（含重试等待）的独立超时（C# 默认 10 秒）。
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// 瞬时失败后的退避时间（C# 默认 1200 毫秒）。
pub const RETRY_DELAY: Duration = Duration::from_millis(1200);

/// LRCLIB 词源。
pub struct LrclibLyricsSource {
    http: OnlineHttp,
    retry_delay: Duration,
}

impl LrclibLyricsSource {
    /// 备用词源名称：设置开关与界面提示都按这个名字对应。
    pub const LABEL: &'static str = "LRCLIB";

    /// 用默认超时与退避创建。
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self::with_timing(transport, REQUEST_TIMEOUT, RETRY_DELAY)
    }

    /// 指定超时与瞬时失败后的退避时间（测试用 0）。
    pub fn with_timing(
        transport: Arc<dyn HttpTransport>,
        timeout: Duration,
        retry_delay: Duration,
    ) -> Self {
        Self {
            http: OnlineHttp::new(transport, timeout),
            retry_delay,
        }
    }

    fn query_uris(queries: &[String]) -> Vec<String> {
        queries
            .iter()
            .filter(|query| !is_blank(query))
            .take(MAX_QUERIES)
            .map(|query| format!("{SEARCH_URL}?q={}", escape_data_string(query.trim())))
            .collect()
    }

    fn track_uri(title: &str, artist: &str) -> String {
        let mut url = format!(
            "{SEARCH_URL}?track_name={}",
            escape_data_string(title.trim())
        );
        if !is_blank(artist) {
            url.push_str("&artist_name=");
            url.push_str(&escape_data_string(artist.trim()));
        }
        url
    }

    async fn search(
        &self,
        uris: &[String],
        threshold: f64,
        query: &LyricsQuery<'_>,
        cancel: &CancelToken,
    ) -> Result<Option<LyricsSourceCandidate>, Cancelled> {
        for uri in uris {
            let Some(json) = self.read_json(uri, cancel).await? else {
                continue;
            };
            let Some(items) = json.as_array() else {
                continue;
            };
            if let Some(best) = Self::select_best(items, threshold, query) {
                return Ok(Some(best));
            }
        }
        Ok(None)
    }

    fn select_best(
        items: &[Value],
        threshold: f64,
        query: &LyricsQuery<'_>,
    ) -> Option<LyricsSourceCandidate> {
        let mut best: Option<LyricsSourceCandidate> = None;
        let mut best_score = 0.0_f64;
        for item in items {
            if !item.is_object() {
                continue;
            }
            if item.get("instrumental") == Some(&Value::Bool(true)) {
                continue;
            }

            let Some(synced) = get_string(item, "syncedLyrics").filter(|text| !is_blank(text))
            else {
                continue;
            };

            let track_name =
                get_string(item, "trackName").unwrap_or_else(|| query.title.to_owned());
            let artist_name =
                get_string(item, "artistName").unwrap_or_else(|| query.artist.to_owned());
            let album_name = get_string(item, "albumName");
            let candidate_duration_ms =
                (get_double(item, "duration") * 1000.0).round_ties_even() as i64;
            if !scorer::duration_matches(
                query.duration_ms,
                candidate_duration_ms,
                DEFAULT_DURATION_TOLERANCE_MS,
            ) {
                continue;
            }

            let score = scorer::score(
                Some(query.title),
                Some(query.artist),
                query.album,
                query.duration_ms,
                Some(&track_name),
                Some(&artist_name),
                album_name.as_deref(),
                candidate_duration_ms,
            );
            if score.rejected || score.total < threshold || score.total <= best_score {
                continue;
            }

            let document = parser::parse(&RawLyrics::from_lrc(synced));
            if document.is_empty() {
                continue;
            }

            best_score = score.total;
            best = Some(LyricsSourceCandidate {
                source_label: Self::LABEL.to_owned(),
                title: track_name,
                artist: artist_name,
                album: album_name.unwrap_or_default(),
                duration_ms: candidate_duration_ms,
                line_lyrics: Some(parser::to_plain_lrc(&document, false)),
                word_lyrics: None,
                bilingual_line_lyrics: None,
                bilingual_word_lyrics: None,
            });
        }
        best
    }

    /// GET 并解析 JSON。LRCLIB 突发的 429 / 503 很常见，短暂等待后重试一次，避免整首歌词白白丢失；
    /// 重试等待也算在同一个独立超时里。其余失败（非 2xx、网络、超时、不是合法 JSON）都返回 `None`。
    async fn read_json(&self, url: &str, cancel: &CancelToken) -> Result<Option<Value>, Cancelled> {
        let attempts = async {
            let mut attempt = 0;
            loop {
                let request = HttpRequest {
                    method: HttpMethod::Get,
                    url: url.to_owned(),
                    headers: vec![(
                        "User-Agent".to_owned(),
                        "LunaticPrincess (https://github.com/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer)"
                            .to_owned(),
                    )],
                    form: None,
                    body: None,
                };
                let response = match self.http.send_raw(request).await {
                    Ok(response) => response,
                    Err(_) => return None,
                };
                if response.is_success() {
                    let text = response.text();
                    return if is_blank(&text) {
                        None
                    } else {
                        serde_json::from_str(&text).ok()
                    };
                }
                if attempt > 0 || !matches!(response.status, 429 | 503) {
                    return None;
                }
                attempt += 1;
                tokio::time::sleep(self.retry_delay).await;
            }
        };

        match cancellable(cancel, tokio::time::timeout(self.http.timeout(), attempts)).await {
            Err(Cancelled) => Err(Cancelled),
            Ok(Err(_elapsed)) => Ok(None),
            Ok(Ok(value)) => Ok(value),
        }
    }
}

fn get_string(element: &Value, name: &str) -> Option<String> {
    element.get(name)?.as_str().map(str::to_owned)
}

/// 数字或数字字符串；其余记 0。
fn get_double(element: &Value, name: &str) -> f64 {
    match element.get(name) {
        Some(Value::Number(number)) => number.as_f64().unwrap_or(0.0),
        Some(Value::String(text)) => text.trim().parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    }
}

impl LyricsSource for LrclibLyricsSource {
    fn label(&self) -> &str {
        Self::LABEL
    }

    fn fetch<'a>(
        &'a self,
        query: &'a LyricsQuery<'a>,
        cancel: &'a CancelToken,
    ) -> SourceFuture<'a, Result<Option<LyricsSourceCandidate>, OnlineError>> {
        Box::pin(async move {
            if is_blank(query.title) {
                return Ok(None);
            }
            let threshold = query.min_score.clamp(MIN_THRESHOLD, MAX_THRESHOLD);

            // 先按「标题 + 歌手」全文检索，全部落空后再退回精确的 track_name + artist_name 查询。
            let uris = Self::query_uris(query.queries);
            if let Some(best) = self.search(&uris, threshold, query, cancel).await? {
                return Ok(Some(best));
            }

            let track = [Self::track_uri(query.title, query.artist)];
            Ok(self.search(&track, threshold, query, cancel).await?)
        })
    }
}
