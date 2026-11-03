//! QRC / KRC 共用的 zlib 解压，行为对齐 .NET 8 的 `ZLibStream`。

use std::io::{ErrorKind, Read};

use flate2::read::ZlibDecoder;

/// 解压 zlib 流；数据被截断时返回已解出的部分（可能为空），数据损坏（头 / 块 / 校验和错误）时返回 `None`。
///
/// .NET 8 的 `ZLibStream` 读到截断的输入时不抛异常，只是少给数据；C# 版的 QRC / KRC 因此能从被截断的
/// 响应里捞出前半部分歌词。Rust 版保持同样的宽容，并用 C# 探针生成的截断夹具逐字节核对。
/// 流结束之后的多余字节（例如 QRC 块填充的零）被忽略。
pub(crate) fn inflate_lenient(data: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    match ZlibDecoder::new(data).read_to_end(&mut output) {
        Ok(_) => Some(output),
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => Some(output),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::ZlibEncoder;

    use super::*;

    fn compress(text: &str) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(text.as_bytes()).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn complete_streams_are_inflated_and_trailing_bytes_are_ignored() {
        let mut data = compress("完整的歌词");
        data.extend([0, 0, 0, 0, 0]);

        assert_eq!(inflate_lenient(&data).unwrap(), "完整的歌词".as_bytes());
    }

    #[test]
    fn truncated_streams_return_the_decoded_prefix() {
        let text: String = (0..300)
            .map(|i| format!("第{i}行 歌词{} ", i * 7919 % 1013))
            .collect();
        let data = compress(&text);

        let partial = inflate_lenient(&data[..data.len() / 2]).unwrap();

        assert!(!partial.is_empty());
        assert!(partial.len() < text.len());
        assert!(text.as_bytes().starts_with(&partial));
    }

    #[test]
    fn a_truncated_header_yields_an_empty_result() {
        assert_eq!(inflate_lenient(&[0x78]).unwrap(), Vec::<u8>::new());
        assert_eq!(inflate_lenient(&[]).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn corrupt_streams_are_rejected() {
        assert_eq!(inflate_lenient(&[0x40, 0x47, 0x61, 0x77]), None);

        let mut data = compress("校验和会被检查");
        let last = data.len() - 1;
        data[last] ^= 0x55;
        assert_eq!(inflate_lenient(&data), None);
    }
}
