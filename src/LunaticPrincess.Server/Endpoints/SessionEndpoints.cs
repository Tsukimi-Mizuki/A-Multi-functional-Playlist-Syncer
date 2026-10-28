using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Netease;
using LunaticPrincess.Server.Contracts;
using LunaticPrincess.Server.Infrastructure;

namespace LunaticPrincess.Server.Endpoints;

public static class SessionEndpoints
{
    public static void Map(IEndpointRouteBuilder app, ServerServices services)
    {
        app.MapGet("/api/session", async (CancellationToken cancellationToken) =>
        {
            var stored = await services.SessionStore.LoadAsync(cancellationToken).ConfigureAwait(false);
            if (stored is null || !stored.IsLoggedIn)
            {
                services.Api.SetSession(null);
                return Results.Json(new SessionDto(false, null), JsonDefaults.Options);
            }

            services.Api.SetSession(stored);
            try
            {
                var user = await services.Api.GetCurrentUserAsync(cancellationToken).ConfigureAwait(false);
                if (user is not null)
                {
                    return Results.Json(new SessionDto(true, user.ToDto()), JsonDefaults.Options);
                }
            }
            catch (NeteaseApiException ex)
            {
                services.Log.Warn("刷新登录态失败，回退到本地会话：" + ex.Message);
            }

            return Results.Json(new SessionDto(true, stored.ToDto()), JsonDefaults.Options);
        });

        app.MapPost("/api/login/qr", async (CancellationToken cancellationToken) =>
        {
            var start = await services.Api.CreateQrLoginAsync(cancellationToken).ConfigureAwait(false);
            services.Log.Info("已生成二维码登录 key：" + start.Key);
            return Results.Json(new QrLoginStartDto(start.Key, start.QrContent, start.ExpiresAt), JsonDefaults.Options);
        });

        app.MapGet("/api/login/qr/{key}", async (string key, CancellationToken cancellationToken) =>
        {
            var status = await services.Api.PollQrLoginAsync(key, cancellationToken).ConfigureAwait(false);
            UserDto? user = null;
            if (status.State == QrLoginState.Confirmed)
            {
                if (status.Session is { IsLoggedIn: true })
                {
                    await services.SessionStore.SaveAsync(status.Session, cancellationToken).ConfigureAwait(false);
                    services.Api.SetSession(status.Session);
                    services.Log.Info("网易云登录成功：" + status.Session.DisplayName);
                    try
                    {
                        var current = await services.Api.GetCurrentUserAsync(cancellationToken).ConfigureAwait(false);
                        user = current?.ToDto() ?? status.Session.ToDto();
                    }
                    catch (NeteaseApiException ex)
                    {
                        services.Log.Warn("登录后获取用户信息失败：" + ex.Message);
                        user = status.Session.ToDto();
                    }
                }
                else
                {
                    services.Log.Warn("二维码已确认但未拿到登录态：" + status.Message);
                }
            }

            return Results.Json(new QrLoginStatusDto(status.State.ToString(), status.Message, user), JsonDefaults.Options);
        });

        app.MapPost("/api/logout", async (CancellationToken cancellationToken) =>
        {
            await services.SessionStore.ClearAsync(cancellationToken).ConfigureAwait(false);
            services.Api.SetSession(null);
            services.Log.Info("已退出网易云账号");
            return Results.NoContent();
        });
    }
}
