using LunaticPrincess.Core.Models;
using Xunit;

namespace LunaticPrincess.Core.Tests.Models;

public sealed class AudioQualityTests
{
    public static TheoryData<AudioQuality, string, string, int> MappingCases => new()
    {
        { AudioQuality.Standard, "standard", "标准", 128_000 },
        { AudioQuality.ExHigh, "exhigh", "极高 (HQ)", 320_000 },
        { AudioQuality.Lossless, "lossless", "无损 (SQ)", 999_000 },
        { AudioQuality.HiRes, "hires", "高解析度无损 (Hi-Res)", 1_999_000 },
        { AudioQuality.SpatialAudio, "jyeffect", "高清臻音 (SpatialAudio)", 1_999_000 },
        { AudioQuality.SurroundAudio, "sky", "沉浸环绕声 (Surround Audio)", 1_999_000 },
        { AudioQuality.Master, "jymaster", "超清母带 (Master)", 1_999_000 },
        { AudioQuality.AudioVivid, "audio_vivid", "臻音全景声 (Audio Vivid)", 1_999_000 },
        { AudioQuality.DolbyAtmos, "dolby", "杜比全景声 (Dolby Atmos)", 1_999_000 },
    };

    [Theory]
    [MemberData(nameof(MappingCases))]
    public void MapsToApiLevelDisplayNameAndBitrate(AudioQuality quality, string level, string displayName, int bitrate)
    {
        Assert.Equal(level, quality.ToApiLevel());
        Assert.Equal(displayName, quality.ToDisplayName());
        Assert.Equal(bitrate, quality.ToBitrate());
    }

    public static TheoryData<AudioQuality, string> LevelCases => new()
    {
        { AudioQuality.Standard, "standard" },
        { AudioQuality.ExHigh, "exhigh" },
        { AudioQuality.Lossless, "lossless" },
        { AudioQuality.HiRes, "hires" },
        { AudioQuality.SpatialAudio, "jyeffect" },
        { AudioQuality.SurroundAudio, "sky" },
        { AudioQuality.Master, "jymaster" },
        { AudioQuality.AudioVivid, "audio_vivid" },
        { AudioQuality.DolbyAtmos, "dolby" },
    };

    [Theory]
    [MemberData(nameof(LevelCases))]
    public void ApiLevelRoundTripsBackToQuality(AudioQuality quality, string level)
    {
        Assert.Equal(quality, level.FromApiLevel());
        Assert.Equal(quality, level.ToUpperInvariant().FromApiLevel());
    }

    [Fact]
    public void AudioVivid_OffersFallbackLevelSpellings()
    {
        Assert.Equal(new[] { "audio_vivid", "vivid" }, AudioQuality.AudioVivid.ToApiLevels());
        Assert.Equal(new[] { "dolby" }, AudioQuality.DolbyAtmos.ToApiLevels());
        Assert.Equal(new[] { "sky" }, AudioQuality.SurroundAudio.ToApiLevels());
    }

    [Fact]
    public void FromApiLevel_UnknownOrEmpty_ReturnsNull()
    {
        Assert.Null("".FromApiLevel());
        Assert.Null(((string?)null).FromApiLevel());
        Assert.Null("surround".FromApiLevel());
    }

    [Fact]
    public void FromApiLevel_LegacyHigher_MapsToExHigh()
    {
        Assert.Equal(AudioQuality.ExHigh, "higher".FromApiLevel());
    }

    [Theory]
    [InlineData("Higher", AudioQuality.ExHigh)]
    [InlineData("higher", AudioQuality.ExHigh)]
    [InlineData("Lossless", AudioQuality.Lossless)]
    [InlineData("dolbyatmos", AudioQuality.DolbyAtmos)]
    [InlineData("  Master  ", AudioQuality.Master)]
    public void TryParseSettingValue_AcceptsCurrentAndLegacyNames(string raw, AudioQuality expected)
    {
        Assert.True(AudioQualityExtensions.TryParseSettingValue(raw, out var quality));
        Assert.Equal(expected, quality);
    }

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData(null)]
    [InlineData("HigherBetter")]
    public void TryParseSettingValue_RejectsUnknownValues(string? raw)
    {
        Assert.False(AudioQualityExtensions.TryParseSettingValue(raw, out _));
    }
}
