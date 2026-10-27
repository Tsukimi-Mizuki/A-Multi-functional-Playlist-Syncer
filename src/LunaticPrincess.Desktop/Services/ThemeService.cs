using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Styling;
using Avalonia.Threading;
using LunaticPrincess.Core.Settings;

namespace LunaticPrincess.Desktop.Services;

/// <summary>
/// 主题切换入口：把 <see cref="ThemeMode"/> 映射到 Avalonia 的 <see cref="ThemeVariant"/>，
/// 刷新转换器使用的画刷缓存，并让已打开的窗口（主窗口与二级窗口）同步标题栏配色。
/// </summary>
public static class ThemeService
{
    public static void Apply(ThemeMode mode)
    {
        var app = Application.Current;
        if (app is null)
        {
            return;
        }

        app.RequestedThemeVariant = ToVariant(mode);
        ThemeBrushes.Refresh();

        // 主题资源在第一次求值时替换，下一帧再刷新一次画刷与标题栏，避免读到旧值。
        Dispatcher.UIThread.Post(
            () =>
            {
                ThemeBrushes.Refresh();
                RefreshWindowChrome();
            },
            DispatcherPriority.Loaded);
    }

    public static ThemeVariant ToVariant(ThemeMode mode) => mode switch
    {
        ThemeMode.Light => ThemeVariant.Light,
        ThemeMode.Dark => ThemeVariant.Dark,
        _ => ThemeVariant.Default,
    };

    /// <summary>按当前实际主题刷新所有已打开窗口的标题栏（主题切换后调用）。</summary>
    public static void RefreshWindowChrome()
    {
        if (Application.Current?.ApplicationLifetime is not IClassicDesktopStyleApplicationLifetime desktop)
        {
            return;
        }

        foreach (var window in desktop.Windows)
        {
            VisualThemeManager.Refresh(window);
        }
    }
}
