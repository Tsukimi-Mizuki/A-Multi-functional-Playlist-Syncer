//! 运行日志：内存环形缓冲 + 实时监听。对应 C# `MainWindowViewModel` 里的日志列表（最多 400 条、不落盘）。
//!
//! 各模块只管 [`LogHub::push`]；外壳把监听器接到前端事件上，并让前端在启动时用 [`LogHub::recent`] 补齐
//! 监听建立之前产生的日志（条目的 `id` 单调递增，前端按 `id` 去重）。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// 内存里保留的日志条数上限。
pub const MAX_LOG_ENTRIES: usize = 400;

/// 日志级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// 普通信息。
    Info,
    /// 成功。
    Success,
    /// 警告（不影响继续使用）。
    Warning,
    /// 错误。
    Error,
}

/// 一条日志。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// 单调递增的编号。
    pub id: u64,
    /// 级别。
    pub level: LogLevel,
    /// 中文内容。
    pub message: String,
    /// 产生时间（Unix 毫秒）。
    pub timestamp_ms: i64,
}

type LogListener = Arc<dyn Fn(&LogEntry) + Send + Sync>;

/// 日志中心：保存最近的日志，并把新日志通知给监听器。
#[derive(Default)]
pub struct LogHub {
    entries: Mutex<VecDeque<LogEntry>>,
    next_id: AtomicU64,
    listener: RwLock<Option<LogListener>>,
}

impl std::fmt::Debug for LogHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogHub").finish_non_exhaustive()
    }
}

impl LogHub {
    /// 新建一个空的日志中心。
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn entries(&self) -> MutexGuard<'_, VecDeque<LogEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 设置（替换）实时监听器。
    pub fn set_listener(&self, listener: impl Fn(&LogEntry) + Send + Sync + 'static) {
        *self
            .listener
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(listener));
    }

    /// 追加一条日志并通知监听器。
    pub fn push(&self, level: LogLevel, message: impl Into<String>) -> LogEntry {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default();
        let entry = {
            let mut entries = self.entries();
            let entry = LogEntry {
                id: self.next_id.fetch_add(1, Ordering::Relaxed) + 1,
                level,
                message: message.into(),
                timestamp_ms,
            };
            entries.push_back(entry.clone());
            while entries.len() > MAX_LOG_ENTRIES {
                entries.pop_front();
            }
            entry
        };
        let listener = self
            .listener
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(listener) = listener {
            listener(&entry);
        }
        entry
    }

    /// 普通信息。
    pub fn info(&self, message: impl Into<String>) {
        self.push(LogLevel::Info, message);
    }

    /// 成功。
    pub fn success(&self, message: impl Into<String>) {
        self.push(LogLevel::Success, message);
    }

    /// 警告。
    pub fn warning(&self, message: impl Into<String>) {
        self.push(LogLevel::Warning, message);
    }

    /// 错误。
    pub fn error(&self, message: impl Into<String>) {
        self.push(LogLevel::Error, message);
    }

    /// 当前保留的所有日志（从旧到新）。
    pub fn recent(&self) -> Vec<LogEntry> {
        self.entries().iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_latest_400_entries_in_order() {
        let hub = LogHub::new();
        for index in 0..450 {
            hub.info(format!("第 {index} 条"));
        }

        let recent = hub.recent();

        assert_eq!(recent.len(), MAX_LOG_ENTRIES);
        assert_eq!(recent.first().unwrap().message, "第 50 条");
        assert_eq!(recent.last().unwrap().message, "第 449 条");
        assert!(recent.windows(2).all(|pair| pair[0].id < pair[1].id));
    }

    #[test]
    fn listener_receives_new_entries_with_levels() {
        let hub = LogHub::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        hub.set_listener(move |entry| {
            sink.lock()
                .unwrap()
                .push((entry.level, entry.message.clone()))
        });

        hub.success("成功");
        hub.warning("警告");
        hub.error("错误");

        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                (LogLevel::Success, "成功".to_owned()),
                (LogLevel::Warning, "警告".to_owned()),
                (LogLevel::Error, "错误".to_owned()),
            ]
        );
    }

    #[test]
    fn serializes_to_camel_case_with_lowercase_level() {
        let hub = LogHub::new();
        hub.push(LogLevel::Success, "好");

        let json = serde_json::to_value(&hub.recent()[0]).unwrap();

        assert_eq!(json["level"], "success");
        assert_eq!(json["message"], "好");
        assert!(json["timestampMs"].as_i64().unwrap() > 0);
        assert_eq!(json["id"], 1);
    }
}
