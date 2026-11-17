use super::*;

#[test]
fn split_artists_uses_every_separator_and_drops_blanks() {
    assert_eq!(
        split_artists("a/b;c；d、e"),
        ["a", "b", "c", "d", "e"].map(String::from)
    );
    assert_eq!(split_artists("  A / / B  "), ["A", "B"].map(String::from));
    assert!(split_artists("   ").is_empty());
    assert!(split_artists("").is_empty());
}

#[test]
fn split_artists_keeps_other_punctuation_inside_names() {
    // `&`、`,` 不是分隔符（与前端一致）；`/` 是，所以 `AC/DC` 会被拆开——编辑器靠 `keep` 避开它。
    assert_eq!(split_artists("Simon & Garfunkel"), ["Simon & Garfunkel"]);
    assert_eq!(split_artists("AC/DC"), ["AC", "DC"]);
}

#[test]
fn trimmed_or_none_drops_blank_text() {
    assert_eq!(trimmed_or_none("  标题 "), Some("标题".to_owned()));
    assert_eq!(trimmed_or_none(" \t "), None);
    assert_eq!(trimmed_or_none(""), None);
}

#[test]
fn parse_number_accepts_blank_and_ascii_digits() {
    assert_eq!(parse_number(TagField::Year, ""), Ok(None));
    assert_eq!(parse_number(TagField::Year, "   "), Ok(None));
    assert_eq!(parse_number(TagField::Year, " 2024 "), Ok(Some(2024)));
    assert_eq!(parse_number(TagField::TrackNumber, "007"), Ok(Some(7)));
    assert_eq!(parse_number(TagField::TrackNumber, "0"), Ok(Some(0)));
    assert_eq!(
        parse_number(TagField::DiscNumber, "4294967295"),
        Ok(Some(u32::MAX))
    );
}

#[test]
fn parse_number_rejects_anything_else() {
    for text in ["4294967296", "-1", "+1", "1.5", "2O24", "１２", "٣", "1 2"] {
        assert_eq!(
            parse_number(TagField::Year, text),
            Err(FormError::NotANumber(TagField::Year)),
            "{text}"
        );
    }
}

#[test]
fn form_error_names_the_field() {
    assert_eq!(
        FormError::NotANumber(TagField::Year).to_string(),
        "年份必须是数字。"
    );
    assert_eq!(
        FormError::NotANumber(TagField::TrackNumber).to_string(),
        "音轨号必须是数字。"
    );
    assert_eq!(
        FormError::NotANumber(TagField::DiscNumber).to_string(),
        "碟号必须是数字。"
    );
}

#[test]
fn cover_change_round_trips_as_tagged_camel_case() {
    let replace = CoverChange::Replace {
        base64: "AAAA".to_owned(),
        mime_type: "image/png".to_owned(),
    };
    let json = serde_json::to_value(&replace).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "kind": "replace", "base64": "AAAA", "mimeType": "image/png" })
    );
    assert_eq!(
        serde_json::from_value::<CoverChange>(json).unwrap(),
        replace
    );
    assert_eq!(
        serde_json::to_value(CoverChange::Keep).unwrap(),
        serde_json::json!({ "kind": "keep" })
    );
    assert_eq!(
        serde_json::from_value::<CoverChange>(serde_json::json!({ "kind": "remove" })).unwrap(),
        CoverChange::Remove
    );
    assert_eq!(CoverChange::default(), CoverChange::Keep);
}
