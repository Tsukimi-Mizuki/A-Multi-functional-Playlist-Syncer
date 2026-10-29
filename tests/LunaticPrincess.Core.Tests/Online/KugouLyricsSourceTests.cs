using System.Net;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Online;
using LunaticPrincess.Core.Tests.Lyrics;
using LunaticPrincess.Core.Tests.Netease;
using Xunit;

namespace LunaticPrincess.Core.Tests.Online;

/// <summary>酷狗词源：hash → 候选搜索 → KRC 下载三段链路、逐字/译文转换与匹配度过滤。</summary>
public sealed class KugouLyricsSourceTests
{
    private const string HashJson = "{\"status\":1,\"data\":{\"info\":[{\"hash\":\"HASH01\"}]}}";

    [Fact]
    public async Task FetchAsync_ReturnsWordLyricsWithTranslation()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(HashJson)
            .EnqueueJson(CandidatesJson("cid1", "Lemon", "米津玄師", 255))
            .EnqueueJson(DownloadJson(BuildKrc()));
        using var client = new HttpClient(handler);
        var source = new KugouLyricsSource(client);

        var candidate = await source.FetchAsync(
            new[] { "Lemon 米津玄師" },
            "Lemon",
            "米津玄師",
            "Lemon",
            255_000,
            LyricsMatchScorer.DefaultMinScore);

        Assert.NotNull(candidate);
        Assert.Equal("酷狗音乐", candidate!.SourceLabel);
        Assert.Equal("Lemon", candidate.Title);
        Assert.Equal("米津玄師", candidate.Artist);
        Assert.True(candidate.HasWordLyrics);
        Assert.True(candidate.HasTranslation);
        Assert.Contains("夢", candidate.WordLyrics, StringComparison.Ordinal);
        Assert.Contains("如果这一切都是梦境该有多好", candidate.BilingualLineLyrics, StringComparison.Ordinal);
        Assert.Contains("如果这一切都是梦境该有多好", candidate.BilingualWordLyrics, StringComparison.Ordinal);

        Assert.Equal(3, handler.Requests.Count);
        Assert.Equal("mobilecdn.kugou.com", handler.Requests[0].Host);
        Assert.Contains("keyword=Lemon", handler.Requests[0].PathAndQuery, StringComparison.Ordinal);
        Assert.Equal("krcs.kugou.com", handler.Requests[1].Host);
        Assert.Contains("hash=HASH01", handler.Requests[1].PathAndQuery, StringComparison.Ordinal);
        Assert.Contains("duration=255", handler.Requests[1].PathAndQuery, StringComparison.Ordinal);
        Assert.Equal("lyrics.kugou.com", handler.Requests[2].Host);
        Assert.Contains("fmt=krc", handler.Requests[2].PathAndQuery, StringComparison.Ordinal);
        Assert.Contains("id=cid1", handler.Requests[2].PathAndQuery, StringComparison.Ordinal);
    }

    [Fact]
    public async Task FetchAsync_RejectsCandidateBelowThreshold()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(HashJson)
            .EnqueueJson(CandidatesJson("cid9", "完全不同的歌", "别的歌手", 255));
        using var client = new HttpClient(handler);
        var source = new KugouLyricsSource(client);

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
    public async Task FetchAsync_RejectsCandidateWithMismatchedDuration()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(HashJson)
            .EnqueueJson(CandidatesJson("cid2", "Lemon", "米津玄師", 100));
        using var client = new HttpClient(handler);
        var source = new KugouLyricsSource(client);

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
    public async Task FetchAsync_FallsBackToLrcWhenKrcCannotBeDecoded()
    {
        var lrcBase64 = Convert.ToBase64String(System.Text.Encoding.UTF8.GetBytes("[00:01.00]Lemon\n[00:02.00]夢ならば"));
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(HashJson)
            .EnqueueJson(CandidatesJson("cid3", "Lemon", "米津玄師", 255))
            .EnqueueJson("{\"status\":200,\"fmt\":\"krc\",\"content\":\"" + Convert.ToBase64String(new byte[32]) + "\"}")
            .EnqueueJson("{\"status\":200,\"fmt\":\"lrc\",\"content\":\"" + lrcBase64 + "\"}");
        using var client = new HttpClient(handler);
        var source = new KugouLyricsSource(client);

        var candidate = await source.FetchAsync(
            new[] { "Lemon 米津玄師" },
            "Lemon",
            "米津玄師",
            null,
            255_000,
            LyricsMatchScorer.DefaultMinScore);

        Assert.NotNull(candidate);
        Assert.False(candidate!.HasWordLyrics);
        Assert.Contains("夢ならば", candidate.LineLyrics, StringComparison.Ordinal);
        Assert.Equal(4, handler.Requests.Count);
        Assert.Contains("fmt=lrc", handler.Requests[3].PathAndQuery, StringComparison.Ordinal);
    }

    [Fact]
    public async Task FetchAsync_ReturnsNullWhenRequestsFail()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueAlways(_ => new HttpResponseMessage(HttpStatusCode.InternalServerError)
            {
                Content = new StringContent(string.Empty),
            });
        using var client = new HttpClient(handler);
        var source = new KugouLyricsSource(client);

        var candidate = await source.FetchAsync(
            new[] { "Lemon 米津玄師" },
            "Lemon",
            "米津玄師",
            null,
            255_000,
            LyricsMatchScorer.DefaultMinScore);

        Assert.Null(candidate);
    }

    private static string CandidatesJson(string id, string song, string singer, int duration) =>
        "{\"status\":200,\"candidates\":[{" +
        "\"id\":\"" + id + "\"," +
        "\"accesskey\":\"ak-" + id + "\"," +
        "\"song\":\"" + song + "\"," +
        "\"singer\":\"" + singer + "\"," +
        "\"duration\":" + duration +
        "}]}";

    private static string DownloadJson(string krcText) =>
        "{\"status\":200,\"fmt\":\"krc\",\"content\":\"" + KrcFixture.EncryptToBase64(krcText) + "\"}";

    private static string BuildKrc() => string.Join("\n",
        "[ti:Lemon]",
        KrcFixture.LanguageHeader("如果这一切都是梦境该有多好"),
        "[1000,3000]<0,800,0>夢<800,900,0>ならば<1700,1300,0>いいのに");
}
