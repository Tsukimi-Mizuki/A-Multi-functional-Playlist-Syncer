using System.Net;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Online;
using LunaticPrincess.Core.Tests.Netease;
using Xunit;

namespace LunaticPrincess.Core.Tests.Online;

/// <summary>LRCLIB 词源：同步歌词选取、全文检索回退、时长硬过滤与伴奏/失败兜底。</summary>
public sealed class LrclibLyricsSourceTests
{
    private const string LemonEntry =
        "{\"trackName\":\"Lemon\",\"artistName\":\"米津玄師\",\"albumName\":\"Lemon\",\"duration\":255.2,\"instrumental\":false,\"syncedLyrics\":\"[00:01.00]夢ならば\\n[00:02.00]どれほどよかったでしょう\"}";

    [Fact]
    public async Task FetchAsync_RetriesAfterTransientFailure()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueStatus(HttpStatusCode.ServiceUnavailable)
            .EnqueueJson(SearchJson(LemonEntry));
        using var client = new HttpClient(handler);
        var source = new LrclibLyricsSource(client, retryDelay: TimeSpan.Zero);

        var candidate = await source.FetchAsync(
            new[] { "Lemon 米津玄師" },
            "Lemon",
            "米津玄師",
            null,
            255_000,
            LyricsMatchScorer.DefaultMinScore);

        Assert.NotNull(candidate);
        Assert.Equal(2, handler.Requests.Count);
        Assert.Equal(handler.Requests[0].PathAndQuery, handler.Requests[1].PathAndQuery, StringComparer.Ordinal);
    }

    [Fact]
    public async Task FetchAsync_SearchesByFullTextQueryFirst()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson(SearchJson(LemonEntry));
        using var client = new HttpClient(handler);
        var source = new LrclibLyricsSource(client);

        var candidate = await source.FetchAsync(
            new[] { "Lemon 米津玄師" },
            "Lemon",
            "米津玄師",
            "Lemon",
            255_000,
            LyricsMatchScorer.DefaultMinScore);

        Assert.NotNull(candidate);
        Assert.Equal("LRCLIB", candidate!.SourceLabel);
        Assert.False(candidate.HasWordLyrics);
        Assert.False(candidate.HasTranslation);
        Assert.Contains("[00:01.000]夢ならば", candidate.LineLyrics, StringComparison.Ordinal);
        Assert.Contains("q=Lemon", handler.Requests[0].PathAndQuery, StringComparison.Ordinal);
        Assert.Single(handler.Requests);
    }

    [Fact]
    public async Task FetchAsync_FallsBackToTrackNameQuery()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson("[]")
            .EnqueueJson(SearchJson(LemonEntry));
        using var client = new HttpClient(handler);
        var source = new LrclibLyricsSource(client);

        var candidate = await source.FetchAsync(
            new[] { "Lemon 米津玄師" },
            "Lemon",
            "米津玄師",
            "Lemon",
            255_000,
            LyricsMatchScorer.DefaultMinScore);

        Assert.NotNull(candidate);
        Assert.Equal(2, handler.Requests.Count);
        Assert.Contains("q=Lemon", handler.Requests[0].PathAndQuery, StringComparison.Ordinal);
        Assert.Contains("track_name=Lemon", handler.Requests[1].PathAndQuery, StringComparison.Ordinal);
        Assert.Contains("artist_name=", handler.Requests[1].PathAndQuery, StringComparison.Ordinal);
    }

    [Fact]
    public async Task FetchAsync_TriesEveryQueryVariant()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson("[]")
            .EnqueueJson(SearchJson(LemonEntry));
        using var client = new HttpClient(handler);
        var source = new LrclibLyricsSource(client);

        var candidate = await source.FetchAsync(
            new[] { "Lemon 米津玄師", "Lemon" },
            "Lemon",
            "米津玄師",
            null,
            255_000,
            LyricsMatchScorer.DefaultMinScore);

        Assert.NotNull(candidate);
        Assert.Equal(2, handler.Requests.Count);
        Assert.Contains("q=Lemon%20", handler.Requests[0].PathAndQuery, StringComparison.Ordinal);
        Assert.EndsWith("q=Lemon", Uri.UnescapeDataString(handler.Requests[1].PathAndQuery), StringComparison.Ordinal);
    }

    [Fact]
    public async Task FetchAsync_RejectsDurationMismatch()
    {
        var handler = new FakeHttpMessageHandler().EnqueueAlways(_ => TestHttp.JsonResponse(SearchJson(
            "{\"trackName\":\"Lemon\",\"artistName\":\"米津玄師\",\"duration\":400.0,\"instrumental\":false,\"syncedLyrics\":\"[00:01.00]別の版\"}")));
        using var client = new HttpClient(handler);
        var source = new LrclibLyricsSource(client);

        var candidate = await source.FetchAsync(
            new[] { "Lemon 米津玄師" },
            "Lemon",
            "米津玄師",
            null,
            255_000,
            LyricsMatchScorer.DefaultMinScore);

        Assert.Null(candidate);
    }

    [Fact]
    public async Task FetchAsync_SkipsInstrumentalAndUnsyncedEntries()
    {
        var handler = new FakeHttpMessageHandler().EnqueueAlways(_ => TestHttp.JsonResponse(SearchJson(
            "{\"trackName\":\"Lemon\",\"artistName\":\"米津玄師\",\"duration\":255.0,\"instrumental\":true,\"syncedLyrics\":\"[00:01.00]伴奏\"}," +
            "{\"trackName\":\"Lemon\",\"artistName\":\"米津玄師\",\"duration\":255.0,\"instrumental\":false,\"syncedLyrics\":null}")));
        using var client = new HttpClient(handler);
        var source = new LrclibLyricsSource(client);

        var candidate = await source.FetchAsync(
            new[] { "Lemon 米津玄師" },
            "Lemon",
            "米津玄師",
            null,
            255_000,
            LyricsMatchScorer.DefaultMinScore);

        Assert.Null(candidate);
    }

    [Fact]
    public async Task FetchAsync_ReturnsNullWhenRequestsFail()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueAlways(_ => new HttpResponseMessage(HttpStatusCode.ServiceUnavailable)
            {
                Content = new StringContent(string.Empty),
            });
        using var client = new HttpClient(handler);
        var source = new LrclibLyricsSource(client);

        var candidate = await source.FetchAsync(
            new[] { "Lemon 米津玄師" },
            "Lemon",
            "米津玄師",
            null,
            255_000,
            LyricsMatchScorer.DefaultMinScore);

        Assert.Null(candidate);
    }

    private static string SearchJson(params string[] entries) => "[" + string.Join(",", entries) + "]";
}
