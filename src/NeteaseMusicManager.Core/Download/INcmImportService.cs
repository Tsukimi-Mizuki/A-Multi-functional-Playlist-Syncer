using NeteaseMusicManager.Core.Ncm;

namespace NeteaseMusicManager.Core.Download;

/// <summary>批量导入本地已有 .ncm：解密、自动写入文件内已有元数据和封面。</summary>
public interface INcmImportService
{
    Task<IReadOnlyList<NcmDecryptResult>> ImportAsync(
        IReadOnlyList<string> ncmPaths,
        string outputDirectory,
        bool overwrite = false,
        IProgress<NcmImportProgress>? progress = null,
        CancellationToken cancellationToken = default);
}
