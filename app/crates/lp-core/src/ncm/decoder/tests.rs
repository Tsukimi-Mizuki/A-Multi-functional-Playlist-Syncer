use std::fs;
use std::io::Cursor;

use serde_json::json;

use super::*;
use crate::ncm::client_comment;
use crate::ncm::synthetic::{
    SyntheticNcm, aes_ecb_encrypt, fake_flac_audio, fake_mp3_audio, fake_png_cover, pad_pkcs7,
};
use crate::test_util::TempDir;

fn decoder() -> FileNcmDecoder {
    FileNcmDecoder
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn decrypt(path: &Path, dir: &TempDir, overwrite: bool) -> NcmDecryptResult {
    decoder()
        .decrypt(path, dir.path(), overwrite, &CancelToken::new())
        .unwrap()
}

fn files_with_extension(dir: &TempDir, extension: &str) -> Vec<PathBuf> {
    fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == extension))
        .collect()
}

// ───────────────────────────── is_ncm_file / read_info ─────────────────────────────

#[test]
fn is_ncm_file_detects_magic_and_rejects_other_files() {
    let dir = TempDir::new();
    let ncm = write(
        &dir,
        "track.ncm",
        &SyntheticNcm::new().build(&fake_mp3_audio(40_000)),
    );
    let plain = write(&dir, "plain.mp3", &fake_mp3_audio(1024));

    assert!(decoder().is_ncm_file(&ncm));
    assert!(!decoder().is_ncm_file(&plain));
    assert!(!decoder().is_ncm_file(&dir.path().join("missing.ncm")));
    assert!(!decoder().is_ncm_file(Path::new("")));
    assert!(!decoder().is_ncm_file(Path::new("   ")));
    // 比魔数还短的文件。
    let short = write(&dir, "short.ncm", b"CTEN");
    assert!(!decoder().is_ncm_file(&short));
}

#[test]
fn read_info_parses_metadata_without_decrypting_audio() {
    let dir = TempDir::new();
    let path = write(
        &dir,
        "metadata.ncm",
        &SyntheticNcm::new().build(&fake_mp3_audio(1000)),
    );

    let info = decoder().read_info(&path).unwrap();

    assert_eq!(info.source_path, path);
    assert!(info.has_metadata());
    let metadata = info.metadata.unwrap();
    assert_eq!(metadata.title.as_deref(), Some("测试歌曲"));
    assert_eq!(metadata.album.as_deref(), Some("测试专辑"));
    assert_eq!(metadata.artists.as_deref(), Some("测试歌手/第二歌手"));
    assert_eq!(metadata.format.as_deref(), Some("mp3"));
    assert_eq!(metadata.bitrate, 320_000);
    assert_eq!(metadata.duration_ms, 215_000);
    assert_eq!(metadata.music_id, Some(1_234_567_890));
    assert_eq!(metadata.alias.as_deref(), Some("别名"));
}

#[test]
fn read_info_fails_for_non_ncm_missing_and_blank_paths() {
    let dir = TempDir::new();
    let plain = write(&dir, "not-ncm.bin", &fake_mp3_audio(512));

    assert!(matches!(
        decoder().read_info(&plain),
        Err(NcmError::InvalidData(message)) if message.contains("文件头不匹配")
    ));
    assert!(matches!(
        decoder().read_info(&dir.path().join("missing.ncm")),
        Err(NcmError::NotFound(_))
    ));
    assert!(matches!(
        decoder().read_info(Path::new(" ")),
        Err(NcmError::InvalidArgument(_))
    ));
}

#[test]
fn read_info_fails_with_invalid_data_for_a_truncated_file() {
    let dir = TempDir::new();
    let valid = SyntheticNcm::new().build(&fake_mp3_audio(2048));
    let path = write(&dir, "truncated.ncm", &valid[..20]);

    assert!(matches!(
        decoder().read_info(&path),
        Err(NcmError::InvalidData(_))
    ));
}

#[test]
fn read_info_ignores_the_audio_region() {
    // 音频区是什么都不影响只读文件头与元数据。
    let dir = TempDir::new();
    let path = write(
        &dir,
        "header-only.ncm",
        &SyntheticNcm::new().build(&[0u8; 4096]),
    );

    let info = decoder().read_info(&path).unwrap();

    assert_eq!(info.metadata.unwrap().title.as_deref(), Some("测试歌曲"));
}

// ───────────────────────────── decrypt：成功路径 ─────────────────────────────

#[test]
fn decrypt_mp3_round_trips_audio_metadata_and_cover() {
    let dir = TempDir::new();
    let audio = fake_mp3_audio(5000);
    let cover = fake_png_cover();
    let path = write(
        &dir,
        "song.ncm",
        &SyntheticNcm::new()
            .cover(&cover)
            .cover_padding(8)
            .build(&audio),
    );

    let result = decrypt(&path, &dir, false);

    assert!(result.success, "{:?}", result.error_message);
    assert_eq!(result.source_path, path);
    let output = result.output_path.clone().unwrap();
    assert_eq!(output.extension().unwrap(), "mp3");
    assert_eq!(result.format.as_deref(), Some("mp3"));
    assert_eq!(fs::read(&output).unwrap(), audio);
    assert_eq!(result.cover_data.as_deref(), Some(cover.as_slice()));
    assert_eq!(result.metadata.unwrap().title.as_deref(), Some("测试歌曲"));
}

#[test]
fn decrypt_detects_flac_by_the_first_block() {
    let dir = TempDir::new();
    let audio = fake_flac_audio(9000);
    let path = write(&dir, "lossless.ncm", &SyntheticNcm::new().build(&audio));

    let result = decrypt(&path, &dir, false);

    assert!(result.success, "{:?}", result.error_message);
    assert_eq!(result.format.as_deref(), Some("flac"));
    let output = result.output_path.unwrap();
    assert_eq!(output.extension().unwrap(), "flac");
    assert_eq!(fs::read(output).unwrap(), audio);
}

#[test]
fn decrypt_survives_chunk_boundaries() {
    let dir = TempDir::new();
    for length in [1, 32_767, 32_768, 32_769, 32_768 * 2 + 517, 32_768 * 3] {
        let audio = fake_mp3_audio(length);
        let path = write(
            &dir,
            &format!("chunk-{length}.ncm"),
            &SyntheticNcm::new().build(&audio),
        );

        let result = decrypt(&path, &dir, true);

        assert!(result.success, "{length}: {:?}", result.error_message);
        assert_eq!(
            fs::read(result.output_path.unwrap()).unwrap(),
            audio,
            "{length}"
        );
    }
}

#[test]
fn decrypt_streams_a_multi_chunk_file() {
    // 约 5 MB、160 个分块；逐字节与原音频一致，说明分块之间的密钥流没有错位。
    let dir = TempDir::new();
    let audio = fake_flac_audio(5 * 1024 * 1024 + 123);
    let path = write(
        &dir,
        "big.ncm",
        &SyntheticNcm::new().cover(&fake_png_cover()).build(&audio),
    );

    let result = decrypt(&path, &dir, false);

    assert!(result.success, "{:?}", result.error_message);
    assert_eq!(fs::read(result.output_path.unwrap()).unwrap(), audio);
}

#[test]
fn decrypt_handles_short_and_long_key_material() {
    let dir = TempDir::new();
    let audio = fake_mp3_audio(70_000);
    for length in [1, 2, 17, 255, 256, 300] {
        let path = write(
            &dir,
            &format!("key-{length}.ncm"),
            &SyntheticNcm::new()
                .key_material_length(length)
                .build(&audio),
        );

        let result = decrypt(&path, &dir, true);

        assert!(result.success, "{length}: {:?}", result.error_message);
        assert_eq!(
            fs::read(result.output_path.unwrap()).unwrap(),
            audio,
            "{length}"
        );
    }
}

#[test]
fn decrypt_without_metadata_still_produces_audio() {
    let dir = TempDir::new();
    let audio = fake_mp3_audio(3000);
    let path = write(
        &dir,
        "no-meta.ncm",
        &SyntheticNcm::new().no_metadata().build(&audio),
    );

    let result = decrypt(&path, &dir, false);

    assert!(result.success, "{:?}", result.error_message);
    assert!(result.metadata.is_none());
    assert_eq!(fs::read(result.output_path.unwrap()).unwrap(), audio);
}

#[test]
fn invalid_metadata_does_not_abort_the_audio() {
    let dir = TempDir::new();
    let audio = fake_mp3_audio(3000);
    let path = write(
        &dir,
        "bad-meta.ncm",
        &SyntheticNcm::new()
            .metadata("{not-valid-json")
            .build(&audio),
    );

    let result = decrypt(&path, &dir, false);

    assert!(result.success, "{:?}", result.error_message);
    assert!(result.metadata.is_none());
    assert_eq!(fs::read(result.output_path.unwrap()).unwrap(), audio);
}

#[test]
fn decrypt_without_a_cover_returns_none() {
    let dir = TempDir::new();
    let path = write(
        &dir,
        "nocover.ncm",
        &SyntheticNcm::new().build(&fake_mp3_audio(500)),
    );

    assert!(decrypt(&path, &dir, false).cover_data.is_none());
}

#[test]
fn decrypt_skips_cover_frame_padding() {
    let dir = TempDir::new();
    let audio = fake_mp3_audio(2000);
    // 只有填充没有封面也要对齐到音频起点。
    let path = write(
        &dir,
        "pad.ncm",
        &SyntheticNcm::new().cover_padding(33).build(&audio),
    );

    let result = decrypt(&path, &dir, false);

    assert!(result.success, "{:?}", result.error_message);
    assert_eq!(fs::read(result.output_path.unwrap()).unwrap(), audio);
}

#[test]
fn decrypt_creates_the_missing_output_directory() {
    let dir = TempDir::new();
    let path = write(
        &dir,
        "nested.ncm",
        &SyntheticNcm::new().build(&fake_mp3_audio(2048)),
    );
    let output_directory = dir.path().join("a").join("b").join("c");

    let result = decoder()
        .decrypt(&path, &output_directory, false, &CancelToken::new())
        .unwrap();

    assert!(result.success, "{:?}", result.error_message);
    let output = result.output_path.unwrap();
    assert!(output.is_file());
    assert!(output.starts_with(&output_directory));
}

#[test]
fn decrypt_names_the_output_after_the_source() {
    let dir = TempDir::new();
    let audio = fake_mp3_audio(100);
    for (source, expected) in [
        ("a.b.ncm", "a.b.mp3"),
        ("晴天 - 周杰伦.ncm", "晴天 - 周杰伦.mp3"),
        ("noext", "noext.mp3"),
        (".ncm", "decrypted.mp3"),
    ] {
        let path = write(&dir, source, &SyntheticNcm::new().build(&audio));

        let result = decrypt(&path, &dir, true);

        assert!(result.success, "{source}");
        assert_eq!(
            result
                .output_path
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy(),
            expected
        );
    }
}

// ───────────────────────────── decrypt：覆盖与清理 ─────────────────────────────

#[test]
fn decrypt_respects_the_overwrite_flag() {
    let dir = TempDir::new();
    let audio = fake_mp3_audio(4000);
    let path = write(&dir, "existing.ncm", &SyntheticNcm::new().build(&audio));

    let first = decrypt(&path, &dir, false);
    assert!(first.success);
    let output = first.output_path.unwrap();

    let sentinel = [1u8, 2, 3, 4];
    fs::write(&output, sentinel).unwrap();

    let skipped = decrypt(&path, &dir, false);
    assert!(!skipped.success);
    assert!(skipped.output_path.is_none());
    assert!(skipped.error_message.unwrap().contains("目标文件已存在"));
    assert_eq!(fs::read(&output).unwrap(), sentinel);

    let overwritten = decrypt(&path, &dir, true);
    assert!(overwritten.success, "{:?}", overwritten.error_message);
    assert_eq!(overwritten.output_path.as_ref(), Some(&output));
    assert_eq!(fs::read(&output).unwrap(), audio);
}

#[test]
fn a_pre_cancelled_token_fails_before_any_output() {
    let dir = TempDir::new();
    let path = write(
        &dir,
        "cancel.ncm",
        &SyntheticNcm::new().build(&fake_mp3_audio(100_000)),
    );
    let token = CancelToken::new();
    token.cancel();

    let result = decoder().decrypt(&path, dir.path(), true, &token);

    assert!(matches!(result, Err(NcmError::Cancelled)));
    assert!(files_with_extension(&dir, "mp3").is_empty());
    assert!(files_with_extension(&dir, "ncmtmp").is_empty());
}

/// 每读一次就数一下，读到指定次数后触发取消。
struct CancelAfterReads<R> {
    inner: R,
    reads_left: usize,
    token: CancelToken,
}

impl<R: Read> Read for CancelAfterReads<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.reads_left == 0 {
            self.token.cancel();
        } else {
            self.reads_left -= 1;
        }
        self.inner.read(buf)
    }
}

#[test]
fn cancelling_mid_stream_stops_between_chunks_and_removes_the_temp_file() {
    let dir = TempDir::new();
    let temp = dir.path().join(".x.ncmtmp");
    let output = dir.path().join("x.mp3");
    let token = CancelToken::new();
    let mut source = CancelAfterReads {
        inner: Cursor::new(vec![0u8; AUDIO_CHUNK_SIZE * 10]),
        reads_left: 3,
        token: token.clone(),
    };

    let result = write_and_commit(&temp, &output, &[1, 2, 3], &mut source, &[0; 256], &token);

    assert!(matches!(result, Err(NcmError::Cancelled)));
    assert!(!temp.exists(), "取消后不应留下临时文件");
    assert!(!output.exists(), "取消后不应出现半截成品");
}

#[test]
fn a_failed_commit_removes_the_temp_file_and_keeps_the_existing_output() {
    let dir = TempDir::new();
    let temp = dir.path().join(".y.ncmtmp");
    // 目标是个目录：改名会失败。
    let output = dir.path().join("y.mp3");
    fs::create_dir(&output).unwrap();

    let result = write_and_commit(
        &temp,
        &output,
        &[1, 2, 3],
        &mut Cursor::new(vec![9u8; 100]),
        &[0; 256],
        &CancelToken::new(),
    );

    assert!(matches!(result, Err(NcmError::Io(_))));
    assert!(!temp.exists());
    assert!(output.is_dir());
}

#[test]
fn no_temporary_files_remain_after_success_or_failure() {
    let dir = TempDir::new();
    let path = write(
        &dir,
        "clean.ncm",
        &SyntheticNcm::new().build(&fake_mp3_audio(6000)),
    );
    assert!(decrypt(&path, &dir, false).success);
    assert!(files_with_extension(&dir, "ncmtmp").is_empty());
    assert_eq!(files_with_extension(&dir, "mp3").len(), 1);

    let broken = write(&dir, "broken.ncm", b"CTENFDAM\0");
    assert!(!decrypt(&broken, &dir, false).success);
    assert!(files_with_extension(&dir, "ncmtmp").is_empty());
}

// ───────────────────────────── decrypt：坏文件 ─────────────────────────────

#[test]
fn decrypt_reports_a_non_ncm_file_as_a_failure_result() {
    let dir = TempDir::new();
    let path = write(&dir, "fake.ncm", &fake_mp3_audio(2048));

    let result = decrypt(&path, &dir, false);

    assert!(!result.success);
    assert!(result.output_path.is_none());
    assert!(result.error_message.unwrap().contains("文件头不匹配"));
    assert!(files_with_extension(&dir, "mp3").is_empty());
}

#[test]
fn decrypt_reports_truncated_files_as_failure_results() {
    let dir = TempDir::new();
    let valid = SyntheticNcm::new().build(&fake_mp3_audio(2048));
    // 魔数之后、密钥之内、元数据之内、封面之前被截断。
    for cut in [8, 9, 20, 40, valid.len() - 2048 - 20] {
        let path = write(&dir, &format!("cut-{cut}.ncm"), &valid[..cut]);

        let result = decrypt(&path, &dir, false);

        assert!(!result.success, "{cut}");
        let message = result.error_message.unwrap();
        assert!(message.contains("NCM 文件不完整"), "{cut}: {message}");
    }
}

#[test]
fn decrypt_reports_a_file_without_audio() {
    let dir = TempDir::new();
    let valid = SyntheticNcm::new().build(&fake_mp3_audio(10));
    let path = write(&dir, "empty-audio.ncm", &valid[..valid.len() - 10]);

    let result = decrypt(&path, &dir, false);

    assert!(!result.success);
    assert_eq!(
        result.error_message.as_deref(),
        Some("NCM 文件中没有音频数据。")
    );
}

#[test]
fn decrypt_rejects_invalid_length_fields() {
    let dir = TempDir::new();
    let valid = SyntheticNcm::new().build(&fake_mp3_audio(100));
    let key_length_at = 10;
    let key_length =
        i32::from_le_bytes(valid[key_length_at..key_length_at + 4].try_into().unwrap());
    let metadata_length_at = key_length_at + 4 + key_length as usize;

    let patch = |at: usize, value: i32| {
        let mut bytes = valid.clone();
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        bytes
    };

    for (name, bytes, expected) in [
        ("key0", patch(key_length_at, 0), "NCM 密钥长度无效（0）"),
        ("keyneg", patch(key_length_at, -5), "NCM 密钥长度无效（-5）"),
        (
            "keyhuge",
            patch(key_length_at, 5 * 1024 * 1024),
            "NCM 密钥长度无效（5242880）",
        ),
        (
            "metahuge",
            patch(metadata_length_at, 17 * 1024 * 1024),
            "NCM 元数据长度无效（17825792）",
        ),
    ] {
        let path = write(&dir, &format!("{name}.ncm"), &bytes);
        let result = decrypt(&path, &dir, false);
        assert!(!result.success, "{name}");
        assert!(
            result.error_message.as_deref().unwrap().contains(expected),
            "{name}: {:?}",
            result.error_message
        );
    }

    // 封面长度：位置在元数据之后的 5 字节与 4 字节帧长度之后。
    let metadata_length = i32::from_le_bytes(
        valid[metadata_length_at..metadata_length_at + 4]
            .try_into()
            .unwrap(),
    );
    let image_length_at = metadata_length_at + 4 + metadata_length as usize + 5 + 4;
    for (name, value, expected) in [
        ("imgneg", -1, "NCM 封面长度无效（-1）。"),
        (
            "imghuge",
            65 * 1024 * 1024,
            "NCM 封面长度无效（68157440）。",
        ),
    ] {
        let path = write(&dir, &format!("{name}.ncm"), &patch(image_length_at, value));
        let result = decrypt(&path, &dir, false);
        assert_eq!(result.error_message.as_deref(), Some(expected), "{name}");
    }
}

#[test]
fn decrypt_rejects_key_data_that_is_too_short() {
    // 只有 17 字节前缀、没有密钥材料。
    let dir = TempDir::new();
    let key_plain: Vec<u8> = (0..17u8).collect();
    let mut encrypted_key = aes_ecb_encrypt(&pad_pkcs7(&key_plain), &CORE_KEY);
    for byte in &mut encrypted_key {
        *byte ^= 0x64;
    }
    let mut bytes = b"CTENFDAM\x01\x00".to_vec();
    bytes.extend_from_slice(&(encrypted_key.len() as i32).to_le_bytes());
    bytes.extend_from_slice(&encrypted_key);
    bytes.extend_from_slice(&[0; 64]);
    let path = write(&dir, "shortkey.ncm", &bytes);

    let result = decrypt(&path, &dir, false);

    assert_eq!(
        result.error_message.as_deref(),
        Some("NCM 密钥数据长度无效。")
    );
}

#[test]
fn decrypt_arguments_are_validated() {
    let dir = TempDir::new();
    let path = write(
        &dir,
        "a.ncm",
        &SyntheticNcm::new().build(&fake_mp3_audio(10)),
    );

    assert!(matches!(
        decoder().decrypt(Path::new(" "), dir.path(), false, &CancelToken::new()),
        Err(NcmError::InvalidArgument(_))
    ));
    assert!(matches!(
        decoder().decrypt(&path, Path::new(""), false, &CancelToken::new()),
        Err(NcmError::InvalidArgument(_))
    ));
    assert!(matches!(
        decoder().decrypt(
            &dir.path().join("none.ncm"),
            dir.path(),
            false,
            &CancelToken::new()
        ),
        Err(NcmError::NotFound(_))
    ));
}

// ───────────────────────────── 格式判定 / 文件名 ─────────────────────────────

#[test]
fn detect_format_recognizes_id3_flac_sync_word_and_defaults_to_flac() {
    assert_eq!(detect_format(b"ID3\x04"), "mp3");
    assert_eq!(detect_format(b"fLaC\0\0"), "flac");
    assert_eq!(detect_format(&[0xFF, 0xFB, 0x90]), "mp3");
    assert_eq!(detect_format(&[0xFF, 0xE2]), "mp3");
    assert_eq!(detect_format(&[0xFF, 0x1B]), "flac");
    assert_eq!(detect_format(b"OggS"), "flac");
    assert_eq!(detect_format(&[0xFF]), "flac");
    assert_eq!(detect_format(b"ID"), "flac");
}

#[test]
fn safe_stem_replaces_invalid_file_name_characters() {
    assert_eq!(safe_stem(Path::new("a.ncm")), "a");
    assert_eq!(safe_stem(Path::new(".ncm")), "ncm");
    assert_eq!(safe_stem(Path::new("a b.c.ncm")), "a b.c");
}

// ───────────────────────────── 元数据解析 ─────────────────────────────

fn parse(json: serde_json::Value) -> Option<NcmMetadata> {
    parse_metadata_json(&json.to_string())
}

#[test]
fn metadata_reads_every_artist_shape_and_trims() {
    let metadata = parse(json!({
        "artist": [["  歌手一 ", 1], "歌手二", {"name": "歌手三"}, [], [5], 7, "  ", {"x": 1}],
    }))
    .unwrap();

    assert_eq!(metadata.artists.as_deref(), Some("歌手一/歌手二/歌手三"));
    assert_eq!(parse(json!({"artist": []})).unwrap().artists, None);
    assert_eq!(parse(json!({"artist": "不是数组"})).unwrap().artists, None);
}

#[test]
fn metadata_reads_strings_numbers_and_blanks() {
    let metadata = parse(json!({
        "musicName": "  晴天 ",
        "album": 123,
        "format": "   ",
        "bitrate": "320000",
        "duration": "215000",
        "musicId": "186016",
    }))
    .unwrap();

    assert_eq!(metadata.title.as_deref(), Some("晴天"));
    assert_eq!(metadata.album.as_deref(), Some("123"));
    assert_eq!(metadata.format, None);
    assert_eq!(metadata.bitrate, 320_000);
    assert_eq!(metadata.duration_ms, 215_000);
    assert_eq!(metadata.music_id, Some(186_016));
}

#[test]
fn metadata_numbers_follow_the_csharp_conversion_rules() {
    // 带小数的 bitrate 当作 0（TryGetInt32 / TryGetInt64 都失败），duration 按双精度截断；超出 int 的整数夹取。
    let metadata = parse(json!({"bitrate": 320000.5, "duration": 215000.9, "musicId": 0})).unwrap();
    assert_eq!(metadata.bitrate, 0);
    assert_eq!(metadata.duration_ms, 215_000);
    assert_eq!(metadata.music_id, None);

    let huge = parse(json!({"bitrate": 99_999_999_999i64, "musicId": -5})).unwrap();
    assert_eq!(huge.bitrate, i32::MAX);
    assert_eq!(huge.music_id, None);

    let junk = parse(json!({"bitrate": "abc", "duration": [], "musicId": null})).unwrap();
    assert_eq!(
        (junk.bitrate, junk.duration_ms, junk.music_id),
        (0, 0, None)
    );
}

#[test]
fn metadata_alias_accepts_a_string_or_an_array() {
    assert_eq!(
        parse(json!({"alias": " 副标题 "}))
            .unwrap()
            .alias
            .as_deref(),
        Some("副标题")
    );
    assert_eq!(
        parse(json!({"alias": [" 甲 ", "", 3, "乙"]}))
            .unwrap()
            .alias
            .as_deref(),
        Some("甲/乙")
    );
    assert_eq!(parse(json!({"alias": []})).unwrap().alias, None);
    assert_eq!(parse(json!({"alias": 5})).unwrap().alias, None);
}

#[test]
fn metadata_json_must_be_an_object() {
    assert!(parse_metadata_json("[1,2]").is_none());
    assert!(parse_metadata_json("\"x\"").is_none());
    assert!(parse_metadata_json("{broken").is_none());
    assert_eq!(parse_metadata_json("{}").unwrap(), NcmMetadata::default());
}

fn encrypted_metadata(prefix: &str, plain: &str) -> Vec<u8> {
    use base64::Engine;
    let cipher = aes_ecb_encrypt(&pad_pkcs7(plain.as_bytes()), &MODIFY_KEY);
    format!(
        "{prefix}{}",
        base64::engine::general_purpose::STANDARD.encode(cipher)
    )
    .into_bytes()
}

#[test]
fn decrypt_metadata_requires_the_prefix_and_the_music_marker() {
    let good = encrypted_metadata("163 key(Don't modify):", "music:{\"musicName\":\"x\"}");
    assert_eq!(
        decrypt_metadata(&good).as_deref(),
        Some("{\"musicName\":\"x\"}")
    );

    let no_prefix = encrypted_metadata("", "music:{}");
    assert_eq!(decrypt_metadata(&no_prefix), None);
    let no_marker = encrypted_metadata("163 key(Don't modify):", "{\"a\":1}");
    assert_eq!(decrypt_metadata(&no_marker), None);
    assert_eq!(decrypt_metadata(b"163 key(Don't modify):"), None);
    assert_eq!(decrypt_metadata(b"163 key(Don't modify):!!!"), None);
    assert_eq!(
        decrypt_metadata("163 key(Don't modify):汉字".as_bytes()),
        None
    );
}

#[test]
fn decrypt_metadata_trims_trailing_padding_characters_and_ignores_whitespace() {
    let mut payload = encrypted_metadata("163 key(Don't modify):", "music:{\"a\":1}\u{0}\r\n  ");
    assert_eq!(decrypt_metadata(&payload).as_deref(), Some("{\"a\":1}"));

    // .NET 的 Base64 解码忽略空白字符。
    payload.insert(30, b'\n');
    payload.insert(40, b' ');
    assert_eq!(decrypt_metadata(&payload).as_deref(), Some("{\"a\":1}"));
}

#[test]
fn metadata_is_shared_with_the_client_comment_format() {
    // 客户端注释与 NCM 元数据用同一把密钥、同一种格式。
    let comment = client_comment::build_comment(r#"{"musicName":"共用","musicId":9}"#);

    let metadata = try_parse_metadata(comment.as_bytes()).unwrap();

    assert_eq!(metadata.title.as_deref(), Some("共用"));
    assert_eq!(metadata.music_id, Some(9));
}

// ───────────────────────────── 低层函数 ─────────────────────────────

#[test]
fn pkcs7_padding_is_stripped_leniently_like_the_reference_implementation() {
    let mut data = vec![1, 2, 3, 4, 4, 4, 4];
    strip_pkcs7_padding(&mut data);
    assert_eq!(data, [1, 2, 3]);

    // 最后一个字节在 1..=16 就去掉，不校验其余填充字节。
    let mut loose = vec![9, 9, 9, 2];
    strip_pkcs7_padding(&mut loose);
    assert_eq!(loose, [9, 9]);

    for untouched in [vec![1, 2, 3, 0], vec![1, 2, 3, 17], vec![5, 6]] {
        let mut data = untouched.clone();
        strip_pkcs7_padding(&mut data);
        assert_eq!(data, untouched);
    }
    let mut empty = Vec::new();
    strip_pkcs7_padding(&mut empty);
    assert!(empty.is_empty());
}

#[test]
fn aes_ecb_decrypt_only_uses_whole_blocks() {
    let key = [7u8; 16];
    assert!(aes_ecb_decrypt(&[], &key).is_empty());
    assert!(aes_ecb_decrypt(&[1; 15], &key).is_empty());

    let cipher = aes_ecb_encrypt(&pad_pkcs7(b"hello"), &key);
    let mut with_tail = cipher.clone();
    with_tail.extend_from_slice(&[0xAA; 5]);
    assert_eq!(aes_ecb_decrypt(&with_tail, &key), b"hello");
}

#[test]
fn key_box_is_a_permutation_and_decrypt_chunk_is_its_own_inverse() {
    let key_data: Vec<u8> = (0..17u8).chain((0..40).map(|i| i * 3 + 1)).collect();
    let key_box = build_key_box(&key_data).unwrap();

    let mut sorted = key_box.to_vec();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..=255u8).collect::<Vec<_>>());

    let original: Vec<u8> = (0..1000).map(|i| (i % 251) as u8).collect();
    let mut data = original.clone();
    decrypt_chunk(&mut data, &key_box);
    assert_ne!(data, original);
    decrypt_chunk(&mut data, &key_box);
    assert_eq!(data, original);
}

#[test]
fn errors_have_chinese_messages() {
    assert_eq!(NcmError::Cancelled.to_string(), "操作已取消");
    assert!(
        NcmError::NotFound(PathBuf::from("x.ncm"))
            .to_string()
            .contains("找不到")
    );
    let denied = NcmError::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    assert!(denied.to_string().contains("权限不足"));
}
