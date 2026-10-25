using NeteaseMusicManager.Core.Models;

namespace NeteaseMusicManager.Core.Netease;

/// <summary>
/// 网易云 web/weapi 客户端。所有方法在失败时应抛出 <see cref="NeteaseApiException"/>
/// （或 OperationCanceledException），不应该返回半截数据。
/// </summary>
public interface INeteaseApiClient
{
    NeteaseSession? Session { get; }
    bool IsLoggedIn { get; }

    /// <summary>切换持久化恢复的登录态；传 null 表示退出登录。</summary>
    void SetSession(NeteaseSession? session);

    Task<QrLoginStart> CreateQrLoginAsync(CancellationToken cancellationToken = default);

    /// <summary>轮询扫码状态；803 时返回 <see cref="QrLoginState.Confirmed"/> 并携带 Session。</summary>
    Task<QrLoginStatus> PollQrLoginAsync(string key, CancellationToken cancellationToken = default);

    Task<NeteaseUser?> GetCurrentUserAsync(CancellationToken cancellationToken = default);

    Task<IReadOnlyList<PlaylistSummary>> GetUserPlaylistsAsync(long userId, CancellationToken cancellationToken = default);

    Task<PlaylistDetail> GetPlaylistDetailAsync(long playlistId, CancellationToken cancellationToken = default);

    Task<IReadOnlyList<NeteaseSong>> GetSongDetailsAsync(
        IReadOnlyCollection<long> songIds,
        CancellationToken cancellationToken = default);

    Task<SongPlaybackInfo> GetSongPlaybackInfoAsync(
        long songId,
        AudioQuality quality,
        CancellationToken cancellationToken = default);

    Task<RawLyrics> GetLyricsAsync(long songId, CancellationToken cancellationToken = default);

    Task<AlbumInfo?> GetAlbumAsync(long albumId, CancellationToken cancellationToken = default);
}

public sealed class NeteaseApiException : Exception
{
    public NeteaseApiException(string message, int? code = null, Exception? innerException = null)
        : base(message, innerException)
    {
        Code = code;
    }

    public int? Code { get; }
}
