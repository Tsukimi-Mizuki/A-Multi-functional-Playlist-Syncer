using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;

namespace LunaticPrincess.Desktop.Services;

/// <summary>
/// 唯一的窗口材质：液态磨砂玻璃。
/// 主窗口与所有二级窗口都使用 Windows 11 AcrylicBlur，让窗口后面的真实内容
/// （动态视频、网页、桌面等）实时模糊透出；二级窗口再用半透明的
/// <c>DialogGlassBrush</c> 作为窗口底色，保证文字可读，并使用统一的深色标题栏。
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
            // 与主窗口一致的 AcrylicBlur：不支持时依次降级到 Blur、None。
            // 底色由各窗口 XAML 的 DialogGlassBrush（半透明）提供，这里不覆盖 Background，
            // 否则会盖掉模糊层。
            window.TransparencyLevelHint = new[]
            {
                WindowTransparencyLevel.AcrylicBlur,
                WindowTransparencyLevel.Blur,
                WindowTransparencyLevel.None,
            };
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
