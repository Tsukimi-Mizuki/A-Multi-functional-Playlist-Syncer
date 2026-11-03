//! 曲库编排：扫描结果缓存、歌单曲目缓存、一次只跑一个扫描、对比并写运行日志。
//!
//! C# 版把这些放在 `MainWindowViewModel`（`_localTracks`、`TryScanLibraryInternalAsync`、`RebuildRows`）里，
//! 这里下沉到 core，Tauri 命令只做转发与事件推送。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::matcher::SongMatcher;
use super::scanner::LocalLibraryService;
use super::track::{LibraryError, LibraryScanProgress, LocalTrack};
use super::view::{
    LibraryCommandError, LibrarySummaryView, MatchOutcomeView, ScanMode, ScanProgressView,
    build_match_view, unscanned_view,
};
use crate::logs::LogHub;
use crate::netease::NeteaseSong;
use crate::utilities::CancelToken;

/// 扫描进度事件的最小间隔：大曲库一秒可以处理几千个文件，每个都推给界面只会拖慢它。
const PROGRESS_INTERVAL: Duration = Duration::from_millis(80);

/// 内存里保留的歌单曲目份数（给 `match_playlist` 用）。
const MAX_REMEMBERED_PLAYLISTS: usize = 6;

/// 扫描进度的接收方（会从工作线程调用）。
pub type ScanProgressSink = Arc<dyn Fn(ScanProgressView) + Send + Sync>;

#[derive(Clone)]
struct Snapshot {
    directory: String,
    tracks: Arc<Vec<LocalTrack>>,
    skipped_directories: usize,
}

impl Snapshot {
    fn summary(&self) -> LibrarySummaryView {
        LibrarySummaryView {
            directory: self.directory.clone(),
            total_files: self.tracks.len(),
            ncm_files: self.tracks.iter().filter(|t| t.is_ncm).count(),
            skipped_directories: self.skipped_directories,
        }
    }
}

/// 曲库编排器。
pub struct LibraryManager {
    service: Arc<LocalLibraryService>,
    matcher: SongMatcher,
    logs: Arc<LogHub>,
    snapshot: Mutex<Option<Snapshot>>,
    playlists: Mutex<Vec<(i64, Arc<Vec<NeteaseSong>>)>>,
    /// 同一时间只允许一个扫描在跑；后来的请求排队等它结束（然后直接复用它的结果）。
    scan_lock: tokio::sync::Mutex<()>,
    /// 正在运行的扫描的取消令牌。
    running: Mutex<Option<CancelToken>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl LibraryManager {
    /// 创建编排器。
    pub fn new(service: Arc<LocalLibraryService>, logs: Arc<LogHub>) -> Arc<Self> {
        Arc::new(Self {
            service,
            matcher: SongMatcher,
            logs,
            snapshot: Mutex::new(None),
            playlists: Mutex::new(Vec::new()),
            scan_lock: tokio::sync::Mutex::new(()),
            running: Mutex::new(None),
        })
    }

    /// 记住刚加载的歌单曲目，供之后按歌单 ID 对比（同一个歌单重新加载会替换旧的）。
    pub fn remember_playlist(&self, playlist_id: i64, songs: &[NeteaseSong]) {
        let mut playlists = lock(&self.playlists);
        playlists.retain(|(id, _)| *id != playlist_id);
        playlists.push((playlist_id, Arc::new(songs.to_vec())));
        if playlists.len() > MAX_REMEMBERED_PLAYLISTS {
            playlists.remove(0);
        }
    }

    /// 请求取消正在运行的扫描；返回是否确实有扫描在运行。
    pub fn cancel(&self) -> bool {
        match lock(&self.running).as_ref() {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// 是否有扫描在运行。
    pub fn is_scanning(&self) -> bool {
        lock(&self.running).is_some()
    }

    /// 当前缓存的扫描结果摘要（没有扫描过为 `None`）。
    pub fn summary(&self) -> Option<LibrarySummaryView> {
        lock(&self.snapshot).as_ref().map(Snapshot::summary)
    }

    /// 丢弃缓存的扫描结果（曲库目录变了、需要强制重扫时用）。
    pub fn invalidate(&self) {
        *lock(&self.snapshot) = None;
    }

    /// 重新扫描曲库目录并缓存结果。
    pub async fn scan(
        &self,
        directory: &str,
        progress: ScanProgressSink,
    ) -> Result<LibrarySummaryView, LibraryCommandError> {
        self.ensure_snapshot(directory, true, progress)
            .await
            .map(|snapshot| snapshot.summary())
    }

    /// 把歌单与曲库对比。`mode` 决定还没有扫描结果时是先扫描还是直接返回「尚未对比」。
    pub async fn match_playlist(
        &self,
        playlist_id: i64,
        mode: ScanMode,
        directory: &str,
        progress: ScanProgressSink,
    ) -> Result<MatchOutcomeView, LibraryCommandError> {
        let songs = lock(&self.playlists)
            .iter()
            .find(|(id, _)| *id == playlist_id)
            .map(|(_, songs)| Arc::clone(songs))
            .ok_or_else(|| {
                LibraryCommandError::new(
                    "playlistNotLoaded",
                    "歌单还没有加载完成，请稍后再对比本地曲库。",
                )
            })?;

        let current = lock(&self.snapshot)
            .clone()
            .filter(|snapshot| same_directory(&snapshot.directory, directory));
        let snapshot = match (current, mode) {
            (Some(snapshot), _) => snapshot,
            (None, ScanMode::Never) => {
                return Ok(unscanned_view(playlist_id, directory, songs.len()));
            }
            (None, ScanMode::IfNeeded) => self.ensure_snapshot(directory, false, progress).await?,
        };

        let tracks = Arc::clone(&snapshot.tracks);
        let matcher = self.matcher;
        let matches = {
            let songs = Arc::clone(&songs);
            let tracks = Arc::clone(&tracks);
            run_blocking(move || matcher.match_songs(&songs, &tracks)).await
        }
        .map_err(|_| internal_error("对比本地曲库"))?;

        let view = build_match_view(playlist_id, directory, &songs, &tracks, &matches);
        let hint = if tracks.is_empty() {
            "（本地曲库里没有找到音频文件）"
        } else {
            ""
        };
        self.logs.info(format!(
            "本地对比：已存在 {}，NCM 待解密 {}，需下载 {}{hint}",
            view.stats.available, view.stats.ncm, view.stats.missing
        ));
        Ok(view)
    }

    /// 返回与 `directory` 对应的扫描结果；`force` 为假且已有可用缓存时直接复用。
    async fn ensure_snapshot(
        &self,
        directory: &str,
        force: bool,
        progress: ScanProgressSink,
    ) -> Result<Snapshot, LibraryCommandError> {
        // 先排队：另一个扫描正在跑时，等它结束，之后多半可以直接复用它的结果。
        let _turn = self.scan_lock.lock().await;
        if !force
            && let Some(snapshot) = lock(&self.snapshot)
                .clone()
                .filter(|snapshot| same_directory(&snapshot.directory, directory))
        {
            return Ok(snapshot);
        }

        if directory.trim().is_empty() || !Path::new(directory).is_dir() {
            let message = format!("曲库目录不存在：{directory}");
            self.logs
                .warning(format!("{message}，请在「设置」中重新选择曲库目录"));
            return Err(LibraryCommandError::new(
                "directoryMissing",
                format!("{message}。请打开「设置」选择曲库目录。"),
            ));
        }

        let token = CancelToken::new();
        *lock(&self.running) = Some(token.clone());
        self.logs.info(format!("开始扫描本地曲库：{directory}"));

        let service = Arc::clone(&self.service);
        let root = PathBuf::from(directory);
        let sink = throttled(progress);
        let outcome = run_blocking(move || {
            let report = |progress: LibraryScanProgress| sink(&progress);
            service.scan(&[root], Some(&report), &token)
        })
        .await;
        *lock(&self.running) = None;

        let outcome = match outcome {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(LibraryError::Cancelled)) => {
                self.logs.warning("扫描本地曲库已取消");
                return Err(LibraryCommandError::new("cancelled", "扫描本地曲库已取消"));
            }
            Err(_) => {
                let error = internal_error("扫描本地曲库");
                self.logs
                    .error(format!("扫描本地曲库失败：{}", error.message));
                return Err(error);
            }
        };

        let snapshot = Snapshot {
            directory: directory.to_owned(),
            skipped_directories: outcome.skipped_directories,
            tracks: Arc::new(outcome.tracks),
        };
        let summary = snapshot.summary();
        self.logs.success(format!(
            "本地曲库扫描完成：{} 个文件（其中 NCM {} 个）",
            summary.total_files, summary.ncm_files
        ));
        if summary.skipped_directories > 0 {
            self.logs.warning(format!(
                "有 {} 个文件夹因权限不足或已被移动而被跳过",
                summary.skipped_directories
            ));
        }
        *lock(&self.snapshot) = Some(snapshot.clone());
        Ok(snapshot)
    }
}

fn internal_error(what: &str) -> LibraryCommandError {
    LibraryCommandError::new("failed", format!("{what}时程序内部出错，请重试。"))
}

/// 曲库目录是否相同（路径文本去掉末尾分隔符后不区分大小写比较）。
fn same_directory(a: &str, b: &str) -> bool {
    let normalize = |value: &str| value.trim().trim_end_matches(['\\', '/']).to_lowercase();
    normalize(a) == normalize(b)
}

/// 给进度回调加上节流：第一条、最后一条（全部处理完）必发，中间的至多每 80 毫秒一条。
fn throttled(sink: ScanProgressSink) -> impl Fn(&LibraryScanProgress) + Send + Sync {
    let last = Mutex::new(None::<Instant>);
    move |progress: &LibraryScanProgress| {
        let mut last = lock(&last);
        let due = match *last {
            None => true,
            Some(at) => at.elapsed() >= PROGRESS_INTERVAL,
        };
        let finished = progress.scanned_files >= progress.total_files;
        if due || finished {
            *last = Some(Instant::now());
            sink(ScanProgressView::from(progress));
        }
    }
}

/// 在阻塞线程池里跑同步任务。lp-core 不依赖 Tauri，直接用 tokio 的阻塞池。
async fn run_blocking<T: Send + 'static>(
    task: impl FnOnce() -> T + Send + 'static,
) -> Result<T, tokio::task::JoinError> {
    tokio::task::spawn_blocking(task).await
}
