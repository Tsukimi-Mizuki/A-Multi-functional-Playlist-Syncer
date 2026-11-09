//! QQ 音乐的搜索与歌词。对应 C# `OnlineMusicSearchService` 里 QQ 相关的部分。
//!
//! - 搜索用 `/soso/fcgi-bin/search_cp`（`new_json=1`）：旧的 `client_search_cp` 已下线（两个域名都恒返回 HTTP 500）；
//!   `c.y.qq.com` 整站 500 时依次回退到 `c6.y.qq.com`；
//! - 歌词优先走 `music.musichallSong.PlayLyricInfo`（`qrc=1&crypt=1&roma=1`，同时返回原文、逐字与翻译），
//!   失败回退旧的 `fcg_query_lyric_new`（至少保留原文）。

use serde_json::{Value, json};

use super::model::{OnlineError, OnlineMusicSource, OnlineSongDetails, OnlineSongResult};
use super::net::{BROWSER_UA, CoverFetch, HttpFailure, OnlineHttp};
use crate::library::text::is_blank;
use crate::lyrics::html::html_decode;
use crate::lyrics::krc::decode_base64_utf8;
use crate::lyrics::{RawLyrics, parser, qrc};
use crate::netease::client::escape_data_string;
use crate::netease::http::{HttpMethod, HttpRequest};
use crate::utilities::{CancelToken, Cancelled};

/// 旧版搜索接口路径（现用，名字沿用 C# 常量）。
const SEARCH_PATH: &str = "/soso/fcgi-bin/search_cp";
/// 旧版歌词接口路径（PlayLyricInfo 不可用时的回退）。
const LEGACY_LYRIC_PATH: &str = "/lyric/fcgi-bin/fcg_query_lyric_new.fcg";
/// `c.y.qq.com` 会整站返回 HTTP 500，依次回退到可用镜像域名。
pub(crate) const DEFAULT_HOSTS: [&str; 2] = ["https://c.y.qq.com", "https://c6.y.qq.com"];
/// 新版歌词接口。
pub(crate) const PLAY_LYRIC_URL: &str = "https://u.y.qq.com/cgi-bin/musicu.fcg";

const SEARCH_LIMIT_MIN: usize = 1;
const SEARCH_LIMIT_MAX: usize = 30;

/// 专辑封面的尺寸档（像素），从大到小：服务端没有这一档（非 2xx）才换小一档。
/// 实测 1000、2000 这类不在档位里的尺寸回 404；原图比请求的小时直接回原图，不放大（C# 版固定 300）。
pub(crate) const COVER_SIZES: [u32; 4] = [1200, 800, 500, 300];

/// 某一档尺寸的专辑封面地址。
pub(crate) fn album_cover_url(album_mid: &str, size: u32) -> String {
    format!("https://y.gtimg.cn/music/photo_new/T002R{size}x{size}M000{album_mid}.jpg")
}

/// 封面地址的候选：标准封面地址按 [`COVER_SIZES`] 依次换成小一档，其他地址只有它自己。
fn cover_size_fallbacks(url: &str) -> Vec<String> {
    let largest = format!("T002R{0}x{0}M000", COVER_SIZES[0]);
    if !url.contains(&largest) {
        return vec![url.to_owned()];
    }
    COVER_SIZES
        .iter()
        .map(|size| url.replacen(&largest, &format!("T002R{size}x{size}M000"), 1))
        .collect()
}

/// QQ 音乐客户端：持有 HTTP 与镜像域名列表（测试可以换成假域名）。
pub(crate) struct QqClient {
    http: OnlineHttp,
    hosts: Vec<String>,
}

/// PlayLyricInfo 解出的歌词。
struct PlayLyric {
    line: String,
    word: Option<String>,
    bilingual: String,
    bilingual_word: Option<String>,
}

impl QqClient {
    pub(crate) fn new(http: OnlineHttp) -> Self {
        Self {
            http,
            hosts: DEFAULT_HOSTS
                .iter()
                .map(|host| (*host).to_owned())
                .collect(),
        }
    }

    fn headers(referer: &str) -> Vec<(String, String)> {
        vec![
            ("User-Agent".to_owned(), BROWSER_UA.to_owned()),
            ("Referer".to_owned(), referer.to_owned()),
            ("Accept".to_owned(), "*/*".to_owned()),
        ]
    }

    /// 旧版 fcg 接口：按顺序尝试各镜像域名，全部失败才报错。
    ///
    /// 与 C# 一致：只有连接失败与非 2xx 才换下一个域名；超时与取消直接向上抛。
    async fn send_legacy(
        &self,
        path_and_query: &str,
        cancel: &CancelToken,
    ) -> Result<String, HttpFailure> {
        let mut last_error: Option<HttpFailure> = None;
        for host in &self.hosts {
            let url = format!("{host}{path_and_query}");
            match self
                .http
                .get_text(&url, Self::headers("https://y.qq.com/"), cancel)
                .await
            {
                Ok(body) => return Ok(body),
                Err(error @ (HttpFailure::Cancelled | HttpFailure::Timeout(_))) => {
                    return Err(error);
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| HttpFailure::Network("QQ 音乐接口不可用。".to_owned())))
    }

    // ───────────────────────────── 搜索 ─────────────────────────────

    pub(crate) async fn search(
        &self,
        keyword: &str,
        limit: usize,
        cancel: &CancelToken,
    ) -> Result<Vec<OnlineSongResult>, OnlineError> {
        let limit = limit.clamp(SEARCH_LIMIT_MIN, SEARCH_LIMIT_MAX);
        let path = format!(
            "{SEARCH_PATH}?w={}&format=json&p=1&n={limit}&cr=1&new_json=1&g_tk=5381",
            escape_data_string(keyword)
        );
        let body = match self.send_legacy(&path, cancel).await {
            Ok(body) => body,
            Err(HttpFailure::Cancelled) => return Err(OnlineError::Cancelled),
            Err(HttpFailure::Timeout(limit)) => {
                return Err(OnlineError::failed(format!(
                    "QQ 音乐搜索超时（超过 {} 秒），请稍后重试或改用网易云音乐",
                    limit.as_secs().max(1)
                )));
            }
            // 两个镜像域名都失败时给出可定位的原因，而不是笼统的“网络连接失败”。
            Err(error) => {
                return Err(OnlineError::failed(format!(
                    "QQ 音乐搜索接口不可用（{}），请稍后重试或改用网易云音乐",
                    error.describe()
                )));
            }
        };

        let root: Value = serde_json::from_str(&body)
            .map_err(|_| OnlineError::failed("QQ 音乐搜索失败：服务端返回的不是合法 JSON"))?;
        if let Some(code) = root.get("code").and_then(Value::as_i64)
            && i32::try_from(code).is_ok_and(|code| code != 0)
        {
            return Err(OnlineError::failed(format!("QQ 音乐搜索失败：code={code}")));
        }

        let Some(list) = root
            .get("data")
            .and_then(|data| data.get("song"))
            .and_then(|song| song.get("list"))
            .and_then(Value::as_array)
        else {
            return Ok(Vec::new());
        };

        Ok(list.iter().filter_map(parse_search_item).collect())
    }

    // ───────────────────────────── 详情 ─────────────────────────────

    /// 下载封面：按 [`COVER_SIZES`] 从大到小尝试，服务端没有这一档才换小一档；
    /// 超时、网络错误、过大等其他失败不再换尺寸重试。只有取消时返回 `Err`。
    async fn download_cover(
        &self,
        url: Option<&str>,
        cancel: &CancelToken,
    ) -> Result<Option<Vec<u8>>, Cancelled> {
        let Some(url) = url.map(str::trim).filter(|url| !url.is_empty()) else {
            return Ok(None);
        };
        for candidate in cover_size_fallbacks(url) {
            match self.http.download_cover(&candidate, cancel).await? {
                CoverFetch::Done(bytes) => return Ok(Some(bytes)),
                CoverFetch::Rejected => continue,
                CoverFetch::Failed => return Ok(None),
            }
        }
        Ok(None)
    }

    pub(crate) async fn details(
        &self,
        song: &OnlineSongResult,
        cancel: &CancelToken,
    ) -> Result<OnlineSongDetails, OnlineError> {
        let cover = self
            .download_cover(song.cover_url.as_deref(), cancel)
            .await?;
        let mut details = OnlineSongDetails::without_extras(song.clone());
        details.cover_data = cover;

        // 优先走 musichall 的 PlayLyricInfo：它同时返回原文和翻译（QQ 的旧 fcg 接口通常没有翻译）。
        match self.play_lyric_info(song, cancel).await {
            Ok(Some(info)) => {
                details.line_lyrics = Some(info.line);
                details.word_lyrics = info.word;
                details.bilingual_line_lyrics = Some(info.bilingual);
                details.bilingual_word_lyrics = info.bilingual_word;
                return Ok(details);
            }
            Ok(None) => {}
            Err(HttpFailure::Cancelled) => return Err(OnlineError::Cancelled),
            Err(timeout @ HttpFailure::Timeout(_)) => {
                return Err(OnlineError::failed(format!(
                    "获取 QQ 音乐歌词失败：{}",
                    timeout.describe()
                )));
            }
            // 连接失败：回退到旧歌词接口，至少保留原文。
            Err(_) => {}
        }

        let path = format!(
            "{LEGACY_LYRIC_PATH}?songmid={}&format=json&nobase64=1&g_tk=5381&trans=1&trans_t=0",
            escape_data_string(&song.source_id)
        );
        let body = match self.send_legacy(&path, cancel).await {
            Ok(body) => body,
            Err(HttpFailure::Cancelled) => return Err(OnlineError::Cancelled),
            Err(timeout @ HttpFailure::Timeout(_)) => {
                return Err(OnlineError::failed(format!(
                    "获取 QQ 音乐歌词失败：{}",
                    timeout.describe()
                )));
            }
            Err(_) => return Ok(details),
        };
        let Ok(root) = serde_json::from_str::<Value>(&body) else {
            return Ok(details);
        };
        if root
            .get("retcode")
            .and_then(Value::as_i64)
            .is_some_and(|code| i32::try_from(code).is_ok_and(|code| code != 0))
        {
            return Ok(details);
        }

        let lyrics = get_string(&root, "lyric");
        let translation = get_string(&root, "trans");
        let Some(lyrics) = lyrics.filter(|text| !is_blank(text)) else {
            return Ok(details);
        };

        let decoded_lyrics = html_decode(&lyrics);
        let decoded_translation = translation
            .filter(|text| !is_blank(text))
            .map(|text| html_decode(&text));
        match decoded_translation.filter(|text| !is_blank(text)) {
            None => details.line_lyrics = Some(decoded_lyrics),
            Some(translation) => {
                let parsed = parser::parse(&RawLyrics {
                    lrc: Some(decoded_lyrics),
                    translated_lrc: Some(translation),
                    ..RawLyrics::default()
                });
                details.line_lyrics = Some(parser::to_plain_lrc(&parsed, false));
                details.bilingual_line_lyrics = Some(parser::to_plain_lrc(&parsed, true));
            }
        }
        Ok(details)
    }

    async fn play_lyric_info(
        &self,
        song: &OnlineSongResult,
        cancel: &CancelToken,
    ) -> Result<Option<PlayLyric>, HttpFailure> {
        let Some(song_id) = song
            .numeric_source_id
            .as_deref()
            .and_then(|id| id.trim().parse::<i64>().ok())
            .filter(|id| *id > 0)
        else {
            return Ok(None);
        };

        let interval = (song.duration_ms / 1000).max(0);
        let payload = json!({
            "comm": {
                "ct": 19, "cv": 2111, "v": 2111, "tmeAppID": "qqmusiclight",
                "format": "json", "nettype": "NETWORK_WIFI", "udid": "0"
            },
            "request": {
                "method": "GetPlayLyricInfo",
                "module": "music.musichallSong.PlayLyricInfo",
                "param": {
                    "albumName": to_base64(&song.album),
                    // crypt=1 + qrc=1：返回加密的逐字歌词（QRC），解密后可得到逐字时间轴。
                    "crypt": 1, "ct": 19, "cv": 2111, "interval": interval,
                    "lrc_t": 0, "qrc": 1, "qrc_t": 0, "roma": 1, "roma_t": 0,
                    "singerName": to_base64(&song.artist),
                    "songID": song_id,
                    "songName": to_base64(&song.title),
                    "trans": 1, "trans_t": 0, "type": 0
                }
            }
        });
        let request = HttpRequest {
            method: HttpMethod::Post,
            url: PLAY_LYRIC_URL.to_owned(),
            headers: vec![
                ("User-Agent".to_owned(), "okhttp/3.14.9".to_owned()),
                ("Referer".to_owned(), "https://y.qq.com/".to_owned()),
                (
                    "Content-Type".to_owned(),
                    "application/json; charset=utf-8".to_owned(),
                ),
            ],
            form: None,
            body: Some(payload.to_string().into_bytes()),
        };
        let response = self.http.send(request, cancel).await?;
        if !response.is_success() {
            return Ok(None);
        }

        let Ok(root) = serde_json::from_slice::<Value>(&response.body) else {
            return Ok(None);
        };
        if root
            .get("code")
            .and_then(Value::as_i64)
            .is_some_and(|code| i32::try_from(code).is_ok_and(|code| code != 0))
        {
            return Ok(None);
        }
        let Some(data) = root.get("request").and_then(|request| request.get("data")) else {
            return Ok(None);
        };

        // crypt=1 时 lyric 字段是十六进制密文（3DES + zlib 压缩的 QRC）；
        // 没有逐字资源时仍是 base64 的普通 LRC 文本，两种都兼容。
        let lyric_field = get_string(data, "lyric");
        let word_text = qrc::try_decrypt(lyric_field.as_deref());
        let line_text = if word_text.is_none() {
            decode_base64(lyric_field.as_deref())
        } else {
            None
        };
        let blank = |text: &Option<String>| text.as_deref().is_none_or(is_blank);
        if blank(&word_text) && blank(&line_text) {
            return Ok(None);
        }

        let translation = decode_lyric_field(get_string(data, "trans").as_deref());
        let romanization = decode_lyric_field(get_string(data, "roma").as_deref());
        let parsed = parser::parse(&RawLyrics {
            lrc: line_text,
            translated_lrc: translation,
            romanized_lrc: romanization,
            yrc: None,
            qrc: word_text,
        });
        let original = parser::to_plain_lrc(&parsed, false);
        let bilingual = parser::to_plain_lrc(&parsed, true);
        if !parsed.has_word_level() {
            return Ok(Some(PlayLyric {
                line: original,
                word: None,
                bilingual,
                bilingual_word: None,
            }));
        }

        Ok(Some(PlayLyric {
            line: original,
            word: Some(parser::to_enhanced_lrc(&parsed, false)),
            bilingual,
            bilingual_word: Some(parser::to_enhanced_lrc(&parsed, true)),
        }))
    }
}

/// 解码 QQ 音乐的歌词字段：加密的 QRC 先解密再转成逐行 LRC，否则按 base64 文本解码。
fn decode_lyric_field(value: Option<&str>) -> Option<String> {
    let value = value.filter(|text| !is_blank(text))?;
    if let Some(decrypted) = qrc::try_decrypt(Some(value)) {
        let converted = parser::to_plain_lrc(
            &parser::parse(&RawLyrics {
                qrc: Some(decrypted),
                ..RawLyrics::default()
            }),
            false,
        );
        return (!is_blank(&converted)).then_some(converted);
    }
    decode_base64(Some(value))
}

fn to_base64(value: &str) -> String {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    STANDARD.encode(value.as_bytes())
}

fn decode_base64(value: Option<&str>) -> Option<String> {
    let value = value.filter(|text| !is_blank(text))?;
    decode_base64_utf8(value)
}

// ───────────────────────────── 搜索结果解析 ─────────────────────────────

/// 字符串字段（只认 JSON 字符串）。
fn get_string(element: &Value, name: &str) -> Option<String> {
    element.get(name)?.as_str().map(str::to_owned)
}

/// 整数字段（只认 JSON 整数，其余记 0）。
fn get_long(element: &Value, name: &str) -> i64 {
    element.get(name).and_then(Value::as_i64).unwrap_or(0)
}

/// 无符号 32 位整数字段。
fn get_uint(element: &Value, name: &str) -> Option<u32> {
    element
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn first_non_blank(values: &[Option<String>]) -> Option<String> {
    values
        .iter()
        .find(|value| value.as_deref().is_some_and(|text| !is_blank(text)))
        .cloned()
        .flatten()
}

fn read_singers(item: &Value) -> String {
    let Some(singers) = item.get("singer").and_then(Value::as_array) else {
        return String::new();
    };
    singers
        .iter()
        .filter_map(|singer| get_string(singer, "name"))
        .filter(|name| !is_blank(name))
        .collect::<Vec<_>>()
        .join("/")
}

/// 一条搜索结果。search_cp（`new_json=1`）返回 songname / songmid / albumname / albummid / songid / pubtime，
/// 旧 client_search_cp 返回 title / mid / album{…} / id，两种字段名都兼容。
fn parse_search_item(item: &Value) -> Option<OnlineSongResult> {
    let title = first_non_blank(&[
        get_string(item, "songname"),
        get_string(item, "title"),
        get_string(item, "name"),
    ]);
    let mid = first_non_blank(&[get_string(item, "songmid"), get_string(item, "mid")]);
    let (title, mid) = (
        title.filter(|t| !is_blank(t))?,
        mid.filter(|m| !is_blank(m))?,
    );

    let artist = read_singers(item);
    let mut album_name = get_string(item, "albumname").unwrap_or_default();
    let mut album_mid = get_string(item, "albummid").unwrap_or_default();
    let mut release = String::new();
    if let Some(album) = item.get("album").filter(|album| album.is_object()) {
        album_name =
            first_non_blank(&[Some(album_name), get_string(album, "name")]).unwrap_or_default();
        album_mid =
            first_non_blank(&[Some(album_mid), get_string(album, "mid")]).unwrap_or_default();
        release = get_string(album, "time_public").unwrap_or_default();
    }

    let year = parse_year(Some(&release))
        .or_else(|| parse_year(get_string(item, "time_public").as_deref()))
        .or_else(|| parse_year_from_unix(get_long(item, "pubtime")));
    let track = get_uint(item, "index_album");
    // index_cd 从 0 开始；只要字段存在，碟号至少是 1。
    let disc = get_uint(item, "index_cd").map(|disc| if disc > 0 { disc + 1 } else { 1 });
    let duration = get_long(item, "interval");
    let cover_url = (!is_blank(&album_mid)).then(|| album_cover_url(&album_mid, COVER_SIZES[0]));

    let song_id = get_long(item, "songid");
    let numeric_id = if song_id > 0 {
        song_id
    } else {
        get_long(item, "id")
    };

    Some(OnlineSongResult {
        source: OnlineMusicSource::QqMusic,
        source_id: mid,
        title,
        artist,
        album: album_name,
        duration_ms: if duration > 0 { duration * 1000 } else { 0 },
        year,
        track_number: track,
        disc_number: disc,
        genre: None,
        composer: None,
        cover_url,
        numeric_source_id: (numeric_id > 0).then(|| numeric_id.to_string()),
    })
}

/// 取日期文本前 4 个字符当年份（与 C# `ParseYear` 一致：1–2999 之内才算）。
pub(crate) fn parse_year(value: Option<&str>) -> Option<u32> {
    let text = value?.trim_matches(crate::library::text::is_dotnet_whitespace);
    if text.encode_utf16().count() < 4 {
        return None;
    }
    let head: String = text.chars().take(4).collect();
    // `uint.TryParse(NumberStyles.Integer)`：允许首尾空白与前导 '+'。
    let digits = head
        .trim_matches(|ch| matches!(ch, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r'))
        .trim_start_matches('+');
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits
        .parse::<u32>()
        .ok()
        .filter(|year| (1..3000).contains(year))
}

/// Unix 秒折算成 UTC 年份；超出 `DateTimeOffset` 能表示的范围时返回 `None`。
pub(crate) fn parse_year_from_unix(seconds: i64) -> Option<u32> {
    // DateTimeOffset.FromUnixTimeSeconds 的上限是 9999-12-31T23:59:59。
    if !(1..=253_402_300_799).contains(&seconds) {
        return None;
    }
    let days = seconds.div_euclid(86_400);
    // Howard Hinnant 的 civil_from_days，只取年份。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    u32::try_from(year)
        .ok()
        .filter(|year| (1..3000).contains(year))
}
