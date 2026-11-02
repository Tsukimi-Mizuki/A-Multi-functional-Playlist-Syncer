using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Netease;

/// <summary>
/// 网易云 web/weapi 客户端。所有方法在失败时应抛出 <see cref="NeteaseApiException"/>
/// （或 OperationCanceledException），不应该返回半截数据。
/// </summary>
public interface INeteaseApiClient
{
    NeteaseSession? Session { get; }
    bool IsLoggedIn { get; }

    /// <summary>
    /// 登录态 Cookie 被服务端续期/轮换后触发（参数为最新登录态）。
    /// 显式 <see cref="SetSession"/> 与扫码登录不触发；订阅方应把登录态落盘，且不要在回调里阻塞。
    /// </summary>
    event EventHandler<NeteaseSession>? SessionChanged;

    /// <summary>切换持久化恢复的登录态；传 null 表示退出登录。</summary>
    void SetSession(NeteaseSession? session);

    Task<QrLoginStart> CreateQrLoginAsync(CancellationToken cancellationToken = default);

    /// <summary>轮询扫码状态；803 时返回 <see cref="QrLoginState.Confirmed"/> 并携带 Session。</summary>
    Task<QrLoginStatus> PollQrLoginAsync(string key, CancellationToken cancellationToken = default);

    /// <summary>
    /// 读取当前账号。接口 HTTP 200 且 code=200 但 account/profile 为空时返回 null（明确未登录）；
    /// 其余失败（HTTP 错误、限流、风控、网络）一律抛出 <see cref="NeteaseApiException"/>，调用方不应据此判定登录失效。
    /// </summary>
    Task<NeteaseUser?> GetCurrentUserAsync(CancellationToken cancellationToken = default);

    /// <summary>
    /// 请求服务端续期登录态（login/token/refresh），并把响应里的 Set-Cookie 合并进当前会话。
    /// 失败（未登录、网络、接口拒绝）只返回 false，不抛出业务异常；取消仍抛 OperationCanceledException。
    /// </summary>
    Task<bool> RefreshLoginAsync(CancellationToken cancellationToken = default);

    Task<IReadOnlyList<PlaylistSummary>> GetUserPlaylistsAsync(long userId, CancellationToken cancellationToken = default);

    Task<PlaylistDetail> GetPlaylistDetailAsync(long playlistId, CancellationToken cancellationToken = default);

    /// <summary>按关键词搜索歌曲（公开接口，无需登录）。</summary>
    Task<IReadOnlyList<NeteaseSong>> SearchSongsAsync(
        string keyword,
        int limit = 20,
        CancellationToken cancellationToken = default);

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
    public NeteaseApiException(
        string message,
        int? code = null,
        Exception? innerException = null,
        bool isHttpStatus = false)
        : base(message, innerException)
    {
        Code = code;
        IsHttpStatus = isHttpStatus;
    }

    /// <summary>接口 JSON 里的业务 code，或（<see cref="IsHttpStatus"/> 为 true 时）HTTP 状态码。</summary>
    public int? Code { get; }

    /// <summary><see cref="Code"/> 是 HTTP 状态码而不是接口业务 code（例如 401/302/502）。</summary>
    public bool IsHttpStatus { get; }

    /// <summary>接口明确返回「未登录」（业务 code=301）；HTTP 层状态码不算。</summary>
    public bool IsNotLoggedIn => !IsHttpStatus && Code == 301;
}
