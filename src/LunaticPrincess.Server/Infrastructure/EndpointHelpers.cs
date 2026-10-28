using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Netease;
using LunaticPrincess.Server.Contracts;

namespace LunaticPrincess.Server.Infrastructure;

public static class EndpointHelpers
{
    public const string CoverUserAgent =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

    public static IResult Accepted(JobStartDto dto)
        => Results.Json(dto, JsonDefaults.Options, statusCode: StatusCodes.Status202Accepted);

    /// <summary>从 session.dat 恢复登录态并交给 API 客户端；未登录时抛出 401。</summary>
    public static async Task<NeteaseSession> RequireSessionAsync(ServerServices services, CancellationToken cancellationToken)
    {
        var session = await services.SessionStore.LoadAsync(cancellationToken).ConfigureAwait(false);
        if (session is null || !session.IsLoggedIn)
        {
            services.Api.SetSession(null);
            throw new ApiException(StatusCodes.Status401Unauthorized, "未登录网易云账号，请先扫码登录", "not_logged_in");
        }

        services.Api.SetSession(session);
        return session;
    }

    public static async Task<long> RequireUserIdAsync(ServerServices services, CancellationToken cancellationToken)
    {
        var session = await RequireSessionAsync(services, cancellationToken).ConfigureAwait(false);
        if (session.UserId > 0)
        {
            return session.UserId;
        }

        var user = await services.Api.GetCurrentUserAsync(cancellationToken).ConfigureAwait(false);
        if (user is null)
        {
            throw new ApiException(StatusCodes.Status401Unauthorized, "登录态已失效，请重新扫码登录", "not_logged_in");
        }

        return user.UserId;
    }

    public static string RequirePath(string? path)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            throw new ApiException(StatusCodes.Status400BadRequest, "path 不能为空", "invalid_request");
        }

        return path.Trim();
    }

    public static string RequireExistingFile(string? path)
    {
        var full = RequirePath(path);
        if (!File.Exists(full))
        {
            throw new ApiException(StatusCodes.Status404NotFound, "文件不存在：" + full, "not_found");
        }

        return full;
    }
}
