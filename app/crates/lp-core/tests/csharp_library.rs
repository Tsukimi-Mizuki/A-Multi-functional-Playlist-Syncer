//! 本地曲库扫描与匹配和 C# 版（`LocalLibraryService` / `SongMatcher`，.NET 8）的交叉验证。
//!
//! `tests/fixtures/library/` 里是临时 C# 探针（已删除、不入库）直接调用现有 C# 实现得到的期望值：
//!
//! - `text-*.json`、`filename-cases.json`、`bmp-classes.json`：C# 内部静态方法（归一化、相似度、歌手拆分、时长分、
//!   文件名推断）与 .NET 字符类别 / 小写映射的输出；
//! - `matcher-cases.json`：五组随机生成的歌单 + 本地曲库（含重复歌曲、同名系列、错误 ID、NCM、缺歌手 / 时长……）
//!   和 C# `SongMatcher.Match` 的逐首结果；
//! - `music/`：C# 用 `Mp3Fixture` / `SyntheticNcmFile` / `AudioTagService`（TagLib）造的合成曲库（静音帧 / 伪随机字节，
//!   没有任何真实歌曲），`scan-expected.json` 是 C# `LocalLibraryService.ScanAsync` 对它的扫描结果，
//!   `songs.json` + `match-expected.json` 是一份假歌单与 C# 的匹配结果。
//!
//! 浮点分数要求与 C# 相差不超过 1e-9（实际逐位相同）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lp_core::library::text::{
    MatchText, is_dotnet_control, is_dotnet_punctuation, is_dotnet_symbol, is_dotnet_whitespace,
    normalize_for_match, similarity, split_artist_tokens, to_lower_invariant,
};
use lp_core::library::{
    LocalAvailability, LocalLibraryService, LocalTrack, MatchMethod, SongMatcher,
    duration_similarity, parse_file_name,
};
use lp_core::ncm::{FileNcmDecoder, NcmMetadata};
use lp_core::netease::{AlbumRef, ArtistRef, NeteaseSong};
use lp_core::tags::LoftyTagService;
use lp_core::utilities::CancelToken;
use serde_json::Value;

const EPSILON: f64 = 1e-9;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("library")
}

fn load(name: &str) -> Vec<Value> {
    let bytes = std::fs::read(fixture_dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    match serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("{name}: {e}")) {
        Value::Array(items) => items,
        other => panic!("{name} 应是数组：{other}"),
    }
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn text_of(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

#[test]
fn normalization_matches_dotnet() {
    let cases = load("text-normalize.json");
    assert!(cases.len() > 250);
    let mut mismatches = Vec::new();
    for case in &cases {
        let input = case["input"].as_str().unwrap();
        let expected = units(case["normalized"].as_str().unwrap());
        let actual = normalize_for_match(Some(input));
        if actual != expected {
            mismatches.push(format!(
                "{input:?}: C# {:?} / Rust {:?}",
                String::from_utf16_lossy(&expected),
                String::from_utf16_lossy(&actual)
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn similarity_matches_dotnet() {
    let cases = load("text-similarity.json");
    assert_eq!(cases.len(), 700);
    for case in &cases {
        let left = case["left"].as_str().unwrap();
        let right = case["right"].as_str().unwrap();
        let expected = case["score"].as_f64().unwrap();
        let actual = similarity(
            &MatchText::from_units(units(left)),
            &MatchText::from_units(units(right)),
        );
        assert!(
            (actual - expected).abs() <= EPSILON,
            "{left:?} vs {right:?}: C# {expected} / Rust {actual}"
        );
    }
}

#[test]
fn artist_tokens_match_dotnet() {
    for case in load("text-artists.json") {
        let input = case["input"].as_str().unwrap();
        let mut expected: Vec<Vec<u16>> = case["tokens"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| units(t.as_str().unwrap()))
            .collect();
        expected.sort();
        let mut actual: Vec<Vec<u16>> = split_artist_tokens(Some(input))
            .into_iter()
            .map(|t| t.units)
            .collect();
        actual.sort();
        assert_eq!(actual, expected, "{input:?}");
    }
}

#[test]
fn duration_similarity_matches_dotnet() {
    for case in load("text-duration.json") {
        let song = case["song"].as_i64().unwrap();
        let local = case["local"].as_i64().unwrap();
        let expected = case["score"].as_f64().unwrap();
        assert!(
            (duration_similarity(song, local) - expected).abs() <= EPSILON,
            "{song} vs {local}"
        );
    }
}

#[test]
fn file_name_guesses_match_dotnet() {
    let cases = load("filename-cases.json");
    assert!(cases.len() > 40);
    for case in cases {
        let name = case["fileName"].as_str().unwrap();
        let guess = parse_file_name(Path::new(&format!("C:\\music\\{name}")));
        assert_eq!(guess.title, text_of(&case["title"]), "{name} 标题");
        assert_eq!(guess.artist, text_of(&case["artist"]), "{name} 歌手");
    }
}

/// 所有 BMP 码元：`char.IsWhiteSpace / IsControl / IsPunctuation / IsSymbol` 与 `ToLowerInvariant` 的结果。
/// .NET 8 用的是 Unicode 15.0，这里的 ICU4X 数据更新；只有新版本才分配 / 改类别的字符允许不同。
#[test]
fn character_classes_and_lowercasing_match_dotnet_for_the_whole_bmp() {
    let table = load_object("bmp-classes.json");
    let mut expected_classes: Vec<char> = Vec::with_capacity(0x10000);
    for run in table["classes"].as_array().unwrap() {
        let kind = run[0].as_str().unwrap().chars().next().unwrap();
        let count = run[1].as_u64().unwrap() as usize;
        expected_classes.extend(std::iter::repeat_n(kind, count));
    }
    assert_eq!(expected_classes.len(), 0x10000);

    let mut lower = std::collections::HashMap::new();
    for pair in table["lower"].as_array().unwrap() {
        lower.insert(
            pair[0].as_u64().unwrap() as u32,
            pair[1].as_u64().unwrap() as u32,
        );
    }

    let mut class_diffs = Vec::new();
    let mut lower_diffs = Vec::new();
    for code in 0u32..=0xFFFF {
        let Some(c) = char::from_u32(code) else {
            // 代理项：.NET 里既不是空白也不是标点 / 符号 / 控制，也没有小写映射。
            assert_eq!(expected_classes[code as usize], '-');
            continue;
        };
        let actual = if is_dotnet_whitespace(c) {
            'w'
        } else if is_dotnet_control(c) {
            'c'
        } else if is_dotnet_punctuation(c) {
            'p'
        } else if is_dotnet_symbol(c) {
            's'
        } else {
            '-'
        };
        if actual != expected_classes[code as usize] {
            class_diffs.push(format!(
                "U+{code:04X}: C# {} / Rust {actual}",
                expected_classes[code as usize]
            ));
        }

        let expected_lower = lower.get(&code).copied().unwrap_or(code);
        if to_lower_invariant(c) as u32 != expected_lower {
            lower_diffs.push(format!(
                "U+{code:04X}: C# U+{expected_lower:04X} / Rust U+{:04X}",
                to_lower_invariant(c) as u32
            ));
        }
    }

    eprintln!(
        "BMP 对比（.NET {}）：类别差异 {} 个，小写差异 {} 个\n{}\n{}",
        table["runtime"].as_str().unwrap(),
        class_diffs.len(),
        lower_diffs.len(),
        class_diffs.join("\n"),
        lower_diffs.join("\n")
    );
    assert_eq!(
        class_diffs,
        known_newer_class_diffs(),
        "字符类别出现了预期之外的差异（Unicode 版本更新后可更新白名单）"
    );
    assert_eq!(
        lower_diffs,
        known_newer_lowercase_diffs(),
        "小写映射出现了预期之外的差异（Unicode 版本更新后可更新白名单）"
    );
}

/// .NET 8（Unicode 15.0）里还是未分配字符、而 ICU4X 已经把它们归为标点 / 符号的码点（Unicode 15.1 至 17 新增）。
fn known_newer_class_diffs() -> Vec<String> {
    const RANGES: [(u32, u32, char); 12] = [
        (0x1B4E, 0x1B4F, 'p'),
        (0x1B7F, 0x1B7F, 'p'),
        (0x20C1, 0x20C4, 's'),
        (0x2427, 0x2429, 's'),
        (0x2B96, 0x2B96, 's'),
        (0x2E60, 0x2E63, 'p'),
        (0x2FFC, 0x2FFF, 's'),
        (0x31E4, 0x31E5, 's'),
        (0x31EF, 0x31EF, 's'),
        (0xFBC3, 0xFBD2, 's'),
        (0xFD90, 0xFD91, 's'),
        (0xFDC8, 0xFDCE, 's'),
    ];
    RANGES
        .iter()
        .flat_map(|&(start, end, kind)| {
            (start..=end).map(move |code| format!("U+{code:04X}: C# - / Rust {kind}"))
        })
        .collect()
}

/// 同上：Unicode 16 新增的大小写对，.NET 8 还不认识。
fn known_newer_lowercase_diffs() -> Vec<String> {
    const PAIRS: [(u32, u32); 8] = [
        (0x1C89, 0x1C8A),
        (0xA7CB, 0x0264),
        (0xA7CC, 0xA7CD),
        (0xA7CE, 0xA7CF),
        (0xA7D2, 0xA7D3),
        (0xA7D4, 0xA7D5),
        (0xA7DA, 0xA7DB),
        (0xA7DC, 0x019B),
    ];
    PAIRS
        .iter()
        .map(|&(code, lower)| format!("U+{code:04X}: C# U+{code:04X} / Rust U+{lower:04X}"))
        .collect()
}

fn load_object(name: &str) -> Value {
    let bytes = std::fs::read(fixture_dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn song_from(value: &Value) -> NeteaseSong {
    NeteaseSong {
        id: value["id"].as_i64().unwrap(),
        name: value["name"].as_str().unwrap().to_owned(),
        alias: text_of(&value["alias"]),
        artists: value["artists"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, name)| ArtistRef {
                id: index as i64 + 1,
                name: name.as_str().unwrap().to_owned(),
            })
            .collect(),
        album: AlbumRef::default(),
        duration_ms: value["durationMs"].as_i64().unwrap() as i32,
        ..NeteaseSong::default()
    }
}

fn availability_of(name: &str) -> LocalAvailability {
    match name {
        "Available" => LocalAvailability::Available,
        "EncryptedNcm" => LocalAvailability::EncryptedNcm,
        "Missing" => LocalAvailability::Missing,
        other => panic!("未知状态 {other}"),
    }
}

fn method_of(name: &str) -> MatchMethod {
    match name {
        "None" => MatchMethod::None,
        "NeteaseId" => MatchMethod::NeteaseId,
        "Metadata" => MatchMethod::Metadata,
        "FileName" => MatchMethod::FileName,
        other => panic!("未知方式 {other}"),
    }
}

fn synthetic_track(index: usize, value: &Value) -> LocalTrack {
    let is_ncm = value["isNcm"].as_bool().unwrap();
    LocalTrack {
        path: PathBuf::from(format!("C:\\music\\{index:04}")),
        title: value["title"].as_str().unwrap().to_owned(),
        artist: text_of(&value["artist"]),
        album: None,
        duration_ms: value["durationMs"].as_i64().unwrap(),
        format: None,
        bitrate: None,
        is_ncm,
        ncm_metadata: None,
        netease_id: value["neteaseId"].as_i64(),
        has_cover: false,
        title_from_file_name: false,
    }
}

#[test]
fn random_playlists_match_exactly_like_csharp() {
    let cases = load("matcher-cases.json");
    assert_eq!(cases.len(), 5);
    let mut total_songs = 0;
    let mut max_diff = 0.0f64;
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let songs: Vec<NeteaseSong> = case["songs"]
            .as_array()
            .unwrap()
            .iter()
            .map(song_from)
            .collect();
        let tracks: Vec<LocalTrack> = case["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, track)| synthetic_track(index, track))
            .collect();
        let expected = case["expected"].as_array().unwrap();
        assert_eq!(songs.len(), expected.len());

        let matches = SongMatcher.match_songs(&songs, &tracks);

        assert_eq!(matches.len(), songs.len());
        for (index, (actual, want)) in matches.iter().zip(expected).enumerate() {
            let label = format!("{name} #{index}「{}」", songs[index].name);
            assert_eq!(
                actual.local_index.map(|i| i as i64),
                want["localIndex"].as_i64(),
                "{label} 本地文件"
            );
            assert_eq!(
                actual.method,
                method_of(want["method"].as_str().unwrap()),
                "{label} 方式"
            );
            assert_eq!(
                actual.availability,
                availability_of(want["availability"].as_str().unwrap()),
                "{label} 状态"
            );
            let diff = (actual.confidence - want["confidence"].as_f64().unwrap()).abs();
            max_diff = max_diff.max(diff);
            assert!(
                diff <= EPSILON,
                "{label} 分数 {} vs {}",
                actual.confidence,
                want["confidence"]
            );
        }
        total_songs += songs.len();
    }
    eprintln!("随机匹配：{total_songs} 首歌，分数最大差值 {max_diff:e}");
    assert_eq!(total_songs, 40 + 90 + 170 + 60 + 25);
}

fn music_dir() -> PathBuf {
    fixture_dir().join("music")
}

fn relative(path: &Path) -> String {
    path.strip_prefix(music_dir())
        .unwrap_or_else(|_| panic!("{} 不在夹具目录里", path.display()))
        .to_string_lossy()
        .replace('\\', "/")
}

fn scan_fixture() -> Vec<LocalTrack> {
    let service = LocalLibraryService::new(Arc::new(FileNcmDecoder), Arc::new(LoftyTagService));
    service
        .scan(&[music_dir()], None, &CancelToken::new())
        .expect("扫描夹具曲库")
        .tracks
}

#[test]
fn scanning_the_synthetic_library_agrees_with_csharp() {
    let expected = load("scan-expected.json");
    let tracks = scan_fixture();

    let actual_paths: Vec<String> = tracks.iter().map(|t| relative(&t.path)).collect();
    let expected_paths: Vec<String> = expected
        .iter()
        .map(|e| e["rel"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(actual_paths, expected_paths, "文件集合与顺序必须一致");
    assert_eq!(tracks.len(), 77);

    for (track, want) in tracks.iter().zip(&expected) {
        let label = want["rel"].as_str().unwrap();
        assert_eq!(
            track.title,
            want["title"].as_str().unwrap(),
            "{label} title"
        );
        assert_eq!(track.artist, text_of(&want["artist"]), "{label} artist");
        assert_eq!(track.album, text_of(&want["album"]), "{label} album");
        assert_eq!(track.format, text_of(&want["format"]), "{label} format");
        assert_eq!(
            track.is_ncm,
            want["isNcm"].as_bool().unwrap(),
            "{label} isNcm"
        );
        assert_eq!(
            track.netease_id,
            want["neteaseId"].as_i64(),
            "{label} neteaseId"
        );
        assert_eq!(
            track.has_cover,
            want["hasCover"].as_bool().unwrap(),
            "{label} hasCover"
        );

        // 时长 / 码率由不同的库（TagLib 与 lofty）按帧头估算，允许很小的偏差；
        // 其中一边读不出来（0）的情况必须两边一致。
        let want_duration = want["durationMs"].as_i64().unwrap();
        assert_eq!(
            track.duration_ms == 0,
            want_duration == 0,
            "{label} 有无时长"
        );
        assert!(
            (track.duration_ms - want_duration).abs() <= 30,
            "{label} 时长 {} vs {want_duration}",
            track.duration_ms
        );
        // 码率只是展示信息：NCM 来自元数据，必须一致；普通音频两边都读到时允许 20% 偏差
        // （静音的 FLAC / Opus 样本在 TagLib 里读不出码率，lofty 按文件大小估算）。
        if want["ncm"].is_object() {
            assert_eq!(
                track.bitrate.map(i64::from),
                want["bitrate"].as_i64(),
                "{label} bitrate"
            );
        } else if let (Some(actual), Some(want)) = (track.bitrate, want["bitrate"].as_i64()) {
            let ratio = f64::from(actual) / want as f64;
            assert!(
                (0.8..=1.2).contains(&ratio),
                "{label} 码率 {actual} vs {want}"
            );
        }

        match (&track.ncm_metadata, want["ncm"].as_object()) {
            (None, None) => {}
            (Some(actual), Some(want)) => {
                assert_eq!(actual.title, text_of(&want["title"]), "{label} ncm title");
                assert_eq!(
                    actual.artists,
                    text_of(&want["artists"]),
                    "{label} ncm artists"
                );
                assert_eq!(actual.album, text_of(&want["album"]), "{label} ncm album");
                assert_eq!(
                    actual.format,
                    text_of(&want["format"]),
                    "{label} ncm format"
                );
                assert_eq!(i64::from(actual.bitrate), want["bitrate"].as_i64().unwrap());
                assert_eq!(actual.duration_ms, want["durationMs"].as_i64().unwrap());
                assert_eq!(actual.music_id, want["musicId"].as_i64(), "{label} ncm id");
            }
            (actual, want) => panic!("{label}: NCM 元数据不一致 {actual:?} / {want:?}"),
        }
    }
}

fn csharp_scanned_tracks() -> Vec<LocalTrack> {
    load("scan-expected.json")
        .iter()
        .map(|want| {
            let ncm = want["ncm"].as_object().map(|n| NcmMetadata {
                title: text_of(&n["title"]),
                artists: text_of(&n["artists"]),
                album: text_of(&n["album"]),
                format: text_of(&n["format"]),
                bitrate: n["bitrate"].as_i64().unwrap() as i32,
                duration_ms: n["durationMs"].as_i64().unwrap(),
                music_id: n["musicId"].as_i64(),
                alias: None,
            });
            LocalTrack {
                path: music_dir().join(want["rel"].as_str().unwrap()),
                title: want["title"].as_str().unwrap().to_owned(),
                artist: text_of(&want["artist"]),
                album: text_of(&want["album"]),
                duration_ms: want["durationMs"].as_i64().unwrap(),
                format: text_of(&want["format"]),
                bitrate: want["bitrate"].as_i64().map(|b| b as i32),
                is_ncm: want["isNcm"].as_bool().unwrap(),
                ncm_metadata: ncm,
                netease_id: want["neteaseId"].as_i64(),
                has_cover: want["hasCover"].as_bool().unwrap(),
                title_from_file_name: false,
            }
        })
        .collect()
}

fn playlist() -> Vec<NeteaseSong> {
    load("songs.json").iter().map(song_from).collect()
}

#[test]
fn matching_the_csharp_scan_gives_csharp_results_exactly() {
    let tracks = csharp_scanned_tracks();
    let songs = playlist();
    let expected = load("match-expected.json");
    assert_eq!(songs.len(), expected.len());

    let matches = SongMatcher.match_songs(&songs, &tracks);

    let mut present = 0;
    for (index, (actual, want)) in matches.iter().zip(&expected).enumerate() {
        let label = format!("#{index}「{}」", songs[index].name);
        let actual_rel = actual.local_index.map(|i| relative(&tracks[i].path));
        assert_eq!(actual_rel, text_of(&want["rel"]), "{label} 本地文件");
        assert_eq!(
            actual.method,
            method_of(want["method"].as_str().unwrap()),
            "{label} 方式"
        );
        assert_eq!(
            actual.availability,
            availability_of(want["availability"].as_str().unwrap()),
            "{label} 状态"
        );
        assert!(
            (actual.confidence - want["confidence"].as_f64().unwrap()).abs() <= EPSILON,
            "{label} 分数"
        );
        present += usize::from(actual.is_present());
    }
    assert_eq!(present, 68);
}

#[test]
fn scanning_then_matching_in_rust_reaches_the_same_decisions_as_csharp() {
    let tracks = scan_fixture();
    let songs = playlist();
    let expected = load("match-expected.json");

    let matches = SongMatcher.match_songs(&songs, &tracks);

    for (index, (actual, want)) in matches.iter().zip(&expected).enumerate() {
        let label = format!("#{index}「{}」", songs[index].name);
        let actual_rel = actual.local_index.map(|i| relative(&tracks[i].path));
        assert_eq!(actual_rel, text_of(&want["rel"]), "{label} 本地文件");
        assert_eq!(
            actual.method,
            method_of(want["method"].as_str().unwrap()),
            "{label} 方式"
        );
        assert_eq!(
            actual.availability,
            availability_of(want["availability"].as_str().unwrap()),
            "{label} 状态"
        );
        // 两边估算的时长最多差几十毫秒，对 12 秒容差下的分数影响在 1e-3 以内。
        assert!(
            (actual.confidence - want["confidence"].as_f64().unwrap()).abs() <= 1e-3,
            "{label} 分数 {} vs {}",
            actual.confidence,
            want["confidence"]
        );
    }
}
