//! 测试辅助：按 NCM 格式的**逆运算**构造合成的 `.ncm` 文件。
//!
//! 移植自 C# `SyntheticNcmFile`，并保持与生产实现相互独立：这里按格式规范做加密（AES-128-ECB 加密密钥 / 元数据，
//! RC4 变种密钥流异或音频），被测的 `FileNcmDecoder` 按规范解密，从而验证双向一致。
//! 密钥盒的构造是按规范**另写**的一份，没有复用生产代码。

use aes::Aes128;
use aes::cipher::{BlockCipherEncrypt, KeyInit};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

pub(crate) const DEFAULT_METADATA_JSON: &str = r#"{"musicName":"测试歌曲","album":"测试专辑","artist":[["测试歌手",123],["第二歌手",456]],"bitrate":320000,"duration":215000,"format":"mp3","musicId":1234567890,"alias":["别名"]}"#;

const NETEASE_MAGIC: &[u8] = b"CTENFDAM";
const CORE_KEY: [u8; 16] = [
    0x68, 0x7A, 0x48, 0x52, 0x41, 0x6D, 0x73, 0x6F, 0x35, 0x6B, 0x49, 0x6E, 0x62, 0x61, 0x78, 0x57,
];
const MODIFY_KEY: [u8; 16] = [
    0x23, 0x31, 0x34, 0x6C, 0x6A, 0x6B, 0x5F, 0x21, 0x5C, 0x5D, 0x26, 0x30, 0x55, 0x3C, 0x27, 0x28,
];

/// 合成 NCM 的可调参数。
#[derive(Debug, Clone)]
pub(crate) struct SyntheticNcm {
    /// 内嵌封面。
    pub cover: Option<Vec<u8>>,
    /// 元数据 JSON（不含 `music:` 前缀）；`None` 或 `include_metadata == false` 时写入空元数据段。
    pub metadata_json: Option<String>,
    /// 封面帧里，封面数据之后额外填充的字节数。
    pub cover_padding: usize,
    /// 是否写元数据段。
    pub include_metadata: bool,
    /// RC4 密钥材料长度（17 字节前缀之后）。
    pub key_material_length: usize,
}

impl Default for SyntheticNcm {
    fn default() -> Self {
        Self {
            cover: None,
            metadata_json: Some(DEFAULT_METADATA_JSON.to_owned()),
            cover_padding: 0,
            include_metadata: true,
            key_material_length: 95,
        }
    }
}

impl SyntheticNcm {
    /// 用默认参数构造。
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn cover(mut self, cover: &[u8]) -> Self {
        self.cover = Some(cover.to_vec());
        self
    }

    pub(crate) fn metadata(mut self, json: &str) -> Self {
        self.metadata_json = Some(json.to_owned());
        self
    }

    pub(crate) fn no_metadata(mut self) -> Self {
        self.include_metadata = false;
        self
    }

    pub(crate) fn cover_padding(mut self, padding: usize) -> Self {
        self.cover_padding = padding;
        self
    }

    pub(crate) fn key_material_length(mut self, length: usize) -> Self {
        self.key_material_length = length;
        self
    }

    /// 生成完整的 `.ncm` 文件内容。
    pub(crate) fn build(&self, audio: &[u8]) -> Vec<u8> {
        assert!(self.key_material_length > 0);

        // 17 字节前缀之后的 key_material 才是密钥盒的种子。
        let mut key_plain: Vec<u8> = (0..17u8).map(|i| 0xA0 + i).collect();
        let key_material: Vec<u8> = (0..self.key_material_length)
            .map(|i| (i * 7 + 3) as u8)
            .collect();
        key_plain.extend_from_slice(&key_material);

        let mut encrypted_key = aes_ecb_encrypt(&pad_pkcs7(&key_plain), &CORE_KEY);
        for byte in &mut encrypted_key {
            *byte ^= 0x64;
        }

        let mut out = Vec::new();
        out.extend_from_slice(NETEASE_MAGIC);
        out.extend_from_slice(&[0x01, 0x00]);
        write_i32(&mut out, encrypted_key.len() as i32);
        out.extend_from_slice(&encrypted_key);

        match (&self.metadata_json, self.include_metadata) {
            (Some(json), true) => {
                let plain = format!("music:{json}");
                let encrypted = aes_ecb_encrypt(&pad_pkcs7(plain.as_bytes()), &MODIFY_KEY);
                let mut payload =
                    format!("163 key(Don't modify):{}", STANDARD.encode(encrypted)).into_bytes();
                for byte in &mut payload {
                    *byte ^= 0x63;
                }
                write_i32(&mut out, payload.len() as i32);
                out.extend_from_slice(&payload);
            }
            _ => write_i32(&mut out, 0),
        }

        // 4 字节 CRC + 1 字节封面版本（解密器直接跳过）。
        out.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x02]);

        let image = self.cover.as_deref().unwrap_or_default();
        write_i32(&mut out, (image.len() + self.cover_padding) as i32);
        write_i32(&mut out, image.len() as i32);
        out.extend_from_slice(image);
        out.extend(std::iter::repeat_n(0x55, self.cover_padding));

        out.extend_from_slice(&scramble_audio(audio, &key_material));
        out
    }
}

/// 按规范独立实现的密钥盒（RC4 的 KSA 变种）。
fn build_key_box(key_material: &[u8]) -> [u8; 256] {
    let mut key_box: Vec<u8> = (0..=255).collect();
    let mut key_offset = 0;
    let mut last: u8 = 0;
    for i in 0..256 {
        let swap = key_box[i];
        let c = ((u32::from(swap) + u32::from(last) + u32::from(key_material[key_offset])) & 0xFF)
            as u8;
        key_offset += 1;
        if key_offset >= key_material.len() {
            key_offset = 0;
        }
        key_box[i] = key_box[usize::from(c)];
        key_box[usize::from(c)] = swap;
        last = c;
    }
    key_box.try_into().expect("密钥盒恰好 256 字节")
}

/// 与解密同一个对合运算（异或），每 32 KB 一块、块内下标重新计数。
fn scramble_audio(audio: &[u8], key_material: &[u8]) -> Vec<u8> {
    let key_box = build_key_box(key_material);
    let mut out = audio.to_vec();
    for chunk in out.chunks_mut(32 * 1024) {
        for (i, byte) in chunk.iter_mut().enumerate() {
            let j = (i + 1) & 0xFF;
            let inner = key_box[(usize::from(key_box[j]) + j) & 0xFF];
            *byte ^= key_box[(usize::from(key_box[j]) + usize::from(inner)) & 0xFF];
        }
    }
    out
}

pub(crate) fn aes_ecb_encrypt(plain: &[u8], key: &[u8; 16]) -> Vec<u8> {
    assert_eq!(plain.len() % 16, 0);
    let aes = Aes128::new(key.into());
    let mut out = Vec::with_capacity(plain.len());
    let (blocks, _) = plain.as_chunks::<16>();
    for chunk in blocks {
        let mut block = aes::Block::from(*chunk);
        aes.encrypt_block(&mut block);
        out.extend_from_slice(&block);
    }
    out
}

pub(crate) fn pad_pkcs7(data: &[u8]) -> Vec<u8> {
    let padding = 16 - data.len() % 16;
    let mut padded = data.to_vec();
    padded.extend(std::iter::repeat_n(padding as u8, padding));
    padded
}

fn write_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// 以 `ID3` 开头的假 MP3 音频（内容为确定性的伪随机字节）。
pub(crate) fn fake_mp3_audio(length: usize) -> Vec<u8> {
    let mut audio: Vec<u8> = (0..length.max(3))
        .map(|i| (i.wrapping_mul(31).wrapping_add(7)) as u8)
        .collect();
    audio[..3].copy_from_slice(b"ID3");
    audio
}

/// 以 `fLaC` 开头的假 FLAC 音频。
pub(crate) fn fake_flac_audio(length: usize) -> Vec<u8> {
    let mut audio: Vec<u8> = (0..length.max(4))
        .map(|i| (i.wrapping_mul(17).wrapping_add(11)) as u8)
        .collect();
    audio[..4].copy_from_slice(b"fLaC");
    audio
}

/// 以 PNG 魔数开头的假封面。
pub(crate) fn fake_png_cover() -> Vec<u8> {
    let mut bytes: Vec<u8> = (0..128usize)
        .map(|i| (i.wrapping_mul(13).wrapping_add(5)) as u8)
        .collect();
    bytes[..8].copy_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    bytes
}
