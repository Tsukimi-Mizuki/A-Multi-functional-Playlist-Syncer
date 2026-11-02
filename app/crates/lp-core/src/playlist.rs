//! 歌单加载编排：取当前账号的歌单列表与歌单曲目，写运行日志，并在接口返回 301 时走登录复核。
//!
//! C# 版把这些逻辑放在 `MainWindowViewModel.RefreshAccountAndPlaylistsAsync` /
//! `LoadPlaylistDetailAsync` / `HandleOperationError` 里，这里下沉到 core，Tauri 命令只做转发。
//!
//! 错误处理与 C# 一致：
//!
//! - 接口业务 `code == 301` 才算“登录失效的迹象”：先经 [`LoginManager::handle_runtime_auth_error`]
//!   续期并复核，复核后仍未登录才清除登录态（此时返回 [`PlaylistErrorKind::SessionExpired`]）；
//!   复核后仍有效或暂时无法确认时保留登录，提示“可稍后重试”。
//! - 其余错误（网络、HTTP、限流……）只提示、不动登录态。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use crate::auth::{LoginManager, RuntimeAuthOutcome};
use crate::logs::LogHub;
use crate::netease::{NeteaseError, PlaylistDetail, PlaylistSummary, ProgressFn};

/// 加载失败的类别，前端据此决定是否提示“重新登录”。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaylistErrorKind {
    /// 还没有登录。
    NotLoggedIn,
    /// 接口说未登录，复核后确认已失效，登录态已清除。
    SessionExpired,
    /// 其他失败（网络、接口错误、暂时无法确认登录状态……），登录态保留。
    Failed,
}

/// 加载歌单失败，`message` 是可以直接展示给用户的中文说明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistError {
    /// 失败类别。
    pub kind: PlaylistErrorKind,
    /// 中文说明。
    pub message: String,
}

impl std::fmt::Display for PlaylistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PlaylistError {}

const LIST_OPERATION: &str = "加载歌单列表失败";
const TRACKS_OPERATION: &str = "加载歌单失败";

/// 歌单加载服务。
pub struct PlaylistService {
    login: Arc<LoginManager>,
    logs: Arc<LogHub>,
}

impl PlaylistService {
    /// 创建服务；请求共用 `login` 持有的客户端与登录态。
    pub fn new(login: Arc<LoginManager>, logs: Arc<LogHub>) -> Self {
        Self { login, logs }
    }

    /// 获取当前账号的全部歌单（创建的 + 收藏的）。
    pub async fn list(&self) -> Result<Vec<PlaylistSummary>, PlaylistError> {
        let user_id = self.current_user_id(LIST_OPERATION).await?;
        match self.login.client().get_user_playlists(user_id).await {
            Ok(playlists) => {
                self.logs
                    .success(format!("已加载 {} 个歌单", playlists.len()));
                Ok(playlists)
            }
            Err(error) => Err(self.fail(LIST_OPERATION, error).await),
        }
    }

    /// 获取一个歌单的完整曲目。`progress` 在需要分批补齐歌曲详情时回报 `(已加载, 总数)`。
    pub async fn tracks(
        &self,
        playlist_id: i64,
        progress: &ProgressFn<'_>,
    ) -> Result<PlaylistDetail, PlaylistError> {
        if !self.login.account().logged_in {
            return Err(not_logged_in());
        }

        let announced = AtomicBool::new(false);
        let logs = &self.logs;
        let reporter = |loaded: usize, total: usize| {
            if loaded < total && !announced.swap(true, Ordering::Relaxed) {
                logs.info(format!(
                    "歌单较大，正在补全 {} 首歌曲详情…",
                    total.saturating_sub(loaded)
                ));
            }
            progress(loaded, total);
        };

        match self
            .login
            .client()
            .get_playlist_detail(playlist_id, Some(&reporter))
            .await
        {
            Ok(detail) => {
                let name = if detail.name.trim().is_empty() {
                    "未命名歌单"
                } else {
                    detail.name.as_str()
                };
                self.logs.success(format!(
                    "歌单「{name}」加载完成，共 {} 首",
                    detail.songs.len()
                ));
                if detail.is_partial {
                    self.logs.warning(format!(
                        "歌单「{name}」有 {} 首歌曲没有取回详情（可能已下架或接口暂时异常）",
                        detail.track_ids.len().saturating_sub(detail.songs.len())
                    ));
                }
                Ok(detail)
            }
            Err(error) => Err(self.fail(TRACKS_OPERATION, error).await),
        }
    }

    /// 当前账号的用户 ID；会话里没有资料（扫码时没取到）时联网补取一次。
    async fn current_user_id(&self, operation: &str) -> Result<i64, PlaylistError> {
        let account = self.login.account();
        if !account.logged_in {
            return Err(not_logged_in());
        }
        if account.user_id > 0 {
            return Ok(account.user_id);
        }

        match self.login.client().get_current_user().await {
            Ok(Some(user)) if user.user_id > 0 => Ok(user.user_id),
            // 接口 HTTP 200 却没有 account / profile：服务端明确认为当前未登录。
            Ok(_) => Err(self
                .fail(
                    operation,
                    NeteaseError::api("接口未返回账号信息，登录状态已失效", 301),
                )
                .await),
            Err(error) => Err(self.fail(operation, error).await),
        }
    }

    /// 记录错误日志并生成面向用户的错误；301 时先走登录复核。
    async fn fail(&self, operation: &str, error: NeteaseError) -> PlaylistError {
        let detail = error.user_message();
        self.logs.error(format!("{operation}：{detail}"));

        if error.is_not_logged_in() {
            return match self.login.handle_runtime_auth_error().await {
                RuntimeAuthOutcome::LoggedOut => PlaylistError {
                    kind: PlaylistErrorKind::SessionExpired,
                    message: "登录状态已失效，请重新扫码登录".to_owned(),
                },
                RuntimeAuthOutcome::Kept | RuntimeAuthOutcome::Skipped => PlaylistError {
                    kind: PlaylistErrorKind::Failed,
                    message: format!("{operation}：{detail}（已保留登录，可稍后重试）"),
                },
            };
        }

        PlaylistError {
            kind: PlaylistErrorKind::Failed,
            message: format!("{operation}：{detail}"),
        }
    }
}

fn not_logged_in() -> PlaylistError {
    PlaylistError {
        kind: PlaylistErrorKind::NotLoggedIn,
        message: "请先扫码登录网易云音乐".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;
    use crate::auth::LoginOptions;
    use crate::logs::LogLevel;
    use crate::netease::NeteaseClient;
    use crate::netease::http::TransportError;
    use crate::session::NeteaseSession;
    use crate::session::test_support::RecordingStore;
    use crate::test_util::{
        FakeTransport, parse_song_ids, song_detail_response_for_ids, test_client,
    };

    const NOT_LOGGED_IN: &str = r#"{"code":301,"message":"需要登录"}"#;
    const ACCOUNT_OK: &str = r#"{"code":200,"account":{"id":42},"profile":{"userId":42,"nickname":"测试用户","vipType":0}}"#;
    const ACCOUNT_NULL: &str = r#"{"code":200,"account":null,"profile":null}"#;
    const PLAYLIST_OK: &str = r#"{"code":200,"playlist":[{"id":7,"name":"歌单","trackCount":2}]}"#;
    const REFRESH_OK: &str = r#"{"code":200}"#;

    struct Fixture {
        transport: Arc<FakeTransport>,
        client: Arc<NeteaseClient>,
        store: Arc<RecordingStore>,
        logs: Arc<LogHub>,
        service: PlaylistService,
        login: Arc<LoginManager>,
    }

    fn fixture(user_id: i64, logged_in: bool) -> Fixture {
        let transport = FakeTransport::new();
        let client = Arc::new(test_client(&transport));
        let store = Arc::new(RecordingStore::default());
        let logs = LogHub::new();
        let login = LoginManager::new(
            Arc::clone(&client),
            store.clone(),
            Arc::clone(&logs),
            LoginOptions {
                keepalive_interval: Duration::from_secs(3600),
            },
        );
        if logged_in {
            *store.stored.lock().unwrap() = Some(NeteaseSession {
                cookie: "MUSIC_U=old; __csrf=c".to_owned(),
                nickname: Some("昵称".to_owned()),
                user_id,
                ..Default::default()
            });
            login.restore();
        }
        let service = PlaylistService::new(Arc::clone(&login), Arc::clone(&logs));
        Fixture {
            transport,
            client,
            store,
            logs,
            service,
            login,
        }
    }

    fn has_log(logs: &LogHub, level: LogLevel, fragment: &str) -> bool {
        logs.recent()
            .iter()
            .any(|entry| entry.level == level && entry.message.contains(fragment))
    }

    fn no_progress(_: usize, _: usize) {}

    #[tokio::test]
    async fn list_loads_playlists_for_the_session_user_and_logs_the_count() {
        let f = fixture(42, true);
        f.transport.enqueue_json(PLAYLIST_OK);

        let playlists = f.service.list().await.unwrap();

        assert_eq!(playlists.len(), 1);
        assert!(has_log(&f.logs, LogLevel::Success, "已加载 1 个歌单"));
        let payload = crate::test_util::decrypt_business_json(
            &f.transport.requests()[0],
            crate::test_util::FIXED_SECRET,
        );
        assert!(payload.contains("\"uid\":42"));
    }

    #[tokio::test]
    async fn list_without_login_fails_without_any_request() {
        let f = fixture(0, false);

        let error = f.service.list().await.unwrap_err();

        assert_eq!(error.kind, PlaylistErrorKind::NotLoggedIn);
        assert_eq!(error.message, "请先扫码登录网易云音乐");
        assert!(f.transport.requests().is_empty());
    }

    #[tokio::test]
    async fn list_fetches_the_account_first_when_the_session_has_no_user_id() {
        let f = fixture(0, true);
        f.transport
            .enqueue_json(ACCOUNT_OK)
            .enqueue_json(PLAYLIST_OK);

        let playlists = f.service.list().await.unwrap();

        assert_eq!(playlists.len(), 1);
        let requests = f.transport.requests();
        assert!(requests[0].url.ends_with("/weapi/nuser/account/get"));
        assert!(requests[1].url.ends_with("/weapi/user/playlist"));
        let payload =
            crate::test_util::decrypt_business_json(&requests[1], crate::test_util::FIXED_SECRET);
        assert!(payload.contains("\"uid\":42"));
    }

    #[tokio::test]
    async fn list_treats_an_empty_account_as_session_loss_and_clears_it_after_rechecking() {
        let f = fixture(0, true);
        // 取账号得到空 → 301 语义 → 续期（失败）→ 复核账号仍为空 → 清除登录。
        f.transport
            .enqueue_json(ACCOUNT_NULL)
            .enqueue_json(NOT_LOGGED_IN) // refresh
            .enqueue_json(ACCOUNT_NULL); // 复核

        let error = f.service.list().await.unwrap_err();

        assert_eq!(error.kind, PlaylistErrorKind::SessionExpired);
        assert_eq!(error.message, "登录状态已失效，请重新扫码登录");
        assert!(!f.client.is_logged_in());
    }

    #[tokio::test]
    async fn code_301_triggers_the_recheck_and_clears_the_login_when_still_logged_out() {
        let f = fixture(42, true);
        f.transport
            .enqueue_json(NOT_LOGGED_IN) // user/playlist
            .enqueue_json(NOT_LOGGED_IN) // refresh
            .enqueue_json(NOT_LOGGED_IN); // 复核 account/get

        let error = f.service.list().await.unwrap_err();

        assert_eq!(error.kind, PlaylistErrorKind::SessionExpired);
        assert!(!f.client.is_logged_in());
        assert!(has_log(&f.logs, LogLevel::Error, "加载歌单列表失败"));
        let paths: Vec<String> = f
            .transport
            .requests()
            .iter()
            .map(|r| r.url.replace("https://music.163.com", ""))
            .collect();
        assert_eq!(
            paths,
            [
                "/weapi/user/playlist",
                "/weapi/login/token/refresh",
                "/weapi/nuser/account/get"
            ]
        );
        // 清除登录态时也要删掉落盘文件。
        assert!(*f.store.cleared.lock().unwrap() >= 1);
    }

    #[tokio::test]
    async fn code_301_keeps_the_login_when_the_recheck_says_it_is_still_valid() {
        let f = fixture(42, true);
        f.transport
            .enqueue_json(NOT_LOGGED_IN) // user/playlist
            .enqueue_json(REFRESH_OK) // refresh
            .enqueue_json(ACCOUNT_OK); // 复核通过

        let error = f.service.list().await.unwrap_err();

        assert_eq!(error.kind, PlaylistErrorKind::Failed);
        assert!(error.message.contains("已保留登录，可稍后重试"));
        assert!(f.client.is_logged_in());
    }

    #[tokio::test]
    async fn code_301_keeps_the_login_when_the_recheck_cannot_reach_the_server() {
        let f = fixture(42, true);
        f.transport
            .enqueue_json(NOT_LOGGED_IN)
            .enqueue_error(TransportError::Network("断网".to_owned())) // refresh
            .enqueue_error(TransportError::Network("断网".to_owned())); // 复核

        let error = f.service.list().await.unwrap_err();

        assert_eq!(error.kind, PlaylistErrorKind::Failed);
        assert!(f.client.is_logged_in());
    }

    #[tokio::test]
    async fn network_errors_never_touch_the_login_or_start_a_recheck() {
        let f = fixture(42, true);
        f.transport
            .enqueue_error(TransportError::Network("dns".to_owned()));

        let error = f.service.list().await.unwrap_err();

        assert_eq!(error.kind, PlaylistErrorKind::Failed);
        assert!(error.message.starts_with("加载歌单列表失败："));
        assert!(error.message.contains("网络连接失败"));
        assert!(f.client.is_logged_in());
        assert_eq!(f.transport.requests().len(), 1);
    }

    #[tokio::test]
    async fn rate_limit_and_http_errors_are_not_login_expiry() {
        for response in [r#"{"code":-460,"message":"风控"}"#, r#"{"code":406}"#] {
            let f = fixture(42, true);
            f.transport.enqueue_json(response);
            let error = f.service.list().await.unwrap_err();
            assert_eq!(error.kind, PlaylistErrorKind::Failed);
            assert!(f.client.is_logged_in());
            assert_eq!(f.transport.requests().len(), 1);
        }
        let f = fixture(42, true);
        f.transport.enqueue_status(401);
        assert_eq!(
            f.service.list().await.unwrap_err().kind,
            PlaylistErrorKind::Failed
        );
        assert!(f.client.is_logged_in());
    }

    #[tokio::test]
    async fn tracks_load_logs_the_summary_and_reports_progress() {
        let f = fixture(42, true);
        let ids: Vec<String> = (1..=600).map(|id| format!(r#"{{"id":{id}}}"#)).collect();
        f.transport.enqueue_json(&format!(
            r#"{{"code":200,"playlist":{{"id":9,"name":"大歌单","trackCount":600,"tracks":[],"trackIds":[{}]}}}}"#,
            ids.join(",")
        ));
        f.transport
            .set_fallback(|request| song_detail_response_for_ids(&parse_song_ids(request)));
        let reports = Mutex::new(Vec::new());
        let progress = |loaded: usize, total: usize| reports.lock().unwrap().push((loaded, total));

        let detail = f.service.tracks(9, &progress).await.unwrap();

        assert_eq!(detail.songs.len(), 600);
        assert_eq!(
            *reports.lock().unwrap(),
            vec![(0, 600), (500, 600), (600, 600)]
        );
        assert!(has_log(
            &f.logs,
            LogLevel::Info,
            "歌单较大，正在补全 600 首"
        ));
        assert!(has_log(
            &f.logs,
            LogLevel::Success,
            "歌单「大歌单」加载完成，共 600 首"
        ));
        assert!(!has_log(&f.logs, LogLevel::Warning, "没有取回详情"));
    }

    #[tokio::test]
    async fn tracks_load_warns_when_the_result_is_partial() {
        let f = fixture(42, true);
        f.transport
            .enqueue_json(
                r#"{"code":200,"playlist":{"id":9,"name":"部分","tracks":[],"trackIds":[{"id":1},{"id":2}]}}"#,
            )
            .enqueue_json(r#"{"code":200,"songs":[{"id":1,"name":"a"}],"privileges":[]}"#);

        let detail = f.service.tracks(9, &no_progress).await.unwrap();

        assert!(detail.is_partial);
        assert!(has_log(
            &f.logs,
            LogLevel::Warning,
            "歌单「部分」有 1 首歌曲没有取回详情"
        ));
    }

    #[tokio::test]
    async fn tracks_requires_a_login() {
        let f = fixture(0, false);

        let error = f.service.tracks(9, &no_progress).await.unwrap_err();

        assert_eq!(error.kind, PlaylistErrorKind::NotLoggedIn);
        assert!(f.transport.requests().is_empty());
    }

    #[tokio::test]
    async fn tracks_301_from_a_batch_triggers_the_recheck() {
        let f = fixture(42, true);
        f.transport
            .enqueue_json(
                r#"{"code":200,"playlist":{"id":9,"name":"x","tracks":[],"trackIds":[{"id":1}]}}"#,
            )
            .enqueue_json(NOT_LOGGED_IN) // song/detail
            .enqueue_json(NOT_LOGGED_IN) // refresh
            .enqueue_json(ACCOUNT_NULL); // 复核

        let error = f.service.tracks(9, &no_progress).await.unwrap_err();

        assert_eq!(error.kind, PlaylistErrorKind::SessionExpired);
        assert!(!f.login.client().is_logged_in());
        assert!(has_log(&f.logs, LogLevel::Error, "加载歌单失败："));
    }

    #[tokio::test]
    async fn a_missing_playlist_is_a_plain_failure() {
        let f = fixture(42, true);
        f.transport.enqueue_json(r#"{"code":200,"msg":"x"}"#);

        let error = f.service.tracks(9, &no_progress).await.unwrap_err();

        assert_eq!(error.kind, PlaylistErrorKind::Failed);
        assert!(error.message.contains("不存在或无权访问"));
        assert!(f.client.is_logged_in());
    }

    #[test]
    fn errors_serialize_with_camel_case_kinds() {
        let value = serde_json::to_value(PlaylistError {
            kind: PlaylistErrorKind::SessionExpired,
            message: "x".to_owned(),
        })
        .unwrap();
        assert_eq!(value["kind"], "sessionExpired");
        assert_eq!(value["message"], "x");
        assert_eq!(
            serde_json::to_value(PlaylistErrorKind::NotLoggedIn).unwrap(),
            "notLoggedIn"
        );
    }
}
