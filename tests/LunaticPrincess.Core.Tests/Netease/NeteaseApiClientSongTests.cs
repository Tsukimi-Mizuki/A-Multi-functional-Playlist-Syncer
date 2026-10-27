using System.Globalization;
using System.Net;
using System.Text.Json;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Netease;
using Xunit;

namespace LunaticPrincess.Core.Tests.Netease;

public sealed class NeteaseApiClientSongTests
{
    private const string FixedSecret = "0123456789abcdef";

    private const string FullSongJson = """
        {
          "code": 200,
          "songs": [
            {
              "id": 186016,
              "name": "晴天",
              "ar": [ { "id": 6452, "name": "周杰伦" } ],
              "al": {
                "id": 13628,
                "name": "叶惠美",
                "picUrl": "https://p1.music.126.net/cover.jpg",
                "publishTime": 1058284800000
              },
              "dt": 269000,
              "no": 1,
              "cd": "1",
              "alias": [ "Sunny Day" ],
              "publishTime": 1058284800000,
              "fee": 1
            }
          ],
          "privileges": [ { "id": 186016, "maxbr": 999000, "fee": 1 } ]
        }
        """;

    [Fact]
    public async Task GetSongDetailsAsync_MapsAllSongFields()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson(FullSongJson);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var songs = await client.GetSongDetailsAsync([186016L]);

        var song = Assert.Single(songs);
        Assert.Equal(186016, song.Id);
        Assert.Equal("晴天", song.Name);
        var artist = Assert.Single(song.Artists);
        Assert.Equal(6452, artist.Id);
        Assert.Equal("周杰伦", artist.Name);
        Assert.Equal("周杰伦", song.ArtistText);
        Assert.Equal(13628, song.Album.Id);
        Assert.Equal("叶惠美", song.Album.Name);
        Assert.Equal("https://p1.music.126.net/cover.jpg", song.CoverUrl);
        Assert.Equal("Sunny Day", song.Alias);
        Assert.Equal("晴天 (Sunny Day)", song.DisplayName);
        Assert.Equal(269000, song.DurationMs);
        Assert.Equal(TimeSpan.FromMilliseconds(269000), song.Duration);
        Assert.Equal(1, song.TrackNumber);
        Assert.Equal(1, song.DiscNumber);
        Assert.Equal(1, song.Fee);
        Assert.Equal(999000, song.MaxBitrate);
        var expectedDate = DateTimeOffset.FromUnixTimeMilliseconds(1058284800000)
            .ToLocalTime()
            .ToString("yyyy-MM-dd", CultureInfo.InvariantCulture);
        Assert.Equal(expectedDate, song.ReleaseDate);

        var request = Assert.Single(handler.Requests);
        Assert.Equal("/weapi/v3/song/detail", request.Uri.AbsolutePath);
        Assert.Equal(new long[] { 186016 }, TestJson.ParseSongIdsFromDetailRequest(request, FixedSecret));
    }

    [Fact]
    public async Task GetSongDetailsAsync_PreservesRequestedOrderDespiteResponseOrder()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueAlways(request => TestHttp.JsonResponse(
                TestJson.SongDetailResponseForIds(TestJson.ParseSongIdsFromDetailRequest(request, FixedSecret), reverse: true)));
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var songs = await client.GetSongDetailsAsync([3L, 1L, 2L]);

        Assert.Equal(new long[] { 3, 1, 2 }, songs.Select(song => song.Id));
        Assert.Single(handler.Requests);
    }

    [Fact]
    public async Task GetSongDetailsAsync_BatchesMoreThan500Ids()
    {
        var ids = Enumerable.Range(1, 600).Select(index => (long)index).ToArray();
        var handler = new FakeHttpMessageHandler()
            .EnqueueAlways(request => TestHttp.JsonResponse(
                TestJson.SongDetailResponseForIds(TestJson.ParseSongIdsFromDetailRequest(request, FixedSecret))));
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var songs = await client.GetSongDetailsAsync(ids);

        Assert.Equal(600, songs.Count);
        Assert.Equal(2, handler.Requests.Count);
        Assert.Equal(500, TestJson.ParseSongIdsFromDetailRequest(handler.Requests[0], FixedSecret).Length);
        Assert.Equal(100, TestJson.ParseSongIdsFromDetailRequest(handler.Requests[1], FixedSecret).Length);
    }

    [Fact]
    public async Task GetSongDetailsAsync_ReturnsEmptyForEmptyInput()
    {
        var handler = new FakeHttpMessageHandler();
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var songs = await client.GetSongDetailsAsync([]);

        Assert.Empty(songs);
        Assert.Empty(handler.Requests);
    }

    [Fact]
    public async Task GetSongPlaybackInfoAsync_MapsUrlMetadataAndTrialInfo()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson("""
            {
              "code": 200,
              "data": [
                {
                  "id": 33894312,
                  "url": "https://m701.music.126.net/song.mp3",
                  "br": 320000,
                  "size": 12345678,
                  "type": "mp3",
                  "level": "exhigh",
                  "fee": 1,
                  "freeTrialInfo": { "start": 0, "end": 45 },
                  "expi": 1200,
                  "code": 200
                }
              ]
            }
            """);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var info = await client.GetSongPlaybackInfoAsync(33894312, AudioQuality.ExHigh);

        Assert.True(info.IsSuccess);
        Assert.Equal(33894312, info.SongId);
        Assert.Equal("https://m701.music.126.net/song.mp3", info.Url);
        Assert.Equal(320000, info.Bitrate);
        Assert.Equal(12345678, info.Size);
        Assert.Equal("mp3", info.Format);
        Assert.Equal("exhigh", info.Level);
        Assert.Equal(1, info.Fee);
        Assert.True(info.IsTrial);
        Assert.Equal(0, info.TrialStartSeconds);
        Assert.Equal(45, info.TrialEndSeconds);
        Assert.Equal(1200, info.ExpiresInSeconds);
        Assert.Null(info.ErrorMessage);

        var request = Assert.Single(handler.Requests);
        Assert.Equal("/weapi/song/enhance/player/url/v1", request.Uri.AbsolutePath);
        using var payload = JsonDocument.Parse(TestHttp.DecryptBusinessJson(request, FixedSecret));
        Assert.Equal("[33894312]", payload.RootElement.GetProperty("ids").GetString());
        Assert.Equal("exhigh", payload.RootElement.GetProperty("level").GetString());
        Assert.Equal("flac", payload.RootElement.GetProperty("encodeType").GetString());
    }

    [Fact]
    public async Task GetSongPlaybackInfoAsync_FallsBackToLegacyEndpointWhenV1Fails()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueStatus(HttpStatusCode.InternalServerError)
            .EnqueueJson("""
                {
                  "code": 200,
                  "data": [ { "id": 33894312, "url": "https://m701.music.126.net/song.mp3", "br": 320000, "type": "mp3", "level": "exhigh", "code": 200 } ]
                }
                """);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var info = await client.GetSongPlaybackInfoAsync(33894312, AudioQuality.ExHigh);

        Assert.True(info.IsSuccess);
        Assert.Equal(320000, info.Bitrate);
        Assert.Equal(2, handler.Requests.Count);
        Assert.Equal("/weapi/song/enhance/player/url/v1", handler.Requests[0].Uri.AbsolutePath);
        Assert.Equal("/weapi/song/enhance/player/url", handler.Requests[1].Uri.AbsolutePath);
        using var payload = JsonDocument.Parse(TestHttp.DecryptBusinessJson(handler.Requests[1], FixedSecret));
        Assert.Equal(320000, payload.RootElement.GetProperty("br").GetInt32());
    }

    [Fact]
    public async Task GetSongPlaybackInfoAsync_ThrowsWhenNoUrlReturned()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson("""{"code":200,"data":[{"id":186016,"url":null,"code":404,"fee":0}]}""")
            .EnqueueJson("""{"code":200,"data":[{"id":186016,"url":null,"code":404,"fee":0}]}""");
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var exception = await Assert.ThrowsAsync<NeteaseApiException>(
            () => client.GetSongPlaybackInfoAsync(186016, AudioQuality.Standard));

        Assert.Contains("404", exception.Message);
        Assert.Equal(2, handler.Requests.Count);
    }

    [Fact]
    public async Task GetSongPlaybackInfoAsync_RetriesLowerQualityAndReportsActualLevel()
    {
        var handler = new FakeHttpMessageHandler()
            // v1 对新音质整体不可用（HTTP 404）→ 本歌曲剩余音质直接走经典接口。
            .EnqueueStatus(HttpStatusCode.NotFound)
            // 经典接口 lossless：无版权，url 为空。
            .EnqueueJson("""{"code":200,"data":[{"id":186016,"url":null,"code":404,"fee":1}]}""")
            // 降档到 exhigh：返回 320k。
            .EnqueueJson("""
                {
                  "code": 200,
                  "data": [ { "id": 186016, "url": "https://m701.music.126.net/song.mp3", "br": 320000, "type": "mp3", "level": "exhigh", "code": 200 } ]
                }
                """);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var info = await client.GetSongPlaybackInfoAsync(186016, AudioQuality.Lossless);

        Assert.True(info.IsSuccess);
        Assert.Equal("exhigh", info.Level);
        Assert.Equal(320000, info.Bitrate);
        Assert.Equal(3, handler.Requests.Count);
        using var firstLegacy = JsonDocument.Parse(TestHttp.DecryptBusinessJson(handler.Requests[1], FixedSecret));
        Assert.Equal(999000, firstLegacy.RootElement.GetProperty("br").GetInt32());
        using var secondLegacy = JsonDocument.Parse(TestHttp.DecryptBusinessJson(handler.Requests[2], FixedSecret));
        Assert.Equal(320000, secondLegacy.RootElement.GetProperty("br").GetInt32());
    }

    [Fact]
    public async Task GetSongPlaybackInfoAsync_ReturnsActualLowerLevelWithoutRetry()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson("""
            {
              "code": 200,
              "data": [ { "id": 186016, "url": "https://m701.music.126.net/song.flac", "br": 320000, "type": "flac", "level": "exhigh", "code": 200 } ]
            }
            """);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var info = await client.GetSongPlaybackInfoAsync(186016, AudioQuality.Lossless);

        Assert.True(info.IsSuccess);
        Assert.Equal("exhigh", info.Level);
        Assert.Equal(320000, info.Bitrate);
        Assert.Single(handler.Requests);
    }

    [Fact]
    public async Task GetSongDetailsAsync_PrefersPerSongPrivilegeOverTopLevelPrivileges()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson("""
            {
              "code": 200,
              "songs": [
                {
                  "id": 186016,
                  "name": "晴天",
                  "ar": [ { "id": 6452, "name": "周杰伦" } ],
                  "al": { "id": 13628, "name": "叶惠美", "picUrl": "https://p1/cover.jpg" },
                  "dt": 269000,
                  "fee": 1,
                  "privilege": { "maxbr": 320000, "fee": 1 }
                }
              ],
              "privileges": [ { "id": 186016, "maxbr": 999000, "fee": 0 } ]
            }
            """);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var songs = await client.GetSongDetailsAsync([186016L]);

        Assert.Equal(320000, Assert.Single(songs).MaxBitrate);
    }

    [Fact]
    public void GetQualityFallbackChain_StartsAtRequestedAndDescendsToStandard()
    {
        Assert.Equal(
            new[] { AudioQuality.HiRes, AudioQuality.Lossless, AudioQuality.ExHigh, AudioQuality.Higher, AudioQuality.Standard },
            NeteaseApiClient.GetQualityFallbackChain(AudioQuality.HiRes));
        Assert.Equal(
            new[] { AudioQuality.Lossless, AudioQuality.ExHigh, AudioQuality.Higher, AudioQuality.Standard },
            NeteaseApiClient.GetQualityFallbackChain(AudioQuality.Lossless));
        Assert.Equal(new[] { AudioQuality.Standard }, NeteaseApiClient.GetQualityFallbackChain(AudioQuality.Standard));
        Assert.Equal(
            new[] { AudioQuality.HiRes, AudioQuality.Lossless, AudioQuality.ExHigh, AudioQuality.Higher, AudioQuality.Standard },
            NeteaseApiClient.GetQualityFallbackChain((AudioQuality)99));
    }
}
