using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using NeteaseMusicManager.Core.Models;

namespace NeteaseMusicManager.Core.Netease;

public sealed partial class NeteaseApiClient
{
    private const int QrKeyTypeDesktop = 3;
    private const int QrKeyTypeWeb = 1;
    private static readonly TimeSpan QrContextLifetime = TimeSpan.FromMinutes(10);

    /// <inheritdoc />
    public async Task<QrLoginStart> CreateQrLoginAsync(CancellationToken cancellationToken = default)
    {
        PruneExpiredQrContexts();

        (string Key, Uri BaseUri, int Type)? created = null;
        try
        {
            created = await TryCreateQrAsync(InterfaceBaseUri, QrKeyTypeDesktop, useDesktopUserAgent: true, cancellationToken)
                .ConfigureAwait(false);
        }
        catch (NeteaseApiException)
        {
            // 桌面端出码接口不可用时降级到 web 端点（type=1）。
        }

        created ??= await TryCreateQrAsync(MusicBaseUri, QrKeyTypeWeb, useDesktopUserAgent: false, cancellationToken)
            .ConfigureAwait(false);

        if (created is null)
        {
            throw new NeteaseApiException("获取登录二维码失败：接口未返回 unikey");
        }

        var (key, baseUri, type) = created.Value;
        _qrLoginContexts[key] = new QrLoginContext(baseUri, type, DateTimeOffset.Now);
        return new QrLoginStart(
            key,
            $"https://music.163.com/login?codekey={Uri.EscapeDataString(key)}",
            DateTimeOffset.Now.Add(QrLifetime));
    }

    /// <inheritdoc />
    public async Task<QrLoginStatus> PollQrLoginAsync(string key, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(key))
        {
            throw new ArgumentException("二维码 key 不能为空。", nameof(key));
        }

        // 进程重启后丢失出码上下文时按 web 端点 type=1 保守轮询。
        var context = _qrLoginContexts.TryGetValue(key, out var known)
            ? known
            : new QrLoginContext(MusicBaseUri, QrKeyTypeWeb, DateTimeOffset.Now);

        var query = $"/api/login/qrcode/client/login?key={Uri.EscapeDataString(key)}&type={context.Type.ToString(CultureInfo.InvariantCulture)}";
        var response = await PostPlainFormAsync(
                new Uri(context.BaseUri, query),
                form: null,
                useDesktopUserAgent: context.Type == QrKeyTypeDesktop,
                cancellationToken)
            .ConfigureAwait(false);

        using var document = ParseJsonOrThrow(response, "轮询扫码状态");
        var root = document.RootElement;
        var code = root.GetInt32("code", -1);
        var apiMessage = root.GetStringOrNull("message") ?? root.GetStringOrNull("msg");

        switch (code)
        {
            case 800:
                _qrLoginContexts.TryRemove(key, out _);
                return new QrLoginStatus(QrLoginState.Expired, "二维码已过期，请重新获取");

            case 801:
                return new QrLoginStatus(QrLoginState.Waiting, "等待扫码");

            case 802:
                return new QrLoginStatus(QrLoginState.Scanned, "已扫码，请在手机端确认登录");

            case 803:
                _qrLoginContexts.TryRemove(key, out _);
                return await CompleteQrLoginAsync(root, response.SetCookies, cancellationToken).ConfigureAwait(false);

            default:
                _qrLoginContexts.TryRemove(key, out _);
                var detail = string.IsNullOrWhiteSpace(apiMessage) ? string.Empty : $"：{apiMessage}";
                return new QrLoginStatus(QrLoginState.Failed, $"扫码登录失败（code={code}{detail}）");
        }
    }

    /// <inheritdoc />
    public async Task<NeteaseUser?> GetCurrentUserAsync(CancellationToken cancellationToken = default)
    {
        using var document = await PostWeapiAsync(
                "/weapi/nuser/account/get",
                new JsonObject(),
                "获取当前用户",
                cancellationToken)
            .ConfigureAwait(false);

        var root = document.RootElement;
        var account = root.GetPropertyOrNull("account");
        if (account is null || account.Value.ValueKind == JsonValueKind.Null)
        {
            return null;
        }

        var profile = root.GetPropertyOrNull("profile");
        if (profile is not { ValueKind: JsonValueKind.Object } profileJson)
        {
            // 极少数响应只有 account 没有 profile：仍然视为已登录。
            var accountId = account.Value.GetInt64("id");
            if (accountId <= 0)
            {
                return null;
            }

            var fallbackUser = new NeteaseUser(
                accountId,
                account.Value.GetStringOrNull("userName") ?? string.Empty,
                null,
                0);
            UpdateSessionFromUser(fallbackUser);
            return fallbackUser;
        }

        var userId = profileJson.GetInt64("userId", account.Value.GetInt64("id"));
        if (userId <= 0)
        {
            return null;
        }

        var user = new NeteaseUser(
            userId,
            profileJson.GetStringOrNull("nickname") ?? account.Value.GetStringOrNull("userName") ?? string.Empty,
            profileJson.GetStringOrNull("avatarUrl"),
            profileJson.GetInt32("vipType"));
        UpdateSessionFromUser(user);
        return user;
    }

    private async Task<QrLoginStatus> CompleteQrLoginAsync(
        JsonElement root,
        IReadOnlyList<string> setCookieHeaders,
        CancellationToken cancellationToken)
    {
        var cookies = BuildCookieCollection(setCookieHeaders, root.GetStringOrNull("cookie"));
        if (cookies.Count == 0)
        {
            return new QrLoginStatus(QrLoginState.Failed, "扫码登录成功，但服务端未返回 Cookie");
        }

        var cookie = string.Join("; ", cookies.Select(pair => $"{pair.Name}={pair.Value}"));
        var pendingSession = new NeteaseSession
        {
            Cookie = cookie,
            LoggedInAt = DateTimeOffset.Now,
        };
        SetSession(pendingSession);

        NeteaseUser? user = null;
        NeteaseApiException? lastError = null;
        for (var attempt = 0; attempt < 2 && user is null; attempt++)
        {
            if (attempt > 0)
            {
                await Task.Delay(TimeSpan.FromMilliseconds(250), cancellationToken).ConfigureAwait(false);
            }

            try
            {
                user = await GetCurrentUserAsync(cancellationToken).ConfigureAwait(false);
            }
            catch (NeteaseApiException ex)
            {
                lastError = ex;
            }
        }

        // 803 已经拿到可用 Cookie：即使读取资料失败也要把 Cookie 登录态交给上层（UserId=0，UI 可稍后调用 GetCurrentUserAsync 补齐）。
        var session = Session ?? pendingSession;
        if (user is null)
        {
            var message = lastError is null
                ? "扫码登录成功，正在获取账号信息"
                : $"扫码登录成功，但读取账号信息失败（可稍后重试）：{lastError.Message}";
            return new QrLoginStatus(QrLoginState.Confirmed, message, session);
        }

        return new QrLoginStatus(QrLoginState.Confirmed, "登录成功", session);
    }

    private async Task<(string Key, Uri BaseUri, int Type)?> TryCreateQrAsync(
        Uri baseUri,
        int type,
        bool useDesktopUserAgent,
        CancellationToken cancellationToken)
    {
        var uri = new Uri(baseUri, "/api/login/qrcode/unikey");
        var form = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            ["type"] = type.ToString(CultureInfo.InvariantCulture),
        };
        var response = await PostPlainFormAsync(uri, form, useDesktopUserAgent, cancellationToken).ConfigureAwait(false);
        if (!response.IsSuccessStatusCode)
        {
            return null;
        }

        try
        {
            using var document = JsonDocument.Parse(response.Body);
            var root = document.RootElement;
            if (root.GetInt32("code", 0) != WeapiCodeSuccess)
            {
                return null;
            }

            var unikey = root.GetStringOrNull("unikey");
            return string.IsNullOrWhiteSpace(unikey) ? null : (unikey!, baseUri, type);
        }
        catch (JsonException)
        {
            return null;
        }
    }

    /// <summary>
    /// 合并 Set-Cookie 头与 803 响应体里的 cookie 串。Set-Cookie 头优先，响应体补齐缺失项。
    /// </summary>
    internal static List<(string Name, string Value)> BuildCookieCollection(
        IReadOnlyList<string> setCookieHeaders,
        string? jsonCookie)
    {
        var cookies = new List<(string Name, string Value)>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        void Add(string? name, string? value)
        {
            if (string.IsNullOrWhiteSpace(name) || string.IsNullOrWhiteSpace(value))
            {
                return;
            }

            var trimmedName = name.Trim();
            if (seen.Add(trimmedName))
            {
                cookies.Add((trimmedName, value.Trim()));
            }
        }

        foreach (var header in setCookieHeaders)
        {
            var (name, value) = ParseCookiePair(header);
            Add(name, value);
        }

        if (!string.IsNullOrWhiteSpace(jsonCookie))
        {
            foreach (var part in jsonCookie.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
            {
                var (name, value) = ParseCookiePair(part);
                Add(name, value);
            }
        }

        // 部分登录响应只下发 __csrf_token；weapi 写接口要求 __csrf。
        var csrfToken = cookies.FirstOrDefault(pair => string.Equals(pair.Name, "__csrf_token", StringComparison.OrdinalIgnoreCase));
        if (!string.IsNullOrEmpty(csrfToken.Name) && !seen.Contains("__csrf"))
        {
            cookies.Add(("__csrf", csrfToken.Value));
        }

        return cookies;
    }

    private static (string? Name, string? Value) ParseCookiePair(string rawCookie)
    {
        if (string.IsNullOrWhiteSpace(rawCookie))
        {
            return (null, null);
        }

        // Set-Cookie: MUSIC_U=xxx; Path=/; HttpOnly —— 只取第一段 name=value。
        var segment = rawCookie.Split(';', 2)[0].Trim();
        var separator = segment.IndexOf('=');
        if (separator <= 0)
        {
            return (null, null);
        }

        return (segment[..separator].Trim(), segment[(separator + 1)..].Trim());
    }

    private void PruneExpiredQrContexts()
    {
        if (_qrLoginContexts.IsEmpty)
        {
            return;
        }

        var threshold = DateTimeOffset.Now - QrContextLifetime;
        foreach (var pair in _qrLoginContexts)
        {
            if (pair.Value.CreatedAt < threshold)
            {
                _qrLoginContexts.TryRemove(pair.Key, out _);
            }
        }
    }
}
