//! 生成 Windows 安全文件名，并支持 `{artist}` / `{title}` / `{album}` / `{track}` / `{year}` / `{id}` 占位符。
//!
//! 对应 C# `FileNameSanitizer`。

/// `Path.GetInvalidFileNameChars()` 在 Windows 上的内容（另外所有控制字符也会被替换）。
const INVALID_CHARS: [char; 9] = ['"', '<', '>', '|', ':', '*', '?', '\\', '/'];

/// 默认的文件名最大长度（按 UTF-16 码元计，与 .NET 的 `string.Length` 一致）。
pub const DEFAULT_MAX_LENGTH: usize = 120;

/// 文件名模板的可替换字段。
#[derive(Debug, Clone, Copy, Default)]
pub struct NameFields<'a> {
    /// 歌名。
    pub title: Option<&'a str>,
    /// 歌手。
    pub artist: Option<&'a str>,
    /// 专辑。
    pub album: Option<&'a str>,
    /// 音轨号，0 表示没有。
    pub track_number: u32,
    /// 年份，0 表示没有。
    pub year: u32,
    /// 网易云歌曲 ID。
    pub song_id: i64,
}

/// 把任意文本变成安全的文件名主干：非法字符与控制字符换成 `_`，去掉首尾空白与结尾的点，超长截断。
/// 空白或清理后为空时返回「未知」。
pub fn sanitize(value: Option<&str>, max_length: usize) -> String {
    let Some(value) = value.filter(|v| !v.trim().is_empty()) else {
        return "未知".to_owned();
    };

    let replaced: String = value
        .chars()
        .map(|c| {
            if INVALID_CHARS.contains(&c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let result = replaced.trim().trim_end_matches('.');
    if result.is_empty() {
        return "未知".to_owned();
    }
    if result.encode_utf16().count() <= max_length {
        return result.to_owned();
    }

    // 按 UTF-16 码元数截断；落在代理对中间时丢弃这半个字符（C# 会留下无效的孤立代理项）。
    let mut kept = String::new();
    let mut units = 0;
    for c in result.chars() {
        let width = c.len_utf16();
        if units + width > max_length {
            break;
        }
        units += width;
        kept.push(c);
    }
    kept.trim_end().to_owned()
}

/// 按模板生成带扩展名的文件名。占位符不区分大小写，按 title、artist、album、track、year、id 的顺序依次替换
/// （与 C# 一致：靠前的替换结果里若含后面的占位符，也会被继续替换）。
pub fn build_file_name(template: &str, fields: NameFields<'_>, extension: &str) -> String {
    let template = if template.trim().is_empty() {
        "{artist} - {title}"
    } else {
        template
    };
    let track = if fields.track_number > 0 {
        format!("{:02}", fields.track_number)
    } else {
        String::new()
    };
    let year = if fields.year > 0 {
        fields.year.to_string()
    } else {
        String::new()
    };

    let mut name = replace_ignore_case(template, "{title}", fields.title.unwrap_or("未知歌曲"));
    name = replace_ignore_case(&name, "{artist}", fields.artist.unwrap_or("未知歌手"));
    name = replace_ignore_case(&name, "{album}", fields.album.unwrap_or("未知专辑"));
    name = replace_ignore_case(&name, "{track}", &track);
    name = replace_ignore_case(&name, "{year}", &year);
    name = replace_ignore_case(&name, "{id}", &fields.song_id.to_string());

    let name = sanitize(Some(name.trim()), 180);
    let dot = if extension.starts_with('.') {
        extension.to_owned()
    } else {
        format!(".{extension}")
    };
    if name.to_lowercase().ends_with(&dot.to_lowercase()) {
        name
    } else {
        name + &dot
    }
}

/// 不区分 ASCII 大小写的全量替换（占位符本身都是 ASCII）。
fn replace_ignore_case(text: &str, needle: &str, replacement: &str) -> String {
    let lower_text = text.to_ascii_lowercase();
    let lower_needle = needle.to_ascii_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(found) = lower_text[cursor..].find(&lower_needle) {
        let start = cursor + found;
        result.push_str(&text[cursor..start]);
        result.push_str(replacement);
        cursor = start + needle.len();
    }
    result.push_str(&text[cursor..]);
    result
}

#[cfg(test)]
mod tests;
