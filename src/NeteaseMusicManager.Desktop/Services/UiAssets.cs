using Avalonia.Controls;
using Avalonia.Platform;

namespace NeteaseMusicManager.Desktop.Services;

/// <summary>窗口图标等静态资源加载（缺失时静默降级，不影响功能）。</summary>
internal static class UiAssets
{
    private static WindowIcon? _windowIcon;
    private static bool _attempted;

    public static void ApplyWindowIcon(Window window)
    {
        try
        {
            if (!_attempted)
            {
                _attempted = true;
                using var stream = AssetLoader.Open(new Uri("avares://NeteaseMusicManager/Assets/app.ico"));
                _windowIcon = new WindowIcon(stream);
            }

            if (_windowIcon is not null)
            {
                window.Icon = _windowIcon;
            }
        }
        catch (Exception)
        {
            // 图标只是装饰。
        }
    }
}
