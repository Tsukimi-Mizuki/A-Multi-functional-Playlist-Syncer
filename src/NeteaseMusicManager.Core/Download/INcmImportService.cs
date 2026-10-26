using NeteaseMusicManager.Core.Lyrics;
using NeteaseMusicManager.Core.Ncm;

namespace NeteaseMusicManager.Core.Download;

/// <summary>批量导入本地 .ncm：解密、自动写入文件内已有元数据和封面，可选自动匹配在线标签、封面与歌词。</summary>
/// <param name="AutoMatchOnline">解密成功后是否自动在线匹配标签、封面与歌词。</param>
/// <param name="MatchOptions">在线匹配参数（来源优先级、逐行/逐字歌词等）；null 使用默认（网易云优先、逐行、强制覆盖）。</param>
public sealed record NcmImportOptions(
    bool AutoMatchOnline = false,
    BatchLyricsOptions? MatchOptions = null);

public interface INcmImportService
{
    Task<IReadOnlyList<NcmDecryptResult>> ImportAsync(
        IReadOnlyList<string> ncmPaths,
        string outputDirectory,
        bool overwrite = false,
        IProgress<NcmImportProgress>? progress = null,
        CancellationToken cancellationToken = default,
        NcmImportOptions? options = null);
}
