//! 歌词与标签编辑器、拖放与启动参数里与界面无关的部分。
//!
//! 对应 C#：`LyricsEditorViewModel` 的“记住来源与歌词格式”、`MainWindowViewModel.HandleDroppedTargetsAsync` /
//! `LyricsEditorViewModel.LoadDroppedPathsAsync` 的拖入解析、`Program.GetStartupLyricsFile`、`ElevationHelper`。
//! 放在 lp-core 里才能单测；Tauri 外壳只转发命令。

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;

use crate::models::{LyricsSourceKind, OnlineSourcePriority};
use crate::online::view::MatchThresholdView;
use crate::settings::AppSettings;
use crate::tags::detect_image_mime;
use crate::utilities::audio_file_types::{
    EMPTY_TARGET_MESSAGE, EXTENSION_HINT, SUPPORTED_EXTENSIONS, try_resolve,
};
use crate::utilities::error_text;

/// 管理员身份运行时 Windows 的 UIPI 会阻止资源管理器向本程序拖放文件，且没有任何客户端开关可以放行，
/// 所以只能在界面上说明原因并给出替代操作（同 C# `ElevationHelper.DragDropBlockedHint`）。
pub const DRAG_DROP_BLOCKED_HINT: &str = "当前以管理员身份运行，Windows 会阻止从资源管理器拖入文件；请改用界面上的按钮选择文件，或以普通权限重新启动。";

/// 拖入 `.ncm` 文件时的提示。
pub const NCM_DROP_MESSAGE: &str = "NCM 文件需要先解密：请使用工具栏的「导入 NCM」处理。";

/// 封面图片文件的大小上限（与在线封面一致）。
pub const MAX_COVER_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// 打开编辑器时需要恢复的内容。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorStateView {
    /// 上次使用的在线搜索来源。
    pub search_source: LyricsSourceKind,
    /// 上次选的歌词格式：逐字优先。
    pub prefer_word_lyrics: bool,
    /// 匹配度阈值（只显示，不在编辑器里改）。
    pub threshold: MatchThresholdView,
    /// 批量获取窗口显示的标签 / 封面来源优先级文案（来自设置，如“网易云音乐优先”）。
    pub tag_priority_text: &'static str,
    /// 批量获取窗口显示的歌词来源优先级文案。
    pub lyrics_priority_text: &'static str,
    /// 是否以管理员身份运行（拖放会被系统阻断）。
    pub is_elevated: bool,
    /// 提权时给用户看的说明。
    pub drag_drop_blocked_hint: &'static str,
    /// 受支持的扩展名（不带点），文件选择器与拖放共用。
    pub extensions: Vec<&'static str>,
    /// 给用户看的一行格式说明。
    pub extension_hint: &'static str,
}

impl EditorStateView {
    /// 由当前设置构造。
    pub fn new(settings: &AppSettings, is_elevated: bool) -> Self {
        Self {
            search_source: settings.lyrics_editor_search_source,
            prefer_word_lyrics: settings.lyrics_editor_prefer_word_lyrics,
            threshold: MatchThresholdView::new(settings.match_threshold),
            tag_priority_text: priority_label(settings.tag_source_priority),
            lyrics_priority_text: priority_label(settings.lyrics_source_priority),
            is_elevated,
            drag_drop_blocked_hint: DRAG_DROP_BLOCKED_HINT,
            extensions: SUPPORTED_EXTENSIONS
                .iter()
                .map(|ext| ext.trim_start_matches('.'))
                .collect(),
            extension_hint: EXTENSION_HINT,
        }
    }
}

/// 来源优先级的界面文案（同 C# `BatchLyricsViewModel.PriorityLabel`）。
pub fn priority_label(priority: OnlineSourcePriority) -> &'static str {
    match priority {
        OnlineSourcePriority::QqMusicFirst => "QQ 音乐优先",
        OnlineSourcePriority::NeteaseOnly => "仅网易云音乐",
        OnlineSourcePriority::QqMusicOnly => "仅 QQ 音乐",
        OnlineSourcePriority::NeteaseFirst => "网易云音乐优先",
    }
}

/// 把编辑器的在线搜索来源与歌词格式写回设置对象（调用方负责保存）；**只改这两个字段**，其余设置原样保留。
/// 返回是否真的有变化（没变化就不必写盘）。
pub fn remember_editor_options(
    settings: &mut AppSettings,
    source: LyricsSourceKind,
    prefer_word_lyrics: bool,
) -> bool {
    if settings.lyrics_editor_search_source == source
        && settings.lyrics_editor_prefer_word_lyrics == prefer_word_lyrics
    {
        return false;
    }
    settings.lyrics_editor_search_source = source;
    settings.lyrics_editor_prefer_word_lyrics = prefer_word_lyrics;
    true
}

/// 拖入结果的类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DropKind {
    /// 解析出了可编辑的音频文件（`path`）。
    Audio,
    /// 没有可编辑的音频，但拖入了 `.ncm`（需要先解密）。
    Ncm,
    /// 没有可用目标（`message` 说明原因）。
    Error,
}

/// 拖入的文件 / 文件夹解析结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DropResolution {
    /// 类别。
    pub kind: DropKind,
    /// `Audio` 时，要载入的第一个音频文件的完整路径。
    pub path: Option<String>,
    /// `Audio` 时是文件名。
    pub file_name: Option<String>,
    /// 给运行日志的一句话：`Audio` 为“拖入文件 / 打开其中的…”，其他是失败原因。
    pub message: String,
    /// 能解析成音频文件的目标个数（编辑器一次只载入一个，多出来的在状态栏说明）。
    pub resolved_count: usize,
    /// 拖入的目标总数。
    pub total: usize,
}

/// 把拖入的路径（文件或文件夹）解析成要载入的音频文件。
///
/// 规则同 C#：文件夹取按名称排序的第一个受支持文件；拖入多个时只取第一个可用的；
/// 一个可用的都没有时，如果拖入了 `.ncm` 就提示改用「导入 NCM」，否则给出第一个失败原因。
pub fn resolve_drop(targets: &[String]) -> DropResolution {
    let total = targets.len();
    let mut first: Option<PathBuf> = None;
    let mut first_error: Option<String> = None;
    let mut ncm_target = false;
    let mut resolved_count = 0;

    for target in targets {
        if has_ncm_extension(target) {
            ncm_target = true;
        }
        match try_resolve(Some(target)) {
            Ok(path) => {
                resolved_count += 1;
                first.get_or_insert(path);
            }
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }

    if let Some(path) = first {
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let message = if total > 1 {
            format!("拖入 {total} 个目标，打开其中的「{file_name}」")
        } else {
            format!("拖入文件：{}", path.display())
        };
        return DropResolution {
            kind: DropKind::Audio,
            path: Some(path.to_string_lossy().into_owned()),
            file_name: Some(file_name),
            message,
            resolved_count,
            total,
        };
    }

    if total == 0 {
        return failure(DropKind::Error, EMPTY_TARGET_MESSAGE.to_owned(), total);
    }
    if ncm_target {
        return failure(DropKind::Ncm, NCM_DROP_MESSAGE.to_owned(), total);
    }
    let message = first_error.unwrap_or_else(|| EMPTY_TARGET_MESSAGE.to_owned());
    failure(DropKind::Error, message, total)
}

fn failure(kind: DropKind, message: String, total: usize) -> DropResolution {
    DropResolution {
        kind,
        path: None,
        file_name: None,
        message,
        resolved_count: 0,
        total,
    }
}

fn has_ncm_extension(target: &str) -> bool {
    Path::new(target.trim())
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ncm"))
}

/// 启动参数的解析结果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartupRequest {
    /// `--lyrics <文件>` / `--edit <文件>` 指定的、确实存在的音频文件（完整路径）。
    pub file: Option<PathBuf>,
    /// 参数给了但文件不存在时，原样记下用来写日志（C# 静默忽略，这里多提示一句）。
    pub missing: Option<String>,
}

/// 解析命令行参数（第一个是程序自己，会被跳过）：`--lyrics <文件>` 或 `--edit <文件>`，
/// 文件必须存在；多个时取第一个存在的。
pub fn parse_startup_args<I, S>(args: I) -> StartupRequest
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args
        .into_iter()
        .skip(1)
        .map(|arg| arg.as_ref().to_owned())
        .collect();
    let mut request = StartupRequest::default();
    for pair in args.windows(2) {
        if (pair[0] == "--lyrics" || pair[0] == "--edit") && !pair[1].trim().is_empty() {
            let candidate = Path::new(&pair[1]);
            if candidate.is_file() {
                request.file = Some(full_path(candidate));
                request.missing = None;
                return request;
            }
            request.missing.get_or_insert_with(|| pair[1].clone());
        }
    }
    request
}

/// 完整路径（不解析符号链接、不加 `\\?\` 前缀）；失败时原样返回。
fn full_path(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// 读进来的封面图片文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverFile {
    /// 图片 MIME（按文件头判断）。
    pub mime_type: String,
    /// Base64（不带 `data:` 前缀）。
    pub base64: String,
    /// 字节数。
    pub size: usize,
    /// 文件名。
    pub file_name: String,
}

/// 读取用户选的封面图片（PNG / JPEG / GIF / BMP / WebP），按文件头校验，最大 10 MB。失败给中文说明。
pub fn read_cover_file(path: &Path) -> Result<CoverFile, String> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let metadata = std::fs::metadata(path).map_err(|_| format!("找不到图片文件「{name}」。"))?;
    if !metadata.is_file() {
        return Err(format!("「{name}」不是图片文件。"));
    }
    if metadata.len() > MAX_COVER_FILE_BYTES {
        return Err("图片超过 10 MB，请换一张小一点的封面。".to_owned());
    }
    let bytes = std::fs::read(path).map_err(|error| error_text::describe(&error))?;
    if bytes.is_empty() {
        return Err(format!("图片文件「{name}」是空的。"));
    }
    if !is_known_image(&bytes) {
        return Err(format!(
            "「{name}」不是支持的图片格式，请选择 PNG / JPEG / WebP / GIF / BMP 图片。"
        ));
    }
    Ok(CoverFile {
        mime_type: detect_image_mime(&bytes).to_owned(),
        base64: STANDARD.encode(&bytes),
        size: bytes.len(),
        file_name: name,
    })
}

/// 文件头是否是已知的图片格式（`detect_image_mime` 认不出时回落 JPEG，不能直接拿来校验）。
fn is_known_image(data: &[u8]) -> bool {
    data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
        || data.starts_with(&[0xFF, 0xD8, 0xFF])
        || data.starts_with(b"GIF8")
        || data.starts_with(b"BM")
        || (data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP")
}

/// 当前进程是否以管理员身份运行（非 Windows 或检测失败时返回 `false`）。
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // SAFETY: 只读查询当前进程令牌；句柄用完即关，缓冲区大小与结构体一致。
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// 非 Windows 平台没有 UIPI，恒为 `false`。
#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

#[cfg(test)]
mod tests;
