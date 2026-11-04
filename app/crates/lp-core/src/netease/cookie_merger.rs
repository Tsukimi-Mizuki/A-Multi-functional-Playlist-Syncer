//! 把服务端后续响应里的 `Set-Cookie` 合并进已保存的 Cookie 串。对应 C# 的 `CookieMerger`。
//!
//! 网易云会轮换 `MUSIC_U`，丢掉就会提前掉登录。规则：
//! - 按名字（忽略大小写）覆盖；
//! - `Max-Age <= 0` 或 `Expires` 已过期视为删除（`Max-Age` 优先于 `Expires`，RFC 6265）；
//! - 空值不会覆盖已有值，也不会新增；
//! - 已有键保持原顺序，新增键追加在末尾。
//!
//! 本程序只有一个扁平的 Cookie 串，不区分 `Domain` / `Path`；而网易云会在同一个响应里对同一个名字下发多条
//! 不同 `Path` 的 `Set-Cookie`（实测：续期或带桌面标识的 weapi 响应里，`MUSIC_A_T` / `MUSIC_R_T` 有一条
//! `Path=/` 的 `Max-Age=0` 删除，同时又有二十多条 `Path=/api/feedback`、`Path=/neapi/clientlog` 等的重新设值，
//! 顺序每次不同）。按名字顺序执行会让令牌被随机地删掉，所以再加两条规则：
//! - **带非根 `Path` 的删除永远忽略**：它只清理那个路径下的 Cookie，与根路径的同名 Cookie 无关；
//! - **同一个响应里既删除又设值的名字，删除忽略、设值生效**：服务端是在迁移 Cookie 的作用域，不是在撤销它。
//!
//! 其余的删除（根路径或没写 `Path`、且本响应没有同名设值）照常执行。`NeteaseClient` 在此之外还有一道保险：
//! 合并结果会丢掉 `MUSIC_U` 时整体忽略。

use std::time::{SystemTime, UNIX_EPOCH};

/// 合并结果：合并后的 Cookie 串，以及相对输入是否真的发生了变化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieMergeResult {
    /// 合并后的 `a=1; b=2` 串；没有变化时原样返回输入。
    pub cookie: String,
    /// 是否有任何变化。
    pub changed: bool,
}

/// 合并 `Set-Cookie` 列表到现有 Cookie 串，以系统时间判断 `Expires` 是否过期。
pub fn merge(existing_cookie: Option<&str>, set_cookie_headers: &[String]) -> CookieMergeResult {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    merge_at(existing_cookie, set_cookie_headers, now)
}

/// 同 [`merge`]，但显式给出“当前时间”（Unix 秒），便于测试。
pub fn merge_at(
    existing_cookie: Option<&str>,
    set_cookie_headers: &[String],
    now_unix: i64,
) -> CookieMergeResult {
    let original = existing_cookie.unwrap_or_default();
    let mut pairs = parse_cookie_string(original);
    let mut changed = false;

    let parsed_headers: Vec<ParsedSetCookie> = set_cookie_headers
        .iter()
        .filter_map(|header| parse_set_cookie(header, now_unix))
        .collect();
    // 本响应里被设值（非删除且值不为空）的名字：这些名字的删除视为作用域迁移，不执行。
    let set_names: Vec<String> = parsed_headers
        .iter()
        .filter(|parsed| !parsed.is_delete && !parsed.value.is_empty())
        .map(|parsed| parsed.name.clone())
        .collect();

    for parsed in parsed_headers {
        let index = pairs
            .iter()
            .position(|(name, _)| name.eq_ignore_ascii_case(&parsed.name));

        if parsed.is_delete {
            let migrating = set_names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&parsed.name));
            if parsed.path_scoped || migrating {
                continue;
            }
            if let Some(index) = index {
                pairs.remove(index);
                changed = true;
            }
            continue;
        }
        if parsed.value.is_empty() {
            // 空值不覆盖已有值，也不新增。
            continue;
        }
        match index {
            None => {
                pairs.push((parsed.name, parsed.value));
                changed = true;
            }
            Some(index) if pairs[index].1 != parsed.value => {
                pairs[index].1 = parsed.value;
                changed = true;
            }
            Some(_) => {}
        }
    }

    if changed {
        CookieMergeResult {
            cookie: join_cookie_pairs(&pairs),
            changed: true,
        }
    } else {
        CookieMergeResult {
            cookie: original.to_owned(),
            changed: false,
        }
    }
}

/// 把键值对拼回 `a=1; b=2`。
pub fn join_cookie_pairs(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// 解析 `a=1; b=2` 形式的 Cookie 串（忽略无法解析的片段；同名保留最后一个的值、第一个的位置）。
pub fn parse_cookie_string(cookie: &str) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for part in cookie.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let Some(separator) = part.find('=').filter(|&i| i > 0) else {
            continue;
        };
        let name = part[..separator].trim();
        let value = part[separator + 1..].trim();
        match pairs
            .iter()
            .position(|(existing, _)| existing.eq_ignore_ascii_case(name))
        {
            Some(index) => pairs[index].1 = value.to_owned(),
            None => pairs.push((name.to_owned(), value.to_owned())),
        }
    }
    pairs
}

struct ParsedSetCookie {
    name: String,
    value: String,
    is_delete: bool,
    /// `Path` 属性存在且不是根路径 `/`：这条 Set-Cookie 只作用于某个子路径。
    path_scoped: bool,
}

fn parse_set_cookie(header: &str, now_unix: i64) -> Option<ParsedSetCookie> {
    if header.trim().is_empty() {
        return None;
    }
    let mut segments = header.split(';').map(str::trim);
    let first = segments.next()?;
    let separator = first.find('=').filter(|&i| i > 0)?;
    let name = first[..separator].trim().to_owned();
    let value = first[separator + 1..].trim().to_owned();

    // Max-Age 优先于 Expires（RFC 6265）。
    let mut max_age_expired: Option<bool> = None;
    let mut expires: Option<i64> = None;
    let mut path_scoped = false;
    for attribute in segments {
        let Some(attribute_separator) = attribute.find('=').filter(|&i| i > 0) else {
            continue;
        };
        let attribute_name = attribute[..attribute_separator].trim();
        let attribute_value = attribute[attribute_separator + 1..].trim();
        if attribute_name.eq_ignore_ascii_case("Max-Age") {
            if let Ok(seconds) = attribute_value.parse::<i64>() {
                max_age_expired = Some(seconds <= 0);
            }
        } else if attribute_name.eq_ignore_ascii_case("Expires")
            && let Some(parsed) = parse_http_date(attribute_value)
        {
            expires = Some(parsed);
        } else if attribute_name.eq_ignore_ascii_case("Path") {
            path_scoped = !attribute_value.is_empty() && attribute_value != "/";
        }
    }

    let is_delete = match max_age_expired {
        Some(expired) => expired,
        None => expires.is_some_and(|at| at <= now_unix),
    };
    Some(ParsedSetCookie {
        name,
        value,
        is_delete,
        path_scoped,
    })
}

/// 按 RFC 6265 §5.1.1 的宽松算法解析 Cookie 日期，返回 Unix 秒（一律按 UTC）。
///
/// 能同时处理 `Thu, 01 Jan 1970 00:00:00 GMT`、网易云等 Java 服务端常用的 `Thu, 01-Jan-1970 00:00:10 GMT`、
/// 两位年份以及 asctime 形式。
fn parse_http_date(text: &str) -> Option<i64> {
    let mut time: Option<(i64, i64, i64)> = None;
    let mut day: Option<i64> = None;
    let mut month: Option<i64> = None;
    let mut year: Option<i64> = None;

    let tokens = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == ':'))
        .filter(|t| !t.is_empty());
    for token in tokens {
        if time.is_none()
            && let Some(parsed) = parse_time_token(token)
        {
            time = Some(parsed);
        } else if day.is_none()
            && let Some(parsed) = parse_digits(token, 1, 2)
        {
            day = Some(parsed);
        } else if month.is_none()
            && let Some(parsed) = parse_month(token)
        {
            month = Some(parsed);
        } else if year.is_none()
            && let Some(parsed) = parse_digits(token, 2, 4)
        {
            year = Some(match parsed {
                70..=99 if token.len() == 2 => parsed + 1900,
                0..=69 if token.len() == 2 => parsed + 2000,
                _ => parsed,
            });
        }
    }

    let (hour, minute, second) = time?;
    let (day, month, year) = (day?, month?, year?);
    if !(1..=31).contains(&day) || year < 1601 || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(unix_from_civil(year, month, day, hour, minute, second))
}

fn parse_time_token(token: &str) -> Option<(i64, i64, i64)> {
    let mut parts = token.split(':');
    let hour = parse_digits(parts.next()?, 1, 2)?;
    let minute = parse_digits(parts.next()?, 1, 2)?;
    let second = parse_digits(parts.next()?, 1, 2)?;
    if parts.next().is_some() {
        return None;
    }
    Some((hour, minute, second))
}

/// 整个 token 必须是 `min..=max` 位的纯数字。
fn parse_digits(token: &str, min: usize, max: usize) -> Option<i64> {
    if token.len() < min || token.len() > max || !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    token.parse().ok()
}

fn parse_month(token: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let prefix = token.get(..3)?.to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|m| *m == prefix)
        .map(|index| index as i64 + 1)
}

/// 公历日期转 Unix 秒（Howard Hinnant 的 days_from_civil 算法）。
pub(crate) fn unix_from_civil(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    days * 86_400 + hour * 3_600 + minute * 60 + second
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> i64 {
        unix_from_civil(2026, 11, 2, 12, 0, 0)
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn merged(existing: &str, headers: &[&str]) -> CookieMergeResult {
        merge_at(Some(existing), &strings(headers), now())
    }

    #[test]
    fn overwrites_existing_value_and_keeps_order() {
        let result = merged(
            "os=pc; MUSIC_U=old; __csrf=c1",
            &["MUSIC_U=new; Path=/; HttpOnly"],
        );
        assert!(result.changed);
        assert_eq!(result.cookie, "os=pc; MUSIC_U=new; __csrf=c1");
    }

    #[test]
    fn appends_new_cookies() {
        let result = merged("MUSIC_U=a", &["NMTID=xyz; Max-Age=3600; Path=/"]);
        assert!(result.changed);
        assert_eq!(result.cookie, "MUSIC_U=a; NMTID=xyz");
    }

    #[test]
    fn name_match_ignores_case() {
        let result = merged("MUSIC_U=a", &["music_u=b"]);
        assert!(result.changed);
        assert_eq!(result.cookie, "MUSIC_U=b");
    }

    #[test]
    fn no_change_reports_unchanged_and_keeps_original_text() {
        let existing = "MUSIC_U=a;  __csrf=c1";
        let result = merged(existing, &["MUSIC_U=a; Path=/", "__csrf=c1"]);
        assert!(!result.changed);
        assert_eq!(result.cookie, existing);
    }

    #[test]
    fn empty_or_garbage_headers_are_unchanged() {
        assert!(!merged("MUSIC_U=a", &[]).changed);
        assert!(!merge_at(Some("MUSIC_U=a"), &[], now()).changed);
        assert!(!merged("MUSIC_U=a", &["", "garbage", "=novalue"]).changed);
    }

    #[test]
    fn empty_value_does_not_overwrite_or_add() {
        let result = merged("MUSIC_U=a", &["MUSIC_U=; Path=/", "NEW=; Path=/"]);
        assert!(!result.changed);
        assert_eq!(result.cookie, "MUSIC_U=a");
    }

    #[test]
    fn max_age_zero_or_negative_deletes_cookie() {
        let zero = merged("MUSIC_U=a; os=pc", &["os=pc; Max-Age=0"]);
        assert!(zero.changed);
        assert_eq!(zero.cookie, "MUSIC_U=a");

        let negative = merged("MUSIC_U=a; os=pc", &["MUSIC_U=; Max-Age=-1; Path=/"]);
        assert!(negative.changed);
        assert_eq!(negative.cookie, "os=pc");
    }

    #[test]
    fn expired_expires_deletes_cookie() {
        let result = merged(
            "MUSIC_U=a; os=pc",
            &["os=pc; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/"],
        );
        assert!(result.changed);
        assert_eq!(result.cookie, "MUSIC_U=a");
    }

    #[test]
    fn expired_dashed_expires_deletes_cookie() {
        // 网易云等 Java 服务端常用 "dd-MMM-yyyy" 形式的 Expires。
        let result = merged(
            "MUSIC_U=a; os=pc",
            &["os=pc; expires=Thu, 01-Jan-1970 00:00:10 GMT; path=/"],
        );
        assert!(result.changed);
        assert_eq!(result.cookie, "MUSIC_U=a");
    }

    #[test]
    fn future_expires_updates_cookie() {
        let result = merged(
            "MUSIC_U=a",
            &["MUSIC_U=b; Expires=Fri, 01 Jan 2100 00:00:00 GMT; Path=/"],
        );
        assert!(result.changed);
        assert_eq!(result.cookie, "MUSIC_U=b");
    }

    #[test]
    fn max_age_takes_precedence_over_expires() {
        let result = merged(
            "MUSIC_U=a",
            &["MUSIC_U=b; Max-Age=600; Expires=Thu, 01 Jan 1970 00:00:00 GMT"],
        );
        assert!(result.changed);
        assert_eq!(result.cookie, "MUSIC_U=b");
    }

    #[test]
    fn deleting_missing_cookie_is_unchanged() {
        assert!(!merged("MUSIC_U=a", &["gone=1; Max-Age=0"]).changed);
    }

    #[test]
    fn later_header_wins_within_one_response() {
        let result = merged("MUSIC_U=a", &["MUSIC_U=b", "MUSIC_U=c"]);
        assert_eq!(result.cookie, "MUSIC_U=c");
    }

    #[test]
    fn empty_existing_builds_from_headers() {
        let result = merge_at(None, &strings(&["MUSIC_U=a; Path=/", "__csrf=c"]), now());
        assert!(result.changed);
        assert_eq!(result.cookie, "MUSIC_U=a; __csrf=c");
    }

    // ---- 同一响应里同名、不同 Path 的 Set-Cookie（形状取自 2026-11 的联网实测，值是假的） ----

    const SESSION: &str =
        "MUSIC_A_T=a1; MUSIC_R_T=r1; __csrf=c1; NMTID=n1; MUSIC_R_U=ru1; MUSIC_U=u1";
    const SCOPES: [&str; 8] = [
        "/api/feedback",
        "/api/clientlog",
        "/eapi/feedback",
        "/eapi/clientlog",
        "/weapi/feedback",
        "/neapi/clientlog",
        "/wapi/feedback",
        "/openapi/clientlog",
    ];

    fn root_delete(name: &str) -> String {
        format!(
            "{name}=; Max-Age=0; Expires=Wed, 04 Nov 2026 09:26:27 GMT; Path=/; Domain=.music.163.com"
        )
    }

    fn scoped_sets(name: &str, value: &str) -> Vec<String> {
        SCOPES
            .iter()
            .map(|path| {
                format!(
                    "{name}={value}; Max-Age=2147483647; Expires=Mon, 22 Nov 2094 12:40:34 GMT; Path={path}; Domain=.music.163.com"
                )
            })
            .collect()
    }

    #[test]
    fn root_delete_next_to_scoped_sets_of_the_same_name_is_a_scope_migration_in_any_order() {
        // 实测：Path=/ 的删除与八个子路径的设值混在一个响应里，位置随机；不论删除在前、在后还是夹在中间，
        // MUSIC_A_T / MUSIC_R_T 都不能丢。
        let mut sets = scoped_sets("MUSIC_A_T", "a1");
        sets.extend(scoped_sets("MUSIC_R_T", "r1"));
        for position in [0, 5, sets.len()] {
            let mut headers = sets.clone();
            headers.insert(position, root_delete("MUSIC_A_T"));
            headers.insert(position, root_delete("MUSIC_R_T"));
            let result = merge_at(Some(SESSION), &headers, now());
            assert!(!result.changed, "位置 {position}");
            assert_eq!(result.cookie, SESSION, "位置 {position}");
        }
    }

    #[test]
    fn scope_migration_still_applies_the_new_value_in_place() {
        let mut headers = vec![root_delete("MUSIC_A_T")];
        headers.extend(scoped_sets("MUSIC_A_T", "a2"));
        headers.push(root_delete("MUSIC_A_T"));
        let result = merge_at(Some(SESSION), &headers, now());
        assert!(result.changed);
        assert_eq!(
            result.cookie,
            "MUSIC_A_T=a2; MUSIC_R_T=r1; __csrf=c1; NMTID=n1; MUSIC_R_U=ru1; MUSIC_U=u1"
        );
    }

    #[test]
    fn path_scoped_delete_never_removes_the_root_cookie() {
        for header in [
            "MUSIC_A_T=; Max-Age=0; Path=/api/feedback; Domain=.music.163.com",
            "MUSIC_A_T=; Max-Age=-1; path=/neapi/clientlog",
            "MUSIC_A_T=x; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/weapi/feedback",
        ] {
            let result = merge_at(Some(SESSION), &[header.to_owned()], now());
            assert!(!result.changed, "{header}");
            assert_eq!(result.cookie, SESSION);
        }
    }

    #[test]
    fn root_delete_without_a_same_name_set_still_deletes() {
        // 没写 Path 与显式 Path=/ 都算根路径；其他名字的设值不会让它免于删除。
        let result = merge_at(
            Some(SESSION),
            &[
                root_delete("MUSIC_A_T"),
                "MUSIC_R_T=; Max-Age=0".to_owned(),
                "__csrf=c2; Path=/".to_owned(),
            ],
            now(),
        );
        assert!(result.changed);
        assert_eq!(
            result.cookie,
            "__csrf=c2; NMTID=n1; MUSIC_R_U=ru1; MUSIC_U=u1"
        );
    }

    #[test]
    fn an_empty_value_is_not_a_set_so_it_does_not_cancel_a_delete() {
        let result = merge_at(
            Some(SESSION),
            &[
                "NMTID=; Path=/api/feedback".to_owned(),
                "NMTID=; Max-Age=0; Path=/".to_owned(),
            ],
            now(),
        );
        assert!(result.changed);
        assert_eq!(
            result.cookie,
            "MUSIC_A_T=a1; MUSIC_R_T=r1; __csrf=c1; MUSIC_R_U=ru1; MUSIC_U=u1"
        );
    }

    #[test]
    fn refresh_response_shape_rotates_tokens_and_ignores_unrelated_entries() {
        let far = "Max-Age=2147483647; Expires=Mon, 22 Nov 2094 12:46:05 GMT";
        let mut headers = vec![
            "MUSIC_U=u2; Max-Age=15552000; Expires=Mon, 03 May 2027 09:31:58 GMT; Path=/; Domain=.music.163.com".to_owned(),
            "MUSIC_R_U=ru2; Max-Age=15552000; Expires=Mon, 03 May 2027 09:31:58 GMT; Path=/eapi/login/token/refresh; Domain=.music.163.com".to_owned(),
            "MUSIC_R_U=ru2; Max-Age=15552000; Expires=Mon, 03 May 2027 09:31:58 GMT; Path=/api/login/token/refresh; Domain=.music.163.com".to_owned(),
            "MUSIC_SNS=; Max-Age=0; Expires=Wed, 04 Nov 2026 09:31:58 GMT; Path=/".to_owned(),
            "__csrf=c2; Max-Age=1296010; Expires=Thu, 19 Nov 2026 09:32:08 GMT; Path=/; Domain=.music.163.com".to_owned(),
        ];
        headers.extend(
            SCOPES
                .iter()
                .map(|path| format!("MUSIC_A_T=a1; {far}; Path={path}; Domain=.music.163.com")),
        );
        headers.extend(
            SCOPES
                .iter()
                .map(|path| format!("MUSIC_R_T=r1; {far}; Path={path}; Domain=.music.163.com")),
        );

        let result = merge_at(Some(SESSION), &headers, now());

        assert!(result.changed);
        // 三个令牌被换发，顺序不变；MUSIC_SNS 本来就没有，其余令牌保持原样。
        assert_eq!(
            result.cookie,
            "MUSIC_A_T=a1; MUSIC_R_T=r1; __csrf=c2; NMTID=n1; MUSIC_R_U=ru2; MUSIC_U=u2"
        );
    }

    #[test]
    fn http_date_parses_common_forms() {
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:10 GMT"), Some(10));
        assert_eq!(
            parse_http_date("Thursday, 01-Jan-70 00:00:10 GMT"),
            Some(10)
        );
        assert_eq!(parse_http_date("Thu Jan  1 00:00:10 1970"), Some(10));
        assert_eq!(
            parse_http_date("Fri, 02 Nov 2029 03:04:05 GMT"),
            Some(unix_from_civil(2029, 11, 2, 3, 4, 5))
        );
        assert_eq!(parse_http_date("not a date"), None);
    }

    #[test]
    fn unix_from_civil_matches_known_timestamps() {
        assert_eq!(unix_from_civil(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(unix_from_civil(2000, 3, 1, 0, 0, 0), 951_868_800);
        assert_eq!(unix_from_civil(2026, 11, 2, 12, 0, 0), 1_793_620_800);
    }
}
