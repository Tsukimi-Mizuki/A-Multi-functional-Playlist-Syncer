namespace NeteaseMusicManager.Core.Models;

/// <summary>下载/试听音质。网易云接口用 level 字符串和 br 数字两种表达方式。</summary>
public enum AudioQuality
{
    Standard = 0,
    Higher = 1,
    ExHigh = 2,
    Lossless = 3,
    HiRes = 4,
}

public static class AudioQualityExtensions
{
    public static string ToApiLevel(this AudioQuality quality) => quality switch
    {
        AudioQuality.Standard => "standard",
        AudioQuality.Higher => "higher",
        AudioQuality.ExHigh => "exhigh",
        AudioQuality.Lossless => "lossless",
        AudioQuality.HiRes => "hires",
        _ => "exhigh",
    };

    public static int ToBitrate(this AudioQuality quality) => quality switch
    {
        AudioQuality.Standard => 128_000,
        AudioQuality.Higher => 192_000,
        AudioQuality.ExHigh => 320_000,
        AudioQuality.Lossless => 999_000,
        AudioQuality.HiRes => 1_999_000,
        _ => 320_000,
    };

    public static string ToDisplayName(this AudioQuality quality) => quality switch
    {
        AudioQuality.Standard => "标准 128k",
        AudioQuality.Higher => "较高 192k",
        AudioQuality.ExHigh => "极高 320k",
        AudioQuality.Lossless => "无损 FLAC",
        AudioQuality.HiRes => "Hi-Res",
        _ => "极高 320k",
    };
}
