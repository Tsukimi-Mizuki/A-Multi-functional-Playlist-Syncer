namespace NeteaseMusicManager.Core.Ncm;

/// <summary>NCM 文件头/元数据（来自加密文件内的 music: JSON）。</summary>
public sealed record NcmMetadata(
    string? Title,
    string? Artists,
    string? Album,
    string? Format,
    int Bitrate,
    long DurationMs,
    long? MusicId,
    string? Alias = null)
{
    public static NcmMetadata Empty { get; } = new(null, null, null, null, 0, 0, null);
}

public sealed record NcmFileInfo(string SourcePath, NcmMetadata? Metadata)
{
    public bool HasMetadata => Metadata is not null;
}

public sealed record NcmDecryptResult(
    string SourcePath,
    string? OutputPath,
    bool Success,
    string? Format = null,
    NcmMetadata? Metadata = null,
    byte[]? CoverData = null,
    string? ErrorMessage = null);

public sealed record NcmImportProgress(int Completed, int Total, string CurrentFile, string? Message = null);

/// <summary>网易云 .ncm 解密器。实现必须同时支持 MP3 与 FLAC 输出。</summary>
public interface INcmDecoder
{
    bool IsNcmFile(string path);

    /// <summary>只读取文件头/加密元数据，不解密音频数据。</summary>
    Task<NcmFileInfo> ReadInfoAsync(string ncmPath, CancellationToken cancellationToken = default);

    /// <summary>
    /// 解密到 <paramref name="outputDirectory"/>，自动识别 mp3/flac 扩展名。
    /// 不负责写标签；<see cref="NcmDecryptResult.CoverData"/> 会返回内嵌封面供标签层使用。
    /// </summary>
    Task<NcmDecryptResult> DecryptAsync(
        string ncmPath,
        string outputDirectory,
        bool overwrite = false,
        CancellationToken cancellationToken = default);
}
