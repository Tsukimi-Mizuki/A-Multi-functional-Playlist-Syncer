use super::*;
use crate::online::OnlineMusicSource;
use crate::tags::SidecarLyrics;

fn view() -> TagView {
    TagView {
        path: "D:\\Music\\a.flac".into(),
        file_name: "a.flac".into(),
        title: None,
        artist: None,
        album: None,
        album_artist: None,
        year: None,
        track_number: None,
        track_total: None,
        disc_number: None,
        disc_total: None,
        genre: None,
        composer: None,
        lyricist: None,
        comment: None,
        lyrics: None,
        sidecar_lyrics: None,
        netease_id: None,
        has_cover: false,
        cover_data_url: None,
        cover_size: None,
        cover_too_large: false,
        cover_mime_type: None,
        cover_width: None,
        cover_height: None,
        cover_picture_type: None,
        duration_ms: 0,
        bitrate: 0,
        file_size: 0,
        file_format: None,
    }
}

fn song() -> OnlineSongResult {
    OnlineSongResult {
        source: OnlineMusicSource::Netease,
        source_id: "1".into(),
        title: "Online Title".into(),
        artist: "Ann/Bob".into(),
        artists: vec!["Ann".into(), " Bob ".into(), "  ".into()],
        album: "Online Album".into(),
        duration_ms: 1000,
        year: None,
        track_number: None,
        disc_number: None,
        genre: None,
        composer: None,
        cover_url: None,
        numeric_source_id: None,
    }
}

fn fields() -> EditorFields {
    EditorFields {
        title: "T".into(),
        artist: "AC/DC".into(),
        album: "Al".into(),
        album_artist: "AC/DC".into(),
        year: "1980".into(),
        track_number: "3".into(),
        disc_number: "1".into(),
        genre: "Rock/Pop".into(),
        composer: "C".into(),
        lyricist: "L".into(),
        comment: "note".into(),
        lyrics: "la".into(),
    }
}

fn list(text: &str, values: &[&str]) -> ArtistList {
    ArtistList {
        text: text.into(),
        values: values.iter().map(|v| v.to_string()).collect(),
    }
}

#[test]
fn from_view_fills_empty_strings_and_numbers() {
    assert_eq!(EditorFields::from_view(&view()), EditorFields::default());
    let mut v = view();
    v.title = Some("T".into());
    v.year = Some(1999);
    v.track_number = Some(7);
    v.disc_number = Some(2);
    v.comment = Some("c".into());
    let f = EditorFields::from_view(&v);
    assert_eq!(f.title, "T");
    assert_eq!(f.year, "1999");
    assert_eq!(f.track_number, "7");
    assert_eq!(f.disc_number, "2");
    assert_eq!(f.comment, "c");
}

#[test]
fn from_view_lyrics_prefers_embedded_then_sidecar() {
    let sidecar = SidecarLyrics {
        path: "a.lrc".into(),
        file_name: "a.lrc".into(),
        text: "side".into(),
    };
    let mut v = view();
    v.sidecar_lyrics = Some(sidecar);
    assert_eq!(EditorFields::from_view(&v).lyrics, "side");
    v.lyrics = Some("embedded".into());
    assert_eq!(EditorFields::from_view(&v).lyrics, "embedded");
}

#[test]
fn value_returns_each_field() {
    let f = fields();
    assert_eq!(f.value(TagField::Title), "T");
    assert_eq!(f.value(TagField::Artist), "AC/DC");
    assert_eq!(f.value(TagField::Album), "Al");
    assert_eq!(f.value(TagField::AlbumArtist), "AC/DC");
    assert_eq!(f.value(TagField::Year), "1980");
    assert_eq!(f.value(TagField::TrackNumber), "3");
    assert_eq!(f.value(TagField::DiscNumber), "1");
    assert_eq!(f.value(TagField::Genre), "Rock/Pop");
    assert_eq!(f.value(TagField::Composer), "C");
    assert_eq!(f.value(TagField::Lyricist), "L");
    assert_eq!(f.value(TagField::Comment), "note");
}

#[test]
fn artist_list_trims_values_and_requires_text() {
    let l = ArtistList::from_online(&song(), "Ann/Bob ").expect("list");
    assert_eq!(l.values, ["Ann", "Bob"]);
    assert_eq!(l.text, "Ann/Bob ");
    assert!(ArtistList::from_online(&song(), "   ").is_none());
    let mut s = song();
    s.artists = vec![" ".into()];
    assert!(ArtistList::from_online(&s, "x").is_none());
    s.artists.clear();
    assert!(ArtistList::from_online(&s, "x").is_none());
}

#[test]
fn plan_keeps_every_unchanged_multi_value_field() {
    let f = fields();
    let plan = multi_value_plan(&f, Some(&f), None);
    assert_eq!(plan.keep, MULTI_VALUE_FIELDS);
    // AC/DC 被拆开的结果无关紧要：后端不碰 keep 里的字段。
    assert_eq!(plan.artists, ["AC", "DC"]);
}

#[test]
fn plan_without_loaded_keeps_nothing() {
    let plan = multi_value_plan(&fields(), None, None);
    assert!(plan.keep.is_empty());
}

#[test]
fn plan_changed_artist_is_split_and_not_kept() {
    let loaded = fields();
    let mut f = loaded.clone();
    f.artist = "X; Y".into();
    let plan = multi_value_plan(&f, Some(&loaded), None);
    assert_eq!(plan.artists, ["X", "Y"]);
    assert_eq!(
        plan.keep,
        [
            TagField::AlbumArtist,
            TagField::Genre,
            TagField::Composer,
            TagField::Lyricist
        ]
    );
}

#[test]
fn plan_online_list_with_same_text_uses_list_and_drops_artist_from_keep() {
    let loaded = fields();
    let mut f = loaded.clone();
    f.artist = "Ann/Bob".into();
    let online = list("Ann/Bob", &["Ann", "Bob"]);
    let plan = multi_value_plan(&f, Some(&loaded), Some(&online));
    assert_eq!(plan.artists, ["Ann", "Bob"]);
    assert!(!plan.keep.contains(&TagField::Artist));
    assert!(plan.keep.contains(&TagField::Genre));
}

#[test]
fn plan_online_list_same_as_file_text_is_not_kept() {
    let loaded = fields();
    let online = list("AC/DC", &["AC/DC"]);
    let plan = multi_value_plan(&loaded, Some(&loaded), Some(&online));
    assert_eq!(plan.artists, ["AC/DC"]);
    assert!(!plan.keep.contains(&TagField::Artist));
}

#[test]
fn plan_online_list_then_user_edit_splits_again() {
    let loaded = fields();
    let mut f = loaded.clone();
    f.artist = "Ann/Bob/Cy".into();
    let online = list("Ann/Bob", &["Ann", "Bob"]);
    let plan = multi_value_plan(&f, Some(&loaded), Some(&online));
    assert_eq!(plan.artists, ["Ann", "Bob", "Cy"]);
    assert!(!plan.keep.contains(&TagField::Artist));
}

#[test]
fn request_basic_fields_and_replace_all() {
    let mut v = view();
    v.netease_id = Some("12345".into());
    let mut f = fields();
    f.title = "  T  ".into();
    f.comment = "  ".into();
    f.genre = String::new();
    let req = build_write_request(&v, &f, Some(&fields()), None, &CoverChange::Keep).expect("ok");
    assert!(req.replace_all);
    assert_eq!(req.title.as_deref(), Some("T"));
    assert_eq!(req.comment, None);
    assert_eq!(req.genre, None);
    assert_eq!(req.album.as_deref(), Some("Al"));
    assert_eq!(req.year, Some(1980));
    assert_eq!(req.track_number, Some(3));
    assert_eq!(req.disc_number, Some(1));
    assert_eq!(req.netease_id, Some(12345));
    assert_eq!(req.track_total, None);
    assert_eq!(req.disc_total, None);
    assert_eq!(req.word_lyrics, None);
    assert_eq!(req.composer.as_deref(), Some("C"));
    assert_eq!(req.lyricist.as_deref(), Some("L"));
    assert_eq!(req.album_artist.as_deref(), Some("AC/DC"));
}

#[test]
fn request_preserves_ac_dc_through_keep() {
    let f = fields();
    let req = build_write_request(&view(), &f, Some(&f), None, &CoverChange::Keep).expect("ok");
    assert!(req.keep.contains(&TagField::Artist));
    assert!(req.keep.contains(&TagField::AlbumArtist));
}

#[test]
fn request_number_errors_in_order() {
    let mut f = fields();
    f.year = "19x".into();
    f.track_number = "a".into();
    f.disc_number = "b".into();
    let err = build_write_request(&view(), &f, None, None, &CoverChange::Keep).unwrap_err();
    assert_eq!(err, FormError::NotANumber(TagField::Year));
    assert_eq!(err.to_string(), "年份必须是数字。");
    f.year.clear();
    let err = build_write_request(&view(), &f, None, None, &CoverChange::Keep).unwrap_err();
    assert_eq!(err.to_string(), "音轨号必须是数字。");
    f.track_number.clear();
    let err = build_write_request(&view(), &f, None, None, &CoverChange::Keep).unwrap_err();
    assert_eq!(err.to_string(), "碟号必须是数字。");
    f.disc_number.clear();
    let req = build_write_request(&view(), &f, None, None, &CoverChange::Keep).expect("ok");
    assert_eq!(
        (req.year, req.track_number, req.disc_number),
        (None, None, None)
    );
}

#[test]
fn request_netease_id_only_when_all_digits() {
    let f = fields();
    let id_of = |text: Option<&str>| {
        let mut v = view();
        v.netease_id = text.map(str::to_owned);
        build_write_request(&v, &f, None, None, &CoverChange::Keep)
            .expect("ok")
            .netease_id
    };
    assert_eq!(id_of(Some("987")), Some(987));
    assert_eq!(id_of(Some("98a")), None);
    assert_eq!(id_of(Some("-5")), None);
    assert_eq!(id_of(Some("")), None);
    assert_eq!(id_of(None), None);
}

#[test]
fn request_lyrics_blank_is_none_but_nonblank_is_untrimmed() {
    let mut f = fields();
    f.lyrics = " \n ".into();
    let req = build_write_request(&view(), &f, None, None, &CoverChange::Keep).expect("ok");
    assert_eq!(req.plain_lyrics, None);
    f.lyrics = "\n[00:01.00]la \n".into();
    let req = build_write_request(&view(), &f, None, None, &CoverChange::Keep).expect("ok");
    assert_eq!(req.plain_lyrics.as_deref(), Some("\n[00:01.00]la \n"));
}

#[test]
fn request_cover_variants() {
    let f = fields();
    let keep = build_write_request(&view(), &f, None, None, &CoverChange::Keep).expect("ok");
    assert_eq!(
        (keep.cover_base64, keep.cover_mime_type, keep.remove_cover),
        (None, None, false)
    );
    let replace = build_write_request(
        &view(),
        &f,
        None,
        None,
        &CoverChange::Replace {
            base64: "QUJD".into(),
            mime_type: "image/png".into(),
        },
    )
    .expect("ok");
    assert_eq!(replace.cover_base64.as_deref(), Some("QUJD"));
    assert_eq!(replace.cover_mime_type.as_deref(), Some("image/png"));
    assert!(!replace.remove_cover);
    let remove = build_write_request(&view(), &f, None, None, &CoverChange::Remove).expect("ok");
    assert!(remove.remove_cover);
    assert_eq!(remove.cover_base64, None);
}

#[test]
fn request_uses_online_artists_when_text_unchanged() {
    let loaded = fields();
    let mut f = loaded.clone();
    f.artist = "Ann/Bob".into();
    let online = list("Ann/Bob", &["Ann", "Bob"]);
    let req = build_write_request(
        &view(),
        &f,
        Some(&loaded),
        Some(&online),
        &CoverChange::Keep,
    )
    .expect("ok");
    assert_eq!(req.artists, ["Ann", "Bob"]);
}

#[test]
fn dirty_cases() {
    let base = fields();
    assert!(!is_dirty(&base, &base, &CoverChange::Keep, None));
    let mut changed = base.clone();
    changed.title = "other".into();
    assert!(is_dirty(&changed, &base, &CoverChange::Keep, None));
    assert!(is_dirty(&base, &base, &CoverChange::Remove, None));
    assert!(is_dirty(
        &base,
        &base,
        &CoverChange::Replace {
            base64: "x".into(),
            mime_type: "image/png".into()
        },
        None
    ));
    // 在线清单的文字恰好与文件里一样：仍是改动。
    let same = list("AC/DC", &["AC/DC"]);
    assert!(is_dirty(&base, &base, &CoverChange::Keep, Some(&same)));
    let other = list("Ann/Bob", &["Ann", "Bob"]);
    assert!(!is_dirty(&base, &base, &CoverChange::Keep, Some(&other)));
}

#[test]
fn search_result_overwrites_title_artist_album_even_when_blank() {
    let prev = fields();
    let mut s = song();
    s.title = String::new();
    s.artist = " ".into();
    s.album = String::new();
    let (f, l) = apply_search_result(&prev, &s, None);
    assert_eq!(f.title, "");
    assert_eq!(f.artist, " ");
    assert_eq!(f.album, "");
    assert_eq!(l, None);
    // 其余保持。
    assert_eq!(f.year, "1980");
    assert_eq!(f.genre, "Rock/Pop");
    assert_eq!(f.album_artist, prev.album_artist);
    assert_eq!(f.lyricist, prev.lyricist);
    assert_eq!(f.comment, prev.comment);
}

#[test]
fn search_result_applies_values_lyrics_and_list() {
    let prev = fields();
    let mut s = song();
    s.year = Some(2001);
    s.track_number = Some(4);
    s.disc_number = Some(2);
    s.genre = Some("  Jazz ".into());
    s.composer = Some("  ".into());
    let (f, l) = apply_search_result(&prev, &s, Some(" [00:01.00]x\n"));
    assert_eq!(f.title, "Online Title");
    assert_eq!(f.artist, "Ann/Bob");
    assert_eq!(f.year, "2001");
    assert_eq!(f.track_number, "4");
    assert_eq!(f.disc_number, "2");
    assert_eq!(f.genre, "Jazz");
    assert_eq!(f.composer, "C");
    assert_eq!(f.lyrics, " [00:01.00]x\n");
    assert_eq!(l, Some(list("Ann/Bob", &["Ann", "Bob"])));
    let (f, _) = apply_search_result(&prev, &s, Some("  "));
    assert_eq!(f.lyrics, "la");
    let (f, _) = apply_search_result(&prev, &s, None);
    assert_eq!(f.lyrics, "la");
}

#[test]
fn online_tags_blank_values_keep_previous() {
    let prev = fields();
    let mut s = song();
    s.title = "  ".into();
    s.artist = String::new();
    s.album = " ".into();
    s.artists.clear();
    let current = list("AC/DC", &["AC/DC"]);
    let (f, l) = apply_online_tags(&prev, &s, Some(&current));
    assert_eq!(f, prev);
    assert_eq!(l, Some(current));
    let (_, l) = apply_online_tags(&prev, &s, None);
    assert_eq!(l, None);
}

#[test]
fn online_tags_apply_trimmed_values_and_keep_lyrics() {
    let prev = fields();
    let mut s = song();
    s.title = "  New  ".into();
    s.year = Some(1975);
    s.genre = Some(" Folk ".into());
    let (f, l) = apply_online_tags(&prev, &s, None);
    assert_eq!(f.title, "New");
    assert_eq!(f.artist, "Ann/Bob");
    assert_eq!(f.album, "Online Album");
    assert_eq!(f.year, "1975");
    assert_eq!(f.genre, "Folk");
    assert_eq!(f.lyrics, "la");
    assert_eq!(l, Some(list("Ann/Bob", &["Ann", "Bob"])));
}

#[test]
fn online_tags_artist_without_list_replaces_remembered_list() {
    let prev = fields();
    let mut s = song();
    s.artists.clear();
    let current = list("AC/DC", &["AC/DC"]);
    let (f, l) = apply_online_tags(&prev, &s, Some(&current));
    assert_eq!(f.artist, "Ann/Bob");
    assert_eq!(l, None);
}

#[test]
fn query_with_title_joins_title_and_artists() {
    let mut f = fields();
    f.title = "  Song ".into();
    f.artist = " Ann / Bob ".into();
    f.album = "  ".into();
    let (keyword, ctx) = online_tags_query("D:\\Music\\x.flac", 4321, &f);
    assert_eq!(keyword, "Song Ann Bob");
    assert_eq!(ctx.title.as_deref(), Some("Song"));
    assert_eq!(ctx.artist.as_deref(), Some("Ann / Bob"));
    assert_eq!(ctx.album, None);
    assert_eq!(ctx.duration_ms, 4321);
    assert_eq!(ctx.file_path.as_deref(), Some("D:\\Music\\x.flac"));
}

#[test]
fn query_without_title_uses_file_name() {
    let mut f = fields();
    f.title = " ".into();
    f.artist = String::new();
    let (keyword, ctx) = online_tags_query("D:\\Music\\03 - Some_Song.flac", 0, &f);
    assert_eq!(keyword, "Some Song");
    assert_eq!(ctx.title, None);
    assert_eq!(ctx.artist, None);
}

#[test]
fn query_with_title_only() {
    let f = EditorFields {
        title: "Solo".into(),
        ..EditorFields::default()
    };
    let (keyword, _) = online_tags_query("a.mp3", 0, &f);
    assert_eq!(keyword, "Solo");
}
