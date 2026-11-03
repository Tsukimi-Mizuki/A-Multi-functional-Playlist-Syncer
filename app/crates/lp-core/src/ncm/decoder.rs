//! 网易云 `.ncm` 加密文件解密器。
//!
//! 对应 C# `NcmDecoder` / `INcmDecoder`。文件结构：`CTENFDAM` 魔数 → 2 字节跳过 → AES-128-ECB 加密的 RC4 密钥
//! → 加密的 `music:` JSON 元数据 → CRC / 版本 → 封面帧 → 用 RC4 变种密钥流异或过的 MP3 / FLAC 音频。
//! 算法参考社区的 ncmdump / ncmcrypt 实现。
//!
//! ## 失败的分类
//!
//! [`NcmDecoder::decrypt`] 对“可预期”的失败（目标文件已存在且未允许覆盖、文件头损坏、不是 NCM、没有音频）
//! 返回 `success == false` 的结果而不是 `Err`，方便批量导入逐条展示原因；参数非法、文件不存在、取消、磁盘 IO 错误
//! 仍然是 `Err`。
//!
//! ## 内存与落盘
//!
//! 音频按 32 KB 一块流式解密，整个文件从不读进内存；输出先写到同目录的临时文件，成功后再原子改名为目标文件，
//! 取消 / 失败都会清理临时文件，绝不会留下半截成品。

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use aes::Aes128;
use aes::cipher::{BlockCipherDecrypt, KeyInit};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use super::client_comment::{BASE64, MODIFY_KEY};
use crate::utilities::CancelToken;
use crate::utilities::error_text;

/// 文件头魔数。
const NETEASE_MAGIC: &[u8; 8] = b"CTENFDAM";
/// 解密 RC4 密钥用的 AES 密钥。
const CORE_KEY: [u8; 16] = [
    0x68, 0x7A, 0x48, 0x52, 0x41, 0x6D, 0x73, 0x6F, 0x35, 0x6B, 0x49, 0x6E, 0x62, 0x61, 0x78, 0x57,
];
const META_KEY_PREFIX: &[u8] = b"163 key(Don't modify):";
const MUSIC_PREFIX: &[u8] = b"music:";
/// 密钥数据里需要丢掉的前缀（`neteasecloudmusic`）。
const KEY_PREFIX_LENGTH: usize = 17;

/// 音频分块大小；密钥流在每块内从头开始计数。
const AUDIO_CHUNK_SIZE: usize = 32 * 1024;
const MAX_KEY_LENGTH: i32 = 4 * 1024 * 1024;
const MAX_METADATA_LENGTH: i32 = 16 * 1024 * 1024;
const MAX_COVER_LENGTH: i32 = 64 * 1024 * 1024;

/// 同一进程内生成临时文件名的序号（C# 用 GUID）。
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// NCM 文件头里的歌曲元数据（来自加密的 `music:` JSON）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NcmMetadata {
    /// 歌名（`musicName`）。
    pub title: Option<String>,
    /// 歌手；多个用 `/` 连接。
    pub artists: Option<String>,
    /// 专辑。
    pub album: Option<String>,
    /// 容器格式（`mp3` / `flac`）。
    pub format: Option<String>,
    /// 码率（bps）。
    pub bitrate: i32,
    /// 时长（毫秒）。
    pub duration_ms: i64,
    /// 网易云歌曲 ID。
    pub music_id: Option<i64>,
    /// 别名 / 副标题，多个用 `/` 连接。
    pub alias: Option<String>,
}

/// 只读文件头得到的信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NcmFileInfo {
    /// 源文件路径。
    pub source_path: PathBuf,
    /// 元数据；缺失或损坏时为 `None`。
    pub metadata: Option<NcmMetadata>,
}

impl NcmFileInfo {
    /// 是否读到了元数据。
    pub fn has_metadata(&self) -> bool {
        self.metadata.is_some()
    }
}

/// 一个 NCM 文件的解密 / 导入结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NcmDecryptResult {
    /// 源 `.ncm` 路径。
    pub source_path: PathBuf,
    /// 输出的音频路径；失败时为 `None`（标签写入失败时仍保留，因为音频已经解密成功）。
    pub output_path: Option<PathBuf>,
    /// 是否成功。
    pub success: bool,
    /// 输出格式（`mp3` / `flac`）。
    pub format: Option<String>,
    /// 文件头里的元数据。
    pub metadata: Option<NcmMetadata>,
    /// 内嵌封面原始字节，供标签层使用。
    pub cover_data: Option<Vec<u8>>,
    /// 失败原因（面向用户的中文）。
    pub error_message: Option<String>,
    /// 解密后自动在线匹配的结果说明（未开启或没有在线服务时为 `None`）。
    pub enrichment_message: Option<String>,
}

impl NcmDecryptResult {
    /// 构造一个失败结果。
    pub fn failure(source_path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self {
            source_path: source_path.into(),
            output_path: None,
            success: false,
            format: None,
            metadata: None,
            cover_data: None,
            error_message: Some(message.into()),
            enrichment_message: None,
        }
    }
}

/// 解密过程中的错误；文案已经是面向用户的中文。
#[derive(Debug, Error)]
pub enum NcmError {
    /// 参数不合法（例如路径为空）。
    #[error("{0}")]
    InvalidArgument(&'static str),
    /// 源文件不存在。
    #[error("找不到要解密的 NCM 文件：{}", .0.display())]
    NotFound(PathBuf),
    /// 文件内容不是合法的 NCM（文件头不匹配、长度字段非法、文件被截断）。
    #[error("{0}")]
    InvalidData(String),
    /// 调用方取消了任务。
    #[error("操作已取消")]
    Cancelled,
    /// 磁盘读写失败（权限、占用、空间不足等）。
    #[error("{}", error_text::describe(.0))]
    Io(#[from] std::io::Error),
}

impl NcmError {
    fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidData(message.into())
    }
}

/// 网易云 `.ncm` 解密器。实现必须同时支持 MP3 与 FLAC 输出。
///
/// 方法是同步的（文件 IO），调用方在异步上下文里用 `spawn_blocking` 包一层。
pub trait NcmDecoder: Send + Sync {
    /// 文件开头是否是 NCM 魔数。文件不存在、读不了都当作“不是”。
    fn is_ncm_file(&self, path: &Path) -> bool;

    /// 只读取文件头与加密元数据，不解密音频。
    fn read_info(&self, ncm_path: &Path) -> Result<NcmFileInfo, NcmError>;

    /// 解密到 `output_directory`（不存在则创建），按内容自动识别 `mp3` / `flac` 扩展名。
    /// 不写标签；内嵌封面通过 [`NcmDecryptResult::cover_data`] 返回给标签层。
    fn decrypt(
        &self,
        ncm_path: &Path,
        output_directory: &Path,
        overwrite: bool,
        cancel: &CancelToken,
    ) -> Result<NcmDecryptResult, NcmError>;
}

/// 生产实现：直接读写磁盘上的文件。
#[derive(Debug, Default, Clone, Copy)]
pub struct FileNcmDecoder;

impl NcmDecoder for FileNcmDecoder {
    fn is_ncm_file(&self, path: &Path) -> bool {
        if path.as_os_str().to_string_lossy().trim().is_empty() {
            return false;
        }
        let Ok(mut file) = File::open(path) else {
            return false;
        };
        let mut header = [0u8; 8];
        file.read_exact(&mut header).is_ok() && &header == NETEASE_MAGIC
    }

    fn read_info(&self, ncm_path: &Path) -> Result<NcmFileInfo, NcmError> {
        validate_input_path(ncm_path)?;
        let mut input = open_input(ncm_path)?;
        let header = parse_header(&mut input, false)?;
        Ok(NcmFileInfo {
            source_path: ncm_path.to_path_buf(),
            metadata: header.metadata,
        })
    }

    fn decrypt(
        &self,
        ncm_path: &Path,
        output_directory: &Path,
        overwrite: bool,
        cancel: &CancelToken,
    ) -> Result<NcmDecryptResult, NcmError> {
        validate_input_path(ncm_path)?;
        if output_directory
            .as_os_str()
            .to_string_lossy()
            .trim()
            .is_empty()
        {
            return Err(NcmError::InvalidArgument("输出目录不能为空。"));
        }
        check_cancelled(cancel)?;

        let mut input = open_input(ncm_path)?;
        let full_output_directory = std::path::absolute(output_directory)
            .map_err(|_| NcmError::InvalidArgument("输出目录路径无效。"))?;
        fs::create_dir_all(&full_output_directory)?;

        let header = match parse_header(&mut input, true) {
            Ok(header) => header,
            Err(NcmError::InvalidData(message)) => {
                return Ok(NcmDecryptResult::failure(ncm_path, message));
            }
            Err(other) => return Err(other),
        };

        // 音频解扰：每 32 KB 一块，块内下标从头开始。
        let mut chunk = vec![0u8; AUDIO_CHUNK_SIZE];
        let first_read = read_chunk(&mut input.reader, &mut chunk, cancel)?;
        if first_read == 0 {
            return Ok(NcmDecryptResult::failure(
                ncm_path,
                "NCM 文件中没有音频数据。",
            ));
        }

        decrypt_chunk(&mut chunk[..first_read], &header.key_box);
        let format = detect_format(&chunk[..first_read]);
        let output_path = full_output_directory.join(build_output_file_name(ncm_path, format));

        if !overwrite && output_path.exists() {
            return Ok(NcmDecryptResult::failure(
                ncm_path,
                format!("目标文件已存在，未启用覆盖：{}", output_path.display()),
            ));
        }

        let temp_path = full_output_directory.join(format!(
            ".{}.{}-{}.ncmtmp",
            safe_stem(ncm_path),
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));

        write_and_commit(
            &temp_path,
            &output_path,
            &chunk[..first_read],
            &mut input.reader,
            &header.key_box,
            cancel,
        )?;

        Ok(NcmDecryptResult {
            source_path: ncm_path.to_path_buf(),
            output_path: Some(output_path),
            success: true,
            format: Some(format.to_owned()),
            metadata: header.metadata,
            cover_data: header.cover_data,
            error_message: None,
            enrichment_message: None,
        })
    }
}

fn validate_input_path(path: &Path) -> Result<(), NcmError> {
    if path.as_os_str().to_string_lossy().trim().is_empty() {
        return Err(NcmError::InvalidArgument("NCM 文件路径不能为空。"));
    }
    Ok(())
}

fn check_cancelled(cancel: &CancelToken) -> Result<(), NcmError> {
    if cancel.is_cancelled() {
        Err(NcmError::Cancelled)
    } else {
        Ok(())
    }
}

fn open_input(path: &Path) -> Result<Input<BufReader<File>>, NcmError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(NcmError::NotFound(path.to_path_buf()));
        }
        Err(error) => return Err(error.into()),
    };
    let len = file.metadata()?.len();
    Ok(Input {
        reader: BufReader::with_capacity(64 * 1024, file),
        len,
        pos: 0,
    })
}

/// 把解密后的音频写进临时文件（先写已经解好的第一块，再流式处理剩余部分），成功后原子改名为目标文件。
/// 任何一步失败（含取消）都会删除临时文件。
fn write_and_commit(
    temp_path: &Path,
    output_path: &Path,
    first_chunk: &[u8],
    source: &mut impl Read,
    key_box: &[u8; 256],
    cancel: &CancelToken,
) -> Result<(), NcmError> {
    let result = write_decrypted(temp_path, first_chunk, source, key_box, cancel)
        .and_then(|()| fs::rename(temp_path, output_path).map_err(NcmError::from));
    if result.is_err() {
        let _ = fs::remove_file(temp_path);
    }
    result
}

fn write_decrypted(
    temp_path: &Path,
    first_chunk: &[u8],
    source: &mut impl Read,
    key_box: &[u8; 256],
    cancel: &CancelToken,
) -> Result<(), NcmError> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)?;
    let mut output = BufWriter::with_capacity(64 * 1024, file);
    output.write_all(first_chunk)?;

    let mut buffer = vec![0u8; AUDIO_CHUNK_SIZE];
    loop {
        let read = read_chunk(source, &mut buffer, cancel)?;
        if read == 0 {
            break;
        }
        decrypt_chunk(&mut buffer[..read], key_box);
        output.write_all(&buffer[..read])?;
    }
    output.flush()?;
    Ok(())
}

/// 带位置记录的输入：文件头解析里的“跳过”需要知道剩余字节数。
struct Input<R> {
    reader: R,
    len: u64,
    pos: u64,
}

impl<R: Read + Seek> Input<R> {
    fn truncated_read() -> NcmError {
        NcmError::invalid("NCM 文件不完整（读取越界）。")
    }

    fn remaining(&self) -> u64 {
        self.len.saturating_sub(self.pos)
    }

    fn read_exact(&mut self, buffer: &mut [u8]) -> Result<(), NcmError> {
        match self.reader.read_exact(buffer) {
            Ok(()) => {
                self.pos += buffer.len() as u64;
                Ok(())
            }
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => Err(Self::truncated_read()),
            Err(error) => Err(error.into()),
        }
    }

    /// 读取 `count` 字节；不预先按声明长度分配内存，长度超过剩余字节时直接判定文件被截断。
    fn read_vec(&mut self, count: usize) -> Result<Vec<u8>, NcmError> {
        if count as u64 > self.remaining() {
            return Err(Self::truncated_read());
        }
        let mut data = Vec::with_capacity(count);
        let read = (&mut self.reader)
            .take(count as u64)
            .read_to_end(&mut data)?;
        if read != count {
            return Err(Self::truncated_read());
        }
        self.pos += count as u64;
        Ok(data)
    }

    fn read_i32(&mut self) -> Result<i32, NcmError> {
        let mut bytes = [0u8; 4];
        self.read_exact(&mut bytes)?;
        Ok(i32::from_le_bytes(bytes))
    }

    fn read_u32(&mut self) -> Result<u32, NcmError> {
        let mut bytes = [0u8; 4];
        self.read_exact(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn skip(&mut self, count: u64, description: &str) -> Result<(), NcmError> {
        if count == 0 {
            return Ok(());
        }
        if count > self.remaining() {
            return Err(NcmError::invalid(format!(
                "NCM 文件不完整（跳过 {description} 时越界）。"
            )));
        }
        self.reader.seek(SeekFrom::Current(count as i64))?;
        self.pos += count;
        Ok(())
    }
}

/// 文件头解析结果。
struct ParsedHeader {
    key_box: [u8; 256],
    metadata: Option<NcmMetadata>,
    cover_data: Option<Vec<u8>>,
}

/// 解析文件头：密钥、加密元数据与封面帧，返回时输入已定位到音频数据起点。
fn parse_header<R: Read + Seek>(
    input: &mut Input<R>,
    read_cover: bool,
) -> Result<ParsedHeader, NcmError> {
    let mut magic = [0u8; 8];
    input.read_exact(&mut magic)?;
    if &magic != NETEASE_MAGIC {
        return Err(NcmError::invalid(
            "不是有效的网易云 NCM 文件（文件头不匹配）。",
        ));
    }

    input.skip(2, "文件头中的保留字段")?;

    let key_length = input.read_i32()?;
    if key_length <= 0 || key_length > MAX_KEY_LENGTH {
        return Err(NcmError::invalid(format!(
            "NCM 密钥长度无效（{key_length}）。"
        )));
    }

    let mut encrypted_key = input.read_vec(key_length as usize)?;
    xor_in_place(&mut encrypted_key, 0x64);
    let key_data = aes_ecb_decrypt(&encrypted_key, &CORE_KEY);
    let key_box = build_key_box(&key_data)?;

    let mut metadata = None;
    let metadata_length = input.read_i32()?;
    if metadata_length > 0 {
        if metadata_length > MAX_METADATA_LENGTH {
            return Err(NcmError::invalid(format!(
                "NCM 元数据长度无效（{metadata_length}）。"
            )));
        }
        let mut encrypted_metadata = input.read_vec(metadata_length as usize)?;
        xor_in_place(&mut encrypted_metadata, 0x63);
        // 元数据缺失 / 损坏绝不能中断音频解密，所以这里只会得到 None。
        metadata = try_parse_metadata(&encrypted_metadata);
    }

    input.skip(5, "CRC 与封面版本号")?;

    let cover_frame_length = input.read_u32()?;
    let image_length = input.read_i32()?;
    if !(0..=MAX_COVER_LENGTH).contains(&image_length) {
        return Err(NcmError::invalid(format!(
            "NCM 封面长度无效（{image_length}）。"
        )));
    }

    let mut cover_data = None;
    if image_length > 0 {
        if read_cover {
            cover_data = Some(input.read_vec(image_length as usize)?);
        } else {
            input.skip(image_length as u64, "封面数据")?;
        }
    }

    let padding = i64::from(cover_frame_length) - i64::from(image_length);
    if padding > 0 {
        input.skip(padding as u64, "封面帧填充数据")?;
    }

    Ok(ParsedHeader {
        key_box,
        metadata,
        cover_data,
    })
}

/// 尽量读满一块；返回实际读到的字节数（0 表示已到文件末尾）。每读一块前检查取消。
fn read_chunk(
    source: &mut impl Read,
    buffer: &mut [u8],
    cancel: &CancelToken,
) -> Result<usize, NcmError> {
    check_cancelled(cancel)?;
    let mut total = 0;
    while total < buffer.len() {
        match source.read(&mut buffer[total..]) {
            Ok(0) => break,
            Ok(read) => total += read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(total)
}

fn xor_in_place(data: &mut [u8], mask: u8) {
    for byte in data {
        *byte ^= mask;
    }
}

/// AES-128-ECB 解密，只处理完整的 16 字节块；再按参考实现的规则去掉 PKCS#7 填充。
fn aes_ecb_decrypt(cipher: &[u8], key: &[u8; 16]) -> Vec<u8> {
    let aes = Aes128::new(key.into());
    let (blocks, _) = cipher.as_chunks::<16>();
    let mut plain = Vec::with_capacity(blocks.len() * 16);
    for chunk in blocks {
        let mut block = aes::Block::from(*chunk);
        aes.decrypt_block(&mut block);
        plain.extend_from_slice(&block);
    }
    strip_pkcs7_padding(&mut plain);
    plain
}

/// 与参考实现一致：最后一个字节在 1..=16 且不超过数据长度时视为填充并去掉，否则原样保留（不校验其余填充字节）。
fn strip_pkcs7_padding(data: &mut Vec<u8>) {
    if let Some(&padding) = data.last() {
        let padding = usize::from(padding);
        if (1..=16).contains(&padding) && padding <= data.len() {
            data.truncate(data.len() - padding);
        }
    }
}

/// 用解密出的 RC4 变种密钥数据构建 256 字节密钥盒。
fn build_key_box(key_data: &[u8]) -> Result<[u8; 256], NcmError> {
    if key_data.len() <= KEY_PREFIX_LENGTH {
        return Err(NcmError::invalid("NCM 密钥数据长度无效。"));
    }
    let key = &key_data[KEY_PREFIX_LENGTH..];

    let mut key_box = [0u8; 256];
    for (index, slot) in key_box.iter_mut().enumerate() {
        *slot = index as u8;
    }

    let mut key_offset = 0;
    let mut last: u8 = 0;
    for index in 0..256 {
        let swap = key_box[index];
        let c = swap.wrapping_add(last).wrapping_add(key[key_offset]);
        key_offset += 1;
        if key_offset >= key.len() {
            key_offset = 0;
        }
        key_box[index] = key_box[usize::from(c)];
        key_box[usize::from(c)] = swap;
        last = c;
    }
    Ok(key_box)
}

/// 一块音频的解扰；块内下标从 0 开始（`j` 从 1 起算）。
fn decrypt_chunk(buffer: &mut [u8], key_box: &[u8; 256]) {
    for (index, byte) in buffer.iter_mut().enumerate() {
        let j = (index + 1) as u8;
        let inner = key_box[usize::from(key_box[usize::from(j)].wrapping_add(j))];
        *byte ^= key_box[usize::from(key_box[usize::from(j)].wrapping_add(inner))];
    }
}

/// 按解密后的第一块判断容器格式。
fn detect_format(first_chunk: &[u8]) -> &'static str {
    if first_chunk.starts_with(b"ID3") {
        return "mp3";
    }
    if first_chunk.starts_with(b"fLaC") {
        return "flac";
    }
    // 没有 ID3v2 头的 MP3 帧同步字（0xFFEx）。
    if first_chunk.len() >= 2 && first_chunk[0] == 0xFF && (first_chunk[1] & 0xE0) == 0xE0 {
        return "mp3";
    }
    "flac"
}

/// 文件名去掉最后一个扩展名（与 .NET `Path.GetFileNameWithoutExtension` 一致：以最后一个 `.` 为界，`.ncm` 的主干为空）。
fn file_stem_net(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match name.rfind('.') {
        Some(index) => name[..index].to_owned(),
        None => name,
    }
}

fn build_output_file_name(ncm_path: &Path, format: &str) -> String {
    let stem = file_stem_net(ncm_path);
    let stem = if stem.trim().is_empty() {
        "decrypted"
    } else {
        stem.as_str()
    };
    format!("{stem}.{format}")
}

/// 临时文件名里用的主干：非法文件名字符换成 `_`。
fn safe_stem(ncm_path: &Path) -> String {
    let stem: String = file_stem_net(ncm_path)
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '"' | '<' | '>' | '|' | ':' | '*' | '?' | '\\' | '/') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if stem.trim().is_empty() {
        "ncm".to_owned()
    } else {
        stem
    }
}

// ───────────────────────────── 元数据 ─────────────────────────────

fn try_parse_metadata(encrypted: &[u8]) -> Option<NcmMetadata> {
    let json = decrypt_metadata(encrypted)?;
    parse_metadata_json(&json)
}

/// 解出 `music:` 之后的 JSON 文本；任何一步不符合预期都返回 `None`。
fn decrypt_metadata(encrypted: &[u8]) -> Option<String> {
    let payload = encrypted.strip_prefix(META_KEY_PREFIX)?;
    if payload.is_empty() {
        return None;
    }
    // .NET 的 `Convert.FromBase64String` 会忽略空白字符；非 ASCII 字节在 .NET 里会变成 `?` 导致解码失败。
    let text: String = payload
        .iter()
        .copied()
        .filter(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .map(|b| if b.is_ascii() { char::from(b) } else { '?' })
        .collect();
    let cipher = base64::Engine::decode(&BASE64, text).ok()?;
    let plain = aes_ecb_decrypt(&cipher, &MODIFY_KEY);
    let json = plain.strip_prefix(MUSIC_PREFIX)?;
    let json = String::from_utf8_lossy(json);
    Some(
        json.trim_end_matches(['\0', ' ', '\r', '\n', '\t'])
            .to_owned(),
    )
}

fn parse_metadata_json(json: &str) -> Option<NcmMetadata> {
    let root: Value = serde_json::from_str(json).ok()?;
    let root = root.as_object()?;
    let field = |name: &str| root.get(name);
    Some(NcmMetadata {
        title: read_string(field("musicName")),
        artists: read_artists(field("artist")),
        album: read_string(field("album")),
        format: read_string(field("format")),
        bitrate: read_i32(field("bitrate")),
        duration_ms: read_i64(field("duration")),
        music_id: Some(read_i64(field("musicId"))).filter(|id| *id > 0),
        alias: read_alias(field("alias")),
    })
}

/// 字符串（数字取其文本），修整后为空则 `None`。
fn read_string(value: Option<&Value>) -> Option<String> {
    let text = match value? {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => return None,
    };
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// `artist` 是数组，元素可以是 `["名字", id]`、纯字符串或 `{ "name": ... }`。
fn read_artists(value: Option<&Value>) -> Option<String> {
    let Value::Array(artists) = value? else {
        return None;
    };
    let names: Vec<String> = artists
        .iter()
        .filter_map(|artist| {
            let name = match artist {
                Value::Array(parts) => parts.first().and_then(Value::as_str),
                Value::String(text) => Some(text.as_str()),
                Value::Object(map) => map.get("name").and_then(Value::as_str),
                _ => None,
            }?;
            let name = name.trim();
            (!name.is_empty()).then(|| name.to_owned())
        })
        .collect();
    (!names.is_empty()).then(|| names.join("/"))
}

fn read_alias(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        Value::Array(items) => {
            let values: Vec<&str> = items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .collect();
            (!values.is_empty()).then(|| values.join("/"))
        }
        _ => None,
    }
}

/// 与 C# `ReadInt32` 一致：整数按 `i32` 夹取，带小数的数字当作 0，数字字符串按整数解析。
fn read_i32(value: Option<&Value>) -> i32 {
    match value {
        Some(Value::Number(number)) => number.as_i64().map_or(0, |n| {
            n.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
        }),
        Some(Value::String(text)) => text.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

/// 与 C# `ReadInt64` 一致：整数直接取，其余数字按双精度截断，数字字符串按整数解析。
fn read_i64(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|f| f as i64))
            .unwrap_or(0),
        Some(Value::String(text)) => text.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

/// 供其他模块在没有元数据时使用的空元数据（对应 C# `NcmMetadata.Empty`）。
impl NcmMetadata {
    /// 全部字段为空。
    pub fn empty() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests;
