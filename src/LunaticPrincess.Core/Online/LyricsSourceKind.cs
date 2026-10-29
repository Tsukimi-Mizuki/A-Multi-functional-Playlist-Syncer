namespace LunaticPrincess.Core.Online;

/// <summary>
/// 歌词检索来源：主词源（网易云音乐、QQ 音乐，可带回标签/封面）与
/// 备用词源（酷狗音乐、LRCLIB，只提供歌词）。
/// 用于歌词编辑器的来源下拉与「备用词源」开关，序列化时按名字保存（与旧版的 OnlineMusicSource 兼容）。
/// </summary>
public enum LyricsSourceKind
{
    Netease = 0,
    QQMusic = 1,
    Kugou = 2,
    Lrclib = 3,
}

public static class LyricsSourceKindExtensions
{
    /// <summary>是否为备用词源（只补歌词，不改写标签与封面）。</summary>
    public static bool IsFallback(this LyricsSourceKind kind) =>
        kind is LyricsSourceKind.Kugou or LyricsSourceKind.Lrclib;

    /// <summary>下拉框与状态提示里显示的来源名。</summary>
    public static string ToDisplayLabel(this LyricsSourceKind kind) => kind switch
    {
        LyricsSourceKind.Netease => "网易云音乐",
        LyricsSourceKind.QQMusic => "QQ 音乐",
        LyricsSourceKind.Kugou => "酷狗音乐（仅歌词）",
        LyricsSourceKind.Lrclib => "LRCLIB（仅歌词）",
        _ => kind.ToString(),
    };

    /// <summary>主词源映射到在线搜索接口的枚举；备用词源返回 null。</summary>
    public static OnlineMusicSource? ToMusicSource(this LyricsSourceKind kind) => kind switch
    {
        LyricsSourceKind.Netease => OnlineMusicSource.Netease,
        LyricsSourceKind.QQMusic => OnlineMusicSource.QQMusic,
        _ => null,
    };
}
