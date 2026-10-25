namespace NeteaseMusicManager.Core.Lyrics;

/// <summary>逐字歌词中的一个词/字，时间单位为毫秒。</summary>
public sealed record LyricWord(int StartMs, int DurationMs, string Text)
{
    public int EndMs => StartMs + Math.Max(0, DurationMs);
}

/// <summary>一行歌词。逐字信息为空表示只有逐行时间轴。</summary>
public sealed record LyricLine(int StartMs, int DurationMs, string Text, IReadOnlyList<LyricWord> Words)
{
    public bool HasWordTiming => Words.Count > 0;
    public int EndMs => StartMs + Math.Max(0, DurationMs);
}

/// <summary>解析后的歌词文档。</summary>
public sealed class LyricDocument
{
    public IReadOnlyList<LyricLine> Lines { get; init; } = Array.Empty<LyricLine>();
    public string? Translation { get; init; }
    public string? Romanization { get; init; }
    public bool HasWordLevel => Lines.Any(l => l.HasWordTiming);
    public bool IsEmpty => Lines.Count == 0;
}
