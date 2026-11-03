//! 曲库扫描与匹配的单元测试。C# 的 `LocalLibraryServiceTests` / `SongMatcherTests` 全部移植，另加：
//! ID 精确优先于高分模糊、一对一冲突、文件名猜测、NCM 元数据参与匹配、无权限目录、取消、并行读取下的稳定顺序。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::*;
use crate::logs::{LogHub, LogLevel};
use crate::ncm::{NcmDecoder, NcmDecryptResult, NcmError, NcmFileInfo, NcmMetadata};
use crate::netease::{AlbumRef, ArtistRef, NeteaseSong};
use crate::tags::{AudioTagData, AudioTagInfo, AudioTagService, TagError};
use crate::test_util::TempDir;
use crate::utilities::CancelToken;

// ---- 测试替身 ----

fn key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

#[derive(Default)]
struct FakeTags {
    entries: Mutex<HashMap<String, AudioTagInfo>>,
    failures: Mutex<HashSet<String>>,
    calls: Mutex<Vec<String>>,
    reads: AtomicUsize,
    /// 第一次读标签时触发（用来在扫描中途取消）。
    on_first_read: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl FakeTags {
    fn set(&self, path: &Path, info: AudioTagInfo) {
        self.entries.lock().unwrap().insert(key(path), info);
    }

    fn fail(&self, path: &Path) {
        self.failures.lock().unwrap().insert(key(path));
    }
}

impl AudioTagService for FakeTags {
    fn read(&self, audio_path: &Path) -> Result<Option<AudioTagInfo>, TagError> {
        if self.reads.fetch_add(1, Ordering::SeqCst) == 0
            && let Some(hook) = self.on_first_read.lock().unwrap().as_ref()
        {
            hook();
        }
        self.calls.lock().unwrap().push(key(audio_path));
        if self.failures.lock().unwrap().contains(&key(audio_path)) {
            return Err(TagError::Unsupported("bad tag".to_owned()));
        }
        Ok(self.entries.lock().unwrap().get(&key(audio_path)).cloned())
    }

    fn write(&self, _audio_path: &Path, _tags: &AudioTagData) -> Result<(), TagError> {
        unreachable!("扫描测试不需要写标签")
    }
}

#[derive(Default)]
struct FakeNcm {
    infos: Mutex<HashMap<String, NcmFileInfo>>,
    failures: Mutex<HashSet<String>>,
    read_calls: Mutex<Vec<String>>,
}

impl FakeNcm {
    fn set(&self, path: &Path, metadata: NcmMetadata) {
        self.infos.lock().unwrap().insert(
            key(path),
            NcmFileInfo {
                source_path: path.to_path_buf(),
                metadata: Some(metadata),
            },
        );
    }

    fn fail(&self, path: &Path) {
        self.failures.lock().unwrap().insert(key(path));
    }
}

impl NcmDecoder for FakeNcm {
    fn is_ncm_file(&self, path: &Path) -> bool {
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ncm"))
    }

    fn read_info(&self, ncm_path: &Path) -> Result<NcmFileInfo, NcmError> {
        self.read_calls.lock().unwrap().push(key(ncm_path));
        if self.failures.lock().unwrap().contains(&key(ncm_path)) {
            return Err(NcmError::InvalidData("truncated header".to_owned()));
        }
        Ok(self
            .infos
            .lock()
            .unwrap()
            .get(&key(ncm_path))
            .cloned()
            .unwrap_or(NcmFileInfo {
                source_path: ncm_path.to_path_buf(),
                metadata: None,
            }))
    }

    fn decrypt(
        &self,
        _ncm_path: &Path,
        _output_directory: &Path,
        _overwrite: bool,
        _cancel: &CancelToken,
    ) -> Result<NcmDecryptResult, NcmError> {
        unreachable!("扫描测试不需要解密")
    }
}

struct Fixture {
    tags: Arc<FakeTags>,
    ncm: Arc<FakeNcm>,
    service: LocalLibraryService,
    dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let tags = Arc::new(FakeTags::default());
        let ncm = Arc::new(FakeNcm::default());
        let service = LocalLibraryService::new(ncm.clone(), tags.clone());
        Self {
            tags,
            ncm,
            service,
            dir: TempDir::new(),
        }
    }

    fn root(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    fn file(&self, relative: &str) -> PathBuf {
        let path = self.dir.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, [0x49, 0x44, 0x33]).unwrap();
        path
    }

    fn scan(&self) -> ScanOutcome {
        self.service
            .scan(&[self.root()], None, &CancelToken::new())
            .unwrap()
    }
}

fn tag_info(title: &str, artist: &str, duration_ms: i64) -> AudioTagInfo {
    AudioTagInfo {
        title: Some(title.to_owned()),
        artist: Some(artist.to_owned()),
        duration_ms,
        ..AudioTagInfo::default()
    }
}

fn find<'a>(tracks: &'a [LocalTrack], suffix: &str) -> &'a LocalTrack {
    let mut found = tracks
        .iter()
        .filter(|t| t.path.to_string_lossy().ends_with(suffix));
    let track = found.next().unwrap_or_else(|| panic!("没有 {suffix}"));
    assert!(found.next().is_none(), "{suffix} 出现了多次");
    track
}

// ---- 扫描（C# LocalLibraryServiceTests） ----

#[test]
fn scan_finds_supported_files_recursively_and_dedupes_overlapping_roots() {
    let f = Fixture::new();
    for name in [
        "a.mp3",
        "album/b.flac",
        "album/disc/c.m4a",
        "d.mp4",
        "e.ogg",
        "f.opus",
        "g.wav",
        "h.ncm",
        "ignore.txt",
    ] {
        f.file(name);
    }
    let album = f.root().join("album");
    let mut with_slash = f.root().into_os_string();
    with_slash.push("\\");

    let outcome = f
        .service
        .scan(
            &[f.root(), album, PathBuf::from(with_slash)],
            None,
            &CancelToken::new(),
        )
        .unwrap();

    let tracks = &outcome.tracks;
    assert_eq!(tracks.len(), 8);
    let unique: HashSet<String> = tracks.iter().map(|t| key(&t.path)).collect();
    assert_eq!(unique.len(), tracks.len());
    assert!(tracks.iter().all(|t| t.path.is_file()));
    assert!(
        tracks
            .iter()
            .all(|t| !t.path.to_string_lossy().ends_with(".txt"))
    );
    let mut formats: Vec<&str> = tracks
        .iter()
        .map(|t| t.format.as_deref().unwrap())
        .collect();
    formats.sort_unstable();
    assert_eq!(
        formats,
        ["flac", "m4a", "mp3", "mp4", "ncm", "ogg", "opus", "wav"]
    );
    assert_eq!(tracks.iter().filter(|t| t.is_ncm).count(), 1);
    assert_eq!(outcome.ncm_count(), 1);
}

#[test]
fn scan_uses_tag_metadata_for_audio_files() {
    let f = Fixture::new();
    let path = f.file("周杰伦 - 晴天.mp3");
    f.tags.set(
        &path,
        AudioTagInfo {
            title: Some("晴天".into()),
            artist: Some("周杰伦".into()),
            album: Some("叶惠美".into()),
            album_artist: Some("周杰伦".into()),
            duration_ms: 269_000,
            bitrate: 320,
            lyrics: Some("[00:01.00]故事的小黄花".into()),
            has_cover: true,
            netease_id: Some(186_016),
            ..AudioTagInfo::default()
        },
    );

    let tracks = f.scan().tracks;

    assert_eq!(tracks.len(), 1);
    let track = &tracks[0];
    assert_eq!(track.path, path);
    assert_eq!(track.title, "晴天");
    assert_eq!(track.artist.as_deref(), Some("周杰伦"));
    assert_eq!(track.album.as_deref(), Some("叶惠美"));
    assert_eq!(track.duration_ms, 269_000);
    assert_eq!(track.format.as_deref(), Some("mp3"));
    assert_eq!(track.bitrate, Some(320));
    assert!(track.has_cover);
    assert!(!track.is_ncm);
    assert_eq!(track.netease_id, Some(186_016));
    assert!(!track.title_from_file_name);
}

#[test]
fn scan_falls_back_to_the_file_name_when_tags_are_missing_or_unreadable() {
    let f = Fixture::new();
    f.file("周杰伦 - 晴天.flac");
    f.file("01. 普通歌曲.ogg");
    let broken = f.file("损坏文件.mp3");
    f.tags.fail(&broken);

    let tracks = f.scan().tracks;

    assert_eq!(tracks.len(), 3);
    let sunny = find(&tracks, "晴天.flac");
    assert_eq!(sunny.title, "晴天");
    assert_eq!(sunny.artist.as_deref(), Some("周杰伦"));
    assert!(sunny.title_from_file_name);
    let plain = find(&tracks, "普通歌曲.ogg");
    assert_eq!(plain.title, "普通歌曲");
    assert_eq!(plain.artist, None);
    let broken_track = find(&tracks, "损坏文件.mp3");
    assert_eq!(broken_track.title, "损坏文件");
    assert_eq!(broken_track.format.as_deref(), Some("mp3"));
    assert_eq!(broken_track.duration_ms, 0);
}

#[test]
fn scan_reads_ncm_header_metadata_and_survives_a_broken_ncm() {
    let f = Fixture::new();
    let ncm_path = f.file("downloads/Lucky.ncm");
    let broken_path = f.file("downloads/broken.ncm");
    f.ncm.set(
        &ncm_path,
        NcmMetadata {
            title: Some("Lucky".into()),
            artists: Some("Jason Mraz/Colbie Caillat".into()),
            album: Some("We Sing. We Dance.".into()),
            format: Some("flac".into()),
            bitrate: 999_000,
            duration_ms: 245_000,
            music_id: Some(123_456),
            alias: None,
        },
    );
    f.ncm.fail(&broken_path);

    let tracks = f.scan().tracks;

    assert_eq!(tracks.len(), 2);
    let lucky = find(&tracks, "Lucky.ncm");
    assert!(lucky.is_ncm);
    assert_eq!(lucky.title, "Lucky");
    assert_eq!(lucky.artist.as_deref(), Some("Jason Mraz/Colbie Caillat"));
    assert_eq!(lucky.album.as_deref(), Some("We Sing. We Dance."));
    assert_eq!(lucky.format.as_deref(), Some("flac"));
    assert_eq!(lucky.bitrate, Some(999_000));
    assert_eq!(lucky.duration_ms, 245_000);
    assert_eq!(lucky.netease_id, Some(123_456));
    assert!(lucky.ncm_metadata.is_some());
    assert!(!lucky.has_cover);
    assert!(!lucky.title_from_file_name);

    let broken = find(&tracks, "broken.ncm");
    assert!(broken.is_ncm);
    assert_eq!(broken.title, "broken");
    assert!(broken.ncm_metadata.is_none());
    assert_eq!(broken.netease_id, None);
    assert_eq!(broken.format.as_deref(), Some("ncm"));
    assert!(broken.title_from_file_name);
}

#[test]
fn scan_ignores_missing_directories_blank_paths_and_unsupported_files() {
    let f = Fixture::new();
    f.file("notes.txt");
    let missing = f.root().join("not-there");

    let outcome = f
        .service
        .scan(
            &[missing, f.root(), PathBuf::from(" "), PathBuf::new()],
            None,
            &CancelToken::new(),
        )
        .unwrap();

    assert!(outcome.tracks.is_empty());
}

#[test]
fn scan_reports_monotonic_progress() {
    let f = Fixture::new();
    f.file("1.mp3");
    f.file("2.mp3");
    f.file("3.flac");
    let reports = Mutex::new(Vec::new());

    let outcome = f
        .service
        .scan(
            &[f.root()],
            Some(&|p| reports.lock().unwrap().push(p)),
            &CancelToken::new(),
        )
        .unwrap();

    assert_eq!(outcome.tracks.len(), 3);
    let reports = reports.into_inner().unwrap();
    assert_eq!(reports.len(), outcome.tracks.len() + 1);
    assert!(reports.iter().all(|r| r.total_files == 3));
    assert_eq!(reports[0].scanned_files, 0);
    assert_eq!(reports.last().unwrap().scanned_files, 3);
    assert!(!reports.last().unwrap().current_file.as_os_str().is_empty());
    for pair in reports.windows(2) {
        assert!(pair[1].scanned_files >= pair[0].scanned_files);
    }
}

#[test]
fn scan_honours_a_cancellation_requested_up_front() {
    let f = Fixture::new();
    f.file("a.mp3");
    let cancel = CancelToken::new();
    cancel.cancel();

    let result = f.service.scan(&[f.root()], None, &cancel);

    assert_eq!(result.unwrap_err(), LibraryError::Cancelled);
    assert_eq!(f.tags.reads.load(Ordering::SeqCst), 0);
}

#[test]
fn scan_stops_reading_when_cancelled_midway_and_keeps_no_partial_result() {
    let f = Fixture::new();
    for index in 0..200 {
        f.file(&format!("song-{index:03}.mp3"));
    }
    let cancel = CancelToken::new();
    let hook = cancel.clone();
    *f.tags.on_first_read.lock().unwrap() = Some(Box::new(move || hook.cancel()));

    let result = f.service.scan(&[f.root()], None, &cancel);

    assert_eq!(result.unwrap_err(), LibraryError::Cancelled);
    // 取消后每个工作线程最多再读完手头那一个文件。
    assert!(
        f.tags.reads.load(Ordering::SeqCst) < 200,
        "取消后应停止读取，实际读了 {}",
        f.tags.reads.load(Ordering::SeqCst)
    );
}

#[test]
fn scan_does_not_follow_directory_symlinks() {
    let f = Fixture::new();
    f.file("music/a.mp3");
    let music = f.root().join("music");
    let link = music.join("loop");
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(&music, &link);
    #[cfg(not(windows))]
    let linked = std::os::unix::fs::symlink(&music, &link);
    if linked.is_err() {
        // 没有创建符号链接的权限：跳过（遍历里另有已访问目录集合兜底）。
        return;
    }

    let outcome = f.service.scan(&[music], None, &CancelToken::new()).unwrap();

    assert_eq!(outcome.tracks.len(), 1);
}

#[test]
fn scan_results_are_sorted_case_insensitively_and_stable_under_parallel_reads() {
    let f = Fixture::new();
    for index in 0..120 {
        let name = if index % 3 == 0 {
            format!("Dir{}/Z-{index:03}.MP3", index % 5)
        } else if index % 3 == 1 {
            format!("dir{}/a-{index:03}.flac", index % 5)
        } else {
            format!("DIR{}/M-{index:03}.Ogg", index % 5)
        };
        f.file(&name);
    }

    let first = f.scan().tracks;
    let second = f.scan().tracks;

    assert_eq!(first.len(), 120);
    assert_eq!(first, second);
    let upper: Vec<String> = first
        .iter()
        .map(|t| t.path.to_string_lossy().to_uppercase())
        .collect();
    let mut sorted = upper.clone();
    sorted.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    assert_eq!(upper, sorted);
}

#[cfg(windows)]
#[test]
fn scan_skips_a_directory_it_cannot_read_and_counts_it() {
    use std::process::Command;

    let f = Fixture::new();
    f.file("open/a.mp3");
    let locked = f.root().join("locked");
    fs::create_dir_all(&locked).unwrap();
    fs::write(locked.join("hidden.mp3"), [0x49, 0x44, 0x33]).unwrap();

    // `whoami` 给出 `域\用户`，icacls 才认得（环境变量 USERNAME 在个别环境里解析不出账号）。
    let user = Command::new("whoami")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    let deny = format!("{user}:(RD)");
    let denied = Command::new("icacls")
        .arg(&locked)
        .args(["/deny", &deny])
        .output()
        .is_ok_and(|output| output.status.success());
    if !denied || fs::read_dir(&locked).is_ok() {
        // 这个环境改不了 ACL（或拒绝项没有生效）：无法构造“无权限目录”，跳过。
        eprintln!("跳过：无法在本环境构造无权限目录");
        let _ = Command::new("icacls")
            .arg(&locked)
            .args(["/remove:d", &user])
            .output();
        return;
    }

    let outcome = f.service.scan(&[f.root()], None, &CancelToken::new());
    // 先还原权限，保证临时目录能被清理。
    let _ = Command::new("icacls")
        .arg(&locked)
        .args(["/remove:d", &user])
        .output();

    let outcome = outcome.unwrap();
    assert_eq!(outcome.tracks.len(), 1, "只扫到能读的目录");
    assert!(outcome.tracks[0].path.ends_with("a.mp3"));
    assert_eq!(outcome.skipped_directories, 1);
}

#[test]
fn unreadable_tags_do_not_stop_the_other_files() {
    let f = Fixture::new();
    let bad = f.file("a-bad.mp3");
    let good = f.file("b-good.mp3");
    f.tags.fail(&bad);
    f.tags.set(&good, tag_info("好歌", "某人", 200_000));

    let tracks = f.scan().tracks;

    assert_eq!(tracks.len(), 2);
    assert_eq!(find(&tracks, "b-good.mp3").title, "好歌");
    assert_eq!(find(&tracks, "a-bad.mp3").title, "a-bad");
}

#[test]
fn blank_tag_values_are_trimmed_or_replaced_by_the_file_name() {
    let f = Fixture::new();
    let path = f.file("李荣浩 - 麻雀.mp3");
    f.tags.set(
        &path,
        AudioTagInfo {
            title: Some("   ".into()),
            artist: Some(" 李荣浩 ".into()),
            album: Some("  ".into()),
            duration_ms: -5,
            bitrate: 0,
            netease_id: Some(0),
            ..AudioTagInfo::default()
        },
    );

    let tracks = f.scan().tracks;

    let track = &tracks[0];
    assert_eq!(track.title, "麻雀");
    assert_eq!(track.artist.as_deref(), Some("李荣浩"));
    assert_eq!(track.album, None);
    assert_eq!(track.duration_ms, 0);
    assert_eq!(track.bitrate, None);
    assert_eq!(track.netease_id, None);
    assert!(track.title_from_file_name);
}

// ---- 文件名推断 ----

fn guess(name: &str) -> (Option<String>, Option<String>) {
    let guess = parse_file_name(Path::new(name));
    (guess.title, guess.artist)
}

#[test]
fn file_name_guess_splits_artist_and_title() {
    assert_eq!(
        guess("周杰伦 - 晴天.mp3"),
        (Some("晴天".into()), Some("周杰伦".into()))
    );
    // 全角 / 长破折号 / 连字符。
    assert_eq!(
        guess("周杰伦 － 晴天.mp3"),
        (Some("晴天".into()), Some("周杰伦".into()))
    );
    assert_eq!(
        guess("A — B – C.flac"),
        (Some("B – C".into()), Some("A".into()))
    );
}

#[test]
fn file_name_guess_strips_track_number_prefixes() {
    assert_eq!(guess("01. 普通歌曲.ogg"), (Some("普通歌曲".into()), None));
    assert_eq!(guess("02 - 歌名.mp3"), (Some("歌名".into()), None));
    assert_eq!(guess("3) 歌名.mp3"), (Some("歌名".into()), None));
    assert_eq!(guess("07 Song.mp3"), (Some("Song".into()), None));
    assert_eq!(
        guess("01. 周杰伦 - 晴天.mp3"),
        (Some("晴天".into()), Some("周杰伦".into()))
    );
    // 三位以上的数字不是音轨号。
    assert_eq!(guess("1989.mp3"), (Some("1989".into()), None));
}

#[test]
fn file_name_guess_needs_spaces_around_the_hyphen_and_both_sides() {
    assert_eq!(guess("Jay-Chou.mp3"), (Some("Jay-Chou".into()), None));
    assert_eq!(guess("歌手 - .mp3"), (Some("歌手 -".into()), None));
    assert_eq!(guess(".mp3"), (None, None));
    assert_eq!(guess("  .mp3"), (None, None));
    // 只有数字、没有分隔符的不算音轨号。
    assert_eq!(guess("01.mp3"), (Some("01".into()), None));
}

// ---- 匹配（C# SongMatcherTests） ----

fn song(
    id: i64,
    name: &str,
    artists: &[&str],
    duration_ms: i32,
    alias: Option<&str>,
) -> NeteaseSong {
    NeteaseSong {
        id,
        name: name.to_owned(),
        alias: alias.map(str::to_owned),
        artists: artists
            .iter()
            .enumerate()
            .map(|(index, name)| ArtistRef {
                id: index as i64 + 1,
                name: (*name).to_owned(),
            })
            .collect(),
        album: AlbumRef {
            id: 1,
            name: "专辑".into(),
            cover_url: None,
        },
        duration_ms,
        track_number: 1,
        ..NeteaseSong::default()
    }
}

fn track(
    path: &str,
    title: &str,
    artist: Option<&str>,
    duration_ms: i64,
    is_ncm: bool,
    netease_id: Option<i64>,
) -> LocalTrack {
    LocalTrack {
        path: PathBuf::from(path),
        title: title.to_owned(),
        artist: artist.map(str::to_owned),
        album: None,
        duration_ms,
        format: Some(if is_ncm { "flac" } else { "mp3" }.to_owned()),
        bitrate: None,
        is_ncm,
        ncm_metadata: None,
        netease_id,
        has_cover: false,
        title_from_file_name: false,
    }
}

fn plain(path: &str, title: &str, artist: &str, duration_ms: i64) -> LocalTrack {
    track(path, title, Some(artist), duration_ms, false, None)
}

fn run(songs: &[NeteaseSong], locals: &[LocalTrack]) -> Vec<SongMatch> {
    SongMatcher.match_songs(songs, locals)
}

#[test]
fn match_prefers_an_exact_netease_id_over_metadata() {
    let songs = [song(42, "晴天", &["周杰伦"], 269_000, None)];
    let locals = [track(
        "C:\\music\\whatever.mp3",
        "完全不同",
        Some("别的歌手"),
        123_000,
        false,
        Some(42),
    )];

    let matches = run(&songs, &locals);

    assert_eq!(matches.len(), 1);
    let m = matches[0];
    assert_eq!(m.local_index, Some(0));
    assert_eq!(m.method, MatchMethod::NeteaseId);
    assert_eq!(m.confidence, 1.0);
    assert_eq!(m.availability, LocalAvailability::Available);
    assert!(m.is_present());
}

#[test]
fn match_marks_encrypted_ncm_availability_for_an_id_match() {
    let songs = [song(7, "Lucky", &["Jason Mraz"], 245_000, None)];
    let locals = [track(
        "C:\\music\\Lucky.ncm",
        "Lucky",
        Some("Jason Mraz"),
        245_000,
        true,
        Some(7),
    )];

    let m = run(&songs, &locals)[0];

    assert_eq!(m.availability, LocalAvailability::EncryptedNcm);
    assert_eq!(m.method, MatchMethod::NeteaseId);
}

#[test]
fn match_uses_normalized_fuzzy_metadata_full_width_brackets_and_punctuation() {
    let songs = [song(1, "Sunny (Live)", &["周杰伦"], 240_000, None)];
    let locals = [plain(
        "C:\\music\\ＳＵＮＮＹ.flac",
        "ＳＵＮＮＹ（现场版）",
        "周杰伦",
        243_000,
    )];

    let m = run(&songs, &locals)[0];

    assert_eq!(m.method, MatchMethod::Metadata);
    assert!(m.confidence >= DEFAULT_THRESHOLD);
    assert_eq!(m.availability, LocalAvailability::Available);
}

#[test]
fn match_strips_punctuation_and_case_for_title_comparison() {
    let songs = [song(1, "Hello, World!", &["Adele"], 240_000, None)];
    let locals = [plain("C:\\music\\x.mp3", "hello world", "adele", 241_000)];

    let m = run(&songs, &locals)[0];

    assert_eq!(m.method, MatchMethod::Metadata);
    assert!(m.confidence > 0.95);
}

#[test]
fn match_splits_artist_names_on_slashes_and_ampersands() {
    let songs = [song(1, "屋顶", &["周杰伦", "温岚"], 305_000, None)];
    let locals = [plain("C:\\music\\x.mp3", "屋顶", "温岚/周杰伦", 305_000)];
    let m = run(&songs, &locals)[0];
    assert_eq!(m.method, MatchMethod::Metadata);
    assert!(m.confidence >= 0.99);

    let songs = [song(
        2,
        "Lucky",
        &["Jason Mraz", "Colbie Caillat"],
        245_000,
        None,
    )];
    let locals = [plain(
        "C:\\music\\y.mp3",
        "Lucky",
        "Jason Mraz & Colbie Caillat",
        245_000,
    )];
    let m = run(&songs, &locals)[0];
    assert_eq!(m.method, MatchMethod::Metadata);
    assert!(m.confidence >= 0.99);
}

#[test]
fn match_rejects_a_different_artist_with_a_large_duration_gap() {
    let songs = [song(1, "同名歌曲", &["歌手甲"], 200_000, None)];
    let locals = [plain("C:\\music\\x.mp3", "同名歌曲", "歌手乙", 215_000)];

    let m = run(&songs, &locals)[0];

    assert!(!m.is_present());
    assert_eq!(m.method, MatchMethod::None);
    assert_eq!(m.availability, LocalAvailability::Missing);
    assert_eq!(m.confidence, 0.0);
}

#[test]
fn match_rejects_completely_different_songs() {
    let songs = [song(1, "晴天", &["周杰伦"], 269_000, None)];
    let locals = [plain("C:\\music\\x.mp3", "Lucky", "Jason Mraz", 245_000)];

    assert!(!run(&songs, &locals)[0].is_present());
}

#[test]
fn match_is_one_to_one_with_greedy_best_duration() {
    let s1 = song(1, "晴天", &["周杰伦"], 269_000, None);
    let close = plain("C:\\music\\close.mp3", "晴天", "周杰伦", 269_000);
    let far = plain("C:\\music\\far.mp3", "晴天", "周杰伦", 260_000);

    let matches = run(std::slice::from_ref(&s1), &[far.clone(), close.clone()]);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].local_index, Some(1), "时长更接近的 close 胜出");

    // 另一首本地曲不会被重复使用，两首歌各得一个。
    let s2 = song(2, "晴天", &["周杰伦"], 269_000, None);
    let matches = run(&[s1, s2], &[far, close]);
    assert_eq!(matches.len(), 2);
    assert!(matches[0].local_index.is_some());
    assert!(matches[1].local_index.is_some());
    assert_ne!(matches[0].local_index, matches[1].local_index);
}

#[test]
fn match_handles_more_songs_than_local_tracks() {
    let songs = [
        song(1, "A", &["X"], 100_000, None),
        song(2, "B", &["Y"], 200_000, None),
        song(3, "C", &["Z"], 300_000, None),
    ];
    let locals = [plain("C:\\music\\b.mp3", "B", "Y", 200_000)];

    let matches = run(&songs, &locals);

    assert_eq!(matches.len(), 3);
    assert!(!matches[0].is_present());
    assert!(matches[1].is_present());
    assert!(!matches[2].is_present());
    assert_eq!(matches[0].availability, LocalAvailability::Missing);
}

#[test]
fn match_preserves_song_order_and_uses_the_alias_when_helpful() {
    let s1 = song(1, "夜曲", &["周杰伦"], 226_000, None);
    let s2 = song(2, "晴天", &["周杰伦"], 269_000, Some("Sunny"));
    let l1 = plain("C:\\music\\1.mp3", "晴天", "周杰伦", 269_000);
    let l2 = plain("C:\\music\\2.mp3", "夜曲", "周杰伦", 226_000);

    let matches = run(&[s1, s2], &[l1, l2]);

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].song_index, 0);
    assert_eq!(matches[1].song_index, 1);
    assert_eq!(matches[0].local_index, Some(1));
    assert_eq!(matches[1].local_index, Some(0));
}

#[test]
fn match_handles_empty_inputs() {
    assert!(run(&[], &[]).is_empty());
    assert!(run(&[], &[plain("C:\\a.mp3", "a", "b", 1)]).is_empty());
    let only_song = run(&[song(1, "a", &["b"], 1000, None)], &[]);
    assert_eq!(only_song.len(), 1);
    assert!(!only_song[0].is_present());
}

#[test]
fn match_tolerates_a_duration_within_twelve_seconds() {
    let songs = [song(1, "晴天", &["周杰伦"], 269_000, None)];
    let locals = [plain("C:\\music\\x.mp3", "晴天", "周杰伦", 280_000)];

    assert!(run(&songs, &locals)[0].is_present());
}

// ---- 匹配（新增） ----

#[test]
fn an_exact_id_beats_a_perfect_fuzzy_score_and_leaves_the_fuzzy_file_free() {
    let s = song(100, "晴天", &["周杰伦"], 269_000, None);
    let perfect_but_untagged = plain("C:\\music\\晴天.mp3", "晴天", "周杰伦", 269_000);
    let tagged_but_different = track(
        "C:\\music\\other.mp3",
        "完全不同的名字",
        Some("别人"),
        10_000,
        false,
        Some(100),
    );
    // 第二首同名歌曲（不同 ID）：ID 匹配后空出来的“完美”文件应该归它。
    let s2 = song(101, "晴天", &["周杰伦"], 269_000, None);

    let matches = run(&[s, s2], &[perfect_but_untagged, tagged_but_different]);

    assert_eq!(matches[0].method, MatchMethod::NeteaseId);
    assert_eq!(matches[0].local_index, Some(1));
    assert_eq!(matches[1].method, MatchMethod::Metadata);
    assert_eq!(matches[1].local_index, Some(0));
    assert!(matches[1].confidence > 0.99);
}

#[test]
fn a_file_taken_by_an_id_match_is_never_reused_by_metadata_matching() {
    let s1 = song(1, "晴天", &["周杰伦"], 269_000, None);
    let s2 = song(2, "晴天", &["周杰伦"], 269_000, None);
    let only = track(
        "C:\\music\\晴天.mp3",
        "晴天",
        Some("周杰伦"),
        269_000,
        false,
        Some(2),
    );

    let matches = run(&[s1, s2], &[only]);

    assert!(!matches[0].is_present(), "唯一的文件已被 ID 匹配占用");
    assert_eq!(matches[1].method, MatchMethod::NeteaseId);
}

#[test]
fn when_two_local_files_share_an_id_only_the_first_is_used_for_the_id_match() {
    let s = song(5, "晴天", &["周杰伦"], 269_000, None);
    let first = track("C:\\a.mp3", "晴天", Some("周杰伦"), 269_000, false, Some(5));
    let second = track("C:\\b.mp3", "晴天", Some("周杰伦"), 269_000, false, Some(5));
    let s_other = song(6, "晴天", &["周杰伦"], 269_000, None);

    let matches = run(&[s, s_other], &[first, second]);

    assert_eq!(matches[0].local_index, Some(0));
    assert_eq!(matches[0].method, MatchMethod::NeteaseId);
    // 第二个文件没有被 ID 匹配占用，可以给另一首同名歌曲。
    assert_eq!(matches[1].local_index, Some(1));
    assert_eq!(matches[1].method, MatchMethod::Metadata);
}

#[test]
fn a_song_id_of_zero_never_matches_by_id() {
    let s = song(0, "歌", &["人"], 1000, None);
    let t = track("C:\\a.mp3", "别的", Some("别人"), 99_000, false, Some(0));

    let matches = run(&[s], &[t]);

    assert_eq!(matches[0].method, MatchMethod::None);
}

#[test]
fn conflicts_are_resolved_in_favour_of_the_higher_score_and_a_loser_falls_back_to_its_next_best() {
    // 两首同名歌曲争同一个本地文件：时长更接近的赢，另一首退而求其次。
    let s_far = song(1, "晴天", &["周杰伦"], 260_000, None);
    let s_near = song(2, "晴天", &["周杰伦"], 269_000, None);
    let best = plain("C:\\best.mp3", "晴天", "周杰伦", 269_000);
    let second = plain("C:\\second.mp3", "晴天", "周杰伦", 255_000);

    let matches = run(&[s_far, s_near], &[best, second]);

    assert_eq!(
        matches[1].local_index,
        Some(0),
        "更接近的歌曲拿到最好的文件"
    );
    assert_eq!(matches[0].local_index, Some(1), "另一首退到次优文件");
    assert!(matches[0].confidence < matches[1].confidence);
}

#[test]
fn equal_scores_go_to_the_earlier_song_and_the_earlier_file() {
    let s1 = song(1, "晴天", &["周杰伦"], 269_000, None);
    let s2 = song(2, "晴天", &["周杰伦"], 269_000, None);
    let a = plain("C:\\a.mp3", "晴天", "周杰伦", 269_000);
    let b = plain("C:\\b.mp3", "晴天", "周杰伦", 269_000);

    let matches = run(&[s1, s2], &[a, b]);

    assert_eq!(matches[0].local_index, Some(0));
    assert_eq!(matches[1].local_index, Some(1));
}

#[test]
fn a_file_name_guessed_track_matches_like_any_metadata_track() {
    let f = Fixture::new();
    f.file("周杰伦 - 晴天.mp3");
    f.file("无关的文件.mp3");
    let tracks = f.scan().tracks;
    let songs = [song(1, "晴天", &["周杰伦"], 269_000, None)];

    let matches = run(&songs, &tracks);

    // 文件名里推断出歌手与歌名，时长未知给中性分：0.55 + 0.30 + 0.075。
    assert_eq!(matches[0].method, MatchMethod::Metadata);
    assert!((matches[0].confidence - 0.925).abs() < 1e-12);
    let local = &tracks[matches[0].local_index.unwrap()];
    assert!(local.title_from_file_name);

    let view = build_match_view(1, "dir", &songs, &tracks, &matches);
    assert_eq!(view.matches[0].method, "fileName", "界面把它标成文件名匹配");
}

#[test]
fn ncm_header_metadata_takes_part_in_matching_and_marks_the_file_as_ncm() {
    let f = Fixture::new();
    let ncm_path = f.file("Lucky.ncm");
    f.ncm.set(
        &ncm_path,
        NcmMetadata {
            title: Some("Lucky".into()),
            artists: Some("Jason Mraz/Colbie Caillat".into()),
            duration_ms: 245_000,
            music_id: None,
            ..NcmMetadata::default()
        },
    );
    let tracks = f.scan().tracks;
    let songs = [song(
        9,
        "Lucky",
        &["Jason Mraz", "Colbie Caillat"],
        245_500,
        None,
    )];

    let m = run(&songs, &tracks)[0];

    assert_eq!(m.method, MatchMethod::Metadata);
    assert_eq!(m.availability, LocalAvailability::EncryptedNcm);
    assert!(m.confidence > 0.98);

    // 带 musicId 时直接按 ID 匹配。
    f.ncm.set(
        &ncm_path,
        NcmMetadata {
            title: Some("乱七八糟".into()),
            music_id: Some(9),
            ..NcmMetadata::default()
        },
    );
    let tracks = f.scan().tracks;
    let m = run(&songs, &tracks)[0];
    assert_eq!(m.method, MatchMethod::NeteaseId);
    assert_eq!(m.availability, LocalAvailability::EncryptedNcm);
}

#[test]
fn unknown_artists_and_durations_are_neutral() {
    // 歌手、时长都未知：0.55 + 0.30 * 0.5 + 0.15 * 0.5 = 0.775，仍然过阈值。
    let songs = [song(1, "晴天", &[], 0, None)];
    let locals = [track("C:\\a.mp3", "晴天", None, 0, false, None)];

    let m = run(&songs, &locals)[0];

    assert!((m.confidence - 0.775).abs() < 1e-12);
}

#[test]
fn a_title_alone_is_not_enough_when_the_artist_clearly_differs() {
    let songs = [song(1, "晴天", &["周杰伦"], 269_000, None)];
    let locals = [plain("C:\\a.mp3", "晴天", "完全不同的歌手", 269_000)];

    // 0.55 + 0.30 * (dice 近似歌手 * 0.6) + 0.15 = 低于 0.72。
    assert!(!run(&songs, &locals)[0].is_present());
}

#[test]
fn duration_similarity_follows_the_linear_tolerance() {
    assert_eq!(duration_similarity(0, 1000), 0.5);
    assert_eq!(duration_similarity(1000, -1), 0.5);
    assert_eq!(duration_similarity(200_000, 200_000), 1.0);
    assert!((duration_similarity(200_000, 206_000) - 0.5).abs() < 1e-12);
    assert_eq!(duration_similarity(200_000, 212_000), 0.0);
    assert_eq!(duration_similarity(212_000, 200_000), 0.0);
}

#[test]
fn a_thousand_songs_against_several_thousand_files_match_the_right_ones() {
    let mut songs = Vec::new();
    let mut locals = Vec::new();
    for index in 0..1000i64 {
        let name = format!("测试歌曲{index:04}号");
        let artist = format!("歌手{}", index % 37);
        let duration = 180_000 + (index as i32 % 90) * 1000;
        songs.push(song(index + 1, &name, &[&artist], duration, None));
        // 本地文件：标题稍有变化（大小写 / 括号），时长有抖动。
        locals.push(plain(
            &format!("C:\\music\\{index}.mp3"),
            &format!("{name}（现场）"),
            &artist,
            i64::from(duration) + 800,
        ));
    }
    // 干扰项。
    for index in 0..2000 {
        locals.push(plain(
            &format!("C:\\noise\\{index}.mp3"),
            &format!("无关内容{index:05}"),
            &format!("路人{}", index % 11),
            100_000 + index * 13,
        ));
    }

    let matches = run(&songs, &locals);

    for (index, m) in matches.iter().enumerate() {
        assert_eq!(m.local_index, Some(index), "第 {index} 首");
        assert_eq!(m.method, MatchMethod::Metadata);
    }
}

// ---- 视图 ----

#[test]
fn duplicated_songs_in_a_playlist_share_the_first_match() {
    let s = song(1, "晴天", &["周杰伦"], 269_000, None);
    let dup = s.clone();
    let other = song(2, "夜曲", &["周杰伦"], 226_000, None);
    let locals = [plain("C:\\a.mp3", "晴天", "周杰伦", 269_000)];
    let songs = [s, other, dup];

    let matches = run(&songs, &locals);
    let view = build_match_view(7, "C:\\music", &songs, &locals, &matches);

    // 一对一约束下第三行本身没有匹配，但界面沿用第一行的结果。
    assert!(!matches[2].is_present());
    assert_eq!(view.matches[0].availability, "available");
    assert_eq!(view.matches[2].availability, "available");
    assert_eq!(view.matches[2].path.as_deref(), Some("C:\\a.mp3"));
    assert_eq!(view.matches[1].availability, "missing");
    assert_eq!(
        view.stats,
        MatchStats {
            total: 3,
            available: 2,
            ncm: 0,
            missing: 1
        }
    );
    assert_eq!(view.playlist_id, 7);
    assert!(view.scanned);
    assert_eq!(view.total_files, 1);
}

#[test]
fn the_view_serializes_to_camel_case_with_stable_names() {
    let s = song(1, "晴天", &["周杰伦"], 269_000, None);
    let locals = [track(
        "C:\\music\\晴天.ncm",
        "晴天",
        Some("周杰伦"),
        269_000,
        true,
        Some(1),
    )];
    let matches = run(std::slice::from_ref(&s), &locals);
    let view = build_match_view(1, "C:\\music", &[s], &locals, &matches);

    let json = serde_json::to_value(&view).unwrap();

    assert_eq!(json["playlistId"], 1);
    assert_eq!(json["ncmFiles"], 1);
    assert_eq!(json["matches"][0]["availability"], "ncm");
    assert_eq!(json["matches"][0]["method"], "neteaseId");
    assert_eq!(json["matches"][0]["fileName"], "晴天.ncm");
    assert_eq!(json["matches"][0]["songId"], 1);
    assert_eq!(json["stats"]["ncm"], 1);

    let unscanned = serde_json::to_value(unscanned_view(1, "C:\\music", 5)).unwrap();
    assert_eq!(unscanned["scanned"], false);
    assert_eq!(unscanned["matches"].as_array().unwrap().len(), 0);
    assert_eq!(unscanned["stats"]["total"], 5);

    let mode: ScanMode = serde_json::from_str("\"ifNeeded\"").unwrap();
    assert_eq!(mode, ScanMode::IfNeeded);
    let mode: ScanMode = serde_json::from_str("\"never\"").unwrap();
    assert_eq!(mode, ScanMode::Never);
}

// ---- 编排器 ----

struct ManagerFixture {
    fixture: Fixture,
    manager: Arc<LibraryManager>,
    logs: Arc<LogHub>,
    progress: Arc<Mutex<Vec<ScanProgressView>>>,
    sink: ScanProgressSink,
}

impl ManagerFixture {
    fn new() -> Self {
        let fixture = Fixture::new();
        let logs = LogHub::new();
        let service = Arc::new(LocalLibraryService::new(
            fixture.ncm.clone(),
            fixture.tags.clone(),
        ));
        let manager = LibraryManager::new(service, Arc::clone(&logs));
        let progress = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&progress);
        let sink: ScanProgressSink = Arc::new(move |p| recorded.lock().unwrap().push(p));
        Self {
            fixture,
            manager,
            logs,
            progress,
            sink,
        }
    }

    fn dir(&self) -> String {
        self.fixture.root().to_string_lossy().into_owned()
    }

    fn messages(&self) -> Vec<String> {
        self.logs.recent().into_iter().map(|e| e.message).collect()
    }
}

#[tokio::test]
async fn match_playlist_scans_when_needed_and_reuses_the_snapshot_afterwards() {
    let m = ManagerFixture::new();
    let path = m.fixture.file("周杰伦 - 晴天.mp3");
    m.fixture
        .tags
        .set(&path, tag_info("晴天", "周杰伦", 269_000));
    m.manager.remember_playlist(
        5,
        &[
            song(1, "晴天", &["周杰伦"], 269_000, None),
            song(2, "夜曲", &["周杰伦"], 226_000, None),
        ],
    );

    let first = m
        .manager
        .match_playlist(5, ScanMode::IfNeeded, &m.dir(), m.sink.clone())
        .await
        .unwrap();
    let reads_after_first = m.fixture.tags.reads.load(Ordering::SeqCst);
    let second = m
        .manager
        .match_playlist(5, ScanMode::IfNeeded, &m.dir(), m.sink.clone())
        .await
        .unwrap();

    assert!(first.scanned);
    assert_eq!(first.stats.available, 1);
    assert_eq!(first.stats.missing, 1);
    assert_eq!(first.matches[0].method, "metadata");
    assert_eq!(first.total_files, 1);
    assert_eq!(second, first);
    assert_eq!(
        m.fixture.tags.reads.load(Ordering::SeqCst),
        reads_after_first,
        "第二次直接复用扫描结果"
    );
    assert!(!m.progress.lock().unwrap().is_empty());
    let messages = m.messages();
    assert!(
        messages
            .iter()
            .any(|t| t == "本地曲库扫描完成：1 个文件（其中 NCM 0 个）")
    );
    assert!(
        messages
            .iter()
            .any(|t| t == "本地对比：已存在 1，NCM 待解密 0，需下载 1")
    );
}

#[tokio::test]
async fn never_mode_does_not_scan_and_reports_the_library_as_not_compared() {
    let m = ManagerFixture::new();
    m.fixture.file("a.mp3");
    m.manager
        .remember_playlist(5, &[song(1, "x", &["y"], 1000, None)]);

    let outcome = m
        .manager
        .match_playlist(5, ScanMode::Never, &m.dir(), m.sink.clone())
        .await
        .unwrap();

    assert!(!outcome.scanned);
    assert!(outcome.matches.is_empty());
    assert_eq!(outcome.stats.total, 1);
    assert_eq!(m.fixture.tags.reads.load(Ordering::SeqCst), 0);
    assert!(m.manager.summary().is_none());

    // 扫描过之后，never 模式也会用缓存对比。
    m.manager.scan(&m.dir(), m.sink.clone()).await.unwrap();
    let outcome = m
        .manager
        .match_playlist(5, ScanMode::Never, &m.dir(), m.sink.clone())
        .await
        .unwrap();
    assert!(outcome.scanned);
    assert_eq!(outcome.matches.len(), 1);
}

#[tokio::test]
async fn changing_the_library_directory_invalidates_the_snapshot() {
    let m = ManagerFixture::new();
    m.fixture.file("one/a.mp3");
    m.fixture.file("two/b.mp3");
    m.fixture.file("two/c.mp3");
    m.manager
        .remember_playlist(5, &[song(1, "x", &["y"], 1000, None)]);
    let one = m.fixture.root().join("one").to_string_lossy().into_owned();
    let two = m.fixture.root().join("two").to_string_lossy().into_owned();

    let first = m.manager.scan(&one, m.sink.clone()).await.unwrap();
    assert_eq!(first.total_files, 1);

    let unscanned = m
        .manager
        .match_playlist(5, ScanMode::Never, &two, m.sink.clone())
        .await
        .unwrap();
    assert!(!unscanned.scanned, "目录变了，旧缓存不能用");

    let rescanned = m
        .manager
        .match_playlist(5, ScanMode::IfNeeded, &format!("{two}\\"), m.sink.clone())
        .await
        .unwrap();
    assert_eq!(rescanned.total_files, 2);
    assert_eq!(m.manager.summary().unwrap().total_files, 2);

    m.manager.invalidate();
    assert!(m.manager.summary().is_none());
}

#[tokio::test]
async fn a_missing_library_directory_gives_a_chinese_error_and_a_log() {
    let m = ManagerFixture::new();
    m.manager
        .remember_playlist(5, &[song(1, "x", &["y"], 1000, None)]);
    let missing = m
        .fixture
        .root()
        .join("不存在")
        .to_string_lossy()
        .into_owned();

    let error = m
        .manager
        .match_playlist(5, ScanMode::IfNeeded, &missing, m.sink.clone())
        .await
        .unwrap_err();

    assert_eq!(error.kind, "directoryMissing");
    assert!(error.message.starts_with("曲库目录不存在："));
    assert!(error.message.contains("设置"));
    let warning = m
        .logs
        .recent()
        .into_iter()
        .find(|e| e.level == LogLevel::Warning)
        .expect("应写一条警告日志");
    assert!(warning.message.contains("曲库目录不存在"));
}

#[tokio::test]
async fn matching_an_unknown_playlist_is_reported() {
    let m = ManagerFixture::new();

    let error = m
        .manager
        .match_playlist(404, ScanMode::IfNeeded, &m.dir(), m.sink.clone())
        .await
        .unwrap_err();

    assert_eq!(error.kind, "playlistNotLoaded");
}

#[tokio::test]
async fn remembered_playlists_are_replaced_on_reload_and_capped() {
    let m = ManagerFixture::new();
    m.fixture.file("a.mp3");
    for id in 1..=8 {
        m.manager
            .remember_playlist(id, &[song(id, "x", &["y"], 1000, None)]);
    }
    // 最早的两个被挤掉。
    for id in [1, 2] {
        let error = m
            .manager
            .match_playlist(id, ScanMode::Never, &m.dir(), m.sink.clone())
            .await
            .unwrap_err();
        assert_eq!(error.kind, "playlistNotLoaded");
    }
    // 重新加载同一个歌单会替换内容。
    m.manager.remember_playlist(
        8,
        &[
            song(81, "a", &["b"], 1000, None),
            song(82, "c", &["d"], 1000, None),
        ],
    );
    let outcome = m
        .manager
        .match_playlist(8, ScanMode::Never, &m.dir(), m.sink.clone())
        .await
        .unwrap();
    assert_eq!(outcome.stats.total, 2);
}

#[tokio::test]
async fn cancel_stops_a_running_scan_and_keeps_the_previous_snapshot() {
    let m = ManagerFixture::new();
    for index in 0..60 {
        m.fixture.file(&format!("song-{index:02}.mp3"));
    }
    m.manager
        .remember_playlist(5, &[song(1, "x", &["y"], 1000, None)]);
    m.manager.scan(&m.dir(), m.sink.clone()).await.unwrap();
    assert_eq!(m.manager.summary().unwrap().total_files, 60);
    assert!(!m.manager.cancel(), "没有任务在运行");

    // 第二次扫描在读第一个文件时被取消。
    let manager = Arc::clone(&m.manager);
    *m.fixture.tags.on_first_read.lock().unwrap() = Some(Box::new(move || {
        manager.cancel();
    }));
    m.fixture.tags.reads.store(0, Ordering::SeqCst);

    let error = m.manager.scan(&m.dir(), m.sink.clone()).await.unwrap_err();

    assert_eq!(error.kind, "cancelled");
    assert!(!m.manager.is_scanning());
    assert_eq!(
        m.manager.summary().unwrap().total_files,
        60,
        "取消不会丢掉上一次的结果"
    );
    assert!(m.messages().iter().any(|t| t == "扫描本地曲库已取消"));
}

#[tokio::test]
async fn concurrent_requests_share_one_scan() {
    let m = ManagerFixture::new();
    for index in 0..30 {
        m.fixture.file(&format!("song-{index:02}.mp3"));
    }
    m.manager
        .remember_playlist(5, &[song(1, "x", &["y"], 1000, None)]);

    let dir = m.dir();
    let (a, b) = tokio::join!(
        m.manager
            .match_playlist(5, ScanMode::IfNeeded, &dir, m.sink.clone()),
        m.manager
            .match_playlist(5, ScanMode::IfNeeded, &dir, m.sink.clone()),
    );

    assert!(a.unwrap().scanned);
    assert!(b.unwrap().scanned);
    assert_eq!(
        m.fixture.tags.reads.load(Ordering::SeqCst),
        30,
        "只扫描了一次"
    );
}

#[tokio::test]
async fn progress_events_are_throttled_but_the_last_one_always_arrives() {
    let m = ManagerFixture::new();
    for index in 0..300 {
        m.fixture.file(&format!("song-{index:03}.mp3"));
    }

    m.manager.scan(&m.dir(), m.sink.clone()).await.unwrap();

    let progress = m.progress.lock().unwrap();
    assert!(!progress.is_empty());
    assert!(progress.len() <= 301);
    let last = progress.last().unwrap();
    assert_eq!((last.scanned, last.total), (300, 300));
    assert!(progress.windows(2).all(|w| w[1].scanned >= w[0].scanned));
}

// ---- 原始对比结果（给下载 / 同步用） ----

#[tokio::test]
async fn raw_matches_expose_the_local_files_and_share_the_first_duplicate() {
    let m = ManagerFixture::new();
    let path = m.fixture.file("周杰伦 - 晴天.mp3");
    m.fixture
        .tags
        .set(&path, tag_info("晴天", "周杰伦", 269_000));
    m.manager.remember_playlist(
        5,
        &[
            song(1, "晴天", &["周杰伦"], 269_000, None),
            song(2, "夜曲", &["周杰伦"], 226_000, None),
            song(1, "晴天", &["周杰伦"], 269_000, None),
        ],
    );

    let matched = m
        .manager
        .match_playlist_raw(5, ScanMode::IfNeeded, &m.dir(), m.sink.clone())
        .await
        .unwrap();

    assert!(matched.scanned);
    assert_eq!(matched.playlist_id, 5);
    assert_eq!(matched.songs.len(), 3);
    assert_eq!(matched.tracks.len(), 1);
    assert_eq!(matched.availability(0), Some(LocalAvailability::Available));
    assert_eq!(matched.availability(1), Some(LocalAvailability::Missing));
    // 重复的行沿用第一次出现那一行的结果。
    assert_eq!(matched.availability(2), Some(LocalAvailability::Available));
    assert_eq!(matched.local_track(0).unwrap().path, path);
    assert_eq!(matched.local_track(2).unwrap().path, path);
    assert!(matched.local_track(1).is_none());
    assert!(matched.local_track(99).is_none());
}

#[tokio::test]
async fn raw_matches_without_a_scan_are_unresolved_rather_than_missing() {
    let m = ManagerFixture::new();
    m.manager
        .remember_playlist(5, &[song(1, "x", &["y"], 1000, None)]);

    let matched = m
        .manager
        .match_playlist_raw(5, ScanMode::Never, &m.dir(), m.sink.clone())
        .await
        .unwrap();

    assert!(!matched.scanned);
    assert_eq!(matched.availability(0), None);
    assert!(matched.local_track(0).is_none());
    assert_eq!(matched.songs.len(), 1);
}

#[tokio::test]
async fn raw_matches_of_an_unknown_playlist_are_reported() {
    let m = ManagerFixture::new();
    let error = m
        .manager
        .match_playlist_raw(404, ScanMode::Never, &m.dir(), m.sink.clone())
        .await
        .unwrap_err();
    assert_eq!(error.kind, "playlistNotLoaded");
    assert!(m.manager.playlist_songs(404).is_none());
}

#[test]
fn remembered_songs_can_be_looked_up_by_playlist() {
    let m = ManagerFixture::new();
    m.manager.remember_playlist(
        7,
        &[
            song(1, "a", &["b"], 1000, None),
            song(2, "c", &["d"], 1000, None),
        ],
    );
    let songs = m.manager.playlist_songs(7).unwrap();
    assert_eq!(songs.iter().map(|s| s.id).collect::<Vec<_>>(), [1, 2]);
}
