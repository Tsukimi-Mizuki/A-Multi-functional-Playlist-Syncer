using System.Text;
using NeteaseMusicManager.Core.Online;
using NeteaseMusicManager.Core.Tags;

namespace NeteaseMusicManager.Core.Lyrics;

public enum BatchLyricsSourcePreference
{
    Auto = 0,
    Netease = 1,
    QQMusic = 2,
}

public sealed record BatchLyricsOptions(
    BatchLyricsSourcePreference Source = BatchLyricsSourcePreference.Auto,
    bool PreferWordLyrics = false,
    bool OverwriteExisting = false,
    bool WriteLrcFile = false);

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
/// 批量歌词：扫描本地音频，用标签/文件名搜索网易云或 QQ 音乐，按标题、歌手和时长选最佳匹配，
/// 再把双语歌词写回标签（可选同时生成 .lrc）。匹配逻辑参考 LDDC 的本地歌曲精确匹配思路。
/// </summary>
public sealed class BatchLyricsService : IBatchLyricsService
{
    private static readonly string[] AudioExtensions =
    [
        ".mp3", ".flac", ".m4a", ".ogg", ".wav", ".aac", ".wma", ".opus",
    ];

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
        var fileName = Path.GetFileName(filePath);
        if (!File.Exists(filePath))
        {
            return Fail(filePath, "文件不存在");
        }

        var info = await _tags.ReadAsync(filePath, cancellationToken).ConfigureAwait(false);
        if (info is null)
        {
            return Fail(filePath, "无法读取音频标签");
        }

        if (!options.OverwriteExisting && !string.IsNullOrWhiteSpace(info.Lyrics))
        {
            return new BatchLyricsResult(filePath, false, true, "已有歌词，跳过");
        }

        var (localTitle, localArtist) = ResolveLocalInfo(filePath, info);
        if (string.IsNullOrWhiteSpace(localTitle))
        {
            return Fail(filePath, "无法从标签或文件名识别歌曲名");
        }

        var sources = options.Source switch
        {
            BatchLyricsSourcePreference.Auto => new[] { OnlineMusicSource.Netease, OnlineMusicSource.QQMusic },
            BatchLyricsSourcePreference.Netease => new[] { OnlineMusicSource.Netease },
            _ => new[] { OnlineMusicSource.QQMusic },
        };

        OnlineSongResult? matched = null;
        double bestScore = 0;
        var query = string.IsNullOrWhiteSpace(localArtist) ? localTitle : $"{localTitle} {localArtist}";
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

            var candidate = FindBest(candidates, localTitle, localArtist, info.DurationMs, out var score);
            if (candidate is not null && score > bestScore)
            {
                matched = candidate;
                bestScore = score;
            }

            if (candidate is not null && IsAcceptable(score))
            {
                break;
            }
        }

        if (matched is null || !IsAcceptable(bestScore))
        {
            return Fail(filePath, "未找到匹配度足够的在线歌曲");
        }

        OnlineSongDetails? details;
        try
        {
            details = await _online.GetDetailsAsync(matched, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception) when (!cancellationToken.IsCancellationRequested)
        {
            return Fail(filePath, "在线歌词请求失败");
        }

        var lyrics = details?.GetLyrics(options.PreferWordLyrics, bilingual: true);
        if (string.IsNullOrWhiteSpace(lyrics))
        {
            return new BatchLyricsResult(
                filePath,
                false,
                false,
                "匹配成功，但该来源没有歌词",
                matched.Title,
                matched.Artist,
                matched.SourceLabel);
        }

        var useWord = options.PreferWordLyrics && details?.HasWordLyrics == true;
        var format = useWord
            ? details!.HasTranslation ? "逐字双语" : "逐字"
            : details?.HasTranslation == true ? "逐行双语" : "逐行";

        await _tags.WriteAsync(
            filePath,
            new AudioTagData
            {
                PlainLyrics = useWord ? null : lyrics,
                WordLyrics = useWord ? lyrics : null,
            },
            cancellationToken).ConfigureAwait(false);

        var lrcWarning = string.Empty;
        if (options.WriteLrcFile)
        {
            try
            {
                var lrcPath = Path.ChangeExtension(filePath, ".lrc");
                await File.WriteAllTextAsync(lrcPath, lyrics, new UTF8Encoding(false), cancellationToken).ConfigureAwait(false);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                lrcWarning = "（.lrc 写入失败）";
            }
        }

        return new BatchLyricsResult(
            filePath,
            true,
            false,
            $"已写入{format}歌词{lrcWarning}",
            matched.Title,
            matched.Artist,
            matched.SourceLabel,
            format);
    }

    private static BatchLyricsResult Fail(string path, string message) =>
        new(path, false, false, message);

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

        if (a == b || a.Contains(b, StringComparison.Ordinal) || b.Contains(a, StringComparison.Ordinal))
        {
            return 1;
        }

        return Similarity(a, b);
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
