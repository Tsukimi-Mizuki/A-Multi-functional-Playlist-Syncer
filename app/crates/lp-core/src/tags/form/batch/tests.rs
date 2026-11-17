use super::*;

fn row(title: &str, artist: &str, year: Option<u32>) -> TagRowView {
    TagRowView {
        readable: true,
        title: Some(title.to_owned()),
        artist: Some(artist.to_owned()),
        year,
        ..TagRowView::default()
    }
}

fn find(values: &[CommonValue], field: TagField) -> &CommonValue {
    values.iter().find(|item| item.field == field).unwrap()
}

fn edits(items: &[(TagField, &str)]) -> TagEdits {
    items.iter().map(|(f, t)| (*f, (*t).to_owned())).collect()
}

#[test]
fn path_key_normalizes_slashes_and_case() {
    assert_eq!(path_key("D:/Music/A.FLAC"), "d:\\music\\a.flac");
    assert_eq!(path_key("D:\\Music\\a.flac"), path_key("d:/MUSIC/A.flac"));
}

#[test]
fn row_value_formats_text_and_numbers() {
    let r = TagRowView {
        title: Some("夜曲".into()),
        year: Some(2005),
        track_number: Some(3),
        ..TagRowView::default()
    };
    assert_eq!(row_value(&r, TagField::Title), "夜曲");
    assert_eq!(row_value(&r, TagField::Year), "2005");
    assert_eq!(row_value(&r, TagField::TrackNumber), "3");
    assert_eq!(row_value(&r, TagField::Album), "");
    assert_eq!(row_value(&r, TagField::DiscNumber), "");
}

#[test]
fn common_values_follow_panel_order() {
    let values = common_values(&[row("a", "x", None)]);
    let order: Vec<TagField> = values.iter().map(|item| item.field).collect();
    assert_eq!(order, BATCH_FIELDS);
}

#[test]
fn common_values_share_equal_values_and_mark_different_ones() {
    let rows = [
        row("同名", "歌手甲", Some(2001)),
        row("同名", "歌手乙", Some(2001)),
    ];
    let values = common_values(&rows);
    let title = find(&values, TagField::Title);
    assert_eq!((title.value.as_str(), title.mixed), ("同名", false));
    let artist = find(&values, TagField::Artist);
    assert_eq!((artist.value.as_str(), artist.mixed), ("", true));
    let year = find(&values, TagField::Year);
    assert_eq!((year.value.as_str(), year.mixed), ("2001", false));
}

#[test]
fn common_values_treat_missing_value_as_different_from_present() {
    let values = common_values(&[row("a", "x", Some(1999)), row("a", "x", None)]);
    assert!(find(&values, TagField::Year).mixed);
}

#[test]
fn common_values_of_no_rows_are_empty_and_not_mixed() {
    let values = common_values(&[]);
    assert_eq!(values.len(), BATCH_FIELDS.len());
    assert!(
        values
            .iter()
            .all(|item| item.value.is_empty() && !item.mixed)
    );
}

#[test]
fn set_edit_removes_the_edit_when_changed_back() {
    let common = common_values(&[row("原标题", "x", None)]);
    let mut e = TagEdits::new();
    set_edit(&mut e, &common, TagField::Title, "新标题".into());
    assert_eq!(e.get(&TagField::Title).map(String::as_str), Some("新标题"));
    set_edit(&mut e, &common, TagField::Title, "原标题".into());
    assert!(e.is_empty());
}

#[test]
fn set_edit_counts_every_edit_on_a_mixed_field() {
    let common = common_values(&[row("a", "x", None), row("b", "x", None)]);
    let mut e = TagEdits::new();
    set_edit(&mut e, &common, TagField::Title, "统一".into());
    assert_eq!(e.get(&TagField::Title).map(String::as_str), Some("统一"));
    // 空串表示把所有选中歌曲的这一项清掉，必须保留。
    set_edit(&mut e, &common, TagField::Title, String::new());
    assert_eq!(e.get(&TagField::Title).map(String::as_str), Some(""));
}

#[test]
fn set_edit_treats_unknown_field_as_empty_and_not_mixed() {
    let mut e = TagEdits::new();
    set_edit(&mut e, &[], TagField::Album, "专辑".into());
    assert_eq!(e.get(&TagField::Album).map(String::as_str), Some("专辑"));
    set_edit(&mut e, &[], TagField::Album, String::new());
    assert!(e.is_empty());
}

#[test]
fn field_views_show_common_value_or_the_edit() {
    let common = common_values(&[row("a", "x", None), row("b", "x", None)]);
    let e = edits(&[(TagField::Title, "统一"), (TagField::Artist, "新歌手")]);
    let views = field_views(&common, &e);
    assert_eq!(views.len(), common.len());
    assert_eq!(views[0].field, TagField::Title);
    assert_eq!(
        views[0],
        FieldView {
            field: TagField::Title,
            value: "统一".into(),
            mixed: true,
            edited: true
        }
    );
    let artist = &views[1];
    assert_eq!(
        (artist.value.as_str(), artist.mixed, artist.edited),
        ("新歌手", false, true)
    );
    let album = views.iter().find(|v| v.field == TagField::Album).unwrap();
    assert_eq!(
        (album.value.as_str(), album.mixed, album.edited),
        ("", false, false)
    );
}

#[test]
fn build_patch_without_edits_is_empty() {
    assert_eq!(
        build_patch(&TagEdits::new(), &CoverChange::Keep),
        Ok(TagPatch::default())
    );
}

#[test]
fn build_patch_handles_text_fields() {
    let e = edits(&[
        (TagField::Title, "  标题 "),
        (TagField::Album, "   "),
        (TagField::AlbumArtist, "合辑"),
        (TagField::Genre, ""),
        (TagField::Composer, "曲"),
        (TagField::Lyricist, "词"),
        (TagField::Comment, " 备注"),
    ]);
    let patch = build_patch(&e, &CoverChange::Keep).unwrap();
    assert_eq!(patch.title, Some(Some("标题".into())));
    assert_eq!(patch.album, Some(None));
    assert_eq!(patch.album_artist, Some(Some("合辑".into())));
    assert_eq!(patch.genre, Some(None));
    assert_eq!(patch.composer, Some(Some("曲".into())));
    assert_eq!(patch.lyricist, Some(Some("词".into())));
    assert_eq!(patch.comment, Some(Some("备注".into())));
    assert_eq!(patch.artists, None);
    assert_eq!(patch.year, None);
}

#[test]
fn build_patch_splits_artists_and_empty_means_clear() {
    let patch = build_patch(
        &edits(&[(TagField::Artist, "甲 / 乙；丙")]),
        &CoverChange::Keep,
    )
    .unwrap();
    assert_eq!(
        patch.artists,
        Some(vec!["甲".into(), "乙".into(), "丙".into()])
    );
    let cleared = build_patch(&edits(&[(TagField::Artist, "  ")]), &CoverChange::Keep).unwrap();
    assert_eq!(cleared.artists, Some(Vec::new()));
}

#[test]
fn build_patch_parses_numbers_and_blank_clears() {
    let e = edits(&[
        (TagField::Year, "2020"),
        (TagField::TrackNumber, " 07 "),
        (TagField::DiscNumber, ""),
    ]);
    let patch = build_patch(&e, &CoverChange::Keep).unwrap();
    assert_eq!(patch.year, Some(Some(2020)));
    assert_eq!(patch.track_number, Some(Some(7)));
    assert_eq!(patch.disc_number, Some(None));
}

#[test]
fn build_patch_reports_the_first_invalid_number_in_fixed_order() {
    let all = edits(&[
        (TagField::Year, "二〇"),
        (TagField::TrackNumber, "x"),
        (TagField::DiscNumber, "y"),
    ]);
    let err = build_patch(&all, &CoverChange::Keep).unwrap_err();
    assert_eq!(err, FormError::NotANumber(TagField::Year));
    assert_eq!(err.to_string(), "年份必须是数字。");

    let later = edits(&[(TagField::TrackNumber, "x"), (TagField::DiscNumber, "y")]);
    let err = build_patch(&later, &CoverChange::Keep).unwrap_err();
    assert_eq!(err.to_string(), "音轨号必须是数字。");

    let disc = edits(&[(TagField::DiscNumber, "-1")]);
    assert_eq!(
        build_patch(&disc, &CoverChange::Keep)
            .unwrap_err()
            .to_string(),
        "碟号必须是数字。"
    );
}

#[test]
fn build_patch_applies_cover_changes() {
    let replace = CoverChange::Replace {
        base64: "AAAA".into(),
        mime_type: "image/png".into(),
    };
    let patch = build_patch(&TagEdits::new(), &replace).unwrap();
    assert_eq!(patch.cover_base64.as_deref(), Some("AAAA"));
    assert_eq!(patch.cover_mime_type.as_deref(), Some("image/png"));
    assert!(!patch.remove_cover);

    let patch = build_patch(&TagEdits::new(), &CoverChange::Remove).unwrap();
    assert!(patch.remove_cover);
    assert_eq!(patch.cover_base64, None);

    let patch = build_patch(&TagEdits::new(), &CoverChange::Keep).unwrap();
    assert!(!patch.remove_cover && patch.cover_base64.is_none());
}

#[test]
fn build_patch_output_converts_to_expected_clear_list() {
    let e = edits(&[
        (TagField::Title, "新标题"),
        (TagField::Album, " "),
        (TagField::Artist, ""),
        (TagField::Year, ""),
        (TagField::Comment, ""),
    ]);
    let data = build_patch(&e, &CoverChange::Keep)
        .unwrap()
        .into_data()
        .unwrap();
    assert_eq!(data.title.as_deref(), Some("新标题"));
    assert!(data.artists.is_empty());
    let mut clear = data.clear.clone();
    clear.sort_by_key(|f| BATCH_FIELDS.iter().position(|x| x == f));
    assert_eq!(
        clear,
        [
            TagField::Artist,
            TagField::Album,
            TagField::Year,
            TagField::Comment
        ]
    );
}

#[test]
fn empty_patch_is_rejected_by_the_backend() {
    let patch = build_patch(&TagEdits::new(), &CoverChange::Keep).unwrap();
    assert!(patch.into_data().is_err());
}
