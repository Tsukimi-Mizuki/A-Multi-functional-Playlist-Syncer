using LunaticPrincess.Core.Online;
using Xunit;

namespace LunaticPrincess.Core.Tests.Online;

public sealed class LyricsSourceKindTests
{
    [Theory]
    [InlineData(LyricsSourceKind.Netease, "网易云音乐", false)]
    [InlineData(LyricsSourceKind.QQMusic, "QQ 音乐", false)]
    [InlineData(LyricsSourceKind.Kugou, "酷狗音乐（仅歌词）", true)]
    [InlineData(LyricsSourceKind.Lrclib, "LRCLIB（仅歌词）", true)]
    public void DisplayLabelAndFallbackFlag(LyricsSourceKind kind, string label, bool isFallback)
    {
        Assert.Equal(label, kind.ToDisplayLabel());
        Assert.Equal(isFallback, kind.IsFallback());
    }

    [Theory]
    [InlineData(LyricsSourceKind.Netease, OnlineMusicSource.Netease)]
    [InlineData(LyricsSourceKind.QQMusic, OnlineMusicSource.QQMusic)]
    public void PrimaryKindMapsToMusicSource(LyricsSourceKind kind, OnlineMusicSource expected)
    {
        Assert.Equal(expected, kind.ToMusicSource());
    }

    [Theory]
    [InlineData(LyricsSourceKind.Kugou)]
    [InlineData(LyricsSourceKind.Lrclib)]
    public void FallbackKindHasNoMusicSource(LyricsSourceKind kind)
    {
        Assert.Null(kind.ToMusicSource());
    }
}
