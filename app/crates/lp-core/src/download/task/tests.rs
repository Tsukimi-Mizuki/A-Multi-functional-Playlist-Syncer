use std::fs;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::download::fetch::MediaFetcher;
use crate::download::ncm_import::NcmImportService;
use crate::download::service::{DownloadProgress, Downloader};
use crate::download::test_support::*;
use crate::library::LocalLibraryService;
use crate::models::{AudioQuality, LyricsWriteMode};
use crate::netease::{NeteaseSong, SongPlaybackInfo};
use crate::test_util::TempDir;

/// 名字各不相同的歌曲：`歌<id>`。
fn named(id: i64) -> NeteaseSong {
    let mut song = song_no_cover(id);
    song.name = format!("歌{id}");
    song
}

struct Rig {
    api: Arc<FakeApi>,
    cdn: Arc<StubCdn>,
    library: Arc<LibraryManager>,
    logs: Arc<LogHub>,
    manager: Arc<TransferManager>,
    library_dir: TempDir,
    download_dir: TempDir,
    reports: Reports,
    started: Arc<Mutex<Vec<TransferStarted>>>,
    scans: Arc<Mutex<usize>>,
}

impl Rig {
    fn new() -> Self {
        let api = FakeApi::playing("mp3");
        let decoder = FakeDecoder::new("flac");
        let tags = RecordingTags::new();
        let cdn = StubCdn::new();
        let logs = LogHub::new();
        let fetcher = MediaFetcher::new(cdn.clone());
        let downloader = Arc::new(DownloadService::new(
            api.clone(),
            tags.clone(),
            decoder.clone(),
            fetcher.clone(),
            None,
        ));
        let importer = Arc::new(NcmImportService::new(decoder.clone(), tags.clone(), None));
        let sync = Arc::new(SyncService::new(
            api.clone(),
            decoder.clone(),
            tags.clone(),
            importer,
            downloader.clone() as Arc<dyn Downloader>,
            fetcher,
            None,
        ));
        let library = LibraryManager::new(
            Arc::new(LocalLibraryService::new(decoder, tags)),
            Arc::clone(&logs),
        );
        let manager = Arc::new(TransferManager::new(
            downloader,
            sync,
            Arc::clone(&library),
            Arc::clone(&logs),
        ));
        Self {
            api,
            cdn,
            library,
            logs,
            manager,
            library_dir: TempDir::new(),
            download_dir: TempDir::new(),
            reports: Reports::default(),
            started: Arc::default(),
            scans: Arc::default(),
        }
    }

    /// 歌单 5：歌1、歌2、歌3 与一首 ID 为 555 的歌（对应假解密器的 NCM 元数据）。
    fn load_playlist(&self) {
        self.library
            .remember_playlist(5, &[named(1), named(2), named(3), named(555), named(2)]);
        for id in [1, 2, 3, 555] {
            self.cdn
                .serve(&format!("https://cdn.example/{id}.mp3"), mp3_bytes());
        }
    }

    fn settings(&self) -> AppSettings {
        let mut settings = AppSettings::new(&self.download_dir.path().to_string_lossy());
        settings.library_directory = self.library_dir.path().to_string_lossy().into_owned();
        settings.write_cover = false;
        settings.lyrics_mode = LyricsWriteMode::None;
        settings
    }

    fn present(&self, name: &str) {
        fs::write(self.library_dir.path().join(name), mp3_bytes()).unwrap();
    }

    fn pending_ncm(&self) {
        fs::write(self.library_dir.path().join("等待.ncm"), ncm_bytes()).unwrap();
    }

    fn messages(&self) -> Vec<String> {
        self.logs.recent().into_iter().map(|e| e.message).collect()
    }

    fn has_message(&self, text: &str) -> bool {
        self.messages().iter().any(|m| m == text)
    }

    async fn download_tracks(
        &self,
        ids: &[i64],
        settings: &AppSettings,
    ) -> Result<TransferSummaryView, TransferError> {
        let started = Arc::clone(&self.started);
        let scans = Arc::clone(&self.scans);
        let record = move |plan: TransferStarted| started.lock().unwrap().push(plan);
        let channels = TransferChannels {
            started: &record,
            sink: self.reports.sink(),
            scan_progress: Arc::new(move |_| *scans.lock().unwrap() += 1),
        };
        self.manager
            .download_tracks(5, ids, settings, &channels)
            .await
    }

    async fn download_missing(
        &self,
        settings: &AppSettings,
    ) -> Result<TransferSummaryView, TransferError> {
        self.download_missing_only(None, settings).await
    }

    async fn download_missing_only(
        &self,
        only: Option<&[i64]>,
        settings: &AppSettings,
    ) -> Result<TransferSummaryView, TransferError> {
        let started = Arc::clone(&self.started);
        let record = move |plan: TransferStarted| started.lock().unwrap().push(plan);
        let channels = TransferChannels {
            started: &record,
            sink: self.reports.sink(),
            scan_progress: Arc::new(|_| {}),
        };
        self.manager
            .download_missing(5, only, settings, &channels)
            .await
    }

    async fn sync(&self, settings: &AppSettings) -> Result<TransferSummaryView, TransferError> {
        let started = Arc::clone(&self.started);
        let record = move |plan: TransferStarted| started.lock().unwrap().push(plan);
        let channels = TransferChannels {
            started: &record,
            sink: self.reports.sink(),
            scan_progress: Arc::new(|_| {}),
        };
        self.manager.sync_playlist(5, settings, &channels).await
    }
}

#[tokio::test]
async fn download_tracks_downloads_only_the_chosen_songs_in_order_without_duplicates() {
    let rig = Rig::new();
    rig.load_playlist();
    let settings = rig.settings();

    let summary = rig
        .download_tracks(&[3, 1, 3, 999], &settings)
        .await
        .unwrap();

    assert_eq!(summary.kind, TransferKind::Selected);
    assert_eq!(summary.total, 2);
    assert_eq!(summary.succeeded, 2);
    assert_eq!(summary.downloaded, 2);
    assert_eq!(summary.failed, 0);
    assert!(!summary.cancelled);
    let ids: Vec<i64> = summary.results.iter().map(|r| r.song_id).collect();
    assert_eq!(ids, [3, 1]);
    assert_eq!(
        file_names(rig.download_dir.path()),
        ["周杰伦 - 歌1.mp3", "周杰伦 - 歌3.mp3"]
    );
    let plan = rig.started.lock().unwrap()[0].clone();
    assert_eq!(plan.kind, TransferKind::Selected);
    assert_eq!(plan.total, 2);
    assert_eq!(plan.song_ids, [3, 1]);
    assert!(rig.has_message(&format!(
        "开始下载 2 首歌曲 → {}（音质：极高 (HQ)）",
        settings.download_directory
    )));
    assert!(rig.has_message("下载完成：成功 2 首，跳过 0 首"));
    // 默认没有接入在线补全：只提示一次。
    assert!(summary.online_match_skipped);
    assert_eq!(
        rig.messages()
            .iter()
            .filter(|m| m.contains("将在后续版本提供"))
            .count(),
        1
    );
}

#[tokio::test]
async fn the_library_is_rescanned_after_a_task_so_the_comparison_can_refresh() {
    let rig = Rig::new();
    rig.load_playlist();
    let mut settings = rig.settings();
    // 下载目录就是曲库目录。
    settings.library_directory = settings.download_directory.clone();

    let summary = rig.download_tracks(&[1], &settings).await.unwrap();

    assert!(summary.library_refreshed);
    assert_eq!(rig.library.summary().unwrap().total_files, 1);
    assert!(*rig.scans.lock().unwrap() > 0, "重新扫描的进度要推给界面");
    // 对比结果立刻反映新文件。
    let matched = rig
        .library
        .match_playlist_raw(
            5,
            ScanMode::Never,
            &settings.library_directory,
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    assert_eq!(
        matched.availability(0),
        Some(crate::library::LocalAvailability::Available)
    );
}

#[tokio::test]
async fn a_missing_library_directory_does_not_stop_the_task_but_is_not_refreshed() {
    let rig = Rig::new();
    rig.load_playlist();
    let mut settings = rig.settings();
    settings.library_directory = rig
        .library_dir
        .path()
        .join("不存在")
        .to_string_lossy()
        .into_owned();

    let summary = rig.download_tracks(&[1], &settings).await.unwrap();

    assert_eq!(summary.succeeded, 1);
    assert!(!summary.library_refreshed);
}

#[tokio::test]
async fn download_tracks_reports_empty_and_unknown_selections() {
    let rig = Rig::new();
    rig.load_playlist();
    let settings = rig.settings();

    let none = rig.download_tracks(&[], &settings).await.unwrap_err();
    assert_eq!(none.kind, "noSongs");
    let unknown = rig.download_tracks(&[404], &settings).await.unwrap_err();
    assert_eq!(unknown.kind, "noSongs");
    assert!(unknown.message.contains("勾选"));
    // 报错后任务槽被释放。
    assert!(!rig.manager.is_running());
}

#[tokio::test]
async fn an_unloaded_playlist_is_reported() {
    let rig = Rig::new();
    let settings = rig.settings();

    let error = rig.download_tracks(&[1], &settings).await.unwrap_err();

    assert_eq!(error.kind, "playlistNotLoaded");
}

#[tokio::test]
async fn download_missing_skips_what_the_library_has_and_the_pending_ncm() {
    let rig = Rig::new();
    rig.load_playlist();
    rig.present("周杰伦 - 歌1.mp3");
    rig.pending_ncm();
    let settings = rig.settings();

    let summary = rig.download_missing(&settings).await.unwrap();

    // 歌2、歌3 缺失；歌1 已存在；555 是待解密的 NCM（不算缺失）；歌单里重复的歌2 只处理一次。
    assert_eq!(summary.kind, TransferKind::Missing);
    assert_eq!(summary.total, 2);
    let ids: Vec<i64> = summary.results.iter().map(|r| r.song_id).collect();
    assert_eq!(ids, [2, 3]);
    assert_eq!(summary.downloaded, 2);
    assert_eq!(rig.started.lock().unwrap()[0].song_ids, [2, 3]);
}

#[tokio::test]
async fn download_missing_keeps_only_the_confirmed_songs_that_are_still_missing() {
    let rig = Rig::new();
    rig.load_playlist();
    rig.present("周杰伦 - 歌1.mp3");
    rig.pending_ncm();
    let settings = rig.settings();

    // 确认框里留下歌3、已存在的歌1、待解密的 555 与不在歌单里的 999；取消勾选了歌2。
    let summary = rig
        .download_missing_only(Some(&[3, 1, 555, 999]), &settings)
        .await
        .unwrap();

    assert_eq!(summary.kind, TransferKind::Missing);
    assert_eq!(summary.total, 1);
    let ids: Vec<i64> = summary.results.iter().map(|r| r.song_id).collect();
    assert_eq!(ids, [3]);
    assert_eq!(rig.started.lock().unwrap()[0].song_ids, [3]);
    assert_eq!(file_names(rig.download_dir.path()), ["周杰伦 - 歌3.mp3"]);
}

#[tokio::test]
async fn download_missing_with_an_empty_or_stale_confirmation_downloads_nothing() {
    let rig = Rig::new();
    rig.load_playlist();
    rig.present("周杰伦 - 歌1.mp3");
    let settings = rig.settings();

    let empty = rig
        .download_missing_only(Some(&[]), &settings)
        .await
        .unwrap_err();
    assert_eq!(empty.kind, "noSongs");
    assert!(empty.message.contains("至少勾选一首"));
    // 确认之后歌1 已经出现在曲库里：不再下载。
    let stale = rig
        .download_missing_only(Some(&[1]), &settings)
        .await
        .unwrap_err();
    assert_eq!(stale.kind, "noSongs");
    assert!(stale.message.contains("没有需要下载的歌曲"));
    assert!(rig.started.lock().unwrap().is_empty());
    assert!(!rig.manager.is_running());
}

#[tokio::test]
async fn download_missing_with_nothing_missing_says_so() {
    let rig = Rig::new();
    rig.library.remember_playlist(5, &[named(1)]);
    rig.present("周杰伦 - 歌1.mp3");

    let error = rig.download_missing(&rig.settings()).await.unwrap_err();

    assert_eq!(error.kind, "noSongs");
    assert!(error.message.contains("没有需要下载的歌曲"));
    assert!(rig.started.lock().unwrap().is_empty());
}

#[tokio::test]
async fn download_missing_treats_a_missing_library_directory_as_empty() {
    let rig = Rig::new();
    rig.load_playlist();
    let mut settings = rig.settings();
    settings.library_directory = rig
        .library_dir
        .path()
        .join("不存在")
        .to_string_lossy()
        .into_owned();

    let summary = rig.download_missing(&settings).await.unwrap();

    assert_eq!(summary.total, 4, "去掉重复的歌2，其余全部算缺失");
    assert!(
        rig.messages()
            .iter()
            .any(|m| m.starts_with("曲库目录不存在，按本地曲库为空处理"))
    );
}

#[tokio::test]
async fn sync_runs_the_three_branches_and_summarises_them() {
    let rig = Rig::new();
    rig.load_playlist();
    rig.present("周杰伦 - 歌1.mp3");
    rig.pending_ncm();
    let settings = rig.settings();

    let summary = rig.sync(&settings).await.unwrap();

    assert_eq!(summary.kind, TransferKind::Sync);
    assert_eq!(summary.total, 4, "歌单里重复的歌2 只同步一次");
    assert_eq!(summary.skipped, 1);
    assert_eq!(summary.decrypted, 1);
    assert_eq!(summary.downloaded, 2);
    assert_eq!(summary.succeeded, 3);
    assert_eq!(summary.failed, 0);
    let by_id = |id: i64| summary.results.iter().find(|r| r.song_id == id).unwrap();
    assert_eq!(by_id(1).source, ItemSource::Skipped);
    assert_eq!(by_id(555).source, ItemSource::DecryptedNcm);
    assert_eq!(by_id(2).source, ItemSource::Downloaded);
    assert!(rig.has_message(&format!(
        "开始同步 4 首歌曲 → {}（音质：极高 (HQ)）",
        settings.download_directory
    )));
    assert!(rig.has_message("同步完成：成功 4 首（下载 2、NCM 解密 1、跳过 1），失败 0 首"));
    assert!(summary.library_refreshed);
}

#[tokio::test]
async fn sync_logs_failures_and_quality_downgrades() {
    let rig = Rig::new();
    rig.library.remember_playlist(5, &[named(1), named(2)]);
    rig.cdn.serve("https://cdn.example/2.mp3", mp3_bytes());
    rig.api.on_playback(|id, _| {
        if id == 1 {
            Ok(SongPlaybackInfo {
                song_id: id,
                url: None,
                error_message: Some("版权受限".to_owned()),
                ..SongPlaybackInfo::default()
            })
        } else {
            Ok(SongPlaybackInfo {
                level: Some("standard".to_owned()),
                ..playable(id, "mp3")
            })
        }
    });
    let mut settings = rig.settings();
    settings.quality = AudioQuality::Lossless;

    let summary = rig.sync(&settings).await.unwrap();

    assert_eq!(summary.failed, 1);
    assert_eq!(summary.succeeded, 1);
    assert!(rig.has_message("同步完成：成功 1 首（下载 1、NCM 解密 0、跳过 0），失败 1 首"));
    assert!(rig.has_message("同步失败：歌1 · 版权受限"));
    assert!(
        rig.messages()
            .iter()
            .any(|m| m.starts_with("歌2 · 实际音质：标准（无损 (SQ) 不可用"))
    );
    let failed = summary.results.iter().find(|r| r.song_id == 1).unwrap();
    assert_eq!(failed.message.as_deref(), Some("版权受限"));
    let downgraded = summary.results.iter().find(|r| r.song_id == 2).unwrap();
    assert!(
        downgraded
            .quality_note
            .as_deref()
            .unwrap()
            .contains("已自动降级")
    );
}

#[tokio::test]
async fn redownload_follows_the_overwrite_setting() {
    let rig = Rig::new();
    rig.library.remember_playlist(5, &[named(1)]);
    rig.cdn.serve("https://cdn.example/1.mp3", mp3_bytes());
    rig.present("周杰伦 - 歌1.mp3");
    let mut settings = rig.settings();

    let kept = rig.sync(&settings).await.unwrap();
    assert_eq!(kept.skipped, 1);

    settings.overwrite_existing = true;
    let again = rig.sync(&settings).await.unwrap();
    assert_eq!(again.downloaded, 1);
    assert_eq!(rig.api.playback_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn only_one_task_runs_at_a_time_and_cancel_reaches_it() {
    let rig = Rig::new();
    rig.load_playlist();
    let settings = rig.settings();
    let gate = Arc::new(tokio::sync::Notify::new());
    let entered = Arc::new(tokio::sync::Notify::new());
    let (g, e) = (Arc::clone(&gate), Arc::clone(&entered));
    rig.cdn.respond_with(move |_| {
        let (gate, entered) = (Arc::clone(&g), Arc::clone(&e));
        Box::pin(async move {
            entered.notify_one();
            gate.notified().await;
            Ok(ok_response(mp3_bytes()))
        })
    });

    let first = {
        let (rig_manager, settings) = (Arc::clone(&rig.manager), settings.clone());
        let sink = rig.reports.sink();
        tokio::spawn(async move {
            let record = |_: TransferStarted| {};
            let channels = TransferChannels {
                started: &record,
                sink,
                scan_progress: Arc::new(|_| {}),
            };
            rig_manager
                .download_tracks(5, &[1], &settings, &channels)
                .await
        })
    };
    entered.notified().await;
    assert!(rig.manager.is_running());

    let busy = rig.download_tracks(&[2], &settings).await.unwrap_err();
    assert_eq!(busy.kind, "busy");
    let busy_sync = rig.sync(&settings).await.unwrap_err();
    assert_eq!(busy_sync.kind, "busy");

    assert!(rig.manager.cancel());
    let summary = tokio::time::timeout(Duration::from_secs(5), first)
        .await
        .expect("取消后应当立刻返回")
        .unwrap()
        .unwrap();
    assert!(summary.cancelled);
    assert!(summary.results.is_empty());
    assert!(!rig.manager.is_running());
    assert!(!rig.manager.cancel(), "没有任务时取消返回 false");
    assert!(
        rig.messages()
            .iter()
            .any(|m| m.starts_with("下载已取消：已处理 0/1"))
    );
    // 取消后可以立刻开始下一个任务。
    rig.cdn
        .respond_with(|_| Box::pin(async { Ok(ok_response(mp3_bytes())) }));
    assert!(rig.download_tracks(&[2], &settings).await.is_ok());
}

#[tokio::test]
async fn failed_downloads_are_logged_with_their_reason() {
    let rig = Rig::new();
    rig.library.remember_playlist(5, &[named(1)]);
    // 没有登记 CDN 内容 → 404。

    let summary = rig.download_tracks(&[1], &rig.settings()).await.unwrap();

    assert_eq!(summary.failed, 1);
    assert!(
        rig.messages()
            .iter()
            .any(|m| m.starts_with("下载失败：歌1 · 媒体下载失败：HTTP 404"))
    );
    assert!(rig.has_message("下载完成：成功 0 首，跳过 0 首，失败 1 首"));
}

#[tokio::test]
async fn progress_events_reach_the_sink_with_song_ids_for_every_row() {
    let rig = Rig::new();
    rig.load_playlist();

    rig.sync(&rig.settings()).await.unwrap();

    let events: Vec<DownloadProgress> = rig.reports.all();
    let finished: Vec<i64> = events
        .iter()
        .filter(|e| e.finish.is_some())
        .map(|e| e.song_id)
        .collect();
    let mut sorted = finished.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, [1, 2, 3, 555]);
    assert!(events.iter().all(|e| e.total == 4));
}

#[test]
fn the_task_slot_allows_one_task_and_releases_on_drop() {
    let slot = TaskSlot::default();
    assert!(!slot.is_running());
    assert!(!slot.cancel());

    let guard = slot.try_begin().unwrap();
    assert!(slot.is_running());
    assert_eq!(slot.try_begin().unwrap_err().kind, "busy");
    assert!(slot.clone().try_begin().is_err());
    assert!(slot.cancel());
    assert!(guard.cancel.is_cancelled());

    drop(guard);
    assert!(!slot.is_running());
    let next = slot.try_begin().unwrap();
    assert!(!next.cancel.is_cancelled(), "上一个任务的取消不影响下一个");
}

#[test]
fn views_serialize_to_camel_case_for_the_frontend() {
    let started = serde_json::to_value(TransferStarted {
        kind: TransferKind::Missing,
        total: 2,
        song_ids: vec![1, 2],
    })
    .unwrap();
    assert_eq!(started["kind"], "missing");
    assert_eq!(started["songIds"], serde_json::json!([1, 2]));

    let summary = serde_json::to_value(TransferSummaryView {
        kind: TransferKind::Sync,
        total: 1,
        succeeded: 1,
        downloaded: 0,
        decrypted: 1,
        skipped: 0,
        failed: 0,
        cancelled: false,
        online_match_skipped: true,
        library_refreshed: true,
        results: vec![TransferItemView {
            song_id: 9,
            song_name: "x".to_owned(),
            success: true,
            skipped: false,
            source: ItemSource::DecryptedNcm,
            output_path: Some(r"D:\x.flac".to_owned()),
            message: None,
            quality_note: None,
        }],
    })
    .unwrap();
    assert_eq!(summary["kind"], "sync");
    assert_eq!(summary["onlineMatchSkipped"], true);
    assert_eq!(summary["libraryRefreshed"], true);
    assert_eq!(summary["results"][0]["songId"], 9);
    assert_eq!(summary["results"][0]["source"], "decryptedNcm");

    let error = serde_json::to_value(TransferError {
        kind: "busy",
        message: "忙".to_owned(),
    })
    .unwrap();
    assert_eq!(error["kind"], "busy");
    assert_eq!(error["message"], "忙");
}
