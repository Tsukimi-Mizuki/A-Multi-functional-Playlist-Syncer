using LunaticPrincess.Core.Download;
using LunaticPrincess.Core.Ncm;
using LunaticPrincess.Server.Contracts;
using LunaticPrincess.Server.Infrastructure;

namespace LunaticPrincess.Server.Endpoints;

public static class NcmEndpoints
{
    public static void Map(IEndpointRouteBuilder app, ServerServices services, JobManager jobs)
    {
        app.MapPost("/api/ncm/scan", async (NcmScanRequest? request, CancellationToken cancellationToken) =>
        {
            var files = NcmScanner.Collect(request?.Directories);
            var results = new List<NcmScanFileDto>(files.Count);
            foreach (var file in files)
            {
                try
                {
                    var info = await services.NcmDecoder.ReadInfoAsync(file, cancellationToken).ConfigureAwait(false);
                    var metadata = info?.Metadata;
                    results.Add(new NcmScanFileDto(
                        file,
                        metadata?.Title,
                        metadata?.Artists,
                        metadata?.Album,
                        metadata?.DurationMs ?? 0,
                        metadata?.Format,
                        metadata?.Bitrate ?? 0,
                        null));
                }
                catch (OperationCanceledException)
                {
                    throw;
                }
                catch (Exception ex)
                {
                    services.Log.Warn("读取 NCM 信息失败：" + file + "：" + ex.Message);
                }
            }

            return Results.Json(new NcmScanResultDto(results), JsonDefaults.Options);
        });

        app.MapPost("/api/ncm/import", async (NcmImportRequest? request, CancellationToken cancellationToken) =>
        {
            var settings = await services.SettingsStore.LoadAsync(cancellationToken).ConfigureAwait(false);
            var files = NcmScanner.Collect(request?.Files);
            if (files.Count == 0)
            {
                throw new ApiException(StatusCodes.Status400BadRequest, "files 不能为空（可以是 .ncm 文件或包含 .ncm 的目录）", "invalid_request");
            }

            var outputDirectory = ResolveOutputDirectory(request?.OutputDirectory, settings);
            DirectoryHelpers.EnsureDirectory(outputDirectory, "输出目录");

            var matchOptions = request?.MatchOptions;
            var lyricsOptions = Mappers.BuildBatchLyricsOptions(
                matchOptions?.TagSourcePriority,
                matchOptions?.LyricsSourcePriority,
                matchOptions?.OverwriteExisting,
                matchOptions?.FetchLyrics,
                matchOptions?.PreferWordLyrics,
                matchOptions?.MatchThreshold,
                settings);
            var importOptions = new NcmImportOptions(request?.AutoMatchOnline ?? false, lyricsOptions);
            var overwrite = request?.Overwrite ?? false;

            var job = jobs.Start("ncm-import", async (record, token) =>
            {
                var progress = new JobProgressSink(record);
                var results = await services.NcmImport
                    .ImportAsync(files, outputDirectory, overwrite, ProgressAdapters.NcmImport(progress), token, importOptions)
                    .ConfigureAwait(false);
                var items = results.Select(r => r.ToDto()).ToArray();
                return new NcmImportResultDto(items, items.Count(i => i.Success), items.Count(i => !i.Success));
            });

            return EndpointHelpers.Accepted(new JobStartDto(job.Id));
        });
    }

    private static string ResolveOutputDirectory(string? requested, LunaticPrincess.Core.Settings.AppSettings settings)
    {
        if (!string.IsNullOrWhiteSpace(requested))
        {
            return requested.Trim();
        }

        if (!string.IsNullOrWhiteSpace(settings.NcmImportOutputDirectory))
        {
            return settings.NcmImportOutputDirectory;
        }

        return settings.DownloadDirectory;
    }
}
