//! 移植自 C# `QrcDecryptorTests`，外加密文往返与错误输入用例。

use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use super::*;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn from_hex(text: &str) -> Vec<u8> {
    decode_hex(text).expect("测试用十六进制合法")
}

/// 与 C# 测试的 `Encrypt` 同一流程：zlib 压缩 → 补零到 8 的倍数 → QRC 3DES 加密 → 小写十六进制。
fn encrypt_text(plain: &str) -> String {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(plain.as_bytes()).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut padded = compressed;
    padded.resize(padded.len().div_ceil(8) * 8, 0);
    let length = padded.len();
    hex(&encrypt(QRC_KEY, &padded, length))
}

#[test]
fn looks_encrypted_accepts_cipher_like_hex() {
    assert!(looks_encrypted(Some("0123456789abcdef")));
    assert!(looks_encrypted(Some("0123456789abcdef0123456789abcdef")));
    assert!(looks_encrypted(Some("  0123456789ABCDEF\n")));
}

#[test]
fn looks_encrypted_rejects_non_cipher_input() {
    for value in [
        None,
        Some(""),
        Some("   "),
        Some("hello world"),
        Some("[00:12.34]plain lrc line"),
        Some("0123456789abc"),
        Some("0123456789abcdeZ"),
        // 十六进制但不足一个 16 字符块 / 不是 16 的倍数。
        Some("0123456789abcde"),
        Some("0123456789abcdef0"),
    ] {
        assert!(!looks_encrypted(value), "{value:?} 不应被识别为密文");
    }
}

#[test]
fn try_decrypt_round_trips_triple_des_plus_zlib() {
    let plain = "<Lyric_1 LyricType=\"1\" LyricContent=\"[0,500]Hello(0,250) world(250,250)\n[500,400]Second(500,400)\"/>";

    assert_eq!(
        try_decrypt(Some(&encrypt_text(plain))).as_deref(),
        Some(plain)
    );
}

#[test]
fn try_decrypt_normalizes_line_endings_and_drops_trailing_nul() {
    let plain = "[0,500]line one\r\n[500,400]line two\0";

    assert_eq!(
        try_decrypt(Some(&encrypt_text(plain))).as_deref(),
        Some("[0,500]line one\n[500,400]line two")
    );
}

#[test]
fn encrypt_block_matches_the_qq_des_variant_vector() {
    // 该向量取自 LDDC 的 tripledes 实现：与教科书 DES 的字节 / 位约定不同，锁定变体行为。
    let key: [u8; 8] = from_hex("133457799BBCDFF1").try_into().unwrap();
    let block: [u8; 8] = from_hex("0123456789ABCDEF").try_into().unwrap();

    let cipher = encrypt_block(&key, &block);

    assert_eq!(hex(&cipher).to_uppercase(), "F63A972CE52AA327");
    // 教科书 DES 的结果是 85E813540F0AB405，两者必须不同。
    assert_ne!(hex(&cipher).to_uppercase(), "85E813540F0AB405");
}

#[test]
fn try_decrypt_returns_none_for_garbage_cipher() {
    assert_eq!(try_decrypt(Some(&"0".repeat(32))), None);
}

#[test]
fn try_decrypt_returns_none_for_plain_lrc() {
    assert_eq!(try_decrypt(Some("[00:01.00]not encrypted")), None);
    assert_eq!(try_decrypt(None), None);
    assert_eq!(try_decrypt(Some("")), None);
}

#[test]
fn triple_des_encrypt_then_decrypt_is_the_identity_for_arbitrary_blocks() {
    let data: Vec<u8> = (0..64_u32).map(|i| (i * 37 + 11) as u8).collect();

    let cipher = encrypt(QRC_KEY, &data, data.len());
    assert_ne!(cipher, data);
    assert_eq!(decrypt(QRC_KEY, &cipher, cipher.len()), data);

    // ECB：相同明文块得到相同密文块。
    let repeated = [data[..8].to_vec(), data[..8].to_vec()].concat();
    let cipher = encrypt(QRC_KEY, &repeated, 16);
    assert_eq!(cipher[..8], cipher[8..]);
}

#[test]
fn trailing_bytes_that_do_not_fill_a_block_are_dropped() {
    let plain = "[0,500]一行(0,500)";
    let mut hex_text = encrypt_text(plain);
    // 追加 4 个十六进制字符 = 2 个字节的尾巴（长度不再是 16 的倍数，直接被识别为非密文）。
    hex_text.push_str("abcd");
    assert_eq!(try_decrypt(Some(&hex_text)), None);
}

#[test]
fn whitespace_around_the_hex_is_ignored() {
    let plain = "[0,500]一行(0,500)";
    let padded = format!("\n  {}  \r\n", encrypt_text(plain));

    assert_eq!(try_decrypt(Some(&padded)).as_deref(), Some(plain));
}

#[test]
fn uppercase_hex_decrypts_identically() {
    let plain = "[0,500]一行(0,500)";

    assert_eq!(
        try_decrypt(Some(&encrypt_text(plain).to_uppercase())).as_deref(),
        Some(plain)
    );
}

#[test]
fn a_compressed_but_blank_payload_is_rejected() {
    assert_eq!(try_decrypt(Some(&encrypt_text("   \0\0"))), None);
}

#[test]
fn decrypted_qrc_feeds_the_lyric_parser() {
    use crate::lyrics::{RawLyrics, parser};

    let plain =
        "<Lyric_1 LyricType=\"1\" LyricContent=\"[0,1000]Tom &amp; Jerry(0,500) 你(500,500)\"/>";
    let decrypted = try_decrypt(Some(&encrypt_text(plain))).unwrap();

    let document = parser::parse(&RawLyrics {
        qrc: Some(decrypted),
        ..RawLyrics::default()
    });

    assert_eq!(document.lines.len(), 1);
    assert_eq!(document.lines[0].text, "Tom & Jerry 你");
    assert!(document.has_word_level());
}
