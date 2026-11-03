//! “同一时间只有一个 NCM 导入任务”的登记处：外壳用它拒绝重复启动，并把 `ncm_cancel` 转给当前任务。

use std::sync::{Arc, Mutex, MutexGuard};

use super::NcmImportError;
use crate::utilities::CancelToken;

/// 当前正在运行的导入任务（至多一个）。克隆后共享同一个登记处。
#[derive(Debug, Clone, Default)]
pub struct NcmJobSlot {
    current: Arc<Mutex<Option<CancelToken>>>,
}

/// 正在运行的任务句柄；丢弃（任务结束、出错或被取消的 future 被丢弃）时自动释放登记。
#[derive(Debug)]
pub struct NcmJob {
    /// 本任务的取消令牌，传给 [`super::NcmImportService::import`]。
    pub cancel: CancelToken,
    slot: Arc<Mutex<Option<CancelToken>>>,
}

fn lock(slot: &Mutex<Option<CancelToken>>) -> MutexGuard<'_, Option<CancelToken>> {
    slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl NcmJobSlot {
    /// 登记一个新任务；已有任务在运行时返回 [`NcmImportError::Busy`]。
    pub fn try_begin(&self) -> Result<NcmJob, NcmImportError> {
        let mut current = lock(&self.current);
        if current.is_some() {
            return Err(NcmImportError::Busy);
        }
        let cancel = CancelToken::new();
        *current = Some(cancel.clone());
        Ok(NcmJob {
            cancel,
            slot: Arc::clone(&self.current),
        })
    }

    /// 请求取消当前任务；返回是否确实有任务在运行。
    pub fn cancel(&self) -> bool {
        match lock(&self.current).as_ref() {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// 是否有任务在运行。
    pub fn is_running(&self) -> bool {
        lock(&self.current).is_some()
    }
}

impl Drop for NcmJob {
    fn drop(&mut self) {
        *lock(&self.slot) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_job_runs_at_a_time_and_dropping_releases_the_slot() {
        let slot = NcmJobSlot::default();
        assert!(!slot.is_running());

        let job = slot.try_begin().unwrap();
        assert!(slot.is_running());
        assert!(matches!(slot.try_begin(), Err(NcmImportError::Busy)));

        drop(job);
        assert!(!slot.is_running());
        assert!(slot.try_begin().is_ok());
    }

    #[test]
    fn cancel_reaches_the_running_job_only() {
        let slot = NcmJobSlot::default();
        assert!(!slot.cancel(), "没有任务时取消应返回 false");

        let job = slot.try_begin().unwrap();
        assert!(!job.cancel.is_cancelled());
        assert!(slot.cancel());
        assert!(job.cancel.is_cancelled());

        // 上一个任务的取消不影响下一个任务。
        drop(job);
        let next = slot.try_begin().unwrap();
        assert!(!next.cancel.is_cancelled());
    }

    #[test]
    fn clones_share_the_same_slot() {
        let slot = NcmJobSlot::default();
        let other = slot.clone();
        let _job = slot.try_begin().unwrap();
        assert!(other.is_running());
        assert!(matches!(other.try_begin(), Err(NcmImportError::Busy)));
    }
}
