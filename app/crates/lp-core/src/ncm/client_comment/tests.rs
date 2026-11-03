use super::*;

const METADATA_JSON: &str = r#"{"musicId":33894312,"musicName":"晴天","artist":[["周杰伦",6452]],"album":"叶惠美","format":"mp3"}"#;

#[test]
fn try_decrypt_round_trips_client_payload() {
    let comment = build_comment(METADATA_JSON);

    assert!(comment.starts_with(METADATA_PREFIX));
    assert_eq!(try_decrypt(Some(&comment)).as_deref(), Some(METADATA_JSON));
    assert_eq!(try_read_music_id(Some(&comment)), Some(33894312));
}

#[test]
fn try_read_music_id_tolerates_surrounding_comment() {
    let comment = format!("自定义备注; {} ; id3v2", build_comment(METADATA_JSON));

    assert_eq!(try_read_music_id(Some(&comment)), Some(33894312));
}

#[test]
fn try_read_music_id_reads_string_encoded_id() {
    let comment = build_comment(r#"{"musicId":"123456"}"#);

    assert_eq!(try_read_music_id(Some(&comment)), Some(123456));
}

#[test]
fn try_read_music_id_ignores_foreign_comments() {
    let cases = [
        None,
        Some(""),
        Some("普通备注"),
        Some("LunaticPrincess:186016"),
        Some("163 key(Don't modify):"),
        Some("163 key(Don't modify):not-base64!!"),
        Some("163 key(Don't modify):AAAA"),
    ];
    for case in cases {
        assert_eq!(try_read_music_id(case), None, "{case:?}");
    }
}

#[test]
fn try_read_music_id_rejects_non_positive_and_non_object_payloads() {
    for json in [
        r#"{"musicId":0}"#,
        r#"{"musicId":-5}"#,
        r#"{"musicId":"abc"}"#,
        r#"{"musicId":1.5}"#,
        r#"{"musicId":null}"#,
        r#"{"other":1}"#,
        r#"[1,2]"#,
    ] {
        let comment = build_comment(json);
        assert_eq!(try_read_music_id(Some(&comment)), None, "{json}");
    }
}

#[test]
fn try_decrypt_accepts_bare_json_plaintext() {
    // 明文直接以 `{` 开头（没有 `music:` 前缀）时原样返回。
    let plain = br#"{"musicId":7}"#;
    let cipher = aes_ecb_encrypt(plain, &MODIFY_KEY);
    let comment = format!("{METADATA_PREFIX}{}", BASE64.encode(cipher));

    assert_eq!(try_read_music_id(Some(&comment)), Some(7));
}

#[test]
fn try_decrypt_rejects_bad_padding_and_wrong_prefix() {
    // 密文长度不是 16 的倍数。
    let comment = format!("{METADATA_PREFIX}{}", BASE64.encode([1u8; 15]));
    assert_eq!(try_decrypt(Some(&comment)), None);

    // 用错误的内容解出来：既不是 `{` 也不是 `music:`。
    let cipher = aes_ecb_encrypt(b"other:{}", &MODIFY_KEY);
    let comment = format!("{METADATA_PREFIX}{}", BASE64.encode(cipher));
    assert_eq!(try_decrypt(Some(&comment)), None);

    // 解密成功但 PKCS7 填充非法。
    let aes = Aes128::new((&MODIFY_KEY).into());
    let mut block = aes::Block::try_from(&[9u8; 16][..]).unwrap();
    aes.encrypt_block(&mut block);
    let comment = format!("{METADATA_PREFIX}{}", BASE64.encode(block));
    assert_eq!(try_decrypt(Some(&comment)), None);
}

#[test]
fn aes_ecb_matches_a_known_vector() {
    // FIPS-197 附录 C.1：key 000102…0f，明文 00112233…ff → 69c4e0d86a7b0430d8cdb78070b4c55a。
    let key: [u8; 16] = core::array::from_fn(|i| i as u8);
    let plain: [u8; 16] = core::array::from_fn(|i| (i as u8) * 0x11);
    let aes = Aes128::new((&key).into());
    let mut block = aes::Block::try_from(&plain[..]).unwrap();
    aes.encrypt_block(&mut block);
    let hex: String = block.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, "69c4e0d86a7b0430d8cdb78070b4c55a");
}
