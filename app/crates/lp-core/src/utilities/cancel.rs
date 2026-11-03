//! 轻量取消令牌：同步的文件 IO（NCM 解密、批量导入）在每个分块之间检查它。
//!
//! 对应 C# 的 `CancellationToken`。lp-core 里异步流程用 tokio 的原语，但解密 / 标签写入是跑在
//! `spawn_blocking` 里的同步代码，只需要一个可克隆、跨线程共享的“是否已取消”标志，不值得再引入 `tokio-util`。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 可克隆的取消令牌；所有克隆共享同一个标志。
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    /// 新建一个未取消的令牌。
    pub fn new() -> Self {
        Self::default()
    }

    /// 请求取消（幂等）。
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// 是否已请求取消。
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
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
}
