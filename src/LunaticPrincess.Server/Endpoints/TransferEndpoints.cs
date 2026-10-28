using LunaticPrincess.Core.Download;
using LunaticPrincess.Core.Library;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Sync;
using LunaticPrincess.Server.Contracts;
using LunaticPrincess.Server.Infrastructure;

namespace LunaticPrincess.Server.Endpoints;

public static class TransferEndpoints
{
    public static void Map(IEndpointRouteBuilder app, ServerServices services, JobManager jobs)
    {
        app.MapPost("/api/download", async (DownloadRequest? request, CancellationToken cancellationToken) =>
        {
            var songs = (request?.Songs ?? Array.Empty<SongDto>()).Select(s => s.ToDomain()).ToArray();
            if (songs.Length == 0)
            {
                throw new ApiException(StatusCodes.Status400BadRequest, "songs 不能为空", "invalid_request");
            }

            var settings = await services.SettingsStore.LoadAsync(cancellationToken).ConfigureAwait(false);
            var options = Mappers.BuildDownloadOptions(request?.Options, settings);
            DirectoryHelpers.EnsureDirectory(options.TargetDirectory, "下载目录");

            var job = jobs.Start("download", async (record, token) =>
            {
                var progress = new JobProgressSink(record);
                var summary = await services.Download
                    .DownloadAsync(songs, options, ProgressAdapters.Download(progress), token)
                    .ConfigureAwait(false);
                return summary.ToDto();
            });

            return EndpointHelpers.Accepted(new JobStartDto(job.Id));
        });

        app.MapPost("/api/sync", async (SyncRequestDto? request, CancellationToken cancellationToken) =>
        {
            var items = (request?.Items ?? Array.Empty<SyncItemRequest>())
                .Where(item => item.Song is not null)
                .ToArray();
            if (items.Length == 0)
            {
                throw new ApiException(StatusCodes.Status400BadRequest, "items 不能为空", "invalid_request");
            }

            var settings = await services.SettingsStore.LoadAsync(cancellationToken).ConfigureAwait(false);
            var options = Mappers.BuildDownloadOptions(request?.Options, settings);
            DirectoryHelpers.EnsureDirectory(options.TargetDirectory, "下载目录");

            var syncRequests = items.Select(item =>
            {
                var song = item.Song!.ToDomain();
                return new SyncRequest
                {
                    Song = song,
                    LocalMatch = BuildLocalMatch(song, item.LocalPath),
                };
            }).ToArray();

            var syncOptions = new SyncOptions
            {
                Download = options,
                RedownloadExisting = options.OverwriteExisting,
            };

            var job = jobs.Start("sync", async (record, token) =>
            {
                var progress = new JobProgressSink(record);
                var summary = await services.Sync
                    .SyncAsync(syncRequests, syncOptions, ProgressAdapters.Sync(progress), token)
                    .ConfigureAwait(false);
                return summary.ToDto();
            });

            return EndpointHelpers.Accepted(new JobStartDto(job.Id));
        });
    }

    /// <summary>同步接口只带本地路径，这里构造出 MusicSyncService 需要的最小 LocalTrack。</summary>
    private static LocalTrack? BuildLocalMatch(NeteaseSong song, string? localPath)
    {
        if (string.IsNullOrWhiteSpace(localPath))
        {
            return null;
        }

        var path = localPath.Trim();
        if (!File.Exists(path))
        {
            return null;
        }

        return new LocalTrack
        {
            Path = path,
            Title = song.Name,
            Artist = song.ArtistText,
            Album = song.Album?.Name,
            DurationMs = song.DurationMs,
            IsNcm = string.Equals(Path.GetExtension(path), ".ncm", StringComparison.OrdinalIgnoreCase),
            NeteaseId = song.Id,
        };
    }
}
