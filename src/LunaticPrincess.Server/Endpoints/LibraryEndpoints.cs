using LunaticPrincess.Core.Library;
using LunaticPrincess.Server.Contracts;
using LunaticPrincess.Server.Infrastructure;

namespace LunaticPrincess.Server.Endpoints;

public static class LibraryEndpoints
{
    public static void Map(IEndpointRouteBuilder app, ServerServices services, JobManager jobs)
    {
        app.MapPost("/api/library/scan", async (LibraryScanRequest? request, CancellationToken cancellationToken) =>
        {
            var settings = await services.SettingsStore.LoadAsync(cancellationToken).ConfigureAwait(false);
            var directories = (request?.Directories ?? Array.Empty<string>())
                .Where(d => !string.IsNullOrWhiteSpace(d))
                .Select(d => d.Trim())
                .ToArray();
            if (directories.Length == 0)
            {
                directories = new[] { settings.LibraryDirectory };
            }

            var job = jobs.Start("library-scan", async (record, token) =>
            {
                var progress = new JobProgressSink(record);
                var tracks = await services.Library
                    .ScanAsync(directories, ProgressAdapters.Library(progress), token)
                    .ConfigureAwait(false);
                return new LibraryScanResultDto(tracks.Select(t => t.ToDto()).ToArray());
            });

            return EndpointHelpers.Accepted(new JobStartDto(job.Id));
        });

        app.MapPost("/api/library/compare", (CompareRequest? request) =>
        {
            var songs = (request?.Songs ?? Array.Empty<SongDto>()).Select(s => s.ToDomain()).ToArray();
            var tracks = (request?.Tracks ?? Array.Empty<LocalTrackDto>()).Select(t => t.ToDomain()).ToArray();
            var matches = services.Matcher.Match(songs, tracks);
            var items = matches.Select(match =>
            {
                var local = match.LocalTrack;
                var status = !match.IsPresent || local is null
                    ? "Missing"
                    : local.IsNcm
                        ? "Ncm"
                        : "Local";
                return new CompareItemDto(
                    match.Song.Id,
                    status,
                    local?.Path,
                    local?.Title,
                    match.IsPresent ? match.Confidence : null);
            }).ToArray();

            return Results.Json(new CompareResultDto(items), JsonDefaults.Options);
        });
    }
}
