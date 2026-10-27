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

        public StubOnlineService(OnlineSongResult song) => _song = song;

        public Task<IReadOnlyList<OnlineSongResult>> SearchAsync(
            OnlineMusicSource source,
            string keyword,
            int limit = 20,
            CancellationToken cancellationToken = default) =>
            Task.FromResult<IReadOnlyList<OnlineSongResult>>(new[] { _song });

        public Task<OnlineSongDetails?> GetDetailsAsync(
            OnlineSongResult song,
            CancellationToken cancellationToken = default) =>
            Task.FromResult<OnlineSongDetails?>(new OnlineSongDetails(song, "[00:01.00]晴天", null));
    }
}
