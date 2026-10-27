using Avalonia;
using Avalonia.Media;
using Avalonia.Styling;

namespace LunaticPrincess.Desktop.Services;

/// <summary>
/// 转换器返回的画刷实例必须固定：Avalonia 的 Binding 不会因主题切换重新求值，
/// 因此这里预置可变的 <see cref="SolidColorBrush"/>，主题切换后修改 Color，
/// 使用同一实例的徽标/日志/状态文本会立即按新主题重绘。
/// </summary>
public static class ThemeBrushes
{
    public static readonly SolidColorBrush TextSecondary = new(Color.Parse("#C2CBD7"));
    public static readonly SolidColorBrush Success = new(Color.Parse("#5BDB95"));
    public static readonly SolidColorBrush SuccessSoft = new(Color.Parse("#1E3428"));
    public static readonly SolidColorBrush Warning = new(Color.Parse("#F0B64D"));
    public static readonly SolidColorBrush WarningSoft = new(Color.Parse("#332917"));
    public static readonly SolidColorBrush Danger = new(Color.Parse("#FF7D7D"));
    public static readonly SolidColorBrush DangerSoft = new(Color.Parse("#3B1C20"));

    /// <summary>从当前主题资源里取 *Brush 的颜色刷新缓存实例。</summary>
    public static void Refresh()
    {
        Refresh(TextSecondary, "TextSecondaryBrush", "#C2CBD7");
        Refresh(Success, "SuccessBrush", "#5BDB95");
        Refresh(SuccessSoft, "SuccessSoftBrush", "#1E3428");
        Refresh(Warning, "WarningBrush", "#F0B64D");
        Refresh(WarningSoft, "WarningSoftBrush", "#332917");
        Refresh(Danger, "DangerBrush", "#FF7D7D");
        Refresh(DangerSoft, "DangerSoftBrush", "#3B1C20");
    }

    private static void Refresh(SolidColorBrush brush, string key, string fallback)
    {
        var color = ResolveColor(key, Color.Parse(fallback));
        if (brush.Color != color)
        {
            brush.Color = color;
        }
    }

    private static Color ResolveColor(string key, Color fallback)
    {
        var app = Application.Current;
        if (app is not null && app.TryGetResource(key, app.ActualThemeVariant, out var value) && value is ISolidColorBrush brush)
        {
            return brush.Color;
        }

        return fallback;
    }
}
