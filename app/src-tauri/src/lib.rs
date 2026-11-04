//! Tauri 外壳：只做命令转发、事件推送、窗口与插件，业务逻辑一律在 `lp-core`。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard};
use std::time::Duration;

use lp_core::auth::{
    AccountSummary, LoginManager, LoginOptions, QrPollOutcome, RuntimeAuthOutcome,
    ValidationOutcome,
};
use lp_core::download::fetch::MediaFetcher;
use lp_core::download::ncm_import::{
    NcmDialogState, NcmImportOptions, NcmImportProgress, NcmImportService, NcmJobSlot,
    NcmProgressView, NcmScanResult, NcmSummaryView, OnlineEnricher, prepare_output_directory,
    remember_state, scan_ncm_files,
};
use lp_core::download::service::{
    DownloadApi, DownloadProgress, DownloadService, Downloader, ProgressSink,
};
use lp_core::download::task::{
    TransferChannels, TransferError, TransferManager, TransferStarted, TransferSummaryView,
};
use lp_core::editor::{
    CoverFile, DropKind, DropResolution, EditorStateView, is_elevated, parse_startup_args,
    read_cover_file as read_cover_image, remember_editor_options, resolve_drop as resolve_targets,
};
use lp_core::library::{
    LibraryCommandError, LibraryManager, LibrarySummaryView, LocalLibraryService, MatchOutcomeView,
    ScanMode, ScanProgressSink, ScanProgressView,
};
use lp_core::logs::{LogEntry, LogHub};
use lp_core::lyrics::batch::{
    BatchEnricher, BatchJobError, BatchLyricsService, BatchManager, BatchProgressView,
    BatchRequest, BatchScanView, BatchSummaryView, SettingsProvider,
};
use lp_core::lyrics::{LyricsError, LyricsMode, LyricsPreview, LyricsService};
use lp_core::models::{AudioQuality, LyricsSourceKind, ThemeMode, WindowMaterial};
use lp_core::ncm::FileNcmDecoder;
use lp_core::netease::http::{HttpTransport, ReqwestTransport};
use lp_core::netease::{NeteaseClient, NeteaseError, PlaylistDetail, PlaylistSummary};
use lp_core::online::view::{
    FallbackLyricsView, LoadedResultView, MatchThresholdView, OnlineCommandError,
    OnlineSearchResponse,
};
use lp_core::online::{
    FallbackRequest, FileContext, KugouLyricsSource, LoadRequest, LrclibLyricsSource, LyricsSource,
    OnlineManager, OnlineMusicSearch, OnlineMusicSearchService, OnlineSongResult, SearchRequest,
    fallback_source_enabled,
};
use lp_core::paths::{AppPaths, PathEnv};
use lp_core::playlist::{PlaylistError, PlaylistService};
use lp_core::session::SessionStore;
use lp_core::settings::{AppSettings, JsonSettingsStore};
use lp_core::sync::SyncService;
use lp_core::tags::{AudioTagService, LoftyTagService, TagCommandError, TagView, WriteTagsRequest};
use lp_core::utilities::CancelToken;
use serde::{Deserialize, Serialize};
use tauri::utils::config::WindowEffectsConfig;
use tauri::window::{Effect, EffectsBuilder};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewWindow, WebviewWindowBuilder};

/// 前端订阅的事件名：新的运行日志。
const EVENT_LOG: &str = "app:log";
/// 前端订阅的事件名：账号状态变化（登录成功、退出、登录失效、资料刷新）。
const EVENT_ACCOUNT: &str = "auth:state";
/// 前端订阅的事件名：大歌单分批补齐曲目详情的进度。
const EVENT_PLAYLIST_PROGRESS: &str = "playlist:progress";
/// 前端订阅的事件名：NCM 导入的逐文件进度。
const EVENT_NCM_PROGRESS: &str = "ncm:progress";
/// 前端订阅的事件名：本地曲库扫描进度（已处理 / 总数 / 当前文件名）。
const EVENT_LIBRARY_PROGRESS: &str = "library:progress";
/// 前端订阅的事件名：下载 / 同步任务开始（会处理哪些歌曲）。
const EVENT_DOWNLOAD_STARTED: &str = "download:started";
/// 前端订阅的事件名：下载 / 同步的逐曲进度（阶段、字节、实际音质、最终结果）。
const EVENT_DOWNLOAD_PROGRESS: &str = "download:progress";
/// 前端订阅的事件名：批量获取标签 / 封面 / 歌词的逐文件进度。
const EVENT_BATCH_PROGRESS: &str = "batch:progress";

/// 返回给前端的应用信息（camelCase）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfoDto {
    name: String,
    version: String,
}

impl From<lp_core::AppInfo> for AppInfoDto {
    fn from(info: lp_core::AppInfo) -> Self {
        Self {
            name: info.name.to_owned(),
            version: info.version.to_owned(),
        }
    }
}

/// 启动时加载好的全局状态，由 Tauri 托管，命令通过 `State<AppState>` 取用。
struct AppState {
    paths: AppPaths,
    /// 当前设置。在线补全与备用词源开关通过它在运行期读取，所以改动立即生效。
    settings: Arc<RwLock<AppSettings>>,
    /// `settings.json` 的读写；只在记住 NCM 导入位置等需要落盘的地方使用。
    settings_store: JsonSettingsStore,
    /// 设置文件读取失败时的中文原因（此时界面使用默认设置）。
    settings_warning: Option<String>,
    /// 用户在设置窗口里主动保存过：读取失败的旧文件已被新设置取代，之后的自动记忆也可以照常写盘。
    settings_recovered: AtomicBool,
    /// 运行日志（内存，最多 400 条）。
    logs: Arc<LogHub>,
    /// 登录编排；登录态（含 Cookie）只留在后端，前端永远拿不到。
    login: Arc<LoginManager>,
    /// 歌单加载（列表与曲目），301 时经 `login` 复核登录。
    playlists: Arc<PlaylistService>,
    /// 歌词预览（取歌词 -> 解析 -> 序列化），301 时经 `login` 复核。
    lyrics: Arc<LyricsService>,
    /// 音频标签读写（lofty）。
    tags: Arc<dyn AudioTagService>,
    /// NCM 批量解密导入（解密后可在线补全，见 `online`）。
    ncm: Arc<NcmImportService>,
    /// 当前正在运行的 NCM 导入任务（至多一个），`ncm_cancel` 通过它取消。
    ncm_job: NcmJobSlot,
    /// 本地曲库扫描结果缓存与歌单对比（扫描一次只跑一个，可取消）。
    library: Arc<LibraryManager>,
    /// 下载 / 同步任务（同一时间只有一个，可取消），结束后重新扫描曲库。
    transfers: Arc<TransferManager>,
    /// 在线搜索 / 取详情 / 备用词源（歌词与标签编辑器用）。
    online: Arc<OnlineManager>,
    /// 批量获取标签、封面与歌词（同一时间一个任务，可取消）。
    batch: Arc<BatchManager>,
    /// 登录文件读取 / 解密失败时的中文原因。
    session_error: Option<String>,
    /// 命令行 `--lyrics/--edit <文件>` 指定的、启动后直接打开编辑器的文件（前端取走一次）。
    startup_file: Mutex<Option<String>>,
}

impl AppState {
    /// 解析数据目录并加载设置与登录态。读取失败不会中断启动，而是记录原因交给前端展示。
    fn load(paths: AppPaths) -> Self {
        let default_download_dir = paths.default_download_dir.to_string_lossy().into_owned();
        let settings_store = JsonSettingsStore::new(paths.settings_file(), default_download_dir);
        let (settings, settings_warning) = match settings_store.load() {
            Ok(settings) => (settings, None),
            Err(error) => (settings_store.defaults(), Some(error.to_string())),
        };

        let logs = LogHub::new();
        logs.info(format!(
            "数据目录：{}（{}）",
            display_path(&paths.data_dir),
            if paths.is_portable {
                "便携模式"
            } else {
                "非便携模式"
            }
        ));
        logs.info(format!("下载目录：{}", settings.download_directory));
        if let Some(warning) = &settings_warning {
            logs.warning(format!("读取设置失败，已使用默认设置：{warning}"));
        }

        let settings = Arc::new(RwLock::new(settings));
        let transport: Arc<dyn HttpTransport> = Arc::new(
            ReqwestTransport::new(lp_core::netease::client::REQUEST_TIMEOUT)
                .expect("初始化网络组件失败"),
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
        let lyrics = Arc::new(LyricsService::new(Arc::clone(&login), Arc::clone(&logs)));
        let restored = login.restore();
        if restored.load_failure.is_some() {
            logs.warning(format!(
                "登录文件位于数据目录：{}",
                display_path(&paths.data_dir)
            ));
        }

        let tags: Arc<dyn AudioTagService> = Arc::new(LoftyTagService);

        // 在线匹配：网易云 / QQ 音乐为主，酷狗与 LRCLIB 作为备用词源。
        // 备用词源开关与“自动补全”的阈值、歌词写入方式都在运行期读设置，所以保存设置后立即生效。
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
        let batch_service = Arc::new(BatchLyricsService::new(
            Arc::clone(&tags),
            Arc::clone(&online_search),
            fallback_sources.clone(),
        ));
        batch_service.set_fallback_filter(Some({
            let settings_provider = Arc::clone(&settings_provider);
            Arc::new(move |source: &dyn LyricsSource| {
                fallback_source_enabled(&settings_provider(), source.label())
            })
        }));
        let online = Arc::new(OnlineManager::new(
            online_search,
            fallback_sources,
            Arc::clone(&settings_provider),
        ));
        let batch = Arc::new(BatchManager::new(
            Arc::clone(&batch_service),
            Arc::clone(&logs),
        ));
        // 下载、同步与 NCM 解密成功后的在线补全：QQ 逐字歌词优先、网易云兜底。
        let enricher: Arc<dyn OnlineEnricher> =
            Arc::new(BatchEnricher::new(batch_service, settings_provider));

        let ncm = Arc::new(NcmImportService::new(
            Arc::new(FileNcmDecoder),
            Arc::clone(&tags),
            Some(Arc::clone(&enricher)),
        ));

        let library = LibraryManager::new(
            Arc::new(LocalLibraryService::new(
                Arc::new(FileNcmDecoder),
                Arc::clone(&tags),
            )),
            Arc::clone(&logs),
        );

        // 音频下载用独立的传输层：只限制连接与“相邻两次读到数据的间隔”，不套 20 秒的接口总超时。
        let download_transport = ReqwestTransport::for_downloads(
            Duration::from_secs(15),
            Duration::from_secs(60),
            Duration::from_secs(30 * 60),
        )
        .expect("初始化下载组件失败");
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
            Arc::clone(&ncm),
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

        Self {
            paths,
            settings,
            settings_store,
            settings_warning,
            settings_recovered: AtomicBool::new(false),
            logs,
            login,
            playlists,
            lyrics,
            tags,
            ncm,
            ncm_job: NcmJobSlot::default(),
            library,
            transfers,
            online,
            batch,
            session_error: restored.load_failure,
            startup_file: Mutex::new(None),
        }
    }

    /// 解析命令行里的 `--lyrics/--edit <文件>`：文件存在就记下来等前端取走；给了但不存在只记一条警告。
    fn apply_startup_args<I, S>(&self, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let request = parse_startup_args(args);
        if let Some(missing) = request.missing {
            self.logs
                .warning(format!("启动参数指定的文件不存在，已忽略：{missing}"));
        }
        if let Some(file) = request.file {
            self.logs
                .info(format!("按启动参数打开编辑器：{}", display_path(&file)));
            *self
                .startup_file
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                Some(file.to_string_lossy().into_owned());
        }
    }

    /// 把编辑器的在线搜索来源与歌词格式写回 `settings.json`（只改这两个字段，其余设置原样保留）。
    ///
    /// 记忆只是便利功能：没有变化不写盘；设置文件当初没能读出来（正在使用默认设置）时只改内存不写盘，
    /// 免得把用户真正的设置覆盖成默认值；写盘失败只记日志。
    fn remember_editor_options(&self, source: LyricsSourceKind, prefer_word_lyrics: bool) {
        let mut settings = self
            .settings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !remember_editor_options(&mut settings, source, prefer_word_lyrics) {
            return;
        }
        if self.settings_unreadable() {
            return;
        }
        match self.settings_store.save(&settings) {
            Ok(saved) => *settings = saved,
            Err(error) => self
                .logs
                .warning(format!("记住编辑器的搜索来源失败：{error}")),
        }
    }

    /// 设置文件当初没能读出来、用户也还没在设置窗口里保存过：此时自动记忆只改内存不写盘。
    fn settings_unreadable(&self) -> bool {
        self.settings_warning.is_some() && !self.settings_recovered.load(Ordering::Relaxed)
    }

    /// 仍然有效的设置读取警告（保存过新设置后就不再提示）。
    fn active_settings_warning(&self) -> Option<&str> {
        self.settings_warning
            .as_deref()
            .filter(|_| self.settings_unreadable())
    }

    /// 应用设置窗口提交的修改并写盘。这是用户的明确操作，所以即使设置文件当初读取失败也照常保存，
    /// 保存成功后读取失败的提示随之消失。
    fn save_settings_patch(&self, patch: &serde_json::Value) -> Result<AppSettings, String> {
        let default_dir = display_path(&self.paths.default_download_dir);
        let mut settings = self
            .settings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let next = settings.apply_patch(patch, &default_dir)?;
        match self.settings_store.save(&next) {
            Ok(saved) => {
                *settings = saved.clone();
                self.settings_recovered.store(true, Ordering::Relaxed);
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

    fn settings(&self) -> RwLockReadGuard<'_, AppSettings> {
        self.settings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn to_dto(&self) -> AppStateDto {
        build_app_state_dto(
            &self.paths,
            &self.settings(),
            self.active_settings_warning(),
            &self.login.account(),
            self.session_error.as_deref(),
        )
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// 设置摘要：前端占位页只需要这几项。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsSummaryDto {
    download_directory: String,
    library_directory: String,
    quality: AudioQuality,
    theme: ThemeMode,
    max_concurrency: i64,
    match_threshold: f64,
    /// 加载歌单后是否自动对比本地曲库。
    auto_match_local: bool,
}

/// 登录态摘要：账号信息 + 读取失败原因，**不含 Cookie**。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSummaryDto {
    logged_in: bool,
    nickname: Option<String>,
    load_error: Option<String>,
    /// 完整账号摘要（头像、会员类型等）。
    account: AccountSummary,
}

/// `get_app_state` 的返回值。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStateDto {
    data_dir: String,
    is_portable: bool,
    default_download_dir: String,
    settings: SettingsSummaryDto,
    settings_warning: Option<String>,
    session: SessionSummaryDto,
    /// 可选的下载音质（与 Rust 端 `AudioQuality` 一一对应，界面下拉直接用）。
    quality_options: Vec<QualityOptionDto>,
}

/// 一档下载音质：`value` 是写进 `settings.json` 的名字，`label` 是界面显示名。
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct QualityOptionDto {
    value: AudioQuality,
    label: &'static str,
}

fn quality_options() -> Vec<QualityOptionDto> {
    AudioQuality::ALL
        .iter()
        .map(|quality| QualityOptionDto {
            value: *quality,
            label: quality.display_name(),
        })
        .collect()
}

fn build_app_state_dto(
    paths: &AppPaths,
    settings: &AppSettings,
    settings_warning: Option<&str>,
    account: &AccountSummary,
    session_error: Option<&str>,
) -> AppStateDto {
    AppStateDto {
        data_dir: display_path(&paths.data_dir),
        is_portable: paths.is_portable,
        default_download_dir: display_path(&paths.default_download_dir),
        settings: SettingsSummaryDto {
            download_directory: settings.download_directory.clone(),
            library_directory: settings.library_directory.clone(),
            quality: settings.quality,
            theme: settings.theme,
            max_concurrency: settings.max_concurrency,
            match_threshold: settings.match_threshold,
            auto_match_local: settings.auto_match_local,
        },
        settings_warning: settings_warning.map(str::to_owned),
        session: SessionSummaryDto {
            logged_in: account.logged_in,
            nickname: account.nickname.clone(),
            load_error: session_error.map(str::to_owned),
            account: account.clone(),
        },
        quality_options: quality_options(),
    }
}

/// `validate_login` 的返回值。
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ValidationDto {
    /// `notLoggedIn` / `valid` / `unverified` / `loggedOut`。
    status: &'static str,
    /// `unverified` 时的中文说明。
    message: Option<String>,
    account: AccountSummary,
}

impl ValidationDto {
    fn new(outcome: ValidationOutcome, account: AccountSummary) -> Self {
        let (status, message) = match outcome {
            ValidationOutcome::NotLoggedIn => ("notLoggedIn", None),
            ValidationOutcome::Valid => ("valid", None),
            ValidationOutcome::Unverified { message } => ("unverified", Some(message)),
            ValidationOutcome::LoggedOut => ("loggedOut", None),
        };
        Self {
            status,
            message,
            account,
        }
    }
}

/// `qr_start` 的返回值。
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct QrStartDto {
    key: String,
    /// 白底黑码的 SVG 文本。
    svg: String,
    lifetime_secs: u64,
}

/// `qr_poll` 的返回值。
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct QrPollDto {
    /// `waiting` / `scanned` / `expired` / `failed` / `success` / `error`。
    state: &'static str,
    message: Option<String>,
    /// `error` 时，是否是暂时性的网络问题（前端可继续重试几次）。
    transient: bool,
    account: Option<AccountSummary>,
}

impl QrPollDto {
    fn plain(state: &'static str) -> Self {
        Self {
            state,
            message: None,
            transient: false,
            account: None,
        }
    }

    fn from_result(result: Result<QrPollOutcome, NeteaseError>) -> Self {
        match result {
            Ok(QrPollOutcome::Waiting) => Self::plain("waiting"),
            Ok(QrPollOutcome::Scanned) => Self::plain("scanned"),
            Ok(QrPollOutcome::Expired) => Self::plain("expired"),
            Ok(QrPollOutcome::Failed { message }) => Self {
                message: Some(message),
                ..Self::plain("failed")
            },
            Ok(QrPollOutcome::Success { account }) => Self {
                account: Some(account),
                ..Self::plain("success")
            },
            Err(error) => Self {
                message: Some(error.user_message()),
                transient: error.is_transient_network(),
                ..Self::plain("error")
            },
        }
    }
}

/// 读取应用名称与版本，用来验证前后端通路。
#[tauri::command]
fn app_info() -> AppInfoDto {
    lp_core::app_info().into()
}

/// 读取数据目录、便携模式、设置摘要与登录态摘要。
#[tauri::command]
fn get_app_state(state: State<'_, AppState>) -> AppStateDto {
    state.to_dto()
}

/// 当前保留的运行日志（前端启动时用它补齐监听建立之前的日志）。
#[tauri::command]
fn get_logs(state: State<'_, AppState>) -> Vec<LogEntry> {
    state.logs.recent()
}

/// 当前账号摘要。
#[tauri::command]
fn login_status(state: State<'_, AppState>) -> AccountSummary {
    state.login.account()
}

/// 启动后的联网校验：读取账号并续期；网络类错误保留登录，明确 301 才清除。
#[tauri::command]
async fn validate_login(state: State<'_, AppState>) -> Result<ValidationDto, String> {
    let login = Arc::clone(&state.login);
    let outcome = login.validate().await;
    Ok(ValidationDto::new(outcome, login.account()))
}

/// 申请登录二维码。
#[tauri::command]
async fn qr_start(state: State<'_, AppState>) -> Result<QrStartDto, String> {
    let login = Arc::clone(&state.login);
    match login.qr_start().await {
        Ok(start) => Ok(QrStartDto {
            key: start.key,
            svg: start.svg,
            lifetime_secs: start.lifetime_secs,
        }),
        Err(error) => {
            let message = error.user_message();
            state.logs.warning(format!("获取登录二维码失败：{message}"));
            Err(message)
        }
    }
}

/// 轮询一次扫码状态；错误折叠进返回值，前端按 `transient` 决定是否继续轮询。
#[tauri::command]
async fn qr_poll(state: State<'_, AppState>, key: String) -> Result<QrPollDto, String> {
    let login = Arc::clone(&state.login);
    Ok(QrPollDto::from_result(login.qr_poll(&key).await))
}

/// 退出登录：清内存会话 → 排空自动落盘 → 删除 `session.dat`。
#[tauri::command]
async fn logout(state: State<'_, AppState>) -> Result<(), String> {
    let login = Arc::clone(&state.login);
    login.logout().await;
    Ok(())
}

/// 后续模块在运行期收到接口 301 时调用：先续期并复核，仍未登录才清除登录态。
#[tauri::command]
async fn recheck_login(state: State<'_, AppState>) -> Result<&'static str, String> {
    let login = Arc::clone(&state.login);
    Ok(match login.handle_runtime_auth_error().await {
        RuntimeAuthOutcome::Skipped => "skipped",
        RuntimeAuthOutcome::Kept => "kept",
        RuntimeAuthOutcome::LoggedOut => "loggedOut",
    })
}

/// `playlist:progress` 事件的载荷。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct PlaylistProgressDto {
    playlist_id: i64,
    /// 已取得详情的歌曲数。
    loaded: usize,
    /// 歌单总曲目数。
    total: usize,
}

/// 当前账号的全部歌单。失败时 `Err` 是 `{ kind, message }`，`message` 已是中文说明。
#[tauri::command]
async fn list_playlists(state: State<'_, AppState>) -> Result<Vec<PlaylistSummary>, PlaylistError> {
    let playlists = Arc::clone(&state.playlists);
    playlists.list().await
}

/// 一个歌单的完整曲目；超过 500 首时分批补齐，期间发 `playlist:progress` 事件。
#[tauri::command]
async fn get_playlist_tracks(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<PlaylistDetail, PlaylistError> {
    let playlists = Arc::clone(&state.playlists);
    let library = Arc::clone(&state.library);
    let report = move |loaded: usize, total: usize| {
        let _ = app.emit(
            EVENT_PLAYLIST_PROGRESS,
            PlaylistProgressDto {
                playlist_id: id,
                loaded,
                total,
            },
        );
    };
    let detail = playlists.tracks(id, &report).await?;
    // 记住曲目，之后 `match_playlist` 按歌单 ID 对比，不必把整份歌单再传一遍。
    library.remember_playlist(detail.id, &detail.songs);
    Ok(detail)
}

/// 一首歌的歌词预览：`mode` 为 `line`（逐行）或 `word`（逐字优先，没有逐字数据时回退逐行），默认带翻译。
/// 只读，不写入任何文件；失败时 `Err` 是 `{ kind, message }`，`message` 已是中文说明。
#[tauri::command]
async fn get_song_lyrics(
    state: State<'_, AppState>,
    id: i64,
    mode: LyricsMode,
) -> Result<LyricsPreview, LyricsError> {
    let lyrics = Arc::clone(&state.lyrics);
    lyrics.get(id, mode).await
}

/// 在后台线程里跑同步的标签读写，任务本身 panic 时转成普通失败。
async fn run_tag_task<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, lp_core::tags::TagError> + Send + 'static,
) -> Result<T, TagCommandError> {
    match tauri::async_runtime::spawn_blocking(task).await {
        Ok(result) => result.map_err(TagCommandError::from),
        Err(_) => Err(TagCommandError {
            kind: "failed",
            message: "处理音频文件时程序内部出错，请重试。".to_owned(),
        }),
    }
}

/// 读取一个本地音频文件的标签、封面（data URL，过大时不内联）、歌词与文件信息。
/// 文件不存在、格式不支持或已损坏时返回 `None`；权限不足等真正的 IO 故障返回 `Err`。只读，不改文件。
#[tauri::command]
async fn read_tags(
    state: State<'_, AppState>,
    path: String,
) -> Result<Option<TagView>, TagCommandError> {
    let tags = Arc::clone(&state.tags);
    let target = PathBuf::from(path);
    run_tag_task(move || Ok(tags.read(&target)?.map(|info| TagView::new(&target, info)))).await
}

/// 写入标签（先写同目录临时副本再原子替换，失败保留原文件）。本模块前端只读，编辑器在模块 10 接入。
#[tauri::command]
async fn write_tags(
    state: State<'_, AppState>,
    path: String,
    request: WriteTagsRequest,
) -> Result<(), TagCommandError> {
    let tags = Arc::clone(&state.tags);
    let data = request.into_data().map_err(TagCommandError::from)?;
    let target = PathBuf::from(path);
    run_tag_task(move || tags.write(&target, &data)).await
}

/// `ncm_import` 的参数（camelCase）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NcmImportRequest {
    /// 要导入的 `.ncm` 文件（前端从扫描结果里取）。
    paths: Vec<String>,
    /// 当前的输入文件夹列表，开始前记住。
    folders: Vec<String>,
    /// 输出目录（必须是完整路径，不存在会创建）。
    output_directory: String,
    /// 目标文件已存在时是否覆盖。
    overwrite: bool,
    /// 解密后是否自动在线匹配。
    auto_match_online: bool,
}

impl AppState {
    /// 把 NCM 导入窗口的输入文件夹与输出目录写回 `settings.json`（只改这两个字段，其余设置原样保留）。
    ///
    /// 记忆只是便利功能：失败只记日志，不打断导入。设置文件当初没能读出来（正在使用默认设置）时
    /// 不写盘，免得把用户真正的设置覆盖成默认值。
    fn remember_ncm_state(&self, folders: &[String], output_directory: &str) {
        if self.settings_unreadable() {
            return;
        }
        let mut settings = self
            .settings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        remember_state(&mut settings, folders, output_directory);
        match self.settings_store.save(&settings) {
            Ok(saved) => *settings = saved,
            Err(error) => self.logs.warning(format!("记住 NCM 导入位置失败：{error}")),
        }
    }
}

/// 打开 NCM 导入窗口时需要恢复的内容：记住的输入文件夹、输出目录（为空回退下载目录）、在线匹配是否可用。
#[tauri::command]
fn ncm_get_state(state: State<'_, AppState>) -> NcmDialogState {
    NcmDialogState::new(&state.settings(), state.ncm.online_match_available())
}

/// 递归查找这些文件夹里的 `.ncm` 文件（不存在的文件夹会列在 `missingFolders` 里）。
#[tauri::command]
async fn ncm_scan(folders: Vec<String>) -> Result<NcmScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || scan_ncm_files(&folders))
        .await
        .map_err(|_| "扫描 .ncm 文件时程序内部出错，请重试。".to_owned())
}

/// 记住 NCM 导入窗口的输入文件夹与输出目录（添加文件夹、关闭窗口时调用）。
#[tauri::command]
fn ncm_save_state(state: State<'_, AppState>, folders: Vec<String>, output_directory: String) {
    state.remember_ncm_state(&folders, &output_directory);
}

/// 批量解密并导入 `.ncm`：逐文件发 `ncm:progress` 事件，结束后返回汇总。
/// 被取消时返回已完成的部分（`cancelled: true`）；同一时间只允许一个任务。
#[tauri::command]
async fn ncm_import(
    app: AppHandle,
    state: State<'_, AppState>,
    request: NcmImportRequest,
) -> Result<NcmSummaryView, String> {
    let output = prepare_output_directory(&request.output_directory)?;
    if request.paths.is_empty() {
        return Err("没有可导入的 .ncm 文件，请先添加包含 .ncm 的文件夹。".to_owned());
    }
    let job = state.ncm_job.try_begin().map_err(|e| e.to_string())?;

    // 记住本次实际使用的位置（与 C# 一致：开始导入前保存）。
    state.remember_ncm_state(&request.folders, &request.output_directory);
    state.logs.info(format!(
        "开始导入 {} 个 NCM 文件，输出到 {}",
        request.paths.len(),
        display_path(&output)
    ));

    let options = NcmImportOptions {
        overwrite: request.overwrite,
        auto_match_online: request.auto_match_online,
    };
    let paths: Vec<PathBuf> = request.paths.iter().map(PathBuf::from).collect();
    let emit = move |progress: NcmImportProgress| {
        let _ = app.emit(EVENT_NCM_PROGRESS, NcmProgressView::from(&progress));
    };
    let service = Arc::clone(&state.ncm);
    let outcome = service
        .import(&paths, &output, options, &emit, &job.cancel)
        .await
        .map_err(|error| {
            let message = error.to_string();
            state.logs.warning(format!("NCM 导入失败：{message}"));
            message
        })?;

    let summary = NcmSummaryView::new(&outcome, &display_path(&output), request.auto_match_online);
    if summary.cancelled {
        state.logs.warning(format!(
            "NCM 导入已取消：已处理 {}/{}，成功 {}，失败 {}",
            outcome.results.len(),
            summary.total,
            summary.succeeded,
            summary.failed
        ));
    } else if summary.failed == 0 {
        state.logs.success(format!(
            "NCM 导入完成：成功 {} 个（共 {} 个）",
            summary.succeeded, summary.total
        ));
    } else {
        state.logs.warning(format!(
            "NCM 导入完成：成功 {} 个，失败 {} 个（共 {} 个）",
            summary.succeeded, summary.failed, summary.total
        ));
    }
    Ok(summary)
}

/// 取消正在进行的 NCM 导入；没有任务时返回 `false`。
#[tauri::command]
fn ncm_cancel(state: State<'_, AppState>) -> bool {
    state.ncm_job.cancel()
}

/// `match_playlist` 的参数（camelCase）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatchPlaylistRequest {
    /// 要对比的歌单（必须已经通过 `get_playlist_tracks` 加载过）。
    playlist_id: i64,
    /// 还没有扫描结果时：`never`（不对比）/ `ifNeeded`（先扫描）。
    scan: ScanMode,
}

impl AppState {
    /// 扫描进度推给前端的接收端。
    fn library_progress_sink(app: AppHandle) -> ScanProgressSink {
        Arc::new(move |progress: ScanProgressView| {
            let _ = app.emit(EVENT_LIBRARY_PROGRESS, progress);
        })
    }

    /// 设置里的曲库目录。
    fn library_directory(&self) -> String {
        self.settings().library_directory.clone()
    }
}

/// 扫描设置里的曲库目录并缓存结果（「重新扫描」）。逐批发 `library:progress` 事件；可用 `library_cancel` 取消。
/// 失败时 `Err` 是 `{ kind, message }`：`directoryMissing` / `cancelled` / `failed`。
#[tauri::command]
async fn scan_library(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LibrarySummaryView, LibraryCommandError> {
    let library = Arc::clone(&state.library);
    let directory = state.library_directory();
    library
        .scan(&directory, AppState::library_progress_sink(app))
        .await
}

/// 把已加载的歌单与本地曲库一对一对比；还没有扫描结果时按 `scan` 决定先扫描还是返回「尚未对比」。
/// 返回的 `matches` 与歌单歌曲一一对应（按歌单顺序）。
#[tauri::command]
async fn match_playlist(
    app: AppHandle,
    state: State<'_, AppState>,
    request: MatchPlaylistRequest,
) -> Result<MatchOutcomeView, LibraryCommandError> {
    let library = Arc::clone(&state.library);
    let directory = state.library_directory();
    library
        .match_playlist(
            request.playlist_id,
            request.scan,
            &directory,
            AppState::library_progress_sink(app),
        )
        .await
}

/// 取消正在进行的曲库扫描；没有扫描在运行时返回 `false`。
#[tauri::command]
fn library_cancel(state: State<'_, AppState>) -> bool {
    state.library.cancel()
}

impl AppState {
    /// 把下载 / 同步的进度事件推给前端的接收端。
    fn download_sink(app: AppHandle) -> ProgressSink {
        Arc::new(move |progress: DownloadProgress| {
            let _ = app.emit(EVENT_DOWNLOAD_PROGRESS, progress);
        })
    }

    /// 当前设置的快照（任务运行期间用户改设置不影响正在跑的任务）。
    fn settings_snapshot(&self) -> AppSettings {
        self.settings().clone()
    }

    /// 修改下载音质并写回 `settings.json`（只改这一个字段，其余设置原样保留）。
    ///
    /// 内存里的设置总是更新，让这次会话立刻生效；设置文件当初没能读出来（正在使用默认设置）时
    /// 不写盘，免得把用户真正的设置覆盖成默认值。
    fn set_quality(&self, quality: AudioQuality) -> Result<(), String> {
        let mut settings = self
            .settings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        settings.quality = quality;
        if self.settings_unreadable() {
            return Ok(());
        }
        match self.settings_store.save(&settings) {
            Ok(saved) => {
                *settings = saved;
                Ok(())
            }
            Err(error) => {
                let message = format!("保存下载音质失败：{error}");
                self.logs.warning(&message);
                Err(message)
            }
        }
    }
}

/// 修改下载音质（工具栏的下拉）。参数是 `AudioQuality` 的成员名；`AudioQuality` 自己的反序列化对未知值
/// 会宽容地回落到默认档位，这里要明确拒绝，免得误把设置改成默认值。
#[tauri::command]
fn set_quality(state: State<'_, AppState>, quality: String) -> Result<(), String> {
    state.set_quality(parse_quality(&quality)?)
}

/// 按成员名解析下载音质；未知名字返回中文说明。
fn parse_quality(name: &str) -> Result<AudioQuality, String> {
    AudioQuality::from_name(name.trim()).ok_or_else(|| format!("无法识别的下载音质：{name}"))
}

/// 构造某个任务的汇报出口：开始事件、逐曲进度、结束后重新扫描曲库的进度。
macro_rules! transfer_channels {
    ($app:expr, $started:ident, $channels:ident) => {
        let $started = {
            let app = $app.clone();
            move |plan: TransferStarted| {
                let _ = app.emit(EVENT_DOWNLOAD_STARTED, plan);
            }
        };
        let $channels = TransferChannels {
            started: &$started,
            sink: AppState::download_sink($app.clone()),
            scan_progress: AppState::library_progress_sink($app.clone()),
        };
    };
}

/// 下载用户勾选的歌曲。逐曲发 `download:progress` 事件；同一时间只允许一个下载 / 同步任务。
/// 失败时 `Err` 是 `{ kind, message }`：`busy` / `noSongs` / `playlistNotLoaded` / `cancelled` / `failed`。
#[tauri::command]
async fn download_tracks(
    app: AppHandle,
    state: State<'_, AppState>,
    playlist_id: i64,
    track_ids: Vec<i64>,
) -> Result<TransferSummaryView, TransferError> {
    let transfers = Arc::clone(&state.transfers);
    let settings = state.settings_snapshot();
    transfer_channels!(app, started, channels);
    transfers
        .download_tracks(playlist_id, &track_ids, &settings, &channels)
        .await
}

/// 下载本地曲库里没有的歌曲（NCM 待解密的不算缺失）。
#[tauri::command]
async fn download_missing(
    app: AppHandle,
    state: State<'_, AppState>,
    playlist_id: i64,
) -> Result<TransferSummaryView, TransferError> {
    let transfers = Arc::clone(&state.transfers);
    let settings = state.settings_snapshot();
    transfer_channels!(app, started, channels);
    transfers
        .download_missing(playlist_id, &settings, &channels)
        .await
}

/// 一键同步整个歌单：已存在的跳过，NCM 解密，缺失的下载。
#[tauri::command]
async fn sync_playlist(
    app: AppHandle,
    state: State<'_, AppState>,
    playlist_id: i64,
) -> Result<TransferSummaryView, TransferError> {
    let transfers = Arc::clone(&state.transfers);
    let settings = state.settings_snapshot();
    transfer_channels!(app, started, channels);
    transfers
        .sync_playlist(playlist_id, &settings, &channels)
        .await
}

/// 取消正在进行的下载 / 同步任务；没有任务时返回 `false`。
#[tauri::command]
fn download_cancel(state: State<'_, AppState>) -> bool {
    state.transfers.cancel()
}

/// 搜索歌曲（歌词与标签编辑器）。`source` 只能是 `Netease` / `QQMusic`；带了 `fileContext` 就给每条结果
/// 按本地文件打分（标题 / 歌手 / 专辑 / 时长四路分值 + 总分，低于匹配度阈值的标 `belowThreshold`），
/// 并按总分从高到低排序；`query` 留空时用文件名推出关键词。
/// 失败时 `Err` 是 `{ kind, message }`：`emptyQuery` / `fallbackSource` / `cancelled` / `failed`。
#[tauri::command]
async fn online_search(
    state: State<'_, AppState>,
    source: LyricsSourceKind,
    query: String,
    file_context: Option<FileContext>,
) -> Result<OnlineSearchResponse, OnlineCommandError> {
    let online = Arc::clone(&state.online);
    online
        .search(
            SearchRequest {
                source,
                query,
                file_context,
            },
            &CancelToken::new(),
        )
        .await
}

/// 取一首搜索结果的标签字段、歌词（逐行 / 逐字 / 双语）与封面。`id` 是搜索结果的 `sourceId`；
/// `song` 可选地传回搜索结果的完整摘要（不传就从最近搜索的缓存里找）。
/// 失败时 `Err` 是 `{ kind, message }`：`unknownSong` / `primarySource` / `cancelled` / `failed`。
#[tauri::command]
async fn online_load_result(
    state: State<'_, AppState>,
    source: LyricsSourceKind,
    id: String,
    word_lyrics: bool,
    song: Option<OnlineSongResult>,
) -> Result<LoadedResultView, OnlineCommandError> {
    let online = Arc::clone(&state.online);
    online
        .load_result(
            LoadRequest {
                source,
                id,
                word_lyrics,
                song,
            },
            &CancelToken::new(),
        )
        .await
}

/// 备用词源（`Kugou` / `Lrclib`，只提供歌词）按本地标题、歌手与时长检索一份歌词；没有匹配返回 `null`。
/// 阈值取自设置；不受“备用词源”开关限制（开关只作用于批量与自动补全）。
#[tauri::command]
async fn lyrics_source_fetch(
    state: State<'_, AppState>,
    source: LyricsSourceKind,
    title: String,
    artist: String,
    duration_ms: i64,
    album: Option<String>,
    word_lyrics: Option<bool>,
) -> Result<Option<FallbackLyricsView>, OnlineCommandError> {
    let online = Arc::clone(&state.online);
    online
        .fetch_fallback(
            FallbackRequest {
                source,
                title,
                artist,
                album,
                duration_ms,
                word_lyrics: word_lyrics.unwrap_or(false),
            },
            &CancelToken::new(),
        )
        .await
}

/// 当前设置里的在线匹配度阈值（0.30–1.00）与百分数文本。
#[tauri::command]
fn get_match_threshold(state: State<'_, AppState>) -> MatchThresholdView {
    state.online.threshold_view()
}

/// 扫描文件夹里的音频文件（`.mp3 .flac .m4a .ogg .wav .aac .wma .opus`）。
#[tauri::command]
async fn batch_scan(
    state: State<'_, AppState>,
    folder: String,
    recursive: bool,
) -> Result<BatchScanView, String> {
    let batch = Arc::clone(&state.batch);
    Ok(batch.scan(folder, recursive).await)
}

/// 批量获取标签、封面与歌词：最多 3 个文件并发，逐文件发 `batch:progress` 事件，结束后返回汇总
/// （被取消时是已处理的部分）。匹配度阈值取自设置；来源次序不传就用设置里的值。
/// 失败时 `Err` 是 `{ kind, message }`：`busy` / `noFiles`。
#[tauri::command]
async fn batch_run(
    app: AppHandle,
    state: State<'_, AppState>,
    options: BatchRequest,
) -> Result<BatchSummaryView, BatchJobError> {
    let batch = Arc::clone(&state.batch);
    let settings = state.settings_snapshot();
    let sink = Arc::new(move |progress: BatchProgressView| {
        let _ = app.emit(EVENT_BATCH_PROGRESS, progress);
    });
    batch.run(options, &settings, sink).await
}

/// 取消正在进行的批量任务；没有任务时返回 `false`。
#[tauri::command]
fn batch_cancel(state: State<'_, AppState>) -> bool {
    state.batch.cancel()
}

/// 编辑器打开时需要恢复的内容：上次的搜索来源与歌词格式、匹配度阈值、是否管理员运行（拖放会被系统阻断）、受支持的扩展名。
#[tauri::command]
fn editor_get_state(state: State<'_, AppState>) -> EditorStateView {
    EditorStateView::new(&state.settings(), is_elevated())
}

/// 记住编辑器的在线搜索来源与歌词格式（只改这两个设置）。
#[tauri::command]
fn editor_remember_options(
    state: State<'_, AppState>,
    source: LyricsSourceKind,
    prefer_word_lyrics: bool,
) {
    state.remember_editor_options(source, prefer_word_lyrics);
}

/// 把拖入的文件 / 文件夹解析成要载入编辑器的音频文件；`log` 为真时（主窗口拖放）顺便写运行日志。
#[tauri::command]
async fn resolve_drop(
    state: State<'_, AppState>,
    targets: Vec<String>,
    log: bool,
) -> Result<DropResolution, String> {
    let resolution = tauri::async_runtime::spawn_blocking(move || resolve_targets(&targets))
        .await
        .map_err(|_| "解析拖入的文件时程序内部出错，请重试。".to_owned())?;
    if log {
        match resolution.kind {
            DropKind::Audio => state.logs.info(&resolution.message),
            DropKind::Ncm | DropKind::Error => state.logs.warning(&resolution.message),
        }
    }
    Ok(resolution)
}

/// 读取用户选的封面图片（PNG / JPEG / WebP / GIF / BMP，最大 10 MB），返回 Base64 与 MIME。失败时 `Err` 是中文说明。
#[tauri::command]
async fn read_cover_file(path: String) -> Result<CoverFile, String> {
    tauri::async_runtime::spawn_blocking(move || read_cover_image(Path::new(&path)))
        .await
        .map_err(|_| "读取图片时程序内部出错，请重试。".to_owned())?
}

/// 命令行 `--lyrics/--edit <文件>` 指定的文件；只返回一次（前端取走后清空），没有时为 `null`。
#[tauri::command]
fn take_startup_file(state: State<'_, AppState>) -> Option<String> {
    state
        .startup_file
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
}

/// 给无边框透明窗口铺系统背景材质，返回实际生效的材质。
///
/// 亚克力会实时模糊并透出窗口下面的桌面与其他窗口，液态玻璃界面靠它“看见下一层”；云母只取壁纸色调。
/// 首选材质不可用时退到另一种；两种都不可用（如远程桌面）时保持纯透明，由前端的玻璃底色兜底。
fn apply_window_material(window: &WebviewWindow, material: WindowMaterial) -> WindowMaterial {
    let try_effect = |effect: Effect| {
        window
            .set_effects(EffectsBuilder::new().effect(effect).build())
            .is_ok()
    };
    let order: &[(Effect, WindowMaterial)] = match material {
        WindowMaterial::Acrylic => &[
            (Effect::Acrylic, WindowMaterial::Acrylic),
            (Effect::Mica, WindowMaterial::Mica),
        ],
        WindowMaterial::Mica => &[
            (Effect::Mica, WindowMaterial::Mica),
            (Effect::Acrylic, WindowMaterial::Acrylic),
        ],
        WindowMaterial::None => &[],
    };
    for (effect, applied) in order {
        if try_effect(*effect) {
            return *applied;
        }
    }
    let _ = window.set_effects(None::<WindowEffectsConfig>);
    WindowMaterial::None
}

/// 让系统材质的底色跟随界面主题（亚克力 / 云母会按窗口主题着色），`System` 交还给系统决定。
fn apply_window_theme(window: &WebviewWindow, theme: ThemeMode) {
    let theme = match theme {
        ThemeMode::Dark => Some(tauri::Theme::Dark),
        ThemeMode::Light => Some(tauri::Theme::Light),
        ThemeMode::System => None,
    };
    let _ = window.set_theme(theme);
}

/// 主窗口的外观：主题 + 材质，返回实际生效的材质。
fn apply_appearance(app: &AppHandle, material: WindowMaterial, theme: ThemeMode) -> WindowMaterial {
    match app.get_webview_window("main") {
        Some(window) => {
            apply_window_theme(&window, theme);
            apply_window_material(&window, material)
        }
        None => material,
    }
}

/// 设置窗口需要的全部数据：完整设置、路径信息与可选项。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsDto {
    settings: AppSettings,
    data_dir: String,
    settings_file: String,
    is_portable: bool,
    default_download_dir: String,
    /// 设置文件读取失败的原因（保存一次新设置后为空）。
    warning: Option<String>,
    quality_options: Vec<QualityOptionDto>,
}

impl AppState {
    fn settings_dto(&self) -> SettingsDto {
        SettingsDto {
            settings: self.settings().clone(),
            data_dir: display_path(&self.paths.data_dir),
            settings_file: display_path(self.settings_store.file_path()),
            is_portable: self.paths.is_portable,
            default_download_dir: display_path(&self.paths.default_download_dir),
            warning: self.active_settings_warning().map(str::to_owned),
            quality_options: quality_options(),
        }
    }
}

/// 读取完整设置（设置窗口打开时调用）。
#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> SettingsDto {
    state.settings_dto()
}

/// 保存设置窗口里改动过的字段（只传改动的键，camelCase），保存后立即应用窗口主题与材质。
/// 校验失败或写盘失败时 `Err` 是中文说明，内存与文件里的设置都保持原样。
#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: serde_json::Value,
) -> Result<SettingsDto, String> {
    let saved = state.save_settings_patch(&patch)?;
    apply_appearance(&app, saved.window_material, saved.theme);
    Ok(state.settings_dto())
}

/// 设置窗口里实时预览窗口主题与材质（不写盘）；取消时前端用已保存的值再调一次即可还原。
/// 返回实际生效的材质名（首选材质不可用时会退到另一种）。
#[tauri::command]
fn preview_appearance(
    app: AppHandle,
    material: String,
    theme: String,
) -> Result<WindowMaterial, String> {
    let material = WindowMaterial::from_name(&material)
        .ok_or_else(|| format!("无法识别的窗口材质：{material}"))?;
    let theme = ThemeMode::from_name(&theme).ok_or_else(|| format!("无法识别的主题：{theme}"))?;
    Ok(apply_appearance(&app, material, theme))
}

/// 启动 Tauri 应用。
pub fn run() {
    let paths = AppPaths::resolve(&PathEnv::from_system())
        .expect("无法准备数据目录：设置、登录态与 WebView 缓存都没有可写的位置");
    let webview_dir = paths.webview_dir();
    let state = AppState::load(paths);
    state.apply_startup_args(std::env::args());
    if is_elevated() {
        // 提权运行时 Windows 会阻止资源管理器向本程序拖放文件，提前在日志里说明（同 C#）。
        state.logs.warning(lp_core::editor::DRAG_DROP_BLOCKED_HINT);
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(move |app| {
            // 把后端的日志与账号变化推给前端。
            let state = app.state::<AppState>();
            let handle = app.handle().clone();
            state.logs.set_listener(move |entry| {
                let _ = handle.emit(EVENT_LOG, entry);
            });
            let handle = app.handle().clone();
            state.login.set_account_listener(move |account| {
                let _ = handle.emit(EVENT_ACCOUNT, account);
            });

            // 主窗口在这里手动创建（tauri.conf.json 里 `create: false`），
            // 以便把 WebView2 的用户数据目录显式放到 `<数据目录>\webview`。
            // 不显式指定时 Tauri 会用 `%LOCALAPPDATA%\<identifier>\EBWebView`，
            // 见 docs/TAURI_REWRITE.md 关于低完整性级别环境的说明。
            let _ = std::fs::create_dir_all(&webview_dir);
            let window_config = app
                .config()
                .app
                .windows
                .first()
                .cloned()
                .ok_or("tauri.conf.json 中缺少主窗口配置")?;
            let window = WebviewWindowBuilder::from_config(app.handle(), &window_config)?
                .data_directory(webview_dir.clone())
                .build()?;
            let (material, theme) = {
                let settings = state.settings();
                (settings.window_material, settings.theme)
            };
            apply_window_theme(&window, theme);
            let applied = apply_window_material(&window, material);
            if applied != material {
                state.logs.warning(format!(
                    "窗口材质「{}」不可用，已改用「{}」",
                    material.name(),
                    applied.name()
                ));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            get_app_state,
            get_logs,
            login_status,
            validate_login,
            qr_start,
            qr_poll,
            logout,
            recheck_login,
            list_playlists,
            get_playlist_tracks,
            get_song_lyrics,
            read_tags,
            write_tags,
            ncm_get_state,
            ncm_scan,
            ncm_save_state,
            ncm_import,
            ncm_cancel,
            scan_library,
            match_playlist,
            library_cancel,
            set_quality,
            download_tracks,
            download_missing,
            sync_playlist,
            download_cancel,
            online_search,
            online_load_result,
            lyrics_source_fetch,
            get_match_threshold,
            batch_scan,
            batch_run,
            batch_cancel,
            editor_get_state,
            editor_remember_options,
            resolve_drop,
            read_cover_file,
            take_startup_file,
            get_settings,
            save_settings,
            preview_appearance
        ])
        .build(tauri::generate_context!())
        .expect("启动 Lunatic Princess 失败");

    app.run(|app_handle, event| {
        if let RunEvent::Exit = event {
            // 退出前停掉定时续期，并等已排队的 Cookie 落盘写完，避免丢掉刚轮换的登录态。
            let login = Arc::clone(&app_handle.state::<AppState>().login);
            tauri::async_runtime::block_on(login.shutdown());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> AppPaths {
        AppPaths {
            data_dir: r"D:\data".into(),
            default_download_dir: r"D:\Music".into(),
            is_portable: true,
        }
    }

    fn dto(account: &AccountSummary, session_error: Option<&str>) -> AppStateDto {
        let mut settings = AppSettings::new(r"D:\Music");
        settings.theme = ThemeMode::Light;
        build_app_state_dto(&paths(), &settings, None, account, session_error)
    }

    fn account(logged_in: bool) -> AccountSummary {
        AccountSummary {
            logged_in,
            nickname: logged_in.then(|| "测试".to_owned()),
            user_id: if logged_in { 7 } else { 0 },
            vip_type: 0,
            avatar_url: logged_in.then(|| "https://p1.music.126.net/a.jpg".to_owned()),
        }
    }

    #[test]
    fn dto_reports_paths_and_settings_summary() {
        let json = serde_json::to_value(dto(&account(false), None)).unwrap();
        assert_eq!(json["dataDir"], r"D:\data");
        assert_eq!(json["isPortable"], true);
        assert_eq!(json["settings"]["theme"], "Light");
        assert_eq!(json["settings"]["quality"], "ExHigh");
        assert_eq!(json["settings"]["maxConcurrency"], 3);
        assert_eq!(json["settings"]["autoMatchLocal"], true);
        assert_eq!(json["session"]["loggedIn"], false);
        assert!(json["session"]["nickname"].is_null());
    }

    #[test]
    fn dto_exposes_the_account_but_never_a_cookie() {
        let text = serde_json::to_string(&dto(&account(true), None)).unwrap();
        assert!(!text.contains("MUSIC_U"));
        assert!(!text.to_lowercase().contains("cookie"));
        assert!(text.contains("\"loggedIn\":true"));
        assert!(text.contains("测试"));
        assert!(text.contains("\"avatarUrl\":\"https://p1.music.126.net/a.jpg\""));
    }

    #[test]
    fn dto_carries_session_load_error() {
        let dto = dto(&account(false), Some("登录文件格式不正确或已损坏"));
        assert!(!dto.session.logged_in);
        assert_eq!(
            dto.session.load_error.as_deref(),
            Some("登录文件格式不正确或已损坏")
        );
    }

    #[test]
    fn validation_dto_uses_camel_case_status_names() {
        let cases = [
            (ValidationOutcome::NotLoggedIn, "notLoggedIn"),
            (ValidationOutcome::Valid, "valid"),
            (
                ValidationOutcome::Unverified {
                    message: "稍后重试".to_owned(),
                },
                "unverified",
            ),
            (ValidationOutcome::LoggedOut, "loggedOut"),
        ];
        for (outcome, status) in cases {
            let dto = ValidationDto::new(outcome, account(false));
            assert_eq!(dto.status, status);
        }
        let unverified = ValidationDto::new(
            ValidationOutcome::Unverified {
                message: "稍后重试".to_owned(),
            },
            account(true),
        );
        assert_eq!(unverified.message.as_deref(), Some("稍后重试"));
    }

    #[test]
    fn qr_poll_dto_maps_every_outcome() {
        assert_eq!(
            QrPollDto::from_result(Ok(QrPollOutcome::Waiting)),
            QrPollDto::plain("waiting")
        );
        assert_eq!(
            QrPollDto::from_result(Ok(QrPollOutcome::Scanned)).state,
            "scanned"
        );
        assert_eq!(
            QrPollDto::from_result(Ok(QrPollOutcome::Expired)).state,
            "expired"
        );

        let failed = QrPollDto::from_result(Ok(QrPollOutcome::Failed {
            message: "扫码登录失败".to_owned(),
        }));
        assert_eq!(failed.state, "failed");
        assert_eq!(failed.message.as_deref(), Some("扫码登录失败"));

        let success = QrPollDto::from_result(Ok(QrPollOutcome::Success {
            account: account(true),
        }));
        assert_eq!(success.state, "success");
        assert_eq!(success.account.unwrap().user_id, 7);
    }

    #[test]
    fn qr_poll_dto_marks_network_errors_as_transient() {
        use lp_core::netease::NeteaseErrorKind;

        let network = QrPollDto::from_result(Err(NeteaseError::new(
            "网络请求失败",
            None,
            NeteaseErrorKind::Network,
        )));
        assert_eq!(network.state, "error");
        assert!(network.transient);
        assert_eq!(
            network.message.as_deref(),
            Some("网络连接失败，请检查网络后重试")
        );

        let api = QrPollDto::from_result(Err(NeteaseError::api("x", 500)));
        assert_eq!(api.state, "error");
        assert!(!api.transient);
    }

    #[test]
    fn qr_start_dto_serializes_to_camel_case() {
        let json = serde_json::to_value(QrStartDto {
            key: "k".to_owned(),
            svg: "<svg/>".to_owned(),
            lifetime_secs: 180,
        })
        .unwrap();
        assert_eq!(json["lifetimeSecs"], 180);
        assert_eq!(json["svg"], "<svg/>");
    }

    #[test]
    fn playlist_progress_dto_serializes_to_camel_case() {
        let json = serde_json::to_value(PlaylistProgressDto {
            playlist_id: 9,
            loaded: 500,
            total: 1832,
        })
        .unwrap();
        assert_eq!(json["playlistId"], 9);
        assert_eq!(json["loaded"], 500);
        assert_eq!(json["total"], 1832);
    }

    #[test]
    fn playlist_errors_reach_the_frontend_as_kind_and_message() {
        use lp_core::playlist::PlaylistErrorKind;

        let json = serde_json::to_value(PlaylistError {
            kind: PlaylistErrorKind::Failed,
            message: "加载歌单失败：网络连接失败，请检查网络后重试".to_owned(),
        })
        .unwrap();
        assert_eq!(json["kind"], "failed");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .starts_with("加载歌单失败")
        );
    }

    /// 在系统临时目录里建一个独立的数据目录，用完删除。
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!("lp-shell-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn paths(&self) -> AppPaths {
            AppPaths {
                data_dir: self.0.clone(),
                default_download_dir: self.0.join("Music"),
                is_portable: true,
            }
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn ncm_import_request_is_read_from_camel_case_json() {
        let request: NcmImportRequest = serde_json::from_value(serde_json::json!({
            "paths": [r"D:\a.ncm"],
            "folders": [r"D:\in"],
            "outputDirectory": r"D:\out",
            "overwrite": true,
            "autoMatchOnline": false,
        }))
        .unwrap();

        assert_eq!(request.paths, [r"D:\a.ncm"]);
        assert_eq!(request.folders, [r"D:\in"]);
        assert_eq!(request.output_directory, r"D:\out");
        assert!(request.overwrite);
        assert!(!request.auto_match_online);
    }

    #[test]
    fn remembering_the_ncm_state_writes_only_its_two_fields() {
        let dir = ScratchDir::new("remember");
        let state = AppState::load(dir.paths());
        // 先在磁盘上放一份带有非默认值的设置。
        {
            let mut settings = state.settings().clone();
            settings.max_concurrency = 6;
            settings.match_threshold = 0.8;
            state.settings_store.save(&settings).unwrap();
            *state.settings.write().unwrap() = settings;
        }

        state.remember_ncm_state(
            &[r"D:\a".to_owned(), r"d:\A".to_owned(), r"E:\b".to_owned()],
            r"  E:\out  ",
        );

        let saved = state.settings_store.load().unwrap();
        assert_eq!(saved.ncm_import_folders, [r"D:\a", r"E:\b"]);
        assert_eq!(saved.ncm_import_output_directory, r"E:\out");
        assert_eq!(saved.max_concurrency, 6);
        assert_eq!(saved.match_threshold, 0.8);
        // 内存里的设置同步更新，窗口再次打开时读到的就是新值。
        let dialog = ncm_dialog_state(&state);
        assert_eq!(dialog.folders, [r"D:\a", r"E:\b"]);
        assert_eq!(dialog.output_directory, r"E:\out");
        assert!(dialog.online_match_available);
    }

    #[test]
    fn untrusted_settings_are_never_overwritten_by_the_ncm_memory() {
        let dir = ScratchDir::new("untrusted");
        let mut state = AppState::load(dir.paths());
        // 设置文件当初没能读出来，正在使用默认值：写盘会把用户真正的设置覆盖掉。
        state.settings_warning = Some("读取失败".to_owned());

        state.remember_ncm_state(&[r"D:\a".to_owned()], r"E:\out");

        assert!(!state.paths.settings_file().exists());
    }

    fn ncm_dialog_state(state: &AppState) -> NcmDialogState {
        NcmDialogState::new(&state.settings(), state.ncm.online_match_available())
    }

    #[test]
    fn only_one_ncm_import_runs_at_a_time_and_cancel_reaches_it() {
        let dir = ScratchDir::new("job");
        let state = AppState::load(dir.paths());

        let job = state.ncm_job.try_begin().unwrap();
        assert!(state.ncm_job.try_begin().is_err());
        assert!(state.ncm_job.cancel());
        assert!(job.cancel.is_cancelled());
        drop(job);
        assert!(!state.ncm_job.cancel());
    }

    #[test]
    fn match_playlist_request_is_read_from_camel_case_json() {
        let request: MatchPlaylistRequest = serde_json::from_value(serde_json::json!({
            "playlistId": 123456789012_i64,
            "scan": "ifNeeded",
        }))
        .unwrap();
        assert_eq!(request.playlist_id, 123_456_789_012);
        assert_eq!(request.scan, ScanMode::IfNeeded);

        let never: MatchPlaylistRequest = serde_json::from_value(serde_json::json!({
            "playlistId": 1,
            "scan": "never",
        }))
        .unwrap();
        assert_eq!(never.scan, ScanMode::Never);
        assert!(
            serde_json::from_value::<MatchPlaylistRequest>(serde_json::json!({
                "playlistId": 1,
                "scan": "always",
            }))
            .is_err()
        );
    }

    #[test]
    fn the_library_directory_comes_from_the_settings_and_cancel_is_a_noop_when_idle() {
        let dir = ScratchDir::new("library");
        let state = AppState::load(dir.paths());
        {
            let mut settings = state.settings.write().unwrap();
            settings.library_directory = r"D:\Music\Lib".to_owned();
        }

        assert_eq!(state.library_directory(), r"D:\Music\Lib");
        assert!(!state.library.cancel());
        assert!(!state.library.is_scanning());
    }

    #[test]
    fn quality_options_list_every_level_with_its_display_name() {
        let json = serde_json::to_value(dto(&account(false), None)).unwrap();
        let options = json["qualityOptions"].as_array().unwrap();
        assert_eq!(options.len(), 9);
        assert_eq!(options[0]["value"], "Standard");
        assert_eq!(options[0]["label"], "标准");
        assert_eq!(options[1]["value"], "ExHigh");
        assert_eq!(options[1]["label"], "极高 (HQ)");
        assert_eq!(options[8]["value"], "DolbyAtmos");
        assert_eq!(options[8]["label"], "杜比全景声 (Dolby Atmos)");
    }

    #[test]
    fn changing_the_quality_writes_only_that_field() {
        let dir = ScratchDir::new("quality");
        let state = AppState::load(dir.paths());
        {
            let mut settings = state.settings().clone();
            settings.max_concurrency = 6;
            settings.match_threshold = 0.8;
            settings.ncm_import_folders = vec![r"D:\in".to_owned()];
            state.settings_store.save(&settings).unwrap();
            *state.settings.write().unwrap() = settings;
        }

        state.set_quality(AudioQuality::Master).unwrap();

        let saved = state.settings_store.load().unwrap();
        assert_eq!(saved.quality, AudioQuality::Master);
        assert_eq!(saved.max_concurrency, 6);
        assert_eq!(saved.match_threshold, 0.8);
        assert_eq!(saved.ncm_import_folders, [r"D:\in"]);
        // 内存里的设置同步更新，下一次下载直接用新音质。
        assert_eq!(state.settings_snapshot().quality, AudioQuality::Master);
    }

    #[test]
    fn only_known_quality_names_are_accepted() {
        assert_eq!(parse_quality("Master"), Ok(AudioQuality::Master));
        assert_eq!(parse_quality(" DolbyAtmos "), Ok(AudioQuality::DolbyAtmos));
        // `AudioQuality` 的反序列化会把未知值悄悄变成默认档位，命令必须自己拒绝。
        assert_eq!(
            parse_quality("Nope"),
            Err("无法识别的下载音质：Nope".to_owned())
        );
        assert!(parse_quality("").is_err());
    }

    #[test]
    fn saving_settings_writes_only_the_patch_and_keeps_other_fields() {
        let dir = ScratchDir::new("settings-save");
        let state = AppState::load(dir.paths());
        state.remember_ncm_state(&[r"D:".to_owned()], r"E:\out");

        let dto = state
            .save_settings_patch(&serde_json::json!({ "theme": "Light", "glassOpacity": 0.3 }))
            .unwrap();

        assert_eq!(dto.theme, ThemeMode::Light);
        assert_eq!(dto.glass_opacity, 0.3);
        let on_disk = JsonSettingsStore::new(state.paths.settings_file(), r"D:\Music")
            .load()
            .unwrap();
        assert_eq!(on_disk.theme, ThemeMode::Light);
        assert_eq!(on_disk.ncm_import_folders, [r"D:"]);
        assert_eq!(on_disk.ncm_import_output_directory, r"E:\out");
    }

    #[test]
    fn invalid_settings_patch_changes_nothing() {
        let dir = ScratchDir::new("settings-invalid");
        let state = AppState::load(dir.paths());
        let before = state.settings_snapshot();

        let error = state
            .save_settings_patch(&serde_json::json!({ "theme": "Light", "maxConcurrency": 99 }))
            .unwrap_err();

        assert!(error.contains("并发"), "{error}");
        assert_eq!(state.settings_snapshot(), before);
        assert!(!state.paths.settings_file().exists());
    }

    #[test]
    fn an_explicit_save_replaces_unreadable_settings_and_clears_the_warning() {
        let dir = ScratchDir::new("settings-recover");
        let mut state = AppState::load(dir.paths());
        state.settings_warning = Some("读取失败".to_owned());
        assert_eq!(state.settings_dto().warning.as_deref(), Some("读取失败"));

        state
            .save_settings_patch(&serde_json::json!({ "reduceMotion": true }))
            .unwrap();

        assert!(state.paths.settings_file().exists());
        assert!(state.settings_dto().warning.is_none());
        assert!(state.to_dto().settings_warning.is_none());
        // 保存过之后，自动记忆也恢复写盘。
        state.set_quality(AudioQuality::Lossless).unwrap();
        let on_disk = JsonSettingsStore::new(state.paths.settings_file(), r"D:\Music")
            .load()
            .unwrap();
        assert_eq!(on_disk.quality, AudioQuality::Lossless);
        assert!(on_disk.reduce_motion);
    }

    #[test]
    fn untrusted_settings_are_never_overwritten_by_a_quality_change() {
        let dir = ScratchDir::new("quality-untrusted");
        let mut state = AppState::load(dir.paths());
        state.settings_warning = Some("读取失败".to_owned());

        state.set_quality(AudioQuality::Lossless).unwrap();

        assert!(!state.paths.settings_file().exists());
        assert_eq!(state.settings_snapshot().quality, AudioQuality::Lossless);
    }

    #[test]
    fn download_and_sync_share_one_job_slot_and_cancel_is_a_noop_when_idle() {
        let dir = ScratchDir::new("transfer-idle");
        let state = AppState::load(dir.paths());
        assert!(!state.transfers.cancel());
        assert!(!state.transfers.is_running());
    }

    #[test]
    fn online_enrichment_is_wired_into_ncm_import_downloads_and_sync() {
        let dir = ScratchDir::new("online-wired");
        let state = AppState::load(dir.paths());
        // NCM 导入窗口据此显示“在线匹配可用”（不再是“将在后续版本提供”）。
        assert!(state.ncm.online_match_available());
        let dialog = serde_json::to_value(NcmDialogState::new(
            &state.settings(),
            state.ncm.online_match_available(),
        ))
        .unwrap();
        assert_eq!(dialog["onlineMatchAvailable"], true);
        assert!(
            dialog["onlineMatchHint"]
                .as_str()
                .unwrap()
                .contains("QQ 音乐逐字歌词优先")
        );
    }

    #[test]
    fn the_match_threshold_command_reads_the_live_settings() {
        let dir = ScratchDir::new("threshold");
        let state = AppState::load(dir.paths());
        assert_eq!(state.online.threshold_view().percent, "60");

        state.settings.write().unwrap().match_threshold = 0.82;

        let view = state.online.threshold_view();
        assert_eq!(view.percent, "82");
        assert_eq!(view.threshold, 0.82);
    }

    #[test]
    fn the_batch_job_is_idle_after_startup_and_cancel_is_a_noop() {
        let dir = ScratchDir::new("batch-idle");
        let state = AppState::load(dir.paths());
        assert!(!state.batch.cancel());
    }

    #[test]
    fn online_commands_use_the_documented_event_name_and_camel_case_arguments() {
        assert_eq!(EVENT_BATCH_PROGRESS, "batch:progress");
        // `fileContext` / `wordLyrics` 等参数名由 Tauri 从 snake_case 转换，这里固化前端依赖的请求结构。
        let context: FileContext = serde_json::from_value(serde_json::json!({
            "title": "晴天", "artist": "周杰伦", "durationMs": 269000, "filePath": r"D:.mp3"
        }))
        .unwrap();
        assert_eq!(context.duration_ms, 269_000);
        let batch: BatchRequest = serde_json::from_value(serde_json::json!({
            "files": [r"D:.mp3"], "overwriteExisting": true, "preferWordLyrics": true
        }))
        .unwrap();
        assert!(batch.fetch_lyrics && batch.overwrite_existing && batch.prefer_word_lyrics);
    }

    #[test]
    fn remembering_the_editor_options_writes_only_its_two_fields() {
        let dir = ScratchDir::new("editor-remember");
        let state = AppState::load(dir.paths());
        {
            let mut settings = state.settings().clone();
            settings.max_concurrency = 6;
            settings.match_threshold = 0.8;
            settings.ncm_import_folders = vec![r"D:\in".to_owned()];
            state.settings_store.save(&settings).unwrap();
            *state.settings.write().unwrap() = settings;
        }

        state.remember_editor_options(LyricsSourceKind::QqMusic, true);

        let saved = state.settings_store.load().unwrap();
        assert_eq!(saved.lyrics_editor_search_source, LyricsSourceKind::QqMusic);
        assert!(saved.lyrics_editor_prefer_word_lyrics);
        assert_eq!(saved.max_concurrency, 6);
        assert_eq!(saved.match_threshold, 0.8);
        assert_eq!(saved.ncm_import_folders, [r"D:\in"]);
        // 下次打开编辑器读到的就是新值。
        let view = serde_json::to_value(EditorStateView::new(&state.settings(), false)).unwrap();
        assert_eq!(view["searchSource"], "QQMusic");
        assert_eq!(view["preferWordLyrics"], true);
    }

    #[test]
    fn unchanged_editor_options_do_not_touch_the_disk() {
        let dir = ScratchDir::new("editor-unchanged");
        let state = AppState::load(dir.paths());

        // 默认值（QQ 音乐、逐行）再记一次：没有变化，不创建设置文件。
        state.remember_editor_options(LyricsSourceKind::QqMusic, false);

        assert!(!state.paths.settings_file().exists());
    }

    #[test]
    fn untrusted_settings_are_never_overwritten_by_the_editor_memory() {
        let dir = ScratchDir::new("editor-untrusted");
        let mut state = AppState::load(dir.paths());
        state.settings_warning = Some("读取失败".to_owned());

        state.remember_editor_options(LyricsSourceKind::Lrclib, true);

        assert!(!state.paths.settings_file().exists());
        assert_eq!(
            state.settings().lyrics_editor_search_source,
            LyricsSourceKind::Lrclib
        );
    }

    #[test]
    fn the_startup_file_is_handed_to_the_frontend_exactly_once() {
        let dir = ScratchDir::new("startup");
        let state = AppState::load(dir.paths());
        let file = dir.0.join("x.mp3");
        std::fs::write(&file, b"x").unwrap();
        let file_text = file.to_string_lossy().into_owned();

        state.apply_startup_args(["LunaticPrincess.exe", "--edit", file_text.as_str()]);

        let taken = state.startup_file.lock().unwrap().take();
        assert_eq!(taken.as_deref(), Some(file_text.as_str()));
        assert!(state.startup_file.lock().unwrap().is_none());
    }

    #[test]
    fn a_missing_startup_file_only_logs_a_warning() {
        let dir = ScratchDir::new("startup-missing");
        let state = AppState::load(dir.paths());

        state.apply_startup_args(["LunaticPrincess.exe", "--lyrics", r"D:\不存在\x.mp3"]);

        assert!(state.startup_file.lock().unwrap().is_none());
        assert!(
            state
                .logs
                .recent()
                .iter()
                .any(|entry| entry.message.contains("启动参数指定的文件不存在"))
        );
    }

    #[test]
    fn transfer_commands_use_camel_case_names_for_the_frontend() {
        // Tauri 把 Rust 参数名转成 camelCase：这里固化前端依赖的几个事件名。
        assert_eq!(EVENT_DOWNLOAD_STARTED, "download:started");
        assert_eq!(EVENT_DOWNLOAD_PROGRESS, "download:progress");
    }
}
