//! 一键同步编排：本地命中则跳过、NCM 则解密、缺失则下载。
//!
//! 对应 C# 的 `Core/Sync/`（`MusicSyncService`、`ISyncService`）。

pub mod service;

pub use service::{
    SyncItemResult, SyncOptions, SyncOutcome, SyncProgress, SyncRequest, SyncService,
    SyncSourceKind,
};
