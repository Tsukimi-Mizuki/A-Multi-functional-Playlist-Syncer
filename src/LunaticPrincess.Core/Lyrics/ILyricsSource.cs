namespace LunaticPrincess.Core.Lyrics;

/// <summary>
/// 备用词源的候选结果：已按项目统一格式生成可直接写入标签的歌词文本。
/// 网易云 / QQ 之外的词源（酷狗、LRCLIB 等）都用这个结构把结果交回批量服务。
/// </summary>
public sealed record LyricsSourceCandidate(
    string SourceLabel,
    string Title,
    string Artist,
    string Album,
    long DurationMs,
    string? LineLyrics = null,
    string? WordLyrics = null,
    string? BilingualLineLyrics = null,
    string? BilingualWordLyrics = null)
{
    /// <summary>
    /// 按偏好选择逐字（增强 LRC）或逐行歌词；逐字不可用时自动回退逐行。
    /// 语义与 <see cref="Online.OnlineSongDetails.GetLyrics"/> 保持一致。
    /// </summary>
    public string? GetLyrics(bool preferWord, bool bilingual)
    {
        if (bilingual)
        {
            var translated = preferWord
                ? BilingualWordLyrics ?? BilingualLineLyrics
                : BilingualLineLyrics;
            if (!string.IsNullOrWhiteSpace(translated))
            {
                return translated;
            }
        }

        return preferWord ? WordLyrics ?? LineLyrics : LineLyrics;
    }

    public bool HasWordLyrics => !string.IsNullOrWhiteSpace(WordLyrics);

    public bool HasTranslation =>
        !string.IsNullOrWhiteSpace(BilingualLineLyrics) ||
        !string.IsNullOrWhiteSpace(BilingualWordLyrics);
}

/// <summary>
/// 备用词源：网易云 / QQ 音乐之外的补充来源（酷狗、LRCLIB 等）。
/// 搜索失败、无匹配或候选低于匹配度阈值时返回 null，不抛异常，避免拖垮整条业务链。
/// </summary>
public interface ILyricsSource
{
    /// <summary>写入日志与结果里的来源名，例如“酷狗音乐”“LRCLIB”。</summary>
    string Label { get; }

    /// <summary>
    /// 按本地歌曲信息检索一份歌词。返回 null 表示该源没有可用结果。
    /// </summary>
    Task<LyricsSourceCandidate?> FetchAsync(
        IReadOnlyList<string> queries,
        string title,
        string artist,
        string? album,
        long durationMs,
        double minScore,
        CancellationToken cancellationToken = default);
}
