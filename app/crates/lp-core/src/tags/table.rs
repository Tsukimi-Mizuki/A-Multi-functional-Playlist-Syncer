//! 批量修改标签窗口用的“表格”：一行一个文件的标签摘要（不带封面数据），以及把同一份改动套用到多个文件。
//!
//! 界面右边是歌曲列表、左边是选中歌曲的标签：列表靠 [`read_rows`] 填，左边改完后由 [`apply_patch`]
//! 只把动过的字段写进所有选中的文件（没提到的字段原样保留，与 Mp3tag 的「〈保持〉」一致）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serialize};

use super::model::{AudioTagData, AudioTagInfo, TagError, TagField};
use super::service::AudioTagService;
use super::sidecar::read_sidecar_lyrics;
use super::text::clean_values;

/// 读标签的并发上限（与曲库扫描一致）。
const READ_WORKERS: usize = 8;
/// 写标签的并发上限：每个文件都要整份复制再替换，开太多只会互相抢磁盘。
const WRITE_WORKERS: usize = 4;
/// 列表里歌词预览的最大字符数。
const LYRICS_PREVIEW_CHARS: usize = 120;

/// 列表里的一行（camelCase）。读不出来的文件也有一行：`readable == false`，`error` 是原因。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRowView {
    /// 完整路径。
    pub path: String,
    /// 文件名。
    pub file_name: String,
    /// 所在目录。
    pub directory: String,
    /// 文件格式（大写扩展名）。
    pub file_format: Option<String>,
    /// 标签是否读出来了。
    pub readable: bool,
    /// 读不出来的原因（中文）。
    pub error: Option<String>,
    /// 标题。
    pub title: Option<String>,
    /// 艺术家（多个用 `/` 连接）。
    pub artist: Option<String>,
    /// 专辑。
    pub album: Option<String>,
    /// 专辑艺术家。
    pub album_artist: Option<String>,
    /// 年份。
    pub year: Option<u32>,
    /// 音轨号。
    pub track_number: Option<u32>,
    /// 碟号。
    pub disc_number: Option<u32>,
    /// 流派。
    pub genre: Option<String>,
    /// 作曲家。
    pub composer: Option<String>,
    /// 作词家。
    pub lyricist: Option<String>,
    /// 备注（已去掉网易云 ID 标记）。
    pub comment: Option<String>,
    /// 歌词的第一行（最多 120 个字符）；没有歌词为 `None`。
    pub lyrics_preview: Option<String>,
    /// 是否有歌词（内嵌的，或旁边同名的 `.lrc` 文件）。
    pub has_lyrics: bool,
    /// 歌词来自旁边同名的 `.lrc` 文件时是它的文件名；内嵌歌词或没有歌词为 `None`。
    pub lyrics_file: Option<String>,
    /// 是否有封面。
    pub has_cover: bool,
    /// 时长（毫秒）。
    pub duration_ms: i64,
}

impl TagRowView {
    /// 由读取结果构造一行。
    pub fn new(path: &Path, info: AudioTagInfo) -> Self {
        Self {
            readable: true,
            lyrics_preview: info.lyrics.as_deref().and_then(lyrics_preview),
            has_lyrics: info.lyrics.is_some(),
            has_cover: info.has_cover,
            duration_ms: info.duration_ms,
            title: info.title,
            artist: info.artist,
            album: info.album,
            album_artist: info.album_artist,
            year: info.year,
            track_number: info.track_number,
            disc_number: info.disc_number,
            genre: info.genre,
            composer: info.composer,
            lyricist: info.lyricist,
            comment: info.comment,
            ..Self::located(path)
        }
    }

    /// 没有内嵌歌词时，用旁边同名 `.lrc` 文件的第一行当预览。
    fn with_sidecar_lyrics(mut self, path: &Path) -> Self {
        if !self.has_lyrics
            && let Some(sidecar) = read_sidecar_lyrics(path)
        {
            self.lyrics_preview = lyrics_preview(&sidecar.text);
            self.has_lyrics = true;
            self.lyrics_file = Some(sidecar.file_name);
        }
        self
    }

    /// 读不出标签的文件：只有路径信息与原因。
    pub fn unreadable(path: &Path, reason: String) -> Self {
        Self {
            error: Some(reason),
            ..Self::located(path)
        }
    }

    /// 只填了路径信息的一行。
    fn located(path: &Path) -> Self {
        Self {
            path: path.to_string_lossy().into_owned(),
            file_name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            directory: path
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned())
                .unwrap_or_default(),
            file_format: path
                .extension()
                .map(|ext| ext.to_string_lossy().to_uppercase())
                .filter(|ext| !ext.is_empty()),
            ..Self::default()
        }
    }

    /// 读一个文件并构造一行；读取失败时返回带原因的行，不向上报错（列表里其余文件照常显示）。
    ///
    /// 文件里没有内嵌歌词时看一眼旁边同名的 `.lrc`，与歌词与标签编辑器打开这一首时显示的一致。
    pub fn read(service: &dyn AudioTagService, path: &Path) -> Self {
        match service.read(path) {
            Ok(Some(info)) => Self::new(path, info).with_sidecar_lyrics(path),
            Ok(None) => Self::unreadable(
                path,
                "无法读取标签：文件不存在、格式不支持或已损坏。".to_owned(),
            ),
            Err(error) => Self::unreadable(path, error.to_string()),
        }
    }
}

/// 歌词的第一行非空白内容，截到 [`LYRICS_PREVIEW_CHARS`] 个字符。
fn lyrics_preview(lyrics: &str) -> Option<String> {
    let line = lyrics
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    Some(line.chars().take(LYRICS_PREVIEW_CHARS).collect())
}

/// 按下标并行跑 `job`（最多 `max_workers` 个线程），结果顺序与下标一致。
/// 工作线程 panic（标签库的缺陷）时把 panic 原样抛给调用方，与曲库扫描一致。
fn run_parallel<T: Send>(
    count: usize,
    max_workers: usize,
    job: impl Fn(usize) -> T + Sync,
) -> Vec<T> {
    if count == 0 {
        return Vec::new();
    }
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, max_workers)
        .min(count);
    let next = AtomicUsize::new(0);
    let mut slots: Vec<Option<T>> = (0..count).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done: Vec<(usize, T)> = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::SeqCst);
                        if index >= count {
                            break;
                        }
                        done.push((index, job(index)));
                    }
                    done
                })
            })
            .collect();
        for handle in handles {
            match handle.join() {
                Ok(done) => {
                    for (index, value) in done {
                        slots[index] = Some(value);
                    }
                }
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
    });
    slots.into_iter().flatten().collect()
}

/// 读出这些文件的标签摘要，顺序与 `paths` 一致。
pub fn read_rows(service: &dyn AudioTagService, paths: &[PathBuf]) -> Vec<TagRowView> {
    run_parallel(paths.len(), READ_WORKERS, |index| {
        TagRowView::read(service, &paths[index])
    })
}

/// “字段出现了”与“字段是 null”要分开：没出现 = 不动，`null` = 清除。
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// 批量修改的一份改动（camelCase）：**没出现的字段不动**，`null` / 空白 / 空数组表示清除，其余是新值。
///
/// 歌词不在这里改（由歌词与标签编辑器逐首处理）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TagPatch {
    #[serde(deserialize_with = "present")]
    pub title: Option<Option<String>>,
    /// 艺术家（前端已拆成多个）；空数组表示清除。
    pub artists: Option<Vec<String>>,
    #[serde(deserialize_with = "present")]
    pub album: Option<Option<String>>,
    #[serde(deserialize_with = "present")]
    pub album_artist: Option<Option<String>>,
    #[serde(deserialize_with = "present")]
    pub year: Option<Option<u32>>,
    #[serde(deserialize_with = "present")]
    pub track_number: Option<Option<u32>>,
    #[serde(deserialize_with = "present")]
    pub disc_number: Option<Option<u32>>,
    #[serde(deserialize_with = "present")]
    pub genre: Option<Option<String>>,
    #[serde(deserialize_with = "present")]
    pub composer: Option<Option<String>>,
    #[serde(deserialize_with = "present")]
    pub lyricist: Option<Option<String>>,
    #[serde(deserialize_with = "present")]
    pub comment: Option<Option<String>>,
    /// 新封面图片字节的 Base64（不带 `data:` 前缀）；不传就不动封面。
    pub cover_base64: Option<String>,
    pub cover_mime_type: Option<String>,
    /// 删除全部封面；同时给了 `coverBase64` 时以新封面为准。
    pub remove_cover: bool,
}

impl TagPatch {
    /// 转成“只更新提到的字段”的写入数据；封面 Base64 不合法或一个字段都没提到时报错。
    pub fn into_data(self) -> Result<AudioTagData, TagError> {
        let mut clear = Vec::new();
        let mut text = |value: Option<Option<String>>, field: TagField| -> Option<String> {
            let value = value?;
            let trimmed = value.as_deref().map(str::trim).unwrap_or_default();
            if trimmed.is_empty() {
                clear.push(field);
                None
            } else {
                Some(trimmed.to_owned())
            }
        };
        let title = text(self.title, TagField::Title);
        let album = text(self.album, TagField::Album);
        let album_artist = text(self.album_artist, TagField::AlbumArtist);
        let genre = text(self.genre, TagField::Genre);
        let composer = text(self.composer, TagField::Composer);
        let lyricist = text(self.lyricist, TagField::Lyricist);
        let comment = text(self.comment, TagField::Comment);

        let mut number = |value: Option<Option<u32>>, field: TagField| -> Option<u32> {
            let value = value?.filter(|&n| n > 0);
            if value.is_none() {
                clear.push(field);
            }
            value
        };
        let year = number(self.year, TagField::Year);
        let track_number = number(self.track_number, TagField::TrackNumber);
        let disc_number = number(self.disc_number, TagField::DiscNumber);

        let artists = self
            .artists
            .as_deref()
            .map(clean_values)
            .unwrap_or_default();
        if self.artists.is_some() && artists.is_empty() {
            clear.push(TagField::Artist);
        }

        let cover_data = match self.cover_base64.as_deref().map(str::trim) {
            Some(text) if !text.is_empty() => Some(
                STANDARD
                    .decode(text)
                    .map_err(|_| TagError::InvalidArgument("封面数据不是有效的 Base64。"))?,
            ),
            _ => None,
        };

        let data = AudioTagData {
            title,
            artists,
            album,
            album_artist,
            genre,
            composer,
            lyricist,
            comment,
            year,
            track_number,
            disc_number,
            cover_mime_type: cover_data.as_ref().and(self.cover_mime_type),
            remove_cover: self.remove_cover && cover_data.is_none(),
            cover_data,
            clear,
            ..AudioTagData::default()
        };
        if data == AudioTagData::default() {
            return Err(TagError::InvalidArgument("没有要修改的内容。"));
        }
        Ok(data)
    }
}

/// 一个文件套用改动的结果（camelCase）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagPatchResult {
    /// 完整路径。
    pub path: String,
    /// 是否写入成功。
    pub success: bool,
    /// 失败原因（中文）；成功为 `None`。
    pub message: Option<String>,
    /// 写入之后重新读出的这一行（失败时就是文件里原来的内容）。
    pub row: TagRowView,
}

/// 把同一份改动写进每个文件（[`TagPatch::into_data`] 的结果），顺序与 `paths` 一致，重复的路径只处理一次。
///
/// 某个文件失败不影响其余文件。改动里动了备注时，先读出这个文件的网易云 ID 并随备注一起写回，
/// 否则 `LunaticPrincess:<id>` 标记会随旧备注一起丢掉，下次对比曲库就认不出这首歌。
pub fn apply_patch(
    service: &dyn AudioTagService,
    paths: &[PathBuf],
    data: &AudioTagData,
) -> Vec<TagPatchResult> {
    let mut seen = HashSet::new();
    let targets: Vec<&PathBuf> = paths
        .iter()
        .filter(|path| seen.insert(path.to_string_lossy().to_lowercase()))
        .collect();
    let touches_comment = data.comment.is_some() || data.clear.contains(&TagField::Comment);

    run_parallel(targets.len(), WRITE_WORKERS, |index| {
        let path = targets[index].as_path();
        let written = if touches_comment {
            service.read(path).and_then(|info| {
                let netease_id = info.and_then(|info| info.netease_id);
                service.write(
                    path,
                    &AudioTagData {
                        netease_id,
                        ..data.clone()
                    },
                )
            })
        } else {
            service.write(path, data)
        };
        TagPatchResult {
            path: path.to_string_lossy().into_owned(),
            success: written.is_ok(),
            message: written.err().map(|error| error.to_string()),
            row: TagRowView::read(service, path),
        }
    })
}

#[cfg(test)]
mod tests;
