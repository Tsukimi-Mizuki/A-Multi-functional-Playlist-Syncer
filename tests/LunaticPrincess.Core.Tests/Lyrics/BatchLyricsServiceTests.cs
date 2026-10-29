using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Online;
using LunaticPrincess.Core.Tags;
using Xunit;

namespace LunaticPrincess.Core.Tests.Lyrics;

/// <summary>批量匹配打分：标题并列写法切段、歌手多值切段（MusicTag / LDDC 思路）。</summary>
public sealed class BatchLyricsServiceTests : IDisposable
{
    private readonly TempDirectory _temp = new("batch-lyrics");

    public void Dispose() => _temp.Dispose();

    [Fact]
    public async Task FetchForFileAsync_MatchesSplitTitleAndMultipleArtists()
    {
        var path = CreateFile("split.mp3");
        var tags = new StubTagService(new AudioTagInfo(
            Title: "晴天 / 叶惠美",
            Artist: "周杰伦/杨瑞代",
            Album: null,
            AlbumArtist: null,
            DurationMs: 269_000,
            Bitrate: 320_000));
        var online = new StubOnlineService(new OnlineSongResult(
            OnlineMusicSource.Netease,
            "186016",
            "晴天",
            "周杰伦",
            "叶惠美",
            269_500,
            2003,
            3));
        var service = new BatchLyricsService(tags, online);

        var result = await service.FetchForFileAsync(
            path,
            new BatchLyricsOptions(OverwriteExisting: true, FetchLyrics: true));

        Assert.True(result.Success, result.Message);
        Assert.Equal("晴天", result.MatchedTitle);
        Assert.Equal("周杰伦", result.MatchedArtist);
        Assert.Contains("标签", result.Message);
        Assert.NotNull(tags.Written);
        Assert.Equal("晴天", tags.Written!.Title);
        Assert.Contains("周杰伦", tags.Written.Artists);
    }

    [Fact]
    public async Task FetchForFileAsync_RejectsUnrelatedCandidate()
    {
        var path = CreateFile("unrelated.mp3");
        var tags = new StubTagService(new AudioTagInfo(
            Title: "完全无关的歌曲",
            Artist: "陌生歌手",
            Album: null,
            AlbumArtist: null,
            DurationMs: 200_000,
            Bitrate: 320_000));
        var online = new StubOnlineService(new OnlineSongResult(
            OnlineMusicSource.Netease,
            "186016",
            "晴天",
            "周杰伦",
            "叶惠美",
            269_500,
            2003,
            3));
        var service = new BatchLyricsService(tags, online);

        var result = await service.FetchForFileAsync(path, new BatchLyricsOptions());

        Assert.False(result.Success);
        Assert.Contains("未找到匹配度足够的在线歌曲", result.Message);
        Assert.Null(tags.Written);
    }

    [Fact]
    public async Task FetchForFileAsync_WritesTagsAndLyricsForExactMatch()
    {
        var path = CreateFile("exact.mp3");
        var tags = new StubTagService(new AudioTagInfo(
            Title: "晴天",
            Artist: "周杰伦",
            Album: null,
            AlbumArtist: null,
            DurationMs: 269_000,
            Bitrate: 320_000));
        var online = new StubOnlineService(new OnlineSongResult(
            OnlineMusicSource.Netease,
            "186016",
            "晴天",
            "周杰伦",
            "叶惠美",
            269_000,
            2003,
            3));
        var service = new BatchLyricsService(tags, online);

        var result = await service.FetchForFileAsync(
            path,
            new BatchLyricsOptions(OverwriteExisting: true, FetchLyrics: true));

        Assert.True(result.Success, result.Message);
        Assert.NotNull(tags.Written);
        Assert.Contains("标签", result.Message);
        Assert.Contains("歌词", result.Message);
        Assert.False(string.IsNullOrWhiteSpace(tags.Written!.PlainLyrics));
    }

    [Fact]
    public async Task FetchForFileAsync_UsesFallbackLyricsWhenOnlineHasNoLyrics()
    {
        var path = CreateFile("fallback-lyrics.mp3");
        var tags = new StubTagService(new AudioTagInfo(
            Title: "Lemon",
            Artist: "米津玄師",
            Album: "Lemon",
            AlbumArtist: null,
            DurationMs: 255_000,
            Bitrate: 320_000,
            HasCover: true));
        var online = new StubOnlineService(
            new OnlineSongResult(
                OnlineMusicSource.Netease,
                "2051316826",
                "Lemon",
                "米津玄師",
                "Lemon",
                255_000,
                2018,
                1),
            lyrics: null);
        var fallback = new StubFallbackSource(
            "LRCLIB",
            new LyricsSourceCandidate(
                "LRCLIB",
                "Lemon",
                "米津玄師",
                "Lemon",
                255_000,
                LineLyrics: "[00:01.00]夢ならば"));
        var service = new BatchLyricsService(tags, online, new[] { fallback });

        var result = await service.FetchForFileAsync(
            path,
            new BatchLyricsOptions(OverwriteExisting: false, FetchLyrics: true));

        Assert.True(result.Success, result.Message);
        Assert.Contains("逐行歌词", result.Message);
        Assert.Contains("歌词来源：LRCLIB", result.Message);
        Assert.NotNull(tags.Written);
        Assert.Equal("[00:01.00]夢ならば", tags.Written!.PlainLyrics);
        Assert.NotEmpty(fallback.SearchedQueries);
    }

    [Fact]
    public async Task FetchForFileAsync_UsesFallbackSourceWhenOnlineSearchMisses()
    {
        var path = CreateFile("fallback-only.mp3");
        var tags = new StubTagService(new AudioTagInfo(
            Title: "冷门曲子",
            Artist: "同人歌手",
            Album: null,
            AlbumArtist: null,
            DurationMs: 200_000,
            Bitrate: 320_000));
        var online = new StubOnlineService(
            new OnlineSongResult(
                OnlineMusicSource.Netease,
                "186016",
                "晴天",
                "周杰伦",
                "叶惠美",
                269_500,
                2003,
                3),
            lyrics: null);
        var broken = new ThrowingFallbackSource();
        var fallback = new StubFallbackSource(
            "酷狗音乐",
            new LyricsSourceCandidate(
                "酷狗音乐",
                "冷门曲子",
                "同人歌手",
                "同人专辑",
                200_500,
                WordLyrics: "[00:01.00]<00:01.00>冷<00:02.00>门"));
        var service = new BatchLyricsService(tags, online, new ILyricsSource[] { broken, fallback });

        var result = await service.FetchForFileAsync(
            path,
            new BatchLyricsOptions(FetchLyrics: true, PreferWordLyrics: true));

        Assert.True(result.Success, result.Message);
        Assert.Equal("酷狗音乐", result.SourceLabel);
        Assert.Equal("逐字歌词", result.LyricsFormat);
        Assert.Contains("逐字歌词", result.Message);
        Assert.NotNull(tags.Written);
        Assert.Null(tags.Written!.Title);
        Assert.Equal("同人专辑", tags.Written.Album);
        Assert.Equal("[00:01.00]<00:01.00>冷<00:02.00>门", tags.Written.WordLyrics);
    }

    [Fact]
    public async Task FetchForFileAsync_DoesNotConsultFallbackWhenOnlineLyricsExist()
    {
        var path = CreateFile("online-first.mp3");
        var tags = new StubTagService(new AudioTagInfo(
            Title: "晴天",
            Artist: "周杰伦",
            Album: "叶惠美",
            AlbumArtist: null,
            DurationMs: 269_000,
            Bitrate: 320_000));
        var online = new StubOnlineService(new OnlineSongResult(
            OnlineMusicSource.Netease,
            "186016",
            "晴天",
            "周杰伦",
            "叶惠美",
            269_500,
            2003,
            3));
        var fallback = new StubFallbackSource(
            "LRCLIB",
            new LyricsSourceCandidate("LRCLIB", "晴天", "周杰伦", "叶惠美", 269_000, LineLyrics: "[00:01.00]别的歌词"));
        var service = new BatchLyricsService(tags, online, new[] { fallback });

        var result = await service.FetchForFileAsync(
            path,
            new BatchLyricsOptions(OverwriteExisting: true, FetchLyrics: true));

        Assert.True(result.Success, result.Message);
        Assert.Empty(fallback.SearchedQueries);
        Assert.Equal("[00:01.00]晴天", tags.Written!.PlainLyrics);
    }

    private string CreateFile(string fileName)
    {
        var path = _temp.File(fileName);
        File.WriteAllBytes(path, new byte[] { 0x00 });
        return path;
    }

    private sealed class StubTagService : IAudioTagService
    {
        private readonly AudioTagInfo _info;

        public StubTagService(AudioTagInfo info) => _info = info;

        public AudioTagData? Written { get; private set; }

        public Task<AudioTagInfo?> ReadAsync(string audioPath, CancellationToken cancellationToken = default) =>
            Task.FromResult<AudioTagInfo?>(_info);

        public Task WriteAsync(string audioPath, AudioTagData tags, CancellationToken cancellationToken = default)
        {
            Written = tags;
            return Task.CompletedTask;
        }
    }

    private sealed class StubOnlineService : IOnlineMusicSearchService
    {
        private readonly OnlineSongResult _song;
        private readonly string? _lyrics;

        public StubOnlineService(OnlineSongResult song, string? lyrics = "[00:01.00]晴天")
        {
            _song = song;
            _lyrics = lyrics;
        }

        public Task<IReadOnlyList<OnlineSongResult>> SearchAsync(
            OnlineMusicSource source,
            string keyword,
            int limit = 20,
            CancellationToken cancellationToken = default) =>
            Task.FromResult<IReadOnlyList<OnlineSongResult>>(new[] { _song });

        public Task<OnlineSongDetails?> GetDetailsAsync(
            OnlineSongResult song,
            CancellationToken cancellationToken = default) =>
            Task.FromResult<OnlineSongDetails?>(new OnlineSongDetails(song, _lyrics, null));
    }


    [Fact]
    public async Task FetchForFileAsync_HonoursFallbackSourceFilter()
    {
        var path = CreateFile("filtered-fallback.mp3");
        var tags = new StubTagService(new AudioTagInfo(
            Title: "冷门曲子",
            Artist: "同人歌手",
            Album: null,
            AlbumArtist: null,
            DurationMs: 200_000,
            Bitrate: 320_000));
        var online = new StubOnlineService(
            new OnlineSongResult(OnlineMusicSource.Netease, "1", "晴天", "周杰伦", "叶惠美", 269_500, 2003, 3),
            lyrics: null);
        var kugou = new StubFallbackSource(
            "酷狗音乐",
            new LyricsSourceCandidate("酷狗音乐", "冷门曲子", "同人歌手", "同人专辑", 200_000, WordLyrics: "[00:01.00]<00:01.00>冷"));
        var lrclib = new StubFallbackSource(
            "LRCLIB",
            new LyricsSourceCandidate("LRCLIB", "冷门曲子", "同人歌手", "同人专辑", 200_000, LineLyrics: "[00:01.00]冷"));
        var service = new BatchLyricsService(tags, online, new ILyricsSource[] { kugou, lrclib })
        {
            // 设置里关掉酷狗音乐后，兜底应直接跳到 LRCLIB。
            FallbackSourceFilter = source => !string.Equals(source.Label, "酷狗音乐", StringComparison.Ordinal),
        };

        var result = await service.FetchForFileAsync(path, new BatchLyricsOptions(FetchLyrics: true));

        Assert.True(result.Success, result.Message);
        Assert.Contains("歌词来源：LRCLIB", result.Message);
        Assert.Empty(kugou.SearchedQueries);
        Assert.NotEmpty(lrclib.SearchedQueries);
        Assert.Equal("[00:01.00]冷", tags.Written!.PlainLyrics);
    }

    [Fact]
    public async Task FetchForFileAsync_WritesNoLyricsWhenEveryFallbackSourceDisabled()
    {
        var path = CreateFile("disabled-fallback.mp3");
        var tags = new StubTagService(new AudioTagInfo(
            Title: "冷门曲子",
            Artist: "同人歌手",
            Album: null,
            AlbumArtist: null,
            DurationMs: 200_000,
            Bitrate: 320_000));
        var online = new StubOnlineService(
            new OnlineSongResult(OnlineMusicSource.Netease, "1", "冷门曲子", "同人歌手", "同人专辑", 200_000, 2020, 1),
            lyrics: null);
        var kugou = new StubFallbackSource(
            "酷狗音乐",
            new LyricsSourceCandidate("酷狗音乐", "冷门曲子", "同人歌手", "同人专辑", 200_000, WordLyrics: "[00:01.00]<00:01.00>冷"));
        var lrclib = new StubFallbackSource(
            "LRCLIB",
            new LyricsSourceCandidate("LRCLIB", "冷门曲子", "同人歌手", "同人专辑", 200_000, LineLyrics: "[00:01.00]冷"));
        var service = new BatchLyricsService(tags, online, new ILyricsSource[] { kugou, lrclib })
        {
            FallbackSourceFilter = _ => false,
        };

        var result = await service.FetchForFileAsync(path, new BatchLyricsOptions(FetchLyrics: true));

        Assert.True(result.Success, result.Message);
        Assert.Contains("未获取到歌词", result.Message);
        Assert.Empty(kugou.SearchedQueries);
        Assert.Empty(lrclib.SearchedQueries);
        Assert.Null(tags.Written!.PlainLyrics);
    }

    private sealed class StubFallbackSource : ILyricsSource
    {
        private readonly LyricsSourceCandidate? _candidate;

        public StubFallbackSource(string label, LyricsSourceCandidate? candidate)
        {
            Label = label;
            _candidate = candidate;
        }

        public string Label { get; }

        public List<string> SearchedQueries { get; } = new();

        public Task<LyricsSourceCandidate?> FetchAsync(
            IReadOnlyList<string> queries,
            string title,
            string artist,
            string? album,
            long durationMs,
            double minScore,
            CancellationToken cancellationToken = default)
        {
            SearchedQueries.AddRange(queries);
            return Task.FromResult(_candidate);
        }
    }

    private sealed class ThrowingFallbackSource : ILyricsSource
    {
        public string Label => "故障源";

        public Task<LyricsSourceCandidate?> FetchAsync(
            IReadOnlyList<string> queries,
            string title,
            string artist,
            string? album,
            long durationMs,
            double minScore,
            CancellationToken cancellationToken = default) =>
            throw new HttpRequestException("模拟词源故障");
    }
}
