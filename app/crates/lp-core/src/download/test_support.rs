//! 下载与同步测试共用的假实现（只在测试里编译）：假接口、假 CDN、假解密器、记录写入的假标签服务、假在线补全。
//!
//! 对应 C# 的 `Download/TestDoubles.cs` 与 `Sync/TestDoubles.cs`。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::fetch::MediaFetcher;
use super::ncm_import::{EnrichError, OnlineEnricher, OnlineMatchOutcome};
use super::service::{
    DownloadApi, DownloadProgress, DownloadService, Downloader, ItemContext, ProgressSink,
};
use crate::lyrics::RawLyrics;
use crate::models::AudioQuality;
use crate::ncm::{NcmDecoder, NcmDecryptResult, NcmError, NcmFileInfo, NcmMetadata};
use crate::netease::http::{
    BodyStream, BoxFuture, HttpRequest, HttpResponse, HttpStreamResponse, HttpTransport,
    TransportError,
};
use crate::netease::{AlbumRef, ArtistRef, NeteaseError, NeteaseSong, SongPlaybackInfo};
use crate::tags::{AudioTagData, AudioTagInfo, AudioTagService, TagError};
use crate::utilities::CancelToken;

pub const NCM_MAGIC: &[u8] = b"CTENFDAM";

pub fn mp3_bytes() -> Vec<u8> {
    vec![0x49, 0x44, 0x33, 0x04, 0x00, 0x00, 0xFF, 0xFB, 0x01]
}

pub fn flac_bytes() -> Vec<u8> {
    vec![0x66, 0x4C, 0x61, 0x43, 0x00, 0x00, 0x01]
}

pub fn png_bytes(marker: u8) -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, marker,
    ]
}

pub fn ncm_bytes() -> Vec<u8> {
    let mut bytes = NCM_MAGIC.to_vec();
    bytes.extend([1, 2, 3, 4]);
    bytes
}

/// 默认歌曲：晴天 / 周杰伦 / 叶惠美，音轨 3，发行 2003。
pub fn song(id: i64) -> NeteaseSong {
    NeteaseSong {
        id,
        name: "晴天".to_owned(),
        artists: vec![ArtistRef {
            id: 1,
            name: "周杰伦".to_owned(),
        }],
        album: AlbumRef {
            id: 2,
            name: "叶惠美".to_owned(),
            cover_url: Some("https://img.example/cover.jpg".to_owned()),
        },
        duration_ms: 269_000,
        release_date: Some("2003-07-31".to_owned()),
        track_number: 3,
        ..NeteaseSong::default()
    }
}

/// 没有封面地址的歌曲。
pub fn song_no_cover(id: i64) -> NeteaseSong {
    let mut song = song(id);
    song.album.cover_url = None;
    song
}

pub fn playable(id: i64, format: &str) -> SongPlaybackInfo {
    SongPlaybackInfo {
        song_id: id,
        url: Some(format!("https://cdn.example/{id}.{format}")),
        format: Some(format.to_owned()),
        bitrate: 320_000,
        ..SongPlaybackInfo::default()
    }
}

// ---- 假接口 ----

type PlaybackHandler =
    Box<dyn Fn(i64, AudioQuality) -> Result<SongPlaybackInfo, NeteaseError> + Send + Sync>;
type LyricsHandler = Box<dyn Fn(i64) -> Result<RawLyrics, NeteaseError> + Send + Sync>;
type DetailHandler = Box<dyn Fn(i64) -> Result<Option<NeteaseSong>, NeteaseError> + Send + Sync>;

#[derive(Default)]
pub struct FakeApi {
    pub playback: Mutex<Option<PlaybackHandler>>,
    pub lyrics: Mutex<Option<LyricsHandler>>,
    pub detail: Mutex<Option<DetailHandler>>,
    pub playback_calls: AtomicUsize,
    pub lyrics_calls: AtomicUsize,
    pub detail_calls: AtomicUsize,
    pub requested_qualities: Mutex<Vec<AudioQuality>>,
}

impl FakeApi {
    /// 每首歌都返回 `https://cdn.example/<id>.<format>` 的播放地址。
    pub fn playing(format: &'static str) -> Arc<Self> {
        let api = Arc::new(Self::default());
        api.on_playback(move |id, _| Ok(playable(id, format)));
        api
    }

    pub fn on_playback(
        &self,
        handler: impl Fn(i64, AudioQuality) -> Result<SongPlaybackInfo, NeteaseError>
        + Send
        + Sync
        + 'static,
    ) {
        *self.playback.lock().unwrap() = Some(Box::new(handler));
    }

    pub fn on_lyrics(
        &self,
        handler: impl Fn(i64) -> Result<RawLyrics, NeteaseError> + Send + Sync + 'static,
    ) {
        *self.lyrics.lock().unwrap() = Some(Box::new(handler));
    }

    pub fn on_detail(
        &self,
        handler: impl Fn(i64) -> Result<Option<NeteaseSong>, NeteaseError> + Send + Sync + 'static,
    ) {
        *self.detail.lock().unwrap() = Some(Box::new(handler));
    }
}

impl DownloadApi for FakeApi {
    fn playback_info<'a>(
        &'a self,
        song_id: i64,
        quality: AudioQuality,
    ) -> BoxFuture<'a, Result<SongPlaybackInfo, NeteaseError>> {
        self.playback_calls.fetch_add(1, Ordering::SeqCst);
        self.requested_qualities.lock().unwrap().push(quality);
        let result = match self.playback.lock().unwrap().as_ref() {
            Some(handler) => handler(song_id, quality),
            None => panic!("本测试未配置播放地址"),
        };
        Box::pin(async move { result })
    }

    fn lyrics<'a>(&'a self, song_id: i64) -> BoxFuture<'a, Result<RawLyrics, NeteaseError>> {
        self.lyrics_calls.fetch_add(1, Ordering::SeqCst);
        let result = match self.lyrics.lock().unwrap().as_ref() {
            Some(handler) => handler(song_id),
            None => Ok(RawLyrics::default()),
        };
        Box::pin(async move { result })
    }

    fn song_detail<'a>(
        &'a self,
        song_id: i64,
    ) -> BoxFuture<'a, Result<Option<NeteaseSong>, NeteaseError>> {
        self.detail_calls.fetch_add(1, Ordering::SeqCst);
        let result = match self.detail.lock().unwrap().as_ref() {
            Some(handler) => handler(song_id),
            None => Ok(None),
        };
        Box::pin(async move { result })
    }
}

// ---- 假 CDN ----

type Responder = Box<
    dyn Fn(&HttpRequest) -> BoxFuture<'static, Result<HttpResponse, TransportError>> + Send + Sync,
>;

/// 按 URL 返回预设字节的假传输；未登记的 URL 返回 404。也可以换成自定义的异步应答。
pub struct StubCdn {
    responses: Mutex<HashMap<String, Vec<u8>>>,
    requests: Mutex<Vec<String>>,
    responder: Mutex<Option<Responder>>,
}

impl StubCdn {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(HashMap::new()),
            requests: Mutex::new(Vec::new()),
            responder: Mutex::new(None),
        })
    }

    pub fn serve(&self, url: &str, bytes: Vec<u8>) {
        self.responses.lock().unwrap().insert(url.to_owned(), bytes);
    }

    pub fn respond_with(
        &self,
        responder: impl Fn(&HttpRequest) -> BoxFuture<'static, Result<HttpResponse, TransportError>>
        + Send
        + Sync
        + 'static,
    ) {
        *self.responder.lock().unwrap() = Some(Box::new(responder));
    }

    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

pub fn ok_response(body: Vec<u8>) -> HttpResponse {
    HttpResponse {
        status: 200,
        set_cookies: Vec::new(),
        body,
    }
}

impl HttpTransport for StubCdn {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        self.requests.lock().unwrap().push(request.url.clone());
        if let Some(responder) = self.responder.lock().unwrap().as_ref() {
            return responder(&request);
        }
        let response = match self.responses.lock().unwrap().get(&request.url) {
            Some(bytes) => ok_response(bytes.clone()),
            None => HttpResponse {
                status: 404,
                set_cookies: Vec::new(),
                body: Vec::new(),
            },
        };
        Box::pin(async move { Ok(response) })
    }
}

/// 分块返回响应体的传输：每块之间 `await` 一次让出，可在中途卡住（`stall_after`）。
pub struct ChunkedTransport {
    pub chunks: Vec<Vec<u8>>,
    /// 声明的 Content-Length（`None` 表示未知）。
    pub declared_length: Option<u64>,
    /// 发完这么多块之后一直等待（模拟网络卡死）。
    pub stall_after: Option<usize>,
    pub started: Arc<tokio::sync::Notify>,
}

struct ChunkBody {
    chunks: std::vec::IntoIter<Vec<u8>>,
    sent: usize,
    stall_after: Option<usize>,
}

impl BodyStream for ChunkBody {
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, TransportError>> {
        Box::pin(async move {
            if self.stall_after == Some(self.sent) {
                std::future::pending::<()>().await;
            }
            tokio::task::yield_now().await;
            self.sent += 1;
            Ok(self.chunks.next())
        })
    }
}

impl HttpTransport for ChunkedTransport {
    fn send(&self, _request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        let body: Vec<u8> = self.chunks.concat();
        Box::pin(async move { Ok(ok_response(body)) })
    }

    fn send_stream(
        &self,
        _request: HttpRequest,
    ) -> BoxFuture<'_, Result<HttpStreamResponse, TransportError>> {
        self.started.notify_one();
        let body = ChunkBody {
            chunks: self.chunks.clone().into_iter(),
            sent: 0,
            stall_after: self.stall_after,
        };
        let content_length = self.declared_length;
        Box::pin(async move {
            Ok(HttpStreamResponse {
                status: 200,
                content_length,
                body: Box::new(body),
            })
        })
    }
}

// ---- 假标签服务 ----

pub struct WriteRecord {
    #[allow(dead_code)]
    pub path: PathBuf,
    pub tags: AudioTagData,
    pub file_existed: bool,
}

type FailRule = Box<dyn Fn(&Path) -> bool + Send + Sync>;

#[derive(Default)]
pub struct RecordingTags {
    pub writes: Mutex<Vec<WriteRecord>>,
    pub fail_rule: Mutex<Option<FailRule>>,
}

impl RecordingTags {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 所有写入都失败。
    pub fn fail_always(&self) {
        self.fail_when(|_| true);
    }

    /// 写入的目标路径满足条件时失败。
    pub fn fail_when(&self, rule: impl Fn(&Path) -> bool + Send + Sync + 'static) {
        *self.fail_rule.lock().unwrap() = Some(Box::new(rule));
    }

    pub fn tags(&self) -> Vec<AudioTagData> {
        self.writes
            .lock()
            .unwrap()
            .iter()
            .map(|record| record.tags.clone())
            .collect()
    }

    pub fn last(&self) -> AudioTagData {
        self.tags().pop().expect("至少写过一次标签")
    }

    pub fn count(&self) -> usize {
        self.writes.lock().unwrap().len()
    }
}

impl AudioTagService for RecordingTags {
    fn read(&self, _audio_path: &Path) -> Result<Option<AudioTagInfo>, TagError> {
        Ok(None)
    }

    fn write(&self, audio_path: &Path, tags: &AudioTagData) -> Result<(), TagError> {
        self.writes.lock().unwrap().push(WriteRecord {
            path: audio_path.to_path_buf(),
            tags: tags.clone(),
            file_existed: audio_path.is_file(),
        });
        let fail = self
            .fail_rule
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|rule| rule(audio_path));
        if fail {
            return Err(TagError::SaveFailed {
                file: audio_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                reason: "标签写入失败".to_owned(),
            });
        }
        Ok(())
    }
}

// ---- 假解密器 ----

type DecryptHandler = Box<
    dyn Fn(&Path, &Path, bool, &CancelToken) -> Result<NcmDecryptResult, NcmError> + Send + Sync,
>;

pub struct FakeDecoder {
    pub metadata: Mutex<NcmMetadata>,
    pub cover: Mutex<Option<Vec<u8>>>,
    pub output_format: &'static str,
    pub read_info_fails: AtomicBool,
    pub calls: Mutex<Vec<(PathBuf, bool)>>,
    pub handler: Mutex<Option<DecryptHandler>>,
}

impl FakeDecoder {
    pub fn new(output_format: &'static str) -> Arc<Self> {
        Arc::new(Self {
            metadata: Mutex::new(NcmMetadata {
                title: Some("NCM 内嵌歌名".to_owned()),
                artists: Some("NCM 内嵌歌手".to_owned()),
                album: Some("NCM 内嵌专辑".to_owned()),
                format: Some("mp3".to_owned()),
                bitrate: 128_000,
                duration_ms: 200_000,
                music_id: Some(555),
                alias: None,
            }),
            cover: Mutex::new(None),
            output_format,
            read_info_fails: AtomicBool::new(false),
            calls: Mutex::new(Vec::new()),
            handler: Mutex::new(None),
        })
    }

    pub fn with_cover(self: Arc<Self>, cover: Vec<u8>) -> Arc<Self> {
        *self.cover.lock().unwrap() = Some(cover);
        self
    }

    pub fn calls(&self) -> Vec<(PathBuf, bool)> {
        self.calls.lock().unwrap().clone()
    }
}

impl NcmDecoder for FakeDecoder {
    fn is_ncm_file(&self, path: &Path) -> bool {
        crate::download::media::has_ncm_magic(path)
    }

    fn read_info(&self, ncm_path: &Path) -> Result<NcmFileInfo, NcmError> {
        if self.read_info_fails.load(Ordering::SeqCst) {
            return Err(NcmError::InvalidData("损坏的头".to_owned()));
        }
        Ok(NcmFileInfo {
            source_path: ncm_path.to_path_buf(),
            metadata: Some(self.metadata.lock().unwrap().clone()),
        })
    }

    fn decrypt(
        &self,
        ncm_path: &Path,
        output_directory: &Path,
        overwrite: bool,
        cancel: &CancelToken,
    ) -> Result<NcmDecryptResult, NcmError> {
        self.calls
            .lock()
            .unwrap()
            .push((ncm_path.to_path_buf(), overwrite));
        if let Some(handler) = self.handler.lock().unwrap().as_ref() {
            return handler(ncm_path, output_directory, overwrite, cancel);
        }
        let stem = ncm_path.file_stem().unwrap().to_string_lossy().into_owned();
        let output = output_directory.join(format!("{stem}.{}", self.output_format));
        fs::write(&output, flac_bytes())?;
        Ok(NcmDecryptResult {
            source_path: ncm_path.to_path_buf(),
            output_path: Some(output),
            success: true,
            format: Some(self.output_format.to_owned()),
            metadata: Some(self.metadata.lock().unwrap().clone()),
            cover_data: self.cover.lock().unwrap().clone(),
            error_message: None,
            enrichment_message: None,
        })
    }
}

// ---- 假在线补全 ----

type EnrichHandler = Box<dyn Fn(&Path) -> Result<OnlineMatchOutcome, EnrichError> + Send + Sync>;

#[derive(Default)]
pub struct FakeEnricher {
    pub calls: Mutex<Vec<PathBuf>>,
    pub handler: Mutex<Option<EnrichHandler>>,
}

impl FakeEnricher {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn on_enrich(
        &self,
        handler: impl Fn(&Path) -> Result<OnlineMatchOutcome, EnrichError> + Send + Sync + 'static,
    ) {
        *self.handler.lock().unwrap() = Some(Box::new(handler));
    }

    pub fn calls(&self) -> Vec<PathBuf> {
        self.calls.lock().unwrap().clone()
    }
}

impl OnlineEnricher for FakeEnricher {
    fn enrich<'a>(
        &'a self,
        audio_path: &'a Path,
        _cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<OnlineMatchOutcome, EnrichError>> {
        self.calls.lock().unwrap().push(audio_path.to_path_buf());
        let result = match self.handler.lock().unwrap().as_ref() {
            Some(handler) => handler(audio_path),
            None => Ok(OnlineMatchOutcome {
                success: true,
                skipped: false,
                message: "已写入标签、封面、双语逐字歌词".to_owned(),
                matched_title: Some("Lemon".to_owned()),
                matched_artist: Some("米津玄師".to_owned()),
                source_label: Some("QQ 音乐".to_owned()),
            }),
        };
        Box::pin(async move { result })
    }
}

// ---- 进度记录 ----

#[derive(Clone, Default)]
pub struct Reports(pub Arc<Mutex<Vec<DownloadProgress>>>);

impl Reports {
    pub fn sink(&self) -> ProgressSink {
        let store = Arc::clone(&self.0);
        Arc::new(move |progress| store.lock().unwrap().push(progress))
    }

    pub fn all(&self) -> Vec<DownloadProgress> {
        self.0.lock().unwrap().clone()
    }

    pub fn context(&self, total: usize) -> ItemContext {
        ItemContext::new(0, total, Arc::new(AtomicUsize::new(0)), self.sink())
    }
}

// ---- 组装 ----

/// 下载服务的测试装配。
pub struct DownloadHarness {
    pub api: Arc<FakeApi>,
    pub tags: Arc<RecordingTags>,
    pub decoder: Arc<FakeDecoder>,
    pub cdn: Arc<StubCdn>,
    pub enricher: Option<Arc<FakeEnricher>>,
    pub service: Arc<DownloadService>,
}

impl DownloadHarness {
    pub fn new(api: Arc<FakeApi>) -> Self {
        Self::with(api, FakeDecoder::new("flac"), StubCdn::new(), None)
    }

    pub fn with(
        api: Arc<FakeApi>,
        decoder: Arc<FakeDecoder>,
        cdn: Arc<StubCdn>,
        enricher: Option<Arc<FakeEnricher>>,
    ) -> Self {
        let tags = RecordingTags::new();
        let hook = enricher
            .clone()
            .map(|e| -> Arc<dyn super::ncm_import::OnlineEnricher> { e });
        let service = Arc::new(DownloadService::new(
            api.clone(),
            tags.clone(),
            decoder.clone(),
            MediaFetcher::new(cdn.clone()),
            hook,
        ));
        Self {
            api,
            tags,
            decoder,
            cdn,
            enricher,
            service,
        }
    }

    pub fn as_downloader(&self) -> Arc<dyn Downloader> {
        self.service.clone()
    }
}

/// 目录里所有残留的临时文件（以 `.` 开头，或带 `.tmp` / `.part`）。
pub fn leftover_temp_files(directory: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![directory.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if name.starts_with('.') {
                    found.push(name);
                } else {
                    stack.push(path);
                }
            } else if name.starts_with('.') || name.contains(".tmp") || name.ends_with(".part") {
                found.push(name);
            }
        }
    }
    found
}

/// 目录里（顶层）所有文件名，排序后返回。
pub fn file_names(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
