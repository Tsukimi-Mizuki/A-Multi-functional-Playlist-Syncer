//! 网易云客户端写进标签注释的 `163 key(Don't modify):<base64>` 元数据。
//!
//! 对应 C# `NeteaseClientComment`。客户端下载或解密出来的音频不带本程序的 `LunaticPrincess:<id>` 标记，
//! 但注释里保留了这段加密 JSON（明文为 `music:{...}`，AES-128-ECB + PKCS7，密钥与 NCM 元数据相同）。
//! 能还原 `musicId` 时，本地曲库对比就可以按 ID 精确命中。
//!
//! 只读注释，不改写任何标签；格式不符或数据损坏时返回 `None`，不影响其它标签读取。

use aes::Aes128;
use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use base64::Engine;
use base64::alphabet::STANDARD as STANDARD_ALPHABET;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

/// 网易云客户端注释前缀。
pub const METADATA_PREFIX: &str = "163 key(Don't modify):";

/// 与 NCM 元数据相同的 AES 密钥（模块 7 的 NCM 解密也用它）。
pub(crate) const MODIFY_KEY: [u8; 16] = [
    0x23, 0x31, 0x34, 0x6C, 0x6A, 0x6B, 0x5F, 0x21, 0x5C, 0x5D, 0x26, 0x30, 0x55, 0x3C, 0x27, 0x28,
];

const MUSIC_PREFIX: &str = "music:";

/// 与 .NET `Convert.FromBase64String` 一致：必须有规范填充，但容忍末尾多余的比特位。
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &STANDARD_ALPHABET,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::RequireCanonical)
        .with_decode_allow_trailing_bits(true),
);

/// 解密注释中的元数据，返回 `music:` 之后的 JSON 文本；不是该格式或损坏时返回 `None`。
pub fn try_decrypt(comment: Option<&str>) -> Option<String> {
    let comment = comment.filter(|c| !c.trim().is_empty())?;
    let index = comment.find(METADATA_PREFIX)?;
    let payload = comment[index + METADATA_PREFIX.len()..].trim();
    let length = payload
        .char_indices()
        .find(|&(_, c)| !is_base64_char(c))
        .map_or(payload.len(), |(i, _)| i);
    if length == 0 {
        return None;
    }

    let cipher = BASE64.decode(&payload[..length]).ok()?;
    let plain = aes_ecb_decrypt(&cipher, &MODIFY_KEY)?;
    let text = String::from_utf8_lossy(&plain);
    let text = text.trim_end_matches(['\0', ' ', '\r', '\n', '\t']);
    if text.starts_with('{') {
        return Some(text.to_owned());
    }
    text.strip_prefix(MUSIC_PREFIX).map(str::to_owned)
}

/// 从注释还原网易云歌曲 ID；注释不带该元数据，或 ID 不是正整数时返回 `None`。
pub fn try_read_music_id(comment: Option<&str>) -> Option<i64> {
    let json = try_decrypt(comment)?;
    let document: serde_json::Value = serde_json::from_str(&json).ok()?;
    let id = document.as_object()?.get("musicId")?;
    let parsed = match id {
        serde_json::Value::Number(number) => number.as_i64()?,
        serde_json::Value::String(text) => text.trim().parse::<i64>().ok()?,
        _ => return None,
    };
    (parsed > 0).then_some(parsed)
}

/// 按客户端格式构造注释内容（加密方向，供测试与迁移使用）。
pub fn build_comment(metadata_json: &str) -> String {
    let plain = format!("{MUSIC_PREFIX}{metadata_json}");
    let cipher = aes_ecb_encrypt(plain.as_bytes(), &MODIFY_KEY);
    format!("{METADATA_PREFIX}{}", BASE64.encode(cipher))
}

/// 对应 C# 的 `char.IsLetterOrDigit(c) || c is '+' or '/' or '='`（Unicode 字母数字也算，随后由 Base64 解码失败兜底）。
fn is_base64_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || (!c.is_ascii() && c.is_alphanumeric())
        || matches!(c, '+' | '/' | '=')
}

/// AES-128-ECB + PKCS#7 解密；长度不是 16 的倍数、为空或填充不合法时返回 `None`。
fn aes_ecb_decrypt(cipher: &[u8], key: &[u8; 16]) -> Option<Vec<u8>> {
    if cipher.is_empty() || !cipher.len().is_multiple_of(16) {
        return None;
    }
    let aes = Aes128::new(key.into());
    let mut plain = Vec::with_capacity(cipher.len());
    let (blocks, _) = cipher.as_chunks::<16>();
    for chunk in blocks {
        let mut block = aes::Block::from(*chunk);
        aes.decrypt_block(&mut block);
        plain.extend_from_slice(&block);
    }
    let pad = usize::from(*plain.last()?);
    if pad == 0 || pad > 16 || pad > plain.len() {
        return None;
    }
    if plain[plain.len() - pad..]
        .iter()
        .any(|&b| usize::from(b) != pad)
    {
        return None;
    }
    plain.truncate(plain.len() - pad);
    Some(plain)
}

fn aes_ecb_encrypt(plain: &[u8], key: &[u8; 16]) -> Vec<u8> {
    let aes = Aes128::new(key.into());
    let pad = 16 - plain.len() % 16;
    let mut padded = plain.to_vec();
    padded.resize(plain.len() + pad, pad as u8);
    let mut out = Vec::with_capacity(padded.len());
    let (blocks, _) = padded.as_chunks::<16>();
    for chunk in blocks {
        let mut block = aes::Block::from(*chunk);
        aes.encrypt_block(&mut block);
        out.extend_from_slice(&block);
    }
    out
}

#[cfg(test)]
mod tests;
