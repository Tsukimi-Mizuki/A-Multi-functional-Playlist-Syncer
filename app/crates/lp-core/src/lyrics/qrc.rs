//! QQ 音乐 QRC 逐字歌词解密：非标准 3DES 变体 + zlib。对应 C# `QqTripleDes` 与 `QrcDecryptor`。
//!
//! 云端 QRC 的加密流程：明文（`<Lyric_1 LyricType="1" LyricContent="..."/>` 或纯文本行）→ zlib 压缩 →
//! 3DES-ECB（24 字节固定密钥、无填充，每 8 字节独立处理）→ 十六进制字符串。解密时反向执行；
//! 长度不是 8 的倍数的尾部字节直接丢弃，与 3DES NoPadding 的块语义一致。
//!
//! 该 DES 变体与教科书 DES 的差异只在字节与位的编号约定（32 位字按小端存放），所以**不能**用标准 DES 实现代替：
//! 已知向量 key `133457799BBCDFF1`、plain `0123456789ABCDEF` → `F63A972CE52AA327`（教科书结果是 `85E813540F0AB405`）。
//! 这里按位运算逐轮移植，以便与 QQ 音乐服务端产生的密文对齐。

use super::zlib::inflate_lenient;

/// QRC 固定密钥（24 字节）。
pub const QRC_KEY: &[u8; 24] = b"!@#)(*$%123ZXC!@!@#)(NHL";

const SBOX: [[u8; 64]; 8] = [
    // S1
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, //
        0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12, 11, 9, 5, 3, 8, //
        4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, //
        15, 12, 8, 2, 4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    // S2（第二行里的两个 15 是该变体自带的，不要“纠正”）
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, //
        3, 13, 4, 7, 15, 2, 8, 15, 12, 0, 1, 10, 6, 9, 11, 5, //
        0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, //
        13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    // S3
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, //
        13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1, //
        13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, //
        1, 10, 13, 0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    // S4（最后一行里的两个 10 同样是变体自带的）
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, //
        13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2, 12, 1, 10, 14, 9, //
        10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, //
        3, 15, 0, 6, 10, 10, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    // S5
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, //
        14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15, 10, 3, 9, 8, 6, //
        4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, //
        11, 8, 12, 7, 1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    // S6
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, //
        10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13, 14, 0, 11, 3, 8, //
        9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, //
        4, 3, 2, 12, 9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    // S7
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, //
        13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5, 12, 2, 15, 8, 6, //
        1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, //
        6, 11, 13, 8, 1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    // S8
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, //
        1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6, 11, 0, 14, 9, 2, //
        7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, //
        2, 1, 14, 7, 4, 10, 8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

const KEY_PERM_C: [usize; 28] = [
    56, 48, 40, 32, 24, 16, 8, 0, 57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59,
    51, 43, 35,
];

const KEY_PERM_D: [usize; 28] = [
    62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29, 21, 13, 5, 60, 52, 44, 36, 28, 20, 12, 4,
    27, 19, 11, 3,
];

const KEY_COMPRESSION: [usize; 48] = [
    13, 16, 10, 23, 0, 4, 2, 27, 14, 5, 20, 9, 22, 18, 11, 3, 25, 7, 15, 6, 26, 19, 12, 1, 40, 51,
    30, 36, 46, 54, 29, 39, 50, 44, 32, 47, 43, 48, 38, 55, 33, 52, 45, 41, 49, 35, 28, 31,
];

const PERMUTATION: [u32; 32] = [
    15, 6, 19, 20, 28, 11, 27, 16, 0, 14, 22, 25, 4, 17, 30, 9, 1, 7, 23, 13, 31, 26, 2, 8, 18, 12,
    29, 5, 21, 10, 3, 24,
];

/// 一个 DES 轮密钥组：16 轮，每轮 6 个字节（放在 u32 里）。
type Schedule = [[u32; 6]; 16];

/// 3DES 的三组密钥表。
type TripleSchedule = [Schedule; 3];

/// 取 `data` 中第 `bit` 位（按该变体的编号），放到 `shift` 位置。
fn bit_num(data: &[u8], bit: usize, shift: u32) -> u32 {
    let byte = data[(bit / 32) * 4 + 3 - (bit % 32) / 8];
    u32::from((byte >> (7 - (bit % 8))) & 1) << shift
}

fn bit_num_intr(value: u32, bit: usize, shift: u32) -> u32 {
    ((value >> (31 - bit)) & 1) << shift
}

fn bit_num_intl(value: u32, bit: u32, shift: u32) -> u32 {
    ((value << bit) & 0x8000_0000) >> shift
}

fn sbox_bit(value: u32) -> usize {
    ((value & 32) | ((value & 31) >> 1) | ((value & 1) << 4)) as usize
}

fn key_schedule(key: &[u8], decrypt: bool) -> Schedule {
    const SHIFTS: [u32; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];
    let mut schedule = [[0_u32; 6]; 16];
    let mut c = 0_u32;
    let mut d = 0_u32;
    for i in 0..28 {
        c |= bit_num(key, KEY_PERM_C[i], 31 - i as u32);
        d |= bit_num(key, KEY_PERM_D[i], 31 - i as u32);
    }

    for (i, shift) in SHIFTS.iter().copied().enumerate() {
        c = ((c << shift) | (c >> (28 - shift))) & 0xffff_fff0;
        d = ((d << shift) | (d >> (28 - shift))) & 0xffff_fff0;
        let target = if decrypt { 15 - i } else { i };
        let mut sub_keys = [0_u32; 6];
        for j in 0..24 {
            sub_keys[j / 8] |= bit_num_intr(c, KEY_COMPRESSION[j], 7 - (j % 8) as u32);
        }
        for j in 24..48 {
            sub_keys[j / 8] |= bit_num_intr(d, KEY_COMPRESSION[j] - 27, 7 - (j % 8) as u32);
        }
        schedule[target] = sub_keys;
    }
    schedule
}

fn triple_des_key_setup(key: &[u8; 24], decrypt: bool) -> TripleSchedule {
    if decrypt {
        [
            key_schedule(&key[16..24], true),
            key_schedule(&key[8..16], false),
            key_schedule(&key[0..8], true),
        ]
    } else {
        [
            key_schedule(&key[0..8], false),
            key_schedule(&key[8..16], true),
            key_schedule(&key[16..24], false),
        ]
    }
}

fn initial_permutation(data: &[u8; 8]) -> (u32, u32) {
    let mut left = 0_u32;
    let mut right = 0_u32;
    for i in 0..32_usize {
        let shift = 31 - i as u32;
        let group = i / 8;
        let within = i % 8;
        left |= bit_num(data, 57 - (8 * within) + (2 * group), shift);
        right |= bit_num(data, 56 - (8 * within) + (2 * group), shift);
    }
    (left, right)
}

fn inverse_permutation(left: u32, right: u32) -> [u8; 8] {
    let mut data = [0_u8; 8];
    for (index, slot) in data.iter_mut().enumerate() {
        let source_bit = (index + 4) % 8;
        let mut value = 0_u32;
        for term in 0..8_usize {
            let source = if term % 2 == 0 { right } else { left };
            value |= bit_num_intr(source, source_bit + (8 * (term / 2)), 7 - term as u32);
        }
        *slot = value as u8;
    }
    data
}

fn feistel(state: u32, key: &[u32; 6]) -> u32 {
    let t1 = bit_num_intl(state, 31, 0)
        | ((state & 0xf000_0000) >> 1)
        | bit_num_intl(state, 4, 5)
        | bit_num_intl(state, 3, 6)
        | ((state & 0x0f00_0000) >> 3)
        | bit_num_intl(state, 8, 11)
        | bit_num_intl(state, 7, 12)
        | ((state & 0x00f0_0000) >> 5)
        | bit_num_intl(state, 12, 17)
        | bit_num_intl(state, 11, 18)
        | ((state & 0x000f_0000) >> 7)
        | bit_num_intl(state, 16, 23);

    let t2 = bit_num_intl(state, 15, 0)
        | ((state & 0x0000_f000) << 15)
        | bit_num_intl(state, 20, 5)
        | bit_num_intl(state, 19, 6)
        | ((state & 0x0000_0f00) << 13)
        | bit_num_intl(state, 24, 11)
        | bit_num_intl(state, 23, 12)
        | ((state & 0x0000_00f0) << 11)
        | bit_num_intl(state, 28, 17)
        | bit_num_intl(state, 27, 18)
        | ((state & 0x0000_000f) << 9)
        | bit_num_intl(state, 0, 23);

    let l0 = ((t1 >> 24) & 0xff) ^ key[0];
    let l1 = ((t1 >> 16) & 0xff) ^ key[1];
    let l2 = ((t1 >> 8) & 0xff) ^ key[2];
    let l3 = ((t2 >> 24) & 0xff) ^ key[3];
    let l4 = ((t2 >> 16) & 0xff) ^ key[4];
    let l5 = ((t2 >> 8) & 0xff) ^ key[5];

    let s = (u32::from(SBOX[0][sbox_bit(l0 >> 2)]) << 28)
        | (u32::from(SBOX[1][sbox_bit(((l0 & 3) << 4) | (l1 >> 4))]) << 24)
        | (u32::from(SBOX[2][sbox_bit(((l1 & 0xf) << 2) | (l2 >> 6))]) << 20)
        | (u32::from(SBOX[3][sbox_bit(l2 & 0x3f)]) << 16)
        | (u32::from(SBOX[4][sbox_bit(l3 >> 2)]) << 12)
        | (u32::from(SBOX[5][sbox_bit(((l3 & 3) << 4) | (l4 >> 4))]) << 8)
        | (u32::from(SBOX[6][sbox_bit(((l4 & 0xf) << 2) | (l5 >> 6))]) << 4)
        | u32::from(SBOX[7][sbox_bit(l5 & 0x3f)]);

    let mut result = 0_u32;
    for (i, permuted) in PERMUTATION.iter().enumerate() {
        result |= bit_num_intl(s, *permuted, i as u32);
    }
    result
}

fn crypt_block(block: &[u8; 8], schedule: &Schedule) -> [u8; 8] {
    let (mut left, mut right) = initial_permutation(block);
    for round_key in &schedule[..15] {
        let previous = right;
        right = feistel(right, round_key) ^ left;
        left = previous;
    }
    left ^= feistel(right, &schedule[15]);
    inverse_permutation(left, right)
}

fn crypt_ecb(key: &[u8; 24], input: &[u8], length: usize, decrypt: bool) -> Vec<u8> {
    let schedule = triple_des_key_setup(key, decrypt);
    let mut output = vec![0_u8; length];
    for offset in (0..length).step_by(8) {
        let mut block = [0_u8; 8];
        block.copy_from_slice(&input[offset..offset + 8]);
        for round in &schedule {
            block = crypt_block(&block, round);
        }
        output[offset..offset + 8].copy_from_slice(&block);
    }
    output
}

/// 按 QRC 语义解密：每 8 字节一块，共 `length` 字节（调用方保证 `length` 是 8 的倍数且不超过 `cipher.len()`）。
pub fn decrypt(key: &[u8; 24], cipher: &[u8], length: usize) -> Vec<u8> {
    crypt_ecb(key, cipher, length, true)
}

/// 按 QRC 语义加密（与 [`decrypt`] 对称，用于自检与测试）。
pub fn encrypt(key: &[u8; 24], plain: &[u8], length: usize) -> Vec<u8> {
    crypt_ecb(key, plain, length, false)
}

/// 单 DES 加密一个 8 字节块（仅用于校验实现与已知向量一致）。
pub fn encrypt_block(key: &[u8; 8], block: &[u8; 8]) -> [u8; 8] {
    crypt_block(block, &key_schedule(key, false))
}

// ===== QrcDecryptor =====

/// 判断文本是否可能是加密后的 QRC（纯十六进制、长度为 16 的倍数）。
pub fn looks_encrypted(value: Option<&str>) -> bool {
    let Some(value) = value.filter(|text| !text.trim().is_empty()) else {
        return false;
    };
    let text = value.trim();
    text.len() >= 16 && text.len() % 16 == 0 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 解密 QRC 十六进制密文；失败返回 `None`。
///
/// 结果已把 `\r\n` / `\r` 统一成 `\n`，并去掉末尾的 NUL（块填充）。
pub fn try_decrypt(encrypted_hex: Option<&str>) -> Option<String> {
    if !looks_encrypted(encrypted_hex) {
        return None;
    }
    let cipher = decode_hex(encrypted_hex?.trim())?;

    let length = cipher.len() - cipher.len() % 8;
    if length == 0 {
        return None;
    }
    let plain = decrypt(QRC_KEY, &cipher, length);

    let inflated = inflate_lenient(&plain)?;
    let decoded = String::from_utf8_lossy(&inflated);
    let text = decoded.trim_end_matches('\0');
    if text.trim().is_empty() {
        return None;
    }
    Some(text.replace("\r\n", "\n").replace('\r', "\n"))
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let (pairs, remainder) = bytes.as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    pairs
        .iter()
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some((high * 16 + low) as u8)
        })
        .collect()
}

#[cfg(test)]
mod tests;
