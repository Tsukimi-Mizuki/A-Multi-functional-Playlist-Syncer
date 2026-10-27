using System.Text.Json.Serialization;

namespace LunaticPrincess.Core.Models;

/// <summary>登录态。Cookie 是调用网易云 weapi 的唯一凭证，必须持久化。</summary>
public sealed class NeteaseSession
{
    [JsonPropertyName("cookie")]
    public string Cookie { get; init; } = string.Empty;

    [JsonPropertyName("nickname")]
    public string? Nickname { get; init; }

    [JsonPropertyName("userId")]
    public long UserId { get; init; }

    [JsonPropertyName("vipType")]
    public int VipType { get; init; }

    [JsonPropertyName("avatarUrl")]
    public string? AvatarUrl { get; init; }

    [JsonPropertyName("loggedInAt")]
    public DateTimeOffset LoggedInAt { get; init; } = DateTimeOffset.Now;

    [JsonIgnore]
    public bool IsLoggedIn => !string.IsNullOrWhiteSpace(Cookie);

    [JsonIgnore]
    public string DisplayName => string.IsNullOrWhiteSpace(Nickname) ? "网易云用户" : Nickname!;

    [JsonIgnore]
    public string CsrfToken => GetCookieValue("__csrf") ?? string.Empty;

    public string? GetCookieValue(string name)
    {
        if (string.IsNullOrWhiteSpace(Cookie))
        {
            return null;
        }

        foreach (var part in Cookie.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            var index = part.IndexOf('=');
            if (index <= 0)
            {
                continue;
            }

            if (string.Equals(part[..index], name, StringComparison.OrdinalIgnoreCase))
            {
                return part[(index + 1)..];
            }
        }

        return null;
    }
}
