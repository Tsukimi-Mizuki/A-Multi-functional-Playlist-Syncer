//! 按音质取链下载、NCM 导入、写入歌词/标签/封面并原子落盘。
//!
//! 对应 C# 的 `Core/Download/`（`DownloadService`、`NcmImportService`、`MediaFileHelper`）：
//!
//! - [`ncm_import`]：批量解密 `.ncm` 并写回内嵌元数据（模块 7）；
//! - [`media`] / [`fetch`]：扩展名与文件名、原子写标签、流式下载（`MediaFileHelper`）；
//! - [`pool`]：有并发上限的任务池；
//! - [`service`]：`DownloadService`；
//! - [`task`]：下载选中 / 下载缺失 / 一键同步的任务编排（挑歌、日志、结束后重新扫描曲库）。

pub mod fetch;
pub mod media;
pub mod ncm_import;
pub mod pool;
pub mod service;
pub mod task;

#[cfg(test)]
pub(crate) mod test_support;
