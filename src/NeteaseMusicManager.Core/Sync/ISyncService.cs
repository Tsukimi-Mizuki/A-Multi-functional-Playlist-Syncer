using NeteaseMusicManager.Core.Download;
using NeteaseMusicManager.Core.Library;
using NeteaseMusicManager.Core.Models;

namespace NeteaseMusicManager.Core.Sync;

public sealed class SyncRequest
{
    public required NeteaseSong Song { get; init; }
    public LocalTrack? LocalMatch { get; init; }
}

public sealed class SyncOptions
{
    public required DownloadOptions Download { get; init; }
    public bool RedownloadExisting { get; init; }
}

public enum SyncSourceKind
{
    Downloaded,
    DecryptedNcm,
    Skipped,
    Failed,
}

public sealed record SyncItemResult(
    long SongId,
    string SongName,
    bool Success,
    SyncSourceKind Source,
    string? OutputPath = null,
    string? Message = null);

public sealed class SyncSummary
{
    public IReadOnlyList<SyncItemResult> Results { get; init; } = Array.Empty<SyncItemResult>();
    public int SuccessCount => Results.Count(r => r.Success);
    public int FailedCount => Results.Count(r => !r.Success);
    public int DownloadedCount => Results.Count(r => r.Source == SyncSourceKind.Downloaded);
    public int DecryptedCount => Results.Count(r => r.Source == SyncSourceKind.DecryptedNcm);
    public int SkippedCount => Results.Count(r => r.Source == SyncSourceKind.Skipped);
}

public sealed record SyncProgress(
    int Completed,
    int Total,
    string SongName,
    string Stage,
    string? Message = null,
    double? Percent = null);

/// <summary>
/// 一键同步：已存在且无需更新 → 跳过；本地是 NCM → 解密 + 重新写标签；
/// 不存在 → 联网下载；始终自动写元数据/封面/歌词。
/// </summary>
public interface ISyncService
{
    Task<SyncSummary> SyncAsync(
        IReadOnlyList<SyncRequest> requests,
        SyncOptions options,
        IProgress<SyncProgress>? progress = null,
        CancellationToken cancellationToken = default);
}
