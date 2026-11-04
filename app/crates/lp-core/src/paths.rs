//! 数据目录与默认路径解析（便携模式、`%LOCALAPPDATA%`、旧目录迁移）。
//!
//! 对应 C# 的 `Core/AppPaths.cs`，解析顺序与降级规则保持一致。
//!
//! 与 C# 版不同的是：C# 在静态字段里直接读环境变量与系统文件夹，这里把这些输入收拢成
//! [`PathEnv`]，解析逻辑只依赖它，因此可以用临时目录完整地单测每个分支。
//!
//! 某些受限运行环境（企业策略 / 沙箱 / 精简用户配置）不允许进程写入 `%LOCALAPPDATA%`
//! 或“音乐”目录，所以每个候选目录都会先做一次真实写入探测，不可写就降级到下一个候选。
//! 便携模式一旦启用（程序目录下存在 `data` 目录）会保持稳定，避免同一份程序在不同启动方式下
//! 使用不同登录态。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{CoreError, CoreResult};

/// 应用目录名（`%LOCALAPPDATA%\LunaticPrincess`、`%MyMusic%\LunaticPrincess`）。
pub const APP_NAME: &str = "LunaticPrincess";
/// 更名前旧版本的目录名，用于自动迁移设置、登录态与默认下载目录。
pub const LEGACY_APP_NAME: &str = "NeteaseMusicManager";
/// 可通过该环境变量强制指定数据目录（测试 / 便携部署）。
pub const DATA_DIR_ENV: &str = "LUNATIC_PRINCESS_DATA_DIR";
/// 更名前的同名环境变量，保留兼容（新变量未设置或为空白时仍然生效）。
pub const LEGACY_DATA_DIR_ENV: &str = "NETEASE_MUSIC_MANAGER_DATA_DIR";

/// 目录解析所需的全部外部输入。
///
/// 生产环境用 [`PathEnv::from_system`] 读取，测试里直接构造。
#[derive(Debug, Clone)]
pub struct PathEnv {
    /// `LUNATIC_PRINCESS_DATA_DIR` 的值。
    pub data_dir_env: Option<String>,
    /// `NETEASE_MUSIC_MANAGER_DATA_DIR` 的值。
    pub legacy_data_dir_env: Option<String>,
    /// 程序所在目录（C# 的 `AppContext.BaseDirectory`）。
    pub base_dir: PathBuf,
    /// `%LOCALAPPDATA%`（Windows 已知文件夹）。
    pub local_app_data: Option<PathBuf>,
    /// “音乐”已知文件夹，可能被用户重定向。
    pub my_music: Option<PathBuf>,
    /// 系统临时目录。
    pub temp_dir: PathBuf,
}

impl PathEnv {
    /// 从当前进程与系统读取真实输入。
    pub fn from_system() -> Self {
        let base_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        Self {
            data_dir_env: std::env::var(DATA_DIR_ENV).ok(),
            legacy_data_dir_env: std::env::var(LEGACY_DATA_DIR_ENV).ok(),
            base_dir,
            local_app_data: known_folders::local_app_data(),
            my_music: known_folders::music(),
            temp_dir: std::env::temp_dir(),
        }
    }
}

/// 解析后的应用路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// 数据目录（设置、登录态、日志、WebView 缓存都在这里）。
    pub data_dir: PathBuf,
    /// 默认下载 / 曲库目录。
    pub default_download_dir: PathBuf,
    /// 数据目录是否位于程序目录之下（便携模式）。
    pub is_portable: bool,
}

impl AppPaths {
    /// 按 C# `AppPaths` 的顺序解析数据目录与默认下载目录（会创建被选中的目录）。
    pub fn resolve(env: &PathEnv) -> CoreResult<Self> {
        let data_dir = resolve_data_dir(env)?;
        let is_portable = is_inside(&data_dir, &env.base_dir);
        let default_download_dir = resolve_download_dir(env, &data_dir, is_portable);
        Ok(Self {
            data_dir,
            default_download_dir,
            is_portable,
        })
    }

    /// `settings.json`。
    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    /// `session.dat`（DPAPI 加密的登录态）。
    pub fn session_file(&self) -> PathBuf {
        self.data_dir.join("session.dat")
    }

    /// 日志目录（仅致命异常落盘）。
    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /// WebView2 用户数据目录。放在数据目录下，避免依赖 Tauri 默认的 `%LOCALAPPDATA%\<identifier>`。
    pub fn webview_dir(&self) -> PathBuf {
        self.data_dir.join("webview")
    }
}

fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}

fn resolve_data_dir(env: &PathEnv) -> CoreResult<PathBuf> {
    // 1. 环境变量：新名优先，空白时才看旧名。
    let override_path = match env.data_dir_env.as_deref() {
        Some(value) if !is_blank(value) => Some(value),
        _ => env.legacy_data_dir_env.as_deref(),
    };
    if let Some(path) = override_path.filter(|value| !is_blank(value)) {
        let path = Path::new(path);
        if try_prepare_directory(path) {
            return Ok(std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()));
        }
    }

    // 2. 已经启用便携模式的安装：继续使用程序目录下的 data，
    //    这样无论通过资源管理器还是受限宿主启动，登录态 / 设置位置都保持一致。
    //    安装包和便携版 zip 都自带这个 data，所以数据默认跟着用户选的程序目录走。
    let portable = env.base_dir.join("data");
    if portable.is_dir() && try_prepare_directory(&portable) {
        return Ok(portable);
    }

    // 3. %LOCALAPPDATA%\LunaticPrincess；只有旧目录时先整体迁移。
    if let Some(local) = env
        .local_app_data
        .as_deref()
        .filter(|p| !p.as_os_str().is_empty())
    {
        let standard = local.join(APP_NAME);
        let legacy = local.join(LEGACY_APP_NAME);
        if !standard.exists() && legacy.exists() {
            // 旧目录可能被占用或没有权限：不阻断启动，继续探测新目录是否可写。
            let _ = fs::rename(&legacy, &standard);
        }
        if try_prepare_directory(&standard) {
            return Ok(standard);
        }
    }

    // 4. 程序目录下的 data（便携模式首次启用）。
    if try_prepare_directory(&portable) {
        return Ok(portable);
    }

    // 5. 临时目录兜底，创建失败就没有可用位置了。
    let fallback = env.temp_dir.join(APP_NAME);
    fs::create_dir_all(&fallback)
        .map_err(|error| CoreError::Path(format!("{}（{error}）", fallback.display())))?;
    Ok(fallback)
}

fn resolve_download_dir(env: &PathEnv, data_dir: &Path, is_portable: bool) -> PathBuf {
    // 便携模式下下载目录也跟随程序目录，避免换一种启动方式后默认路径不可写。
    if is_portable {
        let portable_music = data_dir.join("Music");
        if try_prepare_directory(&portable_music) {
            return portable_music;
        }
    }

    if let Some(music) = env
        .my_music
        .as_deref()
        .filter(|p| !p.as_os_str().is_empty())
    {
        let standard = music.join(APP_NAME);
        let legacy = music.join(LEGACY_APP_NAME);

        // 旧版默认下载目录里已有文件时沿用旧目录，避免用户已下载的内容被拆成两处。
        if !standard.exists() && legacy.exists() {
            return legacy;
        }
        if try_prepare_directory(&standard) {
            return standard;
        }
    }

    let portable_music = data_dir.join("Music");
    if try_prepare_directory(&portable_music) {
        return portable_music;
    }

    env.temp_dir.join(APP_NAME).join("Music")
}

/// `path` 是否位于 `base` 之下（按路径组件比较，忽略大小写，与 C# 的 `OrdinalIgnoreCase` 一致）。
fn is_inside(path: &Path, base: &Path) -> bool {
    let normalize = |p: &Path| -> Vec<String> {
        std::path::absolute(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .components()
            .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    };
    let path = normalize(path);
    let base = normalize(base);
    !base.is_empty() && path.len() >= base.len() && path[..base.len()] == base[..]
}

static PROBE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 尝试创建目录并做一次真实写入探测（某些容器对目录有特殊 ACL）。
fn try_prepare_directory(path: &Path) -> bool {
    if path.as_os_str().is_empty() || fs::create_dir_all(path).is_err() {
        return false;
    }
    let probe = path.join(format!(
        ".write-probe-{}-{}",
        std::process::id(),
        PROBE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .and_then(|mut file| file.write_all(&[0x42]));
    let _ = fs::remove_file(&probe);
    written.is_ok()
}

/// Windows 已知文件夹查询（`SHGetKnownFolderPath`，会跟随用户的文件夹重定向）。
#[cfg(windows)]
mod known_folders {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_LocalAppData, FOLDERID_Music, SHGetKnownFolderPath,
    };
    use windows_sys::core::GUID;

    fn known_folder(id: &GUID) -> Option<PathBuf> {
        let mut raw: *mut u16 = std::ptr::null_mut();
        // SAFETY: `id` 是有效的 GUID 引用；成功时 `raw` 指向以 NUL 结尾的宽字符串，
        // 由 `CoTaskMemFree` 释放（失败时同样需要释放，API 约定如此）。
        unsafe {
            let hr = SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut raw);
            let result = if hr >= 0 && !raw.is_null() {
                let mut len = 0;
                while *raw.add(len) != 0 {
                    len += 1;
                }
                Some(PathBuf::from(OsString::from_wide(
                    std::slice::from_raw_parts(raw, len),
                )))
            } else {
                None
            };
            if !raw.is_null() {
                CoTaskMemFree(raw as *const _);
            }
            result
        }
    }

    pub fn local_app_data() -> Option<PathBuf> {
        known_folder(&FOLDERID_LocalAppData)
    }

    pub fn music() -> Option<PathBuf> {
        known_folder(&FOLDERID_Music)
    }
}

/// 非 Windows 平台没有这些已知文件夹，调用方会自然降级到程序目录 / 临时目录。
#[cfg(not(windows))]
mod known_folders {
    use std::path::PathBuf;

    pub fn local_app_data() -> Option<PathBuf> {
        None
    }

    pub fn music() -> Option<PathBuf> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    /// 构造一个全部指向临时目录的环境，各候选目录互不干扰。
    fn env_in(root: &TempDir) -> PathEnv {
        PathEnv {
            data_dir_env: None,
            legacy_data_dir_env: None,
            base_dir: root.path().join("app"),
            local_app_data: Some(root.path().join("local")),
            my_music: Some(root.path().join("music")),
            temp_dir: root.path().join("temp"),
        }
    }

    /// 一个“是文件而不是目录”的路径：在它下面建目录必然失败，用来模拟不可写位置。
    fn blocked(root: &TempDir, name: &str) -> PathBuf {
        let path = root.path().join(name);
        fs::write(&path, b"x").unwrap();
        path
    }

    #[test]
    fn environment_variable_wins() {
        let root = TempDir::new();
        let mut env = env_in(&root);
        let custom = root.path().join("custom data");
        env.data_dir_env = Some(custom.to_string_lossy().into_owned());

        let paths = AppPaths::resolve(&env).unwrap();

        assert_eq!(paths.data_dir, custom);
        assert!(custom.is_dir());
        assert!(!root.path().join("local").exists());
    }

    #[test]
    fn legacy_environment_variable_is_used_when_new_one_is_blank() {
        let root = TempDir::new();
        let mut env = env_in(&root);
        let legacy = root.path().join("legacy-env");
        env.data_dir_env = Some("   ".to_owned());
        env.legacy_data_dir_env = Some(legacy.to_string_lossy().into_owned());

        assert_eq!(AppPaths::resolve(&env).unwrap().data_dir, legacy);

        env.data_dir_env = None;
        assert_eq!(AppPaths::resolve(&env).unwrap().data_dir, legacy);
    }

    #[test]
    fn new_environment_variable_beats_legacy_one() {
        let root = TempDir::new();
        let mut env = env_in(&root);
        let new = root.path().join("new-env");
        env.data_dir_env = Some(new.to_string_lossy().into_owned());
        env.legacy_data_dir_env = Some(root.path().join("old-env").to_string_lossy().into_owned());

        assert_eq!(AppPaths::resolve(&env).unwrap().data_dir, new);
    }

    #[test]
    fn unusable_environment_override_falls_through() {
        let root = TempDir::new();
        let mut env = env_in(&root);
        let file = blocked(&root, "not-a-dir");
        env.data_dir_env = Some(file.join("sub").to_string_lossy().into_owned());

        let paths = AppPaths::resolve(&env).unwrap();

        assert_eq!(paths.data_dir, root.path().join("local").join(APP_NAME));
    }

    #[test]
    fn existing_portable_data_dir_is_used_and_sticky() {
        let root = TempDir::new();
        let env = env_in(&root);
        let portable = root.path().join("app").join("data");
        fs::create_dir_all(&portable).unwrap();

        let paths = AppPaths::resolve(&env).unwrap();

        assert_eq!(paths.data_dir, portable);
        assert!(paths.is_portable);
        // 便携模式下不应再碰 %LOCALAPPDATA%。
        assert!(!root.path().join("local").exists());
        assert_eq!(paths.default_download_dir, portable.join("Music"));
    }

    #[test]
    fn standard_local_app_data_is_the_normal_case() {
        let root = TempDir::new();
        let env = env_in(&root);

        let paths = AppPaths::resolve(&env).unwrap();

        assert_eq!(paths.data_dir, root.path().join("local").join(APP_NAME));
        assert!(!paths.is_portable);
        assert!(paths.data_dir.is_dir());
        assert_eq!(
            paths.default_download_dir,
            root.path().join("music").join(APP_NAME)
        );
        assert!(paths.default_download_dir.is_dir());
        // 没有便携目录时不应凭空创建。
        assert!(!root.path().join("app").join("data").exists());
    }

    #[test]
    fn legacy_data_directory_is_migrated_wholesale() {
        let root = TempDir::new();
        let env = env_in(&root);
        let legacy = root.path().join("local").join(LEGACY_APP_NAME);
        fs::create_dir_all(legacy.join("logs")).unwrap();
        fs::write(legacy.join("settings.json"), b"{\"theme\":\"Light\"}").unwrap();
        fs::write(legacy.join("logs").join("desktop-fatal.log"), b"old").unwrap();

        let paths = AppPaths::resolve(&env).unwrap();

        let standard = root.path().join("local").join(APP_NAME);
        assert_eq!(paths.data_dir, standard);
        assert!(!legacy.exists(), "旧目录应已被整体移走");
        assert_eq!(
            fs::read(standard.join("settings.json")).unwrap(),
            b"{\"theme\":\"Light\"}"
        );
        assert_eq!(
            fs::read(standard.join("logs").join("desktop-fatal.log")).unwrap(),
            b"old"
        );
    }

    #[test]
    fn legacy_directory_is_left_alone_when_standard_exists() {
        let root = TempDir::new();
        let env = env_in(&root);
        let legacy = root.path().join("local").join(LEGACY_APP_NAME);
        let standard = root.path().join("local").join(APP_NAME);
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("settings.json"), b"legacy").unwrap();
        fs::create_dir_all(&standard).unwrap();

        let paths = AppPaths::resolve(&env).unwrap();

        assert_eq!(paths.data_dir, standard);
        assert!(legacy.join("settings.json").exists());
        assert!(!standard.join("settings.json").exists());
    }

    #[test]
    fn falls_back_to_portable_data_when_local_app_data_is_unwritable() {
        let root = TempDir::new();
        let mut env = env_in(&root);
        env.local_app_data = Some(blocked(&root, "local-file"));

        let paths = AppPaths::resolve(&env).unwrap();

        let portable = root.path().join("app").join("data");
        assert_eq!(paths.data_dir, portable);
        assert!(paths.is_portable);
    }

    #[test]
    fn missing_local_app_data_also_falls_back_to_portable() {
        let root = TempDir::new();
        let mut env = env_in(&root);
        env.local_app_data = None;

        let paths = AppPaths::resolve(&env).unwrap();

        assert_eq!(paths.data_dir, root.path().join("app").join("data"));
    }

    #[test]
    fn falls_back_to_temp_directory_as_last_resort() {
        let root = TempDir::new();
        let mut env = env_in(&root);
        env.local_app_data = Some(blocked(&root, "local-file"));
        env.base_dir = blocked(&root, "base-file");

        let paths = AppPaths::resolve(&env).unwrap();

        assert_eq!(paths.data_dir, root.path().join("temp").join(APP_NAME));
        assert!(!paths.is_portable);
    }

    #[test]
    fn resolve_fails_with_chinese_message_when_nothing_is_usable() {
        let root = TempDir::new();
        let mut env = env_in(&root);
        env.local_app_data = None;
        env.base_dir = blocked(&root, "base-file");
        env.temp_dir = blocked(&root, "temp-file");

        let error = AppPaths::resolve(&env).unwrap_err();

        assert!(error.to_string().starts_with("无法准备数据目录"));
    }

    #[test]
    fn download_directory_prefers_music_folder() {
        let root = TempDir::new();
        let env = env_in(&root);

        let paths = AppPaths::resolve(&env).unwrap();

        assert_eq!(
            paths.default_download_dir,
            root.path().join("music").join(APP_NAME)
        );
    }

    #[test]
    fn legacy_download_directory_is_reused_without_creating_the_new_one() {
        let root = TempDir::new();
        let env = env_in(&root);
        let legacy = root.path().join("music").join(LEGACY_APP_NAME);
        fs::create_dir_all(&legacy).unwrap();

        let paths = AppPaths::resolve(&env).unwrap();

        assert_eq!(paths.default_download_dir, legacy);
        assert!(!root.path().join("music").join(APP_NAME).exists());
    }

    #[test]
    fn existing_new_download_directory_beats_the_legacy_one() {
        let root = TempDir::new();
        let env = env_in(&root);
        fs::create_dir_all(root.path().join("music").join(LEGACY_APP_NAME)).unwrap();
        let standard = root.path().join("music").join(APP_NAME);
        fs::create_dir_all(&standard).unwrap();

        assert_eq!(
            AppPaths::resolve(&env).unwrap().default_download_dir,
            standard
        );
    }

    #[test]
    fn download_directory_falls_back_to_data_music_then_temp() {
        let root = TempDir::new();
        let mut env = env_in(&root);
        env.my_music = None;

        let paths = AppPaths::resolve(&env).unwrap();
        assert_eq!(paths.default_download_dir, paths.data_dir.join("Music"));
        assert!(paths.default_download_dir.is_dir());

        // “音乐”目录不可写，且 data\Music 也建不出来：返回临时目录路径（不要求已创建）。
        let data = paths.data_dir.clone();
        fs::remove_dir(data.join("Music")).unwrap();
        fs::write(data.join("Music"), b"x").unwrap();
        env.my_music = Some(blocked(&root, "music-file"));
        let again = AppPaths::resolve(&env).unwrap();
        assert_eq!(
            again.default_download_dir,
            root.path().join("temp").join(APP_NAME).join("Music")
        );
    }

    #[test]
    fn derived_file_locations_live_under_the_data_directory() {
        let paths = AppPaths {
            data_dir: PathBuf::from("D:\\data"),
            default_download_dir: PathBuf::from("D:\\data\\Music"),
            is_portable: false,
        };
        assert_eq!(
            paths.settings_file(),
            PathBuf::from("D:\\data").join("settings.json")
        );
        assert_eq!(
            paths.session_file(),
            PathBuf::from("D:\\data").join("session.dat")
        );
        assert_eq!(paths.log_dir(), PathBuf::from("D:\\data").join("logs"));
        assert_eq!(
            paths.webview_dir(),
            PathBuf::from("D:\\data").join("webview")
        );
    }

    #[test]
    fn is_inside_compares_components_case_insensitively() {
        let root = TempDir::new();
        let base = root.path().join("App");
        let inside = root.path().join("app").join("data");
        assert!(is_inside(&inside, &base));
        assert!(is_inside(&base, &base));
        // 仅前缀字符串相同的兄弟目录不算“在其下”。
        assert!(!is_inside(&root.path().join("App2").join("data"), &base));
        assert!(!is_inside(&root.path().join("elsewhere"), &base));
    }

    #[test]
    fn write_probe_leaves_no_files_behind() {
        let root = TempDir::new();
        let dir = root.path().join("probe");
        assert!(try_prepare_directory(&dir));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        assert!(!try_prepare_directory(Path::new("")));
    }

    #[cfg(windows)]
    #[test]
    fn system_environment_reports_real_known_folders() {
        let env = PathEnv::from_system();
        assert!(env.local_app_data.as_deref().is_some_and(Path::is_absolute));
        assert!(env.base_dir.is_absolute());
        assert!(env.temp_dir.is_absolute());
        // “音乐”文件夹在极少数精简系统上可能不存在，这里只要求给出时是绝对路径。
        if let Some(music) = env.my_music.as_deref() {
            assert!(music.is_absolute());
        }
    }
}
