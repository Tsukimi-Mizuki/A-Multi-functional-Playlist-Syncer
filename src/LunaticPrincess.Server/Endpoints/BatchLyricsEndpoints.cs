using System.Collections.Concurrent;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Server.Contracts;
using LunaticPrincess.Server.Infrastructure;

namespace LunaticPrincess.Server.Endpoints;

public static class BatchLyricsEndpoints
{
    private const int MaxConcurrency = 3;

    public static void Map(IEndpointRouteBuilder app, ServerServices services, JobManager jobs)
    {
        app.MapPost("/api/batch-lyrics/scan", async (BatchLyricsScanRequest? request, CancellationToken cancellationToken) =>
        {
            var directory = EndpointHelpers.RequirePath(request?.Directory);
            if (!Directory.Exists(directory))
            {
                throw new ApiException(StatusCodes.Status404NotFound, "目录不存在：" + directory, "not_found");
            }

            var files = await services.BatchLyrics
                .ScanAudioFilesAsync(directory, recursive: true, cancellationToken)
                .ConfigureAwait(false);
            return Results.Json(new BatchLyricsScanResultDto(files.ToArray()), JsonDefaults.Options);
        });

        app.MapPost("/api/batch-lyrics/run", async (BatchLyricsRunRequest? request, CancellationToken cancellationToken) =>
        {
            string[] files = (request?.Files ?? Array.Empty<string>())
                .Where(f => !string.IsNullOrWhiteSpace(f))
                .Select(f => f.Trim())
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .ToArray();
            if (files.Length == 0)
            {
                var directory = EndpointHelpers.RequirePath(request?.Directory);
                if (!Directory.Exists(directory))
                {
                    throw new ApiException(StatusCodes.Status404NotFound, "目录不存在：" + directory, "not_found");
                }

                files = (await services.BatchLyrics
                    .ScanAudioFilesAsync(directory, recursive: true, cancellationToken)
                    .ConfigureAwait(false)).ToArray();
            }

            if (files.Length == 0)
            {
                throw new ApiException(StatusCodes.Status400BadRequest, "没有找到需要处理的音频文件", "invalid_request");
            }

            var settings = await services.SettingsStore.LoadAsync(cancellationToken).ConfigureAwait(false);
            var options = request?.Options;
            var lyricsOptions = Mappers.BuildBatchLyricsOptions(
                options?.TagSourcePriority,
                options?.LyricsSourcePriority,
                options?.OverwriteExisting,
                options?.FetchLyrics,
                options?.PreferWordLyrics,
                options?.MatchThreshold,
                settings);
            var total = files.Length;

            var job = jobs.Start("batch-lyrics", async (record, token) =>
            {
                using var semaphore = new SemaphoreSlim(MaxConcurrency, MaxConcurrency);
                var items = new ConcurrentBag<BatchLyricsItemDto>();
                var completed = 0;

                async Task RunOneAsync(string file)
                {
                    await semaphore.WaitAsync(token).ConfigureAwait(false);
                    try
                    {
                        BatchLyricsResult result;
                        try
                        {
                            result = await services.BatchLyrics
                                .FetchForFileAsync(file, lyricsOptions, token)
                                .ConfigureAwait(false);
                        }
                        catch (OperationCanceledException)
                        {
                            throw;
                        }
                        catch (Exception ex)
                        {
                            result = new BatchLyricsResult(file, false, false, "处理失败：" + ex.Message);
                        }

                        items.Add(new BatchLyricsItemDto(
                            file,
                            result.Success,
                            result.SourceLabel,
                            result.Message,
                            result.MatchedTitle,
                            result.MatchedArtist));

                        var done = Interlocked.Increment(ref completed);
                        record.Report(new JobProgress
                        {
                            Completed = done,
                            Total = total,
                            Stage = "正在抓取歌词",
                            Item = Path.GetFileName(file),
                            Message = result.Message,
                            Percent = Math.Clamp(done * 100.0 / total, 0, 100),
                        });
                    }
                    finally
                    {
                        semaphore.Release();
                    }
                }

                await Task.WhenAll(files.Select(RunOneAsync)).ConfigureAwait(false);
                var ordered = items.OrderBy(i => i.Path, StringComparer.OrdinalIgnoreCase).ToArray();
                return new BatchLyricsResultDto(ordered);
            });

            return EndpointHelpers.Accepted(new JobStartDto(job.Id));
        });
    }
}
