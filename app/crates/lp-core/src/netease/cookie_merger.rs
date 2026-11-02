//! 把服务端后续响应里的 `Set-Cookie` 合并进已保存的 Cookie 串。对应 C# 的 `CookieMerger`。
//!
//! 网易云会轮换 `MUSIC_U`，丢掉就会提前掉登录。规则：
//! - 按名字（忽略大小写）覆盖；
//! - `Max-Age <= 0` 或 `Expires` 已过期视为删除（`Max-Age` 优先于 `Expires`，RFC 6265）；
//! - 空值不会覆盖已有值，也不会新增；
//! - 已有键保持原顺序，新增键追加在末尾。

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

    for header in set_cookie_headers {
        let Some(parsed) = parse_set_cookie(header, now_unix) else {
            continue;
        };
        let index = pairs
            .iter()
            .position(|(name, _)| name.eq_ignore_ascii_case(&parsed.name));

        if parsed.is_delete {
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
