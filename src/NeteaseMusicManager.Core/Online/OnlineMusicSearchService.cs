using System.Globalization;
using System.Net;
using System.Text.Json;
using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Core.Netease;

namespace NeteaseMusicManager.Core.Online;

/// <summary>联网搜索实现：网易云走现有 API 客户端，QQ 音乐走公开搜索/歌词接口。</summary>
public sealed class OnlineMusicSearchService : IOnlineMusicSearchService
{
    private const string QqUserAgent =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

    private static readonly Uri QqSearchBase = new("https://c.y.qq.com/soso/fcgi-bin/client_search_cp");
    private static readonly Uri QqLyricBase = new("https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg");

    private readonly INeteaseApiClient _netease;
    private readonly HttpClient _http;
    private readonly TimeSpan _requestTimeout;

    public OnlineMusicSearchService(
        INeteaseApiClient netease,
        HttpClient http,
        TimeSpan? requestTimeout = null)
    {
        _netease = netease ?? throw new ArgumentNullException(nameof(netease));
        _http = http ?? throw new ArgumentNullException(nameof(http));
        _requestTimeout = requestTimeout ?? TimeSpan.FromSeconds(20);
    }

    public Task<IReadOnlyList<OnlineSongResult>> SearchAsync(
        OnlineMusicSource source,
        string keyword,
        int limit = 20,
        CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(keyword))
        {
            return Task.FromResult<IReadOnlyList<OnlineSongResult>>(Array.Empty<OnlineSongResult>());
        }

        return source switch
        {
            OnlineMusicSource.Netease => SearchNeteaseAsync(keyword.Trim(), limit, cancellationToken),
            OnlineMusicSource.QQMusic => SearchQqAsync(keyword.Trim(), limit, cancellationToken),
            _ => Task.FromResult<IReadOnlyList<OnlineSongResult>>(Array.Empty<OnlineSongResult>()),
        };
    }

    public Task<OnlineSongDetails?> GetDetailsAsync(
        OnlineSongResult song,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(song);
        return song.Source switch
        {
            OnlineMusicSource.Netease => GetNeteaseDetailsAsync(song, cancellationToken),
            OnlineMusicSource.QQMusic => GetQqDetailsAsync(song, cancellationToken),
            _ => Task.FromResult<OnlineSongDetails?>(null),
        };
    }

    private async Task<IReadOnlyList<OnlineSongResult>> SearchNeteaseAsync(
        string keyword,
        int limit,
        CancellationToken cancellationToken)
    {
        var songs = await _netease.SearchSongsAsync(keyword, limit, cancellationToken).ConfigureAwait(false);
        return songs.Select(FromNetease).ToList();
    }

    private async Task<OnlineSongDetails?> GetNeteaseDetailsAsync(
        OnlineSongResult summary,
        CancellationToken cancellationToken)
    {
        if (!long.TryParse(summary.SourceId, NumberStyles.Integer, CultureInfo.InvariantCulture, out var songId) ||
            songId <= 0)
        {
            return null;
        }

        NeteaseSong? detail = null;
        try
        {
            var details = await _netease.GetSongDetailsAsync(new[] { songId }, cancellationToken).ConfigureAwait(false);
            detail = details.FirstOrDefault();
        }
        catch (NeteaseApiException)
        {
            // 搜索摘要仍可用于填充标签；歌词单独尝试。
        }

        var song = detail is null
            ? summary
            : new OnlineSongResult(
                OnlineMusicSource.Netease,
                summary.SourceId,
                string.IsNullOrWhiteSpace(detail.Name) ? summary.Title : detail.Name,
                string.IsNullOrWhiteSpace(detail.ArtistText) ? summary.Artist : detail.ArtistText,
                string.IsNullOrWhiteSpace(detail.Album.Name) ? summary.Album : detail.Album.Name,
                detail.DurationMs > 0 ? detail.DurationMs : summary.DurationMs,
                ParseYear(detail.ReleaseDate) ?? summary.Year,
                detail.TrackNumber > 0 ? (uint)detail.TrackNumber : summary.TrackNumber,
                detail.DiscNumber > 0 ? (uint)detail.DiscNumber : summary.DiscNumber,
                summary.Genre,
                summary.Composer,
                string.IsNullOrWhiteSpace(detail.CoverUrl) ? summary.CoverUrl : detail.CoverUrl);

        string? lyrics = null;
        try
        {
            var raw = await _netease.GetLyricsAsync(songId, cancellationToken).ConfigureAwait(false);
            lyrics = FirstNonBlank(raw.Lrc, raw.Yrc, raw.TranslatedLrc);
        }
        catch (NeteaseApiException)
        {
            // 没有歌词权限/数据时仍可应用标签。
        }

        return new OnlineSongDetails(song, lyrics);
    }

    private async Task<IReadOnlyList<OnlineSongResult>> SearchQqAsync(
        string keyword,
        int limit,
        CancellationToken cancellationToken)
    {
        var safeLimit = Math.Clamp(limit, 1, 30).ToString(CultureInfo.InvariantCulture);
        var uri = new Uri(
            QqSearchBase,
            $"?p=1&n={safeLimit}&w={Uri.EscapeDataString(keyword)}&format=json&cr=1&new_json=1");
        var body = await SendStringAsync(uri, "https://y.qq.com/", cancellationToken).ConfigureAwait(false);

        using var document = JsonDocument.Parse(body);
        var root = document.RootElement;
        if (root.TryGetProperty("code", out var code) && code.TryGetInt32(out var codeValue) && codeValue != 0)
        {
            throw new InvalidOperationException($"QQ 音乐搜索失败：code={codeValue}");
        }

        if (!root.TryGetProperty("data", out var data) ||
            !data.TryGetProperty("song", out var songSection) ||
            !songSection.TryGetProperty("list", out var list) ||
            list.ValueKind != JsonValueKind.Array)
        {
            return Array.Empty<OnlineSongResult>();
        }

        var results = new List<OnlineSongResult>();
        foreach (var item in list.EnumerateArray())
        {
            var title = GetString(item, "title") ?? GetString(item, "name");
            var mid = GetString(item, "mid");
            if (string.IsNullOrWhiteSpace(title) || string.IsNullOrWhiteSpace(mid))
            {
                continue;
            }

            var artist = ReadQqSingers(item);
            var albumName = string.Empty;
            var albumMid = string.Empty;
            var release = string.Empty;
            if (item.TryGetProperty("album", out var album) && album.ValueKind == JsonValueKind.Object)
            {
                albumName = GetString(album, "name") ?? string.Empty;
                albumMid = GetString(album, "mid") ?? string.Empty;
                release = GetString(album, "time_public") ?? string.Empty;
            }

            var year = ParseYear(release) ?? ParseYear(GetString(item, "time_public"));
            var track = GetUInt(item, "index_album");
            var disc = GetUInt(item, "index_cd");
            var duration = GetLong(item, "interval");
            var coverUrl = string.IsNullOrWhiteSpace(albumMid)
                ? null
                : $"https://y.gtimg.cn/music/photo_new/T002R300x300M000{albumMid}.jpg";

            results.Add(new OnlineSongResult(
                OnlineMusicSource.QQMusic,
                mid,
                title,
                artist,
                albumName,
                duration > 0 ? duration * 1000 : 0,
                year,
                track,
                disc is uint d && d > 0 ? d + 1 : (disc is not null ? 1u : null),
                null,
                null,
                coverUrl));
        }

        return results;
    }

    private async Task<OnlineSongDetails?> GetQqDetailsAsync(
        OnlineSongResult song,
        CancellationToken cancellationToken)
    {
        try
        {
            var uri = new Uri(
                QqLyricBase,
                $"?songmid={Uri.EscapeDataString(song.SourceId)}&format=json&nobase64=1&g_tk=5381");
            var body = await SendStringAsync(uri, "https://y.qq.com/", cancellationToken).ConfigureAwait(false);
            using var document = JsonDocument.Parse(body);
            var root = document.RootElement;
            if (root.TryGetProperty("retcode", out var retcode) &&
                retcode.TryGetInt32(out var retcodeValue) &&
                retcodeValue != 0)
            {
                return new OnlineSongDetails(song, null);
            }

            var lyrics = GetString(root, "lyric");
            return new OnlineSongDetails(song, string.IsNullOrWhiteSpace(lyrics) ? null : WebUtility.HtmlDecode(lyrics));
        }
        catch (Exception ex) when (ex is HttpRequestException or JsonException or InvalidOperationException)
        {
            return new OnlineSongDetails(song, null);
        }
    }

    private async Task<string> SendStringAsync(Uri uri, string referer, CancellationToken cancellationToken)
    {
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(_requestTimeout);
        using var request = new HttpRequestMessage(HttpMethod.Get, uri);
        request.Headers.TryAddWithoutValidation("User-Agent", QqUserAgent);
        request.Headers.TryAddWithoutValidation("Referer", referer);
        request.Headers.TryAddWithoutValidation("Accept", "*/*");
        using var response = await _http
            .SendAsync(request, HttpCompletionOption.ResponseHeadersRead, timeout.Token)
            .ConfigureAwait(false);
        var body = await response.Content.ReadAsStringAsync(timeout.Token).ConfigureAwait(false);
        if (!response.IsSuccessStatusCode)
        {
            throw new HttpRequestException($"HTTP {(int)response.StatusCode} {response.ReasonPhrase}");
        }

        return body;
    }

    private static OnlineSongResult FromNetease(NeteaseSong song) =>
        new(
            OnlineMusicSource.Netease,
            song.Id.ToString(CultureInfo.InvariantCulture),
            song.Name,
            song.ArtistText,
            song.Album.Name,
            song.DurationMs,
            ParseYear(song.ReleaseDate),
            song.TrackNumber > 0 ? (uint)song.TrackNumber : null,
            song.DiscNumber > 0 ? (uint)song.DiscNumber : null,
            null,
            null,
            song.CoverUrl);

    private static string ReadQqSingers(JsonElement item)
    {
        if (!item.TryGetProperty("singer", out var singers) || singers.ValueKind != JsonValueKind.Array)
        {
            return string.Empty;
        }

        return string.Join(
            "/",
            singers
                .EnumerateArray()
                .Select(singer => GetString(singer, "name"))
                .Where(name => !string.IsNullOrWhiteSpace(name)));
    }

    private static string? GetString(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String
            ? value.GetString()
            : null;

    private static long GetLong(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value) && value.TryGetInt64(out var number) ? number : 0;

    private static uint? GetUInt(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value) && value.TryGetUInt32(out var number) ? number : null;

    private static uint? ParseYear(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        var text = value.Trim();
        if (text.Length < 4)
        {
            return null;
        }

        return uint.TryParse(text[..4], NumberStyles.Integer, CultureInfo.InvariantCulture, out var year) &&
               year is > 0 and < 3000
            ? year
            : null;
    }

    private static string? FirstNonBlank(params string?[] values) =>
        values.FirstOrDefault(value => !string.IsNullOrWhiteSpace(value));
}
