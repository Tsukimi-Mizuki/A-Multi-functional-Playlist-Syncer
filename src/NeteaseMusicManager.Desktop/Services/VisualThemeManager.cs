using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Styling;
using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Desktop.Converters;

namespace NeteaseMusicManager.Desktop.Services;

/// <summary>
/// 外观与设计令牌（参考 Salt UI 的 SaltColors / SaltShapes / 胶囊按钮语言）：
/// - Salt 浅色 / Salt 深色：中性背景、发丝描边卡片、蓝色高亮、胶囊形控件；
/// - 磨砂玻璃 / 亚克力 / 液态玻璃：在 Salt 形状基础上叠加 Windows 材质与彩色光斑；
/// - 经典深色：原实色主题。
/// 颜色通过修改 Application.Resources 的 SolidColorBrush 实例即时传播。
/// </summary>
internal static class VisualThemeManager
{
    private sealed record Palette(
        string AppBackground,
        string Header,
        string Panel,
        string PanelAlt,
        string PanelHover,
        string Input,
        string Border,
        string BorderStrong,
        string TextPrimary,
        string TextSecondary,
        string TextTertiary,
        string Accent,
        string AccentHover,
        string AccentPressed,
        string AccentSoft,
        string AccentBorder,
        string Control,
        string ControlHover,
        string ControlPressed,
        string RowHover,
        string Success,
        string SuccessSoft,
        string Warning,
        string WarningSoft,
        string Danger,
        string DangerSoft,
        string DangerBorder,
        bool LightTheme,
        bool SolidWindow);

    private static readonly Palette Classic = new(
        AppBackground: "#0F0F12", Header: "#151519", Panel: "#17171B", PanelAlt: "#1D1D22",
        PanelHover: "#26262C", Input: "#121215", Border: "#2A2A31", BorderStrong: "#3A3A44",
        TextPrimary: "#F2F2F5", TextSecondary: "#A2A2AE", TextTertiary: "#70707C",
        Accent: "#EC4141", AccentHover: "#FF5C5C", AccentPressed: "#D33232",
        AccentSoft: "#331F1F", AccentBorder: "#5C2A2A",
        Control: "#1D1D22", ControlHover: "#26262C", ControlPressed: "#303038",
        RowHover: "#14FFFFFF",
        Success: "#3DCC7E", SuccessSoft: "#12281D", Warning: "#E8A33D", WarningSoft: "#2A2213",
        Danger: "#F26D6D", DangerSoft: "#2C1718", DangerBorder: "#4A2224",
        LightTheme: false, SolidWindow: true);

    private static readonly Palette Frosted = new(
        AppBackground: "#0A0F17", Header: "#B3151B24", Panel: "#A3151A23", PanelAlt: "#BF1D242F",
        PanelHover: "#D92B3442", Input: "#CC10151D", Border: "#33FFFFFF", BorderStrong: "#59FFFFFF",
        TextPrimary: "#F4F7FB", TextSecondary: "#C2CBD7", TextTertiary: "#96A1B0",
        Accent: "#FF6B7D", AccentHover: "#FF8A99", AccentPressed: "#E05569",
        AccentSoft: "#33EC4141", AccentBorder: "#66FF7A7A",
        Control: "#1AFFFFFF", ControlHover: "#2BFFFFFF", ControlPressed: "#3DFFFFFF",
        RowHover: "#14FFFFFF",
        Success: "#5BDB95", SuccessSoft: "#1E3428", Warning: "#F0B64D", WarningSoft: "#332917",
        Danger: "#FF7D7D", DangerSoft: "#3B1C20", DangerBorder: "#66FF7D7D",
        LightTheme: false, SolidWindow: false);

    private static readonly Palette Acrylic = new(
        AppBackground: "#0B0D11", Header: "#BF101217", Panel: "#B316191F", PanelAlt: "#D91F232B",
        PanelHover: "#E62C313B", Input: "#D9111319", Border: "#38FFFFFF", BorderStrong: "#59FFFFFF",
        TextPrimary: "#F5F6F8", TextSecondary: "#C4C8D1", TextTertiary: "#9AA0AC",
        Accent: "#FF5D78", AccentHover: "#FF7F94", AccentPressed: "#D94861",
        AccentSoft: "#33EC4141", AccentBorder: "#5CFF7A7A",
        Control: "#1AFFFFFF", ControlHover: "#2BFFFFFF", ControlPressed: "#3DFFFFFF",
        RowHover: "#14FFFFFF",
        Success: "#5BDB95", SuccessSoft: "#1E3428", Warning: "#F0B64D", WarningSoft: "#332917",
        Danger: "#FF7D7D", DangerSoft: "#3B1C20", DangerBorder: "#66FF7D7D",
        LightTheme: false, SolidWindow: false);

    private static readonly Palette Liquid = new(
        AppBackground: "#0A0E19", Header: "#B30B1220", Panel: "#8C0C1422", PanelAlt: "#A6162033",
        PanelHover: "#CC26324A", Input: "#B30E1728", Border: "#3DFFFFFF", BorderStrong: "#66FFFFFF",
        TextPrimary: "#FFFFFF", TextSecondary: "#D6DDEB", TextTertiary: "#AAB6CA",
        Accent: "#FF6A91", AccentHover: "#FF8EAC", AccentPressed: "#DA4E74",
        AccentSoft: "#3DFF5C8A", AccentBorder: "#66FF8AAA",
        Control: "#1AFFFFFF", ControlHover: "#2BFFFFFF", ControlPressed: "#3DFFFFFF",
        RowHover: "#14FFFFFF",
        Success: "#6EE7A4", SuccessSoft: "#1E3428", Warning: "#F5C15D", WarningSoft: "#332917",
        Danger: "#FF8798", DangerSoft: "#3B1C23", DangerBorder: "#66FF8798",
        LightTheme: false, SolidWindow: false);

    private static readonly Palette SaltLight = new(
        AppBackground: "#F3F3F3", Header: "#F3F3F3", Panel: "#FFFFFFFF", PanelAlt: "#F7F7F7",
        PanelHover: "#12000000", Input: "#F2F2F2", Border: "#26000000", BorderStrong: "#40000000",
        TextPrimary: "#1E1715", TextSecondary: "#8C8C8C", TextTertiary: "#9E9E9E",
        Accent: "#0470E6", AccentHover: "#2183F0", AccentPressed: "#0360C4",
        AccentSoft: "#1A0470E6", AccentBorder: "#400470E6",
        Control: "#0F000000", ControlHover: "#1A000000", ControlPressed: "#26000000",
        RowHover: "#0A000000",
        Success: "#168A4C", SuccessSoft: "#1A3DCC7E", Warning: "#A96500", WarningSoft: "#1FE8A33D",
        Danger: "#D93636", DangerSoft: "#1AF26D6D", DangerBorder: "#33D93636",
        LightTheme: true, SolidWindow: true);

    private static readonly Palette SaltDark = new(
        AppBackground: "#202020", Header: "#202020", Panel: "#14FFFFFF", PanelAlt: "#1FFFFFFF",
        PanelHover: "#24FFFFFF", Input: "#1AFFFFFF", Border: "#1AFFFFFF", BorderStrong: "#33FFFFFF",
        TextPrimary: "#EBEEF1", TextSecondary: "#BFE1E6EB", TextTertiary: "#80E1E6EB",
        Accent: "#0088FF", AccentHover: "#2B9EFF", AccentPressed: "#0077E0",
        AccentSoft: "#260088FF", AccentBorder: "#4D0088FF",
        Control: "#14FFFFFF", ControlHover: "#24FFFFFF", ControlPressed: "#33FFFFFF",
        RowHover: "#0AFFFFFF",
        Success: "#43D98A", SuccessSoft: "#1A3DCC7E", Warning: "#F0B64D", WarningSoft: "#1FE8A33D",
        Danger: "#FF6B6B", DangerSoft: "#26FF6B6B", DangerBorder: "#4DFF6B6B",
        LightTheme: false, SolidWindow: true);

    public static AppVisualStyle CurrentStyle { get; private set; } = AppVisualStyle.SaltLight;

    public static void Apply(Window window, AppVisualStyle style)
    {
        CurrentStyle = style;
        try
        {
            var palette = GetPalette(style);
            ApplyPalette(palette);
            ApplyThemeVariant(palette);
            ApplyTitleBar(window, palette);

            if (palette.SolidWindow)
            {
                window.TransparencyLevelHint = new[] { WindowTransparencyLevel.None };
                window.Background = new SolidColorBrush(Color.Parse(palette.AppBackground));
            }
            else
            {
                window.TransparencyLevelHint = style == AppVisualStyle.FrostedGlass
                    ? new[]
                    {
                        WindowTransparencyLevel.Mica,
                        WindowTransparencyLevel.AcrylicBlur,
                        WindowTransparencyLevel.Blur,
                        WindowTransparencyLevel.None,
                    }
                    : new[]
                    {
                        WindowTransparencyLevel.AcrylicBlur,
                        WindowTransparencyLevel.Blur,
                        WindowTransparencyLevel.None,
                    };
                window.Background = Brushes.Transparent;
            }
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Apply visual style failed: {ex}");
        }
    }

    /// <summary>对话框不允许使用窗口透明材质，但需要与主窗口共享调色板和原生标题栏颜色。</summary>
    public static void ApplyDialog(Window window, AppVisualStyle style)
    {
        try
        {
            var palette = GetPalette(style);
            ApplyPalette(palette);
            ApplyThemeVariant(palette);
            ApplyTitleBar(window, palette);
            window.TransparencyLevelHint = new[] { WindowTransparencyLevel.None };
            window.Background = new SolidColorBrush(Color.Parse(palette.AppBackground));
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Apply dialog style failed: {ex}");
        }
    }

    private static Palette GetPalette(AppVisualStyle style) => style switch
    {
        AppVisualStyle.SaltLight => SaltLight,
        AppVisualStyle.SaltDark => SaltDark,
        AppVisualStyle.FrostedGlass => Frosted,
        AppVisualStyle.Acrylic => Acrylic,
        AppVisualStyle.LiquidGlass => Liquid,
        _ => Classic,
    };

    private static void ApplyPalette(Palette palette)
    {
        SetBrush("AppBackgroundBrush", palette.AppBackground);
        SetBrush("HeaderBrush", palette.Header);
        SetBrush("PanelBrush", palette.Panel);
        SetBrush("PanelAltBrush", palette.PanelAlt);
        SetBrush("PanelHoverBrush", palette.PanelHover);
        SetBrush("InputBrush", palette.Input);
        SetBrush("BorderBrush", palette.Border);
        SetBrush("BorderStrongBrush", palette.BorderStrong);
        SetBrush("TextPrimaryBrush", palette.TextPrimary);
        SetBrush("TextSecondaryBrush", palette.TextSecondary);
        SetBrush("TextTertiaryBrush", palette.TextTertiary);
        SetBrush("AccentBrush", palette.Accent);
        SetBrush("AccentHoverBrush", palette.AccentHover);
        SetBrush("AccentPressedBrush", palette.AccentPressed);
        SetBrush("AccentSoftBrush", palette.AccentSoft);
        SetBrush("AccentBorderBrush", palette.AccentBorder);
        SetBrush("ControlBrush", palette.Control);
        SetBrush("ControlHoverBrush", palette.ControlHover);
        SetBrush("ControlPressedBrush", palette.ControlPressed);
        SetBrush("RowHoverBrush", palette.RowHover);
        SetBrush("SuccessBrush", palette.Success);
        SetBrush("SuccessSoftBrush", palette.SuccessSoft);
        SetBrush("WarningBrush", palette.Warning);
        SetBrush("WarningSoftBrush", palette.WarningSoft);
        SetBrush("DangerBrush", palette.Danger);
        SetBrush("DangerSoftBrush", palette.DangerSoft);
        SetBrush("DangerBorderBrush", palette.DangerBorder);
        LocalAvailabilityToBrushConverter.SetLightTheme(palette.LightTheme);
        LogLevelToBrushConverter.SetLightTheme(palette.LightTheme);
        StatusKindToBrushConverter.SetLightTheme(palette.LightTheme);
    }

    private static void ApplyThemeVariant(Palette palette)
    {
        if (Application.Current is not { } app)
        {
            return;
        }

        app.RequestedThemeVariant = palette.LightTheme ? ThemeVariant.Light : ThemeVariant.Dark;
        // Fluent 内部控件（勾选框、下拉弹层、滚动条）仍使用系统 accent 资源。
        app.Resources["SystemAccentColor"] = Color.Parse(palette.Accent);
        app.Resources["SystemAccentColorLight1"] = Color.Parse(palette.AccentHover);
        app.Resources["SystemAccentColorLight2"] = Color.Parse(palette.AccentHover);
        app.Resources["SystemAccentColorLight3"] = Color.Parse(palette.AccentHover);
        app.Resources["SystemAccentColorDark1"] = Color.Parse(palette.AccentPressed);
        app.Resources["SystemAccentColorDark2"] = Color.Parse(palette.AccentPressed);
        app.Resources["SystemAccentColorDark3"] = Color.Parse(palette.AccentPressed);
    }

    private const int DwmUseImmersiveDarkMode = 20;
    private const int DwmCaptionColor = 35;
    private const int DwmTextColor = 36;

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);

    /// <summary>让不自定义标题栏的窗口也使用当前主题的颜色，避免 Windows 强调色污染。</summary>
    private static void ApplyTitleBar(Window window, Palette palette)
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

        var darkMode = palette.LightTheme ? 0 : 1;
        DwmSetWindowAttribute(handle, DwmUseImmersiveDarkMode, ref darkMode, sizeof(int));

        var captionColor = ToColorRef(Color.Parse(palette.AppBackground));
        DwmSetWindowAttribute(handle, DwmCaptionColor, ref captionColor, sizeof(int));

        var textColor = ToColorRef(Color.Parse(palette.TextPrimary));
        DwmSetWindowAttribute(handle, DwmTextColor, ref textColor, sizeof(int));
    }

    private static int ToColorRef(Color color) => color.R | (color.G << 8) | (color.B << 16);

    private static void SetBrush(string key, string color)
    {
        if (Application.Current?.Resources[key] is SolidColorBrush brush)
        {
            brush.Color = Color.Parse(color);
        }
    }
}
