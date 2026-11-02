//! Tauri 外壳：只做命令转发、窗口与插件，业务逻辑一律在 `lp-core`。

use serde::Serialize;
use tauri::window::{Effect, EffectsBuilder};
use tauri::{Manager, WebviewWindow};

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

/// 读取应用名称与版本，用来验证前后端通路。
#[tauri::command]
fn app_info() -> AppInfoDto {
    lp_core::app_info().into()
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
    tauri::Builder::default()
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                apply_window_effects(&window);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![app_info])
        .run(tauri::generate_context!())
        .expect("启动 Lunatic Princess 失败");
}
