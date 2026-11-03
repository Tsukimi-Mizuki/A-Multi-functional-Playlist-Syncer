//! 按音质取链下载、NCM 导入、写入歌词/标签/封面并原子落盘。
//!
//! 对应 C# 的 `Core/Download/`（`DownloadService`、`NcmImportService`、`MediaFileHelper`）。
//! 模块 7 先移植 [`ncm_import`]（批量解密 + 写回内嵌元数据）；下载与同步在模块 9。

pub mod ncm_import;
