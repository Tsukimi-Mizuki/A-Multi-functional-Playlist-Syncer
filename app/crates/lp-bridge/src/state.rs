//! 桥接层的组合根：启动时解析数据目录、读设置与登录态，装配 `lp-core` 的服务（对应 Tauri 外壳的 `AppState::load`）。
//!
//! 目前装配登录、歌单、本地曲库、下载 / 同步；其余服务（在线检索、NCM 导入、标签编辑……）
//! 按画面的进度逐个接上。

use std::future::Future;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use lp_core::auth::{LoginManager, LoginOptions};
use lp_core::download::fetch::MediaFetcher;
use lp_core::download::ncm_import::{NcmImportService, OnlineEnricher};
use lp_core::download::service::{DownloadApi, DownloadService, Downloader};
use lp_core::download::task::TransferManager;
use lp_core::library::{LibraryManager, LocalLibraryService};
use lp_core::logs::LogHub;
use lp_core::lyrics::LyricsService;
use lp_core::lyrics::batch::{BatchEnricher, BatchLyricsService, BatchManager, SettingsProvider};
use lp_core::ncm::FileNcmDecoder;
use lp_core::netease::NeteaseClient;
use lp_core::netease::http::{HttpTransport, ReqwestTransport};
use lp_core::online::{
    KugouLyricsSource, LrclibLyricsSource, LyricsSource, OnlineManager, OnlineMusicSearch,
    OnlineMusicSearchService, fallback_source_enabled,
};
use lp_core::paths::{AppPaths, PathEnv};
use lp_core::playlist::PlaylistService;
use lp_core::session::SessionStore;
use lp_core::settings::{AppSettings, JsonSettingsStore};
use lp_core::sync::SyncService;
use lp_core::tags::{AudioTagService, LoftyTagService};
use serde_json::Value;
use tokio::runtime::Runtime;

/// 进程内唯一的桥接状态。
pub(crate) struct Bridge {
    /// `lp-core` 的异步代码（reqwest、续期定时器、登录态自动落盘）都跑在这个运行时里：
    /// flutter_rust_bridge 自己的执行器不保证是 tokio，直接 `await` 会找不到 reactor。
    pub runtime: Runtime,
    pub paths: AppPaths,
    /// 当前设置。命令在桥接层自己的线程上读写，所以与 Tauri 外壳一样用读写锁而不是裸值。
    pub settings: Arc<RwLock<AppSettings>>,
    /// 设置文件（保存设置时写盘）。
    pub settings_store: JsonSettingsStore,
    /// 设置文件读取失败时的中文原因；成功保存一次设置后清空。
    pub settings_warning: Arc<RwLock<Option<String>>>,
    /// 登录文件读取 / 解密失败时的中文原因。
    pub session_error: Option<String>,
    pub logs: Arc<LogHub>,
    pub login: Arc<LoginManager>,
    pub playlists: Arc<PlaylistService>,
    pub library: Arc<LibraryManager>,
    /// 音频标签的读写（标签查看 / 编辑画面直接调用）。
    pub tags: Arc<dyn AudioTagService>,
    /// 在线检索与标签 / 封面详情（编辑器「在线标签」画面）。
    pub online: Arc<OnlineManager>,
    /// 歌单里的歌词预览（只读，不写任何文件）。
    pub lyrics: Arc<LyricsService>,
    /// 批量任务：「歌词转逐字」的两步走与批量在线获取共用一个任务槽。
    pub batch: Arc<BatchManager>,
    /// 下载 / 同步任务（同一时间只允许一个）。
    pub transfers: Arc<TransferManager>,
}

impl Bridge {
    /// 当前设置的一致快照。
    pub fn settings(&self) -> AppSettings {
        self.settings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// 仍然有效的设置读取警告（保存过新设置后就不再提示）。
    pub fn settings_warning(&self) -> Option<String> {
        self.settings_warning
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// 应用设置窗口提交的修改并写盘。
    ///
    /// 这是用户的明确操作，所以即使设置文件当初读取失败也照常保存；保存成功后读取失败的提示随之消失。
    pub fn save_settings_patch(&self, patch: &Value) -> Result<AppSettings, String> {
        let default_dir = self
            .paths
            .default_download_dir
            .to_string_lossy()
            .into_owned();
        let mut settings = self
            .settings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let next = settings.apply_patch(patch, &default_dir)?;
        match self.settings_store.save(&next) {
            Ok(saved) => {
                *settings = saved.clone();
                *self
                    .settings_warning
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
                self.logs.success("设置已保存");
                Ok(saved)
            }
            Err(error) => {
                let message = format!("保存设置失败：{error}");
                self.logs.warning(&message);
                Err(message)
            }
        }
    }
}

static BRIDGE: OnceLock<Result<Bridge, String>> = OnceLock::new();

/// 取桥接状态；第一次调用时装配。装配失败（数据目录不可写、网络组件初始化失败）时返回中文原因。
pub(crate) fn bridge() -> Result<&'static Bridge, String> {
    BRIDGE
        .get_or_init(Bridge::load)
        .as_ref()
        .map_err(Clone::clone)
}

/// 在桥接层的 tokio 运行时里跑一个 future，并等它结束。
pub(crate) async fn run<F>(future: F) -> Result<F::Output, String>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let bridge = bridge()?;
    bridge
        .runtime
        .spawn(future)
        .await
        .map_err(|error| format!("后台任务异常结束：{error}"))
}

impl Bridge {
    fn load() -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("lp-core")
            .build()
            .map_err(|error| format!("初始化异步运行时失败：{error}"))?;
        // 登录编排在构造时会登记自动落盘任务，必须在运行时的上下文里装配。
        let assembled = {
            let _guard = runtime.enter();
            Assembled::load()?
        };
        Ok(Self {
            runtime,
            paths: assembled.paths,
            settings: assembled.settings,
            settings_store: assembled.settings_store,
            settings_warning: assembled.settings_warning,
            session_error: assembled.session_error,
            logs: assembled.logs,
            login: assembled.login,
            playlists: assembled.playlists,
            library: assembled.library,
            tags: assembled.tags,
            online: assembled.online,
            lyrics: assembled.lyrics,
            batch: assembled.batch,
            transfers: assembled.transfers,
        })
    }
}

/// 运行时以外的那部分状态（先装配好，再和运行时一起放进 [`Bridge`]）。
struct Assembled {
    paths: AppPaths,
    settings: Arc<RwLock<AppSettings>>,
    settings_store: JsonSettingsStore,
    settings_warning: Arc<RwLock<Option<String>>>,
    session_error: Option<String>,
    logs: Arc<LogHub>,
    login: Arc<LoginManager>,
    playlists: Arc<PlaylistService>,
    library: Arc<LibraryManager>,
    tags: Arc<dyn AudioTagService>,
    online: Arc<OnlineManager>,
    lyrics: Arc<LyricsService>,
    batch: Arc<BatchManager>,
    transfers: Arc<TransferManager>,
}

impl Assembled {
    fn load() -> Result<Self, String> {
        let paths = AppPaths::resolve(&PathEnv::from_system()).map_err(|error| {
            format!("无法准备数据目录：设置与登录态都没有可写的位置（{error}）")
        })?;
        let default_download_dir = paths.default_download_dir.to_string_lossy().into_owned();
        let settings_store = JsonSettingsStore::new(paths.settings_file(), default_download_dir);
        let (settings, settings_warning) = match settings_store.load() {
            Ok(settings) => (settings, None),
            Err(error) => (settings_store.defaults(), Some(error.to_string())),
        };

        let logs = LogHub::new();
        logs.info(format!(
            "数据目录：{}（{}）",
            paths.data_dir.display(),
            if paths.is_portable {
                "便携模式"
            } else {
                "非便携模式"
            }
        ));
        if let Some(warning) = &settings_warning {
            logs.warning(format!("读取设置失败，已使用默认设置：{warning}"));
        }

        let settings = Arc::new(RwLock::new(settings));
        let transport: Arc<dyn HttpTransport> = Arc::new(
            ReqwestTransport::new(lp_core::netease::client::REQUEST_TIMEOUT)
                .map_err(|error| format!("初始化网络组件失败：{error:?}"))?,
        );
        let client = Arc::new(NeteaseClient::new(Arc::clone(&transport)));
        let session_store = Arc::new(SessionStore::with_system_protector(paths.session_file()));
        let login = LoginManager::new(
            client,
            session_store,
            Arc::clone(&logs),
            LoginOptions::default(),
        );
        let playlists = Arc::new(PlaylistService::new(Arc::clone(&login), Arc::clone(&logs)));
        let restored = login.restore();
        if restored.load_failure.is_some() {
            logs.warning(format!(
                "登录文件位于数据目录：{}",
                paths.data_dir.display()
            ));
        }

        // 本地曲库扫描要读音频标签；NCM 只登记为「待解密」，不解密。
        let tags: Arc<dyn AudioTagService> = Arc::new(LoftyTagService);
        let library = LibraryManager::new(
            Arc::new(LocalLibraryService::new(
                Arc::new(FileNcmDecoder),
                Arc::clone(&tags),
            )),
            Arc::clone(&logs),
        );

        // 下载 / 同步的装配顺序与 Tauri 外壳一致：在线补全依赖批量歌词服务，
        // 同步又依赖 NCM 解密服务，所以先造前者再造后者。
        // 备用词源开关、阈值与歌词写入方式都在运行期读设置，保存设置后立即生效。
        let settings_provider: SettingsProvider = {
            let settings = Arc::clone(&settings);
            Arc::new(move || {
                settings
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
            })
        };
        let online_search: Arc<dyn OnlineMusicSearch> = Arc::new(OnlineMusicSearchService::new(
            login.client().clone(),
            Arc::clone(&transport),
        ));
        let fallback_sources: Vec<Arc<dyn LyricsSource>> = vec![
            Arc::new(KugouLyricsSource::new(Arc::clone(&transport))),
            Arc::new(LrclibLyricsSource::new(Arc::clone(&transport))),
        ];
        // 在线标签 / 歌词的编排器：搜索句柄与备用词源都要留一份自己的，批量补全用的是另一份。
        let online = Arc::new(OnlineManager::new(
            Arc::clone(&online_search),
            fallback_sources.clone(),
            Arc::clone(&settings_provider),
        ));
        let batch_service = Arc::new(BatchLyricsService::new(
            Arc::clone(&tags),
            online_search,
            fallback_sources,
        ));
        batch_service.set_fallback_filter(Some({
            let settings_provider = Arc::clone(&settings_provider);
            Arc::new(move |source: &dyn LyricsSource| {
                fallback_source_enabled(&settings_provider(), source.label())
            })
        }));
        // 歌词预览（按来源取词）与「歌词转逐字」（联网核对 + 写入）都用这份批量检索服务；
        // 批量任务管理器还要持有它自己的一份，最后剩下的那份交给下载 / 同步的在线补全。
        let lyrics = Arc::new(
            LyricsService::new(Arc::clone(&login), Arc::clone(&logs))
                .with_online(Arc::clone(&batch_service)),
        );
        let batch = Arc::new(BatchManager::new(
            Arc::clone(&batch_service),
            Arc::clone(&logs),
        ));
        let enricher: Arc<dyn OnlineEnricher> =
            Arc::new(BatchEnricher::new(batch_service, settings_provider));
        let ncm = Arc::new(NcmImportService::new(
            Arc::new(FileNcmDecoder),
            Arc::clone(&tags),
            Some(Arc::clone(&enricher)),
        ));

        // 音频下载用独立的传输层：只限制连接与「相邻两次读到数据的间隔」，不套 20 秒的接口总超时。
        let download_transport = ReqwestTransport::for_downloads(
            Duration::from_secs(15),
            Duration::from_secs(60),
            Duration::from_secs(30 * 60),
        )
        .map_err(|error| format!("初始化下载组件失败：{error:?}"))?;
        let fetcher = MediaFetcher::new(Arc::new(download_transport));
        let api: Arc<dyn DownloadApi> = login.client().clone();
        let downloader = Arc::new(DownloadService::new(
            Arc::clone(&api),
            Arc::clone(&tags),
            Arc::new(FileNcmDecoder),
            fetcher.clone(),
            Some(Arc::clone(&enricher)),
        ));
        let sync = Arc::new(SyncService::new(
            api,
            Arc::new(FileNcmDecoder),
            Arc::clone(&tags),
            ncm,
            Arc::clone(&downloader) as Arc<dyn Downloader>,
            fetcher,
            Some(enricher),
        ));
        let transfers = Arc::new(TransferManager::new(
            downloader,
            sync,
            Arc::clone(&library),
            Arc::clone(&logs),
        ));

        Ok(Self {
            paths,
            settings,
            settings_store,
            settings_warning: Arc::new(RwLock::new(settings_warning)),
            session_error: restored.load_failure,
            logs,
            login,
            playlists,
            library,
            tags,
            online,
            lyrics,
            batch,
            transfers,
        })
    }
}
