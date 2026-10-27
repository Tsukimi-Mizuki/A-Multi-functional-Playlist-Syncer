using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Styling;

namespace LunaticPrincess.Desktop.Services;

/// <summary>
/// 唯一的窗口材质：液态磨砂玻璃。
/// 主窗口与所有二级窗口都使用 Windows 11 AcrylicBlur，让窗口后面的真实内容
/// （动态视频、网页、桌面等）实时模糊透出；二级窗口再用半透明的
/// <c>DialogGlassBrush</c> 作为窗口底色，保证文字可读。
/// 标题栏颜色跟随当前主题（深色/浅色），主题切换时通过 <see cref="Refresh"/> 更新已打开窗口。
/// </summary>
internal static class VisualThemeManager
{
    private const int DwmUseImmersiveDarkMode = 20;
    private const int DwmCaptionColor = 35;
    private const int DwmTextColor = 36;

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);

    /// <summary>当前是否深色主题（跟随系统时取解析后的实际主题）。</summary>
    private static bool IsDarkTheme => Application.Current?.ActualThemeVariant != ThemeVariant.Light;

    public static void ApplyMainWindow(Window window)
    {
        try
        {
            window.TransparencyLevelHint = new[]
            {
                WindowTransparencyLevel.AcrylicBlur,
                WindowTransparencyLevel.Blur,
                WindowTransparencyLevel.None,
            };
            window.Background = Brushes.Transparent;
            Refresh(window);
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Apply acrylic failed: {ex}");
        }
    }

    public static void ApplyDialog(Window window)
    {
        try
        {
            // 与主窗口一致的 AcrylicBlur：不支持时依次降级到 Blur、None。
            // 底色由各窗口 XAML 的 DialogGlassBrush（半透明）提供，这里不覆盖 Background，
            // 否则会盖掉模糊层。
            window.TransparencyLevelHint = new[]
            {
                WindowTransparencyLevel.AcrylicBlur,
                WindowTransparencyLevel.Blur,
                WindowTransparencyLevel.None,
            };
            Refresh(window);
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Apply dialog theme failed: {ex}");
        }
    }

    /// <summary>按当前主题刷新标题栏（深色/浅色、标题色、文字色）。</summary>
    public static void Refresh(Window window)
    {
        if (!OperatingSystem.IsWindowsVersionAtLeast(10, 0, 22000))
        {
            return;
        }

        var handle = window.TryGetPlatformHandle()?.Handle ?? IntPtr.Zero;
        if (handle == IntPtr.Zero)
        {
            return;
        }

        var dark = IsDarkTheme;
        var darkMode = dark ? 1 : 0;
        DwmSetWindowAttribute(handle, DwmUseImmersiveDarkMode, ref darkMode, sizeof(int));

        var caption = ToColorRef(ResolveColor("AppBackgroundBrush", dark ? "#0A0F17" : "#F3F6FB"));
        DwmSetWindowAttribute(handle, DwmCaptionColor, ref caption, sizeof(int));

        var text = ToColorRef(ResolveColor("TextPrimaryBrush", dark ? "#F4F7FB" : "#151A22"));
        DwmSetWindowAttribute(handle, DwmTextColor, ref text, sizeof(int));
    }

    private static Color ResolveColor(string key, string fallback)
    {
        var app = Application.Current;
        if (app is not null && app.TryGetResource(key, app.ActualThemeVariant, out var value) && value is ISolidColorBrush brush)
        {
            return brush.Color;
        }

        return Color.Parse(fallback);
    }

    private static int ToColorRef(Color color) => color.R | (color.G << 8) | (color.B << 16);
}
