namespace NeteaseMusicManager.Core.Models;

public sealed record ArtistRef(long Id, string Name);

public sealed record AlbumRef(long Id, string Name, string? CoverUrl);

/// <summary>歌单中的一首歌。字段来自网易云 song/detail 与 playlist/detail 的归一化。</summary>
public sealed class NeteaseSong
{
    public long Id { get; init; }
    public string Name { get; init; } = string.Empty;
    public IReadOnlyList<ArtistRef> Artists { get; init; } = Array.Empty<ArtistRef>();
    public AlbumRef Album { get; init; } = new(0, string.Empty, null);
    public string? Alias { get; init; }
    public int DurationMs { get; init; }
    public int TrackNumber { get; init; }
    public int DiscNumber { get; init; } = 1;
    public string? ReleaseDate { get; init; }
    public int Fee { get; init; }
    public int MaxBitrate { get; init; }
    public string? CoverUrl => Album.CoverUrl;

    public string ArtistText => string.Join("/", Artists.Select(a => a.Name).Where(n => !string.IsNullOrWhiteSpace(n)));

    public string DisplayName =>
        string.IsNullOrWhiteSpace(Alias) ? Name : $"{Name} ({Alias})";

    public TimeSpan Duration => TimeSpan.FromMilliseconds(DurationMs);
}

public sealed record NeteaseUser(long UserId, string Nickname, string? AvatarUrl, int VipType);

public sealed record PlaylistSummary(
    long Id,
    string Name,
    string? CoverUrl,
    int TrackCount,
    string? CreatorNickname = null,
    string? Description = null);

public sealed class PlaylistDetail
{
    public long Id { get; init; }
    public string Name { get; init; } = string.Empty;
    public string? CoverUrl { get; init; }
    public string? Description { get; init; }
    public string? CreatorNickname { get; init; }
    public int TrackCount { get; init; }
    public IReadOnlyList<NeteaseSong> Songs { get; init; } = Array.Empty<NeteaseSong>();
    public IReadOnlyList<long> TrackIds { get; init; } = Array.Empty<long>();
    public bool IsPartial { get; init; }
}

public sealed record AlbumInfo(
    long Id,
    string Name,
    string? CoverUrl,
    string? Description,
    string? ReleaseDate,
    IReadOnlyList<ArtistRef> Artists);

public sealed class SongPlaybackInfo
{
    public long SongId { get; init; }
    public string? Url { get; init; }
    public int Bitrate { get; init; }
    public long Size { get; init; }
    public string? Format { get; init; }
    public string? Level { get; init; }
    public int Fee { get; init; }
    public bool IsTrial { get; init; }
    public int? TrialStartSeconds { get; init; }
    public int? TrialEndSeconds { get; init; }
    public int ExpiresInSeconds { get; init; }
    public string? ErrorMessage { get; init; }
    public bool IsSuccess => !string.IsNullOrWhiteSpace(Url);
}

public sealed record RawLyrics(
    string? Lrc,
    string? TranslatedLrc = null,
    string? RomanizedLrc = null,
    string? Yrc = null);

public enum QrLoginState
{
    Waiting,
    Scanned,
    Confirmed,
    Expired,
    Failed,
}

public sealed record QrLoginStart(string Key, string QrContent, DateTimeOffset ExpiresAt);

public sealed record QrLoginStatus(QrLoginState State, string Message, NeteaseSession? Session = null)
{
    public bool IsTerminal => State is QrLoginState.Confirmed or QrLoginState.Expired or QrLoginState.Failed;
}
