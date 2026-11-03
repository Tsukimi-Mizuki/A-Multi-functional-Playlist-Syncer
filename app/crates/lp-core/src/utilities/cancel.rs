//! 取消令牌：同步的文件 IO（NCM 解密、批量导入）在每个分块之间检查它，异步流程用
//! [`cancellable`] 把任何 future 变成“取消即中断”。
//!
//! 对应 C# 的 `CancellationToken`。令牌可克隆、跨线程共享；`cancel()` 之后 [`CancelToken::cancelled`]
//! 立即完成，所以正在等网络的任务能被叫醒，不必轮询。

use std::future::{Future, poll_fn};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;

use tokio::sync::Notify;

#[derive(Debug, Default)]
struct Inner {
    flag: AtomicBool,
    notify: Notify,
}

/// 可克隆的取消令牌；所有克隆共享同一个标志。
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    inner: Arc<Inner>,
}

impl CancelToken {
    /// 新建一个未取消的令牌。
    pub fn new() -> Self {
        Self::default()
    }

    /// 请求取消（幂等），并叫醒所有正在 [`cancelled`](Self::cancelled) 上等待的任务。
    pub fn cancel(&self) {
        self.inner.flag.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    /// 是否已请求取消。
    pub fn is_cancelled(&self) -> bool {
        self.inner.flag.load(Ordering::SeqCst)
    }

    /// 等到被取消才完成；已经取消则立即完成。
    pub async fn cancelled(&self) {
        loop {
            // 必须先登记等待再检查标志，否则取消恰好发生在两者之间就会漏掉唤醒。
            let notified = self.inner.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

/// 操作被取消。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

/// 运行 `future`，但令牌一被取消就丢弃它并返回 [`Cancelled`]。
///
/// 被丢弃的 future 必须是“取消安全”的（网络读取、定时器都是）；阻塞线程里的任务不会因此停下，
/// 它们需要自己检查令牌。
pub async fn cancellable<F: Future>(
    token: &CancelToken,
    future: F,
) -> Result<F::Output, Cancelled> {
    if token.is_cancelled() {
        return Err(Cancelled);
    }
    let mut future = Box::pin(future);
    let mut cancelled = Box::pin(token.cancelled());
    poll_fn(|cx| {
        if let Poll::Ready(output) = future.as_mut().poll(cx) {
            return Poll::Ready(Ok(output));
        }
        if cancelled.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(Cancelled));
        }
        Poll::Pending
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn clones_share_the_same_flag() {
        let token = CancelToken::new();
        let clone = token.clone();
        assert!(!clone.is_cancelled());
        token.cancel();
        assert!(clone.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn cancellable_returns_the_output_when_not_cancelled() {
        let token = CancelToken::new();
        assert_eq!(cancellable(&token, async { 7 }).await, Ok(7));
    }

    #[tokio::test]
    async fn cancellable_refuses_to_start_when_already_cancelled() {
        let token = CancelToken::new();
        token.cancel();
        assert_eq!(cancellable(&token, async { 7 }).await, Err(Cancelled));
    }

    #[tokio::test]
    async fn cancelling_interrupts_a_pending_future() {
        let token = CancelToken::new();
        let canceller = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            canceller.cancel();
        });
        let result = cancellable(&token, std::future::pending::<()>()).await;
        assert_eq!(result, Err(Cancelled));
    }

    #[tokio::test]
    async fn cancelled_completes_immediately_after_cancel() {
        let token = CancelToken::new();
        token.cancel();
        token.cancelled().await;
    }
}
