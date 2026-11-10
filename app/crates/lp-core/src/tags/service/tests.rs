use std::fs;
use std::path::{Path, PathBuf};

use lofty::file::TaggedFileExt;
use lofty::id3::v2::Id3v2Tag;
use lofty::picture::PictureType;
use lofty::tag::{Accessor, ItemKey, TagExt, TagType};

use super::*;
use crate::ncm::client_comment;
use crate::tags::fixtures::{self, PNG_COVER, png_cover_2};
use crate::test_util::TempDir;

const PLAIN_LYRICS: &str = "[00:01.00]普通歌词";
const WORD_LYRICS: &str = "[00:01.00]<00:01.00>逐字<00:01.50>歌词";

fn service() -> LoftyTagService {
    LoftyTagService
}

fn create(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn create_mp3(dir: &TempDir, name: &str) -> PathBuf {
    create(dir, name, &fixtures::mp3_default())
}

fn read(path: &Path) -> AudioTagInfo {
    service()
        .read(path)
        .unwrap()
        .unwrap_or_else(|| panic!("读取 {} 应该成功", path.display()))
}

fn lofty_file(path: &Path) -> lofty::file::TaggedFile {
    lofty::read_from_path(path).unwrap()
}

/// 一份“把能写的都写上”的数据，每种容器的往返测试都用它。
fn full_data() -> AudioTagData {
    AudioTagData {
        replace_all: true,
        title: Some("晴天 Sunny".into()),
        artists: vec!["周杰伦".into(), "Jay & 测试".into()],
        album: Some("叶惠美".into()),
        album_artist: Some("周杰伦".into()),
        track_number: Some(3),
        disc_number: Some(1),
        year: Some(2003),
        genre: Some("Pop/Rock".into()),
        composer: Some("周杰伦/方文山".into()),
        lyricist: Some("方文山/黄俊郎".into()),
        comment: Some("原始备注".into()),
        plain_lyrics: Some(PLAIN_LYRICS.into()),
        word_lyrics: Some(WORD_LYRICS.into()),
        netease_id: Some(186016),
        cover_data: Some(PNG_COVER.to_vec()),
        cover_mime_type: Some("image/png".into()),
        ..Default::default()
    }
}

// ───────────────────────── 从 C# AudioTagServiceTests 移植 ─────────────────────────

#[test]
fn write_round_trips_all_fields() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "歌曲 - 测试歌手.mp3");
    let data = AudioTagData {
        replace_all: false,
        track_total: None,
        ..full_data()
    };

    service().write(&path, &data).unwrap();

    let info = read(&path);
    assert_eq!(info.title.as_deref(), Some("晴天 Sunny"));
    assert_eq!(info.artist.as_deref(), Some("周杰伦/Jay & 测试"));
    assert_eq!(info.album.as_deref(), Some("叶惠美"));
    assert_eq!(info.album_artist.as_deref(), Some("周杰伦"));
    assert_eq!(info.lyrics.as_deref(), Some(WORD_LYRICS));
    assert!(info.has_cover);
    assert_eq!(info.netease_id, Some(186016));
    assert!(info.duration_ms > 0);
    assert!(info.bitrate > 0);

    let tagged = lofty_file(&path);
    let id3 = tagged.tag(TagType::Id3v2).expect("应写入 ID3v2 标签");
    assert_eq!(id3.track(), Some(3));
    assert_eq!(id3.disk(), Some(1));
    assert_eq!(id3.title().as_deref(), Some("晴天 Sunny"));
    assert_eq!(id3.pictures().len(), 1);
    let picture = &id3.pictures()[0];
    assert_eq!(picture.mime_type().map(|m| m.as_str()), Some("image/png"));
    assert_eq!(picture.pic_type(), PictureType::CoverFront);
    assert_eq!(picture.data(), PNG_COVER);
    let comment = id3.comment().unwrap().into_owned();
    assert!(comment.contains("LunaticPrincess:186016"), "{comment}");
    assert!(comment.contains("原始备注"), "{comment}");
}

#[test]
fn write_prefers_word_lyrics_and_falls_back_to_plain_lyrics() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "lyrics.mp3");
    let write = |word: Option<&str>, plain: Option<&str>| {
        service()
            .write(
                &path,
                &AudioTagData {
                    word_lyrics: word.map(str::to_owned),
                    plain_lyrics: plain.map(str::to_owned),
                    ..Default::default()
                },
            )
            .unwrap();
        read(&path).lyrics
    };

    assert_eq!(
        write(Some("word-a"), Some("plain-a")).as_deref(),
        Some("word-a")
    );
    assert_eq!(
        write(Some("  "), Some("plain-b")).as_deref(),
        Some("plain-b")
    );
    assert_eq!(write(None, Some("plain-c")).as_deref(), Some("plain-c"));
}

#[test]
fn write_preserves_existing_comment_and_uses_marker_without_duplicates() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "comment.mp3");

    service()
        .write(
            &path,
            &AudioTagData {
                comment: Some("原始备注".into()),
                netease_id: Some(42),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(read(&path).netease_id, Some(42));

    service()
        .write(
            &path,
            &AudioTagData {
                title: Some("新标题".into()),
                netease_id: Some(43),
                ..Default::default()
            },
        )
        .unwrap();
    let second = read(&path);
    assert_eq!(second.title.as_deref(), Some("新标题"));
    assert_eq!(second.netease_id, Some(43));
    let tagged = lofty_file(&path);
    let comment = tagged
        .tag(TagType::Id3v2)
        .unwrap()
        .comment()
        .unwrap()
        .into_owned();
    assert!(comment.contains("原始备注"), "{comment}");
    assert!(comment.contains("LunaticPrincess:43"), "{comment}");
    assert!(!comment.contains("LunaticPrincess:42"), "{comment}");

    // 下载管线会用 "LunaticPrincess" 作为 comment 基底，不应产生重复标记词。
    service()
        .write(
            &path,
            &AudioTagData {
                comment: Some("LunaticPrincess".into()),
                netease_id: Some(44),
                ..Default::default()
            },
        )
        .unwrap();
    let tagged = lofty_file(&path);
    assert_eq!(
        tagged.tag(TagType::Id3v2).unwrap().comment().as_deref(),
        Some("LunaticPrincess:44")
    );
    assert_eq!(read(&path).netease_id, Some(44));
}

#[test]
fn write_keeps_existing_cover_lyrics_and_netease_id_when_not_provided() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "preserve.mp3");

    service()
        .write(
            &path,
            &AudioTagData {
                title: Some("第一版".into()),
                cover_data: Some(PNG_COVER.to_vec()),
                cover_mime_type: Some("image/png".into()),
                word_lyrics: Some("word-lyrics".into()),
                netease_id: Some(1001),
                ..Default::default()
            },
        )
        .unwrap();
    service()
        .write(
            &path,
            &AudioTagData {
                title: Some("第二版".into()),
                ..Default::default()
            },
        )
        .unwrap();

    let info = read(&path);
    assert_eq!(info.title.as_deref(), Some("第二版"));
    assert_eq!(info.lyrics.as_deref(), Some("word-lyrics"));
    assert!(info.has_cover);
    assert_eq!(info.netease_id, Some(1001));
}

#[test]
fn write_replaces_cover_when_new_cover_provided() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "cover.mp3");
    let second = png_cover_2();

    for cover in [PNG_COVER.to_vec(), second.clone()] {
        service()
            .write(
                &path,
                &AudioTagData {
                    cover_data: Some(cover),
                    cover_mime_type: Some("image/png".into()),
                    ..Default::default()
                },
            )
            .unwrap();
    }

    let tagged = lofty_file(&path);
    let pictures = tagged.tag(TagType::Id3v2).unwrap().pictures();
    assert_eq!(pictures.len(), 1);
    assert_eq!(pictures[0].data(), second);
}

#[test]
fn blank_paths_are_rejected() {
    assert!(matches!(
        service().write(Path::new(" "), &AudioTagData::default()),
        Err(TagError::InvalidArgument(_))
    ));
    assert!(matches!(
        service().read(Path::new(" ")),
        Err(TagError::InvalidArgument(_))
    ));
}

#[test]
fn write_missing_or_unsupported_file_reports_meaningful_error() {
    let dir = TempDir::new();
    let missing = dir.path().join("missing.mp3");
    let error = service()
        .write(&missing, &AudioTagData::default())
        .unwrap_err();
    assert!(matches!(error, TagError::NotFound(_)), "{error}");

    let unsupported = create(&dir, "notes.txt", b"not audio");
    let error = service()
        .write(
            &unsupported,
            &AudioTagData {
                title: Some("x".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(matches!(error, TagError::Unsupported(_)), "{error}");
    assert!(error.to_string().contains("notes.txt"));
    assert_eq!(fs::read(&unsupported).unwrap(), b"not audio");
}

#[test]
fn read_returns_none_for_missing_corrupt_and_unsupported_files() {
    let dir = TempDir::new();
    assert!(
        service()
            .read(&dir.path().join("missing.mp3"))
            .unwrap()
            .is_none()
    );
    assert!(
        service().read(dir.path()).unwrap().is_none(),
        "目录不是音频文件"
    );

    let corrupt = create(&dir, "corrupt.mp3", &[0x00, 0x01, 0x02]);
    assert!(service().read(&corrupt).unwrap().is_none());

    let unsupported = create(&dir, "lyrics.txt", b"just text");
    assert!(service().read(&unsupported).unwrap().is_none());

    let truncated_flac = create(&dir, "cut.flac", &fixtures::flac()[..20]);
    assert!(service().read(&truncated_flac).unwrap().is_none());
}

#[test]
fn read_untagged_mp3_returns_properties_without_tags() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "plain.mp3");

    let info = read(&path);

    assert_eq!(info.title, None);
    assert_eq!(info.artist, None);
    assert_eq!(info.album, None);
    assert_eq!(info.netease_id, None);
    assert!(!info.has_cover);
    assert!(info.cover_data.is_none());
    assert!(info.duration_ms > 0);
    assert_eq!(info.bitrate, 128);
    assert_eq!(info.file_size, 417 * 8);
    assert_eq!(info.file_format.as_deref(), Some("MP3"));
}

#[test]
fn write_round_trips_extended_tags_and_replace_all_can_clear_fields() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "extended-tags.mp3");

    service()
        .write(
            &path,
            &AudioTagData {
                replace_all: true,
                title: Some("T".into()),
                artists: vec!["A1".into(), "A2".into()],
                album: Some("AL".into()),
                album_artist: Some("AA".into()),
                year: Some(2024),
                track_number: Some(2),
                disc_number: Some(1),
                genre: Some("Pop/Rock".into()),
                composer: Some("C1/C2".into()),
                comment: Some("note".into()),
                plain_lyrics: Some("[00:01.00]line".into()),
                ..Default::default()
            },
        )
        .unwrap();

    let info = read(&path);
    assert_eq!(info.title.as_deref(), Some("T"));
    assert_eq!(info.artist.as_deref(), Some("A1/A2"));
    assert_eq!(info.album.as_deref(), Some("AL"));
    assert_eq!(info.album_artist.as_deref(), Some("AA"));
    assert_eq!(info.year, Some(2024));
    assert_eq!(info.track_number, Some(2));
    assert_eq!(info.disc_number, Some(1));
    assert_eq!(info.genre.as_deref(), Some("Pop/Rock"));
    assert_eq!(info.composer.as_deref(), Some("C1/C2"));
    assert_eq!(info.comment.as_deref(), Some("note"));
    assert_eq!(info.lyrics.as_deref(), Some("[00:01.00]line"));
    assert!(info.file_size > 0);
    assert_eq!(info.file_format.as_deref(), Some("MP3"));

    service()
        .write(
            &path,
            &AudioTagData {
                replace_all: true,
                title: Some("Only title".into()),
                plain_lyrics: Some("new lyrics".into()),
                ..Default::default()
            },
        )
        .unwrap();

    let cleared = read(&path);
    assert_eq!(cleared.title.as_deref(), Some("Only title"));
    assert_eq!(cleared.artist, None);
    assert_eq!(cleared.album, None);
    assert_eq!(cleared.album_artist, None);
    assert_eq!(cleared.year, None);
    assert_eq!(cleared.track_number, None);
    assert_eq!(cleared.disc_number, None);
    assert_eq!(cleared.genre, None);
    assert_eq!(cleared.composer, None);
    assert_eq!(cleared.comment, None);
    assert_eq!(cleared.lyrics.as_deref(), Some("new lyrics"));
}

#[test]
fn read_recovers_netease_id_from_client_comment() {
    let json = r#"{"musicId":33894312,"musicName":"晴天","artist":[["周杰伦",6452]],"album":"叶惠美","format":"mp3"}"#;
    let dir = TempDir::new();
    let path = create_mp3(&dir, "client.mp3");
    service()
        .write(
            &path,
            &AudioTagData {
                title: Some("晴天".into()),
                comment: Some(client_comment::build_comment(json)),
                replace_all: true,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(read(&path).netease_id, Some(33894312));
}

// ───────────────────────── 各容器往返 ─────────────────────────

fn assert_full_round_trip(ext: &str, info: &AudioTagInfo) {
    assert_eq!(info.title.as_deref(), Some("晴天 Sunny"), "{ext}");
    assert_eq!(info.artist.as_deref(), Some("周杰伦/Jay & 测试"), "{ext}");
    assert_eq!(info.album.as_deref(), Some("叶惠美"), "{ext}");
    assert_eq!(info.album_artist.as_deref(), Some("周杰伦"), "{ext}");
    assert_eq!(info.year, Some(2003), "{ext}");
    assert_eq!(info.genre.as_deref(), Some("Pop/Rock"), "{ext}");
    assert_eq!(info.composer.as_deref(), Some("周杰伦/方文山"), "{ext}");
    assert_eq!(info.lyricist.as_deref(), Some("方文山/黄俊郎"), "{ext}");
    assert_eq!(info.comment.as_deref(), Some("原始备注"), "{ext}");
    assert_eq!(info.lyrics.as_deref(), Some(WORD_LYRICS), "{ext}");
    assert_eq!(info.track_number, Some(3), "{ext}");
    assert_eq!(info.disc_number, Some(1), "{ext}");
    assert!(info.has_cover, "{ext}");
    assert_eq!(info.cover_data.as_deref(), Some(&PNG_COVER[..]), "{ext}");
    assert_eq!(info.cover_mime_type.as_deref(), Some("image/png"), "{ext}");
    // 写入的封面都是正面封面；MP4 的封面没有类型。
    let picture_type = if ext == "m4a" {
        "封面"
    } else {
        "正面封面"
    };
    assert_eq!(
        info.cover_picture_type.as_deref(),
        Some(picture_type),
        "{ext}"
    );
    assert_eq!(info.file_format, Some(ext.to_uppercase()), "{ext}");
    assert!(info.duration_ms > 0, "{ext}");
}

#[test]
fn every_container_round_trips_all_fields() {
    for (ext, bytes) in fixtures::all_formats() {
        let dir = TempDir::new();
        let path = create(&dir, &format!("full.{ext}"), &bytes);
        service()
            .write(&path, &full_data())
            .unwrap_or_else(|e| panic!("{ext}: {e}"));

        let info = read(&path);
        // 注释显示时已去掉 LunaticPrincess 标记；WAV 的 RIFF INFO 没有歌词 / 封面，由 ID3v2 兜底。
        assert_full_round_trip(ext, &info);
        // 网易云 ID：除 M4A（见下一个测试）外所有容器都能读回。
        assert_eq!(info.netease_id, Some(186016), "{ext}");
    }
}

#[test]
fn replace_all_clears_fields_in_every_container() {
    for (ext, bytes) in fixtures::all_formats() {
        let dir = TempDir::new();
        let path = create(&dir, &format!("clear.{ext}"), &bytes);
        service().write(&path, &full_data()).unwrap();

        service()
            .write(
                &path,
                &AudioTagData {
                    replace_all: true,
                    title: Some("Only".into()),
                    ..Default::default()
                },
            )
            .unwrap();

        let info = read(&path);
        assert_eq!(info.title.as_deref(), Some("Only"), "{ext}");
        assert_eq!(info.artist, None, "{ext}");
        assert_eq!(info.album, None, "{ext}");
        assert_eq!(info.album_artist, None, "{ext}");
        assert_eq!(info.year, None, "{ext}");
        assert_eq!(info.track_number, None, "{ext}");
        assert_eq!(info.disc_number, None, "{ext}");
        assert_eq!(info.genre, None, "{ext}");
        assert_eq!(info.composer, None, "{ext}");
        assert_eq!(info.lyricist, None, "{ext}");
        assert_eq!(info.comment, None, "{ext}");
        assert_eq!(info.lyrics, None, "{ext}");
        assert_eq!(
            info.netease_id, None,
            "{ext}: replace_all 没带 ID 就不保留标记"
        );
        // 封面没有提供，所以保留。
        assert!(info.has_cover, "{ext}");
    }
}

#[test]
fn partial_write_keeps_other_fields_in_every_container() {
    for (ext, bytes) in fixtures::all_formats() {
        let dir = TempDir::new();
        let path = create(&dir, &format!("keep.{ext}"), &bytes);
        service().write(&path, &full_data()).unwrap();

        service()
            .write(
                &path,
                &AudioTagData {
                    title: Some("新标题".into()),
                    ..Default::default()
                },
            )
            .unwrap();

        let info = read(&path);
        assert_eq!(info.title.as_deref(), Some("新标题"), "{ext}");
        assert_eq!(info.artist.as_deref(), Some("周杰伦/Jay & 测试"), "{ext}");
        assert_eq!(info.album.as_deref(), Some("叶惠美"), "{ext}");
        assert_eq!(info.year, Some(2003), "{ext}");
        assert_eq!(info.track_number, Some(3), "{ext}");
        assert_eq!(info.comment.as_deref(), Some("原始备注"), "{ext}");
        assert_eq!(info.lyrics.as_deref(), Some(WORD_LYRICS), "{ext}");
        assert_eq!(info.netease_id, Some(186016), "{ext}");
        assert_eq!(info.cover_data.as_deref(), Some(&PNG_COVER[..]), "{ext}");
    }
}

#[test]
fn remove_cover_deletes_every_picture_but_leaves_other_fields_alone() {
    for (ext, bytes) in fixtures::all_formats() {
        let dir = TempDir::new();
        let path = create(&dir, &format!("cover.{ext}"), &bytes);
        service()
            .write(
                &path,
                &AudioTagData {
                    title: Some("保留的标题".into()),
                    cover_data: Some(PNG_COVER.to_vec()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(read(&path).has_cover, "{ext}");

        // 编辑器“移除封面”：不带新封面、`remove_cover` 为真。
        service()
            .write(
                &path,
                &AudioTagData {
                    replace_all: true,
                    title: Some("保留的标题".into()),
                    remove_cover: true,
                    ..Default::default()
                },
            )
            .unwrap();

        let info = read(&path);
        assert!(!info.has_cover, "{ext}");
        assert!(info.cover_data.is_none(), "{ext}");
        assert_eq!(info.title.as_deref(), Some("保留的标题"), "{ext}");
        let tagged = lofty_file(&path);
        let total: usize = tagged.tags().iter().map(|t| t.pictures().len()).sum();
        assert_eq!(total, 0, "{ext}");
    }
}

#[test]
fn a_new_cover_wins_over_remove_cover() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "both.mp3");
    service()
        .write(
            &path,
            &AudioTagData {
                cover_data: Some(PNG_COVER.to_vec()),
                ..Default::default()
            },
        )
        .unwrap();
    let second = png_cover_2();

    service()
        .write(
            &path,
            &AudioTagData {
                cover_data: Some(second.clone()),
                remove_cover: true,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(read(&path).cover_data.as_deref(), Some(&second[..]));
}

#[test]
fn without_remove_cover_a_missing_cover_leaves_the_existing_one() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "keep.mp3");
    service()
        .write(
            &path,
            &AudioTagData {
                cover_data: Some(PNG_COVER.to_vec()),
                ..Default::default()
            },
        )
        .unwrap();

    // 编辑器保存时封面未改动：`replace_all` 也不能清掉封面。
    service()
        .write(
            &path,
            &AudioTagData {
                replace_all: true,
                title: Some("x".into()),
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(read(&path).cover_data.as_deref(), Some(&PNG_COVER[..]));
}

#[test]
fn cover_is_replaced_not_duplicated_in_every_container() {
    for (ext, bytes) in fixtures::all_formats() {
        let dir = TempDir::new();
        let path = create(&dir, &format!("cover.{ext}"), &bytes);
        let second = png_cover_2();
        for cover in [PNG_COVER.to_vec(), second.clone()] {
            service()
                .write(
                    &path,
                    &AudioTagData {
                        cover_data: Some(cover),
                        ..Default::default()
                    },
                )
                .unwrap();
        }

        let info = read(&path);
        assert_eq!(info.cover_data.as_deref(), Some(&second[..]), "{ext}");
        let tagged = lofty_file(&path);
        let total: usize = tagged.tags().iter().map(|t| t.pictures().len()).sum();
        // WAV 同时有 ID3v2 与 RIFF INFO，图片只会在 ID3v2 里；其余容器都只有一张。
        assert_eq!(total, 1, "{ext}");
    }
}

#[test]
fn wav_writes_both_id3v2_and_riff_info_like_the_csharp_version() {
    let dir = TempDir::new();
    let path = create(&dir, "both.wav", &fixtures::wav());
    service().write(&path, &full_data()).unwrap();

    let tagged = lofty_file(&path);
    let id3 = tagged.tag(TagType::Id3v2).expect("ID3v2");
    let info = tagged.tag(TagType::RiffInfo).expect("RIFF INFO");
    assert_eq!(id3.title().as_deref(), Some("晴天 Sunny"));
    assert_eq!(info.title().as_deref(), Some("晴天 Sunny"));
    assert_eq!(info.album().as_deref(), Some("叶惠美"));
}

#[test]
fn m4a_uses_ilst_and_year_atom() {
    let dir = TempDir::new();
    let path = create(&dir, "atoms.m4a", &fixtures::m4a());
    service().write(&path, &full_data()).unwrap();

    let tagged = lofty_file(&path);
    let ilst = tagged.tag(TagType::Mp4Ilst).expect("ilst");
    assert_eq!(ilst.title().as_deref(), Some("晴天 Sunny"));
    assert_eq!(ilst.get_string(ItemKey::RecordingDate), Some("2003"));
    assert_eq!(ilst.get_string(ItemKey::Lyrics), Some(WORD_LYRICS));
    assert_eq!(ilst.track(), Some(3));
}

// ───────────────────────── ID3v2 细节 ─────────────────────────

fn id3v2_header(path: &Path) -> [u8; 10] {
    let bytes = fs::read(path).unwrap();
    bytes[..10].try_into().unwrap()
}

#[test]
fn new_mp3_tag_uses_id3v23_and_existing_v24_is_kept() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "v23.mp3");
    service().write(&path, &full_data()).unwrap();
    let header = id3v2_header(&path);
    assert_eq!(&header[..3], b"ID3");
    assert_eq!(header[3], 3, "新建标签应是 ID3v2.3");

    // 造一个 ID3v2.4 的文件：用 lofty 直接写。
    let v24 = create_mp3(&dir, "v24.mp3");
    let mut tag = lofty::tag::Tag::new(TagType::Id3v2);
    tag.set_title("旧标题".to_owned());
    tag.save_to_path(&v24, lofty::config::WriteOptions::default())
        .unwrap();
    assert_eq!(id3v2_header(&v24)[3], 4);
    service()
        .write(
            &v24,
            &AudioTagData {
                title: Some("新标题".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(id3v2_header(&v24)[3], 4, "已有 ID3v2.4 应保持");
    assert_eq!(read(&v24).title.as_deref(), Some("新标题"));
}

#[test]
fn replace_all_removes_stale_id3v1_so_cleared_fields_stay_cleared() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "v1.mp3");
    // C# 版保存时会另外写一份 ID3v1：这里造一个带专辑的 ID3v1 尾巴。
    let mut v1 = lofty::tag::Tag::new(TagType::Id3v1);
    v1.set_album("旧专辑".to_owned());
    v1.set_title("旧标题".to_owned());
    v1.save_to_path(&path, lofty::config::WriteOptions::default())
        .unwrap();
    assert!(fs::read(&path).unwrap().windows(3).any(|w| w == b"TAG"));

    service()
        .write(
            &path,
            &AudioTagData {
                replace_all: true,
                title: Some("新标题".into()),
                ..Default::default()
            },
        )
        .unwrap();

    let info = read(&path);
    assert_eq!(info.title.as_deref(), Some("新标题"));
    assert_eq!(info.album, None, "ID3v1 里的旧专辑不应再冒出来");
    assert!(lofty_file(&path).tag(TagType::Id3v1).is_none());
}

#[test]
fn id3v1_only_files_are_readable_as_fallback() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "v1-only.mp3");
    let mut v1 = lofty::tag::Tag::new(TagType::Id3v1);
    v1.set_title("Legacy".to_owned());
    v1.set_artist("Old Artist".to_owned());
    v1.save_to_path(&path, lofty::config::WriteOptions::default())
        .unwrap();

    let info = read(&path);
    assert_eq!(info.title.as_deref(), Some("Legacy"));
    assert_eq!(info.artist.as_deref(), Some("Old Artist"));
}

#[test]
fn unshown_id3v2_frames_survive_a_write() {
    use lofty::id3::v2::{Frame, UniqueFileIdentifierFrame};

    let dir = TempDir::new();
    let path = create_mp3(&dir, "extra.mp3");
    let mut tag = Id3v2Tag::new();
    tag.set_title("原标题".to_owned());
    tag.insert(Frame::UniqueFileIdentifier(UniqueFileIdentifierFrame::new(
        "http://example.org/owner".to_owned(),
        b"ABC123".to_vec(),
    )));
    tag.insert(Frame::UserText(lofty::id3::v2::ExtendedTextFrame::new(
        lofty::TextEncoding::UTF8,
        "MY_CUSTOM".to_owned(),
        "自定义值".to_owned(),
    )));
    tag.save_to_path(&path, lofty::config::WriteOptions::default())
        .unwrap();

    service().write(&path, &full_data()).unwrap();

    let text = fs::read(&path).unwrap();
    let contains = |needle: &[u8]| text.windows(needle.len()).any(|w| w == needle);
    assert!(contains(b"UFID"), "UFID 帧应保留");
    assert!(contains(b"http://example.org/owner"));
    assert!(contains(b"ABC123"));
    let tagged = lofty_file(&path);
    let id3: Id3v2Tag = tagged.tag(TagType::Id3v2).unwrap().clone().into();
    assert_eq!(
        id3.get_user_text("MY_CUSTOM"),
        Some("自定义值"),
        "自定义 TXXX 应保留"
    );
    assert_eq!(id3.title().as_deref(), Some("晴天 Sunny"));
}

#[test]
fn described_comment_frames_are_not_mistaken_for_the_note() {
    use lofty::id3::v2::{CommentFrame, Frame, Id3v2Tag};

    let dir = TempDir::new();
    let path = create_mp3(&dir, "itunnorm.mp3");
    let mut tag = Id3v2Tag::new();
    tag.insert(Frame::Comment(CommentFrame::new(
        lofty::TextEncoding::Latin1,
        *b"eng",
        "iTunNORM".to_owned(),
        " 000012AB 000012AB".to_owned(),
    )));
    tag.save_to_path(&path, lofty::config::WriteOptions::default())
        .unwrap();
    assert_eq!(read(&path).comment, None, "iTunNORM 不是用户备注");

    service()
        .write(
            &path,
            &AudioTagData {
                comment: Some("我的备注".into()),
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(read(&path).comment.as_deref(), Some("我的备注"));
    let bytes = fs::read(&path).unwrap();
    assert!(
        bytes.windows(8).any(|w| w == b"iTunNORM"),
        "带描述的 COMM 帧应保留"
    );
}

// ───────────────────────── 数字、年份、流派 ─────────────────────────

#[test]
fn track_and_disc_totals_are_read_written_and_preserved() {
    for (ext, bytes) in fixtures::all_formats() {
        if ext == "wav" {
            continue; // RIFF INFO 没有总数字段，ID3v2 兜底的行为已由其他测试覆盖。
        }
        let dir = TempDir::new();
        let path = create(&dir, &format!("totals.{ext}"), &bytes);
        service()
            .write(
                &path,
                &AudioTagData {
                    track_number: Some(3),
                    track_total: Some(12),
                    disc_number: Some(1),
                    disc_total: Some(2),
                    ..Default::default()
                },
            )
            .unwrap();
        let info = read(&path);
        assert_eq!(
            (
                info.track_number,
                info.track_total,
                info.disc_number,
                info.disc_total
            ),
            (Some(3), Some(12), Some(1), Some(2)),
            "{ext}"
        );

        // C# 的 AudioTagData 没有总数字段：只改号码时 TagLib 保留已有总数，这里一样。
        service()
            .write(
                &path,
                &AudioTagData {
                    replace_all: true,
                    track_number: Some(5),
                    disc_number: Some(2),
                    ..Default::default()
                },
            )
            .unwrap();
        let info = read(&path);
        assert_eq!(
            (
                info.track_number,
                info.track_total,
                info.disc_number,
                info.disc_total
            ),
            (Some(5), Some(12), Some(2), Some(2)),
            "{ext}"
        );

        // 号码被清除时总数一并清除。
        service()
            .write(
                &path,
                &AudioTagData {
                    replace_all: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let info = read(&path);
        assert_eq!(
            (
                info.track_number,
                info.track_total,
                info.disc_number,
                info.disc_total
            ),
            (None, None, None, None),
            "{ext}"
        );
    }
}

#[test]
fn full_release_date_is_kept_when_the_year_is_unchanged() {
    let dir = TempDir::new();
    let path = create(&dir, "date.flac", &fixtures::flac());
    // 先写一个完整日期。
    let mut vorbis = lofty::tag::Tag::new(TagType::VorbisComments);
    vorbis.insert_text(ItemKey::RecordingDate, "2003-05-21".to_owned());
    vorbis
        .save_to_path(&path, lofty::config::WriteOptions::default())
        .unwrap();
    assert_eq!(read(&path).year, Some(2003));

    service()
        .write(
            &path,
            &AudioTagData {
                replace_all: true,
                year: Some(2003),
                ..Default::default()
            },
        )
        .unwrap();
    let tagged = lofty_file(&path);
    assert_eq!(
        tagged
            .tag(TagType::VorbisComments)
            .unwrap()
            .get_string(ItemKey::RecordingDate),
        Some("2003-05-21")
    );

    service()
        .write(
            &path,
            &AudioTagData {
                replace_all: true,
                year: Some(1999),
                ..Default::default()
            },
        )
        .unwrap();
    let tagged = lofty_file(&path);
    assert_eq!(
        tagged
            .tag(TagType::VorbisComments)
            .unwrap()
            .get_string(ItemKey::RecordingDate),
        Some("1999")
    );
}

#[test]
fn genres_and_composers_split_on_every_separator() {
    let dir = TempDir::new();
    let path = create(&dir, "split.flac", &fixtures::flac());
    service()
        .write(
            &path,
            &AudioTagData {
                genre: Some(" Pop ; Rock／摇滚；民谣\nJazz ".replace('／', "/")),
                composer: Some("A/B;；C".into()),
                artists: vec!["  A  ".into(), "".into(), "B".into()],
                ..Default::default()
            },
        )
        .unwrap();

    let tagged = lofty_file(&path);
    let vorbis = tagged.tag(TagType::VorbisComments).unwrap();
    let genres: Vec<_> = vorbis.get_strings(ItemKey::Genre).collect();
    assert_eq!(genres, ["Pop", "Rock", "摇滚", "民谣", "Jazz"]);
    let composers: Vec<_> = vorbis.get_strings(ItemKey::Composer).collect();
    assert_eq!(composers, ["A", "B", "C"]);
    let artists: Vec<_> = vorbis.get_strings(ItemKey::TrackArtist).collect();
    assert_eq!(artists, ["A", "B"]);
    assert_eq!(read(&path).artist.as_deref(), Some("A/B"));
}

#[test]
fn numeric_id3_genre_references_are_read_as_names() {
    // C# 版（TagLib）把 Pop/Rock 写成 TCON "(13)(17)"，读出来必须是名字。
    let dir = TempDir::new();
    let path = create_mp3(&dir, "genre.mp3");
    let mut tag = lofty::tag::Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::Genre, "(13)(17)".to_owned());
    tag.save_to_path(&path, lofty::config::WriteOptions::default())
        .unwrap();
    assert_eq!(read(&path).genre.as_deref(), Some("Pop/Rock"));
}

// ───────────────────────── 封面 ─────────────────────────

/// 解析 FLAC 的元数据块，返回 `(块类型, 负载)`。
fn flac_blocks(path: &Path) -> Vec<(u8, Vec<u8>)> {
    let bytes = fs::read(path).unwrap();
    assert_eq!(&bytes[..4], b"fLaC");
    let mut pos = 4;
    let mut blocks = Vec::new();
    loop {
        let header = bytes[pos];
        let len = u32::from_be_bytes([0, bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as usize;
        blocks.push((header & 0x7F, bytes[pos + 4..pos + 4 + len].to_vec()));
        pos += 4 + len;
        if header & 0x80 != 0 {
            break;
        }
    }
    blocks
}

#[test]
fn flac_cover_is_written_as_a_native_picture_block_without_duplicates() {
    let dir = TempDir::new();
    let path = create(&dir, "cover.flac", &fixtures::flac());
    service()
        .write(
            &path,
            &AudioTagData {
                title: Some("t".into()),
                cover_data: Some(PNG_COVER.to_vec()),
                ..Default::default()
            },
        )
        .unwrap();

    let blocks = flac_blocks(&path);
    assert_eq!(
        blocks.iter().filter(|(kind, _)| *kind == 6).count(),
        1,
        "应有一个原生 PICTURE 块"
    );
    let comments = blocks
        .iter()
        .find(|(kind, _)| *kind == 4)
        .expect("VorbisComment 块");
    assert!(
        !comments
            .1
            .windows(22)
            .any(|w| w == b"METADATA_BLOCK_PICTURE"),
        "不应再写 METADATA_BLOCK_PICTURE 字段"
    );
    assert_eq!(read(&path).cover_data.as_deref(), Some(&PNG_COVER[..]));
}

#[test]
fn flac_legacy_metadata_block_picture_is_read_and_cleaned_up_on_write() {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    // 手工构造 TagLib 风格的封面：VorbisComment 里的 METADATA_BLOCK_PICTURE（FLAC picture 结构的 Base64）。
    let mut flac_picture = Vec::new();
    flac_picture.extend_from_slice(&3u32.to_be_bytes());
    flac_picture.extend_from_slice(&9u32.to_be_bytes());
    flac_picture.extend_from_slice(b"image/png");
    flac_picture.extend_from_slice(&5u32.to_be_bytes());
    flac_picture.extend_from_slice(b"Cover");
    for field in [0u32, 0, 0, 0, PNG_COVER.len() as u32] {
        flac_picture.extend_from_slice(&field.to_be_bytes());
    }
    flac_picture.extend_from_slice(&PNG_COVER);
    let encoded = STANDARD.encode(&flac_picture);

    let dir = TempDir::new();
    let path = create(&dir, "legacy.flac", &fixtures::flac());
    let mut vorbis = lofty::ogg::tag::VorbisComments::default();
    vorbis.push("TITLE".to_owned(), "旧封面".to_owned());
    vorbis.push("METADATA_BLOCK_PICTURE".to_owned(), encoded);
    // 用 Tag 路径写：lofty 会把 METADATA_BLOCK_PICTURE 字段当作普通文本保存，和 TagLib 的结果一致。
    vorbis
        .save_to_path(&path, lofty::config::WriteOptions::default())
        .unwrap();

    let legacy = read(&path);
    assert_eq!(legacy.title.as_deref(), Some("旧封面"));
    assert!(legacy.has_cover, "兼容读取 METADATA_BLOCK_PICTURE");
    assert_eq!(legacy.cover_data.as_deref(), Some(&PNG_COVER[..]));

    // 换一张新封面：只剩一个原生 PICTURE 块，旧字段被清掉。
    let second = png_cover_2();
    service()
        .write(
            &path,
            &AudioTagData {
                cover_data: Some(second.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    let blocks = flac_blocks(&path);
    assert_eq!(blocks.iter().filter(|(kind, _)| *kind == 6).count(), 1);
    let comments = blocks.iter().find(|(kind, _)| *kind == 4).unwrap();
    assert!(
        !comments
            .1
            .windows(22)
            .any(|w| w == b"METADATA_BLOCK_PICTURE")
    );
    assert_eq!(read(&path).cover_data.as_deref(), Some(&second[..]));
}

#[test]
fn ogg_and_opus_covers_use_metadata_block_picture() {
    for (ext, bytes) in [("ogg", fixtures::ogg_vorbis()), ("opus", fixtures::opus())] {
        let dir = TempDir::new();
        let path = create(&dir, &format!("cover.{ext}"), &bytes);
        service()
            .write(
                &path,
                &AudioTagData {
                    cover_data: Some(PNG_COVER.to_vec()),
                    ..Default::default()
                },
            )
            .unwrap();
        let raw = fs::read(&path).unwrap();
        assert!(
            raw.windows(22).any(|w| w == b"METADATA_BLOCK_PICTURE"),
            "{ext}: Ogg 容器没有原生图片块，封面存在 METADATA_BLOCK_PICTURE 里"
        );
        assert_eq!(
            read(&path).cover_data.as_deref(),
            Some(&PNG_COVER[..]),
            "{ext}"
        );
    }
}

#[test]
fn cover_mime_is_detected_when_not_given() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "mime.mp3");
    let jpeg = [
        0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0,
        0xFF, 0xD9,
    ];
    service()
        .write(
            &path,
            &AudioTagData {
                cover_data: Some(jpeg.to_vec()),
                ..Default::default()
            },
        )
        .unwrap();
    let info = read(&path);
    assert_eq!(info.cover_mime_type.as_deref(), Some("image/jpeg"));
    assert_eq!(info.cover_data.as_deref(), Some(&jpeg[..]));
}

#[test]
fn front_cover_is_preferred_over_the_first_picture() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "two-covers.mp3");
    let mut tag = lofty::tag::Tag::new(TagType::Id3v2);
    tag.push_picture(
        lofty::picture::Picture::unchecked(png_cover_2())
            .pic_type(PictureType::CoverBack)
            .mime_type(lofty::picture::MimeType::Png)
            .build(),
    );
    tag.push_picture(
        lofty::picture::Picture::unchecked(PNG_COVER.to_vec())
            .pic_type(PictureType::CoverFront)
            .mime_type(lofty::picture::MimeType::Png)
            .build(),
    );
    tag.save_to_path(&path, lofty::config::WriteOptions::default())
        .unwrap();

    assert_eq!(read(&path).cover_data.as_deref(), Some(&PNG_COVER[..]));
}

#[test]
fn the_cover_reports_its_picture_type() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "back-only.mp3");
    let mut tag = lofty::tag::Tag::new(TagType::Id3v2);
    tag.push_picture(
        lofty::picture::Picture::unchecked(PNG_COVER.to_vec())
            .pic_type(PictureType::CoverBack)
            .mime_type(lofty::picture::MimeType::Png)
            .build(),
    );
    tag.save_to_path(&path, lofty::config::WriteOptions::default())
        .unwrap();

    let info = read(&path);
    assert_eq!(info.cover_data.as_deref(), Some(&PNG_COVER[..]));
    assert_eq!(info.cover_picture_type.as_deref(), Some("背面封面"));
}

#[test]
fn picture_types_have_chinese_names() {
    assert_eq!(
        picture_type_label(TagType::Id3v2, PictureType::CoverFront),
        "正面封面"
    );
    assert_eq!(
        picture_type_label(TagType::VorbisComments, PictureType::Media),
        "碟面"
    );
    assert_eq!(
        picture_type_label(TagType::Id3v2, PictureType::Other),
        "其他"
    );
    assert_eq!(
        picture_type_label(TagType::Id3v2, PictureType::Undefined(42)),
        "其他"
    );
    // MP4 的封面没有类型（lofty 一律读成 Other）。
    assert_eq!(
        picture_type_label(TagType::Mp4Ilst, PictureType::Other),
        "封面"
    );
}

// ───────────────────────── 未展示字段与写入安全 ─────────────────────────

#[test]
fn unshown_vorbis_fields_survive_a_replace_all_write() {
    let dir = TempDir::new();
    let path = create(&dir, "extra.flac", &fixtures::flac());
    let mut vorbis = lofty::ogg::tag::VorbisComments::default();
    vorbis.push("TITLE".to_owned(), "旧".to_owned());
    vorbis.push("REPLAYGAIN_TRACK_GAIN".to_owned(), "-6.50 dB".to_owned());
    vorbis.push("MY_CUSTOM_FIELD".to_owned(), "保留我".to_owned());
    vorbis.push("ISRC".to_owned(), "CNA001234567".to_owned());
    vorbis
        .save_to_path(&path, lofty::config::WriteOptions::default())
        .unwrap();

    service()
        .write(
            &path,
            &AudioTagData {
                replace_all: true,
                title: Some("新".into()),
                ..Default::default()
            },
        )
        .unwrap();

    let tagged = lofty_file(&path);
    let tag = tagged.tag(TagType::VorbisComments).unwrap();
    assert_eq!(tag.title().as_deref(), Some("新"));
    assert_eq!(
        tag.get_string(ItemKey::ReplayGainTrackGain),
        Some("-6.50 dB")
    );
    assert_eq!(tag.get_string(ItemKey::Isrc), Some("CNA001234567"));
    let raw = fs::read(&path).unwrap();
    assert!(
        raw.windows(15).any(|w| w == b"MY_CUSTOM_FIELD"),
        "未知字段应原样保留"
    );
}

#[test]
fn unshown_m4a_atoms_survive_a_write() {
    use lofty::mp4::{Atom, AtomData, AtomIdent, Ilst};

    let dir = TempDir::new();
    let path = create(&dir, "extra.m4a", &fixtures::m4a());
    let mut ilst = Ilst::new();
    ilst.set_title("旧".to_owned());
    ilst.insert(Atom::new(
        AtomIdent::Freeform {
            mean: "com.apple.iTunes".into(),
            name: "MY_CUSTOM".into(),
        },
        AtomData::UTF8("保留我".to_owned()),
    ));
    ilst.insert(Atom::new(
        AtomIdent::Fourcc(*b"cprt"),
        AtomData::UTF8("版权所有".to_owned()),
    ));
    ilst.save_to_path(&path, lofty::config::WriteOptions::default())
        .unwrap();

    service().write(&path, &full_data()).unwrap();

    let tagged = lofty_file(&path);
    let ilst: Ilst = tagged.tag(TagType::Mp4Ilst).unwrap().clone().into();
    assert_eq!(ilst.title().as_deref(), Some("晴天 Sunny"));
    let text_of = |ident: AtomIdent<'_>| {
        ilst.get(&ident).and_then(|atom| {
            atom.data().find_map(|data| match data {
                AtomData::UTF8(text) => Some(text.clone()),
                _ => None,
            })
        })
    };
    assert_eq!(
        text_of(AtomIdent::Freeform {
            mean: "com.apple.iTunes".into(),
            name: "MY_CUSTOM".into()
        })
        .as_deref(),
        Some("保留我")
    );
    assert_eq!(
        text_of(AtomIdent::Fourcc(*b"cprt")).as_deref(),
        Some("版权所有")
    );
}

fn leftovers(dir: &TempDir) -> Vec<String> {
    fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".lp-tmp-"))
        .collect()
}

#[test]
fn successful_write_leaves_no_temp_files() {
    let dir = TempDir::new();
    let path = create_mp3(&dir, "clean.mp3");
    service().write(&path, &full_data()).unwrap();
    assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
}

#[test]
fn write_keeps_audio_data_intact() {
    let dir = TempDir::new();
    let original = fixtures::mp3_default();
    let path = create(&dir, "audio.mp3", &original);
    service().write(&path, &full_data()).unwrap();
    let after = fs::read(&path).unwrap();
    // 标签在文件开头（ID3v2）；音频帧原样在后面。
    assert!(after.ends_with(&original), "音频数据不应被改动");
}

/// 原文件被其他程序占着（不允许删除 / 替换）时，写入失败，原文件字节不变，也不留临时副本。
#[cfg(windows)]
#[test]
fn failed_write_keeps_the_original_file_and_removes_the_temp_copy() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = TempDir::new();
    let path = create_mp3(&dir, "locked.mp3");
    let before = fs::read(&path).unwrap();
    // 只共享读（FILE_SHARE_READ = 1）：别的进程不能删除 / 重命名覆盖它。
    let _holder = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();

    let error = service().write(&path, &full_data()).unwrap_err();

    assert!(matches!(error, TagError::Io(_)), "{error}");
    assert!(error.to_string().starts_with("文件操作失败"), "{error}");
    drop(_holder);
    assert_eq!(fs::read(&path).unwrap(), before, "原文件必须保持不变");
    assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
}

// ───────────────────────── 兼容性修复与跨实现导出 ─────────────────────────

#[test]
fn m4a_multi_value_fields_joined_with_semicolons_are_split_when_read() {
    // TagLib（C# 版）把多值字段拼成 "a; b" 写进同一个 data 原子。
    use lofty::mp4::{Atom, AtomData, AtomIdent, Ilst};

    let dir = TempDir::new();
    let path = create(&dir, "joined.m4a", &fixtures::m4a());
    let mut ilst = Ilst::new();
    for (ident, text) in [
        (*b"\xa9ART", "周杰伦; Jay & 测试"),
        (*b"\xa9gen", "Pop; Rock"),
        (*b"\xa9wrt", "甲;乙"),
    ] {
        ilst.insert(Atom::new(
            AtomIdent::Fourcc(ident),
            AtomData::UTF8(text.to_owned()),
        ));
    }
    ilst.save_to_path(&path, lofty::config::WriteOptions::default())
        .unwrap();

    let info = read(&path);
    assert_eq!(info.artist.as_deref(), Some("周杰伦/Jay & 测试"));
    assert_eq!(info.genre.as_deref(), Some("Pop/Rock"));
    assert_eq!(info.composer.as_deref(), Some("甲/乙"));
}

/// 在 WAV 末尾追加一个 `LIST` 块，但不更新 RIFF 头里的长度（TagLib 写出的 WAV 就是这样）。
fn wav_with_stale_riff_size() -> Vec<u8> {
    let mut bytes = fixtures::wav();
    let declared = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    bytes.extend_from_slice(b"LIST");
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(b"INFO");
    assert_eq!(declared as usize + 8, bytes.len() - 12, "RIFF 头没有更新");
    bytes
}

#[test]
fn wav_with_a_stale_riff_size_is_repaired_instead_of_duplicating_the_id3_chunk() {
    let dir = TempDir::new();
    let path = create(&dir, "stale.wav", &wav_with_stale_riff_size());
    // 先写一次（尾部的 LIST 块在声明长度之外，旧标签块也追加在那里）。
    let mut id3 = lofty::tag::Tag::new(TagType::Id3v2);
    id3.set_title("旧标题".to_owned());
    let mut bytes = fs::read(&path).unwrap();
    let mut chunk = Vec::new();
    id3.dump_to(&mut chunk, lofty::config::WriteOptions::default())
        .unwrap();
    bytes.extend_from_slice(b"id3 ");
    bytes.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&chunk);
    if chunk.len() % 2 == 1 {
        bytes.push(0);
    }
    fs::write(&path, &bytes).unwrap();
    assert_eq!(read(&path).title.as_deref(), Some("旧标题"));

    service()
        .write(
            &path,
            &AudioTagData {
                title: Some("新标题".into()),
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(read(&path).title.as_deref(), Some("新标题"));
    let after = fs::read(&path).unwrap();
    let declared = u32::from_le_bytes(after[4..8].try_into().unwrap()) as usize + 8;
    assert_eq!(declared, after.len(), "RIFF 头长度应与实际一致");
    let id3_chunks = after
        .windows(4)
        .filter(|w| w.eq_ignore_ascii_case(b"id3 "))
        .count();
    assert_eq!(id3_chunks, 1, "只应有一个 ID3 块");
}

fn data_from_spec(spec: &serde_json::Value) -> AudioTagData {
    let text = |key: &str| spec[key].as_str().map(str::to_owned);
    let number = |key: &str| spec[key].as_u64().map(|n| n as u32);
    AudioTagData {
        replace_all: spec["replaceAll"].as_bool().unwrap_or(false),
        title: text("title"),
        artists: spec["artists"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        album: text("album"),
        album_artist: text("albumArtist"),
        track_number: number("track"),
        disc_number: number("disc"),
        year: number("year"),
        genre: text("genre"),
        composer: text("composer"),
        comment: text("comment"),
        plain_lyrics: text("plainLyrics"),
        word_lyrics: text("wordLyrics"),
        netease_id: spec["neteaseId"].as_i64(),
        cover_data: (spec["cover"] == "png").then(|| PNG_COVER.to_vec()),
        cover_mime_type: text("coverMime"),
        ..Default::default()
    }
}

/// 把与 C# 探针相同的三个场景（写满 / 局部更新 / 完整替换）在每种容器上跑一遍，输出到
/// `LP_EXPORT_RUST_TAGS_DIR`，供 C# 探针读回来对比；没设变量时什么也不做。
#[test]
fn export_rust_written_files_for_the_csharp_probe() {
    let Some(out) = std::env::var_os("LP_EXPORT_RUST_TAGS_DIR") else {
        return;
    };
    let out = PathBuf::from(out);
    fs::create_dir_all(&out).unwrap();
    let spec_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tags/spec-full.json");
    let full: serde_json::Value = serde_json::from_slice(&fs::read(spec_path).unwrap()).unwrap();
    let updated = serde_json::json!({ "title": "新标题 updated", "year": 2010, "track": 5 });
    let cleared = serde_json::json!({
        "replaceAll": true, "title": "Only", "neteaseId": 186016, "plainLyrics": "[00:01.00]only"
    });
    for (ext, bytes) in fixtures::all_formats() {
        let write = |name: &str, steps: &[&serde_json::Value]| {
            let path = out.join(format!("{name}.{ext}"));
            fs::write(&path, &bytes).unwrap();
            for step in steps {
                service().write(&path, &data_from_spec(step)).unwrap();
            }
        };
        write("rust-full", &[&full]);
        write("rust-updated", &[&full, &updated]);
        write("rust-cleared", &[&full, &cleared]);
    }
    // Rust 在 C# 写好的文件上局部改标题（其余字段、封面必须原样保留）。
    for ext in ["mp3", "flac", "m4a", "ogg", "opus", "wav"] {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/tags")
            .join(format!("csharp-full.{ext}"));
        let path = out.join(format!("rust-over-csharp.{ext}"));
        fs::copy(source, &path).unwrap();
        service()
            .write(
                &path,
                &data_from_spec(&serde_json::json!({ "title": "Rust 改的标题" })),
            )
            .unwrap();
    }
}

#[test]
fn id3v23_multi_values_are_joined_with_slashes_and_v24_keeps_separate_values() {
    let dir = TempDir::new();
    let data = AudioTagData {
        artists: vec!["周杰伦".into(), "Jay".into()],
        genre: Some("Pop/Rock".into()),
        composer: Some("甲/乙".into()),
        ..Default::default()
    };

    // 新建标签 → ID3v2.3：一个帧里用 `/` 拼接，没有 NUL 分隔。
    let v23 = create_mp3(&dir, "v23.mp3");
    service().write(&v23, &data).unwrap();
    assert_eq!(id3v2_header(&v23)[3], 3);
    let tagged = lofty_file(&v23);
    let id3 = tagged.tag(TagType::Id3v2).unwrap();
    let artists: Vec<_> = id3.get_strings(ItemKey::TrackArtist).collect();
    assert_eq!(artists, ["周杰伦/Jay"]);
    let genres: Vec<_> = id3.get_strings(ItemKey::Genre).collect();
    assert_eq!(genres, ["Pop/Rock"]);
    assert_eq!(read(&v23).artist.as_deref(), Some("周杰伦/Jay"));

    // 已有 ID3v2.4 → 保持 2.4，多值是分开的条目。
    let v24 = create_mp3(&dir, "v24.mp3");
    let mut tag = lofty::tag::Tag::new(TagType::Id3v2);
    tag.set_title("t".to_owned());
    tag.save_to_path(&v24, lofty::config::WriteOptions::default())
        .unwrap();
    service().write(&v24, &data).unwrap();
    assert_eq!(id3v2_header(&v24)[3], 4);
    let tagged = lofty_file(&v24);
    let id3 = tagged.tag(TagType::Id3v2).unwrap();
    let artists: Vec<_> = id3.get_strings(ItemKey::TrackArtist).collect();
    assert_eq!(artists, ["周杰伦", "Jay"]);
    assert_eq!(read(&v24).artist.as_deref(), Some("周杰伦/Jay"));
    assert_eq!(read(&v24).composer.as_deref(), Some("甲/乙"));
}
