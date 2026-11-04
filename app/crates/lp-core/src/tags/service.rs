//! 基于 `lofty` 的音频标签读写（对应 C# `AudioTagService`，TagLibSharp 2.3.0）。
//!
//! 所有容器统一走 lofty 的通用 [`Tag`] + [`ItemKey`] 抽象：MP3 / AAC / AIFF / WAV → ID3v2，
//! FLAC / OGG / OPUS / Speex → VorbisComments，M4A / MP4 → iTunes `ilst`，APE / MPC / WavPack → APE。
//! 通用 `Tag` 带着“伴随标签”（companion tag），所以编辑器里没展示的字段（TXXX、PRIV、ReplayGain、自定义
//! Vorbis 字段、`ilst` 里的其他原子等）在写回时原样保留。
//!
//! ## 写入的安全性
//!
//! 先把原文件复制成同目录临时副本，所有改动写进副本，再重读副本确认还能解析，最后原子替换原文件
//! （Windows 上是 `MoveFileEx(REPLACE_EXISTING)`）；任何一步失败都清理副本、保留原文件。
//! C# 版是直接就地 `file.Save()`，写到一半失败会损坏文件。

use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use lofty::ape::ApeTag;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, FileType, TaggedFile, TaggedFileExt};
use lofty::id3::v2::Id3v2Tag;
use lofty::iff::wav::RiffInfoList;
use lofty::mp4::Ilst;
use lofty::ogg::tag::VorbisComments;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, ItemValue, Tag, TagExt, TagItem, TagType};

use super::model::{AudioTagData, AudioTagInfo, TagError};
use super::text::{
    NETEASE_COMMENT_PREFIX, clean_comment, clean_display_comment, clean_values, detect_image_mime,
    join_values, null_if_blank, parse_netease_id, split_values, strip_netease_marker,
};

/// 封面图片的描述文字（与 C# 一致）。
const COVER_DESCRIPTION: &str = "Cover";
/// 备注 / 歌词帧的语言码：ID3 规范里 `XXX` 是“未知”，但不少播放器只认 `eng`，所以和多数标签编辑器一样写 `eng`。
const FRAME_LANGUAGE: [u8; 3] = *b"eng";

/// 标签读写服务的抽象，方便下载 / 同步模块在测试里替换成假实现（对应 C# `IAudioTagService`）。
///
/// 方法都是同步的：lofty 本身是同步 IO，调用方在异步上下文里用 `spawn_blocking` 包一层。
pub trait AudioTagService: Send + Sync {
    /// 读取标签与文件信息。
    ///
    /// 文件不存在、格式无法识别或已损坏时返回 `Ok(None)`（与 C# 一致）；权限不足、被占用等
    /// 真正的 IO 故障返回 `Err`，让界面能给出准确原因。
    fn read(&self, audio_path: &Path) -> Result<Option<AudioTagInfo>, TagError>;

    /// 写入标签。按文件容器自动选择 ID3v2 / VorbisComments / MP4 ilst；
    /// `word_lyrics` 应优先写入带逐字时间轴的增强 LRC，`plain_lyrics` 写普通 LRC。
    fn write(&self, audio_path: &Path, tags: &AudioTagData) -> Result<(), TagError>;
}

/// 生产实现：用 lofty 读写。
#[derive(Debug, Default, Clone, Copy)]
pub struct LoftyTagService;

impl AudioTagService for LoftyTagService {
    fn read(&self, audio_path: &Path) -> Result<Option<AudioTagInfo>, TagError> {
        ensure_path(audio_path)?;
        read_info(audio_path)
    }

    fn write(&self, audio_path: &Path, tags: &AudioTagData) -> Result<(), TagError> {
        ensure_path(audio_path)?;
        write_tags(audio_path, tags)
    }
}

fn ensure_path(path: &Path) -> Result<(), TagError> {
    if path.as_os_str().to_string_lossy().trim().is_empty() {
        return Err(TagError::InvalidArgument("音频文件路径不能为空。"));
    }
    Ok(())
}

// ───────────────────────────── 读取 ─────────────────────────────

/// 打开并解析音频文件。`Ok(None)` 表示格式无法识别 / 文件损坏；IO 故障返回 `Err`。
fn open_tagged(path: &Path) -> Result<Option<TaggedFile>, TagError> {
    let file = File::open(path)?;
    let mut probe = Probe::new(BufReader::new(file));
    if let Some(by_extension) = FileType::from_path(path) {
        probe = probe.set_file_type(by_extension);
    }
    let probe = match probe.options(ParseOptions::new()).guess_file_type() {
        Ok(probe) => probe,
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    match probe.read() {
        Ok(tagged) => Ok(Some(tagged)),
        Err(error) => match io_error_in(&error) {
            Some(io) if is_environmental(io) => Err(TagError::Io(copy_io_error(io).into())),
            _ => Ok(None),
        },
    }
}

/// 在错误链里找底层的 `std::io::Error`。
fn io_error_in<'a>(error: &'a (dyn std::error::Error + 'static)) -> Option<&'a std::io::Error> {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(err) = current {
        if let Some(io) = err.downcast_ref::<std::io::Error>() {
            return Some(io);
        }
        current = err.source();
    }
    None
}

/// 复制一份 IO 错误（`io::Error` 不能 clone）：有系统错误码就保留，中文提示要靠它识别“文件被占用”等情况。
fn copy_io_error(error: &std::io::Error) -> std::io::Error {
    match error.raw_os_error() {
        Some(code) => std::io::Error::from_raw_os_error(code),
        None => std::io::Error::from(error.kind()),
    }
}

/// 区分“文件本身有问题”（截断、格式错误 → 当作损坏）和“环境问题”（权限、占用、磁盘 → 要告诉用户）。
fn is_environmental(error: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    matches!(
        error.kind(),
        ErrorKind::PermissionDenied | ErrorKind::StorageFull | ErrorKind::ReadOnlyFilesystem
    ) || matches!(error.raw_os_error(), Some(32 | 33 | 112 | 206))
}

fn read_info(path: &Path) -> Result<Option<AudioTagInfo>, TagError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let Some(tagged) = open_tagged(path)? else {
        return Ok(None);
    };
    Ok(Some(build_info(&tagged, path, metadata.len())))
}

/// 按“主标签优先、其余标签兜底”的顺序排列（等价于 TagLib 的 `CombinedTag`：每个字段取第一个有值的标签）。
fn ordered_tags(tagged: &TaggedFile) -> Vec<&Tag> {
    let primary = tagged.primary_tag_type();
    let mut tags: Vec<&Tag> = tagged.tags().iter().collect();
    tags.sort_by_key(|tag| tag.tag_type() != primary);
    tags
}

fn build_info(tagged: &TaggedFile, path: &Path, file_size: u64) -> AudioTagInfo {
    let tags = ordered_tags(tagged);
    let properties = tagged.properties();

    let texts = |key: ItemKey| first_with_values(&tags, |tag| multi_texts_of(tag, key));
    let comment_raw = comment_text(&tags);
    let cover = find_cover(&tags);

    AudioTagInfo {
        title: null_if_blank(texts(ItemKey::TrackTitle).first().map(String::as_str)),
        artist: join_values(texts(ItemKey::TrackArtist).iter().map(String::as_str)),
        album: null_if_blank(texts(ItemKey::AlbumTitle).first().map(String::as_str)),
        album_artist: join_values(texts(ItemKey::AlbumArtist).iter().map(String::as_str)),
        duration_ms: i64::try_from(properties.duration().as_millis()).unwrap_or(i64::MAX),
        bitrate: properties.audio_bitrate().unwrap_or(0),
        lyrics: null_if_blank(lyrics_text(&tags).as_deref()),
        has_cover: tags.iter().any(|tag| !tag.pictures().is_empty()),
        netease_id: netease_id_of(&tags),
        year: tags.iter().find_map(|tag| year_of(tag)),
        track_number: tags.iter().find_map(|tag| positive(tag.track())),
        track_total: tags.iter().find_map(|tag| positive(tag.track_total())),
        disc_number: tags.iter().find_map(|tag| positive(tag.disk())),
        disc_total: tags.iter().find_map(|tag| positive(tag.disk_total())),
        genre: join_values(texts(ItemKey::Genre).iter().map(String::as_str)),
        composer: join_values(texts(ItemKey::Composer).iter().map(String::as_str)),
        comment: clean_display_comment(comment_raw.as_deref()),
        cover_mime_type: cover.map(cover_mime),
        cover_data: cover.map(|picture| picture.data().to_vec()),
        file_size,
        file_format: path
            .extension()
            .map(|ext| ext.to_string_lossy().to_uppercase())
            .filter(|ext| !ext.is_empty()),
    }
}

fn positive(value: Option<u32>) -> Option<u32> {
    value.filter(|&v| v > 0)
}

/// 第一个“有非空白值”的标签给出的全部值。
fn first_with_values(tags: &[&Tag], pick: impl Fn(&Tag) -> Vec<String>) -> Vec<String> {
    tags.iter()
        .map(|tag| pick(tag))
        .find(|values| values.iter().any(|v| !v.trim().is_empty()))
        .unwrap_or_default()
}

fn text_of(item: &TagItem) -> Option<&str> {
    match item.value() {
        ItemValue::Text(text) | ItemValue::Locator(text) => Some(text),
        _ => None,
    }
}

/// 多值字段（艺术家、专辑艺术家、流派、作曲家）的各个值。
///
/// MP4 的 `ilst` 没有多值约定：TagLib 把多个值用 `; ` 拼进同一个 `data` 原子，读的时候再按 `;` 拆开。
/// C# 版写出的 M4A 都是这种形式，所以读 `ilst` 时也要拆，才能与 C# 读出的“a/b”一致。
fn multi_texts_of(tag: &Tag, key: ItemKey) -> Vec<String> {
    let values = texts_of(tag, &[key]);
    if tag.tag_type() != TagType::Mp4Ilst {
        return values;
    }
    values
        .iter()
        .flat_map(|value| value.split(';'))
        .map(|part| part.trim().to_owned())
        .collect()
}

fn texts_of(tag: &Tag, keys: &[ItemKey]) -> Vec<String> {
    keys.iter()
        .flat_map(|&key| tag.get_items(key))
        .filter_map(|item| text_of(item).map(str::to_owned))
        .collect()
}

/// 备注原文：优先没有描述的备注（`COMM` 的 description 为空 / Vorbis 的 `COMMENT`），其次 Vorbis 的 `DESCRIPTION`。
///
/// 带描述的 `COMM`（`iTunNORM`、`iTunSMPB` 等程序自用数据）不当作备注显示。
fn comment_text(tags: &[&Tag]) -> Option<String> {
    tags.iter().find_map(|tag| {
        tag.get_items(ItemKey::Comment)
            .filter(|item| item.description().is_empty())
            .filter_map(text_of)
            .find(|text| !text.trim().is_empty())
            .or_else(|| {
                tag.get_items(ItemKey::Description)
                    .filter_map(text_of)
                    .find(|text| !text.trim().is_empty())
            })
            .map(str::to_owned)
    })
}

/// 歌词：ID3v2 的 `USLT`、Vorbis 的 `LYRICS` / `UNSYNCEDLYRICS`、MP4 的 `©lyr`。没有描述的优先。
fn lyrics_text(tags: &[&Tag]) -> Option<String> {
    tags.iter().find_map(|tag| {
        let mut candidates: Vec<&TagItem> = tag
            .get_items(ItemKey::UnsyncLyrics)
            .chain(tag.get_items(ItemKey::Lyrics))
            .filter(|item| text_of(item).is_some_and(|t| !t.trim().is_empty()))
            .collect();
        candidates.sort_by_key(|item| !item.description().is_empty());
        candidates
            .first()
            .and_then(|item| text_of(item))
            .map(str::to_owned)
    })
}

/// 网易云 ID：先看备注（标记优先，其次客户端 `163 key`）；备注可能散落在多个带描述的 `COMM` 里，所以全扫一遍。
fn netease_id_of(tags: &[&Tag]) -> Option<i64> {
    let preferred = comment_text(tags);
    if let Some(id) = parse_netease_id(preferred.as_deref()) {
        return Some(id);
    }
    tags.iter()
        .flat_map(|tag| {
            tag.get_items(ItemKey::Comment)
                .chain(tag.get_items(ItemKey::Description))
        })
        .filter_map(text_of)
        .find_map(|text| parse_netease_id(Some(text)))
}

/// 年份：`RecordingDate`（ID3 `TDRC` / Vorbis `DATE` / MP4 `©day`）或 `Year`，取开头的 4 位数字。
fn year_of(tag: &Tag) -> Option<u32> {
    [ItemKey::RecordingDate, ItemKey::Year]
        .into_iter()
        .filter_map(|key| tag.get_string(key))
        .find_map(leading_year)
}

fn leading_year(text: &str) -> Option<u32> {
    let digits: String = text
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.len() < 4 {
        return None;
    }
    digits[..4].parse::<u32>().ok().filter(|&year| year > 0)
}

/// 封面：优先正面封面，没有就取第一张图片。
fn find_cover<'a>(tags: &[&'a Tag]) -> Option<&'a Picture> {
    tags.iter()
        .find_map(|tag| {
            tag.get_picture_type(PictureType::CoverFront)
                .or_else(|| tag.pictures().first())
        })
        .filter(|picture| !picture.data().is_empty())
}

fn cover_mime(picture: &Picture) -> String {
    match picture.mime_type() {
        Some(mime) if !mime.as_str().is_empty() => mime.as_str().to_owned(),
        _ => detect_image_mime(picture.data()).to_owned(),
    }
}

// ───────────────────────────── 写入 ─────────────────────────────

static TEMP_COUNTER: AtomicU32 = AtomicU32::new(0);

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// 同目录临时副本的路径：`<原文件名>.lp-tmp-<进程号>-<序号>`（扩展名不是音频，崩溃残留也不会被当作曲库文件）。
fn temp_path_for(path: &Path) -> PathBuf {
    let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(format!(".lp-tmp-{}-{sequence}", std::process::id()));
    path.with_file_name(name)
}

/// 退出时清理临时副本（成功 rename 之后文件已不存在，`remove_file` 的错误忽略）。
struct TempGuard(PathBuf);

impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn write_tags(path: &Path, data: &AudioTagData) -> Result<(), TagError> {
    if !path.is_file() {
        return Err(TagError::NotFound(path.display().to_string()));
    }
    let name = file_name_of(path);
    let Some(mut tagged) = open_tagged(path)? else {
        return Err(TagError::Unsupported(name));
    };
    let file_type = tagged.file_type();
    let targets = write_targets(file_type).ok_or_else(|| TagError::Unsupported(name.clone()))?;
    let id3v23 = wants_id3v23(path, file_type);

    for &tag_type in targets {
        if tagged.tag(tag_type).is_none() {
            tagged.insert_tag(Tag::new(tag_type));
        }
        let tag = tagged
            .tag_mut(tag_type)
            .ok_or_else(|| TagError::Unsupported(name.clone()))?;
        apply_data(tag, data, id3v23);
    }

    // 编辑器的“完整替换”语义下，ID3v2 已经包含全部信息；遗留的 ID3v1（C# 版每次都会另写一份）会在 ID3v2
    // 缺字段时被当作兜底读出来，导致“清空了的字段又冒出来”，所以一并移除（写一个空的 ID3v1 就是删除）。
    if data.replace_all && file_type == FileType::Mpeg && tagged.tag(TagType::Id3v1).is_some() {
        tagged.insert_tag(Tag::new(TagType::Id3v1));
    }

    // 我们总是“复制一份再整体写出”，从不就地修改，预留填充没有意义，反而让 Ogg 每次多出几 KB。
    let mut options = WriteOptions::default().preferred_padding(0);
    if id3v23 {
        options.use_id3v23(true);
    }
    let temp = temp_path_for(path);
    let _guard = TempGuard(temp.clone());
    fs::copy(path, &temp)?;
    if file_type == FileType::Wav {
        repair_riff_size(&temp)?;
    }
    {
        let mut copy = fs::OpenOptions::new().read(true).write(true).open(&temp)?;
        let mut tags: Vec<&Tag> = tagged.tags().iter().collect();
        tags.sort_by_key(|tag| tag.tag_type() != TagType::Id3v2); // RIFF 容器里先写 ID3v2 块，再写 RIFF INFO
        for tag in tags {
            if tagged.tag_support(tag.tag_type()).is_writable() {
                save_tag(tag, &mut copy, options).map_err(|error| save_error(&error, &name))?;
            }
        }
    }

    // 先确认副本还能被解析、类型没变，再覆盖原文件。
    let verified = verify_copy(&temp, file_type).map_err(|_| TagError::SaveFailed {
        file: name.clone(),
        reason: "写入后的文件校验失败，已保留原文件".to_owned(),
    })?;
    if !verified {
        return Err(TagError::SaveFailed {
            file: name,
            reason: "写入后的文件校验失败，已保留原文件".to_owned(),
        });
    }
    fs::rename(&temp, path)?;
    Ok(())
}

/// 把通用标签存回文件。
///
/// 必须先转成容器自己的标签类型再保存：通用 `Tag` 的保存路径只对 ID3v2 / MP4 带上“伴随标签”
/// （编辑器没展示的 TXXX、自定义 Vorbis 字段、RIFF INFO / APE 的其他条目），其余容器会悄悄丢掉它们。
fn save_tag(
    tag: &Tag,
    file: &mut File,
    options: WriteOptions,
) -> Result<(), lofty::error::FileEncodingError> {
    use std::io::Seek;
    file.rewind()?;
    match tag.tag_type() {
        TagType::VorbisComments => {
            let mut tag = tag.clone();
            keep_vorbis_field_order(&mut tag);
            VorbisComments::from(tag).save_to(file, options)
        }
        TagType::Id3v2 => Id3v2Tag::from(tag.clone()).save_to(file, options),
        TagType::Mp4Ilst => Ilst::from(tag.clone()).save_to(file, options),
        TagType::RiffInfo => RiffInfoList::from(tag.clone()).save_to(file, options),
        TagType::Ape => ApeTag::from(tag.clone()).save_to(file, options),
        _ => tag.save_to(file, options),
    }
}

/// `VorbisComments::from(Tag)` 用 `Tag::take`（内部是 swap 式移除）取出 `EncoderSoftware`，会把排在最前面的
/// 条目换到它的位置，导致同名多值字段（`ARTIST` 等）顺序颠倒。先把它挪到最前面，`swap(0, 0)` 就不会打乱顺序。
fn keep_vorbis_field_order(tag: &mut Tag) {
    let mut items: Vec<TagItem> = tag.items().cloned().collect();
    items.sort_by_key(|item| item.key() != ItemKey::EncoderSoftware);
    tag.retain(|_| false);
    for item in items {
        tag.push_unchecked(item);
    }
}

/// 修复 RIFF 头里偏小的总长度。
///
/// C# 版（TagLibSharp）往 WAV 里追加 `id3 ` / `LIST` 块之后不会更新 RIFF 头的长度，文件实际比头里声明的大。
/// 严格的解析器（lofty、很多播放器）按声明长度找块，会找不到后面追加的标签块，结果是写入时新建一个
/// `ID3 ` 块而旧的 `id3 ` 块还在，读出来仍是旧值。只有当声明长度之后的数据恰好能按块边界走完时才修正，
/// 否则（真正的“尾部垃圾”）原样保留。
fn repair_riff_size(path: &Path) -> Result<(), TagError> {
    use std::io::{Read, Seek, SeekFrom, Write};

    let mut file = fs::OpenOptions::new().read(true).write(true).open(path)?;
    let length = file.metadata()?.len();
    let mut header = [0u8; 12];
    if length < 12
        || file.read_exact(&mut header).is_err()
        || &header[..4] != b"RIFF"
        || &header[8..12] != b"WAVE"
    {
        return Ok(());
    }
    let declared = u64::from(u32::from_le_bytes([
        header[4], header[5], header[6], header[7],
    ])) + 8;
    if declared >= length || length > u64::from(u32::MAX) {
        return Ok(());
    }

    // 从头按块走一遍，确认整份文件都是首尾相接的合法块。
    let mut position = 12u64;
    while position < length {
        if position + 8 > length {
            return Ok(());
        }
        let mut chunk = [0u8; 8];
        file.seek(SeekFrom::Start(position))?;
        file.read_exact(&mut chunk)?;
        let size = u64::from(u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]));
        position += 8 + size + (size & 1);
    }
    // 最后一个块的填充字节可能缺失（长度恰好差 1），允许。
    if position != length && position != length + 1 {
        return Ok(());
    }

    file.seek(SeekFrom::Start(4))?;
    file.write_all(&((length - 8) as u32).to_le_bytes())?;
    Ok(())
}

fn verify_copy(temp: &Path, expected: FileType) -> Result<bool, TagError> {
    let file = File::open(temp)?;
    let probe = Probe::with_file_type(BufReader::new(file), expected).options(ParseOptions::new());
    Ok(probe
        .read()
        .is_ok_and(|tagged| tagged.file_type() == expected))
}

fn save_error(error: &lofty::error::FileEncodingError, name: &str) -> TagError {
    if let Some(io) = io_error_in(error)
        && is_environmental(io)
    {
        return TagError::Io(copy_io_error(io).into());
    }
    TagError::SaveFailed {
        file: name.to_owned(),
        reason: "文件格式不支持或已损坏".to_owned(),
    }
}

/// 各容器要更新的标签：多数是主标签；WAV 与 C# 一样同时写 ID3v2 和 RIFF INFO（资源管理器只认后者）。
fn write_targets(file_type: FileType) -> Option<&'static [TagType]> {
    Some(match file_type {
        FileType::Mpeg | FileType::Aac | FileType::Aiff => &[TagType::Id3v2],
        FileType::Wav => &[TagType::Id3v2, TagType::RiffInfo],
        FileType::Flac | FileType::Opus | FileType::Vorbis | FileType::Speex => {
            &[TagType::VorbisComments]
        }
        FileType::Mp4 => &[TagType::Mp4Ilst],
        FileType::Ape | FileType::Mpc | FileType::WavPack => &[TagType::Ape],
        _ => return None,
    })
}

/// 是否按 ID3v2.3 写：新建标签用 2.3（UTF-16，兼容性最好，与 C# 一致）；文件里已经是 2.4 就保持 2.4。
/// 只对 MP3 / AAC 生效；WAV / AIFF 里的 ID3 块沿用 lofty 默认的 2.4（C# 版写的也是 2.4）。
fn wants_id3v23(path: &Path, file_type: FileType) -> bool {
    matches!(file_type, FileType::Mpeg | FileType::Aac) && existing_id3v2_major(path) != Some(4)
}

/// 读文件开头的 ID3v2 头，返回主版本号（没有 ID3v2 头返回 `None`）。
fn existing_id3v2_major(path: &Path) -> Option<u8> {
    use std::io::Read;
    let mut header = [0u8; 10];
    File::open(path).ok()?.read_exact(&mut header).ok()?;
    (header.starts_with(b"ID3")).then_some(header[3])
}

/// 把 `data` 应用到一个通用标签上，规则逐条对应 C# `ApplyTag`。
pub(super) fn apply_data(tag: &mut Tag, data: &AudioTagData, id3v23: bool) {
    let replace = data.replace_all;
    let tag_type = tag.tag_type();
    // ID3v2.3 没有“多值”：TagLib 与多数播放器用 `/` 把多个值拼进同一个帧；lofty 默认写的 NUL 分隔是 2.4 的做法，
    // 2.3 的读取方会在 NUL 处截断，只剩第一个值。2.4 才用每个值一个条目。
    let join_multi = id3v23 && tag_type == TagType::Id3v2;

    if replace || data.title.is_some() {
        set_single(tag, ItemKey::TrackTitle, data.title.as_deref());
    }

    let artists = clean_values(&data.artists);
    if replace || !artists.is_empty() {
        set_multi(tag, ItemKey::TrackArtist, &artists, join_multi);
    }

    if replace || data.album.is_some() {
        set_single(tag, ItemKey::AlbumTitle, data.album.as_deref());
    }

    if replace || data.album_artist.is_some() {
        let album_artist = null_if_blank(data.album_artist.as_deref());
        set_single(tag, ItemKey::AlbumArtist, album_artist.as_deref());
    }

    apply_numbers(tag, data);

    if replace || data.year.is_some() {
        apply_year(tag, data.year);
    }

    let genres = split_values(data.genre.as_deref());
    if replace || !genres.is_empty() {
        set_multi(tag, ItemKey::Genre, &genres, join_multi);
    }

    let composers = split_values(data.composer.as_deref());
    if replace || !composers.is_empty() {
        set_multi(tag, ItemKey::Composer, &composers, join_multi);
    }

    let lyrics = first_non_blank(&[data.word_lyrics.as_deref(), data.plain_lyrics.as_deref()]);
    if replace || lyrics.is_some() {
        set_lyrics(tag, tag_type, lyrics);
    }

    apply_comment(tag, data);
    apply_cover(tag, data);
}

fn first_non_blank<'a>(values: &[Option<&'a str>]) -> Option<&'a str> {
    values
        .iter()
        .flatten()
        .copied()
        .find(|v| !v.trim().is_empty())
}

fn text_item(key: ItemKey, text: &str) -> TagItem {
    TagItem::new(key, ItemValue::Text(text.to_owned()))
}

/// 单值字段：先清掉旧值；`None` 或空串就只清除（C# 把标签属性设成空串等于删除）。
fn set_single(tag: &mut Tag, key: ItemKey, value: Option<&str>) {
    tag.remove_key(key);
    if let Some(value) = value.filter(|v| !v.is_empty()) {
        tag.push(text_item(key, value));
    }
}

/// 多值字段（艺术家、流派、作曲家）：每个值一个条目，由 lofty 按容器规则编码（Vorbis 多个字段，ID3v2.4 NUL 分隔）。
/// `join` 为真（ID3v2.3）时用 `/` 拼成一个条目。
fn set_multi(tag: &mut Tag, key: ItemKey, values: &[String], join: bool) {
    tag.remove_key(key);
    if join {
        if !values.is_empty() {
            tag.push(text_item(key, &values.join("/")));
        }
        return;
    }
    for value in values {
        tag.push(text_item(key, value));
    }
}

/// 音轨 / 碟号。语义同 C#：`replace_all` 时没给号码就清除；总数 C# 版没有字段，TagLib 会保留已有总数，
/// 这里也一样（`track_total` 为 `None` 时沿用文件里的总数），但号码被清除时总数一并清除。
fn apply_numbers(tag: &mut Tag, data: &AudioTagData) {
    let replace = data.replace_all;
    if replace || data.track_number.is_some() || data.track_total.is_some() {
        let number = if replace || data.track_number.is_some() {
            data.track_number.filter(|&n| n > 0)
        } else {
            tag.track()
        };
        let total = data
            .track_total
            .filter(|&n| n > 0)
            .or_else(|| tag.track_total());
        tag.remove_track();
        tag.remove_track_total();
        if let Some(number) = number {
            tag.set_track(number);
            if let Some(total) = total {
                tag.set_track_total(total);
            }
        }
    }
    if replace || data.disc_number.is_some() || data.disc_total.is_some() {
        let number = if replace || data.disc_number.is_some() {
            data.disc_number.filter(|&n| n > 0)
        } else {
            tag.disk()
        };
        let total = data
            .disc_total
            .filter(|&n| n > 0)
            .or_else(|| tag.disk_total());
        tag.remove_disk();
        tag.remove_disk_total();
        if let Some(number) = number {
            tag.set_disk(number);
            if let Some(total) = total {
                tag.set_disk_total(total);
            }
        }
    }
}

/// 年份：统一写 `RecordingDate`（ID3 `TDRC`→2.3 时转 `TYER`，Vorbis `DATE`，MP4 `©day`）。
/// 已有的完整日期（`2003-05-21`）年份没变时保留，避免每次保存都把日期截成年份。
fn apply_year(tag: &mut Tag, year: Option<u32>) {
    let existing = year_of(tag);
    let keep_full_date = year.is_some() && existing == year;
    if keep_full_date {
        return;
    }
    tag.remove_key(ItemKey::RecordingDate);
    tag.remove_key(ItemKey::Year);
    if let Some(year) = year.filter(|&y| y > 0) {
        tag.push(text_item(ItemKey::RecordingDate, &year.to_string()));
    }
}

/// 歌词：ID3v2 用 `USLT`；Vorbis 用 `LYRICS`（和 TagLib 一致）；MP4 用 `©lyr`。
/// 只动没有描述的那条，别的带描述的歌词帧原样保留。
fn set_lyrics(tag: &mut Tag, tag_type: TagType, lyrics: Option<&str>) {
    tag.retain(|item| {
        !(matches!(item.key(), ItemKey::UnsyncLyrics | ItemKey::Lyrics)
            && item.description().is_empty())
    });
    let Some(lyrics) = lyrics else { return };
    let key = match tag_type {
        TagType::Id3v2 => ItemKey::UnsyncLyrics,
        _ => ItemKey::Lyrics,
    };
    let mut item = text_item(key, lyrics);
    item.set_lang(FRAME_LANGUAGE);
    tag.push(item);
}

/// 备注与网易云 ID 标记，逐行对应 C# `ApplyComment`。
fn apply_comment(tag: &mut Tag, data: &AudioTagData) {
    let existing = if data.replace_all {
        None
    } else {
        null_if_blank(comment_text(&[&*tag]).as_deref())
    };
    let base_comment = data.comment.clone().or(existing);

    let new_comment = if let Some(id) = data.netease_id.filter(|&id| id > 0) {
        let marker = format!("{NETEASE_COMMENT_PREFIX}{id}");
        let cleaned = clean_comment(&strip_netease_marker(base_comment.as_deref().unwrap_or("")));
        Some(if cleaned.is_empty() {
            marker
        } else {
            format!("{cleaned}; {marker}")
        })
    } else if data.replace_all {
        Some(null_if_blank(base_comment.as_deref()).unwrap_or_default())
    } else {
        null_if_blank(base_comment.as_deref())
    };

    if let Some(comment) = new_comment {
        set_comment(tag, &comment);
    }
}

/// 写备注：只替换没有描述的那条（带描述的 `COMM` 如 `iTunNORM` 保留）；空串表示清除。
fn set_comment(tag: &mut Tag, comment: &str) {
    tag.retain(|item| !(item.key() == ItemKey::Comment && item.description().is_empty()));
    if comment.is_empty() {
        return;
    }
    let mut item = text_item(ItemKey::Comment, comment);
    item.set_lang(FRAME_LANGUAGE);
    tag.push(item);
}

/// 封面：提供了就**替换全部图片**，只留这一张正面封面（描述 `Cover`）；没提供就不动，
/// 除非 `remove_cover`（编辑器的“移除封面”）要求删光全部图片。
fn apply_cover(tag: &mut Tag, data: &AudioTagData) {
    let Some(cover) = data.cover_data.as_ref().filter(|bytes| !bytes.is_empty()) else {
        if data.remove_cover {
            while tag.picture_count() > 0 {
                tag.remove_picture(0);
            }
        }
        return;
    };
    let mime = data
        .cover_mime_type
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| detect_image_mime(cover));
    while tag.picture_count() > 0 {
        tag.remove_picture(0);
    }
    tag.push_picture(
        Picture::unchecked(cover.clone())
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::from_str(mime))
            .description(COVER_DESCRIPTION)
            .build(),
    );
}

#[cfg(test)]
mod tests;
