//! Lunatic Princess 的平台无关业务库。
//!
//! 与 C# 版 `LunaticPrincess.Core` 一一对应：这里不依赖任何界面框架，所有能力都必须可单测，
//! Tauri 外壳（`src-tauri`）只负责把这些能力转发给前端。
//! 各模块目前只是骨架，会随重写进度（见 `docs/TAURI_REWRITE.md`）逐步填充。

pub mod download;
pub mod error;
pub mod library;
pub mod lyrics;
pub mod ncm;
pub mod netease;
pub mod online;
pub mod paths;
pub mod session;
pub mod settings;
pub mod sync;
pub mod tags;

use serde::Serialize;

/// 应用的基础信息，用来证明前后端 IPC 通路可用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppInfo {
    /// 产品名称。
    pub name: &'static str,
    /// 版本号，取自 `Cargo.toml`。
    pub version: &'static str,
}

/// 返回应用名称与版本。
pub fn app_info() -> AppInfo {
    AppInfo {
        name: "Lunatic Princess",
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_reports_package_version() {
        let info = app_info();
        assert_eq!(info.name, "Lunatic Princess");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.version.is_empty());
    }
}
