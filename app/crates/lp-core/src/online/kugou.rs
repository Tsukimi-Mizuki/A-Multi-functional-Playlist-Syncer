//! 酷狗音乐词源：搜索候选（KRC 为主、LRC 兜底）→ 逐字歌词转成项目统一的增强 LRC。对应 C# `KugouLyricsSource`。
//!
//! KRC 自带逐行译文，能补上网易云 / QQ 都没有翻译的歌曲；接口无鉴权但会按关键词返回多条候选，
//! 必须用匹配度打分筛选，避免把同名歌曲写错。三段链路：
//! `mobilecdn.kugou.com/api/v3/search/song`（取 hash）→ `krcs.kugou.com/search`（候选 + accesskey）→
//! `lyrics.kugou.com/download?fmt=krc`（KRC，base64）。任何网络 / 证书 / 解析失败都回落成“没有结果”。

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use super::model::{LyricsQuery, LyricsSource, LyricsSourceCandidate, OnlineError, SourceFuture};
use super::net::OnlineHttp;
use crate::library::text::is_blank;
use crate::lyrics::krc::{self, decode_base64_utf8};
use crate::lyrics::scorer::{self, DEFAULT_DURATION_TOLERANCE_MS, MAX_THRESHOLD, MIN_THRESHOLD};
use crate::lyrics::{LyricDocument, RawLyrics, parser};
use crate::netease::client::escape_data_string;
use crate::netease::http::{HttpMethod, HttpRequest, HttpTransport};
use crate::utilities::{CancelToken, Cancelled};

/// 最多尝试几组关键词。
const MAX_QUERIES: usize = 2;
/// 每次候选搜索最多检查几条。
const MAX_CANDIDATES: usize = 8;

const HASH_SEARCH_URL: &str = "https://mobilecdn.kugou.com/api/v3/search/song";
const LYRICS_SEARCH_URL: &str = "https://krcs.kugou.com/search";
const LYRICS_DOWNLOAD_URL: &str = "https://lyrics.kugou.com/download";

/// 单次请求的独立超时（C# 默认 10 秒）。
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// 酷狗音乐词源。
pub struct KugouLyricsSource {
    http: OnlineHttp,
}

#[derive(Debug, Clone)]
struct KugouCandidate {
    id: String,
    access_key: String,
    song: String,
    singer: String,
    duration_ms: i64,
}

impl KugouLyricsSource {
    /// 备用词源名称：设置开关与界面提示都按这个名字对应。
    pub const LABEL: &'static str = "酷狗音乐";

    /// 用默认超时创建。
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self::with_timeout(transport, REQUEST_TIMEOUT)
    }

    /// 指定单次请求的独立超时。
    pub fn with_timeout(transport: Arc<dyn HttpTransport>, timeout: Duration) -> Self {
        Self {
            http: OnlineHttp::new(transport, timeout),
        }
    }

    /// GET 并解析 JSON：非 2xx、网络失败、超时、不是合法 JSON 都返回 `None`；只有取消返回 `Err`。
    async fn read_json(&self, url: &str, cancel: &CancelToken) -> Result<Option<Value>, Cancelled> {
        let request = HttpRequest {
            method: HttpMethod::Get,
            url: url.to_owned(),
            headers: Vec::new(),
            form: None,
            body: None,
        };
        let response = match self.http.send(request, cancel).await {
            Ok(response) => response,
            Err(super::net::HttpFailure::Cancelled) => return Err(Cancelled),
            Err(_) => return Ok(None),
        };
        if !response.is_success() {
            return Ok(None);
        }
        let text = response.text();
        if is_blank(&text) {
            return Ok(None);
        }
        Ok(serde_json::from_str(&text).ok())
    }

    /// 读下载接口的 `content`（base64 文本）：`status` 必须是数字 200。
    async fn read_content_base64(
        &self,
        url: &str,
        cancel: &CancelToken,
    ) -> Result<Option<String>, Cancelled> {
        let Some(json) = self.read_json(url, cancel).await? else {
            return Ok(None);
        };
        if !json.is_object() {
            return Ok(None);
        }
        if json.get("status").and_then(Value::as_i64) != Some(200) {
            return Ok(None);
        }
        Ok(json
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_owned))
    }

    /// 先查歌曲 hash（krcs 接口带 hash 命中率更高），查不到也继续尝试。
    async fn find_hash(
        &self,
        keyword: &str,
        cancel: &CancelToken,
    ) -> Result<Option<String>, Cancelled> {
        let url = format!(
            "{HASH_SEARCH_URL}?format=json&page=1&pagesize=3&showtype=1&keyword={}",
            escape_data_string(keyword)
        );
        let Some(json) = self.read_json(&url, cancel).await? else {
            return Ok(None);
        };
        let Some(info) = json
            .get("data")
            .and_then(|data| data.get("info"))
            .and_then(Value::as_array)
        else {
            return Ok(None);
        };
        Ok(info
            .iter()
            .filter_map(|item| item.get("hash").and_then(Value::as_str))
            .find(|hash| !is_blank(hash))
            .map(str::to_owned))
    }

    #[allow(clippy::too_many_arguments)]
    async fn find_candidate(
        &self,
        keyword: &str,
        title: &str,
        artist: &str,
        album: Option<&str>,
        duration_ms: i64,
        hash: Option<&str>,
        threshold: f64,
        cancel: &CancelToken,
    ) -> Result<Option<KugouCandidate>, Cancelled> {
        let seconds = ((duration_ms as f64 / 1000.0).round_ties_even() as i64).max(1);
        let mut url = format!(
            "{LYRICS_SEARCH_URL}?ver=1&man=yes&client=mobi&keyword={}&duration={seconds}",
            escape_data_string(keyword)
        );
        if let Some(hash) = hash.filter(|hash| !is_blank(hash)) {
            url.push_str("&hash=");
            url.push_str(&escape_data_string(hash));
        }
        let Some(json) = self.read_json(&url, cancel).await? else {
            return Ok(None);
        };
        let Some(candidates) = json.get("candidates").and_then(Value::as_array) else {
            return Ok(None);
        };

        let mut best: Option<KugouCandidate> = None;
        let mut best_score = 0.0_f64;
        for item in candidates.iter().take(MAX_CANDIDATES) {
            let id = get_string(item, "id");
            let access_key = get_string(item, "accesskey");
            let song = get_string(item, "song");
            let singer = get_string(item, "singer");
            let (Some(id), Some(access_key), Some(song)) = (
                id.filter(|v| !is_blank(v)),
                access_key.filter(|v| !is_blank(v)),
                song.filter(|v| !is_blank(v)),
            ) else {
                continue;
            };

            let candidate_duration = duration_to_millis(get_int64(item, "duration"));
            if !scorer::duration_matches(
                duration_ms,
                candidate_duration,
                DEFAULT_DURATION_TOLERANCE_MS,
            ) {
                continue;
            }

            let score = scorer::score(
                Some(title),
                Some(artist),
                album,
                duration_ms,
                Some(&song),
                singer.as_deref(),
                None,
                candidate_duration,
            );
            if score.rejected || score.total < threshold || score.total <= best_score {
                continue;
            }

            best_score = score.total;
            best = Some(KugouCandidate {
                id,
                access_key,
                song,
                singer: singer.unwrap_or_default(),
                duration_ms: candidate_duration,
            });
        }
        Ok(best)
    }

    /// 用 KRC 下载接口拿歌词文档；KRC 解不出来时退回 LRC 文本。
    async fn download_document(
        &self,
        candidate: &KugouCandidate,
        cancel: &CancelToken,
    ) -> Result<Option<LyricDocument>, Cancelled> {
        let krc_url = download_url("krc", candidate);
        let krc_text =
            krc::decrypt_base64(self.read_content_base64(&krc_url, cancel).await?.as_deref());
        if let Some(text) = krc_text.filter(|text| !is_blank(text)) {
            let document = krc::parse(Some(&text));
            if !document.is_empty() {
                return Ok(Some(document));
            }
        }

        let lrc_url = download_url("lrc", candidate);
        let lrc_text = self
            .read_content_base64(&lrc_url, cancel)
            .await?
            .and_then(|content| decode_base64_utf8(content.trim()));
        let Some(lrc_text) = lrc_text.filter(|text| !is_blank(text) && text.contains('[')) else {
            return Ok(None);
        };

        let document = parser::parse(&RawLyrics::from_lrc(lrc_text));
        Ok((!document.is_empty()).then_some(document))
    }

    fn build(candidate: &KugouCandidate, document: &LyricDocument) -> LyricsSourceCandidate {
        let has_translation = document
            .lines
            .iter()
            .any(|line| line.translation.as_deref().is_some_and(|t| !is_blank(t)));
        let plain = parser::to_plain_lrc(document, false);
        let bilingual_plain = has_translation.then(|| parser::to_plain_lrc(document, true));
        let (mut word, mut bilingual_word) = (None, None);
        if document.has_word_level() {
            word = Some(parser::to_enhanced_lrc(document, false));
            bilingual_word = has_translation.then(|| parser::to_enhanced_lrc(document, true));
        }

        LyricsSourceCandidate {
            source_label: Self::LABEL.to_owned(),
            title: candidate.song.clone(),
            artist: candidate.singer.clone(),
            album: String::new(),
            duration_ms: candidate.duration_ms,
            line_lyrics: Some(plain),
            word_lyrics: word,
            bilingual_line_lyrics: bilingual_plain,
            bilingual_word_lyrics: bilingual_word,
        }
    }
}

/// 候选的 `duration`：C# 版按“秒”处理，但实网（2026-11 探针）返回的是毫秒（如 255582），按秒乘 1000 会让所有候选都被时长过滤掉。
/// 一首歌不可能超过 1 万秒（2.7 小时），所以 ≥ 10000 的值按毫秒、更小的按秒（旧接口 / 夹具）。
fn duration_to_millis(raw: i64) -> i64 {
    if raw >= 10_000 {
        raw
    } else {
        raw.saturating_mul(1000)
    }
}

fn download_url(format: &str, candidate: &KugouCandidate) -> String {
    format!(
        "{LYRICS_DOWNLOAD_URL}?ver=1&client=pc&fmt={format}&charset=utf8&id={}&accesskey={}",
        escape_data_string(&candidate.id),
        escape_data_string(&candidate.access_key)
    )
}

fn get_string(element: &Value, name: &str) -> Option<String> {
    element.get(name)?.as_str().map(str::to_owned)
}

/// 数字或数字字符串；其余记 0。
fn get_int64(element: &Value, name: &str) -> i64 {
    match element.get(name) {
        Some(Value::Number(number)) => number.as_i64().unwrap_or(0),
        Some(Value::String(text)) => text.trim().parse::<i64>().unwrap_or(0),
        _ => 0,
    }
}

impl LyricsSource for KugouLyricsSource {
    fn label(&self) -> &str {
        Self::LABEL
    }

    fn fetch<'a>(
        &'a self,
        query: &'a LyricsQuery<'a>,
        cancel: &'a CancelToken,
    ) -> SourceFuture<'a, Result<Option<LyricsSourceCandidate>, OnlineError>> {
        Box::pin(async move {
            let threshold = query.min_score.clamp(MIN_THRESHOLD, MAX_THRESHOLD);
            let mut attempts = 0;

            for keyword in query.queries {
                if is_blank(keyword) {
                    continue;
                }
                if attempts >= MAX_QUERIES {
                    break;
                }
                attempts += 1;

                let hash = self.find_hash(keyword, cancel).await?;
                let Some(candidate) = self
                    .find_candidate(
                        keyword,
                        query.title,
                        query.artist,
                        query.album,
                        query.duration_ms,
                        hash.as_deref(),
                        threshold,
                        cancel,
                    )
                    .await?
                else {
                    continue;
                };

                let Some(document) = self.download_document(&candidate, cancel).await? else {
                    continue;
                };
                if document.is_empty() {
                    continue;
                }
                return Ok(Some(Self::build(&candidate, &document)));
            }
            Ok(None)
        })
    }
}
