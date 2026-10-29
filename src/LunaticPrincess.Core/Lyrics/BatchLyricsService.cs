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
    bool PreferWordLyrics = false,
    double MinScore = LyricsMatchScorer.DefaultMinScore);

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
/// 批量标签与封面：扫描本地音频，用标签/文件名搜索网易云或 QQ 音乐，
/// 按 音樂標籤 的思路做多变体搜索与四路打分（标题/歌手/专辑/时长）重排，
/// 用匹配度阈值筛选后把标题、歌手、专辑、年份、音轨/碟号、风格、作曲家和封面写回标签。
/// 网易云 / QQ 都没有可用歌词时，再按顺序尝试备用词源（酷狗 KRC、LRCLIB），
/// 让冷门曲目与同人音乐也能补上逐行/逐字歌词。
/// </summary>
public sealed class BatchLyricsService : IBatchLyricsService
{
    private static readonly string[] AudioExtensions =
    [
        ".mp3", ".flac", ".m4a", ".ogg", ".wav", ".aac", ".wma", ".opus",
    ];

    /// <summary>每个音源最多尝试几组搜索关键词（多变体搜索的成本控制）。</summary>
    private const int MaxQueriesPerSource = 3;

    /// <summary>每次搜索请求的候选数量。</summary>
    private const int SearchLimit = 10;

    private readonly IAudioTagService _tags;
    private readonly IOnlineMusicSearchService _online;
    private readonly IReadOnlyList<ILyricsSource> _fallbackSources;

    public BatchLyricsService(
        IAudioTagService tags,
        IOnlineMusicSearchService online,
        IReadOnlyList<ILyricsSource>? fallbackSources = null)
    {
        _tags = tags ?? throw new ArgumentNullException(nameof(tags));
        _online = online ?? throw new ArgumentNullException(nameof(online));
        _fallbackSources = fallbackSources ?? Array.Empty<ILyricsSource>();
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

        var queries = LyricsMatchScorer.BuildQueries(localTitle, localArtist);
        var localAlbum = info.Album?.Trim();
        var minScore = Math.Clamp(options.MinScore, LyricsMatchScorer.MinThreshold, LyricsMatchScorer.MaxThreshold);

        var tagDetails = await FindBestDetailsAsync(
            queries,
            localTitle,
            localArtist,
            localAlbum,
            info.DurationMs,
            OrderSources(options.TagPriority),
            requireLyrics: false,
            preferWord: false,
            minScore,
            cancellationToken).ConfigureAwait(false);

        var replace = options.OverwriteExisting;
        var shouldFetchLyrics = options.FetchLyrics &&
                                (replace || string.IsNullOrWhiteSpace(info.Lyrics));
        var lyricsDetails = shouldFetchLyrics
            ? await FindBestDetailsAsync(
                queries,
                localTitle,
                localArtist,
                localAlbum,
                info.DurationMs,
                OrderSources(options.LyricsPriority),
                requireLyrics: true,
                preferWord: options.PreferWordLyrics,
                minScore,
                cancellationToken).ConfigureAwait(false)
            : null;

        // 网易云 / QQ 都没有可用歌词时，再按顺序问备用词源（酷狗、LRCLIB）。
        LyricsSourceCandidate? fallbackLyrics = null;
        var onlineLyrics = lyricsDetails?.GetLyrics(options.PreferWordLyrics, bilingual: true);
        if (shouldFetchLyrics && string.IsNullOrWhiteSpace(onlineLyrics) && _fallbackSources.Count > 0)
        {
            fallbackLyrics = await FindFallbackLyricsAsync(
                queries,
                localTitle,
                localArtist,
                localAlbum,
                info.DurationMs,
                options.PreferWordLyrics,
                minScore,
                cancellationToken).ConfigureAwait(false);
        }

        var metadataSource = tagDetails ?? lyricsDetails;
        if (metadataSource is null && fallbackLyrics is null)
        {
            return Fail(filePath, "未找到匹配度足够的在线歌曲");
        }

        var tagSong = metadataSource?.Song;
        var sourceLabel = tagSong?.SourceLabel ?? fallbackLyrics!.SourceLabel;
        var cover = tagDetails?.CoverData ?? lyricsDetails?.CoverData;

        string? lyrics = null;
        var lyricsFormat = string.Empty;
        var writesLyrics = false;
        var useWord = false;
        if (shouldFetchLyrics)
        {
            if (!string.IsNullOrWhiteSpace(onlineLyrics) && lyricsDetails is not null)
            {
                lyrics = onlineLyrics;
                useWord = options.PreferWordLyrics && lyricsDetails.HasWordLyrics;
                writesLyrics = true;
                lyricsFormat = FormatLabel(useWord, lyricsDetails.HasTranslation);
            }
            else if (fallbackLyrics is not null)
            {
                lyrics = fallbackLyrics.GetLyrics(options.PreferWordLyrics, bilingual: true);
                if (!string.IsNullOrWhiteSpace(lyrics))
                {
                    useWord = options.PreferWordLyrics && fallbackLyrics.HasWordLyrics;
                    writesLyrics = true;
                    lyricsFormat = FormatLabel(useWord, fallbackLyrics.HasTranslation);
                }
            }
        }

        var tagTitle = tagSong?.Title ?? fallbackLyrics?.Title;
        var tagArtist = tagSong?.Artist ?? fallbackLyrics?.Artist;
        var tagAlbum = tagSong?.Album ?? fallbackLyrics?.Album;
        var tagData = new AudioTagData
        {
            Title = replace || string.IsNullOrWhiteSpace(info.Title) ? NullIfBlank(tagTitle) : null,
            Artists = replace || string.IsNullOrWhiteSpace(info.Artist) ? SplitArtists(tagArtist) : Array.Empty<string>(),
            Album = replace || string.IsNullOrWhiteSpace(info.Album) ? NullIfBlank(tagAlbum) : null,
            Year = replace || info.Year is null ? tagSong?.Year : null,
            TrackNumber = replace || info.TrackNumber is null ? tagSong?.TrackNumber : null,
            DiscNumber = replace || info.DiscNumber is null ? tagSong?.DiscNumber : null,
            Genre = replace || string.IsNullOrWhiteSpace(info.Genre) ? NullIfBlank(tagSong?.Genre) : null,
            Composer = replace || string.IsNullOrWhiteSpace(info.Composer) ? NullIfBlank(tagSong?.Composer) : null,
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
                tagTitle,
                tagArtist,
                sourceLabel,
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
        if (writesLyrics && fallbackLyrics is not null)
        {
            message += $"（歌词来源：{fallbackLyrics.SourceLabel}）";
        }

        if (shouldFetchLyrics && !writesLyrics)
        {
            message += "（未获取到歌词）";
        }

        return new BatchLyricsResult(
            filePath,
            true,
            false,
            message,
            tagTitle,
            tagArtist,
            sourceLabel,
            lyricsFormat);
    }

    /// <summary>
    /// 依次询问备用词源：某个源抛异常或没有可用歌词时继续下一个，全部失败返回 null。
    /// 备用源只补歌词，不改写标签来源，因此不影响原有的标签/封面匹配逻辑。
    /// </summary>
    private async Task<LyricsSourceCandidate?> FindFallbackLyricsAsync(
        IReadOnlyList<string> queries,
        string localTitle,
        string localArtist,
        string? localAlbum,
        long localDurationMs,
        bool preferWord,
        double minScore,
        CancellationToken cancellationToken)
    {
        foreach (var source in _fallbackSources)
        {
            LyricsSourceCandidate? candidate;
            try
            {
                candidate = await source.FetchAsync(
                    queries,
                    localTitle,
                    localArtist,
                    localAlbum,
                    localDurationMs,
                    minScore,
                    cancellationToken).ConfigureAwait(false);
            }
            catch (Exception) when (!cancellationToken.IsCancellationRequested)
            {
                continue;
            }

            if (candidate is null || string.IsNullOrWhiteSpace(candidate.GetLyrics(preferWord, bilingual: true)))
            {
                continue;
            }

            return candidate;
        }

        return null;
    }

    private static string FormatLabel(bool useWord, bool hasTranslation) =>
        useWord
            ? hasTranslation ? "双语逐字歌词" : "逐字歌词"
            : hasTranslation ? "双语逐行歌词" : "逐行歌词";

    private async Task<OnlineSongDetails?> FindBestDetailsAsync(
        IReadOnlyList<string> queries,
        string localTitle,
        string localArtist,
        string? localAlbum,
        long localDurationMs,
        IReadOnlyList<OnlineMusicSource> sources,
        bool requireLyrics,
        bool preferWord,
        double minScore,
        CancellationToken cancellationToken)
    {
        foreach (var source in sources)
        {
            var best = await FindBestCandidateAsync(
                source,
                queries,
                localTitle,
                localArtist,
                localAlbum,
                localDurationMs,
                minScore,
                cancellationToken).ConfigureAwait(false);
            if (best is null)
            {
                continue;
            }

            OnlineSongDetails? details;
            try
            {
                details = await _online.GetDetailsAsync(best, cancellationToken).ConfigureAwait(false);
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

    /// <summary>
    /// 多变体搜索 + 打分重排（音樂標籤 思路）：按关键词顺序搜索同一音源，
    /// 标题、歌手、专辑、时长四路打分取最高分候选；达到可信匹配就不再换关键词，
    /// 全部关键词都低于匹配度阈值时返回 null。
    /// </summary>
    private async Task<OnlineSongResult?> FindBestCandidateAsync(
        OnlineMusicSource source,
        IReadOnlyList<string> queries,
        string localTitle,
        string localArtist,
        string? localAlbum,
        long localDurationMs,
        double minScore,
        CancellationToken cancellationToken)
    {
        OnlineSongResult? best = null;
        var bestScore = 0.0;
        var attempts = 0;
        foreach (var query in queries)
        {
            if (attempts >= MaxQueriesPerSource)
            {
                break;
            }

            attempts++;
            IReadOnlyList<OnlineSongResult> candidates;
            try
            {
                candidates = await _online.SearchAsync(source, query, SearchLimit, cancellationToken).ConfigureAwait(false);
            }
            catch (Exception) when (!cancellationToken.IsCancellationRequested)
            {
                continue;
            }

            foreach (var candidate in candidates)
            {
                var score = LyricsMatchScorer.Score(
                    localTitle,
                    localArtist,
                    localAlbum,
                    localDurationMs,
                    candidate.Title,
                    candidate.Artist,
                    candidate.Album,
                    candidate.DurationMs);
                if (score.Rejected)
                {
                    continue;
                }

                if (score.Total > bestScore)
                {
                    bestScore = score.Total;
                    best = candidate;
                }
            }

            // 标题与歌手都可信时没必要继续换关键词，省掉多余请求。
            if (best is not null && bestScore >= minScore &&
                LyricsMatchScorer.TitleScore(localTitle, best.Title) >= LyricsMatchScorer.GoodTitleScore &&
                LyricsMatchScorer.ArtistScore(localArtist, best.Artist) >= LyricsMatchScorer.ReliableArtistScore)
            {
                break;
            }
        }

        return bestScore >= minScore ? best : null;
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
}
