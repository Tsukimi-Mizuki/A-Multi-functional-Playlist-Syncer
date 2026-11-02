//! 把网易云原始 JSON 归一化成强类型模型。对应 C# 的 `NeteaseMappers`（歌曲 / 歌单部分）。
//!
//! 字段类型不稳定（数字 / 字符串混用、缺失），全部经 [`json`] 宽容读取，单个字段异常不会拖垮整条数据。

use std::collections::HashMap;

use serde_json::Value;

use super::json;
use super::models::{AlbumRef, ArtistRef, NeteaseSong, PlaylistSummary};

const DEFAULT_DISC_NUMBER: i32 = 1;

/// 映射一首歌曲（`playlist/detail` 的 `tracks`、`song/detail` 的 `songs` 通用）。
///
/// `privilege` 用于补充接口顶层 `privileges` 数组里的 `maxbr` / `fee`：真实接口里 `privileges` 与 `songs` 平级，
/// 需要调用方按 id 关联后传入。歌曲对象里自带 `privilege` 对象时优先用自带的；
/// 自带的是 `null` 等非对象时回退到外部传入的（C# 在这种情况下会丢掉外部 privilege）。
pub fn map_song(song: &Value, privilege: Option<&Value>) -> NeteaseSong {
    if !song.is_object() {
        return NeteaseSong::default();
    }

    let artists = map_artists(json::property(song, "ar"))
        .or_else(|| map_artists(json::property(song, "artists")))
        .unwrap_or_default();

    let album_value = json::property(song, "al").or_else(|| json::property(song, "album"));
    let album = map_album_ref(album_value);

    let mut publish_time = json::i64_or(song, "publishTime", 0);
    if publish_time <= 0
        && let Some(album_value) = album_value
    {
        publish_time = json::i64_or(album_value, "publishTime", 0);
    }

    let mut disc_number = json::i32_or(song, "cd", json::i32_or(song, "disc", DEFAULT_DISC_NUMBER));
    if disc_number <= 0 {
        disc_number = DEFAULT_DISC_NUMBER;
    }

    let effective_privilege = json::property(song, "privilege")
        .filter(|value| value.is_object())
        .or(privilege.filter(|value| value.is_object()));
    let (max_bitrate, privilege_fee) = match effective_privilege {
        Some(privilege) => (
            json::i32_or(privilege, "maxbr", json::i32_or(privilege, "playMaxbr", 0)),
            json::i32_or(privilege, "fee", 0),
        ),
        None => (0, 0),
    };

    NeteaseSong {
        id: json::i64_or(song, "id", 0),
        name: json::string_or_none(song, "name").unwrap_or_default(),
        artists,
        album,
        alias: map_alias(json::property(song, "alias").or_else(|| json::property(song, "alia"))),
        duration_ms: json::i32_or(song, "dt", json::i32_or(song, "duration", 0)),
        track_number: json::i32_or(song, "no", json::i32_or(song, "trackNo", 0)),
        disc_number,
        release_date: format_release_date(publish_time),
        fee: json::i32_or(song, "fee", privilege_fee),
        max_bitrate,
    }
}

/// 读取接口顶层 `privileges` 数组，按歌曲 id 建索引（用于补充 `maxbr` / `fee`）。
pub fn map_privileges(root: &Value) -> HashMap<i64, &Value> {
    let mut by_id = HashMap::new();
    let Some(Value::Array(privileges)) = json::property(root, "privileges") else {
        return by_id;
    };
    for privilege in privileges.iter().filter(|value| value.is_object()) {
        let id = json::i64_or(privilege, "id", 0);
        if id > 0 {
            by_id.insert(id, privilege);
        }
    }
    by_id
}

/// 映射一批歌曲数组（`songs` / `tracks`），自动按 id 关联顶层 privileges，丢弃 id 非法的项。
pub fn map_songs(items: &[Value], privileges: &HashMap<i64, &Value>) -> Vec<NeteaseSong> {
    items
        .iter()
        .map(|item| {
            let id = json::i64_or(item, "id", 0);
            map_song(item, privileges.get(&id).copied())
        })
        .filter(|song| song.id > 0)
        .collect()
}

/// 映射歌单列表项（`/weapi/user/playlist` 返回的 `playlist` 数组元素）。
pub fn map_playlist_summary(playlist: &Value) -> PlaylistSummary {
    if !playlist.is_object() {
        return PlaylistSummary {
            id: 0,
            name: String::new(),
            cover_url: None,
            track_count: 0,
            creator_nickname: None,
            description: None,
        };
    }
    PlaylistSummary {
        id: json::i64_or(playlist, "id", 0),
        name: json::string_or_none(playlist, "name").unwrap_or_default(),
        cover_url: json::string_or_none(playlist, "coverImgUrl"),
        track_count: json::i32_or(playlist, "trackCount", 0),
        creator_nickname: creator_nickname(playlist),
        description: json::string_or_none(playlist, "description"),
    }
}

/// 歌单创建者昵称。
pub fn creator_nickname(playlist: &Value) -> Option<String> {
    json::property(playlist, "creator")
        .and_then(|creator| json::string_or_none(creator, "nickname"))
}

/// 把毫秒时间戳格式化为 `yyyy-MM-dd`；非法值返回 `None`。
///
/// C# 按本机时区转换；这里固定用东八区：网易云的发行日期是北京时间 0 点，
/// 按 UTC 会少一天，换成其他时区的机器上按本机时区也会不一致。
pub fn format_release_date(publish_time_ms: i64) -> Option<String> {
    const BEIJING_OFFSET_SECS: i64 = 8 * 3600;
    // 9999-12-31T23:59:59.999Z，与 .NET `DateTimeOffset` 的上限一致。
    const MAX_MS: i64 = 253_402_300_799_999;
    if publish_time_ms <= 0 || publish_time_ms > MAX_MS {
        return None;
    }
    let seconds = publish_time_ms.div_euclid(1000) + BEIJING_OFFSET_SECS;
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    if !(1..=9999).contains(&year) {
        return None;
    }
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// 自 1970-01-01 起的天数转公历日期（Howard Hinnant 的 `civil_from_days`）。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    (year + i64::from(month <= 2), month, day)
}

/// 歌手数组；不是数组时返回 `None`（让调用方回落到别的字段），数组里没有有效歌手时是空 `Vec`。
fn map_artists(value: Option<&Value>) -> Option<Vec<ArtistRef>> {
    let Value::Array(items) = value? else {
        return None;
    };
    Some(
        items
            .iter()
            .filter(|item| item.is_object())
            .filter_map(|item| {
                let name = json::string_or_none(item, "name")?;
                if name.trim().is_empty() {
                    return None;
                }
                Some(ArtistRef {
                    id: json::i64_or(item, "id", 0),
                    name,
                })
            })
            .collect(),
    )
}

fn map_album_ref(value: Option<&Value>) -> AlbumRef {
    match value.filter(|album| album.is_object()) {
        None => AlbumRef::default(),
        Some(album) => AlbumRef {
            id: json::i64_or(album, "id", 0),
            name: json::string_or_none(album, "name").unwrap_or_default(),
            cover_url: json::string_or_none(album, "picUrl"),
        },
    }
}

/// 别名数组：只取非空字符串，以 `/` 连接；没有则 `None`。
fn map_alias(value: Option<&Value>) -> Option<String> {
    let Value::Array(items) = value? else {
        return None;
    };
    let parts: Vec<&str> = items
        .iter()
        .filter_map(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn map_song_reads_the_playlist_detail_shape() {
        let song = map_song(
            &json!({
                "id": 1001, "name": "夜曲", "dt": 226000, "no": 3, "cd": "2", "fee": 1,
                "publishTime": 1_099_843_200_000_i64,
                "ar": [{"id": 6452, "name": "周杰伦"}, {"id": 2, "name": "方文山"}],
                "al": {"id": 80, "name": "十一月的萧邦", "picUrl": "https://p1/a.jpg"},
                "alia": ["Nocturne"]
            }),
            None,
        );
        assert_eq!(song.id, 1001);
        assert_eq!(song.name, "夜曲");
        assert_eq!(song.artist_text(), "周杰伦/方文山");
        assert_eq!(song.artists[0].id, 6452);
        assert_eq!(song.album.name, "十一月的萧邦");
        assert_eq!(song.cover_url(), Some("https://p1/a.jpg"));
        assert_eq!(song.alias.as_deref(), Some("Nocturne"));
        assert_eq!(song.display_name(), "夜曲 (Nocturne)");
        assert_eq!(song.duration_ms, 226_000);
        assert_eq!(song.track_number, 3);
        assert_eq!(song.disc_number, 2);
        assert_eq!(song.fee, 1);
        assert_eq!(song.release_date.as_deref(), Some("2004-11-08"));
    }

    #[test]
    fn map_song_falls_back_to_legacy_field_names() {
        let song = map_song(
            &json!({
                "id": 5, "name": "旧字段", "duration": 1000, "trackNo": 9, "disc": 4,
                "artists": [{"id": 1, "name": "歌手"}],
                "album": {"id": 2, "name": "专辑", "publishTime": 1_099_843_200_000_i64},
                "alias": ["a", "", "b"]
            }),
            None,
        );
        assert_eq!(song.artist_text(), "歌手");
        assert_eq!(song.album.id, 2);
        assert_eq!(song.duration_ms, 1000);
        assert_eq!(song.track_number, 9);
        assert_eq!(song.disc_number, 4);
        assert_eq!(song.alias.as_deref(), Some("a/b"));
        // 歌曲自己没有 publishTime 时取专辑的。
        assert_eq!(song.release_date.as_deref(), Some("2004-11-08"));
    }

    #[test]
    fn map_song_defaults_for_missing_or_wrong_typed_fields() {
        let song = map_song(&json!({"id": "7", "name": 12, "cd": 0, "dt": "x"}), None);
        assert_eq!(song.id, 7);
        assert_eq!(song.name, "12");
        assert_eq!(song.disc_number, 1);
        assert_eq!(song.duration_ms, 0);
        assert!(song.artists.is_empty());
        assert_eq!(song.album, AlbumRef::default());
        assert_eq!(song.alias, None);
        assert_eq!(song.release_date, None);

        let empty = map_song(&json!("not an object"), None);
        assert_eq!(empty, NeteaseSong::default());
        assert_eq!(empty.id, 0);
        assert_eq!(empty.disc_number, 1);
    }

    #[test]
    fn map_song_skips_artists_without_a_name() {
        let song = map_song(
            &json!({"id": 1, "ar": [{"id": 1, "name": "  "}, {"id": 2}, "x", {"id": 3, "name": "有名"}]}),
            None,
        );
        assert_eq!(song.artists.len(), 1);
        assert_eq!(song.artist_text(), "有名");
    }

    #[test]
    fn an_empty_ar_array_does_not_fall_back_to_artists() {
        // 与 C# 一致：`ar` 是数组（哪怕为空）就不再看 `artists`。
        let song = map_song(
            &json!({"id": 1, "ar": [], "artists": [{"id": 1, "name": "x"}]}),
            None,
        );
        assert!(song.artists.is_empty());
    }

    #[test]
    fn privilege_supplies_fee_and_max_bitrate() {
        let privilege = json!({"id": 1, "fee": 4, "maxbr": 999_000});
        let song = map_song(&json!({"id": 1, "name": "x"}), Some(&privilege));
        assert_eq!(song.fee, 4);
        assert_eq!(song.max_bitrate, 999_000);

        // 歌曲自己的 fee 优先于 privilege 的 fee；maxbr 缺失时看 playMaxbr。
        let privilege = json!({"id": 1, "fee": 4, "playMaxbr": 320_000});
        let song = map_song(&json!({"id": 1, "fee": 8}), Some(&privilege));
        assert_eq!(song.fee, 8);
        assert_eq!(song.max_bitrate, 320_000);
    }

    #[test]
    fn embedded_privilege_wins_over_the_external_one() {
        let external = json!({"id": 1, "fee": 4, "maxbr": 1});
        let song = map_song(
            &json!({"id": 1, "privilege": {"fee": 1, "maxbr": 320_000}}),
            Some(&external),
        );
        assert_eq!(song.fee, 1);
        assert_eq!(song.max_bitrate, 320_000);

        // 自带 privilege 是 null 时回退到外部的。
        let song = map_song(&json!({"id": 1, "privilege": null}), Some(&external));
        assert_eq!(song.fee, 4);
        assert_eq!(song.max_bitrate, 1);
    }

    #[test]
    fn privileges_are_indexed_by_id_and_junk_is_ignored() {
        let root = json!({"privileges": [{"id": 1, "fee": 1}, {"id": "2", "fee": 8}, {"id": 0}, 5, {"fee": 3}]});
        let privileges = map_privileges(&root);
        assert_eq!(privileges.len(), 2);
        assert_eq!(json::i32_or(privileges[&2], "fee", 0), 8);
        assert!(map_privileges(&json!({})).is_empty());
        assert!(map_privileges(&json!({"privileges": "x"})).is_empty());
    }

    #[test]
    fn map_songs_links_privileges_and_drops_invalid_ids() {
        let root = json!({
            "songs": [{"id": 1, "name": "a"}, {"id": 0, "name": "bad"}, {"id": 2, "name": "b"}],
            "privileges": [{"id": 2, "fee": 1, "maxbr": 320_000}]
        });
        let privileges = map_privileges(&root);
        let Value::Array(items) = &root["songs"] else {
            unreachable!()
        };
        let songs = map_songs(items, &privileges);
        assert_eq!(songs.len(), 2);
        assert_eq!(songs[0].fee, 0);
        assert_eq!(songs[1].fee, 1);
        assert_eq!(songs[1].max_bitrate, 320_000);
    }

    #[test]
    fn playlist_summary_maps_all_fields() {
        let summary = map_playlist_summary(&json!({
            "id": 77, "name": "我喜欢的音乐", "coverImgUrl": "https://p1/c.jpg", "trackCount": 123,
            "creator": {"nickname": "月见"}, "description": "简介"
        }));
        assert_eq!(summary.id, 77);
        assert_eq!(summary.name, "我喜欢的音乐");
        assert_eq!(summary.cover_url.as_deref(), Some("https://p1/c.jpg"));
        assert_eq!(summary.track_count, 123);
        assert_eq!(summary.creator_nickname.as_deref(), Some("月见"));
        assert_eq!(summary.description.as_deref(), Some("简介"));

        let bare = map_playlist_summary(&json!({"id": "9"}));
        assert_eq!(bare.id, 9);
        assert_eq!(bare.name, "");
        assert_eq!(bare.cover_url, None);
        assert_eq!(bare.creator_nickname, None);
        assert_eq!(map_playlist_summary(&json!(null)).id, 0);
    }

    #[test]
    fn release_dates_use_beijing_time_and_reject_bad_values() {
        // 2004-11-07 16:00 UTC 即北京时间 2004-11-08 0 点。
        assert_eq!(
            format_release_date(1_099_843_200_000).as_deref(),
            Some("2004-11-08")
        );
        assert_eq!(format_release_date(1).as_deref(), Some("1970-01-01"));
        // 闰日：2024-02-28 16:00 UTC 是北京时间 2 月 29 日 0 点。
        assert_eq!(
            format_release_date(1_709_136_000_000).as_deref(),
            Some("2024-02-29")
        );
        assert_eq!(
            format_release_date(1_709_164_800_000).as_deref(),
            Some("2024-02-29")
        );
        assert_eq!(format_release_date(0), None);
        assert_eq!(format_release_date(-5), None);
        assert_eq!(format_release_date(i64::MAX), None);
        assert_eq!(format_release_date(253_402_300_800_000), None);
    }
}
