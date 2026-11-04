//! 在线模块与批量服务的测试替身（只在测试里编译）。

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use base64::Engine;
use flate2::Compression;
use flate2::write::ZlibEncoder;

use super::model::{
    LyricsQuery, LyricsSource, LyricsSourceCandidate, OnlineError, OnlineMusicSource,
    OnlineSongDetails, OnlineSongResult, SourceFuture,
};
use super::service::OnlineMusicSearch;
use crate::lyrics::krc::XOR_KEY;
use crate::lyrics::qrc::{QRC_KEY, encrypt};
use crate::netease::http::{BoxFuture, HttpResponse};
use crate::tags::{AudioTagData, AudioTagInfo, AudioTagService, TagError};
use crate::utilities::CancelToken;

/// 200 + JSON 的响应。
pub fn json_ok(json: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        set_cookies: Vec::new(),
        body: json.as_bytes().to_vec(),
    }
}

/// 指定状态码 + 空体的响应。
pub fn status(status: u16) -> HttpResponse {
    HttpResponse {
        status,
        set_cookies: Vec::new(),
        body: Vec::new(),
    }
}

fn zlib(text: &str) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(text.as_bytes()).unwrap();
    encoder.finish().unwrap()
}

/// QQ 的加密 QRC：zlib → 补零到 8 的倍数 → 3DES 变体 → 小写十六进制。
pub fn qrc_hex(plain: &str) -> String {
    let mut padded = zlib(plain);
    padded.resize(padded.len().div_ceil(8) * 8, 0);
    let length = padded.len();
    encrypt(QRC_KEY, &padded, length)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 酷狗的 KRC：明文 `krc1` 魔数 + XOR 混淆的 zlib 流，base64。
pub fn krc_base64(text: &str) -> String {
    let mut bytes = b"krc1".to_vec();
    bytes.extend(
        zlib(text)
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ XOR_KEY[index % XOR_KEY.len()]),
    );
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// 标准 base64。
pub fn base64_of(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text.as_bytes())
}

/// 构造一条搜索结果。
pub fn song(
    source: OnlineMusicSource,
    id: &str,
    title: &str,
    artist: &str,
    album: &str,
    duration_ms: i64,
) -> OnlineSongResult {
    OnlineSongResult {
        source,
        source_id: id.to_owned(),
        title: title.to_owned(),
        artist: artist.to_owned(),
        album: album.to_owned(),
        duration_ms,
        year: Some(2003),
        track_number: Some(3),
        disc_number: None,
        genre: None,
        composer: None,
        cover_url: None,
        numeric_source_id: None,
    }
}

// ───────────────────────────── 假的在线搜索 ─────────────────────────────

type SearchHandler = Box<
    dyn Fn(OnlineMusicSource, &str) -> Result<Vec<OnlineSongResult>, OnlineError> + Send + Sync,
>;
type DetailsHandler =
    Box<dyn Fn(&OnlineSongResult) -> Result<Option<OnlineSongDetails>, OnlineError> + Send + Sync>;

/// 可编程的在线搜索：记录每次搜索 / 取详情的调用。
pub struct StubOnline {
    search: Mutex<SearchHandler>,
    details: Mutex<DetailsHandler>,
    searches: Mutex<Vec<(OnlineMusicSource, String)>>,
    detail_calls: Mutex<Vec<(OnlineMusicSource, String)>>,
    delay: Mutex<std::time::Duration>,
    in_flight: std::sync::atomic::AtomicUsize,
    peak: std::sync::atomic::AtomicUsize,
}

impl StubOnline {
    /// 每次搜索都返回 `songs`，详情带 `lyrics`（`None` 表示没有歌词）。
    pub fn with(songs: Vec<OnlineSongResult>, lyrics: Option<&str>) -> Arc<Self> {
        let lyrics = lyrics.map(str::to_owned);
        Arc::new(Self {
            search: Mutex::new(Box::new(move |_, _| Ok(songs.clone()))),
            details: Mutex::new(Box::new(move |song| {
                let mut details = OnlineSongDetails::without_extras(song.clone());
                details.line_lyrics = lyrics.clone();
                Ok(Some(details))
            })),
            searches: Mutex::new(Vec::new()),
            detail_calls: Mutex::new(Vec::new()),
            delay: Mutex::new(std::time::Duration::ZERO),
            in_flight: std::sync::atomic::AtomicUsize::new(0),
            peak: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// 替换搜索处理。
    pub fn on_search(
        &self,
        handler: impl Fn(OnlineMusicSource, &str) -> Result<Vec<OnlineSongResult>, OnlineError>
        + Send
        + Sync
        + 'static,
    ) {
        *self.search.lock().unwrap() = Box::new(handler);
    }

    /// 替换详情处理。
    pub fn on_details(
        &self,
        handler: impl Fn(&OnlineSongResult) -> Result<Option<OnlineSongDetails>, OnlineError>
        + Send
        + Sync
        + 'static,
    ) {
        *self.details.lock().unwrap() = Box::new(handler);
    }

    /// 让每次搜索先等一会儿（用来观察并发）。
    pub fn set_delay(&self, delay: std::time::Duration) {
        *self.delay.lock().unwrap() = delay;
    }

    /// 同时在途的搜索数的峰值。
    pub fn peak(&self) -> usize {
        self.peak.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 所有搜索调用（来源、关键词）。
    pub fn searches(&self) -> Vec<(OnlineMusicSource, String)> {
        self.searches.lock().unwrap().clone()
    }

    /// 所有详情调用（来源、ID）。
    pub fn detail_calls(&self) -> Vec<(OnlineMusicSource, String)> {
        self.detail_calls.lock().unwrap().clone()
    }
}

impl OnlineMusicSearch for StubOnline {
    fn search<'a>(
        &'a self,
        source: OnlineMusicSource,
        keyword: &'a str,
        _limit: usize,
        _cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Vec<OnlineSongResult>, OnlineError>> {
        self.searches
            .lock()
            .unwrap()
            .push((source, keyword.to_owned()));
        let result = (self.search.lock().unwrap())(source, keyword);
        let delay = *self.delay.lock().unwrap();
        Box::pin(async move {
            use std::sync::atomic::Ordering;
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            result
        })
    }

    fn get_details<'a>(
        &'a self,
        song: &'a OnlineSongResult,
        _cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Option<OnlineSongDetails>, OnlineError>> {
        self.detail_calls
            .lock()
            .unwrap()
            .push((song.source, song.source_id.clone()));
        let result = (self.details.lock().unwrap())(song);
        Box::pin(async move { result })
    }
}

// ───────────────────────────── 假的备用词源 ─────────────────────────────

/// 可编程的备用词源：记录收到的关键词。
pub struct StubFallback {
    label: String,
    result: Mutex<Result<Option<LyricsSourceCandidate>, OnlineError>>,
    queries: Mutex<Vec<String>>,
    calls: Mutex<Vec<(f64, i64)>>,
}

impl StubFallback {
    /// 返回固定候选。
    pub fn returning(label: &str, candidate: Option<LyricsSourceCandidate>) -> Arc<Self> {
        Arc::new(Self {
            label: label.to_owned(),
            result: Mutex::new(Ok(candidate)),
            queries: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
        })
    }

    /// 总是失败。
    pub fn failing(label: &str) -> Arc<Self> {
        Arc::new(Self {
            label: label.to_owned(),
            result: Mutex::new(Err(OnlineError::failed("模拟词源故障"))),
            queries: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
        })
    }

    /// 收到过的关键词。
    pub fn searched_queries(&self) -> Vec<String> {
        self.queries.lock().unwrap().clone()
    }

    /// 每次调用的（阈值，时长）。
    pub fn calls(&self) -> Vec<(f64, i64)> {
        self.calls.lock().unwrap().clone()
    }
}

impl LyricsSource for StubFallback {
    fn label(&self) -> &str {
        &self.label
    }

    fn fetch<'a>(
        &'a self,
        query: &'a LyricsQuery<'a>,
        _cancel: &'a CancelToken,
    ) -> SourceFuture<'a, Result<Option<LyricsSourceCandidate>, OnlineError>> {
        self.queries
            .lock()
            .unwrap()
            .extend(query.queries.iter().cloned());
        self.calls
            .lock()
            .unwrap()
            .push((query.min_score, query.duration_ms));
        let result = self.result.lock().unwrap().clone();
        Box::pin(async move { result })
    }
}

/// 一个只有逐行歌词的备用候选。
pub fn line_candidate(
    label: &str,
    title: &str,
    artist: &str,
    album: &str,
    duration_ms: i64,
    lyrics: &str,
) -> LyricsSourceCandidate {
    LyricsSourceCandidate {
        source_label: label.to_owned(),
        title: title.to_owned(),
        artist: artist.to_owned(),
        album: album.to_owned(),
        duration_ms,
        line_lyrics: Some(lyrics.to_owned()),
        word_lyrics: None,
        bilingual_line_lyrics: None,
        bilingual_word_lyrics: None,
    }
}

// ───────────────────────────── 假的标签服务 ─────────────────────────────

/// 返回固定标签信息并记录写入的标签服务。
pub struct StubTags {
    info: Mutex<Option<AudioTagInfo>>,
    written: Mutex<Vec<AudioTagData>>,
    fail_write: Mutex<bool>,
}

impl StubTags {
    /// 读取总是返回 `info`。
    pub fn with(info: AudioTagInfo) -> Arc<Self> {
        Arc::new(Self {
            info: Mutex::new(Some(info)),
            written: Mutex::new(Vec::new()),
            fail_write: Mutex::new(false),
        })
    }

    /// 读取返回 `None`（无法读取的文件）。
    pub fn unreadable() -> Arc<Self> {
        Arc::new(Self {
            info: Mutex::new(None),
            written: Mutex::new(Vec::new()),
            fail_write: Mutex::new(false),
        })
    }

    /// 让写入失败。
    pub fn fail_writes(&self) {
        *self.fail_write.lock().unwrap() = true;
    }

    /// 最后一次写入的标签。
    pub fn written(&self) -> Option<AudioTagData> {
        self.written.lock().unwrap().last().cloned()
    }

    /// 写入次数。
    pub fn write_count(&self) -> usize {
        self.written.lock().unwrap().len()
    }
}

impl AudioTagService for StubTags {
    fn read(&self, _audio_path: &Path) -> Result<Option<AudioTagInfo>, TagError> {
        Ok(self.info.lock().unwrap().clone())
    }

    fn write(&self, _audio_path: &Path, tags: &AudioTagData) -> Result<(), TagError> {
        if *self.fail_write.lock().unwrap() {
            return Err(TagError::SaveFailed {
                file: "x.mp3".to_owned(),
                reason: "模拟写入失败".to_owned(),
            });
        }
        self.written.lock().unwrap().push(tags.clone());
        Ok(())
    }
}

/// 标题 / 歌手 / 时长的标签信息。
pub fn tag_info(title: &str, artist: &str, duration_ms: i64) -> AudioTagInfo {
    AudioTagInfo {
        title: (!title.is_empty()).then(|| title.to_owned()),
        artist: (!artist.is_empty()).then(|| artist.to_owned()),
        duration_ms,
        bitrate: 320,
        ..AudioTagInfo::default()
    }
}
