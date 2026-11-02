//! NCM 文件解密与客户端注释里的 musicId 解析（AES-128-ECB 密钥盒 + 元数据，32 KB 分块 XOR）。
//!
//! 对应 C# 的 `Core/Ncm/`（`NcmDecoder`、`NeteaseClientComment`）。
