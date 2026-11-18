//! 标签查看与编辑：读本地音频的标签 / 封面 / 歌词与文件信息，按编辑器的表单规则写回。
//!
//! 与 Tauri 外壳一样，校验与拼装写入请求都交给 `lp-core` 的 `tags::form::editor`，
//! 桥接层只负责把「二选一」的结果（数据或带类别的错误）摊平成 frb 能传的类型：
//! frb 的 `Result` 错误位只能放字符串，带 `kind` 的错误要放进返回值里。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use lp_core::editor::{CoverFile, read_cover_file};
use lp_core::lyrics::batch::BatchLyricsService;
use lp_core::tags::form::CoverChange;
use lp_core::tags::form::batch::build_patch;
use lp_core::tags::form::editor::{ArtistList, EditorFields, build_write_request};
use lp_core::tags::{
    AudioTagService, SidecarLyrics, TagCommandError, TagField, TagPatchResult, TagRowView, TagView,
    apply_patch, read_rows,
};

use crate::state::{bridge, run};

/// 同名歌词文件（音频文件旁边的 `.lrc`）。
pub struct SidecarLyricsDto {
    pub path: String,
    pub file_name: String,
    pub text: String,
}

impl From<SidecarLyrics> for SidecarLyricsDto {
    fn from(value: SidecarLyrics) -> Self {
        Self {
            path: value.path,
            file_name: value.file_name,
            text: value.text,
        }
    }
}

/// 查看 / 编辑画面用的标签视图。
///
/// 数字一律转成 `i64`：frb 把 `u64` 映射成 Dart 的 `BigInt`，而这里的字节数、位速落在
/// Dart 的 `int` 范围里，用 `int` 界面代码更省事。封面仍然用 data URL 传，Dart 侧截掉
/// `data:...;base64,` 前缀交给 `Image.memory`。
pub struct TagViewDto {
    pub path: String,
    pub file_name: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<i64>,
    pub track_number: Option<i64>,
    pub track_total: Option<i64>,
    pub disc_number: Option<i64>,
    pub disc_total: Option<i64>,
    pub genre: Option<String>,
    pub composer: Option<String>,
    pub lyricist: Option<String>,
    pub comment: Option<String>,
    pub lyrics: Option<String>,
    pub sidecar_lyrics: Option<SidecarLyricsDto>,
    pub netease_id: Option<String>,
    pub has_cover: bool,
    pub cover_data_url: Option<String>,
    pub cover_size: Option<i64>,
    pub cover_too_large: bool,
    pub cover_mime_type: Option<String>,
    pub cover_width: Option<i64>,
    pub cover_height: Option<i64>,
    pub cover_picture_type: Option<String>,
    pub duration_ms: i64,
    pub bitrate: i64,
    pub file_size: i64,
    pub file_format: Option<String>,
}

fn small_number(value: Option<u32>) -> Option<i64> {
    value.map(i64::from)
}

fn byte_count(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

impl From<TagView> for TagViewDto {
    fn from(value: TagView) -> Self {
        Self {
            path: value.path,
            file_name: value.file_name,
            title: value.title,
            artist: value.artist,
            album: value.album,
            album_artist: value.album_artist,
            year: small_number(value.year),
            track_number: small_number(value.track_number),
            track_total: small_number(value.track_total),
            disc_number: small_number(value.disc_number),
            disc_total: small_number(value.disc_total),
            genre: value.genre,
            composer: value.composer,
            lyricist: value.lyricist,
            comment: value.comment,
            lyrics: value.lyrics,
            sidecar_lyrics: value.sidecar_lyrics.map(SidecarLyricsDto::from),
            netease_id: value.netease_id,
            has_cover: value.has_cover,
            cover_data_url: value.cover_data_url,
            cover_size: value
                .cover_size
                .map(|size| i64::try_from(size).unwrap_or(i64::MAX)),
            cover_too_large: value.cover_too_large,
            cover_mime_type: value.cover_mime_type,
            cover_width: small_number(value.cover_width),
            cover_height: small_number(value.cover_height),
            cover_picture_type: value.cover_picture_type,
            duration_ms: value.duration_ms,
            bitrate: i64::from(value.bitrate),
            file_size: byte_count(value.file_size),
            file_format: value.file_format,
        }
    }
}

/// 编辑器表单里的全部文字（数字也是文字，保存时才校验）。
pub struct EditorFieldsDto {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub year: String,
    pub track_number: String,
    pub disc_number: String,
    pub genre: String,
    pub composer: String,
    pub lyricist: String,
    pub comment: String,
    pub lyrics: String,
}

impl From<&EditorFields> for EditorFieldsDto {
    fn from(value: &EditorFields) -> Self {
        Self {
            title: value.title.clone(),
            artist: value.artist.clone(),
            album: value.album.clone(),
            album_artist: value.album_artist.clone(),
            year: value.year.clone(),
            track_number: value.track_number.clone(),
            disc_number: value.disc_number.clone(),
            genre: value.genre.clone(),
            composer: value.composer.clone(),
            lyricist: value.lyricist.clone(),
            comment: value.comment.clone(),
            lyrics: value.lyrics.clone(),
        }
    }
}

impl EditorFieldsDto {
    fn to_fields(&self) -> EditorFields {
        EditorFields {
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            album_artist: self.album_artist.clone(),
            year: self.year.clone(),
            track_number: self.track_number.clone(),
            disc_number: self.disc_number.clone(),
            genre: self.genre.clone(),
            composer: self.composer.clone(),
            lyricist: self.lyricist.clone(),
            comment: self.comment.clone(),
            lyrics: self.lyrics.clone(),
        }
    }
}

/// 在线结果给的艺术家清单；M4 还没有在线检索，先留着接 M5。
pub struct ArtistListDto {
    pub text: String,
    pub values: Vec<String>,
}

impl ArtistListDto {
    fn to_artist_list(&self) -> ArtistList {
        ArtistList {
            text: self.text.clone(),
            values: self.values.clone(),
        }
    }
}

/// 封面怎么改：`keep`（原样）、`replace`（换新图，要带 Base64 与 MIME）、`remove`（移除）。
pub struct CoverChangeDto {
    pub kind: String,
    pub base64: Option<String>,
    pub mime_type: Option<String>,
}

impl CoverChangeDto {
    fn to_cover(&self) -> Result<CoverChange, String> {
        match self.kind.as_str() {
            "remove" => Ok(CoverChange::Remove),
            "replace" => match (&self.base64, &self.mime_type) {
                (Some(base64), Some(mime_type)) if !mime_type.trim().is_empty() => {
                    Ok(CoverChange::Replace {
                        base64: base64.clone(),
                        mime_type: mime_type.clone(),
                    })
                }
                _ => Err("封面数据不完整，无法写入。".to_owned()),
            },
            // 认不出的类别按「保持不变」处理：宁可少改，也不要误删用户的封面。
            _ => Ok(CoverChange::Keep),
        }
    }
}

/// 带类别的失败（`invalid` / `notFound` / `unsupported` / `failed`）。
pub struct TagErrorDto {
    pub kind: String,
    pub message: String,
}

impl From<TagCommandError> for TagErrorDto {
    fn from(value: TagCommandError) -> Self {
        Self {
            kind: value.kind.to_owned(),
            message: value.message,
        }
    }
}

impl TagErrorDto {
    fn new(kind: &str, message: String) -> Self {
        Self {
            kind: kind.to_owned(),
            message,
        }
    }
}

/// 只读查看的结果：`view` 与 `error` 都为空表示文件读不出标签（格式不支持或已损坏）。
pub struct TagReadResultDto {
    pub view: Option<TagViewDto>,
    pub error: Option<TagErrorDto>,
}

/// 打开编辑器的结果：`view` 与 `fields` 一起给出，`error` 非空时前两者为空。
pub struct TagEditorLoadDto {
    pub view: Option<TagViewDto>,
    pub fields: Option<EditorFieldsDto>,
    pub error: Option<TagErrorDto>,
}

/// 保存的结果：成功时给出重新读回的标签与表单；失败时 `error` 是中文原因。
pub struct TagEditorSaveDto {
    pub view: Option<TagViewDto>,
    pub fields: Option<EditorFieldsDto>,
    pub error: Option<TagErrorDto>,
}

impl TagEditorSaveDto {
    fn failed(kind: &str, message: String) -> Self {
        Self {
            view: None,
            fields: None,
            error: Some(TagErrorDto::new(kind, message)),
        }
    }
}

impl TagEditorLoadDto {
    fn failed(kind: &str, message: String) -> Self {
        Self {
            view: None,
            fields: None,
            error: Some(TagErrorDto::new(kind, message)),
        }
    }
}

/// 用户选的封面图片（M8 的文件对话框接上后用）。
pub struct CoverFileDto {
    pub mime_type: String,
    pub base64: String,
    pub size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub file_name: String,
}

impl From<CoverFile> for CoverFileDto {
    fn from(value: CoverFile) -> Self {
        Self {
            mime_type: value.mime_type,
            base64: value.base64,
            size: i64::try_from(value.size).unwrap_or(i64::MAX),
            width: small_number(value.width),
            height: small_number(value.height),
            file_name: value.file_name,
        }
    }
}

/// 把阻塞的文件读写丢到阻塞线程池里跑：lofty 读一个文件要几十毫秒，不能占住运行时线程。
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| format!("处理音频文件时程序内部出错，请重试：{error}"))
}

/// 读一次标签。`Ok(None)` 是「读不出标签」（格式不支持或文件损坏），不是错误。
fn read_only(tags: &dyn AudioTagService, path: &str) -> Result<Option<TagView>, TagCommandError> {
    let target = PathBuf::from(path);
    match tags.read(&target) {
        Ok(Some(info)) => Ok(Some(TagView::new(&target, info).with_sidecar_lyrics())),
        Ok(None) => Ok(None),
        Err(error) => Err(TagCommandError::from(error)),
    }
}

fn file_label(path: &str) -> String {
    PathBuf::from(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

fn read_values(tags: &dyn AudioTagService, path: &str) -> TagReadResultDto {
    match read_only(tags, path) {
        Ok(Some(view)) => TagReadResultDto {
            view: Some(TagViewDto::from(view)),
            error: None,
        },
        Ok(None) => TagReadResultDto {
            view: None,
            error: None,
        },
        Err(error) => TagReadResultDto {
            view: None,
            error: Some(TagErrorDto::from(error)),
        },
    }
}

fn load_editor(tags: &dyn AudioTagService, path: &str) -> TagEditorLoadDto {
    match read_only(tags, path) {
        Ok(Some(view)) => TagEditorLoadDto {
            fields: Some(EditorFieldsDto::from(&EditorFields::from_view(&view))),
            view: Some(TagViewDto::from(view)),
            error: None,
        },
        Ok(None) => TagEditorLoadDto {
            view: None,
            fields: None,
            error: None,
        },
        Err(error) => TagEditorLoadDto::failed(error.kind, error.message),
    }
}

fn save_values(
    tags: &dyn AudioTagService,
    path: &str,
    fields: EditorFields,
    loaded: Option<EditorFields>,
    online: Option<ArtistList>,
    cover: CoverChange,
) -> TagEditorSaveDto {
    let target = PathBuf::from(path);
    // 保存前重新读一遍：表单是打开时的快照，文件可能已经被别的程序改过；
    // 写入请求要按“现在文件里有什么”来拼（网易云 ID 与多值字段的 keep 都取自它）。
    let view = match read_only(tags, path) {
        Ok(Some(view)) => view,
        Ok(None) => {
            return TagEditorSaveDto::failed(
                "unsupported",
                format!(
                    "无法写入标签：不支持或已损坏的音频文件「{}」。",
                    file_label(path)
                ),
            );
        }
        Err(error) => return TagEditorSaveDto::failed(error.kind, error.message),
    };
    let request =
        match build_write_request(&view, &fields, loaded.as_ref(), online.as_ref(), &cover) {
            Ok(request) => request,
            Err(error) => return TagEditorSaveDto::failed("invalid", error.to_string()),
        };
    let data = match request.into_data() {
        Ok(data) => data,
        Err(error) => {
            let error = TagCommandError::from(error);
            return TagEditorSaveDto::failed(error.kind, error.message);
        }
    };
    if let Err(error) = tags.write(&target, &data) {
        let error = TagCommandError::from(error);
        return TagEditorSaveDto::failed(error.kind, error.message);
    }
    // 写成功后重新读回：文件格式可能把某些值（例如没有碟片总数的容器）丢掉，界面要显示真实结果。
    match read_only(tags, path) {
        Ok(Some(view)) => TagEditorSaveDto {
            fields: Some(EditorFieldsDto::from(&EditorFields::from_view(&view))),
            view: Some(TagViewDto::from(view)),
            error: None,
        },
        Ok(None) => TagEditorSaveDto {
            view: None,
            fields: None,
            error: None,
        },
        Err(error) => TagEditorSaveDto::failed(error.kind, error.message),
    }
}

/// 读取一个音频文件的标签（只读查看用）。
pub async fn read_tags(path: String) -> Result<TagReadResultDto, String> {
    let tags = Arc::clone(&bridge()?.tags);
    run(async move { blocking(move || read_values(&*tags, &path)).await }).await?
}

/// 打开编辑器：读标签并填好表单（歌词优先内嵌，其次同名 `.lrc`）。
pub async fn tag_editor_open(path: String) -> Result<TagEditorLoadDto, String> {
    let tags = Arc::clone(&bridge()?.tags);
    run(async move { blocking(move || load_editor(&*tags, &path)).await }).await?
}

/// 保存编辑器表单。
///
/// `loaded` 是打开时读出的表单（判断多值字段有没有被改过），`online` 是套用过的在线艺术家清单；
/// 两者都可为空，则按「用户改过」处理。
pub async fn tag_editor_save(
    path: String,
    fields: EditorFieldsDto,
    loaded: Option<EditorFieldsDto>,
    online: Option<ArtistListDto>,
    cover: CoverChangeDto,
) -> Result<TagEditorSaveDto, String> {
    let cover = match cover.to_cover() {
        Ok(cover) => cover,
        Err(message) => return Ok(TagEditorSaveDto::failed("invalid", message)),
    };
    let tags = Arc::clone(&bridge()?.tags);
    let fields = fields.to_fields();
    let loaded = loaded.as_ref().map(EditorFieldsDto::to_fields);
    let online = online.as_ref().map(ArtistListDto::to_artist_list);
    run(async move {
        blocking(move || save_values(&*tags, &path, fields, loaded, online, cover)).await
    })
    .await?
}

/// 读用户选的封面图片（校验大小与文件头，返回 Base64 与尺寸）。
pub async fn tag_cover_file(path: String) -> Result<CoverFileDto, String> {
    run(async move {
        blocking(move || read_cover_file(&PathBuf::from(path)).map(CoverFileDto::from)).await
    })
    .await??
}

/// 批量修改标签窗口列表里的一行。读不出来的文件也有一行（`readable == false`，`error` 是原因）。
pub struct TagRowDto {
    pub path: String,
    pub file_name: String,
    pub directory: String,
    pub file_format: Option<String>,
    pub readable: bool,
    pub error: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<i64>,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    pub genre: Option<String>,
    pub composer: Option<String>,
    pub lyricist: Option<String>,
    pub comment: Option<String>,
    pub lyrics_preview: Option<String>,
    pub has_lyrics: bool,
    pub lyrics_file: Option<String>,
    pub has_cover: bool,
    pub duration_ms: i64,
}

impl From<TagRowView> for TagRowDto {
    fn from(value: TagRowView) -> Self {
        Self {
            path: value.path,
            file_name: value.file_name,
            directory: value.directory,
            file_format: value.file_format,
            readable: value.readable,
            error: value.error,
            title: value.title,
            artist: value.artist,
            album: value.album,
            album_artist: value.album_artist,
            year: small_number(value.year),
            track_number: small_number(value.track_number),
            disc_number: small_number(value.disc_number),
            genre: value.genre,
            composer: value.composer,
            lyricist: value.lyricist,
            comment: value.comment,
            lyrics_preview: value.lyrics_preview,
            has_lyrics: value.has_lyrics,
            lyrics_file: value.lyrics_file,
            has_cover: value.has_cover,
            duration_ms: value.duration_ms,
        }
    }
}

/// 用户在批量面板里改过的一个字段：`field` 是 camelCase 的字段名（`title`、`albumArtist` …），
/// `value` 是输入框里的文字。没出现在列表里的字段表示没改过，保存时原样保留。
pub struct TagEditDto {
    pub field: String,
    pub value: String,
}

impl TagEditDto {
    fn to_field(&self) -> Option<TagField> {
        serde_json::from_value(serde_json::Value::String(self.field.clone())).ok()
    }
}

/// 一个文件套用批量改动的结果。
pub struct TagPatchResultDto {
    pub path: String,
    pub success: bool,
    pub message: Option<String>,
    pub row: TagRowDto,
}

impl From<TagPatchResult> for TagPatchResultDto {
    fn from(value: TagPatchResult) -> Self {
        Self {
            path: value.path,
            success: value.success,
            message: value.message,
            row: TagRowDto::from(value.row),
        }
    }
}

/// 批量保存的结果：成功时每个文件一条；改动本身不合法时 `error` 带上原因（此时 `results` 为空）。
pub struct TagPatchOutcomeDto {
    pub results: Option<Vec<TagPatchResultDto>>,
    pub error: Option<TagErrorDto>,
}

impl TagPatchOutcomeDto {
    fn failed(kind: &str, message: String) -> Self {
        Self {
            results: None,
            error: Some(TagErrorDto::new(kind, message)),
        }
    }
}

/// 读出这些文件的标签摘要（不带封面数据），顺序与 `paths` 一致；读不出来的文件也有一行。
pub async fn read_tag_rows(paths: Vec<String>) -> Result<Vec<TagRowDto>, String> {
    let tags = Arc::clone(&bridge()?.tags);
    run(async move {
        blocking(move || {
            let targets: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
            read_rows(&*tags, &targets)
        })
        .await
        .map(|rows| rows.into_iter().map(TagRowDto::from).collect())
    })
    .await?
}

/// 把同一份改动写进这些文件：`edits` 里出现的字段才改，空白表示清除；某个文件失败不影响其余文件。
///
/// 数字校验、艺术家拆分与「空白即清除」都在 `lp-core` 的批量表单规则里（[build_patch]），
/// 与 Tauri 版共用一份。
pub async fn patch_tags(
    paths: Vec<String>,
    edits: Vec<TagEditDto>,
    cover: CoverChangeDto,
) -> Result<TagPatchOutcomeDto, String> {
    let cover = match cover.to_cover() {
        Ok(cover) => cover,
        Err(message) => return Ok(TagPatchOutcomeDto::failed("invalid", message)),
    };
    let mut values = HashMap::new();
    for edit in &edits {
        let Some(field) = edit.to_field() else {
            return Ok(TagPatchOutcomeDto::failed(
                "invalid",
                format!("无法修改标签字段「{}」。", edit.field),
            ));
        };
        values.insert(field, edit.value.clone());
    }
    let patch = match build_patch(&values, &cover) {
        Ok(patch) => patch,
        Err(error) => return Ok(TagPatchOutcomeDto::failed("invalid", error.to_string())),
    };
    let data = match patch.into_data() {
        Ok(data) => data,
        Err(error) => {
            let error = TagCommandError::from(error);
            return Ok(TagPatchOutcomeDto::failed(error.kind, error.message));
        }
    };
    let tags = Arc::clone(&bridge()?.tags);
    run(async move {
        blocking(move || {
            let targets: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
            apply_patch(&*tags, &targets, &data)
        })
        .await
        .map(|results| TagPatchOutcomeDto {
            results: Some(results.into_iter().map(TagPatchResultDto::from).collect()),
            error: None,
        })
    })
    .await?
}

/// 批量面板扫描到的一个音频文件。
pub struct BatchFileDto {
    pub path: String,
    pub name: String,
    pub size: i64,
}

/// `batch_scan` 的结果：目录不存在时 `directory_missing` 为真、文件为空。
pub struct BatchScanDto {
    pub folder: String,
    pub recursive: bool,
    pub directory_missing: bool,
    pub files: Vec<BatchFileDto>,
}

/// 列出目录里的音频文件（扩展名规则与 Tauri 版共用 [BatchLyricsService::scan_audio_files]）。
pub async fn batch_scan(folder: String, recursive: bool) -> Result<BatchScanDto, String> {
    run(async move { blocking(move || scan_audio_folder(folder, recursive)).await }).await?
}

/// 真正扫目录的那一步：磁盘遍历可能很慢，调用方放在阻塞线程里跑。
fn scan_audio_folder(folder: String, recursive: bool) -> BatchScanDto {
    let trimmed = folder.trim().to_owned();
    let directory_missing = trimmed.is_empty() || !PathBuf::from(&trimmed).is_dir();
    let files = if directory_missing {
        Vec::new()
    } else {
        BatchLyricsService::scan_audio_files(&trimmed, recursive)
            .into_iter()
            .map(|path| BatchFileDto {
                name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                size: std::fs::metadata(&path).map_or(0, |meta| meta.len() as i64),
                path: path.to_string_lossy().into_owned(),
            })
            .collect()
    };
    BatchScanDto {
        folder,
        recursive,
        directory_missing,
        files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lp_core::tags::TagError;

    #[test]
    fn blank_folder_is_reported_missing() {
        let scan = scan_audio_folder("   ".to_owned(), true);
        assert!(scan.directory_missing);
        assert!(scan.files.is_empty());
    }

    #[test]
    fn scan_finds_the_audio_fixtures() {
        let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../lp-core/tests/fixtures/library/music");
        let scan = scan_audio_folder(folder.to_string_lossy().into_owned(), true);
        assert!(!scan.directory_missing);
        assert!(scan.files.iter().any(|file| file.name.ends_with(".mp3")));
        assert!(scan.files.iter().any(|file| file.name.ends_with(".flac")));
        assert!(scan.files.iter().all(|file| file.size > 0));
    }

    fn sample_fields() -> EditorFieldsDto {
        EditorFieldsDto {
            title: "夜に駆ける".to_owned(),
            artist: "YOASOBI".to_owned(),
            album: "THE BOOK".to_owned(),
            album_artist: "YOASOBI".to_owned(),
            year: "2019".to_owned(),
            track_number: "1".to_owned(),
            disc_number: "1".to_owned(),
            genre: "J-Pop".to_owned(),
            composer: "Ayase".to_owned(),
            lyricist: "Ayase".to_owned(),
            comment: "备注".to_owned(),
            lyrics: "[00:01.00]沈むように".to_owned(),
        }
    }

    #[test]
    fn editor_fields_survive_a_round_trip() {
        let fields = sample_fields().to_fields();
        let back = EditorFieldsDto::from(&fields);
        let again = back.to_fields();
        assert_eq!(fields, again);
        assert_eq!(back.title, "夜に駆ける");
        assert_eq!(back.lyrics, "[00:01.00]沈むように");
    }

    #[test]
    fn cover_change_kind_decides_what_happens() {
        let keep = CoverChangeDto {
            kind: "keep".to_owned(),
            base64: None,
            mime_type: None,
        };
        assert_eq!(keep.to_cover(), Ok(CoverChange::Keep));

        let remove = CoverChangeDto {
            kind: "remove".to_owned(),
            base64: None,
            mime_type: None,
        };
        assert_eq!(remove.to_cover(), Ok(CoverChange::Remove));

        let replace = CoverChangeDto {
            kind: "replace".to_owned(),
            base64: Some("AA==".to_owned()),
            mime_type: Some("image/png".to_owned()),
        };
        assert_eq!(
            replace.to_cover(),
            Ok(CoverChange::Replace {
                base64: "AA==".to_owned(),
                mime_type: "image/png".to_owned(),
            })
        );
    }

    #[test]
    fn replace_without_data_is_rejected() {
        let broken = CoverChangeDto {
            kind: "replace".to_owned(),
            base64: None,
            mime_type: Some("image/png".to_owned()),
        };
        assert!(broken.to_cover().is_err());
    }

    #[test]
    fn tag_view_numbers_become_i64() {
        let view = TagView::new(std::path::Path::new("C:/Music/曲.mp3"), Default::default());
        let dto = TagViewDto::from(view);
        assert_eq!(dto.file_name, "曲.mp3");
        assert_eq!(dto.year, None);
        assert_eq!(dto.duration_ms, 0);
        assert_eq!(dto.bitrate, 0);
        assert_eq!(dto.file_size, 0);
        assert_eq!(dto.cover_size, None);
        assert_eq!(dto.lyrics, None);
        assert!(dto.sidecar_lyrics.is_none());
        assert!(!dto.has_cover);
    }

    #[test]
    fn error_kind_is_kept_for_the_ui() {
        let error = TagCommandError::from(TagError::Unsupported("坏文件.mp3".to_owned()));
        let dto = TagErrorDto::from(error);
        assert_eq!(dto.kind, "unsupported");
        assert!(dto.message.contains("坏文件.mp3"));

        let failed = TagEditorSaveDto::failed("invalid", "年份必须是数字。".to_owned());
        assert_eq!(
            failed.error.as_ref().map(|e| e.kind.as_str()),
            Some("invalid")
        );
        assert!(failed.view.is_none() && failed.fields.is_none());
    }

    #[test]
    fn batch_field_names_round_trip() {
        let names = [
            ("title", TagField::Title),
            ("artist", TagField::Artist),
            ("album", TagField::Album),
            ("albumArtist", TagField::AlbumArtist),
            ("year", TagField::Year),
            ("trackNumber", TagField::TrackNumber),
            ("discNumber", TagField::DiscNumber),
            ("genre", TagField::Genre),
            ("composer", TagField::Composer),
            ("lyricist", TagField::Lyricist),
            ("comment", TagField::Comment),
        ];
        for (name, field) in names {
            let edit = TagEditDto {
                field: name.to_owned(),
                value: String::new(),
            };
            assert_eq!(edit.to_field(), Some(field), "{name}");
        }
        let unknown = TagEditDto {
            field: "coverBase64".to_owned(),
            value: String::new(),
        };
        assert_eq!(unknown.to_field(), None);
    }

    #[test]
    fn batch_row_numbers_become_i64() {
        let row = TagRowView {
            path: "C:/Music/曲.mp3".to_owned(),
            readable: true,
            year: Some(2024),
            track_number: Some(3),
            disc_number: Some(1),
            duration_ms: 245_000,
            ..TagRowView::default()
        };
        let dto = TagRowDto::from(row);
        assert_eq!(dto.year, Some(2024));
        assert_eq!(dto.track_number, Some(3));
        assert_eq!(dto.disc_number, Some(1));
        assert_eq!(dto.duration_ms, 245_000);
    }

    #[test]
    fn unreadable_row_keeps_its_reason() {
        let row = TagRowView::unreadable(
            std::path::Path::new("C:/Music/坏文件.mp3"),
            "无法读取标签：文件不存在、格式不支持或已损坏。".to_owned(),
        );
        let dto = TagRowDto::from(row);
        assert!(!dto.readable);
        assert_eq!(dto.file_name, "坏文件.mp3");
        assert!(
            dto.error
                .as_deref()
                .is_some_and(|reason| reason.contains("无法读取标签"))
        );
    }
}
