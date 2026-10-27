using System.Collections.Concurrent;
using System.Globalization;
using System.Net;
using System.Text.Json;
using System.Text.Json.Nodes;
using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Netease;

/// <summary>
/// 网易云音乐 web/weapi 客户端（匿名 + 扫码登录态）。
/// <para>
/// 注意：Cookie 由本客户端自行管理。传入的 <see cref="HttpClient"/> 建议使用
/// <c>new HttpClientHandler { UseCookies = false }</c> 构造，避免处理器内置 CookieContainer
/// 在退出登录后继续携带旧 MUSIC_U。
/// </para>
/// </summary>
public sealed partial class NeteaseApiClient : INeteaseApiClient
{
    internal const int WeapiCodeSuccess = 200;

    /// <summary>weapi/公开 API 使用的浏览器 User-Agent。</summary>
    internal const string BrowserUserAgent =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

    /// <summary>新版桌面客户端 User-Agent（扫码登录 interface 域名使用）。</summary>
    internal const string DesktopUserAgent = "NeteaseMusicDesktop/3.0.18";

    /// <summary>匿名请求的固定基础 Cookie（社区惯例，非必需）。</summary>
    internal const string AnonymousCookie = "os=pc; appver=8.9.70";

    /// <summary>二维码有效期约 180 秒。</summary>
    internal static readonly TimeSpan QrLifetime = TimeSpan.FromSeconds(180);

    internal static readonly Uri MusicBaseUri = new("https://music.163.com");
    internal static readonly Uri InterfaceBaseUri = new("https://interface.music.163.com");

    private const string Referer = "https://music.163.com/";
    private static readonly TimeSpan RequestTimeout = TimeSpan.FromSeconds(20);

    private readonly HttpClient _httpClient;
    private readonly Func<string> _secretFactory;
    private readonly object _sessionGate = new();
    private readonly ConcurrentDictionary<string, QrLoginContext> _qrLoginContexts = new(StringComparer.Ordinal);
    private NeteaseSession? _session;

    /// <summary>使用默认随机 secret 生成器创建客户端。</summary>
    /// <param name="httpClient">已配置好的 HttpClient（建议关闭自动 Cookie 管理）。</param>
    public NeteaseApiClient(HttpClient httpClient)
        : this(httpClient, WeapiCrypto.CreateSecret)
    {
    }

    /// <summary>测试用构造函数：允许注入确定性的 weapi secret。</summary>
    internal NeteaseApiClient(HttpClient httpClient, Func<string> secretFactory)
    {
        _httpClient = httpClient ?? throw new ArgumentNullException(nameof(httpClient));
        _secretFactory = secretFactory ?? throw new ArgumentNullException(nameof(secretFactory));
    }

    /// <inheritdoc />
    public NeteaseSession? Session
    {
        get
        {
            lock (_sessionGate)
            {
                return _session;
            }
        }
    }

    /// <inheritdoc />
    public bool IsLoggedIn => Session?.IsLoggedIn == true;

    /// <inheritdoc />
    public void SetSession(NeteaseSession? session)
    {
        lock (_sessionGate)
        {
            _session = session;
        }
    }

    /// <summary>把当前登录态拼成 Cookie 头；未登录时使用匿名基础 Cookie。</summary>
    internal static string BuildCookieHeader(NeteaseSession? session)
    {
        return string.IsNullOrWhiteSpace(session?.Cookie) ? AnonymousCookie : session!.Cookie;
    }

    /// <summary>构造 weapi 业务 JSON：原样 payload + 来自 cookie 的 csrf_token。</summary>
    internal static string BuildBusinessJson(JsonObject payload, string? csrfToken)
    {
        ArgumentNullException.ThrowIfNull(payload);
        var body = payload.DeepClone() as JsonObject ?? new JsonObject();
        body["csrf_token"] = csrfToken ?? string.Empty;
        return body.ToJsonString();
    }

    /// <summary>发送 weapi POST 请求并返回已校验 code 的 JSON 文档（调用方负责 Dispose）。</summary>
    private async Task<JsonDocument> PostWeapiAsync(
        string path,
        JsonObject payload,
        string operation,
        CancellationToken cancellationToken)
    {
        var session = Session;
        var secret = _secretFactory();
        var businessJson = BuildBusinessJson(payload, session?.CsrfToken);
        var (encryptedParams, encSecKey) = WeapiCrypto.Encrypt(businessJson, secret);

        using var content = new FormUrlEncodedContent(
        [
            new KeyValuePair<string, string>("params", encryptedParams),
            new KeyValuePair<string, string>("encSecKey", encSecKey),
        ]);
        using var request = new HttpRequestMessage(HttpMethod.Post, new Uri(MusicBaseUri, path))
        {
            Content = content,
        };
        ApplyHeaders(request, session, BrowserUserAgent);

        var response = await SendAsync(request, cancellationToken).ConfigureAwait(false);
        var document = ParseJsonOrThrow(response, operation);
        try
        {
            EnsureSuccessCode(document.RootElement, operation);
            return document;
        }
        catch
        {
            document.Dispose();
            throw;
        }
    }

    /// <summary>发送 GET 请求并返回 JSON 文档（调用方负责 Dispose）。</summary>
    private async Task<JsonDocument> GetJsonAsync(Uri uri, string operation, CancellationToken cancellationToken)
    {
        var session = Session;
        using var request = new HttpRequestMessage(HttpMethod.Get, uri);
        ApplyHeaders(request, session, BrowserUserAgent);

        var response = await SendAsync(request, cancellationToken).ConfigureAwait(false);
        var document = ParseJsonOrThrow(response, operation);
        try
        {
            EnsureSuccessCode(document.RootElement, operation);
            return document;
        }
        catch
        {
            document.Dispose();
            throw;
        }
    }

    /// <summary>发送裸表单 POST（扫码登录接口不走 weapi 加密）。</summary>
    private async Task<RawHttpResponse> PostPlainFormAsync(
        Uri uri,
        IReadOnlyDictionary<string, string>? form,
        bool useDesktopUserAgent,
        CancellationToken cancellationToken)
    {
        using var request = new HttpRequestMessage(HttpMethod.Post, uri)
        {
            Content = new FormUrlEncodedContent(form ?? new Dictionary<string, string>()),
        };
        ApplyHeaders(request, Session, useDesktopUserAgent ? DesktopUserAgent : BrowserUserAgent);
        return await SendAsync(request, cancellationToken).ConfigureAwait(false);
    }

    private async Task<RawHttpResponse> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
    {
        using var timeoutSource = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutSource.CancelAfter(RequestTimeout);
        try
        {
            using var response = await _httpClient
                .SendAsync(request, HttpCompletionOption.ResponseHeadersRead, timeoutSource.Token)
                .ConfigureAwait(false);

            var body = response.Content is null
                ? string.Empty
                : await response.Content.ReadAsStringAsync(timeoutSource.Token).ConfigureAwait(false);

            var setCookies = new List<string>();
            if (response.Headers.TryGetValues("Set-Cookie", out var cookieHeaders))
            {
                setCookies.AddRange(cookieHeaders);
            }

            return new RawHttpResponse(response.StatusCode, response.IsSuccessStatusCode, body, setCookies);
        }
        catch (OperationCanceledException ex) when (!cancellationToken.IsCancellationRequested)
        {
            throw new NeteaseApiException(
                $"请求超时（超过 {RequestTimeout.TotalSeconds.ToString("0", CultureInfo.InvariantCulture)} 秒）：{request.Method} {request.RequestUri}",
                null,
                ex);
        }
        catch (HttpRequestException ex)
        {
            throw new NeteaseApiException($"网络请求失败：{ex.Message}", null, ex);
        }
    }

    private static JsonDocument ParseJsonOrThrow(RawHttpResponse response, string operation)
    {
        if (!response.IsSuccessStatusCode)
        {
            throw new NeteaseApiException($"{operation}失败：HTTP {response.NumericStatusCode}", response.NumericStatusCode);
        }

        try
        {
            return JsonDocument.Parse(response.Body);
        }
        catch (JsonException ex)
        {
            throw new NeteaseApiException($"{operation}失败：服务端返回的不是合法 JSON", null, ex);
        }
    }

    private static void EnsureSuccessCode(JsonElement root, string operation)
    {
        if (root.ValueKind != JsonValueKind.Object || !root.TryGetProperty("code", out var codeElement))
        {
            // 部分公开接口不返回 code，按成功处理。
            return;
        }

        var code = codeElement.AsInt32(WeapiCodeSuccess);
        if (code != WeapiCodeSuccess)
        {
            throw new NeteaseApiException($"{operation}失败：接口返回 code={code}", code);
        }
    }

    private static void ApplyHeaders(HttpRequestMessage request, NeteaseSession? session, string userAgent)
    {
        request.Headers.TryAddWithoutValidation("User-Agent", userAgent);
        request.Headers.TryAddWithoutValidation("Referer", Referer);
        request.Headers.TryAddWithoutValidation("Accept", "*/*");
        request.Headers.TryAddWithoutValidation("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8");
        request.Headers.TryAddWithoutValidation("Cookie", BuildCookieHeader(session));
    }

    private void UpdateSessionFromUser(NeteaseUser user)
    {
        lock (_sessionGate)
        {
            var current = _session;
            if (current is null || string.IsNullOrWhiteSpace(current.Cookie))
            {
                return;
            }

            _session = new NeteaseSession
            {
                Cookie = current.Cookie,
                Nickname = user.Nickname,
                UserId = user.UserId,
                VipType = user.VipType,
                AvatarUrl = user.AvatarUrl ?? current.AvatarUrl,
                LoggedInAt = current.LoggedInAt,
            };
        }
    }

    /// <summary>HTTP 响应快照（响应体已读入内存，避免超时令牌作用域问题）。</summary>
    private readonly record struct RawHttpResponse(
        HttpStatusCode StatusCode,
        bool IsSuccessStatusCode,
        string Body,
        IReadOnlyList<string> SetCookies)
    {
        public int NumericStatusCode => (int)StatusCode;
    }

    /// <summary>扫码登录 key 与服务端 base/type 的对应关系（轮询必须回到出码时的地址与 type）。</summary>
    private sealed record QrLoginContext(Uri BaseUri, int Type, DateTimeOffset CreatedAt);
}
