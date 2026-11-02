using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Netease;

namespace LunaticPrincess.Core.Storage;

/// <summary>
/// 订阅 <see cref="INeteaseApiClient.SessionChanged"/>，把被服务端续期/轮换过的登录态串行落盘。
/// 写入时总是读取客户端<b>当前</b>登录态（而不是事件携带的快照），连续多次变化会合并成一次写入，
/// 且不会与退出登录竞争：当前已无登录态时直接跳过。
/// </summary>
public sealed class SessionAutoSaver : IDisposable
{
    private readonly INeteaseApiClient _api;
    private readonly ISessionStore _store;
    private readonly Action<Exception>? _onError;
    private readonly object _gate = new();
    private bool _dirty;
    private bool _running;
    private bool _disposed;
    private Task _pump = Task.CompletedTask;

    /// <param name="api">网易云客户端。</param>
    /// <param name="store">登录态存储。</param>
    /// <param name="onError">落盘失败时的回调（吞掉异常，仅用于记录）。</param>
    public SessionAutoSaver(INeteaseApiClient api, ISessionStore store, Action<Exception>? onError = null)
    {
        _api = api ?? throw new ArgumentNullException(nameof(api));
        _store = store ?? throw new ArgumentNullException(nameof(store));
        _onError = onError;
        _api.SessionChanged += OnSessionChanged;
    }

    /// <summary>等待当前已排队的写入全部完成（测试与应用退出前使用）。</summary>
    public Task FlushAsync()
    {
        lock (_gate)
        {
            return _pump;
        }
    }

    public void Dispose()
    {
        lock (_gate)
        {
            if (_disposed)
            {
                return;
            }

            _disposed = true;
        }

        _api.SessionChanged -= OnSessionChanged;
    }

    private void OnSessionChanged(object? sender, NeteaseSession session)
    {
        lock (_gate)
        {
            if (_disposed)
            {
                return;
            }

            _dirty = true;
            if (_running)
            {
                return;
            }

            _running = true;
            _pump = Task.Run(PumpAsync);
        }
    }

    private async Task PumpAsync()
    {
        while (true)
        {
            lock (_gate)
            {
                if (!_dirty)
                {
                    _running = false;
                    return;
                }

                _dirty = false;
            }

            try
            {
                var current = _api.Session;
                if (current is { IsLoggedIn: true })
                {
                    await _store.SaveAsync(current, CancellationToken.None).ConfigureAwait(false);
                }
            }
            catch (Exception ex)
            {
                try
                {
                    _onError?.Invoke(ex);
                }
                catch (Exception)
                {
                    // 记录回调自身出错也不能中断写入循环。
                }
            }
        }
    }
}
