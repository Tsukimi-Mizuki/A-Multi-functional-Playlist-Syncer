using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Ncm;

namespace LunaticPrincess.Core.Library;

public sealed record LocalTrack
{
    public required string Path { get; init; }
    public string Title { get; init; } = string.Empty;
    public string? Artist { get; init; }
    public string? Album { get; init; }
    public long DurationMs { get; init; }
    public string? Format { get; init; }
    public int? Bitrate { get; init; }
    public bool IsNcm { get; init; }
    public NcmMetadata? NcmMetadata { get; init; }
    public long? NeteaseId { get; init; }
    public bool HasCover { get; init; }
}

public sealed record LibraryScanProgress(int ScannedFiles, int TotalFiles, string CurrentFile);

public interface ILocalLibraryService
{
    /// <summary>递归扫描给定目录，支持 mp3/flac/m4a/ogg/wav 及 ncm。</summary>
    Task<IReadOnlyList<LocalTrack>> ScanAsync(
        IEnumerable<string> directories,
        IProgress<LibraryScanProgress>? progress = null,
        CancellationToken cancellationToken = default);
}
