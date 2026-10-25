using System.Net;
using System.Text.Json;
using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Core.Netease;
using Xunit;

namespace NeteaseMusicManager.Core.Tests.Netease;

public sealed class NeteaseApiClientCoreTests
{
    private const string FixedSecret = "0123456789abcdef";
    private const string AccountJson = """
        {
          "code": 200,
          "account": { "id": 42, "userName": "test@example.com" },
          "profile": { "userId": 42, "nickname": "测试用户", "avatarUrl": "https://p1/avatar.jpg", "vipType": 1 }
        }
        """;

    [Fact]
    public async Task GetCurrentUserAsync_ReturnsNullForAnonymousAccount()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson("""{"code":200,"account":null,"profile":null}""");
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var user = await client.GetCurrentUserAsync();

        Assert.Null(user);
        Assert.False(client.IsLoggedIn);
        Assert.Null(client.Session);
    }

    [Fact]
    public async Task GetCurrentUserAsync_MapsProfileAndUpdatesCurrentSession()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson(AccountJson);
        var client = TestHttp.CreateClient(handler, FixedSecret);
        client.SetSession(new NeteaseSession { Cookie = "MUSIC_U=music-u; __csrf=csrf-token" });

        var user = await client.GetCurrentUserAsync();

        Assert.NotNull(user);
        Assert.Equal(42, user.UserId);
        Assert.Equal("测试用户", user.Nickname);
        Assert.Equal(1, user.VipType);
        Assert.Equal("https://p1/avatar.jpg", user.AvatarUrl);

        var session = client.Session;
        Assert.NotNull(session);
        Assert.Equal("MUSIC_U=music-u; __csrf=csrf-token", session.Cookie);
        Assert.Equal(42, session.UserId);
        Assert.Equal("测试用户", session.Nickname);
        Assert.Equal(1, session.VipType);
        Assert.True(session.IsLoggedIn);

        var request = Assert.Single(handler.Requests);
        Assert.Equal("/weapi/nuser/account/get", request.Uri.AbsolutePath);
        Assert.Contains("MUSIC_U=music-u", request.CookieHeader);
        using var payload = JsonDocument.Parse(TestHttp.DecryptBusinessJson(request, FixedSecret));
        Assert.Equal("csrf-token", payload.RootElement.GetProperty("csrf_token").GetString());
    }

    [Fact]
    public async Task WeapiRequest_UsesTwoLayerEncryptionAndDeterministicEncSecKey()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson("""{"code":200,"account":null}""");
        var client = TestHttp.CreateClient(handler, FixedSecret);

        await client.GetCurrentUserAsync();

        var request = Assert.Single(handler.Requests);
        Assert.Equal(HttpMethod.Post, request.Method);
        Assert.Equal("https://music.163.com/weapi/nuser/account/get", request.Uri.ToString());
        Assert.Equal("application/x-www-form-urlencoded", request.GetHeader("Content-Type")!.Split(';')[0]);
        var encryptedParams = request.GetForm("params");
        var encSecKey = request.GetForm("encSecKey");
        Assert.NotNull(encryptedParams);
        Assert.NotNull(encSecKey);
        Assert.Equal(WeapiCrypto.RsaEncryptHex(FixedSecret), encSecKey);
        Assert.Matches("^[0-9a-f]{256}$", encSecKey);

        var plaintext = WeapiCrypto.DecryptPayload(encryptedParams, FixedSecret);
        Assert.Equal("""{"csrf_token":""}""", plaintext);
    }

    [Fact]
    public async Task AnonymousRequest_SendsAnonymousBaseCookie()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson("""{"code":200,"account":null}""");
        var client = TestHttp.CreateClient(handler, FixedSecret);

        await client.GetCurrentUserAsync();

        var request = Assert.Single(handler.Requests);
        Assert.Equal(NeteaseApiClient.AnonymousCookie, request.CookieHeader);
        Assert.Contains("Chrome/", request.GetHeader("User-Agent"));
        Assert.Equal("https://music.163.com/", request.GetHeader("Referer"));
    }

    [Fact]
    public void SetSession_ControlsLoginState()
    {
        var client = TestHttp.CreateClient(new FakeHttpMessageHandler(), FixedSecret);
        Assert.False(client.IsLoggedIn);

        client.SetSession(new NeteaseSession { Cookie = "MUSIC_U=abc" });
        Assert.True(client.IsLoggedIn);
        Assert.NotNull(client.Session);

        client.SetSession(null);
        Assert.False(client.IsLoggedIn);
        Assert.Null(client.Session);
    }

    [Fact]
    public async Task HttpErrorStatus_ThrowsNeteaseApiExceptionWithStatusCode()
    {
        var handler = new FakeHttpMessageHandler().EnqueueStatus(HttpStatusCode.BadGateway);
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var exception = await Assert.ThrowsAsync<NeteaseApiException>(() => client.GetCurrentUserAsync());

        Assert.Equal(502, exception.Code);
        Assert.Contains("HTTP 502", exception.Message);
    }

    [Fact]
    public async Task BusinessErrorCode_ThrowsNeteaseApiExceptionWithCode()
    {
        var handler = new FakeHttpMessageHandler().EnqueueJson("""{"code":-460,"message":"Cheating"}""");
        var client = TestHttp.CreateClient(handler, FixedSecret);

        var exception = await Assert.ThrowsAsync<NeteaseApiException>(() => client.GetCurrentUserAsync());

        Assert.Equal(-460, exception.Code);
        Assert.Contains("-460", exception.Message);
    }

    [Fact]
    public async Task InvalidJsonResponse_ThrowsNeteaseApiException()
    {
        var handler = new FakeHttpMessageHandler().Enqueue(_ => new HttpResponseMessage(HttpStatusCode.OK)
        {
            Content = new StringContent("<html>blocked</html>"),
        });
        var client = TestHttp.CreateClient(handler, FixedSecret);

        await Assert.ThrowsAsync<NeteaseApiException>(() => client.GetCurrentUserAsync());
    }

    [Fact]
    public async Task Session_ConcurrentReadWriteIsThreadSafe()
    {
        var client = TestHttp.CreateClient(new FakeHttpMessageHandler(), FixedSecret);
        var sessions = Enumerable.Range(1, 20)
            .Select(index => new NeteaseSession { Cookie = $"MUSIC_U=user{index}", UserId = index })
            .ToArray();

        await Task.WhenAll(Enumerable.Range(0, 40).Select(index => Task.Run(() =>
        {
            client.SetSession(sessions[index % sessions.Length]);
            var session = client.Session;
            Assert.True(session is null || session.Cookie.StartsWith("MUSIC_U=", StringComparison.Ordinal));
        })));

        Assert.NotNull(client.Session);
    }
}
