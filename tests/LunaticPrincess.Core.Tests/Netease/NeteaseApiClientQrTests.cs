using System.Net;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Netease;
using Xunit;

namespace LunaticPrincess.Core.Tests.Netease;

public sealed class NeteaseApiClientQrTests
{
    private const string AccountJson = """
        {
          "code": 200,
          "account": { "id": 42, "userName": "test@example.com" },
          "profile": {
            "userId": 42,
            "nickname": "测试用户",
            "avatarUrl": "https://p1.music.126.net/avatar.jpg",
            "vipType": 1
          }
        }
        """;

    [Fact]
    public async Task CreateQrLoginAsync_UsesDesktopType3AndMapsQrContent()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson("""{"code":200,"unikey":"abcDEF123456"}""")
            .EnqueueJson("""{"code":801}""");
        var client = TestHttp.CreateClient(handler);
        var before = DateTimeOffset.Now;

        var start = await client.CreateQrLoginAsync();

        Assert.Equal("abcDEF123456", start.Key);
        Assert.Equal("https://music.163.com/login?codekey=abcDEF123456", start.QrContent);
        Assert.InRange(start.ExpiresAt, before.AddSeconds(178), DateTimeOffset.Now.AddSeconds(181));

        var createRequest = Assert.Single(handler.Requests);
        Assert.Equal(HttpMethod.Post, createRequest.Method);
        Assert.Equal("interface.music.163.com", createRequest.Host);
        Assert.Equal("/api/login/qrcode/unikey", createRequest.Uri.AbsolutePath);
        Assert.Equal("3", createRequest.GetForm("type"));
        Assert.Contains("NeteaseMusicDesktop", createRequest.GetHeader("User-Agent"));
        Assert.Equal("https://music.163.com/", createRequest.GetHeader("Referer"));

        // 轮询必须回到出码时的 host 和 type。
        var status = await client.PollQrLoginAsync(start.Key);

        Assert.Equal(QrLoginState.Waiting, status.State);
        Assert.Equal(2, handler.Requests.Count);
        var pollRequest = handler.Requests[1];
        Assert.Equal("interface.music.163.com", pollRequest.Host);
        Assert.Equal("/api/login/qrcode/client/login", pollRequest.Uri.AbsolutePath);
        Assert.Contains("key=abcDEF123456", pollRequest.Uri.Query);
        Assert.Contains("type=3", pollRequest.Uri.Query);
    }

    [Fact]
    public async Task CreateQrLoginAsync_FallsBackToWebType1WhenDesktopEndpointFails()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueStatus(HttpStatusCode.InternalServerError)
            .EnqueueJson("""{"code":200,"unikey":"webKey"}""")
            .EnqueueJson("""{"code":801}""");
        var client = TestHttp.CreateClient(handler);

        var start = await client.CreateQrLoginAsync();
        var status = await client.PollQrLoginAsync(start.Key);

        Assert.Equal("webKey", start.Key);
        Assert.Equal(QrLoginState.Waiting, status.State);
        Assert.Equal(3, handler.Requests.Count);
        Assert.Equal("interface.music.163.com", handler.Requests[0].Host);
        Assert.Equal("music.163.com", handler.Requests[1].Host);
        Assert.Equal("1", handler.Requests[1].GetForm("type"));
        Assert.Equal("music.163.com", handler.Requests[2].Host);
        Assert.Contains("type=1", handler.Requests[2].Uri.Query);
    }

    [Theory]
    [InlineData(800, QrLoginState.Expired)]
    [InlineData(801, QrLoginState.Waiting)]
    [InlineData(802, QrLoginState.Scanned)]
    [InlineData(999, QrLoginState.Failed)]
    public async Task PollQrLoginAsync_MapsServerCodes(int code, QrLoginState expected)
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson($$"""{"code":{{code}},"message":"x"}""");
        var client = TestHttp.CreateClient(handler);

        var status = await client.PollQrLoginAsync("some-key");

        Assert.Equal(expected, status.State);
        Assert.False(string.IsNullOrWhiteSpace(status.Message));
        Assert.Null(status.Session);
    }

    [Theory]
    [InlineData(800, true)]
    [InlineData(801, false)]
    [InlineData(802, false)]
    [InlineData(803, true)]
    public async Task QrLoginStatus_IsTerminalForFinalStates(int code, bool terminal)
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson($$"""{"code":{{code}},"cookie":"MUSIC_U=u; __csrf=c"}""")
            .EnqueueJson(AccountJson, HttpStatusCode.OK);
        var client = TestHttp.CreateClient(handler);

        var status = await client.PollQrLoginAsync("key-1");

        Assert.Equal(terminal, status.IsTerminal);
    }

    [Fact]
    public async Task PollQrLoginAsync_Confirmed_MergesSetCookieAndJsonCookie_AndFillsProfile()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(
                """{"code":803,"cookie":"__csrf_token=json-csrf; music_a=from-json"}""",
                HttpStatusCode.OK,
                "MUSIC_U=music-u-value; Path=/; HttpOnly",
                "__csrf=csrf-cookie-value; Path=/")
            .EnqueueJson(AccountJson);
        var client = TestHttp.CreateClient(handler);

        var status = await client.PollQrLoginAsync("qr-key");

        Assert.Equal(QrLoginState.Confirmed, status.State);
        var session = status.Session;
        Assert.NotNull(session);
        Assert.Contains("MUSIC_U=music-u-value", session.Cookie);
        Assert.Equal("csrf-cookie-value", session.GetCookieValue("__csrf"));
        Assert.Equal("json-csrf", session.GetCookieValue("__csrf_token"));
        Assert.Equal("from-json", session.GetCookieValue("music_a"));
        Assert.Equal(42, session.UserId);
        Assert.Equal("测试用户", session.Nickname);
        Assert.Equal(1, session.VipType);
        Assert.Equal("https://p1.music.126.net/avatar.jpg", session.AvatarUrl);
        Assert.True(session.IsLoggedIn);
        Assert.True(client.IsLoggedIn);

        Assert.Equal(2, handler.Requests.Count);
        var accountRequest = handler.Requests[1];
        Assert.Equal("/weapi/nuser/account/get", accountRequest.Uri.AbsolutePath);
        Assert.Contains("MUSIC_U=music-u-value", accountRequest.CookieHeader);
    }

    [Fact]
    public async Task PollQrLoginAsync_Confirmed_BuildsCookieFromJsonFieldAndAliasesCsrfToken()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson("""{"code":803,"cookie":"MUSIC_U=json-u; __csrf_token=token-value"}""")
            .EnqueueJson(AccountJson);
        var client = TestHttp.CreateClient(handler);

        var status = await client.PollQrLoginAsync("qr-key");

        Assert.Equal(QrLoginState.Confirmed, status.State);
        var session = status.Session;
        Assert.NotNull(session);
        Assert.Equal("json-u", session.GetCookieValue("MUSIC_U"));
        Assert.Equal("token-value", session.GetCookieValue("__csrf"));
        Assert.Equal(42, session.UserId);
    }

    [Fact]
    public async Task PollQrLoginAsync_Confirmed_KeepsCookieSessionWhenProfileFetchFails()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson("""{"code":803}""", HttpStatusCode.OK, "MUSIC_U=only-cookie; Path=/")
            .EnqueueStatus(HttpStatusCode.InternalServerError)
            .EnqueueStatus(HttpStatusCode.InternalServerError);
        var client = TestHttp.CreateClient(handler);

        var status = await client.PollQrLoginAsync("qr-key");

        Assert.Equal(QrLoginState.Confirmed, status.State);
        var session = status.Session;
        Assert.NotNull(session);
        Assert.Equal("only-cookie", session.GetCookieValue("MUSIC_U"));
        Assert.Equal(0, session.UserId);
        Assert.True(session.IsLoggedIn); // 约定：Cookie 非空即登录态
        Assert.Contains("读取账号信息失败", status.Message);
        Assert.Equal(3, handler.Requests.Count);
    }

    [Fact]
    public async Task PollQrLoginAsync_ThrowsNeteaseApiExceptionOnHttpError()
    {
        var handler = new FakeHttpMessageHandler().EnqueueStatus(HttpStatusCode.BadGateway);
        var client = TestHttp.CreateClient(handler);

        var exception = await Assert.ThrowsAsync<NeteaseApiException>(() => client.PollQrLoginAsync("key"));

        Assert.Equal(502, exception.Code);
    }

    [Fact]
    public async Task CreateQrLoginAsync_ThrowsWhenBothEndpointsUnavailable()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueStatus(HttpStatusCode.InternalServerError)
            .EnqueueJson("""{"code":500}""");
        var client = TestHttp.CreateClient(handler);

        await Assert.ThrowsAsync<NeteaseApiException>(() => client.CreateQrLoginAsync());
    }

    [Fact]
    public void BuildCookieCollection_PrefersSetCookieAndAliasesCsrfToken()
    {
        var cookies = NeteaseApiClient.BuildCookieCollection(
            ["MUSIC_U=set-cookie-u; Path=/; HttpOnly", "__csrf_token=token-value; Path=/"],
            "MUSIC_U=json-u; extra=1");

        Assert.Equal("set-cookie-u", cookies.Single(pair => pair.Name == "MUSIC_U").Value);
        Assert.Equal("token-value", cookies.Single(pair => pair.Name == "__csrf_token").Value);
        Assert.Equal("token-value", cookies.Single(pair => pair.Name == "__csrf").Value);
        Assert.Equal("1", cookies.Single(pair => pair.Name == "extra").Value);
        Assert.Equal(4, cookies.Count);
    }
}
