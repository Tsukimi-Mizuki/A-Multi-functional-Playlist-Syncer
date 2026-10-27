using Avalonia.Threading;

namespace LunaticPrincess.Desktop.Services;

/// <summary>把后台回调安全地调度到 UI 线程的小工具。</summary>
public static class UiThread
{
    public static bool IsOnUiThread => Dispatcher.UIThread.CheckAccess();

    public static void Post(Action action)
    {
        if (Dispatcher.UIThread.CheckAccess())
        {
            action();
        }
        else
        {
            Dispatcher.UIThread.Post(action);
        }
    }
}
