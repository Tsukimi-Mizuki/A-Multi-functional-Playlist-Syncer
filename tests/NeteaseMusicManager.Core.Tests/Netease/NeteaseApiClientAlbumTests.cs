using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Core.Netease;
using Xunit;

namespace NeteaseMusicManager.Core.Tests.Netease;

public sealed class NeteaseApiClientAlbumTests
{
    private const string FixedSecret = "0123456789abcdef";

    private const string AlbumJson = """
        {
          "code": 200,
          "album": {
            "id": 18875,
            "name": "依然范特西",
            "picUrl": "https://p1.music.126.net/album.jpg",
            "description": "周杰伦专辑",
            "publishTime": 1157040000000,
            "artists": [ { "id": 6452, "name": "周杰伦" } ]
          }
        }
        """;

    [Fact]
    public async Task GetAlbumAsync_MapsWeapiAlbum()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson(AlbumJson);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var album = await client.GetAlbumAsync(18875);

        Assert.NotNull(album);
        Assert.Equal(18875, album.Id);
        Assert.Equal("依然范特西", album.Name);
        Assert.Equal("https://p1.music.126.net/album.jpg", album.CoverUrl);
        Assert.Equal("周杰伦专辑", album.Description);
        Assert.NotNull(album.ReleaseDate);
        var artist = Assert.Single(album.Artists);
        Assert.Equal(6452, artist.Id);
        Assert.Equal("周杰伦", artist.Name);

        var request = Assert.Single(handler.Requests);
        Assert.Equal("/weapi/v1/album/18875", request.Uri.AbsolutePath);
        Assert.Equal(HttpMethod.Post, request.Method);
    }

    [Fact]
    public async Task GetAlbumAsync_FallsBackToPublicApiWhenWeapiFails()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson("""{"code":500,"message":"weapi 异常"}""")
            .EnqueueJson(AlbumJson);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var album = await client.GetAlbumAsync(18875);

        Assert.NotNull(album);
        Assert.Equal("依然范特西", album.Name);
        Assert.Equal(2, handler.Requests.Count);
        Assert.Equal("/weapi/v1/album/18875", handler.Requests[0].Uri.AbsolutePath);
        Assert.Equal(HttpMethod.Get, handler.Requests[1].Method);
        Assert.Equal("/api/album/18875", handler.Requests[1].Uri.AbsolutePath);
    }

    [Fact]
    public async Task GetAlbumAsync_ReturnsNullWhenAlbumMissing()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson("""{"code":200,"msg":"没有专辑字段"}""")
            .EnqueueJson("""{"code":200}""");
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var album = await client.GetAlbumAsync(18875);

        Assert.Null(album);
    }

    [Fact]
    public async Task GetAlbumAsync_ReturnsNullForInvalidIdWithoutRequests()
    {
        var handler = new FakeHttpMessageHandler();
        var client = TestHttp.CreateClient(handler, FixedSecret);

        Assert.Null(await client.GetAlbumAsync(0));
        Assert.Null(await client.GetAlbumAsync(-1));
        Assert.Empty(handler.Requests);
    }

    [Fact]
    public async Task GetAlbumAsync_ThrowsWhenBothEndpointsFail()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson("""{"code":500,"message":"weapi 异常"}""")
            .EnqueueJson("""{"code":-462,"message":"Cheating"}""");
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var exception = await Assert.ThrowsAsync<NeteaseApiException>(() => client.GetAlbumAsync(18875));

        Assert.Equal(500, exception.Code);
        Assert.Contains("-462", exception.Message);
    }
}
