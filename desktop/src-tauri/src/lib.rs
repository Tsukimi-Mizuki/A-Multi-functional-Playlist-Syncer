use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder};

#[derive(Default)]
struct ServerState {
    base_url: Mutex<Option<String>>,
    child: Mutex<Option<Child>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ServerInfo {
    base_url: Option<String>,
    managed: bool,
}

#[tauri::command]
fn server_info(state: State<'_, ServerState>) -> ServerInfo {
    let url = state.base_url.lock().ok().and_then(|slot| slot.clone());
    ServerInfo {
        managed: url.is_some(),
        base_url: url.or_else(|| std::env::var("LPP_API_BASE").ok()),
    }
}

/// Frontend calls this once at boot: waits until the sidecar answers, at most ~45s.
#[tauri::command]
async fn wait_server(state: State<'_, ServerState>) -> Result<Option<String>, String> {
    for _ in 0..180 {
        let url = state.base_url.lock().map_err(|e| e.to_string())?.clone();
        if url.is_some() {
            return Ok(url);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(state
        .base_url
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .or_else(|| std::env::var("LPP_API_BASE").ok()))
}

/// WebView2 的用户数据目录必须可写；开发机上默认的 %LOCALAPPDATA% 可能不可写，
/// 因此允许用环境变量 LPP_WEBVIEW_DATA_DIR 指定一个可写目录。
fn webview_data_dir() -> Option<PathBuf> {
    let value = std::env::var("LPP_WEBVIEW_DATA_DIR").ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn build_window(app: &tauri::App) -> tauri::Result<()> {
    let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Lunatic Princess")
        .inner_size(1320.0, 860.0)
        .min_inner_size(1040.0, 680.0)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .center()
        .drag_and_drop(false)
        .visible(true);

    if let Some(dir) = webview_data_dir() {
        let _ = std::fs::create_dir_all(&dir);
        eprintln!("[lpp] webview data dir: {}", dir.display());
        builder = builder.data_directory(dir);
    }

    builder.build()?;
    Ok(())
}

/// Looks for the bundled .NET sidecar next to the executable or in the dev layout.
fn resolve_server_exe() -> Option<PathBuf> {
    if let Ok(value) = std::env::var("LPP_SERVER_EXE") {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Some(path);
        }
    }

    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    let names = ["lunatic-princess-server.exe", "LunaticPrincess.Server.exe"];
    let candidates = [
        dir.clone(),
        dir.join("binaries"),
        dir.join("server"),
        dir.join("../../../server"),
        dir.join("../../../../.tmp/server"),
    ];
    for candidate in candidates {
        for name in names {
            let path = candidate.join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// 让系统分配一个空闲端口，避免与本机其他服务（例如 18400 已被占用）冲突。
fn pick_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .unwrap_or(18400)
}

fn wait_until_ready(port: u16) -> bool {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    for _ in 0..240 {
        if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

fn publish_base_url(app: &tauri::AppHandle, base_url: &str) {
    if let Some(state) = app.try_state::<ServerState>() {
        if let Ok(mut slot) = state.base_url.lock() {
            *slot = Some(base_url.to_string());
        }
    }
    if let Some(window) = app.get_webview_window("main") {
        let script = format!(
            "window.__LPP_API_BASE__ = {}; window.dispatchEvent(new Event('lpp-api-ready'));",
            serde_json::to_string(base_url).unwrap_or_else(|_| "null".into())
        );
        let _ = window.eval(&script);
    }
    eprintln!("[lpp] sidecar ready at {base_url}");
}

fn spawn_server(app: tauri::AppHandle) {
    let Some(exe) = resolve_server_exe() else {
        eprintln!("[lpp] sidecar not found; expecting LPP_API_BASE to be set");
        if let Ok(value) = std::env::var("LPP_API_BASE") {
            publish_base_url(&app, &value);
        }
        return;
    };

    let port = pick_port();
    let base_url = format!("http://127.0.0.1:{port}");

    let mut command = Command::new(&exe);
    command
        .arg("--urls")
        .arg(&base_url)
        .env("DOTNET_NOLOGO", "1")
        .env("LPP_SIDECAR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    match command.spawn() {
        Ok(mut child) => {
            let stdout = child.stdout.take();
            if let Some(state) = app.try_state::<ServerState>() {
                if let Ok(mut slot) = state.child.lock() {
                    *slot = Some(child);
                }
            }

            let handle = app.clone();
            let expected = base_url.clone();
            std::thread::spawn(move || {
                // 优先读 sidecar 打印的 LPP_SERVER_READY 行，拿不到就退回端口探测。
                let mut announced = false;
                if let Some(stream) = stdout {
                    let reader = BufReader::new(stream);
                    for line in reader.lines() {
                        let Ok(line) = line else { break };
                        if let Some(rest) = line.trim().strip_prefix("LPP_SERVER_READY") {
                            let url = rest.trim();
                            if !url.is_empty() {
                                publish_base_url(&handle, url);
                                announced = true;
                                break;
                            }
                        }
                    }
                }
                if !announced && wait_until_ready(port) {
                    publish_base_url(&handle, &expected);
                } else if !announced {
                    eprintln!("[lpp] sidecar did not become ready at {expected}");
                }
            });
        }
        Err(error) => eprintln!("[lpp] failed to spawn sidecar: {error}"),
    }
}

fn stop_sidecar(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<ServerState>() {
        if let Ok(mut slot) = state.child.lock() {
            if let Some(mut child) = slot.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(ServerState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![server_info, wait_server])
        .setup(|app| {
            build_window(app)?;
            spawn_server(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Lunatic Princess");

    app.run(|handle, event| {
        if let RunEvent::ExitRequested { .. } | RunEvent::Exit = event {
            stop_sidecar(handle);
        }
    });
}
