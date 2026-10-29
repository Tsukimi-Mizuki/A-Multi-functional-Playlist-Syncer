using System.Globalization;
using System.Text.Json;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Online;

/// <summary>
/// LRCLIB 词源：开放歌词库，只有逐行同步歌词（无逐字、无译文），
/// 但对冷门曲目、同人音乐覆盖不错，用作网易云 / QQ / 酷狗之外的第三层兜底。
/// </summary>
public sealed class LrclibLyricsSource : ILyricsSource
{
    private const int MaxQueries = 3;

    private static readonly Uri SearchUri = new("https://lrclib.net/api/search");

    private readonly HttpClient _http;
    private readonly ILyricParser _parser;
    private readonly TimeSpan _timeout;
    private readonly TimeSpan _retryDelay;

    public LrclibLyricsSource(
        HttpClient http,
        ILyricParser? parser = null,
        TimeSpan? timeout = null,
        TimeSpan? retryDelay = null)
    {
        _http = http ?? throw new ArgumentNullException(nameof(http));
        _parser = parser ?? new LyricParser();
        _timeout = timeout ?? TimeSpan.FromSeconds(10);
        _retryDelay = retryDelay ?? TimeSpan.FromMilliseconds(1200);
    }

    /// <summary>备用词源名称：设置开关与界面提示都按这个名字对应。</summary>
    public const string LyricsSourceLabel = "LRCLIB";

    public string Label => LyricsSourceLabel;

    public async Task<LyricsSourceCandidate?> FetchAsync(
        IReadOnlyList<string> queries,
        string title,
        string artist,
        string? album,
        long durationMs,
        double minScore,
        CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(title))
        {
            return null;
        }

        var threshold = Math.Clamp(minScore, LyricsMatchScorer.MinThreshold, LyricsMatchScorer.MaxThreshold);

        // 先按「标题 + 歌手」全文检索（LRCLIB 的 q 参数比 track_name/artist_name 宽松得多），
        // 全部落空后再退回精确的 track_name + artist_name 查询。
        var best = await SearchAsync(
                BuildQueryUris(queries),
                threshold,
                title,
                artist,
                album,
                durationMs,
                cancellationToken)
            .ConfigureAwait(false);
        if (best is not null)
        {
            return best;
        }

        return await SearchAsync(
                new[] { BuildTrackUri(title, artist) },
                threshold,
                title,
                artist,
                album,
                durationMs,
                cancellationToken)
            .ConfigureAwait(false);
    }

    private static IEnumerable<Uri> BuildQueryUris(IReadOnlyList<string> queries)
    {
        var count = 0;
        foreach (var query in queries)
        {
            if (string.IsNullOrWhiteSpace(query))
            {
                continue;
            }

            yield return new Uri(SearchUri + "?q=" + Uri.EscapeDataString(query.Trim()));
            count++;
            if (count >= MaxQueries)
            {
                yield break;
            }
        }
    }

    private static Uri BuildTrackUri(string title, string artist)
    {
        var url = SearchUri +
                  "?track_name=" + Uri.EscapeDataString(title.Trim()) +
                  (string.IsNullOrWhiteSpace(artist) ? string.Empty : "&artist_name=" + Uri.EscapeDataString(artist.Trim()));
        return new Uri(url);
    }

    private async Task<LyricsSourceCandidate?> SearchAsync(
        IEnumerable<Uri> uris,
        double threshold,
        string title,
        string artist,
        string? album,
        long durationMs,
        CancellationToken cancellationToken)
    {
        foreach (var uri in uris)
        {
            var json = await ReadJsonAsync(uri, cancellationToken).ConfigureAwait(false);
            if (json is null || json.RootElement.ValueKind != JsonValueKind.Array)
            {
                continue;
            }

            var best = SelectBest(json.RootElement, threshold, title, artist, album, durationMs);
            if (best is not null)
            {
                return best;
            }
        }

        return null;
    }

    private LyricsSourceCandidate? SelectBest(
        JsonElement root,
        double threshold,
        string title,
        string artist,
        string? album,
        long durationMs)
    {
        LyricsSourceCandidate? best = null;
        var bestScore = 0.0;
        foreach (var item in root.EnumerateArray())
        {
            if (item.ValueKind != JsonValueKind.Object)
            {
                continue;
            }

            if (item.TryGetProperty("instrumental", out var instrumental) &&
                instrumental.ValueKind == JsonValueKind.True)
            {
                continue;
            }

            var synced = GetString(item, "syncedLyrics");
            if (string.IsNullOrWhiteSpace(synced))
            {
                continue;
            }

            var trackName = GetString(item, "trackName") ?? title;
            var artistName = GetString(item, "artistName") ?? artist;
            var albumName = GetString(item, "albumName");
            var candidateDurationMs = (long)Math.Round(GetDouble(item, "duration") * 1000);
            if (!LyricsMatchScorer.DurationMatches(durationMs, candidateDurationMs))
            {
                continue;
            }

            var score = LyricsMatchScorer.Score(
                title,
                artist,
                album,
                durationMs,
                trackName,
                artistName,
                albumName,
                candidateDurationMs);
            if (score.Rejected || score.Total < threshold || score.Total <= bestScore)
            {
                continue;
            }

            var document = _parser.Parse(new RawLyrics(synced));
            if (document.IsEmpty)
            {
                continue;
            }

            bestScore = score.Total;
            best = new LyricsSourceCandidate(
                Label,
                trackName,
                artistName,
                albumName ?? string.Empty,
                candidateDurationMs,
                _parser.ToPlainLrc(document, includeTranslation: false));
        }

        return best;
    }

    private async Task<JsonDocument?> ReadJsonAsync(Uri uri, CancellationToken cancellationToken)
    {
        try
        {
            using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            linked.CancelAfter(_timeout);

            // LRCLIB 突发的 429 / 503 很常见，短暂等待后重试一次，避免整首歌词白白丢失。
            for (var attempt = 0; ; attempt++)
            {
                using var response = await _http.GetAsync(uri, linked.Token).ConfigureAwait(false);
                if (response.IsSuccessStatusCode)
                {
                    var text = await response.Content.ReadAsStringAsync(linked.Token).ConfigureAwait(false);
                    return string.IsNullOrWhiteSpace(text) ? null : JsonDocument.Parse(text);
                }

                if (attempt > 0 || !IsTransient(response.StatusCode))
                {
                    return null;
                }

                await Task.Delay(_retryDelay, linked.Token).ConfigureAwait(false);
            }
        }
        catch (Exception ex) when ((ex is HttpRequestException or TaskCanceledException or JsonException) &&
                                   !cancellationToken.IsCancellationRequested)
        {
            return null;
        }
    }

    private static bool IsTransient(System.Net.HttpStatusCode statusCode) =>
        statusCode is System.Net.HttpStatusCode.TooManyRequests or System.Net.HttpStatusCode.ServiceUnavailable;

    private static string? GetString(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String
            ? value.GetString()
            : null;

    private static double GetDouble(JsonElement element, string name)
    {
        if (!element.TryGetProperty(name, out var value))
        {
            return 0;
        }

        return value.ValueKind switch
        {
            JsonValueKind.Number when value.TryGetDouble(out var number) => number,
            JsonValueKind.String when double.TryParse(
                value.GetString(),
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out var parsed) => parsed,
            _ => 0,
        };
    }
}
