using System.Globalization;
using System.Text.Json;

namespace LunaticPrincess.Core.Netease;

/// <summary>
/// 网易云接口返回的 JSON 字段解析辅助。接口字段类型不稳定（数字/字符串混用、字段缺失），
/// 这里统一做宽容读取，避免因个别字段类型漂移导致整条数据解析失败。
/// </summary>
internal static class NeteaseJson
{
    /// <summary>读取对象属性；属性不存在或值为 undefined 时返回 null。</summary>
    public static JsonElement? GetPropertyOrNull(this JsonElement element, string name)
    {
        if (element.ValueKind != JsonValueKind.Object || !element.TryGetProperty(name, out var value))
        {
            return null;
        }

        return value.ValueKind == JsonValueKind.Undefined ? null : value;
    }

    /// <summary>读取字符串字段；数字/布尔会转成文本，null 视为缺失。</summary>
    public static string? GetStringOrNull(this JsonElement element, string name)
    {
        var value = element.GetPropertyOrNull(name);
        return value?.GetScalarStringOrNull();
    }

    /// <summary>读取整数数组/对象数组字段。</summary>
    public static JsonElement? GetArrayOrNull(this JsonElement element, string name)
    {
        var value = element.GetPropertyOrNull(name);
        return value is { ValueKind: JsonValueKind.Array } ? value : null;
    }

    /// <summary>读取布尔字段；缺失或非布尔时返回 null。</summary>
    public static bool? GetBoolOrNull(this JsonElement element, string name)
    {
        var value = element.GetPropertyOrNull(name);
        if (value is null)
        {
            return null;
        }

        return value.Value.ValueKind switch
        {
            JsonValueKind.True => true,
            JsonValueKind.False => false,
            JsonValueKind.String => bool.TryParse(value.Value.GetString(), out var parsed) ? parsed : null,
            _ => null,
        };
    }

    /// <summary>读取 int 字段（宽容数字/字符串）。</summary>
    public static int GetInt32(this JsonElement element, string name, int defaultValue = 0)
    {
        var value = element.GetPropertyOrNull(name);
        return value?.AsInt32(defaultValue) ?? defaultValue;
    }

    /// <summary>读取 long 字段（宽容数字/字符串）。</summary>
    public static long GetInt64(this JsonElement element, string name, long defaultValue = 0)
    {
        var value = element.GetPropertyOrNull(name);
        return value?.AsInt64(defaultValue) ?? defaultValue;
    }

    /// <summary>把当前元素按 int 读取。</summary>
    public static int AsInt32(this JsonElement element, int defaultValue = 0)
    {
        var value = element.AsInt64(long.MinValue);
        return value == long.MinValue ? defaultValue : (int)value;
    }

    /// <summary>把当前元素按 long 读取。</summary>
    public static long AsInt64(this JsonElement element, long defaultValue = 0)
    {
        switch (element.ValueKind)
        {
            case JsonValueKind.Number:
                if (element.TryGetInt64(out var integer))
                {
                    return integer;
                }

                return element.TryGetDouble(out var floating) ? (long)floating : defaultValue;
            case JsonValueKind.String:
                return long.TryParse(element.GetString(), NumberStyles.Integer, CultureInfo.InvariantCulture, out var parsed)
                    ? parsed
                    : defaultValue;
            default:
                return defaultValue;
        }
    }

    private static string? GetScalarStringOrNull(this JsonElement element)
    {
        return element.ValueKind switch
        {
            JsonValueKind.String => element.GetString(),
            JsonValueKind.Number => element.GetRawText(),
            JsonValueKind.True => "true",
            JsonValueKind.False => "false",
            _ => null,
        };
    }
}
