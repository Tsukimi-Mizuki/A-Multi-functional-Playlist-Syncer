//! 网易云接口返回的 JSON 字段宽容读取。对应 C# 的 `NeteaseJson`。
//!
//! 接口字段类型不稳定（数字 / 字符串混用、字段缺失），这里统一做宽容读取，
//! 避免因个别字段类型漂移导致整条数据解析失败。

use serde_json::Value;

/// 读取对象属性；不是对象或属性不存在时返回 `None`（JSON `null` 仍然返回 `Some(Value::Null)`）。
pub fn property<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.as_object()?.get(name)
}

/// 读取字符串字段；数字 / 布尔会转成文本，`null` 视为缺失。
pub fn string_or_none(value: &Value, name: &str) -> Option<String> {
    scalar_string(property(value, name)?)
}

/// 读取 i64 字段（宽容数字 / 字符串）。
pub fn i64_or(value: &Value, name: &str, default: i64) -> i64 {
    property(value, name).and_then(as_i64).unwrap_or(default)
}

/// 读取 i32 字段（宽容数字 / 字符串）。
pub fn i32_or(value: &Value, name: &str, default: i32) -> i32 {
    property(value, name)
        .and_then(as_i64)
        .map(|n| n as i32)
        .unwrap_or(default)
}

/// 读取布尔字段；缺失或无法识别时返回 `None`（接受 `true` / `false` 与同名字符串）。
pub fn bool_or_none(value: &Value, name: &str) -> Option<bool> {
    match property(value, name)? {
        Value::Bool(flag) => Some(*flag),
        Value::String(text) if text.trim().eq_ignore_ascii_case("true") => Some(true),
        Value::String(text) if text.trim().eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

/// 把当前值按整数读取：数字（浮点截断）或可解析的字符串。
pub fn as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|float| float as i64)),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn integers_are_read_from_numbers_strings_and_floats() {
        let value = json!({"a": 5, "b": "7", "c": 3.9, "d": "x", "e": null});
        assert_eq!(i64_or(&value, "a", -1), 5);
        assert_eq!(i64_or(&value, "b", -1), 7);
        assert_eq!(i64_or(&value, "c", -1), 3);
        assert_eq!(i64_or(&value, "d", -1), -1);
        assert_eq!(i64_or(&value, "e", -1), -1);
        assert_eq!(i64_or(&value, "missing", -1), -1);
        assert_eq!(i32_or(&value, "b", 0), 7);
    }

    #[test]
    fn booleans_accept_bools_and_boolean_strings_only() {
        let value = json!({"a": true, "b": "False", "c": "yes", "d": 1, "e": null});
        assert_eq!(bool_or_none(&value, "a"), Some(true));
        assert_eq!(bool_or_none(&value, "b"), Some(false));
        assert_eq!(bool_or_none(&value, "c"), None);
        assert_eq!(bool_or_none(&value, "d"), None);
        assert_eq!(bool_or_none(&value, "e"), None);
        assert_eq!(bool_or_none(&value, "missing"), None);
    }

    #[test]
    fn strings_accept_scalars_and_treat_null_as_missing() {
        let value = json!({"a": "x", "b": 12, "c": true, "d": null, "e": {"k": 1}});
        assert_eq!(string_or_none(&value, "a").as_deref(), Some("x"));
        assert_eq!(string_or_none(&value, "b").as_deref(), Some("12"));
        assert_eq!(string_or_none(&value, "c").as_deref(), Some("true"));
        assert_eq!(string_or_none(&value, "d"), None);
        assert_eq!(string_or_none(&value, "e"), None);
        assert_eq!(string_or_none(&value, "missing"), None);
    }
}
