//! 测试用的最小合法音频样本（只在测试里编译）。
//!
//! 全部是静音、几 KB 的合成数据，不含任何受版权保护的音频：
//!
//! - MP3 / WAV / M4A 由本文件按格式规范用代码生成（MP3 与 C# 版的 `Mp3Fixture` 逐字节相同）；
//! - FLAC / Ogg Vorbis / Ogg Opus 是真实编码器生成的静音文件，提交在 `tests/fixtures/audio/`
//!   （生成方式见 `tests/fixtures/audio/README.md`）。

/// 1×1 透明 PNG（合法文件头，标签库只保存原始字节）。
pub(crate) const PNG_COVER: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// 另一张封面：在 PNG 后面追加几个字节，用来区分“新旧封面”。
pub(crate) fn png_cover_2() -> Vec<u8> {
    let mut data = PNG_COVER.to_vec();
    data.extend_from_slice(&[0x42, 0x42, 0x42]);
    data
}

/// 最小合法 MP3：MPEG-1 Layer III、128 kbps / 44.1 kHz 的静音帧（每帧 417 字节）。
pub(crate) fn mp3(frame_count: usize) -> Vec<u8> {
    let mut frame = vec![0u8; 144 * 128_000 / 44_100];
    frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
    frame.repeat(frame_count)
}

/// 默认 8 帧（约 0.2 秒、3.3 KB）的 MP3；C# 夹具是 24 帧，这里取短一点方便把样本提交进仓库。
pub(crate) fn mp3_default() -> Vec<u8> {
    mp3(8)
}

/// 最小合法 WAV：PCM 16 位、8 kHz、单声道、800 个零采样点（0.1 秒）。
pub(crate) fn wav() -> Vec<u8> {
    const SAMPLES: u32 = 800;
    let data_len = SAMPLES * 2;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(4 + 8 + 16 + 8 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // 声道
    out.extend_from_slice(&8000u32.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes()); // 字节率
    out.extend_from_slice(&2u16.to_le_bytes()); // 块对齐
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.resize(out.len() + data_len as usize, 0);
    out
}

/// 真实编码器生成的静音 FLAC（8 kHz 单声道 16 位，0.5 秒，99 字节）。
pub(crate) fn flac() -> Vec<u8> {
    include_bytes!("../../tests/fixtures/audio/silence.flac").to_vec()
}

/// 真实编码器生成的静音 Ogg Vorbis（8 kHz 单声道，0.5 秒）。
pub(crate) fn ogg_vorbis() -> Vec<u8> {
    include_bytes!("../../tests/fixtures/audio/silence.ogg").to_vec()
}

/// 真实编码器生成的静音 Ogg Opus（48 kHz 单声道，0.5 秒）。
pub(crate) fn opus() -> Vec<u8> {
    include_bytes!("../../tests/fixtures/audio/silence.opus").to_vec()
}

fn mp4_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 8);
    out.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
}

fn mp4_full_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut payload = vec![0u8; 4]; // version 0 + flags 0
    payload.extend_from_slice(body);
    mp4_box(kind, &payload)
}

const IDENTITY_MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

/// 最小合法 M4A：单音轨 AAC-LC、44.1 kHz 单声道、10 个 AAC 帧（约 0.23 秒），`moov` 在 `mdat` 之前、没有 `udta`。
///
/// 只有容器结构是真实的（足够让标签库读出时长、码率、采样率并改写 `udta/meta/ilst`），
/// 帧数据是占位字节，不保证能解码出声音。
pub(crate) fn m4a() -> Vec<u8> {
    const FRAMES: u32 = 10;
    const FRAME_BYTES: u32 = 8;
    let timescale = 44_100u32;
    let duration = FRAMES * 1024;

    let ftyp = {
        let mut p = Vec::new();
        p.extend_from_slice(b"M4A ");
        p.extend_from_slice(&0u32.to_be_bytes());
        p.extend_from_slice(b"M4A mp42isom");
        mp4_box(b"ftyp", &p)
    };

    let moov_for = |chunk_offset: u32| -> Vec<u8> {
        let mvhd = {
            let mut b = Vec::new();
            b.extend_from_slice(&0u32.to_be_bytes()); // 创建时间
            b.extend_from_slice(&0u32.to_be_bytes()); // 修改时间
            b.extend_from_slice(&timescale.to_be_bytes());
            b.extend_from_slice(&duration.to_be_bytes());
            b.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // 速率 1.0
            b.extend_from_slice(&0x0100u16.to_be_bytes()); // 音量 1.0
            b.extend_from_slice(&[0u8; 10]);
            for v in IDENTITY_MATRIX {
                b.extend_from_slice(&v.to_be_bytes());
            }
            b.extend_from_slice(&[0u8; 24]);
            b.extend_from_slice(&2u32.to_be_bytes()); // 下一个 track id
            mp4_full_box(b"mvhd", &b)
        };
        let tkhd = {
            let mut b = Vec::new();
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&1u32.to_be_bytes()); // track id
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&duration.to_be_bytes());
            b.extend_from_slice(&[0u8; 8]);
            b.extend_from_slice(&[0u8; 4]); // layer + alternate group
            b.extend_from_slice(&0x0100u16.to_be_bytes()); // 音量
            b.extend_from_slice(&[0u8; 2]);
            for v in IDENTITY_MATRIX {
                b.extend_from_slice(&v.to_be_bytes());
            }
            b.extend_from_slice(&[0u8; 8]); // 宽 + 高
            let mut full = vec![0u8, 0, 0, 3]; // version 0，flags = enabled | in movie
            full.extend_from_slice(&b);
            mp4_box(b"tkhd", &full)
        };
        let mdhd = {
            let mut b = Vec::new();
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&timescale.to_be_bytes());
            b.extend_from_slice(&duration.to_be_bytes());
            b.extend_from_slice(&0x55C4u16.to_be_bytes()); // 语言 und
            b.extend_from_slice(&0u16.to_be_bytes());
            mp4_full_box(b"mdhd", &b)
        };
        let hdlr = {
            let mut b = Vec::new();
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(b"soun");
            b.extend_from_slice(&[0u8; 12]);
            b.extend_from_slice(b"SoundHandler\0");
            mp4_full_box(b"hdlr", &b)
        };
        let smhd = mp4_full_box(b"smhd", &[0u8; 4]);
        let dinf = {
            let url = mp4_box(b"url ", &[0, 0, 0, 1]);
            let mut dref = 1u32.to_be_bytes().to_vec();
            dref.extend_from_slice(&url);
            mp4_box(b"dinf", &mp4_full_box(b"dref", &dref))
        };
        let esds = {
            // ES_Descriptor > DecoderConfigDescriptor > DecoderSpecificInfo（AAC-LC，44.1 kHz，单声道）。
            let mut dec_config = vec![0x40, 0x15, 0x00, 0x00, 0x00];
            dec_config.extend_from_slice(&128_000u32.to_be_bytes()); // 最大码率
            dec_config.extend_from_slice(&128_000u32.to_be_bytes()); // 平均码率
            dec_config.extend_from_slice(&[0x05, 0x02, 0x12, 0x08]);
            let mut es = vec![0x00, 0x01, 0x00]; // ES_ID + flags
            es.push(0x04);
            es.push(dec_config.len() as u8);
            es.extend_from_slice(&dec_config);
            es.extend_from_slice(&[0x06, 0x01, 0x02]); // SLConfigDescriptor
            let mut b = vec![0x03, es.len() as u8];
            b.extend_from_slice(&es);
            mp4_full_box(b"esds", &b)
        };
        let stsd = {
            let mut entry = vec![0u8; 6];
            entry.extend_from_slice(&1u16.to_be_bytes()); // data reference index
            entry.extend_from_slice(&[0u8; 8]);
            entry.extend_from_slice(&1u16.to_be_bytes()); // 声道
            entry.extend_from_slice(&16u16.to_be_bytes()); // 采样位宽
            entry.extend_from_slice(&[0u8; 4]);
            entry.extend_from_slice(&(timescale << 16).to_be_bytes());
            entry.extend_from_slice(&esds);
            let mut b = 1u32.to_be_bytes().to_vec();
            b.extend_from_slice(&mp4_box(b"mp4a", &entry));
            mp4_full_box(b"stsd", &b)
        };
        let stts = {
            let mut b = 1u32.to_be_bytes().to_vec();
            b.extend_from_slice(&FRAMES.to_be_bytes());
            b.extend_from_slice(&1024u32.to_be_bytes());
            mp4_full_box(b"stts", &b)
        };
        let stsc = {
            let mut b = 1u32.to_be_bytes().to_vec();
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&FRAMES.to_be_bytes());
            b.extend_from_slice(&1u32.to_be_bytes());
            mp4_full_box(b"stsc", &b)
        };
        let stsz = {
            let mut b = 0u32.to_be_bytes().to_vec();
            b.extend_from_slice(&FRAMES.to_be_bytes());
            for _ in 0..FRAMES {
                b.extend_from_slice(&FRAME_BYTES.to_be_bytes());
            }
            mp4_full_box(b"stsz", &b)
        };
        let stco = {
            let mut b = 1u32.to_be_bytes().to_vec();
            b.extend_from_slice(&chunk_offset.to_be_bytes());
            mp4_full_box(b"stco", &b)
        };
        let stbl = mp4_box(b"stbl", &[stsd, stts, stsc, stsz, stco].concat());
        let minf = mp4_box(b"minf", &[smhd, dinf, stbl].concat());
        let mdia = mp4_box(b"mdia", &[mdhd, hdlr, minf].concat());
        let trak = mp4_box(b"trak", &[tkhd, mdia].concat());
        mp4_box(b"moov", &[mvhd, trak].concat())
    };

    // `stco` 的偏移与 `moov` 的长度有关，但字段宽度固定，所以先用 0 量一次长度。
    let moov_len = moov_for(0).len() as u32;
    let chunk_offset = ftyp.len() as u32 + moov_len + 8;
    let moov = moov_for(chunk_offset);
    let mut mdat_payload = Vec::new();
    for _ in 0..FRAMES {
        mdat_payload.extend_from_slice(&[0x01, 0x40, 0x22, 0x80, 0xA3, 0x5E, 0xE6, 0x80]);
    }
    let mdat = mp4_box(b"mdat", &mdat_payload);
    [ftyp, moov, mdat].concat()
}

/// 各容器的样本与对应扩展名，供“每种格式都跑一遍”的测试使用。
pub(crate) fn all_formats() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("mp3", mp3_default()),
        ("flac", flac()),
        ("m4a", m4a()),
        ("ogg", ogg_vorbis()),
        ("opus", opus()),
        ("wav", wav()),
    ]
}

#[test]
fn fixtures_have_expected_container_markers() {
    assert_eq!(mp3_default().len(), 417 * 8);
    assert_eq!(&wav()[..4], b"RIFF");
    assert_eq!(&flac()[..4], b"fLaC");
    assert_eq!(&ogg_vorbis()[..4], b"OggS");
    assert_eq!(&opus()[..4], b"OggS");
    assert_eq!(&m4a()[4..8], b"ftyp");
    assert_eq!(&PNG_COVER[1..4], b"PNG");
}

/// 把全部样本写到 `LP_EXPORT_SAMPLES_DIR`，供跨实现探针（C# TagLibSharp）使用；没设变量时什么也不做。
#[test]
fn export_samples_when_requested() {
    let Some(dir) = std::env::var_os("LP_EXPORT_SAMPLES_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (ext, bytes) in all_formats() {
        std::fs::write(dir.join(format!("base.{ext}")), bytes).unwrap();
    }
}
