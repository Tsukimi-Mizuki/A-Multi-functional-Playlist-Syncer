using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Download;

public sealed class DownloadOptions
{
    public required string TargetDirectory { get; init; }
    public AudioQuality Quality { get; init; } = AudioQuality.ExHigh;
    public LyricsWriteMode LyricsMode { get; init; } = LyricsWriteMode.Line;
    public bool WriteCover { get; init; } = true;
    public bool OverwriteExisting { get; init; }
    public bool SkipExisting { get; init; } = true;
    public string FileNameTemplate { get; init; } = "{artist} - {title}";
    public int MaxConcurrency { get; init; } = 3;

    /// <summary>写入基础标签后是否再在线补全标签/封面/歌词（QQ 逐字歌词优先、网易云兜底）。</summary>
    public bool AutoMatchOnline { get; init; } = true;

    /// <summary>在线补全参数；为空时使用 <see cref="BatchLyricsOptions.AutoEnrich"/>。</summary>
    public BatchLyricsOptions? MatchOptions { get; init; }
}

public sealed record DownloadProgress(
    int Completed,
    int Total,
    string SongName,
    string Stage,
    string? Message = null,
    double? Percent = null);

public sealed record DownloadItemResult(
    long SongId,
    string SongName,
    bool Success,
    string? OutputPath = null,
    string? ErrorMessage = null,
    bool DecryptedFromNcm = false,
    bool Skipped = false);

public sealed class DownloadSummary
{
    public IReadOnlyList<DownloadItemResult> Results { get; init; } = Array.Empty<DownloadItemResult>();
    public int SuccessCount => Results.Count(r => r.Success && !r.Skipped);
    public int SkippedCount => Results.Count(r => r.Skipped);
    public int FailedCount => Results.Count(r => !r.Success && !r.Skipped);
}

/// <summary>按歌单歌曲批量下载、自动识别 NCM 并写标签/歌词。</summary>
public interface IDownloadService
{
    Task<DownloadSummary> DownloadAsync(
        IReadOnlyList<NeteaseSong> songs,
        DownloadOptions options,
        IProgress<DownloadProgress>? progress = null,
        CancellationToken cancellationToken = default);
}
