use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::download::fetch::MediaFetcher;
use crate::download::ncm_import::{NcmImportService, OnlineEnricher};
use crate::download::service::DownloadItemResult;
use crate::download::test_support::*;
use crate::lyrics::RawLyrics;
use crate::models::LyricsWriteMode;
use crate::ncm::NcmDecryptResult;
use crate::netease::http::BoxFuture;
use crate::netease::{NeteaseError, NeteaseSong};
use crate::tags::{AudioTagService, LoftyTagService};
use crate::test_util::TempDir;

const NCM_COVER: [u8; 9] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x01];

type DownloadHandler =
    Box<dyn Fn(&NeteaseSong, &DownloadOptions) -> DownloadItemResult + Send + Sync>;

/// 假下载器：记录调用；默认往目标目录写 `<歌名>.mp3` 并返回成功。
struct FakeDownloader {
    calls: Mutex<Vec<(NeteaseSong, DownloadOptions)>>,
    handler: Mutex<Option<DownloadHandler>>,
    running: AtomicUsize,
    peak: AtomicUsize,
    delay: Duration,
}

impl FakeDownloader {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            handler: Mutex::new(None),
            running: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            delay: Duration::ZERO,
        })
    }

    fn slow(delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            handler: Mutex::new(None),
            running: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            delay,
        })
    }

    fn on_download(
        &self,
        handler: impl Fn(&NeteaseSong, &DownloadOptions) -> DownloadItemResult + Send + Sync + 'static,
    ) {
        *self.handler.lock().unwrap() = Some(Box::new(handler));
    }

    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl Downloader for FakeDownloader {
    fn download_one<'a>(
        &'a self,
        song: &'a NeteaseSong,
        options: &'a DownloadOptions,
        _ctx: &'a ItemContext,
        _cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<DownloadItemResult, Cancelled>> {
        self.calls
            .lock()
            .unwrap()
            .push((song.clone(), options.clone()));
        Box::pin(async move {
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            self.running.fetch_sub(1, Ordering::SeqCst);
            let result = match self.handler.lock().unwrap().as_ref() {
                Some(handler) => handler(song, options),
                None => {
                    let output = options.target_directory.join(format!("{}.mp3", song.name));
                    fs::write(&output, mp3_bytes()).unwrap();
                    DownloadItemResult {
                        song_id: song.id,
                        song_name: song.name.clone(),
                        success: true,
                        output_path: Some(output),
                        error_message: None,
                        decrypted_from_ncm: false,
                        skipped: false,
                        quality_note: None,
                        online_match_skipped: false,
                    }
                }
            };
            Ok(result)
        })
    }
}

struct Harness {
    api: Arc<FakeApi>,
    decoder: Arc<FakeDecoder>,
    tags: Arc<RecordingTags>,
    cdn: Arc<StubCdn>,
    downloader: Arc<FakeDownloader>,
    enricher: Option<Arc<FakeEnricher>>,
    service: Arc<SyncService>,
}

fn harness_with(
    decoder: Arc<FakeDecoder>,
    downloader: Arc<FakeDownloader>,
    enricher: Option<Arc<FakeEnricher>>,
) -> Harness {
    let api = Arc::new(FakeApi::default());
    let tags = RecordingTags::new();
    let cdn = StubCdn::new();
    let hook = enricher.clone().map(|e| -> Arc<dyn OnlineEnricher> { e });
    let importer = Arc::new(NcmImportService::new(decoder.clone(), tags.clone(), None));
    let service = Arc::new(SyncService::new(
        api.clone(),
        decoder.clone(),
        tags.clone(),
        importer,
        downloader.clone(),
        MediaFetcher::new(cdn.clone()),
        hook,
    ));
    Harness {
        api,
        decoder,
        tags,
        cdn,
        downloader,
        enricher,
        service,
    }
}

fn harness() -> Harness {
    harness_with(FakeDecoder::new("flac"), FakeDownloader::new(), None)
}

fn sync_song(id: i64) -> NeteaseSong {
    let mut song = song(id);
    song.album.cover_url = Some("https://img.example/sync-cover.jpg".to_owned());
    song.track_number = 2;
    song
}

fn local(path: PathBuf, is_ncm: bool) -> LocalTrack {
    LocalTrack {
        path,
        title: "本地标题".to_owned(),
        artist: Some("本地歌手".to_owned()),
        album: None,
        duration_ms: 240_000,
        format: Some(if is_ncm { "flac" } else { "mp3" }.to_owned()),
        bitrate: None,
        is_ncm,
        ncm_metadata: None,
        netease_id: None,
        has_cover: false,
        title_from_file_name: false,
    }
}

fn download_options(dir: &TempDir) -> DownloadOptions {
    DownloadOptions::new(dir.path())
}

fn sync_options(dir: &TempDir) -> SyncOptions {
    SyncOptions {
        download: download_options(dir),
        redownload_existing: false,
    }
}

async fn run(
    h: &Harness,
    requests: Vec<SyncRequest>,
    options: SyncOptions,
) -> (SyncOutcome, Reports) {
    let reports = Reports::default();
    let outcome = h
        .service
        .sync(requests, options, reports.sink(), &CancelToken::new())
        .await
        .unwrap();
    (outcome, reports)
}

fn ncm_file(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, ncm_bytes()).unwrap();
    path
}

fn request(song: NeteaseSong, local: Option<LocalTrack>) -> SyncRequest {
    SyncRequest { song, local }
}

fn assert_no_temp_files(dir: &TempDir) {
    let leftovers = leftover_temp_files(dir.path());
    assert!(leftovers.is_empty(), "遗留临时文件: {leftovers:?}");
}

#[tokio::test]
async fn a_local_ncm_is_decrypted_and_gets_online_metadata_and_lyrics() {
    let dir = TempDir::new();
    let h = harness_with(
        FakeDecoder::new("flac").with_cover(NCM_COVER.to_vec()),
        FakeDownloader::new(),
        None,
    );
    let ncm = ncm_file(&dir, "Lucky.ncm");
    h.api.on_detail(|_| Ok(Some(sync_song(7))));
    h.api.on_lyrics(|_| Ok(RawLyrics::from_lrc("[00:00.000]x")));
    let mut asked = sync_song(7);
    asked.name = "请求歌名".to_owned();
    asked.artists[0].name = "请求歌手".to_owned();
    asked.album.name = "请求专辑".to_owned();

    let (outcome, reports) = run(
        &h,
        vec![request(asked, Some(local(ncm, true)))],
        sync_options(&dir),
    )
    .await;

    let result = &outcome.results[0];
    assert!(result.success);
    assert_eq!(result.source, SyncSourceKind::DecryptedNcm);
    assert_eq!(outcome.decrypted_count(), 1);
    assert_eq!(outcome.downloaded_count(), 0);
    let output = result.output_path.clone().unwrap();
    assert_eq!(output, dir.path().join("周杰伦 - 晴天.flac"));
    assert!(output.is_file());
    assert_eq!(h.downloader.call_count(), 0);
    let decrypts = h.decoder.calls();
    assert_eq!(decrypts.len(), 1);
    assert!(decrypts[0].1, "解密允许覆盖（目标是临时目录）");

    let tags = h.tags.last();
    assert_eq!(tags.title.as_deref(), Some("晴天"));
    assert_eq!(tags.artists, ["周杰伦"]);
    assert_eq!(tags.cover_data, Some(NCM_COVER.to_vec()));
    assert_eq!(tags.cover_mime_type.as_deref(), Some("image/png"));
    assert_eq!(tags.plain_lyrics.as_deref(), Some("[00:00.000]x"));
    assert_eq!(tags.word_lyrics, None);
    assert_eq!(tags.netease_id, Some(7));
    assert!(
        h.cdn.requests().is_empty(),
        "内嵌封面够用，不该请求封面地址"
    );
    assert_no_temp_files(&dir);

    let reports = reports.all();
    let last = reports.last().unwrap();
    assert_eq!((last.completed, last.total), (1, 1));
    assert!(reports.iter().any(|r| r.stage == "解密 NCM"));
    assert_eq!(last.stage, "NCM 解密完成");
}

#[tokio::test]
async fn auto_match_enriches_the_decrypted_file() {
    let dir = TempDir::new();
    let h = harness_with(
        FakeDecoder::new("flac"),
        FakeDownloader::new(),
        Some(FakeEnricher::new()),
    );
    let ncm = ncm_file(&dir, "Lucky.ncm");
    h.api.on_detail(|_| Ok(Some(sync_song(7))));

    let (outcome, _) = run(
        &h,
        vec![request(sync_song(7), Some(local(ncm, true)))],
        sync_options(&dir),
    )
    .await;

    let result = &outcome.results[0];
    assert!(result.success);
    assert_eq!(result.source, SyncSourceKind::DecryptedNcm);
    assert_eq!(
        h.enricher.as_ref().unwrap().calls(),
        [result.output_path.clone().unwrap()]
    );
    assert!(result.message.as_deref().unwrap().contains("已写入标签"));
    assert!(!outcome.online_match_skipped);
}

#[tokio::test]
async fn auto_match_can_be_switched_off() {
    let dir = TempDir::new();
    let h = harness_with(
        FakeDecoder::new("flac"),
        FakeDownloader::new(),
        Some(FakeEnricher::new()),
    );
    let ncm = ncm_file(&dir, "Lucky.ncm");
    let mut options = sync_options(&dir);
    options.download.auto_match_online = false;

    let (outcome, _) = run(
        &h,
        vec![request(sync_song(7), Some(local(ncm, true)))],
        options,
    )
    .await;

    assert!(outcome.results[0].success);
    assert!(h.enricher.as_ref().unwrap().calls().is_empty());
    assert_eq!(outcome.results[0].message, None);
}

#[tokio::test]
async fn without_an_enricher_the_outcome_flags_the_skipped_online_match() {
    let dir = TempDir::new();
    let h = harness();
    let ncm = ncm_file(&dir, "Lucky.ncm");
    let existing = dir.path().join("existing.mp3");
    fs::write(&existing, [1]).unwrap();

    let (outcome, _) = run(
        &h,
        vec![
            request(sync_song(7), Some(local(ncm, true))),
            request(sync_song(8), Some(local(existing, false))),
        ],
        sync_options(&dir),
    )
    .await;

    assert!(outcome.online_match_skipped);
    assert!(outcome.results[0].online_match_skipped);
    assert!(!outcome.results[1].online_match_skipped);
    // 不在每首歌的消息里重复提示。
    assert_eq!(outcome.results[0].message, None);
}

#[tokio::test]
async fn word_lyrics_mode_writes_the_enhanced_lyrics() {
    let dir = TempDir::new();
    let h = harness();
    let ncm = ncm_file(&dir, "Lucky.ncm");
    h.api.on_lyrics(|_| Ok(RawLyrics::from_lrc("[00:00.000]x")));
    let mut options = sync_options(&dir);
    options.download.lyrics_mode = LyricsWriteMode::Word;

    let (outcome, _) = run(
        &h,
        vec![request(sync_song(7), Some(local(ncm, true)))],
        options,
    )
    .await;

    assert_eq!(outcome.decrypted_count(), 1);
    let tags = h.tags.last();
    assert_eq!(tags.plain_lyrics.as_deref(), Some("[00:00.000]x"));
    assert_eq!(tags.word_lyrics.as_deref(), Some("[00:00.000]x"));
}

#[tokio::test]
async fn an_ncm_import_failure_is_a_failed_item_without_touching_the_downloader() {
    let dir = TempDir::new();
    let decoder = FakeDecoder::new("flac");
    *decoder.handler.lock().unwrap() = Some(Box::new(|path, _, _, _| {
        Ok(NcmDecryptResult::failure(path, "NCM 解密失败"))
    }));
    let h = harness_with(decoder, FakeDownloader::new(), None);
    let ncm = ncm_file(&dir, "Lucky.ncm");

    let (outcome, _) = run(
        &h,
        vec![request(sync_song(7), Some(local(ncm, true)))],
        sync_options(&dir),
    )
    .await;

    let result = &outcome.results[0];
    assert!(!result.success);
    assert_eq!(result.source, SyncSourceKind::Failed);
    assert!(result.message.as_deref().unwrap().contains("解密失败"));
    assert_eq!(h.downloader.call_count(), 0);
    assert_no_temp_files(&dir);
}

#[tokio::test]
async fn a_missing_local_ncm_fails_without_importing_or_downloading() {
    let dir = TempDir::new();
    let h = harness();
    let gone = dir.path().join("gone.ncm");

    let (outcome, _) = run(
        &h,
        vec![request(sync_song(7), Some(local(gone, true)))],
        sync_options(&dir),
    )
    .await;

    let result = &outcome.results[0];
    assert!(!result.success);
    assert_eq!(result.source, SyncSourceKind::Failed);
    assert!(
        result
            .message
            .as_deref()
            .unwrap()
            .starts_with("本地 NCM 文件不存在")
    );
    assert!(h.decoder.calls().is_empty());
    assert_eq!(h.downloader.call_count(), 0);
}

#[tokio::test]
async fn request_metadata_is_the_fallback_when_the_api_and_the_header_fail() {
    let dir = TempDir::new();
    let h = harness();
    h.decoder.read_info_fails.store(true, Ordering::SeqCst);
    h.api
        .on_detail(|_| Err(NeteaseError::protocol("网络不可用")));
    let ncm = ncm_file(&dir, "Lucky.ncm");

    let (outcome, _) = run(
        &h,
        vec![request(sync_song(7), Some(local(ncm, true)))],
        sync_options(&dir),
    )
    .await;

    let result = &outcome.results[0];
    assert!(result.success);
    assert_eq!(result.source, SyncSourceKind::DecryptedNcm);
    assert_eq!(
        result.output_path.as_deref(),
        Some(dir.path().join("周杰伦 - 晴天.flac").as_path())
    );
    let tags = h.tags.last();
    assert_eq!(tags.title.as_deref(), Some("晴天"));
    assert_eq!(tags.artists, ["周杰伦"]);
}

#[tokio::test]
async fn the_file_name_falls_back_to_the_ncm_header_then_the_local_scan() {
    let dir = TempDir::new();
    let h = harness();
    h.api.on_detail(|id| {
        // 在线详情缺歌名与歌手（空串），文件名只能靠 NCM 头部与本地扫描结果。
        let mut detail = song(id);
        detail.name = String::new();
        detail.artists.clear();
        Ok(Some(detail))
    });
    let ncm = ncm_file(&dir, "Lucky.ncm");

    let (outcome, _) = run(
        &h,
        vec![request(sync_song(7), Some(local(ncm, true)))],
        sync_options(&dir),
    )
    .await;

    assert_eq!(
        outcome.results[0]
            .output_path
            .as_ref()
            .unwrap()
            .file_name()
            .unwrap(),
        "NCM 内嵌歌手 - NCM 内嵌歌名.flac"
    );
}

#[tokio::test]
async fn the_http_cover_is_used_when_the_import_has_none() {
    let dir = TempDir::new();
    let h = harness();
    let ncm = ncm_file(&dir, "Lucky.ncm");
    h.cdn
        .serve("https://img.example/sync-cover.jpg", png_bytes(4));
    h.api.on_detail(|_| Ok(Some(sync_song(7))));

    let (outcome, _) = run(
        &h,
        vec![request(sync_song(7), Some(local(ncm, true)))],
        sync_options(&dir),
    )
    .await;

    assert_eq!(outcome.decrypted_count(), 1);
    assert_eq!(h.tags.last().cover_data, Some(png_bytes(4)));
    assert!(
        h.cdn
            .requests()
            .contains(&"https://img.example/sync-cover.jpg".to_owned())
    );
}

#[tokio::test]
async fn an_existing_regular_file_is_skipped_unless_redownloading() {
    let dir = TempDir::new();
    let h = harness();
    let existing = dir.path().join("existing.mp3");
    fs::write(&existing, [1, 2, 3]).unwrap();

    let (outcome, reports) = run(
        &h,
        vec![request(sync_song(1), Some(local(existing.clone(), false)))],
        sync_options(&dir),
    )
    .await;

    let result = &outcome.results[0];
    assert!(result.success);
    assert_eq!(result.source, SyncSourceKind::Skipped);
    assert_eq!(result.output_path.as_deref(), Some(existing.as_path()));
    assert_eq!(result.message.as_deref(), Some("本地已存在，跳过。"));
    assert_eq!(outcome.skipped_count(), 1);
    assert_eq!(h.downloader.call_count(), 0);
    assert_eq!(reports.all().last().unwrap().stage, "已跳过");
    assert!(
        reports
            .all()
            .last()
            .unwrap()
            .finish
            .as_ref()
            .unwrap()
            .skipped
    );
}

#[tokio::test]
async fn an_existing_regular_file_is_redownloaded_when_requested() {
    let dir = TempDir::new();
    let h = harness();
    let existing = dir.path().join("existing.mp3");
    fs::write(&existing, [1, 2, 3]).unwrap();
    let mut options = sync_options(&dir);
    options.redownload_existing = true;
    let expected_options = options.download.clone();

    let (outcome, _) = run(
        &h,
        vec![request(sync_song(1), Some(local(existing, false)))],
        options,
    )
    .await;

    let result = &outcome.results[0];
    assert!(result.success);
    assert_eq!(result.source, SyncSourceKind::Downloaded);
    let calls = h.downloader.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0.id, 1);
    // 下载选项原样传给下载器（目录已规范成绝对路径）。
    assert_eq!(calls[0].1.quality, expected_options.quality);
    assert_eq!(
        calls[0].1.file_name_template,
        expected_options.file_name_template
    );
    assert_eq!(
        calls[0].1.target_directory,
        std::path::absolute(dir.path()).unwrap()
    );
}

#[tokio::test]
async fn a_missing_local_file_is_downloaded_via_the_downloader() {
    let dir = TempDir::new();
    let h = harness();

    let (outcome, _) = run(&h, vec![request(sync_song(1), None)], sync_options(&dir)).await;

    let result = &outcome.results[0];
    assert!(result.success);
    assert_eq!(result.source, SyncSourceKind::Downloaded);
    assert_eq!(
        result.output_path.as_deref(),
        Some(
            std::path::absolute(dir.path())
                .unwrap()
                .join("晴天.mp3")
                .as_path()
        )
    );
    assert_eq!(outcome.downloaded_count(), 1);
    assert_eq!(outcome.success_count(), 1);
    assert_eq!(h.downloader.call_count(), 1);
}

#[tokio::test]
async fn a_downloader_failure_is_a_failed_item_with_its_reason() {
    let dir = TempDir::new();
    let h = harness();
    h.downloader.on_download(|song, _| DownloadItemResult {
        song_id: song.id,
        song_name: song.name.clone(),
        success: false,
        output_path: None,
        error_message: Some("无版权".to_owned()),
        decrypted_from_ncm: false,
        skipped: false,
        quality_note: Some("实际音质：标准".to_owned()),
        online_match_skipped: false,
    });

    let (outcome, _) = run(&h, vec![request(sync_song(1), None)], sync_options(&dir)).await;

    let result = &outcome.results[0];
    assert!(!result.success);
    assert_eq!(result.source, SyncSourceKind::Failed);
    assert_eq!(result.message.as_deref(), Some("无版权"));
    assert_eq!(result.quality_note.as_deref(), Some("实际音质：标准"));
    assert_eq!(outcome.failed_count(), 1);
}

#[tokio::test]
async fn a_downloader_skip_maps_to_the_skipped_source() {
    let dir = TempDir::new();
    let h = harness();
    h.downloader
        .on_download(|song, options| DownloadItemResult {
            song_id: song.id,
            song_name: song.name.clone(),
            success: true,
            output_path: Some(options.target_directory.join("skip.mp3")),
            error_message: None,
            decrypted_from_ncm: false,
            skipped: true,
            quality_note: None,
            online_match_skipped: false,
        });

    let (outcome, _) = run(&h, vec![request(sync_song(1), None)], sync_options(&dir)).await;

    let result = &outcome.results[0];
    assert!(result.success);
    assert_eq!(result.source, SyncSourceKind::Skipped);
    assert_eq!(result.message.as_deref(), Some("已存在，跳过。"));
}

#[tokio::test]
async fn duplicate_requests_produce_one_result_each_in_order() {
    let dir = TempDir::new();
    let h = harness();
    let requests = vec![
        request(sync_song(1), None),
        request(sync_song(2), None),
        request(sync_song(1), None),
    ];

    let (outcome, _) = run(&h, requests, sync_options(&dir)).await;

    let ids: Vec<i64> = outcome.results.iter().map(|r| r.song_id).collect();
    assert_eq!(ids, [1, 2, 1]);
    assert!(
        outcome
            .results
            .iter()
            .all(|r| r.success && r.source == SyncSourceKind::Downloaded)
    );
    assert_eq!(h.downloader.call_count(), 3);
    assert_eq!(outcome.success_count(), 3);
}

#[tokio::test]
async fn progress_is_aggregated_across_the_branches() {
    let dir = TempDir::new();
    let h = harness();
    let existing = dir.path().join("existing.mp3");
    fs::write(&existing, [1]).unwrap();
    let mut options = sync_options(&dir);
    options.download.max_concurrency = 1;

    let (outcome, reports) = run(
        &h,
        vec![
            request(sync_song(1), Some(local(existing, false))),
            request(sync_song(2), None),
        ],
        options,
    )
    .await;

    assert_eq!(outcome.success_count(), 2);
    assert_eq!(outcome.skipped_count(), 1);
    assert_eq!(outcome.downloaded_count(), 1);
    let reports = reports.all();
    let last = reports.last().unwrap();
    assert_eq!((last.completed, last.total), (2, 2));
    assert!(reports.iter().any(|r| r.stage == "已跳过"));
    assert!(reports.iter().any(|r| r.stage == "下载完成"));
    assert!(reports.iter().any(|r| r.stage == "处理中"));
}

#[tokio::test]
async fn a_tag_write_failure_after_decrypting_is_still_a_success_with_a_warning() {
    let dir = TempDir::new();
    let h = harness();
    // 只让最后一步（写到 `.tmp` 临时副本）失败，解密后的基础标签照常写入。
    h.tags.fail_when(|path| {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().contains(".tmp"))
    });
    let ncm = ncm_file(&dir, "Lucky.ncm");

    let (outcome, _) = run(
        &h,
        vec![request(sync_song(7), Some(local(ncm, true)))],
        sync_options(&dir),
    )
    .await;

    let result = &outcome.results[0];
    assert!(result.success);
    assert_eq!(result.source, SyncSourceKind::DecryptedNcm);
    assert!(result.message.as_deref().unwrap().contains("标签写入失败"));
    assert!(result.output_path.as_ref().unwrap().is_file());
    assert_no_temp_files(&dir);
}

#[tokio::test]
async fn a_pre_cancelled_sync_processes_nothing() {
    let dir = TempDir::new();
    let h = harness();
    let cancel = CancelToken::new();
    cancel.cancel();

    let outcome = h
        .service
        .sync(
            vec![request(sync_song(1), None)],
            sync_options(&dir),
            Reports::default().sink(),
            &cancel,
        )
        .await
        .unwrap();

    assert!(outcome.cancelled);
    assert!(outcome.results.is_empty());
    assert_eq!(h.downloader.call_count(), 0);
}

#[tokio::test]
async fn cancelling_keeps_the_finished_results_and_cleans_up_the_rest() {
    let dir = TempDir::new();
    let h = harness();
    let cancel = CancelToken::new();
    let seen = Arc::new(AtomicUsize::new(0));
    let (counter, token) = (Arc::clone(&seen), cancel.clone());
    h.downloader.on_download(move |song, options| {
        if counter.fetch_add(1, Ordering::SeqCst) == 1 {
            token.cancel();
        }
        let output = options.target_directory.join(format!("{}.mp3", song.id));
        fs::write(&output, [1]).unwrap();
        DownloadItemResult {
            song_id: song.id,
            song_name: song.name.clone(),
            success: true,
            output_path: Some(output),
            error_message: None,
            decrypted_from_ncm: false,
            skipped: false,
            quality_note: None,
            online_match_skipped: false,
        }
    });
    let mut options = sync_options(&dir);
    options.download.max_concurrency = 1;

    let outcome = h
        .service
        .sync(
            (1..=5).map(|id| request(sync_song(id), None)).collect(),
            options,
            Reports::default().sink(),
            &cancel,
        )
        .await
        .unwrap();

    assert!(outcome.cancelled);
    assert_eq!(outcome.total, 5);
    assert_eq!(outcome.results.len(), 2);
    assert_eq!(h.downloader.call_count(), 2);
}

#[tokio::test]
async fn an_empty_request_list_is_an_empty_outcome() {
    let dir = TempDir::new();
    let h = harness();

    let (outcome, _) = run(&h, Vec::new(), sync_options(&dir)).await;

    assert!(outcome.results.is_empty());
    assert_eq!(outcome.total, 0);
    assert!(!outcome.cancelled);
}

#[tokio::test]
async fn a_blank_target_directory_is_refused() {
    let h = harness();
    let error = h
        .service
        .sync(
            vec![request(sync_song(1), None)],
            SyncOptions {
                download: DownloadOptions::new(""),
                redownload_existing: false,
            },
            Reports::default().sink(),
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, DownloadError::EmptyDirectory);
}

#[tokio::test]
async fn decrypting_never_overwrites_an_unrelated_file_named_after_the_ncm() {
    let dir = TempDir::new();
    let h = harness();
    // 下载目录里恰好有一个与 `.ncm` 主干同名的音频：C# 会把它覆盖，这里解密在临时子目录里，不受影响。
    let bystander = dir.path().join("Lucky.flac");
    fs::write(&bystander, b"precious").unwrap();
    let ncm = ncm_file(&dir, "Lucky.ncm");

    let (outcome, _) = run(
        &h,
        vec![request(sync_song(7), Some(local(ncm, true)))],
        sync_options(&dir),
    )
    .await;

    assert!(outcome.results[0].success);
    assert_eq!(fs::read(&bystander).unwrap(), b"precious");
    assert_no_temp_files(&dir);
}

#[tokio::test]
async fn items_are_processed_concurrently_up_to_the_limit_and_results_keep_their_order() {
    let dir = TempDir::new();
    let downloader = FakeDownloader::slow(Duration::from_millis(30));
    let h = harness_with(FakeDecoder::new("flac"), downloader, None);
    let mut options = sync_options(&dir);
    options.download.max_concurrency = 3;

    let (outcome, _) = run(
        &h,
        (1..=7).map(|id| request(sync_song(id), None)).collect(),
        options,
    )
    .await;

    assert_eq!(outcome.success_count(), 7);
    assert_eq!(h.downloader.peak.load(Ordering::SeqCst), 3);
    let ids: Vec<i64> = outcome.results.iter().map(|r| r.song_id).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5, 6, 7]);
}

#[tokio::test]
async fn the_three_branches_run_together() {
    let dir = TempDir::new();
    let h = harness();
    let existing = dir.path().join("present.mp3");
    fs::write(&existing, [1]).unwrap();
    let ncm = ncm_file(&dir, "needs.ncm");

    let (outcome, _) = run(
        &h,
        vec![
            request(sync_song(1), Some(local(existing, false))),
            request(sync_song(2), Some(local(ncm, true))),
            request(sync_song(3), None),
        ],
        sync_options(&dir),
    )
    .await;

    assert_eq!(outcome.skipped_count(), 1);
    assert_eq!(outcome.decrypted_count(), 1);
    assert_eq!(outcome.downloaded_count(), 1);
    assert_eq!(outcome.failed_count(), 0);
}

#[tokio::test]
async fn the_real_pipeline_decrypts_a_synthetic_ncm_into_a_tagged_mp3() {
    use crate::ncm::FileNcmDecoder;
    use crate::ncm::synthetic::{SyntheticNcm, fake_png_cover};

    let dir = TempDir::new();
    let ncm_dir = TempDir::new();
    let ncm_path = ncm_dir.path().join("Lucky.ncm");
    let audio = crate::tags::fixtures::mp3_default();
    fs::write(
        &ncm_path,
        SyntheticNcm::new().cover(&fake_png_cover()).build(&audio),
    )
    .unwrap();

    let tags: Arc<dyn AudioTagService> = Arc::new(LoftyTagService);
    let decoder: Arc<dyn crate::ncm::NcmDecoder> = Arc::new(FileNcmDecoder);
    let api = Arc::new(FakeApi::default());
    api.on_detail(|_| Ok(Some(sync_song(7))));
    api.on_lyrics(|_| Ok(RawLyrics::from_lrc("[00:01.000]同步歌词")));
    let importer = Arc::new(NcmImportService::new(
        Arc::clone(&decoder),
        Arc::clone(&tags),
        None,
    ));
    let cdn = StubCdn::new();
    let service = Arc::new(SyncService::new(
        api,
        decoder,
        Arc::clone(&tags),
        importer,
        FakeDownloader::new(),
        MediaFetcher::new(cdn),
        None,
    ));

    let outcome = service
        .sync(
            vec![request(sync_song(7), Some(local(ncm_path, true)))],
            sync_options(&dir),
            Reports::default().sink(),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    let result = &outcome.results[0];
    assert!(result.success, "{:?}", result.message);
    assert_eq!(result.source, SyncSourceKind::DecryptedNcm);
    let output = result.output_path.clone().unwrap();
    assert_eq!(output.file_name().unwrap(), "周杰伦 - 晴天.mp3");
    let info = tags.read(&output).unwrap().expect("可以读回标签");
    assert_eq!(info.title.as_deref(), Some("晴天"));
    assert_eq!(info.artist.as_deref(), Some("周杰伦"));
    assert_eq!(info.album.as_deref(), Some("叶惠美"));
    assert_eq!(info.netease_id, Some(7));
    assert_eq!(info.year, Some(2003));
    assert!(info.has_cover);
    assert!(info.lyrics.as_deref().unwrap().contains("同步歌词"));
    assert_no_temp_files(&dir);
    assert_eq!(file_names(dir.path()), ["周杰伦 - 晴天.mp3"]);
}

#[test]
fn sync_progress_and_item_results_serialize_for_the_frontend() {
    let result = SyncItemResult {
        song_id: 1,
        song_name: "a".to_owned(),
        success: true,
        source: SyncSourceKind::DecryptedNcm,
        output_path: Some(PathBuf::from(r"D:\a.flac")),
        message: Some("m".to_owned()),
        quality_note: None,
        online_match_skipped: false,
    };
    let finish = serde_json::to_value(result.to_finish()).unwrap();
    assert_eq!(finish["source"], "decryptedNcm");
    assert_eq!(finish["skipped"], false);
    assert_eq!(finish["outputPath"], r"D:\a.flac");
    assert_eq!(result.stage(), "NCM 解密完成");
    assert_eq!(SyncItemResult::failed(&song(1), "x").stage(), "失败");
}
