//! 下载与一键同步与 C# 版（`DownloadService` / `MusicSyncService` / `AudioTagService`）的交叉兼容测试。
//!
//! `tests/fixtures/download/scenarios.json` 描述 12 个场景（假的播放接口响应、假的 CDN 内容、预先存在的文件、选项），
//! `inputs/` 是“下载内容”（合成 MP3、静音 FLAC、合成 `.ncm`、封面），全部合成，没有任何真实歌曲。
//! 临时 C# 探针（已删除、不入库）用真实的 C# `DownloadService` / `MusicSyncService` / `NcmDecoder` / `AudioTagService`
//! 跑同样的场景，记录：每首歌的结果（成败、是否跳过、来源、输出文件名、消息）、输出目录里的文件、每个文件被
//! C# 读回的标签（含封面 SHA-256）、降档提示、出现过的阶段名，存成 `expected.json`。
//! 这里用 Rust 的 `DownloadService` / `SyncService` + 真实的 lofty 标签服务与 `FileNcmDecoder` 跑同样的场景，要求逐项一致。
//!
//! 已知且有意的差异（见 `docs/TAURI_REWRITE.md`）：C# 的 HTTP 错误消息带原因短语（`HTTP 404 Not Found`），探针已去掉；
//! 同步里的 NCM 解密阶段名不同（C# 把导入进度的消息当阶段名），只比较两边共有的阶段。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lp_core::download::fetch::MediaFetcher;
use lp_core::download::ncm_import::NcmImportService;
use lp_core::download::service::{
    DownloadApi, DownloadOptions, DownloadProgress, DownloadService, Downloader, ProgressSink,
};
use lp_core::library::LocalTrack;
use lp_core::lyrics::RawLyrics;
use lp_core::models::{AudioQuality, LyricsWriteMode};
use lp_core::ncm::{FileNcmDecoder, NcmDecoder};
use lp_core::netease::http::{BoxFuture, HttpRequest, HttpResponse, HttpTransport, TransportError};
use lp_core::netease::{AlbumRef, ArtistRef, NeteaseError, NeteaseSong, SongPlaybackInfo};
use lp_core::sync::{SyncOptions, SyncRequest, SyncService, SyncSourceKind};
use lp_core::tags::{AudioTagService, LoftyTagService};
use lp_core::utilities::CancelToken;
use serde_json::Value;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("download")
}

fn read_json(name: &str) -> Value {
    serde_json::from_slice(&fs::read(fixture_dir().join(name)).expect("读取夹具"))
        .expect("合法 JSON")
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("lp-csharp-dl-{tag}-{}", std::process::id()));
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
fn the_test_sha256_matches_a_known_vector() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

// ───────────────────────────── 场景装配 ─────────────────────────────

struct ScenarioApi {
    playback: HashMap<i64, Value>,
    lyrics: HashMap<i64, Value>,
    details: HashMap<i64, NeteaseSong>,
}

impl DownloadApi for ScenarioApi {
    fn playback_info<'a>(
        &'a self,
        song_id: i64,
        _quality: AudioQuality,
    ) -> BoxFuture<'a, Result<SongPlaybackInfo, NeteaseError>> {
        let info = match self.playback.get(&song_id) {
            None => SongPlaybackInfo {
                song_id,
                error_message: Some("无版权".to_owned()),
                ..SongPlaybackInfo::default()
            },
            Some(entry) => SongPlaybackInfo {
                song_id,
                url: entry["url"].as_str().map(str::to_owned),
                format: entry["format"].as_str().map(str::to_owned),
                level: entry["level"].as_str().map(str::to_owned),
                is_trial: entry["isTrial"].as_bool().unwrap_or(false),
                error_message: entry["error"].as_str().map(str::to_owned),
                ..SongPlaybackInfo::default()
            },
        };
        Box::pin(async move { Ok(info) })
    }

    fn lyrics<'a>(&'a self, song_id: i64) -> BoxFuture<'a, Result<RawLyrics, NeteaseError>> {
        let raw = match self.lyrics.get(&song_id) {
            None => RawLyrics::default(),
            Some(entry) => RawLyrics {
                lrc: entry["lrc"].as_str().map(str::to_owned),
                translated_lrc: entry["tlyric"].as_str().map(str::to_owned),
                romanized_lrc: None,
                yrc: entry["yrc"].as_str().map(str::to_owned),
                qrc: None,
            },
        };
        Box::pin(async move { Ok(raw) })
    }

    fn song_detail<'a>(
        &'a self,
        song_id: i64,
    ) -> BoxFuture<'a, Result<Option<NeteaseSong>, NeteaseError>> {
        let found = self.details.get(&song_id).cloned();
        Box::pin(async move { Ok(found) })
    }
}

struct Cdn(HashMap<String, Vec<u8>>);

impl HttpTransport for Cdn {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        let response = match self.0.get(&request.url) {
            Some(body) => HttpResponse {
                status: 200,
                set_cookies: Vec::new(),
                body: body.clone(),
            },
            None => HttpResponse {
                status: 404,
                set_cookies: Vec::new(),
                body: Vec::new(),
            },
        };
        Box::pin(async move { Ok(response) })
    }
}

fn to_song(value: &Value) -> NeteaseSong {
    NeteaseSong {
        id: value["id"].as_i64().unwrap(),
        name: value["name"].as_str().unwrap().to_owned(),
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
        album: AlbumRef {
            id: 2,
            name: value["album"].as_str().unwrap().to_owned(),
            cover_url: value["coverUrl"].as_str().map(str::to_owned),
        },
        alias: value["alias"].as_str().map(str::to_owned),
        duration_ms: 200_000,
        track_number: value["trackNumber"].as_i64().unwrap() as i32,
        disc_number: value["discNumber"].as_i64().unwrap() as i32,
        release_date: value["releaseDate"].as_str().map(str::to_owned),
        fee: 0,
        max_bitrate: 0,
    }
}

fn options_of(scenario: &Value, directory: &Path) -> DownloadOptions {
    let o = &scenario["options"];
    let mut options = DownloadOptions::new(directory);
    options.quality = AudioQuality::from_name(o["quality"].as_str().unwrap()).unwrap();
    options.lyrics_mode = LyricsWriteMode::from_name(o["lyricsMode"].as_str().unwrap()).unwrap();
    options.write_cover = o["writeCover"].as_bool().unwrap();
    options.overwrite_existing = o["overwriteExisting"].as_bool().unwrap_or(false);
    options.skip_existing = o["skipExisting"].as_bool().unwrap_or(true);
    if let Some(template) = o["fileNameTemplate"].as_str() {
        options.file_name_template = template.to_owned();
    }
    options.max_concurrency = 1;
    options.auto_match_online = false;
    options
}

/// 一个场景的运行结果，形状与探针写出的 `expected.json` 一致。
struct Observed {
    results: Vec<Value>,
    files: BTreeMap<String, Value>,
    notes: BTreeSet<String>,
    stages: BTreeSet<String>,
}

fn tag_summary(tags: &dyn AudioTagService, path: &Path) -> Value {
    let Some(info) = tags.read(path).unwrap() else {
        return Value::Null;
    };
    serde_json::json!({
        "title": info.title,
        "artist": info.artist,
        "album": info.album,
        "albumArtist": info.album_artist,
        "year": info.year,
        "trackNumber": info.track_number,
        "discNumber": info.disc_number,
        "lyrics": info.lyrics,
        "neteaseId": info.netease_id,
        "comment": info.comment,
        "hasCover": info.has_cover,
        "coverSha256": info.cover_data.as_deref().filter(|c| !c.is_empty()).map(sha256_hex),
        "fileFormat": info.file_format,
    })
}

async fn run_scenario(scenario: &Value) -> Observed {
    let work = Scratch::new(scenario["name"].as_str().unwrap());
    let out_dir = work.0.join("out");
    let local_dir = work.0.join("local");
    fs::create_dir_all(&out_dir).unwrap();
    fs::create_dir_all(&local_dir).unwrap();
    let inputs = fixture_dir().join("inputs");

    for pre in scenario["preexisting"].as_array().into_iter().flatten() {
        fs::copy(
            inputs.join(pre["file"].as_str().unwrap()),
            out_dir.join(pre["name"].as_str().unwrap()),
        )
        .unwrap();
    }

    let songs: Vec<NeteaseSong> = scenario["songs"]
        .as_array()
        .unwrap()
        .iter()
        .map(to_song)
        .collect();
    let as_map = |key: &str| -> HashMap<i64, Value> {
        scenario[key]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(id, value)| (id.parse().unwrap(), value.clone()))
            .collect()
    };
    let api = Arc::new(ScenarioApi {
        playback: as_map("playback"),
        lyrics: as_map("lyrics"),
        details: as_map("details")
            .into_iter()
            .map(|(id, value)| (id, to_song(&value)))
            .collect(),
    });

    let mut responses = HashMap::new();
    for (url, file) in scenario["http"].as_object().into_iter().flatten() {
        let file = file.as_str().unwrap();
        let body = if file == "@empty" {
            Vec::new()
        } else {
            fs::read(inputs.join(file)).unwrap()
        };
        responses.insert(url.clone(), body);
    }
    let fetcher = MediaFetcher::new(Arc::new(Cdn(responses)));

    let tags: Arc<dyn AudioTagService> = Arc::new(LoftyTagService);
    let decoder: Arc<dyn NcmDecoder> = Arc::new(FileNcmDecoder);
    let downloader = Arc::new(DownloadService::new(
        api.clone(),
        Arc::clone(&tags),
        Arc::clone(&decoder),
        fetcher.clone(),
        None,
    ));
    let options = options_of(scenario, &out_dir);

    let stages = Arc::new(Mutex::new(BTreeSet::new()));
    let notes = Arc::new(Mutex::new(BTreeSet::new()));
    let (stage_store, note_store) = (Arc::clone(&stages), Arc::clone(&notes));
    let sink: ProgressSink = Arc::new(move |progress: DownloadProgress| {
        stage_store.lock().unwrap().insert(progress.stage.clone());
        if let Some(message) = &progress.message
            && message.contains("已自动降级")
        {
            note_store.lock().unwrap().insert(message.clone());
        }
    });

    let mut results = Vec::new();
    let relative = |path: &Path| -> String {
        path.strip_prefix(&out_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    };
    if scenario["mode"] == "download" {
        let outcome = downloader
            .download(songs, options, sink, &CancelToken::new())
            .await
            .unwrap();
        for result in &outcome.results {
            results.push(serde_json::json!({
                "songId": result.song_id,
                "success": result.success,
                "skipped": result.skipped,
                "source": if result.skipped { "skipped" } else if !result.success { "failed" } else { "downloaded" },
                "output": result.output_path.as_deref().map(relative),
                "message": result.error_message,
            }));
        }
    } else {
        let importer = Arc::new(NcmImportService::new(
            Arc::clone(&decoder),
            Arc::clone(&tags),
            None,
        ));
        let sync = Arc::new(SyncService::new(
            api.clone(),
            decoder,
            Arc::clone(&tags),
            importer,
            downloader.clone() as Arc<dyn Downloader>,
            fetcher,
            None,
        ));
        let local = &scenario["local"];
        let requests: Vec<SyncRequest> = songs
            .into_iter()
            .map(|song| {
                let local_track = local[song.id.to_string()].as_object().map(|entry| {
                    let path = local_dir.join(entry["name"].as_str().unwrap());
                    fs::copy(inputs.join(entry["file"].as_str().unwrap()), &path).unwrap();
                    let is_ncm = entry["isNcm"].as_bool().unwrap();
                    LocalTrack {
                        path,
                        title: "本地标题".to_owned(),
                        artist: Some("本地歌手".to_owned()),
                        album: None,
                        duration_ms: 240_000,
                        format: Some(if is_ncm { "flac" } else { "mp3" }.to_owned()),
                        bitrate: None,
                        is_ncm,
                        ncm_metadata: None,
                        netease_id: None,
                        has_cover: false,
                        title_from_file_name: false,
                    }
                });
                SyncRequest {
                    song,
                    local: local_track,
                }
            })
            .collect();
        let outcome = sync
            .sync(
                requests,
                SyncOptions {
                    download: options,
                    redownload_existing: false,
                },
                sink,
                &CancelToken::new(),
            )
            .await
            .unwrap();
        for result in &outcome.results {
            let source = match result.source {
                SyncSourceKind::Downloaded => "downloaded",
                SyncSourceKind::DecryptedNcm => "decryptedNcm",
                SyncSourceKind::Skipped => "skipped",
                SyncSourceKind::Failed => "failed",
            };
            let output = result.output_path.as_deref().map(|path| {
                if result.source == SyncSourceKind::Skipped {
                    path.file_name().unwrap().to_string_lossy().into_owned()
                } else {
                    relative(path)
                }
            });
            results.push(serde_json::json!({
                "songId": result.song_id,
                "success": result.success,
                "skipped": result.source == SyncSourceKind::Skipped,
                "source": source,
                "output": output,
                "message": result.message,
            }));
        }
    }

    let mut files = BTreeMap::new();
    for entry in fs::read_dir(&out_dir).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let mut value = serde_json::json!({ "isDirectory": path.is_dir() });
        if path.is_file() {
            let summary = tag_summary(tags.as_ref(), &path);
            if !summary.is_null() {
                value["tags"] = summary;
            }
        }
        files.insert(name, value);
    }
    let notes = notes.lock().unwrap().clone();
    let stages = stages.lock().unwrap().clone();
    Observed {
        results,
        files,
        notes,
        stages,
    }
}

// ───────────────────────────── 比较 ─────────────────────────────

fn string_set(value: &Value) -> BTreeSet<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect()
}

/// 同步里 C# 把导入进度的消息当阶段名；这些阶段 Rust 里没有对应物，比较时去掉。
const CSHARP_ONLY_SYNC_STAGES: [&str; 2] = ["正在解密 NCM…", "完成"];

#[tokio::test]
async fn every_scenario_matches_what_the_csharp_services_produce() {
    let scenarios = read_json("scenarios.json");
    let expected = read_json("expected.json");
    let scenarios = scenarios.as_array().unwrap();
    assert_eq!(scenarios.len(), expected.as_array().unwrap().len());
    assert!(scenarios.len() >= 12);

    for (scenario, want) in scenarios.iter().zip(expected.as_array().unwrap()) {
        let name = scenario["name"].as_str().unwrap();
        assert_eq!(want["name"], scenario["name"]);
        let got = run_scenario(scenario).await;

        // 每首歌的结果。
        let want_results = want["results"].as_array().unwrap();
        assert_eq!(got.results.len(), want_results.len(), "{name}：结果数量");
        for (got, want) in got.results.iter().zip(want_results) {
            for key in [
                "songId", "success", "skipped", "source", "output", "message",
            ] {
                assert_eq!(
                    got[key], want[key],
                    "{name}：歌曲 {} 的 {key}",
                    want["songId"]
                );
            }
        }

        // 输出目录里的文件（含没有遗留临时文件 / 目录）与每个文件的标签。
        let want_names: BTreeSet<String> =
            want["files"].as_object().unwrap().keys().cloned().collect();
        let got_names: BTreeSet<String> = got.files.keys().cloned().collect();
        assert_eq!(got_names, want_names, "{name}：输出目录里的文件");
        for (file, want_file) in want["files"].as_object().unwrap() {
            let got_file = &got.files[file];
            assert_eq!(
                got_file["isDirectory"], want_file["isDirectory"],
                "{name}：{file}"
            );
            assert_eq!(got_file["tags"], want_file["tags"], "{name}：{file} 的标签");
        }

        // 降档提示逐字相同。
        assert_eq!(got.notes, string_set(&want["notes"]), "{name}：降档提示");

        // 阶段：下载场景两边完全相同；同步场景只比较两边共有的阶段。
        let mut want_stages = string_set(&want["stages"]);
        let mut got_stages = got.stages.clone();
        if scenario["mode"] == "sync" {
            for stage in CSHARP_ONLY_SYNC_STAGES {
                want_stages.remove(stage);
                got_stages.remove(stage);
            }
            // Rust 的同步在导入阶段用“解密 NCM”，C# 同名阶段也存在；其余阶段必须出现在两边。
            let common: BTreeSet<_> = want_stages.intersection(&got_stages).cloned().collect();
            for stage in [
                "处理中",
                "已跳过",
                "NCM 解密完成",
                "下载完成",
                "获取歌曲信息",
                "写入标签",
            ] {
                if want_stages.contains(stage) {
                    assert!(
                        common.contains(stage),
                        "{name}：缺少阶段 {stage}（Rust 有 {got_stages:?}）"
                    );
                }
            }
        } else {
            assert_eq!(got_stages, want_stages, "{name}：阶段");
        }
    }
}
