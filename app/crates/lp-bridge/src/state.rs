//! 桥接层的组合根：启动时解析数据目录、读设置与登录态，装配 `lp-core` 的服务（对应 Tauri 外壳的 `AppState::load`）。
//!
//! 原型阶段只装配登录与歌单；其余服务（下载、曲库、在线匹配……）按画面的进度逐个接上。

use std::future::Future;
use std::sync::{Arc, OnceLock};

use lp_core::auth::{LoginManager, LoginOptions};
use lp_core::logs::LogHub;
use lp_core::netease::NeteaseClient;
use lp_core::netease::http::{HttpTransport, ReqwestTransport};
use lp_core::paths::{AppPaths, PathEnv};
use lp_core::playlist::PlaylistService;
use lp_core::session::SessionStore;
use lp_core::settings::{AppSettings, JsonSettingsStore};
use tokio::runtime::Runtime;

/// 进程内唯一的桥接状态。
pub(crate) struct Bridge {
    /// `lp-core` 的异步代码（reqwest、续期定时器、登录态自动落盘）都跑在这个运行时里：
    /// flutter_rust_bridge 自己的执行器不保证是 tokio，直接 `await` 会找不到 reactor。
    pub runtime: Runtime,
    pub paths: AppPaths,
    pub settings: AppSettings,
    /// 设置文件读取失败时的中文原因（此时使用默认设置）。
    pub settings_warning: Option<String>,
    /// 登录文件读取 / 解密失败时的中文原因。
    pub session_error: Option<String>,
    pub logs: Arc<LogHub>,
    pub login: Arc<LoginManager>,
    pub playlists: Arc<PlaylistService>,
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
            settings_warning: assembled.settings_warning,
            session_error: assembled.session_error,
            logs: assembled.logs,
            login: assembled.login,
            playlists: assembled.playlists,
        })
    }
}

/// 运行时以外的那部分状态（先装配好，再和运行时一起放进 [`Bridge`]）。
struct Assembled {
    paths: AppPaths,
    settings: AppSettings,
    settings_warning: Option<String>,
    session_error: Option<String>,
    logs: Arc<LogHub>,
    login: Arc<LoginManager>,
    playlists: Arc<PlaylistService>,
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

        Ok(Self {
            paths,
            settings,
            settings_warning,
            session_error: restored.load_failure,
            logs,
            login,
            playlists,
        })
    }
}
