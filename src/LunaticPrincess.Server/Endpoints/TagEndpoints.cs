using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Online;
using LunaticPrincess.Server.Contracts;
using LunaticPrincess.Server.Infrastructure;

namespace LunaticPrincess.Server.Endpoints;

public static class TagEndpoints
{
    private const int MaxMatches = 10;

    public static void Map(IEndpointRouteBuilder app, ServerServices services)
    {
        app.MapPost("/api/tags/read", async (TagReadRequest? request, CancellationToken cancellationToken) =>
        {
            var path = EndpointHelpers.RequireExistingFile(request?.Path);
            var info = await services.Tags.ReadAsync(path, cancellationToken).ConfigureAwait(false)
                ?? throw new ApiException(StatusCodes.Status404NotFound, "无法读取音频标签：" + path, "not_found");
            return Results.Json(info.ToTagInfoDto(path), JsonDefaults.Options);
        });

        app.MapPost("/api/tags/write", async (TagWriteRequest? request, CancellationToken cancellationToken) =>
        {
            var path = EndpointHelpers.RequireExistingFile(request?.Path);
            var existing = await services.Tags.ReadAsync(path, cancellationToken).ConfigureAwait(false);
            var tags = Mappers.ToAudioTagData(request?.Tags, existing, request?.Lyrics, request?.CoverBase64);
            await services.Tags.WriteAsync(path, tags, cancellationToken).ConfigureAwait(false);
            services.Log.Info("已写入标签：" + path);
            return Results.Json(new OkDto(true), JsonDefaults.Options);
        });

        app.MapPost("/api/lyrics/search", async (LyricsSearchRequest? request, CancellationToken cancellationToken) =>
        {
            var title = request?.Title?.Trim() ?? "";
            if (title.Length == 0)
            {
                throw new ApiException(StatusCodes.Status400BadRequest, "title 不能为空", "invalid_request");
            }

            var artist = request?.Artist;
            var album = request?.Album;
            var durationMs = request?.DurationMs ?? 0;
            var settings = await services.SettingsStore.LoadAsync(cancellationToken).ConfigureAwait(false);
            var source = Mappers.ParseEnumOrDefault(request?.Source, OnlineMusicSource.Netease, "source");
            var preferWord = request?.PreferWordLyrics ?? settings.LyricsEditorPreferWordLyrics;
            var threshold = Mappers.ClampThreshold(request?.MatchThreshold ?? settings.MatchThreshold);

            var candidates = new List<(OnlineSongResult Song, double Score)>();
            var seen = new HashSet<string>(StringComparer.Ordinal);
            foreach (var query in LyricsMatchScorer.BuildQueries(title, artist))
            {
                IReadOnlyList<OnlineSongResult> results;
                try
                {
                    results = await services.OnlineSearch
                        .SearchAsync(source, query, 20, cancellationToken)
                        .ConfigureAwait(false);
                }
                catch (OperationCanceledException)
                {
                    throw;
                }
                catch (Exception ex)
                {
                    services.Log.Warn("在线搜索失败（" + query + "）：" + ex.Message);
                    continue;
                }

                foreach (var song in results)
                {
                    var key = song.Source + ":" + song.SourceId;
                    if (!seen.Add(key))
                    {
                        continue;
                    }

                    var score = LyricsMatchScorer.Score(
                        title,
                        artist,
                        album,
                        durationMs,
                        song.Title,
                        song.Artist,
                        song.Album,
                        song.DurationMs);
                    if (score.Rejected || score.Total < threshold)
                    {
                        continue;
                    }

                    candidates.Add((song, score.Total));
                }
            }

            var matches = new List<LyricsMatchDto>();
            foreach (var candidate in candidates.OrderByDescending(c => c.Score).Take(MaxMatches))
            {
                string? lineLyrics = null;
                string? wordLyrics = null;
                string? coverUrl = candidate.Song.CoverUrl;
                try
                {
                    var details = await services.OnlineSearch
                        .GetDetailsAsync(candidate.Song, cancellationToken)
                        .ConfigureAwait(false);
                    if (details is not null)
                    {
                        lineLyrics = details.GetLyrics(preferWord, bilingual: true);
                        wordLyrics = details.WordLyrics;
                        coverUrl = details.Song.CoverUrl ?? coverUrl;
                    }
                }
                catch (OperationCanceledException)
                {
                    throw;
                }
                catch (Exception ex)
                {
                    services.Log.Warn("获取歌词详情失败：" + candidate.Song.Title + "：" + ex.Message);
                }

                matches.Add(new LyricsMatchDto(
                    candidate.Song.Source,
                    candidate.Song.SourceId,
                    candidate.Song.Title,
                    candidate.Song.Artist,
                    candidate.Song.Album,
                    candidate.Song.DurationMs,
                    candidate.Score,
                    coverUrl,
                    new LyricsDto(lineLyrics, null, wordLyrics)));
            }

            return Results.Json(new LyricsSearchResultDto(matches), JsonDefaults.Options);
        });
    }
}
