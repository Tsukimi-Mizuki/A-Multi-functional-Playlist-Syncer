using System.Net;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Netease;
using Xunit;

namespace LunaticPrincess.Core.Tests.Netease;

/// <summary>登录态续期、Set-Cookie 合并、「登录失效」判定的相关测试。</summary>
public sealed class NeteaseApiClientSessionTests
{
    private const string FixedSecret = "0123456789abcdef";
    private const string AccountJson = """
        {
          "code": 200,
          "account": { "id": 42, "userName": "u" },
          "profile": { "userId": 42, "nickname": "测试用户", "vipType": 1 }
        }
        """;

    private static NeteaseApiClient CreateLoggedInClient(FakeHttpMessageHandler handler)
    {
        var client = TestHttp.CreateClient(handler, FixedSecret);
        client.SetSession(new NeteaseSession
        {
            Cookie = "MUSIC_U=old; __csrf=csrf1",
            UserId = 42,
            Nickname = "旧昵称",
        });
        return client;
    }

    [Fact]
    public async Task WeapiResponse_SetCookieIsMergedAndRaisesSessionChanged()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(AccountJson, HttpStatusCode.OK, "MUSIC_U=rotated; Max-Age=1296000; Path=/; HttpOnly");
        var client = CreateLoggedInClient(handler);
        var raised = new List<NeteaseSession>();
        client.SessionChanged += (_, session) => raised.Add(session);

        await client.GetCurrentUserAsync();

        var session = client.Session;
        Assert.NotNull(session);
        Assert.Equal("rotated", session.GetCookieValue("MUSIC_U"));
        Assert.Equal("csrf1", session.CsrfToken);
        Assert.Equal(42, session.UserId);
        var changed = Assert.Single(raised);
        Assert.Equal("rotated", changed.GetCookieValue("MUSIC_U"));
    }

    [Fact]
    public async Task GetJsonResponse_SetCookieIsAlsoMerged()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson("""{"code":200,"lrc":{"lyric":"[00:01.00]x"}}""", HttpStatusCode.OK, "MUSIC_U=rotated; Path=/");
        var client = CreateLoggedInClient(handler);
        var raised = 0;
        client.SessionChanged += (_, _) => raised++;

        await client.GetLyricsAsync(1);

        Assert.Equal("rotated", client.Session!.GetCookieValue("MUSIC_U"));
        Assert.Equal(1, raised);
    }

    [Fact]
    public async Task SetCookieWithSameValues_DoesNotRaiseSessionChanged()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(AccountJson, HttpStatusCode.OK, "MUSIC_U=old; Path=/");
        var client = CreateLoggedInClient(handler);
        var raised = 0;
        client.SessionChanged += (_, _) => raised++;

        await client.GetCurrentUserAsync();

        Assert.Equal(0, raised);
    }

    [Fact]
    public async Task AnonymousResponse_SetCookieIsIgnored()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson("""{"code":200,"account":null}""", HttpStatusCode.OK, "MUSIC_A=anon; Path=/");
        var client = TestHttp.CreateClient(handler, FixedSecret);
        var raised = 0;
        client.SessionChanged += (_, _) => raised++;

        await client.GetCurrentUserAsync();

        Assert.Null(client.Session);
        Assert.Equal(0, raised);
    }

    [Fact]
    public async Task NextRequest_UsesRotatedCookie()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(AccountJson, HttpStatusCode.OK, "MUSIC_U=rotated; Path=/")
            .EnqueueJson(AccountJson);
        var client = CreateLoggedInClient(handler);

        await client.GetCurrentUserAsync();
        await client.GetCurrentUserAsync();

        Assert.Contains("MUSIC_U=old", handler.Requests[0].CookieHeader);
        Assert.Contains("MUSIC_U=rotated", handler.Requests[1].CookieHeader);
    }

    [Fact]
    public async Task SetCookieThatWouldDropMusicU_IsIgnored()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson(AccountJson, HttpStatusCode.OK, "MUSIC_U=; Max-Age=0; Path=/", "os=pc; Path=/");
        var client = CreateLoggedInClient(handler);
        var raised = 0;
        client.SessionChanged += (_, _) => raised++;

        await client.GetCurrentUserAsync();

        Assert.Equal("old", client.Session!.GetCookieValue("MUSIC_U"));
        Assert.Equal(0, raised);
    }

    [Fact]
    public async Task SetCookieFromOutdatedRequest_DoesNotLeakIntoNewSession()
    {
        var gate = new TaskCompletionSource();
        var handler = new FakeHttpMessageHandler().Enqueue(_ =>
        {
            gate.Task.GetAwaiter().GetResult();
            var response = TestHttp.JsonResponse(AccountJson);
            response.Headers.TryAddWithoutValidation("Set-Cookie", "MUSIC_U=stale; Path=/");
            return response;
        });
        var client = CreateLoggedInClient(handler);

        var pending = Task.Run(() => client.GetCurrentUserAsync());
        while (handler.Requests.Count == 0)
        {
            await Task.Delay(5);
        }

        client.SetSession(new NeteaseSession { Cookie = "MUSIC_U=other-account" });
        gate.SetResult();
        await pending;

        Assert.Equal("other-account", client.Session!.GetCookieValue("MUSIC_U"));
    }

    [Fact]
    public async Task RefreshLoginAsync_Success_PostsRefreshEndpointAndMergesCookies()
    {
        var handler = new FakeHttpMessageHandler()
            .EnqueueJson("""{"code":200}""", HttpStatusCode.OK, "MUSIC_U=renewed; Max-Age=1296000; Path=/");
        var client = CreateLoggedInClient(handler);
        var raised = 0;
        client.SessionChanged += (_, _) => raised++;

        var ok = await client.RefreshLoginAsync();

        Assert.True(ok);
        var request = Assert.Single(handler.Requests);
        Assert.Equal("/weapi/login/token/refresh", request.Uri.AbsolutePath);
        Assert.Equal("""{"csrf_token":"csrf1"}""", TestHttp.DecryptBusinessJson(request, FixedSecret));
        Assert.Equal("renewed", client.Session!.GetCookieValue("MUSIC_U"));
        Assert.Equal(1, raised);
    }

    [Theory]
    [InlineData("""{"code":301,"message":"需要登录"}""", HttpStatusCode.OK)]
    [InlineData("""{"code":-460}""", HttpStatusCode.OK)]
    [InlineData("", HttpStatusCode.BadGateway)]
    public async Task RefreshLoginAsync_Failure_ReturnsFalseWithoutThrowing(string json, HttpStatusCode status)
    {
        var handler = new FakeHttpMessageHandler().Enqueue(_ => TestHttp.JsonResponse(json, status));
        var client = CreateLoggedInClient(handler);

        Assert.False(await client.RefreshLoginAsync());
        Assert.Equal("old", client.Session!.GetCookieValue("MUSIC_U"));
    }

    [Fact]
    public async Task RefreshLoginAsync_WithoutSession_ReturnsFalseAndSendsNothing()
    {
        var handler = new FakeHttpMessageHandler();
        var client = TestHttp.CreateClient(handler, FixedSecret);

        Assert.False(await client.RefreshLoginAsync());
        Assert.Empty(handler.Requests);
    }

    [Fact]
    public async Task RefreshLoginAsync_Cancellation_Propagates()
    {
        var client = CreateLoggedInClient(new FakeHttpMessageHandler().EnqueueJson("""{"code":200}"""));
        using var cts = new CancellationTokenSource();
        cts.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => client.RefreshLoginAsync(cts.Token));
    }

    [Fact]
    public async Task GetCurrentUserAsync_AccountNull_ReturnsNull()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson("""{"code":200,"account":null,"profile":null}""");
        var client = CreateLoggedInClient(handler);

        Assert.Null(await client.GetCurrentUserAsync());
    }

    [Theory]
    [InlineData(HttpStatusCode.Found, 302)]
    [InlineData(HttpStatusCode.Unauthorized, 401)]
    [InlineData(HttpStatusCode.InternalServerError, 500)]
    public async Task GetCurrentUserAsync_HttpStatusErrors_AreNotLoginExpiry(HttpStatusCode status, int expectedCode)
    {
        var handler = new FakeHttpMessageHandler().EnqueueStatus(status);
        var client = CreateLoggedInClient(handler);

        var exception = await Assert.ThrowsAsync<NeteaseApiException>(() => client.GetCurrentUserAsync());

        Assert.Equal(expectedCode, exception.Code);
        Assert.True(exception.IsHttpStatus);
        Assert.False(exception.IsNotLoggedIn);
    }

    [Theory]
    [InlineData(-460)]
    [InlineData(406)]
    [InlineData(-2)]
    public async Task GetCurrentUserAsync_OtherBusinessCodes_AreNotLoginExpiry(int code)
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson($$"""{"code":{{code}}}""");
        var client = CreateLoggedInClient(handler);

        var exception = await Assert.ThrowsAsync<NeteaseApiException>(() => client.GetCurrentUserAsync());

        Assert.Equal(code, exception.Code);
        Assert.False(exception.IsHttpStatus);
        Assert.False(exception.IsNotLoggedIn);
    }

    [Fact]
    public async Task GetCurrentUserAsync_Code301_IsExplicitNotLoggedIn()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson("""{"code":301,"message":"需要登录"}""");
        var client = CreateLoggedInClient(handler);

        var exception = await Assert.ThrowsAsync<NeteaseApiException>(() => client.GetCurrentUserAsync());

        Assert.Equal(301, exception.Code);
        Assert.False(exception.IsHttpStatus);
        Assert.True(exception.IsNotLoggedIn);
    }

    [Fact]
    public void BuildPlaybackCookieHeader_DoesNotDuplicateKeysAlreadyInSession()
    {
        var session = new NeteaseSession { Cookie = "MUSIC_U=u; os=android; appver=9.9.9" };

        var header = NeteaseApiClient.BuildPlaybackCookieHeader(session);

        var names = header.Split(';', StringSplitOptions.TrimEntries).Select(part => part.Split('=')[0]).ToArray();
        Assert.Equal(names.Length, names.Distinct(StringComparer.OrdinalIgnoreCase).Count());
        Assert.Contains("os=android", header);
        Assert.Contains("appver=9.9.9", header);
        Assert.DoesNotContain("os=pc", header);
        Assert.Contains("deviceId=LunaticPrincessPc", header);
        Assert.Contains("channel=netease", header);
    }

    [Fact]
    public void BuildPlaybackCookieHeader_Anonymous_UsesClientFlagsOnce()
    {
        var header = NeteaseApiClient.BuildPlaybackCookieHeader(null);

        var names = header.Split(';', StringSplitOptions.TrimEntries).Select(part => part.Split('=')[0]).ToArray();
        Assert.Equal(names.Length, names.Distinct(StringComparer.OrdinalIgnoreCase).Count());
        Assert.Contains("appver=2.10.6", header);
    }

    [Fact]
    public void BuildCookieCollection_DuplicateKeysLastWins_HeadersStillBeatBody()
    {
        var cookies = NeteaseApiClient.BuildCookieCollection(
            ["MUSIC_U=first; Path=/", "MUSIC_U=second; Path=/"],
            "MUSIC_U=body; extra=1; extra=2");

        Assert.Equal("second", cookies.Single(pair => pair.Name == "MUSIC_U").Value);
        Assert.Equal("2", cookies.Single(pair => pair.Name == "extra").Value);
    }
}
