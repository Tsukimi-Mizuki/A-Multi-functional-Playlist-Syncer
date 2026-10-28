using System.Text.Json;

namespace LunaticPrincess.Server.Infrastructure;

/// <summary>Server-Sent Events 写入工具。</summary>
public static class Sse
{
    public static void Prepare(HttpResponse response)
    {
        response.StatusCode = StatusCodes.Status200OK;
        response.ContentType = "text/event-stream; charset=utf-8";
        response.Headers.CacheControl = "no-cache";
        response.Headers["X-Accel-Buffering"] = "no";
    }

    public static string Frame(string eventName, object payload)
        => "event: " + eventName + "\ndata: " + JsonSerializer.Serialize(payload, payload.GetType(), JsonDefaults.Options) + "\n\n";

    public static async Task WriteAsync(HttpResponse response, string frame, CancellationToken cancellationToken)
    {
        await response.WriteAsync(frame, cancellationToken).ConfigureAwait(false);
        await response.Body.FlushAsync(cancellationToken).ConfigureAwait(false);
    }

    public static Task HeartbeatAsync(HttpResponse response, CancellationToken cancellationToken)
        => WriteAsync(response, ": ping\n\n", cancellationToken);
}
