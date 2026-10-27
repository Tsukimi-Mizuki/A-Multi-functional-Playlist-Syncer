using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Storage;

/// <summary>登录 Cookie 持久化。桌面端实现使用 DPAPI 加密，后续 Android 端可替换实现。</summary>
public interface ISessionStore
{
    Task<NeteaseSession?> LoadAsync(CancellationToken cancellationToken = default);
    Task SaveAsync(NeteaseSession session, CancellationToken cancellationToken = default);
    Task ClearAsync(CancellationToken cancellationToken = default);
}
