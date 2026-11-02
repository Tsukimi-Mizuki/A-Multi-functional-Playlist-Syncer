//! 登录 Cookie 被服务端续期 / 轮换后的自动落盘。对应 C# 的 `SessionAutoSaver`。
//!
//! 订阅 [`NeteaseClient`] 的会话变化，把变化串行写盘。写入时总是读取客户端**当前**登录态
//! （而不是事件携带的快照），连续多次变化会合并成一次写入，且不会与退出登录竞争：
//! 当前已无登录态时直接跳过。

use std::sync::{Arc, Mutex, MutexGuard, Weak};

use tokio::sync::watch;

use super::SessionStorage;
use crate::error::CoreError;
use crate::netease::{NeteaseClient, SubscriptionId};

/// 落盘失败时的回调类型。
pub type ErrorHandler = Box<dyn Fn(&CoreError) + Send + Sync>;

#[derive(Default)]
struct State {
    dirty: bool,
    running: bool,
    disposed: bool,
}

struct Shared {
    client: Weak<NeteaseClient>,
    store: Arc<dyn SessionStorage>,
    on_error: Option<ErrorHandler>,
    state: Mutex<State>,
    /// `true` 表示写盘循环正在运行；`flush` 等它回到 `false`。
    running: watch::Sender<bool>,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 写盘循环：同步执行（文件与 DPAPI 都是阻塞调用），由 `spawn_blocking` 承载。
    fn pump(&self) {
        loop {
            {
                let mut state = self.state();
                if !state.dirty {
                    state.running = false;
                    self.running.send_replace(false);
                    return;
                }
                state.dirty = false;
            }

            let current = self
                .client
                .upgrade()
                .and_then(|client| client.session())
                .filter(|session| session.is_logged_in());
            if let Some(session) = current
                && let Err(error) = self.store.save(&session)
                && let Some(on_error) = &self.on_error
            {
                // 记录回调自身出错也不能中断写入循环。
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_error(&error)));
            }
        }
    }
}

/// 订阅会话变化并串行落盘。
pub struct SessionAutoSaver {
    shared: Arc<Shared>,
    client: Weak<NeteaseClient>,
    subscription: SubscriptionId,
}

impl SessionAutoSaver {
    /// 开始监听 `client`。`on_error` 在落盘失败时调用（吞掉异常，仅用于记录）。
    pub fn new(
        client: &Arc<NeteaseClient>,
        store: Arc<dyn SessionStorage>,
        on_error: Option<ErrorHandler>,
    ) -> Self {
        let shared = Arc::new(Shared {
            client: Arc::downgrade(client),
            store,
            on_error,
            state: Mutex::new(State::default()),
            running: watch::channel(false).0,
        });
        let listener_shared = Arc::clone(&shared);
        let subscription = client.subscribe_session_changed(move |_| {
            Self::on_session_changed(&listener_shared);
        });
        Self {
            shared,
            client: Arc::downgrade(client),
            subscription,
        }
    }

    fn on_session_changed(shared: &Arc<Shared>) {
        {
            let mut state = shared.state();
            if state.disposed {
                return;
            }
            state.dirty = true;
            if state.running {
                return;
            }
            state.running = true;
            shared.running.send_replace(true);
        }

        let worker = Arc::clone(shared);
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn_blocking(move || worker.pump());
            }
            // 不在 tokio 运行时里（理论上不会发生，请求本身就在运行时中）：直接在当前线程写。
            Err(_) => worker.pump(),
        }
    }

    /// 等待当前已排队的写入全部完成（测试与应用退出前使用）。
    pub async fn flush(&self) {
        let mut receiver = self.shared.running.subscribe();
        // 发送端活得比 flush 久，等待只会因值变为 false 而结束。
        let _ = receiver.wait_for(|running| !*running).await;
    }

    /// 停止监听；之后的会话变化不再落盘。
    pub fn dispose(&self) {
        {
            let mut state = self.shared.state();
            if state.disposed {
                return;
            }
            state.disposed = true;
        }
        if let Some(client) = self.client.upgrade() {
            client.unsubscribe_session_changed(self.subscription);
        }
    }
}

impl Drop for SessionAutoSaver {
    fn drop(&mut self) {
        self.dispose();
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Condvar, Mutex};

    use crate::error::{CoreError, CoreResult};
    use crate::session::{NeteaseSession, SessionStorage};

    /// 记录保存 / 清除调用的假存储；可让 `save` 抛错，或在闸门关闭时阻塞。
    #[derive(Default)]
    pub struct RecordingStore {
        pub saved: Mutex<Vec<NeteaseSession>>,
        pub cleared: Mutex<u32>,
        pub fail_on_save: Mutex<bool>,
        /// 启动时要返回的登录态。
        pub stored: Mutex<Option<NeteaseSession>>,
        /// `load` 要返回的失败原因。
        pub load_error: Mutex<Option<String>>,
        /// 所有调用的顺序记录（`save` / `clear`），用来断言顺序。
        pub operations: Mutex<Vec<&'static str>>,
        /// `(闸门是否关闭, 有线程正卡在闸门上)`。
        gate: (Mutex<(bool, bool)>, Condvar),
    }

    impl RecordingStore {
        pub fn close_gate(&self) {
            self.gate.0.lock().unwrap().0 = true;
        }

        pub fn open_gate(&self) {
            self.gate.0.lock().unwrap().0 = false;
            self.gate.1.notify_all();
        }

        pub fn is_blocked(&self) -> bool {
            self.gate.0.lock().unwrap().1
        }
    }

    impl SessionStorage for RecordingStore {
        fn load(&self) -> CoreResult<Option<NeteaseSession>> {
            if let Some(reason) = self.load_error.lock().unwrap().clone() {
                return Err(CoreError::Session(reason));
            }
            Ok(self.stored.lock().unwrap().clone())
        }

        fn save(&self, session: &NeteaseSession) -> CoreResult<()> {
            let mut gate = self.gate.0.lock().unwrap();
            while gate.0 {
                gate.1 = true;
                gate = self.gate.1.wait(gate).unwrap();
            }
            gate.1 = false;
            drop(gate);
            if *self.fail_on_save.lock().unwrap() {
                return Err(CoreError::Session("磁盘已满".to_owned()));
            }
            self.operations.lock().unwrap().push("save");
            self.saved.lock().unwrap().push(session.clone());
            Ok(())
        }

        fn clear(&self) -> CoreResult<()> {
            self.operations.lock().unwrap().push("clear");
            *self.cleared.lock().unwrap() += 1;
            Ok(())
        }

        fn last_load_failure(&self) -> Option<String> {
            self.load_error.lock().unwrap().clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::test_support::RecordingStore;
    use super::*;
    use crate::session::NeteaseSession;
    use crate::test_util::{FakeTransport, test_client};

    const ACCOUNT_JSON: &str =
        r#"{"code":200,"account":{"id":1},"profile":{"userId":1,"nickname":"n"}}"#;

    fn rotating_client(responses: usize) -> (Arc<FakeTransport>, Arc<NeteaseClient>) {
        let transport = FakeTransport::new();
        for index in 1..=responses {
            let cookie = format!("MUSIC_U=r{index}; Path=/");
            transport.enqueue_json_with_cookies(ACCOUNT_JSON, &[cookie.as_str()]);
        }
        let client = Arc::new(test_client(&transport));
        client.set_session(Some(NeteaseSession {
            cookie: "MUSIC_U=old".to_owned(),
            ..Default::default()
        }));
        (transport, client)
    }

    async fn wait_until_blocked(store: &RecordingStore) {
        while !store.is_blocked() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }

    #[tokio::test]
    async fn rotated_cookie_is_persisted_with_latest_session() {
        let (_transport, client) = rotating_client(1);
        let store = Arc::new(RecordingStore::default());
        let saver = SessionAutoSaver::new(&client, store.clone(), None);

        client.get_current_user().await.unwrap();
        saver.flush().await;

        let saved = store.saved.lock().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].cookie_value("MUSIC_U").as_deref(), Some("r1"));
    }

    #[tokio::test]
    async fn save_failure_is_reported_and_does_not_break_anything() {
        let (_transport, client) = rotating_client(1);
        let store = Arc::new(RecordingStore::default());
        *store.fail_on_save.lock().unwrap() = true;
        let errors = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&errors);
        let saver = SessionAutoSaver::new(
            &client,
            store,
            Some(Box::new(move |error| {
                sink.lock().unwrap().push(error.to_string())
            })),
        );

        client.get_current_user().await.unwrap();
        saver.flush().await;

        assert_eq!(*errors.lock().unwrap(), vec!["磁盘已满".to_owned()]);
    }

    #[tokio::test]
    async fn dispose_stops_listening() {
        let (_transport, client) = rotating_client(1);
        let store = Arc::new(RecordingStore::default());
        let saver = SessionAutoSaver::new(&client, store.clone(), None);
        saver.dispose();

        client.get_current_user().await.unwrap();
        saver.flush().await;

        assert!(store.saved.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn dropping_the_saver_stops_listening() {
        let (_transport, client) = rotating_client(1);
        let store = Arc::new(RecordingStore::default());
        drop(SessionAutoSaver::new(&client, store.clone(), None));

        client.get_current_user().await.unwrap();

        assert!(store.saved.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn consecutive_changes_are_coalesced_into_one_follow_up_write() {
        let (_transport, client) = rotating_client(3);
        let store = Arc::new(RecordingStore::default());
        let saver = SessionAutoSaver::new(&client, store.clone(), None);
        store.close_gate();

        client.get_current_user().await.unwrap();
        wait_until_blocked(&store).await;
        // 第一次写入卡住期间又来了两次变化。
        client.get_current_user().await.unwrap();
        client.get_current_user().await.unwrap();
        store.open_gate();
        saver.flush().await;

        let saved = store.saved.lock().unwrap();
        assert_eq!(saved.len(), 2, "后两次变化应合并成一次写入");
        assert_eq!(saved[0].cookie_value("MUSIC_U").as_deref(), Some("r1"));
        assert_eq!(saved[1].cookie_value("MUSIC_U").as_deref(), Some("r3"));
    }

    #[tokio::test]
    async fn pending_write_is_skipped_once_the_session_was_cleared() {
        let (_transport, client) = rotating_client(2);
        let store = Arc::new(RecordingStore::default());
        let saver = SessionAutoSaver::new(&client, store.clone(), None);
        store.close_gate();

        client.get_current_user().await.unwrap();
        wait_until_blocked(&store).await;
        client.get_current_user().await.unwrap();
        // 退出登录：先清内存会话，再放行闸门。排队的第二次写入必须被跳过。
        client.set_session(None);
        store.open_gate();
        saver.flush().await;

        assert_eq!(store.saved.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn flush_with_nothing_pending_returns_immediately() {
        let (_transport, client) = rotating_client(0);
        let saver = SessionAutoSaver::new(&client, Arc::new(RecordingStore::default()), None);

        saver.flush().await;
    }
}
