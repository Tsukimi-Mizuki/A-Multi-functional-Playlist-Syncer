//! 同名歌词文件：`歌曲.flac` 旁边的 `歌曲.lrc`。
//!
//! 不少下载工具与播放器把歌词存成这样的独立文件，音频文件里并没有内嵌歌词。
//! 界面在“文件里没有歌词”时拿它的内容来显示；它不改变 [`AudioTagInfo`](super::AudioTagInfo)
//! 的含义——下载、同步、批量在线获取判断“有没有歌词”时仍然只看音频文件本身。
//!
//! 程序平时**只读不写**这个文件。唯一的例外是「歌词转逐字」（[`write_sidecar_lyrics`]，用户 2026-11-16 的决定）：
//! 歌词本来就在 `.lrc` 里的歌，转换后的逐字歌词写回这个 `.lrc`，写之前把原文件备份到同目录的 `_backup\`。
//! 程序仍然从不删除 `.lrc`。

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// 歌词文件的大小上限：正常的歌词只有几十 KB，超过这个数的多半不是歌词，不读。
pub const MAX_SIDECAR_LYRICS_BYTES: u64 = 2 * 1024 * 1024;

/// 同名歌词文件的内容（camelCase）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarLyrics {
    /// 歌词文件的完整路径。
    pub path: String,
    /// 歌词文件名。
    pub file_name: String,
    /// 歌词原文（已去掉 BOM）。
    pub text: String,
}

/// 音频文件对应的歌词文件路径：同一个目录、同一个主文件名、扩展名 `.lrc`。不检查它是否存在。
pub fn sidecar_lyrics_path(audio_path: &Path) -> PathBuf {
    audio_path.with_extension("lrc")
}

/// 读音频文件旁边的同名 `.lrc`。
///
/// 没有这个文件、它太大、读不出来、不是 UTF-8 / 带 BOM 的 UTF-16，或者内容全是空白时返回 `None`：
/// 歌词文件只是锦上添花，任何问题都不该让“读标签”失败。
pub fn read_sidecar_lyrics(audio_path: &Path) -> Option<SidecarLyrics> {
    let path = sidecar_lyrics_path(audio_path);
    // 音频文件自己就叫 `.lrc` 的情况（不会是音频，但别把它当成自己的歌词）。
    if path == audio_path {
        return None;
    }
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_SIDECAR_LYRICS_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .ok()?
        .take(MAX_SIDECAR_LYRICS_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_SIDECAR_LYRICS_BYTES {
        return None;
    }
    let text = decode_text(&bytes)?;
    if text.trim().is_empty() {
        return None;
    }
    Some(SidecarLyrics {
        file_name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: path.to_string_lossy().into_owned(),
        text,
    })
}

/// 备份目录的名字（放在歌词文件所在的目录下）。和用户自己的歌词清理脚本用的是同一个名字。
pub const SIDECAR_BACKUP_DIRECTORY: &str = "_backup";

/// 把新的歌词写进已有的 `.lrc`（「歌词转逐字」用）。
///
/// - 写之前把原文件复制到同目录的 `_backup\<文件名>`；那里已经有同名备份时不覆盖（留着最早的那一份）。
///   备份失败就不写。
/// - 一律写成 UTF-8；原文件带 BOM（UTF-8 或 UTF-16）时新文件也带 UTF-8 的 BOM，没有就不带。
/// - 原文件用 `\r\n` 换行时新内容也换成 `\r\n`。
/// - 先写临时文件再改名替换，中途失败原文件不变。
///
/// 只改写已经存在的歌词文件：文件不存在时返回错误，不会凭空创建。
pub fn write_sidecar_lyrics(lrc_path: &Path, text: &str) -> std::io::Result<()> {
    let original = fs::read(lrc_path)?;
    let had_bom = original.starts_with(&[0xEF, 0xBB, 0xBF])
        || original.starts_with(&[0xFF, 0xFE])
        || original.starts_with(&[0xFE, 0xFF]);
    let crlf = decode_text(&original).is_some_and(|text| text.contains("\r\n"));

    let directory = lrc_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = lrc_path.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "歌词文件路径没有文件名")
    })?;
    let backup_directory = directory.join(SIDECAR_BACKUP_DIRECTORY);
    fs::create_dir_all(&backup_directory)?;
    let backup = backup_directory.join(file_name);
    if !backup.exists() {
        fs::copy(lrc_path, &backup)?;
    }

    let body = if crlf {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text.to_owned()
    };
    let mut temp_name = file_name.to_os_string();
    temp_name.push(".tmp");
    let temp = directory.join(temp_name);
    let written = (|| {
        let mut file = fs::File::create(&temp)?;
        if had_bom {
            file.write_all(&[0xEF, 0xBB, 0xBF])?;
        }
        file.write_all(body.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, lrc_path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// 按 BOM 认编码：UTF-8（带不带 BOM 都行）与带 BOM 的 UTF-16。其他编码（GBK 等）认不出来，返回 `None`，
/// 宁可不显示也不显示乱码。
fn decode_text(bytes: &[u8]) -> Option<String> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(rest.to_vec()).ok();
    }
    let utf16 = |rest: &[u8], unit: fn([u8; 2]) -> u16| {
        let (pairs, leftover) = rest.as_chunks::<2>();
        if !leftover.is_empty() {
            return None;
        }
        let units: Vec<u16> = pairs.iter().map(|pair| unit(*pair)).collect();
        String::from_utf16(&units).ok()
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, u16::from_be_bytes);
    }
    String::from_utf8(bytes.to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn the_sidecar_shares_the_folder_and_the_stem() {
        assert_eq!(
            sidecar_lyrics_path(Path::new(r"D:\音乐\歌手 - 歌名 (feat. 某人).flac")),
            Path::new(r"D:\音乐\歌手 - 歌名 (feat. 某人).lrc")
        );
        // 主文件名里的点不算扩展名。
        assert_eq!(
            sidecar_lyrics_path(Path::new(r"D:\a\01. 歌名.v2.mp3")),
            Path::new(r"D:\a\01. 歌名.v2.lrc")
        );
    }

    #[test]
    fn reads_utf8_with_and_without_a_bom() {
        let dir = TempDir::new();
        let audio = write(&dir, "歌 - 名.flac", b"fLaC");
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice("[00:01.00]第一句\n[00:02.00]第二句\n".as_bytes());
        let lrc = write(&dir, "歌 - 名.lrc", &with_bom);

        let lyrics = read_sidecar_lyrics(&audio).unwrap();
        assert_eq!(lyrics.text, "[00:01.00]第一句\n[00:02.00]第二句\n");
        assert_eq!(lyrics.file_name, "歌 - 名.lrc");
        assert_eq!(lyrics.path, lrc.to_string_lossy());

        fs::write(&lrc, "[00:01.00]没有 BOM\r\n").unwrap();
        assert_eq!(
            read_sidecar_lyrics(&audio).unwrap().text,
            "[00:01.00]没有 BOM\r\n"
        );
    }

    #[test]
    fn reads_utf16_with_a_bom_in_either_byte_order() {
        let dir = TempDir::new();
        let audio = write(&dir, "a.mp3", b"");
        let text = "[00:01.00]歌词";
        for (bom, unit) in [
            ([0xFF, 0xFE], u16::to_le_bytes as fn(u16) -> [u8; 2]),
            ([0xFE, 0xFF], u16::to_be_bytes as fn(u16) -> [u8; 2]),
        ] {
            let mut bytes = bom.to_vec();
            bytes.extend(text.encode_utf16().flat_map(unit));
            write(&dir, "a.lrc", &bytes);
            assert_eq!(read_sidecar_lyrics(&audio).unwrap().text, text);
        }
    }

    #[test]
    fn missing_blank_oversized_or_undecodable_files_are_ignored() {
        let dir = TempDir::new();
        let audio = write(&dir, "a.flac", b"fLaC");
        assert_eq!(read_sidecar_lyrics(&audio), None, "没有歌词文件");

        let lrc = write(&dir, "a.lrc", b"  \r\n\t\n");
        assert_eq!(read_sidecar_lyrics(&audio), None, "全是空白");

        // GBK 的「歌词」：不是合法的 UTF-8，认不出来就不显示。
        fs::write(&lrc, [0xB8, 0xE8, 0xB4, 0xCA]).unwrap();
        assert_eq!(read_sidecar_lyrics(&audio), None, "认不出编码");

        // 奇数个字节的 UTF-16。
        fs::write(&lrc, [0xFF, 0xFE, 0x41]).unwrap();
        assert_eq!(read_sidecar_lyrics(&audio), None, "残缺的 UTF-16");

        let too_big = vec![b'a'; usize::try_from(MAX_SIDECAR_LYRICS_BYTES).unwrap() + 1];
        fs::write(&lrc, too_big).unwrap();
        assert_eq!(read_sidecar_lyrics(&audio), None, "超过大小上限");

        // 同名的是个文件夹。
        fs::remove_file(&lrc).unwrap();
        fs::create_dir(&lrc).unwrap();
        assert_eq!(read_sidecar_lyrics(&audio), None, "同名的是文件夹");
    }

    #[test]
    fn a_lyrics_file_is_not_its_own_sidecar() {
        let dir = TempDir::new();
        let lrc = write(&dir, "a.lrc", "[00:01.00]歌词".as_bytes());
        assert_eq!(read_sidecar_lyrics(&lrc), None);
    }

    #[test]
    fn serializes_to_camel_case() {
        let json = serde_json::to_value(SidecarLyrics {
            path: r"D:\a.lrc".to_owned(),
            file_name: "a.lrc".to_owned(),
            text: "x".to_owned(),
        })
        .unwrap();
        assert_eq!(json["fileName"], "a.lrc");
        assert_eq!(json["path"], r"D:\a.lrc");
        assert_eq!(json["text"], "x");
    }
}
