using LunaticPrincess.Core.Models;
using LunaticPrincess.Server.Contracts;
using LunaticPrincess.Server.Infrastructure;

namespace LunaticPrincess.Server.Endpoints;

public static class PlaylistEndpoints
{
    private const int DetailChunkSize = 200;

    public static void Map(IEndpointRouteBuilder app, ServerServices services)
    {
        app.MapGet("/api/playlists", async (CancellationToken cancellationToken) =>
        {
            var userId = await EndpointHelpers.RequireUserIdAsync(services, cancellationToken).ConfigureAwait(false);
            var playlists = await services.Api.GetUserPlaylistsAsync(userId, cancellationToken).ConfigureAwait(false);
            var dto = new PlaylistListDto(userId, playlists.Select(p => p.ToDto()).ToArray());
            return Results.Json(dto, JsonDefaults.Options);
        });

        app.MapGet("/api/playlists/{id:long}", async (long id, CancellationToken cancellationToken) =>
        {
            await EndpointHelpers.RequireSessionAsync(services, cancellationToken).ConfigureAwait(false);
            var detail = await services.Api.GetPlaylistDetailAsync(id, cancellationToken).ConfigureAwait(false);
            if (detail is null)
            {
                throw new ApiException(StatusCodes.Status404NotFound, "歌单不存在：" + id, "not_found");
            }

            if (detail.IsPartial && detail.TrackIds is { Count: > 0 })
            {
                detail = await HydrateAsync(services, detail, cancellationToken).ConfigureAwait(false);
            }

            return Results.Json(detail.ToDto(), JsonDefaults.Options);
        });
    }

    /// <summary>歌单详情可能是分页的（IsPartial），用 GetSongDetailsAsync 分批补全缺失的曲目。</summary>
    private static async Task<PlaylistDetail> HydrateAsync(ServerServices services, PlaylistDetail detail, CancellationToken cancellationToken)
    {
        var songs = detail.Songs is null ? new List<NeteaseSong>() : detail.Songs.ToList();
        var known = new HashSet<long>(songs.Select(s => s.Id));
        var missing = detail.TrackIds.Where(trackId => !known.Contains(trackId)).ToArray();
        foreach (var chunk in missing.Chunk(DetailChunkSize))
        {
            var details = await services.Api.GetSongDetailsAsync(chunk, cancellationToken).ConfigureAwait(false);
            songs.AddRange(details);
        }

        return new PlaylistDetail
        {
            Id = detail.Id,
            Name = detail.Name,
            CoverUrl = detail.CoverUrl,
            Description = detail.Description,
            CreatorNickname = detail.CreatorNickname,
            TrackCount = detail.TrackCount,
            Songs = songs,
            TrackIds = detail.TrackIds,
            IsPartial = false,
        };
    }
}
