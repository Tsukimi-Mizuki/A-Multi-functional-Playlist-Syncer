using System.Net;
using System.Text.Json;
using NeteaseMusicManager.Core.Netease;
using Xunit;

namespace NeteaseMusicManager.Core.Tests.Netease;

public sealed class NeteaseApiClientLyricTests
{
    private const string FixedSecret = "0123456789abcdef";

    private const string LyricJson = """
        {
          "code": 200,
          "lrc": { "version": 10, "lyric": "[00:00.000]作词 : 测试\n[00:12.570]第一句\n" },
          "tlyric": { "version": 0, "lyric": "[00:12.570]First line\n" },
          "romalrc": { "version": 0, "lyric": "[00:12.570]romaji\n" },
          "yrc": { "version": 3, "lyric": "[12580,3470](12580,250,0)第(12830,300,0)一(13130,370,0)句\n" }
        }
        """;

    [Fact]
    public async Task GetLyricsAsync_MapsAllLyricSectionsFromPublicEndpoint()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson(LyricJson);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var lyrics = await client.GetLyricsAsync(33894312);

        Assert.Contains("[00:12.570]第一句", lyrics.Lrc);
        Assert.Contains("First line", lyrics.TranslatedLrc);
        Assert.Contains("romaji", lyrics.RomanizedLrc);
        Assert.Contains("(12580,250,0)", lyrics.Yrc);

        var request = Assert.Single(handler.Requests);
        Assert.Equal(HttpMethod.Get, request.Method);
        Assert.Equal("music.163.com", request.Host);
        Assert.Equal("/api/song/lyric", request.Uri.AbsolutePath);
        Assert.Contains("id=33894312", request.Uri.Query);
        Assert.Contains("yv=1", request.Uri.Query);
        Assert.Contains("lv=-1", request.Uri.Query);
        Assert.Contains("tv=-1", request.Uri.Query);
        Assert.Equal("os=pc; appver=8.9.70", request.CookieHeader);
    }

    [Fact]
    public async Task GetLyricsAsync_FallsBackToWeapiWhenPublicEndpointFails()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueStatus(HttpStatusCode.InternalServerError)
            .EnqueueJson(LyricJson);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var lyrics = await client.GetLyricsAsync(33894312);

        Assert.NotEmpty(lyrics.Lrc!);
        Assert.Equal(2, handler.Requests.Count);
        var weapiRequest = handler.Requests[1];
        Assert.Equal(HttpMethod.Post, weapiRequest.Method);
        Assert.Equal("/weapi/song/lyric", weapiRequest.Uri.AbsolutePath);

        using var payload = JsonDocument.Parse(TestHttp.DecryptBusinessJson(weapiRequest, FixedSecret));
        Assert.Equal(33894312, payload.RootElement.GetProperty("id").GetInt64());
        Assert.Equal(1, payload.RootElement.GetProperty("yv").GetInt32());
        Assert.Equal(-1, payload.RootElement.GetProperty("tv").GetInt32());
        Assert.Equal(0, payload.RootElement.GetProperty("kv").GetInt32());
        Assert.Equal(string.Empty, payload.RootElement.GetProperty("csrf_token").GetString());
    }

    [Fact]
    public async Task GetLyricsAsync_FallsBackToWeapiV1WhenPreviousSourcesFail()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueStatus(HttpStatusCode.InternalServerError)
            .EnqueueJson("""{"code":500,"message":"接口异常"}""")
            .EnqueueJson(LyricJson);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var lyrics = await client.GetLyricsAsync(33894312);

        Assert.NotEmpty(lyrics.Yrc!);
        Assert.Equal(3, handler.Requests.Count);
        Assert.Equal("/weapi/song/lyric/v1", handler.Requests[2].Uri.AbsolutePath);
    }

    [Fact]
    public async Task GetLyricsAsync_ReturnsEmptyRawLyricsForNolyric()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson("""{"code":200,"nolyric":true}""");
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var lyrics = await client.GetLyricsAsync(186016);

        Assert.Null(lyrics.Lrc);
        Assert.Null(lyrics.TranslatedLrc);
        Assert.Null(lyrics.RomanizedLrc);
        Assert.Null(lyrics.Yrc);
        Assert.Single(handler.Requests);
    }

    [Fact]
    public async Task GetLyricsAsync_ThrowsWhenAllSourcesFail()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueStatus(HttpStatusCode.InternalServerError)
            .EnqueueStatus(HttpStatusCode.InternalServerError)
            .EnqueueStatus(HttpStatusCode.InternalServerError);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var exception = await Assert.ThrowsAsync<NeteaseApiException>(() => client.GetLyricsAsync(186016));

        Assert.Contains("歌词", exception.Message);
        Assert.Equal(3, handler.Requests.Count);
    }
}
