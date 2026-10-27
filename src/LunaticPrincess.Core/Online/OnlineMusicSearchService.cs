using System.Globalization;
using System.Net;
using System.Text;
using System.Text.Json;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Netease;

namespace LunaticPrincess.Core.Online;

/// <summary>联网搜索实现：网易云走现有 API 客户端，QQ 音乐走公开搜索/歌词接口。</summary>
public sealed class OnlineMusicSearchService : IOnlineMusicSearchService
{
    private const string QqUserAgent =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

    private const string QqLegacySearchPath = "/soso/fcgi-bin/client_search_cp";
    private const string QqLegacyLyricPath = "/lyric/fcgi-bin/fcg_query_lyric_new.fcg";

    // c.y.qq.com 会整站返回 HTTP 500，依次回退到可用镜像域名。
    private static readonly string[] QqLegacyHosts = { "https://c.y.qq.com", "https://c6.y.qq.com" };
    private static readonly Uri QqPlayLyricUri = new("https://u.y.qq.com/cgi-bin/musicu.fcg");

    private readonly INeteaseApiClient _netease;
    private readonly HttpClient _http;
    private readonly ILyricParser _lyrics;
    private readonly TimeSpan _requestTimeout;

    public OnlineMusicSearchService(
        INeteaseApiClient netease,
        HttpClient http,
        ILyricParser lyrics,
        TimeSpan? requestTimeout = null)
    {
        _netease = netease ?? throw new ArgumentNullException(nameof(netease));
        _http = http ?? throw new ArgumentNullException(nameof(http));
        _lyrics = lyrics ?? throw new ArgumentNullException(nameof(lyrics));
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

        string? lineLyrics = null;
        string? wordLyrics = null;
        string? bilingualLineLyrics = null;
        string? bilingualWordLyrics = null;
        try
        {
            var raw = await _netease.GetLyricsAsync(songId, cancellationToken).ConfigureAwait(false);
            var parsed = _lyrics.Parse(raw);
            lineLyrics = _lyrics.ToPlainLrc(parsed, includeTranslation: false);
            wordLyrics = parsed.HasWordLevel
                ? _lyrics.ToEnhancedLrc(parsed, includeTranslation: false)
                : null;
            if (parsed.Translation is not null)
            {
                bilingualLineLyrics = _lyrics.ToPlainLrc(parsed, includeTranslation: true);
                bilingualWordLyrics = parsed.HasWordLevel
                    ? _lyrics.ToEnhancedLrc(parsed, includeTranslation: true)
                    : null;
            }
        }
        catch (NeteaseApiException)
        {
            // 没有歌词权限/数据时仍可应用标签和封面。
        }

        var cover = await TryDownloadCoverAsync(song.CoverUrl, cancellationToken).ConfigureAwait(false);
        return new OnlineSongDetails(song, lineLyrics, wordLyrics, cover, bilingualLineLyrics, bilingualWordLyrics);
    }

    private async Task<IReadOnlyList<OnlineSongResult>> SearchQqAsync(
        string keyword,
        int limit,
        CancellationToken cancellationToken)
    {
        var safeLimit = Math.Clamp(limit, 1, 30).ToString(CultureInfo.InvariantCulture);
        var body = await SendQqLegacyAsync(
            $"{QqLegacySearchPath}?p=1&n={safeLimit}&w={Uri.EscapeDataString(keyword)}&format=json&cr=1&new_json=1",
            cancellationToken).ConfigureAwait(false);

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

            var numericId = GetLong(item, "id");
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
                coverUrl,
                numericId > 0 ? numericId.ToString(CultureInfo.InvariantCulture) : null));
        }

        return results;
    }

    private async Task<OnlineSongDetails?> GetQqDetailsAsync(
        OnlineSongResult song,
        CancellationToken cancellationToken)
    {
        var cover = await TryDownloadCoverAsync(song.CoverUrl, cancellationToken).ConfigureAwait(false);

        // 优先走 musichall 的 PlayLyricInfo：它同时返回原文和翻译（QQ 的旧 fcg 接口通常没有翻译）。
        try
        {
            var playLyric = await TryGetQqPlayLyricInfoAsync(song, cancellationToken).ConfigureAwait(false);
            if (playLyric is { } info)
            {
                return new OnlineSongDetails(song, info.Line, info.Word, cover, info.Bilingual, info.BilingualWord);
            }
        }
        catch (Exception ex) when (ex is HttpRequestException or JsonException or InvalidOperationException or FormatException)
        {
            // 回退到旧歌词接口，至少保留原文。
        }

        try
        {
            var body = await SendQqLegacyAsync(
                $"{QqLegacyLyricPath}?songmid={Uri.EscapeDataString(song.SourceId)}&format=json&nobase64=1&g_tk=5381&trans=1&trans_t=0",
                cancellationToken).ConfigureAwait(false);
            using var document = JsonDocument.Parse(body);
            var root = document.RootElement;
            if (root.TryGetProperty("retcode", out var retcode) &&
                retcode.TryGetInt32(out var retcodeValue) &&
                retcodeValue != 0)
            {
                return new OnlineSongDetails(song, null, null, cover);
            }

            var lyrics = GetString(root, "lyric");
            var translation = GetString(root, "trans");
            if (string.IsNullOrWhiteSpace(lyrics))
            {
                return new OnlineSongDetails(song, null, null, cover);
            }

            var decodedLyrics = WebUtility.HtmlDecode(lyrics);
            var decodedTranslation = string.IsNullOrWhiteSpace(translation)
                ? null
                : WebUtility.HtmlDecode(translation);
            if (string.IsNullOrWhiteSpace(decodedTranslation))
            {
                return new OnlineSongDetails(song, decodedLyrics, null, cover);
            }

            var parsed = _lyrics.Parse(new RawLyrics(decodedLyrics, decodedTranslation));
            var line = _lyrics.ToPlainLrc(parsed, includeTranslation: false);
            var bilingual = _lyrics.ToPlainLrc(parsed, includeTranslation: true);
            return new OnlineSongDetails(song, line, null, cover, bilingual, null);
        }
        catch (Exception ex) when (ex is HttpRequestException or JsonException or InvalidOperationException)
        {
            return new OnlineSongDetails(song, null, null, cover);
        }
    }

    private async Task<(string? Line, string? Word, string? Bilingual, string? BilingualWord)?> TryGetQqPlayLyricInfoAsync(
        OnlineSongResult song,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(song.NumericSourceId) ||
            !long.TryParse(song.NumericSourceId, NumberStyles.Integer, CultureInfo.InvariantCulture, out var songId) ||
            songId <= 0)
        {
            return null;
        }

        var interval = Math.Max(0, song.DurationMs / 1000);
        var payload = new
        {
            comm = new
            {
                ct = 19,
                cv = 2111,
                v = 2111,
                tmeAppID = "qqmusiclight",
                format = "json",
                nettype = "NETWORK_WIFI",
                udid = "0",
            },
            request = new
            {
                method = "GetPlayLyricInfo",
                module = "music.musichallSong.PlayLyricInfo",
                param = new
                {
                    albumName = ToBase64(song.Album),
                    // crypt=1 + qrc=1：返回加密的逐字歌词（QRC），解密后可得到逐字时间轴。
                    crypt = 1,
                    ct = 19,
                    cv = 2111,
                    interval,
                    lrc_t = 0,
                    qrc = 1,
                    qrc_t = 0,
                    roma = 1,
                    roma_t = 0,
                    singerName = ToBase64(song.Artist),
                    songID = songId,
                    songName = ToBase64(song.Title),
                    trans = 1,
                    trans_t = 0,
                    type = 0,
                },
            },
        };

        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(_requestTimeout);
        using var request = new HttpRequestMessage(HttpMethod.Post, QqPlayLyricUri)
        {
            Content = new StringContent(
                JsonSerializer.Serialize(payload),
                Encoding.UTF8,
                "application/json"),
        };
        request.Headers.TryAddWithoutValidation("User-Agent", "okhttp/3.14.9");
        request.Headers.TryAddWithoutValidation("Referer", "https://y.qq.com/");
        using var response = await _http.SendAsync(request, timeout.Token).ConfigureAwait(false);
        if (!response.IsSuccessStatusCode)
        {
            return null;
        }

        var body = await response.Content.ReadAsStringAsync(timeout.Token).ConfigureAwait(false);
        using var document = JsonDocument.Parse(body);
        var root = document.RootElement;
        if (root.TryGetProperty("code", out var code) &&
            code.TryGetInt32(out var codeValue) &&
            codeValue != 0)
        {
            return null;
        }

        if (!root.TryGetProperty("request", out var requestElement) ||
            !requestElement.TryGetProperty("data", out var data))
        {
            return null;
        }

        // crypt=1 时 lyric 字段是十六进制密文（3DES + zlib 压缩的 QRC）；
        // 没有逐字资源时仍是 base64 的普通 LRC 文本，两种都兼容。
        var lyricField = GetString(data, "lyric");
        var wordText = QrcDecryptor.TryDecrypt(lyricField);
        var lineText = wordText is null ? DecodeBase64(lyricField) : null;
        if (string.IsNullOrWhiteSpace(wordText) && string.IsNullOrWhiteSpace(lineText))
        {
            return null;
        }

        var translation = DecodeQqLyricField(GetString(data, "trans"));
        var romanization = DecodeQqLyricField(GetString(data, "roma"));
        var parsed = _lyrics.Parse(new RawLyrics(lineText, translation, romanization, Qrc: wordText));
        var original = _lyrics.ToPlainLrc(parsed, includeTranslation: false);
        var bilingual = _lyrics.ToPlainLrc(parsed, includeTranslation: true);
        if (!parsed.HasWordLevel)
        {
            return (original, null, bilingual, null);
        }

        var word = _lyrics.ToEnhancedLrc(parsed, includeTranslation: false);
        var bilingualWord = _lyrics.ToEnhancedLrc(parsed, includeTranslation: true);
        return (original, word, bilingual, bilingualWord);
    }

    /// <summary>解码 QQ 音乐的歌词字段：加密的 QRC 先解密再转成逐行 LRC，否则按 base64 文本解码。</summary>
    private string? DecodeQqLyricField(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        var decrypted = QrcDecryptor.TryDecrypt(value);
        if (decrypted is not null)
        {
            var converted = _lyrics.ToPlainLrc(
                _lyrics.Parse(new RawLyrics(Lrc: null, Qrc: decrypted)),
                includeTranslation: false);
            return string.IsNullOrWhiteSpace(converted) ? null : converted;
        }

        return DecodeBase64(value);
    }

    private static string ToBase64(string? value) =>
        Convert.ToBase64String(Encoding.UTF8.GetBytes(value ?? string.Empty));

    private static string? DecodeBase64(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        try
        {
            return Encoding.UTF8.GetString(Convert.FromBase64String(value));
        }
        catch (FormatException)
        {
            return null;
        }
    }

    private async Task<byte[]?> TryDownloadCoverAsync(string? coverUrl, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(coverUrl))
        {
            return null;
        }

        try
        {
            using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            timeout.CancelAfter(_requestTimeout);
            using var request = new HttpRequestMessage(HttpMethod.Get, coverUrl);
            request.Headers.TryAddWithoutValidation("User-Agent", QqUserAgent);
            request.Headers.TryAddWithoutValidation("Referer", "https://y.qq.com/");
            using var response = await _http
                .SendAsync(request, HttpCompletionOption.ResponseHeadersRead, timeout.Token)
                .ConfigureAwait(false);
            if (!response.IsSuccessStatusCode)
            {
                return null;
            }

            if (response.Content.Headers.ContentLength is > 10 * 1024 * 1024)
            {
                return null;
            }

            var bytes = await response.Content.ReadAsByteArrayAsync(timeout.Token).ConfigureAwait(false);
            return bytes.Length is > 0 and <= 10 * 1024 * 1024 ? bytes : null;
        }
        catch (Exception ex) when (ex is HttpRequestException or OperationCanceledException or IOException)
        {
            return null;
        }
    }

    /// <summary>QQ 旧版 fcg 接口：按顺序尝试各镜像域名，全部失败才抛出异常。</summary>
    private async Task<string> SendQqLegacyAsync(string pathAndQuery, CancellationToken cancellationToken)
    {
        HttpRequestException? lastError = null;
        foreach (var host in QqLegacyHosts)
        {
            try
            {
                return await SendStringAsync(new Uri(host + pathAndQuery), "https://y.qq.com/", cancellationToken).ConfigureAwait(false);
            }
            catch (HttpRequestException ex)
            {
                lastError = ex;
            }
        }

        throw lastError ?? new HttpRequestException("QQ 音乐接口不可用。");
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
