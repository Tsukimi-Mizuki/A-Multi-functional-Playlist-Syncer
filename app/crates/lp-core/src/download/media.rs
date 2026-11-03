//! 下载 / 同步共用的媒体文件工具：扩展名推断、NCM 魔数识别、唯一文件名、标签原子写入与临时文件清理。
//!
//! 对应 C# `MediaFileHelper`（其中 NCM 导入专用的 `CreateNcmTagData` 在 [`super::ncm_import`]，
//! HTTP 下载在 [`super::fetch`]）。

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::netease::NeteaseSong;
use crate::tags::{AudioTagData, AudioTagService, detect_image_mime};
use crate::utilities::file_name_sanitizer::{self, NameFields};

/// 写进备注里的程序标记（与 NCM 导入一致）。
pub const COMMENT_MARK: &str = "LunaticPrincess";

/// 判断“目录里已经有这首歌”时认可的音频扩展名（含点，小写；不含 `.ncm`）。
pub const AUDIO_EXTENSIONS: [&str; 7] = [".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav"];

/// 能从播放接口的 `type` 或 URL 里推断出来的容器扩展名。
const KNOWN_CONTAINER_EXTENSIONS: [&str; 8] = [
    ".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".aac",
];

/// `.ncm` 文件头魔数 `CTENFDAM`。
const NCM_MAGIC: &[u8; 8] = b"CTENFDAM";

/// 同一进程内生成临时文件名的序号（C# 用 GUID）。
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 生成一个唯一的临时文件名片段：`<进程号>-<序号>`。
pub fn unique_token() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// 是否是 NCM 文件头（`CTENFDAM`）。
pub fn is_ncm_magic(header: &[u8]) -> bool {
    header.len() >= NCM_MAGIC.len() && &header[..NCM_MAGIC.len()] == NCM_MAGIC
}

/// 读取文件前 8 字节判断是否为 NCM；文件过短 / 不可读返回 `false`。
pub fn has_ncm_magic(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut buffer = [0u8; 8];
    let mut read = 0;
    while read < buffer.len() {
        match file.read(&mut buffer[read..]) {
            Ok(0) | Err(_) => break,
            Ok(count) => read += count,
        }
    }
    is_ncm_magic(&buffer[..read])
}

/// 由接口返回的 `type` 与下载 URL 推断最终扩展名（含点）；两者都不可用时回退 `.mp3`。
///
/// 注意：请求无损但服务端可能返回 mp3，必须使用实际返回的格式。
pub fn resolve_extension(format: Option<&str>, url: Option<&str>) -> String {
    normalize_extension(format)
        .or_else(|| normalize_extension(url_extension(url).as_deref()))
        .unwrap_or_else(|| ".mp3".to_owned())
}

/// 取文件路径 / URL 的扩展名（含点、小写）；未知返回 `None`。
pub fn resolve_container_extension(path_or_url: Option<&str>) -> Option<String> {
    let text = path_or_url.map(str::trim).filter(|text| !text.is_empty())?;
    normalize_extension(path_extension(text))
}

/// 与 .NET `Path.GetExtension` 一致：文件名里最后一个 `.` 起的部分，`.` 在末尾则为空。
fn path_extension(path: &str) -> Option<&str> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(index) if index + 1 < name.len() => Some(&name[index..]),
        _ => None,
    }
}

/// URL 路径部分（去掉查询串与片段）的扩展名。
fn url_extension(url: Option<&str>) -> Option<String> {
    let url = url.map(str::trim).filter(|url| !url.is_empty())?;
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    // 绝对 URL：只看主机之后的路径，免得把 `cdn.example` 的 `.example` 当成扩展名。
    let path = match without_query.split_once("://") {
        Some((_, rest)) => rest.find('/').map_or("", |index| &rest[index..]),
        None => without_query,
    };
    path_extension(path).map(str::to_owned)
}

fn normalize_extension(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim().trim_matches('.').to_lowercase();
    let length = trimmed.chars().count();
    if !(2..=5).contains(&length) || !trimmed.chars().all(char::is_alphanumeric) {
        return None;
    }
    let extension = format!(".{trimmed}");
    KNOWN_CONTAINER_EXTENSIONS
        .contains(&extension.as_str())
        .then_some(extension)
}

/// 从发行日期（`yyyy-MM-dd` 等）解析年份；不合法返回 0。
pub fn parse_year(release_date: Option<&str>) -> u32 {
    let Some(text) = release_date else {
        return 0;
    };
    let mut chars = text.chars();
    let year_text: String = chars.by_ref().take(4).collect();
    if year_text.chars().count() < 4 || !year_text.chars().all(|c| c.is_ascii_digit()) {
        return 0;
    }
    match year_text.parse::<u32>() {
        Ok(year) if year > 1900 && year < 2200 => year,
        _ => 0,
    }
}

/// 按文件名模板生成不含扩展名的安全文件名（C# `BuildBaseName`）。
///
/// 先带 `.mp3` 生成再去掉，保证与 [`file_name_sanitizer::build_file_name`] 的清理 / 截断规则一致。
pub fn build_base_name(
    template: &str,
    title: Option<&str>,
    artist: Option<&str>,
    album: Option<&str>,
    track_number: i32,
    year: u32,
    song_id: i64,
) -> String {
    let file_name = file_name_sanitizer::build_file_name(
        template,
        NameFields {
            title,
            artist,
            album,
            track_number: u32::try_from(track_number).unwrap_or(0),
            year,
            song_id,
        },
        "mp3",
    );
    let base = file_name.strip_suffix(".mp3").unwrap_or(&file_name);
    if base.trim().is_empty() {
        "未知歌曲".to_owned()
    } else {
        base.to_owned()
    }
}

/// 在目录（只看顶层）里找同基名、且是音频扩展名的已有文件，用于“跳过已存在”的判断。
pub fn find_existing_audio_file(directory: &Path, base_name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(directory).ok()?;
    let wanted = base_name.to_lowercase();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let Some(extension) = path_extension(&name) else {
            continue;
        };
        if !AUDIO_EXTENSIONS
            .iter()
            .any(|known| known.eq_ignore_ascii_case(extension))
        {
            continue;
        }
        let stem = &name[..name.len() - extension.len()];
        if stem.to_lowercase() == wanted && path.is_file() {
            return Some(path);
        }
    }
    None
}

/// 成品文件放哪里、叫什么。
#[derive(Debug, Clone, Copy)]
pub struct OutputPlan<'a> {
    /// 目标目录。
    pub directory: &'a Path,
    /// 不含扩展名的文件名主干。
    pub base_name: &'a str,
    /// 扩展名（含点）。
    pub extension: &'a str,
    /// 文件名冲突时用来加后缀的歌曲 ID。
    pub song_id: Option<i64>,
    /// 是否直接覆盖同名文件。
    pub overwrite: bool,
}

/// 把已下载 / 已解密的音频原子写入目标目录：先移成临时文件写标签，再移到最终文件名。
///
/// 标签写入失败时保留未打标签的音频（返回的 `bool` 为 `false`）。`overwrite` 为真直接覆盖同名文件，
/// 否则文件名冲突时用 `(歌曲ID)`、`(2)` 等后缀取一个不冲突的名字。
pub fn write_tags_atomically(
    tag_service: &dyn AudioTagService,
    source: &Path,
    plan: &OutputPlan<'_>,
    tags: &AudioTagData,
) -> std::io::Result<(bool, PathBuf)> {
    let OutputPlan {
        directory: target_directory,
        base_name,
        extension,
        song_id,
        overwrite,
    } = *plan;
    let temp_path = target_directory.join(format!(
        ".{}.{}.tmp{extension}",
        safe_temp_stem(base_name),
        unique_token()
    ));
    fs::rename(source, &temp_path)?;
    let guard = TempFile(temp_path.clone());

    // 标签写入失败不应丢弃音频文件；交给调用方在进度里提示。
    let tagged = tag_service.write(&temp_path, tags).is_ok();

    let output = if overwrite {
        let output = target_directory.join(format!("{base_name}{extension}"));
        fs::rename(&temp_path, &output)?;
        output
    } else {
        move_to_unique_path(&temp_path, target_directory, base_name, extension, song_id)?
    };
    drop(guard);
    Ok((tagged, output))
}

/// 移动到目录内唯一的文件名；冲突时使用 `(歌曲ID)` 与 `(2)` / `(3)`… 后缀。
///
/// 先用 `create_new` 占位再改名覆盖占位文件：占位是原子的，所以并发下载同名歌曲时不会互相覆盖
/// （`fs::rename` 在 Windows 上会覆盖已有文件，不能直接拿来检测冲突）。
pub fn move_to_unique_path(
    source: &Path,
    target_directory: &Path,
    base_name: &str,
    extension: &str,
    song_id: Option<i64>,
) -> std::io::Result<PathBuf> {
    for attempt in 0..512 {
        let candidate = candidate_path(target_directory, base_name, extension, song_id, attempt);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(placeholder) => {
                drop(placeholder);
                return match fs::rename(source, &candidate) {
                    Ok(()) => Ok(candidate),
                    Err(error) => {
                        let _ = fs::remove_file(&candidate);
                        Err(error)
                    }
                };
            }
            // 并发下载 / 目录里已有同名文件，尝试下一个候选名。
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::other(format!(
        "无法为「{base_name}」生成唯一的文件名。"
    )))
}

fn candidate_path(
    directory: &Path,
    base_name: &str,
    extension: &str,
    song_id: Option<i64>,
    attempt: usize,
) -> PathBuf {
    if attempt == 0 {
        return directory.join(format!("{base_name}{extension}"));
    }
    match song_id {
        Some(id) => {
            let prefixed = format!("{base_name} ({id})");
            let name = if attempt == 1 {
                prefixed
            } else {
                format!("{prefixed} ({attempt})")
            };
            directory.join(format!("{name}{extension}"))
        }
        None => directory.join(format!("{base_name} ({}){extension}", attempt + 1)),
    }
}

/// 临时文件名里用的主干：清理后截到 80 个码元，空白则用 `track`。
fn safe_temp_stem(base_name: &str) -> String {
    let stem = file_name_sanitizer::sanitize(Some(base_name), 80);
    if stem.trim().is_empty() {
        "track".to_owned()
    } else {
        stem
    }
}

/// 构造下载 / 同步写标签用的元数据（C# `CreateTagData`）。
pub fn create_tag_data(
    song: &NeteaseSong,
    cover: Option<&[u8]>,
    plain_lyrics: Option<&str>,
    word_lyrics: Option<&str>,
) -> AudioTagData {
    let artists: Vec<String> = song
        .artists
        .iter()
        .map(|artist| artist.name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    let cover = cover.filter(|cover| !cover.is_empty());
    let year = parse_year(song.release_date.as_deref());
    AudioTagData {
        title: null_if_empty(Some(&song.name)),
        album_artist: artists.first().cloned(),
        artists,
        album: null_if_empty(Some(&song.album.name)),
        track_number: u32::try_from(song.track_number).ok().filter(|n| *n > 0),
        disc_number: u32::try_from(song.disc_number).ok().filter(|n| *n > 0),
        year: (year > 0).then_some(year),
        cover_data: cover.map(<[u8]>::to_vec),
        cover_mime_type: cover.map(|cover| detect_image_mime(cover).to_owned()),
        plain_lyrics: null_if_empty(plain_lyrics),
        word_lyrics: null_if_empty(word_lyrics),
        netease_id: (song.id > 0).then_some(song.id),
        comment: Some(COMMENT_MARK.to_owned()),
        ..AudioTagData::default()
    }
}

/// 去掉首尾空白；空白串视为没有。
pub fn null_if_empty(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// 第一个非空白的值（去掉首尾空白）。
pub fn first_non_empty(values: &[Option<&str>]) -> Option<String> {
    values.iter().find_map(|value| null_if_empty(*value))
}

/// 离开作用域时删除（若存在）的临时文件：覆盖正常、出错与 future 被取消丢弃三种退出路径。
#[derive(Debug, Default)]
pub struct TempFile(pub PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        try_delete_file(&self.0);
    }
}

/// 一组需要清理的临时文件 / 目录，可以随时增减。
#[derive(Debug, Default)]
pub struct TempFiles {
    files: Vec<PathBuf>,
    directories: Vec<PathBuf>,
}

impl TempFiles {
    /// 登记一个临时文件。
    pub fn add_file(&mut self, path: PathBuf) {
        self.files.push(path);
    }

    /// 登记一个临时目录（连同里面的内容一起删）。
    pub fn add_directory(&mut self, path: PathBuf) {
        self.directories.push(path);
    }

    /// 不再需要清理这个文件（已经被改名成成品）。
    pub fn forget_file(&mut self, path: &Path) {
        self.files.retain(|file| file != path);
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        for file in &self.files {
            try_delete_file(file);
        }
        for directory in &self.directories {
            let _ = fs::remove_dir_all(directory);
        }
    }
}

/// 删除文件；失败（不存在、被占用）静默忽略，清理临时文件失败不影响主流程。
pub fn try_delete_file(path: &Path) {
    if path.as_os_str().is_empty() {
        return;
    }
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netease::{AlbumRef, ArtistRef};
    use crate::test_util::TempDir;

    #[test]
    fn extension_prefers_the_reported_format_over_the_url() {
        assert_eq!(
            resolve_extension(Some("flac"), Some("https://x/a.mp3")),
            ".flac"
        );
        assert_eq!(resolve_extension(Some(".MP3 "), None), ".mp3");
        assert_eq!(
            resolve_extension(None, Some("https://cdn.example/file.flac?token=abc")),
            ".flac"
        );
        assert_eq!(
            resolve_extension(Some("exe"), Some("https://cdn.example/x.m4a#frag")),
            ".m4a"
        );
        // 两者都不可用时回退 mp3；主机名里的点不是扩展名。
        assert_eq!(resolve_extension(None, Some("https://cdn.example")), ".mp3");
        assert_eq!(
            resolve_extension(None, Some("https://cdn.example/")),
            ".mp3"
        );
        assert_eq!(resolve_extension(Some(""), Some("")), ".mp3");
        assert_eq!(
            resolve_extension(Some("a b"), Some("https://x/y.tar")),
            ".mp3"
        );
    }

    #[test]
    fn container_extension_comes_from_the_path() {
        assert_eq!(
            resolve_container_extension(Some(r"D:\music\歌.FLAC")).as_deref(),
            Some(".flac")
        );
        assert_eq!(resolve_container_extension(Some("a.ncm")), None);
        assert_eq!(resolve_container_extension(Some("noext")), None);
        assert_eq!(resolve_container_extension(Some("  ")), None);
        assert_eq!(resolve_container_extension(None), None);
    }

    #[test]
    fn year_is_taken_from_the_first_four_digits() {
        assert_eq!(parse_year(Some("2003-07-31")), 2003);
        assert_eq!(parse_year(Some("1999")), 1999);
        assert_eq!(parse_year(Some("1900-01-01")), 0);
        assert_eq!(parse_year(Some("2200-01-01")), 0);
        assert_eq!(parse_year(Some("20x3-01-01")), 0);
        assert_eq!(parse_year(Some("203")), 0);
        assert_eq!(parse_year(None), 0);
    }

    #[test]
    fn base_name_follows_the_template_and_never_comes_back_empty() {
        let name = build_base_name(
            "{artist} - {title}",
            Some("晴天"),
            Some("周杰伦"),
            None,
            0,
            0,
            1,
        );
        assert_eq!(name, "周杰伦 - 晴天");
        assert_eq!(
            build_base_name(
                "{track} {title} ({year}) [{id}]",
                Some("a"),
                None,
                None,
                3,
                2003,
                7
            ),
            "03 a (2003) [7]"
        );
        // 文件名里的非法字符被替换。
        assert_eq!(
            build_base_name("{title}", Some("A/B:C"), None, None, 0, 0, 1),
            "A_B_C"
        );
        // 模板为空白时回落到默认模板。
        assert_eq!(
            build_base_name("  ", Some("t"), Some("a"), None, 0, 0, 1),
            "a - t"
        );
    }

    #[test]
    fn ncm_magic_is_recognised_from_the_first_eight_bytes() {
        let dir = TempDir::new();
        let ncm = dir.path().join("a.bin");
        fs::write(&ncm, b"CTENFDAM\x01\x02").unwrap();
        let short = dir.path().join("b.bin");
        fs::write(&short, b"CTEN").unwrap();
        let other = dir.path().join("c.bin");
        fs::write(&other, b"ID3\x04\x00\x00").unwrap();

        assert!(has_ncm_magic(&ncm));
        assert!(!has_ncm_magic(&short));
        assert!(!has_ncm_magic(&other));
        assert!(!has_ncm_magic(&dir.path().join("missing.bin")));
        assert!(is_ncm_magic(b"CTENFDAMxyz"));
        assert!(!is_ncm_magic(b"CTENFDA"));
    }

    #[test]
    fn existing_files_are_found_by_base_name_ignoring_case_and_non_audio() {
        let dir = TempDir::new();
        fs::write(dir.path().join("周杰伦 - 晴天.MP3"), [1]).unwrap();
        fs::write(dir.path().join("别的.flac"), [1]).unwrap();
        fs::write(dir.path().join("周杰伦 - 夜曲.lrc"), [1]).unwrap();
        fs::write(dir.path().join(".周杰伦 - 夜曲.x.tmp.mp3"), [1]).unwrap();

        let found = find_existing_audio_file(dir.path(), "周杰伦 - 晴天").unwrap();
        assert_eq!(found.file_name().unwrap(), "周杰伦 - 晴天.MP3");
        assert!(find_existing_audio_file(dir.path(), "周杰伦 - 晴天").is_some());
        assert!(find_existing_audio_file(dir.path(), "周杰伦 - 夜曲").is_none());
        assert!(find_existing_audio_file(dir.path(), "别的").is_some());
        assert!(find_existing_audio_file(&dir.path().join("nope"), "x").is_none());
    }

    #[test]
    fn unique_paths_add_the_song_id_then_a_counter() {
        let dir = TempDir::new();
        let make = |content: u8| {
            let source = dir.path().join(format!("src-{content}.part"));
            fs::write(&source, [content]).unwrap();
            source
        };

        let first = move_to_unique_path(&make(1), dir.path(), "歌", ".mp3", Some(11)).unwrap();
        let second = move_to_unique_path(&make(2), dir.path(), "歌", ".mp3", Some(11)).unwrap();
        let third = move_to_unique_path(&make(3), dir.path(), "歌", ".mp3", Some(11)).unwrap();
        let no_id = move_to_unique_path(&make(4), dir.path(), "歌", ".mp3", None).unwrap();

        assert_eq!(first.file_name().unwrap(), "歌.mp3");
        assert_eq!(second.file_name().unwrap(), "歌 (11).mp3");
        assert_eq!(third.file_name().unwrap(), "歌 (11) (2).mp3");
        assert_eq!(no_id.file_name().unwrap(), "歌 (2).mp3");
        assert_eq!(fs::read(&first).unwrap(), [1]);
        assert_eq!(fs::read(&second).unwrap(), [2]);
        assert_eq!(fs::read(&third).unwrap(), [3]);
    }

    #[test]
    fn unique_move_never_clobbers_an_existing_file() {
        let dir = TempDir::new();
        let existing = dir.path().join("歌.mp3");
        fs::write(&existing, b"original").unwrap();
        let source = dir.path().join("new.part");
        fs::write(&source, b"new").unwrap();

        let moved = move_to_unique_path(&source, dir.path(), "歌", ".mp3", Some(5)).unwrap();

        assert_eq!(fs::read(&existing).unwrap(), b"original");
        assert_eq!(moved.file_name().unwrap(), "歌 (5).mp3");
        assert!(!source.exists());
    }

    #[test]
    fn tag_data_trims_and_maps_every_field() {
        let song = NeteaseSong {
            id: 42,
            name: "  晴天 ".to_owned(),
            artists: vec![
                ArtistRef {
                    id: 1,
                    name: " 周杰伦 ".to_owned(),
                },
                ArtistRef {
                    id: 2,
                    name: "  ".to_owned(),
                },
            ],
            album: AlbumRef {
                id: 2,
                name: "叶惠美".to_owned(),
                cover_url: None,
            },
            track_number: 3,
            disc_number: 1,
            release_date: Some("2003-07-31".to_owned()),
            ..NeteaseSong::default()
        };
        let cover = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 1];

        let tags = create_tag_data(&song, Some(&cover), Some("[00:01.00]a"), Some(" "));

        assert_eq!(tags.title.as_deref(), Some("晴天"));
        assert_eq!(tags.artists, ["周杰伦"]);
        assert_eq!(tags.album_artist.as_deref(), Some("周杰伦"));
        assert_eq!(tags.album.as_deref(), Some("叶惠美"));
        assert_eq!(tags.track_number, Some(3));
        assert_eq!(tags.disc_number, Some(1));
        assert_eq!(tags.year, Some(2003));
        assert_eq!(tags.cover_data.as_deref(), Some(cover.as_slice()));
        assert_eq!(tags.cover_mime_type.as_deref(), Some("image/png"));
        assert_eq!(tags.plain_lyrics.as_deref(), Some("[00:01.00]a"));
        assert_eq!(tags.word_lyrics, None);
        assert_eq!(tags.netease_id, Some(42));
        assert_eq!(tags.comment.as_deref(), Some("LunaticPrincess"));
        assert!(!tags.replace_all);
    }

    #[test]
    fn tag_data_leaves_unknown_fields_empty() {
        let song = NeteaseSong {
            id: 0,
            disc_number: 0,
            ..NeteaseSong::default()
        };
        let tags = create_tag_data(&song, Some(&[]), None, None);
        assert_eq!(tags.title, None);
        assert!(tags.artists.is_empty());
        assert_eq!(tags.album_artist, None);
        assert_eq!(tags.track_number, None);
        assert_eq!(tags.disc_number, None);
        assert_eq!(tags.year, None);
        assert_eq!(tags.cover_data, None);
        assert_eq!(tags.netease_id, None);
    }

    #[test]
    fn temp_files_are_removed_when_dropped_unless_forgotten() {
        let dir = TempDir::new();
        let keep = dir.path().join("keep.tmp");
        let drop_me = dir.path().join("drop.tmp");
        let nested = dir.path().join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("x"), [1]).unwrap();
        fs::write(&keep, [1]).unwrap();
        fs::write(&drop_me, [1]).unwrap();
        {
            let mut temps = TempFiles::default();
            temps.add_file(keep.clone());
            temps.add_file(drop_me.clone());
            temps.add_directory(nested.clone());
            temps.forget_file(&keep);
        }
        assert!(keep.exists());
        assert!(!drop_me.exists());
        assert!(!nested.exists());
    }

    #[test]
    fn first_non_empty_skips_blank_values() {
        assert_eq!(
            first_non_empty(&[None, Some("  "), Some(" 值 "), Some("x")]).as_deref(),
            Some("值")
        );
        assert_eq!(first_non_empty(&[None, Some("")]), None);
    }
}
