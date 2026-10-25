using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;
using NeteaseMusicManager.Core.Models;

namespace NeteaseMusicManager.Desktop.Services;

/// <summary>
/// 应用窗口级外观：
/// 经典 = 实色窗口；
/// 磨砂玻璃 = Mica + 冷色雾面玻璃；
/// 亚克力 = Windows AcrylicBlur；
/// 液态玻璃 = AcrylicBlur + XAML 中的彩色流动背景和高光层。
/// 主题色通过修改 Application.Resources 中的 SolidColorBrush 实例实时传播，
/// 避免重建全部控件或重新加载 XAML。
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
        string AccentSoft,
        string AccentBorder);

    private static readonly Palette Classic = new(
        AppBackground: "#0F0F12",
        Header: "#151519",
        Panel: "#17171B",
        PanelAlt: "#1D1D22",
        PanelHover: "#26262C",
        Input: "#121215",
        Border: "#2A2A31",
        BorderStrong: "#3A3A44",
        TextPrimary: "#F2F2F5",
        TextSecondary: "#A2A2AE",
        TextTertiary: "#70707C",
        AccentSoft: "#331F1F",
        AccentBorder: "#5C2A2A");

    private static readonly Palette Frosted = new(
        AppBackground: "#0A0F17",
        Header: "#B3151B24",
        Panel: "#A3151A23",
        PanelAlt: "#BF1D242F",
        PanelHover: "#D92B3442",
        Input: "#CC10151D",
        Border: "#33FFFFFF",
        BorderStrong: "#59FFFFFF",
        TextPrimary: "#F4F7FB",
        TextSecondary: "#C2CBD7",
        TextTertiary: "#96A1B0",
        AccentSoft: "#33EC4141",
        AccentBorder: "#66FF7A7A");

    private static readonly Palette Acrylic = new(
        AppBackground: "#0B0D11",
        Header: "#BF101217",
        Panel: "#B316191F",
        PanelAlt: "#D91F232B",
        PanelHover: "#E62C313B",
        Input: "#D9111319",
        Border: "#38FFFFFF",
        BorderStrong: "#59FFFFFF",
        TextPrimary: "#F5F6F8",
        TextSecondary: "#C4C8D1",
        TextTertiary: "#9AA0AC",
        AccentSoft: "#33EC4141",
        AccentBorder: "#5CFF7A7A");

    private static readonly Palette Liquid = new(
        AppBackground: "#0A0E19",
        Header: "#B30B1220",
        Panel: "#8C0C1422",
        PanelAlt: "#A6162033",
        PanelHover: "#CC26324A",
        Input: "#B30E1728",
        Border: "#3DFFFFFF",
        BorderStrong: "#66FFFFFF",
        TextPrimary: "#FFFFFF",
        TextSecondary: "#D6DDEB",
        TextTertiary: "#AAB6CA",
        AccentSoft: "#3DFF5C8A",
        AccentBorder: "#66FF8AAA");

    public static void Apply(Window window, AppVisualStyle style)
    {
        try
        {
            ApplyPalette(style switch
            {
                AppVisualStyle.FrostedGlass => Frosted,
                AppVisualStyle.Acrylic => Acrylic,
                AppVisualStyle.LiquidGlass => Liquid,
                _ => Classic,
            });

            if (style == AppVisualStyle.Classic)
            {
                window.TransparencyLevelHint = new[] { WindowTransparencyLevel.None };
                window.Background = new SolidColorBrush(Color.Parse("#0F0F12"));
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
            // 外观效果不能影响主流程；失败时保留当前窗口样式。
            System.Diagnostics.Debug.WriteLine($"Apply visual style failed: {ex}");
        }
    }

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
        SetBrush("AccentSoftBrush", palette.AccentSoft);
        SetBrush("AccentBorderBrush", palette.AccentBorder);
    }

    private static void SetBrush(string key, string color)
    {
        if (Application.Current?.Resources[key] is SolidColorBrush brush)
        {
            brush.Color = Color.Parse(color);
        }
    }
}
