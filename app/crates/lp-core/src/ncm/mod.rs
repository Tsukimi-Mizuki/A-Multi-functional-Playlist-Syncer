//! NCM 文件解密与客户端注释里的 musicId 解析（AES-128-ECB 密钥盒 + 元数据，32 KB 分块 XOR）。
//!
//! 对应 C# 的 `Core/Ncm/`（`NcmDecoder`、`NeteaseClientComment`）。
//! 模块 6 先移植 [`client_comment`]（标签读取依赖它）；NCM 解密本身在模块 7。

pub mod client_comment;
