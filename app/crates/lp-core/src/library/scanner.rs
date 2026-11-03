//! 本地曲库扫描器（对应 C# `LocalLibraryService`）。
//!
//! 递归遍历目录、按绝对路径去重（不跟随目录链接 / 联接点，防死循环）；对 `.ncm` 读取文件头里的元数据，
//! 对普通音频读取标签（交给 `AudioTagService`），标签缺失时用「歌手 - 歌曲名」的文件名回退。
//! 单个文件或目录失败不会中断整批扫描。
//!
//! 与 C# 的差异：C# 是逐个文件顺序读标签；这里用一组工作线程并行读（标签读取是纯磁盘 / CPU 活，互不依赖），
//! 结果仍按完整路径排序，顺序与线程调度无关。

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, LazyLock, Mutex};

use regex::Regex;

use super::track::{LibraryError, LibraryScanProgress, LocalTrack, ScanOutcome};
use crate::ncm::{NcmDecoder, NcmMetadata};
use crate::tags::{AudioTagInfo, AudioTagService};
use crate::utilities::CancelToken;

/// 扫描支持的扩展名（含点，小写）。与编辑器用的 `audio_file_types` 不同：这里没有 `.aac` / `.wma`，有 `.ncm`。
pub const SUPPORTED_EXTENSIONS: [&str; 8] = [
    ".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".ncm",
];

/// 并行读标签的工作线程上限。
const MAX_WORKERS: usize = 8;

// Windows 文件属性：System 的文件与目录、带重解析点的目录（符号链接 / 联接点）不参与扫描。
#[cfg(windows)]
const ATTRIBUTE_SYSTEM: u32 = 0x4;
#[cfg(windows)]
const ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

// 可选的前导音轨号：「01.」「02 - 」「3)」等。
static TRACK_NUMBER_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\d{1,3}\s*[\.\-_、\)\]]\s*").expect("音轨号正则合法"));

// 「01 Song」这类只有空格分隔的两位音轨号。
static TWO_DIGIT_TRACK_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\d{1,2}\s+").expect("音轨号正则合法"));

static ARTIST_TITLE_SEPARATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+[-–—－]\s+").expect("分隔符正则合法"));

/// 扫描进度回调：会从多个工作线程调用（内部已串行化、已处理数单调递增）。
pub type ProgressCallback<'a> = &'a (dyn Fn(LibraryScanProgress) + Sync);

/// 本地曲库扫描器。
pub struct LocalLibraryService {
    ncm: Arc<dyn NcmDecoder>,
    tags: Arc<dyn AudioTagService>,
}

impl LocalLibraryService {
    /// 创建扫描器。
    pub fn new(ncm: Arc<dyn NcmDecoder>, tags: Arc<dyn AudioTagService>) -> Self {
        Self { ncm, tags }
    }

    /// 递归扫描给定目录，支持 mp3 / flac / m4a / mp4 / ogg / opus / wav 及 ncm。
    ///
    /// 空路径与不存在的目录被忽略，重叠的根目录只扫一次；取消返回 [`LibraryError::Cancelled`]
    /// （部分结果不保留：不完整的曲库拿去匹配只会得到一堆误报的「需下载」）。
    pub fn scan(
        &self,
        directories: &[PathBuf],
        progress: Option<ProgressCallback<'_>>,
        cancel: &CancelToken,
    ) -> Result<ScanOutcome, LibraryError> {
        let roots = resolve_roots(directories);
        let (files, skipped_directories) = collect_files(&roots, cancel)?;

        let total = files.len();
        if let Some(report) = progress {
            report(LibraryScanProgress {
                scanned_files: 0,
                total_files: total,
                current_file: files.first().cloned().unwrap_or_default(),
            });
        }

        let tracks = self.read_all(&files, progress, cancel)?;
        Ok(ScanOutcome {
            tracks,
            skipped_directories,
        })
    }

    fn read_all(
        &self,
        files: &[PathBuf],
        progress: Option<ProgressCallback<'_>>,
        cancel: &CancelToken,
    ) -> Result<Vec<LocalTrack>, LibraryError> {
        let total = files.len();
        if total == 0 {
            return Ok(Vec::new());
        }

        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .clamp(1, MAX_WORKERS)
            .min(total);
        let next = AtomicUsize::new(0);
        // 已处理数与回调放在同一把锁里：回调看到的数字一定单调递增。
        let reported = Mutex::new(0usize);

        let mut slots: Vec<Option<LocalTrack>> = (0..total).map(|_| None).collect();
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    scope.spawn(|| {
                        let mut done: Vec<(usize, LocalTrack)> = Vec::new();
                        loop {
                            if cancel.is_cancelled() {
                                break;
                            }
                            let index = next.fetch_add(1, AtomicOrdering::SeqCst);
                            if index >= total {
                                break;
                            }
                            let file = &files[index];
                            done.push((index, self.read_track(file)));

                            let mut count = reported
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            *count += 1;
                            if let Some(report) = progress {
                                report(LibraryScanProgress {
                                    scanned_files: *count,
                                    total_files: total,
                                    current_file: file.clone(),
                                });
                            }
                        }
                        done
                    })
                })
                .collect();

            for handle in handles {
                // 工作线程 panic（标签库的缺陷）时把 panic 原样抛给调用方，而不是悄悄丢文件。
                match handle.join() {
                    Ok(done) => {
                        for (index, track) in done {
                            slots[index] = Some(track);
                        }
                    }
                    Err(payload) => std::panic::resume_unwind(payload),
                }
            }
        });

        if cancel.is_cancelled() {
            return Err(LibraryError::Cancelled);
        }
        Ok(slots.into_iter().flatten().collect())
    }

    fn read_track(&self, file: &Path) -> LocalTrack {
        match extension_lower(file).as_str() {
            ".ncm" => self.read_ncm_track(file),
            extension => self.read_audio_track(file, extension),
        }
    }

    fn read_ncm_track(&self, file: &Path) -> LocalTrack {
        let extension = extension_lower(file);
        // 文件头损坏、被占用等都当作没有元数据，仍以文件名回退展示。
        let metadata: Option<NcmMetadata> = if self.ncm.is_ncm_file(file) {
            self.ncm.read_info(file).ok().and_then(|info| info.metadata)
        } else {
            None
        };

        let fallback = parse_file_name(file);
        let meta_title = metadata
            .as_ref()
            .and_then(|m| non_blank(m.title.as_deref()));
        let title_from_file_name = meta_title.is_none() && fallback.title.is_some();
        LocalTrack {
            path: file.to_path_buf(),
            title: meta_title
                .or_else(|| fallback.title.clone())
                .unwrap_or_else(|| "未知".to_owned()),
            artist: first_non_empty([
                metadata.as_ref().and_then(|m| m.artists.as_deref()),
                fallback.artist.as_deref(),
            ]),
            album: metadata
                .as_ref()
                .and_then(|m| non_blank(m.album.as_deref())),
            duration_ms: metadata
                .as_ref()
                .map(|m| m.duration_ms)
                .filter(|d| *d > 0)
                .unwrap_or(0),
            format: first_non_empty([
                metadata.as_ref().and_then(|m| m.format.as_deref()),
                Some(extension.trim_start_matches('.')),
            ]),
            bitrate: metadata
                .as_ref()
                .map(|m| m.bitrate)
                .filter(|bitrate| *bitrate > 0),
            is_ncm: true,
            netease_id: metadata
                .as_ref()
                .and_then(|m| m.music_id)
                .filter(|id| *id > 0),
            ncm_metadata: metadata,
            // NCM 封面在加密音频段内，仅读头信息不进行解密，因此扫描阶段未知。
            has_cover: false,
            title_from_file_name,
        }
    }

    fn read_audio_track(&self, file: &Path, extension: &str) -> LocalTrack {
        // 标签损坏、格式不支持、被占用都当作没有标签，仍以文件名回退展示。
        let info: Option<AudioTagInfo> = self.tags.read(file).ok().flatten();

        let fallback = parse_file_name(file);
        let tag_title = info.as_ref().and_then(|i| non_blank(i.title.as_deref()));
        let title_from_file_name = tag_title.is_none() && fallback.title.is_some();
        LocalTrack {
            path: file.to_path_buf(),
            title: tag_title
                .or_else(|| fallback.title.clone())
                .unwrap_or_else(|| "未知".to_owned()),
            artist: first_non_empty([
                info.as_ref().and_then(|i| i.artist.as_deref()),
                fallback.artist.as_deref(),
            ]),
            album: info.as_ref().and_then(|i| non_blank(i.album.as_deref())),
            duration_ms: info
                .as_ref()
                .map(|i| i.duration_ms)
                .filter(|d| *d > 0)
                .unwrap_or(0),
            format: Some(extension.trim_start_matches('.').to_owned()),
            bitrate: info
                .as_ref()
                .map(|i| i.bitrate)
                .filter(|bitrate| *bitrate > 0)
                .map(|bitrate| i32::try_from(bitrate).unwrap_or(i32::MAX)),
            is_ncm: false,
            ncm_metadata: None,
            netease_id: info
                .as_ref()
                .and_then(|i| i.netease_id)
                .filter(|id| *id > 0),
            has_cover: info.as_ref().is_some_and(|i| i.has_cover),
            title_from_file_name,
        }
    }
}

/// 文件名里推断出的标题与歌手。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileNameGuess {
    /// 歌曲名。
    pub title: Option<String>,
    /// 歌手。
    pub artist: Option<String>,
}

/// 从文件名推断「歌手 - 歌曲名」；支持中文 / 全角连字符，并去掉前导音轨号。无法拆分时只返回歌曲名。
pub fn parse_file_name(path: &Path) -> FileNameGuess {
    let stem = path
        .file_name()
        .map(|name| file_stem(&name.to_string_lossy()))
        .unwrap_or_default();
    let stem = stem.trim();
    if stem.is_empty() {
        return FileNameGuess::default();
    }

    let stem = TRACK_NUMBER_PREFIX.replace(stem, "").trim().to_owned();
    let stem = TWO_DIGIT_TRACK_PREFIX.replace(&stem, "").trim().to_owned();
    if stem.is_empty() {
        return FileNameGuess::default();
    }

    let mut parts = ARTIST_TITLE_SEPARATOR.splitn(&stem, 2);
    if let (Some(artist), Some(title)) = (parts.next(), parts.next())
        && !artist.trim().is_empty()
        && !title.trim().is_empty()
    {
        return FileNameGuess {
            title: Some(title.trim().to_owned()),
            artist: Some(artist.trim().to_owned()),
        };
    }

    FileNameGuess {
        title: Some(stem),
        artist: None,
    }
}

/// 与 .NET `Path.GetFileNameWithoutExtension` 一致：去掉最后一个 `.` 起的部分（`.` 在开头时整个都是扩展名）。
fn file_stem(name: &str) -> String {
    match name.rfind('.') {
        Some(index) => name[..index].to_owned(),
        None => name.to_owned(),
    }
}

/// 与 .NET `Path.GetExtension(..).ToLowerInvariant()` 一致：含点的小写扩展名，没有为空串。
fn extension_lower(path: &Path) -> String {
    let Some(name) = path.file_name() else {
        return String::new();
    };
    let name = name.to_string_lossy();
    match name.rfind('.') {
        Some(index) if index + 1 < name.len() => name[index..].to_lowercase(),
        _ => String::new(),
    }
}

fn non_blank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<String> {
    values.into_iter().find_map(non_blank)
}

fn is_supported(path: &Path) -> bool {
    let extension = extension_lower(path);
    SUPPORTED_EXTENSIONS.contains(&extension.as_str())
}

/// 与 .NET `StringComparer.OrdinalIgnoreCase` 一致：先转大写，再按 UTF-16 码元逐个比较。
fn compare_ordinal_ignore_case(a: &str, b: &str) -> Ordering {
    a.to_uppercase()
        .encode_utf16()
        .cmp(b.to_uppercase().encode_utf16())
}

/// `Path.GetFullPath` + `TrimEndingDirectorySeparator` + 小写，用来给目录与文件去重。
fn canonical_key(path: &Path) -> Option<String> {
    let full = std::path::absolute(path).ok()?;
    let text = full.to_string_lossy().into_owned();
    let trimmed = text.trim_end_matches(['\\', '/']);
    // `C:\` 这样的盘符根目录不能被削成 `C:`。
    let key = if trimmed.is_empty() || trimmed.ends_with(':') {
        text.as_str()
    } else {
        trimmed
    };
    Some(key.to_lowercase())
}

/// 把输入目录整理成要扫描的根：去掉空白与不存在的目录，重叠 / 重复的只留一个。
fn resolve_roots(directories: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    for directory in directories {
        if directory.as_os_str().to_string_lossy().trim().is_empty() {
            continue;
        }
        let Ok(full) = std::path::absolute(directory) else {
            continue;
        };
        if !full.is_dir() {
            continue;
        }
        if let Some(key) = canonical_key(&full)
            && seen.insert(key)
        {
            roots.push(full);
        }
    }
    roots
}

#[cfg(windows)]
fn file_attributes(entry: &fs::DirEntry) -> u32 {
    use std::os::windows::fs::MetadataExt;
    entry
        .metadata()
        .map(|metadata| metadata.file_attributes())
        .unwrap_or(0)
}

/// 遍历所有根目录，返回排序后的受支持文件，以及被跳过（无权限 / 枚举失败）的目录数。
fn collect_files(
    roots: &[PathBuf],
    cancel: &CancelToken,
) -> Result<(Vec<PathBuf>, usize), LibraryError> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut seen_files: HashSet<String> = HashSet::new();
    let mut visited_directories: HashSet<String> = HashSet::new();
    let mut skipped = 0usize;

    for root in roots {
        let mut pending: Vec<PathBuf> = vec![root.clone()];
        while let Some(directory) = pending.pop() {
            if cancel.is_cancelled() {
                return Err(LibraryError::Cancelled);
            }

            let Some(key) = canonical_key(&directory) else {
                continue;
            };
            if !visited_directories.insert(key) {
                continue;
            }

            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(_) => {
                    // 权限 / IO 错误：跳过该目录，继续其它目录。
                    skipped += 1;
                    continue;
                }
            };

            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };

                if file_type.is_dir() {
                    #[cfg(windows)]
                    if file_attributes(&entry) & (ATTRIBUTE_SYSTEM | ATTRIBUTE_REPARSE_POINT) != 0 {
                        continue;
                    }
                    pending.push(path);
                    continue;
                }

                if !is_supported(&path) {
                    continue;
                }
                // 链接要确认指向的是文件（目录链接不跟随）。
                let is_file = if file_type.is_symlink() {
                    fs::metadata(&path).is_ok_and(|metadata| metadata.is_file())
                } else {
                    file_type.is_file()
                };
                if !is_file {
                    continue;
                }
                #[cfg(windows)]
                if file_attributes(&entry) & ATTRIBUTE_SYSTEM != 0 {
                    continue;
                }

                if let Some(key) = canonical_key(&path)
                    && seen_files.insert(key)
                {
                    files.push(std::path::absolute(&path).unwrap_or(path));
                }
            }
        }
    }

    files.sort_by(|a, b| compare_ordinal_ignore_case(&a.to_string_lossy(), &b.to_string_lossy()));
    Ok((files, skipped))
}
