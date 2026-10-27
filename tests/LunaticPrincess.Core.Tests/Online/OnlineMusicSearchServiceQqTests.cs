using System.Net;
using System.Net.Http;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Online;
using LunaticPrincess.Core.Tests.Netease;
using LunaticPrincess.Core.Tests.Sync;
using Xunit;

namespace LunaticPrincess.Core.Tests.Online;

/// <summary>
/// QQ 音乐搜索回归测试：旧 <c>/soso/fcgi-bin/client_search_cp</c> 已被 QQ 下线（两个镜像域名都返回 HTTP 500，
/// 界面因此只显示“网络连接失败”），改用同目录的 <c>search_cp</c> 并兼容 new_json 字段。
/// </summary>
public sealed class OnlineMusicSearchServiceQqTests
{
    private const string SearchCpJson =
        """
        {"code":0,"data":{"keyword":"Lemon","song":{"curnum":1,"curpage":1,"totalnum":1,"list":[
          {"songname":"Lemon","songmid":"000akynZ2Rbro5","songid":213086592,
           "albumname":"レモン","albummid":"002OR8wD3Lo3E5","interval":256,"pubtime":1500000000,
           "singer":[{"name":"米津玄師"},{"name":"第二歌手"}]}
        ]}}}
        """;

    [Fact]
    public async Task SearchQq_UsesSearchCpEndpointAndParsesFields()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson(SearchCpJson);
        var service = CreateService(handler);

        var results = await service.SearchAsync(OnlineMusicSource.QQMusic, "Lemon");

        var request = Assert.Single(handler.Requests);
        Assert.Contains("/soso/fcgi-bin/search_cp", request.PathAndQuery, StringComparison.Ordinal);
        Assert.DoesNotContain("client_search_cp", request.PathAndQuery, StringComparison.Ordinal);
        Assert.Contains("new_json=1", request.PathAndQuery, StringComparison.Ordinal);
        Assert.Contains("w=Lemon", request.PathAndQuery, StringComparison.Ordinal);

        var song = Assert.Single(results);
        Assert.Equal(OnlineMusicSource.QQMusic, song.Source);
        Assert.Equal("000akynZ2Rbro5", song.SourceId);
        Assert.Equal("Lemon", song.Title);
        Assert.Equal("米津玄師/第二歌手", song.Artist);
        Assert.Equal("レモン", song.Album);
        Assert.Equal(256_000, song.DurationMs);
        Assert.Equal(2017u, song.Year);
        Assert.Equal("213086592", song.NumericSourceId);
        Assert.Contains("002OR8wD3Lo3E5", song.CoverUrl, StringComparison.Ordinal);
    }

    [Fact]
    public async Task SearchQq_FallsBackToSecondMirrorHost()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueStatus(HttpStatusCode.InternalServerError)
            .EnqueueJson(SearchCpJson);
        var service = CreateService(handler);

        var results = await service.SearchAsync(OnlineMusicSource.QQMusic, "Lemon");

        Assert.Equal(2, handler.Requests.Count);
        Assert.Equal("c.y.qq.com", handler.Requests[0].Host);
        Assert.Equal("c6.y.qq.com", handler.Requests[1].Host);
        Assert.Single(results);
    }

    [Fact]
    public async Task SearchQq_BothHostsFail_ReportsFriendlyReason()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueStatus(HttpStatusCode.InternalServerError)
            .EnqueueStatus(HttpStatusCode.InternalServerError);
        var service = CreateService(handler);

        var error = await Assert.ThrowsAsync<InvalidOperationException>(
            () => service.SearchAsync(OnlineMusicSource.QQMusic, "Lemon"));

        Assert.Contains("QQ 音乐搜索接口不可用", error.Message, StringComparison.Ordinal);
    }

    private static OnlineMusicSearchService CreateService(FakeHttpMessageHandler handler) =>
        new(new FakeApiForSync(), new HttpClient(handler), new LyricParser());
}
