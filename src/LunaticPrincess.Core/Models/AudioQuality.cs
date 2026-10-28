namespace LunaticPrincess.Core.Models;

/// <summary>下载/试听音质，与网易云客户端的音质档位一一对应（从低到高）。</summary>
public enum AudioQuality
{
    /// <summary>标准（MP3 128k）。</summary>
    Standard = 0,

    /// <summary>极高 HQ（MP3 320k）。</summary>
    ExHigh = 1,

    /// <summary>无损 SQ（FLAC）。</summary>
    Lossless = 2,

    /// <summary>高解析度无损 Hi-Res（FLAC）。</summary>
    HiRes = 3,

    /// <summary>高清臻音 Spatial Audio。</summary>
    SpatialAudio = 4,

    /// <summary>沉浸环绕声 Surround Audio。</summary>
    SurroundAudio = 5,

    /// <summary>超清母带 Master。</summary>
    Master = 6,

    /// <summary>臻音全景声 Audio Vivid。</summary>
    AudioVivid = 7,

    /// <summary>杜比全景声 Dolby Atmos。</summary>
    DolbyAtmos = 8,
}

public static class AudioQualityExtensions
{
    /// <summary>请求播放地址时使用的 level 字符串（首选写法）。</summary>
    public static string ToApiLevel(this AudioQuality quality) => quality switch
    {
        AudioQuality.Standard => "standard",
        AudioQuality.ExHigh => "exhigh",
        AudioQuality.Lossless => "lossless",
        AudioQuality.HiRes => "hires",
        AudioQuality.SpatialAudio => "jyeffect",
        AudioQuality.SurroundAudio => "sky",
        AudioQuality.Master => "jymaster",
        AudioQuality.AudioVivid => "audio_vivid",
        AudioQuality.DolbyAtmos => "dolby",
        _ => "exhigh",
    };

    /// <summary>请求 level 字符串的候选写法：首个拿不到播放地址时依次退让（部分档位服务端有多个别名）。</summary>
    public static IReadOnlyList<string> ToApiLevels(this AudioQuality quality) => quality switch
    {
        AudioQuality.AudioVivid => ["audio_vivid", "vivid"],
        _ => [quality.ToApiLevel()],
    };

    /// <summary>经典接口（/weapi/song/enhance/player/url）使用的 br 数值。</summary>
    public static int ToBitrate(this AudioQuality quality) => quality switch
    {
        AudioQuality.Standard => 128_000,
        AudioQuality.ExHigh => 320_000,
        AudioQuality.Lossless => 999_000,
        _ => 1_999_000,
    };

    /// <summary>界面显示名称，与网易云客户端的音质命名一致。</summary>
    public static string ToDisplayName(this AudioQuality quality) => quality switch
    {
        AudioQuality.Standard => "标准",
        AudioQuality.ExHigh => "极高 (HQ)",
        AudioQuality.Lossless => "无损 (SQ)",
        AudioQuality.HiRes => "高解析度无损 (Hi-Res)",
        AudioQuality.SpatialAudio => "高清臻音 (SpatialAudio)",
        AudioQuality.SurroundAudio => "沉浸环绕声 (Surround Audio)",
        AudioQuality.Master => "超清母带 (Master)",
        AudioQuality.AudioVivid => "臻音全景声 (Audio Vivid)",
        AudioQuality.DolbyAtmos => "杜比全景声 (Dolby Atmos)",
        _ => "极高 (HQ)",
    };

    /// <summary>把接口返回的 level 字符串解析成音质档位；无法识别时返回 null。</summary>
    public static AudioQuality? FromApiLevel(this string? level) => level?.Trim().ToLowerInvariant() switch
    {
        "standard" => AudioQuality.Standard,
        "higher" or "exhigh" => AudioQuality.ExHigh,
        "lossless" => AudioQuality.Lossless,
        "hires" => AudioQuality.HiRes,
        "jyeffect" => AudioQuality.SpatialAudio,
        "sky" => AudioQuality.SurroundAudio,
        "jymaster" => AudioQuality.Master,
        "audio_vivid" or "vivid" => AudioQuality.AudioVivid,
        "dolby" => AudioQuality.DolbyAtmos,
        _ => null,
    };

    /// <summary>解析设置文件里的音质名称：兼容旧版 "Higher"（已并入极高）。</summary>
    public static bool TryParseSettingValue(string? value, out AudioQuality quality)
    {
        quality = default;
        if (string.IsNullOrWhiteSpace(value))
        {
            return false;
        }

        var trimmed = value.Trim();
        if (string.Equals(trimmed, "Higher", StringComparison.OrdinalIgnoreCase))
        {
            quality = AudioQuality.ExHigh;
            return true;
        }

        return Enum.TryParse(trimmed, ignoreCase: true, out quality) && Enum.IsDefined(quality);
    }
}
