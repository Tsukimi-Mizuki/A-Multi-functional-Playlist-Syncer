using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using NeteaseMusicManager.Core.Models;

namespace NeteaseMusicManager.Core.Lyrics;

/// <summary>
/// 普通 LRC / 增强 LRC / 网易云 YRC 解析与序列化。
/// </summary>
/// <remarks>
/// <para>支持：一行多个时间标签、[mm:ss]/[mm:ss.xx]/[mm:ss.xxx]、[offset:±ms] 整体偏移、
/// 增强 LRC 的 &lt;mm:ss.xx&gt; 词标签、YRC v0 的 [行起始,行时长](词起始,词时长,flag)词 格式，
/// 以及新版接口返回的 JSON 风格 YRC 行（{"t":0,"c":[{"tx":"..."}]}）。</para>
/// <para>翻译（tlyric）按最近时间戳逐行匹配到 <see cref="LyricLine.Translation"/>；无法匹配的翻译行被忽略。
/// 逐字时间若小于行起始时间则按“相对行起始”处理，否则按绝对毫秒处理。</para>
/// </remarks>
public sealed class LyricParser : ILyricParser
{
    /// <summary>翻译行与原文行时间差在 800ms 内即视为同一行。</summary>
    private const int TranslationToleranceMs = 800;

    private static readonly Regex TimestampPattern = new(
        @"^(?<minutes>\d{1,4}):(?<seconds>\d{1,2})(?:[.:](?<fraction>\d{1,3}))?$",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    private static readonly Regex OffsetTagPattern = new(
        @"\[offset:\s*(?<offset>[+-]?\d+)\s*\]",
        RegexOptions.Compiled | RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);

    private static readonly Regex YrcLineHeaderPattern = new(
        @"^\[(?<start>\d{1,10}),(?<duration>\d{1,10})\]",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    /// <inheritdoc />
    public LyricDocument Parse(RawLyrics rawLyrics)
    {
        ArgumentNullException.ThrowIfNull(rawLyrics);

        var yrcLines = ParseYrc(rawLyrics.Yrc);
        var primary = yrcLines.Count > 0 ? yrcLines : ParseLrc(rawLyrics.Lrc);
        var lines = BuildLines(primary);

        string? translation = NormalizeRaw(rawLyrics.TranslatedLrc);
        if (translation is not null && lines.Count > 0)
        {
            lines = MergeTranslation(lines, translation);
        }

        return new LyricDocument
        {
            Lines = lines,
            Translation = translation,
            Romanization = NormalizeRaw(rawLyrics.RomanizedLrc),
        };
    }

    /// <inheritdoc />
    public string ToPlainLrc(LyricDocument document) => ToPlainLrc(document, includeTranslation: true);

    /// <inheritdoc />
    public string ToPlainLrc(LyricDocument document, bool includeTranslation)
    {
        ArgumentNullException.ThrowIfNull(document);
        if (document.IsEmpty)
        {
            return string.Empty;
        }

        var builder = new StringBuilder();
        foreach (var line in document.Lines)
        {
            var timestamp = FormatTimestamp(line.StartMs);
            foreach (var part in SplitText(line.Text))
            {
                AppendLine(builder, timestamp + part.Trim());
            }

            if (includeTranslation && !string.IsNullOrWhiteSpace(line.Translation))
            {
                foreach (var part in SplitText(line.Translation))
                {
                    AppendLine(builder, timestamp + part.Trim());
                }
            }
        }

        return builder.ToString();
    }

    /// <inheritdoc />
    public string ToEnhancedLrc(LyricDocument document) => ToEnhancedLrc(document, includeTranslation: true);

    /// <inheritdoc />
    public string ToEnhancedLrc(LyricDocument document, bool includeTranslation)
    {
        ArgumentNullException.ThrowIfNull(document);
        if (document.IsEmpty)
        {
            return string.Empty;
        }

        if (!document.HasWordLevel)
        {
            return ToPlainLrc(document, includeTranslation);
        }

        var builder = new StringBuilder();
        foreach (var line in document.Lines)
        {
            var timestamp = FormatTimestamp(line.StartMs);

            if (line.HasWordTiming)
            {
                var lineBuilder = new StringBuilder(timestamp);
                foreach (var word in line.Words)
                {
                    if (word.Text.Length == 0)
                    {
                        continue;
                    }

                    var wordStart = Math.Max(line.StartMs, word.StartMs);
                    lineBuilder
                        .Append('<')
                        .Append(FormatTime(wordStart))
                        .Append('>')
                        .Append(SanitizeWordText(word.Text));
                }

                AppendLine(builder, lineBuilder.ToString());
            }
            else
            {
                foreach (var part in SplitText(line.Text))
                {
                    AppendLine(builder, timestamp + part.Trim());
                }
            }

            if (includeTranslation && !string.IsNullOrWhiteSpace(line.Translation))
            {
                foreach (var part in SplitText(line.Translation))
                {
                    AppendLine(builder, timestamp + part.Trim());
                }
            }
        }

        return builder.ToString();
    }

    private static IReadOnlyList<LyricLine> BuildLines(List<RawLyricLine> raw)
    {
        if (raw.Count == 0)
        {
            return Array.Empty<LyricLine>();
        }

        // OrderBy 为稳定排序，同一时间戳保持源顺序。
        var ordered = raw.OrderBy(line => line.StartMs).ToList();
        var result = new List<LyricLine>(ordered.Count);

        for (var index = 0; index < ordered.Count; index++)
        {
            var current = ordered[index];
            var start = Math.Max(0, current.StartMs);
            int? nextStart = index + 1 < ordered.Count ? Math.Max(0, ordered[index + 1].StartMs) : null;

            var lineDuration = current.DurationMs is > 0
                ? current.DurationMs.Value
                : nextStart is > 0 && nextStart.Value > start
                    ? nextStart.Value - start
                    : 0;

            var words = NormalizeWords(current.Words, start, lineDuration, nextStart);

            if (lineDuration <= 0 && words.Count > 0)
            {
                lineDuration = Math.Max(0, words[^1].EndMs - start);
            }

            var text = current.Text.Length > 0
                ? current.Text
                : string.Concat(words.Select(word => word.Text));

            result.Add(new LyricLine(start, lineDuration, text, words));
        }

        return result;
    }

    private static IReadOnlyList<LyricWord> NormalizeWords(
        IReadOnlyList<RawWord> rawWords,
        int lineStart,
        int lineDuration,
        int? nextLineStart)
    {
        if (rawWords.Count == 0)
        {
            return Array.Empty<LyricWord>();
        }

        var starts = new int[rawWords.Count];
        for (var index = 0; index < rawWords.Count; index++)
        {
            var word = rawWords[index];
            var start = word.StartMs;
            if (start < lineStart)
            {
                // 相对时间（少数实现按“相对行起始”写入词标签）。
                start = lineStart + Math.Max(0, start);
            }

            starts[index] = Math.Max(lineStart, start);
        }

        var words = new List<LyricWord>(rawWords.Count);
        for (var index = 0; index < rawWords.Count; index++)
        {
            var rawWord = rawWords[index];
            int duration;
            if (rawWord.DurationMs is int explicitDuration)
            {
                duration = Math.Max(0, explicitDuration);
            }
            else if (index + 1 < starts.Length)
            {
                duration = Math.Max(0, starts[index + 1] - starts[index]);
            }
            else if (lineDuration > 0)
            {
                duration = Math.Max(0, lineStart + lineDuration - starts[index]);
            }
            else if (nextLineStart is > 0 && nextLineStart.Value > starts[index])
            {
                duration = nextLineStart.Value - starts[index];
            }
            else
            {
                duration = 0;
            }

            words.Add(new LyricWord(starts[index], duration, rawWord.Text));
        }

        return words;
    }

    private static IReadOnlyList<LyricLine> MergeTranslation(IReadOnlyList<LyricLine> lines, string translatedLrc)
    {
        var translated = BuildLines(ParseLrc(translatedLrc));
        if (translated.Count == 0)
        {
            return lines;
        }

        var used = new bool[translated.Count];
        var merged = new List<LyricLine>(lines.Count);
        foreach (var line in lines)
        {
            string? translation = FindNearestTranslation(line.StartMs, translated, used);
            merged.Add(translation is null ? line : line with { Translation = translation });
        }

        return merged;
    }

    private static string? FindNearestTranslation(int startMs, IReadOnlyList<LyricLine> translated, bool[] used)
    {
        var bestIndex = -1;
        var bestDistance = int.MaxValue;
        for (var index = 0; index < translated.Count; index++)
        {
            if (used[index])
            {
                continue;
            }

            var distance = Math.Abs(translated[index].StartMs - startMs);
            if (distance < bestDistance)
            {
                bestDistance = distance;
                bestIndex = index;
            }
        }

        if (bestIndex < 0 || bestDistance > TranslationToleranceMs)
        {
            return null;
        }

        used[bestIndex] = true;
        var text = translated[bestIndex].Text.Trim();
        // QQ/网易云用 // 表示该行没有翻译或为纯音乐，不写入双语歌词。
        return text.Length == 0 || text.All(ch => ch == '/') ? null : text;
    }

    /// <summary>解析网易云 YRC（v0 明文行或 v1 JSON 风格行，二者可混合出现）。</summary>
    private static List<RawLyricLine> ParseYrc(string? yrc)
    {
        var result = new List<RawLyricLine>();
        if (string.IsNullOrWhiteSpace(yrc))
        {
            return result;
        }

        foreach (var physicalLine in SplitPhysicalLines(yrc))
        {
            if (string.IsNullOrWhiteSpace(physicalLine))
            {
                continue;
            }

            var line = physicalLine.Trim();
            if (line.StartsWith('{'))
            {
                if (TryParseJsonLine(line, out var jsonLine))
                {
                    result.Add(jsonLine);
                }

                continue;
            }

            if (TryParseYrcLine(line, out var yrcLine))
            {
                result.Add(yrcLine);
                continue;
            }

            // 容错：YRC 字段里混入了普通 LRC 行。
            result.AddRange(ParseLrcPhysicalLine(physicalLine, offsetMs: 0));
        }

        return result;
    }

    private static bool TryParseYrcLine(string line, out RawLyricLine parsed)
    {
        parsed = default!;
        var match = YrcLineHeaderPattern.Match(line);
        if (!match.Success)
        {
            return false;
        }

        if (!int.TryParse(match.Groups["start"].Value, NumberStyles.Integer, CultureInfo.InvariantCulture, out var startMs))
        {
            return false;
        }

        _ = int.TryParse(match.Groups["duration"].Value, NumberStyles.Integer, CultureInfo.InvariantCulture, out var durationMs);

        var rest = line[match.Length..];
        var words = new List<RawWord>();
        var index = 0;
        while (index < rest.Length)
        {
            if (rest[index] != '(')
            {
                index++;
                continue;
            }

            var close = rest.IndexOf(')', index + 1);
            if (close < 0)
            {
                break;
            }

            var content = rest[(index + 1)..close];
            var parts = content.Split(',');
            if (parts.Length >= 2
                && int.TryParse(parts[0].Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out var wordStart)
                && int.TryParse(parts[1].Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out var wordDuration))
            {
                var textStart = close + 1;
                var nextTag = rest.IndexOf('(', textStart);
                var wordText = nextTag < 0 ? rest[textStart..] : rest[textStart..nextTag];
                words.Add(new RawWord(wordStart, Math.Max(0, wordDuration), wordText));
                index = nextTag < 0 ? rest.Length : nextTag;
                continue;
            }

            index = close + 1;
        }

        var text = words.Count > 0
            ? string.Concat(words.Select(word => word.Text))
            : rest.Trim();

        parsed = new RawLyricLine(startMs, durationMs, text, words);
        return true;
    }

    /// <summary>解析新版接口的 JSON 风格歌词行：{"t":123,"c":[{"tx":"文本"}]}。</summary>
    private static bool TryParseJsonLine(string line, out RawLyricLine parsed)
    {
        parsed = default!;
        try
        {
            using var document = JsonDocument.Parse(line);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object || !root.TryGetProperty("t", out var timeElement))
            {
                return false;
            }

            var startMs = timeElement.ValueKind switch
            {
                JsonValueKind.Number when timeElement.TryGetInt32(out var t) => t,
                JsonValueKind.Number when timeElement.TryGetDouble(out var t) => (int)t,
                JsonValueKind.String when int.TryParse(timeElement.GetString(), out var t) => t,
                _ => 0,
            };

            var textBuilder = new StringBuilder();
            if (root.TryGetProperty("c", out var content) && content.ValueKind == JsonValueKind.Array)
            {
                foreach (var item in content.EnumerateArray())
                {
                    if (item.ValueKind == JsonValueKind.Object
                        && item.TryGetProperty("tx", out var tx)
                        && tx.ValueKind == JsonValueKind.String)
                    {
                        textBuilder.Append(tx.GetString());
                    }
                }
            }

            parsed = new RawLyricLine(startMs, null, textBuilder.ToString(), Array.Empty<RawWord>());
            return true;
        }
        catch (JsonException)
        {
            return false;
        }
    }

    /// <summary>解析普通 LRC 文本（含 [offset] 与增强 LRC 词标签）。</summary>
    private static List<RawLyricLine> ParseLrc(string? lrc)
    {
        var result = new List<RawLyricLine>();
        if (string.IsNullOrWhiteSpace(lrc))
        {
            return result;
        }

        var offsetMs = ParseOffset(lrc);
        foreach (var physicalLine in SplitPhysicalLines(lrc))
        {
            result.AddRange(ParseLrcPhysicalLine(physicalLine, offsetMs));
        }

        return result;
    }

    private static List<RawLyricLine> ParseLrcPhysicalLine(string physicalLine, int offsetMs)
    {
        var result = new List<RawLyricLine>();
        if (string.IsNullOrWhiteSpace(physicalLine))
        {
            return result;
        }

        var line = physicalLine.TrimStart();
        if (line.StartsWith('{'))
        {
            if (TryParseJsonLine(line.Trim(), out var jsonLine))
            {
                result.Add(jsonLine);
            }

            return result;
        }

        var position = 0;
        var timestamps = new List<int>();
        while (TryReadLrcTimestamp(physicalLine, ref position, offsetMs, out var milliseconds))
        {
            timestamps.Add(milliseconds);
        }

        if (timestamps.Count == 0)
        {
            // 元数据标签（[ti:]、[ar:]、[offset:] 等）直接忽略。
            return result;
        }

        var rest = physicalLine[position..];
        var words = ParseEnhancedWords(rest, timestamps[0]);
        var text = words.Count > 0
            ? string.Concat(words.Select(word => word.Text))
            : rest.Trim();

        foreach (var timestamp in timestamps)
        {
            result.Add(new RawLyricLine(timestamp, null, text, words));
        }

        return result;
    }

    private static bool TryReadLrcTimestamp(string line, ref int position, int offsetMs, out int milliseconds)
    {
        milliseconds = 0;
        var index = position;
        while (index < line.Length && char.IsWhiteSpace(line[index]))
        {
            index++;
        }

        if (index >= line.Length || line[index] != '[')
        {
            return false;
        }

        var close = line.IndexOf(']', index + 1);
        if (close < 0)
        {
            return false;
        }

        var content = line[(index + 1)..close];
        var match = TimestampPattern.Match(content);
        if (!match.Success)
        {
            return false;
        }

        var minutes = int.Parse(match.Groups["minutes"].Value, CultureInfo.InvariantCulture);
        var seconds = int.Parse(match.Groups["seconds"].Value, CultureInfo.InvariantCulture);
        var fraction = ParseFractionMilliseconds(match.Groups["fraction"].Value);
        var total = (long)minutes * 60_000 + (long)seconds * 1000 + fraction;
        total = Math.Clamp(total + offsetMs, 0, int.MaxValue);

        position = close + 1;
        milliseconds = (int)total;
        return true;
    }

    private static List<RawWord> ParseEnhancedWords(string rest, int lineStart)
    {
        var words = new List<RawWord>();
        var index = 0;
        while (true)
        {
            var open = rest.IndexOf('<', index);
            if (open < 0)
            {
                break;
            }

            var close = rest.IndexOf('>', open + 1);
            if (close < 0)
            {
                break;
            }

            var tag = rest[(open + 1)..close];
            if (!TryParseWordTimestamp(tag, out var absoluteMs))
            {
                index = close + 1;
                continue;
            }

            var textStart = close + 1;
            var nextTag = rest.IndexOf('<', textStart);
            var text = nextTag < 0 ? rest[textStart..] : rest[textStart..nextTag];
            words.Add(new RawWord(absoluteMs, null, text));

            if (nextTag < 0)
            {
                break;
            }

            index = nextTag;
        }

        return words;
    }

    private static bool TryParseWordTimestamp(string content, out int milliseconds)
    {
        milliseconds = 0;
        var match = TimestampPattern.Match(content.Trim());
        if (!match.Success)
        {
            return false;
        }

        var minutes = int.Parse(match.Groups["minutes"].Value, CultureInfo.InvariantCulture);
        var seconds = int.Parse(match.Groups["seconds"].Value, CultureInfo.InvariantCulture);
        var fraction = ParseFractionMilliseconds(match.Groups["fraction"].Value);
        var total = (long)minutes * 60_000 + (long)seconds * 1000 + fraction;
        if (total is < 0 or > int.MaxValue)
        {
            return false;
        }

        milliseconds = (int)total;
        return true;
    }

    private static int ParseFractionMilliseconds(string fraction)
    {
        if (string.IsNullOrEmpty(fraction))
        {
            return 0;
        }

        if (!int.TryParse(fraction, NumberStyles.Integer, CultureInfo.InvariantCulture, out var value))
        {
            return 0;
        }

        return fraction.Length switch
        {
            1 => value * 100,
            2 => value * 10,
            _ => value,
        };
    }

    private static int ParseOffset(string lrc)
    {
        var match = OffsetTagPattern.Match(lrc);
        if (!match.Success)
        {
            return 0;
        }

        return int.TryParse(match.Groups["offset"].Value, NumberStyles.Integer, CultureInfo.InvariantCulture, out var offset)
            ? offset
            : 0;
    }

    private static IEnumerable<string> SplitPhysicalLines(string text) =>
        text.Replace("\r\n", "\n", StringComparison.Ordinal).Replace('\r', '\n').Split('\n');

    private static string[] SplitText(string? text)
    {
        if (string.IsNullOrEmpty(text))
        {
            return new[] { string.Empty };
        }

        return text
            .Replace("\r\n", "\n", StringComparison.Ordinal)
            .Replace('\r', '\n')
            .Split('\n');
    }

    private static string SanitizeWordText(string text) =>
        text.Replace("\r\n", " ", StringComparison.Ordinal).Replace('\r', ' ').Replace('\n', ' ');

    private static void AppendLine(StringBuilder builder, string line)
    {
        if (builder.Length > 0)
        {
            builder.Append('\n');
        }

        builder.Append(line);
    }

    private static string? NormalizeRaw(string? value) => string.IsNullOrWhiteSpace(value) ? null : value;

    private static string FormatTimestamp(int milliseconds) => "[" + FormatTime(milliseconds) + "]";

    private static string FormatTime(int milliseconds)
    {
        var total = Math.Max(0, milliseconds);
        var totalSeconds = total / 1000;
        var minutes = totalSeconds / 60;
        var seconds = totalSeconds % 60;
        var centiseconds = total % 1000 / 10;
        return string.Create(
            CultureInfo.InvariantCulture,
            $"{minutes:00}:{seconds:00}.{centiseconds:00}");
    }

    /// <summary>解析过程中的中间行（词时间尚未归一化为绝对/相对）。</summary>
    private sealed class RawLyricLine
    {
        public RawLyricLine(int startMs, int? durationMs, string text, IReadOnlyList<RawWord> words)
        {
            StartMs = startMs;
            DurationMs = durationMs;
            Text = text ?? string.Empty;
            Words = words ?? Array.Empty<RawWord>();
        }

        public int StartMs { get; }

        public int? DurationMs { get; }

        public string Text { get; }

        public IReadOnlyList<RawWord> Words { get; }
    }

    private sealed record RawWord(int StartMs, int? DurationMs, string Text);
}
