using System.Globalization;
using System.Text;
using System.Text.Json;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Online;

/// <summary>
/// 酷狗音乐词源：搜索候选（KRC 为主、LRC 兜底）→ 逐字歌词转成项目统一的增强 LRC。
/// KRC 自带逐行译文，能补上网易云 / QQ 都没有翻译的歌曲；
/// 接口无鉴权但会按关键词返回多条候选，必须用匹配度打分筛选，避免把同名歌曲写错。
/// </summary>
public sealed class KugouLyricsSource : ILyricsSource
{
    private const int MaxQueries = 2;
    private const int MaxCandidates = 8;

    private static readonly Uri HashSearchUri = new("https://mobilecdn.kugou.com/api/v3/search/song");
    private static readonly Uri LyricsSearchUri = new("https://krcs.kugou.com/search");
    private static readonly Uri LyricsDownloadUri = new("https://lyrics.kugou.com/download");

    private readonly HttpClient _http;
    private readonly ILyricParser _parser;
    private readonly TimeSpan _timeout;

    public KugouLyricsSource(HttpClient http, ILyricParser? parser = null, TimeSpan? timeout = null)
    {
        _http = http ?? throw new ArgumentNullException(nameof(http));
        _parser = parser ?? new LyricParser();
        _timeout = timeout ?? TimeSpan.FromSeconds(10);
    }

    /// <summary>备用词源名称：设置开关与界面提示都按这个名字对应。</summary>
    public const string LyricsSourceLabel = "酷狗音乐";

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
        ArgumentNullException.ThrowIfNull(queries);
        var threshold = Math.Clamp(minScore, LyricsMatchScorer.MinThreshold, LyricsMatchScorer.MaxThreshold);
        var attempts = 0;

        foreach (var query in queries)
        {
            if (string.IsNullOrWhiteSpace(query))
            {
                continue;
            }

            if (attempts >= MaxQueries)
            {
                break;
            }

            attempts++;
            var hash = await FindHashAsync(query, cancellationToken).ConfigureAwait(false);
            var candidate = await FindCandidateAsync(
                query,
                title,
                artist,
                album,
                durationMs,
                hash,
                threshold,
                cancellationToken).ConfigureAwait(false);
            if (candidate is null)
            {
                continue;
            }

            var document = await DownloadDocumentAsync(candidate, cancellationToken).ConfigureAwait(false);
            if (document is null || document.IsEmpty)
            {
                continue;
            }

            return Build(candidate, document);
        }

        return null;
    }

    private LyricsSourceCandidate Build(KugouCandidate candidate, LyricDocument document)
    {
        var hasTranslation = document.Lines.Any(line => !string.IsNullOrWhiteSpace(line.Translation));
        var plain = _parser.ToPlainLrc(document, includeTranslation: false);
        var bilingualPlain = hasTranslation ? _parser.ToPlainLrc(document, includeTranslation: true) : null;
        string? word = null;
        string? bilingualWord = null;
        if (document.HasWordLevel)
        {
            word = _parser.ToEnhancedLrc(document, includeTranslation: false);
            bilingualWord = hasTranslation ? _parser.ToEnhancedLrc(document, includeTranslation: true) : null;
        }

        return new LyricsSourceCandidate(
            Label,
            candidate.Song,
            candidate.Singer,
            string.Empty,
            candidate.DurationMs,
            plain,
            word,
            bilingualPlain,
            bilingualWord);
    }

    /// <summary>用 KRC 下载接口拿歌词文档；KRC 解不出来时退回 LRC 文本。</summary>
    private async Task<LyricDocument?> DownloadDocumentAsync(
        KugouCandidate candidate,
        CancellationToken cancellationToken)
    {
        var krcUrl = new Uri(
            LyricsDownloadUri + "?ver=1&client=pc&fmt=krc&charset=utf8" +
            "&id=" + Uri.EscapeDataString(candidate.Id) +
            "&accesskey=" + Uri.EscapeDataString(candidate.AccessKey));
        var krcText = KrcLyricsParser.DecryptBase64(await ReadContentBase64Async(krcUrl, cancellationToken).ConfigureAwait(false));
        if (!string.IsNullOrWhiteSpace(krcText))
        {
            var document = KrcLyricsParser.Parse(krcText);
            if (!document.IsEmpty)
            {
                return document;
            }
        }

        var lrcUrl = new Uri(
            LyricsDownloadUri + "?ver=1&client=pc&fmt=lrc&charset=utf8" +
            "&id=" + Uri.EscapeDataString(candidate.Id) +
            "&accesskey=" + Uri.EscapeDataString(candidate.AccessKey));
        var lrcText = DecodeBase64Text(await ReadContentBase64Async(lrcUrl, cancellationToken).ConfigureAwait(false));
        if (string.IsNullOrWhiteSpace(lrcText) || !lrcText.Contains('[', StringComparison.Ordinal))
        {
            return null;
        }

        var lrcDocument = _parser.Parse(new RawLyrics(lrcText));
        return lrcDocument.IsEmpty ? null : lrcDocument;
    }

    private async Task<JsonDocument?> ReadJsonAsync(Uri uri, CancellationToken cancellationToken)
    {
        var text = await ReadStringAsync(uri, cancellationToken).ConfigureAwait(false);
        if (string.IsNullOrWhiteSpace(text))
        {
            return null;
        }

        try
        {
            return JsonDocument.Parse(text);
        }
        catch (JsonException)
        {
            return null;
        }
    }

    private async Task<string?> ReadStringAsync(Uri uri, CancellationToken cancellationToken)
    {
        try
        {
            using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            linked.CancelAfter(_timeout);
            using var response = await _http.GetAsync(uri, linked.Token).ConfigureAwait(false);
            if (!response.IsSuccessStatusCode)
            {
                return null;
            }

            return await response.Content.ReadAsStringAsync(linked.Token).ConfigureAwait(false);
        }
        catch (Exception ex) when ((ex is HttpRequestException or TaskCanceledException) &&
                                   !cancellationToken.IsCancellationRequested)
        {
            return null;
        }
    }

    private async Task<string?> ReadContentBase64Async(Uri uri, CancellationToken cancellationToken)
    {
        using var json = await ReadJsonAsync(uri, cancellationToken).ConfigureAwait(false);
        if (json is null || json.RootElement.ValueKind != JsonValueKind.Object)
        {
            return null;
        }

        if (!json.RootElement.TryGetProperty("status", out var status) ||
            status.ValueKind != JsonValueKind.Number ||
            status.GetInt32() != 200)
        {
            return null;
        }

        return json.RootElement.TryGetProperty("content", out var content) &&
               content.ValueKind == JsonValueKind.String
            ? content.GetString()
            : null;
    }

    /// <summary>先查歌曲 hash（krcs 接口带 hash 命中率更高），查不到也继续尝试。</summary>
    private async Task<string?> FindHashAsync(string keyword, CancellationToken cancellationToken)
    {
        var uri = new Uri(
            HashSearchUri + "?format=json&page=1&pagesize=3&showtype=1&keyword=" +
            Uri.EscapeDataString(keyword));
        using var json = await ReadJsonAsync(uri, cancellationToken).ConfigureAwait(false);
        if (json is null ||
            !json.RootElement.TryGetProperty("data", out var data) ||
            !data.TryGetProperty("info", out var info) ||
            info.ValueKind != JsonValueKind.Array)
        {
            return null;
        }

        foreach (var item in info.EnumerateArray())
        {
            if (item.TryGetProperty("hash", out var hash) &&
                hash.ValueKind == JsonValueKind.String &&
                !string.IsNullOrWhiteSpace(hash.GetString()))
            {
                return hash.GetString();
            }
        }

        return null;
    }

    private async Task<KugouCandidate?> FindCandidateAsync(
        string keyword,
        string title,
        string artist,
        string? album,
        long durationMs,
        string? hash,
        double threshold,
        CancellationToken cancellationToken)
    {
        var seconds = Math.Max(1, (int)Math.Round(durationMs / 1000d));
        var uri = new Uri(
            LyricsSearchUri + "?ver=1&man=yes&client=mobi&keyword=" + Uri.EscapeDataString(keyword) +
            "&duration=" + seconds.ToString(CultureInfo.InvariantCulture) +
            (string.IsNullOrWhiteSpace(hash) ? string.Empty : "&hash=" + Uri.EscapeDataString(hash)));
        using var json = await ReadJsonAsync(uri, cancellationToken).ConfigureAwait(false);
        if (json is null ||
            !json.RootElement.TryGetProperty("candidates", out var candidates) ||
            candidates.ValueKind != JsonValueKind.Array)
        {
            return null;
        }

        KugouCandidate? best = null;
        var bestScore = 0.0;
        var inspected = 0;
        foreach (var item in candidates.EnumerateArray())
        {
            if (inspected++ >= MaxCandidates)
            {
                break;
            }

            var id = GetString(item, "id");
            var accessKey = GetString(item, "accesskey");
            var song = GetString(item, "song");
            var singer = GetString(item, "singer");
            if (string.IsNullOrWhiteSpace(id) || string.IsNullOrWhiteSpace(accessKey) || string.IsNullOrWhiteSpace(song))
            {
                continue;
            }

            var candidateDuration = GetInt64(item, "duration") * 1000;
            if (!LyricsMatchScorer.DurationMatches(durationMs, candidateDuration))
            {
                continue;
            }

            var score = LyricsMatchScorer.Score(
                title,
                artist,
                album,
                durationMs,
                song,
                singer,
                null,
                candidateDuration);
            if (score.Rejected || score.Total < threshold || score.Total <= bestScore)
            {
                continue;
            }

            bestScore = score.Total;
            best = new KugouCandidate(id!, accessKey!, song!, singer ?? string.Empty, candidateDuration);
        }

        return best;
    }

    private static string? GetString(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String
            ? value.GetString()
            : null;

    private static long GetInt64(JsonElement element, string name)
    {
        if (!element.TryGetProperty(name, out var value))
        {
            return 0;
        }

        return value.ValueKind switch
        {
            JsonValueKind.Number when value.TryGetInt64(out var number) => number,
            JsonValueKind.String when long.TryParse(
                value.GetString(),
                NumberStyles.Integer,
                CultureInfo.InvariantCulture,
                out var parsed) => parsed,
            _ => 0,
        };
    }

    private static string? DecodeBase64Text(string? base64)
    {
        if (string.IsNullOrWhiteSpace(base64))
        {
            return null;
        }

        try
        {
            return Encoding.UTF8.GetString(Convert.FromBase64String(base64.Trim()));
        }
        catch (FormatException)
        {
            return null;
        }
    }

    private sealed record KugouCandidate(
        string Id,
        string AccessKey,
        string Song,
        string Singer,
        long DurationMs);
}
