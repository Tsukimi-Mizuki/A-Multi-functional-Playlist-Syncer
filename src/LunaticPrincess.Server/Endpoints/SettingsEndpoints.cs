using LunaticPrincess.Server.Contracts;
using LunaticPrincess.Server.Infrastructure;

namespace LunaticPrincess.Server.Endpoints;

public static class SettingsEndpoints
{
    public static void Map(IEndpointRouteBuilder app, ServerServices services)
    {
        app.MapGet("/api/settings", async (CancellationToken cancellationToken) =>
        {
            var settings = await services.SettingsStore.LoadAsync(cancellationToken).ConfigureAwait(false);
            settings.Normalize();
            return Results.Json(settings.ToDto(), JsonDefaults.Options);
        });

        app.MapPut("/api/settings", async (AppSettingsUpdateDto? patch, CancellationToken cancellationToken) =>
        {
            var settings = await services.SettingsStore.LoadAsync(cancellationToken).ConfigureAwait(false);
            settings.ApplyPatch(patch);
            settings.Normalize();
            await services.SettingsStore.SaveAsync(settings, cancellationToken).ConfigureAwait(false);
            services.Log.Info("已保存设置");
            return Results.Json(settings.ToDto(), JsonDefaults.Options);
        });
    }
}
