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
/// 标题栏用主题里的 <c>CaptionBrush</c>（纯色，与顶栏/底栏的 <c>ChromeBarBrush</c> 同色），
/// 深色/浅色同时切换沉浸式深色标志、标题底色与标题文字色；
/// 主题切换时通过 <see cref="Refresh"/> 更新已打开窗口。
/// </summary>
internal static class VisualThemeManager
{
    private const int DwmUseImmersiveDarkMode = 20;
    private const int DwmBorderColor = 34;
    private const int DwmCaptionColor = 35;
    private const int DwmTextColor = 36;

    /// <summary>DWMWA_COLOR_NONE：让 DWM 不画那条 1px 边框（自绘标题栏时用）。</summary>
    private const int DwmColorNone = unchecked((int)0xFFFFFFFE);

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
            window.Opened += (_, _) => ScheduleClamp(window);
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
            window.Opened += (_, _) => ScheduleClamp(window);
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

        // 标题栏只能是纯色（DWM 不支持半透明标题栏），取主题里的 CaptionBrush：
        // 与顶栏（账号栏）、底栏的 ChromeBarBrush 同一色值，视觉上连成一体。
        // 不用 COLOR_NONE 让系统材质画标题栏：Acrylic 会按系统主题着色，深色下比客户区更亮、
        // 浅色下明显发灰（实测 84 / 152，客户区为 38 / 241），反而更割裂。
        var caption = ToColorRef(ResolveColor("CaptionBrush", dark ? "#151A23" : "#FEFEFE"));
        DwmSetWindowAttribute(handle, DwmCaptionColor, ref caption, sizeof(int));

        var text = ToColorRef(ResolveColor("TextPrimaryBrush", dark ? "#F4F7FB" : "#151A22"));
        DwmSetWindowAttribute(handle, DwmTextColor, ref text, sizeof(int));

        // 自绘标题栏的窗口（主窗口）不让 DWM 画那圈浅灰色的 1px 边框，
        // 否则深色标题栏最外沿会有一道亮线，看着不像融进界面。
        if (window.ExtendClientAreaToDecorationsHint)
        {
            var border = DwmColorNone;
            DwmSetWindowAttribute(handle, DwmBorderColor, ref border, sizeof(int));
        }
    }

    /// <summary>
    /// 窗口显示后再夹取位置：Owner 居中定位可能在 Opened 之后才生效，所以延后一轮调度。
    /// </summary>
    private static void ScheduleClamp(Window window)
    {
        Avalonia.Threading.Dispatcher.UIThread.Post(
            () => ClampToWorkingArea(window),
            Avalonia.Threading.DispatcherPriority.Loaded);
    }

    /// <summary>
    /// 高 DPI 下固定尺寸的窗口可能超出屏幕工作区（例如 125% 缩放时设置窗口高 950 物理像素），
    /// 这里按工作区裁到可容纳的尺寸，超出的长条内容由窗口内的 ScrollViewer 滚动。
    /// </summary>
    private static void ClampToWorkingArea(Window window)
    {
        try
        {
            var screen = window.Screens.ScreenFromWindow(window) ?? window.Screens.Primary;
            if (screen is null)
            {
                return;
            }

            var scale = window.RenderScaling > 0 ? window.RenderScaling : 1d;
            var area = screen.WorkingArea;
            var maxWidth = area.Width / scale - 24;
            var maxHeight = area.Height / scale - 24;
            if (maxWidth <= 0 || maxHeight <= 0)
            {
                return;
            }

            var width = double.IsNaN(window.Width) ? maxWidth : Math.Min(window.Width, maxWidth);
            var height = double.IsNaN(window.Height) ? maxHeight : Math.Min(window.Height, maxHeight);
            var resized = Math.Abs(width - window.Width) > 0.5 || Math.Abs(height - window.Height) > 0.5;

            var widthPx = (int)Math.Round(width * scale);
            var heightPx = (int)Math.Round(height * scale);
            var current = window.Position;
            // 以原中心点重新摆放，再夹回工作区。
            var left = current.X + ((int)Math.Round((double.IsNaN(window.Width) ? width : window.Width) * scale) - widthPx) / 2;
            var top = current.Y + ((int)Math.Round((double.IsNaN(window.Height) ? height : window.Height) * scale) - heightPx) / 2;
            left = Math.Clamp(left, area.X, Math.Max(area.X, area.Right - widthPx));
            top = Math.Clamp(top, area.Y, Math.Max(area.Y, area.Bottom - heightPx));
            var moved = left != current.X || top != current.Y;

            if (!resized && !moved)
            {
                return;
            }

            if (resized)
            {
                window.Width = width;
                window.Height = height;
            }

            window.Position = new PixelPoint(left, top);
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Clamp window to working area failed: {ex}");
        }
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
