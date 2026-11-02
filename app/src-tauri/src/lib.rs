//! Tauri 外壳：只做命令转发、事件推送、窗口与插件，业务逻辑一律在 `lp-core`。

use std::path::Path;
use std::sync::{Arc, RwLock, RwLockReadGuard};

use lp_core::auth::{
    AccountSummary, LoginManager, LoginOptions, QrPollOutcome, RuntimeAuthOutcome,
    ValidationOutcome,
};
use lp_core::logs::{LogEntry, LogHub};
use lp_core::models::{AudioQuality, ThemeMode};
use lp_core::netease::http::ReqwestTransport;
use lp_core::netease::{NeteaseClient, NeteaseError};
use lp_core::paths::{AppPaths, PathEnv};
use lp_core::session::SessionStore;
use lp_core::settings::{AppSettings, JsonSettingsStore};
use serde::Serialize;
use tauri::window::{Effect, EffectsBuilder};
use tauri::{Emitter, Manager, RunEvent, State, WebviewWindow, WebviewWindowBuilder};

/// 前端订阅的事件名：新的运行日志。
const EVENT_LOG: &str = "app:log";
/// 前端订阅的事件名：账号状态变化（登录成功、退出、登录失效、资料刷新）。
const EVENT_ACCOUNT: &str = "auth:state";

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
    settings: RwLock<AppSettings>,
    /// 设置文件读取失败时的中文原因（此时界面使用默认设置）。
    settings_warning: Option<String>,
    /// 运行日志（内存，最多 400 条）。
    logs: Arc<LogHub>,
    /// 登录编排；登录态（含 Cookie）只留在后端，前端永远拿不到。
    login: Arc<LoginManager>,
    /// 登录文件读取 / 解密失败时的中文原因。
    session_error: Option<String>,
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

        let transport = ReqwestTransport::new(lp_core::netease::client::REQUEST_TIMEOUT)
            .expect("初始化网络组件失败");
        let client = Arc::new(NeteaseClient::new(Arc::new(transport)));
        let session_store = Arc::new(SessionStore::with_system_protector(paths.session_file()));
        let login = LoginManager::new(
            client,
            session_store,
            Arc::clone(&logs),
            LoginOptions::default(),
        );

        let restored = login.restore();
        if restored.load_failure.is_some() {
            logs.warning(format!(
                "登录文件位于数据目录：{}",
                display_path(&paths.data_dir)
            ));
        }

        Self {
            paths,
            settings: RwLock::new(settings),
            settings_warning,
            logs,
            login,
            session_error: restored.load_failure,
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
            self.settings_warning.as_deref(),
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
        },
        settings_warning: settings_warning.map(str::to_owned),
        session: SessionSummaryDto {
            logged_in: account.logged_in,
            nickname: account.nickname.clone(),
            load_error: session_error.map(str::to_owned),
            account: account.clone(),
        },
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

/// 给无边框透明窗口铺系统背景材质：优先 Mica（Win11），不支持时回退 Acrylic。
fn apply_window_effects(window: &WebviewWindow) {
    let mica = EffectsBuilder::new().effect(Effect::Mica).build();
    if window.set_effects(mica).is_err() {
        let acrylic = EffectsBuilder::new().effect(Effect::Acrylic).build();
        // 两种材质都不可用（如远程桌面）时保持纯透明，由前端 CSS 背景兜底。
        let _ = window.set_effects(acrylic);
    }
}

/// 启动 Tauri 应用。
pub fn run() {
    let paths = AppPaths::resolve(&PathEnv::from_system())
        .expect("无法准备数据目录：设置、登录态与 WebView 缓存都没有可写的位置");
    let webview_dir = paths.webview_dir();
    let state = AppState::load(paths);

    let app = tauri::Builder::default()
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
            apply_window_effects(&window);
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
            recheck_login
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
}
