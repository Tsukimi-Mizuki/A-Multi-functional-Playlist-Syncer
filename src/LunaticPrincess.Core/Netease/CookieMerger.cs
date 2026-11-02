using System.Globalization;

namespace LunaticPrincess.Core.Netease;

/// <summary>Cookie 合并结果：合并后的 Cookie 串，以及相对输入是否真的发生了变化。</summary>
public readonly record struct CookieMergeResult(string Cookie, bool Changed);

/// <summary>
/// 把服务端后续响应里的 Set-Cookie 合并进已保存的 Cookie 串（服务端会轮换 MUSIC_U，丢掉就会提前掉登录）。
/// 规则：按名字（忽略大小写）覆盖；Max-Age&lt;=0 或 Expires 已过期视为删除；空值不会覆盖已有值；
/// 已有键保持原顺序，新增键追加在末尾。
/// </summary>
public static class CookieMerger
{
    /// <summary>合并 Set-Cookie 列表到现有 Cookie 串。</summary>
    /// <param name="existingCookie">现有 "a=1; b=2" 形式的 Cookie 串，可为空。</param>
    /// <param name="setCookieHeaders">响应里的 Set-Cookie 头原文。</param>
    /// <param name="now">判断 Expires 是否过期的当前时间；默认取系统时间（测试可注入）。</param>
    public static CookieMergeResult Merge(
        string? existingCookie,
        IEnumerable<string>? setCookieHeaders,
        DateTimeOffset? now = null)
    {
        var original = existingCookie ?? string.Empty;
        var pairs = ParseCookieString(original);
        if (setCookieHeaders is null)
        {
            return new CookieMergeResult(original, false);
        }

        var current = now ?? DateTimeOffset.UtcNow;
        var changed = false;
        foreach (var header in setCookieHeaders)
        {
            if (!TryParseSetCookie(header, current, out var name, out var value, out var isDelete))
            {
                continue;
            }

            var index = pairs.FindIndex(pair => string.Equals(pair.Name, name, StringComparison.OrdinalIgnoreCase));
            if (isDelete)
            {
                if (index >= 0)
                {
                    pairs.RemoveAt(index);
                    changed = true;
                }

                continue;
            }

            if (string.IsNullOrEmpty(value))
            {
                // 空值不覆盖已有值，也不新增。
                continue;
            }

            if (index < 0)
            {
                pairs.Add((name, value));
                changed = true;
            }
            else if (!string.Equals(pairs[index].Value, value, StringComparison.Ordinal))
            {
                pairs[index] = (pairs[index].Name, value);
                changed = true;
            }
        }

        return changed
            ? new CookieMergeResult(string.Join("; ", pairs.Select(pair => $"{pair.Name}={pair.Value}")), true)
            : new CookieMergeResult(original, false);
    }

    /// <summary>解析 "a=1; b=2" 形式的 Cookie 串（忽略无法解析的片段；同名保留最后一个）。</summary>
    internal static List<(string Name, string Value)> ParseCookieString(string cookie)
    {
        var pairs = new List<(string Name, string Value)>();
        foreach (var part in cookie.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            var separator = part.IndexOf('=');
            if (separator <= 0)
            {
                continue;
            }

            var name = part[..separator].Trim();
            var value = part[(separator + 1)..].Trim();
            var existing = pairs.FindIndex(pair => string.Equals(pair.Name, name, StringComparison.OrdinalIgnoreCase));
            if (existing >= 0)
            {
                pairs[existing] = (pairs[existing].Name, value);
            }
            else
            {
                pairs.Add((name, value));
            }
        }

        return pairs;
    }

    private static bool TryParseSetCookie(
        string? header,
        DateTimeOffset now,
        out string name,
        out string value,
        out bool isDelete)
    {
        name = string.Empty;
        value = string.Empty;
        isDelete = false;
        if (string.IsNullOrWhiteSpace(header))
        {
            return false;
        }

        var segments = header.Split(';', StringSplitOptions.TrimEntries);
        var first = segments[0];
        var separator = first.IndexOf('=');
        if (separator <= 0)
        {
            return false;
        }

        name = first[..separator].Trim();
        value = first[(separator + 1)..].Trim();

        // Max-Age 优先于 Expires（RFC 6265）。
        int? maxAge = null;
        DateTimeOffset? expires = null;
        for (var index = 1; index < segments.Length; index++)
        {
            var attribute = segments[index];
            var attributeSeparator = attribute.IndexOf('=');
            if (attributeSeparator <= 0)
            {
                continue;
            }

            var attributeName = attribute[..attributeSeparator].Trim();
            var attributeValue = attribute[(attributeSeparator + 1)..].Trim();
            if (attributeName.Equals("Max-Age", StringComparison.OrdinalIgnoreCase)
                && long.TryParse(attributeValue, NumberStyles.Integer, CultureInfo.InvariantCulture, out var seconds))
            {
                maxAge = seconds <= 0 ? 0 : 1;
            }
            else if (attributeName.Equals("Expires", StringComparison.OrdinalIgnoreCase)
                     && DateTimeOffset.TryParse(
                         attributeValue,
                         CultureInfo.InvariantCulture,
                         DateTimeStyles.AssumeUniversal | DateTimeStyles.AdjustToUniversal,
                         out var parsed))
            {
                expires = parsed;
            }
        }

        isDelete = maxAge is not null ? maxAge == 0 : expires is { } expiresAt && expiresAt <= now;
        return true;
    }
}
