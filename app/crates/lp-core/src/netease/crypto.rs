//! 网易云请求体加密：weapi 与 eapi。
//!
//! weapi 对应 C# 的 `WeapiCrypto`，算法一致：
//! `params = Base64(AES-128-CBC(Base64(AES-128-CBC(业务 JSON, 固定 key)), 随机 secret))`，
//! `encSecKey` = 教材式 RSA（无填充，即 `m^e mod n`）加密“反转后的 secret”，输出 256 位小写 hex。
//!
//! eapi（桌面 / 移动客户端用的接口，目前只给搜索用）：
//! `params = HEX(AES-128-ECB(PKCS#7, "<路径>-36cd479b6b5-<业务 JSON>-36cd479b6b5-<摘要>"))`（大写），
//! 摘要 = `MD5("nobody<路径>use<业务 JSON>md5forencrypt")`（小写 hex），固定 key `e82ckenh8dichen8`。

use aes::cipher::{BlockCipherEncrypt, BlockModeEncrypt, KeyInit, KeyIvInit};
use aes::{Aes128, Block};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use cbc::cipher::block_padding::Pkcs7;
use md5::{Digest, Md5};
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

/// eapi 固定 AES key（AES-128-ECB）。
pub const EAPI_KEY: [u8; 16] = *b"e82ckenh8dichen8";
/// eapi 明文里分隔“路径 / 业务 JSON / 摘要”的固定字符串。
const EAPI_SEPARATOR: &str = "-36cd479b6b5-";
const AES_BLOCK_SIZE: usize = 16;

/// 把业务 JSON 加密成 eapi 表单需要的 `params`（大写 hex）。
///
/// `api_path` 是**网页风格**的接口路径（如 `/api/cloudsearch/pc`，不是实际请求用的 `/eapi/...`）：
/// 摘要与明文里用的是它，请求 URL 才是把 `/api/` 换成 `/eapi/` 的结果。
pub fn eapi_encrypt(api_path: &str, business_json: &str) -> String {
    let digest = Md5::digest(format!("nobody{api_path}use{business_json}md5forencrypt").as_bytes());
    let plain = format!(
        "{api_path}{EAPI_SEPARATOR}{business_json}{EAPI_SEPARATOR}{}",
        hex_lower(&digest)
    );
    hex_upper(&ecb_encrypt_pkcs7(&EAPI_KEY, plain.as_bytes()))
}

/// AES-128-ECB（PKCS#7）加密。ECB 只用于复现网易云 eapi 的固定格式，不要用在别处。
fn ecb_encrypt_pkcs7(key: &[u8; 16], plain: &[u8]) -> Vec<u8> {
    let cipher = Aes128::new(&(*key).into());
    let padding = AES_BLOCK_SIZE - plain.len() % AES_BLOCK_SIZE;
    let mut padded = plain.to_vec();
    padded.resize(plain.len() + padding, padding as u8);
    let mut out = Vec::with_capacity(padded.len());
    let (blocks, _) = padded.as_chunks::<AES_BLOCK_SIZE>();
    for chunk in blocks {
        let mut block = Block::from(*chunk);
        cipher.encrypt_block(&mut block);
        out.extend_from_slice(&block);
    }
    out
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

/// 解开 eapi 的 `params`，返回 `(路径, 业务 JSON, 摘要)`，仅供单元测试验证加密结果使用。
#[cfg(test)]
pub(crate) fn eapi_decrypt(params_hex: &str) -> (String, String, String) {
    use aes::cipher::BlockCipherDecrypt;

    let data: Vec<u8> = (0..params_hex.len() / 2)
        .map(|index| {
            u8::from_str_radix(&params_hex[index * 2..index * 2 + 2], 16).expect("合法 hex")
        })
        .collect();
    assert_eq!(
        data.len() % AES_BLOCK_SIZE,
        0,
        "密文长度必须是分组长度的整数倍"
    );
    let cipher = Aes128::new(&EAPI_KEY.into());
    let mut plain = Vec::with_capacity(data.len());
    let (blocks, _) = data.as_chunks::<AES_BLOCK_SIZE>();
    for chunk in blocks {
        let mut block = Block::from(*chunk);
        cipher.decrypt_block(&mut block);
        plain.extend_from_slice(&block);
    }
    let padding = usize::from(*plain.last().expect("非空密文"));
    assert!((1..=AES_BLOCK_SIZE).contains(&padding), "合法 PKCS#7 填充");
    plain.truncate(plain.len() - padding);

    let text = String::from_utf8(plain).expect("明文应是 UTF-8");
    let mut parts = text.splitn(3, EAPI_SEPARATOR);
    let path = parts.next().expect("缺少路径").to_owned();
    let json = parts.next().expect("缺少业务 JSON").to_owned();
    let digest = parts.next().expect("缺少摘要").to_owned();
    (path, json, digest)
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

    // eapi 参考向量由 NeteaseCloudMusicApi `util/crypto.js` 的 `eapi()` 算法在 Node.js（内置 crypto）里独立生成，
    // 这个格式也在 2026-11 的联网探针里被 `interface.music.163.com` 真实接受（返回了搜索结果）。
    const EAPI_SEARCH_JSON: &str =
        r#"{"s":"晴天 周杰伦","type":1,"limit":5,"offset":0,"total":true}"#;
    const EAPI_SEARCH_PARAMS: &str = "2B5D64177AA6460FBAA3DCB1285E28954BBB4F7556E09B0FB25750F12398BB5061A01CA77A1F670BD1E32D808C76A04A7BE0409B588BE8D6D0A0947DFCB861D9C7EDA2DD75B3EB8BC38A26457F24E689227F0BF6A62BF9661C11233240DFAF7430AF1FCF2A9705886F4D126B33FFF694660578D5569FDE8B836D1E4AF3529F24EC9BCE073137A5543AB8C651D884FC7F6AA3B102FBE7296AB0DB9EA5C46AD12B";

    #[test]
    fn eapi_encrypt_matches_independent_reference_implementation() {
        assert_eq!(
            eapi_encrypt("/api/cloudsearch/pc", EAPI_SEARCH_JSON),
            EAPI_SEARCH_PARAMS
        );
        assert_eq!(
            eapi_encrypt("/api/song/lyric/v1", r#"{"id":"186016"}"#),
            "04AE33D34A93FE3EC22DA8FA305D290AB337D0FE5F36D211DE0D338CC6AA89D0243EAB72A5ECAF3D823F2201F3546F7058B18724FF6FD90C9859EA5F92FB59392BD8C40D73518442E65DAD305431D0D7CFFFB2DC58EC1CE4E24C70A6929A0304"
        );
    }

    #[test]
    fn eapi_plaintext_has_path_json_and_md5_digest() {
        let (path, json, digest) =
            eapi_decrypt(&eapi_encrypt("/api/cloudsearch/pc", EAPI_SEARCH_JSON));
        assert_eq!(path, "/api/cloudsearch/pc");
        assert_eq!(json, EAPI_SEARCH_JSON);
        assert_eq!(digest, "a9a797200366d6bf67fdbfc2e7530bde");
    }

    #[test]
    fn eapi_params_are_uppercase_hex_of_whole_blocks() {
        for json in ["{}", EAPI_SEARCH_JSON, r#"{"k":"一二三四五六七八九十"}"#] {
            let params = eapi_encrypt("/api/x", json);
            assert_eq!(params.len() % 32, 0, "每个 AES 分组是 32 个 hex 字符");
            assert!(
                params
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
            );
        }
    }

    #[test]
    fn aes_128_ecb_matches_fips_197_vector_and_pads_a_full_block() {
        // FIPS-197 附录 C.1：key 000102…0f，明文 00112233…ff → 69c4e0d86a7b0430d8cdb78070b4c55a。
        let key: [u8; 16] = std::array::from_fn(|index| index as u8);
        let plain: [u8; 16] = std::array::from_fn(|index| (index as u8) * 0x11);
        let cipher = ecb_encrypt_pkcs7(&key, &plain);
        assert_eq!(hex_lower(&cipher[..16]), "69c4e0d86a7b0430d8cdb78070b4c55a");
        // 明文恰好是整数个分组时，PKCS#7 仍会补一整块（0x10 × 16）。
        assert_eq!(cipher.len(), 32);
        assert_eq!(ecb_encrypt_pkcs7(&key, &[]).len(), 16);
        assert_eq!(ecb_encrypt_pkcs7(&key, &[1; 15]).len(), 16);
        assert_eq!(ecb_encrypt_pkcs7(&key, &[1; 17]).len(), 32);
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
