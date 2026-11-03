//! 歌词解析与 C# 版的逐字节对照测试。
//!
//! `tests/fixtures/lyrics/` 里的 JSON 由一个临时 C# 探针生成（已删除，不入库）：探针直接编译 C# 版
//! `LyricParser` / `QqTripleDes` / `QrcDecryptor` / `KrcLyricsParser` 的源码，对同一批输入记录
//! 解析结果（每行 / 每词的时间与文本、译文）以及 `ToPlainLrc` / `ToEnhancedLrc`（含 / 不含译文）的完整输出。
//! 这里用 Rust 实现处理同样的输入，要求结果**完全相等**；解密用例里的密文由 C# 的 .NET `ZLibStream` +
//! 3DES 变体生成，等于是跨实现验证 Rust 的解密。
//!
//! 夹具里没有任何真实歌词：输入都是手写的假文本，或网易云歌词接口的公开样例结构。

use std::path::PathBuf;

use lp_core::lyrics::{LyricDocument, RawLyrics, krc, parser, qrc};
use serde_json::{Value, json};

fn load(name: &str) -> Vec<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("lyrics")
        .join(name);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|e| panic!("读取夹具 {} 失败：{e}", path.display()));
    match serde_json::from_slice(&bytes).expect("夹具是合法 JSON") {
        Value::Array(items) => items,
        other => panic!("夹具应是数组：{other}"),
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(str::to_owned)
}

/// 与 C# 探针里的 `DocJson` 同一形状。
fn document_json(document: &LyricDocument) -> Value {
    let lines: Vec<Value> = document
        .lines
        .iter()
        .map(|line| {
            let words: Vec<Value> = line
                .words
                .iter()
                .map(|word| {
                    json!({ "startMs": word.start_ms, "durationMs": word.duration_ms, "text": word.text })
                })
                .collect();
            json!({
                "startMs": line.start_ms,
                "durationMs": line.duration_ms,
                "text": line.text,
                "words": words,
                "translation": line.translation,
            })
        })
        .collect();
    json!({
        "lines": lines,
        "translation": document.translation,
        "romanization": document.romanization,
    })
}

/// 找出两个 JSON 值第一处不同的位置，便于定位。
fn first_difference(path: &str, left: &Value, right: &Value) -> Option<String> {
    if left == right {
        return None;
    }
    match (left, right) {
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Some(format!("{path}: 长度 {} != {}", a.len(), b.len()));
            }
            a.iter()
                .zip(b)
                .enumerate()
                .find_map(|(i, (x, y))| first_difference(&format!("{path}[{i}]"), x, y))
        }
        (Value::Object(a), Value::Object(b)) => a
            .keys()
            .chain(b.keys())
            .find_map(|key| first_difference(&format!("{path}.{key}"), &a[key], &b[key])),
        _ => Some(format!("{path}: Rust={left} C#={right}")),
    }
}

fn assert_no_failures(kind: &str, total: usize, failures: Vec<String>) {
    assert!(total > 0, "{kind} 夹具为空");
    assert!(
        failures.is_empty(),
        "{kind}：{} / {total} 个用例与 C# 不一致：\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn parse_and_serialize_match_csharp_byte_for_byte() {
    let cases = load("parse-cases.json");
    let mut failures = Vec::new();

    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let raw = RawLyrics {
            lrc: text(&case["raw"], "lrc"),
            translated_lrc: text(&case["raw"], "translatedLrc"),
            romanized_lrc: text(&case["raw"], "romanizedLrc"),
            yrc: text(&case["raw"], "yrc"),
            qrc: text(&case["raw"], "qrc"),
        };

        let document = parser::parse(&raw);
        if let Some(diff) =
            first_difference("document", &document_json(&document), &case["document"])
        {
            failures.push(format!("[{name}] {diff}"));
            continue;
        }

        for (field, actual) in [
            ("plain", parser::to_plain_lrc(&document, true)),
            ("plainNoTranslation", parser::to_plain_lrc(&document, false)),
            ("enhanced", parser::to_enhanced_lrc(&document, true)),
            (
                "enhancedNoTranslation",
                parser::to_enhanced_lrc(&document, false),
            ),
        ] {
            let expected = case[field].as_str().unwrap();
            if actual != expected {
                failures.push(format!(
                    "[{name}] {field}:\n  Rust={actual:?}\n  C#  ={expected:?}"
                ));
            }
        }
    }

    assert_no_failures("LRC/YRC/QRC 解析与序列化", cases.len(), failures);
}

#[test]
fn krc_parse_matches_csharp() {
    let cases = load("krc-parse-cases.json");
    let mut failures = Vec::new();

    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let document = krc::parse(case["text"].as_str());
        if let Some(diff) =
            first_difference("document", &document_json(&document), &case["document"])
        {
            failures.push(format!("[{name}] {diff}"));
        }
    }

    assert_no_failures("KRC 解析", cases.len(), failures);
}

#[test]
fn krc_and_qrc_decryption_match_csharp() {
    let cases = load("decrypt-cases.json");
    let mut failures = Vec::new();

    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let input = case["input"].as_str();
        let actual = match case["kind"].as_str().unwrap() {
            "krc" => krc::decrypt_base64(input),
            "qrc" => qrc::try_decrypt(input),
            other => panic!("未知用例类型 {other}"),
        };
        let expected = case["expected"].as_str().map(str::to_owned);
        if actual != expected {
            failures.push(format!("[{name}]\n  Rust={actual:?}\n  C#  ={expected:?}"));
        }
    }

    assert_no_failures("KRC / QRC 解密", cases.len(), failures);
}

fn hex_to_bytes(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn triple_des_variant_matches_csharp_vectors() {
    let vectors = load("tripledes-vectors.json");
    assert!(vectors.len() >= 5);

    for vector in &vectors {
        let key = hex_to_bytes(vector["key"].as_str().unwrap());
        let plain = hex_to_bytes(vector["plain"].as_str().unwrap());
        let expected = hex_to_bytes(vector["encrypted"].as_str().unwrap());

        if vector["single"].as_bool() == Some(true) {
            let key: [u8; 8] = key.try_into().unwrap();
            let block: [u8; 8] = plain.try_into().unwrap();
            assert_eq!(qrc::encrypt_block(&key, &block).to_vec(), expected);
            continue;
        }

        let key: [u8; 24] = key.try_into().unwrap();
        assert_eq!(qrc::encrypt(&key, &plain, plain.len()), expected);
        // 把“明文”当密文解密，要求与 C# 的 Decrypt 输出相同（覆盖解密方向的密钥顺序）。
        let decrypted = hex_to_bytes(vector["decryptedAsIfCipher"].as_str().unwrap());
        assert_eq!(qrc::decrypt(&key, &plain, plain.len()), decrypted);
    }
}
