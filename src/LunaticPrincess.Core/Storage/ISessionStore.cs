using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Storage;

/// <summary>登录 Cookie 持久化。桌面端实现使用 DPAPI 加密，后续 Android 端可替换实现。</summary>
public interface ISessionStore
{
    Task<NeteaseSession?> LoadAsync(CancellationToken cancellationToken = default);

    /// <summary>
    /// 最近一次 <see cref="LoadAsync"/> 返回 null 时的失败原因（中文）。
    /// 文件不存在属于正常的「未登录」，此时为 null；文件损坏、无法解密或读取失败时给出说明。
    /// </summary>
    string? LastLoadFailure => null;

    Task SaveAsync(NeteaseSession session, CancellationToken cancellationToken = default);
    Task ClearAsync(CancellationToken cancellationToken = default);
}
