using LunaticPrincess.Server.Contracts;
using LunaticPrincess.Server.Infrastructure;

namespace LunaticPrincess.Server.Endpoints;

public static class JobEndpoints
{
    private static readonly TimeSpan HeartbeatInterval = TimeSpan.FromSeconds(15);

    public static void Map(IEndpointRouteBuilder app, ServerServices services, JobManager jobs)
    {
        app.MapGet("/api/jobs/{id}", (string id) =>
        {
            if (!jobs.TryGet(id, out var job))
            {
                throw new ApiException(StatusCodes.Status404NotFound, "任务不存在：" + id, "job_not_found");
            }

            return Results.Json(jobs.Snapshot(job), JsonDefaults.Options);
        });

        app.MapGet("/api/jobs/{id}/events", async (HttpContext context, string id) =>
        {
            if (!jobs.TryGet(id, out var job))
            {
                throw new ApiException(StatusCodes.Status404NotFound, "任务不存在：" + id, "job_not_found");
            }

            var cancellationToken = context.RequestAborted;
            Sse.Prepare(context.Response);
            using var subscription = job.Subscribe();
            try
            {
                foreach (var frame in subscription.History)
                {
                    await Sse.WriteAsync(context.Response, frame.Payload, cancellationToken).ConfigureAwait(false);
                    if (frame.Terminal)
                    {
                        return;
                    }
                }

                var reader = subscription.Reader;
                if (reader is null)
                {
                    return;
                }

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

                    while (reader.TryRead(out var frame))
                    {
                        await Sse.WriteAsync(context.Response, frame.Payload, cancellationToken).ConfigureAwait(false);
                        if (frame.Terminal)
                        {
                            return;
                        }
                    }
                }
            }
            catch (OperationCanceledException)
            {
                // 客户端断开，正常结束。
            }
        });

        app.MapPost("/api/jobs/{id}/cancel", (string id) =>
        {
            if (!jobs.TryGet(id, out var job))
            {
                throw new ApiException(StatusCodes.Status404NotFound, "任务不存在：" + id, "job_not_found");
            }

            job.RequestCancel();
            services.Log.Info("已请求取消任务：" + id);
            return Results.NoContent();
        });
    }
}
