using LunaticPrincess.Server.Infrastructure;

namespace LunaticPrincess.Server.Endpoints;

public static class LogEndpoints
{
    private static readonly TimeSpan HeartbeatInterval = TimeSpan.FromSeconds(20);

    public static void Map(IEndpointRouteBuilder app, ServerServices services)
    {
        app.MapGet("/api/logs", async (HttpContext context) =>
        {
            var cancellationToken = context.RequestAborted;
            Sse.Prepare(context.Response);
            try
            {
                foreach (var entry in services.Log.Snapshot())
                {
                    await Sse.WriteAsync(context.Response, Sse.Frame("log", entry), cancellationToken).ConfigureAwait(false);
                }

                using var subscription = services.Log.Subscribe();
                var reader = subscription.Reader;
                while (!cancellationToken.IsCancellationRequested)
                {
                    bool available;
                    using (var wait = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken))
                    {
                        wait.CancelAfter(HeartbeatInterval);
                        try
                        {
                            available = await reader.WaitToReadAsync(wait.Token).ConfigureAwait(false);
                        }
                        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
                        {
                            await Sse.HeartbeatAsync(context.Response, cancellationToken).ConfigureAwait(false);
                            continue;
                        }
                    }

                    if (!available)
                    {
                        return;
                    }

                    while (reader.TryRead(out var entry))
                    {
                        await Sse.WriteAsync(context.Response, Sse.Frame("log", entry), cancellationToken).ConfigureAwait(false);
                    }
                }
            }
            catch (OperationCanceledException)
            {
                // 客户端断开，正常结束。
            }
        });
    }
}
