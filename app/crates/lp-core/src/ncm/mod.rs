//! NCM 文件解密与客户端注释里的 musicId 解析（AES-128-ECB 密钥盒 + 元数据，32 KB 分块 XOR）。
//!
//! 对应 C# 的 `Core/Ncm/`：
//!
//! - [`client_comment`]：`NeteaseClientComment`（标签读取依赖它）；
//! - [`decoder`]：`NcmDecoder` / `INcmDecoder`，流式解密 `.ncm`。

pub mod client_comment;
pub mod decoder;

pub use decoder::{
    FileNcmDecoder, NcmDecoder, NcmDecryptResult, NcmError, NcmFileInfo, NcmMetadata,
};

#[cfg(test)]
pub(crate) mod synthetic;
