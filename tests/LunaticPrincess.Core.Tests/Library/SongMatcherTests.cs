using LunaticPrincess.Core.Library;
using LunaticPrincess.Core.Models;
using Xunit;

namespace LunaticPrincess.Core.Tests.Library;

public sealed class SongMatcherTests
{
    private static NeteaseSong Song(
        long id,
        string name,
        string[] artists,
        int durationMs,
        string? alias = null) =>
        new()
        {
            Id = id,
            Name = name,
            Alias = alias,
            Artists = artists.Select((artist, index) => new ArtistRef(index + 1, artist)).ToArray(),
            Album = new AlbumRef(1, "专辑", null),
            DurationMs = durationMs,
            TrackNumber = 1,
        };

    private static LocalTrack Track(
        string path,
        string title,
        string? artist,
        long durationMs,
        bool isNcm = false,
        long? neteaseId = null) =>
        new()
        {
            Path = path,
            Title = title,
            Artist = artist,
            DurationMs = durationMs,
            Format = isNcm ? "flac" : "mp3",
            IsNcm = isNcm,
            NeteaseId = neteaseId,
        };

    [Fact]
    public void Match_PrefersExactNeteaseId_OverMetadata()
    {
        var song = Song(42, "晴天", new[] { "周杰伦" }, 269_000);
        var local = Track("C:\\music\\whatever.mp3", "完全不同", "别的歌手", 123_000, neteaseId: 42);

        var matches = new SongMatcher().Match(new[] { song }, new[] { local });

        var match = Assert.Single(matches);
        Assert.Same(song, match.Song);
        Assert.Same(local, match.LocalTrack);
        Assert.Equal(MatchMethod.NeteaseId, match.Method);
        Assert.Equal(1.0, match.Confidence);
        Assert.Equal(LocalAvailability.Available, match.Availability);
        Assert.True(match.IsPresent);
    }

    [Fact]
    public void Match_MarksEncryptedNcmAvailability_ForIdMatch()
    {
        var song = Song(7, "Lucky", new[] { "Jason Mraz" }, 245_000);
        var local = Track("C:\\music\\Lucky.ncm", "Lucky", "Jason Mraz", 245_000, isNcm: true, neteaseId: 7);

        var match = Assert.Single(new SongMatcher().Match(new[] { song }, new[] { local }));

        Assert.Equal(LocalAvailability.EncryptedNcm, match.Availability);
        Assert.Equal(MatchMethod.NeteaseId, match.Method);
    }

    [Fact]
    public void Match_UsesNormalizedFuzzyMetadata_FullWidthBracketsAndPunctuation()
    {
        var song = Song(1, "Sunny (Live)", new[] { "周杰伦" }, 240_000);
        var local = Track("C:\\music\\ＳＵＮＮＹ.flac", "ＳＵＮＮＹ（现场版）", "周杰伦", 243_000);

        var match = Assert.Single(new SongMatcher().Match(new[] { song }, new[] { local }));

        Assert.Equal(MatchMethod.Metadata, match.Method);
        Assert.True(match.Confidence >= SongMatcher.DefaultThreshold);
        Assert.Equal(LocalAvailability.Available, match.Availability);
    }

    [Fact]
    public void Match_StripsPunctuationAndCase_ForTitleComparison()
    {
        var song = Song(1, "Hello, World!", new[] { "Adele" }, 240_000);
        var local = Track("C:\\music\\x.mp3", "hello world", "adele", 241_000);

        var match = Assert.Single(new SongMatcher().Match(new[] { song }, new[] { local }));

        Assert.Equal(MatchMethod.Metadata, match.Method);
        Assert.True(match.Confidence > 0.95);
    }

    [Fact]
    public void Match_SplitsArtistNames_OnSlashesAndAmpersands()
    {
        var song = Song(1, "屋顶", new[] { "周杰伦", "温岚" }, 305_000);
        var local = Track("C:\\music\\x.mp3", "屋顶", "温岚/周杰伦", 305_000);

        var match = Assert.Single(new SongMatcher().Match(new[] { song }, new[] { local }));

        Assert.Equal(MatchMethod.Metadata, match.Method);
        Assert.True(match.Confidence >= 0.99);

        var song2 = Song(2, "Lucky", new[] { "Jason Mraz", "Colbie Caillat" }, 245_000);
        var local2 = Track("C:\\music\\y.mp3", "Lucky", "Jason Mraz & Colbie Caillat", 245_000);

        var match2 = Assert.Single(new SongMatcher().Match(new[] { song2 }, new[] { local2 }));
        Assert.Equal(MatchMethod.Metadata, match2.Method);
        Assert.True(match2.Confidence >= 0.99);
    }

    [Fact]
    public void Match_Rejects_DifferentArtist_WithLargeDurationGap()
    {
        var song = Song(1, "同名歌曲", new[] { "歌手甲" }, 200_000);
        var local = Track("C:\\music\\x.mp3", "同名歌曲", "歌手乙", 215_000);

        var match = Assert.Single(new SongMatcher().Match(new[] { song }, new[] { local }));

        Assert.False(match.IsPresent);
        Assert.Equal(MatchMethod.None, match.Method);
        Assert.Equal(LocalAvailability.Missing, match.Availability);
        Assert.Equal(0, match.Confidence);
    }

    [Fact]
    public void Match_RejectsCompletelyDifferentSongs()
    {
        var song = Song(1, "晴天", new[] { "周杰伦" }, 269_000);
        var local = Track("C:\\music\\x.mp3", "Lucky", "Jason Mraz", 245_000);

        var match = Assert.Single(new SongMatcher().Match(new[] { song }, new[] { local }));

        Assert.False(match.IsPresent);
    }

    [Fact]
    public void Match_IsOneToOne_WithGreedyBestDuration()
    {
        var song = Song(1, "晴天", new[] { "周杰伦" }, 269_000);
        var close = Track("C:\\music\\close.mp3", "晴天", "周杰伦", 269_000);
        var far = Track("C:\\music\\far.mp3", "晴天", "周杰伦", 260_000);

        var matches = new SongMatcher().Match(new[] { song }, new[] { far, close });

        var match = Assert.Single(matches);
        Assert.Same(close, match.LocalTrack);

        // 另一首本地曲不会被重复使用，第二首歌只能缺失。
        var song2 = Song(2, "晴天", new[] { "周杰伦" }, 269_000);
        var matches2 = new SongMatcher().Match(new[] { song, song2 }, new[] { far, close });
        Assert.Equal(2, matches2.Count);
        Assert.NotNull(matches2[0].LocalTrack);
        Assert.NotNull(matches2[1].LocalTrack);
        Assert.NotSame(matches2[0].LocalTrack, matches2[1].LocalTrack);
    }

    [Fact]
    public void Match_HandlesMoreSongsThanLocalTracks()
    {
        var songs = new[]
        {
            Song(1, "A", new[] { "X" }, 100_000),
            Song(2, "B", new[] { "Y" }, 200_000),
            Song(3, "C", new[] { "Z" }, 300_000),
        };
        var local = new[] { Track("C:\\music\\b.mp3", "B", "Y", 200_000) };

        var matches = new SongMatcher().Match(songs, local);

        Assert.Equal(3, matches.Count);
        Assert.False(matches[0].IsPresent);
        Assert.True(matches[1].IsPresent);
        Assert.False(matches[2].IsPresent);
        Assert.Equal(LocalAvailability.Missing, matches[0].Availability);
    }

    [Fact]
    public void Match_PreservesSongOrder_AndUsesAliasWhenHelpful()
    {
        var song1 = Song(1, "夜曲", new[] { "周杰伦" }, 226_000);
        var song2 = Song(2, "晴天", new[] { "周杰伦" }, 269_000, alias: "Sunny");
        var local1 = Track("C:\\music\\1.mp3", "晴天", "周杰伦", 269_000);
        var local2 = Track("C:\\music\\2.mp3", "夜曲", "周杰伦", 226_000);

        var matches = new SongMatcher().Match(new[] { song1, song2 }, new[] { local1, local2 });

        Assert.Equal(2, matches.Count);
        Assert.Same(song1, matches[0].Song);
        Assert.Same(song2, matches[1].Song);
        Assert.Same(local2, matches[0].LocalTrack);
        Assert.Same(local1, matches[1].LocalTrack);
    }

    [Fact]
    public void Match_HandlesEmptyInputs()
    {
        var matcher = new SongMatcher();
        Assert.Empty(matcher.Match(Array.Empty<NeteaseSong>(), Array.Empty<LocalTrack>()));
        Assert.Empty(matcher.Match(Array.Empty<NeteaseSong>(), new[] { Track("C:\\a.mp3", "a", "b", 1) }));
    }

    [Fact]
    public void Match_ToleratesDurationWithinTwelveSeconds()
    {
        var song = Song(1, "晴天", new[] { "周杰伦" }, 269_000);
        var local = Track("C:\\music\\x.mp3", "晴天", "周杰伦", 280_000);

        var match = Assert.Single(new SongMatcher().Match(new[] { song }, new[] { local }));

        Assert.True(match.IsPresent);
    }
}
