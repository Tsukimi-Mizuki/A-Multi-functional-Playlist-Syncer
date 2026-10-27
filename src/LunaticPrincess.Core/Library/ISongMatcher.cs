using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Library;

public enum MatchMethod
{
    None,
    NeteaseId,
    Metadata,
    FileName,
}

public enum LocalAvailability
{
    Missing,
    Available,
    EncryptedNcm,
}

public sealed class SongMatch
{
    public required NeteaseSong Song { get; init; }
    public LocalTrack? LocalTrack { get; init; }
    public MatchMethod Method { get; init; }
    public double Confidence { get; init; }
    public LocalAvailability Availability { get; init; }

    public bool IsPresent => LocalTrack is not null;
}

public interface ISongMatcher
{
    /// <summary>把歌单歌曲与本地曲库做一对一匹配；每首本地曲最多匹配一首歌单歌曲。</summary>
    IReadOnlyList<SongMatch> Match(
        IReadOnlyList<NeteaseSong> playlistSongs,
        IReadOnlyList<LocalTrack> localTracks);
}
