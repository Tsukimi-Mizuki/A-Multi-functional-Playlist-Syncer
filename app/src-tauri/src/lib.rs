//! Tauri 外壳：只做命令转发、窗口与插件，业务逻辑一律在 `lp-core`。

use std::path::Path;
use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard};

use lp_core::models::{AudioQuality, ThemeMode};
use lp_core::paths::{AppPaths, PathEnv};
use lp_core::session::{NeteaseSession, SessionStore};
use lp_core::settings::{AppSettings, JsonSettingsStore};
use serde::Serialize;
use tauri::window::{Effect, EffectsBuilder};
use tauri::{State, WebviewWindow, WebviewWindowBuilder};

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
    /// 登录态只留在后端；前端永远拿不到 Cookie。
    session: Mutex<Option<NeteaseSession>>,
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

        let session_store = SessionStore::with_system_protector(paths.session_file());
        let (session, session_error) = match session_store.load() {
            Ok(session) => (session, None),
            Err(error) => (None, Some(error.to_string())),
        };

        Self {
            paths,
            settings: RwLock::new(settings),
            settings_warning,
            session: Mutex::new(session),
            session_error,
        }
    }

    fn settings(&self) -> RwLockReadGuard<'_, AppSettings> {
        self.settings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn session(&self) -> MutexGuard<'_, Option<NeteaseSession>> {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn to_dto(&self) -> AppStateDto {
        let settings = self.settings();
        let session = self.session();
        AppStateDto {
            data_dir: display_path(&self.paths.data_dir),
            is_portable: self.paths.is_portable,
            default_download_dir: display_path(&self.paths.default_download_dir),
            settings: SettingsSummaryDto {
                download_directory: settings.download_directory.clone(),
                library_directory: settings.library_directory.clone(),
                quality: settings.quality,
                theme: settings.theme,
                max_concurrency: settings.max_concurrency,
                match_threshold: settings.match_threshold,
            },
            settings_warning: self.settings_warning.clone(),
            session: SessionSummaryDto {
                logged_in: session.is_some(),
                nickname: session.as_ref().map(|s| s.display_name().to_owned()),
                load_error: self.session_error.clone(),
            },
        }
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

/// 登录态摘要：只有是否登录与昵称，**不含 Cookie**。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSummaryDto {
    logged_in: bool,
    nickname: Option<String>,
    load_error: Option<String>,
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

    tauri::Builder::default()
        .manage(state)
        .setup(move |app| {
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
        .invoke_handler(tauri::generate_handler![app_info, get_app_state])
        .run(tauri::generate_context!())
        .expect("启动 Lunatic Princess 失败");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(session: Option<NeteaseSession>, session_error: Option<&str>) -> AppState {
        let mut settings = AppSettings::new(r"D:\Music");
        settings.theme = ThemeMode::Light;
        AppState {
            paths: AppPaths {
                data_dir: r"D:\data".into(),
                default_download_dir: r"D:\Music".into(),
                is_portable: true,
            },
            settings: RwLock::new(settings),
            settings_warning: None,
            session: Mutex::new(session),
            session_error: session_error.map(str::to_owned),
        }
    }

    #[test]
    fn dto_reports_paths_and_settings_summary() {
        let json = serde_json::to_value(state_with(None, None).to_dto()).unwrap();
        assert_eq!(json["dataDir"], r"D:\data");
        assert_eq!(json["isPortable"], true);
        assert_eq!(json["settings"]["theme"], "Light");
        assert_eq!(json["settings"]["quality"], "ExHigh");
        assert_eq!(json["settings"]["maxConcurrency"], 3);
        assert_eq!(json["session"]["loggedIn"], false);
        assert!(json["session"]["nickname"].is_null());
    }

    #[test]
    fn dto_never_contains_the_cookie() {
        let session = NeteaseSession {
            cookie: "MUSIC_U=top-secret; __csrf=abc".into(),
            nickname: Some("测试".into()),
            ..Default::default()
        };
        let text = serde_json::to_string(&state_with(Some(session), None).to_dto()).unwrap();
        assert!(!text.contains("top-secret"));
        assert!(!text.contains("MUSIC_U"));
        assert!(text.contains("\"loggedIn\":true"));
        assert!(text.contains("测试"));
    }

    #[test]
    fn dto_carries_session_load_error() {
        let dto = state_with(None, Some("登录文件格式不正确或已损坏")).to_dto();
        assert!(!dto.session.logged_in);
        assert_eq!(
            dto.session.load_error.as_deref(),
            Some("登录文件格式不正确或已损坏")
        );
    }
}
