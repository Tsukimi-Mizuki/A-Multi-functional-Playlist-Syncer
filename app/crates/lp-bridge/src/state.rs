//! 桥接层的组合根：启动时解析数据目录、读设置与登录态，装配 `lp-core` 的服务（对应 Tauri 外壳的 `AppState::load`）。
//!
//! 原型阶段只装配登录、歌单与本地曲库；其余服务（下载、在线检索、NCM……）按画面的进度逐个接上。

use std::future::Future;
use std::sync::{Arc, OnceLock, RwLock};

use lp_core::auth::{LoginManager, LoginOptions};
use lp_core::library::{LibraryManager, LocalLibraryService};
use lp_core::logs::LogHub;
use lp_core::ncm::FileNcmDecoder;
use lp_core::netease::NeteaseClient;
use lp_core::netease::http::{HttpTransport, ReqwestTransport};
use lp_core::paths::{AppPaths, PathEnv};
use lp_core::playlist::PlaylistService;
use lp_core::session::SessionStore;
use lp_core::settings::{AppSettings, JsonSettingsStore};
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

        let transport: Arc<dyn HttpTransport> = Arc::new(
            ReqwestTransport::new(lp_core::netease::client::REQUEST_TIMEOUT)
                .map_err(|error| format!("初始化网络组件失败：{error:?}"))?,
        );
        let client = Arc::new(NeteaseClient::new(transport));
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

        Ok(Self {
            paths,
            settings: Arc::new(RwLock::new(settings)),
            settings_store,
            settings_warning: Arc::new(RwLock::new(settings_warning)),
            session_error: restored.load_failure,
            logs,
            login,
            playlists,
            library,
        })
    }
}
