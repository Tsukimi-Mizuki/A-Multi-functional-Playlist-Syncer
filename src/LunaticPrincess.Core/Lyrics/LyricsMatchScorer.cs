using System.Globalization;
using System.Text;

namespace LunaticPrincess.Core.Lyrics;

/// <summary>匹配度打分：标题、歌手、专辑、时长四路分数与加权总分。</summary>
/// <param name="Title">标题相似度（0-1）。</param>
/// <param name="Artist">歌手相似度（0-1）。</param>
/// <param name="Album">专辑相似度（0-1）。</param>
/// <param name="Duration">时长相似度（0-1）。</param>
/// <param name="Total">加权总分（0-1），用于和匹配度阈值比较。</param>
/// <param name="Rejected">是否直接排除（例如本地不是伴奏而候选是伴奏）。</param>
/// <param name="RejectReason">排除原因，便于日志与界面提示。</param>
public sealed record LyricsMatchScore(
    double Title,
    double Artist,
    double Album,
    double Duration,
    double Total,
    bool Rejected = false,
    string? RejectReason = null)
{
    /// <summary>标题与歌手都达到可信阈值，可以认为匹配非常确定。</summary>
    public bool IsConfident =>
        !Rejected &&
        Title >= LyricsMatchScorer.GoodTitleScore &&
        Artist >= LyricsMatchScorer.ReliableArtistScore;
}

/// <summary>
/// 在线搜索结果与本地歌曲的匹配度打分，思路参考 音樂標籤（Android）：
/// 先做全角/大小写/括号后缀归一化，标题、歌手、专辑三路打分并支持多值切段，
/// 用阈值做重排（歌手可信、标题太差、专辑不符、伴奏版本降权），
/// 最后按“标题 &gt; 歌手 &gt; 时长”的权重合成总分，交给调用方用匹配度阈值筛选。
/// </summary>
public static class LyricsMatchScorer
{
    /// <summary>默认匹配度阈值（等价于 音樂標籤 的 0.6 分界）。</summary>
    public const double DefaultMinScore = 0.60;

    /// <summary>阈值滑块允许的最小值。</summary>
    public const double MinThreshold = 0.30;

    /// <summary>阈值滑块允许的最大值。</summary>
    public const double MaxThreshold = 1.00;

    /// <summary>标题达到该分数算“好标题”（音樂標籤 的 0.8）。</summary>
    public const double GoodTitleScore = 0.80;

    /// <summary>标题低于该分数算“差标题”（音樂標籤 的 0.5）。</summary>
    public const double PoorTitleScore = 0.50;

    /// <summary>歌手达到该分数算可信（音樂標籤 的 0.8）。</summary>
    public const double ReliableArtistScore = 0.80;

    /// <summary>专辑达到该分数算相符（音樂標籤 的 0.8）。</summary>
    public const double ReliableAlbumScore = 0.80;

    /// <summary>标题里的并列分隔符（MusicTag 的 [\/&amp;,] 思路，另外补上全角与中文写法）。</summary>
    private static readonly char[] TitleSeparators = ['/', '&', ',', '，', '、', ';', '；', '·', '|'];

    /// <summary>歌手里常见的多值分隔符。</summary>
    private static readonly char[] ArtistSeparators = ['/', '&', ',', '，', '、', ';', '；', '·', '・', '×', '|', '+'];

    /// <summary>歌手字段里的连接词，与分隔符等价。</summary>
    private static readonly string[] ArtistKeywords = ["feat.", "ft.", "featuring", "cv:", "vs.", "with"];

    /// <summary>伴奏 / 纯音乐标记：本地不是伴奏而候选是伴奏时直接排除。</summary>
    private static readonly string[] InstrumentalKeywords = ["instrumental", "off vocal", "offvocal", "karaoke", "伴奏", "纯音乐", "伴唱"];

    /// <summary>版本标记：Live / Remix / 翻唱 等版本不一致时降权，避免现场版顶替录音室版。</summary>
    private static readonly string[] VersionKeywords =
    [
        "live", "remix", "acoustic", "demo", "karaoke", "cover", "edit", "unplugged", "instrumental",
        "tv size", "tv ver", "现场", "演唱会", "翻唱", "伴奏", "纯音乐", "伴唱", "片段", "剧场版",
    ];

    /// <summary>把文本归一化：全角转半角、转小写、标点转空格、压缩空白。</summary>
    public static string NormalizeText(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return string.Empty;
        }

        var form = value.Normalize(NormalizationForm.FormKC).ToLowerInvariant();
        var builder = new StringBuilder(form.Length);
        var pendingSpace = false;
        foreach (var ch in form)
        {
            if (char.IsLetterOrDigit(ch))
            {
                if (pendingSpace && builder.Length > 0)
                {
                    builder.Append(' ');
                }

                pendingSpace = false;
                builder.Append(ch);
            }
            else
            {
                pendingSpace = true;
            }
        }

        return builder.ToString();
    }

    /// <summary>去掉所有空白与标点后的紧凑文本，用于“完全相同”这类判断。</summary>
    public static string CompactText(string? value) => NormalizeText(value).Replace(" ", string.Empty, StringComparison.Ordinal);

    /// <summary>去掉括号里的修饰（(Live)、(Remastered)、【电视剧片尾曲】等），保留标题主体。</summary>
    public static string StripBrackets(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return string.Empty;
        }

        var builder = new StringBuilder(value.Length);
        var depth = 0;
        foreach (var ch in value)
        {
            if (ch is '(' or '[' or '{' or '（' or '【' or '〈' or '《' or '「' or '『' or '〔')
            {
                depth++;
                continue;
            }

            if (ch is ')' or ']' or '}' or '）' or '】' or '〉' or '》' or '」' or '』' or '〕')
            {
                if (depth > 0)
                {
                    depth--;
                }

                continue;
            }

            if (depth == 0)
            {
                builder.Append(ch);
            }
        }

        return builder.ToString();
    }

    /// <summary>标题主体（去掉括号修饰后的归一化文本）。</summary>
    public static string CoreTitle(string? value) => NormalizeText(StripBrackets(value));

    /// <summary>判断标题是否是伴奏 / 纯音乐版本。</summary>
    public static bool IsInstrumental(string? title)
    {
        var text = NormalizeText(title);
        if (text.Length == 0)
        {
            return false;
        }

        return InstrumentalKeywords.Any(keyword => text.Contains(keyword, StringComparison.Ordinal));
    }

    /// <summary>
    /// 生成本地歌曲的多组搜索关键词（音樂標籤 的多变体思路）：
    /// “标题 歌手” → “歌手 标题” → “标题主体” → “标题主体 歌手” → 去掉 feat. 尾巴。
    /// 调用方按顺序尝试，命中可信匹配就不再继续换关键词。
    /// </summary>
    public static IReadOnlyList<string> BuildQueries(string? title, string? artist)
    {
        var queries = new List<string>();
        var rawTitle = Collapse(title);
        var rawArtist = Collapse(artist);
        if (rawTitle.Length == 0)
        {
            return queries;
        }

        var core = Collapse(StripBrackets(rawTitle));
        var noFeat = Collapse(RemoveFeaturing(rawTitle));

        if (rawArtist.Length > 0)
        {
            Add($"{rawTitle} {rawArtist}");
        }

        Add(rawTitle);

        if (rawArtist.Length > 0)
        {
            Add($"{rawArtist} {rawTitle}");
        }

        if (core.Length > 0 && !string.Equals(core, rawTitle, StringComparison.Ordinal))
        {
            Add(rawArtist.Length > 0 ? $"{core} {rawArtist}" : core);
        }

        if (noFeat.Length > 0 && !string.Equals(noFeat, rawTitle, StringComparison.Ordinal))
        {
            Add(rawArtist.Length > 0 ? $"{noFeat} {rawArtist}" : noFeat);
        }

        // “A / B” 这类并列标题：整串往往搜不到，退化成第一段再试一次。
        var segments = SplitMultiValue(rawTitle, TitleSeparators, Array.Empty<string>());
        if (segments.Count > 1 && segments[0].Trim().Length > 0)
        {
            Add(rawArtist.Length > 0 ? $"{segments[0].Trim()} {rawArtist}" : segments[0].Trim());
        }

        return queries;

        void Add(string? query)
        {
            var value = Collapse(query);
            if (value.Length > 0 && !queries.Contains(value, StringComparer.OrdinalIgnoreCase))
            {
                queries.Add(value);
            }
        }
    }

    /// <summary>计算本地歌曲与一条在线搜索结果的匹配度。</summary>
    public static LyricsMatchScore Score(
        string? localTitle,
        string? localArtist,
        string? localAlbum,
        long localDurationMs,
        string? resultTitle,
        string? resultArtist,
        string? resultAlbum,
        long resultDurationMs)
    {
        var title = TitleScore(localTitle, resultTitle);
        var artist = ArtistScore(localArtist, resultArtist);
        var album = AlbumScore(localAlbum, resultAlbum);
        var duration = DurationScore(localDurationMs, resultDurationMs);

        var hasArtist = !string.IsNullOrWhiteSpace(localArtist) && !string.IsNullOrWhiteSpace(resultArtist);
        var total = hasArtist
            ? title * 0.55 + artist * 0.25 + duration * 0.20
            : title * 0.72 + duration * 0.20 + album * 0.08;

        // 专辑只是轻量信号：同专辑多版本、合辑很常见，不能因为专辑不符就直接判否。
        var hasLocalAlbum = !string.IsNullOrWhiteSpace(localAlbum);
        var hasResultAlbum = !string.IsNullOrWhiteSpace(resultAlbum);
        if (hasLocalAlbum && hasResultAlbum && album < ReliableAlbumScore)
        {
            total *= 0.97;
        }

        // 版本标记不一致时降权（本地没有 Live，候选是 Live；或反过来）。
        var localVersions = VersionTags(localTitle);
        var resultVersions = VersionTags(resultTitle);
        var extra = resultVersions.Count(tag => !localVersions.Contains(tag));
        var missing = localVersions.Count(tag => !resultVersions.Contains(tag));
        for (var i = 0; i < extra; i++)
        {
            total *= 0.86;
        }

        for (var i = 0; i < missing; i++)
        {
            total *= 0.94;
        }

        // 歌手完全不同：多半是别人的同名歌曲（翻唱、同名不同曲），显著降权，避免把标签写错。
        if (hasArtist && artist < 0.30)
        {
            total *= 0.75;
        }

        total = Math.Clamp(total, 0, 1);

        if (IsInstrumental(resultTitle) && !IsInstrumental(localTitle))
        {
            return new LyricsMatchScore(title, artist, album, duration, total, true, "候选是伴奏/纯音乐版本");
        }

        return new LyricsMatchScore(title, artist, album, duration, total);
    }

    /// <summary>标题相似度：完整标题、标题主体、并列分段各算一次，取最佳。</summary>
    public static double TitleScore(string? localTitle, string? resultTitle)
    {
        var a = NormalizeText(localTitle);
        var b = NormalizeText(resultTitle);
        if (a.Length == 0 || b.Length == 0)
        {
            return 0;
        }

        var best = Compare(a, b);

        var aCore = CoreTitle(localTitle);
        var bCore = CoreTitle(resultTitle);
        if (aCore.Length > 0 && bCore.Length > 0)
        {
            best = Math.Max(best, Compare(aCore, bCore));
        }

        if (aCore.Length >= 2 && b.Length > 0)
        {
            best = Math.Max(best, Compare(aCore, b) * 0.98);
        }

        if (bCore.Length >= 2 && a.Length > 0)
        {
            best = Math.Max(best, Compare(a, bCore) * 0.98);
        }

        // “A / B” 这类并列标题：切段后逐段比较，命中某一段时轻微降权。
        foreach (var segment in SplitMultiValue(localTitle, TitleSeparators, Array.Empty<string>()))
        {
            var normalized = NormalizeText(segment);
            if (normalized.Length < 2)
            {
                continue;
            }

            best = Math.Max(best, Compare(normalized, b) * 0.97);
            if (bCore.Length >= 2)
            {
                best = Math.Max(best, Compare(normalized, bCore) * 0.96);
            }
        }

        return Math.Clamp(best, 0, 1);
    }

    /// <summary>歌手相似度：整体相似度 + 多值切段两两比较（“A/B”“A feat. B”“A CV:B”）。</summary>
    public static double ArtistScore(string? localArtist, string? resultArtist)
    {
        var a = NormalizeText(localArtist);
        var b = NormalizeText(resultArtist);
        if (a.Length == 0 || b.Length == 0)
        {
            return 0.5;
        }

        var best = Compare(a, b);
        foreach (var left in SplitMultiValue(localArtist, ArtistSeparators, ArtistKeywords))
        {
            var l = NormalizeText(left);
            if (l.Length == 0)
            {
                continue;
            }

            foreach (var right in SplitMultiValue(resultArtist, ArtistSeparators, ArtistKeywords))
            {
                var r = NormalizeText(right);
                if (r.Length == 0)
                {
                    continue;
                }

                var score = l == r
                    ? 0.98
                    : Compare(l, r) * 0.97;
                if (score > best)
                {
                    best = score;
                }
            }
        }

        return Math.Clamp(best, 0, 1);
    }

    /// <summary>专辑相似度（为空时返回中性值 0.5）。</summary>
    public static double AlbumScore(string? localAlbum, string? resultAlbum)
    {
        var a = NormalizeText(localAlbum);
        var b = NormalizeText(resultAlbum);
        if (a.Length == 0 || b.Length == 0)
        {
            return 0.5;
        }

        return Math.Clamp(Compare(a, b), 0, 1);
    }

    /// <summary>时长相似度：完全一致最好，相差越大越低（超过 40 秒视为不相关）。</summary>
    public static double DurationScore(long localMs, long resultMs)
    {
        if (localMs <= 0 || resultMs <= 0)
        {
            return 0.5;
        }

        var diff = Math.Abs(localMs - resultMs);
        return diff switch
        {
            <= 2000 => 1,
            <= 5000 => 0.90,
            <= 10000 => 0.75,
            <= 20000 => 0.45,
            <= 40000 => 0.15,
            _ => 0,
        };
    }

    /// <summary>把 “A/B”“A &amp; B”“A feat. B” 之类的多值字段切成若干片段；没有分隔符时返回空列表。</summary>
    public static IReadOnlyList<string> SplitMultiValue(string? value, char[] separators, string[] keywords)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return Array.Empty<string>();
        }

        var hasSeparator =
            value.IndexOfAny(separators) >= 0 ||
            keywords.Any(keyword => value.Contains(keyword, StringComparison.OrdinalIgnoreCase));
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

    private const char MultiValueMarker = '\u0001';

    private const string MultiValueMarkerText = "\u0001";

    private static List<string> VersionTags(string? title)
    {
        var text = NormalizeText(title);
        var tags = new List<string>();
        if (text.Length == 0)
        {
            return tags;
        }

        foreach (var keyword in VersionKeywords)
        {
            // 英文标记按整词匹配，避免 "cover" 命中 "discover"、"live" 命中 "delivery"；
            // 中文标记没有词边界，直接用包含判断。
            var isAscii = keyword.All(ch => ch < 0x80);
            if (isAscii
                    ? System.Text.RegularExpressions.Regex.IsMatch(text, $@"\b{System.Text.RegularExpressions.Regex.Escape(keyword)}\b")
                    : text.Contains(keyword, StringComparison.Ordinal))
            {
                tags.Add(keyword);
            }
        }

        return tags;
    }

    private static string RemoveFeaturing(string value)
    {
        foreach (var keyword in ArtistKeywords)
        {
            var index = value.IndexOf(keyword, StringComparison.OrdinalIgnoreCase);
            if (index > 0)
            {
                return value[..index];
            }
        }

        return value;
    }

    private static string Collapse(string? value) =>
        string.IsNullOrWhiteSpace(value)
            ? string.Empty
            : System.Text.RegularExpressions.Regex.Replace(value.Trim(), @"\s+", " ");

    /// <summary>两段归一化文本的相似度：编辑距离 + 包含关系 + 词序颠倒救援。</summary>
    private static double Compare(string a, string b)
    {
        if (a == b)
        {
            return 1;
        }

        if (a.Length == 0 || b.Length == 0)
        {
            return 0;
        }

        var best = Similarity(a, b);

        if (a.Length >= 3 && b.Length >= 3 && (a.Contains(b, StringComparison.Ordinal) || b.Contains(a, StringComparison.Ordinal)))
        {
            var ratio = (double)Math.Min(a.Length, b.Length) / Math.Max(a.Length, b.Length);
            best = Math.Max(best, 0.88 + 0.09 * ratio);
        }

        var aTokens = a.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        var bTokens = b.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        if (aTokens.Length > 1 &&
            aTokens.Length == bTokens.Length &&
            string.Equals(string.Join(' ', aTokens.Reverse()), string.Join(' ', bTokens), StringComparison.Ordinal))
        {
            best = Math.Max(best, 0.95);
        }

        return Math.Clamp(best, 0, 1);
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
