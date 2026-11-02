//! 登录编排：启动恢复与校验、登录续期、扫码登录落盘、退出登录、运行期「未登录」复核。
//!
//! C# 版把这些逻辑放在 `MainWindowViewModel` 与 `QrLoginViewModel` 里，这里下沉到 core，
//! 外壳只做命令转发与事件推送，因此全部可以用假传输 + 假存储单测。
//!
//! 关键不变量（与 C# v1.3 的“登录保持”修复一致）：
//!
//! - **「登录失效」只认接口 `code == 301`**，或 `account/get` 返回 200 但 account / profile 为空；
//!   HTTP 401 / 302 / 5xx、406、-460、超时一律只提示“暂时无法验证”，不清除登录态。
//! - 判定失效前先 `refresh_login` 续期再复核一次，仍明确未登录才清除。
//! - 清除登录态的顺序固定：先置空内存会话（在途请求的 Set-Cookie 不再合并）、再排空自动落盘、最后删文件，
//!   否则已排队的落盘会在删除之后把 `session.dat` 写回去。
//! - 运行期 301 复核同一时间只做一次，避免批量请求同时报 301 时并发打爆限流。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, Weak};
use std::time::Duration;

use serde::Serialize;
use tokio::task::JoinHandle;

use crate::logs::LogHub;
use crate::netease::{NeteaseClient, NeteaseError};
use crate::qr;
use crate::session::{NeteaseSession, SessionAutoSaver, SessionStorage};

/// 运行期间续期登录态的默认间隔（约 12 小时）。
pub const DEFAULT_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);

/// 可以交给前端显示的账号摘要。**不含 Cookie**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    /// 是否已登录。
    pub logged_in: bool,
    /// 昵称（已登录时一定有值，缺失时显示“网易云用户”）。
    pub nickname: Option<String>,
    /// 用户 ID，未知时为 0。
    pub user_id: i64,
    /// 会员类型，0 为非会员。
    pub vip_type: i32,
    /// 头像地址（网易云返回 `http://` 时已升级为 `https://`，WebView 的 CSP 不允许混合内容）。
    pub avatar_url: Option<String>,
}

impl AccountSummary {
    /// 未登录。
    pub fn logged_out() -> Self {
        Self {
            logged_in: false,
            nickname: None,
            user_id: 0,
            vip_type: 0,
            avatar_url: None,
        }
    }

    /// 从登录态生成摘要。
    pub fn from_session(session: Option<&NeteaseSession>) -> Self {
        match session.filter(|s| s.is_logged_in()) {
            None => Self::logged_out(),
            Some(session) => Self {
                logged_in: true,
                nickname: Some(session.display_name().to_owned()),
                user_id: session.user_id,
                vip_type: session.vip_type,
                avatar_url: session
                    .avatar_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|url| !url.is_empty())
                    .map(|url| match url.strip_prefix("http://") {
                        Some(rest) => format!("https://{rest}"),
                        None => url.to_owned(),
                    }),
            },
        }
    }
}

/// [`LoginManager::restore`] 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreOutcome {
    /// 恢复出的账号（可能是未登录）。
    pub account: AccountSummary,
    /// 登录文件损坏 / 无法解密时的中文原因。
    pub load_failure: Option<String>,
}

/// 启动校验的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationOutcome {
    /// 本来就没有登录态。
    NotLoggedIn,
    /// 校验通过（已尝试续期）。
    Valid,
    /// 暂时无法验证（网络、HTTP、风控、限流），登录态已保留。
    Unverified {
        /// 面向用户的说明。
        message: String,
    },
    /// 接口明确说未登录，续期后复核仍未登录，已清除登录态。
    LoggedOut,
}

/// 运行期 301 复核的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeAuthOutcome {
    /// 已经有一次复核在进行，本次直接跳过。
    Skipped,
    /// 复核后仍然登录有效，或暂时无法确认（登录态保留）。
    Kept,
    /// 复核后确认已失效，登录态已清除。
    LoggedOut,
}

/// 申请到的登录二维码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrStartInfo {
    /// 轮询用的 key。
    pub key: String,
    /// 二维码 SVG（白底黑码）。
    pub svg: String,
    /// 二维码有效期（秒）。
    pub lifetime_secs: u64,
}

/// 一次扫码轮询对界面的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrPollOutcome {
    /// 等待扫码。
    Waiting,
    /// 已扫码，等待手机端确认。
    Scanned,
    /// 二维码已过期。
    Expired,
    /// 失败（服务端拒绝或保存登录态失败）。
    Failed {
        /// 面向用户的说明。
        message: String,
    },
    /// 登录成功，登录态已保存并生效。
    Success {
        /// 登录后的账号。
        account: AccountSummary,
    },
}

/// 构造选项。
#[derive(Debug, Clone, Copy)]
pub struct LoginOptions {
    /// 运行期续期间隔；测试里缩短它。
    pub keepalive_interval: Duration,
}

impl Default for LoginOptions {
    fn default() -> Self {
        Self {
            keepalive_interval: DEFAULT_KEEPALIVE_INTERVAL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginCheck {
    LoggedIn,
    LoggedOut,
    Unknown,
}

type AccountListener = Arc<dyn Fn(&AccountSummary) + Send + Sync>;

/// 复核标志的 RAII 复位，保证任何退出路径（含 panic / 被取消）都会放开下一次复核。
struct RecheckGuard<'a>(&'a AtomicBool);

impl Drop for RecheckGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// 登录编排器。
pub struct LoginManager {
    client: Arc<NeteaseClient>,
    store: Arc<dyn SessionStorage>,
    logs: Arc<LogHub>,
    saver: SessionAutoSaver,
    account_listener: RwLock<Option<AccountListener>>,
    recheck_running: AtomicBool,
    keepalive: Mutex<Option<JoinHandle<()>>>,
    keepalive_interval: Duration,
}

impl std::fmt::Debug for LoginManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginManager")
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

impl LoginManager {
    /// 创建编排器，并开始监听客户端的 Cookie 续期以自动落盘。
    pub fn new(
        client: Arc<NeteaseClient>,
        store: Arc<dyn SessionStorage>,
        logs: Arc<LogHub>,
        options: LoginOptions,
    ) -> Arc<Self> {
        let error_logs = Arc::clone(&logs);
        let saver = SessionAutoSaver::new(
            &client,
            Arc::clone(&store),
            Some(Box::new(move |error| {
                error_logs.warning(format!("保存续期后的登录状态失败：{error}"));
            })),
        );
        Arc::new(Self {
            client,
            store,
            logs,
            saver,
            account_listener: RwLock::new(None),
            recheck_running: AtomicBool::new(false),
            keepalive: Mutex::new(None),
            keepalive_interval: options.keepalive_interval,
        })
    }

    /// 底层客户端（后续模块的歌单 / 下载等请求共用同一份会话）。
    pub fn client(&self) -> &Arc<NeteaseClient> {
        &self.client
    }

    /// 设置（替换）账号变化监听器：登录成功、退出、资料刷新时调用。
    pub fn set_account_listener(&self, listener: impl Fn(&AccountSummary) + Send + Sync + 'static) {
        *self
            .account_listener
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(listener));
    }

    /// 当前账号摘要。
    pub fn account(&self) -> AccountSummary {
        AccountSummary::from_session(self.client.session().as_ref())
    }

    fn emit_account(&self) {
        let listener = self
            .account_listener
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(listener) = listener {
            listener(&self.account());
        }
    }

    /// 读取已保存的登录态并交给客户端（同步，不联网）。联网校验见 [`validate`](Self::validate)。
    pub fn restore(&self) -> RestoreOutcome {
        match self.store.load() {
            Ok(Some(session)) => {
                self.logs
                    .success(format!("已恢复登录状态：{}", session.display_name()));
                self.client.set_session(Some(session));
                RestoreOutcome {
                    account: self.account(),
                    load_failure: None,
                }
            }
            Ok(None) => {
                self.logs.info("尚未登录，点击右上角「扫码登录」开始");
                RestoreOutcome {
                    account: AccountSummary::logged_out(),
                    load_failure: None,
                }
            }
            Err(error) => {
                let reason = error.to_string();
                self.logs.warning(format!(
                    "读取已保存的登录状态失败：{reason}。请重新扫码登录"
                ));
                RestoreOutcome {
                    account: AccountSummary::logged_out(),
                    load_failure: Some(reason),
                }
            }
        }
    }

    // ===== 启动校验 / 续期 =====

    /// 启动后的联网校验：读取账号 → 成功后续期一次。
    ///
    /// - 网络 / HTTP / 风控 / 限流错误：保留登录，返回 [`ValidationOutcome::Unverified`]；
    /// - 明确 301（或账号为空）：先续期再复核，仍未登录才清除登录态。
    pub async fn validate(self: &Arc<Self>) -> ValidationOutcome {
        if !self.client.is_logged_in() {
            return ValidationOutcome::NotLoggedIn;
        }
        self.ensure_keepalive();

        match self.fetch_account().await {
            Ok(()) => {
                self.logs.success("账号信息同步完成");
                self.emit_account();
                self.renew().await;
                ValidationOutcome::Valid
            }
            Err(error) if error.is_not_logged_in() => self.handle_startup_auth_error().await,
            Err(error) => {
                // 限流、风控、HTTP 异常、超时等都不代表登录失效：保留登录，等网络恢复后再用。
                let message = error.user_message();
                self.logs.warning(format!(
                    "暂时无法验证登录状态，已保留登录并进入离线模式：{message}"
                ));
                ValidationOutcome::Unverified {
                    message: format!("暂时无法验证登录状态（已保留登录）：{message}"),
                }
            }
        }
    }

    /// 读取账号并更新内存会话；账号为空按“服务端明确认为未登录”处理（等同 301）。
    async fn fetch_account(&self) -> Result<(), NeteaseError> {
        match self.client.get_current_user().await? {
            Some(user) if user.user_id > 0 => Ok(()),
            // 接口 HTTP 200 却没有 account / profile：服务端明确认为当前未登录。
            _ => Err(NeteaseError::api("接口未返回账号信息，登录状态已失效", 301)),
        }
    }

    async fn handle_startup_auth_error(&self) -> ValidationOutcome {
        match self.recheck().await {
            LoginCheck::LoggedOut => {
                self.clear_login_state("登录状态已失效，请重新扫码登录")
                    .await;
                ValidationOutcome::LoggedOut
            }
            LoginCheck::LoggedIn => {
                self.logs.success("登录态已续期，账号信息同步完成");
                self.emit_account();
                ValidationOutcome::Valid
            }
            LoginCheck::Unknown => {
                self.logs.warning("暂时无法验证登录状态，保留登录");
                ValidationOutcome::Unverified {
                    message: "暂时无法验证登录状态（已保留登录），请稍后重试".to_owned(),
                }
            }
        }
    }

    /// 先请求续期，再重新读取账号，判断登录是否真的失效。
    async fn recheck(&self) -> LoginCheck {
        self.client.refresh_login().await;
        match self.client.get_current_user().await {
            Ok(Some(user)) if user.user_id > 0 => LoginCheck::LoggedIn,
            Ok(_) => LoginCheck::LoggedOut,
            Err(error) if error.is_not_logged_in() => LoginCheck::LoggedOut,
            Err(_) => LoginCheck::Unknown,
        }
    }

    /// 请求服务端续期登录态；失败只记日志，不打扰用户。
    pub async fn renew(&self) {
        match self.client.try_refresh_login().await {
            Ok(()) => self.logs.success("登录态已续期"),
            Err(error) => self.logs.warning(format!(
                "登录态续期未成功（{}），将继续使用当前登录状态",
                error.message()
            )),
        }
    }

    /// 运行期间遇到接口返回「未登录」(301)：先续期并复核，仍明确未登录才清除登录态。
    ///
    /// 批量下载等场景可能同时报出多条 301：同一时间只复核一次，其余调用直接返回
    /// [`RuntimeAuthOutcome::Skipped`]，避免并发请求触发网易云限流。
    pub async fn handle_runtime_auth_error(&self) -> RuntimeAuthOutcome {
        if self
            .recheck_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return RuntimeAuthOutcome::Skipped;
        }
        let check = {
            let _guard = RecheckGuard(&self.recheck_running);
            self.recheck().await
        };

        if check == LoginCheck::LoggedOut {
            self.clear_login_state("登录状态已失效，请重新扫码登录")
                .await;
            RuntimeAuthOutcome::LoggedOut
        } else {
            RuntimeAuthOutcome::Kept
        }
    }

    // ===== 定时续期 =====

    /// 启动运行期续期任务（幂等）：每隔 `keepalive_interval` 在已登录时续期一次。
    /// 必须在 tokio 运行时中调用。
    pub fn ensure_keepalive(self: &Arc<Self>) {
        let mut slot = self.keepalive_slot();
        if slot.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return;
        }
        let weak: Weak<Self> = Arc::downgrade(self);
        let interval = self.keepalive_interval;
        *slot = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                let Some(manager) = weak.upgrade() else {
                    break;
                };
                if manager.client.is_logged_in() {
                    manager.renew().await;
                }
            }
        }));
    }

    fn keepalive_slot(&self) -> MutexGuard<'_, Option<JoinHandle<()>>> {
        self.keepalive
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 停止定时续期。
    pub fn stop_keepalive(&self) {
        if let Some(handle) = self.keepalive_slot().take() {
            handle.abort();
        }
    }

    // ===== 扫码登录 =====

    /// 申请登录二维码并渲染成 SVG。
    pub async fn qr_start(&self) -> Result<QrStartInfo, NeteaseError> {
        let start = self.client.create_qr_login().await?;
        let svg = qr::render_svg(&start.qr_content).map_err(NeteaseError::protocol)?;
        Ok(QrStartInfo {
            key: start.key,
            svg,
            lifetime_secs: start.lifetime.as_secs(),
        })
    }

    /// 轮询一次扫码状态；确认登录时先保存登录态再让它生效。
    pub async fn qr_poll(self: &Arc<Self>, key: &str) -> Result<QrPollOutcome, NeteaseError> {
        use crate::netease::QrLoginStatus;

        match self.client.poll_qr_login(key).await? {
            QrLoginStatus::Waiting => Ok(QrPollOutcome::Waiting),
            QrLoginStatus::Scanned => Ok(QrPollOutcome::Scanned),
            QrLoginStatus::Expired => Ok(QrPollOutcome::Expired),
            QrLoginStatus::Failed { message } => Ok(QrPollOutcome::Failed { message }),
            QrLoginStatus::Confirmed { session, message } => {
                Ok(self.finish_qr_login(session, &message))
            }
        }
    }

    fn finish_qr_login(self: &Arc<Self>, session: NeteaseSession, message: &str) -> QrPollOutcome {
        if !session.is_logged_in() {
            self.client.set_session(None);
            return QrPollOutcome::Failed {
                message: "登录会话无效，请刷新二维码重试".to_owned(),
            };
        }

        // 先落盘再让登录态生效：保存失败时不能留下“内存已登录、重启就丢”的半截状态。
        if let Err(error) = self.store.save(&session) {
            self.client.set_session(None);
            let message = format!("保存登录状态失败：{error}");
            self.logs.error(&message);
            return QrPollOutcome::Failed { message };
        }
        self.client.set_session(Some(session.clone()));
        self.logs
            .success(format!("登录成功：{}", session.display_name()));
        self.ensure_keepalive();

        if session.user_id <= 0 {
            // 803 已经拿到 Cookie 但当时没读到账号资料：后台补一次，补上后刷新界面。
            self.logs.warning(message);
            let manager = Arc::clone(self);
            tokio::spawn(async move {
                if manager.fetch_account().await.is_ok() {
                    manager.emit_account();
                }
            });
        }
        self.emit_account();
        QrPollOutcome::Success {
            account: self.account(),
        }
    }

    // ===== 退出 / 清除 =====

    /// 退出登录：清内存会话 → 排空自动落盘 → 删除 `session.dat`。
    pub async fn logout(&self) {
        self.clear_login_state("已退出登录").await;
    }

    async fn clear_login_state(&self, message: &str) {
        // 先切断内存登录态（在途请求的 Set-Cookie 不再合并），再等自动落盘排空，最后删文件；
        // 顺序反过来会让已排队的落盘在删除后把 session.dat 写回去。
        self.client.set_session(None);
        self.saver.flush().await;
        if let Err(error) = self.store.clear() {
            self.logs.warning(format!("清除本地登录状态失败：{error}"));
        }
        self.emit_account();
        self.logs.info(message);
    }

    /// 应用退出前调用：停止续期任务，等已排队的落盘写完，并停止监听。
    pub async fn shutdown(&self) {
        self.stop_keepalive();
        self.saver.flush().await;
        self.saver.dispose();
    }
}

impl Drop for LoginManager {
    fn drop(&mut self) {
        if let Some(handle) = self.keepalive_slot().take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use tokio::sync::Notify;

    use super::*;
    use crate::logs::LogLevel;
    use crate::netease::http::TransportError;
    use crate::session::test_support::RecordingStore;
    use crate::test_util::{FakeTransport, test_client};

    const ACCOUNT_OK: &str = r#"{
        "code": 200,
        "account": { "id": 42, "userName": "u" },
        "profile": {
            "userId": 42,
            "nickname": "测试用户",
            "avatarUrl": "http://p1.music.126.net/a.jpg",
            "vipType": 1
        }
    }"#;
    const NOT_LOGGED_IN: &str = r#"{"code":301,"message":"需要登录"}"#;
    const ACCOUNT_NULL: &str = r#"{"code":200,"account":null,"profile":null}"#;
    const REFRESH_OK: &str = r#"{"code":200}"#;

    struct Fixture {
        transport: Arc<FakeTransport>,
        client: Arc<NeteaseClient>,
        store: Arc<RecordingStore>,
        logs: Arc<LogHub>,
        manager: Arc<LoginManager>,
    }

    fn saved_session() -> NeteaseSession {
        NeteaseSession {
            cookie: "MUSIC_U=old; __csrf=csrf1".to_owned(),
            nickname: Some("旧昵称".to_owned()),
            user_id: 42,
            ..Default::default()
        }
    }

    /// 带一份已保存登录态、续期间隔 100 秒的夹具；`restore()` 已执行。
    fn logged_in_fixture() -> Fixture {
        let fixture = fixture();
        *fixture.store.stored.lock().unwrap() = Some(saved_session());
        fixture.manager.restore();
        fixture
    }

    fn fixture() -> Fixture {
        let transport = FakeTransport::new();
        let client = Arc::new(test_client(&transport));
        let store = Arc::new(RecordingStore::default());
        let logs = LogHub::new();
        let manager = LoginManager::new(
            Arc::clone(&client),
            store.clone(),
            Arc::clone(&logs),
            LoginOptions {
                keepalive_interval: Duration::from_secs(100),
            },
        );
        Fixture {
            transport,
            client,
            store,
            logs,
            manager,
        }
    }

    fn has_log(logs: &LogHub, level: LogLevel, fragment: &str) -> bool {
        logs.recent()
            .iter()
            .any(|e| e.level == level && e.message.contains(fragment))
    }

    fn request_paths(transport: &FakeTransport) -> Vec<String> {
        transport
            .requests()
            .iter()
            .map(|r| r.url.replace("https://music.163.com", ""))
            .collect()
    }

    // ---- restore ----

    #[test]
    fn restore_applies_the_saved_session_without_network() {
        let f = fixture();
        *f.store.stored.lock().unwrap() = Some(saved_session());

        let outcome = f.manager.restore();

        assert!(outcome.account.logged_in);
        assert_eq!(outcome.account.nickname.as_deref(), Some("旧昵称"));
        assert!(outcome.load_failure.is_none());
        assert!(f.client.is_logged_in());
        assert!(has_log(
            &f.logs,
            LogLevel::Success,
            "已恢复登录状态：旧昵称"
        ));
        assert!(f.transport.requests().is_empty());
    }

    #[test]
    fn restore_without_a_saved_session_is_logged_out() {
        let f = fixture();

        let outcome = f.manager.restore();

        assert_eq!(outcome.account, AccountSummary::logged_out());
        assert!(!f.client.is_logged_in());
        assert!(has_log(&f.logs, LogLevel::Info, "尚未登录"));
    }

    #[test]
    fn restore_reports_an_unreadable_session_file() {
        let f = fixture();
        *f.store.load_error.lock().unwrap() = Some("登录文件格式不正确或已损坏".to_owned());

        let outcome = f.manager.restore();

        assert!(!outcome.account.logged_in);
        assert_eq!(
            outcome.load_failure.as_deref(),
            Some("登录文件格式不正确或已损坏")
        );
        assert!(has_log(
            &f.logs,
            LogLevel::Warning,
            "读取已保存的登录状态失败"
        ));
    }

    #[test]
    fn account_summary_never_exposes_the_cookie_and_upgrades_http_avatars() {
        let session = NeteaseSession {
            cookie: "MUSIC_U=top-secret".to_owned(),
            avatar_url: Some("http://p1.music.126.net/a.jpg".to_owned()),
            ..Default::default()
        };

        let summary = AccountSummary::from_session(Some(&session));
        let text = serde_json::to_string(&summary).unwrap();

        assert!(!text.contains("top-secret"));
        assert!(text.contains("\"loggedIn\":true"));
        assert_eq!(summary.nickname.as_deref(), Some("网易云用户"));
        assert_eq!(
            summary.avatar_url.as_deref(),
            Some("https://p1.music.126.net/a.jpg")
        );
    }

    // ---- validate ----

    #[tokio::test]
    async fn validate_success_syncs_account_then_renews_and_persists_rotated_cookie() {
        let f = logged_in_fixture();
        f.transport.enqueue_json(ACCOUNT_OK);
        f.transport
            .enqueue_json_with_cookies(REFRESH_OK, &["MUSIC_U=renewed; Max-Age=1296000; Path=/"]);
        let notified = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&notified);
        f.manager.set_account_listener(move |account| {
            assert!(account.logged_in);
            counter.fetch_add(1, Ordering::SeqCst);
        });

        let outcome = f.manager.validate().await;
        f.manager.shutdown().await;

        assert_eq!(outcome, ValidationOutcome::Valid);
        assert_eq!(
            request_paths(&f.transport),
            vec!["/weapi/nuser/account/get", "/weapi/login/token/refresh"]
        );
        assert!(has_log(&f.logs, LogLevel::Success, "账号信息同步完成"));
        assert!(has_log(&f.logs, LogLevel::Success, "登录态已续期"));
        assert_eq!(notified.load(Ordering::SeqCst), 1);
        let saved = f.store.saved.lock().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].cookie_value("MUSIC_U").as_deref(), Some("renewed"));
        assert_eq!(f.manager.account().nickname.as_deref(), Some("测试用户"));
        assert_eq!(
            f.manager.account().avatar_url.as_deref(),
            Some("https://p1.music.126.net/a.jpg")
        );
    }

    #[tokio::test]
    async fn validate_warns_when_renewal_is_rejected_but_stays_valid() {
        let f = logged_in_fixture();
        f.transport.enqueue_json(ACCOUNT_OK);
        f.transport.enqueue_json(r#"{"code":-460}"#);

        let outcome = f.manager.validate().await;

        assert_eq!(outcome, ValidationOutcome::Valid);
        // 日志要带上失败原因，方便判断是被拒绝还是网络问题。
        assert!(has_log(
            &f.logs,
            LogLevel::Warning,
            "登录态续期未成功（续期登录状态失败：接口返回 code=-460）"
        ));
    }

    #[tokio::test]
    async fn validate_without_a_session_does_nothing() {
        let f = fixture();

        assert_eq!(f.manager.validate().await, ValidationOutcome::NotLoggedIn);
        assert!(f.transport.requests().is_empty());
    }

    #[tokio::test]
    async fn validate_keeps_the_login_on_network_errors() {
        let f = logged_in_fixture();
        f.transport
            .enqueue_error(TransportError::Network("dns".to_owned()));

        let outcome = f.manager.validate().await;

        let ValidationOutcome::Unverified { message } = outcome else {
            panic!("应为 Unverified，实际 {outcome:?}");
        };
        assert!(message.contains("已保留登录"));
        assert!(f.client.is_logged_in());
        assert_eq!(*f.store.cleared.lock().unwrap(), 0);
        assert!(has_log(
            &f.logs,
            LogLevel::Warning,
            "暂时无法验证登录状态，已保留登录并进入离线模式"
        ));
        // 只发了一次账号请求，没有因此续期或复核。
        assert_eq!(f.transport.requests().len(), 1);
    }

    #[tokio::test]
    async fn validate_keeps_the_login_on_http_and_risk_control_errors() {
        for (status, body) in [
            (401, ""),
            (302, ""),
            (500, ""),
            (200, r#"{"code":-460}"#),
            (200, r#"{"code":406}"#),
        ] {
            let f = logged_in_fixture();
            f.transport.enqueue_raw(status, body);

            let outcome = f.manager.validate().await;

            assert!(
                matches!(outcome, ValidationOutcome::Unverified { .. }),
                "{status} {body}"
            );
            assert!(f.client.is_logged_in());
            assert_eq!(*f.store.cleared.lock().unwrap(), 0);
        }
    }

    #[tokio::test]
    async fn validate_recovers_when_301_is_followed_by_a_successful_renewal() {
        let f = logged_in_fixture();
        f.transport.enqueue_json(NOT_LOGGED_IN);
        f.transport
            .enqueue_json_with_cookies(REFRESH_OK, &["MUSIC_U=renewed; Path=/"]);
        f.transport.enqueue_json(ACCOUNT_OK);

        let outcome = f.manager.validate().await;
        f.manager.shutdown().await;

        assert_eq!(outcome, ValidationOutcome::Valid);
        assert_eq!(
            request_paths(&f.transport),
            vec![
                "/weapi/nuser/account/get",
                "/weapi/login/token/refresh",
                "/weapi/nuser/account/get"
            ]
        );
        assert!(f.client.is_logged_in());
        assert_eq!(*f.store.cleared.lock().unwrap(), 0);
        assert!(has_log(
            &f.logs,
            LogLevel::Success,
            "登录态已续期，账号信息同步完成"
        ));
        assert_eq!(f.store.saved.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn validate_clears_the_login_when_301_persists_after_renewal() {
        let f = logged_in_fixture();
        f.transport.enqueue_json(NOT_LOGGED_IN);
        f.transport.enqueue_json(REFRESH_OK);
        f.transport.enqueue_json(NOT_LOGGED_IN);
        let notified = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&notified);
        f.manager
            .set_account_listener(move |account| sink.lock().unwrap().push(account.logged_in));

        let outcome = f.manager.validate().await;

        assert_eq!(outcome, ValidationOutcome::LoggedOut);
        assert!(!f.client.is_logged_in());
        assert_eq!(*f.store.cleared.lock().unwrap(), 1);
        assert!(has_log(
            &f.logs,
            LogLevel::Info,
            "登录状态已失效，请重新扫码登录"
        ));
        assert_eq!(*notified.lock().unwrap(), vec![false]);
    }

    #[tokio::test]
    async fn validate_clears_the_login_when_the_account_is_empty_after_renewal() {
        let f = logged_in_fixture();
        f.transport.enqueue_json(NOT_LOGGED_IN);
        f.transport.enqueue_json(NOT_LOGGED_IN); // 续期被拒，返回 false 即可
        f.transport.enqueue_json(ACCOUNT_NULL);

        assert_eq!(f.manager.validate().await, ValidationOutcome::LoggedOut);
        assert_eq!(*f.store.cleared.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn validate_treats_an_empty_account_as_an_expired_login_and_rechecks() {
        let f = logged_in_fixture();
        f.transport.enqueue_json(ACCOUNT_NULL);
        f.transport.enqueue_json(REFRESH_OK);
        f.transport.enqueue_json(ACCOUNT_OK);

        assert_eq!(f.manager.validate().await, ValidationOutcome::Valid);
        assert!(f.client.is_logged_in());
        assert_eq!(*f.store.cleared.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn validate_keeps_the_login_when_the_recheck_itself_hits_a_network_error() {
        let f = logged_in_fixture();
        f.transport.enqueue_json(NOT_LOGGED_IN);
        f.transport.enqueue_error(TransportError::Timeout);
        f.transport
            .enqueue_error(TransportError::Network("断网".to_owned()));

        let outcome = f.manager.validate().await;

        assert!(matches!(outcome, ValidationOutcome::Unverified { .. }));
        assert!(f.client.is_logged_in());
        assert_eq!(*f.store.cleared.lock().unwrap(), 0);
        assert!(has_log(
            &f.logs,
            LogLevel::Warning,
            "暂时无法验证登录状态，保留登录"
        ));
    }

    // ---- logout ----

    #[tokio::test]
    async fn logout_cuts_the_memory_session_before_draining_saves_and_never_writes_back() {
        let f = logged_in_fixture();
        // 两次 Cookie 轮换：第一次写盘卡在闸门上，第二次排队等待。
        f.transport
            .enqueue_json_with_cookies(ACCOUNT_OK, &["MUSIC_U=r1; Path=/"]);
        f.transport
            .enqueue_json_with_cookies(ACCOUNT_OK, &["MUSIC_U=r2; Path=/"]);
        f.store.close_gate();
        f.client.get_current_user().await.unwrap();
        while !f.store.is_blocked() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        f.client.get_current_user().await.unwrap();

        let manager = Arc::clone(&f.manager);
        let logout = tokio::spawn(async move { manager.logout().await });
        while f.client.is_logged_in() {
            tokio::task::yield_now().await;
        }
        // 内存会话已经先被切断；此时文件还不能删（落盘还没排空）。
        assert_eq!(*f.store.cleared.lock().unwrap(), 0);
        f.store.open_gate();
        logout.await.unwrap();

        // 在途的第一次写入先完成，随后才删文件，排队的第二次写入因已退出而被跳过。
        assert_eq!(*f.store.operations.lock().unwrap(), vec!["save", "clear"]);
        assert!(!f.client.is_logged_in());
        assert!(has_log(&f.logs, LogLevel::Info, "已退出登录"));
    }

    #[tokio::test]
    async fn logout_notifies_listeners_and_clears_the_file() {
        let f = logged_in_fixture();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        f.manager
            .set_account_listener(move |account| sink.lock().unwrap().push(account.clone()));

        f.manager.logout().await;

        assert_eq!(*f.store.cleared.lock().unwrap(), 1);
        assert_eq!(*seen.lock().unwrap(), vec![AccountSummary::logged_out()]);
    }

    // ---- 运行期复核 ----

    #[tokio::test]
    async fn runtime_recheck_runs_once_at_a_time() {
        let f = logged_in_fixture();
        let gate = Arc::new(Notify::new());
        f.transport.enqueue_gated(REFRESH_OK, Arc::clone(&gate));
        f.transport.enqueue_json(ACCOUNT_OK);

        let manager = Arc::clone(&f.manager);
        let first = tokio::spawn(async move { manager.handle_runtime_auth_error().await });
        while f.transport.requests().is_empty() {
            tokio::task::yield_now().await;
        }

        // 第一次复核还在进行：其余调用立即跳过，且不发任何请求。
        assert_eq!(
            f.manager.handle_runtime_auth_error().await,
            RuntimeAuthOutcome::Skipped
        );
        assert_eq!(
            f.manager.handle_runtime_auth_error().await,
            RuntimeAuthOutcome::Skipped
        );
        assert_eq!(f.transport.requests().len(), 1);

        gate.notify_one();
        assert_eq!(first.await.unwrap(), RuntimeAuthOutcome::Kept);
        assert_eq!(f.transport.requests().len(), 2); // 一次续期 + 一次账号复核

        // 复核结束后可以再次触发。
        f.transport.enqueue_json(REFRESH_OK);
        f.transport.enqueue_json(ACCOUNT_OK);
        assert_eq!(
            f.manager.handle_runtime_auth_error().await,
            RuntimeAuthOutcome::Kept
        );
        assert_eq!(f.transport.requests().len(), 4);
    }

    #[tokio::test]
    async fn runtime_recheck_clears_the_login_only_when_still_unauthenticated() {
        let f = logged_in_fixture();
        f.transport.enqueue_json(REFRESH_OK);
        f.transport.enqueue_json(NOT_LOGGED_IN);

        assert_eq!(
            f.manager.handle_runtime_auth_error().await,
            RuntimeAuthOutcome::LoggedOut
        );
        assert!(!f.client.is_logged_in());
        assert_eq!(*f.store.cleared.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn runtime_recheck_keeps_the_login_when_renewal_fixes_it_or_when_unsure() {
        let f = logged_in_fixture();
        f.transport.enqueue_json(REFRESH_OK);
        f.transport.enqueue_json(ACCOUNT_OK);
        assert_eq!(
            f.manager.handle_runtime_auth_error().await,
            RuntimeAuthOutcome::Kept
        );

        f.transport.enqueue_error(TransportError::Timeout);
        f.transport.enqueue_raw(500, "");
        assert_eq!(
            f.manager.handle_runtime_auth_error().await,
            RuntimeAuthOutcome::Kept
        );
        assert!(f.client.is_logged_in());
        assert_eq!(*f.store.cleared.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn runtime_recheck_flag_is_released_if_the_recheck_is_cancelled() {
        let f = logged_in_fixture();
        f.transport
            .enqueue_gated(REFRESH_OK, Arc::new(Notify::new()));
        let manager = Arc::clone(&f.manager);
        let first = tokio::spawn(async move { manager.handle_runtime_auth_error().await });
        while f.transport.requests().is_empty() {
            tokio::task::yield_now().await;
        }

        first.abort();
        let _ = first.await;

        f.transport.enqueue_json(REFRESH_OK);
        f.transport.enqueue_json(ACCOUNT_OK);
        assert_eq!(
            f.manager.handle_runtime_auth_error().await,
            RuntimeAuthOutcome::Kept
        );
    }

    // ---- 定时续期 ----

    async fn let_tasks_run() {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn keepalive_renews_on_every_interval_and_stops_when_cancelled() {
        let f = logged_in_fixture();
        for _ in 0..3 {
            f.transport.enqueue_json(REFRESH_OK);
        }
        f.manager.ensure_keepalive();
        f.manager.ensure_keepalive(); // 幂等：不会起第二个任务
        let_tasks_run().await;
        assert!(f.transport.requests().is_empty(), "间隔未到不应续期");

        tokio::time::advance(Duration::from_secs(101)).await;
        let_tasks_run().await;
        assert_eq!(f.transport.requests().len(), 1);
        assert!(request_paths(&f.transport)[0].ends_with("/login/token/refresh"));
        assert!(has_log(&f.logs, LogLevel::Success, "登录态已续期"));

        tokio::time::advance(Duration::from_secs(100)).await;
        let_tasks_run().await;
        assert_eq!(f.transport.requests().len(), 2);

        f.manager.stop_keepalive();
        tokio::time::advance(Duration::from_secs(1000)).await;
        let_tasks_run().await;
        assert_eq!(f.transport.requests().len(), 2, "取消后不再续期");
        assert_eq!(f.transport.pending_replies(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn keepalive_skips_ticks_while_logged_out() {
        let f = logged_in_fixture();
        f.manager.ensure_keepalive();
        f.client.set_session(None);

        tokio::time::advance(Duration::from_secs(350)).await;
        let_tasks_run().await;

        assert!(f.transport.requests().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn keepalive_can_be_restarted_after_it_was_stopped() {
        let f = logged_in_fixture();
        f.transport.enqueue_json(REFRESH_OK);
        f.manager.ensure_keepalive();
        f.manager.stop_keepalive();
        f.manager.ensure_keepalive();
        let_tasks_run().await; // 先让新任务开始计时，再推进时间

        tokio::time::advance(Duration::from_secs(101)).await;
        let_tasks_run().await;

        assert_eq!(f.transport.requests().len(), 1);
    }

    #[tokio::test]
    async fn shutdown_drains_pending_saves() {
        let f = logged_in_fixture();
        f.transport
            .enqueue_json_with_cookies(ACCOUNT_OK, &["MUSIC_U=r1; Path=/"]);
        f.client.get_current_user().await.unwrap();

        f.manager.shutdown().await;

        assert_eq!(f.store.saved.lock().unwrap().len(), 1);
    }

    // ---- 扫码登录 ----

    #[tokio::test]
    async fn qr_start_returns_a_key_and_a_renderable_svg() {
        let f = fixture();
        f.transport
            .enqueue_json(r#"{"code":200,"unikey":"abcDEF123456"}"#);

        let start = f.manager.qr_start().await.unwrap();

        assert_eq!(start.key, "abcDEF123456");
        assert!(start.svg.contains("<svg"));
        assert_eq!(start.lifetime_secs, 180);
    }

    #[tokio::test]
    async fn qr_poll_maps_waiting_scanned_expired_and_failed() {
        let cases: [(&str, QrPollOutcome); 4] = [
            (r#"{"code":801}"#, QrPollOutcome::Waiting),
            (r#"{"code":802}"#, QrPollOutcome::Scanned),
            (r#"{"code":800}"#, QrPollOutcome::Expired),
            (
                r#"{"code":999,"message":"x"}"#,
                QrPollOutcome::Failed {
                    message: "扫码登录失败（code=999：x）".to_owned(),
                },
            ),
        ];
        for (body, expected) in cases {
            let f = fixture();
            f.transport.enqueue_json(body);
            assert_eq!(f.manager.qr_poll("k").await.unwrap(), expected, "{body}");
            assert!(!f.client.is_logged_in());
        }
    }

    #[tokio::test]
    async fn qr_poll_confirmed_saves_the_session_before_it_takes_effect() {
        let f = fixture();
        f.transport.enqueue_json_with_cookies(
            r#"{"code":803}"#,
            &["MUSIC_U=fresh; Path=/", "__csrf=c; Path=/"],
        );
        f.transport.enqueue_json(ACCOUNT_OK);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        f.manager
            .set_account_listener(move |account| sink.lock().unwrap().push(account.clone()));

        let outcome = f.manager.qr_poll("k").await.unwrap();

        let QrPollOutcome::Success { account } = outcome else {
            panic!("应为 Success，实际 {outcome:?}");
        };
        assert_eq!(account.nickname.as_deref(), Some("测试用户"));
        assert_eq!(account.user_id, 42);
        assert!(f.client.is_logged_in());
        let saved = f.store.saved.lock().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].cookie_value("MUSIC_U").as_deref(), Some("fresh"));
        assert_eq!(saved[0].user_id, 42);
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert!(has_log(&f.logs, LogLevel::Success, "登录成功：测试用户"));
        f.manager.stop_keepalive();
    }

    #[tokio::test]
    async fn qr_poll_confirmed_but_unsaveable_does_not_leave_a_half_logged_in_state() {
        let f = fixture();
        *f.store.fail_on_save.lock().unwrap() = true;
        f.transport
            .enqueue_json_with_cookies(r#"{"code":803}"#, &["MUSIC_U=fresh; Path=/"]);
        f.transport.enqueue_json(ACCOUNT_OK);

        let outcome = f.manager.qr_poll("k").await.unwrap();

        let QrPollOutcome::Failed { message } = outcome else {
            panic!("应为 Failed，实际 {outcome:?}");
        };
        assert!(message.starts_with("保存登录状态失败："));
        assert!(!f.client.is_logged_in());
        assert!(has_log(&f.logs, LogLevel::Error, "保存登录状态失败"));
    }

    #[tokio::test(start_paused = true)]
    async fn qr_poll_confirmed_without_profile_fills_it_in_later() {
        let f = fixture();
        f.transport
            .enqueue_json_with_cookies(r#"{"code":803}"#, &["MUSIC_U=fresh; Path=/"]);
        f.transport.enqueue_status(500);
        f.transport.enqueue_status(500);
        f.transport.enqueue_json(ACCOUNT_OK); // 后台补取

        let outcome = f.manager.qr_poll("k").await.unwrap();
        assert!(matches!(outcome, QrPollOutcome::Success { .. }));
        assert_eq!(f.manager.account().user_id, 0);
        assert!(has_log(&f.logs, LogLevel::Warning, "读取账号信息失败"));

        for _ in 0..50 {
            if f.manager.account().user_id == 42 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(f.manager.account().user_id, 42);
        assert_eq!(f.manager.account().nickname.as_deref(), Some("测试用户"));
        f.manager.stop_keepalive();
    }

    #[tokio::test]
    async fn qr_poll_errors_propagate_so_the_caller_can_retry() {
        let f = fixture();
        f.transport
            .enqueue_error(TransportError::Network("断网".to_owned()));

        let error = f.manager.qr_poll("k").await.unwrap_err();

        assert!(error.is_transient_network());
    }
}
