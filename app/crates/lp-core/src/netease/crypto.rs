//! 网易云 weapi 请求体加密。对应 C# 的 `WeapiCrypto`，算法一致：
//!
//! `params = Base64(AES-128-CBC(Base64(AES-128-CBC(业务 JSON, 固定 key)), 随机 secret))`，
//! `encSecKey` = 教材式 RSA（无填充，即 `m^e mod n`）加密“反转后的 secret”，输出 256 位小写 hex。

use aes::Aes128;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockModeEncrypt, KeyIvInit};
use num_bigint::BigUint;
use rand::RngExt;

/// weapi 内层固定 AES key。
pub const INNER_KEY: &str = "0CoJUm6Qyw8W8jud";
/// 两层 AES-CBC 共用的初始向量。
pub const IV: &str = "0102030405060708";
/// 网易云固定 RSA 模数（2048 bit）；与 C# 一样带一个前导 `00`。
pub const MODULUS_HEX: &str = "00e0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b725152b3ab17a876aea8a5aa76d2e417629ec4ee341f56135fccf695280104e0312ecbda92557c93870114af6c9d05c4f7f0c3685b7a46bee255932575cce10b424d813cfe4875d3e82047b97ddef52741d546b8e289dc6935b3ece0462db0a22b8e7";
/// 网易云固定 RSA 公钥指数。
pub const EXPONENT_HEX: &str = "010001";
/// 随机 secret 长度（16 个 `[A-Za-z0-9]` 字符）。
pub const SECRET_LENGTH: usize = 16;

const ENC_SEC_KEY_HEX_LENGTH: usize = 256;
const SECRET_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

type Aes128CbcEnc = cbc::Encryptor<Aes128>;

/// 生成一个符合网易云要求的 16 位随机 secret（`[A-Za-z0-9]`）。
pub fn create_secret() -> String {
    let mut rng = rand::rng();
    (0..SECRET_LENGTH)
        .map(|_| char::from(SECRET_ALPHABET[rng.random_range(0..SECRET_ALPHABET.len())]))
        .collect()
}

/// 把业务 JSON 加密成 weapi 表单需要的 `(params, encSecKey)`。
///
/// `secret` 必须是 16 个字符，否则返回 `None`（C# 在这里抛 `ArgumentException`）。
pub fn encrypt(business_json: &str, secret: &str) -> Option<(String, String)> {
    if secret.len() != SECRET_LENGTH {
        return None;
    }
    let inner = aes_encrypt_to_base64(business_json.as_bytes(), INNER_KEY.as_bytes());
    let outer = aes_encrypt_to_base64(inner.as_bytes(), secret.as_bytes());
    Some((outer, rsa_encrypt_hex(secret)))
}

/// AES-128-CBC（PKCS#7）加密并返回 Base64。key 必须是 16 字节。
fn aes_encrypt_to_base64(plain: &[u8], key: &[u8]) -> String {
    let encryptor = Aes128CbcEnc::new_from_slices(key, IV.as_bytes())
        .expect("weapi 的 AES key 与 IV 长度固定为 16 字节");
    STANDARD.encode(encryptor.encrypt_padded_vec::<Pkcs7>(plain))
}

/// 两层 AES-128-CBC 解密，仅供单元测试验证加密结果使用。
#[cfg(test)]
pub(crate) fn decrypt_payload(params_base64: &str, secret: &str) -> String {
    use cbc::cipher::BlockModeDecrypt;

    fn decrypt(cipher_base64: &str, key: &[u8]) -> Vec<u8> {
        let cipher = STANDARD.decode(cipher_base64).expect("合法 Base64");
        cbc::Decryptor::<Aes128>::new_from_slices(key, IV.as_bytes())
            .expect("key 与 IV 长度正确")
            .decrypt_padded_vec::<Pkcs7>(&cipher)
            .expect("合法 PKCS#7 填充")
    }

    let inner = decrypt(params_base64, secret.as_bytes());
    let inner = String::from_utf8(inner).expect("内层应是 Base64 文本");
    String::from_utf8(decrypt(&inner, INNER_KEY.as_bytes())).expect("业务 JSON 应是 UTF-8")
}

/// 教材式 RSA（无填充）加密 secret，输出 256 位小写 hex。
pub fn rsa_encrypt_hex(secret: &str) -> String {
    // 网易云要求把 secret 逐字节反转后再做 RSA（服务端会反转回来）。
    let reversed: Vec<u8> = secret.bytes().rev().collect();
    let message = BigUint::from_bytes_be(&reversed);
    let modulus = BigUint::parse_bytes(MODULUS_HEX.as_bytes(), 16).expect("固定模数是合法 hex");
    let exponent = BigUint::parse_bytes(EXPONENT_HEX.as_bytes(), 16).expect("固定指数是合法 hex");
    let cipher = message.modpow(&exponent, &modulus);
    // `to_str_radix(16)` 与 JS 的 `BigInt.toString(16)` 一样不带前导 0，这里补齐到 256 位。
    format!(
        "{:0>width$}",
        cipher.to_str_radix(16),
        width = ENC_SEC_KEY_HEX_LENGTH
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // 参考向量由独立实现（Node.js crypto + BigInt）生成，与 C# `WeapiCryptoTests` 相同，
    // 锁定 AES / 双次加密 / RSA 全链路。
    const REFERENCE_JSON: &str = r#"{"csrf_token":"","uid":"1"}"#;
    const REFERENCE_SECRET: &str = "0123456789abcdef";
    const REFERENCE_PARAMS: &str =
        "echuH06dDs7OxMA2egejtndmqu1aYuHshFuQMQCaqlxZeX1+XSQpJU0kXp3uWfRV";
    const REFERENCE_ENC_SEC_KEY: &str = "35701388baf89fed412e11269b9c76625d095ecaf17f03fa018abe19ea2d38b949debf242ee39a71ca1f6cda71b1b86a45aa909ee27f7e78e267d34e732f0de948206c3340a788d0003372183e2f753c1f78b66ac23d134ac1fc9b993156520ea826b8aa89a962d4491b4b8d7e08738e1da9b07aa39bf4a7ef0b1c210728cd52";

    #[test]
    fn encrypt_matches_independent_reference_implementation() {
        let (params, enc_sec_key) = encrypt(REFERENCE_JSON, REFERENCE_SECRET).unwrap();
        assert_eq!(params, REFERENCE_PARAMS);
        assert_eq!(enc_sec_key, REFERENCE_ENC_SEC_KEY);
    }

    #[test]
    fn encrypt_matches_second_independent_vector() {
        let (params, enc_sec_key) =
            encrypt(r#"{"csrf_token":"csrf123","id":42}"#, "Zz9AaBbCcDdEeFfG").unwrap();
        assert_eq!(
            params,
            "Q3pZQ8xAhNJnBJgOUdQz1jwOVFR8dSzgq58qXtA66cHu/2GBxCYvvBEY201jheFLoR4yV/QrTnhU0Ns0nObaNHyX8NVYjkRKsJf2dvLEIUc="
        );
        assert_eq!(
            enc_sec_key,
            "71c93ba0b9f0fc3dab23882d21a7ea8e199a54b2166e1e6d5412ddf04a007e9a0a22d498c725598892e3695d6d03c43b63971aec5bf84fe21c6492acfbc18586c929aa6b78f4d8b31c6061ee1a38c0f66423c0071426219a50a4b5a17088d09f255b581c6619023c8937d7869979c153b23029202071209a03a2d8bb0503197c"
        );
    }

    #[test]
    fn decrypt_payload_round_trips_weapi_plaintext() {
        let (params, _) = encrypt(REFERENCE_JSON, REFERENCE_SECRET).unwrap();
        assert_eq!(decrypt_payload(&params, REFERENCE_SECRET), REFERENCE_JSON);
    }

    #[test]
    fn rsa_encrypt_hex_is_256_lowercase_hex_characters() {
        for secret in [
            REFERENCE_SECRET,
            "abcdefghijklmnop",
            "0123456789ABCDEF",
            "Zz9AaBbCcDdEeFfG",
        ] {
            let encrypted = rsa_encrypt_hex(secret);
            assert_eq!(encrypted.len(), 256);
            assert!(
                encrypted
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            );
        }
    }

    #[test]
    fn rsa_encrypt_hex_is_deterministic_for_same_secret() {
        assert_eq!(
            rsa_encrypt_hex(REFERENCE_SECRET),
            rsa_encrypt_hex(REFERENCE_SECRET)
        );
    }

    #[test]
    fn create_secret_returns_16_alphanumeric_characters() {
        let secrets: Vec<String> = (0..32).map(|_| create_secret()).collect();
        for secret in &secrets {
            assert_eq!(secret.len(), 16);
            assert!(secret.bytes().all(|b| b.is_ascii_alphanumeric()));
        }
        let mut distinct = secrets.clone();
        distinct.sort();
        distinct.dedup();
        assert!(distinct.len() > 1, "随机 secret 不应重复");
    }

    #[test]
    fn encrypt_rejects_non_16_character_secret() {
        assert!(encrypt(REFERENCE_JSON, "too-short").is_none());
        assert!(encrypt(REFERENCE_JSON, "12345678901234567").is_none());
    }

    #[test]
    fn modulus_is_2048_bit_netease_key() {
        assert_eq!(MODULUS_HEX.len(), 258); // 2048 bit = 256 hex + 前导 00
        assert!(MODULUS_HEX.starts_with("00e0b509f6"));
        assert!(MODULUS_HEX.ends_with("b0a22b8e7"));
        assert_eq!(EXPONENT_HEX, "010001");
        assert_eq!(SECRET_LENGTH, 16);
    }
}
