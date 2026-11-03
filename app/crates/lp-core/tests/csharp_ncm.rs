//! NCM 解密与导入与 C# 版（`NcmDecoder` / `NcmImportService` / `AudioTagService`）的交叉兼容测试。
//!
//! `tests/fixtures/ncm/cases/*.ncm` 是临时 C# 探针（已删除、不入库）用 C# 的 `SyntheticNcmFile` 合成的样本
//! （合法文件、各种元数据形态、多分块、各种损坏与截断）；`expected.json` 记录 C# 对同一批文件的：
//! `ReadInfoAsync`、`DecryptAsync`（成功与否、错误文案、输出格式与文件名、输出内容的 SHA-256、封面的 SHA-256、元数据）、
//! 以及 `NcmImportService.ImportAsync` 写标签之后 `AudioTagService.ReadAsync` 读回的字段。
//! 这里用 Rust 处理同一批文件，要求逐项一致。`sanitizer.json` 是 `FileNameSanitizer` 的输入输出向量。
//!
//! 夹具里没有任何真实歌曲：音频是静音帧 / 伪随机字节，元数据是手写的假数据，封面是 1×1 PNG 或伪随机字节。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lp_core::download::ncm_import::{NcmImportOptions, NcmImportService};
use lp_core::ncm::{FileNcmDecoder, NcmDecoder, NcmError, NcmMetadata};
use lp_core::tags::{AudioTagService, LoftyTagService};
use lp_core::utilities::CancelToken;
use lp_core::utilities::file_name_sanitizer::{self, NameFields};
use serde_json::Value;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("ncm")
}

fn read_json(name: &str) -> Value {
    let bytes = fs::read(fixture_dir().join(name)).expect("读取期望值");
    serde_json::from_slice(&bytes).expect("期望值是合法 JSON")
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("lp-csharp-ncm-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ───────────────────────────── 最小的 SHA-256（只给测试用，避免为此加依赖） ─────────────────────────────

fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());

    for block in message.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (slot, word) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *slot = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(value);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

#[test]
fn the_test_sha256_matches_known_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let long = vec![0x61u8; 1000];
    assert_eq!(
        sha256_hex(&long),
        "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
    );
}

// ───────────────────────────── 比较 ─────────────────────────────

fn text(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(str::to_owned)
}

fn assert_metadata(actual: Option<&NcmMetadata>, expected: &Value, context: &str) {
    if expected.is_null() {
        assert!(
            actual.is_none(),
            "{context}: C# 没有元数据，Rust 却读到了 {actual:?}"
        );
        return;
    }
    let actual = actual.unwrap_or_else(|| panic!("{context}: C# 有元数据，Rust 没读到"));
    assert_eq!(actual.title, text(expected, "title"), "{context}: title");
    assert_eq!(
        actual.artists,
        text(expected, "artists"),
        "{context}: artists"
    );
    assert_eq!(actual.album, text(expected, "album"), "{context}: album");
    assert_eq!(actual.format, text(expected, "format"), "{context}: format");
    assert_eq!(
        i64::from(actual.bitrate),
        expected["bitrate"].as_i64().unwrap(),
        "{context}: bitrate"
    );
    assert_eq!(
        actual.duration_ms,
        expected["durationMs"].as_i64().unwrap(),
        "{context}: durationMs"
    );
    assert_eq!(
        actual.music_id,
        expected["musicId"].as_i64(),
        "{context}: musicId"
    );
    assert_eq!(actual.alias, text(expected, "alias"), "{context}: alias");
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn rust_reads_and_decrypts_every_csharp_sample_like_csharp_does() {
    let expected = read_json("expected.json");
    let cases = expected.as_array().unwrap();
    assert_eq!(cases.len(), 32, "夹具的样本数");
    let scratch = Scratch::new("decrypt");
    let decoder = FileNcmDecoder;

    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = fixture_dir()
            .join("cases")
            .join(case["fileName"].as_str().unwrap());

        // ReadInfo
        let info = decoder.read_info(&source);
        if case["readInfo"]["ok"].as_bool().unwrap() {
            let info = info.unwrap_or_else(|e| panic!("{name}: read_info 应成功：{e}"));
            assert_metadata(info.metadata.as_ref(), &case["readInfo"]["metadata"], name);
        } else {
            let message = case["readInfo"]["message"].as_str().unwrap();
            assert_eq!(case["readInfo"]["kind"], "InvalidDataException", "{name}");
            match info {
                Err(NcmError::InvalidData(actual)) => {
                    assert_eq!(actual, message, "{name}: read_info 的错误文案");
                }
                other => panic!("{name}: read_info 应是 InvalidData：{other:?}"),
            }
        }

        // Decrypt
        let decrypt = &case["decrypt"];
        assert!(
            decrypt["threw"].is_null(),
            "{name}: 夹具里 C# 没有抛异常的样本"
        );
        let out_dir = scratch.0.join(name);
        let result = decoder
            .decrypt(&source, &out_dir, false, &CancelToken::new())
            .unwrap_or_else(|e| panic!("{name}: decrypt 不应抛错：{e}"));

        assert_eq!(
            result.success,
            decrypt["success"].as_bool().unwrap(),
            "{name}: success"
        );
        assert_eq!(
            result.error_message,
            text(decrypt, "errorMessage"),
            "{name}: errorMessage"
        );
        assert_eq!(result.format, text(decrypt, "format"), "{name}: format");
        assert_eq!(
            result.output_path.as_deref().map(file_name),
            text(decrypt, "outputName"),
            "{name}: 输出文件名"
        );
        if let Some(output) = &result.output_path {
            let bytes = fs::read(output).unwrap();
            assert_eq!(
                bytes.len() as u64,
                decrypt["outputLength"].as_u64().unwrap(),
                "{name}: 输出长度"
            );
            assert_eq!(
                sha256_hex(&bytes),
                decrypt["outputSha256"].as_str().unwrap(),
                "{name}: 输出内容"
            );
        }
        assert_eq!(
            result.cover_data.as_deref().map(sha256_hex),
            text(decrypt, "coverSha256"),
            "{name}: 封面"
        );
        assert_metadata(result.metadata.as_ref(), &decrypt["metadata"], name);
    }

    // 解密不留临时文件。
    for entry in fs::read_dir(&scratch.0).unwrap() {
        for file in fs::read_dir(entry.unwrap().path()).unwrap() {
            let name = file_name(&file.unwrap().path());
            assert!(!name.ends_with(".ncmtmp"), "遗留临时文件 {name}");
        }
    }
}

#[tokio::test]
async fn rust_import_writes_the_same_tags_as_csharp() {
    let expected = read_json("expected.json");
    let scratch = Scratch::new("import");
    let service = NcmImportService::new(Arc::new(FileNcmDecoder), Arc::new(LoftyTagService), None);
    let mut compared = 0;

    for case in expected.as_array().unwrap() {
        if !case["tag"].as_bool().unwrap() {
            continue;
        }
        let name = case["name"].as_str().unwrap();
        let source = fixture_dir()
            .join("cases")
            .join(case["fileName"].as_str().unwrap());
        let out_dir = scratch.0.join(name);
        let outcome = service
            .import(
                &[source],
                &out_dir,
                NcmImportOptions::default(),
                &|_| {},
                &CancelToken::new(),
            )
            .await
            .unwrap();
        let result = &outcome.results[0];
        let import = &case["import"];

        assert_eq!(
            result.success,
            import["success"].as_bool().unwrap(),
            "{name}: {:?}",
            result.error_message
        );
        assert_eq!(result.error_message, text(import, "errorMessage"), "{name}");
        assert_eq!(
            result.output_path.as_deref().map(file_name),
            text(import, "outputName"),
            "{name}: 输出文件名"
        );

        // 反方向（Rust 写 → C# 读）无法在 Rust 的测试里跑：设了环境变量就把输出导出，供 C# 探针读回比对。
        if let Some(export) = std::env::var_os("LP_EXPORT_NCM_DIR") {
            let export = PathBuf::from(export);
            fs::create_dir_all(&export).unwrap();
            let output = result.output_path.as_ref().unwrap();
            fs::copy(output, export.join(file_name(output))).unwrap();
        }

        let tags = &import["tags"];
        let info = LoftyTagService
            .read(result.output_path.as_ref().unwrap())
            .unwrap()
            .unwrap_or_else(|| panic!("{name}: Rust 写出的文件读不了"));
        assert_eq!(info.title, text(tags, "title"), "{name}: title");
        assert_eq!(info.artist, text(tags, "artist"), "{name}: artist");
        assert_eq!(info.album, text(tags, "album"), "{name}: album");
        assert_eq!(
            info.album_artist,
            text(tags, "albumArtist"),
            "{name}: albumArtist"
        );
        assert_eq!(
            info.netease_id,
            tags["neteaseId"].as_i64(),
            "{name}: neteaseId"
        );
        assert_eq!(
            info.has_cover,
            tags["hasCover"].as_bool().unwrap(),
            "{name}: hasCover"
        );
        assert_eq!(
            info.cover_data.as_deref().map(sha256_hex),
            text(tags, "coverSha256"),
            "{name}: 封面内容"
        );
        assert_eq!(info.comment, text(tags, "comment"), "{name}: comment");
        assert_eq!(
            info.file_format,
            text(tags, "fileFormat"),
            "{name}: fileFormat"
        );
        assert_eq!(
            info.year.map(i64::from),
            tags["year"].as_i64(),
            "{name}: year"
        );
        assert_eq!(info.lyrics, text(tags, "lyrics"), "{name}: lyrics");
        compared += 1;
    }
    assert_eq!(compared, 11);
}

#[test]
fn file_name_sanitizer_matches_csharp() {
    let vectors = read_json("sanitizer.json");

    for case in vectors["sanitize"].as_array().unwrap() {
        let actual = file_name_sanitizer::sanitize(
            case["input"].as_str(),
            case["max"].as_u64().unwrap() as usize,
        );
        let expected = case["output"].as_str().unwrap();
        // 唯一允许的差异：截断落在代理对中间时 C# 会留下孤立代理项（无法用 Rust 字符串表示）。
        assert_eq!(
            actual, expected,
            "sanitize({:?}, {})",
            case["input"], case["max"]
        );
    }

    for case in vectors["build"].as_array().unwrap() {
        let actual = file_name_sanitizer::build_file_name(
            case["template"].as_str().unwrap(),
            NameFields {
                title: case["title"].as_str(),
                artist: case["artist"].as_str(),
                album: case["album"].as_str(),
                track_number: case["track"].as_u64().unwrap() as u32,
                year: case["year"].as_u64().unwrap() as u32,
                song_id: case["id"].as_i64().unwrap(),
            },
            case["ext"].as_str().unwrap(),
        );
        assert_eq!(
            actual,
            case["output"].as_str().unwrap(),
            "build_file_name({:?})",
            case["template"]
        );
    }
}
