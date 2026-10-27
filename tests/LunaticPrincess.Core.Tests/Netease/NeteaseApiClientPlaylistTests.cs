using System.Text;
using System.Text.Json;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Netease;
using Xunit;

namespace LunaticPrincess.Core.Tests.Netease;

public sealed class NeteaseApiClientPlaylistTests
{
    private const string FixedSecret = "0123456789abcdef";
    private const string TrackJson = """{"id":22,"name":"歌曲22","ar":[{"id":1,"name":"歌手"}],"al":{"id":2,"name":"专辑","picUrl":"https://p1/cover.jpg"},"dt":2000,"fee":0}""";

    [Fact]
    public async Task GetUserPlaylistsAsync_MapsSummaryFields()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson("""
            {
              "code": 200,
              "more": false,
              "playlist": [
                {
                  "id": 1001,
                  "name": "我喜欢的音乐",
                  "coverImgUrl": "https://p1.music.126.net/cover.jpg",
                  "trackCount": 25,
                  "creator": { "nickname": "小明" },
                  "description": "我的收藏"
                },
                { "id": 1002, "name": "第二个歌单", "coverImgUrl": null, "trackCount": 3, "creator": null }
              ]
            }
            """);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var playlists = await client.GetUserPlaylistsAsync(42);

        Assert.Equal(2, playlists.Count);
        Assert.Equal(1001, playlists[0].Id);
        Assert.Equal("我喜欢的音乐", playlists[0].Name);
        Assert.Equal("https://p1.music.126.net/cover.jpg", playlists[0].CoverUrl);
        Assert.Equal(25, playlists[0].TrackCount);
        Assert.Equal("小明", playlists[0].CreatorNickname);
        Assert.Equal("我的收藏", playlists[0].Description);
        Assert.Equal(3, playlists[1].TrackCount);
        Assert.Null(playlists[1].CreatorNickname);

        var request = Assert.Single(handler.Requests);
        Assert.Equal("/weapi/user/playlist", request.Uri.AbsolutePath);
        var payload = TestHttp.DecryptBusinessJson(request, FixedSecret);
        using var payloadDocument = JsonDocument.Parse(payload);
        Assert.Equal(42, payloadDocument.RootElement.GetProperty("uid").GetInt64());
        Assert.Equal(1000, payloadDocument.RootElement.GetProperty("limit").GetInt32());
        Assert.Equal(0, payloadDocument.RootElement.GetProperty("offset").GetInt32());
        Assert.False(payloadDocument.RootElement.GetProperty("includeVideo").GetBoolean());
        Assert.Equal(string.Empty, payloadDocument.RootElement.GetProperty("csrf_token").GetString());
    }

    [Fact]
    public async Task GetUserPlaylistsAsync_PaginatesWhenPageIsFull()
    {
        var firstPage = BuildPlaylistPage(1000, startId: 1, more: true);
        var secondPage = BuildPlaylistPage(1, startId: 1001, more: false);
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(firstPage)
            .EnqueueJson(secondPage);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var playlists = await client.GetUserPlaylistsAsync(1);

        Assert.Equal(1001, playlists.Count);
        Assert.Equal(2, handler.Requests.Count);
        using var secondPayload = JsonDocument.Parse(TestHttp.DecryptBusinessJson(handler.Requests[1], FixedSecret));
        Assert.Equal(1000, secondPayload.RootElement.GetProperty("offset").GetInt32());
    }

    [Fact]
    public async Task GetPlaylistDetailAsync_FetchesMissingTracksInTrackIdsOrder()
    {
        var detailJson = """
            {
              "code": 200,
              "playlist": {
                "id": 100,
                "name": "测试歌单",
                "coverImgUrl": "https://p1/cover.jpg",
                "description": "描述",
                "trackCount": 3,
                "creator": { "nickname": "创建者" },
                "tracks": [TRACK_JSON],
                "trackIds": [ { "id": 11 }, { "id": 22 }, { "id": 33 } ]
              },
              "privileges": [ { "id": 22, "maxbr": 999000, "fee": 1 } ]
            }
            """.Replace("TRACK_JSON", TrackJson);
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(detailJson)
            .EnqueueAlways(request => TestHttp.JsonResponse(
                TestJson.SongDetailResponseForIds(TestJson.ParseSongIdsFromDetailRequest(request, FixedSecret))));
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var detail = await client.GetPlaylistDetailAsync(100);

        Assert.Equal(100, detail.Id);
        Assert.Equal("测试歌单", detail.Name);
        Assert.Equal("https://p1/cover.jpg", detail.CoverUrl);
        Assert.Equal("描述", detail.Description);
        Assert.Equal("创建者", detail.CreatorNickname);
        Assert.Equal(3, detail.TrackCount);
        Assert.Equal(new long[] { 11, 22, 33 }, detail.TrackIds);
        Assert.Equal(new long[] { 11, 22, 33 }, detail.Songs.Select(song => song.Id));
        Assert.False(detail.IsPartial);
        // 内嵌曲目关联顶层 privileges 的 maxbr；批量补齐的曲目关联各自响应里的 privileges。
        Assert.Equal(320000, detail.Songs[0].MaxBitrate);
        Assert.Equal(999000, detail.Songs[1].MaxBitrate);
        Assert.Equal(320000, detail.Songs[2].MaxBitrate);

        Assert.Equal(2, handler.Requests.Count);
        var batchRequest = handler.Requests[1];
        Assert.Equal("/weapi/v3/song/detail", batchRequest.Uri.AbsolutePath);
        Assert.Equal(new long[] { 11, 33 }, TestJson.ParseSongIdsFromDetailRequest(batchRequest, FixedSecret));
    }

    [Fact]
    public async Task GetPlaylistDetailAsync_BatchesDetailRequestsAt500AndPreservesOrder()
    {
        const int totalTracks = 1200;
        var handler = new FakeHttpMessageHandler()
            .EnqueueAlways(request => TestHttp.JsonResponse(
                TestJson.SongDetailResponseForIds(TestJson.ParseSongIdsFromDetailRequest(request, FixedSecret))));
        handler.EnqueueJson(BuildLargePlaylistJson(totalTracks));
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var detail = await client.GetPlaylistDetailAsync(1);

        Assert.Equal(totalTracks, detail.Songs.Count);
        Assert.Equal(Enumerable.Range(1, totalTracks).Select(index => (long)index), detail.Songs.Select(song => song.Id));
        Assert.False(detail.IsPartial);

        var batchRequests = handler.Requests.Skip(1).ToArray();
        Assert.Equal(3, batchRequests.Length);
        Assert.Equal(500, TestJson.ParseSongIdsFromDetailRequest(batchRequests[0], FixedSecret).Length);
        Assert.Equal(500, TestJson.ParseSongIdsFromDetailRequest(batchRequests[1], FixedSecret).Length);
        Assert.Equal(200, TestJson.ParseSongIdsFromDetailRequest(batchRequests[2], FixedSecret).Length);
    }

    [Fact]
    public async Task GetPlaylistDetailAsync_MarksPartialWhenMissingTracksCannotBeRecovered()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson("""
                {
                  "code": 200,
                  "playlist": {
                    "id": 7,
                    "name": "部分歌单",
                    "trackCount": 2,
                    "tracks": [],
                    "trackIds": [ { "id": 11 }, { "id": 22 } ]
                  }
                }
                """)
            .EnqueueJson("""{"code":500,"message":"接口异常"}""");
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var detail = await client.GetPlaylistDetailAsync(7);

        Assert.True(detail.IsPartial);
        Assert.Empty(detail.Songs);
        Assert.Equal(new long[] { 11, 22 }, detail.TrackIds);
    }

    [Fact]
    public async Task GetPlaylistDetailAsync_ThrowsWhenPlaylistPayloadMissing()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson("""{"code":200,"msg":"not found"}""");
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var exception = await Assert.ThrowsAsync<NeteaseApiException>(() => client.GetPlaylistDetailAsync(999));

        Assert.Equal(404, exception.Code);
    }

    [Fact]
    public async Task GetPlaylistDetailAsync_ThrowsWhenBusinessCodeIsNot200()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson("""{"code":404,"msg":"歌单不存在"}""");
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var exception = await Assert.ThrowsAsync<NeteaseApiException>(() => client.GetPlaylistDetailAsync(999));

        Assert.Equal(404, exception.Code);
    }

    private static string BuildPlaylistPage(int count, int startId, bool more)
    {
        var builder = new StringBuilder();
        builder.Append("{\"code\":200,\"more\":").Append(more ? "true" : "false").Append(",\"playlist\":[");
        for (var index = 0; index < count; index++)
        {
            if (index > 0)
            {
                builder.Append(',');
            }

            var id = startId + index;
            builder.Append("{\"id\":").Append(id)
                .Append(",\"name\":\"歌单").Append(id)
                .Append("\",\"coverImgUrl\":null,\"trackCount\":").Append(index)
                .Append('}');
        }

        builder.Append("]}");
        return builder.ToString();
    }

    private static string BuildLargePlaylistJson(int trackCount)
    {
        var builder = new StringBuilder();
        builder.Append("{\"code\":200,\"playlist\":{\"id\":1,\"name\":\"大歌单\",\"trackCount\":")
            .Append(trackCount)
            .Append(",\"tracks\":[],\"trackIds\":[");
        for (var index = 1; index <= trackCount; index++)
        {
            if (index > 1)
            {
                builder.Append(',');
            }

            builder.Append("{\"id\":").Append(index).Append('}');
        }

        builder.Append("]}}");
        return builder.ToString();
    }
}
