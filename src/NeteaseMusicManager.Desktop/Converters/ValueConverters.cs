using System.Globalization;
using Avalonia.Data.Converters;
using Avalonia.Media;
using NeteaseMusicManager.Core.Library;

namespace NeteaseMusicManager.Desktop.Converters;

/// <summary>本地匹配状态 → 徽标颜色。参数 "Background" 取浅色底，"Foreground" 取文字/圆点色。</summary>
public sealed class LocalAvailabilityToBrushConverter : IValueConverter
{
    public string Mode { get; set; } = "Foreground";

    private static readonly SolidColorBrush PresentFg = new(Color.Parse("#3DCC7E"));
    private static readonly SolidColorBrush PresentBg = new(Color.Parse("#14281D"));
    private static readonly SolidColorBrush NcmFg = new(Color.Parse("#E8A33D"));
    private static readonly SolidColorBrush NcmBg = new(Color.Parse("#2A2213"));
    private static readonly SolidColorBrush MissingFg = new(Color.Parse("#F26D6D"));
    private static readonly SolidColorBrush MissingBg = new(Color.Parse("#2C1718"));

    /// <summary>由主题管理器在切换浅/深色主题时更新；所有徽标共享同一组画刷。</summary>
    public static void SetLightTheme(bool light)
    {
        Set(PresentFg, light ? "#168A4C" : "#3DCC7E");
        Set(PresentBg, light ? "#E8F7EF" : "#14281D");
        Set(NcmFg, light ? "#A96500" : "#E8A33D");
        Set(NcmBg, light ? "#FFF5E3" : "#2A2213");
        Set(MissingFg, light ? "#D93636" : "#F26D6D");
        Set(MissingBg, light ? "#FDECEC" : "#2C1718");
    }

    public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        var foreground = string.Equals(Mode, "Foreground", StringComparison.OrdinalIgnoreCase);
        var availability = value is LocalAvailability state ? state : LocalAvailability.Missing;
        return availability switch
        {
            LocalAvailability.Available => foreground ? PresentFg : PresentBg,
            LocalAvailability.EncryptedNcm => foreground ? NcmFg : NcmBg,
            _ => foreground ? MissingFg : MissingBg,
        };
    }

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();

    private static void Set(SolidColorBrush brush, string color) => brush.Color = Color.Parse(color);
}

/// <summary>日志级别 → 文本颜色。</summary>
public sealed class LogLevelToBrushConverter : IValueConverter
{
    private static readonly SolidColorBrush Info = new(Color.Parse("#A2A2AE"));
    private static readonly SolidColorBrush Success = new(Color.Parse("#3DCC7E"));
    private static readonly SolidColorBrush Warning = new(Color.Parse("#E8A33D"));
    private static readonly SolidColorBrush Error = new(Color.Parse("#F26D6D"));

    public static void SetLightTheme(bool light)
    {
        Set(Info, light ? "#8C8C8C" : "#A2A2AE");
        Set(Success, light ? "#168A4C" : "#3DCC7E");
        Set(Warning, light ? "#A96500" : "#E8A33D");
        Set(Error, light ? "#D93636" : "#F26D6D");
    }

    public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        (value as string) switch
        {
            "success" => Success,
            "warning" => Warning,
            "error" => Error,
            _ => Info,
        };

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();

    private static void Set(SolidColorBrush brush, string color) => brush.Color = Color.Parse(color);
}

/// <summary>扫码状态类型 → 状态文本颜色。</summary>
public sealed class StatusKindToBrushConverter : IValueConverter
{
    private static readonly SolidColorBrush Waiting = new(Color.Parse("#A2A2AE"));
    private static readonly SolidColorBrush Scanned = new(Color.Parse("#E8A33D"));
    private static readonly SolidColorBrush Success = new(Color.Parse("#3DCC7E"));
    private static readonly SolidColorBrush Error = new(Color.Parse("#F26D6D"));

    public static void SetLightTheme(bool light)
    {
        Set(Waiting, light ? "#8C8C8C" : "#A2A2AE");
        Set(Scanned, light ? "#A96500" : "#E8A33D");
        Set(Success, light ? "#168A4C" : "#3DCC7E");
        Set(Error, light ? "#D93636" : "#F26D6D");
    }

    public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        (value as string) switch
        {
            "scanned" => Scanned,
            "success" => Success,
            "error" or "expired" => Error,
            _ => Waiting,
        };

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();

    private static void Set(SolidColorBrush brush, string color) => brush.Color = Color.Parse(color);
}

public sealed class InverseBoolConverter : IValueConverter
{
    public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        value is bool flag ? !flag : true;

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        value is bool flag ? !flag : true;
}

public sealed class NotEmptyStringConverter : IValueConverter
{
    public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        !string.IsNullOrWhiteSpace(value as string);

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}
