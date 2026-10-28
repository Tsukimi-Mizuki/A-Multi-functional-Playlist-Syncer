using LunaticPrincess.Server.Contracts;
using LunaticPrincess.Server.Infrastructure;

namespace LunaticPrincess.Server.Endpoints;

public static class HealthEndpoints
{
    public const string Version = "1.0.0";

    public static void Map(IEndpointRouteBuilder app)
    {
        app.MapGet("/api/health", () => Results.Json(new HealthDto(true, Version), JsonDefaults.Options));
    }
}
