using System.Text;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Online;
using LunaticPrincess.Core.Tags;

namespace LunaticPrincess.Core.Lyrics;

public sealed record BatchLyricsOptions(
    OnlineSourcePriority TagPriority = OnlineSourcePriority.NeteaseFirst,
    OnlineSourcePriority LyricsPriority = OnlineSourcePriority.NeteaseFirst,
    bool OverwriteExisting = false,
    bool FetchLyrics = true,
    bool PreferWordLyrics = false);

public sealed record BatchLyricsResult(
    string FilePath,
    bool Success,
    bool Skipped,
    string Message,
    string? MatchedTitle = null,
    string? MatchedArtist = null,
    string? SourceLabel = null,
    string? LyricsFormat = null);

public interface IBatchLyricsService
{
    Task<IReadOnlyList<string>> ScanAudioFilesAsync(
        string directory,
        bool recursive,
        CancellationToken cancellationToken = default);

    Task<BatchLyricsResult> FetchForFileAsync(
        string filePath,
        BatchLyricsOptions options,
        CancellationToken cancellationToken = default);
}

/// <summary>
/// 批量标签与封面：扫描本地音频，用标签/文件名搜索网易云或 QQ 音乐，按标题、歌手和时长选最佳匹配，
/// 再把标题、歌手、专辑、年份、音轨/碟号、风格、作曲家和封面写回标签；
/// 不读取/修改歌词。匹配逻辑参考 LDDC 的本地歌曲精确匹配思路。
/// </summary>
public sealed class BatchLyricsService : IBatchLyricsService
{
    private static readonly string[] AudioExtensions =
    [
        ".mp3", ".flac", ".m4a", ".ogg", ".wav", ".aac", ".wma", ".opus",
    ];

    /// <summary>标题里的并列分隔符（MusicTag 的 [\/&amp;,] 思路，另外补上全角与中文写法）。</summary>
    private static readonly char[] TitleSeparators = ['/', '&', ',', '，', '、', ';', '；', '·', '|'];

    /// <summary>歌手里常见的多值分隔符。</summary>
    private static readonly char[] ArtistSeparators = ['/', '&', ',', '，', '、', ';', '；', '·', '・', '×', '|', '+'];

    /// <summary>歌手字段里的连接词，与分隔符等价。</summary>
    private static readonly string[] ArtistKeywords = ["feat.", "ft.", "featuring", "cv:", "vs."];

    /// <summary>多值字段切段用的内部占位符（不会是真实字符）。</summary>
    private const char MultiValueMarker = '\u0001';

    private const string MultiValueMarkerText = "\u0001";

    private readonly IAudioTagService _tags;
    private readonly IOnlineMusicSearchService _online;

    public BatchLyricsService(IAudioTagService tags, IOnlineMusicSearchService online)
    {
        _tags = tags ?? throw new ArgumentNullException(nameof(tags));
        _online = online ?? throw new ArgumentNullException(nameof(online));
    }

    public Task<IReadOnlyList<string>> ScanAudioFilesAsync(
        string directory,
        bool recursive,
        CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(directory) || !Directory.Exists(directory))
        {
            return Task.FromResult<IReadOnlyList<string>>(Array.Empty<string>());
        }

        return Task.Run<IReadOnlyList<string>>(() =>
        {
            var option = recursive ? SearchOption.AllDirectories : SearchOption.TopDirectoryOnly;
            return Directory
                .EnumerateFiles(directory, "*", option)
                .Where(path => AudioExtensions.Contains(Path.GetExtension(path), StringComparer.OrdinalIgnoreCase))
                .OrderBy(path => path, StringComparer.CurrentCultureIgnoreCase)
                .ToList();
        }, cancellationToken);
    }

    public async Task<BatchLyricsResult> FetchForFileAsync(
        string filePath,
        BatchLyricsOptions options,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(options);
        if (!File.Exists(filePath))
        {
            return Fail(filePath, "文件不存在");
        }

        var info = await _tags.ReadAsync(filePath, cancellationToken).ConfigureAwait(false);
        if (info is null)
        {
            return Fail(filePath, "无法读取音频标签");
        }

        var (localTitle, localArtist) = ResolveLocalInfo(filePath, info);
        if (string.IsNullOrWhiteSpace(localTitle))
        {
            return Fail(filePath, "无法从标签或文件名识别歌曲名");
        }

        var query = string.IsNullOrWhiteSpace(localArtist) ? localTitle : $"{localTitle} {localArtist}";
        var tagDetails = await FindBestDetailsAsync(
            query,
            localTitle,
            localArtist,
            info.DurationMs,
            OrderSources(options.TagPriority),
            requireLyrics: false,
            preferWord: false,
            cancellationToken).ConfigureAwait(false);

        var replace = options.OverwriteExisting;
        var shouldFetchLyrics = options.FetchLyrics &&
                                (replace || string.IsNullOrWhiteSpace(info.Lyrics));
        var lyricsDetails = shouldFetchLyrics
            ? await FindBestDetailsAsync(
                query,
                localTitle,
                localArtist,
                info.DurationMs,
                OrderSources(options.LyricsPriority),
                requireLyrics: true,
                preferWord: options.PreferWordLyrics,
                cancellationToken).ConfigureAwait(false)
            : null;

        var metadataSource = tagDetails ?? lyricsDetails;
        if (metadataSource is null)
        {
            return Fail(filePath, "未找到匹配度足够的在线歌曲");
        }

        var tagSong = metadataSource.Song;
        var cover = tagDetails?.CoverData ?? lyricsDetails?.CoverData;

        string? lyrics = null;
        var lyricsFormat = string.Empty;
        var writesLyrics = false;
        var useWord = false;
        if (shouldFetchLyrics && lyricsDetails is not null)
        {
            lyrics = lyricsDetails.GetLyrics(options.PreferWordLyrics, bilingual: true);
            if (!string.IsNullOrWhiteSpace(lyrics))
            {
                useWord = options.PreferWordLyrics && lyricsDetails.HasWordLyrics;
                writesLyrics = true;
                lyricsFormat = useWord
                    ? lyricsDetails.HasTranslation ? "双语逐字歌词" : "逐字歌词"
                    : lyricsDetails.HasTranslation ? "双语逐行歌词" : "逐行歌词";
            }
        }

        var tagData = new AudioTagData
        {
            Title = replace || string.IsNullOrWhiteSpace(info.Title) ? NullIfBlank(tagSong.Title) : null,
            Artists = replace || string.IsNullOrWhiteSpace(info.Artist) ? SplitArtists(tagSong.Artist) : Array.Empty<string>(),
            Album = replace || string.IsNullOrWhiteSpace(info.Album) ? NullIfBlank(tagSong.Album) : null,
            Year = replace || info.Year is null ? tagSong.Year : null,
            TrackNumber = replace || info.TrackNumber is null ? tagSong.TrackNumber : null,
            DiscNumber = replace || info.DiscNumber is null ? tagSong.DiscNumber : null,
            Genre = replace || string.IsNullOrWhiteSpace(info.Genre) ? NullIfBlank(tagSong.Genre) : null,
            Composer = replace || string.IsNullOrWhiteSpace(info.Composer) ? NullIfBlank(tagSong.Composer) : null,
            CoverData = replace || !info.HasCover ? cover : null,
            PlainLyrics = writesLyrics && !useWord ? lyrics : null,
            WordLyrics = writesLyrics && useWord ? lyrics : null,
        };

        var writesTags =
            tagData.Title is not null ||
            tagData.Artists.Count > 0 ||
            tagData.Album is not null ||
            tagData.Year is not null ||
            tagData.TrackNumber is not null ||
            tagData.DiscNumber is not null ||
            tagData.Genre is not null ||
            tagData.Composer is not null;
        var writesCover = tagData.CoverData is { Length: > 0 };

        if (!writesTags && !writesCover && !writesLyrics)
        {
            return new BatchLyricsResult(
                filePath,
                false,
                true,
                "标签、封面与歌词已完整，跳过",
                tagSong.Title,
                tagSong.Artist,
                metadataSource.Song.SourceLabel,
                lyricsFormat);
        }

        await _tags.WriteAsync(filePath, tagData, cancellationToken).ConfigureAwait(false);

        var parts = new List<string>();
        if (writesTags)
        {
            parts.Add("标签");
        }

        if (writesCover)
        {
            parts.Add("封面");
        }

        if (writesLyrics)
        {
            parts.Add(lyricsFormat);
        }

        var message = "已写入" + string.Join("、", parts);
        if (shouldFetchLyrics && !writesLyrics)
        {
            message += "（未获取到歌词）";
        }

        return new BatchLyricsResult(
            filePath,
            true,
            false,
            message,
            tagSong.Title,
            tagSong.Artist,
            metadataSource.Song.SourceLabel,
            lyricsFormat);
    }

    private async Task<OnlineSongDetails?> FindBestDetailsAsync(
        string query,
        string localTitle,
        string localArtist,
        long localDurationMs,
        IReadOnlyList<OnlineMusicSource> sources,
        bool requireLyrics,
        bool preferWord,
        CancellationToken cancellationToken)
    {
        foreach (var source in sources)
        {
            IReadOnlyList<OnlineSongResult> candidates;
            try
            {
                candidates = await _online.SearchAsync(source, query, 5, cancellationToken).ConfigureAwait(false);
            }
            catch (Exception) when (!cancellationToken.IsCancellationRequested)
            {
                continue;
            }

            var candidate = FindBest(candidates, localTitle, localArtist, localDurationMs, out var score);
            if (candidate is null || !IsAcceptable(score))
            {
                continue;
            }

            OnlineSongDetails? details;
            try
            {
                details = await _online.GetDetailsAsync(candidate, cancellationToken).ConfigureAwait(false);
            }
            catch (Exception) when (!cancellationToken.IsCancellationRequested)
            {
                continue;
            }

            if (details is null)
            {
                continue;
            }

            if (requireLyrics && string.IsNullOrWhiteSpace(details.GetLyrics(preferWord, bilingual: true)))
            {
                continue;
            }

            return details;
        }

        return null;
    }

    private static IReadOnlyList<OnlineMusicSource> OrderSources(OnlineSourcePriority priority) =>
        priority switch
        {
            OnlineSourcePriority.QQMusicFirst =>
                [OnlineMusicSource.QQMusic, OnlineMusicSource.Netease],
            OnlineSourcePriority.NeteaseOnly =>
                [OnlineMusicSource.Netease],
            OnlineSourcePriority.QQMusicOnly =>
                [OnlineMusicSource.QQMusic],
            _ => [OnlineMusicSource.Netease, OnlineMusicSource.QQMusic],
        };

    private static BatchLyricsResult Fail(string path, string message) =>
        new(path, false, false, message);

    private static string? NullIfBlank(string? value) =>
        string.IsNullOrWhiteSpace(value) ? null : value.Trim();

    private static IReadOnlyList<string> SplitArtists(string? value) =>
        string.IsNullOrWhiteSpace(value)
            ? Array.Empty<string>()
            : value
                .Split(new[] { '/', ';', '；', '、' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                .ToArray();

    private static (string Title, string Artist) ResolveLocalInfo(string path, AudioTagInfo info)
    {
        var title = info.Title?.Trim() ?? string.Empty;
        var artist = info.Artist?.Trim() ?? string.Empty;
        if (!string.IsNullOrWhiteSpace(title))
        {
            return (title, artist);
        }

        var name = Path.GetFileNameWithoutExtension(path).Trim();
        name = System.Text.RegularExpressions.Regex.Replace(
            name,
            @"^\s*\d{1,3}[\s._-]+",
            string.Empty);
        name = name.Replace('_', ' ').Trim();

        // 文件名没有标签时无法可靠判断“歌手 - 标题”的顺序，直接用整个文件名作为标题，
        // 由搜索和相似度匹配处理，避免把标题和歌手拆反。
        return (name, artist);
    }

    private static OnlineSongResult? FindBest(
        IReadOnlyList<OnlineSongResult> candidates,
        string localTitle,
        string localArtist,
        long localDurationMs,
        out double bestScore)
    {
        OnlineSongResult? best = null;
        bestScore = 0;
        foreach (var candidate in candidates)
        {
            var titleScore = TitleScore(localTitle, candidate.Title);
            var artistScore = ArtistScore(localArtist, candidate.Artist);
            var durationScore = DurationScore(localDurationMs, candidate.DurationMs);
            var score = !string.IsNullOrWhiteSpace(localArtist)
                ? titleScore * 0.60 + artistScore * 0.25 + durationScore * 0.15
                : titleScore * 0.75 + durationScore * 0.25;
            if (score > bestScore)
            {
                bestScore = score;
                best = candidate;
            }
        }

        return best;
    }

    private static bool IsAcceptable(double score) => score >= 0.55;

    private static double TitleScore(string localTitle, string resultTitle)
    {
        var a = Normalize(localTitle);
        var b = Normalize(resultTitle);
        if (a.Length == 0 || b.Length == 0)
        {
            return 0;
        }

        var best = a == b ? 1 : Compare(a, b);

        // MusicTag 桌面端思路：标题常写成 “A / B”“A & B” 这类并列形式，
        // 按分隔符切段后逐段比较取最佳，命中某一段时轻微降权（0.99）作为惩罚。
        foreach (var segment in SplitMultiValue(localTitle, TitleSeparators, Array.Empty<string>()))
        {
            var normalized = Normalize(segment);
            if (normalized.Length < 2)
            {
                continue;
            }

            var score = normalized == b ? 0.99 : Compare(normalized, b) * 0.98;
            if (score > best)
            {
                best = score;
            }
        }

        return best;
    }

    private static double Compare(string a, string b)
    {
        if (a == b)
        {
            return 1;
        }

        if (a.Length >= 3 && b.Length >= 3 && (a.Contains(b, StringComparison.Ordinal) || b.Contains(a, StringComparison.Ordinal)))
        {
            return 0.88;
        }

        return Similarity(a, b);
    }

    private static double ArtistScore(string localArtist, string resultArtist)
    {
        var a = Normalize(localArtist);
        var b = Normalize(resultArtist);
        if (a.Length == 0 || b.Length == 0)
        {
            return 0.5;
        }

        var best = a == b || a.Contains(b, StringComparison.Ordinal) || b.Contains(a, StringComparison.Ordinal)
            ? 1
            : Similarity(a, b);

        // 歌手段位常是 “A/B”“A feat. B”“A · B”“张三 CV:李四” 这类多值写法
        // （LDDC 的 artist_str2list 与 MusicTag 的多歌手处理），切段后两两比较取最佳。
        foreach (var left in SplitMultiValue(localArtist, ArtistSeparators, ArtistKeywords))
        {
            var l = Normalize(left);
            if (l.Length == 0)
            {
                continue;
            }

            foreach (var right in SplitMultiValue(resultArtist, ArtistSeparators, ArtistKeywords))
            {
                var r = Normalize(right);
                if (r.Length == 0)
                {
                    continue;
                }

                var score = l == r
                    ? 0.98
                    : l.Contains(r, StringComparison.Ordinal) || r.Contains(l, StringComparison.Ordinal)
                        ? 0.9
                        : Similarity(l, r) * 0.97;
                if (score > best)
                {
                    best = score;
                }
            }
        }

        return best;
    }

    /// <summary>把 “A/B”“A &amp; B”“A feat. B” 之类的多值字段切成若干片段。</summary>
    private static IReadOnlyList<string> SplitMultiValue(string value, char[] separators, string[] keywords)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return Array.Empty<string>();
        }

        var hasSeparator = value.IndexOfAny(separators) >= 0 || keywords.Any(keyword => value.Contains(keyword, StringComparison.OrdinalIgnoreCase));
        if (!hasSeparator)
        {
            return Array.Empty<string>();
        }

        var working = value;
        foreach (var keyword in keywords)
        {
            working = working.Replace(keyword, MultiValueMarkerText, StringComparison.OrdinalIgnoreCase);
        }

        return working
            .Split(separators.Append(MultiValueMarker).ToArray(), StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
    }

    private static double DurationScore(long localMs, long resultMs)
    {
        if (localMs <= 0 || resultMs <= 0)
        {
            return 0.5;
        }

        var diff = Math.Abs(localMs - resultMs);
        return diff switch
        {
            <= 3000 => 1,
            <= 10000 => 0.75,
            <= 20000 => 0.4,
            _ => 0,
        };
    }

    private static string Normalize(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return string.Empty;
        }

        var form = value.Normalize(NormalizationForm.FormKC).ToLowerInvariant();
        var builder = new StringBuilder(form.Length);
        foreach (var ch in form)
        {
            if (char.IsLetterOrDigit(ch))
            {
                builder.Append(ch);
            }
        }

        return builder.ToString();
    }

    private static double Similarity(string a, string b)
    {
        if (a == b)
        {
            return 1;
        }

        var max = Math.Max(a.Length, b.Length);
        if (max == 0)
        {
            return 1;
        }

        var previous = new int[b.Length + 1];
        var current = new int[b.Length + 1];
        for (var j = 0; j <= b.Length; j++)
        {
            previous[j] = j;
        }

        for (var i = 1; i <= a.Length; i++)
        {
            current[0] = i;
            for (var j = 1; j <= b.Length; j++)
            {
                var cost = a[i - 1] == b[j - 1] ? 0 : 1;
                current[j] = Math.Min(
                    Math.Min(current[j - 1] + 1, previous[j] + 1),
                    previous[j - 1] + cost);
            }

            (previous, current) = (current, previous);
        }

        return 1.0 - (double)previous[b.Length] / max;
    }
}
