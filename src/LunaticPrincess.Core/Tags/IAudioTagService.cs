using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Tags;

public sealed class AudioTagData
{
    public string? Title { get; init; }
    public IReadOnlyList<string> Artists { get; init; } = Array.Empty<string>();
    public string? Album { get; init; }
    public string? AlbumArtist { get; init; }
    public uint? TrackNumber { get; init; }
    public uint? DiscNumber { get; init; }
    public uint? Year { get; init; }
    public byte[]? CoverData { get; init; }
    public string? CoverMimeType { get; init; }
    public string? PlainLyrics { get; init; }
    public string? WordLyrics { get; init; }

    /// <summary>额外写入网易云歌曲 ID，便于下次扫描时精确匹配。</summary>
    public long? NeteaseId { get; init; }

    public string? Comment { get; init; }

    /// <summary>流派；多个值用 / 分隔。</summary>
    public string? Genre { get; init; }

    /// <summary>作曲家；多个值用 / 分隔。</summary>
    public string? Composer { get; init; }

    /// <summary>编辑器保存时替换全部字段（包括清空未填写的标签），而不是只更新非空字段。</summary>
    public bool ReplaceAll { get; init; }
}

public sealed record AudioTagInfo(
    string? Title,
    string? Artist,
    string? Album,
    string? AlbumArtist,
    long DurationMs,
    int Bitrate,
    string? Lyrics = null,
    bool HasCover = false,
    long? NeteaseId = null,
    uint? Year = null,
    uint? TrackNumber = null,
    uint? DiscNumber = null,
    string? Genre = null,
    string? Composer = null,
    string? Comment = null,
    byte[]? CoverData = null,
    long FileSize = 0,
    string? FileFormat = null);

/// <summary>使用 TagLibSharp 读写 MP3/FLAC/M4A/OGG 标签。</summary>
public interface IAudioTagService
{
    Task<AudioTagInfo?> ReadAsync(string audioPath, CancellationToken cancellationToken = default);

    /// <summary>
    /// 写入标签。按文件容器自动选择 ID3v2 / VorbisComment / MP4 tag。
    /// WordLyrics 应优先写入带逐字时间轴的增强 LRC；PlainLyrics 写入普通 LRC。
    /// </summary>
    Task WriteAsync(string audioPath, AudioTagData tags, CancellationToken cancellationToken = default);
}
