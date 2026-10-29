using System.Globalization;
using System.IO.Compression;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace LunaticPrincess.Core.Lyrics;

/// <summary>
/// 酷狗 KRC 歌词的解密与解析。
/// KRC 文件 = 明文魔数 <c>krc1</c> + XOR 混淆的 zlib 流，解压后是 UTF-8 文本；
/// 文本行形如 <c>[行起始ms,行时长ms]&lt;词偏移,词时长,0&gt;词&lt;…&gt;词</c>，
/// 逐字词的标签在词文本之前，词时间是相对行起始的偏移。
/// </summary>
public static class KrcLyricsParser
{
    private static readonly byte[] XorKey =
    [
        0x40, 0x47, 0x61, 0x77, 0x5E, 0x32, 0x74, 0x47,
        0x51, 0x36, 0x31, 0x2D, 0xCE, 0xD2, 0x6E, 0x69,
    ];

    private static readonly byte[] Magic = "krc1"u8.ToArray();

    private static readonly Regex LineRegex = new(
        @"^\[(\d+),(\d+)\](.*)$",
        RegexOptions.Compiled);

    private static readonly Regex WordRegex = new(
        @"<(\d+),(\d+),-?\d+>",
        RegexOptions.Compiled);

    private static readonly Regex LanguageRegex = new(
        @"^\[language:([A-Za-z0-9+/=]*)\]$",
        RegexOptions.Compiled);

    /// <summary>
    /// 把酷狗下载接口返回的 base64 内容解码为 KRC 明文；格式不符或解压失败返回 null。
    /// </summary>
    public static string? DecryptBase64(string? base64)
    {
        if (string.IsNullOrWhiteSpace(base64))
        {
            return null;
        }

        byte[] raw;
        try
        {
            raw = Convert.FromBase64String(base64.Trim());
        }
        catch (FormatException)
        {
            return null;
        }

        return Decrypt(raw);
    }

    /// <summary>把加密后的 KRC 字节流解成明文歌词文本。</summary>
    public static string? Decrypt(byte[] encrypted)
    {
        ArgumentNullException.ThrowIfNull(encrypted);
        if (encrypted.Length == 0)
        {
            return null;
        }

        // 魔数是明文，不参与 XOR 与解压；没有魔数的老格式从头开始。
        var start = encrypted.Length >= Magic.Length &&
                    encrypted.AsSpan(0, Magic.Length).SequenceEqual(Magic)
            ? Magic.Length
            : 0;

        var data = (byte[])encrypted.Clone();
        for (var i = start; i < data.Length; i++)
        {
            data[i] ^= XorKey[(i - start) % XorKey.Length];
        }

        if (start >= data.Length)
        {
            return null;
        }

        try
        {
            using var input = new MemoryStream(data, start, data.Length - start, writable: false);
            using var zlib = new ZLibStream(input, CompressionMode.Decompress);
            using var reader = new StreamReader(zlib, Encoding.UTF8, detectEncodingFromByteOrderMarks: true);
            return reader.ReadToEnd();
        }
        catch (Exception ex) when (ex is InvalidDataException or IOException)
        {
            // 少数接口直接返回明文 UTF-8（没有 zlib 头），按明文兜底。
            var text = Encoding.UTF8.GetString(data, start, data.Length - start);
            return text.Contains('[', StringComparison.Ordinal) ? text : null;
        }
    }

    /// <summary>解析 KRC 明文为歌词文档（含逐字时间轴与逐行译文）。</summary>
    public static LyricDocument Parse(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return new LyricDocument();
        }

        var translations = ReadTranslations(text);
        var lines = new List<LyricLine>();

        foreach (var rawLine in text.Split('\n'))
        {
            var line = rawLine.TrimEnd('\r').Trim();
            if (line.Length == 0)
            {
                continue;
            }

            var match = LineRegex.Match(line);
            if (!match.Success)
            {
                continue;
            }

            var lineStart = ParseInt(match.Groups[1].Value);
            var lineDuration = ParseInt(match.Groups[2].Value);
            var content = match.Groups[3].Value;

            var words = new List<LyricWord>();
            var wordMatches = WordRegex.Matches(content);
            for (var i = 0; i < wordMatches.Count; i++)
            {
                var wordMatch = wordMatches[i];
                var textStart = wordMatch.Index + wordMatch.Length;
                var textEnd = i + 1 < wordMatches.Count ? wordMatches[i + 1].Index : content.Length;
                if (textEnd <= textStart)
                {
                    continue;
                }

                var wordText = content[textStart..textEnd];
                words.Add(new LyricWord(
                    lineStart + ParseInt(wordMatch.Groups[1].Value),
                    ParseInt(wordMatch.Groups[2].Value),
                    wordText));
            }

            var lineText = words.Count > 0
                ? string.Concat(words.Select(word => word.Text)).Trim()
                : content.Trim();
            if (lineText.Length == 0 && words.Count == 0)
            {
                continue;
            }

            string? translation = null;
            if (translations is not null && lines.Count < translations.Count)
            {
                translation = translations[lines.Count];
            }

            lines.Add(new LyricLine(lineStart, lineDuration, lineText, words, translation));
        }

        return new LyricDocument { Lines = lines };
    }

    /// <summary>读取 <c>[language:…]</c> 里的逐行译文（type=1）；没有时返回 null。</summary>
    private static IReadOnlyList<string?>? ReadTranslations(string text)
    {
        foreach (var rawLine in text.Split('\n'))
        {
            var line = rawLine.TrimEnd('\r').Trim();
            var match = LanguageRegex.Match(line);
            if (!match.Success)
            {
                continue;
            }

            try
            {
                var json = Encoding.UTF8.GetString(Convert.FromBase64String(match.Groups[1].Value));
                using var document = JsonDocument.Parse(json);
                if (!document.RootElement.TryGetProperty("content", out var content) ||
                    content.ValueKind != JsonValueKind.Array)
                {
                    return null;
                }

                foreach (var track in content.EnumerateArray())
                {
                    if (!track.TryGetProperty("type", out var type) ||
                        type.ValueKind != JsonValueKind.Number ||
                        type.GetInt32() != 1)
                    {
                        continue;
                    }

                    if (!track.TryGetProperty("lyricContent", out var lyricContent) ||
                        lyricContent.ValueKind != JsonValueKind.Array)
                    {
                        return null;
                    }

                    var result = new List<string?>();
                    foreach (var entry in lyricContent.EnumerateArray())
                    {
                        var value = FlattenText(entry);
                        result.Add(string.IsNullOrWhiteSpace(value) ? null : value.Trim());
                    }

                    return result;
                }
            }
            catch (Exception ex) when (ex is FormatException or JsonException or InvalidOperationException)
            {
                return null;
            }
        }

        return null;
    }

    private static string FlattenText(JsonElement element)
    {
        switch (element.ValueKind)
        {
            case JsonValueKind.String:
                return element.GetString() ?? string.Empty;
            case JsonValueKind.Array:
                var builder = new StringBuilder();
                foreach (var item in element.EnumerateArray())
                {
                    builder.Append(FlattenText(item));
                }

                return builder.ToString();
            default:
                return string.Empty;
        }
    }

    private static int ParseInt(string value) =>
        int.TryParse(value, NumberStyles.Integer, CultureInfo.InvariantCulture, out var parsed) ? parsed : 0;
}
