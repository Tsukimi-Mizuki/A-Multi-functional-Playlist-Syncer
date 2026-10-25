using NeteaseMusicManager.Core.Models;

namespace NeteaseMusicManager.Core.Lyrics;

/// <summary>普通 LRC / 增强 LRC / 网易云 YRC 解析与序列化。</summary>
public interface ILyricParser
{
    /// <summary>优先使用 YRC 逐字数据；没有时回退到 LRC，并合并翻译。</summary>
    LyricDocument Parse(RawLyrics rawLyrics);

    string ToPlainLrc(LyricDocument document);

    /// <summary>生成增强 LRC（行时间 + 词标签），用于把逐字歌词写入标签。</summary>
    string ToEnhancedLrc(LyricDocument document);
}
