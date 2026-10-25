namespace NeteaseMusicManager.Core.Online;

public enum OnlineMusicSource
{
    Netease,
    QQMusic,
}

/// <summary>联网搜索返回的歌曲摘要（同时包含可直接写入标签的常用字段）。</summary>
public sealed record OnlineSongResult(
    OnlineMusicSource Source,
    string SourceId,
    string Title,
    string Artist,
    string Album,
    long DurationMs,
    uint? Year = null,
    uint? TrackNumber = null,
    uint? DiscNumber = null,
    string? Genre = null,
    string? Composer = null,
    string? CoverUrl = null,
    /// <summary>QQ 音乐的数值 song id；逐行翻译接口需要。</summary>
    string? NumericSourceId = null)
{
    public string SourceLabel => Source == OnlineMusicSource.Netease ? "网易云音乐" : "QQ 音乐";

    public string DisplayTitle => string.IsNullOrWhiteSpace(Title) ? "未知歌曲" : Title;

    public string Subtitle
    {
        get
        {
            var artist = string.IsNullOrWhiteSpace(Artist) ? "未知歌手" : Artist;
            var album = string.IsNullOrWhiteSpace(Album) ? "未知专辑" : Album;
            return $"{artist} · {album}";
        }
    }

    public string DurationText
    {
        get
        {
            if (DurationMs <= 0)
            {
                return "--:--";
            }

            var duration = TimeSpan.FromMilliseconds(DurationMs);
            return $"{(int)duration.TotalMinutes}:{duration.Seconds:00}";
        }
    }
}

public sealed record OnlineSongDetails(
    OnlineSongResult Song,
    string? LineLyrics,
    string? WordLyrics,
    byte[]? CoverData = null,
    string? BilingualLineLyrics = null,
    string? BilingualWordLyrics = null)
{
    /// <summary>
    /// 按偏好选择逐字（增强 LRC）或逐行歌词；逐字不可用时自动回退逐行。
    /// <paramref name="bilingual"/> 为 true 时优先返回原文 + 翻译的双语版本。
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
        !string.IsNullOrWhiteSpace(BilingualLineLyrics) || !string.IsNullOrWhiteSpace(BilingualWordLyrics);
}

/// <summary>网易云 / QQ 音乐联网搜索与歌词读取。</summary>
public interface IOnlineMusicSearchService
{
    Task<IReadOnlyList<OnlineSongResult>> SearchAsync(
        OnlineMusicSource source,
        string keyword,
        int limit = 20,
        CancellationToken cancellationToken = default);

    Task<OnlineSongDetails?> GetDetailsAsync(
        OnlineSongResult song,
        CancellationToken cancellationToken = default);
}
