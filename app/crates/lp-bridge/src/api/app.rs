//! 应用级：启动装配、应用状态摘要、运行日志与账号变化的推送、退出前的收尾。

use lp_core::auth::AccountSummary;
use lp_core::logs::{LogEntry, LogLevel};

use crate::frb_generated::StreamSink;
use crate::state::{bridge, run};

/// flutter_rust_bridge 的初始化钩子（Dart 端 `RustLib.init()` 时调用）。
#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}

/// 应用状态摘要（启动后界面第一次要用的东西）。
pub struct AppStateDto {
    /// 版本号（取自 Cargo 工作区）。
    pub version: String,
    /// 数据目录（设置、登录态所在）。
    pub data_dir: String,
    /// 是否便携模式（数据在程序目录的 `data\` 下）。
    pub is_portable: bool,
    /// 当前账号。
    pub account: AccountDto,
    /// 登录文件读取 / 解密失败时的中文原因。
    pub session_error: Option<String>,
    /// 设置文件读取失败时的中文原因。
    pub settings_warning: Option<String>,
    /// 下载目录。
    pub download_directory: String,
}

/// 账号摘要（不含任何 Cookie）。
#[derive(Clone)]
pub struct AccountDto {
    pub logged_in: bool,
    /// 昵称；已登录时一定有值。
    pub nickname: Option<String>,
    pub user_id: i64,
    /// 会员类型，0 为非会员。
    pub vip_type: i32,
    /// 头像地址（https）。
    pub avatar_url: Option<String>,
}

impl From<AccountSummary> for AccountDto {
    fn from(account: AccountSummary) -> Self {
        Self {
            logged_in: account.logged_in,
            nickname: account.nickname,
            user_id: account.user_id,
            vip_type: account.vip_type,
            avatar_url: account.avatar_url,
        }
    }
}

/// 一条运行日志。
#[derive(Clone)]
pub struct LogEntryDto {
    /// 单调递增的编号（界面按它去重）。
    pub id: u64,
    /// `info` / `success` / `warning` / `error`。
    pub level: String,
    pub message: String,
    /// 产生时间（Unix 毫秒）。
    pub timestamp_ms: i64,
}

impl From<&LogEntry> for LogEntryDto {
    fn from(entry: &LogEntry) -> Self {
        let level = match entry.level {
            LogLevel::Info => "info",
            LogLevel::Success => "success",
            LogLevel::Warning => "warning",
            LogLevel::Error => "error",
        };
        Self {
            id: entry.id,
            level: level.to_owned(),
            message: entry.message.clone(),
            timestamp_ms: entry.timestamp_ms,
        }
    }
}

/// 装配（第一次调用时）并返回应用状态摘要。
pub fn load_app() -> Result<AppStateDto, String> {
    let bridge = bridge()?;
    Ok(AppStateDto {
        version: lp_core::app_info().version.to_owned(),
        data_dir: bridge.paths.data_dir.display().to_string(),
        is_portable: bridge.paths.is_portable,
        account: bridge.login.account().into(),
        session_error: bridge.session_error.clone(),
        settings_warning: bridge.settings_warning(),
        download_directory: bridge.settings().download_directory,
    })
}

/// 当前保留的运行日志（最多 400 条），界面启动时用它补齐订阅之前的日志。
pub fn recent_logs() -> Result<Vec<LogEntryDto>, String> {
    Ok(bridge()?
        .logs
        .recent()
        .iter()
        .map(LogEntryDto::from)
        .collect())
}

/// 订阅新的运行日志（只保留最后一个订阅者）。
pub fn watch_logs(sink: StreamSink<LogEntryDto>) -> Result<(), String> {
    bridge()?.logs.set_listener(move |entry| {
        // Dart 那边的订阅已经关掉时发送会失败，忽略即可。
        let _ = sink.add(LogEntryDto::from(entry));
    });
    Ok(())
}

/// 订阅账号变化：登录成功、退出、登录失效、资料刷新（只保留最后一个订阅者）。
pub fn watch_account(sink: StreamSink<AccountDto>) -> Result<(), String> {
    bridge()?.login.set_account_listener(move |account| {
        let _ = sink.add(AccountDto::from(account.clone()));
    });
    Ok(())
}

/// 退出前的收尾：停掉定时续期，等排队中的登录态落盘写完（否则可能丢掉刚轮换的 Cookie）。
pub async fn shutdown() -> Result<(), String> {
    let login = std::sync::Arc::clone(&bridge()?.login);
    run(async move { login.shutdown().await }).await
}
