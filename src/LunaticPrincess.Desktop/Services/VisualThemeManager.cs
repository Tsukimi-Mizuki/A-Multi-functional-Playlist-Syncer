using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;

namespace LunaticPrincess.Desktop.Services;

/// <summary>
/// 唯一的窗口材质：磨砂玻璃。
/// 主窗口使用 Windows 11 AcrylicBlur，让窗口后面的真实内容（动态视频、网页、桌面等）
/// 实时模糊透出；对话框保持可读的实色深色背景，但使用相同的深色标题栏。
/// </summary>
internal static class VisualThemeManager
{
    private const string FallbackBackground = "#0E131C";

    private const int DwmUseImmersiveDarkMode = 20;
    private const int DwmCaptionColor = 35;
    private const int DwmTextColor = 36;

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);

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
            ApplyDarkTitleBar(window, "#0A0F17", "#F4F7FB");
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
            window.TransparencyLevelHint = new[] { WindowTransparencyLevel.None };
            window.Background = new SolidColorBrush(Color.Parse(FallbackBackground));
            ApplyDarkTitleBar(window, FallbackBackground, "#F4F7FB");
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Apply dialog theme failed: {ex}");
        }
    }

    private static void ApplyDarkTitleBar(Window window, string backgroundHex, string textHex)
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

        var dark = 1;
        DwmSetWindowAttribute(handle, DwmUseImmersiveDarkMode, ref dark, sizeof(int));

        var caption = ToColorRef(Color.Parse(backgroundHex));
        DwmSetWindowAttribute(handle, DwmCaptionColor, ref caption, sizeof(int));

        var text = ToColorRef(Color.Parse(textHex));
        DwmSetWindowAttribute(handle, DwmTextColor, ref text, sizeof(int));
    }

    private static int ToColorRef(Color color) => color.R | (color.G << 8) | (color.B << 16);
}
