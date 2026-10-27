using System.Net;
using System.Text;
using System.Text.Json;
using LunaticPrincess.Core.Netease;

namespace LunaticPrincess.Core.Tests.Netease;

/// <summary>可编排响应、记录请求的假 HttpMessageHandler（测试不访问真实网络）。</summary>
internal sealed class FakeHttpMessageHandler : HttpMessageHandler
{
    private readonly Queue<Func<CapturedRequest, HttpResponseMessage>> _responders = new();
    private readonly object _gate = new();
    private Func<CapturedRequest, HttpResponseMessage>? _alwaysResponder;

    public List<CapturedRequest> Requests { get; } = [];

    public FakeHttpMessageHandler Enqueue(Func<CapturedRequest, HttpResponseMessage> responder)
    {
        ArgumentNullException.ThrowIfNull(responder);
        lock (_gate)
        {
            _responders.Enqueue(responder);
        }

        return this;
    }

    public FakeHttpMessageHandler EnqueueJson(
        string json,
        HttpStatusCode statusCode = HttpStatusCode.OK,
        params string[] setCookieHeaders)
    {
        return Enqueue(_ =>
        {
            var response = TestHttp.JsonResponse(json, statusCode);
            foreach (var cookie in setCookieHeaders)
            {
                response.Headers.TryAddWithoutValidation("Set-Cookie", cookie);
            }

            return response;
        });
    }

    public FakeHttpMessageHandler EnqueueStatus(HttpStatusCode statusCode)
    {
        return Enqueue(_ => new HttpResponseMessage(statusCode) { Content = new StringContent(string.Empty) });
    }

    /// <summary>当前队列耗尽后持续使用的响应器（用于批量请求）。</summary>
    public FakeHttpMessageHandler EnqueueAlways(Func<CapturedRequest, HttpResponseMessage> responder)
    {
        ArgumentNullException.ThrowIfNull(responder);
        lock (_gate)
        {
            _alwaysResponder = responder;
        }

        return this;
    }

    protected override async Task<HttpResponseMessage> SendAsync(
        HttpRequestMessage request,
        CancellationToken cancellationToken)
    {
        var captured = await CapturedRequest.CreateAsync(request, cancellationToken).ConfigureAwait(false);
        Func<CapturedRequest, HttpResponseMessage> responder;
        lock (_gate)
        {
            Requests.Add(captured);
            if (_responders.Count > 0)
            {
                responder = _responders.Dequeue();
            }
            else if (_alwaysResponder is not null)
            {
                responder = _alwaysResponder;
            }
            else
            {
                throw new InvalidOperationException($"未预期的 HTTP 请求：{request.Method} {request.RequestUri}");
            }
        }

        return responder(captured);
    }
}

/// <summary>一次被记录的 HTTP 请求快照。</summary>
internal sealed class CapturedRequest
{
    public HttpMethod Method { get; private init; } = HttpMethod.Get;

    public Uri Uri { get; private init; } = new("https://music.163.com/");

    public string Body { get; private init; } = string.Empty;

    public IReadOnlyDictionary<string, string> Headers { get; private init; } =
        new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);

    public IReadOnlyDictionary<string, string> Form { get; private init; } =
        new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);

    public string Host => Uri.Host;

    public string PathAndQuery => Uri.PathAndQuery;

    public string? CookieHeader => GetHeader("Cookie");

    public string? GetHeader(string name) => Headers.TryGetValue(name, out var value) ? value : null;

    public string? GetForm(string name) => Form.TryGetValue(name, out var value) ? value : null;

    public static async Task<CapturedRequest> CreateAsync(
        HttpRequestMessage request,
        CancellationToken cancellationToken)
    {
        var headers = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (var header in request.Headers)
        {
            headers[header.Key] = string.Join(", ", header.Value);
        }

        var body = request.Content is null
            ? string.Empty
            : await request.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);

        if (request.Content is not null)
        {
            foreach (var header in request.Content.Headers)
            {
                headers[header.Key] = string.Join(", ", header.Value);
            }
        }

        var form = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        var mediaType = request.Content?.Headers.ContentType?.MediaType;
        if (body.Length > 0 && string.Equals(mediaType, "application/x-www-form-urlencoded", StringComparison.OrdinalIgnoreCase))
        {
            foreach (var pair in body.Split('&', StringSplitOptions.RemoveEmptyEntries))
            {
                var separator = pair.IndexOf('=');
                if (separator <= 0)
                {
                    continue;
                }

                var name = Uri.UnescapeDataString(pair[..separator].Replace('+', ' '));
                var value = Uri.UnescapeDataString(pair[(separator + 1)..].Replace('+', ' '));
                form[name] = value;
            }
        }

        return new CapturedRequest
        {
            Method = request.Method,
            Uri = request.RequestUri ?? new Uri("https://music.163.com/"),
            Body = body,
            Headers = headers,
            Form = form,
        };
    }
}

internal static class TestHttp
{
    public static HttpResponseMessage JsonResponse(string json, HttpStatusCode statusCode = HttpStatusCode.OK)
    {
        return new HttpResponseMessage(statusCode)
        {
            Content = new StringContent(json, Encoding.UTF8, "application/json"),
        };
    }

    public static NeteaseApiClient CreateClient(FakeHttpMessageHandler handler, string? fixedSecret = null)
    {
        var httpClient = new HttpClient(handler, disposeHandler: false);
        return fixedSecret is null
            ? new NeteaseApiClient(httpClient)
            : new NeteaseApiClient(httpClient, () => fixedSecret);
    }

    /// <summary>用固定 secret 解开 weapi 请求里的业务 JSON（仅测试使用）。</summary>
    public static string DecryptBusinessJson(CapturedRequest request, string secret)
    {
        var encryptedParams = request.GetForm("params")
                              ?? throw new InvalidOperationException("请求缺少 params 表单字段");
        return WeapiCrypto.DecryptPayload(encryptedParams, secret);
    }
}

internal static class TestJson
{
    public static string SongJson(long id)
    {
        return $$"""
            {
              "id": {{id}},
              "name": "歌曲{{id}}",
              "ar": [{ "id": 1, "name": "歌手A" }],
              "al": { "id": 2, "name": "专辑", "picUrl": "https://p1.music.126.net/cover.jpg" },
              "dt": 1000,
              "fee": 0
            }
            """;
    }

    /// <summary>按请求 c 字段里的 id 生成 song/detail 响应（默认倒序返回，用于验证客户端自行排序）。</summary>
    public static string SongDetailResponseForIds(IEnumerable<long> ids, bool reverse = true)
    {
        var requested = ids.ToArray();
        var ordered = reverse ? requested.Reverse() : requested;
        var songs = string.Join(",", ordered.Select(SongJson));
        // 真实接口把 maxbr/fee 放在顶层 privileges 数组里。
        var privileges = string.Join(",", requested.Select(id => $$"""{"id":{{id}},"maxbr":320000,"fee":0}"""));
        return $$"""{"code":200,"songs":[{{songs}}],"privileges":[{{privileges}}]}""";
    }

    public static long[] ParseSongIdsFromDetailRequest(CapturedRequest request, string secret)
    {
        using var payload = JsonDocument.Parse(TestHttp.DecryptBusinessJson(request, secret));
        var cJson = payload.RootElement.GetProperty("c").GetString() ?? "[]";
        using var document = JsonDocument.Parse(cJson);
        return document.RootElement
            .EnumerateArray()
            .Select(element => element.GetProperty("id").GetInt64())
            .ToArray();
    }
}
